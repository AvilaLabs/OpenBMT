//! The owner's line about receipts, and the offer to merge them
//! (`docs/OWNER-COINS-RULES.md`, OC-16 to OC-21), and the fee samples the
//! line rests on (`docs/FEE-RULES.md`, FE-20).
//!
//! The wallet's coins are what the screen calls payments or receipts: each
//! one costs an input when it is spent, so the number of them and the fee
//! of the last blocks give what spending them all would cost. The fee comes
//! from the blocks this device has checked and downloaded in full, never
//! from a service. The words of every state are in [`crate::say`].

use std::fs;
use std::io;
use std::path::Path;

use bitcoin::consensus::deserialize;
use bitcoin::hashes::Hash;
use bitcoin::{Block, BlockHash};
use openbmt_owner::{Built, CashAmount, Stage};
use openbmt_spend::{consolidation_cost_sats, Coin, MAX_CONSOLIDATION_INPUTS};
use openbmt_watch::fees::{LOW_WINDOW_BLOCKS, SAMPLE_EVERY_BLOCKS};
use openbmt_watch::{block_fee_sample, FeeHistory, HeaderChain, WellFormedBlock};

use crate::node::{NodeError, OwnerNode, OwnerPort};
use crate::watcher::write_file;

/// FE-20: the fee history's file, in the node's directory.
pub const FEE_FILE: &str = "fee-history.bin";
/// The largest fee history file the node reads, in bytes. A stored history
/// is under 29,000 bytes (512 samples).
const MAX_FEE_FILE_BYTES: u64 = 64 * 1024;
/// OC-19: fewest coins the offer to merge needs **[to tune]**.
pub const MERGE_MIN_COINS: usize = 10;
/// OC-17: the lowest fee rate the cost is worked out at, in satoshis per
/// 1000 virtual bytes: 1 satoshi per virtual byte, the lowest that nodes
/// relay by default.
pub const MIN_FEE_RATE_PER_KVB: u64 = 1_000;
/// OC-21: most bytes of previous transactions one merge takes in, half of
/// the 8 MiB the signer reads.
pub const MAX_MERGE_PARENT_BYTES: usize = 4 * 1024 * 1024;
/// OC-20: the name the Key app shows for where a merge goes.
pub const MERGE_LABEL: &str = "Your own wallet";

/// OC-18: what the line about receipts says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReceiptsLine {
    /// The view is not ready, or has not checked up to the tip (OC-14).
    NotReady,
    /// The wallet holds no coin.
    NoCoins,
    /// The wallet holds coins, and fewer than 3 blocks of the last 144 give
    /// a fee yet (FE-13).
    FeeUnknown {
        /// The coins the wallet holds.
        payments: usize,
    },
    /// The count and the cost.
    Known {
        /// The coins the wallet holds.
        payments: usize,
        /// What spending all of them costs, in satoshis (OC-17).
        cost_sats: u64,
        /// The rate the cost is worked out at, in satoshis per 1000 virtual
        /// bytes.
        rate_per_kvb: u64,
    },
}

/// OC-19: the offer to merge the wallet's coins into one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MergeOffer {
    /// The coins one merge takes in (OC-21).
    pub coins: usize,
    /// The coins the wallet holds that a merge may spend.
    pub of: usize,
    /// The rate the merge is built at, in satoshis per 1000 virtual bytes.
    pub rate_per_kvb: u64,
    /// The fee of the merge at that rate, in satoshis.
    pub cost_sats: u64,
    /// The fee of the same merge at the usual rate (FE-17), in satoshis.
    pub usual_cost_sats: u64,
}

impl MergeOffer {
    /// What merging now saves against the usual fee, in satoshis.
    pub fn saving_sats(&self) -> u64 {
        self.usual_cost_sats.saturating_sub(self.cost_sats)
    }
}

/// Why a merge was not built.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MergeRefused {
    /// OC-19: the conditions of the offer do not hold now.
    NoOffer,
    /// OC-20: a refund or cash-out is waiting in the queue or in a session,
    /// and the merge would take the same coins.
    OtherPaymentsWaiting,
    /// OC-20: the session could not take the merge. The queue is as it was.
    NotBuilt(String),
    /// The node is not ready to build, or the queue could not be stored.
    Node(NodeError),
}

impl std::fmt::Display for MergeRefused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoOffer => write!(f, "OC-19: there is no offer to merge now"),
            Self::OtherPaymentsWaiting => write!(
                f,
                "OC-20: a refund or cash-out is waiting; the merge would take the same coins"
            ),
            Self::NotBuilt(why) => write!(f, "OC-20: the merge was not built: {why}"),
            Self::Node(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for MergeRefused {}

impl From<NodeError> for MergeRefused {
    fn from(e: NodeError) -> Self {
        Self::Node(e)
    }
}

/// OC-17: the rate a cost is worked out at: the history's rate, in
/// thousandths of a satoshi per virtual byte, which is satoshis per 1000
/// virtual bytes, and at least [`MIN_FEE_RATE_PER_KVB`].
pub fn cost_rate(rate_milli: u64) -> u64 {
    rate_milli.max(MIN_FEE_RATE_PER_KVB)
}

/// OC-16, OC-17, OC-18: the line, from whether the view is ready, the coins
/// held and the recent rate.
pub fn receipts_line(ready: bool, payments: usize, recent_milli: Option<u64>) -> ReceiptsLine {
    if !ready {
        return ReceiptsLine::NotReady;
    }
    if payments == 0 {
        return ReceiptsLine::NoCoins;
    }
    let Some(rate) = recent_milli else {
        return ReceiptsLine::FeeUnknown { payments };
    };
    let rate_per_kvb = cost_rate(rate);
    match consolidation_cost_sats(payments, rate_per_kvb) {
        Some(cost_sats) => ReceiptsLine::Known {
            payments,
            cost_sats,
            rate_per_kvb,
        },
        None => ReceiptsLine::FeeUnknown { payments },
    }
}

/// The key of SP-11: oldest first, ties by transaction ID then output.
fn order_key(coin: &Coin) -> (std::cmp::Reverse<u32>, [u8; 32], u32) {
    (
        std::cmp::Reverse(coin.confirmations),
        coin.outpoint.txid.to_byte_array(),
        coin.outpoint.vout,
    )
}

/// OC-19, OC-21: the coins one merge takes in. The ones a spend may use
/// (`min_confirmations`), oldest first, at most
/// [`MAX_CONSOLIDATION_INPUTS`] and at most [`MAX_MERGE_PARENT_BYTES`] of
/// previous transactions. The count of the coins that were eligible comes
/// with them.
pub fn merge_pool(coins: Vec<Coin>, min_confirmations: u32) -> (Vec<Coin>, usize) {
    let mut eligible: Vec<Coin> = coins
        .into_iter()
        .filter(|c| c.confirmations >= min_confirmations)
        .collect();
    let of = eligible.len();
    eligible.sort_by_key(order_key);
    let mut bytes = 0usize;
    let mut pool = Vec::new();
    for coin in eligible {
        let size = coin.prev_tx.total_size();
        if pool.len() >= MAX_CONSOLIDATION_INPUTS
            || (!pool.is_empty() && bytes.saturating_add(size) > MAX_MERGE_PARENT_BYTES)
        {
            break;
        }
        bytes = bytes.saturating_add(size);
        pool.push(coin);
    }
    (pool, of)
}

/// OC-19: the offer, from the merge's coins, the coins the wallet may
/// merge, the recent and the usual rate and whether the recent one is low.
/// None unless all three conditions hold.
pub fn merge_offer(
    merged: usize,
    of: usize,
    recent_milli: Option<u64>,
    usual_milli: Option<u64>,
    low: Option<bool>,
) -> Option<MergeOffer> {
    if low != Some(true) || of < MERGE_MIN_COINS {
        return None;
    }
    let rate_per_kvb = cost_rate(recent_milli?);
    let usual = cost_rate(usual_milli?);
    let cost_sats = consolidation_cost_sats(merged, rate_per_kvb)?;
    let usual_cost_sats = consolidation_cost_sats(merged, usual)?;
    if usual_cost_sats <= cost_sats {
        return None;
    }
    Some(MergeOffer {
        coins: merged,
        of,
        rate_per_kvb,
        cost_sats,
        usual_cost_sats,
    })
}

/// FE-20: read the stored fee history. An unreadable file gives an empty
/// history and the line for the notes.
pub(crate) fn read_fee_history(dir: &Path) -> (FeeHistory, Option<String>) {
    let path = dir.join(FEE_FILE);
    let size = match fs::metadata(&path) {
        Ok(meta) => meta.len(),
        Err(e) if e.kind() == io::ErrorKind::NotFound => return (FeeHistory::new(), None),
        Err(e) => {
            return (
                FeeHistory::new(),
                Some(format!(
                    "{FEE_FILE}: {e}; the fee history starts empty (FE-20)"
                )),
            )
        }
    };
    if size > MAX_FEE_FILE_BYTES {
        return (
            FeeHistory::new(),
            Some(format!(
                "{FEE_FILE} is too large; the fee history starts empty (FE-20)"
            )),
        );
    }
    match fs::read(&path) {
        Ok(bytes) => match FeeHistory::from_bytes(&bytes) {
            Ok(history) => (history, None),
            Err(e) => (
                FeeHistory::new(),
                Some(format!(
                    "{FEE_FILE} does not read back: {e}; the fee history starts empty (FE-20)"
                )),
            ),
        },
        Err(e) => (
            FeeHistory::new(),
            Some(format!(
                "{FEE_FILE}: {e}; the fee history starts empty (FE-20)"
            )),
        ),
    }
}

impl<P: OwnerPort> OwnerNode<P> {
    // FE-20: the fee samples.

    /// The fee samples this node has taken.
    pub fn fee_history(&self) -> &FeeHistory {
        &self.fees
    }

    /// Build the new history, store it, and only then swap it in. False,
    /// with a note, if it could not be stored.
    fn commit_fees(&mut self, next: FeeHistory) -> bool {
        match write_file(&self.dir, FEE_FILE, &next.to_bytes()) {
            Ok(()) => {
                self.fees = next;
                true
            }
            Err(e) => {
                self.note(format!(
                    "the fee history could not be stored: {e}; it is as it was (FE-20)"
                ));
                false
            }
        }
    }

    /// FE-20: a fee sample from a block the node downloaded in full and has
    /// checked (WR-20 to WR-22). The block must be on the best chain.
    pub(crate) fn take_fee_sample(&mut self, hash: &BlockHash, bytes: &[u8]) {
        let Some(height) = self.watcher.chain().height_of(hash) else {
            return;
        };
        let block: Block = match deserialize(bytes) {
            Ok(b) => b,
            Err(_) => return,
        };
        let sample = match block_fee_sample(&block, height, self.wallet.network()) {
            Ok(s) => s,
            Err(e) => {
                self.note(format!("block {hash} gave no fee sample: {e}"));
                return;
            }
        };
        let mut next = self.fees.clone();
        if next.insert(sample) {
            self.commit_fees(next);
        }
    }

    /// FE-20: at the start, the samples of blocks that the stored chain no
    /// longer holds are dropped: the chain may have been reorganized while
    /// the node was off.
    pub(crate) fn prune_fee_samples(&mut self) {
        let chain = self.watcher.chain();
        let mut next = FeeHistory::new();
        for sample in self.fees.samples() {
            if chain
                .hash_at(sample.height)
                .is_none_or(|held| held == sample.block)
            {
                next.insert(*sample);
            }
        }
        if next != self.fees {
            self.note("fee samples of blocks that left the chain were dropped (FE-20)".to_string());
            self.commit_fees(next);
        }
    }

    /// FE-20: the block that was asked for by hash arrived. It is checked
    /// on its own (WR-20 to WR-22) and must be on the best chain; the coin
    /// view does not read it.
    pub(crate) fn fee_block(&mut self, hash: &BlockHash, bytes: &[u8]) {
        if self.fee_request.map(|(h, _)| h) == Some(*hash) {
            self.fee_request = None;
        }
        match WellFormedBlock::parse(bytes) {
            Ok(b) if b.hash() == *hash => {}
            _ => {
                self.note(format!("block {hash} asked for a fee sample did not check"));
                return;
            }
        }
        self.take_fee_sample(hash, bytes);
    }

    /// FE-20: the chain was reorganized above `from`: the samples at and
    /// above it go, and a block asked for that left is not waited for.
    pub(crate) fn forget_fee_samples_from(&mut self, from: u32, left: &[BlockHash]) {
        if self
            .fee_request
            .is_some_and(|(hash, height)| height >= from || left.contains(&hash))
        {
            self.fee_request = None;
        }
        let mut next = self.fees.clone();
        next.forget_from(from);
        if next != self.fees {
            self.commit_fees(next);
        }
    }

    /// FE-20: at the tip, with no sample in the last 24 checked blocks,
    /// ask for the newest checked block in full. One request at a time; a
    /// request that has gone 24 blocks unanswered is let go and asked again.
    pub(crate) fn ask_for_fee_block(&mut self, height: u32) {
        let tip = self.watcher.chain().tip_height();
        if self.halted.is_some() || self.awaiting_rescan > 0 || !self.view.ready() {
            return;
        }
        // Only blocks in the window the history reads are worth a download.
        if height.saturating_add(LOW_WINDOW_BLOCKS) <= tip {
            return;
        }
        if let Some((_, asked)) = self.fee_request {
            if height < asked.saturating_add(SAMPLE_EVERY_BLOCKS) {
                return;
            }
            self.fee_request = None;
        }
        let recent = self
            .fees
            .samples()
            .iter()
            .rev()
            .find(|s| s.height <= height)
            .map(|s| s.height);
        if recent.is_some_and(|h| height.saturating_sub(h) < SAMPLE_EVERY_BLOCKS) {
            return;
        }
        let Some(hash) = self.watcher.chain().hash_at(height) else {
            return;
        };
        self.fee_request = Some((hash, height));
        self.watcher.port().fetch_block(hash);
    }

    // OC-16 to OC-21: the line and the offer.

    /// Whether the view has checked every block up to the tip and is ready:
    /// the state in which the coins can be counted (OC-14).
    fn counted(&self) -> bool {
        self.halted.is_none()
            && self.awaiting_rescan == 0
            && self.view.ready()
            && self.view.checked() >= self.watcher.chain().tip_height()
    }

    /// OC-16 to OC-18: the line about receipts.
    pub fn receipts_line(&self) -> ReceiptsLine {
        let tip = self.watcher.chain().tip_height();
        receipts_line(
            self.counted(),
            self.view.coins(tip).len(),
            self.fees.recent_rate(tip),
        )
    }

    /// OC-19: the offer to merge, when it holds.
    pub fn merge_offer(&self) -> Option<MergeOffer> {
        if !self.counted() {
            return None;
        }
        let tip = self.watcher.chain().tip_height();
        let (pool, of) = merge_pool(self.view.coins(tip), self.owner.options().min_confirmations);
        merge_offer(
            pool.len(),
            of,
            self.fees.recent_rate(tip),
            self.fees.usual_rate(tip),
            self.fees.is_low(tip),
        )
    }

    /// OC-20: approve the offer. The merge is queued as a cash-out of every
    /// coin it takes in to the wallet's next change address and built into a
    /// session for the Key app, as any spend is: it is signed by two keys
    /// there and taken by [`OwnerNode::take`], and nothing is signed or
    /// sent here. On a refusal the queue is as it was.
    pub fn approve_merge(&mut self, now: u64) -> Result<Built, MergeRefused> {
        let offer = self.merge_offer().ok_or(MergeRefused::NoOffer)?;
        let waiting = |stage: &Stage| matches!(stage, Stage::Queued | Stage::InSession { .. });
        if self.owner.refunds().iter().any(|r| waiting(&r.stage))
            || self.owner.cash_outs().iter().any(|c| waiting(&c.stage))
        {
            return Err(MergeRefused::OtherPaymentsWaiting);
        }
        self.ready_for_queue_write()?;
        let script = self
            .wallet
            .change_script(self.view.change_next())
            .map_err(|e| MergeRefused::NotBuilt(e.to_string()))?;
        let write = self.queue_writer();
        let id = self
            .owner
            .queue_cash_out(script, CashAmount::All, MERGE_LABEL, &write)
            .map_err(|e| MergeRefused::Node(e.into()))?;
        let min_confirmations = self.owner.options().min_confirmations;
        let built = self.build_request_from(offer.rate_per_kvb, now, |coins| {
            merge_pool(coins, min_confirmations).0
        });
        let (why, session) = match built {
            Ok(built) => match built.skipped.iter().find(|s| s.id == id) {
                None => return Ok(built),
                Some(skipped) => (
                    MergeRefused::NotBuilt(skipped.reason.clone()),
                    Some(built.session_id),
                ),
            },
            Err(e) => (MergeRefused::Node(e), None),
        };
        // Not built: the session that came out without the merge is given
        // up and the merge entry goes, so that the queue is as it was.
        let write = self.queue_writer();
        if let Some(session) = session {
            if let Err(e) = self.owner.abandon_session(&session, &write) {
                self.note(format!(
                    "the session without the merge was not given up: {e}"
                ));
            }
        }
        let write = self.queue_writer();
        if let Err(e) = self.owner.cancel(id, &write) {
            self.note(format!(
                "the merge entry could not be taken out of the queue: {e}"
            ));
        }
        Err(why)
    }
}
