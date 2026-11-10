//! The owner node: the watcher, the wallet's coins, the owner's queue and the
//! sending of finished transactions (`docs/OWNER-COINS-RULES.md`,
//! `docs/NETWORK-RULES.md` NR-86 to NR-91).
//!
//! [`OwnerNode`] puts four things together. The [`OwnerWatcher`] follows the
//! chain. The [`CoinView`] learns from each checked block what the wallet
//! holds. The `Owner` queue builds a session for the Key app and takes the
//! answer. A transaction that comes out is held as pending, handed to the
//! driver to send, and followed until it is in a block at the refund depth.
//!
//! ## Files
//!
//! In the node's directory: [`COINS_FILE`] (the coin view, `OBM8`) and
//! [`QUEUE_FILE`] (the owner's queue, `OBM4`), both written whole through
//! the watcher's atomic write, and the watcher's own. The queue is written
//! before the coin view takes the new transaction, so a restart finds the
//! transaction in the queue's outbox and picks up from there
//! ([`OwnerNode::start_with`]).
//!
//! ## What a restart does
//!
//! Every transaction in the queue's outbox that is not yet reported
//! confirmed is held again and sent again. A transaction that a block in the
//! last 100 confirmed stays in the outbox until it is at the refund depth.
//! One whose coins the view no longer holds and that no block of the last 100
//! shows is `Unresolved` and is left in the outbox with a note: the view
//! cannot tell whether it confirmed long ago or was replaced, and a rescan
//! (see [`OwnerNode::rescan`]) shows which.
//!
//! ## Sending
//!
//! With a proxy set, sending goes through a second driver of its own
//! (`<dir>/broadcast`, its own lock) that connects through the proxy. With
//! no proxy and no setting that allows it, nothing is sent: the transaction
//! is held as [`SendState::Waiting`] and [`OwnerNode::broadcast`] says why.
//! With the setting, the main driver sends and a note says it is not behind
//! Tor, each time (NR-91).
//!
//! ## Stops
//!
//! A block that cannot be applied, or a view that cannot be stored, halts
//! the node's view of the chain (see [`OwnerNode::halted`]): the view stays
//! where it was and no session is built until the node is restarted. It is
//! never silent: a note says what failed.

use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::io;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use bitcoin::bip32::Xpriv;
use bitcoin::consensus::deserialize;
use bitcoin::{Network, OutPoint, ScriptBuf, Transaction, Txid};
use openbmt_core::batch::BatchPolicy;
use openbmt_core::identity::{DeviceId, Identity};
use openbmt_core::wallet::{Keychain, WalletDescription};
use openbmt_counter_node::{DriverPort, MAX_EVENTS_PER_PUMP};
use openbmt_net_io::{
    AppEvent, Driver, DriverConfig, FileStorage, Handle, OsRandomness, Randomness, RelayState,
    SystemClock,
};
use openbmt_owner::{Built, Delivery, Options, Owner, OwnerError, Stage, Taken};
use openbmt_signer::SignerState;
use openbmt_watch::{HeaderChain, Refusal, WellFormedBlock};

use crate::coins::{ApplyError, CoinView, PendingError, ScriptSet, ViewState, UNDO_BLOCKS};
use crate::watcher::{write_file, OwnerConfig, OwnerStartError, OwnerWatcher};

/// The coin view's file.
pub const COINS_FILE: &str = "coins.bin";
/// The owner's queue file.
pub const QUEUE_FILE: &str = "owner-queue.bin";
/// The directory of the second driver, the one behind the proxy.
pub const BROADCAST_DIR: &str = "broadcast";
/// What [`OwnerNode::broadcast`] says when nothing can send (NR-91).
pub const NO_TOR: &str =
    "Tor is not set up for sending; turn on sending without Tor or set the proxy";
/// Blocks between two writes of the chain position while the node is behind.
const CHECKED_EVERY: u32 = 100;
/// Most states kept; finished ones go first.
const MAX_STATES: usize = 256;

/// The driver as the owner node uses it: the counter node's port, and what
/// the owner node adds to it.
pub trait OwnerPort: DriverPort {
    /// NR-86: hold and announce a transaction.
    fn broadcast(&self, tx: Vec<u8>);
    /// NR-86: stop holding a transaction.
    fn forget(&self, txid: Txid);
    /// NR-90: check the blocks above this height again.
    fn rescan_from(&self, height: u32);
    /// Whether to fetch the transactions that peers announce.
    fn set_fetch_transactions(&self, on: bool);
}

impl OwnerPort for Handle {
    fn broadcast(&self, tx: Vec<u8>) {
        Handle::broadcast(self, tx);
    }

    fn forget(&self, txid: Txid) {
        Handle::forget(self, txid);
    }

    fn rescan_from(&self, height: u32) {
        Handle::rescan_from(self, height);
    }

    fn set_fetch_transactions(&self, on: bool) {
        Handle::set_fetch_transactions(self, on);
    }
}

/// Where a finished transaction is (NR-86 to NR-89).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SendState {
    /// Held, and not sent: nothing is set up to send it (NR-91).
    Waiting,
    /// Announced to the peers; nothing has come back.
    Sent,
    /// Peers that were not asked have announced it, from this many network
    /// groups. This is not a confirmation, and it is not proof that a miner
    /// has it.
    SeenBack {
        /// Distinct groups.
        groups: usize,
    },
    /// Not seen back in ten minutes. It was announced to every peer again.
    NotSeen,
    /// In a checked block, this many blocks deep (the tip counts as 1).
    Confirmed {
        /// Confirmations.
        depth: u32,
    },
    /// At the refund depth, and reported to the queue as paid.
    Done,
    /// A block spent its coins in another transaction. The entries are
    /// back in the queue, to be signed again if the owner chooses.
    Replaced,
    /// Neither held, nor in a block the view still remembers (see the module
    /// docs).
    Unresolved,
}

impl fmt::Display for SendState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Waiting => write!(f, "waiting: not sent, Tor is not set up for sending"),
            Self::Sent => write!(f, "sent"),
            Self::SeenBack { groups } => write!(f, "seen back from {groups} network groups"),
            Self::NotSeen => write!(f, "not seen back"),
            Self::Confirmed { depth } => write!(f, "confirmed, {depth} deep"),
            Self::Done => write!(f, "done"),
            Self::Replaced => write!(f, "replaced by another transaction"),
            Self::Unresolved => write!(f, "unresolved: a rescan will show what happened"),
        }
    }
}

/// Why [`OwnerNode::broadcast`] sent nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SendRefused {
    /// The bytes are not one transaction.
    Malformed,
    /// Neither a proxy nor sending without Tor is set (NR-91). The
    /// transaction is held as [`SendState::Waiting`].
    NoTor,
}

impl fmt::Display for SendRefused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Malformed => write!(f, "the bytes are not one transaction"),
            Self::NoTor => write!(f, "{NO_TOR}"),
        }
    }
}

impl std::error::Error for SendRefused {}

/// Why the node did not start, or did not do what it was asked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NodeError {
    /// The watcher did not start.
    Start(OwnerStartError),
    /// A file of the node could not be read, or written.
    Files(String),
    /// A setting that cannot work.
    Config(String),
    /// The view is not ready to build from, and says why.
    NotReady(String),
    /// The owner's queue refused, or could not be stored.
    Owner(String),
}

impl fmt::Display for NodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Start(e) => write!(f, "{e}"),
            Self::Files(e) => write!(f, "{e}"),
            Self::Config(e) => write!(f, "{e}"),
            Self::NotReady(e) => write!(f, "not ready: {e}"),
            Self::Owner(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for NodeError {}

impl From<OwnerError<String>> for NodeError {
    fn from(e: OwnerError<String>) -> Self {
        Self::Owner(e.to_string())
    }
}

/// What the owner's queue is made of: the keys' public side, the shop's
/// choices and where randomness comes from.
pub struct OwnerParts {
    /// The owner root's secret, the one the watcher's identity is made from.
    pub owner_secret: [u8; 32],
    /// The wallet.
    pub wallet: WalletDescription,
    /// The Key app's identity.
    pub approver: DeviceId,
    /// The batch policy.
    pub policy: BatchPolicy,
    /// The queue's options when it is made. A stored queue keeps its own.
    pub options: Options,
    /// Confirmations at which a transaction is done (the refund depth). At
    /// most 100: the view remembers no more blocks than that.
    pub refund_depth: u32,
    /// The source of the session's random bytes.
    pub random: Box<dyn Randomness>,
}

impl OwnerParts {
    /// Parts with the system's randomness and the default refund depth, 6.
    pub fn new(
        owner_secret: [u8; 32],
        wallet: WalletDescription,
        approver: DeviceId,
        policy: BatchPolicy,
    ) -> Result<Self, NodeError> {
        let random = OsRandomness::new().map_err(|e| NodeError::Config(e.to_string()))?;
        Ok(Self {
            owner_secret,
            wallet,
            approver,
            policy,
            options: Options::default(),
            refund_depth: 6,
            random: Box::new(random),
        })
    }
}

/// The wallet scripts from index 0 up to the marks.
fn scripts_for(wallet: &WalletDescription, receive: u32, change: u32) -> ScriptSet {
    let mut set = ScriptSet::new();
    for i in 0..receive {
        if let Ok(script) = wallet.receive_script(i) {
            set.insert(script, (Keychain::Receive, i));
        }
    }
    for i in 0..change {
        if let Ok(script) = wallet.change_script(i) {
            set.insert(script, (Keychain::Change, i));
        }
    }
    set
}

/// The marks that follow from what the queue and the view hold.
fn marks_of(owner: &Owner, view: &CoinView) -> (u32, u32) {
    let mut receive = owner.receive_mark().max(view.receive_next());
    let mut change = owner.change_mark().max(view.change_next());
    let held = view.owned().map(|c| (c.chain, c.index));
    let pending = view
        .pending()
        .flat_map(|p| p.own.iter().map(|(_, chain, index)| (*chain, *index)));
    for (chain, index) in held.chain(pending) {
        let next = index.saturating_add(1);
        match chain {
            Keychain::Receive => receive = receive.max(next),
            Keychain::Change => change = change.max(next),
        }
    }
    (receive, change)
}

/// The owner node. See the module docs.
pub struct OwnerNode<P: OwnerPort = Handle> {
    watcher: OwnerWatcher<P>,
    /// The second driver, behind the proxy.
    sender: Option<OwnerWatcher<P>>,
    without_tor: bool,
    owner: Owner,
    view: CoinView,
    scripts: ScriptSet,
    wallet: WalletDescription,
    dir: PathBuf,
    refund_depth: u32,
    random: Box<dyn Randomness>,
    states: BTreeMap<Txid, SendState>,
    relays: BTreeMap<Txid, (usize, usize)>,
    halted: Option<String>,
}

impl OwnerNode<Handle> {
    /// Start the node on real drivers.
    pub fn start(config: OwnerConfig, parts: OwnerParts) -> Result<Self, NodeError> {
        Self::start_with(config, parts, |driver, dir, scripts| {
            let storage = FileStorage::new(&dir).map_err(|e| e.to_string())?;
            let random = OsRandomness::new().map_err(|e| e.to_string())?;
            Driver::spawn(driver, storage, SystemClock, random, scripts).map_err(|e| e.to_string())
        })
    }
}

impl<P: OwnerPort> OwnerNode<P> {
    /// Start the node on drivers that `spawn` makes, given each driver's
    /// settings, its directory and the scripts it starts with.
    pub fn start_with(
        config: OwnerConfig,
        parts: OwnerParts,
        mut spawn: impl FnMut(DriverConfig, PathBuf, Vec<ScriptBuf>) -> Result<P, String>,
    ) -> Result<Self, NodeError> {
        let OwnerConfig {
            dir,
            identity,
            driver,
            clock,
            health,
            birthday,
            broadcast_proxy,
            broadcast_without_tor,
            ..
        } = config;
        let OwnerParts {
            owner_secret,
            wallet,
            approver,
            policy,
            options,
            refund_depth,
            random,
        } = parts;
        if refund_depth == 0 || refund_depth > UNDO_BLOCKS {
            return Err(NodeError::Config(format!(
                "the refund depth must be from 1 to {UNDO_BLOCKS}, not {refund_depth}"
            )));
        }
        fs::create_dir_all(&dir).map_err(|e| NodeError::Files(e.to_string()))?;

        let mut view = match fs::read(dir.join(COINS_FILE)) {
            Ok(bytes) => CoinView::from_bytes(&bytes)
                .map_err(|e| NodeError::Files(format!("{COINS_FILE} does not read back: {e}")))?,
            Err(e) if e.kind() == io::ErrorKind::NotFound => CoinView::new(birthday),
            Err(e) => return Err(NodeError::Files(format!("{COINS_FILE}: {e}"))),
        };
        let owner = match fs::read(dir.join(QUEUE_FILE)) {
            Ok(bytes) => Owner::restore(&owner_secret, wallet.clone(), approver, policy, &bytes)
                .map_err(|e| NodeError::Files(format!("{QUEUE_FILE}: {e}")))?,
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                Owner::new(&owner_secret, wallet.clone(), approver, policy, options)
                    .map_err(|e| NodeError::Config(e.to_string()))?
            }
            Err(e) => return Err(NodeError::Files(format!("{QUEUE_FILE}: {e}"))),
        };
        let sender_identity = Identity::from_secret_bytes(&owner_secret)
            .map_err(|e| NodeError::Config(format!("the owner key: {e}")))?;

        // What the queue handed out that the view does not know of yet.
        let (receive, change) = marks_of(&owner, &view);
        view.raise_marks(receive, change, |b| write_file(&dir, COINS_FILE, b))
            .map_err(NodeError::Files)?;
        let scripts = scripts_for(&wallet, view.receive_watch(), view.change_watch());

        let main_config = OwnerConfig {
            dir: dir.clone(),
            identity,
            driver: driver.clone(),
            clock,
            health,
            watch_from: Some(view.checked()),
            birthday,
            broadcast_proxy,
            broadcast_without_tor,
        };
        let initial: Vec<ScriptBuf> = scripts.keys().cloned().collect();
        let watcher = OwnerWatcher::start_with(main_config, |d, p| spawn(d, p, initial))
            .map_err(NodeError::Start)?;
        // The owner node asks for no unconfirmed transaction: only a
        // checked block makes a coin.
        watcher.port().set_fetch_transactions(false);

        let sender = match broadcast_proxy {
            Some(proxy) => {
                let mut sending = driver;
                sending.proxy = Some(proxy);
                let config = OwnerConfig::new(dir.join(BROADCAST_DIR), sender_identity, sending);
                let sender = OwnerWatcher::start_with(config, |d, p| spawn(d, p, Vec::new()))
                    .map_err(NodeError::Start)?;
                sender.port().set_fetch_transactions(false);
                Some(sender)
            }
            None => None,
        };

        let mut node = Self {
            watcher,
            sender,
            without_tor: broadcast_without_tor,
            owner,
            view,
            scripts,
            wallet,
            dir,
            refund_depth,
            random,
            states: BTreeMap::new(),
            relays: BTreeMap::new(),
            halted: None,
        };
        node.reconcile();
        Ok(node)
    }

    // What the node knows.

    /// The watcher.
    pub fn watcher(&self) -> &OwnerWatcher<P> {
        &self.watcher
    }

    /// The watcher, to make heartbeats and read its notes.
    pub fn watcher_mut(&mut self) -> &mut OwnerWatcher<P> {
        &mut self.watcher
    }

    /// The coin view.
    pub fn view(&self) -> &CoinView {
        &self.view
    }

    /// The owner's queue, to read.
    pub fn owner(&self) -> &Owner {
        &self.owner
    }

    /// The owner's queue, to change. A change is stored through
    /// [`OwnerNode::queue_writer`]; call [`OwnerNode::refresh_marks`]
    /// after one that queues a batch.
    pub fn owner_mut(&mut self) -> &mut Owner {
        &mut self.owner
    }

    /// A function that stores the queue where the node keeps it.
    pub fn queue_writer(&self) -> impl Fn(&[u8]) -> Result<(), String> + 'static {
        let dir = self.dir.clone();
        move |bytes| write_file(&dir, QUEUE_FILE, bytes)
    }

    /// The coins that `build_request` would be given: empty unless the view
    /// is ready.
    pub fn coins(&self) -> Vec<openbmt_spend::Coin> {
        self.view.coins(self.watcher.chain().tip_height())
    }

    /// What the wallet holds as far as the view knows, confirmed and the
    /// owner's own change.
    pub fn balance(&self) -> u64 {
        self.view.balance()
    }

    /// The state of the view.
    pub fn state(&self) -> ViewState {
        self.view.state()
    }

    /// Why the node stopped following the chain, if it did.
    pub fn halted(&self) -> Option<&str> {
        self.halted.as_deref()
    }

    /// The network.
    pub fn network(&self) -> Network {
        self.wallet.network()
    }

    /// Where a finished transaction is.
    pub fn send_state(&self, txid: &Txid) -> Option<SendState> {
        let stored = *self.states.get(txid)?;
        if matches!(
            stored,
            SendState::Done | SendState::Replaced | SendState::Unresolved | SendState::Waiting
        ) {
            return Some(stored);
        }
        match self.view.confirmed_height(txid) {
            Some(h) => {
                let tip = self.watcher.chain().tip_height();
                Some(SendState::Confirmed {
                    depth: tip.saturating_sub(h).saturating_add(1),
                })
            }
            None => Some(stored),
        }
    }

    /// Every transaction the node follows, with where it is.
    pub fn send_states(&self) -> Vec<(Txid, SendState)> {
        self.states
            .keys()
            .filter_map(|t| self.send_state(t).map(|s| (*t, s)))
            .collect()
    }

    /// How many peers a transaction was announced to and how many asked for
    /// it (NR-86). A peer that asked may have discarded it.
    pub fn relay_counts(&self, txid: &Txid) -> Option<(usize, usize)> {
        self.relays.get(txid).copied()
    }

    fn note(&mut self, line: String) {
        self.watcher.note(line);
    }

    fn stop(&mut self, line: String) {
        self.note(line.clone());
        if self.halted.is_none() {
            self.halted = Some(line);
        }
    }

    // The queue and the keys.

    /// The marks, from what the queue and the view hold. When they grow the
    /// new scripts are watched and the blocks above the birthday are checked
    /// again for them (OC-07). Called by every step that can change them.
    pub fn refresh_marks(&mut self) {
        let (receive, change) = marks_of(&self.owner, &self.view);
        let dir = self.dir.clone();
        match self
            .view
            .raise_marks(receive, change, |b| write_file(&dir, COINS_FILE, b))
        {
            Ok(false) => {}
            Ok(true) => {
                let new = scripts_for(
                    &self.wallet,
                    self.view.receive_watch(),
                    self.view.change_watch(),
                );
                for script in new.keys() {
                    if !self.scripts.contains_key(script) {
                        self.watcher.port().watch_script(script.clone());
                    }
                }
                self.scripts = new;
                self.watcher.port().rescan_from(self.view.birthday());
                self.note(format!(
                    "new addresses are watched; the blocks above {} are checked again",
                    self.view.birthday()
                ));
            }
            Err(e) => self.stop(format!("the coin view could not be stored: {e}")),
        }
    }

    /// Start again from the birthday: for a view in the deep reorg state.
    pub fn rescan(&mut self) -> Result<(), NodeError> {
        let dir = self.dir.clone();
        self.view
            .reset(|b| write_file(&dir, COINS_FILE, b))
            .map_err(NodeError::Files)?;
        self.watcher.port().rescan_from(self.view.birthday());
        self.halted = None;
        self.note("rescanning from the birthday height".to_string());
        Ok(())
    }

    /// Build the next session for the Key app from the queue and the coins
    /// the view holds. The view must be ready and have checked every block up
    /// to the tip, or nothing is built.
    pub fn build_request(&mut self, fee_rate: u64, now: u64) -> Result<Built, NodeError> {
        if let Some(why) = &self.halted {
            return Err(NodeError::NotReady(why.clone()));
        }
        if !self.view.ready() {
            return Err(NodeError::NotReady(self.view.state().to_string()));
        }
        let tip = self.watcher.chain().tip_height();
        if self.view.checked() < tip {
            return Err(NodeError::NotReady(format!(
                "the coin view has checked blocks up to {} and the chain is at {tip}",
                self.view.checked()
            )));
        }
        let coins = self.view.coins(tip);
        let mut random = [0u8; 96];
        for chunk in random.chunks_mut(8) {
            let word = self.random.next_u64().to_le_bytes();
            let n = chunk.len();
            if let Some(src) = word.get(..n) {
                chunk.copy_from_slice(src);
            }
        }
        let change_index = self.view.change_next();
        let dir = self.dir.clone();
        let built =
            self.owner
                .build_request(&coins, fee_rate, change_index, tip, now, &random, |b| {
                    write_file(&dir, QUEUE_FILE, b)
                })?;
        self.refresh_marks();
        Ok(built)
    }

    /// Take the Key app's response. The queue and key 1's state are stored
    /// together first. Then each transaction that came out is held as
    /// pending, sent, and followed. A step after the store that fails is
    /// noted and tried again by the next restart.
    pub fn take(
        &mut self,
        response: &[u8],
        key1: &mut SignerState,
        key1_account: &Xpriv,
        now: u64,
        persist_key1: impl FnOnce(&[u8]) -> Result<(), String>,
    ) -> Result<Taken, NodeError> {
        let dir = self.dir.clone();
        let taken = self
            .owner
            .take_response(response, key1, key1_account, now, |queue, key| {
                write_file(&dir, QUEUE_FILE, queue)?;
                persist_key1(key)
            })?;
        self.refresh_marks();
        self.reconcile();
        Ok(taken)
    }

    // Sending.

    /// NR-86, NR-91: send a finished transaction, through the second driver
    /// if there is one, else through the main one if the owner turned
    /// that on, else not at all. In the last case the transaction is held as
    /// [`SendState::Waiting`] and the answer says why.
    pub fn broadcast(&mut self, tx: &[u8]) -> Result<Txid, SendRefused> {
        let parsed: Transaction = deserialize(tx).map_err(|_| SendRefused::Malformed)?;
        let txid = parsed.compute_txid();
        match self.send(&txid, tx) {
            Ok(()) => {
                self.states.entry(txid).or_insert(SendState::Sent);
                Ok(txid)
            }
            Err(e) => {
                self.states.entry(txid).or_insert(SendState::Waiting);
                self.note(format!("{txid} is not sent: {e}"));
                Err(e)
            }
        }
    }

    fn send(&mut self, txid: &Txid, tx: &[u8]) -> Result<(), SendRefused> {
        if let Some(sender) = &self.sender {
            sender.port().broadcast(tx.to_vec());
            Ok(())
        } else if self.without_tor {
            self.note(format!(
                "sending without Tor: {txid} goes out on the connections that also watch the wallet"
            ));
            self.watcher.port().broadcast(tx.to_vec());
            Ok(())
        } else {
            Err(SendRefused::NoTor)
        }
    }

    fn unsend(&mut self, txid: &Txid) {
        match &self.sender {
            Some(sender) => sender.port().forget(*txid),
            None => self.watcher.port().forget(*txid),
        }
    }

    fn is_signed(&self, txid: &Txid) -> bool {
        let signed = |s: &Stage| matches!(s, Stage::Signed { txid: t } if t == txid);
        self.owner.refunds().iter().any(|r| signed(&r.stage))
            || self.owner.cash_outs().iter().any(|c| signed(&c.stage))
    }

    /// Send, mark the entries as sent, and record the state.
    fn dispatch(&mut self, txid: Txid, bytes: &[u8]) {
        match self.send(&txid, bytes) {
            Ok(()) => {
                self.states.insert(txid, SendState::Sent);
                if self.is_signed(&txid) {
                    let dir = self.dir.clone();
                    if let Err(e) = self
                        .owner
                        .mark_broadcast(&txid, |b| write_file(&dir, QUEUE_FILE, b))
                    {
                        self.note(format!("{txid} was sent but could not be marked: {e}"));
                    }
                }
            }
            Err(e) => {
                self.states.insert(txid, SendState::Waiting);
                self.note(format!("{txid} is not sent: {e}"));
            }
        }
    }

    fn known_input(&self, op: &OutPoint) -> bool {
        self.view.owned().any(|c| c.outpoint == *op)
            || self.view.pending().any(|p| {
                p.tx.compute_txid() == op.txid && p.own.iter().any(|(v, _, _)| *v == op.vout)
            })
    }

    /// Every transaction in the queue's outbox that is not followed yet is
    /// held, sent and followed (the module docs, "What a restart does").
    fn reconcile(&mut self) {
        let list: Vec<(Txid, Vec<u8>)> = self
            .owner
            .outbox()
            .iter()
            .filter_map(|d| match &d.what {
                Delivery::Transaction { txid, tx, .. } => Some((*txid, tx.clone())),
                _ => None,
            })
            .collect();
        for (txid, bytes) in list {
            if self.states.contains_key(&txid) {
                continue;
            }
            let Ok(tx) = deserialize::<Transaction>(&bytes) else {
                self.note(format!("{txid} in the queue is not a transaction"));
                self.states.insert(txid, SendState::Unresolved);
                continue;
            };
            if self.view.confirmed_height(&txid).is_some() {
                self.states.insert(txid, SendState::Sent);
            } else if self.view.pending().any(|p| p.tx.compute_txid() == txid) {
                self.dispatch(txid, &bytes);
            } else if tx
                .input
                .iter()
                .all(|i| self.known_input(&i.previous_output))
            {
                let dir = self.dir.clone();
                match self
                    .view
                    .add_pending(&tx, &self.scripts, |b| write_file(&dir, COINS_FILE, b))
                {
                    Ok(_) => {
                        self.refresh_marks();
                        self.dispatch(txid, &bytes);
                    }
                    Err(PendingError::Full) => {
                        self.note(format!("{txid} waits: 64 transactions are held already"));
                    }
                    Err(PendingError::Store(e)) => {
                        self.note(format!("{txid} could not be held: {e}"));
                    }
                }
            } else {
                self.note(format!(
                    "{txid} is unresolved: its coins are not in the view and no recent block shows it"
                ));
                self.states.insert(txid, SendState::Unresolved);
            }
        }
        if self.states.len() > MAX_STATES {
            self.states.retain(|_, s| *s != SendState::Done);
        }
    }

    /// A transaction at the refund depth is reported paid and let go; one
    /// whose coins another transaction spent goes back to the queue.
    fn settle(&mut self) {
        let tip = self.watcher.chain().tip_height();
        let depth = self.refund_depth;
        let done: Vec<Txid> = self
            .owner
            .outbox()
            .iter()
            .filter_map(|d| match &d.what {
                Delivery::Transaction { txid, .. } => Some(*txid),
                _ => None,
            })
            .filter(|txid| {
                self.view
                    .confirmed_height(txid)
                    .is_some_and(|h| tip.saturating_sub(h).saturating_add(1) >= depth)
            })
            .collect();
        for txid in done {
            let dir = self.dir.clone();
            match self
                .owner
                .mark_confirmed(&txid, |b| write_file(&dir, QUEUE_FILE, b))
            {
                Ok(()) => {
                    self.unsend(&txid);
                    self.states.insert(txid, SendState::Done);
                    self.relays.remove(&txid);
                    self.note(format!("{txid} is {depth} deep: done"));
                }
                Err(e) => self.note(format!("{txid} could not be marked paid: {e}")),
            }
        }
    }

    fn replaced(&mut self, txid: Txid) {
        let dir = self.dir.clone();
        match self
            .owner
            .mark_replaced(&txid, |b| write_file(&dir, QUEUE_FILE, b))
        {
            Ok(()) => self.note(format!(
                "{txid} was replaced: another transaction spent its coins; its entries are back \
                 in the queue and nothing is signed again by itself"
            )),
            Err(e) => self.note(format!("{txid} was replaced but could not be marked: {e}")),
        }
        self.unsend(&txid);
        self.states.insert(txid, SendState::Replaced);
        self.relays.remove(&txid);
        if let Err(e) = self
            .view
            .drop_pending(&txid, |b| write_file(&dir, COINS_FILE, b))
        {
            self.stop(format!("the coin view could not be stored: {e}"));
        }
    }

    // What arrives.

    /// Take the driver's events for up to `wait` and handle each.
    pub fn pump(&mut self, wait: Duration) {
        let deadline = Instant::now() + wait;
        for _ in 0..MAX_EVENTS_PER_PUMP {
            let left = deadline.saturating_duration_since(Instant::now());
            let Some(event) = self.watcher.next_event(left) else {
                break;
            };
            self.handle(&event);
            self.watcher.apply(&event);
        }
        if let Some(mut sender) = self.sender.take() {
            for _ in 0..MAX_EVENTS_PER_PUMP {
                let Some(event) = sender.next_event(Duration::ZERO) else {
                    break;
                };
                self.handle_relay(&event);
                sender.apply(&event);
            }
            self.sender = Some(sender);
        }
        self.refresh_marks();
        self.settle();
    }

    /// Stop the drivers.
    pub fn shutdown(&mut self) {
        if let Some(sender) = &mut self.sender {
            sender.shutdown();
        }
        self.watcher.shutdown();
    }

    fn handle(&mut self, event: &AppEvent) {
        match event {
            AppEvent::Chain { left, .. } if !left.is_empty() => self.left_the_chain(left),
            AppEvent::Block { hash, bytes } => self.block(hash, bytes),
            AppEvent::Checked(height) => self.checked(*height),
            _ => self.handle_relay(event),
        }
    }

    fn handle_relay(&mut self, event: &AppEvent) {
        match event {
            AppEvent::TxRelayed {
                txid,
                peers_asked,
                peers_fetched,
            } => {
                self.relays.insert(*txid, (*peers_asked, *peers_fetched));
            }
            AppEvent::TxState { txid, state } => {
                let live = matches!(
                    self.states.get(txid),
                    Some(SendState::Sent | SendState::SeenBack { .. } | SendState::NotSeen)
                );
                if live {
                    let mapped = match state {
                        RelayState::Sent => SendState::Sent,
                        RelayState::SeenBack { groups } => SendState::SeenBack { groups: *groups },
                        RelayState::NotSeen => {
                            self.note(format!(
                                "{txid} was not seen back in ten minutes; it was announced to \
                                 every peer again"
                            ));
                            SendState::NotSeen
                        }
                    };
                    self.states.insert(*txid, mapped);
                }
            }
            _ => {}
        }
    }

    fn left_the_chain(&mut self, left: &[bitcoin::BlockHash]) {
        // The watcher has not applied this event yet, so its view still has
        // the heights of the blocks that left.
        let fork = left
            .last()
            .and_then(|h| self.watcher.chain().height_of(h))
            .map_or_else(
                || {
                    let n = u32::try_from(left.len()).unwrap_or(u32::MAX);
                    self.view.checked().saturating_sub(n)
                },
                |h| h.saturating_sub(1),
            );
        let dir = self.dir.clone();
        match self
            .view
            .reorganize(left, fork, |b| write_file(&dir, COINS_FILE, b))
        {
            Ok(effect) => {
                if self.view.state() == ViewState::DeepReorg {
                    self.note("deep reorg: rescan".to_string());
                }
                for txid in effect.reopened {
                    self.note(format!(
                        "{txid} was in a block that left the chain; it is sent again"
                    ));
                    let bytes = self.owner.outbox().iter().find_map(|d| match &d.what {
                        Delivery::Transaction { txid: t, tx, .. } if *t == txid => Some(tx.clone()),
                        _ => None,
                    });
                    if let Some(bytes) = bytes {
                        self.dispatch(txid, &bytes);
                    }
                }
            }
            Err(e) => self.stop(format!("the coin view could not be stored: {e}")),
        }
    }

    fn block(&mut self, hash: &bitcoin::BlockHash, bytes: &[u8]) {
        if self.halted.is_some() {
            return;
        }
        let block = match WellFormedBlock::parse(bytes) {
            Ok(b) if b.hash() == *hash => b,
            Ok(_) => return self.stop(format!("a block did not have the hash {hash} it came as")),
            Err(e) => return self.stop(format!("block {hash} was refused: {e}")),
        };
        let dir = self.dir.clone();
        match self
            .view
            .apply_block(&block, self.watcher.chain(), &self.scripts, |b| {
                write_file(&dir, COINS_FILE, b)
            }) {
            Ok(effect) => {
                for txid in effect.confirmed {
                    self.note(format!("{txid} is in block {hash}"));
                }
                for txid in effect.replaced {
                    self.replaced(txid);
                }
            }
            // A block that is not on the best chain is not the wallet's
            // business: the driver says it joined before it hands it over, so
            // this is a block that has left since.
            Err(ApplyError::Refused(Refusal::NotOnChain)) => {
                self.note(format!(
                    "block {hash} is not on the best chain; passed over"
                ));
            }
            Err(ApplyError::Refused(r)) => self.stop(format!("block {hash} was refused: {r}")),
            Err(e) => self.stop(format!("block {hash} could not be applied: {e}")),
        }
    }

    fn checked(&mut self, height: u32) {
        if self.halted.is_some() {
            return;
        }
        let tip = self.watcher.chain().tip_height();
        let caught_up = matches!(self.view.state(), ViewState::Rescanning { to } if height >= to);
        if height >= tip || caught_up || height.saturating_sub(self.view.checked()) >= CHECKED_EVERY
        {
            let dir = self.dir.clone();
            if let Err(e) = self
                .view
                .set_checked(height, |b| write_file(&dir, COINS_FILE, b))
            {
                self.stop(format!("the coin view could not be stored: {e}"));
            }
        }
    }
}
