//! The driver against a Bitcoin node on regtest, as a command that prints
//! what the app would see.
//!
//! ```text
//! cargo run -p openbmt-net-io --example regtest_node -- \
//!     <port> <data dir> <watch from> <script hex>
//! ```
//!
//! `scripts/netio-interop-core.sh` uses this to check the driver against
//! Bitcoin Core. It starts the driver with one fixed peer at
//! `127.0.0.1:<port>` and one watched script, keeps the headers and the
//! address book in `<data dir>`, and prints one line for each thing the app
//! is told:
//!
//! - `peers <ready> filters=<n> best=<height> tip=<height> ahead=<yes|no>
//!   v2=<n>` (`v2` counts the ready peers on the encrypted transport)
//! - `chain joined=<n> top=<height> left=<n>`
//! - `block <hash> <bytes> pays <txid>:<sats> ...`
//! - `sighting <txid> from=<n> pays <sats>`
//! - `also <txid>`
//! - `note <text>`
//! - `stopped`
//!
//! A line of `quit` on the standard input (or its end) stops the driver
//! cleanly: it saves and then prints `stopped`.
//!
//! This is a test tool, built for regtest alone. The peer is on this
//! machine, which the counter does not allow outside a check like this.

use std::io::{BufRead, Write};
use std::net::SocketAddr;
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::RecvTimeoutError;
use std::sync::Arc;
use std::time::Duration;

use bitcoin::consensus::deserialize;
use bitcoin::{Block, Network, ScriptBuf, Transaction};
use openbmt_net_io::{
    AppEvent, DirLock, Driver, DriverConfig, FileStorage, OsRandomness, SystemClock,
};
use openbmt_watch::PeerTransport;

const USAGE: &str = "usage: regtest_node <port> <data dir> <watch from> <script hex>";

fn paid(tx: &Transaction, script: &ScriptBuf) -> u64 {
    tx.output
        .iter()
        .filter(|o| o.script_pubkey == *script)
        .map(|o| o.value.to_sat())
        .sum()
}

/// One line for an event, or `None` for the end.
fn describe(event: &AppEvent, script: &ScriptBuf) -> Option<String> {
    Some(match event {
        AppEvent::Peers(peers, ahead, (height, _)) => {
            let filters = peers.iter().filter(|p| p.serves_filters).count();
            let best = peers.iter().map(|p| p.best_height).max().unwrap_or(0);
            let v2 = peers
                .iter()
                .filter(|p| p.transport == PeerTransport::V2)
                .count();
            format!(
                "peers {} filters={filters} best={best} tip={height} ahead={} v2={v2}",
                peers.len(),
                if ahead.is_some() { "yes" } else { "no" }
            )
        }
        AppEvent::Chain { joined, left } => {
            let top = joined.last().map_or(0, |(h, _)| *h);
            format!(
                "chain joined={} top={top} left={}",
                joined.len(),
                left.len()
            )
        }
        AppEvent::Block { hash, bytes } => {
            let mut line = format!("block {hash} {}", bytes.len());
            match deserialize::<Block>(bytes) {
                Ok(block) => {
                    line.push_str(" pays");
                    for tx in &block.txdata {
                        let sats = paid(tx, script);
                        if sats > 0 {
                            line.push_str(&format!(" {}:{sats}", tx.compute_txid()));
                        }
                    }
                }
                Err(e) => line.push_str(&format!(" unreadable: {e}")),
            }
            line
        }
        AppEvent::Sighting { tx, from } => match deserialize::<Transaction>(tx) {
            Ok(t) => format!(
                "sighting {} from={} pays {}",
                t.compute_txid(),
                from.len(),
                paid(&t, script)
            ),
            Err(e) => format!("sighting unreadable: {e}"),
        },
        AppEvent::AlsoAnnounced { txid, .. } => format!("also {txid}"),
        AppEvent::Note(text) => format!("note {text}"),
        AppEvent::Checked(height) => format!("checked {height}"),
        AppEvent::Stopped => return None,
    })
}

fn run(args: &[String]) -> Result<(), String> {
    let [port, dir, watch_from, script] = args else {
        return Err(USAGE.to_string());
    };
    let port: u16 = port.parse().map_err(|_| format!("port: {port}"))?;
    let watch_from: u32 = watch_from
        .parse()
        .map_err(|_| format!("watch from: {watch_from}"))?;
    let script = ScriptBuf::from_hex(script).map_err(|e| format!("script: {e}"))?;
    // NR-79: this process owns the directory until it ends.
    let _lock = DirLock::acquire(dir).map_err(|e| e.to_string())?;
    let storage = FileStorage::new(dir).map_err(|e| format!("data dir: {e}"))?;
    let random = OsRandomness::new().map_err(|e| format!("randomness: {e}"))?;
    let mut config = DriverConfig::new(Network::Regtest, watch_from);
    config.allow_local = true;
    config.fixed_peers = vec![SocketAddr::from(([127, 0, 0, 1], port))];
    let mut handle = Driver::spawn(config, storage, SystemClock, random, vec![script.clone()])
        .map_err(|e| e.to_string())?;

    let quit = Arc::new(AtomicBool::new(false));
    let flag = quit.clone();
    std::thread::spawn(move || {
        for line in std::io::stdin().lock().lines() {
            match line {
                Ok(l) if l.trim() != "quit" => {}
                _ => break,
            }
        }
        flag.store(true, Ordering::SeqCst);
    });

    let mut last = String::new();
    let mut print = |line: String| {
        // The same `peers` line twice in a row says nothing new.
        if line != last || !line.starts_with("peers") {
            println!("{line}");
            let _ = std::io::stdout().flush();
            last = line;
        }
    };
    let mut stopping = false;
    loop {
        if quit.load(Ordering::SeqCst) && !stopping {
            stopping = true;
            handle.shutdown();
        }
        match handle.events().recv_timeout(Duration::from_millis(100)) {
            Ok(event) => match describe(&event, &script) {
                Some(line) => print(line),
                None => {
                    println!("stopped");
                    return Ok(());
                }
            },
            Err(RecvTimeoutError::Timeout) if stopping => {
                println!("stopped");
                return Ok(());
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return Ok(()),
        }
    }
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
