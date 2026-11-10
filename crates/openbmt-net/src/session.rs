//! One peer, from the first `version` to the last byte (NR-05, NR-11 to
//! NR-14).
//!
//! A [`Session`] has no socket and no clock. The caller feeds it the bytes
//! received and the time in seconds, takes out the bytes to send, and reads
//! the [`Event`]s. Any error is a [`Drop`](crate::Drop): the connection must
//! be closed, and every later call returns the same drop.

use bitcoin::block::Header;
use bitcoin::hashes::Hash;
use bitcoin::p2p::address::Address;
use bitcoin::p2p::message::NetworkMessage;
use bitcoin::p2p::message_blockdata::{GetHeadersMessage, Inventory};
use bitcoin::p2p::message_filter::GetCFilters;
use bitcoin::p2p::message_network::VersionMessage;
use bitcoin::p2p::{Magic, ServiceFlags};
use bitcoin::{BlockHash, Network, Txid};

use crate::frame::{encode, Message, PeerAddress};
use crate::{
    Drop, FrameReader, HANDSHAKE_SECS, MAX_OUTGOING_BYTES, MAX_USER_AGENT, PING_SECS,
    PROTOCOL_VERSION, PUSH_SLACK,
};

/// What a session needs to start. The caller supplies the nonce.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionConfig {
    /// The network the counter is on.
    pub network: Network,
    /// A fresh random nonce, for `version` and for the pings.
    pub nonce: u64,
    /// The height of the counter's best chain.
    pub tip_height: u32,
}

/// What the counter asks of a peer after the handshake. None of them sends
/// a transaction (NR-55).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Request {
    /// `getheaders` from the locator, with no stop hash (NR-30).
    GetHeaders {
        /// Block hashes, newest first.
        locator: Vec<BlockHash>,
    },
    /// `getdata` for these transactions, with witness data (NR-51).
    GetTx(Vec<Txid>),
    /// `getdata` for one block, with witness data (NR-45).
    GetBlock(BlockHash),
    /// `getcfilters` of type 0 for one block (NR-40).
    GetFilter {
        /// The block's height.
        height: u32,
        /// The block's hash.
        block: BlockHash,
    },
    /// `getaddr` (NR-22).
    GetAddr,
}

/// What a good peer produced. Nothing in an event is believed: the watcher
/// checks it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// NR-12, NR-13: both sides have sent `verack`. The services and the
    /// start height are the peer's claims.
    Ready {
        /// The peer's services.
        services: ServiceFlags,
        /// The peer's start height; a negative one is 0.
        start_height: u32,
    },
    /// Headers, at most 2,000 (NR-30).
    Headers(Vec<Header>),
    /// Transactions and blocks the peer says it has (NR-50).
    Announced {
        /// Transactions, by txid.
        txids: Vec<Txid>,
        /// Blocks.
        blocks: Vec<BlockHash>,
    },
    /// A transaction as the bytes the peer sent, checked to be one
    /// transaction and nothing more.
    Tx(Vec<u8>),
    /// A block as the bytes the peer sent, not parsed here.
    Block(Vec<u8>),
    /// A basic compact filter (NR-40).
    Filter {
        /// The block the filter is for.
        block: BlockHash,
        /// The filter bytes.
        filter: Vec<u8>,
    },
    /// What the peer does not have of what was asked.
    NotFound {
        /// Transactions, by txid.
        txids: Vec<Txid>,
        /// Blocks.
        blocks: Vec<BlockHash>,
    },
    /// Addresses the peer told of, at most 1,000 (NR-22).
    Addresses(Vec<PeerAddress>),
}

/// One peer connection, as bytes and events.
#[derive(Debug)]
pub struct Session {
    config: SessionConfig,
    magic: Magic,
    reader: FrameReader,
    outgoing: Vec<u8>,
    started: u64,
    /// The peer's services and start height, once its `version` is good.
    peer: Option<(ServiceFlags, i32)>,
    ready: bool,
    last_ping: u64,
    pings_sent: u64,
    awaiting_pong: Option<u64>,
    dropped: Option<Drop>,
}

impl Session {
    /// A session that has queued the counter's `version` (NR-11). `now` is
    /// in seconds.
    pub fn new(config: SessionConfig, now: u64) -> Self {
        let magic = Magic::from(config.network);
        let silent = Address {
            services: ServiceFlags::NONE,
            address: [0; 8],
            port: 0,
        };
        let version = VersionMessage {
            version: PROTOCOL_VERSION,
            services: ServiceFlags::NONE,
            timestamp: i64::try_from(now).unwrap_or(i64::MAX),
            receiver: silent.clone(),
            sender: silent,
            nonce: config.nonce,
            user_agent: String::new(),
            start_height: i32::try_from(config.tip_height).unwrap_or(i32::MAX),
            relay: true,
        };
        Self {
            config,
            magic,
            reader: FrameReader::new(magic),
            outgoing: encode(magic, &NetworkMessage::Version(version)),
            started: now,
            peer: None,
            ready: false,
            last_ping: now,
            pings_sent: 0,
            awaiting_pong: None,
            dropped: None,
        }
    }

    /// Bytes received from the peer, and the time. Returns the events they
    /// made.
    pub fn receive(&mut self, bytes: &[u8], now: u64) -> Result<Vec<Event>, Drop> {
        self.guarded(|s| {
            s.check_handshake_time(now)?;
            let mut events = Vec::new();
            for chunk in bytes.chunks(PUSH_SLACK) {
                s.reader.push(chunk)?;
                while let Some(frame) = s.reader.next()? {
                    s.handle(frame.parse()?, now, &mut events)?;
                }
            }
            s.check_ping_time(now)?;
            Ok(events)
        })
    }

    /// The bytes queued for the peer, emptied. After a drop there are none.
    pub fn take_outgoing(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.outgoing)
    }

    /// The time has passed. Ends a handshake that took too long and sends
    /// the next ping (NR-14).
    pub fn tick(&mut self, now: u64) -> Result<(), Drop> {
        self.guarded(|s| {
            s.check_handshake_time(now)?;
            s.check_ping_time(now)
        })
    }

    /// Ask the peer for something. Before the handshake is over this is a
    /// drop (NR-13).
    pub fn send(&mut self, request: Request) -> Result<(), Drop> {
        self.guarded(|s| {
            if !s.ready {
                return Err(Drop::NotReady);
            }
            match request {
                Request::GetHeaders { locator } => {
                    s.queue(NetworkMessage::GetHeaders(GetHeadersMessage {
                        version: PROTOCOL_VERSION,
                        locator_hashes: locator,
                        stop_hash: BlockHash::all_zeros(),
                    }))
                }
                Request::GetTx(txids) if txids.is_empty() => Ok(()),
                Request::GetTx(txids) => s.queue(NetworkMessage::GetData(
                    txids
                        .into_iter()
                        .map(Inventory::WitnessTransaction)
                        .collect(),
                )),
                Request::GetBlock(hash) => {
                    s.queue(NetworkMessage::GetData(vec![Inventory::WitnessBlock(hash)]))
                }
                Request::GetFilter { height, block } => {
                    s.queue(NetworkMessage::GetCFilters(GetCFilters {
                        filter_type: 0,
                        start_height: height,
                        stop_hash: block,
                    }))
                }
                Request::GetAddr => s.queue(NetworkMessage::GetAddr),
            }
        })
    }

    /// Run one call: a session that has dropped returns the same drop, and
    /// a call that fails ends the session.
    fn guarded<T>(&mut self, f: impl FnOnce(&mut Self) -> Result<T, Drop>) -> Result<T, Drop> {
        if let Some(d) = &self.dropped {
            return Err(d.clone());
        }
        let result = f(self);
        if let Err(d) = &result {
            self.dropped = Some(d.clone());
            self.outgoing = Vec::new();
        }
        result
    }

    /// NR-05: queue a message, unless too many bytes already wait.
    fn queue(&mut self, message: NetworkMessage) -> Result<(), Drop> {
        let bytes = encode(self.magic, &message);
        if self.outgoing.len().saturating_add(bytes.len()) > MAX_OUTGOING_BYTES {
            return Err(Drop::SendBacklog);
        }
        self.outgoing.extend_from_slice(&bytes);
        Ok(())
    }

    /// NR-14: a handshake not finished 30 seconds after the start ends.
    fn check_handshake_time(&self, now: u64) -> Result<(), Drop> {
        if !self.ready && now.saturating_sub(self.started) >= HANDSHAKE_SECS {
            return Err(Drop::HandshakeTimeout);
        }
        Ok(())
    }

    /// NR-14: after the handshake, a ping every 2 minutes, and a drop if
    /// the last one is still unanswered when the next is due.
    fn check_ping_time(&mut self, now: u64) -> Result<(), Drop> {
        if !self.ready || now.saturating_sub(self.last_ping) < PING_SECS {
            return Ok(());
        }
        if self.awaiting_pong.is_some() {
            return Err(Drop::PingUnanswered);
        }
        self.pings_sent = self.pings_sent.wrapping_add(1);
        let nonce = self.config.nonce.wrapping_add(self.pings_sent);
        self.queue(NetworkMessage::Ping(nonce))?;
        self.awaiting_pong = Some(nonce);
        self.last_ping = now;
        Ok(())
    }

    fn handle(&mut self, message: Message, now: u64, events: &mut Vec<Event>) -> Result<(), Drop> {
        if self.peer.is_none() {
            // NR-12: the peer's version comes first.
            return match message {
                Message::Version(version) => self.on_version(version),
                _ => Err(Drop::FirstNotVersion),
            };
        }
        if !self.ready {
            // NR-13: only the handshake until both verack.
            return match message {
                Message::Version(_) => Err(Drop::DuplicateVersion),
                Message::Verack => self.on_verack(now, events),
                Message::SendAddrV2 | Message::SendHeaders | Message::Ignored(_) => Ok(()),
                other => Err(Drop::BeforeHandshake {
                    command: other.command().name(),
                }),
            };
        }
        match message {
            Message::Version(_) => return Err(Drop::DuplicateVersion),
            Message::Verack => return Err(Drop::DuplicateVerack),
            Message::SendAddrV2 | Message::SendHeaders | Message::Ignored(_) => {}
            Message::Ping(nonce) => self.queue(NetworkMessage::Pong(nonce))?,
            Message::Pong(nonce) => {
                if self.awaiting_pong == Some(nonce) {
                    self.awaiting_pong = None;
                }
            }
            Message::Headers(headers) => events.push(Event::Headers(headers)),
            Message::Inv { txids, blocks } => events.push(Event::Announced { txids, blocks }),
            Message::NotFound { txids, blocks } => events.push(Event::NotFound { txids, blocks }),
            Message::Tx(bytes) => events.push(Event::Tx(bytes)),
            Message::Block(bytes) => events.push(Event::Block(bytes)),
            Message::CFilter { block, filter } => events.push(Event::Filter { block, filter }),
            Message::Addresses(list) => events.push(Event::Addresses(list)),
        }
        Ok(())
    }

    /// NR-12: the checks on the peer's `version`. Then NR-13: the counter
    /// answers with `sendaddrv2` and its `verack`, in that order.
    fn on_version(&mut self, version: VersionMessage) -> Result<(), Drop> {
        if version.version < PROTOCOL_VERSION {
            return Err(Drop::VersionTooOld {
                version: version.version,
            });
        }
        let services = version.services;
        if !services.has(ServiceFlags::WITNESS)
            || !(services.has(ServiceFlags::NETWORK) || services.has(ServiceFlags::NETWORK_LIMITED))
        {
            return Err(Drop::MissingServices {
                services: services.to_u64(),
            });
        }
        if version.nonce == self.config.nonce {
            return Err(Drop::OwnNonce);
        }
        if version.user_agent.len() > MAX_USER_AGENT {
            return Err(Drop::UserAgentTooLong {
                len: version.user_agent.len(),
            });
        }
        self.queue(NetworkMessage::SendAddrV2)?;
        self.queue(NetworkMessage::Verack)?;
        self.peer = Some((services, version.start_height));
        Ok(())
    }

    /// NR-13: the peer's `verack` ends the handshake. `sendheaders` goes
    /// out now: a peer does not read it before the handshake is over.
    fn on_verack(&mut self, now: u64, events: &mut Vec<Event>) -> Result<(), Drop> {
        let (services, height) = self.peer.unwrap_or((ServiceFlags::NONE, 0));
        self.ready = true;
        self.last_ping = now;
        self.reader.set_handshake_done();
        self.queue(NetworkMessage::SendHeaders)?;
        events.push(Event::Ready {
            services,
            start_height: u32::try_from(height).unwrap_or(0),
        });
        Ok(())
    }
}
