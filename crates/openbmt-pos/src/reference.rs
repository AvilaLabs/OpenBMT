//! The sale reference (PS-20 to PS-22) and the idempotency key (PS-81).
//!
//! The reference is a short code a cashier can type into a POS tender's
//! note, so that a match can be exact. It is a pure function of the counter's
//! key and the sale index. It is a convenience for matching, never a proof of
//! payment.

use std::fmt;

use bitcoin::hashes::{sha256, Hash, HashEngine};
use openbmt_core::identity::DeviceId;

/// Tag of the hash the reference is cut from.
pub const TAG_REFERENCE: &str = "OpenBMT/pos-reference/v1";
/// Tag of the idempotency key.
pub const TAG_IDEMPOTENCY: &str = "OpenBMT/pos-close/v1";

/// PS-20: the 31 characters a reference is made of: the digits and letters
/// without `0`, `O`, `1`, `I` and `L`.
pub const ALPHABET: &[u8; 31] = b"23456789ABCDEFGHJKMNPQRSTUVWXYZ";
/// PS-20: characters in a reference, the check character included.
pub const REF_LEN: usize = 6;

const MODULUS: u32 = 31;

/// BIP340-style tagged hash. The matching function in `openbmt-core` is not
/// public, so the same construction is repeated here.
pub(crate) fn tagged_hash(tag: &str, msg: &[&[u8]]) -> [u8; 32] {
    let th = sha256::Hash::hash(tag.as_bytes());
    let mut eng = sha256::Hash::engine();
    eng.input(th.as_byte_array());
    eng.input(th.as_byte_array());
    for part in msg {
        eng.input(part);
    }
    sha256::Hash::from_engine(eng).to_byte_array()
}

/// The value of a character of the alphabet, upper-case.
fn value_of(c: u8) -> Option<u32> {
    ALPHABET
        .iter()
        .position(|a| *a == c)
        .and_then(|p| u32::try_from(p).ok())
}

/// The character with this value.
fn char_of(v: u32) -> u8 {
    usize::try_from(v % MODULUS)
        .ok()
        .and_then(|i| ALPHABET.get(i).copied())
        .unwrap_or(b'2')
}

/// PS-20: the check value of five values: each times its place (1 to 5),
/// summed, modulo 31.
fn check_of(values: &[u32]) -> u32 {
    values
        .iter()
        .zip(1u32..)
        .fold(0u32, |acc, (v, w)| (acc + v * w) % MODULUS)
}

/// A sale reference: six upper-case characters of the alphabet, the last
/// one the check (PS-20).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RefCode([u8; REF_LEN]);

impl RefCode {
    /// PS-20: the reference of sale `index` of the counter with this key.
    pub fn for_sale(counter: &DeviceId, index: u32) -> Self {
        let digest = tagged_hash(TAG_REFERENCE, &[&counter.to_bytes(), &index.to_le_bytes()]);
        let mut values = [0u32; REF_LEN - 1];
        for (v, pair) in values.iter_mut().zip(digest.as_chunks::<2>().0) {
            let n = pair.iter().fold(0u32, |a, b| (a << 8) | u32::from(*b));
            *v = n % MODULUS;
        }
        let check = check_of(&values);
        let mut out = [0u8; REF_LEN];
        for (o, v) in out.iter_mut().zip(values.iter().chain([&check])) {
            *o = char_of(*v);
        }
        Self(out)
    }

    /// Read exactly six characters as a reference: case is ignored, the
    /// characters must be in the alphabet and the check must hold.
    pub fn parse(s: &str) -> Option<Self> {
        let bytes = s.as_bytes();
        if bytes.len() != REF_LEN {
            return None;
        }
        let mut values = [0u32; REF_LEN];
        let mut out = [0u8; REF_LEN];
        for ((v, o), b) in values.iter_mut().zip(out.iter_mut()).zip(bytes) {
            let up = b.to_ascii_uppercase();
            *v = value_of(up)?;
            *o = up;
        }
        let (last, head) = values.split_last()?;
        (check_of(head) == *last).then_some(Self(out))
    }

    /// The reference as text, for the counter to show.
    pub fn as_str(&self) -> &str {
        // The bytes come from the alphabet, which is ASCII.
        std::str::from_utf8(&self.0).unwrap_or("")
    }

    /// The six bytes.
    pub fn as_bytes(&self) -> &[u8; REF_LEN] {
        &self.0
    }
}

impl fmt::Display for RefCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// PS-21: find the reference in free text. Case does not matter. The text is
/// cut into runs of letters and digits; a run of exactly six characters of
/// the alphabet with a good check is a reference. If there is none, or more
/// than one different one, nothing is found.
pub fn find_reference(text: &str) -> Option<RefCode> {
    let mut found: Option<RefCode> = None;
    for token in text.split(|c: char| !c.is_ascii_alphanumeric()) {
        if let Some(code) = RefCode::parse(token) {
            match found {
                Some(other) if other != code => return None,
                _ => found = Some(code),
            }
        }
    }
    found
}

/// PS-81: the key a close request carries, so that a retry does not close an
/// order twice: the tagged hash of the counter's key and the sale index.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct IdempotencyKey([u8; 32]);

impl IdempotencyKey {
    /// The key of sale `index` of the counter with this key.
    pub fn for_sale(counter: &DeviceId, index: u32) -> Self {
        Self(tagged_hash(
            TAG_IDEMPOTENCY,
            &[&counter.to_bytes(), &index.to_le_bytes()],
        ))
    }

    /// The 32 bytes.
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// The key as lower-case hex, for a header or a field.
    pub fn to_hex(&self) -> String {
        self.0.iter().map(|b| format!("{b:02x}")).collect()
    }
}
