#![no_main]

use std::sync::{Arc, Mutex};

use libfuzzer_sys::fuzz_target;
use openbmt_key_ffi::{KeyApp, KeyStore, StoreError};

/// An in-memory store that holds the input as the saved state.
struct Mem {
    saved: Mutex<Option<Vec<u8>>>,
}

impl KeyStore for Mem {
    fn load(&self) -> Result<Option<Vec<u8>>, StoreError> {
        Ok(self.saved.lock().unwrap().clone())
    }

    fn save(&self, bytes: Vec<u8>) -> Result<(), StoreError> {
        *self.saved.lock().unwrap() = Some(bytes);
        Ok(())
    }

    fn now(&self) -> u64 {
        10_000_000
    }
}

fuzz_target!(|data: &[u8]| {
    // The first byte picks the version byte of an `OBM7` header put in front
    // of the rest (1 to 4), or leaves the input as it is.
    let bytes = match data.split_first() {
        Some((mode, rest)) if (1..=4).contains(&(mode % 5)) => {
            let mut v = b"OBM7".to_vec();
            v.push(mode % 5);
            v.extend_from_slice(rest);
            v
        }
        _ => data.to_vec(),
    };
    let store = Arc::new(Mem {
        saved: Mutex::new(Some(bytes)),
    });
    // The state is read strictly: it opens, or it is refused; it never panics.
    if let Ok(app) = KeyApp::open(store) {
        let _ = app.state();
        let _ = app.lock_status();
    }
});
