#![no_main]

use libfuzzer_sys::fuzz_target;
use openbmt_core::bundle::CounterBundle;

fuzz_target!(|data: &[u8]| {
    if let Ok(decoded) = CounterBundle::from_bytes(data) {
        assert_eq!(decoded.to_bytes(), data);
    }
});
