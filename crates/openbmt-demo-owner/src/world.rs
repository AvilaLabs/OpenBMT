//! The pretend chain the demo owner's node reads: the drivers' place. The
//! node is given ports whose events this module sends (a chain of real
//! regtest blocks, the transactions a customer broadcasts, the peers that say
//! the chain is healthy), and the page moves the chain by calling [`pay`] and
//! [`mine`]. The chain is the one of the demo counter's module
//! (`openbmt_demo::chain`), and is moved by the same calls at the same times,
//! so that the two phones see the same blocks.

use std::str::FromStr;
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use bitcoin::consensus::encode::serialize;
use bitcoin::hashes::{sha256, Hash};
use bitcoin::{Address, BlockHash, Network, ScriptBuf, Txid};
use openbmt_counter_node::DriverPort;
use openbmt_demo::chain::{payment, SimChain};
use openbmt_net_io::{AppEvent, EventReceiver};
use openbmt_owner_ffi::HostSpawn;
use openbmt_owner_node::OwnerPort;
use openbmt_watch::{HeaderChain, Peer, PeerAddr, PeerTransport};

/// The network of the demo.
pub const NETWORK: Network = Network::Regtest;

struct World {
    chain: SimChain,
    events: Sender<AppEvent>,
    watched: Vec<ScriptBuf>,
    paid: u32,
}

static WORLD: Mutex<Option<World>> = Mutex::new(None);

fn world() -> MutexGuard<'static, Option<World>> {
    WORLD.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Three peers in three network groups that serve filters, at the tip: what
/// the health rules need to say the chain is healthy.
fn peers_event(height: u32, time: u64) -> AppEvent {
    let peers = (20..23)
        .map(|n| Peer {
            addr: PeerAddr::Ip(std::net::IpAddr::from([n, n, 1, 1])),
            serves_filters: true,
            best_height: height,
            transport: PeerTransport::V1,
        })
        .collect();
    AppEvent::Peers(peers, None, (height, time))
}

/// The port of the node on the simulated chain. Nothing the owner sends
/// (a payment, a rescan) leaves the page: the demo owner only watches.
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

impl OwnerPort for PagePort {
    fn broadcast(&self, _tx: Vec<u8>) {}

    fn forget(&self, _txid: Txid) {}

    fn unwatch_scripts(&self, scripts: Vec<ScriptBuf>) {
        if let Some(w) = world().as_mut() {
            w.watched.retain(|s| !scripts.contains(s));
        }
    }

    fn rescan_from(&self, _height: u32) {}

    fn set_fetch_transactions(&self, _on: bool) {}

    fn fetch_block(&self, _hash: BlockHash) {}
}

/// What a start of the node gets as its driver: a port on a new chain that
/// holds the genesis block, with healthy peers.
pub fn host() -> HostSpawn {
    Arc::new(|_driver, _dir, scripts| {
        let now = crate::now();
        let (tx, rx) = channel();
        tx.send(peers_event(0, now))
            .map_err(|_| "the events are closed".to_string())?;
        *world() = Some(World {
            chain: SimChain::on_genesis(NETWORK, now),
            events: tx,
            watched: scripts,
            paid: 0,
        });
        Ok(Box::new(PagePort { events: rx.into() }) as Box<dyn OwnerPort + Send>)
    })
}

/// A customer pays `sats` to `address`: a transaction is broadcast into the
/// pool, and the node is told of it as of a peer's announcement. Returns its
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
    w.chain.broadcast(tx.clone());
    w.events
        .send(AppEvent::Sighting {
            tx: serialize(&tx),
            from: vec![PeerAddr::Ip(std::net::IpAddr::from([20, 20, 1, 1]))],
            own: false,
        })
        .map_err(|_| "the node is not reading".to_string())?;
    Ok(txid.to_string())
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

#[cfg(test)]
mod tests;
