//! Identities, batches, mined blocks on a real header chain, a scratch
//! directory, a driver that tests feed by hand and a node on top of them.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};

use bitcoin::absolute::LockTime;
use bitcoin::block::{Header, Version as BlockVersion};
use bitcoin::blockdata::constants::genesis_block;
use bitcoin::consensus::encode::serialize;
use bitcoin::hashes::Hash;
use bitcoin::transaction::Version;
use bitcoin::{
    Amount as Sats, Block, BlockHash, CompactTarget, Network, OutPoint, ScriptBuf, Sequence,
    Transaction, TxIn, TxMerkleNode, TxOut, Txid, WScriptHash, Witness,
};
use openbmt_core::batch::{AddressBatch, BatchPolicy, PinnedRoots, SignedBatch};
use openbmt_core::identity::Identity;
use openbmt_core::settings::Settings;
use openbmt_counter::{Amount, Currency, Pairing, Rate, RateSource};
use openbmt_net_io::{AppEvent, Clock, DriverConfig, EventReceiver, OwnNode, OwnNodeEvent};
use openbmt_watch::{HeaderStore, Health, PeerAddr};

use crate::{DriverPort, Launch, Node, NodeConfig};

/// The first moment of the node's clock.
pub const T0: u64 = 1_700_000_000;
/// Satoshis per dollar in the test settings.
pub const SATS_PER_USD: u64 = 1_000;

/// A numbered identity.
pub fn id(n: u8) -> Identity {
    Identity::from_secret_bytes(&[n; 32]).unwrap()
}

/// The owner (1) and the Key app (2).
pub fn roots() -> PinnedRoots {
    PinnedRoots::new(id(1).device_id(), id(2).device_id()).unwrap()
}

/// $100,000 per bitcoin, in cents: 1,000 sats per dollar.
pub fn rate() -> Rate {
    Rate::new(10_000_000).unwrap()
}

/// A rate source whose rate was fetched at the clock's time.
pub fn source_at(time: &Arc<AtomicU64>) -> RateSource {
    RateSource::new("test-fixed", time.load(Ordering::SeqCst)).unwrap()
}

/// A rate source whose rate was fetched at `now`.
pub fn fresh(now: u64) -> RateSource {
    RateSource::new("test-fixed", now).unwrap()
}

/// Dollars.
pub fn usd() -> Currency {
    Currency::new(*b"USD").unwrap()
}

/// $5.00, which needs 5,000 satoshis at [`rate`].
pub fn five_dollars() -> Amount {
    Amount(500)
}

/// Settings for counter 3 with light checks that customers on the Wi-Fi
/// cannot reach.
pub fn settings() -> Settings {
    let mut s = Settings::defaults(SATS_PER_USD);
    s.customer_wifi = false;
    // No floor, so small sales open (CR-75).
    s.min_sale_sats = 0;
    s.devices.insert(id(3).device_id());
    s.counters.insert(id(3).device_id(), 10_000);
    s
}

/// The pairing of the test counter (identity 3).
pub fn pairing() -> Pairing {
    Pairing {
        roots: roots(),
        wallet_checksum: [9; 32],
        batch_policy: BatchPolicy { min_count: 1 },
    }
}

/// A script that is P2WSH-shaped for the byte `n`.
pub fn p2wsh(n: u8) -> ScriptBuf {
    ScriptBuf::new_p2wsh(&WScriptHash::from_byte_array([n; 32]))
}

/// The script of the sale with receive index `index` in [`batch`].
pub fn script_of(index: u32) -> ScriptBuf {
    p2wsh(index as u8 + 1)
}

/// A signed batch of `count` addresses from `start` for the test counter.
pub fn batch(start: u32, count: u32) -> SignedBatch {
    let batch = AddressBatch {
        wallet_checksum: [9; 32],
        counter: id(3).device_id(),
        start,
        count,
        scripts: (start..start + count).map(script_of).collect(),
    };
    SignedBatch {
        owner_sig: batch.sign(&id(1)),
        approver_sig: batch.sign(&id(2)),
        batch,
    }
}

/// A peer in its own /16 for each `n` from 20 up.
pub fn peer(n: u8) -> PeerAddr {
    PeerAddr::Ip(format!("{n}.{n}.1.1").parse().unwrap())
}

/// A health answer that is healthy.
pub fn healthy() -> Health {
    Health {
        healthy: true,
        problem: None,
        behind: false,
    }
}

// Transactions and blocks.

/// A transaction spending a coin that is not null, `prev`, and paying each
/// (script, sats).
pub fn spend(prev: u8, outs: &[(&ScriptBuf, u64)]) -> Transaction {
    Transaction {
        version: Version::TWO,
        lock_time: LockTime::ZERO,
        input: vec![TxIn {
            previous_output: OutPoint {
                txid: Txid::from_byte_array([prev; 32]),
                vout: 0,
            },
            script_sig: ScriptBuf::new(),
            sequence: Sequence::MAX,
            witness: Witness::new(),
        }],
        output: outs
            .iter()
            .map(|(s, v)| TxOut {
                value: Sats::from_sat(*v),
                script_pubkey: (*s).clone(),
            })
            .collect(),
    }
}

/// The bytes of a transaction.
pub fn bytes(tx: &Transaction) -> Vec<u8> {
    serialize(tx)
}

fn coinbase(height: u32, tag: u32) -> Transaction {
    let mut script = height.to_le_bytes().to_vec();
    script.extend_from_slice(&tag.to_le_bytes());
    Transaction {
        version: Version::TWO,
        lock_time: LockTime::ZERO,
        input: vec![TxIn {
            previous_output: OutPoint::null(),
            script_sig: ScriptBuf::from_bytes(script),
            sequence: Sequence::MAX,
            witness: Witness::new(),
        }],
        output: vec![TxOut {
            value: Sats::from_sat(5_000_000_000),
            script_pubkey: p2wsh(0xee),
        }],
    }
}

/// The hash of the regtest genesis block.
pub fn genesis() -> BlockHash {
    genesis_block(Network::Regtest).block_hash()
}

/// A block at `height` on top of `prev` with these transactions, mined to the
/// regtest target. Its time rises with the height, so that it passes the
/// header store's median-time rule. `tag` makes two blocks of one height
/// different.
pub fn mine(prev: BlockHash, height: u32, tag: u32, txs: &[Transaction]) -> Block {
    let time = genesis_block(Network::Regtest).header.time + height * 10;
    let mut all = vec![coinbase(height, tag)];
    all.extend(txs.iter().cloned());
    let mut block = Block {
        header: Header {
            version: BlockVersion::ONE,
            prev_blockhash: prev,
            merkle_root: TxMerkleNode::all_zeros(),
            time,
            bits: CompactTarget::from_consensus(0x207f_ffff),
            nonce: 0,
        },
        txdata: all,
    };
    block.header.merkle_root = block.compute_merkle_root().unwrap();
    while block.header.validate_pow(block.header.target()).is_err() {
        block.header.nonce += 1;
    }
    block
}

/// A chain of blocks from the genesis block, for tests to take from.
#[derive(Debug, Clone, Default)]
pub struct Blocks {
    blocks: Vec<Block>,
}

impl Blocks {
    /// The block at `height` (the genesis block is not in the list).
    pub fn at(&self, height: u32) -> &Block {
        &self.blocks[height as usize - 1]
    }

    /// The tip height.
    pub fn height(&self) -> u32 {
        self.blocks.len() as u32
    }

    /// The hash of the tip, or of the genesis block.
    pub fn tip_hash(&self) -> BlockHash {
        self.blocks.last().map_or_else(genesis, Block::block_hash)
    }

    /// Mine one more block with these transactions.
    pub fn extend(&mut self, txs: &[Transaction]) -> &Block {
        let height = self.height() + 1;
        let block = mine(self.tip_hash(), height, 0, txs);
        self.blocks.push(block);
        &self.blocks[height as usize - 1]
    }

    /// The headers as a stored header file holds them.
    pub fn stored_headers(&self) -> Vec<u8> {
        let mut store = HeaderStore::new(Network::Regtest).unwrap();
        for block in &self.blocks {
            store.accept(&block.header, u64::MAX / 2).unwrap();
        }
        store.to_bytes()
    }
}

/// The `Chain` event for one block joining on top of the chain so far.
pub fn joined(height: u32, block: &Block) -> AppEvent {
    AppEvent::Chain {
        joined: vec![(height, block.block_hash())],
        left: vec![],
    }
}

/// The `Block` event for a block.
pub fn block_event(block: &Block) -> AppEvent {
    AppEvent::Block {
        hash: block.block_hash(),
        bytes: serialize(block),
    }
}

// A scratch directory.

/// A directory that is removed when it goes out of scope.
pub struct Scratch(PathBuf);

impl Scratch {
    /// A new, empty directory.
    pub fn new() -> Self {
        static N: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "openbmt-counter-node-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    /// The directory.
    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

// A driver that tests feed by hand.

/// What the node asked of the fake driver.
#[derive(Debug, Default)]
pub struct Calls {
    pub watched: Vec<ScriptBuf>,
    pub all_blocks: Vec<bool>,
    pub shutdown: bool,
    /// The transactions the node asked the own node about, in order.
    pub asked: Vec<Txid>,
    /// How many times the node asked to dial the own node again.
    pub retried: u32,
}

/// A driver port whose events the test sends.
pub struct FakePort {
    rx: EventReceiver,
    own_rx: Receiver<OwnNodeEvent>,
    calls: Arc<Mutex<Calls>>,
}

impl DriverPort for FakePort {
    fn events(&self) -> &EventReceiver {
        &self.rx
    }

    fn watch_script(&self, script: ScriptBuf) {
        self.calls.lock().unwrap().watched.push(script);
    }

    fn set_all_blocks(&self, on: bool) {
        self.calls.lock().unwrap().all_blocks.push(on);
    }

    fn shutdown(&mut self) {
        self.calls.lock().unwrap().shutdown = true;
    }

    fn own_node_events(&self) -> Option<&Receiver<OwnNodeEvent>> {
        Some(&self.own_rx)
    }

    fn ask_own_node(&self, txid: Txid) {
        self.calls.lock().unwrap().asked.push(txid);
    }

    fn retry_own_node(&self) {
        self.calls.lock().unwrap().retried += 1;
    }
}

/// A clock the test sets.
#[derive(Clone)]
pub struct TestClock(pub Arc<AtomicU64>);

impl Clock for TestClock {
    fn now(&self) -> u64 {
        self.0.load(Ordering::SeqCst)
    }
}

/// A node on a fake driver, and the ends the test holds.
pub struct Rig {
    pub node: Node<FakePort>,
    pub events: Sender<AppEvent>,
    pub own: Sender<OwnNodeEvent>,
    pub calls: Arc<Mutex<Calls>>,
    pub time: Arc<AtomicU64>,
    pub launch: Launch,
}

impl Rig {
    /// Send events and let the node take them.
    pub fn feed(&mut self, events: Vec<AppEvent>) -> Vec<openbmt_counter::Report> {
        for event in events {
            self.events.send(event).unwrap();
        }
        self.node.pump(std::time::Duration::ZERO)
    }

    /// Send own-node events and let the node take them.
    pub fn feed_own(&mut self, events: Vec<OwnNodeEvent>) -> Vec<openbmt_counter::Report> {
        for event in events {
            self.own.send(event).unwrap();
        }
        self.node.pump(std::time::Duration::ZERO)
    }

    /// The `all_blocks` calls so far.
    pub fn all_blocks(&self) -> Vec<bool> {
        self.calls.lock().unwrap().all_blocks.clone()
    }

    /// A `Peers` event with no peers (the test health rule is "healthy").
    pub fn peers() -> AppEvent {
        AppEvent::Peers(vec![], None, (0, 0))
    }
}

/// A node configuration for the test counter on `dir`, with a health rule
/// that always says healthy.
pub fn config(dir: &Path, time: &Arc<AtomicU64>) -> NodeConfig {
    let mut config = NodeConfig::new(
        dir,
        id(3),
        pairing(),
        settings(),
        DriverConfig::new(Network::Regtest, 0),
    );
    config.first_batch = Some(batch(0, 10));
    config.clock = Box::new(TestClock(time.clone()));
    config.health = Box::new(|_| healthy());
    config
}

/// Start a node on a fake driver over `dir`, at the first moment.
pub fn start(dir: &Path) -> Rig {
    start_at(dir, Arc::new(AtomicU64::new(T0)))
}

/// Start a node on a fake driver over `dir`, with this clock.
pub fn start_at(dir: &Path, time: Arc<AtomicU64>) -> Rig {
    start_with(config(dir, &time), time)
}

/// Start a node from a configuration on a fake driver. `time` is the number
/// the configuration's clock reads.
pub fn start_with(config: NodeConfig, time: Arc<AtomicU64>) -> Rig {
    try_start_with(config, time).unwrap()
}

/// [`start_with`] that gives the error of a start that is refused.
pub fn try_start_with(
    config: NodeConfig,
    time: Arc<AtomicU64>,
) -> Result<Rig, crate::node::StartError> {
    let (events, rx) = channel();
    let (own, own_rx) = channel();
    let calls = Arc::new(Mutex::new(Calls::default()));
    let seen: Arc<Mutex<Option<Launch>>> = Arc::new(Mutex::new(None));
    let port_calls = calls.clone();
    let seen_launch = seen.clone();
    let node = Node::start_with(config, move |launch| {
        *seen_launch.lock().unwrap() = Some(launch);
        Ok(FakePort {
            rx: rx.into(),
            own_rx,
            calls: port_calls,
        })
    })
    ?;
    let launch = seen.lock().unwrap().take().unwrap();
    Ok(Rig {
        node,
        events,
        own,
        calls,
        time,
        launch,
    })
}

/// A Tor v3 name (the key is 32 bytes of 7).
pub const OWN_NAME: &str = "a4dqobyha4dqobyha4dqobyha4dqobyha4dqobyha4dqobyha4dwc6ad.onion";

/// The own node of the tests, at the usual regtest port.
pub fn own_node() -> OwnNode {
    OwnNode::onion(OWN_NAME, 18_444).unwrap()
}

/// A node configuration as [`config`], with an own node and a Tor proxy.
pub fn config_with_own_node(dir: &Path, time: &Arc<AtomicU64>) -> NodeConfig {
    let mut config = config(dir, time);
    config.driver.proxy = Some("127.0.0.1:9050".parse().unwrap());
    config.driver.own_node = Some(own_node());
    config
}

/// Start a node with an own node on a fake driver over `dir`.
pub fn start_with_own_node(dir: &Path) -> Rig {
    let time = Arc::new(AtomicU64::new(T0));
    start_with(config_with_own_node(dir, &time), time)
}

/// An own-node state: connected, at this height, heard at `heard`.
pub fn own_connected(height: u32, heard: u64) -> OwnNodeEvent {
    OwnNodeEvent::State(openbmt_watch::OwnNodeInput {
        link: openbmt_watch::OwnNodeLink::Connected,
        best_height: Some(height),
        last_heard: Some(heard),
    })
}

/// An own-node state: not connected since `since`.
pub fn own_down(since: u64) -> OwnNodeEvent {
    OwnNodeEvent::State(openbmt_watch::OwnNodeInput {
        link: openbmt_watch::OwnNodeLink::Down { since },
        best_height: None,
        last_heard: None,
    })
}
