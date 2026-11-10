//! Many peers at once, and what they send as facts for the watcher (NR-20,
//! NR-21, NR-25, NR-30 to NR-33, NR-40 to NR-46, NR-50 to NR-55).
//!
//! A [`Manager`] owns the watcher's [`HeaderStore`] and one [`Session`] per
//! peer. Like the rest of the crate it has no sockets, no clock, no threads
//! and no randomness: a driver owns the connections, passes in the bytes
//! and the time, writes out what [`Manager::outgoing`] returns and acts on
//! the [`Output`]s.
//!
//! The manager sends nothing but requests (NR-55). It keeps no mempool and
//! no block: a downloaded block and a fetched transaction leave as an
//! [`Output`] and are forgotten. Every map and queue has a bound (see the
//! constants), so no peer can make it hold more.
//!
//! Two things are decided here that `docs/NETWORK-RULES.md` does not say in
//! so many words:
//!
//! - A peer that answers late, after the request was given up or the block
//!   left the chain, is not dropped for it: the manager remembers the last
//!   [`MAX_STALE`] requests it gave up on, per peer.
//! - A peer is dropped, without a mark against it, after
//!   [`MAX_UNANSWERED`] requests in a row that it let run out.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fmt;
use std::net::{IpAddr, Ipv4Addr};

use bitcoin::bip158::BlockFilter;
use bitcoin::block::Header;
use bitcoin::consensus::encode::{deserialize, deserialize_partial, VarInt};
use bitcoin::hashes::{sha256, Hash};
use bitcoin::p2p::ServiceFlags;
use bitcoin::{Block, BlockHash, Network, ScriptBuf, Transaction, Txid};
use openbmt_watch::{
    host_group, network_group, Accepted, HeaderChain, HeaderStore, NetGroup, Peer, PeerAddr,
    PeerTransport, Refusal, WellFormedBlock, BEHIND_BLOCKS,
};

use crate::v2::V2Entropy;
use crate::{Drop, Event, PeerAddress, Request, Session, SessionConfig, MAX_HEADERS};

mod broadcast;
use broadcast::Held;
pub use broadcast::{
    BroadcastRefused, RelayState, MAX_BROADCAST_WEIGHT, MAX_HELD, NOT_SEEN_SECS, SEEN_BACK_GROUPS,
};

/// NR-20: connections the counter keeps, as the caller sets it.
pub const DEFAULT_TARGET_PEERS: usize = 8;
/// NR-21: connections to peers that offer compact filters, when there are
/// any.
pub const MIN_FILTER_PEERS: usize = 2;
/// NR-32, NR-33: seconds between two `getheaders` the counter sends to one
/// peer because of an unknown parent or an announced block it does not hold.
pub const REASK_SECS: u64 = 60;
/// NR-41: most scripts the manager watches.
pub const MAX_WATCHED_SCRIPTS: usize = 100_000;
/// NR-40: most blocks in work at once.
pub const MAX_BLOCKS_IN_WORK: usize = 8;
/// NR-42, NR-45: seconds a filter or a block may take to arrive.
pub const ANSWER_SECS: u64 = 60;
/// NR-40: seconds a peer must have shown its newest block before it is asked
/// for that block's filter, so that its filter index has taken the block.
pub const FILTER_SETTLE_SECS: u64 = 2;
/// NR-33: most announced blocks remembered for one peer while the store
/// does not hold them.
pub const MAX_ANNOUNCED_BLOCKS: usize = 8;
/// Most filter requests made for one block, over all peers, so that a
/// block's record never grows with the peers that come and go.
pub const MAX_FILTER_ASKS: usize = 6;
/// NR-54: transaction IDs remembered.
pub const MAX_REMEMBERED_TXIDS: usize = 100_000;
/// NR-53: most announcing peers kept for one transaction.
pub const MAX_ANNOUNCERS: usize = 8;
/// NR-51: seconds a transaction may take to arrive.
pub const TX_ANSWER_SECS: u64 = 30;
/// NR-51: transactions asked for and not yet answered, over all peers.
pub const MAX_PENDING_TXS: usize = 5_000;
/// Most transaction IDs in one `getdata` the manager writes.
pub const MAX_TXIDS_PER_REQUEST: usize = 1_000;
/// Requests in a row a peer may let run out before it is dropped.
pub const MAX_UNANSWERED: u32 = 3;
/// Given-up requests remembered per peer, so that a late answer is not
/// taken for unasked data.
pub const MAX_STALE: usize = 64;
/// NR-51: transactions asked of one peer and not yet answered. One
/// `getdata`'s worth, so that no single peer can hold every place in
/// [`MAX_PENDING_TXS`].
pub const MAX_PENDING_TXS_PER_PEER: usize = 1_000;
/// NR-33: seconds a peer has to deliver the headers for a height it claims.
pub const CLAIM_SECS: u64 = 600;

/// Names a connection. The caller chooses it, unique per connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PeerId(pub u64);

/// What the manager is set up with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ManagerConfig {
    /// The network the counter is on.
    pub network: Network,
    /// NR-20: how many connections to keep (8).
    pub target_peers: usize,
    /// NR-40: blocks above this height are checked.
    pub watch_from: u32,
    /// Take a peer at a local or private address. Off in the counter, where
    /// such an address is the Box's node (NR-20). On only for a check
    /// against a node on the same machine.
    pub allow_local: bool,
}

/// NR-20, NR-21: what the driver should dial next.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Wanted {
    /// Network groups that already have a connection.
    pub avoid: BTreeSet<NetGroup>,
    /// True while fewer than 2 connected groups have a ready peer that
    /// offers compact filters.
    pub want_filters: bool,
}

/// NR-80: how a new connection is to speak. The caller decides; the
/// randomness of the encrypted transport comes with it.
#[derive(Debug)]
pub enum TransportChoice {
    /// The old transport (NR-81 after a refusal of the encrypted one).
    V1,
    /// The encrypted transport of BIP324.
    V2(V2Entropy),
}

/// Why [`Manager::connected`] did not take a connection. Nothing is stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectRefused {
    /// The ID is already used by a connection.
    IdInUse,
    /// NR-20: a connected peer is in the same network group.
    SameGroup,
    /// NR-20: the address is local or private, as the Box's node would be.
    LocalGroup,
    /// NR-20: the target number of peers is connected.
    Full,
    /// NR-80: the randomness for the encrypted transport gives no key.
    BadEntropy,
}

impl fmt::Display for ConnectRefused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::IdInUse => write!(f, "the peer ID is in use"),
            Self::SameGroup => write!(f, "NR-20: a connected peer is in the same network group"),
            Self::LocalGroup => write!(f, "NR-20: the address is local"),
            Self::Full => write!(f, "NR-20: the target number of peers is connected"),
            Self::BadEntropy => write!(f, "NR-80: the randomness gives no key"),
        }
    }
}

impl std::error::Error for ConnectRefused {}

/// What the driver and the app act on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Output {
    /// Close the socket. The manager has forgotten the peer.
    Disconnect {
        /// The connection.
        id: PeerId,
        /// The rule and what happened, in words.
        reason: String,
        /// NR-25: true when the peer broke a rule of sections 1 or 2, sent
        /// a header, block or filter that is refused, or sent data that was
        /// not asked of it. The driver tells the address book.
        misbehaved: bool,
    },
    /// NR-81: the encrypted transport was not spoken by this peer: it closed,
    /// or answered with an old-transport message, before any of its packets
    /// authenticated. The driver may try the peer again with the old
    /// transport, and remembers that it did not speak the new one. Sent
    /// before the `Disconnect` of the same connection, if there is one.
    V2NotSpoken {
        /// The connection.
        id: PeerId,
    },
    /// The handshake is done. The driver tells the address book.
    Ready {
        /// The connection.
        id: PeerId,
        /// The services the peer states.
        services: ServiceFlags,
    },
    /// NR-22: addresses a peer told of.
    Addresses {
        /// The network group of the peer that sent them.
        from: NetGroup,
        /// The addresses, at most 1,000.
        list: Vec<PeerAddress>,
    },
    /// The best chain changed (NR-31, NR-46).
    Chain {
        /// Blocks that joined, lowest first.
        joined: Vec<(u32, BlockHash)>,
        /// Blocks that left, old tip first.
        left: Vec<BlockHash>,
    },
    /// A downloaded block that passed the watcher's checks on its own and
    /// is on the best chain. The app hands it to the watcher.
    Block {
        /// The block's hash.
        hash: BlockHash,
        /// The block as the peer sent it.
        bytes: Vec<u8>,
    },
    /// NR-53: a fetched transaction and every peer that has announced it so
    /// far. The manager keeps no transaction bytes.
    Sighting {
        /// The transaction as the peer sent it.
        tx: Vec<u8>,
        /// Where the peers that announced it were reached.
        from: Vec<PeerAddr>,
    },
    /// NR-53: a peer announced a transaction that was fetched already. The
    /// app maps the ID to the transaction if it kept it.
    AlsoAnnounced {
        /// The transaction.
        txid: Txid,
        /// Where the peer was reached.
        from: PeerAddr,
    },
    /// NR-86: how many peers a held transaction was announced to and how
    /// many have asked for it. Sent when either count changes.
    TxRelayed {
        /// The held transaction.
        txid: Txid,
        /// Peers that were sent an `inv` for it.
        peers_asked: usize,
        /// Peers that asked for it and were sent it. A peer that asked may
        /// have thrown it away.
        peers_fetched: usize,
    },
    /// NR-87, NR-88: what the peers have shown of a held transaction. Sent
    /// when it changes.
    TxState {
        /// The held transaction.
        txid: Txid,
        /// What the peers have shown.
        state: RelayState,
    },
}

/// A request the manager gave up on, which a slow peer may still answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stale {
    Block(BlockHash),
    Filter(BlockHash),
    Tx(Txid),
}

#[derive(Debug)]
struct PeerState {
    addr: PeerAddr,
    group: NetGroup,
    session: Session,
    ready: bool,
    services: ServiceFlags,
    /// NR-33: the highest block the peer has shown.
    shown_height: u32,
    /// When `shown_height` last rose.
    shown_at: u64,
    /// NR-33: blocks the peer announced that the store did not hold yet,
    /// newest last, at most [`MAX_ANNOUNCED_BLOCKS`].
    announced: VecDeque<BlockHash>,
    ahead_since: Option<u64>,
    /// NR-32: when the last re-ask of headers went out.
    last_reask: Option<u64>,
    unanswered: u32,
    stale: VecDeque<Stale>,
    /// NR-33: the peer sent headers that the store could not take for a
    /// reason that is not the peer's doing.
    blocked: bool,
}

/// What a peer's filter said about a block.
#[derive(Debug, Clone, Copy)]
struct Answer {
    digest: sha256::Hash,
    matched: bool,
}

/// A filter asked of one peer.
#[derive(Debug, Clone, Copy)]
struct Ask {
    group: NetGroup,
    answer: Option<Answer>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stage {
    Start,
    Filters,
    Fetch,
}

/// A block on the best chain that is being checked (NR-40 to NR-45).
#[derive(Debug)]
struct BlockWork {
    hash: BlockHash,
    height: u32,
    stage: Stage,
    /// Peers asked for the filter, and what they answered.
    asks: BTreeMap<PeerId, Ask>,
    asked_total: usize,
    filters_started: u64,
    fetch_peer: Option<PeerId>,
    fetch_since: u64,
    /// Peers that did not give this block, so that another is tried.
    failed: BTreeSet<PeerId>,
    resume_at: u64,
}

impl BlockWork {
    fn new(height: u32, hash: BlockHash) -> Self {
        Self {
            hash,
            height,
            stage: Stage::Start,
            asks: BTreeMap::new(),
            asked_total: 0,
            filters_started: 0,
            fetch_peer: None,
            fetch_since: 0,
            failed: BTreeSet::new(),
            resume_at: 0,
        }
    }
}

#[derive(Debug)]
struct Announcer {
    id: PeerId,
    addr: PeerAddr,
    tried: bool,
}

/// A transaction ID the manager remembers (NR-54).
#[derive(Debug)]
struct TxEntry {
    announcers: Vec<Announcer>,
    fetched: bool,
    pending: bool,
    /// In `waiting`.
    queued: bool,
    /// In `tx_order`. An entry with a request pending may leave the order
    /// and come back when the request ends.
    in_order: bool,
}

/// Many peers, one header chain, and the facts for the watcher.
#[derive(Debug)]
pub struct Manager {
    config: ManagerConfig,
    local_group: NetGroup,
    store: HeaderStore,
    peers: BTreeMap<PeerId, PeerState>,
    scripts: BTreeSet<ScriptBuf>,
    all_blocks: bool,
    fetch_transactions: bool,
    /// The latest time the caller passed in.
    now: u64,
    /// The next height of the best chain that no work has been made for.
    next_height: u32,
    work: BTreeMap<u32, BlockWork>,
    txs: BTreeMap<Txid, TxEntry>,
    tx_order: VecDeque<Txid>,
    /// Transactions that may be asked for, oldest first.
    waiting: VecDeque<Txid>,
    pending_tx: BTreeMap<Txid, (PeerId, u64)>,
    /// Outputs made by calls that return none.
    deferred: Vec<Output>,
    /// NR-86: transactions the app asked to broadcast.
    held: BTreeMap<Txid, Held>,
    /// How many peers were forgotten, so that a pass can see a drop.
    drops: u64,
}

/// The sizes of the collections, for the tests of the bounds.
#[cfg(test)]
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Sizes {
    pub peers: usize,
    pub work: usize,
    pub txs: usize,
    pub tx_order: usize,
    pub waiting: usize,
    pub pending_tx: usize,
    pub scripts: usize,
    pub deferred: usize,
    pub stale: usize,
    pub side_headers: usize,
}

impl Manager {
    /// A manager over this header store, with no peers. `watch_from` of the
    /// config is the height above which blocks are checked.
    pub fn new(config: ManagerConfig, store: HeaderStore) -> Self {
        let local = PeerAddr::Ip(IpAddr::V4(Ipv4Addr::LOCALHOST));
        let mut manager = Self {
            config,
            local_group: network_group(&local),
            store,
            peers: BTreeMap::new(),
            scripts: BTreeSet::new(),
            all_blocks: false,
            fetch_transactions: true,
            now: 0,
            next_height: 0,
            work: BTreeMap::new(),
            txs: BTreeMap::new(),
            tx_order: VecDeque::new(),
            waiting: VecDeque::new(),
            pending_tx: BTreeMap::new(),
            deferred: Vec::new(),
            held: BTreeMap::new(),
            drops: 0,
        };
        manager.next_height = manager.floor();
        manager
    }

    /// NR-31: the counter's chain, which is the store's.
    pub fn chain(&self) -> &HeaderStore {
        &self.store
    }

    /// NR-20, NR-21: what to dial next, or `None` when the target number of
    /// peers is connected.
    pub fn wanted(&self) -> Option<Wanted> {
        if self.peers.len() >= self.config.target_peers {
            return None;
        }
        let avoid = self.peers.values().map(|p| p.group).collect();
        let filter_groups: BTreeSet<NetGroup> = self
            .peers
            .values()
            .filter(|p| p.ready && p.services.has(ServiceFlags::COMPACT_FILTERS))
            .map(|p| p.group)
            .collect();
        Some(Wanted {
            avoid,
            want_filters: filter_groups.len() < MIN_FILTER_PEERS,
        })
    }

    /// NR-20: a new outbound connection is open, on the old transport.
    /// Nothing is stored when it is refused. The counter's `version` waits
    /// in [`Manager::outgoing`].
    pub fn connected(
        &mut self,
        id: PeerId,
        addr: PeerAddr,
        nonce: u64,
        now: u64,
    ) -> Result<(), ConnectRefused> {
        self.connected_with(id, addr, nonce, now, TransportChoice::V1)
    }

    /// NR-20, NR-80: a new outbound connection is open, to speak the
    /// transport the caller chose. On the encrypted one, the counter's key
    /// waits in [`Manager::outgoing`] and the `version` follows the peer's
    /// version packet.
    pub fn connected_with(
        &mut self,
        id: PeerId,
        addr: PeerAddr,
        nonce: u64,
        now: u64,
        transport: TransportChoice,
    ) -> Result<(), ConnectRefused> {
        self.now = now;
        if self.peers.contains_key(&id) {
            return Err(ConnectRefused::IdInUse);
        }
        let group = if self.config.allow_local {
            host_group(&addr)
        } else {
            network_group(&addr)
        };
        if group == self.local_group && !self.config.allow_local {
            return Err(ConnectRefused::LocalGroup);
        }
        if self.peers.values().any(|p| p.group == group) {
            return Err(ConnectRefused::SameGroup);
        }
        if self.peers.len() >= self.config.target_peers {
            return Err(ConnectRefused::Full);
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
        self.peers.insert(
            id,
            PeerState {
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
                stale: VecDeque::new(),
                blocked: false,
            },
        );
        Ok(())
    }

    /// Bytes arrived from this peer. An unknown peer is ignored.
    pub fn received(&mut self, id: PeerId, bytes: &[u8], now: u64) -> Vec<Output> {
        self.now = now;
        let mut out = std::mem::take(&mut self.deferred);
        let result = match self.peers.get_mut(&id) {
            Some(peer) => peer.session.receive(bytes, now),
            None => return out,
        };
        match result {
            Err(d) => self.drop_session(id, d, &mut out),
            Ok(events) => {
                for event in events {
                    if !self.peers.contains_key(&id) {
                        break;
                    }
                    self.on_event(id, event, now, &mut out);
                }
            }
        }
        self.settle(now, &mut out);
        out
    }

    /// The time has passed: handshakes and pings (NR-14), and every request
    /// that has run out (NR-42, NR-45, NR-51).
    pub fn tick(&mut self, now: u64) -> Vec<Output> {
        self.now = now;
        let mut out = std::mem::take(&mut self.deferred);
        let ids: Vec<PeerId> = self.peers.keys().copied().collect();
        for id in ids {
            let result = match self.peers.get_mut(&id) {
                Some(peer) => peer.session.tick(now),
                None => continue,
            };
            if let Err(d) = result {
                self.drop_session(id, d, &mut out);
            }
        }
        let expired: Vec<(Txid, PeerId)> = self
            .pending_tx
            .iter()
            .filter(|(_, (_, deadline))| now >= *deadline)
            .map(|(txid, (id, _))| (*txid, *id))
            .collect();
        for (txid, id) in expired {
            self.release_tx(&txid);
            self.push_stale(id, Stale::Tx(txid));
            self.note_unanswered(id, &mut out);
        }
        self.age_held(now, &mut out);
        self.settle(now, &mut out);
        out
    }

    /// The bytes to write to this peer, emptied. Never more than the
    /// session's cap (NR-05).
    pub fn outgoing(&mut self, id: PeerId) -> Vec<u8> {
        match self.peers.get_mut(&id) {
            Some(peer) => peer.session.take_outgoing(),
            None => Vec::new(),
        }
    }

    /// The socket closed, or the driver dropped it. The peer is forgotten
    /// and its requests go to others.
    pub fn closed(&mut self, id: PeerId, now: u64) -> Vec<Output> {
        self.now = now;
        let mut out = std::mem::take(&mut self.deferred);
        if let Some(peer) = self.peers.remove(&id) {
            // NR-81: a close before any packet authenticated.
            if peer.session.v2_unproven() {
                out.push(Output::V2NotSpoken { id });
            }
            self.drops = self.drops.saturating_add(1);
            self.release_tx_of(id);
        }
        self.settle(now, &mut out);
        out
    }

    /// NR-41: watch a script. Returns false, and watches nothing more, when
    /// [`MAX_WATCHED_SCRIPTS`] are watched already.
    pub fn watch_script(&mut self, script: ScriptBuf) -> bool {
        if self.scripts.contains(&script) {
            return true;
        }
        if self.scripts.len() >= MAX_WATCHED_SCRIPTS {
            return false;
        }
        self.scripts.insert(script);
        true
    }

    /// NR-40, NR-41: check the blocks above `height` again, against every
    /// script that is watched now. Work in progress is given up and starts
    /// over from the first block above `height`. For a caller that has
    /// added scripts whose payments may be in blocks it has checked already.
    pub fn rescan_from(&mut self, height: u32) {
        self.config.watch_from = height;
        let heights: Vec<u32> = self.work.keys().copied().collect();
        for h in heights {
            if let Some(mut item) = self.work.remove(&h) {
                self.abandon_asks(&mut item);
                self.abandon_fetch(&mut item);
            }
        }
        self.next_height = self.floor();
        self.settle_now();
    }

    /// NR-44: while on, every new block is downloaded in full. The caller
    /// turns it on while any sale has a payment that is seen and not yet
    /// confirmed.
    pub fn set_all_blocks(&mut self, on: bool) {
        self.all_blocks = on;
        self.settle_now();
    }

    /// NR-51, NR-52: whether announced transactions are fetched. On by
    /// default. Announcements are remembered either way.
    pub fn set_fetch_transactions(&mut self, on: bool) {
        self.fetch_transactions = on;
        self.settle_now();
    }

    /// WR-50, WR-52: one entry per ready peer, with its best shown height
    /// (NR-33).
    pub fn health_peers(&self) -> Vec<Peer> {
        self.peers
            .values()
            .filter(|p| p.ready)
            .map(|p| Peer {
                addr: p.addr,
                serves_filters: p.services.has(ServiceFlags::COMPACT_FILTERS),
                best_height: p.shown_height,
                transport: if p.session.is_v2() {
                    PeerTransport::V2
                } else {
                    PeerTransport::V1
                },
            })
            .collect()
    }

    /// NR-33: the earliest time since which a ready peer has shown a height
    /// more than 2 above the tip without delivering the headers. For
    /// `HealthInput::behind_since`; `None` when no peer is ahead.
    pub fn ahead_since(&self) -> Option<u64> {
        self.peers
            .values()
            .filter(|p| p.ready)
            .filter_map(|p| p.ahead_since)
            .min()
    }

    /// Every block above `watch_from` up to and including this height has
    /// been checked (NR-40 to NR-45), and none was left out. The app may
    /// keep it as the next `watch_from`.
    pub fn checked_height(&self) -> u32 {
        let open = self
            .work
            .keys()
            .next()
            .copied()
            .map_or(self.next_height, |h| h.min(self.next_height));
        open.saturating_sub(1).max(self.config.watch_from)
    }

    #[cfg(test)]
    pub(crate) fn sizes(&self) -> Sizes {
        Sizes {
            peers: self.peers.len(),
            work: self.work.len(),
            txs: self.txs.len(),
            tx_order: self.tx_order.len(),
            waiting: self.waiting.len(),
            pending_tx: self.pending_tx.len(),
            scripts: self.scripts.len(),
            deferred: self.deferred.len(),
            stale: self
                .peers
                .values()
                .map(|p| p.stale.len())
                .max()
                .unwrap_or(0),
            side_headers: self.store.side_count(),
        }
    }

    // ----- plumbing -------------------------------------------------------

    /// The first height that is checked.
    fn floor(&self) -> u32 {
        self.config
            .watch_from
            .max(self.store.start_height())
            .saturating_add(1)
    }

    /// Run the passes for a call that returns no outputs; what they make
    /// waits for the next call that does.
    fn settle_now(&mut self) {
        let mut out = Vec::new();
        self.settle(self.now, &mut out);
        self.deferred.extend(out);
    }

    /// Run the passes until a pass drops no peer.
    fn settle(&mut self, now: u64, out: &mut Vec<Output>) {
        loop {
            let before = self.drops;
            self.update_ahead(now, out);
            self.advance_blocks(now, out);
            self.pump_tx(now, out);
            if self.drops == before {
                break;
            }
        }
    }

    /// NR-33: note which ready peers show a height above the tip, and drop
    /// a peer whose claim has stood for [`CLAIM_SECS`] with no headers to
    /// back it. A peer whose headers the store could not take stays: that
    /// is the case WR-52 is for.
    fn update_ahead(&mut self, now: u64, out: &mut Vec<Output>) {
        let limit = u64::from(self.store.tip_height()).saturating_add(u64::from(BEHIND_BLOCKS));
        let mut empty_claims = Vec::new();
        for (id, peer) in self.peers.iter_mut().filter(|(_, p)| p.ready) {
            let ahead = u64::from(peer.shown_height) > limit;
            match (ahead, peer.ahead_since) {
                (true, None) => peer.ahead_since = Some(now),
                (false, Some(_)) => peer.ahead_since = None,
                (true, Some(since)) => {
                    if !peer.blocked && now.saturating_sub(since) >= CLAIM_SECS {
                        empty_claims.push(*id);
                    }
                }
                (false, None) => {}
            }
        }
        for id in empty_claims {
            self.drop_peer(
                id,
                "NR-33: claimed a height and did not deliver the headers for it".to_string(),
                true,
                out,
            );
        }
    }

    /// Queue a request. A refusal ends the peer.
    fn send(&mut self, id: PeerId, request: Request, out: &mut Vec<Output>) -> bool {
        let result = match self.peers.get_mut(&id) {
            Some(peer) => peer.session.send(request),
            None => return false,
        };
        match result {
            Ok(()) => true,
            Err(d) => {
                self.drop_session(id, d, out);
                false
            }
        }
    }

    /// A session ended. NR-25 applies to every rule of sections 1 and 2, and
    /// not to the timeouts of NR-14.
    fn drop_session(&mut self, id: PeerId, d: Drop, out: &mut Vec<Output>) {
        // NR-25 does not apply to a timeout (NR-14), nor to the encrypted
        // transport (NR-81, NR-83): a path can make those happen.
        let misbehaved = !matches!(d.rule(), "NR-14" | "NR-81" | "NR-83");
        if d == Drop::V2NotSpoken && self.peers.contains_key(&id) {
            out.push(Output::V2NotSpoken { id });
        }
        self.drop_peer(id, d.to_string(), misbehaved, out);
    }

    /// Forget the peer and tell the driver to close it.
    fn drop_peer(&mut self, id: PeerId, reason: String, misbehaved: bool, out: &mut Vec<Output>) {
        if self.peers.remove(&id).is_none() {
            return;
        }
        self.drops = self.drops.saturating_add(1);
        self.release_tx_of(id);
        out.push(Output::Disconnect {
            id,
            reason,
            misbehaved,
        });
    }

    fn push_stale(&mut self, id: PeerId, stale: Stale) {
        if let Some(peer) = self.peers.get_mut(&id) {
            if peer.stale.len() >= MAX_STALE {
                peer.stale.pop_front();
            }
            peer.stale.push_back(stale);
        }
    }

    /// True, and forgets it, when this is a request that was given up.
    fn take_stale(&mut self, id: PeerId, stale: Stale) -> bool {
        let Some(peer) = self.peers.get_mut(&id) else {
            return false;
        };
        match peer.stale.iter().position(|s| *s == stale) {
            Some(index) => {
                peer.stale.remove(index);
                true
            }
            None => false,
        }
    }

    /// A request ran out. Three in a row end the peer, without a mark
    /// against it.
    fn note_unanswered(&mut self, id: PeerId, out: &mut Vec<Output>) {
        let Some(peer) = self.peers.get_mut(&id) else {
            return;
        };
        peer.unanswered = peer.unanswered.saturating_add(1);
        if peer.unanswered >= MAX_UNANSWERED {
            self.drop_peer(
                id,
                "NR-42: three requests in a row were not answered in time".to_string(),
                false,
                out,
            );
        }
    }

    fn answered(&mut self, id: PeerId) {
        if let Some(peer) = self.peers.get_mut(&id) {
            peer.unanswered = 0;
        }
    }

    fn on_event(&mut self, id: PeerId, event: Event, now: u64, out: &mut Vec<Output>) {
        match event {
            Event::Ready {
                services,
                start_height,
            } => self.on_ready(id, services, start_height, out),
            Event::Headers(headers) => self.on_headers(id, &headers, now, out),
            Event::Announced { txids, blocks } => {
                if blocks.iter().any(|b| !self.store.holds(b)) {
                    self.reask(id, now, out);
                }
                // NR-40: a peer that announces a block on our best chain
                // has shown it.
                let height = blocks.iter().filter_map(|b| self.store.height_of(b)).max();
                if let Some(height) = height {
                    self.raise_shown(id, height, now);
                }
                // A block the store does not hold yet is remembered: the
                // header may come from another peer a moment later, and
                // this peer will not announce the block twice.
                let unknown: Vec<BlockHash> = blocks
                    .iter()
                    .filter(|b| self.store.height_of(b).is_none())
                    .copied()
                    .collect();
                if let Some(peer) = self.peers.get_mut(&id) {
                    for hash in unknown {
                        if peer.announced.contains(&hash) {
                            continue;
                        }
                        if peer.announced.len() >= MAX_ANNOUNCED_BLOCKS {
                            peer.announced.pop_front();
                        }
                        peer.announced.push_back(hash);
                    }
                }
                for txid in txids {
                    self.announce(id, txid, out);
                }
            }
            Event::Requested(txids) => self.serve_held(id, &txids, out),
            Event::Tx(bytes) => self.on_tx(id, bytes, out),
            Event::Block(bytes) => self.on_block(id, bytes, out),
            Event::Filter { block, filter } => self.on_filter(id, block, &filter, out),
            Event::NotFound { txids, blocks } => self.on_not_found(id, &txids, &blocks),
            Event::Addresses(list) => {
                if let Some(peer) = self.peers.get(&id) {
                    // The book knows the groups of `network_group` alone, not
                    // the host groups of a check on one machine.
                    out.push(Output::Addresses {
                        from: network_group(&peer.addr),
                        list,
                    });
                }
            }
        }
    }

    /// NR-30: ask for headers from the counter's own chain. The only
    /// `getaddr` goes out now too (NR-22).
    fn on_ready(
        &mut self,
        id: PeerId,
        services: ServiceFlags,
        start_height: u32,
        out: &mut Vec<Output>,
    ) {
        let Some(peer) = self.peers.get_mut(&id) else {
            return;
        };
        peer.ready = true;
        peer.services = services;
        peer.shown_height = start_height;
        peer.shown_at = self.now;
        out.push(Output::Ready { id, services });
        let locator = self.store.locator();
        if self.send(id, Request::GetHeaders { locator }, out) {
            self.send(id, Request::GetAddr, out);
            // NR-86: what is held is announced to a peer that comes late.
            self.announce_held_to_new(id, out);
        }
    }

    /// NR-33, NR-40: the peer has shown this height.
    fn raise_shown(&mut self, id: PeerId, height: u32, now: u64) {
        if let Some(peer) = self.peers.get_mut(&id) {
            if height > peer.shown_height {
                peer.shown_height = height;
                peer.shown_at = now;
            }
        }
    }

    /// NR-33: a block that a peer announced before the store held it, and
    /// that is on the best chain now, is a block that peer has shown.
    fn resolve_announced(&mut self, now: u64) {
        let store = &self.store;
        for peer in self.peers.values_mut() {
            if peer.announced.is_empty() {
                continue;
            }
            let best = peer
                .announced
                .iter()
                .filter_map(|hash| store.height_of(hash))
                .max();
            peer.announced
                .retain(|hash| store.height_of(hash).is_none());
            if let Some(height) = best {
                if height > peer.shown_height {
                    peer.shown_height = height;
                    peer.shown_at = now;
                }
            }
        }
    }

    /// NR-32: ask a peer again from the locator, at most once a minute.
    fn reask(&mut self, id: PeerId, now: u64, out: &mut Vec<Output>) {
        let Some(peer) = self.peers.get_mut(&id) else {
            return;
        };
        let allowed = peer
            .last_reask
            .is_none_or(|t| now.saturating_sub(t) >= REASK_SECS);
        if !allowed {
            return;
        }
        peer.last_reask = Some(now);
        let locator = self.store.locator();
        self.send(id, Request::GetHeaders { locator }, out);
    }

    /// NR-30 to NR-33.
    fn on_headers(&mut self, id: PeerId, headers: &[Header], now: u64, out: &mut Vec<Output>) {
        let mut clean = true;
        let mut blocked = false;
        let mut joined: Vec<(u32, BlockHash)> = Vec::new();
        let mut shown = 0u32;
        let mut refused: Option<String> = None;
        for header in headers {
            let tip_before = self.store.tip_height();
            match self.store.accept(header, now) {
                Ok(Accepted::Known | Accepted::Side) => {}
                Ok(Accepted::Extended) => joined.push(self.store.tip()),
                Ok(Accepted::Reorganized { left }) => {
                    flush_chain(&mut joined, out);
                    let fork =
                        tip_before.saturating_sub(u32::try_from(left.len()).unwrap_or(u32::MAX));
                    out.push(Output::Chain {
                        joined: self.joined_above(fork),
                        left: left.clone(),
                    });
                    self.on_reorganized(fork, &left);
                }
                // NR-32.
                Err(Refusal::UnknownParent) => {
                    clean = false;
                    self.reask(id, now, out);
                    break;
                }
                // WR-06, WR-07, and a header the store itself cannot place:
                // not the peer's doing.
                Err(
                    Refusal::TimeTooFarAhead { .. }
                    | Refusal::SideHeaderTooDeep
                    | Refusal::SideHeadersFull
                    | Refusal::AncestorMissing,
                ) => {
                    clean = false;
                    blocked = true;
                    break;
                }
                // NR-25: any other refusal of WR-05.
                Err(e) => {
                    refused = Some(format!("NR-25: header refused: {e}"));
                    break;
                }
            }
            if let Some(height) = self.store.height_of(&header.block_hash()) {
                shown = shown.max(height);
            }
        }
        flush_chain(&mut joined, out);
        if let Some(reason) = refused {
            self.drop_peer(id, reason, true, out);
            return;
        }
        if let Some(peer) = self.peers.get_mut(&id) {
            if shown > peer.shown_height {
                peer.shown_height = shown;
                peer.shown_at = now;
            }
            if blocked {
                peer.blocked = true;
            } else if clean && !headers.is_empty() {
                peer.blocked = false;
            }
        }
        self.resolve_announced(now);
        // NR-30: a full message means there may be more.
        if clean && headers.len() == MAX_HEADERS {
            let locator = self.store.locator();
            self.send(id, Request::GetHeaders { locator }, out);
        }
    }

    /// The blocks of the best chain above this height, lowest first.
    fn joined_above(&self, fork: u32) -> Vec<(u32, BlockHash)> {
        let tip = self.store.tip_height();
        let mut joined = Vec::new();
        let mut height = fork;
        while height < tip {
            height = match height.checked_add(1) {
                Some(h) => h,
                None => break,
            };
            if let Some(header) = self.store.header_at(height) {
                joined.push((height, header.block_hash()));
            }
        }
        joined
    }

    /// NR-46: work for blocks that left is dropped, and the blocks that
    /// joined are treated as new.
    fn on_reorganized(&mut self, fork: u32, left: &[BlockHash]) {
        let dropped: Vec<u32> = self
            .work
            .iter()
            .filter(|(height, w)| **height > fork || left.contains(&w.hash))
            .map(|(height, _)| *height)
            .collect();
        for height in dropped {
            if let Some(mut item) = self.work.remove(&height) {
                self.abandon_asks(&mut item);
                self.abandon_fetch(&mut item);
            }
        }
        self.next_height = self
            .next_height
            .min(fork.saturating_add(1))
            .max(self.floor());
    }

    /// The requests of this item that are out become requests given up.
    fn abandon_asks(&mut self, item: &mut BlockWork) {
        let open: Vec<PeerId> = item
            .asks
            .iter()
            .filter(|(_, ask)| ask.answer.is_none())
            .map(|(id, _)| *id)
            .collect();
        for id in open {
            self.push_stale(id, Stale::Filter(item.hash));
            item.asks.remove(&id);
        }
    }

    fn abandon_fetch(&mut self, item: &mut BlockWork) {
        if let Some(id) = item.fetch_peer.take() {
            self.push_stale(id, Stale::Block(item.hash));
        }
    }

    // ----- blocks (NR-40 to NR-46) ---------------------------------------

    /// Start work for the next blocks of the best chain and move every item
    /// on, until nothing changes.
    fn advance_blocks(&mut self, now: u64, out: &mut Vec<Output>) {
        loop {
            let mut progressed = self.refill();
            let heights: Vec<u32> = self.work.keys().copied().collect();
            for height in heights {
                let Some(mut item) = self.work.remove(&height) else {
                    continue;
                };
                if self.advance_item(&mut item, now, out) {
                    self.work.insert(height, item);
                } else {
                    progressed = true;
                }
            }
            if !progressed {
                break;
            }
        }
    }

    /// Make work for the best chain's blocks from `next_height` on, up to
    /// the limit. Returns whether any was made.
    fn refill(&mut self) -> bool {
        let floor = self.floor();
        if self.next_height < floor {
            self.next_height = floor;
        }
        let tip = self.store.tip_height();
        let mut added = false;
        while self.work.len() < MAX_BLOCKS_IN_WORK && self.next_height <= tip {
            let Some(header) = self.store.header_at(self.next_height) else {
                break;
            };
            let height = self.next_height;
            self.work
                .insert(height, BlockWork::new(height, header.block_hash()));
            added = true;
            match height.checked_add(1) {
                Some(next) => self.next_height = next,
                None => break,
            }
        }
        added
    }

    /// Ready peers, lowest ID first.
    fn ready_peers(&self) -> Vec<PeerId> {
        self.peers
            .iter()
            .filter(|(_, p)| p.ready)
            .map(|(id, _)| *id)
            .collect()
    }

    /// Ready peers that offer compact filters, lowest ID first.
    fn filter_peers(&self) -> Vec<PeerId> {
        self.peers
            .iter()
            .filter(|(_, p)| p.ready && p.services.has(ServiceFlags::COMPACT_FILTERS))
            .map(|(id, _)| *id)
            .collect()
    }

    /// NR-40: whether this ready peer has shown the block at this height,
    /// long enough ago that its filter index has taken it. A peer that has
    /// shown a higher block has had the time already.
    fn filter_ready(&self, id: PeerId, height: u32, now: u64) -> bool {
        self.peers.get(&id).is_some_and(|p| {
            p.shown_height > height
                || (p.shown_height == height
                    && now >= p.shown_at.saturating_add(FILTER_SETTLE_SECS))
        })
    }

    /// Move one block's work on. False when the block is done.
    fn advance_item(&mut self, item: &mut BlockWork, now: u64, out: &mut Vec<Output>) -> bool {
        item.failed.retain(|id| self.peers.contains_key(id));
        item.asks
            .retain(|id, ask| ask.answer.is_some() || self.peers.contains_key(id));
        if item
            .fetch_peer
            .is_some_and(|id| !self.peers.contains_key(&id))
        {
            item.fetch_peer = None;
        }
        loop {
            match item.stage {
                Stage::Start => {
                    // With nothing to look for, the block is done at once.
                    if !self.all_blocks && self.scripts.is_empty() {
                        return false;
                    }
                    // NR-44, and NR-42 for want of two filter peers.
                    if self.all_blocks || self.filter_peers().len() < MIN_FILTER_PEERS {
                        item.stage = Stage::Fetch;
                    } else {
                        item.stage = Stage::Filters;
                        item.filters_started = now;
                    }
                }
                Stage::Filters => {
                    if self.all_blocks {
                        self.abandon_asks(item);
                        item.stage = Stage::Fetch;
                        continue;
                    }
                    let answers: Vec<Answer> =
                        item.asks.values().filter_map(|ask| ask.answer).collect();
                    if let [first, second, ..] = answers.as_slice() {
                        // NR-42: both answered.
                        if first.digest != second.digest || first.matched || second.matched {
                            item.stage = Stage::Fetch;
                            continue;
                        }
                        return false;
                    }
                    if now >= item.filters_started.saturating_add(ANSWER_SECS) {
                        // NR-42: fewer than two answered in time.
                        let open: Vec<PeerId> = item
                            .asks
                            .iter()
                            .filter(|(_, ask)| ask.answer.is_none())
                            .map(|(id, _)| *id)
                            .collect();
                        self.abandon_asks(item);
                        for id in open {
                            self.note_unanswered(id, out);
                        }
                        item.stage = Stage::Fetch;
                        continue;
                    }
                    self.top_up_filters(item, now, out);
                    return true;
                }
                Stage::Fetch => return self.advance_fetch(item, now, out),
            }
        }
    }

    /// NR-40: ask until two peers in different groups have been asked, but
    /// only peers that have shown the block. The peers take turns by block
    /// height, so that no two peers are asked about every block.
    fn top_up_filters(&mut self, item: &mut BlockWork, now: u64, out: &mut Vec<Output>) {
        let mut peers: Vec<PeerId> = self
            .filter_peers()
            .into_iter()
            .filter(|id| self.filter_ready(*id, item.height, now))
            .collect();
        if let Some(turn) = usize::try_from(item.height)
            .ok()
            .and_then(|h| h.checked_rem(peers.len()))
        {
            peers.rotate_left(turn);
        }
        for id in peers {
            if item.asks.len() >= MIN_FILTER_PEERS || item.asked_total >= MAX_FILTER_ASKS {
                break;
            }
            let Some(group) = self.peers.get(&id).map(|p| p.group) else {
                continue;
            };
            if item.asks.contains_key(&id) || item.asks.values().any(|ask| ask.group == group) {
                continue;
            }
            item.asks.insert(
                id,
                Ask {
                    group,
                    answer: None,
                },
            );
            item.asked_total = item.asked_total.saturating_add(1);
            self.send(
                id,
                Request::GetFilter {
                    height: item.height,
                    block: item.hash,
                },
                out,
            );
        }
    }

    /// NR-45: the block from one peer at a time. True while the block is in
    /// work.
    fn advance_fetch(&mut self, item: &mut BlockWork, now: u64, out: &mut Vec<Output>) -> bool {
        if let Some(id) = item.fetch_peer {
            if now < item.fetch_since.saturating_add(ANSWER_SECS) {
                return true;
            }
            item.fetch_peer = None;
            item.failed.insert(id);
            self.push_stale(id, Stale::Block(item.hash));
            self.note_unanswered(id, out);
        }
        if now < item.resume_at {
            return true;
        }
        let ready = self.ready_peers();
        // NR-45: a peer that has shown the block is asked first.
        let shown = |id: &PeerId| {
            self.peers
                .get(id)
                .is_some_and(|p| p.shown_height >= item.height)
        };
        let pick = ready
            .iter()
            .find(|id| !item.failed.contains(id) && shown(id))
            .or_else(|| ready.iter().find(|id| !item.failed.contains(id)));
        match pick {
            Some(&id) => {
                item.fetch_peer = Some(id);
                item.fetch_since = now;
                self.send(id, Request::GetBlock(item.hash), out);
            }
            None if !ready.is_empty() => {
                // Every ready peer failed: start over after a pause.
                item.failed.clear();
                item.resume_at = now.saturating_add(ANSWER_SECS);
            }
            None => {}
        }
        true
    }

    /// NR-40: a filter arrived.
    fn on_filter(&mut self, id: PeerId, block: BlockHash, filter: &[u8], out: &mut Vec<Output>) {
        let asked = self.work.values().any(|w| {
            w.hash == block
                && w.stage == Stage::Filters
                && w.asks.get(&id).is_some_and(|ask| ask.answer.is_none())
        });
        if !asked {
            if !self.take_stale(id, Stale::Filter(block)) {
                self.drop_peer(
                    id,
                    "NR-25: a filter that was not asked for".to_string(),
                    true,
                    out,
                );
            }
            return;
        }
        // NR-41, NR-42: a filter that cannot be read counts as a match.
        let matched = filter_matches(filter, &block, &self.scripts);
        let answer = Answer {
            digest: sha256::Hash::hash(filter),
            matched,
        };
        if let Some(ask) = self
            .work
            .values_mut()
            .find(|w| w.hash == block && w.stage == Stage::Filters)
            .and_then(|w| w.asks.get_mut(&id))
        {
            ask.answer = Some(answer);
        }
        self.answered(id);
    }

    /// NR-45, NR-43: a block arrived.
    fn on_block(&mut self, id: PeerId, bytes: Vec<u8>, out: &mut Vec<Output>) {
        let hash = bytes
            .get(..80)
            .and_then(|raw| deserialize::<Header>(raw).ok())
            .map(|header| header.block_hash());
        let asked = hash.and_then(|hash| {
            self.work
                .iter()
                .find(|(_, w)| {
                    w.stage == Stage::Fetch && w.fetch_peer == Some(id) && w.hash == hash
                })
                .map(|(height, _)| *height)
        });
        let Some(height) = asked else {
            if hash.is_some_and(|hash| self.take_stale(id, Stale::Block(hash))) {
                return;
            }
            let other = self
                .work
                .values()
                .any(|w| w.stage == Stage::Fetch && w.fetch_peer == Some(id));
            let reason = if other {
                "NR-25: a block other than the one asked for"
            } else {
                "NR-25: a block that was not asked for"
            };
            self.drop_peer(id, reason.to_string(), true, out);
            return;
        };
        let Some(mut item) = self.work.remove(&height) else {
            return;
        };
        self.answered(id);
        let parsed = WellFormedBlock::parse(&bytes);
        let wb = match parsed {
            Ok(wb) if wb.hash() == item.hash => wb,
            other => {
                let reason = match other {
                    Err(e) => format!("NR-25: block refused: {e}"),
                    Ok(_) => "NR-25: a block other than the one asked for".to_string(),
                };
                // Another peer gives it.
                item.fetch_peer = None;
                item.failed.insert(id);
                self.work.insert(height, item);
                self.drop_peer(id, reason, true, out);
                return;
            }
        };
        // WR-01: the block must still be on the best chain.
        if self.store.height_of(&item.hash) != Some(item.height) {
            return;
        }
        // NR-43: a peer whose filter did not match a block that pays a
        // watched script sent a false filter.
        if self.block_pays_watched(&wb, &bytes) {
            let liars: Vec<PeerId> = item
                .asks
                .iter()
                .filter(|(_, ask)| ask.answer.is_some_and(|a| !a.matched))
                .map(|(peer, _)| *peer)
                .collect();
            for peer in liars {
                self.drop_peer(
                    peer,
                    "NR-25: a filter proven false by the block (NR-43)".to_string(),
                    true,
                    out,
                );
            }
        }
        out.push(Output::Block {
            hash: item.hash,
            bytes,
        });
    }

    /// Whether the block pays a script that is watched. The outputs are
    /// scanned once; the watcher's own check confirms each script found. A
    /// payment that the watcher refuses (WR-22) accuses nobody.
    fn block_pays_watched(&self, wb: &WellFormedBlock, bytes: &[u8]) -> bool {
        let Ok(block) = deserialize::<Block>(bytes) else {
            return false;
        };
        let found: BTreeSet<&ScriptBuf> = block
            .txdata
            .iter()
            .skip(1)
            .flat_map(|tx| tx.output.iter())
            .map(|o| &o.script_pubkey)
            .filter(|s| self.scripts.contains(*s))
            .collect();
        for script in found {
            if matches!(wb.payments(&self.store, script), Ok(p) if !p.is_empty()) {
                return true;
            }
        }
        false
    }

    /// A peer does not have what was asked.
    fn on_not_found(&mut self, id: PeerId, txids: &[Txid], blocks: &[BlockHash]) {
        for txid in txids {
            if self.pending_tx.get(txid).is_some_and(|(p, _)| *p == id) {
                self.release_tx(txid);
                self.answered(id);
            }
        }
        let mut answered = false;
        for item in self.work.values_mut() {
            if item.stage == Stage::Fetch
                && item.fetch_peer == Some(id)
                && blocks.contains(&item.hash)
            {
                item.fetch_peer = None;
                item.failed.insert(id);
                answered = true;
            }
        }
        if answered {
            self.answered(id);
        }
    }

    // ----- transactions (NR-50 to NR-54) ---------------------------------

    /// NR-50: this peer says it has this transaction.
    fn announce(&mut self, id: PeerId, txid: Txid, out: &mut Vec<Output>) {
        // NR-87: one of the counter's own broadcasts, heard back.
        if self.heard_held(id, &txid, out) {
            return;
        }
        let Some(addr) = self.peers.get(&id).map(|p| p.addr) else {
            return;
        };
        if let Some(entry) = self.txs.get_mut(&txid) {
            if entry.announcers.iter().any(|a| a.id == id)
                || entry.announcers.len() >= MAX_ANNOUNCERS
            {
                return;
            }
            entry.announcers.push(Announcer {
                id,
                addr,
                tried: false,
            });
            if entry.fetched {
                // NR-53: a further sighting.
                out.push(Output::AlsoAnnounced { txid, from: addr });
            } else if !entry.pending {
                self.queue_tx(txid, true);
            }
            return;
        }
        self.txs.insert(
            txid,
            TxEntry {
                announcers: vec![Announcer {
                    id,
                    addr,
                    tried: false,
                }],
                fetched: false,
                pending: false,
                queued: false,
                in_order: true,
            },
        );
        self.tx_order.push_back(txid);
        self.trim_tx_order();
        self.queue_tx(txid, false);
    }

    /// NR-54: forget the oldest IDs beyond the bound. An ID with a request
    /// pending leaves the order and stays until the request ends.
    fn trim_tx_order(&mut self) {
        while self.tx_order.len() > MAX_REMEMBERED_TXIDS {
            let Some(old) = self.tx_order.pop_front() else {
                break;
            };
            let pending = self.txs.get(&old).is_some_and(|e| e.pending);
            if pending {
                if let Some(entry) = self.txs.get_mut(&old) {
                    entry.in_order = false;
                }
            } else {
                self.txs.remove(&old);
            }
        }
    }

    /// Put a transaction in the line to be asked for, at the front when it
    /// is asked again. The line has the bound of the remembered IDs.
    fn queue_tx(&mut self, txid: Txid, front: bool) {
        match self.txs.get_mut(&txid) {
            Some(entry) if !entry.queued => entry.queued = true,
            _ => return,
        }
        if self.waiting.len() >= MAX_REMEMBERED_TXIDS {
            let dropped = if front {
                self.waiting.pop_back()
            } else {
                self.waiting.pop_front()
            };
            if let Some(entry) = dropped.and_then(|old| self.txs.get_mut(&old)) {
                entry.queued = false;
            }
        }
        if front {
            self.waiting.push_front(txid);
        } else {
            self.waiting.push_back(txid);
        }
    }

    /// A request for a transaction ended without the transaction. The next
    /// announcer is tried at the next pass.
    fn release_tx(&mut self, txid: &Txid) {
        self.pending_tx.remove(txid);
        let Some(entry) = self.txs.get_mut(txid) else {
            return;
        };
        entry.pending = false;
        let again = entry.announcers.iter().any(|a| !a.tried);
        self.return_to_order(txid);
        if again {
            self.queue_tx(*txid, true);
        }
    }

    /// An entry that left the order while its request was out comes back.
    fn return_to_order(&mut self, txid: &Txid) {
        let Some(entry) = self.txs.get_mut(txid) else {
            return;
        };
        if !entry.in_order {
            entry.in_order = true;
            self.tx_order.push_back(*txid);
            self.trim_tx_order();
        }
    }

    fn release_tx_of(&mut self, id: PeerId) {
        let owned: Vec<Txid> = self
            .pending_tx
            .iter()
            .filter(|(_, (peer, _))| *peer == id)
            .map(|(txid, _)| *txid)
            .collect();
        for txid in owned {
            self.release_tx(&txid);
        }
    }

    /// NR-51: ask for announced transactions, from the first announcer that
    /// has not been asked, while there is room.
    fn pump_tx(&mut self, now: u64, out: &mut Vec<Output>) {
        if !self.fetch_transactions {
            return;
        }
        let mut batches: BTreeMap<PeerId, Vec<Txid>> = BTreeMap::new();
        let mut per_peer: BTreeMap<PeerId, usize> = BTreeMap::new();
        for (id, _) in self.pending_tx.values() {
            let n = per_peer.entry(*id).or_default();
            *n = n.saturating_add(1);
        }
        // Transactions whose announcers all have a full share keep their
        // place in the line. One pass looks at each waiting one once.
        let mut held: Vec<Txid> = Vec::new();
        let mut looked = 0usize;
        let limit = self.waiting.len();
        while self.pending_tx.len() < MAX_PENDING_TXS && looked < limit {
            let Some(txid) = self.waiting.pop_front() else {
                break;
            };
            looked = looked.saturating_add(1);
            let Some(entry) = self.txs.get_mut(&txid) else {
                continue;
            };
            entry.queued = false;
            if entry.fetched || entry.pending {
                continue;
            }
            let peers = &self.peers;
            let untried = |a: &Announcer| !a.tried && peers.get(&a.id).is_some_and(|p| p.ready);
            let has_room = |a: &Announcer| {
                per_peer.get(&a.id).copied().unwrap_or(0) < MAX_PENDING_TXS_PER_PEER
            };
            let Some(slot) = entry
                .announcers
                .iter_mut()
                .find(|a| untried(a) && has_room(a))
            else {
                if entry.announcers.iter().any(untried) {
                    entry.queued = true;
                    held.push(txid);
                }
                continue;
            };
            slot.tried = true;
            let id = slot.id;
            let n = per_peer.entry(id).or_default();
            *n = n.saturating_add(1);
            entry.pending = true;
            self.pending_tx
                .insert(txid, (id, now.saturating_add(TX_ANSWER_SECS)));
            batches.entry(id).or_default().push(txid);
        }
        for txid in held.into_iter().rev() {
            self.waiting.push_front(txid);
        }
        for (id, txids) in batches {
            for chunk in txids.chunks(MAX_TXIDS_PER_REQUEST) {
                if !self.send(id, Request::GetTx(chunk.to_vec()), out) {
                    break;
                }
            }
        }
    }

    /// NR-51, NR-53: a transaction arrived.
    fn on_tx(&mut self, id: PeerId, bytes: Vec<u8>, out: &mut Vec<Output>) {
        let Ok(tx) = deserialize::<Transaction>(&bytes) else {
            self.drop_peer(
                id,
                "NR-25: a transaction that does not parse".to_string(),
                true,
                out,
            );
            return;
        };
        let txid = tx.compute_txid();
        if !self.pending_tx.get(&txid).is_some_and(|(p, _)| *p == id) {
            if !self.take_stale(id, Stale::Tx(txid)) {
                self.drop_peer(
                    id,
                    "NR-25: a transaction that was not asked for".to_string(),
                    true,
                    out,
                );
            }
            return;
        }
        self.pending_tx.remove(&txid);
        self.answered(id);
        let from = match self.txs.get_mut(&txid) {
            Some(entry) => {
                entry.pending = false;
                entry.fetched = true;
                entry.announcers.iter().map(|a| a.addr).collect()
            }
            None => Vec::new(),
        };
        self.return_to_order(&txid);
        out.push(Output::Sighting { tx: bytes, from });
    }
}

/// Send what joined as one `Chain` output.
fn flush_chain(joined: &mut Vec<(u32, BlockHash)>, out: &mut Vec<Output>) {
    if !joined.is_empty() {
        out.push(Output::Chain {
            joined: std::mem::take(joined),
            left: Vec::new(),
        });
    }
}

/// NR-41, NR-42: whether the filter matches any watched script. A filter
/// that cannot be read counts as a match. The count of elements is checked
/// first: no filter of at most 1,000,000 bytes has more than one element
/// for every 2 bits, and the reader multiplies the count.
fn filter_matches(filter: &[u8], block: &BlockHash, scripts: &BTreeSet<ScriptBuf>) -> bool {
    let readable = match deserialize_partial::<VarInt>(filter) {
        Ok((VarInt(count), used)) => {
            let bits = u64::try_from(filter.len().saturating_sub(used))
                .unwrap_or(u64::MAX)
                .saturating_mul(8);
            count <= bits
        }
        Err(_) => false,
    };
    if !readable {
        return true;
    }
    let query = scripts.iter().map(|s| s.as_bytes());
    BlockFilter::new(filter)
        .match_any(block, query)
        .unwrap_or(true)
}
