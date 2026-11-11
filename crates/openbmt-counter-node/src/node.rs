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
//!   for `open` and `judge`.
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
use std::sync::mpsc::RecvTimeoutError;
use std::time::{Duration, Instant};

use bitcoin::consensus::deserialize;
use bitcoin::{BlockHash, ScriptBuf, Transaction, Txid};
use openbmt_core::batch::{AddError, SignedBatch};
use openbmt_core::command::SignedCommand;
use openbmt_core::heartbeat::SignedHeartbeat;
use openbmt_core::identity::Identity;
use openbmt_core::records::Record;
use openbmt_core::settings::Settings;
use openbmt_core::wire::DecodeError;
use openbmt_counter::{
    Actor, Amount, BlockError, CancelRefused, CashRefund, CommandRefused, Counter, Currency, Facts,
    HeartbeatRefused, HeartbeatView, JudgeError, LoadError, OpenFacts, OpenRefused, Opened,
    Overridden, Pairing, Quote, Rate, RateSource, RefundRefused, RefundRequest, RefundView, Report,
    RequoteRefused, Screen, SightingError, StoreFault, WithdrawRefused,
};
use openbmt_net_io::{
    AppEvent, Clock, DirLock, Driver, DriverConfig, EventReceiver, FileStorage, Handle, Key,
    OsRandomness, Storage, SystemClock,
};
use openbmt_watch::{
    check_health, HeaderChain, HeaderStore, Health, HealthInput, PeerAddr, Source, WellFormedBlock,
};

use crate::chainview::ChainView;
use crate::dirstore::{DirStore, DirStoreError};
use crate::notices::NodeRecords;

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
}

/// The function that answers the health question (WR-50 to WR-53). It is
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
    health: Option<Health>,
    settings: Settings,
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

        let mut counter =
            Counter::load(identity, pairing, settings.clone(), loaded.stored(), &store)
                .map_err(StartError::Counter)?;
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

        let port = spawn(Launch {
            driver,
            dir,
            scripts: counter.watched_scripts(),
        })
        .map_err(StartError::Driver)?;
        let mut node = Self {
            counter,
            port,
            store,
            view,
            clock,
            health_fn: health,
            health: None,
            settings,
            known,
            cache: VecDeque::new(),
            notes,
            all_blocks: false,
            checked: loaded.checked.unwrap_or(watch_from),
            checked_cap: None,
            stopped: false,
            last_settle: None,
            settle_wanted: false,
            _lock: lock,
        };
        node.update_all_blocks();
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

    fn note(&mut self, line: String) {
        if self.notes.len() >= NOTE_LOG {
            self.notes.pop_front();
        }
        self.notes.push_back(line);
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
                let answer = (self.health_fn)(&input);
                if self.health != Some(answer) {
                    // A sale that waits on the check can turn red, or green
                    // again, when the answer changes.
                    touched.extend(self.sales_with_payments());
                }
                self.health = Some(answer);
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
                if self.offer(&tx, &from, touched, reports) {
                    self.remember(&tx);
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
        let facts = OpenFacts {
            health: self.health.unwrap_or(NO_ANSWER_YET),
            rescan_pending: false,
            now: self.clock.now(),
        };
        let opened = self
            .counter
            .open(amount, currency, rate, source, facts, &mut self.store)?;
        self.known.insert(opened.index);
        self.port.watch_script(opened.script.clone());
        Ok(opened)
    }

    /// Work out the state of a sale now (CR-10 to CR-19). What it reports is
    /// in the returned screen, once.
    pub fn judge(&mut self, index: u32) -> Result<Screen, JudgeError<DirStoreError>> {
        let now = self.clock.now();
        let facts = Facts {
            chain: &self.view,
            health: self.health.unwrap_or(NO_ANSWER_YET),
            heartbeat: self.counter.heartbeat_for_policy(now),
            remote_order: false,
            now,
        };
        self.counter.judge(index, &facts, &mut self.store)
    }

    /// Give a sale a new quote (CR-05).
    pub fn requote(
        &mut self,
        index: u32,
        rate: Rate,
        source: &RateSource,
    ) -> Result<Quote, RequoteRefused<DirStoreError>> {
        let now = self.clock.now();
        self.counter
            .requote(index, rate, source, now, &self.view, &mut self.store)
    }

    /// The cashier cancels a sale with nothing received (CR-07).
    pub fn cancel(&mut self, index: u32) -> Result<(), CancelRefused<DirStoreError>> {
        let now = self.clock.now();
        self.counter.cancel(index, now, &mut self.store)
    }

    /// A person accepts a sale (CR-20 to CR-22). The caller has checked the
    /// PIN.
    pub fn override_sale(&mut self, index: u32, actor: Actor) -> Overridden<DirStoreError> {
        let now = self.clock.now();
        let facts = Facts {
            chain: &self.view,
            health: self.health.unwrap_or(NO_ANSWER_YET),
            heartbeat: self.counter.heartbeat_for_policy(now),
            remote_order: false,
            now,
        };
        self.counter
            .override_sale(index, actor, &facts, &mut self.store)
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

    /// Take a new signed batch of addresses.
    pub fn add_batch(&mut self, signed: SignedBatch) -> Result<(), AddError<DirStoreError>> {
        self.counter.add_batch(signed, &mut self.store)
    }

    /// Stop the driver, then take the events it left in the channel. The
    /// driver saves its headers and address book as it stops. Returns what
    /// those events reported.
    pub fn shutdown(&mut self) -> Vec<Report> {
        self.port.shutdown();
        self.pump(Duration::ZERO)
    }
}
