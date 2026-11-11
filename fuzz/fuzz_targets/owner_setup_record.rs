#![no_main]

use libfuzzer_sys::fuzz_target;
use openbmt_owner::ceremony::OwnerSetup;

fuzz_target!(|data: &[u8]| {
    // It must never panic; bytes that are read come back out unchanged.
    if let Ok(setup) = OwnerSetup::from_bytes(data) {
        assert_eq!(setup.to_bytes(), data);
    }
});
