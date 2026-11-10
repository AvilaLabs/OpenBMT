//! What the driver must not decide for itself: where bytes are kept, what
//! time it is and where randomness comes from (NR-23, NR-11).
//!
//! The driver asks for these through traits so that an app on another
//! platform passes its own, and so that the tests pass fakes.

use std::fmt::Display;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use bitcoin::hashes::{sha256, Hash};

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

/// A source of random numbers, for the nonce of each connection (NR-11) and
/// for the draw of the next address (NR-23). It must not repeat.
pub trait Randomness {
    /// The next number.
    fn next_u64(&mut self) -> u64;
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
/// If a read from the file ever fails, the numbers that follow come from a
/// hash of the clock and a counter. They do not repeat, which is all a nonce
/// needs, but they are not unpredictable, and the failure is counted in
/// [`OsRandomness::fallbacks`].
#[derive(Debug)]
pub struct OsRandomness {
    file: File,
    counter: u64,
    fallbacks: u64,
}

impl OsRandomness {
    /// Open `/dev/urandom`.
    pub fn new() -> io::Result<Self> {
        Ok(Self {
            file: File::open("/dev/urandom")?,
            counter: 0,
            fallbacks: 0,
        })
    }

    /// How many numbers came from the fallback because a read failed.
    pub fn fallbacks(&self) -> u64 {
        self.fallbacks
    }
}

impl Randomness for OsRandomness {
    fn next_u64(&mut self) -> u64 {
        let mut bytes = [0u8; 8];
        self.counter = self.counter.wrapping_add(1);
        if self.file.read_exact(&mut bytes).is_ok() {
            return u64::from_le_bytes(bytes);
        }
        self.fallbacks = self.fallbacks.saturating_add(1);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let mut input = Vec::with_capacity(24);
        input.extend_from_slice(&nanos.to_le_bytes());
        input.extend_from_slice(&self.counter.to_le_bytes());
        let hash = sha256::Hash::hash(&input).to_byte_array();
        let mut word = [0u8; 8];
        for (slot, byte) in word.iter_mut().zip(hash.iter()) {
            *slot = *byte;
        }
        u64::from_le_bytes(word)
    }
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
        fs::create_dir_all(&dir)?;
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
        let mut file = File::create(self.temp_path(key))?;
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
