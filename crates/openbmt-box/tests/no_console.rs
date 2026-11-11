//! `openbmt-box --no-console` as a process: ended by SIGKILL at any point,
//! it starts again on the same directory with the same key (BX-52).

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

struct Dir(PathBuf);

impl Dir {
    fn new(name: &str) -> Self {
        let base = std::env::var_os("OPENBMT_TEST_TMP")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        let dir = base.join(format!("openbmt-box-bin-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

// Cargo gives the binary's path only at compile time. It names the target
// folder of this build, not a source folder, so a reused object cannot point
// at another checkout.
#[allow(clippy::disallowed_macros)]
fn spawn(dir: &Path, stdout: Stdio) -> Child {
    Command::new(env!("CARGO_BIN_EXE_openbmt-box"))
        .arg(dir)
        .arg("127.0.0.1:0")
        .arg("--no-console")
        .stdin(Stdio::null())
        .stdout(stdout)
        .stderr(Stdio::null())
        .spawn()
        .unwrap()
}

/// Start the Box without a console; return it with its key and its state
/// as it printed them.
fn start(dir: &Path) -> (Child, String, String) {
    let mut child = spawn(dir, Stdio::piped());
    let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
    let (mut key, mut state) = (String::new(), String::new());
    while state.is_empty() {
        let line = lines
            .next()
            .expect("the Box ended before it said its state")
            .unwrap();
        if let Some(k) = line.strip_prefix("key: ") {
            key = k.to_string();
        }
        if let Some(s) = line.strip_prefix("state: ") {
            state = s.to_string();
        }
    }
    (child, key, state)
}

fn kill(mut child: Child) {
    // SIGKILL: no handler can run.
    child.kill().unwrap();
    let status = child.wait().unwrap();
    assert!(!status.success());
}

#[test]
fn bx_52_a_box_without_a_console_keeps_running_with_standard_input_closed() {
    let dir = Dir::new("runs");
    let (mut child, _, state) = start(&dir.0);
    assert_eq!(state.trim(), "unpaired");
    std::thread::sleep(Duration::from_millis(500));
    assert!(child.try_wait().unwrap().is_none(), "the Box ended");
    kill(child);
}

#[test]
fn bx_52_a_box_killed_at_any_point_starts_again_with_its_key() {
    let dir = Dir::new("kills");
    let (child, key, _) = start(&dir.0);
    assert!(!key.is_empty());
    // Killed the moment it said its state, the first time its key was made.
    kill(child);
    for delay_ms in [0u64, 1, 5, 20, 100, 300] {
        let (child, again, state) = start(&dir.0);
        assert_eq!(again, key, "the key changed");
        assert_eq!(state.trim(), "unpaired");
        std::thread::sleep(Duration::from_millis(delay_ms));
        kill(child);
    }
}

#[test]
fn bx_52_a_box_killed_during_its_first_start_never_leaves_a_damaged_key() {
    // Killed a varying time after the spawn, before it may have said
    // anything; whatever is on disk, the next start succeeds and its key is
    // then kept.
    let dir = Dir::new("first");
    for delay_ms in [0u64, 2, 5, 10, 20, 40] {
        let mut child = spawn(&dir.0, Stdio::null());
        std::thread::sleep(Duration::from_millis(delay_ms));
        let _ = child.kill();
        let _ = child.wait();
    }
    let (child, key, state) = start(&dir.0);
    assert_eq!(state.trim(), "unpaired");
    kill(child);
    let (child, again, _) = start(&dir.0);
    assert_eq!(again, key);
    kill(child);
}
