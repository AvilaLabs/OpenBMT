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
//! - The counters and Boxes the link talks to are the ones in the signed
//!   settings in force, and in the settings waiting to be delivered
//!   ([`OwnerLink::follow`], LK-67): a counter the settings add is talked to
//!   at once, and one they remove is kept until it holds the settings that
//!   removed it, and then dropped.
//! - A device list is noted and dropped: it travels inside the settings
//!   (CR-166).
//! - Settings the owner signed are carried to the devices on their device list
//!   (LK-64 to LK-66, [`OwnerLink::carry_settings`]). A counter shows it holds
//!   a version by the `Setting` record in the owner's copy of its chain. A Box
//!   has no such record, so a Box counts as holding a version once the relay
//!   took the message for it. A version is sent to a device again after
//!   [`OwnerLinkConfig::settings_resend_secs`], so that a device that is off
//!   does not fill its mailbox with copies.
//! - A POS notice (BX-42) is taken only from a key the table holds as a Box,
//!   with the Box's own signature and a number above the last from that Box.
//!   It is kept, the newest [`MAX_BOX_REPORTS`], as the Box's report and is
//!   never merged into the owner's own matching. The list and the numbers
//!   are in memory: a report is a hint, and the owner's own matching does
//!   not depend on it. Anything refused is noted.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::{Path, PathBuf};

use openbmt_core::batch::PinnedRoots;
use openbmt_core::batch::SignedBatch;
use openbmt_core::command::SignedCommand;
use openbmt_core::identity::DeviceId;
use openbmt_core::records::{DeviceList, Record};
use openbmt_core::settings::{Settings, SettingsError, SignedSettings};
use openbmt_counter_node::DriverPort;
use openbmt_courier::{
    decode_records, received_content, Courier, Post, Received, RelayPost, SendError,
};
use openbmt_link::{Kind, Role};
use openbmt_pos::PosNotice;

use crate::copy::{CheckInTaken, CopyError, CounterCopy, Stop};
use crate::watcher::OwnerWatcher;

/// Most lines the link's own notes keep; the oldest goes first.
pub const NOTE_LOG: usize = 64;

/// Most reports of the Box the link keeps; the oldest goes first.
pub const MAX_BOX_REPORTS: usize = 256;

/// Most settings the link carries at once (LK-64).
pub const MAX_CARRIED_SETTINGS: usize = 16;

/// One thing the Box reported (BX-42): shown as the Box's, never as the
/// owner's own finding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoxReport {
    /// The Box that sent it.
    pub from: DeviceId,
    /// What it said, as the Box signed it.
    pub notice: PosNotice,
    /// When the link took it, by the clock the steps were given.
    pub taken_at: u64,
}

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
    /// Seconds before settings that a device does not yet show it holds are
    /// sent to it again (LK-64).
    pub settings_resend_secs: u64,
}

impl Default for OwnerLinkConfig {
    fn default() -> Self {
        Self {
            heartbeat_every_secs: 20,
            check_in_grace_secs: 30 * 60,
            poll_secs: 1,
            retry_secs: 5,
            settings_resend_secs: 120,
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

/// Why settings were not taken to carry (LK-64). Nothing was changed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SettingsNotCarried {
    /// LK-64: the settings do not check against the owner's pinned roots, or
    /// do not follow the settings before them, or lack a signature they
    /// need.
    Check(SettingsError),
    /// LK-64: the link carries as many settings as it takes.
    Full,
}

impl std::fmt::Display for SettingsNotCarried {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Check(e) => write!(f, "LK-64: the settings are not sent: {e}"),
            Self::Full => write!(
                f,
                "LK-64: the link carries {MAX_CARRIED_SETTINGS} settings already"
            ),
        }
    }
}

impl std::error::Error for SettingsNotCarried {}

/// How many devices still lack settings the link carries (LK-66), for the
/// outbox row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SettingsWaiting {
    /// Counters whose copy does not yet show the version.
    pub counters: usize,
    /// Boxes the relay has not yet taken the settings for.
    pub boxes: usize,
}

impl SettingsWaiting {
    /// Count what `missing` (from [`OwnerLink::settings_missing`]) holds.
    pub fn of(missing: &[(DeviceId, Role)]) -> Self {
        let boxes = missing.iter().filter(|(_, r)| *r == Role::Box).count();
        Self {
            counters: missing.len() - boxes,
            boxes,
        }
    }
}

/// One signed settings the link carries.
#[derive(Debug, Clone)]
struct Carried {
    /// The outbox number of the delivery.
    id: u32,
    version: u32,
    bytes: Vec<u8>,
    /// Who must hold it: the counters and Boxes on its device list, and those
    /// that the settings before it listed.
    targets: BTreeMap<DeviceId, Role>,
}

/// The owner's end of the link. See the module docs.
pub struct OwnerLink<R: Post = RelayPost> {
    courier: Courier<R>,
    root: PathBuf,
    devices: DeviceList,
    counters: Vec<DeviceId>,
    /// LK-67: the counters the settings name now, which the link keeps
    /// whatever else it carries.
    wanted: BTreeSet<DeviceId>,
    copies: BTreeMap<DeviceId, CounterCopy>,
    cfg: OwnerLinkConfig,
    alerts: Vec<Alert>,
    last_check_in_at: BTreeMap<DeviceId, u64>,
    started_at: Option<u64>,
    last_heartbeat_at: Option<u64>,
    next_relay_at: u64,
    notes: VecDeque<String>,
    box_reports: VecDeque<BoxReport>,
    box_numbers: BTreeMap<DeviceId, u64>,
    /// LK-64: the settings carried, lowest version first.
    carried: Vec<Carried>,
    /// LK-64: the targets of the settings last carried, for the next ones.
    last_targets: BTreeMap<DeviceId, Role>,
    /// LK-64: when each version was last sent to each device.
    settings_sent: BTreeMap<(DeviceId, u32), u64>,
    /// LK-65: the Boxes the relay took each version for.
    boxes_sent: BTreeSet<(DeviceId, u32)>,
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
            wanted: counters.iter().copied().collect(),
            counters,
            copies,
            cfg,
            alerts: Vec::new(),
            last_check_in_at: BTreeMap::new(),
            started_at: None,
            last_heartbeat_at: None,
            next_relay_at: 0,
            notes,
            box_reports: VecDeque::new(),
            box_numbers: BTreeMap::new(),
            carried: Vec::new(),
            last_targets: BTreeMap::new(),
            settings_sent: BTreeMap::new(),
            boxes_sent: BTreeSet::new(),
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

    /// The signed device list the copies are checked against.
    pub fn devices(&self) -> &DeviceList {
        &self.devices
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

    /// What the Box reported, oldest first (BX-42). These are the Box's
    /// words and are never merged into the owner's own matching.
    pub fn box_reports(&self) -> impl Iterator<Item = &BoxReport> {
        self.box_reports.iter()
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

    /// LK-64: take signed settings to carry to the devices on their device
    /// list. The settings are checked first: against `roots` and, when
    /// `previous` is given, as a change from it (so that settings that lack a
    /// signature they need are never sent); without `previous` the
    /// signatures that are there and the values are checked. `id` is the
    /// number of the outbox delivery. Settings of a version already carried
    /// are not taken twice. They are sent at once, to the devices that need
    /// them, and again by [`OwnerLink::step`].
    pub fn carry_settings(
        &mut self,
        id: u32,
        signed: &SignedSettings,
        roots: &PinnedRoots,
        previous: Option<&Settings>,
        now: u64,
    ) -> Result<(), SettingsNotCarried> {
        let version = signed.settings.version;
        if self.carried.iter().any(|c| c.version == version) {
            return Ok(());
        }
        match previous {
            Some(previous) => signed.verify(roots, Some(previous)).map(|_| ()),
            None => signed.verify_signatures(roots),
        }
        .map_err(SettingsNotCarried::Check)?;
        if self.carried.len() >= MAX_CARRIED_SETTINGS {
            return Err(SettingsNotCarried::Full);
        }
        let mut targets = BTreeMap::new();
        let lists = [
            previous.map(roles_of),
            Some(roles_of(&signed.settings)),
            Some(self.last_targets.clone()),
        ];
        for (device, role) in lists.into_iter().flatten().flatten() {
            targets.insert(device, role);
        }
        // The owner's own key is never a target.
        targets.remove(&roots.owner);
        for (device, role) in &targets {
            self.courier.set_peer(*device, *role);
        }
        self.last_targets = roles_of(&signed.settings);
        self.carried.push(Carried {
            id,
            version,
            bytes: signed.to_bytes(),
            targets,
        });
        self.carried.sort_by_key(|c| c.version);
        self.send_settings(now);
        Ok(())
    }

    /// LK-67: make the counters and Boxes the link talks to those the settings
    /// name: `counters` and `boxes` are the ones in the signed settings in
    /// force and in the settings waiting to be delivered. A counter not on
    /// the link yet gets a copy and its heartbeats from now on; a Box is
    /// taken as a peer. A counter the settings no longer name stays while
    /// settings carried by the link are still for it (LK-66), and is dropped
    /// when they are done. The copies of a dropped counter stay on disk.
    pub fn follow(
        &mut self,
        counters: &[DeviceId],
        boxes: &[DeviceId],
    ) -> Result<(), OwnerLinkError> {
        self.wanted = counters.iter().copied().collect();
        for counter in counters {
            if self.counters.contains(counter) {
                continue;
            }
            let devices = self.devices.with([*counter]);
            let (copy, torn) =
                CounterCopy::open(&self.root, *counter, &devices).map_err(|error| {
                    OwnerLinkError::Copy {
                        counter: *counter,
                        error,
                    }
                })?;
            if let Some(torn) = torn {
                self.note(format!(
                    "the copy of {counter:?} ended in a record that was cut short: {} bytes cut \
                     off, {} records kept",
                    torn.cut_bytes, torn.kept_records
                ));
            }
            self.devices = devices;
            self.copies.insert(*counter, copy);
            self.counters.push(*counter);
            self.courier.set_peer(*counter, Role::Counter);
        }
        for b in boxes {
            self.courier.set_peer(*b, Role::Box);
        }
        self.drop_retired();
        Ok(())
    }

    /// LK-67: drop the counters the settings no longer name and that no
    /// carried settings are for.
    fn drop_retired(&mut self) {
        let kept: BTreeSet<DeviceId> = self
            .carried
            .iter()
            .flat_map(|c| {
                c.targets
                    .iter()
                    .filter(|(_, role)| **role == Role::Counter)
                    .map(|(d, _)| *d)
            })
            .collect();
        let gone: Vec<DeviceId> = self
            .counters
            .iter()
            .filter(|c| !self.wanted.contains(*c) && !kept.contains(*c))
            .copied()
            .collect();
        for counter in gone {
            self.counters.retain(|c| *c != counter);
            self.copies.remove(&counter);
            self.last_check_in_at.remove(&counter);
            self.alerts.retain(|a| match a {
                Alert::MissedCheckIn { counter: c, .. } | Alert::Diverged { counter: c, .. } => {
                    *c != counter
                }
            });
        }
    }

    /// LK-66: the devices that do not yet hold the settings of delivery `id`:
    /// counters whose copy does not show the version, and Boxes the relay has
    /// not taken it for. Counters without a copy on this link (added after it
    /// started) are not waited for. Empty for settings the link does not
    /// carry.
    pub fn settings_missing(&self, id: u32) -> Vec<(DeviceId, Role)> {
        let Some(carried) = self.carried.iter().find(|c| c.id == id) else {
            return Vec::new();
        };
        carried
            .targets
            .iter()
            .filter(|(device, role)| !self.holds(device, **role, carried.version, true))
            .map(|(device, role)| (*device, *role))
            .collect()
    }

    /// LK-66: forget the settings of delivery `id`, after the caller took it
    /// out of the outbox.
    pub fn settings_done(&mut self, id: u32) {
        let gone: Vec<u32> = self
            .carried
            .iter()
            .filter(|c| c.id == id)
            .map(|c| c.version)
            .collect();
        self.carried.retain(|c| c.id != id);
        self.settings_sent.retain(|(_, v), _| !gone.contains(v));
        self.boxes_sent.retain(|(_, v)| !gone.contains(v));
        self.drop_retired();
    }

    /// Whether `device` shows it holds `version` (LK-65). A counter with no
    /// copy on this link counts as holding when `unknown_holds`.
    fn holds(&self, device: &DeviceId, role: Role, version: u32, unknown_holds: bool) -> bool {
        match role {
            Role::Box => self.boxes_sent.contains(&(*device, version)),
            _ => match self.copies.get(device) {
                Some(copy) => copy.settings_held() >= Some(version),
                None => unknown_holds,
            },
        }
    }

    /// LK-64: send each device the lowest carried version above what it shows
    /// it holds, if it was not sent within the resend time. A counter takes
    /// the versions in order, so that each is checked against the one before
    /// it. `false` if the relay could not be reached.
    fn send_settings(&mut self, now: u64) -> bool {
        let mut work: Vec<(DeviceId, Role, u32, Vec<u8>)> = Vec::new();
        let mut seen: BTreeSet<DeviceId> = BTreeSet::new();
        for c in &self.carried {
            for (device, role) in &c.targets {
                if seen.contains(device) || self.holds(device, *role, c.version, false) {
                    continue;
                }
                seen.insert(*device);
                let due = self
                    .settings_sent
                    .get(&(*device, c.version))
                    .is_none_or(|at| now >= at.saturating_add(self.cfg.settings_resend_secs));
                if due {
                    work.push((*device, *role, c.version, c.bytes.clone()));
                }
            }
        }
        for (device, role, version, bytes) in work {
            // Stamped before the send: a refusal is not tried every step.
            self.settings_sent.insert((device, version), now);
            match self.courier.send(&device, Kind::Settings, &bytes) {
                Ok(()) => {
                    if role == Role::Box {
                        self.boxes_sent.insert((device, version));
                    }
                }
                Err(e) if e.is_retryable() => {
                    self.settings_sent.remove(&(device, version));
                    return false;
                }
                Err(e) => self.note(format!(
                    "LK-64: settings version {version} were not sent to {device:?}: {e}"
                )),
            }
        }
        true
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
        if reached {
            reached = self.send_settings(now);
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
            Kind::PosNotice => self.take_pos_notice(&message, now),
            Kind::DeviceList => self.note(
                "CR-166: a device list: it travels inside the settings, so it is dropped"
                    .to_string(),
            ),
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

    /// BX-42: keep a Box's report. Refused, with a note, unless the sender is
    /// a Box in the table, the notice reads, it names that sender, its
    /// signature is good and its number is above the last from that Box.
    fn take_pos_notice(&mut self, message: &Received, now: u64) {
        let from = message.from;
        if message.role != Role::Box {
            self.note(format!(
                "BX-42: a POS notice from {from:?}, which is not a Box: refused"
            ));
            return;
        }
        let notice = match PosNotice::from_bytes(&message.content) {
            Ok(notice) => notice,
            Err(e) => {
                self.note(format!(
                    "BX-42: a POS notice from {from:?} does not decode: {e}"
                ));
                return;
            }
        };
        if notice.from != from || !notice.verify() {
            self.note(format!(
                "BX-42: a POS notice from {from:?} is not signed by that Box: refused"
            ));
            return;
        }
        let last = self.box_numbers.get(&from).copied().unwrap_or(0);
        if notice.number <= last {
            self.note(format!(
                "BX-42: a POS notice from {from:?} numbered {} is not above {last}: refused",
                notice.number
            ));
            return;
        }
        self.box_numbers.insert(from, notice.number);
        if self.box_reports.len() >= MAX_BOX_REPORTS {
            self.box_reports.pop_front();
        }
        self.box_reports.push_back(BoxReport {
            from,
            notice,
            taken_at: now,
        });
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

/// The counters and Boxes a settings value lists, with their roles.
fn roles_of(settings: &Settings) -> BTreeMap<DeviceId, Role> {
    let mut out = BTreeMap::new();
    for counter in settings.counters.keys() {
        out.insert(*counter, Role::Counter);
    }
    for b in &settings.boxes {
        out.insert(*b, Role::Box);
    }
    out
}
