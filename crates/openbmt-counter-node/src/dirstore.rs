//! The counter's storage on a directory (CR-40).
//!
//! Each [`Slot`] is one file, written whole: the new bytes go to a temporary
//! file in the same directory, the file is flushed to disk, it is renamed
//! over the old one and the directory is flushed. A crash before the rename
//! leaves the old file as it was. A temporary file that a crash left behind
//! is never read and is overwritten by the next write.
//!
//! The records go to one file that only grows. Each record is written as a
//! length (four bytes, little endian) and the record's bytes, and the file is
//! flushed before the write is reported. A crash in the middle of a write can
//! leave a last record that is cut short. At the next load that tail is cut
//! off and reported; it was never reported as stored, so nothing that was
//! reported is lost. Anything else in the file that does not read back is an
//! error, and nothing is cut.
//!
//! One more small file holds the height up to which every block has been
//! checked (NR-75), written whole like a slot.

use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use openbmt_core::records::{Chain, Record, MAX_BODY};
use openbmt_counter::{Slot, Store, Stored};

/// The longest record the file takes, in bytes: the largest body, the fixed
/// fields and the signature, with room to spare.
pub const MAX_RECORD_BYTES: usize = MAX_BODY + 256;

const RECORDS_FILE: &str = "records.bin";
const CHECKED_FILE: &str = "checked.bin";

/// Why a file operation or a read failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DirStoreError {
    /// The operating system refused an operation on a file.
    Io {
        /// The file.
        file: String,
        /// What was being done.
        op: &'static str,
        /// What the system said.
        kind: io::ErrorKind,
    },
    /// A file holds something that is not what was written.
    Corrupt {
        /// The file.
        file: String,
        /// What is wrong with it.
        why: &'static str,
    },
    /// A record is longer than the file takes.
    RecordTooLarge(usize),
}

impl fmt::Display for DirStoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { file, op, kind } => write!(f, "could not {op} {file}: {kind}"),
            Self::Corrupt { file, why } => write!(f, "{file} cannot be read back: {why}"),
            Self::RecordTooLarge(n) => {
                write!(f, "a record of {n} bytes is longer than the file takes")
            }
        }
    }
}

impl std::error::Error for DirStoreError {}

fn io_err<'a>(file: &'a str, op: &'static str) -> impl FnOnce(io::Error) -> DirStoreError + 'a {
    move |e| DirStoreError::Io {
        file: file.to_string(),
        op,
        kind: e.kind(),
    }
}

fn corrupt(file: &str, why: &'static str) -> DirStoreError {
    DirStoreError::Corrupt {
        file: file.to_string(),
        why,
    }
}

/// The file a slot is kept in.
pub fn slot_file(slot: Slot) -> String {
    match slot {
        Slot::Batches => "batches.bin".to_string(),
        Slot::Exposure => "exposure.bin".to_string(),
        Slot::Pause => "pause.bin".to_string(),
        Slot::Desk => "desk.bin".to_string(),
        Slot::Sale(index) => format!("sale-{index}.bin"),
    }
}

/// The index of the sale a file name belongs to, if it is a sale file.
fn sale_index(name: &str) -> Option<u32> {
    let digits = name.strip_prefix("sale-")?.strip_suffix(".bin")?;
    // Only the form `slot_file` writes: digits, no leading zero.
    if digits.is_empty() || (digits.len() > 1 && digits.starts_with('0')) {
        return None;
    }
    if !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok()
}

/// A last record that was cut short and was cut off at load.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TornTail {
    /// Whole records that were kept.
    pub kept_records: usize,
    /// Bytes that were cut off.
    pub cut_bytes: u64,
}

/// Everything the directory holds, as read back.
#[derive(Debug, Clone, Default)]
pub struct Loaded {
    /// The address batches (`OBMC`).
    pub batches: Option<Vec<u8>>,
    /// The exposure ledger (`OBME`).
    pub exposure: Option<Vec<u8>>,
    /// The pause tracker (`OBMP`).
    pub pause: Option<Vec<u8>>,
    /// The counter's own state (`OBMG`).
    pub desk: Option<Vec<u8>>,
    /// Each sale (`OBML`) with the index of its slot, lowest first.
    pub sales: Vec<(u32, Vec<u8>)>,
    /// The records, in the order they were written. Each one parses; the
    /// chain is checked by `Counter::load`.
    pub records: Vec<Record>,
    /// A cut-short last record that was cut off, if there was one.
    pub torn: Option<TornTail>,
    /// The height up to which every block has been checked.
    pub checked: Option<u32>,
}

impl Loaded {
    /// What `Counter::load` needs.
    pub fn stored(&self) -> Stored<'_> {
        Stored {
            batches: self.batches.as_deref(),
            exposure: self.exposure.as_deref(),
            pause: self.pause.as_deref(),
            desk: self.desk.as_deref(),
            sales: self.sales.iter().map(|(i, b)| (*i, b.as_slice())).collect(),
            records: Chain::from_records(self.records.clone()),
        }
    }
}

/// [`Store`] on a directory. See the module docs.
#[derive(Debug)]
pub struct DirStore {
    dir: PathBuf,
}

impl DirStore {
    /// Keep files in this directory, which is created if it is missing.
    pub fn open(dir: impl Into<PathBuf>) -> Result<Self, DirStoreError> {
        let dir = dir.into();
        fs::create_dir_all(&dir).map_err(io_err(&dir.display().to_string(), "create"))?;
        Ok(Self { dir })
    }

    /// The directory the files are in.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn path(&self, name: &str) -> PathBuf {
        self.dir.join(name)
    }

    fn read(&self, name: &str) -> Result<Option<Vec<u8>>, DirStoreError> {
        match fs::read(self.path(name)) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(io_err(name, "read")(e)),
        }
    }

    fn sync_dir(&self) -> Result<(), DirStoreError> {
        let name = self.dir.display().to_string();
        File::open(&self.dir)
            .and_then(|d| d.sync_all())
            .map_err(io_err(&name, "flush"))
    }

    /// Write a file whole: temporary file, flush, rename, flush the
    /// directory. On an error the old file is as it was.
    fn write_whole(&self, name: &str, bytes: &[u8]) -> Result<(), DirStoreError> {
        let temp = self.path(&format!("{name}.tmp"));
        let written = File::create(&temp).and_then(|mut f| {
            f.write_all(bytes)?;
            f.sync_all()
        });
        if let Err(e) = written {
            return Err(io_err(name, "write")(e));
        }
        fs::rename(&temp, self.path(name)).map_err(io_err(name, "replace"))?;
        self.sync_dir()
    }

    /// Replace the stored height up to which every block has been checked.
    pub fn put_checked(&mut self, height: u32) -> Result<(), DirStoreError> {
        self.write_whole(CHECKED_FILE, &height.to_le_bytes())
    }

    /// Read everything back. A cut-short last record is cut off the file
    /// here, so that the next record is written after the last whole one.
    pub fn load(&self) -> Result<Loaded, DirStoreError> {
        let mut loaded = Loaded {
            batches: self.read(&slot_file(Slot::Batches))?,
            exposure: self.read(&slot_file(Slot::Exposure))?,
            pause: self.read(&slot_file(Slot::Pause))?,
            desk: self.read(&slot_file(Slot::Desk))?,
            ..Loaded::default()
        };
        if let Some(bytes) = self.read(CHECKED_FILE)? {
            let four: [u8; 4] = bytes
                .as_slice()
                .try_into()
                .map_err(|_| corrupt(CHECKED_FILE, "it is not four bytes long"))?;
            loaded.checked = Some(u32::from_le_bytes(four));
        }
        let listing = fs::read_dir(&self.dir).map_err(io_err("the data directory", "list"))?;
        for entry in listing {
            let entry = entry.map_err(io_err("the data directory", "list"))?;
            let name = entry.file_name();
            let Some(index) = name.to_str().and_then(sale_index) else {
                continue;
            };
            let file = slot_file(Slot::Sale(index));
            if let Some(bytes) = self.read(&file)? {
                loaded.sales.push((index, bytes));
            }
        }
        loaded.sales.sort_by_key(|(i, _)| *i);
        self.load_records(&mut loaded)?;
        Ok(loaded)
    }

    fn load_records(&self, loaded: &mut Loaded) -> Result<(), DirStoreError> {
        let Some(bytes) = self.read(RECORDS_FILE)? else {
            return Ok(());
        };
        let mut rest = bytes.as_slice();
        let mut kept = 0u64;
        let mut torn = false;
        while !rest.is_empty() {
            let Some((len_bytes, after)) = rest.split_first_chunk::<4>() else {
                torn = true;
                break;
            };
            let len = usize::try_from(u32::from_le_bytes(*len_bytes))
                .map_err(|_| corrupt(RECORDS_FILE, "a record has an impossible length"))?;
            if len > MAX_RECORD_BYTES {
                return Err(corrupt(RECORDS_FILE, "a record has an impossible length"));
            }
            let Some((record, after)) = after.split_at_checked(len) else {
                torn = true;
                break;
            };
            let parsed = Record::from_bytes(record)
                .map_err(|_| corrupt(RECORDS_FILE, "a record does not read back"))?;
            loaded.records.push(parsed);
            kept = kept.saturating_add(4).saturating_add(len as u64);
            rest = after;
        }
        if torn {
            let cut = (bytes.len() as u64).saturating_sub(kept);
            let file = OpenOptions::new()
                .write(true)
                .open(self.path(RECORDS_FILE))
                .map_err(io_err(RECORDS_FILE, "open"))?;
            file.set_len(kept).map_err(io_err(RECORDS_FILE, "cut"))?;
            file.sync_all().map_err(io_err(RECORDS_FILE, "flush"))?;
            loaded.torn = Some(TornTail {
                kept_records: loaded.records.len(),
                cut_bytes: cut,
            });
        }
        Ok(())
    }
}

impl Store for DirStore {
    type Error = DirStoreError;

    fn put(&mut self, slot: Slot, bytes: &[u8]) -> Result<(), DirStoreError> {
        self.write_whole(&slot_file(slot), bytes)
    }

    fn append_record(&mut self, bytes: &[u8]) -> Result<(), DirStoreError> {
        if bytes.len() > MAX_RECORD_BYTES {
            return Err(DirStoreError::RecordTooLarge(bytes.len()));
        }
        let path = self.path(RECORDS_FILE);
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
        // One buffer, one write: the length and the record are never parted
        // by this process.
        let mut buf = Vec::with_capacity(bytes.len().saturating_add(4));
        buf.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
        buf.extend_from_slice(bytes);
        if let Err(e) = file.write_all(&buf).and_then(|()| file.sync_data()) {
            // Take back what may have been written, so that the next record
            // does not follow a cut-short one.
            let _ = file.set_len(before);
            return Err(io_err(RECORDS_FILE, "append to")(e));
        }
        if !existed {
            self.sync_dir()?;
        }
        Ok(())
    }
}
