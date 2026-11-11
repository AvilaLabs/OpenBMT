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
//! them, except onion names, which are printed as `<onion>`; keep the output
//! out of the repository.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::process::ExitCode;
use std::sync::mpsc::RecvTimeoutError;
use std::time::{Duration, Instant};

use bitcoin::hashes::{sha256, Hash};
use bitcoin::p2p::address::AddrV2;
use bitcoin::{BlockHash, Network, ScriptBuf, WScriptHash};
use openbmt_net::AddressBook;
use openbmt_net_io::{
    AppEvent, DirLock, Driver, DriverConfig, FileStorage, OsRandomness, SystemClock,
};
use openbmt_watch::{network_group, HeaderStore, NetGroup, PeerAddr, PeerTransport};

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

/// A line of text with each word that holds an onion name printed as
/// `<onion>`: the notes name peers, and an onion address is not kept in a
/// log.
fn redact(text: &str) -> String {
    text.split(' ')
        .map(|w| if w.contains(".onion") { "<onion>" } else { w })
        .collect::<Vec<_>>()
        .join(" ")
}

/// How many Tor v3 addresses the stored book holds, and how many of them
/// were drawn (tried) and had a handshake finish at some time.
fn onion_in_book(path: &std::path::Path) -> (usize, usize, usize) {
    let Ok(bytes) = fs::read(path) else {
        return (0, 0, 0);
    };
    let Ok(book) = AddressBook::from_bytes(&bytes, 0) else {
        return (0, 0, 0);
    };
    let tor: Vec<_> = book
        .iter()
        .filter(|e| matches!(e.addr, AddrV2::TorV3(_)))
        .collect();
    (
        tor.len(),
        tor.iter().filter(|e| e.last_try.is_some()).count(),
        tor.iter().filter(|e| e.last_good.is_some()).count(),
    )
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
    let mut config = DriverConfig::new(network, start_tip);
    // With OPENBMT_PROXY set to the address of a SOCKS5 proxy (Tor's),
    // every peer is reached through it and no name is resolved here
    // (NR-26, NR-78).
    if let Ok(proxy) = std::env::var("OPENBMT_PROXY") {
        config.proxy = Some(proxy.parse().map_err(|e| format!("OPENBMT_PROXY: {e}"))?);
    }
    let onion_start = onion_in_book(&book_path);
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
    let mut blocks: Vec<(f64, u32, BlockHash, usize)> = Vec::new();
    let mut heights: BTreeMap<BlockHash, u32> = BTreeMap::new();
    let mut joins: Vec<(f64, u32, u32, usize)> = Vec::new();
    let mut tip_height = start_tip;
    let mut max_ready = 0usize;
    let mut first_ready: Option<f64> = None;
    let mut time_max_ready: Option<f64> = None;
    let mut not_behind: Option<f64> = None;
    let mut two_filter_groups: Option<f64> = None;
    let mut first_filter: Option<f64> = None;
    // With a proxy: lookups of the seeds' filter names, from the round
    // notes (answered, asked).
    let mut lookups = (0u64, 0u64);
    let mut last_peers = String::new();
    // NR-81, NR-82: the distinct peers that were ready on each transport,
    // the refusals the driver noted, and the drops by rule. Only counts
    // are printed, never addresses.
    let mut seen_v2: BTreeSet<String> = BTreeSet::new();
    let mut seen_v1: BTreeSet<String> = BTreeSet::new();
    let mut v2_refusals = 0u64;
    let mut drops: BTreeMap<String, u64> = BTreeMap::new();
    // NR-92 to NR-96: onion peers. Distinct keys that were ready, their
    // groups and the ones that serve filters (never printed), the most at
    // once, the dials the driver noted and why they failed.
    let mut onion_ready: BTreeSet<[u8; 32]> = BTreeSet::new();
    let mut onion_filter_peers: BTreeSet<[u8; 32]> = BTreeSet::new();
    let mut onion_groups: BTreeSet<NetGroup> = BTreeSet::new();
    let mut max_onion_ready = 0usize;
    let mut first_onion_ready: Option<f64> = None;
    let mut onion_dials = 0u64;
    let mut onion_failures: BTreeMap<String, u64> = BTreeMap::new();

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
                let mut onion_now = 0usize;
                for p in &peers {
                    if let PeerAddr::Onion(key) = p.addr {
                        onion_now += 1;
                        onion_ready.insert(key);
                        onion_groups.insert(network_group(&p.addr));
                        if p.serves_filters {
                            onion_filter_peers.insert(key);
                        }
                    }
                    match p.transport {
                        PeerTransport::V2 => seen_v2.insert(format!("{:?}", p.addr)),
                        PeerTransport::V1 => seen_v1.insert(format!("{:?}", p.addr)),
                    };
                }
                if onion_now > 0 && first_onion_ready.is_none() {
                    first_onion_ready = Some(t);
                }
                max_onion_ready = max_onion_ready.max(onion_now);
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
                if filters > 0 && first_filter.is_none() {
                    first_filter = Some(t);
                }
                if filter_groups.len() >= 2 && two_filter_groups.is_none() {
                    two_filter_groups = Some(t);
                }
                let line = format!(
                    "peers ready={ready} onion={onion_now} filters={filters} groups={} \
                     filter_groups={} ahead_since={ahead:?} tip={height}",
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
            }
            AppEvent::AlsoAnnounced { .. } => {
                *kinds.entry("also_announced").or_default() += 1;
            }
            AppEvent::Note(text) => {
                *kinds.entry("note").or_default() += 1;
                if text.contains("does not speak the encrypted transport") {
                    v2_refusals += 1;
                }
                // "seed round through the proxy: A of B lookups ..."
                if let Some(rest) = text.strip_prefix("seed round through the proxy: ") {
                    let mut words = rest.split_whitespace();
                    let answered = words.next().and_then(|w| w.parse::<u64>().ok());
                    let asked = words.nth(1).and_then(|w| w.parse::<u64>().ok());
                    if let (Some(a), Some(b)) = (answered, asked) {
                        lookups = (lookups.0 + a, lookups.1 + b);
                    }
                }
                if text == "dialing an onion peer" {
                    onion_dials += 1;
                }
                if let Some(why) = text.strip_prefix("onion dial failed: ") {
                    *onion_failures.entry(redact(why)).or_default() += 1;
                }
                if text.contains(" dropped") {
                    if let Some(at) = text.find("NR-") {
                        let rule: String = text.chars().skip(at).take(5).collect();
                        *drops.entry(rule).or_default() += 1;
                    }
                }
                println!("{t:.1} note {}", redact(&text));
            }
            AppEvent::TxRelayed { .. } | AppEvent::TxState { .. } | AppEvent::FeeBlock { .. } => {}
            AppEvent::Checked(height) => {
                *kinds.entry("checked").or_default() += 1;
                println!("{t:.1} checked up to {height}");
            }
            AppEvent::RescanStarted(height) => {
                println!("{t:.1} rescan started from {height}");
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
                println!("{t:.1} note {}", redact(&text));
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
    // A transaction is handed on as a sighting once (NR-53); later
    // announcements are `AlsoAnnounced`. The example keeps no set of ids, so
    // that a long run measures the driver's memory and not its own.
    println!("sightings: {sightings}");
    println!(
        "peers ready on v2: {} distinct, on v1: {} distinct",
        seen_v2.len(),
        seen_v1.len()
    );
    println!("v2 refusals noted (fallback to v1): {v2_refusals}");
    let onion_end = onion_in_book(&book_path);
    println!(
        "onion addresses in the book: {} at start, {} at the end \
         ({} drawn, {} with a finished handshake in the stored book)",
        onion_start.0, onion_end.0, onion_end.1, onion_end.2
    );
    println!(
        "onion dials started: {onion_dials}; failed: {}",
        onion_failures.values().sum::<u64>()
    );
    for (why, n) in &onion_failures {
        println!("  onion dial failed ({n}): {why}");
    }
    println!(
        "onion peers ready: {} distinct in {} groups, {} serving filters, \
         most at once {max_onion_ready}, first at {}",
        onion_ready.len(),
        onion_groups.len(),
        onion_filter_peers.len(),
        secs(first_onion_ready)
    );
    for (rule, n) in &drops {
        println!("drops by {rule}: {n}");
    }
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
    println!("first filter peer ready: {}", secs(first_filter));
    println!(
        "two filter peers in different groups: {}",
        secs(two_filter_groups)
    );
    println!(
        "seed lookups through the proxy: {} answered of {} finished in rounds",
        lookups.0, lookups.1
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
