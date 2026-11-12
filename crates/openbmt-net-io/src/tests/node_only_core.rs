//! NR-106 to NR-110 against Bitcoin Core on regtest, with Core as the own
//! node and nothing else.
//!
//! Not a unit test: it needs two nodes, so it is ignored and is run by
//! `scripts/net-node-only-core.sh`, which starts them, mines blocks, sets the
//! environment below and runs this test. The own node is made with the
//! constructor of NR-99 (a clear-net address on this machine); a real Tor
//! address needs a Tor proxy and is not part of this check.
//!
//! Environment (all set by the script):
//!
//! - `OBM_CORE_CLI`: the `bitcoin-cli` binary;
//! - `OBM_CORE_DATADIR`, `OBM_CORE_RPCPORT`: the first node's directory and
//!   RPC port. It serves compact filters (`-peerblockfilters=1`);
//! - `OBM_CORE_P2PPORT`: its peer port on 127.0.0.1 (it listens on
//!   127.0.0.2 too, where nothing may connect);
//! - `OBM_CORE2_P2PPORT`: the peer port of a second node on 127.0.0.1 that
//!   does not serve compact filters;
//! - `OBM_CORE_ADDR`: a wallet address of the first node, to pay;
//! - `OBM_CORE_HEIGHT`: the height of its chain;
//! - `OBM_CORE_RAWTX`, `OBM_CORE_RAWTXID`: a signed transaction of the first
//!   node's wallet that it has not broadcast, and its ID.

use std::net::SocketAddr;
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use bitcoin::address::NetworkUnchecked;
use bitcoin::consensus::deserialize;
use bitcoin::hex::FromHex;
use bitcoin::{Address, Block, Network, Txid};
use openbmt_net::OwnNode;
use openbmt_watch::{NodeOnlyFault, NodeOnlyInput, NodeOnlyTracker, OwnNodeLink};

use super::kit::*;
use crate::dial::Net;
use crate::driver::{AppEvent, Driver, DriverConfig, Handle, NetMode, OwnNodeEvent};
use crate::env::SystemClock;
use crate::socks::Target;

fn var(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("{name} is not set: run the script"))
}

/// A `bitcoin-cli` call against the first node.
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

/// What the app has read so far.
#[derive(Default)]
struct Seen {
    tip: u32,
    checked: u32,
    blocks: Vec<Vec<u8>>,
    sightings: Vec<Vec<u8>>,
    has: Vec<Txid>,
    state: Option<NodeOnlyInput>,
}

impl Seen {
    /// Read everything that is waiting. The driver holds back when the app
    /// does not read (NR-74), so every wait reads.
    fn drain(&mut self, handle: &Handle) {
        while let Ok(event) = handle.events().try_recv() {
            match event {
                AppEvent::Peers(_, _, (height, _)) => self.tip = height,
                AppEvent::Checked(h) => self.checked = h,
                AppEvent::Block { bytes, .. } => self.blocks.push(bytes),
                AppEvent::Sighting { tx, .. } => self.sightings.push(tx),
                _ => {}
            }
        }
        while let Ok(event) = handle.own_node_events().try_recv() {
            match event {
                OwnNodeEvent::Has(txid) => self.has.push(txid),
                OwnNodeEvent::NodeOnly(input) => self.state = Some(input),
                OwnNodeEvent::State(_) => panic!("a State event with the mode on"),
            }
        }
    }

    /// Wait up to `secs` for the condition.
    fn wait(&mut self, handle: &Handle, secs: u64, cond: impl Fn(&Seen) -> bool) -> bool {
        let end = Instant::now() + Duration::from_secs(secs);
        loop {
            self.drain(handle);
            if cond(self) {
                return true;
            }
            if Instant::now() >= end {
                return false;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}

/// The system's net, with every dial written down.
fn recording_net() -> (Net, Arc<Mutex<Vec<Target>>>) {
    let system = Net::system();
    let log: Arc<Mutex<Vec<Target>>> = Arc::default();
    let l = log.clone();
    let dial = system.dial.clone();
    let net = Net {
        dial: Arc::new(move |target, proxy, token| {
            l.lock().unwrap().push(target.clone());
            dial(target, proxy, token)
        }),
        resolve: system.resolve.clone(),
        proxy_resolve: system.proxy_resolve.clone(),
    };
    (net, log)
}

fn node_only_config(own: SocketAddr) -> DriverConfig {
    let mut config = DriverConfig::new(Network::Regtest, 0);
    config.allow_local = true;
    config.own_node = Some(OwnNode::clear_net_for_tests(own));
    config.mode = NetMode::NodeOnly {
        light_fallback: false,
    };
    config
}

#[test]
#[ignore = "needs Bitcoin Core on regtest: run scripts/net-node-only-core.sh"]
fn node_only_core_regtest() {
    let port: u16 = var("OBM_CORE_P2PPORT").parse().expect("a port");
    let port2: u16 = var("OBM_CORE2_P2PPORT").parse().expect("a port");
    let height: u32 = var("OBM_CORE_HEIGHT").parse().expect("a height");
    let pay_to: Address = var("OBM_CORE_ADDR")
        .parse::<Address<NetworkUnchecked>>()
        .expect("an address")
        .require_network(Network::Regtest)
        .expect("a regtest address");
    let core = SocketAddr::from(([127, 0, 0, 1], port));
    let core_elsewhere = SocketAddr::from(([127, 0, 0, 2], port));

    // The own node is the only peer: a fixed peer, set to another address of
    // the same Core, must never be dialed.
    let mut config = node_only_config(core);
    config.fixed_peers = vec![core_elsewhere];
    let (net, log) = recording_net();
    let mut handle = Driver::spawn_with(
        config,
        MemStorage::default(),
        SystemClock,
        CountingRandomness(11),
        vec![pay_to.script_pubkey()],
        net,
    )
    .expect("the driver starts");
    let mut seen = Seen::default();

    println!("1. Core is the own node and the only peer");
    assert!(
        seen.wait(&handle, 30, |s| s.state.is_some_and(|i| {
            i.node.link == OwnNodeLink::Connected && i.node.best_height == Some(height)
        })),
        "the own node did not connect and show Core's height"
    );
    let peers = cli(&["getpeerinfo"]);
    let count = peers.matches("\"addr\"").count();
    assert_eq!(count, 1, "Core sees {count} peers: {peers}");
    assert!(
        log.lock().unwrap().iter().all(|t| *t == Target::Addr(core)),
        "dialed {:?}",
        log.lock().unwrap()
    );
    println!(
        "PASS  connected to nothing but Core: 1 peer at Core, dials {:?}",
        log.lock().unwrap()
    );

    println!("2. headers and filters: the chain and every block checked");
    assert!(
        seen.wait(&handle, 60, |s| s.tip == height && s.checked >= height),
        "tip {} checked {} of {height}",
        seen.tip,
        seen.checked
    );
    // Every block of the chain was settled by its one filter: none paid the
    // watched script, so no block was downloaded.
    assert!(
        seen.blocks.is_empty(),
        "{} blocks fetched",
        seen.blocks.len()
    );
    println!(
        "PASS  headers to {}, {} blocks settled by one filter each, no block fetched",
        seen.tip, seen.checked
    );

    println!("3. a payment in the mempool: Core announces it, the node reports it");
    let txid: Txid = cli(&[
        "-rpcwallet=miner",
        "sendtoaddress",
        &pay_to.to_string(),
        "0.001",
    ])
    .parse()
    .expect("a txid");
    assert!(
        seen.wait(&handle, 60, |s| s.has.contains(&txid)
            && s.sightings
                .iter()
                .any(|b| deserialize::<bitcoin::Transaction>(b)
                    .is_ok_and(|t| t.compute_txid() == txid))),
        "no report of {txid}"
    );
    handle.ask_own_node(txid);
    println!("PASS  {txid} announced by Core: reported, and fetched from Core");

    println!("4. a block that pays a watched script: filter, then the block");
    let mine = cli(&["-rpcwallet=miner", "getnewaddress"]);
    cli(&["-rpcwallet=miner", "generatetoaddress", "1", &mine]);
    assert!(
        seen.wait(&handle, 60, |s| !s.blocks.is_empty() && s.tip == height + 1),
        "the block was not fetched"
    );
    let block: Block = deserialize(&seen.blocks[0]).expect("a block");
    assert!(block.txdata.iter().any(|t| t.compute_txid() == txid));
    println!(
        "PASS  block {} fetched on one matching filter, with {txid}",
        block.block_hash()
    );
    assert!(seen.wait(&handle, 30, |s| s.checked == height + 1));

    println!("5. a broadcast: Core accepts the transaction");
    let raw = Vec::<u8>::from_hex(&var("OBM_CORE_RAWTX")).expect("hex");
    let raw_id: Txid = var("OBM_CORE_RAWTXID").parse().expect("a txid");
    handle.broadcast(raw);
    let end = Instant::now() + Duration::from_secs(30);
    let mut accepted = false;
    while Instant::now() < end {
        seen.drain(&handle);
        if cli(&["getrawmempool"]).contains(&raw_id.to_string()) {
            accepted = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    assert!(accepted, "Core did not accept {raw_id}");
    assert!(
        log.lock().unwrap().iter().all(|t| *t == Target::Addr(core)),
        "dialed {:?}",
        log.lock().unwrap()
    );
    println!("PASS  {raw_id} sent to Core only, and in Core's mempool");
    handle.shutdown();

    println!("6. a node without compact filters is reported");
    let second = SocketAddr::from(([127, 0, 0, 1], port2));
    let (net, log) = recording_net();
    let mut handle = Driver::spawn_with(
        node_only_config(second),
        MemStorage::default(),
        SystemClock,
        CountingRandomness(12),
        vec![pay_to.script_pubkey()],
        net,
    )
    .expect("the driver starts");
    let mut seen = Seen::default();
    assert!(
        seen.wait(&handle, 30, |s| s.state.is_some_and(|i| matches!(
            i.fault,
            Some(NodeOnlyFault::MissingFilters { .. })
        ))),
        "the missing filters were not reported"
    );
    let input = seen.state.expect("a state");
    let health = NodeOnlyTracker::new().check(Some(&input), 0, now());
    assert!(!health.own.usable);
    println!("   problem {:?}, line {:?}", health.problem, health.line());
    assert!(log
        .lock()
        .unwrap()
        .iter()
        .all(|t| *t == Target::Addr(second)));
    println!("PASS  a node without NODE_COMPACT_FILTERS: missing-filters state, not usable");
    handle.shutdown();

    println!("ALL PASS");
}
