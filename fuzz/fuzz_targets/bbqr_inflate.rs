#![no_main]

use libfuzzer_sys::fuzz_target;
use openbmt_bbqr::inflate;

fuzz_target!(|data: &[u8]| {
    // Any bytes as a deflate stream, with the first two bytes as the cap.
    let Some((cap, stream)) = data.split_first_chunk::<2>() else {
        return;
    };
    let cap = usize::from(u16::from_be_bytes(*cap));
    match inflate(stream, cap) {
        // What comes out never passes the cap.
        Ok(out) => assert!(out.len() <= cap),
        Err(e) => assert!(e.rule().starts_with("BQ-")),
    }
});
