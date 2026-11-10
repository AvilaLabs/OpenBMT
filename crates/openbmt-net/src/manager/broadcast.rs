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

use std::collections::BTreeSet;
use std::fmt;

use bitcoin::consensus::encode::deserialize;
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

/// What the peers have shown of one held transaction (NR-87, NR-88).
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
    /// [`NOT_SEEN_SECS`] passed with no two groups announcing it back. It
    /// has been announced to every ready peer again.
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
    state: RelayState,
    /// The counts and the state last reported.
    reported: Option<(usize, usize, RelayState)>,
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
        let ready = self.ready_peers();
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
        if !self.peers.get(&id).is_some_and(|p| p.ready) {
            return;
        }
        if self.send(id, Request::Announce(vec![txid]), out) {
            if let Some(held) = self.held.get_mut(&txid) {
                held.asked.insert(id);
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
                if held.seen.len() >= SEEN_BACK_GROUPS {
                    held.state = RelayState::SeenBack {
                        groups: held.seen.len(),
                    };
                }
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
        let due: Vec<Txid> = self
            .held
            .iter()
            .filter(|(_, h)| {
                !matches!(h.state, RelayState::SeenBack { .. })
                    && now.saturating_sub(h.since) >= NOT_SEEN_SECS
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
