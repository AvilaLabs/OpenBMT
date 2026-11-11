//! The PIN lock around the state the Key app stores
//! (`docs/SIGNER-RULES.md` SR-71 to SR-74; the frame, the cost and the PIN
//! rules are `docs/LOCK-RULES.md`, kept by `openbmt-lock`).
//!
//! What this file adds to the shared crate is the store the Key app writes
//! through: every `save` seals the bytes in a locked frame under the held
//! key, with a new nonce each time, and the Android Keystore wraps the frame
//! again outside: the device and the PIN are both needed.

use std::sync::{Arc, Mutex};

pub use openbmt_lock::{
    decode, is_locked, KdfParams, LockedFrame, LockedHeader, HEADER_LEN, MAGIC_LOCKED,
    MAX_PIN_CHARS, MIN_PASSPHRASE_CHARS, MIN_PIN_DIGITS, NONCE_LEN, SALT_LEN,
};
pub(crate) use openbmt_lock::{open, reseal, LockKey, OpenError};

use crate::error::{refused_by, KeyError, StoreError};
use crate::store::KeyStore;

/// What the Key app asks of Argon2 (LC-07): the standard parameters, and
/// small ones in this crate's own tests and under the fuzzer. A release
/// build has neither.
pub(crate) const COST: KdfParams = openbmt_lock::cost!();

/// SR-72: why a PIN is not taken (LC-03).
pub(crate) fn check_pin(pin: &str) -> Result<(), KeyError> {
    openbmt_lock::check_pin(pin).map_err(|e| refused_by(e.rule, e.text))
}

/// What the lock holds between two calls, shared by the Key app and the
/// store it writes through.
pub(crate) type LockCell = Mutex<Option<LockKey>>;

/// The store the Key app writes through: every `save` seals the bytes in a
/// locked frame under the held key (SR-71), and everything else is the
/// platform's.
pub(crate) struct Sealing {
    raw: Arc<dyn KeyStore>,
    cell: Arc<LockCell>,
}

impl Sealing {
    pub(crate) fn new(raw: Arc<dyn KeyStore>, cell: Arc<LockCell>) -> Self {
        Self { raw, cell }
    }
}

fn failed(text: &str) -> StoreError {
    StoreError::Failed {
        text: text.to_string(),
    }
}

/// A nonce from the platform's random source.
pub(crate) fn draw_nonce(raw: &dyn KeyStore) -> Result<[u8; NONCE_LEN], StoreError> {
    let bytes = raw.random(NONCE_LEN as u32)?;
    <[u8; NONCE_LEN]>::try_from(bytes.as_slice())
        .map_err(|_| failed("the random source gave a different number of bytes than asked for"))
}

impl KeyStore for Sealing {
    fn load(&self) -> Result<Option<Vec<u8>>, StoreError> {
        self.raw.load()
    }

    fn save(&self, mut bytes: Vec<u8>) -> Result<(), StoreError> {
        let result = (|| {
            let nonce = draw_nonce(self.raw.as_ref())?;
            let frame = {
                let mut guard = self
                    .cell
                    .lock()
                    .map_err(|_| failed("the lock of the stored state is not usable"))?;
                let key = guard
                    .as_mut()
                    .ok_or_else(|| failed("no PIN is set, so nothing can be stored"))?;
                key.seal_next(nonce, &bytes).ok_or_else(|| {
                    failed("the random source gave the same nonce twice, so nothing was stored")
                })?
            };
            self.raw.save(frame)
        })();
        bytes.fill(0);
        std::hint::black_box(&mut bytes);
        result
    }

    fn random(&self, n: u32) -> Result<Vec<u8>, StoreError> {
        self.raw.random(n)
    }

    fn now(&self) -> u64 {
        self.raw.now()
    }
}
