//! NR-73: the file storage, and the clock and randomness that come with it.

use std::fs;
use std::path::PathBuf;

use crate::env::{
    Clock, DirLock, FileStorage, Key, LockError, OsRandomness, Randomness, Storage, SystemClock,
    LOCK_FILE,
};

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

#[test]
fn nr_79_a_second_lock_on_the_same_directory_fails_and_a_later_one_succeeds() {
    let dir = TempDir::new("lock-twice");
    // The directory is made if it is missing.
    let first = DirLock::acquire(&dir.0).unwrap();
    assert!(dir.0.join(LOCK_FILE).is_file());
    // A second open file description conflicts, even in the same process:
    // the lock belongs to the open file, not to the process.
    let err = DirLock::acquire(&dir.0).unwrap_err();
    assert_eq!(err, LockError::Busy(dir.0.clone()));
    let text = err.to_string();
    assert!(
        text.starts_with("another OpenBMT process is using "),
        "{text}"
    );
    assert!(text.contains(&dir.0.display().to_string()), "{text}");
    drop(first);
    // Released when the holder is dropped.
    let again = DirLock::acquire(&dir.0).unwrap();
    assert_eq!(again.dir(), dir.0.as_path());
}

#[test]
fn nr_79_the_lock_does_not_stop_the_storage_and_leaves_no_process_number() {
    let dir = TempDir::new("lock-storage");
    let _lock = DirLock::acquire(&dir.0).unwrap();
    // The storage shares the directory and does not lock it again.
    let mut storage = FileStorage::new(&dir.0).unwrap();
    storage.save(Key::Book, b"book").unwrap();
    assert_eq!(storage.load(Key::Book).unwrap(), Some(b"book".to_vec()));
    assert_eq!(fs::read(dir.0.join(LOCK_FILE)).unwrap(), Vec::<u8>::new());
}

#[test]
fn nr_79_two_directories_do_not_conflict() {
    let a = TempDir::new("lock-a");
    let b = TempDir::new("lock-b");
    let _first = DirLock::acquire(&a.0).unwrap();
    let _second = DirLock::acquire(&b.0).unwrap();
}

/// The directory the child process of the two-process test locks. Not set
/// in a normal run, where `nr_79_child_holds_the_lock` does nothing.
const CHILD_DIR: &str = "OPENBMT_NR79_CHILD_DIR";

/// The child half of the two-process test: the parent runs this test in a
/// new process. It takes the lock, tells the parent by making `ready`, and
/// holds the lock until it is killed.
#[test]
fn nr_79_child_holds_the_lock() {
    let Some(dir) = std::env::var_os(CHILD_DIR) else {
        return;
    };
    let dir = PathBuf::from(dir);
    let _lock = DirLock::acquire(&dir).unwrap();
    fs::write(dir.join("ready"), b"").unwrap();
    // Bounded, so that a parent that never kills the child cannot leave it
    // running for good.
    for _ in 0..600 {
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}

#[test]
fn nr_79_a_lock_held_by_another_process_is_busy_until_that_process_ends() {
    let dir = TempDir::new("lock-process");
    fs::create_dir_all(&dir.0).unwrap();
    let exe = std::env::current_exe().unwrap();
    let mut child = std::process::Command::new(exe)
        .args(["--exact", "tests::storage::nr_79_child_holds_the_lock"])
        .env(CHILD_DIR, &dir.0)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    // Wait for the child to say it holds the lock.
    let mut ready = false;
    for _ in 0..400 {
        if dir.0.join("ready").exists() {
            ready = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    if !ready {
        let _ = child.kill();
        let _ = child.wait();
        panic!("the child did not take the lock");
    }
    let held = DirLock::acquire(&dir.0);
    // The child ends without releasing anything itself: a crash.
    child.kill().unwrap();
    child.wait().unwrap();
    assert_eq!(held.unwrap_err(), LockError::Busy(dir.0.clone()));
    // No stale lock is left behind.
    let after = DirLock::acquire(&dir.0).unwrap();
    assert_eq!(after.dir(), dir.0.as_path());
}

#[test]
fn nr_79_the_lock_file_is_made_empty_and_other_files_are_not_touched() {
    let dir = TempDir::new("lock-file");
    fs::create_dir_all(&dir.0).unwrap();
    fs::write(dir.0.join("keep"), b"data").unwrap();
    let lock = DirLock::acquire(&dir.0).unwrap();
    assert_eq!(fs::read(dir.0.join(LOCK_FILE)).unwrap(), Vec::<u8>::new());
    assert_eq!(fs::read(dir.0.join("keep")).unwrap(), b"data");
    let mut names: Vec<_> = fs::read_dir(&dir.0)
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    names.sort();
    assert_eq!(names, ["keep", LOCK_FILE]);
    // A refused second take changes nothing, and the file stays after the
    // holder goes.
    assert!(DirLock::acquire(&dir.0).is_err());
    drop(lock);
    assert!(dir.0.join(LOCK_FILE).is_file());
    assert_eq!(fs::read(dir.0.join(LOCK_FILE)).unwrap(), Vec::<u8>::new());
}

#[cfg(unix)]
#[test]
fn nr_73_the_files_and_the_folder_are_private_even_over_a_wider_temporary() {
    use std::os::unix::fs::PermissionsExt;
    let mode = |p: &std::path::Path| fs::metadata(p).unwrap().permissions().mode() & 0o777;
    let dir = TempDir::new("private");
    let mut storage = FileStorage::new(dir.0.join("nested/deeper")).unwrap();
    assert_eq!(mode(storage.dir()), 0o700);
    assert_eq!(mode(&dir.0.join("nested")), 0o700);
    let tmp = storage.dir().join("headers.bin.tmp");
    fs::write(&tmp, b"stale").unwrap();
    fs::set_permissions(&tmp, fs::Permissions::from_mode(0o644)).unwrap();
    storage.save(Key::Headers, b"one").unwrap();
    assert_eq!(mode(&storage.path(Key::Headers)), 0o600);
    let lock = DirLock::acquire(dir.0.join("other/lockdir")).unwrap();
    assert_eq!(mode(&dir.0.join("other/lockdir")), 0o700);
    assert_eq!(mode(&dir.0.join("other/lockdir").join(LOCK_FILE)), 0o600);
    drop(lock);
}
