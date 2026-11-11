//! The peer's side of the encrypted transport, for the tests: the
//! responder of the BIP324 handshake, written with the `bip324` crate, and
//! the [`Wire`] that lets the session and manager tests run over either
//! transport.
//!
//! The kit's functions (`session`, `frame`, `outgoing`, `Net::join`, ...)
//! look at the thread's [`mode`]. A test that should also run over the
//! encrypted transport is run again inside [`in_v2`], with no copy of its
//! body. Only the kit may take bytes from a session of such a test: bytes
//! that a test discards with `take_outgoing` never reach the responder's
//! cipher, and what the session writes after them cannot be read.

use std::cell::{Cell, RefCell};

use bip324::{
    CipherSession, FillBytes, GarbageResult, Handshake, PacketType, ReceivedKey, Role, SentVersion,
    VersionResult, NUM_LENGTH_BYTES,
};
use bitcoin::consensus::encode::deserialize_partial;
use bitcoin::p2p::message::{NetworkMessage, RawNetworkMessage};
use bitcoin::secp256k1::Secp256k1;

use super::kit::{magic, raw_frame};
use crate::v2::{contents_of, short_name};

/// Which transport the kit speaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// The old transport.
    V1,
    /// The encrypted transport.
    V2,
}

thread_local! {
    static MODE: Cell<Mode> = const { Cell::new(Mode::V1) };
    /// The peer's side of the thread's one session.
    pub static SESSION_WIRE: RefCell<Wire> = RefCell::new(Wire::v1());
}

/// The transport the kit speaks on this thread.
pub fn mode() -> Mode {
    MODE.with(Cell::get)
}

/// Run a test body with the kit on the encrypted transport.
pub fn in_v2(body: impl FnOnce()) {
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            MODE.with(|m| m.set(Mode::V1));
            SESSION_WIRE.with(|w| *w.borrow_mut() = Wire::v1());
        }
    }
    MODE.with(|m| m.set(Mode::V2));
    let _reset = Reset;
    body();
}

/// The two values the key exchange asks its random source for.
struct Fixed {
    secret: [u8; 32],
    aux: [u8; 32],
    drawn: u8,
}

impl FillBytes for Fixed {
    fn fill_bytes(&mut self, dest: &mut [u8; 32]) {
        *dest = if self.drawn == 0 {
            self.secret
        } else {
            self.aux
        };
        self.drawn += 1;
    }
}

/// What a packet held.
pub struct Plain {
    pub decoy: bool,
    /// The contents: the message type and the payload.
    pub contents: Vec<u8>,
}

/// The responder of the handshake, and the cipher after it.
pub struct Peer2 {
    secret: [u8; 32],
    aux: [u8; 32],
    garbage: Vec<u8>,
    decoys: Vec<Vec<u8>>,
    sent: Option<Handshake<SentVersion>>,
    cipher: Option<CipherSession>,
    inbuf: Vec<u8>,
}

impl Peer2 {
    /// A responder whose key comes from this number.
    pub fn new(seed: u8) -> Self {
        Self {
            secret: [seed.max(1); 32],
            aux: [seed ^ 0x5a; 32],
            garbage: Vec::new(),
            decoys: Vec::new(),
            sent: None,
            cipher: None,
            inbuf: Vec::new(),
        }
    }

    /// Garbage of this many bytes after the key.
    pub fn with_garbage(mut self, bytes: usize) -> Self {
        self.garbage = (0..bytes).map(|i| (i % 251) as u8).collect();
        self
    }

    /// These decoy packets before the version packet.
    pub fn with_decoys(mut self, decoys: Vec<Vec<u8>>) -> Self {
        self.decoys = decoys;
        self
    }

    /// The responder's first flight, given the first 64 bytes of the
    /// initiator: its key and garbage, then its terminator, its decoys and
    /// its version packet.
    pub fn answer(&mut self, ours: &[u8]) -> Vec<u8> {
        let their: [u8; 64] = ours[..64].try_into().unwrap();
        let mut feed = Fixed {
            secret: self.secret,
            aux: self.aux,
            drawn: 0,
        };
        let curve = Secp256k1::new();
        let h = Handshake::new_with_rng(magic().to_bytes(), Role::Responder, &mut feed, &curve)
            .unwrap();
        let mut out = vec![0u8; 64 + self.garbage.len()];
        let h = h.send_key(Some(&self.garbage), &mut out).unwrap();
        let h = h.receive_key(their).unwrap();
        let decoys: Vec<&[u8]> = self.decoys.iter().map(Vec::as_slice).collect();
        let mut tail = vec![0u8; Handshake::<ReceivedKey<'_>>::send_version_len(Some(&decoys))];
        let h = h.send_version(&mut tail, Some(&decoys)).unwrap();
        self.sent = Some(h);
        out.extend_from_slice(&tail);
        out
    }

    /// The session ID, once the initiator's version packet has been read.
    pub fn session_id(&self) -> Option<[u8; 32]> {
        self.cipher.as_ref().map(|c| c.id().to_bytes())
    }

    /// Bytes from the initiator, read as packets. Before the cipher exists
    /// this reads the initiator's terminator and version packet.
    pub fn hear(&mut self, bytes: &[u8]) -> Vec<Plain> {
        self.inbuf.extend_from_slice(bytes);
        if self.cipher.is_none() {
            let h = self.sent.take().expect("answer comes first");
            let buf = self.inbuf.clone();
            match h.receive_garbage(&buf).unwrap() {
                GarbageResult::NeedMoreData(h) => {
                    self.sent = Some(h);
                    return Vec::new();
                }
                GarbageResult::FoundGarbage {
                    mut handshake,
                    consumed_bytes,
                } => {
                    let mut at = consumed_bytes;
                    loop {
                        let head: [u8; 3] = buf[at..at + 3].try_into().unwrap();
                        let rest = handshake.decrypt_packet_len(head).unwrap();
                        let mut packet = buf[at + 3..at + 3 + rest].to_vec();
                        at += 3 + rest;
                        match handshake.receive_version(&mut packet).unwrap() {
                            VersionResult::Complete { cipher } => {
                                self.cipher = Some(cipher);
                                break;
                            }
                            VersionResult::Decoy(next) => handshake = next,
                        }
                    }
                    self.inbuf.drain(..at);
                }
            }
        }
        let cipher = self.cipher.as_mut().unwrap();
        let mut out = Vec::new();
        while let Some(head) = self.inbuf.first_chunk::<NUM_LENGTH_BYTES>().copied() {
            // Reading the length changes the cipher: only when the whole
            // packet is there is it read, and then once.
            let mut probe = cipher.clone();
            let rest = probe.inbound().decrypt_packet_len(head);
            if self.inbuf.len() < 3 + rest {
                break;
            }
            *cipher = probe;
            let mut packet = self.inbuf[3..3 + rest].to_vec();
            let (kind, plain) = cipher
                .inbound()
                .decrypt_in_place(&mut packet, None)
                .unwrap();
            out.push(Plain {
                decoy: kind == PacketType::Decoy,
                contents: plain[1..].to_vec(),
            });
            self.inbuf.drain(..3 + rest);
        }
        out
    }

    /// One packet from the responder with these contents.
    pub fn say(&mut self, contents: &[u8], decoy: bool) -> Vec<u8> {
        let cipher = self.cipher.as_mut().expect("the handshake is through");
        let kind = if decoy {
            PacketType::Decoy
        } else {
            PacketType::Genuine
        };
        let mut packet = vec![0u8; contents.len() + 20];
        cipher
            .outbound()
            .encrypt(contents, &mut packet, kind, None)
            .unwrap();
        packet
    }
}

/// A message from a packet's contents.
pub fn message_of(contents: &[u8]) -> NetworkMessage {
    let (name, payload) = if contents[0] == 0 {
        let field = &contents[1..13];
        let end = field.iter().position(|b| *b == 0).unwrap_or(12);
        (
            String::from_utf8(field[..end].to_vec()).unwrap(),
            &contents[13..],
        )
    } else {
        (
            short_name(contents[0]).expect("a known ID").to_string(),
            &contents[1..],
        )
    };
    let bytes = raw_frame(&name, payload);
    let (raw, used): (RawNetworkMessage, usize) = deserialize_partial(&bytes).unwrap();
    assert_eq!(used, bytes.len());
    raw.into_payload()
}

/// The peer's side of one connection, on the transport of the kit's mode.
pub struct Wire {
    peer: Option<Peer2>,
    /// Messages read from the session already, not yet taken by a test.
    pub backlog: Vec<NetworkMessage>,
}

impl Wire {
    /// The old transport.
    pub fn v1() -> Self {
        Self {
            peer: None,
            backlog: Vec::new(),
        }
    }

    /// The encrypted transport, with a responder whose key comes from this
    /// number.
    pub fn v2(seed: u8) -> Self {
        Self {
            peer: Some(Peer2::new(seed)),
            backlog: Vec::new(),
        }
    }

    /// A wire for the kit's mode.
    pub fn for_mode(seed: u8) -> Self {
        match mode() {
            Mode::V1 => Self::v1(),
            Mode::V2 => Self::v2(seed),
        }
    }

    /// The responder, for tests of the transport itself.
    pub fn peer(&mut self) -> &mut Peer2 {
        self.peer.as_mut().expect("the encrypted transport")
    }

    /// The bytes of a message from the peer.
    pub fn frame(&mut self, message: NetworkMessage) -> Vec<u8> {
        match &mut self.peer {
            None => crate::encode(magic(), &message),
            Some(peer) => peer.say(&contents_of(&message), false),
        }
    }

    /// What the session wrote, as messages. On the encrypted transport the
    /// first call has to follow `answer`.
    pub fn decode(&mut self, bytes: &[u8]) -> Vec<NetworkMessage> {
        match &mut self.peer {
            None => super::kit::decode_all(bytes),
            Some(peer) => peer
                .hear(bytes)
                .into_iter()
                .filter(|p| !p.decoy)
                .map(|p| message_of(&p.contents))
                .collect(),
        }
    }

    /// The encrypted handshake up to the point where the session has sent
    /// its `version`: the local key is the session's first 64 bytes; the reply is
    /// fed back by the caller. Returns the reply.
    pub fn answer(&mut self, ours: &[u8]) -> Vec<u8> {
        self.peer().answer(ours)
    }
}
