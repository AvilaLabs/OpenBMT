//! The counter's records, as the counter keeps them in memory (CR-81).
//!
//! Every record is in the store's record log. The counter holds the length
//! of the chain, its head and the records from `base` up; the ones below
//! `base` are read back from the store when asked, and what comes back is
//! checked (CR-42).

use std::collections::VecDeque;

use openbmt_core::identity::{DeviceId, Identity};
use openbmt_core::records::{ChainError, Record, RecordKind, GENESIS};

use crate::store::{Store, StoreFault};

/// CR-81: how many of the newest records stay in memory whatever the owner's
/// mark is **[to tune]**.
pub const KEEP_RECORDS: u64 = 1_024;
/// CR-81: most records above the owner's mark that stay in memory. When the
/// owner has taken less than this many records short of the head, the newest
/// this many stay and the older ones are read from the store when they are
/// sent **[to tune]**.
pub const MAX_UNSENT_IN_MEMORY: u64 = 16_384;
/// CR-81: most records one call of `records_from` returns.
pub const MAX_READ: usize = 1_024;
/// How many records are asked of the store at once.
pub(crate) const READ_CHUNK: usize = 256;

/// The chain's length and head, and its records from `base` up.
#[derive(Debug, Clone)]
pub(crate) struct RecordLog {
    base: u64,
    mem: VecDeque<Record>,
    len: u64,
    head: [u8; 32],
}

impl RecordLog {
    pub(crate) fn new() -> Self {
        Self {
            base: 0,
            mem: VecDeque::new(),
            len: 0,
            head: GENESIS,
        }
    }

    pub(crate) fn len(&self) -> u64 {
        self.len
    }

    pub(crate) fn head(&self) -> [u8; 32] {
        self.head
    }

    pub(crate) fn base(&self) -> u64 {
        self.base
    }

    pub(crate) fn in_memory(&self) -> usize {
        self.mem.len()
    }

    /// Build and sign the record that comes next, without adding it.
    pub(crate) fn next(
        &self,
        author: &Identity,
        kind: RecordKind,
        body: Vec<u8>,
    ) -> Result<Record, ChainError> {
        Record::build(author, self.len, self.head, kind, body)
    }

    /// Add the record `next` built, once it is stored.
    pub(crate) fn push(&mut self, record: Record) {
        self.head = record.hash();
        self.len = self.len.saturating_add(1);
        self.mem.push_back(record);
    }

    /// Where `base` should be for this owner's mark: the older of the mark
    /// and the newest `KEEP_RECORDS`, but never more than
    /// `MAX_UNSENT_IN_MEMORY` records back. A mark that is not known counts
    /// as zero.
    fn target_base(&self, mark: Option<u64>) -> u64 {
        let above_mark = self.len.saturating_sub(mark.unwrap_or(0));
        let held = above_mark.clamp(KEEP_RECORDS, MAX_UNSENT_IN_MEMORY);
        self.len.saturating_sub(held)
    }

    /// Move `base` up to where the owner's mark allows. It never moves
    /// down: what left memory is read from the store.
    pub(crate) fn trim(&mut self, mark: Option<u64>) {
        let target = self.target_base(mark);
        while self.base < target && self.mem.pop_front().is_some() {
            self.base = self.base.saturating_add(1);
        }
    }

    fn in_mem(&self, n: u64) -> Option<&Record> {
        let at = usize::try_from(n.checked_sub(self.base)?).ok()?;
        self.mem.get(at)
    }

    /// Read `count` records from the store, from number `from`, and check
    /// them: they parse, they are the numbers asked for, this counter signed
    /// them, each follows the one before, and the last one before `base` is
    /// the one the first in memory follows.
    fn read_disk<S: Store>(
        &self,
        store: &S,
        me: &DeviceId,
        from: u64,
        count: u64,
    ) -> Result<Vec<Record>, StoreFault<S::Error>> {
        let mut out: Vec<Record> = Vec::new();
        let mut next = from;
        let end = from.saturating_add(count);
        while next < end {
            let want = usize::try_from(end - next).unwrap_or(usize::MAX);
            let chunk = store
                .read_records(next, want.min(READ_CHUNK))
                .map_err(StoreFault::Read)?;
            if chunk.is_empty() {
                return Err(StoreFault::Damaged {
                    seq: next,
                    why: "the record is missing",
                });
            }
            for bytes in chunk.iter().take(want) {
                let seq = next;
                let damaged = |why| StoreFault::Damaged { seq, why };
                let record =
                    Record::from_bytes(bytes).map_err(|_| damaged("it does not read back"))?;
                if record.seq != seq {
                    return Err(damaged("it is not the record asked for"));
                }
                if &record.author != me || !record.signature_ok() {
                    return Err(damaged("it is not signed by this counter"));
                }
                let before = match out.last() {
                    Some(last) => Some(last.hash()),
                    None if seq == 0 => Some(GENESIS),
                    None => None,
                };
                if before.is_some_and(|hash| hash != record.prev) {
                    return Err(damaged("it does not follow the one before"));
                }
                out.push(record);
                next = next.saturating_add(1);
            }
        }
        if let (Some(last), Some(first_in_memory)) = (out.last(), self.mem.front()) {
            if last.seq.saturating_add(1) == self.base && last.hash() != first_in_memory.prev {
                return Err(StoreFault::Damaged {
                    seq: last.seq,
                    why: "the record in memory after it does not follow it",
                });
            }
        }
        Ok(out)
    }

    /// The record numbered `n`, from memory or from the store.
    pub(crate) fn record_at<S: Store>(
        &self,
        store: &S,
        me: &DeviceId,
        n: u64,
    ) -> Result<Option<Record>, StoreFault<S::Error>> {
        if n >= self.len {
            return Ok(None);
        }
        if let Some(record) = self.in_mem(n) {
            return Ok(Some(record.clone()));
        }
        Ok(self.read_disk(store, me, n, 1)?.into_iter().next())
    }

    /// Up to `max` records from number `from`, oldest first.
    pub(crate) fn records_from<S: Store>(
        &self,
        store: &S,
        me: &DeviceId,
        from: u64,
        max: usize,
    ) -> Result<Vec<Record>, StoreFault<S::Error>> {
        let max = max.min(MAX_READ);
        if from >= self.len || max == 0 {
            return Ok(Vec::new());
        }
        let end = self.len.min(from.saturating_add(max as u64));
        let mut out = Vec::new();
        if from < self.base {
            out = self.read_disk(store, me, from, end.min(self.base) - from)?;
        }
        if end > self.base {
            let start = from.max(self.base);
            let skip = usize::try_from(start - self.base).unwrap_or(usize::MAX);
            let take = usize::try_from(end - start).unwrap_or(usize::MAX);
            out.extend(self.mem.iter().skip(skip).take(take).cloned());
        }
        Ok(out)
    }

    /// LK-55: the hash of the record before number `n`, which is the head
    /// when `n` is the length and [`GENESIS`] when `n` is zero.
    pub(crate) fn hash_at<S: Store>(
        &self,
        store: &S,
        me: &DeviceId,
        n: u64,
    ) -> Result<Option<[u8; 32]>, StoreFault<S::Error>> {
        if n > self.len {
            return Ok(None);
        }
        let Some(last) = n.checked_sub(1) else {
            return Ok(Some(GENESIS));
        };
        Ok(self.record_at(store, me, last)?.map(|r| r.hash()))
    }

    /// Start from records already verified: the length, the head and the
    /// newest of them.
    pub(crate) fn from_parts(len: u64, head: [u8; 32], tail: VecDeque<Record>) -> Self {
        let base = len.saturating_sub(tail.len() as u64);
        Self {
            base,
            mem: tail,
            len,
            head,
        }
    }
}
