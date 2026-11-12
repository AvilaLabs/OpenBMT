//! The request book's file (RQ-30, RQ-31): written by atomic steps (a new
//! file, a sync, a rename, a sync of the folder) and read strictly.
//!
//! The bytes are the book's own (`OBM=`). The owner app seals them with its
//! storage key before they reach the file and opens them after, as it does
//! its other files; that is the caller's, as it is for them.

use std::fs::{self, File};
use std::io::{self, Write};
use std::path::Path;

use crate::book::RequestBook;
use crate::error::BookError;

/// The file under the owner app's directory.
pub const REQUEST_BOOK_FILE: &str = "request-book";

/// Write a file of the directory whole: temporary file, flush, rename, flush
/// the directory. On an error the old file is as it was.
pub fn write_file(dir: &Path, name: &str, bytes: &[u8]) -> Result<(), BookError> {
    let temp = dir.join(format!("{name}.tmp"));
    let written = File::create(&temp).and_then(|mut f| {
        f.write_all(bytes)?;
        f.sync_all()
    });
    let result = written
        .and_then(|()| fs::rename(&temp, dir.join(name)))
        .and_then(|()| File::open(dir).and_then(|d| d.sync_all()));
    result.map_err(|e| BookError::Io(format!("{name}: {e}")))
}

/// Read a file of the directory; `None` if it is not there.
pub fn read_file(dir: &Path, name: &str) -> Result<Option<Vec<u8>>, BookError> {
    match fs::read(dir.join(name)) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(BookError::Io(format!("read {name}: {e}"))),
    }
}

impl RequestBook {
    /// Open the book under `dir`; no file is an empty book. A file that
    /// cannot be read is an error, and is never replaced (RQ-31): the
    /// caller makes no new request until it can be read.
    pub fn open(dir: &Path) -> Result<Self, BookError> {
        match read_file(dir, REQUEST_BOOK_FILE)? {
            Some(bytes) => Self::from_bytes(&bytes),
            None => Ok(Self::new()),
        }
    }

    /// Save the book whole under `dir` (RQ-03, RQ-12: before an address or a
    /// quote is shown or shared).
    pub fn save(&self, dir: &Path) -> Result<(), BookError> {
        write_file(dir, REQUEST_BOOK_FILE, &self.to_bytes())
    }
}
