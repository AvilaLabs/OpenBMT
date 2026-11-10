#![no_main]

use libfuzzer_sys::fuzz_target;
use openbmt_counter::Event;

fuzz_target!(|data: &[u8]| {
    // It must never panic; bytes that are read come back out unchanged.
    if let Ok(decoded) = Event::decode(data) {
        assert_eq!(decoded.encode(), data);
    }
});
