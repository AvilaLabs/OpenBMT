//! Four-word fingerprints from the BIP39 English list.
//!
//! Four 11-bit numbers each pick one word out of the 2,048-word list, so the
//! words carry 44 bits. They are easy to read aloud and compare across two
//! screens.
//!
//! [`words_from_digest`] is the plain mapping from the first 44 bits of a
//! digest to words. [`fingerprint_words`] first runs the digest through
//! [`stretch`], which costs about 2^20 SHA-256 calls, and then maps the
//! result. Strength, stated exactly:
//!
//! - The words carry 44 bits. Because of the stretch, finding a second input
//!   whose words equal a given set of words costs about 2^44 tries of about
//!   2^21 SHA-256 compressions each, about 2^65 compressions in all.
//! - A party that chooses both compared values can still find a colliding
//!   pair in about 2^22 tries (the birthday bound on 44 bits). The words must
//!   therefore never be the only check where one party supplies both sides.
//! - Software always compares the full 32-byte hashes. The words are a human
//!   cross-check, not a replacement for that comparison.

use bip39::Language;
use bitcoin::hashes::{sha256, Hash, HashEngine};

use crate::hashing::tagged_hash;

/// Number of words shown.
pub const WORD_COUNT: usize = 4;

/// Number of chained SHA-256 rounds in [`stretch`].
pub const STRETCH_ROUNDS: u32 = 1 << 20;

/// Tag for the first step of [`stretch`].
pub const TAG_WORDS: &str = "OpenBMT/words/v1";

/// Slow, deterministic stretch of a 32-byte digest.
///
/// `h0 = tagged_hash(TAG_WORDS, digest)`, then for `i` in `1..=STRETCH_ROUNDS`
/// `h_i = SHA256(h_{i-1} || digest)`. The result is `h_STRETCH_ROUNDS`.
pub fn stretch(digest: &[u8; 32]) -> [u8; 32] {
    let mut h = tagged_hash(TAG_WORDS, digest);
    for _ in 0..STRETCH_ROUNDS {
        let mut eng = sha256::Hash::engine();
        eng.input(&h);
        eng.input(digest);
        h = sha256::Hash::from_engine(eng).to_byte_array();
    }
    h
}

/// Four words for `digest` after [`stretch`]. This is what screens show.
pub fn fingerprint_words(digest: &[u8; 32]) -> [&'static str; WORD_COUNT] {
    words_from_digest(&stretch(digest))
}

/// Turn the first 44 bits of `digest` into four BIP39 words.
pub fn words_from_digest(digest: &[u8; 32]) -> [&'static str; WORD_COUNT] {
    let list = Language::English.word_list();
    // 44 bits = the first 5.5 bytes, read as a big-endian number.
    let mut acc: u64 = 0;
    for b in &digest[..6] {
        acc = (acc << 8) | u64::from(*b);
    }
    let bits44 = acc >> 4; // 48 bits read, keep the top 44
    let mut out = [""; WORD_COUNT];
    for (i, slot) in out.iter_mut().enumerate() {
        let shift = 11 * (WORD_COUNT - 1 - i);
        let idx = ((bits44 >> shift) & 0x7ff) as usize;
        *slot = list[idx];
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_zero_digest_is_abandon() {
        assert_eq!(words_from_digest(&[0; 32]), ["abandon"; 4]);
    }

    #[test]
    fn all_ones_digest_is_last_word() {
        let last = Language::English.word_list()[2047];
        assert_eq!(words_from_digest(&[0xff; 32]), [last; 4]);
    }

    #[test]
    fn only_first_44_bits_matter() {
        let mut a = [0u8; 32];
        a[0] = 0b0000_0000;
        a[1] = 0b0010_0000; // bits 8..11 -> contributes to word 1
        let mut b = a;
        b[5] |= 0x0f; // bits 44..48: ignored
        b[20] = 0xaa; // far bytes: ignored
        assert_eq!(words_from_digest(&a), words_from_digest(&b));
        let mut c = a;
        c[5] |= 0x10; // bit 43: counts
        assert_ne!(words_from_digest(&a), words_from_digest(&c));
    }

    #[test]
    fn word_order_is_big_endian() {
        let list = Language::English.word_list();
        // Bit 10 (counting from the left) is the last bit of the first
        // 11-bit group, so it makes the first word index 1.
        let mut d = [0u8; 32];
        d[1] = 0x20;
        assert_eq!(
            words_from_digest(&d),
            [list[1], "abandon", "abandon", "abandon"]
        );
        // Bit 43 is the last bit of the fourth group.
        let mut e = [0u8; 32];
        e[5] = 0x10;
        assert_eq!(
            words_from_digest(&e),
            ["abandon", "abandon", "abandon", list[1]]
        );
    }

    #[test]
    fn stretch_known_answers() {
        // Independent computation with Python hashlib: h = SHA256(SHA256(tag)
        // || SHA256(tag) || digest), then 2^20 times h = SHA256(h || digest).
        let hex = |b: [u8; 32]| b.iter().map(|x| format!("{x:02x}")).collect::<String>();
        assert_eq!(
            hex(stretch(&[0u8; 32])),
            "81264a018f07cf5d290fa6d3a3c84977af5cd042bb088749165560dc9fddba9b"
        );
        let mut seq = [0u8; 32];
        for (i, b) in seq.iter_mut().enumerate() {
            *b = i as u8;
        }
        assert_eq!(
            hex(stretch(&seq)),
            "bea3406ccf1e581b19988ca9d8d994201d0ce4be50ca897d09bae5860d089f89"
        );
    }

    #[test]
    fn fingerprint_words_are_the_stretched_mapping() {
        let d = [7u8; 32];
        assert_eq!(fingerprint_words(&d), words_from_digest(&stretch(&d)));
        assert_ne!(fingerprint_words(&[0; 32]), words_from_digest(&[0; 32]));
        assert_ne!(fingerprint_words(&d), words_from_digest(&d));
    }
}
