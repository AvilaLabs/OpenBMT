//! What the driver must not decide for itself: where bytes are kept, what
//! time it is and where randomness comes from (NR-23, NR-11).
//!
//! The driver asks for these through traits so that an app on another
//! platform passes its own, and so that the tests pass fakes.

use std::fmt::{self, Display};
use std::fs::{self, File, TryLockError};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// The two things the driver keeps.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Key {
    /// The watcher's header store (WR-08).
    Headers,
    /// The address book (NR-22).
    Book,
}

/// A place to keep one value per [`Key`]. A save replaces the whole value:
/// after a crash a load returns the old value or the new one, never a mix.
pub trait Storage {
    /// What a failed load or save says.
    type Error: Display;

    /// The stored bytes, or `None` when nothing was saved yet.
    fn load(&mut self, key: Key) -> Result<Option<Vec<u8>>, Self::Error>;

    /// Replace the stored bytes.
    fn save(&mut self, key: Key, bytes: &[u8]) -> Result<(), Self::Error>;
}

/// The time in seconds since the Unix epoch.
pub trait Clock {
    /// Now, in whole seconds.
    fn now(&self) -> u64;
}

/// The random source gave no numbers. Nothing is made from a number that was
/// not drawn: the caller refuses the step that needed it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RandomError;

impl Display for RandomError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("the system's random source gave no numbers")
    }
}

impl std::error::Error for RandomError {}

/// A source of random numbers, for the nonce of each connection (NR-11) and
/// for the draw of the next address (NR-23). It must not repeat.
pub trait Randomness {
    /// The next number, or an error when the source gave none. There is no
    /// stand-in number.
    fn next_u64(&mut self) -> Result<u64, RandomError>;

    /// Whether the numbers can be used for a secret key (NR-80). A source
    /// that knows its numbers could be guessed says no, and the driver then
    /// does not offer the encrypted transport. A source that does not know
    /// says yes.
    fn unpredictable(&self) -> bool {
        true
    }
}

/// The system clock. A clock set before 1970 reads as 0.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    }
}

/// Randomness read from `/dev/urandom`. For Linux and other Unix systems.
/// An Android app passes its own [`Randomness`] (for example one over
/// `SecureRandom`) instead.
///
/// If a read from the file fails, the call returns [`RandomError`]. No other
/// numbers take the place of the ones that were not read.
#[derive(Debug)]
pub struct OsRandomness {
    file: File,
}

impl OsRandomness {
    /// Open `/dev/urandom`.
    pub fn new() -> io::Result<Self> {
        Ok(Self {
            file: File::open("/dev/urandom")?,
        })
    }
}

#[cfg(test)]
impl OsRandomness {
    /// A source over any file, for the tests.
    pub(crate) fn with_file(file: File) -> Self {
        Self { file }
    }
}

impl Randomness for OsRandomness {
    fn next_u64(&mut self) -> Result<u64, RandomError> {
        read_word(&mut self.file)
    }
}

/// One number from a reader, or [`RandomError`] when it gives fewer than 8
/// bytes.
fn read_word(source: &mut impl Read) -> Result<u64, RandomError> {
    let mut bytes = [0u8; 8];
    source.read_exact(&mut bytes).map_err(|_| RandomError)?;
    Ok(u64::from_le_bytes(bytes))
}

/// Values kept as files in one directory.
///
/// A save writes a temporary file in the same directory, flushes it to disk,
/// renames it over the old file and flushes the directory. A crash before
/// the rename leaves the old file as it was. A temporary file that a crash
/// left behind is never read and is overwritten by the next save.
#[derive(Debug, Clone)]
pub struct FileStorage {
    dir: PathBuf,
}

impl FileStorage {
    /// Keep files in this directory, which is created if it is missing.
    pub fn new(dir: impl Into<PathBuf>) -> io::Result<Self> {
        let dir = dir.into();
        crate::private::create_dir_all(&dir)?;
        Ok(Self { dir })
    }

    /// The directory the files are in.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn name(key: Key) -> &'static str {
        match key {
            Key::Headers => "headers.bin",
            Key::Book => "book.bin",
        }
    }

    /// The file that holds the value.
    pub fn path(&self, key: Key) -> PathBuf {
        self.dir.join(Self::name(key))
    }

    fn temp_path(&self, key: Key) -> PathBuf {
        self.dir.join(format!("{}.tmp", Self::name(key)))
    }

    /// The first half of a save: the new value in the temporary file, on
    /// disk. The old file is untouched.
    pub(crate) fn write_temp(&self, key: Key, bytes: &[u8]) -> io::Result<()> {
        let mut file = crate::private::create_file(&self.temp_path(key))?;
        file.write_all(bytes)?;
        file.sync_all()
    }

    /// The second half of a save: rename, then flush the directory.
    pub(crate) fn commit_temp(&self, key: Key) -> io::Result<()> {
        fs::rename(self.temp_path(key), self.path(key))?;
        File::open(&self.dir)?.sync_all()
    }
}

impl Storage for FileStorage {
    type Error = io::Error;

    fn load(&mut self, key: Key) -> io::Result<Option<Vec<u8>>> {
        match fs::read(self.path(key)) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e),
        }
    }

    fn save(&mut self, key: Key, bytes: &[u8]) -> io::Result<()> {
        self.write_temp(key, bytes)?;
        self.commit_temp(key)
    }
}

/// Why a [`DirLock`] was not taken.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LockError {
    /// Another handle holds the lock: another OpenBMT process, or another
    /// start in this one, is using the directory.
    Busy(PathBuf),
    /// The directory or the lock file could not be made or locked.
    Io(String),
}

impl fmt::Display for LockError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Busy(dir) => write!(f, "another OpenBMT process is using {}", dir.display()),
            Self::Io(e) => write!(f, "the directory could not be locked: {e}"),
        }
    }
}

impl std::error::Error for LockError {}

/// The name of the lock file in a data directory.
pub const LOCK_FILE: &str = "lock";

/// NR-79: the right to use a data directory. Taken once by whatever owns
/// the directory (a node, a watcher, an example) before it reads anything,
/// and held until it is dropped. [`FileStorage`] does not take it: it shares
/// the directory with its owner.
///
/// The lock is an exclusive lock on the file `lock` in the directory. The
/// system releases it when the file is closed or the process ends, so a
/// crash leaves nothing to clean up, and the file holds no process number.
#[derive(Debug)]
pub struct DirLock {
    dir: PathBuf,
    // Held for its lock; closing the file releases it.
    _file: File,
}

impl DirLock {
    /// Make the directory if it is missing, then take the lock. Does not
    /// wait: a directory that is in use is a [`LockError::Busy`].
    pub fn acquire(dir: impl Into<PathBuf>) -> Result<Self, LockError> {
        let dir = dir.into();
        let io = |e: io::Error| LockError::Io(format!("{}: {e}", dir.display()));
        crate::private::create_dir_all(&dir).map_err(io)?;
        let file = crate::private::open_file(&dir.join(LOCK_FILE)).map_err(io)?;
        match file.try_lock() {
            Ok(()) => Ok(Self { dir, _file: file }),
            Err(TryLockError::WouldBlock) => Err(LockError::Busy(dir)),
            Err(TryLockError::Error(e)) => Err(io(e)),
        }
    }

    /// The directory that is locked.
    pub fn dir(&self) -> &Path {
        &self.dir
    }
}
