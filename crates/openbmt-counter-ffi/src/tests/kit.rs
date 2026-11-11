//! Identities, a signed batch and signed settings as the app hands them over,
//! blocks mined on a real header chain, a scratch folder and a driver the
//! tests feed by hand. This is the shape of the test kit of
//! `openbmt-counter-node`, which is private to that crate.

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
use openbmt_core::batch::{AddressBatch, PinnedRoots, SignedBatch};
use openbmt_core::identity::Identity;
use openbmt_core::settings::{Settings, SignedSettings};
use openbmt_counter_node::{DriverPort, Launch, PairingRecord};
use openbmt_net_io::{AppEvent, EventReceiver, OwnNodeEvent};
use openbmt_watch::{HeaderStore, Peer, PeerAddr, PeerTransport};

use crate::host::Port;
use crate::tests::wire::{frame, W};
use crate::{dispatch, Registry, StartConfig};

/// The first moment of the app's clock.
pub const T0: u64 = 1_700_000_000;
/// $100,000 per bitcoin, in cents: 1,000 sats per dollar.
pub const RATE: u64 = 10_000_000;
/// $5.00, which needs 5,000 satoshis at [`RATE`].
pub const FIVE_DOLLARS: u64 = 500;

/// A numbered identity: the owner (1), the Key app (2), the counter (3).
pub fn id(n: u8) -> Identity {
    Identity::from_secret_bytes(&[n; 32]).expect("a key")
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

/// The shop's settings, signed by both roots as first settings must be: no
/// floor on sales, and customers who cannot join the Wi-Fi.
pub fn signed_settings() -> SignedSettings {
    let mut s = Settings::defaults(1_000);
    s.version = 1;
    s.customer_wifi = false;
    s.min_sale_sats = 0;
    s.devices.insert(id(3).device_id());
    s.counters.insert(id(3).device_id(), 10_000);
    SignedSettings {
        owner_sig: s.sign(&id(1)),
        approver_sig: Some(s.sign(&id(2))),
        settings: s,
    }
}

/// The pairing record of the test counter (identity 3) on regtest, with
/// these settings and no peers: what `pair_finish` returns, built by hand.
pub fn record_with(settings: SignedSettings) -> PairingRecord {
    PairingRecord {
        identity_secret: [3; 32],
        roots: PinnedRoots::new(id(1).device_id(), id(2).device_id()).expect("roots"),
        wallet: [9; 32],
        network: Network::Regtest,
        settings_floor: 0,
        settings,
        first_batch: Some(batch(0, 10)),
        peers: Vec::new(),
    }
}

/// The record of the test counter.
pub fn record() -> PairingRecord {
    record_with(signed_settings())
}

/// What the app hands `start`, for a data folder.
pub fn start_config(dir: &str) -> StartConfig {
    StartConfig {
        dir: dir.to_string(),
        now: T0,
        record: record().to_bytes(),
        proxy: None,
        own_node: None,
        lightning: None,
    }
}

/// The request frame of `start`.
pub fn start_request(dir: &Path) -> Vec<u8> {
    frame(
        1,
        0,
        &W::new()
            .start(&start_config(dir.to_str().expect("UTF-8")))
            .0,
    )
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

fn coinbase(height: u32) -> Transaction {
    let mut script = height.to_le_bytes().to_vec();
    script.extend_from_slice(&0u32.to_le_bytes());
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

/// A block at `height` on top of `prev` with these transactions, mined to
/// the regtest target.
pub fn mine(prev: BlockHash, height: u32, txs: &[Transaction]) -> Block {
    let time = genesis_block(Network::Regtest).header.time + height * 10;
    let mut all = vec![coinbase(height)];
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
    block.header.merkle_root = block.compute_merkle_root().expect("a root");
    while block.header.validate_pow(block.header.target()).is_err() {
        block.header.nonce += 1;
    }
    block
}

/// The headers of these blocks as a stored header file holds them, which a
/// real driver writes as it stops.
pub fn stored_headers(blocks: &[Block]) -> Vec<u8> {
    let mut store = HeaderStore::new(Network::Regtest).expect("a store");
    for block in blocks {
        store.accept(&block.header, u64::MAX / 2).expect("accepted");
    }
    store.to_bytes()
}

/// The `Chain` event for one block joining the chain.
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

/// A `Sighting` event for a transaction seen by these peers.
pub fn sighting(tx: &Transaction, from: Vec<PeerAddr>) -> AppEvent {
    AppEvent::Sighting {
        tx: serialize(tx),
        from,
    }
}

/// A peer in its own /16 for each `n` from 20 up.
pub fn peer(n: u8) -> PeerAddr {
    PeerAddr::Ip(format!("{n}.{n}.1.1").parse().expect("an address"))
}

/// Three peers in three network groups that serve filters, at the tip of
/// the first moment: the real health rules say healthy.
pub fn peers_event() -> AppEvent {
    let peers = (20..23)
        .map(|n| Peer {
            addr: peer(n),
            serves_filters: true,
            best_height: 0,
            transport: PeerTransport::V1,
        })
        .collect();
    AppEvent::Peers(peers, None, (0, T0))
}

// A scratch folder.

/// A directory that is removed when it goes out of scope.
pub struct Scratch(PathBuf);

impl Scratch {
    /// A new, empty directory.
    pub fn new() -> Self {
        static N: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "openbmt-counter-ffi-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("a directory");
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
    pub asked: Vec<Txid>,
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
        self.calls.lock().expect("lock").watched.push(script);
    }

    fn set_all_blocks(&self, on: bool) {
        self.calls.lock().expect("lock").all_blocks.push(on);
    }

    fn shutdown(&mut self) {
        self.calls.lock().expect("lock").shutdown = true;
    }

    fn own_node_events(&self) -> Option<&Receiver<OwnNodeEvent>> {
        Some(&self.own_rx)
    }

    fn ask_own_node(&self, txid: Txid) {
        self.calls.lock().expect("lock").asked.push(txid);
    }

    fn retry_own_node(&self) {
        self.calls.lock().expect("lock").retried += 1;
    }
}

/// A registry whose next start runs on a fake driver, a scratch folder, and
/// the ends the test holds.
pub struct Rig {
    registry: Registry,
    scratch: Scratch,
    events: Arc<Mutex<Option<Sender<AppEvent>>>>,
    own: Arc<Mutex<Option<Sender<OwnNodeEvent>>>>,
    launch: Arc<Mutex<Option<Launch>>>,
    pub calls: Arc<Mutex<Calls>>,
}

impl Rig {
    pub fn new() -> Self {
        let rig = Self {
            registry: Registry::new(),
            scratch: Scratch::new(),
            events: Arc::new(Mutex::new(None)),
            own: Arc::new(Mutex::new(None)),
            launch: Arc::new(Mutex::new(None)),
            calls: Arc::new(Mutex::new(Calls::default())),
        };
        rig.arm();
        rig
    }

    /// The next start runs on a new fake driver.
    pub fn arm(&self) {
        let events = Arc::clone(&self.events);
        let own = Arc::clone(&self.own);
        let seen = Arc::clone(&self.launch);
        let calls = Arc::clone(&self.calls);
        *self.registry.spawner.lock().expect("lock") = Some(Box::new(move |launch| {
            let (tx, rx) = channel();
            let (own_tx, own_rx) = channel();
            *events.lock().expect("lock") = Some(tx);
            *own.lock().expect("lock") = Some(own_tx);
            *seen.lock().expect("lock") = Some(launch);
            Ok(Port(Box::new(FakePort {
                rx: rx.into(),
                own_rx,
                calls,
            })))
        }));
    }

    pub fn registry(&self) -> &Registry {
        &self.registry
    }

    pub fn dir(&self) -> PathBuf {
        self.scratch.path().to_path_buf()
    }

    /// Send events to the node's driver port.
    pub fn send(&self, events: Vec<AppEvent>) {
        let guard = self.events.lock().expect("lock");
        let tx = guard.as_ref().expect("a started node");
        for event in events {
            tx.send(event).expect("sent");
        }
    }

    /// Send events on the own node's channel.
    pub fn send_own(&self, events: Vec<OwnNodeEvent>) {
        let guard = self.own.lock().expect("lock");
        let tx = guard.as_ref().expect("a started node");
        for event in events {
            tx.send(event).expect("sent");
        }
    }

    /// What the driver was last started with.
    pub fn launch(&self) -> Launch {
        self.launch
            .lock()
            .expect("lock")
            .clone()
            .expect("a started node")
    }

    /// One call.
    pub fn call(&self, op: u16, handle: u64, fields: &[u8]) -> Vec<u8> {
        dispatch(&self.registry, &frame(op, handle, fields))
    }

    /// Start the node from this configuration and return its handle.
    pub fn start_with(&self, config: &StartConfig) -> u64 {
        let response = self.call(1, 0, &crate::tests::wire::W::new().start(config).0);
        crate::tests::wire::Rd::ok(&response).u64()
    }

    /// Start the node and return its handle.
    pub fn start(&self) -> u64 {
        let response = dispatch(&self.registry, &start_request(&self.dir()));
        crate::tests::wire::Rd::ok(&response).u64()
    }
}
