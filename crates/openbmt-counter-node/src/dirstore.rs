//! The counter's storage on a directory (CR-40).
//!
//! Each [`Slot`] is one file, written whole: the new bytes go to a temporary
//! file in the same directory, the file is flushed to disk, it is renamed
//! over the old one and the directory is flushed. A crash before the rename
//! leaves the old file as it was. A temporary file that a crash left behind
//! is never read and is overwritten by the next write.
//!
//! A slot is removed (CR-77) by removing its file and flushing the directory.
//! A slot whose file is gone reads back as empty: a sale that was settled is
//! simply not loaded.
//!
//! The records go to one file that only grows. Each record is written as a
//! length (four bytes, little endian) and the record's bytes, and the file is
//! flushed before the write is reported. A crash in the middle of a write can
//! leave a last record that is cut short. At the next load that tail is cut
//! off and reported; it was never reported as stored, so nothing that was
//! reported is lost. Anything else in the file that does not read back is an
//! error, and nothing is cut.
//!
//! The records are not held in memory (CR-81). The load reads the file once
//! to check its framing, cut a torn tail and note where every
//! [`INDEX_STRIDE`]th record starts; [`Store::read_records`] then seeks to
//! the nearest mark and reads forward. The marks are 8 bytes for every
//! [`INDEX_STRIDE`] records.
//!
//! One more small file holds the height up to which every block has been
//! checked (NR-75), written whole like a slot. Another holds the newest
//! signed settings the counter took from the owner (CR-161), also written
//! whole.

use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufReader, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
#[cfg(test)]
use std::sync::atomic::AtomicU32;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, PoisonError};

use openbmt_core::records::{Record, MAX_BODY};
use openbmt_counter::{Slot, Store, Stored};

/// The longest record the file takes, in bytes: the largest body, the fixed
/// fields and the signature, with room to spare.
pub const MAX_RECORD_BYTES: usize = MAX_BODY + 256;

/// How many records lie between two marks of the record file's index.
pub const INDEX_STRIDE: u64 = 64;

const RECORDS_FILE: &str = "records.bin";
const CHECKED_FILE: &str = "checked.bin";
/// The file of the newest signed settings the counter took (CR-161).
pub const SETTINGS_FILE: &str = "settings.bin";
/// The file of the one change that waits for a counter's confirmation
/// (CR-239).
pub const CONFIRM_FILE: &str = "confirm.bin";

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
        Slot::Ranges => "ranges.bin".to_string(),
        Slot::Exposure => "exposure.bin".to_string(),
        Slot::Pause => "pause.bin".to_string(),
        Slot::Desk => "desk.bin".to_string(),
        Slot::Sale(index) => format!("sale-{index}.bin"),
        Slot::Swap(index, n) => format!("swap-{index}-{n}.bin"),
    }
}

/// The sale index and the swap number a file name belongs to, if it is a
/// swap file (CR-220): the form `slot_file` writes, digits with no leading
/// zero.
fn swap_key(name: &str) -> Option<(u32, u32)> {
    let rest = name.strip_prefix("swap-")?.strip_suffix(".bin")?;
    let (a, b) = rest.split_once('-')?;
    let plain = |d: &str| {
        !d.is_empty()
            && !(d.len() > 1 && d.starts_with('0'))
            && d.bytes().all(|c| c.is_ascii_digit())
    };
    if !plain(a) || !plain(b) {
        return None;
    }
    Some((a.parse().ok()?, b.parse().ok()?))
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
    /// The ranges of a counter paired with the shop's own wallet (`OBM;`).
    pub ranges: Option<Vec<u8>>,
    /// The exposure ledger (`OBME`).
    pub exposure: Option<Vec<u8>>,
    /// The pause tracker (`OBMP`).
    pub pause: Option<Vec<u8>>,
    /// The counter's own state (`OBMG`).
    pub desk: Option<Vec<u8>>,
    /// Each sale (`OBML`) with the index of its slot, lowest first.
    pub sales: Vec<(u32, Vec<u8>)>,
    /// Each swap file (CR-220) with the sale's index and the swap's number,
    /// lowest first.
    pub swaps: Vec<(u32, u32, Vec<u8>)>,
    /// How many records the file holds. Each one parses; they are read back
    /// with `Store::read_records` and the chain is checked by
    /// `Counter::load`.
    pub record_count: u64,
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
            ranges: self.ranges.as_deref(),
            exposure: self.exposure.as_deref(),
            pause: self.pause.as_deref(),
            desk: self.desk.as_deref(),
            sales: self.sales.iter().map(|(i, b)| (*i, b.as_slice())).collect(),
        }
    }
}

/// [`Store`] on a directory. See the module docs.
#[derive(Debug)]
pub struct DirStore {
    dir: PathBuf,
    index: Mutex<Option<RecordIndex>>,
    /// Set when an append could not be taken back: no record is appended
    /// until the directory is opened again (CR-81 item 8).
    poisoned: AtomicBool,
    #[cfg(test)]
    pub(crate) faults: Faults,
}

/// Faults a test asks for: each counts the calls that fail, one by one.
#[cfg(test)]
#[derive(Debug, Default)]
pub(crate) struct Faults {
    pub(crate) sync_data: AtomicU32,
    pub(crate) sync_dir: AtomicU32,
    pub(crate) take_back: AtomicU32,
}

#[cfg(test)]
impl Faults {
    fn fire(&self, which: &AtomicU32) -> io::Result<()> {
        if which.load(Ordering::SeqCst) > 0 {
            which.fetch_sub(1, Ordering::SeqCst);
            return Err(io::Error::other("injected"));
        }
        Ok(())
    }
}

/// Where the records are in the record file.
#[derive(Debug, Clone, Default)]
struct RecordIndex {
    /// Whole records in the file.
    count: u64,
    /// Bytes they take.
    end: u64,
    /// The offset of record `k * INDEX_STRIDE`, for each k.
    marks: Vec<u64>,
}

impl DirStore {
    /// Keep files in this directory, which is created if it is missing.
    pub fn open(dir: impl Into<PathBuf>) -> Result<Self, DirStoreError> {
        let dir = dir.into();
        fs::create_dir_all(&dir).map_err(io_err(&dir.display().to_string(), "create"))?;
        Ok(Self {
            dir,
            index: Mutex::new(None),
            poisoned: AtomicBool::new(false),
            #[cfg(test)]
            faults: Faults::default(),
        })
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

    /// `sync_data` on the record file, with the test seam.
    fn data_synced(&self, file: &File) -> io::Result<()> {
        #[cfg(test)]
        self.faults.fire(&self.faults.sync_data)?;
        file.sync_data()
    }

    /// The directory flush of an append, with the test seam.
    fn dir_synced(&self) -> io::Result<()> {
        #[cfg(test)]
        self.faults.fire(&self.faults.sync_dir)?;
        File::open(&self.dir).and_then(|d| d.sync_all())
    }

    /// Put the record file back as it was before an append that failed:
    /// cut to `before` and flushed, or, for a file that append made,
    /// removed and the directory flushed.
    fn take_back(&self, file: &File, path: &Path, before: u64, created: bool) -> io::Result<()> {
        #[cfg(test)]
        self.faults.fire(&self.faults.take_back)?;
        if created {
            match fs::remove_file(path) {
                Ok(()) => {}
                Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                Err(e) => return Err(e),
            }
            File::open(&self.dir).and_then(|d| d.sync_all())
        } else {
            file.set_len(before)?;
            file.sync_data()
        }
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

    /// CR-161: replace the stored signed settings, written whole. On an
    /// error the old file is as it was.
    pub fn put_settings(&mut self, bytes: &[u8]) -> Result<(), DirStoreError> {
        self.write_whole(SETTINGS_FILE, bytes)
    }

    /// CR-161: the stored signed settings, or `None` when the counter has
    /// taken none from the owner. The bytes are not read here.
    pub fn settings_file(&self) -> Result<Option<Vec<u8>>, DirStoreError> {
        self.read(SETTINGS_FILE)
    }

    /// CR-239: replace the stored change that waits to be confirmed, written
    /// whole. On an error the old file is as it was.
    pub fn put_confirm(&mut self, bytes: &[u8]) -> Result<(), DirStoreError> {
        self.write_whole(CONFIRM_FILE, bytes)
    }

    /// CR-239: the stored change that waits to be confirmed, or `None`. The
    /// bytes are not read here.
    pub fn confirm_file(&self) -> Result<Option<Vec<u8>>, DirStoreError> {
        self.read(CONFIRM_FILE)
    }

    /// CR-239: when the file of the waiting change was last written, in
    /// seconds since 1970, which is when the change arrived. `None` when
    /// there is no file or the system does not say.
    pub fn confirm_written_at(&self) -> Option<u64> {
        let modified = fs::metadata(self.path(CONFIRM_FILE))
            .and_then(|m| m.modified())
            .ok()?;
        modified
            .duration_since(std::time::UNIX_EPOCH)
            .ok()
            .map(|d| d.as_secs())
    }

    /// CR-239: take the waiting change away: remove the file and flush the
    /// directory. No file is nothing to remove.
    pub fn remove_confirm(&mut self) -> Result<(), DirStoreError> {
        match fs::remove_file(self.path(CONFIRM_FILE)) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(io_err(CONFIRM_FILE, "remove")(e)),
        }
        self.sync_dir()
    }

    /// Read everything back. A cut-short last record is cut off the file
    /// here, so that the next record is written after the last whole one.
    pub fn load(&self) -> Result<Loaded, DirStoreError> {
        let mut loaded = Loaded {
            batches: self.read(&slot_file(Slot::Batches))?,
            ranges: self.read(&slot_file(Slot::Ranges))?,
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
            if let Some((index, n)) = name.to_str().and_then(swap_key) {
                if let Some(bytes) = self.read(&slot_file(Slot::Swap(index, n)))? {
                    loaded.swaps.push((index, n, bytes));
                }
                continue;
            }
            let Some(index) = name.to_str().and_then(sale_index) else {
                continue;
            };
            let file = slot_file(Slot::Sale(index));
            if let Some(bytes) = self.read(&file)? {
                loaded.sales.push((index, bytes));
            }
        }
        loaded.sales.sort_by_key(|(i, _)| *i);
        loaded.swaps.sort_by_key(|(i, n, _)| (*i, *n));
        self.load_records(&mut loaded)?;
        Ok(loaded)
    }

    /// Read the record file once: check that every record is framed and
    /// parses, cut a torn tail, and note where the records are. Nothing is
    /// kept but the index.
    fn scan_records(&self) -> Result<(RecordIndex, Option<TornTail>), DirStoreError> {
        let file = match File::open(self.path(RECORDS_FILE)) {
            Ok(file) => file,
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                return Ok((RecordIndex::default(), None))
            }
            Err(e) => return Err(io_err(RECORDS_FILE, "read")(e)),
        };
        let size = file
            .metadata()
            .map_err(io_err(RECORDS_FILE, "measure"))?
            .len();
        let mut reader = BufReader::new(file);
        let mut index = RecordIndex::default();
        let mut buf: Vec<u8> = Vec::new();
        let mut torn = false;
        while index.end < size {
            let left = size - index.end;
            if left < 4 {
                torn = true;
                break;
            }
            let mut len_bytes = [0u8; 4];
            reader
                .read_exact(&mut len_bytes)
                .map_err(io_err(RECORDS_FILE, "read"))?;
            let len = usize::try_from(u32::from_le_bytes(len_bytes))
                .map_err(|_| corrupt(RECORDS_FILE, "a record has an impossible length"))?;
            if len > MAX_RECORD_BYTES {
                return Err(corrupt(RECORDS_FILE, "a record has an impossible length"));
            }
            if left - 4 < len as u64 {
                torn = true;
                break;
            }
            buf.resize(len, 0);
            reader
                .read_exact(&mut buf)
                .map_err(io_err(RECORDS_FILE, "read"))?;
            Record::from_bytes(&buf)
                .map_err(|_| corrupt(RECORDS_FILE, "a record does not read back"))?;
            if index.count.is_multiple_of(INDEX_STRIDE) {
                index.marks.push(index.end);
            }
            index.count = index.count.saturating_add(1);
            index.end = index.end.saturating_add(4).saturating_add(len as u64);
        }
        drop(reader);
        let mut cut = None;
        if torn {
            let file = OpenOptions::new()
                .write(true)
                .open(self.path(RECORDS_FILE))
                .map_err(io_err(RECORDS_FILE, "open"))?;
            file.set_len(index.end)
                .map_err(io_err(RECORDS_FILE, "cut"))?;
            file.sync_all().map_err(io_err(RECORDS_FILE, "flush"))?;
            cut = Some(TornTail {
                kept_records: usize::try_from(index.count).unwrap_or(usize::MAX),
                cut_bytes: size.saturating_sub(index.end),
            });
        }
        Ok((index, cut))
    }

    fn load_records(&self, loaded: &mut Loaded) -> Result<(), DirStoreError> {
        let (index, torn) = self.scan_records()?;
        loaded.record_count = index.count;
        loaded.torn = torn;
        *self.index.lock().unwrap_or_else(PoisonError::into_inner) = Some(index);
        Ok(())
    }

    /// Run `f` on the index, which is made by a scan if the load has not
    /// made it yet.
    fn with_index<T>(
        &self,
        f: impl FnOnce(&mut RecordIndex) -> Result<T, DirStoreError>,
    ) -> Result<T, DirStoreError> {
        let mut guard = self.index.lock().unwrap_or_else(PoisonError::into_inner);
        if guard.is_none() {
            *guard = Some(self.scan_records()?.0);
        }
        match guard.as_mut() {
            Some(index) => f(index),
            None => Err(corrupt(RECORDS_FILE, "it has no index")),
        }
    }
}

impl Store for DirStore {
    type Error = DirStoreError;

    fn put(&mut self, slot: Slot, bytes: &[u8]) -> Result<(), DirStoreError> {
        self.write_whole(&slot_file(slot), bytes)
    }

    fn delete(&mut self, slot: Slot) -> Result<(), DirStoreError> {
        let name = slot_file(slot);
        for file in [name.clone(), format!("{name}.tmp")] {
            match fs::remove_file(self.path(&file)) {
                Ok(()) => {}
                Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                Err(e) => return Err(io_err(&file, "remove")(e)),
            }
        }
        self.sync_dir()
    }

    fn append_record(&mut self, bytes: &[u8]) -> Result<(), DirStoreError> {
        if bytes.len() > MAX_RECORD_BYTES {
            return Err(DirStoreError::RecordTooLarge(bytes.len()));
        }
        // CR-81 item 8: after a take back that failed, the file is not known.
        if self.poisoned.load(Ordering::SeqCst) {
            return Err(DirStoreError::Io {
                file: RECORDS_FILE.to_string(),
                op: "append to",
                kind: io::ErrorKind::Other,
            });
        }
        let path = self.path(RECORDS_FILE);
        let existed = path.exists();
        // The index first: a scan after the write would count it twice.
        self.with_index(|_| Ok(()))?;
        let mut file = OpenOptions::new()
            .append(true)
            .create(true)
            .open(&path)
            .map_err(io_err(RECORDS_FILE, "open"))?;
        let before = file
            .metadata()
            .map_err(io_err(RECORDS_FILE, "measure"))?
            .len();
        // A file that holds nothing yet has no entry in the directory that
        // is known to be flushed.
        let created = !existed || before == 0;
        // One buffer, one write: the length and the record are never parted
        // by this process.
        let mut buf = Vec::with_capacity(bytes.len().saturating_add(4));
        buf.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
        buf.extend_from_slice(bytes);
        // Everything that can fail comes before the index moves.
        let written = file
            .write_all(&buf)
            .and_then(|()| self.data_synced(&file))
            .and_then(|()| if created { self.dir_synced() } else { Ok(()) });
        if let Err(e) = written {
            // Take back what may have been written, so that the next record
            // does not follow a cut-short one or repeat a whole one.
            if self.take_back(&file, &path, before, created).is_err() {
                self.poisoned.store(true, Ordering::SeqCst);
            }
            return Err(io_err(RECORDS_FILE, "append to")(e));
        }
        self.with_index(|index| {
            if index.count.is_multiple_of(INDEX_STRIDE) {
                index.marks.push(index.end);
            }
            index.count = index.count.saturating_add(1);
            index.end = index
                .end
                .saturating_add(4)
                .saturating_add(bytes.len() as u64);
            Ok(())
        })
    }

    fn read_records(&self, from: u64, max: usize) -> Result<Vec<Vec<u8>>, DirStoreError> {
        self.with_index(|index| {
            if from >= index.count || max == 0 {
                return Ok(Vec::new());
            }
            let Some(mark) = usize::try_from(from / INDEX_STRIDE)
                .ok()
                .and_then(|k| index.marks.get(k))
            else {
                return Err(corrupt(RECORDS_FILE, "it has no mark for the record"));
            };
            let mut file =
                File::open(self.path(RECORDS_FILE)).map_err(io_err(RECORDS_FILE, "open"))?;
            file.seek(SeekFrom::Start(*mark))
                .map_err(io_err(RECORDS_FILE, "seek in"))?;
            let mut reader = BufReader::new(file);
            let take = u64::try_from(max)
                .unwrap_or(u64::MAX)
                .min(index.count - from);
            let mut out = Vec::new();
            for n in (from - from % INDEX_STRIDE)..from.saturating_add(take) {
                let mut len_bytes = [0u8; 4];
                reader
                    .read_exact(&mut len_bytes)
                    .map_err(|_| corrupt(RECORDS_FILE, "it ends early"))?;
                let len = usize::try_from(u32::from_le_bytes(len_bytes))
                    .map_err(|_| corrupt(RECORDS_FILE, "a record has an impossible length"))?;
                if len > MAX_RECORD_BYTES {
                    return Err(corrupt(RECORDS_FILE, "a record has an impossible length"));
                }
                if n < from {
                    reader
                        .seek_relative(len as i64)
                        .map_err(io_err(RECORDS_FILE, "seek in"))?;
                    continue;
                }
                let mut bytes = vec![0u8; len];
                reader
                    .read_exact(&mut bytes)
                    .map_err(|_| corrupt(RECORDS_FILE, "it ends early"))?;
                out.push(bytes);
            }
            Ok(out)
        })
    }
}
