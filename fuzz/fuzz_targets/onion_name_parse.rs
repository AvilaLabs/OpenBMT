#![no_main]

use libfuzzer_sys::fuzz_target;
use openbmt_net::onion::{onion_name, parse_onion_name};

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    // A name that is read is exactly the name written for its key.
    if let Some(key) = parse_onion_name(text) {
        assert_eq!(onion_name(&key), text);
    }
    // Any 32 bytes make a name that reads back to them.
    if let Ok(key) = <[u8; 32]>::try_from(data) {
        assert_eq!(parse_onion_name(&onion_name(&key)), Some(key));
    }
});
