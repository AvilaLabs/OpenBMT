//! The node settles old sales (CR-82): when it does, how often, and what it
//! says.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use bitcoin::hashes::Hash;
use bitcoin::{ScriptBuf, WScriptHash};
use openbmt_core::batch::{AddressBatch, SignedBatch};
use openbmt_counter::SETTLE_SECS;

use super::kit::*;
use crate::{Node, SETTLE_EVERY_SECS, SETTLE_MIN_GAP_SECS};

fn wide_script(i: u32) -> ScriptBuf {
    let mut bytes = [0x5a; 32];
    bytes[..4].copy_from_slice(&i.to_le_bytes());
    ScriptBuf::new_p2wsh(&WScriptHash::from_byte_array(bytes))
}

fn wide_batch(count: u32) -> SignedBatch {
    let batch = AddressBatch {
        wallet_checksum: [9; 32],
        counter: id(3).device_id(),
        start: 0,
        count,
        scripts: (0..count).map(wide_script).collect(),
    };
    SignedBatch {
        owner_sig: batch.sign(&id(1)),
        approver_sig: batch.sign(&id(2)),
        batch,
    }
}

fn settle_notes(rig: &mut Rig) -> Vec<String> {
    rig.node
        .take_notes()
        .into_iter()
        .filter(|n| n.contains("CR-77"))
        .collect()
}

fn at(rig: &Rig, time: u64) {
    rig.time.store(time, Ordering::SeqCst);
}

fn pump(rig: &mut Rig) {
    rig.feed(vec![]);
}

/// A rig with the health answered and `n` sales opened and cancelled, a
/// minute apart.
fn rig_with_ended_sales(dir: &Scratch, n: u32) -> Rig {
    let time = Arc::new(AtomicU64::new(T0));
    let mut config = config(dir.path(), &time);
    config.first_batch = Some(wide_batch(n.max(10)));
    let mut rig = start_with(config, time);
    rig.feed(vec![Rig::peers()]);
    for i in 0..n {
        let opened = rig
            .node
            .open(five_dollars(), usd(), rate(), &source_at(&rig.time))
            .unwrap();
        assert_eq!(opened.index, i);
        rig.node.cancel(i).unwrap();
        // Well inside the hour's allowance of addresses (CR-04).
        rig.time.fetch_add(60, Ordering::SeqCst);
    }
    rig
}

fn owner_holds_all(node: &mut Node<FakePort>) {
    let all = node.records_len();
    node.set_owner_mark(all);
}

#[test]
fn cr_82_the_node_settles_only_after_the_owner_s_mark_is_known() {
    let dir = Scratch::new();
    let mut rig = rig_with_ended_sales(&dir, 2);
    at(&rig, T0 + 2 * SETTLE_SECS);
    pump(&mut rig);
    assert!(settle_notes(&mut rig).is_empty());
    assert_eq!(rig.node.counter().held_sales(), 2);

    owner_holds_all(&mut rig.node);
    pump(&mut rig);
    // Count only: no index and no address.
    assert_eq!(
        settle_notes(&mut rig),
        vec!["CR-77: settled 2 sales and released 2 addresses"]
    );
    assert_eq!(rig.node.counter().held_sales(), 0);
}

#[test]
fn cr_82_a_node_that_settles_nothing_says_nothing() {
    let dir = Scratch::new();
    let mut rig = rig_with_ended_sales(&dir, 1);
    owner_holds_all(&mut rig.node);
    pump(&mut rig);
    assert!(settle_notes(&mut rig).is_empty());
    assert_eq!(rig.node.counter().held_sales(), 1);
}

#[test]
fn cr_82_never_twice_inside_a_minute_and_the_rest_a_minute_later() {
    let dir = Scratch::new();
    let mut rig = rig_with_ended_sales(&dir, 300);
    let start = T0 + 300 * 60 + SETTLE_SECS;
    at(&rig, start);
    owner_holds_all(&mut rig.node);
    pump(&mut rig);
    let notes = settle_notes(&mut rig);
    assert_eq!(notes.len(), 1, "{notes:?}");
    assert!(notes[0].contains("settled 256 sales"), "{notes:?}");
    assert_eq!(rig.node.counter().held_sales(), 44);
    // Same moment, and one second short of the minute: nothing.
    pump(&mut rig);
    at(&rig, start + SETTLE_MIN_GAP_SECS - 1);
    pump(&mut rig);
    assert!(settle_notes(&mut rig).is_empty());
    assert_eq!(rig.node.counter().held_sales(), 44);
    // The minute: the rest, since sales were left.
    at(&rig, start + SETTLE_MIN_GAP_SECS);
    pump(&mut rig);
    let notes = settle_notes(&mut rig);
    assert_eq!(notes.len(), 1, "{notes:?}");
    assert!(notes[0].contains("settled 44 sales"), "{notes:?}");
    assert_eq!(rig.node.counter().held_sales(), 0);
}

#[test]
fn cr_82_after_that_once_an_hour() {
    let dir = Scratch::new();
    let mut rig = rig_with_ended_sales(&dir, 1);
    owner_holds_all(&mut rig.node);
    let t1 = T0 + 60 + SETTLE_SECS;
    at(&rig, t1);
    pump(&mut rig);
    assert_eq!(settle_notes(&mut rig).len(), 1);

    // A second sale that ends at t1 and is ready a month later.
    let opened = rig
        .node
        .open(five_dollars(), usd(), rate(), &source_at(&rig.time))
        .unwrap();
    rig.node.cancel(opened.index).unwrap();
    owner_holds_all(&mut rig.node);
    let ready = t1 + SETTLE_SECS;
    // An attempt half an hour before it is ready finds nothing and counts as
    // the hour's attempt.
    at(&rig, ready - SETTLE_EVERY_SECS / 2);
    pump(&mut rig);
    assert!(settle_notes(&mut rig).is_empty());
    at(&rig, ready);
    pump(&mut rig);
    assert!(settle_notes(&mut rig).is_empty(), "the hour is not up");
    assert_eq!(rig.node.counter().held_sales(), 1);
    at(&rig, ready + SETTLE_EVERY_SECS / 2);
    pump(&mut rig);
    assert_eq!(settle_notes(&mut rig).len(), 1);
    assert_eq!(rig.node.counter().held_sales(), 0);
}

#[test]
fn cr_82_the_next_start_hands_the_driver_the_shorter_list() {
    let dir = Scratch::new();
    let mut rig = rig_with_ended_sales(&dir, 3);
    // A fourth sale that is still open.
    let open = rig
        .node
        .open(five_dollars(), usd(), rate(), &source_at(&rig.time))
        .unwrap();
    assert_eq!(rig.node.counter().watched_scripts().len(), 4);
    at(&rig, T0 + 3 * 60 + SETTLE_SECS);
    owner_holds_all(&mut rig.node);
    pump(&mut rig);
    assert_eq!(rig.node.counter().held_sales(), 1);
    let time = rig.time.clone();
    drop(rig);

    let again = start_at(dir.path(), time);
    assert_eq!(again.launch.scripts, vec![open.script.clone()]);
    assert_eq!(again.node.counter().held_sales(), 1);
    // Nothing is settled in the new run until the owner's mark comes.
    assert_eq!(again.node.counter().owner_mark(), None);
}
