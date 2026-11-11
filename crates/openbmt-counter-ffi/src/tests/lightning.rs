//! CR-112 to CR-116 through `dispatch`: the setup, the sale's invoice, the
//! code that carries it, and a payment, on the lightning crate's recorded
//! replies and a driver the test feeds by hand.

use openbmt_core::settings::Settings;
use openbmt_counter_node::{LightningRecord, LnTransport};
use openbmt_lightning::testing::{
    hash, lnd, lnd_created, lnd_denied, lnd_lookup, lndconnect_text, node_key, pin_fingerprint_hex,
    rune, secret, Make, INVOICE_RUNE,
};
use openbmt_lightning::Credential;

use super::kit::*;
use super::wire::{Rd, Screen, W};
use crate::StartConfig;

const OP_LN_SETUP: u16 = 50;
const OP_LN_REPLY: u16 = 51;
const OP_LN_FAILED: u16 = 52;
const OP_LN_PIN: u16 = 53;
const OP_LN_NEXT: u16 = 54;
const OP_LN_REMOVE: u16 = 55;
const STATE_PAID_LIGHTNING: u8 = 11;

/// A request the node hands out: (id, host:port, method, path, body).
#[derive(Debug, PartialEq, Eq)]
struct Req {
    id: u64,
    host_port: String,
    method: u8,
    path: String,
    header_name: String,
    header_value: String,
    body: Option<String>,
}

fn read_req(r: &mut Rd<'_>) -> Req {
    Req {
        id: r.u64(),
        host_port: r.string(),
        method: r.u8(),
        path: r.string(),
        header_name: r.string(),
        header_value: r.string(),
        body: r.opt(Rd::string),
    }
}

fn settings_with(node: Option<[u8; 33]>) -> openbmt_core::settings::SignedSettings {
    let mut s: Settings = signed_settings().settings;
    s.lightning_node = node;
    openbmt_core::settings::SignedSettings {
        owner_sig: s.sign(&id(1)),
        approver_sig: Some(s.sign(&id(2))),
        settings: s,
    }
}

fn config(rig: &Rig, node: Option<[u8; 33]>, saved: Option<Vec<u8>>) -> StartConfig {
    StartConfig {
        dir: rig.dir().to_str().expect("UTF-8").to_string(),
        now: T0,
        record: record_with(settings_with(node)).to_bytes(),
        proxy: None,
        own_node: None,
        lightning: saved,
        link_proxy: None,
    }
}

fn saved(c: Credential) -> Vec<u8> {
    LightningRecord {
        credential: c,
        node: node_key(1),
    }
    .to_bytes()
}

fn setup_lnd(rig: &Rig, handle: u64) -> Result<Req, Box<super::wire::Fault>> {
    let fields = W::new()
        .u8(0)
        .string(&lndconnect_text())
        .string("")
        .string("")
        .0;
    let response = rig.call(OP_LN_SETUP, handle, &fields);
    if let Some(fault) = Rd::error(&response) {
        return Err(Box::new(fault));
    }
    let mut r = Rd::ok(&response);
    let req = read_req(&mut r);
    r.done();
    Ok(req)
}

/// The answer to a request: the next request and the record, if any.
fn reply(
    rig: &Rig,
    handle: u64,
    id: u64,
    status: u32,
    body: &str,
) -> (Option<Req>, Option<Vec<u8>>) {
    let fields = W::new().u64(id).u32(status).bytes(body.as_bytes()).0;
    let response = rig.call(OP_LN_REPLY, handle, &fields);
    let mut r = Rd::ok(&response);
    let next = r.opt(read_req);
    let record = r.opt(Rd::bytes);
    r.done();
    (next, record)
}

fn next(rig: &Rig, handle: u64, now: u64) -> Option<Req> {
    let response = rig.call(OP_LN_NEXT, handle, &W::new().u64(now).0);
    let mut r = Rd::ok(&response);
    let req = r.opt(read_req);
    r.done();
    req
}

fn judge(rig: &Rig, handle: u64, now: u64, index: u32) -> Screen {
    let response = rig.call(12, handle, &W::new().u64(now).u32(index).0);
    let mut r = Rd::ok(&response);
    let screen = r.screen();
    r.done();
    screen
}

fn open(rig: &Rig, handle: u64) {
    rig.send(vec![peers_event()]);
    rig.call(10, handle, &W::new().u64(T0).u32(50).0);
    let response = rig.call(
        11,
        handle,
        &W::new().open(T0, FIVE_DOLLARS, "USD", RATE, "test-fixed").0,
    );
    Rd::ok(&response);
}

fn created_body(sats: u64) -> String {
    let h = hash(1);
    let mut m = Make::good(h);
    m.msat = Some(sats * 1_000);
    m.description = Some("Sale 0".to_string());
    let reply = lnd_created(&h, &m.write());
    String::from_utf8(reply.body).expect("text")
}

fn lookup_body(state: &str, paid: Option<u64>) -> String {
    let reply = lnd_lookup(&hash(1), 5_000_000, state, paid, paid.map(|_| T0 + 4));
    String::from_utf8(reply.body).expect("text")
}

fn status_line(rig: &Rig, handle: u64) -> Option<(bool, super::wire::Words)> {
    let response = rig.call(30, handle, &[]);
    let mut r = Rd::ok(&response);
    let _ = (r.u32(), r.u32());
    let _ = r.opt(|r| (r.bool(), r.bool(), r.words()));
    let _ = r.opt(|r| (r.bool(), r.words()));
    let _ = r.list(Rd::u32);
    let _ = (r.u64(), r.bool());
    let line = r.opt(|r| (r.bool(), r.words()));
    r.done();
    line
}

#[test]
fn cr_115_the_setup_runs_the_test_and_returns_the_record_to_seal() {
    let rig = Rig::new();
    let handle = rig.start_with(&config(&rig, Some(node_key(1)), None));
    assert!(status_line(&rig, handle).is_none());
    let first = setup_lnd(&rig, handle).expect("the first request");
    assert_eq!(first.host_port, "node.local:8080");
    assert_eq!(first.method, 0);
    // The pinned certificate is the one in the code, and no other.
    let pin = |id: u64, der: &[u8]| {
        let response = rig.call(OP_LN_PIN, handle, &W::new().u64(id).bytes(der).0);
        let mut r = Rd::ok(&response);
        let ok = r.bool();
        r.done();
        ok
    };
    assert!(pin(first.id, &der_of_pin()));
    assert!(!pin(first.id, &[1, 2, 3]));
    assert!(!pin(first.id + 99, &der_of_pin()));
    let denied = String::from_utf8(lnd_denied().body).expect("text");
    let (second, record) = reply(&rig, handle, first.id, 500, &denied);
    assert!(record.is_none());
    let second = second.expect("the second probe");
    let (third, _) = reply(&rig, handle, second.id, 500, &denied);
    let third = third.expect("the test invoice");
    assert_eq!(third.method, 1);
    // The test invoice: one satoshi, signed by the settings' node.
    let h = hash(3);
    let mut m = Make::good(h);
    m.msat = Some(1_000);
    m.description = Some("OpenBMT test".to_string());
    let body = String::from_utf8(lnd_created(&h, &m.write()).body).expect("text");
    let (next_req, record) = reply(&rig, handle, third.id, 200, &body);
    assert!(next_req.is_none());
    let record = record.expect("the record to seal");
    let read = LightningRecord::from_bytes(&record).expect("a record");
    assert_eq!(read.credential, lnd());
    assert_eq!(read.node, node_key(1));
    // It is in use at once, and the status says so.
    let (ready, words) = status_line(&rig, handle).expect("a line");
    assert!(ready);
    assert_eq!(words.heading, "Lightning ready");
    // Removing it deletes it at once.
    Rd::ok(&rig.call(OP_LN_REMOVE, handle, &[])).done();
    assert!(status_line(&rig, handle).is_none());
}

#[test]
fn cr_115_core_lightning_takes_the_address_the_rune_and_the_certificate() {
    let rig = Rig::new();
    let handle = rig.start_with(&config(&rig, Some(node_key(1)), None));
    let fields = W::new()
        .u8(1)
        .string("node.local:8080")
        .string(&rune(INVOICE_RUNE))
        .string(&pin_fingerprint_hex())
        .0;
    let response = rig.call(OP_LN_SETUP, handle, &fields);
    let mut r = Rd::ok(&response);
    let first = read_req(&mut r);
    r.done();
    assert_eq!(first.header_name, "Rune");
    // A rune that allows more than invoices is refused with words.
    let fields = W::new()
        .u8(1)
        .string("node.local:8080")
        .string(&rune("=7"))
        .string(&pin_fingerprint_hex())
        .0;
    let fault = Rd::error(&rig.call(OP_LN_SETUP, handle, &fields)).expect("refused");
    assert!(fault.rule.starts_with("LN-"), "{}", fault.rule);
    assert_eq!(fault.status, 1);
}

#[test]
fn cr_115_no_node_in_the_settings_means_nothing_is_tested_and_the_words_say_so() {
    let rig = Rig::new();
    let handle = rig.start_with(&config(&rig, None, None));
    let fault = setup_lnd(&rig, handle).expect_err("refused");
    assert_eq!(fault.rule, "CR-115");
    assert!(
        fault.words.what.contains("has not named"),
        "{}",
        fault.words.what
    );
    // Nothing is waiting to be sent.
    assert!(next(&rig, handle, T0).is_none());
    assert!(status_line(&rig, handle).is_none());
}

#[test]
fn cr_115_a_setup_that_gets_no_answer_says_why() {
    let rig = Rig::new();
    let handle = rig.start_with(&config(&rig, Some(node_key(1)), None));
    let first = setup_lnd(&rig, handle).expect("a request");
    // PinRefused is tag 0.
    let fault = Rd::error(&rig.call(OP_LN_FAILED, handle, &W::new().u64(first.id).u8(0).0))
        .expect("refused");
    assert_eq!(fault.rule, "CR-114");
    assert_eq!(fault.words.heading, LnTransport::PinRefused.words().heading);
    // The test is over: a later answer is not taken.
    let fault = Rd::error(&rig.call(
        OP_LN_REPLY,
        handle,
        &W::new().u64(first.id).u32(500).bytes(b"{}").0,
    ))
    .map(|f| f.rule);
    assert!(
        fault.is_none(),
        "a stale answer to the node's own number is dropped"
    );
}

#[test]
fn cr_113_a_sale_shows_its_chain_code_then_the_lightning_code_and_is_paid_at_once() {
    let rig = Rig::new();
    let handle = rig.start_with(&config(&rig, Some(node_key(1)), Some(saved(lnd()))));
    open(&rig, handle);
    // The on-chain code is there at once.
    let early = judge(&rig, handle, T0, 0);
    let chain = early.payment.expect("a code");
    assert!(chain.uri.starts_with("BITCOIN:"));
    assert!(!chain.uri.to_lowercase().contains("lightning="));
    let ln = early.lightning.expect("waiting");
    assert!(ln.waiting && !ln.invoice && ln.unavailable.is_none());

    // The node is asked for an invoice, and the answer becomes the code.
    let create = next(&rig, handle, T0).expect("an invoice request");
    assert_eq!(create.path, "/v1/invoices");
    reply(&rig, handle, create.id, 200, &created_body(5_000));
    let shown = judge(&rig, handle, T0, 0);
    let code = shown.payment.expect("a code");
    assert!(code.uri.starts_with("BITCOIN:"));
    assert!(code.uri.contains("LIGHTNING=LNBCRT"), "{}", code.uri);
    assert!(code.copy_uri.starts_with("bitcoin:"));
    assert!(code.copy_uri.contains("&lightning=lnbcrt"));
    assert_eq!(code.address, chain.address);
    let ln = shown.lightning.expect("an invoice");
    assert!(ln.invoice && !ln.waiting);

    // The payment: final at once, with the reports, once.
    let lookup = next(&rig, handle, T0).expect("a lookup");
    reply(
        &rig,
        handle,
        lookup.id,
        200,
        &lookup_body("SETTLED", Some(5_000_000)),
    );
    let response = rig.call(10, handle, &W::new().u64(T0).u32(50).0);
    let mut r = Rd::ok(&response);
    let (reports, _, _) = r.pump();
    r.done();
    assert!(
        reports.contains(&super::wire::Report::PosPaid(0)),
        "{reports:?}"
    );
    let paid = judge(&rig, handle, T0, 0);
    assert_eq!(paid.state, STATE_PAID_LIGHTNING);
    assert_eq!(paid.words.heading, "Paid over Lightning");
    assert!(paid.payment.is_none());
    assert!(paid.lightning.is_none());
    assert!(next(&rig, handle, T0 + 60).is_none());
}

#[test]
fn cr_113_six_seconds_without_an_invoice_leaves_the_chain_code_and_one_line() {
    let rig = Rig::new();
    let handle = rig.start_with(&config(&rig, Some(node_key(1)), Some(saved(lnd()))));
    open(&rig, handle);
    assert!(next(&rig, handle, T0).is_some());
    let late = judge(&rig, handle, T0 + 6, 0);
    assert!(late
        .payment
        .expect("the chain code")
        .uri
        .starts_with("BITCOIN:"));
    let ln = late.lightning.expect("a line");
    let words = ln.unavailable.expect("the line");
    assert_eq!(words.heading, "Lightning not available");
    assert!(words.detail.is_some());
}

#[test]
fn cr_113_an_invoice_for_another_node_is_never_shown() {
    let rig = Rig::new();
    let handle = rig.start_with(&config(&rig, Some(node_key(1)), Some(saved(lnd()))));
    open(&rig, handle);
    let create = next(&rig, handle, T0).expect("a request");
    let h = hash(1);
    let mut m = Make::good(h);
    m.msat = Some(5_000_000);
    m.description = Some("Sale 0".to_string());
    m.key = secret(2);
    let body = String::from_utf8(lnd_created(&h, &m.write()).body).expect("text");
    reply(&rig, handle, create.id, 200, &body);
    let screen = judge(&rig, handle, T0, 0);
    assert!(!screen.payment.expect("code").uri.contains("LIGHTNING="));
    let words = screen.lightning.expect("a line").unavailable.expect("why");
    assert_eq!(words.code.as_deref(), Some("LN-17"));
}

#[test]
fn cr_116_a_saved_connection_for_another_node_is_off_and_the_status_says_so() {
    let rig = Rig::new();
    let handle = rig.start_with(&config(&rig, Some(node_key(2)), Some(saved(lnd()))));
    let (ready, words) = status_line(&rig, handle).expect("a line");
    assert!(!ready);
    assert_eq!(words.heading, "Lightning turned off");
    open(&rig, handle);
    assert!(next(&rig, handle, T0).is_none());
    assert!(judge(&rig, handle, T0, 0).lightning.is_none());
}

#[test]
fn cr_116_an_unreadable_saved_connection_is_off_and_the_status_says_so() {
    let rig = Rig::new();
    let handle = rig.start_with(&config(&rig, Some(node_key(1)), Some(vec![1, 2, 3])));
    let (ready, words) = status_line(&rig, handle).expect("a line");
    assert!(!ready);
    assert_eq!(words.heading, "Lightning connection lost");
}

#[test]
fn cr_114_a_status_that_is_not_a_status_is_refused() {
    let rig = Rig::new();
    let handle = rig.start_with(&config(&rig, Some(node_key(1)), Some(saved(lnd()))));
    let fault = Rd::error(&rig.call(
        OP_LN_REPLY,
        handle,
        &W::new().u64(1).u32(70_000).bytes(b"").0,
    ))
    .expect("refused");
    assert_eq!(fault.rule, "ln-status");
}

/// The DER bytes of the certificate every recorded credential pins.
fn der_of_pin() -> Vec<u8> {
    openbmt_lightning::testing::der(300, 7)
}
