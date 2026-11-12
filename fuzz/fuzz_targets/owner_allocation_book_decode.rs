#![no_main]

use libfuzzer_sys::fuzz_target;
use openbmt_core::identity::Identity;
use openbmt_owner::own_book::AllocationBook;

// Any bytes are read strictly or refused, and what is read gives the same
// bytes back (OA-106). The first byte picks whether the rest follows the
// start of a valid book of three ranges for two counters, so that mutated
// bytes get past the header and the ranges' checks.
fuzz_target!(|data: &[u8]| {
    let Some((&mode, rest)) = data.split_first() else {
        return;
    };
    let bytes: Vec<u8> = if mode & 1 == 1 {
        let id = |n: u8| Identity::from_secret_bytes(&[n; 32]).unwrap().device_id();
        let mut book = AllocationBook::new();
        let _ = book.allocate(id(2));
        let _ = book.allocate(id(3));
        let _ = book.allocate(id(2));
        let valid = book.to_bytes();
        let keep = usize::from(mode >> 1).min(valid.len());
        let mut v = valid[..keep].to_vec();
        v.extend_from_slice(rest);
        v
    } else {
        rest.to_vec()
    };
    if let Ok(book) = AllocationBook::from_bytes(&bytes) {
        assert_eq!(book.to_bytes(), bytes);
    }
});
