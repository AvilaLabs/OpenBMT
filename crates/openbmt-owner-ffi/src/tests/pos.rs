//! The till ops (70 to 75, `docs/OWNER-COINS-RULES.md` OC-30 to OC-37)
//! through `dispatch`: the settings, the request and the reply of a read,
//! an alert acknowledged, and strict decoding of every new request.

use super::kit::*;
use super::screen::check_wire_words;
use super::wire::{Fault, Rd, Words, W};
use crate::{LinkConfig, RouteView};

const SQUARE_PAGE1: &[u8] =
    include_bytes!("../../../openbmt-pos-readers/tests/fixtures/square_orders_page1.json");
const SQUARE_LAST: &[u8] =
    include_bytes!("../../../openbmt-pos-readers/tests/fixtures/square_orders_last.json");

/// After the fixtures' orders were closed, so that a Bitcoin tender with no
/// sale is overdue.
const NOW: u64 = 1_794_324_000;

const OP_STATE: u16 = 70;
const OP_SETUP: u16 = 71;
const OP_REMOVE: u16 = 72;
const OP_REQUEST: u16 = 73;
const OP_REPLY: u16 = 74;
const OP_ACK: u16 = 75;

fn fault(response: &[u8]) -> Fault {
    let f = Rd::error(response).expect("an error response");
    check_wire_words(&f.words).unwrap_or_else(|why| panic!("{why}: {f:?}"));
    f
}

fn ready(link: bool) -> (Rig, u64) {
    let rig = Rig::new();
    let handle = if link {
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
        Rd::ok(&rig.call(1, 0, &W::new().start(&config).0)).u64()
    } else {
        rig.start()
    };
    rig.send(vec![peers_event()]);
    rig.pump(handle);
    (rig, handle)
}

/// The fields of `pos_setup`, written out again.
fn setup_fields(vendor: u8, location: &str, names: &[&str], currencies: &[&str]) -> Vec<u8> {
    let mut w = W::new().u8(vendor).string(location).u32(names.len() as u32);
    for n in names {
        w = w.string(n);
    }
    w = w.u32(currencies.len() as u32);
    for c in currencies {
        w = w.string(c);
    }
    w.0
}

fn square_fields() -> Vec<u8> {
    setup_fields(0, "LOCSQ1", &["Bitcoin"], &["USD"])
}

#[derive(Debug, PartialEq, Eq)]
enum Subject {
    Tender(String, String),
    Sale(Vec<u8>, u32),
}

#[derive(Debug)]
struct Alert {
    kind: u8,
    subject: Subject,
    raised_at: u64,
    words: Words,
}

fn read_key(r: &mut Rd<'_>) -> (u8, Subject) {
    let kind = r.u8();
    let subject = match r.u8() {
        0 => Subject::Tender(r.string(), r.string()),
        1 => Subject::Sale(r.bytes(), r.u32()),
        t => panic!("a subject tag {t}"),
    };
    (kind, subject)
}

fn read_alert(r: &mut Rd<'_>) -> Alert {
    let (kind, subject) = read_key(r);
    Alert {
        kind,
        subject,
        raised_at: r.u64(),
        words: r.words(),
    }
}

struct State {
    setup: Option<(u8, String, Vec<String>, Vec<String>)>,
    last_poll_at: Option<u64>,
    open: Vec<Alert>,
    acknowledged: u32,
    tracked_orders: u32,
    matches: u32,
}

fn state(rig: &Rig, handle: u64) -> State {
    let response = rig.call(OP_STATE, handle, &[]);
    let mut r = Rd::ok(&response);
    let setup = r.opt(|r| (r.u8(), r.string(), r.list(Rd::string), r.list(Rd::string)));
    let last_poll_at = r.opt(Rd::u64);
    let open = r.list(read_alert);
    let acknowledged = r.u32();
    let tracked_orders = r.u32();
    let matches = r.u32();
    let (_by_order, _retired) = (r.u32(), r.u64());
    r.done();
    State {
        setup,
        last_poll_at,
        open,
        acknowledged,
        tracked_orders,
        matches,
    }
}

fn reply_fields(now: u64, status: u32, body: &[u8]) -> Vec<u8> {
    W::new().u64(now).u32(status).bytes(body).0
}

fn ack_fields(now: u64, kind: u8, order: &str, tender: &str) -> Vec<u8> {
    W::new()
        .u64(now)
        .u8(kind)
        .u8(0)
        .string(order)
        .string(tender)
        .0
}

#[test]
fn oc_30_an_untouched_node_has_no_till_and_each_op_says_what_is_missing() {
    let (rig, handle) = ready(false);
    let s = state(&rig, handle);
    assert!(s.setup.is_none() && s.last_poll_at.is_none() && s.open.is_empty());
    assert_eq!((s.acknowledged, s.tracked_orders, s.matches), (0, 0, 0));
    // No till is connected, and no link is held: each says so.
    let f = fault(&rig.call(OP_REQUEST, handle, &W::new().u64(NOW).0));
    assert_eq!((f.status, f.rule.as_str()), (3, "link-none"));
    let f = fault(&rig.call(OP_REPLY, handle, &reply_fields(NOW, 200, b"{}")));
    assert_eq!(f.status, 3, "{f:?}");
    let f = fault(&rig.call(OP_ACK, handle, &ack_fields(NOW, 0, "O", "T")));
    assert_eq!(f.status, 1, "{f:?}");
    // Taking off a till that is not there is not a fault.
    Rd::ok(&rig.call(OP_REMOVE, handle, &[])).done();
}

#[test]
fn oc_31_the_settings_are_checked_kept_over_a_restart_and_taken_off() {
    let (rig, handle) = ready(false);
    // Every bound is the matcher's: a currency in lower case, no names, a
    // Clover merchant id that is not letters and digits.
    for fields in [
        setup_fields(0, "LOCSQ1", &["Bitcoin"], &["usd"]),
        setup_fields(0, "LOCSQ1", &[], &["USD"]),
        setup_fields(0, "", &["Bitcoin"], &["USD"]),
        setup_fields(1, "bad id!", &["Bitcoin"], &["USD"]),
    ] {
        let f = fault(&rig.call(OP_SETUP, handle, &fields));
        assert_eq!(f.status, 1, "{f:?}");
    }
    assert!(state(&rig, handle).setup.is_none());
    Rd::ok(&rig.call(OP_SETUP, handle, &square_fields())).done();
    let s = state(&rig, handle);
    assert_eq!(
        s.setup,
        Some((
            0,
            "LOCSQ1".to_string(),
            vec!["Bitcoin".to_string()],
            vec!["USD".to_string()]
        ))
    );
    // The settings are in the node's folder: a node started on it again
    // holds them.
    Rd::ok(&rig.call(2, handle, &[])).pump();
    rig.arm();
    let handle = rig.start();
    assert!(state(&rig, handle).setup.is_some());
    // A Clover till replaces them.
    Rd::ok(&rig.call(
        OP_SETUP,
        handle,
        &setup_fields(1, "MERCHANT1", &["Bitcoin", "BTC"], &["USD", "EUR"]),
    ))
    .done();
    let s = state(&rig, handle);
    assert_eq!(s.setup.as_ref().map(|x| x.0), Some(1));
    assert_eq!(s.setup.as_ref().map(|x| x.2.len()), Some(2));
    Rd::ok(&rig.call(OP_REMOVE, handle, &[])).done();
    assert!(state(&rig, handle).setup.is_none());
}

#[test]
fn oc_33_a_read_is_a_request_the_app_sends_and_a_reply_it_hands_back() {
    let (rig, handle) = ready(true);
    Rd::ok(&rig.call(OP_SETUP, handle, &square_fields())).done();

    // The request: a POST to Square's search path with the location in the
    // body and no sign-in anywhere in it.
    let response = rig.call(OP_REQUEST, handle, &W::new().u64(NOW).0);
    let mut r = Rd::ok(&response);
    let (method, url, body) = (r.u8(), r.string(), r.bytes());
    r.done();
    assert_eq!(method, 1);
    assert_eq!(url, "https://connect.squareup.com/v2/orders/search");
    let text = String::from_utf8(body).expect("JSON");
    assert!(text.contains("LOCSQ1"), "{text}");
    assert!(!text.to_ascii_lowercase().contains("bearer"));

    // A refused sign-in ends the read and says how to make a new token.
    let f = fault(&rig.call(OP_REPLY, handle, &reply_fields(NOW, 401, b"")));
    assert_eq!(f.status, 1, "{f:?}");
    assert!(!f.words.next.is_empty());
    // A reply with no request waiting is a state, not a fault of the till.
    let f = fault(&rig.call(OP_REPLY, handle, &reply_fields(NOW, 200, SQUARE_PAGE1)));
    assert_eq!(f.status, 3, "{f:?}");
    // The next read may not start within a minute of the last.
    let f = fault(&rig.call(OP_REQUEST, handle, &W::new().u64(NOW + 30).0));
    assert_eq!(f.status, 3, "{f:?}");
    assert!(state(&rig, handle).last_poll_at.is_none());

    // Page 1 says there is more; the next request carries the marker.
    let later = NOW + 60;
    Rd::ok(&rig.call(OP_REQUEST, handle, &W::new().u64(later).0));
    let response = rig.call(OP_REPLY, handle, &reply_fields(later, 200, SQUARE_PAGE1));
    let mut r = Rd::ok(&response);
    let (again, more_left, pages, orders) = (r.bool(), r.bool(), r.u32(), r.u32());
    let (new_matches, new_alerts, cleared, retired) =
        (r.u32(), r.list(read_alert), r.u32(), r.u64());
    r.done();
    assert!(again && more_left && pages == 1 && orders > 0);
    assert!((new_matches, cleared, retired) == (0, 0, 0) && new_alerts.is_empty());
    let response = rig.call(OP_REQUEST, handle, &W::new().u64(later).0);
    let mut r = Rd::ok(&response);
    let (_, _, body) = (r.u8(), r.string(), r.bytes());
    assert!(String::from_utf8(body)
        .expect("JSON")
        .contains("PAGE2OPAQUE-cursor_value.1"));

    // The last page ends the read, and the matching is done on it. The link
    // holds no records of the counter yet, so no sale is there for a Bitcoin
    // tender to match, and its alerts are raised.
    let response = rig.call(OP_REPLY, handle, &reply_fields(later, 200, SQUARE_LAST));
    // (the clock of the matching is the reply's)
    let mut r = Rd::ok(&response);
    let (again, more_left, pages, _orders) = (r.bool(), r.bool(), r.u32(), r.u32());
    let (new_matches, new_alerts, _cleared, _retired) =
        (r.u32(), r.list(read_alert), r.u32(), r.u64());
    r.done();
    assert!(!again && !more_left && pages == 2);
    assert_eq!(new_matches, 0);
    assert!(
        new_alerts.iter().any(|a| a.kind == 0),
        "a Bitcoin tender nobody paid: {new_alerts:?}"
    );
    for a in &new_alerts {
        check_wire_words(&a.words).expect("words");
        assert!(!a.words.what.is_empty() && !a.words.next.is_empty());
    }

    // The screen shows the same alerts, the poll time and the counts.
    let s = state(&rig, handle);
    assert_eq!(s.last_poll_at, Some(later));
    assert!(s.tracked_orders > 0, "the orders with a Bitcoin tender are tracked");
    assert_eq!(s.open.len(), new_alerts.len());
    for (open, new) in s.open.iter().zip(&new_alerts) {
        assert_eq!((open.kind, &open.subject), (new.kind, &new.subject));
        assert_eq!(open.raised_at, new.raised_at);
    }

    // Acknowledging one is a change of the phone's own ledger.
    let first = &s.open[0];
    let Subject::Tender(order, tender) = &first.subject else {
        panic!("a tender alert first: {first:?}")
    };
    Rd::ok(&rig.call(
        OP_ACK,
        handle,
        &ack_fields(later, first.kind, order, tender),
    ))
    .done();
    let after = state(&rig, handle);
    assert_eq!(after.acknowledged, 1);
    assert_eq!(after.open.len(), s.open.len() - 1);
    // An alert that is not open is refused by name.
    let f = fault(&rig.call(OP_ACK, handle, &ack_fields(later, 3, "NOPE", "NOPE")));
    assert_eq!(f.status, 1, "{f:?}");
    // A key that cannot be an alert is refused before the ledger is asked.
    let f = fault(&rig.call(OP_ACK, handle, &ack_fields(later, 0, "", "T")));
    assert_eq!(f.status, 1, "{f:?}");
    let sale = W::new().u64(later).u8(2).u8(1).bytes(&[1, 2, 3]).u32(0).0;
    let f = fault(&rig.call(OP_ACK, handle, &sale));
    assert_eq!((f.status, f.rule.as_str()), (1, "id-length"));
}

#[test]
fn oc_33_a_page_of_8_mib_is_taken_and_one_more_than_the_cap_is_not() {
    let (rig, handle) = ready(true);
    Rd::ok(&rig.call(OP_SETUP, handle, &square_fields())).done();
    // 8 MiB passes the request cap; it reaches the till and is refused there
    // because no request was waiting.
    let big = vec![b' '; 8 << 20];
    let f = fault(&rig.call(OP_REPLY, handle, &reply_fields(NOW, 200, &big)));
    assert_eq!(f.status, 3, "{f:?}");
    // Past the cap of a request, it is refused whole.
    let over = vec![b' '; 9 << 20];
    let f = fault(&rig.call(OP_REPLY, handle, &reply_fields(NOW, 200, &over)));
    assert_eq!((f.status, f.rule.as_str()), (1, "request-too-long"));
}

#[test]
fn oc_31_every_new_request_is_read_strictly() {
    let (rig, handle) = ready(true);
    let requests: Vec<(u16, Vec<u8>)> = vec![
        (OP_SETUP, square_fields()),
        (OP_REQUEST, W::new().u64(NOW).0),
        (OP_REPLY, reply_fields(NOW, 200, b"{}")),
        (OP_ACK, ack_fields(NOW, 0, "O", "T")),
    ];
    for (op, fields) in &requests {
        // Every cut is refused, and so is a byte left over.
        for n in 0..fields.len() {
            let f = fault(&rig.call(*op, handle, &fields[..n]));
            assert_eq!(
                (f.status, f.rule.as_str()),
                (1, "request-malformed"),
                "op {op} cut at {n}"
            );
        }
        let mut more = fields.clone();
        more.push(0);
        let f = fault(&rig.call(*op, handle, &more));
        assert_eq!(
            (f.status, f.rule.as_str()),
            (1, "request-malformed"),
            "op {op}"
        );
    }
    // The ops that take nothing refuse a byte.
    for op in [OP_STATE, OP_REMOVE] {
        let f = fault(&rig.call(op, handle, &[0]));
        assert_eq!(f.rule, "request-malformed", "op {op}");
    }
    // A tag that is not defined: the vendor, an alert kind, a subject.
    let bad_vendor = setup_fields(2, "L", &["Bitcoin"], &["USD"]);
    let bad_kind = ack_fields(NOW, 9, "O", "T");
    let bad_subject = W::new().u64(NOW).u8(0).u8(2).0;
    for (op, fields) in [
        (OP_SETUP, bad_vendor),
        (OP_ACK, bad_kind),
        (OP_ACK, bad_subject),
    ] {
        let f = fault(&rig.call(op, handle, &fields));
        assert_eq!(
            (f.status, f.rule.as_str()),
            (1, "request-malformed"),
            "op {op}"
        );
    }
    // Text that is not UTF-8 in a location.
    let mut bad_text = W::new().u8(0).u32(1).0;
    bad_text.push(0xff);
    bad_text.extend_from_slice(&W::new().u32(0).u32(0).0);
    let f = fault(&rig.call(OP_SETUP, handle, &bad_text));
    assert_eq!(f.rule, "request-malformed");
    // None of it changed anything.
    assert!(state(&rig, handle).setup.is_none());
}

#[test]
fn oc_37_a_refusal_of_the_till_has_words_that_pass_the_checks() {
    let (rig, handle) = ready(true);
    Rd::ok(&rig.call(OP_SETUP, handle, &square_fields())).done();
    for (i, status) in [401u32, 403, 429, 500, 0].into_iter().enumerate() {
        let at = NOW + 60 * i as u64;
        Rd::ok(&rig.call(OP_REQUEST, handle, &W::new().u64(at).0));
        let f = fault(&rig.call(OP_REPLY, handle, &reply_fields(NOW, status, b"")));
        assert_eq!(f.status, 1, "status {status}: {f:?}");
        assert!(f.words.heading.len() <= 28 && !f.words.next.is_empty());
    }
}
