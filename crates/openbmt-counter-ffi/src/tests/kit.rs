//! Identities, a signed batch and signed settings as the app hands them over,
//! blocks mined on a real header chain, a scratch folder and a driver the
//! tests feed by hand. This is the shape of the test kit of
//! `openbmt-counter-node`, which is private to that crate.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{channel, Sender};
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
use openbmt_core::batch::{AddressBatch, SignedBatch};
use openbmt_core::identity::Identity;
use openbmt_core::settings::{Settings, SignedSettings};
use openbmt_counter_node::DriverPort;
use openbmt_net_io::{AppEvent, EventReceiver};
use openbmt_watch::{HeaderStore, Peer, PeerAddr, PeerTransport};

use crate::host::Port;
use crate::tests::wire::{frame, W};
use crate::{dispatch, CounterNetwork, Registry, StartConfig};

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

/// What the app hands `start`, for a data folder.
pub fn start_config(dir: &str) -> StartConfig {
    StartConfig {
        dir: dir.to_string(),
        network: CounterNetwork::Regtest,
        now: T0,
        identity_secret: vec![3; 32],
        owner_root: id(1).device_id().to_bytes().to_vec(),
        approver_root: id(2).device_id().to_bytes().to_vec(),
        wallet_checksum: vec![9; 32],
        batch_min_count: 1,
        settings: signed_settings().to_bytes(),
        first_batch: Some(batch(0, 10).to_bytes()),
        proxy: None,
        fixed_peers: Vec::new(),
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
}

/// A driver port whose events the test sends.
pub struct FakePort {
    rx: EventReceiver,
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
}

/// A registry whose next start runs on a fake driver, a scratch folder, and
/// the ends the test holds.
pub struct Rig {
    registry: Registry,
    scratch: Scratch,
    events: Arc<Mutex<Option<Sender<AppEvent>>>>,
    pub calls: Arc<Mutex<Calls>>,
}

impl Rig {
    pub fn new() -> Self {
        let rig = Self {
            registry: Registry::new(),
            scratch: Scratch::new(),
            events: Arc::new(Mutex::new(None)),
            calls: Arc::new(Mutex::new(Calls::default())),
        };
        rig.arm();
        rig
    }

    /// The next start runs on a new fake driver.
    pub fn arm(&self) {
        let events = Arc::clone(&self.events);
        let calls = Arc::clone(&self.calls);
        *self.registry.spawner.lock().expect("lock") = Some(Box::new(move |_launch| {
            let (tx, rx) = channel();
            *events.lock().expect("lock") = Some(tx);
            Ok(Port(Box::new(FakePort {
                rx: rx.into(),
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
