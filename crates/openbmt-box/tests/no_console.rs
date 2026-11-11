//! `openbmt-box --no-console` as a process: ended by SIGKILL at any point,
//! it starts again on the same directory with the same key (BX-52).

use std::io::{BufRead, BufReader, Read, Write};
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

#[test]
fn bx_52_a_box_without_a_console_ends_at_sigterm_and_starts_again_with_its_key() {
    use std::os::unix::process::ExitStatusExt;
    let dir = Dir::new("term");
    let (mut child, key, _) = start(&dir.0);
    // SIGTERM by the system's kill program, which is what a service
    // manager's stop sends. The Box has no handler, so the default action
    // ends it, by that signal and in good time.
    let sent = Command::new("kill")
        .arg("-TERM")
        .arg(child.id().to_string())
        .status()
        .unwrap();
    assert!(sent.success());
    let until = std::time::Instant::now() + Duration::from_secs(5);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        assert!(std::time::Instant::now() < until, "the Box did not stop");
        std::thread::sleep(Duration::from_millis(10));
    };
    assert_eq!(status.signal(), Some(15));
    let (child, again, state) = start(&dir.0);
    assert_eq!(again, key, "the key changed");
    assert_eq!(state.trim(), "unpaired");
    kill(child);
}

// As `spawn`: the path names this build's target folder.
#[allow(clippy::disallowed_macros)]
fn command() -> Command {
    Command::new(env!("CARGO_BIN_EXE_openbmt-box"))
}

/// The Box with its status page on `127.0.0.1:0`; its address as printed.
fn start_with_page(dir: &Path) -> (Child, String) {
    let mut child = command()
        .arg(dir)
        .arg("127.0.0.1:0")
        .arg("--no-console")
        .arg("--status-page")
        .arg("127.0.0.1:0")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
    let mut page = String::new();
    loop {
        let line = lines
            .next()
            .expect("the Box ended before it said its state")
            .unwrap();
        if let Some(addr) = line.strip_prefix("status page: ") {
            page = addr.to_string();
        }
        if line.starts_with("state: ") {
            break;
        }
    }
    // Keep the pipe open and drained so the Box never blocks on it.
    std::thread::spawn(move || for _ in lines {});
    (child, page)
}

fn fetch(addr: &str) -> String {
    let mut s = std::net::TcpStream::connect(addr).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    write!(s, "GET / HTTP/1.1\r\nHost: {addr}\r\n\r\n").unwrap();
    let mut out = String::new();
    let _ = s.read_to_string(&mut out);
    out
}

#[test]
fn bx_61_a_box_started_with_a_status_page_address_serves_it() {
    let dir = Dir::new("page");
    let (child, page) = start_with_page(&dir.0);
    assert!(!page.is_empty(), "the Box did not say where the page is");
    let answer = fetch(&page);
    assert!(answer.starts_with("HTTP/1.1 200 "), "{answer}");
    assert!(answer.contains("Waiting for an invitation"));
    kill(child);
}

#[test]
fn bx_61_a_box_started_without_the_argument_has_no_page() {
    let dir = Dir::new("nopage");
    let mut child = spawn(&dir.0, Stdio::piped());
    let lines = BufReader::new(child.stdout.take().unwrap()).lines();
    for line in lines {
        let line = line.unwrap();
        assert!(!line.starts_with("status page"), "{line}");
        if line.starts_with("state: ") {
            break;
        }
    }
    kill(child);
}

#[test]
fn bx_61_a_status_page_address_that_is_not_one_or_is_taken_stops_the_box() {
    let dir = Dir::new("pagebad");
    for arg in ["not-an-address", "8987"] {
        let status = command()
            .arg(&dir.0)
            .arg("127.0.0.1:0")
            .arg("--no-console")
            .arg("--status-page")
            .arg(arg)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .unwrap();
        assert_eq!(status.code(), Some(2), "{arg}");
    }
    // The argument without a value is wrong too.
    let status = command()
        .arg(&dir.0)
        .arg("127.0.0.1:0")
        .arg("--status-page")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .unwrap();
    assert_eq!(status.code(), Some(2));
    // An address in use is a failure at the start, not a Box with no page.
    let taken = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let status = command()
        .arg(&dir.0)
        .arg("127.0.0.1:0")
        .arg("--no-console")
        .arg("--status-page")
        .arg(taken.local_addr().unwrap().to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .unwrap();
    assert_eq!(status.code(), Some(1));
}
