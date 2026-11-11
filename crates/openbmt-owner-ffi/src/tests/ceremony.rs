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
