#![no_main]

use libfuzzer_sys::fuzz_target;
use openbmt_core::policy::PauseTracker;

fuzz_target!(|data: &[u8]| {
    if let Ok(decoded) = PauseTracker::from_bytes(data) {
        assert_eq!(decoded.to_bytes(), data);
    }
});
