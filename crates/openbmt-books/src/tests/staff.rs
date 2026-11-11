//! Section 9: changes to the cashiers (BK-35 to BK-37).

use openbmt_core::records::RecordKind;
use openbmt_core::say::{check, Tone};
use openbmt_counter::{StaffChange, StaffEvent};

use super::fixture::*;
use crate::say::staff_words;
use crate::{Books, BooksError, StaffNote};

fn change(at: u64, change: StaffChange, name: &str) -> StaffEvent {
    StaffEvent {
        at,
        change,
        name: name.to_string(),
        by: "Max".to_string(),
    }
}

fn add(kit: &mut Kit, e: &StaffEvent) {
    kit.chain
        .append(&kit.counter, RecordKind::Pairing, e.encode())
        .expect("appended");
}

#[test]
fn bk_35_a_pairing_record_that_is_a_staff_change_is_read() {
    let mut kit = kit_with(&[opened(0, JAN_31_235930, 100)]);
    add(&mut kit, &change(FEB_1_0100, StaffChange::Added, "Ann"));
    let books = kit.books();
    assert_eq!(books.sales().len(), 1, "the sale is as it was");
    assert_eq!(books.staff_changes().len(), 1);
}

#[test]
fn bk_35_a_pairing_record_that_is_no_staff_change_refuses_the_build() {
    for body in [vec![1u8, 2, 3], vec![0xff; 5], Vec::new()] {
        let mut kit = kit_with(&[opened(0, JAN_31_235930, 100)]);
        kit.chain
            .append(&kit.counter, RecordKind::Pairing, body)
            .expect("appended");
        let err = Books::from_chains(&kit.devices, &[(kit.counter_id(), &kit.chain)])
            .expect_err("refused");
        assert!(
            matches!(err, BooksError::BadEvent { seq: 1, .. }),
            "{err:?}"
        );
        assert!(err.to_string().contains("BK-03"));
    }
    // A sale event in a pairing record is no staff change either.
    let mut kit = kit_with(&[]);
    kit.chain
        .append(
            &kit.counter,
            RecordKind::Pairing,
            opened(0, JAN_31_235930, 100).encode(),
        )
        .expect("appended");
    assert!(Books::from_chains(&kit.devices, &[(kit.counter_id(), &kit.chain)]).is_err());
}

#[test]
fn bk_35_a_time_out_of_range_refuses_the_build() {
    let mut kit = kit_with(&[]);
    add(&mut kit, &change(u64::MAX, StaffChange::Added, "Ann"));
    let err =
        Books::from_chains(&kit.devices, &[(kit.counter_id(), &kit.chain)]).expect_err("refused");
    assert!(matches!(err, BooksError::TimeOutOfRange { .. }), "{err:?}");
}

#[test]
fn bk_36_the_changes_are_kept_in_chain_order_and_are_in_no_total() {
    let mut kit = kit_with(&[opened(0, JAN_31_235930, 100)]);
    add(&mut kit, &change(FEB_1_0100, StaffChange::Added, "Ann"));
    add(&mut kit, &change(FEB_1_0100 + 5, StaffChange::Added, "Bob"));
    add(&mut kit, &change(FEB_2_NOON, StaffChange::Removed, "Ann"));
    let before = kit_with(&[opened(0, JAN_31_235930, 100)]).books();
    let books = kit.books();
    let notes: Vec<(StaffChange, &str, &str, u64)> = books
        .staff_changes()
        .iter()
        .map(|n| (n.change, n.name.as_str(), n.by.as_str(), n.at))
        .collect();
    assert_eq!(
        notes,
        vec![
            (StaffChange::Added, "Ann", "Max", FEB_1_0100),
            (StaffChange::Added, "Bob", "Max", FEB_1_0100 + 5),
            (StaffChange::Removed, "Ann", "Max", FEB_2_NOON),
        ]
    );
    assert!(books
        .staff_changes()
        .iter()
        .all(|n| n.counter == kit.counter_id()));
    // The sales, the totals and the high-water mark are those of the same
    // chain without the changes.
    assert_eq!(books.sales(), before.sales());
    assert_eq!(books.high_water(), before.high_water());
    assert_eq!(
        books.utc_day_totals().unwrap(),
        before.utc_day_totals().unwrap()
    );
    // The encrypted bundle does not carry them.
    let bytes = books.to_bundle();
    let back = Books::from_bundle(&bytes).expect("reads back");
    assert!(back.staff_changes().is_empty());
    assert_eq!(back.sales(), books.sales());
}

#[test]
fn bk_36_a_record_of_another_device_is_not_the_counters() {
    let mut kit = kit_with(&[]);
    kit.chain
        .append(
            &kit.owner,
            RecordKind::Pairing,
            change(FEB_1_0100, StaffChange::Added, "Ann").encode(),
        )
        .expect("appended");
    assert!(kit.books().staff_changes().is_empty());
}

fn note(kind: StaffChange, name: &str, by: &str) -> StaffNote {
    StaffNote {
        counter: identity(1).device_id(),
        at: FEB_1_0100,
        change: kind,
        name: name.to_string(),
        by: by.to_string(),
    }
}

#[test]
fn bk_37_each_change_has_words_that_keep_the_screen_rules() {
    let added = note(StaffChange::Added, "Ann", "Max").words("Corner");
    let removed = note(StaffChange::Removed, "Ann", "Max").words("Corner");
    assert_eq!(check(&added), Ok(()));
    assert_eq!(check(&removed), Ok(()));
    assert_ne!(added.heading, removed.heading);
    assert_eq!(added.tone, Tone::Info);
    assert_eq!(added.at, Some(FEB_1_0100));
    // The names are quoted text, never in the sentences, and are cleaned.
    assert!(!added.what.contains("Ann") && !added.what.contains("Max"));
    assert!(!added.next.contains("Ann") && !added.next.contains("Max"));
    let quoted = added.quoted.as_ref().expect("quoted");
    assert!(quoted.text.contains("Ann") && quoted.text.contains("Max"));
    let nasty = note(StaffChange::Added, "A\u{202e}nn\n\u{7}", "M\tax").words("Corner");
    let q = nasty.quoted.as_ref().expect("quoted");
    assert!(!q.text.chars().any(|c| c.is_control() || c == '\u{202e}'));
    assert_eq!(check(&nasty), Ok(()));
    assert!(added.what.contains("Corner"));
    // The list SC-12 asks for passes the checks together.
    let all = staff_words();
    assert_eq!(all.len(), 2);
    assert_eq!(openbmt_core::say::check_distinct(&all), Ok(()));
}
