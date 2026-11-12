//! Small, bounded JSON reader for Bitcoin Core `listdescriptors` exports.
//! No serde dependency is used in core.

#[derive(Debug)]
pub(super) enum Value {
    Object(Vec<(String, Value)>),
    Array(Vec<Value>),
    String(String),
    Bool(bool),
    Null,
    Number,
}

const MAX_DEPTH: usize = 16;

pub(super) fn parse(input: &str) -> Result<Value, ()> {
    if input.len() > 4096 {
        return Err(());
    }
    let mut p = Parser { input, at: 0 };
    p.ws();
    let value = p.value(0)?;
    p.ws();
    if p.at != input.len() {
        return Err(());
    }
    Ok(value)
}

pub(super) fn strings<'a>(value: &'a Value, out: &mut Vec<&'a str>) {
    match value {
        Value::Object(members) => {
            for (key, value) in members {
                out.push(key);
                strings(value, out);
            }
        }
        Value::Array(values) => values.iter().for_each(|value| strings(value, out)),
        Value::String(s) => out.push(s),
        Value::Bool(_) | Value::Null | Value::Number => {}
    }
}

fn member<'a>(object: &'a [(String, Value)], key: &str) -> Option<&'a Value> {
    object.iter().find(|(name, _)| name == key).map(|(_, v)| v)
}

pub(super) fn listdescriptors(value: &Value) -> Result<Vec<Entry<'_>>, ()> {
    let Value::Object(root) = value else {
        return Err(());
    };
    let Value::Array(descriptors) = member(root, "descriptors").ok_or(())? else {
        return Err(());
    };
    descriptors
        .iter()
        .map(|value| {
            let Value::Object(object) = value else {
                return Err(());
            };
            let Value::String(desc) = member(object, "desc").ok_or(())? else {
                return Err(());
            };
            let Value::Bool(active) = member(object, "active").ok_or(())? else {
                return Err(());
            };
            let internal = match member(object, "internal") {
                None => None,
                Some(Value::Bool(b)) => Some(*b),
                _ => return Err(()),
            };
            Ok(Entry {
                desc,
                active: *active,
                internal,
            })
        })
        .collect()
}

pub(super) struct Entry<'a> {
    pub desc: &'a str,
    pub active: bool,
    pub internal: Option<bool>,
}

struct Parser<'a> {
    input: &'a str,
    at: usize,
}

impl Parser<'_> {
    fn ws(&mut self) {
        while self
            .input
            .as_bytes()
            .get(self.at)
            .is_some_and(|b| matches!(b, b' ' | b'\t' | b'\r' | b'\n'))
        {
            self.at += 1;
        }
    }

    fn value(&mut self, depth: usize) -> Result<Value, ()> {
        if depth > MAX_DEPTH {
            return Err(());
        }
        self.ws();
        match self.input.as_bytes().get(self.at).copied().ok_or(())? {
            b'{' if depth < MAX_DEPTH => self.object(depth + 1),
            b'[' if depth < MAX_DEPTH => self.array(depth + 1),
            b'"' => self.string().map(Value::String),
            b't' if self.literal("true") => Ok(Value::Bool(true)),
            b'f' if self.literal("false") => Ok(Value::Bool(false)),
            b'n' if self.literal("null") => Ok(Value::Null),
            b'-' | b'0'..=b'9' => {
                self.number()?;
                Ok(Value::Number)
            }
            _ => Err(()),
        }
    }

    fn object(&mut self, depth: usize) -> Result<Value, ()> {
        self.at += 1;
        self.ws();
        let mut members: Vec<(String, Value)> = Vec::new();
        if self.take(b'}') {
            return Ok(Value::Object(members));
        }
        loop {
            self.ws();
            if self.input.as_bytes().get(self.at) != Some(&b'"') {
                return Err(());
            }
            let key = self.string()?;
            if members.iter().any(|(seen, _)| seen == &key) {
                return Err(());
            }
            self.ws();
            if !self.take(b':') {
                return Err(());
            }
            let value = self.value(depth)?;
            members.push((key, value));
            self.ws();
            if self.take(b'}') {
                break;
            }
            if !self.take(b',') {
                return Err(());
            }
        }
        Ok(Value::Object(members))
    }

    fn array(&mut self, depth: usize) -> Result<Value, ()> {
        self.at += 1;
        self.ws();
        let mut values = Vec::new();
        if self.take(b']') {
            return Ok(Value::Array(values));
        }
        loop {
            values.push(self.value(depth)?);
            self.ws();
            if self.take(b']') {
                break;
            }
            if !self.take(b',') {
                return Err(());
            }
        }
        Ok(Value::Array(values))
    }

    fn string(&mut self) -> Result<String, ()> {
        if !self.take(b'"') {
            return Err(());
        }
        let mut out = Vec::new();
        loop {
            let b = *self.input.as_bytes().get(self.at).ok_or(())?;
            self.at += 1;
            match b {
                b'"' => return String::from_utf8(out).map_err(|_| ()),
                0..=0x1f => return Err(()),
                b'\\' => {
                    let escape = *self.input.as_bytes().get(self.at).ok_or(())?;
                    self.at += 1;
                    match escape {
                        b'"' | b'\\' | b'/' => out.push(escape),
                        b'b' => out.push(8),
                        b'f' => out.push(12),
                        b'n' => out.push(b'\n'),
                        b'r' => out.push(b'\r'),
                        b't' => out.push(b'\t'),
                        b'u' => {
                            let first = self.hex4()?;
                            let scalar = if (0xd800..=0xdbff).contains(&first) {
                                if self.input.as_bytes().get(self.at..self.at + 2) != Some(b"\\u") {
                                    return Err(());
                                }
                                self.at += 2;
                                let second = self.hex4()?;
                                if !(0xdc00..=0xdfff).contains(&second) {
                                    return Err(());
                                }
                                0x10000
                                    + (((first - 0xd800) as u32) << 10)
                                    + (second - 0xdc00) as u32
                            } else if (0xdc00..=0xdfff).contains(&first) {
                                return Err(());
                            } else {
                                first as u32
                            };
                            let ch = char::from_u32(scalar).ok_or(())?;
                            let mut buf = [0; 4];
                            out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
                        }
                        _ => return Err(()),
                    }
                }
                _ => out.push(b),
            }
        }
    }

    fn hex4(&mut self) -> Result<u16, ()> {
        let bytes = self.input.as_bytes().get(self.at..self.at + 4).ok_or(())?;
        let mut n = 0u16;
        for b in bytes {
            n = n * 16
                + match b {
                    b'0'..=b'9' => (b - b'0') as u16,
                    b'a'..=b'f' => (b - b'a' + 10) as u16,
                    b'A'..=b'F' => (b - b'A' + 10) as u16,
                    _ => return Err(()),
                };
        }
        self.at += 4;
        Ok(n)
    }

    fn number(&mut self) -> Result<(), ()> {
        let bytes = self.input.as_bytes();
        if self.take(b'-') && self.at == bytes.len() {
            return Err(());
        }
        if self.take(b'0') {
            if bytes.get(self.at).is_some_and(u8::is_ascii_digit) {
                return Err(());
            }
        } else {
            if !bytes
                .get(self.at)
                .is_some_and(|b| (b'1'..=b'9').contains(b))
            {
                return Err(());
            }
            self.at += 1;
            while bytes.get(self.at).is_some_and(u8::is_ascii_digit) {
                self.at += 1;
            }
        }
        if self.take(b'.') {
            if !bytes.get(self.at).is_some_and(u8::is_ascii_digit) {
                return Err(());
            }
            while bytes.get(self.at).is_some_and(u8::is_ascii_digit) {
                self.at += 1;
            }
        }
        if bytes.get(self.at).is_some_and(|b| matches!(b, b'e' | b'E')) {
            self.at += 1;
            if bytes.get(self.at).is_some_and(|b| matches!(b, b'+' | b'-')) {
                self.at += 1;
            }
            if !bytes.get(self.at).is_some_and(u8::is_ascii_digit) {
                return Err(());
            }
            while bytes.get(self.at).is_some_and(u8::is_ascii_digit) {
                self.at += 1;
            }
        }
        Ok(())
    }

    fn literal(&mut self, literal: &str) -> bool {
        if self.input.as_bytes().get(self.at..self.at + literal.len()) == Some(literal.as_bytes()) {
            self.at += literal.len();
            true
        } else {
            false
        }
    }

    fn take(&mut self, byte: u8) -> bool {
        if self.input.as_bytes().get(self.at) == Some(&byte) {
            self.at += 1;
            true
        } else {
            false
        }
    }
}
