#![no_main]

use libfuzzer_sys::fuzz_target;
use openbmt_link::store::{decode_leaving, encode_leaving};

fuzz_target!(|data: &[u8]| {
    // It must never panic; a list that is read comes back out unchanged
    // (LK-42).
    if let Some(list) = decode_leaving(data) {
        assert_eq!(encode_leaving(&list), data);
    }
});
