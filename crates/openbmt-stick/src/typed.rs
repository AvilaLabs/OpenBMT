//! SK-28: the owner types 12 words; the BIP39 checksum is checked.

use crate::mix::ENTROPY_BYTES;

/// Words in a key.
pub const WORD_COUNT: usize = 12;

/// The word the typed letters mean: a whole word of the English list, or
/// the one word that starts with them (the first four letters of a BIP39
/// word are unique). Upper case is the same as lower.
pub fn complete_word(typed: &str) -> Option<&'static str> {
    let t = typed.to_ascii_lowercase();
    if t.is_empty() {
        return None;
    }
    let list = bip39::Language::English.word_list();
    if let Some(w) = list.iter().find(|w| **w == t) {
        return Some(w);
    }
    let mut hits = list.iter().filter(|w| w.starts_with(&t));
    match (hits.next(), hits.next()) {
        (Some(w), None) => Some(w),
        _ => None,
    }
}

/// Why 12 words were refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypedError {
    /// Not 12 words.
    Count,
    /// A word that is not on the list.
    BadWord,
    /// The BIP39 checksum does not match.
    BadChecksum,
}

/// The entropy of 12 typed words, if each is on the list and the checksum
/// matches.
pub fn check_words(words: &[&str]) -> Result<[u8; ENTROPY_BYTES], TypedError> {
    if words.len() != WORD_COUNT {
        return Err(TypedError::Count);
    }
    let m = bip39::Mnemonic::parse_normalized(&words.join(" ")).map_err(|e| match e {
        bip39::Error::InvalidChecksum => TypedError::BadChecksum,
        _ => TypedError::BadWord,
    })?;
    let mut out = [0u8; ENTROPY_BYTES];
    out.copy_from_slice(&m.to_entropy());
    Ok(out)
}
