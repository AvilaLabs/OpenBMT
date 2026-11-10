#![no_main]

use libfuzzer_sys::fuzz_target;
use openbmt_session::Response;

fuzz_target!(|data: &[u8]| {
    if let Ok(decoded) = Response::from_bytes(data) {
        assert_eq!(decoded.as_bytes(), data);
    }
});
