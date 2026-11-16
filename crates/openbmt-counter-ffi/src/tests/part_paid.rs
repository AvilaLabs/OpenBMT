//! CR-285 and CR-286: a part paid sale leads with what is missing and its
//! code asks for it; what a wait carries is the node's own seconds.

use bitcoin::Address;

use super::kit::*;
use super::wire::{Rd, W};

const PART: u64 = 3_000;

fn judge(rig: &Rig, handle: u64, index: u32) -> super::wire::Screen {
    let response = rig.call(12, handle, &W::new().u64(T0).u32(index).0);
    let mut r = Rd::ok(&response);
    let screen = r.screen();
    r.done();
    screen
}

fn open_sale(rig: &Rig, handle: u64) -> super::wire::Opened {
    let response = rig.call(
        11,
        handle,
        &W::new().open(T0, FIVE_DOLLARS, "USD", RATE, "test-fixed").0,
    );
    let mut r = Rd::ok(&response);
    let opened = r.opened();
    r.done();
    opened
}

fn ready() -> (Rig, u64) {
    let rig = Rig::new();
    let handle = rig.start();
    rig.send(vec![peers_event()]);
    rig.call(10, handle, &W::new().u64(T0).u32(50).0);
    (rig, handle)
}

fn address() -> String {
    Address::from_script(&script_of(0), bitcoin::Network::Regtest)
        .expect("an address")
        .to_string()
}

fn pump(rig: &Rig, handle: u64) {
    rig.call(10, handle, &W::new().u64(T0).u32(50).0);
}

#[test]
fn cr_285_the_code_of_a_part_paid_sale_asks_for_what_is_missing() {
    let (rig, handle) = ready();
    let opened = open_sale(&rig, handle);
    assert_eq!(opened.needs_sats, 5_000);
    let screen = judge(&rig, handle, 0);
    assert_eq!((screen.missing, screen.missing_minor), (0, None));
    assert_eq!(
        screen.payment.as_ref().map(|p| p.copy_uri.clone()),
        Some(format!("bitcoin:{}?amount=0.00005", address()))
    );

    let part = spend(1, &[(&script_of(0), PART)]);
    rig.send(vec![sighting(&part, vec![peer(20)])]);
    pump(&rig, handle);
    let screen = judge(&rig, handle, 0);
    assert_eq!(screen.state, 1, "PartPaid");
    assert_eq!((screen.received, screen.needs), (PART, 5_000));
    assert_eq!(screen.missing, 2_000);
    // 2,000 of 5,000 sats of a sale of 500 minor units.
    assert_eq!(screen.missing_minor, Some(200));
    let payment = screen.payment.expect("a code");
    assert_eq!(
        payment.copy_uri,
        format!("bitcoin:{}?amount=0.00002", address())
    );
    assert_eq!(
        payment.uri,
        format!("BITCOIN:{}?amount=0.00002", address().to_ascii_uppercase())
    );
}

#[test]
fn cr_285_the_missing_amount_is_rounded_up_to_a_minor_unit() {
    let (rig, handle) = ready();
    open_sale(&rig, handle);
    // What arrived is valued at its quote in whole minor units (CR-06), so
    // the sale needs a little more than 5,000: the rest is not a round sum.
    let part = spend(1, &[(&script_of(0), 3_999)]);
    rig.send(vec![sighting(&part, vec![peer(20)])]);
    pump(&rig, handle);
    let screen = judge(&rig, handle, 0);
    assert_eq!(screen.missing, screen.needs - 3_999);
    assert_eq!((screen.needs, screen.missing), (5_009, 1_010));
    // 1,010 of 5,009 sats of 500 minor units is 100.8: up to 101.
    assert_eq!(screen.missing_minor, Some(101));
}

#[test]
fn cr_285_a_second_payment_of_exactly_the_missing_amount_turns_the_sale_paid() {
    let (rig, handle) = ready();
    open_sale(&rig, handle);
    let part = spend(1, &[(&script_of(0), PART)]);
    rig.send(vec![sighting(&part, vec![peer(20)])]);
    pump(&rig, handle);
    let screen = judge(&rig, handle, 0);
    assert_eq!(screen.state, 1);
    // The amount the code asks for is paid to the same address.
    assert_eq!(screen.missing, 2_000);
    let rest = spend(2, &[(&script_of(0), screen.missing)]);
    rig.send(vec![sighting(&rest, vec![peer(20)])]);
    pump(&rig, handle);
    let screen = judge(&rig, handle, 0);
    assert_ne!(screen.state, 1, "no longer part paid");
    assert_eq!(screen.received, 5_000);
    assert_eq!((screen.missing, screen.missing_minor), (0, None));
}

#[test]
fn cr_285_a_requote_moves_only_the_missing_part() {
    let (rig, handle) = ready();
    open_sale(&rig, handle);
    let part = spend(1, &[(&script_of(0), PART)]);
    rig.send(vec![sighting(&part, vec![peer(20)])]);
    pump(&rig, handle);
    // The price of bitcoin halves: what has arrived keeps its value (CR-08)
    // and the unpaid part is priced at the new quote.
    let response = rig.call(
        13,
        handle,
        &W::new()
            .u64(T0 + 1)
            .u32(0)
            .string("USD")
            .u64(RATE / 2)
            .string("test-fixed")
            .u64(T0 + 1)
            .0,
    );
    let mut r = Rd::ok(&response);
    let quote = r.opened();
    r.done();
    let screen = judge(&rig, handle, 0);
    assert_eq!(screen.state, 1, "PartPaid");
    assert_eq!(screen.received, PART, "what arrived is not touched");
    assert_eq!(screen.needs, quote.needs_sats);
    assert_eq!(screen.missing, quote.needs_sats - PART);
    assert_eq!(quote.needs_sats, PART + 4_000);
    let payment = screen.payment.expect("a code");
    assert_eq!(
        payment.copy_uri,
        format!("bitcoin:{}?amount=0.00004", address())
    );
}

#[test]
fn cr_286_a_check_that_waits_carries_its_seconds() {
    let (rig, handle) = ready();
    open_sale(&rig, handle);
    let pay = spend(1, &[(&script_of(0), 5_000)]);
    rig.send(vec![sighting(&pay, vec![peer(20)])]);
    pump(&rig, handle);
    let screen = judge(&rig, handle, 0);
    assert_eq!(screen.state, 2, "Waiting");
    assert!(screen.check_healthy);
    let seen = screen.seen_secs.or(screen.watch_secs_left);
    assert!(seen.is_some(), "a waiting payment says how long it waited");
    // The two are never given together.
    assert!(screen.seen_secs.is_none() || screen.watch_secs_left.is_none());
}
