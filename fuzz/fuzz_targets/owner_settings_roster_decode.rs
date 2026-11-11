#![no_main]

use libfuzzer_sys::fuzz_target;
use openbmt_owner_node::settings_roster::{decode_roster, encode_roster};

fuzz_target!(|data: &[u8]| {
    // It must never panic; a roster that is read comes back out unchanged
    // (LK-76).
    if let Ok(entries) = decode_roster(data) {
        assert_eq!(encode_roster(&entries).as_deref(), Some(data));
    }
});
