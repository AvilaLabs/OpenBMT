//! The node: one counter, one driver, one directory.
//!
//! [`Node`] is the glue between the three parts that exist apart: the
//! counter's judgment of a sale (`openbmt-counter`), the network driver that
//! owns the sockets (`openbmt-net-io`) and the files (`DirStore`). It has no
//! rule of its own. It takes the driver's events one at a time, hands each to
//! the counter with the time, the header chain and the health answer, and
//! hands back what the counter says must leave (`Report`).
//!
//! ## What the node does with each event
//!
//! - `Peers`: runs the health rules (WR-50 to WR-53) and keeps the answer
//!   for `open` and `judge`. WR-54 is applied to it after every batch of
//!   events, from the checked height and the clock.
//! - `Chain`: updates the [`ChainView`].
//! - `Sighting`: offers the transaction to the counter once for every peer
//!   address it came from. If any sale took it, the bytes are kept in a small
//!   cache (the last 256 transactions) so that a later announcement can be
//!   offered as one more sighting.
//! - `AlsoAnnounced`: one more sighting, if the cache holds the transaction.
//!   The network crate keeps no transactions (NR-54), so a transaction that
//!   no sale took is not here, and its later announcements change nothing.
//! - `Block`: parsed and handed to the counter.
//! - `Checked`: the height is written down before the next `Peers` event is
//!   looked at, and is the `watch_from` of the next start.
//! - `Note`: kept in a bounded log.
//!
//! - Own-node events (CR-103): `Has(txid)` is given to the sales that know
//!   the transaction (WR-61); a `State` is kept, the own node's health line
//!   is worked out again with the tracker (WR-63) and the sales that have a
//!   payment are judged again when it changed. The node asks the own node
//!   about the unconfirmed payments of live sales when it starts (the sales
//!   are restored then), when the own node connects and after each sighting
//!   that a sale took (WR-65).
//!
//! After the events, every live sale that an event touched is judged, and
//! what the judging reports is returned by [`Node::pump`]. Reports from the
//! `judge`, `override_sale` and the other calls of the node come back in
//! their own results; each report is returned once, by whichever call made it.
//!
//! ## Settling old sales
//!
//! The link hands the node the owner's mark ([`Node::set_owner_mark`],
//! LK-55). From then on the pump calls `Counter::settle` (CR-76 to CR-79): at
//! the first pump after the first mark of the run, then once an hour, sooner
//! while sales are left to settle, and never twice inside a minute (CR-82).
//! A settle that did something is one line in the log, with counts only.
//! The driver keeps the scripts it was given until the next start; the
//! shorter list is what it is started with then, and the owner's wallet
//! watches the rest (CR-80).
//!
//! ## The checked height
//!
//! The driver tells the node how far every block has been checked against the
//! scripts being watched. The node writes it to a small file, and at the next
//! start it is the driver's `watch_from`, so that a restart does not ask for
//! the filters and the blocks it already checked (NR-75).
//!
//! If there is no such file, the node has never run on this directory. A
//! counter that has never run has shown no address, so nothing before now
//! can pay it, and `watch_from` is the tip of the stored headers. The file is
//! written at that moment, before any address can be shown. If there is no
//! such file but the directory already holds addresses that were shown, the
//! file was lost, and the node checks again from the start block of the
//! header store: the safe side.
//!
//! The stored height is also never above the tip of the stored headers when
//! it is used: a header file that is older than the height (a crash between
//! two saves of the headers) makes the node check the blocks above its tip
//! again.
//!
//! When blocks leave the best chain, the stored height is taken down to the
//! block below the lowest of them before anything else is done with the
//! event: the blocks that replace them have not been checked yet.
//!
//! A block that could not be taken (it did not parse, or a write failed)
//! stops the stored height below that block for the rest of the run, so that
//! the next start asks for it again.
//!
//! ## The owner's heartbeat
//!
//! Accepting an unconfirmed payment at once needs the owner's signed
//! heartbeat (CR-50 to CR-56). [`Node::heartbeat`] takes its bytes, hands
//! them to the counter with the node's header chain and clock, and judges
//! again every live sale that has a payment, so that a sale that waited for
//! want of a heartbeat is accepted as soon as one is taken. `judge` and
//! `override_sale` give the instant decision the counter's own answer
//! ([`Counter::heartbeat_for_policy`]). Until a heartbeat has been taken
//! since the start, and whenever the last one does not count, that answer is
//! "not verified", and a sale waits for a confirmation however many peers
//! have seen it.
//!
//! How the bytes get here from the owner's device is not part of the node;
//! the transport comes with the Box. A heartbeat that is not verified because
//! the owner's tip and the node's differ at a height the node holds is
//! written to the node's log as a disagreement.

use std::collections::{BTreeSet, VecDeque};
use std::fmt;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

use bitcoin::consensus::deserialize;
use bitcoin::{Address, BlockHash, Network, ScriptBuf, Transaction, Txid};
use openbmt_core::batch::{AddError, PinnedRoots, SignedBatch};
use openbmt_core::command::SignedCommand;
use openbmt_core::heartbeat::SignedHeartbeat;
use openbmt_core::identity::Identity;
use openbmt_core::records::Record;
use openbmt_core::say::Words;
use openbmt_core::settings::{
    Change, SettingNote, Settings, SignedSettings, SwapProvider, LIGHTNING_NODE_BYTES,
};
use openbmt_core::wire::DecodeError;
use openbmt_counter::{
    Actor, Amount, BatchTakeRefused, BlockError, CancelRefused, CashRefund, CommandRefused,
    Counter, Currency, DayTotals, Facts, HeartbeatRefused, HeartbeatView, JudgeError,
    LightningPaid, LoadError, NoteRefused, OpenFacts, OpenRefused, Opened, Overridden, Pairing,
    Quote, Rate, RateSource, RefundRefused, RefundRequest, RefundView, Report, RequoteRefused,
    SaleExtras, Screen, SightingError, Slot, StaffEvent, StaffRefused, Store, StoreFault, SwapPaid,
    TotalsRefused, WithdrawRefused,
};
use openbmt_lightning::{CertPin, Credential, Reply};
use openbmt_net_io::{
    AppEvent, Clock, DirLock, Driver, DriverConfig, EventReceiver, FileStorage, Handle, Key,
    OsRandomness, OwnNodeEvent, Storage, SystemClock,
};
use openbmt_watch::{
    check_blocks_checked, check_health, BlockProgress, HeaderChain, HeaderStore, Health,
    HealthInput, OwnNodeHealth, OwnNodeInput, OwnNodeLink, OwnNodeTracker, PeerAddr, Source,
    WellFormedBlock,
};

use crate::chainview::ChainView;
use crate::dirstore::{DirStore, DirStoreError};
use crate::lightning::{
    Lightning, LightningStatus, LnEvent, LnRequest, LnTransport, SaleLightning, SetupRefused,
    SetupStep,
};
use crate::notices::NodeRecords;
use crate::settings::{check_not_rolled, SettingsFault, SettingsRefused};
use crate::swap::{Effect, SaleSwap, SwapDriver, SwapRequest, Update};

/// Most transactions the node keeps for later announcements.
pub const TX_CACHE: usize = 256;
/// Most lines the node's log keeps; the oldest goes first.
pub const NOTE_LOG: usize = 256;
/// CR-82: seconds between two settles.
pub const SETTLE_EVERY_SECS: u64 = 3_600;
/// CR-82: fewest seconds between two settles, however they were asked for.
pub const SETTLE_MIN_GAP_SECS: u64 = 60;
/// Most events one call of [`Node::pump`] takes, so that a driver that never
/// stops sending cannot keep the call from returning.
pub const MAX_EVENTS_PER_PUMP: usize = 4096;
/// Most own-node events one call of [`Node::pump`] takes.
pub const MAX_OWN_EVENTS_PER_PUMP: usize = 512;
/// CR-103: most transactions the node asks the own node about in one go
/// (the driver remembers 256, NR-101).
pub const MAX_OWN_ASKS: usize = 256;

/// The answer before the first `Peers` event: the check is not working.
const NO_ANSWER_YET: Health = Health {
    healthy: false,
    problem: None,
    behind: false,
};

/// Why the node did not start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartError {
    /// Another process, or another start in this one, is using the
    /// directory (NR-79).
    Locked(String),
    /// The directory could not be opened or read.
    Dir(DirStoreError),
    /// The stored headers could not be read at all.
    Headers(String),
    /// What the directory holds does not read back as a counter (CR-42).
    Counter(LoadError),
    /// The first signed batch was not taken.
    FirstBatch(String),
    /// The checked height could not be written.
    Checked(DirStoreError),
    /// The settings the counter would use do not agree with its own records
    /// (CR-163).
    Settings(SettingsFault),
    /// The driver did not start.
    Driver(String),
}

impl fmt::Display for StartError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Locked(e) => write!(f, "{e}"),
            Self::Dir(e) => write!(f, "the data directory: {e}"),
            Self::Headers(e) => write!(f, "the stored headers: {e}"),
            Self::Counter(e) => write!(f, "the stored counter: {e}"),
            Self::FirstBatch(e) => write!(f, "the first batch of addresses: {e}"),
            Self::Checked(e) => write!(f, "the checked height: {e}"),
            Self::Settings(e) => write!(f, "{e}"),
            Self::Driver(e) => write!(f, "the driver: {e}"),
        }
    }
}

impl std::error::Error for StartError {}

/// Why [`Node::heartbeat`] did not take a heartbeat.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HeartbeatNotTaken {
    /// The bytes are not a heartbeat. Nothing changed.
    Bytes(DecodeError),
    /// The counter refused it (CR-50, CR-51, CR-54). Nothing changed.
    Refused(HeartbeatRefused<DirStoreError>),
}

impl fmt::Display for HeartbeatNotTaken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Bytes(e) => write!(f, "not a heartbeat: {e}"),
            Self::Refused(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for HeartbeatNotTaken {}

/// What taking a heartbeat did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeartbeatTaken {
    /// What the counter makes of the heartbeat (CR-51 to CR-53).
    pub view: HeartbeatView,
    /// What judging the sales again reported, once.
    pub reports: Vec<Report>,
}

/// What the driver is started with, for [`Node::start_with`].
#[derive(Debug, Clone)]
pub struct Launch {
    /// The driver's settings, with `watch_from` set from the stored height.
    pub driver: DriverConfig,
    /// The directory the driver keeps its headers and address book in.
    pub dir: PathBuf,
    /// The scripts to watch from the first peer on.
    pub scripts: Vec<ScriptBuf>,
}

/// The driver as the node uses it. [`Handle`] is the real one; a test
/// supplies its own and feeds events by hand.
pub trait DriverPort {
    /// The events, in order.
    fn events(&self) -> &EventReceiver;
    /// Watch a script.
    fn watch_script(&self, script: ScriptBuf);
    /// While on, every new block is downloaded in full (NR-44).
    fn set_all_blocks(&self, on: bool);
    /// Stop.
    fn shutdown(&mut self);
    /// NR-103: what the own node says, on its own channel. `None` for a
    /// driver that has no such channel.
    fn own_node_events(&self) -> Option<&Receiver<OwnNodeEvent>> {
        None
    }
    /// NR-101: ask the own node whether it holds this transaction.
    fn ask_own_node(&self, _txid: Txid) {}
    /// NR-100: dial the own node again after it broke a rule.
    fn retry_own_node(&self) {}
    /// NR-86: hold a finished transaction (consensus bytes) and send it to
    /// the peers. The counter sends nothing else (NR-55): only the claim of
    /// a swap (CR-222).
    fn send_claim(&self, _tx: Vec<u8>) {}
}

impl DriverPort for Handle {
    fn events(&self) -> &EventReceiver {
        Handle::events(self)
    }

    fn watch_script(&self, script: ScriptBuf) {
        Handle::watch_script(self, script);
    }

    fn set_all_blocks(&self, on: bool) {
        Handle::set_all_blocks(self, on);
    }

    fn shutdown(&mut self) {
        Handle::shutdown(self);
    }

    fn own_node_events(&self) -> Option<&Receiver<OwnNodeEvent>> {
        Some(Handle::own_node_events(self))
    }

    fn ask_own_node(&self, txid: Txid) {
        Handle::ask_own_node(self, txid);
    }

    fn retry_own_node(&self) {
        Handle::retry_own_node(self);
    }

    fn send_claim(&self, tx: Vec<u8>) {
        Handle::broadcast(self, tx);
    }
}

/// CR-219: 32 bytes from the operating system, or none when it has none.
fn os_random() -> Option<[u8; 32]> {
    use std::io::Read;
    let mut file = std::fs::File::open("/dev/urandom").ok()?;
    let mut bytes = [0u8; 32];
    file.read_exact(&mut bytes).ok()?;
    Some(bytes)
}

/// The function that answers the health question (WR-50 to WR-53; the node
/// applies WR-54 to its answer). It is
/// `Send`, so that an app can keep the node behind a lock and call it from
/// more than one thread.
pub type HealthFn = Box<dyn Fn(&HealthInput<'_>) -> Health + Send>;

/// What a node is started with.
pub struct NodeConfig {
    /// The directory for everything the node keeps.
    pub dir: PathBuf,
    /// The counter's identity.
    pub identity: Identity,
    /// What the counter was paired with.
    pub pairing: Pairing,
    /// The shop's settings, already checked by the caller.
    pub settings: Settings,
    /// The first signed batch of addresses. Taken only when the directory
    /// holds no batches yet.
    pub first_batch: Option<SignedBatch>,
    /// The driver's settings. Its `watch_from` is replaced by the stored
    /// checked height.
    pub driver: DriverConfig,
    /// The node's clock. The driver has its own, the system's.
    pub clock: Box<dyn Clock + Send>,
    /// The health rules. [`check_health`] unless a check against a node with
    /// fewer peers than the rules ask for puts something else here.
    pub health: HealthFn,
    /// The saved Lightning connection, as the app sealed it (CR-115), if
    /// there is one. Not part of the settings and never sent anywhere.
    pub lightning: Option<Vec<u8>>,
    /// CR-162: the counter talks to the owner through a link, so the first
    /// start after pairing records the settings it was paired with, as the
    /// owner looks for that record. Off unless the app has a link.
    pub announce_settings: bool,
}

impl NodeConfig {
    /// A configuration with the system clock and the real health rules.
    pub fn new(
        dir: impl Into<PathBuf>,
        identity: Identity,
        pairing: Pairing,
        settings: Settings,
        driver: DriverConfig,
    ) -> Self {
        Self {
            dir: dir.into(),
            identity,
            pairing,
            settings,
            first_batch: None,
            driver,
            clock: Box::new(SystemClock),
            health: Box::new(check_health),
            lightning: None,
            announce_settings: false,
        }
    }
}

/// The counter, joined to a driver and a directory. See the module docs.
pub struct Node<P: DriverPort = Handle> {
    counter: Counter,
    port: P,
    store: DirStore,
    view: ChainView,
    clock: Box<dyn Clock + Send>,
    health_fn: HealthFn,
    /// The last answer of `health_fn`, before WR-54 is applied.
    health_base: Option<Health>,
    health: Option<Health>,
    /// WR-54: since when a header has been known above the checked height.
    blocks_behind_since: Option<u64>,
    settings: Settings,
    /// CR-160: the two roots the counter pinned at pairing.
    roots: PinnedRoots,
    /// CR-164: what the node was started with that settings can change: the
    /// Tor switch and the Lightning node.
    started_with: (
        bool,
        Option<[u8; LIGHTNING_NODE_BYTES]>,
        Option<SwapProvider>,
    ),
    /// CR-162: the settings in force have no `Setting` record yet.
    setting_unrecorded: Option<SettingNote>,
    /// CR-103: the driver was started with an own node.
    own_configured: bool,
    /// What the driver last said about the own node (NR-102), or, before it
    /// has said anything, that there has been no connection since the start.
    own_input: Option<OwnNodeInput>,
    /// WR-63: since when the own node has been one block behind.
    own_tracker: OwnNodeTracker,
    /// The health line of the own node as last worked out.
    own_health: OwnNodeHealth,
    /// The sales the node knows of, live or not.
    known: BTreeSet<u32>,
    cache: VecDeque<(Txid, Vec<u8>)>,
    notes: VecDeque<String>,
    all_blocks: bool,
    checked: u32,
    checked_cap: Option<u32>,
    stopped: bool,
    /// CR-82: when the last settle was tried, by the node's clock.
    last_settle: Option<u64>,
    /// CR-82: a settle is wanted as soon as the minute allows: the owner's
    /// first mark of this run came, or the last call left sales to settle.
    settle_wanted: bool,
    /// CR-112 to CR-116: the driver of the shop's own Lightning node.
    ln: Lightning,
    /// What a Lightning payment reported, for the next pump.
    ln_reports: Vec<Report>,
    /// CR-218 to CR-226: the driver of Lightning through a swap provider.
    swap: SwapDriver,
    /// The operating system's randomness for a swap's secrets (CR-219).
    swap_random: Box<dyn FnMut() -> Option<[u8; 32]> + Send>,
    /// The lockup scripts the driver was already asked to watch.
    swap_watching: Vec<ScriptBuf>,
    network: Network,
    /// NR-79. Last, so that it is released after the driver has stopped.
    _lock: DirLock,
}

impl Node<Handle> {
    /// Start the node on a real driver: the driver keeps its headers and its
    /// address book in the same directory.
    pub fn start(config: NodeConfig) -> Result<Self, StartError> {
        Self::start_with(config, |launch| {
            let storage = FileStorage::new(&launch.dir).map_err(|e| e.to_string())?;
            let random = OsRandomness::new().map_err(|e| e.to_string())?;
            Driver::spawn(launch.driver, storage, SystemClock, random, launch.scripts)
                .map_err(|e| e.to_string())
        })
    }
}

impl<P: DriverPort> Node<P> {
    /// Start the node on a driver that `spawn` makes. `spawn` is given what
    /// the driver must be started with.
    pub fn start_with(
        config: NodeConfig,
        spawn: impl FnOnce(Launch) -> Result<P, String>,
    ) -> Result<Self, StartError> {
        let NodeConfig {
            dir,
            identity,
            pairing,
            settings,
            first_batch,
            mut driver,
            clock,
            health,
            lightning,
            announce_settings,
        } = config;
        // NR-79: before anything in the directory is read.
        let lock = DirLock::acquire(&dir).map_err(|e| StartError::Locked(e.to_string()))?;
        let mut notes = VecDeque::new();
        let mut store = DirStore::open(&dir).map_err(StartError::Dir)?;
        let loaded = store.load().map_err(StartError::Dir)?;
        if let Some(torn) = loaded.torn {
            notes.push_back(format!(
                "the record file ended in a record that was cut short: {} bytes cut off, \
                 {} records kept",
                torn.cut_bytes, torn.kept_records
            ));
        }

        // The headers the driver will start from, read here before it is
        // spawned, so that the app's view is the driver's.
        let network = driver.network;
        let mut files = FileStorage::new(&dir).map_err(|e| StartError::Headers(e.to_string()))?;
        let stored_headers = files
            .load(Key::Headers)
            .map_err(|e| StartError::Headers(e.to_string()))?;
        let new_store =
            || HeaderStore::new(network).map_err(|e| StartError::Headers(e.to_string()));
        let headers = match stored_headers {
            None => new_store()?,
            Some(bytes) => match HeaderStore::from_bytes(network, &bytes) {
                Ok(headers) => headers,
                Err(e) => {
                    notes.push_back(format!(
                        "the stored headers do not read back ({e}); starting from the first header"
                    ));
                    new_store()?
                }
            },
        };
        let view = ChainView::from_store(&headers);

        let roots = pairing.roots;
        let mut counter =
            Counter::load(identity, pairing, settings.clone(), loaded.stored(), &store)
                .map_err(StartError::Counter)?;
        // CR-163: settings below what the counter's own records name are an
        // old copy of the file.
        check_not_rolled(settings.version, counter.settings_recorded())
            .map_err(StartError::Settings)?;
        // CR-162: the settings in force are recorded; the first start after
        // pairing records the pairing's.
        let setting_unrecorded = (announce_settings
            && counter.settings_recorded() < Some(settings.version))
        .then(|| SettingNote {
            version: settings.version,
            change: Change::Loosening,
            hash: settings.hash(),
        });
        if loaded.batches.is_none() {
            if let Some(batch) = first_batch {
                counter
                    .add_batch(batch, &mut store)
                    .map_err(|e: AddError<DirStoreError>| StartError::FirstBatch(e.to_string()))?;
            }
        }
        let mut known: BTreeSet<u32> = loaded.sales.iter().map(|(i, _)| *i).collect();
        known.extend(counter.unreadable());

        // Where the driver starts checking blocks.
        let tip = headers.tip_height();
        let shown_before = !counter.watched_scripts().is_empty() || !known.is_empty();
        let watch_from = match loaded.checked {
            Some(checked) => checked.min(tip),
            None if shown_before => headers.start_height(),
            None => tip,
        };
        if loaded.checked.is_none() {
            store.put_checked(watch_from).map_err(StartError::Checked)?;
        }
        driver.watch_from = watch_from;
        let own_configured = driver.own_node.is_some();
        let started_at = clock.now();
        let counter_id: [u8; 4] = counter
            .device_id()
            .to_bytes()
            .first_chunk::<4>()
            .copied()
            .unwrap_or_default();
        let ln = Lightning::new(
            driver.network,
            counter_id,
            settings.lightning_node,
            lightning.as_deref(),
        );

        // CR-220: the swaps that were saved, with the sale scripts they claim
        // to.
        let swap = SwapDriver::new(
            driver.network,
            settings.swap.as_ref(),
            &loaded.swaps,
            |index| counter.sale(index).map(|s| s.script().clone()),
        );
        let mut scripts = counter.watched_scripts();
        let launch_swap_scripts = swap.watched_scripts();
        scripts.extend(launch_swap_scripts.iter().cloned());
        let network = driver.network;
        let port = spawn(Launch {
            driver,
            dir,
            scripts,
        })
        .map_err(StartError::Driver)?;
        let mut node = Self {
            counter,
            port,
            store,
            view,
            clock,
            health_fn: health,
            health_base: None,
            health: None,
            blocks_behind_since: None,
            started_with: (
                settings.tor_on,
                settings.lightning_node,
                settings.swap.clone(),
            ),
            settings,
            roots,
            setting_unrecorded,
            own_configured,
            own_input: own_configured.then_some(OwnNodeInput {
                link: OwnNodeLink::Down { since: started_at },
                best_height: None,
                last_heard: None,
            }),
            own_tracker: OwnNodeTracker::new(),
            own_health: OwnNodeHealth::not_configured(),
            known,
            cache: VecDeque::new(),
            notes,
            all_blocks: false,
            checked: loaded.checked.unwrap_or(watch_from),
            checked_cap: None,
            stopped: false,
            last_settle: None,
            settle_wanted: false,
            ln,
            ln_reports: Vec::new(),
            swap,
            swap_random: Box::new(os_random),
            swap_watching: launch_swap_scripts,
            network,
            _lock: lock,
        };
        // CR-112: a sale that was open at the last stop asks for an invoice
        // again, and the invoices of the quotes of sales open or lately over
        // are asked for by their labels: Core Lightning answers with the
        // ones it made.
        for index in node.known.iter().copied().collect::<Vec<_>>() {
            let Some(s) = node.counter.sale(index) else {
                continue;
            };
            let quotes: Vec<(u64, u64)> = s
                .quotes()
                .iter()
                .map(|q| {
                    (
                        openbmt_counter::sats_for(s.amount(), q.rate).unwrap_or(0),
                        q.from,
                    )
                })
                .collect();
            let open = s.is_live();
            // CR-208: a request is never asked of the Lightning node.
            if s.is_request() {
                continue;
            }
            node.ln.sale_restored(index, &quotes, open, started_at);
        }
        node.take_ln_notes();
        node.restore_swaps(started_at);
        node.update_all_blocks();
        node.retry_setting_record();
        // WR-65: the sales are restored, so the own node is asked about
        // their unconfirmed payments.
        node.own_health_now();
        node.ask_own_node_about_sales();
        Ok(node)
    }

    // What the node knows.

    /// The counter, for reading (the alarm, the exposure, the sales).
    pub fn counter(&self) -> &Counter {
        &self.counter
    }

    /// CR-81: how many records the counter has written.
    pub fn records_len(&self) -> u64 {
        self.counter.records_len()
    }

    /// CR-81: the hash of the counter's newest record, or `GENESIS`.
    pub fn records_head(&self) -> [u8; 32] {
        self.counter.head()
    }

    /// CR-81: the record numbered `n`, from memory or the record file.
    pub fn record_at(&self, n: u64) -> Result<Option<Record>, StoreFault<DirStoreError>> {
        self.counter.record_at(n, &self.store)
    }

    /// CR-81: up to `max` records from number `from` on, oldest first.
    pub fn records_from(
        &self,
        from: u64,
        max: usize,
    ) -> Result<Vec<Record>, StoreFault<DirStoreError>> {
        self.counter.records_from(from, max, &self.store)
    }

    /// LK-55: the hash of the record before number `n` (see
    /// `Counter::hash_at`).
    pub fn hash_at(&self, n: u64) -> Result<Option<[u8; 32]>, StoreFault<DirStoreError>> {
        self.counter.hash_at(n, &self.store)
    }

    /// The counter's records as a source that can be read in order, for the
    /// notice outbox.
    pub fn record_source(&self) -> NodeRecords<'_> {
        NodeRecords {
            counter: &self.counter,
            store: &self.store,
        }
    }

    /// The best header chain as the node knows it.
    pub fn chain(&self) -> &ChainView {
        &self.view
    }

    /// The last answer of the health rules, if a `Peers` event has come.
    pub fn health(&self) -> Option<Health> {
        self.health
    }

    /// CR-107: the health line of the own node, worked out now (WR-63).
    /// `OwnNodeHealth::not_configured()` when the driver has no own node.
    pub fn own_node_health(&mut self) -> OwnNodeHealth {
        self.own_health_now()
    }

    /// CR-103: whether the driver was started with an own node.
    pub fn own_node_configured(&self) -> bool {
        self.own_configured
    }

    /// CR-107: dial the own node again after it was dropped for breaking a
    /// rule of the peer protocol (NR-100). Without effect when it was not.
    pub fn retry_own_node(&mut self) {
        if self.own_configured {
            // WR-63: the way to the node is built again, so "connecting"
            // holds for another 120 seconds if it is not connected now.
            if self
                .own_input
                .is_none_or(|i| i.link != OwnNodeLink::Connected)
            {
                self.own_tracker.retry(self.clock.now());
            }
            self.port.retry_own_node();
        }
    }

    /// The height up to which every block has been checked and written down.
    pub fn checked_height(&self) -> u32 {
        self.checked
    }

    /// The indexes of the sales the node knows of, lowest first.
    pub fn sales(&self) -> impl Iterator<Item = u32> + '_ {
        self.known.iter().copied()
    }

    /// The lines the node and the driver have logged, oldest first.
    pub fn notes(&self) -> impl Iterator<Item = &str> {
        self.notes.iter().map(String::as_str)
    }

    /// Take the logged lines.
    pub fn take_notes(&mut self) -> Vec<String> {
        self.notes.drain(..).collect()
    }

    /// How many transactions the node keeps for later announcements.
    pub fn cached_transactions(&self) -> usize {
        self.cache.len()
    }

    /// Whether the node keeps this transaction for later announcements.
    pub fn is_cached(&self, txid: &Txid) -> bool {
        self.cache.iter().any(|(id, _)| id == txid)
    }

    /// Whether the driver has stopped.
    pub fn stopped(&self) -> bool {
        self.stopped
    }

    fn take_ln_notes(&mut self) {
        for line in self.ln.take_notes() {
            self.note(line);
        }
    }

    fn note(&mut self, line: String) {
        if self.notes.len() >= NOTE_LOG {
            self.notes.pop_front();
        }
        self.notes.push_back(line);
    }

    // CR-160 to CR-166: the owner's settings.

    /// The settings in force.
    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    /// CR-164: whether the settings in force change what the node was
    /// started with (the Tor switch or the Lightning node), so that the app
    /// must start the node again before they take effect.
    pub fn restart_needed(&self) -> bool {
        (
            self.settings.tor_on,
            self.settings.lightning_node,
            self.settings.swap.clone(),
        ) != self.started_with
    }

    /// CR-165: whether the settings in force no longer list this counter.
    pub fn removed(&self) -> bool {
        !self.counter.is_listed()
    }

    /// CR-160 to CR-162: take signed settings the owner sent. Older or equal
    /// versions are dropped (`Ok(None)`); the owner sends again until it sees
    /// the counter's record. Newer ones are checked against the pinned roots
    /// and the settings in force, written to the settings file and only then
    /// used, and then recorded. On an error nothing changed.
    pub fn take_settings(
        &mut self,
        signed: &SignedSettings,
    ) -> Result<Option<Change>, SettingsRefused> {
        if signed.settings.version <= self.settings.version {
            return Ok(None);
        }
        let change = signed
            .verify(&self.roots, Some(&self.settings))
            .map_err(SettingsRefused::Verify)?;
        self.store
            .put_settings(&signed.to_bytes())
            .map_err(SettingsRefused::Store)?;
        let before = self.restart_needed();
        self.counter.set_settings(signed.settings.clone());
        self.settings = signed.settings.clone();
        self.setting_unrecorded = Some(SettingNote {
            version: signed.settings.version,
            change,
            hash: signed.settings.hash(),
        });
        self.note(format!(
            "CR-160: settings version {} taken ({})",
            signed.settings.version,
            match change {
                Change::Tightening => "a tightening",
                Change::Loosening => "a loosening",
            }
        ));
        if self.removed() {
            self.note("CR-165: the settings no longer list this counter".to_string());
        }
        if !before && self.restart_needed() {
            self.note("CR-164: the settings change Tor or Lightning: restart needed".to_string());
        }
        self.retry_setting_record();
        Ok(Some(change))
    }

    /// CR-162: write the `Setting` record the settings in force are waiting
    /// for, if one is. A failure is a note, once, and the record is tried
    /// again at the next call.
    pub fn retry_setting_record(&mut self) {
        let Some(note) = self.setting_unrecorded else {
            return;
        };
        match self.counter.record_setting(note, &mut self.store) {
            Ok(_) => self.setting_unrecorded = None,
            Err(e) => {
                let line = format!(
                    "CR-162: the record of settings version {} was not written: {e}",
                    note.version
                );
                if self.notes.back() != Some(&line) {
                    self.note(line);
                }
            }
        }
    }

    // What arrives.

    /// Take the driver's events for up to `wait`, apply each, then judge
    /// every live sale that an event touched. Returns what must leave the
    /// counter. With a `wait` of zero it takes what is already waiting.
    pub fn pump(&mut self, wait: Duration) -> Vec<Report> {
        let deadline = Instant::now() + wait;
        let mut reports = Vec::new();
        let mut touched = BTreeSet::new();
        for _ in 0..MAX_EVENTS_PER_PUMP {
            let Some(event) = self.next_event(deadline) else {
                break;
            };
            self.apply(event, &mut touched, &mut reports);
        }
        self.take_own_events(&mut touched);
        self.swap_step(&mut reports);
        // WR-54 reads the checked height and the clock, which move without
        // a `Peers` event.
        self.refresh_health(&mut touched);
        // Whether a block must be downloaded in full depends on the sales'
        // payments and the chain, and those change only with an event.
        let changed = !touched.is_empty();
        for index in touched {
            let live = self.counter.sale(index).is_some_and(|s| s.is_live());
            if !live {
                continue;
            }
            match self.judge(index) {
                Ok(screen) => reports.extend(screen.reports),
                Err(e) => self.note(format!("sale {index} could not be judged: {e}")),
            }
        }
        if changed {
            self.update_all_blocks();
        }
        self.settle_when_due();
        self.retry_setting_record();
        reports.append(&mut self.ln_reports);
        self.take_ln_notes();
        reports
    }

    /// CR-76: the owner's mark, as the link took it (LK-55). The first one of
    /// this run asks for a settle at the next pump.
    pub fn set_owner_mark(&mut self, records_held: u64) {
        if self.counter.owner_mark().is_none() {
            self.settle_wanted = true;
        }
        self.counter.set_owner_mark(records_held);
    }

    /// CR-82: settle once the owner's mark is known, at the first pump after
    /// it, then once an hour (sooner while sales are left to settle), and
    /// never twice inside a minute. What it did is a line in the log: the
    /// counts only. The driver keeps the scripts it was given until the next
    /// start; the shorter list is what it is then started with.
    fn settle_when_due(&mut self) {
        if self.counter.owner_mark().is_none() {
            return;
        }
        let now = self.clock.now();
        let gap = |secs: u64| {
            self.last_settle
                .is_none_or(|at| now >= at.saturating_add(secs))
        };
        if !gap(SETTLE_MIN_GAP_SECS) || !(self.settle_wanted || gap(SETTLE_EVERY_SECS)) {
            return;
        }
        self.last_settle = Some(now);
        self.settle_wanted = false;
        match self.counter.settle(&mut self.store, now) {
            Ok(done) => {
                self.settle_wanted = done.remaining;
                if done.count > 0 || done.released > 0 {
                    self.note(format!(
                        "CR-77: settled {} sales and released {} addresses",
                        done.count, done.released
                    ));
                }
            }
            Err(e) => self.note(format!("CR-77: settling stopped: {e}")),
        }
    }

    fn next_event(&mut self, deadline: Instant) -> Option<AppEvent> {
        let remaining = deadline.saturating_duration_since(Instant::now());
        let got = if remaining.is_zero() {
            self.port
                .events()
                .try_recv()
                .map_err(|e| matches!(e, std::sync::mpsc::TryRecvError::Disconnected))
        } else {
            self.port
                .events()
                .recv_timeout(remaining)
                .map_err(|e| e == RecvTimeoutError::Disconnected)
        };
        match got {
            Ok(event) => Some(event),
            Err(disconnected) => {
                if disconnected {
                    self.stopped = true;
                }
                None
            }
        }
    }

    fn apply(&mut self, event: AppEvent, touched: &mut BTreeSet<u32>, reports: &mut Vec<Report>) {
        match event {
            AppEvent::Peers(peers, ahead_since, (tip_height, tip_time)) => {
                self.view.set_tip_time(tip_height, tip_time);
                let input = HealthInput {
                    peers: &peers,
                    tip_height,
                    tip_time,
                    now: self.clock.now(),
                    behind_since: ahead_since,
                };
                self.health_base = Some((self.health_fn)(&input));
                self.refresh_health(touched);
            }
            AppEvent::Chain { joined, left } => {
                // The blocks that take the place of the ones that left have
                // not been checked. The driver tells of its checked height
                // only when it rises, so the stored one is taken down here.
                let fork = left
                    .iter()
                    .filter_map(|hash| self.view.height_of(hash))
                    .min()
                    .map(|lowest| lowest.saturating_sub(1));
                self.view.apply(&joined, &left);
                if let Some(fork) = fork {
                    self.lower_checked(fork);
                }
                touched.extend(self.sales_with_payments());
            }
            AppEvent::Sighting { tx, from } => {
                let swap_took = self.swap_sighting(&tx, &from);
                if self.offer(&tx, &from, touched, reports) || swap_took {
                    self.remember(&tx);
                    self.ask_own_node_about(&tx);
                }
            }
            AppEvent::AlsoAnnounced { txid, from } => {
                let kept = self
                    .cache
                    .iter()
                    .find(|(id, _)| *id == txid)
                    .map(|(_, bytes)| bytes.clone());
                if let Some(tx) = kept {
                    self.offer(&tx, &[from], touched, reports);
                }
            }
            AppEvent::Block { hash, bytes } => self.take_block(hash, &bytes, touched, reports),
            AppEvent::Checked(height) => self.write_checked(height),
            AppEvent::RescanStarted(_) => {}
            AppEvent::Note(line) => self.note(line),
            AppEvent::Stopped => self.stopped = true,
            // The counter never broadcasts (NR-55), and never asks for a
            // block by its hash (NR-47).
            AppEvent::TxRelayed { .. } | AppEvent::TxState { .. } | AppEvent::FeeBlock { .. } => {}
        }
    }

    // CR-103: the own node.

    /// WR-54: the answer of the health rules with the block frontier taken
    /// in. A sale that waits on the check can turn red, or green again,
    /// when the answer changes.
    fn refresh_health(&mut self, touched: &mut BTreeSet<u32>) {
        let Some(base) = self.health_base else {
            return;
        };
        let now = self.clock.now();
        let tip_height = self.view.tip_height();
        if tip_height > self.checked {
            self.blocks_behind_since.get_or_insert(now);
        } else {
            self.blocks_behind_since = None;
        }
        let answer = check_blocks_checked(
            base,
            &BlockProgress {
                tip_height,
                checked_height: self.checked,
                behind_since: self.blocks_behind_since,
                now,
            },
        );
        if self.health != Some(answer) {
            touched.extend(self.sales_with_payments());
        }
        self.health = Some(answer);
    }

    /// WR-63: the own node's health line now, with the counter's tip and
    /// the node's clock.
    fn own_health_now(&mut self) -> OwnNodeHealth {
        if !self.own_configured {
            return OwnNodeHealth::not_configured();
        }
        let now = self.clock.now();
        let tip = self.view.tip_height();
        let health = self.own_tracker.check(self.own_input.as_ref(), tip, now);
        self.own_health = health;
        health
    }

    fn take_own_events(&mut self, touched: &mut BTreeSet<u32>) {
        let mut events = Vec::new();
        if let Some(rx) = self.port.own_node_events() {
            while events.len() < MAX_OWN_EVENTS_PER_PUMP {
                match rx.try_recv() {
                    Ok(event) => events.push(event),
                    Err(_) => break,
                }
            }
        }
        for event in events {
            match event {
                // WR-61: a report for a transaction no sale knows records
                // nothing; the node is asked again after the light client's
                // sighting.
                OwnNodeEvent::Has(txid) => touched.extend(self.counter.own_node_saw(txid)),
                OwnNodeEvent::State(input) => self.own_state(input, touched),
            }
        }
    }

    fn own_state(&mut self, input: OwnNodeInput, touched: &mut BTreeSet<u32>) {
        let was_connected = self
            .own_input
            .is_some_and(|i| i.link == OwnNodeLink::Connected);
        let before = self.own_health;
        self.own_input = Some(input);
        let after = self.own_health_now();
        if input.link == OwnNodeLink::Connected && !was_connected {
            self.ask_own_node_about_sales();
        }
        if before.usable != after.usable || before.problem != after.problem {
            touched.extend(self.sales_with_payments());
        }
    }

    /// WR-65: ask the own node about every unconfirmed payment of the live
    /// sales, at most [`MAX_OWN_ASKS`], the newest sales first.
    fn ask_own_node_about_sales(&mut self) {
        if !self.own_configured {
            return;
        }
        let depth = u32::from(self.settings.min_confirmations).max(1);
        let now = self.clock.now();
        let mut asked = 0;
        for index in self.known.iter().rev() {
            let Some(sale) = self.counter.sale(*index).filter(|s| s.is_live()) else {
                continue;
            };
            let mut watch = sale.watch().clone();
            if watch.status(&self.view, now).min_confirmations >= depth {
                continue;
            }
            for tx in sale.watch().transactions() {
                if asked >= MAX_OWN_ASKS {
                    return;
                }
                self.port.ask_own_node(tx.compute_txid());
                asked += 1;
            }
        }
    }

    /// WR-65: ask the own node about a transaction a sale has just taken
    /// from the light client.
    fn ask_own_node_about(&self, tx: &[u8]) {
        if !self.own_configured {
            return;
        }
        if let Ok(parsed) = deserialize::<Transaction>(tx) {
            self.port.ask_own_node(parsed.compute_txid());
        }
    }

    /// Offer a transaction to the counter once for each address. True if any
    /// sale did not ignore it.
    fn offer(
        &mut self,
        tx: &[u8],
        from: &[PeerAddr],
        touched: &mut BTreeSet<u32>,
        reports: &mut Vec<Report>,
    ) -> bool {
        let now = self.clock.now();
        let mut taken = false;
        for addr in from {
            match self
                .counter
                .sighting(tx, Source::Peer(*addr), now, &mut self.store)
            {
                Ok(done) => {
                    for (index, result) in done.results {
                        taken = true;
                        touched.insert(index);
                        if let Err(refusal) = result {
                            self.note(format!("sale {index} refused a transaction: {refusal}"));
                        }
                    }
                    reports.extend(done.reports);
                }
                Err(e @ SightingError::Refused(_)) => {
                    self.note(format!("a transaction was not taken: {e}"));
                }
                Err(e) => self.note(format!("a transaction was not stored: {e}")),
            }
        }
        taken
    }

    fn remember(&mut self, tx: &[u8]) {
        let Ok(parsed) = deserialize::<Transaction>(tx) else {
            return;
        };
        let txid = parsed.compute_txid();
        if self.cache.iter().any(|(id, _)| *id == txid) {
            return;
        }
        if self.cache.len() >= TX_CACHE {
            self.cache.pop_front();
        }
        self.cache.push_back((txid, tx.to_vec()));
    }

    fn take_block(
        &mut self,
        hash: BlockHash,
        bytes: &[u8],
        touched: &mut BTreeSet<u32>,
        reports: &mut Vec<Report>,
    ) {
        let now = self.clock.now();
        let block = match WellFormedBlock::parse(bytes) {
            Ok(block) if block.hash() == hash => block,
            Ok(_) => {
                self.note(format!("block {hash}: the bytes are another block"));
                self.hold_checked(&hash);
                return;
            }
            Err(e) => {
                self.note(format!("block {hash} was refused: {e}"));
                self.hold_checked(&hash);
                return;
            }
        };
        self.swap.block(&block, &self.view, now);
        match self.counter.block(&block, &self.view, now, &mut self.store) {
            Ok(taken) => {
                for (index, result) in taken.results {
                    touched.insert(index);
                    if let Err(refusal) = result {
                        self.note(format!(
                            "sale {index}: block {hash} was refused for it: {refusal}"
                        ));
                    }
                }
                reports.extend(taken.reports);
            }
            Err(e @ BlockError::Refused(_)) => {
                self.note(format!("block {hash} was not taken: {e}"));
                self.hold_checked(&hash);
            }
            Err(e) => {
                self.note(format!("block {hash} was not stored: {e}"));
                self.hold_checked(&hash);
            }
        }
    }

    /// A block was not taken: the stored height must not pass it, so that the
    /// next start asks for it again.
    fn hold_checked(&mut self, hash: &BlockHash) {
        let cap = self
            .view
            .height_of(hash)
            .map_or(self.checked, |h| h.saturating_sub(1));
        self.checked_cap = Some(self.checked_cap.map_or(cap, |c| c.min(cap)));
    }

    /// Blocks above `height` left the chain: what is stored must not say
    /// that their heights were checked. If the write fails, the stored
    /// height is held where it is for the rest of the run and the failure
    /// is logged; the blocks of the new branch are still checked now, in
    /// this run.
    fn lower_checked(&mut self, height: u32) {
        if height >= self.checked {
            return;
        }
        match self.store.put_checked(height) {
            Ok(()) => self.checked = height,
            Err(e) => self.note(format!(
                "the checked height was not taken down to {height} after a \
                 reorganization: {e}"
            )),
        }
    }

    fn write_checked(&mut self, height: u32) {
        let height = self.checked_cap.map_or(height, |cap| height.min(cap));
        if height <= self.checked {
            return;
        }
        match self.store.put_checked(height) {
            Ok(()) => self.checked = height,
            Err(e) => self.note(format!("the checked height {height} was not stored: {e}")),
        }
    }

    // NR-44.

    fn sales_with_payments(&self) -> Vec<u32> {
        self.known
            .iter()
            .copied()
            .filter(|index| {
                self.counter
                    .sale(*index)
                    .is_some_and(|s| s.is_live() && s.watch().transactions().next().is_some())
            })
            .collect()
    }

    /// NR-44: whether any live sale has something received and is not yet
    /// confirmed at the shop's depth.
    fn wants_all_blocks(&self) -> bool {
        let depth = u32::from(self.settings.min_confirmations).max(1);
        let now = self.clock.now();
        self.known
            .iter()
            .filter_map(|index| self.counter.sale(*index))
            .filter(|sale| sale.is_live())
            .any(|sale| {
                let mut watch = sale.watch().clone();
                let status = watch.status(&self.view, now);
                status.received_sats > 0 && status.min_confirmations < depth
            })
    }

    fn update_all_blocks(&mut self) {
        let want = self.wants_all_blocks();
        if want != self.all_blocks {
            self.port.set_all_blocks(want);
            self.all_blocks = want;
        }
    }

    // CR-218 to CR-226: Lightning through a swap provider.

    /// CR-220: after a start, every open sale that has no swap alive asks
    /// for one, and the sales whose swap is on the way do not expire.
    fn restore_swaps(&mut self, now: u64) {
        let sales: Vec<(u32, ScriptBuf, u64, bool)> = self
            .known
            .iter()
            .filter_map(|i| self.counter.sale(*i))
            .filter(|s| !s.is_request())
            .map(|s| (s.index(), s.script().clone(), s.needs_sats(), s.is_live()))
            .collect();
        for (index, script, sats, open) in sales {
            if let Ok(address) = Address::from_script(&script, self.network) {
                self.swap.sale_restored(index, address, sats, open, now);
            }
            if self.swap.sale_pending(index) {
                self.counter.set_swap_pending(index, true);
            }
        }
        self.swap_flush();
    }

    /// The lockup of a swap may be among the transactions the peers announce.
    fn swap_sighting(&mut self, tx: &[u8], from: &[PeerAddr]) -> bool {
        let now = self.clock.now();
        let mut taken = false;
        for addr in from {
            taken |= self.swap.sighting(tx, Source::Peer(*addr), now);
        }
        taken
    }

    /// CR-220: write what the driver asks to be written and, once a record
    /// that holds a claim is written, send the claim to the peers. A record
    /// that was not written is asked for again at the next call.
    fn swap_flush(&mut self) {
        // The lockup address of a swap is watched from the moment the swap
        // is made (CR-220).
        for script in self.swap.watched_scripts() {
            if !self.swap_watching.contains(&script) {
                self.port.watch_script(script.clone());
                self.swap_watching.push(script);
            }
        }
        for effect in self.swap.flush() {
            let Effect::Save {
                index,
                n,
                bytes,
                send,
            } = effect;
            match self.store.put(Slot::Swap(index, n), &bytes) {
                Ok(()) => {
                    self.swap.saved(index, n, &bytes);
                    if let Some(tx) = send {
                        self.port.send_claim(tx);
                    }
                }
                Err(e) => self.note(format!(
                    "CR-220: the record of swap {index}-{n} was not written: {e}"
                )),
            }
        }
        for line in self.swap.take_notes() {
            self.note(line);
        }
    }

    /// CR-219 to CR-223: what the chain and the provider showed is given to
    /// the counter.
    fn swap_step(&mut self, reports: &mut Vec<Report>) {
        if !self.swap_active() {
            return;
        }
        let now = self.clock.now();
        let facts = Facts {
            chain: &self.view,
            health: self.health.unwrap_or(NO_ANSWER_YET),
            heartbeat: self.counter.heartbeat_for_policy(now),
            remote_order: false,
            now,
        };
        let counter = &self.counter;
        let updates = self.swap.step(now, &self.view, &mut |index, sats, status| {
            counter.swap_decide(index, sats, status, &facts).is_some()
        });
        // The records first: a claim is sent only after its record is
        // written (CR-220).
        self.swap_flush();
        for update in updates {
            match update {
                Update::Claimed {
                    index,
                    n,
                    payment_hash,
                    invoice_sats,
                    received_sats,
                    lockup_confirmed,
                    status,
                } => {
                    let facts = Facts {
                        chain: &self.view,
                        health: self.health.unwrap_or(NO_ANSWER_YET),
                        heartbeat: self.counter.heartbeat_for_policy(now),
                        remote_order: false,
                        now,
                    };
                    let instant = if lockup_confirmed {
                        None
                    } else {
                        self.counter
                            .swap_decide(index, invoice_sats, &status, &facts)
                    };
                    if !lockup_confirmed && instant.is_none() {
                        // The claim is sent; the sale is paid when the
                        // lockup is in a block.
                        continue;
                    }
                    let paid = SwapPaid {
                        payment_hash,
                        invoice_sats,
                        received_sats,
                    };
                    match self
                        .counter
                        .swap_paid(index, paid, instant, &facts, &mut self.store)
                    {
                        Ok(Some(screen)) => {
                            reports.extend(screen.reports);
                            self.swap.told(index, n, lockup_confirmed);
                        }
                        Ok(None) => {}
                        Err(openbmt_counter::LightningRefused::Store(e)) => self.note(format!(
                            "sale {index}: a swap payment was not recorded: {e}"
                        )),
                        Err(e) => {
                            // The sale is over or was paid another way: the
                            // claim pays the shop's address and the counter
                            // shows it as a late payment (CR-19).
                            self.note(format!("sale {index}: a swap payment was not taken: {e}"));
                            self.swap.told(index, n, lockup_confirmed);
                        }
                    }
                }
                Update::LockupConfirmed { index } => {
                    if let Err(e) = self.counter.swap_lockup_confirmed(index, &mut self.store) {
                        self.note(format!("sale {index}: the exposure was not released: {e}"));
                    }
                }
                Update::Cancel { index } => {
                    match self.counter.swap_cancelled(index, now, &mut self.store) {
                        Ok(r) => reports.extend(r),
                        Err(e) => {
                            self.note(format!("sale {index}: the cancel was not stored: {e}"))
                        }
                    }
                }
                Update::Pending { index, pending } => {
                    self.counter.set_swap_pending(index, pending);
                }
                Update::Failed { index, n, failure } => {
                    self.note(format!("CR-223: swap {index}-{n} failed: {failure:?}"));
                }
                Update::Stuck { index } => {
                    self.note(format!(
                        "CR-222: the claim of sale {index} is not in a block and the deadline is near"
                    ));
                }
            }
        }
        self.swap_flush();
    }

    /// Whether any part of the swap driver has work: a provider is set, or
    /// a swap that was saved is not over.
    fn swap_active(&self) -> bool {
        self.swap.is_on() || !self.swap.watched_scripts().is_empty()
    }

    /// CR-219, CR-225: the next request for the provider, as the bytes of an
    /// HTTP/1.1 request for its onion name, or none.
    pub fn swap_next(&mut self) -> Option<SwapRequest> {
        let now = self.clock.now();
        let tip = self.view.tip_height();
        let request = self.swap.next(now, tip, &mut *self.swap_random);
        self.swap_flush();
        request
    }

    /// CR-225: the bytes that came back for the request with this number.
    pub fn swap_reply(&mut self, id: u64, bytes: &[u8]) {
        let now = self.clock.now();
        let tip = self.view.tip_height();
        self.swap.reply(id, bytes, now, tip);
        self.swap_flush();
    }

    /// CR-225: the request with this number got no answer.
    pub fn swap_failed(&mut self, id: u64) {
        let now = self.clock.now();
        self.swap.failed(id, now);
        self.swap_flush();
    }

    /// CR-222: what the owner is told when a claim is stuck, if one is.
    pub fn swap_status(&self) -> Option<Words> {
        self.swap.any_stuck().then(crate::swap::claim_stuck_words)
    }

    /// CR-219: whether the swap provider of the settings is in use.
    pub fn swap_on(&self) -> bool {
        self.swap.is_on()
    }

    /// The source of the randomness for a swap's secrets: the operating
    /// system's unless a test puts another.
    pub fn set_swap_random(&mut self, random: Box<dyn FnMut() -> Option<[u8; 32]> + Send>) {
        self.swap_random = random;
    }

    // What the cashier asks.

    /// Open a sale (CR-01 to CR-05). The check must be healthy: before the
    /// first `Peers` event it is not. The address is given to the driver to
    /// watch before it is returned.
    pub fn open(
        &mut self,
        amount: Amount,
        currency: Currency,
        rate: Rate,
        source: &RateSource,
    ) -> Result<Opened, OpenRefused<DirStoreError>> {
        self.open_by(amount, currency, rate, source, None)
    }

    /// [`Node::open`], naming the person at the till who opens the sale
    /// (CR-151).
    pub fn open_by(
        &mut self,
        amount: Amount,
        currency: Currency,
        rate: Rate,
        source: &RateSource,
        by: Option<&Actor>,
    ) -> Result<Opened, OpenRefused<DirStoreError>> {
        self.open_with_tip(amount, Amount(0), currency, rate, source, by)
    }

    /// [`Node::open_by`] for a sale with a tip (CR-189, CR-190): the code is
    /// for the amount and the tip together.
    pub fn open_with_tip(
        &mut self,
        amount: Amount,
        tip: Amount,
        currency: Currency,
        rate: Rate,
        source: &RateSource,
        by: Option<&Actor>,
    ) -> Result<Opened, OpenRefused<DirStoreError>> {
        self.open_with_extras(
            amount,
            tip,
            currency,
            rate,
            source,
            by,
            SaleExtras::default(),
        )
    }

    /// [`Node::open_with_tip`] for a sale with a note (CR-206) or a request
    /// to send (CR-205). A request is not asked of the shop's Lightning node
    /// (CR-208).
    #[allow(clippy::too_many_arguments)]
    pub fn open_with_extras(
        &mut self,
        amount: Amount,
        tip: Amount,
        currency: Currency,
        rate: Rate,
        source: &RateSource,
        by: Option<&Actor>,
        extras: SaleExtras<'_>,
    ) -> Result<Opened, OpenRefused<DirStoreError>> {
        let facts = OpenFacts {
            health: self.health.unwrap_or(NO_ANSWER_YET),
            rescan_pending: false,
            now: self.clock.now(),
        };
        let opened = self.counter.open_with_extras(
            amount,
            tip,
            currency,
            rate,
            source,
            facts,
            by,
            extras,
            &mut self.store,
        )?;
        self.known.insert(opened.index);
        self.port.watch_script(opened.script.clone());
        if extras.request.is_none() {
            self.ln
                .sale_opened(opened.index, opened.needs_sats, self.clock.now());
            // CR-219: the sale asks the swap provider for an invoice too.
            if let Ok(address) = Address::from_script(&opened.script, self.network) {
                self.swap
                    .sale_opened(opened.index, address, opened.needs_sats, self.clock.now());
            }
        }
        Ok(opened)
    }

    /// CR-206: add a note to a sale that has none.
    pub fn add_note(&mut self, index: u32, note: &str) -> Result<(), NoteRefused<DirStoreError>> {
        let now = self.clock.now();
        self.counter.add_note(index, note, now, &mut self.store)
    }

    /// Work out the state of a sale now (CR-10 to CR-19). What it reports is
    /// in the returned screen, once.
    pub fn judge(&mut self, index: u32) -> Result<Screen, JudgeError<DirStoreError>> {
        let now = self.clock.now();
        let own = self.own_health_now();
        let facts = Facts {
            chain: &self.view,
            health: self.health.unwrap_or(NO_ANSWER_YET),
            heartbeat: self.counter.heartbeat_for_policy(now),
            remote_order: false,
            now,
        };
        self.counter
            .judge_with_own_node(index, &facts, &own, &mut self.store)
    }

    /// CR-113: what the screen of a sale shows of Lightning.
    pub fn lightning_sale(&mut self, index: u32) -> SaleLightning {
        let now = self.clock.now();
        let live = self.counter.sale(index).is_some_and(|s| s.is_live());
        if !live {
            return SaleLightning::Off;
        }
        let view = self.ln.sale_view(index, now);
        self.take_ln_notes();
        if !matches!(view, SaleLightning::Off) {
            return view;
        }
        // CR-219: one Lightning path at a time, so only one of the two has
        // anything to show.
        match self.swap.sale_view(index, now) {
            SaleSwap::Off => SaleLightning::Off,
            SaleSwap::Waiting => SaleLightning::Waiting,
            SaleSwap::Invoice(checked) => SaleLightning::Invoice(checked),
            SaleSwap::NotForThisSale => SaleLightning::NotForThisSale,
            SaleSwap::Unavailable(words) => SaleLightning::Unavailable(words),
            SaleSwap::Confirming(words) => SaleLightning::Confirming(words),
            SaleSwap::Failed(words) => SaleLightning::Failed(words),
        }
    }

    /// CR-116: the Lightning line of the status, when a credential is saved.
    pub fn lightning_status(&self) -> Option<LightningStatus> {
        self.ln.status()
    }

    /// Whether sales get a Lightning code now.
    pub fn lightning_on(&self) -> bool {
        self.ln.is_on()
    }

    /// CR-115: remove the Lightning connection at once.
    pub fn lightning_remove(&mut self) {
        self.ln.remove();
    }

    /// CR-115: begin the test of a connection. Nothing is tested or kept if
    /// the signed settings name no node.
    pub fn lightning_setup(&mut self, credential: Credential) -> Result<LnRequest, SetupRefused> {
        let now = self.clock.now();
        self.ln.setup_begin(credential, now)
    }

    /// CR-112: the next request for the node to send, if one is due.
    pub fn lightning_next(&mut self) -> Option<LnRequest> {
        let now = self.clock.now();
        let counter = &self.counter;
        let request = self
            .ln
            .next(now, |i| counter.sale(i).is_some_and(|s| s.is_live()));
        self.take_ln_notes();
        request
    }

    /// CR-114: the certificate the node must show for this request.
    pub fn lightning_pin(&self, id: u64) -> Option<&CertPin> {
        self.ln.pin_for(id)
    }

    /// Whether `id` is the number of the setup's request (CR-115).
    pub fn lightning_is_setup(&self, id: u64) -> bool {
        self.ln.is_setup(id)
    }

    /// CR-115: the answer to a setup request.
    pub fn lightning_setup_answer(
        &mut self,
        id: u64,
        reply: &Reply,
    ) -> Result<SetupStep, SetupRefused> {
        let now = self.clock.now();
        let step = self.ln.setup_answer(id, reply)?;
        if let SetupStep::Done(_) = step {
            // The credential is in use: the open sales ask for invoices.
            let live: Vec<(u32, u64)> = self
                .known
                .iter()
                .filter_map(|i| self.counter.sale(*i))
                .filter(|s| s.is_live() && !s.is_request())
                .map(|s| (s.index(), s.needs_sats()))
                .collect();
            for (index, sats) in live {
                self.ln.sale_opened(index, sats, now);
            }
        }
        Ok(step)
    }

    /// CR-115: a setup request got no answer.
    pub fn lightning_setup_failed(&mut self, id: u64, why: LnTransport) -> SetupRefused {
        self.ln.setup_failed(id, why)
    }

    /// CR-112: the answer to the request the node handed out. A payment
    /// the node reports is judged by the counter (CR-109), and what that
    /// reports comes back with the next pump.
    pub fn lightning_reply(&mut self, id: u64, reply: &Reply) {
        if let LnEvent::Paid { index, facts } = self.ln.reply(id, reply) {
            self.lightning_paid(index, facts);
        }
        self.take_ln_notes();
    }

    /// CR-114: the request got no answer.
    pub fn lightning_failed(&mut self, id: u64, why: LnTransport) {
        let now = self.clock.now();
        self.ln.failed(id, why, now);
        self.take_ln_notes();
    }

    fn lightning_paid(&mut self, index: u32, facts: openbmt_lightning::PaidFacts) {
        let now = self.clock.now();
        let heartbeat = self.counter.heartbeat_for_policy(now);
        let judged = Facts {
            chain: &self.view,
            health: self.health.unwrap_or(NO_ANSWER_YET),
            heartbeat,
            remote_order: false,
            now,
        };
        let paid = LightningPaid {
            payment_hash: facts.payment_hash,
            amount_msat: facts.amount_received_msat,
            settled_at: facts.settled_at,
        };
        let hash = paid.payment_hash;
        // CR-117: a sale already paid over Lightning by another invoice.
        let by_another = self
            .counter
            .sale(index)
            .and_then(|s| s.lightning())
            .is_some_and(|p| p.payment_hash != hash);
        let result = if by_another {
            Err(openbmt_counter::LightningRefused::SaleClosed)
        } else {
            self.counter
                .lightning_paid(index, paid, &judged, &mut self.store)
        };
        match result {
            Ok(screen) => {
                self.ln_reports.extend(screen.reports);
                self.ln.paid(index, &hash);
            }
            // CR-117: the sale is over, or was paid another way: this is a
            // late payment, recorded and flagged for the owner.
            Err(
                openbmt_counter::LightningRefused::SaleClosed
                | openbmt_counter::LightningRefused::PaidAnotherWay,
            ) => match self
                .counter
                .lightning_late(index, paid, now, &mut self.store)
            {
                Ok(reports) => {
                    self.note(format!(
                        "sale {index}: a Lightning invoice was paid after the sale was over"
                    ));
                    self.ln_reports.extend(reports);
                    self.ln.settled(index, &hash);
                }
                // The node still has it: asked again.
                Err(e) => self.note(format!(
                    "sale {index}: a late Lightning payment was not recorded: {e}"
                )),
            },
            Err(e) => {
                self.note(format!(
                    "sale {index}: a Lightning payment was not taken: {e}"
                ));
                // A payment the counter will not take now is not asked
                // about again; a failure to write is, because the node
                // still has it.
                if !matches!(e, openbmt_counter::LightningRefused::Store(_)) {
                    self.ln.settled(index, &hash);
                }
            }
        }
    }

    /// Give a sale a new quote (CR-05).
    pub fn requote(
        &mut self,
        index: u32,
        rate: Rate,
        source: &RateSource,
    ) -> Result<Quote, RequoteRefused<DirStoreError>> {
        let now = self.clock.now();
        let quote = self
            .counter
            .requote(index, rate, source, now, &self.view, &mut self.store)?;
        // CR-112: a new quote asks for a new invoice.
        let sats = self.counter.sale(index).map_or(0, |s| s.needs_sats());
        self.ln.sale_requoted(index, sats, now);
        self.swap.sale_requoted(index, sats, now);
        Ok(quote)
    }

    /// The cashier cancels a sale with nothing received (CR-07).
    pub fn cancel(&mut self, index: u32) -> Result<(), CancelRefused<DirStoreError>> {
        self.cancel_by(index, None)
    }

    /// [`Node::cancel`], naming the person at the till who cancels the sale
    /// (CR-151).
    pub fn cancel_by(
        &mut self,
        index: u32,
        by: Option<&Actor>,
    ) -> Result<(), CancelRefused<DirStoreError>> {
        let now = self.clock.now();
        self.counter.cancel_by(index, now, by, &mut self.store)?;
        self.ln.sale_closed(index);
        self.swap.sale_closed(index);
        Ok(())
    }

    /// A person accepts a sale (CR-20 to CR-22). The caller has checked the
    /// PIN.
    pub fn override_sale(&mut self, index: u32, actor: Actor) -> Overridden<DirStoreError> {
        let now = self.clock.now();
        let own = self.own_health_now();
        let facts = Facts {
            chain: &self.view,
            health: self.health.unwrap_or(NO_ANSWER_YET),
            heartbeat: self.counter.heartbeat_for_policy(now),
            remote_order: false,
            now,
        };
        self.counter
            .override_sale_with_own_node(index, actor, &facts, &own, &mut self.store)
    }

    /// A person asks for a refund of a sale (CR-60 to CR-69). The request is
    /// a record that travels to the owner with the others; nothing is paid.
    /// Returns the request number. The caller has checked the PIN, and has
    /// read the destination with `openbmt_counter::refund_destination`.
    pub fn request_refund(
        &mut self,
        index: u32,
        request: RefundRequest,
    ) -> Result<u64, RefundRefused<DirStoreError>> {
        let now = self.clock.now();
        self.counter
            .request_refund(index, request, now, &self.view, &mut self.store)
    }

    /// A person withdraws a refund request (CR-70). The owner honours it
    /// only if the refund is still queued and not in an open session.
    pub fn withdraw_refund(
        &mut self,
        index: u32,
        number: u64,
        actor: Actor,
    ) -> Result<(), WithdrawRefused<DirStoreError>> {
        let now = self.clock.now();
        self.counter
            .withdraw_refund(index, number, actor, now, &mut self.store)
    }

    /// A refund paid in cash from the till (CR-71).
    pub fn refund_in_cash(
        &mut self,
        index: u32,
        cash: CashRefund,
    ) -> Result<(), RefundRefused<DirStoreError>> {
        let now = self.clock.now();
        self.counter
            .refund_in_cash(index, cash, now, &self.view, &mut self.store)
    }

    /// CR-217: what a bitcoin refund of a sale paid over Lightning can still
    /// ask; `None` for a sale that no Lightning payment settled.
    pub fn refund_onchain_room(&self, index: u32) -> Option<u64> {
        self.counter
            .refund_onchain_room(index, self.clock.now(), &self.view)
    }

    /// The refunds of a sale, for the screen and the receipt (CR-72).
    pub fn refund_views(&self, index: u32) -> Vec<RefundView> {
        self.counter.refund_views(index)
    }

    /// Take a heartbeat the owner's watcher signed (CR-50 to CR-56), as the
    /// bytes of `SignedHeartbeat::to_bytes`. The counter judges it against
    /// the node's header chain and clock. Then every live sale that has a
    /// payment is judged again, and what that reports is returned once. A
    /// heartbeat that does not count (unhealthy, or a tip the node does not
    /// hold) is taken, its number is used up, and the reason is logged; a
    /// disagreement over a block the node holds is logged as such.
    pub fn heartbeat(&mut self, bytes: &[u8]) -> Result<HeartbeatTaken, HeartbeatNotTaken> {
        let signed = SignedHeartbeat::from_bytes(bytes).map_err(HeartbeatNotTaken::Bytes)?;
        let now = self.clock.now();
        let view = self
            .counter
            .heartbeat(&signed, &self.view, now, &mut self.store)
            .map_err(HeartbeatNotTaken::Refused)?;
        if let HeartbeatView::NotVerified(why) = view {
            if why.is_disagreement() {
                self.note(format!(
                    "heartbeat disagreement: {why} (owner tip height {})",
                    signed.heartbeat.tip_height
                ));
            } else {
                self.note(format!("heartbeat not verified: {why}"));
            }
        }
        let mut reports = Vec::new();
        for index in self.sales_with_payments() {
            match self.judge(index) {
                Ok(screen) => reports.extend(screen.reports),
                Err(e) => self.note(format!("sale {index} could not be judged: {e}")),
            }
        }
        self.update_all_blocks();
        Ok(HeartbeatTaken { view, reports })
    }

    /// Take a command the owner signed (CR-14).
    pub fn command(&mut self, signed: &SignedCommand) -> Result<(), CommandRefused<DirStoreError>> {
        let now = self.clock.now();
        self.counter.command(signed, now, &mut self.store)
    }

    /// Append a check-in record to the counter's chain, through the same
    /// stored path as the other records: the file is written first and the
    /// chain changes only if that worked. Returns the record, which the link
    /// sends to the owner (LK-57).
    pub fn check_in(&mut self) -> Result<Record, StoreFault<DirStoreError>> {
        self.counter.check_in(&mut self.store)
    }

    /// CR-143: write a change to the cashiers as a record, through the same
    /// stored path as the others. The record reaches the owner with them.
    pub fn record_staff(
        &mut self,
        event: &StaffEvent,
    ) -> Result<Record, StaffRefused<DirStoreError>> {
        self.counter.record_staff(event, &mut self.store)
    }

    /// CR-152: the totals of the day `[from, to)` from the counter's records.
    pub fn day_totals(
        &self,
        from: u64,
        to: u64,
        people: bool,
    ) -> Result<DayTotals, TotalsRefused<DirStoreError>> {
        self.counter.day_totals(from, to, people, &self.store)
    }

    /// Take a new signed batch of addresses and write the record that says
    /// it is held (CR-167). A batch already held is taken as nothing
    /// (CR-168).
    pub fn add_batch(
        &mut self,
        signed: SignedBatch,
    ) -> Result<(), BatchTakeRefused<DirStoreError>> {
        self.counter.take_batch(signed, &mut self.store)
    }

    /// Stop the driver, then take the events it left in the channel. The
    /// driver saves its headers and address book as it stops. Returns what
    /// those events reported.
    pub fn shutdown(&mut self) -> Vec<Report> {
        self.port.shutdown();
        self.pump(Duration::ZERO)
    }
}
