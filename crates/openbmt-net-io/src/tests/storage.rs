//! NR-73: the file storage, and the clock and randomness that come with it.

use std::fs;
use std::path::PathBuf;

use crate::env::{Clock, FileStorage, Key, OsRandomness, Randomness, Storage, SystemClock};

/// A new directory under the build's own temporary directory, removed at
/// the end of the test.
struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let base = std::env::var_os("TMPDIR")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        let dir = base.join(format!("openbmt-net-io-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        Self(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn nr_73_a_saved_value_loads_again_and_the_other_key_is_empty() {
    let dir = TempDir::new("roundtrip");
    let mut storage = FileStorage::new(dir.0.join("nested")).unwrap();
    assert_eq!(storage.load(Key::Headers).unwrap(), None);
    storage.save(Key::Headers, b"one").unwrap();
    assert_eq!(storage.load(Key::Headers).unwrap().unwrap(), b"one");
    assert_eq!(storage.load(Key::Book).unwrap(), None);
    storage.save(Key::Headers, b"two, longer").unwrap();
    assert_eq!(storage.load(Key::Headers).unwrap().unwrap(), b"two, longer");
    storage.save(Key::Headers, b"3").unwrap();
    assert_eq!(storage.load(Key::Headers).unwrap().unwrap(), b"3");
    // A new object over the same directory reads the same value.
    let mut again = FileStorage::new(storage.dir()).unwrap();
    assert_eq!(again.load(Key::Headers).unwrap().unwrap(), b"3");
}

#[test]
fn nr_73_a_save_that_stops_before_the_rename_leaves_the_old_value() {
    let dir = TempDir::new("interrupted");
    let mut storage = FileStorage::new(&dir.0).unwrap();
    storage.save(Key::Book, b"old").unwrap();
    // The first half of a save, and then the process ends.
    storage.write_temp(Key::Book, b"new value").unwrap();
    assert_eq!(storage.load(Key::Book).unwrap().unwrap(), b"old");
    // The next save overwrites the leftover and goes through.
    storage.save(Key::Book, b"newer").unwrap();
    assert_eq!(storage.load(Key::Book).unwrap().unwrap(), b"newer");
}

#[test]
fn nr_73_a_garbage_temporary_file_is_never_read() {
    let dir = TempDir::new("garbage");
    let mut storage = FileStorage::new(&dir.0).unwrap();
    // A leftover with nothing saved yet: the value is not there.
    fs::write(dir.0.join("headers.bin.tmp"), b"\xff\xfe garbage").unwrap();
    fs::write(dir.0.join("book.bin.tmp"), b"").unwrap();
    fs::write(dir.0.join("unrelated.tmp"), b"x").unwrap();
    assert_eq!(storage.load(Key::Headers).unwrap(), None);
    assert_eq!(storage.load(Key::Book).unwrap(), None);
    storage.save(Key::Headers, b"real").unwrap();
    assert_eq!(storage.load(Key::Headers).unwrap().unwrap(), b"real");
    // The unrelated file is left alone, and no temporary file is left over
    // from the save.
    assert!(dir.0.join("unrelated.tmp").exists());
    assert!(!dir.0.join("headers.bin.tmp").exists());
}

#[test]
fn nr_73_a_value_that_is_a_directory_is_an_error_not_an_empty_value() {
    let dir = TempDir::new("notafile");
    let mut storage = FileStorage::new(&dir.0).unwrap();
    fs::create_dir(dir.0.join("book.bin")).unwrap();
    assert!(storage.load(Key::Book).is_err());
}

#[test]
fn nr_23_the_system_clock_and_urandom_give_changing_values() {
    assert!(SystemClock.now() > 1_700_000_000);
    let mut rng = OsRandomness::new().unwrap();
    let a = rng.next_u64();
    let b = rng.next_u64();
    let c = rng.next_u64();
    assert!(a != b || b != c);
    assert_eq!(rng.fallbacks(), 0);
}
