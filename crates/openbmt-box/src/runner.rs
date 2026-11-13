//! The Box running: [`BoxCore`] wired to a relay and a server (BX-01,
//! BX-03, BX-21 to BX-24).
//!
//! The relay's identity is the Box's key. While the Box is unpaired its
//! member list is empty and it offers its claim (BX-12) when an invitation is
//! open; it takes a welcome through a hook that calls
//! [`BoxCore::take_welcome`] (LK-27). Once paired, the member list is the
//! one [`BoxCore::members`] gives and is replaced whenever settings change.
//!
//! The Box is also a reader of its own mailbox. A courier over the relay in
//! this process ([`LocalPost`], no socket) fetches what is for the Box and
//! opens it with the roles of BX-24. A message of kind Settings goes to
//! [`BoxCore::take_settings`]; a paid notice goes to the POS side
//! ([`BoxPos`], BX-40); any other kind is noted as not handled yet and
//! dropped. The same worker runs the POS timer and sends what the POS side
//! found to the owner as POS notices (BX-42). A Box that is not paired or is
//! turned off runs none of that (BX-03, BX-23).
//!
//! The Box can also take sale requests from a till on a listener of its own
//! (BX-70 to BX-79). The listener keeps the sales in a table in memory; the
//! worker sends what the table has for the counters and moves the sales on
//! what the counters' notices say.
//!
//! The lock order is the relay's, then the core's: the welcome hook runs
//! inside the relay's lock and takes the core's. Nothing here takes the
//! relay's lock while holding the core's.

use std::collections::{BTreeMap, VecDeque};
use std::fmt;
use std::fs;
use std::io;
use std::net::{SocketAddr, TcpListener};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use openbmt_core::identity::{DeviceId, Identity};
use openbmt_core::settings::DeviceRole;
use openbmt_courier::{Courier, FirstStart, Post, Received, StartError};
use openbmt_link::proto::{Refusal, Request, Response};
use openbmt_link::relay::{sign_challenge, Action};
use openbmt_link::{
    os_random, ClientError, DirStore, Fetched, Kind, Relay, RelayConfig, Role, Server,
    ServerConfig, WelcomeAnswer,
};
use openbmt_net_io::env::{DirLock, LockError};
use openbmt_net_io::Clock;

use crate::files::{BoxFiles, DirFiles};
use crate::https::{CurlRunner, Transport};
use crate::pos::{BoxPos, PosOpenError, PosSetup, PosState};
use crate::pos_config::{PosConfig, PosConfigError, POS_CONFIG_FILE};
use crate::say::note_words;
use crate::state::{BoxCore, BoxError, BoxStatus};
use crate::status_page::{Entered, PageBackend, PageConfig, PageView, StatusPage};
use crate::till::{
    check_till_key, clean_reference, find_counter, make_till_key, parse_amount, KeyCheck, NewSale,
    SaleInfo, TillRefusal, TillTable, TILL_KEY_FILE,
};
use crate::till_listener::{CreateBody, TillBackend, TillConfig, TillListener};
use crate::vendor::{VendorConnector, VENDOR_POLL_SECS};

/// The relay's own files (mailboxes, member list) are in this directory of
/// the data directory.
pub const RELAY_DIR: &str = "relay";
/// Most lines the Box's notes keep; the oldest goes first.
pub const NOTE_LOG: usize = 64;
/// How long the worker waits between steps.
pub const STEP_PAUSE: Duration = Duration::from_millis(200);

/// The connection number the Box's own reads use on its relay.
const LOCAL_CONN: u64 = u64::MAX;

/// Why the Box did not start.
#[derive(Debug)]
pub enum StartFailure {
    /// Another process has the data directory (BX-02).
    Lock(LockError),
    /// The state could not be loaded (BX-51) or the system failed (BX-01).
    State(BoxError),
    /// The relay's storage could not be opened (LK-41).
    Relay(String),
    /// The Box's own link files do not read back: it needs a new pairing
    /// (LK-40).
    Courier(StartError),
    /// The listener could not be made.
    Listen(io::Error),
}

impl fmt::Display for StartFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Lock(e) => write!(f, "BX-02: {e}"),
            Self::State(e) => write!(f, "{e}"),
            Self::Relay(e) => write!(f, "LK-41: the relay's files: {e}"),
            Self::Courier(e) => write!(f, "{e}"),
            Self::Listen(e) => write!(f, "the Box could not listen: {e}"),
        }
    }
}

impl std::error::Error for StartFailure {}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn link_role(role: DeviceRole) -> Role {
    match role {
        DeviceRole::Owner => Role::Owner,
        DeviceRole::Counter => Role::Counter,
        DeviceRole::Box => Role::Box,
    }
}

type Relays = Arc<Mutex<Relay<DirStore>>>;
type Core = Arc<Mutex<BoxCore<DirFiles>>>;
type Notes = Arc<Mutex<VecDeque<String>>>;
type View = Arc<Mutex<PosView>>;
type Till = Arc<Mutex<TillTable>>;

/// What the console's `pos` command shows (BX-41): the POS side as the
/// worker last saw it. It never holds the token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PosView {
    /// This build has no connector: the Box does not read a POS.
    NotInstalled,
    /// The POS side could not be opened, and why. The relay runs on.
    Disabled(PosOpenError),
    /// The POS side as of its last step.
    State(Box<PosState>),
}

impl fmt::Display for PosView {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotInstalled => write!(f, "no POS connector is installed in this program"),
            Self::Disabled(why) => write!(f, "the POS side is off: {why}"),
            Self::State(state) => write!(f, "{state}"),
        }
    }
}

fn note(notes: &Notes, line: String) {
    let mut n = lock(notes);
    if n.len() >= NOTE_LOG {
        n.pop_front();
    }
    n.push_back(line);
}

/// A [`Post`] on the relay in this process: every call goes through the
/// relay's own `handle`, with its challenges, as it would on the wire.
pub struct LocalPost {
    relay: Relays,
    clock: Arc<dyn Clock + Send + Sync>,
}

impl LocalPost {
    fn ask(&self, request: Request) -> Result<Response, ClientError> {
        let now = self.clock.now();
        let mut relay = lock(&self.relay);
        Ok(relay.handle(LOCAL_CONN, request, now, &mut os_random))
    }

    fn signed(
        &self,
        reader: &Identity,
        action: Action,
        number: u64,
        make: impl FnOnce([u8; 32], openbmt_core::identity::Sig) -> Request,
    ) -> Result<Response, ClientError> {
        let now = self.clock.now();
        let mut relay = lock(&self.relay);
        let challenge = match relay.handle(LOCAL_CONN, Request::Challenge, now, &mut os_random) {
            Response::Challenge { challenge } => challenge,
            Response::Refused(r) => return Err(ClientError::Refused(r)),
            _ => return Err(ClientError::Unexpected),
        };
        let key = relay.device_id();
        let sig = sign_challenge(reader, &challenge, &key, action, number);
        Ok(relay.handle(LOCAL_CONN, make(challenge, sig), now, &mut os_random))
    }
}

fn refused(r: Refusal) -> ClientError {
    ClientError::Refused(r)
}

impl Post for LocalPost {
    fn make_mailbox(&mut self, reader: &Identity) -> Result<(), ClientError> {
        let who = reader.device_id();
        match self.signed(reader, Action::Make, 0, |challenge, sig| {
            Request::MakeMailbox {
                reader: who,
                challenge,
                sig,
            }
        })? {
            Response::Made => Ok(()),
            Response::Refused(r) => Err(refused(r)),
            _ => Err(ClientError::Unexpected),
        }
    }

    fn put(&mut self, sealed: &[u8]) -> Result<u64, ClientError> {
        match self.ask(Request::Put {
            sealed: sealed.to_vec(),
        })? {
            Response::Stored { id } => Ok(id),
            Response::Refused(r) => Err(refused(r)),
            _ => Err(ClientError::Unexpected),
        }
    }

    fn fetch(&mut self, reader: &Identity, cursor: u64) -> Result<Fetched, ClientError> {
        let who = reader.device_id();
        match self.signed(reader, Action::Fetch, cursor, |challenge, sig| {
            Request::Fetch {
                reader: who,
                challenge,
                sig,
                cursor,
            }
        })? {
            Response::Messages { items, more } => Ok(Fetched { items, more }),
            Response::Refused(r) => Err(refused(r)),
            _ => Err(ClientError::Unexpected),
        }
    }

    fn ack(&mut self, reader: &Identity, up_to: u64) -> Result<u32, ClientError> {
        let who = reader.device_id();
        match self.signed(reader, Action::Ack, up_to, |challenge, sig| Request::Ack {
            reader: who,
            challenge,
            sig,
            up_to,
        })? {
            Response::Acked { dropped } => Ok(dropped),
            Response::Refused(r) => Err(refused(r)),
            _ => Err(ClientError::Unexpected),
        }
    }

    fn close(&mut self) {
        lock(&self.relay).close(LOCAL_CONN);
    }
}

/// Make the relay's member list and its claim what the core says (BX-03,
/// BX-12, BX-22). The list is stored first by the relay itself (LK-21).
fn sync_relay(core: &Core, relay: &Relays, clock: &dyn Clock, notes: &Notes) {
    let now = clock.now();
    let (members, claim) = {
        let mut core = lock(core);
        (core.members(), core.claim(now))
    };
    let mut relay = lock(relay);
    let held: Vec<DeviceId> = relay.members().copied().collect();
    if held != members {
        if let Err(e) = relay.set_members(&members, now) {
            note(notes, format!("the member list was not replaced: {e}"));
        }
    }
    relay.set_claim(claim);
}

/// BX-12: give the core an invitation with a new nonce and offer its claim
/// at once.
fn enter_into(
    core: &Core,
    relay: &Relays,
    clock: &dyn Clock,
    notes: &Notes,
    text: &str,
) -> Result<(), BoxError> {
    let mut nonce = [0u8; 32];
    os_random(&mut nonce).map_err(|_| BoxError::NoRandomness)?;
    let now = clock.now();
    lock(core).enter_invitation(text, now, nonce)?;
    sync_relay(core, relay, clock, notes);
    Ok(())
}

/// The Box as the status page reads it (BX-55 to BX-57). It holds the
/// state's locks only for the moment of one call.
struct PageHandles {
    core: Core,
    relay: Relays,
    clock: Arc<dyn Clock + Send + Sync>,
    notes: Notes,
    relay_port: u16,
}

impl PageBackend for PageHandles {
    fn view(&self) -> PageView {
        let (status, shop, devices) = {
            let mut core = lock(&self.core);
            let status = core.status(self.clock.now());
            let shop = core.roots().map(|r| r.owner.fingerprint_words());
            (status, shop, core.members().len())
        };
        // Only the words of the last note: its detail may carry text from
        // the system or a vendor.
        let last_note = lock(&self.notes).back().map(|line| {
            let mut words = note_words(line);
            words.detail = None;
            words.quoted = None;
            words
        });
        PageView {
            status,
            version: option_env!("CARGO_PKG_VERSION").unwrap_or("unknown"),
            relay_port: self.relay_port,
            devices,
            shop,
            last_note,
        }
    }

    fn enter_invitation(&self, text: &str) -> Entered {
        // Looked at and entered under one lock, so that two posts cannot
        // both find the Box waiting.
        let mut nonce = [0u8; 32];
        if os_random(&mut nonce).is_err() {
            return Entered::Refused(BoxError::NoRandomness);
        }
        let now = self.clock.now();
        let result = {
            let mut core = lock(&self.core);
            match core.status(now) {
                BoxStatus::Unpaired => core.enter_invitation(text, now, nonce),
                BoxStatus::InvitationOpen => return Entered::Held,
                BoxStatus::Paired | BoxStatus::TurnedOff => Err(BoxError::AlreadyPaired),
            }
        };
        match result {
            Ok(()) => {
                sync_relay(&self.core, &self.relay, &*self.clock, &self.notes);
                Entered::Taken
            }
            Err(e) => Entered::Refused(e),
        }
    }
}

/// The Box as the till listener sees it (BX-70 to BX-75). It holds the
/// state's locks only for the moment of one call, and never the core's and
/// the table's together.
struct TillHandles {
    core: Core,
    clock: Arc<dyn Clock + Send + Sync>,
    table: Till,
}

impl TillHandles {
    /// The counters and the money of the settings in force, or why the Box
    /// cannot take a sale (BX-72).
    fn shop(&self) -> Result<(Vec<DeviceId>, openbmt_core::currency::ShopCurrency), TillRefusal> {
        let mut core = lock(&self.core);
        if core.status(self.clock.now()) != BoxStatus::Paired {
            return Err(TillRefusal::NotPaired);
        }
        let settings = core.settings().ok_or(TillRefusal::NotPaired)?;
        Ok((
            settings.counters.keys().copied().collect(),
            settings.currency,
        ))
    }
}

impl TillBackend for TillHandles {
    fn check_key(&self, presented: Option<&str>) -> KeyCheck {
        let stored = lock(&self.core).files().read(TILL_KEY_FILE);
        match stored {
            Ok(Some(stored)) => check_till_key(Some(&stored), presented),
            _ => KeyCheck::Wrong(None),
        }
    }

    fn create(&self, body: CreateBody) -> Result<([u8; 16], SaleInfo), TillRefusal> {
        let (counters, money) = self.shop()?;
        let counter = find_counter(counters, &body.counter)
            .ok_or_else(|| TillRefusal::UnknownCounter(body.counter.clone()))?;
        if body.currency != money.code() {
            return Err(TillRefusal::WrongMoney {
                shop: money.code().to_string(),
                asked: body.currency,
            });
        }
        let amount =
            parse_amount(&body.amount, money.minor_digits()).ok_or(TillRefusal::BadAmount)?;
        let mut random = [0u8; 32];
        os_random(&mut random).map_err(|_| TillRefusal::Fault)?;
        let mut id = [0u8; 16];
        id.copy_from_slice(random.get(..16).ok_or(TillRefusal::Fault)?);
        let new = NewSale {
            counter,
            amount,
            currency: openbmt_counter::Currency::of_shop(money),
            reference: clean_reference(&body.reference),
            request: body.request,
            digits: money.minor_digits(),
        };
        let mut table = lock(&self.table);
        let (id, _) = table.create(self.clock.now(), id, new)?;
        let info = table.info(&id).ok_or(TillRefusal::Fault)?;
        Ok((id, info))
    }

    fn read(&self, sale: &[u8; 16]) -> Result<SaleInfo, TillRefusal> {
        self.shop()?;
        let mut table = lock(&self.table);
        table
            .state(self.clock.now(), sale)
            .and_then(|_| table.info(sale))
            .ok_or(TillRefusal::Lost)
    }

    fn cancel(&self, sale: &[u8; 16]) -> Result<SaleInfo, TillRefusal> {
        self.shop()?;
        let mut table = lock(&self.table);
        table
            .cancel(self.clock.now(), sale)
            .and_then(|_| table.info(sale))
            .ok_or(TillRefusal::Lost)
    }
}

/// What the worker thread owns.
struct Worker {
    core: Core,
    relay: Relays,
    clock: Arc<dyn Clock + Send + Sync>,
    notes: Notes,
    dir: PathBuf,
    courier: Option<Courier<LocalPost>>,
    pos: Option<BoxPos<DirFiles>>,
    view: View,
    send_error: Option<String>,
    till: Till,
    till_error: Option<String>,
}

impl Worker {
    fn peers(&self) -> BTreeMap<DeviceId, Role> {
        lock(&self.core)
            .roles()
            .into_iter()
            .map(|(d, r)| (d, link_role(r)))
            .collect()
    }

    fn make_courier(&self) -> Result<Courier<LocalPost>, StartError> {
        let identity = lock(&self.core)
            .identity()
            .map_err(|_| StartError::Corrupt {
                file: crate::state::KEY_FILE,
            })?;
        let post = LocalPost {
            relay: Arc::clone(&self.relay),
            clock: Arc::clone(&self.clock),
        };
        Courier::start_with(
            post,
            identity,
            Role::Box,
            self.peers(),
            &self.dir,
            FirstStart::No,
        )
    }

    /// One step: bring the relay in line with the core, then read the
    /// Box's own mailbox.
    fn step(&mut self) {
        sync_relay(&self.core, &self.relay, &*self.clock, &self.notes);
        let working = {
            let mut core = lock(&self.core);
            matches!(core.status(self.clock.now()), BoxStatus::Paired)
        };
        if !working {
            self.courier = None;
            self.pos_stop();
            return;
        }
        if self.courier.is_none() {
            match self.make_courier() {
                Ok(c) => self.courier = Some(c),
                Err(e) => {
                    note(&self.notes, format!("the Box's link files: {e}"));
                    return;
                }
            }
        }
        let peers = self.peers();
        let Some(courier) = self.courier.as_mut() else {
            return;
        };
        if *courier.peers() != peers {
            let gone: Vec<DeviceId> = courier
                .peers()
                .keys()
                .filter(|d| !peers.contains_key(d))
                .copied()
                .collect();
            for d in gone {
                courier.remove_peer(&d);
            }
            for (d, r) in &peers {
                courier.set_peer(*d, *r);
            }
        }
        let received = match courier.poll() {
            Ok(r) => r,
            Err(e) => {
                note(&self.notes, format!("{e}"));
                return;
            }
        };
        for line in courier.take_notes() {
            note(&self.notes, line);
        }
        let mut changed = false;
        let mut paid_notices = Vec::new();
        for r in received {
            if r.kind == Kind::PaidNotice {
                paid_notices.push((r.from, r.content));
            } else {
                changed |= self.handle(r);
            }
        }
        if changed {
            sync_relay(&self.core, &self.relay, &*self.clock, &self.notes);
        }
        self.till_step(&paid_notices);
        self.pos_step(paid_notices);
    }

    /// BX-76, BX-78: move the till's sales on what the counters' notices
    /// say, let the time pass, and send the counters what the table has for
    /// them. A message the relay did not take stays for the next step.
    fn till_step(&mut self, notices: &[(DeviceId, Vec<u8>)]) {
        let now = self.clock.now();
        let roles = lock(&self.core).roles();
        let mut lines = Vec::new();
        let out = {
            let mut table = lock(&self.till);
            for (sender, bytes) in notices {
                if let Err(line) = table.take_notice(now, sender, bytes, &roles) {
                    lines.push(line);
                }
            }
            table.sweep(now);
            table.outgoing()
        };
        let mut done = Vec::new();
        if let Some(courier) = self.courier.as_mut() {
            for o in &out {
                match courier.send(&o.counter, Kind::PosNotice, &o.message.to_bytes()) {
                    Ok(()) => {
                        self.till_error = None;
                        done.push(o.seq);
                    }
                    Err(e) => {
                        let line =
                            format!("BX-76: a till sale could not be sent to a counter: {e}");
                        if self.till_error.as_deref() != Some(line.as_str()) {
                            self.till_error = Some(line.clone());
                            lines.push(line);
                        }
                        // A counter that is not on the list, or a message
                        // that cannot be made, will not work later either.
                        if e.is_retryable() {
                            break;
                        }
                        done.push(o.seq);
                    }
                }
            }
        }
        if !done.is_empty() {
            lock(&self.till).sent(&done);
        }
        for line in lines {
            note(&self.notes, line);
        }
    }

    /// The Box is unpaired or turned off: the POS side stops (BX-03, BX-23).
    fn pos_stop(&mut self) {
        if let Some(pos) = self.pos.as_mut() {
            pos.stop();
            *lock(&self.view) = PosView::State(Box::new(pos.state()));
        }
    }

    /// BX-40 to BX-43: hand the paid notices to the POS side, run its timer,
    /// and send the owner what it found.
    fn pos_step(&mut self, notices: Vec<(DeviceId, Vec<u8>)>) {
        let Some(pos) = self.pos.as_mut() else {
            if !notices.is_empty() {
                note(
                    &self.notes,
                    format!(
                        "{} paid notices arrived and this Box has no POS: dropped",
                        notices.len()
                    ),
                );
            }
            return;
        };
        let now = self.clock.now();
        let (roles, token, owner, tor) = {
            let core = lock(&self.core);
            (
                core.roles(),
                core.read_pos_token().map_err(|e| e.to_string()),
                core.roots().map(|r| r.owner),
                // A Box that cannot tell goes through Tor (BX-47).
                core.settings().is_none_or(|s| s.tor_on),
            )
        };
        pos.set_tor(tor);
        let mut lines = Vec::new();
        lines.extend(pos.follow_counters(&roles));
        if !notices.is_empty() {
            lines.extend(pos.take_notices(&notices, &roles, now));
        }
        lines.extend(pos.tick(now, token));
        if let (Some(owner), Some(courier)) = (owner, self.courier.as_mut()) {
            for report in pos.pending_reports() {
                match courier.send(&owner, Kind::PosNotice, &report.to_bytes()) {
                    Ok(()) => {
                        self.send_error = None;
                        if let Err(e) = pos.reports_sent(report.number) {
                            lines.push(format!("BX-50: the reports were not stored: {e}"));
                            break;
                        }
                    }
                    Err(e) => {
                        let line = format!("a POS notice could not be sent to the owner: {e}");
                        if self.send_error.as_deref() != Some(line.as_str()) {
                            self.send_error = Some(line.clone());
                            lines.push(line);
                        }
                        break;
                    }
                }
            }
        }
        *lock(&self.view) = PosView::State(Box::new(pos.state()));
        for line in lines {
            note(&self.notes, line);
        }
    }

    /// Handle one opened message; whether the settings changed.
    fn handle(&self, r: Received) -> bool {
        if r.kind != Kind::Settings {
            note(
                &self.notes,
                format!("a message of kind {:?} is not handled yet", r.kind),
            );
            return false;
        }
        let result = lock(&self.core).take_settings(&r.content);
        match result {
            Ok(taken) => {
                if taken.turned_off {
                    note(
                        &self.notes,
                        "BX-23: the settings no longer list this Box: it is turned off".into(),
                    );
                }
                if let Some(kind) = taken.token_error {
                    note(
                        &self.notes,
                        format!("BX-23: the POS token could not be erased: {kind}"),
                    );
                }
                true
            }
            Err(e) => {
                note(&self.notes, format!("settings from the owner refused: {e}"));
                false
            }
        }
    }
}

/// A Box that is running: its state, its relay on a port, and the thread
/// that reads its own mailbox.
pub struct BoxRunner {
    core: Core,
    relay: Relays,
    clock: Arc<dyn Clock + Send + Sync>,
    notes: Notes,
    view: View,
    till: Till,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
    server: Option<Server>,
    page: Option<StatusPage>,
    till_listener: Option<TillListener>,
    addr: SocketAddr,
    id: DeviceId,
    _dir_lock: DirLock,
}

impl BoxRunner {
    /// Take the data directory (BX-02), load the state (BX-51), make the
    /// relay and serve it on `bind`. When the data directory holds a
    /// `pos-config` (BX-49) the Box reads that vendor's POS over curl; a
    /// config that does not read leaves the POS side off and says why.
    pub fn start(
        dir: impl Into<PathBuf>,
        bind: SocketAddr,
        clock: Arc<dyn Clock + Send + Sync>,
        cfg: ServerConfig,
    ) -> Result<Self, StartFailure> {
        let dir: PathBuf = dir.into();
        let pos = vendor_setup(&dir);
        Self::start_inner(dir, bind, clock, cfg, pos)
    }

    /// [`BoxRunner::start`] with a POS connector (BX-40 to BX-43). With
    /// `None` the Box takes no part in the POS. A POS side whose files do
    /// not read back is left off and noted; the relay runs on.
    pub fn start_with(
        dir: impl Into<PathBuf>,
        bind: SocketAddr,
        clock: Arc<dyn Clock + Send + Sync>,
        cfg: ServerConfig,
        pos: Option<PosSetup>,
    ) -> Result<Self, StartFailure> {
        Self::start_inner(dir.into(), bind, clock, cfg, Ok(pos))
    }

    fn start_inner(
        dir: PathBuf,
        bind: SocketAddr,
        clock: Arc<dyn Clock + Send + Sync>,
        cfg: ServerConfig,
        pos: Result<Option<PosSetup>, PosOpenError>,
    ) -> Result<Self, StartFailure> {
        let dir_lock = DirLock::acquire(&dir).map_err(StartFailure::Lock)?;
        let core =
            BoxCore::open(DirFiles::new(&dir), &mut os_random).map_err(StartFailure::State)?;
        let id = core.device_id();
        let identity = core.identity().map_err(StartFailure::State)?;
        let store =
            DirStore::new(dir.join(RELAY_DIR)).map_err(|e| StartFailure::Relay(e.to_string()))?;
        let mut relay = Relay::new(identity, RelayConfig::default(), store)
            .map_err(|e| StartFailure::Relay(e.to_string()))?;
        relay.set_quiet_without_members(true);
        let core: Core = Arc::new(Mutex::new(core));
        {
            let core = Arc::clone(&core);
            relay.set_welcome_hook(Some(Box::new(move |welcome, settings, now| {
                let mut core = lock(&core);
                match core.take_welcome(welcome, settings, now) {
                    Ok(()) => WelcomeAnswer::Welcomed(core.members()),
                    // A welcome that did not spend the invitation leaves the
                    // claim on offer.
                    Err(_) if core.invitation_open(now) => WelcomeAnswer::Refused,
                    Err(_) => WelcomeAnswer::Withdrawn,
                }
            })));
        }
        let relay: Relays = Arc::new(Mutex::new(relay));
        let notes: Notes = Arc::new(Mutex::new(VecDeque::new()));
        let erased = {
            let mut core = lock(&core);
            if core.turned_off() {
                core.erase_pos_token().err()
            } else {
                None
            }
        };
        if let Some(e) = erased {
            note(
                &notes,
                format!("BX-23: the POS token could not be erased: {e}"),
            );
        }
        sync_relay(&core, &relay, &*clock, &notes);

        let (pos_side, view) = match pos {
            Err(e) => {
                note(&notes, format!("the POS side is off: {e}"));
                (None, PosView::Disabled(e))
            }
            Ok(None) => (None, PosView::NotInstalled),
            Ok(Some(setup)) => {
                let identity = lock(&core).identity().map_err(StartFailure::State)?;
                match BoxPos::open(DirFiles::new(&dir), identity, setup) {
                    Ok(p) => {
                        let state = PosView::State(Box::new(p.state()));
                        (Some(p), state)
                    }
                    Err(e) => {
                        note(&notes, format!("the POS side is off: {e}"));
                        (None, PosView::Disabled(e))
                    }
                }
            }
        };
        let view: View = Arc::new(Mutex::new(view));
        let till: Till = Arc::new(Mutex::new(TillTable::new()));
        let mut worker = Worker {
            core: Arc::clone(&core),
            relay: Arc::clone(&relay),
            clock: Arc::clone(&clock),
            notes: Arc::clone(&notes),
            dir: dir.clone(),
            courier: None,
            pos: pos_side,
            view: Arc::clone(&view),
            send_error: None,
            till: Arc::clone(&till),
            till_error: None,
        };
        // A paired Box whose number files are gone stops here (LK-40).
        let paired = matches!(lock(&core).status(clock.now()), BoxStatus::Paired);
        if paired {
            worker.courier = Some(worker.make_courier().map_err(StartFailure::Courier)?);
        }

        let listener = TcpListener::bind(bind).map_err(StartFailure::Listen)?;
        let server = Server::start_shared(listener, Arc::clone(&relay), Arc::clone(&clock), cfg)
            .map_err(StartFailure::Listen)?;
        let addr = server.local_addr();
        let stop = Arc::new(AtomicBool::new(false));
        let handle = {
            let stop = Arc::clone(&stop);
            thread::Builder::new()
                .name("box-worker".into())
                .spawn(move || {
                    while !stop.load(Ordering::SeqCst) {
                        worker.step();
                        thread::sleep(STEP_PAUSE);
                    }
                })
                .map_err(StartFailure::Listen)?
        };
        Ok(Self {
            core,
            relay,
            clock,
            notes,
            view,
            till,
            stop,
            worker: Some(handle),
            server: Some(server),
            page: None,
            till_listener: None,
            addr,
            id,
            _dir_lock: dir_lock,
        })
    }

    /// The address the relay listens on.
    pub fn local_addr(&self) -> SocketAddr {
        self.addr
    }

    /// The Box's key, which is also the relay's (BX-01).
    pub fn device_id(&self) -> DeviceId {
        self.id
    }

    /// What the Box is doing now.
    pub fn status(&self) -> BoxStatus {
        lock(&self.core).status(self.clock.now())
    }

    /// The relay's member list now (BX-22).
    pub fn members(&self) -> Vec<DeviceId> {
        lock(&self.relay).members().copied().collect()
    }

    /// Take the invitation a person typed (BX-12) and offer its claim at
    /// once. The nonce is new.
    pub fn enter_invitation(&self, text: &str) -> Result<(), BoxError> {
        enter_into(&self.core, &self.relay, &*self.clock, &self.notes, text)
    }

    /// Serve the status page (BX-54) on `bind`; the address it listens on.
    /// A second call replaces the first page.
    pub fn serve_status_page(
        &mut self,
        bind: SocketAddr,
        cfg: PageConfig,
    ) -> Result<SocketAddr, StartFailure> {
        let listener = TcpListener::bind(bind).map_err(StartFailure::Listen)?;
        let backend = Arc::new(PageHandles {
            core: Arc::clone(&self.core),
            relay: Arc::clone(&self.relay),
            clock: Arc::clone(&self.clock),
            notes: Arc::clone(&self.notes),
            relay_port: self.addr.port(),
        });
        let page = StatusPage::start(listener, backend, cfg).map_err(StartFailure::Listen)?;
        let addr = page.local_addr();
        self.page = Some(page);
        Ok(addr)
    }

    /// Serve the till listener (BX-70) on `bind`; the address it listens on.
    /// A second call replaces the first listener.
    pub fn serve_till(
        &mut self,
        bind: SocketAddr,
        cfg: TillConfig,
    ) -> Result<SocketAddr, StartFailure> {
        let listener = TcpListener::bind(bind).map_err(StartFailure::Listen)?;
        let backend = Arc::new(TillHandles {
            core: Arc::clone(&self.core),
            clock: Arc::clone(&self.clock),
            table: Arc::clone(&self.till),
        });
        let till = TillListener::start(listener, backend, cfg).map_err(StartFailure::Listen)?;
        let addr = till.local_addr();
        self.till_listener = Some(till);
        Ok(addr)
    }

    /// BX-71: make a new till key and keep only its hash. The key is
    /// returned as 64 lowercase hex and is not kept anywhere. The listener
    /// reads the hash at each request, so the new key works at once and the
    /// old one stops.
    pub fn make_till_key(&self) -> Result<String, BoxError> {
        make_till_key(lock(&self.core).files_mut())
    }

    /// Store the POS token the merchant typed (BX-41). Only a paired Box that
    /// the settings list takes one.
    pub fn store_pos_token(&self, token: &str) -> Result<(), BoxError> {
        lock(&self.core).store_pos_token(token)
    }

    /// BX-49: take the line `pos-setup` was given, keep it as the Box's POS
    /// vendor and say when it takes effect: at the next start.
    pub fn store_pos_config(&self, line: &str) -> Result<PosConfig, PosConfigError> {
        let config = PosConfig::parse(line)?;
        lock(&self.core)
            .files_mut()
            .write(POS_CONFIG_FILE, &config.to_bytes())
            .map_err(|e| PosConfigError::Io(e.kind()))?;
        Ok(config)
    }

    /// The POS side as the worker last saw it, without the token (BX-41).
    pub fn pos_view(&self) -> PosView {
        lock(&self.view).clone()
    }

    /// The notes the Box kept, oldest first.
    pub fn notes(&self) -> Vec<String> {
        lock(&self.notes).iter().cloned().collect()
    }

    /// The state, for a test.
    pub fn core(&self) -> &Mutex<BoxCore<DirFiles>> {
        &self.core
    }

    /// The Box's four fingerprint words (BX-14).
    pub fn fingerprint_words(&self) -> [&'static str; 4] {
        self.id.fingerprint_words()
    }

    /// Stop the worker and the server.
    pub fn stop(mut self) {
        self.shutdown();
    }

    fn shutdown(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(handle) = self.worker.take() {
            let _ = handle.join();
        }
        if let Some(page) = self.page.take() {
            page.stop();
        }
        if let Some(till) = self.till_listener.take() {
            till.stop();
        }
        if let Some(server) = self.server.take() {
            server.stop();
        }
    }
}

impl Drop for BoxRunner {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// BX-49: the POS side the data directory's `pos-config` asks for: none when
/// there is no file, the vendor's reader over curl when there is one.
fn vendor_setup(dir: &Path) -> Result<Option<PosSetup>, PosOpenError> {
    let bytes = match DirFiles::new(dir).read(POS_CONFIG_FILE) {
        Ok(Some(bytes)) => bytes,
        Ok(None) => return Ok(None),
        Err(e) => {
            return Err(PosOpenError::Io {
                file: POS_CONFIG_FILE,
                kind: e.kind(),
            })
        }
    };
    let config = PosConfig::from_bytes(&bytes).map_err(PosOpenError::Config)?;
    let transport = Arc::new(Transport::new());
    let connector = VendorConnector::new(
        &config,
        Box::new(CurlRunner::new(config.curl_path())),
        Arc::clone(&transport),
    )
    .map_err(PosOpenError::Config)?;
    let settings = config.settings(Vec::new()).map_err(PosOpenError::Config)?;
    let mut setup = PosSetup::new(settings, Box::new(connector));
    setup.interval_secs = VENDOR_POLL_SECS;
    setup.min_gap_secs = VENDOR_POLL_SECS;
    setup.counters_from_list = true;
    setup.transport = Some(transport);
    Ok(Some(setup))
}

/// Read an invitation from a file once and delete the file (BX-12). The
/// first line, trimmed, is the text.
pub fn read_invitation_file(path: &Path) -> io::Result<String> {
    let text = fs::read_to_string(path)?;
    fs::remove_file(path)?;
    Ok(invitation_text(&text))
}

/// The invitation text in the contents of an invitation file: the first
/// line, trimmed (BX-12).
pub fn invitation_text(contents: &str) -> String {
    contents.lines().next().unwrap_or("").trim().to_string()
}
