//! A strict reader for the JSON object that Coldcard-style wallets put in
//! a `bytes` code (UR-29): one object whose values are read as strings or
//! integers; nested objects and arrays are walked and dropped. UTF-8, no
//! duplicate key at any depth, nothing after the object but white space.

/// The most nested objects and arrays, the outer object counted.
const MAX_DEPTH: usize = 4;
/// The most values in one message, at every depth.
const MAX_VALUES: usize = 1024;

/// A value of the outer object.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Value {
    Text(String),
    /// An integer, a boolean, a null or a nested value: present, but not
    /// read.
    Other,
}

/// The entries of the outer object, in order, each key once.
pub(crate) fn read_object(bytes: &[u8]) -> Option<Vec<(String, Value)>> {
    let text = std::str::from_utf8(bytes).ok()?;
    let mut r = Reader {
        rest: text,
        values: 0,
    };
    r.space();
    let entries = r.object(1, true)?;
    r.space();
    if r.rest.is_empty() {
        Some(entries)
    } else {
        None
    }
}

struct Reader<'a> {
    rest: &'a str,
    values: usize,
}

impl Reader<'_> {
    fn space(&mut self) {
        self.rest = self.rest.trim_start_matches([' ', '\t', '\n', '\r']);
    }

    fn eat(&mut self, c: char) -> bool {
        match self.rest.strip_prefix(c) {
            Some(rest) => {
                self.rest = rest;
                true
            }
            None => false,
        }
    }

    fn literal(&mut self, word: &str) -> bool {
        match self.rest.strip_prefix(word) {
            Some(rest) => {
                self.rest = rest;
                true
            }
            None => false,
        }
    }

    /// An object; its entries are kept only for the outer one.
    fn object(&mut self, depth: usize, keep: bool) -> Option<Vec<(String, Value)>> {
        if depth > MAX_DEPTH || !self.eat('{') {
            return None;
        }
        let mut entries: Vec<(String, Value)> = Vec::new();
        let mut seen: Vec<String> = Vec::new();
        self.space();
        if self.eat('}') {
            return Some(entries);
        }
        loop {
            self.space();
            let key = self.string()?;
            if seen.contains(&key) {
                return None;
            }
            self.space();
            if !self.eat(':') {
                return None;
            }
            self.space();
            let value = self.value(depth)?;
            if keep {
                entries.push((key.clone(), value));
            }
            seen.push(key);
            self.space();
            if self.eat(',') {
                continue;
            }
            if self.eat('}') {
                return Some(entries);
            }
            return None;
        }
    }

    fn array(&mut self, depth: usize) -> Option<()> {
        if depth > MAX_DEPTH || !self.eat('[') {
            return None;
        }
        self.space();
        if self.eat(']') {
            return Some(());
        }
        loop {
            self.space();
            self.value(depth)?;
            self.space();
            if self.eat(',') {
                continue;
            }
            return self.eat(']').then_some(());
        }
    }

    /// A value inside an object or array at `depth`.
    fn value(&mut self, depth: usize) -> Option<Value> {
        self.values += 1;
        if self.values > MAX_VALUES {
            return None;
        }
        match self.rest.chars().next()? {
            '"' => self.string().map(Value::Text),
            '{' => self.object(depth + 1, false).map(|_| Value::Other),
            '[' => self.array(depth + 1).map(|()| Value::Other),
            't' => self.literal("true").then_some(Value::Other),
            'f' => self.literal("false").then_some(Value::Other),
            'n' => self.literal("null").then_some(Value::Other),
            _ => self.integer().map(|()| Value::Other),
        }
    }

    /// `-?(0|[1-9][0-9]*)`: no fraction, no exponent.
    fn integer(&mut self) -> Option<()> {
        let digits = self.rest.strip_prefix('-').unwrap_or(self.rest);
        let n = digits.bytes().take_while(u8::is_ascii_digit).count();
        if n == 0 || (n > 1 && digits.starts_with('0')) || n > 18 {
            return None;
        }
        let rest = digits.get(n..)?;
        if rest.starts_with(['.', 'e', 'E']) {
            return None;
        }
        self.rest = rest;
        Some(())
    }

    fn string(&mut self) -> Option<String> {
        if !self.eat('"') {
            return None;
        }
        let mut out = String::new();
        loop {
            let c = self.rest.chars().next()?;
            self.rest = self.rest.get(c.len_utf8()..)?;
            match c {
                '"' => return Some(out),
                '\\' => {
                    let e = self.rest.chars().next()?;
                    self.rest = self.rest.get(e.len_utf8()..)?;
                    match e {
                        '"' | '\\' | '/' => out.push(e),
                        'b' => out.push('\u{8}'),
                        'f' => out.push('\u{c}'),
                        'n' => out.push('\n'),
                        'r' => out.push('\r'),
                        't' => out.push('\t'),
                        'u' => out.push(self.unicode()?),
                        _ => return None,
                    }
                }
                c if c < '\u{20}' => return None,
                c => out.push(c),
            }
            if out.len() > 4096 {
                return None;
            }
        }
    }

    fn hex4(&mut self) -> Option<u32> {
        let digits = self.rest.get(..4)?;
        if !digits.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        self.rest = self.rest.get(4..)?;
        u32::from_str_radix(digits, 16).ok()
    }

    /// The character of a `\u` escape, with a surrogate pair joined; a
    /// lone surrogate is refused.
    fn unicode(&mut self) -> Option<char> {
        let first = self.hex4()?;
        if (0xD800..0xDC00).contains(&first) {
            if !self.literal("\\u") {
                return None;
            }
            let second = self.hex4()?;
            if !(0xDC00..0xE000).contains(&second) {
                return None;
            }
            return char::from_u32(0x10000 + ((first - 0xD800) << 10) + (second - 0xDC00));
        }
        char::from_u32(first)
    }
}
