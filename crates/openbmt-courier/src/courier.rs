//! The courier (LK-50 to LK-53): numbering, sealing, sending, polling,
//! opening and acknowledging, for one device.
//!
//! A [`Courier`] holds the device's identity and role, a table of the devices
//! it talks to, the two number tables of LK-40 on disk, and a [`Post`], which
//! is how it reaches the relay. The real post, [`RelayPost`], opens an
//! `openbmt_link::Client` connection when work starts and drops it when the
//! batch of work is over; tests use a post made on a relay in memory.
//!
//! Where the code is more exact than the rules:
//!
//! - Each poll starts from the start of the mailbox, and a message that was
//!   taken or refused is acknowledged, so a mailbox that works holds only
//!   what has not been fetched. That makes a cursor kept between polls
//!   needless, and wrong if the relay ever starts a mailbox's numbers again.
//! - At most [`MAX_ROUNDS`] fetches are made in one poll.
//! - Failures after something was taken (a second fetch, the acknowledgement)
//!   do not lose what was taken: the poll returns it and notes the failure.
//!   What was not acknowledged comes again and meets LK-06.

use std::collections::{BTreeMap, VecDeque};
use std::fmt;
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use openbmt_core::identity::{DeviceId, Identity};
use openbmt_link::seal::step_for;
use openbmt_link::{
    os_random, seal, Client, ClientConfig, ClientError, Fetched, Inbox, Kind, NextError, Outbox,
    Role, Route, SealError, TakeError,
};

/// The file of the last number taken from each sender (LK-40).
pub const INBOX_FILE: &str = "link-inbox";
/// The file of the last number sent to each reader (LK-40).
pub const OUTBOX_FILE: &str = "link-outbox";
/// Most lines the courier's notes keep; the oldest goes first.
pub const NOTE_LOG: usize = 64;
/// Most fetches one poll makes (64 messages each).
pub const MAX_ROUNDS: usize = 16;

/// How the courier reaches the relay. Each call is one request; the courier
/// calls [`Post::close`] when a batch of work is over.
pub trait Post {
    /// Make the reader's mailbox if it has none (LK-21).
    fn make_mailbox(&mut self, reader: &Identity) -> Result<(), ClientError>;
    /// Hand the relay a sealed message (LK-20).
    fn put(&mut self, sealed: &[u8]) -> Result<u64, ClientError>;
    /// Fetch what is after `cursor` (LK-24).
    fn fetch(&mut self, reader: &Identity, cursor: u64) -> Result<Fetched, ClientError>;
    /// Say that everything up to `up_to` is taken (LK-24).
    fn ack(&mut self, reader: &Identity, up_to: u64) -> Result<u32, ClientError>;
    /// End the batch: drop whatever connection is open.
    fn close(&mut self);
}

/// The real post: a connection to the relay made when the first call of a
/// batch needs it, and dropped by [`Post::close`] or by any failure.
#[derive(Debug)]
pub struct RelayPost {
    route: Route,
    relay: DeviceId,
    cfg: ClientConfig,
    conn: Option<Client>,
}

impl RelayPost {
    /// A post for the relay at `route` whose key is `relay` (LK-32).
    pub fn new(route: Route, relay: DeviceId, cfg: ClientConfig) -> Self {
        Self {
            route,
            relay,
            cfg,
            conn: None,
        }
    }

    fn with<T>(
        &mut self,
        work: impl FnOnce(&mut Client) -> Result<T, ClientError>,
    ) -> Result<T, ClientError> {
        if self.conn.is_none() {
            self.conn = Some(Client::connect(&self.route, Some(&self.relay), &self.cfg)?);
        }
        let result = match self.conn.as_mut() {
            Some(client) => work(client),
            None => Err(ClientError::Unexpected),
        };
        if result.is_err() {
            self.conn = None;
        }
        result
    }
}

impl Post for RelayPost {
    fn make_mailbox(&mut self, reader: &Identity) -> Result<(), ClientError> {
        self.with(|c| c.make_mailbox(reader))
    }

    fn put(&mut self, sealed: &[u8]) -> Result<u64, ClientError> {
        self.with(|c| c.put(sealed))
    }

    fn fetch(&mut self, reader: &Identity, cursor: u64) -> Result<Fetched, ClientError> {
        self.with(|c| c.fetch(reader, cursor))
    }

    fn ack(&mut self, reader: &Identity, up_to: u64) -> Result<u32, ClientError> {
        self.with(|c| c.ack(reader, up_to))
    }

    fn close(&mut self) {
        self.conn = None;
    }
}

/// Whether the device has run before (LK-53).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FirstStart {
    /// The first start: missing number files are made empty.
    Yes,
    /// Any other start: a missing number file means a new pairing is needed.
    No,
}

/// A file operation that failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreError {
    /// The file.
    pub file: &'static str,
    /// What was being done.
    pub op: &'static str,
    /// What the system said.
    pub kind: io::ErrorKind,
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "LK-40: could not {} {}: {}",
            self.op, self.file, self.kind
        )
    }
}

impl std::error::Error for StoreError {}

/// Why the courier did not start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartError {
    /// A number file is missing and this is not the first start.
    NeedsNewPairing {
        /// The file.
        file: &'static str,
    },
    /// A number file does not read back.
    Corrupt {
        /// The file.
        file: &'static str,
    },
    /// A file could not be read or written.
    Store(StoreError),
}

impl fmt::Display for StartError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NeedsNewPairing { file } => write!(
                f,
                "LK-53: {file} is missing and this is not the first start: \
                 the device needs a new pairing (LK-40)"
            ),
            Self::Corrupt { file } => write!(
                f,
                "LK-53: {file} does not read back: the device needs a new pairing (LK-40)"
            ),
            Self::Store(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for StartError {}

/// Why a message was not sent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SendError {
    /// The reader is not in the table (LK-50).
    UnknownPeer,
    /// The roles may not send this kind (LK-08).
    KindNotAllowed,
    /// The content does not fit a message (LK-07).
    TooLarge,
    /// No number could be taken and stored (LK-40).
    Number(NextError<StoreError>),
    /// The message could not be sealed.
    Seal(SealError),
    /// The relay could not be reached or refused. A new try is allowed and
    /// takes a new number (LK-51).
    Relay(ClientError),
}

impl SendError {
    /// Whether the caller may try again later.
    pub fn is_retryable(&self) -> bool {
        matches!(self, Self::Relay(_))
    }
}

impl fmt::Display for SendError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownPeer => write!(f, "LK-50: the reader is not a device this one talks to"),
            Self::KindNotAllowed => write!(f, "LK-08: this role may not send this kind there"),
            Self::TooLarge => write!(f, "LK-07: content is larger than one message holds"),
            Self::Number(e) => write!(f, "{e}"),
            Self::Seal(e) => write!(f, "{e}"),
            Self::Relay(e) => write!(f, "LK-51: the message did not reach the relay: {e}"),
        }
    }
}

impl std::error::Error for SendError {}

/// Why a poll gave nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PollError {
    /// The relay could not be reached or refused, before anything was taken.
    Relay(ClientError),
}

impl fmt::Display for PollError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Relay(e) => write!(f, "LK-52: the mailbox could not be read: {e}"),
        }
    }
}

impl std::error::Error for PollError {}

/// A message that opened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Received {
    /// The sender.
    pub from: DeviceId,
    /// The sender's role in the table.
    pub role: Role,
    /// What it is.
    pub kind: Kind,
    /// The content, without padding.
    pub content: Vec<u8>,
}

/// What a courier is started with when it reaches a real relay.
pub struct CourierConfig {
    /// The directory for the two number files. It must exist.
    pub dir: PathBuf,
    /// This device's identity.
    pub identity: Identity,
    /// This device's role.
    pub role: Role,
    /// The devices it talks to and their roles.
    pub peers: BTreeMap<DeviceId, Role>,
    /// How to reach the relay (LK-31).
    pub route: Route,
    /// The relay's key (LK-32).
    pub relay: DeviceId,
    /// The relay timeouts.
    pub client: ClientConfig,
}

/// Write a file whole: temporary file, flush, rename, flush the directory.
fn write_whole(dir: &Path, file: &'static str, bytes: &[u8]) -> Result<(), StoreError> {
    let fail = |op: &'static str| {
        move |e: io::Error| StoreError {
            file,
            op,
            kind: e.kind(),
        }
    };
    let temp = dir.join(format!("{file}.tmp"));
    openbmt_link::private::create_file(&temp)
        .and_then(|mut f| {
            f.write_all(bytes)?;
            f.sync_all()
        })
        .map_err(fail("write"))?;
    fs::rename(&temp, dir.join(file)).map_err(fail("replace"))?;
    File::open(dir)
        .and_then(|d| d.sync_all())
        .map_err(fail("flush"))
}

/// The bytes of a number file: `None` if it is not there.
fn read_file(dir: &Path, file: &'static str) -> Result<Option<Vec<u8>>, StartError> {
    match fs::read(dir.join(file)) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(StartError::Store(StoreError {
            file,
            op: "read",
            kind: e.kind(),
        })),
    }
}

/// One device's end of the link. See the module docs.
pub struct Courier<P: Post = RelayPost> {
    identity: Identity,
    role: Role,
    post: P,
    peers: BTreeMap<DeviceId, Role>,
    dir: PathBuf,
    inbox: Inbox,
    outbox: Outbox,
    mailbox_made: bool,
    cursor: u64,
    holding: bool,
    reachable: bool,
    refused: u64,
    notes: VecDeque<String>,
}

impl Courier<RelayPost> {
    /// A courier for a real relay.
    pub fn start(config: CourierConfig, first: FirstStart) -> Result<Self, StartError> {
        let CourierConfig {
            dir,
            identity,
            role,
            peers,
            route,
            relay,
            client,
        } = config;
        Self::start_with(
            RelayPost::new(route, relay, client),
            identity,
            role,
            peers,
            dir,
            first,
        )
    }
}

impl<P: Post> Courier<P> {
    /// A courier that reaches the relay through `post`. The two number files
    /// are read from `dir` (LK-53).
    pub fn start_with(
        post: P,
        identity: Identity,
        role: Role,
        peers: BTreeMap<DeviceId, Role>,
        dir: impl Into<PathBuf>,
        first: FirstStart,
    ) -> Result<Self, StartError> {
        let dir = dir.into();
        let inbox_bytes = read_file(&dir, INBOX_FILE)?;
        let outbox_bytes = read_file(&dir, OUTBOX_FILE)?;
        if first == FirstStart::No {
            if inbox_bytes.is_none() {
                return Err(StartError::NeedsNewPairing { file: INBOX_FILE });
            }
            if outbox_bytes.is_none() {
                return Err(StartError::NeedsNewPairing { file: OUTBOX_FILE });
            }
        }
        let inbox = match &inbox_bytes {
            Some(bytes) => {
                Inbox::from_bytes(bytes).map_err(|_| StartError::Corrupt { file: INBOX_FILE })?
            }
            None => Inbox::default(),
        };
        let outbox = match &outbox_bytes {
            Some(bytes) => {
                Outbox::from_bytes(bytes).map_err(|_| StartError::Corrupt { file: OUTBOX_FILE })?
            }
            None => Outbox::default(),
        };
        // Written now, so that a file that later goes missing is known for
        // what it is.
        if inbox_bytes.is_none() {
            write_whole(&dir, INBOX_FILE, &inbox.to_bytes()).map_err(StartError::Store)?;
        }
        if outbox_bytes.is_none() {
            write_whole(&dir, OUTBOX_FILE, &outbox.to_bytes()).map_err(StartError::Store)?;
        }
        Ok(Self {
            identity,
            role,
            post,
            peers,
            dir,
            inbox,
            outbox,
            mailbox_made: false,
            cursor: 0,
            holding: false,
            reachable: true,
            refused: 0,
            notes: VecDeque::new(),
        })
    }

    /// This device's key.
    pub fn device_id(&self) -> DeviceId {
        self.identity.device_id()
    }

    /// This device's role.
    pub fn role(&self) -> Role {
        self.role
    }

    /// The devices this one talks to.
    pub fn peers(&self) -> &BTreeMap<DeviceId, Role> {
        &self.peers
    }

    /// Add a device to the table, or change its role.
    pub fn set_peer(&mut self, id: DeviceId, role: Role) {
        self.peers.insert(id, role);
    }

    /// Take a device out of the table.
    pub fn remove_peer(&mut self, id: &DeviceId) {
        self.peers.remove(id);
    }

    /// The numbers taken.
    pub fn inbox(&self) -> &Inbox {
        &self.inbox
    }

    /// The numbers sent.
    pub fn outbox(&self) -> &Outbox {
        &self.outbox
    }

    /// The post, for tests and tools.
    pub fn post(&self) -> &P {
        &self.post
    }

    /// The last mailbox number of the latest poll that was taken or refused.
    pub fn cursor(&self) -> u64 {
        self.cursor
    }

    /// How many messages have been refused since the start (LK-52).
    pub fn refused(&self) -> u64 {
        self.refused
    }

    /// Whether the last call that went to the relay worked.
    pub fn reachable(&self) -> bool {
        self.reachable
    }

    /// The courier's lines, oldest first.
    pub fn notes(&self) -> impl Iterator<Item = &str> {
        self.notes.iter().map(String::as_str)
    }

    /// Take the courier's lines.
    pub fn take_notes(&mut self) -> Vec<String> {
        self.notes.drain(..).collect()
    }

    fn note(&mut self, line: String) {
        if self.notes.len() >= NOTE_LOG {
            self.notes.pop_front();
        }
        self.notes.push_back(line);
    }

    /// Start a batch of work: the connection, once made, stays until
    /// [`Courier::end`], so that several sends use one connection.
    pub fn begin(&mut self) {
        self.holding = true;
    }

    /// End a batch of work and drop the connection.
    pub fn end(&mut self) {
        self.holding = false;
        self.post.close();
    }

    fn finish_call(&mut self) {
        if !self.holding {
            self.post.close();
        }
    }

    /// A call to the relay failed: the connection goes, and a change from
    /// working to not working is noted once.
    fn failed(&mut self, e: ClientError) -> ClientError {
        self.post.close();
        if self.reachable {
            self.reachable = false;
            self.note(format!("a call to the relay failed: {e}"));
        }
        e
    }

    fn worked(&mut self) {
        if !self.reachable {
            self.reachable = true;
            self.note("the relay answers calls again".to_string());
        }
    }

    /// Send `content` as a message of `kind` to `to` (LK-51). The number is
    /// stored before the message is sealed, and the message is sealed before
    /// the relay sees anything. A failure to reach the relay may be tried
    /// again; the second try seals a new message with a new number.
    pub fn send(&mut self, to: &DeviceId, kind: Kind, content: &[u8]) -> Result<(), SendError> {
        let to_role = *self.peers.get(to).ok_or(SendError::UnknownPeer)?;
        if !kind.allowed(self.role, to_role) {
            return Err(SendError::KindNotAllowed);
        }
        if step_for(content.len()).is_none() {
            return Err(SendError::TooLarge);
        }
        let dir = &self.dir;
        let number = self
            .outbox
            .next(to, |bytes| write_whole(dir, OUTBOX_FILE, bytes))
            .map_err(SendError::Number)?;
        let sealed = seal(
            &self.identity,
            self.role,
            to,
            to_role,
            number,
            kind,
            content,
            &mut os_random,
        )
        .map_err(SendError::Seal)?;
        let put = self.post.put(&sealed);
        let result = match put {
            Ok(_) => {
                self.worked();
                Ok(())
            }
            Err(e) => Err(SendError::Relay(self.failed(e))),
        };
        self.finish_call();
        result
    }

    /// Fetch, open and acknowledge what is in the mailbox (LK-52). The
    /// messages that opened are returned in the order they came.
    pub fn poll(&mut self) -> Result<Vec<Received>, PollError> {
        let result = self.poll_inner();
        self.finish_call();
        result
    }

    fn poll_inner(&mut self) -> Result<Vec<Received>, PollError> {
        if !self.mailbox_made {
            if let Err(e) = self.post.make_mailbox(&self.identity) {
                return Err(PollError::Relay(self.failed(e)));
            }
            self.mailbox_made = true;
        }
        self.cursor = 0;
        let mut out = Vec::new();
        for round in 0..MAX_ROUNDS {
            let fetched = match self.post.fetch(&self.identity, self.cursor) {
                Ok(f) => f,
                Err(e) => {
                    let e = self.failed(e);
                    // Mailbox may have been lost with the relay's state.
                    self.mailbox_made = false;
                    if round == 0 {
                        return Err(PollError::Relay(e));
                    }
                    break;
                }
            };
            self.worked();
            let mut taken_through = None;
            let mut stopped = false;
            for (id, sealed) in fetched.items {
                let peers = &self.peers;
                let dir = &self.dir;
                let opened = self.inbox.open(
                    &self.identity,
                    self.role,
                    &sealed,
                    |d| peers.get(d).copied(),
                    |bytes| write_whole(dir, INBOX_FILE, bytes),
                );
                match opened {
                    Ok(o) => match self.peers.get(&o.sender).copied() {
                        Some(role) => out.push(Received {
                            from: o.sender,
                            role,
                            kind: o.kind,
                            content: o.content,
                        }),
                        None => self.refuse("LK-50: the sender left the table".to_string()),
                    },
                    Err(TakeError::Persist(e)) => {
                        // Not taken and not acknowledged: it comes again.
                        self.note(format!("message {id} left in the mailbox: {e}"));
                        stopped = true;
                        break;
                    }
                    Err(e) => self.refuse(format!("message {id} refused: {e}")),
                }
                self.cursor = id;
                taken_through = Some(id);
            }
            if let Some(up_to) = taken_through {
                if let Err(e) = self.post.ack(&self.identity, up_to) {
                    let e = self.failed(e);
                    self.note(format!("the mailbox was not acknowledged: {e}"));
                    break;
                }
            }
            if stopped || !fetched.more {
                break;
            }
        }
        Ok(out)
    }

    fn refuse(&mut self, line: String) {
        self.refused = self.refused.saturating_add(1);
        self.note(line);
    }
}
