//! The kernel's random bytes are checked wherever the Key app makes a key,
//! an identity, a salt or a nonce (SS-02): a source that gives zeros, one
//! byte repeated or the same block twice makes nothing.

use std::sync::{Arc, Mutex};

use super::kit::*;
use crate::{EntropySource, KeyApp, KeyNetwork, KeyStore, StoreError};

#[derive(Clone, Copy)]
enum Mode {
    Good,
    Zeros,
    OneByte,
    SameBlock,
}

/// A store whose test blocks follow a mode that the test changes.
struct Scripted {
    inner: Arc<MemStore>,
    mode: Mutex<Mode>,
}

impl Scripted {
    fn new(mode: Mode) -> Arc<Self> {
        Arc::new(Self {
            inner: MemStore::new(),
            mode: Mutex::new(mode),
        })
    }

    fn set(&self, mode: Mode) {
        *self.mode.lock().expect("lock") = mode;
    }
}

impl KeyStore for Scripted {
    fn load(&self) -> Result<Option<Vec<u8>>, StoreError> {
        self.inner.load()
    }

    fn save(&self, bytes: Vec<u8>) -> Result<(), StoreError> {
        self.inner.save(bytes)
    }

    fn test_block(&self) -> Option<[u8; 32]> {
        let mode = *self.mode.lock().expect("lock");
        match mode {
            Mode::Good => self.inner.test_block(),
            Mode::Zeros => Some([0; 32]),
            Mode::OneByte => Some([0x5a; 32]),
            Mode::SameBlock => Some(std::array::from_fn(|i| u8::try_from(i).expect("fits") + 3)),
        }
    }

    fn now(&self) -> u64 {
        self.inner.now()
    }
}

const BAD: [Mode; 3] = [Mode::Zeros, Mode::OneByte, Mode::SameBlock];

#[test]
fn a_bad_source_makes_no_pin_salt() {
    for mode in BAD {
        let store = Scripted::new(mode);
        let app = KeyApp::open(store.clone()).expect("opens");
        let err = app.set_pin(PIN.to_string()).expect_err("refused");
        assert_eq!(err.rule(), "random");
        assert_eq!(store.inner.saves(), 0);
    }
}

#[test]
fn a_bad_source_makes_no_key_two_and_no_identity() {
    for mode in BAD {
        let store = Scripted::new(Mode::Good);
        let app = KeyApp::open(store.clone()).expect("opens");
        app.set_pin(PIN.to_string()).expect("a PIN");
        store.set(mode);
        let err = app
            .create_from_entropy(EntropySource::Platform, KeyNetwork::Regtest)
            .expect_err("no key 2");
        assert_eq!(err.rule(), "random");
        let err = app.create_approver_only().expect_err("no identity");
        assert_eq!(err.rule(), "random");
        assert_eq!(store.inner.saves(), 0);
    }
}

#[test]
fn a_bad_source_makes_no_lock_nonce() {
    for mode in BAD {
        let store = Scripted::new(mode);
        assert!(crate::lock::draw_nonce(store.as_ref()).is_err());
    }
    let store = Scripted::new(Mode::Good);
    assert!(crate::lock::draw_nonce(store.as_ref()).is_ok());
}

#[test]
fn a_good_source_still_makes_keys() {
    let store = Scripted::new(Mode::Good);
    let app = KeyApp::open(store.clone()).expect("opens");
    app.set_pin(PIN.to_string()).expect("a PIN");
    app.create_from_entropy(EntropySource::Platform, KeyNetwork::Regtest)
        .expect("a key");
    assert_eq!(store.inner.saves(), 1);
}
