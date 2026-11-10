//! The peer's side of a connection, for the tests: messages written with
//! the bitcoin crate, and the counter's output read back with it.

use bitcoin::absolute::LockTime;
use bitcoin::block::{Header, Version as BlockVersion};
use bitcoin::consensus::encode::{deserialize_partial, serialize};
use bitcoin::hashes::{sha256d, Hash};
use bitcoin::p2p::address::Address;
use bitcoin::p2p::message::{NetworkMessage, RawNetworkMessage};
use bitcoin::p2p::message_network::VersionMessage;
use bitcoin::p2p::{Magic, ServiceFlags};
use bitcoin::transaction::Version as TxVersion;
use bitcoin::{
    Amount, BlockHash, CompactTarget, Network, OutPoint, ScriptBuf, Sequence, Transaction, TxIn,
    TxMerkleNode, TxOut, Txid, Witness,
};

use super::v2kit::{mode, Mode, Wire, SESSION_WIRE};
use crate::{Event, FrameReader, Session, SessionConfig, V2Entropy};

/// The counter's nonce in the tests.
pub const OUR_NONCE: u64 = 0x1122_3344_5566_7788;
/// The time the session starts.
pub const START: u64 = 1_000;

pub fn magic() -> Magic {
    Magic::from(Network::Regtest)
}

pub fn config() -> SessionConfig {
    SessionConfig {
        network: Network::Regtest,
        nonce: OUR_NONCE,
        tip_height: 800_000,
    }
}

/// A session on the transport of the kit's mode. On the encrypted one the
/// handshake is through, and the session's `version` is the first thing
/// [`outgoing`] returns.
pub fn session() -> Session {
    match mode() {
        Mode::V1 => {
            SESSION_WIRE.with(|w| *w.borrow_mut() = Wire::v1());
            Session::new(config(), START)
        }
        Mode::V2 => {
            let mut s = Session::new_v2(config(), START, V2Entropy::new([7; 32], [9; 32])).unwrap();
            let mut wire = Wire::v2(1);
            let reply = wire.answer(&s.take_outgoing());
            assert!(s.receive(&reply, START).unwrap().is_empty());
            let first = s.take_outgoing();
            wire.backlog = wire.decode(&first);
            SESSION_WIRE.with(|w| *w.borrow_mut() = wire);
            s
        }
    }
}

/// A reader that has seen the handshake, so that every command is read.
pub fn open_reader() -> FrameReader {
    let mut reader = FrameReader::new(magic());
    reader.set_handshake_done();
    reader
}

/// The services of a good full node.
pub fn full_node() -> ServiceFlags {
    ServiceFlags::NETWORK | ServiceFlags::WITNESS
}

/// A `version` from a good peer.
pub fn peer_version() -> VersionMessage {
    let silent = Address {
        services: ServiceFlags::NONE,
        address: [0; 8],
        port: 0,
    };
    VersionMessage {
        version: 70_016,
        services: full_node(),
        timestamp: 1_700_000_000,
        receiver: silent.clone(),
        sender: silent,
        nonce: 42,
        user_agent: "/Satoshi:28.0.0/".to_string(),
        start_height: 850_000,
        relay: true,
    }
}

/// A message from the peer, in bytes, on the transport of the kit's mode.
pub fn frame(message: NetworkMessage) -> Vec<u8> {
    SESSION_WIRE.with(|w| w.borrow_mut().frame(message))
}

pub fn version_frame() -> Vec<u8> {
    frame(NetworkMessage::Version(peer_version()))
}

pub fn verack_frame() -> Vec<u8> {
    frame(NetworkMessage::Verack)
}

/// The double-SHA256 checksum of a payload.
pub fn checksum(payload: &[u8]) -> [u8; 4] {
    let [a, b, c, d, ..] = sha256d::Hash::hash(payload).to_byte_array();
    [a, b, c, d]
}

/// A header written by hand, with any length and checksum.
pub fn raw_header(magic: [u8; 4], command: &[u8; 12], len: u32, checksum: [u8; 4]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&magic);
    out.extend_from_slice(command);
    out.extend_from_slice(&len.to_le_bytes());
    out.extend_from_slice(&checksum);
    out
}

/// A command name padded with NULs.
pub fn command_field(name: &str) -> [u8; 12] {
    let mut field = [0u8; 12];
    for (slot, byte) in field.iter_mut().zip(name.bytes()) {
        *slot = byte;
    }
    field
}

/// A whole message written by hand, with a right checksum.
pub fn raw_frame(command: &str, payload: &[u8]) -> Vec<u8> {
    let mut out = raw_header(
        magic().to_bytes(),
        &command_field(command),
        u32::try_from(payload.len()).unwrap(),
        checksum(payload),
    );
    out.extend_from_slice(payload);
    out
}

/// Only the 24-byte header of a message whose payload would have `len`
/// bytes.
pub fn header_only(command: &str, len: u32) -> Vec<u8> {
    raw_header(magic().to_bytes(), &command_field(command), len, [0; 4])
}

/// What the counter wrote, read back as messages.
pub fn decode_all(mut bytes: &[u8]) -> Vec<NetworkMessage> {
    let mut out = Vec::new();
    while !bytes.is_empty() {
        let (raw, used): (RawNetworkMessage, usize) = deserialize_partial(bytes).unwrap();
        assert_eq!(raw.magic(), &magic());
        out.push(raw.into_payload());
        bytes = &bytes[used..];
    }
    out
}

/// Take what the session queued, as messages.
pub fn outgoing(s: &mut Session) -> Vec<NetworkMessage> {
    SESSION_WIRE.with(|w| {
        let mut wire = w.borrow_mut();
        let mut out = std::mem::take(&mut wire.backlog);
        out.extend(wire.decode(&s.take_outgoing()));
        out
    })
}

/// A session that has finished the handshake; its output is taken.
pub fn ready_session() -> Session {
    let mut s = session();
    let mut stream = version_frame();
    stream.extend(verack_frame());
    let events = s.receive(&stream, START).unwrap();
    assert!(matches!(events[..], [Event::Ready { .. }]));
    // What the handshake wrote is read and dropped.
    outgoing(&mut s);
    s
}

/// A header for the tests. Nothing here checks its proof of work.
pub fn header(n: u32) -> Header {
    Header {
        version: BlockVersion::from_consensus(0x2000_0000),
        prev_blockhash: BlockHash::from_byte_array([n as u8; 32]),
        merkle_root: TxMerkleNode::from_byte_array([7; 32]),
        time: 1_700_000_000 + n,
        bits: CompactTarget::from_consensus(0x207f_ffff),
        nonce: n,
    }
}

/// The payload of a message, without the 24-byte header.
pub fn payload_of(message: &NetworkMessage) -> Vec<u8> {
    serialize(message)
}

/// A small transaction with a witness.
pub fn simple_tx() -> Transaction {
    let mut witness = Witness::new();
    witness.push([1u8; 64]);
    Transaction {
        version: TxVersion::TWO,
        lock_time: LockTime::ZERO,
        input: vec![TxIn {
            previous_output: OutPoint::new(Txid::from_byte_array([3; 32]), 1),
            script_sig: ScriptBuf::new(),
            sequence: Sequence::MAX,
            witness,
        }],
        output: vec![TxOut {
            value: Amount::from_sat(50_000),
            script_pubkey: ScriptBuf::from_bytes(vec![0x51]),
        }],
    }
}
