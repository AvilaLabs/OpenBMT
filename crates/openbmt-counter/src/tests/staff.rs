//! Section 14: the record of a change to the cashiers (CR-143), and the
//! person at the till in the records `Opened` and `Cancelled` (CR-151).

use openbmt_core::records::{Record, RecordKind};
use openbmt_core::wire::DecodeError;

use super::kit::*;
use crate::{
    Actor, Amount, CancelRefused, Event, OpenRefused, Role, StaffChange, StaffEvent, StaffRefused,
};

fn added() -> StaffEvent {
    StaffEvent {
        at: T0 + 5,
        change: StaffChange::Added,
        name: "Ann".to_string(),
        by: "Max".to_string(),
    }
}

fn ann() -> Actor {
    Actor {
        role: Role::Cashier,
        name: "Ann".to_string(),
    }
}

// CR-143.

#[test]
fn cr_143_a_change_is_written_as_a_pairing_record_and_reads_back() {
    let mut w = World::new();
    let before = w.c.records_len();
    let record = w.c.record_staff(&added(), &mut w.m).unwrap();
    assert_eq!(record.kind, RecordKind::Pairing);
    assert_eq!(w.c.records_len(), before + 1);
    assert_eq!(w.m.log, vec!["record".to_string()], "one write, the record");
    assert_eq!(StaffEvent::decode(&record.body).unwrap(), added());
    // The stored copy is the record.
    let stored = Record::from_bytes(w.m.records.last().unwrap()).unwrap();
    assert_eq!(stored.body, record.body);
    let removed = StaffEvent {
        change: StaffChange::Removed,
        ..added()
    };
    let record = w.c.record_staff(&removed, &mut w.m).unwrap();
    assert_eq!(StaffEvent::decode(&record.body).unwrap(), removed);
}

#[test]
fn cr_143_the_body_is_the_tag_the_time_and_two_names_and_holds_no_pin() {
    let body = added().encode();
    assert_eq!(body[0], 32);
    assert_eq!(body[1..9], (T0 + 5).to_le_bytes());
    assert_eq!(&body[9..], b"\x03Ann\x03Max");
    let removed = StaffEvent {
        change: StaffChange::Removed,
        ..added()
    };
    assert_eq!(removed.encode()[0], 33);
    assert_eq!(added().kind(), RecordKind::Pairing);
}

#[test]
fn cr_143_the_decoder_is_strict() {
    let good = added().encode();
    assert_eq!(StaffEvent::decode(&good).unwrap(), added());
    assert_eq!(StaffEvent::decode(&good).unwrap().encode(), good);
    // Every cut is refused, and so is a byte left over.
    for n in 0..good.len() {
        assert!(StaffEvent::decode(&good[..n]).is_err(), "cut at {n}");
    }
    let mut longer = good.clone();
    longer.push(0);
    assert_eq!(StaffEvent::decode(&longer), Err(DecodeError::TrailingBytes));
    // A tag that is neither added nor removed, including every sale event's.
    for tag in (0..=255u8).filter(|t| *t != 32 && *t != 33) {
        let mut bad = good.clone();
        bad[0] = tag;
        assert!(StaffEvent::decode(&bad).is_err(), "tag {tag}");
    }
    // An empty name, a control character and text that is not text.
    let mut empty = good.clone();
    empty.splice(9..13, [0u8]);
    assert!(StaffEvent::decode(&empty).is_err(), "an empty name");
    let mut control = good.clone();
    control[10] = 0x07;
    assert!(StaffEvent::decode(&control).is_err(), "a control character");
    let mut not_text = good.clone();
    not_text[10] = 0xff;
    assert!(StaffEvent::decode(&not_text).is_err(), "not text");
    // The sale events do not read as staff events, nor staff events as sale events.
    let sale = Event::Expired { index: 1, at: T0 }.encode();
    assert!(StaffEvent::decode(&sale).is_err());
    assert!(Event::decode(&good).is_err());
}

#[test]
fn cr_143_a_name_that_is_not_a_label_writes_nothing() {
    let mut w = World::new();
    let before = w.c.records_len();
    for bad in ["", "tab\there", &"x".repeat(65)] {
        let e = StaffEvent {
            name: bad.to_string(),
            ..added()
        };
        assert_eq!(
            w.c.record_staff(&e, &mut w.m).unwrap_err(),
            StaffRefused::BadName("cashier's name")
        );
        let e = StaffEvent {
            by: bad.to_string(),
            ..added()
        };
        let err = w.c.record_staff(&e, &mut w.m).unwrap_err();
        assert_eq!(err, StaffRefused::BadName("manager's name"));
        assert_eq!(err.rule(), "CR-143");
        assert!(err.to_string().starts_with("CR-143"));
    }
    assert_eq!(w.c.records_len(), before);
    assert!(w.m.records.is_empty());
}

#[test]
fn cr_143_a_write_that_fails_leaves_the_chain_as_it_was() {
    let mut w = World::new();
    w.m.fail_all = true;
    let err = w.c.record_staff(&added(), &mut w.m).unwrap_err();
    assert!(matches!(err, StaffRefused::Store(_)));
    assert_eq!(err.rule(), "CR-40");
    assert_eq!(w.c.records_len(), 0);
    w.m.reset();
    w.c.record_staff(&added(), &mut w.m).unwrap();
    assert_eq!(w.c.records_len(), 1);
}

#[test]
fn cr_143_a_counter_started_again_reads_past_the_staff_records() {
    let mut w = World::new();
    let o = w.open_five();
    w.c.record_staff(&added(), &mut w.m).unwrap();
    w.c.cancel(o.index, w.now, &mut w.m).unwrap();
    let head = w.c.head();
    let count = w.c.records_len();
    w.restart();
    assert_eq!(w.c.records_len(), count);
    assert_eq!(w.c.head(), head);
    // The sale is still there and a new record follows the chain.
    assert!(w.c.sale(o.index).is_some());
    w.c.record_staff(&added(), &mut w.m).unwrap();
    assert_eq!(w.c.records_len(), count + 1);
}

// CR-151.

fn opened_by(w: &mut World, by: Option<&Actor>) -> Result<u32, OpenRefused<&'static str>> {
    let facts = w.open_facts();
    w.c.open_by(
        Amount(500),
        usd(),
        rate(),
        &fresh(facts.now),
        facts,
        by,
        &mut w.m,
    )
    .map(|o| o.index)
}

#[test]
fn cr_151_a_sale_and_a_cancel_carry_the_person_at_the_till() {
    let mut w = World::new();
    let who = ann();
    let index = opened_by(&mut w, Some(&who)).unwrap();
    w.c.cancel_by(index, w.now, Some(&who), &mut w.m).unwrap();
    let events = w.m.events();
    assert!(matches!(
        &events[0],
        Event::Opened { by: Some((Role::Cashier, name)), .. } if name == "Ann"
    ));
    assert!(matches!(
        &events[1],
        Event::Cancelled { by: Some((Role::Cashier, name)), .. } if name == "Ann"
    ));
    for e in &events {
        assert_eq!(Event::decode(&e.encode()).unwrap(), *e);
    }
}

#[test]
fn cr_151_a_record_without_a_person_is_what_it_was() {
    let mut w = World::new();
    let index = w.open_five().index;
    w.c.cancel(index, w.now, &mut w.m).unwrap();
    let events = w.m.events();
    assert!(matches!(&events[0], Event::Opened { by: None, .. }));
    assert!(matches!(&events[1], Event::Cancelled { by: None, .. }));
    // The body of a cancel with no person is the three fixed fields and no more.
    assert_eq!(events[1].encode().len(), 1 + 4 + 8);
    // A body written with a person reads, and without it is a prefix of it.
    let with = Event::Cancelled {
        index: 1,
        at: T0,
        by: Some((Role::Manager, "Max".to_string())),
    }
    .encode();
    let without = Event::Cancelled {
        index: 1,
        at: T0,
        by: None,
    }
    .encode();
    assert!(with.starts_with(&without) && with.len() > without.len());
    assert!(Event::decode(&without).is_ok() && Event::decode(&with).is_ok());
    // A person's role that is not known, a name that is not a label and a cut
    // name are refused.
    for bad in [
        [&without[..], &[9u8, 1, b'a'][..]].concat(),
        [&without[..], &[1u8, 0][..]].concat(),
        [&without[..], &[1u8, 3, b'a'][..]].concat(),
        [&without[..], &[1u8][..]].concat(),
    ] {
        assert!(Event::decode(&bad).is_err(), "{bad:?}");
    }
}

#[test]
fn cr_151_a_name_that_is_not_a_label_refuses_the_call_before_anything_is_written() {
    let mut w = World::new();
    let bad = Actor {
        role: Role::Cashier,
        name: String::new(),
    };
    let err = opened_by(&mut w, Some(&bad)).unwrap_err();
    assert_eq!(err, OpenRefused::BadName);
    assert_eq!(err.rule(), "CR-61");
    assert!(w.m.log.is_empty(), "nothing was written: {:?}", w.m.log);
    let index = w.open_five().index;
    w.m.reset();
    let err =
        w.c.cancel_by(index, w.now, Some(&bad), &mut w.m)
            .unwrap_err();
    assert_eq!(err, CancelRefused::BadName);
    assert!(w.m.log.is_empty());
    assert!(w.c.sale(index).is_some_and(|s| s.is_live()));
}
