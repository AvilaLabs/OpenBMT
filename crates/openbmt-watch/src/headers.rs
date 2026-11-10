//! Section 1: the header chain the library keeps itself (WR-04 to WR-08).
//!
//! [`HeaderStore`] holds the headers of one network from a start block on,
//! checks every header it is given and follows the chain with the most
//! work. It has no network, no clock and no storage: the caller passes in
//! the header and the time, and keeps the bytes of [`HeaderStore::to_bytes`].

use std::collections::{BTreeMap, BTreeSet};

use bitcoin::block::Header;
use bitcoin::blockdata::constants::genesis_block;
use bitcoin::consensus::{deserialize, serialize};
use bitcoin::hashes::Hash;
use bitcoin::{BlockHash, CompactTarget, Network};
use openbmt_core::wire::{self, DecodeError, Reader};

use crate::{HeaderChain, Refusal};

/// WR-08: magic of the stored form.
pub const MAGIC_HEADER_STORE: [u8; 4] = *b"OBMW";
/// WR-08: most headers a stored form may hold after the start block.
pub const MAX_STORED_HEADERS: usize = 2_000_000;
/// WR-06: seconds a header's time may be ahead of the clock.
pub const MAX_HEADER_AHEAD_SECS: u64 = 7_200;
/// WR-05: how many headers the median time is taken over.
pub const MEDIAN_TIME_SPAN: usize = 11;
/// WR-05: blocks in a difficulty period.
pub const RETARGET_INTERVAL: u32 = 2_016;
/// WR-07: how far below the tip a header off the best chain may be held.
pub const SIDE_DEPTH: u32 = 2_016;
/// WR-07: most headers held off the best chain.
pub const MAX_SIDE_HEADERS: usize = 4_032;

/// WR-04: the mainnet checkpoint: the 80-byte header of block 967,680.
pub(crate) const MAINNET_CHECKPOINT: [u8; 80] = [
    0x00, 0x00, 0x00, 0x30, 0x41, 0x0c, 0xcb, 0x1c, 0xba, 0x62, 0x3e, 0x26, 0xf2, 0xe0, 0x2f, 0x3f,
    0xcb, 0x0e, 0x7f, 0x06, 0xf6, 0x69, 0x98, 0xb6, 0x8c, 0xb5, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x3d, 0x16, 0xae, 0x25, 0x58, 0xe8, 0xec, 0x7b, 0x8d, 0xd3, 0x69, 0xee,
    0x29, 0xbb, 0x27, 0x33, 0xfb, 0xd3, 0x9b, 0xcc, 0x10, 0x58, 0xc6, 0x81, 0x97, 0x0a, 0x36, 0xa7,
    0xf1, 0xa2, 0xbd, 0xea, 0x11, 0x35, 0xae, 0x6a, 0xc5, 0x1e, 0x02, 0x17, 0x48, 0x0c, 0x35, 0x81,
];
/// WR-04: the height of the mainnet checkpoint, 480 retarget periods.
pub const MAINNET_CHECKPOINT_HEIGHT: u32 = 967_680;

/// What [`HeaderStore::accept`] did with a header it did not refuse.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Accepted {
    /// The store already held this header.
    Known,
    /// The header is the new tip of the best chain.
    Extended,
    /// The header is held, and the best chain is still the one it was.
    Side,
    /// The best chain changed to one that has more work. `left` are the
    /// blocks that left it, the old tip first.
    Reorganized {
        /// Blocks that are no longer on the best chain, old tip first.
        left: Vec<BlockHash>,
    },
}

/// A header the store holds, where it sits and the work up to it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Entry {
    header: Header,
    height: u32,
    /// Total work from the start block, big endian.
    work: [u8; 32],
}

/// WR-04 to WR-08: the headers of one network from its start block, checked
/// and followed by most work.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeaderStore {
    network: Network,
    start_height: u32,
    start_hash: BlockHash,
    /// Every header held, on the best chain or beside it.
    held: BTreeMap<BlockHash, Entry>,
    /// The best chain from the start block; never empty.
    best: Vec<BlockHash>,
    /// The held headers that are not on the best chain.
    side: BTreeSet<BlockHash>,
}

fn network_byte(network: Network) -> Option<u8> {
    match network {
        Network::Bitcoin => Some(0),
        Network::Signet => Some(1),
        Network::Regtest => Some(2),
        _ => None,
    }
}

/// Big-endian sum, `None` on overflow.
fn add_work(total: &[u8; 32], work: bitcoin::Work) -> Option<[u8; 32]> {
    let add = work.to_be_bytes();
    let mut out = [0u8; 32];
    let mut carry = 0u16;
    for i in (0..32).rev() {
        let sum = u16::from(*total.get(i)?) + u16::from(*add.get(i)?) + carry;
        *out.get_mut(i)? = (sum & 0xff) as u8;
        carry = sum >> 8;
    }
    (carry == 0).then_some(out)
}

impl HeaderStore {
    /// WR-04: a store at the start block of this network. Testnet and
    /// Testnet4 are refused.
    pub fn new(network: Network) -> Result<Self, Refusal> {
        let (header, height) = match network {
            Network::Bitcoin => (
                deserialize::<Header>(&MAINNET_CHECKPOINT)
                    .map_err(|_| Refusal::NetworkNotSupported)?,
                MAINNET_CHECKPOINT_HEIGHT,
            ),
            Network::Signet | Network::Regtest => (genesis_block(network).header, 0),
            _ => return Err(Refusal::NetworkNotSupported),
        };
        let start_hash = header.block_hash();
        let mut held = BTreeMap::new();
        held.insert(
            start_hash,
            Entry {
                header,
                height,
                work: [0; 32],
            },
        );
        Ok(Self {
            network,
            start_height: height,
            start_hash,
            held,
            best: vec![start_hash],
            side: BTreeSet::new(),
        })
    }

    /// The network of this store.
    pub fn network(&self) -> Network {
        self.network
    }

    /// Height of the start block.
    pub fn start_height(&self) -> u32 {
        self.start_height
    }

    /// The hash of the start block.
    pub fn start_hash(&self) -> BlockHash {
        self.start_hash
    }

    /// Height and hash of the tip of the best chain.
    pub fn tip(&self) -> (u32, BlockHash) {
        let hash = self.best.last().copied().unwrap_or(self.start_hash);
        (self.tip_height(), hash)
    }

    /// The hash on the best chain at this height.
    fn best_at(&self, height: u32) -> Option<&BlockHash> {
        let index = usize::try_from(height.checked_sub(self.start_height)?).ok()?;
        self.best.get(index)
    }

    /// The header on the best chain at this height, the start block
    /// included.
    pub fn header_at(&self, height: u32) -> Option<&Header> {
        self.held.get(self.best_at(height)?).map(|e| &e.header)
    }

    /// How many headers are held beside the best chain.
    pub fn side_count(&self) -> usize {
        self.side.len()
    }

    /// Whether the store holds this header, on the best chain or beside it.
    pub fn holds(&self, block: &BlockHash) -> bool {
        self.held.contains_key(block)
    }

    /// The block locator from the tip, as Bitcoin Core builds it: single
    /// steps back for the first 11 blocks, then steps that double, and the
    /// start block last.
    pub fn locator(&self) -> Vec<BlockHash> {
        let mut out = Vec::new();
        let mut height = self.tip_height();
        let mut step = 1u32;
        loop {
            if let Some(hash) = self.best_at(height) {
                out.push(*hash);
            }
            if height <= self.start_height {
                break;
            }
            height = height.saturating_sub(step).max(self.start_height);
            if out.len() > 10 {
                step = step.saturating_mul(2);
            }
        }
        out
    }

    /// WR-05 to WR-07: check this header and hold it. `now` is the caller's
    /// clock in seconds. A refusal stores nothing.
    pub fn accept(&mut self, header: &Header, now: u64) -> Result<Accepted, Refusal> {
        self.accept_inner(header, Some(now))
    }

    fn accept_inner(&mut self, header: &Header, now: Option<u64>) -> Result<Accepted, Refusal> {
        let hash = header.block_hash();
        if self.held.contains_key(&hash) {
            return Ok(Accepted::Known);
        }
        // WR-05, in this order.
        let parent = self
            .held
            .get(&header.prev_blockhash)
            .ok_or(Refusal::UnknownParent)?;
        let height = parent
            .height
            .checked_add(1)
            .ok_or(Refusal::HeightOverflow)?;
        let required = self.required_bits(parent, height)?;
        if header.bits != required {
            return Err(Refusal::WrongBits {
                expected: required.to_consensus(),
                found: header.bits.to_consensus(),
            });
        }
        header
            .validate_pow(header.target())
            .map_err(|_| Refusal::ChainProofOfWork)?;
        let median = self.median_time_past(parent)?;
        if header.time <= median {
            return Err(Refusal::TimeNotAfterMedian {
                median,
                time: header.time,
            });
        }
        // WR-06.
        if let Some(now) = now {
            if u64::from(header.time) > now.saturating_add(MAX_HEADER_AHEAD_SECS) {
                return Err(Refusal::TimeTooFarAhead { time: header.time });
            }
        }
        // WR-07.
        if parent.height.saturating_add(SIDE_DEPTH) < self.tip_height() {
            return Err(Refusal::SideHeaderTooDeep);
        }
        let work = add_work(&parent.work, header.work()).ok_or(Refusal::WorkOverflow)?;
        let better = match self.held.get(&self.tip().1) {
            Some(tip) => work > tip.work,
            None => true,
        };
        if !better && self.side_count() >= MAX_SIDE_HEADERS {
            return Err(Refusal::SideHeadersFull);
        }
        self.held.insert(
            hash,
            Entry {
                header: *header,
                height,
                work,
            },
        );
        if !better {
            self.side.insert(hash);
            return Ok(Accepted::Side);
        }
        let outcome = if self.tip().1 == header.prev_blockhash {
            self.best.push(hash);
            Accepted::Extended
        } else {
            self.switch_to(hash)
        };
        self.prune();
        Ok(outcome)
    }

    /// Make the branch that ends in `new_tip` the best chain.
    fn switch_to(&mut self, new_tip: BlockHash) -> Accepted {
        // Walk down the branch to the first block that is on the best chain.
        let mut branch = Vec::new();
        let mut cursor = new_tip;
        let mut fork_height = self.start_height;
        while let Some(entry) = self.held.get(&cursor) {
            if self.best_at(entry.height) == Some(&cursor) {
                fork_height = entry.height;
                break;
            }
            branch.push(cursor);
            cursor = entry.header.prev_blockhash;
        }
        let keep = usize::try_from(fork_height.saturating_sub(self.start_height))
            .map_or(1, |n| n.saturating_add(1));
        let mut left: Vec<BlockHash> = self.best.drain(keep..).collect();
        left.reverse();
        branch.reverse();
        self.side.extend(left.iter().copied());
        for hash in &branch {
            self.side.remove(hash);
        }
        self.best.extend(branch);
        Accepted::Reorganized { left }
    }

    /// Drop headers beside the best chain that are too far below the tip,
    /// and those whose parent was dropped.
    fn prune(&mut self) {
        if self.side.is_empty() {
            return;
        }
        let tip = self.tip_height();
        let mut side: Vec<(u32, BlockHash)> = self
            .side
            .iter()
            .filter_map(|hash| self.held.get(hash).map(|e| (e.height, *hash)))
            .collect();
        side.sort();
        for (height, hash) in side {
            let orphaned = match self.held.get(&hash) {
                Some(e) => !self.held.contains_key(&e.header.prev_blockhash),
                None => false,
            };
            if height.saturating_add(SIDE_DEPTH) < tip || orphaned {
                self.held.remove(&hash);
                self.side.remove(&hash);
            }
        }
    }

    /// WR-05: the bits a block at `height` on the branch of `parent` must
    /// state, as Bitcoin Core works them out.
    fn required_bits(&self, parent: &Entry, height: u32) -> Result<CompactTarget, Refusal> {
        if !height.is_multiple_of(RETARGET_INTERVAL) {
            return Ok(parent.header.bits);
        }
        // The first block of the period that ends at `parent`.
        let mut hash = parent.header.prev_blockhash;
        for _ in 1..RETARGET_INTERVAL - 1 {
            hash = self
                .held
                .get(&hash)
                .ok_or(Refusal::AncestorMissing)?
                .header
                .prev_blockhash;
        }
        let first = self.held.get(&hash).ok_or(Refusal::AncestorMissing)?;
        let timespan = i64::from(parent.header.time) - i64::from(first.header.time);
        Ok(CompactTarget::from_next_work_required(
            parent.header.bits,
            u64::try_from(timespan).unwrap_or(0),
            self.network.params(),
        ))
    }

    /// WR-05: the median time of `parent` and up to 10 headers before it on
    /// its own branch, back to the start block.
    fn median_time_past(&self, parent: &Entry) -> Result<u32, Refusal> {
        let mut times = Vec::with_capacity(MEDIAN_TIME_SPAN);
        let mut entry = parent;
        loop {
            times.push(entry.header.time);
            if times.len() == MEDIAN_TIME_SPAN || entry.height <= self.start_height {
                break;
            }
            entry = self
                .held
                .get(&entry.header.prev_blockhash)
                .ok_or(Refusal::AncestorMissing)?;
        }
        times.sort_unstable();
        times
            .get(times.len() / 2)
            .copied()
            .ok_or(Refusal::AncestorMissing)
    }

    /// WR-08: the stored form: the best chain after the start block.
    pub fn to_bytes(&self) -> Vec<u8> {
        let count = self.best.len().saturating_sub(1);
        let mut out = wire::start(&MAGIC_HEADER_STORE, 37 + count.saturating_mul(80));
        out.push(network_byte(self.network).unwrap_or(u8::MAX));
        out.extend_from_slice(&self.start_hash.to_byte_array());
        out.extend_from_slice(&u32::try_from(count).unwrap_or(u32::MAX).to_le_bytes());
        for hash in self.best.iter().skip(1) {
            if let Some(entry) = self.held.get(hash) {
                out.extend_from_slice(&serialize(&entry.header));
            }
        }
        out
    }

    /// WR-08: read a stored form for this network. Every header goes through
    /// the checks of WR-05 in order and must extend the tip; the clock of
    /// WR-06 is not asked. Any failure refuses the whole file.
    pub fn from_bytes(network: Network, bytes: &[u8]) -> Result<Self, Refusal> {
        let mut store = Self::new(network)?;
        let mut r = Reader::framed(bytes, &MAGIC_HEADER_STORE).map_err(Refusal::StoreMalformed)?;
        let found = r.u8().map_err(Refusal::StoreMalformed)?;
        if Some(found) != network_byte(network) {
            return Err(Refusal::StoreWrongNetwork);
        }
        let start: [u8; 32] = r.array().map_err(Refusal::StoreMalformed)?;
        if start != store.start_hash.to_byte_array() {
            return Err(Refusal::StoreWrongStart);
        }
        let count = r
            .count(MAX_STORED_HEADERS, 80)
            .map_err(Refusal::StoreMalformed)?;
        for index in 0..count {
            let raw = r.take(80).map_err(Refusal::StoreMalformed)?;
            let header: Header =
                deserialize(raw).map_err(|_| Refusal::StoreMalformed(DecodeError::Truncated))?;
            let refused = |why: Refusal| Refusal::StoreHeaderRefused {
                index: u32::try_from(index).unwrap_or(u32::MAX),
                why: Box::new(why),
            };
            if header.prev_blockhash != store.tip().1 {
                return Err(refused(Refusal::StoreNotChained));
            }
            match store.accept_inner(&header, None) {
                Ok(Accepted::Extended) => {}
                Ok(_) => return Err(refused(Refusal::StoreNotChained)),
                Err(why) => return Err(refused(why)),
            }
        }
        r.finish().map_err(Refusal::StoreMalformed)?;
        Ok(store)
    }
}

impl HeaderChain for HeaderStore {
    fn height_of(&self, block: &BlockHash) -> Option<u32> {
        let entry = self.held.get(block)?;
        (self.best_at(entry.height) == Some(block)).then_some(entry.height)
    }

    fn tip_height(&self) -> u32 {
        u32::try_from(self.best.len().saturating_sub(1))
            .map_or(u32::MAX, |n| self.start_height.saturating_add(n))
    }

    fn tip_time(&self) -> u64 {
        self.held
            .get(&self.tip().1)
            .map_or(0, |e| u64::from(e.header.time))
    }
}
