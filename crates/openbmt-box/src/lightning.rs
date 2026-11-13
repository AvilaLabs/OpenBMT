//! Shop Lightning's store on the Box (BX-90 to BX-95).
//!
//! The Box keeps the second copy of the state of the Lightning counter that
//! the settings in force name with this Box as its second device. The copy
//! is sealed with the node's key before it reaches the Box (SL-75), and the
//! Box holds no key that opens it. It sees the names, the sizes and the
//! times of change.
//!
//! The files, in the data directory:
//!
//! | File | Holds |
//! |---|---|
//! | `lightning/<counter ID in hex>/<name in hex>` | one blob (BX-93) |
//! | `lightning/<counter ID in hex>/<name in hex>.tmp` | a write in progress, whose parts have come so far (BX-91) |
//! | `lightning-tenants` | the counters served, and for each whether it is named or when its 30 days end (BX-94) |
//! | `link-store-inbox`, `link-store-outbox` | the numbers of the Store and Stored kinds (LK-81) |
//!
//! [`ShopStore`] reads no clock and opens no socket: the time and the
//! randomness are passed in. The blobs are written with the standard
//! library, the small files through [`BoxFiles`].

use std::collections::BTreeMap;
use std::fmt;
use std::fs::{self, DirBuilder, File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

use openbmt_core::hkdf::sha256_of;
use openbmt_core::identity::{DeviceId, Identity};
use openbmt_core::wire::{self, DecodeError, Reader};
use openbmt_link::numbers::{STORE_INBOX_FILE, STORE_OUTBOX_FILE};
use openbmt_link::shopln::{
    name_ok, Part, StoreBody, StoreCause, StoreMessage, StoredBody, StoredMessage, MAX_BLOB_LEN,
    MAX_LIST_BYTES, MAX_PART_LEN, MAX_STORED_NAME_LEN,
};
use openbmt_link::{seal, Inbox, Kind, Outbox, Role, StoreAnswer, StoreParties};

use crate::files::BoxFiles;

/// The folder of the data directory that holds the stored state (BX-93).
pub const LIGHTNING_DIR: &str = "lightning";
/// The counters the Box serves, and when each stops being served (BX-94).
pub const TENANTS_FILE: &str = "lightning-tenants";
/// Seconds a write may wait for its next part (BX-91).
pub const PART_TIMEOUT_SECS: u64 = 60;
/// Most writes in progress for one counter (BX-91).
pub const MAX_WRITES: usize = 8;
/// Most bytes the Box keeps for one counter: 1 GiB (BX-92).
pub const MAX_STORE_BYTES: u64 = 1 << 30;
/// Most names the Box keeps for one counter (BX-92).
pub const MAX_NAMES: usize = 10_000;
/// Seconds a counter is served after the settings stop naming this Box as
/// its second device: 30 days (BX-94).
pub const GRACE_SECS: u64 = 30 * 24 * 60 * 60;

const MAGIC_TENANTS: [u8; 4] = *b"OBM\"";
/// The most counters the tenants file holds: the counters a settings value
/// may name over 30 days are few; this only bounds a damaged file.
const MAX_TENANTS: usize = 64;
const TMP_SUFFIX: &str = ".tmp";

/// Why the Box's store could not be opened or kept up. Each names its rule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoreError {
    /// A file that must be read is damaged (BX-51).
    Damaged {
        /// The file.
        file: &'static str,
    },
    /// A file or folder could not be read or written (BX-91).
    Io {
        /// What was being done.
        op: &'static str,
        /// What the system said.
        kind: io::ErrorKind,
    },
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Damaged { file } => write!(f, "BX-51: {file} does not read back"),
            Self::Io { op, kind } => write!(f, "BX-91: {op}: {kind}"),
        }
    }
}

impl std::error::Error for StoreError {}

fn io_err(op: &'static str) -> impl Fn(io::Error) -> StoreError {
    move |e| StoreError::Io { op, kind: e.kind() }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn unhex(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2)
        || !text.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
    {
        return None;
    }
    (0..text.len() / 2)
        .map(|i| u8::from_str_radix(text.get(2 * i..2 * i + 2)?, 16).ok())
        .collect()
}

/// A counter the Box serves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Tenant {
    counter: DeviceId,
    /// 0 while the settings name it; else the second after which it is no
    /// longer served (BX-94).
    ends: u64,
}

fn encode_tenants(tenants: &[Tenant]) -> Vec<u8> {
    let mut out = wire::start(&MAGIC_TENANTS, 4 + tenants.len() * 40);
    out.extend_from_slice(&wire::len_u32(tenants.len()).to_le_bytes());
    for t in tenants {
        out.extend_from_slice(&t.counter.to_bytes());
        out.extend_from_slice(&t.ends.to_le_bytes());
    }
    out
}

fn decode_tenants(bytes: &[u8]) -> Result<Vec<Tenant>, DecodeError> {
    let mut r = Reader::framed(bytes, &MAGIC_TENANTS)?;
    let n = r.count(MAX_TENANTS, 40)?;
    let mut out: Vec<Tenant> = Vec::with_capacity(n);
    for _ in 0..n {
        let counter = r.device_id()?;
        let ends = r.u64_le()?;
        if out.iter().any(|t| t.counter == counter) {
            return Err(DecodeError::NonCanonical("a counter is listed twice"));
        }
        out.push(Tenant { counter, ends });
    }
    r.finish()?;
    Ok(out)
}

/// A write in progress: its parts so far are in a temporary file.
#[derive(Debug)]
struct Pending {
    parts: u32,
    next: u32,
    sha256: [u8; 32],
    file: File,
    path: PathBuf,
    written: u64,
    last: u64,
}

/// What the Box keeps for one counter.
#[derive(Debug)]
struct Space {
    dir: PathBuf,
    /// Every blob, with its size.
    blobs: BTreeMap<String, u64>,
    writes: BTreeMap<String, Pending>,
    /// The blob last read, for the next part of the same read.
    cache: Option<(String, Vec<u8>, [u8; 32])>,
    /// A write was refused for room and nothing has freed any since.
    refused_full: bool,
}

fn sync_dir(dir: &Path) -> io::Result<()> {
    File::open(dir).and_then(|d| d.sync_all())
}

impl Space {
    /// Open the counter's folder, making it if it is not there. A temporary
    /// file left by a write that never finished is removed: the old blob is
    /// what the counter has (BX-91).
    fn open(dir: PathBuf) -> Result<Self, StoreError> {
        DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&dir)
            .map_err(io_err("the counter's folder could not be made"))?;
        let mut blobs = BTreeMap::new();
        let entries = fs::read_dir(&dir).map_err(io_err("the counter's folder cannot be read"))?;
        for entry in entries.flatten() {
            let file_name = entry.file_name();
            let Some(file_name) = file_name.to_str() else {
                continue;
            };
            if file_name.ends_with(TMP_SUFFIX) {
                let _ = fs::remove_file(entry.path());
                continue;
            }
            let name = unhex(file_name)
                .and_then(|b| String::from_utf8(b).ok())
                .filter(|n| name_ok(n));
            if let (Some(name), Ok(meta)) = (name, entry.metadata()) {
                if meta.is_file() {
                    blobs.insert(name, meta.len());
                }
            }
        }
        Ok(Self {
            dir,
            blobs,
            writes: BTreeMap::new(),
            cache: None,
            refused_full: false,
        })
    }

    fn blob_path(&self, name: &str) -> PathBuf {
        self.dir.join(hex(name.as_bytes()))
    }

    fn used_bytes(&self, except: &str) -> u64 {
        let blobs: u64 = self
            .blobs
            .iter()
            .filter(|(n, _)| n.as_str() != except)
            .map(|(_, len)| *len)
            .sum();
        let pending: u64 = self
            .writes
            .iter()
            .filter(|(n, _)| n.as_str() != except)
            .map(|(_, w)| w.written)
            .sum();
        blobs.saturating_add(pending)
    }

    fn at_limit(&self) -> bool {
        self.used_bytes("") >= MAX_STORE_BYTES || self.blobs.len() >= MAX_NAMES
    }

    /// Drop the writes whose next part is 60 seconds late (BX-91).
    fn expire(&mut self, now: u64) {
        let late: Vec<String> = self
            .writes
            .iter()
            .filter(|(_, w)| now.saturating_sub(w.last) >= PART_TIMEOUT_SECS)
            .map(|(n, _)| n.clone())
            .collect();
        for name in late {
            self.drop_write(&name);
        }
    }

    fn drop_write(&mut self, name: &str) {
        if let Some(w) = self.writes.remove(name) {
            let _ = fs::remove_file(&w.path);
        }
    }

    fn write(&mut self, name: &str, part: &Part, now: u64) -> Result<StoredBody, StoreCause> {
        if part.number == 0 {
            if name.len() > MAX_STORED_NAME_LEN {
                return Err(StoreCause::NameTooLong);
            }
            // A first part starts the write again if one was in progress.
            self.drop_write(name);
            if self.writes.len() >= MAX_WRITES {
                return Err(StoreCause::TooManyWrites);
            }
            if !self.blobs.contains_key(name) && self.blobs.len() + self.new_names() >= MAX_NAMES {
                self.refused_full = true;
                return Err(StoreCause::TooManyNames);
            }
            let path = self
                .dir
                .join(format!("{}{TMP_SUFFIX}", hex(name.as_bytes())));
            let file = OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .mode(0o600)
                .open(&path)
                .map_err(|_| StoreCause::Storage)?;
            self.writes.insert(
                name.to_string(),
                Pending {
                    parts: part.parts,
                    next: 0,
                    sha256: part.sha256,
                    file,
                    path,
                    written: 0,
                    last: now,
                },
            );
        }
        let Some(w) = self.writes.get(name) else {
            return Err(StoreCause::PartOutOfOrder);
        };
        if part.number != w.next || part.parts != w.parts || part.sha256 != w.sha256 {
            self.drop_write(name);
            return Err(StoreCause::PartOutOfOrder);
        }
        let after = w.written.saturating_add(part.bytes.len() as u64);
        if after > MAX_BLOB_LEN as u64 {
            self.drop_write(name);
            return Err(StoreCause::BlobTooLarge);
        }
        if self.used_bytes(name).saturating_add(after) > MAX_STORE_BYTES {
            self.drop_write(name);
            self.refused_full = true;
            return Err(StoreCause::StoreFull);
        }
        let last_part = part.number + 1 == part.parts;
        let Some(w) = self.writes.get_mut(name) else {
            return Err(StoreCause::PartOutOfOrder);
        };
        if w.file.write_all(&part.bytes).is_err() {
            self.drop_write(name);
            return Err(StoreCause::Storage);
        }
        w.written = after;
        w.next += 1;
        w.last = now;
        if !last_part {
            return Ok(StoredBody::Taken { part: part.number });
        }
        self.finish(name)
    }

    /// The last part is in: check the SHA-256 of the whole, then sync the
    /// file, rename it over the old blob and sync the folder (BX-91).
    fn finish(&mut self, name: &str) -> Result<StoredBody, StoreCause> {
        let Some(w) = self.writes.remove(name) else {
            return Err(StoreCause::PartOutOfOrder);
        };
        let fail = |path: &Path, cause| {
            let _ = fs::remove_file(path);
            Err(cause)
        };
        let mut bytes = Vec::with_capacity(usize::try_from(w.written).unwrap_or(0));
        let read = File::open(&w.path).and_then(|mut f| f.read_to_end(&mut bytes));
        if read.is_err() {
            return fail(&w.path, StoreCause::Storage);
        }
        let sha = sha256_of(&bytes);
        if sha != w.sha256 {
            return fail(&w.path, StoreCause::WrongSha);
        }
        let place = w
            .file
            .sync_all()
            .and_then(|()| fs::rename(&w.path, self.blob_path(name)))
            .and_then(|()| sync_dir(&self.dir));
        if place.is_err() {
            return fail(&w.path, StoreCause::Storage);
        }
        self.blobs.insert(name.to_string(), w.written);
        self.cache = None;
        self.refused_full = false;
        Ok(StoredBody::Written { sha256: sha })
    }

    fn new_names(&self) -> usize {
        self.writes
            .keys()
            .filter(|n| !self.blobs.contains_key(n.as_str()))
            .count()
    }

    fn read(&mut self, name: &str, part: u32) -> Result<StoredBody, StoreCause> {
        if !self.blobs.contains_key(name) {
            return Ok(StoredBody::None);
        }
        if self.cache.as_ref().is_none_or(|(n, _, _)| n != name) {
            let bytes = fs::read(self.blob_path(name)).map_err(|_| StoreCause::Storage)?;
            let sha = sha256_of(&bytes);
            self.cache = Some((name.to_string(), bytes, sha));
        }
        let Some((_, bytes, sha)) = self.cache.as_ref() else {
            return Err(StoreCause::Storage);
        };
        let parts = bytes.len().div_ceil(MAX_PART_LEN).max(1);
        let at = usize::try_from(part).map_err(|_| StoreCause::PartOutOfOrder)?;
        if at >= parts {
            return Err(StoreCause::PartOutOfOrder);
        }
        let start = at * MAX_PART_LEN;
        let end = bytes.len().min(start + MAX_PART_LEN);
        Ok(StoredBody::Part(Part {
            number: part,
            parts: u32::try_from(parts).map_err(|_| StoreCause::Storage)?,
            sha256: *sha,
            bytes: bytes.get(start..end).unwrap_or_default().to_vec(),
        }))
    }

    fn list(&self, after: &str) -> StoredBody {
        let mut names = Vec::new();
        let mut total = 0usize;
        let mut more = false;
        let rest = self
            .blobs
            .keys()
            .filter(|n| after.is_empty() || n.as_str() > after);
        for name in rest {
            if total + 1 + name.len() > MAX_LIST_BYTES {
                more = true;
                break;
            }
            total += 1 + name.len();
            names.push(name.clone());
        }
        StoredBody::Names { names, more }
    }

    fn remove(&mut self, name: &str) -> Result<StoredBody, StoreCause> {
        self.drop_write(name);
        if self.blobs.contains_key(name) {
            fs::remove_file(self.blob_path(name)).map_err(|_| StoreCause::Storage)?;
            sync_dir(&self.dir).map_err(|_| StoreCause::Storage)?;
            self.blobs.remove(name);
            self.cache = None;
            self.refused_full = false;
        }
        Ok(StoredBody::Removed)
    }
}

/// The Box's store of Shop Lightning's state (BX-90 to BX-95).
pub struct ShopStore<F: BoxFiles> {
    files: F,
    root: PathBuf,
    identity: Identity,
    inbox: Inbox,
    outbox: Outbox,
    tenants: Vec<Tenant>,
    spaces: BTreeMap<DeviceId, Space>,
}

impl<F: BoxFiles> fmt::Debug for ShopStore<F> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ShopStore")
            .field("tenants", &self.tenants)
            .finish_non_exhaustive()
    }
}

impl<F: BoxFiles> ShopStore<F> {
    /// Open the store in the data directory `root` (its `lightning` folder
    /// is made when a counter is first served), with the small files in
    /// `files`. The Box's own key seals the answers.
    pub fn open(
        files: F,
        root: impl Into<PathBuf>,
        identity: Identity,
    ) -> Result<Self, StoreError> {
        let root: PathBuf = root.into();
        let tenants = match files
            .read(TENANTS_FILE)
            .map_err(io_err("the tenants file cannot be read"))?
        {
            Some(bytes) => {
                decode_tenants(&bytes).map_err(|_| StoreError::Damaged { file: TENANTS_FILE })?
            }
            None => Vec::new(),
        };
        let inbox = match files
            .read(STORE_INBOX_FILE)
            .map_err(io_err("the store's inbox cannot be read"))?
        {
            Some(bytes) => Inbox::from_bytes(&bytes).map_err(|_| StoreError::Damaged {
                file: STORE_INBOX_FILE,
            })?,
            None => Inbox::default(),
        };
        let outbox = match files
            .read(STORE_OUTBOX_FILE)
            .map_err(io_err("the store's outbox cannot be read"))?
        {
            Some(bytes) => Outbox::from_bytes(&bytes).map_err(|_| StoreError::Damaged {
                file: STORE_OUTBOX_FILE,
            })?,
            None => Outbox::default(),
        };
        Ok(Self {
            files,
            root,
            identity,
            inbox,
            outbox,
            tenants,
            spaces: BTreeMap::new(),
        })
    }

    /// The small files, for a test.
    #[cfg(test)]
    pub(crate) fn files_for_test(&self) -> &F {
        &self.files
    }

    fn counter_dir(&self, counter: &DeviceId) -> PathBuf {
        self.root.join(LIGHTNING_DIR).join(hex(&counter.to_bytes()))
    }

    fn persist_tenants(&mut self, tenants: &[Tenant]) -> Result<(), StoreError> {
        self.files
            .write(TENANTS_FILE, &encode_tenants(tenants))
            .map_err(io_err("the tenants file was not written"))
    }

    /// BX-90, BX-94: make the counters served what the settings in force
    /// say. `named` is the Lightning counter of the settings when they name
    /// this Box as its second device. A counter that was named and is not
    /// now is served 30 days more and then its folder is deleted; a counter
    /// named again within the 30 days is served as before and finds its
    /// state as it was.
    pub fn sync(&mut self, named: Option<DeviceId>, now: u64) -> Result<(), StoreError> {
        let mut next = self.tenants.clone();
        if let Some(c) = named {
            match next.iter_mut().find(|t| t.counter == c) {
                Some(t) => t.ends = 0,
                None => next.push(Tenant {
                    counter: c,
                    ends: 0,
                }),
            }
        }
        for t in &mut next {
            if Some(t.counter) != named && t.ends == 0 {
                t.ends = now.saturating_add(GRACE_SECS);
            }
        }
        let (kept, gone): (Vec<Tenant>, Vec<Tenant>) =
            next.into_iter().partition(|t| t.ends == 0 || now < t.ends);
        if kept != self.tenants || !gone.is_empty() {
            self.persist_tenants(&kept)?;
            self.tenants = kept;
        }
        for t in gone {
            self.spaces.remove(&t.counter);
            let dir = self.counter_dir(&t.counter);
            fs::remove_dir_all(&dir)
                .or_else(|e| {
                    if e.kind() == io::ErrorKind::NotFound {
                        Ok(())
                    } else {
                        Err(e)
                    }
                })
                .map_err(io_err("a counter's folder was not deleted"))?;
        }
        Ok(())
    }

    /// The counters served at `now`.
    pub fn served(&self, now: u64) -> Vec<DeviceId> {
        self.tenants
            .iter()
            .filter(|t| t.ends == 0 || now < t.ends)
            .map(|t| t.counter)
            .collect()
    }

    /// Whether the store of any counter served is full (BX-92): it is at a
    /// limit, or a write was refused for room and nothing has freed any
    /// since.
    pub fn is_full(&self) -> bool {
        self.spaces.values().any(|s| s.refused_full || s.at_limit())
    }

    /// The names a counter's folder holds, with their sizes (for a test and
    /// for the console).
    pub fn blobs(&self, counter: &DeviceId) -> Vec<(String, u64)> {
        self.spaces
            .get(counter)
            .map(|s| s.blobs.iter().map(|(n, l)| (n.clone(), *l)).collect())
            .unwrap_or_default()
    }

    fn space(&mut self, counter: &DeviceId) -> Result<&mut Space, StoreCause> {
        if !self.spaces.contains_key(counter) {
            let space = Space::open(self.counter_dir(counter)).map_err(|_| StoreCause::Storage)?;
            self.spaces.insert(*counter, space);
        }
        self.spaces.get_mut(counter).ok_or(StoreCause::Storage)
    }

    fn process(&mut self, counter: &DeviceId, message: &StoreMessage, now: u64) -> StoredBody {
        let space = match self.space(counter) {
            Ok(s) => s,
            Err(cause) => return StoredBody::Refused(cause),
        };
        space.expire(now);
        let result = match &message.body {
            StoreBody::Write { name, part } => space.write(name, part, now),
            StoreBody::Read { name, part } => space.read(name, *part),
            StoreBody::List { after } => Ok(space.list(after)),
            StoreBody::Remove { name } => space.remove(name),
        };
        result.unwrap_or_else(StoredBody::Refused)
    }

    /// Answer one sealed Store message (LK-82, BX-90): open it as the Box,
    /// from a counter served only; do what it asks; seal the Stored answer.
    /// The answer to a write says "written" only after the blob is in place
    /// (BX-91). `Refused` when the message does not open, is not from a
    /// counter served, is not a Store, or the Box cannot seal its answer.
    pub fn answer<E>(
        &mut self,
        sealed: &[u8],
        now: u64,
        random: &mut impl FnMut(&mut [u8; 32]) -> Result<(), E>,
    ) -> StoreAnswer {
        for space in self.spaces.values_mut() {
            space.expire(now);
        }
        let served = self.served(now);
        let me = self.identity.device_id();
        let files = &mut self.files;
        let opened = self.inbox.open_store(
            &self.identity,
            Role::Box,
            sealed,
            |d| served.contains(d).then_some(Role::Counter),
            |bytes| files.write(STORE_INBOX_FILE, bytes),
        );
        let Ok(opened) = opened else {
            return StoreAnswer::Refused;
        };
        let parties = StoreParties {
            counter: opened.sender,
            second_device: me,
        };
        if opened.kind != Kind::Store
            || !StoreParties::allows(Some(&parties), Kind::Store, &opened.sender, &me)
        {
            return StoreAnswer::Refused;
        }
        let Ok(message) = StoreMessage::from_bytes(&opened.content) else {
            return StoreAnswer::Refused;
        };
        let body = self.process(&opened.sender, &message, now);
        let stored = StoredMessage {
            request_id: message.request_id,
            action: message.action(),
            body,
        };
        let files = &mut self.files;
        let number = self.outbox.next(&opened.sender, |bytes| {
            files.write(STORE_OUTBOX_FILE, bytes)
        });
        let Ok(number) = number else {
            return StoreAnswer::Refused;
        };
        match seal(
            &self.identity,
            Role::Box,
            &opened.sender,
            Role::Counter,
            number,
            Kind::Stored,
            &stored.to_bytes(),
            random,
        ) {
            Ok(sealed) => StoreAnswer::Answer(sealed),
            Err(_) => StoreAnswer::Refused,
        }
    }
}
