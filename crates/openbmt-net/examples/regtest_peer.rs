//! One peer session against a Bitcoin node on regtest, as a blocking TCP
//! driver.
//!
//! ```text
//! cargo run -p openbmt-net --example regtest_peer -- <port> <command> [args]
//! ```
//!
//! `scripts/net-interop-core.sh` uses this to check the session against
//! Bitcoin Core. Each command opens its own connection to
//! `127.0.0.1:<port>`, runs the handshake, feeds every header received to
//! the watcher's header store with the system clock, and then does one
//! thing:
//!
//! - `handshake`: print the peer's services and start height.
//! - `sync <tip hash>`: ask for headers until the store's tip is that hash.
//! - `announce <seconds>`: sync, then record what the peer sends unasked.
//! - `filter <height> <block hash> <script hex>`: ask for the block's basic
//!   filter and match the script against it.
//! - `block <block hash> <script hex>`: sync, ask for the block and list
//!   the payments to the script that the watcher's block checks prove.
//! - `tx <seconds> <script hex>`: wait for announced transactions, fetch
//!   them and stop at the first that pays the script.
//! - `silent <seconds>`: send nothing after the handshake and print what
//!   arrives.
//!
//! The transport is the counter's (NR-80, NR-81): the encrypted one first,
//! and after a peer that does not speak it the old one at once, which is
//! remembered in the file named by `REGTEST_MEMORY` (default: none, so each
//! run begins with the encrypted transport). `REGTEST_V2=off` uses the old
//! transport alone. After the handshake every command prints
//! `transport v1|v2`, `fallback yes|no` and `session_id <hex or none>`.
//!
//! The output is one fact per line, ending with exit code 0, or a line
//! `failed <reason>` and exit code 1, or a line `dropped <NR-xx: reason>`
//! and exit code 2 when the session ended the connection.
//!
//! This is a test tool, built for regtest alone. The nonce is taken from
//! the system time, which a real caller must not do (NR-11).

use std::fs::File;
use std::io::{ErrorKind, Read, Write};
use std::net::{Shutdown, TcpStream};
use std::process::ExitCode;
use std::str::FromStr;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use bitcoin::bip158::BlockFilter;
use bitcoin::consensus::deserialize;
use bitcoin::p2p::ServiceFlags;
use bitcoin::{BlockHash, Network, ScriptBuf, Transaction};
use openbmt_net::{Drop, Event, Request, Session, SessionConfig, V2Entropy};
use openbmt_watch::{payments_in_block, Accepted, HeaderStore};

const NETWORK: Network = Network::Regtest;
const USAGE: &str = "usage: regtest_peer <port> handshake | sync <tip hash> | announce <seconds> \
| filter <height> <block hash> <script hex> | block <block hash> <script hex> \
| tx <seconds> <script hex> | silent <seconds>";
/// Seconds to wait for one answer.
const ANSWER_SECS: u64 = 30;
/// Seconds a sync may take.
const SYNC_SECS: u64 = 60;

/// Why a command ended without finishing.
enum Exit {
    Failed(String),
    Dropped(Drop),
}

impl From<Drop> for Exit {
    fn from(d: Drop) -> Self {
        Exit::Dropped(d)
    }
}

fn failed<T>(why: impl Into<String>) -> Result<T, Exit> {
    Err(Exit::Failed(why.into()))
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Cuts a byte stream into messages to report their command names. Used
/// only to say what was sent and received.
#[derive(Default)]
struct Scanner {
    buf: Vec<u8>,
}

impl Scanner {
    /// Feed bytes; get back `(command, payload length)` of each complete
    /// message.
    fn push(&mut self, bytes: &[u8]) -> Vec<(String, usize)> {
        self.buf.extend_from_slice(bytes);
        let mut out = Vec::new();
        while self.buf.len() >= 24 {
            let len = u32::from_le_bytes([self.buf[16], self.buf[17], self.buf[18], self.buf[19]])
                as usize;
            if self.buf.len() < 24 + len {
                break;
            }
            let name: Vec<u8> = self.buf[4..16]
                .iter()
                .copied()
                .take_while(|b| *b != 0)
                .collect();
            out.push((String::from_utf8_lossy(&name).into_owned(), len));
            self.buf.drain(..24 + len);
        }
        out
    }
}

struct Peer {
    stream: TcpStream,
    session: Session,
    store: HeaderStore,
    received: Scanner,
    sent: Scanner,
    /// Commands received, in order, not yet taken.
    received_log: Vec<String>,
    /// Commands sent, in order.
    sent_log: Vec<String>,
    ready: bool,
    /// Set once the handshake's own messages have been written.
    counting: bool,
    /// Bytes sent after the handshake, not counting `pong`.
    sent_after_handshake: usize,
    accepted: usize,
    header_messages: usize,
    /// Whether the old transport is in use because the encrypted one was
    /// refused (NR-81).
    fallback: bool,
}

/// A key's worth of entropy from the system.
fn entropy() -> Result<V2Entropy, Exit> {
    let mut secret = [0u8; 32];
    let mut aux = [0u8; 32];
    let read = File::open("/dev/urandom").and_then(|mut f| {
        f.read_exact(&mut secret)?;
        f.read_exact(&mut aux)
    });
    if let Err(e) = read {
        return failed(format!("randomness: {e}"));
    }
    Ok(V2Entropy::new(secret, aux))
}

/// NR-81: ports whose peer did not speak the encrypted transport, as
/// `<port> <until>` lines in the file of `REGTEST_MEMORY`.
fn remembered(port: u16) -> bool {
    let Ok(path) = std::env::var("REGTEST_MEMORY") else {
        return false;
    };
    let text = std::fs::read_to_string(path).unwrap_or_default();
    text.lines().any(|line| {
        let mut parts = line.split_whitespace();
        let (Some(p), Some(until)) = (parts.next(), parts.next()) else {
            return false;
        };
        p == port.to_string() && until.parse::<u64>().is_ok_and(|u| u > now_secs())
    })
}

/// Remember the port for a day.
fn remember(port: u16) {
    if let Ok(path) = std::env::var("REGTEST_MEMORY") {
        let line = format!("{port} {}\n", now_secs() + 86_400);
        let mut text = std::fs::read_to_string(&path).unwrap_or_default();
        text.push_str(&line);
        let _ = std::fs::write(path, text);
    }
}

impl Peer {
    /// Connect and run the handshake: the encrypted transport first, and the
    /// old one when the peer does not speak it.
    fn open(port: u16) -> Result<(Self, ServiceFlags, u32), Exit> {
        let off = std::env::var("REGTEST_V2").is_ok_and(|v| v == "off");
        if !off && !remembered(port) {
            let mut peer = Self::connect(port, true)?;
            match peer.handshake() {
                Ok((services, height)) => return Ok((peer, services, height)),
                Err(Exit::Dropped(Drop::V2NotSpoken)) => remember(port),
                Err(other) => return Err(other),
            }
            let mut peer = Self::connect(port, false)?;
            peer.fallback = true;
            let (services, height) = peer.handshake()?;
            return Ok((peer, services, height));
        }
        let mut peer = Self::connect(port, false)?;
        let (services, height) = peer.handshake()?;
        Ok((peer, services, height))
    }

    fn connect(port: u16, v2: bool) -> Result<Self, Exit> {
        let stream = match TcpStream::connect(("127.0.0.1", port)) {
            Ok(s) => s,
            Err(e) => return failed(format!("connect to port {port}: {e}")),
        };
        stream
            .set_read_timeout(Some(Duration::from_secs(1)))
            .map_err(|e| Exit::Failed(format!("socket: {e}")))?;
        let store = HeaderStore::new(NETWORK).map_err(|e| Exit::Failed(format!("store: {e}")))?;
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(1);
        let config = SessionConfig {
            network: NETWORK,
            nonce,
            tip_height: store.tip().0,
        };
        let session = if v2 {
            Session::new_v2(config, now_secs(), entropy()?)?
        } else {
            Session::new(config, now_secs())
        };
        let mut peer = Self {
            stream,
            session,
            store,
            received: Scanner::default(),
            sent: Scanner::default(),
            received_log: Vec::new(),
            sent_log: Vec::new(),
            ready: false,
            counting: false,
            sent_after_handshake: 0,
            accepted: 0,
            header_messages: 0,
            fallback: false,
        };
        peer.flush()?;
        Ok(peer)
    }

    /// Write what the session queued.
    fn flush(&mut self) -> Result<(), Exit> {
        let bytes = self.session.take_outgoing();
        if bytes.is_empty() {
            return Ok(());
        }
        if self.session.is_v2() {
            // Packets cannot be read here. A `pong` is one packet of 29
            // bytes (9 of contents, 20 of overhead); other bytes count.
            if self.counting && bytes.len() != 29 {
                self.sent_after_handshake += bytes.len();
            }
        } else {
            for (command, len) in self.sent.push(&bytes) {
                if self.counting && command != "pong" {
                    self.sent_after_handshake += 24 + len;
                }
                self.sent_log.push(command);
            }
        }
        if let Err(e) = self.stream.write_all(&bytes) {
            return failed(format!("write: {e}"));
        }
        Ok(())
    }

    /// One read of up to a second, then `tick`. Headers go to the store.
    fn pump(&mut self) -> Result<Vec<Event>, Exit> {
        let mut buf = [0u8; 65_536];
        let events = match self.stream.read(&mut buf) {
            // NR-81: a close before any packet authenticated.
            Ok(0) if self.session.v2_unproven() => return Err(Exit::Dropped(Drop::V2NotSpoken)),
            Ok(0) => return failed("the peer closed the connection"),
            Ok(n) => {
                if !self.session.is_v2() {
                    for (command, _) in self.received.push(&buf[..n]) {
                        self.received_log.push(command);
                    }
                }
                self.session.receive(&buf[..n], now_secs())?
            }
            Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {
                self.session.tick(now_secs())?;
                Vec::new()
            }
            Err(e) => return failed(format!("read: {e}")),
        };
        for event in &events {
            if let Event::Headers(headers) = event {
                self.header_messages += 1;
                for header in headers {
                    match self.store.accept(header, now_secs()) {
                        Ok(Accepted::Known) => {}
                        Ok(_) => self.accepted += 1,
                        Err(e) => return failed(format!("header refused: {e}")),
                    }
                }
            }
        }
        self.flush()?;
        // The `sendheaders` that goes out with the end of the handshake is
        // part of the handshake; bytes written after that are counted.
        self.counting = self.ready;
        Ok(events)
    }

    /// Run the handshake. Returns the peer's services and start height.
    fn handshake(&mut self) -> Result<(ServiceFlags, u32), Exit> {
        loop {
            for event in self.pump()? {
                if let Event::Ready {
                    services,
                    start_height,
                } = event
                {
                    self.ready = true;
                    return Ok((services, start_height));
                }
            }
        }
    }

    fn send(&mut self, request: Request) -> Result<(), Exit> {
        self.session.send(request)?;
        self.flush()
    }

    fn ask_headers(&mut self) -> Result<(), Exit> {
        let locator = self.store.locator();
        self.send(Request::GetHeaders { locator })
    }

    /// NR-30: ask for headers, and again after every full message, until
    /// the tip is `expected`, or (without one) until a short message.
    fn sync(&mut self, expected: Option<BlockHash>) -> Result<(), Exit> {
        let deadline = Instant::now() + Duration::from_secs(SYNC_SECS);
        self.ask_headers()?;
        loop {
            if expected == Some(self.store.tip().1) {
                return Ok(());
            }
            if Instant::now() > deadline {
                return failed("sync: time ran out before the tip was reached");
            }
            for event in self.pump()? {
                if let Event::Headers(headers) = event {
                    if headers.len() == openbmt_net::MAX_HEADERS {
                        self.ask_headers()?;
                    } else if expected.is_none() {
                        return Ok(());
                    }
                }
            }
        }
    }

    /// Wait for the first event `pick` wants.
    fn wait<T>(&mut self, mut pick: impl FnMut(Event) -> Option<T>) -> Result<T, Exit> {
        let deadline = Instant::now() + Duration::from_secs(ANSWER_SECS);
        loop {
            if Instant::now() > deadline {
                return failed(format!("no answer in {ANSWER_SECS} s"));
            }
            for event in self.pump()? {
                if let Some(found) = pick(event) {
                    return Ok(found);
                }
            }
        }
    }

    fn print_tip(&self) {
        let (height, hash) = self.store.tip();
        println!("tip {height} {hash}");
    }
}

impl std::ops::Drop for Peer {
    fn drop(&mut self) {
        let _ = self.stream.shutdown(Shutdown::Both);
    }
}

fn parse<T: FromStr>(what: &str, text: &str) -> Result<T, Exit> {
    match text.parse() {
        Ok(v) => Ok(v),
        Err(_) => failed(format!("{what} is not valid: {text}")),
    }
}

fn script_arg(text: &str) -> Result<ScriptBuf, Exit> {
    match ScriptBuf::from_hex(text) {
        Ok(s) => Ok(s),
        Err(e) => failed(format!("script is not hex: {e}")),
    }
}

fn paid(tx: &Transaction, script: &ScriptBuf) -> u64 {
    tx.output
        .iter()
        .filter(|o| o.script_pubkey == *script)
        .map(|o| o.value.to_sat())
        .sum()
}

fn run(port: u16, args: &[String]) -> Result<(), Exit> {
    let arg = |i: usize| -> Result<&str, Exit> {
        match args.get(i) {
            Some(a) => Ok(a.as_str()),
            None => failed(USAGE),
        }
    };
    let command = arg(0)?;
    // Check the arguments before dialing.
    match command {
        "handshake" => {}
        "sync" => {
            parse::<BlockHash>("tip hash", arg(1)?)?;
        }
        "announce" | "silent" => {
            parse::<u64>("seconds", arg(1)?)?;
        }
        "filter" => {
            parse::<u32>("height", arg(1)?)?;
            parse::<BlockHash>("block hash", arg(2)?)?;
            script_arg(arg(3)?)?;
        }
        "block" => {
            parse::<BlockHash>("block hash", arg(1)?)?;
            script_arg(arg(2)?)?;
        }
        "tx" => {
            parse::<u64>("seconds", arg(1)?)?;
            script_arg(arg(2)?)?;
        }
        _ => return failed(USAGE),
    }
    let (mut peer, services, start_height) = Peer::open(port)?;
    println!(
        "transport {}",
        if peer.session.is_v2() { "v2" } else { "v1" }
    );
    println!("fallback {}", if peer.fallback { "yes" } else { "no" });
    match peer.session.v2_session_id() {
        Some(id) => println!("session_id {}", hex(&id)),
        None => println!("session_id none"),
    }
    match command {
        "handshake" => {
            println!("ready {:x} {start_height}", services.to_u64());
            let filters = services.has(ServiceFlags::COMPACT_FILTERS);
            println!("filters {}", if filters { "yes" } else { "no" });
            if peer.session.is_v2() {
                println!("sent encrypted");
            } else {
                println!("sent {}", peer.sent_log.join(" "));
            }
        }
        "sync" => {
            let expected: BlockHash = parse("tip hash", arg(1)?)?;
            peer.sync(Some(expected))?;
            println!("headers {}", peer.accepted);
            println!("messages {}", peer.header_messages);
            peer.print_tip();
        }
        "announce" => {
            let seconds: u64 = parse("seconds", arg(1)?)?;
            peer.sync(None)?;
            println!("waiting");
            let deadline = Instant::now() + Duration::from_secs(seconds);
            while Instant::now() < deadline {
                for event in peer.pump()? {
                    match event {
                        Event::Headers(headers) => println!("announced-headers {}", headers.len()),
                        Event::Announced { blocks, .. } if !blocks.is_empty() => {
                            for block in &blocks {
                                println!("announced-inv-block {block}");
                            }
                            // An inv carries no header: ask for it.
                            peer.ask_headers()?;
                        }
                        _ => {}
                    }
                }
            }
            peer.print_tip();
        }
        "filter" => {
            let height: u32 = parse("height", arg(1)?)?;
            let block: BlockHash = parse("block hash", arg(2)?)?;
            let script = script_arg(arg(3)?)?;
            peer.send(Request::GetFilter { height, block })?;
            let (got, bytes) = peer.wait(|e| match e {
                Event::Filter { block, filter } => Some((block, filter)),
                _ => None,
            })?;
            if got != block {
                return failed(format!("the filter is for {got}, not {block}"));
            }
            println!("filter-bytes {}", bytes.len());
            let query = [script.as_bytes()];
            let hit = match BlockFilter::new(&bytes).match_any(&block, query.into_iter()) {
                Ok(hit) => hit,
                Err(e) => return failed(format!("filter does not read: {e}")),
            };
            println!("match {}", if hit { "yes" } else { "no" });
        }
        "block" => {
            let block: BlockHash = parse("block hash", arg(1)?)?;
            let script = script_arg(arg(2)?)?;
            peer.sync(None)?;
            peer.send(Request::GetBlock(block))?;
            let bytes = peer.wait(|e| match e {
                Event::Block(bytes) => Some(bytes),
                _ => None,
            })?;
            println!("block-bytes {}", bytes.len());
            let found = match payments_in_block(&peer.store, &bytes, &script) {
                Ok(found) => found,
                Err(e) => return failed(format!("block refused: {e}")),
            };
            println!("payments {}", found.len());
            for payment in &found {
                let tx = payment.tx();
                println!(
                    "payment {} {} {}",
                    tx.compute_txid(),
                    payment.position(),
                    paid(tx, &script)
                );
            }
        }
        "tx" => {
            let seconds: u64 = parse("seconds", arg(1)?)?;
            let script = script_arg(arg(2)?)?;
            let deadline = Instant::now() + Duration::from_secs(seconds);
            while Instant::now() < deadline {
                for event in peer.pump()? {
                    match event {
                        Event::Announced { txids, .. } if !txids.is_empty() => {
                            for txid in &txids {
                                println!("announced {txid}");
                            }
                            peer.send(Request::GetTx(txids))?;
                        }
                        Event::Tx(bytes) => {
                            let tx: Transaction = match deserialize(&bytes) {
                                Ok(tx) => tx,
                                Err(e) => return failed(format!("tx does not parse: {e}")),
                            };
                            let sats = paid(&tx, &script);
                            let witness = tx.input.iter().any(|i| !i.witness.is_empty());
                            println!(
                                "tx {} {sats} {}",
                                tx.compute_txid(),
                                if witness { "yes" } else { "no" }
                            );
                            if sats > 0 {
                                return Ok(());
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
        "silent" => {
            let seconds: u64 = parse("seconds", arg(1)?)?;
            let deadline = Instant::now() + Duration::from_secs(seconds);
            while Instant::now() < deadline {
                let events = peer.pump()?;
                for command in peer.received_log.drain(..) {
                    println!("received {command}");
                }
                for event in events {
                    println!("event {}", event_name(&event));
                    if let Event::Announced { txids, blocks } = &event {
                        for txid in txids {
                            println!("event-tx {txid}");
                        }
                        for block in blocks {
                            println!("event-block {block}");
                        }
                    }
                }
            }
            println!("sent-bytes-after-handshake {}", peer.sent_after_handshake);
        }
        _ => unreachable!("checked above"),
    }
    Ok(())
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn event_name(event: &Event) -> &'static str {
    match event {
        Event::Ready { .. } => "ready",
        Event::Headers(_) => "headers",
        Event::Announced { .. } => "announced",
        Event::Tx(_) => "tx",
        Event::Block(_) => "block",
        Event::Filter { .. } => "filter",
        Event::NotFound { .. } => "notfound",
        Event::Addresses(_) => "addresses",
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(port) = args.first().and_then(|p| p.parse::<u16>().ok()) else {
        println!("failed {USAGE}");
        return ExitCode::from(1);
    };
    match run(port, &args[1..]) {
        Ok(()) => ExitCode::SUCCESS,
        Err(Exit::Failed(why)) => {
            println!("failed {why}");
            ExitCode::from(1)
        }
        Err(Exit::Dropped(d)) => {
            println!("dropped {d}");
            ExitCode::from(2)
        }
    }
}
