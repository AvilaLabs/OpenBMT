//! BK-38 to BK-40: tips.

use openbmt_counter::{Amount, Event, Role};

use super::fixture::*;
use crate::{Books, Date, RevealsPayments};

fn tipped(index: u32, at: u64, cents: u64, tip: u64, by: Option<(Role, &str)>) -> Event {
    Event::Opened {
        index,
        at,
        amount: Amount(cents),
        tip: Amount(tip),
        currency: usd(),
        needs_sats: 4_321_987,
        minor_per_btc: 4_281_700,
        source: source("fixture-rate", at),
        by: by.map(|(r, n)| (r, n.to_string())),
    }
}

/// Three sales on one day: Ann's with a tip, Bob's with a tip, and one with
/// no tip and no person; and a fourth that was never paid.
fn day() -> Kit {
    kit_with(&[
        tipped(0, FEB_1_0100, 1850, 275, Some((Role::Cashier, "Ann"))),
        final_event(0, FEB_1_0100 + 10, SATS_PAID),
        tipped(1, FEB_1_0100 + 20, 1000, 100, Some((Role::Cashier, "Bob"))),
        final_event(1, FEB_1_0100 + 30, SATS_PAID),
        tipped(2, FEB_1_0100 + 40, 500, 0, None),
        final_event(2, FEB_1_0100 + 50, SATS_PAID),
        tipped(3, FEB_1_0100 + 60, 700, 70, Some((Role::Cashier, "Ann"))),
        tipped(4, FEB_1_0100 + 70, 300, 30, Some((Role::Manager, "Ann"))),
        final_event(4, FEB_1_0100 + 80, SATS_PAID),
    ])
}

#[test]
fn bk_38_a_sale_holds_its_tip_and_the_person_and_an_old_record_has_none() {
    let kit = day();
    let books = kit.books();
    let s = books.sale(&kit.key(0)).expect("sale");
    assert_eq!(s.amount_minor, 1850);
    assert_eq!(s.tip_minor(), 275);
    assert_eq!(s.due_minor(), Some(2125));
    let note = s.tip.as_ref().expect("a tip");
    assert_eq!(note.by, Some((Role::Cashier, "Ann".to_string())));
    let none = books.sale(&kit.key(2)).expect("sale");
    assert!(none.tip.is_none());
    assert_eq!(none.tip_minor(), 0);
    // A record of a build from before tips (tag 1) reads as no tip.
    let old = kit_with(&[opened(0, FEB_1_0100, 500)]);
    assert!(old.books().sales()[0].tip.is_none());
}

#[test]
fn bk_38_the_accounting_line_is_the_amount_and_the_tip_together() {
    let kit = day();
    let files = kit.books().accounting_export().expect("export");
    let all: String = files.iter().map(|(_, f)| f.as_str()).collect();
    // 18.50 + 2.75, 10.00 + 1.00, 5.00 and 3.00 + 0.30; sale 3 is not final.
    for amount in ["21.25", "11.00", "5.00", "3.30"] {
        assert!(all.contains(amount), "{amount} in {all}");
    }
    assert!(!all.contains("18.50"), "{all}");
    assert_eq!(all.lines().filter(|l| l.contains("Sale ")).count(), 4);
}

#[test]
fn bk_38_the_ledger_has_the_tip_in_the_column_tips() {
    let kit = day();
    let ledger = kit
        .books()
        .full_ledger(RevealsPayments::Acknowledged)
        .expect("ledger");
    let header = ledger
        .lines()
        .find(|l| l.starts_with("Type,"))
        .expect("header");
    assert!(
        header.contains(",Tips,Sale note,Till reference,"),
        "{header}"
    );
    let rows: Vec<&str> = ledger.lines().filter(|l| l.starts_with("sale,")).collect();
    assert_eq!(rows.len(), 5);
    assert!(rows.iter().any(|r| r.ends_with(",2.75,,,,,,,")));
    assert!(rows.iter().any(|r| r.ends_with(",1.00,,,,,,,")));
    // The amount column is the sale's own, and a sale without a tip is empty.
    assert!(rows.iter().any(|r| r.contains(",18.50,")));
    assert!(rows
        .iter()
        .any(|r| r.contains(",5.00,") && r.ends_with(",,,,,,,,")));
}

#[test]
fn bk_39_the_totals_of_a_day_have_the_tips_in_all_and_for_each_person() {
    let books = day().books();
    let totals = books.utc_day_totals().expect("totals");
    assert_eq!(totals.len(), 1);
    let t = &totals[0];
    assert_eq!(
        t.day,
        Date {
            year: 2025,
            month: 2,
            day: 1
        }
    );
    // The four sales that received funds, without the tips.
    assert_eq!(t.sales, 4);
    assert_eq!(t.minor.get(&usd()), Some(&(1850 + 1000 + 500 + 300)));
    // The tips of those four: 275 + 100 + 30, and the waiting sale 3 never
    // received funds, so its 70 is not there.
    assert_eq!(t.tips_minor.get(&usd()), Some(&405));
    let by = |role, name: &str| t.tips_by_person.get(&(usd(), role, name.to_string()));
    assert_eq!(by(Role::Cashier, "Ann"), Some(&275));
    assert_eq!(by(Role::Cashier, "Bob"), Some(&100));
    // The same name in another role is another person.
    assert_eq!(by(Role::Manager, "Ann"), Some(&30));
    assert_eq!(t.tips_by_person.len(), 3);
    // Shop-local totals carry them too.
    let local = books.local_day_totals(60).expect("totals");
    assert_eq!(local[0].tips_minor.get(&usd()), Some(&405));
}

#[test]
fn bk_39_a_tip_without_a_person_is_in_the_sum_only_and_a_sum_that_does_not_fit_is_an_error() {
    let kit = kit_with(&[
        tipped(0, FEB_1_0100, 500, 40, None),
        final_event(0, FEB_1_0100 + 10, SATS_PAID),
    ]);
    let t = &kit.books().utc_day_totals().expect("totals")[0];
    assert_eq!(t.tips_minor.get(&usd()), Some(&40));
    assert!(t.tips_by_person.is_empty());
    let kit = kit_with(&[
        tipped(0, FEB_1_0100, 500, u64::MAX - 5, None),
        final_event(0, FEB_1_0100 + 10, SATS_PAID),
        tipped(1, FEB_1_0100 + 20, 500, 10, None),
        final_event(1, FEB_1_0100 + 30, SATS_PAID),
    ]);
    assert!(kit.books().utc_day_totals().is_err());
}

#[test]
fn bk_40_the_bundle_is_version_three_only_with_a_tip_and_reads_back() {
    let kit = day();
    let books = kit.books();
    let bytes = books.to_bundle();
    assert_eq!(&bytes[..4], b"OBM6");
    assert_eq!(bytes[4], 3);
    let back = Books::from_bundle(&bytes).expect("bundle");
    assert_eq!(back, books);
    assert_eq!(back.to_bundle(), bytes);
    assert_eq!(back.sales()[0].tip, books.sales()[0].tip);
    for n in 0..bytes.len() {
        assert!(Books::from_bundle(&bytes[..n]).is_err(), "cut at {n}");
    }
    let mut longer = bytes.clone();
    longer.push(0);
    assert!(Books::from_bundle(&longer).is_err());
    // No sale with a tip: the bundle is the one earlier builds wrote.
    let plain = kit_with(&[opened(0, FEB_1_0100, 2500), final_event(0, FEB_2_NOON, 5)]);
    let plain_bytes = plain.books().to_bundle();
    assert_eq!(plain_bytes[4], 1);
    assert!(Books::from_bundle(&plain_bytes).expect("bundle").sales()[0]
        .tip
        .is_none());
    // The same bytes marked as version 3 are not a bundle: each sale would
    // need its mode and its tip.
    let mut three = plain_bytes.clone();
    three[4] = 3;
    assert!(Books::from_bundle(&three).is_err());
}

#[test]
fn bk_40_a_bundle_with_a_tip_of_nothing_or_a_bad_name_or_no_tip_at_all_is_refused() {
    let kit = kit_with(&[
        tipped(0, FEB_1_0100, 500, 40, Some((Role::Cashier, "Ann"))),
        final_event(0, FEB_1_0100 + 10, SATS_PAID),
    ]);
    let bytes = kit.books().to_bundle();
    assert_eq!(bytes[4], 3);
    // The tip is the last thing in the sale: flag 1, 8 bytes, flag 1, role,
    // name length and the name; the disposals and returns (two counts of 4
    // bytes) follow.
    let tail = 4 + 4;
    let name_at = bytes.len() - tail - 3;
    let flag_at = name_at - 1 - 1 - 1 - 8 - 1;
    assert_eq!(bytes[flag_at], 1);
    let amount_at = flag_at + 1;
    assert_eq!(&bytes[amount_at..amount_at + 8], &40u64.to_le_bytes());
    // A tip of nothing.
    let mut zero = bytes.clone();
    zero[amount_at..amount_at + 8].fill(0);
    assert!(Books::from_bundle(&zero).is_err());
    // A name of no length.
    let mut empty = bytes.clone();
    empty[name_at - 1] = 0;
    assert!(Books::from_bundle(&empty).is_err());
    // A role that is not known.
    let mut role = bytes.clone();
    role[name_at - 2] = 9;
    assert!(Books::from_bundle(&role).is_err());
    // A flag that says there is no tip: version 3 with no sale with a tip
    // does not encode back to itself.
    let mut none = bytes.clone();
    none[flag_at] = 0;
    assert!(Books::from_bundle(&none).is_err());
    // A tip that makes the sum overflow.
    let mut big = bytes.clone();
    big[amount_at..amount_at + 8].copy_from_slice(&u64::MAX.to_le_bytes());
    assert!(Books::from_bundle(&big).is_err());
}
