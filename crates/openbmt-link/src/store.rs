//! Where a relay keeps its mailboxes (LK-41).
//!
//! A store keeps, per reader, an append-only list of events: a message put
//! under a number, and a floor (every number up to it is gone). Replaying
//! the events in order gives the mailbox. [`MemoryStore`] is for tests.
//! [`DirStore`] keeps one log file per mailbox.
//!
//! A log record is `length u32 LE | type u8 | body | check 4`, where the
//! length counts the type and the body, and the check is the first four
//! bytes of the SHA-256 of the type and the body. A torn write leaves a
//! last record that is short or fails its check; loading cuts the file back
//! to the last good record, so at most the message being written is lost.
//! When more than half of a file is records that no longer count
//! (acknowledged or pushed out), the file is rewritten whole: a temporary
//! file, flushed, renamed over the log, and the directory flushed.

use std::collections::HashMap;
use std::fmt::{self, Display};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use bitcoin::hashes::{sha256, Hash};
use openbmt_core::identity::DeviceId;

use crate::relay::Mailbox;
use crate::seal::MAX_SEALED_LEN;

/// One thing a store remembers about a mailbox.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// A message held under this number.
    Put {
        /// The relay's number for it.
        id: u64,
        /// The sealed bytes.
        sealed: Vec<u8>,
    },
    /// Every number up to this one is gone.
    Floor(u64),
}

/// What a store holds when it is opened.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Loaded {
    /// The member list.
    pub members: Vec<DeviceId>,
    /// The devices whose mailboxes are leaving (LK-28), each with the time
    /// it became so. A store that never kept any gives none.
    pub leaving: Vec<(DeviceId, u64)>,
    /// Each member's and each leaving device's mailbox as events.
    pub mailboxes: Vec<(DeviceId, Vec<Event>)>,
}

/// A place to keep mailboxes. Every write is on disk (or in memory, for the
/// test store) before it returns.
pub trait MailStore {
    /// What a failed call says.
    type Error: Display;

    /// The member list, the leaving list and the mailbox of every member
    /// and every leaving device with its events, in order. A mailbox of a
    /// device on neither list is not returned.
    fn load(&mut self) -> Result<Loaded, Self::Error>;

    /// Replace the member list and the leaving list (LK-21, LK-28) and
    /// remove the mailboxes of every device that is on neither. The lists
    /// are stored before the mailboxes go.
    fn set_members(
        &mut self,
        members: &[DeviceId],
        leaving: &[(DeviceId, u64)],
    ) -> Result<(), Self::Error>;

    /// Make an empty mailbox. The relay asks only when it holds none for
    /// that reader, so anything a store still has under that name is left
    /// over from a removed member and is emptied.
    fn create(&mut self, reader: &DeviceId) -> Result<(), Self::Error>;

    /// Record a message put under `id`.
    fn put(&mut self, reader: &DeviceId, id: u64, sealed: &[u8]) -> Result<(), Self::Error>;

    /// Record that every number up to `up_to` is gone.
    fn ack(&mut self, reader: &DeviceId, up_to: u64) -> Result<(), Self::Error>;

    /// Rewrite the mailbox's record from `live` if most of it is dead. The
    /// relay ignores a failure: the old record is still whole.
    fn compact(&mut self, reader: &DeviceId, live: &Mailbox) -> Result<(), Self::Error>;
}

/// A store in memory, for tests. Set `fail_writes` to make every write fail.
#[derive(Debug, Default)]
pub struct MemoryStore {
    boxes: HashMap<DeviceId, Vec<Event>>,
    members: Vec<DeviceId>,
    leaving: Vec<(DeviceId, u64)>,
    /// Make `create`, `put`, `ack` and `set_members` fail.
    pub fail_writes: bool,
}

/// The test store's only error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MemoryFailure;

impl Display for MemoryFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "the memory store was told to fail")
    }
}

impl MemoryStore {
    /// An empty store.
    pub fn new() -> Self {
        Self::default()
    }

    /// The mailboxes held.
    pub fn len(&self) -> usize {
        self.boxes.len()
    }

    /// Whether there are no mailboxes.
    pub fn is_empty(&self) -> bool {
        self.boxes.is_empty()
    }

    fn kept(&self, reader: &DeviceId) -> bool {
        self.members.contains(reader) || self.leaving.iter().any(|(d, _)| d == reader)
    }

    /// What is kept for one reader.
    pub fn events(&self, reader: &DeviceId) -> Vec<Event> {
        self.boxes.get(reader).cloned().unwrap_or_default()
    }
}

impl MailStore for MemoryStore {
    type Error = MemoryFailure;

    fn load(&mut self) -> Result<Loaded, MemoryFailure> {
        Ok(Loaded {
            members: self.members.clone(),
            leaving: self.leaving.clone(),
            mailboxes: self
                .boxes
                .iter()
                .filter(|(k, _)| self.kept(k))
                .map(|(k, v)| (*k, v.clone()))
                .collect(),
        })
    }

    fn set_members(
        &mut self,
        members: &[DeviceId],
        leaving: &[(DeviceId, u64)],
    ) -> Result<(), MemoryFailure> {
        if self.fail_writes {
            return Err(MemoryFailure);
        }
        self.members = members.to_vec();
        self.leaving = leaving.to_vec();
        let (members, leaving) = (&self.members, &self.leaving);
        self.boxes
            .retain(|k, _| members.contains(k) || leaving.iter().any(|(d, _)| d == k));
        Ok(())
    }

    fn create(&mut self, reader: &DeviceId) -> Result<(), MemoryFailure> {
        if self.fail_writes {
            return Err(MemoryFailure);
        }
        self.boxes.insert(*reader, Vec::new());
        Ok(())
    }

    fn put(&mut self, reader: &DeviceId, id: u64, sealed: &[u8]) -> Result<(), MemoryFailure> {
        if self.fail_writes {
            return Err(MemoryFailure);
        }
        self.boxes.entry(*reader).or_default().push(Event::Put {
            id,
            sealed: sealed.to_vec(),
        });
        Ok(())
    }

    fn ack(&mut self, reader: &DeviceId, up_to: u64) -> Result<(), MemoryFailure> {
        if self.fail_writes {
            return Err(MemoryFailure);
        }
        self.boxes
            .entry(*reader)
            .or_default()
            .push(Event::Floor(up_to));
        Ok(())
    }

    fn compact(&mut self, _reader: &DeviceId, _live: &Mailbox) -> Result<(), MemoryFailure> {
        Ok(())
    }
}

const REC_PUT: u8 = 1;
const REC_FLOOR: u8 = 2;
const CHECK_LEN: usize = 4;
/// The extension of a log file.
const EXT: &str = "mbx";
/// The file that holds the member list.
const MEMBERS_FILE: &str = "members.set";
/// The file that holds the leaving list (LK-42): 40 bytes per device (its
/// key, then the time it became leaving as u64 LE), then the check. A
/// directory without it has no leaving mailboxes.
const LEAVING_FILE: &str = "leaving.set";

/// One log file per mailbox, in a directory.
#[derive(Debug)]
pub struct DirStore {
    dir: PathBuf,
    /// Bytes in each log, as last written or loaded.
    sizes: HashMap<DeviceId, u64>,
}

fn record(kind: u8, body: &[u8]) -> Vec<u8> {
    let inner_len = body.len().saturating_add(1);
    let mut inner = Vec::with_capacity(inner_len);
    inner.push(kind);
    inner.extend_from_slice(body);
    let check = sha256::Hash::hash(&inner).to_byte_array();
    let mut out = Vec::with_capacity(4 + inner.len() + CHECK_LEN);
    out.extend_from_slice(&u32::try_from(inner_len).unwrap_or(u32::MAX).to_le_bytes());
    out.extend_from_slice(&inner);
    out.extend_from_slice(check.get(..CHECK_LEN).unwrap_or(&[]));
    out
}

fn put_record(id: u64, sealed: &[u8]) -> Vec<u8> {
    let mut body = Vec::with_capacity(8 + sealed.len());
    body.extend_from_slice(&id.to_le_bytes());
    body.extend_from_slice(sealed);
    record(REC_PUT, &body)
}

fn floor_record(up_to: u64) -> Vec<u8> {
    record(REC_FLOOR, &up_to.to_le_bytes())
}

/// Read records until the bytes end or one is bad. Returns the events and
/// the length of the good part.
fn parse_log(bytes: &[u8]) -> (Vec<Event>, usize) {
    let mut events = Vec::new();
    let mut at = 0usize;
    while let Some(rest) = bytes.get(at..) {
        let Some(head) = rest.get(..4).and_then(|h| <[u8; 4]>::try_from(h).ok()) else {
            break;
        };
        let Ok(len) = usize::try_from(u32::from_le_bytes(head)) else {
            break;
        };
        if len == 0 || len > MAX_SEALED_LEN + 9 {
            break;
        }
        let Some(end) = len.checked_add(4 + CHECK_LEN) else {
            break;
        };
        let Some(whole) = rest.get(..end) else { break };
        let (Some(inner), Some(check)) = (whole.get(4..4 + len), whole.get(4 + len..)) else {
            break;
        };
        let want = sha256::Hash::hash(inner).to_byte_array();
        if want.get(..CHECK_LEN) != Some(check) {
            break;
        }
        let Some((kind, body)) = inner.split_first() else {
            break;
        };
        let event = match *kind {
            REC_PUT => {
                let Some((id, sealed)) = body.split_at_checked(8) else {
                    break;
                };
                let Ok(id) = <[u8; 8]>::try_from(id) else {
                    break;
                };
                Event::Put {
                    id: u64::from_le_bytes(id),
                    sealed: sealed.to_vec(),
                }
            }
            REC_FLOOR => {
                let Ok(n) = <[u8; 8]>::try_from(body) else {
                    break;
                };
                Event::Floor(u64::from_le_bytes(n))
            }
            _ => break,
        };
        events.push(event);
        at = at.saturating_add(end);
    }
    (events, at)
}

/// The bytes of a leaving list (LK-42): each device's key and the time it
/// became leaving (u64 LE), then the first four bytes of their SHA-256.
pub fn encode_leaving(leaving: &[(DeviceId, u64)]) -> Vec<u8> {
    let mut out = Vec::with_capacity(leaving.len().saturating_mul(40).saturating_add(CHECK_LEN));
    for (device, at) in leaving {
        out.extend_from_slice(&device.to_bytes());
        out.extend_from_slice(&at.to_le_bytes());
    }
    let check = sha256::Hash::hash(&out).to_byte_array();
    out.extend_from_slice(check.get(..CHECK_LEN).unwrap_or(&[]));
    out
}

/// Read a leaving list back (LK-42); `None` for bytes that are not one.
/// What it reads encodes to the same bytes.
pub fn decode_leaving(bytes: &[u8]) -> Option<Vec<(DeviceId, u64)>> {
    let split = bytes.len().checked_sub(CHECK_LEN)?;
    let (body, check) = bytes.split_at(split);
    let want = sha256::Hash::hash(body).to_byte_array();
    if want.get(..CHECK_LEN) != Some(check) {
        return None;
    }
    let (entries, rest) = body.as_chunks::<40>();
    if !rest.is_empty() {
        return None;
    }
    entries
        .iter()
        .map(|entry| {
            let (key, at) = entry.split_at_checked(32)?;
            let key = <[u8; 32]>::try_from(key).ok()?;
            let at = <[u8; 8]>::try_from(at).ok()?;
            Some((DeviceId::from_bytes(&key).ok()?, u64::from_le_bytes(at)))
        })
        .collect()
}

fn hex(bytes: &[u8; 32]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn unhex(name: &str) -> Option<DeviceId> {
    if name.len() != 64 {
        return None;
    }
    let mut bytes = [0u8; 32];
    for (slot, pair) in bytes.iter_mut().zip(name.as_bytes().chunks(2)) {
        let text = std::str::from_utf8(pair).ok()?;
        *slot = u8::from_str_radix(text, 16).ok()?;
    }
    DeviceId::from_bytes(&bytes).ok()
}

impl DirStore {
    /// The member list, or an empty one if none was ever stored. A file that
    /// fails its check is an error: the relay does not start on a list it
    /// cannot trust.
    fn read_members(&self) -> io::Result<Vec<DeviceId>> {
        let bytes = match fs::read(self.dir.join(MEMBERS_FILE)) {
            Ok(b) => b,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e),
        };
        let bad = || io::Error::new(io::ErrorKind::InvalidData, "the member file is damaged");
        let split = bytes.len().checked_sub(CHECK_LEN).ok_or_else(bad)?;
        let (keys, check) = bytes.split_at(split);
        let want = sha256::Hash::hash(keys).to_byte_array();
        if want.get(..CHECK_LEN) != Some(check) || keys.len() % 32 != 0 {
            return Err(bad());
        }
        keys.chunks(32)
            .map(|c| {
                <[u8; 32]>::try_from(c)
                    .ok()
                    .and_then(|k| DeviceId::from_bytes(&k).ok())
                    .ok_or_else(bad)
            })
            .collect()
    }

    /// The leaving list, or an empty one if none was ever stored (LK-42). A
    /// file that fails its check is an error, as for the members.
    fn read_leaving(&self) -> io::Result<Vec<(DeviceId, u64)>> {
        let bytes = match fs::read(self.dir.join(LEAVING_FILE)) {
            Ok(b) => b,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e),
        };
        decode_leaving(&bytes).ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "the leaving file is damaged")
        })
    }

    /// Write a list file whole: a temporary file, flushed, renamed over the
    /// old one, and the directory flushed.
    fn write_list(&self, name: &str, body: Vec<u8>) -> io::Result<()> {
        let path = self.dir.join(name);
        let temp = self.dir.join(format!("{name}.tmp"));
        let mut file = openbmt_net_io::private::create_file(&temp)?;
        file.write_all(&body)?;
        file.sync_all()?;
        fs::rename(&temp, &path)?;
        self.sync_dir()
    }

    /// Keep logs in this directory, which is created if it is missing.
    pub fn new(dir: impl Into<PathBuf>) -> io::Result<Self> {
        let dir = dir.into();
        openbmt_net_io::private::create_dir_all(&dir)?;
        Ok(Self {
            dir,
            sizes: HashMap::new(),
        })
    }

    /// The directory the logs are in.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// The log file of a reader's mailbox.
    pub fn path(&self, reader: &DeviceId) -> PathBuf {
        self.dir.join(format!("{}.{EXT}", hex(&reader.to_bytes())))
    }

    fn append(&mut self, reader: &DeviceId, bytes: &[u8]) -> io::Result<()> {
        let mut file = OpenOptions::new().append(true).open(self.path(reader))?;
        file.write_all(bytes)?;
        file.sync_data()?;
        let size = self.sizes.entry(*reader).or_insert(0);
        *size = size.saturating_add(u64::try_from(bytes.len()).unwrap_or(u64::MAX));
        Ok(())
    }

    fn sync_dir(&self) -> io::Result<()> {
        File::open(&self.dir)?.sync_all()
    }
}

impl MailStore for DirStore {
    type Error = io::Error;

    fn load(&mut self) -> io::Result<Loaded> {
        let members = self.read_members()?;
        let leaving = self.read_leaving()?;
        let mut out = Vec::new();
        for entry in fs::read_dir(&self.dir)? {
            let path = entry?.path();
            if path.extension().and_then(|e| e.to_str()) != Some(EXT) {
                continue;
            }
            let Some(reader) = path.file_stem().and_then(|s| s.to_str()).and_then(unhex) else {
                continue;
            };
            if !members.contains(&reader) && !leaving.iter().any(|(d, _)| *d == reader) {
                // Left behind by a crash between storing a new list and
                // removing the files; it must not come back if the device
                // is added again.
                fs::remove_file(&path)?;
                continue;
            }
            let bytes = fs::read(&path)?;
            let (events, good) = parse_log(&bytes);
            if good < bytes.len() {
                // A torn tail: cut the file back to the last whole record.
                let file = OpenOptions::new().write(true).open(&path)?;
                file.set_len(u64::try_from(good).unwrap_or(0))?;
                file.sync_all()?;
            }
            self.sizes
                .insert(reader, u64::try_from(good).unwrap_or(u64::MAX));
            out.push((reader, events));
        }
        Ok(Loaded {
            members,
            leaving,
            mailboxes: out,
        })
    }

    fn create(&mut self, reader: &DeviceId) -> io::Result<()> {
        openbmt_net_io::private::create_file(&self.path(reader))?.sync_all()?;
        self.sync_dir()?;
        self.sizes.insert(*reader, 0);
        Ok(())
    }

    fn put(&mut self, reader: &DeviceId, id: u64, sealed: &[u8]) -> io::Result<()> {
        self.append(reader, &put_record(id, sealed))
    }

    fn ack(&mut self, reader: &DeviceId, up_to: u64) -> io::Result<()> {
        self.append(reader, &floor_record(up_to))
    }

    fn compact(&mut self, reader: &DeviceId, live: &Mailbox) -> io::Result<()> {
        let size = self.sizes.get(reader).copied().unwrap_or(0);
        let floor = live.next_id().saturating_sub(1);
        // What the rewritten file would hold: a floor record and the live
        // puts. A record is 4 (length) + 1 (type) + body + 4 (check).
        let mut live_len = if floor > 0 { 17usize } else { 0 };
        for (_, sealed) in live.messages() {
            live_len = live_len.saturating_add(17).saturating_add(sealed.len());
        }
        let live_len = u64::try_from(live_len).unwrap_or(u64::MAX);
        // Compact only when more than half of the file is dead.
        if size.saturating_sub(live_len).saturating_mul(2) <= size {
            return Ok(());
        }
        let mut fresh = Vec::new();
        if floor > 0 {
            fresh.extend_from_slice(&floor_record(floor));
        }
        for (id, sealed) in live.messages() {
            fresh.extend_from_slice(&put_record(id, sealed));
        }
        let path = self.path(reader);
        let temp = path.with_extension("mbx.tmp");
        let mut file = openbmt_net_io::private::create_file(&temp)?;
        file.write_all(&fresh)?;
        file.sync_all()?;
        fs::rename(&temp, &path)?;
        self.sync_dir()?;
        self.sizes.insert(*reader, live_len);
        Ok(())
    }

    fn set_members(&mut self, members: &[DeviceId], leaving: &[(DeviceId, u64)]) -> io::Result<()> {
        // The leaving list goes first: a crash between the two files leaves
        // a leaving device that is still a member, which the relay ignores,
        // and never a removed member with no mailbox.
        self.write_list(LEAVING_FILE, encode_leaving(leaving))?;
        let mut body = Vec::with_capacity(members.len().saturating_mul(32));
        for m in members {
            body.extend_from_slice(&m.to_bytes());
        }
        let check = sha256::Hash::hash(&body).to_byte_array();
        body.extend_from_slice(check.get(..CHECK_LEN).unwrap_or(&[]));
        self.write_list(MEMBERS_FILE, body)?;
        for entry in fs::read_dir(&self.dir)? {
            let path = entry?.path();
            if path.extension().and_then(|e| e.to_str()) != Some(EXT) {
                continue;
            }
            let Some(reader) = path.file_stem().and_then(|s| s.to_str()).and_then(unhex) else {
                continue;
            };
            if !members.contains(&reader) && !leaving.iter().any(|(d, _)| *d == reader) {
                // The list is stored, so the change has happened. A file
                // that will not go is removed at the next start (`load`)
                // and emptied if the device is added again (`create`).
                let _ = fs::remove_file(&path);
                self.sizes.remove(&reader);
            }
        }
        let _ = self.sync_dir();
        Ok(())
    }
}
