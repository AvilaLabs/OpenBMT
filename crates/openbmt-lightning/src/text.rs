//! The small text codecs of the crate: hex and base64. No external package.

/// Lower-case hex.
pub(crate) fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        for nibble in [b >> 4, b & 15] {
            out.push(char::from_digit(u32::from(nibble), 16).unwrap_or('0'));
        }
    }
    out
}

fn nibble(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

/// Bytes from hex of either case; `None` for an odd length or any other
/// character.
pub(crate) fn unhex(s: &str) -> Option<Vec<u8>> {
    let (pairs, rest) = s.as_bytes().as_chunks::<2>();
    if !rest.is_empty() {
        return None;
    }
    pairs
        .iter()
        .map(|[h, l]| Some(nibble(*h)? << 4 | nibble(*l)?))
        .collect()
}

/// Exactly `N` bytes from hex.
pub(crate) fn unhex_array<const N: usize>(s: &str) -> Option<[u8; N]> {
    <[u8; N]>::try_from(unhex(s)?).ok()
}

/// Which alphabet of base64 a text is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Alphabet {
    /// `A-Za-z0-9+/`.
    Standard,
    /// `A-Za-z0-9-_`.
    Url,
}

fn sextet(c: u8, alphabet: Alphabet) -> Option<u8> {
    match c {
        b'A'..=b'Z' => Some(c - b'A'),
        b'a'..=b'z' => Some(c - b'a' + 26),
        b'0'..=b'9' => Some(c - b'0' + 52),
        b'+' if alphabet == Alphabet::Standard => Some(62),
        b'/' if alphabet == Alphabet::Standard => Some(63),
        b'-' if alphabet == Alphabet::Url => Some(62),
        b'_' if alphabet == Alphabet::Url => Some(63),
        _ => None,
    }
}

/// Bytes from base64 in one alphabet, with or without `=` padding. A
/// character of the other alphabet, a space, a padding that is not at the
/// end or bits left over that are not zero is refused.
pub(crate) fn unbase64(s: &str, alphabet: Alphabet) -> Option<Vec<u8>> {
    let body = s.trim_end_matches('=');
    let padding = s.len() - body.len();
    if padding > 2 || (padding > 0 && !s.len().is_multiple_of(4)) {
        return None;
    }
    let mut out = Vec::with_capacity(body.len() * 3 / 4 + 1);
    let (mut acc, mut bits) = (0u32, 0u32);
    for c in body.bytes() {
        acc = (acc << 6 | u32::from(sextet(c, alphabet)?)) & 0xffff;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(u8::try_from(acc >> bits & 0xff).ok()?);
        }
    }
    // A group of one character is never whole, and the spare bits are zero.
    if bits >= 6 || acc & ((1 << bits) - 1) != 0 {
        return None;
    }
    Some(out)
}

/// Base64 with padding, for the replies of the tests.
#[cfg(test)]
pub(crate) fn base64(bytes: &[u8], alphabet: Alphabet) -> String {
    let table: &[u8; 64] = match alphabet {
        Alphabet::Standard => b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/",
        Alphabet::Url => b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_",
    };
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let mut n = 0u32;
        for i in 0..3 {
            n = n << 8 | u32::from(chunk.get(i).copied().unwrap_or(0));
        }
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(char::from(table[(n >> (18 - 6 * i) & 63) as usize]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// A text of ASCII letters and digits and the given extra characters, 1 to
/// `most` long.
pub(crate) fn plain(s: &str, most: usize, extra: &str) -> bool {
    !s.is_empty()
        && s.len() <= most
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || extra.contains(c))
}
