//! Three keys and the wallet they make, a Key app, blocks mined on a real
//! header chain, a scratch folder and drivers the tests feed by hand. This
//! is the shape of the test kit of `openbmt-owner-node`, which is private to
//! that crate.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex, OnceLock};

use bitcoin::absolute::LockTime;
use bitcoin::bip32::{DerivationPath, Xpriv, Xpub};
use bitcoin::block::{Header, Version as BlockVersion};
use bitcoin::blockdata::constants::genesis_block;
use bitcoin::consensus::encode::{deserialize, serialize};
use bitcoin::hashes::Hash;
use bitcoin::secp256k1::{All, Secp256k1};
use bitcoin::transaction::Version;
use bitcoin::{
    Address, Amount as Sats, Block, BlockHash, CompactTarget, Network, OutPoint, ScriptBuf,
    Sequence, Transaction, TxIn, TxMerkleNode, TxOut, Txid, WPubkeyHash, WScriptHash, Witness,
};
use openbmt_approver::{Approver, Choice};
use openbmt_core::batch::BatchPolicy;
use openbmt_core::identity::{DeviceId, Identity};
use openbmt_core::wallet::{KeySlot, WalletDescription};
use openbmt_counter_node::DriverPort;
use openbmt_net_io::{AppEvent, EventReceiver};
use openbmt_owner_node::OwnerPort;
use openbmt_signer::SignerState;

use crate::host::Port;
use crate::tests::wire::{frame, Rd, W};
use crate::{dispatch, OwnerNetwork, Registry, StartConfig};

/// The first moment of the app's clock.
pub const T0: u64 = 1_700_000_000;
/// The network of every test.
pub const NET: Network = Network::Regtest;
/// The owner root's secret.
pub const OWNER_SECRET: [u8; 32] = [0x11; 32];
/// The Key app root's secret.
pub const APPROVER_SECRET: [u8; 32] = [0x22; 32];

/// A numbered identity: the owner (0x11), the Key app (0x22), the counter
/// (0x31).
pub fn id(n: u8) -> Identity {
    Identity::from_secret_bytes(&[n; 32]).expect("a key")
}

/// The test counter's root.
pub fn counter() -> DeviceId {
    id(0x31).device_id()
}

/// The test counter's root as the app hands it over.
pub fn counter_bytes() -> Vec<u8> {
    counter().to_bytes().to_vec()
}

/// The policy of the test wallet's batches.
pub fn policy() -> BatchPolicy {
    BatchPolicy { min_count: 3 }
}

fn secp() -> &'static Secp256k1<All> {
    static CONTEXT: OnceLock<Secp256k1<All>> = OnceLock::new();
    CONTEXT.get_or_init(Secp256k1::new)
}

/// Three keys from fixed seeds, and the wallet they make.
pub struct Kit {
    pub accounts: [Xpriv; 3],
    pub xpubs: [Xpub; 3],
    pub wallet: WalletDescription,
}

pub fn kit() -> &'static Kit {
    static KIT: OnceLock<Kit> = OnceLock::new();
    KIT.get_or_init(|| {
        let path: DerivationPath = "m/48'/1'/0'/2'".parse().expect("a path");
        let mut accounts = Vec::new();
        let mut slots = Vec::new();
        for n in 0..3u8 {
            let master = Xpriv::new_master(NET, &[n + 1; 32]).expect("a master key");
            let account = master.derive_priv(secp(), &path).expect("an account");
            slots.push(KeySlot {
                index: n + 1,
                origin_fingerprint: master.fingerprint(secp()),
                origin_path: path.clone(),
                xpub: Xpub::from_priv(secp(), &account),
            });
            accounts.push(account);
        }
        let wallet =
            WalletDescription::new(NET, slots.try_into().expect("three")).expect("a wallet");
        let xpubs = [0, 1, 2].map(|n| Xpub::from_priv(secp(), &accounts[n]));
        Kit {
            accounts: accounts.try_into().expect("three"),
            xpubs,
            wallet,
        }
    })
}

/// A script that pays an outside wallet.
pub fn external(n: u8) -> ScriptBuf {
    ScriptBuf::new_p2wpkh(&WPubkeyHash::from_byte_array([n; 20]))
}

/// That script as the address a customer's wallet shows.
pub fn external_address(n: u8) -> String {
    Address::from_script(&external(n), NET)
        .expect("an address")
        .to_string()
}

/// The wallet's receive script `i`.
pub fn receive(i: u32) -> ScriptBuf {
    kit().wallet.receive_script(i).expect("a script")
}

/// What the app hands `start`, for a data folder.
pub fn start_config(dir: &str) -> StartConfig {
    let k = kit();
    StartConfig {
        dir: dir.to_string(),
        network: OwnerNetwork::Regtest,
        now: T0,
        owner_secret: OWNER_SECRET.to_vec(),
        approver_root: id(0x22).device_id().to_bytes().to_vec(),
        wallet: k.wallet.canonical_string().to_string(),
        key1_account: k.accounts[0].encode().to_vec(),
        key3_xpub: Some(k.xpubs[2].to_string()),
        batch_min_count: 3,
        refund_depth: 6,
        birthday: 0,
        proxy: None,
        fixed_peers: Vec::new(),
        broadcast_proxy: None,
        without_tor: true,
        link: None,
        own_wallet: None,
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

/// The Key app, with Key 2 attached.
pub fn key_app() -> Approver {
    let k = kit();
    let mut a = Approver::new(
        &APPROVER_SECRET,
        k.wallet.clone(),
        id(0x11).device_id(),
        policy(),
    )
    .expect("an approver");
    let key2 = SignerState::register(
        k.wallet.canonical_string(),
        NET,
        &k.xpubs[1],
        Some(&k.xpubs[2]),
    )
    .expect("Key 2");
    a.attach_signer(key2, k.accounts[1]).expect("attached");
    a
}

/// The Key app approves every line of a request it can and declines the
/// rest, and returns its response.
pub fn answer(approver: &mut Approver, request: &[u8], now: u64) -> Vec<u8> {
    let mut session = approver.open(request, now).expect("a session");
    let items: Vec<u32> = session.lines().iter().map(|l| l.item).collect();
    for item in items {
        let line = session
            .lines()
            .into_iter()
            .find(|l| l.item == item)
            .expect("a line");
        let choice = if line.can_approve {
            Choice::Approve
        } else {
            Choice::Decline("not here".to_string())
        };
        session.decide(item, choice).expect("decided");
    }
    session
        .finish(|_| Ok::<(), String>(()))
        .expect("a response")
}

// Transactions and blocks.

/// A transaction spending a coin nobody here knows, `prev`, and paying each
/// (script, sats).
pub fn tx_from(prev: u8, outs: &[(&ScriptBuf, u64)]) -> Transaction {
    Transaction {
        version: Version::TWO,
        lock_time: LockTime::ZERO,
        input: vec![TxIn {
            previous_output: OutPoint::new(Txid::from_byte_array([prev; 32]), 0),
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
            script_pubkey: ScriptBuf::new_p2wsh(&WScriptHash::from_byte_array([0xee; 32])),
        }],
    }
}

/// The hash of the regtest genesis block.
pub fn genesis() -> BlockHash {
    genesis_block(NET).block_hash()
}

/// A block at `height` on top of `prev` with these transactions, mined to
/// the regtest target.
pub fn mine_block(prev: BlockHash, height: u32, txs: &[Transaction]) -> Block {
    let time = genesis_block(NET).header.time + height * 10;
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

// Peers.

/// A peer in its own /16 for each `n` from 20 up.
pub fn peer(n: u8) -> openbmt_watch::PeerAddr {
    openbmt_watch::PeerAddr::Ip(format!("{n}.{n}.1.1").parse().expect("an address"))
}

/// Three peers in three network groups that serve filters, at the tip of
/// the first moment.
pub fn peers_event() -> AppEvent {
    let peers = (20..23)
        .map(|n| openbmt_watch::Peer {
            addr: peer(n),
            serves_filters: true,
            best_height: 0,
            transport: openbmt_watch::PeerTransport::V1,
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
            "openbmt-owner-ffi-{}-{}",
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

// Drivers that tests feed by hand.

/// What the fake drivers were asked, one line each, tagged `main`, `send` or
/// `deep` by the driver that was asked.
pub type Log = Arc<Mutex<Vec<String>>>;

/// A driver port whose events the test sends.
pub struct FakePort {
    rx: EventReceiver,
    log: Log,
    tag: &'static str,
}

impl FakePort {
    fn say(&self, line: String) {
        self.log
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(format!("{} {line}", self.tag));
    }
}

impl DriverPort for FakePort {
    fn events(&self) -> &EventReceiver {
        &self.rx
    }

    fn watch_script(&self, _: ScriptBuf) {
        self.say("watch".to_string());
    }

    fn set_all_blocks(&self, _: bool) {}

    fn shutdown(&mut self) {
        self.say("shutdown".to_string());
    }
}

impl OwnerPort for FakePort {
    fn unwatch_scripts(&self, scripts: Vec<ScriptBuf>) {
        self.say(format!("unwatch {}", scripts.len()));
    }

    fn broadcast(&self, tx: Vec<u8>) {
        let txid = deserialize::<Transaction>(&tx)
            .map(|t| t.compute_txid().to_string())
            .unwrap_or_default();
        self.say(format!("broadcast {txid}"));
    }

    fn forget(&self, txid: Txid) {
        self.say(format!("forget {txid}"));
    }

    fn rescan_from(&self, height: u32) {
        self.say(format!("rescan {height}"));
    }

    fn set_fetch_transactions(&self, on: bool) {
        self.say(format!("fetch {on}"));
    }

    fn fetch_block(&self, hash: BlockHash) {
        self.say(format!("fetch-block {hash}"));
    }
}

/// What a driver was started with.
#[derive(Debug, Clone)]
pub struct Spawned {
    pub proxy: Option<SocketAddr>,
    pub dir: PathBuf,
}

/// A registry whose next start runs on fake drivers, a scratch folder, and
/// the ends the test holds.
pub struct Rig {
    registry: Registry,
    scratch: Scratch,
    log: Log,
    senders: Arc<Mutex<Vec<Sender<AppEvent>>>>,
    spawned: Arc<Mutex<Vec<Spawned>>>,
    chain: Mutex<Vec<Block>>,
    scans_seen: Mutex<usize>,
}

impl Rig {
    pub fn new() -> Self {
        let rig = Self {
            registry: Registry::new(),
            scratch: Scratch::new(),
            log: Arc::default(),
            senders: Arc::default(),
            spawned: Arc::default(),
            chain: Mutex::new(Vec::new()),
            scans_seen: Mutex::new(0),
        };
        rig.arm();
        rig
    }

    /// The next start runs on new fake drivers.
    pub fn arm(&self) {
        let (log, senders, spawned) = (
            Arc::clone(&self.log),
            Arc::clone(&self.senders),
            Arc::clone(&self.spawned),
        );
        // A node that is started again starts from the log of its drivers.
        senders.lock().unwrap_or_else(|e| e.into_inner()).clear();
        spawned.lock().unwrap_or_else(|e| e.into_inner()).clear();
        *self.registry.spawner.lock().expect("lock") = Some(Box::new(
            move |driver, dir: PathBuf, _scripts: Vec<ScriptBuf>| {
                let (tx, rx) = channel();
                let first = senders.lock().unwrap_or_else(|e| e.into_inner()).is_empty();
                let tag = if dir.ends_with("deepcheck") {
                    "deep"
                } else if first {
                    "main"
                } else {
                    "send"
                };
                spawned
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .push(Spawned {
                        proxy: driver.proxy,
                        dir,
                    });
                senders.lock().unwrap_or_else(|e| e.into_inner()).push(tx);
                Ok(Port(Box::new(FakePort {
                    rx: rx.into(),
                    log: Arc::clone(&log),
                    tag,
                })))
            },
        ));
    }

    pub fn registry(&self) -> &Registry {
        &self.registry
    }

    pub fn dir(&self) -> PathBuf {
        self.scratch.path().to_path_buf()
    }

    /// What the drivers were asked so far.
    pub fn calls(&self) -> Vec<String> {
        self.log.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// The drivers started so far.
    pub fn spawned(&self) -> Vec<Spawned> {
        self.spawned
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// Send events to the main driver's port, without taking them.
    pub fn send(&self, events: Vec<AppEvent>) {
        let guard = self.senders.lock().unwrap_or_else(|e| e.into_inner());
        let tx = guard.first().expect("a started node");
        for event in events {
            tx.send(event).expect("sent");
        }
    }

    /// One call.
    pub fn call(&self, op: u16, handle: u64, fields: &[u8]) -> Vec<u8> {
        dispatch(&self.registry, &frame(op, handle, fields))
    }

    /// Start the node and return its handle.
    pub fn start(&self) -> u64 {
        let response = dispatch(&self.registry, &start_request(&self.dir()));
        Rd::ok(&response).u64()
    }

    /// Start the node for a shop with its own wallet (OA-105), with the
    /// wallet's file, and return the response.
    pub fn start_own(&self, wallet: &[u8]) -> Vec<u8> {
        let mut config = start_config(self.dir().to_str().expect("UTF-8"));
        // A single-key wallet is for the main network only (WL-11).
        config.network = OwnerNetwork::Mainnet;
        config.own_wallet = Some(wallet.to_vec());
        config.approver_root = Vec::new();
        config.wallet = String::new();
        config.key1_account = Vec::new();
        config.key3_xpub = None;
        dispatch(&self.registry, &frame(1, 0, &W::new().start(&config).0))
    }

    /// Take the events that are waiting, without waiting for more.
    pub fn pump(&self, handle: u64) -> (Vec<String>, bool) {
        let response = self.call(10, handle, &W::new().u64(T0).u32(0).0);
        let mut r = Rd::ok(&response);
        let out = r.pump();
        r.done();
        out
    }

    /// Send events, take them, and answer what the node asks of the driver
    /// as the driver would (NR-90): check the blocks above the height again.
    pub fn feed(&self, handle: u64, events: Vec<AppEvent>) {
        self.send(events);
        self.pump(handle);
        self.settle(handle);
    }

    fn settle(&self, handle: u64) {
        loop {
            let log = self.calls();
            let mut seen = self.scans_seen.lock().expect("lock");
            let asked: Vec<u32> = log
                .get(*seen..)
                .unwrap_or_default()
                .iter()
                .filter_map(|l| l.strip_prefix("main rescan ")?.parse().ok())
                .collect();
            *seen = log.len();
            drop(seen);
            let Some(from) = asked.last() else { break };
            self.send(vec![AppEvent::RescanStarted(*from)]);
            self.pump(handle);
            let blocks: Vec<(u32, Block)> = self
                .chain
                .lock()
                .expect("lock")
                .iter()
                .enumerate()
                .map(|(i, b)| (u32::try_from(i).expect("fits") + 1, b.clone()))
                .filter(|(h, _)| h > from)
                .collect();
            for (height, block) in blocks {
                self.send(vec![
                    AppEvent::Block {
                        hash: block.block_hash(),
                        bytes: serialize(&block),
                    },
                    AppEvent::Checked(height),
                ]);
                self.pump(handle);
            }
        }
    }

    /// The driver says a block joined the chain, hands it over and says it
    /// has checked it.
    pub fn mine(&self, handle: u64, txs: &[Transaction]) -> Block {
        let block = {
            let mut chain = self.chain.lock().expect("lock");
            let height = u32::try_from(chain.len()).expect("fits") + 1;
            let prev = chain.last().map_or_else(genesis, Block::block_hash);
            let block = mine_block(prev, height, txs);
            chain.push(block.clone());
            block
        };
        let height = u32::try_from(self.chain.lock().expect("lock").len()).expect("fits");
        self.feed(
            handle,
            vec![
                AppEvent::Chain {
                    joined: vec![(height, block.block_hash())],
                    left: vec![],
                },
                AppEvent::Block {
                    hash: block.block_hash(),
                    bytes: serialize(&block),
                },
                AppEvent::Checked(height),
            ],
        );
        block
    }

    pub fn mine_empty(&self, handle: u64, n: u32) {
        for _ in 0..n {
            self.mine(handle, &[]);
        }
    }

    /// Fund receive index 0 with a payment, and bury it.
    pub fn fund(&self, handle: u64, sats: u64) -> Transaction {
        let pay = tx_from(7, &[(&receive(0), sats)]);
        self.mine(handle, std::slice::from_ref(&pay));
        self.mine_empty(handle, 5);
        pay
    }

    /// Sign and queue a batch of three addresses for the test counter.
    pub fn queue_batch(&self, handle: u64) -> u32 {
        let response = self.call(
            45,
            handle,
            &W::new().u64(T0).bytes(&counter_bytes()).u32(0).u32(3).0,
        );
        let mut r = Rd::ok(&response);
        let id = r.u32();
        r.done();
        id
    }
}
