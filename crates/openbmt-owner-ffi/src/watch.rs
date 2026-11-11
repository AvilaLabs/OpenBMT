//! The owner phone's background watcher, as the app calls it (OA-80 to
//! OA-89, LK-75).
//!
//! The watcher is its own handle: it is not a node, it holds no owner secret
//! and no key 1, and the ops of a node refuse it (`watch-only`). The app
//! keeps what it needs to start it in a file of its own that the Android
//! Keystore seals and the PIN does not guard (OA-81): the watch key's secret
//! and what the watcher needs to reach the counters. That file is
//! [`WatchStart`] behind a magic and a version, read strictly by
//! [`decode_file`]: exact length, every list and string within its limit, and
//! the bytes encode again to the same bytes.

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Mutex;
use std::thread::JoinHandle;
use std::time::Duration;

use openbmt_core::identity::{DeviceId, Identity};
use openbmt_net_io::DriverConfig;
use openbmt_owner_node::{WatchConfig, WatchLinkConfig, WatchNode};

use crate::codec::{wire_record, wire_record_get, Get, Put, Reader};
use crate::error::{internal, refused_by, wrong_state, AppError};
use crate::views::{OwnerNetwork, RouteView};

/// The first four bytes of the watch file.
pub const MAGIC_WATCH_FILE: [u8; 4] = *b"OBM#";
/// The version of the watch file.
pub const WATCH_FILE_VERSION: u8 = 1;
/// The most bytes the file may hold.
pub const MAX_WATCH_FILE: usize = 16 * 1024;
/// The most counters the file may name.
pub const MAX_WATCH_COUNTERS: usize = 64;
/// The most first peers the file may name.
pub const MAX_WATCH_PEERS: usize = 16;
/// The longest text of the file, in bytes.
const MAX_TEXT: usize = 1024;

/// What the watcher is started with, and what the file holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WatchStart {
    /// The node's data folder: the app's own (`noBackupFilesDir`).
    pub dir: String,
    /// The network.
    pub network: OwnerNetwork,
    /// The watch key's secret, 32 bytes.
    pub secret: Vec<u8>,
    /// The SOCKS5 proxy of Tor for the connections, `host:port`.
    pub proxy: Option<String>,
    /// Peers to connect to first, `host:port`.
    pub fixed_peers: Vec<String>,
    /// How to reach the relay.
    pub route: RouteView,
    /// The relay's key, 32 bytes.
    pub relay_key: Vec<u8>,
    /// The counters' roots, each 32 bytes.
    pub counters: Vec<Vec<u8>>,
    /// Seconds between the heartbeats sent to a counter (LK-61).
    pub heartbeat_every_secs: u64,
}

wire_record!(WatchStart {
    dir,
    network,
    secret,
    proxy,
    fixed_peers,
    route,
    relay_key,
    counters,
    heartbeat_every_secs
});
wire_record_get!(WatchStart {
    dir,
    network,
    secret,
    proxy,
    fixed_peers,
    route,
    relay_key,
    counters,
    heartbeat_every_secs
});

/// What one turn of the watcher says, for the notification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WatchStepView {
    /// Heartbeats the relay took since the start.
    pub sent: u64,
    /// Whether the last call to the relay worked.
    pub reached: bool,
    /// Whether the relay answered and refused: the watch key is not one of
    /// its members.
    pub relay_refused: bool,
    /// The watcher's own lines since the last turn.
    pub notes: Vec<String>,
}

wire_record!(WatchStepView {
    sent,
    reached,
    relay_refused,
    notes
});

fn bad(text: impl std::fmt::Display) -> AppError {
    refused_by("watch-file", format!("the watch file: {text}"))
}

fn text_ok(what: &str, text: &str) -> Result<(), AppError> {
    if text.len() > MAX_TEXT {
        return Err(bad(format!("{what} is longer than {MAX_TEXT} bytes")));
    }
    Ok(())
}

impl WatchStart {
    /// The file: magic, version, then the fields.
    pub fn to_file(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&MAGIC_WATCH_FILE);
        out.push(WATCH_FILE_VERSION);
        self.put(&mut out);
        out
    }

    /// The limits of the file, which `decode_file` enforces and the app's
    /// writer must meet.
    fn check(&self) -> Result<(), AppError> {
        if self.secret.len() != 32 {
            return Err(bad("the watch key's secret is not 32 bytes"));
        }
        if Identity::from_secret_bytes(&to32(&self.secret)?).is_err() {
            return Err(bad("the watch key's secret is not a valid key"));
        }
        if self.relay_key.len() != 32 {
            return Err(bad("the relay's key is not 32 bytes"));
        }
        if self.counters.len() > MAX_WATCH_COUNTERS {
            return Err(bad(format!(
                "more than {MAX_WATCH_COUNTERS} counters are named"
            )));
        }
        for c in &self.counters {
            DeviceId::from_bytes(&to32(c)?).map_err(|_| bad("a counter's root is not a key"))?;
        }
        if self.fixed_peers.len() > MAX_WATCH_PEERS {
            return Err(bad(format!("more than {MAX_WATCH_PEERS} first peers")));
        }
        text_ok("the folder", &self.dir)?;
        if self.dir.is_empty() {
            return Err(bad("the folder is empty"));
        }
        for p in self.proxy.iter().chain(&self.fixed_peers) {
            text_ok("an address", p)?;
        }
        match &self.route {
            RouteView::Direct { addr } => text_ok("the relay's address", addr)?,
            RouteView::Onion { proxy, host, .. } => {
                text_ok("the relay's proxy", proxy)?;
                text_ok("the relay's name", host)?;
            }
        }
        Ok(())
    }
}

fn to32(bytes: &[u8]) -> Result<[u8; 32], AppError> {
    <[u8; 32]>::try_from(bytes).map_err(|_| bad("a key is not 32 bytes"))
}

/// Read the file strictly (OA-81): at most [`MAX_WATCH_FILE`] bytes, the
/// magic, the version, every field, nothing left over, every list and text
/// within its limit, every key a key, and the bytes encode again to the
/// same bytes.
pub fn decode_file(bytes: &[u8]) -> Result<WatchStart, AppError> {
    if bytes.len() > MAX_WATCH_FILE {
        return Err(bad(format!("it is longer than {MAX_WATCH_FILE} bytes")));
    }
    let mut r = Reader::new(bytes);
    if r.take(4)? != MAGIC_WATCH_FILE {
        return Err(bad("it does not start OBM#"));
    }
    let version = r.u8()?;
    if version != WATCH_FILE_VERSION {
        return Err(bad(format!("version {version} is not known")));
    }
    let start = WatchStart::get(&mut r)?;
    r.end()?;
    start.check()?;
    if start.to_file() != bytes {
        return Err(bad("it does not encode again to the same bytes"));
    }
    Ok(start)
}

/// A file for the secret: the public key the settings would name (OA-81).
/// The secret must be 32 bytes that make a key; the app draws them from
/// the system's random source and keeps them in the sealed file, never
/// derived from any wallet or identity key.
pub fn public_key(secret: &[u8]) -> Result<Vec<u8>, AppError> {
    let secret = <[u8; 32]>::try_from(secret)
        .map_err(|_| refused_by("watch-key", "the watch key's secret is not 32 bytes"))?;
    let identity = Identity::from_secret_bytes(&secret)
        .map_err(|e| refused_by("watch-key", format!("the watch key's secret: {e}")))?;
    Ok(identity.device_id().to_bytes().to_vec())
}

/// What the watcher's thread is asked.
enum Order {
    Step(u64, u32, Sender<WatchStepView>),
    Stop(Sender<()>),
}

/// One running watcher. The node and its clock are not `Send`, so they live
/// on a thread of their own that this layer starts, as the owner node does
/// (`host`); the calls are orders sent to it.
pub struct WatchEntry {
    orders: Mutex<Option<Sender<Order>>>,
    thread: Mutex<Option<JoinHandle<()>>>,
}

fn work(node: &mut WatchNode, now: u64, wait_ms: u32) -> WatchStepView {
    node.pump(Duration::from_millis(u64::from(
        wait_ms.min(crate::host::MAX_WAIT_MS),
    )));
    node.step(now);
    let state = node.state();
    WatchStepView {
        sent: state.sent,
        reached: state.reached,
        relay_refused: state.relay_refused,
        notes: node.take_notes(),
    }
}

fn run_thread(config: WatchConfig, ready: Sender<Result<(), AppError>>, orders: Receiver<Order>) {
    let mut node = match WatchNode::start(config) {
        Ok(node) => {
            let _ = ready.send(Ok(()));
            node
        }
        Err(e) => {
            let _ = ready.send(Err(refused_by("watch-start", e)));
            return;
        }
    };
    while let Ok(order) = orders.recv() {
        match order {
            Order::Step(now, wait_ms, reply) => {
                let view = catch_unwind(AssertUnwindSafe(|| work(&mut node, now, wait_ms)));
                // A panic leaves nothing sent back: the caller sees the
                // watcher stopped.
                match view {
                    Ok(view) => {
                        let _ = reply.send(view);
                    }
                    Err(_) => break,
                }
            }
            Order::Stop(reply) => {
                node.shutdown();
                let _ = reply.send(());
                return;
            }
        }
    }
    node.shutdown();
}

impl WatchEntry {
    /// Start a watcher from the file's contents. The real driver and the
    /// real relay, on the node's folder.
    pub(crate) fn start(file: &[u8]) -> Result<Self, AppError> {
        let s = decode_file(file)?;
        let relay = DeviceId::from_bytes(&to32(&s.relay_key)?)
            .map_err(|_| bad("the relay's key is not a key"))?;
        let mut counters = Vec::with_capacity(s.counters.len());
        for c in &s.counters {
            counters.push(
                DeviceId::from_bytes(&to32(c)?)
                    .map_err(|_| bad("a counter's root is not a key"))?,
            );
        }
        let mut driver = DriverConfig::new(s.network.network(), 0);
        driver.proxy = s
            .proxy
            .as_deref()
            .map(str::parse)
            .transpose()
            .map_err(|e| refused_by("start-proxy", format!("the proxy address: {e}")))?;
        for peer in &s.fixed_peers {
            driver.fixed_peers.push(peer.parse().map_err(|e| {
                refused_by("start-peer", format!("the peer address {peer:?}: {e}"))
            })?);
        }
        let route = crate::host::route_of(&s.route)?;
        let link = WatchLinkConfig {
            heartbeat_every_secs: if s.heartbeat_every_secs > 0 {
                s.heartbeat_every_secs
            } else {
                WatchLinkConfig::default().heartbeat_every_secs
            },
            ..WatchLinkConfig::default()
        };
        let config = WatchConfig {
            dir: s.dir.into(),
            secret: to32(&s.secret)?,
            driver,
            counters,
            route,
            relay,
            link,
        };
        let (orders_tx, orders_rx) = channel();
        let (ready_tx, ready_rx) = channel();
        let thread = std::thread::Builder::new()
            .name("openbmt-owner-watch".to_string())
            .spawn(move || run_thread(config, ready_tx, orders_rx))
            .map_err(|e| internal(format!("the watcher's thread did not start: {e}")))?;
        let ready = ready_rx
            .recv()
            .unwrap_or_else(|_| Err(internal("the watcher's thread ended before it started")));
        if let Err(e) = ready {
            let _ = thread.join();
            return Err(e);
        }
        Ok(Self {
            orders: Mutex::new(Some(orders_tx)),
            thread: Mutex::new(Some(thread)),
        })
    }

    /// One turn at `now`: take the driver's events for at most `wait_ms`,
    /// then let the link send what is due.
    pub(crate) fn step(&self, now: u64, wait_ms: u32) -> Result<WatchStepView, AppError> {
        let sender = self
            .orders
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
            .ok_or_else(|| wrong_state("watch-stopped", "the watcher is stopped"))?;
        let (tx, rx) = channel();
        sender
            .send(Order::Step(now, wait_ms, tx))
            .map_err(|_| wrong_state("watch-stopped", "the watcher is stopped"))?;
        rx.recv()
            .map_err(|_| internal("the watcher failed in a way it did not expect"))
    }

    /// Stop the driver and let go of the folder.
    pub(crate) fn stop(&self) -> Result<(), AppError> {
        let sender = self.orders.lock().unwrap_or_else(|e| e.into_inner()).take();
        if let Some(sender) = sender {
            let (tx, rx) = channel();
            if sender.send(Order::Stop(tx)).is_ok() {
                let _ = rx.recv();
            }
        }
        if let Some(thread) = self.thread.lock().unwrap_or_else(|e| e.into_inner()).take() {
            let _ = thread.join();
        }
        Ok(())
    }
}
