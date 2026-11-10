#![no_main]

use libfuzzer_sys::fuzz_target;
use openbmt_core::boxpair::{Claim, Invitation, Welcome};

fuzz_target!(|data: &[u8]| {
    // A claim and a welcome that decode encode back to the same bytes.
    if let Ok(c) = Claim::from_bytes(data) {
        assert_eq!(c.to_bytes(), data);
    }
    if let Ok(w) = Welcome::from_bytes(data) {
        assert_eq!(w.to_bytes(), data);
    }
    // The text of an invitation: it never panics, and one that is read
    // reads back from its own canonical text to the same invitation.
    if let Ok(text) = std::str::from_utf8(data) {
        if let Ok(i) = Invitation::from_text(text) {
            let canonical = i.to_text();
            assert_eq!(canonical.len(), 35);
            assert_eq!(Invitation::from_text(&canonical), Ok(i));
        }
    }
});
