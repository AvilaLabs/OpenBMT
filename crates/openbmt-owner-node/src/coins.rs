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
//! ## What is watched
//!
//! Receive indices from 0 to the highest a signed batch handed out plus
//! [`GAP`], change indices from 0 to the next change index plus [`GAP`]. The
//! two marks are kept here ([`CoinView::raise_marks`]). When they grow, the
//! view goes back to the birthday height and the blocks above it are
//! checked again for the new scripts ([`ViewState::Rescanning`]); no coin is
//! offered until the rescan has caught up with where the view was.
//!
//! ## The stored form
//!
//! `OBMe`, version 1, strict like the other stored forms: sorted lists, no
//! repeats, every length checked before anything is reserved, and every coin
//! must be an output of the transaction it carries. The chain position
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

/// The magic of the stored coin view: `OBMe`.
pub const MAGIC_OWNER_COINS: [u8; 4] = *b"OBMe";
/// Unused indices watched beyond the highest one in use.
pub const GAP: u32 = 20;
/// Blocks of undo entries kept.
pub const UNDO_BLOCKS: u32 = 100;
/// Most coins held.
pub const MAX_COINS: usize = 20_000;
/// Most held transactions kept as pending.
pub const MAX_PENDING: usize = 64;
/// The largest transaction a stored coin may carry, in bytes.
pub const MAX_TX_BYTES: usize = 400_000;
/// The largest stored view, in bytes.
pub const MAX_VIEW_BYTES: usize = 128 * 1024 * 1024;
/// The largest wallet index (the wallet takes unhardened indices only).
const INDEX_LIMIT: u32 = 1 << 31;

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

    /// What the view can say about itself.
    pub fn state(&self) -> ViewState {
        self.state
    }

    /// Whether the coins can be offered to a spend.
    pub fn ready(&self) -> bool {
        self.state == ViewState::Ready
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

    /// The height of the block that holds a coin, if the view holds it.
    pub fn height_of(&self, outpoint: &OutPoint) -> Option<u32> {
        self.coins.get(outpoint).map(|c| c.height)
    }

    // ---- changing ------------------------------------------------------------

    /// Raise the marks. Returns whether either grew. When one does, the view
    /// goes back to the birthday and rescans (see the module docs); the new
    /// value is stored first.
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
        next.begin_rescan();
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

    /// Go back to the birthday for a rescan of the scripts just added. The
    /// coins already known stay (a block seen again does not add one twice).
    fn begin_rescan(&mut self) {
        let to = match self.state {
            ViewState::Rescanning { to } => to.max(self.checked),
            _ => self.checked,
        };
        if self.state == ViewState::DeepReorg {
            return;
        }
        if to > self.birthday {
            self.state = ViewState::Rescanning { to };
            self.checked = self.birthday;
        }
    }

    /// The driver reports that every block up to `height` has been checked.
    /// The position is stored; if a rescan has caught up, the view is ready
    /// again.
    pub fn set_checked<E>(
        &mut self,
        height: u32,
        persist: impl FnOnce(&[u8]) -> Result<(), E>,
    ) -> Result<(), E> {
        if height <= self.checked {
            return Ok(());
        }
        let mut next = self.clone();
        next.checked = height;
        if let ViewState::Rescanning { to } = next.state {
            if height >= to {
                next.state = ViewState::Ready;
            }
        }
        persist(&encode(&next))?;
        *self = next;
        Ok(())
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
                spent_coins.push(coin);
            }
            spenders.insert(outpoint, by);
        }
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
        let changed = needs_undo || !effect.replaced.is_empty();
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
                }
                None => {
                    // A block with no undo entry that holds a coin was
                    // applied and pruned.
                    if next.coins.values().any(|c| c.block == *hash) {
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
    r.finish()?;
    Ok(CoinView {
        birthday,
        checked,
        receive_next,
        change_next,
        state,
        coins,
        undo,
        pending,
    })
}
