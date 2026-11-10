//! The counter node against a Bitcoin node on regtest, as a command that
//! reads simple lines on its standard input and prints one line for each
//! thing it does.
//!
//! ```text
//! counter_regtest <data dir> <node address> [--regtest-single-peer]
//! ```
//!
//! `scripts/counter-e2e-core.sh` uses this to check the whole receive path
//! against Bitcoin Core. The node is the one fixed local peer of the driver.
//!
//! **Test only, never use.** The three identities and the three wallet keys
//! are made from fixed bytes that anyone can read here. The money they would
//! guard is worth nothing, and must stay so.
//!
//! On the first run in a data directory the example makes a 2-of-3 wallet
//! description from three fixed test keys, and a signed batch of 20
//! addresses derived from it by the core crate. Later runs read both back
//! from the directory.
//!
//! `--regtest-single-peer` replaces the answer of the health rules with
//! "healthy". The rules ask for peers in three network groups and two filter
//! peers (WR-50), which one local node cannot be. The switch is here and not
//! in the library, it is printed loudly at the start, and what the real rules
//! say for the same peers is printed by the `health` command. A run with the
//! switch is not a check of the health rules.
//!
//! Commands, one per line:
//!
//! - `open <minor units> <rate in minor units per bitcoin>`
//! - `judge <index>`
//! - `pump <seconds>`
//! - `health`
//! - `status`
//! - `quit`
//!
//! Between commands the node keeps taking the driver's events. Every command
//! ends with a line `done <n>`, where `n` counts the commands.
//!
//! Lines printed:
//!
//! - `descriptor <wallet description>`
//! - `warning ...` for the single-peer switch
//! - `opened index=<i> address=<address> sats=<n>` or `refused <text>`
//! - `sale <i> state=<State> received=<sats> needs=<sats> mode=<mode> why=<text>`
//! - `report <Kind> index=<i> ...`, once for each report
//! - `health used=<healthy|unhealthy> real=<ok|problem> peers=<n>`
//! - `status tip=<h> checked=<h> peers=<n>`
//! - `note <text>`
//! - `stopped`

use std::io::{BufRead, Write};
use std::net::SocketAddr;
use std::process::ExitCode;
use std::str::FromStr;
use std::sync::mpsc::{channel, TryRecvError};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use bitcoin::bip32::{DerivationPath, Xpriv, Xpub};
use bitcoin::secp256k1::Secp256k1;
use bitcoin::{Address, Network};
use openbmt_core::batch::{BatchBuilder, BatchPolicy, PinnedRoots, SignedBatch};
use openbmt_core::identity::Identity;
use openbmt_core::settings::Settings;
use openbmt_core::wallet::{KeySlot, WalletDescription};
use openbmt_counter::{Amount, Currency, Pairing, Rate, Report};
use openbmt_counter_node::{Node, NodeConfig};
use openbmt_net_io::DriverConfig;
use openbmt_watch::{check_health, HeaderChain, Health, HealthInput};

const USAGE: &str = "usage: counter_regtest <data dir> <node address> [--regtest-single-peer]";
const ADDRESSES: u32 = 20;

/// TEST ONLY, NEVER USE: the secret bytes of the three identities.
const OWNER_SECRET: [u8; 32] = [0x11; 32];
const APPROVER_SECRET: [u8; 32] = [0x12; 32];
const COUNTER_SECRET: [u8; 32] = [0x13; 32];
/// TEST ONLY, NEVER USE: the seeds of the three wallet keys.
const KEY_SEEDS: [[u8; 32]; 3] = [[0xa1; 32], [0xa2; 32], [0xa3; 32]];

/// What the health function saw last, for the `health` and `status` lines.
#[derive(Debug, Default)]
struct Seen {
    real: Option<Health>,
    peers: usize,
}

fn identity(secret: &[u8; 32]) -> Result<Identity, String> {
    Identity::from_secret_bytes(secret).map_err(|e| format!("identity: {e}"))
}

/// One of the three test keys, on the wallet's account path for regtest.
fn test_slot(index: u8, seed: &[u8; 32]) -> Result<KeySlot, String> {
    let secp = Secp256k1::new();
    let master = Xpriv::new_master(Network::Regtest, seed).map_err(|e| e.to_string())?;
    let path = DerivationPath::from_str("m/48h/1h/0h/2h").map_err(|e| e.to_string())?;
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

struct Setup {
    wallet: WalletDescription,
    config: NodeConfig,
}

fn setup(
    dir: &str,
    peer: SocketAddr,
    single_peer: bool,
    seen: &Arc<Mutex<Seen>>,
) -> Result<Setup, String> {
    let owner = identity(&OWNER_SECRET)?;
    let approver = identity(&APPROVER_SECRET)?;
    let counter = identity(&COUNTER_SECRET)?;

    let slots = [
        test_slot(1, &KEY_SEEDS[0])?,
        test_slot(2, &KEY_SEEDS[1])?,
        test_slot(3, &KEY_SEEDS[2])?,
    ];
    let wallet = WalletDescription::new(Network::Regtest, slots).map_err(|e| e.to_string())?;
    let policy = BatchPolicy {
        min_count: ADDRESSES,
    };
    let pairing = Pairing {
        roots: PinnedRoots::new(owner.device_id(), approver.device_id())
            .map_err(|e| e.to_string())?,
        wallet_checksum: wallet.checksum32(),
        batch_policy: policy,
    };

    // The default settings, with this counter given the whole of every slice.
    let mut settings = Settings::defaults(1_000);
    settings.devices.insert(counter.device_id());
    settings.counters.insert(counter.device_id(), 10_000);

    let batch = BatchBuilder::new(policy)
        .derive(&wallet, counter.device_id(), 0, ADDRESSES)
        .map_err(|e| e.to_string())?;
    let first_batch = SignedBatch {
        owner_sig: batch.sign(&owner),
        approver_sig: batch.sign(&approver),
        batch,
    };

    let mut driver = DriverConfig::new(Network::Regtest, 0);
    driver.allow_local = true;
    driver.fixed_peers = vec![peer];
    let mut config = NodeConfig::new(dir, counter, pairing, settings, driver);
    config.first_batch = Some(first_batch);
    let seen = seen.clone();
    config.health = Box::new(move |input: &HealthInput<'_>| {
        let real = check_health(input);
        if let Ok(mut s) = seen.lock() {
            s.real = Some(real);
            s.peers = input.peers.len();
        }
        if single_peer {
            // Regtest with one local node only. Not the rules.
            Health {
                healthy: true,
                problem: None,
                behind: real.behind,
            }
        } else {
            real
        }
    });
    Ok(Setup { wallet, config })
}

fn say(line: impl AsRef<str>) {
    println!("{}", line.as_ref());
    let _ = std::io::stdout().flush();
}

fn describe(report: &Report) -> String {
    match report {
        Report::PosPaid { index } => format!("report PosPaid index={index}"),
        Report::PosUnpaid { index } => format!("report PosUnpaid index={index}"),
        Report::BooksFinal {
            index,
            amount,
            currency,
            received_sats,
        } => format!(
            "report BooksFinal index={index} amount={} {currency} sats={received_sats}",
            amount.0
        ),
        Report::SurplusOwed { index, sats } => {
            format!("report SurplusOwed index={index} sats={sats}")
        }
        Report::LatePayment { index, sats } => {
            format!("report LatePayment index={index} sats={sats}")
        }
        Report::Alarm { index } => format!("report Alarm index={index}"),
        Report::Dust { index, sats } => format!("report Dust index={index} sats={sats}"),
    }
}

fn show(node: &mut Node, reports: &[Report]) {
    for report in reports {
        say(describe(report));
    }
    for note in node.take_notes() {
        say(format!("note {note}"));
    }
}

fn number<T: FromStr>(word: Option<&str>, what: &str) -> Result<T, String> {
    word.ok_or_else(|| format!("{what} is missing"))?
        .parse()
        .map_err(|_| format!("{what} is not a number"))
}

/// Run one command. `Ok(false)` is `quit`.
fn command(node: &mut Node, line: &str, seen: &Arc<Mutex<Seen>>) -> Result<bool, String> {
    let mut words = line.split_whitespace();
    match words.next() {
        Some("open") => {
            let minor: u64 = number(words.next(), "the amount")?;
            let rate: u64 = number(words.next(), "the rate")?;
            let rate = Rate::new(rate).ok_or("the rate is zero")?;
            let usd = Currency::new(*b"USD").ok_or("currency")?;
            match node.open(Amount(minor), usd, rate) {
                Ok(opened) => {
                    let address = Address::from_script(&opened.script, Network::Regtest)
                        .map_err(|e| e.to_string())?;
                    say(format!(
                        "opened index={} address={address} sats={}",
                        opened.index, opened.needs_sats
                    ));
                }
                Err(e) => say(format!("refused {e}")),
            }
        }
        Some("judge") => {
            let index: u32 = number(words.next(), "the index")?;
            match node.judge(index) {
                Ok(screen) => {
                    say(format!(
                        "sale {index} state={:?} received={} needs={} mode={:?} why={}",
                        screen.state,
                        screen.received_sats,
                        screen.needs_sats,
                        screen.check_mode,
                        screen.why_text()
                    ));
                    show(node, &screen.reports);
                }
                Err(e) => say(format!("error {e}")),
            }
        }
        Some("pump") => {
            let secs: f64 = number(words.next(), "the seconds")?;
            let reports = node.pump(Duration::from_secs_f64(secs.clamp(0.0, 600.0)));
            show(node, &reports);
        }
        Some("health") => {
            let seen = seen.lock().map_err(|_| "lock".to_string())?;
            let used = match node.health() {
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
        Some("status") => {
            let peers = seen.lock().map_err(|_| "lock".to_string())?.peers;
            say(format!(
                "status tip={} checked={} peers={peers}",
                node.chain().tip_height(),
                node.checked_height()
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
    let Setup { wallet, config } = setup(dir, peer, single_peer, &seen)?;

    say(format!("descriptor {}", wallet.canonical_string()));
    if single_peer {
        say("warning --regtest-single-peer: the health answer is replaced by \"healthy\"");
        say("warning one local node cannot meet WR-50 (3 groups, 2 filter peers):");
        say("warning THIS RUN IS NOT A CHECK OF THE HEALTH RULES");
    }
    let mut node = Node::start(config).map_err(|e| e.to_string())?;
    say(format!(
        "started checked={} tip={}",
        node.checked_height(),
        node.chain().tip_height()
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
                match command(&mut node, line.trim(), &seen) {
                    Ok(true) => {}
                    Ok(false) => break,
                    Err(e) => say(format!("error {e}")),
                }
                done += 1;
                say(format!("done {done}"));
            }
            Err(TryRecvError::Empty) => {
                let reports = node.pump(Duration::from_millis(100));
                show(&mut node, &reports);
            }
            Err(TryRecvError::Disconnected) => break,
        }
    }
    let reports = node.shutdown();
    show(&mut node, &reports);
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
