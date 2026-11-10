#![no_main]

use libfuzzer_sys::fuzz_target;
use openbmt_link::{Request, Response};

fuzz_target!(|data: &[u8]| {
    if let Ok(r) = Request::decode(data) {
        assert_eq!(r.encode().unwrap(), data);
    }
    if let Ok(r) = Response::decode(data) {
        assert_eq!(r.encode().unwrap(), data);
    }
});
