//! A strict, small JSON reader.
//!
//! It reads the whole document so that a malformed one is refused, keeps the
//! values in a tree, and the code that follows looks up the few fields it
//! wants. Input, depth and the number of values are bounded by [`Limits`], so
//! the work and the memory are bounded before the document is read.
//!
//! Integers are kept exactly or not at all: a number with no fraction and no
//! exponent that fits an `i64` (or, above it, a `u64`) is [`Value::Int`] or
//! [`Value::UInt`]; any other number is [`Value::Number`], which keeps only
//! its exact text, so that nothing downstream can use a rounded one.
//!
//! This crate has had internal review only. No independent security audit
//! has been done yet.

#![forbid(unsafe_code)]
#![deny(missing_docs)]
#![cfg_attr(
    not(test),
    deny(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing
    )
)]

mod parse;
mod value;

pub use parse::parse;
pub use value::Value;

/// The most bytes read by default: 8 MiB.
pub const MAX_BYTES: usize = 8 * 1024 * 1024;
/// The most levels of arrays and objects inside one another by default.
pub const MAX_DEPTH: usize = 64;
/// The most values (of any kind, in every array and object) by default.
pub const MAX_VALUES: usize = 400_000;

/// The bounds of one reading.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// The most bytes of the document.
    pub max_bytes: usize,
    /// The most levels of arrays and objects inside one another.
    pub max_depth: usize,
    /// The most values in the whole document.
    pub max_values: usize,
}

impl Limits {
    /// 8 MiB, 64 levels, 400,000 values.
    pub const DEFAULT: Self = Self {
        max_bytes: MAX_BYTES,
        max_depth: MAX_DEPTH,
        max_values: MAX_VALUES,
    };
}

impl Default for Limits {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// Why a document was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JsonError {
    /// More bytes than the limit; refused before it is read.
    TooLong,
    /// Arrays and objects nested deeper than the limit.
    TooDeep,
    /// More values than the limit.
    TooManyValues,
    /// Not UTF-8.
    NotUtf8,
    /// Not a JSON document.
    Malformed,
    /// Bytes after the document.
    TrailingBytes,
    /// A key twice in one object, however it was spelled.
    DuplicateKey,
}

impl std::fmt::Display for JsonError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::TooLong => "the document is longer than the limit",
            Self::TooDeep => "the document is nested deeper than the limit",
            Self::TooManyValues => "the document holds more values than the limit",
            Self::NotUtf8 => "the document is not UTF-8",
            Self::Malformed => "the document is not JSON",
            Self::TrailingBytes => "the document has bytes after it",
            Self::DuplicateKey => "the document has a key twice in one object",
        })
    }
}

impl std::error::Error for JsonError {}

#[cfg(test)]
mod tests;
