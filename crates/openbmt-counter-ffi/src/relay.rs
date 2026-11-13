//! The counter's link to the owner behind the entry: the link, what the
//! screen shows of it, and the relay for counters the owner can hand a
//! counter that was paired before the shop had a Box
//! (`docs/COUNTER-RULES.md`, CR-130 to CR-136).
//!
//! One step of the link has three parts (`CounterLink::poll_step`,
//! `take_step`, `send_step`). The first and the last call the relay and can
//! wait as long as it does; only the middle needs the node. So the step
//! holds the link, which only the app's link thread uses, for all three, and
//! the node's lock for the middle alone: a Box that does not answer never
//! holds up a sale. What the screen shows is a snapshot kept beside the
//! link, so `link_state` does not wait for a step either.
//!
//! Two threads must not use the link's number files at once (LK-51). The
//! entry keeps a lease on the data folder for as long as its link lives,
//! and a start on the same folder waits for the old one to let go.

use std::collections::BTreeSet;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

use openbmt_core::identity::{DeviceId, Identity};
use openbmt_core::relay::{RelayHint, SignedRelayHint};
use openbmt_counter_node::{
    start_link, CounterLink, DriverPort, LinkLine, LinkNotStarted, LinkState, Node,
    NoticeStoreError, PairedRecord, RelayRefused, SettingsLine,
};
use openbmt_courier::{Courier, Post};
use openbmt_link::Role;

use crate::error::{from_rule, CounterError};
use crate::views::{LinkStepView, LinkView, WordsView};

/// How long a start waits for an earlier link on the same folder to let go.
/// An earlier link lets go when its step that is at the relay ends, which
/// the relay's own timeouts bound.
pub(crate) const LEASE_WAIT: Duration = Duration::from_secs(45);

/// The folders that have a link, in this process.
static LINK_DIRS: Mutex<BTreeSet<PathBuf>> = Mutex::new(BTreeSet::new());

fn dirs() -> MutexGuard<'static, BTreeSet<PathBuf>> {
    LINK_DIRS.lock().unwrap_or_else(|e| e.into_inner())
}

/// The right to run a link on a data folder. It is let go when dropped.
pub(crate) struct LinkLease(PathBuf);

impl LinkLease {
    /// Take the folder, waiting up to `wait` for another link to let go.
    pub(crate) fn claim(dir: &Path, wait: Duration) -> Option<Self> {
        let end = Instant::now() + wait;
        loop {
            if dirs().insert(dir.to_path_buf()) {
                return Some(Self(dir.to_path_buf()));
            }
            if Instant::now() >= end {
                return None;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

impl Drop for LinkLease {
    fn drop(&mut self) {
        dirs().remove(&self.0);
    }
}

/// What a start needs to make the link (CR-134).
pub(crate) struct LinkPlan {
    pub(crate) hint: RelayHint,
    pub(crate) identity: Identity,
    pub(crate) notice_identity: Identity,
    pub(crate) owner: DeviceId,
    pub(crate) proxy: Option<SocketAddr>,
    pub(crate) tor_on: bool,
    pub(crate) dir: PathBuf,
}

/// What the screen shows of the link, kept apart from the link.
#[derive(Clone)]
struct Snapshot {
    line: LinkLine,
    connected: bool,
    since: Option<u64>,
    refused: bool,
    state: Option<LinkState>,
}

/// The link of an entry.
pub(crate) struct LinkSlot {
    /// The link, or none when it did not start. Held for a whole step.
    pub(crate) link: Mutex<Option<CounterLink>>,
    snapshot: Mutex<Snapshot>,
    _lease: Option<LinkLease>,
}

impl LinkSlot {
    /// Make the link of `plan`. A link that cannot start leaves a slot
    /// whose line says why; the counter itself goes on.
    pub(crate) fn start<P: DriverPort>(
        plan: LinkPlan,
        wait: Duration,
        node: &Node<P>,
    ) -> Result<Self, NoticeStoreError> {
        let lease = LinkLease::claim(&plan.dir, wait);
        let started = match lease {
            None => Err(LinkNotStarted::Files(
                "an earlier link on this folder has not let go".to_string(),
            )),
            Some(_) => start_link(
                &plan.dir,
                plan.identity,
                plan.owner,
                &plan.hint,
                plan.proxy,
                plan.tor_on,
            ),
        };
        let (mut link, line) = match started {
            Ok(link) => (Some(link), LinkLine::Connecting),
            Err(e) => (None, LinkLine::from(&e)),
        };
        // Keep an outbox even before the first Box is added. Its signer is
        // read from the same checked pairing as the node and courier.
        if let Some(link) = link.as_mut() {
            link.enable_notices(plan.notice_identity, plan.dir, node)?;
            sync_box_peers(link.courier_mut(), node.settings().boxes.iter().copied());
        }
        Ok(Self {
            link: Mutex::new(link),
            snapshot: Mutex::new(Snapshot {
                line,
                connected: false,
                since: None,
                refused: false,
                state: None,
            }),
            _lease: lease,
        })
    }

    fn snapshot(&self) -> Snapshot {
        self.snapshot
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// The screen's view of the link.
    pub(crate) fn view(&self) -> LinkView {
        let s = self.snapshot();
        let state = s.state.as_ref();
        LinkView {
            words: WordsView(s.line.words()),
            connected: s.connected,
            since: s.since,
            refused: s.refused,
            heartbeats_taken: state.map_or(0, |st| st.heartbeats_taken),
            owner_records: state.map_or(0, |st| st.owner_records),
            last_check_in_at: state.and_then(|st| st.last_check_in_at),
            settings_version: state.map_or(0, |st| st.settings_version),
            restart_needed: state.is_some_and(|st| st.restart_needed),
            settings_line: state
                .and_then(|st| SettingsLine::of(st.removed, st.restart_needed))
                .map(|line| WordsView(line.words())),
        }
    }

    /// What `link_step` returns.
    pub(crate) fn step_view(&self, notes: Vec<String>) -> LinkStepView {
        LinkStepView {
            link: self.view(),
            notes,
        }
    }

    /// Take the link's state after a step that called the relay: the line
    /// follows the last call, and the time of the first failure stays until
    /// a call works.
    pub(crate) fn update(&self, state: LinkState, now: u64) {
        let mut s = self.snapshot.lock().unwrap_or_else(|e| e.into_inner());
        if state.relay_reachable {
            s.line = LinkLine::Connected;
            s.connected = true;
            s.since = None;
            s.refused = false;
        } else {
            let since = *s.since.get_or_insert(now);
            s.connected = false;
            s.refused = state.relay_refused;
            s.line = if state.relay_refused {
                LinkLine::Refused
            } else {
                LinkLine::NotReachable { since: Some(since) }
            };
        }
        s.state = Some(state);
    }
}

/// Make the courier's Box destinations exactly the Boxes in the settings
/// currently in force. Keep its owner and other peer roles intact.
pub(crate) fn sync_box_peers<R: Post>(
    courier: &mut Courier<R>,
    boxes: impl IntoIterator<Item = DeviceId>,
) {
    let boxes: BTreeSet<DeviceId> = boxes.into_iter().collect();
    let stale: Vec<DeviceId> = courier
        .peers()
        .iter()
        .filter(|(id, role)| **role == Role::Box && !boxes.contains(id))
        .map(|(id, _)| *id)
        .collect();
    for id in stale {
        courier.remove_peer(&id);
    }
    for id in boxes {
        // The owner takes precedence in settings (BX-24). More generally,
        // preserve any already configured non-Box route role on collision.
        if !courier.peers().contains_key(&id) {
            courier.set_peer(id, Role::Box);
        }
    }
}

/// CR-132: the pairing record `record` with the relay of `frame`, which
/// must be signed by the owner root the record pinned. Returns the new
/// record for the app to keep in place of the old; the link starts from it
/// at the next start. Nothing is changed here.
pub(crate) fn take_relay(record: &[u8], frame: &[u8]) -> Result<Vec<u8>, CounterError> {
    let record = PairedRecord::from_bytes(record).map_err(crate::host::record_refused)?;
    let signed =
        SignedRelayHint::from_bytes(frame).map_err(|e| relay_refused(RelayRefused::Bytes(e)))?;
    let next = record.with_relay(&signed).map_err(relay_refused)?;
    Ok(next.to_bytes())
}

fn relay_refused(e: RelayRefused) -> CounterError {
    from_rule(e.rule(), &e, e.words())
}
