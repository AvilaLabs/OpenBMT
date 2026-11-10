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
//! [`BoxCore::take_settings`]; any other kind is noted as not handled yet and
//! dropped.
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

use crate::files::DirFiles;
use crate::state::{BoxCore, BoxError, BoxStatus};

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
        if let Err(e) = relay.set_members(&members) {
            note(notes, format!("the member list was not replaced: {e}"));
        }
    }
    relay.set_claim(claim);
}

/// What the worker thread owns.
struct Worker {
    core: Core,
    relay: Relays,
    clock: Arc<dyn Clock + Send + Sync>,
    notes: Notes,
    dir: PathBuf,
    courier: Option<Courier<LocalPost>>,
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
        for r in received {
            changed |= self.handle(r);
        }
        if changed {
            sync_relay(&self.core, &self.relay, &*self.clock, &self.notes);
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
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
    server: Option<Server>,
    addr: SocketAddr,
    id: DeviceId,
    _dir_lock: DirLock,
}

impl BoxRunner {
    /// Take the data directory (BX-02), load the state (BX-51), make the
    /// relay and serve it on `bind`.
    pub fn start(
        dir: impl Into<PathBuf>,
        bind: SocketAddr,
        clock: Arc<dyn Clock + Send + Sync>,
        cfg: ServerConfig,
    ) -> Result<Self, StartFailure> {
        let dir: PathBuf = dir.into();
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

        let mut worker = Worker {
            core: Arc::clone(&core),
            relay: Arc::clone(&relay),
            clock: Arc::clone(&clock),
            notes: Arc::clone(&notes),
            dir: dir.clone(),
            courier: None,
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
            stop,
            worker: Some(handle),
            server: Some(server),
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
        let mut nonce = [0u8; 32];
        os_random(&mut nonce).map_err(|_| BoxError::NoRandomness)?;
        let now = self.clock.now();
        lock(&self.core).enter_invitation(text, now, nonce)?;
        sync_relay(&self.core, &self.relay, &*self.clock, &self.notes);
        Ok(())
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

/// Read an invitation from a file once and delete the file (BX-12). The
/// first line, trimmed, is the text.
pub fn read_invitation_file(path: &Path) -> io::Result<String> {
    let text = fs::read_to_string(path)?;
    fs::remove_file(path)?;
    Ok(text.lines().next().unwrap_or("").trim().to_string())
}
