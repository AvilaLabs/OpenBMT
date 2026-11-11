#![no_main]

use libfuzzer_sys::fuzz_target;
use openbmt_counter_node::lightning::LightningRecord;

fuzz_target!(|data: &[u8]| {
    // What reads is what was written: writing it again and reading that
    // gives the same record, and the bytes of the second writing are the
    // bytes of the first.
    if let Ok(decoded) = LightningRecord::from_bytes(data) {
        let again = decoded.to_bytes();
        let back = LightningRecord::from_bytes(&again).expect("a written record reads back");
        assert_eq!(back, decoded);
        assert_eq!(back.to_bytes(), again);
    }
});
