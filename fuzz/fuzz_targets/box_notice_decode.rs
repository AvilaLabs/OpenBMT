#![no_main]

use libfuzzer_sys::fuzz_target;
use openbmt_pos::{PaidNotice, PosNotice, PAID_NOTICE_LEN};

fuzz_target!(|data: &[u8]| {
    // A paid notice that decodes is the pinned length, encodes back to the
    // same bytes, and checking its signature never panics.
    if let Ok(n) = PaidNotice::from_bytes(data) {
        assert_eq!(data.len(), PAID_NOTICE_LEN);
        assert_eq!(n.to_bytes(), data);
        let _ = n.verify();
        let _ = n.sale();
    }
    // The same for the Box's POS notice, whose text never panics.
    if let Ok(n) = PosNotice::from_bytes(data) {
        assert_eq!(n.to_bytes(), data);
        let _ = n.verify();
        let _ = n.text();
    }
});
