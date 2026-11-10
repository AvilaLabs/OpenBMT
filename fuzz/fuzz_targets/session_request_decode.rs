#![no_main]

use libfuzzer_sys::fuzz_target;
use openbmt_session::Request;

fuzz_target!(|data: &[u8]| {
    if let Ok(decoded) = Request::from_bytes(data) {
        assert_eq!(decoded.as_bytes(), data);
    }
});
