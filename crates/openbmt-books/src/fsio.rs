//! The one file helper (BK-29).

use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

/// BK-29: write `bytes` as the whole of `path`: to a temporary name in the
/// same folder, flushed to disk, renamed over the target, and the folder flushed. The target is
/// either what it was or the whole new file. On a failure the temporary file
/// is removed.
pub fn write_whole(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let name = path
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "BK-29: no file name"))?;
    let mut temp_name = name.to_os_string();
    temp_name.push(".part");
    let temp: PathBuf = path.with_file_name(temp_name);
    let result = (|| {
        let mut file = File::create(&temp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        fs::rename(&temp, path)?;
        // The rename is in the folder's entry: flush the folder too.
        let folder = match path.parent() {
            Some(p) if !p.as_os_str().is_empty() => p,
            _ => Path::new("."),
        };
        File::open(folder)?.sync_all()
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}
