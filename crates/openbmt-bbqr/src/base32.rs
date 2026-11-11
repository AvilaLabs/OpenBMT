//! Base32 of RFC 4648 section 6 without padding (BQ-09).

use crate::error::BbqrError;

const ALPHABET: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";

/// The characters of `data` in base32, with no `=`.
pub(crate) fn encode(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(5) * 8);
    let mut acc: u32 = 0;
    let mut bits: u32 = 0;
    for byte in data {
        acc = (acc << 8) | u32::from(*byte);
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            let index = usize::try_from((acc >> bits) & 31).unwrap_or(0);
            out.push(char::from(ALPHABET.get(index).copied().unwrap_or(b'A')));
        }
        acc &= (1 << bits) - 1;
    }
    if bits > 0 {
        let index = usize::try_from((acc << (5 - bits)) & 31).unwrap_or(0);
        out.push(char::from(ALPHABET.get(index).copied().unwrap_or(b'A')));
    }
    out
}

fn value(c: u8) -> Option<u32> {
    match c {
        b'A'..=b'Z' => Some(u32::from(c - b'A')),
        b'2'..=b'7' => Some(u32::from(c - b'2') + 26),
        _ => None,
    }
}

/// BQ-09: the bytes of upper-case base32 text with no padding. The text
/// must end after whole bytes: a last group of 2, 4, 5 or 7 characters or
/// none, with the unused bits zero, so that one text has one reading.
pub(crate) fn decode(text: &str) -> Result<Vec<u8>, BbqrError> {
    if !matches!(text.len() % 8, 0 | 2 | 4 | 5 | 7) {
        return Err(BbqrError::Base32("a last group that is not whole bytes"));
    }
    let mut out = Vec::with_capacity(text.len() / 8 * 5 + 4);
    let mut acc: u32 = 0;
    let mut bits: u32 = 0;
    for c in text.bytes() {
        let v = value(c).ok_or(BbqrError::Base32("a character outside A-Z and 2-7"))?;
        acc = (acc << 5) | v;
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            out.push(u8::try_from((acc >> bits) & 0xff).unwrap_or(0));
            acc &= (1 << bits) - 1;
        }
    }
    if acc != 0 {
        return Err(BbqrError::Base32("bits after the last byte are not zero"));
    }
    Ok(out)
}
