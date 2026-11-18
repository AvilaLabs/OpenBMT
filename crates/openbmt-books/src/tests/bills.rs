//! BK-44 and BK-45: bills and tabs, and the bundle of version 5.

use openbmt_counter::{Amount, BillEnd, BillKind, Event, PartWay};

use super::fixture::*;
use crate::{Books, RevealsPayments};

fn bill_opened(number: u32, at: u64, kind: BillKind, cents: u64) -> Event {
    Event::BillOpened {
        number,
        at,
        kind,
        amount: Amount(cents),
        currency: usd(),
    }
}

fn part_sale(number: u32, at: u64, sale: u32) -> Event {
    Event::BillPartSale { number, at, sale }
}

fn part_other(number: u32, at: u64, cents: u64, way: PartWay) -> Event {
    Event::BillPartPaid {
        number,
        at,
        amount: Amount(cents),
        way,
    }
}

fn ended(number: u32, at: u64, how: BillEnd, unpaid: u64) -> Event {
    Event::BillEnded {
        number,
        at,
        how,
        unpaid: Amount(unpaid),
    }
}

fn ledger(kit: &Kit) -> String {
    kit.books()
        .full_ledger(RevealsPayments::Acknowledged)
        .expect("ledger")
}

fn bill_key(kit: &Kit, number: u32) -> crate::BillKey {
    crate::BillKey {
        counter: kit.counter_id(),
        number,
    }
}

/// A bill of $30.00 with two sales (one paid, one not) and $12.00 in cash,
/// closed short.
fn short_bill() -> Kit {
    kit_with(&[
        opened(0, FEB_1_0100, 1_000),
        final_event(0, FEB_1_0100 + 10, SATS_PAID),
        opened(1, FEB_1_0100 + 20, 500),
        bill_opened(1, FEB_1_0100 + 30, BillKind::Bill, 3_000),
        part_sale(1, FEB_1_0100 + 32, 0),
        part_other(1, FEB_1_0100 + 33, 1_200, PartWay::Cash),
        part_sale(1, FEB_1_0100 + 34, 1),
        ended(1, FEB_1_0100 + 40, BillEnd::ClosedShort, 800),
    ])
}

// BK-44.

#[test]
fn bk_44_a_bill_is_folded_from_its_records() {
    let kit = short_bill();
    let books = kit.books();
    assert_eq!(books.bills().len(), 1);
    let bill = books.bill(&bill_key(&kit, 1)).expect("the bill");
    assert_eq!(bill.kind, BillKind::Bill);
    assert_eq!(bill.amount_minor, 3_000);
    assert_eq!(bill.parts.len(), 3);
    assert_eq!(bill.opened_at, FEB_1_0100 + 30);
    let end = bill.ended.expect("ended");
    assert_eq!(end.how, BillEnd::ClosedShort);
    assert_eq!(end.unpaid_minor, 800);
    // Only the paid sale counts as bitcoin; the cash is the cashier's word.
    assert_eq!(books.bill_received(bill), Ok((1_000, 1_200)));
    // The records of a bill are not sales: the high-water mark is the
    // sales' highest index, not the bill number.
    let kit2 = kit_with(&[
        opened(0, FEB_1_0100, 100),
        bill_opened(9, FEB_1_0100 + 1, BillKind::Tab, 100),
    ]);
    assert_eq!(kit2.books().sales().len(), 1);
    assert_eq!(kit2.books().bills().len(), 1);
}

#[test]
fn bk_44_a_tab_takes_its_last_amount() {
    let kit = kit_with(&[
        bill_opened(1, FEB_1_0100, BillKind::Tab, 2_000),
        Event::BillAmount {
            number: 1,
            at: FEB_1_0100 + 3,
            amount: Amount(3_500),
        },
        Event::BillAmount {
            number: 1,
            at: FEB_1_0100 + 4,
            amount: Amount(5_000),
        },
    ]);
    let books = kit.books();
    let bill = &books.bills()[0];
    assert_eq!(bill.amount_minor, 5_000);
    assert!(bill.ended.is_none());
}

#[test]
fn bk_44_the_full_ledger_has_one_line_per_bill_after_the_sales() {
    let kit = short_bill();
    let books = kit.books();
    let text = ledger(&kit);
    let types: Vec<&str> = text
        .lines()
        .skip(3)
        .map(|l| l.split(',').next().unwrap_or(""))
        .collect();
    assert_eq!(types, ["sale", "sale", "bill"]);
    let line = text
        .lines()
        .find(|l| l.starts_with("bill,"))
        .expect("the bill");
    // The note names the cashier's word and has no comma; every cell is
    // plain, so the line splits at its commas.
    let row: Vec<&str> = line.split(',').collect();
    assert_eq!(row.len(), 36, "{line}");
    assert_eq!(row[4], "closed short");
    assert_eq!(row[9], "30.00");
    assert_eq!(row[31], "1");
    assert_eq!(row[32], "10.00", "received in bitcoin");
    assert_eq!(row[33], "12.00", "received another way");
    assert_eq!(row[34], "8.00", "left unpaid when closed short");
    let id0 = books.sale(&kit.key(0)).unwrap().id;
    let id1 = books.sale(&kit.key(1)).unwrap().id;
    assert_eq!(row[35], format!("{id0} {id1}"));
    // The cash is written as the cashier's word.
    assert!(
        text.contains("paid another way"),
        "the cashier's word is named"
    );
    // The header names the new columns.
    assert!(text.contains(
        "Bill number,Bill received in bitcoin,Bill received another way,\
         Bill unpaid when closed short,Bill sale IDs"
    ));
}

#[test]
fn bk_44_a_part_paid_another_way_is_never_an_income_line() {
    let kit = kit_with(&[
        bill_opened(1, FEB_1_0100, BillKind::Bill, 1_200),
        part_other(1, FEB_1_0100 + 1, 1_200, PartWay::Card),
        ended(1, FEB_1_0100 + 2, BillEnd::Settled, 0),
    ]);
    let books = kit.books();
    assert!(books.accounting_export().expect("export").is_empty());
    assert!(books.utc_day_totals().expect("totals").is_empty());
    // An unpaid sale part is not received either.
    let kit = short_bill();
    let files = kit.books().accounting_export().expect("export");
    assert_eq!(files[0].1.lines().count(), 2, "the one paid sale only");
}

#[test]
fn bk_44_records_that_do_not_make_a_bill_refuse_the_books() {
    let cases: Vec<Vec<Event>> = vec![
        vec![part_other(1, FEB_1_0100, 100, PartWay::Cash)],
        vec![
            bill_opened(1, FEB_1_0100, BillKind::Bill, 100),
            bill_opened(1, FEB_1_0100 + 1, BillKind::Bill, 100),
        ],
        vec![
            bill_opened(1, FEB_1_0100, BillKind::Bill, 100),
            part_sale(1, FEB_1_0100 + 1, 5),
        ],
        vec![
            opened(0, FEB_1_0100, 100),
            bill_opened(1, FEB_1_0100, BillKind::Bill, 100),
            bill_opened(2, FEB_1_0100, BillKind::Bill, 100),
            part_sale(1, FEB_1_0100 + 1, 0),
            part_sale(2, FEB_1_0100 + 2, 0),
        ],
        vec![
            bill_opened(1, FEB_1_0100, BillKind::Bill, 100),
            ended(1, FEB_1_0100 + 1, BillEnd::Cancelled, 0),
            part_other(1, FEB_1_0100 + 2, 1, PartWay::Cash),
        ],
        vec![
            bill_opened(1, FEB_1_0100, BillKind::Bill, 100),
            Event::BillAmount {
                number: 1,
                at: FEB_1_0100 + 1,
                amount: Amount(200),
            },
        ],
    ];
    for events in cases {
        let kit = kit_with(&events);
        assert!(
            Books::from_chains(&kit.devices, &[(kit.counter_id(), &kit.chain)]).is_err(),
            "{events:?}"
        );
    }
}

// BK-45.

#[test]
fn bk_45_the_bundle_is_version_five_with_a_bill_and_reads_back() {
    for kit in [
        short_bill(),
        kit_with(&[bill_opened(1, FEB_1_0100, BillKind::Tab, 100)]),
    ] {
        let books = kit.books();
        let bytes = books.to_bundle();
        assert_eq!(&bytes[..4], b"OBM6");
        assert_eq!(bytes[4], 5);
        let back = Books::from_bundle(&bytes).expect("bundle");
        assert_eq!(back, books);
        assert_eq!(back.to_bundle(), bytes);
        for n in 0..bytes.len() {
            assert!(Books::from_bundle(&bytes[..n]).is_err(), "cut at {n}");
        }
        let mut longer = bytes.clone();
        longer.push(0);
        assert!(Books::from_bundle(&longer).is_err());
    }
    let back = Books::from_bundle(&short_bill().books().to_bundle()).expect("bundle");
    assert_eq!(back.bills()[0].parts.len(), 3);
}

#[test]
fn bk_45_a_bundle_with_no_bill_is_written_as_before_and_older_versions_still_read() {
    let plain = kit_with(&[opened(0, FEB_1_0100, 2500), final_event(0, FEB_2_NOON, 5)]);
    let bytes = plain.books().to_bundle();
    assert_eq!(bytes[4], 1);
    // Version 4 with a note and no bill stays version 4.
    let noted = kit_with(&[
        opened(0, FEB_1_0100, 500),
        Event::Noted {
            index: 0,
            at: FEB_1_0100,
            note: "Tour".to_string(),
        },
    ]);
    let nb = noted.books().to_bundle();
    assert_eq!(nb[4], 4);
    assert!(Books::from_bundle(&nb).is_ok());
    // Version 5 with nothing in it would not encode back to itself.
    let mut five = bytes.clone();
    five[4] = 5;
    assert!(Books::from_bundle(&five).is_err());
    // A bill whose sale part is not in the bundle, a bill listed twice or out
    // of order and a counter that is not listed are refused.
    let kit = short_bill();
    let books = kit.books();
    let mut bad = books.clone();
    bad.sales.retain(|s| s.key != kit.key(1));
    assert!(Books::from_bundle(&bad.to_bundle()).is_err());
    let mut twice = books.clone();
    twice.bills.push(twice.bills[0].clone());
    assert!(Books::from_bundle(&twice.to_bundle()).is_err());
    let mut zero = books.clone();
    zero.bills[0].amount_minor = 0;
    assert!(Books::from_bundle(&zero.to_bundle()).is_err());
    let mut taken = books.clone();
    let mut again = taken.bills[0].clone();
    again.key.number = 2;
    taken.bills.push(again);
    assert!(
        Books::from_bundle(&taken.to_bundle()).is_err(),
        "a sale in two bills"
    );
    let mut unpaid = books.clone();
    unpaid.bills[0].ended.as_mut().unwrap().unpaid_minor = 0;
    assert!(Books::from_bundle(&unpaid.to_bundle()).is_err());
}
