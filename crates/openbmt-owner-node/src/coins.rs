//! The wallet's coins, as the owner's device learns them from checked blocks.
//!
//! [`CoinView`] holds the unspent outputs that pay the wallet's scripts, with
//! the block each one is in, and gives them to `Owner::build_request` as
//! `openbmt_spend::Coin`s with their confirmations counted from the tip.
//! It has no network, no clock and no files: the caller hands it a block
//! that has been parsed and checked ([`WellFormedBlock`]) and the best
//! chain, and a `persist` step that stores the new value before it takes
//! effect, as the owner's queue does.
//!
//! ## What is a coin
//!
//! - A coin is learned only from a block that passed the checks the counter
//!   uses: the block is on the best chain (WR-01), well formed (WR-20 to
//!   WR-22) and the payment is proven by the block's own merkle root. The
//!   node hands blocks here only after the driver's filter checks (two
//!   groups, or the full-block fallback, NR-40 to NR-45). A block that fails
//!   is refused and changes nothing.
//! - An unconfirmed transaction from a peer is never a coin. The one
//!   exception is the change of a transaction the owner's own device
//!   finished ([`CoinView::add_pending`]): it is listed with 0
//!   confirmations, and `build_request` decides by its `min_confirmations`
//!   (SP-10) whether to spend it.
//! - A coin that an input of a checked block spends is removed. A coin
//!   spent by a transaction the owner holds ([`CoinView::pending_out`]) is
//!   not offered by [`CoinView::coins`] until that transaction is replaced.
//!
//! ## Reorganizations
//!
//! Each block that changed the view keeps an undo entry for the last
//! [`UNDO_BLOCKS`] blocks: the coins it spent and the held transactions it
//! confirmed. When blocks leave the best chain ([`CoinView::reorganize`])
//! what they spent is put back, what they created is removed, and the
//! transactions they confirmed are held again. A reorganization of more
//! than [`UNDO_BLOCKS`] blocks, or of a block that changed the view and has
//! no undo entry, makes the view's state [`ViewState::DeepReorg`]
//! ("deep reorg: rescan"): no coin is offered until [`CoinView::reset`]
//! starts a rescan from the birthday height. It is never silent.
//!
//! ## Receipts
//!
//! Every output that pays a receive script in a checked block is also kept as
//! a [`Receipt`], and a receipt stays when the coin is spent. Receipts are
//! what the owner's own figure of "what a sale received" is made from
//! (OC-12 to OC-14, `crate::facts`). A reorganization removes the receipts of
//! the blocks that left with the same undo records as the coins. At most
//! [`MAX_RECEIPTS`] are kept. A receipt may leave the log (OC-28) when its
//! coin is spent and its block is more than [`RECEIPT_KEEP_BLOCKS`] below
//! the tip; the log is pruned, oldest block first, when it passes 90% of the
//! cap. The receive indices that lost a receipt are kept as a bounded list
//! of ranges (OC-29), and a refund for one of them is refused as too old
//! instead of being answered with a wrong zero. When nothing can be pruned
//! and the cap is reached, the view takes the named state "receipt log
//! full" ([`ReceiptState::LogFull`]): no more receipts are recorded and no
//! facts are given, while the coins are still tracked and the owner can
//! still spend.
//!
//! ## What is watched
//!
//! Receive indices from 0 to the highest a signed batch handed out plus
//! [`GAP`], change indices from 0 to the next change index plus [`GAP`]. The
//! two marks are kept here ([`CoinView::raise_marks_and_rescan`]). At runtime,
//! growing them stores a rewind to one block below the checked position. The
//! driver rechecks that short range against the new scripts, covering work
//! already in flight without rescanning from the birthday. A new view,
//! [`CoinView::reset`] and the deep reorg state scan from the birthday.
//!
//! ## Scripts set aside
//!
//! When the wallet's scripts would not fit the live watch, old quiet
//! receive scripts are set aside (OC-22, OC-23, `crate::live`). The view
//! keeps their indices as runs ([`Spans`]) and the schedule of the deep
//! check that looks at them (OC-24 to OC-26): when the last one finished,
//! when the next is due, whether the last one failed, and the height above
//! which they have not been checked. They are stored in the same bytes as
//! the coins, so a restart sees both or neither. The view has no clock: the
//! caller gives it the times.
//!
//! ## The stored form
//!
//! `OBMe`, version 1, strict like the other stored forms: sorted lists, no
//! repeats, pruned index ranges sorted and apart, every length checked before anything is reserved, every coin
//! must be an output of the transaction it carries, and, unless the receipt
//! log is full, every unspent receive coin has its receipt. The chain position
//! (`checked`) is in the same bytes as the coins it reflects, so a restart
//! sees both or neither.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use bitcoin::consensus::{deserialize, serialize};
use bitcoin::hashes::Hash;
use bitcoin::{BlockHash, OutPoint, ScriptBuf, Transaction, Txid};
use openbmt_core::wallet::Keychain;
use openbmt_core::wire::{self, DecodeError, Reader};
use openbmt_spend::Coin;
use openbmt_watch::{HeaderChain, Refusal, WellFormedBlock};

use crate::live::{Spans, MAX_RELEASED_SPANS};

/// The magic of the stored coin view: `OBMe`.
pub const MAGIC_OWNER_COINS: [u8; 4] = *b"OBMe";
/// Unused indices watched beyond the highest one in use.
pub const GAP: u32 = 20;
/// Blocks of undo entries kept.
pub const UNDO_BLOCKS: u32 = 100;
/// Most coins held.
pub const MAX_COINS: usize = 20_000;
/// OC-13: most receipts kept.
pub const MAX_RECEIPTS: usize = 262_144;
/// OC-28: pruning starts when more receipts than this are kept (90%).
pub const PRUNE_START: usize = MAX_RECEIPTS / 10 * 9;
/// OC-28: pruning frees receipts down to this many (80%).
pub const PRUNE_TO: usize = MAX_RECEIPTS / 10 * 8;
/// OC-28: a receipt of a spent coin may leave the log when its block is more
/// than this many blocks below the tip: 400 days at 144 blocks a day, 57,600
/// **[to tune]**. It is longer than any refund window a shop is likely to
/// honour. Block time is not kept per receipt (the header chain gives only
/// the tip's), so the age is counted in blocks.
pub const RECEIPT_KEEP_BLOCKS: u32 = 57_600;
/// OC-29: most ranges of pruned receive indices kept.
pub const MAX_PRUNED_RANGES: usize = 1024;
/// Most held transactions kept as pending.
pub const MAX_PENDING: usize = 64;
/// The largest transaction a stored coin may carry, in bytes.
pub const MAX_TX_BYTES: usize = 400_000;
/// The largest stored view, in bytes.
pub const MAX_VIEW_BYTES: usize = 128 * 1024 * 1024;
/// The largest wallet index (the wallet takes unhardened indices only).
pub(crate) const INDEX_LIMIT: u32 = 1 << 31;

/// A script the wallet watches, with its place in the wallet.
pub type ScriptSet = BTreeMap<ScriptBuf, (Keychain, u32)>;

/// What the view can say about itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewState {
    /// The coins are the wallet's, as far as the checked chain shows.
    Ready,
    /// Blocks below the old position are being checked again for scripts
    /// that were added. The view is complete from this height up only;
    /// below `to` it is still catching up.
    Rescanning {
        /// The position the view had before the rescan began.
        to: u32,
    },
    /// "Deep reorg: rescan". A reorganization went further back than the
    /// view can undo. Nothing is offered until a rescan has been started.
    DeepReorg,
}

impl fmt::Display for ViewState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Ready => write!(f, "ready"),
            Self::Rescanning { to } => write!(f, "rescanning up to block {to}"),
            Self::DeepReorg => write!(f, "deep reorg: rescan"),
        }
    }
}

/// Whether the view holds every receipt it has seen (OC-13).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReceiptState {
    /// Every receipt seen since the last rescan is kept.
    Complete,
    /// "Receipt log full". [`MAX_RECEIPTS`] are kept and no more are
    /// recorded, so no sale's facts are given. Coins are still tracked.
    LogFull,
}

impl fmt::Display for ReceiptState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Complete => write!(f, "complete"),
            Self::LogFull => write!(f, "receipt log full"),
        }
    }
}

/// A payment to one of the wallet's receive scripts, in a checked block. It
/// stays after the coin is spent (OC-12).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Receipt {
    /// The receive index of the script paid.
    pub index: u32,
    /// The output.
    pub outpoint: OutPoint,
    /// The amount in satoshis.
    pub sats: u64,
    /// The height of the block that holds it.
    pub height: u32,
    /// That block.
    pub block: BlockHash,
}

/// An output of the wallet that is unspent as far as the view knows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnedCoin {
    /// The outpoint.
    pub outpoint: OutPoint,
    /// The whole transaction that made it (SP-01 asks for all of it).
    pub tx: Transaction,
    /// The keychain of its script.
    pub chain: Keychain,
    /// The index of its script.
    pub index: u32,
    /// The height of the block that holds it.
    pub height: u32,
    /// That block.
    pub block: BlockHash,
}

impl OwnedCoin {
    /// The amount in satoshis, 0 if the output is missing (a stored coin
    /// always has its output).
    pub fn sats(&self) -> u64 {
        usize::try_from(self.outpoint.vout)
            .ok()
            .and_then(|v| self.tx.output.get(v))
            .map_or(0, |o| o.value.to_sat())
    }

    fn as_coin(&self, confirmations: u32) -> Coin {
        Coin {
            outpoint: self.outpoint,
            prev_tx: self.tx.clone(),
            vout: self.outpoint.vout,
            chain: self.chain,
            index: self.index,
            confirmations,
        }
    }
}

/// A transaction of the owner's own that has not been seen in a block, and
/// the outputs of it that pay the wallet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingTx {
    /// The transaction.
    pub tx: Transaction,
    /// The outputs that pay the wallet: the output index, keychain and
    /// index.
    pub own: Vec<(u32, Keychain, u32)>,
}

/// What a block did to the view when it was applied.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BlockEffect {
    /// Coins the block created.
    pub created: Vec<OutPoint>,
    /// Coins the block spent.
    pub spent: Vec<OutPoint>,
    /// What spent each of them: the ID of the transaction in the block. The
    /// node compares it with the transactions it was given only the ID of
    /// (AS-89).
    pub spent_by: Vec<(OutPoint, Txid)>,
    /// Held transactions the block confirmed.
    pub confirmed: Vec<Txid>,
    /// Held transactions whose input the block spent in another
    /// transaction: they can no longer confirm.
    pub replaced: Vec<Txid>,
}

/// What a reorganization did to the view.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReorgEffect {
    /// Coins put back.
    pub restored: usize,
    /// Coins removed.
    pub removed: usize,
    /// Held transactions that were confirmed by a block that left, and are
    /// held again.
    pub reopened: Vec<Txid>,
}

/// Why a block was not applied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApplyError<E> {
    /// The block failed the watcher's checks. Nothing changed.
    Refused(Refusal),
    /// More coins than the view holds.
    Full,
    /// The new view could not be stored. Nothing changed.
    Store(E),
}

impl<E: fmt::Display> fmt::Display for ApplyError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Refused(r) => write!(f, "{r}"),
            Self::Full => write!(f, "the coin view holds {MAX_COINS} coins already"),
            Self::Store(e) => write!(f, "the coin view could not be stored: {e}"),
        }
    }
}

/// Undo for one block that changed the view.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Undo {
    block: BlockHash,
    spent: Vec<OwnedCoin>,
    confirmed: Vec<PendingTx>,
}

/// The wallet's coins. See the module docs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoinView {
    birthday: u32,
    checked: u32,
    receive_next: u32,
    change_next: u32,
    state: ViewState,
    coins: BTreeMap<OutPoint, OwnedCoin>,
    undo: BTreeMap<u32, Undo>,
    pending: BTreeMap<Txid, PendingTx>,
    /// OC-12: the receipts by receive index, each list ascending by
    /// outpoint.
    receipts: BTreeMap<u32, Vec<Receipt>>,
    receipt_total: usize,
    receipt_log_full: bool,
    /// OC-29: the receive indices that lost a receipt to pruning, as ranges
    /// (start to inclusive end), sorted, apart, at most
    /// [`MAX_PRUNED_RANGES`].
    pruned: BTreeMap<u32, u32>,
    /// OC-22: receive indices set aside from the live watch.
    released: Spans,
    /// OC-24 to OC-26: the schedule of the deep check.
    deep: DeepSchedule,
}

/// OC-24 to OC-26: when the scripts that were set aside are checked.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DeepSchedule {
    /// When the last deep check finished, on the owner's clock; 0 if none
    /// has.
    pub last: u64,
    /// When the next one is due; 0 while nothing is set aside. A time at or
    /// before the clock's is due now.
    pub due: u64,
    /// Whether the last attempt did not finish.
    pub failed: bool,
    /// The height above which the scripts set aside have not been checked.
    /// 0 while nothing is set aside.
    pub from: u32,
}

impl CoinView {
    /// A view with no coins that reflects the chain up to `checked`, which
    /// is where a wallet that has had no payment before `birthday` starts.
    /// `birthday` is the height at or below which the wallet received
    /// nothing; blocks above it are checked.
    pub fn new(birthday: u32) -> Self {
        Self {
            birthday,
            checked: birthday,
            receive_next: 0,
            change_next: 0,
            state: ViewState::Ready,
            coins: BTreeMap::new(),
            undo: BTreeMap::new(),
            pending: BTreeMap::new(),
            receipts: BTreeMap::new(),
            receipt_total: 0,
            receipt_log_full: false,
            pruned: BTreeMap::new(),
            released: Spans::new(),
            deep: DeepSchedule::default(),
        }
    }

    // ---- reading -------------------------------------------------------------

    /// The height the wallet's first receipt is above.
    pub fn birthday(&self) -> u32 {
        self.birthday
    }

    /// The chain position the view reflects: every block above the birthday
    /// up to here has been checked for the scripts watched.
    pub fn checked(&self) -> u32 {
        self.checked
    }

    /// The highest receive index handed out, plus one.
    pub fn receive_next(&self) -> u32 {
        self.receive_next
    }

    /// The next change index.
    pub fn change_next(&self) -> u32 {
        self.change_next
    }

    /// Receive indices watched: 0 up to the highest handed out plus
    /// [`GAP`], excluded.
    pub fn receive_watch(&self) -> u32 {
        self.receive_next.saturating_add(GAP).min(INDEX_LIMIT)
    }

    /// Change indices watched, the same way.
    pub fn change_watch(&self) -> u32 {
        self.change_next.saturating_add(GAP).min(INDEX_LIMIT)
    }

    /// OC-22: the receive indices set aside from the live watch.
    pub fn released(&self) -> &Spans {
        &self.released
    }

    /// OC-24 to OC-26: the schedule of the deep check.
    pub fn deep(&self) -> DeepSchedule {
        self.deep
    }

    /// OC-22: how many scripts the live watch holds: the receive scripts
    /// watched that are not set aside, and the change scripts watched.
    pub fn live_count(&self) -> u64 {
        let receive = u64::from(self.receive_watch());
        let aside = self.released.count_below(self.receive_watch());
        receive
            .saturating_sub(aside)
            .saturating_add(u64::from(self.change_watch()))
    }

    /// What the view can say about itself.
    pub fn state(&self) -> ViewState {
        self.state
    }

    /// Whether the coins can be offered to a spend.
    pub fn ready(&self) -> bool {
        self.state == ViewState::Ready
    }

    /// OC-12: the receipts at a receive index, lowest outpoint first. A
    /// receipt is kept after its coin is spent.
    pub fn receipts_at(&self, index: u32) -> &[Receipt] {
        self.receipts.get(&index).map_or(&[], Vec::as_slice)
    }

    /// OA-113: the receive indices a payment was seen at: those with a
    /// receipt kept, those whose receipt was pruned (OC-29) and those that
    /// hold an unspent or unconfirmed-spend coin.
    pub fn paid_receive_indexes(&self) -> BTreeSet<u32> {
        let mut paid: BTreeSet<u32> = self
            .receipts
            .iter()
            .filter(|(_, kept)| !kept.is_empty())
            .map(|(index, _)| *index)
            .collect();
        for (start, end) in &self.pruned {
            paid.extend(*start..=*end);
        }
        paid.extend(
            self.owned()
                .filter(|c| c.chain == Keychain::Receive)
                .map(|c| c.index),
        );
        paid
    }

    /// How many receipts are kept.
    pub fn receipt_count(&self) -> usize {
        self.receipt_total
    }

    /// OC-13: whether every receipt seen is kept.
    pub fn receipt_state(&self) -> ReceiptState {
        if self.receipt_log_full {
            ReceiptState::LogFull
        } else {
            ReceiptState::Complete
        }
    }

    /// OC-29: whether a receipt at this receive index was pruned, so that
    /// the receipts kept for it may not be all it received.
    pub fn is_pruned(&self, index: u32) -> bool {
        self.pruned
            .range(..=index)
            .next_back()
            .is_some_and(|(_, end)| *end >= index)
    }

    /// OC-29: how many ranges of pruned indices are kept.
    pub fn pruned_ranges(&self) -> usize {
        self.pruned.len()
    }

    /// The confirmed coins held, lowest outpoint first.
    pub fn owned(&self) -> impl Iterator<Item = &OwnedCoin> {
        self.coins.values()
    }

    /// How many confirmed coins are held.
    pub fn len(&self) -> usize {
        self.coins.len()
    }

    /// Whether no confirmed coin is held.
    pub fn is_empty(&self) -> bool {
        self.coins.is_empty()
    }

    /// The transactions of the owner's own that are held as pending.
    pub fn pending(&self) -> impl Iterator<Item = &PendingTx> {
        self.pending.values()
    }

    /// The coins that a pending transaction spends: they are not offered.
    pub fn pending_out(&self) -> BTreeSet<OutPoint> {
        self.pending
            .values()
            .flat_map(|p| p.tx.input.iter().map(|i| i.previous_output))
            .filter(|op| self.coins.contains_key(op))
            .collect()
    }

    /// What the confirmed coins add up to, in satoshis, the ones a pending
    /// transaction spends included.
    pub fn balance(&self) -> u64 {
        self.coins
            .values()
            .fold(0u64, |sum, c| sum.saturating_add(c.sats()))
    }

    /// What the coins that [`CoinView::coins`] offers add up to, change of
    /// pending transactions included.
    pub fn spendable(&self, tip: u32) -> u64 {
        self.coins(tip).iter().fold(0u64, |sum, c| {
            sum.saturating_add(c.output().map_or(0, |o| o.value.to_sat()))
        })
    }

    /// The coins for `Owner::build_request`: the confirmed ones that no
    /// pending transaction spends, with their confirmations counted from
    /// `tip` (a coin in the tip block has 1), and the change of pending
    /// transactions with 0. Empty unless the view is ready.
    pub fn coins(&self, tip: u32) -> Vec<Coin> {
        if !self.ready() {
            return Vec::new();
        }
        let out = self.pending_out();
        let mut list: Vec<Coin> = self
            .coins
            .values()
            .filter(|c| !out.contains(&c.outpoint))
            .map(|c| {
                let depth = if c.height <= tip {
                    tip - c.height + 1
                } else {
                    0
                };
                c.as_coin(depth)
            })
            .collect();
        for p in self.pending.values() {
            let txid = p.tx.compute_txid();
            for (vout, chain, index) in &p.own {
                let outpoint = OutPoint::new(txid, *vout);
                if self.coins.contains_key(&outpoint) {
                    continue;
                }
                list.push(Coin {
                    outpoint,
                    prev_tx: p.tx.clone(),
                    vout: *vout,
                    chain: *chain,
                    index: *index,
                    confirmations: 0,
                });
            }
        }
        list
    }

    /// The height of the block that confirmed a transaction the owner held,
    /// if that block is still among the last [`UNDO_BLOCKS`] that changed
    /// the view.
    pub fn confirmed_height(&self, txid: &Txid) -> Option<u32> {
        self.undo
            .iter()
            .find(|(_, u)| u.confirmed.iter().any(|p| p.tx.compute_txid() == *txid))
            .map(|(h, _)| *h)
    }

    /// Transaction IDs whose confirmation and transaction evidence is still
    /// inside the retained undo horizon.
    pub fn retained_confirmations(&self) -> BTreeSet<Txid> {
        self.undo
            .values()
            .flat_map(|undo| undo.confirmed.iter().map(|p| p.tx.compute_txid()))
            .collect()
    }

    /// The height of the block that holds a coin, if the view holds it.
    pub fn height_of(&self, outpoint: &OutPoint) -> Option<u32> {
        self.coins.get(outpoint).map(|c| c.height)
    }

    // ---- changing ------------------------------------------------------------

    /// Raise the marks without changing the checked position. This is useful
    /// when the scripts are installed before a new driver starts. Runtime
    /// growth should use [`CoinView::raise_marks_and_rescan`].
    pub fn raise_marks<E>(
        &mut self,
        receive_next: u32,
        change_next: u32,
        persist: impl FnOnce(&[u8]) -> Result<(), E>,
    ) -> Result<bool, E> {
        let receive_next = receive_next.max(self.receive_next).min(INDEX_LIMIT);
        let change_next = change_next.max(self.change_next).min(INDEX_LIMIT);
        if receive_next == self.receive_next && change_next == self.change_next {
            return Ok(false);
        }
        let mut next = self.clone();
        next.receive_next = receive_next;
        next.change_next = change_next;
        persist(&encode(&next))?;
        *self = next;
        Ok(true)
    }

    /// Raise the watch marks and begin a bounded rescan of the checked range.
    /// `through` is the highest chain height that may already have been
    /// checked or fetched with the old scripts. The mark and rescan position
    /// are stored together so a restart cannot keep stale scripts with a
    /// ready view. Rewind one block to include the previous checked tip; the
    /// birthday remains the floor.
    pub fn raise_marks_and_rescan<E>(
        &mut self,
        receive_next: u32,
        change_next: u32,
        through: u32,
        persist: impl FnOnce(&[u8]) -> Result<(), E>,
    ) -> Result<bool, E> {
        self.watch_plan(receive_next, change_next, through, &[], 0, persist)
    }

    /// OC-07, OC-22: raise the marks as [`CoinView::raise_marks_and_rescan`]
    /// does, and in the same write set receive indices aside from the live
    /// watch. `due` is when the first deep check is due, used only when none
    /// was set aside before. Setting scripts aside needs no rescan: the
    /// height above which they have not been checked is the position of the
    /// view after any rewind, or lower if scripts were set aside before.
    /// Returns whether anything changed.
    pub fn watch_plan<E>(
        &mut self,
        receive_next: u32,
        change_next: u32,
        through: u32,
        release: &[u32],
        due: u64,
        persist: impl FnOnce(&[u8]) -> Result<(), E>,
    ) -> Result<bool, E> {
        let receive_next = receive_next.max(self.receive_next).min(INDEX_LIMIT);
        let change_next = change_next.max(self.change_next).min(INDEX_LIMIT);
        let marks_grew = receive_next != self.receive_next || change_next != self.change_next;
        let mut next = self.clone();
        next.receive_next = receive_next;
        next.change_next = change_next;
        if marks_grew && next.state != ViewState::DeepReorg {
            let safe = next.checked.saturating_sub(1).max(next.birthday);
            let to = through.max(next.checked);
            next.checked = safe;
            next.state = if to > safe {
                ViewState::Rescanning { to }
            } else {
                ViewState::Ready
            };
        }
        let was_empty = next.released.is_empty();
        let mut added = false;
        for index in release {
            if next.released.runs() >= MAX_RELEASED_SPANS && !next.released.contains(*index) {
                break;
            }
            added |= next.released.insert(*index);
        }
        if added {
            if was_empty {
                next.deep = DeepSchedule {
                    last: next.deep.last,
                    due,
                    failed: false,
                    from: next.checked,
                };
            } else {
                next.deep.from = next.deep.from.min(next.checked);
            }
        }
        if !marks_grew && !added {
            return Ok(false);
        }
        persist(&encode(&next))?;
        *self = next;
        Ok(true)
    }

    /// OC-12, OC-22: put receive indices back in the live watch, because a
    /// payment to them was found. The blocks above `rewind_to` are checked
    /// again by the live watch, which did not have these scripts while the
    /// deep check ran: the position goes down to `rewind_to` and the view
    /// takes the rescanning state up to `through` (or the position it had).
    /// Returns whether anything changed.
    pub fn return_to_live<E>(
        &mut self,
        indices: &[u32],
        rewind_to: u32,
        through: u32,
        persist: impl FnOnce(&[u8]) -> Result<(), E>,
    ) -> Result<bool, E> {
        let mut next = self.clone();
        let mut removed = false;
        for index in indices {
            removed |= next.released.remove(*index);
        }
        if !removed {
            return Ok(false);
        }
        next.normalize_deep();
        if next.state != ViewState::DeepReorg && rewind_to < next.checked {
            let safe = rewind_to.max(next.birthday);
            let to = through.max(next.checked);
            next.checked = safe;
            next.state = if to > safe {
                ViewState::Rescanning { to }
            } else {
                ViewState::Ready
            };
        }
        persist(&encode(&next))?;
        *self = next;
        Ok(true)
    }

    /// With nothing set aside there is no schedule.
    fn normalize_deep(&mut self) {
        if self.released.is_empty() {
            self.deep.due = 0;
            self.deep.failed = false;
            self.deep.from = 0;
        }
    }

    /// OC-24: a deep check has checked every script set aside, up to
    /// `target`. The next one is due at `next_due`.
    pub fn deep_finished<E>(
        &mut self,
        now: u64,
        next_due: u64,
        target: u32,
        persist: impl FnOnce(&[u8]) -> Result<(), E>,
    ) -> Result<(), E> {
        let mut next = self.clone();
        next.deep.last = now;
        next.deep.due = next_due;
        next.deep.failed = false;
        next.deep.from = next.deep.from.max(target);
        next.normalize_deep();
        if next == *self {
            return Ok(());
        }
        persist(&encode(&next))?;
        *self = next;
        Ok(())
    }

    /// OC-26: a deep check did not finish. It is tried again at `retry_at`.
    pub fn deep_failed<E>(
        &mut self,
        retry_at: u64,
        persist: impl FnOnce(&[u8]) -> Result<(), E>,
    ) -> Result<(), E> {
        let mut next = self.clone();
        next.deep.due = retry_at;
        next.deep.failed = true;
        next.normalize_deep();
        if next == *self {
            return Ok(());
        }
        persist(&encode(&next))?;
        *self = next;
        Ok(())
    }

    /// OC-24: the owner asks for a deep check at once. Stored, so that a
    /// restart before it starts does not lose the request. Returns false
    /// when nothing is set aside.
    pub fn deep_request<E>(
        &mut self,
        persist: impl FnOnce(&[u8]) -> Result<(), E>,
    ) -> Result<bool, E> {
        if self.released.is_empty() {
            return Ok(false);
        }
        if self.deep.due == 1 {
            return Ok(true);
        }
        let mut next = self.clone();
        next.deep.due = 1;
        persist(&encode(&next))?;
        *self = next;
        Ok(true)
    }

    /// Start again from the birthday: the coins are dropped and every block
    /// above the birthday is checked. For a view in the deep reorg state,
    /// and for a wallet whose files were lost. Pending transactions stay.
    pub fn reset<E>(&mut self, persist: impl FnOnce(&[u8]) -> Result<(), E>) -> Result<(), E> {
        let mut next = self.clone();
        let to = next.checked.max(next.birthday);
        next.coins.clear();
        next.undo.clear();
        next.clear_receipts();
        // The scripts set aside were never part of the scan from the
        // birthday: a deep check from there is due at once.
        if !next.released.is_empty() {
            next.deep.from = next.birthday;
            next.deep.due = 1;
            next.deep.failed = false;
        }
        next.checked = next.birthday;
        next.state = if to > next.birthday {
            ViewState::Rescanning { to }
        } else {
            ViewState::Ready
        };
        persist(&encode(&next))?;
        *self = next;
        Ok(())
    }

    /// The driver reports that every block up to `height` has been checked.
    /// The position is stored; if a rescan has caught up, the view is ready
    /// again.
    pub fn set_checked<E>(
        &mut self,
        height: u32,
        persist: impl FnOnce(&[u8]) -> Result<(), E>,
    ) -> Result<(), E> {
        let mut next = self.clone();
        let position = height.max(next.checked);
        next.checked = position;
        if let ViewState::Rescanning { to } = next.state {
            if position >= to {
                next.state = ViewState::Ready;
            }
        }
        next.undo
            .retain(|h, _| h.saturating_add(UNDO_BLOCKS) > position.max(*h));
        if next == *self {
            return Ok(());
        }
        persist(&encode(&next))?;
        *self = next;
        Ok(())
    }

    /// Forget every receipt, and the full state with them.
    fn clear_receipts(&mut self) {
        self.receipts.clear();
        self.receipt_total = 0;
        self.receipt_log_full = false;
        self.pruned.clear();
    }

    /// OC-29: note that a receipt at `index` was pruned. Ranges that touch
    /// are joined. Past [`MAX_PRUNED_RANGES`] the two ranges with the least
    /// space between them are joined, so the list is bounded: an index in
    /// that space is then refused as too old too, which is the safe way to be
    /// wrong.
    fn mark_pruned(&mut self, index: u32) {
        if self.is_pruned(index) {
            return;
        }
        self.pruned.insert(index, index);
        let mut ranges: Vec<(u32, u32)> = self.pruned.iter().map(|(s, e)| (*s, *e)).collect();
        // Join ranges that touch.
        let mut joined: Vec<(u32, u32)> = Vec::with_capacity(ranges.len());
        for (s, e) in ranges.drain(..) {
            match joined.last_mut() {
                Some((_, pe)) if pe.saturating_add(1) >= s => *pe = (*pe).max(e),
                _ => joined.push((s, e)),
            }
        }
        while joined.len() > MAX_PRUNED_RANGES {
            let mut best = 0;
            let mut best_gap = u32::MAX;
            for (i, pair) in joined.windows(2).enumerate() {
                let [first, second] = pair else { continue };
                let gap = second.0.saturating_sub(first.1);
                if gap < best_gap {
                    best_gap = gap;
                    best = i;
                }
            }
            let Some(next) = joined.get(best + 1).copied() else {
                break;
            };
            if let Some(first) = joined.get_mut(best) {
                first.1 = next.1;
            }
            joined.remove(best + 1);
        }
        self.pruned = joined.into_iter().collect();
    }

    /// OC-28: let go of receipts of spent coins that are more than
    /// [`RECEIPT_KEEP_BLOCKS`] below `tip`, oldest block first, when more
    /// than [`PRUNE_START`] are kept, until [`PRUNE_TO`] are. A receipt of a
    /// coin that is unspent, one that a pending transaction spends included,
    /// never leaves. Returns how many left. The view changes in memory only;
    /// the caller stores it.
    fn prune(&mut self, tip: u32) -> usize {
        if self.receipt_total <= PRUNE_START {
            return 0;
        }
        let cutoff = tip.saturating_sub(RECEIPT_KEEP_BLOCKS);
        let mut old: Vec<(u32, u32, OutPoint)> = self
            .receipts
            .values()
            .flatten()
            .filter(|r| r.height < cutoff && !self.coins.contains_key(&r.outpoint))
            .map(|r| (r.height, r.index, r.outpoint))
            .collect();
        old.sort_unstable();
        old.truncate(self.receipt_total - PRUNE_TO);
        for (_, index, outpoint) in &old {
            let Some(list) = self.receipts.get_mut(index) else {
                continue;
            };
            if let Ok(at) = list.binary_search_by_key(outpoint, |r| r.outpoint) {
                list.remove(at);
                self.receipt_total = self.receipt_total.saturating_sub(1);
            }
            if list.is_empty() {
                self.receipts.remove(index);
            }
            self.mark_pruned(*index);
        }
        old.len()
    }

    /// OC-28: prune the receipts that may leave and store the result before
    /// it takes effect. The node calls it when it starts, with the position
    /// the view reflects; blocks applied later prune by themselves. Returns
    /// how many receipts left.
    pub fn prune_receipts<E>(
        &mut self,
        tip: u32,
        persist: impl FnOnce(&[u8]) -> Result<(), E>,
    ) -> Result<usize, E> {
        let mut next = self.clone();
        let n = next.prune(tip);
        if n == 0 {
            return Ok(0);
        }
        persist(&encode(&next))?;
        *self = next;
        Ok(n)
    }

    /// OC-12, OC-13: keep a receipt once. Returns whether the view changed.
    fn record_receipt(&mut self, receipt: Receipt) -> bool {
        if self.receipt_log_full {
            return false;
        }
        let known = self.receipts.get(&receipt.index).is_some_and(|list| {
            list.binary_search_by_key(&receipt.outpoint, |r| r.outpoint)
                .is_ok()
        });
        if known {
            return false;
        }
        if self.receipt_total >= MAX_RECEIPTS {
            self.receipt_log_full = true;
            return true;
        }
        let list = self.receipts.entry(receipt.index).or_default();
        let at = list.partition_point(|r| r.outpoint < receipt.outpoint);
        list.insert(at, receipt);
        self.receipt_total += 1;
        true
    }

    /// A receipt added by hand, for tests of what rests on receipts.
    #[cfg(test)]
    pub(crate) fn add_receipt_for_test(&mut self, receipt: Receipt) -> bool {
        self.record_receipt(receipt)
    }

    /// OC-12: remove the receipts of one block.
    fn remove_receipts_of(&mut self, block: &BlockHash) {
        let mut removed = 0;
        self.receipts.retain(|_, list| {
            let before = list.len();
            list.retain(|r| r.block != *block);
            removed += before - list.len();
            !list.is_empty()
        });
        self.receipt_total = self.receipt_total.saturating_sub(removed);
    }

    /// Hold a transaction of the owner's own: its change is offered with 0
    /// confirmations and the coins it spends are not offered. `scripts` are
    /// the wallet scripts watched, to find the change. Giving the same
    /// transaction again changes nothing.
    pub fn add_pending<E>(
        &mut self,
        tx: &Transaction,
        scripts: &ScriptSet,
        persist: impl FnOnce(&[u8]) -> Result<(), E>,
    ) -> Result<bool, PendingError<E>> {
        let txid = tx.compute_txid();
        if self.pending.contains_key(&txid) {
            return Ok(false);
        }
        if self.pending.len() >= MAX_PENDING {
            return Err(PendingError::Full);
        }
        let own = tx
            .output
            .iter()
            .enumerate()
            .filter_map(|(vout, out)| {
                let (chain, index) = scripts.get(&out.script_pubkey)?;
                Some((u32::try_from(vout).ok()?, *chain, *index))
            })
            .collect();
        let mut next = self.clone();
        next.pending.insert(
            txid,
            PendingTx {
                tx: tx.clone(),
                own,
            },
        );
        persist(&encode(&next)).map_err(PendingError::Store)?;
        *self = next;
        Ok(true)
    }

    /// Stop holding a transaction (it was dropped, or replaced and put back
    /// in the queue). Stored first. True if it was held.
    pub fn drop_pending<E>(
        &mut self,
        txid: &Txid,
        persist: impl FnOnce(&[u8]) -> Result<(), E>,
    ) -> Result<bool, E> {
        if !self.pending.contains_key(txid) {
            return Ok(false);
        }
        let mut next = self.clone();
        next.pending.remove(txid);
        persist(&encode(&next))?;
        *self = next;
        Ok(true)
    }

    /// Apply a block that the driver handed over and that parsed. `scripts`
    /// are the wallet scripts watched. A block that is not on the best chain
    /// of `chain` is refused (WR-01) and changes nothing, as is one whose
    /// payments the watcher refuses. A block already applied is applied
    /// again without harm: a rescan reads blocks twice.
    pub fn apply_block<C: HeaderChain + ?Sized, E>(
        &mut self,
        block: &WellFormedBlock,
        chain: &C,
        scripts: &ScriptSet,
        persist: impl FnOnce(&[u8]) -> Result<(), E>,
    ) -> Result<BlockEffect, ApplyError<E>> {
        let mut next = self.clone();
        let hash = block.hash();
        let mut effect = BlockEffect::default();
        let mut height = None;
        // OC-28: make room before this block's receipts are recorded.
        let mut receipts_changed = next.prune(chain.tip_height()) > 0;

        // The scripts this block pays.
        let hits: BTreeSet<&ScriptBuf> = block
            .output_scripts()
            .filter_map(|s| scripts.get_key_value(s).map(|(k, _)| k))
            .collect();
        for script in hits {
            let Some((keychain, index)) = scripts.get(script) else {
                continue;
            };
            let payments = block.payments(chain, script).map_err(ApplyError::Refused)?;
            for proven in payments {
                height = Some(proven.height());
                let tx = proven.tx();
                let txid = tx.compute_txid();
                for (vout, out) in tx.output.iter().enumerate() {
                    if out.script_pubkey != *script {
                        continue;
                    }
                    let Ok(vout) = u32::try_from(vout) else {
                        continue;
                    };
                    let outpoint = OutPoint::new(txid, vout);
                    // OC-12: a payment to a receive script is a receipt,
                    // whether or not a later block spends it.
                    if *keychain == Keychain::Receive {
                        receipts_changed |= next.record_receipt(Receipt {
                            index: *index,
                            outpoint,
                            sats: out.value.to_sat(),
                            height: proven.height(),
                            block: proven.block(),
                        });
                    }
                    if next.coins.contains_key(&outpoint) {
                        continue;
                    }
                    next.coins.insert(
                        outpoint,
                        OwnedCoin {
                            outpoint,
                            tx: tx.clone(),
                            chain: *keychain,
                            index: *index,
                            height: proven.height(),
                            block: proven.block(),
                        },
                    );
                    effect.created.push(outpoint);
                }
            }
        }
        if next.coins.len() > MAX_COINS {
            return Err(ApplyError::Full);
        }

        // What the block spends: every coin held now, the ones just made
        // included.
        let wanted: Vec<OutPoint> = next.coins.keys().copied().collect();
        let spends = block.spends(chain, &wanted).map_err(ApplyError::Refused)?;
        let mut spent_coins: Vec<OwnedCoin> = Vec::new();
        let mut spenders: BTreeMap<OutPoint, Txid> = BTreeMap::new();
        for (outpoint, by, _) in spends {
            if let Some(coin) = next.coins.remove(&outpoint) {
                effect.spent.push(outpoint);
                effect.spent_by.push((outpoint, by));
                spent_coins.push(coin);
            }
            spenders.insert(outpoint, by);
        }
        // OC-28: what this block spent may be what a receipt waited for.
        receipts_changed |= next.prune(chain.tip_height()) > 0;
        // Held transactions: confirmed if the block spent their inputs in
        // them, replaced if in another transaction.
        let mut confirmed: Vec<PendingTx> = Vec::new();
        let held: Vec<Txid> = next.pending.keys().copied().collect();
        for txid in held {
            let Some(p) = next.pending.get(&txid) else {
                continue;
            };
            let by: Vec<Txid> =
                p.tx.input
                    .iter()
                    .filter_map(|i| spenders.get(&i.previous_output).copied())
                    .collect();
            if by.is_empty() {
                continue;
            }
            if by.iter().all(|b| *b == txid) {
                if let Some(p) = next.pending.remove(&txid) {
                    effect.confirmed.push(txid);
                    confirmed.push(p);
                }
            } else if by.iter().any(|b| *b != txid) {
                next.pending.remove(&txid);
                effect.replaced.push(txid);
            }
        }

        let needs_undo =
            !effect.created.is_empty() || !effect.spent.is_empty() || !confirmed.is_empty();
        let changed = needs_undo || !effect.replaced.is_empty() || receipts_changed;
        if needs_undo {
            let height = match height {
                Some(h) => h,
                None => chain
                    .height_of(&hash)
                    .ok_or(ApplyError::Refused(Refusal::NotOnChain))?,
            };
            let entry = next.undo.entry(height).or_insert_with(|| Undo {
                block: hash,
                spent: Vec::new(),
                confirmed: Vec::new(),
            });
            if entry.block != hash {
                *entry = Undo {
                    block: hash,
                    spent: Vec::new(),
                    confirmed: Vec::new(),
                };
            }
            for coin in spent_coins {
                if !entry.spent.iter().any(|c| c.outpoint == coin.outpoint) {
                    entry.spent.push(coin);
                }
            }
            for p in confirmed {
                if !entry.confirmed.iter().any(|c| c.tx == p.tx) {
                    entry.confirmed.push(p);
                }
            }
            let tip = chain.tip_height();
            next.undo
                .retain(|h, _| h.saturating_add(UNDO_BLOCKS) > tip.max(*h));
        }
        if changed {
            persist(&encode(&next)).map_err(ApplyError::Store)?;
            *self = next;
        }
        Ok(effect)
    }

    /// Blocks left the best chain (old tip first). What they spent is put
    /// back, what they created is removed, and the held transactions they
    /// confirmed are held again. `fork` is the height of the block below the
    /// lowest of them: the position is taken down to it. A reorganization
    /// the undo entries do not cover makes the state
    /// [`ViewState::DeepReorg`].
    pub fn reorganize<E>(
        &mut self,
        left: &[BlockHash],
        fork: u32,
        persist: impl FnOnce(&[u8]) -> Result<(), E>,
    ) -> Result<ReorgEffect, E> {
        let mut next = self.clone();
        let mut effect = ReorgEffect::default();
        let mut deep = left.len() > usize::try_from(UNDO_BLOCKS).unwrap_or(usize::MAX);
        for hash in left {
            let at = next
                .undo
                .iter()
                .find(|(_, u)| u.block == *hash)
                .map(|(h, _)| *h);
            match at {
                Some(h) => {
                    let Some(undo) = next.undo.remove(&h) else {
                        continue;
                    };
                    for coin in undo.spent {
                        // OC-28: the receipt of a coin put back is kept
                        // again, whether or not it was pruned while the
                        // coin was spent. The pruned range stays: the
                        // index may have lost other receipts. A full log
                        // takes its named state.
                        if coin.chain == Keychain::Receive {
                            next.record_receipt(Receipt {
                                index: coin.index,
                                outpoint: coin.outpoint,
                                sats: coin.sats(),
                                height: coin.height,
                                block: coin.block,
                            });
                        }
                        next.coins.insert(coin.outpoint, coin);
                        effect.restored += 1;
                    }
                    for p in undo.confirmed {
                        let txid = p.tx.compute_txid();
                        effect.reopened.push(txid);
                        next.pending.insert(txid, p);
                    }
                    let gone: Vec<OutPoint> = next
                        .coins
                        .iter()
                        .filter(|(_, c)| c.block == *hash)
                        .map(|(op, _)| *op)
                        .collect();
                    for op in gone {
                        next.coins.remove(&op);
                        effect.removed += 1;
                    }
                    // OC-12: the receipts go with the coins.
                    next.remove_receipts_of(hash);
                }
                None => {
                    // A block with no undo entry that holds a coin or a
                    // receipt was applied and pruned.
                    if next.coins.values().any(|c| c.block == *hash)
                        || next.receipts.values().flatten().any(|r| r.block == *hash)
                    {
                        deep = true;
                    }
                }
            }
        }
        if deep {
            // The position stays where it was: a reset rescans up to it.
            next.state = ViewState::DeepReorg;
        } else {
            next.checked = next.checked.min(fork).max(next.birthday);
            if !next.released.is_empty() {
                next.deep.from = next.deep.from.min(next.checked);
            }
        }
        persist(&encode(&next))?;
        *self = next;
        Ok(effect)
    }

    // ---- stored form ---------------------------------------------------------

    /// The stored form, `OBMe`.
    pub fn to_bytes(&self) -> Vec<u8> {
        encode(self)
    }

    /// Read the stored form strictly.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, DecodeError> {
        decode(bytes)
    }
}

/// Why a transaction was not held as pending.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PendingError<E> {
    /// [`MAX_PENDING`] are held already.
    Full,
    /// The new view could not be stored.
    Store(E),
}

// ---- encoding ---------------------------------------------------------------

fn len32(len: usize) -> u32 {
    u32::try_from(len).unwrap_or(u32::MAX)
}

fn put_tx(out: &mut Vec<u8>, tx: &Transaction) {
    let bytes = serialize(tx);
    out.extend_from_slice(&len32(bytes.len()).to_le_bytes());
    out.extend_from_slice(&bytes);
}

fn chain_byte(chain: Keychain) -> u8 {
    match chain {
        Keychain::Receive => 0,
        Keychain::Change => 1,
    }
}

fn put_coin(out: &mut Vec<u8>, c: &OwnedCoin) {
    out.extend_from_slice(&c.outpoint.txid.to_byte_array());
    out.extend_from_slice(&c.outpoint.vout.to_le_bytes());
    put_tx(out, &c.tx);
    out.push(chain_byte(c.chain));
    out.extend_from_slice(&c.index.to_le_bytes());
    out.extend_from_slice(&c.height.to_le_bytes());
    out.extend_from_slice(&c.block.to_byte_array());
}

fn put_pending(out: &mut Vec<u8>, p: &PendingTx) {
    put_tx(out, &p.tx);
    out.extend_from_slice(&len32(p.own.len()).to_le_bytes());
    for (vout, chain, index) in &p.own {
        out.extend_from_slice(&vout.to_le_bytes());
        out.push(chain_byte(*chain));
        out.extend_from_slice(&index.to_le_bytes());
    }
}

fn encode(v: &CoinView) -> Vec<u8> {
    let mut out = wire::start(&MAGIC_OWNER_COINS, 256);
    out.extend_from_slice(&v.birthday.to_le_bytes());
    out.extend_from_slice(&v.checked.to_le_bytes());
    out.extend_from_slice(&v.receive_next.to_le_bytes());
    out.extend_from_slice(&v.change_next.to_le_bytes());
    match v.state {
        ViewState::Ready => out.extend_from_slice(&[0, 0, 0, 0, 0]),
        ViewState::Rescanning { to } => {
            out.push(1);
            out.extend_from_slice(&to.to_le_bytes());
        }
        ViewState::DeepReorg => out.extend_from_slice(&[2, 0, 0, 0, 0]),
    }
    // OC-22, OC-24: the scripts set aside as runs, then the schedule.
    out.extend_from_slice(&len32(v.released.runs()).to_le_bytes());
    for (start, end) in v.released.pairs() {
        out.extend_from_slice(&start.to_le_bytes());
        out.extend_from_slice(&end.to_le_bytes());
    }
    out.extend_from_slice(&v.deep.last.to_le_bytes());
    out.extend_from_slice(&v.deep.due.to_le_bytes());
    out.push(u8::from(v.deep.failed));
    out.extend_from_slice(&v.deep.from.to_le_bytes());
    out.extend_from_slice(&len32(v.coins.len()).to_le_bytes());
    for c in v.coins.values() {
        put_coin(&mut out, c);
    }
    out.extend_from_slice(&len32(v.undo.len()).to_le_bytes());
    for (height, u) in &v.undo {
        out.extend_from_slice(&height.to_le_bytes());
        out.extend_from_slice(&u.block.to_byte_array());
        out.extend_from_slice(&len32(u.spent.len()).to_le_bytes());
        for c in &u.spent {
            put_coin(&mut out, c);
        }
        out.extend_from_slice(&len32(u.confirmed.len()).to_le_bytes());
        for p in &u.confirmed {
            put_pending(&mut out, p);
        }
    }
    out.extend_from_slice(&len32(v.pending.len()).to_le_bytes());
    for p in v.pending.values() {
        put_pending(&mut out, p);
    }
    // OC-12, OC-13: the log-full flag, then the receipts ascending by index
    // and outpoint.
    out.push(u8::from(v.receipt_log_full));
    out.extend_from_slice(&len32(v.receipt_total).to_le_bytes());
    for r in v.receipts.values().flatten() {
        out.extend_from_slice(&r.index.to_le_bytes());
        out.extend_from_slice(&r.outpoint.txid.to_byte_array());
        out.extend_from_slice(&r.outpoint.vout.to_le_bytes());
        out.extend_from_slice(&r.sats.to_le_bytes());
        out.extend_from_slice(&r.height.to_le_bytes());
        out.extend_from_slice(&r.block.to_byte_array());
    }
    // OC-29: the pruned index ranges.
    out.extend_from_slice(&len32(v.pruned.len()).to_le_bytes());
    for (start, end) in &v.pruned {
        out.extend_from_slice(&start.to_le_bytes());
        out.extend_from_slice(&end.to_le_bytes());
    }
    out
}

fn get_tx(r: &mut Reader<'_>) -> Result<Transaction, DecodeError> {
    let n = usize::try_from(r.u32_le()?).map_err(|_| DecodeError::TooLarge)?;
    if n > MAX_TX_BYTES {
        return Err(DecodeError::TooLarge);
    }
    let bytes = r.take(n)?;
    let tx: Transaction =
        deserialize(bytes).map_err(|_| DecodeError::NonCanonical("not a transaction"))?;
    if serialize(&tx) != bytes {
        return Err(DecodeError::NonCanonical(
            "transaction is not in its own form",
        ));
    }
    Ok(tx)
}

fn get_chain(r: &mut Reader<'_>) -> Result<Keychain, DecodeError> {
    match r.u8()? {
        0 => Ok(Keychain::Receive),
        1 => Ok(Keychain::Change),
        _ => Err(DecodeError::NonCanonical("keychain is not 0 or 1")),
    }
}

fn get_coin(r: &mut Reader<'_>) -> Result<OwnedCoin, DecodeError> {
    let txid = Txid::from_byte_array(r.array()?);
    let vout = r.u32_le()?;
    let tx = get_tx(r)?;
    let chain = get_chain(r)?;
    let index = r.u32_le()?;
    let height = r.u32_le()?;
    let block = BlockHash::from_byte_array(r.array()?);
    if tx.compute_txid() != txid {
        return Err(DecodeError::NonCanonical("coin is not of its transaction"));
    }
    if usize::try_from(vout).map_or(true, |v| v >= tx.output.len()) {
        return Err(DecodeError::NonCanonical("coin names a missing output"));
    }
    if index >= INDEX_LIMIT {
        return Err(DecodeError::NonCanonical("index is out of range"));
    }
    Ok(OwnedCoin {
        outpoint: OutPoint::new(txid, vout),
        tx,
        chain,
        index,
        height,
        block,
    })
}

fn get_pending(r: &mut Reader<'_>) -> Result<PendingTx, DecodeError> {
    let tx = get_tx(r)?;
    let n = r.count(tx.output.len(), 9)?;
    let mut own = Vec::with_capacity(n);
    let mut last: Option<u32> = None;
    for _ in 0..n {
        let vout = r.u32_le()?;
        let chain = get_chain(r)?;
        let index = r.u32_le()?;
        if usize::try_from(vout).map_or(true, |v| v >= tx.output.len())
            || last.is_some_and(|l| l >= vout)
            || index >= INDEX_LIMIT
        {
            return Err(DecodeError::NonCanonical("change list is not in order"));
        }
        last = Some(vout);
        own.push((vout, chain, index));
    }
    Ok(PendingTx { tx, own })
}

fn decode(bytes: &[u8]) -> Result<CoinView, DecodeError> {
    if bytes.len() > MAX_VIEW_BYTES {
        return Err(DecodeError::TooLarge);
    }
    let mut r = Reader::framed(bytes, &MAGIC_OWNER_COINS)?;
    let birthday = r.u32_le()?;
    let checked = r.u32_le()?;
    let receive_next = r.u32_le()?;
    let change_next = r.u32_le()?;
    if receive_next > INDEX_LIMIT || change_next > INDEX_LIMIT {
        return Err(DecodeError::NonCanonical("mark is out of range"));
    }
    if checked < birthday {
        return Err(DecodeError::NonCanonical("position is below the birthday"));
    }
    let state = {
        let kind = r.u8()?;
        let to = r.u32_le()?;
        match (kind, to) {
            (0, 0) => ViewState::Ready,
            (1, to) if to > checked => ViewState::Rescanning { to },
            (2, 0) => ViewState::DeepReorg,
            _ => return Err(DecodeError::NonCanonical("state is not canonical")),
        }
    };
    let n = r.count(MAX_RELEASED_SPANS, 8)?;
    let mut pairs = Vec::with_capacity(n);
    for _ in 0..n {
        let start = r.u32_le()?;
        let end = r.u32_le()?;
        pairs.push((start, end));
    }
    let released = Spans::from_pairs(&pairs).ok_or(DecodeError::NonCanonical(
        "set-aside runs are not canonical",
    ))?;
    let deep = DeepSchedule {
        last: r.u64_le()?,
        due: r.u64_le()?,
        failed: r.flag()?,
        from: r.u32_le()?,
    };
    if released.is_empty() && (deep.due != 0 || deep.failed || deep.from != 0) {
        return Err(DecodeError::NonCanonical(
            "a deep check is scheduled with nothing set aside",
        ));
    }
    if !released.is_empty() && deep.due == 0 {
        return Err(DecodeError::NonCanonical(
            "scripts are set aside with no deep check due",
        ));
    }
    let n = r.count(MAX_COINS, 80)?;
    let mut coins = BTreeMap::new();
    let mut last: Option<OutPoint> = None;
    for _ in 0..n {
        let c = get_coin(&mut r)?;
        if last.is_some_and(|l| l >= c.outpoint) {
            return Err(DecodeError::NonCanonical("coins are not in order"));
        }
        last = Some(c.outpoint);
        coins.insert(c.outpoint, c);
    }
    let n = r.count(UNDO_BLOCKS as usize, 44)?;
    let mut undo = BTreeMap::new();
    let mut last_height: Option<u32> = None;
    for _ in 0..n {
        let height = r.u32_le()?;
        if last_height.is_some_and(|l| l >= height) {
            return Err(DecodeError::NonCanonical("undo entries are not in order"));
        }
        last_height = Some(height);
        let block = BlockHash::from_byte_array(r.array()?);
        let ns = r.count(MAX_COINS, 80)?;
        let mut spent = Vec::with_capacity(ns);
        let mut last: Option<OutPoint> = None;
        for _ in 0..ns {
            let c = get_coin(&mut r)?;
            if last.is_some_and(|l| l >= c.outpoint) {
                return Err(DecodeError::NonCanonical("undo coins are not in order"));
            }
            last = Some(c.outpoint);
            spent.push(c);
        }
        let nc = r.count(MAX_PENDING, 8)?;
        let mut confirmed = Vec::with_capacity(nc);
        for _ in 0..nc {
            confirmed.push(get_pending(&mut r)?);
        }
        undo.insert(
            height,
            Undo {
                block,
                spent,
                confirmed,
            },
        );
    }
    let n = r.count(MAX_PENDING, 8)?;
    let mut pending = BTreeMap::new();
    let mut last: Option<Txid> = None;
    for _ in 0..n {
        let p = get_pending(&mut r)?;
        let txid = p.tx.compute_txid();
        if last.is_some_and(|l| l >= txid) {
            return Err(DecodeError::NonCanonical("pending list is not in order"));
        }
        last = Some(txid);
        pending.insert(txid, p);
    }
    let receipt_log_full = r.flag()?;
    let n = r.count(MAX_RECEIPTS, 84)?;
    let mut receipts: BTreeMap<u32, Vec<Receipt>> = BTreeMap::new();
    let mut outpoints: BTreeSet<OutPoint> = BTreeSet::new();
    let mut last: Option<(u32, OutPoint)> = None;
    for _ in 0..n {
        let index = r.u32_le()?;
        let txid = Txid::from_byte_array(r.array()?);
        let vout = r.u32_le()?;
        let outpoint = OutPoint::new(txid, vout);
        let sats = r.u64_le()?;
        let height = r.u32_le()?;
        let block = BlockHash::from_byte_array(r.array()?);
        if index >= INDEX_LIMIT {
            return Err(DecodeError::NonCanonical("index is out of range"));
        }
        if last.is_some_and(|l| l >= (index, outpoint)) {
            return Err(DecodeError::NonCanonical("receipts are not in order"));
        }
        if !outpoints.insert(outpoint) {
            return Err(DecodeError::NonCanonical("a receipt is listed twice"));
        }
        last = Some((index, outpoint));
        receipts.entry(index).or_default().push(Receipt {
            index,
            outpoint,
            sats,
            height,
            block,
        });
    }
    let np = r.count(MAX_PRUNED_RANGES, 8)?;
    let mut pruned = BTreeMap::new();
    let mut prev_end: Option<u32> = None;
    for _ in 0..np {
        let start = r.u32_le()?;
        let end = r.u32_le()?;
        if start > end || end >= INDEX_LIMIT {
            return Err(DecodeError::NonCanonical("pruned range is not valid"));
        }
        if prev_end.is_some_and(|p| start <= p.saturating_add(1)) {
            return Err(DecodeError::NonCanonical(
                "pruned ranges are not in order and apart",
            ));
        }
        prev_end = Some(end);
        pruned.insert(start, end);
    }
    r.finish()?;
    // Unless the log is full, every unspent receive coin has its receipt.
    if !receipt_log_full {
        for c in coins.values().filter(|c| c.chain == Keychain::Receive) {
            let held = receipts
                .get(&c.index)
                .and_then(|list| list.iter().find(|r| r.outpoint == c.outpoint));
            let fits = held
                .is_some_and(|r| r.sats == c.sats() && r.height == c.height && r.block == c.block);
            if !fits {
                return Err(DecodeError::NonCanonical(
                    "a receive coin has no matching receipt",
                ));
            }
        }
    }
    Ok(CoinView {
        birthday,
        checked,
        receive_next,
        change_next,
        state,
        coins,
        undo,
        pending,
        receipts,
        receipt_total: n,
        receipt_log_full,
        pruned,
        released,
        deep,
    })
}
