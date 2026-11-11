//! The owner phone's background watcher (LK-75).
//!
//! While the owner app is closed the phone can go on following the chain and
//! telling the counters so, so that instant sales do not wait for the owner
//! to open the app. [`WatchNode`] is that, and only that: an [`OwnerWatcher`]
//! on a driver, with no wallet, no queue, no coin view and none of the
//! owner's keys. It is started with the shop's watch key (CR-185) and signs
//! the heartbeats with it, under the tag of the watch key (CR-186). A
//! [`WatchLink`] sends them to the counters through the relay, as messages of
//! kind Heartbeat from a sender in the role Watch (LK-74), and sends nothing
//! else.
//!
//! What it cannot do is by construction: it holds no owner secret, so it signs
//! no batch, no setting, no pairing and no command, and it holds no key 1, so
//! it signs no spend.
//!
//! Where the rules are silent:
//!
//! - The messages the counters send to the owner are sealed to the owner's key
//!   (LK-02), which the watcher does not hold. It reads none: it never polls a
//!   mailbox, so the counters' check-ins and records wait for the owner app, as
//!   they did before.
//! - The link's number files are in a folder of their own, `watch-link`, in
//!   the node's folder, because the watch key is a sender of its own and its
//!   numbers are its own (LK-06). The heartbeat number is the watcher's own
//!   file, the same one the owner node uses, so the line of numbers goes on
//!   from one to the other.
//! - The watcher uses the node's folder for its headers and its address book,
//!   and takes the folder's lock, so it cannot run at the same time as the
//!   owner node (NR-79). It follows the chain's headers; it watches no
//!   scripts, so a coin that arrives while only the watcher runs is found by
//!   the owner node's scan when the app next starts.
//! - The relay must list the watch key as a member (LK-20), like any sender.
//!   A relay that does not refuses the message, and the link says so.

use std::collections::{BTreeMap, VecDeque};
use std::path::{Path, PathBuf};
use std::time::Duration;

use openbmt_core::identity::{DeviceId, Identity};
use openbmt_counter_node::{DriverPort, HealthFn};
use openbmt_courier::{Courier, CourierConfig, FirstStart, Post, RelayPost};
use openbmt_link::{ClientConfig, ClientError, Kind, Role, Route};
use openbmt_net_io::{DriverConfig, Handle};

use crate::watcher::{OwnerConfig, OwnerStartError, OwnerWatcher};

/// The folder, in the node's folder, of the watcher's number files.
pub const WATCH_LINK_DIR: &str = "watch-link";

/// Most lines the link's own notes keep; the oldest goes first.
pub const NOTE_LOG: usize = 32;

/// The intervals of the watcher's link (LK-75). The defaults are those of the
/// owner's link (LK-61).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WatchLinkConfig {
    /// Seconds between the heartbeats sent to a counter.
    pub heartbeat_every_secs: u64,
    /// Seconds to wait after a failure to reach the relay.
    pub retry_secs: u64,
}

impl Default for WatchLinkConfig {
    fn default() -> Self {
        Self {
            heartbeat_every_secs: 20,
            retry_secs: 5,
        }
    }
}

/// What the watcher's link can say about itself, for a notification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WatchState {
    /// Heartbeats the relay took.
    pub sent: u64,
    /// Whether the last call to the relay worked. `false` before the first.
    pub reached: bool,
    /// Whether the last call that failed was refused by the relay, which
    /// answered: the watch key is not one of its members.
    pub relay_refused: bool,
}

/// Why the watcher's link did not start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WatchLinkError {
    /// The number files could not be made or read (LK-53).
    Files(String),
}

impl std::fmt::Display for WatchLinkError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Files(e) => write!(f, "LK-75: the watcher's link files: {e}"),
        }
    }
}

impl std::error::Error for WatchLinkError {}

/// The sending end of the watcher: heartbeats to each counter, and nothing
/// else (LK-75).
pub struct WatchLink<R: Post = RelayPost> {
    courier: Courier<R>,
    counters: Vec<DeviceId>,
    cfg: WatchLinkConfig,
    last_at: Option<u64>,
    next_relay_at: u64,
    sent: u64,
    reached: bool,
    relay_refused: bool,
    notes: VecDeque<String>,
}

impl<R: Post> WatchLink<R> {
    /// A link that sends to `counters` through `courier`. The courier's role
    /// must be [`Role::Watch`] and its peers the counters.
    pub fn new(courier: Courier<R>, counters: Vec<DeviceId>, cfg: WatchLinkConfig) -> Self {
        Self {
            courier,
            counters,
            cfg,
            last_at: None,
            next_relay_at: 0,
            sent: 0,
            reached: false,
            relay_refused: false,
            notes: VecDeque::new(),
        }
    }

    /// The courier, for reading.
    pub fn courier(&self) -> &Courier<R> {
        &self.courier
    }

    /// The courier, for the tests that change what it knows.
    #[cfg(test)]
    pub(crate) fn courier_mut(&mut self) -> &mut Courier<R> {
        &mut self.courier
    }

    /// What the link says about itself.
    pub fn state(&self) -> WatchState {
        WatchState {
            sent: self.sent,
            reached: self.reached,
            relay_refused: self.relay_refused,
        }
    }

    /// Take the link's lines, and the courier's.
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

    /// One turn at `now` (seconds): when a heartbeat is due, ask the watcher
    /// for one for each counter and send it. The relay is called at most once
    /// in `heartbeat_every_secs`, and after a failure not again before
    /// `retry_secs`.
    pub fn step<P: DriverPort>(&mut self, watcher: &mut OwnerWatcher<P>, now: u64) {
        if now < self.next_relay_at {
            return;
        }
        let due = self
            .last_at
            .is_none_or(|at| now >= at.saturating_add(self.cfg.heartbeat_every_secs));
        if !due {
            return;
        }
        self.last_at = Some(now);
        self.courier.begin();
        let mut reached = true;
        for counter in self.counters.clone() {
            let Some(bytes) = watcher.heartbeat(counter) else {
                continue;
            };
            match self.courier.send(&counter, Kind::Heartbeat, &bytes) {
                Ok(()) => {
                    self.sent = self.sent.saturating_add(1);
                    self.relay_refused = false;
                }
                Err(e) if e.is_retryable() => {
                    self.relay_refused = matches!(
                        &e,
                        openbmt_courier::SendError::Relay(ClientError::Refused(_))
                    );
                    reached = false;
                    break;
                }
                Err(e) => self.note(format!("the heartbeat for {counter:?} was not sent: {e}")),
            }
        }
        self.courier.end();
        self.reached = reached;
        if !reached {
            self.next_relay_at = now.saturating_add(self.cfg.retry_secs);
            // Tried again at the next turn after the pause, not after a
            // whole interval.
            self.last_at = None;
        }
    }
}

/// What the watcher is started with: everything it needs and nothing of the
/// owner's.
pub struct WatchConfig {
    /// The node's folder (the same the owner node uses).
    pub dir: PathBuf,
    /// The watch key's secret: the shop's, whose public key the signed
    /// settings name (CR-185).
    pub secret: [u8; 32],
    /// The driver's settings: the network, the proxy and the first peers.
    pub driver: DriverConfig,
    /// The counters' roots.
    pub counters: Vec<DeviceId>,
    /// How to reach the relay.
    pub route: Route,
    /// The relay's key.
    pub relay: DeviceId,
    /// The link's intervals.
    pub link: WatchLinkConfig,
    /// The health rules: `None` is the real ones. A check against a node
    /// with fewer peers than the rules ask for puts something else here, as
    /// it does in [`OwnerConfig`].
    pub health: Option<HealthFn>,
}

/// Why the watcher did not start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WatchStartError {
    /// The watcher itself did not start.
    Watcher(OwnerStartError),
    /// The link did not start.
    Link(WatchLinkError),
    /// The watch key's secret is not a valid key.
    Key,
}

impl std::fmt::Display for WatchStartError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Watcher(e) => write!(f, "{e}"),
            Self::Link(e) => write!(f, "{e}"),
            Self::Key => write!(f, "LK-75: the watch key is not a valid key"),
        }
    }
}

impl std::error::Error for WatchStartError {}

/// The background watcher: [`OwnerWatcher`] and [`WatchLink`]. See the module
/// docs.
pub struct WatchNode<P: DriverPort = Handle, R: Post = RelayPost> {
    watcher: OwnerWatcher<P>,
    link: WatchLink<R>,
}

impl WatchNode<Handle, RelayPost> {
    /// Start on a real driver and a real relay.
    pub fn start(config: WatchConfig) -> Result<Self, WatchStartError> {
        use openbmt_net_io::{Driver, FileStorage, OsRandomness, SystemClock};
        let WatchConfig {
            dir,
            secret,
            driver,
            counters,
            route,
            relay,
            link,
            health,
        } = config;
        let key = |_| WatchStartError::Key;
        let courier = start_courier(
            &dir,
            Identity::from_secret_bytes(&secret).map_err(key)?,
            &counters,
            route,
            relay,
        )?;
        let identity = Identity::from_secret_bytes(&secret).map_err(key)?;
        let mut owner = OwnerConfig::new(&dir, identity, driver);
        owner.watch_key = true;
        if let Some(health) = health {
            owner.health = health;
        }
        let watcher = OwnerWatcher::start_with(owner, |driver, dir| {
            let storage = FileStorage::new(&dir).map_err(|e| e.to_string())?;
            let random = OsRandomness::new().map_err(|e| e.to_string())?;
            Driver::spawn(driver, storage, SystemClock, random, Vec::new())
                .map_err(|e| e.to_string())
        })
        .map_err(WatchStartError::Watcher)?;
        Ok(Self {
            watcher,
            link: WatchLink::new(courier, counters, link),
        })
    }
}

/// The courier of the watcher's link: role Watch, the counters as peers, the
/// number files in their own folder (LK-53).
fn start_courier(
    dir: &Path,
    identity: Identity,
    counters: &[DeviceId],
    route: Route,
    relay: DeviceId,
) -> Result<Courier<RelayPost>, WatchStartError> {
    let files = dir.join(WATCH_LINK_DIR);
    std::fs::create_dir_all(&files)
        .map_err(|e| WatchStartError::Link(WatchLinkError::Files(e.to_string())))?;
    let first = if ["link-inbox", "link-outbox"]
        .iter()
        .any(|f| files.join(f).exists())
    {
        FirstStart::No
    } else {
        FirstStart::Yes
    };
    let peers: BTreeMap<DeviceId, Role> = counters.iter().map(|c| (*c, Role::Counter)).collect();
    Courier::start(
        CourierConfig {
            dir: files,
            identity,
            role: Role::Watch,
            peers,
            route,
            relay,
            client: ClientConfig::default(),
        },
        first,
    )
    .map_err(|e| WatchStartError::Link(WatchLinkError::Files(e.to_string())))
}

impl<P: DriverPort, R: Post> WatchNode<P, R> {
    /// Join a watcher and a link already made, for a test.
    pub fn from_parts(watcher: OwnerWatcher<P>, link: WatchLink<R>) -> Self {
        Self { watcher, link }
    }

    /// The watcher.
    pub fn watcher(&self) -> &OwnerWatcher<P> {
        &self.watcher
    }

    /// The watcher, to make a heartbeat by hand in a check.
    pub fn watcher_mut(&mut self) -> &mut OwnerWatcher<P> {
        &mut self.watcher
    }

    /// The link.
    pub fn link(&self) -> &WatchLink<R> {
        &self.link
    }

    /// Wait up to `wait` for the driver's events and take them.
    pub fn pump(&mut self, wait: Duration) {
        self.watcher.pump(wait);
    }

    /// One turn of the link at `now`.
    pub fn step(&mut self, now: u64) {
        self.link.step(&mut self.watcher, now);
    }

    /// What the link says about itself.
    pub fn state(&self) -> WatchState {
        self.link.state()
    }

    /// The lines of the watcher and of the link.
    pub fn take_notes(&mut self) -> Vec<String> {
        let mut lines = self.watcher.take_notes();
        lines.extend(self.link.take_notes());
        lines
    }

    /// Stop the driver and let go of the folder.
    pub fn shutdown(&mut self) {
        self.watcher.shutdown();
    }
}
