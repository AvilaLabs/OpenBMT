//! The owner node on drivers that the test feeds by hand: the coin view
//! from checked blocks, sending and its states, restarts, replacement,
//! reorganizations and the Tor rule (OC-01 to OC-09, NR-86 to NR-91).

use std::net::SocketAddr;
use std::path::PathBuf;
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
use openbmt_net_io::{AppEvent, DriverConfig, EventReceiver, Randomness};
use openbmt_owner::{Delivery, SaleFacts, SaleKey, Stage};
use openbmt_signer::SignerState;
use openbmt_spend::Coin;

use super::coins::{mine, tx_from};
use super::{id, Dir};
use crate::coins::ViewState;
use crate::{
    NodeError, OwnerConfig, OwnerNode, OwnerParts, OwnerPort, SendRefused, SendState, NO_TOR,
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
}

impl Default for Setup {
    fn default() -> Self {
        Self {
            proxy: None,
            without_tor: true,
            refund_depth: 6,
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
    chain: Vec<Block>,
    /// How much of the log has been looked at for scan requests.
    scans_seen: usize,
}

fn start(dir: &Dir, setup: &Setup) -> Result<Rig, NodeError> {
    start_at(dir, setup, Vec::new())
}

fn start_at(dir: &Dir, setup: &Setup, chain: Vec<Block>) -> Result<Rig, NodeError> {
    let log: Log = Arc::default();
    let spawned: Arc<Mutex<Vec<Spawned>>> = Arc::default();
    let senders: Arc<Mutex<Vec<Sender<AppEvent>>>> = Arc::default();
    let mut config = OwnerConfig::new(dir.path(), id(0x11), DriverConfig::new(NET, 0));
    config.broadcast_proxy = setup.proxy;
    config.broadcast_without_tor = setup.without_tor;
    config.health = Box::new(|_| openbmt_watch::Health {
        healthy: true,
        problem: None,
        behind: false,
    });
    let mut parts = OwnerParts::new(OWNER_SECRET, kit().wallet.clone(), approver_id(), policy())?;
    parts.refund_depth = setup.refund_depth;
    parts.random = Box::new(Counter(1));
    let (l, s, sn) = (log.clone(), spawned.clone(), senders.clone());
    let node = OwnerNode::start_with(config, parts, move |driver, dir, scripts| {
        let (tx, rx) = channel();
        let tag = if s.lock().unwrap().is_empty() {
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
            .queue_batch(batch(0, 3), &write)
            .unwrap();
        self.node.refresh_marks();
        self.node
            .owner_mut()
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
            .queue_refund(sale(1), "Mia", external(9), sats, None, &write)
            .unwrap()
    }

    /// Fund receive index 0 with a payment, and bury it.
    fn fund(&mut self, sats: u64) -> Transaction {
        let pay = tx_from(
            OutPoint::new(Txid::from_byte_array([7; 32]), 0),
            &[(&receive(0), sats)],
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
    assert_eq!(offered(&rig.node), vec![(op, 6)]);
    assert_eq!(rig.node.balance(), 300_000);
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
fn oc_07_new_marks_watch_new_scripts_and_scan_from_the_birthday_once() {
    let dir = Dir::new();
    let mut rig = start(&dir, &Setup::default()).unwrap();
    // Nothing is handed out: only the gap is watched, on both keychains.
    assert_eq!(rig.spawned.lock().unwrap()[0].scripts, 40);
    let before = rig.calls();
    rig.queue_refund(10_000);
    let after = rig.calls();
    let new = &after[before.len()..];
    // 3 more receive scripts, and one scan from the birthday.
    assert_eq!(new.iter().filter(|c| *c == "main watch").count(), 3);
    assert_eq!(new.iter().filter(|c| *c == "main rescan 0").count(), 1);
    rig.node.refresh_marks();
    assert_eq!(
        rig.calls().len(),
        after.len(),
        "nothing grew, nothing is said"
    );
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
    assert_eq!(rig.node.balance(), 300_000);
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
    assert_eq!(offered(&rig.node), vec![(change, 0)]);
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

    // A block holds it.
    rig.mine(std::slice::from_ref(&tx));
    assert_eq!(
        rig.node.send_state(&txid),
        Some(SendState::Confirmed { depth: 1 })
    );
    assert_eq!(offered(&rig.node), vec![(change, 1)]);
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
    assert_eq!(rig.node.coins().len(), 1);
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
    assert!(rig.node.coins().is_empty());
    assert!(rig.calls().contains(&format!("main forget {txid}")));
    let notes = rig.node.watcher_mut().take_notes();
    assert!(
        notes.iter().any(|n| n.contains("was replaced")),
        "{notes:?}"
    );
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
