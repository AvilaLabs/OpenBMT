#![no_main]

use libfuzzer_sys::fuzz_target;
use openbmt_pos::{reference_ok, TillMessage, MAX_TILL_REFERENCE};

fuzz_target!(|data: &[u8]| {
    // What the Box sends a counter for a till sale: a message that decodes
    // holds a reference inside its bound, an amount above zero, and encodes
    // back to the same bytes.
    if let Ok(message) = TillMessage::from_bytes(data) {
        assert_eq!(message.to_bytes(), data);
        if let TillMessage::Request(r) = &message {
            assert!(r.amount.0 > 0);
            assert!(r.reference.len() <= MAX_TILL_REFERENCE);
            assert!(reference_ok(&r.reference));
        }
    }
});
