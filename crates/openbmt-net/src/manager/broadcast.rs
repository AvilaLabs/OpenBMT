//! The transactions the app asks the manager to broadcast, and the evidence
//! that the network took them (NR-86 to NR-89).
//!
//! The manager holds each transaction the app gave it until the app calls
//! [`Manager::forget`]. It announces the transaction with `inv` by txid (the
//! counter does not negotiate wtxid relay, so a peer announces and asks by
//! txid too) to half of the ready peers, serves it when a peer asks with
//! `getdata`, and listens for the other half to announce it back. Nothing
//! here believes a peer: a peer that fetched a transaction may have thrown it
//! away, and a peer that announced it back has said so and shown nothing.
//!
//! With `node_only` on no other peer can announce a transaction back, so the
//! own node is asked for it instead (NR-112): once the node's relay delay is
//! over, `getdata` by txid is answered with the transaction only from its
//! memory pool, and the same transaction back, witness included, is
//! evidence that the node holds it.

use std::collections::BTreeSet;
use std::fmt;

use bitcoin::consensus::encode::{deserialize, serialize};
use bitcoin::{Transaction, Txid};
use openbmt_watch::NetGroup;

use super::{Manager, Output, PeerId};
use crate::Request;

/// NR-86: most transactions held for broadcast.
pub const MAX_HELD: usize = 64;
/// NR-86: heaviest transaction held, in weight units. The same bound as the
/// signer's `MAX_WEIGHT`.
pub const MAX_BROADCAST_WEIGHT: u64 = 400_000;
/// NR-87: network groups, other than those that were asked, that must
/// announce a transaction back for it to count as seen.
pub const SEEN_BACK_GROUPS: usize = 2;
/// NR-88: seconds after which a transaction that is neither seen back nor
/// in a block is reported as not seen.
pub const NOT_SEEN_SECS: u64 = 600;
/// NR-112: seconds after the own node was sent the announcement before it is
/// first asked for the transaction (its relay delay is 2 minutes).
pub const NODE_ASK_AFTER_SECS: u64 = 130;
/// NR-112: seconds between two requests to the own node for the same
/// transaction, until it answers.
pub const NODE_ASK_EVERY_SECS: u64 = 60;

/// What the peers have shown of one held transaction (NR-87, NR-88,
/// NR-112).
///
/// `SeenBack` and `HeldByNode` are two kinds of evidence that may come in
/// either order, and only with the light fallback running (NR-110) can both
/// come. The state follows whichever happened first, and the combined state
/// [`RelayState::SeenBackAndHeldByNode`] keeps both once both have. The
/// manager never goes from a combined or a single state back to `Sent`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelayState {
    /// Announced, and nothing has come back yet.
    Sent,
    /// Peers that were not asked have announced it back, from this many
    /// network groups (at least [`SEEN_BACK_GROUPS`]).
    SeenBack {
        /// Distinct network groups that announced it back.
        groups: usize,
    },
    /// NR-112: the own node answered a `getdata` for it with the same
    /// transaction, witness included: it is in the node's memory pool. Only
    /// in the mode of NR-106.
    HeldByNode,
    /// Both of the above: peers announced it back from `groups` network
    /// groups and the own node holds it. Only with the light fallback
    /// running.
    SeenBackAndHeldByNode {
        /// Distinct network groups that announced it back.
        groups: usize,
    },
    /// [`NOT_SEEN_SECS`] passed with no two groups announcing it back and
    /// the own node not answering. It has been announced to every ready peer
    /// again (to the own node alone in the mode of NR-106).
    NotSeen,
}

/// Why [`Manager::broadcast`] did not hold a transaction (NR-86).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BroadcastRefused {
    /// The bytes are not one transaction.
    Malformed,
    /// The transaction weighs more than [`MAX_BROADCAST_WEIGHT`].
    TooHeavy {
        /// Its weight in weight units.
        weight: u64,
    },
    /// [`MAX_HELD`] transactions are held already.
    Full,
}

impl fmt::Display for BroadcastRefused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Malformed => write!(f, "NR-86: the bytes are not one transaction"),
            Self::TooHeavy { weight } => write!(
                f,
                "NR-86: the transaction weighs {weight} units; the limit is {MAX_BROADCAST_WEIGHT}"
            ),
            Self::Full => write!(f, "NR-86: {MAX_HELD} transactions are held already"),
        }
    }
}

impl std::error::Error for BroadcastRefused {}

/// A transaction held for broadcast.
#[derive(Debug)]
pub(super) struct Held {
    tx: Transaction,
    /// Peers that were sent an `inv` for it.
    asked: BTreeSet<PeerId>,
    /// Peers that asked for it and were sent it.
    fetched: BTreeSet<PeerId>,
    /// Groups of the peers that were not asked and announced it back.
    seen: BTreeSet<NetGroup>,
    /// When it was announced, or announced again.
    since: u64,
    /// NR-112: when the own node was last sent the announcement.
    node_sent: Option<u64>,
    /// NR-112: when the own node was last asked for it since then.
    node_asked_at: Option<u64>,
    /// NR-112: the own node has been asked at least once, so a `tx` from it
    /// is an answer and not unasked data.
    node_asked: bool,
    /// NR-112: the own node answered with the same transaction.
    by_node: bool,
    state: RelayState,
    /// The counts and the state last reported.
    reported: Option<(usize, usize, RelayState)>,
}

impl Held {
    /// The state that the evidence so far makes: peers that announced it
    /// back (NR-87) and the own node that holds it (NR-112), whichever came
    /// first and both once both have. With neither, the state stays.
    fn refresh_state(&mut self) {
        let seen = self.seen.len() >= SEEN_BACK_GROUPS;
        let groups = self.seen.len();
        self.state = match (seen, self.by_node) {
            (true, true) => RelayState::SeenBackAndHeldByNode { groups },
            (true, false) => RelayState::SeenBack { groups },
            (false, true) => RelayState::HeldByNode,
            (false, false) => self.state,
        };
    }
}

impl Manager {
    /// NR-86: hold a transaction and announce it. It is announced to half
    /// of the ready peers, at least one, and to each peer that becomes ready
    /// later. Giving the same transaction again changes nothing. The
    /// transaction's ID is returned. What the peers do with it is reported
    /// by [`Output::TxRelayed`] and [`Output::TxState`].
    pub fn broadcast(&mut self, tx: &[u8]) -> Result<Txid, BroadcastRefused> {
        let tx: Transaction = deserialize(tx).map_err(|_| BroadcastRefused::Malformed)?;
        let weight = tx.weight().to_wu();
        if weight > MAX_BROADCAST_WEIGHT {
            return Err(BroadcastRefused::TooHeavy { weight });
        }
        let txid = tx.compute_txid();
        if self.held.contains_key(&txid) {
            return Ok(txid);
        }
        if self.held.len() >= MAX_HELD {
            return Err(BroadcastRefused::Full);
        }
        // NR-109: with the own node alone, a transaction goes to it only.
        let ready: Vec<PeerId> = self
            .ready_peers()
            .into_iter()
            .filter(|id| !self.node.on || self.peers.get(id).is_some_and(|p| p.own))
            .collect();
        let take = (ready.len() / 2).max(1);
        let first: Vec<PeerId> = ready.into_iter().take(take).collect();
        self.held.insert(
            txid,
            Held {
                tx,
                asked: BTreeSet::new(),
                fetched: BTreeSet::new(),
                seen: BTreeSet::new(),
                since: self.now,
                node_sent: None,
                node_asked_at: None,
                node_asked: false,
                by_node: false,
                state: RelayState::Sent,
                reported: None,
            },
        );
        let mut out = Vec::new();
        for id in first {
            self.announce_held(txid, id, &mut out);
        }
        self.report_held(&txid, &mut out);
        self.deferred.extend(out);
        Ok(txid)
    }

    /// NR-86: stop holding a transaction. Nothing more is announced or
    /// served for it, and what the peers showed of it is forgotten.
    pub fn forget(&mut self, txid: &Txid) {
        self.held.remove(txid);
    }

    /// The transactions held, by ID.
    pub fn held(&self) -> Vec<Txid> {
        self.held.keys().copied().collect()
    }

    /// NR-87, NR-88: what the peers have shown of a held transaction.
    pub fn relay_state(&self, txid: &Txid) -> Option<RelayState> {
        self.held.get(txid).map(|h| h.state)
    }

    /// Announce one held transaction to one peer.
    fn announce_held(&mut self, txid: Txid, id: PeerId, out: &mut Vec<Output>) {
        // NR-109: in the mode of NR-106, only the own node is told.
        if !self
            .peers
            .get(&id)
            .is_some_and(|p| p.ready && (p.own || !self.node.on))
        {
            return;
        }
        let own = self.peers.get(&id).is_some_and(|p| p.own);
        let now = self.now;
        if self.send(id, Request::Announce(vec![txid]), out) {
            if let Some(held) = self.held.get_mut(&txid) {
                held.asked.insert(id);
                if own && self.node.on {
                    // NR-112: the time to ask the node starts again.
                    held.node_sent = Some(now);
                    held.node_asked_at = None;
                }
            }
        }
    }

    /// NR-86: a peer became ready; every held transaction is announced to
    /// it.
    pub(super) fn announce_held_to_new(&mut self, id: PeerId, out: &mut Vec<Output>) {
        let txids: Vec<Txid> = self.held.keys().copied().collect();
        for txid in txids {
            self.announce_held(txid, id, out);
            self.report_held(&txid, out);
        }
    }

    /// NR-86: a peer asks for transactions. Each one that is held is sent;
    /// any other is not answered (NR-55).
    pub(super) fn serve_held(&mut self, id: PeerId, txids: &[Txid], out: &mut Vec<Output>) {
        for txid in txids {
            let Some(tx) = self.held.get(txid).map(|h| h.tx.clone()) else {
                continue;
            };
            if self.send(id, Request::SendTx(Box::new(tx)), out) {
                if let Some(held) = self.held.get_mut(txid) {
                    held.fetched.insert(id);
                }
                self.report_held(txid, out);
            }
        }
    }

    /// NR-87: a peer announced a transaction. True when it is a held one,
    /// which is then never asked for.
    pub(super) fn heard_held(&mut self, id: PeerId, txid: &Txid, out: &mut Vec<Output>) -> bool {
        let group = self.peers.get(&id).map(|p| p.group);
        let Some(held) = self.held.get_mut(txid) else {
            return false;
        };
        if let Some(group) = group {
            if !held.asked.contains(&id) {
                held.seen.insert(group);
                held.refresh_state();
            }
        }
        self.report_held(txid, out);
        true
    }

    /// NR-88: held transactions that have not been seen back in time are
    /// reported and announced to every ready peer again.
    pub(super) fn age_held(&mut self, now: u64, out: &mut Vec<Output>) {
        // A transaction held before the manager had its first time was
        // given time 0: its ten minutes start now.
        for held in self.held.values_mut().filter(|h| h.since == 0) {
            held.since = now;
        }
        for held in self.held.values_mut().filter(|h| h.node_sent == Some(0)) {
            held.node_sent = Some(now);
        }
        self.ask_node_held(now, out);
        let due: Vec<Txid> = self
            .held
            .iter()
            .filter(|(_, h)| {
                !matches!(
                    h.state,
                    RelayState::SeenBack { .. }
                        | RelayState::HeldByNode
                        | RelayState::SeenBackAndHeldByNode { .. }
                ) && now.saturating_sub(h.since) >= NOT_SEEN_SECS
            })
            .map(|(txid, _)| *txid)
            .collect();
        for txid in due {
            if let Some(held) = self.held.get_mut(&txid) {
                held.state = RelayState::NotSeen;
                held.since = now;
            }
            for id in self.ready_peers() {
                self.announce_held(txid, id, out);
            }
            self.report_held(&txid, out);
        }
    }

    /// NR-112: with `node_only` on, ask the own node for each held
    /// transaction it has not answered for: first [`NODE_ASK_AFTER_SECS`]
    /// after it was sent the announcement, then every
    /// [`NODE_ASK_EVERY_SECS`]. Never runs in the other mode.
    fn ask_node_held(&mut self, now: u64, out: &mut Vec<Output>) {
        if !self.node.on {
            return;
        }
        let Some(id) = self.node_peer() else {
            return;
        };
        let due: Vec<Txid> = self
            .held
            .iter()
            .filter(|(_, h)| {
                !h.by_node
                    && match (h.node_sent, h.node_asked_at) {
                        (Some(sent), None) => now.saturating_sub(sent) >= NODE_ASK_AFTER_SECS,
                        (Some(_), Some(at)) => now.saturating_sub(at) >= NODE_ASK_EVERY_SECS,
                        (None, _) => false,
                    }
            })
            .map(|(txid, _)| *txid)
            .collect();
        for txid in due {
            // The request is `getdata` by txid with witness (`MSG_WITNESS_TX`).
            if !self.send(id, Request::GetTx(vec![txid]), out) {
                return;
            }
            if let Some(held) = self.held.get_mut(&txid) {
                held.node_asked_at = Some(now);
                held.node_asked = true;
            }
        }
    }

    /// NR-112: a `tx` from the own node for a held transaction that it was
    /// asked for. `None` when it is not one (the caller goes on as for any
    /// `tx`); otherwise whether it is the held transaction byte for byte,
    /// witness included. A transaction that is not is a fault of the node.
    pub(super) fn node_held_reply(
        &mut self,
        id: PeerId,
        tx: &Transaction,
        bytes: &[u8],
        out: &mut Vec<Output>,
    ) -> Option<bool> {
        if !self.node.on || !self.peers.get(&id).is_some_and(|p| p.own) {
            return None;
        }
        let txid = tx.compute_txid();
        let held = self.held.get_mut(&txid).filter(|h| h.node_asked)?;
        if serialize(&held.tx) != bytes {
            return Some(false);
        }
        held.by_node = true;
        held.refresh_state();
        self.report_held(&txid, out);
        Some(true)
    }

    /// Say what changed about a held transaction since it was last said.
    fn report_held(&mut self, txid: &Txid, out: &mut Vec<Output>) {
        let Some(held) = self.held.get_mut(txid) else {
            return;
        };
        let now = (held.asked.len(), held.fetched.len(), held.state);
        let (old_asked, old_fetched, old_state) = match held.reported {
            Some((a, f, s)) => (Some(a), Some(f), Some(s)),
            None => (None, None, None),
        };
        held.reported = Some(now);
        if old_asked != Some(now.0) || old_fetched != Some(now.1) {
            out.push(Output::TxRelayed {
                txid: *txid,
                peers_asked: now.0,
                peers_fetched: now.1,
            });
        }
        if old_state != Some(now.2) {
            out.push(Output::TxState {
                txid: *txid,
                state: now.2,
            });
        }
    }
}
