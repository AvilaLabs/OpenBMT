#![no_main]

use libfuzzer_sys::fuzz_target;
use openbmt_pos::{PaidNotice, PosNotice, PAID_NOTICE_LEN, PAID_NOTICE_TILL_LEN};

fuzz_target!(|data: &[u8]| {
    // A paid notice that decodes is one of the two pinned lengths (the longer
    // names a till sale), encodes back to the same bytes, and checking its
    // signature never panics.
    if let Ok(n) = PaidNotice::from_bytes(data) {
        let want = if n.till_sale.is_some() {
            PAID_NOTICE_TILL_LEN
        } else {
            PAID_NOTICE_LEN
        };
        assert_eq!(data.len(), want);
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
