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
//! In the node's directory: [`COINS_FILE`] (the coin view, `OBMe`),
//! [`QUEUE_FILE`] (the owner's queue, `OBM4`), [`PAID_LOG_FILE`] (what was
//! paid out, `OBM&`, OC-48), [`SETTLE_OWED_FILE`] (the settlements a block
//! owes the queue, `OBM,`, OC-52) and [`TAKE_JOURNAL_FILE`]
//! (a joint queue/key 1 roll-forward record), written whole through the
//! watcher's atomic write. The queue is written before the coin view takes
//! the new transaction, so a restart finds the transaction in the queue's
//! outbox and picks up from there ([`OwnerNode::start_with`]). A journal left
//! by a failed take blocks queue changes until [`OwnerNode::recover_key1`]
//! has restored both snapshots.
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
//! ## The live watch and the deep check
//!
//! The driver watches at most `MAX_WATCHED_SCRIPTS` scripts (NR-41). When the
//! wallet's scripts would pass the live watch's bound, the oldest quiet
//! receive scripts are set aside (OC-22, OC-23): the driver is told to stop
//! watching them (NR-97). A deep check looks at them every month and when the
//! owner asks (OC-24 to OC-27, [`OwnerNode::request_deep_check`]); it runs on
//! a second driver of its own, in `<dir>/deepcheck`, apart from the one that
//! watches the wallet, so that the live watch and the coin view's state are
//! not disturbed while it runs. The code is in `crate::deep`.
//!
//! ## Stops
//!
//! A block that cannot be applied, or a view that cannot be stored, halts
//! the node's view of the chain (see [`OwnerNode::halted`]): the view stays
//! where it was and no session is built until the node is restarted. It is
//! never silent: a note says what failed.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use bitcoin::bip32::Xpriv;
use bitcoin::bip32::Xpub;
use bitcoin::consensus::deserialize;
use bitcoin::hashes::Hash;
use bitcoin::{BlockHash, Network, OutPoint, ScriptBuf, Transaction, Txid};
use openbmt_core::batch::BatchPolicy;
use openbmt_core::identity::{DeviceId, Identity};
use openbmt_core::pinned_wallet::PinnedWallet;
use openbmt_core::wallet::Keychain;
use openbmt_core::wallet_single::ImportedWallet;
use openbmt_counter_node::{ChainView, DriverPort, MAX_EVENTS_PER_PUMP};
use openbmt_net_io::{
    AppEvent, DirLock, Driver, DriverConfig, FileStorage, Handle, OsRandomness, Randomness,
    RelayState, SystemClock, MAX_WATCHED_SCRIPTS,
};
use openbmt_owner::own_book::AllocationBook;
use openbmt_owner::own_spend::{OwnBuilt, OwnFile, OwnTaken};
use openbmt_owner::starter::{held_sats, IncomingCoin};
use openbmt_owner::{Built, Delivery, Held, IssuedRange, Options, Owner, OwnerError, Stage, Taken};
use openbmt_signer::SignerState;
use openbmt_watch::{FeeHistory, HeaderChain, Refusal, WellFormedBlock};

use crate::coins::{
    ApplyError, CoinView, PendingError, ReorgEffect, ScriptSet, ViewState, GAP, INDEX_LIMIT,
    UNDO_BLOCKS,
};
use crate::deep::{DeepRun, DeepSpawn};
use crate::live::{plan_release, WatchLimits, REBALANCE_SECS};
use crate::paid_log::{value_at, PaidEntry, PaidLog, PaidValue, PaidWhat, PAID_LOG_FILE};
use crate::receipts::read_fee_history;
use crate::requests::Requests;
use crate::settle_owed::{Owed, OwedKind, SettleOwed, SETTLE_OWED_FILE};
use crate::take_journal::{TakeJournal, MAX_JOURNAL_BYTES};
use crate::watcher::{write_file, OwnerConfig, OwnerStartError, OwnerWatcher};

static TAKE_JOURNAL_LOCK: Mutex<()> = Mutex::new(());

/// The coin view's file.
pub const COINS_FILE: &str = "coins.bin";
/// The owner's queue file.
pub const QUEUE_FILE: &str = "owner-queue.bin";
/// A committed response awaiting roll-forward into the caller's key store.
pub const TAKE_JOURNAL_FILE: &str = "take-journal.bin";
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
    /// NR-97: stop watching these scripts.
    fn unwatch_scripts(&self, scripts: Vec<ScriptBuf>);
    /// NR-90: check the blocks above this height again.
    fn rescan_from(&self, height: u32);
    /// Whether to fetch the transactions that peers announce.
    fn set_fetch_transactions(&self, on: bool);
    /// NR-47: download this block of the best chain, whatever the filters
    /// say. It comes back as [`AppEvent::FeeBlock`].
    fn fetch_block(&self, hash: BlockHash);
}

impl OwnerPort for Handle {
    fn broadcast(&self, tx: Vec<u8>) {
        Handle::broadcast(self, tx);
    }

    fn forget(&self, txid: Txid) {
        Handle::forget(self, txid);
    }

    fn unwatch_scripts(&self, scripts: Vec<ScriptBuf>) {
        Handle::unwatch_scripts(self, scripts);
    }

    fn rescan_from(&self, height: u32) {
        Handle::rescan_from(self, height);
    }

    fn set_fetch_transactions(&self, on: bool) {
        Handle::set_fetch_transactions(self, on);
    }

    fn fetch_block(&self, hash: BlockHash) {
        Handle::fetch_block(self, hash);
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
    /// The shop's own node gave it back when asked, so it is in that node's
    /// pool of unconfirmed transactions (NR-112). Peers that were not asked
    /// may also have announced it, from this many network groups (0 when
    /// none have). This is not a confirmation.
    HeldByNode {
        /// Distinct groups that announced it back, 0 for none.
        groups: usize,
    },
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
            Self::HeldByNode { groups: 0 } => write!(f, "held by the shop's node"),
            Self::HeldByNode { groups } => {
                write!(
                    f,
                    "held by the shop's node, seen back from {groups} network groups"
                )
            }
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
    Owner(OwnerError<String>),
    /// The wallet's receive and change ranges exceed the driver's watch limit.
    WatchLimit {
        /// Scripts required by the receive and change ranges together.
        needed: u64,
        /// Scripts the network driver can follow at once.
        limit: usize,
    },
    /// A committed take must be rolled forward into the owner's key 1 state.
    KeyRecoveryRequired,
}

impl fmt::Display for NodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Start(e) => write!(f, "{e}"),
            Self::Files(e) => write!(f, "{e}"),
            Self::Config(e) => write!(f, "{e}"),
            Self::NotReady(e) => write!(f, "not ready: {e}"),
            Self::Owner(e) => write!(f, "{e}"),
            Self::WatchLimit { needed, limit } => write!(
                f,
                "the wallet needs {needed} watched scripts; the driver limit is {limit}"
            ),
            Self::KeyRecoveryRequired => write!(f, "key state needs recovery"),
        }
    }
}

fn remove_take_journal(dir: &std::path::Path) -> Result<(), String> {
    match fs::remove_file(dir.join(TAKE_JOURNAL_FILE)) {
        Ok(()) => fs::File::open(dir)
            .and_then(|directory| directory.sync_all())
            .map_err(|error| format!("{TAKE_JOURNAL_FILE}: {error}")),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("{TAKE_JOURNAL_FILE}: {error}")),
    }
}

/// Store a queue snapshot under the take-journal serialization lock.
pub(crate) fn write_queue_file(dir: &std::path::Path, bytes: &[u8]) -> Result<(), String> {
    let _commit = TAKE_JOURNAL_LOCK
        .lock()
        .map_err(|_| "take journal lock is poisoned".to_string())?;
    match fs::symlink_metadata(dir.join(TAKE_JOURNAL_FILE)) {
        Ok(_) => return Err("a committed response needs key 1 recovery".to_string()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.to_string()),
    }
    write_file(dir, QUEUE_FILE, bytes)
}

pub(crate) fn read_take_journal(dir: &std::path::Path) -> Result<Option<TakeJournal>, String> {
    let path = dir.join(TAKE_JOURNAL_FILE);
    let file = match fs::File::open(&path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("{TAKE_JOURNAL_FILE}: {error}")),
    };
    let size = file
        .metadata()
        .map_err(|error| format!("{TAKE_JOURNAL_FILE}: {error}"))?
        .len();
    if size > u64::try_from(MAX_JOURNAL_BYTES).unwrap_or(u64::MAX) {
        return Err(format!("{TAKE_JOURNAL_FILE}: file is too large"));
    }
    let mut bytes = Vec::new();
    file.take(u64::try_from(MAX_JOURNAL_BYTES + 1).unwrap_or(u64::MAX))
        .read_to_end(&mut bytes)
        .map_err(|error| format!("{TAKE_JOURNAL_FILE}: {error}"))?;
    if bytes.len() > MAX_JOURNAL_BYTES {
        return Err(format!("{TAKE_JOURNAL_FILE}: file is too large"));
    }
    TakeJournal::from_bytes(&bytes)
        .map(Some)
        .map_err(|error| format!("{TAKE_JOURNAL_FILE} does not read back: {error}"))
}

impl std::error::Error for NodeError {}

impl From<OwnerError<String>> for NodeError {
    fn from(e: OwnerError<String>) -> Self {
        Self::Owner(e)
    }
}

/// What the owner's queue is made of: the keys' public side, the shop's
/// choices and where randomness comes from.
pub struct OwnerParts {
    /// The owner root's secret, the one the watcher's identity is made from.
    pub owner_secret: [u8; 32],
    /// The wallet, the 2-of-3 wallet of a shop with a Key app or the single
    /// key of a shop with its own wallet.
    pub wallet: ImportedWallet,
    /// The Key app's identity. A shop with its own wallet has none: the
    /// caller gives a key nobody holds (`own_approver`).
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
    /// OC-22 to OC-24: the live watch's bound, the quiet time and the time
    /// between deep checks. The defaults are the constants of
    /// [`crate::live`]; a check against a real node shrinks them.
    pub limits: WatchLimits,
    /// OC-10, OA-107: for a shop with its own wallet, the allocation book,
    /// whose ranges are the issued ranges the live watch and the deep check
    /// follow (OC-22 to OC-24). `None` for a shop with a Key app, whose
    /// issued ranges are those of its queue.
    pub own_book: Option<AllocationBook>,
    /// SS-43: the wallets the shop pinned before a re-pin, each watched up
    /// to the index its book had reached. Empty for any other shop.
    pub former: Vec<FormerParts>,
}

/// SS-43: a wallet pinned before the re-pin, which the node keeps watching
/// so that a payment that arrives later is seen.
#[derive(Debug, Clone)]
pub struct FormerParts {
    /// The wallet.
    pub wallet: PinnedWallet,
    /// One past the highest receive index given out for it.
    pub end: u32,
    /// The file its coin view is kept in, under the node's folder.
    pub file: String,
}

/// SS-43: what a former wallet holds that the node saw.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FormerHeld {
    /// The checksum of the former wallet's pinned frame.
    pub wallet: [u8; 32],
    /// What its coins add up to, in sats.
    pub sats: u64,
    /// How many coins it holds.
    pub coins: u32,
}

/// A former wallet as the node follows it: its own coin view, apart from the
/// wallet in force, fed by the same blocks.
pub(crate) struct FormerWatch {
    wallet: PinnedWallet,
    file: String,
    pub(crate) view: CoinView,
    pub(crate) scripts: ScriptSet,
}

impl FormerWatch {
    fn open(
        dir: &Path,
        parts: FormerParts,
        birthday: u32,
        chain: &ChainView,
    ) -> Result<Self, NodeError> {
        let mut view = match fs::read(dir.join(&parts.file)) {
            Ok(bytes) => CoinView::from_bytes(&bytes)
                .map_err(|e| NodeError::Files(format!("{} does not read back: {e}", parts.file)))?,
            Err(e) if e.kind() == io::ErrorKind::NotFound => CoinView::new(birthday),
            Err(e) => return Err(NodeError::Files(format!("{}: {e}", parts.file))),
        };
        let file = parts.file.clone();
        // OC-53: what left the chain while the node was stopped is undone
        // before the view is used.
        if let Some((left, fork)) = view.left_while_stopped(|h| chain.hash_at(h)) {
            view.reorganize(&left, fork, |b| write_file(dir, &file, b))
                .map_err(|e| NodeError::Files(format!("{file}: {e}")))?;
        }
        let change = view.change_next();
        let through = view.checked();
        view.watch_plan(parts.end, change, through, &[], 0, |b| {
            write_file(dir, &file, b)
        })
        .map_err(NodeError::Files)?;
        let scripts = former_scripts(parts.wallet.wallet(), &view);
        Ok(Self {
            wallet: parts.wallet,
            file: parts.file,
            view,
            scripts,
        })
    }

    pub(crate) fn has_script(&self, script: &ScriptBuf) -> bool {
        self.scripts.contains_key(script)
    }

    /// The wallet this watch follows.
    pub(crate) fn pinned(&self) -> &PinnedWallet {
        &self.wallet
    }

    fn knows(&self, op: &OutPoint) -> bool {
        self.view.owned().any(|c| c.outpoint == *op)
            || self.view.pending().any(|p| {
                p.tx.compute_txid() == op.txid && p.own.iter().any(|(v, _, _)| *v == op.vout)
            })
    }
}

/// Every script of a former wallet the view watches: no script of it is set
/// aside, since the wallet is small and no longer grows.
fn former_scripts(wallet: &ImportedWallet, view: &CoinView) -> ScriptSet {
    let mut set = ScriptSet::new();
    for i in 0..view.receive_watch() {
        if let Ok(script) = wallet.receive_script(i) {
            set.insert(script, (Keychain::Receive, i));
        }
    }
    for i in 0..view.change_watch() {
        if let Ok(script) = wallet.change_script(i) {
            set.insert(script, (Keychain::Change, i));
        }
    }
    set
}

impl OwnerParts {
    /// Parts with the system's randomness and the default refund depth, 6.
    pub fn new(
        owner_secret: [u8; 32],
        wallet: impl Into<ImportedWallet>,
        approver: DeviceId,
        policy: BatchPolicy,
    ) -> Result<Self, NodeError> {
        let random = OsRandomness::new().map_err(|e| NodeError::Config(e.to_string()))?;
        Ok(Self {
            owner_secret,
            wallet: wallet.into(),
            approver,
            policy,
            options: Options::default(),
            refund_depth: 6,
            random: Box::new(random),
            limits: WatchLimits::default(),
            own_book: None,
            former: Vec::new(),
        })
    }
}

/// NR-41, OC-22: the scripts the live watch would hold must fit the
/// driver's limit: the receive scripts watched (the marks plus the gap)
/// less the `aside` that are set aside, and the change scripts watched.
fn ensure_marks_fit_watch_budget(receive: u32, change: u32, aside: u64) -> Result<(), NodeError> {
    // Match CoinView's receive_watch/change_watch ranges before marks are
    // persisted.
    let receive = receive.saturating_add(GAP).min(INDEX_LIMIT);
    let change = change.saturating_add(GAP).min(INDEX_LIMIT);
    let needed = u64::from(receive)
        .saturating_sub(aside)
        .saturating_add(u64::from(change));
    if needed > u64::try_from(MAX_WATCHED_SCRIPTS).unwrap_or(u64::MAX) {
        return Err(NodeError::WatchLimit {
            needed,
            limit: MAX_WATCHED_SCRIPTS,
        });
    }
    Ok(())
}

/// OC-22: the wallet scripts the live watch holds: the receive scripts up to
/// the marks plus the gap that are not set aside, and the change scripts.
fn live_scripts(wallet: &ImportedWallet, view: &CoinView) -> Result<ScriptSet, NodeError> {
    ensure_marks_fit_watch_budget(
        view.receive_next(),
        view.change_next(),
        view.released().count_below(view.receive_watch()),
    )?;
    let mut set = ScriptSet::new();
    for i in 0..view.receive_watch() {
        if view.released().contains(i) {
            continue;
        }
        if let Ok(script) = wallet.receive_script(i) {
            set.insert(script, (Keychain::Receive, i));
        }
    }
    for i in 0..view.change_watch() {
        if let Ok(script) = wallet.change_script(i) {
            set.insert(script, (Keychain::Change, i));
        }
    }
    Ok(set)
}

/// The receive indices that hold an unspent coin or a coin of a held
/// transaction: they stay in the live watch, because a spend must be seen.
fn held_receive(view: &CoinView) -> std::collections::BTreeSet<u32> {
    let coins = view
        .owned()
        .filter(|c| c.chain == Keychain::Receive)
        .map(|c| c.index);
    let pending = view.pending().flat_map(|p| {
        p.own
            .iter()
            .filter(|(_, chain, _)| *chain == Keychain::Receive)
            .map(|(_, _, index)| *index)
    });
    coins.chain(pending).collect()
}

/// OC-22, OC-23: the receive indices to set aside for these marks, none
/// when `allow` is false. `keep` are the indices of the desk's requests,
/// which are never set aside (RQ-04). An error when the live watch would still pass the
/// driver's limit.
fn release_plan(
    issuing: &Issuing<'_>,
    view: &CoinView,
    keep: &std::collections::BTreeSet<u32>,
    marks: (u32, u32),
    now: u64,
    limits: &WatchLimits,
    allow: bool,
) -> Result<Vec<u32>, NodeError> {
    let (receive, change) = marks;
    let receive_watch = receive.saturating_add(GAP).min(INDEX_LIMIT);
    let change_watch = change.saturating_add(GAP).min(INDEX_LIMIT);
    let aside = view.released().count_below(receive_watch);
    let live = u64::from(receive_watch)
        .saturating_sub(aside)
        .saturating_add(u64::from(change_watch));
    let bound = u64::try_from(limits.live).unwrap_or(u64::MAX);
    let mut release = Vec::new();
    if allow && live > bound {
        // RQ-04: the address of a request stays in the live watch for good.
        let mut held = held_receive(view);
        held.extend(keep.iter().copied());
        release = plan_release(
            issuing.ranges,
            view.released(),
            &held,
            now,
            limits.quiet_secs,
            live - bound,
        );
    }
    let added = u64::try_from(release.len()).unwrap_or(u64::MAX);
    ensure_marks_fit_watch_budget(receive, change, aside.saturating_add(added))?;
    Ok(release)
}

/// What the node follows of the addresses given out (OC-10): the marks and
/// the issued ranges, from the owner's queue for a shop with a Key app and
/// from the allocation book for a shop with its own wallet.
struct Issuing<'a> {
    receive: u32,
    change: u32,
    ranges: &'a [IssuedRange],
}

/// The issued ranges of a shop with its own wallet, from its book.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OwnIssued {
    ranges: Vec<IssuedRange>,
    /// The end of the highest range: the receive mark.
    next: u32,
}

impl OwnIssued {
    fn of(book: &AllocationBook) -> Self {
        Self {
            ranges: book.issued(),
            next: book.next_start(),
        }
    }
}

fn issuing<'a>(owner: &'a Owner, own: Option<&'a OwnIssued>) -> Issuing<'a> {
    match own {
        Some(own) => Issuing {
            receive: own.next,
            change: owner.change_mark(),
            ranges: &own.ranges,
        },
        None => Issuing {
            receive: owner.receive_mark(),
            change: owner.change_mark(),
            ranges: owner.issued_ranges(),
        },
    }
}

/// The marks that follow from what the queue (or the book) and the view
/// hold.
fn marks_of(issuing: &Issuing<'_>, view: &CoinView) -> (u32, u32) {
    let mut receive = issuing.receive.max(view.receive_next());
    let mut change = issuing.change.max(view.change_next());
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
    pub(crate) watcher: OwnerWatcher<P>,
    /// The second driver, behind the proxy.
    sender: Option<OwnerWatcher<P>>,
    without_tor: bool,
    pub(crate) owner: Owner,
    pub(crate) view: CoinView,
    pub(crate) scripts: ScriptSet,
    pub(crate) wallet: ImportedWallet,
    /// OC-10: the issued ranges of a shop with its own wallet.
    pub(crate) own: Option<OwnIssued>,
    /// SS-43: the wallets pinned before the re-pin.
    pub(crate) former: Vec<FormerWatch>,
    /// SS-41: the shop's wallet was re-pinned after this node started, so
    /// what the view holds is the old wallet's: nothing is read from it as
    /// the wallet in force until the node starts again.
    repinned: bool,
    /// RQ-04: with `repinned`, the wallet pinned now, which new requests are
    /// made from.
    pub(crate) pinned_now: Option<ImportedWallet>,
    pub(crate) dir: PathBuf,
    refund_depth: u32,
    random: Box<dyn Randomness>,
    states: BTreeMap<Txid, SendState>,
    relays: BTreeMap<Txid, (usize, usize)>,
    pub(crate) halted: Option<String>,
    /// A watch-range limit is returned directly to queue/build callers too.
    watch_limit: Option<NodeError>,
    /// A runtime rescan command is queued, but its ordered start marker has
    /// not yet reached this node. Ignore old `Checked` events until then.
    pub(crate) awaiting_rescan: u32,
    take_journal: Option<TakeJournal>,
    /// OC-48: what was paid out.
    paid_log: PaidLog,
    /// OC-52: the settlements a block owes the queue.
    owed: SettleOwed,
    /// RQ-30: the owner's payment requests and the arrivals that wait.
    pub(crate) requests: Requests,
    pub(crate) owner_secret: [u8; 32],
    approver: DeviceId,
    policy: BatchPolicy,
    /// FE-20: the fee samples of the blocks this node has checked.
    pub(crate) fees: FeeHistory,
    /// FE-20: the block asked for by hash to take a sample from, with its
    /// height, until it arrives, leaves the chain or is given up.
    pub(crate) fee_request: Option<(BlockHash, u32)>,
    /// OC-22 to OC-24: the live watch's numbers.
    pub(crate) limits: WatchLimits,
    /// The main driver's settings, for the deep check's driver.
    pub(crate) driver_config: DriverConfig,
    /// Starts the deep check's driver (OC-24).
    pub(crate) deep_spawn: DeepSpawn<P>,
    /// The deep check in progress, if any (OC-24).
    pub(crate) deep_run: Option<DeepRun<P>>,
    /// The next time the node looks again for scripts to set aside when none
    /// could be set aside before.
    next_rebalance: u64,
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
        mut spawn: impl FnMut(DriverConfig, PathBuf, Vec<ScriptBuf>) -> Result<P, String> + 'static,
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
            limits,
            own_book,
            former,
        } = parts;
        limits.check().map_err(NodeError::Config)?;
        let now = clock.now();
        let driver_config = driver.clone();
        if refund_depth == 0 || refund_depth > UNDO_BLOCKS {
            return Err(NodeError::Config(format!(
                "the refund depth must be from 1 to {UNDO_BLOCKS}, not {refund_depth}"
            )));
        }
        // NR-79: own the node directory before reading or changing the
        // journal, queue, or coin view. The primary watcher takes ownership
        // of this same lock once it starts.
        let lock = DirLock::acquire(&dir)
            .map_err(|e| NodeError::Start(OwnerStartError::Locked(e.to_string())))?;
        fs::create_dir_all(&dir).map_err(|e| NodeError::Files(e.to_string()))?;

        // OC-53: the header chain the node holds is read before the coin view
        // is used.
        let (chain, header_notes) =
            crate::watcher::load_chain(&dir, driver.network).map_err(NodeError::Start)?;
        let mut view = match fs::read(dir.join(COINS_FILE)) {
            Ok(bytes) => CoinView::from_bytes(&bytes)
                .map_err(|e| NodeError::Files(format!("{COINS_FILE} does not read back: {e}")))?,
            Err(e) if e.kind() == io::ErrorKind::NotFound => CoinView::new(birthday),
            Err(e) => return Err(NodeError::Files(format!("{COINS_FILE}: {e}"))),
        };
        // What left the chain while the node was stopped is undone as a
        // reorganization it saw is; the rest follows once the node exists.
        let mut stopped_reorg = None;
        if let Some((left, fork)) = view.left_while_stopped(|h| chain.hash_at(h)) {
            let effect = view
                .reorganize(&left, fork, |b| write_file(&dir, COINS_FILE, b))
                .map_err(|e| NodeError::Files(format!("{COINS_FILE}: {e}")))?;
            stopped_reorg = Some((left, fork, effect));
        }
        // OC-28: a log that is nearly full is pruned before anything else
        // reads it; the position the view reflects stands for the tip.
        let position = view.checked();
        view.prune_receipts(position, |b| write_file(&dir, COINS_FILE, b))
            .map_err(|e| NodeError::Files(format!("{COINS_FILE}: {e}")))?;
        // FE-20: a fee history that does not read back is a note and an
        // empty history, never a stop.
        let (fees, fee_note) = read_fee_history(&dir);
        let take_journal = read_take_journal(&dir).map_err(NodeError::Files)?;
        let paid_log = match fs::read(dir.join(PAID_LOG_FILE)) {
            Ok(bytes) => PaidLog::from_bytes(&bytes).map_err(|e| {
                NodeError::Files(format!("{PAID_LOG_FILE} does not read back: {e}"))
            })?,
            Err(e) if e.kind() == io::ErrorKind::NotFound => PaidLog::new(),
            Err(e) => return Err(NodeError::Files(format!("{PAID_LOG_FILE}: {e}"))),
        };
        let owed = match fs::read(dir.join(SETTLE_OWED_FILE)) {
            Ok(bytes) => SettleOwed::from_bytes(&bytes).map_err(|e| {
                NodeError::Files(format!("{SETTLE_OWED_FILE} does not read back: {e}"))
            })?,
            Err(e) if e.kind() == io::ErrorKind::NotFound => SettleOwed::new(),
            Err(e) => return Err(NodeError::Files(format!("{SETTLE_OWED_FILE}: {e}"))),
        };
        let queue_bytes = if let Some(journal) = &take_journal {
            Ok(journal.queue.clone())
        } else {
            fs::read(dir.join(QUEUE_FILE))
        };
        let owner = match queue_bytes {
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
        // OC-22: a script set aside that holds a coin (a restart between the
        // deep check's find and its return to the live watch) goes back.
        let stuck: Vec<u32> = held_receive(&view)
            .into_iter()
            .filter(|i| view.released().contains(*i))
            .collect();
        if !stuck.is_empty() {
            let (from, checked) = (view.deep().from, view.checked());
            view.return_to_live(&stuck, from, checked, |b| write_file(&dir, COINS_FILE, b))
                .map_err(NodeError::Files)?;
        }
        let own = own_book.as_ref().map(OwnIssued::of);
        // RQ-01, RQ-31: the request book of a shop with its own wallet.
        let requests = Requests::open(
            &dir,
            own.is_some()
                .then(|| PinnedWallet::new(wallet.clone()).checksum32()),
        );
        let kept = requests.indexes();
        let issued = issuing(&owner, own.as_ref());
        let (receive, change) = marks_of(&issued, &view);
        let release = release_plan(&issued, &view, &kept, (receive, change), now, &limits, true)?;
        let old_checked = view.checked();
        view.watch_plan(
            receive,
            change,
            old_checked,
            &release,
            now.saturating_add(limits.deep_secs).max(2),
            |b| write_file(&dir, COINS_FILE, b),
        )
        .map_err(NodeError::Files)?;
        let scripts = live_scripts(&wallet, &view)?;
        let mut former_watches = Vec::with_capacity(former.len());
        for f in former {
            former_watches.push(FormerWatch::open(&dir, f, birthday, &chain)?);
        }
        // The blocks are read again from the lowest position of any view: a
        // former wallet whose marks grew has its own to catch up.
        let watch_from = former_watches
            .iter()
            .map(|f| f.view.checked())
            .fold(view.checked(), u32::min);

        let main_config = OwnerConfig {
            dir: dir.clone(),
            identity,
            watch_key: false,
            driver: driver.clone(),
            clock,
            health,
            watch_from: Some(watch_from),
            birthday,
            broadcast_proxy,
            broadcast_without_tor,
        };
        let initial: Vec<ScriptBuf> = scripts
            .keys()
            .chain(former_watches.iter().flat_map(|f| f.scripts.keys()))
            .cloned()
            .collect();
        let watcher =
            OwnerWatcher::start_with_chain(main_config, lock, chain, header_notes, |d, p| {
                spawn(d, p, initial)
            })
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
            own,
            former: former_watches,
            repinned: false,
            pinned_now: None,
            dir,
            refund_depth,
            random,
            states: BTreeMap::new(),
            relays: BTreeMap::new(),
            halted: None,
            watch_limit: None,
            awaiting_rescan: 0,
            take_journal,
            paid_log,
            owed,
            requests,
            owner_secret,
            approver,
            policy,
            fees,
            fee_request: None,
            limits,
            driver_config,
            deep_spawn: Box::new(spawn),
            deep_run: None,
            next_rebalance: now.saturating_add(REBALANCE_SECS),
        };
        if let Some(line) = fee_note {
            node.note(line);
        }
        node.prune_fee_samples();
        if let Some((left, fork, effect)) = stopped_reorg {
            node.note(format!(
                "the chain changed while the node was stopped: {} block(s) left",
                left.len()
            ));
            node.forget_owed_blocks(&left);
            node.forget_fee_samples_from(fork.saturating_add(1), &left);
            // RQ-37, OC-53: the requests undo the same blocks.
            node.requests_reorg(fork);
            node.after_reorg(effect);
        }
        if node.take_journal.is_none() {
            node.reconcile();
        } else {
            node.note(
                "a committed response needs key 1 recovery before queue operations".to_string(),
            );
        }
        node.settle_owed();
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

    /// The owner's identity, made again from the secret the node holds, for
    /// a caller that opens another file of the shop (the shop's state of a
    /// shop with its own wallet).
    pub fn owner_identity(&self) -> Result<Identity, NodeError> {
        Identity::from_secret_bytes(&self.owner_secret)
            .map_err(|e| NodeError::Config(format!("the owner key: {e}")))
    }

    /// The refund depth the node was started with.
    pub fn refund_depth(&self) -> u32 {
        self.refund_depth
    }

    /// OC-14, OC-15: apply the refund requests that arrived on the link to
    /// the owner's queue, with the sales' facts taken from this node's own
    /// coin view at its own tip, never from the counters' records. A request
    /// whose facts must wait for the view is tried again on the next call.
    /// Refused while a committed take needs key 1 recovery.
    pub fn run_refund_intake<R: openbmt_courier::Post>(
        &mut self,
        intake: &mut crate::refunds::RefundIntake,
        link: &crate::link::OwnerLink<R>,
    ) -> Result<Vec<(DeviceId, crate::refunds::Applied)>, NodeError> {
        self.ready_for_queue_write()?;
        let evidence = crate::refunds::Evidence {
            view: &self.view,
            tip: self.watcher.chain().tip_height(),
            refund_depth: self.refund_depth,
        };
        let write = self.queue_writer();
        let mut persist = |bytes: &[u8]| write(bytes);
        Ok(intake.run(&mut self.owner, link, &evidence, &mut persist))
    }

    /// Whether a committed take still needs to be copied into the owner's
    /// key 1 state store. Call [`OwnerNode::recover_key1`] before queue changes or another
    /// response is taken.
    pub fn take_recovery_pending(&self) -> bool {
        self.take_journal.is_some()
    }

    pub(crate) fn ready_for_queue_write(&self) -> Result<(), NodeError> {
        if let Some(error) = &self.watch_limit {
            Err(error.clone())
        } else if self.take_recovery_pending() {
            Err(NodeError::KeyRecoveryRequired)
        } else {
            Ok(())
        }
    }

    /// The owner's queue, to change. A change is stored through
    /// [`OwnerNode::queue_writer`]; call [`OwnerNode::refresh_marks`]
    /// after one that queues a batch.
    pub fn owner_mut(&mut self) -> Result<&mut Owner, NodeError> {
        self.ready_for_queue_write()?;
        Ok(&mut self.owner)
    }

    /// A function that stores the queue where the node keeps it. It refuses
    /// a stale queue snapshot and refuses writes while a take journal awaits
    /// key 1 recovery.
    pub fn queue_writer(&self) -> impl Fn(&[u8]) -> Result<(), String> + 'static {
        let dir = self.dir.clone();
        let journal_path = dir.join(TAKE_JOURNAL_FILE);
        let was_pending = self.take_recovery_pending();
        let expected = match fs::read(dir.join(QUEUE_FILE)) {
            Ok(bytes) => Ok(bytes),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(error) => Err(error.to_string()),
        };
        let expected = Arc::new(Mutex::new(expected));
        move |bytes| {
            if was_pending {
                return Err("a committed response needs key 1 recovery".to_string());
            }
            let _commit = TAKE_JOURNAL_LOCK
                .lock()
                .map_err(|_| "take journal lock is poisoned".to_string())?;
            match fs::symlink_metadata(&journal_path) {
                Ok(_) => return Err("a committed response needs key 1 recovery".to_string()),
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.to_string()),
            }
            let mut prior = expected
                .lock()
                .map_err(|_| "queue writer lock is poisoned".to_string())?;
            let expected_bytes = prior.as_ref().map_err(|error| error.clone())?;
            let current = match fs::read(dir.join(QUEUE_FILE)) {
                Ok(bytes) => bytes,
                Err(error) if error.kind() == io::ErrorKind::NotFound => Vec::new(),
                Err(error) => return Err(error.to_string()),
            };
            if current != *expected_bytes {
                return Err("owner queue changed since this writer was created".to_string());
            }
            write_file(&dir, QUEUE_FILE, bytes)?;
            *prior = Ok(bytes.to_vec());
            Ok(())
        }
    }

    /// OC-48: what the owner paid out, oldest first: every refund and
    /// cash-out whose transaction was sent, kept after the queue forgets it.
    pub fn paid_log(&self) -> &[PaidEntry] {
        self.paid_log.entries()
    }

    /// OC-48 to OC-50: store the log entries for the refunds and the
    /// cash-out the transaction `txid` of the outbox pays, unless the log
    /// holds them already. The new log is built, stored, and swapped in only
    /// when the store succeeds.
    fn log_paid(&mut self, txid: &Txid) -> Result<(), String> {
        let Some(tx_bytes) = self.owner.outbox().iter().find_map(|d| match &d.what {
            Delivery::Transaction { txid: t, tx, .. } if t == txid => Some(tx.clone()),
            _ => None,
        }) else {
            return Ok(());
        };
        let Some((refunds, cash_out)) = self.owner.outbox().iter().find_map(|d| match &d.what {
            Delivery::Transaction {
                txid: t,
                refunds,
                cash_out,
                ..
            } if t == txid => Some((refunds.clone(), *cash_out)),
            _ => None,
        }) else {
            return Ok(());
        };
        let tx: Transaction = deserialize(&tx_bytes).map_err(|e| e.to_string())?;
        self.log_paid_entries(txid, Some(&tx), refunds, cash_out)
    }

    /// AS-89: [`OwnerNode::log_paid`] for the entries of a transaction that
    /// was signed elsewhere. The node never held its bytes: `tx` is the
    /// transaction as the block holds it (OC-54), and without it each amount
    /// is the entry's own.
    fn log_paid_elsewhere(&mut self, txid: &Txid, tx: Option<&Transaction>) -> Result<(), String> {
        let Some((refunds, cash_out)) = self
            .owner
            .signed_elsewhere()
            .iter()
            .find(|e| e.txid == *txid)
            .map(|e| (e.refunds.clone(), e.cash_out))
        else {
            return Ok(());
        };
        self.log_paid_entries(txid, tx, refunds, cash_out)
    }

    fn log_paid_entries(
        &mut self,
        txid: &Txid,
        tx: Option<&Transaction>,
        refunds: Vec<u32>,
        cash_out: Option<u32>,
    ) -> Result<(), String> {
        let at = self.watcher.now();
        let outputs: &[bitcoin::TxOut] = tx.map_or(&[], |t| t.output.as_slice());
        let mut used = vec![false; outputs.len()];
        let mut paid = |script: &ScriptBuf| -> Option<u64> {
            let found = outputs
                .iter()
                .zip(used.iter_mut())
                .find(|(o, u)| !**u && o.script_pubkey == *script);
            found.map(|(o, u)| {
                *u = true;
                o.value.to_sat()
            })
        };
        let mut unknown_cash_out = false;
        let mut added = Vec::new();
        for id in refunds {
            let Some(r) = self.owner.refunds().iter().find(|r| r.id == id) else {
                continue;
            };
            let sats = paid(&r.script).unwrap_or(r.sats);
            let value = match (r.shop_value, r.books.and_then(|b| b.last_quote)) {
                (Some(v), _) => Some(PaidValue {
                    currency: v.currency,
                    minor: v.minor,
                }),
                (None, Some(q)) => Some(PaidValue {
                    currency: q.currency,
                    minor: value_at(sats, q.minor_per_btc),
                }),
                (None, None) => None,
            };
            added.push(PaidEntry {
                what: PaidWhat::Refund {
                    sale: r.sale,
                    request: r.books.map(|b| b.request),
                },
                number: id,
                txid: *txid,
                sats,
                at,
                value,
            });
        }
        if let Some(id) = cash_out {
            if let Some(c) = self.owner.cash_outs().iter().find(|c| c.id == id) {
                // OC-49: a cash-out of everything is logged at what its
                // transaction paid; without that figure it is not logged at
                // all, never at zero.
                let found = match c.amount {
                    openbmt_owner::CashAmount::Sats(n) => Some(paid(&c.script).unwrap_or(n)),
                    openbmt_owner::CashAmount::All => paid(&c.script),
                };
                match found {
                    Some(sats) => added.push(PaidEntry {
                        what: PaidWhat::CashOut,
                        number: id,
                        txid: *txid,
                        sats,
                        at,
                        value: c.price.map(|p| PaidValue {
                            currency: p.currency,
                            minor: value_at(sats, p.minor_per_btc),
                        }),
                    }),
                    None => unknown_cash_out = true,
                }
            }
        }
        if unknown_cash_out {
            self.note(format!(
                "{txid} paid the cash-out of everything but its amount is not known; it is not logged"
            ));
        }
        match self.paid_log.with_added(&added) {
            Ok(None) => Ok(()),
            Ok(Some(next)) => {
                write_file(&self.dir, PAID_LOG_FILE, &next.to_bytes())?;
                self.paid_log = next;
                Ok(())
            }
            Err(max) => Err(format!("the log holds {max} entries already")),
        }
    }

    /// OC-50: the transaction `txid` is leaving the outbox, or its items are
    /// going back to the queue, with no confirmation that holds. Each entry
    /// of the log that it paid is undone by a later entry. It is written
    /// after the queue is told and while the item is still owed, and writing
    /// it twice is a no-op, so a restart between the two writes it once. It
    /// is never written only because a block left (OC-05).
    fn undo_logged(&mut self, txid: &Txid) -> Result<(), String> {
        let at = self.watcher.now();
        let undoings = self.paid_log.undoings_for(txid, at);
        match self.paid_log.with_added(&undoings) {
            Ok(None) => Ok(()),
            Ok(Some(next)) => {
                write_file(&self.dir, PAID_LOG_FILE, &next.to_bytes())?;
                self.paid_log = next;
                Ok(())
            }
            Err(max) => Err(format!("the log holds {max} entries already")),
        }
    }

    /// [`OwnerNode::undo_logged`] for a settlement: true when it is stored,
    /// and the item may leave the list.
    fn undone_for_item(&mut self, txid: &Txid) -> bool {
        match self.undo_logged(txid) {
            Ok(()) => true,
            Err(e) => {
                self.note(format!("{txid} could not be undone in the log: {e}"));
                false
            }
        }
    }

    /// Archive paid rows whose transactions are no longer in the coin view's
    /// undo horizon. Rows tied to a still-reorgable transaction remain linked
    /// to it so a reorganization can reopen them.
    pub fn archive_paid(&mut self) -> Result<usize, NodeError> {
        let _commit = TAKE_JOURNAL_LOCK
            .lock()
            .map_err(|_| NodeError::Files("take journal lock is poisoned".to_string()))?;
        self.ready_for_queue_write()?;
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
        let retained = self.view.retained_confirmations();
        let dir = self.dir.clone();
        self.owner
            .archive_paid(&retained, |b| write_file(&dir, QUEUE_FILE, b))
            .map_err(NodeError::from)
    }

    /// Roll a committed take forward into the owner's key 1 state store after
    /// a failed write or restart. The account key is used only as a trust
    /// anchor when reading the journaled signer snapshot; no private key is
    /// journaled. The persistence callback must only write key 1 state and
    /// must not call back into this node while recovery is running.
    pub fn recover_key1(
        &mut self,
        key1: &mut SignerState,
        key1_account: &Xpriv,
        persist_key1: impl FnOnce(&[u8]) -> Result<(), String>,
    ) -> Result<(), NodeError> {
        let commit = TAKE_JOURNAL_LOCK
            .lock()
            .map_err(|_| NodeError::Files("take journal lock is poisoned".to_string()))?;
        let Some(journal) = self.take_journal.clone() else {
            return Ok(());
        };
        let account = Xpub::from_priv(&bitcoin::secp256k1::Secp256k1::new(), key1_account);
        let restored = SignerState::from_bytes(&journal.key1, self.network(), &account)
            .map_err(|e| NodeError::Files(format!("journaled key 1 state is invalid: {e}")))?;
        let owner = Owner::restore(
            &self.owner_secret,
            self.wallet.clone(),
            self.approver,
            self.policy,
            &journal.queue,
        )
        .map_err(|e| NodeError::Files(format!("journaled {QUEUE_FILE} is invalid: {e}")))?;

        write_file(&self.dir, QUEUE_FILE, &journal.queue).map_err(NodeError::Files)?;
        persist_key1(&journal.key1).map_err(NodeError::Files)?;
        remove_take_journal(&self.dir).map_err(NodeError::Files)?;

        self.owner = owner;
        *key1 = restored;
        self.take_journal = None;
        self.halted = None;
        self.watch_limit = None;
        drop(commit);
        self.refresh_marks();
        self.reconcile();
        self.settle_owed();
        Ok(())
    }

    /// The coins that `build_request` would be given: empty unless the view
    /// is ready.
    pub fn coins(&self) -> Vec<openbmt_spend::Coin> {
        self.view.coins(self.watcher.chain().tip_height())
    }

    /// What can be offered to a spend: confirmed unreserved coins and the
    /// owner's own pending change.
    pub fn balance(&self) -> u64 {
        self.coins().iter().fold(0u64, |sum, coin| {
            sum.saturating_add(coin.output().map_or(0, |output| output.value.to_sat()))
        })
    }

    /// SS-36: what the starter wallet holds, from the coins the view offers
    /// and the payments into the wallet that the node holds and no block has
    /// confirmed (the library's [`held_sats`]). `None` unless the view is
    /// ready: a view that is not ready has no coin to count.
    ///
    /// [`held_sats`]: openbmt_owner::starter::held_sats
    pub fn starter_held(&self) -> Option<u64> {
        if self.repinned || !self.view.ready() {
            return None;
        }
        let coins = self.coins();
        let incoming: Vec<IncomingCoin> = self
            .view
            .pending()
            .flat_map(|p| {
                let txid = p.tx.compute_txid();
                p.own.iter().filter_map(move |(vout, _, _)| {
                    let at = usize::try_from(*vout).ok()?;
                    Some(IncomingCoin {
                        outpoint: OutPoint::new(txid, *vout),
                        sats: p.tx.output.get(at)?.value.to_sat(),
                    })
                })
            })
            .collect();
        Some(held_sats(&coins, &incoming))
    }

    /// SS-30, SS-42: the coins the starter wallet signs from, the height of
    /// the tip they are counted from and the next index of the change
    /// chain. The view must be ready and have checked every block up to the
    /// tip, as for the spends of the shop's own wallet (AS-101).
    pub fn starter_coins(&self) -> Result<(Vec<openbmt_spend::Coin>, u32, u32), NodeError> {
        if self.repinned {
            return Err(NodeError::NotReady(
                "the shop's wallet changed: the node starts again".into(),
            ));
        }
        if let Some(why) = &self.halted {
            return Err(NodeError::NotReady(why.clone()));
        }
        if self.awaiting_rescan > 0 {
            return Err(NodeError::NotReady(
                "waiting for the script rescan to start".into(),
            ));
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
        Ok((self.view.coins(tip), tip, self.view.change_next()))
    }

    /// SS-30, SS-42: a finished transaction of the starter wallet goes to
    /// the outbox, and the node sends it and follows it as it does the
    /// spends of the shop's own wallet (AS-104). `refunds` are the queued
    /// refunds it pays.
    pub fn take_starter(&mut self, tx: &[u8], refunds: &[u32]) -> Result<Txid, NodeError> {
        let write = self.queue_writer();
        let txid = self.owner_mut()?.take_starter(tx, refunds, write)?;
        self.refresh_marks();
        self.reconcile();
        Ok(txid)
    }

    /// SS-41: the shop's wallet was re-pinned: the node reads nothing more
    /// of the view as the wallet in force, and the app starts it again.
    /// `wallet` is the wallet pinned now: the requests made from here on take
    /// their addresses from it (RQ-04), the earlier ones from the wallet they
    /// were made from.
    pub fn mark_repinned(&mut self, wallet: ImportedWallet) {
        self.repinned = true;
        self.pinned_now = Some(wallet);
    }

    /// SS-43: the former wallets that hold a coin the node saw, each with
    /// what it holds. A view that is not ready, or has not checked every
    /// block up to the tip, says nothing: it has no coin to count.
    pub fn former_held(&self) -> Vec<FormerHeld> {
        let tip = self.watcher.chain().tip_height();
        self.former
            .iter()
            .filter(|f| f.view.ready() && f.view.checked() >= tip)
            .filter_map(|f| {
                let coins = f.view.coins(tip);
                let sats = coins.iter().fold(0u64, |sum, coin| {
                    sum.saturating_add(coin.output().map_or(0, |o| o.value.to_sat()))
                });
                (!coins.is_empty()).then(|| FormerHeld {
                    wallet: f.wallet.checksum32(),
                    sats,
                    coins: u32::try_from(coins.len()).unwrap_or(u32::MAX),
                })
            })
            .collect()
    }

    /// SS-43: the coins of the former wallet `wallet` and the tip they are
    /// counted from, for the move of a late payment. Ready and caught up
    /// as [`OwnerNode::starter_coins`] needs.
    pub fn former_coins(
        &self,
        wallet: &[u8; 32],
    ) -> Result<(Vec<openbmt_spend::Coin>, u32), NodeError> {
        if let Some(why) = &self.halted {
            return Err(NodeError::NotReady(why.clone()));
        }
        if self.awaiting_rescan > 0 {
            return Err(NodeError::NotReady(
                "waiting for the script rescan to start".into(),
            ));
        }
        let f = self
            .former
            .iter()
            .find(|f| f.wallet.checksum32() == *wallet)
            .ok_or_else(|| NodeError::NotReady("no such former wallet".into()))?;
        if !f.view.ready() {
            return Err(NodeError::NotReady(f.view.state().to_string()));
        }
        let tip = self.watcher.chain().tip_height();
        if f.view.checked() < tip {
            return Err(NodeError::NotReady(format!(
                "the coin view has checked blocks up to {} and the chain is at {tip}",
                f.view.checked()
            )));
        }
        Ok((f.view.coins(tip), tip))
    }

    /// The height of the block that holds `txid`, in the view of the wallet
    /// in force or of a former one.
    fn confirmed_height_any(&self, txid: &Txid) -> Option<u32> {
        self.view.confirmed_height(txid).or_else(|| {
            self.former
                .iter()
                .find_map(|f| f.view.confirmed_height(txid))
        })
    }

    /// SS-43: how deep the transaction `txid` is in the checked chain: 0
    /// while the node holds it and no block does, `None` when the node
    /// knows nothing of it.
    pub fn starter_confirmations(&self, txid: &Txid) -> Option<u32> {
        let tip = self.watcher.chain().tip_height();
        if let Some(h) = self.confirmed_height_any(txid) {
            return Some(tip.saturating_sub(h).saturating_add(1));
        }
        match self.send_state(txid) {
            Some(SendState::Confirmed { depth }) => Some(depth),
            Some(SendState::Done) => Some(self.refund_depth().max(1)),
            Some(_) => Some(0),
            None => None,
        }
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
        match self.confirmed_height_any(txid) {
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

    pub(crate) fn note(&mut self, line: String) {
        self.watcher.note(line);
    }

    pub(crate) fn stop(&mut self, line: String) {
        self.note(line.clone());
        if self.halted.is_none() {
            self.halted = Some(line);
        }
    }

    // The queue and the keys.

    /// The marks, from what the queue and the view hold. On growth, the new
    /// scripts are installed and the driver rechecks from one block below the
    /// last checked height. This short scan catches work already in flight;
    /// it does not revisit the wallet's birthday range. It runs at start
    /// before the driver is started, right before each block and
    /// reorganization, and after each batch or spend changes the queue.
    pub fn refresh_marks(&mut self) {
        if let Err(e) = self.refresh_marks_checked() {
            // A failure that did not stop the node already stops it here.
            if self.halted.is_none() {
                self.stop(e.to_string());
            }
        }
    }

    /// OC-10, OA-107: a shop with its own wallet gave out another range.
    /// The book's ranges, each with the owner's clock when it was given, are
    /// the issued ranges the live watch follows (OC-22): the new indexes are
    /// watched from here on, and quiet ones are set aside as OC-23 says. A
    /// book that has not grown changes nothing. Refused for a node that was
    /// not started with a book.
    pub fn set_own_book(&mut self, book: &AllocationBook) -> Result<(), NodeError> {
        let Some(own) = &self.own else {
            return Err(NodeError::Config(
                "OA-107: the node was started for a shop with a Key app".to_string(),
            ));
        };
        // A book only grows: the same count and end is the same book.
        if own.next == book.next_start()
            && u32::try_from(own.ranges.len()).is_ok_and(|n| n == book.ranges_given())
        {
            return Ok(());
        }
        self.own = Some(OwnIssued::of(book));
        self.refresh_marks_checked()
    }

    /// OC-22: whether the live watch could follow the indexes up to `end`
    /// (one past the last) with the scripts that can be set aside now. A
    /// shop with its own wallet asks before it gives a counter another
    /// range.
    pub fn own_room_for(&self, end: u32) -> bool {
        if self.watch_limit.is_some() {
            return false;
        }
        let issued = issuing(&self.owner, self.own.as_ref());
        let wider = Issuing {
            receive: issued.receive.max(end),
            change: issued.change,
            ranges: issued.ranges,
        };
        let marks = marks_of(&wider, &self.view);
        release_plan(
            &wider,
            &self.view,
            &self.desk_kept(),
            marks,
            self.watcher.now(),
            &self.limits,
            self.deep_run.is_none(),
        )
        .is_ok()
    }

    fn refresh_marks_checked(&mut self) -> Result<(), NodeError> {
        if let Some(error) = &self.watch_limit {
            return Err(error.clone());
        }
        let issued = issuing(&self.owner, self.own.as_ref());
        let (receive, change) = marks_of(&issued, &self.view);
        let grew = receive > self.view.receive_next() || change > self.view.change_next();
        let now = self.watcher.now();
        // OC-22: scripts are set aside when the marks grow, and now and then
        // as time makes more of them quiet; never while a deep check runs,
        // which would leave them out of the pass.
        let allow = self.deep_run.is_none() && (grew || now >= self.next_rebalance);
        let release = match release_plan(
            &issued,
            &self.view,
            &self.desk_kept(),
            (receive, change),
            now,
            &self.limits,
            allow,
        ) {
            Ok(release) => release,
            Err(error) => {
                self.watch_limit = Some(error.clone());
                self.stop(error.to_string());
                return Err(error);
            }
        };
        if allow {
            self.next_rebalance = now.saturating_add(REBALANCE_SECS);
        }
        let dir = self.dir.clone();
        let tip = self.watcher.chain().tip_height();
        let due = now.saturating_add(self.limits.deep_secs).max(2);
        match self
            .view
            .watch_plan(receive, change, tip, &release, due, |b| {
                write_file(&dir, COINS_FILE, b)
            }) {
            Ok(false) => Ok(()),
            Ok(true) => {
                let new = live_scripts(&self.wallet, &self.view)?;
                let added: Vec<ScriptBuf> = new
                    .keys()
                    .filter(|s| !self.scripts.contains_key(*s))
                    .cloned()
                    .collect();
                let gone: Vec<ScriptBuf> = self
                    .scripts
                    .keys()
                    .filter(|s| !new.contains_key(*s))
                    .cloned()
                    .collect();
                for script in added {
                    self.watcher.port().watch_script(script);
                }
                if !gone.is_empty() {
                    self.watcher.port().unwatch_scripts(gone);
                }
                self.scripts = new;
                if !release.is_empty() {
                    self.note(format!(
                        "{} old addresses are checked once a month instead of all the time",
                        release.len()
                    ));
                }
                if grew && self.view.state() != ViewState::DeepReorg {
                    let from = self.view.checked();
                    self.awaiting_rescan = self.awaiting_rescan.saturating_add(1);
                    self.watcher.port().rescan_from(from);
                    match self.view.state() {
                        ViewState::Rescanning { to } => self.note(format!(
                            "new addresses are watched; rechecking blocks after {from} through {to}"
                        )),
                        _ => self.note(
                            "new addresses are watched; the driver is updating its watch set"
                                .to_string(),
                        ),
                    }
                }
                Ok(())
            }
            Err(e) => {
                let detail = format!("the coin view could not be stored: {e}");
                self.stop(detail.clone());
                Err(NodeError::Files(detail))
            }
        }
    }

    /// Start again from the birthday: for a view in the deep reorg state, a
    /// restored owner or a person who suspects a missed payment.
    pub fn rescan(&mut self) -> Result<(), NodeError> {
        let issued = issuing(&self.owner, self.own.as_ref());
        let (receive, change) = marks_of(&issued, &self.view);
        let aside = self
            .view
            .released()
            .count_below(receive.saturating_add(GAP).min(INDEX_LIMIT));
        ensure_marks_fit_watch_budget(receive, change, aside)?;
        self.cancel_deep("the wallet is checked again from the start");
        let dir = self.dir.clone();
        self.view
            .reset(|b| write_file(&dir, COINS_FILE, b))
            .map_err(NodeError::Files)?;
        self.watcher.port().rescan_from(self.view.birthday());
        self.awaiting_rescan = self.awaiting_rescan.saturating_add(1);
        self.halted = None;
        self.watch_limit = None;
        self.note("rescanning from the birthday height".to_string());
        Ok(())
    }

    /// Build the next session for the Key app from the queue and the coins
    /// the view holds. The view must be ready and have checked every block up
    /// to the tip, or nothing is built.
    pub fn build_request(&mut self, fee_rate: u64, now: u64) -> Result<Built, NodeError> {
        self.build_request_from(fee_rate, now, |coins| coins)
    }

    /// [`OwnerNode::build_request`] over the coins that `pick` keeps of the
    /// ones the view offers. The merge of OC-20 hands in fewer coins than
    /// the wallet holds when they do not fit one transaction.
    pub(crate) fn build_request_from(
        &mut self,
        fee_rate: u64,
        now: u64,
        pick: impl FnOnce(Vec<openbmt_spend::Coin>) -> Vec<openbmt_spend::Coin>,
    ) -> Result<Built, NodeError> {
        self.build_with(fee_rate, now, pick, false)
            .map(|(built, _)| built)
    }

    /// AS-90: [`OwnerNode::build_request`] for a wallet whose key 2 is a
    /// hardware signer. The spends are in a session of their own, whose ID
    /// is returned beside the Key app's request.
    pub fn build_request_for_signer(
        &mut self,
        fee_rate: u64,
        now: u64,
    ) -> Result<(Built, Option<[u8; 32]>), NodeError> {
        self.build_with(fee_rate, now, |coins| coins, true)
    }

    fn build_with(
        &mut self,
        fee_rate: u64,
        now: u64,
        pick: impl FnOnce(Vec<openbmt_spend::Coin>) -> Vec<openbmt_spend::Coin>,
        for_signer: bool,
    ) -> Result<(Built, Option<[u8; 32]>), NodeError> {
        let commit = TAKE_JOURNAL_LOCK
            .lock()
            .map_err(|_| NodeError::Files("take journal lock is poisoned".to_string()))?;
        self.ready_for_queue_write()?;
        if let Some(why) = &self.halted {
            return Err(NodeError::NotReady(why.clone()));
        }
        if self.awaiting_rescan > 0 {
            return Err(NodeError::NotReady(
                "waiting for the script rescan to start".into(),
            ));
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
        // OC-47: a refund is ready on the facts of the view as it is now.
        {
            let dir = self.dir.clone();
            self.refresh_refund_facts(tip, &mut |b| write_file(&dir, QUEUE_FILE, b))
                .map_err(NodeError::Files)?;
        }
        let coins = pick(self.view.coins(tip));
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
        let built = if for_signer {
            self.owner.build_request_for_signer(
                &coins,
                fee_rate,
                change_index,
                tip,
                now,
                &random,
                |b| write_file(&dir, QUEUE_FILE, b),
            )?
        } else {
            let built = self.owner.build_request(
                &coins,
                fee_rate,
                change_index,
                tip,
                now,
                &random,
                |b| write_file(&dir, QUEUE_FILE, b),
            )?;
            (built, None)
        };
        drop(commit);
        self.refresh_marks_checked()?;
        Ok(built)
    }

    /// AS-101: freeze the queued refunds and cash-outs the owner picked into
    /// a session for the shop's own wallet, from the coins the view holds.
    /// The view must be ready and have checked every block up to the tip, or
    /// nothing is built. Nothing is signed: the wallet signs (AS-102 to
    /// AS-104).
    pub fn build_own_spend(
        &mut self,
        picked: &[u32],
        fee_rate: u64,
        now: u64,
    ) -> Result<OwnBuilt, NodeError> {
        self.build_own_from(picked, fee_rate, now, |coins| coins)
    }

    /// [`OwnerNode::build_own_spend`] over the coins that `pick` keeps of
    /// the ones the view offers: the merge of AS-106 hands in fewer coins
    /// than the wallet holds when they do not fit one transaction (OC-21).
    pub(crate) fn build_own_from(
        &mut self,
        picked: &[u32],
        fee_rate: u64,
        now: u64,
        pick: impl FnOnce(Vec<openbmt_spend::Coin>) -> Vec<openbmt_spend::Coin>,
    ) -> Result<OwnBuilt, NodeError> {
        let commit = TAKE_JOURNAL_LOCK
            .lock()
            .map_err(|_| NodeError::Files("take journal lock is poisoned".to_string()))?;
        self.ready_for_queue_write()?;
        if let Some(why) = &self.halted {
            return Err(NodeError::NotReady(why.clone()));
        }
        if self.awaiting_rescan > 0 {
            return Err(NodeError::NotReady(
                "waiting for the script rescan to start".into(),
            ));
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
        // OC-47: a refund is ready on the facts of the view as it is now.
        {
            let dir = self.dir.clone();
            self.refresh_refund_facts(tip, &mut |b| write_file(&dir, QUEUE_FILE, b))
                .map_err(NodeError::Files)?;
        }
        let coins = pick(self.view.coins(tip));
        let mut random = [0u8; 32];
        for chunk in random.chunks_mut(8) {
            let word = self.random.next_u64().to_le_bytes();
            let n = chunk.len();
            if let Some(src) = word.get(..n) {
                chunk.copy_from_slice(src);
            }
        }
        let change_index = self.view.change_next();
        let dir = self.dir.clone();
        let built = self.owner.build_own_spend(
            &coins,
            picked,
            fee_rate,
            change_index,
            tip,
            now,
            &random,
            |b| write_file(&dir, QUEUE_FILE, b),
        )?;
        drop(commit);
        self.refresh_marks_checked()?;
        Ok(built)
    }

    /// AS-102, AS-105: the file of item `number` of the own session
    /// `session`, for the shop's wallet. From the first call the item's
    /// transaction is watched: a block that holds it pays its entries, and a
    /// block that spends one of its coins in another transaction gives them
    /// back (OC-52).
    pub fn own_file(&mut self, session: &[u8; 32], number: u32) -> Result<OwnFile, NodeError> {
        let write = self.queue_writer();
        Ok(self.owner_mut()?.own_file(session, number, write)?)
    }

    /// AS-103, AS-104: take what the shop's wallet returned for item
    /// `number` of the own session `session`. Only the queue changes, so it
    /// is stored as any queue change; the finished transaction then goes out
    /// through the outbox and the send path as any other.
    pub fn take_own(
        &mut self,
        session: &[u8; 32],
        number: u32,
        returned: &[u8],
    ) -> Result<OwnTaken, NodeError> {
        let write = self.queue_writer();
        let taken = self
            .owner_mut()?
            .take_own(session, number, returned, write)?;
        self.refresh_marks();
        self.reconcile();
        Ok(taken)
    }

    /// Take the Key app's response. A durable journal commits the queue and
    /// key 1 snapshot as one roll-forward unit before either separate store
    /// is changed. A failed second store requires [`OwnerNode::recover_key1`].
    /// The persistence callback must only write key 1 state and must not call
    /// back into this node while the commit is running.
    pub fn take(
        &mut self,
        response: &[u8],
        key1: &mut SignerState,
        key1_account: &Xpriv,
        now: u64,
        persist_key1: impl FnOnce(&[u8]) -> Result<(), String>,
    ) -> Result<Taken, NodeError> {
        let commit = TAKE_JOURNAL_LOCK
            .lock()
            .map_err(|_| NodeError::Files("take journal lock is poisoned".to_string()))?;
        self.ready_for_queue_write()?;
        let dir = self.dir.clone();
        let mut committed = None;
        let taken = self
            .owner
            .take_response(response, key1, key1_account, now, |queue, key| {
                let journal = TakeJournal {
                    queue: queue.to_vec(),
                    key1: key.to_vec(),
                };
                let bytes = journal.to_bytes()?;
                if let Err(error) = write_file(&dir, TAKE_JOURNAL_FILE, &bytes) {
                    if dir.join(TAKE_JOURNAL_FILE).exists() {
                        committed = Some(journal);
                    }
                    return Err(error);
                }
                committed = Some(journal);
                write_file(&dir, QUEUE_FILE, queue)?;
                persist_key1(key)?;
                remove_take_journal(&dir)
            });
        let taken = match taken {
            Ok(taken) => taken,
            Err(error) => {
                if let Some(journal) = committed {
                    self.take_journal = Some(journal);
                    self.stop(format!(
                        "a committed response needs key 1 recovery: {error}"
                    ));
                    return Err(NodeError::KeyRecoveryRequired);
                }
                return Err(error.into());
            }
        };
        drop(commit);
        self.refresh_marks();
        self.reconcile();
        Ok(taken)
    }

    /// AS-91: sign item `number` of the hardware signer's session with key 1
    /// and make the file for the signer. The queue and key 1's state are
    /// committed as one unit through the take journal, as for
    /// [`OwnerNode::take`], and a failed second store requires
    /// [`OwnerNode::recover_key1`].
    pub fn export_for_signer(
        &mut self,
        session: &[u8; 32],
        number: u32,
        key1: &mut SignerState,
        key1_account: &Xpriv,
        now: u64,
        persist_key1: impl FnOnce(&[u8]) -> Result<(), String>,
    ) -> Result<openbmt_owner::hardware::SignerFile, NodeError> {
        let commit = TAKE_JOURNAL_LOCK
            .lock()
            .map_err(|_| NodeError::Files("take journal lock is poisoned".to_string()))?;
        self.ready_for_queue_write()?;
        let dir = self.dir.clone();
        let mut committed = None;
        let file =
            self.owner
                .export_for_signer(session, number, key1, key1_account, now, |queue, key| {
                    let journal = TakeJournal {
                        queue: queue.to_vec(),
                        key1: key.to_vec(),
                    };
                    let bytes = journal.to_bytes()?;
                    if let Err(error) = write_file(&dir, TAKE_JOURNAL_FILE, &bytes) {
                        if dir.join(TAKE_JOURNAL_FILE).exists() {
                            committed = Some(journal);
                        }
                        return Err(error);
                    }
                    committed = Some(journal);
                    write_file(&dir, QUEUE_FILE, queue)?;
                    persist_key1(key)?;
                    remove_take_journal(&dir)
                });
        let file = match file {
            Ok(file) => file,
            Err(error) => {
                if let Some(journal) = committed {
                    self.take_journal = Some(journal);
                    self.stop(format!(
                        "a committed signing file needs key 1 recovery: {error}"
                    ));
                    return Err(NodeError::KeyRecoveryRequired);
                }
                return Err(error.into());
            }
        };
        drop(commit);
        Ok(file)
    }

    /// AS-96: take what the hardware signer returned for item `number` of its
    /// session. Only the queue changes, so it is stored as any queue change.
    pub fn take_from_signer(
        &mut self,
        session: &[u8; 32],
        number: u32,
        returned: &[u8],
    ) -> Result<openbmt_owner::hardware::SignerTaken, NodeError> {
        let write = self.queue_writer();
        let taken = self
            .owner_mut()?
            .take_from_signer(session, number, returned, write)?;
        self.refresh_marks();
        self.reconcile();
        Ok(taken)
    }

    /// AS-66, AS-89: give up on an open approval. One whose file went to the
    /// hardware signer is refused while a coin it spends is gone from the
    /// view: something spent it that the node was not told of, perhaps the
    /// transaction itself, and the entries must not be planned again freely.
    pub fn abandon_session(&mut self, id: &[u8; 32]) -> Result<(), NodeError> {
        let gone = self
            .owner
            .exported_transactions(Some(id))
            .iter()
            .flat_map(|e| e.inputs.iter())
            .any(|op| !self.known_input(op));
        if gone {
            return Err(NodeError::Owner(OwnerError::Refused(
                openbmt_owner::Refusal::WrongStage(
                    "its dropped transaction may still confirm; ask for it again",
                ),
            )));
        }
        let write = self.queue_writer();
        self.owner_mut()?.abandon_session(id, write)?;
        Ok(())
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
        if self.halted.is_some() {
            return;
        }
        match self.send(&txid, bytes) {
            Ok(()) => {
                self.states.insert(txid, SendState::Sent);
                // OC-48: the payment is in the log before the queue is told
                // it was sent. Without it the entry stays signed, and the
                // next start sends again and tries again.
                if let Err(e) = self.log_paid(&txid) {
                    self.note(format!("{txid} was sent but could not be logged: {e}"));
                    return;
                }
                if self.is_signed(&txid) {
                    let dir = self.dir.clone();
                    if let Err(e) = self
                        .owner
                        .mark_broadcast(&txid, |b| write_queue_file(&dir, b))
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
        self.main_knows(op) || self.former.iter().any(|f| f.knows(op))
    }

    fn main_knows(&self, op: &OutPoint) -> bool {
        self.view.owned().any(|c| c.outpoint == *op)
            || self.view.pending().any(|p| {
                p.tx.compute_txid() == op.txid && p.own.iter().any(|(v, _, _)| *v == op.vout)
            })
    }

    /// Every transaction in the queue's outbox that is not followed yet is
    /// held, sent and followed (the module docs, "What a restart does").
    fn reconcile(&mut self) {
        if self.halted.is_some() {
            return;
        }
        if self.take_recovery_pending() {
            self.note("queue recovery waits for the owner's key 1 state recovery".to_string());
            return;
        }
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
            if self.confirmed_height_any(&txid).is_some() {
                self.states.insert(txid, SendState::Sent);
            } else if self.view.pending().any(|p| p.tx.compute_txid() == txid)
                || self
                    .former
                    .iter()
                    .any(|f| f.view.pending().any(|p| p.tx.compute_txid() == txid))
            {
                self.dispatch(txid, &bytes);
            } else if let Some(at) = self
                .former
                .iter()
                .position(|f| tx.input.iter().all(|i| f.knows(&i.previous_output)))
                .filter(|_| !tx.input.iter().all(|i| self.main_knows(&i.previous_output)))
            {
                // SS-43: a move of a late payment spends the coins of a
                // former wallet, which that wallet's own view holds.
                let dir = self.dir.clone();
                let Some(f) = self.former.get_mut(at) else {
                    continue;
                };
                match f
                    .view
                    .add_pending(&tx, &f.scripts, |b| write_file(&dir, &f.file, b))
                {
                    Ok(_) => self.dispatch(txid, &bytes),
                    Err(PendingError::Full) => {
                        self.note(format!("{txid} waits: 64 transactions are held already"));
                    }
                    Err(PendingError::Store(e)) => {
                        self.note(format!("{txid} could not be held: {e}"));
                    }
                }
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
        // A transaction that was paid and then reorged is present in the
        // view's pending set even if the UI already archived its rows and its
        // outbox record. The view's undo record is the durable signed source.
        let pending: Vec<Transaction> = self.view.pending().map(|p| p.tx.clone()).collect();
        for tx in pending {
            let txid = tx.compute_txid();
            if self.states.contains_key(&txid) {
                continue;
            }
            let bytes = bitcoin::consensus::serialize(&tx);
            if let Err(e) = self.ready_for_queue_write() {
                self.note(format!("{txid} waits to reopen after Key recovery: {e}"));
                return;
            }
            let dir = self.dir.clone();
            match self.owner.reopen_paid(&tx, |b| write_queue_file(&dir, b)) {
                Ok(_) => self.dispatch(txid, &bytes),
                Err(e) => {
                    self.stop(format!(
                        "{txid} could not be reopened after a reorganization: {e}"
                    ));
                    return;
                }
            }
        }
        if self.states.len() > MAX_STATES {
            self.states.retain(|_, s| *s != SendState::Done);
        }
    }

    /// A transaction at the refund depth is reported paid and let go; one
    /// whose coins another transaction spent goes back to the queue.
    fn settle(&mut self) {
        if self.halted.is_some() || self.take_recovery_pending() {
            return;
        }
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
                self.confirmed_height_any(txid)
                    .is_some_and(|h| tip.saturating_sub(h).saturating_add(1) >= depth)
            })
            .collect();
        for txid in done {
            if self.ready_for_queue_write().is_err() {
                return;
            }
            if let Err(e) = self.log_paid(&txid) {
                self.note(format!(
                    "{txid} is {depth} deep but could not be logged: {e}"
                ));
                continue;
            }
            let dir = self.dir.clone();
            match self
                .owner
                .mark_confirmed(&txid, |b| write_queue_file(&dir, b))
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
        // A transaction whose UI rows were archived has no outbox to remove
        // when it reconfirms. Finish tracking it from the coin view's undo.
        let finished_without_rows: Vec<Txid> = self
            .states
            .iter()
            .filter_map(|(txid, state)| {
                if matches!(
                    state,
                    SendState::Sent
                        | SendState::SeenBack { .. }
                        | SendState::NotSeen
                        | SendState::HeldByNode { .. }
                ) && !self.owner.outbox().iter().any(|d| {
                    matches!(
                        &d.what,
                        Delivery::Transaction { txid: held, .. } if held == txid
                    )
                }) && self
                    .confirmed_height_any(txid)
                    .is_some_and(|h| tip.saturating_sub(h).saturating_add(1) >= depth)
                {
                    Some(*txid)
                } else {
                    None
                }
            })
            .collect();
        for txid in finished_without_rows {
            self.unsend(&txid);
            self.states.insert(txid, SendState::Done);
            self.relays.remove(&txid);
        }
    }

    /// OC-52, AS-89: what the block `hash` owes the queue, found from its
    /// transactions alone. The transaction of an open exported approval or of
    /// a signed-elsewhere entry that the block holds is paid; an entry one of
    /// whose coins the block spends in another transaction is beaten.
    fn owed_in(&self, hash: &BlockHash, bytes: &[u8]) -> Vec<Owed> {
        let exported = self.owner.exported_transactions(None);
        let elsewhere = self.owner.signed_elsewhere();
        // OC-52: the transactions of the owner's own outbox, with the coins
        // each spends.
        let outbox: Vec<(Txid, Vec<OutPoint>)> = self
            .owner
            .outbox()
            .iter()
            .filter_map(|d| match &d.what {
                Delivery::Transaction { txid, tx, .. } => {
                    let tx: Transaction = deserialize(tx).ok()?;
                    Some((*txid, tx.input.iter().map(|i| i.previous_output).collect()))
                }
                _ => None,
            })
            .collect();
        if exported.is_empty() && elsewhere.is_empty() && outbox.is_empty() {
            return Vec::new();
        }
        let Ok(block) = deserialize::<bitcoin::Block>(bytes) else {
            return Vec::new();
        };
        let mut ids: BTreeSet<Txid> = BTreeSet::new();
        let mut spent: BTreeMap<OutPoint, Txid> = BTreeMap::new();
        for tx in &block.txdata {
            let id = tx.compute_txid();
            ids.insert(id);
            for input in &tx.input {
                spent.insert(input.previous_output, id);
            }
        }
        // OC-54: what a block paid is kept with the transaction as the block
        // holds it.
        let in_block = |txid: &Txid| block.txdata.iter().find(|t| t.compute_txid() == *txid);
        let mut owed = Vec::new();
        // OC-55: the transaction that spent one of a beaten transaction's
        // coins, whose outputs a new transaction may spend.
        let beaten_by = |txid: &Txid, inputs: &[OutPoint]| {
            inputs
                .iter()
                .find_map(|op| spent.get(op).copied().filter(|by| by != txid))
        };
        for (txid, inputs) in &outbox {
            if let Some(by) = beaten_by(txid, inputs).filter(|_| !ids.contains(txid)) {
                owed.push(Owed {
                    kind: OwedKind::OutboxBeaten,
                    txid: *txid,
                    block: *hash,
                    tx: None,
                    by: Some(by),
                });
            }
        }
        // AS-105: an item whose file went to the shop's own wallet is beaten
        // when a block spends one of its coins in another transaction.
        for open in self.owner.own_exported_transactions() {
            if let Some(by) =
                beaten_by(&open.txid, &open.inputs).filter(|_| !ids.contains(&open.txid))
            {
                owed.push(Owed {
                    kind: OwedKind::ExportedBeaten,
                    txid: open.txid,
                    block: *hash,
                    tx: None,
                    by: Some(by),
                });
            }
        }
        for open in &exported {
            if ids.contains(&open.txid) {
                owed.push(Owed {
                    kind: OwedKind::ExportedPaid,
                    txid: open.txid,
                    block: *hash,
                    tx: in_block(&open.txid).cloned(),
                    by: None,
                });
            }
        }
        for entry in elsewhere {
            let (kind, tx, by) = if ids.contains(&entry.txid) {
                (
                    OwedKind::ElsewherePaid,
                    in_block(&entry.txid).cloned(),
                    None,
                )
            } else if let Some(by) = beaten_by(&entry.txid, &entry.inputs) {
                (OwedKind::ElsewhereBeaten, None, Some(by))
            } else {
                continue;
            };
            owed.push(Owed {
                kind,
                txid: entry.txid,
                block: *hash,
                tx,
                by,
            });
        }
        owed
    }

    /// OC-52: store the owed list `next`, and keep it only if it was stored.
    fn store_owed(&mut self, next: SettleOwed) -> Result<(), String> {
        if next == self.owed {
            return Ok(());
        }
        write_file(&self.dir, SETTLE_OWED_FILE, &next.to_bytes())?;
        self.owed = next;
        Ok(())
    }

    /// OC-52: an item is settled or can never be: take it out of the file.
    /// If that cannot be stored the item stays, and settling it again finds
    /// nothing open and takes it out then.
    fn forget_owed(&mut self, item: &Owed) {
        let next = self.owed.without(item);
        if let Err(e) = self.store_owed(next) {
            self.note(format!(
                "{} is settled but still listed as owed: {e}",
                item.txid
            ));
        }
    }

    /// OC-52: the blocks that left the chain take what they owed with them.
    fn forget_owed_blocks(&mut self, blocks: &[BlockHash]) {
        // An outbox transaction whose beating block left is not beaten: the
        // view holds it again, as the block took it out.
        let back: Vec<Txid> = self
            .owed
            .items()
            .iter()
            .filter(|o| o.kind == OwedKind::OutboxBeaten && blocks.contains(&o.block))
            .map(|o| o.txid)
            .collect();
        let next = self.owed.without_blocks(blocks);
        let dropped = self.owed.items().len().saturating_sub(next.items().len());
        if dropped == 0 {
            return;
        }
        match self.store_owed(next) {
            Ok(()) => {
                self.note(format!(
                    "{dropped} settlement(s) owed by a block that left the chain were dropped"
                ));
                self.hold_again(&back);
            }
            Err(e) => self.note(format!(
                "settlements owed by a block that left the chain: {e}"
            )),
        }
    }

    /// The view holds these outbox transactions as pending again.
    fn hold_again(&mut self, txids: &[Txid]) {
        let held: Vec<Transaction> = self
            .owner
            .outbox()
            .iter()
            .filter_map(|d| match &d.what {
                Delivery::Transaction { txid, tx, .. } if txids.contains(txid) => {
                    deserialize(tx).ok()
                }
                _ => None,
            })
            .collect();
        let dir = self.dir.clone();
        for tx in held {
            if let Err(e) = self
                .view
                .add_pending(&tx, &self.scripts, |b| write_file(&dir, COINS_FILE, b))
            {
                self.note(format!(
                    "{} could not be held again: {e:?}",
                    tx.compute_txid()
                ));
            }
        }
    }

    /// OC-52, AS-89: transactions that were given to a hardware signer are
    /// followed by their IDs, which signatures do not change. One a block
    /// holds pays its entries; one whose coin a block spent in another
    /// transaction can no longer confirm. Each is settled from the owed list,
    /// whenever the queue may be written, and leaves the list only after its
    /// change to the queue is stored.
    fn settle_owed(&mut self) {
        if self.halted.is_some() || self.owed.is_empty() {
            return;
        }
        let items: Vec<Owed> = self.owed.items().to_vec();
        for item in items {
            if self.halted.is_some() {
                return;
            }
            // OC-53: a block the chain does not hold has left, whatever the
            // chain's height.
            if self.watcher.chain().height_of(&item.block).is_none() {
                self.note(format!(
                    "{} was owed by block {}, which is not on the best chain; dropped",
                    item.txid, item.block
                ));
                self.forget_owed(&item);
                continue;
            }
            match item.kind {
                OwedKind::ExportedPaid => self.settle_exported(&item),
                OwedKind::ElsewherePaid => self.settle_elsewhere_paid(&item),
                OwedKind::ElsewhereBeaten => self.settle_elsewhere_beaten(&item),
                OwedKind::ExportedBeaten => self.settle_exported_beaten(&item),
                OwedKind::OutboxBeaten => self.settle_outbox_beaten(&item),
            }
        }
    }

    /// An approval still open whose file went to the signer: the transaction
    /// may have been finished and sent without the owner.
    fn settle_exported(&mut self, item: &Owed) {
        let (txid, block) = (item.txid, item.block);
        let Some(open) = self
            .owner
            .exported_transactions(None)
            .into_iter()
            .find(|e| e.txid == txid)
        else {
            return self.forget_owed(item);
        };
        if self.ready_for_queue_write().is_err() {
            self.note(format!("{txid} waits for key 1 recovery"));
            return;
        }
        let dir = self.dir.clone();
        if let Err(e) = self.log_paid_entries(&txid, item.tx.as_ref(), open.refunds, open.cash_out)
        {
            self.note(format!(
                "{txid} is in block {block} but could not be logged: {e}"
            ));
            return;
        }
        if !self.keep_confirmed(item) {
            return;
        }
        match self
            .owner
            .mark_exported_confirmed(&txid, |b| write_queue_file(&dir, b))
        {
            Ok(()) => {
                self.note(format!(
                    "{txid} was given to the signer and is in block {block}: its entries are paid"
                ));
                self.forget_owed(item);
            }
            Err(e) => self.note(format!("{txid} could not be marked paid: {e}")),
        }
    }

    /// OC-54: the transaction a block paid, taken from that block, goes into
    /// the view's undo record of the block, so that a reorganization that
    /// removes the block reopens its entries and follows the transaction.
    /// False when that could not be stored: the item stays owed.
    fn keep_confirmed(&mut self, item: &Owed) -> bool {
        let Some(tx) = &item.tx else {
            return true;
        };
        let dir = self.dir.clone();
        match self
            .view
            .keep_confirmed(&item.block, tx, self.watcher.chain(), &self.scripts, |b| {
                write_file(&dir, COINS_FILE, b)
            }) {
            Ok(_) => true,
            Err(e) => {
                self.note(format!(
                    "{} is in block {} but the coin view could not be stored: {e}",
                    item.txid, item.block
                ));
                false
            }
        }
    }

    /// AS-105: another transaction spent a coin of a payment whose file went
    /// to the shop's own wallet, so it can no longer confirm. Its item leaves
    /// the session and its entries keep the outpoints it spent (AS-68).
    fn settle_exported_beaten(&mut self, item: &Owed) {
        let txid = item.txid;
        if !self
            .owner
            .own_exported_transactions()
            .iter()
            .any(|e| e.txid == txid)
        {
            if self.undone_for_item(&txid) {
                self.forget_owed(item);
            }
            return;
        }
        if self.ready_for_queue_write().is_err() {
            self.note(format!("{txid} waits for key 1 recovery"));
            return;
        }
        let dir = self.dir.clone();
        let held = self.held_by(item);
        match self
            .owner
            .mark_exported_replaced_held(&txid, held, |b| write_queue_file(&dir, b))
        {
            Ok(()) => {
                self.note(format!(
                    "{txid} was given to the wallet and another transaction spent its coins; \
                     its entries are back in the queue"
                ));
                if self.undone_for_item(&txid) {
                    self.forget_owed(item);
                }
            }
            Err(e) => self.note(format!("{txid} could not be released: {e}")),
        }
    }

    fn settle_elsewhere_paid(&mut self, item: &Owed) {
        let (txid, block) = (item.txid, item.block);
        if !self.owner.signed_elsewhere().iter().any(|e| e.txid == txid) {
            return self.forget_owed(item);
        }
        if self.ready_for_queue_write().is_err() {
            self.note(format!("{txid} waits for key 1 recovery"));
            return;
        }
        let dir = self.dir.clone();
        if let Err(e) = self.log_paid_elsewhere(&txid, item.tx.as_ref()) {
            self.note(format!(
                "{txid} is in block {block} but could not be logged: {e}"
            ));
            return;
        }
        if !self.keep_confirmed(item) {
            return;
        }
        match self
            .owner
            .mark_elsewhere_confirmed(&txid, |b| write_queue_file(&dir, b))
        {
            Ok(dropped) => {
                self.note(format!(
                    "{txid} was signed elsewhere and is in block {block}: its entries are paid"
                ));
                self.forget_owed(item);
                for gone in dropped {
                    self.unsend(&gone);
                    self.states.insert(gone, SendState::Replaced);
                    self.relays.remove(&gone);
                    if let Err(e) = self
                        .view
                        .drop_pending(&gone, |b| write_file(&dir, COINS_FILE, b))
                    {
                        self.stop(format!("the coin view could not be stored: {e}"));
                        return;
                    }
                }
            }
            Err(e) => self.note(format!("{txid} could not be marked paid: {e}")),
        }
    }

    fn settle_elsewhere_beaten(&mut self, item: &Owed) {
        let txid = item.txid;
        let Some(old) = self
            .owner
            .signed_elsewhere()
            .iter()
            .find(|e| e.txid == txid)
            .map(|e| e.held)
        else {
            if self.undone_for_item(&txid) {
                self.forget_owed(item);
            }
            return;
        };
        if self.ready_for_queue_write().is_err() {
            self.note(format!("{txid} waits for key 1 recovery"));
            return;
        }
        let dir = self.dir.clone();
        // OC-55: held until the block that beat it is deep. One that is held
        // already takes a later block only when its own has left the chain.
        let old_gone = old.is_some_and(|h| {
            self.watcher
                .chain()
                .height_of(&BlockHash::from_byte_array(h.block))
                .is_none()
        });
        let held = self.held_by(item);
        match self
            .owner
            .hold_elsewhere(&txid, held, old_gone, |b| write_queue_file(&dir, b))
        {
            Ok(()) => {
                self.note(format!(
                    "{txid} was signed elsewhere and another transaction spent its coins; its \
                     entries are held until block {} is more than {UNDO_BLOCKS} deep",
                    item.block
                ));
                if self.undone_for_item(&txid) {
                    self.forget_owed(item);
                }
            }
            Err(e) => self.note(format!("{txid} could not be released: {e}")),
        }
    }

    /// OC-55: the block that beat a transaction, as the queue keeps it.
    fn held_by(&self, item: &Owed) -> Held {
        Held {
            by: item.by.unwrap_or_else(Txid::all_zeros),
            block: item.block.to_byte_array(),
            height: self.watcher.chain().height_of(&item.block).unwrap_or(0),
        }
    }

    /// OC-55: a transaction that was beaten is no longer watched, and its
    /// entries are no longer held, when the block that beat it is more than
    /// [`UNDO_BLOCKS`] deep on the best chain: at [`UNDO_BLOCKS`] the coin
    /// view still holds its undo and a reorganization can take it. One whose
    /// block has left stays held and watched. Idempotent: a release that was stored leaves nothing to release.
    fn release_holds(&mut self) {
        if self.halted.is_some() {
            return;
        }
        let ready: Vec<Txid> = self
            .owner
            .signed_elsewhere()
            .iter()
            .filter(|e| {
                e.held.is_some_and(|h| {
                    openbmt_watch::chain::confirmations(
                        self.watcher.chain(),
                        &BlockHash::from_byte_array(h.block),
                    )
                    .is_some_and(|n| n > UNDO_BLOCKS)
                })
            })
            .map(|e| e.txid)
            .collect();
        for txid in ready {
            if self.ready_for_queue_write().is_err() {
                self.note(format!("{txid} release waits for key 1 recovery"));
                return;
            }
            let dir = self.dir.clone();
            match self
                .owner
                .release_held(&txid, |b| write_queue_file(&dir, b))
            {
                Ok(()) => self.note(format!(
                    "{txid} is no longer watched: the block that beat it is more than {UNDO_BLOCKS} deep; \
                     its entries are back in the queue"
                )),
                Err(e) => self.note(format!("{txid} could not be released: {e}")),
            }
        }
    }

    /// OC-55: the entries that are held, with the blocks left until the block
    /// that beat their payment is more than [`UNDO_BLOCKS`] deep: 0 at the
    /// tip where they are released.
    pub fn held(&self) -> Vec<(openbmt_owner::HeldEntry, u32)> {
        self.owner
            .held_entries()
            .into_iter()
            .map(|h| {
                let deep = openbmt_watch::chain::confirmations(
                    self.watcher.chain(),
                    &BlockHash::from_byte_array(h.block),
                )
                .unwrap_or(0);
                (h, (UNDO_BLOCKS + 1).saturating_sub(deep))
            })
            .collect()
    }

    /// OC-52: another transaction spent a coin of one in the owner's own
    /// outbox. It leaves the outbox and its entries go back to the queue for
    /// a new approval; the item leaves the list once that is stored.
    fn settle_outbox_beaten(&mut self, item: &Owed) {
        let txid = item.txid;
        let waiting = self
            .owner
            .outbox()
            .iter()
            .any(|d| matches!(&d.what, Delivery::Transaction { txid: t, .. } if *t == txid));
        if !waiting {
            // The queue was told in an earlier try; the log may not be.
            if self.undone_for_item(&txid) {
                self.forget_owed(item);
            }
            return;
        }
        if self.ready_for_queue_write().is_err() {
            self.note(format!("{txid} replacement waits for key 1 recovery"));
            return;
        }
        let dir = self.dir.clone();
        let held = self.held_by(item);
        match self
            .owner
            .mark_replaced_held(&txid, held, |b| write_queue_file(&dir, b))
        {
            Ok(()) => {
                self.note(format!(
                    "{txid} was replaced: another transaction spent its coins; its entries are \
                     held until block {} is more than {UNDO_BLOCKS} deep, and nothing is signed again by \
                     itself",
                    item.block
                ));
                self.unsend(&txid);
                self.states.insert(txid, SendState::Replaced);
                self.relays.remove(&txid);
                if let Err(e) = self
                    .view
                    .drop_pending(&txid, |b| write_file(&dir, COINS_FILE, b))
                {
                    self.stop(format!("the coin view could not be stored: {e}"));
                    return;
                }
                if self.undone_for_item(&txid) {
                    self.forget_owed(item);
                }
            }
            Err(e) => self.note(format!("{txid} was replaced but could not be marked: {e}")),
        }
    }

    fn replaced(&mut self, txid: Txid) {
        if self.ready_for_queue_write().is_err() {
            self.note(format!("{txid} replacement waits for key 1 recovery"));
            return;
        }
        let dir = self.dir.clone();
        match self
            .owner
            .mark_replaced(&txid, |b| write_queue_file(&dir, b))
        {
            Ok(()) => {
                self.note(format!(
                    "{txid} was replaced: another transaction spent its coins; its entries are \
                     back in the queue and nothing is signed again by itself"
                ));
                self.undone_for_item(&txid);
            }
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
        self.deep_tick();
        self.settle();
        self.settle_owed();
        self.release_holds();
        self.requests_tick();
    }

    /// Stop the drivers.
    pub fn shutdown(&mut self) {
        self.cancel_deep("the node is stopping");
        if let Some(sender) = &mut self.sender {
            sender.shutdown();
        }
        self.watcher.shutdown();
    }

    fn handle(&mut self, event: &AppEvent) {
        match event {
            AppEvent::Chain { left, .. } if !left.is_empty() => self.left_the_chain(left),
            AppEvent::Block { hash, bytes } => self.block(hash, bytes),
            AppEvent::FeeBlock { hash, bytes } => self.fee_block(hash, bytes),
            AppEvent::Checked(height) => self.checked(*height),
            AppEvent::RescanStarted(_) => {
                self.awaiting_rescan = self.awaiting_rescan.saturating_sub(1)
            }
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
                    Some(
                        SendState::Sent
                            | SendState::SeenBack { .. }
                            | SendState::NotSeen
                            | SendState::HeldByNode { .. }
                    )
                );
                if live {
                    let mapped = match state {
                        RelayState::Sent => SendState::Sent,
                        RelayState::SeenBack { groups } => SendState::SeenBack { groups: *groups },
                        RelayState::HeldByNode => SendState::HeldByNode { groups: 0 },
                        RelayState::SeenBackAndHeldByNode { groups } => {
                            SendState::HeldByNode { groups: *groups }
                        }
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
        // OC-24: a deep check that read the chain before it changed starts
        // again when the view has caught up.
        self.cancel_deep("the chain changed while it ran");
        self.forget_owed_blocks(left);
        self.refresh_marks();
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
        // FE-20: the samples of the blocks that left go with them.
        self.forget_fee_samples_from(fork.saturating_add(1), left);
        let dir = self.dir.clone();
        let mut notes = Vec::new();
        for f in &mut self.former {
            if let Err(e) = f
                .view
                .reorganize(left, fork, |b| write_file(&dir, &f.file, b))
            {
                notes.push(format!("a former wallet's view could not be stored: {e}"));
            }
        }
        for line in notes {
            self.note(line);
        }
        match self
            .view
            .reorganize(left, fork, |b| write_file(&dir, COINS_FILE, b))
        {
            Ok(effect) => {
                // RQ-37: the requests undo the same blocks, before the facts
                // of the queued refunds are worked out again.
                self.requests_reorg(fork);
                self.after_reorg(effect);
            }
            Err(e) => self.stop(format!("the coin view could not be stored: {e}")),
        }
    }

    /// What follows the coin view's reorganization: the facts of the queued
    /// refunds, and the transactions the blocks that left had confirmed.
    fn after_reorg(&mut self, effect: ReorgEffect) {
        if self.view.state() == ViewState::DeepReorg {
            self.note("deep reorg: rescan".to_string());
        }
        // OC-47: the facts of every queued refund follow the receipts
        // that left, in the same step as the coin view.
        match self.ready_for_queue_write() {
            Ok(()) => {
                let dir = self.dir.clone();
                let tip = self.view.checked();
                if let Err(e) = self.refresh_refund_facts(tip, &mut |b| write_queue_file(&dir, b)) {
                    self.stop(format!("the refunds' facts could not be stored: {e}"));
                }
            }
            Err(e) => self.note(format!("the refunds' facts wait to be refreshed: {e}")),
        }
        for txid in effect.reopened {
            let tx = self
                .view
                .pending()
                .find(|p| p.tx.compute_txid() == txid)
                .map(|p| p.tx.clone());
            let Some(tx) = tx else {
                self.stop(format!(
                    "{txid} left the chain but its signed transaction is missing"
                ));
                continue;
            };
            if self.take_recovery_pending() {
                self.note(format!(
                    "{txid} waits to reopen after the owner's key 1 state recovery"
                ));
                continue;
            }
            if let Err(e) = self.ready_for_queue_write() {
                self.note(format!("{txid} waits to reopen: {e}"));
                continue;
            }
            let dir = self.dir.clone();
            match self.owner.reopen_paid(&tx, |b| write_queue_file(&dir, b)) {
                Ok(had_rows) => {
                    if had_rows {
                        self.note(format!(
                            "{txid} left the chain; its paid entries are open again"
                        ));
                    } else {
                        self.note(format!(
                            "{txid} left the chain; its signed transaction is sent again"
                        ));
                    }
                    self.dispatch(txid, &bitcoin::consensus::serialize(&tx));
                }
                Err(e) => self.stop(format!(
                    "{txid} left the chain but its paid entries could not be reopened: {e}"
                )),
            }
        }
    }

    /// OC-47: work out again, from the coin view as it is, the facts of
    /// every sale that has a refund in the queue, manual or from intake. A
    /// sale the view cannot give facts for keeps its amount and no longer
    /// counts as at the refund depth. Each change is stored before it is
    /// swapped in.
    fn refresh_refund_facts<E: std::fmt::Display>(
        &mut self,
        tip: u32,
        persist: &mut impl FnMut(&[u8]) -> Result<(), E>,
    ) -> Result<(), String> {
        let mut sales: Vec<openbmt_owner::SaleKey> =
            self.owner.refunds().iter().map(|r| r.sale).collect();
        sales.sort();
        sales.dedup();
        for sale in sales {
            let held = self.owner.sale_facts(&sale);
            // RQ-26: a return of a request is a refund of the desk's sale;
            // what it received is what the book credited at the depth.
            let fresh = match self.desk_facts(&sale) {
                Some(facts) => facts,
                None => match crate::facts::verified_facts(
                    &self.view,
                    &self.owner,
                    sale,
                    tip,
                    self.refund_depth,
                ) {
                    Ok(facts) => facts,
                    Err(_) => openbmt_owner::SaleFacts {
                        received_sats: held.map_or(0, |f| f.received_sats),
                        reached_refund_depth: false,
                    },
                },
            };
            if held == Some(fresh) {
                continue;
            }
            self.owner
                .set_sale_facts(sale, fresh, &mut *persist)
                .map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    fn block(&mut self, hash: &bitcoin::BlockHash, bytes: &[u8]) {
        if self.halted.is_some() || self.awaiting_rescan > 0 {
            return;
        }
        // The marks first: a script the queue handed out is read in this
        // block (OC-07).
        self.refresh_marks();
        // A node the marks stopped takes nothing of the block: neither the
        // owed list nor the coin view is touched.
        if self.halted.is_some() {
            return;
        }
        let block = match WellFormedBlock::parse(bytes) {
            Ok(b) if b.hash() == *hash => b,
            Ok(_) => return self.stop(format!("a block did not have the hash {hash} it came as")),
            Err(e) => return self.stop(format!("block {hash} was refused: {e}")),
        };
        // OC-52: what the block owes is written down before the coin view
        // takes it, so a restart that finds it not taken processes it again.
        let owed = self.owed_in(hash, bytes);
        if !owed.is_empty() {
            let stored = self
                .owed
                .merged(&owed)
                .map_err(|e| e.to_string())
                .and_then(|next| self.store_owed(next));
            if let Err(e) = stored {
                return self.stop(format!(
                    "block {hash} owes settlements that could not be stored: {e}"
                ));
            }
        }
        let dir = self.dir.clone();
        match self
            .view
            .apply_block(&block, self.watcher.chain(), &self.scripts, |b| {
                write_file(&dir, COINS_FILE, b)
            }) {
            Ok(effect) => {
                self.apply_former(&block, hash);
                // RQ-15: the payments to requests are found in the block the
                // view applied, and credited at the confirmation depth.
                self.requests_arrivals(hash, bytes);
                // FE-20: a block downloaded in full and checked gives a
                // fee sample.
                self.take_fee_sample(hash, bytes);
                for txid in effect.confirmed {
                    self.note(format!("{txid} is in block {hash}"));
                }
                for txid in effect.replaced {
                    // One in the outbox is settled from the owed list.
                    let owed = self
                        .owed
                        .items()
                        .iter()
                        .any(|o| o.kind == OwedKind::OutboxBeaten && o.txid == txid);
                    if !owed {
                        self.replaced(txid);
                    }
                }
                self.settle_owed();
            }
            // A block that is not on the best chain is not the wallet's
            // business: the driver says it joined before it hands it over, so
            // this is a block that has left since.
            Err(ApplyError::Refused(Refusal::NotOnChain)) => {
                self.note(format!(
                    "block {hash} is not on the best chain; passed over"
                ));
                self.forget_owed_blocks(&[*hash]);
            }
            Err(ApplyError::Refused(r)) => self.stop(format!("block {hash} was refused: {r}")),
            Err(e) => self.stop(format!("block {hash} could not be applied: {e}")),
        }
    }

    /// SS-43: the block goes to the view of each former wallet too. One that
    /// cannot take it is noted and does not stop the node: its wallet is not
    /// the one in force.
    fn apply_former(&mut self, block: &WellFormedBlock, hash: &BlockHash) {
        let dir = self.dir.clone();
        let mut notes = Vec::new();
        for f in &mut self.former {
            match f
                .view
                .apply_block(block, self.watcher.chain(), &f.scripts, |b| {
                    write_file(&dir, &f.file, b)
                }) {
                Ok(effect) => {
                    for txid in effect.confirmed {
                        notes.push(format!("{txid} is in block {hash}"));
                    }
                }
                Err(ApplyError::Refused(Refusal::NotOnChain)) => {}
                Err(e) => notes.push(format!("a former wallet could not take block {hash}: {e}")),
            }
        }
        for line in notes {
            self.note(line);
        }
    }

    fn checked(&mut self, height: u32) {
        if self.halted.is_some() || self.awaiting_rescan > 0 {
            return;
        }
        let tip = self.watcher.chain().tip_height();
        let caught_up = matches!(self.view.state(), ViewState::Rescanning { to } if height >= to);
        if height >= tip || caught_up || height.saturating_sub(self.view.checked()) >= CHECKED_EVERY
        {
            let dir = self.dir.clone();
            let hash = self.watcher.chain().hash_at(height);
            if let Err(e) = self
                .view
                .set_checked_at(height, hash, |b| write_file(&dir, COINS_FILE, b))
            {
                self.stop(format!("the coin view could not be stored: {e}"));
            }
        }
        let mut notes = Vec::new();
        let dir = self.dir.clone();
        for f in &mut self.former {
            let caught = matches!(f.view.state(), ViewState::Rescanning { to } if height >= to);
            if height >= tip || caught || height.saturating_sub(f.view.checked()) >= CHECKED_EVERY {
                let hash = self.watcher.chain().hash_at(height);
                if let Err(e) = f
                    .view
                    .set_checked_at(height, hash, |b| write_file(&dir, &f.file, b))
                {
                    notes.push(format!("a former wallet's view could not be stored: {e}"));
                }
            }
        }
        for line in notes {
            self.note(line);
        }
        // FE-20: when the node is at the tip and has taken no sample in the
        // last 24 checked blocks, it asks for one more full block.
        self.ask_for_fee_block(height);
    }
}

#[cfg(test)]
mod watch_budget_tests {
    use super::{ensure_marks_fit_watch_budget, NodeError};
    use openbmt_net_io::MAX_WATCHED_SCRIPTS;

    #[test]
    fn watch_ranges_fit_exactly_at_the_driver_limit() {
        assert_eq!(
            ensure_marks_fit_watch_budget((MAX_WATCHED_SCRIPTS - 40) as u32, 0, 0),
            Ok(())
        );
        assert_eq!(
            ensure_marks_fit_watch_budget((MAX_WATCHED_SCRIPTS - 39) as u32, 0, 0),
            Err(NodeError::WatchLimit {
                needed: (MAX_WATCHED_SCRIPTS + 1) as u64,
                limit: MAX_WATCHED_SCRIPTS,
            })
        );
    }

    #[test]
    fn scripts_set_aside_make_room_under_the_limit() {
        assert_eq!(
            ensure_marks_fit_watch_budget((MAX_WATCHED_SCRIPTS - 39) as u32, 0, 1),
            Ok(())
        );
        assert_eq!(ensure_marks_fit_watch_budget(500_000, 5, 410_000), Ok(()));
        assert!(ensure_marks_fit_watch_budget(500_000, 5, 399_000).is_err());
    }
}
