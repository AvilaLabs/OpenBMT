#![no_main]

use libfuzzer_sys::fuzz_target;
use openbmt_core::bundle::CounterBundle;

/// What the decoder must hold of a bundle it took: the same bytes back, and,
/// when it carries a relay (version 2, CR-130), the same bundle without the
/// relay is the version 1 of it.
fn check(data: &[u8]) {
    if let Ok(decoded) = CounterBundle::from_bytes(data) {
        assert_eq!(decoded.to_bytes(), data);
        assert_eq!(data[4], if decoded.relay.is_some() { 2 } else { 1 });
        if decoded.relay.is_some() {
            let plain = decoded.clone().with_relay(None).to_bytes();
            assert_eq!(plain[4], 1);
            let again = CounterBundle::from_bytes(&plain).expect("version 1 reads");
            assert!(again.relay.is_none());
            assert_eq!(again.with_relay(decoded.relay.clone()), decoded);
        }
    }
}

fuzz_target!(|data: &[u8]| {
    check(data);
    // The same bytes behind the frame header of each version, so that the
    // input gets past the magic to the parts.
    for version in [1u8, 2] {
        let mut framed = b"OBMs".to_vec();
        framed.push(version);
        framed.extend_from_slice(data);
        check(&framed);
    }
});
