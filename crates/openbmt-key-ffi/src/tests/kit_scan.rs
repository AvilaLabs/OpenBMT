//! The check of a printed kit by scan: the description text only.

use bitcoin::bip32::Xpub;
use openbmt_core::wallet::{descriptor_checksum, KeySlot, WalletDescription};

use super::kit::*;
use crate::kitcheck::compare_kit;
use crate::KitCheck;

fn own_text(rig: &Rig) -> String {
    rig.wallet.canonical_string().to_string()
}

/// `text` with its body changed by `edit`, and a fresh descriptor checksum.
fn resealed(text: &str, edit: impl Fn(&str) -> String) -> String {
    let body = text.split('#').next().expect("a body");
    let body = edit(body);
    let sum = descriptor_checksum(&body).expect("a checksum");
    format!("{body}#{sum}")
}

fn different(check: KitCheck) -> (u32, bool, bool) {
    match check {
        KitCheck::Different {
            first_difference,
            same_checksum_words,
            parses,
        } => (first_difference, same_checksum_words, parses),
        KitCheck::Same => panic!("expected a difference"),
    }
}

#[test]
fn the_same_description_is_the_same() {
    let rig = rig();
    let own = own_text(&rig);
    assert_eq!(
        rig.app.check_kit(own.clone()).expect("checked"),
        KitCheck::Same
    );
    // White space at the ends is trimmed, and only there.
    assert_eq!(
        rig.app.check_kit(format!("{own}\n")).expect("checked"),
        KitCheck::Same
    );
    assert_eq!(
        rig.app
            .check_kit(format!(" \t\r\n{own} \r\n"))
            .expect("checked"),
        KitCheck::Same
    );
    let inside = own.replacen("sortedmulti(", "sortedmulti( ", 1);
    assert!(matches!(
        rig.app.check_kit(inside).expect("checked"),
        KitCheck::Different { .. }
    ));
}

#[test]
fn the_check_changes_nothing_and_stores_nothing() {
    let rig = rig();
    let saves = rig.store.saves();
    let saved = rig.store.saved();
    let snapshot = rig.app.wallet_view().expect("view");
    rig.app.check_kit(own_text(&rig)).expect("checked");
    rig.app.check_kit("junk".to_string()).expect("checked");
    assert_eq!(rig.store.saves(), saves);
    assert_eq!(rig.store.saved(), saved);
    assert_eq!(rig.app.wallet_view().expect("view"), snapshot);
    assert!(!String::from_utf8_lossy(&rig.store.plain().expect("stored")).contains("junk"));
}

#[test]
fn a_changed_character_in_an_xpub_is_reported() {
    let rig = rig();
    let own = own_text(&rig);
    let at = own.find("tpub").expect("an xpub") + 20;
    let mut bytes = own.clone().into_bytes();
    bytes[at] = if bytes[at] == b'a' { b'b' } else { b'a' };
    let edited = String::from_utf8(bytes).expect("ascii");
    // The old checksum no longer fits.
    let (first, words, parses) = different(rig.app.check_kit(edited.clone()).expect("checked"));
    assert_eq!(first as usize, at);
    assert!(!words && !parses);
    // With a fresh checksum the xpub itself is still wrong.
    let sealed = resealed(&edited, str::to_string);
    let (first, words, parses) = different(rig.app.check_kit(sealed).expect("checked"));
    assert_eq!(first as usize, at);
    assert!(!words && !parses);
}

#[test]
fn a_different_valid_description_parses_and_has_other_words() {
    let rig = rig();
    let (_, slot4) = fixed_key(4, 1);
    let public = rig.app.key2_public().expect("asked").expect("key 2");
    let slots = rig.wallet.slots().clone();
    let swapped = WalletDescription::new(
        NET,
        [
            KeySlot { index: 1, ..slot4 },
            slots[1].clone(),
            slots[2].clone(),
        ],
    )
    .expect("a wallet");
    assert_ne!(swapped.checksum_words(), rig.wallet.checksum_words());
    let (first, words, parses) = different(
        rig.app
            .check_kit(swapped.canonical_string().to_string())
            .expect("checked"),
    );
    assert!(parses && !words);
    let own = own_text(&rig);
    let expected = own
        .bytes()
        .zip(swapped.canonical_string().bytes())
        .position(|(a, b)| a != b)
        .expect("they differ");
    assert_eq!(first as usize, expected);
    // Key 2's own slot is the one the Key app holds.
    assert_eq!(
        slots[1].xpub.to_string(),
        public
            .account_xpub
            .parse::<Xpub>()
            .expect("xpub")
            .to_string()
    );
}

#[test]
fn another_marker_for_the_same_wallet_differs_but_parses() {
    let rig = rig();
    let own = own_text(&rig);
    let primes = resealed(&own, |b| b.replace('h', "'"));
    let parses_directly = WalletDescription::parse(&primes, NET);
    let (first, words, parses) = different(rig.app.check_kit(primes).expect("checked"));
    assert!(first > 0);
    assert_eq!(parses, parses_directly.is_ok());
    // The same wallet has the same words.
    assert_eq!(words, parses_directly.is_ok());
}

#[test]
fn nonsense_and_cuts_are_different_and_do_not_parse() {
    let rig = rig();
    let own = own_text(&rig);
    for (text, first) in [
        ("", 0usize),
        ("junk", 0),
        (&own[..own.len() - 1], own.len() - 1),
        (&own[..10], 10),
    ] {
        let (at, words, parses) = different(rig.app.check_kit(text.to_string()).expect("checked"));
        assert_eq!(at as usize, first, "{text}");
        assert!(!words && !parses);
    }
    let longer = format!("{own}x");
    let (at, _, _) = different(rig.app.check_kit(longer).expect("checked"));
    assert_eq!(at as usize, own.len());
}

#[test]
fn long_input_is_refused_before_anything_else() {
    let rig = rig();
    let e = rig.app.check_kit("a".repeat(9_000)).expect_err("too long");
    assert_eq!(e.rule(), "kit-too-long");
    // Even when the Key app is not ready.
    let app = open_new(MemStore::new());
    assert_eq!(
        app.check_kit("a".repeat(8_193))
            .expect_err("too long")
            .rule(),
        "kit-too-long"
    );
    assert!(matches!(
        rig.app.check_kit("a".repeat(8_192)).expect("at the limit"),
        KitCheck::Different { .. }
    ));
}

#[test]
fn only_a_ready_key_app_checks_a_kit() {
    let app = open_new(MemStore::new());
    assert_eq!(
        app.check_kit("x".to_string())
            .expect_err("not ready")
            .rule(),
        "not-ready"
    );
}

#[test]
fn matching_words_with_other_keys_are_flagged() {
    // Four words cannot be matched by search in a test, so the comparison
    // is run with a stubbed words function (the R4-2 case).
    let own = "wsh(own)#aaaaaaaa";
    let words = ["one", "two", "three", "four"];
    let stub = |t: &str| match t {
        "wsh(forged)#bbbbbbbb" => Some(words),
        "wsh(other)#cccccccc" => Some(["five", "two", "three", "four"]),
        _ => None,
    };
    assert_eq!(
        compare_kit(own, &words, "wsh(own)#aaaaaaaa", stub),
        KitCheck::Same
    );
    assert_eq!(
        compare_kit(own, &words, " wsh(forged)#bbbbbbbb ", stub),
        KitCheck::Different {
            first_difference: 4,
            same_checksum_words: true,
            parses: true
        }
    );
    assert_eq!(
        compare_kit(own, &words, "wsh(other)#cccccccc", stub),
        KitCheck::Different {
            first_difference: 5,
            same_checksum_words: false,
            parses: true
        }
    );
    assert_eq!(
        compare_kit(own, &words, "wsh(own", stub),
        KitCheck::Different {
            first_difference: 7,
            same_checksum_words: false,
            parses: false
        }
    );
}
