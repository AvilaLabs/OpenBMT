//! Four-word fingerprints from the BIP39 English list.
//!
//! The first 44 bits of a 32-byte digest are cut into four 11-bit numbers,
//! and each number picks one word out of the 2,048-word list. Four words are
//! easy to read aloud and compare across two screens. They are a human check,
//! not a replacement for comparing the hashes in software.

use bip39::Language;

/// Number of words shown.
pub const WORD_COUNT: usize = 4;

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
}
