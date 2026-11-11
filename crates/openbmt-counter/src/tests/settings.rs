//! What the counter does with the settings it takes: the record that names
//! them (CR-162), the sales that keep what they were opened with (CR-160) and
//! the counter the settings remove (CR-165).

use openbmt_core::records::{Record, RecordKind};
use openbmt_core::settings::{Change, SettingNote};

use super::kit::*;
use crate::{Amount, Counter, OpenRefused, State, StoreFault};

fn note(version: u32, change: Change) -> SettingNote {
    SettingNote {
        version,
        change,
        hash: [version as u8; 32],
    }
}

#[test]
fn cr_162_a_setting_record_names_the_version_the_change_and_the_hash() {
    let mut w = World::new();
    assert_eq!(w.c.settings_recorded(), None);
    let n = note(4, Change::Loosening);
    let record = w.c.record_setting(n, &mut w.m).unwrap();
    assert_eq!(record.kind, RecordKind::Setting);
    assert_eq!(SettingNote::from_body(&record.body), Ok(n));
    assert_eq!(w.c.settings_recorded(), Some(4));
    // It is the chain's next record and was stored first (CR-40).
    assert_eq!(w.m.log, vec!["record".to_string()]);
    assert_eq!(w.c.records_len(), 1);
    // A lower version does not take the number back.
    w.c.record_setting(note(3, Change::Tightening), &mut w.m)
        .unwrap();
    assert_eq!(w.c.settings_recorded(), Some(4));
}

#[test]
fn cr_162_the_highest_setting_record_is_read_back_at_start() {
    let mut w = World::new();
    w.c.record_setting(note(2, Change::Tightening), &mut w.m)
        .unwrap();
    w.c.record_setting(note(5, Change::Loosening), &mut w.m)
        .unwrap();
    // A sale in between does not disturb it.
    w.open_five();
    let again = w.m.reload();
    assert_eq!(again.settings_recorded(), Some(5));
    assert_eq!(again.records_len(), w.c.records_len());
    // A chain with none says so.
    assert_eq!(World::new().m.reload().settings_recorded(), None);
}

#[test]
fn cr_162_a_record_that_is_not_stored_changes_nothing() {
    let mut w = World::new();
    w.m.fail_all = true;
    let err =
        w.c.record_setting(note(2, Change::Tightening), &mut w.m)
            .unwrap_err();
    assert!(matches!(err, StoreFault::Write(_)));
    assert_eq!(w.c.settings_recorded(), None);
    assert_eq!(w.c.records_len(), 0);
}

#[test]
fn cr_162_a_setting_record_is_not_taken_for_a_sale_event() {
    // The body of a setting record is 37 bytes that no event reads, and the
    // chain with one in it still loads and still settles sales.
    let mut w = World::new();
    let o = w.open_five();
    w.c.record_setting(note(9, Change::Loosening), &mut w.m)
        .unwrap();
    let again = w.m.reload();
    assert_eq!(again.records_len(), 2);
    assert!(again.sale(o.index).is_some());
    let kinds: Vec<RecordKind> =
        w.m.records
            .iter()
            .map(|b| Record::from_bytes(b).unwrap().kind)
            .collect();
    assert_eq!(kinds, vec![RecordKind::Sale, RecordKind::Setting]);
}

#[test]
fn cr_160_a_sale_keeps_the_tolerance_it_was_opened_with() {
    let mut w = World::new();
    let o = w.open_five();
    let opened_with = w.c.sale(o.index).unwrap().tolerance_bps;
    let mut s = settings();
    s.version += 1;
    s.underpayment_tolerance_bps = opened_with + 100;
    s.min_confirmations = 2;
    w.c.set_settings(s);
    assert_eq!(w.c.sale(o.index).unwrap().tolerance_bps, opened_with);
    // The next sale takes the new tolerance and the counter reads the new
    // depth.
    let next = w.open_five();
    assert_eq!(
        w.c.sale(next.index).unwrap().tolerance_bps,
        opened_with + 100
    );
    assert_eq!(w.c.settings().min_confirmations, 2);
}

fn removed() -> Counter {
    let mut s = settings();
    s.version += 1;
    s.counters.clear();
    s.devices.remove(&id(3).device_id());
    let mut c = Counter::new(id(3), pairing(), settings());
    c.set_settings(s);
    c
}

#[test]
fn cr_165_settings_that_do_not_list_the_counter_refuse_a_new_sale() {
    let mut w = World::new();
    assert!(w.c.is_listed());
    let open = w.open_five();
    w.m.reset();
    let mut s = settings();
    s.version += 1;
    s.counters.clear();
    w.c.set_settings(s);
    assert!(!w.c.is_listed());
    let facts = w.open_facts();
    let err =
        w.c.open(
            Amount(500),
            usd(),
            rate(),
            &fresh(facts.now),
            facts,
            &mut w.m,
        )
        .unwrap_err();
    assert_eq!(err, OpenRefused::Removed);
    assert_eq!(err.rule(), "CR-165");
    let text = err.to_string();
    assert!(text.starts_with("CR-165: "), "{text}");
    assert!(
        text.contains("removed from the shop by the owner"),
        "{text}"
    );
    assert!(text.contains("ask the owner"), "{text}");
    // Nothing was written: no address was handed out.
    assert_eq!(w.m.writes, 0);
    // The sale that was open still settles.
    let tx = w.pay(&open, 5_000);
    w.confirm(&[tx], 100);
    let screen = w.judge(open.index);
    assert_eq!(screen.state, State::Confirmed);
}

#[test]
fn cr_165_a_counter_off_the_device_list_is_removed_too() {
    let mut s = settings();
    s.version += 1;
    s.devices.remove(&id(3).device_id());
    let mut c = Counter::new(id(3), pairing(), settings());
    assert!(c.is_listed());
    c.set_settings(s);
    assert!(!c.is_listed());
    assert!(!removed().is_listed());
}
