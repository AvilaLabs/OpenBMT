#![no_main]

use libfuzzer_sys::fuzz_target;
use openbmt_counter_node::PairingRecord;

/// What the decoder must hold of a record it took: the same bytes back, the
/// version that matches the relay (CR-131), and a record with its relay left
/// out reads as version 1.
fn check(data: &[u8]) {
    if let Ok(decoded) = PairingRecord::from_bytes(data) {
        assert_eq!(decoded.to_bytes(), data);
        assert_eq!(data[4], if decoded.relay.is_some() { 2 } else { 1 });
        if decoded.relay.is_some() {
            let mut plain = decoded.clone();
            plain.relay = None;
            let bytes = plain.to_bytes();
            assert_eq!(bytes[4], 1);
            assert!(PairingRecord::from_bytes(&bytes)
                .expect("version 1 reads")
                .relay
                .is_none());
        }
    }
}

fuzz_target!(|data: &[u8]| {
    check(data);
    // The same bytes behind the frame header of each version.
    for version in [1u8, 2] {
        let mut framed = b"OBMt".to_vec();
        framed.push(version);
        framed.extend_from_slice(data);
        check(&framed);
    }
});
