//! The owner's watcher against a Bitcoin node on regtest, as a command that
//! reads simple lines on its standard input and prints one line for each
//! thing it does.
//!
//! ```text
//! owner_regtest <data dir> <node address> [--regtest-single-peer]
//!     [--relay <address> --relay-key <hex> [--heartbeat-every <seconds>]]
//! ```
//!
//! `scripts/instant-e2e-core.sh` uses this together with the counter example
//! (`counter_regtest` in `openbmt-counter-node`): the owner's watcher makes
//! signed heartbeats, and the script carries their bytes to the counter as hex.
//! `scripts/link-e2e-core.sh` uses the link instead (below). The node is the
//! one fixed local peer of the driver.
//!
//! `--relay <address> --relay-key <hex>` starts the link (`OwnerLink`,
//! `docs/LINK-RULES.md` section 6) next to the watcher: the relay's address
//! and its key (the first line the relay example prints). The one counter is
//! the example counter's fixed test key (`counter_regtest`). The link step
//! runs in the loop between commands: it sends the heartbeat of the watcher to
//! the counter every `--heartbeat-every` seconds (20 by default), takes the
//! counter's records into a verified copy kept under `<data dir>/counters`,
//! takes its check-ins, and answers with what it holds. The two number files
//! of the link (LK-53) are kept in the data directory: the start is a first
//! start if neither is there, and any other start needs both.
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
//! - `linkstate`
//! - `copies`
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
//! - `linkstate alerts=<n> refused=<n> reachable=<bool>`: the number of alerts
//!   now raised, the messages the courier refused, and whether the last call
//!   to the relay worked
//! - `copy counter=<hex> records=<n> head=<hex> check_in_seq=<n|none>
//!   check_in_held=<true|false|none> last_check_in_at=<secs|none>`, for each
//!   counter (`copies`): the length and head of the owner's copy of its chain,
//!   the latest check-in kept, and whether the copy holds it at its place
//! - `alert <text>`, once for each alert as it is raised
//! - `note <text>`
//! - `stopped`

use std::collections::{BTreeMap, BTreeSet};
use std::io::{BufRead, Write};
use std::net::SocketAddr;
use std::path::Path;
use std::process::ExitCode;
use std::sync::mpsc::{channel, TryRecvError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use bitcoin::hashes::Hash;
use bitcoin::Network;
use openbmt_core::heartbeat::WatcherHeartbeat;
use openbmt_core::identity::{DeviceId, Identity};
use openbmt_core::records::DeviceList;
use openbmt_courier::{Courier, CourierConfig, FirstStart};
use openbmt_link::{ClientConfig, Role, Route};
use openbmt_net_io::DriverConfig;
use openbmt_owner_node::{OwnerConfig, OwnerLink, OwnerLinkConfig, OwnerWatcher};
use openbmt_watch::{check_health, network_group, HeaderChain, Health, HealthInput};

const USAGE: &str = "usage: owner_regtest <data dir> <node address> [--regtest-single-peer] \
                     [--relay <address> --relay-key <hex> [--heartbeat-every <seconds>]]";

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

/// The link to the counter through the relay. The counter is the fixed test
/// key of `counter_regtest`.
fn start_link(
    dir: &str,
    addr: SocketAddr,
    relay_key: DeviceId,
    heartbeat_every: Option<u64>,
) -> Result<OwnerLink, String> {
    let counter = identity(&COUNTER_SECRET)?.device_id();
    let owner = identity(&OWNER_SECRET)?.device_id();
    let first = if ["link-inbox", "link-outbox"]
        .iter()
        .any(|f| Path::new(dir).join(f).exists())
    {
        FirstStart::No
    } else {
        FirstStart::Yes
    };
    let courier = Courier::start(
        CourierConfig {
            dir: dir.into(),
            identity: identity(&OWNER_SECRET)?,
            role: Role::Owner,
            peers: BTreeMap::from([(counter, Role::Counter)]),
            route: Route::Direct(addr),
            relay: relay_key,
            client: ClientConfig::default(),
        },
        first,
    )
    .map_err(|e| e.to_string())?;
    let mut config = OwnerLinkConfig::default();
    if let Some(secs) = heartbeat_every {
        config.heartbeat_every_secs = secs;
    }
    OwnerLink::new(
        courier,
        dir,
        DeviceList::new([owner, counter]),
        vec![counter],
        config,
    )
    .map_err(|e| e.to_string())
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
    link: Option<&OwnerLink>,
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
        Some("linkstate") => {
            let link = link.ok_or("the link is not started: give --relay and --relay-key")?;
            say(format!(
                "linkstate alerts={} refused={} reachable={}",
                link.alerts().len(),
                link.courier().refused(),
                link.courier().reachable()
            ));
        }
        Some("copies") => {
            let link = link.ok_or("the link is not started: give --relay and --relay-key")?;
            for counter in link.counters() {
                let state = link.state(counter).ok_or("no copy of that counter")?;
                say(format!(
                    "copy counter={} records={} head={} check_in_seq={} check_in_held={} \
                     last_check_in_at={}",
                    hex(&counter.to_bytes()),
                    state.records,
                    hex(&state.head),
                    state
                        .check_in_seq
                        .map_or("none".to_string(), |n| n.to_string()),
                    state
                        .check_in_held
                        .map_or("none".to_string(), |b| b.to_string()),
                    state
                        .last_check_in_at
                        .map_or("none".to_string(), |t| t.to_string()),
                ));
            }
        }
        Some("quit") => return Ok(false),
        Some(other) => return Err(format!("unknown command: {other}")),
        None => {}
    }
    Ok(true)
}

fn run(args: &[String]) -> Result<(), String> {
    let mut single_peer = false;
    let mut relay: Option<SocketAddr> = None;
    let mut relay_key: Option<DeviceId> = None;
    let mut heartbeat_every: Option<u64> = None;
    let mut rest = Vec::new();
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        if arg == "--regtest-single-peer" {
            single_peer = true;
        } else if arg == "--relay" {
            let addr = args.next().ok_or(USAGE)?;
            relay = Some(addr.parse().map_err(|_| format!("relay address: {addr}"))?);
        } else if arg == "--relay-key" {
            let key = args.next().ok_or(USAGE)?;
            let bytes: [u8; 32] = unhex(key)?
                .try_into()
                .map_err(|_| "the relay key is not 32 bytes".to_string())?;
            relay_key = Some(DeviceId::from_bytes(&bytes).map_err(|e| format!("relay key: {e}"))?);
        } else if arg == "--heartbeat-every" {
            let secs = args.next().ok_or(USAGE)?;
            heartbeat_every = Some(secs.parse().map_err(|_| format!("seconds: {secs}"))?);
        } else {
            rest.push(arg.as_str());
        }
    }
    let [dir, peer] = rest.as_slice() else {
        return Err(USAGE.to_string());
    };
    if relay.is_some() != relay_key.is_some() {
        return Err("--relay and --relay-key go together".to_string());
    }
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
    let mut link = match (relay, relay_key) {
        (Some(addr), Some(key)) => Some(start_link(dir, addr, key, heartbeat_every)?),
        _ => None,
    };
    let mut alerts_shown = 0usize;

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
                match command(&mut watcher, link.as_ref(), line.trim(), &seen) {
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
                if let Some(link) = link.as_mut() {
                    link.step(&mut watcher, now_secs());
                    for note in link.take_notes() {
                        say(format!("note link {note}"));
                    }
                    let alerts = link.alerts();
                    for alert in alerts.iter().skip(alerts_shown) {
                        say(format!("alert {alert:?}"));
                    }
                    alerts_shown = alerts.len();
                }
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
