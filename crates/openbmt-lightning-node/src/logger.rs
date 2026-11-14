//! The node's log: one file in the node's directory, one line a record.
//! Nothing here reaches a person; it is for the manager and for the tests.

use lightning::util::logger::{Level, Logger as LdkLogger, Record};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// The size at which the log is moved aside and a new one started.
pub const LOG_CAP: u64 = 16 * 1024 * 1024;

/// Writes the library's log and the node's own lines to a file. When a line
/// would take the file over [`LOG_CAP`] the file is renamed to the same name
/// with `.1` added (replacing an older one) and a new file started, so that
/// at most those two files exist. A failure of the log is never an error of
/// the node: the line is dropped.
pub struct Logger {
    path: PathBuf,
    file: Mutex<Current>,
    min: Level,
    cap: u64,
}

struct Current {
    file: std::fs::File,
    len: u64,
}

fn open_append(path: &Path) -> std::io::Result<std::fs::File> {
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
}

impl Logger {
    /// Opens (appending) `path`, keeping records at `min` and above.
    pub fn open(path: &Path, min: Level) -> std::io::Result<Self> {
        Self::open_capped(path, min, LOG_CAP)
    }

    /// As [`Logger::open`], with the size at which the file is moved aside.
    pub(crate) fn open_capped(path: &Path, min: Level, cap: u64) -> std::io::Result<Self> {
        let file = open_append(path)?;
        let len = file.metadata().map(|m| m.len()).unwrap_or(0);
        Ok(Self {
            path: path.to_path_buf(),
            file: Mutex::new(Current { file, len }),
            min,
            cap,
        })
    }

    fn rotate(&self, cur: &mut Current) {
        let mut old = self.path.clone().into_os_string();
        old.push(".1");
        // If the rename fails, go on in the same file rather than lose the
        // lines; a failed open below drops them instead.
        if std::fs::rename(&self.path, PathBuf::from(old)).is_err() {
            return;
        }
        if let Ok(file) = open_append(&self.path) {
            *cur = Current { file, len: 0 };
        }
    }

    /// One line of the node's own.
    pub fn line(&self, level: Level, what: &str) {
        if level < self.min {
            return;
        }
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let text = format!("{now} {level:<5} {what}\n");
        let size = u64::try_from(text.len()).unwrap_or(u64::MAX);
        if let Ok(mut cur) = self.file.lock() {
            if cur.len > 0 && cur.len.saturating_add(size) > self.cap {
                self.rotate(&mut cur);
            }
            if cur.file.write_all(text.as_bytes()).is_ok() {
                cur.len = cur.len.saturating_add(size);
            }
        }
    }
}

impl LdkLogger for Logger {
    fn log(&self, record: Record) {
        if record.level < self.min {
            return;
        }
        self.line(
            record.level,
            &format!("{}:{} {}", record.module_path, record.line, record.args),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sl_20_the_log_is_moved_aside_at_its_cap_and_only_two_files_remain() {
        let dir = crate::testutil::temp_dir("log-cap");
        let path = dir.join("node.log");
        let logger = Logger::open_capped(&path, Level::Debug, 200).unwrap();
        for n in 0..100 {
            logger.line(Level::Info, &format!("line number {n:04}"));
        }
        let names: std::collections::BTreeSet<String> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            names,
            ["node.log", "node.log.1"]
                .map(String::from)
                .into_iter()
                .collect()
        );
        let now = std::fs::read_to_string(&path).unwrap();
        let before = std::fs::read_to_string(dir.join("node.log.1")).unwrap();
        assert!(now.len() <= 200 && before.len() <= 200);
        assert!(now.contains("line number 0099"));
        // The last lines are in order across the two files.
        assert!(before.contains("line number 00"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn sl_20_a_log_that_cannot_rotate_or_write_never_fails_the_caller() {
        let dir = crate::testutil::temp_dir("log-broken");
        let path = dir.join("node.log");
        let logger = Logger::open_capped(&path, Level::Debug, 50).unwrap();
        logger.line(Level::Info, "first line, long enough to fill the cap");
        // The directory goes: the rename and the new file both fail.
        std::fs::remove_dir_all(&dir).unwrap();
        for _ in 0..5 {
            logger.line(Level::Info, "after the directory went away");
        }
    }
}
