//! The key ceremony from end to end (`docs/OWNER-COINS-RULES.md`, OC-40 to
//! OC-46; `docs/SIGNER-RULES.md`, SR-64 to SR-69): the Key app makes key 2
//! and takes key 3, the owner phone makes key 1 and builds the wallet, the
//! Key app accepts the setup, and the owner's node starts from the finished
//! setup. Each side runs through its own ops; the bytes between them are the
//! ones the phones would carry.

use std::sync::{Arc, Mutex};

use bitcoin::bip32::Xpub;
use bitcoin::hashes::{sha256, Hash};
use openbmt_key_ffi::{EntropySource, KeyApp, KeyError, KeyNetwork, KeyStore, StoreError};
use openbmt_signer::KeyPart;

use super::kit::*;
use super::wire::{frame, Rd, W};
use crate::ceremony::{
    build_wallet, finish, make_key1, open, read_answer, stage, BuildArgs, FinishArgs, Key1Args,
};
use crate::{dispatch, AppError, OwnerNetwork};

/// What the Key app's platform gives it, in memory.
struct Store(Mutex<(Option<Vec<u8>>, u64)>);

impl Store {
    fn new() -> Arc<Self> {
        Arc::new(Self(Mutex::new((None, 0))))
    }
}

impl KeyStore for Store {
    fn load(&self) -> Result<Option<Vec<u8>>, StoreError> {
        Ok(self.0.lock().expect("lock").0.clone())
    }
    fn save(&self, bytes: Vec<u8>) -> Result<(), StoreError> {
        self.0.lock().expect("lock").0 = Some(bytes);
        Ok(())
    }
    fn random(&self, n: u32) -> Result<Vec<u8>, StoreError> {
        let mut s = self.0.lock().expect("lock");
        let mut out = Vec::new();
        while out.len() < n as usize {
            s.1 += 1;
            let block =
                sha256::Hash::hash(&[b"ceremony-e2e".as_slice(), &s.1.to_le_bytes()].concat());
            out.extend_from_slice(block.as_byte_array());
        }
        out.truncate(n as usize);
        Ok(out)
    }
    fn now(&self) -> u64 {
        T0
    }
}

/// A Key app with key 2 made.
fn key_app_with_key2() -> Arc<KeyApp> {
    let app = KeyApp::open(Store::new()).expect("opens");
    app.create_from_entropy(EntropySource::Platform, KeyNetwork::Regtest)
        .expect("key 2");
    app
}

/// Key 3 as another wallet app exports it: the test kit's third key.
fn key3() -> KeyPart {
    let k = kit();
    KeyPart {
        fingerprint: k.wallet.slots()[2].origin_fingerprint,
        xpub: k.xpubs[2],
    }
}

fn answer_of(app: &KeyApp) -> Vec<u8> {
    app.take_key3(key3().origin_text()).expect("key 3 taken");
    app.public_answer().expect("the answer")
}

fn started(answer: &[u8]) -> (crate::ceremony::Key1Made, crate::ceremony::WalletBuiltView) {
    let made = make_key1(Key1Args {
        network: OwnerNetwork::Regtest,
        show_words: false,
    })
    .expect("key 1");
    let read = read_answer(answer.to_vec()).expect("answer reads");
    assert!(read.key2.is_some());
    let built = build_wallet(BuildArgs {
        record: made.record.clone(),
        answer: answer.to_vec(),
        key2_text: None,
        batch_min_count: 3,
    })
    .expect("wallet");
    (made, built)
}

#[test]
fn oc_40_to_oc_45_the_ceremony_runs_through_the_ops_and_the_node_starts() {
    let app = key_app_with_key2();
    let answer = answer_of(&app);
    let (made, built) = started(&answer);
    assert!(made.words.is_none(), "key 1 shows no words unless asked");
    assert_eq!(stage(made.record.clone()).expect("stage").stage, 1);
    assert_eq!(stage(built.record.clone()).expect("stage").stage, 2);

    // The Key app accepts the setup, and the words and fingerprints agree.
    let view = app
        .accept_wallet_setup(built.wallet_setup.clone())
        .expect("accepted");
    assert_eq!(view.checksum_words, built.checksum_words);
    let fingerprints: Vec<String> = view.slots.iter().map(|s| s.fingerprint.clone()).collect();
    assert_eq!(fingerprints, built.fingerprints);
    assert_eq!(view.first_receive_address, built.first_receive_address);
    assert_eq!(built.pairs.len(), 3);

    // The owner confirms; the node starts from the record.
    let done = finish(FinishArgs {
        record: built.record.clone(),
        refund_depth: 6,
        birthday: 0,
    })
    .expect("finished");
    let fields = open(done.record.clone()).expect("opens");
    assert_eq!(fields.wallet, built.description);
    assert_eq!(fields.refund_depth, 6);
    let rig = Rig::new();
    let mut config = start_config(rig.dir().to_str().expect("UTF-8"));
    config.owner_secret = fields.owner_secret.clone();
    config.approver_root = fields.approver_root.clone();
    config.wallet = fields.wallet.clone();
    config.key1_account = fields.key1_account.clone();
    config.key3_xpub = fields.key3_xpub.clone();
    config.batch_min_count = fields.batch_min_count;
    config.refund_depth = fields.refund_depth;
    config.birthday = fields.birthday;
    let response = dispatch(rig.registry(), &frame(1, 0, &W::new().start(&config).0));
    assert_eq!(Rd::ok(&response).u64(), 1, "the node started");
}

#[test]
fn oc_40_the_ops_make_key_1_through_dispatch() {
    let rig = Rig::new();
    let args = W::new().u8(3).u8(0).0;
    let response = dispatch(rig.registry(), &frame(80, 0, &args));
    let mut r = Rd::ok(&response);
    assert!(!r.bytes().is_empty(), "the record");
}

#[test]
fn oc_41_a_swapped_key_3_is_refused_by_the_key_app() {
    let app = key_app_with_key2();
    let answer = answer_of(&app);
    let (_, built) = started(&answer);
    // The same wallet with another key 3: the Key app holds the first.
    let other = KeyApp::open(Store::new()).expect("opens");
    other
        .create_from_entropy(EntropySource::Platform, KeyNetwork::Regtest)
        .expect("key 2");
    other
        .take_key3(
            KeyPart {
                fingerprint: kit().wallet.slots()[0].origin_fingerprint,
                xpub: kit().xpubs[0],
            }
            .origin_text(),
        )
        .expect("another key 3");
    let err = other
        .accept_wallet_setup(built.wallet_setup.clone())
        .expect_err("refused");
    assert!(matches!(err, KeyError::Refused { .. }), "{err}");
}

#[test]
fn oc_41_the_wrong_network_a_single_signature_key_and_key_2_as_key_3_are_refused() {
    let app = key_app_with_key2();
    // A single-signature export is not a key.
    let err = app
        .take_key3("zpub6rFR7y4Q2AijBEqTUquhVz398htDFrtymD9xYYfG1m4wAcvPhXNfE3EfH1r1ADqtfSdVCToUG868RvUUkgDKf31mGDtKsAYz2oz2AGutZYs".to_string())
        .expect_err("single-signature");
    assert!(matches!(err, KeyError::Refused { .. }), "{err}");
    // Key 2's own key is not key 3.
    let key2 = app.key2_public().expect("asked").expect("key 2");
    let text = format!(
        "[{}/48h/1h/0h/2h]{}",
        key2.master_fingerprint, key2.account_xpub
    );
    let err = app.take_key3(text).expect_err("key 2 again");
    assert!(matches!(err, KeyError::Refused { .. }), "{err}");
    // A mainnet key on a regtest Key app.
    let main: Xpub = kit().xpubs[2];
    let mut raw = main.encode().to_vec();
    raw[..4].copy_from_slice(&[0x04, 0x88, 0xB2, 0x1E]);
    let text = format!(
        "[{}/48h/0h/0h/2h]{}",
        key3().fingerprint,
        bitcoin::base58::encode_check(&raw)
    );
    let err = app.take_key3(text).expect_err("wrong network");
    assert!(matches!(err, KeyError::Refused { .. }), "{err}");
}

#[test]
fn oc_42_a_cut_or_reordered_setup_is_refused_by_the_key_app() {
    let app = key_app_with_key2();
    let answer = answer_of(&app);
    let (_, built) = started(&answer);
    let good = built.wallet_setup;
    for cut in [1, 5, good.len() / 2, good.len() - 1] {
        let err = app
            .accept_wallet_setup(good[..good.len() - cut].to_vec())
            .expect_err("cut");
        assert!(matches!(err, KeyError::Refused { .. }), "{err}");
    }
    let mut reordered = good.clone();
    reordered.swap(6, 7);
    assert!(app.accept_wallet_setup(reordered).is_err());
    // Nothing above changed the Key app: the right bytes are still taken.
    app.accept_wallet_setup(good).expect("accepted");
}

#[test]
fn oc_44_a_cut_answer_or_record_is_refused_by_the_owner_ops() {
    let app = key_app_with_key2();
    let answer = answer_of(&app);
    let (made, built) = started(&answer);
    for cut in [1, 7, answer.len() / 2] {
        let e: AppError = read_answer(answer[..answer.len() - cut].to_vec()).expect_err("cut");
        assert!(!e.to_string().is_empty());
    }
    for cut in [1, 9] {
        assert!(stage(made.record[..made.record.len() - cut].to_vec()).is_err());
        assert!(open(built.record[..built.record.len() - cut].to_vec()).is_err());
    }
    // A record that is not finished gives no start values.
    assert!(open(built.record.clone()).is_err());
}

#[test]
fn oc_45_the_defaults_are_the_librarys_own_numbers() {
    use crate::ceremony::{defaults, DEFAULT_REFUND_DEPTH};
    use openbmt_counter_node::BATCH_MIN_COUNT;
    use openbmt_owner_node::OwnerParts;
    // The refund depth is the one the node starts with.
    let parts = OwnerParts::new([1; 32], kit().wallet.clone(), counter(), policy()).expect("parts");
    assert_eq!(DEFAULT_REFUND_DEPTH, parts.refund_depth);
    for (network, birthday) in [
        (
            OwnerNetwork::Mainnet,
            openbmt_watch::headers::MAINNET_CHECKPOINT_HEIGHT,
        ),
        (OwnerNetwork::Testnet, 0),
        (OwnerNetwork::Signet, 0),
        (OwnerNetwork::Regtest, 0),
    ] {
        let d = defaults(network).expect("defaults");
        assert_eq!(d.batch_min_count, BATCH_MIN_COUNT);
        assert_eq!(d.refund_depth, 6);
        assert_eq!(d.birthday, birthday);
    }
    // Through dispatch: the network in, three numbers out.
    let rig = Rig::new();
    let response = dispatch(rig.registry(), &frame(86, 0, &W::new().u8(0).0));
    let mut r = Rd::ok(&response);
    assert_eq!(r.u32(), BATCH_MIN_COUNT);
    assert_eq!(r.u32(), 6);
    assert_eq!(r.u32(), 967_680);
    // A network that is not defined is refused.
    let bad = dispatch(rig.registry(), &frame(86, 0, &W::new().u8(9).0));
    assert_ne!(bad.get(5), Some(&0), "refused");
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Key 2 as text, the way a hardware wallet exports it (SR-64): the kit's second key.
fn key2_text() -> String {
    KeyPart {
        fingerprint: kit().wallet.slots()[1].origin_fingerprint,
        xpub: kit().xpubs[1],
    }
    .origin_text()
}

/// The Key app's answers the JVM harness of the Kotlin binding uses: one with key 2, one with none.
fn kotlin_fixtures() -> (Vec<u8>, Vec<u8>) {
    let with = answer_of(&key_app_with_key2());
    let none = KeyApp::open(Store::new()).expect("opens");
    none.create_approver_only().expect("approver only");
    (with, answer_of(&none))
}

#[test]
fn oc_41_the_answers_of_the_kotlin_harness_are_the_key_apps() {
    let (with, none) = kotlin_fixtures();
    if std::env::var_os("OPENBMT_PRINT_FIXTURES").is_some() {
        println!("ANSWER_WITH_KEY2 = {}", hex(&with));
        println!("ANSWER_WITHOUT_KEY2 = {}", hex(&none));
        println!("KEY2_TEXT = {}", key2_text());
    }
    let path = std::path::PathBuf::from(
        std::env::var_os("CARGO_MANIFEST_DIR")
            .expect("cargo sets CARGO_MANIFEST_DIR when it runs tests"),
    )
    .join("bindings/tests/ProtocolDecoderTest.kt");
    let harness = std::fs::read_to_string(&path).expect("the harness");
    for (name, value) in [
        ("ANSWER_WITH_KEY2", hex(&with)),
        ("ANSWER_WITHOUT_KEY2", hex(&none)),
        ("KEY2_TEXT", key2_text()),
    ] {
        assert!(
            harness.contains(&format!("{name} =\n        \"{value}\"")),
            "{name} in ProtocolDecoderTest.kt is not what the Key app makes: run this test with \
             OPENBMT_PRINT_FIXTURES=1 and --nocapture"
        );
    }
    // The answer without key 2 builds a wallet only with key 2 as text.
    let made = make_key1(Key1Args {
        network: OwnerNetwork::Regtest,
        show_words: false,
    })
    .expect("key 1");
    let args = |text: Option<String>| BuildArgs {
        record: made.record.clone(),
        answer: none.clone(),
        key2_text: text,
        batch_min_count: 3,
    };
    assert!(build_wallet(args(None)).is_err());
    build_wallet(args(Some(key2_text()))).expect("key 2 as text");
    // The answer with key 2 is refused when key 2 comes as text too.
    let both = BuildArgs {
        answer: with,
        ..args(Some(key2_text()))
    };
    assert!(build_wallet(both)
        .expect_err("twice")
        .to_string()
        .contains("OC-41"));
}
