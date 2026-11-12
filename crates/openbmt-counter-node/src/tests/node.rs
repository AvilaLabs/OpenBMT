//! `Node` against a driver that the test feeds by hand.

use std::fs;
use std::sync::atomic::AtomicU64;
use std::sync::Arc;
use std::time::{Duration, Instant};

use bitcoin::hashes::Hash;
use bitcoin::Transaction;
use openbmt_core::policy::{CheckMode, OffReason};
use openbmt_counter::{Amount, OpenRefused, Report, RequestDays, SaleExtras, State, Store, Why};
use openbmt_net_io::AppEvent;
use openbmt_watch::{check_health, HeaderChain, Peer, PeerTransport, Problem};

use super::kit::*;
use crate::{DirStore, Node, StartError, NOTE_LOG, TX_CACHE};

fn sighting(tx: &Transaction, from: Vec<openbmt_watch::PeerAddr>) -> AppEvent {
    AppEvent::Sighting {
        tx: bytes(tx),
        from,
    }
}

/// A rig with the first sale open and the health answered.
fn rig_with_a_sale(dir: &Scratch) -> (Rig, Transaction) {
    let mut rig = start(dir.path());
    assert!(rig.feed(vec![Rig::peers()]).is_empty());
    let opened = rig
        .node
        .open(five_dollars(), usd(), rate(), &source_at(&rig.time))
        .unwrap();
    assert_eq!(opened.index, 0);
    assert_eq!(opened.needs_sats, 5_000);
    let pay = spend(1, &[(&script_of(0), 5_000)]);
    (rig, pay)
}

#[test]
fn a_sale_goes_from_not_paid_to_waiting_to_confirmed_to_final() {
    let dir = Scratch::new();
    let (mut rig, pay) = rig_with_a_sale(&dir);
    // The address is watched before the customer sees it.
    assert_eq!(rig.calls.lock().unwrap().watched, vec![script_of(0)]);
    let screen = rig.node.judge(0).unwrap();
    assert_eq!(screen.state, State::NotPaidYet);
    assert!(screen.reports.is_empty());
    assert!(rig.all_blocks().is_empty());

    // Seen by one group: waiting, and every new block is downloaded.
    assert!(rig.feed(vec![sighting(&pay, vec![peer(20)])]).is_empty());
    let screen = rig.node.judge(0).unwrap();
    assert_eq!(screen.state, State::Waiting);
    assert_eq!(screen.why, Why::NotSeenEnough);
    assert_eq!(screen.check_mode, None);
    assert_eq!(screen.received_sats, 5_000);
    assert!(screen.reports.is_empty());
    assert_eq!(rig.all_blocks(), vec![true]);

    // One block with the payment: confirmed, the POS is told once, and the
    // blocks need not all be downloaded any more.
    let mut blocks = Blocks::default();
    let first = blocks.extend(std::slice::from_ref(&pay)).clone();
    let reports = rig.feed(vec![joined(1, &first), block_event(&first)]);
    assert_eq!(reports, vec![Report::PosPaid { index: 0 }]);
    let screen = rig.node.judge(0).unwrap();
    assert_eq!(screen.state, State::Confirmed);
    assert_eq!(screen.why, Why::ConfirmedAt(1));
    assert!(screen.reports.is_empty());
    assert_eq!(rig.all_blocks(), vec![true, false]);

    // Four more blocks: still confirmed, nothing reported.
    let mut events = Vec::new();
    for height in 2..=5 {
        let block = blocks.extend(&[]).clone();
        events.push(joined(height, &block));
    }
    assert!(rig.feed(events).is_empty());
    assert_eq!(rig.node.judge(0).unwrap().state, State::Confirmed);

    // The sixth: final, and the books are told once.
    let block = blocks.extend(&[]).clone();
    let reports = rig.feed(vec![joined(6, &block)]);
    assert_eq!(
        reports,
        vec![Report::BooksFinal {
            index: 0,
            amount: five_dollars(),
            currency: usd(),
            received_sats: 5_000,
        }]
    );
    let screen = rig.node.judge(0).unwrap();
    assert_eq!(screen.state, State::Final);
    assert!(screen.reports.is_empty());
    assert!(rig
        .feed(vec![AppEvent::Peers(vec![], None, (6, 0))])
        .is_empty());
    assert_eq!(rig.all_blocks(), vec![true, false]);
}

#[test]
fn a_reorganization_before_the_final_depth_turns_the_sale_red() {
    let dir = Scratch::new();
    let (mut rig, pay) = rig_with_a_sale(&dir);
    rig.feed(vec![sighting(&pay, vec![peer(20)])]);
    let mut blocks = Blocks::default();
    let first = blocks.extend(std::slice::from_ref(&pay)).clone();
    let reports = rig.feed(vec![joined(1, &first), block_event(&first)]);
    assert_eq!(reports, vec![Report::PosPaid { index: 0 }]);
    assert_eq!(rig.all_blocks(), vec![true, false]);

    // Two blocks on another branch take the chain: the block with the
    // payment is gone.
    let alt1 = mine(genesis(), 1, 1, &[]);
    let alt2 = mine(alt1.block_hash(), 2, 1, &[]);
    let reorg = AppEvent::Chain {
        joined: vec![(1, alt1.block_hash()), (2, alt2.block_hash())],
        left: vec![first.block_hash()],
    };
    let reports = rig.feed(vec![reorg]);
    assert_eq!(reports, vec![Report::PosUnpaid { index: 0 }]);
    let screen = rig.node.judge(0).unwrap();
    assert_eq!(screen.state, State::DoNotHandOver);
    assert_eq!(screen.why, Why::Reopened);
    assert!(screen.reopened);
    assert!(screen.reports.is_empty());
    // The payment is unconfirmed again: every new block is wanted.
    assert_eq!(rig.all_blocks(), vec![true, false, true]);
    // Another pass changes nothing and reports nothing.
    assert!(rig.feed(vec![]).is_empty());

    // Mined again on the new branch: green again, and told again.
    let again = mine(alt2.block_hash(), 3, 1, &[pay]);
    let reports = rig.feed(vec![joined(3, &again), block_event(&again)]);
    assert_eq!(reports, vec![Report::PosPaid { index: 0 }]);
    assert_eq!(rig.node.judge(0).unwrap().state, State::Confirmed);
    assert_eq!(rig.all_blocks(), vec![true, false, true, false]);
}

#[test]
fn a_restarted_node_shows_the_same_states_and_reports_nothing_again() {
    let dir = Scratch::new();
    let (mut rig, pay) = rig_with_a_sale(&dir);
    let second = rig
        .node
        .open(five_dollars(), usd(), rate(), &source_at(&rig.time))
        .unwrap();
    assert_eq!(second.index, 1);
    let pay_second = spend(2, &[(&script_of(1), 5_000)]);
    rig.feed(vec![
        sighting(&pay, vec![peer(20)]),
        sighting(&pay_second, vec![peer(20)]),
    ]);
    let mut blocks = Blocks::default();
    let first = blocks.extend(std::slice::from_ref(&pay)).clone();
    let reports = rig.feed(vec![
        joined(1, &first),
        block_event(&first),
        AppEvent::Checked(1),
    ]);
    assert_eq!(reports, vec![Report::PosPaid { index: 0 }]);
    assert_eq!(rig.node.judge(0).unwrap().state, State::Confirmed);
    assert_eq!(rig.node.judge(1).unwrap().state, State::Waiting);
    assert_eq!(rig.node.checked_height(), 1);

    // The driver saves its headers as it stops; the node stops with it.
    fs::write(dir.path().join("headers.bin"), blocks.stored_headers()).unwrap();
    assert!(rig.node.shutdown().is_empty());
    assert!(rig.calls.lock().unwrap().shutdown);
    // The directory is the next node's once this one is gone (NR-79).
    drop(rig);

    let mut again = start(dir.path());
    // The driver starts after the blocks that were checked, and watches the
    // addresses that were shown.
    assert_eq!(again.launch.driver.watch_from, 1);
    assert_eq!(again.launch.scripts.len(), 2);
    assert!(again.launch.scripts.contains(&script_of(0)));
    assert!(again.launch.scripts.contains(&script_of(1)));
    assert_eq!(again.node.checked_height(), 1);
    assert_eq!(again.node.chain().tip_height(), 1);
    // The payment still waits for its sale: every block is wanted.
    assert_eq!(again.all_blocks(), vec![true]);
    let screen = again.node.judge(0).unwrap();
    assert_eq!(screen.state, State::Confirmed);
    assert!(screen.reports.is_empty());
    assert!(again.feed(vec![Rig::peers()]).is_empty());
    for (index, state) in [(0, State::Confirmed), (1, State::Waiting)] {
        let screen = again.node.judge(index).unwrap();
        assert_eq!(screen.state, state);
        assert!(screen.reports.is_empty());
    }
    assert_eq!(again.node.sales().collect::<Vec<_>>(), vec![0, 1]);

    // The next address is the third: none is used twice.
    let third = again
        .node
        .open(five_dollars(), usd(), rate(), &source_at(&again.time))
        .unwrap();
    assert_eq!(third.index, 2);

    // Five more blocks: the books are told, by the new node, once.
    let mut events = Vec::new();
    for height in 2..=6 {
        let block = blocks.extend(&[]).clone();
        events.push(joined(height, &block));
    }
    let reports = again.feed(events);
    assert_eq!(reports.len(), 1);
    assert!(matches!(reports[0], Report::BooksFinal { index: 0, .. }));
}

#[test]
fn announcements_add_groups_only_for_cached_transactions() {
    let dir = Scratch::new();
    let (mut rig, pay) = rig_with_a_sale(&dir);
    let txid = pay.compute_txid();
    // A transaction that pays no sale is not kept.
    let nothing = spend(9, &[(&p2wsh(0x55), 100)]);
    rig.feed(vec![sighting(&nothing, vec![peer(40)])]);
    assert_eq!(rig.node.cached_transactions(), 0);
    rig.feed(vec![sighting(&pay, vec![peer(20)])]);
    assert_eq!(rig.node.cached_transactions(), 1);
    assert!(rig.node.is_cached(&txid));
    assert_eq!(rig.node.judge(0).unwrap().check_mode, None);

    // Announced again by a peer the node never saw it from: one more group.
    // Announced by a transaction it kept nothing of: nothing.
    rig.feed(vec![
        AppEvent::AlsoAnnounced {
            txid: nothing.compute_txid(),
            from: peer(41),
        },
        AppEvent::AlsoAnnounced {
            txid: bitcoin::Txid::from_byte_array([9; 32]),
            from: peer(42),
        },
        AppEvent::AlsoAnnounced {
            txid,
            from: peer(21),
        },
    ]);
    assert_eq!(rig.node.judge(0).unwrap().check_mode, None);
    assert_eq!(rig.node.cached_transactions(), 1);
    rig.feed(vec![AppEvent::AlsoAnnounced {
        txid,
        from: peer(22),
    }]);
    // Three groups have seen it. Even so it is not accepted at once: no
    // owner heartbeat has been taken.
    let screen = rig.node.judge(0).unwrap();
    assert_eq!(screen.check_mode, Some(CheckMode::Light));
    assert_eq!(screen.state, State::Waiting);
    assert_eq!(screen.why, Why::InstantOff(OffReason::NoWatcher));
    assert_eq!(rig.node.counter().exposure_outstanding(), 0);
}

#[test]
fn a_sighting_counts_once_for_every_peer_it_came_from() {
    let dir = Scratch::new();
    let (mut rig, pay) = rig_with_a_sale(&dir);
    rig.feed(vec![sighting(
        &pay,
        vec![peer(20), peer(21), peer(22), peer(22)],
    )]);
    let screen = rig.node.judge(0).unwrap();
    assert_eq!(screen.check_mode, Some(CheckMode::Light));
    assert_eq!(screen.state, State::Waiting);
    assert_eq!(rig.node.cached_transactions(), 1);
}

#[test]
fn nr_54_a_transaction_offered_again_as_a_sighting_from_counted_groups_changes_nothing() {
    let dir = Scratch::new();
    let (mut rig, pay) = rig_with_a_sale(&dir);
    let from = vec![peer(20), peer(21), peer(22)];
    rig.feed(vec![sighting(&pay, from.clone())]);
    let before = rig.node.judge(0).unwrap();
    // The network crate forgot the ID and fetched the transaction again: the
    // same groups report it once more, alone and together.
    assert!(rig.feed(vec![sighting(&pay, from)]).is_empty());
    assert!(rig.feed(vec![sighting(&pay, vec![peer(21)])]).is_empty());
    let after = rig.node.judge(0).unwrap();
    assert_eq!(format!("{before:?}"), format!("{after:?}"));
    assert_eq!(after.received_sats, 5_000);
    assert!(after.reports.is_empty());
}

#[test]
fn the_transaction_cache_holds_the_last_256() {
    let dir = Scratch::new();
    let (mut rig, _) = rig_with_a_sale(&dir);
    let all: Vec<Transaction> = (0..300u32)
        .map(|n| {
            let mut tx = spend(1, &[(&script_of(0), 5_000)]);
            tx.input[0].previous_output.vout = n;
            tx
        })
        .collect();
    for chunk in all.chunks(30) {
        rig.feed(chunk.iter().map(|t| sighting(t, vec![peer(20)])).collect());
    }
    assert_eq!(rig.node.cached_transactions(), TX_CACHE);
    assert!(!rig.node.is_cached(&all[43].compute_txid()));
    assert!(rig.node.is_cached(&all[44].compute_txid()));
    assert!(rig.node.is_cached(&all[299].compute_txid()));
}

#[test]
fn the_log_is_bounded_and_can_be_taken() {
    let dir = Scratch::new();
    let mut rig = start(dir.path());
    let notes: Vec<AppEvent> = (0..NOTE_LOG + 44)
        .map(|n| AppEvent::Note(format!("note {n}")))
        .collect();
    rig.feed(notes);
    assert_eq!(rig.node.notes().count(), NOTE_LOG);
    assert_eq!(rig.node.notes().next(), Some("note 44"));
    let taken = rig.node.take_notes();
    assert_eq!(taken.len(), NOTE_LOG);
    assert_eq!(rig.node.notes().count(), 0);
}

#[test]
fn opening_needs_the_health_rules_to_hold() {
    let dir = Scratch::new();
    let time = Arc::new(AtomicU64::new(T0));
    let mut config = config(dir.path(), &time);
    config.health = Box::new(check_health);
    let mut rig = start_with(config, time);
    // No Peers event yet.
    assert!(matches!(
        rig.node
            .open(five_dollars(), usd(), rate(), &source_at(&rig.time))
            .unwrap_err(),
        OpenRefused::Unhealthy(None)
    ));
    // One peer is fewer than three groups (WR-50).
    let peer_at = |n: u8| Peer {
        addr: peer(n),
        serves_filters: true,
        best_height: 0,
        transport: PeerTransport::V1,
    };
    rig.feed(vec![AppEvent::Peers(vec![peer_at(20)], None, (0, T0))]);
    assert_eq!(
        rig.node.health().unwrap().problem,
        Some(Problem::TooFewPeerGroups)
    );
    assert!(matches!(
        rig.node
            .open(five_dollars(), usd(), rate(), &source_at(&rig.time))
            .unwrap_err(),
        OpenRefused::Unhealthy(Some(Problem::TooFewPeerGroups))
    ));
    // Three groups, two with filters, a tip of this hour.
    let mut peers = vec![peer_at(20), peer_at(21)];
    peers.push(Peer {
        serves_filters: false,
        ..peer_at(22)
    });
    rig.feed(vec![AppEvent::Peers(peers.clone(), None, (0, T0))]);
    assert!(rig.node.health().unwrap().healthy);
    let opened = rig
        .node
        .open(five_dollars(), usd(), rate(), &source_at(&rig.time))
        .unwrap();
    assert_eq!(opened.index, 0);
    // The tip goes stale (WR-51): the answer changes.
    rig.time
        .store(T0 + 3 * 3_600, std::sync::atomic::Ordering::SeqCst);
    rig.feed(vec![AppEvent::Peers(peers, None, (0, T0))]);
    assert_eq!(rig.node.health().unwrap().problem, Some(Problem::TipIsOld));
}

#[test]
fn the_driver_starts_where_the_checked_height_and_the_headers_say() {
    let mut blocks = Blocks::default();
    for _ in 0..3 {
        blocks.extend(&[]);
    }
    let headers = blocks.stored_headers();

    // A counter that never ran: after the tip of the headers, and the height
    // is written down before any address is shown.
    let dir = Scratch::new();
    fs::write(dir.path().join("headers.bin"), &headers).unwrap();
    let rig = start(dir.path());
    assert_eq!(rig.launch.driver.watch_from, 3);
    assert_eq!(rig.node.checked_height(), 3);
    assert_eq!(
        fs::read(dir.path().join("checked.bin")).unwrap(),
        3u32.to_le_bytes()
    );
    assert_eq!(rig.node.chain().tip_height(), 3);
    // Each start below needs the directory free (NR-79).
    drop(rig);

    // A stored height below the tip is used as it is.
    DirStore::open(dir.path()).unwrap().put_checked(2).unwrap();
    assert_eq!(start(dir.path()).launch.driver.watch_from, 2);

    // One above the tip (the headers were saved before the height) is cut to
    // the tip: the blocks above it are asked for again.
    DirStore::open(dir.path()).unwrap().put_checked(10).unwrap();
    assert_eq!(start(dir.path()).launch.driver.watch_from, 3);

    // Addresses were shown and the height is gone: from the first block.
    let mut rig = start(dir.path());
    rig.feed(vec![Rig::peers()]);
    rig.node
        .open(five_dollars(), usd(), rate(), &source_at(&rig.time))
        .unwrap();
    drop(rig);
    fs::remove_file(dir.path().join("checked.bin")).unwrap();
    assert_eq!(start(dir.path()).launch.driver.watch_from, 0);

    // Headers that do not read back start from the first header, and say so.
    let bad = Scratch::new();
    fs::write(bad.path().join("headers.bin"), b"not headers").unwrap();
    let rig = start(bad.path());
    assert_eq!(rig.node.chain().tip_height(), 0);
    assert!(rig.node.notes().any(|n| n.contains("do not read back")));
}

#[test]
fn a_block_that_could_not_be_stored_keeps_the_checked_height_below_it() {
    let dir = Scratch::new();
    let (mut rig, pay) = rig_with_a_sale(&dir);
    rig.feed(vec![sighting(&pay, vec![peer(20)])]);
    assert_eq!(rig.node.checked_height(), 0);
    // The sale can no longer be written.
    fs::create_dir(dir.path().join("sale-0.bin.tmp")).unwrap();
    let mut blocks = Blocks::default();
    let first = blocks.extend(&[pay]).clone();
    let second = blocks.extend(&[]).clone();
    let reports = rig.feed(vec![
        joined(1, &first),
        block_event(&first),
        joined(2, &second),
        AppEvent::Checked(2),
    ]);
    assert!(reports.is_empty());
    assert!(rig.node.notes().any(|n| n.contains("was not stored")));
    // The block was lost for this run, so the stored height stays under it.
    assert_eq!(rig.node.checked_height(), 0);
    assert_eq!(
        DirStore::open(dir.path()).unwrap().load().unwrap().checked,
        Some(0)
    );
    // The cap stays for the rest of the run.
    fs::remove_dir(dir.path().join("sale-0.bin.tmp")).unwrap();
    rig.feed(vec![AppEvent::Checked(9)]);
    assert_eq!(rig.node.checked_height(), 0);
}

#[test]
fn a_block_that_does_not_parse_is_logged_and_holds_the_checked_height() {
    let dir = Scratch::new();
    let (mut rig, _) = rig_with_a_sale(&dir);
    let mut blocks = Blocks::default();
    let first = blocks.extend(&[]).clone();
    rig.feed(vec![
        joined(1, &first),
        AppEvent::Block {
            hash: first.block_hash(),
            bytes: vec![1, 2, 3],
        },
        AppEvent::Checked(1),
    ]);
    assert!(rig.node.notes().any(|n| n.contains("was refused")));
    assert_eq!(rig.node.checked_height(), 0);
}

#[test]
fn a_directory_that_does_not_read_back_stops_the_start() {
    let dir = Scratch::new();
    {
        let mut store = DirStore::open(dir.path()).unwrap();
        store.put(openbmt_counter::Slot::Desk, b"junk").unwrap();
    }
    let time = Arc::new(AtomicU64::new(T0));
    let result = Node::start_with(config(dir.path(), &time), |_| -> Result<FakePort, String> {
        Err("the driver must not start".to_string())
    });
    assert!(matches!(result.err(), Some(StartError::Counter(_))));
}

#[test]
fn a_cut_short_record_is_cut_off_at_start_and_logged() {
    let dir = Scratch::new();
    let (mut rig, _) = rig_with_a_sale(&dir);
    rig.node.shutdown();
    drop(rig);
    let path = dir.path().join("records.bin");
    let whole = fs::read(&path).unwrap();
    assert!(!whole.is_empty());
    let mut torn = whole.clone();
    torn.extend_from_slice(&[200, 0, 0, 0, 1, 2, 3]);
    fs::write(&path, &torn).unwrap();
    let again = start(dir.path());
    assert!(again.node.notes().any(|n| n.contains("cut short")));
    assert_eq!(fs::read(&path).unwrap(), whole);
}

#[test]
fn pump_waits_for_events_up_to_the_time_and_sees_the_driver_stop() {
    let dir = Scratch::new();
    let mut rig = start(dir.path());
    let began = Instant::now();
    assert!(rig.node.pump(Duration::from_millis(60)).is_empty());
    assert!(began.elapsed() >= Duration::from_millis(55));
    assert!(!rig.node.stopped());
    rig.events.send(AppEvent::Stopped).unwrap();
    rig.node.pump(Duration::ZERO);
    assert!(rig.node.stopped());
    drop(rig);

    let mut rig = start(dir.path());
    drop(rig.events);
    rig.node.pump(Duration::ZERO);
    assert!(rig.node.stopped());
}

#[test]
fn a_late_payment_is_reported_by_pump() {
    // A sale that expires, then is paid: the counter flags it, and pump
    // returns the report once.
    let dir = Scratch::new();
    let time = Arc::new(AtomicU64::new(T0));
    let mut rig = start_at(dir.path(), time.clone());
    rig.feed(vec![Rig::peers()]);
    rig.node
        .open(five_dollars(), usd(), rate(), &source_at(&rig.time))
        .unwrap();
    time.store(T0 + 2_000, std::sync::atomic::Ordering::SeqCst);
    assert_eq!(rig.node.judge(0).unwrap().state, State::Expired);
    let late = spend(1, &[(&script_of(0), 5_000)]);
    let reports = rig.feed(vec![sighting(&late, vec![peer(20)])]);
    assert_eq!(
        reports,
        vec![Report::LatePayment {
            index: 0,
            sats: 5_000
        }]
    );
    assert!(rig.feed(vec![sighting(&late, vec![peer(21)])]).is_empty());
}

#[test]
fn a_reorganization_takes_the_stored_checked_height_down_to_the_fork() {
    let dir = Scratch::new();
    let (mut rig, _pay) = rig_with_a_sale(&dir);
    let mut blocks = Blocks::default();
    let first = blocks.extend(&[]).clone();
    let second = blocks.extend(&[]).clone();
    let third = blocks.extend(&[]).clone();
    rig.feed(vec![
        joined(1, &first),
        joined(2, &second),
        joined(3, &third),
        AppEvent::Checked(3),
    ]);
    assert_eq!(rig.node.checked_height(), 3);
    let stored = |dir: &Scratch| DirStore::open(dir.path()).unwrap().load().unwrap().checked;
    assert_eq!(stored(&dir), Some(3));

    // Blocks 2 and 3 leave; three others take their place. The driver says
    // nothing new about its checked height until it is above 3 again.
    let alt2 = mine(first.block_hash(), 2, 1, &[]);
    let alt3 = mine(alt2.block_hash(), 3, 1, &[]);
    let alt4 = mine(alt3.block_hash(), 4, 1, &[]);
    rig.feed(vec![AppEvent::Chain {
        joined: vec![
            (2, alt2.block_hash()),
            (3, alt3.block_hash()),
            (4, alt4.block_hash()),
        ],
        left: vec![third.block_hash(), second.block_hash()],
    }]);
    assert_eq!(rig.node.checked_height(), 1);
    assert_eq!(stored(&dir), Some(1));
    // When the new branch has been checked, the height rises again.
    rig.feed(vec![AppEvent::Checked(4)]);
    assert_eq!(rig.node.checked_height(), 4);
    assert_eq!(stored(&dir), Some(4));
}

#[test]
fn nr_79_a_second_start_on_the_same_directory_fails_before_it_reads_or_spawns() {
    let dir = Scratch::new();
    let first = start(dir.path());
    let before = fs::read(dir.path().join("records.bin")).ok();
    let names = || {
        let mut all: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        all.sort();
        all
    };
    let listing = names();
    let time = Arc::new(AtomicU64::new(T0));
    let result = Node::start_with(config(dir.path(), &time), |_| -> Result<FakePort, String> {
        Err("the driver must not start".to_string())
    });
    match result.err() {
        Some(StartError::Locked(why)) => {
            assert!(
                why.starts_with("another OpenBMT process is using "),
                "{why}"
            );
        }
        other => panic!("{other:?}"),
    }
    // The first node is as it was.
    assert_eq!(fs::read(dir.path().join("records.bin")).ok(), before);
    // The refused start made and removed no file.
    assert_eq!(names(), listing);
    drop(first);
    // Released when the first node is dropped.
    let again = start(dir.path());
    assert_eq!(again.launch.driver.watch_from, 0);
}

#[test]
fn a_refund_request_is_a_record_on_the_nodes_chain_and_survives_a_restart() {
    use openbmt_counter::{Actor, Amount, Event, RefundAsk, RefundRequest, RefundState, Role};
    let dir = Scratch::new();
    let (mut rig, pay) = rig_with_a_sale(&dir);
    let mut blocks = Blocks::default();
    let first = blocks.extend(std::slice::from_ref(&pay)).clone();
    let mut events = vec![joined(1, &first), block_event(&first)];
    for height in 2..=6 {
        let block = blocks.extend(&[]).clone();
        events.push(joined(height, &block));
    }
    rig.feed(events);
    assert_eq!(rig.node.judge(0).unwrap().state, State::Final);

    let ann = Actor {
        role: Role::Cashier,
        name: "ann".to_string(),
    };
    let request = RefundRequest {
        ask: RefundAsk::Sats(2_000),
        destination: p2wsh(0xa1),
        actor: ann.clone(),
        reason: "wrong size".to_string(),
    };
    let records_before = rig.node.records_len();
    assert_eq!(rig.node.request_refund(0, request).unwrap(), 1);
    assert_eq!(rig.node.records_len(), records_before + 1);
    let views = rig.node.refund_views(0);
    assert_eq!(views.len(), 1);
    assert_eq!(
        (views[0].asked, views[0].state),
        (2_000, RefundState::Requested)
    );

    // A second request over what was received is refused by the counter.
    let again = RefundRequest {
        ask: RefundAsk::Sats(3_001),
        destination: p2wsh(0xa2),
        actor: ann.clone(),
        reason: "wrong size".to_string(),
    };
    assert!(rig.node.request_refund(0, again).is_err());
    rig.node
        .refund_in_cash(
            0,
            openbmt_counter::CashRefund {
                actor: ann.clone(),
                minor: Amount(100),
                reason: "till".to_string(),
            },
        )
        .unwrap();
    rig.node.withdraw_refund(0, 1, ann).unwrap();

    // The records are on disk: a restarted node reads the same.
    let kinds: Vec<_> = rig
        .node
        .records_from(0, usize::MAX)
        .unwrap()
        .iter()
        .filter_map(|r| Event::decode(&r.body).ok())
        .filter(|e| e.kind() == openbmt_core::records::RecordKind::Refund)
        .collect();
    assert_eq!(kinds.len(), 3);
    drop(rig);
    let rig = start(dir.path());
    let views = rig.node.refund_views(0);
    assert_eq!(views.len(), 2);
    assert_eq!(views[0].state, RefundState::Withdrawn);
    assert_eq!(views[1].state, RefundState::PaidInCash);
}

#[test]
fn cr_210_a_request_paid_while_the_node_was_closed_shows_paid_on_the_next_start() {
    let dir = Scratch::new();
    let mut rig = start(dir.path());
    assert!(rig.feed(vec![Rig::peers()]).is_empty());
    let opened = rig
        .node
        .open_with_extras(
            five_dollars(),
            Amount(0),
            usd(),
            rate(),
            &source_at(&rig.time),
            None,
            SaleExtras {
                note: Some("Tour 12 May"),
                request: Some(RequestDays::Seven),
            },
        )
        .unwrap();
    assert_eq!(opened.quote.until, opened.quote.from + 7 * 86_400);
    assert_eq!(
        rig.node.judge(opened.index).unwrap().state,
        State::NotPaidYet
    );
    assert!(rig.node.shutdown().is_empty());
    drop(rig);

    // Closed for two days; the payment was mined meanwhile.
    let pay = spend(1, &[(&script_of(0), 5_000)]);
    let mut blocks = Blocks::default();
    let first = blocks.extend(std::slice::from_ref(&pay)).clone();
    fs::write(dir.path().join("headers.bin"), blocks.stored_headers()).unwrap();
    let time = Arc::new(AtomicU64::new(T0 + 2 * 86_400));
    let mut again = start_at(dir.path(), time);
    assert!(again.launch.scripts.contains(&script_of(0)));
    assert!(again.feed(vec![Rig::peers()]).is_empty());
    let sale = again.node.counter().sale(0).unwrap();
    assert_eq!(sale.note(), Some("Tour 12 May"));
    assert!(sale.is_request());
    let reports = again.feed(vec![joined(1, &first), block_event(&first)]);
    assert_eq!(reports, vec![Report::PosPaid { index: 0 }]);
    let screen = again.node.judge(0).unwrap();
    assert_eq!(screen.state, State::Confirmed);
    assert_eq!(screen.received_sats, 5_000);
}
