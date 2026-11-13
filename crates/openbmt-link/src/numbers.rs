//! The message numbers a device keeps (LK-06, LK-40).
//!
//! An [`Inbox`] holds, per sender, the last number taken. An [`Outbox`]
//! holds, per reader, the last number sent. Both change the same way as the
//! other stored values in this workspace: build the new value, hand its
//! bytes to `persist`, and swap only if `persist` returned `Ok`. A message's
//! content is returned, and a number handed out, only after that.
//!
//! A table that is missing from storage is not the same as an empty one. A
//! device that starts for the first time makes `Inbox::default()`; a device
//! whose file has vanished must be paired again (LK-40) and must not call
//! `default()` in its place. The caller tells the two apart; these types
//! cannot.

use std::collections::BTreeMap;
use std::fmt;

use openbmt_core::identity::{DeviceId, Identity};
use openbmt_core::wire::{self, DecodeError, Reader};

use crate::seal::{open, OpenError, Opened, Role};

/// Magic of a stored [`Inbox`].
pub const MAGIC_INBOX: [u8; 4] = *b"OBMI";
/// Magic of a stored [`Outbox`].
pub const MAGIC_OUTBOX: [u8; 4] = *b"OBMU";

/// The file of the numbers of the Store and Stored kinds a device has taken
/// (LK-81), beside the device's `link-inbox`.
pub const STORE_INBOX_FILE: &str = "link-store-inbox";
/// The file of the numbers of the Store and Stored kinds a device has sent
/// (LK-81), beside the device's `link-outbox`.
pub const STORE_OUTBOX_FILE: &str = "link-store-outbox";

/// The most peers a table holds. A shop has a handful of devices; the bound
/// only keeps a hostile file from asking for memory.
const MAX_PEERS: usize = 1024;

fn encode(magic: &[u8; 4], table: &BTreeMap<DeviceId, u64>) -> Vec<u8> {
    let mut out = wire::start(magic, 4 + table.len() * 40);
    out.extend_from_slice(&wire::len_u32(table.len()).to_le_bytes());
    for (peer, n) in table {
        out.extend_from_slice(&peer.to_bytes());
        out.extend_from_slice(&n.to_le_bytes());
    }
    out
}

fn decode(magic: &[u8; 4], bytes: &[u8]) -> Result<BTreeMap<DeviceId, u64>, DecodeError> {
    let mut r = Reader::framed(bytes, magic)?;
    let count = r.count(MAX_PEERS, 40)?;
    let mut table = BTreeMap::new();
    let mut previous: Option<DeviceId> = None;
    for _ in 0..count {
        let peer = r.device_id()?;
        let n = r.u64_le()?;
        if previous.is_some_and(|p| p >= peer) {
            return Err(DecodeError::NonCanonical(
                "peers are not in ascending order",
            ));
        }
        previous = Some(peer);
        table.insert(peer, n);
    }
    r.finish()?;
    Ok(table)
}

/// Why a message was not taken.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TakeError<E> {
    /// The message did not open.
    Open(OpenError),
    /// The new number could not be stored, so the content is not given out.
    Persist(E),
    /// The table holds as many senders as it may, and this one is new.
    Full,
}

impl<E: fmt::Display> fmt::Display for TakeError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Open(e) => write!(f, "{e}"),
            Self::Persist(e) => write!(f, "LK-40: the number could not be stored: {e}"),
            Self::Full => write!(f, "LK-40: no room for another sender's number"),
        }
    }
}

impl<E: fmt::Debug + fmt::Display> std::error::Error for TakeError<E> {}

/// The last number taken from each sender.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Inbox {
    last: BTreeMap<DeviceId, u64>,
}

impl Inbox {
    /// The last number taken from `sender`; 0 if none was.
    pub fn last(&self, sender: &DeviceId) -> u64 {
        self.last.get(sender).copied().unwrap_or(0)
    }

    /// The bytes to store.
    pub fn to_bytes(&self) -> Vec<u8> {
        encode(&MAGIC_INBOX, &self.last)
    }

    /// Read stored bytes, strictly.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, DecodeError> {
        Ok(Self {
            last: decode(&MAGIC_INBOX, bytes)?,
        })
    }

    /// Open `sealed` (LK-05), store the new last number for its sender, and
    /// only then return the content (LK-06, LK-40). If `persist` fails the
    /// inbox is unchanged and nothing is returned. A Store or Stored
    /// message is refused here: its numbers are a count of their own
    /// ([`Inbox::open_store`], LK-81).
    pub fn open<E>(
        &mut self,
        reader: &Identity,
        reader_role: Role,
        sealed: &[u8],
        allowed: impl Fn(&DeviceId) -> Option<Role>,
        persist: impl FnOnce(&[u8]) -> Result<(), E>,
    ) -> Result<Opened, TakeError<E>> {
        self.open_counted(reader, reader_role, sealed, allowed, persist, false)
    }

    /// [`Inbox::open`] for the inbox of the Store and Stored kinds, which is
    /// a table of its own beside the other one (LK-81). Any other kind is
    /// refused, so that the two counts never mix.
    pub fn open_store<E>(
        &mut self,
        reader: &Identity,
        reader_role: Role,
        sealed: &[u8],
        allowed: impl Fn(&DeviceId) -> Option<Role>,
        persist: impl FnOnce(&[u8]) -> Result<(), E>,
    ) -> Result<Opened, TakeError<E>> {
        self.open_counted(reader, reader_role, sealed, allowed, persist, true)
    }

    fn open_counted<E>(
        &mut self,
        reader: &Identity,
        reader_role: Role,
        sealed: &[u8],
        allowed: impl Fn(&DeviceId) -> Option<Role>,
        persist: impl FnOnce(&[u8]) -> Result<(), E>,
        apart: bool,
    ) -> Result<Opened, TakeError<E>> {
        let opened = open(reader, reader_role, sealed, allowed, |s| self.last(s))
            .map_err(TakeError::Open)?;
        if opened.kind.counts_apart() != apart {
            return Err(TakeError::Open(OpenError::KindNotAllowed(opened.kind)));
        }
        let mut next = self.last.clone();
        if !next.contains_key(&opened.sender) && next.len() >= MAX_PEERS {
            return Err(TakeError::Full);
        }
        next.insert(opened.sender, opened.number);
        persist(&encode(&MAGIC_INBOX, &next)).map_err(TakeError::Persist)?;
        self.last = next;
        Ok(opened)
    }
}

/// Why a number was not handed out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NextError<E> {
    /// The counter for this reader is at the top of the u64 range.
    Exhausted,
    /// The new number could not be stored, so it is not handed out.
    Persist(E),
}

impl<E: fmt::Display> fmt::Display for NextError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Exhausted => write!(f, "LK-06: message numbers for this reader are used up"),
            Self::Persist(e) => write!(f, "LK-40: the number could not be stored: {e}"),
        }
    }
}

impl<E: fmt::Debug + fmt::Display> std::error::Error for NextError<E> {}

/// The last number sent to each reader.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Outbox {
    last: BTreeMap<DeviceId, u64>,
}

impl Outbox {
    /// The last number sent to `reader`; 0 if none was.
    pub fn last(&self, reader: &DeviceId) -> u64 {
        self.last.get(reader).copied().unwrap_or(0)
    }

    /// The bytes to store.
    pub fn to_bytes(&self) -> Vec<u8> {
        encode(&MAGIC_OUTBOX, &self.last)
    }

    /// Read stored bytes, strictly.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, DecodeError> {
        Ok(Self {
            last: decode(&MAGIC_OUTBOX, bytes)?,
        })
    }

    /// The next number for `reader`. It is stored before it is returned, so
    /// a message with it can leave (LK-40). A number that was stored and not
    /// used is lost, which LK-06 allows.
    pub fn next<E>(
        &mut self,
        reader: &DeviceId,
        persist: impl FnOnce(&[u8]) -> Result<(), E>,
    ) -> Result<u64, NextError<E>> {
        let number = self
            .last(reader)
            .checked_add(1)
            .ok_or(NextError::Exhausted)?;
        let mut next = self.last.clone();
        if !next.contains_key(reader) && next.len() >= MAX_PEERS {
            return Err(NextError::Exhausted);
        }
        next.insert(*reader, number);
        persist(&encode(&MAGIC_OUTBOX, &next)).map_err(NextError::Persist)?;
        self.last = next;
        Ok(number)
    }
}
