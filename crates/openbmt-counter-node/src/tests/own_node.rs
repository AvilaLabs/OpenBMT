//! The own node through the node (CR-103, CR-104, CR-107, CR-108), on a
//! driver that the test feeds by hand: the light client's sightings on one
//! channel, the own node's reports and states on the other.

use std::sync::atomic::Ordering;

use bitcoin::hashes::Hash;
use bitcoin::{Transaction, Txid};
use openbmt_core::heartbeat::WatcherHeartbeat;
use openbmt_core::policy::{CheckMode, InstantLabel, WaitReason};
use openbmt_counter::say::CheckLine;
use openbmt_counter::{Amount, Report, State, Why};
use openbmt_net_io::{AppEvent, OwnNodeEvent};
use openbmt_watch::OwnNodeProblem;

use super::kit::*;
use crate::Node;

/// The node's chain starts at the regtest genesis block: height 0.
fn hb(seq: u64, time: u64) -> Vec<u8> {
    WatcherHeartbeat {
        counter: id(3).device_id(),
        seq,
        time,
        tip_height: 0,
        tip_hash: genesis().to_byte_array(),
        healthy: true,
    }
    .sign(&id(1))
    .to_bytes()
}

/// $20: over the light check's $10, inside the own-node tier's $25.
const TWENTY: Amount = Amount(2_000);

fn payment() -> Transaction {
    spend(1, &[(&script_of(0), 20_000)])
}

fn seen_by_three(tx: &Transaction) -> AppEvent {
    AppEvent::Sighting {
        tx: bytes(tx),
        from: vec![peer(20), peer(21), peer(22)],
    }
}

/// A rig with an own node, a sale of $20 paid and seen by three groups, the
/// clock ten seconds on (past the conflict watch) and a heartbeat taken.
fn rig_with_a_seen_sale(dir: &Scratch) -> (Rig, Transaction) {
    let mut rig = start_with_own_node(dir.path());
    rig.feed(vec![Rig::peers()]);
    rig.node
        .open(TWENTY, usd(), rate(), &source_at(&rig.time))
        .unwrap();
    let tx = payment();
    rig.feed(vec![seen_by_three(&tx)]);
    rig.time.store(T0 + 10, Ordering::SeqCst);
    rig.node.heartbeat(&hb(1, T0 + 10)).unwrap();
    (rig, tx)
}

fn txid(tx: &Transaction) -> Txid {
    tx.compute_txid()
}

#[test]
fn cr_103_the_node_asks_the_own_node_after_a_light_sighting() {
    let dir = Scratch::new();
    let mut rig = start_with_own_node(dir.path());
    rig.feed(vec![Rig::peers()]);
    rig.node
        .open(TWENTY, usd(), rate(), &source_at(&rig.time))
        .unwrap();
    assert!(rig.calls.lock().unwrap().asked.is_empty());
    let tx = payment();
    rig.feed(vec![seen_by_three(&tx)]);
    assert_eq!(rig.calls.lock().unwrap().asked, vec![txid(&tx)]);
    // A transaction no sale takes is not asked about.
    let other = spend(2, &[(&p2wsh(0xaa), 20_000)]);
    rig.feed(vec![seen_by_three(&other)]);
    assert_eq!(rig.calls.lock().unwrap().asked, vec![txid(&tx)]);
}

#[test]
fn cr_103_a_restored_sale_is_asked_about_when_the_node_starts() {
    let dir = Scratch::new();
    let (mut rig, tx) = rig_with_a_seen_sale(&dir);
    assert!(rig.node.shutdown().is_empty());
    drop(rig);
    let again = start_with_own_node(dir.path());
    assert_eq!(again.calls.lock().unwrap().asked, vec![txid(&tx)]);
}

#[test]
fn cr_103_the_own_node_connecting_is_asked_again_and_only_once_per_connect() {
    let dir = Scratch::new();
    let (mut rig, tx) = rig_with_a_seen_sale(&dir);
    let before = rig.calls.lock().unwrap().asked.len();
    rig.feed_own(vec![own_connected(0, T0 + 10)]);
    assert_eq!(rig.calls.lock().unwrap().asked.len(), before + 1);
    assert_eq!(rig.calls.lock().unwrap().asked.last(), Some(&txid(&tx)));
    // The same state again is not a new connection.
    rig.feed_own(vec![own_connected(0, T0 + 10)]);
    assert_eq!(rig.calls.lock().unwrap().asked.len(), before + 1);
    // After a drop and a connection it is.
    rig.feed_own(vec![own_down(T0 + 11), own_connected(0, T0 + 12)]);
    assert_eq!(rig.calls.lock().unwrap().asked.len(), before + 2);
}

#[test]
fn cr_103_a_confirmed_payment_is_not_asked_about() {
    let dir = Scratch::new();
    let (mut rig, tx) = rig_with_a_seen_sale(&dir);
    let mut blocks = Blocks::default();
    let first = blocks.extend(std::slice::from_ref(&tx)).clone();
    rig.feed(vec![joined(1, &first), block_event(&first)]);
    let before = rig.calls.lock().unwrap().asked.len();
    rig.feed_own(vec![own_connected(1, T0 + 10)]);
    assert_eq!(rig.calls.lock().unwrap().asked.len(), before);
}

#[test]
fn cr_104_a_report_and_a_usable_node_give_the_own_node_tier() {
    let dir = Scratch::new();
    let (mut rig, tx) = rig_with_a_seen_sale(&dir);
    // Before the own node says anything: the light tier, over its limit.
    let screen = rig.node.judge(0).unwrap();
    assert_eq!(screen.state, State::Waiting);
    assert_eq!(screen.why, Why::Wait(WaitReason::OverSaleLimit));
    assert_eq!(screen.check_mode, Some(CheckMode::Light));
    assert!(!screen.own_node_saw);
    assert_eq!(rig.node.counter().exposure_outstanding(), 0);

    // The report alone, with the node not yet connected, is the fallback.
    rig.feed_own(vec![OwnNodeEvent::Has(txid(&tx))]);
    let screen = rig.node.judge(0).unwrap();
    assert_eq!(screen.state, State::Waiting);
    assert!(screen.own_node_saw && !screen.own_node_counted);
    assert_eq!(screen.check_line(), Some(CheckLine::LightOwnNodeUnusable));

    // The node connects: the sale is judged again and accepted at once.
    let reports = rig.feed_own(vec![own_connected(0, T0 + 10)]);
    assert_eq!(reports, vec![Report::PosPaid { index: 0 }]);
    let screen = rig.node.judge(0).unwrap();
    assert_eq!(screen.state, State::PaidInstant);
    assert_eq!(screen.why, Why::Instant(InstantLabel::YourNode));
    assert_eq!(screen.check_mode, Some(CheckMode::NodeAlsoSeenByLight));
    assert_eq!(rig.node.counter().exposure_outstanding(), 20_000);
    assert_eq!(
        rig.node.counter().sale(0).unwrap().accepted(),
        Some((InstantLabel::YourNode, CheckMode::NodeAlsoSeenByLight))
    );
}

#[test]
fn cr_104_the_own_node_alone_is_not_a_check() {
    let dir = Scratch::new();
    let mut rig = start_with_own_node(dir.path());
    rig.feed(vec![Rig::peers()]);
    rig.node
        .open(TWENTY, usd(), rate(), &source_at(&rig.time))
        .unwrap();
    let tx = payment();
    // Seen by one group only, and by the own node.
    rig.feed(vec![AppEvent::Sighting {
        tx: bytes(&tx),
        from: vec![peer(20)],
    }]);
    rig.time.store(T0 + 10, Ordering::SeqCst);
    rig.node.heartbeat(&hb(1, T0 + 10)).unwrap();
    rig.feed_own(vec![
        own_connected(0, T0 + 10),
        OwnNodeEvent::Has(txid(&tx)),
    ]);
    let screen = rig.node.judge(0).unwrap();
    assert_eq!(screen.state, State::Waiting);
    assert_eq!(screen.why, Why::NotSeenEnough);
    assert_eq!(screen.check_mode, None);
    assert_eq!(screen.check_line(), Some(CheckLine::OwnNodeWaitingForLight));
}

#[test]
fn cr_104_a_report_for_a_transaction_no_sale_knows_is_asked_again_after_the_sighting() {
    let dir = Scratch::new();
    let mut rig = start_with_own_node(dir.path());
    rig.feed(vec![Rig::peers()]);
    rig.node
        .open(TWENTY, usd(), rate(), &source_at(&rig.time))
        .unwrap();
    let tx = payment();
    // The own node announces the payment before the light client has seen it.
    rig.feed_own(vec![own_connected(0, T0), OwnNodeEvent::Has(txid(&tx))]);
    assert_eq!(rig.node.judge(0).unwrap().state, State::NotPaidYet);
    // The sighting makes the node ask, and the answer is taken.
    rig.feed(vec![seen_by_three(&tx)]);
    assert_eq!(rig.calls.lock().unwrap().asked, vec![txid(&tx)]);
    rig.feed_own(vec![OwnNodeEvent::Has(txid(&tx))]);
    let screen = rig.node.judge(0).unwrap();
    assert!(screen.own_node_saw);
}

#[test]
fn cr_104_a_node_that_goes_down_turns_the_sale_back_to_the_light_tier() {
    let dir = Scratch::new();
    let (mut rig, tx) = rig_with_a_seen_sale(&dir);
    rig.feed_own(vec![OwnNodeEvent::Has(txid(&tx)), own_down(T0 + 10)]);
    let screen = rig.node.judge(0).unwrap();
    assert_eq!(screen.state, State::Waiting);
    assert_eq!(screen.check_mode, Some(CheckMode::Light));
    assert!(screen.own_node_saw && !screen.own_node_counted);
    let health = rig.node.own_node_health();
    assert_eq!(
        health.problem,
        Some(OwnNodeProblem::Unreachable { since: T0 + 10 })
    );
    // A node that is silent for more than ten minutes is the same.
    rig.feed_own(vec![own_connected(0, T0 + 10)]);
    assert_eq!(rig.node.judge(0).unwrap().state, State::PaidInstant);
}

#[test]
fn cr_104_a_silent_node_is_not_used() {
    let dir = Scratch::new();
    let (mut rig, tx) = rig_with_a_seen_sale(&dir);
    rig.feed_own(vec![OwnNodeEvent::Has(txid(&tx))]);
    // Connected, heard at T0, and the clock has run more than ten minutes on.
    rig.time.store(T0 + 700, Ordering::SeqCst);
    rig.feed_own(vec![own_connected(0, T0)]);
    let screen = rig.node.judge(0).unwrap();
    assert_eq!(screen.state, State::Waiting);
    assert!(screen.own_node_saw && !screen.own_node_counted);
    assert_eq!(
        rig.node.own_node_health().problem,
        Some(OwnNodeProblem::Silent { since: T0 })
    );
}

#[test]
fn cr_107_the_health_line_and_try_again() {
    let dir = Scratch::new();
    let mut rig = start_with_own_node(dir.path());
    assert!(rig.node.own_node_configured());
    // Before the driver has said anything: not connected since the start.
    let health = rig.node.own_node_health();
    assert!(health.configured && !health.usable);
    assert_eq!(
        health.problem,
        Some(OwnNodeProblem::Unreachable { since: T0 })
    );
    assert!(health.words().is_some());
    rig.feed_own(vec![own_connected(0, T0)]);
    let health = rig.node.own_node_health();
    assert!(health.usable);
    assert_eq!(health.words().unwrap().heading, "Your node is checking too");
    // Try again goes to the driver.
    assert_eq!(rig.calls.lock().unwrap().retried, 0);
    rig.node.retry_own_node();
    assert_eq!(rig.calls.lock().unwrap().retried, 1);
}

#[test]
fn cr_108_without_an_own_node_nothing_is_asked_and_no_report_counts() {
    let dir = Scratch::new();
    let mut rig = start(dir.path());
    assert!(!rig.node.own_node_configured());
    assert!(rig.node.own_node_health().words().is_none());
    rig.feed(vec![Rig::peers()]);
    rig.node
        .open(TWENTY, usd(), rate(), &source_at(&rig.time))
        .unwrap();
    let tx = payment();
    rig.feed(vec![seen_by_three(&tx)]);
    rig.time.store(T0 + 10, Ordering::SeqCst);
    rig.node.heartbeat(&hb(1, T0 + 10)).unwrap();
    assert!(rig.calls.lock().unwrap().asked.is_empty());
    // A report that nobody should have sent, and a state, change nothing.
    rig.feed_own(vec![
        OwnNodeEvent::Has(txid(&tx)),
        own_connected(0, T0 + 10),
    ]);
    let screen = rig.node.judge(0).unwrap();
    assert_eq!(screen.state, State::Waiting);
    assert_eq!(screen.why, Why::Wait(WaitReason::OverSaleLimit));
    assert_eq!(screen.check_mode, Some(CheckMode::Light));
    rig.node.retry_own_node();
    assert_eq!(rig.calls.lock().unwrap().retried, 0);
}

#[test]
fn cr_103_the_node_is_generic_over_a_driver_without_an_own_node_channel() {
    // A port that keeps the default methods: no channel, asks go nowhere.
    struct Bare(openbmt_net_io::EventReceiver);
    impl crate::DriverPort for Bare {
        fn events(&self) -> &openbmt_net_io::EventReceiver {
            &self.0
        }
        fn watch_script(&self, _: bitcoin::ScriptBuf) {}
        fn set_all_blocks(&self, _: bool) {}
        fn shutdown(&mut self) {}
    }
    let dir = Scratch::new();
    let time = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(T0));
    let (_tx, rx) = std::sync::mpsc::channel();
    let mut node = Node::start_with(config_with_own_node(dir.path(), &time), move |_| {
        Ok(Bare(rx.into()))
    })
    .unwrap();
    assert!(node.pump(std::time::Duration::ZERO).is_empty());
    node.retry_own_node();
    assert!(!node.own_node_health().usable);
}
