//! The pinned host name of a swap provider: a Tor v3 onion name (SW-82).
//!
//! A copy of the check of `openbmt-net` (`onion::parse_onion_name`, NR-92),
//! which this crate does not depend on: the checksum is SHA3-256, written
//! here as in that crate. A test of this crate holds the two to the same
//! answer on every name it tries.

// ---- SHA3-256 of FIPS 202 (the checksum of a v3 name) ----

/// Bytes of one block of input: 1600 bits less twice the 256-bit output.
const RATE: usize = 136;

const ROUNDS: usize = 24;

/// The round constants of Keccak-f[1600].
const RC: [u64; ROUNDS] = [
    0x0000_0000_0000_0001,
    0x0000_0000_0000_8082,
    0x8000_0000_0000_808a,
    0x8000_0000_8000_8000,
    0x0000_0000_0000_808b,
    0x0000_0000_8000_0001,
    0x8000_0000_8000_8081,
    0x8000_0000_0000_8009,
    0x0000_0000_0000_008a,
    0x0000_0000_0000_0088,
    0x0000_0000_8000_8009,
    0x0000_0000_8000_000a,
    0x0000_0000_8000_808b,
    0x8000_0000_0000_008b,
    0x8000_0000_0000_8089,
    0x8000_0000_0000_8003,
    0x8000_0000_0000_8002,
    0x8000_0000_0000_0080,
    0x0000_0000_0000_800a,
    0x8000_0000_8000_000a,
    0x8000_0000_8000_8081,
    0x8000_0000_0000_8080,
    0x0000_0000_8000_0001,
    0x8000_0000_8000_8008,
];

/// The rotation of each lane in the rho step, by lane `x + 5 * y`.
const ROT: [u32; 25] = [
    0, 1, 62, 28, 27, 36, 44, 6, 55, 20, 3, 10, 43, 25, 39, 41, 45, 15, 21, 8, 18, 2, 61, 56, 14,
];

/// The state is 25 lanes of 64 bits, lane `x + 5 * y`. Every index is below
/// 25 by construction.
#[allow(clippy::indexing_slicing)]
fn keccak_f(a: &mut [u64; 25]) {
    for rc in RC {
        // theta
        let mut c = [0u64; 5];
        for (x, cx) in c.iter_mut().enumerate() {
            *cx = (0..5).fold(0, |acc, y| acc ^ a[x + 5 * y]);
        }
        for x in 0..5 {
            let d = c[(x + 4) % 5] ^ c[(x + 1) % 5].rotate_left(1);
            for y in 0..5 {
                a[x + 5 * y] ^= d;
            }
        }
        // rho and pi
        let mut b = [0u64; 25];
        for x in 0..5 {
            for y in 0..5 {
                let to = y + 5 * ((2 * x + 3 * y) % 5);
                b[to] = a[x + 5 * y].rotate_left(ROT[x + 5 * y]);
            }
        }
        // chi
        for y in 0..5 {
            for x in 0..5 {
                a[x + 5 * y] = b[x + 5 * y] ^ (!b[(x + 1) % 5 + 5 * y] & b[(x + 2) % 5 + 5 * y]);
            }
        }
        // iota
        a[0] ^= rc;
    }
}

/// XOR a block into the state, then permute.
fn absorb(state: &mut [u64; 25], block: &[u8; RATE]) {
    let (words, _) = block.as_chunks::<8>();
    for (lane, word) in state.iter_mut().zip(words) {
        *lane ^= u64::from_le_bytes(*word);
    }
    keccak_f(state);
}

/// SHA3-256 of the input.
fn sha3_256(input: &[u8]) -> [u8; 32] {
    let mut state = [0u64; 25];
    let (full, rest) = input.as_chunks::<RATE>();
    for block in full {
        absorb(&mut state, block);
    }
    // The last block: what is left, the suffix 01 and the 1 bit of the
    // padding after it, and the last 1 bit of the padding at the end.
    let mut last = [0u8; RATE];
    for (to, from) in last.iter_mut().zip(rest) {
        *to = *from;
    }
    if let Some(b) = last.get_mut(rest.len()) {
        *b ^= 0x06;
    }
    last[RATE - 1] ^= 0x80;
    absorb(&mut state, &last);
    let mut out = [0u8; 32];
    let (words, _) = out.as_chunks_mut::<8>();
    for (word, lane) in words.iter_mut().zip(state) {
        *word = lane.to_le_bytes();
    }
    out
}

// ---- the name ----

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

/// SHA3-256 as lower-case hex, for the tests of the function.
#[cfg(test)]
pub(crate) fn sha3_256_hex(input: &[u8]) -> String {
    sha3_256(input).iter().map(|b| format!("{b:02x}")).collect()
}
