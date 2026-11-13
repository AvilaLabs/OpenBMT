//! The node's log: one file in the node's directory, one line a record.
//! Nothing here reaches a person; it is for the manager and for the tests.

use lightning::util::logger::{Level, Logger as LdkLogger, Record};
use std::io::Write;
use std::path::Path;
use std::sync::Mutex;

/// Writes the library's log and the node's own lines to a file.
pub struct Logger {
    file: Mutex<std::fs::File>,
    min: Level,
}

impl Logger {
    /// Opens (appending) `path`, keeping records at `min` and above.
    pub fn open(path: &Path, min: Level) -> std::io::Result<Self> {
        let file = std::fs::OpenOptions::new().create(true).append(true).open(path)?;
        Ok(Self { file: Mutex::new(file), min })
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
        if let Ok(mut f) = self.file.lock() {
            let _ = writeln!(f, "{now} {level:<5} {what}");
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
