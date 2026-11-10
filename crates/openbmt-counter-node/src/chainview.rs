//! The app's view of the best header chain.
//!
//! The driver's thread owns the real [`HeaderStore`]; the app cannot ask it
//! anything. [`ChainView`] answers the watcher's three questions
//! ([`HeaderChain`]) from what the app is told: the stored headers at start,
//! then every [`AppEvent::Chain`](openbmt_net_io::AppEvent::Chain) and the
//! tip time that comes with each
//! [`AppEvent::Peers`](openbmt_net_io::AppEvent::Peers).
//!
//! It holds the hash of every block on the best chain, from the start block
//! to the tip, and nothing else: not the headers, not the side chains.
//! `height_of` answers only for a block that is on the best chain now.

use std::collections::{BTreeMap, HashMap};

use bitcoin::BlockHash;
use openbmt_watch::{HeaderChain, HeaderStore};

/// The best chain as the app knows it. See the module docs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChainView {
    by_height: BTreeMap<u32, BlockHash>,
    heights: HashMap<BlockHash, u32>,
    tip_time: u64,
}

impl ChainView {
    /// The view of a header store: its best chain, walked from its start
    /// block to its tip.
    pub fn from_store(store: &HeaderStore) -> Self {
        let mut view = Self {
            by_height: BTreeMap::new(),
            heights: HashMap::new(),
            tip_time: store.tip_time(),
        };
        for height in store.start_height()..=store.tip_height() {
            if let Some(header) = store.header_at(height) {
                view.insert(height, header.block_hash());
            }
        }
        view
    }

    fn insert(&mut self, height: u32, hash: BlockHash) {
        if let Some(old) = self.by_height.insert(height, hash) {
            self.heights.remove(&old);
        }
        // A hash that was at another height moves.
        if let Some(old_height) = self.heights.insert(hash, height) {
            if old_height != height && self.by_height.get(&old_height) == Some(&hash) {
                self.by_height.remove(&old_height);
            }
        }
    }

    fn remove(&mut self, hash: &BlockHash) {
        if let Some(height) = self.heights.remove(hash) {
            if self.by_height.get(&height) == Some(hash) {
                self.by_height.remove(&height);
            }
        }
    }

    /// The best chain changed: blocks `left` it (old tip first) and blocks
    /// `joined` it (lowest first), as in the driver's event. The ones that
    /// left are taken out first, so a reorganization is one call.
    pub fn apply(&mut self, joined: &[(u32, BlockHash)], left: &[BlockHash]) {
        for hash in left {
            self.remove(hash);
        }
        for (height, hash) in joined {
            self.insert(*height, *hash);
        }
    }

    /// The time in the header at `height`, as the driver reports it with its
    /// peers. It is taken only when `height` is the tip of this view: a time
    /// that belongs to another tip would be the wrong time.
    pub fn set_tip_time(&mut self, height: u32, time: u64) {
        if height == self.tip_height() {
            self.tip_time = time;
        }
    }

    /// The hash on the best chain at this height.
    pub fn hash_at(&self, height: u32) -> Option<BlockHash> {
        self.by_height.get(&height).copied()
    }

    /// How many blocks the view holds, the start block included.
    pub fn len(&self) -> usize {
        self.by_height.len()
    }

    /// Whether the view holds no block. A view made from a header store is
    /// never empty.
    pub fn is_empty(&self) -> bool {
        self.by_height.is_empty()
    }
}

impl HeaderChain for ChainView {
    fn height_of(&self, block: &BlockHash) -> Option<u32> {
        self.heights.get(block).copied()
    }

    fn tip_height(&self) -> u32 {
        self.by_height.keys().next_back().copied().unwrap_or(0)
    }

    fn tip_time(&self) -> u64 {
        self.tip_time
    }
}
