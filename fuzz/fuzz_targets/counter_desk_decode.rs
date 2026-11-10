#![no_main]

use libfuzzer_sys::fuzz_target;
use openbmt_counter::Desk;

fuzz_target!(|data: &[u8]| {
    // It must never panic; bytes that are read come back out unchanged.
    if let Ok(decoded) = Desk::from_bytes(data) {
        assert_eq!(decoded.to_bytes(), data);
    }
});
