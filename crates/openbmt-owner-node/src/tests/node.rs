//! The owner node on drivers that the test feeds by hand: the coin view
//! from checked blocks, sending and its states, restarts, replacement,
//! reorganizations and the Tor rule (OC-01 to OC-09, NR-86 to NR-91).

use std::fs;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::AtomicU64;
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use bitcoin::bip32::{DerivationPath, Xpriv, Xpub};
use bitcoin::consensus::{deserialize, serialize};
use bitcoin::hashes::Hash;
use bitcoin::secp256k1::{All, Secp256k1};
use bitcoin::{Block, BlockHash, Network, OutPoint, ScriptBuf, Transaction, Txid, WPubkeyHash};
use openbmt_approver::{Approver, Choice};
use openbmt_core::batch::{AddressBatch, BatchBuilder, BatchPolicy};
use openbmt_core::identity::DeviceId;
use openbmt_core::wallet::{KeySlot, WalletDescription};
use openbmt_counter_node::DriverPort;
use openbmt_net_io::{AppEvent, DriverConfig, EventReceiver, Randomness, Storage};
use openbmt_owner::own_book::AllocationBook;
use openbmt_owner::{Delivery, SaleFacts, SaleKey, Stage};
use openbmt_signer::SignerState;
use openbmt_spend::Coin;

mod former;
mod live;
mod own;
mod own_spend;
mod paid_log;
mod receipts;
mod requests;
mod stopped;

use super::coins::{mine, tx_from};
use super::{id, Dir, TestClock};
use crate::coins::{CoinView, ViewState};
use crate::live::WatchLimits;
use crate::own::own_approver;
use crate::{
    NodeError, OwnerConfig, OwnerNode, OwnerParts, OwnerPort, SendRefused, SendState, COINS_FILE,
    NO_TOR,
};

const NET: Network = Network::Regtest;
const NOW: u64 = 10_000_000;
const OWNER_SECRET: [u8; 32] = [0x11; 32];
const APPROVER_SECRET: [u8; 32] = [0x22; 32];

fn secp() -> &'static Secp256k1<All> {
    static CONTEXT: OnceLock<Secp256k1<All>> = OnceLock::new();
    CONTEXT.get_or_init(Secp256k1::new)
}

struct Kit {
    accounts: [Xpriv; 3],
    xpubs: [Xpub; 3],
    wallet: WalletDescription,
}

/// Three keys from fixed seeds, and the wallet they make. Test only.
fn kit() -> &'static Kit {
    static KIT: OnceLock<Kit> = OnceLock::new();
    KIT.get_or_init(|| {
        let path: DerivationPath = "m/48'/1'/0'/2'".parse().unwrap();
        let mut accounts = Vec::new();
        let mut slots = Vec::new();
        for n in 0..3u8 {
            let master = Xpriv::new_master(NET, &[n + 1; 32]).unwrap();
            let account = master.derive_priv(secp(), &path).unwrap();
            slots.push(KeySlot {
                index: n + 1,
                origin_fingerprint: master.fingerprint(secp()),
                origin_path: path.clone(),
                xpub: Xpub::from_priv(secp(), &account),
            });
            accounts.push(account);
        }
        let wallet = WalletDescription::new(NET, slots.try_into().unwrap()).unwrap();
        let xpubs = [0, 1, 2].map(|n| Xpub::from_priv(secp(), &accounts[n]));
        Kit {
            accounts: accounts.try_into().unwrap(),
            xpubs,
            wallet,
        }
    })
}

fn policy() -> BatchPolicy {
    BatchPolicy { min_count: 3 }
}

fn approver_id() -> DeviceId {
    id(0x22).device_id()
}

fn key1() -> SignerState {
    let k = kit();
    SignerState::register(
        k.wallet.canonical_string(),
        NET,
        &k.xpubs[0],
        Some(&k.xpubs[2]),
    )
    .unwrap()
}

fn key_app() -> Approver {
    let k = kit();
    let mut a = Approver::new(
        &APPROVER_SECRET,
        k.wallet.clone(),
        id(0x11).device_id(),
        policy(),
    )
    .unwrap();
    let key2 = SignerState::register(
        k.wallet.canonical_string(),
        NET,
        &k.xpubs[1],
        Some(&k.xpubs[2]),
    )
    .unwrap();
    a.attach_signer(key2, k.accounts[1]).unwrap();
    a
}

fn external(n: u8) -> ScriptBuf {
    ScriptBuf::new_p2wpkh(&WPubkeyHash::from_byte_array([n; 20]))
}

fn receive(i: u32) -> ScriptBuf {
    kit().wallet.receive_script(i).unwrap()
}

fn batch(start: u32, count: u32) -> AddressBatch {
    BatchBuilder::new(policy())
        .derive(&kit().wallet, id(0x31).device_id(), start, count)
        .unwrap()
}

fn sale(n: u32) -> SaleKey {
    SaleKey {
        counter: id(0x31).device_id(),
        index: n,
    }
}

// ---- the fake drivers --------------------------------------------------------------

type Log = Arc<Mutex<Vec<String>>>;

struct FakePort {
    rx: EventReceiver,
    log: Log,
    tag: &'static str,
}

impl FakePort {
    fn say(&self, line: String) {
        self.log
            .lock()
            .unwrap()
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
    fn shutdown(&mut self) {}
}

impl OwnerPort for FakePort {
    fn unwatch_scripts(&self, scripts: Vec<ScriptBuf>) {
        self.say(format!("unwatch {}", scripts.len()));
    }
    fn broadcast(&self, tx: Vec<u8>) {
        let txid = deserialize::<Transaction>(&tx).unwrap().compute_txid();
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
struct Spawned {
    proxy: Option<SocketAddr>,
    dir: PathBuf,
    watch_from: u32,
    scripts: usize,
}

struct Counter(u64);

impl Randomness for Counter {
    fn next_u64(&mut self) -> u64 {
        self.0 += 1;
        self.0
    }
}

struct Setup {
    proxy: Option<SocketAddr>,
    without_tor: bool,
    refund_depth: u32,
    /// OC-22 to OC-24: the live watch's numbers.
    limits: WatchLimits,
    /// The node's clock, when a test sets it.
    time: Option<Arc<AtomicU64>>,
    /// A shop with its own wallet: the allocation book it starts with.
    own: Option<AllocationBook>,
    /// With `own`: the shop's wallet, when a test needs keys it knows.
    own_wallet: Option<openbmt_core::wallet_single::ImportedWallet>,
    /// SS-43: the wallets pinned before a re-pin.
    former: Vec<crate::FormerParts>,
    /// The customer pays the fee of a refund.
    customer_fee: bool,
}

impl Default for Setup {
    fn default() -> Self {
        Self {
            proxy: None,
            without_tor: true,
            refund_depth: 6,
            limits: WatchLimits::default(),
            time: None,
            own: None,
            own_wallet: None,
            former: Vec::new(),
            customer_fee: false,
        }
    }
}

/// A node, and the senders of its drivers' events.
struct Rig {
    node: OwnerNode<FakePort>,
    main: Sender<AppEvent>,
    sender: Option<Sender<AppEvent>>,
    log: Log,
    spawned: Arc<Mutex<Vec<Spawned>>>,
    /// The senders of every driver that was started, in order.
    senders: Arc<Mutex<Vec<Sender<AppEvent>>>>,
    chain: Vec<Block>,
    /// How much of the log has been looked at for scan requests.
    scans_seen: usize,
}

fn start(dir: &Dir, setup: &Setup) -> Result<Rig, NodeError> {
    start_at(dir, setup, Vec::new())
}

/// What a driver that kept the chain in the node's directory leaves there:
/// the headers of `chain`.
fn save_headers(dir: &Dir, chain: &[Block]) {
    let mut store = openbmt_watch::HeaderStore::new(NET).unwrap();
    for block in chain {
        store.accept(&block.header, 2_000_000_000).unwrap();
    }
    openbmt_net_io::FileStorage::new(dir.path())
        .unwrap()
        .save(openbmt_net_io::Key::Headers, &store.to_bytes())
        .unwrap();
}

fn start_at(dir: &Dir, setup: &Setup, chain: Vec<Block>) -> Result<Rig, NodeError> {
    // The driver of a node that stopped keeps its headers.
    if !chain.is_empty() {
        save_headers(dir, &chain);
    }
    let log: Log = Arc::default();
    let spawned: Arc<Mutex<Vec<Spawned>>> = Arc::default();
    let senders: Arc<Mutex<Vec<Sender<AppEvent>>>> = Arc::default();
    let mut config = OwnerConfig::new(dir.path(), id(0x11), DriverConfig::new(NET, 0));
    config.broadcast_proxy = setup.proxy;
    config.broadcast_without_tor = setup.without_tor;
    if let Some(time) = &setup.time {
        config.clock = Box::new(TestClock(time.clone()));
    }
    config.health = Box::new(|_| openbmt_watch::Health {
        healthy: true,
        problem: None,
        behind: false,
    });
    let mut parts = match &setup.own {
        Some(book) => {
            let approver = own_approver(&OWNER_SECRET).unwrap();
            let mut parts = OwnerParts::new(
                OWNER_SECRET,
                setup.own_wallet.clone().unwrap_or_else(own::single),
                approver,
                policy(),
            )?;
            parts.own_book = Some(book.clone());
            parts
        }
        None => OwnerParts::new(OWNER_SECRET, kit().wallet.clone(), approver_id(), policy())?,
    };
    if setup.customer_fee {
        parts.options.fee_payer = openbmt_owner::FeePayer::Customer;
    }
    parts.refund_depth = setup.refund_depth;
    parts.former = setup.former.clone();
    parts.limits = setup.limits;
    parts.random = Box::new(Counter(1));
    let (l, s, sn) = (log.clone(), spawned.clone(), senders.clone());
    let node = OwnerNode::start_with(config, parts, move |driver, dir, scripts| {
        let (tx, rx) = channel();
        let tag = if dir.ends_with("deepcheck") {
            "deep"
        } else if s.lock().unwrap().is_empty() {
            "main"
        } else {
            "send"
        };
        s.lock().unwrap().push(Spawned {
            proxy: driver.proxy,
            dir,
            watch_from: driver.watch_from,
            scripts: scripts.len(),
        });
        sn.lock().unwrap().push(tx);
        Ok(FakePort {
            rx: rx.into(),
            log: l.clone(),
            tag,
        })
    })?;
    let mut all = senders.lock().unwrap().clone().into_iter();
    let main = all.next().unwrap();
    let sender = all.next();
    let mut rig = Rig {
        node,
        main,
        sender,
        log,
        spawned,
        senders,
        chain,
        scans_seen: 0,
    };
    // A restarted driver tells the node the chain it holds again.
    if !rig.chain.is_empty() {
        let joined = rig
            .chain
            .iter()
            .enumerate()
            .map(|(i, b)| (u32::try_from(i).unwrap() + 1, b.block_hash()))
            .collect();
        rig.feed(vec![AppEvent::Chain {
            joined,
            left: vec![],
        }]);
        // And it checks the blocks above where it was told to start.
        let from = rig.spawned.lock().unwrap()[0].watch_from;
        rig.replay_from(from);
    }
    Ok(rig)
}

impl Rig {
    fn calls(&self) -> Vec<String> {
        self.log.lock().unwrap().clone()
    }

    fn feed(&mut self, events: Vec<AppEvent>) {
        self.deliver(events);
        self.rescan();
    }

    fn deliver(&mut self, events: Vec<AppEvent>) {
        for e in events {
            self.main.send(e).unwrap();
        }
        self.node.pump(Duration::ZERO);
    }

    /// The driver checks the blocks above a height again when the node asks
    /// it to (NR-90): it hands each over and says it has checked it.
    fn rescan(&mut self) {
        loop {
            let log = self.calls();
            let asked: Vec<u32> = log[self.scans_seen..]
                .iter()
                .filter_map(|l| l.strip_prefix("main rescan ")?.parse().ok())
                .collect();
            self.scans_seen = log.len();
            let Some(from) = asked.last() else { break };
            self.deliver(vec![AppEvent::RescanStarted(*from)]);
            self.replay_from(*from);
        }
    }

    fn replay_from(&mut self, from: u32) {
        let blocks: Vec<(u32, Block)> = self
            .chain
            .iter()
            .enumerate()
            .map(|(i, b)| (u32::try_from(i).unwrap() + 1, b.clone()))
            .filter(|(h, _)| *h > from)
            .collect();
        for (height, block) in blocks {
            self.deliver(vec![
                AppEvent::Block {
                    hash: block.block_hash(),
                    bytes: serialize(&block),
                },
                AppEvent::Checked(height),
            ]);
        }
        self.rescan();
    }

    fn tip_hash(&self) -> BlockHash {
        self.chain.last().map_or_else(
            || bitcoin::blockdata::constants::genesis_block(NET).block_hash(),
            Block::block_hash,
        )
    }

    /// The driver says a block joined the chain, hands it over and says it
    /// has checked it.
    fn mine(&mut self, txs: &[Transaction]) -> Block {
        let height = u32::try_from(self.chain.len()).unwrap() + 1;
        let block = mine(self.tip_hash(), height, 0, txs);
        self.chain.push(block.clone());
        self.feed(vec![
            AppEvent::Chain {
                joined: vec![(height, block.block_hash())],
                left: vec![],
            },
            AppEvent::Block {
                hash: block.block_hash(),
                bytes: serialize(&block),
            },
            AppEvent::Checked(height),
        ]);
        block
    }

    fn mine_empty(&mut self, n: u32) {
        for _ in 0..n {
            self.mine(&[]);
        }
    }

    /// Queue a batch, a sale and a refund of `sats` to `external(9)`.
    fn queue_refund(&mut self, sats: u64) -> u32 {
        let write = self.node.queue_writer();
        self.node
            .owner_mut()
            .unwrap()
            .queue_batch(batch(0, 3), 1_000, &write)
            .unwrap();
        self.node.refresh_marks();
        self.node
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
        self.node
            .owner_mut()
            .unwrap()
            .queue_refund(sale(1), "Mia", external(9), sats, None, &write)
            .unwrap()
    }

    /// Fund receive index 0 with a payment, and bury it. The sale that
    /// `queue_refund` queues against, at index 1, is paid 50,000 in the same
    /// transaction, so the view's own facts (OC-47) hold what the rig says.
    fn fund(&mut self, sats: u64) -> Transaction {
        let pay = tx_from(
            OutPoint::new(Txid::from_byte_array([7; 32]), 0),
            &[(&receive(0), sats), (&receive(1), 50_000)],
        );
        self.mine(std::slice::from_ref(&pay));
        self.mine_empty(5);
        pay
    }

    /// The Key app approves every line of a request.
    fn answer(&self, approver: &mut Approver, request: &[u8]) -> Vec<u8> {
        let mut session = approver.open(request, NOW).unwrap();
        let items: Vec<u32> = session.lines().iter().map(|l| l.item).collect();
        for item in items {
            // The batch names a counter that is not on any device list here.
            let line = session
                .lines()
                .into_iter()
                .find(|l| l.item == item)
                .unwrap();
            let choice = if line.can_approve {
                Choice::Approve
            } else {
                Choice::Decline("not here".to_string())
            };
            session.decide(item, choice).unwrap();
        }
        session.finish(|_| Ok::<(), String>(())).unwrap()
    }

    /// Build, approve and take.
    fn spend(&mut self) -> Txid {
        let built = self.node.build_request(1_000, NOW).unwrap();
        self.rescan();
        let mut approver = key_app();
        let response = self.answer(&mut approver, &built.request);
        let mut signer = key1();
        self.node
            .take(&response, &mut signer, &kit().accounts[0], NOW, |_| Ok(()))
            .unwrap();
        self.rescan();
        self.txids().pop().expect("a transaction came out")
    }

    fn txids(&self) -> Vec<Txid> {
        self.node
            .owner()
            .outbox()
            .iter()
            .filter_map(|d| match &d.what {
                Delivery::Transaction { txid, .. } => Some(*txid),
                _ => None,
            })
            .collect()
    }

    fn tx(&self, txid: &Txid) -> Transaction {
        self.node
            .owner()
            .outbox()
            .iter()
            .find_map(|d| match &d.what {
                Delivery::Transaction { txid: t, tx, .. } if t == txid => {
                    Some(deserialize(tx).unwrap())
                }
                _ => None,
            })
            .unwrap()
    }
}

fn offered(node: &OwnerNode<FakePort>) -> Vec<(OutPoint, u32)> {
    let mut v: Vec<_> = node
        .coins()
        .iter()
        .map(|c: &Coin| (c.outpoint, c.confirmations))
        .collect();
    v.sort();
    v
}

// ---- the coin view through the node --------------------------------------------------

#[test]
fn oc_01_a_payment_in_a_checked_block_is_a_coin() {
    let dir = Dir::new();
    let mut rig = start(&dir, &Setup::default()).unwrap();
    rig.queue_refund(10_000);
    let pay = rig.fund(300_000);
    let op = OutPoint::new(pay.compute_txid(), 0);
    let sale_coin = OutPoint::new(pay.compute_txid(), 1);
    assert_eq!(offered(&rig.node), vec![(op, 6), (sale_coin, 6)]);
    assert_eq!(rig.node.balance(), 350_000);
    assert_eq!(rig.node.state(), ViewState::Ready);
}

#[test]
fn oc_02_an_unconfirmed_payment_a_peer_shows_is_not_a_coin() {
    let dir = Dir::new();
    let mut rig = start(&dir, &Setup::default()).unwrap();
    rig.queue_refund(10_000);
    let pay = tx_from(
        OutPoint::new(Txid::from_byte_array([8; 32]), 0),
        &[(&receive(0), 99_000)],
    );
    rig.feed(vec![
        AppEvent::Sighting {
            tx: serialize(&pay),
            from: vec![],
        },
        AppEvent::AlsoAnnounced {
            txid: pay.compute_txid(),
            from: openbmt_watch::PeerAddr::Ip("1.2.3.4".parse().unwrap()),
        },
    ]);
    assert!(rig.node.coins().is_empty());
    assert_eq!(rig.node.balance(), 0);
    // The node asked the drivers not to fetch what peers announce.
    assert!(rig.calls().iter().any(|c| c == "main fetch false"));
}

#[test]
fn a_failed_key_store_write_keeps_a_joint_commit_for_same_process_recovery() {
    let dir = Dir::new();
    let mut rig = start(&dir, &Setup::default()).unwrap();
    rig.queue_refund(10_000);
    rig.fund(300_000);
    let built = rig.node.build_request(1_000, NOW).unwrap();
    let mut approver = key_app();
    let response = rig.answer(&mut approver, &built.request);
    let stale_writer = rig.node.queue_writer();
    let stale_queue = fs::read(dir.path().join(crate::QUEUE_FILE)).unwrap();
    let mut signer = key1();
    let old_snapshot = signer.to_bytes();
    let error = rig
        .node
        .take(&response, &mut signer, &kit().accounts[0], NOW, |_| {
            Err("key store unavailable".to_string())
        })
        .unwrap_err();
    assert_eq!(error, NodeError::KeyRecoveryRequired);
    assert_eq!(signer.to_bytes(), old_snapshot);
    assert!(rig.node.take_recovery_pending());
    assert_eq!(
        rig.node.build_request(1_000, NOW).unwrap_err(),
        NodeError::KeyRecoveryRequired
    );
    assert!(rig.node.owner().outbox().is_empty());

    let queue_before_wrong_key = fs::read(dir.path().join(crate::QUEUE_FILE)).unwrap();
    let mut wrong_key_store_called = false;
    assert!(rig
        .node
        .recover_key1(&mut signer, &kit().accounts[1], |_| {
            wrong_key_store_called = true;
            Ok(())
        })
        .is_err());
    assert!(!wrong_key_store_called);
    assert_eq!(signer.to_bytes(), old_snapshot);
    assert_eq!(
        fs::read(dir.path().join(crate::QUEUE_FILE)).unwrap(),
        queue_before_wrong_key
    );
    assert!(dir.path().join(crate::TAKE_JOURNAL_FILE).exists());
    assert_eq!(
        rig.node.build_request(1_000, NOW).unwrap_err(),
        NodeError::KeyRecoveryRequired
    );

    let mut persisted = Vec::new();
    rig.node
        .recover_key1(&mut signer, &kit().accounts[0], |bytes| {
            persisted = bytes.to_vec();
            Ok(())
        })
        .unwrap();
    assert!(!rig.node.take_recovery_pending());
    assert_eq!(signer.to_bytes(), persisted);
    assert_ne!(signer.to_bytes(), old_snapshot);
    assert!(signer.latest_time_seen() >= NOW);
    assert!(signer.change_mark() > 0);
    let entries: Vec<_> = signer.log().collect();
    assert_eq!(entries.len(), 1);
    assert!(entries[0].time() >= NOW);
    assert!(!entries[0].inputs().is_empty());
    assert_eq!(rig.node.owner().outbox().len(), 1);
    let committed_queue = fs::read(dir.path().join(crate::QUEUE_FILE)).unwrap();
    assert!(stale_writer(&stale_queue).is_err());
    assert_eq!(
        fs::read(dir.path().join(crate::QUEUE_FILE)).unwrap(),
        committed_queue
    );
}

#[test]
fn a_pending_take_journal_rolls_forward_after_restart_before_queue_actions() {
    let dir = Dir::new();
    let (chain, old_snapshot) = {
        let mut rig = start(&dir, &Setup::default()).unwrap();
        rig.queue_refund(10_000);
        rig.fund(300_000);
        let built = rig.node.build_request(1_000, NOW).unwrap();
        let mut approver = key_app();
        let response = rig.answer(&mut approver, &built.request);
        let mut signer = key1();
        let old_snapshot = signer.to_bytes();
        rig.node
            .take(&response, &mut signer, &kit().accounts[0], NOW, |_| {
                Err("key store unavailable".to_string())
            })
            .unwrap_err();
        (rig.chain.clone(), old_snapshot)
    };
    let mut rig = start_at(&dir, &Setup::default(), chain).unwrap();
    assert!(rig.node.take_recovery_pending());
    assert_eq!(
        rig.node.build_request(1_000, NOW).unwrap_err(),
        NodeError::KeyRecoveryRequired
    );
    let mut signer = SignerState::from_bytes(&old_snapshot, NET, &kit().xpubs[0]).unwrap();
    let mut persisted = Vec::new();
    rig.node
        .recover_key1(&mut signer, &kit().accounts[0], |bytes| {
            persisted = bytes.to_vec();
            Ok(())
        })
        .unwrap();
    let restored = SignerState::from_bytes(&persisted, NET, &kit().xpubs[0]).unwrap();
    assert_eq!(signer.to_bytes(), restored.to_bytes());
    assert_ne!(signer.to_bytes(), old_snapshot);
    assert!(signer.latest_time_seen() >= NOW);
    assert!(signer.change_mark() > 0);
    let entries: Vec<_> = signer.log().collect();
    assert_eq!(entries.len(), 1);
    assert!(entries[0].time() >= NOW);
    assert!(!entries[0].inputs().is_empty());
    assert_eq!(rig.node.owner().outbox().len(), 1);
}

#[test]
fn an_oversized_journal_file_is_rejected_from_metadata_before_reading_it() {
    let dir = Dir::new();
    let path = dir.path().join(crate::TAKE_JOURNAL_FILE);
    let file = fs::File::create(&path).unwrap();
    file.set_len(u64::try_from(crate::take_journal::MAX_JOURNAL_BYTES).unwrap() + 1)
        .unwrap();
    let error = crate::node::read_take_journal(dir.path()).unwrap_err();
    assert!(error.contains("too large"));
}

#[test]
fn oc_05_a_block_that_leaves_takes_its_coin_with_it_and_the_position_down() {
    let dir = Dir::new();
    let mut rig = start(&dir, &Setup::default()).unwrap();
    rig.queue_refund(10_000);
    let pay = tx_from(
        OutPoint::new(Txid::from_byte_array([7; 32]), 0),
        &[(&receive(0), 80_000)],
    );
    let first = rig.mine(&[pay]);
    assert_eq!(rig.node.balance(), 80_000);
    assert_eq!(rig.node.view().checked(), 1);
    // Another block 1 takes its place.
    let other = mine(
        bitcoin::blockdata::constants::genesis_block(NET).block_hash(),
        1,
        7,
        &[],
    );
    rig.feed(vec![
        AppEvent::Chain {
            joined: vec![(1, other.block_hash())],
            left: vec![first.block_hash()],
        },
        AppEvent::Block {
            hash: other.block_hash(),
            bytes: serialize(&other),
        },
        AppEvent::Checked(1),
    ]);
    assert_eq!(rig.node.balance(), 0);
    assert!(rig.node.coins().is_empty());
    assert_eq!(rig.node.view().checked(), 1);
}

#[test]
fn oc_47_a_refund_waits_while_its_receipt_is_off_the_chain_and_goes_when_it_is_back() {
    let dir = Dir::new();
    let mut rig = start(&dir, &Setup::default()).unwrap();
    let refund = rig.queue_refund(10_000);
    rig.fund(300_000);
    let facts = |rig: &Rig| rig.node.owner().sale_facts(&sale(1)).unwrap();
    let stage = |rig: &Rig| {
        rig.node
            .owner()
            .refunds()
            .iter()
            .find(|r| r.id == refund)
            .unwrap()
            .stage
            .clone()
    };
    // The receipt is on the chain, deep enough.
    assert_eq!(
        facts(&rig),
        SaleFacts {
            received_sats: 50_000,
            reached_refund_depth: true
        }
    );
    assert_eq!(rig.node.owner().waiting_for_depth(refund), Some(false));

    // A longer branch from the genesis block drops every receipt.
    let left: Vec<BlockHash> = rig.chain.iter().rev().map(Block::block_hash).collect();
    let mut branch = Vec::new();
    let mut prev = bitcoin::blockdata::constants::genesis_block(NET).block_hash();
    for height in 1..=7 {
        let block = mine(prev, height, 100 + height, &[]);
        prev = block.block_hash();
        branch.push(block);
    }
    let joined = branch
        .iter()
        .enumerate()
        .map(|(i, b)| (u32::try_from(i).unwrap() + 1, b.block_hash()))
        .collect();
    rig.chain = branch.clone();
    rig.feed(vec![AppEvent::Chain { joined, left }]);
    // The facts fell in the step that changed the coin view, before any
    // block of the new branch was handed over.
    assert!(!facts(&rig).reached_refund_depth);
    assert_eq!(facts(&rig).received_sats, 0);
    assert_eq!(rig.node.owner().waiting_for_depth(refund), Some(true));
    for (i, block) in branch.iter().enumerate() {
        rig.deliver(vec![
            AppEvent::Block {
                hash: block.block_hash(),
                bytes: serialize(block),
            },
            AppEvent::Checked(u32::try_from(i).unwrap() + 1),
        ]);
    }
    assert_eq!(rig.node.owner().waiting_for_depth(refund), Some(true));

    // A stale cache cannot make it ready: a session works the facts out
    // again first.
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
    rig.node.build_request(1_000, NOW).unwrap();
    assert_eq!(stage(&rig), Stage::Queued);
    assert!(!facts(&rig).reached_refund_depth);

    // The receipt confirms again and the refund is ready again.
    let again = tx_from(
        OutPoint::new(Txid::from_byte_array([8; 32]), 0),
        &[(&receive(0), 300_000), (&receive(1), 50_000)],
    );
    rig.mine(&[again]);
    rig.mine_empty(5);
    rig.node.build_request(1_000, NOW).unwrap();
    assert!(matches!(stage(&rig), Stage::InSession { .. }));
}

fn rescans(rig: &Rig) -> usize {
    rig.calls()
        .iter()
        .filter(|c| c.starts_with("main rescan"))
        .count()
}

#[test]
fn oc_07_new_marks_rescan_only_the_bounded_checked_range() {
    let dir = Dir::new();
    let mut rig = start(&dir, &Setup::default()).unwrap();
    rig.mine_empty(2);
    // Nothing is handed out: only the gap is watched, on both keychains.
    assert_eq!(rig.spawned.lock().unwrap()[0].scripts, 40);
    let before = rig.calls();
    rig.queue_refund(10_000);
    let after = rig.calls();
    let new = &after[before.len()..];
    // 3 more receive scripts and a bounded scan from the latest checked block.
    assert_eq!(new.iter().filter(|c| *c == "main watch").count(), 3);
    assert_eq!(new.iter().filter(|c| *c == "main rescan 1").count(), 1);
    assert_eq!(rig.node.state(), ViewState::Rescanning { to: 2 });
    assert!(matches!(
        rig.node.build_request(1_000, NOW),
        Err(NodeError::NotReady(_))
    ));
    rig.rescan();
    assert_eq!(rig.node.state(), ViewState::Ready);
    rig.node.refresh_marks();
    assert_eq!(
        rig.calls().len(),
        after.len(),
        "nothing grew, nothing is said"
    );
    // A spend raises the change mark and starts a short scan.
    rig.fund(300_000);
    let txid = rig.spend();
    assert!(rig.node.send_state(&txid).is_some());
    assert!(rescans(&rig) >= 1);
    rig.rescan();
    assert_eq!(rig.node.state(), ViewState::Ready);
}

#[test]
fn oc_07_a_fresh_wallet_waits_for_the_ordered_watch_update_without_a_future_height() {
    let dir = Dir::new();
    let mut rig = start(&dir, &Setup::default()).unwrap();
    let write = rig.node.queue_writer();
    rig.node
        .owner_mut()
        .unwrap()
        .queue_batch(batch(0, 3), 1_000, &write)
        .unwrap();
    rig.node.refresh_marks();
    assert_eq!(rig.node.state(), ViewState::Ready);
    rig.rescan();
    assert_eq!(rig.node.state(), ViewState::Ready);
}

#[test]
fn oc_07_a_payment_to_a_new_script_is_found_after_the_bounded_scan() {
    let dir = Dir::new();
    let mut rig = start(&dir, &Setup::default()).unwrap();
    rig.mine_empty(2);
    let write = rig.node.queue_writer();
    // Larger than the gap: index 29 was not watched before.
    rig.node
        .owner_mut()
        .unwrap()
        .queue_batch(batch(0, 30), 1_000, &write)
        .unwrap();
    rig.node.refresh_marks();
    assert!(matches!(rig.node.state(), ViewState::Rescanning { .. }));
    let pay = tx_from(
        OutPoint::new(Txid::from_byte_array([7; 32]), 0),
        &[(&receive(29), 70_000)],
    );
    // The block can be fetched with the old filters and arrive before the
    // rescan command. A stale Checked event must not make the view ready.
    let height = u32::try_from(rig.chain.len()).unwrap() + 1;
    let block = mine(rig.tip_hash(), height, 0, std::slice::from_ref(&pay));
    rig.chain.push(block.clone());
    rig.deliver(vec![
        AppEvent::Chain {
            joined: vec![(height, block.block_hash())],
            left: vec![],
        },
        AppEvent::Checked(height),
    ]);
    assert!(matches!(rig.node.state(), ViewState::Rescanning { .. }));
    rig.rescan();
    assert_eq!(rig.node.balance(), 70_000);
    assert_eq!(rig.node.state(), ViewState::Ready);
    assert_eq!(rescans(&rig), 1);
    assert!(!matches!(
        rig.node.build_request(1_000, NOW),
        Err(NodeError::NotReady(_))
    ));
}

#[test]
fn oc_07_a_restart_between_the_queue_and_the_view_raises_the_marks_before_the_next_block() {
    let dir = Dir::new();
    let chain = {
        let mut rig = start(&dir, &Setup::default()).unwrap();
        rig.mine_empty(2);
        // The batch is stored in the queue; the view is never told.
        let write = rig.node.queue_writer();
        rig.node
            .owner_mut()
            .unwrap()
            .queue_batch(batch(0, 30), 1_000, &write)
            .unwrap();
        rig.chain.clone()
    };
    let mut rig = start_at(&dir, &Setup::default(), chain).unwrap();
    // The driver starts with the grown set at the safe rewind position, and
    // the persisted scan resumes without an extra command.
    assert_eq!(rig.spawned.lock().unwrap()[0].scripts, 50 + 20);
    assert_eq!(rig.node.view().receive_next(), 30);
    assert_eq!(rig.spawned.lock().unwrap()[0].watch_from, 1);
    let pay = tx_from(
        OutPoint::new(Txid::from_byte_array([7; 32]), 0),
        &[(&receive(29), 70_000)],
    );
    rig.mine(&[pay]);
    assert_eq!(rig.node.balance(), 70_000);
    assert_eq!(rescans(&rig), 0);
}

#[test]
fn oc_07_runtime_growth_is_bounded_and_explicit_rescan_still_starts_at_birthday() {
    let dir = Dir::new();
    let mut rig = start(&dir, &Setup::default()).unwrap();
    rig.queue_refund(10_000);
    rig.fund(300_000);
    assert_eq!(rescans(&rig), 1);
    // A new view starts the driver at the birthday: no command needed.
    assert_eq!(rig.spawned.lock().unwrap()[0].watch_from, 0);
    rig.node.rescan().unwrap();
    assert_eq!(rescans(&rig), 2);
    assert!(rig.calls().contains(&"main rescan 0".to_string()));
}

#[test]
fn oc_09_nothing_is_built_until_the_view_has_checked_up_to_the_tip() {
    let dir = Dir::new();
    let mut rig = start(&dir, &Setup::default()).unwrap();
    rig.queue_refund(10_000);
    rig.fund(300_000);
    // The chain moves on, and no block has been checked yet.
    let next = mine(rig.tip_hash(), 7, 0, &[]);
    rig.feed(vec![AppEvent::Chain {
        joined: vec![(7, next.block_hash())],
        left: vec![],
    }]);
    match rig.node.build_request(1_000, NOW) {
        Err(NodeError::NotReady(why)) => assert!(why.contains("up to 6"), "{why}"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn oc_09_a_block_that_cannot_be_applied_stops_the_view_and_says_why() {
    let dir = Dir::new();
    let mut rig = start(&dir, &Setup::default()).unwrap();
    rig.queue_refund(10_000);
    rig.fund(300_000);
    // A block the best chain does not hold.
    let stray = mine(BlockHash::from_byte_array([5; 32]), 9, 3, &[]);
    rig.feed(vec![AppEvent::Block {
        hash: stray.block_hash(),
        bytes: serialize(&stray),
    }]);
    // It is not on the best chain: it is noted and passed over. A block that
    // is garbage stops the view.
    assert!(rig.node.halted().is_none());
    let notes = rig.node.watcher_mut().take_notes();
    assert!(
        notes.iter().any(|n| n.contains("not on the best chain")),
        "{notes:?}"
    );
    rig.feed(vec![AppEvent::Block {
        hash: stray.block_hash(),
        bytes: vec![1, 2, 3],
    }]);
    assert!(rig.node.halted().is_some());
    assert!(matches!(
        rig.node.build_request(1_000, NOW),
        Err(NodeError::NotReady(_))
    ));
    let notes = rig.node.watcher_mut().take_notes();
    assert!(notes.iter().any(|n| n.contains("was refused")), "{notes:?}");
}

#[test]
fn oc_08_a_restart_resumes_from_the_stored_position_and_coins() {
    let dir = Dir::new();
    {
        let mut rig = start(&dir, &Setup::default()).unwrap();
        rig.queue_refund(10_000);
        rig.fund(300_000);
        assert_eq!(rig.node.view().checked(), 6);
    }
    let rig = start(&dir, &Setup::default()).unwrap();
    // The driver is told where to start, and the coins are there.
    assert_eq!(rig.spawned.lock().unwrap()[0].watch_from, 6);
    assert_eq!(rig.spawned.lock().unwrap()[0].scripts, 43);
    assert_eq!(rig.node.balance(), 350_000);
}

#[test]
fn oc_08_a_file_that_does_not_read_back_stops_the_start() {
    let dir = Dir::new();
    {
        let mut rig = start(&dir, &Setup::default()).unwrap();
        rig.queue_refund(10_000);
        rig.fund(300_000);
    }
    let path = dir.path().join(crate::COINS_FILE);
    let mut bytes = std::fs::read(&path).unwrap();
    bytes[0] ^= 1;
    std::fs::write(&path, bytes).unwrap();
    match start(&dir, &Setup::default()) {
        Err(NodeError::Files(why)) => assert!(why.contains("does not read back"), "{why}"),
        Err(e) => panic!("{e}"),
        Ok(_) => panic!("a view that does not read back was used"),
    }
}

#[test]
fn a_refund_depth_the_view_cannot_follow_is_refused() {
    let dir = Dir::new();
    let setup = Setup {
        refund_depth: 101,
        ..Setup::default()
    };
    assert!(matches!(start(&dir, &setup), Err(NodeError::Config(_))));
}

// ---- sending -----------------------------------------------------------------------

#[test]
fn nr_86_a_refund_is_signed_held_sent_followed_and_done_at_the_refund_depth() {
    let dir = Dir::new();
    let mut rig = start(&dir, &Setup::default()).unwrap();
    let refund = rig.queue_refund(10_000);
    let pay = rig.fund(300_000);
    let txid = rig.spend();

    // Taken: the transaction pays the refund, is held and was sent once.
    let tx = rig.tx(&txid);
    assert!(tx
        .output
        .iter()
        .any(|o| o.script_pubkey == external(9) && o.value.to_sat() == 10_000));
    assert_eq!(
        tx.input[0].previous_output,
        OutPoint::new(pay.compute_txid(), 0)
    );
    assert_eq!(rig.node.send_state(&txid), Some(SendState::Sent));
    let sent = format!("main broadcast {txid}");
    assert_eq!(rig.calls().iter().filter(|c| **c == sent).count(), 1);
    assert!(matches!(
        rig.node.owner().refunds()[0].stage,
        Stage::Sent { .. }
    ));
    // Its change is a coin at no confirmations, and the spent coin is not
    // offered.
    let change = OutPoint::new(txid, u32::try_from(1 - tx_pays_first(&tx)).unwrap());
    let sale_coin = OutPoint::new(pay.compute_txid(), 1);
    let mut want = vec![(change, 0), (sale_coin, 6)];
    want.sort();
    assert_eq!(offered(&rig.node), want);
    assert_eq!(
        rig.node.balance(),
        50_000
            + tx.output
                .get(usize::try_from(change.vout).unwrap())
                .unwrap()
                .value
                .to_sat()
    );
    let notes = rig.node.watcher_mut().take_notes();
    assert!(
        notes.iter().any(|n| n.contains("sending without Tor")),
        "{notes:?}"
    );

    // The peers: not asked yet by two groups, then seen back.
    rig.feed(vec![AppEvent::TxRelayed {
        txid,
        peers_asked: 1,
        peers_fetched: 1,
    }]);
    assert_eq!(rig.node.relay_counts(&txid), Some((1, 1)));
    rig.feed(vec![AppEvent::TxState {
        txid,
        state: openbmt_net_io::RelayState::SeenBack { groups: 2 },
    }]);
    assert_eq!(
        rig.node.send_state(&txid),
        Some(SendState::SeenBack { groups: 2 })
    );

    // NR-112: the shop's own node gives it back too; both are kept.
    rig.feed(vec![AppEvent::TxState {
        txid,
        state: openbmt_net_io::RelayState::SeenBackAndHeldByNode { groups: 2 },
    }]);
    assert_eq!(
        rig.node.send_state(&txid),
        Some(SendState::HeldByNode { groups: 2 })
    );

    // A block holds it.
    rig.mine(std::slice::from_ref(&tx));
    assert_eq!(
        rig.node.send_state(&txid),
        Some(SendState::Confirmed { depth: 1 })
    );
    let mut want = vec![(change, 1), (sale_coin, 7)];
    want.sort();
    assert_eq!(offered(&rig.node), want);
    rig.mine_empty(4);
    assert_eq!(
        rig.node.send_state(&txid),
        Some(SendState::Confirmed { depth: 5 })
    );
    assert!(!rig.node.owner().outbox().is_empty());
    // The refund depth: done, paid, and the driver lets go of it.
    rig.mine_empty(1);
    assert_eq!(rig.node.send_state(&txid), Some(SendState::Done));
    assert!(rig.node.owner().outbox().is_empty());
    assert!(matches!(
        rig.node
            .owner()
            .refunds()
            .iter()
            .find(|r| r.id == refund)
            .map(|r| r.stage.clone()),
        Some(Stage::Paid { .. })
    ));
    assert!(rig.calls().contains(&format!("main forget {txid}")));
    assert_eq!(rig.node.balance(), rig.node.view().balance());
}

#[test]
fn nr_86_paid_refund_reopens_from_undo_after_depth_and_survives_restart() {
    let dir = Dir::new();
    let mut rig = start(&dir, &Setup::default()).unwrap();
    let refund = rig.queue_refund(10_000);
    rig.fund(300_000);
    let txid = rig.spend();
    let tx = rig.tx(&txid);
    rig.mine(std::slice::from_ref(&tx));
    rig.mine_empty(5);
    assert!(matches!(
        rig.node.owner().refunds().iter().find(|r| r.id == refund).unwrap().stage,
        Stage::Paid { txid: paid } if paid == txid
    ));
    assert!(rig.node.owner().outbox().is_empty());
    assert_eq!(rig.node.archive_paid().unwrap(), 0);

    // Replace heights 7..12 with a longer branch. The old confirmation
    // remains in the 100-block undo horizon, including its signed bytes.
    let left: Vec<BlockHash> = rig.chain[6..].iter().rev().map(Block::block_hash).collect();
    let mut branch = Vec::new();
    let mut prev = rig.chain[5].block_hash();
    for height in 7..=13 {
        let block = mine(prev, height, height, &[]);
        prev = block.block_hash();
        branch.push(block);
    }
    let joined = branch
        .iter()
        .enumerate()
        .map(|(i, block)| (u32::try_from(i).unwrap() + 7, block.block_hash()))
        .collect();
    rig.chain.truncate(6);
    rig.chain.extend(branch.iter().cloned());
    rig.feed(vec![AppEvent::Chain { joined, left }]);
    for (i, block) in branch.iter().enumerate() {
        let height = u32::try_from(i).unwrap() + 7;
        rig.deliver(vec![
            AppEvent::Block {
                hash: block.block_hash(),
                bytes: serialize(block),
            },
            AppEvent::Checked(height),
        ]);
    }
    assert!(matches!(
        rig.node.owner().refunds().iter().find(|r| r.id == refund).unwrap().stage,
        Stage::Sent { txid: reopened } if reopened == txid
    ));
    let delivery = rig
        .node
        .owner()
        .outbox()
        .iter()
        .find_map(|d| match &d.what {
            Delivery::Transaction { txid: held, tx, .. } if *held == txid => Some(tx.clone()),
            _ => None,
        })
        .unwrap();
    assert_eq!(deserialize::<Transaction>(&delivery).unwrap(), tx);
    assert_eq!(rig.node.send_state(&txid), Some(SendState::Sent));

    let chain = rig.chain.clone();
    rig.node.shutdown();
    drop(rig);
    let mut rig = start_at(&dir, &Setup::default(), chain).unwrap();
    assert_eq!(rig.node.send_state(&txid), Some(SendState::Sent));
    let restarted_tx = rig.tx(&txid);
    assert_eq!(restarted_tx, tx);

    rig.mine(std::slice::from_ref(&tx));
    rig.mine_empty(5);
    assert!(matches!(
        rig.node.owner().refunds().iter().find(|r| r.id == refund).unwrap().stage,
        Stage::Paid { txid: paid } if paid == txid
    ));
    rig.mine_empty(95);
    assert_eq!(rig.node.archive_paid().unwrap(), 1);
    assert!(rig.node.owner().refunds().iter().all(|r| r.id != refund));
}

/// The position of the refund output (the one to `external(9)`).
fn tx_pays_first(tx: &Transaction) -> usize {
    tx.output
        .iter()
        .position(|o| o.script_pubkey == external(9))
        .unwrap()
}

#[test]
fn nr_86_a_restart_sends_again_what_was_not_confirmed_and_signs_nothing_again() {
    let dir = Dir::new();
    let (txid, chain) = {
        let mut rig = start(&dir, &Setup::default()).unwrap();
        rig.queue_refund(10_000);
        rig.fund(300_000);
        (rig.spend(), rig.chain.clone())
    };
    let rig = start_at(&dir, &Setup::default(), chain).unwrap();
    assert_eq!(
        rig.calls()
            .iter()
            .filter(|c| **c == format!("main broadcast {txid}"))
            .count(),
        1,
        "the restarted node sends it again"
    );
    assert_eq!(rig.node.send_state(&txid), Some(SendState::Sent));
    assert_eq!(rig.txids(), vec![txid], "nothing was signed again");
    assert!(rig.node.owner().open_sessions().is_empty());
    assert!(matches!(
        rig.node.owner().refunds()[0].stage,
        Stage::Sent { .. }
    ));
    // The transaction and its change are in the view, and its input is not
    // offered.
    assert_eq!(rig.node.view().pending().count(), 1);
    // The change, and the coin of the sale the refund is for.
    assert_eq!(rig.node.coins().len(), 2);
}

#[test]
fn nr_86_a_restart_that_finds_it_in_a_block_follows_it_to_the_depth() {
    let dir = Dir::new();
    let (txid, chain) = {
        let mut rig = start(&dir, &Setup::default()).unwrap();
        rig.queue_refund(10_000);
        rig.fund(300_000);
        let txid = rig.spend();
        let tx = rig.tx(&txid);
        rig.mine(&[tx]);
        (txid, rig.chain.clone())
    };
    let rig = start_at(&dir, &Setup::default(), chain).unwrap();
    // Confirmed: not sent again.
    assert!(rig.calls().iter().all(|c| !c.contains("broadcast")));
    assert_eq!(rig.txids(), vec![txid]);
    assert_eq!(
        rig.node.send_state(&txid),
        Some(SendState::Confirmed { depth: 1 })
    );
}

#[test]
fn nr_86_a_transaction_another_spent_over_is_replaced_and_not_signed_again() {
    let dir = Dir::new();
    let mut rig = start(&dir, &Setup::default()).unwrap();
    rig.queue_refund(10_000);
    let pay = rig.fund(300_000);
    let txid = rig.spend();
    // A block spends the same coin in another transaction.
    let other = tx_from(
        OutPoint::new(pay.compute_txid(), 0),
        &[(&external(77), 299_000)],
    );
    rig.mine(&[other]);
    assert_eq!(rig.node.send_state(&txid), Some(SendState::Replaced));
    assert!(rig.node.owner().outbox().is_empty());
    match &rig.node.owner().refunds()[0].stage {
        Stage::Returned { reason } => assert!(reason.contains("spent by another"), "{reason}"),
        other => panic!("{other:?}"),
    }
    assert!(rig.node.owner().open_sessions().is_empty());
    assert!(rig.node.view().pending().next().is_none());
    // Only the coin of the sale the refund is for is left.
    assert_eq!(rig.node.coins().len(), 1);
    assert!(rig.calls().contains(&format!("main forget {txid}")));
    let notes = rig.node.watcher_mut().take_notes();
    assert!(
        notes.iter().any(|n| n.contains("was replaced")),
        "{notes:?}"
    );
}

/// A refund exported to a hardware signer and then abandoned: the unsigned
/// transaction of the file, whose ID signatures do not change.
fn exported_then_abandoned(rig: &mut Rig) -> (Transaction, u32) {
    let refund = rig.queue_refund(10_000);
    rig.fund(300_000);
    let (_, session) = rig.node.build_request_for_signer(1_000, NOW).unwrap();
    let session = session.expect("the refund is a spend for the signer");
    let mut signer = key1();
    let file = rig
        .node
        .export_for_signer(
            &session,
            0,
            &mut signer,
            &kit().accounts[0],
            NOW,
            |_| Ok(()),
        )
        .unwrap();
    let tx = bitcoin::psbt::Psbt::deserialize(&file.psbt)
        .unwrap()
        .unsigned_tx;
    let write = rig.node.queue_writer();
    rig.node
        .owner_mut()
        .unwrap()
        .abandon_session(&session, &write)
        .unwrap();
    assert_eq!(rig.node.owner().signed_elsewhere().len(), 1);
    (tx, refund)
}

#[test]
fn as_89_an_abandoned_approval_whose_transaction_confirms_pays_its_entries() {
    let dir = Dir::new();
    let mut rig = start(&dir, &Setup::default()).unwrap();
    let (tx, refund) = exported_then_abandoned(&mut rig);
    let txid = tx.compute_txid();
    // The refund is back in the queue, holding what the old one spends.
    assert!(matches!(rig.node.owner().refunds()[0].stage, Stage::Queued));
    assert_eq!(rig.node.owner().refunds()[0].conflict.len(), tx.input.len());
    // A block holds the old transaction, finished elsewhere.
    rig.mine(std::slice::from_ref(&tx));
    assert!(rig.node.owner().signed_elsewhere().is_empty());
    let entry = rig
        .node
        .owner()
        .refunds()
        .iter()
        .find(|r| r.id == refund)
        .cloned()
        .unwrap();
    assert_eq!(entry.stage, Stage::Paid { txid });
    // It is in the log of what was paid, once.
    assert_eq!(rig.node.paid_log().len(), 1);
    assert_eq!(rig.node.paid_log()[0].txid, txid);
    let notes = rig.node.watcher_mut().take_notes();
    assert!(
        notes.iter().any(|n| n.contains("signed elsewhere")),
        "{notes:?}"
    );
    // A restart keeps it paid.
    drop(rig);
    let rig = start(&dir, &Setup::default()).unwrap();
    assert!(rig.node.owner().signed_elsewhere().is_empty());
    assert!(matches!(
        rig.node.owner().refunds()[0].stage,
        Stage::Paid { .. }
    ));
}

#[test]
fn as_89_an_abandoned_approval_whose_coin_another_transaction_spent_is_released() {
    let dir = Dir::new();
    let mut rig = start(&dir, &Setup::default()).unwrap();
    let (tx, _) = exported_then_abandoned(&mut rig);
    let other = tx_from(tx.input[0].previous_output, &[(&external(77), 1_000)]);
    rig.mine(&[other]);
    // OC-55: the old transaction is still watched and the refund is held
    // until the block that beat it is deep; then it is let go.
    assert_eq!(rig.node.owner().held_entries().len(), 1);
    assert_eq!(rig.node.owner().signed_elsewhere().len(), 1);
    assert!(matches!(rig.node.owner().refunds()[0].stage, Stage::Queued));
    assert!(rig.node.paid_log().is_empty());
    rig.mine_empty(99);
    assert_eq!(rig.node.owner().signed_elsewhere().len(), 1);
    rig.mine_empty(1);
    assert!(rig.node.owner().signed_elsewhere().is_empty());
    assert!(rig.node.owner().held_entries().is_empty());
}

#[test]
fn as_89_a_transaction_that_confirms_while_its_approval_is_open_pays_its_entries_and_ends_it() {
    let dir = Dir::new();
    let mut rig = start(&dir, &Setup::default()).unwrap();
    let refund = rig.queue_refund(10_000);
    rig.fund(300_000);
    let (_, session) = rig.node.build_request_for_signer(1_000, NOW).unwrap();
    let session = session.unwrap();
    let mut signer = key1();
    let file = rig
        .node
        .export_for_signer(
            &session,
            0,
            &mut signer,
            &kit().accounts[0],
            NOW,
            |_| Ok(()),
        )
        .unwrap();
    let tx = bitcoin::psbt::Psbt::deserialize(&file.psbt)
        .unwrap()
        .unsigned_tx;
    let txid = tx.compute_txid();
    // The signer's answer was never brought back: the transaction is
    // finished and sent elsewhere, and a block holds it.
    rig.mine(std::slice::from_ref(&tx));
    let entry = rig
        .node
        .owner()
        .refunds()
        .iter()
        .find(|r| r.id == refund)
        .cloned()
        .unwrap();
    assert_eq!(entry.stage, Stage::Paid { txid });
    assert!(!rig.node.owner().open_sessions().contains(&session));
    assert!(rig.node.owner().signed_elsewhere().is_empty());
    assert_eq!(rig.node.paid_log().len(), 1);
    // There is nothing left to abandon.
    assert!(rig.node.abandon_session(&session).is_err());
    drop(rig);
    let rig = start(&dir, &Setup::default()).unwrap();
    assert!(matches!(
        rig.node.owner().refunds()[0].stage,
        Stage::Paid { .. }
    ));
}

#[test]
fn as_89_an_exported_approval_whose_coin_is_gone_is_not_abandoned_into_the_queue() {
    let dir = Dir::new();
    let mut rig = start(&dir, &Setup::default()).unwrap();
    rig.queue_refund(10_000);
    rig.fund(300_000);
    let (_, session) = rig.node.build_request_for_signer(1_000, NOW).unwrap();
    let session = session.unwrap();
    let mut signer = key1();
    let file = rig
        .node
        .export_for_signer(
            &session,
            0,
            &mut signer,
            &kit().accounts[0],
            NOW,
            |_| Ok(()),
        )
        .unwrap();
    let tx = bitcoin::psbt::Psbt::deserialize(&file.psbt)
        .unwrap()
        .unsigned_tx;
    // A block the node applied spent a coin of the file in a transaction
    // that is not the file's, which nothing here was watching for.
    let other = tx_from(tx.input[0].previous_output, &[(&external(77), 1_000)]);
    rig.mine(&[other]);
    let before = rig.node.owner().snapshot().unwrap();
    assert!(rig.node.abandon_session(&session).is_err());
    // Nothing changed: the entry is still in its approval, held.
    assert_eq!(rig.node.owner().snapshot().unwrap(), before);
    assert!(rig.node.owner().open_sessions().contains(&session));
    assert!(matches!(
        rig.node.owner().refunds()[0].stage,
        Stage::InSession { .. }
    ));
}

#[test]
fn nr_86_a_block_that_leaves_sends_a_confirmed_transaction_again() {
    let dir = Dir::new();
    let mut rig = start(&dir, &Setup::default()).unwrap();
    rig.queue_refund(10_000);
    rig.fund(300_000);
    let txid = rig.spend();
    let tx = rig.tx(&txid);
    let block = rig.mine(&[tx]);
    assert!(matches!(
        rig.node.send_state(&txid),
        Some(SendState::Confirmed { .. })
    ));
    let before = rig.calls().len();
    let other = mine(block.header.prev_blockhash, 7, 9, &[]);
    rig.chain.pop();
    rig.chain.push(other.clone());
    rig.feed(vec![
        AppEvent::Chain {
            joined: vec![(7, other.block_hash())],
            left: vec![block.block_hash()],
        },
        AppEvent::Block {
            hash: other.block_hash(),
            bytes: serialize(&other),
        },
        AppEvent::Checked(7),
    ]);
    assert_eq!(rig.node.send_state(&txid), Some(SendState::Sent));
    assert!(rig.calls()[before..].contains(&format!("main broadcast {txid}")));
    // Its input is held back again: the coin is not offered twice.
    assert_eq!(rig.node.view().pending().count(), 1);
}

#[test]
fn nr_88_not_seen_is_shown_and_noted() {
    let dir = Dir::new();
    let mut rig = start(&dir, &Setup::default()).unwrap();
    rig.queue_refund(10_000);
    rig.fund(300_000);
    let txid = rig.spend();
    rig.feed(vec![AppEvent::TxState {
        txid,
        state: openbmt_net_io::RelayState::NotSeen,
    }]);
    assert_eq!(rig.node.send_state(&txid), Some(SendState::NotSeen));
    let notes = rig.node.watcher_mut().take_notes();
    assert!(
        notes.iter().any(|n| n.contains("not seen back")),
        "{notes:?}"
    );
}

// ---- Tor -----------------------------------------------------------------------------

fn some_tx() -> Vec<u8> {
    serialize(&tx_from(
        OutPoint::new(Txid::from_byte_array([3; 32]), 0),
        &[(&external(5), 1_000)],
    ))
}

#[test]
fn nr_91_with_no_proxy_and_no_setting_nothing_is_sent_and_it_says_why() {
    let dir = Dir::new();
    let setup = Setup {
        without_tor: false,
        ..Setup::default()
    };
    let mut rig = start(&dir, &setup).unwrap();
    let err = rig.node.broadcast(&some_tx()).unwrap_err();
    assert_eq!(err, SendRefused::NoTor);
    assert_eq!(
        err.to_string(),
        "Tor is not set up for sending; turn on sending without Tor or set the proxy"
    );
    assert_eq!(err.to_string(), NO_TOR);
    let txid = deserialize::<Transaction>(&some_tx())
        .unwrap()
        .compute_txid();
    assert_eq!(rig.node.send_state(&txid), Some(SendState::Waiting));
    assert!(rig.calls().iter().all(|c| !c.contains("broadcast")));
    assert_eq!(rig.spawned.lock().unwrap().len(), 1, "no second driver");
}

#[test]
fn nr_91_a_refund_waits_without_tor_and_stays_signed() {
    let dir = Dir::new();
    let setup = Setup {
        without_tor: false,
        ..Setup::default()
    };
    let mut rig = start(&dir, &setup).unwrap();
    rig.queue_refund(10_000);
    rig.fund(300_000);
    let txid = rig.spend();
    assert_eq!(rig.node.send_state(&txid), Some(SendState::Waiting));
    // Signed, not sent: it was not reported as broadcast.
    assert!(matches!(
        rig.node.owner().refunds()[0].stage,
        Stage::Signed { .. }
    ));
    assert!(rig.calls().iter().all(|c| !c.contains("broadcast")));
    // Its change is still a coin, so that the owner's view of the wallet is
    // whole.
    assert_eq!(rig.node.view().pending().count(), 1);
}

#[test]
fn nr_91_sending_without_tor_says_so_each_time() {
    let dir = Dir::new();
    let mut rig = start(&dir, &Setup::default()).unwrap();
    let first = some_tx();
    let second = serialize(&tx_from(
        OutPoint::new(Txid::from_byte_array([4; 32]), 0),
        &[(&external(6), 2_000)],
    ));
    rig.node.broadcast(&first).unwrap();
    rig.node.broadcast(&second).unwrap();
    let notes = rig.node.watcher_mut().take_notes();
    assert_eq!(
        notes
            .iter()
            .filter(|n| n.starts_with("sending without Tor"))
            .count(),
        2,
        "{notes:?}"
    );
    assert_eq!(
        rig.calls()
            .iter()
            .filter(|c| c.contains("broadcast"))
            .count(),
        2
    );
}

#[test]
fn nr_91_a_proxy_means_a_second_driver_in_its_own_directory_that_does_the_sending() {
    let dir = Dir::new();
    let proxy: SocketAddr = "127.0.0.1:9050".parse().unwrap();
    let setup = Setup {
        proxy: Some(proxy),
        without_tor: false,
        ..Setup::default()
    };
    let mut rig = start(&dir, &setup).unwrap();
    {
        let spawned = rig.spawned.lock().unwrap();
        assert_eq!(spawned.len(), 2);
        assert_eq!(spawned[0].proxy, None);
        assert_eq!(spawned[1].proxy, Some(proxy));
        assert_eq!(spawned[1].dir, dir.path().join("broadcast"));
        assert_eq!(spawned[1].scripts, 0, "it watches nothing");
    }
    let txid = rig.node.broadcast(&some_tx()).unwrap();
    assert!(rig.calls().contains(&format!("send broadcast {txid}")));
    assert!(rig.calls().iter().all(|c| !c.starts_with("main broadcast")));
    // Its state comes from the second driver's events.
    rig.sender
        .as_ref()
        .unwrap()
        .send(AppEvent::TxState {
            txid,
            state: openbmt_net_io::RelayState::SeenBack { groups: 3 },
        })
        .unwrap();
    rig.node.pump(Duration::ZERO);
    assert_eq!(
        rig.node.send_state(&txid),
        Some(SendState::SeenBack { groups: 3 })
    );
    // Both drivers are asked not to fetch what peers announce.
    assert!(rig.calls().contains(&"send fetch false".to_string()));
    assert!(rig.calls().contains(&"main fetch false".to_string()));
    // The second driver has a lock of its own.
    assert!(dir.path().join("broadcast").is_dir());
}

#[test]
fn nr_86_bytes_that_are_not_a_transaction_are_refused() {
    let dir = Dir::new();
    let mut rig = start(&dir, &Setup::default()).unwrap();
    assert_eq!(rig.node.broadcast(&[1, 2, 3]), Err(SendRefused::Malformed));
}

#[test]
fn the_weight_limit_the_node_relies_on_is_the_signers() {
    assert_eq!(
        openbmt_net::MAX_BROADCAST_WEIGHT,
        openbmt_signer::MAX_WEIGHT
    );
}

#[test]
fn oc_07_start_refuses_an_over_limit_watch_range_before_driver_or_mark_write() {
    let dir = Dir::new();
    let mut view = CoinView::new(0);
    view.raise_marks(99_961, 0, |_| Ok::<(), ()>(())).unwrap();
    let saved = view.to_bytes();
    fs::write(dir.path().join(COINS_FILE), &saved).unwrap();

    let mut config = OwnerConfig::new(dir.path(), id(0x11), DriverConfig::new(NET, 0));
    config.health = Box::new(|_| openbmt_watch::Health {
        healthy: true,
        problem: None,
        behind: false,
    });
    let mut parts =
        OwnerParts::new(OWNER_SECRET, kit().wallet.clone(), approver_id(), policy()).unwrap();
    parts.random = Box::new(Counter(1));
    let spawned = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let record = spawned.clone();
    let result = OwnerNode::<FakePort>::start_with(config, parts, move |_, _, _| {
        record.store(true, std::sync::atomic::Ordering::SeqCst);
        Err("watch limit should stop startup first".to_string())
    });
    match result {
        Err(NodeError::WatchLimit { needed, limit }) => {
            assert_eq!(needed, 100_001);
            assert_eq!(limit, openbmt_net_io::MAX_WATCHED_SCRIPTS);
        }
        Err(other) => panic!("unexpected startup error: {other}"),
        Ok(_) => panic!("over-limit wallet started"),
    }
    assert!(!spawned.load(std::sync::atomic::Ordering::SeqCst));
    assert_eq!(fs::read(dir.path().join(COINS_FILE)).unwrap(), saved);
}

#[test]
fn nr_79_second_owner_node_start_locks_before_reading_or_changing_state() {
    let dir = Dir::new();
    let _first = start(&dir, &Setup::default()).unwrap();

    // If startup reaches any state read before taking the directory lock,
    // these deliberately invalid files cause a different error. Keep their
    // bytes to prove the failed start does not rewrite them either.
    let sentinels = [
        (COINS_FILE, vec![0xc1]),
        (crate::QUEUE_FILE, vec![0xa1]),
        (crate::TAKE_JOURNAL_FILE, vec![0xb2]),
    ];
    for (name, bytes) in &sentinels {
        fs::write(dir.path().join(name), bytes).unwrap();
    }

    let config = OwnerConfig::new(dir.path(), id(0x11), DriverConfig::new(NET, 0));
    let parts =
        OwnerParts::new(OWNER_SECRET, kit().wallet.clone(), approver_id(), policy()).unwrap();
    let spawned = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let record = spawned.clone();
    let result = OwnerNode::<FakePort>::start_with(config, parts, move |_, _, _| {
        record.store(true, std::sync::atomic::Ordering::SeqCst);
        Err("a locked node must not spawn".to_string())
    });

    assert!(matches!(
        result,
        Err(NodeError::Start(crate::OwnerStartError::Locked(_)))
    ));
    assert!(!spawned.load(std::sync::atomic::Ordering::SeqCst));
    for (name, bytes) in sentinels {
        assert_eq!(fs::read(dir.path().join(name)).unwrap(), bytes);
    }
}

// ---- OC-52: settlements a block owes the queue (on AS-89) ------------------

/// An approval exported to a hardware signer and left open: its refund, its
/// session and the unsigned transaction of the file.
fn exported_open(rig: &mut Rig) -> (Transaction, u32, [u8; 32]) {
    let refund = rig.queue_refund(10_000);
    rig.fund(300_000);
    let (_, session) = rig.node.build_request_for_signer(1_000, NOW).unwrap();
    let session = session.expect("the refund is a spend for the signer");
    let mut signer = key1();
    let file = rig
        .node
        .export_for_signer(
            &session,
            0,
            &mut signer,
            &kit().accounts[0],
            NOW,
            |_| Ok(()),
        )
        .unwrap();
    let tx = bitcoin::psbt::Psbt::deserialize(&file.psbt)
        .unwrap()
        .unsigned_tx;
    (tx, refund, session)
}

/// Restart the node with a take journal pending, as after a take whose key
/// store write failed: the journal holds the queue as it is.
fn restart_with_journal(dir: &Dir, rig: Rig) -> Rig {
    let chain = rig.chain.clone();
    drop(rig);
    let journal = crate::take_journal::TakeJournal {
        queue: fs::read(dir.path().join(crate::QUEUE_FILE)).unwrap(),
        key1: key1().to_bytes(),
    };
    fs::write(
        dir.path().join(crate::TAKE_JOURNAL_FILE),
        journal.to_bytes().unwrap(),
    )
    .unwrap();
    let rig = start_at(dir, &Setup::default(), chain).unwrap();
    assert!(rig.node.take_recovery_pending());
    rig
}

fn recover(rig: &mut Rig) {
    let mut signer = key1();
    rig.node
        .recover_key1(&mut signer, &kit().accounts[0], |_| Ok(()))
        .unwrap();
    assert!(!rig.node.take_recovery_pending());
}

fn owed_on_disk(dir: &Dir) -> crate::SettleOwed {
    match fs::read(dir.path().join(crate::SETTLE_OWED_FILE)) {
        Ok(bytes) => crate::SettleOwed::from_bytes(&bytes).unwrap(),
        Err(_) => crate::SettleOwed::new(),
    }
}

/// A directory where a file's temporary copy goes makes every write of that
/// file fail until it is removed.
fn block_writes(dir: &Dir, file: &str) -> PathBuf {
    let path = dir.path().join(format!("{file}.tmp"));
    fs::create_dir(&path).unwrap();
    path
}

fn refund_stage(rig: &Rig) -> Stage {
    rig.node.owner().refunds()[0].stage.clone()
}

#[test]
fn oc_52_as_89_an_exported_payment_confirmed_during_key_recovery_settles_once_after_it() {
    let dir = Dir::new();
    let mut rig = start(&dir, &Setup::default()).unwrap();
    let (tx, _, session) = exported_open(&mut rig);
    let txid = tx.compute_txid();
    let mut rig = restart_with_journal(&dir, rig);
    rig.mine(std::slice::from_ref(&tx));
    // Nothing is settled while key 1 recovery is pending, and it is owed.
    assert!(rig.node.owner().open_sessions().contains(&session));
    assert!(rig.node.paid_log().is_empty());
    assert_eq!(owed_on_disk(&dir).items().len(), 1);
    assert_eq!(owed_on_disk(&dir).items()[0].txid, txid);
    let notes = rig.node.watcher_mut().take_notes();
    assert!(
        notes.iter().any(|n| n.contains("key 1 recovery")),
        "{notes:?}"
    );
    recover(&mut rig);
    assert_eq!(refund_stage(&rig), Stage::Paid { txid });
    assert!(!rig.node.owner().open_sessions().contains(&session));
    assert_eq!(rig.node.paid_log().len(), 1);
    assert!(owed_on_disk(&dir).is_empty());
    assert!(rig.node.abandon_session(&session).is_err());
    // More blocks settle nothing again.
    rig.mine_empty(2);
    assert_eq!(rig.node.paid_log().len(), 1);
}

#[test]
fn oc_52_as_89_a_failed_queue_write_is_settled_on_the_next_tick_and_logged_once() {
    let dir = Dir::new();
    let mut rig = start(&dir, &Setup::default()).unwrap();
    let (tx, _, session) = exported_open(&mut rig);
    let txid = tx.compute_txid();
    let blocked = block_writes(&dir, crate::QUEUE_FILE);
    rig.mine(std::slice::from_ref(&tx));
    // The log was written, the queue was not: the approval is still open.
    assert_eq!(rig.node.paid_log().len(), 1);
    assert!(rig.node.owner().open_sessions().contains(&session));
    assert_eq!(owed_on_disk(&dir).items().len(), 1);
    // Still failing on the tick.
    rig.deliver(vec![]);
    assert_eq!(owed_on_disk(&dir).items().len(), 1);
    fs::remove_dir(blocked).unwrap();
    rig.deliver(vec![]);
    assert_eq!(refund_stage(&rig), Stage::Paid { txid });
    assert!(!rig.node.owner().open_sessions().contains(&session));
    assert_eq!(rig.node.paid_log().len(), 1);
    assert!(owed_on_disk(&dir).is_empty());
}

#[test]
fn oc_52_as_89_a_restart_between_the_owed_write_and_the_coin_view_commit_settles_once() {
    let dir = Dir::new();
    let mut rig = start(&dir, &Setup::default()).unwrap();
    let (tx, _, session) = exported_open(&mut rig);
    let txid = tx.compute_txid();
    // The watch set grows with the approval; let that settle first.
    rig.feed(vec![]);
    let blocked = block_writes(&dir, COINS_FILE);
    rig.mine(std::slice::from_ref(&tx));
    // The block was written down as owed, and the coin view did not take it.
    assert!(rig.node.halted().is_some());
    assert_eq!(owed_on_disk(&dir).items().len(), 1);
    assert!(rig.node.owner().open_sessions().contains(&session));
    let chain = rig.chain.clone();
    drop(rig);
    fs::remove_dir(blocked).unwrap();
    let rig = start_at(&dir, &Setup::default(), chain).unwrap();
    assert_eq!(refund_stage(&rig), Stage::Paid { txid });
    assert!(!rig.node.owner().open_sessions().contains(&session));
    assert_eq!(rig.node.paid_log().len(), 1);
    assert!(owed_on_disk(&dir).is_empty());
}

#[test]
fn oc_52_as_89_a_settlement_still_owed_at_start_is_settled_at_start() {
    let dir = Dir::new();
    let mut rig = start(&dir, &Setup::default()).unwrap();
    let (tx, _, session) = exported_open(&mut rig);
    let txid = tx.compute_txid();
    let blocked = block_writes(&dir, crate::QUEUE_FILE);
    rig.mine(std::slice::from_ref(&tx));
    assert_eq!(owed_on_disk(&dir).items().len(), 1);
    let chain = rig.chain.clone();
    drop(rig);
    fs::remove_dir(blocked).unwrap();
    let mut rig = start_at(&dir, &Setup::default(), chain).unwrap();
    assert_eq!(refund_stage(&rig), Stage::Paid { txid });
    assert!(!rig.node.owner().open_sessions().contains(&session));
    assert_eq!(rig.node.paid_log().len(), 1);
    assert!(owed_on_disk(&dir).is_empty());
    let notes = rig.node.watcher_mut().take_notes();
    assert!(
        notes.iter().any(|n| n.contains("its entries are paid")),
        "{notes:?}"
    );
}

#[test]
fn oc_52_as_89_a_settlement_owed_by_a_block_that_left_the_chain_is_dropped_unlogged() {
    let dir = Dir::new();
    let mut rig = start(&dir, &Setup::default()).unwrap();
    let (tx, _, session) = exported_open(&mut rig);
    let mut rig = restart_with_journal(&dir, rig);
    let first = rig.mine(std::slice::from_ref(&tx));
    assert_eq!(owed_on_disk(&dir).items().len(), 1);
    // Another block takes its place.
    let height = u32::try_from(rig.chain.len()).unwrap();
    let parent = if height >= 2 {
        rig.chain[usize::try_from(height).unwrap() - 2].block_hash()
    } else {
        bitcoin::blockdata::constants::genesis_block(NET).block_hash()
    };
    let other = mine(parent, height, 7, &[]);
    rig.feed(vec![AppEvent::Chain {
        joined: vec![(height, other.block_hash())],
        left: vec![first.block_hash()],
    }]);
    assert!(owed_on_disk(&dir).is_empty());
    let notes = rig.node.watcher_mut().take_notes();
    assert!(
        notes.iter().any(|n| n.contains("left the chain")),
        "{notes:?}"
    );
    recover(&mut rig);
    assert!(rig.node.paid_log().is_empty());
    assert!(rig.node.owner().open_sessions().contains(&session));
    assert!(matches!(refund_stage(&rig), Stage::InSession { .. }));
}

#[test]
fn oc_52_as_89_a_signed_elsewhere_payment_confirmed_during_key_recovery_settles_once_after_it() {
    let dir = Dir::new();
    let mut rig = start(&dir, &Setup::default()).unwrap();
    let (tx, _) = exported_then_abandoned(&mut rig);
    let txid = tx.compute_txid();
    let mut rig = restart_with_journal(&dir, rig);
    rig.mine(std::slice::from_ref(&tx));
    assert_eq!(rig.node.owner().signed_elsewhere().len(), 1);
    assert!(rig.node.paid_log().is_empty());
    assert_eq!(owed_on_disk(&dir).items().len(), 1);
    recover(&mut rig);
    assert!(rig.node.owner().signed_elsewhere().is_empty());
    assert_eq!(refund_stage(&rig), Stage::Paid { txid });
    assert_eq!(rig.node.paid_log().len(), 1);
    assert!(owed_on_disk(&dir).is_empty());
}

#[test]
fn oc_52_as_89_a_signed_elsewhere_payment_beaten_during_key_recovery_is_released_after_it() {
    let dir = Dir::new();
    let mut rig = start(&dir, &Setup::default()).unwrap();
    let (tx, _) = exported_then_abandoned(&mut rig);
    let other = tx_from(tx.input[0].previous_output, &[(&external(77), 1_000)]);
    let mut rig = restart_with_journal(&dir, rig);
    rig.mine(&[other]);
    assert_eq!(rig.node.owner().signed_elsewhere().len(), 1);
    assert_eq!(owed_on_disk(&dir).items().len(), 1);
    recover(&mut rig);
    assert_eq!(rig.node.owner().held_entries().len(), 1);
    assert!(matches!(refund_stage(&rig), Stage::Queued));
    assert!(rig.node.paid_log().is_empty());
    assert!(owed_on_disk(&dir).is_empty());
}

#[test]
fn oc_52_as_89_a_signed_elsewhere_payment_with_a_failed_queue_write_settles_on_the_next_block() {
    let dir = Dir::new();
    let mut rig = start(&dir, &Setup::default()).unwrap();
    let (tx, _) = exported_then_abandoned(&mut rig);
    let txid = tx.compute_txid();
    let blocked = block_writes(&dir, crate::QUEUE_FILE);
    rig.mine(std::slice::from_ref(&tx));
    assert_eq!(rig.node.owner().signed_elsewhere().len(), 1);
    assert_eq!(rig.node.paid_log().len(), 1);
    fs::remove_dir(blocked).unwrap();
    rig.mine_empty(1);
    assert!(rig.node.owner().signed_elsewhere().is_empty());
    assert_eq!(refund_stage(&rig), Stage::Paid { txid });
    assert_eq!(rig.node.paid_log().len(), 1);
    assert!(owed_on_disk(&dir).is_empty());
}

#[test]
fn oc_52_as_89_a_signed_elsewhere_payment_beaten_with_a_failed_queue_write_settles_later() {
    let dir = Dir::new();
    let mut rig = start(&dir, &Setup::default()).unwrap();
    let (tx, _) = exported_then_abandoned(&mut rig);
    let other = tx_from(tx.input[0].previous_output, &[(&external(77), 1_000)]);
    let blocked = block_writes(&dir, crate::QUEUE_FILE);
    rig.mine(&[other]);
    assert_eq!(rig.node.owner().signed_elsewhere().len(), 1);
    assert_eq!(owed_on_disk(&dir).items().len(), 1);
    fs::remove_dir(blocked).unwrap();
    rig.deliver(vec![]);
    assert_eq!(rig.node.owner().held_entries().len(), 1);
    assert!(matches!(refund_stage(&rig), Stage::Queued));
    assert!(rig.node.paid_log().is_empty());
    assert!(owed_on_disk(&dir).is_empty());
}

#[test]
fn oc_52_an_owed_file_that_does_not_read_back_stops_the_start() {
    let dir = Dir::new();
    fs::write(dir.path().join(crate::SETTLE_OWED_FILE), [0xb2]).unwrap();
    let error = start(&dir, &Setup::default()).err().unwrap();
    assert!(
        matches!(error, NodeError::Files(ref e) if e.contains("OC-52")),
        "{error:?}"
    );
}

#[test]
fn oc_07_a_coin_view_write_that_fails_while_the_marks_grow_stops_the_node() {
    let dir = Dir::new();
    let mut rig = start(&dir, &Setup::default()).unwrap();
    let blocked = block_writes(&dir, COINS_FILE);
    rig.queue_refund(10_000);
    rig.feed(vec![]);
    let halted = rig.node.halted().unwrap_or_default().to_string();
    assert!(halted.contains("coin view could not be stored"), "{halted}");
    assert!(halted.contains(COINS_FILE), "{halted}");
    fs::remove_dir(blocked).unwrap();
}

#[test]
fn oc_07_a_block_that_finds_the_node_stopped_by_the_marks_takes_nothing() {
    let dir = Dir::new();
    let mut rig = start(&dir, &Setup::default()).unwrap();
    let (tx, _, session) = exported_open(&mut rig);
    // The rescans the approval asked for are answered first.
    rig.feed(vec![]);
    // A batch queued without refreshing the marks: the block finds them grown.
    let write = rig.node.queue_writer();
    rig.node
        .owner_mut()
        .unwrap()
        .queue_batch(batch(3, 3), 1_000, &write)
        .unwrap();
    let checked = rig.node.view().checked();
    let blocked = block_writes(&dir, COINS_FILE);
    let height = u32::try_from(rig.chain.len()).unwrap() + 1;
    let block = mine(rig.tip_hash(), height, 0, std::slice::from_ref(&tx));
    rig.deliver(vec![
        AppEvent::Chain {
            joined: vec![(height, block.block_hash())],
            left: vec![],
        },
        AppEvent::Block {
            hash: block.block_hash(),
            bytes: serialize(&block),
        },
    ]);
    assert!(rig.node.halted().is_some());
    assert!(owed_on_disk(&dir).is_empty());
    assert_eq!(rig.node.view().checked(), checked);
    assert!(rig.node.owner().open_sessions().contains(&session));
    fs::remove_dir(blocked).unwrap();
}
