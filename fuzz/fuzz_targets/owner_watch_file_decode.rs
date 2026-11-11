#![no_main]

use libfuzzer_sys::fuzz_target;
use openbmt_owner_ffi::watch::decode_file;

fuzz_target!(|data: &[u8]| {
    // It must never panic; bytes that are read come back out unchanged (OA-81).
    if let Ok(start) = decode_file(data) {
        assert_eq!(start.to_file(), data);
    }
});
