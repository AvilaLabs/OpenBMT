//! BK-43 to BK-48: the till's reference, bills and tabs, the bundle of
//! version 5 and the match against the till's own payments.

use openbmt_counter::{Amount, BillEnd, BillKind, Event, PartWay};

use super::fixture::*;
use crate::say::all_words;
use crate::till::till_words;
use crate::{
    read_till_csv, Books, MappingError, RevealsPayments, SaleKey, TillCsvProblem, TillItemKey,
    TillMapping, MAX_TILL_BYTES, MAX_TILL_LINES,
};

fn referenced(index: u32, at: u64, text: &str) -> Event {
    Event::Referenced {
        index,
        at,
        reference: text.to_string(),
    }
}

fn bill_opened(number: u32, at: u64, kind: BillKind, cents: u64) -> Event {
    Event::BillOpened {
        number,
        at,
        kind,
        amount: Amount(cents),
        currency: usd(),
    }
}

fn bill_ref(number: u32, at: u64, text: &str) -> Event {
    Event::BillReferenced {
        number,
        at,
        reference: text.to_string(),
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
        bill_ref(1, FEB_1_0100 + 31, "CHK-9"),
        part_sale(1, FEB_1_0100 + 32, 0),
        part_other(1, FEB_1_0100 + 33, 1_200, PartWay::Cash),
        part_sale(1, FEB_1_0100 + 34, 1),
        ended(1, FEB_1_0100 + 40, BillEnd::ClosedShort, 800),
    ])
}

// BK-43.

#[test]
fn bk_43_a_sale_holds_its_reference_and_the_first_one_stands() {
    let kit = kit_with(&[
        opened(0, FEB_1_0100, 500),
        referenced(0, FEB_1_0100 + 1, "CHK-17"),
        referenced(0, FEB_1_0100 + 2, "CHK-18"),
        opened(1, FEB_1_0100 + 3, 700),
    ]);
    let books = kit.books();
    assert_eq!(
        books.sale(&kit.key(0)).unwrap().till_reference.as_deref(),
        Some("CHK-17")
    );
    assert_eq!(books.sale(&kit.key(1)).unwrap().till_reference, None);
    // The high-water mark is the sales' own.
    let rows = ledger(&kit);
    assert!(rows.contains("=none") || rows.contains(&format!("={}", 1)));
}

#[test]
fn bk_43_the_full_ledger_has_till_reference_after_sale_note() {
    let kit = kit_with(&[
        opened(0, FEB_1_0100, 500),
        referenced(0, FEB_1_0100 + 1, "A#1/b.c"),
        final_event(0, FEB_1_0100 + 10, SATS_PAID),
        opened(1, FEB_1_0100 + 20, 700),
    ]);
    let ledger = ledger(&kit);
    let header = ledger
        .lines()
        .find(|l| l.starts_with("Type,"))
        .expect("header");
    assert!(header.contains(",Sale note,Till reference,"), "{header}");
    let rows: Vec<&str> = ledger.lines().filter(|l| l.starts_with("sale,")).collect();
    assert_eq!(rows.len(), 2);
    // The cells after the quoted ones are counted from the end: the reference
    // is the sixth from the last.
    let col = |row: &str| {
        let cells: Vec<&str> = row.split(',').collect();
        cells[cells.len() - 6].to_string()
    };
    assert_eq!(col(rows[0]), "A#1/b.c");
    assert_eq!(col(rows[1]), "");
}

#[test]
fn bk_43_the_accounting_export_holds_the_reference_after_the_sale_id() {
    let kit = kit_with(&[
        opened(0, FEB_1_0100, 1850),
        referenced(0, FEB_1_0100 + 1, "CHK-17"),
        final_event(0, FEB_1_0100 + 10, SATS_PAID),
        opened(1, FEB_1_0100 + 20, 700),
        final_event(1, FEB_1_0100 + 30, SATS_PAID),
    ]);
    let books = kit.books();
    let files = books.accounting_export().expect("export");
    let text = &files[0].1;
    let id0 = books.sale(&kit.key(0)).unwrap().id;
    let id1 = books.sale(&kit.key(1)).unwrap().id;
    assert!(
        text.contains(&format!("Sale {id0} ref CHK-17,18.50")),
        "{text}"
    );
    assert!(text.contains(&format!("Sale {id1},7.00")), "{text}");
    // Still three columns, and no amount in satoshis.
    assert!(!text.contains(&SATS_PAID.to_string()));
    for line in text.lines() {
        assert_eq!(line.split(',').count(), 3, "{line}");
    }
    // A sale in a bill with a reference carries the bill's.
    let kit = kit_with(&[
        opened(0, FEB_1_0100, 1850),
        final_event(0, FEB_1_0100 + 10, SATS_PAID),
        bill_opened(1, FEB_1_0100 + 11, BillKind::Bill, 1850),
        bill_ref(1, FEB_1_0100 + 12, "TAB-4"),
        part_sale(1, FEB_1_0100 + 13, 0),
        ended(1, FEB_1_0100 + 14, BillEnd::Settled, 0),
    ]);
    let books = kit.books();
    let id = books.sale(&kit.key(0)).unwrap().id;
    let files = books.accounting_export().expect("export");
    assert!(files[0].1.contains(&format!("Sale {id} ref TAB-4,18.50")));
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
    assert_eq!(bill.reference.as_deref(), Some("CHK-9"));
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
fn bk_44_a_tab_takes_its_last_amount_and_a_second_reference_is_ignored() {
    let kit = kit_with(&[
        bill_opened(1, FEB_1_0100, BillKind::Tab, 2_000),
        bill_ref(1, FEB_1_0100 + 1, "T-1"),
        bill_ref(1, FEB_1_0100 + 2, "T-2"),
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
    assert_eq!(bill.reference.as_deref(), Some("T-1"));
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
    assert_eq!(row.len(), 37, "{line}");
    assert_eq!(row[4], "closed short");
    assert_eq!(row[9], "30.00");
    assert_eq!(row[31], "CHK-9");
    assert_eq!(row[32], "1");
    assert_eq!(row[33], "10.00", "received in bitcoin");
    assert_eq!(row[34], "12.00", "received another way");
    assert_eq!(row[35], "8.00", "left unpaid when closed short");
    let id0 = books.sale(&kit.key(0)).unwrap().id;
    let id1 = books.sale(&kit.key(1)).unwrap().id;
    assert_eq!(row[36], format!("{id0} {id1}"));
    // The cash is written as the cashier's word.
    assert!(
        text.contains("paid another way"),
        "the cashier's word is named"
    );
    // The header names the new columns.
    assert!(text.contains(
        "Till reference,Bill number,Bill received in bitcoin,Bill received another way,\
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
fn bk_45_the_bundle_is_version_five_with_a_reference_or_a_bill_and_reads_back() {
    for kit in [
        short_bill(),
        kit_with(&[
            opened(0, FEB_1_0100, 500),
            referenced(0, FEB_1_0100, "CHK-17"),
            final_event(0, FEB_1_0100 + 10, SATS_PAID),
            opened(1, FEB_1_0100 + 20, 700),
        ]),
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
    assert_eq!(back.bills()[0].reference.as_deref(), Some("CHK-9"));
    assert_eq!(back.bills()[0].parts.len(), 3);
}

#[test]
fn bk_45_a_bundle_with_neither_is_written_as_before_and_older_versions_still_read() {
    let plain = kit_with(&[opened(0, FEB_1_0100, 2500), final_event(0, FEB_2_NOON, 5)]);
    let bytes = plain.books().to_bundle();
    assert_eq!(bytes[4], 1);
    // Version 4 with a note and no reference stays version 4.
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
    let mut weird = books.clone();
    weird.bills[0].reference = Some("has space".to_string());
    assert!(Books::from_bundle(&weird.to_bundle()).is_err());
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

// BK-46.

fn mapping() -> TillMapping {
    TillMapping {
        reference: 0,
        amount: 1,
        payment_type: 2,
        time: 3,
        bitcoin_type: "Bitcoin".to_string(),
        has_header: true,
    }
}

const GOOD: &str = "Check,Amount,Tender,Time\r\n\
CHK-17,18.50,Bitcoin,2025-02-01T01:05:00\r\n\
CHK-18,7.00,Cash,2025-02-01 02:00\r\n\
\"CHK, 19\",5,bitcoin,1738371600\r\n";

#[test]
fn bk_46_the_till_file_is_read_by_the_owners_mapping() {
    let lines = read_till_csv(GOOD.as_bytes(), &mapping(), 0).expect("read");
    assert_eq!(lines.len(), 3);
    assert_eq!(lines[0].reference.as_deref(), Some("CHK-17"));
    assert_eq!(lines[0].amount_micro, 18_500_000);
    assert_eq!(lines[0].line, 2);
    assert!(lines[0].is_bitcoin(&mapping()));
    assert!(!lines[1].is_bitcoin(&mapping()));
    assert_eq!(lines[2].reference.as_deref(), Some("CHK, 19"));
    assert!(
        lines[2].is_bitcoin(&mapping()),
        "capital letters do not matter"
    );
    assert_eq!(lines[2].day.to_string(), "2025-02-01");
    // Unix seconds read at the shop's offset: 01:00 UTC is the day before in
    // New York.
    let ny = read_till_csv(GOOD.as_bytes(), &mapping(), -300).expect("read");
    assert_eq!(ny[2].day.to_string(), "2025-01-31");
    // Without a header the first line is a payment; LF and a missing last
    // newline read too; a byte order mark is skipped.
    let mut m = mapping();
    m.has_header = false;
    let raw = "\u{feff}K1,1.5,Bitcoin,2025-02-01\nK2,2,Card,2025-02-01";
    let lines = read_till_csv(raw.as_bytes(), &m, 0).expect("read");
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0].amount_micro, 1_500_000);
    // A quoted field may hold a line break and a quote.
    let q = "Check,Amount,Tender,Time\r\n\"A\r\nB \"\"x\"\"\",1,Bitcoin,2025-02-01\r\nZ,2,Bitcoin,2025-02-01\r\n";
    let lines = read_till_csv(q.as_bytes(), &mapping(), 0).expect("read");
    assert_eq!(lines[0].reference.as_deref(), Some("A\r\nB \"x\""));
    assert_eq!(lines[1].line, 4, "the line it starts on");
}

#[test]
fn bk_46_a_file_that_does_not_read_is_refused_whole_naming_the_line() {
    let at = |text: &str| {
        read_till_csv(text.as_bytes(), &mapping(), 0)
            .map(|_| ())
            .map_err(|e| (e.line, e.problem))
    };
    let head = "Check,Amount,Tender,Time\r\n";
    let ok = "A,1,Bitcoin,2025-02-01\r\n";
    assert_eq!(
        at(&format!("{head}{ok}B,one,Bitcoin,2025-02-01\r\n")),
        Err((3, TillCsvProblem::BadAmount))
    );
    assert_eq!(
        at(&format!("{head}{ok}{ok}B,1,Bitcoin,yesterday\r\n")),
        Err((4, TillCsvProblem::BadTime))
    );
    assert_eq!(
        at(&format!("{head}{ok}B,1,Bitcoin\r\n")),
        Err((3, TillCsvProblem::TooFewFields))
    );
    assert_eq!(
        at(&format!("{head}{ok}\"B,1,Bitcoin,2025-02-01\r\n")),
        Err((3, TillCsvProblem::UnclosedQuote))
    );
    assert_eq!(
        at(&format!("{head}{ok}\"B\"x,1,Bitcoin,2025-02-01\r\n")),
        Err((3, TillCsvProblem::TextAfterQuote))
    );
    assert_eq!(
        at(&format!("{head}{ok}B\"x,1,Bitcoin,2025-02-01\r\n")),
        Err((3, TillCsvProblem::QuoteInField))
    );
    assert_eq!(
        at(&format!("{head}{ok}\r\n{ok}")),
        Err((3, TillCsvProblem::EmptyLine))
    );
    for bad_amount in [
        "1,5",
        "1.2345678",
        "-",
        "",
        "1e3",
        "$5",
        "99999999999999.00",
    ] {
        assert!(
            at(&format!("{head}A,{bad_amount},Bitcoin,2025-02-01\r\n")).is_err(),
            "{bad_amount}"
        );
    }
    for bad_time in ["2025-13-01", "2025-02-30", "2025-2-1", "1969-12-31", ""] {
        assert!(
            at(&format!("{head}A,1,Bitcoin,{bad_time}\r\n")).is_err(),
            "{bad_time}"
        );
    }
    // Not UTF-8, naming the line of the bad byte.
    let mut bytes = format!("{head}{ok}").into_bytes();
    bytes.extend_from_slice(b"B,1,Bit\xffcoin,2025-02-01\r\n");
    let e = read_till_csv(&bytes, &mapping(), 0).unwrap_err();
    assert_eq!((e.line, e.problem), (3, TillCsvProblem::NotUtf8));
    assert_eq!(e.rule(), "BK-46");
    assert!(e.to_string().contains("line 3"));
    // A mapping that is not allowed.
    let mut m = mapping();
    m.time = 0;
    let e = read_till_csv(GOOD.as_bytes(), &m, 0).unwrap_err();
    assert_eq!(e.problem, TillCsvProblem::Mapping(MappingError::SameColumn));
    assert_eq!(e.line, 0);
    let mut m = mapping();
    m.bitcoin_type = "  ".to_string();
    assert_eq!(m.check(), Err(MappingError::BitcoinType));
}

#[test]
fn bk_46_a_file_is_at_most_eight_mib_and_one_hundred_thousand_lines() {
    let m = TillMapping {
        has_header: false,
        ..mapping()
    };
    let row = "A,1,Bitcoin,2025-02-01\n";
    let full = row.repeat(MAX_TILL_LINES);
    assert_eq!(
        read_till_csv(full.as_bytes(), &m, 0)
            .expect("100,000")
            .len(),
        MAX_TILL_LINES
    );
    let over = format!("{full}{row}");
    let e = read_till_csv(over.as_bytes(), &m, 0).unwrap_err();
    assert_eq!(
        (e.line, e.problem),
        (MAX_TILL_LINES + 1, TillCsvProblem::TooManyLines)
    );
    let big = vec![b'a'; MAX_TILL_BYTES + 1];
    let e = read_till_csv(&big, &m, 0).unwrap_err();
    assert_eq!(e.problem, TillCsvProblem::TooLarge);
    // 8 MiB exactly is not too large (it fails to read for another reason).
    let exact = vec![b'a'; MAX_TILL_BYTES];
    assert_ne!(
        read_till_csv(&exact, &m, 0).unwrap_err().problem,
        TillCsvProblem::TooLarge
    );
    // A lone carriage return ends a line too, as some tills write it, and
    // counts toward the limit the same way.
    let cr = "A,1,Bitcoin,2025-02-01\r".repeat(MAX_TILL_LINES + 1);
    let e = read_till_csv(cr.as_bytes(), &m, 0).unwrap_err();
    assert_eq!(
        (e.line, e.problem),
        (MAX_TILL_LINES + 1, TillCsvProblem::TooManyLines)
    );
}

#[test]
fn bk_46_a_lone_carriage_return_ends_a_line_and_the_lines_named_exist() {
    let m = TillMapping {
        has_header: false,
        ..mapping()
    };
    let lines =
        read_till_csv(b"K1,1,Bitcoin,2025-02-01\rK2,2,Card,2025-02-01\r", &m, 0).expect("read");
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[1].line, 2);
    // A lone carriage return inside quotes counts as a line as well.
    let q = "\"A\rB\",1,Bitcoin,2025-02-01\nZ,x,Bitcoin,2025-02-01\n";
    let e = read_till_csv(q.as_bytes(), &m, 0).unwrap_err();
    assert_eq!((e.line, e.problem), (3, TillCsvProblem::BadAmount));
    // Not UTF-8 after lone carriage returns names the same line.
    let mut bytes = b"K1,1,Bitcoin,2025-02-01\rK2,1,Bitcoin,2025-02-01\r".to_vec();
    bytes.extend_from_slice(b"B,1,Bit\xffcoin,2025-02-01\r");
    let e = read_till_csv(&bytes, &m, 0).unwrap_err();
    assert_eq!((e.line, e.problem), (3, TillCsvProblem::NotUtf8));
}

// BK-47.

fn line(text: &str) -> Vec<crate::TillLine> {
    read_till_csv(
        format!("Check,Amount,Tender,Time\r\n{text}").as_bytes(),
        &mapping(),
        0,
    )
    .expect("read")
}

#[test]
fn bk_47_matched_differs_and_the_line_nothing_paid() {
    let kit = kit_with(&[
        // Paid, reference CHK-17, $18.50: matches.
        opened(0, FEB_1_0100, 1_850),
        referenced(0, FEB_1_0100 + 1, "CHK-17"),
        final_event(0, FEB_1_0100 + 10, SATS_PAID),
        // Paid, reference CHK-20, $20.00: the till says $25.00.
        opened(1, FEB_1_0100 + 20, 2_000),
        referenced(1, FEB_1_0100 + 21, "CHK-20"),
        final_event(1, FEB_1_0100 + 30, SATS_PAID),
        // Paid, reference CHK-99: in no line of the file.
        opened(2, FEB_1_0100 + 40, 900),
        referenced(2, FEB_1_0100 + 41, "CHK-99"),
        final_event(2, FEB_1_0100 + 50, SATS_PAID),
        // Open: never paid, with the reference of a line the till closed as
        // bitcoin.
        opened(3, FEB_1_0100 + 60, 4_000),
        referenced(3, FEB_1_0100 + 61, "CHK-31"),
        // Paid, no reference.
        opened(4, FEB_1_0100 + 70, 1_100),
        final_event(4, FEB_1_0100 + 80, SATS_PAID),
        // Paid bitcoin, but the till closed it as cash.
        opened(5, FEB_1_0100 + 90, 300),
        referenced(5, FEB_1_0100 + 91, "CHK-40"),
        final_event(5, FEB_1_0100 + 100, SATS_PAID),
        // Paid on another day than the file covers.
        opened(6, FEB_2_NOON, 300),
        referenced(6, FEB_2_NOON + 1, "CHK-77"),
        final_event(6, FEB_2_NOON + 10, SATS_PAID),
    ]);
    let books = kit.books();
    let before = books.clone();
    let lines = line(
        "CHK-17,18.50,Bitcoin,2025-02-01T01:05:00\r\n\
         CHK-20,25.00,Bitcoin,2025-02-01T01:10:00\r\n\
         CHK-31,40.00,Bitcoin,2025-02-01T01:20:00\r\n\
         CHK-40,3.00,Cash,2025-02-01T01:30:00\r\n\
         ,12.00,Bitcoin,2025-02-01T01:40:00\r\n\
         CHK-50,8.00,Card,2025-02-01T01:50:00\r\n",
    );
    let found = books.match_till(&lines, &mapping(), 0);
    assert_eq!(books, before, "the match changes no record and no total");
    assert_eq!(found.matched.len(), 1);
    assert_eq!(found.matched[0].reference, "CHK-17");
    assert_eq!(found.matched[0].books_minor, 1_850);
    assert_eq!(found.matched[0].lines, [2]);
    assert_eq!(found.amount_differs.len(), 1);
    assert_eq!(found.amount_differs[0].reference, "CHK-20");
    assert_eq!(found.amount_differs[0].till_micro, 25_000_000);
    assert_eq!(found.amount_differs[0].books_minor, 2_000);
    // A line closed as bitcoin that nothing paid.
    assert_eq!(found.till_without_payment.len(), 1);
    assert_eq!(
        found.till_without_payment[0].reference.as_deref(),
        Some("CHK-31")
    );
    assert_eq!(found.till_without_payment[0].line, 4);
    // A paid sale whose reference is in no line of the file.
    assert_eq!(found.payment_not_in_till.len(), 1);
    assert_eq!(
        found.payment_not_in_till[0].key,
        TillItemKey::Sale(SaleKey {
            counter: kit.counter_id(),
            index: 2
        })
    );
    // Its own lists, to be matched by hand.
    assert_eq!(found.till_without_reference.len(), 1);
    assert_eq!(found.till_without_reference[0].line, 6);
    assert_eq!(found.payment_without_reference.len(), 1);
    // The one the till closed as cash is not lost.
    assert_eq!(found.paid_on_other_type.len(), 1);
    assert_eq!(
        found.paid_on_other_type[0].reference.as_deref(),
        Some("CHK-40")
    );
    assert!(!found.is_empty());
}

#[test]
fn bk_47_a_bill_pairs_by_its_reference_with_what_bitcoin_paid_of_it() {
    // $10.00 in bitcoin and $12.00 in cash on a bill of $30.00 (closed
    // short): the till's bitcoin line for the check is $10.00.
    let kit = short_bill();
    let books = kit.books();
    let ok = line("CHK-9,10.00,Bitcoin,2025-02-01T02:00:00\r\n");
    let found = books.match_till(&ok, &mapping(), 0);
    assert_eq!(found.matched.len(), 1, "{found:?}");
    assert!(matches!(
        found.matched[0].items[0].key,
        TillItemKey::Bill(_)
    ));
    let off = line("CHK-9,30.00,Bitcoin,2025-02-01T02:00:00\r\n");
    let found = books.match_till(&off, &mapping(), 0);
    assert_eq!(found.amount_differs.len(), 1);
    // Several lines of one check are added together.
    let two = line(
        "CHK-9,4.00,Bitcoin,2025-02-01T02:00:00\r\nCHK-9,6.00,Bitcoin,2025-02-01T02:01:00\r\n",
    );
    let found = books.match_till(&two, &mapping(), 0);
    assert_eq!(found.matched.len(), 1);
    assert_eq!(found.matched[0].lines, [2, 3]);
    // A bill of cash only is not a bitcoin payment, and a tab is looked at
    // as a bill is.
    let kit = kit_with(&[
        bill_opened(1, FEB_1_0100, BillKind::Tab, 500),
        bill_ref(1, FEB_1_0100 + 1, "T-1"),
        part_other(1, FEB_1_0100 + 2, 500, PartWay::Cash),
    ]);
    let found = kit
        .books()
        .match_till(&line("X,1.00,Bitcoin,2025-02-01\r\n"), &mapping(), 0);
    assert!(found.payment_not_in_till.is_empty());
    assert_eq!(found.till_without_payment.len(), 1);
}

#[test]
fn bk_47_the_amounts_are_equal_to_the_minor_unit() {
    let kit = kit_with(&[
        opened(0, FEB_1_0100, 1_850),
        referenced(0, FEB_1_0100, "A"),
        final_event(0, FEB_1_0100 + 10, SATS_PAID),
    ]);
    let books = kit.books();
    let go = |amount: &str| {
        books.match_till(
            &line(&format!("A,{amount},Bitcoin,2025-02-01\r\n")),
            &mapping(),
            0,
        )
    };
    assert_eq!(go("18.50").matched.len(), 1);
    assert_eq!(go("18.5").matched.len(), 1);
    assert_eq!(go("18.500").matched.len(), 1);
    assert_eq!(go("18.51").amount_differs.len(), 1);
    assert_eq!(
        go("18.505").amount_differs.len(),
        1,
        "half a cent is not equal"
    );
    assert_eq!(go("-18.50").amount_differs.len(), 1);
}

// BK-48.

#[test]
fn bk_48_the_library_reads_and_matches_and_its_refusals_carry_placeholders() {
    // The reading and the match are functions of their input: the same file
    // and books give the same report.
    let kit = short_bill();
    let books = kit.books();
    let lines = line("CHK-9,10.00,Bitcoin,2025-02-01T02:00:00\r\n");
    assert_eq!(
        books.match_till(&lines, &mapping(), 0),
        books.match_till(&lines, &mapping(), 0)
    );
    // Every refusal of the till's file has words that name its line and its
    // rule, and they are checked with the books' other words.
    let words = till_words();
    assert_eq!(words.len(), 13);
    let names: Vec<String> = all_words().into_iter().map(|(n, _)| n).collect();
    for (name, w) in &words {
        assert!(!w.heading.starts_with("WORDS_NEEDED_"), "{name}");
        assert!(
            names.contains(name),
            "{name} is checked with the other words"
        );
        assert!(w.heading.chars().count() < 28, "{name}");
        assert!(w.what.ends_with('.') && w.next.ends_with('.'), "{name}");
        assert_eq!(w.code, Some("BK-46"));
    }
    assert!(all_words()
        .iter()
        .all(|(_, w)| !w.heading.starts_with("WORDS_NEEDED_")));
}
