//! The owner's watcher against a Bitcoin node on regtest, as a command that
//! reads simple lines on its standard input and prints one line for each
//! thing it does.
//!
//! ```text
//! owner_regtest <data dir> <node address> [--regtest-single-peer]
//! ```
//!
//! `scripts/instant-e2e-core.sh` uses this together with the counter example
//! (`counter_regtest` in `openbmt-counter-node`): the owner's watcher makes
//! signed heartbeats, and the script carries their bytes to the counter as hex.
//! The node is the one fixed local peer of the driver.
//!
//! **Test only, never use.** The identities are made from fixed bytes that
//! anyone can read here, the same ones as in `counter_regtest`.
//!
//! `--regtest-single-peer` replaces the answer of the health rules with
//! "healthy", as in `counter_regtest`: one local node is one peer and, on
//! loopback, one network group (WR-41), so the real rules cannot hold. The
//! real answer is printed by the `health` and `groups` commands. A run with
//! the switch is not a check of the health rules.
//!
//! Commands, one per line:
//!
//! - `heartbeat [owner|approver|other-counter]`: make the next heartbeat for
//!   the counter. `owner` (the default) is the real one. `approver` is signed
//!   with the Key app's key, and `other-counter` names another counter, both
//!   to check that the counter refuses them. All three use up a number.
//! - `pump <seconds>`
//! - `health`
//! - `groups`
//! - `status`
//! - `quit`
//!
//! Every command ends with a line `done <n>`.
//!
//! Lines printed:
//!
//! - `warning ...` for the single-peer switch
//! - `heartbeat <hex>` (or `heartbeat none <reason>`)
//! - `health used=<healthy|unhealthy> real=<ok|problem> peers=<n>`
//! - `groups peers=<n> groups=<n> filter_groups=<n>`
//! - `status tip=<h> hash=<hex> seq=<n>`
//! - `note <text>`
//! - `stopped`

use std::collections::BTreeSet;
use std::io::{BufRead, Write};
use std::net::SocketAddr;
use std::process::ExitCode;
use std::sync::mpsc::{channel, TryRecvError};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use bitcoin::hashes::Hash;
use bitcoin::Network;
use openbmt_core::heartbeat::WatcherHeartbeat;
use openbmt_core::identity::{DeviceId, Identity};
use openbmt_net_io::DriverConfig;
use openbmt_owner_node::{OwnerConfig, OwnerWatcher};
use openbmt_watch::{check_health, network_group, HeaderChain, Health, HealthInput};

const USAGE: &str = "usage: owner_regtest <data dir> <node address> [--regtest-single-peer]";

/// TEST ONLY, NEVER USE: the secret bytes of the identities.
const OWNER_SECRET: [u8; 32] = [0x11; 32];
const APPROVER_SECRET: [u8; 32] = [0x12; 32];
const COUNTER_SECRET: [u8; 32] = [0x13; 32];
const OTHER_COUNTER_SECRET: [u8; 32] = [0x14; 32];

/// What the health function saw last, for the `health` and `groups` lines.
#[derive(Debug, Default)]
struct Seen {
    real: Option<Health>,
    peers: usize,
    groups: usize,
    filter_groups: usize,
}

fn identity(secret: &[u8; 32]) -> Result<Identity, String> {
    Identity::from_secret_bytes(secret).map_err(|e| format!("identity: {e}"))
}

fn say(line: impl AsRef<str>) {
    println!("{}", line.as_ref());
    let _ = std::io::stdout().flush();
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn show_notes(watcher: &mut OwnerWatcher) {
    for note in watcher.take_notes() {
        say(format!("note {note}"));
    }
}

/// Make a heartbeat the way `OwnerWatcher::heartbeat` does, but signed with
/// another key or naming another counter, for the counter to refuse. It
/// takes the watcher's next number, so the number is used up like a real one.
fn variant(
    watcher: &mut OwnerWatcher,
    counter: DeviceId,
    signer: &Identity,
) -> Result<Vec<u8>, String> {
    // Make a real heartbeat to take its number, time and tip, then sign the
    // same fields again.
    let real = watcher
        .heartbeat(counter)
        .ok_or("the watcher made no heartbeat")?;
    let parsed =
        openbmt_core::heartbeat::SignedHeartbeat::from_bytes(&real).map_err(|e| e.to_string())?;
    let fields = WatcherHeartbeat {
        counter,
        ..parsed.heartbeat
    };
    Ok(fields.sign(signer).to_bytes())
}

fn command(
    watcher: &mut OwnerWatcher,
    line: &str,
    seen: &Arc<Mutex<Seen>>,
) -> Result<bool, String> {
    let counter = identity(&COUNTER_SECRET)?.device_id();
    let mut words = line.split_whitespace();
    match words.next() {
        Some("heartbeat") => {
            let made = match words.next().unwrap_or("owner") {
                "owner" => watcher.heartbeat(counter),
                "approver" => Some(variant(watcher, counter, &identity(&APPROVER_SECRET)?)?),
                "other-counter" => {
                    let other = identity(&OTHER_COUNTER_SECRET)?.device_id();
                    Some(variant(watcher, other, &identity(&OWNER_SECRET)?)?)
                }
                other => return Err(format!("unknown kind of heartbeat: {other}")),
            };
            match made {
                Some(bytes) => say(format!("heartbeat {}", hex(&bytes))),
                None => say("heartbeat none it could not be made (see the notes)"),
            }
            show_notes(watcher);
        }
        Some("pump") => {
            let secs: f64 = words
                .next()
                .ok_or("the seconds are missing")?
                .parse()
                .map_err(|_| "the seconds are not a number".to_string())?;
            watcher.pump(Duration::from_secs_f64(secs.clamp(0.0, 600.0)));
            show_notes(watcher);
        }
        Some("health") => {
            let seen = seen.lock().map_err(|_| "lock".to_string())?;
            let used = match watcher.health() {
                Some(h) if h.healthy => "healthy",
                _ => "unhealthy",
            };
            let real = match seen.real {
                Some(h) => h.problem.map_or("ok".to_string(), |p| format!("{p:?}")),
                None => "none-yet".to_string(),
            };
            say(format!(
                "health used={used} real={real} peers={}",
                seen.peers
            ));
        }
        Some("groups") => {
            let seen = seen.lock().map_err(|_| "lock".to_string())?;
            say(format!(
                "groups peers={} groups={} filter_groups={}",
                seen.peers, seen.groups, seen.filter_groups
            ));
        }
        Some("status") => {
            let tip = watcher.chain().tip_height();
            let hash = watcher
                .chain()
                .hash_at(tip)
                .map_or("none".to_string(), |h| hex(&h.to_byte_array()));
            say(format!(
                "status tip={tip} hash={hash} seq={}",
                watcher.last_seq()
            ));
        }
        Some("quit") => return Ok(false),
        Some(other) => return Err(format!("unknown command: {other}")),
        None => {}
    }
    Ok(true)
}

fn run(args: &[String]) -> Result<(), String> {
    let mut single_peer = false;
    let mut rest = Vec::new();
    for arg in args {
        if arg == "--regtest-single-peer" {
            single_peer = true;
        } else {
            rest.push(arg.as_str());
        }
    }
    let [dir, peer] = rest.as_slice() else {
        return Err(USAGE.to_string());
    };
    let peer: SocketAddr = peer.parse().map_err(|_| format!("node address: {peer}"))?;
    let seen = Arc::new(Mutex::new(Seen::default()));

    let mut driver = DriverConfig::new(Network::Regtest, 0);
    driver.allow_local = true;
    driver.fixed_peers = vec![peer];
    let mut config = OwnerConfig::new(dir, identity(&OWNER_SECRET)?, driver);
    let seen_by_rules = seen.clone();
    config.health = Box::new(move |input: &HealthInput<'_>| {
        let real = check_health(input);
        if let Ok(mut s) = seen_by_rules.lock() {
            s.real = Some(real);
            s.peers = input.peers.len();
            s.groups = input
                .peers
                .iter()
                .map(|p| network_group(&p.addr))
                .collect::<BTreeSet<_>>()
                .len();
            s.filter_groups = input
                .peers
                .iter()
                .filter(|p| p.serves_filters)
                .map(|p| network_group(&p.addr))
                .collect::<BTreeSet<_>>()
                .len();
        }
        if single_peer {
            // Regtest with local nodes only. Not the rules.
            Health {
                healthy: true,
                problem: None,
                behind: real.behind,
            }
        } else {
            real
        }
    });
    if single_peer {
        say("warning --regtest-single-peer: the health answer is replaced by \"healthy\"");
        say("warning loopback addresses are one network group, so WR-50 cannot hold:");
        say("warning THIS RUN IS NOT A CHECK OF THE HEALTH RULES");
    }
    let mut watcher = OwnerWatcher::start(config).map_err(|e| e.to_string())?;
    say(format!(
        "started tip={} owner={}",
        watcher.chain().tip_height(),
        hex(&watcher.device_id().to_bytes())
    ));

    let (lines, input) = channel();
    std::thread::spawn(move || {
        for line in std::io::stdin().lock().lines() {
            match line {
                Ok(l) => {
                    if lines.send(l).is_err() {
                        return;
                    }
                }
                Err(_) => return,
            }
        }
    });

    let mut done = 0u64;
    loop {
        match input.try_recv() {
            Ok(line) => {
                match command(&mut watcher, line.trim(), &seen) {
                    Ok(true) => {}
                    Ok(false) => break,
                    Err(e) => say(format!("error {e}")),
                }
                done += 1;
                say(format!("done {done}"));
            }
            Err(TryRecvError::Empty) => {
                watcher.pump(Duration::from_millis(100));
                show_notes(&mut watcher);
            }
            Err(TryRecvError::Disconnected) => break,
        }
    }
    watcher.shutdown();
    show_notes(&mut watcher);
    say("stopped");
    Ok(())
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(why) => {
            say(format!("failed {why}"));
            ExitCode::from(1)
        }
    }
}
