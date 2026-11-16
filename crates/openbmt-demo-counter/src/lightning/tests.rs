//! The pretend Lightning node, answering the counter's own driver through the
//! framed calls: the sale's code carries an invoice the counter checked, the
//! page pays it, and the counter's judgment turns the sale paid over
//! Lightning with no block mined.

use super::*;
use crate::{call, set_now, world};
use std::sync::atomic::{AtomicU64, Ordering};

const FIRST: u64 = 1_700_000_000;
/// The clock only goes forward and the tests share it: each starts a thousand
/// seconds after the last time given.
static BASE: AtomicU64 = AtomicU64::new(FIRST);

fn t0() -> u64 {
    BASE.load(Ordering::SeqCst)
}
/// The demo's pretend price, 73,554.18 USD per bitcoin, in cents.
const RATE: u64 = 7_355_418;
/// `State::PaidLightning` on the wire (CR-109).
const PAID_LIGHTNING: u8 = 11;

fn frame(op: u16, handle: u64, fields: &[u8]) -> Vec<u8> {
    let mut out = b"OBMm".to_vec();
    out.push(1);
    out.extend_from_slice(&op.to_le_bytes());
    out.extend_from_slice(&handle.to_le_bytes());
    out.extend_from_slice(fields);
    out
}

fn put_bytes(out: &mut Vec<u8>, v: &[u8]) {
    out.extend_from_slice(&(v.len() as u32).to_le_bytes());
    out.extend_from_slice(v);
}

fn ok(response: &[u8]) -> &[u8] {
    assert_eq!(response.get(..4), Some(b"OBMn".as_slice()));
    assert_eq!(response.get(5), Some(&0), "the call failed: {response:?}");
    response.get(6..).expect("a body")
}

/// The folder of this start. The node's files are kept in memory by folder
/// name for the whole run, so each start has a folder of its own: a sale of an
/// earlier test is not restored.
fn folder() -> std::path::PathBuf {
    std::env::temp_dir().join(format!("openbmt-demo-ln-{}-{}", std::process::id(), t0()))
}

/// The fields of `start` with the saved Lightning connection.
fn start_fields(record: &[u8], lightning: &[u8]) -> Vec<u8> {
    let dir = folder();
    let mut f = Vec::new();
    put_bytes(&mut f, dir.to_str().expect("UTF-8").as_bytes());
    f.extend_from_slice(&t0().to_le_bytes());
    put_bytes(&mut f, record);
    // The proxy and the own node: none. The Lightning record: given.
    f.extend_from_slice(&[0, 0, 1]);
    put_bytes(&mut f, lightning);
    // The link's proxy and Shop Lightning's seed: none.
    f.extend_from_slice(&[0, 0]);
    f
}

fn open_fields(minor: u64) -> Vec<u8> {
    let mut f = Vec::new();
    f.extend_from_slice(&t0().to_le_bytes());
    f.extend_from_slice(&minor.to_le_bytes());
    put_bytes(&mut f, b"USD");
    f.extend_from_slice(&RATE.to_le_bytes());
    put_bytes(&mut f, b"demo");
    f.extend_from_slice(&t0().to_le_bytes());
    f.push(0);
    f
}

fn time_fields(now: u64) -> Vec<u8> {
    let mut f = now.to_le_bytes().to_vec();
    f.extend_from_slice(&0u32.to_le_bytes());
    f
}

fn judge_fields(now: u64, index: u32) -> Vec<u8> {
    let mut f = now.to_le_bytes().to_vec();
    f.extend_from_slice(&index.to_le_bytes());
    f
}

/// A string of the wire at `at`: its length, then its bytes.
fn string(body: &[u8], at: &mut usize) -> String {
    let len = u32::from_le_bytes(body[*at..*at + 4].try_into().unwrap()) as usize;
    *at += 4;
    let out = std::str::from_utf8(&body[*at..*at + len])
        .unwrap()
        .to_string();
    *at += len;
    out
}

/// What `ln_next` handed out: the request number, the method, the path and
/// the body.
fn ln_next(handle: u64, now: u64) -> Option<(u64, String, String, String)> {
    let response = call(&frame(54, handle, &now.to_le_bytes()));
    let body = ok(&response);
    if body.first() == Some(&0) {
        return None;
    }
    let id = u64::from_le_bytes(body[1..9].try_into().unwrap());
    let mut at = 9;
    let _host_port = string(body, &mut at);
    let method = if body[at] == 1 { "POST" } else { "GET" };
    at += 1;
    let path = string(body, &mut at);
    let _header_name = string(body, &mut at);
    let _header_value = string(body, &mut at);
    let text = if body[at] == 1 {
        at += 1;
        string(body, &mut at)
    } else {
        String::new()
    };
    Some((id, method.to_string(), path, text))
}

/// The page's loop: every request the counter has, answered by the node.
fn serve(handle: u64, now: u64) -> usize {
    set_now(now);
    let mut served = 0;
    while let Some((id, method, path, body)) = ln_next(handle, now) {
        let (status, reply) = answer(&method, &path, &body, now);
        let mut f = id.to_le_bytes().to_vec();
        f.extend_from_slice(&u32::from(status).to_le_bytes());
        put_bytes(&mut f, &reply);
        ok(&call(&frame(51, handle, &f)));
        served += 1;
        assert!(served < 10, "the driver keeps asking");
    }
    served
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

fn state(handle: u64, now: u64, index: u32) -> u8 {
    *ok(&call(&frame(12, handle, &judge_fields(now, index))))
        .first()
        .expect("a state")
}

fn start() -> u64 {
    BASE.store(crate::now().max(FIRST) + 1_000, Ordering::SeqCst);
    set_now(t0());
    let record = crate::record::demo_record().expect("a record");
    let lightning = super::record().expect("a Lightning record");
    let started = call(&frame(1, 0, &start_fields(&record, &lightning)));
    let handle = u64::from_le_bytes(ok(&started)[..8].try_into().unwrap());
    ok(&call(&frame(10, handle, &time_fields(t0()))));
    handle
}

fn stop(handle: u64) {
    ok(&call(&frame(2, handle, &[])));
    let _ = std::fs::remove_dir_all(folder());
}

/// Opens a sale of `minor` cents and returns its index, the sats it needs and
/// its address.
fn open(handle: u64, minor: u64) -> (u32, u64, String) {
    let opened = call(&frame(11, handle, &open_fields(minor)));
    let body = ok(&opened);
    // The index, the sats, the rate, the quote's two times, then the address.
    let mut at = 36;
    (
        u32::from_le_bytes(body[..4].try_into().unwrap()),
        u64::from_le_bytes(body[4..12].try_into().unwrap()),
        string(body, &mut at),
    )
}

#[test]
fn a_sale_paid_over_the_pretend_node_is_judged_by_the_counter() {
    let _turn = crate::ONE_AT_A_TIME
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let handle = start();
    let (index, needs, _) = open(handle, 800);
    assert_eq!(needs, 10_877);

    // The counter asks the node for an invoice; the invoice passes LN-17 and
    // the sale's code carries it beside the address.
    set_now(t0() + 1);
    assert!(serve(handle, t0() + 1) >= 1);
    let screen = call(&frame(12, handle, &judge_fields(t0() + 1, index)));
    let invoice = node()[0].text.clone();
    assert!(
        invoice.starts_with("lnbcrt"),
        "a regtest invoice: {invoice}"
    );
    let with_invoice = format!("lightning={invoice}");
    assert!(
        contains(&screen, with_invoice.as_bytes()),
        "the sale's code carries the invoice"
    );
    assert!(contains(&screen, b"bitcoin:"), "and the address");
    assert_eq!(node()[0].msat, 10_877_000);
    assert_ne!(state(handle, t0() + 1, index), PAID_LIGHTNING);

    // The customer's wallet pays the invoice; the counter's next lookup finds
    // it settled, and its own judgment turns the sale paid. No block is mined.
    assert_eq!(pay(&invoice, t0() + 2), Ok(10_877));
    set_now(t0() + 4);
    serve(handle, t0() + 4);
    ok(&call(&frame(10, handle, &time_fields(t0() + 4))));
    assert_eq!(state(handle, t0() + 4, index), PAID_LIGHTNING);

    // The invoice is paid once.
    assert!(pay(&invoice, t0() + 5).is_err());
    stop(handle);
}

#[test]
fn the_on_chain_path_still_works_on_a_second_sale() {
    let _turn = crate::ONE_AT_A_TIME
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let handle = start();
    let (first, _, _) = open(handle, 800);
    serve(handle, t0() + 1);
    let invoice = node()[0].text.clone();
    assert_eq!(pay(&invoice, t0() + 2), Ok(10_877));
    serve(handle, t0() + 4);
    ok(&call(&frame(10, handle, &time_fields(t0() + 4))));
    assert_eq!(state(handle, t0() + 4, first), PAID_LIGHTNING);

    // The second sale is paid to its address, on the pretend chain.
    let (second, needs, address) = open(handle, 800);
    assert_ne!(first, second);
    set_now(t0() + 5);
    serve(handle, t0() + 5);
    world::pay(&address, needs).expect("paid");
    let mut paid = false;
    for t in 6..=25u64 {
        set_now(t0() + t);
        let mut beat = (t0() + t).to_le_bytes().to_vec();
        put_bytes(&mut beat, &world::heartbeat(t0() + t).expect("a heartbeat"));
        ok(&call(&frame(20, handle, &beat)));
        serve(handle, t0() + t);
        ok(&call(&frame(10, handle, &time_fields(t0() + t))));
        paid |= state(handle, t0() + t, second) == 3;
    }
    assert!(paid, "the second sale is accepted on-chain at once");
    stop(handle);
}

#[test]
fn the_node_pays_only_its_own_invoices_and_answers_unknown_ones_not_found() {
    let _turn = crate::ONE_AT_A_TIME
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    reset();
    assert!(pay("lnbcrt1nothing", t0()).is_err());
    let (status, _) = answer("GET", &format!("/v1/invoice/{}", "00".repeat(32)), "", t0());
    assert_eq!(status, 404);
    let (status, _) = answer("GET", "/v1/getinfo", "", t0());
    assert_eq!(status, 404);
}
