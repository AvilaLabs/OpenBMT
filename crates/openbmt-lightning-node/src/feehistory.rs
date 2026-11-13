//! The fee history of a node that has no Bitcoin Core (SL-26a): samples of the
//! blocks the watcher downloads, kept as the owner's device keeps them
//! (`docs/FEE-RULES.md` FE-20). The history type, its rates and its stored
//! form are `openbmt_watch`'s; this file only decides which blocks give a
//! sample, keeps the file, and tells [`CoreFees`] what the history says.

use crate::chain::CoreFees;
use bitcoin::consensus::deserialize;
use bitcoin::{Block, BlockHash, Network};
use openbmt_watch::fees::SAMPLE_EVERY_BLOCKS;
use openbmt_watch::{block_fee_sample, FeeHistory};
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// The history's file in the node's directory.
pub const FEE_FILE: &str = "fee-history.bin";
/// A file larger than this is not read.
const MAX_FEE_FILE_BYTES: u64 = 64 * 1024;

/// The history, its file, and the one block asked for by hash.
pub struct FeeKeeper {
    dir: PathBuf,
    network: Network,
    history: FeeHistory,
    /// The block asked for by hash and its height, while it is outstanding.
    request: Option<(BlockHash, u32)>,
    fees: Arc<CoreFees>,
}

/// Writes the file whole to a temporary one, flushes it, renames it over the
/// old one and flushes the directory.
fn write_file(dir: &Path, name: &str, bytes: &[u8]) -> Result<(), String> {
    let temp = dir.join(format!("{name}.tmp"));
    let written = File::create(&temp).and_then(|mut f| {
        f.write_all(bytes)?;
        f.sync_all()
    });
    written
        .and_then(|()| fs::rename(&temp, dir.join(name)))
        .and_then(|()| File::open(dir).and_then(|d| d.sync_all()))
        .map_err(|e| format!("{name}: {e}"))
}

/// Reads the stored history strictly. A missing file is an empty history; a
/// file that is too large, cut, changed or unreadable is an empty history and
/// a line for the log.
pub fn read_history(dir: &Path) -> (FeeHistory, Option<String>) {
    let path = dir.join(FEE_FILE);
    let empty = |why: String| {
        (
            FeeHistory::new(),
            Some(format!("{FEE_FILE} {why}; the fee history starts empty")),
        )
    };
    match fs::metadata(&path) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => return (FeeHistory::new(), None),
        Err(e) => return empty(e.to_string()),
        Ok(meta) if meta.len() > MAX_FEE_FILE_BYTES => return empty("is too large".to_string()),
        Ok(_) => {}
    }
    match fs::read(&path)
        .map_err(|e| e.to_string())
        .and_then(|bytes| {
            FeeHistory::from_bytes(&bytes).map_err(|e| format!("does not read back: {e}"))
        }) {
        Ok(history) => (history, None),
        Err(why) => empty(why),
    }
}

impl FeeKeeper {
    /// Reads the file and gives [`CoreFees`] what it says at `tip`. The lines
    /// are for the node's log.
    pub fn load(
        dir: &Path,
        network: Network,
        fees: Arc<CoreFees>,
        tip: u32,
    ) -> (Self, Vec<String>) {
        let (history, note) = read_history(dir);
        fees.use_history(history.clone(), tip);
        let keeper = Self {
            dir: dir.to_path_buf(),
            network,
            history,
            request: None,
            fees,
        };
        (keeper, note.into_iter().collect())
    }

    /// The samples held.
    pub fn history(&self) -> &FeeHistory {
        &self.history
    }

    /// Drops the samples of blocks that the chain no longer holds at their
    /// height (the chain may have been reorganized while the node was off).
    pub fn prune(&mut self, held: impl Fn(u32) -> Option<BlockHash>, tip: u32) -> Vec<String> {
        let mut next = FeeHistory::new();
        for sample in self.history.samples() {
            if held(sample.height).is_none_or(|h| h == sample.block) {
                next.insert(*sample);
            }
        }
        if next == self.history {
            return Vec::new();
        }
        let mut notes = vec!["fee samples of blocks that left the chain were dropped".to_string()];
        notes.extend(self.commit(next, tip));
        notes
    }

    /// Stores `next` and only then swaps it in; with a line for the log if it
    /// could not be stored, and the history as it was.
    fn commit(&mut self, next: FeeHistory, tip: u32) -> Option<String> {
        match write_file(&self.dir, FEE_FILE, &next.to_bytes()) {
            Ok(()) => {
                self.history = next;
                self.fees.use_history(self.history.clone(), tip);
                None
            }
            Err(e) => Some(format!(
                "the fee history could not be stored: {e}; it is as it was"
            )),
        }
    }

    /// A sample from a block downloaded in full and checked by the driver, at
    /// the height it has on the best chain.
    pub fn take(&mut self, hash: &BlockHash, height: u32, bytes: &[u8], tip: u32) -> Vec<String> {
        if self.request.map(|(h, _)| h) == Some(*hash) {
            self.request = None;
        }
        let block: Block = match deserialize(bytes) {
            Ok(b) => b,
            Err(_) => return Vec::new(),
        };
        let sample = match block_fee_sample(&block, height, self.network) {
            Ok(s) => s,
            Err(e) => return vec![format!("block {hash} gave no fee sample: {e}")],
        };
        let mut next = self.history.clone();
        if next.insert(sample) {
            return self.commit(next, tip).into_iter().collect();
        }
        Vec::new()
    }

    /// The best chain was reorganized above `from`: the samples at and above
    /// it go, and a block asked for that left is not waited for.
    pub fn forget_from(&mut self, from: u32, left: &[BlockHash], tip: u32) -> Vec<String> {
        if self
            .request
            .is_some_and(|(hash, height)| height >= from || left.contains(&hash))
        {
            self.request = None;
        }
        let mut next = self.history.clone();
        next.forget_from(from);
        if next != self.history {
            return self.commit(next, tip).into_iter().collect();
        }
        Vec::new()
    }

    /// The block to ask the driver for, if one is due (FE-20): when no sample
    /// is held in the last 24 blocks and none is outstanding, the tip block.
    /// A request 24 blocks old is let go and asked again.
    pub fn want_block(&mut self, tip_height: u32, tip_hash: BlockHash) -> Option<BlockHash> {
        if let Some((_, asked)) = self.request {
            if tip_height < asked.saturating_add(SAMPLE_EVERY_BLOCKS) {
                return None;
            }
            self.request = None;
        }
        let recent = self
            .history
            .samples()
            .iter()
            .rev()
            .find(|s| s.height <= tip_height)
            .map(|s| s.height);
        if recent.is_some_and(|h| tip_height.saturating_sub(h) < SAMPLE_EVERY_BLOCKS) {
            return None;
        }
        self.request = Some((tip_hash, tip_height));
        Some(tip_hash)
    }

    /// Tells [`CoreFees`] the tip has moved, so its windows move with it.
    pub fn tip_moved(&self, tip: u32) {
        self.fees.use_history(self.history.clone(), tip);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::temp_dir;
    use bitcoin::absolute::LockTime;
    use bitcoin::hashes::Hash;
    use bitcoin::transaction::Version;
    use bitcoin::{Amount, OutPoint, ScriptBuf, Sequence, Transaction, TxIn, TxOut, Witness};
    use openbmt_watch::FeeSample;

    fn sample(height: u32) -> FeeSample {
        FeeSample {
            height,
            block: BlockHash::from_byte_array([height as u8; 32]),
            fees_sats: 100,
            vsize: 1000,
            claimed_less: false,
            rate_milli: Some(5_000),
        }
    }

    fn keeper(name: &str) -> (FeeKeeper, PathBuf, Arc<CoreFees>) {
        let dir = temp_dir(name);
        std::fs::create_dir_all(&dir).unwrap();
        let fees = Arc::new(CoreFees::default());
        let (k, notes) = FeeKeeper::load(&dir, Network::Regtest, Arc::clone(&fees), 0);
        assert!(notes.is_empty());
        (k, dir, fees)
    }

    fn tx(inputs: Vec<OutPoint>, sats: u64) -> Transaction {
        Transaction {
            version: Version::TWO,
            lock_time: LockTime::ZERO,
            input: inputs
                .into_iter()
                .map(|previous_output| TxIn {
                    previous_output,
                    script_sig: ScriptBuf::new(),
                    sequence: Sequence::MAX,
                    witness: Witness::new(),
                })
                .collect(),
            output: vec![TxOut {
                value: Amount::from_sat(sats),
                script_pubkey: ScriptBuf::new(),
            }],
        }
    }

    /// A block whose coinbase claims the regtest subsidy and 1,000 sat of fees,
    /// with one more transaction.
    fn block() -> Vec<u8> {
        let coinbase = tx(vec![OutPoint::null()], 5_000_000_000 + 1_000);
        let other = tx(
            vec![OutPoint {
                txid: bitcoin::Txid::from_byte_array([1; 32]),
                vout: 0,
            }],
            10,
        );
        let mut b = Block {
            header: bitcoin::block::Header {
                version: bitcoin::block::Version::ONE,
                prev_blockhash: BlockHash::all_zeros(),
                merkle_root: bitcoin::TxMerkleNode::all_zeros(),
                time: 0,
                bits: bitcoin::CompactTarget::from_consensus(0x207f_ffff),
                nonce: 0,
            },
            txdata: vec![coinbase, other],
        };
        b.header.merkle_root = b.compute_merkle_root().unwrap();
        bitcoin::consensus::serialize(&b)
    }

    #[test]
    fn sl_26a_a_block_gives_a_sample_that_is_stored_and_read_back() {
        let (mut k, dir, fees) = keeper("fee-roundtrip");
        let bytes = block();
        let hash = deserialize::<Block>(&bytes).unwrap().block_hash();
        assert!(k.take(&hash, 10, &bytes, 10).is_empty());
        assert_eq!(k.history().samples().len(), 1);
        assert_eq!(k.history().samples()[0].fees_sats, 1_000);
        assert_eq!(k.history().samples()[0].height, 10);
        assert!(dir.join(FEE_FILE).exists());
        assert!(!dir.join(format!("{FEE_FILE}.tmp")).exists());
        // The same block again changes nothing.
        assert!(k.take(&hash, 10, &bytes, 10).is_empty());
        // A new keeper reads the file back, whole.
        let (again, notes) = FeeKeeper::load(&dir, Network::Regtest, fees, 10);
        assert!(notes.is_empty());
        assert_eq!(again.history(), k.history());
    }

    #[test]
    fn sl_26a_a_missing_or_damaged_file_is_an_empty_history_and_a_note() {
        let dir = temp_dir("fee-damaged");
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(read_history(&dir), (FeeHistory::new(), None));
        // Garbage.
        std::fs::write(dir.join(FEE_FILE), b"not a history").unwrap();
        let (h, note) = read_history(&dir);
        assert!(h.samples().is_empty());
        assert!(note.unwrap().contains("does not read back"));
        // A good file with a byte cut off, and with one changed.
        let mut good = FeeHistory::new();
        good.insert(sample(5));
        let bytes = good.to_bytes();
        std::fs::write(dir.join(FEE_FILE), &bytes[..bytes.len() - 1]).unwrap();
        assert!(read_history(&dir).1.is_some());
        let mut longer = bytes.clone();
        longer.push(0);
        std::fs::write(dir.join(FEE_FILE), &longer).unwrap();
        assert!(read_history(&dir).1.is_some());
        // Too large.
        std::fs::write(dir.join(FEE_FILE), vec![0u8; 64 * 1024 + 1]).unwrap();
        assert!(read_history(&dir).1.unwrap().contains("too large"));
        // The next sample replaces the damaged file.
        let fees = Arc::new(CoreFees::default());
        let (mut k, notes) = FeeKeeper::load(&dir, Network::Regtest, fees, 0);
        assert_eq!(notes.len(), 1);
        let bytes = block();
        let hash = deserialize::<Block>(&bytes).unwrap().block_hash();
        k.take(&hash, 3, &bytes, 3);
        assert!(read_history(&dir).1.is_none());
    }

    #[test]
    fn sl_26a_a_reorganization_forgets_the_samples_at_and_above_the_fork() {
        let (mut k, dir, _) = keeper("fee-reorg");
        let mut next = FeeHistory::new();
        for h in [10, 20, 30] {
            next.insert(sample(h));
        }
        assert!(k.commit(next, 30).is_none());
        assert!(k.forget_from(20, &[], 25).is_empty());
        let heights: Vec<u32> = k.history().samples().iter().map(|s| s.height).collect();
        assert_eq!(heights, vec![10]);
        // And the file says so too.
        assert_eq!(read_history(&dir).0, *k.history());
    }

    #[test]
    fn sl_26a_samples_of_blocks_that_left_the_chain_are_dropped_at_the_start() {
        let (mut k, dir, _) = keeper("fee-prune");
        let mut next = FeeHistory::new();
        for h in [10, 20, 30] {
            next.insert(sample(h));
        }
        k.commit(next, 30);
        // The chain holds the block of 10, another at 20, nothing known of 30.
        let held = |h: u32| {
            (h == 10)
                .then(|| sample(10).block)
                .or((h == 20).then(BlockHash::all_zeros))
        };
        let notes = k.prune(held, 30);
        assert_eq!(notes.len(), 1);
        let heights: Vec<u32> = k.history().samples().iter().map(|s| s.height).collect();
        assert_eq!(heights, vec![10, 30]);
        assert_eq!(read_history(&dir).0, *k.history());
    }

    #[test]
    fn sl_26a_a_file_that_cannot_be_written_leaves_the_history_as_it_was() {
        let (mut k, dir, _) = keeper("fee-unwritable");
        let mut one = FeeHistory::new();
        one.insert(sample(10));
        assert!(k.commit(one.clone(), 10).is_none());
        std::fs::remove_dir_all(&dir).unwrap();
        let mut two = one.clone();
        two.insert(sample(11));
        let note = k.commit(two, 11).unwrap();
        assert!(note.contains("could not be stored"));
        assert_eq!(*k.history(), one);
    }

    #[test]
    fn sl_26a_a_block_is_asked_for_when_none_was_sampled_in_24_blocks() {
        let (mut k, _, _) = keeper("fee-ask");
        let h = |n: u8| BlockHash::from_byte_array([n; 32]);
        // An empty history holds none: the tip block is asked for.
        assert_eq!(k.want_block(100, h(1)), Some(h(1)));
        // One outstanding at a time, until 24 more blocks have passed.
        assert_eq!(k.want_block(110, h(2)), None);
        assert_eq!(k.want_block(124, h(3)), Some(h(3)));
        // The block arriving ends the request, and the sample ends the need.
        let bytes = block();
        let hash = deserialize::<Block>(&bytes).unwrap().block_hash();
        k.request = Some((hash, 124));
        k.take(&hash, 124, &bytes, 124);
        assert_eq!(k.want_block(130, h(4)), None);
        assert_eq!(k.want_block(147, h(5)), None);
        assert_eq!(k.want_block(148, h(6)), Some(h(6)));
    }

    #[test]
    fn sl_26a_a_block_asked_for_that_left_is_not_waited_for() {
        let (mut k, _, _) = keeper("fee-left");
        let h = BlockHash::from_byte_array([7; 32]);
        assert_eq!(k.want_block(100, h), Some(h));
        k.forget_from(100, &[h], 100);
        assert!(k
            .want_block(101, BlockHash::from_byte_array([8; 32]))
            .is_some());
    }

    #[test]
    fn sl_26a_a_block_that_gives_no_sample_is_a_note_and_changes_nothing() {
        let (mut k, _, _) = keeper("fee-nosample");
        let hash = BlockHash::from_byte_array([9; 32]);
        assert!(k.take(&hash, 5, b"not a block", 5).is_empty());
        let mut bytes = block();
        let mut b: Block = deserialize(&bytes).unwrap();
        b.txdata[0].output[0].value = Amount::from_sat(1);
        b.header.merkle_root = b.compute_merkle_root().unwrap();
        bytes = bitcoin::consensus::serialize(&b);
        // The coinbase claims less than the subsidy: a sample of no fees, not
        // an error.
        assert!(k.take(&b.block_hash(), 5, &bytes, 5).is_empty());
        assert!(k.history().samples()[0].claimed_less);
        // A block that changed after its merkle root was made is refused.
        b.txdata[1].output[0].value = Amount::from_sat(2);
        let note = k.take(&b.block_hash(), 6, &bitcoin::consensus::serialize(&b), 6);
        assert_eq!(note.len(), 1);
        assert_eq!(k.history().samples().len(), 1);
    }
}
