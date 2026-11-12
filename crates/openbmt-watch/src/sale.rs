//! Sections 4 and 5: the evidence for one sale.
//!
//! A [`SaleWatch`] holds what has been reported about the transactions that
//! pay one sale: who reported them, whether any block holds them, whether
//! they conflict. [`SaleWatch::status`] works out what the rules say about
//! the sale right now. Nothing here sees a clock or a chain except what the
//! caller passes in.

use std::collections::BTreeSet;

use bitcoin::consensus::{deserialize, serialize};
use bitcoin::hashes::Hash;
use bitcoin::{BlockHash, OutPoint, Script, ScriptBuf, Transaction, TxOut, Txid};
use openbmt_core::policy::{classify_replacement, CheckMode, ReplacementKind};
use openbmt_core::wire::{self, DecodeError, Reader};

use crate::chain::{confirmations, HeaderChain};
use crate::netgroup::{network_group, NetGroup, PeerAddr};
use crate::node_only::NodeOnlyHealth;
use crate::own_node::OwnNodeHealth;
use crate::proof::ProvenPayment;
use crate::{
    Refusal, MAX_ENTRY_BLOCKS, MAX_ENTRY_GROUPS, MAX_REPLACED_REMEMBERED, MAX_SALE_TRANSACTIONS,
    MAX_SCRIPT_BYTES, MAX_TOLERANCE_BPS, MAX_TX_BYTES, SEEN_GROUPS,
};

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
    /// sale knows, or a fee bump took its place earlier. It was ignored.
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
    /// WR-61: the own node has reported the standing unconfirmed payment.
    /// True whether or not the node can be used now (WR-63).
    pub own_node_saw: bool,
    /// WR-62: the own node's report is what makes `mode` the own-node
    /// check: the own node reported it, the node can be used now and the
    /// light client saw it too. False with `own_node_saw` true means the
    /// counter fell back to the light check (WR-64); the health line for
    /// the own node says why.
    pub own_node_counted: bool,
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
    /// WR-61: the own node reported it. Not stored (WR-65).
    by_own_node: bool,
    /// WR-66: when the own node first reported it, if the caller said. Not
    /// stored: after a restart the node reports again and the time starts
    /// again, the safe side.
    own_reported: Option<u64>,
    /// WR-46.
    first_seen: Option<u64>,
    /// Blocks that held it, whether or not they are still on the chain.
    blocks: Vec<BlockHash>,
    /// The caller's clock when a block first held it, if the caller gave
    /// one (see [`SaleWatch::confirmed_at`]).
    learned: Option<u64>,
}

/// One standing payment as the last status found it.
#[derive(Debug, Clone, Copy)]
struct Standing {
    txid: Txid,
    paid: u64,
    /// WR-31: when it was first seen, or the block that held it was first
    /// learned of when it never counted as seen.
    at: Option<u64>,
}

impl Entry {
    /// WR-46, WR-66: when the time since first seen starts: the first
    /// sighting that made the transaction seen and, with `node_only` on, the
    /// own node's first report if it came earlier.
    fn clock_start(&self, node_only: bool) -> Option<u64> {
        match (
            self.first_seen,
            node_only.then_some(self.own_reported).flatten(),
        ) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }

    fn spends(&self, outpoint: &OutPoint) -> bool {
        self.tx.input.iter().any(|i| &i.previous_output == outpoint)
    }

    fn conflicts_with(&self, other: &Transaction) -> bool {
        other.input.iter().any(|i| self.spends(&i.previous_output))
    }

    /// WR-66, WR-68: the mode with `node_only` on. The own node's report
    /// while it can be used; else, with `light_fallback`, the light client's
    /// three groups (WR-44); else none.
    fn node_only_mode(&self, own_usable: bool, light_fallback: bool) -> Option<CheckMode> {
        if self.by_own_node && own_usable {
            Some(CheckMode::OwnNodeOnly)
        } else if light_fallback && self.groups.len() >= SEEN_GROUPS {
            Some(CheckMode::Light)
        } else {
            None
        }
    }

    /// WR-44, WR-45, WR-62.
    fn mode(&self, own_usable: bool) -> Option<CheckMode> {
        let seen = self.groups.len() >= SEEN_GROUPS;
        let node = self.by_box || (self.by_own_node && own_usable);
        match (seen, node, self.by_box) {
            (true, true, _) => Some(CheckMode::NodeAlsoSeenByLight),
            (true, false, _) => Some(CheckMode::Light),
            (false, _, true) => Some(CheckMode::BoxReportsOnly),
            (false, _, false) => None,
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
    /// Transactions a fee bump took the place of. A late sighting of one is
    /// not a new conflict.
    replaced: BTreeSet<Txid>,
    /// The payments that counted at the last status, ascending by id. Not
    /// stored: every status works it out again.
    last_standing: Vec<Standing>,
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
            replaced: BTreeSet::new(),
            last_standing: Vec::new(),
        }
    }

    /// The script the sale is paid to.
    pub fn script(&self) -> &Script {
        self.script.as_script()
    }

    /// The amount the sale is judged against.
    pub fn sale_sats(&self) -> u64 {
        self.sale_sats
    }

    /// The shop's tolerance this watch was made with, in basis points of the
    /// sale amount (never above 10,000).
    pub fn tolerance_bps(&self) -> u32 {
        self.tolerance_bps
    }

    /// The counter moves the amount when the price lock is renewed. Evidence
    /// does not change.
    pub fn set_sale_sats(&mut self, sale_sats: u64) {
        self.sale_sats = sale_sats;
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
        self.take_proven(proven, None)
    }

    /// As [`SaleWatch::confirmed`], with the caller's clock in seconds: the
    /// time the caller learned of the block. A payment that was never seen
    /// before a block held it has that time as its first-seen time in
    /// [`SaleWatch::standing_first_seen`] (WR-31).
    pub fn confirmed_at(&mut self, proven: ProvenPayment, now: u64) -> Result<(), Refusal> {
        self.take_proven(proven, Some(now))
    }

    fn take_proven(&mut self, proven: ProvenPayment, learned: Option<u64>) -> Result<(), Refusal> {
        let txid = proven.tx.compute_txid();
        let result = self.admit(proven.tx, None, Some(proven.block))?;
        if result == Sighted::Ignored {
            return Err(Refusal::PaysNothing);
        }
        if let Some(entry) = self.entries.iter_mut().find(|e| e.txid == txid) {
            if entry.learned.is_none() {
                entry.learned = learned;
            }
        }
        Ok(())
    }

    /// WR-61: the own node reports that it holds this transaction in its
    /// memory pool. Returns whether the sale knows the transaction; if it does
    /// not, nothing is recorded, so the caller asks the node again once the
    /// light client has sighted it (WR-65).
    ///
    /// The report is not a sighting: it starts no clock (WR-46), counts in no
    /// network group (WR-60) and changes no amount (WR-31).
    pub fn own_node_saw(&mut self, txid: Txid) -> bool {
        match self.entries.iter_mut().find(|e| e.txid == txid) {
            Some(entry) => {
                entry.by_own_node = true;
                true
            }
            None => false,
        }
    }

    /// As [`SaleWatch::own_node_saw`], with the caller's clock at the
    /// report. With `node_only` on (WR-66) the conflict watch of section 6
    /// counts from the first such report: the node's announcements stand in
    /// for the peers'. In the mode with the light client the time is not
    /// used (WR-46).
    pub fn own_node_saw_at(&mut self, txid: Txid, at: u64) -> bool {
        match self.entries.iter_mut().find(|e| e.txid == txid) {
            Some(entry) => {
                entry.by_own_node = true;
                entry.own_reported.get_or_insert(at);
                true
            }
            None => false,
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
        self.status_with_own_node(chain, now, &OwnNodeHealth::not_configured())
    }

    /// As [`SaleWatch::status`], with the health of the own node (WR-62,
    /// WR-63). The own node's report counts toward the mode only while
    /// `own.usable` holds; a sale it reported while it was down or behind
    /// gets the light check's mode, and `own_node_counted` says so.
    pub fn status_with_own_node<C: HeaderChain + ?Sized>(
        &mut self,
        chain: &C,
        now: u64,
        own: &OwnNodeHealth,
    ) -> SaleStatus {
        self.status_inner(chain, now, own, None)
    }

    /// As [`SaleWatch::status`], with `node_only` on (WR-66 to WR-68). "Seen
    /// by the light client" is not used: a payment the own node holds while
    /// it can be used has the mode `CheckMode::OwnNodeOnly`, and
    /// `own_node_counted` says the node is what makes it. With
    /// `health.light_fallback` on, a payment that the random peers
    /// reported from enough network groups (WR-44) has the light check's
    /// mode when the node does not make the mode, and keeps it after the
    /// node returns. A payment that neither holds has no mode. The Box's
    /// report gives no mode here.
    pub fn status_node_only<C: HeaderChain + ?Sized>(
        &mut self,
        chain: &C,
        now: u64,
        health: &NodeOnlyHealth,
    ) -> SaleStatus {
        self.status_inner(chain, now, &health.own, Some(health.light_fallback))
    }

    /// `node_only` is `Some(light_fallback)` in the mode of WR-66.
    fn status_inner<C: HeaderChain + ?Sized>(
        &mut self,
        chain: &C,
        now: u64,
        own: &OwnNodeHealth,
        node_only: Option<bool>,
    ) -> SaleStatus {
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

        // WR-31: standing transactions that conflict with each other count
        // once, at the smallest amount any of them pays.
        let counted = self.counted(&standing);
        let received_sats = self.sum(|i| counted.contains(&i));
        let mut last: Vec<Standing> = counted
            .iter()
            .filter(|&&i| self.entries[i].paid > 0)
            .map(|&i| {
                let e = &self.entries[i];
                Standing {
                    txid: e.txid,
                    paid: e.paid,
                    at: e.first_seen.or(e.learned),
                }
            })
            .collect();
        last.sort_unstable_by_key(|p| p.txid.to_byte_array());
        self.last_standing = last;
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
                match node_only {
                    Some(light_fallback) => e.node_only_mode(own.usable, light_fallback),
                    None => e.mode(own.usable),
                },
                e.groups.len(),
                e.clock_start(node_only.is_some())
                    .map_or(0, |t| now.saturating_sub(t)),
            ),
            None => (None, 0, 0),
        };
        // WR-61, WR-62.
        let own_node_saw = weakest.is_some_and(|e| e.by_own_node);
        let own_node_counted = weakest.is_some_and(|e| {
            e.by_own_node && own.usable && (node_only.is_some() || e.groups.len() >= SEEN_GROUPS)
        });

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
            own_node_saw,
            own_node_counted,
        }
    }

    /// WR-31: the transactions that make up the received amount: each group
    /// of standing transactions joined by conflicts is represented by the
    /// one that pays the least (the smaller id when two pay the same). Only
    /// one transaction of such a group can confirm.
    fn counted(&self, standing: &[bool]) -> BTreeSet<usize> {
        let n = self.entries.len();
        let mut group: Vec<usize> = (0..n).collect();
        // At most 20 entries: joining by repeated passes is enough.
        loop {
            let mut changed = false;
            for i in 0..n {
                for j in (i + 1)..n {
                    if standing[i]
                        && standing[j]
                        && group[i] != group[j]
                        && self.entries[i].conflicts_with(&self.entries[j].tx)
                    {
                        let (keep, drop) = (group[i].min(group[j]), group[i].max(group[j]));
                        for g in &mut group {
                            if *g == drop {
                                *g = keep;
                            }
                        }
                        changed = true;
                    }
                }
            }
            if !changed {
                break;
            }
        }
        (0..n)
            .filter_map(|g| {
                (0..n)
                    .filter(|&i| standing[i] && group[i] == g)
                    .min_by_key(|&i| (self.entries[i].paid, self.entries[i].txid.to_byte_array()))
            })
            .collect()
    }

    /// WR-31: the standing transactions that pay the sale and what each
    /// pays, ascending by id, as the last [`SaleWatch::status`] found them.
    /// Their sum is that status's `received_sats`. Empty before the first
    /// status, and after a restart until the next one.
    pub fn standing(&self) -> Vec<(Txid, u64)> {
        self.last_standing
            .iter()
            .map(|p| (p.txid, p.paid))
            .collect()
    }

    /// WR-31: the earliest time any payment of [`SaleWatch::standing`] was
    /// first seen; for a payment that was never seen before a block held
    /// it, the time the caller gave to [`SaleWatch::confirmed_at`]. `None`
    /// when no standing payment has a time.
    pub fn standing_first_seen(&self) -> Option<u64> {
        self.last_standing.iter().filter_map(|p| p.at).min()
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
                    if entry.blocks.len() >= MAX_ENTRY_BLOCKS {
                        entry.blocks.remove(0);
                    }
                    entry.blocks.push(block);
                }
            }
            return Ok(Sighted::Known);
        }

        // WR-42: an unconfirmed transaction that a fee bump already took
        // the place of. A peer that is slow to hear of the bump still relays
        // it; that is not a new conflict. In a block it is a different
        // matter and goes on.
        if block.is_none() && self.replaced.contains(&txid) {
            return Ok(Sighted::Ignored);
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
            by_own_node: false,
            own_reported: None,
            first_seen: None,
            blocks: block.into_iter().collect(),
            learned: None,
        };
        if let Some((source, at)) = sighted {
            record_source(&mut entry, source, at);
        }
        let kind = if bump {
            let replaced: BTreeSet<usize> = conflicts.into_iter().collect();
            for &i in &replaced {
                if self.replaced.len() < MAX_REPLACED_REMEMBERED {
                    self.replaced.insert(self.entries[i].txid);
                }
            }
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

/// Require `key` to sort strictly after the previous one in a list.
fn ascending<const N: usize>(
    last: &mut Option<[u8; N]>,
    key: [u8; N],
    why: &'static str,
) -> Result<(), DecodeError> {
    if last.is_some_and(|prev| prev >= key) {
        return Err(DecodeError::NonCanonical(why));
    }
    *last = Some(key);
    Ok(())
}

impl Entry {
    fn write(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&self.txid.to_byte_array());
        let tx = serialize(&self.tx);
        out.extend_from_slice(&wire::len_u32(tx.len()).to_le_bytes());
        out.extend_from_slice(&tx);
        out.extend_from_slice(&self.paid.to_le_bytes());
        out.extend_from_slice(&wire::len_u32(self.groups.len()).to_le_bytes());
        for g in &self.groups {
            out.extend_from_slice(&g.key());
        }
        out.push(u8::from(self.by_box));
        wire::put_opt_u64(out, self.first_seen);
        let mut blocks: Vec<[u8; 32]> = self.blocks.iter().map(|b| b.to_byte_array()).collect();
        blocks.sort_unstable();
        out.extend_from_slice(&wire::len_u32(blocks.len()).to_le_bytes());
        for b in &blocks {
            out.extend_from_slice(b);
        }
        wire::put_opt_u64(out, self.learned);
    }

    fn read(r: &mut Reader<'_>, script: &ScriptBuf, latest: u64) -> Result<Self, DecodeError> {
        let txid = Txid::from_byte_array(r.array()?);
        let tx_len = usize::try_from(r.u32_le()?).map_err(|_| DecodeError::TooLarge)?;
        if tx_len > MAX_TX_BYTES {
            return Err(DecodeError::TooLarge);
        }
        let tx_bytes = r.take(tx_len)?;
        let tx: Transaction = deserialize(tx_bytes)
            .map_err(|_| DecodeError::NonCanonical("a stored transaction does not parse"))?;
        if serialize(&tx) != tx_bytes {
            return Err(DecodeError::NonCanonical(
                "a stored transaction is not in its canonical form",
            ));
        }
        if tx.compute_txid() != txid {
            return Err(DecodeError::NonCanonical(
                "a stored transaction id is not the id of its transaction",
            ));
        }
        let paid = r.u64_le()?;
        if paid != paid_to(&tx.output, script) {
            return Err(DecodeError::NonCanonical(
                "a stored amount is not what its transaction pays the sale",
            ));
        }
        let n = r.count(MAX_ENTRY_GROUPS, 5)?;
        let mut groups = BTreeSet::new();
        let mut last: Option<[u8; 5]> = None;
        for _ in 0..n {
            let key = r.array::<5>()?;
            ascending(&mut last, key, "network groups are not in ascending order")?;
            let group = NetGroup::from_key(key)
                .ok_or(DecodeError::NonCanonical("a network group is not valid"))?;
            groups.insert(group);
        }
        let by_box = r.flag()?;
        let first_seen = r.opt_u64()?;
        let n = r.count(MAX_ENTRY_BLOCKS, 32)?;
        let mut blocks = Vec::with_capacity(n);
        let mut last: Option<[u8; 32]> = None;
        for _ in 0..n {
            let hash = r.array::<32>()?;
            ascending(&mut last, hash, "blocks are not in ascending order")?;
            blocks.push(BlockHash::from_byte_array(hash));
        }
        let learned = r.opt_u64()?;
        if learned.is_some() && blocks.is_empty() {
            return Err(DecodeError::NonCanonical(
                "a block time is stored for a transaction that no block held",
            ));
        }
        // The clock starts when a transaction first counts as seen or is
        // reported by the Box, and never before; it cannot be later than the
        // latest sighting.
        let counts = by_box || groups.len() >= SEEN_GROUPS;
        match first_seen {
            Some(at) if !counts || at > latest => {
                return Err(DecodeError::NonCanonical(
                    "a first-seen time does not fit the evidence",
                ))
            }
            None if counts => {
                return Err(DecodeError::NonCanonical(
                    "a seen transaction has no first-seen time",
                ))
            }
            _ => {}
        }
        Ok(Self {
            txid,
            tx,
            paid,
            groups,
            by_box,
            by_own_node: false,
            own_reported: None,
            first_seen,
            blocks,
            learned,
        })
    }
}

/// The bytes of a set of transaction ids, smallest first.
fn sorted_txids(set: &BTreeSet<Txid>) -> Vec<[u8; 32]> {
    let mut ids: Vec<[u8; 32]> = set.iter().map(|t| t.to_byte_array()).collect();
    ids.sort_unstable();
    ids
}

/// A set of transaction ids that must be in strictly ascending order.
fn read_txid_set(
    r: &mut Reader<'_>,
    max: usize,
    why: &'static str,
) -> Result<BTreeSet<Txid>, DecodeError> {
    let n = r.count(max, 32)?;
    let mut set = BTreeSet::new();
    let mut last: Option<[u8; 32]> = None;
    for _ in 0..n {
        let id = r.array::<32>()?;
        ascending(&mut last, id, why)?;
        set.insert(Txid::from_byte_array(id));
    }
    Ok(set)
}

/// One record of `elsewhere` as stored: the coin, the spending transaction
/// and the block.
fn elsewhere_bytes((outpoint, by, block): &(OutPoint, Txid, BlockHash)) -> [u8; 100] {
    let mut out = [0u8; 100];
    out[..32].copy_from_slice(&outpoint.txid.to_byte_array());
    out[32..36].copy_from_slice(&outpoint.vout.to_le_bytes());
    out[36..68].copy_from_slice(&by.to_byte_array());
    out[68..].copy_from_slice(&block.to_byte_array());
    out
}

impl SaleWatch {
    /// Snapshot: `OBMV`, version 1, then the script (u32 length and bytes),
    /// the sale amount (u64 LE), the tolerance (u32 LE), the conflict, needs
    /// a person flags, the latest time (u64 LE), the transactions in the
    /// order the sale keeps them, the transactions that were confirmed at the
    /// last status, the replaced transactions and the spends elsewhere.
    ///
    /// Each transaction is its id, its length (u32 LE) and consensus bytes,
    /// what it pays the sale (u64 LE), its network groups (a u32 count and
    /// the five-byte keys of [`NetGroup::key`]), the Box flag, the first-seen
    /// time (a flag byte and, when the flag is 1, a u64 LE), the blocks
    /// that held it (a u32 count and the hashes) and the time a block was
    /// first learned of (a flag byte and, when the flag is 1, a u64 LE;
    /// the flag is 0 when no block held it). Every set is written in
    /// ascending byte order. Each spend elsewhere is the coin (id and index
    /// u32 LE), the spending transaction and the block.
    pub fn to_bytes(&self) -> Vec<u8> {
        let script = self.script.as_bytes();
        let mut out = wire::start(&wire::MAGIC_SALE_WATCH, 64 + script.len());
        out.extend_from_slice(&wire::len_u32(script.len()).to_le_bytes());
        out.extend_from_slice(script);
        out.extend_from_slice(&self.sale_sats.to_le_bytes());
        out.extend_from_slice(&self.tolerance_bps.to_le_bytes());
        out.push(u8::from(self.conflict));
        out.push(u8::from(self.needs_a_person));
        out.extend_from_slice(&self.latest.to_le_bytes());
        out.extend_from_slice(&wire::len_u32(self.entries.len()).to_le_bytes());
        for e in &self.entries {
            e.write(&mut out);
        }
        for set in [&self.last_confirmed, &self.replaced] {
            let ids = sorted_txids(set);
            out.extend_from_slice(&wire::len_u32(ids.len()).to_le_bytes());
            for id in &ids {
                out.extend_from_slice(id);
            }
        }
        let mut elsewhere: Vec<[u8; 100]> = self.elsewhere.iter().map(elsewhere_bytes).collect();
        elsewhere.sort_unstable();
        out.extend_from_slice(&wire::len_u32(elsewhere.len()).to_le_bytes());
        for e in &elsewhere {
            out.extend_from_slice(e);
        }
        out
    }

    /// Restore a snapshot strictly (see `openbmt_core::wire`). Besides the
    /// framing it checks what the live code guarantees: each stored id and
    /// amount is worked out again from the stored transaction and the script;
    /// no id repeats; the caps of the live code ([`MAX_SALE_TRANSACTIONS`],
    /// [`MAX_TX_BYTES`], [`MAX_REPLACED_REMEMBERED`], [`MAX_ENTRY_GROUPS`],
    /// [`MAX_ENTRY_BLOCKS`], [`MAX_SCRIPT_BYTES`], [`MAX_TOLERANCE_BPS`])
    /// hold; every set is in strictly ascending order; a first-seen time is
    /// there exactly when the evidence says the clock started, and is not
    /// after the latest time; the transactions that were confirmed at the
    /// last status are transactions of the sale.
    ///
    /// The spends elsewhere come back in ascending byte order, which is not
    /// necessarily the order they were given in.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, DecodeError> {
        let mut r = Reader::framed(bytes, &wire::MAGIC_SALE_WATCH)?;
        let script_len = usize::try_from(r.u32_le()?).map_err(|_| DecodeError::TooLarge)?;
        if script_len > MAX_SCRIPT_BYTES {
            return Err(DecodeError::TooLarge);
        }
        let script = ScriptBuf::from_bytes(r.take(script_len)?.to_vec());
        let sale_sats = r.u64_le()?;
        let tolerance_bps = r.u32_le()?;
        if tolerance_bps > MAX_TOLERANCE_BPS {
            return Err(DecodeError::NonCanonical("tolerance is above 10000"));
        }
        let conflict = r.flag()?;
        let needs_a_person = r.flag()?;
        let latest = r.u64_le()?;

        let n = r.count(MAX_SALE_TRANSACTIONS, 54)?;
        let mut entries: Vec<Entry> = Vec::with_capacity(n);
        for _ in 0..n {
            let entry = Entry::read(&mut r, &script, latest)?;
            if entries.iter().any(|e| e.txid == entry.txid) {
                return Err(DecodeError::NonCanonical("a transaction is stored twice"));
            }
            entries.push(entry);
        }
        let last_confirmed = read_txid_set(
            &mut r,
            MAX_SALE_TRANSACTIONS,
            "confirmed transactions are not in ascending order",
        )?;
        if last_confirmed
            .iter()
            .any(|t| !entries.iter().any(|e| e.txid == *t))
        {
            return Err(DecodeError::NonCanonical(
                "a confirmed transaction is not a transaction of the sale",
            ));
        }
        let replaced = read_txid_set(
            &mut r,
            MAX_REPLACED_REMEMBERED,
            "replaced transactions are not in ascending order",
        )?;

        let n = r.count(usize::MAX, 100)?;
        let mut elsewhere = Vec::with_capacity(n);
        let mut last: Option<[u8; 100]> = None;
        for _ in 0..n {
            let record = r.array::<100>()?;
            ascending(
                &mut last,
                record,
                "spends elsewhere are not in ascending order",
            )?;
            let [txid @ .., v0, v1, v2, v3] =
                <[u8; 36]>::try_from(&record[..36]).map_err(|_| DecodeError::Truncated)?;
            let by = <[u8; 32]>::try_from(&record[36..68]).map_err(|_| DecodeError::Truncated)?;
            let block = <[u8; 32]>::try_from(&record[68..]).map_err(|_| DecodeError::Truncated)?;
            elsewhere.push((
                OutPoint {
                    txid: Txid::from_byte_array(txid),
                    vout: u32::from_le_bytes([v0, v1, v2, v3]),
                },
                Txid::from_byte_array(by),
                BlockHash::from_byte_array(block),
            ));
        }
        r.finish()?;
        Ok(Self {
            script,
            sale_sats,
            tolerance_bps,
            entries,
            conflict,
            needs_a_person,
            latest,
            elsewhere,
            last_confirmed,
            replaced,
            last_standing: Vec::new(),
        })
    }
}

/// WR-41, WR-44, WR-46: record who reported a transaction, and start its
/// clock when it first counts as seen or reported by the Box.
fn record_source(entry: &mut Entry, source: Source, at: u64) {
    match source {
        Source::Peer(addr) => {
            if entry.groups.len() < MAX_ENTRY_GROUPS {
                entry.groups.insert(network_group(&addr));
            }
        }
        Source::Box => entry.by_box = true,
    }
    if entry.first_seen.is_none() && (entry.by_box || entry.groups.len() >= SEEN_GROUPS) {
        entry.first_seen = Some(at);
    }
}
