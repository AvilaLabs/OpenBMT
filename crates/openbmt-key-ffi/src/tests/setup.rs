//! Setup: the key, its words, the wallet, the stored envelope.

use bitcoin::bip32::{Xpriv, Xpub};
use openbmt_core::entropy::words_from_dice_hashed;

use super::kit::*;
use crate::envelope;
use crate::{
    coin_bits, dice_bits, dice_face_counts, EntropySource, KeyApp, KeyError, KeyNetwork,
    KeyWordCount, SetupState,
};

#[test]
fn setup_runs_from_an_empty_store_to_a_wallet() {
    let store = MemStore::new();
    let app = KeyApp::open(store.clone()).expect("opens empty");
    assert_eq!(app.state().expect("state"), SetupState::Empty);
    assert_eq!(store.saves(), 0);
    // Nothing else works yet, and each refusal says what to do.
    assert_eq!(
        app.pair_offer_bytes().expect_err("not ready").rule(),
        "not-ready"
    );
    assert_eq!(
        app.key_words().expect_err("no key").rule(),
        "words-shown-once"
    );
    assert_eq!(app.key2_public().expect_err("no key").rule(), "empty");

    app.create_from_entropy(EntropySource::Platform, KeyNetwork::Regtest)
        .expect("a key is made");
    assert_eq!(store.saves(), 1);
    assert_eq!(
        app.state().expect("state"),
        SetupState::HasIdentity {
            holds_key2: true,
            words_waiting: true
        }
    );

    // The words are shown once and are not in the stored bytes.
    let words = app.key_words().expect("the words");
    assert_eq!(words.len(), 24);
    let again = app.key_words().expect_err("shown once");
    assert_eq!(again.rule(), "words-shown-once");
    assert_eq!(
        app.state().expect("state"),
        SetupState::HasIdentity {
            holds_key2: true,
            words_waiting: false
        }
    );
    let stored = store.saved().expect("stored");
    // The whole phrase is nowhere in the stored bytes.
    let phrase = words.join(" ");
    assert!(!String::from_utf8_lossy(&stored).contains(&phrase));

    // The wallet.
    let (wallet, _, key3) = wallet_with(&app);
    let view = app
        .accept_wallet(setup_for(&wallet, &key3))
        .expect("accepted");
    assert_eq!(
        view.checksum_words,
        wallet.checksum_words().map(String::from).to_vec()
    );
    assert_eq!(view.wallet_id, wallet.checksum32().to_vec());
    assert_eq!(view.slots.len(), 3);
    assert_eq!(
        view.first_receive_address,
        wallet
            .address(openbmt_core::wallet::Keychain::Receive, 0)
            .expect("an address")
            .to_string()
    );
    assert_eq!(view.own_root_words.len(), 4);
    assert_eq!(
        view.owner_root_words,
        owner_id().fingerprint_words().map(String::from).to_vec()
    );
    assert_eq!(store.saves(), 2);
    assert_eq!(
        app.state().expect("state"),
        SetupState::Ready { holds_key2: true }
    );
    // The offer is for this wallet and these two roots.
    let offer =
        openbmt_core::pairing::PairOffer::from_bytes(&app.pair_offer_bytes().expect("offer"))
            .expect("decodes");
    assert_eq!(
        offer.binding(),
        openbmt_core::pairing::PairOffer::new(
            owner_id(),
            openbmt_core::identity::DeviceId::from_bytes(
                &app.device_root().expect("root").try_into().expect("32")
            )
            .expect("root"),
            wallet.checksum32()
        )
        .expect("an offer")
        .binding()
    );
    // Neither step can be done again.
    assert_eq!(
        app.create_from_entropy(EntropySource::Platform, KeyNetwork::Regtest)
            .expect_err("made already")
            .rule(),
        "wallet-set"
    );
    assert_eq!(
        app.accept_wallet(setup_for(&wallet, &key3))
            .expect_err("again")
            .rule(),
        "wallet-set"
    );
}

#[test]
fn key_2_comes_from_the_dice_and_a_fixed_path() {
    let store = MemStore::new();
    let app = KeyApp::open(store).expect("opens");
    let rolls = rolls();
    app.create_from_entropy(
        EntropySource::DiceHashed {
            rolls: rolls.clone(),
        },
        KeyNetwork::Regtest,
    )
    .expect("a key");
    // The same words as the core function, in any tool that follows it.
    let expected: Vec<String> = words_from_dice_hashed(&rolls)
        .expect("words")
        .words()
        .map(String::from)
        .collect();
    assert_eq!(app.key_words().expect("words"), expected);
    // The account key is derived again here, on its own.
    let mnemonic = bip39::Mnemonic::parse(expected.join(" ")).expect("a phrase");
    let seed = mnemonic.to_seed_normalized("");
    let master = Xpriv::new_master(NET, &seed).expect("master");
    let account = master.derive_priv(&secp(), &path()).expect("account");
    let public = app.key2_public().expect("asked").expect("key 2");
    assert_eq!(
        public.master_fingerprint,
        master.fingerprint(&secp()).to_string()
    );
    assert_eq!(
        public.account_xpub,
        Xpub::from_priv(&secp(), &account).to_string()
    );
    assert_eq!(public.origin_path, "48h/1h/0h/2h");
}

#[test]
fn the_other_sources_follow_the_core_functions() {
    let store = MemStore::new();
    let app = KeyApp::open(store).expect("opens");
    // Coin flips: heads then tails is a 1, tails then heads a 0. 128 bits
    // make 12 words; a short run is refused with the core's own text.
    let heads: Vec<bool> = (0..100).map(|i| i % 4 == 1 || i % 4 == 2).collect();
    let progress = coin_bits(heads.clone(), KeyWordCount::Twelve);
    assert_eq!(progress.have, 50);
    assert_eq!(progress.still_needed, 128 - 50);
    let err = app
        .create_from_entropy(
            EntropySource::CoinFlips {
                heads: heads.clone(),
                words: KeyWordCount::Twelve,
            },
            KeyNetwork::Regtest,
        )
        .expect_err("not enough bits");
    assert!(matches!(err, KeyError::Refused { .. }));
    assert_eq!(err.rule(), "entropy");
    assert_eq!(app.state().expect("state"), SetupState::Empty);

    let long: Vec<bool> = (0..600).map(|i| i % 4 == 1 || i % 4 == 2).collect();
    app.create_from_entropy(
        EntropySource::CoinFlips {
            heads: long,
            words: KeyWordCount::Twelve,
        },
        KeyNetwork::Regtest,
    )
    .expect("enough flips");
    assert_eq!(app.key_words().expect("words").len(), 12);

    // Dice, skew-corrected.
    let rolls = "12".repeat(300);
    let progress = dice_bits(rolls.clone(), KeyWordCount::TwentyFour).expect("bits");
    assert_eq!(progress.have, 300);
    app.create_from_entropy(
        EntropySource::DiceSkewCorrected {
            rolls,
            words: KeyWordCount::TwentyFour,
        },
        KeyNetwork::Regtest,
    )
    .expect("a key");
    assert_eq!(app.key_words().expect("words").len(), 24);

    // Dice, hashed: too few rolls are refused.
    let err = app
        .create_from_entropy(
            EntropySource::DiceHashed {
                rolls: "123".to_string(),
            },
            KeyNetwork::Regtest,
        )
        .expect_err("too few");
    assert_eq!(err.rule(), "entropy");
    assert_eq!(
        dice_face_counts("112".to_string()).expect("counts"),
        vec![2, 1, 0, 0, 0, 0]
    );
    assert!(dice_face_counts("7".to_string()).is_err());
}

#[test]
fn making_a_key_again_before_the_wallet_replaces_it() {
    let store = MemStore::new();
    let app = KeyApp::open(store.clone()).expect("opens");
    app.create_from_entropy(EntropySource::Platform, KeyNetwork::Regtest)
        .expect("a key");
    let first = app.key2_public().expect("asked").expect("key 2");
    app.create_from_entropy(EntropySource::Platform, KeyNetwork::Regtest)
        .expect("a key again");
    let second = app.key2_public().expect("asked").expect("key 2");
    assert_ne!(first.account_xpub, second.account_xpub);
    assert_eq!(store.saves(), 2);
    // The stored bytes are the second key.
    let again = KeyApp::open(store).expect("restarts");
    assert_eq!(again.key2_public().expect("asked").expect("key 2"), second);
    assert_eq!(
        again.state().expect("state"),
        SetupState::HasIdentity {
            holds_key2: true,
            words_waiting: false
        }
    );
}

#[test]
fn a_store_that_fails_during_setup_changes_nothing() {
    let store = MemStore::new();
    let app = KeyApp::open(store.clone()).expect("opens");
    store.fail_saves(true);
    let err = app
        .create_from_entropy(EntropySource::Platform, KeyNetwork::Regtest)
        .expect_err("fails");
    assert!(matches!(err, KeyError::Store { .. }));
    assert_eq!(err.rule(), "AS-34");
    assert_eq!(app.state().expect("state"), SetupState::Empty);
    assert!(store.saved().is_none());

    store.fail_saves(false);
    app.create_from_entropy(EntropySource::Platform, KeyNetwork::Regtest)
        .expect("a key");
    let (wallet, _, key3) = wallet_with(&app);
    store.fail_saves(true);
    let before = store.saved();
    let err = app
        .accept_wallet(setup_for(&wallet, &key3))
        .expect_err("fails");
    assert_eq!(err.rule(), "AS-34");
    assert_eq!(store.saved(), before);
    assert!(matches!(
        app.state().expect("state"),
        SetupState::HasIdentity { .. }
    ));
    store.fail_saves(false);
    app.accept_wallet(setup_for(&wallet, &key3))
        .expect("accepted now");
}

#[test]
fn a_wallet_that_does_not_fit_the_key_is_refused_by_the_rule_crates() {
    let store = MemStore::new();
    let app = KeyApp::open(store).expect("opens");
    app.create_from_entropy(EntropySource::Platform, KeyNetwork::Regtest)
        .expect("a key");
    let (wallet, _, key3) = wallet_with(&app);

    // key 2's slot holds another key.
    let (_, s1) = fixed_key(1, 1);
    let (_, s2) = fixed_key(2, 2);
    let (_, s3) = fixed_key(3, 3);
    let other =
        openbmt_core::wallet::WalletDescription::new(NET, [s1, s2, s3.clone()]).expect("a wallet");
    let err = app
        .accept_wallet(setup_for(&other, &s3.xpub))
        .expect_err("not this key");
    assert_eq!(err.rule(), "SR-61");

    // Key 3 is not in the wallet.
    let mut setup = setup_for(&wallet, &key3);
    setup.key3_xpub = Some(fixed_key(9, 3).1.xpub.to_string());
    assert_eq!(app.accept_wallet(setup).expect_err("key 3").rule(), "SR-62");

    // The owner root is the Key app's own.
    let mut setup = setup_for(&wallet, &key3);
    setup.owner_root = app.device_root().expect("root");
    assert!(matches!(
        app.accept_wallet(setup).expect_err("same root"),
        KeyError::Refused { .. }
    ));

    // The owner root is not a key.
    let mut setup = setup_for(&wallet, &key3);
    setup.owner_root = vec![1; 31];
    assert_eq!(app.accept_wallet(setup).expect_err("short").rule(), "owner");
    let mut setup = setup_for(&wallet, &key3);
    setup.owner_root = vec![0; 32];
    assert_eq!(app.accept_wallet(setup).expect_err("zero").rule(), "owner");

    // Not a description, not an xpub, another network.
    let mut setup = setup_for(&wallet, &key3);
    setup.description = "wsh(nothing)".to_string();
    assert_eq!(app.accept_wallet(setup).expect_err("text").rule(), "wallet");
    let mut setup = setup_for(&wallet, &key3);
    setup.key3_xpub = Some("xpub-not".to_string());
    assert_eq!(app.accept_wallet(setup).expect_err("xpub").rule(), "xpub");
    let mut setup = setup_for(&wallet, &key3);
    setup.network = KeyNetwork::Mainnet;
    assert_eq!(
        app.accept_wallet(setup).expect_err("network").rule(),
        "network"
    );

    // Nothing was accepted by any of that.
    assert!(matches!(
        app.state().expect("state"),
        SetupState::HasIdentity { .. }
    ));
    app.accept_wallet(setup_for(&wallet, &key3))
        .expect("the right one");
}

#[test]
fn the_envelope_is_strict() {
    let rig = rig();
    let bytes = rig.store.saved().expect("stored");
    assert_eq!(&bytes[..4], b"OBM7");
    let decoded = envelope::decode(&bytes).expect("decodes");
    let again = envelope::encode(
        &decoded.material,
        decoded.wallet.as_ref().map(|(w, s)| (w, s.as_slice())),
    );
    assert_eq!(again, bytes);

    // Every cut is refused, and so is every extra byte.
    for n in 0..bytes.len() {
        assert!(envelope::decode(&bytes[..n]).is_err(), "cut at {n}");
    }
    let mut longer = bytes.clone();
    longer.push(0);
    assert!(envelope::decode(&longer).is_err());
    // Every flipped bit is refused or reads back as exactly those bytes.
    for i in 0..bytes.len() {
        for bit in 0..8 {
            let mut flipped = bytes.clone();
            flipped[i] ^= 1 << bit;
            if let Ok(d) = envelope::decode(&flipped) {
                let back = envelope::encode(
                    &d.material,
                    d.wallet.as_ref().map(|(w, s)| (w, s.as_slice())),
                );
                assert_eq!(back, flipped, "byte {i} bit {bit}");
            }
        }
    }
    // Another magic and another version are refused.
    let mut other = bytes.clone();
    other[3] = b'8';
    assert!(envelope::decode(&other).is_err());
    let mut other = bytes;
    other[4] = 2;
    assert!(envelope::decode(&other).is_err());
}

#[test]
fn stored_bytes_that_are_wrong_are_refused_at_open() {
    let rig = rig();
    let good = rig.store.saved().expect("stored");

    // Garbage.
    let store = MemStore::new();
    store.set_saved(Some(vec![1, 2, 3]));
    let err = KeyApp::open(store).err().expect("refused");
    assert!(matches!(err, KeyError::Store { .. }));
    assert_eq!(err.rule(), "OBM7");

    // A snapshot that is not the approver's: flip a byte near the end, in
    // the session log or the marks.
    let store = MemStore::new();
    let mut bad = good.clone();
    let at = bad.len() - 3;
    bad[at] ^= 0xff;
    store.set_saved(Some(bad));
    assert!(KeyApp::open(store).is_err());

    // The good bytes open.
    let store = MemStore::new();
    store.set_saved(Some(good));
    assert!(KeyApp::open(store).is_ok());
}
