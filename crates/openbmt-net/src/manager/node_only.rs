//! The shop's own node as the only peer (NR-105 to NR-111).
//!
//! With `node_only` on, the own node is not the second check of
//! `manager/own.rs` but the one peer: it is in `peers`, so it serves
//! headers, filters, blocks and transactions through the same checks as any
//! peer (NR-106). What differs is kept here:
//!
//! - one filter is the whole check, because there is nobody to compare with
//!   (NR-40 to NR-42 with a quorum of 1); with the random peers of NR-110
//!   running, the quorum is 2 again;
//! - the node's services must suit (NR-107), and a node whose do not is
//!   dropped and reported;
//! - a block older than the node serves is not asked for, and is reported
//!   (NR-108);
//! - what the node announced it holds is reported as `OwnNodeHas` (NR-101),
//!   and the app can ask about a transaction (`ask_own_node`);
//! - transactions go to the own node alone (NR-109), in `broadcast.rs`;
//! - the own node is not in the light client's health peers, not a source
//!   of its sightings, and gives the book no address (WR-66).

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use bitcoin::p2p::ServiceFlags;
use bitcoin::{BlockHash, Txid};

use super::{
    ConnectRefused, Manager, Output, PeerId, PeerState, TransportChoice, MAX_OWN_ANNOUNCED,
    MAX_OWN_WANTED, MIN_FILTER_PEERS, OWN_ANSWER_SECS,
};
use crate::own_node::OwnNode;
use crate::{Request, Session, SessionConfig};
use openbmt_watch::{host_group, HeaderChain, PeerAddr};

/// NR-108: the blocks a node that keeps only recent ones serves
/// (`NODE_NETWORK_LIMITED`, BIP 159).
pub const NODE_LIMITED_BLOCKS: u32 = 288;

/// NR-107: whether these services suit: compact filters (BIP 157) and
/// blocks (`NODE_NETWORK` or `NODE_NETWORK_LIMITED`).
pub(super) fn serves(services: ServiceFlags) -> bool {
    services.has(ServiceFlags::COMPACT_FILTERS)
        && (services.has(ServiceFlags::NETWORK) || services.has(ServiceFlags::NETWORK_LIMITED))
}

/// What the manager knows about the own node with `node_only` on
/// (NR-102, NR-107, NR-108).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NodeOnlyView {
    /// The handshake is done and the node's services suit.
    pub ready: bool,
    /// The best height it has shown. `None` before the handshake.
    pub best_height: Option<u32>,
    /// The time anything last came from it, in seconds.
    pub last_heard: Option<u64>,
    /// The connection was made to a Tor v3 address (NR-98).
    pub authenticated: bool,
    /// NR-107: when the node last showed services that do not suit, until a
    /// connection to it shows ones that do.
    pub unfit_since: Option<u64>,
    /// NR-108: the oldest block the node does not serve and is needed, and
    /// when that was found out.
    pub lacks: Option<(u32, u64)>,
}

/// What the manager keeps for the mode.
#[derive(Debug, Default)]
pub(super) struct NodeOnlyState {
    /// `node_only` is on.
    pub(super) on: bool,
    /// The random peers of NR-110 are running.
    pub(super) fallback: bool,
    /// NR-107.
    pub(super) unfit: Option<u64>,
    /// NR-108: height to the block and the time it was found out.
    pub(super) lacks: BTreeMap<u32, (BlockHash, u64)>,
    /// NR-101: transactions the node announced, newest last.
    held: BTreeSet<Txid>,
    held_order: VecDeque<Txid>,
    /// NR-101: requests for a transaction and when they run out.
    asked: BTreeMap<Txid, u64>,
}

impl Manager {
    /// NR-106: turn the mode on or off. On, one compact filter is the whole
    /// check of a block (the own node is the only peer), and a transaction
    /// goes out to the own node alone (NR-109). The caller connects the own
    /// node with [`Manager::connected_node_only`] and dials nobody else.
    pub fn set_node_only(&mut self, on: bool) {
        self.node.on = on;
        if !on {
            self.node.fallback = false;
            self.node.lacks.clear();
            self.node.unfit = None;
        }
        self.settle_now();
    }

    /// NR-110: the random peers are running (or are not). With them, the
    /// check of a block needs two filters again; the own node stays the only
    /// peer that a transaction goes to (NR-109).
    pub fn set_node_only_fallback(&mut self, on: bool) {
        self.node.fallback = on && self.node.on;
        self.settle_now();
    }

    /// NR-106: the own node's connection is open. It is one peer of the
    /// manager, apart from the target number (NR-20), from network groups and
    /// from the local group; its transport is as the caller chose
    /// (NR-80). Refused when an own node is connected already.
    pub fn connected_node_only(
        &mut self,
        id: PeerId,
        node: &OwnNode,
        nonce: u64,
        now: u64,
        transport: TransportChoice,
    ) -> Result<(), ConnectRefused> {
        self.now = now;
        if self.peers.contains_key(&id) {
            return Err(ConnectRefused::IdInUse);
        }
        if self.peers.values().any(|p| p.own) {
            return Err(ConnectRefused::OwnNodeConnected);
        }
        let addr = match node.host() {
            crate::own_node::OwnNodeHost::Onion(name) => {
                crate::onion::parse_onion_name(name).map_or(PeerAddr::Named, PeerAddr::Onion)
            }
            crate::own_node::OwnNodeHost::Clear(sock) => PeerAddr::Ip(sock.ip()),
        };
        let config = SessionConfig {
            network: self.config.network,
            nonce,
            tip_height: self.store.tip().0,
        };
        let session = match transport {
            TransportChoice::V1 => Session::new(config, now),
            TransportChoice::V2(entropy) => {
                Session::new_v2(config, now, entropy).map_err(|_| ConnectRefused::BadEntropy)?
            }
        };
        let mut peer = PeerState::new(addr, host_group(&addr), session, now);
        peer.own = true;
        peer.authenticated = node.is_authenticated();
        self.peers.insert(id, peer);
        Ok(())
    }

    /// NR-102, NR-107, NR-108: what is known of the own node, or `None`
    /// while the mode is off.
    pub fn node_only_view(&self) -> Option<NodeOnlyView> {
        if !self.node.on {
            return None;
        }
        let peer = self.peers.values().find(|p| p.own);
        Some(NodeOnlyView {
            ready: peer.is_some_and(|p| p.ready),
            best_height: peer.filter(|p| p.ready).map(|p| p.shown_height),
            last_heard: peer.and_then(|p| p.heard),
            authenticated: peer.is_some_and(|p| p.authenticated),
            unfit_since: self.node.unfit,
            lacks: self
                .node
                .lacks
                .iter()
                .next()
                .map(|(height, (_, since))| (*height, *since)),
        })
    }

    /// The own node's connection, if one is open.
    pub(super) fn node_peer(&self) -> Option<PeerId> {
        self.peers.iter().find(|(_, p)| p.own).map(|(id, _)| *id)
    }

    /// Filters that make the check of a block: one with the own node alone,
    /// two otherwise (NR-42).
    pub(super) fn filter_quorum(&self) -> usize {
        if self.node.on && !self.node.fallback {
            1
        } else {
            MIN_FILTER_PEERS
        }
    }

    /// NR-108: the ready peers that may be asked for this block. The own
    /// node is left out when it keeps only recent blocks and this one is
    /// older than it serves; that is recorded, and the block waits.
    pub(super) fn block_servers(&mut self, height: u32, hash: BlockHash) -> Vec<PeerId> {
        let tip = self.store.tip_height();
        let now = self.now;
        let mut out = Vec::new();
        for (id, peer) in self.peers.iter().filter(|(_, p)| p.ready) {
            let limited = self.node.on && peer.own && !peer.services.has(ServiceFlags::NETWORK);
            if limited && peer.shown_height.max(tip).saturating_sub(height) > NODE_LIMITED_BLOCKS {
                self.node.lacks.entry(height).or_insert((hash, now));
            } else {
                out.push(*id);
            }
        }
        out
    }

    /// A peer left. An own node's requests are void; what the app asked
    /// about stays wanted (NR-101).
    pub(super) fn node_gone(&mut self, peer: &PeerState) {
        if peer.own {
            self.node.asked.clear();
        }
    }

    /// Blocks that are no longer in work are no longer lacking.
    pub(super) fn node_settled(&mut self) {
        let work = &self.work;
        let fee = self.fee_work.as_ref().map(|w| w.height);
        self.node
            .lacks
            .retain(|height, _| work.contains_key(height) || fee == Some(*height));
    }

    /// Requests that have run out are let go (NR-101).
    pub(super) fn node_tick(&mut self, now: u64) {
        self.node.asked.retain(|_, deadline| now < *deadline);
    }

    /// NR-101: the node announced a transaction: it holds it.
    pub(super) fn node_announced(&mut self, txid: Txid, out: &mut Vec<Output>) {
        if self.node.held.insert(txid) {
            self.node.held_order.push_back(txid);
            if self.node.held_order.len() > MAX_OWN_ANNOUNCED {
                if let Some(old) = self.node.held_order.pop_front() {
                    self.node.held.remove(&old);
                }
            }
        }
        out.push(Output::OwnNodeHas { txid });
        self.own_wanted.retain(|t| *t != txid);
    }

    /// NR-101: a transaction that the app asked the node about came.
    /// True when it was one of those, which is then not a sighting.
    pub(super) fn node_answered(&mut self, id: PeerId, txid: &Txid, out: &mut Vec<Output>) -> bool {
        if !self.peers.get(&id).is_some_and(|p| p.own) || self.node.asked.remove(txid).is_none() {
            return false;
        }
        self.own_wanted.retain(|t| t != txid);
        out.push(Output::OwnNodeHas { txid: *txid });
        true
    }

    /// An own node's `notfound` ends its requests: no answer is not a "no".
    pub(super) fn node_not_found(&mut self, id: PeerId, txids: &[Txid]) {
        if self.peers.get(&id).is_some_and(|p| p.own) {
            for txid in txids {
                self.node.asked.remove(txid);
            }
        }
    }

    /// NR-101: the app asks whether the node holds a transaction.
    pub(super) fn ask_node_only(&mut self, txid: Txid) -> Vec<Output> {
        if !self.own_wanted.contains(&txid) {
            if self.own_wanted.len() >= MAX_OWN_WANTED {
                self.own_wanted.pop_front();
            }
            self.own_wanted.push_back(txid);
        }
        let mut out = Vec::new();
        if self.node.held.contains(&txid) {
            self.own_wanted.retain(|t| *t != txid);
            out.push(Output::OwnNodeHas { txid });
        } else if self.peers.values().any(|p| p.own && p.ready) {
            self.node_ask(&[txid], &mut out);
        }
        out
    }

    /// Everything the app asked about is asked of a node that has just
    /// become ready.
    pub(super) fn node_ask_wanted(&mut self, out: &mut Vec<Output>) {
        let wanted: Vec<Txid> = self.own_wanted.iter().copied().collect();
        let (held, rest): (Vec<Txid>, Vec<Txid>) =
            wanted.into_iter().partition(|t| self.node.held.contains(t));
        for txid in held {
            self.own_wanted.retain(|t| *t != txid);
            out.push(Output::OwnNodeHas { txid });
        }
        self.node_ask(&rest, out);
    }

    fn node_ask(&mut self, txids: &[Txid], out: &mut Vec<Output>) {
        let Some(id) = self.node_peer() else {
            return;
        };
        let now = self.now;
        let fresh: Vec<Txid> = txids
            .iter()
            .filter(|t| !self.node.asked.contains_key(*t))
            .copied()
            .collect();
        if fresh.is_empty() {
            return;
        }
        for txid in &fresh {
            self.node
                .asked
                .insert(*txid, now.saturating_add(OWN_ANSWER_SECS));
        }
        self.send(id, Request::GetTx(fresh), out);
    }
}

impl PeerState {
    /// A peer that is not ready yet.
    pub(super) fn new(
        addr: PeerAddr,
        group: openbmt_watch::NetGroup,
        session: Session,
        now: u64,
    ) -> Self {
        Self {
            addr,
            group,
            session,
            ready: false,
            services: ServiceFlags::NONE,
            shown_height: 0,
            shown_at: now,
            announced: VecDeque::new(),
            ahead_since: None,
            last_reask: None,
            unanswered: 0,
            body_misses: 0,
            stale: VecDeque::new(),
            blocked: false,
            own: false,
            authenticated: false,
            heard: None,
        }
    }
}
