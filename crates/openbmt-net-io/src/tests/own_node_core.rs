//! NR-98 to NR-103 against Bitcoin Core on regtest, as the own node.
//!
//! Not a unit test: it needs a node, so it is ignored and is run by
//! `scripts/netio-own-node-core.sh`, which starts the node, mines blocks,
//! sets the environment below and runs this test. The own node is made with
//! the constructor of NR-99 (a clear-net address on this machine); a real
//! Tor address needs a Tor proxy and is not part of this check.
//!
//! Environment (all set by the script):
//!
//! - `OBM_CORE_CLI`: the `bitcoin-cli` binary;
//! - `OBM_CORE_DATADIR`, `OBM_CORE_RPCPORT`: the node's directory and RPC
//!   port;
//! - `OBM_CORE_P2PPORT`: the node's peer port on 127.0.0.1;
//! - `OBM_CORE_ADDR`: a wallet address of the node, to pay;
//! - `OBM_CORE_HEIGHT`: the height of the node's chain.

use std::net::SocketAddr;
use std::process::Command;
use std::time::{Duration, Instant};

use bitcoin::address::NetworkUnchecked;
use bitcoin::consensus::deserialize;
use bitcoin::hashes::Hash;
use bitcoin::{Address, Network, Transaction, Txid};
use openbmt_net::OwnNode;
use openbmt_watch::{
    check_own_node, OwnNodeInput, OwnNodeLink, OwnNodeProblem, PeerAddr, SaleWatch, Source,
};

use super::kit::*;
use crate::dial::Net;
use crate::driver::{AppEvent, Driver, DriverConfig, Handle, OwnNodeEvent};
use crate::env::SystemClock;

fn var(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("{name} is not set: run the script"))
}

/// A `bitcoin-cli` call against the node.
fn cli(args: &[&str]) -> String {
    let out = Command::new(var("OBM_CORE_CLI"))
        .arg("-regtest")
        .arg(format!("-datadir={}", var("OBM_CORE_DATADIR")))
        .arg(format!("-rpcport={}", var("OBM_CORE_RPCPORT")))
        .args(args)
        .output()
        .expect("bitcoin-cli runs");
    assert!(
        out.status.success(),
        "bitcoin-cli {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// What the app has read from the peers' events so far.
#[derive(Default)]
struct Seen {
    /// The counter's tip from the last `Peers` event.
    tip: u32,
    /// The transactions of the peers' sightings.
    sightings: Vec<Vec<u8>>,
}

impl Seen {
    /// Read every event of the peers that is waiting. The driver holds back
    /// when the app does not read them (NR-74), so every wait below reads
    /// them too.
    fn drain(&mut self, handle: &Handle) {
        while let Ok(event) = handle.events().try_recv() {
            match event {
                AppEvent::Peers(_, _, (height, _)) => self.tip = height,
                AppEvent::Sighting { tx, .. } => self.sightings.push(tx),
                _ => {}
            }
        }
    }
}

/// The next own-node event that `want` accepts, within `secs`, reading the
/// events of the peers meanwhile.
fn own_event(
    handle: &Handle,
    seen: &mut Seen,
    secs: u64,
    want: impl Fn(&OwnNodeEvent) -> bool,
) -> Option<OwnNodeEvent> {
    let end = Instant::now() + Duration::from_secs(secs);
    while Instant::now() < end {
        seen.drain(handle);
        match handle
            .own_node_events()
            .recv_timeout(Duration::from_millis(100))
        {
            Ok(event) if want(&event) => return Some(event),
            Ok(_) | Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return None,
        }
    }
    None
}

fn state_with(
    handle: &Handle,
    seen: &mut Seen,
    secs: u64,
    want: impl Fn(&OwnNodeInput) -> bool,
) -> Option<OwnNodeInput> {
    match own_event(
        handle,
        seen,
        secs,
        |e| matches!(e, OwnNodeEvent::State(s) if want(s)),
    ) {
        Some(OwnNodeEvent::State(s)) => Some(s),
        _ => None,
    }
}

#[test]
#[ignore = "needs Bitcoin Core on regtest: run scripts/netio-own-node-core.sh"]
fn own_node_core_regtest() {
    let port: u16 = var("OBM_CORE_P2PPORT").parse().expect("a port");
    let height: u32 = var("OBM_CORE_HEIGHT").parse().expect("a height");
    let pay_to: Address = var("OBM_CORE_ADDR")
        .parse::<Address<NetworkUnchecked>>()
        .expect("an address")
        .require_network(Network::Regtest)
        .expect("a regtest address");
    // The own node and the one random peer reach Core at two loopback
    // addresses, because the driver never dials the own node's address as a
    // peer (NR-99). The script has Core listen on both.
    let core = SocketAddr::from(([127, 0, 0, 1], port));
    let core_as_peer = SocketAddr::from(([127, 0, 0, 2], port));

    // The own node and, beside it, the same node as one random peer, so that
    // the counter's chain is Core's too. The sale below is checked by the
    // watcher's rules from what the driver reports.
    let mut config = DriverConfig::new(Network::Regtest, 0);
    config.allow_local = true;
    config.fixed_peers = vec![core_as_peer];
    config.own_node = Some(OwnNode::clear_net_for_tests(core));
    let mut handle = Driver::spawn_with(
        config,
        MemStorage::default(),
        SystemClock,
        CountingRandomness(7),
        vec![pay_to.script_pubkey()],
        Net::system(),
    )
    .expect("the driver starts");

    let mut seen = Seen::default();
    println!("1. connected to Core as the own node");
    let state = state_with(&handle, &mut seen, 30, |s| {
        s.link == OwnNodeLink::Connected && s.best_height == Some(height)
    })
    .expect("the own node connects and shows Core's height");
    println!("   state {state:?}");

    println!("2. the counter's chain reaches Core's tip too");
    let end = Instant::now() + Duration::from_secs(30);
    while seen.tip < height && Instant::now() < end {
        std::thread::sleep(Duration::from_millis(200));
        seen.drain(&handle);
    }
    assert_eq!(seen.tip, height, "the counter's tip");
    let health = check_own_node(Some(&state), seen.tip, now(), None);
    println!("   health {health:?}");
    assert!(health.usable, "{health:?}");
    assert_eq!(health.own_height, Some(height));
    assert_eq!(health.our_height, seen.tip);

    println!("3. a payment: the own node reports it from its memory pool");
    let txid: Txid = cli(&[
        "-rpcwallet=miner",
        "sendtoaddress",
        &pay_to.to_string(),
        "0.001",
    ])
    .parse()
    .expect("a txid");
    handle.ask_own_node(txid);
    let reported = own_event(&handle, &mut seen, 30, |e| *e == OwnNodeEvent::Has(txid));
    assert!(reported.is_some(), "Core did not report {txid}");
    println!("   Has({txid})");
    // And an ID Core does not hold is not reported.
    let unknown = Txid::from_byte_array([0xab; 32]);
    handle.ask_own_node(unknown);
    let said = own_event(&handle, &mut seen, 3, |e| *e == OwnNodeEvent::Has(unknown));
    assert!(said.is_none());
    println!("   an unknown transaction is not reported");

    // The light path, from the one peer here, saw the payment too.
    let end = Instant::now() + Duration::from_secs(30);
    while seen.sightings.is_empty() && Instant::now() < end {
        std::thread::sleep(Duration::from_millis(200));
        seen.drain(&handle);
    }
    let tx_bytes = seen
        .sightings
        .first()
        .expect("a sighting of the payment")
        .clone();
    let tx: Transaction = deserialize(&tx_bytes).expect("a transaction");
    assert_eq!(tx.compute_txid(), txid);
    // One peer is one group; the watcher needs three for the light client.
    // The two others are stood in for here, to show the mode the rules give.
    let mut watch = SaleWatch::new(pay_to.script_pubkey(), 100_000, 0);
    let groups = [
        PeerAddr::Ip("8.8.1.1".parse().unwrap()),
        PeerAddr::Ip("9.9.1.1".parse().unwrap()),
        PeerAddr::Ip("1.1.1.1".parse().unwrap()),
    ];
    for g in groups {
        watch.sighting(&tx_bytes, Source::Peer(g), now()).unwrap();
    }
    assert!(watch.own_node_saw(txid));
    let chain = openbmt_watch::HeaderStore::new(Network::Regtest).unwrap();
    let health = check_own_node(Some(&state), seen.tip, now(), None);
    let status = watch.status_with_own_node(&chain, now(), &health);
    println!(
        "   mode {:?}, own node saw it: {}, counted: {}",
        status.mode, status.own_node_saw, status.own_node_counted
    );
    assert_eq!(
        status.mode,
        Some(openbmt_core::policy::CheckMode::NodeAlsoSeenByLight)
    );
    assert!(status.own_node_saw && status.own_node_counted);

    println!("4. a block: the own node's tip moves with Core's");
    cli(&[
        "-rpcwallet=miner",
        "generatetoaddress",
        "1",
        &cli(&["-rpcwallet=miner", "getnewaddress"]),
    ]);
    let moved = state_with(&handle, &mut seen, 30, |s| {
        s.best_height == Some(height + 1)
    });
    assert!(moved.is_some(), "the own node's tip did not move");
    println!("   own node tip {}", height + 1);

    println!("5. Core stops: the own node is down, and the counter falls back");
    let before = now();
    cli(&["stop"]);
    let down = state_with(&handle, &mut seen, 60, |s| {
        matches!(s.link, OwnNodeLink::Down { .. })
    })
    .expect("the driver says the own node is down");
    let OwnNodeLink::Down { since } = down.link else {
        unreachable!()
    };
    assert!(since >= before.saturating_sub(1), "down since {since}");
    let health = check_own_node(Some(&down), seen.tip, now(), None);
    assert_eq!(health.problem, Some(OwnNodeProblem::Unreachable { since }));
    let status = watch.status_with_own_node(&chain, now(), &health);
    println!(
        "   mode {:?}, own node saw it: {}, counted: {}",
        status.mode, status.own_node_saw, status.own_node_counted
    );
    assert_eq!(status.mode, Some(openbmt_core::policy::CheckMode::Light));
    assert!(status.own_node_saw && !status.own_node_counted);
    let words = health.words().expect("words for the line");
    println!(
        "   words: {} / {} / {}",
        words.heading, words.what, words.next
    );
    assert_eq!(words.at, Some(since));
    assert!(words.what.contains("light check"));

    handle.shutdown();
    println!("ALL PASS");
}
