//! Where the Box keeps its own files (BX-50).
//!
//! [`BoxFiles`] is the small set of operations the Box's state needs: read a
//! whole file, write one whole, remove one. [`DirFiles`] does them in the
//! data directory: a whole file is written to a temporary file, flushed,
//! renamed over the old one, and the directory is flushed. Files are made
//! with mode 0600. [`MemFiles`] does them in memory, and can be told to fail
//! from the n-th write on, so that a test can stop the Box between two
//! writes.

use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{self, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

/// The operations the Box's state needs from its storage.
pub trait BoxFiles {
    /// The whole file, or `None` if there is none.
    fn read(&self, name: &str) -> io::Result<Option<Vec<u8>>>;
    /// Write the whole file (BX-50): after this returns `Ok` the file holds
    /// these bytes and no others, even if the power goes at once.
    fn write(&mut self, name: &str, bytes: &[u8]) -> io::Result<()>;
    /// Remove the file; no error if it is not there.
    fn remove(&mut self, name: &str) -> io::Result<()>;
    /// The permission bits of the file (the lowest nine), or `None` if there
    /// is none (BX-41).
    fn mode(&self, name: &str) -> io::Result<Option<u32>>;
}

/// The files of a data directory.
#[derive(Debug)]
pub struct DirFiles {
    dir: PathBuf,
}

impl DirFiles {
    /// Keep files in `dir`, which must exist.
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    /// The directory.
    pub fn dir(&self) -> &Path {
        &self.dir
    }
}

fn flush_dir(dir: &Path) -> io::Result<()> {
    File::open(dir).and_then(|d| d.sync_all())
}

impl BoxFiles for DirFiles {
    fn read(&self, name: &str) -> io::Result<Option<Vec<u8>>> {
        match fs::read(self.dir.join(name)) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e),
        }
    }

    fn write(&mut self, name: &str, bytes: &[u8]) -> io::Result<()> {
        let temp = self.dir.join(format!("{name}.tmp"));
        let mut file = openbmt_link::private::create_file(&temp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temp, self.dir.join(name))?;
        flush_dir(&self.dir)
    }

    fn remove(&mut self, name: &str) -> io::Result<()> {
        match fs::remove_file(self.dir.join(name)) {
            Ok(()) => flush_dir(&self.dir),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e),
        }
    }

    fn mode(&self, name: &str) -> io::Result<Option<u32>> {
        match fs::metadata(self.dir.join(name)) {
            Ok(meta) => Ok(Some(meta.permissions().mode() & 0o777)),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e),
        }
    }
}

/// Files in memory, for tests.
#[derive(Debug, Default, Clone)]
pub struct MemFiles {
    files: BTreeMap<String, Vec<u8>>,
    writes: usize,
    fail_from: Option<usize>,
    fail_removes: bool,
    modes: BTreeMap<String, u32>,
}

impl MemFiles {
    /// No files.
    pub fn new() -> Self {
        Self::default()
    }

    /// Every write from the `n`-th on (counting from 1, counting only
    /// writes made after this call) fails and changes nothing, as a full
    /// disk would.
    pub fn fail_writes_from(&mut self, n: usize) {
        self.writes = 0;
        self.fail_from = Some(n);
    }

    /// Writes work again.
    pub fn stop_failing(&mut self) {
        self.fail_from = None;
    }

    /// Whether removing fails.
    pub fn set_fail_removes(&mut self, fail: bool) {
        self.fail_removes = fail;
    }

    /// The names of the files held.
    pub fn names(&self) -> Vec<String> {
        self.files.keys().cloned().collect()
    }

    /// Give a file other permission bits than the 0600 every file is made
    /// with, as someone with a shell could.
    pub fn set_mode(&mut self, name: &str, mode: u32) {
        self.modes.insert(name.to_string(), mode);
    }

    /// Put bytes in a file directly, as damage would.
    pub fn put(&mut self, name: &str, bytes: &[u8]) {
        self.files.insert(name.to_string(), bytes.to_vec());
    }
}

impl BoxFiles for MemFiles {
    fn read(&self, name: &str) -> io::Result<Option<Vec<u8>>> {
        Ok(self.files.get(name).cloned())
    }

    fn write(&mut self, name: &str, bytes: &[u8]) -> io::Result<()> {
        self.writes = self.writes.saturating_add(1);
        if self.fail_from.is_some_and(|n| self.writes >= n) {
            return Err(io::Error::other("no space left on device"));
        }
        self.files.insert(name.to_string(), bytes.to_vec());
        self.modes.remove(name);
        Ok(())
    }

    fn remove(&mut self, name: &str) -> io::Result<()> {
        if self.fail_removes {
            return Err(io::Error::other("cannot remove"));
        }
        self.files.remove(name);
        self.modes.remove(name);
        Ok(())
    }

    fn mode(&self, name: &str) -> io::Result<Option<u32>> {
        Ok(self
            .files
            .contains_key(name)
            .then(|| self.modes.get(name).copied().unwrap_or(0o600)))
    }
}
