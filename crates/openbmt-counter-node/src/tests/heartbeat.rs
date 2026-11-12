//! The owner's heartbeat through the node (CR-50 to CR-56), on a driver that
//! the test feeds by hand. The peers are fed as three network groups, so the
//! real instant decision is asked, with the real policy and no switch.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use bitcoin::hashes::Hash;
use bitcoin::Transaction;
use openbmt_core::heartbeat::{HeartbeatError, WatcherHeartbeat};
use openbmt_core::policy::{CheckMode, OffReason};
use openbmt_counter::{HeartbeatRefused, HeartbeatView, NotVerified, Report, State, Why};
use openbmt_net_io::AppEvent;

use super::kit::*;
use crate::HeartbeatNotTaken;

/// The node's chain starts at the regtest genesis block: height 0.
fn hb(seq: u64, time: u64) -> WatcherHeartbeat {
    WatcherHeartbeat {
        counter: id(3).device_id(),
        seq,
        time,
        tip_height: 0,
        tip_hash: genesis().to_byte_array(),
        healthy: true,
    }
}

fn owner_bytes(h: &WatcherHeartbeat) -> Vec<u8> {
    h.sign(&id(1)).to_bytes()
}

/// A rig with sale 0 paid and seen by three groups, and the clock ten
/// seconds on, past the conflict watch.
fn rig_with_a_seen_sale(dir: &Scratch) -> Rig {
    let mut rig = start(dir.path());
    assert!(rig.feed(vec![Rig::peers()]).is_empty());
    rig.node
        .open(five_dollars(), usd(), rate(), &source_at(&rig.time))
        .unwrap();
    let pay: Transaction = spend(1, &[(&script_of(0), 5_000)]);
    rig.feed(vec![AppEvent::Sighting {
        tx: bytes(&pay),
        from: vec![peer(20), peer(21), peer(22)],
    }]);
    rig.time.store(T0 + 10, Ordering::SeqCst);
    rig
}

#[test]
fn a_waiting_sale_is_accepted_when_the_first_heartbeat_is_taken() {
    let dir = Scratch::new();
    let mut rig = rig_with_a_seen_sale(&dir);
    let screen = rig.node.judge(0).unwrap();
    assert_eq!(screen.state, State::Waiting);
    assert_eq!(screen.why, Why::InstantOff(OffReason::NoWatcher));
    assert_eq!(screen.check_mode, Some(CheckMode::Light));
    assert_eq!(rig.node.counter().exposure_outstanding(), 0);

    let taken = rig.node.heartbeat(&owner_bytes(&hb(1, T0 + 10))).unwrap();
    assert_eq!(taken.view, HeartbeatView::Verified { sent_at: T0 + 10 });
    assert_eq!(taken.reports, vec![Report::PosPaid { index: 0 }]);
    assert_eq!(rig.node.counter().exposure_outstanding(), 5_000);
    let screen = rig.node.judge(0).unwrap();
    assert_eq!(screen.state, State::PaidInstant);
    assert!(screen.reports.is_empty(), "PosPaid is reported once");
}

#[test]
fn a_heartbeat_that_is_too_old_waits_again() {
    let dir = Scratch::new();
    let mut rig = rig_with_a_seen_sale(&dir);
    rig.node.heartbeat(&owner_bytes(&hb(1, T0 + 10))).unwrap();
    assert_eq!(rig.node.judge(0).unwrap().state, State::PaidInstant);

    // a second sale, seen after the heartbeat's age limit has run out
    let max = u64::from(settings().heartbeat_max_age_secs);
    rig.time.store(T0 + 10 + max + 1, Ordering::SeqCst);
    rig.node
        .open(five_dollars(), usd(), rate(), &source_at(&rig.time))
        .unwrap();
    let pay = spend(2, &[(&script_of(1), 5_000)]);
    rig.feed(vec![AppEvent::Sighting {
        tx: bytes(&pay),
        from: vec![peer(20), peer(21), peer(22)],
    }]);
    rig.time.fetch_add(10, Ordering::SeqCst);
    let screen = rig.node.judge(1).unwrap();
    assert_eq!(screen.state, State::Waiting);
    assert_eq!(screen.why, Why::InstantOff(OffReason::NoWatcher));
}

#[test]
fn what_is_not_a_good_heartbeat_is_refused_and_changes_nothing() {
    let dir = Scratch::new();
    let mut rig = rig_with_a_seen_sale(&dir);
    let good = owner_bytes(&hb(1, T0 + 10));

    // not a heartbeat at all, or cut short
    assert!(matches!(
        rig.node.heartbeat(b"not a heartbeat"),
        Err(HeartbeatNotTaken::Bytes(_))
    ));
    assert!(matches!(
        rig.node.heartbeat(&good[..good.len() - 1]),
        Err(HeartbeatNotTaken::Bytes(_))
    ));
    // signed by the Key app's root
    let by_approver = hb(1, T0 + 10).sign(&id(2)).to_bytes();
    assert_eq!(
        rig.node.heartbeat(&by_approver).unwrap_err(),
        HeartbeatNotTaken::Refused(HeartbeatRefused::Refused(
            HeartbeatError::OwnerSignatureInvalid
        ))
    );
    // naming another counter
    let mut other = hb(1, T0 + 10);
    other.counter = id(4).device_id();
    assert_eq!(
        rig.node.heartbeat(&owner_bytes(&other)).unwrap_err(),
        HeartbeatNotTaken::Refused(HeartbeatRefused::Refused(HeartbeatError::WrongCounter))
    );
    assert_eq!(rig.node.judge(0).unwrap().state, State::Waiting);

    // a good one is taken, and the same bytes again are a replay
    assert!(rig.node.heartbeat(&good).is_ok());
    assert_eq!(
        rig.node.heartbeat(&good).unwrap_err(),
        HeartbeatNotTaken::Refused(HeartbeatRefused::Refused(HeartbeatError::Stale {
            last: 1,
            got: 1
        }))
    );
    // a heartbeat from the future is refused
    let future = owner_bytes(&hb(2, T0 + 10 + 500));
    assert_eq!(
        rig.node.heartbeat(&future).unwrap_err(),
        HeartbeatNotTaken::Refused(HeartbeatRefused::FromTheFuture { ahead_secs: 500 })
    );
}

#[test]
fn an_unhealthy_heartbeat_takes_the_acceptance_away_and_is_logged() {
    let dir = Scratch::new();
    let mut rig = rig_with_a_seen_sale(&dir);
    rig.node.heartbeat(&owner_bytes(&hb(1, T0 + 10))).unwrap();
    assert_eq!(rig.node.judge(0).unwrap().state, State::PaidInstant);
    rig.node.take_notes();

    let mut sick = hb(2, T0 + 10);
    sick.healthy = false;
    let taken = rig.node.heartbeat(&owner_bytes(&sick)).unwrap();
    assert_eq!(
        taken.view,
        HeartbeatView::NotVerified(NotVerified::OwnerUnhealthy)
    );
    let notes = rig.node.take_notes();
    assert!(
        notes
            .iter()
            .any(|n| n.starts_with("heartbeat not verified")),
        "{notes:?}"
    );
    // accepted at once for good (CR-13): the green screen stays
    assert_eq!(rig.node.judge(0).unwrap().state, State::PaidInstant);
    assert!(!rig.node.counter().heartbeat_for_policy(T0 + 10).verified);
}

#[test]
fn a_tip_the_node_does_not_hold_is_a_disagreement_only_at_a_held_height() {
    let dir = Scratch::new();
    let mut rig = rig_with_a_seen_sale(&dir);
    rig.node.take_notes();

    // a block at height 1 that the node does not hold yet: not an alarm
    let mut ahead = hb(1, T0 + 10);
    ahead.tip_height = 1;
    ahead.tip_hash = [0x42; 32];
    let taken = rig.node.heartbeat(&owner_bytes(&ahead)).unwrap();
    assert_eq!(
        taken.view,
        HeartbeatView::NotVerified(NotVerified::TipNotYetHeld)
    );
    let notes = rig.node.take_notes();
    assert!(
        notes.iter().all(|n| !n.contains("disagreement")),
        "{notes:?}"
    );

    // another block at height 0, which the node holds
    let mut other = hb(2, T0 + 10);
    other.tip_hash = [0x43; 32];
    let taken = rig.node.heartbeat(&owner_bytes(&other)).unwrap();
    assert_eq!(
        taken.view,
        HeartbeatView::NotVerified(NotVerified::TipDisagrees)
    );
    let notes = rig.node.take_notes();
    assert!(
        notes
            .iter()
            .any(|n| n.starts_with("heartbeat disagreement")),
        "{notes:?}"
    );
    assert_eq!(rig.node.judge(0).unwrap().state, State::Waiting);
}

#[test]
fn after_a_restart_there_is_no_heartbeat_and_the_old_number_is_refused() {
    let dir = Scratch::new();
    let mut rig = rig_with_a_seen_sale(&dir);
    let first = owner_bytes(&hb(1, T0 + 10));
    rig.node.heartbeat(&first).unwrap();
    drop(rig);

    let mut rig = start_at(dir.path(), Arc::new(AtomicU64::new(T0 + 20)));
    assert_eq!(rig.node.counter().heartbeat_view(), None);
    assert_eq!(
        rig.node.heartbeat(&first).unwrap_err(),
        HeartbeatNotTaken::Refused(HeartbeatRefused::Refused(HeartbeatError::Stale {
            last: 1,
            got: 1
        }))
    );
    rig.feed(vec![Rig::peers()]);
    assert!(rig.node.heartbeat(&owner_bytes(&hb(2, T0 + 20))).is_ok());
    assert!(rig.node.counter().heartbeat_for_policy(T0 + 20).verified);
}

// WR-54.

#[test]
fn wr_54_behind_on_blocks_a_sale_still_opens_and_is_not_accepted_at_once() {
    let dir = Scratch::new();
    let mut rig = start(dir.path());
    let block = mine(genesis(), 1, 1, &[]);
    rig.feed(vec![Rig::peers(), joined(1, &block)]);
    rig.time.store(T0 + 16, Ordering::SeqCst);
    rig.feed(vec![]);
    assert_eq!(
        rig.node.health().unwrap().problem,
        Some(openbmt_watch::Problem::BlocksBehind)
    );
    // The sale opens, and a payment seen by three groups waits.
    rig.node
        .open(five_dollars(), usd(), rate(), &source_at(&rig.time))
        .unwrap();
    let pay: Transaction = spend(1, &[(&script_of(0), 5_000)]);
    rig.feed(vec![AppEvent::Sighting {
        tx: bytes(&pay),
        from: vec![peer(20), peer(21), peer(22)],
    }]);
    rig.time.store(T0 + 30, Ordering::SeqCst);
    let mut h = hb(1, T0 + 30);
    h.tip_height = 1;
    h.tip_hash = block.block_hash().to_byte_array();
    rig.node.heartbeat(&owner_bytes(&h)).unwrap();
    let screen = rig.node.judge(0).unwrap();
    assert_ne!(screen.state, State::PaidInstant);
    assert_eq!(rig.node.counter().exposure_outstanding(), 0);
}

#[test]
fn wr_54_headers_ahead_of_the_checked_height_keep_a_sale_from_being_accepted_at_once() {
    let dir = Scratch::new();
    let mut rig = rig_with_a_seen_sale(&dir);
    // The headers advance by a block whose body does not arrive: the checked
    // height stays at 0.
    let block = mine(genesis(), 1, 1, &[]);
    rig.feed(vec![joined(1, &block)]);
    let heartbeat = |seq: u64, time: u64| {
        let mut h = hb(seq, time);
        h.tip_height = 1;
        h.tip_hash = block.block_hash().to_byte_array();
        owner_bytes(&h)
    };
    // Inside the 15 seconds the check still works.
    rig.time.store(T0 + 10 + 15, Ordering::SeqCst);
    rig.feed(vec![]);
    assert!(rig.node.health().unwrap().healthy);
    // Past them it does not, and the sale waits for a confirmation.
    rig.time.store(T0 + 10 + 16, Ordering::SeqCst);
    rig.feed(vec![]);
    let health = rig.node.health().unwrap();
    assert!(!health.healthy);
    assert_eq!(health.problem, Some(openbmt_watch::Problem::BlocksBehind));
    rig.node.heartbeat(&heartbeat(1, T0 + 26)).unwrap();
    let screen = rig.node.judge(0).unwrap();
    assert_ne!(screen.state, State::PaidInstant);
    assert_eq!(
        screen.why,
        Why::CannotCheck(Some(openbmt_watch::Problem::BlocksBehind))
    );
    assert_eq!(rig.node.counter().exposure_outstanding(), 0);
    // The body arrives and the frontier moves: instant is open again.
    rig.feed(vec![AppEvent::Checked(1)]);
    assert!(rig.node.health().unwrap().healthy);
    assert_eq!(rig.node.judge(0).unwrap().state, State::PaidInstant);
}
