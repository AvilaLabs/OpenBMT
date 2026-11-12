//! NR-105 (the driver's part), NR-106 to NR-111: the driver with the own
//! node as the only peer, against fake nodes on loopback sockets. The check
//! against Bitcoin Core is `node_only_core.rs`.

use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use bitcoin::consensus::serialize;
use bitcoin::p2p::address::AddrV2;
use bitcoin::p2p::ServiceFlags;
use bitcoin::{Network, ScriptBuf, Witness};
use openbmt_net::onion::onion_name;
use openbmt_net::{AddressBook, OwnNode, Source};
use openbmt_watch::{
    NodeOnlyFault, NodeOnlyInput, NodeOnlyLine, NodeOnlyProblem, NodeOnlyTracker, OwnNodeLink,
};

use super::kit::*;
use crate::dial::{DialError, Net};
use crate::driver::{
    AppEvent, Driver, DriverConfig, Handle, NetMode, OwnNodeEvent, StartError, FALLBACK_STOP_SECS,
};
use crate::env::Key;
use crate::socks::Target;
use crate::RelayState;

const T0: u64 = 1_700_000_000;

fn onion() -> String {
    onion_name(&[9; 32])
}

fn target() -> Target {
    Target::Name(onion(), 8333)
}

/// A config in the mode, with an own node behind a proxy that the fake net
/// never uses.
fn config(light_fallback: bool) -> DriverConfig {
    let mut c = DriverConfig::new(Network::Regtest, 0);
    c.v2 = false;
    c.proxy = Some(SocketAddr::from(([127, 0, 0, 1], 9)));
    c.own_node = Some(OwnNode::onion(&onion(), 8333).unwrap());
    c.mode = NetMode::NodeOnly { light_fallback };
    c
}

fn start(config: DriverConfig, storage: MemStorage, net: Net, clock: &TestClock) -> Handle {
    start_with(config, storage, net, clock, Vec::new())
}

fn start_with(
    config: DriverConfig,
    storage: MemStorage,
    net: Net,
    clock: &TestClock,
    scripts: Vec<ScriptBuf>,
) -> Handle {
    Driver::spawn_with(
        config,
        storage,
        clock.clone(),
        CountingRandomness(1),
        scripts,
        net,
    )
    .unwrap()
}

/// The next event of the mode that `want` accepts, within `secs`.
fn event(
    handle: &Handle,
    secs: u64,
    want: impl Fn(&NodeOnlyInput) -> bool,
) -> Option<NodeOnlyInput> {
    let end = Instant::now() + Duration::from_secs(secs);
    while let Some(left) = end.checked_duration_since(Instant::now()) {
        // The driver holds back while the peers' events are not read.
        while handle.events().try_recv().is_ok() {}
        match handle
            .own_node_events()
            .recv_timeout(left.min(Duration::from_millis(100)))
        {
            Ok(OwnNodeEvent::NodeOnly(input)) if want(&input) => return Some(input),
            Ok(OwnNodeEvent::State(_)) => panic!("a State event with the mode on"),
            _ => {}
        }
    }
    None
}

fn connected(input: &NodeOnlyInput) -> bool {
    input.node.link == OwnNodeLink::Connected && input.fault.is_none()
}

fn settle() {
    std::thread::sleep(Duration::from_millis(700));
}

/// A node that serves headers, filters and blocks of this chain.
fn node_serving(blocks: Vec<bitcoin::Block>, limited: bool) -> FakePeer {
    FakePeer::start(Behavior::Node(NodeScript {
        filters: true,
        limited,
        blocks,
        ..NodeScript::default()
    }))
}

fn filter_node() -> FakePeer {
    FakePeer::start(Behavior::Node(NodeScript {
        height: 7,
        filters: true,
        ..NodeScript::default()
    }))
}

// NR-105 (what the driver does with the mode) and NR-106.

#[test]
fn nr_106_the_mode_needs_an_own_node_and_refuses_the_config_without_one() {
    let mut c = config(false);
    c.own_node = None;
    let (net, ..) = net_to(SocketAddr::from(([127, 0, 0, 1], 1)), Vec::new());
    let err = Driver::spawn_with(
        c,
        MemStorage::default(),
        TestClock::at(T0),
        CountingRandomness(1),
        Vec::new(),
        net,
    )
    .expect_err("no own node");
    assert_eq!(err, StartError::NodeOnlyNeedsOwnNode);
    assert!(err.to_string().starts_with("NR-105: "));
    // Mixed with no own node still starts, and is the default.
    assert_eq!(DriverConfig::new(Network::Regtest, 0).mode, NetMode::Mixed);
}

#[test]
fn nr_106_the_driver_dials_the_own_node_and_nothing_else() {
    let node = filter_node();
    let other = FakePeer::start_on("127.0.0.2", Behavior::Honest(Vec::new()));
    // Everything that could be dialed: an anchor, a fixed peer, the book and
    // the seeds.
    let mut book = AddressBook::new();
    let anchor = AddrV2::Ipv4(Ipv4Addr::new(8, 8, 8, 8));
    book.add(&anchor, 8333, ServiceFlags::NETWORK, Source::Seed, T0);
    book.set_anchors(&[(anchor, 8333)]);
    let storage = MemStorage::with(Key::Book, book.to_bytes());
    let mut c = config(false);
    c.allow_local = true;
    c.fixed_peers = vec![other.addr];
    c.seeds = vec!["seed.example".to_string()];
    let (inner, _, lookups) = net_to(other.addr, vec![other.addr]);
    let node_addr = node.addr;
    let net = Net {
        dial: Arc::new(move |t, proxy, token| {
            if *t == target() {
                TcpStream::connect(node_addr).map_err(|e| DialError::Connect(e.kind()))
            } else {
                (inner.dial)(t, proxy, token)
            }
        }),
        resolve: inner.resolve.clone(),
        proxy_resolve: inner.proxy_resolve.clone(),
    };
    let clock = TestClock::at(T0);
    let handle = start(c, storage.clone(), net, &clock);
    let state = event(&handle, 10, connected).expect("the own node connects");
    assert_eq!(state.node.best_height, Some(7));
    assert!(!state.light_fallback && !state.fallback_running);
    for step in 1..4 {
        clock.set(T0 + 400 * step);
        settle();
    }
    assert_eq!(node.accept_count(), 1);
    assert_eq!(other.accept_count(), 0);
    assert_eq!(lookups.load(Ordering::SeqCst), 0);
    // The book was not touched: not read into anchors and not saved (the
    // anchors are set every 10 minutes otherwise).
    assert_eq!(storage.save_count(Key::Book), 0);
    // The own node is not one of the peers of the light client.
    let peers = wait_for(&handle, 3, |e| matches!(e, AppEvent::Peers(..)));
    assert!(peers
        .unwrap()
        .iter()
        .all(|e| !matches!(e, AppEvent::Peers(p, ..) if !p.is_empty())));
}

#[test]
fn nr_106_a_transaction_the_node_announces_is_a_report_and_no_light_sighting() {
    let tx = {
        let blocks = blocks(1, 10);
        blocks[0].txdata[0].clone()
    };
    let node = FakePeer::start(Behavior::Node(NodeScript {
        height: 3,
        filters: true,
        announce: vec![tx.compute_txid()],
        mempool: vec![tx.clone()],
        ..NodeScript::default()
    }));
    let (net, ..) = net_to(node.addr, Vec::new());
    let clock = TestClock::at(T0);
    let handle = start(config(false), MemStorage::default(), net, &clock);
    // The node announces right after the handshake, so its report and the
    // fetched transaction may come before the connected state: all three are
    // watched from the start, and nothing read is thrown away.
    let end = Instant::now() + Duration::from_secs(10);
    let (mut up, mut has, mut sighted) = (false, false, None);
    while Instant::now() < end && !(up && has && sighted.is_some()) {
        match handle
            .own_node_events()
            .recv_timeout(Duration::from_millis(50))
        {
            Ok(OwnNodeEvent::Has(t)) => has |= t == tx.compute_txid(),
            Ok(OwnNodeEvent::NodeOnly(input)) => up |= connected(&input),
            Ok(OwnNodeEvent::State(_)) => panic!("a State event with the mode on"),
            _ => {}
        }
        while let Ok(e) = handle.events().try_recv() {
            if let AppEvent::Sighting { tx: bytes, from } = e {
                sighted = Some((bytes, from));
            }
        }
    }
    assert!(up, "the own node connects");
    assert!(has, "the node's report");
    let (bytes, from) = sighted.expect("the transaction is fetched");
    assert_eq!(bytes, serialize(&tx));
    assert!(from.is_empty(), "{from:?}");
}

// NR-107.

#[test]
fn nr_107_a_node_without_compact_filters_is_reported_and_not_usable() {
    let node = FakePeer::start(Behavior::Node(NodeScript::default()));
    let (net, ..) = net_to(node.addr, Vec::new());
    let clock = TestClock::at(T0);
    let handle = start(config(false), MemStorage::default(), net, &clock);
    let input = event(&handle, 10, |i| {
        matches!(i.fault, Some(NodeOnlyFault::MissingFilters { .. }))
    })
    .expect("the state says the filters are missing");
    assert!(matches!(input.node.link, OwnNodeLink::Down { .. }));
    let health = NodeOnlyTracker::new().check(Some(&input), 0, T0 + 1);
    assert!(!health.own.usable);
    assert!(matches!(
        health.problem,
        Some(NodeOnlyProblem::MissingFilters { .. })
    ));
    assert!(matches!(health.line(), NodeOnlyLine::NotUsable(_)));
}

#[test]
fn nr_107_a_node_with_filters_is_usable() {
    let node = filter_node();
    let (net, ..) = net_to(node.addr, Vec::new());
    let clock = TestClock::at(T0);
    let handle = start(config(false), MemStorage::default(), net, &clock);
    let input = event(&handle, 10, connected).expect("connected");
    let health = NodeOnlyTracker::new().check(Some(&input), 7, T0 + 1);
    assert!(health.own.usable, "{health:?}");
    assert_eq!(health.line(), NodeOnlyLine::Usable);
}

// NR-108.

#[test]
fn nr_108_a_block_older_than_the_node_serves_is_reported_by_the_driver() {
    let script = ScriptBuf::from_bytes(vec![
        0x00, 0x14, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7,
    ]);
    let mut chain: Vec<bitcoin::Block> = Vec::new();
    let mut prev = bitcoin::blockdata::constants::genesis_block(Network::Regtest).header;
    for tag in 1..=300 {
        let block = block_paying(&prev, tag, &script);
        prev = block.header;
        chain.push(block);
    }
    let node = node_serving(chain, true);
    let (net, ..) = net_to(node.addr, Vec::new());
    let clock = TestClock::at(T0);
    let handle = start_with(
        config(false),
        MemStorage::default(),
        net,
        &clock,
        vec![script],
    );
    let input = event(&handle, 20, |i| {
        matches!(i.fault, Some(NodeOnlyFault::LacksBlock { .. }))
    })
    .expect("the state names the block it lacks");
    let Some(NodeOnlyFault::LacksBlock { height, .. }) = input.fault else {
        unreachable!()
    };
    assert_eq!(height, 1);
    let health = NodeOnlyTracker::new().check(Some(&input), 300, T0 + 1);
    assert!(matches!(
        health.problem,
        Some(NodeOnlyProblem::LacksBlock { height: 1, .. })
    ));
}

// NR-109.

#[test]
fn nr_109_a_transaction_is_announced_to_the_own_node_and_to_nobody_else() {
    let node = filter_node();
    let other = FakePeer::start_on("127.0.0.2", Behavior::Honest(Vec::new()));
    let mut c = config(false);
    c.allow_local = true;
    c.fixed_peers = vec![other.addr];
    let (net, ..) = net_to(node.addr, Vec::new());
    let clock = TestClock::at(T0);
    let handle = start(c, MemStorage::default(), net, &clock);
    assert!(event(&handle, 10, connected).is_some());
    let tx = blocks(1, 10)[0].txdata[0].clone();
    handle.broadcast(serialize(&tx));
    assert!(until(10, || node
        .invs
        .lock()
        .unwrap()
        .contains(&tx.compute_txid())));
    assert_eq!(other.accept_count(), 0);
}

// NR-112.

#[test]
fn nr_112_a_held_transaction_the_node_answers_for_is_held_by_node() {
    let mut tx = blocks(1, 10)[0].txdata[0].clone();
    tx.input[0].witness = Witness::from_slice(&[vec![1, 2, 3], vec![4; 33]]);
    let node = FakePeer::start(Behavior::Node(NodeScript {
        height: 7,
        filters: true,
        mempool: vec![tx.clone()],
        ..NodeScript::default()
    }));
    let (net, ..) = net_to(node.addr, Vec::new());
    let clock = TestClock::at(T0);
    let handle = start(config(false), MemStorage::default(), net, &clock);
    assert!(event(&handle, 10, connected).is_some());
    let txid = tx.compute_txid();
    handle.broadcast(serialize(&tx));
    assert!(until(10, || node.invs.lock().unwrap().contains(&txid)));
    // Before the 130 seconds nothing asks for it, so the state is `Sent`.
    clock.set(T0 + 100);
    let seen = wait_for(&handle, 2, |e| {
        matches!(
            e,
            AppEvent::TxState {
                state: RelayState::HeldByNode,
                ..
            }
        )
    });
    assert!(seen.is_none(), "held by the node before it was asked");
    clock.set(T0 + 140);
    let seen = wait_for(&handle, 10, |e| {
        matches!(
            e,
            AppEvent::TxState {
                state: RelayState::HeldByNode,
                ..
            }
        )
    });
    assert!(seen.is_some(), "the node's answer was not reported");
}

// NR-110.

/// A net where the own node is dialed only while `up` is set, and any other
/// address goes to `other`.
fn switchable(node: SocketAddr, other: SocketAddr, up: Arc<AtomicBool>) -> Net {
    let (inner, ..) = net_to(other, Vec::new());
    Net {
        dial: Arc::new(move |t, _, _| {
            if *t == target() {
                if up.load(Ordering::SeqCst) {
                    TcpStream::connect(node).map_err(|e| DialError::Connect(e.kind()))
                } else {
                    Err(DialError::Connect(std::io::ErrorKind::ConnectionRefused))
                }
            } else {
                TcpStream::connect(other).map_err(|e| DialError::Connect(e.kind()))
            }
        }),
        resolve: inner.resolve.clone(),
        proxy_resolve: inner.proxy_resolve.clone(),
    }
}

#[test]
fn nr_110_without_light_fallback_nobody_else_is_dialed_while_the_node_is_down() {
    let node = filter_node();
    let other = FakePeer::start_on("127.0.0.2", Behavior::Honest(Vec::new()));
    let mut c = config(false);
    c.allow_local = true;
    c.fixed_peers = vec![other.addr];
    let up = Arc::new(AtomicBool::new(false));
    let net = switchable(node.addr, other.addr, up);
    let clock = TestClock::at(T0);
    let handle = start(c, MemStorage::default(), net, &clock);
    let input = event(&handle, 10, |i| {
        matches!(i.node.link, OwnNodeLink::Down { .. })
    })
    .expect("down");
    assert!(!input.fallback_running && !input.light_fallback);
    for step in 1..4 {
        clock.set(T0 + 200 * step);
        settle();
    }
    assert_eq!(other.accept_count(), 0);
    assert_eq!(node.accept_count(), 0);
    let health = NodeOnlyTracker::new().check(Some(&input), 0, T0 + 600);
    assert!(matches!(health.line(), NodeOnlyLine::NotUsable(_)));
}

#[test]
fn nr_110_with_light_fallback_the_random_peers_start_with_a_note_and_stop_after_two_minutes() {
    let node = filter_node();
    let other = FakePeer::start_on("127.0.0.2", Behavior::Honest(Vec::new()));
    let mut c = config(true);
    c.allow_local = true;
    c.fixed_peers = vec![other.addr];
    let up = Arc::new(AtomicBool::new(false));
    let net = switchable(node.addr, other.addr, up.clone());
    let clock = TestClock::at(T0);
    let handle = start(c, MemStorage::default(), net, &clock);
    // The node cannot be used: the random peers run, and the state says so.
    let input = event(&handle, 10, |i| i.fallback_running).expect("the fallback starts");
    assert!(input.light_fallback);
    assert!(until(10, || other.accept_count() == 1));
    let health = NodeOnlyTracker::new().check(Some(&input), 0, T0 + 1);
    assert!(matches!(health.line(), NodeOnlyLine::CheckingWithOthers(_)));
    let notes = wait_for(
        &handle,
        5,
        |e| matches!(e, AppEvent::Note(n) if n.contains("checking with other nodes")),
    );
    assert!(notes.is_some(), "the start is said in a note");
    // The node comes up; the random peers stay for two minutes more.
    up.store(true, Ordering::SeqCst);
    clock.set(T0 + 10);
    let input = event(&handle, 10, connected).expect("the node connects");
    assert!(input.fallback_running);
    clock.set(T0 + 10 + FALLBACK_STOP_SECS - 5);
    settle();
    assert_eq!(other.closed_by_driver.load(Ordering::SeqCst), 0);
    clock.set(T0 + 10 + FALLBACK_STOP_SECS + 5);
    let input = event(&handle, 10, |i| {
        !i.fallback_running && i.node.link == OwnNodeLink::Connected
    })
    .expect("the fallback stops");
    assert!(input.light_fallback);
    assert!(until(10, || other.closed_by_driver.load(Ordering::SeqCst) == 1));
    let notes = wait_for(
        &handle,
        5,
        |e| matches!(e, AppEvent::Note(n) if n.contains("no other nodes are asked")),
    );
    assert!(notes.is_some(), "the stop is said in a note");
    // And nobody is dialed again.
    let accepted = other.accept_count();
    clock.set(T0 + 10 + FALLBACK_STOP_SECS + 300);
    settle();
    assert_eq!(other.accept_count(), accepted);
}

// NR-111.

#[test]
fn nr_111_no_other_node_learns_anything_from_a_device_in_the_mode() {
    // With the own node up, a fixed peer and a seed name set, no seed is
    // asked, no other node is dialed, and nothing is announced to the own
    // node that it did not ask for: what it learns is what it is asked.
    let node = filter_node();
    let other = FakePeer::start_on("127.0.0.2", Behavior::Honest(Vec::new()));
    let mut c = config(false);
    c.allow_local = true;
    c.fixed_peers = vec![other.addr];
    c.seeds = vec!["seed.example".to_string()];
    let up = Arc::new(AtomicBool::new(true));
    let net = switchable(node.addr, other.addr, up);
    let clock = TestClock::at(T0);
    let handle = start(c, MemStorage::default(), net, &clock);
    assert!(event(&handle, 10, connected).is_some());
    clock.set(T0 + 1_000);
    settle();
    assert_eq!(node.accept_count(), 1);
    assert_eq!(other.accept_count(), 0);
    assert!(node.invs.lock().unwrap().is_empty());
    assert!(node.txs.lock().unwrap().is_empty());
}
