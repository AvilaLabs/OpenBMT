//! The part of CBOR (RFC 8949) a UR needs: unsigned integers (major type
//! 0), byte strings (major type 2) and the head of an array (major type
//! 4), always in the shortest form. Nothing else is read or written, and a
//! longer-than-needed head is refused (UR-13, UR-08).

use crate::error::UrError;

pub(crate) const MAJOR_UINT: u8 = 0;
pub(crate) const MAJOR_BYTES: u8 = 2;
pub(crate) const MAJOR_ARRAY: u8 = 4;

/// The head of an item: its major type and argument, in the shortest form.
pub(crate) fn push_head(out: &mut Vec<u8>, major: u8, value: u64) {
    let m = major << 5;
    if value < 24 {
        out.push(m | value as u8);
    } else if value <= 0xff {
        out.push(m | 24);
        out.push(value as u8);
    } else if value <= 0xffff {
        out.push(m | 25);
        out.extend_from_slice(&(value as u16).to_be_bytes());
    } else if value <= 0xffff_ffff {
        out.push(m | 26);
        out.extend_from_slice(&(value as u32).to_be_bytes());
    } else {
        out.push(m | 27);
        out.extend_from_slice(&value.to_be_bytes());
    }
}

/// Reads a head; gives the major type, the argument and what follows. Only
/// the shortest form is accepted; indefinite lengths and reserved
/// additional information are refused.
pub(crate) fn read_head(buf: &[u8]) -> Option<(u8, u64, &[u8])> {
    let (&first, rest) = buf.split_first()?;
    let major = first >> 5;
    let info = first & 31;
    let (width, min): (usize, u64) = match info {
        0..=23 => return Some((major, u64::from(info), rest)),
        24 => (1, 24),
        25 => (2, 0x100),
        26 => (4, 0x1_0000),
        27 => (8, 0x1_0000_0000),
        _ => return None,
    };
    let (num, rest) = rest.split_at_checked(width)?;
    let value = num.iter().fold(0u64, |a, &b| (a << 8) | u64::from(b));
    if value < min {
        return None;
    }
    Some((major, value, rest))
}

/// A byte string item (major type 2) holding `data`.
pub fn encode_bytes(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() + 9);
    push_head(&mut out, MAJOR_BYTES, data.len() as u64);
    out.extend_from_slice(data);
    out
}

/// The content of a CBOR byte string that is the whole of `cbor`: shortest
/// head, exact length, nothing after it (UR-13).
pub fn decode_bytes(cbor: &[u8]) -> Result<&[u8], UrError> {
    let Some((major, len, rest)) = read_head(cbor) else {
        return Err(UrError::Cbor("the head is cut, indefinite or not shortest"));
    };
    if major != MAJOR_BYTES {
        return Err(UrError::Cbor("the item is not a byte string"));
    }
    let Ok(len) = usize::try_from(len) else {
        return Err(UrError::Cbor("the length does not fit"));
    };
    if rest.len() != len {
        return Err(UrError::Cbor("the length does not match the content"));
    }
    Ok(rest)
}
