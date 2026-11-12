//! The fee samples, the line about receipts and the offer to merge them
//! (FE-20, `docs/OWNER-COINS-RULES.md` OC-16 to OC-21), on a driver that
//! the test feeds by hand.

use bitcoin::absolute::LockTime;
use bitcoin::block::{Header, Version as BlockVersion};
use bitcoin::transaction::Version;
use bitcoin::{Amount, CompactTarget, Sequence, TxIn, TxMerkleNode, TxOut, Witness};
use openbmt_core::wallet::Keychain;
use openbmt_spend::{
    consolidation_cost_sats, plan, ChangePosition, SpendRequest, MAX_CONSOLIDATION_INPUTS,
};
use openbmt_watch::fees::{subsidy, SAMPLE_EVERY_BLOCKS};
use openbmt_watch::{FeeHistory, FeeSample};

use super::*;
use crate::receipts::{
    cost_rate, merge_offer, merge_pool, receipts_line, MergeOffer, MergeRefused, ReceiptsLine,
    FEE_FILE, MAX_MERGE_PARENT_BYTES, MERGE_MIN_COINS,
};

/// A block whose coinbase claims the subsidy and `fees`, with these
/// transactions after it.
fn fee_block(prev: BlockHash, height: u32, tag: u32, txs: &[Transaction], fees: u64) -> Block {
    let mut script = height.to_le_bytes().to_vec();
    script.extend_from_slice(&tag.to_le_bytes());
    let coinbase = Transaction {
        version: Version::TWO,
        lock_time: LockTime::ZERO,
        input: vec![TxIn {
            previous_output: OutPoint::null(),
            script_sig: ScriptBuf::from_bytes(script),
            sequence: Sequence::MAX,
            witness: Witness::new(),
        }],
        output: vec![TxOut {
            value: Amount::from_sat(subsidy(height, NET) + fees),
            script_pubkey: external(0xee),
        }],
    };
    let mut all = vec![coinbase];
    all.extend(txs.iter().cloned());
    let mut block = Block {
        header: Header {
            version: BlockVersion::ONE,
            prev_blockhash: prev,
            merkle_root: TxMerkleNode::all_zeros(),
            time: bitcoin::blockdata::constants::genesis_block(NET)
                .header
                .time
                + height * 10,
            bits: CompactTarget::from_consensus(0x207f_ffff),
            nonce: 0,
        },
        txdata: all,
    };
    block.header.merkle_root = block.compute_merkle_root().unwrap();
    while block.header.validate_pow(block.header.target()).is_err() {
        block.header.nonce += 1;
    }
    block
}

/// A transaction that pays a fee and means nothing else.
fn filler(n: u8) -> Transaction {
    tx_from(
        OutPoint::new(Txid::from_byte_array([n; 32]), 0),
        &[(&external(n), 1_000)],
    )
}

/// The fees at which `txs` average `rate_milli`: exact for a rate that is a
/// multiple of 1,000, since a transaction here has no witness.
fn fees_at(txs: &[Transaction], rate_milli: u64) -> u64 {
    let weight: u64 = txs.iter().map(|t| t.weight().to_wu()).sum();
    rate_milli * weight / 4_000
}

impl Rig {
    fn next_block(&mut self, txs: &[Transaction], rate_milli: u64) -> Block {
        let height = u32::try_from(self.chain.len()).unwrap() + 1;
        let fees = fees_at(txs, rate_milli);
        let block = fee_block(self.tip_hash(), height, 0, txs, fees);
        self.chain.push(block.clone());
        block
    }

    /// A block the driver downloaded in full: it joins, is handed over and
    /// is checked. Its fees average `rate_milli`.
    pub(super) fn mine_full(&mut self, txs: &[Transaction], rate_milli: u64) -> Block {
        let block = self.next_block(txs, rate_milli);
        let height = u32::try_from(self.chain.len()).unwrap();
        self.feed(vec![
            AppEvent::Chain {
                joined: vec![(height, block.block_hash())],
                left: vec![],
            },
            AppEvent::Block {
                hash: block.block_hash(),
                bytes: serialize(&block),
            },
            AppEvent::Checked(height),
        ]);
        block
    }

    /// A block that no filter matched: it joins and is checked, and the
    /// driver never downloads it.
    pub(super) fn pass_over(&mut self) -> Block {
        let tx = [filler(9)];
        let block = self.next_block(&tx, 1_000);
        let height = u32::try_from(self.chain.len()).unwrap();
        self.feed(vec![
            AppEvent::Chain {
                joined: vec![(height, block.block_hash())],
                left: vec![],
            },
            AppEvent::Checked(height),
        ]);
        block
    }

    /// What the driver sends when it has the block asked for by hash.
    fn give_fee_block(&mut self, height: usize) {
        let block = self.chain[height - 1].clone();
        self.feed(vec![AppEvent::FeeBlock {
            hash: block.block_hash(),
            bytes: serialize(&block),
        }]);
    }

    /// The blocks the node asked the driver for by hash, in order.
    fn asked_for(&self) -> Vec<BlockHash> {
        self.calls()
            .iter()
            .filter_map(|c| c.strip_prefix("main fetch-block ")?.parse().ok())
            .collect()
    }

    fn history(&self) -> Vec<(u32, BlockHash, Option<u64>)> {
        self.node
            .fee_history()
            .samples()
            .iter()
            .map(|s| (s.height, s.block, s.rate_milli))
            .collect()
    }
}

fn stored_history(dir: &Dir) -> FeeHistory {
    FeeHistory::from_bytes(&fs::read(dir.path().join(FEE_FILE)).unwrap()).unwrap()
}

// ---- FE-20: the samples ----------------------------------------------------------------

#[test]
fn fe_20_a_sample_comes_from_each_block_downloaded_in_full_and_checked() {
    let dir = Dir::new();
    let mut rig = start(&dir, &Setup::default()).unwrap();
    let first = rig.mine_full(&[filler(1)], 2_000);
    let second = rig.mine_full(&[filler(2)], 3_000);
    let third = rig.mine_full(&[], 0);
    assert_eq!(
        rig.history(),
        vec![
            (1, first.block_hash(), Some(2_000)),
            (2, second.block_hash(), Some(3_000)),
            // A block with only a coinbase is a sample with no rate (FE-06).
            (3, third.block_hash(), None),
        ]
    );
    // The history is stored, whole, as it is held.
    assert_eq!(&stored_history(&dir), rig.node.fee_history());
    // Blocks no one downloaded give no sample.
    rig.pass_over();
    assert_eq!(rig.history().len(), 3);
}

#[test]
fn fe_20_a_block_seen_twice_is_one_sample_and_one_write() {
    let dir = Dir::new();
    let mut rig = start(&dir, &Setup::default()).unwrap();
    let block = rig.mine_full(&[filler(1)], 2_000);
    let stored = fs::read(dir.path().join(FEE_FILE)).unwrap();
    fs::remove_file(dir.path().join(FEE_FILE)).unwrap();
    // The driver hands the same block over again (a rescan).
    rig.feed(vec![AppEvent::Block {
        hash: block.block_hash(),
        bytes: serialize(&block),
    }]);
    assert_eq!(rig.history().len(), 1);
    assert!(
        !dir.path().join(FEE_FILE).exists(),
        "the same block changes nothing, so nothing is written"
    );
    drop(stored);
}

#[test]
fn fe_20_after_24_blocks_with_no_sample_one_more_full_block_is_asked_for() {
    let dir = Dir::new();
    let mut rig = start(&dir, &Setup::default()).unwrap();
    // An empty history has no sample in the last 24 blocks: the first
    // checked block is asked for.
    let first = rig.pass_over();
    assert_eq!(rig.asked_for(), vec![first.block_hash()]);
    // It comes: a sample, and the request is done.
    rig.give_fee_block(1);
    assert_eq!(rig.history().len(), 1);
    assert_eq!(rig.history()[0].0, 1);
    // 23 more blocks: 24 since the sample at 1 is not yet reached.
    for _ in 2..=SAMPLE_EVERY_BLOCKS {
        rig.pass_over();
    }
    assert_eq!(rig.asked_for().len(), 1, "no sample for 23 blocks only");
    // The 24th block after it: the newest checked block is asked for.
    let next = rig.pass_over();
    assert_eq!(rig.asked_for(), vec![first.block_hash(), next.block_hash()]);
    // The block comes from the driver, not through the coin view.
    let checked = rig.node.view().checked();
    rig.give_fee_block(usize::try_from(SAMPLE_EVERY_BLOCKS + 1).unwrap());
    assert_eq!(rig.node.view().checked(), checked);
    assert_eq!(rig.history().len(), 2);
    assert_eq!(rig.history()[1].0, SAMPLE_EVERY_BLOCKS + 1);
}

#[test]
fn fe_20_at_most_one_fee_block_is_outstanding_at_a_time() {
    let dir = Dir::new();
    let mut rig = start(&dir, &Setup::default()).unwrap();
    let first = rig.pass_over();
    assert_eq!(rig.asked_for(), vec![first.block_hash()]);
    // It does not come. No other request goes out while it is outstanding.
    for _ in 2..=SAMPLE_EVERY_BLOCKS {
        rig.pass_over();
    }
    assert_eq!(rig.asked_for(), vec![first.block_hash()]);
    // After 24 blocks unanswered it is let go and the newest block is
    // asked for in its place: still one at a time.
    let later = rig.pass_over();
    assert_eq!(
        rig.asked_for(),
        vec![first.block_hash(), later.block_hash()]
    );
    for _ in 0..3 {
        rig.pass_over();
    }
    assert_eq!(rig.asked_for().len(), 2);
}

#[test]
fn fe_20_a_block_only_in_the_window_of_the_history_is_worth_a_download() {
    let dir = Dir::new();
    let mut rig = start(&dir, &Setup::default()).unwrap();
    // The chain is announced to its tip first; the node has checked none.
    let mut blocks = Vec::new();
    for _ in 0..4_100u32 {
        blocks.push(rig.next_block(&[filler(9)], 1_000));
    }
    let joined = blocks
        .iter()
        .enumerate()
        .map(|(i, b)| (u32::try_from(i).unwrap() + 1, b.block_hash()))
        .collect();
    rig.deliver(vec![AppEvent::Chain {
        joined,
        left: vec![],
    }]);
    // A block more than 4,032 below the tip is outside every window.
    rig.deliver(vec![AppEvent::Checked(60)]);
    assert!(rig.asked_for().is_empty());
    // One inside it is not.
    rig.deliver(vec![AppEvent::Checked(4_000)]);
    assert_eq!(rig.asked_for(), vec![blocks[3_999].block_hash()]);
}

#[test]
fn fe_20_a_fee_block_that_does_not_check_or_is_not_on_the_chain_gives_no_sample() {
    let dir = Dir::new();
    let mut rig = start(&dir, &Setup::default()).unwrap();
    let first = rig.pass_over();
    let second = rig.pass_over();
    // Another block's bytes under the first's hash.
    rig.feed(vec![AppEvent::FeeBlock {
        hash: first.block_hash(),
        bytes: serialize(&second),
    }]);
    assert!(rig.history().is_empty());
    let notes = rig.node.watcher_mut().take_notes();
    assert!(
        notes.iter().any(|n| n.contains("did not check")),
        "{notes:?}"
    );
    // A block that checks and is on no chain of this node's.
    let stray = fee_block(first.block_hash(), 2, 77, &[filler(3)], 5_000);
    rig.feed(vec![AppEvent::FeeBlock {
        hash: stray.block_hash(),
        bytes: serialize(&stray),
    }]);
    assert!(rig.history().is_empty());
    // A block whose merkle root does not match its transactions.
    let mut bad = second.clone();
    bad.txdata.push(filler(4));
    rig.feed(vec![AppEvent::FeeBlock {
        hash: second.block_hash(),
        bytes: serialize(&bad),
    }]);
    assert!(rig.history().is_empty());
    // The right block gives a sample.
    rig.give_fee_block(2);
    assert_eq!(rig.history().len(), 1);
}

#[test]
fn fe_20_a_reorganization_forgets_the_samples_at_and_above_the_fork() {
    let dir = Dir::new();
    let mut rig = start(&dir, &Setup::default()).unwrap();
    let blocks: Vec<Block> = (1..=5)
        .map(|_| rig.mine_full(&[filler(1)], 2_000))
        .collect();
    assert_eq!(rig.history().len(), 5);
    // Blocks 4 and 5 leave: the old tip first.
    let new4 = fee_block(blocks[2].block_hash(), 4, 40, &[filler(2)], 7_000);
    let new5 = fee_block(new4.block_hash(), 5, 50, &[filler(2)], 7_000);
    rig.chain.truncate(3);
    rig.chain.push(new4.clone());
    rig.chain.push(new5.clone());
    rig.deliver(vec![AppEvent::Chain {
        joined: vec![(4, new4.block_hash()), (5, new5.block_hash())],
        left: vec![blocks[4].block_hash(), blocks[3].block_hash()],
    }]);
    let kept: Vec<u32> = rig.history().iter().map(|(h, _, _)| *h).collect();
    assert_eq!(kept, vec![1, 2, 3]);
    assert_eq!(&stored_history(&dir), rig.node.fee_history());
    // The blocks that took their place give their own samples.
    rig.deliver(vec![
        AppEvent::Block {
            hash: new4.block_hash(),
            bytes: serialize(&new4),
        },
        AppEvent::Block {
            hash: new5.block_hash(),
            bytes: serialize(&new5),
        },
        AppEvent::Checked(5),
    ]);
    let h = rig.history();
    assert_eq!(h.len(), 5);
    assert_eq!((h[3].0, h[3].1), (4, new4.block_hash()));
    assert_eq!((h[4].0, h[4].1), (5, new5.block_hash()));
}

#[test]
fn fe_20_a_fee_block_that_left_the_chain_is_not_waited_for() {
    let dir = Dir::new();
    let mut rig = start(&dir, &Setup::default()).unwrap();
    let first = rig.pass_over();
    assert_eq!(rig.asked_for(), vec![first.block_hash()]);
    // Block 1 leaves for another block 1: the new one is asked for, with no
    // wait for the old.
    let other = fee_block(
        bitcoin::blockdata::constants::genesis_block(NET).block_hash(),
        1,
        5,
        &[filler(5)],
        0,
    );
    rig.chain.clear();
    rig.chain.push(other.clone());
    rig.feed(vec![
        AppEvent::Chain {
            joined: vec![(1, other.block_hash())],
            left: vec![first.block_hash()],
        },
        AppEvent::Checked(1),
    ]);
    assert_eq!(
        rig.asked_for(),
        vec![first.block_hash(), other.block_hash()]
    );
}

#[test]
fn fe_20_the_history_is_stored_and_read_back_at_the_next_start() {
    let dir = Dir::new();
    let held = {
        let mut rig = start(&dir, &Setup::default()).unwrap();
        rig.mine_full(&[filler(1)], 2_000);
        rig.mine_full(&[filler(2)], 4_000);
        rig.node.fee_history().clone()
    };
    assert_eq!(held.samples().len(), 2);
    let mut rig = start(&dir, &Setup::default()).unwrap();
    assert_eq!(rig.node.fee_history(), &held);
    assert!(rig.node.watcher_mut().take_notes().is_empty());
}

#[test]
fn fe_20_a_file_that_does_not_read_back_is_a_note_and_an_empty_history() {
    for bytes in [
        b"not a fee history".to_vec(),
        Vec::new(),
        vec![0u8; 100_000],
    ] {
        let dir = Dir::new();
        fs::write(dir.path().join(FEE_FILE), &bytes).unwrap();
        let mut rig = start(&dir, &Setup::default()).unwrap();
        assert!(rig.node.fee_history().samples().is_empty());
        let notes = rig.node.watcher_mut().take_notes();
        assert!(
            notes
                .iter()
                .any(|n| n.contains(FEE_FILE) && n.contains("starts empty")),
            "{notes:?}"
        );
        // The node goes on, and its next sample replaces the file.
        rig.mine_full(&[filler(1)], 2_000);
        assert_eq!(stored_history(&dir).samples().len(), 1);
    }
    // A cut file reads the same way.
    let dir = Dir::new();
    let mut full = FeeHistory::new();
    full.insert(sample(10, 2_000));
    let bytes = full.to_bytes();
    fs::write(dir.path().join(FEE_FILE), &bytes[..bytes.len() - 1]).unwrap();
    let rig = start(&dir, &Setup::default()).unwrap();
    assert!(rig.node.fee_history().samples().is_empty());
}

#[test]
fn fe_20_samples_of_blocks_the_chain_no_longer_holds_are_dropped_at_start() {
    let dir = Dir::new();
    let mut rig = start(&dir, &Setup::default()).unwrap();
    let first = rig.mine_full(&[filler(1)], 2_000);
    rig.mine_full(&[filler(2)], 2_000);
    // The history holds another block 2, as when the chain was reorganized
    // while the node was off.
    let mut stale = rig.node.fee_history().clone();
    assert!(stale.insert(sample(2, 9_000)));
    rig.node.fees = stale;
    rig.node.prune_fee_samples();
    assert_eq!(rig.history(), vec![(1, first.block_hash(), Some(2_000))]);
    assert_eq!(&stored_history(&dir), rig.node.fee_history());
    let notes = rig.node.watcher_mut().take_notes();
    assert!(
        notes.iter().any(|n| n.contains("left the chain")),
        "{notes:?}"
    );
    // A history that the chain holds is left alone.
    let quiet_dir = Dir::new();
    let mut quiet = start(&quiet_dir, &Setup::default()).unwrap();
    quiet.mine_full(&[filler(1)], 2_000);
    quiet.node.watcher_mut().take_notes();
    quiet.node.prune_fee_samples();
    assert_eq!(quiet.history().len(), 1);
    assert!(quiet.node.watcher_mut().take_notes().is_empty());
}

fn sample(height: u32, rate_milli: u64) -> FeeSample {
    let mut hash = [0u8; 32];
    hash[..4].copy_from_slice(&height.to_le_bytes());
    FeeSample {
        height,
        block: BlockHash::from_byte_array(hash),
        fees_sats: rate_milli,
        vsize: 1_000,
        claimed_less: false,
        rate_milli: Some(rate_milli),
    }
}

// ---- the line and the offer ------------------------------------------------------------

pub(super) const TIP: u32 = 200;
pub(super) const COIN: u64 = 100_000;

/// A history of 17 older samples at `old` and 3 at the tip at `recent`
/// (thousandths of a satoshi per virtual byte), written before the node
/// starts: its recent rate is `recent`, its usual rate is `old`.
pub(super) fn write_history(dir: &Dir, old: u64, recent: u64) {
    let mut history = FeeHistory::new();
    for h in 10..27 {
        history.insert(sample(h, old));
    }
    for h in TIP - 2..=TIP {
        history.insert(sample(h, recent));
    }
    fs::write(dir.path().join(FEE_FILE), history.to_bytes()).unwrap();
}

/// A node at the tip with `n` coins of `sats` each, one on each of the
/// first `n` receive addresses, buried.
fn holding(dir: &Dir, n: u32, sats: u64, old: u64) -> Rig {
    let mut rig = start(dir, &Setup::default()).unwrap();
    let outs: Vec<(ScriptBuf, u64)> = (0..n).map(|i| (receive(i), sats)).collect();
    let refs: Vec<(&ScriptBuf, u64)> = outs.iter().map(|(s, v)| (s, *v)).collect();
    let pay = tx_from(OutPoint::new(Txid::from_byte_array([7; 32]), 0), &refs);
    rig.mine_full(&[pay], old);
    while rig.chain.len() < usize::try_from(TIP).unwrap() {
        rig.pass_over();
    }
    rig
}

fn shaped(n: u32, sats: u64, old: u64, recent: u64) -> (Dir, Rig) {
    let dir = Dir::new();
    write_history(&dir, old, recent);
    let rig = holding(&dir, n, sats, old);
    (dir, rig)
}

#[test]
fn oc_18_the_line_has_a_state_for_no_view_no_coins_no_fee_and_the_line_itself() {
    // The view has not checked up to the tip.
    let dir = Dir::new();
    let mut rig = start(&dir, &Setup::default()).unwrap();
    assert_eq!(rig.node.receipts_line(), ReceiptsLine::NoCoins);
    let block = rig.next_block(&[filler(1)], 1_000);
    rig.deliver(vec![AppEvent::Chain {
        joined: vec![(1, block.block_hash())],
        left: vec![],
    }]);
    assert_eq!(rig.node.receipts_line(), ReceiptsLine::NotReady);
    assert_eq!(rig.node.merge_offer(), None);
    // Coins, and no sample yet.
    let dir = Dir::new();
    let rig = holding(&dir, 12, COIN, 5_000);
    assert_eq!(
        rig.node.receipts_line(),
        ReceiptsLine::FeeUnknown { payments: 12 }
    );
    // Coins and a rate: the line.
    let (_dir, rig) = shaped(12, COIN, 5_000, 2_500);
    let cost = consolidation_cost_sats(12, 2_500).unwrap();
    assert_eq!(
        rig.node.receipts_line(),
        ReceiptsLine::Known {
            payments: 12,
            cost_sats: cost,
            rate_per_kvb: 2_500
        }
    );
}

#[test]
fn oc_17_the_cost_is_the_planner_s_size_at_the_rate() {
    let (_dir, rig) = shaped(12, COIN, 5_000, 2_500);
    let ReceiptsLine::Known { cost_sats, .. } = rig.node.receipts_line() else {
        panic!("{:?}", rig.node.receipts_line());
    };
    // The fee the planner gives a spend of every coin to one output.
    let wallet = &kit().wallet;
    let request = SpendRequest {
        payments: vec![(wallet.change_script(0).unwrap(), 0)],
        fee_rate: 2_500,
        change_index: 0,
        min_confirmations: 1,
        send_all: true,
        must_spend: Vec::new(),
        lock_height: 0,
        change_position: ChangePosition::Last,
    };
    let planned = plan(wallet, &rig.node.coins(), &request).unwrap();
    assert_eq!(planned.inputs().len(), 12);
    assert_eq!(cost_sats, planned.fee_sats());
    // Not 12 times the cost of one receipt: the transaction's own bytes
    // and its output are counted once.
    assert_ne!(
        cost_sats,
        12 * openbmt_spend::receipt_spend_cost_sats(2_500)
    );
}

#[test]
fn oc_17_a_rate_below_what_nodes_relay_is_worked_at_one_satoshi_per_byte() {
    assert_eq!(cost_rate(0), 1_000);
    assert_eq!(cost_rate(999), 1_000);
    assert_eq!(cost_rate(1_000), 1_000);
    assert_eq!(cost_rate(1_001), 1_001);
    let (_dir, rig) = shaped(12, COIN, 800, 200);
    assert_eq!(
        rig.node.receipts_line(),
        ReceiptsLine::Known {
            payments: 12,
            cost_sats: consolidation_cost_sats(12, 1_000).unwrap(),
            rate_per_kvb: 1_000
        }
    );
}

#[test]
fn oc_16_the_count_follows_a_receipt_and_a_spend() {
    let (_dir, mut rig) = shaped(12, COIN, 5_000, 1_000);
    let count = |rig: &Rig| match rig.node.receipts_line() {
        ReceiptsLine::Known { payments, .. } => payments,
        other => panic!("{other:?}"),
    };
    assert_eq!(count(&rig), 12);
    // A payment to the wallet.
    let pay = tx_from(
        OutPoint::new(Txid::from_byte_array([8; 32]), 0),
        &[(&receive(12), COIN)],
    );
    rig.mine_full(&[pay], 5_000);
    assert_eq!(count(&rig), 13);
    // A change coin counts too: it costs an input to spend. A refund that
    // takes one coin and gives change leaves the count where it was.
    rig.queue_refund(10_000);
    rig.spend();
    assert_eq!(count(&rig), 13);
}

#[test]
fn oc_19_the_offer_holds_when_the_fee_is_low_enough_coins_wait_and_it_saves() {
    let (_dir, rig) = shaped(12, COIN, 5_000, 1_000);
    let now = consolidation_cost_sats(12, 1_000).unwrap();
    let usual = consolidation_cost_sats(12, 5_000).unwrap();
    let offer = rig.node.merge_offer().expect("an offer");
    assert_eq!(
        offer,
        MergeOffer {
            coins: 12,
            of: 12,
            rate_per_kvb: 1_000,
            cost_sats: now,
            usual_cost_sats: usual,
        }
    );
    assert_eq!(offer.saving_sats(), usual - now);
    assert!(offer.saving_sats() > 0);
}

#[test]
fn oc_19_a_fee_that_is_not_low_hides_the_offer() {
    // Recent 6,000 against a usual 5,000: not at or below the quartile.
    let (_dir, rig) = shaped(12, COIN, 5_000, 6_000);
    assert_eq!(rig.node.merge_offer(), None);
    // The line is still there.
    assert!(matches!(
        rig.node.receipts_line(),
        ReceiptsLine::Known {
            payments: 12,
            rate_per_kvb: 6_000,
            ..
        }
    ));
}

#[test]
fn oc_19_too_little_history_hides_the_offer() {
    let dir = Dir::new();
    let rig = holding(&dir, 12, COIN, 5_000);
    assert_eq!(rig.node.fee_history().is_low(TIP), None);
    assert_eq!(rig.node.merge_offer(), None);
}

#[test]
fn oc_19_fewer_than_ten_coins_hide_the_offer() {
    let (_dir, rig) = shaped(MERGE_MIN_COINS as u32 - 1, COIN, 5_000, 1_000);
    assert_eq!(rig.node.merge_offer(), None);
    assert!(matches!(
        rig.node.receipts_line(),
        ReceiptsLine::Known { payments: 9, .. }
    ));
    // Ten is enough.
    let (_dir, rig) = shaped(MERGE_MIN_COINS as u32, COIN, 5_000, 1_000);
    assert_eq!(rig.node.merge_offer().map(|o| o.coins), Some(10));
}

#[test]
fn oc_19_a_merge_that_saves_nothing_is_not_offered() {
    // Both rates are below the floor: the cost now and at the usual fee are
    // the same.
    let (_dir, rig) = shaped(12, COIN, 800, 500);
    assert_eq!(rig.node.fee_history().is_low(TIP), Some(true));
    assert_eq!(rig.node.merge_offer(), None);
    // The same with equal rates above it.
    let (_dir, rig) = shaped(12, COIN, 3_000, 3_000);
    assert_eq!(rig.node.fee_history().is_low(TIP), Some(true));
    assert_eq!(rig.node.merge_offer(), None);
}

#[test]
fn oc_19_a_view_that_is_not_ready_hides_the_offer() {
    let (_dir, mut rig) = shaped(12, COIN, 5_000, 1_000);
    assert!(rig.node.merge_offer().is_some());
    let next = rig.next_block(&[filler(9)], 1_000);
    rig.deliver(vec![AppEvent::Chain {
        joined: vec![(TIP + 1, next.block_hash())],
        left: vec![],
    }]);
    assert_eq!(rig.node.merge_offer(), None);
}

#[test]
fn oc_20_approving_queues_one_merge_of_every_coin_and_signs_and_sends_nothing() {
    let (_dir, mut rig) = shaped(12, COIN, 5_000, 1_000);
    let offer = rig.node.merge_offer().unwrap();
    let change = kit()
        .wallet
        .change_script(rig.node.view().change_next())
        .unwrap();
    let built = rig.node.approve_merge(NOW).unwrap();
    rig.rescan();
    // One cash-out of everything, to the next change address, in a session
    // for the Key app.
    let cash = rig.node.owner().cash_outs().to_vec();
    assert_eq!(cash.len(), 1);
    assert_eq!(cash[0].script, change);
    assert_eq!(cash[0].amount, openbmt_owner::CashAmount::All);
    assert!(matches!(cash[0].stage, Stage::InSession { .. }));
    assert_eq!(rig.node.owner().open_sessions(), vec![built.session_id]);
    assert!(built.skipped.is_empty());
    // Nothing is signed or sent without the Key app.
    assert!(rig.txids().is_empty());
    assert!(rig.node.owner().outbox().is_empty());
    assert!(rig.calls().iter().all(|c| !c.contains("broadcast")));
    // It is never automatic: with no approval, no session came by itself,
    // and the offer is the same.
    assert_eq!(rig.node.merge_offer(), Some(offer));
    // The Key app approves and key 1 signs: the ordinary two-key session.
    let mut approver = key_app();
    let response = rig.answer(&mut approver, &built.request);
    let mut signer = key1();
    rig.node
        .take(&response, &mut signer, &kit().accounts[0], NOW, |_| Ok(()))
        .unwrap();
    rig.rescan();
    let txid = rig.txids().pop().expect("a transaction came out");
    let tx = rig.tx(&txid);
    assert_eq!(tx.input.len(), 12);
    assert_eq!(tx.output.len(), 1);
    assert_eq!(tx.output[0].script_pubkey, change);
    // The fee is the one the offer said.
    let paid: u64 = 12 * COIN - tx.output[0].value.to_sat();
    assert_eq!(paid, offer.cost_sats);
    assert_eq!(rig.txids().len(), 1);
    // The wallet now holds one coin, and the offer is gone.
    assert!(matches!(
        rig.node.receipts_line(),
        ReceiptsLine::Known { payments: 1, .. }
    ));
    assert_eq!(rig.node.merge_offer(), None);
    assert_eq!(rig.node.approve_merge(NOW), Err(MergeRefused::NoOffer));
    assert!(rig
        .calls()
        .iter()
        .any(|c| *c == format!("main broadcast {txid}")));
}

#[test]
fn oc_20_without_an_offer_nothing_is_queued() {
    let (_dir, mut rig) = shaped(12, COIN, 5_000, 6_000);
    assert_eq!(rig.node.approve_merge(NOW), Err(MergeRefused::NoOffer));
    assert!(rig.node.owner().cash_outs().is_empty());
    assert!(rig.node.owner().open_sessions().is_empty());
}

#[test]
fn oc_20_waiting_refunds_and_cash_outs_stop_the_merge_and_leave_the_queue() {
    let dir = Dir::new();
    write_history(&dir, 5_000, 1_000);
    let mut rig = start(&dir, &Setup::default()).unwrap();
    rig.queue_refund(10_000);
    let outs: Vec<(ScriptBuf, u64)> = (0..12).map(|i| (receive(i), COIN)).collect();
    let refs: Vec<(&ScriptBuf, u64)> = outs.iter().map(|(s, v)| (s, *v)).collect();
    let pay = tx_from(OutPoint::new(Txid::from_byte_array([7; 32]), 0), &refs);
    rig.mine_full(&[pay], 5_000);
    while rig.chain.len() < usize::try_from(TIP).unwrap() {
        rig.pass_over();
    }
    assert!(rig.node.merge_offer().is_some());
    assert_eq!(
        rig.node.approve_merge(NOW),
        Err(MergeRefused::OtherPaymentsWaiting)
    );
    assert!(rig.node.owner().cash_outs().is_empty());
    assert!(rig.node.owner().open_sessions().is_empty());
    assert_eq!(rig.node.owner().refunds().len(), 1);
    // A cash-out waiting stops it the same way.
    let refund = rig.node.owner().refunds()[0].id;
    rig.node
        .owner_mut()
        .unwrap()
        .cancel(refund, |_| Ok::<(), String>(()))
        .unwrap();
    rig.node
        .owner_mut()
        .unwrap()
        .queue_cash_out(
            external(5),
            openbmt_owner::CashAmount::Sats(20_000),
            "Mia",
            |_| Ok::<(), String>(()),
        )
        .unwrap();
    assert_eq!(
        rig.node.approve_merge(NOW),
        Err(MergeRefused::OtherPaymentsWaiting)
    );
    assert_eq!(rig.node.owner().cash_outs().len(), 1);
}

#[test]
fn oc_20_a_merge_the_session_cannot_take_is_refused_and_the_queue_is_as_it_was() {
    // 12 coins of 100 sats: the fee of the merge is more than they hold.
    let (_dir, mut rig) = shaped(12, 100, 5_000, 1_000);
    assert!(rig.node.merge_offer().is_some());
    let refused = rig.node.approve_merge(NOW).unwrap_err();
    assert!(matches!(refused, MergeRefused::NotBuilt(_)), "{refused:?}");
    // The entry and the session without it are gone; only the number the
    // queue gave the entry is used up.
    assert!(rig.node.owner().cash_outs().is_empty());
    assert!(rig.node.owner().open_sessions().is_empty());
    assert!(rig.node.owner().refunds().is_empty());
    assert!(rig.txids().is_empty());
    assert!(rig.calls().iter().all(|c| !c.contains("broadcast")));
}

// ---- which coins one merge takes -------------------------------------------------------

fn coin(n: u32, confirmations: u32, parent_bytes: usize) -> Coin {
    let mut prev = filler(0);
    prev.input[0].previous_output = OutPoint::new(Txid::from_byte_array([1; 32]), n);
    // Pad the parent with a long script so that its size is known.
    prev.output[0].script_pubkey = ScriptBuf::from_bytes(vec![0x51; parent_bytes]);
    let mut id = [0u8; 32];
    id[..4].copy_from_slice(&n.to_be_bytes());
    Coin {
        outpoint: OutPoint::new(Txid::from_byte_array(id), 0),
        prev_tx: prev,
        vout: 0,
        chain: Keychain::Receive,
        index: n,
        confirmations,
    }
}

#[test]
fn oc_21_a_merge_takes_the_oldest_coins_up_to_the_most_one_transaction_holds() {
    let coins: Vec<Coin> = (0..450).map(|n| coin(n, 1 + n % 7, 50)).collect();
    let (pool, of) = merge_pool(coins, 1);
    assert_eq!(of, 450);
    assert_eq!(pool.len(), MAX_CONSOLIDATION_INPUTS);
    // Oldest first: no coin left out is older than one taken.
    let weakest = pool.iter().map(|c| c.confirmations).min().unwrap();
    let left_out = 450 - MAX_CONSOLIDATION_INPUTS;
    let older_left_out = (0..450u32)
        .map(|n| 1 + n % 7)
        .filter(|c| *c > weakest)
        .count();
    assert_eq!(
        pool.iter().filter(|c| c.confirmations > weakest).count(),
        older_left_out,
        "every coin older than the youngest taken is taken"
    );
    assert_eq!(pool.len() + left_out, 450);
    assert!(pool.windows(2).all(|w| {
        (
            std::cmp::Reverse(w[0].confirmations),
            w[0].outpoint.txid.to_byte_array(),
        ) <= (
            std::cmp::Reverse(w[1].confirmations),
            w[1].outpoint.txid.to_byte_array(),
        )
    }));
}

#[test]
fn oc_21_a_merge_stops_at_the_bytes_of_previous_transactions_the_signer_reads() {
    let big = MAX_MERGE_PARENT_BYTES / 5 + 1_000;
    let coins: Vec<Coin> = (0..20).map(|n| coin(n, 6, big)).collect();
    let (pool, of) = merge_pool(coins, 1);
    assert_eq!(of, 20);
    assert!(pool.len() >= 4 && pool.len() <= 5, "{}", pool.len());
    let bytes: usize = pool.iter().map(|c| c.prev_tx.total_size()).sum();
    assert!(bytes <= MAX_MERGE_PARENT_BYTES);
    // A first coin that alone is over the cap is still taken: one merge of
    // one coin.
    let (pool, _) = merge_pool(vec![coin(0, 6, MAX_MERGE_PARENT_BYTES + 10)], 1);
    assert_eq!(pool.len(), 1);
}

#[test]
fn oc_21_coins_too_young_to_spend_are_not_taken_in_and_not_counted() {
    let coins: Vec<Coin> = (0..12)
        .map(|n| coin(n, if n < 4 { 0 } else { 3 }, 50))
        .collect();
    let (pool, of) = merge_pool(coins, 1);
    assert_eq!((pool.len(), of), (8, 8));
    // Eight is below the least the offer needs, though the wallet holds 12.
    assert_eq!(
        merge_offer(pool.len(), of, Some(1_000), Some(5_000), Some(true)),
        None
    );
}

#[test]
fn oc_19_the_offer_needs_every_condition() {
    let some = |merged, of, now, usual, low| merge_offer(merged, of, now, usual, low);
    assert!(some(12, 12, Some(1_000), Some(5_000), Some(true)).is_some());
    assert_eq!(some(12, 12, Some(1_000), Some(5_000), Some(false)), None);
    assert_eq!(some(12, 12, Some(1_000), Some(5_000), None), None);
    assert_eq!(some(12, 12, None, Some(5_000), Some(true)), None);
    assert_eq!(some(12, 12, Some(1_000), None, Some(true)), None);
    assert_eq!(some(9, 9, Some(1_000), Some(5_000), Some(true)), None);
    assert_eq!(some(12, 12, Some(5_000), Some(5_000), Some(true)), None);
    assert_eq!(some(12, 12, Some(6_000), Some(5_000), Some(true)), None);
    // The coins a merge takes are fewer than the wallet holds when they
    // do not fit one transaction: the cost is that of the merge.
    let capped = some(400, 450, Some(1_000), Some(5_000), Some(true)).unwrap();
    assert_eq!((capped.coins, capped.of), (400, 450));
    assert_eq!(
        capped.cost_sats,
        consolidation_cost_sats(400, 1_000).unwrap()
    );
}

#[test]
fn oc_18_the_line_in_every_state_from_the_pure_function() {
    assert_eq!(receipts_line(false, 5, Some(2_000)), ReceiptsLine::NotReady);
    assert_eq!(receipts_line(true, 0, Some(2_000)), ReceiptsLine::NoCoins);
    assert_eq!(
        receipts_line(true, 5, None),
        ReceiptsLine::FeeUnknown { payments: 5 }
    );
    assert_eq!(
        receipts_line(true, 5, Some(2_000)),
        ReceiptsLine::Known {
            payments: 5,
            cost_sats: consolidation_cost_sats(5, 2_000).unwrap(),
            rate_per_kvb: 2_000
        }
    );
    // More coins than one transaction holds are counted in rounds.
    assert_eq!(
        receipts_line(true, 1_000, Some(1_000)),
        ReceiptsLine::Known {
            payments: 1_000,
            cost_sats: consolidation_cost_sats(1_000, 1_000).unwrap(),
            rate_per_kvb: 1_000
        }
    );
}
