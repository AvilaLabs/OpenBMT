#![no_main]

use std::path::Path;
use std::sync::OnceLock;

use libfuzzer_sys::fuzz_target;
use openbmt_core::identity::Identity;
use openbmt_core::pinned_wallet::{IndexRange, SignedRange};
use openbmt_owner_node::{PendingRange, RangeOutbox};

/// The bytes of a valid file of two ranges for one counter, made once by
/// the outbox itself in a folder of its own.
fn valid() -> &'static [u8] {
    static VALID: OnceLock<Vec<u8>> = OnceLock::new();
    VALID.get_or_init(|| {
        let owner = Identity::from_secret_bytes(&[1; 32]).unwrap();
        let counter = Identity::from_secret_bytes(&[2; 32]).unwrap().device_id();
        let dir = std::env::temp_dir().join(format!("obm-range-outbox-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut outbox = RangeOutbox::open(&dir).unwrap();
        for start in [0, 1_000] {
            let range = IndexRange::new(counter, start, start + 1_000).unwrap();
            outbox
                .push(PendingRange {
                    counter,
                    range: SignedRange::sign(&owner, range).unwrap(),
                    queued_at: 1_700_000_000,
                })
                .unwrap();
        }
        let bytes = outbox.to_bytes();
        let _ = std::fs::remove_dir_all(&dir);
        bytes
    })
}

// Any bytes are read strictly or refused, and what is read gives the same
// bytes back (LK-72, OA-112). The first byte picks whether the rest follows
// the start of a valid file of two ranges, so that mutated bytes get past
// the header and the entries' checks.
fuzz_target!(|data: &[u8]| {
    let Some((&mode, rest)) = data.split_first() else {
        return;
    };
    let bytes: Vec<u8> = if mode & 1 == 1 {
        let valid = valid();
        let keep = usize::from(mode >> 1).min(valid.len());
        let mut v = valid[..keep].to_vec();
        v.extend_from_slice(rest);
        v
    } else {
        rest.to_vec()
    };
    if let Ok(outbox) = RangeOutbox::from_bytes(Path::new("."), &bytes) {
        assert_eq!(outbox.to_bytes(), bytes);
    }
});
