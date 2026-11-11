//! The owner phone's background watcher against a Bitcoin node on regtest:
//! the watch-only mode of the owner node (`WatchNode`, LK-75), as a command
//! that reads simple lines on its standard input and prints one line for
//! each thing it does.
//!
//! ```text
//! watch_regtest <data dir> <node address> --relay <address> --relay-key <hex>
//!     [--regtest-single-peer] [--heartbeat-every <seconds>]
//!     [--heartbeat-max-age <seconds>]
//! ```
//!
//! `scripts/link-e2e-core.sh` uses this after the owner example
//! (`owner_regtest`) has stopped, on the same data directory, so that the
//! heartbeat number carries on from the owner's. The watcher has no owner
//! secret, no wallet and no key 1: only the watch key's secret and the one
//! counter of `counter_regtest`. It follows the chain through the node and
//! sends the counter a heartbeat signed by the watch key through the relay.
//!
//! The command also plays the owner's and the Key app's part of settings, for
//! the script: it makes the signed settings that name the watch key (a
//! loosening, signed by the owner and the test approver) and that remove it
//! (a tightening, signed by the owner alone). Those are made here from the
//! fixed test keys of `counter_regtest` and are not the watcher's: the
//! watcher itself signs nothing but heartbeats.
//!
//! `--regtest-single-peer` replaces the answer of the health rules with
//! "healthy", as in `owner_regtest`; a run with it is not a check of the
//! health rules.
//!
//! **Test only, never use.** Every key is made from fixed bytes.
//!
//! Commands, one per line:
//!
//! `watch_regtest --settings <version> name|remove [--heartbeat-max-age <seconds>]`
//! prints `settings <hex>` as the command below does and exits, without
//! starting anything.
//!
//! - `watchkey`: prints the watch key's public key
//! - `settings <version> name|remove`: prints the signed settings, at that
//!   version, that name or remove the watch key
//! - `heartbeat`: prints a heartbeat for the counter, signed by the watch
//!   key, as hex (it takes a number)
//! - `pump <seconds>`
//! - `state`
//! - `quit`
//!
//! Every command ends with a line `done <n>`. Lines printed: `watchkey
//! <hex>`, `settings <hex>`, `heartbeat <hex>`, `state sent=<n> reached=<bool>
//! relay_refused=<bool> tip=<h>`, `note <text>`, `started` and `stopped`.

use std::io::{BufRead, Write};
use std::net::SocketAddr;
use std::process::ExitCode;
use std::sync::mpsc::{channel, TryRecvError};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use bitcoin::Network;
use openbmt_core::identity::{DeviceId, Identity};
use openbmt_core::settings::{Settings, SignedSettings};
use openbmt_link::Route;
use openbmt_net_io::DriverConfig;
use openbmt_owner_node::{WatchConfig, WatchLinkConfig, WatchNode};
use openbmt_watch::{check_health, HeaderChain, Health, HealthInput};

const USAGE: &str = "usage: watch_regtest <data dir> <node address> --relay <address> \
                     --relay-key <hex> [--regtest-single-peer] [--heartbeat-every <seconds>] \
                     [--heartbeat-max-age <seconds>]";

/// TEST ONLY, NEVER USE: the secret bytes of the identities, as in
/// `owner_regtest`, and the watch key's own.
const OWNER_SECRET: [u8; 32] = [0x11; 32];
const APPROVER_SECRET: [u8; 32] = [0x12; 32];
const COUNTER_SECRET: [u8; 32] = [0x13; 32];
const WATCH_SECRET: [u8; 32] = [0x5a; 32];

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

fn unhex(text: &str) -> Result<Vec<u8>, String> {
    if !text.len().is_multiple_of(2) || !text.is_ascii() {
        return Err("the hex is not whole bytes".to_string());
    }
    (0..text.len() / 2)
        .map(|i| {
            text.get(2 * i..2 * i + 2)
                .and_then(|pair| u8::from_str_radix(pair, 16).ok())
                .ok_or_else(|| "the hex has a character that is not a digit".to_string())
        })
        .collect()
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// The settings `counter_regtest` starts with, at `version`, with the watch
/// key named or not.
fn settings(version: u32, name: bool, max_age: Option<u32>) -> Result<Settings, String> {
    let counter = identity(&COUNTER_SECRET)?.device_id();
    let mut s = Settings::defaults(1_000);
    s.devices.insert(counter);
    s.counters.insert(counter, 10_000);
    if let Some(secs) = max_age {
        s.heartbeat_max_age_secs = secs;
    }
    s.version = version;
    if name {
        s.watch_key = Some(identity(&WATCH_SECRET)?.device_id());
    }
    Ok(s)
}

/// The signed settings that name or remove the watch key, for `--settings`
/// and the `settings` command.
fn signed_settings(
    version: u32,
    what: &str,
    max_age: Option<u32>,
) -> Result<SignedSettings, String> {
    match what {
        "name" => {
            // Naming the key loosens: the owner and the approver sign.
            let s = settings(version, true, max_age)?;
            Ok(SignedSettings {
                owner_sig: s.sign(&identity(&OWNER_SECRET)?),
                approver_sig: Some(s.sign(&identity(&APPROVER_SECRET)?)),
                settings: s,
            })
        }
        "remove" => {
            // Removing it tightens: the owner alone signs.
            let s = settings(version, false, max_age)?;
            Ok(SignedSettings {
                owner_sig: s.sign(&identity(&OWNER_SECRET)?),
                approver_sig: None,
                settings: s,
            })
        }
        _ => Err("settings <version> name|remove".to_string()),
    }
}

fn run(args: &[String]) -> Result<(), String> {
    if args.first().map(String::as_str) == Some("--settings") {
        let version = args
            .get(1)
            .and_then(|v| v.parse().ok())
            .ok_or("--settings <version> name|remove")?;
        let what = args.get(2).ok_or("--settings <version> name|remove")?;
        let max_age = match args.get(3).map(String::as_str) {
            Some("--heartbeat-max-age") => Some(
                args.get(4)
                    .and_then(|v| v.parse().ok())
                    .ok_or("--heartbeat-max-age <seconds>")?,
            ),
            _ => None,
        };
        say(format!(
            "settings {}",
            hex(&signed_settings(version, what, max_age)?.to_bytes())
        ));
        return Ok(());
    }
    let mut single_peer = false;
    let mut relay: Option<SocketAddr> = None;
    let mut relay_key: Option<DeviceId> = None;
    let mut heartbeat_every: Option<u64> = None;
    let mut max_age: Option<u32> = None;
    let mut rest = Vec::new();
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--regtest-single-peer" => single_peer = true,
            "--relay" => {
                let addr = args.next().ok_or(USAGE)?;
                relay = Some(addr.parse().map_err(|_| format!("relay address: {addr}"))?);
            }
            "--relay-key" => {
                let key = args.next().ok_or(USAGE)?;
                let bytes: [u8; 32] = unhex(key)?
                    .try_into()
                    .map_err(|_| "the relay key is not 32 bytes".to_string())?;
                relay_key =
                    Some(DeviceId::from_bytes(&bytes).map_err(|e| format!("relay key: {e}"))?);
            }
            "--heartbeat-every" => {
                let secs = args.next().ok_or(USAGE)?;
                heartbeat_every = Some(secs.parse().map_err(|_| format!("seconds: {secs}"))?);
            }
            "--heartbeat-max-age" => {
                let secs = args.next().ok_or(USAGE)?;
                max_age = Some(secs.parse().map_err(|_| format!("seconds: {secs}"))?);
            }
            other => rest.push(other),
        }
    }
    let [dir, peer] = rest.as_slice() else {
        return Err(USAGE.to_string());
    };
    let (Some(relay), Some(relay_key)) = (relay, relay_key) else {
        return Err(USAGE.to_string());
    };
    let peer: SocketAddr = peer.parse().map_err(|_| format!("node address: {peer}"))?;
    let counter = identity(&COUNTER_SECRET)?.device_id();

    let mut driver = DriverConfig::new(Network::Regtest, 0);
    driver.allow_local = true;
    driver.fixed_peers = vec![peer];
    let mut link = WatchLinkConfig::default();
    if let Some(secs) = heartbeat_every {
        link.heartbeat_every_secs = secs;
    }
    let health: Option<openbmt_counter_node::HealthFn> = if single_peer {
        say("warning --regtest-single-peer: the health answer is replaced by \"healthy\"");
        say("warning THIS RUN IS NOT A CHECK OF THE HEALTH RULES");
        Some(Box::new(|input: &HealthInput<'_>| {
            let real = check_health(input);
            Health {
                healthy: true,
                problem: None,
                behind: real.behind,
            }
        }))
    } else {
        None
    };
    let mut node = WatchNode::start(WatchConfig {
        dir: dir.into(),
        secret: WATCH_SECRET,
        driver,
        counters: vec![counter],
        route: Route::Direct(relay),
        relay: relay_key,
        link,
        health,
    })
    .map_err(|e| e.to_string())?;
    say(format!(
        "started tip={}",
        node.watcher().chain().tip_height()
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
                match command(&mut node, line.trim(), &counter, max_age) {
                    Ok(true) => {}
                    Ok(false) => break,
                    Err(e) => say(format!("error {e}")),
                }
                done += 1;
                say(format!("done {done}"));
            }
            Err(TryRecvError::Empty) => {
                node.pump(Duration::from_millis(100));
                node.step(now_secs());
                for note in node.take_notes() {
                    say(format!("note {note}"));
                }
            }
            Err(TryRecvError::Disconnected) => break,
        }
    }
    node.shutdown();
    say("stopped");
    Ok(())
}

fn command(
    node: &mut WatchNode,
    line: &str,
    counter: &DeviceId,
    max_age: Option<u32>,
) -> Result<bool, String> {
    let mut words = line.split_whitespace();
    match words.next() {
        Some("watchkey") => say(format!(
            "watchkey {}",
            hex(&identity(&WATCH_SECRET)?.device_id().to_bytes())
        )),
        Some("settings") => {
            let version: u32 = words
                .next()
                .ok_or("the version is missing")?
                .parse()
                .map_err(|_| "the version is not a number".to_string())?;
            let signed = signed_settings(version, words.next().unwrap_or(""), max_age)?;
            say(format!("settings {}", hex(&signed.to_bytes())));
        }
        Some("heartbeat") => match node.watcher_mut().heartbeat(*counter) {
            Some(bytes) => say(format!("heartbeat {}", hex(&bytes))),
            None => say("heartbeat none it could not be made"),
        },
        Some("pump") => {
            let secs: f64 = words
                .next()
                .ok_or("the seconds are missing")?
                .parse()
                .map_err(|_| "the seconds are not a number".to_string())?;
            node.pump(Duration::from_secs_f64(secs.clamp(0.0, 600.0)));
        }
        Some("state") => {
            let s = node.state();
            say(format!(
                "state sent={} reached={} relay_refused={} tip={}",
                s.sent,
                s.reached,
                s.relay_refused,
                node.watcher().chain().tip_height()
            ));
        }
        Some("quit") => return Ok(false),
        Some(other) => return Err(format!("unknown command: {other}")),
        None => {}
    }
    Ok(true)
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
