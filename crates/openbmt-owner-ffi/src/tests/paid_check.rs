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
    let rig = Rig::new();
    let events = [
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
    ];
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
    if garbage_marks {
        std::fs::write(rig.dir().join("paid-check.bin"), b"OBM\x17\x01garbage").expect("a file");
    }
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
    let alarms = r.list(|r| {
        (
            r.bytes(),
            r.u32(),
            r.string(),
            r.u64(),
            r.u64(),
            r.u64(),
            r.u64(),
        )
    });
    let arrived = r.list(|r| (r.bytes(), r.u32(), r.u64()));
    let waiting = r.list(line);
    let not_checkable = r.list(line);
    let not_checkable_more = r.u32();
    r.done();
    Check {
        kept,
        alarms: alarms.len(),
        arrived: arrived.len(),
        waiting,
        not_checkable,
        not_checkable_more,
    }
}

#[test]
fn oc_60_the_check_lists_a_sale_the_wallet_cannot_speak_for_and_the_day_counts_it() {
    let (rig, handle) = ready(false);
    // The test shop issued no address of the counter's, so the wallet cannot
    // say what was paid to the sale's address: the sale is listed, not
    // judged, and no alarm is raised however late it is.
    let c = check(&rig, handle, PAID_AT + 10 * 86_400);
    assert!(c.kept);
    assert_eq!((c.alarms, c.arrived, c.not_checkable_more), (0, 0, 0));
    assert!(c.waiting.is_empty());
    assert_eq!(
        c.not_checkable,
        vec![(counter_bytes(), 7, 5_000, PAID_AT)],
        "the counter, the sale's number, what it recorded and when"
    );
    // The day of the Paid: one sale, none seen, one too old to check.
    let response = rig.call(OP_DAY, handle, &W::new().u64(PAID_AT + 100).u32(0).0);
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
        vec![(counter_bytes(), 1, 5_000, (0, 0, 1, 0), 0, true)],
        "sales, total, seen, waiting, too old, missing, seen total, named"
    );
    // Another day has no line.
    let response = rig.call(OP_DAY, handle, &W::new().u64(PAID_AT + 3 * 86_400).u32(0).0);
    let mut r = Rd::ok(&response);
    assert!(r.list(|r| r.bytes()).is_empty());
    r.done();
}

#[test]
fn oc_62_an_alarm_that_is_not_raised_is_not_cleared() {
    let (rig, handle) = ready(false);
    let fields = W::new().u64(PAID_AT).bytes(&counter_bytes()).u32(7).0;
    let f = fault(&rig.call(OP_CLEAR, handle, &fields));
    assert_eq!((f.status, f.rule.as_str()), (1, "paid-alarm-unknown"));
    // A counter that is not 32 bytes is refused before any mark is looked at.
    let f = fault(&rig.call(
        OP_CLEAR,
        handle,
        &W::new().u64(PAID_AT).bytes(&[1; 5]).u32(7).0,
    ));
    assert_eq!(f.status, 1, "{f:?}");
}

#[test]
fn oc_64_checks_that_do_not_read_back_are_named_and_cleared_by_the_owner() {
    let (rig, handle) = ready(true);
    // The node started: the file did not stop it. Nothing is checked.
    let c = check(&rig, handle, PAID_AT + 10 * 86_400);
    assert!(!c.kept);
    assert!(c.alarms == 0 && c.waiting.is_empty() && c.not_checkable.is_empty());
    let response = rig.call(OP_DAY, handle, &W::new().u64(PAID_AT + 100).u32(0).0);
    let mut r = Rd::ok(&response);
    assert!(r.list(|r| r.bytes()).is_empty());
    r.done();
    // Clearing an alarm says why it cannot.
    let fields = W::new().u64(PAID_AT).bytes(&counter_bytes()).u32(7).0;
    let f = fault(&rig.call(OP_CLEAR, handle, &fields));
    assert_eq!((f.status, f.rule.as_str()), (3, "paid-not-kept"));
    // The owner clears the checks, and they run again.
    Rd::ok(&rig.call(OP_RESET, handle, &[])).done();
    let c = check(&rig, handle, PAID_AT + 10 * 86_400);
    assert!(c.kept);
    assert_eq!(c.not_checkable.len(), 1);
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
