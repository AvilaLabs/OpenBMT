#![no_main]

use libfuzzer_sys::fuzz_target;
use openbmt_core::pairing::PairRequest;

fuzz_target!(|data: &[u8]| {
    if let Ok(decoded) = PairRequest::from_bytes(data) {
        assert_eq!(decoded.to_bytes(), data);
    }
});
