//! The batches of addresses the owner has sent and the counter has not yet
//! shown as held (LK-70 to LK-72).
//!
//! The file holds each pending batch with the counter it was sent to and the
//! time it was first sent. It is written whole (a temporary file, a flush, a
//! rename, a flush of the directory) before anything is sent and after every
//! change, and read back strictly: a file that does not read, that is not in
//! the order it is written in or that does not re-encode to the same bytes is
//! refused whole.
//!
//! Layout: the magic `OBM!`, the version byte 1, the number of entries (u32
//! LE), and for each entry the counter (32 bytes), the time it was queued
//! (u64 LE), the length of the signed batch (u32 LE) and the bytes of
//! `SignedBatch::to_bytes`. Entries are in the order of the counter's bytes,
//! then of the first index of the batch, with no two of the same counter and
//! first index.

use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use openbmt_core::batch::SignedBatch;
use openbmt_core::identity::DeviceId;
use openbmt_core::wire::{self, DecodeError, Reader};

use crate::watcher::write_file;

/// The file under the link's directory.
pub const BATCH_OUTBOX_FILE: &str = "link-batches";

/// Most batches the file holds at once.
pub const MAX_PENDING_BATCHES: usize = 64;

const MAGIC: [u8; 4] = *b"OBM!";

/// One batch the owner is still waiting to see held.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingBatch {
    /// The counter it was sent to.
    pub counter: DeviceId,
    /// The batch, as it was signed.
    pub batch: SignedBatch,
    /// When it was first sent, by the clock the link's steps were given.
    pub queued_at: u64,
}

impl PendingBatch {
    fn key(&self) -> ([u8; 32], u32) {
        (self.counter.to_bytes(), self.batch.batch.start)
    }
}

/// Why the file could not be used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BatchOutboxError {
    /// The file could not be read or written.
    Io(String),
    /// The file is not in the form it is written in.
    Corrupt(&'static str),
    /// More batches than the file may hold.
    TooMany,
}

impl fmt::Display for BatchOutboxError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "LK-72: the pending batches: {e}"),
            Self::Corrupt(why) => write!(f, "LK-72: the file of pending batches is bad: {why}"),
            Self::TooMany => write!(
                f,
                "LK-72: more than {MAX_PENDING_BATCHES} batches are waiting to be held"
            ),
        }
    }
}

impl std::error::Error for BatchOutboxError {}

fn corrupt(e: DecodeError) -> BatchOutboxError {
    match e {
        DecodeError::NonCanonical(why) => BatchOutboxError::Corrupt(why),
        DecodeError::TrailingBytes => BatchOutboxError::Corrupt("bytes follow the last entry"),
        _ => BatchOutboxError::Corrupt("it is cut short or not framed as it is written"),
    }
}

/// The encoding of a list of entries. The list must be in order; the caller
/// keeps it so.
fn encode(items: &[PendingBatch]) -> Vec<u8> {
    let mut out = wire::start(&MAGIC, 8 + items.len() * 200);
    out.extend_from_slice(&wire::len_u32(items.len()).to_le_bytes());
    for item in items {
        out.extend_from_slice(&item.counter.to_bytes());
        out.extend_from_slice(&item.queued_at.to_le_bytes());
        let bytes = item.batch.to_bytes();
        out.extend_from_slice(&wire::len_u32(bytes.len()).to_le_bytes());
        out.extend_from_slice(&bytes);
    }
    out
}

/// Read the file strictly (LK-72).
fn decode(bytes: &[u8]) -> Result<Vec<PendingBatch>, BatchOutboxError> {
    let mut r = Reader::framed(bytes, &MAGIC).map_err(corrupt)?;
    let n = usize::try_from(r.u32_le().map_err(corrupt)?)
        .map_err(|_| BatchOutboxError::Corrupt("too many entries"))?;
    if n > MAX_PENDING_BATCHES {
        return Err(BatchOutboxError::TooMany);
    }
    let mut items: Vec<PendingBatch> = Vec::with_capacity(n);
    for _ in 0..n {
        let counter = r.device_id().map_err(corrupt)?;
        let queued_at = r.u64_le().map_err(corrupt)?;
        let len = usize::try_from(r.u32_le().map_err(corrupt)?)
            .map_err(|_| BatchOutboxError::Corrupt("a batch is too long"))?;
        let raw = r.take(len).map_err(corrupt)?;
        let batch = SignedBatch::from_bytes(raw).map_err(corrupt)?;
        let item = PendingBatch {
            counter,
            batch,
            queued_at,
        };
        if let Some(last) = items.last() {
            if last.key() >= item.key() {
                return Err(BatchOutboxError::Corrupt(
                    "the entries are not in order, or one is there twice",
                ));
            }
        }
        items.push(item);
    }
    r.finish().map_err(corrupt)?;
    if encode(&items) != bytes {
        return Err(BatchOutboxError::Corrupt(
            "it does not encode to the same bytes",
        ));
    }
    Ok(items)
}

/// The pending batches, in memory and in their file.
#[derive(Debug)]
pub struct BatchOutbox {
    dir: PathBuf,
    items: Vec<PendingBatch>,
}

impl BatchOutbox {
    /// Open the file under `dir`; no file is an empty list.
    pub fn open(dir: &Path) -> Result<Self, BatchOutboxError> {
        let items = match fs::read(dir.join(BATCH_OUTBOX_FILE)) {
            Ok(bytes) => decode(&bytes)?,
            Err(e) if e.kind() == io::ErrorKind::NotFound => Vec::new(),
            Err(e) => return Err(BatchOutboxError::Io(format!("read: {e}"))),
        };
        Ok(Self {
            dir: dir.to_path_buf(),
            items,
        })
    }

    /// What is pending, in the order of the file: by counter, then by the
    /// first index of the batch.
    pub fn items(&self) -> &[PendingBatch] {
        &self.items
    }

    /// Whether nothing is pending.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Replace the list with `next`, saved first. The list in memory is
    /// replaced only if the file was written.
    fn replace(&mut self, mut next: Vec<PendingBatch>) -> Result<(), BatchOutboxError> {
        next.sort_by_key(PendingBatch::key);
        if next.len() > MAX_PENDING_BATCHES {
            return Err(BatchOutboxError::TooMany);
        }
        write_file(&self.dir, BATCH_OUTBOX_FILE, &encode(&next)).map_err(BatchOutboxError::Io)?;
        self.items = next;
        Ok(())
    }

    /// Add a batch, or replace the one for the same counter and first index
    /// (keeping the time of the first). Saved before it returns.
    pub fn push(&mut self, item: PendingBatch) -> Result<(), BatchOutboxError> {
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
        mut keep: impl FnMut(&PendingBatch) -> bool,
    ) -> Result<usize, BatchOutboxError> {
        let next: Vec<PendingBatch> = self.items.iter().filter(|i| keep(i)).cloned().collect();
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

    fn pending(counter: u8, start: u32, at: u64) -> PendingBatch {
        use bitcoin::hashes::Hash;
        use bitcoin::{ScriptBuf, WScriptHash};
        use openbmt_core::batch::AddressBatch;
        use openbmt_core::identity::Identity;
        let id = |n: u8| Identity::from_secret_bytes(&[n; 32]).unwrap();
        let batch = AddressBatch {
            wallet_checksum: [9; 32],
            counter: id(counter).device_id(),
            start,
            count: 2,
            scripts: (1..=2u8)
                .map(|n| ScriptBuf::new_p2wsh(&WScriptHash::from_byte_array([n; 32])))
                .collect(),
        };
        PendingBatch {
            counter: id(counter).device_id(),
            batch: SignedBatch {
                owner_sig: batch.sign(&id(1)),
                approver_sig: batch.sign(&id(2)),
                batch,
            },
            queued_at: at,
        }
    }

    #[test]
    fn the_file_round_trips_byte_for_byte_and_refuses_every_cut_and_extra_byte() {
        let mut items = vec![pending(3, 0, 10), pending(3, 2, 11), pending(4, 0, 12)];
        items.sort_by_key(PendingBatch::key);
        let bytes = encode(&items);
        assert_eq!(decode(&bytes).unwrap(), items);
        for n in 0..bytes.len() {
            assert!(decode(&bytes[..n]).is_err(), "cut at {n}");
        }
        let mut longer = bytes.clone();
        longer.push(0);
        assert!(decode(&longer).is_err());
        let mut bad_magic = bytes.clone();
        bad_magic[3] = b'1';
        assert!(decode(&bad_magic).is_err());
        let mut bad_version = bytes;
        bad_version[4] = 2;
        assert!(decode(&bad_version).is_err());
    }

    #[test]
    fn entries_out_of_order_or_twice_are_refused() {
        let a = pending(3, 0, 1);
        let b = pending(3, 2, 1);
        assert!(decode(&encode(&[a.clone(), b.clone()])).is_ok());
        assert!(decode(&encode(&[b, a.clone()])).is_err());
        assert!(decode(&encode(&[a.clone(), a])).is_err());
    }

    #[test]
    fn a_count_above_the_most_is_refused_before_it_is_read() {
        let mut bytes = wire::start(&MAGIC, 4);
        bytes.extend_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(decode(&bytes), Err(BatchOutboxError::TooMany));
    }
}
