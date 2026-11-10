//! Peers for the manager tests: a regtest chain of mined headers and blocks,
//! peers that finish the handshake, and the messages they send. What the
//! manager writes is decoded with the bitcoin crate, and every command in it
//! is checked against the list the counter may send (NR-55).

use std::net::{IpAddr, Ipv4Addr};

use bitcoin::absolute::LockTime;
use bitcoin::bip158::BlockFilter;
use bitcoin::block::{Header, Version as BlockVersion};
use bitcoin::blockdata::constants::genesis_block;
use bitcoin::consensus::encode::serialize;
use bitcoin::hashes::Hash;
use bitcoin::p2p::message::NetworkMessage;
use bitcoin::p2p::message_blockdata::Inventory;
use bitcoin::p2p::message_filter::CFilter;
use bitcoin::p2p::ServiceFlags;
use bitcoin::transaction::Version as TxVersion;
use bitcoin::{
    Amount, Block, BlockHash, CompactTarget, Network, OutPoint, ScriptBuf, Sequence, Transaction,
    TxIn, TxMerkleNode, TxOut, Txid, Witness,
};
use openbmt_watch::{HeaderStore, PeerAddr};

use super::kit::{decode_all, frame, peer_version, verack_frame};
use crate::manager::FILTER_SETTLE_SECS;
use crate::{Manager, ManagerConfig, Output, PeerId};

/// The caller's clock when a test starts, in seconds. Regtest headers are
/// from 2011 on, so none is too far ahead of it.
pub const T0: u64 = 1_700_000_000;
/// Regtest difficulty.
const BITS: u32 = 0x207f_ffff;
/// The commands the counter may write (NR-55): requests, the handshake and
/// the answer to a ping. No `tx`, `inv`, `headers`, `addr` or `block`, and
/// no answer to `getdata`, `getheaders`, `getaddr` or `mempool`.
pub const MAY_SEND: [&str; 10] = [
    "version",
    "verack",
    "sendaddrv2",
    "sendheaders",
    "ping",
    "pong",
    "getheaders",
    "getdata",
    "getcfilters",
    "getaddr",
];

pub fn genesis() -> Header {
    genesis_block(Network::Regtest).header
}

fn root_of(tag: u32) -> TxMerkleNode {
    let mut root = [0u8; 32];
    root[..4].copy_from_slice(&tag.to_le_bytes());
    TxMerkleNode::from_byte_array(root)
}

/// A header mined until it meets its own target.
pub fn mine(parent: BlockHash, time: u32, bits: u32, root: TxMerkleNode) -> Header {
    let mut header = Header {
        version: BlockVersion::ONE,
        prev_blockhash: parent,
        merkle_root: root,
        time,
        bits: CompactTarget::from_consensus(bits),
        nonce: 0,
    };
    while header.validate_pow(header.target()).is_err() {
        header.nonce += 1;
    }
    header
}

/// A header whose hash does not meet the target it states.
pub fn unmined(parent: &Header, tag: u32) -> Header {
    let mut header = Header {
        version: BlockVersion::ONE,
        prev_blockhash: parent.block_hash(),
        merkle_root: root_of(tag),
        time: parent.time + 600,
        bits: CompactTarget::from_consensus(BITS),
        nonce: 0,
    };
    while header.validate_pow(header.target()).is_ok() {
        header.nonce += 1;
    }
    header
}

/// A header on this parent; `tag` makes headers with one parent different.
pub fn child(parent: &Header, tag: u32) -> Header {
    mine(parent.block_hash(), parent.time + 600, BITS, root_of(tag))
}

/// `n` headers one after the other on this parent, with the tags `tag` on.
pub fn extend(parent: &Header, n: usize, tag: u32) -> Vec<Header> {
    let mut out: Vec<Header> = Vec::new();
    for i in 0..n {
        let next = child(out.last().unwrap_or(parent), tag + i as u32);
        out.push(next);
    }
    out
}

pub fn script(n: u8) -> ScriptBuf {
    let mut bytes = vec![0x00, 0x14];
    bytes.extend([n; 20]);
    ScriptBuf::from_bytes(bytes)
}

pub fn coinbase(tag: u32) -> Transaction {
    let mut script_sig = vec![0x04];
    script_sig.extend(tag.to_le_bytes());
    Transaction {
        version: TxVersion::TWO,
        lock_time: LockTime::ZERO,
        input: vec![TxIn {
            previous_output: OutPoint::null(),
            script_sig: ScriptBuf::from_bytes(script_sig),
            sequence: Sequence::MAX,
            witness: Witness::new(),
        }],
        output: vec![TxOut {
            value: Amount::from_sat(5_000_000_000),
            script_pubkey: script(0xee),
        }],
    }
}

/// A transaction that spends a coin only `tag` names and pays this script.
pub fn pay(to: &ScriptBuf, sats: u64, tag: u32) -> Transaction {
    let mut txid = [0u8; 32];
    txid[..4].copy_from_slice(&tag.to_le_bytes());
    txid[31] = 0x99;
    Transaction {
        version: TxVersion::TWO,
        lock_time: LockTime::ZERO,
        input: vec![TxIn {
            previous_output: OutPoint::new(Txid::from_byte_array(txid), 0),
            script_sig: ScriptBuf::new(),
            sequence: Sequence::MAX,
            witness: Witness::new(),
        }],
        output: vec![TxOut {
            value: Amount::from_sat(sats),
            script_pubkey: to.clone(),
        }],
    }
}

/// A mined block on this parent: a coinbase and these transactions.
pub fn block_on(parent: &Header, txs: Vec<Transaction>, tag: u32) -> Block {
    let mut txdata = vec![coinbase(tag)];
    txdata.extend(txs);
    let mut block = Block {
        header: *parent,
        txdata,
    };
    let root = block.compute_merkle_root().expect("has transactions");
    block.header = mine(parent.block_hash(), parent.time + 600, BITS, root);
    block
}

/// A chain of blocks on this parent, one per entry, holding these
/// transactions. The tags start at `tag`.
pub fn blocks_on(parent: &Header, specs: Vec<Vec<Transaction>>, tag: u32) -> Vec<Block> {
    let mut out: Vec<Block> = Vec::new();
    for (i, txs) in specs.into_iter().enumerate() {
        let next = block_on(
            out.last().map_or(parent, |b| &b.header),
            txs,
            tag + i as u32,
        );
        out.push(next);
    }
    out
}

pub fn headers_of(blocks: &[Block]) -> Vec<Header> {
    blocks.iter().map(|b| b.header).collect()
}

/// The basic filter of a block, as an honest peer serves it.
pub fn filter_of(block: &Block) -> Vec<u8> {
    BlockFilter::new_script_filter(block, |_| Ok(ScriptBuf::new()))
        .expect("a filter can be made")
        .content
}

/// The filter a block with the same header would have if it held a
/// coinbase and these transactions: a peer's lie about the block.
pub fn filter_with(block: &Block, txs: Vec<Transaction>) -> Vec<u8> {
    let mut other = block.clone();
    other.txdata = vec![coinbase(0)];
    other.txdata.extend(txs);
    filter_of(&other)
}

pub fn bytes_of(block: &Block) -> Vec<u8> {
    serialize(block)
}

/// An address in its own /16.
pub fn addr(n: u8) -> PeerAddr {
    PeerAddr::Ip(IpAddr::V4(Ipv4Addr::new(20 + n, 1, 0, 1)))
}

pub fn filters() -> ServiceFlags {
    ServiceFlags::NETWORK | ServiceFlags::WITNESS | ServiceFlags::COMPACT_FILTERS
}

pub fn plain() -> ServiceFlags {
    ServiceFlags::NETWORK | ServiceFlags::WITNESS
}

/// A peer that has joined: its ID, what the manager said when the
/// handshake ended and what it wrote to the peer then.
pub struct Joined {
    pub id: PeerId,
    pub out: Vec<Output>,
    pub sent: Vec<NetworkMessage>,
}

/// A manager on regtest and the clock the tests move.
pub struct Net {
    pub m: Manager,
    pub now: u64,
}

impl Net {
    pub fn new() -> Self {
        Self::with(0, 8)
    }

    pub fn with(watch_from: u32, target_peers: usize) -> Self {
        let config = ManagerConfig {
            network: Network::Regtest,
            target_peers,
            watch_from,
            allow_local: false,
        };
        Self {
            m: Manager::new(config, HeaderStore::new(Network::Regtest).expect("regtest")),
            now: T0,
        }
    }

    /// Connect peer `n` (in its own group) and finish the handshake.
    pub fn join(&mut self, n: u8, services: ServiceFlags, start_height: i32) -> Joined {
        let id = PeerId(u64::from(n));
        self.m
            .connected(id, addr(n), 0x1000 + u64::from(n), self.now)
            .expect("connected");
        let first = self.sent(id);
        assert!(
            matches!(first[..], [NetworkMessage::Version(_)]),
            "{first:?}"
        );
        let mut version = peer_version();
        version.services = services;
        version.start_height = start_height;
        let mut stream = frame(NetworkMessage::Version(version));
        stream.extend(verack_frame());
        let out = self.m.received(id, &stream, self.now);
        let sent = self.sent(id);
        Joined { id, out, sent }
    }

    /// A full node without filters, at height 0.
    pub fn up(&mut self, n: u8) -> PeerId {
        self.join(n, plain(), 0).id
    }

    /// A full node with filters, at height 0.
    pub fn up_filters(&mut self, n: u8) -> PeerId {
        self.join(n, filters(), 0).id
    }

    /// What the manager wrote to the peer, decoded and checked (NR-55).
    pub fn sent(&mut self, id: PeerId) -> Vec<NetworkMessage> {
        let messages = decode_all(&self.m.outgoing(id));
        for message in &messages {
            assert!(
                MAY_SEND.contains(&message.cmd()),
                "the counter wrote a {}",
                message.cmd()
            );
        }
        messages
    }

    pub fn feed(&mut self, id: PeerId, message: NetworkMessage) -> Vec<Output> {
        self.m.received(id, &frame(message), self.now)
    }

    pub fn feed_bytes(&mut self, id: PeerId, bytes: &[u8]) -> Vec<Output> {
        self.m.received(id, bytes, self.now)
    }

    /// Move the clock on and tick.
    pub fn tick(&mut self, secs: u64) -> Vec<Output> {
        self.now += secs;
        self.m.tick(self.now)
    }

    pub fn headers(&mut self, id: PeerId, headers: &[Header]) -> Vec<Output> {
        self.feed(id, NetworkMessage::Headers(headers.to_vec()))
    }

    pub fn block(&mut self, id: PeerId, block: &Block) -> Vec<Output> {
        self.feed(id, NetworkMessage::Block(block.clone()))
    }

    pub fn filter(&mut self, id: PeerId, block: BlockHash, filter: Vec<u8>) -> Vec<Output> {
        self.feed(
            id,
            NetworkMessage::CFilter(CFilter {
                filter_type: 0,
                block_hash: block,
                filter,
            }),
        )
    }

    pub fn tx(&mut self, id: PeerId, tx: &Transaction) -> Vec<Output> {
        self.feed(id, NetworkMessage::Tx(tx.clone()))
    }

    pub fn inv_txs(&mut self, id: PeerId, txids: &[Txid]) -> Vec<Output> {
        let list = txids.iter().map(|t| Inventory::Transaction(*t)).collect();
        self.feed(id, NetworkMessage::Inv(list))
    }

    pub fn inv_block(&mut self, id: PeerId, hash: BlockHash) -> Vec<Output> {
        self.feed(id, NetworkMessage::Inv(vec![Inventory::Block(hash)]))
    }

    pub fn not_found_block(&mut self, id: PeerId, hash: BlockHash) -> Vec<Output> {
        self.feed(
            id,
            NetworkMessage::NotFound(vec![Inventory::WitnessBlock(hash)]),
        )
    }

    pub fn not_found_tx(&mut self, id: PeerId, txid: Txid) -> Vec<Output> {
        self.feed(
            id,
            NetworkMessage::NotFound(vec![Inventory::WitnessTransaction(txid)]),
        )
    }
}

/// The locators of the `getheaders` in these messages.
pub fn header_requests(messages: &[NetworkMessage]) -> Vec<Vec<BlockHash>> {
    messages
        .iter()
        .filter_map(|m| match m {
            NetworkMessage::GetHeaders(g) => Some(g.locator_hashes.clone()),
            _ => None,
        })
        .collect()
}

/// The (height, block) of the `getcfilters` in these messages.
pub fn filter_requests(messages: &[NetworkMessage]) -> Vec<(u32, BlockHash)> {
    messages
        .iter()
        .filter_map(|m| match m {
            NetworkMessage::GetCFilters(g) => Some((g.start_height, g.stop_hash)),
            _ => None,
        })
        .collect()
}

/// The blocks asked for with `getdata`.
pub fn block_requests(messages: &[NetworkMessage]) -> Vec<BlockHash> {
    let mut out = Vec::new();
    for message in messages {
        if let NetworkMessage::GetData(list) = message {
            for item in list {
                if let Inventory::WitnessBlock(hash) = item {
                    out.push(*hash);
                }
            }
        }
    }
    out
}

/// The transactions asked for with `getdata`.
pub fn tx_requests(messages: &[NetworkMessage]) -> Vec<Txid> {
    let mut out = Vec::new();
    for message in messages {
        if let NetworkMessage::GetData(list) = message {
            for item in list {
                if let Inventory::WitnessTransaction(txid) = item {
                    out.push(*txid);
                }
            }
        }
    }
    out
}

/// The peers disconnected in these outputs, with whether they misbehaved.
pub fn disconnects(out: &[Output]) -> Vec<(PeerId, bool)> {
    out.iter()
        .filter_map(|o| match o {
            Output::Disconnect { id, misbehaved, .. } => Some((*id, *misbehaved)),
            _ => None,
        })
        .collect()
}

/// The reason text of the one disconnect in these outputs.
pub fn reason(out: &[Output]) -> String {
    out.iter()
        .find_map(|o| match o {
            Output::Disconnect { reason, .. } => Some(reason.clone()),
            _ => None,
        })
        .expect("a disconnect")
}

/// The hashes of the blocks handed out in these outputs.
pub fn blocks_out(out: &[Output]) -> Vec<BlockHash> {
    out.iter()
        .filter_map(|o| match o {
            Output::Block { hash, .. } => Some(*hash),
            _ => None,
        })
        .collect()
}

/// The chain changes in these outputs.
/// What joined and what left in one `Chain` output.
pub type Change = (Vec<(u32, BlockHash)>, Vec<BlockHash>);

pub fn chain_out(out: &[Output]) -> Vec<Change> {
    out.iter()
        .filter_map(|o| match o {
            Output::Chain { joined, left } => Some((joined.clone(), left.clone())),
            _ => None,
        })
        .collect()
}

/// A rig with a script watched, two peers that serve filters (1 and 2) and
/// one that does not (3), and a chain of blocks. Peer 3 delivers the headers
/// first; then both filter peers send them too and the clock moves past
/// [`FILTER_SETTLE_SECS`], so that every peer has shown every block (NR-40).
pub struct Rig {
    pub n: Net,
    pub a: PeerId,
    pub b: PeerId,
    pub c: PeerId,
    pub blocks: Vec<Block>,
}

impl Rig {
    /// `specs` are the transactions of each block, from height 1. The
    /// headers are delivered, and what the peers were asked is not taken.
    pub fn new(specs: Vec<Vec<Transaction>>) -> Self {
        Self::with(0, specs)
    }

    pub fn with(watch_from: u32, specs: Vec<Vec<Transaction>>) -> Self {
        let mut n = Net::with(watch_from, 8);
        assert!(n.m.watch_script(script(7)));
        let a = n.up_filters(1);
        let b = n.up_filters(2);
        let c = n.up(3);
        let blocks = blocks_on(&genesis(), specs, 1);
        let headers = headers_of(&blocks);
        n.headers(c, &headers);
        let mut rig = Self { n, a, b, c, blocks };
        rig.show(&headers);
        rig
    }

    /// Both filter peers send these headers too, and the clock moves past
    /// [`FILTER_SETTLE_SECS`], so that they are asked for the filters.
    pub fn show(&mut self, headers: &[Header]) {
        self.n.headers(self.a, headers);
        self.n.headers(self.b, headers);
        self.n.tick(FILTER_SETTLE_SECS);
    }

    /// The block at this height.
    pub fn at(&self, height: usize) -> &Block {
        &self.blocks[height - 1]
    }

    pub fn hash(&self, height: usize) -> BlockHash {
        self.at(height).block_hash()
    }

    /// Both filter peers give the honest filter of the block at this height.
    pub fn both_honest(&mut self, height: usize) -> Vec<Output> {
        let (hash, filter) = (self.hash(height), filter_of(self.at(height)));
        let mut out = self.n.filter(self.a, hash, filter.clone());
        out.extend(self.n.filter(self.b, hash, filter));
        out
    }
}

/// A transaction ID from a number, for announcements that are never
/// answered.
pub fn fake_txid(n: u64) -> Txid {
    let mut bytes = [0u8; 32];
    bytes[..8].copy_from_slice(&n.to_le_bytes());
    bytes[31] = 0x42;
    Txid::from_byte_array(bytes)
}
