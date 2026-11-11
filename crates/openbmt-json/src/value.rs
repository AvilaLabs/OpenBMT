//! The tree a document is read into.

/// A value of the document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    /// `null`.
    Null,
    /// `true` or `false`.
    Bool(bool),
    /// An integer (no fraction, no exponent, not `-0`) that fits an `i64`.
    Int(i64),
    /// An integer above `i64::MAX` that fits a `u64`.
    UInt(u64),
    /// A number that has the grammar of one and is not an exact integer that
    /// fits: a fraction, an exponent, `-0`, or an integer too large. Its
    /// value is not kept.
    Number,
    /// A string, with its escapes resolved.
    Str(String),
    /// An array.
    Arr(Vec<Value>),
    /// An object, in the order of the document; no key is in it twice.
    Obj(Vec<(String, Value)>),
}

impl Value {
    /// The value of a key, when this is an object that has it.
    pub fn get(&self, key: &str) -> Option<&Value> {
        match self {
            Value::Obj(items) => items.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    /// The text, when this is a string.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Str(s) => Some(s),
            _ => None,
        }
    }

    /// The items, when this is an array.
    pub fn as_array(&self) -> Option<&[Value]> {
        match self {
            Value::Arr(items) => Some(items),
            _ => None,
        }
    }

    /// The members, when this is an object.
    pub fn as_object(&self) -> Option<&[(String, Value)]> {
        match self {
            Value::Obj(items) => Some(items),
            _ => None,
        }
    }

    /// The boolean, when this is `true` or `false`.
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(b) => Some(*b),
            _ => None,
        }
    }

    /// The integer, when this is an exact one that fits an `i64`.
    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Value::Int(i) => Some(*i),
            _ => None,
        }
    }

    /// The integer, when this is an exact one that is not negative.
    pub fn as_u64(&self) -> Option<u64> {
        match self {
            Value::Int(i) => u64::try_from(*i).ok(),
            Value::UInt(u) => Some(*u),
            _ => None,
        }
    }

    /// Whether this is `null`.
    pub fn is_null(&self) -> bool {
        matches!(self, Value::Null)
    }
}
