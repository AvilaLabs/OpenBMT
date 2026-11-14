#![no_main]

use libfuzzer_sys::fuzz_target;
use openbmt_lightning_node::{expect, funding, paid};

/// The kept blobs, with re-encode equality: what reads writes back to
/// exactly the input.
fn check(data: &[u8]) {
    if let Some(again) = expect::reencode_for_test(data) {
        assert_eq!(again, data);
    }
    // The paid event's hash is the name it is kept under, not in the bytes.
    if let Some(again) = paid::reencode_for_test([9; 32], data) {
        assert_eq!(again, data);
    }
    if let Some(again) = funding::reencode_for_test(data) {
        assert_eq!(again, data);
    }
}

fuzz_target!(|data: &[u8]| {
    check(data);
    // The same bytes behind the header of each, so that the input gets past
    // the version byte: version 1 and the count, and versions 1 to 3 for an
    // event (the funding blob has version 1 as well).
    let mut book = vec![1u8];
    let entries = (data.len() / 48) as u32;
    book.extend_from_slice(&entries.to_be_bytes());
    book.extend_from_slice(data);
    check(&book);
    for version in 1u8..=3 {
        let mut event = vec![version];
        event.extend_from_slice(data);
        check(&event);
    }
});
