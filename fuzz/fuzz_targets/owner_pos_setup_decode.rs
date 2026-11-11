#![no_main]

use libfuzzer_sys::fuzz_target;
use openbmt_owner_node::PosSetup;

fuzz_target!(|data: &[u8]| {
    // It must never panic; bytes that are read come back out unchanged.
    if let Ok(decoded) = PosSetup::from_bytes(data) {
        assert_eq!(decoded.to_bytes(), data);
    }
});
