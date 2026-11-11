#![no_main]

use libfuzzer_sys::fuzz_target;
use openbmt_core::relay::SignedRelayHint;

/// A frame that decodes encodes back to the same bytes, and a hint that
/// decoded is one that can be made again (CR-132, CR-133).
fn check(data: &[u8]) {
    if let Ok(decoded) = SignedRelayHint::from_bytes(data) {
        assert_eq!(decoded.to_bytes(), data);
        let hint = &decoded.hint;
        let made = openbmt_core::relay::RelayHint::new(hint.route.clone(), hint.key)
            .expect("a decoded hint is a hint");
        assert_eq!(&made, hint);
        // The check of the signature never panics, whoever it is by.
        let _ = decoded.verify(&hint.device().expect("a key"));
    }
}

fuzz_target!(|data: &[u8]| {
    check(data);
    // The same bytes behind the frame header.
    let mut framed = b"OBMy".to_vec();
    framed.push(1);
    framed.extend_from_slice(data);
    check(&framed);
});
