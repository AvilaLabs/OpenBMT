//! The driver: one core thread that owns the manager and the address book,
//! reader threads that move bytes in, dial threads that open connections,
//! and the app's handle.
//!
//! ```text
//!   reader thread (one per connection) --\
//!   dial thread (one per attempt) --------+--> bounded channel --> core thread
//!   seed lookup thread -------------------/                          |  Manager
//!   app commands (own channel + a wake-up) ------------------------>|  AddressBook
//!                                                                    |  writes to sockets
//!   app <-- bounded event channel <----------------------------------/
//! ```
//!
//! Only the core thread touches the manager, the book, the storage, the
//! clock and the randomness. It writes to the sockets itself, with a write
//! timeout, so a peer that does not read cannot hold it for more than
//! [`WRITE_TIMEOUT_SECS`] seconds.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fmt;
use std::io::{ErrorKind, Read, Write};
use std::net::{IpAddr, Shutdown, SocketAddr, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{
    channel, sync_channel, Receiver, RecvError, RecvTimeoutError, Sender, SyncSender, TryRecvError,
    TrySendError,
};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use bitcoin::p2p::address::AddrV2;
use bitcoin::p2p::ServiceFlags;
use bitcoin::{BlockHash, Network, ScriptBuf, Txid};
use openbmt_net::book::{BAN_SECS, V2_RETRY_SECS};
use openbmt_net::manager::Wanted;
use openbmt_net::onion::{onion_name, parse_onion_name};
use openbmt_net::{
    Added, AddressBook, ConnectRefused, Manager, ManagerConfig, OnionDraw, Output, PeerAddress,
    PeerId, RelayState, Source, TransportChoice, V2Entropy,
};
use openbmt_watch::{host_group, network_group, HeaderStore, Peer, PeerAddr};

use crate::dial::{DialError, Net, RESOLVE_SECS};
use crate::env::{Clock, Key, Randomness, Storage};
use crate::socks::{ReplyCode, SocksError, Target};

/// Most dials in flight at once.
pub const MAX_DIALS_IN_FLIGHT: usize = 4;
/// Sockets the driver may hold beyond the target number of peers: the
/// connections being opened.
pub const EXTRA_SOCKETS: usize = 4;
/// The largest target number of peers the driver accepts.
pub const MAX_TARGET_PEERS: usize = 64;
/// NR-70: seconds an address that closed or failed is not dialed again.
pub const REDIAL_PAUSE_SECS: u64 = 30;
/// NR-73: seconds between two settings of the anchors.
pub const ANCHOR_SECS: u64 = 600;
/// NR-73: seconds that must pass between two saves of the headers.
pub const HEADER_SAVE_SECS: u64 = 60;
/// NR-75: most seconds between two [`AppEvent::Peers`].
pub const PEERS_EVENT_SECS: u64 = 10;
/// NR-72: seconds between two rounds of seed lookups.
pub const SEED_ROUND_SECS: u64 = 300;
/// NR-72: with Tor on, lookups of each seed's filter name per round, each
/// on a circuit of its own.
pub const TOR_LOOKUPS_PER_SEED: usize = 3;
/// NR-72: most Tor lookups in flight at once; the rest wait their turn.
pub const MAX_TOR_LOOKUPS_IN_FLIGHT: usize = 4;
/// NR-72: seconds a round of Tor lookups may take in all, before the
/// answers still missing are ignored. Three waves of the longest lookup.
pub const TOR_ROUND_SECS: u64 = 100;
/// Bytes one read from a socket may return.
pub const READ_BUFFER: usize = 65_536;
/// Messages waiting between the reader threads and the core thread.
pub const INBOUND_CAPACITY: usize = 256;
/// Events waiting for the app.
pub const EVENT_CAPACITY: usize = 64;
/// NR-74: bytes of blocks and transactions that may wait for the app. A
/// starting value, to be tuned. One event larger than this is still
/// delivered, alone.
pub const EVENT_BYTES: usize = 32 * 1024 * 1024;
/// Seconds a write to a peer may take before the peer is closed.
pub const WRITE_TIMEOUT_SECS: u64 = 3;
/// Seconds a reader thread waits in one read before it looks for a stop.
pub const READ_TIMEOUT_SECS: u64 = 1;
/// Seconds `shutdown` waits for the threads before it lets go of the ones
/// that are still in a connect or a lookup.
pub const SHUTDOWN_SECS: u64 = 5;
/// The longest the core thread waits for a message.
const WAIT_MILLIS: u64 = 200;
/// Messages the core thread takes from the channel before it ticks.
const BATCH: usize = 256;
/// Draws from the book for one dial.
const PICK_TRIES: usize = 8;

/// What the driver is told and not left to decide.
#[derive(Debug, Clone)]
pub struct DriverConfig {
    /// The network the counter is on.
    pub network: Network,
    /// NR-20: how many connections to keep (8).
    pub target_peers: usize,
    /// NR-40: blocks above this height are checked.
    pub watch_from: u32,
    /// NR-26: `Some` means Tor is on, and every connection goes through
    /// this SOCKS5 proxy.
    pub proxy: Option<SocketAddr>,
    /// DNS seed host names. Empty on regtest.
    pub seeds: Vec<String>,
    /// Peers that are dialed first and kept. With `allow_local`, these may
    /// be on this machine, for a check against a node here.
    pub fixed_peers: Vec<SocketAddr>,
    /// Take a peer at a local or private address. Off in the counter.
    pub allow_local: bool,
    /// NR-70: seconds before an address that closed or failed is dialed
    /// again. [`REDIAL_PAUSE_SECS`] unless a test shortens it.
    pub redial_pause_secs: u64,
    /// NR-74: bytes of events that may wait for the app.
    /// [`EVENT_BYTES`] unless a test shortens it.
    pub event_bytes: usize,
    /// NR-80: try the encrypted transport first. On in the counter; off only
    /// to check the driver against peers that speak the old one alone.
    pub v2: bool,
}

/// The DNS seed names that Bitcoin Core 32.1 ships for the network
/// (`kernel/chainparams.cpp`), without the final dot. Empty for a network
/// that has none.
pub fn core_seeds(network: Network) -> Vec<String> {
    let names: &[&str] = match network {
        Network::Bitcoin => &[
            "dnsseed.bluematt.me",
            "seed.bitcoin.jonasschnelli.ch",
            "seed.btc.petertodd.net",
            "seed.bitcoin.sprovoost.nl",
            "dnsseed.emzy.de",
            "seed.bitcoin.wiz.biz",
            "seed.mainnet.achownodes.xyz",
        ],
        Network::Signet => &[
            "seed.signet.bitcoin.sprovoost.nl",
            "seed.signet.achownodes.xyz",
        ],
        _ => &[],
    };
    names.iter().map(|n| (*n).to_string()).collect()
}

/// NR-72: the prefix that asks a seed only for nodes that state the
/// network, witness and compact-filter services (bits 1, 8 and 64).
pub const FILTER_SEED_PREFIX: &str = "x49.";

impl DriverConfig {
    /// The counter's settings: 8 peers, no proxy, no seeds, no fixed peers,
    /// no local addresses.
    pub fn new(network: Network, watch_from: u32) -> Self {
        Self {
            network,
            target_peers: openbmt_net::manager::DEFAULT_TARGET_PEERS,
            watch_from,
            proxy: None,
            seeds: core_seeds(network),
            fixed_peers: Vec::new(),
            allow_local: false,
            redial_pause_secs: REDIAL_PAUSE_SECS,
            event_bytes: EVENT_BYTES,
            v2: true,
        }
    }
}

/// What the app sees.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppEvent {
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
    /// NR-53: a fetched transaction and where the peers that announced it
    /// were reached.
    Sighting {
        /// The transaction as the peer sent it.
        tx: Vec<u8>,
        /// The peers.
        from: Vec<PeerAddr>,
    },
    /// NR-53: a peer announced a transaction that was fetched already.
    AlsoAnnounced {
        /// The transaction.
        txid: Txid,
        /// Where the peer was reached.
        from: PeerAddr,
    },
    /// NR-75: the ready peers, the earliest time since which a peer has
    /// claimed a height above the tip without proving it, and the tip's
    /// height and time. Sent whenever the ready peers or the tip change, and
    /// at least every 10 seconds, so that the app can run the health check.
    Peers(Vec<Peer>, Option<u64>, (u32, u64)),
    /// Every block above `watch_from` up to and including this height has
    /// been checked against the scripts that were watched. Sent when it
    /// rises. The app stores it and passes it as `watch_from` at the next
    /// start, so that a restart does not check the same blocks again.
    Checked(u32),
    /// A requested rescan has been applied to the manager. Events before this
    /// marker may describe work started with an older watch set.
    RescanStarted(u32),
    /// NR-86, NR-90: how many peers a transaction the app asked to broadcast
    /// was announced to and how many have asked for it. Sent when either
    /// count changes.
    TxRelayed {
        /// The transaction.
        txid: Txid,
        /// Peers that were sent an announcement.
        peers_asked: usize,
        /// Peers that asked for it. A peer that asked may have discarded it.
        peers_fetched: usize,
    },
    /// NR-87, NR-88: what the peers have shown of a transaction the app
    /// asked to broadcast. Sent when it changes. A confirmation is not
    /// here: the app reads that from its own checked blocks.
    TxState {
        /// The transaction.
        txid: Txid,
        /// What the peers have shown.
        state: RelayState,
    },
    /// Something the app may want to show or log: a stored file that did not
    /// read back, a save that failed, a peer that was dropped for breaking a
    /// rule.
    Note(String),
    /// The driver has stopped. Sent after `shutdown` if there is room.
    Stopped,
}

/// Why [`Driver::spawn`] did not start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartError {
    /// `target_peers` is 0 or more than [`MAX_TARGET_PEERS`].
    TargetPeers(usize),
    /// The watcher's header store does not take this network.
    Network(String),
    /// The storage could not be read.
    Storage(String),
    /// A thread could not be started.
    Thread(ErrorKind),
}

impl fmt::Display for StartError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TargetPeers(n) => write!(
                f,
                "NR-20: {n} peers is not between 1 and {MAX_TARGET_PEERS}"
            ),
            Self::Network(e) => write!(f, "the network is not supported: {e}"),
            Self::Storage(e) => write!(f, "the stored data could not be read: {e}"),
            Self::Thread(k) => write!(f, "a thread could not be started: {k}"),
        }
    }
}

impl std::error::Error for StartError {}

/// What the app asks of the core thread.
enum Command {
    WatchScript(ScriptBuf),
    AllBlocks(bool),
    FetchTransactions(bool),
    Broadcast(Vec<u8>),
    Forget(Txid),
    Rescan(u32),
}

/// What the threads send the core thread.
enum Msg {
    /// Look at the commands.
    Wake,
    Data(PeerId, Vec<u8>),
    Closed(PeerId),
    Dialed(PeerId, Result<TcpStream, DialError>),
    /// A seed lookup of this round: the name, and the addresses or the
    /// reason it failed.
    Resolved(u64, String, Result<Vec<SocketAddr>, String>),
    /// A seed lookup through the Tor proxy: the round, the name, and the
    /// one address or the reason it failed.
    ProxyResolved(u64, String, Result<IpAddr, SocksError>),
}

/// What the threads share.
#[derive(Debug, Default)]
pub(crate) struct Shared {
    /// Set when the app asks to stop.
    stop: AtomicBool,
    /// Chunks the reader threads have read, for the tests of the limits.
    pub(crate) chunks_read: AtomicU64,
}

/// The bytes an event holds that the driver counts against the budget of
/// [`EVENT_BYTES`]: the payload of a block or a transaction.
fn payload_bytes(event: &AppEvent) -> usize {
    match event {
        AppEvent::Block { bytes, .. } => bytes.len(),
        AppEvent::Sighting { tx, .. } => tx.len(),
        _ => 0,
    }
}

/// Take `size` off a count of queued bytes, never below zero.
fn release(queued: &AtomicUsize, size: usize) {
    let mut now = queued.load(Ordering::SeqCst);
    loop {
        let next = now.saturating_sub(size);
        match queued.compare_exchange_weak(now, next, Ordering::SeqCst, Ordering::SeqCst) {
            Ok(_) => return,
            Err(seen) => now = seen,
        }
    }
}

/// The app's end of the channel of events (NR-74). It takes the events in
/// order, and each one it takes frees its bytes in the driver's budget, so
/// read through it and not through anything else.
#[derive(Debug)]
pub struct EventReceiver {
    events: Receiver<AppEvent>,
    queued: Arc<AtomicUsize>,
}

impl EventReceiver {
    /// Wrap a plain channel that no driver counts, for an app's own test
    /// double of the driver.
    pub fn unmetered(events: Receiver<AppEvent>) -> Self {
        Self {
            events,
            queued: Arc::default(),
        }
    }

    /// Wait for the next event.
    pub fn recv(&self) -> Result<AppEvent, RecvError> {
        self.events.recv().map(|e| self.taken(e))
    }

    /// Wait for the next event for up to `timeout`.
    pub fn recv_timeout(&self, timeout: Duration) -> Result<AppEvent, RecvTimeoutError> {
        self.events.recv_timeout(timeout).map(|e| self.taken(e))
    }

    /// The next event if one is waiting.
    pub fn try_recv(&self) -> Result<AppEvent, TryRecvError> {
        self.events.try_recv().map(|e| self.taken(e))
    }

    /// The bytes of blocks and transactions in events that the driver has
    /// handed over and the app has not taken.
    pub fn queued_bytes(&self) -> usize {
        self.queued.load(Ordering::SeqCst)
    }

    fn taken(&self, event: AppEvent) -> AppEvent {
        release(&self.queued, payload_bytes(&event));
        event
    }
}

impl From<Receiver<AppEvent>> for EventReceiver {
    fn from(events: Receiver<AppEvent>) -> Self {
        Self::unmetered(events)
    }
}

/// The app's end of the driver.
///
/// The event channel holds at most [`EVENT_CAPACITY`] events and at most
/// [`EVENT_BYTES`] bytes of blocks and transactions (NR-74). If the app does
/// not read it, the core thread waits for room, and so it reads nothing more
/// from the peers: the reader threads then fill the channel of
/// [`INBOUND_CAPACITY`] messages and wait too, and the peers wait on their
/// sockets. The driver holds back rather than grow without bound. Blocks are
/// the largest events, up to 4,000,000 bytes each (NR-02), so the count alone
/// would allow 64 of them; the byte budget stops at about [`EVENT_BYTES`]
/// instead, plus the one event the core thread holds. An event that is
/// larger than the budget is delivered once the app has taken the others.
#[derive(Debug)]
pub struct Handle {
    events: EventReceiver,
    commands: Sender<Command>,
    wake: SyncSender<Msg>,
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
}

impl Handle {
    /// The events, in order. Read them: see the note on [`Handle`].
    pub fn events(&self) -> &EventReceiver {
        &self.events
    }

    /// NR-41: watch a script. A script that is added after peers are
    /// connected is checked only against blocks that are fetched after it
    /// is added.
    pub fn watch_script(&self, script: ScriptBuf) {
        self.command(Command::WatchScript(script));
    }

    /// NR-44: while on, every new block is downloaded in full.
    pub fn set_all_blocks(&self, on: bool) {
        self.command(Command::AllBlocks(on));
    }

    /// NR-51, NR-52: whether announced transactions are fetched.
    pub fn set_fetch_transactions(&self, on: bool) {
        self.command(Command::FetchTransactions(on));
    }

    /// NR-86, NR-90: hold a finished transaction (consensus bytes) and send
    /// it to the peers. One that is refused (too heavy, not a transaction,
    /// too many held) comes back as a [`AppEvent::Note`]. What the peers do
    /// with it comes back as [`AppEvent::TxRelayed`] and
    /// [`AppEvent::TxState`], until [`Handle::forget`].
    pub fn broadcast(&self, tx: Vec<u8>) {
        self.command(Command::Broadcast(tx));
    }

    /// NR-86, NR-90: stop holding a transaction given to [`Handle::broadcast`].
    pub fn forget(&self, txid: Txid) {
        self.command(Command::Forget(txid));
    }

    /// NR-90: check every block above this height again, against all the
    /// scripts watched now.
    pub fn rescan_from(&self, height: u32) {
        self.command(Command::Rescan(height));
    }

    /// Stop. Saves the headers and the address book, closes every socket and
    /// joins the threads. A thread that is still in a connect or a seed
    /// lookup after [`SHUTDOWN_SECS`] is let go: it holds no data and ends
    /// by itself. Safe to call more than once.
    pub fn shutdown(&mut self) {
        self.shared.stop.store(true, Ordering::SeqCst);
        let _ = self.wake.try_send(Msg::Wake);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }

    fn command(&self, command: Command) {
        // After a stop the core thread is gone and the command is dropped.
        if self.commands.send(command).is_ok() {
            let _ = self.wake.try_send(Msg::Wake);
        }
    }

    #[cfg(test)]
    pub(crate) fn shared(&self) -> &Arc<Shared> {
        &self.shared
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// Starts the driver.
#[derive(Debug)]
pub struct Driver;

impl Driver {
    /// Start the core thread and return the app's handle.
    ///
    /// `scripts` are watched before the first peer is connected. A block is
    /// checked against the scripts that are watched when its work starts, and
    /// a block whose work found nothing to look for is finished at once; so
    /// the scripts must be known before the first peer sends a header, or
    /// the blocks from `watch_from` on that arrive before them are not
    /// checked against them.
    ///
    /// The stored headers and address book are read here. A file that is
    /// not there starts empty; a file that does not read back also starts
    /// empty and is reported as the first [`AppEvent::Note`]; a storage that
    /// cannot be read at all is a [`StartError`].
    pub fn spawn<S, C, R>(
        config: DriverConfig,
        storage: S,
        clock: C,
        randomness: R,
        scripts: Vec<ScriptBuf>,
    ) -> Result<Handle, StartError>
    where
        S: Storage + Send + 'static,
        C: Clock + Send + 'static,
        R: Randomness + Send + 'static,
    {
        Self::spawn_with(config, storage, clock, randomness, scripts, Net::system())
    }

    pub(crate) fn spawn_with<S, C, R>(
        config: DriverConfig,
        mut storage: S,
        clock: C,
        randomness: R,
        scripts: Vec<ScriptBuf>,
        net: Net,
    ) -> Result<Handle, StartError>
    where
        S: Storage + Send + 'static,
        C: Clock + Send + 'static,
        R: Randomness + Send + 'static,
    {
        if config.target_peers == 0 || config.target_peers > MAX_TARGET_PEERS {
            return Err(StartError::TargetPeers(config.target_peers));
        }
        let now = clock.now();
        let mut notes = Vec::new();
        let stored = storage
            .load(Key::Headers)
            .map_err(|e| StartError::Storage(format!("headers: {e}")))?;
        let new_store =
            || HeaderStore::new(config.network).map_err(|e| StartError::Network(e.to_string()));
        let store = match stored {
            None => new_store()?,
            Some(bytes) => match HeaderStore::from_bytes(config.network, &bytes) {
                Ok(store) => store,
                Err(e) => {
                    notes.push(format!(
                        "the stored headers do not read back ({e}); starting from the first header"
                    ));
                    new_store()?
                }
            },
        };
        let stored = storage
            .load(Key::Book)
            .map_err(|e| StartError::Storage(format!("address book: {e}")))?;
        let book = match stored {
            None => AddressBook::new(),
            Some(bytes) => match AddressBook::from_bytes(&bytes, now) {
                Ok(book) => book,
                Err(e) => {
                    notes.push(format!(
                        "the stored address book does not read back ({e}); starting with an empty one"
                    ));
                    AddressBook::new()
                }
            },
        };
        let mut manager = Manager::new(
            ManagerConfig {
                network: config.network,
                target_peers: config.target_peers,
                watch_from: config.watch_from,
                allow_local: config.allow_local,
            },
            store,
        );
        for script in scripts {
            if !manager.watch_script(script) {
                notes.push("too many scripts: one was not watched".to_string());
            }
        }
        let (inbound_tx, inbound_rx) = sync_channel(INBOUND_CAPACITY);
        let (events_tx, events_rx) = sync_channel(EVENT_CAPACITY);
        let queued = Arc::new(AtomicUsize::new(0));
        let (command_tx, command_rx) = channel();
        let shared = Arc::new(Shared::default());
        let tip = manager.chain().tip().1;
        let core = Core {
            initial: initial_targets(&book, config.proxy.is_some()),
            tip_saved: tip,
            config,
            storage,
            clock,
            rng: randomness,
            net,
            manager,
            book,
            inbound_tx: inbound_tx.clone(),
            inbound_rx,
            commands: command_rx,
            events: events_tx,
            queued: queued.clone(),
            shared: shared.clone(),
            notes,
            next_id: 1,
            conns: BTreeMap::new(),
            dialing: BTreeMap::new(),
            retired: Vec::new(),
            pause: BTreeMap::new(),
            banned: BTreeMap::new(),
            v2: V2Track::default(),
            noted_local: BTreeSet::new(),
            seed_cursor: 0,
            lookup_unsupported: false,
            last_round_empty: false,
            round: None,
            round_count: 0,
            last_round: None,
            last_tick: None,
            last_peers: None,
            last_peers_at: 0,
            last_checked: None,
            last_header_save: now,
            next_anchor_at: now.saturating_add(ANCHOR_SECS),
        };
        let thread = thread::Builder::new()
            .name("net-core".to_string())
            .spawn(move || core.run())
            .map_err(|e| StartError::Thread(e.kind()))?;
        Ok(Handle {
            events: EventReceiver {
                events: events_rx,
                queued,
            },
            commands: command_tx,
            wake: inbound_tx,
            shared,
            thread: Some(thread),
        })
    }
}

// ----- addresses ---------------------------------------------------------

/// The ways to reach a book entry. An address is dialed by its IP address.
/// A Tor v3 address is dialed only through the proxy, by its host name
/// (NR-92): without one it has no target. Other kinds are not dialed.
fn target_of(addr: &AddrV2, port: u16, proxy: bool) -> Option<Target> {
    match addr {
        AddrV2::Ipv4(ip) => Some(Target::Addr(SocketAddr::new(IpAddr::V4(*ip), port))),
        AddrV2::Ipv6(ip) => Some(Target::Addr(SocketAddr::new(IpAddr::V6(*ip), port))),
        AddrV2::TorV3(key) if proxy => Some(Target::Name(onion_name(key), port)),
        _ => None,
    }
}

/// The public key of the onion address that a target names, if it is one.
fn onion_key(target: &Target) -> Option<[u8; 32]> {
    match target {
        Target::Name(name, _) => parse_onion_name(name),
        Target::Addr(_) => None,
    }
}

/// The book's key for an address that was dialed, if it has one.
fn book_key(target: &Target) -> Option<(AddrV2, u16)> {
    match target {
        Target::Addr(SocketAddr::V4(a)) => Some((AddrV2::Ipv4(*a.ip()), a.port())),
        Target::Addr(SocketAddr::V6(a)) => Some((AddrV2::Ipv6(*a.ip()), a.port())),
        Target::Name(_, port) => Some((AddrV2::TorV3(onion_key(target)?), *port)),
    }
}

/// NR-93: an onion peer is grouped by its key, a seed name as a name.
fn peer_addr(target: &Target) -> PeerAddr {
    match (target, onion_key(target)) {
        (_, Some(key)) => PeerAddr::Onion(key),
        (Target::Addr(a), None) => PeerAddr::Ip(a.ip()),
        (Target::Name(..), None) => PeerAddr::Named,
    }
}

/// NR-94: the most onion peers, connected and being opened, for a target
/// number of peers: half of it, rounded down, and at least 1.
pub(crate) fn onion_cap(target_peers: usize) -> usize {
    (target_peers / 2).max(1)
}

/// NR-24: the stored anchors, to be dialed first. An onion anchor without a
/// proxy is skipped and stays in the book.
fn initial_targets(book: &AddressBook, proxy: bool) -> VecDeque<Target> {
    book.anchors()
        .iter()
        .filter_map(|(addr, port)| target_of(addr, *port, proxy))
        .collect()
}

/// NR-24: the two entries that have been connected longest, the oldest
/// first. Each is an address, a port and the time the handshake finished.
pub(crate) fn longest_connected(ready: &[(AddrV2, u16, u64)]) -> Vec<(AddrV2, u16)> {
    let mut sorted: Vec<&(AddrV2, u16, u64)> = ready.iter().collect();
    sorted.sort_by_key(|(_, _, since)| *since);
    sorted
        .into_iter()
        .take(openbmt_net::book::MAX_ANCHORS)
        .map(|(addr, port, _)| (addr.clone(), *port))
        .collect()
}

fn default_port(network: Network) -> u16 {
    match network {
        Network::Signet => 38_333,
        Network::Regtest => 18_444,
        _ => 8_333,
    }
}

fn v2_of(ip: IpAddr) -> AddrV2 {
    match ip {
        IpAddr::V4(a) => AddrV2::Ipv4(a),
        IpAddr::V6(a) => AddrV2::Ipv6(a),
    }
}

// ----- the core thread ---------------------------------------------------

/// A connection that is open.
struct Conn {
    /// The core thread's end, for writing.
    stream: TcpStream,
    reader: Option<JoinHandle<()>>,
    target: Target,
    /// When the handshake finished.
    ready_at: Option<u64>,
}

/// A connection being opened.
struct Dial {
    target: Target,
    handle: Option<JoinHandle<()>>,
}

/// A round of seed lookups.
struct Round {
    id: u64,
    started: u64,
    /// Seconds after `started` that answers are still taken.
    window: u64,
    /// Lookups not answered yet, those waiting included.
    outstanding: usize,
    /// Tor lookups waiting for a free place, by the name to ask.
    queue: VecDeque<String>,
    /// Tor lookups in a thread now.
    in_flight: usize,
    /// Tor lookups that gave an address the book took, and that failed.
    usable: usize,
    failed: usize,
}

/// NR-80, NR-81: what the driver keeps about the encrypted transport.
#[derive(Default)]
struct V2Track {
    /// Connections whose peer did not speak it, until they are removed.
    refused: BTreeSet<PeerId>,
    /// Targets that get the old transport until this time, for the ones that
    /// the address book does not hold.
    v1_until: BTreeMap<Target, u64>,
}

/// What `Peers` carried last.
#[derive(PartialEq)]
struct Snapshot {
    peers: Vec<Peer>,
    ahead_since: Option<u64>,
    tip: (u32, u64),
}

struct Core<S, C, R> {
    config: DriverConfig,
    storage: S,
    clock: C,
    rng: R,
    net: Net,
    manager: Manager,
    book: AddressBook,
    inbound_tx: SyncSender<Msg>,
    inbound_rx: Receiver<Msg>,
    commands: Receiver<Command>,
    events: SyncSender<AppEvent>,
    /// NR-74: bytes of blocks and transactions handed to `events` that the
    /// app has not taken.
    queued: Arc<AtomicUsize>,
    shared: Arc<Shared>,
    /// Notes from the start, sent first.
    notes: Vec<String>,
    next_id: u64,
    conns: BTreeMap<PeerId, Conn>,
    dialing: BTreeMap<PeerId, Dial>,
    /// Reader threads of closed connections, not yet joined.
    retired: Vec<JoinHandle<()>>,
    /// NR-70: not dialed before this time.
    pause: BTreeMap<Target, u64>,
    /// NR-25: not dialed before this time, for peers the book may not hold
    /// (a peer on this machine, or a seed name).
    banned: BTreeMap<Target, u64>,
    v2: V2Track,
    noted_local: BTreeSet<Target>,
    /// Anchors, dialed first after a start.
    initial: VecDeque<Target>,
    seed_cursor: usize,
    /// NR-72: the proxy answered a lookup with "command not supported", so
    /// it is not Tor; the seed names are dialed as peers from then on.
    lookup_unsupported: bool,
    /// NR-72: the last Tor round brought no address, so a seed name may be
    /// dialed as a peer until the next round starts.
    last_round_empty: bool,
    round: Option<Round>,
    round_count: u64,
    last_round: Option<u64>,
    last_tick: Option<u64>,
    last_peers: Option<Snapshot>,
    last_peers_at: u64,
    last_checked: Option<u32>,
    tip_saved: BlockHash,
    last_header_save: u64,
    next_anchor_at: u64,
}

/// The reader thread of one connection: bytes in, until the socket ends.
fn read_loop(id: PeerId, mut stream: TcpStream, tx: SyncSender<Msg>, shared: Arc<Shared>) {
    let mut buf = vec![0u8; READ_BUFFER];
    loop {
        if shared.stop.load(Ordering::Relaxed) {
            return;
        }
        match stream.read(&mut buf) {
            Ok(0) => {
                let _ = tx.send(Msg::Closed(id));
                return;
            }
            Ok(n) => {
                shared.chunks_read.fetch_add(1, Ordering::Relaxed);
                let Some(chunk) = buf.get(..n) else {
                    return;
                };
                // Blocks while the channel is full: back-pressure.
                if tx.send(Msg::Data(id, chunk.to_vec())).is_err() {
                    return;
                }
            }
            Err(e)
                if matches!(
                    e.kind(),
                    ErrorKind::WouldBlock | ErrorKind::TimedOut | ErrorKind::Interrupted
                ) => {}
            Err(_) => {
                let _ = tx.send(Msg::Closed(id));
                return;
            }
        }
    }
}

impl<S, C, R> Core<S, C, R>
where
    S: Storage,
    C: Clock,
    R: Randomness,
{
    fn run(mut self) {
        for note in std::mem::take(&mut self.notes) {
            self.emit(AppEvent::Note(note));
        }
        while !self.stopping() {
            self.step();
        }
        self.finish();
    }

    fn stopping(&self) -> bool {
        self.shared.stop.load(Ordering::SeqCst)
    }

    /// One pass of the loop.
    fn step(&mut self) {
        match self
            .inbound_rx
            .recv_timeout(Duration::from_millis(WAIT_MILLIS))
        {
            Ok(msg) => self.on_msg(msg),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => {
                self.shared.stop.store(true, Ordering::SeqCst);
                return;
            }
        }
        for _ in 0..BATCH {
            match self.inbound_rx.try_recv() {
                Ok(msg) => self.on_msg(msg),
                Err(TryRecvError::Empty | TryRecvError::Disconnected) => break,
            }
        }
        self.on_commands();
        if self.stopping() {
            return;
        }
        let now = self.clock.now();
        if self.last_tick != Some(now) {
            self.last_tick = Some(now);
            let out = self.manager.tick(now);
            self.handle_outputs(out, now);
        }
        self.flush_writes(now);
        self.reap();
        self.expire(now);
        self.dial_more(now);
        self.send_peers(now);
        self.persist(now);
    }

    fn on_commands(&mut self) {
        while let Ok(command) = self.commands.try_recv() {
            match command {
                Command::WatchScript(script) => {
                    if !self.manager.watch_script(script) {
                        self.emit(AppEvent::Note(
                            "too many scripts: one was not watched".to_string(),
                        ));
                    }
                }
                Command::AllBlocks(on) => self.manager.set_all_blocks(on),
                Command::FetchTransactions(on) => self.manager.set_fetch_transactions(on),
                Command::Broadcast(tx) => {
                    if let Err(e) = self.manager.broadcast(&tx) {
                        self.emit(AppEvent::Note(format!("a transaction was not held: {e}")));
                    }
                }
                Command::Forget(txid) => self.manager.forget(&txid),
                Command::Rescan(height) => {
                    self.manager.rescan_from(height);
                    // The checked height was reported above `height` already;
                    // it is reported again as it rises.
                    self.last_checked = Some(height);
                    self.emit(AppEvent::RescanStarted(height));
                }
            }
        }
    }

    fn on_msg(&mut self, msg: Msg) {
        let now = self.clock.now();
        match msg {
            Msg::Wake => {}
            Msg::Data(id, bytes) => {
                if self.conns.contains_key(&id) {
                    let out = self.manager.received(id, &bytes, now);
                    self.handle_outputs(out, now);
                }
            }
            Msg::Closed(id) => {
                if let Some(target) = self.conns.get(&id).map(|c| c.target.clone()) {
                    let out = self.manager.closed(id, now);
                    self.note_v2_refusals(&out);
                    self.remove_conn(id, now, false);
                    self.emit(AppEvent::Note(format!(
                        "peer {target} closed the connection"
                    )));
                    self.handle_outputs(out, now);
                }
            }
            Msg::Dialed(id, result) => self.on_dialed(id, result, now),
            Msg::Resolved(round, name, result) => self.on_resolved(round, &name, result, now),
            Msg::ProxyResolved(round, name, result) => {
                self.on_proxy_resolved(round, &name, result, now);
            }
        }
    }

    /// Send an event to the app. While the channel is full, or the events
    /// it holds would pass the byte budget with this one, this waits, which
    /// is the back-pressure (NR-74); it gives up when the app asks to stop.
    /// An event is always sent when the app has taken every other counted
    /// event, however large it is.
    fn emit(&self, event: AppEvent) {
        let size = payload_bytes(&event);
        let mut event = event;
        loop {
            let queued = self.queued.load(Ordering::SeqCst);
            let over = queued > 0 && queued.saturating_add(size) > self.config.event_bytes;
            if !over {
                self.queued.fetch_add(size, Ordering::SeqCst);
                match self.events.try_send(event) {
                    Ok(()) => return,
                    Err(TrySendError::Disconnected(_)) => {
                        self.shared.stop.store(true, Ordering::SeqCst);
                        return;
                    }
                    Err(TrySendError::Full(back)) => {
                        release(&self.queued, size);
                        event = back;
                    }
                }
            }
            if self.stopping() {
                return;
            }
            thread::sleep(Duration::from_millis(5));
        }
    }

    // ----- what the manager says -----------------------------------------

    fn handle_outputs(&mut self, outputs: Vec<Output>, now: u64) {
        for output in outputs {
            match output {
                Output::Disconnect {
                    id,
                    reason,
                    misbehaved,
                } => {
                    // The manager has forgotten the peer already.
                    let target = self.conns.get(&id).map(|c| c.target.clone());
                    self.remove_conn(id, now, misbehaved);
                    if let Some(target) = target {
                        let mark = if misbehaved {
                            " (not dialed for 24 h)"
                        } else {
                            ""
                        };
                        self.emit(AppEvent::Note(format!(
                            "peer {target} dropped{mark}: {reason}"
                        )));
                    }
                }
                Output::V2NotSpoken { id } => self.note_v2_refused(id),
                Output::Ready { id, services } => {
                    if let Some(conn) = self.conns.get_mut(&id) {
                        conn.ready_at = Some(now);
                        if let Some((addr, port)) = book_key(&conn.target) {
                            self.book.connected_ok(&addr, port, services, now);
                        }
                    }
                }
                Output::Addresses { from, list } => self.learn(from, &list, now),
                Output::Chain { joined, left } => self.emit(AppEvent::Chain { joined, left }),
                Output::Block { hash, bytes } => {
                    // NR-73: the headers reach the disk before the app can
                    // store anything that names this block. An app whose
                    // stored sale named a block that the stored headers do
                    // not hold would, after a crash, take the block for one
                    // that left the chain.
                    if self.manager.chain().tip().1 != self.tip_saved {
                        self.save_headers(now);
                    }
                    self.emit(AppEvent::Block { hash, bytes });
                }
                Output::Sighting { tx, from } => self.emit(AppEvent::Sighting { tx, from }),
                Output::AlsoAnnounced { txid, from } => {
                    self.emit(AppEvent::AlsoAnnounced { txid, from })
                }
                Output::TxRelayed {
                    txid,
                    peers_asked,
                    peers_fetched,
                } => self.emit(AppEvent::TxRelayed {
                    txid,
                    peers_asked,
                    peers_fetched,
                }),
                Output::TxState { txid, state } => self.emit(AppEvent::TxState { txid, state }),
            }
        }
    }

    /// NR-22: addresses a peer told of.
    fn learn(&mut self, from: openbmt_watch::NetGroup, list: &[PeerAddress], now: u64) {
        for a in list {
            self.book
                .add(&a.addr, a.port, a.services, Source::Peer(from), now);
        }
    }

    // ----- connections ---------------------------------------------------

    /// Forget a connection: close the socket, set the pause, and tell the
    /// book. The manager has been told by the caller.
    fn remove_conn(&mut self, id: PeerId, now: u64, misbehaved: bool) {
        let Some(mut conn) = self.conns.remove(&id) else {
            return;
        };
        let _ = conn.stream.shutdown(Shutdown::Both);
        if let Some(reader) = conn.reader.take() {
            self.retired.push(reader);
        }
        let v2_refused = self.v2.refused.remove(&id);
        if v2_refused {
            self.fall_back_to_v1(&conn.target, now);
        } else {
            self.pause_target(&conn.target, now);
        }
        if misbehaved {
            // NR-25.
            self.banned
                .insert(conn.target.clone(), now.saturating_add(BAN_SECS));
            if let Some((addr, port)) = book_key(&conn.target) {
                self.book.misbehaved(&addr, port, now);
            }
        } else if conn.ready_at.is_none() && !v2_refused {
            if let Some((addr, port)) = book_key(&conn.target) {
                self.book.failed(&addr, port, now);
            }
        }
    }

    /// NR-81: a peer that did not speak the encrypted transport is tried
    /// again at once with the old one, and is dialed with it for a day.
    fn fall_back_to_v1(&mut self, target: &Target, now: u64) {
        self.v2
            .v1_until
            .insert(target.clone(), now.saturating_add(V2_RETRY_SECS));
        if let Some((addr, port)) = book_key(target) {
            self.book.v2_failed(&addr, port, now);
        }
        self.initial.push_front(target.clone());
        self.emit(AppEvent::Note(format!(
            "peer {target} does not speak the encrypted transport; trying the old one"
        )));
    }

    fn note_v2_refused(&mut self, id: PeerId) {
        if self.conns.contains_key(&id) {
            self.v2.refused.insert(id);
        }
    }

    /// The `V2NotSpoken` outputs, noted before the connection is removed.
    fn note_v2_refusals(&mut self, outputs: &[Output]) {
        for output in outputs {
            if let Output::V2NotSpoken { id } = output {
                self.note_v2_refused(*id);
            }
        }
    }

    /// NR-80: which transport a new connection to this target speaks. The
    /// old one when the settings say so, when the randomness may be
    /// predictable (a key from it would be no protection), or when NR-81 or
    /// the address book says so.
    fn transport_for(&mut self, target: &Target, now: u64) -> TransportChoice {
        let remembered = self.v2.v1_until.get(target).is_some_and(|t| *t > now);
        let book_says_v1 = book_key(target).is_some_and(|(a, p)| !self.book.v2_first(&a, p, now));
        if !self.config.v2 || remembered || book_says_v1 || !self.rng.unpredictable() {
            return TransportChoice::V1;
        }
        TransportChoice::V2(V2Entropy::from_words(|| self.rng.next_u64()))
    }

    fn pause_target(&mut self, target: &Target, now: u64) {
        self.pause.insert(
            target.clone(),
            now.saturating_add(self.config.redial_pause_secs),
        );
    }

    fn on_dialed(&mut self, id: PeerId, result: Result<TcpStream, DialError>, now: u64) {
        let Some(mut dial) = self.dialing.remove(&id) else {
            return;
        };
        if let Some(handle) = dial.handle.take() {
            let _ = handle.join();
        }
        match result {
            Err(e) => {
                if onion_key(&dial.target).is_some() {
                    self.emit(AppEvent::Note(format!("onion dial failed: {e}")));
                }
                self.dial_failed(&dial.target, now);
            }
            Ok(stream) => self.adopt(id, dial.target, stream, now),
        }
    }

    /// A dial failed: pause the address and tell the book.
    fn dial_failed(&mut self, target: &Target, now: u64) {
        self.pause_target(target, now);
        if let Some((addr, port)) = book_key(target) {
            self.book.failed(&addr, port, now);
        }
    }

    /// A socket is open: give it to the manager, or close it at once.
    fn adopt(&mut self, id: PeerId, target: Target, stream: TcpStream, now: u64) {
        let nonce = self.rng.next_u64();
        let transport = self.transport_for(&target, now);
        if let Err(refused) =
            self.manager
                .connected_with(id, peer_addr(&target), nonce, now, transport)
        {
            let _ = stream.shutdown(Shutdown::Both);
            self.pause_target(&target, now);
            if refused == ConnectRefused::LocalGroup && self.noted_local.insert(target.clone()) {
                self.emit(AppEvent::Note(format!(
                    "peer {target} is on this machine and local peers are not allowed"
                )));
            }
            return;
        }
        let _ = stream.set_nodelay(true);
        let prepared = stream
            .set_write_timeout(Some(Duration::from_secs(WRITE_TIMEOUT_SECS)))
            .and_then(|()| stream.set_read_timeout(Some(Duration::from_secs(READ_TIMEOUT_SECS))))
            .and_then(|()| stream.try_clone());
        let reader_stream = match prepared {
            Ok(s) => s,
            Err(_) => return self.abandon(id, &target, stream, now),
        };
        let tx = self.inbound_tx.clone();
        let shared = self.shared.clone();
        let spawned = thread::Builder::new()
            .name("net-read".to_string())
            .spawn(move || read_loop(id, reader_stream, tx, shared));
        match spawned {
            Ok(reader) => {
                self.conns.insert(
                    id,
                    Conn {
                        stream,
                        reader: Some(reader),
                        target,
                        ready_at: None,
                    },
                );
            }
            Err(_) => self.abandon(id, &target, stream, now),
        }
    }

    /// A connection the manager took could not be set up.
    fn abandon(&mut self, id: PeerId, target: &Target, stream: TcpStream, now: u64) {
        let _ = stream.shutdown(Shutdown::Both);
        let out = self.manager.closed(id, now);
        self.dial_failed(target, now);
        self.handle_outputs(out, now);
    }

    /// Write what the manager has queued for each peer. A write that fails
    /// closes the peer.
    fn flush_writes(&mut self, now: u64) {
        loop {
            let mut failed = false;
            let ids: Vec<PeerId> = self.conns.keys().copied().collect();
            for id in ids {
                let bytes = self.manager.outgoing(id);
                if bytes.is_empty() {
                    continue;
                }
                let Some(conn) = self.conns.get_mut(&id) else {
                    continue;
                };
                if conn.stream.write_all(&bytes).is_err() {
                    failed = true;
                    let out = self.manager.closed(id, now);
                    self.note_v2_refusals(&out);
                    self.remove_conn(id, now, false);
                    self.handle_outputs(out, now);
                }
            }
            if !failed {
                break;
            }
        }
    }

    /// Join the reader threads that have ended.
    fn reap(&mut self) {
        let (done, running): (Vec<_>, Vec<_>) = std::mem::take(&mut self.retired)
            .into_iter()
            .partition(|h| h.is_finished());
        self.retired = running;
        for handle in done {
            let _ = handle.join();
        }
    }

    /// Forget pauses and bans that have run out, and a seed round that has
    /// taken too long.
    fn expire(&mut self, now: u64) {
        self.pause.retain(|_, until| *until > now);
        self.banned.retain(|_, until| *until > now);
        self.v2.v1_until.retain(|_, until| *until > now);
        let late = self
            .round
            .as_ref()
            .is_some_and(|r| now >= r.started.saturating_add(r.window));
        if late {
            self.end_round();
        }
    }

    // ----- dialing -------------------------------------------------------

    fn usable(&self, target: &Target, now: u64) -> bool {
        let waiting = |map: &BTreeMap<Target, u64>| map.get(target).is_some_and(|t| *t > now);
        !waiting(&self.pause)
            && !waiting(&self.banned)
            && !self.conns.values().any(|c| c.target == *target)
            && !self.dialing.values().any(|d| d.target == *target)
    }

    fn has_name(&self) -> bool {
        // An onion peer is a name that the book knows (NR-93): it does not
        // stand in the way of a seed name.
        let is_name = |t: &Target| matches!(t, Target::Name(..)) && onion_key(t).is_none();
        self.conns.values().any(|c| is_name(&c.target))
            || self.dialing.values().any(|d| is_name(&d.target))
    }

    /// NR-71: dial while the manager wants peers.
    fn dial_more(&mut self, now: u64) {
        loop {
            let sockets = self.conns.len().saturating_add(self.dialing.len());
            if self.dialing.len() >= MAX_DIALS_IN_FLIGHT
                || sockets >= self.config.target_peers
                || sockets >= self.config.target_peers.saturating_add(EXTRA_SOCKETS)
            {
                return;
            }
            let Some(mut wanted) = self.manager.wanted() else {
                return;
            };
            // Connections being opened will take their groups.
            for dial in self.dialing.values() {
                let addr = peer_addr(&dial.target);
                wanted.avoid.insert(if self.config.allow_local {
                    host_group(&addr)
                } else {
                    network_group(&addr)
                });
            }
            let Some(target) = self.next_target(&wanted, now) else {
                return;
            };
            self.start_dial(target, now);
        }
    }

    /// NR-71: the next address to dial. The stored anchors first (NR-24),
    /// then the fixed peers, then the book (NR-23), then the seeds.
    fn next_target(&mut self, wanted: &Wanted, now: u64) -> Option<Target> {
        while let Some(target) = self.initial.pop_front() {
            if self.usable(&target, now) {
                return Some(target);
            }
        }
        let fixed = self
            .config
            .fixed_peers
            .iter()
            .map(|a| Target::Addr(*a))
            .find(|t| self.usable(t, now));
        if fixed.is_some() {
            return fixed;
        }
        // NR-72: filters are wanted and the book has no address to dial
        // that offers them: ask the seeds too. The book is dialed meanwhile.
        let onion = self.onion_draw();
        if wanted.want_filters
            && !self
                .book
                .has_filter_candidate_with(&wanted.avoid, now, onion)
        {
            self.start_round_if_due(now);
        }
        for _ in 0..PICK_TRIES {
            let random = self.rng.next_u64();
            let Some(picked) =
                self.book
                    .pick_with(&wanted.avoid, wanted.want_filters, random, now, onion)
            else {
                break;
            };
            if let Some(target) = target_of(&picked.addr, picked.port, self.config.proxy.is_some())
            {
                if self.usable(&target, now) {
                    return Some(target);
                }
            }
        }
        self.ask_seeds(now)
    }

    /// NR-92, NR-94: how the next draw treats onion addresses. Never without
    /// a proxy, and never when the onion peers, connected and being
    /// opened, are already half of the target (at least 1); otherwise the
    /// kinds are mixed.
    fn onion_draw(&self) -> OnionDraw {
        if self.config.proxy.is_none() {
            return OnionDraw::Never;
        }
        let held = self
            .conns
            .values()
            .map(|c| &c.target)
            .chain(self.dialing.values().map(|d| &d.target))
            .filter(|t| onion_key(t).is_some())
            .count();
        if held < onion_cap(self.config.target_peers) {
            OnionDraw::Mixed
        } else {
            OnionDraw::Never
        }
    }

    /// NR-72: start a round of seed lookups (through the proxy with Tor on)
    /// if one is due and none is running.
    fn start_round_if_due(&mut self, now: u64) {
        let due = self
            .last_round
            .is_none_or(|t| now >= t.saturating_add(SEED_ROUND_SECS));
        if self.config.seeds.is_empty() || self.round.is_some() || !due {
            return;
        }
        if self.config.proxy.is_none() {
            self.start_round(now);
        } else if !self.lookup_unsupported {
            self.start_tor_round(now);
        }
    }

    /// NR-72, NR-26: the book cannot fill the target, so ask the seeds.
    fn ask_seeds(&mut self, now: u64) -> Option<Target> {
        if self.config.seeds.is_empty() {
            return None;
        }
        let port = default_port(self.config.network);
        let due = self
            .last_round
            .is_none_or(|t| now >= t.saturating_add(SEED_ROUND_SECS));
        if self.config.proxy.is_some() {
            // NR-26: the system's resolver is never called. The filter
            // names are looked up through Tor itself (NR-72).
            if !self.lookup_unsupported && self.round.is_none() && due {
                self.start_tor_round(now);
            }
            // A proxy that cannot look names up, or a round that gave
            // nothing: one seed name at a time is dialed as an ordinary
            // peer, and answers `getaddr` like any peer.
            let fallback =
                self.lookup_unsupported || (self.round.is_none() && self.last_round_empty);
            if !fallback || self.has_name() {
                return None;
            }
            let count = self.config.seeds.len();
            for step in 0..count {
                let index = self.seed_cursor.saturating_add(step).checked_rem(count)?;
                let Some(name) = self.config.seeds.get(index) else {
                    continue;
                };
                let target = Target::Name(name.clone(), port);
                if self.usable(&target, now) {
                    self.seed_cursor = index.saturating_add(1);
                    return Some(target);
                }
            }
            return None;
        }
        if self.round.is_none() && due {
            self.start_round(now);
        }
        None
    }

    fn start_dial(&mut self, target: Target, now: u64) {
        let id = PeerId(self.next_id);
        self.next_id = self.next_id.saturating_add(1);
        if onion_key(&target).is_some() {
            self.emit(AppEvent::Note("dialing an onion peer".to_string()));
        }
        let net = self.net.clone();
        let tx = self.inbound_tx.clone();
        let proxy = self.config.proxy;
        let to = target.clone();
        // NR-78: a fresh number for each connection.
        let token = self.rng.next_u64();
        let spawned = thread::Builder::new()
            .name("net-dial".to_string())
            .spawn(move || {
                let result = (net.dial)(&to, proxy, token);
                let _ = tx.send(Msg::Dialed(id, result));
            });
        match spawned {
            Ok(handle) => {
                self.dialing.insert(
                    id,
                    Dial {
                        target,
                        handle: Some(handle),
                    },
                );
            }
            Err(_) => self.dial_failed(&target, now),
        }
    }

    /// NR-72: look up every seed name, each in a thread of its own. A
    /// lookup that hangs holds nothing: its answer is ignored if it comes
    /// after the round has ended.
    fn start_round(&mut self, now: u64) {
        self.round_count = self.round_count.saturating_add(1);
        self.last_round = Some(now);
        let id = self.round_count;
        let port = default_port(self.config.network);
        let mut outstanding = 0usize;
        // Each seed is asked twice: for any node, and for nodes that state
        // the compact-filter service. A seed that does not know the second
        // question answers nothing.
        let names: Vec<String> = self
            .config
            .seeds
            .iter()
            .flat_map(|n| [n.clone(), format!("{FILTER_SEED_PREFIX}{n}")])
            .collect();
        for name in names {
            let net = self.net.clone();
            let tx = self.inbound_tx.clone();
            let asked = name.clone();
            let spawned = thread::Builder::new()
                .name("net-seed".to_string())
                .spawn(move || {
                    let result = (net.resolve)(&asked, port);
                    let _ = tx.send(Msg::Resolved(id, asked, result));
                });
            if spawned.is_ok() {
                outstanding = outstanding.saturating_add(1);
            }
        }
        self.round = Some(Round {
            id,
            started: now,
            window: RESOLVE_SECS,
            outstanding,
            queue: VecDeque::new(),
            in_flight: 0,
            usable: 0,
            failed: 0,
        });
    }

    /// NR-72, NR-26: with Tor on, ask each seed's filter name
    /// [`TOR_LOOKUPS_PER_SEED`] times through the proxy, each lookup in a
    /// thread of its own with a fresh token and so on its own circuit, at
    /// most [`MAX_TOR_LOOKUPS_IN_FLIGHT`] at once. Each lookup gives one
    /// address.
    fn start_tor_round(&mut self, now: u64) {
        self.round_count = self.round_count.saturating_add(1);
        self.last_round = Some(now);
        self.last_round_empty = false;
        let mut queue = VecDeque::new();
        for seed in &self.config.seeds {
            for _ in 0..TOR_LOOKUPS_PER_SEED {
                queue.push_back(format!("{FILTER_SEED_PREFIX}{seed}"));
            }
        }
        self.round = Some(Round {
            id: self.round_count,
            started: now,
            window: TOR_ROUND_SECS,
            outstanding: queue.len(),
            queue,
            in_flight: 0,
            usable: 0,
            failed: 0,
        });
        self.pump_round();
    }

    /// Start queued lookups while there is a free place.
    fn pump_round(&mut self) {
        let Some(proxy) = self.config.proxy else {
            return;
        };
        loop {
            let Some(round) = self.round.as_mut() else {
                return;
            };
            if round.in_flight >= MAX_TOR_LOOKUPS_IN_FLIGHT {
                return;
            }
            let Some(name) = round.queue.pop_front() else {
                break;
            };
            let id = round.id;
            // NR-78: a fresh number for each lookup.
            let token = self.rng.next_u64();
            let net = self.net.clone();
            let tx = self.inbound_tx.clone();
            let asked = name.clone();
            let spawned = thread::Builder::new()
                .name("net-seed".to_string())
                .spawn(move || {
                    let result = (net.proxy_resolve)(proxy, &asked, token);
                    let _ = tx.send(Msg::ProxyResolved(id, asked, result));
                });
            if let Some(round) = self.round.as_mut() {
                if spawned.is_ok() {
                    round.in_flight = round.in_flight.saturating_add(1);
                } else {
                    round.outstanding = round.outstanding.saturating_sub(1);
                    round.failed = round.failed.saturating_add(1);
                }
            }
        }
        if self.round.as_ref().is_some_and(|r| r.outstanding == 0) {
            self.end_round();
        }
    }

    /// The round is over, because everything came back or its time ran
    /// out. A Tor round that gave no address lets a seed name be dialed as
    /// a peer until the next round.
    fn end_round(&mut self) {
        let Some(round) = self.round.take() else {
            return;
        };
        if self.config.proxy.is_none() {
            return;
        }
        self.last_round_empty = round.usable == 0;
        // Lookups that never came back count as asked.
        let asked = round
            .usable
            .saturating_add(round.failed)
            .saturating_add(round.outstanding);
        self.emit(AppEvent::Note(format!(
            "seed round through the proxy: {} of {asked} lookups gave an address",
            round.usable
        )));
    }

    fn on_proxy_resolved(
        &mut self,
        round: u64,
        name: &str,
        result: Result<IpAddr, SocksError>,
        now: u64,
    ) {
        let port = default_port(self.config.network);
        let Some(current) = self.round.as_mut() else {
            return;
        };
        if current.id != round || now >= current.started.saturating_add(current.window) {
            return;
        }
        current.in_flight = current.in_flight.saturating_sub(1);
        current.outstanding = current.outstanding.saturating_sub(1);
        let services =
            ServiceFlags::NETWORK | ServiceFlags::WITNESS | ServiceFlags::COMPACT_FILTERS;
        let mut note = None;
        match result {
            Ok(ip) => {
                let added = self.book.add(&v2_of(ip), port, services, Source::Seed, now);
                if matches!(added, Added::Refused(_)) {
                    current.failed = current.failed.saturating_add(1);
                } else {
                    current.usable = current.usable.saturating_add(1);
                }
            }
            Err(SocksError::Refused(ReplyCode::CommandNotSupported)) => {
                current.failed = current.failed.saturating_add(1);
                // Not Tor: stop asking, for good.
                let waiting = current.queue.len();
                current.outstanding = current.outstanding.saturating_sub(waiting);
                current.queue.clear();
                if !self.lookup_unsupported {
                    self.lookup_unsupported = true;
                    note = Some(
                        "the proxy cannot look names up (not Tor); seed names are dialed as peers"
                            .to_string(),
                    );
                }
            }
            Err(e) => {
                current.failed = current.failed.saturating_add(1);
                note = Some(format!("seed lookup {name}: {e}"));
            }
        }
        if let Some(text) = note {
            self.emit(AppEvent::Note(text));
        }
        self.pump_round();
    }

    fn on_resolved(
        &mut self,
        round: u64,
        name: &str,
        result: Result<Vec<SocketAddr>, String>,
        now: u64,
    ) {
        let Some(current) = self.round.as_mut() else {
            return;
        };
        if current.id != round || now >= current.started.saturating_add(RESOLVE_SECS) {
            return;
        }
        current.outstanding = current.outstanding.saturating_sub(1);
        if current.outstanding == 0 {
            self.round = None;
        }
        let filtered = name.starts_with(FILTER_SEED_PREFIX);
        let services = if filtered {
            ServiceFlags::NETWORK | ServiceFlags::WITNESS | ServiceFlags::COMPACT_FILTERS
        } else {
            ServiceFlags::NETWORK | ServiceFlags::WITNESS
        };
        match result {
            // Not every seed knows the filtered question.
            Err(_) if filtered => {}
            Err(e) => self.emit(AppEvent::Note(format!("seed {name}: {e}"))),
            Ok(found) => {
                for addr in found {
                    self.book
                        .add(&v2_of(addr.ip()), addr.port(), services, Source::Seed, now);
                }
            }
        }
    }

    // ----- what the app sees, and what is kept --------------------------

    /// NR-75: send `Peers` when something changed, and every 10 seconds.
    fn send_peers(&mut self, now: u64) {
        let chain = self.manager.chain();
        let (height, _) = chain.tip();
        let time = chain.header_at(height).map_or(0, |h| u64::from(h.time));
        let snapshot = Snapshot {
            peers: self.manager.health_peers(),
            ahead_since: self.manager.ahead_since(),
            tip: (height, time),
        };
        let due = now >= self.last_peers_at.saturating_add(PEERS_EVENT_SECS);
        if due || self.last_peers.as_ref() != Some(&snapshot) {
            self.last_peers_at = now;
            self.emit(AppEvent::Peers(
                snapshot.peers.clone(),
                snapshot.ahead_since,
                snapshot.tip,
            ));
            self.last_peers = Some(snapshot);
        }
        let checked = self.manager.checked_height();
        if self.last_checked.is_none_or(|c| checked > c) {
            self.last_checked = Some(checked);
            self.emit(AppEvent::Checked(checked));
        }
    }

    /// NR-73: save the headers when the tip has moved and a minute has
    /// passed; set the anchors and save the book every ten minutes.
    fn persist(&mut self, now: u64) {
        let tip = self.manager.chain().tip().1;
        if tip != self.tip_saved && now >= self.last_header_save.saturating_add(HEADER_SAVE_SECS) {
            self.save_headers(now);
        }
        if now >= self.next_anchor_at {
            self.next_anchor_at = now.saturating_add(ANCHOR_SECS);
            self.set_anchors();
            self.save_book();
        }
    }

    fn save_headers(&mut self, now: u64) {
        let bytes = self.manager.chain().to_bytes();
        let tip = self.manager.chain().tip().1;
        match self.storage.save(Key::Headers, &bytes) {
            Ok(()) => {
                self.tip_saved = tip;
                self.last_header_save = now;
            }
            Err(e) => {
                // Try again at the next pass of the minute.
                self.last_header_save = now;
                self.emit(AppEvent::Note(format!("could not save the headers: {e}")));
            }
        }
    }

    fn save_book(&mut self) {
        let bytes = self.book.to_bytes();
        if let Err(e) = self.storage.save(Key::Book, &bytes) {
            self.emit(AppEvent::Note(format!(
                "could not save the address book: {e}"
            )));
        }
    }

    /// NR-24: the two ready peers connected longest become the anchors.
    fn set_anchors(&mut self) {
        let ready: Vec<(AddrV2, u16, u64)> = self
            .conns
            .values()
            .filter_map(|c| {
                let since = c.ready_at?;
                let (addr, port) = book_key(&c.target)?;
                Some((addr, port, since))
            })
            .collect();
        let anchors = longest_connected(&ready);
        self.book.set_anchors(&anchors);
    }

    // ----- stopping ------------------------------------------------------

    /// Save, close every socket and join the threads.
    fn finish(mut self) {
        let now = self.clock.now();
        self.set_anchors();
        self.save_headers(now);
        self.save_book();
        for conn in self.conns.values() {
            let _ = conn.stream.shutdown(Shutdown::Both);
        }
        let mut handles: Vec<JoinHandle<()>> = std::mem::take(&mut self.retired);
        for conn in self.conns.values_mut() {
            handles.extend(conn.reader.take());
        }
        for dial in self.dialing.values_mut() {
            handles.extend(dial.handle.take());
        }
        let events = self.events.clone();
        // Dropping the receiver ends any thread that waits to send.
        drop(self);
        let end = Instant::now() + Duration::from_secs(SHUTDOWN_SECS);
        while !handles.is_empty() && Instant::now() < end {
            let (done, running): (Vec<_>, Vec<_>) =
                handles.into_iter().partition(|h| h.is_finished());
            handles = running;
            for handle in done {
                let _ = handle.join();
            }
            if !handles.is_empty() {
                thread::sleep(Duration::from_millis(5));
            }
        }
        // A thread still in a connect is let go; it ends by itself.
        drop(handles);
        let _ = events.try_send(AppEvent::Stopped);
    }
}
