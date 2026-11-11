//! Adding a counter from end to end (`docs/SESSION-RULES.md`, AS-69 to
//! AS-73): the counter's request read, the settings and the first batch
//! queued, the session through the real Key app, the response taken, the
//! counter bundle, and the counter's own ceremony accepting it. Every call
//! on the owner's side goes through `dispatch`. Each refusal has its test.

// A refusal is a whole error response and a taken response a tuple of the
// wire's rows; the tests read them as they are.
#![allow(clippy::result_large_err, clippy::type_complexity)]

use bitcoin::Network;
use openbmt_approver::{Approver, Choice};
use openbmt_core::pairing::PairOffer;
use openbmt_counter_node::{batch_policy, Ceremony, BATCH_MIN_COUNT};

use super::kit::*;
use super::screen::check_wire_words;
use super::wire::{Rd, Words, W};

const OUTCOME_DELIVERED: u8 = 0;

fn words(w: &Words) {
    check_wire_words(w).unwrap_or_else(|why| panic!("{why}: {w:?}"));
}

/// What the Key app shows as the pairing offer.
fn offer_for(network: Network) -> Vec<u8> {
    PairOffer::new(
        id(0x11).device_id(),
        id(0x22).device_id(),
        kit().wallet.checksum32(),
        network,
    )
    .expect("an offer")
    .to_bytes()
}

/// A counter's ceremony: its request and the words it shows.
fn begin(n: u8, offer: &[u8], now: u64) -> (Ceremony, Vec<u8>, Vec<String>) {
    let (ceremony, started) =
        Ceremony::begin(offer, "Front counter", [n; 32], [n ^ 0x55; 32], now).expect("a request");
    let words = started.words.iter().map(|w| (*w).to_string()).collect();
    (ceremony, started.request, words)
}

/// What `counter_scan` gave back.
#[derive(Debug)]
struct Scanned {
    pairing: u32,
    counter: Vec<u8>,
    name: String,
    session_words: Vec<String>,
    needs_rate: bool,
    window: u32,
    shares: Vec<(Vec<u8>, u32)>,
}

fn scan(rig: &Rig, handle: u64, request: &[u8]) -> Result<Scanned, super::wire::Fault> {
    let response = rig.call(90, handle, &W::new().bytes(request).0);
    if let Some(e) = Rd::error(&response) {
        words(&e.words);
        return Err(e);
    }
    let mut r = Rd::ok(&response);
    let s = Scanned {
        pairing: r.u32(),
        counter: r.bytes(),
        name: r.string(),
        session_words: r.list(Rd::string),
        needs_rate: r.bool(),
        window: r.u32(),
        shares: r.list(|r| (r.bytes(), r.u32())),
    };
    words(&r.words());
    r.done();
    Ok(s)
}

/// What `counter_confirm` gave back: settings, version, batch, count, shares.
type Confirmed = (u32, u32, u32, u32, Vec<(Vec<u8>, u32)>);

fn confirm(
    rig: &Rig,
    handle: u64,
    now: u64,
    pairing: u32,
    rate: u64,
) -> Result<Confirmed, super::wire::Fault> {
    let response = rig.call(91, handle, &W::new().u64(now).u32(pairing).u64(rate).0);
    if let Some(e) = Rd::error(&response) {
        words(&e.words);
        return Err(e);
    }
    let mut r = Rd::ok(&response);
    let c = (
        r.u32(),
        r.u32(),
        r.u32(),
        r.u32(),
        r.list(|r| (r.bytes(), r.u32())),
    );
    words(&r.words());
    r.done();
    Ok(c)
}

fn waiting(rig: &Rig, handle: u64) -> Vec<(Vec<u8>, String)> {
    let response = rig.call(92, handle, &[]);
    let mut r = Rd::ok(&response);
    let rows = r.list(|r| (r.bytes(), r.string()));
    r.done();
    rows
}

fn bundle(rig: &Rig, handle: u64, counter: &[u8]) -> Result<(Vec<u8>, bool), super::wire::Fault> {
    let response = rig.call(93, handle, &W::new().bytes(counter).0);
    if let Some(e) = Rd::error(&response) {
        words(&e.words);
        return Err(e);
    }
    let mut r = Rd::ok(&response);
    let out = (r.bytes(), r.bool());
    words(&r.words());
    r.done();
    Ok(out)
}

/// The network answered and the wallet holds a coin. The batch the owner
/// queued is watched, so this comes after `counter_confirm`.
fn ready(rig: &Rig, handle: u64) {
    rig.send(vec![peers_event()]);
    rig.pump(handle);
    rig.fund(handle, 300_000);
}

/// Build the approval session and return its request.
fn build(rig: &Rig, handle: u64, now: u64) -> Vec<u8> {
    let response = rig.call(30, handle, &W::new().u64(now).u64(1_000).0);
    let mut r = Rd::ok(&response);
    let (_, request, _) = r.built();
    r.done();
    request
}

/// The Key app approves everything and returns the words it showed for the
/// pairing and its response.
fn approve(approver: &mut Approver, request: &[u8], now: u64) -> (Vec<String>, Vec<u8>) {
    let mut session = approver.open(request, now).expect("a session");
    let mut shown = Vec::new();
    let items: Vec<u32> = session.lines().iter().map(|l| l.item).collect();
    for item in items {
        let line = session
            .lines()
            .into_iter()
            .find(|l| l.item == item)
            .expect("a line");
        if let openbmt_approver::Detail::Pairing(p) = &line.detail {
            shown = p
                .session_words
                .expect("the words")
                .iter()
                .map(|w| (*w).to_string())
                .collect();
        }
        assert!(line.can_approve, "{line:?}");
        session.decide(item, Choice::Approve).expect("decided");
    }
    let response = session
        .finish(|_| Ok::<(), String>(()))
        .expect("a response");
    (shown, response)
}

fn take(
    rig: &Rig,
    handle: u64,
    now: u64,
    response: &[u8],
) -> Result<Vec<(u32, u8, u32, Option<String>, Words)>, super::wire::Fault> {
    let response = rig.call(32, handle, &W::new().u64(now).bytes(response).0);
    if let Some(e) = Rd::error(&response) {
        words(&e.words);
        return Err(e);
    }
    let mut r = Rd::ok(&response);
    let (_, outcomes, _) = r.taken();
    r.done();
    Ok(outcomes)
}

#[test]
fn as_69_to_73_a_counter_is_added_from_its_request_to_its_first_sale() {
    let rig = Rig::new();
    let handle = rig.start();
    let offer = offer_for(NET);
    let (mut ceremony, request, counter_words) = begin(0x32, &offer, T0);

    // AS-69: the request is read; the words are the counter's own.
    let scanned = scan(&rig, handle, &request).expect("scanned");
    assert_eq!(scanned.name, "Front counter");
    assert_eq!(scanned.session_words, counter_words);
    assert!(scanned.needs_rate, "a fresh phone holds no settings");
    assert_eq!(scanned.window, 600);
    assert_eq!(scanned.shares, vec![(scanned.counter.clone(), 10_000)]);
    assert_eq!(
        waiting(&rig, handle),
        vec![],
        "not waiting before the Key app approved"
    );

    // AS-70, AS-71: settings and the first batch, in one step.
    let (_, version, _, count, shares) =
        confirm(&rig, handle, T0 + 20, scanned.pairing, 1_000).expect("confirmed");
    assert_eq!(version, 1);
    assert_eq!(count, BATCH_MIN_COUNT, "the counter's own minimum");
    assert_eq!(shares, scanned.shares);

    // The approval session: the Key app shows the same four words.
    ready(&rig, handle);
    let session_request = build(&rig, handle, T0 + 30);
    let mut approver = key_app();
    let (key_words, response) = approve(&mut approver, &session_request, T0 + 40);
    assert_eq!(key_words, counter_words);
    let outcomes = take(&rig, handle, T0 + 50, &response).expect("taken");
    let delivered = outcomes.iter().filter(|o| o.1 == OUTCOME_DELIVERED).count();
    assert!(delivered >= 3, "{outcomes:?}");

    // AS-73: the counter waits for its code.
    assert_eq!(
        waiting(&rig, handle),
        vec![(scanned.counter.clone(), "Front counter".to_string())]
    );

    // AS-72: the code; the counter says "They match" and takes it.
    let (code, with_batch) = bundle(&rig, handle, &scanned.counter).expect("a bundle");
    assert!(with_batch);
    ceremony.confirm(T0 + 60).expect("they match");
    let record = ceremony
        .finish(&code, &batch_policy(), T0 + 70)
        .expect("the counter takes it");
    assert_eq!(record.settings.settings.version, 1);
    assert_eq!(
        record.first_batch.as_ref().map(|b| b.batch.count),
        Some(BATCH_MIN_COUNT)
    );

    // Showing the code again changes nothing; then the counter is marked
    // paired and nothing waits.
    assert_eq!(
        bundle(&rig, handle, &scanned.counter).expect("again").0,
        code
    );
    Rd::ok(&rig.call(94, handle, &W::new().bytes(&scanned.counter).0)).done();
    assert_eq!(waiting(&rig, handle), vec![]);
    let fault = bundle(&rig, handle, &scanned.counter).expect_err("delivered");
    assert!(fault.rule.starts_with("AS-"), "{fault:?}");

    // A restart on the same folder keeps what was done.
    Rd::ok(&rig.call(2, handle, &[])).pump();
    rig.arm();
    let handle = rig.start();
    assert_eq!(waiting(&rig, handle), vec![]);
    Rd::ok(&rig.call(2, handle, &[])).pump();
}

#[test]
fn as_72_a_waiting_counter_survives_a_restart_and_its_code_is_the_same() {
    let rig = Rig::new();
    let handle = rig.start();
    let (_, request, _) = begin(0x32, &offer_for(NET), T0);
    let s = scan(&rig, handle, &request).expect("scanned");
    confirm(&rig, handle, T0 + 20, s.pairing, 1_000).expect("confirmed");
    ready(&rig, handle);
    let session_request = build(&rig, handle, T0 + 30);
    let (_, response) = approve(&mut key_app(), &session_request, T0 + 40);
    take(&rig, handle, T0 + 50, &response).expect("taken");
    let (code, _) = bundle(&rig, handle, &s.counter).expect("a bundle");

    Rd::ok(&rig.call(2, handle, &[])).pump();
    rig.arm();
    let handle = rig.start();
    assert_eq!(waiting(&rig, handle).len(), 1);
    assert_eq!(bundle(&rig, handle, &s.counter).expect("a bundle").0, code);
    Rd::ok(&rig.call(2, handle, &[])).pump();
}

#[test]
fn as_70_a_second_counter_splits_the_instant_limit_in_equal_shares() {
    let rig = Rig::new();
    let handle = rig.start();
    let offer = offer_for(NET);
    let (_, first, _) = begin(0x32, &offer, T0);
    let one = scan(&rig, handle, &first).expect("scanned");
    confirm(&rig, handle, T0 + 20, one.pairing, 1_000).expect("confirmed");
    ready(&rig, handle);
    let session_request = build(&rig, handle, T0 + 30);
    let (_, response) = approve(&mut key_app(), &session_request, T0 + 40);
    take(&rig, handle, T0 + 50, &response).expect("taken");

    // The phone holds settings now: no rate is needed, and the shares are
    // halves.
    let (_, second, _) = begin(0x33, &offer, T0 + 100);
    let two = scan(&rig, handle, &second).expect("scanned");
    assert!(!two.needs_rate);
    assert_eq!(two.shares.len(), 2);
    assert_eq!(two.shares.iter().map(|s| s.1).sum::<u32>(), 10_000);
    assert!(two.shares.iter().all(|s| s.1 == 5_000));
    let (_, version, _, _, _) = confirm(&rig, handle, T0 + 120, two.pairing, 0).expect("confirmed");
    assert_eq!(version, 2);
    Rd::ok(&rig.call(2, handle, &[])).pump();
}

#[test]
fn as_69_a_code_that_is_not_a_counter_request_is_refused() {
    let rig = Rig::new();
    let handle = rig.start();
    let e = scan(&rig, handle, b"OBMJ-not-a-request").expect_err("refused");
    assert_eq!(e.rule, "AS-58", "{e:?}");
    let e = scan(&rig, handle, &offer_for(NET)).expect_err("an offer is not a request");
    assert_eq!(e.rule, "AS-58", "{e:?}");
    assert!(waiting(&rig, handle).is_empty());
    Rd::ok(&rig.call(2, handle, &[])).pump();
}

#[test]
fn as_69_a_request_for_another_shop_or_network_is_refused() {
    let rig = Rig::new();
    let handle = rig.start();
    // Another network: the counter's ceremony names mainnet.
    let (_, request, _) = begin(0x32, &offer_for(Network::Bitcoin), T0);
    let e = scan(&rig, handle, &request).expect_err("refused");
    assert!(e.rule.starts_with("AS-"), "{e:?}");
    // Another shop: an offer from other roots.
    let other = PairOffer::new(
        id(0x41).device_id(),
        id(0x22).device_id(),
        kit().wallet.checksum32(),
        NET,
    )
    .expect("an offer")
    .to_bytes();
    let (_, request, _) = begin(0x32, &other, T0);
    let e = scan(&rig, handle, &request).expect_err("refused");
    assert!(e.rule.starts_with("AS-"), "{e:?}");
    Rd::ok(&rig.call(2, handle, &[])).pump();
}

#[test]
fn as_69_a_counter_that_is_waiting_or_paired_is_refused_again() {
    let rig = Rig::new();
    let handle = rig.start();
    let (_, request, _) = begin(0x32, &offer_for(NET), T0);
    let s = scan(&rig, handle, &request).expect("scanned");
    // Queued already.
    let e = scan(&rig, handle, &request).expect_err("refused");
    assert_eq!(e.rule, "AS-69", "{e:?}");
    // Paired and waiting.
    confirm(&rig, handle, T0 + 20, s.pairing, 1_000).expect("confirmed");
    ready(&rig, handle);
    let session_request = build(&rig, handle, T0 + 30);
    let (_, response) = approve(&mut key_app(), &session_request, T0 + 40);
    take(&rig, handle, T0 + 50, &response).expect("taken");
    let e = scan(&rig, handle, &request).expect_err("refused");
    assert_eq!(e.rule, "AS-69", "{e:?}");
    Rd::ok(&rig.call(2, handle, &[])).pump();
}

#[test]
fn as_70_the_start_needs_a_rate_when_the_phone_holds_no_settings() {
    let rig = Rig::new();
    let handle = rig.start();
    let (_, request, _) = begin(0x32, &offer_for(NET), T0);
    let s = scan(&rig, handle, &request).expect("scanned");
    let e = confirm(&rig, handle, T0 + 20, s.pairing, 0).expect_err("refused");
    assert_eq!(e.rule, "AS-70", "{e:?}");
    // Nothing was queued: a rate makes it work.
    confirm(&rig, handle, T0 + 30, s.pairing, 1_000).expect("confirmed");
    Rd::ok(&rig.call(2, handle, &[])).pump();
}

#[test]
fn as_70_a_pairing_that_was_never_queued_is_refused() {
    let rig = Rig::new();
    let handle = rig.start();
    let e = confirm(&rig, handle, T0, 77, 1_000).expect_err("refused");
    assert!(e.rule.starts_with("AS-"), "{e:?}");
    Rd::ok(&rig.call(2, handle, &[])).pump();
}

#[test]
fn as_69_a_full_pairing_queue_refuses_the_next_request() {
    let rig = Rig::new();
    let handle = rig.start();
    let offer = offer_for(NET);
    for n in 1..=16u8 {
        let (_, request, _) = begin(0x50 + n, &offer, T0);
        scan(&rig, handle, &request).expect("queued");
    }
    let (_, request, _) = begin(0x70, &offer, T0);
    let e = scan(&rig, handle, &request).expect_err("full");
    assert!(e.text.contains("full") || !e.rule.is_empty(), "{e:?}");
    Rd::ok(&rig.call(2, handle, &[])).pump();
}

#[test]
fn as_72_a_bundle_before_the_approval_is_not_ready() {
    let rig = Rig::new();
    let handle = rig.start();
    let (_, request, _) = begin(0x32, &offer_for(NET), T0);
    let s = scan(&rig, handle, &request).expect("scanned");
    let e = bundle(&rig, handle, &s.counter).expect_err("not ready");
    assert!(e.rule.starts_with("AS-"), "{e:?}");
    confirm(&rig, handle, T0 + 20, s.pairing, 1_000).expect("confirmed");
    let e = bundle(&rig, handle, &s.counter).expect_err("still not ready");
    assert!(e.rule.starts_with("AS-"), "{e:?}");
    let e = bundle(&rig, handle, &[7u8; 32]).expect_err("an unknown counter");
    assert!(e.rule.starts_with("AS-"), "{e:?}");
    let e = bundle(&rig, handle, &[7u8; 5]).expect_err("not a root");
    assert!(!e.rule.is_empty(), "{e:?}");
    Rd::ok(&rig.call(2, handle, &[])).pump();
}

#[test]
fn as_71_a_key_app_that_declines_leaves_the_counter_unpaired() {
    let rig = Rig::new();
    let handle = rig.start();
    let (_, request, _) = begin(0x32, &offer_for(NET), T0);
    let s = scan(&rig, handle, &request).expect("scanned");
    confirm(&rig, handle, T0 + 20, s.pairing, 1_000).expect("confirmed");
    ready(&rig, handle);
    let session_request = build(&rig, handle, T0 + 30);
    let mut approver = key_app();
    let mut session = approver.open(&session_request, T0 + 40).expect("a session");
    let items: Vec<u32> = session.lines().iter().map(|l| l.item).collect();
    for item in items {
        session
            .decide(item, Choice::Decline("not now".to_string()))
            .expect("decided");
    }
    let response = session
        .finish(|_| Ok::<(), String>(()))
        .expect("a response");
    let outcomes = take(&rig, handle, T0 + 50, &response).expect("taken");
    assert!(
        outcomes.iter().all(|o| o.1 != OUTCOME_DELIVERED),
        "{outcomes:?}"
    );
    for o in &outcomes {
        words(&o.4);
    }
    assert_eq!(waiting(&rig, handle), vec![], "nothing was paired");
    let e = bundle(&rig, handle, &s.counter).expect_err("not ready");
    assert!(e.rule.starts_with("AS-"), "{e:?}");
    Rd::ok(&rig.call(2, handle, &[])).pump();
}

#[test]
fn as_60_a_response_for_another_session_is_refused() {
    let one = Rig::new();
    let other = Rig::new();
    let (h1, h2) = (one.start(), other.start());
    let offer = offer_for(NET);
    let (_, request, _) = begin(0x32, &offer, T0);
    let (_, request2, _) = begin(0x34, &offer, T0);
    let s1 = scan(&one, h1, &request).expect("scanned");
    let s2 = scan(&other, h2, &request2).expect("scanned");
    confirm(&one, h1, T0 + 20, s1.pairing, 1_000).expect("confirmed");
    confirm(&other, h2, T0 + 20, s2.pairing, 1_000).expect("confirmed");
    ready(&one, h1);
    ready(&other, h2);
    let r1 = build(&one, h1, T0 + 30);
    let r2 = build(&other, h2, T0 + 30);
    let (_, response2) = approve(&mut key_app(), &r2, T0 + 40);
    let e = take(&one, h1, T0 + 50, &response2).expect_err("another session");
    assert!(e.rule.starts_with("AS-"), "{e:?}");
    // Its own response still works, and the other one is not hurt.
    let (_, response1) = approve(&mut key_app(), &r1, T0 + 41);
    take(&one, h1, T0 + 60, &response1).expect("taken");
    Rd::ok(&one.call(2, h1, &[])).pump();
    Rd::ok(&other.call(2, h2, &[])).pump();
}
