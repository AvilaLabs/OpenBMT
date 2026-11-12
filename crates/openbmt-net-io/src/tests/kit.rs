//! Fakes for the tests: a storage in memory, a counting randomness, a peer
//! on a loopback socket and the nets that map any address to it.

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::{IpAddr, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use bitcoin::bip158::BlockFilter;
use bitcoin::block::{Block, Header, Version};
use bitcoin::blockdata::constants::genesis_block;
use bitcoin::consensus::deserialize;
use bitcoin::hashes::Hash;
use bitcoin::p2p::address::Address;
use bitcoin::p2p::message::NetworkMessage;
use bitcoin::p2p::message_blockdata::Inventory;
use bitcoin::p2p::message_filter::{CFilter, GetCFilters};
use bitcoin::p2p::message_network::VersionMessage;
use bitcoin::p2p::{Magic, ServiceFlags};
use bitcoin::transaction::Version as TxVersion;
use bitcoin::{
    Amount, BlockHash, CompactTarget, Network, OutPoint, ScriptBuf, Sequence, Transaction, TxIn,
    TxMerkleNode, TxOut, Txid, Witness,
};
use openbmt_net::encode;

use crate::dial::{DialError, Net};
use crate::driver::{AppEvent, Handle};
use crate::env::{Clock, Key, Randomness, Storage};
use crate::socks::{ReplyCode, SocksError, Target};

/// The start height the fake peer claims.
pub const PEER_HEIGHT: u32 = 850_000;
/// Time of the regtest genesis block.
pub const GENESIS_TIME: u32 = 1_296_688_602;

/// A storage that keeps its values in memory and can be shared.
#[derive(Clone, Default)]
pub struct MemStorage {
    pub values: Arc<Mutex<BTreeMap<Key, Vec<u8>>>>,
    /// The key of every save, in order.
    pub saves: Arc<Mutex<Vec<Key>>>,
}

impl MemStorage {
    pub fn with(key: Key, bytes: Vec<u8>) -> Self {
        let s = Self::default();
        s.values.lock().unwrap().insert(key, bytes);
        s
    }

    pub fn get(&self, key: Key) -> Option<Vec<u8>> {
        self.values.lock().unwrap().get(&key).cloned()
    }

    /// How many times this key was saved.
    pub fn save_count(&self, key: Key) -> usize {
        self.saves
            .lock()
            .unwrap()
            .iter()
            .filter(|k| **k == key)
            .count()
    }
}

impl Storage for MemStorage {
    type Error = String;

    fn load(&mut self, key: Key) -> Result<Option<Vec<u8>>, String> {
        Ok(self.get(key))
    }

    fn save(&mut self, key: Key, bytes: &[u8]) -> Result<(), String> {
        self.values.lock().unwrap().insert(key, bytes.to_vec());
        self.saves.lock().unwrap().push(key);
        Ok(())
    }
}

/// A clock the test sets. The driver reads it on every pass.
#[derive(Clone)]
pub struct TestClock(pub Arc<AtomicU64>);

impl TestClock {
    pub fn at(time: u64) -> Self {
        Self(Arc::new(AtomicU64::new(time)))
    }

    pub fn set(&self, time: u64) {
        self.0.store(time, Ordering::SeqCst);
    }
}

impl Clock for TestClock {
    fn now(&self) -> u64 {
        self.0.load(Ordering::SeqCst)
    }
}

/// Numbers that differ from call to call.
pub struct CountingRandomness(pub u64);

impl Randomness for CountingRandomness {
    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        self.0
    }
}

/// A regtest header that extends `prev`, with a valid proof of work.
pub fn mine(prev: BlockHash, time: u32) -> Header {
    let mut header = Header {
        version: Version::from_consensus(0x2000_0000),
        prev_blockhash: prev,
        merkle_root: TxMerkleNode::all_zeros(),
        time,
        bits: CompactTarget::from_consensus(0x207f_ffff),
        nonce: 0,
    };
    while header.validate_pow(header.target()).is_err() {
        header.nonce += 1;
    }
    header
}

/// `n` regtest headers on top of the genesis block.
pub fn chain(n: u32) -> Vec<Header> {
    let mut prev = genesis_block(Network::Regtest).block_hash();
    let mut out = Vec::new();
    for i in 1..=n {
        let header = mine(prev, GENESIS_TIME + 600 * i);
        prev = header.block_hash();
        out.push(header);
    }
    out
}

/// A block on `prev` whose coinbase pays `pad` bytes of script to nobody,
/// so that a test can make it as large as it needs. The tag makes the
/// coinbase, and so the block, differ from another on the same parent.
pub fn block_on(prev: &Header, tag: u32, pad: usize) -> Block {
    let mut script_sig = vec![0x04];
    script_sig.extend(tag.to_le_bytes());
    let coinbase = Transaction {
        version: TxVersion::TWO,
        lock_time: bitcoin::absolute::LockTime::ZERO,
        input: vec![TxIn {
            previous_output: OutPoint::null(),
            script_sig: ScriptBuf::from_bytes(script_sig),
            sequence: Sequence::MAX,
            witness: Witness::new(),
        }],
        output: vec![TxOut {
            value: Amount::from_sat(5_000_000_000),
            script_pubkey: ScriptBuf::from_bytes(vec![0x6a; pad.max(1)]),
        }],
    };
    let mut block = Block {
        header: *prev,
        txdata: vec![coinbase],
    };
    let root = block.compute_merkle_root().unwrap();
    let mut header = Header {
        version: Version::from_consensus(0x2000_0000),
        prev_blockhash: prev.block_hash(),
        merkle_root: root,
        time: prev.time + 600,
        bits: CompactTarget::from_consensus(0x207f_ffff),
        nonce: 0,
    };
    while header.validate_pow(header.target()).is_err() {
        header.nonce += 1;
    }
    block.header = header;
    block
}

/// A block on `prev` whose coinbase pays 1,000 satoshis to this script.
pub fn block_paying(prev: &Header, tag: u32, to: &ScriptBuf) -> Block {
    let mut block = block_on(prev, tag, 1);
    block.txdata[0].output[0].script_pubkey = to.clone();
    let root = block.compute_merkle_root().unwrap();
    let mut header = Header {
        merkle_root: root,
        nonce: 0,
        ..block.header
    };
    while header.validate_pow(header.target()).is_err() {
        header.nonce += 1;
    }
    block.header = header;
    block
}

/// `n` blocks of about `pad` bytes each on top of the genesis block.
pub fn blocks(n: u32, pad: usize) -> Vec<Block> {
    let mut prev = genesis_block(Network::Regtest).header;
    let mut out = Vec::new();
    for i in 1..=n {
        let block = block_on(&prev, i, pad);
        prev = block.header;
        out.push(block);
    }
    out
}

pub fn magic() -> Magic {
    Magic::from(Network::Regtest)
}

fn frame(message: NetworkMessage) -> Vec<u8> {
    encode(magic(), &message)
}

fn version_message(start_height: u32, services: ServiceFlags) -> VersionMessage {
    let silent = Address {
        services: ServiceFlags::NONE,
        address: [0; 8],
        port: 0,
    };
    VersionMessage {
        version: 70_016,
        services,
        timestamp: 1_700_000_000,
        receiver: silent.clone(),
        sender: silent,
        nonce: 42,
        user_agent: "/fake:1/".to_string(),
        start_height: i32::try_from(start_height).unwrap(),
        relay: true,
    }
}

/// What the fake peer does after the handshake.
#[derive(Clone)]
pub enum Behavior {
    /// Answer `getheaders` with these headers.
    Serve(Vec<Header>),
    /// Like `Serve`, but claim a height of exactly these headers, so that
    /// the manager never takes the peer for one that claims too much.
    Honest(Vec<Header>),
    /// Answer `getheaders`, then tell of these addresses.
    ServeAndTell(Vec<Header>, Vec<SocketAddr>),
    /// Close the connection after the first `getheaders`.
    CloseAfterHandshake,
    /// Send bytes that are not a message.
    Garbage,
    /// Send these headers in messages of one header each, two milliseconds
    /// apart.
    Flood(Vec<Header>),
    /// Accept the connection and say nothing.
    Silent,
    /// Answer `getheaders` with the headers of these blocks, and `getdata`
    /// with the blocks asked for.
    ServeBlocks(Vec<Block>),
    /// Like `Honest`, and ask for every transaction announced with `inv`
    /// and keep what comes back.
    Fetches(Vec<Header>),
    /// A node that serves as the own node: after the handshake it announces
    /// these txids with `inv` and answers `getdata` for the transactions
    /// it holds (`notfound` for the others); it claims this height.
    Node(NodeScript),
    /// An own node that, after the handshake, sends a block nobody asked
    /// for.
    NodeSendsBlock,
    /// An own node that closes the connection after the handshake.
    NodeCloses,
}

/// What a fake own node announces and holds.
#[derive(Clone, Default)]
pub struct NodeScript {
    pub height: u32,
    pub announce: Vec<Txid>,
    pub mempool: Vec<Transaction>,
    /// Advertise compact filters (NODE_COMPACT_FILTERS).
    pub filters: bool,
    /// Advertise NODE_NETWORK_LIMITED instead of NODE_NETWORK.
    pub limited: bool,
    /// A chain the node serves: its headers at `getheaders`, the filter of
    /// each block at `getcfilters` and the block at `getdata`, unless it is
    /// `limited` and the block is more than 288 below its tip. Claims the
    /// height of the chain when `height` is 0.
    pub blocks: Vec<Block>,
}

/// The height the fake peer claims in its version message. A peer that
/// claims more than it proves is dropped after 10 minutes (NR-33), so one
/// that is to stay must claim what it serves.
fn claimed_height(behavior: &Behavior) -> u32 {
    match behavior {
        Behavior::Honest(headers) | Behavior::Fetches(headers) => {
            u32::try_from(headers.len()).unwrap()
        }
        Behavior::Node(script) if script.height == 0 => {
            u32::try_from(script.blocks.len()).unwrap_or(0)
        }
        Behavior::Node(script) => script.height,
        _ => PEER_HEIGHT,
    }
}

/// The services the fake peer states.
fn claimed_services(behavior: &Behavior) -> ServiceFlags {
    match behavior {
        Behavior::Node(script) => {
            let blocks = if script.limited {
                ServiceFlags::NETWORK_LIMITED
            } else {
                ServiceFlags::NETWORK
            };
            let filters = if script.filters {
                ServiceFlags::COMPACT_FILTERS
            } else {
                ServiceFlags::NONE
            };
            blocks | ServiceFlags::WITNESS | filters
        }
        _ => ServiceFlags::NETWORK | ServiceFlags::WITNESS,
    }
}

/// A peer on a loopback socket.
pub struct FakePeer {
    pub addr: SocketAddr,
    /// Connections accepted.
    pub accepts: Arc<AtomicUsize>,
    /// The payloads of the `getheaders` messages received.
    pub getheaders: Arc<Mutex<Vec<Vec<u8>>>>,
    /// Connections the driver closed (a read returned 0).
    pub closed_by_driver: Arc<AtomicUsize>,
    /// Instants of the accepts.
    pub accepted_at: Arc<Mutex<Vec<Instant>>>,
    /// The txids announced with `inv`, and the payloads of the `tx`
    /// messages received.
    pub invs: Arc<Mutex<Vec<Txid>>>,
    pub txs: Arc<Mutex<Vec<Vec<u8>>>>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl FakePeer {
    pub fn start(behavior: Behavior) -> Self {
        Self::start_on("127.0.0.1", behavior)
    }

    /// A peer on another loopback address, so that it is a group of its own
    /// when the driver allows local peers.
    pub fn start_on(ip: &str, behavior: Behavior) -> Self {
        let listener = TcpListener::bind(format!("{ip}:0")).unwrap();
        listener.set_nonblocking(true).unwrap();
        let addr = listener.local_addr().unwrap();
        let mut peer = Self {
            addr,
            accepts: Arc::default(),
            getheaders: Arc::default(),
            closed_by_driver: Arc::default(),
            accepted_at: Arc::default(),
            invs: Arc::default(),
            txs: Arc::default(),
            stop: Arc::default(),
            thread: None,
        };
        let (accepts, getheaders, closed, at, stop) = (
            peer.accepts.clone(),
            peer.getheaders.clone(),
            peer.closed_by_driver.clone(),
            peer.accepted_at.clone(),
            peer.stop.clone(),
        );
        let (invs, txs) = (peer.invs.clone(), peer.txs.clone());
        let thread = thread::spawn(move || {
            let mut handlers = Vec::new();
            while !stop.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        accepts.fetch_add(1, Ordering::SeqCst);
                        at.lock().unwrap().push(Instant::now());
                        let (b, g, c, s) = (
                            behavior.clone(),
                            getheaders.clone(),
                            closed.clone(),
                            stop.clone(),
                        );
                        let seen = (invs.clone(), txs.clone());
                        handlers.push(thread::spawn(move || serve(stream, &b, &g, &c, &s, &seen)));
                    }
                    Err(_) => thread::sleep(Duration::from_millis(10)),
                }
            }
            for h in handlers {
                let _ = h.join();
            }
        });
        peer.thread = Some(thread);
        peer
    }

    pub fn accept_count(&self) -> usize {
        self.accepts.load(Ordering::SeqCst)
    }
}

impl Drop for FakePeer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// Cut complete messages out of the buffer: (command, payload).
fn messages(buf: &mut Vec<u8>) -> Vec<(String, Vec<u8>)> {
    let mut out = Vec::new();
    while buf.len() >= 24 {
        let len = u32::from_le_bytes([buf[16], buf[17], buf[18], buf[19]]) as usize;
        if buf.len() < 24 + len {
            break;
        }
        let name: Vec<u8> = buf[4..16].iter().copied().take_while(|b| *b != 0).collect();
        out.push((
            String::from_utf8_lossy(&name).into_owned(),
            buf[24..24 + len].to_vec(),
        ));
        buf.drain(..24 + len);
    }
    out
}

/// Run a fake peer on a stream that is already connected.
pub fn serve_stream(stream: TcpStream, behavior: &Behavior, stop: &AtomicBool) {
    let getheaders = Mutex::new(Vec::new());
    let closed = AtomicUsize::new(0);
    let seen = Seen::default();
    serve(stream, behavior, &getheaders, &closed, stop, &seen);
}

/// What a fake peer saw of transactions: the txids announced and the
/// payloads of `tx` messages.
type Seen = (Arc<Mutex<Vec<Txid>>>, Arc<Mutex<Vec<Vec<u8>>>>);

fn serve(
    mut stream: TcpStream,
    behavior: &Behavior,
    getheaders: &Mutex<Vec<Vec<u8>>>,
    closed: &AtomicUsize,
    stop: &AtomicBool,
    seen: &Seen,
) {
    stream
        .set_read_timeout(Some(Duration::from_millis(50)))
        .unwrap();
    if matches!(behavior, Behavior::Silent) {
        // Say nothing; read until the driver closes.
        let mut buf = [0u8; 1024];
        while !stop.load(Ordering::SeqCst) {
            if let Ok(0) = stream.read(&mut buf) {
                closed.fetch_add(1, Ordering::SeqCst);
                return;
            }
        }
        return;
    }
    let _ = stream.write_all(&frame(NetworkMessage::Version(version_message(
        claimed_height(behavior),
        claimed_services(behavior),
    ))));
    let _ = stream.write_all(&frame(NetworkMessage::Verack));
    match behavior {
        Behavior::Node(script) if !script.announce.is_empty() => {
            let list = script
                .announce
                .iter()
                .map(|t| Inventory::Transaction(*t))
                .collect();
            let _ = stream.write_all(&frame(NetworkMessage::Inv(list)));
        }
        Behavior::NodeSendsBlock => {
            let block = blocks(1, 10).remove(0);
            let _ = stream.write_all(&frame(NetworkMessage::Block(block)));
        }
        Behavior::NodeCloses => {
            thread::sleep(Duration::from_millis(100));
            return;
        }
        _ => {}
    }
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    let mut garbage_sent = false;
    while !stop.load(Ordering::SeqCst) {
        match stream.read(&mut chunk) {
            Ok(0) => {
                closed.fetch_add(1, Ordering::SeqCst);
                return;
            }
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
            Err(_) => {}
        }
        for (command, payload) in messages(&mut buf) {
            match command.as_str() {
                "getheaders" => {
                    getheaders.lock().unwrap().push(payload);
                    match behavior {
                        Behavior::Serve(headers)
                        | Behavior::Honest(headers)
                        | Behavior::Fetches(headers)
                        | Behavior::ServeAndTell(headers, _) => {
                            // The addresses come first, so that the
                            // headers' event proves they were read.
                            if let Behavior::ServeAndTell(_, told) = behavior {
                                let list = told
                                    .iter()
                                    .map(|a| {
                                        (1_700_000_000, Address::new(a, ServiceFlags::NETWORK))
                                    })
                                    .collect();
                                let _ = stream.write_all(&frame(NetworkMessage::Addr(list)));
                            }
                            let _ =
                                stream.write_all(&frame(NetworkMessage::Headers(headers.clone())));
                        }
                        Behavior::ServeBlocks(blocks) => {
                            let headers = blocks.iter().map(|b| b.header).collect();
                            let _ = stream.write_all(&frame(NetworkMessage::Headers(headers)));
                        }
                        Behavior::Node(script) if !script.blocks.is_empty() => {
                            let headers = script.blocks.iter().map(|b| b.header).collect();
                            let _ = stream.write_all(&frame(NetworkMessage::Headers(headers)));
                        }
                        Behavior::CloseAfterHandshake => return,
                        Behavior::Flood(headers) => {
                            for header in headers {
                                let m = NetworkMessage::Headers(vec![*header]);
                                if stream.write_all(&frame(m)).is_err() {
                                    return;
                                }
                                thread::sleep(Duration::from_millis(2));
                            }
                        }
                        Behavior::Garbage if !garbage_sent => {
                            garbage_sent = true;
                            let _ = stream.write_all(&[0xff; 40]);
                        }
                        _ => {}
                    }
                }
                "getcfilters" => {
                    if let Behavior::Node(script) = behavior {
                        let asked: GetCFilters = deserialize(&payload).unwrap();
                        if let Some(block) = script
                            .blocks
                            .iter()
                            .find(|b| b.block_hash() == asked.stop_hash)
                        {
                            let filter =
                                BlockFilter::new_script_filter(block, |_| Ok(ScriptBuf::new()))
                                    .unwrap()
                                    .content;
                            let message = NetworkMessage::CFilter(CFilter {
                                filter_type: 0,
                                block_hash: asked.stop_hash,
                                filter,
                            });
                            let _ = stream.write_all(&frame(message));
                        }
                    }
                }
                "getdata" => {
                    if let Behavior::Node(script) = behavior {
                        let asked: Vec<Inventory> = deserialize(&payload).unwrap_or_default();
                        let mut missing = Vec::new();
                        for item in asked {
                            if let Inventory::Block(hash) | Inventory::WitnessBlock(hash) = item {
                                if let Some(block) =
                                    script.blocks.iter().find(|b| b.block_hash() == hash)
                                {
                                    let message = NetworkMessage::Block(block.clone());
                                    let _ = stream.write_all(&frame(message));
                                }
                                continue;
                            }
                            let (Inventory::Transaction(txid)
                            | Inventory::WitnessTransaction(txid)) = item
                            else {
                                continue;
                            };
                            match script.mempool.iter().find(|t| t.compute_txid() == txid) {
                                Some(tx) => {
                                    let _ =
                                        stream.write_all(&frame(NetworkMessage::Tx(tx.clone())));
                                }
                                None => missing.push(Inventory::WitnessTransaction(txid)),
                            }
                        }
                        if !missing.is_empty() {
                            let _ = stream.write_all(&frame(NetworkMessage::NotFound(missing)));
                        }
                    }
                    if let Behavior::ServeBlocks(blocks) = behavior {
                        let asked: Vec<Inventory> = deserialize(&payload).unwrap_or_default();
                        for item in asked {
                            let (Inventory::Block(hash) | Inventory::WitnessBlock(hash)) = item
                            else {
                                continue;
                            };
                            if let Some(block) = blocks.iter().find(|b| b.block_hash() == hash) {
                                let message = NetworkMessage::Block(block.clone());
                                let _ = stream.write_all(&frame(message));
                            }
                        }
                    }
                }
                "inv" => {
                    let announced: Vec<Inventory> = deserialize(&payload).unwrap_or_default();
                    let mut ask = Vec::new();
                    for item in announced {
                        if let Inventory::Transaction(txid) = item {
                            seen.0.lock().unwrap().push(txid);
                            ask.push(Inventory::WitnessTransaction(txid));
                        }
                    }
                    if matches!(behavior, Behavior::Fetches(_)) && !ask.is_empty() {
                        let _ = stream.write_all(&frame(NetworkMessage::GetData(ask)));
                    }
                }
                "tx" => seen.1.lock().unwrap().push(payload),
                "ping" => {
                    let mut nonce = [0u8; 8];
                    nonce.copy_from_slice(&payload[..8]);
                    let _ =
                        stream.write_all(&frame(NetworkMessage::Pong(u64::from_le_bytes(nonce))));
                }
                _ => {}
            }
        }
    }
}

/// What the dial hook saw.
pub type DialLog = Arc<Mutex<Vec<(Target, Option<SocketAddr>)>>>;

/// A net whose dials all go to `to`, and whose lookups return `resolved`.
pub fn net_to(to: SocketAddr, resolved: Vec<SocketAddr>) -> (Net, DialLog, Arc<AtomicUsize>) {
    let log: DialLog = Arc::default();
    let lookups = Arc::new(AtomicUsize::new(0));
    let (l, n) = (log.clone(), lookups.clone());
    let net = Net {
        dial: Arc::new(move |target, proxy, _| {
            l.lock().unwrap().push((target.clone(), proxy));
            TcpStream::connect(to).map_err(|e| DialError::Connect(e.kind()))
        }),
        resolve: Arc::new(move |_, _| {
            n.fetch_add(1, Ordering::SeqCst);
            Ok(resolved.clone())
        }),
        proxy_resolve: Arc::new(not_tor),
    };
    (net, log, lookups)
}

/// A proxy that is not Tor: it does not know the lookup command.
pub fn not_tor(_: SocketAddr, _: &str, _: u64) -> Result<IpAddr, SocksError> {
    Err(SocksError::Refused(ReplyCode::CommandNotSupported))
}

/// Wait up to `secs` for an event the test wants; the events before it are
/// returned too.
pub fn wait_for(
    handle: &Handle,
    secs: u64,
    want: impl Fn(&AppEvent) -> bool,
) -> Option<Vec<AppEvent>> {
    let end = Instant::now() + Duration::from_secs(secs);
    let mut seen = Vec::new();
    while let Some(left) = end.checked_duration_since(Instant::now()) {
        match handle.events().recv_timeout(left) {
            Ok(event) => {
                let hit = want(&event);
                seen.push(event);
                if hit {
                    return Some(seen);
                }
            }
            Err(_) => return None,
        }
    }
    None
}

/// An event with at least one ready peer.
pub fn has_peer(event: &AppEvent) -> bool {
    matches!(event, AppEvent::Peers(peers, ..) if !peers.is_empty())
}

pub fn is_note(event: &AppEvent) -> bool {
    matches!(event, AppEvent::Note(_))
}

/// Poll until the condition holds or `secs` pass.
pub fn until(secs: u64, cond: impl Fn() -> bool) -> bool {
    let end = Instant::now() + Duration::from_secs(secs);
    while Instant::now() < end {
        if cond() {
            return true;
        }
        thread::sleep(Duration::from_millis(20));
    }
    cond()
}

/// What a net for Tor tests saw.
#[derive(Clone, Default)]
pub struct TorLog {
    /// The names asked of the proxy, with the token of each lookup.
    pub asked: Arc<Mutex<Vec<(String, u64)>>>,
    /// The dials.
    pub dials: DialLog,
    /// Calls of the system's resolver.
    pub system: Arc<AtomicUsize>,
}

/// A net with a proxy lookup that answers by `answer(n)` for the n-th call
/// (from 0), every dial refused, and a system resolver that is counted.
pub fn tor_net(
    answer: impl Fn(usize) -> Result<IpAddr, SocksError> + Send + Sync + 'static,
) -> (Net, TorLog) {
    let log = TorLog::default();
    let (d, a, r) = (log.dials.clone(), log.asked.clone(), log.system.clone());
    let net = Net {
        dial: Arc::new(move |target, proxy, _| {
            d.lock().unwrap().push((target.clone(), proxy));
            Err(DialError::Connect(std::io::ErrorKind::ConnectionRefused))
        }),
        resolve: Arc::new(move |_, _| {
            r.fetch_add(1, Ordering::SeqCst);
            Ok(Vec::new())
        }),
        proxy_resolve: Arc::new(move |_, name, token| {
            let n = {
                let mut asked = a.lock().unwrap();
                asked.push((name.to_string(), token));
                asked.len() - 1
            };
            answer(n)
        }),
    };
    (net, log)
}
