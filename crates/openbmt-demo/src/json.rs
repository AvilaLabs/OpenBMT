//! The reply text: JSON written by hand, since `openbmt-json` only reads.

/// A JSON string: quote, backslash and control characters escaped, every
/// other character as it is.
pub fn quote(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            c if u32::from(c) < 0x20 => out.push_str(&format!("\\u{:04x}", u32::from(c))),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// An object written field by field, in the order given.
#[derive(Debug, Default)]
pub struct Obj {
    fields: Vec<String>,
}

impl Obj {
    /// An object with no fields.
    pub fn new() -> Self {
        Self::default()
    }

    /// A field whose value is already JSON.
    pub fn raw(mut self, key: &str, json: &str) -> Self {
        self.fields.push(format!("{}:{json}", quote(key)));
        self
    }

    /// A string field.
    pub fn str(self, key: &str, value: &str) -> Self {
        self.raw(key, &quote(value))
    }

    /// An unsigned number field.
    pub fn num(self, key: &str, value: u64) -> Self {
        self.raw(key, &value.to_string())
    }

    /// A true or false field.
    pub fn bool(self, key: &str, value: bool) -> Self {
        self.raw(key, if value { "true" } else { "false" })
    }

    /// A field that is a string, or `null` when there is none.
    pub fn opt_str(self, key: &str, value: Option<&str>) -> Self {
        match value {
            Some(v) => self.str(key, v),
            None => self.raw(key, "null"),
        }
    }

    /// A field that is a number, or `null` when there is none.
    pub fn opt_num(self, key: &str, value: Option<u64>) -> Self {
        match value {
            Some(v) => self.num(key, v),
            None => self.raw(key, "null"),
        }
    }

    /// The object as text.
    pub fn done(self) -> String {
        format!("{{{}}}", self.fields.join(","))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quote_escapes_what_json_needs() {
        assert_eq!(quote("a\"b\\c\nd\u{1}é"), "\"a\\\"b\\\\c\\nd\\u0001é\"");
    }

    #[test]
    fn an_object_reads_back_with_the_repo_parser() {
        let text = Obj::new()
            .str("a", "x\"y")
            .num("b", 7)
            .bool("c", true)
            .opt_str("d", None)
            .raw("e", "[1,2]")
            .done();
        let v = openbmt_json::parse(text.as_bytes(), &openbmt_json::Limits::DEFAULT).unwrap();
        assert_eq!(v.get("a").and_then(|x| x.as_str()), Some("x\"y"));
        assert_eq!(v.get("b").and_then(|x| x.as_u64()), Some(7));
        assert_eq!(v.get("c").and_then(|x| x.as_bool()), Some(true));
        assert!(v.get("d").is_some_and(|x| x.is_null()));
    }
}
