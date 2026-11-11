//! The Key app's part of the key ceremony (`docs/SIGNER-RULES.md`, SR-64 to
//! SR-69): key 3 taken and kept before the wallet, the public answer, and
//! the wallet setup accepted against the key 3 held.

use std::sync::Arc;

use bitcoin::base58;
use bitcoin::bip32::{Fingerprint, Xpub};
use bitcoin::Network;
use openbmt_core::say::check;
use openbmt_core::wallet::{KeySlot, WalletDescription};
use openbmt_signer::{CeremonyRefusal, KeyAnswer, KeyPart, WalletSetupBytes};

use super::kit::*;
use super::wire::{frame, Rd, W};
use crate::{
    dispatch, envelope, words_for, EntropySource, KeyApp, KeyError, KeyNetwork, Kind, Registry,
    SetupState,
};

/// A Key app with key 2 made, no key 3 yet.
fn fresh() -> (Arc<KeyApp>, Arc<MemStore>) {
    let store = MemStore::new();
    let app = KeyApp::open(store.clone()).expect("opens");
    app.create_from_entropy(EntropySource::Platform, KeyNetwork::Regtest)
        .expect("a key");
    (app, store)
}

/// The fixed key 3 (seed 3) in the form another wallet app exports.
fn key3_part() -> KeyPart {
    let (_, slot) = fixed_key(3, 3);
    KeyPart {
        fingerprint: slot.origin_fingerprint,
        xpub: slot.xpub,
    }
}

fn key3_text() -> String {
    key3_part().origin_text()
}

fn reversioned(part: &KeyPart, version: [u8; 4]) -> String {
    let mut raw = part.xpub.encode().to_vec();
    raw[..4].copy_from_slice(&version);
    base58::encode_check(&raw)
}

/// The wallet of key 1 (seed 1), the app's key 2 and `key3`.
fn wallet_for(app: &KeyApp, key3: &KeyPart) -> WalletDescription {
    let public = app.key2_public().expect("asked").expect("key 2");
    let (_, slot1) = fixed_key(1, 1);
    let slot2 = KeySlot {
        index: 2,
        origin_fingerprint: public
            .master_fingerprint
            .parse::<Fingerprint>()
            .expect("hex"),
        origin_path: path(),
        xpub: public.account_xpub.parse::<Xpub>().expect("an xpub"),
    };
    let slot3 = KeySlot {
        index: 3,
        origin_fingerprint: key3.fingerprint,
        origin_path: path(),
        xpub: key3.xpub,
    };
    WalletDescription::new(NET, [slot1, slot2, slot3]).expect("a wallet")
}

fn setup_bytes(wallet: &WalletDescription) -> Vec<u8> {
    WalletSetupBytes {
        network: NET,
        description: wallet.clone(),
        owner_root: owner_id(),
        batch_min_count: SMALL_BATCH,
    }
    .to_bytes()
}

#[test]
fn sr_67_key_3_is_kept_before_the_wallet_and_comes_back_after_a_restart() {
    let (app, store) = fresh();
    assert_eq!(app.key3_view().expect("asked"), None);
    let view = app.take_key3(key3_text()).expect("taken");
    assert_eq!(view.master_fingerprint, key3_part().fingerprint.to_string());
    assert_eq!(view.origin_path, "48h/1h/0h/2h");
    assert_eq!(view.account_xpub, key3_part().xpub.to_string());
    assert!(!view.mainnet);
    assert_eq!(check(&view.words()), Ok(()));
    assert_eq!(app.key3_view().expect("asked"), Some(view.clone()));
    // Stored before any wallet exists, and the state is still "no wallet".
    let bytes = store.saved().expect("stored");
    assert_eq!(&bytes[..5], b"OBM7\x02");
    let decoded = envelope::decode(&bytes).expect("decodes");
    assert_eq!(decoded.material.key3, Some(key3_part()));
    assert!(decoded.wallet.is_none());
    assert!(matches!(
        app.state().expect("state"),
        SetupState::HasIdentity { .. }
    ));
    // A restart finds it.
    let again = KeyApp::open(store.clone()).expect("opens");
    assert_eq!(again.key3_view().expect("asked"), Some(view));
    // Making key 2 again keeps key 3.
    again
        .create_from_entropy(EntropySource::Platform, KeyNetwork::Regtest)
        .expect("again");
    assert!(again.key3_view().expect("asked").is_some());
    assert_eq!(
        envelope::decode(&store.saved().expect("stored"))
            .expect("decodes")
            .material
            .key3,
        Some(key3_part())
    );
}

#[test]
fn sr_64_both_hardened_marks_and_the_multisig_segwit_forms_are_taken() {
    for text in [
        key3_part().origin_text(),
        format!(
            "[{}/48'/1'/0'/2']{}",
            key3_part().fingerprint,
            key3_part().xpub
        ),
        format!(
            "[{}/48h/1h/0h/2h]{}",
            key3_part().fingerprint,
            reversioned(&key3_part(), [0x02, 0x57, 0x54, 0x83])
        ),
    ] {
        let (app, _) = fresh();
        let view = app.take_key3(text.clone()).expect("taken");
        assert_eq!(view.account_xpub, key3_part().xpub.to_string(), "{text}");
    }
    // The Zpub form on mainnet, on a device with no key 2.
    let master = bitcoin::bip32::Xpriv::new_master(Network::Bitcoin, &[3; 32]).expect("master");
    let account = master
        .derive_priv(
            &secp(),
            &"m/48'/0'/0'/2'"
                .parse::<bitcoin::bip32::DerivationPath>()
                .expect("a path"),
        )
        .expect("account");
    let main = KeyPart {
        fingerprint: master.fingerprint(&secp()),
        xpub: Xpub::from_priv(&secp(), &account),
    };
    let store = MemStore::new();
    let app = KeyApp::open(store).expect("opens");
    app.create_approver_only().expect("identity");
    let zpub = format!(
        "[{}/48h/0h/0h/2h]{}",
        main.fingerprint,
        reversioned(&main, [0x02, 0xAA, 0x7E, 0xD3])
    );
    let view = app.take_key3(zpub).expect("taken");
    assert!(view.mainnet);
    assert_eq!(view.account_xpub, main.xpub.to_string());
}

#[test]
fn sr_64_sr_65_a_refused_text_changes_nothing_and_has_words_of_its_own() {
    let (app, store) = fresh();
    app.take_key3(key3_text()).expect("taken");
    let saved = store.saved();
    let saves = store.saves();
    let k = key3_part();
    let other = {
        let (_, slot) = fixed_key(4, 3);
        KeyPart {
            fingerprint: slot.origin_fingerprint,
            xpub: slot.xpub,
        }
    };
    let zpub = reversioned(&other, [0x04, 0xB2, 0x47, 0x46]);
    let cases: Vec<(String, &str, CeremonyRefusal)> = vec![
        (other.xpub.to_string(), "SR-64", CeremonyRefusal::NoOrigin),
        (
            format!("[{}/84h/1h/0h]{}", other.fingerprint, other.xpub),
            "SR-64",
            CeremonyRefusal::BadOrigin,
        ),
        ("not a key".to_string(), "SR-64", CeremonyRefusal::NotAKey),
        (
            format!("[{}/48h/1h/0h/2h]{zpub}", other.fingerprint),
            "SR-65",
            CeremonyRefusal::SingleSignature,
        ),
        (zpub, "SR-65", CeremonyRefusal::SingleSignature),
        (
            format!(
                "[{}/48h/1h/0h/2h]{}",
                other.fingerprint,
                reversioned(&other, [0x02, 0x95, 0xB4, 0x3F])
            ),
            "SR-65",
            CeremonyRefusal::OtherScript,
        ),
        (
            format!(
                "[{}/48h/1h/0h/2h]{}",
                k.fingerprint,
                bitcoin::bip32::Xpriv::new_master(Network::Regtest, &[3; 32]).expect("m")
            ),
            "SR-64",
            CeremonyRefusal::SecretKey,
        ),
    ];
    for (text, rule, want) in cases {
        let err = app.take_key3(text.clone()).expect_err("refused");
        assert_eq!(err.rule(), rule, "{text}");
        assert_eq!(err.text(), want.to_string(), "{text}");
        assert!(matches!(err, KeyError::Refused { .. }));
        // The words are the refusal's own, not the app's fallback.
        let words = err.words();
        assert_eq!(words, want.words());
        assert_eq!(check(&words), Ok(()));
        assert!(!err.text().contains(&text[..text.len().min(12)]) || text.len() < 12);
    }
    assert_eq!(store.saved(), saved, "nothing was stored");
    assert_eq!(store.saves(), saves);
    assert_eq!(
        app.key3_view()
            .expect("asked")
            .expect("still held")
            .account_xpub,
        k.xpub.to_string()
    );
}

#[test]
fn sr_66_key_3_must_be_for_key_2s_network_and_must_not_be_key_2() {
    let (app, store) = fresh();
    let saves = store.saves();
    // A mainnet key 3 for a regtest key 2.
    let master = bitcoin::bip32::Xpriv::new_master(Network::Bitcoin, &[3; 32]).expect("master");
    let account = master
        .derive_priv(
            &secp(),
            &"m/48'/0'/0'/2'"
                .parse::<bitcoin::bip32::DerivationPath>()
                .expect("a path"),
        )
        .expect("account");
    let main = KeyPart {
        fingerprint: master.fingerprint(&secp()),
        xpub: Xpub::from_priv(&secp(), &account),
    };
    let err = app.take_key3(main.origin_text()).expect_err("refused");
    assert_eq!(err.rule(), "SR-66");
    assert_eq!(err.text(), CeremonyRefusal::NetworkDiffers.to_string());
    assert_eq!(err.words(), CeremonyRefusal::NetworkDiffers.words());
    // Key 2's own key as key 3.
    let public = app.key2_public().expect("asked").expect("key 2");
    let own = format!(
        "[{}/{}]{}",
        public.master_fingerprint, public.origin_path, public.account_xpub
    );
    let err = app.take_key3(own).expect_err("refused");
    assert_eq!(err.rule(), "SR-66");
    assert_eq!(err.text(), CeremonyRefusal::SameAsKey2.to_string());
    assert_eq!(store.saves(), saves);
    assert_eq!(app.key3_view().expect("asked"), None);
}

#[test]
fn sr_67_key_3_is_taken_only_after_the_key_is_made_and_before_the_wallet() {
    let store = MemStore::new();
    let app = KeyApp::open(store.clone()).expect("opens");
    let err = app.take_key3(key3_text()).expect_err("no key yet");
    assert_eq!(err.rule(), "empty");
    assert!(matches!(err, KeyError::State { .. }));
    assert_eq!(store.saves(), 0);
    assert_eq!(app.key3_view().expect_err("no key yet").rule(), "empty");
    // After the wallet is accepted it is not replaced.
    let r = rig();
    let err = r.app.take_key3(key3_text()).expect_err("wallet is set");
    assert_eq!(err.rule(), "wallet-set");
    assert_eq!(check(&err.words()), Ok(()));
    // A newer key 3 replaces an older one before that.
    let (app, _) = fresh();
    app.take_key3(key3_text()).expect("first");
    let (_, slot) = fixed_key(5, 3);
    let newer = KeyPart {
        fingerprint: slot.origin_fingerprint,
        xpub: slot.xpub,
    };
    app.take_key3(newer.origin_text()).expect("replaced");
    assert_eq!(
        app.key3_view().expect("asked").expect("held").account_xpub,
        newer.xpub.to_string()
    );
}

#[test]
fn sr_69_the_public_answer_holds_the_public_parts_and_needs_key_3() {
    let (app, store) = fresh();
    let err = app.public_answer().expect_err("no key 3");
    assert_eq!(err.rule(), "key3-missing");
    assert_eq!(err.words(), CeremonyRefusal::NoKey3.words());
    app.take_key3(key3_text()).expect("taken");
    let bytes = app.public_answer().expect("the answer");
    assert_eq!(&bytes[..5], b"OBMv\x01");
    let answer = KeyAnswer::from_bytes(&bytes).expect("reads");
    let public = app.key2_public().expect("asked").expect("key 2");
    assert_eq!(answer.network, Network::Regtest);
    let key2 = answer.key2.expect("key 2");
    assert_eq!(key2.fingerprint.to_string(), public.master_fingerprint);
    assert_eq!(key2.xpub.to_string(), public.account_xpub);
    assert_eq!(answer.key3, key3_part());
    assert_eq!(
        answer.device_root.to_bytes().to_vec(),
        app.device_root().expect("root")
    );
    // No secret is in it: not the identity, not key 2's account key.
    let find = |needle: &[u8]| bytes.windows(needle.len()).any(|w| w == needle);
    let stored = envelope::decode(&store.saved().expect("stored")).expect("decodes");
    assert!(!find(stored.material.identity.bytes()));
    let k2 = stored.material.key2.as_ref().expect("key 2");
    assert!(!find(&k2.account.private_key.secret_bytes()));
}

#[test]
fn sr_69_a_device_with_no_key_2_answers_without_it() {
    let store = MemStore::new();
    let app = KeyApp::open(store).expect("opens");
    app.create_approver_only().expect("identity");
    app.take_key3(key3_text()).expect("taken");
    let answer = KeyAnswer::from_bytes(&app.public_answer().expect("answer")).expect("reads");
    assert!(answer.key2.is_none());
    assert_eq!(answer.network, Network::Testnet, "the kind only");
    assert_eq!(answer.key3, key3_part());
}

#[test]
fn sr_67_the_wallet_setup_is_accepted_against_the_key_3_held() {
    let (app, store) = fresh();
    app.take_key3(key3_text()).expect("taken");
    let wallet = wallet_for(&app, &key3_part());
    let view = app
        .accept_wallet_setup(setup_bytes(&wallet))
        .expect("accepted");
    // Both sides show the same words and fingerprints.
    assert_eq!(
        view.checksum_words,
        wallet.checksum_words().map(String::from).to_vec()
    );
    assert_eq!(
        view.slots
            .iter()
            .map(|s| s.fingerprint.clone())
            .collect::<Vec<_>>(),
        wallet.fingerprints().map(|f| f.to_string()).to_vec()
    );
    assert_eq!(
        view.owner_root_words,
        owner_id().fingerprint_words().map(String::from).to_vec()
    );
    // SR-67: the wallet is kept, not registered, until it is confirmed.
    assert_eq!(
        app.state().expect("state"),
        SetupState::WalletPending { holds_key2: true }
    );
    let decoded = envelope::decode(&store.saved().expect("stored")).expect("decodes");
    assert_eq!(decoded.material.key3, Some(key3_part()));
    assert!(decoded.wallet.is_none());
    assert_eq!(decoded.pending, Some(setup_bytes(&wallet)));
    app.confirm_wallet().expect("confirmed");
    assert!(matches!(
        app.state().expect("state"),
        SetupState::Ready { holds_key2: true }
    ));
    // Key 3 stays in the stored state, and a restart finds the wallet.
    let decoded = envelope::decode(&store.saved().expect("stored")).expect("decodes");
    assert_eq!(decoded.material.key3, Some(key3_part()));
    assert!(decoded.wallet.is_some());
    assert!(decoded.pending.is_none());
    let again = KeyApp::open(store).expect("opens");
    assert_eq!(again.wallet_view().expect("view"), view);
}

#[test]
fn sr_67_a_wallet_with_another_key_3_is_refused_and_nothing_changes() {
    let (app, store) = fresh();
    app.take_key3(key3_text()).expect("taken");
    let saves = store.saves();
    let (_, slot) = fixed_key(7, 3);
    let swapped = KeyPart {
        fingerprint: slot.origin_fingerprint,
        xpub: slot.xpub,
    };
    let wallet = wallet_for(&app, &swapped);
    let err = app
        .accept_wallet_setup(setup_bytes(&wallet))
        .expect_err("refused");
    assert_eq!(err.rule(), "key3-not-in-wallet");
    assert_eq!(err.text(), CeremonyRefusal::Key3NotInWallet.to_string());
    assert_eq!(err.words(), CeremonyRefusal::Key3NotInWallet.words());
    assert_eq!(store.saves(), saves);
    assert!(matches!(
        app.state().expect("state"),
        SetupState::HasIdentity { .. }
    ));
    // The right account key under another fingerprint is another key.
    let wrong_fp = KeyPart {
        fingerprint: slot.origin_fingerprint,
        xpub: key3_part().xpub,
    };
    let wallet = wallet_for(&app, &wrong_fp);
    let err = app
        .accept_wallet_setup(setup_bytes(&wallet))
        .expect_err("refused");
    assert_eq!(err.rule(), "key3-not-in-wallet");
    // The right wallet is still accepted afterwards.
    let wallet = wallet_for(&app, &key3_part());
    app.accept_wallet_setup(setup_bytes(&wallet))
        .expect("accepted");
}

#[test]
fn sr_67_the_ceremony_setup_is_refused_when_no_key_3_was_taken() {
    let (app, store) = fresh();
    let wallet = wallet_for(&app, &key3_part());
    let err = app
        .accept_wallet_setup(setup_bytes(&wallet))
        .expect_err("refused");
    assert_eq!(err.rule(), "key3-missing");
    assert_eq!(err.text(), CeremonyRefusal::NoKey3.to_string());
    assert_eq!(store.saves(), 1, "only the key was stored");
}

#[test]
fn sr_67_a_key_3_given_with_the_wallet_must_be_the_key_3_held() {
    let (app, _) = fresh();
    app.take_key3(key3_text()).expect("taken");
    let wallet = wallet_for(&app, &key3_part());
    let (_, other) = fixed_key(7, 3);
    let mut setup = setup_for(&wallet, &other.xpub);
    let err = app.accept_wallet(setup.clone()).expect_err("refused");
    assert_eq!(err.rule(), "key3-differs");
    assert_eq!(err.text(), CeremonyRefusal::Key3Differs.to_string());
    // The same key, or none, is taken: the key held is the one used.
    setup.key3_xpub = Some(key3_part().xpub.to_string());
    let (app2, _) = fresh();
    app2.take_key3(key3_text()).expect("taken");
    let wallet2 = wallet_for(&app2, &key3_part());
    let mut setup2 = setup_for(&wallet2, &key3_part().xpub);
    setup2.key3_xpub = None;
    app2.accept_wallet(setup2).expect("held key 3 used");
    let (app3, _) = fresh();
    app3.take_key3(key3_text()).expect("taken");
    let wallet3 = wallet_for(&app3, &key3_part());
    app3.accept_wallet(setup_for(&wallet3, &key3_part().xpub))
        .expect("the same key given");
    // The old way, with nothing held, still works as before.
    let rig = rig();
    assert_eq!(rig.view.slots.len(), 3);
}

#[test]
fn sr_66_the_wallet_must_be_for_the_network_of_key_3_and_key_3_must_not_be_key_2() {
    // A key 3 taken on a device with no key 2, then a wallet for mainnet.
    let store = MemStore::new();
    let app = KeyApp::open(store).expect("opens");
    app.create_approver_only().expect("identity");
    app.take_key3(key3_text()).expect("taken");
    let (_, slot1) = fixed_key(1, 1);
    let (_, slot2) = fixed_key(2, 2);
    let (_, slot3) = fixed_key(3, 3);
    let wallet = WalletDescription::new(NET, [slot1, slot2, slot3]).expect("a wallet");
    // The wallet is for regtest and key 3 for the test kind: fine.
    let mut bytes = setup_bytes(&wallet);
    // Network byte, after the 5 bytes of the frame: 3 regtest, 0 mainnet.
    assert_eq!(bytes[5], 3);
    bytes[5] = 0;
    let err = app.accept_wallet_setup(bytes).expect_err("refused");
    assert_eq!(err.kind(), Kind::Refused);
    app.accept_wallet_setup(setup_bytes(&wallet))
        .expect("accepted for the test kind");
}

#[test]
fn sr_68_a_cut_changed_or_extended_setup_is_refused_without_a_change() {
    let (app, store) = fresh();
    app.take_key3(key3_text()).expect("taken");
    let saves = store.saves();
    let wallet = wallet_for(&app, &key3_part());
    let good = setup_bytes(&wallet);
    for n in 0..good.len() {
        let err = app
            .accept_wallet_setup(good[..n].to_vec())
            .expect_err("cut");
        assert_eq!(err.rule(), "SR-68", "cut at {n}");
    }
    let mut longer = good.clone();
    longer.push(0);
    assert_eq!(
        app.accept_wallet_setup(longer).expect_err("longer").rule(),
        "SR-68"
    );
    // A frame of the other kind: the Key app's own answer.
    let answer = app.public_answer().expect("answer");
    assert_eq!(
        app.accept_wallet_setup(answer)
            .expect_err("not a setup")
            .rule(),
        "SR-68"
    );
    // Reordered: two bytes of the description swapped.
    let mut reordered = good.clone();
    reordered.swap(20, 21);
    assert_ne!(reordered, good);
    assert_eq!(
        app.accept_wallet_setup(reordered)
            .expect_err("reordered")
            .kind(),
        Kind::Refused
    );
    // Every changed byte is refused, until one lands in the owner root or the
    // batch minimum: the wallet is then still the one that was made, and it
    // is accepted with that owner root.
    for i in 0..good.len() {
        let mut flipped = good.clone();
        flipped[i] ^= 0x01;
        match app.accept_wallet_setup(flipped) {
            Err(e) => {
                assert!(matches!(e, KeyError::Refused { .. }), "byte {i}: {e}");
                assert_eq!(store.saves(), saves, "byte {i} changed the store");
            }
            Ok(view) => {
                assert_eq!(
                    view.checksum_words,
                    wallet.checksum_words().map(String::from).to_vec()
                );
                assert!(i >= good.len() - 36, "byte {i} is not in the owner part");
                break;
            }
        }
    }
}

#[test]
fn the_ops_80_to_83_run_the_ceremony_through_the_call_protocol() {
    let store = MemStore::new();
    let reg = Registry::new();
    let call = |op: u16, handle: u64, fields: W| {
        let response = dispatch(&reg, store.clone(), &frame(op, handle, &fields.0));
        assert_eq!(&response[..4], b"OBM9");
        (response[5], response[6..].to_vec())
    };
    let (status, body) = call(1, 0, W::new());
    assert_eq!(status, 0);
    let app = Rd::new(&body).u64();
    let (status, _) = call(
        11,
        app,
        W::new()
            .source(&EntropySource::Platform)
            .network(KeyNetwork::Regtest),
    );
    assert_eq!(status, 0);
    // Key 3 not taken: the view is empty and the answer is refused.
    let (status, body) = call(81, app, W::new());
    assert_eq!((status, body), (0, vec![0]));
    let (status, _) = call(82, app, W::new());
    assert_eq!(status, 1);
    let (status, body) = call(80, app, W::new().string(&key3_text()));
    assert_eq!(status, 0);
    let mut r = Rd::new(&body);
    assert!(!r.bool());
    assert_eq!(r.string(), key3_part().fingerprint.to_string());
    assert_eq!(r.string(), "48h/1h/0h/2h");
    assert_eq!(r.string(), key3_part().xpub.to_string());
    let (status, body) = call(82, app, W::new());
    assert_eq!(status, 0);
    let answer = Rd::new(&body).bytes();
    assert_eq!(&answer[..5], b"OBMv\x01");
    let a = KeyAnswer::from_bytes(&answer).expect("reads");
    // The wallet setup goes back through op 83.
    let key2 = a.key2.expect("key 2");
    let (_, slot1) = fixed_key(1, 1);
    let slot2 = KeySlot {
        index: 2,
        origin_fingerprint: key2.fingerprint,
        origin_path: path(),
        xpub: key2.xpub,
    };
    let (_, slot3) = fixed_key(3, 3);
    let wallet = WalletDescription::new(NET, [slot1, slot2, slot3]).expect("a wallet");
    let bad = {
        let mut b = setup_bytes(&wallet);
        b.truncate(b.len() - 1);
        b
    };
    let (status, _) = call(83, app, W::new().bytes(&bad));
    assert_eq!(status, 1);
    let (status, body) = call(83, app, W::new().bytes(&setup_bytes(&wallet)));
    assert_eq!(status, 0);
    let mut r = Rd::new(&body);
    let words = r.strings();
    assert_eq!(words, wallet.checksum_words().map(String::from).to_vec());
    // A refusal of any of these ops carries the words of the refusal.
    let (status, _) = call(80, app, W::new().string(&key3_text()));
    assert_eq!(status, 3, "the wallet is pending: a state error");
    // Op 84 confirms, and then there is nothing to confirm or discard.
    let (status, body) = call(84, app, W::new());
    assert_eq!((status, body), (0, vec![]));
    let (status, _) = call(84, app, W::new());
    assert_eq!(status, 3);
    let (status, _) = call(85, app, W::new());
    assert_eq!(status, 3);
    let (status, _) = call(80, app, W::new().string(&key3_text()));
    assert_eq!(status, 3, "the wallet is set: a state error");
}

#[test]
fn the_op_85_discards_the_pending_wallet_through_the_call_protocol() {
    let store = MemStore::new();
    let reg = Registry::new();
    let call = |op: u16, handle: u64, fields: W| {
        let response = dispatch(&reg, store.clone(), &frame(op, handle, &fields.0));
        (response[5], response[6..].to_vec())
    };
    let (_, body) = call(1, 0, W::new());
    let app = Rd::new(&body).u64();
    call(
        11,
        app,
        W::new()
            .source(&EntropySource::Platform)
            .network(KeyNetwork::Regtest),
    );
    call(80, app, W::new().string(&key3_text()));
    let (_, body) = call(82, app, W::new());
    let a = KeyAnswer::from_bytes(&Rd::new(&body).bytes()).expect("reads");
    let key2 = a.key2.expect("key 2");
    let (_, slot1) = fixed_key(1, 1);
    let slot2 = KeySlot {
        index: 2,
        origin_fingerprint: key2.fingerprint,
        origin_path: path(),
        xpub: key2.xpub,
    };
    let (_, slot3) = fixed_key(3, 3);
    let wallet = WalletDescription::new(NET, [slot1, slot2, slot3]).expect("a wallet");
    assert_eq!(call(83, app, W::new().bytes(&setup_bytes(&wallet))).0, 0);
    let (_, body) = call(10, app, W::new());
    assert_eq!(body, vec![3, 1], "the wallet is pending, key 2 is held");
    assert_eq!(call(85, app, W::new()), (0, vec![]));
    let (_, body) = call(10, app, W::new());
    assert_eq!(body[..2], [1, 1], "back to an identity that holds key 2");
    // Key 3 is kept: the answer is still there, and the wallet can be accepted again.
    assert_eq!(call(82, app, W::new()).0, 0);
    assert_eq!(call(83, app, W::new().bytes(&setup_bytes(&wallet))).0, 0);
}

#[test]
fn every_refusal_of_the_ceremony_has_words_of_its_own_and_not_the_fallback() {
    let fallback = words_for(Kind::Refused, "a-rule-nobody-wrote", "a text nobody wrote");
    for r in openbmt_signer::ALL_REFUSALS {
        let words = words_for(Kind::Refused, r.rule(), &r.to_string());
        assert_eq!(words, r.words(), "{r:?}");
        assert_ne!(words.heading, fallback.heading, "{r:?}");
        assert_eq!(check(&words), Ok(()));
    }
}

// ---- SR-67: the wallet is kept until the key holder confirms it ------------------------

/// The wallet of key 1 made from `seed`, the app's key 2 and `key3`.
fn wallet_with_key1(app: &KeyApp, key3: &KeyPart, seed: u8) -> WalletDescription {
    let public = app.key2_public().expect("asked").expect("key 2");
    let (_, slot1) = fixed_key(seed, 1);
    let slot2 = KeySlot {
        index: 2,
        origin_fingerprint: public
            .master_fingerprint
            .parse::<Fingerprint>()
            .expect("hex"),
        origin_path: path(),
        xpub: public.account_xpub.parse::<Xpub>().expect("an xpub"),
    };
    let slot3 = KeySlot {
        index: 3,
        origin_fingerprint: key3.fingerprint,
        origin_path: path(),
        xpub: key3.xpub,
    };
    WalletDescription::new(NET, [slot1, slot2, slot3]).expect("a wallet")
}

fn pending() -> (
    Arc<KeyApp>,
    Arc<MemStore>,
    crate::WalletView,
    WalletDescription,
) {
    let (app, store) = fresh();
    app.take_key3(key3_text()).expect("taken");
    let wallet = wallet_for(&app, &key3_part());
    let view = app
        .accept_wallet_setup(setup_bytes(&wallet))
        .expect("accepted");
    (app, store, view, wallet)
}

#[test]
fn sr_67_a_pending_wallet_co_signs_nothing_and_offers_no_pairing() {
    let (app, _, _, _) = pending();
    assert_eq!(
        app.state().expect("state"),
        SetupState::WalletPending { holds_key2: true }
    );
    for err in [
        app.open_session(Vec::new()).err().expect("no session"),
        app.signer_status().expect_err("no status"),
        app.pair_offer_bytes().expect_err("no pairing"),
        app.receive_mark().expect_err("no mark"),
    ] {
        assert_eq!(err.kind(), Kind::State);
        assert_eq!(err.rule(), "wallet-pending");
        assert_eq!(check(&err.words()), Ok(()));
    }
    // The comparison can be shown again at any time.
    assert!(app.wallet_view().is_ok());
    // Nothing else of the setup moves until the wallet is confirmed or discarded.
    for err in [
        app.take_key3(key3_text()).expect_err("no new key 3"),
        app.create_from_entropy(EntropySource::Platform, KeyNetwork::Regtest)
            .expect_err("no new key 2"),
        app.create_approver_only().expect_err("no new identity"),
        app.accept_wallet_setup(setup_bytes(&wallet_for(&app, &key3_part())))
            .expect_err("no second wallet"),
    ] {
        assert_eq!(err.rule(), "wallet-pending");
    }
    // After the confirmation the same calls work.
    app.confirm_wallet().expect("confirmed");
    assert!(app.signer_status().is_ok());
    assert!(app.pair_offer_bytes().is_ok());
}

#[test]
fn sr_67_a_confirmed_wallet_is_the_registered_wallet() {
    let (app, store, view, _) = pending();
    let before = store.saves();
    app.confirm_wallet().expect("confirmed");
    assert_eq!(store.saves(), before + 1, "stored before it returns");
    assert_eq!(app.wallet_view().expect("view"), view);
    // SR-63 applies from now on: the wallet cannot be set again.
    let err = app
        .accept_wallet_setup(setup_bytes(&wallet_for(&app, &key3_part())))
        .expect_err("already set");
    assert_eq!(err.rule(), "wallet-set");
    let again = KeyApp::open(store).expect("opens");
    assert_eq!(
        again.state().expect("state"),
        SetupState::Ready { holds_key2: true }
    );
}

#[test]
fn sr_67_a_discarded_wallet_leaves_the_state_before_it_and_another_can_be_accepted() {
    let (app, store, first, _) = pending();
    let key2 = app.key2_public().expect("asked");
    app.discard_wallet().expect("discarded");
    assert!(matches!(
        app.state().expect("state"),
        SetupState::HasIdentity {
            holds_key2: true,
            ..
        }
    ));
    // Key 2 and key 3 are kept, in memory and in the stored bytes.
    assert_eq!(app.key2_public().expect("asked"), key2);
    assert!(app.key3_view().expect("asked").is_some());
    let bytes = store.saved().expect("stored");
    assert_eq!(&bytes[..5], b"OBM7\x02");
    let decoded = envelope::decode(&bytes).expect("decodes");
    assert!(decoded.wallet.is_none() && decoded.pending.is_none());
    assert_eq!(decoded.material.key3, Some(key3_part()));
    // Another wallet is accepted and confirmed; its words are its own.
    let other = wallet_with_key1(&app, &key3_part(), 5);
    let second = app
        .accept_wallet_setup(setup_bytes(&other))
        .expect("accepted");
    assert_ne!(second.checksum_words, first.checksum_words);
    app.confirm_wallet().expect("confirmed");
    assert_eq!(app.wallet_view().expect("view"), second);
}

#[test]
fn sr_67_a_restart_while_pending_reopens_on_the_pending_wallet() {
    let (app, store, view, _) = pending();
    drop(app);
    let again = KeyApp::open(store.clone()).expect("opens");
    assert_eq!(
        again.state().expect("state"),
        SetupState::WalletPending { holds_key2: true }
    );
    assert_eq!(again.wallet_view().expect("view"), view);
    assert!(again.open_session(Vec::new()).is_err());
    // It is confirmed after the restart, and a second restart finds it registered.
    again.confirm_wallet().expect("confirmed");
    let third = KeyApp::open(store.clone()).expect("opens");
    assert_eq!(
        third.state().expect("state"),
        SetupState::Ready { holds_key2: true }
    );
    // Or it is discarded after a restart.
    let (app, store, _, _) = pending();
    drop(app);
    let again = KeyApp::open(store.clone()).expect("opens");
    again.discard_wallet().expect("discarded");
    let third = KeyApp::open(store).expect("opens");
    assert!(matches!(
        third.state().expect("state"),
        SetupState::HasIdentity { .. }
    ));
    assert!(third.key3_view().expect("asked").is_some());
}

#[test]
fn sr_67_confirm_and_discard_with_nothing_pending_are_refused() {
    let store = MemStore::new();
    let empty = KeyApp::open(store.clone()).expect("opens");
    assert_eq!(
        empty.confirm_wallet().expect_err("nothing").rule(),
        "nothing-pending"
    );
    assert_eq!(
        empty.discard_wallet().expect_err("nothing").rule(),
        "nothing-pending"
    );
    let (app, store) = fresh();
    app.take_key3(key3_text()).expect("taken");
    let saves = store.saves();
    assert_eq!(
        app.confirm_wallet().expect_err("nothing").rule(),
        "nothing-pending"
    );
    assert_eq!(
        app.discard_wallet().expect_err("nothing").rule(),
        "nothing-pending"
    );
    assert_eq!(store.saves(), saves, "nothing was stored");
    let wallet = wallet_for(&app, &key3_part());
    app.accept_wallet_setup(setup_bytes(&wallet))
        .expect("accepted");
    app.confirm_wallet().expect("confirmed");
    // Registered already: there is nothing pending to confirm again or to discard.
    assert_eq!(
        app.confirm_wallet().expect_err("twice").rule(),
        "nothing-pending"
    );
    assert_eq!(
        app.discard_wallet().expect_err("registered").rule(),
        "nothing-pending"
    );
    assert!(matches!(
        app.state().expect("state"),
        SetupState::Ready { .. }
    ));
    for e in [
        app.confirm_wallet().expect_err("x"),
        app.discard_wallet().expect_err("y"),
    ] {
        assert_eq!(check(&e.words()), Ok(()));
    }
}

#[test]
fn sr_67_a_failing_store_changes_nothing_on_confirm_or_discard() {
    let (app, store, _, _) = pending();
    store.fail_saves(true);
    assert_eq!(app.confirm_wallet().expect_err("fails").kind(), Kind::Store);
    assert_eq!(app.discard_wallet().expect_err("fails").kind(), Kind::Store);
    assert!(matches!(
        app.state().expect("state"),
        SetupState::WalletPending { .. }
    ));
    store.fail_saves(false);
    app.confirm_wallet().expect("confirmed");
}

#[test]
fn sr_67_the_pending_envelope_is_strict() {
    let (_, store, _, wallet) = pending();
    let bytes = store.saved().expect("stored");
    assert_eq!(&bytes[..5], b"OBM7\x03");
    let decoded = envelope::decode(&bytes).expect("decodes");
    let pending_bytes = decoded.pending.clone().expect("pending");
    assert_eq!(pending_bytes, setup_bytes(&wallet));
    assert_eq!(
        envelope::encode_pending(&decoded.material, &pending_bytes),
        bytes
    );
    for cut in 0..bytes.len() {
        assert!(envelope::decode(&bytes[..cut]).is_err(), "cut at {cut}");
    }
    let mut longer = bytes.clone();
    longer.push(0);
    assert!(envelope::decode(&longer).is_err());
    // A pending wallet and a registered wallet never come together: the wallet flag is the last
    // byte but one of a pending envelope's tail, and 1 there is refused.
    let mut both = bytes.clone();
    let last = both.len() - 1;
    both[last] = 1;
    assert!(envelope::decode(&both).is_err());
    // A version-3 envelope with no key 3 is refused.
    let mut no_key3 = bytes.clone();
    no_key3[4] = 3;
    let (app, store2) = fresh();
    drop(app);
    let mut v1 = store2.saved().expect("stored");
    v1[4] = 3;
    assert!(envelope::decode(&v1).is_err());
    no_key3.truncate(5);
    assert!(envelope::decode(&no_key3).is_err());
    // A stored pending wallet that the checks no longer accept is not opened.
    let (_, store3, _, _) = pending();
    let mut bad = store3.saved().expect("stored");
    // Flip the network byte of the setup frame (4 bytes of length, then magic, version, network).
    let at = bad
        .windows(5)
        .position(|w| w == b"OBMw\x01")
        .expect("the setup frame");
    bad[at + 5] = 0;
    store3.set_saved(Some(bad));
    assert!(KeyApp::open(store3).is_err());
}
