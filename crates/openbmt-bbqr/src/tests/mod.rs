//! Tests, named after the rule each proves.

mod inflate;
mod meta;
mod parts;

use crate::{Joiner, Kind};

pub(crate) const PSBT_ONLY: &[Kind] = &[Kind::Psbt, Kind::Transaction];

/// A reader for a PSBT or a transaction.
pub(crate) fn joiner() -> Joiner {
    Joiner::new(PSBT_ONLY)
}

/// `len` bytes that are the same every run.
pub(crate) fn sample(len: usize) -> Vec<u8> {
    let mut x: u32 = 0x1234_5678;
    (0..len)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            u8::try_from(x >> 24).unwrap_or(0)
        })
        .collect()
}

/// The hex of `bytes`, upper case.
pub(crate) fn hex_upper(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02X}")).collect()
}

/// The bytes of a hex text.
pub(crate) fn unhex(text: &str) -> Vec<u8> {
    text.as_bytes()
        .chunks(2)
        .map(|p| u8::from_str_radix(std::str::from_utf8(p).unwrap_or("00"), 16).unwrap_or(0))
        .collect()
}
