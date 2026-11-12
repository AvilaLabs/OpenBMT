#![no_main]

use libfuzzer_sys::fuzz_target;
use openbmt_core::identity::Identity;
use openbmt_core::pinned_wallet::{RangeBook, SignedRange};

fuzz_target!(|data: &[u8]| {
    if let Ok(decoded) = SignedRange::from_bytes(data) {
        assert_eq!(decoded.to_bytes(), data);
        // Never panics, and a range from arbitrary bytes is not signed by a
        // key it has never met (WL-24).
        let owner = Identity::from_secret_bytes(&[1; 32]).unwrap().device_id();
        assert!(decoded.verify(&owner).is_err());
        let mut book = RangeBook::new(decoded.range.counter);
        let _ = book.accept(&decoded.range);
        let _ = book.next();
    }
});
