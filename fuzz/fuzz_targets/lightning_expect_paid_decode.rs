#![no_main]

use libfuzzer_sys::fuzz_target;
use openbmt_lightning_node::{expect, paid};

/// Both kept blobs, with re-encode equality: what reads writes back to
/// exactly the input.
fn check(data: &[u8]) {
    if let Some(again) = expect::reencode_for_test(data) {
        assert_eq!(again, data);
    }
    // The paid event's hash is the name it is kept under, not in the bytes.
    if let Some(again) = paid::reencode_for_test([9; 32], data) {
        assert_eq!(again, data);
    }
}

fuzz_target!(|data: &[u8]| {
    check(data);
    // The same bytes behind the header of each, so that the input gets past
    // the version byte: version 1 and the count, and version 1 for an event.
    let mut book = vec![1u8];
    let entries = (data.len() / 48) as u32;
    book.extend_from_slice(&entries.to_be_bytes());
    book.extend_from_slice(data);
    check(&book);
    let mut event = vec![1u8];
    event.extend_from_slice(data);
    check(&event);
});
