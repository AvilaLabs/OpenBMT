//! The owner's end of the link (LK-55, LK-58 to LK-63).
//!
//! [`OwnerLink`] owns a [`Courier`], one [`CounterCopy`] for each counter on
//! the list, and the alerts the app reads. It is driven by the app loop next
//! to `OwnerWatcher::pump`.
//!
//! Each [`OwnerLink::step`]:
//!
//! 1. raises or clears the missed check-in alerts, whether or not the relay
//!    can be reached (a relay that is down is one of the reasons for a missed
//!    check-in);
//! 2. polls the mailbox and takes what came: records into the copy of the
//!    counter that sent them (LK-58, LK-59), check-ins as the latest (LK-60);
//! 3. tells each counter whose records it took what it now holds, once
//!    (LK-55, LK-60);
//! 4. every `heartbeat_every_secs`, asks the watcher for a heartbeat for each
//!    counter and sends it (LK-61).
//!
//! Where the rules are silent:
//!
//! - A check-in is taken only from the counter that wrote it. A check-in
//!   written by another device is refused and noted. A record in a Records
//!   message need only be authored by a device on the list (LK-58), and the
//!   copy is of the sender's chain.
//! - The time of the last check-in is the owner's own clock at the moment it
//!   was taken, and is not stored: after a restart the grace period of a
//!   counter runs from the first step.
//! - A divergence alert stays until [`OwnerLink::clear`] is called. The
//!   evidence is kept, so a message that shows it again raises it again.
//! - Nothing is done for the settings, device lists and Box notices that are
//!   not handled yet: they are noted and dropped.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::{Path, PathBuf};

use openbmt_core::batch::SignedBatch;
use openbmt_core::command::SignedCommand;
use openbmt_core::identity::DeviceId;
use openbmt_core::records::{DeviceList, Record};
use openbmt_counter_node::DriverPort;
use openbmt_courier::{
    decode_records, received_content, Courier, Post, Received, RelayPost, SendError,
};
use openbmt_link::Kind;

use crate::copy::{CheckInTaken, CopyError, CounterCopy, Stop};
use crate::watcher::OwnerWatcher;

/// Most lines the link's own notes keep; the oldest goes first.
pub const NOTE_LOG: usize = 64;

/// The intervals of the link (LK-61, LK-62). The defaults are the ones in the
/// rules.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnerLinkConfig {
    /// Seconds between the heartbeats sent to a counter (LK-61).
    pub heartbeat_every_secs: u64,
    /// Seconds without a check-in before the alert (LK-62).
    pub check_in_grace_secs: u64,
    /// Fewest seconds between two polls of the relay.
    pub poll_secs: u64,
    /// Seconds to wait after a failure to reach the relay.
    pub retry_secs: u64,
}

impl Default for OwnerLinkConfig {
    fn default() -> Self {
        Self {
            heartbeat_every_secs: 20,
            check_in_grace_secs: 30 * 60,
            poll_secs: 1,
            retry_secs: 5,
        }
    }
}

/// Something the owner must be told (LK-62).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Alert {
    /// No check-in has come from the counter for the grace period.
    MissedCheckIn {
        /// The counter.
        counter: DeviceId,
        /// When the last check-in was taken, or when the link first ran if
        /// none has been since the start.
        last_at: u64,
    },
    /// The counter's chain and the owner's copy differ at a record (LK-59,
    /// LK-60).
    Diverged {
        /// The counter.
        counter: DeviceId,
        /// The number of the record.
        seq: u64,
    },
}

/// What the link says about one counter, for a screen or a log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CounterState {
    /// Records in the owner's copy.
    pub records: u64,
    /// The head of the copy.
    pub head: [u8; 32],
    /// The number of the latest check-in kept.
    pub check_in_seq: Option<u64>,
    /// Whether the copy holds the latest check-in at its place: `None` with
    /// no check-in kept, `Some(false)` while the copy is too short or holds
    /// another record there.
    pub check_in_held: Option<bool>,
    /// When a check-in was last taken, by the clock the steps were given.
    pub last_check_in_at: Option<u64>,
}

/// Why the link did not start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OwnerLinkError {
    /// The copy of a counter's chain could not be opened.
    Copy {
        /// The counter.
        counter: DeviceId,
        /// What is wrong.
        error: CopyError,
    },
}

impl std::fmt::Display for OwnerLinkError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Copy { counter, error } => {
                write!(f, "the copy of the chain of {counter:?}: {error}")
            }
        }
    }
}

impl std::error::Error for OwnerLinkError {}

/// The owner's end of the link. See the module docs.
pub struct OwnerLink<R: Post = RelayPost> {
    courier: Courier<R>,
    root: PathBuf,
    devices: DeviceList,
    counters: Vec<DeviceId>,
    copies: BTreeMap<DeviceId, CounterCopy>,
    cfg: OwnerLinkConfig,
    alerts: Vec<Alert>,
    last_check_in_at: BTreeMap<DeviceId, u64>,
    started_at: Option<u64>,
    last_heartbeat_at: Option<u64>,
    next_relay_at: u64,
    notes: VecDeque<String>,
}

impl<R: Post> OwnerLink<R> {
    /// A link for the counters in `counters`, with the copies kept under
    /// `root` (the owner node's directory). `devices` is the signed device
    /// list: every device that was on it at any time (`DeviceList`). A copy
    /// that is on disk is read back and checked from its first record; a cut
    /// last record is cut off and noted.
    pub fn new(
        courier: Courier<R>,
        root: impl Into<PathBuf>,
        devices: DeviceList,
        counters: Vec<DeviceId>,
        cfg: OwnerLinkConfig,
    ) -> Result<Self, OwnerLinkError> {
        let root = root.into();
        let mut copies = BTreeMap::new();
        let mut notes = VecDeque::new();
        for counter in &counters {
            let (copy, torn) = CounterCopy::open(&root, *counter, &devices).map_err(|error| {
                OwnerLinkError::Copy {
                    counter: *counter,
                    error,
                }
            })?;
            if let Some(torn) = torn {
                notes.push_back(format!(
                    "the copy of {counter:?} ended in a record that was cut short: {} bytes cut \
                     off, {} records kept",
                    torn.cut_bytes, torn.kept_records
                ));
            }
            copies.insert(*counter, copy);
        }
        Ok(Self {
            courier,
            root,
            devices,
            counters,
            copies,
            cfg,
            alerts: Vec::new(),
            last_check_in_at: BTreeMap::new(),
            started_at: None,
            last_heartbeat_at: None,
            next_relay_at: 0,
            notes,
        })
    }

    /// The courier, for reading.
    pub fn courier(&self) -> &Courier<R> {
        &self.courier
    }

    /// The courier, for the tests that change what it knows.
    pub fn courier_mut(&mut self) -> &mut Courier<R> {
        &mut self.courier
    }

    /// The directory the copies are under.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The counters on the list.
    pub fn counters(&self) -> &[DeviceId] {
        &self.counters
    }

    /// The owner's copy of a counter's chain.
    pub fn copy(&self, counter: &DeviceId) -> Option<&CounterCopy> {
        self.copies.get(counter)
    }

    /// What the link says about a counter.
    pub fn state(&self, counter: &DeviceId) -> Option<CounterState> {
        let copy = self.copies.get(counter)?;
        Some(CounterState {
            records: copy.len(),
            head: copy.head(),
            check_in_seq: copy.check_in().map(|r| r.seq),
            check_in_held: copy.check_in().map(|c| {
                usize::try_from(c.seq)
                    .ok()
                    .and_then(|n| copy.records().get(n))
                    .is_some_and(|held| held.hash() == c.hash())
            }),
            last_check_in_at: self.last_check_in_at.get(counter).copied(),
        })
    }

    /// The alerts that are raised now (LK-62). Each is in the list once.
    pub fn alerts(&self) -> &[Alert] {
        &self.alerts
    }

    /// The owner's word that an alert is dealt with. A missed check-in
    /// clears itself when one comes; a divergence is cleared only here, and
    /// is raised again by the next message that shows it.
    pub fn clear(&mut self, alert: &Alert) {
        self.alerts.retain(|a| a != alert);
    }

    /// The lines the link and its courier have logged, oldest first.
    pub fn notes(&self) -> impl Iterator<Item = &str> {
        self.notes
            .iter()
            .map(String::as_str)
            .chain(self.courier.notes())
    }

    /// Take the logged lines.
    pub fn take_notes(&mut self) -> Vec<String> {
        let mut lines: Vec<String> = self.notes.drain(..).collect();
        lines.extend(self.courier.take_notes());
        lines
    }

    fn note(&mut self, line: String) {
        if self.notes.len() >= NOTE_LOG {
            self.notes.pop_front();
        }
        self.notes.push_back(line);
    }

    fn raise(&mut self, alert: Alert) {
        if !self.alerts.contains(&alert) {
            self.alerts.push(alert);
        }
    }

    /// Send a signed command to a counter (LK-61). The counter checks it by
    /// its own rules.
    pub fn send_command(
        &mut self,
        counter: &DeviceId,
        command: &SignedCommand,
    ) -> Result<(), SendError> {
        self.send_now(counter, Kind::Command, &command.to_bytes())
    }

    /// Send a signed batch of addresses to a counter (LK-61).
    pub fn send_batch(&mut self, counter: &DeviceId, batch: &SignedBatch) -> Result<(), SendError> {
        self.send_now(counter, Kind::Batch, &batch.to_bytes())
    }

    fn send_now(&mut self, to: &DeviceId, kind: Kind, content: &[u8]) -> Result<(), SendError> {
        self.courier.send(to, kind, content)
    }

    /// One turn of the link at `now` (seconds).
    pub fn step<P: DriverPort>(&mut self, watcher: &mut OwnerWatcher<P>, now: u64) {
        let started = *self.started_at.get_or_insert(now);
        self.check_missed(now, started);
        if now < self.next_relay_at {
            return;
        }
        self.next_relay_at = now.saturating_add(self.cfg.poll_secs);
        self.courier.begin();
        let mut reply_due = BTreeSet::new();
        let mut reached = match self.courier.poll() {
            Ok(received) => {
                for message in received {
                    self.route(message, now, &mut reply_due);
                }
                true
            }
            Err(_) => false,
        };
        // Taking a check-in may have cleared an alert; taking records may
        // have raised one.
        self.check_missed(now, started);
        if reached {
            reached = self.reply(&reply_due);
        }
        if reached {
            reached = self.heartbeats(watcher, now);
        }
        self.courier.end();
        if !reached {
            self.next_relay_at = now.saturating_add(self.cfg.retry_secs);
        }
    }

    /// LK-62: the alert for a counter that has not checked in.
    fn check_missed(&mut self, now: u64, started: u64) {
        let counters = self.counters.clone();
        for counter in counters {
            let last_at = self
                .last_check_in_at
                .get(&counter)
                .copied()
                .unwrap_or(started);
            let silent = now.saturating_sub(last_at) >= self.cfg.check_in_grace_secs;
            let raised = self
                .alerts
                .iter()
                .any(|a| matches!(a, Alert::MissedCheckIn { counter: c, .. } if *c == counter));
            if silent && !raised {
                self.raise(Alert::MissedCheckIn { counter, last_at });
            }
        }
    }

    fn route(&mut self, message: Received, now: u64, reply_due: &mut BTreeSet<DeviceId>) {
        match message.kind {
            Kind::Records => self.take_records(message.from, &message.content, reply_due),
            Kind::CheckIn => self.take_check_in(message.from, &message.content, now),
            other => self.note(format!("a {other:?} message: not handled yet")),
        }
    }

    /// LK-58, LK-59.
    fn take_records(&mut self, from: DeviceId, content: &[u8], reply_due: &mut BTreeSet<DeviceId>) {
        let records = match decode_records(content) {
            Ok(records) => records,
            Err(e) => {
                self.note(format!("records from {from:?} do not decode: {e}"));
                return;
            }
        };
        let Some(copy) = self.copies.get_mut(&from) else {
            self.note(format!(
                "records from {from:?}, which is not a counter on the list: dropped"
            ));
            return;
        };
        let taken = copy.take(records, &self.devices);
        let diverged_at = copy.diverged_at();
        // Even a message that was refused is answered: the counter learns
        // what the copy holds and sends from there.
        reply_due.insert(from);
        if let Some(seq) = taken.diverged {
            self.raise(Alert::Diverged { counter: from, seq });
        }
        if let Some(seq) = diverged_at {
            self.raise(Alert::Diverged { counter: from, seq });
        }
        if let Some(stop) = taken.stopped {
            self.note(format!(
                "records from {from:?} stopped after {} taken, {} skipped: {stop}",
                taken.appended, taken.skipped
            ));
            if let Stop::Store(_) = stop {
                // Not answered: nothing is known to be held.
                reply_due.remove(&from);
            }
        }
    }

    /// LK-60.
    fn take_check_in(&mut self, from: DeviceId, content: &[u8], now: u64) {
        let record = match Record::from_bytes(content) {
            Ok(record) => record,
            Err(e) => {
                self.note(format!("a check-in from {from:?} does not decode: {e}"));
                return;
            }
        };
        let Some(copy) = self.copies.get_mut(&from) else {
            self.note(format!(
                "a check-in from {from:?}, which is not a counter on the list: dropped"
            ));
            return;
        };
        match copy.take_check_in(record, &self.devices) {
            CheckInTaken::Taken { .. } => self.checked_in(from, now),
            CheckInTaken::Diverged { seq } => {
                self.checked_in(from, now);
                self.raise(Alert::Diverged { counter: from, seq });
            }
            CheckInTaken::Older => {
                self.note(format!("an older check-in from {from:?}: ignored"));
            }
            CheckInTaken::Refused(e) => {
                self.note(format!("a check-in from {from:?} was refused: {e}"));
            }
            CheckInTaken::Store(e) => {
                self.note(format!("a check-in from {from:?} was not kept: {e}"));
            }
        }
    }

    fn checked_in(&mut self, counter: DeviceId, now: u64) {
        self.last_check_in_at.insert(counter, now);
        self.alerts
            .retain(|a| !matches!(a, Alert::MissedCheckIn { counter: c, .. } if *c == counter));
    }

    /// LK-55: tell each counter what the copy holds. `false` if the relay
    /// could not be reached.
    fn reply(&mut self, due: &BTreeSet<DeviceId>) -> bool {
        for counter in due {
            let Some(copy) = self.copies.get(counter) else {
                continue;
            };
            let content = received_content(copy.len(), &copy.head());
            match self.courier.send(counter, Kind::RecordsReceived, &content) {
                Ok(()) => {}
                Err(e) if e.is_retryable() => return false,
                Err(e) => self.note(format!("the receipt for {counter:?} was not sent: {e}")),
            }
        }
        true
    }

    /// LK-61. `false` if the relay could not be reached.
    fn heartbeats<P: DriverPort>(&mut self, watcher: &mut OwnerWatcher<P>, now: u64) -> bool {
        let due = self
            .last_heartbeat_at
            .is_none_or(|at| now >= at.saturating_add(self.cfg.heartbeat_every_secs));
        if !due {
            return true;
        }
        self.last_heartbeat_at = Some(now);
        for counter in self.counters.clone() {
            let Some(bytes) = watcher.heartbeat(counter) else {
                continue;
            };
            match self.courier.send(&counter, Kind::Heartbeat, &bytes) {
                Ok(()) => {}
                Err(e) if e.is_retryable() => return false,
                Err(e) => self.note(format!("the heartbeat for {counter:?} was not sent: {e}")),
            }
        }
        true
    }
}
