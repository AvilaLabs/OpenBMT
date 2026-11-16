//! The pretend chain the demo counter's node reads: the driver's place. The
//! node is given a port whose events this module sends (a chain of real
//! regtest blocks, the transactions a customer broadcasts, the peers that
//! say the chain is healthy), and the page moves the chain by calling
//! [`pay`] and [`mine`]. The chain is the one of the books demo
//! (`openbmt_demo::chain`).

use std::collections::HashMap;
use std::str::FromStr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use bitcoin::consensus::encode::serialize;
use bitcoin::hashes::{sha256, Hash};
use bitcoin::{Address, Network, ScriptBuf, Txid};
use openbmt_core::heartbeat::WatcherHeartbeat;
use openbmt_counter_ffi::HostSpawn;
use openbmt_counter_node::{DriverPort, Launch};
use openbmt_demo::chain::{payment, SimChain};
use openbmt_net_io::{AppEvent, EventReceiver};
use openbmt_watch::{HeaderChain, Peer, PeerAddr, PeerTransport};

/// The network of the demo.
pub const NETWORK: Network = Network::Regtest;

struct World {
    chain: SimChain,
    events: Sender<AppEvent>,
    watched: Vec<ScriptBuf>,
    paid: u32,
    /// The coin each payment spends, by the payment's id: the same coin is
    /// spent again by a replacement.
    coins: HashMap<Txid, [u8; 32]>,
}

/// The heartbeats made so far, over every chain this module has held: the
/// counter keeps the last number it took, so a new chain goes on counting.
static BEATS: AtomicU64 = AtomicU64::new(0);

static WORLD: Mutex<Option<World>> = Mutex::new(None);

fn world() -> MutexGuard<'static, Option<World>> {
    WORLD.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The first number of each of the three peers' addresses: three network
/// groups.
const PEER_GROUPS: [u8; 3] = [20, 21, 22];

/// Three peers in three network groups that serve filters, at the tip: what
/// the health rules need to say the chain is healthy.
fn peers_event(height: u32, time: u64) -> AppEvent {
    let peers = PEER_GROUPS
        .into_iter()
        .map(|n| Peer {
            addr: PeerAddr::Ip(std::net::IpAddr::from([n, n, 1, 1])),
            serves_filters: true,
            best_height: height,
            transport: PeerTransport::V1,
        })
        .collect();
    AppEvent::Peers(peers, None, (height, time))
}

/// The port of the node on the simulated chain.
struct PagePort {
    events: EventReceiver,
}

impl DriverPort for PagePort {
    fn events(&self) -> &EventReceiver {
        &self.events
    }

    fn watch_script(&self, script: ScriptBuf) {
        if let Some(w) = world().as_mut() {
            if !w.watched.contains(&script) {
                w.watched.push(script);
            }
        }
    }

    fn set_all_blocks(&self, _on: bool) {}

    fn shutdown(&mut self) {
        *world() = None;
    }
}

/// What a start of the node gets as its driver: a port on a new chain that
/// holds the genesis block, with healthy peers.
pub fn host() -> HostSpawn {
    Arc::new(|launch: Launch| {
        let now = crate::now();
        crate::lightning::reset();
        let (tx, rx) = channel();
        tx.send(peers_event(0, now))
            .map_err(|_| "the events are closed".to_string())?;
        *world() = Some(World {
            chain: SimChain::on_genesis(NETWORK, now),
            events: tx,
            watched: launch.scripts,
            paid: 0,
            coins: HashMap::new(),
        });
        Ok(Box::new(PagePort { events: rx.into() }) as Box<dyn DriverPort + Send>)
    })
}

/// A customer pays `sats` to `address`: a transaction is broadcast into the
/// pool, and the node is told of it as of each peer's announcement. Returns its
/// id, as hex.
pub fn pay(address: &str, sats: u64) -> Result<String, String> {
    let script = Address::from_str(address)
        .map_err(|e| format!("the address does not read: {e}"))?
        .require_network(NETWORK)
        .map_err(|e| format!("the address is for another network: {e}"))?
        .script_pubkey();
    let mut guard = world();
    let w = guard.as_mut().ok_or("no node is running")?;
    w.paid += 1;
    let coin = sha256::Hash::hash(&w.paid.to_le_bytes()).to_byte_array();
    let tx = payment(coin, &script, sats);
    let txid = tx.compute_txid();
    w.coins.insert(txid, coin);
    w.chain.broadcast(tx.clone());
    announce(w, &tx)?;
    Ok(txid.to_string())
}

/// The light check counts a payment seen when peers of three network groups
/// have announced it (WR-44): each of the three peers does.
fn announce(w: &World, tx: &bitcoin::Transaction) -> Result<(), String> {
    for n in PEER_GROUPS {
        w.events
            .send(AppEvent::Sighting {
                tx: serialize(tx),
                from: vec![PeerAddr::Ip(std::net::IpAddr::from([n, n, 1, 1]))],
                own: false,
            })
            .map_err(|_| "the node is not reading".to_string())?;
    }
    Ok(())
}

/// The script of the pretend customer's own address, to which a cancelled
/// payment's coin goes back: not the shop's.
fn customer_script() -> ScriptBuf {
    ScriptBuf::new_p2wsh(&bitcoin::WScriptHash::from_byte_array([0xc5; 32]))
}

/// The customer takes a payment back: the payment `txid` is replaced by a
/// payment of the same coin, `sats_to_other` to an address of the customer's
/// own. The first leaves the mempool, the replacement is broadcast and
/// announced as `pay` announces. Returns the replacement's id, as hex.
pub fn replace(txid: &str, sats_to_other: u64) -> Result<String, String> {
    let id = Txid::from_str(txid).map_err(|e| format!("the payment's id does not read: {e}"))?;
    let mut guard = world();
    let w = guard.as_mut().ok_or("no node is running")?;
    let coin = *w.coins.get(&id).ok_or("no such payment was made")?;
    let tx = payment(coin, &customer_script(), sats_to_other);
    let replacement = tx.compute_txid();
    if !w.chain.remove(&id) {
        return Err("the payment is no longer waiting".to_string());
    }
    w.coins.insert(replacement, coin);
    w.chain.broadcast(tx.clone());
    announce(w, &tx)?;
    Ok(replacement.to_string())
}

/// The heartbeat the owner's phone would send the counter at `now`: signed
/// with the owner key of the demo, healthy, at the chain's tip, and numbered
/// above the last one made. The light check waits for one (CR-50).
pub fn heartbeat(now: u64) -> Result<Vec<u8>, String> {
    let mut guard = world();
    let w = guard.as_mut().ok_or("no node is running")?;
    let seq = BEATS.fetch_add(1, Ordering::SeqCst) + 1;
    let counter = openbmt_demo::wallet::counter()?;
    let owner = openbmt_demo::wallet::owner()?;
    let beat = WatcherHeartbeat {
        counter: counter.device_id(),
        seq,
        time: now,
        tip_height: w.chain.tip_height(),
        tip_hash: w.chain.tip_hash().to_byte_array(),
        healthy: true,
    };
    Ok(beat.sign(&owner).to_bytes())
}

/// A block is mined of everything in the pool, at the page's time: the node is
/// told the chain grew, is handed the block and is told the new tip. Returns
/// the new height.
pub fn mine() -> Result<u32, String> {
    let mut guard = world();
    let w = guard.as_mut().ok_or("no node is running")?;
    let block = w.chain.mine(crate::now());
    let height = w.chain.tip_height();
    let hash = block.block_hash();
    let sent = [
        AppEvent::Chain {
            joined: vec![(height, hash)],
            left: vec![],
        },
        AppEvent::Block {
            hash,
            bytes: serialize(&block),
        },
        AppEvent::Checked(height),
        peers_event(height, w.chain.tip_time()),
    ];
    for event in sent {
        w.events
            .send(event)
            .map_err(|_| "the node is not reading".to_string())?;
    }
    Ok(height)
}

/// The hash of the pretend chain's tip, as text: the page's check that this
/// chain and the other module's chain are the same one.
pub fn tip() -> Result<String, String> {
    let guard = world();
    let w = guard.as_ref().ok_or("no node is running")?;
    Ok(w.chain.tip_hash().to_string())
}

#[cfg(test)]
mod tests;
