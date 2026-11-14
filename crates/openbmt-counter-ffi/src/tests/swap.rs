//! CR-219 and CR-225 through `dispatch`: the requests for the swap provider
//! are handed out and answered by number, and a sale shows the small line,
//! the failure and the line of the status.

use openbmt_core::settings::{Settings, SignedSettings, SwapProvider};

use super::kit::*;
use super::wire::{Rd, Screen, W};
use crate::StartConfig;

const OP_SWAP_NEXT: u16 = 66;
const OP_SWAP_REPLY: u16 = 67;
const OP_SWAP_FAILED: u16 = 68;
const ONION: &str = "a4dqobyha4dqobyha4dqobyha4dqobyha4dqobyha4dqobyha4dwc6ad.onion";

fn settings_with(max_sale_sats: u64) -> SignedSettings {
    let mut s: Settings = signed_settings().settings;
    s.swap = Some(SwapProvider {
        name: "Pretend".to_string(),
        onion: ONION.to_string(),
        path_prefix: "/api".to_string(),
        max_sale_sats,
    });
    SignedSettings {
        owner_sig: s.sign(&id(1)),
        approver_sig: Some(s.sign(&id(2))),
        settings: s,
    }
}

fn config(rig: &Rig, max_sale_sats: u64) -> StartConfig {
    StartConfig {
        dir: rig.dir().to_str().expect("UTF-8").to_string(),
        now: T0,
        record: record_with(settings_with(max_sale_sats)).to_bytes(),
        proxy: None,
        own_node: None,
        lightning: None,
        link_proxy: None,
        shop_seed: None,
    }
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

fn judge(rig: &Rig, handle: u64, now: u64, index: u32) -> Screen {
    let response = rig.call(12, handle, &W::new().u64(now).u32(index).0);
    let mut r = Rd::ok(&response);
    let screen = r.screen();
    r.done();
    screen
}

/// The next request for the provider: its number, host and port, and bytes.
fn next(rig: &Rig, handle: u64, now: u64) -> Option<(u64, String, Vec<u8>)> {
    let response = rig.call(OP_SWAP_NEXT, handle, &W::new().u64(now).0);
    let mut r = Rd::ok(&response);
    let req = r.opt(|r| (r.u64(), r.string(), r.bytes()));
    r.done();
    req
}

#[test]
fn cr_219_a_sale_too_big_for_the_shops_limit_shows_the_small_line_and_asks_for_nothing() {
    let rig = Rig::new();
    let handle = rig.start_with(&config(&rig, 1));
    open(&rig, handle);
    assert!(next(&rig, handle, T0).is_none());
    let screen = judge(&rig, handle, T0, 0);
    assert!(screen.payment.is_some(), "the on-chain code is there");
    let ln = screen.lightning.expect("the line");
    assert!(ln.not_for_this_sale && !ln.invoice && !ln.waiting);
    assert!(ln.unavailable.is_none() && ln.failed.is_none());
}

#[test]
fn cr_225_the_request_is_plain_http_for_the_onion_and_an_unknown_number_is_dropped() {
    let rig = Rig::new();
    let handle = rig.start_with(&config(&rig, 1_000_000));
    open(&rig, handle);
    let (id, host_port, bytes) = next(&rig, handle, T0).expect("the pairs request");
    assert_eq!(host_port, format!("{ONION}:80"));
    let text = String::from_utf8(bytes).expect("ASCII");
    assert!(
        text.starts_with("GET /api/v2/swap/reverse HTTP/1.1\r\n"),
        "{text}"
    );
    assert!(text.contains(&format!("Host: {ONION}\r\n")));
    // One at a time: none until it is answered.
    assert!(next(&rig, handle, T0).is_none());
    // An answer to another number changes nothing.
    let other = W::new().u64(id + 100).bytes(b"HTTP/1.1 200 OK\r\n\r\n").0;
    Rd::ok(&rig.call(OP_SWAP_REPLY, handle, &other));
    assert!(next(&rig, handle, T0).is_none());
    // The news that there was no answer: the line says the provider is off
    // for now, and the sale is not changed.
    let failed = W::new().u64(id).0;
    Rd::ok(&rig.call(OP_SWAP_FAILED, handle, &failed));
    let screen = judge(&rig, handle, T0, 0);
    let ln = screen.lightning.expect("the line");
    let words = ln.unavailable.expect("why");
    assert_eq!(words.heading, "Lightning off for now");
    assert!(screen.payment.is_some());
}

#[test]
fn cr_219_with_no_provider_nothing_is_handed_out() {
    let rig = Rig::new();
    let handle = rig.start();
    open(&rig, handle);
    assert!(next(&rig, handle, T0).is_none());
    assert!(judge(&rig, handle, T0, 0).lightning.is_none());
}
