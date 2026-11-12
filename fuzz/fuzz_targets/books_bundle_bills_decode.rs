#![no_main]

use libfuzzer_sys::fuzz_target;
use openbmt_books::{Books, MAGIC_BUNDLE, MAX_BILLS};

/// What the decoder must hold of a bundle that carries bills (BK-43, BK-44):
/// the same bytes back, and every bill inside its bounds.
fn check(data: &[u8]) {
    if let Ok(books) = Books::from_bundle(data) {
        assert_eq!(books.to_bundle(), data);
        assert!(books.bills().len() <= MAX_BILLS);
        for bill in books.bills() {
            assert!(bill.parts.len() <= openbmt_books::MAX_BILL_PARTS);
            assert!(bill.amount_minor > 0);
        }
    }
}

fuzz_target!(|data: &[u8]| {
    check(data);
    // The same bytes behind the magic and the version that carries bills (5),
    // so that the input gets past the header to the sales, returns and bills.
    let mut framed = MAGIC_BUNDLE.to_vec();
    framed.push(5);
    framed.extend_from_slice(data);
    check(&framed);
});
