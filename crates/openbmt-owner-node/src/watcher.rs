//! The owner's watcher: one driver, one directory, one chain view.
//!
//! ## What it does with each event
//!
//! - `Peers`: keeps the tip's time in the [`ChainView`] and runs the health
//!   rules (WR-50 to WR-53), keeping the answer for the next heartbeat.
//! - `Chain`: updates the [`ChainView`].
//! - `Note`: kept in a bounded log.
//! - `Stopped`: noted.
//! - `Sighting`, `AlsoAnnounced`, `Block` and `Checked` are not used: the
//!   watcher has no script to watch in this slice.
//!
//! ## The heartbeat
//!
//! [`OwnerWatcher::heartbeat`] builds the next heartbeat for a counter: the
//! next number, the clock's time, the tip of the view (height and hash) and
//! the health answer of the real rules ("not healthy" before the first
//! `Peers` event). The owner root signs it and the bytes are handed back.
//!
//! The number is written to a small file in the watcher's directory before
//! the bytes are returned (the file is written whole: temporary file, flush,
//! rename, flush of the directory). It is the larger of the last number plus
//! one and the clock's time in seconds, so a directory that was lost or
//! restored from an old copy does not start again from a low number that the
//! counter would refuse for good. If the number cannot be stored, no
//! heartbeat is returned and the failure is logged. A stored number that does
//! not read back is logged and taken as zero: a number that is too low is
//! refused by the counter and does no harm.

use std::collections::VecDeque;
use std::fmt;
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::RecvTimeoutError;
use std::time::{Duration, Instant};

use bitcoin::hashes::Hash;
use openbmt_core::heartbeat::WatcherHeartbeat;
use openbmt_core::identity::{DeviceId, Identity};
use openbmt_counter_node::{ChainView, DriverPort, HealthFn, MAX_EVENTS_PER_PUMP};
use openbmt_net_io::{
    AppEvent, Clock, DirLock, Driver, DriverConfig, FileStorage, Handle, Key, OsRandomness,
    Storage, SystemClock,
};
use openbmt_watch::{check_health, HeaderChain, HeaderStore, Health, HealthInput};

/// The file that holds the number of the last heartbeat made.
pub const HEARTBEAT_FILE: &str = "heartbeat-seq.bin";
/// Most lines the log keeps; the oldest goes first.
pub const NOTE_LOG: usize = 256;

/// The answer before the first `Peers` event: the check is not working.
const NO_ANSWER_YET: Health = Health {
    healthy: false,
    problem: None,
    behind: false,
};

/// Why the watcher did not start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OwnerStartError {
    /// Another process, or another start in this one, is using the
    /// directory (NR-79).
    Locked(String),
    /// The directory could not be made or read.
    Dir(String),
    /// The stored headers could not be read at all.
    Headers(String),
    /// The driver did not start.
    Driver(String),
}

impl fmt::Display for OwnerStartError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Locked(e) => write!(f, "{e}"),
            Self::Dir(e) => write!(f, "the data directory: {e}"),
            Self::Headers(e) => write!(f, "the stored headers: {e}"),
            Self::Driver(e) => write!(f, "the driver: {e}"),
        }
    }
}

impl std::error::Error for OwnerStartError {}

/// What a watcher is started with.
pub struct OwnerConfig {
    /// The directory for everything the watcher keeps. It is its own: the
    /// driver's headers and address book, and the heartbeat number.
    pub dir: PathBuf,
    /// The owner root's identity, which signs the heartbeats.
    pub identity: Identity,
    /// The driver's settings. Its `watch_from` is replaced by the tip of the
    /// stored headers.
    pub driver: DriverConfig,
    /// The watcher's clock. The driver has its own, the system's.
    pub clock: Box<dyn Clock>,
    /// The health rules. [`check_health`] unless a check against a node with
    /// fewer peers than the rules ask for puts something else here.
    pub health: HealthFn,
}

impl OwnerConfig {
    /// A configuration with the system clock and the real health rules.
    pub fn new(dir: impl Into<PathBuf>, identity: Identity, driver: DriverConfig) -> Self {
        Self {
            dir: dir.into(),
            identity,
            driver,
            clock: Box::new(SystemClock),
            health: Box::new(check_health),
        }
    }
}

/// The owner's watcher. See the module docs.
pub struct OwnerWatcher<P: DriverPort = Handle> {
    identity: Identity,
    port: P,
    dir: PathBuf,
    view: ChainView,
    clock: Box<dyn Clock>,
    health_fn: HealthFn,
    health: Option<Health>,
    last_seq: u64,
    notes: VecDeque<String>,
    stopped: bool,
    /// NR-79. Last, so that it is released after the driver has stopped.
    _lock: DirLock,
}

impl OwnerWatcher<Handle> {
    /// Start the watcher on a real driver that keeps its headers and its
    /// address book in the watcher's directory.
    pub fn start(config: OwnerConfig) -> Result<Self, OwnerStartError> {
        Self::start_with(config, |driver, dir| {
            let storage = FileStorage::new(&dir).map_err(|e| e.to_string())?;
            let random = OsRandomness::new().map_err(|e| e.to_string())?;
            Driver::spawn(driver, storage, SystemClock, random, Vec::new())
                .map_err(|e| e.to_string())
        })
    }
}

impl<P: DriverPort> OwnerWatcher<P> {
    /// Start the watcher on a driver that `spawn` makes, given the driver's
    /// settings and the directory.
    pub fn start_with(
        config: OwnerConfig,
        spawn: impl FnOnce(DriverConfig, PathBuf) -> Result<P, String>,
    ) -> Result<Self, OwnerStartError> {
        let OwnerConfig {
            dir,
            identity,
            mut driver,
            clock,
            health,
        } = config;
        // NR-79: before anything in the directory is read. This makes the
        // directory too.
        let lock = DirLock::acquire(&dir).map_err(|e| OwnerStartError::Locked(e.to_string()))?;
        fs::create_dir_all(&dir).map_err(|e| OwnerStartError::Dir(e.to_string()))?;
        let mut notes = VecDeque::new();

        // The headers the driver will start from, read here before it is
        // spawned, so that the view is the driver's.
        let network = driver.network;
        let mut files =
            FileStorage::new(&dir).map_err(|e| OwnerStartError::Headers(e.to_string()))?;
        let stored = files
            .load(Key::Headers)
            .map_err(|e| OwnerStartError::Headers(e.to_string()))?;
        let new_store =
            || HeaderStore::new(network).map_err(|e| OwnerStartError::Headers(e.to_string()));
        let headers = match stored {
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
        // Nothing is watched, so there are no blocks to check below the tip.
        driver.watch_from = headers.tip_height();

        let last_seq = match read_seq(&dir) {
            Ok(seq) => seq.unwrap_or(0),
            Err(why) => {
                notes.push_back(format!(
                    "{HEARTBEAT_FILE} does not read back ({why}); the next number will come \
                     from the clock"
                ));
                0
            }
        };

        let port = spawn(driver, dir.clone()).map_err(OwnerStartError::Driver)?;
        Ok(Self {
            identity,
            port,
            dir,
            view,
            clock,
            health_fn: health,
            health: None,
            last_seq,
            notes,
            stopped: false,
            _lock: lock,
        })
    }

    // What the watcher knows.

    /// The owner's key, which a counter pins as its owner root.
    pub fn device_id(&self) -> DeviceId {
        self.identity.device_id()
    }

    /// The best header chain as the watcher knows it.
    pub fn chain(&self) -> &ChainView {
        &self.view
    }

    /// The last answer of the health rules, if a `Peers` event has come.
    pub fn health(&self) -> Option<Health> {
        self.health
    }

    /// The number of the last heartbeat made (0 if none).
    pub fn last_seq(&self) -> u64 {
        self.last_seq
    }

    /// The lines the watcher and the driver have logged, oldest first.
    pub fn notes(&self) -> impl Iterator<Item = &str> {
        self.notes.iter().map(String::as_str)
    }

    /// Take the logged lines.
    pub fn take_notes(&mut self) -> Vec<String> {
        self.notes.drain(..).collect()
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

    /// Take the driver's events for up to `wait` and apply each. With a
    /// `wait` of zero it takes what is already waiting.
    pub fn pump(&mut self, wait: Duration) {
        let deadline = Instant::now() + wait;
        for _ in 0..MAX_EVENTS_PER_PUMP {
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
                Ok(event) => self.apply(event),
                Err(disconnected) => {
                    if disconnected {
                        self.stopped = true;
                    }
                    break;
                }
            }
        }
    }

    fn apply(&mut self, event: AppEvent) {
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
                self.health = Some((self.health_fn)(&input));
            }
            AppEvent::Chain { joined, left } => self.view.apply(&joined, &left),
            AppEvent::Note(line) => self.note(line),
            AppEvent::Stopped => self.stopped = true,
            AppEvent::Sighting { .. }
            | AppEvent::AlsoAnnounced { .. }
            | AppEvent::Block { .. }
            | AppEvent::Checked(_) => {}
        }
    }

    // What goes to the counter.

    /// The next signed heartbeat for `counter`, as the bytes of
    /// `SignedHeartbeat::to_bytes`: the next number, the clock's time, the
    /// tip of the view and the health answer of the real rules. The number is
    /// stored before the bytes are returned. `None` if it could not be
    /// stored, or if the view holds no hash at its tip; the reason is logged.
    pub fn heartbeat(&mut self, counter: DeviceId) -> Option<Vec<u8>> {
        let now = self.clock.now();
        let tip_height = self.view.tip_height();
        let Some(tip_hash) = self.view.hash_at(tip_height) else {
            self.note("no heartbeat: the view of the chain holds no tip".to_string());
            return None;
        };
        let seq = self.last_seq.saturating_add(1).max(now);
        if let Err(e) = write_seq(&self.dir, seq) {
            self.note(format!("no heartbeat: the number could not be stored: {e}"));
            return None;
        }
        self.last_seq = seq;
        let healthy = self.health.unwrap_or(NO_ANSWER_YET).healthy;
        let heartbeat = WatcherHeartbeat {
            counter,
            seq,
            time: now,
            tip_height,
            tip_hash: tip_hash.to_byte_array(),
            healthy,
        };
        Some(heartbeat.sign(&self.identity).to_bytes())
    }

    /// Stop the driver, then take the events it left in the channel.
    pub fn shutdown(&mut self) {
        self.port.shutdown();
        self.pump(Duration::ZERO);
    }
}

/// The stored number: eight bytes, little endian. `Ok(None)` if there is no
/// file.
fn read_seq(dir: &Path) -> Result<Option<u64>, String> {
    match fs::read(dir.join(HEARTBEAT_FILE)) {
        Ok(bytes) => <[u8; 8]>::try_from(bytes.as_slice())
            .map(|b| Some(u64::from_le_bytes(b)))
            .map_err(|_| format!("{} bytes, not 8", bytes.len())),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

/// Write the number whole: temporary file, flush, rename, flush the
/// directory. On an error the old file is as it was.
fn write_seq(dir: &Path, seq: u64) -> Result<(), String> {
    let temp = dir.join(format!("{HEARTBEAT_FILE}.tmp"));
    let written = File::create(&temp).and_then(|mut f| {
        f.write_all(&seq.to_le_bytes())?;
        f.sync_all()
    });
    let result = written
        .and_then(|()| fs::rename(&temp, dir.join(HEARTBEAT_FILE)))
        .and_then(|()| File::open(dir).and_then(|d| d.sync_all()));
    result.map_err(|e| format!("{HEARTBEAT_FILE}: {e}"))
}
