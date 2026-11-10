//! Sections 4 and 5: the evidence for one sale.
//!
//! A [`SaleWatch`] holds what has been reported about the transactions that
//! pay one sale: who reported them, whether any block holds them, whether
//! they conflict. [`SaleWatch::status`] works out what the rules say about
//! the sale right now. Nothing here sees a clock or a chain except what the
//! caller passes in.

use std::collections::BTreeSet;

use bitcoin::consensus::deserialize;
use bitcoin::{BlockHash, OutPoint, ScriptBuf, Transaction, TxOut, Txid};
use openbmt_core::policy::{classify_replacement, CheckMode, ReplacementKind};

use crate::chain::{confirmations, HeaderChain};
use crate::netgroup::{network_group, NetGroup, PeerAddr};
use crate::proof::ProvenPayment;
use crate::{Refusal, MAX_SALE_TRANSACTIONS, MAX_TOLERANCE_BPS, MAX_TX_BYTES, SEEN_GROUPS};

/// Where a sighting of an unconfirmed transaction came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// A Bitcoin peer, with its address.
    Peer(PeerAddr),
    /// The shop's Box. It is never counted as a peer (WR-41).
    Box,
}

/// What a sighting did to the sale (WR-40, WR-42).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sighted {
    /// The transaction pays the sale nothing and conflicts with nothing the
    /// sale knows. It was ignored.
    Ignored,
    /// A new transaction, kept.
    New,
    /// The sale already knew this transaction. The source was recorded.
    Known,
    /// A fee bump: it took the place of the transactions it conflicts with.
    Replaced,
    /// It conflicts with a known transaction and is not a fee bump. Both are
    /// kept and the conflict flag is set.
    Conflicting,
}

/// Everything the rules say about a sale at one moment (see
/// [`SaleWatch::status`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SaleStatus {
    /// WR-31: the sum of what the standing transactions pay the sale.
    pub received_sats: u64,
    /// What standing transactions with at least one confirmation now pay.
    pub confirmed_sats: u64,
    /// WR-32: the largest depth at which the sale is confirmed; 0 if it is
    /// not confirmed.
    pub min_confirmations: u32,
    /// WR-42: a conflict was seen. Once true, always true.
    pub conflict_seen: bool,
    /// WR-45: how the standing unconfirmed payment was checked, `None` if
    /// there is nothing to accept at once (the sale waits for a
    /// confirmation).
    pub mode: Option<CheckMode>,
    /// The network groups that reported the standing unconfirmed payment.
    pub peer_groups: usize,
    /// WR-46: seconds since the payment was first seen.
    pub secs_since_first_seen: u64,
    /// WR-33: a transaction that was confirmed at the last status is not
    /// confirmed now. True for one status only.
    pub unconfirmed_again: bool,
    /// WR-34: the sale refused a transaction and needs a person.
    pub needs_a_person: bool,
}

/// What is known about one transaction of the sale.
#[derive(Debug, Clone)]
struct Entry {
    txid: Txid,
    tx: Transaction,
    /// WR-30: what it pays the sale.
    paid: u64,
    groups: BTreeSet<NetGroup>,
    by_box: bool,
    /// WR-46.
    first_seen: Option<u64>,
    /// Blocks that held it, whether or not they are still on the chain.
    blocks: Vec<BlockHash>,
}

impl Entry {
    fn spends(&self, outpoint: &OutPoint) -> bool {
        self.tx.input.iter().any(|i| &i.previous_output == outpoint)
    }

    fn conflicts_with(&self, other: &Transaction) -> bool {
        other.input.iter().any(|i| self.spends(&i.previous_output))
    }

    /// WR-44, WR-45.
    fn mode(&self) -> Option<CheckMode> {
        let seen = self.groups.len() >= SEEN_GROUPS;
        match (self.by_box, seen) {
            (true, true) => Some(CheckMode::NodeAlsoSeenByLight),
            (false, true) => Some(CheckMode::Light),
            (true, false) => Some(CheckMode::BoxReportsOnly),
            (false, false) => None,
        }
    }
}

/// The evidence for one sale.
#[derive(Debug, Clone)]
pub struct SaleWatch {
    script: ScriptBuf,
    sale_sats: u64,
    tolerance_bps: u32,
    entries: Vec<Entry>,
    conflict: bool,
    needs_a_person: bool,
    /// The latest time a sighting was recorded at.
    latest: u64,
    /// Transactions in blocks that spend a coin of the sale's transactions
    /// and are not the sale's transactions.
    elsewhere: Vec<(OutPoint, Txid, BlockHash)>,
    /// The transactions that were confirmed at the last status.
    last_confirmed: BTreeSet<Txid>,
}

/// WR-30: the sum of all outputs whose script is exactly `script`.
fn paid_to(outputs: &[TxOut], script: &ScriptBuf) -> u64 {
    outputs
        .iter()
        .filter(|o| &o.script_pubkey == script)
        .fold(0u64, |sum, o| sum.saturating_add(o.value.to_sat()))
}

impl SaleWatch {
    /// A watch for a sale of `sale_sats` paid to `script`. `tolerance_bps`
    /// is the shop's tolerance in basis points of the sale amount; above
    /// 10,000 it is taken as 10,000.
    pub fn new(script: ScriptBuf, sale_sats: u64, tolerance_bps: u32) -> Self {
        Self {
            script,
            sale_sats,
            tolerance_bps: tolerance_bps.min(MAX_TOLERANCE_BPS),
            entries: Vec::new(),
            conflict: false,
            needs_a_person: false,
            latest: 0,
            elsewhere: Vec::new(),
            last_confirmed: BTreeSet::new(),
        }
    }

    /// WR-40: a sighting of an unconfirmed transaction, in full, with where
    /// it came from and the caller's clock in seconds.
    ///
    /// A transaction that pays the sale nothing and conflicts with nothing
    /// known is ignored. A refusal changes nothing, except that a
    /// transaction the sale cannot keep marks it "needs a person" (WR-34).
    ///
    /// The time recorded is never earlier than a time recorded before
    /// (WR-46).
    pub fn sighting(
        &mut self,
        tx_bytes: &[u8],
        source: Source,
        now: u64,
    ) -> Result<Sighted, Refusal> {
        let tx: Transaction = deserialize(tx_bytes).map_err(|_| Refusal::NotATransaction)?;
        let at = now.max(self.latest);
        self.latest = at;
        self.admit(tx, Some((source, at)), None)
    }

    /// A payment proven in a block, from `verify_box_proof` or
    /// `payments_in_block`. Whether the block is on the chain is worked out
    /// again at every status (WR-33).
    ///
    /// A transaction that pays the sale nothing and conflicts with nothing
    /// known is refused (WR-30). One that conflicts with a known
    /// transaction is handled as in WR-42.
    pub fn confirmed(&mut self, proven: ProvenPayment) -> Result<(), Refusal> {
        match self.admit(proven.tx, None, Some(proven.block))? {
            Sighted::Ignored => Err(Refusal::PaysNothing),
            _ => Ok(()),
        }
    }

    /// The last sentence of WR-42: a block on the chain holds `by`, a
    /// transaction that spends `outpoint` and is not one of the sale's
    /// transactions. The conflict flag is set for good. While `block` is on
    /// the chain, the sale's transactions that spend `outpoint` are displaced
    /// (WR-43).
    ///
    /// Coins the sale does not watch are ignored; see
    /// [`SaleWatch::watched_outpoints`].
    pub fn spent_elsewhere(&mut self, outpoint: OutPoint, by: Txid, block: BlockHash) {
        if self.entries.iter().any(|e| e.txid == by) {
            return;
        }
        if !self.entries.iter().any(|e| e.spends(&outpoint)) {
            return;
        }
        self.conflict = true;
        let record = (outpoint, by, block);
        if !self.elsewhere.contains(&record) {
            self.elsewhere.push(record);
        }
    }

    /// The coins that the sale's known transactions spend. Give them to
    /// `spends_in_block` to look for the spends of the last sentence of
    /// WR-42.
    pub fn watched_outpoints(&self) -> Vec<OutPoint> {
        let set: BTreeSet<OutPoint> = self
            .entries
            .iter()
            .flat_map(|e| e.tx.input.iter().map(|i| i.previous_output))
            .collect();
        set.into_iter().collect()
    }

    /// The transactions the sale keeps, in full (WR-34).
    pub fn transactions(&self) -> impl Iterator<Item = &Transaction> {
        self.entries.iter().map(|e| &e.tx)
    }

    /// Blocks that spent a coin of the sale's transactions with another
    /// transaction, as given to [`SaleWatch::spent_elsewhere`].
    pub fn spent_elsewhere_in(&self) -> &[(OutPoint, Txid, BlockHash)] {
        &self.elsewhere
    }

    /// Everything the rules say about the sale right now.
    ///
    /// Confirmation is worked out again from the chain at every call
    /// (WR-33). The call takes `&mut self` because `unconfirmed_again`
    /// compares with the answer of the last call and so needs the memory of
    /// it: it is true for the first status after a confirmed transaction
    /// stopped being confirmed, and false for the next.
    pub fn status<C: HeaderChain + ?Sized>(&mut self, chain: &C, now: u64) -> SaleStatus {
        let confs: Vec<u32> = self
            .entries
            .iter()
            .map(|e| {
                e.blocks
                    .iter()
                    .filter_map(|b| confirmations(chain, b))
                    .max()
                    .unwrap_or(0)
            })
            .collect();
        let confirmed_outpoints: Vec<OutPoint> = self
            .elsewhere
            .iter()
            .filter(|(_, by, block)| {
                confirmations(chain, block).is_some() && !self.entries.iter().any(|e| e.txid == *by)
            })
            .map(|(outpoint, _, _)| *outpoint)
            .collect();

        // WR-43: displaced while a conflicting transaction is confirmed.
        let standing: Vec<bool> = (0..self.entries.len())
            .map(|i| {
                let by_known = (0..self.entries.len()).any(|j| {
                    j != i && confs[j] >= 1 && self.entries[j].conflicts_with(&self.entries[i].tx)
                });
                let by_other = confirmed_outpoints
                    .iter()
                    .any(|o| self.entries[i].spends(o));
                !(by_known || by_other)
            })
            .collect();

        // WR-31.
        let received_sats = self.sum(|i| standing[i]);
        let confirmed_sats = self.sum(|i| standing[i] && confs[i] >= 1);

        // WR-32.
        let tolerance =
            (u128::from(self.sale_sats) * u128::from(self.tolerance_bps) / 10_000) as u64;
        let required = self.sale_sats - tolerance;
        let mut depths: Vec<u32> = (0..self.entries.len())
            .filter(|&i| standing[i] && confs[i] >= 1)
            .map(|i| confs[i])
            .collect();
        depths.sort_unstable_by(|a, b| b.cmp(a));
        depths.dedup();
        let min_confirmations = depths
            .into_iter()
            .find(|&d| {
                let sum = self.sum(|i| standing[i] && confs[i] >= d);
                sum > 0 && sum >= required
            })
            .unwrap_or(0);

        // WR-44 to WR-46, for the weakest standing unconfirmed payment: the
        // fewest groups, then the latest first-seen time.
        let weakest = (0..self.entries.len())
            .filter(|&i| standing[i] && confs[i] == 0 && self.entries[i].paid > 0)
            .min_by_key(|&i| {
                let e = &self.entries[i];
                (
                    e.groups.len(),
                    std::cmp::Reverse(e.first_seen.unwrap_or(u64::MAX)),
                )
            })
            .map(|i| &self.entries[i]);
        let (mode, peer_groups, secs_since_first_seen) = match weakest {
            Some(e) => (
                e.mode(),
                e.groups.len(),
                e.first_seen.map_or(0, |t| now.saturating_sub(t)),
            ),
            None => (None, 0, 0),
        };

        // WR-33.
        let now_confirmed: BTreeSet<Txid> = (0..self.entries.len())
            .filter(|&i| confs[i] >= 1)
            .map(|i| self.entries[i].txid)
            .collect();
        let unconfirmed_again = self
            .last_confirmed
            .difference(&now_confirmed)
            .next()
            .is_some();
        self.last_confirmed = now_confirmed;

        SaleStatus {
            received_sats,
            confirmed_sats,
            min_confirmations,
            conflict_seen: self.conflict,
            mode,
            peer_groups,
            secs_since_first_seen,
            unconfirmed_again,
            needs_a_person: self.needs_a_person,
        }
    }

    /// The sum of what the chosen transactions pay.
    fn sum(&self, keep: impl Fn(usize) -> bool) -> u64 {
        self.entries
            .iter()
            .enumerate()
            .filter(|(i, _)| keep(*i))
            .fold(0u64, |sum, (_, e)| sum.saturating_add(e.paid))
    }

    /// Take a transaction into the sale: WR-40, WR-42, WR-34.
    fn admit(
        &mut self,
        tx: Transaction,
        sighted: Option<(Source, u64)>,
        block: Option<BlockHash>,
    ) -> Result<Sighted, Refusal> {
        let txid = tx.compute_txid();

        // A transaction the sale knows: record the source or the block.
        if let Some(entry) = self.entries.iter_mut().find(|e| e.txid == txid) {
            if let Some((source, at)) = sighted {
                record_source(entry, source, at);
            }
            if let Some(block) = block {
                if !entry.blocks.contains(&block) {
                    entry.blocks.push(block);
                }
            }
            return Ok(Sighted::Known);
        }

        // WR-40.
        let paid = paid_to(&tx.output, &self.script);
        let conflicts: Vec<usize> = (0..self.entries.len())
            .filter(|&i| self.entries[i].conflicts_with(&tx))
            .collect();
        if paid == 0 && conflicts.is_empty() {
            return Ok(Sighted::Ignored);
        }

        // WR-34.
        let len = tx.total_size();
        if len > MAX_TX_BYTES {
            self.needs_a_person = true;
            return Err(Refusal::TransactionTooLarge { len });
        }

        // WR-42: a fee bump only if it is one for every transaction it
        // conflicts with, and none of them was ever confirmed.
        let bump = !conflicts.is_empty()
            && conflicts.iter().all(|&i| {
                let old = &self.entries[i];
                old.blocks.is_empty()
                    && classify_replacement(&old.tx.output, &tx.output, &self.script)
                        == ReplacementKind::FeeBump
            });
        let kept = self.entries.len() - if bump { conflicts.len() } else { 0 };
        if kept + 1 > MAX_SALE_TRANSACTIONS {
            self.needs_a_person = true;
            return Err(Refusal::TooManyTransactions);
        }

        let mut entry = Entry {
            txid,
            tx,
            paid,
            groups: BTreeSet::new(),
            by_box: false,
            first_seen: None,
            blocks: block.into_iter().collect(),
        };
        if let Some((source, at)) = sighted {
            record_source(&mut entry, source, at);
        }
        let kind = if bump {
            let replaced: BTreeSet<usize> = conflicts.into_iter().collect();
            let mut index = 0;
            self.entries.retain(|_| {
                index += 1;
                !replaced.contains(&(index - 1))
            });
            Sighted::Replaced
        } else if conflicts.is_empty() {
            Sighted::New
        } else {
            self.conflict = true;
            Sighted::Conflicting
        };
        self.entries.push(entry);
        Ok(kind)
    }
}

/// WR-41, WR-44, WR-46: record who reported a transaction, and start its
/// clock when it first counts as seen or reported by the Box.
fn record_source(entry: &mut Entry, source: Source, at: u64) {
    match source {
        Source::Peer(addr) => {
            entry.groups.insert(network_group(&addr));
        }
        Source::Box => entry.by_box = true,
    }
    if entry.first_seen.is_none() && (entry.by_box || entry.groups.len() >= SEEN_GROUPS) {
        entry.first_seen = Some(at);
    }
}
