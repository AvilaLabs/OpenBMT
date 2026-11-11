//! A strict, small JSON reader (RT-10 to RT-14).
//!
//! It reads the whole document so that a malformed one is refused, keeps the
//! values in a tree, and the code that follows looks up the few fields it
//! wants. Input is bounded before it is read, and so is the depth, so the
//! work and the memory are bounded by [`MAX_RESPONSE_BYTES`].

use crate::error::ParseRefused;

/// RT-10: the most bytes of a reply that are read.
pub const MAX_RESPONSE_BYTES: usize = 64 * 1024;
/// RT-11: the most levels of arrays and objects inside one another.
pub const MAX_DEPTH: usize = 16;

/// A value of the document. Numbers, true, false and null are checked and
/// then not kept: no field the rules read is one of them.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Value {
    /// A number, true, false or null.
    Other,
    /// A string, with its escapes resolved.
    Str(String),
    /// An array.
    Arr(Vec<Value>),
    /// An object; no key is in it twice (RT-14).
    Obj(Vec<(String, Value)>),
}

impl Value {
    /// The value of a key, when this is an object that has it.
    pub(crate) fn get(&self, key: &str) -> Option<&Value> {
        match self {
            Value::Obj(items) => items.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    /// The text, when this is a string.
    pub(crate) fn as_str(&self) -> Option<&str> {
        match self {
            Value::Str(s) => Some(s),
            _ => None,
        }
    }
}

/// Reads a whole document: RT-10 to RT-14.
pub(crate) fn parse_document(bytes: &[u8]) -> Result<Value, ParseRefused> {
    if bytes.len() > MAX_RESPONSE_BYTES {
        return Err(ParseRefused::TooLong);
    }
    if std::str::from_utf8(bytes).is_err() {
        return Err(ParseRefused::NotUtf8);
    }
    let mut reader = Reader { bytes, pos: 0 };
    let value = reader.value(0)?;
    reader.skip_space();
    if reader.pos != bytes.len() {
        return Err(ParseRefused::TrailingBytes);
    }
    Ok(value)
}

struct Reader<'a> {
    bytes: &'a [u8],
    pos: usize,
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

    fn expect(&mut self, byte: u8) -> Result<(), ParseRefused> {
        if self.peek() == Some(byte) {
            self.pos += 1;
            Ok(())
        } else {
            Err(ParseRefused::Malformed)
        }
    }

    fn word(&mut self, word: &[u8]) -> Result<Value, ParseRefused> {
        let end = self.pos + word.len();
        if self.bytes.get(self.pos..end) == Some(word) {
            self.pos = end;
            Ok(Value::Other)
        } else {
            Err(ParseRefused::Malformed)
        }
    }

    fn value(&mut self, depth: usize) -> Result<Value, ParseRefused> {
        self.skip_space();
        match self.peek() {
            Some(b'{') => self.object(depth + 1),
            Some(b'[') => self.array(depth + 1),
            Some(b'"') => self.string().map(Value::Str),
            Some(b't') => self.word(b"true"),
            Some(b'f') => self.word(b"false"),
            Some(b'n') => self.word(b"null"),
            Some(b'-' | b'0'..=b'9') => self.number(),
            _ => Err(ParseRefused::Malformed),
        }
    }

    fn array(&mut self, depth: usize) -> Result<Value, ParseRefused> {
        if depth > MAX_DEPTH {
            return Err(ParseRefused::TooDeep);
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
                _ => return Err(ParseRefused::Malformed),
            }
        }
    }

    fn object(&mut self, depth: usize) -> Result<Value, ParseRefused> {
        if depth > MAX_DEPTH {
            return Err(ParseRefused::TooDeep);
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
                _ => return Err(ParseRefused::Malformed),
            }
        }
        // RT-14: a key twice in one object, however it was spelled.
        let mut keys: Vec<&str> = items.iter().map(|(k, _)| k.as_str()).collect();
        keys.sort_unstable();
        if keys.windows(2).any(|w| w.first() == w.get(1)) {
            return Err(ParseRefused::DuplicateKey);
        }
        Ok(Value::Obj(items))
    }

    fn hex4(&mut self) -> Result<u32, ParseRefused> {
        let end = self.pos + 4;
        let digits = self
            .bytes
            .get(self.pos..end)
            .ok_or(ParseRefused::Malformed)?;
        let mut v = 0u32;
        for d in digits {
            let n = char::from(*d).to_digit(16).ok_or(ParseRefused::Malformed)?;
            v = v * 16 + n;
        }
        self.pos = end;
        Ok(v)
    }

    fn string(&mut self) -> Result<String, ParseRefused> {
        self.expect(b'"')?;
        let mut out: Vec<u8> = Vec::new();
        loop {
            let b = self.peek().ok_or(ParseRefused::Malformed)?;
            self.pos += 1;
            match b {
                b'"' => break,
                0x00..=0x1f => return Err(ParseRefused::Malformed),
                b'\\' => {
                    let e = self.peek().ok_or(ParseRefused::Malformed)?;
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
                        _ => return Err(ParseRefused::Malformed),
                    };
                    let mut buf = [0u8; 4];
                    out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
                }
                _ => out.push(b),
            }
        }
        String::from_utf8(out).map_err(|_| ParseRefused::NotUtf8)
    }

    /// The character of a `\u` escape, with the four digits still to read;
    /// a high surrogate must be followed by a low one, and a low one alone
    /// is refused.
    fn unicode(&mut self) -> Result<char, ParseRefused> {
        let first = self.hex4()?;
        if (0xd800..0xdc00).contains(&first) {
            self.expect(b'\\')?;
            self.expect(b'u')?;
            let second = self.hex4()?;
            if !(0xdc00..0xe000).contains(&second) {
                return Err(ParseRefused::Malformed);
            }
            let c = 0x10000 + ((first - 0xd800) << 10) + (second - 0xdc00);
            return char::from_u32(c).ok_or(ParseRefused::Malformed);
        }
        char::from_u32(first).ok_or(ParseRefused::Malformed)
    }

    fn digits(&mut self) -> usize {
        let start = self.pos;
        while matches!(self.peek(), Some(b'0'..=b'9')) {
            self.pos += 1;
        }
        self.pos - start
    }

    /// The JSON number grammar; the value is not kept.
    fn number(&mut self) -> Result<Value, ParseRefused> {
        if self.peek() == Some(b'-') {
            self.pos += 1;
        }
        match self.peek() {
            Some(b'0') => self.pos += 1,
            Some(b'1'..=b'9') => {
                self.digits();
            }
            _ => return Err(ParseRefused::Malformed),
        }
        if self.peek() == Some(b'.') {
            self.pos += 1;
            if self.digits() == 0 {
                return Err(ParseRefused::Malformed);
            }
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.pos += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.pos += 1;
            }
            if self.digits() == 0 {
                return Err(ParseRefused::Malformed);
            }
        }
        Ok(Value::Other)
    }
}
