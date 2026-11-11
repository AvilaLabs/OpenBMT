#![no_main]

use std::sync::Arc;

use bip39::Language;
use libfuzzer_sys::fuzz_target;
use openbmt_key_ffi::{
    word_is_valid, word_suggestions, EntropySource, KeyApp, KeyError, KeyNetwork, KeyStore,
    SetupState, StoreError,
};

/// A store that keeps nothing and draws a fixed identity secret.
struct Null;

impl KeyStore for Null {
    fn load(&self) -> Result<Option<Vec<u8>>, StoreError> {
        Ok(None)
    }

    fn save(&self, _bytes: Vec<u8>) -> Result<(), StoreError> {
        Ok(())
    }

    fn random(&self, n: u32) -> Result<Vec<u8>, StoreError> {
        Ok(vec![0x42; n as usize])
    }

    fn now(&self) -> u64 {
        1_000_000
    }
}

/// The words of the input: lines of text when the first byte is even, and
/// indices into the English list (so that words are valid and only the
/// checksum is left to chance) when it is odd.
fn words_of(data: &[u8]) -> Option<Vec<String>> {
    let (first, rest) = data.split_first()?;
    if first % 2 == 0 {
        let text = std::str::from_utf8(rest).ok()?;
        return Some(text.split('\n').take(40).map(str::to_string).collect());
    }
    let list = Language::English.word_list();
    Some(
        rest.as_chunks::<2>()
            .0
            .iter()
            .take(26)
            .map(|c| list[usize::from(u16::from_le_bytes(*c)) % 2048].to_string())
            .collect(),
    )
}

fuzz_target!(|data: &[u8]| {
    let Some(words) = words_of(data) else {
        return;
    };
    let cleaned: Vec<String> = words.iter().map(|w| w.trim().to_lowercase()).collect();
    let Ok(app) = KeyApp::open(Arc::new(Null)) else {
        panic!("an empty store opens");
    };
    app.set_pin("482913".to_string()).expect("a PIN");
    let result = app.create_from_entropy(
        EntropySource::TypedWords {
            words: words.clone(),
        },
        KeyNetwork::Mainnet,
    );
    match result {
        Ok(()) => {
            assert!(cleaned.len() == 12 || cleaned.len() == 24);
            assert!(cleaned.iter().all(|w| word_is_valid(w.clone())));
            assert_eq!(
                app.state().expect("state"),
                SetupState::HasIdentity {
                    holds_key2: true,
                    words_waiting: false
                }
            );
            assert!(app.key_words().is_err());
        }
        Err(KeyError::Refused { rule, text }) => {
            assert!(matches!(
                rule.as_str(),
                "words-count" | "words-unknown" | "words-checksum"
            ));
            // The words are never echoed. Only letters, and longer than
            // every word of the messages, so a word cannot be a piece of
            // the message's own text (the fuzzer does find such inputs).
            for w in cleaned
                .iter()
                .filter(|w| w.len() >= 9 && w.bytes().all(|b| b.is_ascii_alphabetic()))
            {
                assert!(!text.contains(w.as_str()));
            }
            assert_eq!(app.state().expect("state"), SetupState::Empty);
        }
        Err(other) => panic!("a refusal of another kind: {other:?}"),
    }
    // The picker never panics, and what it gives is valid.
    for w in &words {
        for s in word_suggestions(w.clone()) {
            assert!(word_is_valid(s));
        }
    }
});
