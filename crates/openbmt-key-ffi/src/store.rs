//! The app's side of the Key app: storage, randomness and the clock.

use crate::error::StoreError;

/// The store of a Key app between two calls of the call protocol: nothing
/// is kept, nothing is drawn, and the clock reads 0.
pub(crate) struct Detached;

impl KeyStore for Detached {
    fn load(&self) -> Result<Option<Vec<u8>>, StoreError> {
        Err(detached())
    }

    fn save(&self, _bytes: Vec<u8>) -> Result<(), StoreError> {
        Err(detached())
    }

    fn random(&self, _n: u32) -> Result<Vec<u8>, StoreError> {
        Err(detached())
    }

    fn now(&self) -> u64 {
        0
    }
}

fn detached() -> StoreError {
    StoreError::Failed {
        text: "no store is attached outside a call".to_string(),
    }
}

/// What the Android app implements. The Rust side never touches the file
/// system, the clock or a random source itself.
///
/// The bytes handed to `save` are the `OBM7` envelope: they hold the
/// identity secret and, when this device holds key 2, key 2's account
/// secret. The Rust side does not protect them at rest. Wrapping them with
/// a key from the Android Keystore, keeping them out of backups and out of
/// device transfer, and never writing them anywhere else, is the app's job.
pub trait KeyStore: Send + Sync {
    /// The bytes last passed to `save`, or `None` on a first start.
    fn load(&self) -> Result<Option<Vec<u8>>, StoreError>;

    /// Store `bytes`. The bytes must be durable (flushed to storage and
    /// synced) before this returns without error; the Key app releases
    /// a signature or a response only after that (AS-34). On any error
    /// the app's old bytes must still be what `load` returns.
    fn save(&self, bytes: Vec<u8>) -> Result<(), StoreError>;

    /// `n` bytes from the platform's cryptographically secure random
    /// source. Fewer or more bytes than asked for are refused.
    fn random(&self, n: u32) -> Result<Vec<u8>, StoreError>;

    /// The time now, in seconds since 1970-01-01 UTC.
    fn now(&self) -> u64;
}
