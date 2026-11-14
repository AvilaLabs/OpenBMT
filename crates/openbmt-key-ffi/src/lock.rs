//! The PIN lock around the state the Key app stores
//! (`docs/SIGNER-RULES.md` SR-71 to SR-74; the frame, the cost and the PIN
//! rules are `docs/LOCK-RULES.md`, kept by `openbmt-lock`).
//!
//! What this file adds to the shared crate is the store the Key app writes
//! through: every `save` seals the bytes in a locked frame under the held
//! key, with a new nonce each time, and the Android Keystore wraps the frame
//! again outside: the device and the PIN are both needed.

use std::sync::{Arc, Mutex, MutexGuard};

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

/// SR-92: the one turn at the store. Every compare-and-save, and the
/// rewrite of a changed PIN, holds it from reading the stored generation to
/// the end of the write, so that two handles of this process never both
/// find the generation they expect and both write.
pub(crate) fn store_turn() -> MutexGuard<'static, ()> {
    static TURN: Mutex<()> = Mutex::new(());
    TURN.lock().unwrap_or_else(|p| p.into_inner())
}

/// SR-92: the generation of the stored bytes, as read through the platform's
/// store just now, with `key`: 0 when nothing is stored or what is stored
/// is not a locked frame (an older state with no PIN). `Stale` when the
/// stored frame is not under `key`, or does not open under it.
pub(crate) fn stored_generation(raw: &dyn KeyStore, key: &LockKey) -> Result<u64, StoreError> {
    match raw.load()? {
        Some(stored) if is_locked(&stored) => key
            .stored_generation(&stored)
            .map_err(|_| StoreError::Stale),
        _ => Ok(0),
    }
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

/// A nonce from the kernel's checked random bytes.
pub(crate) fn draw_nonce(raw: &dyn KeyStore) -> Result<[u8; NONCE_LEN], StoreError> {
    let mut nonce = [0u8; NONCE_LEN];
    crate::keys::draw(raw, &mut nonce).map_err(|e| failed(&e.to_string()))?;
    Ok(nonce)
}

impl KeyStore for Sealing {
    fn load(&self) -> Result<Option<Vec<u8>>, StoreError> {
        self.raw.load()
    }

    /// SR-92: compare and save. The order is fixed: this handle's turn at
    /// the store is taken; the stored frame is read through the platform's
    /// store, and its generation, which is sealed inside it, is read with
    /// the held key; only when that is the generation this handle opened or
    /// last saved is the new frame, one generation higher, written, and only
    /// after the write is the new generation remembered. The platform's
    /// write (a temporary file and a rename) is atomic, so a reader sees the
    /// old frame or the new one. A different generation writes nothing and
    /// is `Stale`.
    fn save(&self, mut bytes: Vec<u8>) -> Result<(), StoreError> {
        let result = (|| {
            let nonce = draw_nonce(self.raw.as_ref())?;
            let _turn = store_turn();
            let mut guard = self
                .cell
                .lock()
                .map_err(|_| failed("the lock of the stored state is not usable"))?;
            let key = guard
                .as_mut()
                .ok_or_else(|| failed("no PIN is set, so nothing can be stored"))?;
            if stored_generation(self.raw.as_ref(), key)? != key.generation() {
                return Err(StoreError::Stale);
            }
            let next = key
                .generation()
                .checked_add(1)
                .ok_or_else(|| failed("the stored state has been saved too often"))?;
            let frame = key.seal_generation(nonce, next, &bytes).ok_or_else(|| {
                failed("the random source gave the same nonce twice, so nothing was stored")
            })?;
            self.raw.save(frame)?;
            key.set_generation(next);
            Ok(())
        })();
        bytes.fill(0);
        std::hint::black_box(&mut bytes);
        result
    }

    #[cfg(any(test, feature = "test-random"))]
    fn test_block(&self) -> Option<[u8; 32]> {
        self.raw.test_block()
    }

    fn now(&self) -> u64 {
        self.raw.now()
    }
}
