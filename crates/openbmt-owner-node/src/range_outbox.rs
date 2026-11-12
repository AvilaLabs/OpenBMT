//! The ranges of indexes the owner has sent to a counter paired with the
//! shop's own wallet and the counter has not yet shown as held (OA-112,
//! LK-77). It is the counterpart of [`crate::batch_outbox`], kept and read
//! the same way.
//!
//! The file holds each pending range with the counter it was sent to and the
//! time it was first sent. It is written whole (a temporary file, a flush, a
//! rename, a flush of the directory) before anything is sent and after every
//! change, and read back strictly: a file that does not read, that is not in
//! the order it is written in or that does not re-encode to the same bytes is
//! refused whole.
//!
//! Layout: the magic `OBM>`, the version byte 1, the number of entries (u32
//! LE), and for each entry the counter (32 bytes), the time it was queued
//! (u64 LE), the length of the signed range (u32 LE) and the bytes of
//! `SignedRange::to_bytes`. Entries are in the order of the counter's bytes,
//! then of the first index of the range, with no two of the same counter and
//! first index.

use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use openbmt_core::identity::DeviceId;
use openbmt_core::pinned_wallet::SignedRange;
use openbmt_core::wire::{self, DecodeError, Reader};

use crate::watcher::write_file;

/// The file under the link's directory.
pub const RANGE_OUTBOX_FILE: &str = "link-ranges";

/// Most ranges the file holds at once.
pub const MAX_PENDING_RANGES: usize = 64;

const MAGIC: [u8; 4] = *b"OBM>";

/// One range the owner is still waiting to see held.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingRange {
    /// The counter it was sent to.
    pub counter: DeviceId,
    /// The range, as it was signed.
    pub range: SignedRange,
    /// When it was first sent, by the clock the link's steps were given.
    pub queued_at: u64,
}

impl PendingRange {
    fn key(&self) -> ([u8; 32], u32) {
        (self.counter.to_bytes(), self.range.range.start)
    }
}

/// Why the file could not be used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RangeOutboxError {
    /// The file could not be read or written.
    Io(String),
    /// The file is not in the form it is written in.
    Corrupt(&'static str),
    /// More ranges than the file may hold.
    TooMany,
}

impl fmt::Display for RangeOutboxError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "LK-77: the pending ranges: {e}"),
            Self::Corrupt(why) => write!(f, "LK-77: the file of pending ranges is bad: {why}"),
            Self::TooMany => write!(
                f,
                "LK-77: more than {MAX_PENDING_RANGES} ranges are waiting to be held"
            ),
        }
    }
}

impl std::error::Error for RangeOutboxError {}

fn corrupt(e: DecodeError) -> RangeOutboxError {
    match e {
        DecodeError::NonCanonical(why) => RangeOutboxError::Corrupt(why),
        DecodeError::TrailingBytes => RangeOutboxError::Corrupt("bytes follow the last entry"),
        _ => RangeOutboxError::Corrupt("it is cut short or not framed as it is written"),
    }
}

/// The encoding of a list of entries. The list must be in order; the caller
/// keeps it so.
fn encode(items: &[PendingRange]) -> Vec<u8> {
    let mut out = wire::start(&MAGIC, 8 + items.len() * 200);
    out.extend_from_slice(&wire::len_u32(items.len()).to_le_bytes());
    for item in items {
        out.extend_from_slice(&item.counter.to_bytes());
        out.extend_from_slice(&item.queued_at.to_le_bytes());
        let bytes = item.range.to_bytes();
        out.extend_from_slice(&wire::len_u32(bytes.len()).to_le_bytes());
        out.extend_from_slice(&bytes);
    }
    out
}

/// Read the file strictly (LK-72).
fn decode(bytes: &[u8]) -> Result<Vec<PendingRange>, RangeOutboxError> {
    let mut r = Reader::framed(bytes, &MAGIC).map_err(corrupt)?;
    let n = usize::try_from(r.u32_le().map_err(corrupt)?)
        .map_err(|_| RangeOutboxError::Corrupt("too many entries"))?;
    if n > MAX_PENDING_RANGES {
        return Err(RangeOutboxError::TooMany);
    }
    let mut items: Vec<PendingRange> = Vec::with_capacity(n);
    for _ in 0..n {
        let counter = r.device_id().map_err(corrupt)?;
        let queued_at = r.u64_le().map_err(corrupt)?;
        let len = usize::try_from(r.u32_le().map_err(corrupt)?)
            .map_err(|_| RangeOutboxError::Corrupt("a range is too long"))?;
        let raw = r.take(len).map_err(corrupt)?;
        let range = SignedRange::from_bytes(raw).map_err(corrupt)?;
        let item = PendingRange {
            counter,
            range,
            queued_at,
        };
        if let Some(last) = items.last() {
            if last.key() >= item.key() {
                return Err(RangeOutboxError::Corrupt(
                    "the entries are not in order, or one is there twice",
                ));
            }
        }
        items.push(item);
    }
    r.finish().map_err(corrupt)?;
    if encode(&items) != bytes {
        return Err(RangeOutboxError::Corrupt(
            "it does not encode to the same bytes",
        ));
    }
    Ok(items)
}

/// The pending ranges, in memory and in their file.
#[derive(Debug)]
pub struct RangeOutbox {
    dir: PathBuf,
    items: Vec<PendingRange>,
}

impl RangeOutbox {
    /// The list a file's bytes hold, read strictly (LK-72), and what it
    /// encodes to. For `open` and for tests that feed it any bytes.
    pub fn from_bytes(dir: &Path, bytes: &[u8]) -> Result<Self, RangeOutboxError> {
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
    pub fn open(dir: &Path) -> Result<Self, RangeOutboxError> {
        let items = match fs::read(dir.join(RANGE_OUTBOX_FILE)) {
            Ok(bytes) => decode(&bytes)?,
            Err(e) if e.kind() == io::ErrorKind::NotFound => Vec::new(),
            Err(e) => return Err(RangeOutboxError::Io(format!("read: {e}"))),
        };
        Ok(Self {
            dir: dir.to_path_buf(),
            items,
        })
    }

    /// What is pending, in the order of the file: by counter, then by the
    /// first index of the range.
    pub fn items(&self) -> &[PendingRange] {
        &self.items
    }

    /// Whether nothing is pending.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Replace the list with `next`, saved first. The list in memory is
    /// replaced only if the file was written.
    fn replace(&mut self, mut next: Vec<PendingRange>) -> Result<(), RangeOutboxError> {
        next.sort_by_key(PendingRange::key);
        if next.len() > MAX_PENDING_RANGES {
            return Err(RangeOutboxError::TooMany);
        }
        write_file(&self.dir, RANGE_OUTBOX_FILE, &encode(&next)).map_err(RangeOutboxError::Io)?;
        self.items = next;
        Ok(())
    }

    /// Add a range, or replace the one for the same counter and first index
    /// (keeping the time of the first). Saved before it returns.
    pub fn push(&mut self, item: PendingRange) -> Result<(), RangeOutboxError> {
        let mut next = self.items.clone();
        let mut item = item;
        if let Some(at) = next.iter().position(|i| i.key() == item.key()) {
            let old = next.remove(at);
            item.queued_at = old.queued_at;
        }
        next.push(item);
        self.replace(next)
    }

    /// Keep only the entries for which `keep` is true. Saved only if one
    /// was dropped.
    pub fn retain(
        &mut self,
        mut keep: impl FnMut(&PendingRange) -> bool,
    ) -> Result<usize, RangeOutboxError> {
        let next: Vec<PendingRange> = self.items.iter().filter(|i| keep(i)).cloned().collect();
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
    use openbmt_core::pinned_wallet::IndexRange;

    fn id(n: u8) -> Identity {
        Identity::from_secret_bytes(&[n; 32]).unwrap()
    }

    fn pending(counter: u8, start: u32, at: u64) -> PendingRange {
        let range = IndexRange::new(id(counter).device_id(), start, start + 1_000).unwrap();
        PendingRange {
            counter: id(counter).device_id(),
            range: SignedRange::sign(&id(1), range).unwrap(),
            queued_at: at,
        }
    }

    #[test]
    fn the_file_round_trips_byte_for_byte_and_refuses_every_cut_and_extra_byte() {
        let mut items = vec![
            pending(3, 0, 10),
            pending(3, 2_000, 11),
            pending(4, 1_000, 12),
        ];
        items.sort_by_key(PendingRange::key);
        let bytes = encode(&items);
        assert_eq!(bytes.get(..4), Some(&b"OBM>"[..]));
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
    fn entries_out_of_order_or_twice_are_refused() {
        let a = pending(3, 0, 1);
        let b = pending(3, 2_000, 1);
        assert!(decode(&encode(&[a.clone(), b.clone()])).is_ok());
        assert!(decode(&encode(&[b, a.clone()])).is_err());
        assert!(decode(&encode(&[a.clone(), a])).is_err());
    }

    #[test]
    fn a_count_above_the_most_is_refused_before_it_is_read() {
        let mut bytes = wire::start(&MAGIC, 4);
        bytes.extend_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(decode(&bytes), Err(RangeOutboxError::TooMany));
    }

    #[test]
    fn a_range_that_does_not_decode_is_refused() {
        let mut bytes = wire::start(&MAGIC, 64);
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(&id(3).device_id().to_bytes());
        bytes.extend_from_slice(&0u64.to_le_bytes());
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(&[1, 2, 3]);
        assert!(decode(&bytes).is_err());
    }
}
