#![no_main]

use libfuzzer_sys::fuzz_target;
use openbmt_core::identity::Identity;
use openbmt_core::pairing_own::RangeLedger;

/// Bytes that read as a ledger are re-checked against the pinned owner root
/// (CR-234), so none read under a key that did not sign the ranges. A ledger
/// of no ranges is the one that can, and it hands out nothing.
fn check(data: &[u8]) {
    let owner = Identity::from_secret_bytes(&[1; 32]).unwrap().device_id();
    let counter = Identity::from_secret_bytes(&[3; 32]).unwrap().device_id();
    if let Ok(mut decoded) = RangeLedger::from_bytes(data, owner, counter) {
        assert_eq!(decoded.to_bytes(), data);
        if decoded.signed().is_empty() {
            assert!(decoded.next().is_err());
        }
    }
}

fuzz_target!(|data: &[u8]| {
    check(data);
    let mut framed = b"OBM;\x01".to_vec();
    framed.extend_from_slice(data);
    check(&framed);
});
