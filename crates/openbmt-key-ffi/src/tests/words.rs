//! A key made from words the owner already has.

use std::sync::Arc;

use super::kit::*;
use crate::{
    word_is_valid, word_suggestions, EntropySource, KeyApp, KeyError, KeyNetwork, SetupState,
};

const ABANDON_12: &str = "abandon abandon abandon abandon abandon abandon abandon abandon \
                          abandon abandon abandon about";

fn typed(text: &str) -> EntropySource {
    EntropySource::TypedWords {
        words: text.split(' ').map(str::to_string).collect(),
    }
}

fn make(source: EntropySource, network: KeyNetwork) -> (Arc<KeyApp>, Result<(), KeyError>) {
    let store = MemStore::new();
    let app = KeyApp::open(store).expect("opens empty");
    let r = app.create_from_entropy(source, network);
    (app, r)
}

fn refusal(source: EntropySource) -> KeyError {
    let (app, r) = make(source, KeyNetwork::Mainnet);
    let e = r.expect_err("refused");
    // Nothing was made.
    assert_eq!(app.state().expect("state"), SetupState::Empty);
    e
}

#[test]
fn the_well_known_phrase_gives_the_well_known_key() {
    let (app, r) = make(typed(ABANDON_12), KeyNetwork::Mainnet);
    r.expect("made");
    let public = app.key2_public().expect("asked").expect("key 2 here");
    assert_eq!(public.master_fingerprint, "73c5da0a");
    assert_eq!(
        public.account_xpub,
        "xpub6DkFAXWQ2dHxq2vatrt9qyA3bXYU4ToWQwCHbf5XB2mSTexcHZCeKS1VZYcPoBd5X8yVcbXFHJR9R8UCVpt82VX1VhR28mCyxUFL4r6KFrf"
    );
    // The owner has the words already: none are waiting or returned.
    assert_eq!(
        app.state().expect("state"),
        SetupState::HasIdentity {
            holds_key2: true,
            words_waiting: false
        }
    );
    assert_eq!(
        app.key_words().expect_err("nothing to show").rule(),
        "words-shown-once"
    );
}

#[test]
fn the_24_word_vector_is_accepted() {
    // BIP39 test vector: all-zero entropy.
    let phrase = format!("{} art", ["abandon"; 23].join(" "));
    let (app, r) = make(typed(&phrase), KeyNetwork::Regtest);
    r.expect("made");
    let public = app.key2_public().expect("asked").expect("key 2 here");
    assert_eq!(public.master_fingerprint.len(), 8);
    assert_ne!(public.master_fingerprint, "73c5da0a");
}

#[test]
fn a_typed_key_goes_through_setup_like_any_other() {
    let store = MemStore::new();
    let app = KeyApp::open(store.clone()).expect("opens empty");
    app.create_from_entropy(typed(ABANDON_12), KeyNetwork::Regtest)
        .expect("made");
    let (wallet, _, key3) = wallet_with(&app);
    app.accept_wallet(setup_for(&wallet, &key3))
        .expect("accepted");
    // Fresh signer: empty history, marks at zero.
    let status = app.signer_status().expect("status");
    assert_eq!(status.spend_log_count, 0);
    assert_eq!(status.receive_mark, 0);
    assert!(status.allowlist.is_empty());
    // The phrase is nowhere in the stored bytes.
    let stored = store.saved().expect("stored");
    assert!(!String::from_utf8_lossy(&stored).contains("abandon"));
}

#[test]
fn case_and_spaces_around_words_are_accepted() {
    let words: Vec<String> = ABANDON_12
        .split(' ')
        .enumerate()
        .map(|(i, w)| match i % 3 {
            0 => format!("  {}", w.to_uppercase()),
            1 => format!("{w}\n"),
            _ => format!("\t{}{} ", &w[..1].to_uppercase(), &w[1..]),
        })
        .collect();
    let (app, r) = make(EntropySource::TypedWords { words }, KeyNetwork::Mainnet);
    r.expect("made");
    assert_eq!(
        app.key2_public()
            .expect("asked")
            .expect("key 2")
            .master_fingerprint,
        "73c5da0a"
    );
}

#[test]
fn a_wrong_checksum_is_refused() {
    let phrase = ABANDON_12.replace("about", "abandon");
    let e = refusal(typed(&phrase));
    assert_eq!(e.rule(), "words-checksum");
}

#[test]
fn other_counts_are_refused() {
    for n in [0usize, 1, 11, 13, 15, 18, 21, 23, 25] {
        let words = vec!["abandon".to_string(); n];
        let e = refusal(EntropySource::TypedWords { words });
        assert_eq!(e.rule(), "words-count", "{n} words");
        assert!(e.text().contains(&n.to_string()));
    }
}

#[test]
fn a_word_that_is_not_on_the_list_names_its_position_and_is_not_echoed() {
    for (len, bad) in [(12usize, "zzyzx"), (24, "abandonx")] {
        for at in 0..len {
            let mut words = vec!["abandon".to_string(); len];
            words[at] = bad.to_string();
            let e = refusal(EntropySource::TypedWords { words });
            assert_eq!(e.rule(), "words-unknown");
            assert!(
                e.text().contains(&format!("word {} ", at + 1)),
                "position {} in {}",
                at + 1,
                e.text()
            );
            assert!(!e.text().contains(bad));
        }
    }
}

#[test]
fn no_error_repeats_typed_words() {
    let mut words: Vec<String> = ABANDON_12.split(' ').map(str::to_string).collect();
    words[3] = "secretword".to_string();
    let e = refusal(EntropySource::TypedWords { words });
    assert!(!e.text().contains("secretword"));
    let phrase = ABANDON_12.replace("about", "ability");
    let e = refusal(typed(&phrase));
    assert_eq!(e.rule(), "words-checksum");
    assert!(!e.text().contains("ability") && !e.text().contains("abandon"));
}

#[test]
fn a_ready_key_app_does_not_take_typed_words() {
    let rig = rig();
    let e = rig
        .app
        .create_from_entropy(typed(ABANDON_12), KeyNetwork::Regtest)
        .expect_err("wallet set");
    assert_eq!(e.rule(), "wallet-set");
}

#[test]
fn the_picker_suggests_and_checks() {
    assert!(word_suggestions(String::new()).is_empty());
    assert!(word_suggestions("  ".to_string()).is_empty());
    assert!(word_suggestions("q".repeat(100)).is_empty());
    assert!(word_suggestions("xyz".to_string()).is_empty());
    let a = word_suggestions(" AB ".to_string());
    assert_eq!(a.len(), 8);
    assert_eq!(a[0], "abandon");
    assert!(a.iter().all(|w| w.starts_with("ab")));
    assert_eq!(word_suggestions("zoo".to_string()), vec!["zoo"]);
    assert_eq!(word_suggestions("aban".to_string()), vec!["abandon"]);
    assert!(word_is_valid(" About ".to_string()));
    assert!(!word_is_valid("abou".to_string()));
    assert!(!word_is_valid(String::new()));
    assert!(!word_is_valid("abandon about".to_string()));
}
