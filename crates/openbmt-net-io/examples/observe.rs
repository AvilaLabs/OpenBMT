//! The driver against the real Bitcoin network, as an observer.
//!
//! ```text
//! cargo run -p openbmt-net-io --example observe -- \
//!     <bitcoin|signet> <data dir> <seconds>
//! ```
//!
//! This starts the driver with its defaults (the network's DNS seeds, 8
//! peers, no proxy, no local peers) and keeps the headers and the address
//! book in `<data dir>`. It watches one script that nobody pays, so filters
//! are asked for and compared and nothing matches. It reads events for
//! `<seconds>`, prints one line for each, then stops the driver and prints
//! totals.
//!
//! The first block it checks is the one above the tip it starts with: the
//! stored tip if the data dir has headers, else the network's start block.
//!
//! The driver never sends a transaction, an address or a script, so this
//! only reads. Peer addresses appear in `note` lines, as the driver writes
//! them; keep the output out of the repository.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::process::ExitCode;
use std::sync::mpsc::RecvTimeoutError;
use std::time::{Duration, Instant};

use bitcoin::hashes::{sha256, Hash};
use bitcoin::{BlockHash, Network, ScriptBuf, WScriptHash};
use openbmt_net_io::{
    AppEvent, DirLock, Driver, DriverConfig, FileStorage, OsRandomness, SystemClock,
};
use openbmt_watch::{network_group, HeaderStore, NetGroup};

const USAGE: &str = "usage: observe <bitcoin|signet> <data dir> <seconds> [headers-only]";

/// A line of `/proc/self/io`, or 0.
fn proc_io(field: &str) -> u64 {
    let Ok(text) = fs::read_to_string("/proc/self/io") else {
        return 0;
    };
    text.lines()
        .find_map(|l| l.strip_prefix(field)?.trim().parse().ok())
        .unwrap_or(0)
}

/// Bytes received and sent on every interface but loopback, from the whole
/// machine (not only this process).
fn net_dev() -> (u64, u64) {
    let Ok(text) = fs::read_to_string("/proc/net/dev") else {
        return (0, 0);
    };
    let (mut rx, mut tx) = (0u64, 0u64);
    for line in text.lines().skip(2) {
        let Some((name, rest)) = line.split_once(':') else {
            continue;
        };
        if name.trim() == "lo" {
            continue;
        }
        let f: Vec<u64> = rest
            .split_whitespace()
            .filter_map(|w| w.parse().ok())
            .collect();
        rx = rx.saturating_add(f.first().copied().unwrap_or(0));
        tx = tx.saturating_add(f.get(8).copied().unwrap_or(0));
    }
    (rx, tx)
}

fn size_of(path: &std::path::Path) -> String {
    match fs::metadata(path) {
        Ok(m) => format!("{} bytes", m.len()),
        Err(_) => "missing".to_string(),
    }
}

fn secs(t: Option<f64>) -> String {
    t.map_or("never".to_string(), |t| format!("{t:.1}s"))
}

fn run(args: &[String]) -> Result<(), String> {
    // A fourth argument, `headers-only`, watches no script: the headers
    // are fetched and stored and no block is checked. Run it once on a
    // fresh data dir before the real run, so that the real run starts at
    // the network's tip and not at the start block.
    let (network, dir, seconds, headers_only) = match args {
        [n, d, s] => (n, d, s, false),
        [n, d, s, mode] if mode == "headers-only" => (n, d, s, true),
        _ => return Err(USAGE.to_string()),
    };
    let network = match network.as_str() {
        "bitcoin" => Network::Bitcoin,
        "signet" => Network::Signet,
        other => return Err(format!("network: {other}")),
    };
    let seconds: u64 = seconds.parse().map_err(|_| format!("seconds: {seconds}"))?;
    // NR-79: this process owns the directory until it ends.
    let _lock = DirLock::acquire(dir).map_err(|e| e.to_string())?;
    let storage = FileStorage::new(dir).map_err(|e| format!("data dir: {e}"))?;
    let headers_path = storage.path(openbmt_net_io::Key::Headers);
    let book_path = storage.path(openbmt_net_io::Key::Book);

    // The tip the driver will start from: what the stored headers hold, or
    // the start block. Read the same way the driver reads them.
    let start_tip = match fs::read(&headers_path) {
        Ok(bytes) => HeaderStore::from_bytes(network, &bytes).map_err(|e| e.to_string())?,
        Err(_) => HeaderStore::new(network).map_err(|e| e.to_string())?,
    }
    .tip();
    let (start_tip, start_hash) = start_tip;

    // A script nobody pays: P2WSH of the SHA-256 of a fixed phrase.
    let digest = sha256::Hash::hash(b"openbmt observe example");
    let script = ScriptBuf::new_p2wsh(&WScriptHash::from_raw_hash(digest));

    let random = OsRandomness::new().map_err(|e| format!("randomness: {e}"))?;
    let config = DriverConfig::new(network, start_tip);
    let started = Instant::now();
    let (rx0, tx0) = net_dev();
    let io0 = proc_io("rchar:");
    let scripts = if headers_only {
        Vec::new()
    } else {
        vec![script]
    };
    let mut handle =
        Driver::spawn(config, storage, SystemClock, random, scripts).map_err(|e| e.to_string())?;
    println!("0.0 start network={network} start_tip={start_tip} {start_hash} seconds={seconds}");

    let end = started + Duration::from_secs(seconds);
    let mut kinds: BTreeMap<&str, u64> = BTreeMap::new();
    let mut sightings = 0u64;
    let mut sighting_ids: BTreeSet<String> = BTreeSet::new();
    let mut blocks: Vec<(f64, u32, BlockHash, usize)> = Vec::new();
    let mut heights: BTreeMap<BlockHash, u32> = BTreeMap::new();
    let mut joins: Vec<(f64, u32, u32, usize)> = Vec::new();
    let mut tip_height = start_tip;
    let mut max_ready = 0usize;
    let mut first_ready: Option<f64> = None;
    let mut time_max_ready: Option<f64> = None;
    let mut not_behind: Option<f64> = None;
    let mut two_filter_groups: Option<f64> = None;
    let mut last_peers = String::new();

    while Instant::now() < end {
        let wait = end.saturating_duration_since(Instant::now());
        let event = match handle
            .events()
            .recv_timeout(wait.min(Duration::from_millis(500)))
        {
            Ok(event) => event,
            Err(RecvTimeoutError::Timeout) => continue,
            Err(RecvTimeoutError::Disconnected) => break,
        };
        let t = started.elapsed().as_secs_f64();
        match event {
            AppEvent::Peers(peers, ahead, (height, _)) => {
                *kinds.entry("peers").or_default() += 1;
                tip_height = height;
                let ready = peers.len();
                let filters = peers.iter().filter(|p| p.serves_filters).count();
                let groups: BTreeSet<NetGroup> =
                    peers.iter().map(|p| network_group(&p.addr)).collect();
                let filter_groups: BTreeSet<NetGroup> = peers
                    .iter()
                    .filter(|p| p.serves_filters)
                    .map(|p| network_group(&p.addr))
                    .collect();
                if ready > 0 && first_ready.is_none() {
                    first_ready = Some(t);
                }
                if ready > max_ready {
                    max_ready = ready;
                    time_max_ready = Some(t);
                }
                if ready > 0 && ahead.is_none() && not_behind.is_none() {
                    not_behind = Some(t);
                }
                if filter_groups.len() >= 2 && two_filter_groups.is_none() {
                    two_filter_groups = Some(t);
                }
                let line = format!(
                    "peers ready={ready} filters={filters} groups={} filter_groups={} \
                     ahead_since={ahead:?} tip={height}",
                    groups.len(),
                    filter_groups.len()
                );
                // Print every change, and a repeat only when it is not
                // the same as the line before.
                if line != last_peers {
                    println!("{t:.1} {line}");
                    last_peers = line;
                }
            }
            AppEvent::Chain { joined, left } => {
                *kinds.entry("chain").or_default() += 1;
                let top = joined.last().map_or(0, |(h, _)| *h);
                let low = joined.first().map_or(0, |(h, _)| *h);
                for (h, hash) in &joined {
                    heights.insert(*hash, *h);
                }
                joins.push((t, low, top, joined.len()));
                println!(
                    "{t:.1} chain joined={} top={top} left={}",
                    joined.len(),
                    left.len()
                );
            }
            AppEvent::Block { hash, bytes } => {
                *kinds.entry("block").or_default() += 1;
                let h = heights.get(&hash).copied().unwrap_or(0);
                blocks.push((t, h, hash, bytes.len()));
                println!("{t:.1} block {hash} {} bytes", bytes.len());
            }
            AppEvent::Sighting { tx, from } => {
                *kinds.entry("sighting").or_default() += 1;
                sightings += 1;
                // Only the id: the transaction itself is not printed.
                let id = match bitcoin::consensus::deserialize::<bitcoin::Transaction>(&tx) {
                    Ok(t) => t.compute_txid().to_string(),
                    Err(e) => format!("unreadable({e})"),
                };
                println!("{t:.1} sighting {id} from={}", from.len());
                sighting_ids.insert(id);
            }
            AppEvent::AlsoAnnounced { .. } => {
                *kinds.entry("also_announced").or_default() += 1;
            }
            AppEvent::Note(text) => {
                *kinds.entry("note").or_default() += 1;
                println!("{t:.1} note {text}");
            }
            AppEvent::Checked(height) => {
                *kinds.entry("checked").or_default() += 1;
                println!("{t:.1} checked up to {height}");
            }
            AppEvent::Stopped => {
                *kinds.entry("stopped").or_default() += 1;
                println!("{t:.1} stopped");
                break;
            }
        }
    }

    let before_stop = started.elapsed().as_secs_f64();
    handle.shutdown();
    let stop_took = started.elapsed().as_secs_f64() - before_stop;
    // Events that arrived while stopping (the final tip may have moved).
    while let Ok(event) = handle.events().try_recv() {
        let t = started.elapsed().as_secs_f64();
        match event {
            AppEvent::Peers(_, _, (height, _)) => tip_height = height,
            AppEvent::Note(text) => {
                *kinds.entry("note").or_default() += 1;
                println!("{t:.1} note {text}");
            }
            AppEvent::Stopped => {
                *kinds.entry("stopped").or_default() += 1;
                println!("{t:.1} stopped");
            }
            _ => {}
        }
    }
    let (rx1, tx1) = net_dev();
    let io1 = proc_io("rchar:");

    println!("--- totals");
    for (kind, n) in &kinds {
        println!("events {kind}: {n}");
    }
    println!(
        "sightings: {sightings} ({} distinct txids)",
        sighting_ids.len()
    );
    println!("blocks fetched in full: {}", blocks.len());
    for (t, h, hash, size) in &blocks {
        println!("  at {t:.1}s height {h} {hash} {size} bytes");
    }
    for (t, low, top, n) in &joins {
        println!("  chain at {t:.1}s: {n} joined, {low}..{top}");
    }
    println!("start tip: {start_tip}");
    println!("final tip height: {tip_height}");
    // The driver saves the headers when it stops, so the file holds the
    // final tip.
    match fs::read(&headers_path)
        .map_err(|e| e.to_string())
        .and_then(|b| HeaderStore::from_bytes(network, &b).map_err(|e| e.to_string()))
    {
        Ok(store) => {
            let (h, hash) = store.tip();
            println!("final tip in the saved headers: {h} {hash}");
        }
        Err(e) => println!("final tip in the saved headers: unreadable ({e})"),
    }
    println!("first ready peer: {}", secs(first_ready));
    println!(
        "most ready peers: {max_ready}, first reached at {}",
        secs(time_max_ready)
    );
    println!(
        "first moment no peer ahead (with a ready peer): {}",
        secs(not_behind)
    );
    println!(
        "two filter peers in different groups: {}",
        secs(two_filter_groups)
    );
    println!("shutdown took: {stop_took:.1}s");
    println!("headers file: {}", size_of(&headers_path));
    println!("book file: {}", size_of(&book_path));
    println!(
        "process rchar delta: {} bytes (all reads, files and sockets)",
        io1.saturating_sub(io0)
    );
    println!(
        "machine-wide non-loopback bytes in the same period: rx {} tx {}",
        rx1.saturating_sub(rx0),
        tx1.saturating_sub(tx0)
    );
    Ok(())
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(why) => {
            println!("failed {why}");
            ExitCode::from(1)
        }
    }
}
