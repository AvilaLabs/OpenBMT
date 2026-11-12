#![no_main]

use libfuzzer_sys::fuzz_target;
use openbmt_core::pairing_own::OwnRequest;

fn check(data: &[u8]) {
    if let Ok(decoded) = OwnRequest::from_bytes(data) {
        assert_eq!(decoded.to_bytes(), data);
    }
}

fuzz_target!(|data: &[u8]| {
    check(data);
    let mut framed = b"OBM.\x01".to_vec();
    framed.extend_from_slice(data);
    check(&framed);
});
