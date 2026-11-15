//! The counter-check ops (239 to 242, `docs/OWNER-COINS-RULES.md` OC-60 to
//! OC-64) through `dispatch`: a counter's copy of records that shows a sale
//! paid, the list, the day, an alarm that is not there, and the checks that
//! do not read back.

use openbmt_core::policy::{CheckMode, InstantLabel};
use openbmt_core::records::Chain;
use openbmt_counter::{Amount, Currency, Event, RateSource, Role};

use super::kit::*;
use super::screen::check_wire_words;
use super::wire::{Fault, Rd, W};
use openbmt_owner::SaleKey;
use openbmt_owner_node::{CheckResult, PaidMarks, MAX_PAID_MARKS};

use crate::{LinkConfig, RouteView};

const OP_CHECK: u16 = 239;
const OP_DAY: u16 = 240;
const OP_CLEAR: u16 = 241;
const OP_RESET: u16 = 242;

/// The time of the Paid of the test sale.
const PAID_AT: u64 = 1_800_000_000;

fn fault(response: &[u8]) -> Fault {
    let f = Rd::error(response).expect("an error response");
    check_wire_words(&f.words).unwrap_or_else(|why| panic!("{why}: {f:?}"));
    f
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// A node with a link to the test counter, whose copy of the records holds a
/// sale opened by a cashier and accepted at once.
fn ready(garbage_marks: bool) -> (Rig, u64) {
    ready_in(|dir| {
        if garbage_marks {
            std::fs::write(dir.join("paid-check.bin"), b"OBM\x17\x01garbage").expect("a file");
        }
    })
}

/// The same, after `prepare` has set the node's folder up.
fn ready_in(prepare: impl FnOnce(&std::path::Path)) -> (Rig, u64) {
    ready_with(paid_sale(), prepare)
}

/// The records of the test sale.
fn paid_sale() -> Vec<Event> {
    vec![
        Event::Opened {
            index: 7,
            at: PAID_AT - 60,
            amount: Amount(950),
            tip: Amount(0),
            currency: Currency::new(*b"USD").expect("a currency"),
            needs_sats: 5_000,
            minor_per_btc: 1,
            source: RateSource::new("test-fixed", PAID_AT - 60).expect("a source"),
            by: Some((Role::Cashier, "Ana".to_string())),
        },
        Event::PaidAtOnce {
            index: 7,
            at: PAID_AT,
            at_risk_sats: 5_000,
            label: InstantLabel::LightCheck,
            mode: CheckMode::Light,
        },
    ]
}

/// A node with a link to the test counter, whose copy of the records holds
/// `events`.
fn ready_with(events: Vec<Event>, prepare: impl FnOnce(&std::path::Path)) -> (Rig, u64) {
    let rig = Rig::new();
    let mut chain = Chain::new();
    for e in &events {
        chain
            .append(&id(0x31), e.kind(), e.encode())
            .expect("a record");
    }
    let copy = rig.dir().join("counters").join(hex(&counter_bytes()));
    std::fs::create_dir_all(&copy).expect("a folder");
    let mut file = Vec::new();
    for r in chain.records() {
        let bytes = r.to_bytes();
        file.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
        file.extend_from_slice(&bytes);
    }
    std::fs::write(copy.join("records"), file).expect("the records");
    prepare(&rig.dir());
    let mut config = start_config(rig.dir().to_str().expect("UTF-8"));
    config.link = Some(LinkConfig {
        route: RouteView::Direct {
            addr: "127.0.0.1:9".to_string(),
        },
        relay_key: id(0x41).device_id().to_bytes().to_vec(),
        counters: vec![counter_bytes()],
        heartbeat_every_secs: 60,
        check_in_grace_secs: 3_600,
    });
    let handle = Rd::ok(&rig.call(1, 0, &W::new().start(&config).0)).u64();
    rig.send(vec![peers_event()]);
    rig.pump(handle);
    (rig, handle)
}

type Line = (Vec<u8>, u32, u64, u64);

fn line(r: &mut Rd<'_>) -> Line {
    (r.bytes(), r.u32(), r.u64(), r.u64())
}

struct Check {
    kept: bool,
    full: bool,
    unreadable: usize,
    alarms: usize,
    arrived: usize,
    waiting: Vec<Line>,
    not_checkable: Vec<Line>,
    not_checkable_more: u32,
}

fn check(rig: &Rig, handle: u64, now: u64) -> Check {
    let response = rig.call(OP_CHECK, handle, &W::new().u64(now).0);
    let mut r = Rd::ok(&response);
    let kept = r.bool();
    let full = r.bool();
    let unreadable = r.list(|r| r.bytes());
    let alarms = r.list(|r| {
        (
            r.bytes(),
            r.u32(),
            r.string(),
            r.u64(),
            r.u64(),
            r.u64(),
            r.u64(),
            r.bool(),
        )
    });
    let arrived = r.list(|r| (r.bytes(), r.u32(), r.u64()));
    let waiting = r.list(line);
    let not_checkable = r.list(line);
    let not_checkable_more = r.u32();
    r.done();
    Check {
        kept,
        full,
        unreadable: unreadable.len(),
        alarms: alarms.len(),
        arrived: arrived.len(),
        waiting,
        not_checkable,
        not_checkable_more,
    }
}

#[test]
fn oc_61_a_sale_at_an_index_never_issued_is_an_alarm_and_the_day_counts_it_missing() {
    let (rig, handle) = ready(false);
    // The test shop issued no address of the counter's, so no payment at the
    // sale's index can be the shop's: an alarm at once, however recent.
    let c = check(&rig, handle, PAID_AT + 10);
    assert!(c.kept);
    assert_eq!((c.alarms, c.arrived, c.not_checkable_more), (1, 0, 0));
    assert!(c.waiting.is_empty() && c.not_checkable.is_empty());
    // The day the owner phone first read the Paid: one sale, none seen, one
    // missing. The counter's own time does not place it.
    let response = rig.call(OP_DAY, handle, &W::new().u64(T0 + 100).u32(0).0);
    let mut r = Rd::ok(&response);
    let days = r.list(|r| {
        (
            r.bytes(),
            r.u32(),
            r.u64(),
            (r.u32(), r.u32(), r.u32(), r.u32()),
            r.u64(),
            r.bool(),
        )
    });
    r.done();
    assert_eq!(
        days,
        vec![(counter_bytes(), 1, 5_000, (0, 0, 0, 1), 0, true)],
        "sales, total, seen, waiting, too old, missing, seen total, named"
    );
    // Another day has no line.
    let response = rig.call(OP_DAY, handle, &W::new().u64(T0 + 3 * 86_400).u32(0).0);
    let mut r = Rd::ok(&response);
    assert!(r.list(|r| r.bytes()).is_empty());
    r.done();
}

#[test]
fn oc_62_only_a_raised_alarm_is_cleared() {
    let (rig, handle) = ready(false);
    let fields = W::new().u64(PAID_AT).bytes(&counter_bytes()).u32(8).0;
    let f = fault(&rig.call(OP_CLEAR, handle, &fields));
    assert_eq!((f.status, f.rule.as_str()), (1, "paid-alarm-unknown"));
    // The raised one is cleared, once, and its alert goes with it.
    let fields = W::new().u64(PAID_AT).bytes(&counter_bytes()).u32(7).0;
    Rd::ok(&rig.call(OP_CLEAR, handle, &fields)).done();
    assert!(paid_alerts(&rig, handle).is_empty());
    let f = fault(&rig.call(OP_CLEAR, handle, &fields));
    assert_eq!(f.rule.as_str(), "paid-alarm-unknown");
    // A counter that is not 32 bytes is refused before any mark is looked at.
    let f = fault(&rig.call(
        OP_CLEAR,
        handle,
        &W::new().u64(PAID_AT).bytes(&[1; 5]).u32(7).0,
    ));
    assert_eq!(f.status, 1, "{f:?}");
}

#[test]
fn oc_64_checks_that_do_not_read_back_are_named_and_still_run() {
    let (rig, handle) = ready(true);
    assert_eq!(check(&rig, handle, PAID_AT).unreadable, 0);
    // The node started: the file did not stop it, and the check still shows
    // its mismatch.
    let c = check(&rig, handle, PAID_AT + 10 * 86_400);
    assert!(!c.kept && !c.full);
    assert_eq!(c.alarms, 1);
    // The mismatch and the not-kept alert are both among the alerts.
    assert_eq!(paid_alerts(&rig, handle), vec![7]);
    assert_eq!(alerts_of(&rig, handle, 5), vec![0]);
    // The day is still given.
    let response = rig.call(OP_DAY, handle, &W::new().u64(T0 + 100).u32(0).0);
    let mut r = Rd::ok(&response);
    assert_eq!(r.list(|r| r.bytes()).len(), 1);
    // The owner can clear the alarm, in memory.
    let fields = W::new().u64(PAID_AT).bytes(&counter_bytes()).u32(7).0;
    Rd::ok(&rig.call(OP_CLEAR, handle, &fields)).done();
    // The file was not written over.
    assert_eq!(
        std::fs::read(rig.dir().join("paid-check.bin")).expect("the file"),
        b"OBM\x17\x01garbage"
    );
    // The owner clears the checks, and they are kept again; the alert goes.
    Rd::ok(&rig.call(OP_RESET, handle, &[])).done();
    assert!(alerts_of(&rig, handle, 5).is_empty());
    let c = check(&rig, handle, PAID_AT + 10 * 86_400);
    assert!(c.kept);
    assert_eq!(c.alarms, 1);
    assert!(alerts_of(&rig, handle, 5).is_empty());
}

#[test]
fn oc_61_a_counter_whose_records_do_not_fold_is_named_and_alarms() {
    // A payment for a sale that was never opened: the copy does not fold.
    let (rig, handle) = ready_with(
        vec![Event::PaidAtOnce {
            index: 3,
            at: PAID_AT,
            at_risk_sats: 1,
            label: InstantLabel::LightCheck,
            mode: CheckMode::Light,
        }],
        |_| {},
    );
    let c = check(&rig, handle, PAID_AT);
    assert_eq!((c.unreadable, c.alarms), (1, 0));
    assert_eq!(
        alerts_of(&rig, handle, 4),
        vec![0],
        "among the owner's alerts"
    );
    // It is not the owner's to clear.
    let clear = W::new().u8(4).bytes(&counter_bytes()).u64(0).0;
    let f = fault(&rig.call(51, handle, &clear));
    assert_eq!(f.rule.as_str(), "alert-unknown");
}

#[test]
fn oc_64_marks_full_still_show_the_mismatch_and_say_so() {
    let (rig, handle) = ready_in(|dir| {
        // The most marks a file keeps, each a sale only read, of the counter's
        // later indexes.
        let read: Vec<SaleKey> = (8..8 + MAX_PAID_MARKS as u32)
            .map(|index| SaleKey {
                counter: counter(),
                index,
            })
            .collect();
        let marks = PaidMarks::new().after(
            &CheckResult {
                read,
                ..CheckResult::default()
            },
            1_000,
        );
        std::fs::write(dir.join("paid-check.bin"), marks.to_bytes()).expect("the marks");
    });
    let c = check(&rig, handle, PAID_AT + 10);
    assert!(c.kept, "the file reads back");
    assert!(c.full, "one more than a file keeps");
    assert_eq!(c.alarms, 1, "the mismatch is shown");
    assert_eq!(paid_alerts(&rig, handle), vec![7]);
    assert_eq!(alerts_of(&rig, handle, 5), vec![1], "the full alert");
    // The owner clears the checks: nothing is full.
    Rd::ok(&rig.call(OP_RESET, handle, &[])).done();
    assert!(alerts_of(&rig, handle, 5).is_empty());
}

#[test]
fn oc_62_alerts_come_back_after_a_restart_from_the_marks() {
    // The marks of an earlier run: a sale the records do not hold, raised.
    let (rig, handle) = ready_in(|dir| {
        let marks = PaidMarks::new().after(
            &CheckResult {
                raised: vec![SaleKey {
                    counter: counter(),
                    index: 99,
                }],
                ..CheckResult::default()
            },
            1_000,
        );
        std::fs::write(dir.join("paid-check.bin"), marks.to_bytes()).expect("the marks");
    });
    // No page was opened: the alert of the stored mark is raised, with the one
    // the check raised for the sale of the records.
    let mut sales = paid_alerts(&rig, handle);
    sales.sort_unstable();
    assert_eq!(sales, vec![7, 99]);
}

#[test]
fn oc_62_a_failed_run_is_retried_at_the_next_pump() {
    // The marks cannot be written: their temporary file is a folder.
    let (rig, handle) = ready_in(|dir| {
        std::fs::create_dir(dir.join("paid-check.bin.tmp")).expect("a folder");
    });
    // The check ran and its mismatch is among the alerts, with the one that
    // says the marks are not kept.
    assert_eq!(paid_alerts(&rig, handle), vec![7]);
    assert_eq!(alerts_of(&rig, handle, 5), vec![0]);
    assert!(!rig.dir().join("paid-check.bin").exists());
    // The folder goes: no new tip, and the next pump runs it again.
    std::fs::remove_dir(rig.dir().join("paid-check.bin.tmp")).expect("removed");
    rig.pump(handle);
    assert!(
        rig.dir().join("paid-check.bin").exists(),
        "written by the retry"
    );
    assert!(alerts_of(&rig, handle, 5).is_empty());
    assert_eq!(paid_alerts(&rig, handle), vec![7]);
}

#[test]
fn oc_60_a_node_with_no_link_has_nothing_to_check() {
    let rig = Rig::new();
    let handle = rig.start();
    rig.send(vec![peers_event()]);
    rig.pump(handle);
    let c = check(&rig, handle, PAID_AT);
    assert!(c.kept && c.alarms == 0 && c.arrived == 0);
    assert!(c.waiting.is_empty() && c.not_checkable.is_empty());
}

#[test]
fn oc_62_every_request_is_decoded_strictly() {
    let (rig, handle) = ready(false);
    // The ops take their fields whole: a cut or a byte over is refused.
    for (op, fields) in [
        (OP_CHECK, W::new().u64(1).0),
        (OP_DAY, W::new().u64(1).u32(0).0),
        (OP_CLEAR, W::new().u64(1).bytes(&counter_bytes()).u32(7).0),
    ] {
        let mut more = fields.clone();
        more.push(0);
        let f = fault(&rig.call(op, handle, &more));
        assert_eq!(f.status, 1, "a byte left over, op {op}: {f:?}");
        let f = fault(&rig.call(op, handle, &fields[..fields.len() - 1]));
        assert_eq!(f.status, 1, "a cut field, op {op}: {f:?}");
    }
    let f = fault(&rig.call(OP_RESET, handle, &[0]));
    assert_eq!(f.status, 1, "{f:?}");
}

/// The values of the alerts of a kind that `link_state` lists.
fn alerts_of(rig: &Rig, handle: u64, kind: u8) -> Vec<u64> {
    let response = rig.call(50, handle, &[]);
    let mut r = Rd::ok(&response);
    r.link()
        .alerts
        .iter()
        .filter(|a| a.0 == kind)
        .map(|a| a.2)
        .collect()
}

/// The sales of the alerts of kind 3 that `link_state` lists.
fn paid_alerts(rig: &Rig, handle: u64) -> Vec<u64> {
    alerts_of(rig, handle, 3)
}

#[test]
fn oc_62_a_mismatch_reaches_the_alerts_without_a_page_and_a_new_tip_raises_it() {
    let (rig, handle) = ready(false);
    // No call asked for the check: the pump of the start ran it, and the
    // mismatch is among the owner's alerts.
    assert_eq!(paid_alerts(&rig, handle), vec![7]);
    // Clearing the checks drops the alert; nothing runs them until the view
    // has a new tip.
    Rd::ok(&rig.call(242, handle, &[])).done();
    assert!(paid_alerts(&rig, handle).is_empty());
    rig.pump(handle);
    assert!(paid_alerts(&rig, handle).is_empty(), "no new tip, no run");
    rig.mine(handle, &[]);
    assert_eq!(
        paid_alerts(&rig, handle),
        vec![7],
        "a new tip runs the checks"
    );
    // Clearing the alert on the link clears the mark: the next tip does not
    // raise it again.
    let clear = W::new().u8(3).bytes(&counter_bytes()).u64(7).0;
    Rd::ok(&rig.call(51, handle, &clear)).done();
    assert!(paid_alerts(&rig, handle).is_empty());
    rig.mine(handle, &[]);
    assert!(paid_alerts(&rig, handle).is_empty(), "raised once per sale");
}
