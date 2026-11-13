//! `openbmt-box till-key` and `--till-listen` as a process (BX-70, BX-71, BX-80).

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use openbmt_box::till::{hex, reply_mac, till_bearer, till_reply_key};

struct Dir(PathBuf);

impl Dir {
    fn new(name: &str) -> Self {
        let base = std::env::var_os("OPENBMT_TEST_TMP")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        let dir = base.join(format!("openbmt-box-till-{}-{name}", std::process::id()));
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
fn program() -> Command {
    Command::new(env!("CARGO_BIN_EXE_openbmt-box"))
}

/// `openbmt-box till-key <dir>`: the lines it printed and whether it worked.
fn till_key(dir: &Path) -> (Vec<String>, bool) {
    let out = program().arg("till-key").arg(dir).output().unwrap();
    let text = String::from_utf8(out.stdout).unwrap();
    (
        text.lines().map(str::to_string).collect(),
        out.status.success(),
    )
}

fn key_of(lines: &[String]) -> String {
    lines
        .iter()
        .find_map(|l| l.strip_prefix("till key: "))
        .expect("a till key line")
        .to_string()
}

#[test]
fn bx_71_the_till_key_command_prints_the_key_once_and_stores_only_its_hash() {
    let dir = Dir::new("key");
    let (lines, ok) = till_key(&dir.0);
    assert!(ok);
    assert_eq!(lines[0], "New till key");
    assert!(lines[2].starts_with("Next: "));
    let key = key_of(&lines);
    assert_eq!(key.len(), 64);
    // Printed on one line only.
    assert_eq!(lines.iter().filter(|l| l.contains(&key)).count(), 1);
    // Stored as the version, the hash of the token and the reply key, none of
    // them the key, with mode 0600, and nothing else is left in the
    // directory.
    let file = dir.0.join("till-key");
    let stored = std::fs::read(&file).unwrap();
    assert_eq!(stored.len(), 65);
    assert_eq!(stored[0], 2);
    assert!(!String::from_utf8_lossy(&stored).contains(&key));
    assert_eq!(
        std::fs::metadata(&file).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert_eq!(std::fs::read_dir(&dir.0).unwrap().count(), 1);
    // A new key replaces the old.
    let (again, ok) = till_key(&dir.0);
    assert!(ok);
    assert_ne!(key_of(&again), key);
    assert_ne!(std::fs::read(&file).unwrap(), stored);
}

#[test]
fn bx_71_the_till_key_command_refuses_a_folder_that_is_not_there() {
    let dir = Dir::new("nokey");
    let missing = dir.0.join("missing");
    let out = program().arg("till-key").arg(&missing).output().unwrap();
    assert!(!out.status.success());
    assert!(out.stdout.is_empty());
    let err = String::from_utf8(out.stderr).unwrap();
    assert!(err.contains("Next: "), "{err}");
    assert!(!err.contains("till key: "));
    // And no folder to name at all.
    let out = program().arg("till-key").output().unwrap();
    assert!(!out.status.success());
}

fn spawn(dir: &Path, extra: &[&str]) -> Child {
    program()
        .arg(dir)
        .arg("127.0.0.1:0")
        .arg("--no-console")
        .args(extra)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap()
}

/// The lines the Box prints up to its state line.
fn startup(child: &mut Child) -> Vec<String> {
    let mut lines = Vec::new();
    for line in BufReader::new(child.stdout.take().unwrap()).lines() {
        let line = line.unwrap();
        let done = line.starts_with("state: ");
        lines.push(line);
        if done {
            break;
        }
    }
    lines
}

fn kill(mut child: Child) {
    child.kill().unwrap();
    child.wait().unwrap();
}

#[test]
fn bx_70_the_till_listener_is_off_unless_the_box_is_asked() {
    let dir = Dir::new("off");
    let mut child = spawn(&dir.0, &[]);
    let lines = startup(&mut child);
    assert!(
        lines.iter().all(|l| !l.starts_with("till listener: ")),
        "{lines:?}"
    );
    kill(child);
}

#[test]
fn bx_70_with_till_listen_the_box_serves_json_and_says_where() {
    let dir = Dir::new("on");
    let (key_lines, ok) = till_key(&dir.0);
    assert!(ok);
    let key = key_of(&key_lines);
    let key_bytes: [u8; 32] = (0..32)
        .map(|i| u8::from_str_radix(&key[i * 2..i * 2 + 2], 16).unwrap())
        .collect::<Vec<u8>>()
        .try_into()
        .unwrap();
    let mut child = spawn(&dir.0, &["--till-listen", "127.0.0.1:0"]);
    let lines = startup(&mut child);
    let addr = lines
        .iter()
        .find_map(|l| l.strip_prefix("till listener: "))
        .expect("the till listener line");
    let nonce = "0123456789abcdef0123456789abcdef";
    let path = format!("/till/v1/sales/{}", "0".repeat(32));
    let ask = |bearer: &str| {
        let mut s = TcpStream::connect(addr).unwrap();
        s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        s.write_all(
            format!(
                "GET {path} HTTP/1.1\r\nHost: b\r\nX-OBM-Nonce: {nonce}\r\n\
                 Authorization: Bearer {bearer}\r\n\r\n"
            )
            .as_bytes(),
        )
        .unwrap();
        let mut text = String::new();
        s.read_to_string(&mut text).unwrap();
        text
    };
    let mac_of = |text: &str| {
        text.lines()
            .find_map(|l| l.strip_prefix("X-OBM-Reply: "))
            .map(str::to_string)
    };
    let body_of = |text: &str| text.split_once("\r\n\r\n").unwrap().1.to_string();
    // A token that is not the Box's: 401 and the words, signed under the
    // reply key made from the key.
    let text = ask(&"0".repeat(64));
    assert!(text.starts_with("HTTP/1.1 401 "), "{text}");
    assert!(text.contains("\"error\":\"tillKeyWrong\""), "{text}");
    assert!(text.contains("Content-Type: application/json"));
    let reply = till_reply_key(&key_bytes);
    let want = reply_mac(
        &reply,
        "GET",
        &path,
        nonce,
        b"",
        401,
        body_of(&text).as_bytes(),
    );
    assert_eq!(mac_of(&text), Some(want));
    // The key itself is not the token; the token is, and the answer to it is
    // signed too.
    assert!(ask(&key).starts_with("HTTP/1.1 401 "));
    let text = ask(&hex(&till_bearer(&key_bytes)));
    assert!(text.starts_with("HTTP/1.1 503 "), "{text}");
    let want = reply_mac(
        &reply,
        "GET",
        &path,
        nonce,
        b"",
        503,
        body_of(&text).as_bytes(),
    );
    assert_eq!(mac_of(&text), Some(want));
    kill(child);
}

#[test]
fn bx_70_an_address_that_is_not_one_stops_the_box_at_the_start() {
    let dir = Dir::new("bad");
    for arg in ["localhost", "127.0.0.1", "300.1.1.1:80"] {
        let out = program()
            .arg(&dir.0)
            .arg("127.0.0.1:0")
            .arg("--no-console")
            .args(["--till-listen", arg])
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert!(!out.status.success(), "{arg}");
        let err = String::from_utf8(out.stderr).unwrap();
        assert!(err.contains("Address not understood"), "{err}");
    }
    // An address that cannot be bound (taken by another listener).
    let taken = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let out = program()
        .arg(&dir.0)
        .arg("127.0.0.1:0")
        .arg("--no-console")
        .args(["--till-listen", &taken.local_addr().unwrap().to_string()])
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(!String::from_utf8(out.stdout)
        .unwrap()
        .contains("till listener: "));
    // And the flag with nothing after it.
    let out = program()
        .arg(&dir.0)
        .arg("127.0.0.1:0")
        .arg("--till-listen")
        .output()
        .unwrap();
    assert!(!out.status.success());
}
