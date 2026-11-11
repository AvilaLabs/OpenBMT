//! The live watch and the deep check, on drivers that the test feeds by hand
//! (`docs/OWNER-COINS-RULES.md`, OC-22 to OC-27).

use std::collections::BTreeSet;
use std::sync::atomic::Ordering;

use super::*;
use crate::live::{DeepLine, DEEP_RETRY_SECS, DEEP_STALL_SECS};

const START: u64 = 1_700_000_000;
const DAY: u64 = 86_400;

fn limits(live: usize) -> WatchLimits {
    WatchLimits {
        live,
        quiet_secs: 60 * DAY,
        deep_secs: 30 * DAY,
    }
}

fn setup(live: usize) -> (Setup, Arc<AtomicU64>) {
    let time = Arc::new(AtomicU64::new(START));
    let setup = Setup {
        limits: limits(live),
        time: Some(time.clone()),
        ..Setup::default()
    };
    (setup, time)
}

fn pay_to(index: u32, from: u8, sats: u64) -> Transaction {
    tx_from(
        OutPoint::new(Txid::from_byte_array([from; 32]), 0),
        &[(&receive(index), sats)],
    )
}

impl Rig {
    /// Sign a batch at `at` on the owner's clock and let the node see it.
    fn issue(&mut self, start: u32, count: u32, at: u64) {
        let write = self.node.queue_writer();
        self.node
            .owner_mut()
            .unwrap()
            .queue_batch(batch(start, count), at, &write)
            .unwrap();
        self.node.refresh_marks();
        self.rescan();
    }

    /// One pass of the node's loop.
    fn tick(&mut self) {
        self.node.pump(Duration::ZERO);
        self.rescan();
    }

    fn released(&self) -> BTreeSet<u32> {
        self.node.view().released().iter().collect()
    }

    /// The indices of the deep check's drivers started so far.
    fn deep_starts(&self) -> Vec<Spawned> {
        self.spawned
            .lock()
            .unwrap()
            .iter()
            .filter(|s| s.dir.ends_with("deepcheck"))
            .cloned()
            .collect()
    }

    fn deep_sender(&self) -> Sender<AppEvent> {
        let spawned = self.spawned.lock().unwrap();
        let at = spawned
            .iter()
            .rposition(|s| s.dir.ends_with("deepcheck"))
            .expect("a deep driver was started");
        self.senders.lock().unwrap()[at].clone()
    }

    /// The deep driver hands over every block above `from` and says it has
    /// checked it, as a rescan does.
    fn deep_feed(&mut self, from: u32) {
        let tx = self.deep_sender();
        let blocks: Vec<(u32, Block)> = self
            .chain
            .iter()
            .enumerate()
            .map(|(i, b)| (u32::try_from(i).unwrap() + 1, b.clone()))
            .filter(|(h, _)| *h > from)
            .collect();
        for (height, block) in blocks {
            tx.send(AppEvent::Block {
                hash: block.block_hash(),
                bytes: serialize(&block),
            })
            .unwrap();
            tx.send(AppEvent::Checked(height)).unwrap();
        }
        self.tick();
    }

    fn count(&self, line: &str) -> usize {
        self.calls().iter().filter(|l| l.as_str() == line).count()
    }
}

/// A node with 60 receive addresses issued at `START`, all quiet by the end,
/// set aside down to a live watch of `live`. Returns the node and its clock.
fn aged(dir: &Dir, live: usize) -> (Rig, Arc<AtomicU64>) {
    let (setup, time) = setup(live);
    let mut rig = start(dir, &setup).unwrap();
    rig.issue(0, 60, START);
    time.store(START + 61 * DAY, Ordering::SeqCst);
    rig.tick();
    // A block above the height at which the scripts were set aside.
    rig.mine_empty(1);
    (rig, time)
}

// ---- OC-22, OC-23: what leaves the live watch --------------------------------------

#[test]
fn oc_22_below_the_bound_nothing_leaves() {
    let dir = Dir::new();
    // 60 issued, 20 more watched as the gap, 20 change: exactly 100.
    let (mut rig, time) = {
        let (setup, time) = setup(100);
        (start(&dir, &setup).unwrap(), time)
    };
    rig.issue(0, 60, START);
    time.store(START + 400 * DAY, Ordering::SeqCst);
    rig.tick();
    assert_eq!(rig.node.view().live_count(), 100);
    assert!(rig.released().is_empty());
    assert!(rig.node.old_addresses_line().is_none());
    assert!(!rig.calls().iter().any(|l| l.contains("unwatch")));
    assert_eq!(rig.node.view().deep().due, 0);
}

#[test]
fn oc_23_over_the_bound_only_quiet_scripts_leave_oldest_first_never_one_with_a_coin() {
    let dir = Dir::new();
    let (setup, time) = setup(60);
    let mut rig = start(&dir, &setup).unwrap();
    rig.issue(0, 30, START);
    rig.issue(30, 30, START + 10 * DAY);
    // Index 5 holds a coin: its spend must be seen, so it stays.
    rig.mine(&[pay_to(5, 7, 50_000)]);
    assert_eq!(rig.node.balance(), 50_000);
    assert_eq!(rig.node.view().live_count(), 100);

    // 61 days on: only the first batch is quiet. It all leaves but index 5,
    // and the live watch is still over its bound: nothing more is quiet.
    time.store(START + 61 * DAY, Ordering::SeqCst);
    rig.tick();
    let first: BTreeSet<u32> = (0..30).filter(|i| *i != 5).collect();
    assert_eq!(rig.released(), first);
    assert_eq!(rig.node.view().live_count(), 100 - 29);
    assert_eq!(rig.count("main unwatch 29"), 1);
    assert!(rig.node.scripts.contains_key(&receive(5)));
    assert!(!rig.node.scripts.contains_key(&receive(0)));
    // Every change script stays.
    for i in 0..20 {
        let change = kit().wallet.change_script(i).unwrap();
        assert!(rig.node.scripts.contains_key(&change), "change {i}");
    }

    // 71 days on: the second batch is quiet too, and leaves oldest first,
    // only as far as the bound needs: 11 of its 30.
    time.store(START + 71 * DAY, Ordering::SeqCst);
    rig.tick();
    let mut want = first;
    want.extend(30..41);
    assert_eq!(rig.released(), want);
    assert_eq!(rig.node.view().live_count(), 60);
    assert_eq!(rig.count("main unwatch 11"), 1);
    // The coin is still there, and still watched.
    assert_eq!(rig.node.balance(), 50_000);
    assert!(rig.node.scripts.contains_key(&receive(5)));
}

#[test]
fn oc_23_a_script_issued_but_not_yet_quiet_never_leaves_however_far_over_the_bound() {
    let dir = Dir::new();
    let (setup, time) = setup(40);
    let mut rig = start(&dir, &setup).unwrap();
    rig.issue(0, 60, START);
    time.store(START + 59 * DAY, Ordering::SeqCst);
    rig.tick();
    assert!(rig.released().is_empty());
    // The hard limit is the driver's, not the bound: the node still runs.
    assert!(rig.node.halted().is_none());
    assert_eq!(rig.node.view().live_count(), 100);
}

#[test]
fn oc_22_the_live_watch_of_a_restarted_node_leaves_the_set_aside_scripts_out() {
    let dir = Dir::new();
    let (setup, time) = setup(60);
    let chain = {
        let mut rig = start(&dir, &setup).unwrap();
        rig.issue(0, 60, START);
        time.store(START + 61 * DAY, Ordering::SeqCst);
        rig.tick();
        assert_eq!(rig.released().len(), 40);
        rig.chain.clone()
    };
    let rig = start_at(&dir, &setup, chain).unwrap();
    assert_eq!(rig.released().len(), 40);
    // 20 receive scripts and 20 of the gap, and 20 change scripts.
    assert_eq!(rig.spawned.lock().unwrap()[0].scripts, 60);
    assert_eq!(rig.node.scripts.len(), 60);
}

#[test]
fn oc_23_the_issue_time_is_the_owner_clock_of_the_batch_not_the_nodes_view_of_it() {
    // A batch signed long ago and seen by the node only now is quiet at
    // once: the time comes from the owner's record.
    let dir = Dir::new();
    let (setup, time) = setup(60);
    let mut rig = start(&dir, &setup).unwrap();
    time.store(START + 200 * DAY, Ordering::SeqCst);
    rig.issue(0, 60, START);
    assert_eq!(rig.released().len(), 40);
}

// ---- OC-24, OC-25: the deep check ---------------------------------------------------

#[test]
fn oc_24_a_payment_to_a_set_aside_script_is_found_becomes_a_receipt_and_the_script_returns() {
    let dir = Dir::new();
    let (mut rig, time) = aged(&dir, 60);
    assert_eq!(rig.released(), (0..40).collect::<BTreeSet<u32>>());
    let due = rig.node.view().deep().due;
    assert_eq!(due, START + 61 * DAY + 30 * DAY);
    assert_eq!(
        rig.node.old_addresses_line(),
        Some(DeepLine::FirstCheckWaiting {
            count: 40,
            next: due
        })
    );

    // A payment to a set aside script is not seen by the live watch.
    rig.mine(&[pay_to(3, 7, 70_000)]);
    rig.mine_empty(6);
    assert_eq!(rig.node.balance(), 0);
    assert!(rig.node.view().receipts_at(3).is_empty());

    // Before the check is due nothing starts.
    time.store(due - 1, Ordering::SeqCst);
    rig.tick();
    assert!(rig.deep_starts().is_empty());

    // At the due time the check starts, on a driver of its own, with the 40
    // scripts and from the height the view had when they were set aside.
    time.store(due, Ordering::SeqCst);
    let calls_before = rig.calls().len();
    rig.tick();
    let started = rig.deep_starts();
    assert_eq!(started.len(), 1);
    assert_eq!(started[0].scripts, 40);
    assert_eq!(started[0].watch_from, rig.node.view().deep().from);
    assert!(rig.node.deep_running());
    assert_eq!(
        rig.node.old_addresses_line(),
        Some(DeepLine::Checking { count: 40 })
    );
    // While it runs the live watch and the view are as they were.
    let checked = rig.node.view().checked();
    assert_eq!(rig.node.state(), ViewState::Ready);
    assert!(!rig.calls()[calls_before..]
        .iter()
        .any(|l| l.starts_with("main ") && l != "main fetch false"));

    // The driver reads the blocks and says it has checked up to the tip.
    let from = started[0].watch_from;
    assert_eq!(rig.node.view().checked(), checked);
    rig.deep_feed(from);
    assert!(!rig.node.deep_running());
    // A receipt, a coin, and the script back in the live watch.
    let receipts = rig.node.view().receipts_at(3);
    assert_eq!(receipts.len(), 1);
    assert_eq!(receipts[0].sats, 70_000);
    assert_eq!(rig.node.balance(), 70_000);
    assert!(!rig.released().contains(&3));
    assert_eq!(rig.released().len(), 39);
    assert!(rig.node.scripts.contains_key(&receive(3)));
    assert!(rig.node.halted().is_none());
    assert_eq!(rig.node.state(), ViewState::Ready);
    // The schedule: finished now, next in 30 days, checked up to the tip.
    let deep = rig.node.view().deep();
    assert_eq!(deep.last, due);
    assert_eq!(deep.due, due + 30 * DAY);
    assert!(!deep.failed);
    assert_eq!(deep.from, u32::try_from(rig.chain.len()).unwrap());
    assert_eq!(
        rig.node.old_addresses_line(),
        Some(DeepLine::Monthly {
            count: 39,
            days_ago: 0,
            next: due + 30 * DAY
        })
    );
}

#[test]
fn oc_24_a_coin_found_by_the_deep_check_is_spendable() {
    let dir = Dir::new();
    let (mut rig, time) = aged(&dir, 60);
    let pay = pay_to(3, 7, 300_000);
    rig.mine(std::slice::from_ref(&pay));
    rig.mine_empty(6);
    time.store(rig.node.view().deep().due, Ordering::SeqCst);
    rig.tick();
    let from = rig.deep_starts()[0].watch_from;
    rig.deep_feed(from);
    assert_eq!(rig.node.balance(), 300_000);

    // A refund of the sale on the counter's address 1 is signed by both
    // keys and takes the found coin.
    let write = rig.node.queue_writer();
    rig.node
        .owner_mut()
        .unwrap()
        .set_sale_facts(
            sale(1),
            SaleFacts {
                received_sats: 50_000,
                reached_refund_depth: true,
            },
            &write,
        )
        .unwrap();
    rig.node
        .owner_mut()
        .unwrap()
        .queue_refund(sale(1), "Mia", external(9), 10_000, None, &write)
        .unwrap();
    let txid = rig.spend();
    let tx = rig.tx(&txid);
    assert!(tx
        .input
        .iter()
        .any(|i| i.previous_output == OutPoint::new(pay.compute_txid(), 0)));
}

#[test]
fn oc_24_the_check_runs_on_request_and_only_when_something_is_set_aside() {
    let dir = Dir::new();
    let (setup, _time) = setup(60);
    let mut rig = start(&dir, &setup).unwrap();
    // Nothing is set aside: nothing to check.
    assert!(!rig.node.request_deep_check().unwrap());
    rig.tick();
    assert!(rig.deep_starts().is_empty());

    let other = Dir::new();
    let (mut rig, time) = aged(&other, 60);
    let due = rig.node.view().deep().due;
    assert!(due > time.load(Ordering::SeqCst));
    rig.tick();
    assert!(rig.deep_starts().is_empty());
    // The owner asks: it starts at once, though it is not due.
    assert!(rig.node.request_deep_check().unwrap());
    rig.tick();
    assert_eq!(rig.deep_starts().len(), 1);
    // Asking again while it runs changes nothing.
    assert!(rig.node.request_deep_check().unwrap());
    rig.tick();
    assert_eq!(rig.deep_starts().len(), 1);
}

#[test]
fn oc_24_passes_are_bounded_by_the_live_watch_and_a_payment_in_the_last_is_found() {
    let dir = Dir::new();
    // A bound of 20 scripts: the 60 set aside are checked 20 at a time.
    let (mut rig, time) = aged(&dir, 20);
    assert_eq!(rig.released().len(), 60);
    rig.mine(&[pay_to(50, 7, 80_000)]);
    rig.mine_empty(2);
    time.store(rig.node.view().deep().due, Ordering::SeqCst);
    rig.tick();
    let started = rig.deep_starts();
    assert_eq!(started.len(), 1);
    assert_eq!(started[0].scripts, 20);
    let from = started[0].watch_from;

    // Pass 1: scripts 0 to 19. Nothing is paid.
    rig.deep_feed(from);
    assert!(rig.node.deep_running());
    assert_eq!(rig.count("deep unwatch 20"), 1);
    assert_eq!(rig.count(&format!("deep rescan {from}")), 1);
    // The driver's old events come before it says the rescan began and are
    // not read: a block that pays script 50 is not applied by pass 2.
    let tx = rig.deep_sender();
    let late = rig.chain[0].clone();
    tx.send(AppEvent::Block {
        hash: late.block_hash(),
        bytes: serialize(&late),
    })
    .unwrap();
    tx.send(AppEvent::Checked(u32::try_from(rig.chain.len()).unwrap()))
        .unwrap();
    rig.tick();
    assert!(rig.node.deep_running());
    assert_eq!(
        rig.count("deep unwatch 20"),
        1,
        "the early Checked is stale"
    );
    tx.send(AppEvent::RescanStarted(from)).unwrap();
    rig.deep_feed(from);
    // Pass 2 is done; pass 3 holds script 50.
    assert!(rig.node.deep_running());
    assert_eq!(rig.count("deep unwatch 20"), 2);
    assert_eq!(rig.node.balance(), 0);
    tx.send(AppEvent::RescanStarted(from)).unwrap();
    rig.deep_feed(from);
    assert!(!rig.node.deep_running());
    assert_eq!(rig.node.balance(), 80_000);
    assert_eq!(rig.released().len(), 59);
    assert!(!rig.released().contains(&50));
    assert!(rig.node.scripts.contains_key(&receive(50)));
    assert_eq!(rig.node.view().deep().last, time.load(Ordering::SeqCst));
}

#[test]
fn oc_26_a_check_that_fails_keeps_the_set_and_is_tried_again() {
    let dir = Dir::new();
    let (mut rig, time) = aged(&dir, 60);
    rig.mine(&[pay_to(3, 7, 70_000)]);
    rig.mine_empty(2);
    let due = rig.node.view().deep().due;
    time.store(due, Ordering::SeqCst);
    rig.tick();
    assert!(rig.node.deep_running());
    // The driver stops.
    rig.deep_sender().send(AppEvent::Stopped).unwrap();
    rig.tick();
    assert!(!rig.node.deep_running());
    assert_eq!(rig.released().len(), 40, "no script is lost");
    let deep = rig.node.view().deep();
    assert!(deep.failed);
    assert_eq!(deep.due, due + DEEP_RETRY_SECS);
    assert_eq!(deep.last, 0);
    assert_eq!(
        rig.node.old_addresses_line(),
        Some(DeepLine::Failed {
            count: 40,
            next: due + DEEP_RETRY_SECS
        })
    );
    assert!(rig.node.halted().is_none());
    // Not before the pause.
    time.store(due + DEEP_RETRY_SECS - 1, Ordering::SeqCst);
    rig.tick();
    assert_eq!(rig.deep_starts().len(), 1);
    // After it, a second driver, and this time the check finishes.
    time.store(due + DEEP_RETRY_SECS, Ordering::SeqCst);
    rig.tick();
    assert_eq!(rig.deep_starts().len(), 2);
    let from = rig.deep_starts()[1].watch_from;
    rig.deep_feed(from);
    assert!(!rig.node.deep_running());
    assert_eq!(rig.node.balance(), 70_000);
    assert!(!rig.node.view().deep().failed);
    assert_eq!(rig.node.view().deep().last, due + DEEP_RETRY_SECS);
}

#[test]
fn oc_26_a_driver_that_makes_no_progress_fails_the_check() {
    let dir = Dir::new();
    let (mut rig, time) = aged(&dir, 60);
    let due = rig.node.view().deep().due;
    time.store(due, Ordering::SeqCst);
    rig.tick();
    assert!(rig.node.deep_running());
    time.store(due + DEEP_STALL_SECS, Ordering::SeqCst);
    rig.tick();
    assert!(
        rig.node.deep_running(),
        "an hour of silence is not yet a stall"
    );
    time.store(due + DEEP_STALL_SECS + 1, Ordering::SeqCst);
    rig.tick();
    assert!(!rig.node.deep_running());
    assert!(rig.node.view().deep().failed);
    assert_eq!(rig.released().len(), 40);
}

#[test]
fn oc_26_a_driver_that_cannot_start_fails_the_check_and_loses_no_script() {
    let dir = Dir::new();
    let (setup, time) = setup(60);
    let mut rig = start(&dir, &setup).unwrap();
    rig.issue(0, 60, START);
    time.store(START + 61 * DAY, Ordering::SeqCst);
    rig.tick();
    rig.mine_empty(1);
    // Something else holds the deep check's directory.
    let held = crate::watcher::OwnerWatcher::<FakePort>::start_with(
        OwnerConfig::new(
            dir.path().join("deepcheck"),
            id(0x11),
            DriverConfig::new(NET, 0),
        ),
        |_, _| {
            let (_tx, rx) = channel();
            Ok(FakePort {
                rx: rx.into(),
                log: Arc::default(),
                tag: "held",
            })
        },
    )
    .unwrap();
    time.store(rig.node.view().deep().due, Ordering::SeqCst);
    rig.tick();
    assert!(!rig.node.deep_running());
    assert!(rig.node.view().deep().failed);
    assert_eq!(rig.released().len(), 40);
    drop(held);
}

#[test]
fn oc_27_a_restart_in_the_middle_loses_no_script_and_the_check_starts_again() {
    let dir = Dir::new();
    let (setup, time) = setup(60);
    let (chain, released) = {
        let mut rig = start(&dir, &setup).unwrap();
        rig.issue(0, 60, START);
        time.store(START + 61 * DAY, Ordering::SeqCst);
        rig.tick();
        rig.mine(&[pay_to(3, 7, 70_000)]);
        rig.mine_empty(2);
        time.store(rig.node.view().deep().due, Ordering::SeqCst);
        rig.tick();
        assert!(rig.node.deep_running());
        // The node goes away with the check half done.
        (rig.chain.clone(), rig.released())
    };
    let mut rig = start_at(&dir, &setup, chain).unwrap();
    assert_eq!(rig.released(), released);
    assert!(rig.node.view().deep().due <= time.load(Ordering::SeqCst));
    assert_eq!(rig.node.balance(), 0);
    // It starts again by itself, from the stored height, and finds the
    // payment.
    assert!(rig.node.deep_running());
    let from = rig.deep_starts()[0].watch_from;
    assert_eq!(from, rig.node.view().deep().from);
    rig.deep_feed(from);
    assert_eq!(rig.node.balance(), 70_000);
    assert!(!rig.released().contains(&3));
}

#[test]
fn oc_27_a_restart_between_a_find_and_its_return_puts_the_script_back() {
    // The coin was stored; the script was still set aside when the node went
    // away. The next start puts it back in the live watch.
    let dir = Dir::new();
    let (setup, time) = setup(60);
    let chain = {
        let mut rig = start(&dir, &setup).unwrap();
        rig.issue(0, 60, START);
        time.store(START + 61 * DAY, Ordering::SeqCst);
        rig.tick();
        rig.mine(&[pay_to(3, 7, 70_000)]);
        rig.mine_empty(2);
        rig.chain.clone()
    };
    // Write the view as that crash would leave it: the receipt and the coin
    // of index 3 are there, and index 3 is still set aside.
    let path = dir.path().join(COINS_FILE);
    let mut view = CoinView::from_bytes(&fs::read(&path).unwrap()).unwrap();
    assert!(view.released().contains(3));
    let block = &chain[0];
    let mut scripts = crate::coins::ScriptSet::new();
    scripts.insert(receive(3), (openbmt_core::wallet::Keychain::Receive, 3));
    let store = chain_view(&chain);
    let parsed = openbmt_watch::WellFormedBlock::parse(&serialize(block)).unwrap();
    view.apply_block(&parsed, &store, &scripts, |_| Ok::<(), ()>(()))
        .unwrap();
    fs::write(&path, view.to_bytes()).unwrap();
    let rig = start_at(&dir, &setup, chain).unwrap();
    assert!(!rig.released().contains(&3));
    assert!(rig.node.scripts.contains_key(&receive(3)));
    assert_eq!(rig.node.balance(), 70_000);
}

/// The best chain of a test, for `apply_block`.
fn chain_view(blocks: &[Block]) -> openbmt_counter_node::ChainView {
    let store = openbmt_watch::HeaderStore::new(NET).unwrap();
    let mut view = openbmt_counter_node::ChainView::from_store(&store);
    let joined: Vec<(u32, BlockHash)> = blocks
        .iter()
        .enumerate()
        .map(|(i, b)| (u32::try_from(i).unwrap() + 1, b.block_hash()))
        .collect();
    view.apply(&joined, &[]);
    view
}

#[test]
fn oc_24_a_change_of_the_chain_stops_a_running_check_without_failing_it() {
    let dir = Dir::new();
    let (mut rig, time) = aged(&dir, 60);
    rig.mine_empty(3);
    time.store(rig.node.view().deep().due, Ordering::SeqCst);
    rig.tick();
    assert!(rig.node.deep_running());
    let left = rig.chain.pop().unwrap().block_hash();
    rig.feed(vec![AppEvent::Chain {
        joined: vec![],
        left: vec![left],
    }]);
    // It is not a failure: the check starts again at once, on a new driver,
    // as the view has caught up with the shorter chain.
    assert!(!rig.node.view().deep().failed);
    assert_eq!(rig.deep_starts().len(), 2);
    assert_eq!(rig.released().len(), 40);
}

#[test]
fn oc_24_a_rescan_from_the_birthday_has_the_deep_check_due_from_the_birthday() {
    let dir = Dir::new();
    let (mut rig, time) = aged(&dir, 60);
    rig.mine_empty(3);
    time.store(rig.node.view().deep().due, Ordering::SeqCst);
    rig.tick();
    let from = rig.deep_starts()[0].watch_from;
    rig.deep_feed(from);
    assert!(rig.node.view().deep().due > time.load(Ordering::SeqCst));
    rig.node.rescan().unwrap();
    let deep = rig.node.view().deep();
    assert_eq!(deep.from, 0);
    assert!(deep.due <= time.load(Ordering::SeqCst));
    assert_eq!(rig.released().len(), 40);
}
