//! A relay in memory and a [`Post`] on it, so that two couriers can talk in
//! a test without a socket. It is a thin cover over
//! `openbmt_link::Relay<MemoryStore>`: every call goes through the relay's own
//! `handle`, with its challenges, so the rules of section 2 apply as they do
//! on the wire. What it adds are switches for a test to pull: the relay can
//! be down, and it can take messages and lose them.

use std::io;
use std::sync::{Arc, Mutex, MutexGuard};

use openbmt_core::identity::{DeviceId, Identity};
use openbmt_link::proto::{Refusal, Request, Response};
use openbmt_link::relay::{sign_challenge, Action};
use openbmt_link::{os_random, ClientError, Fetched, MemoryStore, Relay, RelayConfig};

use crate::courier::Post;

struct Inner {
    relay: Relay<MemoryStore>,
    now: u64,
    down: bool,
    lose_puts: bool,
    next_conn: u64,
    puts: u64,
    calls: u64,
}

/// A relay held in memory, shared by the posts made from it.
#[derive(Clone)]
pub struct MemoryRelay {
    inner: Arc<Mutex<Inner>>,
}

fn broken() -> ClientError {
    ClientError::Io(io::ErrorKind::Other)
}

fn refused(r: Refusal) -> ClientError {
    ClientError::Refused(r)
}

impl MemoryRelay {
    /// A relay with this identity whose members are `members`.
    pub fn new(identity: Identity, members: &[DeviceId]) -> Result<Self, String> {
        let mut relay = Relay::new(identity, RelayConfig::default(), MemoryStore::new())
            .map_err(|e| e.to_string())?;
        relay.set_members(members).map_err(|e| e.to_string())?;
        Ok(Self {
            inner: Arc::new(Mutex::new(Inner {
                relay,
                now: 0,
                down: false,
                lose_puts: false,
                next_conn: 1,
                puts: 0,
                calls: 0,
            })),
        })
    }

    fn lock(&self) -> Result<MutexGuard<'_, Inner>, ClientError> {
        self.inner.lock().map_err(|_| broken())
    }

    /// The relay's key.
    pub fn device_id(&self) -> Option<DeviceId> {
        self.inner.lock().ok().map(|i| i.relay.device_id())
    }

    /// A new post: a connection of its own.
    pub fn post(&self) -> MemoryPost {
        let conn = match self.inner.lock() {
            Ok(mut i) => {
                let c = i.next_conn;
                i.next_conn = c.saturating_add(1);
                c
            }
            Err(_) => 0,
        };
        MemoryPost {
            relay: self.clone(),
            conn,
        }
    }

    /// Replace the member list, as whoever runs the relay does (LK-21).
    pub fn set_members(&self, members: &[DeviceId]) -> Result<(), String> {
        let mut inner = self
            .inner
            .lock()
            .map_err(|_| "the relay is broken".to_string())?;
        inner.relay.set_members(members).map_err(|e| e.to_string())
    }

    /// While down, every call fails as an unreachable relay does.
    pub fn set_down(&self, down: bool) {
        if let Ok(mut i) = self.inner.lock() {
            i.down = down;
        }
    }

    /// While on, a put is answered as stored and the message is dropped.
    pub fn set_lose_puts(&self, lose: bool) {
        if let Ok(mut i) = self.inner.lock() {
            i.lose_puts = lose;
        }
    }

    /// The relay's clock, in seconds.
    pub fn set_now(&self, now: u64) {
        if let Ok(mut i) = self.inner.lock() {
            i.now = now;
        }
    }

    /// How many puts the relay was asked for, lost or not.
    pub fn puts(&self) -> u64 {
        self.inner.lock().map_or(0, |i| i.puts)
    }

    /// How many calls the posts made, whether the relay was down or not.
    pub fn calls(&self) -> u64 {
        self.inner.lock().map_or(0, |i| i.calls)
    }

    /// The sealed messages held for `reader`, oldest first.
    pub fn held(&self, reader: &DeviceId) -> Vec<Vec<u8>> {
        match self.inner.lock() {
            Ok(i) => i
                .relay
                .mailbox(reader)
                .map(|m| m.messages().map(|(_, s)| s.to_vec()).collect())
                .unwrap_or_default(),
            Err(_) => Vec::new(),
        }
    }

    /// Put a sealed message in as a relay that has forgotten which numbers
    /// it took would (a restarted one, or one that lies), bypassing the down
    /// and lose switches. A test uses it to hand out old messages again.
    pub fn put_raw(&self, sealed: &[u8]) -> Result<u64, ClientError> {
        let mut i = self.lock()?;
        let now = i.now;
        i.relay.forget_taken();
        match i.relay.handle(
            0,
            Request::Put {
                sealed: sealed.to_vec(),
            },
            now,
            &mut os_random,
        ) {
            Response::Stored { id } => Ok(id),
            Response::Refused(r) => Err(refused(r)),
            _ => Err(ClientError::Unexpected),
        }
    }
}

/// A post on a [`MemoryRelay`].
pub struct MemoryPost {
    relay: MemoryRelay,
    conn: u64,
}

impl MemoryPost {
    /// One request that needs a challenge: ask for it, sign it, send it.
    fn signed(
        &mut self,
        reader: &Identity,
        action: Action,
        number: u64,
        build: impl FnOnce(&Identity, [u8; 32], openbmt_core::identity::Sig) -> Request,
    ) -> Result<Response, ClientError> {
        let mut i = self.relay.lock()?;
        i.calls = i.calls.saturating_add(1);
        if i.down {
            return Err(ClientError::Connect(io::ErrorKind::ConnectionRefused));
        }
        let now = i.now;
        let relay_id = i.relay.device_id();
        let challenge = match i
            .relay
            .handle(self.conn, Request::Challenge, now, &mut os_random)
        {
            Response::Challenge { challenge } => challenge,
            Response::Refused(r) => return Err(refused(r)),
            _ => return Err(ClientError::Unexpected),
        };
        let sig = sign_challenge(reader, &challenge, &relay_id, action, number);
        Ok(i.relay.handle(
            self.conn,
            build(reader, challenge, sig),
            now,
            &mut os_random,
        ))
    }
}

impl Post for MemoryPost {
    fn make_mailbox(&mut self, reader: &Identity) -> Result<(), ClientError> {
        match self.signed(reader, Action::Make, 0, |r, challenge, sig| {
            Request::MakeMailbox {
                reader: r.device_id(),
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
        let mut i = self.relay.lock()?;
        i.calls = i.calls.saturating_add(1);
        if i.down {
            return Err(ClientError::Connect(io::ErrorKind::ConnectionRefused));
        }
        i.puts = i.puts.saturating_add(1);
        if i.lose_puts {
            return Ok(0);
        }
        let now = i.now;
        match i.relay.handle(
            self.conn,
            Request::Put {
                sealed: sealed.to_vec(),
            },
            now,
            &mut os_random,
        ) {
            Response::Stored { id } => Ok(id),
            Response::Refused(r) => Err(refused(r)),
            _ => Err(ClientError::Unexpected),
        }
    }

    fn fetch(&mut self, reader: &Identity, cursor: u64) -> Result<Fetched, ClientError> {
        match self.signed(reader, Action::Fetch, cursor, |r, challenge, sig| {
            Request::Fetch {
                reader: r.device_id(),
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
        match self.signed(reader, Action::Ack, up_to, |r, challenge, sig| {
            Request::Ack {
                reader: r.device_id(),
                challenge,
                sig,
                up_to,
            }
        })? {
            Response::Acked { dropped } => Ok(dropped),
            Response::Refused(r) => Err(refused(r)),
            _ => Err(ClientError::Unexpected),
        }
    }

    fn close(&mut self) {
        if let Ok(mut i) = self.relay.inner.lock() {
            i.relay.close(self.conn);
        }
    }
}
