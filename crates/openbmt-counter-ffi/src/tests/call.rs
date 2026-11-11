//! Every op through `dispatch`, on a node with a driver the test feeds by
//! hand.

use bitcoin::hashes::Hash;
use bitcoin::{Address, Network};
use openbmt_core::command::{Action, OwnerCommand, SignedCommand};
use openbmt_core::heartbeat::WatcherHeartbeat;
use openbmt_core::settings::{Settings, SignedSettings};
use openbmt_counter_node::PairingRecord;

use super::kit::*;
use super::wire::{frame, Fault, Rd, Report, W};

/// A rig with a started node that has heard from the network.
fn ready() -> (Rig, u64) {
    let rig = Rig::new();
    let handle = rig.start();
    rig.send(vec![peers_event()]);
    pump(&rig, handle, 50);
    (rig, handle)
}

fn pump(rig: &Rig, handle: u64, wait_ms: u32) -> Vec<Report> {
    let response = rig.call(10, handle, &W::new().u64(T0).u32(wait_ms).0);
    let mut r = Rd::ok(&response);
    let (reports, _, _) = r.pump();
    r.done();
    reports
}

fn open(rig: &Rig, handle: u64, minor: u64) -> super::wire::Opened {
    let response = rig.call(
        11,
        handle,
        &W::new().open(T0, minor, "USD", RATE, "test-fixed").0,
    );
    let mut r = Rd::ok(&response);
    let opened = r.opened();
    r.done();
    opened
}

fn judge(rig: &Rig, handle: u64, index: u32) -> super::wire::Screen {
    let response = rig.call(12, handle, &W::new().u64(T0).u32(index).0);
    let mut r = Rd::ok(&response);
    let screen = r.screen();
    r.done();
    screen
}

fn fault(response: &[u8]) -> Fault {
    Rd::error(response).expect("an error response")
}

fn unit(response: &[u8]) {
    Rd::ok(response).done();
}

#[test]
fn start_and_stop_hold_the_handles_and_the_limit() {
    let rig = Rig::new();
    let first = rig.start();
    assert_eq!(first, 1);
    // A second node on another folder; a third is over the limit.
    let other = Scratch::new();
    rig.arm();
    let second = dispatch_start(&rig, other.path());
    assert_eq!(Rd::ok(&second).u64(), 2);
    let third_dir = Scratch::new();
    rig.arm();
    let third = fault(&dispatch_start(&rig, third_dir.path()));
    assert_eq!((third.status, third.rule.as_str()), (3, "node-limit"));
    // The same folder twice is refused by the folder's lock (NR-79).
    let same = fault(&dispatch_start(&rig, &rig.dir()));
    assert!(
        matches!(same.rule.as_str(), "NR-79" | "node-limit"),
        "{same:?}"
    );
    // Stop makes room, and the handle is gone.
    unit_pump(&rig.call(2, first, &[]));
    assert_eq!(fault(&rig.call(2, first, &[])).rule, "handle-unknown");
    rig.arm();
    let again = dispatch_start(&rig, third_dir.path());
    assert_eq!(Rd::ok(&again).u64(), 3);
}

fn dispatch_start(rig: &Rig, dir: &std::path::Path) -> Vec<u8> {
    crate::dispatch(rig.registry(), &start_request(dir))
}

fn unit_pump(response: &[u8]) {
    let mut r = Rd::ok(response);
    r.pump();
    r.done();
}

#[test]
fn start_refuses_what_the_pairing_record_does_not_hold() {
    let rig = Rig::new();
    let dir_path = rig.dir();
    let dir = dir_path.to_str().expect("UTF-8");
    let base = start_config(dir);
    let with = |record: PairingRecord| {
        let mut c = base.clone();
        c.record = record.to_bytes();
        c
    };
    let mut cases: Vec<(&str, crate::StartConfig)> = Vec::new();
    let mut c = base.clone();
    c.dir = String::new();
    cases.push(("start-dir", c));
    // A record that does not read: junk, and a secret that is not a key.
    let mut c = base.clone();
    c.record = vec![1, 2, 3];
    cases.push(("CR-97", c));
    let mut c = base.clone();
    c.record[5..37].fill(0);
    cases.push(("CR-97", c));
    // Settings signed by the owner alone are loosening and need both.
    let mut signed = signed_settings();
    signed.approver_sig = None;
    cases.push(("CR-94", with(record_with(signed))));
    // Settings that are not newer than the pairing's floor.
    let mut r = record();
    r.settings_floor = r.settings.settings.version;
    cases.push(("CR-94", with(r)));
    // Settings that do not list this counter.
    let mut s = Settings::defaults(1_000);
    s.version = 1;
    s.devices.insert(id(4).device_id());
    s.counters.insert(id(4).device_id(), 10_000);
    let other = SignedSettings {
        owner_sig: s.sign(&id(1)),
        approver_sig: Some(s.sign(&id(2))),
        settings: s,
    };
    cases.push(("CR-95", with(record_with(other))));
    // A first batch for another counter.
    let mut r = record();
    if let Some(b) = r.first_batch.as_mut() {
        b.batch.counter = id(4).device_id();
    }
    cases.push(("CR-96", with(r)));
    let mut c = base;
    c.proxy = Some("not an address".to_string());
    cases.push(("start-proxy", c));
    for (rule, config) in cases {
        let response = rig.call(1, 0, &W::new().start(&config).0);
        let f = fault(&response);
        assert_eq!((f.status, f.rule.as_str()), (1, rule), "{f:?}");
        assert!(!f.words.heading.is_empty());
    }
    // None of that left a node behind.
    rig.arm();
    assert_eq!(rig.start(), 1);
}

#[test]
fn cr_87_the_node_does_not_start_with_tor_on_and_no_proxy() {
    let rig = Rig::new();
    let dir = rig.dir();
    let mut tor = signed_settings();
    tor.settings.tor_on = true;
    tor.owner_sig = tor.settings.sign(&id(1));
    tor.approver_sig = Some(tor.settings.sign(&id(2)));
    let mut c = start_config(dir.to_str().expect("UTF-8"));
    c.record = record_with(tor).to_bytes();
    // Tor on, no proxy: refused, with the rule in its Display.
    let f = fault(&rig.call(1, 0, &W::new().start(&c).0));
    assert_eq!((f.status, f.rule.as_str()), (1, "CR-87"), "{f:?}");
    assert!(f.text.contains("CR-87"), "{f:?}");
    assert!(!f.words.heading.is_empty());
    // Tor on, proxy given: starts.
    c.proxy = Some("127.0.0.1:9050".to_string());
    rig.arm();
    assert_eq!(rig.start_with(&c), 1);
    // Tor off, no proxy: starts (the default settings).
    let rig = Rig::new();
    rig.arm();
    assert_eq!(rig.start(), 1);
}

#[test]
fn pump_open_judge_requote_and_cancel() {
    let (rig, handle) = ready();
    let opened = open(&rig, handle, FIVE_DOLLARS);
    assert_eq!((opened.index, opened.needs_sats), (0, 5_000));

    // A new quote at twice the price halves what the sale needs, and the
    // payment request follows it.
    let response = rig.call(
        13,
        handle,
        &W::new()
            .u64(T0 + 1)
            .u32(0)
            .u64(RATE * 2)
            .string("test-fixed")
            .u64(T0 + 1)
            .0,
    );
    let mut r = Rd::ok(&response);
    let quote = r.opened();
    r.done();
    assert_eq!(quote.index, 0);
    assert_eq!(quote.needs_sats, 2_500);
    assert_eq!(quote.quote_rate, RATE * 2);
    assert!(
        quote.payment.uri.ends_with("?amount=0.000025"),
        "{}",
        quote.payment.uri
    );
    assert_eq!(quote.payment.address, opened.payment.address);
    assert_ne!(quote.payment.bits, opened.payment.bits);
    assert_eq!(judge(&rig, handle, 0).needs, 2_500);

    // A bad rate and a bad label are refused before the node is asked.
    let f = fault(&rig.call(
        13,
        handle,
        &W::new().u64(T0).u32(0).u64(0).string("x").u64(T0).0,
    ));
    assert_eq!(f.rule, "sale-rate");
    let f = fault(&rig.call(
        13,
        handle,
        &W::new().u64(T0).u32(0).u64(RATE).string("").u64(T0).0,
    ));
    assert_eq!(f.rule, "sale-source");
    let f = fault(&rig.call(
        11,
        handle,
        &W::new().open(T0, 100, "usd", RATE, "test-fixed").0,
    ));
    assert_eq!(f.rule, "sale-currency");
    let f = fault(&rig.call(12, handle, &W::new().u64(T0).u32(99).0));
    assert_eq!(f.status, 1);
    assert!(!f.words.what.is_empty());

    // Cancel a sale with nothing received; it ends as cancelled.
    unit(&rig.call(14, handle, &W::new().u64(T0).u32(0).0));
    let screen = judge(&rig, handle, 0);
    assert_eq!(screen.state, 9, "Cancelled");
    assert_eq!(screen.payment, None, "an ended sale shows no code");
    let f = fault(&rig.call(14, handle, &W::new().u64(T0).u32(0).0));
    assert_eq!(f.status, 1);
}

#[test]
fn an_override_returns_its_reports_and_a_refusal_keeps_them() {
    let (rig, handle) = ready();
    open(&rig, handle, FIVE_DOLLARS);
    // Part of the sale has arrived.
    let part = spend(1, &[(&script_of(0), 2_000)]);
    rig.send(vec![sighting(&part, vec![peer(20)])]);
    pump(&rig, handle, 50);
    assert_eq!(judge(&rig, handle, 0).state, 1, "PartPaid");
    let response = rig.call(15, handle, &W::new().u64(T0).u32(0).actor(true, "mia").0);
    let mut r = Rd::ok(&response);
    let reports = r.reports();
    r.done();
    assert_eq!(reports, vec![Report::PosPaid(0)]);
    assert_eq!(judge(&rig, handle, 0).state, 6, "Overridden");
    // Again: refused, with the counter's words.
    let f = fault(&rig.call(15, handle, &W::new().u64(T0).u32(0).actor(true, "mia").0));
    assert_eq!(f.status, 1);
    assert!(!f.words.heading.is_empty() && !f.words.next.is_empty());
    // No sale, no override.
    let f = fault(&rig.call(15, handle, &W::new().u64(T0).u32(7).actor(false, "ann").0));
    assert_eq!(f.status, 1);
}

#[test]
fn refunds_are_asked_listed_withdrawn_and_paid_in_cash() {
    let (rig, handle) = ready();
    open(&rig, handle, FIVE_DOLLARS);
    let pay = spend(1, &[(&script_of(0), 5_000)]);
    rig.send(vec![sighting(&pay, vec![peer(20)])]);
    pump(&rig, handle, 50);
    let block = mine(genesis(), 1, std::slice::from_ref(&pay));
    rig.send(vec![joined(1, &block), block_event(&block)]);
    assert_eq!(pump(&rig, handle, 50), vec![Report::PosPaid(0)]);

    let destination = Address::from_script(&p2wsh(0x77), Network::Regtest)
        .expect("an address")
        .to_string();
    let ask_sats = |sats: u64, to: &str| {
        W::new()
            .u64(T0)
            .u32(0)
            .u8(0)
            .u64(sats)
            .string(to)
            .actor(false, "ann")
            .string("wrong item")
            .0
    };
    let response = rig.call(16, handle, &ask_sats(1_000, &destination));
    let mut r = Rd::ok(&response);
    let number = r.u64();
    r.done();
    // A refund asked at the dollar price carries its rate.
    let value = W::new()
        .u64(T0)
        .u32(0)
        .u8(1)
        .u64(100)
        .u64(RATE)
        .string("test-fixed")
        .u64(T0)
        .string(&destination)
        .actor(true, "mia")
        .string("changed mind")
        .0;
    let response = rig.call(16, handle, &value);
    let mut r = Rd::ok(&response);
    let second = r.u64();
    r.done();
    assert_ne!(number, second);
    // A destination that is not an address is refused with the rule of the
    // destination check.
    let f = fault(&rig.call(16, handle, &ask_sats(1_000, "not an address")));
    assert_eq!((f.status, f.rule.as_str()), (1, "CR-62"));

    let list = |rig: &Rig| {
        let response = rig.call(19, handle, &W::new().u32(0).0);
        let mut r = Rd::ok(&response);
        let rows = r.list(|r| {
            let sale = r.u32();
            let number = r.opt(Rd::u64);
            let asked = r.u64();
            let value = r.opt(Rd::u64);
            let state = r.u8();
            let role = r.u8();
            let name = r.string();
            let at = r.u64();
            let words = r.words();
            (sale, number, asked, value, state, role, name, at, words)
        });
        r.done();
        rows
    };
    let rows = list(&rig);
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].0, 0);
    assert_eq!(rows[0].1, Some(number));
    assert_eq!(rows[0].2, 1_000);
    assert_eq!(rows[0].6, "ann");
    assert_eq!(rows[1].3, Some(100));
    assert_eq!(rows[1].5, 1, "a manager");
    assert!(rows
        .iter()
        .all(|row| row.7 == T0 && !row.8.heading.is_empty()));

    // Withdraw the first; cash for part of the sale.
    unit(&rig.call(
        17,
        handle,
        &W::new().u64(T0).u32(0).u64(number).actor(false, "ann").0,
    ));
    assert_eq!(list(&rig)[0].4, 2, "Withdrawn");
    let f = fault(&rig.call(
        17,
        handle,
        &W::new().u64(T0).u32(0).u64(424_242).actor(false, "ann").0,
    ));
    assert_eq!(f.status, 1);
    unit(
        &rig.call(
            18,
            handle,
            &W::new()
                .u64(T0)
                .u32(0)
                .actor(true, "mia")
                .u64(100)
                .string("till")
                .0,
        ),
    );
    let rows = list(&rig);
    assert_eq!(rows.len(), 3);
    assert_eq!((rows[2].1, rows[2].4), (None, 3), "paid in cash");
}

fn heartbeat_bytes(seq: u64) -> Vec<u8> {
    WatcherHeartbeat {
        counter: id(3).device_id(),
        seq,
        time: T0,
        tip_height: 0,
        tip_hash: genesis().to_byte_array(),
        healthy: true,
    }
    .sign(&id(1))
    .to_bytes()
}

#[test]
fn a_heartbeat_is_taken_and_a_bad_one_is_refused_with_words() {
    let (rig, handle) = ready();
    let response = rig.call(20, handle, &W::new().u64(T0).bytes(&heartbeat_bytes(1)).0);
    let mut r = Rd::ok(&response);
    let verified = r.bool();
    let sent_at = r.opt(Rd::u64);
    let words = r.words();
    let reports = r.reports();
    r.done();
    assert!(verified);
    assert_eq!(sent_at, Some(T0));
    assert!(!words.heading.is_empty());
    assert!(reports.is_empty());
    // The same number again, and bytes that are no heartbeat.
    let f = fault(&rig.call(20, handle, &W::new().u64(T0).bytes(&heartbeat_bytes(1)).0));
    assert_eq!(f.status, 1);
    assert!(f.rule.starts_with("CR-"), "{}", f.rule);
    let f = fault(&rig.call(20, handle, &W::new().u64(T0).bytes(&[1, 2, 3]).0));
    assert_eq!(f.rule, "CR-50");
    assert!(!f.words.what.is_empty());
}

#[test]
fn a_command_a_check_in_and_a_batch_are_taken() {
    let (rig, handle) = ready();
    let command = OwnerCommand {
        counter: id(3).device_id(),
        seq: 1,
        action: Action::ClearCancelAlarm,
    };
    let signed = SignedCommand {
        owner_sig: command.sign(&id(1)),
        command,
    };
    unit(&rig.call(21, handle, &W::new().u64(T0).bytes(&signed.to_bytes()).0));
    // The number is used up.
    let f = fault(&rig.call(21, handle, &W::new().u64(T0).bytes(&signed.to_bytes()).0));
    assert_eq!(f.rule, "CR-14");
    let f = fault(&rig.call(21, handle, &W::new().u64(T0).bytes(&[9; 4]).0));
    assert_eq!(f.rule, "command-bytes");

    let response = rig.call(22, handle, &W::new().u64(T0).0);
    let mut r = Rd::ok(&response);
    let record = r.bytes();
    r.done();
    assert!(!record.is_empty());

    unit(&rig.call(
        23,
        handle,
        &W::new().u64(T0).bytes(&batch(10, 10).to_bytes()).0,
    ));
    // The same batch again is not the next one.
    let f = fault(&rig.call(
        23,
        handle,
        &W::new().u64(T0).bytes(&batch(10, 10).to_bytes()).0,
    ));
    assert_eq!(f.rule, "batch-refused");
    let f = fault(&rig.call(23, handle, &W::new().u64(T0).bytes(&[1]).0));
    assert_eq!(f.rule, "batch-bytes");
}

#[test]
fn status_and_notes_answer_before_and_after_the_network() {
    let rig = Rig::new();
    let handle = rig.start();
    let status = |rig: &Rig| {
        let response = rig.call(30, handle, &[]);
        let mut r = Rd::ok(&response);
        let checked = r.u32();
        let tip = r.u32();
        let health = r.opt(|r| (r.bool(), r.bool(), r.words()));
        assert!(
            r.opt(|r| (r.bool(), r.words())).is_none(),
            "no own node set"
        );
        let sales = r.list(Rd::u32);
        let below = r.u64();
        let stopped = r.bool();
        assert!(
            r.opt(|r| (r.bool(), r.words())).is_none(),
            "no Lightning credential saved"
        );
        r.done();
        (checked, tip, health, sales, below, stopped)
    };
    let (checked, tip, health, sales, below, stopped) = status(&rig);
    assert_eq!((checked, tip), (0, 0));
    assert!(health.is_none(), "no answer before the first peers event");
    assert_eq!((sales, below, stopped), (vec![], 0, false));
    rig.send(vec![peers_event()]);
    pump(&rig, handle, 50);
    let (_, _, health, _, _, _) = status(&rig);
    assert!(health.expect("an answer").0);

    let response = rig.call(31, handle, &[]);
    let mut r = Rd::ok(&response);
    let notes = r.list(Rd::string);
    r.done();
    assert!(notes.is_empty());
    // A driver that says "stopped" is shown in the status and the pump.
    rig.send(vec![openbmt_net_io::AppEvent::Note("a line".to_string())]);
    let response = rig.call(10, handle, &W::new().u64(T0).u32(50).0);
    let mut r = Rd::ok(&response);
    let (reports, notes, stopped) = r.pump();
    assert_eq!(
        (reports, notes, stopped),
        (vec![], vec!["a line".to_string()], false)
    );
    // Every op that needs a node refuses a handle that is not one.
    for op in [2u16, 10, 12, 30, 31] {
        let f = fault(&rig.call(op, 99, &W::new().u64(T0).u32(0).0));
        assert_eq!(f.rule, "handle-unknown", "op {op}");
    }
    let _ = frame(0, 0, &[]);
}

#[test]
fn a_report_of_each_kind_is_written_as_the_protocol_says() {
    use crate::codec::Put;
    use crate::views::ReportView;
    use openbmt_counter::{Amount, Currency, Report as R};
    let usd = Currency::new(*b"USD").expect("a currency");
    let cases = [
        (R::PosPaid { index: 7 }, vec![0, 7, 0, 0, 0]),
        (R::PosUnpaid { index: 7 }, vec![1, 7, 0, 0, 0]),
        (
            R::BooksFinal {
                index: 1,
                amount: Amount(2),
                currency: usd,
                received_sats: 3,
            },
            vec![
                2, 1, 0, 0, 0, 2, 0, 0, 0, 0, 0, 0, 0, 3, 0, 0, 0, b'U', b'S', b'D', 3, 0, 0, 0, 0,
                0, 0, 0,
            ],
        ),
        (
            R::SurplusOwed { index: 1, sats: 2 },
            vec![3, 1, 0, 0, 0, 2, 0, 0, 0, 0, 0, 0, 0],
        ),
        (
            R::LatePayment { index: 1, sats: 2 },
            vec![4, 1, 0, 0, 0, 2, 0, 0, 0, 0, 0, 0, 0],
        ),
        (R::Alarm { index: 1 }, vec![5, 1, 0, 0, 0]),
        (
            R::Dust { index: 1, sats: 2 },
            vec![6, 1, 0, 0, 0, 2, 0, 0, 0, 0, 0, 0, 0],
        ),
    ];
    for (report, bytes) in cases {
        let mut out = Vec::new();
        ReportView(report).put(&mut out);
        assert_eq!(out, bytes);
    }
}
