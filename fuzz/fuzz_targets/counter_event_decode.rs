#![no_main]

use libfuzzer_sys::fuzz_target;
use openbmt_counter::{Event, StaffEvent};

fuzz_target!(|data: &[u8]| {
    // It must never panic; bytes that are read come back out unchanged.
    if let Ok(decoded) = Event::decode(data) {
        assert_eq!(decoded.encode(), data);
    }
    // The record of a change to the cashiers (CR-143) is read as strictly: a
    // body that is read encodes to the input, and is no sale event.
    if let Ok(decoded) = StaffEvent::decode(data) {
        assert_eq!(decoded.encode(), data);
        assert!(Event::decode(data).is_err());
    }
});
