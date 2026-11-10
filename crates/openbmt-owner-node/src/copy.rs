//! The owner's copy of one counter's chain (LK-58 to LK-60).
//!
//! The records go to `<dir>/counters/<device id in hex>/records` as a log that
//! only grows: each record is its length (4 bytes, little endian) and the
//! bytes of `Record::to_bytes`, written in one call and flushed before the
//! record counts, as the counter node does with its own file. A last record
//! that a crash left cut short is cut off when the copy is opened, and
//! reported. The latest check-in is a small file next to it (`check-in`),
//! written whole to a temporary file and renamed.
//!
//! The copy is the owner's evidence: a record is taken only if it extends the
//! copy exactly (LK-58), and nothing the copy holds is ever overwritten
//! (LK-59). When the whole copy is read back at the start it is checked again
//! from the first record, so a file that was edited is found.

use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use openbmt_core::identity::{DeviceId, TAG_RECORD};
use openbmt_core::records::{Chain, ChainError, DeviceList, Record, RecordKind, GENESIS, MAX_BODY};
use openbmt_counter_node::TornTail;

/// The file of the records, in the counter's directory.
pub const RECORDS_FILE: &str = "records";
/// The file of the latest check-in.
pub const CHECK_IN_FILE: &str = "check-in";

/// The longest record the file takes, as in the counter node's file.
const MAX_RECORD_BYTES: usize = MAX_BODY + 256;

/// Why a copy could not be opened or written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CopyError {
    /// The operating system refused an operation on a file.
    Io {
        /// The file.
        file: &'static str,
        /// What was being done.
        op: &'static str,
        /// What the system said.
        kind: io::ErrorKind,
    },
    /// A file does not read back as what was written.
    Corrupt {
        /// The file.
        file: &'static str,
        /// What is wrong with it.
        why: &'static str,
    },
    /// The stored records are not a valid chain of the shop's devices.
    NotAChain(ChainError),
}

impl fmt::Display for CopyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { file, op, kind } => write!(f, "LK-58: could not {op} {file}: {kind}"),
            Self::Corrupt { file, why } => {
                write!(f, "LK-58: {file} cannot be read back: {why}")
            }
            Self::NotAChain(e) => {
                write!(f, "LK-58: the stored records are not a valid chain: {e}")
            }
        }
    }
}

impl std::error::Error for CopyError {}

fn io_err(file: &'static str, op: &'static str) -> impl FnOnce(io::Error) -> CopyError {
    move |e| CopyError::Io {
        file,
        op,
        kind: e.kind(),
    }
}

/// Why a message stopped at a record (LK-58, LK-59).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stop {
    /// The record's number is above the copy's length: records between are
    /// missing.
    Gap {
        /// The number the copy needs next.
        expected: u64,
        /// The number the record has.
        found: u64,
    },
    /// The author is not on the signed device list.
    UnknownAuthor {
        /// The record's number.
        seq: u64,
    },
    /// The signature is not the author's.
    BadSignature {
        /// The record's number.
        seq: u64,
    },
    /// The body is above the largest.
    BodyTooLarge {
        /// The record's number.
        seq: u64,
    },
    /// A check-in whose body does not say what its own place says.
    BadCheckIn {
        /// The record's number.
        seq: u64,
    },
    /// The record does not follow the copy's last record (a divergence when
    /// the copy holds one).
    PrevMismatch {
        /// The record's number.
        seq: u64,
    },
    /// The copy holds another record with this number (LK-59).
    Diverged {
        /// The record's number.
        seq: u64,
    },
    /// The record could not be written to the copy's file.
    Store(CopyError),
}

impl fmt::Display for Stop {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Gap { expected, found } => write!(
                f,
                "LK-58: record {found} came where record {expected} was needed"
            ),
            Self::UnknownAuthor { seq } => {
                write!(f, "LK-58: record {seq} is from a device not on the list")
            }
            Self::BadSignature { seq } => write!(f, "LK-58: record {seq} has a bad signature"),
            Self::BodyTooLarge { seq } => write!(f, "LK-58: record {seq} has a body too large"),
            Self::BadCheckIn { seq } => {
                write!(f, "LK-58: check-in {seq} does not match its place")
            }
            Self::PrevMismatch { seq } => {
                write!(
                    f,
                    "LK-58: record {seq} does not follow the copy's last record"
                )
            }
            Self::Diverged { seq } => write!(
                f,
                "LK-59: the copy holds another record {seq}: nothing was overwritten"
            ),
            Self::Store(e) => write!(f, "{e}"),
        }
    }
}

/// What taking the records of one message did (LK-58, LK-59).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Taken {
    /// Records added to the copy.
    pub appended: u64,
    /// Records the copy already held, skipped without a word.
    pub skipped: u64,
    /// The number of the record at which the counter's chain and the copy
    /// differ, if they do (LK-59).
    pub diverged: Option<u64>,
    /// Why the rest of the message was not taken, if it was not.
    pub stopped: Option<Stop>,
}

/// What taking a check-in did (LK-60).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckInTaken {
    /// It is genuine, and it is now the latest. `matches` is true when the
    /// copy holds it at its place; false when the copy is still too short.
    Taken {
        /// Whether the copy already holds the check-in's record.
        matches: bool,
    },
    /// It is genuine and the copy holds a record differently from what it
    /// says (LK-59). It is kept as the latest, as evidence.
    Diverged {
        /// The number of the record the copy holds differently.
        seq: u64,
    },
    /// It has a lower number than the latest kept, and was ignored.
    Older,
    /// It is not a genuine check-in of this counter.
    Refused(ChainError),
    /// It could not be written.
    Store(CopyError),
}

/// The owner's copy of one counter's chain. See the module docs.
#[derive(Debug)]
pub struct CounterCopy {
    dir: PathBuf,
    counter: DeviceId,
    records: Vec<Record>,
    head: [u8; 32],
    check_in: Option<Record>,
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

impl CounterCopy {
    /// Open the copy of `counter`'s chain under `root` (the owner node's
    /// directory), making its directory if it is not there. The records are
    /// read back and checked from the first (LK-58). A last record that was
    /// cut short is cut off and reported.
    pub fn open(
        root: &Path,
        counter: DeviceId,
        devices: &DeviceList,
    ) -> Result<(Self, Option<TornTail>), CopyError> {
        let dir = root.join("counters").join(hex(&counter.to_bytes()));
        fs::create_dir_all(&dir).map_err(io_err(RECORDS_FILE, "create the directory of"))?;
        let mut copy = Self {
            dir,
            counter,
            records: Vec::new(),
            head: GENESIS,
            check_in: None,
        };
        let torn = copy.load_records()?;
        Chain::from_records(copy.records.clone())
            .verify(devices)
            .map_err(CopyError::NotAChain)?;
        copy.head = copy.records.last().map_or(GENESIS, Record::hash);
        if let Some(bytes) = copy.read(CHECK_IN_FILE)? {
            let record = Record::from_bytes(&bytes).map_err(|_| CopyError::Corrupt {
                file: CHECK_IN_FILE,
                why: "it is not a record",
            })?;
            if record.kind != RecordKind::CheckIn || record.author != counter {
                return Err(CopyError::Corrupt {
                    file: CHECK_IN_FILE,
                    why: "it is not a check-in of this counter",
                });
            }
            copy.check_in = Some(record);
        }
        Ok((copy, torn))
    }

    fn read(&self, file: &'static str) -> Result<Option<Vec<u8>>, CopyError> {
        match fs::read(self.dir.join(file)) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(io_err(file, "read")(e)),
        }
    }

    fn load_records(&mut self) -> Result<Option<TornTail>, CopyError> {
        let Some(bytes) = self.read(RECORDS_FILE)? else {
            return Ok(None);
        };
        let mut rest = bytes.as_slice();
        let mut kept = 0u64;
        let mut torn = false;
        while !rest.is_empty() {
            let Some((len_bytes, after)) = rest.split_first_chunk::<4>() else {
                torn = true;
                break;
            };
            let len = usize::try_from(u32::from_le_bytes(*len_bytes)).map_err(|_| {
                CopyError::Corrupt {
                    file: RECORDS_FILE,
                    why: "a record has an impossible length",
                }
            })?;
            if len > MAX_RECORD_BYTES {
                return Err(CopyError::Corrupt {
                    file: RECORDS_FILE,
                    why: "a record has an impossible length",
                });
            }
            let Some((record, after)) = after.split_at_checked(len) else {
                torn = true;
                break;
            };
            let parsed = Record::from_bytes(record).map_err(|_| CopyError::Corrupt {
                file: RECORDS_FILE,
                why: "a record does not read back",
            })?;
            self.records.push(parsed);
            kept = kept.saturating_add(4).saturating_add(len as u64);
            rest = after;
        }
        if !torn {
            return Ok(None);
        }
        let cut = (bytes.len() as u64).saturating_sub(kept);
        let file = OpenOptions::new()
            .write(true)
            .open(self.dir.join(RECORDS_FILE))
            .map_err(io_err(RECORDS_FILE, "open"))?;
        file.set_len(kept).map_err(io_err(RECORDS_FILE, "cut"))?;
        file.sync_all().map_err(io_err(RECORDS_FILE, "flush"))?;
        Ok(Some(TornTail {
            kept_records: self.records.len(),
            cut_bytes: cut,
        }))
    }

    /// The counter this is the copy of.
    pub fn counter(&self) -> DeviceId {
        self.counter
    }

    /// The directory of this copy.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// The records held, in order.
    pub fn records(&self) -> &[Record] {
        &self.records
    }

    /// How many records are held.
    pub fn len(&self) -> u64 {
        self.records.len() as u64
    }

    /// Whether the copy holds no record.
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    /// The hash of the last record held, or 32 zeros for none.
    pub fn head(&self) -> [u8; 32] {
        self.head
    }

    /// The latest check-in kept.
    pub fn check_in(&self) -> Option<&Record> {
        self.check_in.as_ref()
    }

    fn at(&self, seq: u64) -> Option<&Record> {
        usize::try_from(seq).ok().and_then(|n| self.records.get(n))
    }

    /// Add one record's bytes to the file; the file is as it was on an
    /// error.
    fn append_file(&self, bytes: &[u8]) -> Result<(), CopyError> {
        if bytes.len() > MAX_RECORD_BYTES {
            return Err(CopyError::Corrupt {
                file: RECORDS_FILE,
                why: "the record is longer than the file takes",
            });
        }
        let path = self.dir.join(RECORDS_FILE);
        let existed = path.exists();
        let mut file = OpenOptions::new()
            .append(true)
            .create(true)
            .open(&path)
            .map_err(io_err(RECORDS_FILE, "open"))?;
        let before = file
            .metadata()
            .map_err(io_err(RECORDS_FILE, "measure"))?
            .len();
        let len = u32::try_from(bytes.len()).map_err(|_| CopyError::Corrupt {
            file: RECORDS_FILE,
            why: "the record is longer than the file takes",
        })?;
        // One buffer, one write: the length and the record are never parted
        // by this process.
        let mut buf = Vec::with_capacity(bytes.len().saturating_add(4));
        buf.extend_from_slice(&len.to_le_bytes());
        buf.extend_from_slice(bytes);
        if let Err(e) = file.write_all(&buf).and_then(|()| file.sync_data()) {
            let _ = file.set_len(before);
            return Err(io_err(RECORDS_FILE, "append to")(e));
        }
        if !existed {
            File::open(&self.dir)
                .and_then(|d| d.sync_all())
                .map_err(io_err(RECORDS_FILE, "flush"))?;
        }
        Ok(())
    }

    /// Write the check-in file whole: temporary file, flush, rename, flush
    /// the directory.
    fn write_check_in(&self, bytes: &[u8]) -> Result<(), CopyError> {
        let temp = self.dir.join(format!("{CHECK_IN_FILE}.tmp"));
        File::create(&temp)
            .and_then(|mut f| {
                f.write_all(bytes)?;
                f.sync_all()
            })
            .map_err(io_err(CHECK_IN_FILE, "write"))?;
        fs::rename(&temp, self.dir.join(CHECK_IN_FILE))
            .map_err(io_err(CHECK_IN_FILE, "replace"))?;
        File::open(&self.dir)
            .and_then(|d| d.sync_all())
            .map_err(io_err(CHECK_IN_FILE, "flush"))
    }

    /// Take the records of one message, in order (LK-58, LK-59). Each is
    /// taken only if it extends the copy; the first that cannot be taken
    /// stops the message, and the records before it stay. A record the copy
    /// already holds is skipped. A record that is not the one the copy holds
    /// at that number is a divergence, and nothing is overwritten.
    pub fn take(&mut self, records: Vec<Record>, devices: &DeviceList) -> Taken {
        let mut taken = Taken::default();
        for record in records {
            let held = self.len();
            let seq = record.seq;
            if seq < held {
                match self.at(seq) {
                    Some(ours) if ours.hash() == record.hash() => {
                        taken.skipped = taken.skipped.saturating_add(1);
                        continue;
                    }
                    _ => {
                        taken.diverged = Some(seq);
                        taken.stopped = Some(Stop::Diverged { seq });
                        break;
                    }
                }
            }
            if seq > held {
                taken.stopped = Some(Stop::Gap {
                    expected: held,
                    found: seq,
                });
                break;
            }
            if let Err(stop) = self.check_extends(&record, devices) {
                if matches!(stop, Stop::PrevMismatch { .. }) && held > 0 {
                    // The counter's record before this one is not the one the
                    // copy holds: the chains differ at `held - 1`.
                    taken.diverged = Some(held - 1);
                }
                taken.stopped = Some(stop);
                break;
            }
            if let Err(e) = self.append_file(&record.to_bytes()) {
                taken.stopped = Some(Stop::Store(e));
                break;
            }
            self.head = record.hash();
            self.records.push(record);
            taken.appended = taken.appended.saturating_add(1);
        }
        taken
    }

    /// The checks of `Chain::verify` for one record that would be the next.
    fn check_extends(&self, record: &Record, devices: &DeviceList) -> Result<(), Stop> {
        let seq = record.seq;
        if !devices.contains(&record.author) {
            return Err(Stop::UnknownAuthor { seq });
        }
        if record.body.len() > MAX_BODY {
            return Err(Stop::BodyTooLarge { seq });
        }
        if !record
            .author
            .verify_tagged(TAG_RECORD, &record.signing_bytes(), &record.sig)
        {
            return Err(Stop::BadSignature { seq });
        }
        if record.prev != self.head {
            return Err(Stop::PrevMismatch { seq });
        }
        if record.kind == RecordKind::CheckIn {
            match record.check_in_claim() {
                Ok((len, head)) if len == record.seq && head == record.prev => {}
                _ => return Err(Stop::BadCheckIn { seq }),
            }
        }
        Ok(())
    }

    /// Take a check-in (LK-60). It must be a genuine check-in written by this
    /// counter and on the list; it is compared with the copy when the copy is
    /// long enough (`Chain::check_against_check_in`). The latest one is kept.
    pub fn take_check_in(&mut self, record: Record, devices: &DeviceList) -> CheckInTaken {
        if record.author != self.counter {
            return CheckInTaken::Refused(ChainError::UnknownAuthor { seq: record.seq });
        }
        // Genuine: a check-in, by a device on the list, signed, with a body
        // of the right shape. Against an empty chain the only thing left to
        // say is that the chain is too short.
        match Chain::from_records(Vec::new()).check_against_check_in(&record, devices) {
            Err(ChainError::Truncated { .. }) | Ok(()) => {}
            Err(e) => return CheckInTaken::Refused(e),
        }
        match record.check_in_claim() {
            Ok((len, head)) if len == record.seq && head == record.prev => {}
            _ => return CheckInTaken::Refused(ChainError::BadCheckIn { seq: record.seq }),
        }
        if self
            .check_in
            .as_ref()
            .is_some_and(|latest| record.seq < latest.seq)
        {
            return CheckInTaken::Older;
        }
        if self.check_in.as_ref() != Some(&record) {
            if let Err(e) = self.write_check_in(&record.to_bytes()) {
                return CheckInTaken::Store(e);
            }
        }
        let held = self.at(record.seq).map(Record::hash) == Some(record.hash());
        self.check_in = Some(record);
        match self.diverged_at() {
            Some(seq) => CheckInTaken::Diverged { seq },
            None => CheckInTaken::Taken { matches: held },
        }
    }

    /// Where the latest check-in and the copy disagree (LK-60): the number of
    /// the record the copy holds differently, if any. A copy too short to
    /// compare disagrees with nothing.
    pub fn diverged_at(&self) -> Option<u64> {
        let check_in = self.check_in.as_ref()?;
        let (len, head) = check_in.check_in_claim().ok()?;
        // The records the check-in says came before it.
        if len > 0 && self.len() >= len {
            let before = self.at(len - 1)?;
            if before.hash() != head {
                return Some(len - 1);
            }
        }
        if self.len() > check_in.seq {
            let here = self.at(check_in.seq)?;
            if here.hash() != check_in.hash() {
                return Some(check_in.seq);
            }
        }
        None
    }
}
