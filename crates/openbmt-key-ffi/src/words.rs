//! Words the person already has (`docs/DESIGN.md` §2.1 step 3, §5.4): the
//! typed phrase, and the helpers of the app's word picker.
//!
//! The typed words are never echoed in an error and never stored. This is
//! the same fresh-signer path as every other source: the signer starts with
//! an empty history and no rule is relaxed. Restore-and-sign mode
//! (`docs/DESIGN.md` §5.4) is not built.

use bip39::{Language, Mnemonic};

use crate::envelope::wipe_string;
use crate::error::{refused_by, KeyError};

/// The most suggestions the picker is given.
const MAX_SUGGESTIONS: usize = 8;

/// The longest word of the English list is 8 letters; anything much longer
/// is refused without looking it up.
const MAX_TYPED_WORD_BYTES: usize = 64;

fn clean(word: &str) -> String {
    word.trim().to_lowercase()
}

/// Up to 8 BIP39 English words that start with `prefix` (trimmed and
/// lower-cased), for the word picker. Empty for an empty prefix.
#[uniffi::export]
pub fn word_suggestions(prefix: String) -> Vec<String> {
    let prefix = clean(&prefix);
    if prefix.is_empty() || prefix.len() > MAX_TYPED_WORD_BYTES {
        return Vec::new();
    }
    Language::English
        .words_by_prefix(&prefix)
        .iter()
        .take(MAX_SUGGESTIONS)
        .map(|w| (*w).to_string())
        .collect()
}

/// Whether `word` (trimmed and lower-cased) is on the BIP39 English list.
#[uniffi::export]
pub fn word_is_valid(word: String) -> bool {
    let word = clean(&word);
    word.len() <= MAX_TYPED_WORD_BYTES && Language::English.find_word(&word).is_some()
}

/// The mnemonic of the typed `words`. Refused, with a distinct rule each,
/// for a count other than 12 or 24 (`words-count`), a word that is not on
/// the list (`words-unknown`, naming its 1-based position only) and a
/// checksum that does not fit (`words-checksum`). The cleaned copies are
/// overwritten before this returns.
pub(crate) fn mnemonic_from_typed(words: &[String]) -> Result<Mnemonic, KeyError> {
    if words.len() != 12 && words.len() != 24 {
        return Err(refused_by(
            "words-count",
            format!("{} words were typed; a key has 12 or 24", words.len()),
        ));
    }
    let cleaned: Vec<String> = words.iter().map(|w| clean(w)).collect();
    let result = parse_cleaned(&cleaned);
    cleaned.into_iter().for_each(wipe_string);
    result
}

fn parse_cleaned(cleaned: &[String]) -> Result<Mnemonic, KeyError> {
    for (i, w) in cleaned.iter().enumerate() {
        if w.len() > MAX_TYPED_WORD_BYTES || Language::English.find_word(w).is_none() {
            return Err(refused_by(
                "words-unknown",
                format!(
                    "word {} is not on the word list; check how it is spelled",
                    i + 1
                ),
            ));
        }
    }
    let joined = cleaned.join(" ");
    let parsed = Mnemonic::parse_in_normalized(Language::English, &joined);
    wipe_string(joined);
    parsed.map_err(|_| {
        refused_by(
            "words-checksum",
            "the words do not fit together: one is wrong or out of order",
        )
    })
}
