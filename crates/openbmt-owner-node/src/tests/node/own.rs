//! The owner node of a shop with its own wallet: the allocation book's
//! ranges are the issued ranges of OC-10, so the live watch, the set-aside
//! of quiet receive scripts and the deep check (OC-22 to OC-24) cover every
//! index of the book (OA-107). The bounds are small, as in `live.rs`.

use std::collections::BTreeSet;
use std::sync::atomic::Ordering;

use super::live::{pay_to_script, setup, DAY, START};
use super::*;
use crate::live::DeepLine;
use openbmt_core::wallet_single::ImportedWallet;
use openbmt_owner::own_book::{AllocationBook, RANGE_LEN};

/// The description of a single-key wallet, as a shop with its own wallet
/// holds it.
const SINGLE: &str = "wpkh([818bc9b6/84h/0h/0h]xpub6CgURS1GrKZkAoRGBTKYX9FhLgkKnQVZ6vdNBUbiu24xtRJ6RzLKpdg4CqsVyBLzimcJsviP59hSoGJEpTJiXE8bdBLw1qPC1DXRatKehb8/<0;1>/*)#96yh9yhx";

pub(super) fn single() -> ImportedWallet {
    ImportedWallet::read(SINGLE).unwrap()
}

pub(super) fn own_receive(index: u32) -> ScriptBuf {
    single().receive_script(index).unwrap()
}

fn counter() -> DeviceId {
    id(0x31).device_id()
}

impl Rig {
    /// The book gives `device` the next range at `at`, and the node is told.
    fn give(&mut self, book: &mut AllocationBook, device: DeviceId, at: u64) {
        book.allocate(device, at).unwrap();
        self.node.set_own_book(book).unwrap();
        self.rescan();
    }
}

fn own_setup(live: usize) -> (Setup, Arc<AtomicU64>) {
    let (mut setup, time) = setup(live);
    setup.own = Some(AllocationBook::new());
    (setup, time)
}

#[test]
fn oc_10_every_range_of_the_book_is_an_issued_range_and_every_index_in_it_is_watched() {
    let dir = Dir::new();
    let (setup, _time) = own_setup(10_000);
    let mut rig = start(&dir, &setup).unwrap();
    let mut book = AllocationBook::new();
    rig.give(&mut book, counter(), START);
    // Receive 0 to 999 and the gap of 20, and 20 change scripts.
    assert_eq!(rig.node.view().live_count(), 1_000 + 20 + 20);
    assert!(rig.node.scripts.contains_key(&own_receive(0)));
    assert!(rig.node.scripts.contains_key(&own_receive(999)));
    assert!(rig.node.scripts.contains_key(&own_receive(1_019)));
    assert!(!rig.node.scripts.contains_key(&own_receive(1_020)));
    // The desk's range joins at the same moment it is written.
    rig.give(&mut book, rig.node.owner().owner_id(), START + 5);
    assert_eq!(rig.node.view().live_count(), 2_000 + 20 + 20);
    assert!(rig.node.scripts.contains_key(&own_receive(1_999)));
    // The same book again changes nothing.
    let calls = rig.calls().len();
    rig.node.set_own_book(&book).unwrap();
    assert_eq!(rig.calls().len(), calls);
    assert_eq!(rig.node.view().live_count(), 2_040);
}

#[test]
fn oc_22_a_node_started_for_a_key_app_refuses_a_book() {
    let dir = Dir::new();
    let (setup, _time) = super::live::setup(100);
    let mut rig = start(&dir, &setup).unwrap();
    assert!(matches!(
        rig.node.set_own_book(&AllocationBook::new()),
        Err(NodeError::Config(_))
    ));
}

#[test]
fn oc_23_past_the_live_bound_only_quiet_receive_scripts_are_set_aside_oldest_issued_first() {
    let dir = Dir::new();
    // A bound of 2,040: two ranges, the gap and the change scripts fit.
    let (setup, time) = own_setup(2_040);
    let mut rig = start(&dir, &setup).unwrap();
    let mut book = AllocationBook::new();
    rig.give(&mut book, counter(), START);
    rig.give(&mut book, counter(), START + 10 * DAY);
    assert_eq!(rig.node.view().live_count(), 2_040);
    assert!(rig.released().is_empty());
    // A third range passes the bound, and nothing is quiet: the shop still
    // gets it, and the node keeps running.
    rig.give(&mut book, counter(), START + 20 * DAY);
    assert_eq!(rig.node.view().live_count(), 3_040);
    assert!(rig.released().is_empty());
    assert!(rig.node.halted().is_none());
    // 61 days after the first range: it is quiet, the others are not. The
    // oldest 1,000 receive scripts leave and the watch is at its bound.
    time.store(START + 61 * DAY, Ordering::SeqCst);
    rig.tick();
    assert_eq!(
        rig.released(),
        (0..RANGE_LEN).collect::<BTreeSet<u32>>(),
        "the first range, whole"
    );
    assert_eq!(rig.node.view().live_count(), 2_040);
    assert!(!rig.node.scripts.contains_key(&own_receive(0)));
    assert!(rig.node.scripts.contains_key(&own_receive(1_000)));
    // Every change script stays.
    for i in 0..20 {
        let change = single().change_script(i).unwrap();
        assert!(rig.node.scripts.contains_key(&change), "change {i}");
    }
    assert!(rig.node.old_addresses_line().is_some());
}

#[test]
fn oc_22_the_live_watch_of_a_restarted_own_wallet_node_leaves_the_set_aside_scripts_out() {
    let dir = Dir::new();
    let (mut setup, time) = own_setup(540);
    let mut book = AllocationBook::new();
    let chain = {
        let mut rig = start(&dir, &setup).unwrap();
        rig.give(&mut book, counter(), START);
        time.store(START + 61 * DAY, Ordering::SeqCst);
        rig.tick();
        assert_eq!(rig.released().len(), 500);
        rig.chain.clone()
    };
    setup.own = Some(book);
    let rig = start_at(&dir, &setup, chain).unwrap();
    assert_eq!(rig.released().len(), 500);
    assert_eq!(rig.node.scripts.len(), 540);
}

/// A node with the first range given at `START`, set aside down to a live
/// watch of 540, 61 days later: indexes 0 to 499 are out of the live watch.
fn aged(dir: &Dir) -> (Rig, Arc<AtomicU64>) {
    let (setup, time) = own_setup(540);
    let mut rig = start(dir, &setup).unwrap();
    let mut book = AllocationBook::new();
    rig.give(&mut book, counter(), START);
    time.store(START + 61 * DAY, Ordering::SeqCst);
    rig.tick();
    rig.mine_empty(1);
    (rig, time)
}

#[test]
fn oc_24_a_payment_to_an_index_set_aside_is_found_by_the_deep_check() {
    let dir = Dir::new();
    let (mut rig, time) = aged(&dir);
    assert_eq!(rig.released(), (0..500).collect::<BTreeSet<u32>>());
    let due = rig.node.view().deep().due;
    assert_eq!(
        rig.node.old_addresses_line(),
        Some(DeepLine::FirstCheckWaiting {
            count: 500,
            next: due
        })
    );
    // A payment to an index that was set aside is not seen by the live watch.
    rig.mine(&[pay_to_script(&own_receive(3), 7, 70_000)]);
    rig.mine_empty(6);
    assert_eq!(rig.node.balance(), 0);
    assert!(rig.node.view().receipts_at(3).is_empty());
    // A payment to one that is still watched is seen at once.
    rig.mine(&[pay_to_script(&own_receive(700), 8, 11_000)]);
    assert_eq!(rig.node.view().receipts_at(700).len(), 1);

    time.store(due, Ordering::SeqCst);
    rig.tick();
    let started = rig.deep_starts();
    assert_eq!(started.len(), 1);
    assert_eq!(started[0].scripts, 500);
    let from = started[0].watch_from;
    rig.deep_feed(from);
    assert!(!rig.node.deep_running());
    let receipts = rig.node.view().receipts_at(3);
    assert_eq!(receipts.len(), 1);
    assert_eq!(receipts[0].sats, 70_000);
    assert_eq!(rig.node.balance(), 81_000);
    assert!(!rig.released().contains(&3));
    assert!(rig.node.scripts.contains_key(&own_receive(3)));
    assert!(rig.node.halted().is_none());
}

#[test]
fn oa_113_the_paid_indexes_come_from_the_view_receipts_and_coins() {
    let dir = Dir::new();
    let (setup, _time) = own_setup(10_000);
    let mut rig = start(&dir, &setup).unwrap();
    let mut book = AllocationBook::new();
    rig.give(&mut book, counter(), START);
    rig.mine(&[
        pay_to_script(&own_receive(2), 7, 5_000),
        pay_to_script(&own_receive(40), 8, 6_000),
    ]);
    assert_eq!(
        rig.node.view().paid_receive_indexes(),
        BTreeSet::from([2, 40])
    );
}

#[test]
fn oc_22_a_range_that_would_not_fit_the_driver_is_not_offered_room() {
    let dir = Dir::new();
    // The driver's own limit is 100,000 scripts: with a quiet book a range
    // fits, with nothing quiet at the limit it does not.
    let (setup, _time) = own_setup(2_040);
    let mut rig = start(&dir, &setup).unwrap();
    let mut book = AllocationBook::new();
    rig.give(&mut book, counter(), START);
    assert!(rig.node.own_room_for(2_000));
    // 99,000 more receive indexes, none quiet: past 100,000 scripts.
    assert!(!rig.node.own_room_for(99_990));
}
