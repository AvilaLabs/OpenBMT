//! BX-40 from the counter's side: the link sends a paid notice to the Box
//! for each change of a sale's state, keeps the notices until the relay has
//! taken them, and never gives the Box a record.

use bitcoin::Transaction;
use openbmt_core::identity::DeviceId;
use openbmt_core::records::{Chain, DeviceList};
use openbmt_counter::{Amount, Report};
use openbmt_courier::{Courier, FirstStart, MemoryPost, MemoryRelay, Received};
use openbmt_link::{Kind, Role};
use openbmt_net_io::AppEvent;
use openbmt_pos::{
    facts_from_chain, facts_from_notices, NoticeState, PaidNotice, SaleCancel, SaleRequest,
    TillMessage,
};

use std::sync::atomic::AtomicU64;
use std::sync::Arc;

use super::kit::config as config_in;
use super::kit::*;
use crate::notices::{NoticeOutbox, NOTICE_FILE};
use crate::{CounterLink, CounterLinkConfig};

fn config() -> CounterLinkConfig {
    CounterLinkConfig {
        poll_secs: 0,
        retry_secs: 0,
        ..CounterLinkConfig::default()
    }
}

fn owner() -> DeviceId {
    id(1).device_id()
}

fn counter() -> DeviceId {
    id(3).device_id()
}

fn the_box() -> DeviceId {
    id(7).device_id()
}

fn sighting(tx: &Transaction) -> AppEvent {
    AppEvent::Sighting {
        tx: bytes(tx),
        from: vec![peer(20)],
    }
}

struct World {
    rig: Rig,
    link: CounterLink<MemoryPost>,
    box_side: Courier<MemoryPost>,
    relay: MemoryRelay,
    link_dir: Scratch,
    _box_dir: Scratch,
    _node_dir: Scratch,
    now: u64,
}

fn link_over(
    relay: &MemoryRelay,
    dir: &Scratch,
    first: FirstStart,
    boxes: bool,
) -> CounterLink<MemoryPost> {
    let mut peers = vec![(owner(), Role::Owner)];
    if boxes {
        peers.push((the_box(), Role::Box));
    }
    let courier = Courier::start_with(
        relay.post(),
        id(3),
        Role::Counter,
        peers.into_iter().collect(),
        dir.path(),
        first,
    )
    .unwrap();
    CounterLink::new(courier, owner(), config())
}

fn world(boxes: bool) -> World {
    world_with(boxes, false)
}

/// A world whose settings in force name the Box (BX-20) when `named` is set.
fn world_with(boxes: bool, named: bool) -> World {
    let relay = MemoryRelay::new(id(99), &[owner(), counter(), the_box()]).unwrap();
    let node_dir = Scratch::new();
    let link_dir = Scratch::new();
    let box_dir = Scratch::new();
    let mut rig = if named {
        let time = Arc::new(AtomicU64::new(T0));
        let mut config = config_in(node_dir.path(), &time);
        config.settings.devices.insert(the_box());
        config.settings.boxes.insert(the_box());
        start_with(config, time)
    } else {
        start(node_dir.path())
    };
    let mut link = link_over(&relay, &link_dir, FirstStart::Yes, boxes);
    link.enable_notices(id(3), link_dir.path(), &rig.node)
        .unwrap();
    let mut box_side = Courier::start_with(
        relay.post(),
        id(7),
        Role::Box,
        [(counter(), Role::Counter)].into_iter().collect(),
        box_dir.path(),
        FirstStart::Yes,
    )
    .unwrap();
    let mut owner_side = Courier::start_with(
        relay.post(),
        id(1),
        Role::Owner,
        [(counter(), Role::Counter)].into_iter().collect(),
        Scratch::new().path(),
        FirstStart::Yes,
    )
    .unwrap();
    box_side.poll().unwrap();
    owner_side.poll().unwrap();
    link.courier_mut().poll().unwrap();
    assert!(rig.feed(vec![Rig::peers()]).is_empty());
    World {
        rig,
        link,
        box_side,
        relay,
        link_dir,
        _box_dir: box_dir,
        _node_dir: node_dir,
        now: T0,
    }
}

impl World {
    fn step(&mut self) {
        self.now += 1;
        self.link.step(&mut self.rig.node, self.now);
    }

    fn notices(&mut self) -> Vec<PaidNotice> {
        let got = self.box_side.poll().unwrap();
        got.iter()
            .map(|r| {
                assert_eq!(r.kind, Kind::PaidNotice);
                assert_eq!(r.from, counter());
                PaidNotice::from_bytes(&r.content).unwrap()
            })
            .collect()
    }

    fn open_sale(&mut self) {
        self.rig
            .node
            .open(five_dollars(), usd(), rate(), &source_at(&self.rig.time))
            .unwrap();
    }
}

fn states(n: &[PaidNotice]) -> Vec<(u64, u32, NoticeState)> {
    n.iter().map(|n| (n.number, n.index, n.state)).collect()
}

#[test]
fn bx_40_a_sale_that_opens_is_paid_goes_unpaid_and_becomes_final_is_told_to_the_box() {
    let mut w = world(true);
    w.open_sale();
    w.step();
    let got = w.notices();
    assert_eq!(states(&got), vec![(1, 0, NoticeState::Opened)]);
    assert!(got[0].verify());
    assert_eq!((got[0].amount.0, got[0].opened_at), (500, T0));

    let pay = spend(1, &[(&script_of(0), 5_000)]);
    w.rig.feed(vec![sighting(&pay)]);
    let mut blocks = Blocks::default();
    let first = blocks.extend(std::slice::from_ref(&pay)).clone();
    assert_eq!(
        w.rig.feed(vec![joined(1, &first), block_event(&first)]),
        vec![Report::PosPaid { index: 0 }]
    );
    w.step();
    assert_eq!(states(&w.notices()), vec![(2, 0, NoticeState::Paid)]);

    // A reorganization takes the payment's block away: unpaid again.
    let alt1 = mine(genesis(), 1, 1, &[]);
    let alt2 = mine(alt1.block_hash(), 2, 1, &[]);
    let reorg = AppEvent::Chain {
        joined: vec![(1, alt1.block_hash()), (2, alt2.block_hash())],
        left: vec![first.block_hash()],
    };
    assert_eq!(
        w.rig.feed(vec![reorg]),
        vec![Report::PosUnpaid { index: 0 }]
    );
    w.step();
    assert_eq!(states(&w.notices()), vec![(3, 0, NoticeState::Unpaid)]);

    // Mined again, then six deep: paid again, then final.
    let again = mine(alt2.block_hash(), 3, 1, &[pay]);
    w.rig.feed(vec![joined(3, &again), block_event(&again)]);
    w.step();
    let paid_again = w.notices();
    assert_eq!(states(&paid_again), vec![(4, 0, NoticeState::Paid)]);
    let mut tip = again;
    let mut events = Vec::new();
    for height in 4..=8 {
        tip = mine(tip.block_hash(), height, 1, &[]);
        events.push(joined(height, &tip));
    }
    w.rig.feed(events);
    w.step();
    let last = w.notices();
    assert_eq!(states(&last), vec![(5, 0, NoticeState::Final)]);

    // Nothing more is sent while nothing changes.
    w.step();
    assert!(w.notices().is_empty());
}

#[test]
fn bx_40_the_notices_give_the_facts_the_counters_own_records_give() {
    let mut w = world(true);
    w.open_sale();
    w.open_sale();
    let pay = spend(1, &[(&script_of(0), 5_000)]);
    w.rig.feed(vec![sighting(&pay)]);
    let mut blocks = Blocks::default();
    let first = blocks.extend(std::slice::from_ref(&pay)).clone();
    w.rig.feed(vec![joined(1, &first), block_event(&first)]);
    w.step();
    let notices = w.notices();
    assert_eq!(notices.len(), 3);
    let records = w.rig.node.records_from(0, usize::MAX).unwrap();
    let chain = Chain::from_records(records);
    let from_chain = facts_from_chain(&chain, &DeviceList::new([counter()]), &counter()).unwrap();
    assert_eq!(facts_from_notices(&notices), from_chain);
}

#[test]
fn bx_40_the_counter_never_gives_the_box_its_records() {
    let mut w = world(true);
    w.open_sale();
    // Several steps: check-ins and records go out to the owner.
    for _ in 0..3 {
        w.step();
    }
    assert!(w.relay.puts() > 0);
    // Everything the Box holds is a paid notice.
    let got: Vec<Received> = w.box_side.poll().unwrap();
    assert!(!got.is_empty());
    assert!(got.iter().all(|r| r.kind == Kind::PaidNotice));
    // And the courier refuses to address a Records message to a Box at all.
    let records = w.rig.node.records_from(0, usize::MAX).unwrap();
    let (content, _) = openbmt_courier::records_content(&records, 4_000);
    let err = w
        .link
        .courier_mut()
        .send(&the_box(), Kind::Records, &content)
        .unwrap_err();
    assert_eq!(err, openbmt_courier::SendError::KindNotAllowed);
    // Nor the check-in.
    assert!(w
        .link
        .courier_mut()
        .send(&the_box(), Kind::CheckIn, &content)
        .is_err());
}

#[test]
fn bx_40_with_no_box_nothing_is_sent_and_nothing_is_stored() {
    let mut w = world(false);
    w.open_sale();
    w.step();
    w.step();
    assert!(w.notices().is_empty());
    assert!(!w.link_dir.path().join(NOTICE_FILE).exists());
    assert_eq!(w.link.notices().unwrap().last_number(), 0);
}

#[test]
fn bx_40_a_notice_the_relay_has_not_taken_is_kept_and_sent_after_a_restart() {
    let mut w = world(true);
    w.open_sale();
    w.relay.set_down(true);
    w.step();
    // Stored before anything was sent: number 1 is in the file.
    assert_eq!(w.link.notices().unwrap().last_number(), 1);
    assert!(w.link_dir.path().join(NOTICE_FILE).exists());
    let reopened =
        NoticeOutbox::open(id(3), w.link_dir.path(), &w.rig.node.record_source()).unwrap();
    assert_eq!(reopened.queue().len(), 1);
    assert_eq!(reopened.queue()[0].number, 1);

    // The counter restarts: a new link over the same files.
    let mut link = link_over(&w.relay, &w.link_dir, FirstStart::No, true);
    link.enable_notices(id(3), w.link_dir.path(), &w.rig.node)
        .unwrap();
    w.link = link;
    w.relay.set_down(false);
    w.step();
    assert_eq!(states(&w.notices()), vec![(1, 0, NoticeState::Opened)]);
    // What the relay took is not sent again after another restart, and the
    // next change gets the next number.
    let mut link = link_over(&w.relay, &w.link_dir, FirstStart::No, true);
    link.enable_notices(id(3), w.link_dir.path(), &w.rig.node)
        .unwrap();
    w.link = link;
    w.step();
    assert!(w.notices().is_empty());
    w.open_sale();
    w.step();
    assert_eq!(states(&w.notices()), vec![(2, 1, NoticeState::Opened)]);
}

#[test]
fn bx_40_a_notice_the_relay_lost_is_not_sent_again_but_the_next_one_carries_the_sale() {
    // The relay answers "stored" and drops the message. The counter cannot
    // know. The Box learns the sale from the next notice, which carries the
    // whole sale.
    let mut w = world(true);
    w.open_sale();
    w.relay.set_lose_puts(true);
    w.step();
    w.relay.set_lose_puts(false);
    assert!(w.notices().is_empty());
    let pay = spend(1, &[(&script_of(0), 5_000)]);
    w.rig.feed(vec![sighting(&pay)]);
    let mut blocks = Blocks::default();
    let first = blocks.extend(std::slice::from_ref(&pay)).clone();
    w.rig.feed(vec![joined(1, &first), block_event(&first)]);
    w.step();
    let got = w.notices();
    assert_eq!(states(&got), vec![(2, 0, NoticeState::Paid)]);
    let facts = facts_from_notices(&got);
    assert_eq!((facts[0].amount.0, facts[0].opened_at), (500, T0));
    assert!(facts[0].is_paid_now());
}

#[test]
fn bx_40_a_notice_that_cannot_be_stored_or_sent_is_a_note_and_never_an_error_to_the_counter() {
    let mut w = world(true);
    w.open_sale();
    // The directory for the notice file is gone: nothing can be stored, so
    // nothing is sent, and the sale is as it was.
    let gone = Scratch::new();
    let path = gone.path().join("nowhere");
    let mut link = link_over(&w.relay, &w.link_dir, FirstStart::No, true);
    link.enable_notices(id(3), &path, &w.rig.node).unwrap();
    w.link = link;
    w.step();
    assert!(w.notices().is_empty());
    let notes = w.link.take_notes();
    assert!(notes.iter().any(|n| n.contains("not queued")), "{notes:?}");
    // The same trouble is noted once, not every second.
    w.step();
    w.step();
    assert!(w
        .link
        .take_notes()
        .iter()
        .all(|n| !n.contains("not queued")));
    // The sale is untouched and can still be judged.
    assert_eq!(
        w.rig.node.judge(0).unwrap().state,
        openbmt_counter::State::NotPaidYet
    );
}

#[test]
fn bx_40_notices_are_signed_with_the_counters_own_key_only() {
    let w = world(true);
    let mut link = link_over(&w.relay, &w.link_dir, FirstStart::No, true);
    let other = link.enable_notices(id(4), w.link_dir.path(), &w.rig.node);
    assert!(other.is_err());
}

#[test]
fn bx_40_a_damaged_notice_file_is_refused_at_start() {
    let w = world(true);
    std::fs::write(w.link_dir.path().join(NOTICE_FILE), b"not a notice file").unwrap();
    let mut link = link_over(&w.relay, &w.link_dir, FirstStart::No, true);
    assert!(link
        .enable_notices(id(3), w.link_dir.path(), &w.rig.node)
        .is_err());
}

#[test]
fn bx_40_a_queue_over_its_bound_drops_the_oldest_and_says_so() {
    use crate::notices::MAX_QUEUED;
    use openbmt_counter::{Amount, Event, RateSource};
    let dir = Scratch::new();
    let me = id(3);
    let mut chain = Chain::new();
    for index in 0..4_200u32 {
        let e = Event::Opened {
            index,
            at: T0,
            amount: Amount(500),
            tip: Amount(0),
            currency: usd(),
            needs_sats: 1,
            minor_per_btc: 1,
            source: RateSource::new("test-fixed", T0).unwrap(),
            by: None,
        };
        chain.append(&me, e.kind(), e.encode()).unwrap();
    }
    let mut out = NoticeOutbox::open(id(3), dir.path(), &[][..]).unwrap();
    let dropped = out.collect(chain.records()).unwrap();
    assert_eq!(dropped, 4_200 - MAX_QUEUED);
    assert_eq!(out.queue().len(), MAX_QUEUED);
    assert_eq!(out.last_number(), 4_200);
    assert_eq!(out.queue()[0].number, 4_200 - MAX_QUEUED as u64 + 1);
    // A restart reads the same queue back and folds the same records.
    let back = NoticeOutbox::open(id(3), dir.path(), chain.records()).unwrap();
    assert_eq!(back.queue(), out.queue());
    assert_eq!(back.last_number(), 4_200);
    // Taken by the only Box, the queue is emptied and the mark is kept.
    out.mark_sent(&the_box(), 4_200);
    out.settle(&[the_box()]).unwrap();
    assert!(out.queue().is_empty());
}

// CR-266 to CR-270: a sale from the till.

fn till_request(sale: u8, cents: u64, lapses_at: u64) -> Vec<u8> {
    TillMessage::Request(SaleRequest {
        sale: [sale; 16],
        amount: Amount(cents),
        currency: usd(),
        reference: "POS/0001".to_string(),
        expires_at: lapses_at,
    })
    .to_bytes()
}

fn till_cancel(sale: u8) -> Vec<u8> {
    TillMessage::Cancel(SaleCancel { sale: [sale; 16] }).to_bytes()
}

impl World {
    /// The Box sends the counter a POS notice and the link takes it.
    fn box_sends(&mut self, content: &[u8]) {
        self.box_side
            .send(&counter(), Kind::PosNotice, content)
            .unwrap();
        self.step();
    }

    fn start_till_sale(&mut self, sale: u8) -> openbmt_counter::Opened {
        self.rig
            .node
            .till_start(&[sale; 16], rate(), &source_at(&self.rig.time), None)
            .unwrap()
    }

    fn notes(&mut self) -> Vec<String> {
        self.link.take_notes()
    }
}

#[test]
fn cr_266_a_request_box_sends_of_the_settings_waits_for_the_cashier() {
    let mut w = world_with(true, true);
    w.box_sends(&till_request(1, 500, T0 + 300));
    let waiting = w.rig.node.till_waiting();
    assert_eq!(waiting.len(), 1);
    assert_eq!(waiting[0].amount, Amount(500));
    assert_eq!(waiting[0].sale, [1; 16]);
    // Nothing is sent to the Box for a request that only waits.
    assert!(w.notices().is_empty());
    assert_eq!(w.rig.node.sales().count(), 0);
}

#[test]
fn cr_266_a_notice_from_a_key_that_is_not_the_box_of_the_settings_is_dropped_with_a_note() {
    // The key is in the courier's table as a Box, but the settings in force
    // do not name it.
    let mut w = world_with(true, false);
    w.box_sends(&till_request(1, 500, T0 + 300));
    assert!(w.rig.node.till_waiting().is_empty());
    assert!(w.notes().iter().any(|n| n.contains("CR-266")));
    assert!(w.notices().is_empty());
}

#[test]
fn cr_266_a_request_that_does_not_fit_or_does_not_read_is_dropped_with_a_note() {
    let mut w = world_with(true, true);
    // Not the shop's currency.
    let eur = TillMessage::Request(SaleRequest {
        sale: [1; 16],
        amount: Amount(500),
        currency: openbmt_counter::Currency::new(*b"EUR").unwrap(),
        reference: String::new(),
        expires_at: T0 + 300,
    })
    .to_bytes();
    w.box_sends(&eur);
    // A lapse time that is past.
    w.box_sends(&till_request(2, 500, T0));
    // Not a till message at all.
    w.box_sends(b"nothing of the till");
    assert!(w.rig.node.till_waiting().is_empty());
    let notes = w.notes();
    assert_eq!(notes.iter().filter(|n| n.contains("CR-266")).count(), 3);
    assert!(w.notices().is_empty());
}

#[test]
fn cr_266_at_most_eight_requests_wait() {
    let mut w = world_with(true, true);
    for n in 1..=9u8 {
        w.box_sends(&till_request(n, 100 * u64::from(n), T0 + 300));
    }
    assert_eq!(w.rig.node.till_waiting().len(), 8);
    assert!(w.notes().iter().any(|n| n.contains("8 requests")));
}

#[test]
fn cr_267_not_now_hides_a_request_and_sends_nothing() {
    let mut w = world_with(true, true);
    w.box_sends(&till_request(1, 500, T0 + 300));
    assert!(w.rig.node.till_not_now(&[1; 16]));
    assert!(w.rig.node.till_waiting().is_empty());
    w.step();
    assert!(w.notices().is_empty());
}

#[test]
fn cr_268_start_sends_the_opened_notice_naming_the_till_sale_and_every_later_one_does_too() {
    let mut w = world_with(true, true);
    w.box_sends(&till_request(1, 500, T0 + 300));
    let opened = w.start_till_sale(1);
    assert!(w.rig.node.till_waiting().is_empty());
    assert_eq!(w.rig.node.sale_till(opened.index), Some([1; 16]));
    w.step();
    let got = w.notices();
    assert_eq!(states(&got), vec![(1, opened.index, NoticeState::Opened)]);
    assert_eq!(got[0].till_sale, Some([1; 16]));
    assert!(got[0].verify());
    assert_eq!((got[0].amount.0, got[0].currency), (500, usd()));

    // Paid, unpaid again, paid and final: each names the till's sale.
    let pay = spend(1, &[(&script_of(opened.index), 5_000)]);
    w.rig.feed(vec![sighting(&pay)]);
    let mut blocks = Blocks::default();
    let first = blocks.extend(std::slice::from_ref(&pay)).clone();
    w.rig.feed(vec![joined(1, &first), block_event(&first)]);
    w.step();
    let paid = w.notices();
    assert_eq!(states(&paid), vec![(2, opened.index, NoticeState::Paid)]);
    assert_eq!(paid[0].till_sale, Some([1; 16]));
    let alt1 = mine(genesis(), 1, 1, &[]);
    let alt2 = mine(alt1.block_hash(), 2, 1, &[]);
    let reorg = AppEvent::Chain {
        joined: vec![(1, alt1.block_hash()), (2, alt2.block_hash())],
        left: vec![first.block_hash()],
    };
    w.rig.feed(vec![reorg]);
    w.step();
    let unpaid = w.notices();
    assert_eq!(
        states(&unpaid),
        vec![(3, opened.index, NoticeState::Unpaid)]
    );
    assert_eq!(unpaid[0].till_sale, Some([1; 16]));
    let again = mine(alt2.block_hash(), 3, 1, &[pay]);
    w.rig.feed(vec![joined(3, &again), block_event(&again)]);
    let mut tip = again;
    let mut events = Vec::new();
    for height in 4..=8 {
        tip = mine(tip.block_hash(), height, 1, &[]);
        events.push(joined(height, &tip));
    }
    w.rig.feed(events);
    w.step();
    let last = w.notices();
    assert!(!last.is_empty());
    for n in &last {
        assert_eq!(n.till_sale, Some([1; 16]), "{:?}", n.state);
        assert!(n.verify());
    }
    assert_eq!(last.last().map(|n| n.state), Some(NoticeState::Final));
}

#[test]
fn cr_268_a_sale_typed_at_the_counter_names_no_till_sale() {
    let mut w = world_with(true, true);
    w.open_sale();
    w.step();
    let got = w.notices();
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].till_sale, None);
    assert_eq!(got[0].to_bytes().len(), openbmt_pos::PAID_NOTICE_LEN);
}

#[test]
fn cr_269_a_cancel_before_start_removes_the_request_and_sends_nothing() {
    let mut w = world_with(true, true);
    w.box_sends(&till_request(1, 500, T0 + 300));
    w.box_sends(&till_cancel(1));
    assert!(w.rig.node.till_waiting().is_empty());
    assert!(w
        .rig
        .node
        .till_start(&[1; 16], rate(), &source_at(&w.rig.time), None)
        .is_err());
    w.step();
    assert!(w.notices().is_empty());
    assert_eq!(w.rig.node.sales().count(), 0);
}

#[test]
fn cr_269_a_cancel_after_start_marks_the_sale_and_a_payment_that_comes_anyway_is_told() {
    let mut w = world_with(true, true);
    w.box_sends(&till_request(1, 500, T0 + 300));
    let opened = w.start_till_sale(1);
    w.step();
    assert_eq!(
        states(&w.notices()),
        vec![(1, opened.index, NoticeState::Opened)]
    );
    w.box_sends(&till_cancel(1));
    assert!(w.rig.node.till_cancelled(opened.index));
    // The sale is not ended and nothing is sent about the cancel.
    assert!(w.notices().is_empty());
    assert!(w.rig.node.counter().sale(opened.index).is_some());
    // The payment arrives anyway: it is counted and told, naming the sale.
    let pay = spend(1, &[(&script_of(opened.index), 5_000)]);
    w.rig.feed(vec![sighting(&pay)]);
    let mut blocks = Blocks::default();
    let first = blocks.extend(std::slice::from_ref(&pay)).clone();
    w.rig.feed(vec![joined(1, &first), block_event(&first)]);
    w.step();
    let paid = w.notices();
    assert_eq!(states(&paid), vec![(2, opened.index, NoticeState::Paid)]);
    assert_eq!(paid[0].till_sale, Some([1; 16]));
    assert!(w.rig.node.till_cancelled(opened.index));
}

#[test]
fn cr_270_a_restart_of_the_notices_goes_on_naming_the_till_sale() {
    let mut w = world_with(true, true);
    w.box_sends(&till_request(1, 500, T0 + 300));
    let opened = w.start_till_sale(1);
    w.step();
    assert_eq!(w.notices().len(), 1);
    // A new outbox over the same file folds the records again.
    let mut outbox =
        NoticeOutbox::open(id(3), w.link_dir.path(), &w.rig.node.record_source()).unwrap();
    let pay = spend(1, &[(&script_of(opened.index), 5_000)]);
    w.rig.feed(vec![sighting(&pay)]);
    let mut blocks = Blocks::default();
    let first = blocks.extend(std::slice::from_ref(&pay)).clone();
    w.rig.feed(vec![joined(1, &first), block_event(&first)]);
    outbox.collect(&w.rig.node.record_source()).unwrap();
    let queued = outbox.queue();
    assert_eq!(queued.len(), 1);
    assert_eq!(queued[0].state, NoticeState::Paid);
    assert_eq!(queued[0].till_sale, Some([1; 16]));
}

#[test]
fn bx_77_the_queue_file_keeps_a_notice_that_names_a_till_sale_and_one_that_does_not() {
    let mut w = world_with(true, true);
    w.open_sale();
    w.box_sends(&till_request(1, 500, T0 + 300));
    w.start_till_sale(1);
    // A queue of one notice with no till sale and one with it, stored and
    // read back whole.
    let dir = Scratch::new();
    let mut outbox = NoticeOutbox::open(id(3), dir.path(), &[][..]).unwrap();
    outbox.collect(&w.rig.node.record_source()).unwrap();
    let before: Vec<PaidNotice> = outbox.queue().to_vec();
    assert_eq!(before.len(), 2);
    assert_eq!(before[0].till_sale, None);
    assert_eq!(before[1].till_sale, Some([1; 16]));
    let again = NoticeOutbox::open(id(3), dir.path(), &w.rig.node.record_source()).unwrap();
    assert_eq!(again.queue(), &before[..]);
}
