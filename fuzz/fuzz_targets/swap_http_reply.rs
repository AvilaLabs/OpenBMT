#![no_main]

use libfuzzer_sys::fuzz_target;
use openbmt_swap::{parse_response, Pairs, Status, MAX_HTTP_BYTES};

fuzz_target!(|data: &[u8]| {
    // The reply of the provider as HTTP bytes. The same bytes give the same
    // answer, and a body that is read is within the limit.
    let first = parse_response(data);
    assert_eq!(first, parse_response(data));
    if let Ok(response) = &first {
        assert!(response.body().len() <= MAX_HTTP_BYTES);
        assert!((200..=599).contains(&response.status()));
    }
    // The same bytes as the body of the pairs call and of the status call.
    let _ = Pairs::parse(data, 1_800_000_000);
    if let Ok(status) = Status::parse(data) {
        assert_eq!(Status::parse(data).as_ref(), Ok(&status));
    }
});
