//! The switches the owner has sent to counters paired with the starter
//! wallet and the counter has not yet answered in its records (SS-41, LK-78).
//! It is the counterpart of [`crate::range_outbox`], kept and read the same
//! way.
//!
//! The file holds each pending switch with the time it was first queued. It
//! is written whole (a temporary file, a flush, a rename, a flush of the
//! directory) before anything is sent and after every change, and read back
//! strictly: a file that does not read, that is not in the order it is
//! written in or that does not re-encode to the same bytes is refused whole.
//!
//! Layout: the magic `OBM|`, the version byte 1, the number of entries (u32
//! LE), and for each entry the time it was queued (u64 LE), the length of
//! the switch (u32 LE) and the bytes of `SwitchOwn::to_bytes`. Entries are in
//! the order of the counter's bytes, with no two for the same counter and
//! new wallet.

use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use openbmt_core::pairing_own::SwitchOwn;
use openbmt_core::wire::{self, DecodeError, Reader};

use crate::watcher::write_file;

/// The file under the link's directory.
pub const SWITCH_OUTBOX_FILE: &str = "link-switches";

/// Most switches the file holds at once.
pub const MAX_PENDING_SWITCHES: usize = 64;

const MAGIC: [u8; 4] = *b"OBM|";

/// One switch the owner is still waiting to see answered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingSwitch {
    /// The switch, as it was signed.
    pub switch: SwitchOwn,
    /// When it was first queued, by the clock the link's steps were given.
    pub queued_at: u64,
}

impl PendingSwitch {
    fn key(&self) -> ([u8; 32], [u8; 32]) {
        (self.switch.counter.to_bytes(), self.switch.wallet_hash())
    }
}

/// Why the file could not be used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SwitchOutboxError {
    /// The file could not be read or written.
    Io(String),
    /// The file is not in the form it is written in.
    Corrupt(&'static str),
    /// More switches than the file may hold.
    TooMany,
}

impl fmt::Display for SwitchOutboxError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "LK-78: the pending switches: {e}"),
            Self::Corrupt(why) => write!(f, "LK-78: the file of pending switches is bad: {why}"),
            Self::TooMany => write!(
                f,
                "LK-78: more than {MAX_PENDING_SWITCHES} switches are waiting for an answer"
            ),
        }
    }
}

impl std::error::Error for SwitchOutboxError {}

fn corrupt(e: DecodeError) -> SwitchOutboxError {
    match e {
        DecodeError::NonCanonical(why) => SwitchOutboxError::Corrupt(why),
        DecodeError::TrailingBytes => SwitchOutboxError::Corrupt("bytes follow the last entry"),
        _ => SwitchOutboxError::Corrupt("it is cut short or not framed as it is written"),
    }
}

/// The encoding of a list of entries. The list must be in order; the caller
/// keeps it so.
fn encode(items: &[PendingSwitch]) -> Vec<u8> {
    let mut out = wire::start(&MAGIC, 8 + items.len() * 400);
    out.extend_from_slice(&wire::len_u32(items.len()).to_le_bytes());
    for item in items {
        out.extend_from_slice(&item.queued_at.to_le_bytes());
        let bytes = item.switch.to_bytes();
        out.extend_from_slice(&wire::len_u32(bytes.len()).to_le_bytes());
        out.extend_from_slice(&bytes);
    }
    out
}

/// Read the file strictly (LK-72).
fn decode(bytes: &[u8]) -> Result<Vec<PendingSwitch>, SwitchOutboxError> {
    let mut r = Reader::framed(bytes, &MAGIC).map_err(corrupt)?;
    let n = usize::try_from(r.u32_le().map_err(corrupt)?)
        .map_err(|_| SwitchOutboxError::Corrupt("too many entries"))?;
    if n > MAX_PENDING_SWITCHES {
        return Err(SwitchOutboxError::TooMany);
    }
    let mut items: Vec<PendingSwitch> = Vec::with_capacity(n);
    for _ in 0..n {
        let queued_at = r.u64_le().map_err(corrupt)?;
        let len = usize::try_from(r.u32_le().map_err(corrupt)?)
            .map_err(|_| SwitchOutboxError::Corrupt("a switch is too long"))?;
        let raw = r.take(len).map_err(corrupt)?;
        let switch = SwitchOwn::from_bytes(raw).map_err(corrupt)?;
        let item = PendingSwitch { switch, queued_at };
        if let Some(last) = items.last() {
            if last.key() >= item.key() {
                return Err(SwitchOutboxError::Corrupt(
                    "the entries are not in order, or one is there twice",
                ));
            }
        }
        items.push(item);
    }
    r.finish().map_err(corrupt)?;
    if encode(&items) != bytes {
        return Err(SwitchOutboxError::Corrupt(
            "it does not encode to the same bytes",
        ));
    }
    Ok(items)
}

/// The pending switches, in memory and in their file.
#[derive(Debug)]
pub struct SwitchOutbox {
    dir: PathBuf,
    items: Vec<PendingSwitch>,
}

impl SwitchOutbox {
    /// The list a file's bytes hold, read strictly (LK-72). For tests that
    /// feed it any bytes.
    pub fn from_bytes(dir: &Path, bytes: &[u8]) -> Result<Self, SwitchOutboxError> {
        Ok(Self {
            dir: dir.to_path_buf(),
            items: decode(bytes)?,
        })
    }

    /// The bytes of the file for the list in memory.
    pub fn to_bytes(&self) -> Vec<u8> {
        encode(&self.items)
    }

    /// Open the file under `dir`; no file is an empty list.
    pub fn open(dir: &Path) -> Result<Self, SwitchOutboxError> {
        let items = match fs::read(dir.join(SWITCH_OUTBOX_FILE)) {
            Ok(bytes) => decode(&bytes)?,
            Err(e) if e.kind() == io::ErrorKind::NotFound => Vec::new(),
            Err(e) => return Err(SwitchOutboxError::Io(format!("read: {e}"))),
        };
        Ok(Self {
            dir: dir.to_path_buf(),
            items,
        })
    }

    /// What is pending, in the order of the file.
    pub fn items(&self) -> &[PendingSwitch] {
        &self.items
    }

    /// Whether nothing is pending.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    fn replace(&mut self, mut next: Vec<PendingSwitch>) -> Result<(), SwitchOutboxError> {
        next.sort_by_key(PendingSwitch::key);
        if next.len() > MAX_PENDING_SWITCHES {
            return Err(SwitchOutboxError::TooMany);
        }
        write_file(&self.dir, SWITCH_OUTBOX_FILE, &encode(&next)).map_err(SwitchOutboxError::Io)?;
        self.items = next;
        Ok(())
    }

    /// Add a switch, or replace the one for the same counter and new wallet
    /// (keeping the time of the first). Saved before it returns.
    pub fn push(&mut self, item: PendingSwitch) -> Result<(), SwitchOutboxError> {
        let mut next = self.items.clone();
        let mut item = item;
        if let Some(at) = next.iter().position(|i| i.key() == item.key()) {
            let old = next.remove(at);
            item.queued_at = old.queued_at;
        }
        next.push(item);
        self.replace(next)
    }

    /// Keep only the entries for which `keep` is true. Saved only if one was
    /// dropped.
    pub fn retain(
        &mut self,
        mut keep: impl FnMut(&PendingSwitch) -> bool,
    ) -> Result<usize, SwitchOutboxError> {
        let next: Vec<PendingSwitch> = self.items.iter().filter(|i| keep(i)).cloned().collect();
        let dropped = self.items.len() - next.len();
        if dropped > 0 {
            self.replace(next)?;
        }
        Ok(dropped)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use openbmt_core::identity::Identity;
    use openbmt_core::pinned_wallet::{IndexRange, PinnedWallet, SignedRange};

    const OLD: &str = "wpkh([818bc9b6/84h/0h/0h]xpub6CgURS1GrKZkAoRGBTKYX9FhLgkKnQVZ6vdNBUbiu24xtRJ6RzLKpdg4CqsVyBLzimcJsviP59hSoGJEpTJiXE8bdBLw1qPC1DXRatKehb8/<0;1>/*)#96yh9yhx";
    const NEW: &str = "wsh(sortedmulti(2,[73c5da0a/48h/0h/0h/2h]xpub6DkFAXWQ2dHxq2vatrt9qyA3bXYU4ToWQwCHbf5XB2mSTexcHZCeKS1VZYcPoBd5X8yVcbXFHJR9R8UCVpt82VX1VhR28mCyxUFL4r6KFrf/<0;1>/*,[b8688df1/48h/0h/0h/2h]xpub6FQya7zGhR92kacYsNnjreouvnHJMpXYsUXnW6NJJAJRCKsa26TzDy4LdnGhEurr3d6y1J8PJ7EEMKQp74XTqYvmGJNogYXSKDszYHtF8mX/<0;1>/*,[28645006/48h/0h/0h/2h]xpub6DnEBNkSJKBYQmsbhS1sP9cNdtU5c9PLFGCjTJmxicxc13WB8zNNGQazabQpyFAGW5bV9tMko4uBxDxjUKL6dSAcx1tEbgEHtgSqyRsekh6/<0;1>/*))#34zcp4yt";

    fn id(n: u8) -> Identity {
        Identity::from_secret_bytes(&[n; 32]).unwrap()
    }

    fn pending(counter: u8, at: u64) -> PendingSwitch {
        let c = id(counter).device_id();
        let range = SignedRange::sign(&id(1), IndexRange::new(c, 1_000, 2_000).unwrap()).unwrap();
        let old = PinnedWallet::from_text(OLD).unwrap();
        let new = PinnedWallet::from_text(NEW).unwrap();
        PendingSwitch {
            switch: SwitchOwn::sign(&id(1), c, old.checksum32(), new, range),
            queued_at: at,
        }
    }

    #[test]
    fn lk_78_the_file_round_trips_byte_for_byte_and_refuses_every_cut_and_extra_byte() {
        let mut items = vec![pending(3, 10), pending(4, 11)];
        items.sort_by_key(PendingSwitch::key);
        let bytes = encode(&items);
        assert_eq!(bytes.get(..4), Some(&b"OBM|"[..]));
        assert_eq!(decode(&bytes).unwrap(), items);
        for n in 0..bytes.len() {
            assert!(decode(&bytes[..n]).is_err(), "cut at {n}");
        }
        let mut longer = bytes.clone();
        longer.push(0);
        assert!(decode(&longer).is_err());
        let mut bad_magic = bytes.clone();
        bad_magic[3] = b'!';
        assert!(decode(&bad_magic).is_err());
        let mut bad_version = bytes;
        bad_version[4] = 2;
        assert!(decode(&bad_version).is_err());
    }

    #[test]
    fn lk_78_entries_out_of_order_or_twice_are_refused() {
        let mut two = vec![pending(3, 1), pending(4, 1)];
        two.sort_by_key(PendingSwitch::key);
        assert!(decode(&encode(&two)).is_ok());
        two.reverse();
        assert!(decode(&encode(&two)).is_err());
        let a = pending(3, 1);
        assert!(decode(&encode(&[a.clone(), a])).is_err());
    }

    #[test]
    fn lk_78_a_count_above_the_most_is_refused_before_it_is_read() {
        let mut bytes = wire::start(&MAGIC, 4);
        bytes.extend_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(decode(&bytes), Err(SwitchOutboxError::TooMany));
    }

    #[test]
    fn lk_78_a_push_keeps_the_first_time_and_the_file_is_written_before_it_returns() {
        let dir = std::env::temp_dir().join(format!("switch-outbox-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut outbox = SwitchOutbox::open(&dir).unwrap();
        assert!(outbox.is_empty());
        outbox.push(pending(3, 10)).unwrap();
        outbox.push(pending(3, 50)).unwrap();
        assert_eq!(outbox.items().len(), 1);
        assert_eq!(outbox.items()[0].queued_at, 10);
        let again = SwitchOutbox::open(&dir).unwrap();
        assert_eq!(again.items(), outbox.items());
        assert_eq!(outbox.retain(|_| false).unwrap(), 1);
        assert!(SwitchOutbox::open(&dir).unwrap().is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
