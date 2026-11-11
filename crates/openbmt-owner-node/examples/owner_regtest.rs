//! The owner's watcher against a Bitcoin node on regtest, as a command that
//! reads simple lines on its standard input and prints one line for each
//! thing it does.
//!
//! ```text
//! owner_regtest <data dir> <node address> [--regtest-single-peer]
//!     [--relay <address> --relay-key <hex> [--heartbeat-every <seconds>]
//!     [--batch-resend <seconds>]]
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
//! The refund requests in the counter's records are applied to the owner's
//! queue (`RefundIntake`, `docs/COUNTER-RULES.md` CR-60 to CR-74) after each
//! link step. The queue is the owner node's (`OwnerNode`), over the same
//! fixed test wallet as the counter's; the intake's own state is kept in the
//! data directory. The facts of a sale, what it received and whether that
//! reached the refund depth, come from the node's own coin view
//! (`docs/OWNER-COINS-RULES.md`, OC-14, OC-15), never from the counter's
//! records. At its first start the owner signs one batch of 20 addresses
//! for the counter (OC-10), which is what makes the sale addresses the
//! owner's; the node then watches them.
//!
//! **Test only, never use.** The identities and the wallet keys are made from
//! fixed bytes that anyone can read here, the same ones as in
//! `counter_regtest`.
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
//! - `sendbatch <start>`: sign a batch of 20 addresses from `<start>` for the
//!   counter and hand it to the link, which keeps it and sends it again until
//!   the counter's records show it held (LK-70, LK-71). The owner's queue is
//!   not used: the batch goes straight to the link.
//! - `batches`
//! - `pump <seconds>`
//! - `health`
//! - `groups`
//! - `status`
//! - `linkstate`
//! - `copies`
//! - `refunds`
//! - `facts <sale>`
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
//! - `batches counter=<hex> waiting=<n> since=<secs|none>`: the batches that
//!   wait for the counter to hold them
//! - `linkstate alerts=<n> refused=<n> reachable=<bool>`: the number of alerts
//!   now raised, the messages the courier refused, and whether the last call
//!   to the relay worked
//! - `copy counter=<hex> records=<n> head=<hex> check_in_seq=<n|none>
//!   check_in_held=<true|false|none> last_check_in_at=<secs|none>`, for each
//!   counter (`copies`): the length and head of the owner's copy of its chain,
//!   the latest check-in kept, and whether the copy holds it at its place
//! - `refund sale=<i> counter=<hex> sats=<n> address=<address> cashier=<name>
//!   stage=<Stage>`, for each refund on the owner's queue (`refunds`), and
//!   `refunds queued=<n> notes=<n>`
//! - `refundnote seq=<n> number=<n> sale=<i> kind=<Kind> text=<text>`, for
//!   each line of the intake's list (`refunds`)
//! - `facts sale=<i> held_received=<n> held_deep=<bool> view_received=<n>
//!   view_deep=<bool> receipts=<n>` (`facts`): what the owner's queue holds
//!   for the sale, and what the node's own view shows now, with the number
//!   of payments to the sale's address that the view recorded; or
//!   `facts sale=<i> refused <why>` when the view cannot say
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

use bitcoin::bip32::{DerivationPath, Xpriv, Xpub};
use bitcoin::hashes::Hash;
use bitcoin::secp256k1::Secp256k1;
use bitcoin::{Address, Network};
use openbmt_core::batch::{BatchBuilder, BatchPolicy, SignedBatch};
use openbmt_core::heartbeat::WatcherHeartbeat;
use openbmt_core::identity::{DeviceId, Identity};
use openbmt_core::records::DeviceList;
use openbmt_core::wallet::{KeySlot, WalletDescription};
use openbmt_courier::{Courier, CourierConfig, FirstStart};
use openbmt_link::{ClientConfig, Role, Route};
use openbmt_net_io::DriverConfig;
use openbmt_owner::{Options, SaleKey};
use openbmt_owner_node::{
    verified_facts, OwnerConfig, OwnerLink, OwnerLinkConfig, OwnerNode, OwnerParts, OwnerWatcher,
    RefundIntake,
};
use openbmt_watch::{check_health, network_group, HeaderChain, Health, HealthInput};

const USAGE: &str = "usage: owner_regtest <data dir> <node address> [--regtest-single-peer] \
                     [--relay <address> --relay-key <hex> [--heartbeat-every <seconds>]\n                     [--batch-resend <seconds>]]";

/// TEST ONLY, NEVER USE: the secret bytes of the identities.
const OWNER_SECRET: [u8; 32] = [0x11; 32];
const APPROVER_SECRET: [u8; 32] = [0x12; 32];
const COUNTER_SECRET: [u8; 32] = [0x13; 32];
const OTHER_COUNTER_SECRET: [u8; 32] = [0x14; 32];
/// TEST ONLY, NEVER USE: the seeds of the three wallet keys, as in
/// `counter_regtest`.
const KEY_SEEDS: [[u8; 32]; 3] = [[0xa1; 32], [0xa2; 32], [0xa3; 32]];
/// The batch size the counter example uses.
const ADDRESSES: u32 = 20;

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

/// One of the three test keys, on the wallet's account path for regtest.
fn test_slot(index: u8, seed: &[u8; 32]) -> Result<KeySlot, String> {
    let secp = Secp256k1::new();
    let master = Xpriv::new_master(Network::Regtest, seed).map_err(|e| e.to_string())?;
    let path: DerivationPath = "m/48h/1h/0h/2h".parse().map_err(|_| "path".to_string())?;
    let account = master
        .derive_priv(&secp, &path)
        .map_err(|e| e.to_string())?;
    Ok(KeySlot {
        index,
        origin_fingerprint: master.fingerprint(&secp),
        origin_path: path,
        xpub: Xpub::from_priv(&secp, &account),
    })
}

/// The wallet of the counter example.
fn test_wallet() -> Result<WalletDescription, String> {
    let slots = [
        test_slot(1, &KEY_SEEDS[0])?,
        test_slot(2, &KEY_SEEDS[1])?,
        test_slot(3, &KEY_SEEDS[2])?,
    ];
    WalletDescription::new(Network::Regtest, slots).map_err(|e| e.to_string())
}

/// The refund depth of the owner, as the counter example's final state.
const REFUND_DEPTH: u32 = 6;

/// OC-10: sign the one batch of addresses for the counter, if the owner has
/// not done it yet, and tell the node to watch them.
fn issue_addresses(node: &mut OwnerNode, wallet: &WalletDescription) -> Result<(), String> {
    let counter = identity(&COUNTER_SECRET)?.device_id();
    if node.owner().counter_of(0) == Some(counter) {
        return Ok(());
    }
    let batch = BatchBuilder::new(BatchPolicy {
        min_count: ADDRESSES,
    })
    .derive(wallet, counter, 0, ADDRESSES)
    .map_err(|e| e.to_string())?;
    let write = node.queue_writer();
    node.owner_mut()
        .map_err(|e| e.to_string())?
        .queue_batch(batch, now_secs(), &write)
        .map_err(|e| e.to_string())?;
    node.refresh_marks();
    Ok(())
}

/// The link to the counter through the relay. The counter is the fixed test
/// key of `counter_regtest`.
fn start_link(
    dir: &str,
    addr: SocketAddr,
    relay_key: DeviceId,
    heartbeat_every: Option<u64>,
    batch_resend: Option<u64>,
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
    if let Some(secs) = batch_resend {
        config.batch_resend_secs = secs;
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

fn show_notes(node: &mut OwnerNode) {
    for note in node.watcher_mut().take_notes() {
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
    node: &mut OwnerNode,
    mut link: Option<&mut OwnerLink>,
    intake: &RefundIntake,
    wallet: &WalletDescription,
    line: &str,
    seen: &Arc<Mutex<Seen>>,
) -> Result<bool, String> {
    let counter = identity(&COUNTER_SECRET)?.device_id();
    let mut words = line.split_whitespace();
    match words.next() {
        Some("heartbeat") => {
            let made = match words.next().unwrap_or("owner") {
                "owner" => node.watcher_mut().heartbeat(counter),
                "approver" => Some(variant(
                    node.watcher_mut(),
                    counter,
                    &identity(&APPROVER_SECRET)?,
                )?),
                "other-counter" => {
                    let other = identity(&OTHER_COUNTER_SECRET)?.device_id();
                    Some(variant(
                        node.watcher_mut(),
                        other,
                        &identity(&OWNER_SECRET)?,
                    )?)
                }
                other => return Err(format!("unknown kind of heartbeat: {other}")),
            };
            match made {
                Some(bytes) => say(format!("heartbeat {}", hex(&bytes))),
                None => say("heartbeat none it could not be made (see the notes)"),
            }
            show_notes(node);
        }
        Some("pump") => {
            let secs: f64 = words
                .next()
                .ok_or("the seconds are missing")?
                .parse()
                .map_err(|_| "the seconds are not a number".to_string())?;
            node.pump(Duration::from_secs_f64(secs.clamp(0.0, 600.0)));
            show_notes(node);
        }
        Some("health") => {
            let seen = seen.lock().map_err(|_| "lock".to_string())?;
            let used = match node.watcher().health() {
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
            let tip = node.watcher().chain().tip_height();
            let hash = node
                .watcher()
                .chain()
                .hash_at(tip)
                .map_or("none".to_string(), |h| hex(&h.to_byte_array()));
            say(format!(
                "status tip={tip} hash={hash} seq={}",
                node.watcher().last_seq()
            ));
        }
        Some("sendbatch") => {
            let link = link
                .as_deref_mut()
                .ok_or("the link is not started: give --relay and --relay-key")?;
            let start: u32 = words
                .next()
                .and_then(|w| w.parse().ok())
                .ok_or("sendbatch needs the first index")?;
            let batch = BatchBuilder::new(BatchPolicy {
                min_count: ADDRESSES,
            })
            .derive(wallet, counter, start, ADDRESSES)
            .map_err(|e| e.to_string())?;
            let signed = SignedBatch {
                owner_sig: batch.sign(&identity(&OWNER_SECRET)?),
                approver_sig: batch.sign(&identity(&APPROVER_SECRET)?),
                batch,
            };
            match link.send_batch(&counter, &signed, now_secs()) {
                Ok(()) => say(format!("sendbatch start={start} sent")),
                Err(e) => say(format!("sendbatch start={start} not-sent {e}")),
            }
        }
        Some("batches") => {
            let link = link
                .as_deref()
                .ok_or("the link is not started: give --relay and --relay-key")?;
            let waiting = link.batches_waiting(&counter);
            say(format!(
                "batches counter={} waiting={} since={}",
                hex(&counter.to_bytes()),
                waiting.as_ref().map_or(0, |w| w.batches),
                waiting.map_or("none".to_string(), |w| w.since.to_string()),
            ));
        }
        Some("linkstate") => {
            let link = link
                .as_deref()
                .ok_or("the link is not started: give --relay and --relay-key")?;
            say(format!(
                "linkstate alerts={} refused={} reachable={}",
                link.alerts().len(),
                link.courier().refused(),
                link.courier().reachable()
            ));
        }
        Some("copies") => {
            let link = link
                .as_deref()
                .ok_or("the link is not started: give --relay and --relay-key")?;
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
        Some("refunds") => {
            for r in node.owner().refunds() {
                let address = Address::from_script(&r.script, Network::Regtest)
                    .map_or_else(|_| hex(r.script.as_bytes()), |a| a.to_string());
                say(format!(
                    "refund sale={} counter={} sats={} address={address} cashier={} stage={:?}",
                    r.sale.index,
                    hex(&r.sale.counter.to_bytes()),
                    r.sats,
                    r.cashier,
                    r.stage
                ));
            }
            for n in intake.notes() {
                say(format!(
                    "refundnote seq={} number={} sale={} kind={:?} text={}",
                    n.seq, n.number, n.sale, n.kind, n.text
                ));
            }
            say(format!(
                "refunds queued={} notes={}",
                node.owner().refunds().len(),
                intake.notes().len()
            ));
        }
        Some("facts") => {
            let index: u32 = words
                .next()
                .ok_or("the sale is missing")?
                .parse()
                .map_err(|_| "the sale is not a number".to_string())?;
            let key = SaleKey { counter, index };
            let held = node.owner().sale_facts(&key);
            let tip = node.watcher().chain().tip_height();
            let view = node.view();
            match verified_facts(view, node.owner(), key, tip, node.refund_depth()) {
                Ok(seen) => say(format!(
                    "facts sale={index} held_received={} held_deep={} view_received={} \
                     view_deep={} receipts={}",
                    held.map_or("none".to_string(), |f| f.received_sats.to_string()),
                    held.map_or("none".to_string(), |f| f.reached_refund_depth.to_string()),
                    seen.received_sats,
                    seen.reached_refund_depth,
                    view.receipts_at(index).len()
                )),
                Err(why) => say(format!("facts sale={index} refused {why}")),
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
    let mut batch_resend: Option<u64> = None;
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
        } else if arg == "--batch-resend" {
            let secs = args.next().ok_or(USAGE)?;
            batch_resend = Some(secs.parse().map_err(|_| format!("seconds: {secs}"))?);
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
    let wallet = test_wallet()?;
    let mut parts = OwnerParts::new(
        OWNER_SECRET,
        wallet.clone(),
        identity(&APPROVER_SECRET)?.device_id(),
        BatchPolicy {
            min_count: ADDRESSES,
        },
    )
    .map_err(|e| e.to_string())?;
    parts.options = Options::default();
    parts.refund_depth = REFUND_DEPTH;
    let mut node = OwnerNode::start(config, parts).map_err(|e| e.to_string())?;
    issue_addresses(&mut node, &wallet)?;
    say(format!(
        "started tip={} owner={}",
        node.watcher().chain().tip_height(),
        hex(&node.watcher().device_id().to_bytes())
    ));
    let mut link = match (relay, relay_key) {
        (Some(addr), Some(key)) => Some(start_link(dir, addr, key, heartbeat_every, batch_resend)?),
        _ => None,
    };
    let mut alerts_shown = 0usize;
    let mut intake = RefundIntake::open(Path::new(dir)).map_err(|e| e.to_string())?;

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
                match command(
                    &mut node,
                    link.as_mut(),
                    &intake,
                    &wallet,
                    line.trim(),
                    &seen,
                ) {
                    Ok(true) => {}
                    Ok(false) => break,
                    Err(e) => say(format!("error {e}")),
                }
                done += 1;
                say(format!("done {done}"));
            }
            Err(TryRecvError::Empty) => {
                node.pump(Duration::from_millis(100));
                show_notes(&mut node);
                if let Some(link) = link.as_mut() {
                    link.step(node.watcher_mut(), now_secs());
                    // OC-14, OC-15: the facts come from the node's own view.
                    match node.run_refund_intake(&mut intake, link) {
                        Ok(done) => {
                            for (counter, applied) in done {
                                // A request that only waits for the view is
                                // not news on every turn.
                                let news = openbmt_owner_node::Applied {
                                    waiting: None,
                                    ..applied.clone()
                                };
                                if news != openbmt_owner_node::Applied::default() {
                                    say(format!(
                                        "note refunds counter={} {applied:?}",
                                        hex(&counter.to_bytes())
                                    ));
                                }
                            }
                        }
                        Err(e) => say(format!("note refunds not applied: {e}")),
                    }
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
    node.shutdown();
    show_notes(&mut node);
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
