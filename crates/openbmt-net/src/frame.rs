//! The wire: cutting a byte stream into messages and reading each payload
//! (NR-01 to NR-05).
//!
//! [`FrameReader`] decides from the 24-byte header alone whether a message
//! is read on: the magic, the command and the length against the limit for
//! that command, all before any payload is kept. [`Frame::parse`] reads a
//! payload as its command, and refuses one that leaves bytes over.

use std::net::SocketAddr;

use bitcoin::block::Header;
use bitcoin::consensus::encode::{deserialize, deserialize_partial, serialize, VarInt};
use bitcoin::hashes::{sha256d, Hash};
use bitcoin::p2p::address::{AddrV2, AddrV2Message, Address};
use bitcoin::p2p::message::{NetworkMessage, RawNetworkMessage};
use bitcoin::p2p::message_filter::CFilter;
use bitcoin::p2p::message_network::VersionMessage;
use bitcoin::p2p::{Magic, ServiceFlags};
use bitcoin::{BlockHash, Txid};

use crate::{
    Drop, MAX_ADDRESSES, MAX_ADDR_PAYLOAD, MAX_BLOCK_PAYLOAD, MAX_CFILTER_PAYLOAD, MAX_HEADERS,
    MAX_HEADERS_PAYLOAD, MAX_INVENTORY, MAX_INV_PAYLOAD, MAX_SMALL_PAYLOAD, MAX_TX_PAYLOAD,
    MAX_UNKNOWN_PAYLOAD, MAX_VERSION_PAYLOAD, PUSH_SLACK,
};

/// Bytes in the header of a message.
pub const HEADER_LEN: usize = 24;
/// Bytes of one header in a `headers` message: the header and a zero count
/// of transactions.
const HEADERS_ENTRY_LEN: usize = 81;

/// A command the counter reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    /// `version`
    Version,
    /// `verack`
    Verack,
    /// `ping`
    Ping,
    /// `pong`
    Pong,
    /// `sendaddrv2`
    SendAddrV2,
    /// `sendheaders`
    SendHeaders,
    /// `wtxidrelay`, read and ignored
    WtxidRelay,
    /// `feefilter`, read and ignored
    FeeFilter,
    /// `sendcmpct`, read and ignored
    SendCmpct,
    /// `headers`
    Headers,
    /// `inv`
    Inv,
    /// `notfound`
    NotFound,
    /// `tx`
    Tx,
    /// `block`
    Block,
    /// `cfilter`
    CFilter,
    /// `addr`
    Addr,
    /// `addrv2`
    AddrV2,
}

impl Command {
    /// The command as it is written in a header.
    pub fn name(self) -> &'static str {
        match self {
            Self::Version => "version",
            Self::Verack => "verack",
            Self::Ping => "ping",
            Self::Pong => "pong",
            Self::SendAddrV2 => "sendaddrv2",
            Self::SendHeaders => "sendheaders",
            Self::WtxidRelay => "wtxidrelay",
            Self::FeeFilter => "feefilter",
            Self::SendCmpct => "sendcmpct",
            Self::Headers => "headers",
            Self::Inv => "inv",
            Self::NotFound => "notfound",
            Self::Tx => "tx",
            Self::Block => "block",
            Self::CFilter => "cfilter",
            Self::Addr => "addr",
            Self::AddrV2 => "addrv2",
        }
    }

    fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "version" => Self::Version,
            "verack" => Self::Verack,
            "ping" => Self::Ping,
            "pong" => Self::Pong,
            "sendaddrv2" => Self::SendAddrV2,
            "sendheaders" => Self::SendHeaders,
            "wtxidrelay" => Self::WtxidRelay,
            "feefilter" => Self::FeeFilter,
            "sendcmpct" => Self::SendCmpct,
            "headers" => Self::Headers,
            "inv" => Self::Inv,
            "notfound" => Self::NotFound,
            "tx" => Self::Tx,
            "block" => Self::Block,
            "cfilter" => Self::CFilter,
            "addr" => Self::Addr,
            "addrv2" => Self::AddrV2,
            _ => return None,
        })
    }

    /// NR-02: the most payload bytes the command may carry.
    pub fn limit(self) -> u32 {
        match self {
            Self::Block => MAX_BLOCK_PAYLOAD,
            Self::Tx => MAX_TX_PAYLOAD,
            Self::CFilter => MAX_CFILTER_PAYLOAD,
            Self::Headers => MAX_HEADERS_PAYLOAD,
            Self::Inv | Self::NotFound => MAX_INV_PAYLOAD,
            Self::Addr | Self::AddrV2 => MAX_ADDR_PAYLOAD,
            Self::Version => MAX_VERSION_PAYLOAD,
            Self::Verack
            | Self::Ping
            | Self::Pong
            | Self::SendAddrV2
            | Self::SendHeaders
            | Self::WtxidRelay
            | Self::FeeFilter
            | Self::SendCmpct => MAX_SMALL_PAYLOAD,
        }
    }

    /// NR-13: whether the command may arrive before the handshake ends.
    fn belongs_to_handshake(self) -> bool {
        matches!(
            self,
            Self::Version
                | Self::Verack
                | Self::SendAddrV2
                | Self::SendHeaders
                | Self::WtxidRelay
                | Self::FeeFilter
                | Self::SendCmpct
        )
    }
}

/// A message of a command the counter reads, with its payload checked
/// against the header and not yet parsed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    /// The command.
    pub command: Command,
    /// The payload, whose length and checksum the header stated.
    pub payload: Vec<u8>,
}

/// One address a peer told of, from `addr` or `addrv2`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerAddress {
    /// The address.
    pub addr: AddrV2,
    /// The port, 0 if not applicable.
    pub port: u16,
    /// The services the peer states for it. A claim.
    pub services: ServiceFlags,
    /// The time the peer states it was last seen, in seconds. A claim.
    pub time: u32,
}

/// A payload read as its command (NR-04).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Message {
    /// `version`
    Version(VersionMessage),
    /// `verack`
    Verack,
    /// `ping` with its nonce
    Ping(u64),
    /// `pong` with its nonce
    Pong(u64),
    /// `sendaddrv2`
    SendAddrV2,
    /// `sendheaders`
    SendHeaders,
    /// A command that is read and ignored.
    Ignored(Command),
    /// `headers`, at most 2,000
    Headers(Vec<Header>),
    /// `inv`: transactions by txid and blocks; other entries are dropped
    Inv {
        /// Transactions announced by txid.
        txids: Vec<Txid>,
        /// Blocks announced.
        blocks: Vec<BlockHash>,
    },
    /// `notfound`: what the peer does not have
    NotFound {
        /// Transactions, by txid.
        txids: Vec<Txid>,
        /// Blocks.
        blocks: Vec<BlockHash>,
    },
    /// `tx`: the bytes, checked to be one transaction and nothing more
    Tx(Vec<u8>),
    /// `block`: the bytes, not parsed here
    Block(Vec<u8>),
    /// `cfilter` of type 0
    CFilter {
        /// The block the filter is for.
        block: BlockHash,
        /// The filter bytes.
        filter: Vec<u8>,
    },
    /// `addr` or `addrv2`, at most 1,000
    Addresses(Vec<PeerAddress>),
}

impl Message {
    /// The command the message came from.
    pub fn command(&self) -> Command {
        match self {
            Self::Version(_) => Command::Version,
            Self::Verack => Command::Verack,
            Self::Ping(_) => Command::Ping,
            Self::Pong(_) => Command::Pong,
            Self::SendAddrV2 => Command::SendAddrV2,
            Self::SendHeaders => Command::SendHeaders,
            Self::Ignored(command) => *command,
            Self::Headers(_) => Command::Headers,
            Self::Inv { .. } => Command::Inv,
            Self::NotFound { .. } => Command::NotFound,
            Self::Tx(_) => Command::Tx,
            Self::Block(_) => Command::Block,
            Self::CFilter { .. } => Command::CFilter,
            Self::Addresses(_) => Command::Addr,
        }
    }
}

impl Frame {
    /// NR-04: read the payload as its command. A payload that does not
    /// parse, or leaves bytes over, is a drop.
    pub fn parse(self) -> Result<Message, Drop> {
        let command = self.command;
        let name = command.name();
        let bad = |_| Drop::Malformed { command: name };
        let p = self.payload.as_slice();
        Ok(match command {
            Command::Version => Message::Version(deserialize(p).map_err(bad)?),
            Command::Verack => empty(p, name).map(|()| Message::Verack)?,
            Command::SendAddrV2 => empty(p, name).map(|()| Message::SendAddrV2)?,
            Command::SendHeaders => empty(p, name).map(|()| Message::SendHeaders)?,
            Command::Ping => Message::Ping(deserialize(p).map_err(bad)?),
            Command::Pong => Message::Pong(deserialize(p).map_err(bad)?),
            Command::WtxidRelay | Command::FeeFilter | Command::SendCmpct => {
                Message::Ignored(command)
            }
            Command::Headers => Message::Headers(parse_headers(p)?),
            Command::Inv | Command::NotFound => {
                let witness_too = command == Command::NotFound;
                let (txids, blocks) = parse_inventory(p, name, witness_too)?;
                if command == Command::Inv {
                    Message::Inv { txids, blocks }
                } else {
                    Message::NotFound { txids, blocks }
                }
            }
            Command::Tx => {
                // Checked to be one transaction, then handed on as bytes.
                deserialize::<bitcoin::Transaction>(p).map_err(bad)?;
                Message::Tx(self.payload)
            }
            Command::Block => Message::Block(self.payload),
            Command::CFilter => {
                let filter: CFilter = deserialize(p).map_err(bad)?;
                if filter.filter_type != 0 {
                    return Err(Drop::WrongFilterType {
                        filter_type: filter.filter_type,
                    });
                }
                Message::CFilter {
                    block: filter.block_hash,
                    filter: filter.filter,
                }
            }
            Command::Addr => {
                let list: Vec<(u32, Address)> = deserialize(p).map_err(bad)?;
                if list.len() > MAX_ADDRESSES {
                    return Err(too_many(name, list.len(), MAX_ADDRESSES));
                }
                Message::Addresses(list.into_iter().filter_map(old_address).collect())
            }
            Command::AddrV2 => {
                let list: Vec<AddrV2Message> = deserialize(p).map_err(bad)?;
                if list.len() > MAX_ADDRESSES {
                    return Err(too_many(name, list.len(), MAX_ADDRESSES));
                }
                Message::Addresses(
                    list.into_iter()
                        .map(|m| PeerAddress {
                            addr: m.addr,
                            port: m.port,
                            services: m.services,
                            time: m.time,
                        })
                        .collect(),
                )
            }
        })
    }
}

fn too_many(command: &'static str, count: usize, limit: usize) -> Drop {
    Drop::TooManyItems {
        command,
        count: u64::try_from(count).unwrap_or(u64::MAX),
        limit,
    }
}

/// A message with no payload has no payload.
fn empty(payload: &[u8], command: &'static str) -> Result<(), Drop> {
    if payload.is_empty() {
        Ok(())
    } else {
        Err(Drop::Malformed { command })
    }
}

/// Transactions by txid and blocks from an `inv` or `notfound` payload.
/// `inv` takes only the plain types; the `notfound` that answers a request
/// for witness data names the witness types too. Entries of other types
/// are left out. The entries are read by hand: the bitcoin crate reads the
/// error type (0) without its hash, which no peer sends that way.
fn parse_inventory(
    payload: &[u8],
    command: &'static str,
    witness_too: bool,
) -> Result<(Vec<Txid>, Vec<BlockHash>), Drop> {
    const ENTRY: usize = 36;
    let bad = || Drop::Malformed { command };
    let (VarInt(count), used) = deserialize_partial::<VarInt>(payload).map_err(|_| bad())?;
    let count = usize::try_from(count).map_err(|_| bad())?;
    if count > MAX_INVENTORY {
        return Err(too_many(command, count, MAX_INVENTORY));
    }
    let rest = payload.get(used..).ok_or_else(bad)?;
    if count.checked_mul(ENTRY) != Some(rest.len()) {
        return Err(bad());
    }
    let mut txids = Vec::new();
    let mut blocks = Vec::new();
    for entry in rest.as_chunks::<ENTRY>().0 {
        let (kind, hash) = entry.split_first_chunk::<4>().ok_or_else(bad)?;
        let hash: [u8; 32] = hash.try_into().map_err(|_| bad())?;
        match u32::from_le_bytes(*kind) {
            1 => txids.push(Txid::from_byte_array(hash)),
            2 => blocks.push(BlockHash::from_byte_array(hash)),
            0x4000_0001 if witness_too => txids.push(Txid::from_byte_array(hash)),
            0x4000_0002 if witness_too => blocks.push(BlockHash::from_byte_array(hash)),
            _ => {}
        }
    }
    Ok((txids, blocks))
}

/// An `addr` entry as a [`PeerAddress`]. A Tor v2 address in the old form
/// has no place in the new one and is left out.
fn old_address((time, address): (u32, Address)) -> Option<PeerAddress> {
    let addr = match address.socket_addr().ok()? {
        SocketAddr::V4(a) => AddrV2::Ipv4(*a.ip()),
        SocketAddr::V6(a) => AddrV2::Ipv6(*a.ip()),
    };
    Some(PeerAddress {
        addr,
        port: address.port,
        services: address.services,
        time,
    })
}

/// NR-04: a `headers` message holds at most 2,000 headers, each followed
/// by a zero count of transactions, and nothing more.
fn parse_headers(payload: &[u8]) -> Result<Vec<Header>, Drop> {
    let bad = || Drop::Malformed { command: "headers" };
    let (VarInt(count), used) = deserialize_partial::<VarInt>(payload).map_err(|_| bad())?;
    let count = usize::try_from(count).map_err(|_| bad())?;
    if count > MAX_HEADERS {
        return Err(too_many("headers", count, MAX_HEADERS));
    }
    let rest = payload.get(used..).ok_or_else(bad)?;
    if count.checked_mul(HEADERS_ENTRY_LEN) != Some(rest.len()) {
        return Err(bad());
    }
    let mut headers = Vec::with_capacity(count);
    for entry in rest.as_chunks::<HEADERS_ENTRY_LEN>().0 {
        let (header, zero) = entry
            .split_at_checked(HEADERS_ENTRY_LEN - 1)
            .ok_or_else(bad)?;
        if zero != [0u8] {
            return Err(bad());
        }
        headers.push(deserialize(header).map_err(|_| bad())?);
    }
    Ok(headers)
}

/// Write a message with the network's magic, the command, the length and
/// the checksum.
pub fn encode(magic: Magic, message: &NetworkMessage) -> Vec<u8> {
    serialize(&RawNetworkMessage::new(magic, message.clone()))
}

/// The first four bytes of the double SHA-256 of the payload.
fn checksum(payload: &[u8]) -> [u8; 4] {
    let [a, b, c, d, ..] = sha256d::Hash::hash(payload).to_byte_array();
    [a, b, c, d]
}

/// The fields of a header.
struct Header24 {
    magic: [u8; 4],
    command: [u8; 12],
    len: u32,
    checksum: [u8; 4],
}

fn split_header(bytes: &[u8]) -> Option<Header24> {
    let (magic, rest) = bytes.split_first_chunk::<4>()?;
    let (command, rest) = rest.split_first_chunk::<12>()?;
    let (len, rest) = rest.split_first_chunk::<4>()?;
    let (checksum, _) = rest.split_first_chunk::<4>()?;
    Some(Header24 {
        magic: *magic,
        command: *command,
        len: u32::from_le_bytes(*len),
        checksum: *checksum,
    })
}

/// NR-01: the command is printable ASCII, then NULs to the end.
fn command_name(field: &[u8; 12]) -> Result<&str, Drop> {
    let end = field.iter().position(|b| *b == 0).unwrap_or(field.len());
    let (name, padding) = field.split_at_checked(end).ok_or(Drop::BadCommand)?;
    if name.is_empty()
        || !name.iter().all(|b| (0x20..=0x7e).contains(b))
        || padding.iter().any(|b| *b != 0)
    {
        return Err(Drop::BadCommand);
    }
    std::str::from_utf8(name).map_err(|_| Drop::BadCommand)
}

/// Cuts a byte stream into messages (NR-01 to NR-03, NR-05).
///
/// The caller pushes what it received, in reads of at most
/// [`PUSH_SLACK`] bytes, and calls [`FrameReader::next`] until it returns
/// `None` before the next push. The reader holds at most the message in
/// progress and one read.
#[derive(Debug)]
pub struct FrameReader {
    magic: Magic,
    buf: Vec<u8>,
    /// Bytes at the front of `buf` that have been handed out.
    start: usize,
    /// The limit of the message whose header has been read, else 0.
    limit: usize,
    handshake_done: bool,
    dropped: Option<Drop>,
}

impl FrameReader {
    /// A reader for the network with this magic.
    pub fn new(magic: Magic) -> Self {
        Self {
            magic,
            buf: Vec::new(),
            start: 0,
            limit: 0,
            handshake_done: false,
            dropped: None,
        }
    }

    /// NR-13: from now on the commands that belong after the handshake are
    /// read. Until then one that arrives is a drop at its header.
    pub fn set_handshake_done(&mut self) {
        self.handshake_done = true;
    }

    /// Bytes held that have not been handed out.
    pub fn buffered(&self) -> usize {
        self.buf.len().saturating_sub(self.start)
    }

    /// NR-05: the most bytes [`FrameReader::buffered`] may reach now: a
    /// header, the message in progress and one read.
    pub fn buffer_cap(&self) -> usize {
        HEADER_LEN
            .saturating_add(self.limit)
            .saturating_add(PUSH_SLACK)
    }

    /// Add bytes received. More than [`FrameReader::buffer_cap`] held is a
    /// drop (NR-05).
    pub fn push(&mut self, bytes: &[u8]) -> Result<(), Drop> {
        if let Some(d) = &self.dropped {
            return Err(d.clone());
        }
        self.buf.drain(..self.start.min(self.buf.len()));
        self.start = 0;
        if self.buf.len().saturating_add(bytes.len()) > self.buffer_cap() {
            return Err(self.fail(Drop::ReceiveBacklog));
        }
        self.buf.extend_from_slice(bytes);
        Ok(())
    }

    /// The next message of a command the counter reads, or `None` when
    /// more bytes are needed. Messages of other commands are consumed and
    /// not returned (NR-03).
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> Result<Option<Frame>, Drop> {
        if let Some(d) = &self.dropped {
            return Err(d.clone());
        }
        match self.next_frame() {
            Ok(frame) => Ok(frame),
            Err(d) => Err(self.fail(d)),
        }
    }

    fn fail(&mut self, drop: Drop) -> Drop {
        self.buf = Vec::new();
        self.start = 0;
        self.dropped = Some(drop.clone());
        drop
    }

    fn next_frame(&mut self) -> Result<Option<Frame>, Drop> {
        loop {
            let avail = self.buf.get(self.start..).unwrap_or_default();
            let Some(head) = split_header(avail) else {
                return Ok(None);
            };
            if head.magic != self.magic.to_bytes() {
                return Err(Drop::WrongMagic);
            }
            let name = command_name(&head.command)?;
            let command = Command::from_name(name);
            let limit = match command {
                Some(c) => {
                    if !self.handshake_done && !c.belongs_to_handshake() {
                        return Err(Drop::BeforeHandshake { command: c.name() });
                    }
                    c.limit()
                }
                None => MAX_UNKNOWN_PAYLOAD,
            };
            if head.len > limit {
                return Err(match command {
                    Some(c) => Drop::TooLong {
                        command: c.name(),
                        len: head.len,
                        limit,
                    },
                    None => Drop::UnknownTooLong { len: head.len },
                });
            }
            self.limit = usize::try_from(limit).unwrap_or(usize::MAX);
            let total = usize::try_from(head.len)
                .ok()
                .and_then(|n| n.checked_add(HEADER_LEN))
                .ok_or(Drop::ReceiveBacklog)?;
            let Some(body) = avail.get(HEADER_LEN..total) else {
                return Ok(None);
            };
            if checksum(body) != head.checksum {
                return Err(Drop::BadChecksum);
            }
            let frame = command.map(|command| Frame {
                command,
                payload: body.to_vec(),
            });
            self.start = self.start.saturating_add(total);
            self.limit = 0;
            if self.start >= self.buf.len() {
                self.start = 0;
                if self.buf.capacity() > PUSH_SLACK {
                    self.buf = Vec::new();
                } else {
                    self.buf.clear();
                }
            }
            if frame.is_some() {
                return Ok(frame);
            }
        }
    }
}
