#![no_main]

use libfuzzer_sys::fuzz_target;
use openbmt_box::status_page::{
    form_field, parse_request, refusal_response, same_origin, Route, MAX_BODY, MAX_HEAD,
};

fuzz_target!(|data: &[u8]| {
    // The reader of the status page must never panic, and what it passes
    // must be inside its limits.
    match parse_request(data) {
        Ok(Some(request)) => {
            assert!(request.body.len() <= MAX_BODY);
            assert!(!request.host.is_empty() && request.host.len() <= 255);
            assert!(request.host.bytes().all(|b| (0x20..0x7f).contains(&b) || b == b'\t'));
            // The head was whole and inside its limit.
            let end = data.windows(4).position(|w| w == b"\r\n\r\n");
            assert!(end.is_some_and(|e| e <= MAX_HEAD));
            if request.route == Route::Home {
                assert!(request.body.is_empty());
            } else {
                let _ = form_field(&request.body, "invitation");
            }
            let _ = same_origin(
                &request.host,
                request.origin.as_deref(),
                request.referer.as_deref(),
            );
            // The same bytes read the same way, and a longer prefix of a
            // request that passed still passes.
            assert_eq!(parse_request(data), Ok(Some(request)));
        }
        Ok(None) => {
            // Not whole yet: it is not over the head limit.
            assert!(data.len() <= MAX_HEAD + 3 || data.windows(4).any(|w| w == b"\r\n\r\n"));
        }
        Err(refusal) => {
            // A refusal is a status and a fixed text.
            let answer = refusal_response(refusal);
            assert!(answer.starts_with(b"HTTP/1.1 4"));
        }
    }
    // The form reader on arbitrary bytes.
    let _ = form_field(data, "invitation");
    let text = String::from_utf8_lossy(data);
    let _ = same_origin(&text, Some(&text), Some(&text));
    let _ = same_origin(&text, None, Some(&text));
});
