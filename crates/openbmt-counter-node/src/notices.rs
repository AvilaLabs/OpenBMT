//! The paid notices a counter sends to a Box (BX-40), and the file that
//! keeps them until the relay has taken them.
//!
//! A [`NoticeOutbox`] folds the counter's own records, oldest first, with
//! [`NoticeMaker`]: each time a sale changes state it makes a notice, gives it
//! the next number, signs it and queues it. The number, the queue and how far
//! each Box has been sent are stored whole in one file before any notice
//! leaves, and after a restart what the relay has not taken is sent again. The
//! Box refuses a number it has already taken, so a resend is harmless.
//!
//! Nothing in here touches the counter's sales or its records: the records
//! are read, never written. They are read in order through a
//! [`RecordSource`], a chunk at a time, so that a chain that is not all in
//! memory (CR-81) is folded without being held.
//!
//! Where the rules are silent:
//!
//! - With no Box in the courier's table nothing is queued and the records
//!   are not read: the file does not move. A Box added later is sent the
//!   notices of the whole chain, newest [`MAX_QUEUED`] of them.
//! - "Taken by the relay" is the only acknowledgement there is: the link has
//!   no kind for the Box to answer with.
//! - A queue above [`MAX_QUEUED`] drops its oldest notices and says so. A
//!   Box that misses one learns the state of the sale from the next notice
//!   about it, which carries the whole sale.

use std::collections::BTreeMap;
use std::fmt;
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use openbmt_core::identity::{DeviceId, Identity};
use openbmt_core::records::{Record, RecordKind};
use openbmt_core::wire::{self, DecodeError, Reader};
use openbmt_counter::{Counter, Event};
use openbmt_pos::{NoticeMaker, PaidNotice, PAID_NOTICE_LEN};

use crate::dirstore::DirStore;

/// The file in the link's directory.
pub const NOTICE_FILE: &str = "notice-outbox";
/// Most notices the queue holds.
pub const MAX_QUEUED: usize = 4_096;
/// Most Boxes the file remembers a send mark for.
pub const MAX_BOXES: usize = 64;
/// How many records are read at once when the chain is folded.
const FOLD_CHUNK: usize = 256;

/// Records that can be read in order, a few at a time.
pub trait RecordSource {
    /// How many records there are.
    fn records_len(&self) -> u64;

    /// Up to `max` records from number `from` on, oldest first. The text of
    /// the error is noted by the caller.
    fn read(&self, from: u64, max: usize) -> Result<Vec<Record>, String>;
}

impl RecordSource for [Record] {
    fn records_len(&self) -> u64 {
        self.len() as u64
    }

    fn read(&self, from: u64, max: usize) -> Result<Vec<Record>, String> {
        let from = usize::try_from(from).unwrap_or(usize::MAX);
        Ok(self.iter().skip(from).take(max).cloned().collect())
    }
}

/// The records of a node's counter: from its memory, and from the record
/// file below what memory holds (CR-81).
pub struct NodeRecords<'a> {
    pub(crate) counter: &'a Counter,
    pub(crate) store: &'a DirStore,
}

impl RecordSource for NodeRecords<'_> {
    fn records_len(&self) -> u64 {
        self.counter.records_len()
    }

    fn read(&self, from: u64, max: usize) -> Result<Vec<Record>, String> {
        self.counter
            .records_from(from, max, self.store)
            .map_err(|e| e.to_string())
    }
}

const MAGIC_NOTICE_OUTBOX: [u8; 4] = *b"OBMb";

/// Why the notice file could not be opened or written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NoticeStoreError {
    /// The file does not read back.
    Corrupt(DecodeError),
    /// The identity is not the one that signs the counter's records.
    NotThisCounter,
    /// The counter's records could not be read.
    Records(String),
    /// The file could not be read or written.
    Io {
        /// What was being done.
        op: &'static str,
        /// What the system said.
        kind: io::ErrorKind,
    },
}

impl fmt::Display for NoticeStoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Corrupt(e) => write!(f, "BX-40: {NOTICE_FILE} does not read back: {e}"),
            Self::NotThisCounter => write!(
                f,
                "BX-40: the notices are signed with a key that is not this counter's"
            ),
            Self::Records(e) => write!(f, "BX-40: the records could not be read: {e}"),
            Self::Io { op, kind } => write!(f, "BX-40: could not {op} {NOTICE_FILE}: {kind}"),
        }
    }
}

impl std::error::Error for NoticeStoreError {}

fn io_error(op: &'static str) -> impl FnOnce(io::Error) -> NoticeStoreError {
    move |e| NoticeStoreError::Io { op, kind: e.kind() }
}

/// Write the file whole: temporary file, flush, rename, flush the directory.
fn write_whole(dir: &Path, bytes: &[u8]) -> Result<(), NoticeStoreError> {
    let temp = dir.join(format!("{NOTICE_FILE}.tmp"));
    File::create(&temp)
        .and_then(|mut f| {
            f.write_all(bytes)?;
            f.sync_all()
        })
        .map_err(io_error("write"))?;
    fs::rename(&temp, dir.join(NOTICE_FILE)).map_err(io_error("replace"))?;
    File::open(dir)
        .and_then(|d| d.sync_all())
        .map_err(io_error("flush"))
}

/// What is stored: the numbers and the queue.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Held {
    /// The last number given.
    last_number: u64,
    /// How many of the counter's records have been folded.
    scanned: u64,
    /// For each Box, the last number the relay took.
    sent: BTreeMap<DeviceId, u64>,
    /// The notices not yet taken by every Box, by number.
    queue: Vec<PaidNotice>,
}

impl Held {
    fn to_bytes(&self) -> Vec<u8> {
        let mut out = wire::start(
            &MAGIC_NOTICE_OUTBOX,
            16 + self.sent.len() * 40 + self.queue.len() * PAID_NOTICE_LEN,
        );
        out.extend_from_slice(&self.last_number.to_le_bytes());
        out.extend_from_slice(&self.scanned.to_le_bytes());
        out.extend_from_slice(&wire::len_u32(self.sent.len()).to_le_bytes());
        for (b, n) in &self.sent {
            out.extend_from_slice(&b.to_bytes());
            out.extend_from_slice(&n.to_le_bytes());
        }
        out.extend_from_slice(&wire::len_u32(self.queue.len()).to_le_bytes());
        for n in &self.queue {
            out.extend_from_slice(&n.to_bytes());
        }
        out
    }

    fn from_bytes(bytes: &[u8]) -> Result<Self, DecodeError> {
        let mut r = Reader::framed(bytes, &MAGIC_NOTICE_OUTBOX)?;
        let last_number = r.u64_le()?;
        let scanned = r.u64_le()?;
        let mut sent = BTreeMap::new();
        for _ in 0..r.count(MAX_BOXES, 40)? {
            let b = r.device_id()?;
            let n = r.u64_le()?;
            if sent.last_key_value().is_some_and(|(k, _)| *k >= b) {
                return Err(DecodeError::NonCanonical("Boxes are not in key order"));
            }
            sent.insert(b, n);
        }
        let mut queue: Vec<PaidNotice> = Vec::new();
        for _ in 0..r.count(MAX_QUEUED, PAID_NOTICE_LEN)? {
            let n = PaidNotice::from_bytes(r.take(PAID_NOTICE_LEN)?)?;
            if queue.last().is_some_and(|p| p.number >= n.number) || n.number > last_number {
                return Err(DecodeError::NonCanonical("notice numbers are not rising"));
            }
            queue.push(n);
        }
        r.finish()?;
        Ok(Self {
            last_number,
            scanned,
            sent,
            queue,
        })
    }
}

/// The counter's notices: the fold of its records, the number, the queue.
pub struct NoticeOutbox {
    identity: Identity,
    dir: PathBuf,
    maker: NoticeMaker,
    held: Held,
    undecoded: u64,
}

impl fmt::Debug for NoticeOutbox {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "NoticeOutbox(last {}, {} queued)",
            self.held.last_number,
            self.held.queue.len()
        )
    }
}

/// Fold one record into the maker if it is one of the counter's events and
/// return the notice it calls for. An event that does not decode, or that
/// the fold refuses, is an error and changes nothing.
fn fold(
    maker: &mut NoticeMaker,
    me: &DeviceId,
    record: &Record,
) -> Result<Option<openbmt_pos::NoticeDraft>, ()> {
    let holds_events = matches!(
        record.kind,
        RecordKind::Sale | RecordKind::Payment | RecordKind::Override | RecordKind::Conflict
    );
    if &record.author != me || !holds_events {
        return Ok(None);
    }
    let event = Event::decode(&record.body).map_err(|_| ())?;
    maker.apply(&event).map_err(|_| ())
}

impl NoticeOutbox {
    /// Open the file in `dir` (made if there is none) for the counter whose
    /// identity this is, and fold the records the file says were already
    /// turned into notices, so that the next notice is the next change.
    pub fn open<R: RecordSource + ?Sized>(
        identity: Identity,
        dir: impl Into<PathBuf>,
        records: &R,
    ) -> Result<Self, NoticeStoreError> {
        let dir = dir.into();
        let me = identity.device_id();
        let held = match fs::read(dir.join(NOTICE_FILE)) {
            Ok(bytes) => Held::from_bytes(&bytes).map_err(NoticeStoreError::Corrupt)?,
            Err(e) if e.kind() == io::ErrorKind::NotFound => Held::default(),
            Err(e) => return Err(io_error("read")(e)),
        };
        if held.queue.iter().any(|n| n.counter != me) {
            return Err(NoticeStoreError::NotThisCounter);
        }
        let mut maker = NoticeMaker::new(me);
        let mut undecoded = 0;
        let upto = held.scanned.min(records.records_len());
        let mut next = 0u64;
        while next < upto {
            let want = usize::try_from(upto - next).unwrap_or(usize::MAX);
            let chunk = records
                .read(next, want.min(FOLD_CHUNK))
                .map_err(NoticeStoreError::Records)?;
            if chunk.is_empty() {
                return Err(NoticeStoreError::Records(format!(
                    "no record {next} though the chain has {upto}"
                )));
            }
            for record in chunk.iter().take(want) {
                if fold(&mut maker, &me, record).is_err() {
                    undecoded += 1;
                }
                next += 1;
            }
        }
        Ok(Self {
            identity,
            dir,
            maker,
            held,
            undecoded,
        })
    }

    /// The last number given.
    pub fn last_number(&self) -> u64 {
        self.held.last_number
    }

    /// The notices queued, by number.
    pub fn queue(&self) -> &[PaidNotice] {
        &self.held.queue
    }

    /// How many records were events that did not decode or that the fold
    /// refused, and so made no notice.
    pub fn undecoded(&self) -> u64 {
        self.undecoded
    }

    /// Fold the records not yet folded, queue the notices they call for and
    /// store the new state before it takes effect. Returns how many notices
    /// were dropped from the queue's front, if it was over the bound.
    pub fn collect<R: RecordSource + ?Sized>(
        &mut self,
        records: &R,
    ) -> Result<usize, NoticeStoreError> {
        let me = self.identity.device_id();
        let len = records.records_len();
        let from = self.held.scanned;
        if from >= len {
            return Ok(0);
        }
        let mut maker = self.maker.clone();
        let mut held = self.held.clone();
        let mut undecoded = self.undecoded;
        let mut next = from;
        let mut over = 0usize;
        while next < len {
            let want = usize::try_from(len - next).unwrap_or(usize::MAX);
            let chunk = records
                .read(next, want.min(FOLD_CHUNK))
                .map_err(NoticeStoreError::Records)?;
            if chunk.is_empty() {
                return Err(NoticeStoreError::Records(format!(
                    "no record {next} though the chain has {len}"
                )));
            }
            for record in chunk.iter().take(want) {
                match fold(&mut maker, &me, record) {
                    Ok(Some(draft)) => {
                        let number = held.last_number.saturating_add(1);
                        if let Some(notice) = PaidNotice::sign(&self.identity, number, &draft) {
                            held.last_number = number;
                            held.queue.push(notice);
                        }
                    }
                    Ok(None) => {}
                    Err(()) => undecoded += 1,
                }
                next += 1;
            }
            // The queue is cut as it grows, so that a long stretch of
            // records is not held as notices either.
            let cut = held.queue.len().saturating_sub(MAX_QUEUED);
            held.queue.drain(..cut);
            over += cut;
        }
        held.scanned = len;
        write_whole(&self.dir, &held.to_bytes())?;
        self.maker = maker;
        self.held = held;
        self.undecoded = undecoded;
        Ok(over)
    }

    /// The notices the relay has not yet taken for this Box, by number.
    pub fn unsent(&self, to: &DeviceId) -> Vec<PaidNotice> {
        let after = self.held.sent.get(to).copied().unwrap_or(0);
        self.held
            .queue
            .iter()
            .filter(|n| n.number > after)
            .cloned()
            .collect()
    }

    /// Record that the relay took the notices up to `number` for this Box.
    /// Only for memory: [`NoticeOutbox::settle`] stores it.
    pub fn mark_sent(&mut self, to: &DeviceId, number: u64) {
        let at = self.held.sent.entry(*to).or_insert(0);
        *at = (*at).max(number);
    }

    /// Forget the send marks of Boxes that are not in `boxes`, drop the
    /// notices every Box in it has taken, and store the result. With no Box
    /// in the list the queue is kept as it is.
    pub fn settle(&mut self, boxes: &[DeviceId]) -> Result<(), NoticeStoreError> {
        let mut held = self.held.clone();
        held.sent.retain(|b, _| boxes.contains(b));
        if let Some(taken) = boxes
            .iter()
            .map(|b| held.sent.get(b).copied().unwrap_or(0))
            .min()
        {
            held.queue.retain(|n| n.number > taken);
        }
        if held == self.held {
            return Ok(());
        }
        write_whole(&self.dir, &held.to_bytes())?;
        self.held = held;
        Ok(())
    }
}
