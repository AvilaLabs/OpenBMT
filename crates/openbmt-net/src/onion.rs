//! The host name of a Tor v3 address (NR-92).
//!
//! The proxy is asked to connect to `<base32 of key, checksum, version>`
//! followed by `.onion`. The name is made from the 32-byte public key that
//! the book keeps, and read back to it for the group of the peer (NR-93).

use crate::sha3::sha3_256;

/// The suffix of an onion host name.
pub const SUFFIX: &str = ".onion";
/// The version byte of a Tor v3 address.
const VERSION: u8 = 3;
/// Characters of the base32 part of the name: 35 bytes in 5-bit groups.
const NAME_CHARS: usize = 56;
/// The base32 alphabet of RFC 4648, in lower case.
const ALPHABET: &[u8; 32] = b"abcdefghijklmnopqrstuvwxyz234567";

/// The checksum of a key: the first 2 bytes of SHA3-256 of `.onion
/// checksum`, the key and the version byte.
fn checksum(key: &[u8; 32]) -> [u8; 2] {
    let mut input = Vec::with_capacity(15 + 32 + 1);
    input.extend_from_slice(b".onion checksum");
    input.extend_from_slice(key);
    input.push(VERSION);
    let hash = sha3_256(&input);
    [hash[0], hash[1]]
}

/// NR-92: the host name for the public key of a Tor v3 address: 56
/// characters and `.onion`.
pub fn onion_name(key: &[u8; 32]) -> String {
    let mut raw = [0u8; 35];
    raw[..32].copy_from_slice(key);
    raw[32..34].copy_from_slice(&checksum(key));
    raw[34] = VERSION;
    let mut name = String::with_capacity(NAME_CHARS + SUFFIX.len());
    // 35 bytes are 280 bits: exactly 56 groups of 5 bits, no padding.
    let mut buffer = 0u32;
    let mut bits = 0u32;
    for byte in raw {
        buffer = (buffer << 8) | u32::from(byte);
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            let index = usize::try_from((buffer >> bits) & 31).unwrap_or(0);
            name.push(char::from(ALPHABET[index]));
        }
        buffer &= (1 << bits) - 1;
    }
    name.push_str(SUFFIX);
    name
}

/// The public key of an onion host name, if the name is exactly what
/// [`onion_name`] writes for it: lower case, 56 characters, the right
/// checksum and version, and `.onion` after.
pub fn parse_onion_name(name: &str) -> Option<[u8; 32]> {
    let base = name.strip_suffix(SUFFIX)?;
    if base.len() != NAME_CHARS {
        return None;
    }
    let mut raw = [0u8; 35];
    let mut at = 0usize;
    let mut buffer = 0u32;
    let mut bits = 0u32;
    for c in base.bytes() {
        let value = ALPHABET.iter().position(|a| *a == c)?;
        buffer = (buffer << 5) | u32::try_from(value).ok()?;
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            *raw.get_mut(at)? = u8::try_from((buffer >> bits) & 255).ok()?;
            at += 1;
            buffer &= (1 << bits) - 1;
        }
    }
    let key: [u8; 32] = raw[..32].try_into().ok()?;
    (raw[32..34] == checksum(&key) && raw[34] == VERSION).then_some(key)
}
