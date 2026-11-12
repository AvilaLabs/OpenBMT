#![no_main]

use libfuzzer_sys::fuzz_target;
use openbmt_counter::{Bills, MAX_BILL_PARTS, MAX_OPEN_BILLS};

/// What the decoder must hold of the bills file (CR-253): the same bytes
/// back, and every bound of the file.
fn check(data: &[u8]) {
    if let Ok(bills) = Bills::from_bytes(data) {
        assert_eq!(bills.to_bytes(), data);
        assert!(bills.next_number() > 0);
        assert!(bills.open_count() <= MAX_OPEN_BILLS);
        let mut last = 0;
        for bill in bills.iter() {
            assert!(bill.number() > last && bill.number() < bills.next_number());
            last = bill.number();
            assert!(bill.parts().len() <= MAX_BILL_PARTS);
            assert!(bill.amount().0 > 0);
            // A bitcoin part names a sale that is a part of this bill only.
            for part in bill.parts() {
                if let openbmt_counter::BillPart::Sale(sale) = part {
                    assert_eq!(bills.bill_of_sale(*sale), Some(bill.number()));
                }
            }
        }
    }
}

fuzz_target!(|data: &[u8]| {
    check(data);
    // The same bytes behind the magic and the version of the file, so that
    // the input gets past the header to the bills.
    let mut framed = b"OBM[".to_vec();
    framed.push(1);
    framed.extend_from_slice(data);
    check(&framed);
});
