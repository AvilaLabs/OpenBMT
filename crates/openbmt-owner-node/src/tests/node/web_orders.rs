//! The owner phone's visits to a store (`docs/WEB-ORDER-RULES.md`, WO-05 to
//! WO-32), against a store the test plays by the plugin's rules: its HMAC
//! check, its pool of addresses and its answers.

use std::collections::BTreeSet;

use openbmt_core::currency::ShopCurrency;
use openbmt_owner::own_book::AllocationBook;
use openbmt_owner::starter::{KernelRandom, KERNEL_BYTES};
use openbmt_owner::web_order::{
    pairing_id_of, Notice, NoticeState, PairingCode, Range, SignedRequest, HEADER_MAC,
    HEADER_NONCE, HEADER_TIME, ROUTE_NOTICE, ROUTE_ORDERS, ROUTE_RANGE,
};
use openbmt_requests::Status;

use super::live::{pay_to_script, START};
use super::own::{own_receive, own_setup};
use super::*;
use crate::requests::{RequestContext, RequestsOff};
use crate::web_orders::{
    check_store_url, decode, encode, web_key_seed, WebError, MAGIC_WEB_ORDERS,
};
use crate::web_visit::{WebCall, WebCallKind, WebFailure, WebStep};
use crate::{RefusedWhy, WEB_ORDERS_FILE};

const STORE: &str = "https://shop.example/store";

/// A kernel that gives different bytes each time.
struct Bytes(u8);

impl KernelRandom for Bytes {
    fn read32(&mut self) -> Option<[u8; KERNEL_BYTES]> {
        self.0 = self.0.wrapping_add(1);
        let mut out = [0u8; KERNEL_BYTES];
        for (i, b) in out.iter_mut().enumerate() {
            *b = self
                .0
                .wrapping_mul(31)
                .wrapping_add(u8::try_from(i).unwrap() * 7 + 1);
        }
        Some(out)
    }
}

fn context(depth: u8) -> RequestContext {
    RequestContext {
        currency: ShopCurrency::Usd,
        min_sale_sats: 0,
        min_confirmations: depth,
        tz_offset_secs: 0,
    }
}

fn desk_rig(dir: &Dir, depth: u8) -> (Rig, AllocationBook, Setup) {
    let (setup, _time) = own_setup(10_000);
    let mut rig = start(dir, &setup).unwrap();
    let mut book = AllocationBook::new();
    let desk = rig.node.owner().owner_id();
    rig.give(&mut book, desk, START);
    rig.node.set_request_context(context(depth), None);
    (rig, book, setup)
}

fn restart(rig: Rig, dir: &Dir, book: AllocationBook, setup: &Setup) -> Rig {
    let mut rig = restart_bare(rig, dir, book, setup);
    rig.node.set_request_context(context(1), None);
    rig
}

/// A restart that has not been given the shop's settings yet.
fn restart_bare(rig: Rig, dir: &Dir, book: AllocationBook, setup: &Setup) -> Rig {
    let chain = rig.chain.clone();
    let mut rig = rig;
    rig.node.shutdown();
    drop(rig);
    let mut setup = setup.clone();
    setup.own = Some(book);
    start_at(dir, &setup, chain).unwrap()
}

fn unhex(text: &str) -> Vec<u8> {
    (0..text.len() / 2)
        .map(|i| u8::from_str_radix(&text[2 * i..2 * i + 2], 16).unwrap())
        .collect()
}

/// A store as the plugin plays it: the pairing it was given, the pool of
/// addresses, the orders, and what it was told.
struct Store {
    public: [u8; 32],
    secret: [u8; 32],
    next_index: u32,
    pool: Vec<String>,
    orders: Vec<(u64, String, u64)>,
    notices: Vec<Notice>,
    hits: Vec<WebCallKind>,
    /// Answer 403 to everything.
    denied: bool,
}

impl Store {
    fn paired(code: &str) -> Self {
        let code = PairingCode::from_text(code).unwrap();
        Self {
            public: *code.public(),
            secret: *code.secret(),
            next_index: 0,
            pool: Vec::new(),
            orders: Vec::new(),
            notices: Vec::new(),
            hits: Vec::new(),
            denied: false,
        }
    }

    /// The customer picks Bitcoin: the next address of the pool.
    fn order(&mut self, number: u64, sats: u64) -> String {
        let address = self.pool.remove(0);
        self.orders.push((number, address.clone(), sats));
        address
    }

    fn handle(&mut self, call: &WebCall) -> (u16, Vec<u8>) {
        self.hits.push(call.kind);
        if self.denied {
            return (403, Vec::new());
        }
        let route = call
            .url
            .strip_prefix("https://shop.example/store/wp-json")
            .unwrap();
        let header = |name: &str| {
            call.headers
                .iter()
                .find(|(n, _)| n == name)
                .map(|(_, v)| v.clone())
                .unwrap()
        };
        let nonce: [u8; 16] = unhex(&header(HEADER_NONCE)).try_into().unwrap();
        let time: u64 = header(HEADER_TIME).parse().unwrap();
        let mac =
            SignedRequest::make(&self.secret, "POST", route, time, nonce, &call.body).unwrap();
        assert_eq!(
            mac.mac_text(),
            header(HEADER_MAC),
            "the HMAC is the plugin's"
        );
        match call.kind {
            WebCallKind::Orders => {
                assert_eq!(route, ROUTE_ORDERS);
                let text = String::from_utf8(call.body.clone()).unwrap();
                let since: u64 = text
                    .strip_prefix("{\"since\":")
                    .and_then(|t| t.strip_suffix('}'))
                    .unwrap()
                    .parse()
                    .unwrap();
                let list: Vec<String> = self
                    .orders
                    .iter()
                    .filter(|(n, _, _)| *n > since)
                    .map(|(n, a, s)| {
                        format!(
                            "{{\"order\":{n},\"address\":\"{a}\",\"sats\":{s},\"quoted_at\":{START},\"total_minor\":2500,\"currency\":\"USD\"}}"
                        )
                    })
                    .collect();
                (
                    200,
                    format!(
                        "{{\"orders\":[{}],\"left\":{}}}",
                        list.join(","),
                        self.pool.len()
                    )
                    .into_bytes(),
                )
            }
            WebCallKind::Range => {
                assert_eq!(route, ROUTE_RANGE);
                let bytes = unhex(std::str::from_utf8(&call.body).unwrap());
                let id = pairing_id_of(&self.secret);
                let range = Range::open(&bytes, &self.public, &id).unwrap();
                let result = if range.start == self.next_index {
                    self.next_index += u32::try_from(range.addresses.len()).unwrap();
                    self.pool.extend(range.addresses);
                    "accepted"
                } else {
                    "gap"
                };
                let status = if result == "accepted" { 200 } else { 409 };
                (
                    status,
                    format!("{{\"result\":\"{result}\",\"left\":{}}}", self.pool.len())
                        .into_bytes(),
                )
            }
            WebCallKind::Notice => {
                assert_eq!(route, ROUTE_NOTICE);
                let bytes = unhex(std::str::from_utf8(&call.body).unwrap());
                let notice = Notice::open(&bytes, &self.public).unwrap();
                self.notices.push(notice);
                (200, b"{\"action\":\"complete\"}".to_vec())
            }
        }
    }
}

/// One visit: the node's steps, with the desk's next unused indexes, until it
/// is idle. Returns the kinds of call it made.
fn visit(rig: &mut Rig, store: &mut Store, now: u64, force: bool) -> Vec<WebCallKind> {
    let mut rng = Bytes(100);
    let mut kinds = Vec::new();
    for _ in 0..40 {
        match rig.node.web_step(now, force, &mut rng).unwrap() {
            WebStep::Idle => return kinds,
            WebStep::Busy => panic!("a call is in the air"),
            WebStep::NeedIndexes(n) => {
                let taken = rig.node.request_indexes();
                let indexes: Vec<u32> = (0..).filter(|i| !taken.contains(i)).take(n).collect();
                rig.node.web_give_indexes(&indexes).unwrap();
            }
            WebStep::Call(call) => {
                kinds.push(call.kind);
                let (status, body) = store.handle(&call);
                rig.node.web_reply(status, &body, now).unwrap();
            }
        }
    }
    panic!("the visit did not end");
}

/// A node paired with a store that holds the first range.
fn paired(dir: &Dir, depth: u8) -> (Rig, AllocationBook, Setup, Store) {
    let (mut rig, book, setup) = desk_rig(dir, depth);
    let shown = rig
        .node
        .web_pair_start(
            "https://Shop.Example/store/",
            "Corner Shop",
            START,
            &mut Bytes(1),
        )
        .unwrap();
    rig.node.web_pair_confirm().unwrap();
    let mut store = Store::paired(&shown.code);
    let kinds = visit(&mut rig, &mut store, START, false);
    assert_eq!(kinds, [WebCallKind::Orders, WebCallKind::Range]);
    assert_eq!(store.pool.len(), 100);
    (rig, book, setup, store)
}

#[test]
fn wo_05_a_pairing_shows_a_code_the_plugin_reads_and_waits_for_the_owner() {
    let dir = Dir::new();
    let (mut rig, _book, _setup) = desk_rig(&dir, 1);
    let shown = rig
        .node
        .web_pair_start(STORE, "Corner Shop", START, &mut Bytes(1))
        .unwrap();
    let code = PairingCode::from_text(&shown.code).unwrap();
    assert_eq!(code.shop_name(), "Corner Shop");
    assert_eq!(code.fingerprint(), shown.fingerprint);
    assert_eq!(shown.url, STORE);
    // Until the owner says the fingerprints match, nothing is visited.
    let mut store = Store::paired(&shown.code);
    assert!(visit(&mut rig, &mut store, START, true).is_empty());
    let info = rig.node.web_state();
    assert_eq!(info.pairing, Some(crate::PairState::Waiting));
    assert_eq!(info.code.as_deref(), Some(shown.code.as_str()));
    assert_eq!(info.fingerprint, shown.fingerprint);
    rig.node.web_pair_confirm().unwrap();
    assert_eq!(rig.node.web_state().pairing, Some(crate::PairState::Paired));
    assert_eq!(rig.node.web_state().code, None);
    // A confirmed pairing is not replaced by a new code.
    assert_eq!(
        rig.node
            .web_pair_start(STORE, "Corner Shop", START, &mut Bytes(9))
            .unwrap_err(),
        WebError::AlreadyPaired
    );
}

#[test]
fn wo_06_a_store_without_https_or_with_a_bent_address_is_refused() {
    for good in [
        "https://shop.example",
        "https://shop.example/",
        "HTTPS://Shop.Example:8443/blog/",
        "https://127.0.0.1",
    ] {
        assert!(check_store_url(good).is_ok(), "{good}");
    }
    assert_eq!(
        check_store_url("HTTPS://Shop.Example:8443/blog/").unwrap(),
        "https://shop.example:8443/blog"
    );
    for bad in [
        "http://shop.example",
        "shop.example",
        "https://",
        "https://user@shop.example",
        "https://shop.example?x=1",
        "https://shop.example/#top",
        "https://shop.example:0",
        "https://shop.example:99999",
        "https://shop..example",
        "https://-shop.example",
        "https://shop.example/a b",
        "https://[::1]",
        "ftp://shop.example",
    ] {
        assert_eq!(check_store_url(bad), Err(WebError::Url), "{bad}");
    }
    let dir = Dir::new();
    let (mut rig, _book, _setup) = desk_rig(&dir, 1);
    assert_eq!(
        rig.node
            .web_pair_start("http://shop.example", "Corner Shop", START, &mut Bytes(1))
            .unwrap_err(),
        WebError::Url
    );
    assert!(!dir.path().join(WEB_ORDERS_FILE).exists());
}

#[test]
fn rq_01_a_shop_with_a_key_app_has_no_web_orders() {
    let dir = Dir::new();
    let mut rig = start(&dir, &Setup::default()).unwrap();
    let refused = rig
        .node
        .web_pair_start(STORE, "Corner Shop", START, &mut Bytes(1))
        .unwrap_err();
    assert_eq!(refused, WebError::Off(RequestsOff::KeyApp));
    assert!(rig.node.web_state().off.is_some());
    assert!(!dir.path().join(WEB_ORDERS_FILE).exists());
}

#[test]
fn wo_10_wo_12_the_first_visit_sends_a_range_that_is_stored_and_watched_before() {
    let dir = Dir::new();
    let (rig, _book, _setup, store) = paired(&dir, 1);
    // The range holds the desk's first 100 indexes, sent from 0 (WO-10).
    assert_eq!(store.next_index, 100);
    let kept = rig.node.request_indexes();
    assert_eq!(kept, (0..100).collect::<BTreeSet<u32>>());
    assert_eq!(rig.node.web_indexes(), kept);
    // The file holds it, and the addresses are the wallet's own.
    let state = decode(&std::fs::read(dir.path().join(WEB_ORDERS_FILE)).unwrap()).unwrap();
    assert_eq!(state.blocks.len(), 1);
    assert!(state.blocks[0].delivered);
    assert_eq!(state.next_seq, 100);
    let first = bitcoin::Address::from_script(&own_receive(0), bitcoin::Network::Bitcoin)
        .unwrap()
        .to_string();
    assert_eq!(store.pool.first(), Some(&first));
    let info = rig.node.web_state();
    assert_eq!((info.addresses_sent, info.left), (100, Some(100)));
    assert_eq!(info.last_visit, Some(START));
    // A request made by hand never takes one of them.
    let taken = rig.node.request_indexes();
    assert!(taken.contains(&0) && taken.contains(&99) && !taken.contains(&100));
}

#[test]
fn wo_11_a_range_is_sent_at_the_visit_after_the_store_has_fewer_than_20_left() {
    let dir = Dir::new();
    let (mut rig, _book, _setup, mut store) = paired(&dir, 1);
    for n in 1..=80 {
        store.order(n, 10_000);
    }
    // 20 are left: not yet low.
    rig.node.web_visit_soon();
    let kinds = visit(&mut rig, &mut store, START + 10, false);
    assert_eq!(kinds, [WebCallKind::Orders]);
    store.order(81, 10_000);
    rig.node.web_visit_soon();
    let kinds = visit(&mut rig, &mut store, START + 20, false);
    assert_eq!(kinds, [WebCallKind::Orders, WebCallKind::Range]);
    assert_eq!((store.next_index, store.pool.len()), (200, 119));
    // Each address was given once, in order.
    let all: BTreeSet<&String> = store
        .pool
        .iter()
        .chain(store.orders.iter().map(|(_, a, _)| a).collect::<Vec<_>>())
        .collect();
    assert_eq!(all.len(), 200);
    assert_eq!(rig.node.web_indexes().len(), 200);
}

#[test]
fn wo_20_wo_22_an_order_becomes_a_request_at_its_address_and_bad_ones_are_shown() {
    let dir = Dir::new();
    let (mut rig, _book, _setup, mut store) = paired(&dir, 1);
    let address = store.order(7, 50_000);
    // An order for an address the owner phone never sent, and one that uses
    // an address of the first order again.
    store.orders.push((
        8,
        "bc1qw508d6qejxtdg4y5r3zarvary0c5xw7kv8f3t4".to_string(),
        1_000,
    ));
    store.orders.push((9, address.clone(), 1_000));
    store.orders.push((10, store.pool[0].clone(), 0));
    rig.node.web_visit_soon();
    visit(&mut rig, &mut store, START + 5, false);
    let book = rig.node.requests().unwrap();
    assert_eq!(book.requests().len(), 1);
    let request = book.request(1).unwrap();
    assert_eq!(request.amount, 50_000);
    assert_eq!(request.address_index, 0);
    assert_eq!(request.note, "Web order 7");
    let info = rig.node.web_state();
    assert_eq!(info.orders, 1);
    let why: Vec<(u64, RefusedWhy)> = info.refused.iter().map(|r| (r.order, r.why)).collect();
    assert_eq!(
        why,
        [
            (8, RefusedWhy::AddressNotSent),
            (9, RefusedWhy::AddressUsedBefore),
            (10, RefusedWhy::NoAmount),
        ]
    );
    assert!(info.refused[0]
        .why
        .words()
        .heading
        .starts_with("WORDS_NEEDED_WO_22_"));
    // The same orders asked for again make nothing more.
    rig.node.web_visit_soon();
    visit(&mut rig, &mut store, START + 6, false);
    assert_eq!(rig.node.requests().unwrap().requests().len(), 1);
    assert_eq!(rig.node.web_state().refused.len(), 3);
}

/// The transaction of a payment to the address of desk index `index`.
fn pay(index: u32, from: u8, sats: u64) -> Transaction {
    pay_to_script(&own_receive(index), from, sats)
}

#[test]
fn wo_30_wo_18_a_notice_is_signed_only_once_the_payment_is_at_the_depth() {
    let dir = Dir::new();
    let (mut rig, _book, _setup, mut store) = paired(&dir, 3);
    store.order(7, 50_000);
    rig.node.web_visit_soon();
    visit(&mut rig, &mut store, START + 5, false);
    let tx = pay(0, 5, 50_000);
    rig.mine(std::slice::from_ref(&tx));
    // One confirmation of three: nothing is counted and nothing is sent.
    assert_eq!(
        rig.node.requests().unwrap().request(1).unwrap().status(0),
        Status::Waiting
    );
    rig.node.web_visit_soon();
    let kinds = visit(&mut rig, &mut store, START + 6, false);
    assert_eq!(kinds, [WebCallKind::Orders]);
    assert!(store.notices.is_empty());
    rig.mine_empty(2);
    assert_eq!(
        rig.node.requests().unwrap().request(1).unwrap().status(0),
        Status::Paid
    );
    // The pump asks for the visit by itself, and the notice goes in it.
    let kinds = visit(&mut rig, &mut store, START + 7, false);
    assert_eq!(kinds, [WebCallKind::Orders, WebCallKind::Notice]);
    let notice = store.notices.last().unwrap();
    assert_eq!(notice.order, 7);
    assert_eq!(notice.address, store.orders[0].1);
    assert_eq!(notice.state, NoticeState::Paid);
    assert_eq!(notice.counted_sats, 50_000);
    assert_eq!(notice.depth, 3);
    // The transaction ID is the one a block explorer shows.
    let mut shown = tx.compute_txid().to_byte_array();
    shown.reverse();
    assert_eq!(notice.txids, vec![shown]);
    // The store took it: it is not sent again, however often the node visits.
    rig.node.web_visit_soon();
    let kinds = visit(&mut rig, &mut store, START + 8, false);
    assert_eq!(kinds, [WebCallKind::Orders]);
    assert_eq!(store.notices.len(), 1);
    let info = rig.node.web_state();
    assert_eq!((info.told, info.notices_waiting), (1, 0));
    assert_eq!(rig.node.web_order_stand(7), Some(crate::OrderStand::Told));
}

#[test]
fn wo_30_a_part_payment_then_the_rest_and_an_extra_each_send_a_notice_of_their_state() {
    let dir = Dir::new();
    let (mut rig, _book, _setup, mut store) = paired(&dir, 1);
    store.order(7, 50_000);
    rig.node.web_visit_soon();
    visit(&mut rig, &mut store, START + 5, false);
    rig.mine(&[pay(0, 5, 20_000)]);
    visit(&mut rig, &mut store, START + 6, false);
    rig.mine(&[pay(0, 6, 30_000)]);
    visit(&mut rig, &mut store, START + 7, false);
    rig.mine(&[pay(0, 7, 1_000)]);
    visit(&mut rig, &mut store, START + 8, false);
    let states: Vec<(NoticeState, u64)> = store
        .notices
        .iter()
        .map(|n| (n.state, n.counted_sats))
        .collect();
    assert_eq!(
        states,
        [
            (NoticeState::PartPaid, 20_000),
            (NoticeState::Paid, 50_000),
            (NoticeState::Overpaid, 51_000),
        ]
    );
    assert_eq!(store.notices[2].txids.len(), 3);
}

#[test]
fn wo_30_a_payment_after_the_request_was_cancelled_or_late_is_paid_late() {
    let dir = Dir::new();
    let (mut rig, _book, _setup, mut store) = paired(&dir, 1);
    store.order(7, 50_000);
    rig.node.web_visit_soon();
    visit(&mut rig, &mut store, START + 5, false);
    rig.node.cancel_request(1, START + 6).unwrap();
    rig.mine(&[pay(0, 5, 50_000)]);
    visit(&mut rig, &mut store, START + 7, false);
    assert_eq!(store.notices.len(), 1);
    assert_eq!(store.notices[0].state, NoticeState::PaidLate);
    assert_eq!(store.notices[0].counted_sats, 50_000);
}

#[test]
fn wo_12_a_payment_to_an_address_before_its_order_is_taken_is_not_lost() {
    let dir = Dir::new();
    let (mut rig, _book, _setup, mut store) = paired(&dir, 1);
    // The customer pays before the owner phone's next visit.
    let address = store.order(7, 50_000);
    rig.mine(&[pay(0, 5, 50_000)]);
    assert_eq!(rig.node.request_arrivals().len(), 1);
    rig.mine_empty(2);
    // It waits for the order, past the depth.
    assert_eq!(rig.node.request_arrivals().len(), 1);
    rig.node.web_visit_soon();
    let kinds = visit(&mut rig, &mut store, START + 5, false);
    assert!(kinds.contains(&WebCallKind::Orders));
    // The pump credits what waited, and the next visit tells the store.
    rig.node.pump(Duration::ZERO);
    visit(&mut rig, &mut store, START + 6, false);
    let request = rig.node.requests().unwrap().request(1).unwrap().clone();
    assert_eq!(request.status(0), Status::Paid);
    assert_eq!(store.notices.len(), 1);
    assert_eq!(store.notices[0].address, address);
    assert!(rig.node.request_arrivals().is_empty());
}

#[test]
fn wo_07_unpairing_forgets_the_key_and_keeps_the_addresses_watched() {
    let dir = Dir::new();
    let (mut rig, _book, _setup, mut store) = paired(&dir, 1);
    store.order(7, 50_000);
    rig.node.web_visit_soon();
    visit(&mut rig, &mut store, START + 5, false);
    rig.node.web_unpair().unwrap();
    assert_eq!(rig.node.web_state().pairing, None);
    assert!(visit(&mut rig, &mut store, START + 6, true).is_empty());
    assert_eq!(rig.node.web_indexes().len(), 100);
    assert_eq!(rig.node.web_state().orders, 1);
    assert_eq!(rig.node.web_unpair().unwrap_err(), WebError::NotPaired);
    // The key and the secret are not in the file any more.
    let bytes = std::fs::read(dir.path().join(WEB_ORDERS_FILE)).unwrap();
    assert_eq!(bytes.get(5), Some(&0));
    // A payment to a given address is still seen.
    rig.mine(&[pay(3, 5, 9_000)]);
    assert_eq!(rig.node.request_arrivals().len(), 1);
    // A new pairing continues the sequence, so no address is given twice.
    let shown = rig
        .node
        .web_pair_start(STORE, "Corner Shop", START + 7, &mut Bytes(70))
        .unwrap();
    rig.node.web_pair_confirm().unwrap();
    let mut second = Store::paired(&shown.code);
    second.next_index = 100;
    visit(&mut rig, &mut second, START + 8, false);
    assert_eq!(second.pool.len(), 100);
    assert!(second.pool.iter().all(|a| !store.pool.contains(a)));
    assert_eq!(rig.node.web_indexes().len(), 200);
}

#[test]
fn wo_21_a_failed_visit_waits_30_seconds_then_twice_as_long_and_a_forced_one_does_not() {
    let dir = Dir::new();
    let (mut rig, _book, _setup, mut store) = paired(&dir, 1);
    let mut rng = Bytes(9);
    rig.node.web_visit_soon();
    let WebStep::Call(_) = rig.node.web_step(START + 100, false, &mut rng).unwrap() else {
        panic!("a call");
    };
    rig.node.web_failed(WebFailure::NotReached, START + 100);
    assert_eq!(rig.node.web_state().failure, Some(WebFailure::NotReached));
    assert_eq!(rig.node.web_state().retry_at, Some(START + 130));
    assert_eq!(
        rig.node.web_step(START + 129, false, &mut rng).unwrap(),
        WebStep::Idle
    );
    let WebStep::Call(_) = rig.node.web_step(START + 130, false, &mut rng).unwrap() else {
        panic!("a call");
    };
    rig.node.web_failed(WebFailure::TorMissing, START + 130);
    assert_eq!(rig.node.web_state().retry_at, Some(START + 190));
    // The owner asks: no waiting.
    let WebStep::Call(call) = rig.node.web_step(START + 131, true, &mut rng).unwrap() else {
        panic!("a call");
    };
    let (status, body) = store.handle(&call);
    rig.node.web_reply(status, &body, START + 131).unwrap();
    assert_eq!(rig.node.web_state().failure, None);
    assert_eq!(rig.node.web_state().retry_at, None);
    // The waiting never passes an hour.
    for n in 0..12 {
        rig.node.web_visit_soon();
        let now = START + 1_000 + n;
        let _ = rig.node.web_step(now + 4_000, true, &mut rng).unwrap();
        rig.node.web_failed(WebFailure::NotReached, now);
    }
    assert_eq!(rig.node.web_state().retry_at, Some(START + 1_011 + 3_600));
}

#[test]
fn wo_01_a_store_that_answers_403_is_shown_as_denied_and_nothing_changes() {
    let dir = Dir::new();
    let (mut rig, _book, _setup, mut store) = paired(&dir, 1);
    store.denied = true;
    rig.node.web_visit_soon();
    let kinds = visit(&mut rig, &mut store, START + 5, false);
    assert_eq!(kinds, [WebCallKind::Orders]);
    assert_eq!(rig.node.web_state().failure, Some(WebFailure::Denied));
    assert_eq!(rig.node.web_state().last_visit, Some(START));
}

#[test]
fn wo_20_a_reply_with_nothing_in_the_air_or_of_another_shape_changes_nothing() {
    let dir = Dir::new();
    let (mut rig, _book, _setup, mut store) = paired(&dir, 1);
    assert_eq!(
        rig.node.web_reply(200, b"{}", START).unwrap_err(),
        WebError::Sequence
    );
    store.order(7, 50_000);
    rig.node.web_visit_soon();
    let mut rng = Bytes(9);
    let WebStep::Call(_) = rig.node.web_step(START + 5, false, &mut rng).unwrap() else {
        panic!("a call");
    };
    assert_eq!(
        rig.node.web_step(START + 5, true, &mut rng).unwrap(),
        WebStep::Busy
    );
    rig.node
        .web_reply(200, b"{\"orders\":\"no\"}", START + 5)
        .unwrap();
    assert_eq!(rig.node.web_state().failure, Some(WebFailure::BadAnswer));
    assert_eq!(rig.node.web_state().orders, 0);
    assert_eq!(rig.node.requests().unwrap().requests().len(), 0);
}

#[test]
fn wo_22_an_order_below_the_shops_smallest_sale_is_refused_and_shown() {
    let dir = Dir::new();
    let (mut rig, _book, _setup, mut store) = paired(&dir, 1);
    let mut ctx = context(1);
    ctx.min_sale_sats = 20_000;
    rig.node.set_request_context(ctx, None);
    store.order(7, 5_000);
    store.order(8, 30_000);
    rig.node.web_visit_soon();
    visit(&mut rig, &mut store, START + 5, false);
    let info = rig.node.web_state();
    assert_eq!(info.refused.len(), 1);
    assert_eq!(
        (info.refused[0].order, info.refused[0].why),
        (7, RefusedWhy::NoRequest)
    );
    assert_eq!(info.orders, 2);
    assert_eq!(
        rig.node.web_order_stand(7),
        Some(crate::OrderStand::Refused)
    );
    // The refused order's address is used: a second order cannot take it.
    let address = store.orders[0].1.clone();
    store.orders.push((9, address, 30_000));
    rig.node.web_visit_soon();
    visit(&mut rig, &mut store, START + 6, false);
    assert_eq!(
        rig.node.web_state().refused.last().unwrap().why,
        RefusedWhy::AddressUsedBefore
    );
}

#[test]
fn wo_22_an_order_waits_for_the_shops_settings_and_is_taken_once_they_are_given() {
    let dir = Dir::new();
    let (rig, book, setup, mut store) = paired(&dir, 1);
    store.order(7, 50_000);
    let mut rig = restart_bare(rig, &dir, book, &setup);
    let kinds = visit(&mut rig, &mut store, START + 5, false);
    assert_eq!(kinds, [WebCallKind::Orders]);
    // Nothing was made, nothing refused, and the order is asked for again.
    assert_eq!(rig.node.requests().unwrap().requests().len(), 0);
    let info = rig.node.web_state();
    assert_eq!((info.orders, info.refused.len()), (0, 0));
    assert_eq!(info.failure, Some(WebFailure::NotReady));
    rig.node.set_request_context(context(1), None);
    let kinds = visit(&mut rig, &mut store, START + 6, true);
    assert_eq!(kinds, [WebCallKind::Orders]);
    assert_eq!(rig.node.requests().unwrap().requests().len(), 1);
    assert_eq!(rig.node.web_state().orders, 1);
    assert_eq!(rig.node.web_state().failure, None);
}

#[test]
fn wo_12_a_restart_reads_the_file_and_goes_on_where_the_visits_were() {
    let dir = Dir::new();
    let (mut rig, book, setup, mut store) = paired(&dir, 1);
    store.order(7, 50_000);
    rig.node.web_visit_soon();
    visit(&mut rig, &mut store, START + 5, false);
    rig.mine(&[pay(0, 5, 50_000)]);
    visit(&mut rig, &mut store, START + 6, false);
    assert_eq!(store.notices.len(), 1);
    let mut rig = restart(rig, &dir, book, &setup);
    // The node visits when it starts (WO-21), and has nothing new to say.
    let kinds = visit(&mut rig, &mut store, START + 7, false);
    assert_eq!(kinds, [WebCallKind::Orders]);
    assert_eq!(store.notices.len(), 1);
    assert_eq!(rig.node.web_indexes().len(), 100);
    assert_eq!(rig.node.web_state().orders, 1);
    assert_eq!(rig.node.web_state().told, 1);
    // The next range continues the sequence.
    for n in 8..=90 {
        store.order(n, 10_000);
    }
    rig.node.web_visit_soon();
    visit(&mut rig, &mut store, START + 8, false);
    assert_eq!(store.next_index, 200);
}

#[test]
fn wo_12_a_visit_cut_off_after_the_request_was_made_does_not_make_it_twice() {
    let dir = Dir::new();
    let (mut rig, _book, _setup, mut store) = paired(&dir, 1);
    store.order(7, 50_000);
    // The request is in the book, but the file of web orders does not know.
    let index = 0;
    let new = openbmt_requests::NewRequest {
        note: "Web order 7".to_string(),
        amount: 50_000,
        price_in: openbmt_requests::PriceIn::Bitcoin,
        due: openbmt_requests::day_of(START + 86_400, 0),
        repeat: None,
    };
    rig.node.make_request(&new, index, START).unwrap();
    rig.node.web_visit_soon();
    visit(&mut rig, &mut store, START + 5, false);
    assert_eq!(rig.node.requests().unwrap().requests().len(), 1);
    assert_eq!(rig.node.web_state().orders, 1);
    assert!(rig.node.web_state().refused.is_empty());
}

#[test]
fn wo_12_the_file_reads_strictly_and_encodes_to_the_same_bytes() {
    let dir = Dir::new();
    let (mut rig, _book, _setup, mut store) = paired(&dir, 1);
    store.order(7, 50_000);
    rig.node.web_visit_soon();
    visit(&mut rig, &mut store, START + 5, false);
    rig.mine(&[pay(0, 5, 50_000)]);
    visit(&mut rig, &mut store, START + 6, false);
    let bytes = std::fs::read(dir.path().join(WEB_ORDERS_FILE)).unwrap();
    assert_eq!(&bytes[..4], &MAGIC_WEB_ORDERS);
    let state = decode(&bytes).unwrap();
    assert_eq!(encode(&state), bytes);
    // A byte cut, a byte added, a bad magic, a version and every single byte
    // changed in the header part are refused or read back as something that
    // encodes to the same bytes.
    assert!(decode(&bytes[..bytes.len() - 1]).is_err());
    let mut longer = bytes.clone();
    longer.push(0);
    assert!(decode(&longer).is_err());
    let mut bad = bytes.clone();
    bad[0] ^= 1;
    assert!(decode(&bad).is_err());
    bad = bytes.clone();
    bad[4] = 2;
    assert!(decode(&bad).is_err());
    for at in 5..bytes.len().min(400) {
        let mut changed = bytes.clone();
        changed[at] ^= 0x01;
        if let Ok(read) = decode(&changed) {
            assert_eq!(encode(&read), changed, "byte {at}");
        }
    }
}

#[test]
fn wo_12_a_file_that_does_not_read_back_leaves_web_orders_off_and_is_not_touched() {
    let dir = Dir::new();
    let (rig, book, setup, _store) = paired(&dir, 1);
    let mut bytes = std::fs::read(dir.path().join(WEB_ORDERS_FILE)).unwrap();
    let last = bytes.len() - 1;
    bytes[last] ^= 0xff;
    bytes.push(7);
    std::fs::write(dir.path().join(WEB_ORDERS_FILE), &bytes).unwrap();
    let mut rig = restart(rig, &dir, book, &setup);
    let info = rig.node.web_state();
    assert!(matches!(info.off, Some(WebError::File(_))));
    assert!(matches!(
        rig.node
            .web_pair_start(STORE, "Corner Shop", START, &mut Bytes(1)),
        Err(WebError::File(_))
    ));
    assert!(matches!(
        rig.node.web_step(START, true, &mut Bytes(1)),
        Err(WebError::File(_))
    ));
    assert_eq!(
        std::fs::read(dir.path().join(WEB_ORDERS_FILE)).unwrap(),
        bytes
    );
    assert_eq!(rig.node.request_indexes().len(), 0);
}

#[test]
fn wo_43_the_key_a_pairing_makes_signs_for_the_code_it_shows() {
    let dir = Dir::new();
    let (mut rig, _book, _setup) = desk_rig(&dir, 1);
    let shown = rig
        .node
        .web_pair_start(STORE, "Corner Shop", START, &mut Bytes(1))
        .unwrap();
    let code = PairingCode::from_text(&shown.code).unwrap();
    let bytes = std::fs::read(dir.path().join(WEB_ORDERS_FILE)).unwrap();
    let state = decode(&bytes).unwrap();
    let pairing = state.pairing.unwrap();
    assert_eq!(pairing.key(&rig.node.owner_secret).public(), *code.public());
    assert_eq!(pairing.secret, *code.secret());
}

#[test]
fn wo_43_the_key_is_made_from_the_owner_secret_and_the_file_holds_no_seed() {
    let dir = Dir::new();
    let (mut rig, book, setup) = desk_rig(&dir, 1);
    let shown = rig
        .node
        .web_pair_start(STORE, "Corner Shop", START, &mut Bytes(1))
        .unwrap();
    let owner_secret = rig.node.owner_secret;
    let bytes = std::fs::read(dir.path().join(WEB_ORDERS_FILE)).unwrap();
    let pairing = decode(&bytes).unwrap().pairing.unwrap();
    let seed = web_key_seed(&owner_secret, &pairing.secret);
    assert!(
        !bytes.windows(32).any(|w| w == seed),
        "the file holds the seed"
    );
    // The same owner secret gives the same key after a restart.
    let rig = restart(rig, &dir, book, &setup);
    let state = rig.node.web_state();
    assert_eq!(state.fingerprint, shown.fingerprint);
    let again = decode(&std::fs::read(dir.path().join(WEB_ORDERS_FILE)).unwrap())
        .unwrap()
        .pairing
        .unwrap();
    assert_eq!(
        again.key(&owner_secret).public(),
        pairing.key(&owner_secret).public()
    );
    // Another owner secret gives another key.
    let mut other = owner_secret;
    other[0] ^= 1;
    assert_ne!(web_key_seed(&other, &pairing.secret), seed);
    assert_ne!(
        again.key(&other).public(),
        again.key(&owner_secret).public()
    );
}
