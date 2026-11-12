//! The reader: the grammar of RFC 8259, checked whole.

use crate::{JsonError, Limits, Value};

/// Read a whole document, within `limits`. The length is checked first, then
/// that the bytes are UTF-8, then the document, then that nothing follows it.
pub fn parse(bytes: &[u8], limits: &Limits) -> Result<Value, JsonError> {
    if bytes.len() > limits.max_bytes {
        return Err(JsonError::TooLong);
    }
    if std::str::from_utf8(bytes).is_err() {
        return Err(JsonError::NotUtf8);
    }
    let mut reader = Reader {
        bytes,
        pos: 0,
        values: 0,
        limits: *limits,
    };
    let value = reader.value(0)?;
    reader.skip_space();
    if reader.pos != bytes.len() {
        return Err(JsonError::TrailingBytes);
    }
    Ok(value)
}

struct Reader<'a> {
    bytes: &'a [u8],
    pos: usize,
    values: usize,
    limits: Limits,
}

impl Reader<'_> {
    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn skip_space(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.pos += 1;
        }
    }

    fn expect(&mut self, byte: u8) -> Result<(), JsonError> {
        if self.peek() == Some(byte) {
            self.pos += 1;
            Ok(())
        } else {
            Err(JsonError::Malformed)
        }
    }

    fn word(&mut self, word: &[u8], value: Value) -> Result<Value, JsonError> {
        let end = self.pos + word.len();
        if self.bytes.get(self.pos..end) == Some(word) {
            self.pos = end;
            Ok(value)
        } else {
            Err(JsonError::Malformed)
        }
    }

    fn value(&mut self, depth: usize) -> Result<Value, JsonError> {
        self.skip_space();
        self.values += 1;
        if self.values > self.limits.max_values {
            return Err(JsonError::TooManyValues);
        }
        match self.peek() {
            Some(b'{') => self.object(depth + 1),
            Some(b'[') => self.array(depth + 1),
            Some(b'"') => self.string().map(Value::Str),
            Some(b't') => self.word(b"true", Value::Bool(true)),
            Some(b'f') => self.word(b"false", Value::Bool(false)),
            Some(b'n') => self.word(b"null", Value::Null),
            Some(b'-' | b'0'..=b'9') => self.number(),
            _ => Err(JsonError::Malformed),
        }
    }

    fn array(&mut self, depth: usize) -> Result<Value, JsonError> {
        if depth > self.limits.max_depth {
            return Err(JsonError::TooDeep);
        }
        self.expect(b'[')?;
        let mut items = Vec::new();
        self.skip_space();
        if self.peek() == Some(b']') {
            self.pos += 1;
            return Ok(Value::Arr(items));
        }
        loop {
            items.push(self.value(depth)?);
            self.skip_space();
            match self.peek() {
                Some(b',') => self.pos += 1,
                Some(b']') => {
                    self.pos += 1;
                    return Ok(Value::Arr(items));
                }
                _ => return Err(JsonError::Malformed),
            }
        }
    }

    fn object(&mut self, depth: usize) -> Result<Value, JsonError> {
        if depth > self.limits.max_depth {
            return Err(JsonError::TooDeep);
        }
        self.expect(b'{')?;
        let mut items: Vec<(String, Value)> = Vec::new();
        self.skip_space();
        if self.peek() == Some(b'}') {
            self.pos += 1;
            return Ok(Value::Obj(items));
        }
        loop {
            self.skip_space();
            let key = self.string()?;
            self.skip_space();
            self.expect(b':')?;
            let value = self.value(depth)?;
            items.push((key, value));
            self.skip_space();
            match self.peek() {
                Some(b',') => self.pos += 1,
                Some(b'}') => {
                    self.pos += 1;
                    break;
                }
                _ => return Err(JsonError::Malformed),
            }
        }
        // A key twice in one object, however it was spelled.
        let mut keys: Vec<&str> = items.iter().map(|(k, _)| k.as_str()).collect();
        keys.sort_unstable();
        if keys.windows(2).any(|w| w.first() == w.get(1)) {
            return Err(JsonError::DuplicateKey);
        }
        Ok(Value::Obj(items))
    }

    fn hex4(&mut self) -> Result<u32, JsonError> {
        let end = self.pos + 4;
        let digits = self.bytes.get(self.pos..end).ok_or(JsonError::Malformed)?;
        let mut v = 0u32;
        for d in digits {
            let n = char::from(*d).to_digit(16).ok_or(JsonError::Malformed)?;
            v = v * 16 + n;
        }
        self.pos = end;
        Ok(v)
    }

    fn string(&mut self) -> Result<String, JsonError> {
        self.expect(b'"')?;
        let mut out: Vec<u8> = Vec::new();
        loop {
            let b = self.peek().ok_or(JsonError::Malformed)?;
            self.pos += 1;
            match b {
                b'"' => break,
                0x00..=0x1f => return Err(JsonError::Malformed),
                b'\\' => {
                    let e = self.peek().ok_or(JsonError::Malformed)?;
                    self.pos += 1;
                    let c = match e {
                        b'"' => '"',
                        b'\\' => '\\',
                        b'/' => '/',
                        b'b' => '\u{8}',
                        b'f' => '\u{c}',
                        b'n' => '\n',
                        b'r' => '\r',
                        b't' => '\t',
                        b'u' => self.unicode()?,
                        _ => return Err(JsonError::Malformed),
                    };
                    let mut buf = [0u8; 4];
                    out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
                }
                _ => out.push(b),
            }
        }
        String::from_utf8(out).map_err(|_| JsonError::NotUtf8)
    }

    /// The character of a `\u` escape, with the four digits still to read;
    /// a high surrogate must be followed by a low one, and a low one alone
    /// is refused.
    fn unicode(&mut self) -> Result<char, JsonError> {
        let first = self.hex4()?;
        if (0xd800..0xdc00).contains(&first) {
            self.expect(b'\\')?;
            self.expect(b'u')?;
            let second = self.hex4()?;
            if !(0xdc00..0xe000).contains(&second) {
                return Err(JsonError::Malformed);
            }
            let c = 0x10000 + ((first - 0xd800) << 10) + (second - 0xdc00);
            return char::from_u32(c).ok_or(JsonError::Malformed);
        }
        char::from_u32(first).ok_or(JsonError::Malformed)
    }

    fn digits(&mut self) -> usize {
        let start = self.pos;
        while matches!(self.peek(), Some(b'0'..=b'9')) {
            self.pos += 1;
        }
        self.pos - start
    }

    /// The JSON number grammar. An integer that fits is kept as a value; any
    /// other number is checked and kept as its exact text.
    fn number(&mut self) -> Result<Value, JsonError> {
        let start = self.pos;
        let mut integer = true;
        if self.peek() == Some(b'-') {
            self.pos += 1;
        }
        match self.peek() {
            Some(b'0') => self.pos += 1,
            Some(b'1'..=b'9') => {
                self.digits();
            }
            _ => return Err(JsonError::Malformed),
        }
        if self.peek() == Some(b'.') {
            integer = false;
            self.pos += 1;
            if self.digits() == 0 {
                return Err(JsonError::Malformed);
            }
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            integer = false;
            self.pos += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.pos += 1;
            }
            if self.digits() == 0 {
                return Err(JsonError::Malformed);
            }
        }
        let text = self
            .bytes
            .get(start..self.pos)
            .and_then(|b| std::str::from_utf8(b).ok())
            .ok_or(JsonError::Malformed)?;
        if !integer || text == "-0" {
            return Ok(Value::Number(text.to_string()));
        }
        if let Ok(i) = text.parse::<i64>() {
            Ok(Value::Int(i))
        } else if let Ok(u) = text.parse::<u64>() {
            Ok(Value::UInt(u))
        } else {
            Ok(Value::Number(text.to_string()))
        }
    }
}
