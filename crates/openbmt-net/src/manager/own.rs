//! The merchant's own node inside the manager (NR-98 to NR-103).
//!
//! The own node is one session of its own, kept apart from the random peers:
//! it is not in `peers`, so it takes no place of the target number (NR-20),
//! no network group, no filter or block work, no part in the health check's
//! peers and no announcement of a transaction to fetch (NR-50, NR-51). What
//! it says is kept in this module and nowhere else:
//!
//! - which transactions it holds in its memory pool: those it announced, and
//!   those it sent when asked with [`Manager::ask_own_node`];
//! - how far it has got: its own statement at the handshake, and the blocks
//!   it announced or sent headers for that the counter's chain holds.
//!
//! A message from the node that was not asked for (a block, a filter, a
//! transaction nobody asked for) is a protocol violation and ends the
//! session as NR-25 says. Nothing else ends it: a node that answers slowly
//! or not at all is left connected (NR-100).

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use bitcoin::consensus::encode::deserialize;
use bitcoin::{BlockHash, Transaction, Txid};
use openbmt_watch::HeaderChain;

use super::{ConnectRefused, Manager, Output, PeerId, TransportChoice};
use crate::own_node::OwnNode;
use crate::{Drop, Event, Request, Session, SessionConfig};
use bitcoin::p2p::ServiceFlags;

/// NR-101: transaction IDs the own node announced that are remembered.
pub const MAX_OWN_ANNOUNCED: usize = 50_000;
/// NR-101: transactions the app asked about and that are not answered yet.
pub const MAX_OWN_WANTED: usize = 256;
/// NR-101: seconds the own node may take to answer a request for a
/// transaction. A late answer ends nothing: the request is asked again at
/// the next [`Manager::ask_own_node`] or the next connection.
pub const OWN_ANSWER_SECS: u64 = 30;
/// NR-102: announced blocks that the counter's chain does not hold yet and
/// are remembered.
pub const MAX_OWN_BLOCKS: usize = 8;

/// What the manager knows about the own node (NR-102).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OwnNodeView {
    /// The handshake is done.
    pub ready: bool,
    /// The best height it has shown: its statement at the handshake, raised
    /// by every block it announced or sent a header for that the counter's
    /// chain holds. `None` before the handshake.
    pub best_height: Option<u32>,
    /// The time anything last came from it, in seconds.
    pub last_heard: Option<u64>,
    /// The connection was made to a Tor v3 address (NR-98). False only in a
    /// test of the driver on one machine.
    pub authenticated: bool,
}

#[derive(Debug)]
pub(super) struct OwnPeer {
    id: PeerId,
    session: Session,
    authenticated: bool,
    ready: bool,
    /// Its statement at the handshake, raised by `raise`.
    shown: Option<u32>,
    /// Announced blocks that the store did not hold when announced.
    blocks: VecDeque<BlockHash>,
    heard: Option<u64>,
    /// NR-101: what it announced.
    announced: BTreeSet<Txid>,
    announced_order: VecDeque<Txid>,
    /// Requests for a transaction and when they run out.
    pending: BTreeMap<Txid, u64>,
}

impl Manager {
    /// NR-98, NR-100: the connection to the own node is open. The
    /// authenticated address is the driver's to dial; the manager only
    /// keeps the session apart. It is refused when an own node is connected
    /// already or the ID is in use, and takes no place of the target number
    /// of peers.
    pub fn connected_own_node(
        &mut self,
        id: PeerId,
        node: &OwnNode,
        nonce: u64,
        now: u64,
        transport: TransportChoice,
    ) -> Result<(), ConnectRefused> {
        self.now = now;
        if self.peers.contains_key(&id) || self.own.as_ref().is_some_and(|o| o.id == id) {
            return Err(ConnectRefused::IdInUse);
        }
        if self.own.is_some() {
            return Err(ConnectRefused::OwnNodeConnected);
        }
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
        self.own = Some(OwnPeer {
            id,
            session,
            authenticated: node.is_authenticated(),
            ready: false,
            shown: None,
            blocks: VecDeque::new(),
            heard: None,
            announced: BTreeSet::new(),
            announced_order: VecDeque::new(),
            pending: BTreeMap::new(),
        });
        Ok(())
    }

    /// Whether this connection is the own node's.
    pub fn is_own_node(&self, id: PeerId) -> bool {
        self.own.as_ref().is_some_and(|o| o.id == id)
    }

    /// NR-102: what is known of the own node, or `None` while no connection
    /// to it is open.
    pub fn own_node(&self) -> Option<OwnNodeView> {
        let own = self.own.as_ref()?;
        let known = own
            .blocks
            .iter()
            .filter_map(|b| self.store.height_of(b))
            .max();
        Some(OwnNodeView {
            ready: own.ready,
            best_height: own.shown.map(|s| s.max(known.unwrap_or(0))),
            last_heard: own.heard,
            authenticated: own.authenticated,
        })
    }

    /// NR-101: ask whether the own node holds this transaction in its memory
    /// pool. If it announced it already, the answer is in the outputs of this
    /// call as [`Output::OwnNodeHas`]. If not, it is asked with `getdata`,
    /// and the answer comes when it sends the transaction or announces it. A
    /// node that does not have it, or is not serving it yet, says nothing
    /// that counts: no answer is not a "no". The transaction stays wanted, up
    /// to [`MAX_OWN_WANTED`], and is asked again at the next connection.
    pub fn ask_own_node(&mut self, txid: Txid) -> Vec<Output> {
        if !self.own_wanted.contains(&txid) {
            if self.own_wanted.len() >= MAX_OWN_WANTED {
                self.own_wanted.pop_front();
            }
            self.own_wanted.push_back(txid);
        }
        let mut out = Vec::new();
        let Some(own) = self.own.as_mut() else {
            return out;
        };
        if own.announced.contains(&txid) {
            self.own_wanted.retain(|t| *t != txid);
            out.push(Output::OwnNodeHas { txid });
        } else if own.ready {
            self.own_ask(&[txid], &mut out);
        }
        out
    }

    /// Request transactions that are not already requested.
    fn own_ask(&mut self, txids: &[Txid], out: &mut Vec<Output>) {
        let now = self.now;
        let Some(own) = self.own.as_mut() else {
            return;
        };
        let fresh: Vec<Txid> = txids
            .iter()
            .filter(|t| !own.pending.contains_key(*t))
            .copied()
            .collect();
        if fresh.is_empty() {
            return;
        }
        for txid in &fresh {
            own.pending
                .insert(*txid, now.saturating_add(OWN_ANSWER_SECS));
        }
        let id = own.id;
        if let Err(d) = own.session.send(Request::GetTx(fresh)) {
            self.own_drop_session(id, d, out);
        }
    }

    /// Bytes from the own node.
    pub(super) fn own_received(&mut self, id: PeerId, bytes: &[u8], now: u64) -> Vec<Output> {
        let mut out = std::mem::take(&mut self.deferred);
        let Some(own) = self.own.as_mut() else {
            return out;
        };
        match own.session.receive(bytes, now) {
            Err(d) => self.own_drop_session(id, d, &mut out),
            Ok(events) => {
                own.heard = Some(now);
                for event in events {
                    if !self.is_own_node(id) {
                        break;
                    }
                    self.on_own_event(id, event, &mut out);
                }
            }
        }
        out
    }

    /// The handshake and ping timers of the own node, and its requests that
    /// have run out. Neither ends the connection for a slow answer
    /// (NR-100); the handshake and ping timers of NR-14 do.
    pub(super) fn own_tick(&mut self, now: u64, out: &mut Vec<Output>) {
        let Some(own) = self.own.as_mut() else {
            return;
        };
        let id = own.id;
        if let Err(d) = own.session.tick(now) {
            self.own_drop_session(id, d, out);
            return;
        }
        own.pending.retain(|_, deadline| now < *deadline);
    }

    /// The bytes to write to the own node, emptied.
    pub(super) fn own_outgoing(&mut self) -> Vec<u8> {
        match self.own.as_mut() {
            Some(own) => own.session.take_outgoing(),
            None => Vec::new(),
        }
    }

    /// The socket of the own node closed.
    pub(super) fn own_closed(&mut self, id: PeerId, out: &mut Vec<Output>) {
        if !self.is_own_node(id) {
            return;
        }
        if let Some(own) = self.own.take() {
            // NR-81: a close before any packet authenticated.
            if own.session.v2_unproven() {
                out.push(Output::V2NotSpoken { id });
            }
        }
    }

    /// NR-25 for a session that ended. The timers of NR-14 and the
    /// transport's own failures (NR-81, NR-83) are not the node's doing.
    fn own_drop_session(&mut self, id: PeerId, d: Drop, out: &mut Vec<Output>) {
        let misbehaved = !matches!(d.rule(), "NR-14" | "NR-81" | "NR-83");
        if d == Drop::V2NotSpoken {
            out.push(Output::V2NotSpoken { id });
        }
        self.own_drop(id, d.to_string(), misbehaved, out);
    }

    fn own_drop(&mut self, id: PeerId, reason: String, misbehaved: bool, out: &mut Vec<Output>) {
        if self.own.take().is_none() {
            return;
        }
        out.push(Output::Disconnect {
            id,
            reason,
            misbehaved,
        });
    }

    fn on_own_event(&mut self, id: PeerId, event: Event, out: &mut Vec<Output>) {
        match event {
            Event::Ready {
                services,
                start_height,
            } => self.on_own_ready(id, services, start_height, out),
            Event::Headers(headers) => {
                let hashes: Vec<BlockHash> = headers.iter().map(|h| h.block_hash()).collect();
                self.own_blocks(&hashes);
            }
            Event::Announced { txids, blocks } => {
                self.own_blocks(&blocks);
                for txid in txids {
                    self.own_announced(txid, out);
                }
            }
            Event::Tx(bytes) => self.on_own_tx(id, &bytes, out),
            Event::NotFound { txids, .. } => {
                if let Some(own) = self.own.as_mut() {
                    for txid in txids {
                        own.pending.remove(&txid);
                    }
                }
            }
            // Addresses and requests for a held transaction are of no use
            // to the own node's role, and are not acted on.
            Event::Addresses(_) | Event::Requested(_) => {}
            Event::Block(_) | Event::Filter { .. } => self.own_drop(
                id,
                "NR-25: data that was not asked for".to_string(),
                true,
                out,
            ),
        }
    }

    fn on_own_ready(
        &mut self,
        id: PeerId,
        services: ServiceFlags,
        start_height: u32,
        out: &mut Vec<Output>,
    ) {
        let Some(own) = self.own.as_mut() else {
            return;
        };
        own.ready = true;
        own.shown = Some(start_height);
        out.push(Output::OwnNodeReady {
            id,
            services,
            height: start_height,
        });
        // Everything the app asked about is asked again of the new session.
        let wanted: Vec<Txid> = self.own_wanted.iter().copied().collect();
        self.own_ask(&wanted, out);
    }

    /// Blocks it has shown: those the chain holds raise its height, the rest
    /// are remembered for [`Manager::own_node`] to place later.
    fn own_blocks(&mut self, hashes: &[BlockHash]) {
        let Some(own) = self.own.as_mut() else {
            return;
        };
        for hash in hashes {
            match self.store.height_of(hash) {
                Some(height) => own.shown = Some(own.shown.map_or(height, |s| s.max(height))),
                None => {
                    if own.blocks.contains(hash) {
                        continue;
                    }
                    if own.blocks.len() >= MAX_OWN_BLOCKS {
                        own.blocks.pop_front();
                    }
                    own.blocks.push_back(*hash);
                }
            }
        }
    }

    /// NR-101: the node announced a transaction.
    fn own_announced(&mut self, txid: Txid, out: &mut Vec<Output>) {
        let Some(own) = self.own.as_mut() else {
            return;
        };
        if own.announced.insert(txid) {
            own.announced_order.push_back(txid);
            if own.announced_order.len() > MAX_OWN_ANNOUNCED {
                if let Some(old) = own.announced_order.pop_front() {
                    own.announced.remove(&old);
                }
            }
        }
        if self.own_wanted.contains(&txid) {
            self.own_wanted.retain(|t| *t != txid);
            out.push(Output::OwnNodeHas { txid });
        }
    }

    /// NR-101: the node sent a transaction. Only one that was asked for is
    /// taken; any other is a protocol violation.
    fn on_own_tx(&mut self, id: PeerId, bytes: &[u8], out: &mut Vec<Output>) {
        let Ok(tx) = deserialize::<Transaction>(bytes) else {
            self.own_drop(
                id,
                "NR-25: a transaction that does not parse".to_string(),
                true,
                out,
            );
            return;
        };
        let txid = tx.compute_txid();
        let asked = self
            .own
            .as_mut()
            .is_some_and(|own| own.pending.remove(&txid).is_some());
        if !asked {
            self.own_drop(
                id,
                "NR-25: a transaction that was not asked for".to_string(),
                true,
                out,
            );
            return;
        }
        self.own_wanted.retain(|t| *t != txid);
        out.push(Output::OwnNodeHas { txid });
    }
}
