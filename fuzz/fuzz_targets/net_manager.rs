#![no_main]

use std::net::{IpAddr, Ipv4Addr};

use bitcoin::hashes::{sha256d, Hash};
use bitcoin::p2p::address::Address;
use bitcoin::p2p::message::NetworkMessage;
use bitcoin::p2p::message_network::VersionMessage;
use bitcoin::p2p::{Magic, ServiceFlags};
use bitcoin::{Network, ScriptBuf};
use libfuzzer_sys::fuzz_target;
use openbmt_net::{encode, Manager, ManagerConfig, Output, PeerId, MAX_OUTGOING_BYTES};
use openbmt_watch::{HeaderStore, PeerAddr};

const PEERS: u8 = 3;
const COMMANDS: [&str; 8] = [
    "headers", "inv", "block", "tx", "cfilter", "notfound", "addr", "ping",
];

/// The peer's `version` and `verack`, with the services of a full node that
/// serves filters.
fn handshake(start_height: i32) -> Vec<u8> {
    let silent = Address {
        services: ServiceFlags::NONE,
        address: [0; 8],
        port: 0,
    };
    let version = VersionMessage {
        version: 70_016,
        services: ServiceFlags::NETWORK | ServiceFlags::WITNESS | ServiceFlags::COMPACT_FILTERS,
        timestamp: 1_700_000_000,
        receiver: silent.clone(),
        sender: silent,
        nonce: 42,
        user_agent: String::new(),
        start_height,
        relay: true,
    };
    let magic = Magic::from(Network::Regtest);
    let mut out = encode(magic, &NetworkMessage::Version(version));
    out.extend(encode(magic, &NetworkMessage::Verack));
    out
}

/// A message of the given command with a right header and checksum around
/// the payload, so that the payload reaches the code that reads it.
fn framed(command: &str, payload: &[u8]) -> Vec<u8> {
    let mut field = [0u8; 12];
    for (slot, byte) in field.iter_mut().zip(command.bytes()) {
        *slot = byte;
    }
    let sum = sha256d::Hash::hash(payload).to_byte_array();
    let mut out = Magic::from(Network::Regtest).to_bytes().to_vec();
    out.extend_from_slice(&field);
    out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    out.extend_from_slice(&sum[..4]);
    out.extend_from_slice(payload);
    out
}

fn take(data: &[u8], at: &mut usize) -> Option<u8> {
    let byte = data.get(*at).copied();
    *at += 1;
    byte
}

fuzz_target!(|data: &[u8]| {
    // Steps: a byte whose low 2 bits choose connect, receive, tick or close
    // and whose next 2 bits choose the peer; bit 4 puts a good handshake
    // before the bytes received and bit 5 frames them as a command that the
    // next byte chooses. The first byte sets the manager up.
    let mut at = 0;
    let Some(setup) = take(data, &mut at) else {
        return;
    };
    let config = ManagerConfig {
        network: Network::Regtest,
        target_peers: usize::from(PEERS),
        watch_from: u32::from(setup >> 4),
        allow_local: false,
    };
    let Ok(store) = HeaderStore::new(Network::Regtest) else {
        return;
    };
    let mut manager = Manager::new(config, store);
    if setup & 1 == 1 {
        manager.set_all_blocks(true);
    }
    if setup & 2 == 2 {
        manager.watch_script(ScriptBuf::from_bytes(vec![0x51]));
    }
    if setup & 4 == 4 {
        manager.set_fetch_transactions(false);
    }
    let mut now = 1_700_000_000u64;
    while let Some(step) = take(data, &mut at) {
        let id = PeerId(u64::from((step >> 2) % PEERS));
        let mut outputs = Vec::new();
        match step & 3 {
            0 => {
                let n = ((step >> 2) % PEERS) + 1;
                let addr = PeerAddr::Ip(IpAddr::V4(Ipv4Addr::new(20 + n, 1, 0, 1)));
                let _ = manager.connected(id, addr, 0x1000 + u64::from(n), now);
            }
            1 => {
                let Some(len) = take(data, &mut at) else {
                    break;
                };
                let end = data.len().min(at + usize::from(len) * 4);
                let mut payload = data.get(at..end).unwrap_or(&[]).to_vec();
                at = end;
                if step & 0x20 != 0 {
                    let pick = payload.first().copied().unwrap_or(0);
                    let command = COMMANDS[usize::from(pick) % COMMANDS.len()];
                    payload = framed(command, payload.get(1..).unwrap_or(&[]));
                }
                if step & 0x10 != 0 {
                    let mut bytes = handshake(i32::from(len));
                    bytes.extend(payload);
                    payload = bytes;
                }
                outputs = manager.received(id, &payload, now);
            }
            2 => {
                let Some(secs) = take(data, &mut at) else {
                    break;
                };
                now = now.saturating_add(u64::from(secs) * 4);
                outputs = manager.tick(now);
            }
            _ => outputs = manager.closed(id, now),
        }
        for output in &outputs {
            if let Output::Disconnect { reason, .. } = output {
                assert!(!reason.is_empty());
            }
        }
        // What waits to be sent never passes the cap.
        for k in 0..PEERS {
            assert!(manager.outgoing(PeerId(u64::from(k))).len() <= MAX_OUTGOING_BYTES);
        }
    }
});
