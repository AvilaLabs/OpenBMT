//! A relay on a TCP port.
//!
//! ```text
//! relay <data dir> <bind addr> [--test-identity]
//! ```
//!
//! The relay's identity is read from `<data dir>/identity.key` (32 bytes),
//! made from the operating system's randomness the first time it starts,
//! with mode 0600. `--test-identity` uses a fixed, public key instead and
//! writes no key file: for tests only. The members are the identity keys of
//! the shop's devices, one 64-hex-character key per line in
//! `<data dir>/members`; blank lines and `#` comments are skipped and
//! anything else stops the start. Without the file the relay has no members
//! and takes nothing. The list is put there by hand until the Box's pairing
//! is settled. Mailboxes are kept in
//! `<data dir>/mailboxes`. The relay's public key is printed on the first
//! line, then the address it listens on.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::net::TcpListener;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use openbmt_core::identity::{DeviceId, Identity};
use openbmt_link::store::DirStore;
use openbmt_link::{os_random, Relay, RelayConfig, Server, ServerConfig};
use openbmt_net_io::{DirLock, SystemClock};

/// The key `--test-identity` uses.
const TEST_SECRET: [u8; 32] = [99; 32];

fn load_identity(dir: &Path) -> Result<Identity, String> {
    let path = dir.join("identity.key");
    if !path.exists() {
        let mut secret = [0u8; 32];
        os_random(&mut secret).map_err(|e| format!("no randomness: {e}"))?;
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)
            .map_err(|e| format!("cannot make {}: {e}", path.display()))?;
        file.write_all(&secret)
            .and_then(|()| file.sync_all())
            .map_err(|e| format!("cannot write {}: {e}", path.display()))?;
    }
    let bytes = fs::read(&path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let secret: [u8; 32] = bytes
        .as_slice()
        .try_into()
        .map_err(|_| format!("{} is not 32 bytes", path.display()))?;
    Identity::from_secret_bytes(&secret).map_err(|e| format!("{}: {e}", path.display()))
}

fn read_members(dir: &Path) -> Result<Vec<DeviceId>, String> {
    let path = dir.join("members");
    let text = match fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(format!("cannot read {}: {e}", path.display())),
    };
    let mut members = Vec::new();
    for (n, line) in text.lines().enumerate() {
        let line = line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let bad = || format!("{}:{}: not a 64-character hex key", path.display(), n + 1);
        if line.len() != 64 || !line.is_ascii() {
            return Err(bad());
        }
        let mut key = [0u8; 32];
        for (slot, pair) in key.iter_mut().zip(line.as_bytes().chunks(2)) {
            let text = std::str::from_utf8(pair).map_err(|_| bad())?;
            *slot = u8::from_str_radix(text, 16).map_err(|_| bad())?;
        }
        members.push(
            DeviceId::from_bytes(&key)
                .map_err(|_| format!("{}:{}: not a valid key", path.display(), n + 1))?,
        );
    }
    Ok(members)
}

fn run() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let test = args.iter().any(|a| a == "--test-identity");
    let plain: Vec<&String> = args.iter().filter(|a| !a.starts_with("--")).collect();
    let [dir, bind] = plain.as_slice() else {
        return Err("usage: relay <data dir> <bind addr> [--test-identity]".into());
    };
    let dir = Path::new(dir.as_str());
    // NR-79: this process owns the directory until it ends. Taking the lock
    // makes the directory if it is missing.
    let _lock = DirLock::acquire(dir).map_err(|e| e.to_string())?;
    let identity = if test {
        Identity::from_secret_bytes(&TEST_SECRET).map_err(|e| e.to_string())?
    } else {
        load_identity(dir)?
    };
    let key = identity.device_id().to_bytes();
    let store = DirStore::new(dir.join("mailboxes")).map_err(|e| e.to_string())?;
    let mut relay =
        Relay::new(identity, RelayConfig::default(), store).map_err(|e| e.to_string())?;
    let members = read_members(dir)?;
    relay.set_members(&members).map_err(|e| e.to_string())?;
    let count = relay.members().count();
    let listener =
        TcpListener::bind(bind.as_str()).map_err(|e| format!("cannot bind {bind}: {e}"))?;
    let server = Server::start(
        listener,
        relay,
        Arc::new(SystemClock),
        ServerConfig::default(),
    )
    .map_err(|e| e.to_string())?;
    let hex: String = key.iter().map(|b| format!("{b:02x}")).collect();
    println!("{hex}");
    println!("{}", server.local_addr());
    println!("{count} members");
    loop {
        std::thread::sleep(Duration::from_secs(3600));
    }
}

fn main() {
    if let Err(e) = run() {
        eprintln!("relay: {e}");
        std::process::exit(2);
    }
}
