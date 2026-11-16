//! The node of the demo counter, started and driven through the framed
//! calls, on the pretend chain: a sale is opened, paid on the chain and seen
//! by the node.

use super::*;
use crate::{call, set_now};

const T0: u64 = 1_700_000_000;
/// The demo's pretend price, 73,554.18 USD per bitcoin, in cents.
const RATE: u64 = 7_355_418;

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

/// The fields of `start`: the folder, the time, the record and no options.
/// The folder is in the system's temporary place, for a build whose files are
/// on a disk, and is only a name for one whose files are in memory.
fn start_fields(record: &[u8]) -> Vec<u8> {
    let dir = std::env::temp_dir().join(format!("openbmt-demo-counter-{}", std::process::id()));
    let mut f = Vec::new();
    put_bytes(&mut f, dir.to_str().expect("UTF-8").as_bytes());
    f.extend_from_slice(&T0.to_le_bytes());
    put_bytes(&mut f, record);
    f.extend_from_slice(&[0, 0, 0, 0, 0]);
    f
}

/// The fields of `open` for `minor` cents of USD.
fn open_fields(minor: u64) -> Vec<u8> {
    let mut f = Vec::new();
    f.extend_from_slice(&T0.to_le_bytes());
    f.extend_from_slice(&minor.to_le_bytes());
    put_bytes(&mut f, b"USD");
    f.extend_from_slice(&RATE.to_le_bytes());
    put_bytes(&mut f, b"demo");
    f.extend_from_slice(&T0.to_le_bytes());
    f.push(0);
    f
}

fn ok(response: &[u8]) -> &[u8] {
    assert_eq!(response.get(..4), Some(b"OBMn".as_slice()));
    assert_eq!(response.get(5), Some(&0), "the call failed: {response:?}");
    response.get(6..).expect("a body")
}

/// The tests share one registry, and the till's request goes to the one node
/// that runs: one test at a time.

#[test]
fn a_sale_paid_on_the_pretend_chain_is_seen_by_the_node() {
    let _turn = crate::ONE_AT_A_TIME
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    set_now(T0);
    let record = crate::record::demo_record().expect("a record");
    let started = call(&frame(1, 0, &start_fields(&record)));
    let handle = u64::from_le_bytes(ok(&started).get(..8).expect("a handle").try_into().unwrap());

    // The node learns the chain is healthy from its first events.
    let mut pump = Vec::new();
    pump.extend_from_slice(&T0.to_le_bytes());
    pump.extend_from_slice(&0u32.to_le_bytes());
    ok(&call(&frame(10, handle, &pump)));

    let opened = call(&frame(11, handle, &open_fields(800)));
    let body = ok(&opened);
    let index = u32::from_le_bytes(body.get(..4).expect("an index").try_into().unwrap());
    let needs = u64::from_le_bytes(body.get(4..12).expect("sats").try_into().unwrap());
    assert_eq!(needs, 10_877);
    // index, needs, rate, from, until, then the address string.
    let len = u32::from_le_bytes(body.get(36..40).expect("a length").try_into().unwrap());
    let address = std::str::from_utf8(body.get(40..40 + len as usize).expect("an address"))
        .expect("UTF-8")
        .to_string();

    pay(&address, needs).expect("paid");
    ok(&call(&frame(10, handle, &pump)));
    let mut judge = Vec::new();
    judge.extend_from_slice(&T0.to_le_bytes());
    judge.extend_from_slice(&index.to_le_bytes());
    let seen = call(&frame(12, handle, &judge));
    let state = *ok(&seen).first().expect("a state");
    assert_ne!(state, 0, "the sale is still not paid yet");

    // The owner's heartbeats and a few seconds of watching: the sale is of
    // 8.00 USD, under the first instant limit, and the counter accepts it
    // before a block (state 3, `PaidInstant`).
    let mut instant = false;
    for t in 1..=15u64 {
        set_now(T0 + t);
        let mut beat = Vec::new();
        beat.extend_from_slice(&(T0 + t).to_le_bytes());
        put_bytes(&mut beat, &heartbeat(T0 + t).expect("a heartbeat"));
        ok(&call(&frame(20, handle, &beat)));
        let mut pump = Vec::new();
        pump.extend_from_slice(&(T0 + t).to_le_bytes());
        pump.extend_from_slice(&0u32.to_le_bytes());
        ok(&call(&frame(10, handle, &pump)));
        let mut judge = Vec::new();
        judge.extend_from_slice(&(T0 + t).to_le_bytes());
        judge.extend_from_slice(&index.to_le_bytes());
        instant |= ok(&call(&frame(12, handle, &judge))).first() == Some(&3);
    }
    assert!(instant, "the sale of 8.00 USD is accepted at once");

    assert_eq!(mine().expect("mined"), 1);
    ok(&call(&frame(10, handle, &pump)));
    let after = call(&frame(12, handle, &judge));
    assert_ne!(ok(&after).first().copied(), Some(0));

    ok(&call(&frame(2, handle, &[])));
    let _ = std::fs::remove_dir_all(
        std::env::temp_dir().join(format!("openbmt-demo-counter-{}", std::process::id())),
    );
}

/// CR-266 to CR-269: the Box tells the counter that the till asks for a sale of
/// 8.00 USD; the card waits, Start opens the sale for the till's own sale id,
/// and the sale is paid on the pretend chain.
#[test]
fn a_request_from_the_till_waits_starts_and_is_paid() {
    let _one = crate::ONE_AT_A_TIME
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    set_now(T0);
    let record = crate::record::demo_record_with_box().expect("a record");
    let started = call(&frame(1, 0, &start_fields(&record)));
    let handle = u64::from_le_bytes(ok(&started).get(..8).expect("a handle").try_into().unwrap());
    let mut pump = Vec::new();
    pump.extend_from_slice(&T0.to_le_bytes());
    pump.extend_from_slice(&0u32.to_le_bytes());
    ok(&call(&frame(10, handle, &pump)));

    assert_eq!(crate::till::request(800), Ok(crate::till::TILL_SALE));
    let waiting = call(&frame(85, handle, &T0.to_le_bytes()));
    let rows = ok(&waiting);
    assert_eq!(
        u32::from_le_bytes(rows.get(..4).expect("a count").try_into().unwrap()),
        1
    );
    assert!(rows.windows(16).any(|w| w == crate::till::TILL_SALE));

    let mut start = Vec::new();
    start.extend_from_slice(&T0.to_le_bytes());
    put_bytes(&mut start, &crate::till::TILL_SALE);
    start.extend_from_slice(&RATE.to_le_bytes());
    put_bytes(&mut start, b"demo");
    start.extend_from_slice(&T0.to_le_bytes());
    start.push(0);
    let opened = call(&frame(86, handle, &start));
    let body = ok(&opened);
    let index = u32::from_le_bytes(body.get(..4).expect("an index").try_into().unwrap());
    let needs = u64::from_le_bytes(body.get(4..12).expect("sats").try_into().unwrap());
    assert_eq!(needs, 10_877);
    let len = u32::from_le_bytes(body.get(36..40).expect("a length").try_into().unwrap());
    let address = std::str::from_utf8(body.get(40..40 + len as usize).expect("an address"))
        .expect("UTF-8")
        .to_string();

    // The request left the list, and the sale names the till's sale.
    let waiting = call(&frame(85, handle, &T0.to_le_bytes()));
    assert_eq!(ok(&waiting).get(..4), Some(&0u32.to_le_bytes()[..]));
    let of_sale = call(&frame(88, handle, &index.to_le_bytes()));
    assert!(ok(&of_sale)
        .windows(16)
        .any(|w| w == crate::till::TILL_SALE));

    pay(&address, needs).expect("paid");
    ok(&call(&frame(10, handle, &pump)));
    let mut judge = Vec::new();
    judge.extend_from_slice(&T0.to_le_bytes());
    judge.extend_from_slice(&index.to_le_bytes());
    let seen = call(&frame(12, handle, &judge));
    assert_ne!(*ok(&seen).first().expect("a state"), 0, "the sale is paid");

    ok(&call(&frame(2, handle, &[])));
    let _ = std::fs::remove_dir_all(
        std::env::temp_dir().join(format!("openbmt-demo-counter-{}", std::process::id())),
    );
}

#[test]
fn a_counter_with_no_box_in_its_settings_drops_the_request() {
    let _one = crate::ONE_AT_A_TIME
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    set_now(T0);
    let record = crate::record::demo_record().expect("a record");
    let started = call(&frame(1, 0, &start_fields(&record)));
    let handle = u64::from_le_bytes(ok(&started).get(..8).expect("a handle").try_into().unwrap());
    assert!(crate::till::request(800).is_err());
    ok(&call(&frame(2, handle, &[])));
}

/// The state byte of the sale `index` at `at`, after the counter has read what the chain told it.
fn state_at(handle: u64, index: u32, at: u64) -> u8 {
    set_now(at);
    let mut beat = Vec::new();
    beat.extend_from_slice(&at.to_le_bytes());
    put_bytes(&mut beat, &heartbeat(at).expect("a heartbeat"));
    ok(&call(&frame(20, handle, &beat)));
    let mut pump = Vec::new();
    pump.extend_from_slice(&at.to_le_bytes());
    pump.extend_from_slice(&0u32.to_le_bytes());
    ok(&call(&frame(10, handle, &pump)));
    let mut judge = Vec::new();
    judge.extend_from_slice(&at.to_le_bytes());
    judge.extend_from_slice(&index.to_le_bytes());
    *ok(&call(&frame(12, handle, &judge)))
        .first()
        .expect("a state")
}

/// A customer who pays and then spends the same coin to an address of their own
/// within the counter's short check (about eight seconds of its clock): the counter's state is the red
/// do-not-hand-over state (7), and never a green one.
#[test]
fn a_payment_replaced_within_the_short_check_turns_red() {
    let _one = crate::ONE_AT_A_TIME
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    set_now(T0);
    let record = crate::record::demo_record().expect("a record");
    let started = call(&frame(1, 0, &start_fields(&record)));
    let handle = u64::from_le_bytes(ok(&started).get(..8).expect("a handle").try_into().unwrap());
    let mut pump = Vec::new();
    pump.extend_from_slice(&T0.to_le_bytes());
    pump.extend_from_slice(&0u32.to_le_bytes());
    ok(&call(&frame(10, handle, &pump)));
    let opened = call(&frame(11, handle, &open_fields(800)));
    let body = ok(&opened);
    let index = u32::from_le_bytes(body.get(..4).expect("an index").try_into().unwrap());
    let needs = u64::from_le_bytes(body.get(4..12).expect("sats").try_into().unwrap());
    let len = u32::from_le_bytes(body.get(36..40).expect("a length").try_into().unwrap());
    let address = std::str::from_utf8(body.get(40..40 + len as usize).expect("an address"))
        .expect("UTF-8")
        .to_string();

    let txid = pay(&address, needs).expect("paid");
    let mut states = Vec::new();
    for t in 1..=3u64 {
        states.push(state_at(handle, index, T0 + t));
    }
    assert!(states.iter().all(|s| *s != 3), "still short of green");
    let other = replace(&txid, needs).expect("replaced");
    assert_ne!(other, txid);
    assert_eq!(world().as_ref().expect("a chain").chain.mempool_len(), 1);
    assert!(replace(&txid, needs).is_err(), "the first payment is gone");
    let mut after = Vec::new();
    for t in 4..=20u64 {
        after.push(state_at(handle, index, T0 + t));
    }
    assert_eq!(after.last(), Some(&7), "the counter turns red");
    assert!(after.iter().all(|s| *s != 3), "green never shows");

    ok(&call(&frame(2, handle, &[])));
    let _ = std::fs::remove_dir_all(
        std::env::temp_dir().join(format!("openbmt-demo-counter-{}", std::process::id())),
    );
}
