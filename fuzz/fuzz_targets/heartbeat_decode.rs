#![no_main]

use libfuzzer_sys::fuzz_target;
use openbmt_core::batch::PinnedRoots;
use openbmt_core::identity::Identity;
use openbmt_core::heartbeat::SignedHeartbeat;

fuzz_target!(|data: &[u8]| {
    if let Ok(decoded) = SignedHeartbeat::from_bytes(data) {
        assert_eq!(decoded.to_bytes(), data);
        // The check under either key never panics, and a heartbeat made from
        // arbitrary bytes is not signed by keys it has never met (CR-186).
        let id = |n: u8| Identity::from_secret_bytes(&[n; 32]).unwrap().device_id();
        let roots = PinnedRoots::new(id(1), id(2)).unwrap();
        let watch = id(3);
        let _ = decoded.verify(&roots, Some(&watch), &id(4), 0);
        let _ = decoded.verify(&roots, None, &id(4), 0);
    }
});
