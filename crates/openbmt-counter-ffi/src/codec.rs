//! The field encodings of the call protocol (`crate::call`).
//!
//! Integers are little-endian, a bool is one byte 0 or 1, bytes and strings
//! are a `u32` length and then the content, an option is a flag byte 0 or 1
//! and then the value, a list is a `u32` count and then the elements, an
//! enum is a tag byte (and then the fields of its variant). Decoding is
//! strict: a short buffer, a flag or tag that is not defined, text that is
//! not UTF-8 and a length that is more than the bytes left are refused, and
//! so are bytes left over after the last field.
//!
//! [`Put`] writes a value and [`Get`] reads it; [`Shaped`] and [`Define`]
//! describe it, so that the protocol description in
//! `bindings/PROTOCOL.md` is generated from the same code that encodes.

use crate::error::{refused_by, CounterError};

/// The most bytes a request may hold.
pub(crate) const MAX_REQUEST: usize = 1 << 20;

fn malformed(text: impl std::fmt::Display) -> CounterError {
    refused_by("request-malformed", text)
}

/// A strict cursor over the bytes of a request.
pub(crate) struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    pub(crate) fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    pub(crate) fn remaining(&self) -> usize {
        self.data.len().saturating_sub(self.pos)
    }

    pub(crate) fn take(&mut self, n: usize) -> Result<&'a [u8], CounterError> {
        let end = self
            .pos
            .checked_add(n)
            .ok_or_else(|| malformed("a length is out of range"))?;
        let bytes = self
            .data
            .get(self.pos..end)
            .ok_or_else(|| malformed("the request ends before its last field"))?;
        self.pos = end;
        Ok(bytes)
    }

    pub(crate) fn u8(&mut self) -> Result<u8, CounterError> {
        self.take(1)?
            .first()
            .copied()
            .ok_or_else(|| malformed("the request ends before its last field"))
    }

    pub(crate) fn u16(&mut self) -> Result<u16, CounterError> {
        let b: [u8; 2] = self
            .take(2)?
            .try_into()
            .map_err(|_| malformed("the request ends before its last field"))?;
        Ok(u16::from_le_bytes(b))
    }

    pub(crate) fn u32(&mut self) -> Result<u32, CounterError> {
        let b: [u8; 4] = self
            .take(4)?
            .try_into()
            .map_err(|_| malformed("the request ends before its last field"))?;
        Ok(u32::from_le_bytes(b))
    }

    pub(crate) fn u64(&mut self) -> Result<u64, CounterError> {
        let b: [u8; 8] = self
            .take(8)?
            .try_into()
            .map_err(|_| malformed("the request ends before its last field"))?;
        Ok(u64::from_le_bytes(b))
    }

    /// A length or count that cannot be more than the bytes left: every
    /// element takes at least one byte.
    pub(crate) fn count(&mut self) -> Result<usize, CounterError> {
        let n = usize::try_from(self.u32()?).map_err(|_| malformed("a length is out of range"))?;
        if n > self.remaining() {
            return Err(malformed("a length is more than the bytes that are left"));
        }
        Ok(n)
    }

    /// The request must end here.
    pub(crate) fn end(&self) -> Result<(), CounterError> {
        if self.remaining() == 0 {
            Ok(())
        } else {
            Err(malformed("bytes are left over after the last field"))
        }
    }
}

/// The error for a tag byte that names no variant.
pub(crate) fn bad_tag(what: &str, tag: u8) -> CounterError {
    malformed(format!("{tag} is not a tag of {what}"))
}

/// What a type is called in the protocol description.
pub(crate) trait Shaped {
    fn shape() -> String;
}

/// A value that is written to a response (or, in tests, a request).
pub(crate) trait Put: Shaped {
    fn put(&self, out: &mut Vec<u8>);
}

/// A value that is read from a request.
pub(crate) trait Get: Shaped + Sized {
    fn get(r: &mut Reader<'_>) -> Result<Self, CounterError>;
}

/// A named type of the protocol, for the generated description.
pub(crate) struct TypeDef {
    pub name: &'static str,
    /// The layout, in one line.
    pub body: String,
    /// For an enum: the variants and their tag bytes.
    pub tags: Vec<(&'static str, u8)>,
}

/// A named type that describes itself.
pub(crate) trait Define {
    fn define() -> TypeDef;
}

/// The shape of the field that `_field` reads.
pub(crate) fn field_shape<S, T: Shaped>(_field: fn(&S) -> &T) -> String {
    T::shape()
}

macro_rules! primitive {
    ($t:ty, $name:expr, $read:ident) => {
        impl Shaped for $t {
            fn shape() -> String {
                $name.to_string()
            }
        }
        impl Put for $t {
            fn put(&self, out: &mut Vec<u8>) {
                out.extend_from_slice(&self.to_le_bytes());
            }
        }
        impl Get for $t {
            fn get(r: &mut Reader<'_>) -> Result<Self, CounterError> {
                r.$read()
            }
        }
    };
}

primitive!(u8, "u8", u8);
primitive!(u32, "u32", u32);
primitive!(u64, "u64", u64);

impl Shaped for bool {
    fn shape() -> String {
        "bool".to_string()
    }
}

impl Put for bool {
    fn put(&self, out: &mut Vec<u8>) {
        out.push(u8::from(*self));
    }
}

impl Get for bool {
    fn get(r: &mut Reader<'_>) -> Result<Self, CounterError> {
        match r.u8()? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(malformed("a bool is not 0 or 1")),
        }
    }
}

impl Shaped for () {
    fn shape() -> String {
        "()".to_string()
    }
}

impl Put for () {
    fn put(&self, _out: &mut Vec<u8>) {}
}

impl Get for () {
    fn get(_r: &mut Reader<'_>) -> Result<Self, CounterError> {
        Ok(())
    }
}

impl Shaped for String {
    fn shape() -> String {
        "string".to_string()
    }
}

/// A length prefix for a field that is at most 4 GiB; a longer one cannot
/// be written, and is cut to what the prefix says.
fn put_len(out: &mut Vec<u8>, n: usize) -> usize {
    let len = u32::try_from(n).unwrap_or(u32::MAX);
    out.extend_from_slice(&len.to_le_bytes());
    usize::try_from(len).unwrap_or(n)
}

impl Put for String {
    fn put(&self, out: &mut Vec<u8>) {
        let n = put_len(out, self.len());
        out.extend_from_slice(self.as_bytes().get(..n).unwrap_or(self.as_bytes()));
    }
}

impl Get for String {
    fn get(r: &mut Reader<'_>) -> Result<Self, CounterError> {
        let n = r.count()?;
        let bytes = r.take(n)?;
        String::from_utf8(bytes.to_vec()).map_err(|_| malformed("a string is not UTF-8"))
    }
}

impl<T: Shaped> Shaped for Option<T> {
    fn shape() -> String {
        format!("option<{}>", T::shape())
    }
}

impl<T: Put> Put for Option<T> {
    fn put(&self, out: &mut Vec<u8>) {
        match self {
            None => out.push(0),
            Some(v) => {
                out.push(1);
                v.put(out);
            }
        }
    }
}

impl<T: Get> Get for Option<T> {
    fn get(r: &mut Reader<'_>) -> Result<Self, CounterError> {
        match r.u8()? {
            0 => Ok(None),
            1 => Ok(Some(T::get(r)?)),
            _ => Err(malformed("an option flag is not 0 or 1")),
        }
    }
}

/// A list. A list of `u8` is the bytes type: the count, then the bytes.
impl<T: Shaped> Shaped for Vec<T> {
    fn shape() -> String {
        format!("list<{}>", T::shape())
    }
}

impl<T: Put> Put for Vec<T> {
    fn put(&self, out: &mut Vec<u8>) {
        let n = put_len(out, self.len());
        self.iter().take(n).for_each(|v| v.put(out));
    }
}

impl<T: Get> Get for Vec<T> {
    fn get(r: &mut Reader<'_>) -> Result<Self, CounterError> {
        let n = r.count()?;
        let mut v = Vec::with_capacity(n.min(1_024));
        for _ in 0..n {
            v.push(T::get(r)?);
        }
        Ok(v)
    }
}

impl<A: Shaped, B: Shaped> Shaped for (A, B) {
    fn shape() -> String {
        format!("({}, {})", A::shape(), B::shape())
    }
}

impl<A: Get, B: Get> Get for (A, B) {
    fn get(r: &mut Reader<'_>) -> Result<Self, CounterError> {
        let a = A::get(r)?;
        let b = B::get(r)?;
        Ok((a, b))
    }
}

/// An enum without fields: one tag byte.
macro_rules! wire_unit_enum {
    ($ty:ident { $($var:ident = $tag:literal),+ $(,)? }) => {
        impl $crate::codec::Shaped for $ty {
            fn shape() -> String {
                stringify!($ty).to_string()
            }
        }
        impl $crate::codec::Put for $ty {
            fn put(&self, out: &mut Vec<u8>) {
                out.push(match self { $(Self::$var => $tag),+ });
            }
        }
        impl $crate::codec::Get for $ty {
            fn get(r: &mut $crate::codec::Reader<'_>) -> Result<Self, $crate::CounterError> {
                match r.u8()? {
                    $($tag => Ok(Self::$var),)+
                    t => Err($crate::codec::bad_tag(stringify!($ty), t)),
                }
            }
        }
        impl $crate::codec::Define for $ty {
            fn define() -> $crate::codec::TypeDef {
                $crate::codec::TypeDef {
                    name: stringify!($ty),
                    body: "enum, one tag byte".to_string(),
                    tags: vec![$((stringify!($var), $tag)),+],
                }
            }
        }
    };
}

/// A record: its fields one after the other, in the order given.
macro_rules! wire_record {
    ($ty:ident { $($f:ident),+ $(,)? }) => {
        impl $crate::codec::Shaped for $ty {
            fn shape() -> String {
                stringify!($ty).to_string()
            }
        }
        impl $crate::codec::Put for $ty {
            fn put(&self, out: &mut Vec<u8>) {
                $($crate::codec::Put::put(&self.$f, out);)+
            }
        }
        impl $crate::codec::Define for $ty {
            fn define() -> $crate::codec::TypeDef {
                let fields: Vec<String> = vec![$(format!(
                    "{}: {}",
                    stringify!($f),
                    $crate::codec::field_shape::<$ty, _>(|s| &s.$f)
                )),+];
                $crate::codec::TypeDef {
                    name: stringify!($ty),
                    body: format!("record: {}", fields.join(", ")),
                    tags: Vec::new(),
                }
            }
        }
    };
}

/// The reading side of a record that a request carries.
macro_rules! wire_record_get {
    ($ty:ident { $($f:ident),+ $(,)? }) => {
        impl $crate::codec::Get for $ty {
            fn get(r: &mut $crate::codec::Reader<'_>) -> Result<Self, $crate::CounterError> {
                Ok(Self { $($f: $crate::codec::Get::get(r)?),+ })
            }
        }
    };
}

pub(crate) use {wire_record, wire_record_get, wire_unit_enum};

#[cfg(test)]
mod tests {
    use super::*;

    fn round<T: Put + Get + PartialEq + std::fmt::Debug>(v: T) {
        let mut out = Vec::new();
        v.put(&mut out);
        let mut r = Reader::new(&out);
        assert_eq!(T::get(&mut r).expect("reads"), v);
        r.end().expect("nothing left");
        // Every cut is refused.
        for n in 0..out.len() {
            let mut r = Reader::new(out.get(..n).expect("a prefix"));
            assert!(T::get(&mut r).and_then(|_| r.end()).is_err(), "cut at {n}");
        }
    }

    #[test]
    fn the_primitives_round_trip_and_refuse_every_cut() {
        round(0x0102_0304u32);
        round(u64::MAX);
        round(true);
        round("hello \u{e9}".to_string());
        round(Some(7u32));
        round(None::<u32>);
        round(vec![1u8, 2, 3]);
        round(vec![vec!["a".to_string()], vec![]]);
        let mut bytes = Vec::new();
        5u32.put(&mut bytes);
        "x".to_string().put(&mut bytes);
        let mut r = Reader::new(&bytes);
        assert_eq!(
            <(u32, String)>::get(&mut r).expect("reads"),
            (5, "x".to_string())
        );
        r.end().expect("nothing left");
    }

    #[test]
    fn bad_flags_lengths_and_text_are_refused() {
        assert!(bool::get(&mut Reader::new(&[2])).is_err());
        assert!(Option::<u8>::get(&mut Reader::new(&[2, 0])).is_err());
        // A length longer than what is left.
        assert!(String::get(&mut Reader::new(&[9, 0, 0, 0, b'a'])).is_err());
        assert!(Vec::<u8>::get(&mut Reader::new(&[0xff, 0xff, 0xff, 0xff])).is_err());
        // Not UTF-8.
        assert!(String::get(&mut Reader::new(&[1, 0, 0, 0, 0xff])).is_err());
        assert!(Reader::new(&[1]).end().is_err());
    }
}
