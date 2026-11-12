#![no_main]

use libfuzzer_sys::fuzz_target;
use openbmt_core::pairing_own::OwnBundle;

/// What the decoder must hold of a bundle it took: the same bytes back, and
/// the version that matches the relay (CR-230).
fn check(data: &[u8]) {
    if let Ok(decoded) = OwnBundle::from_bytes(data) {
        assert_eq!(decoded.to_bytes(), data);
        assert_eq!(data[4], if decoded.relay.is_some() { 2 } else { 1 });
    }
}

fuzz_target!(|data: &[u8]| {
    check(data);
    // The same bytes behind the frame header of each version, so that the
    // input gets past the magic to the parts.
    for version in [1u8, 2] {
        let mut framed = b"OBM/".to_vec();
        framed.push(version);
        framed.extend_from_slice(data);
        check(&framed);
    }
});
