#![no_main]

use libfuzzer_sys::fuzz_target;
use openbmt_core::pairing_own::OwnOffer;

fn check(data: &[u8]) {
    if let Ok(decoded) = OwnOffer::from_bytes(data) {
        assert_eq!(decoded.to_bytes(), data);
        // CR-227: the main network only, and the first address is derivable
        // (CR-228) for any offer that decodes.
        assert_eq!(decoded.network(), bitcoin::Network::Bitcoin);
        let _ = decoded.first_address();
        let _ = decoded.binding();
    }
}

fuzz_target!(|data: &[u8]| {
    check(data);
    // The same bytes behind the frame header and the main network's code.
    let mut framed = b"OBM-\x01\x00".to_vec();
    framed.extend_from_slice(data);
    check(&framed);
});
