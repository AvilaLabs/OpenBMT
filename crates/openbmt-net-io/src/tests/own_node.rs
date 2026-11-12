//! NR-98 to NR-103: the merchant's own node, in the driver, against fake
//! nodes on loopback sockets. The check against Bitcoin Core is
//! `own_node_core.rs`.

use std::fs;
use std::io::ErrorKind;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use bitcoin::absolute::LockTime;
use bitcoin::hashes::Hash;
use bitcoin::transaction::Version;
use bitcoin::{
    Amount, Network, OutPoint, ScriptBuf, Sequence, Transaction, TxIn, TxOut, Txid, Witness,
};
use openbmt_net::onion::onion_name;
use openbmt_net::{OwnNode, OwnNodeHost};
use openbmt_watch::{check_own_node, OwnNodeInput, OwnNodeLink, OwnNodeProblem};

use super::kit::*;
use crate::dial::{DialError, Net};
use crate::driver::{
    own_wait, AppEvent, Driver, DriverConfig, Handle, OwnNodeEvent, StartError,
    OWN_BACKOFF_FIRST_SECS, OWN_BACKOFF_MAX_SECS, OWN_EVENT_CAPACITY,
};
use crate::env::Clock;
use crate::socks::Target;

const T0: u64 = 1_700_000_000;

fn onion() -> String {
    onion_name(&[9; 32])
}

fn node() -> OwnNode {
    OwnNode::onion(&onion(), 8333).unwrap()
}

fn txid(n: u8) -> Txid {
    Txid::from_byte_array([n; 32])
}

/// A transaction with an ID of its own.
fn tx_paying(n: u8) -> Transaction {
    Transaction {
        version: Version::TWO,
        lock_time: LockTime::ZERO,
        input: vec![TxIn {
            previous_output: OutPoint::new(txid(n), 0),
            script_sig: ScriptBuf::new(),
            sequence: Sequence::MAX,
            witness: Witness::new(),
        }],
        output: vec![TxOut {
            value: Amount::from_sat(1_000),
            script_pubkey: ScriptBuf::from_bytes(vec![0x51]),
        }],
    }
}

/// A config with an own node and a proxy that the fake net never uses.
fn own_config() -> DriverConfig {
    let mut c = DriverConfig::new(Network::Regtest, 0);
    c.v2 = false;
    c.proxy = Some(SocketAddr::from(([127, 0, 0, 1], 9)));
    c.own_node = Some(node());
    c
}

fn start(config: DriverConfig, net: Net, clock: &TestClock) -> Handle {
    Driver::spawn_with(
        config,
        MemStorage::default(),
        clock.clone(),
        CountingRandomness(1),
        Vec::new(),
        net,
    )
    .unwrap()
}

/// The next own-node event that `want` accepts, within `secs`.
fn own_event(
    handle: &Handle,
    secs: u64,
    want: impl Fn(&OwnNodeEvent) -> bool,
) -> Option<OwnNodeEvent> {
    let end = Instant::now() + Duration::from_secs(secs);
    while let Some(left) = end.checked_duration_since(Instant::now()) {
        match handle.own_node_events().recv_timeout(left) {
            Ok(event) if want(&event) => return Some(event),
            Ok(_) => {}
            Err(_) => return None,
        }
    }
    None
}

fn connected(event: &OwnNodeEvent) -> bool {
    matches!(
        event,
        OwnNodeEvent::State(OwnNodeInput {
            link: OwnNodeLink::Connected,
            ..
        })
    )
}

fn has(want: Txid) -> impl Fn(&OwnNodeEvent) -> bool {
    move |e| *e == OwnNodeEvent::Has(want)
}

/// The settle time of the driver's loop.
fn settle() {
    std::thread::sleep(Duration::from_millis(700));
}

// NR-98.

#[test]
fn nr_98_a_tor_own_node_without_a_proxy_does_not_start() {
    let mut config = own_config();
    config.proxy = None;
    let (net, ..) = net_to(SocketAddr::from(([127, 0, 0, 1], 1)), Vec::new());
    let result = Driver::spawn_with(
        config,
        MemStorage::default(),
        TestClock::at(T0),
        CountingRandomness(1),
        Vec::new(),
        net,
    );
    let err = result.expect_err("no proxy");
    assert_eq!(err, StartError::OwnNodeNeedsTor);
    assert!(err.to_string().starts_with("NR-98: "));
}

#[test]
fn nr_98_the_own_node_is_dialed_by_its_name_through_the_proxy() {
    let peer = FakePeer::start(Behavior::Node(NodeScript::default()));
    let (net, log, _) = net_to(peer.addr, Vec::new());
    let clock = TestClock::at(T0);
    let handle = start(own_config(), net, &clock);
    assert!(own_event(&handle, 10, connected).is_some());
    let dials = log.lock().unwrap().clone();
    assert_eq!(dials.len(), 1);
    assert_eq!(dials[0].0, Target::Name(onion(), 8333));
    assert_eq!(dials[0].1, Some(SocketAddr::from(([127, 0, 0, 1], 9))));
}

#[test]
fn nr_98_without_an_own_node_nothing_is_dialed_for_it_and_nothing_is_sent() {
    let (net, log, _) = net_to(SocketAddr::from(([127, 0, 0, 1], 1)), Vec::new());
    let mut config = DriverConfig::new(Network::Regtest, 0);
    config.v2 = false;
    let clock = TestClock::at(T0);
    let handle = start(config, net, &clock);
    settle();
    assert!(handle
        .own_node_events()
        .recv_timeout(Duration::from_millis(800))
        .is_err());
    assert!(log.lock().unwrap().is_empty());
}

// NR-99.

/// Every file under `dir` with this extension, as (path, text).
fn files(dir: &Path, ext: &str, out: &mut Vec<(PathBuf, String)>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if path
                .file_name()
                .is_some_and(|n| n == "target" || n == ".git")
            {
                continue;
            }
            files(&path, ext, out);
        } else if path.extension().is_some_and(|e| e == ext) {
            if let Ok(text) = fs::read_to_string(&path) {
                out.push((path, text));
            }
        }
    }
}

fn root() -> PathBuf {
    let manifest = std::env::var_os("CARGO_MANIFEST_DIR").expect("run by cargo");
    PathBuf::from(manifest).join("../..")
}

const FEATURE: &str = "clear-own-node-for-tests";
const CONSTRUCTOR: &str = "clear_net_for_tests";

#[test]
fn nr_99_only_the_drivers_dev_dependency_turns_the_feature_on() {
    let mut manifests = Vec::new();
    files(&root(), "toml", &mut manifests);
    manifests.retain(|(path, _)| path.file_name().is_some_and(|n| n == "Cargo.toml"));
    assert!(manifests.len() > 10, "found {} manifests", manifests.len());
    let mut named = Vec::new();
    for (path, text) in &manifests {
        let mut section = String::new();
        for line in text.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with('[') {
                section = trimmed.to_string();
            }
            if trimmed.contains(FEATURE) && !trimmed.starts_with('#') {
                let name = path
                    .components()
                    .rev()
                    .nth(1)
                    .map(|c| c.as_os_str().to_string_lossy().into_owned())
                    .unwrap_or_default();
                named.push((name, section.clone()));
            }
        }
    }
    named.sort();
    named.dedup();
    assert_eq!(
        named,
        vec![
            ("openbmt-net".to_string(), "[features]".to_string()),
            (
                "openbmt-net-io".to_string(),
                "[dev-dependencies]".to_string()
            ),
        ],
        "the feature is named where it should not be"
    );
}

#[test]
fn nr_99_only_the_drivers_tests_call_the_constructor() {
    let mut sources = Vec::new();
    for dir in ["crates", "fuzz"] {
        files(&root().join(dir), "rs", &mut sources);
    }
    assert!(sources.len() > 100, "found {} sources", sources.len());
    let mut users: Vec<String> = sources
        .iter()
        .filter(|(_, text)| text.contains(CONSTRUCTOR))
        .map(|(path, _)| {
            let text = path.to_string_lossy().replace('\\', "/");
            text.split("crates/").last().unwrap_or("").to_string()
        })
        .collect();
    users.sort();
    // The definition, the driver's tests (the module of tests, which
    // `lib.rs` compiles only for a test), and the documents of the module.
    for user in &users {
        assert!(
            user == "openbmt-net/src/own_node.rs" || user.starts_with("openbmt-net-io/src/tests/"),
            "{user} names the constructor"
        );
    }
    assert!(users.iter().any(|u| u == "openbmt-net/src/own_node.rs"));
    // The tests of the driver are compiled only for a test.
    let lib = fs::read_to_string(root().join("crates/openbmt-net-io/src/lib.rs")).unwrap();
    assert!(lib.contains("#[cfg(test)]\nmod tests;"));
    // The constructor itself exists only with the feature.
    let own = fs::read_to_string(root().join("crates/openbmt-net/src/own_node.rs")).unwrap();
    let at = own.find(&format!("pub fn {CONSTRUCTOR}")).unwrap();
    let before = &own[..at];
    let attribute = before.rfind("#[cfg(").unwrap();
    assert!(before[attribute..].starts_with(&format!("#[cfg(feature = \"{FEATURE}\")]")));
    assert_eq!(own.matches("#[cfg(feature").count(), 1);
}

#[test]
fn nr_99_a_clear_net_own_node_is_not_authenticated() {
    let own = OwnNode::clear_net_for_tests(SocketAddr::from(([127, 0, 0, 1], 18444)));
    assert!(!own.is_authenticated());
    assert_eq!(
        own.host(),
        &OwnNodeHost::Clear(SocketAddr::from(([127, 0, 0, 1], 18444)))
    );
    // A Tor one is.
    assert!(node().is_authenticated());
}

#[test]
fn nr_99_a_fixed_peer_on_this_machine_is_a_peer_and_never_the_own_node() {
    // A node at a clear-net address that is only a fixed peer: it is one of
    // the peers (it is in the `Peers` event) and the own node is not there.
    let peer = FakePeer::start(Behavior::Honest(Vec::new()));
    let mut config = DriverConfig::new(Network::Regtest, 0);
    config.v2 = false;
    config.allow_local = true;
    config.fixed_peers = vec![peer.addr];
    let clock = TestClock::at(T0);
    let (net, ..) = net_to(peer.addr, Vec::new());
    let handle = start(config, net, &clock);
    assert!(wait_for(&handle, 10, has_peer).is_some());
    assert!(handle
        .own_node_events()
        .recv_timeout(Duration::from_millis(800))
        .is_err());
}

#[test]
fn nr_99_the_own_nodes_address_is_never_dialed_as_a_peer() {
    // The same address is the own node and a fixed peer: it is dialed once,
    // as the own node, and not again as one of the random peers.
    let peer = FakePeer::start(Behavior::Node(NodeScript::default()));
    let mut config = own_config();
    config.proxy = None;
    config.allow_local = true;
    config.fixed_peers = vec![peer.addr];
    config.own_node = Some(OwnNode::clear_net_for_tests(peer.addr));
    let (net, ..) = net_to(peer.addr, Vec::new());
    let clock = TestClock::at(T0);
    let handle = start(config, net, &clock);
    assert!(own_event(&handle, 10, connected).is_some());
    for step in 1..4 {
        clock.set(T0 + 100 * step);
        settle();
    }
    assert_eq!(peer.accept_count(), 1);
    let events = wait_for(&handle, 3, |e| matches!(e, AppEvent::Peers(..))).unwrap();
    assert!(events
        .iter()
        .all(|e| !matches!(e, AppEvent::Peers(p, ..) if !p.is_empty())));
}

// NR-100.

#[test]
fn nr_100_the_wait_doubles_to_five_minutes_and_starts_again_after_a_stable_connection() {
    let mut backoff = OWN_BACKOFF_FIRST_SECS;
    let mut waits = Vec::new();
    for _ in 0..9 {
        let (wait, next) = own_wait(backoff, false);
        waits.push(wait);
        backoff = next;
    }
    assert_eq!(waits, [5, 10, 20, 40, 80, 160, 300, 300, 300]);
    assert_eq!(own_wait(backoff, true), (OWN_BACKOFF_FIRST_SECS, 10));
    assert_eq!(OWN_BACKOFF_MAX_SECS, 300);
}

#[test]
fn nr_100_a_failed_dial_is_tried_again_with_the_wait_of_the_rule() {
    let clock = TestClock::at(T0);
    let times: Arc<Mutex<Vec<u64>>> = Arc::default();
    let (t, c) = (times.clone(), clock.clone());
    let (inner, ..) = net_to(SocketAddr::from(([127, 0, 0, 1], 1)), Vec::new());
    let net = Net {
        dial: Arc::new(move |_, _, _| {
            t.lock().unwrap().push(c.now());
            Err(DialError::Connect(ErrorKind::ConnectionRefused))
        }),
        resolve: inner.resolve.clone(),
        proxy_resolve: inner.proxy_resolve.clone(),
    };
    let handle = start(own_config(), net, &clock);
    assert!(until(10, || times.lock().unwrap().len() == 1));
    // Let the driver see the first failure at the start time, before the
    // clock moves: the wait is counted from the time it sees.
    settle();
    let mut now = T0;
    for (i, wait) in [5u64, 10, 20, 40, 80, 160, 300, 300]
        .into_iter()
        .enumerate()
    {
        // One second short of the wait: no dial.
        clock.set(now + wait - 1);
        settle();
        assert_eq!(times.lock().unwrap().len(), i + 1, "before wait {i}");
        clock.set(now + wait);
        assert!(
            until(10, || times.lock().unwrap().len() == i + 2),
            "wait {i}"
        );
        // Let the driver see the failure at this time, before the clock moves.
        settle();
        now += wait;
    }
    assert_eq!(
        *times.lock().unwrap(),
        [
            T0,
            T0 + 5,
            T0 + 15,
            T0 + 35,
            T0 + 75,
            T0 + 155,
            T0 + 315,
            T0 + 615,
            T0 + 915
        ]
    );
    // The app was told how it stands: not connected since the start.
    let down = own_event(&handle, 10, |e| matches!(e, OwnNodeEvent::State(_)));
    assert!(matches!(
        down,
        Some(OwnNodeEvent::State(OwnNodeInput {
            link: OwnNodeLink::Down { since: T0 },
            best_height: None,
            ..
        }))
    ));
}

#[test]
fn nr_100_a_closed_connection_is_dialed_again_and_the_wait_doubles() {
    let peer = FakePeer::start(Behavior::NodeCloses);
    let (net, ..) = net_to(peer.addr, Vec::new());
    let clock = TestClock::at(T0);
    let handle = start(own_config(), net, &clock);
    assert!(until(10, || peer.accept_count() == 1));
    settle();
    assert_eq!(peer.accept_count(), 1);
    // The first wait is 5 seconds, then 10.
    clock.set(T0 + 4);
    settle();
    assert_eq!(peer.accept_count(), 1);
    clock.set(T0 + 5);
    assert!(until(10, || peer.accept_count() == 2));
    settle();
    clock.set(T0 + 5 + 9);
    settle();
    assert_eq!(peer.accept_count(), 2);
    clock.set(T0 + 5 + 10);
    assert!(until(10, || peer.accept_count() == 3));
    // Down since the connection ended, not since the start.
    let state = own_event(&handle, 10, |e| {
        matches!(
            e,
            OwnNodeEvent::State(OwnNodeInput {
                link: OwnNodeLink::Down { since },
                ..
            }) if *since == T0 + 5
        )
    });
    assert!(state.is_some());
}

#[test]
fn nr_100_it_takes_no_place_of_the_target_number_of_peers() {
    let own = FakePeer::start_on("127.0.0.2", Behavior::Node(NodeScript::default()));
    let peer = FakePeer::start(Behavior::Honest(Vec::new()));
    let (peer_addr, own_addr) = (peer.addr, own.addr);
    let log: DialLog = Arc::default();
    let l = log.clone();
    let net = Net {
        dial: Arc::new(move |target, proxy, _| {
            l.lock().unwrap().push((target.clone(), proxy));
            let to = if *target == Target::Addr(own_addr) {
                own_addr
            } else {
                peer_addr
            };
            std::net::TcpStream::connect(to).map_err(|e| DialError::Connect(e.kind()))
        }),
        resolve: Arc::new(|_, _| Ok(Vec::new())),
        proxy_resolve: Arc::new(not_tor),
    };
    let mut config = own_config();
    config.proxy = None;
    config.own_node = Some(OwnNode::clear_net_for_tests(own_addr));
    config.allow_local = true;
    config.fixed_peers = vec![peer_addr];
    config.target_peers = 1;
    let clock = TestClock::at(T0);
    let handle = start(config, net, &clock);
    assert!(own_event(&handle, 10, connected).is_some());
    // One peer, as the target says, and it is not the own node.
    let seen = wait_for(
        &handle,
        10,
        |e| matches!(e, AppEvent::Peers(p, ..) if p.len() == 1),
    );
    assert!(seen.is_some());
    assert_eq!(peer.accept_count(), 1);
    assert_eq!(own.accept_count(), 1);
}

#[test]
fn nr_100_a_violation_is_reported_and_not_retried_until_the_app_asks() {
    let peer = FakePeer::start(Behavior::NodeSendsBlock);
    let (net, ..) = net_to(peer.addr, Vec::new());
    let clock = TestClock::at(T0);
    let handle = start(own_config(), net, &clock);
    let refused = own_event(&handle, 10, |e| {
        matches!(
            e,
            OwnNodeEvent::State(OwnNodeInput {
                link: OwnNodeLink::Refused { .. },
                ..
            })
        )
    });
    assert!(refused.is_some());
    // Reported in words for a person to see, with no address in it.
    let note = wait_for(
        &handle,
        5,
        |e| matches!(e, AppEvent::Note(t) if t.contains("dropped (not dialed until asked)")),
    )
    .and_then(|events| events.into_iter().last());
    let Some(AppEvent::Note(text)) = note else {
        panic!("no note")
    };
    assert!(text.starts_with("the own node dropped"), "{text}");
    assert!(text.contains("NR-25"), "{text}");
    assert!(!text.contains(".onion"), "{text}");
    // However long it waits, it is not dialed again.
    for secs in [10, 100, 1_000, 10_000] {
        clock.set(T0 + secs);
        settle();
    }
    assert_eq!(peer.accept_count(), 1);
    // Asked, it is dialed again.
    handle.retry_own_node();
    assert!(until(10, || peer.accept_count() == 2));
}

#[test]
fn nr_100_asking_again_without_a_violation_does_nothing() {
    let peer = FakePeer::start(Behavior::Node(NodeScript::default()));
    let (net, ..) = net_to(peer.addr, Vec::new());
    let clock = TestClock::at(T0);
    let handle = start(own_config(), net, &clock);
    assert!(own_event(&handle, 10, connected).is_some());
    handle.retry_own_node();
    settle();
    assert_eq!(peer.accept_count(), 1);
}

// NR-101.

#[test]
fn nr_101_an_announced_transaction_and_a_served_one_are_reports() {
    let tx = tx_paying(5);
    let script = NodeScript {
        height: 10,
        announce: vec![txid(1)],
        mempool: vec![tx.clone()],
        ..NodeScript::default()
    };
    let peer = FakePeer::start(Behavior::Node(script));
    let (net, ..) = net_to(peer.addr, Vec::new());
    let clock = TestClock::at(T0);
    let handle = start(own_config(), net, &clock);
    assert!(own_event(&handle, 10, connected).is_some());
    // Announced: reported when asked about.
    handle.ask_own_node(txid(1));
    assert!(own_event(&handle, 5, has(txid(1))).is_some());
    // Held, not announced: sent when asked for.
    let held = tx.compute_txid();
    handle.ask_own_node(held);
    assert!(own_event(&handle, 5, has(held)).is_some());
    // Not held: nothing is reported, and the node stays connected.
    handle.ask_own_node(txid(77));
    assert!(own_event(&handle, 2, has(txid(77))).is_none());
    assert_eq!(peer.accept_count(), 1);
    // None of it was a sighting of the peers.
    while let Ok(event) = handle.events().try_recv() {
        assert!(
            !matches!(
                event,
                AppEvent::Sighting { .. } | AppEvent::AlsoAnnounced { .. }
            ),
            "{event:?}"
        );
    }
}

// NR-102.

#[test]
fn nr_102_the_state_carries_the_nodes_height_and_the_own_node_is_not_a_peer() {
    let script = NodeScript {
        height: 123,
        ..NodeScript::default()
    };
    let peer = FakePeer::start(Behavior::Node(script));
    let (net, ..) = net_to(peer.addr, Vec::new());
    let clock = TestClock::at(T0);
    let handle = start(own_config(), net, &clock);
    let state = own_event(&handle, 10, connected).expect("connected");
    let OwnNodeEvent::State(input) = state else {
        panic!("not a state")
    };
    assert_eq!(input.best_height, Some(123));
    assert_eq!(input.last_heard, Some(T0));
    // The watcher's rules read it: ahead of a counter at 0, usable.
    let health = check_own_node(Some(&input), 0, T0 + 1, None);
    assert!(health.usable, "{health:?}");
    // Behind a counter at 125: not usable, and says by how much.
    let health = check_own_node(Some(&input), 125, T0 + 1, None);
    assert_eq!(health.problem, Some(OwnNodeProblem::Behind { blocks: 2 }));
    // The `Peers` event never holds it.
    let peers = wait_for(&handle, 3, |e| matches!(e, AppEvent::Peers(..)));
    let Some(events) = peers else {
        panic!("no peers event")
    };
    for event in events {
        if let AppEvent::Peers(list, ..) = event {
            assert!(list.is_empty(), "{list:?}");
        }
    }
}

// NR-103.

#[test]
fn nr_103_the_state_comes_again_every_ten_seconds() {
    let peer = FakePeer::start(Behavior::Node(NodeScript::default()));
    let (net, ..) = net_to(peer.addr, Vec::new());
    let clock = TestClock::at(T0);
    let handle = start(own_config(), net, &clock);
    assert!(own_event(&handle, 10, connected).is_some());
    // Nothing changed: no state until 10 seconds pass.
    settle();
    while handle.own_node_events().try_recv().is_ok() {}
    settle();
    assert!(handle.own_node_events().try_recv().is_err());
    clock.set(T0 + 10);
    assert!(own_event(&handle, 5, connected).is_some());
}

#[test]
fn nr_103_a_full_channel_never_holds_the_driver() {
    // The app does not read the own node's channel; the driver goes on.
    let announce: Vec<Txid> = (0..=255u8).chain(0..=255u8).map(txid).collect();
    let script = NodeScript {
        height: 5,
        announce: announce.clone(),
        mempool: Vec::new(),
        ..NodeScript::default()
    };
    let peer = FakePeer::start(Behavior::Node(script));
    let (net, ..) = net_to(peer.addr, Vec::new());
    let clock = TestClock::at(T0);
    let handle = start(own_config(), net, &clock);
    for i in 0..(OWN_EVENT_CAPACITY + 200) {
        let mut bytes = [0u8; 32];
        bytes[..8].copy_from_slice(&(i as u64).to_le_bytes());
        handle.ask_own_node(Txid::from_byte_array(bytes));
        handle.ask_own_node(txid((i % 256) as u8));
    }
    clock.set(T0 + 1);
    // The events of the peers still come, and the driver still stops.
    assert!(wait_for(&handle, 10, |e| matches!(e, AppEvent::Peers(..))).is_some());
    settle();
    let mut count = 0;
    while handle.own_node_events().try_recv().is_ok() {
        count += 1;
    }
    assert!(count <= OWN_EVENT_CAPACITY, "{count}");
    let mut handle = handle;
    handle.shutdown();
}
