#![no_main]

use libfuzzer_sys::fuzz_target;
use openbmt_box::till::{clean_reference, parse_amount, request_id_ok};
use openbmt_box::till_listener::{parse_object, parse_request, reject_response, Route, MAX_REQUEST};

fuzz_target!(|data: &[u8]| {
    // The reader of the till listener must never panic, and what it passes
    // must be inside its limits.
    match parse_request(data) {
        Ok(Some(request)) => {
            assert!(request.body.len() <= MAX_REQUEST);
            let end = data.windows(4).position(|w| w == b"\r\n\r\n");
            assert!(end.is_some_and(|e| e + 4 + request.body.len() <= MAX_REQUEST));
            if request.route != Route::Create {
                assert!(request.body.is_empty());
            } else {
                let _ = parse_object(&request.body);
            }
            // The same bytes read the same way.
            assert_eq!(parse_request(data), Ok(Some(request)));
        }
        Ok(None) => assert!(data.len() <= MAX_REQUEST),
        Err(reject) => {
            // A rejection is a status and a fixed body with no words.
            let answer = reject_response(reject);
            assert!(answer.starts_with(b"HTTP/1.1 4"));
            assert!(!answer.windows(5).any(|w| w == b"words"));
        }
    }
    // The body reader, the amount reader and the cleaners on arbitrary
    // bytes.
    let _ = parse_object(data);
    let text = String::from_utf8_lossy(data);
    for digits in [0, 2] {
        let _ = parse_amount(&text, digits);
    }
    assert!(clean_reference(&text).len() <= 40);
    let _ = request_id_ok(&text);
});
