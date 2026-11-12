//! BK-41 and BK-42: notes.

use openbmt_counter::Event;

use super::fixture::*;
use crate::{Books, RevealsPayments};

fn noted(index: u32, at: u64, note: &str) -> Event {
    Event::Noted {
        index,
        at,
        note: note.to_string(),
    }
}

fn ledger(kit: &Kit) -> String {
    kit.books()
        .full_ledger(RevealsPayments::Acknowledged)
        .expect("ledger")
}

#[test]
fn bk_41_a_sale_holds_its_note_and_the_last_record_stands() {
    let kit = kit_with(&[
        opened(0, FEB_1_0100, 500),
        noted(0, FEB_1_0100, "Tour 12 May"),
        opened(1, FEB_1_0100 + 5, 700),
        noted(1, FEB_1_0100 + 6, "first try"),
        noted(1, FEB_1_0100 + 7, "second try"),
        opened(2, FEB_1_0100 + 8, 900),
    ]);
    let books = kit.books();
    assert_eq!(
        books.sale(&kit.key(0)).unwrap().note.as_deref(),
        Some("Tour 12 May")
    );
    assert_eq!(
        books.sale(&kit.key(1)).unwrap().note.as_deref(),
        Some("second try")
    );
    assert_eq!(books.sale(&kit.key(2)).unwrap().note, None);
}

#[test]
fn bk_41_the_full_ledger_has_the_note_in_the_column_sale_note() {
    let kit = kit_with(&[
        opened(0, FEB_1_0100, 500),
        noted(0, FEB_1_0100, "Tour, 12 May"),
        final_event(0, FEB_1_0100 + 10, SATS_PAID),
        opened(1, FEB_1_0100 + 20, 700),
        opened(2, FEB_1_0100 + 30, 900),
        noted(2, FEB_1_0100 + 31, "=SUM(A1)"),
    ]);
    let ledger = ledger(&kit);
    let header = ledger
        .lines()
        .find(|l| l.starts_with("Type,"))
        .expect("header");
    assert!(
        header.contains(",Tips,Sale note,Till reference,"),
        "{header}"
    );
    let rows: Vec<&str> = ledger.lines().filter(|l| l.starts_with("sale,")).collect();
    assert_eq!(rows.len(), 3);
    // A comma puts the note in quotes, and a formula is made text (BK-21).
    assert!(rows[0].contains(",\"Tour, 12 May\","), "{}", rows[0]);
    assert!(rows[1].ends_with(",,,,,,,"), "{}", rows[1]);
    assert!(rows[2].contains(",'=SUM(A1),"), "{}", rows[2]);
}

#[test]
fn bk_41_the_accounting_export_holds_no_note_and_no_total_changes() {
    let plain = kit_with(&[
        opened(0, FEB_1_0100, 1850),
        final_event(0, FEB_1_0100 + 10, 5),
    ]);
    let with = kit_with(&[
        opened(0, FEB_1_0100, 1850),
        noted(0, FEB_1_0100, "Ana Lopez deposit"),
        final_event(0, FEB_1_0100 + 10, 5),
    ]);
    let a = plain.books().accounting_export().expect("export");
    let b = with.books().accounting_export().expect("export");
    assert_eq!(a, b);
    assert!(!b.iter().any(|(_, f)| f.contains("Ana")));
    assert_eq!(
        plain.books().utc_day_totals().unwrap(),
        with.books().utc_day_totals().unwrap()
    );
}

#[test]
fn bk_42_the_bundle_is_version_four_only_with_a_note_and_reads_back() {
    let kit = kit_with(&[
        opened(0, FEB_1_0100, 500),
        noted(0, FEB_1_0100, "Tour 12 May"),
        final_event(0, FEB_1_0100 + 10, SATS_PAID),
        opened(1, FEB_1_0100 + 20, 700),
    ]);
    let books = kit.books();
    let bytes = books.to_bundle();
    assert_eq!(&bytes[..4], b"OBM6");
    assert_eq!(bytes[4], 4);
    let back = Books::from_bundle(&bytes).expect("bundle");
    assert_eq!(back, books);
    assert_eq!(back.to_bundle(), bytes);
    assert_eq!(back.sales()[0].note.as_deref(), Some("Tour 12 May"));
    assert_eq!(back.sales()[1].note, None);
    for n in 0..bytes.len() {
        assert!(Books::from_bundle(&bytes[..n]).is_err(), "cut at {n}");
    }
    let mut longer = bytes.clone();
    longer.push(0);
    assert!(Books::from_bundle(&longer).is_err());
    // No sale with a note: the bundle is the one earlier builds wrote.
    let plain = kit_with(&[opened(0, FEB_1_0100, 2500), final_event(0, FEB_2_NOON, 5)]);
    let plain_bytes = plain.books().to_bundle();
    assert_eq!(plain_bytes[4], 1);
    // The same bytes marked as version 4 are not a bundle.
    let mut four = plain_bytes.clone();
    four[4] = 4;
    assert!(Books::from_bundle(&four).is_err());
}

#[test]
fn bk_42_a_bundle_with_a_note_that_is_not_text_of_one_to_sixty_four_is_refused() {
    let kit = kit_with(&[opened(0, FEB_1_0100, 500), noted(0, FEB_1_0100, "Tour")]);
    let bytes = kit.books().to_bundle();
    // The note ends the sale, and the sale is the only one: flag, length,
    // text, and then the disposals, returns, staff notes and counter lists
    // that follow all of them.
    let at = bytes
        .windows(6)
        .position(|w| w == [1, 4, b'T', b'o', b'u', b'r'])
        .expect("the note");
    let with = |note: &[u8]| {
        let mut m = bytes[..at].to_vec();
        m.extend_from_slice(note);
        m.extend_from_slice(&bytes[at + 6..]);
        Books::from_bundle(&m)
    };
    assert!(with(&[1, 4, b'T', b'o', b'u', b'r']).is_ok());
    assert!(with(&[1, 0]).is_err());
    assert!(with(&[1, 2, b'a', 0x07]).is_err());
    let mut long = vec![1, 65];
    long.extend(std::iter::repeat_n(b'n', 65));
    assert!(with(&long).is_err());
    // Not UTF-8.
    assert!(with(&[1, 2, 0xff, 0xfe]).is_err());
    // A flag that says there is no note: version 4 with no sale with a note
    // does not encode back to itself.
    assert!(with(&[0]).is_err());
}
