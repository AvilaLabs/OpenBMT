//! The coin view (`docs/OWNER-COINS-RULES.md`, OC-01 to OC-09): coins from
//! checked blocks, spends, change, reorganizations, the marks and the
//! stored form.

use bitcoin::absolute::LockTime;
use bitcoin::block::{Header, Version as BlockVersion};
use bitcoin::blockdata::constants::genesis_block;
use bitcoin::consensus::serialize;
use bitcoin::hashes::Hash;
use bitcoin::transaction::Version;
use bitcoin::{
    Amount, Block, BlockHash, CompactTarget, Network, OutPoint, ScriptBuf, Sequence, Transaction,
    TxIn, TxMerkleNode, TxOut, Txid, Witness,
};
use openbmt_core::wallet::Keychain;
use openbmt_counter_node::ChainView;
use openbmt_watch::{HeaderChain, HeaderStore, Refusal, WellFormedBlock};

use openbmt_owner::SaleKey;

use super::counter;
use crate::coins::{
    ApplyError, CoinView, PendingError, Receipt, ReceiptState, ScriptSet, ViewState, GAP,
    MAGIC_OWNER_COINS, MAX_PENDING, MAX_PRUNED_RANGES, MAX_RECEIPTS, PRUNE_START, PRUNE_TO,
    RECEIPT_KEEP_BLOCKS, UNDO_BLOCKS,
};
use crate::facts::{verified_facts, FactsRefused};

pub(super) fn script(n: u8) -> ScriptBuf {
    let mut bytes = vec![0x00, 0x20];
    bytes.extend([n; 32]);
    ScriptBuf::from_bytes(bytes)
}

fn scripts() -> ScriptSet {
    let mut set = ScriptSet::new();
    set.insert(script(1), (Keychain::Receive, 0));
    set.insert(script(2), (Keychain::Receive, 1));
    set.insert(script(9), (Keychain::Change, 0));
    set
}

/// A transaction spending `prev` (an outpoint, or a number that makes
/// one) and paying each (script, sats).
pub(super) fn tx_from(prev: OutPoint, outs: &[(&ScriptBuf, u64)]) -> Transaction {
    Transaction {
        version: Version::TWO,
        lock_time: LockTime::ZERO,
        input: vec![TxIn {
            previous_output: prev,
            script_sig: ScriptBuf::new(),
            sequence: Sequence::MAX,
            witness: Witness::new(),
        }],
        output: outs
            .iter()
            .map(|(s, v)| TxOut {
                value: Amount::from_sat(*v),
                script_pubkey: (*s).clone(),
            })
            .collect(),
    }
}

/// A transaction from a coin nobody here knows (a foreign payment).
pub(super) fn foreign(n: u8, outs: &[(&ScriptBuf, u64)]) -> Transaction {
    tx_from(OutPoint::new(Txid::from_byte_array([n; 32]), 0), outs)
}

pub(super) fn op(tx: &Transaction, vout: u32) -> OutPoint {
    OutPoint::new(tx.compute_txid(), vout)
}

pub(super) fn coinbase(height: u32, tag: u32) -> Transaction {
    let mut s = height.to_le_bytes().to_vec();
    s.extend_from_slice(&tag.to_le_bytes());
    Transaction {
        version: Version::TWO,
        lock_time: LockTime::ZERO,
        input: vec![TxIn {
            previous_output: OutPoint::null(),
            script_sig: ScriptBuf::from_bytes(s),
            sequence: Sequence::MAX,
            witness: Witness::new(),
        }],
        output: vec![TxOut {
            value: Amount::from_sat(5_000_000_000),
            script_pubkey: script(0xee),
        }],
    }
}

pub(super) fn mine(prev: BlockHash, height: u32, tag: u32, txs: &[Transaction]) -> Block {
    let mut all = vec![coinbase(height, tag)];
    all.extend(txs.iter().cloned());
    let mut block = Block {
        header: Header {
            version: BlockVersion::ONE,
            prev_blockhash: prev,
            merkle_root: TxMerkleNode::all_zeros(),
            time: genesis_block(Network::Regtest).header.time + height * 10,
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

/// A best chain of blocks the tests build, with the view of it that the
/// node keeps.
struct Chain {
    blocks: Vec<Block>,
    view: ChainView,
}

impl Chain {
    fn new() -> Self {
        let store = HeaderStore::new(Network::Regtest).unwrap();
        Self {
            blocks: Vec::new(),
            view: ChainView::from_store(&store),
        }
    }

    fn tip_hash(&self) -> BlockHash {
        self.blocks.last().map_or_else(
            || genesis_block(Network::Regtest).block_hash(),
            |b| b.block_hash(),
        )
    }

    /// Mine one more block with these transactions.
    fn push(&mut self, txs: &[Transaction]) -> Block {
        let height = u32::try_from(self.blocks.len()).unwrap() + 1;
        let block = mine(self.tip_hash(), height, 0, txs);
        self.view.apply(&[(height, block.block_hash())], &[]);
        self.blocks.push(block.clone());
        block
    }

    /// Replace the blocks above `fork` with blocks holding these
    /// transactions (one block for each entry). Returns the hashes that left,
    /// old tip first, and the new blocks.
    fn reorg(&mut self, fork: u32, specs: &[Vec<Transaction>]) -> (Vec<BlockHash>, Vec<Block>) {
        let keep = usize::try_from(fork).unwrap();
        let left: Vec<BlockHash> = self.blocks[keep..]
            .iter()
            .rev()
            .map(Block::block_hash)
            .collect();
        self.blocks.truncate(keep);
        let mut joined = Vec::new();
        let mut made = Vec::new();
        for (i, txs) in specs.iter().enumerate() {
            let height = fork + u32::try_from(i).unwrap() + 1;
            let block = mine(self.tip_hash(), height, 7, txs);
            joined.push((height, block.block_hash()));
            self.blocks.push(block.clone());
            made.push(block);
        }
        self.view.apply(&joined, &left);
        (left, made)
    }

    fn height(&self) -> u32 {
        self.view.tip_height()
    }
}

/// A place the view is stored, that can be made to fail.
#[derive(Default)]
struct Disk {
    bytes: Vec<u8>,
    writes: usize,
    fail_at: Option<usize>,
}

impl Disk {
    fn put(&mut self, bytes: &[u8]) -> Result<(), &'static str> {
        let n = self.writes;
        self.writes += 1;
        if self.fail_at == Some(n) {
            return Err("the disk is full");
        }
        self.bytes = bytes.to_vec();
        Ok(())
    }
}

fn parsed(block: &Block) -> WellFormedBlock {
    WellFormedBlock::parse(&serialize(block)).unwrap()
}

fn apply(view: &mut CoinView, chain: &Chain, block: &Block) -> crate::coins::BlockEffect {
    view.apply_block(
        &parsed(block),
        &chain.view,
        &scripts(),
        |_| Ok::<(), ()>(()),
    )
    .unwrap()
}

fn ok(_: &[u8]) -> Result<(), ()> {
    Ok(())
}

/// The outpoints and amounts that `coins` offers, sorted.
fn offered(view: &CoinView, tip: u32) -> Vec<(OutPoint, u64, u32)> {
    let mut v: Vec<_> = view
        .coins(tip)
        .iter()
        .map(|c| {
            (
                c.outpoint,
                c.output().unwrap().value.to_sat(),
                c.confirmations,
            )
        })
        .collect();
    v.sort();
    v
}

#[test]
fn oc_01_a_payment_in_a_checked_block_is_a_coin_with_confirmations_from_the_tip() {
    let mut chain = Chain::new();
    let mut view = CoinView::new(0);
    let pay = foreign(1, &[(&script(1), 50_000), (&script(77), 1)]);
    let block = chain.push(std::slice::from_ref(&pay));
    let effect = apply(&mut view, &chain, &block);
    assert_eq!(effect.created, vec![op(&pay, 0)]);
    assert_eq!(view.balance(), 50_000);
    assert_eq!(offered(&view, 1), vec![(op(&pay, 0), 50_000, 1)]);
    chain.push(&[]);
    chain.push(&[]);
    assert_eq!(
        offered(&view, chain.height()),
        vec![(op(&pay, 0), 50_000, 3)]
    );
    let coin = &view.coins(1)[0];
    assert_eq!(coin.prev_tx, pay);
    assert_eq!((coin.chain, coin.index), (Keychain::Receive, 0));
}

#[test]
fn oc_01_a_block_that_fails_its_check_is_ignored() {
    let chain = Chain::new();
    let mut view = CoinView::new(0);
    let pay = foreign(1, &[(&script(1), 50_000)]);
    // A block that was never on the best chain.
    let stray = mine(chain.tip_hash(), 1, 99, std::slice::from_ref(&pay));
    let err = view
        .apply_block(&parsed(&stray), &chain.view, &scripts(), ok)
        .unwrap_err();
    assert_eq!(err, ApplyError::Refused(Refusal::NotOnChain));
    assert!(view.is_empty());
    // Bytes that are not a block never reach the view: the parse refuses.
    let mut bytes = serialize(&stray);
    let last = bytes.len() - 1;
    bytes[last] ^= 1;
    assert!(WellFormedBlock::parse(&bytes).is_err());
    // A block whose merkle root does not match its transactions.
    let mut bad = stray.clone();
    bad.header.merkle_root = TxMerkleNode::from_byte_array([3; 32]);
    assert!(WellFormedBlock::parse(&serialize(&bad)).is_err());
}

#[test]
fn oc_01_a_store_that_fails_leaves_the_view_as_it_was() {
    let mut chain = Chain::new();
    let mut view = CoinView::new(0);
    let block = chain.push(&[foreign(1, &[(&script(1), 50_000)])]);
    let before = view.clone();
    let err = view
        .apply_block(&parsed(&block), &chain.view, &scripts(), |_| Err("full"))
        .unwrap_err();
    assert_eq!(err, ApplyError::Store("full"));
    assert_eq!(view, before);
}

#[test]
fn oc_01_a_block_seen_twice_is_not_counted_twice() {
    let mut chain = Chain::new();
    let mut view = CoinView::new(0);
    let block = chain.push(&[foreign(1, &[(&script(1), 50_000)])]);
    apply(&mut view, &chain, &block);
    let once = view.clone();
    let again = apply(&mut view, &chain, &block);
    assert!(again.created.is_empty());
    assert_eq!(view, once);
}

#[test]
fn oc_04_an_input_that_spends_a_coin_in_a_checked_block_removes_it() {
    let mut chain = Chain::new();
    let mut view = CoinView::new(0);
    let pay = foreign(1, &[(&script(1), 50_000), (&script(2), 30_000)]);
    let b1 = chain.push(std::slice::from_ref(&pay));
    apply(&mut view, &chain, &b1);
    assert_eq!(view.len(), 2);
    // A spend of the first coin that pays change to the wallet.
    let spend = tx_from(op(&pay, 0), &[(&script(9), 40_000), (&script(55), 9_000)]);
    let b2 = chain.push(std::slice::from_ref(&spend));
    let effect = apply(&mut view, &chain, &b2);
    assert_eq!(effect.spent, vec![op(&pay, 0)]);
    assert_eq!(effect.created, vec![op(&spend, 0)]);
    let mut want = vec![(op(&pay, 1), 30_000, 2), (op(&spend, 0), 40_000, 1)];
    want.sort();
    assert_eq!(offered(&view, 2), want);
    assert_eq!(view.balance(), 70_000);
}

#[test]
fn oc_04_a_coin_made_and_spent_in_one_block_is_never_held() {
    let mut chain = Chain::new();
    let mut view = CoinView::new(0);
    let pay = foreign(1, &[(&script(1), 50_000)]);
    let spend = tx_from(op(&pay, 0), &[(&script(55), 49_000)]);
    let block = chain.push(&[pay, spend]);
    let effect = apply(&mut view, &chain, &block);
    assert_eq!(effect.created.len(), 1);
    assert_eq!(effect.spent.len(), 1);
    assert!(view.is_empty());
}

#[test]
fn oc_02_an_unconfirmed_foreign_transaction_is_never_a_coin() {
    // The view has no way to take a transaction that is not in a block: only
    // `add_pending` takes one, and it lists the change of its own
    // transaction. A foreign payment to a wallet script is not a coin until
    // a checked block holds it.
    let view = CoinView::new(0);
    assert!(view.coins(10).is_empty());
    assert_eq!(view.pending_out().len(), 0);
    assert_eq!(view.balance(), 0);
}

#[test]
fn oc_03_the_change_of_an_own_transaction_is_offered_with_no_confirmations() {
    let mut chain = Chain::new();
    let mut view = CoinView::new(0);
    let pay = foreign(1, &[(&script(1), 50_000)]);
    let b1 = chain.push(std::slice::from_ref(&pay));
    apply(&mut view, &chain, &b1);
    let own = tx_from(op(&pay, 0), &[(&script(55), 20_000), (&script(9), 29_000)]);
    assert_eq!(view.add_pending(&own, &scripts(), ok), Ok(true));
    // Again: nothing changes.
    assert_eq!(view.add_pending(&own, &scripts(), ok), Ok(false));
    assert_eq!(
        view.pending_out().into_iter().collect::<Vec<_>>(),
        vec![op(&pay, 0)]
    );
    // The spent coin is not offered; its change is, with 0 confirmations.
    assert_eq!(offered(&view, 1), vec![(op(&own, 1), 29_000, 0)]);
    assert_eq!(view.balance(), 50_000);
    assert_eq!(view.spendable(1), 29_000);
    // A block confirms it: it is no longer pending, the change is a real
    // coin and the spent coin is gone.
    let b2 = chain.push(std::slice::from_ref(&own));
    let effect = apply(&mut view, &chain, &b2);
    assert_eq!(effect.confirmed, vec![own.compute_txid()]);
    assert!(view.pending().next().is_none());
    assert_eq!(offered(&view, 2), vec![(op(&own, 1), 29_000, 1)]);
}

#[test]
fn oc_03_a_held_transaction_is_replaced_when_a_block_spends_its_input_in_another() {
    let mut chain = Chain::new();
    let mut view = CoinView::new(0);
    let pay = foreign(1, &[(&script(1), 50_000)]);
    let b1 = chain.push(std::slice::from_ref(&pay));
    apply(&mut view, &chain, &b1);
    let mine_tx = tx_from(op(&pay, 0), &[(&script(55), 20_000), (&script(9), 29_000)]);
    view.add_pending(&mine_tx, &scripts(), ok).unwrap();
    let other = tx_from(op(&pay, 0), &[(&script(66), 49_000)]);
    let b2 = chain.push(&[other]);
    let effect = apply(&mut view, &chain, &b2);
    assert_eq!(effect.replaced, vec![mine_tx.compute_txid()]);
    assert!(effect.confirmed.is_empty());
    assert!(view.pending().next().is_none());
    assert!(view.is_empty());
}

#[test]
fn oc_03_at_most_64_transactions_are_held_pending() {
    let mut view = CoinView::new(0);
    for n in 0..MAX_PENDING {
        let t = foreign(u8::try_from(n).unwrap(), &[(&script(9), 1_000)]);
        assert_eq!(view.add_pending(&t, &scripts(), ok), Ok(true));
    }
    let t = foreign(200, &[(&script(9), 1_000)]);
    assert_eq!(
        view.add_pending(&t, &scripts(), ok),
        Err(PendingError::Full)
    );
    let first = foreign(0, &[(&script(9), 1_000)]).compute_txid();
    assert_eq!(view.drop_pending(&first, ok), Ok(true));
    assert_eq!(view.drop_pending(&first, ok), Ok(false));
}

#[test]
fn oc_05_a_reorganization_of_one_block_puts_back_what_it_spent() {
    let mut chain = Chain::new();
    let mut view = CoinView::new(0);
    let pay = foreign(1, &[(&script(1), 50_000)]);
    let b1 = chain.push(std::slice::from_ref(&pay));
    apply(&mut view, &chain, &b1);
    let before = view.clone();
    let spend = tx_from(op(&pay, 0), &[(&script(9), 40_000)]);
    let b2 = chain.push(std::slice::from_ref(&spend));
    apply(&mut view, &chain, &b2);
    assert_eq!(offered(&view, 2), vec![(op(&spend, 0), 40_000, 1)]);
    // Block 2 leaves; a block with no spend takes its place.
    let (left, new) = chain.reorg(1, &[vec![], vec![]]);
    assert_eq!(left, vec![b2.block_hash()]);
    let effect = view.reorganize(&left, 1, ok).unwrap();
    assert_eq!(effect.restored, 1);
    assert_eq!(effect.removed, 1);
    assert_eq!(offered(&view, 3), vec![(op(&pay, 0), 50_000, 3)]);
    for b in &new {
        apply(&mut view, &chain, b);
    }
    assert_eq!(view.coins(3), before.coins(3));
    assert_eq!(view.state(), ViewState::Ready);
}

#[test]
fn oc_05_a_reorganization_of_three_blocks_with_a_spend_inside_is_undone() {
    let mut chain = Chain::new();
    let mut view = CoinView::new(0);
    let pay = foreign(1, &[(&script(1), 50_000), (&script(2), 20_000)]);
    let b1 = chain.push(std::slice::from_ref(&pay));
    apply(&mut view, &chain, &b1);
    let s1 = tx_from(op(&pay, 0), &[(&script(9), 45_000)]);
    let b2 = chain.push(std::slice::from_ref(&s1));
    apply(&mut view, &chain, &b2);
    let b3 = chain.push(&[foreign(2, &[(&script(1), 7_000)])]);
    apply(&mut view, &chain, &b3);
    let s2 = tx_from(op(&s1, 0), &[(&script(2), 44_000)]);
    let b4 = chain.push(std::slice::from_ref(&s2));
    apply(&mut view, &chain, &b4);
    assert_eq!(view.len(), 3);
    // Blocks 2 to 4 leave. The branch that replaces them pays nothing.
    let (left, new) = chain.reorg(1, &[vec![], vec![], vec![], vec![]]);
    assert_eq!(left.len(), 3);
    let effect = view.reorganize(&left, 1, ok).unwrap();
    assert_eq!(effect.restored, 2);
    assert_eq!(view.len(), 2);
    assert_eq!(
        offered(&view, chain.height()),
        vec![(op(&pay, 0), 50_000, 5), (op(&pay, 1), 20_000, 5)]
    );
    assert_eq!(view.checked(), 0);
    for b in &new {
        apply(&mut view, &chain, b);
    }
    assert_eq!(view.len(), 2);
}

#[test]
fn oc_05_a_transaction_confirmed_by_a_block_that_leaves_is_held_again() {
    let mut chain = Chain::new();
    let mut view = CoinView::new(0);
    let pay = foreign(1, &[(&script(1), 50_000)]);
    let b1 = chain.push(std::slice::from_ref(&pay));
    apply(&mut view, &chain, &b1);
    let own = tx_from(op(&pay, 0), &[(&script(55), 20_000), (&script(9), 29_000)]);
    view.add_pending(&own, &scripts(), ok).unwrap();
    let b2 = chain.push(std::slice::from_ref(&own));
    apply(&mut view, &chain, &b2);
    assert!(view.pending().next().is_none());
    let (left, _) = chain.reorg(1, &[vec![], vec![]]);
    let effect = view.reorganize(&left, 1, ok).unwrap();
    assert_eq!(effect.reopened, vec![own.compute_txid()]);
    assert_eq!(view.pending().count(), 1);
    assert_eq!(offered(&view, 3), vec![(op(&own, 1), 29_000, 0)]);
}

#[test]
fn oc_06_a_reorganization_deeper_than_100_blocks_is_a_named_state() {
    let mut chain = Chain::new();
    let mut view = CoinView::new(0);
    let pay = foreign(1, &[(&script(1), 50_000)]);
    let b1 = chain.push(std::slice::from_ref(&pay));
    apply(&mut view, &chain, &b1);
    for _ in 0..UNDO_BLOCKS + 1 {
        chain.push(&[]);
    }
    view.set_checked(chain.height(), ok).unwrap();
    let specs = vec![vec![]; usize::try_from(UNDO_BLOCKS).unwrap() + 2];
    let (left, _) = chain.reorg(0, &specs);
    assert_eq!(left.len(), usize::try_from(UNDO_BLOCKS).unwrap() + 2);
    view.reorganize(&left, 0, ok).unwrap();
    assert_eq!(view.state(), ViewState::DeepReorg);
    assert_eq!(view.state().to_string(), "deep reorg: rescan");
    // Nothing is offered while it stands.
    assert!(view.coins(chain.height()).is_empty());
    // A reset starts a rescan from the birthday.
    view.reset(ok).unwrap();
    assert!(view.is_empty());
    assert_eq!(view.checked(), 0);
    assert!(matches!(view.state(), ViewState::Rescanning { .. }));
}

#[test]
fn oc_06_exactly_100_blocks_can_still_be_undone() {
    let mut chain = Chain::new();
    let mut view = CoinView::new(0);
    let pay = foreign(1, &[(&script(1), 50_000)]);
    let b1 = chain.push(&[pay]);
    apply(&mut view, &chain, &b1);
    let n = usize::try_from(UNDO_BLOCKS).unwrap();
    for _ in 0..n - 1 {
        chain.push(&[]);
    }
    // 100 blocks leave, the first of them the one that paid.
    let (left, _) = chain.reorg(0, &vec![vec![]; n + 1]);
    assert_eq!(left.len(), n);
    view.reorganize(&left, 0, ok).unwrap();
    assert_eq!(view.state(), ViewState::Ready);
    assert!(view.is_empty());
}

#[test]
fn oc_05_confirmed_transactions_leave_the_undo_horizon_after_empty_checked_blocks() {
    let mut chain = Chain::new();
    let mut view = CoinView::new(0);
    let funding = foreign(1, &[(&script(1), 50_000)]);
    let first = chain.push(std::slice::from_ref(&funding));
    apply(&mut view, &chain, &first);
    view.set_checked(1, ok).unwrap();
    let spend = tx_from(op(&funding, 0), &[(&script(2), 40_000)]);
    view.add_pending(&spend, &scripts(), ok).unwrap();
    let confirmed = chain.push(std::slice::from_ref(&spend));
    apply(&mut view, &chain, &confirmed);
    view.set_checked(2, ok).unwrap();
    assert_eq!(view.confirmed_height(&spend.compute_txid()), Some(2));

    for height in 3..=101 {
        chain.push(&[]);
        view.set_checked(height, ok).unwrap();
    }
    assert_eq!(view.confirmed_height(&spend.compute_txid()), Some(2));
    chain.push(&[]);
    view.set_checked(102, ok).unwrap();
    assert_eq!(view.confirmed_height(&spend.compute_txid()), None);
}

#[test]
fn oc_07_the_watch_set_is_the_marks_plus_the_gap() {
    let mut view = CoinView::new(100);
    assert_eq!((view.receive_watch(), view.change_watch()), (GAP, GAP));
    // Nothing above the birthday has been checked: no rescan is needed.
    assert_eq!(view.raise_marks(5, 2, ok), Ok(true));
    assert_eq!(
        (view.receive_watch(), view.change_watch()),
        (5 + GAP, 2 + GAP)
    );
    assert_eq!(view.state(), ViewState::Ready);
    // Marks never go down, and no growth is no change.
    assert_eq!(view.raise_marks(3, 1, ok), Ok(false));
    assert_eq!((view.receive_next(), view.change_next()), (5, 2));
    // With blocks checked, growth rescans nothing: the position stays and
    // the view stays ready, because the new scripts are the device's own.
    view.set_checked(150, ok).unwrap();
    assert_eq!(view.raise_marks(6, 2, ok), Ok(true));
    assert_eq!(view.state(), ViewState::Ready);
    assert_eq!(view.checked(), 150);
    assert_eq!(view.raise_marks(6, 2, ok), Ok(false));
}

#[test]
fn oc_07_mark_growth_persists_a_bounded_rewind_and_survives_write_failure() {
    let mut view = CoinView::new(0);
    view.set_checked(5, ok).unwrap();
    let before = view.clone();
    assert!(view
        .raise_marks_and_rescan(30, 2, 8, |_| Err::<(), _>("disk"))
        .is_err());
    assert_eq!(view, before);

    assert_eq!(view.raise_marks_and_rescan(30, 2, 8, ok), Ok(true));
    assert_eq!((view.receive_next(), view.change_next()), (30, 2));
    assert_eq!(view.checked(), 4);
    assert_eq!(view.state(), ViewState::Rescanning { to: 8 });
    let restored = CoinView::from_bytes(&view.to_bytes()).unwrap();
    assert_eq!(restored, view);
}

#[test]
fn oc_07_mark_growth_at_the_birthday_has_no_fictitious_future_height() {
    let mut view = CoinView::new(100);
    assert_eq!(view.raise_marks_and_rescan(3, 1, 100, ok), Ok(true));
    assert_eq!(view.checked(), 100);
    assert_eq!(view.state(), ViewState::Ready);
}

#[test]
fn oc_07_a_payment_to_a_script_that_just_entered_the_watch_set_is_found_with_no_rescan() {
    let mut chain = Chain::new();
    let mut view = CoinView::new(0);
    let first = chain.push(&[]);
    apply(&mut view, &chain, &first);
    view.set_checked(1, ok).unwrap();
    // The device hands out script 3 (receive index 2); the view's marks are
    // raised before the next block is applied.
    assert_eq!(view.raise_marks(3, 1, ok), Ok(true));
    assert_eq!(view.state(), ViewState::Ready);
    let pay = foreign(1, &[(&script(3), 12_000)]);
    let next = chain.push(std::slice::from_ref(&pay));
    let mut more = scripts();
    more.insert(script(3), (Keychain::Receive, 2));
    view.apply_block(&parsed(&next), &chain.view, &more, ok)
        .unwrap();
    view.set_checked(2, ok).unwrap();
    assert_eq!(view.state(), ViewState::Ready);
    assert_eq!(offered(&view, 2), vec![(op(&pay, 0), 12_000, 1)]);
}

#[test]
fn oc_07_growth_larger_than_the_gap_is_no_rescan_and_the_last_address_is_watched() {
    let mut chain = Chain::new();
    let mut view = CoinView::new(0);
    let first = chain.push(&[]);
    apply(&mut view, &chain, &first);
    view.set_checked(1, ok).unwrap();
    // A batch of several hundred addresses in one step.
    assert_eq!(view.raise_marks(500, 1, ok), Ok(true));
    assert_eq!(view.state(), ViewState::Ready);
    assert_eq!(view.checked(), 1);
    assert_eq!(view.receive_watch(), 500 + GAP);
    let pay = foreign(2, &[(&script(77), 9_000)]);
    chain.push(&[]);
    let later = chain.push(std::slice::from_ref(&pay));
    let mut more = scripts();
    more.insert(script(77), (Keychain::Receive, 499));
    view.apply_block(&parsed(&later), &chain.view, &more, ok)
        .unwrap();
    assert_eq!(offered(&view, 3), vec![(op(&pay, 0), 9_000, 1)]);
    assert_eq!(view.coins(3)[0].index, 499);
}

#[test]
fn oc_07_a_rescan_from_the_birthday_finds_a_payment_to_a_script_added_later() {
    let mut chain = Chain::new();
    let mut view = CoinView::new(0);
    let pay = foreign(1, &[(&script(3), 12_000)]);
    let blocks = [
        chain.push(&[]),
        chain.push(std::slice::from_ref(&pay)),
        chain.push(&[]),
    ];
    // The blocks are checked against the scripts known then: nothing.
    for b in &blocks {
        apply(&mut view, &chain, b);
    }
    view.set_checked(3, ok).unwrap();
    assert!(view.is_empty());
    // A reset (a new view, a restored owner): script 3 is watched now, and
    // the blocks are read again from the birthday.
    view.reset(ok).unwrap();
    assert_eq!(view.checked(), 0);
    assert!(matches!(view.state(), ViewState::Rescanning { to: 3 }));
    let mut more = scripts();
    more.insert(script(3), (Keychain::Receive, 2));
    for b in &blocks {
        view.apply_block(&parsed(b), &chain.view, &more, ok)
            .unwrap();
    }
    view.set_checked(3, ok).unwrap();
    assert_eq!(view.state(), ViewState::Ready);
    assert_eq!(offered(&view, 3), vec![(op(&pay, 0), 12_000, 2)]);
    assert_eq!(view.coins(3)[0].index, 2);
}

#[test]
fn oc_07_a_rescan_takes_a_spend_that_the_new_script_needs() {
    // A coin on a script added later is created in block 2 and spent in
    // block 3: the rescan, in order, leaves it spent.
    let mut chain = Chain::new();
    let mut view = CoinView::new(0);
    let pay = foreign(1, &[(&script(3), 12_000)]);
    let spend = tx_from(op(&pay, 0), &[(&script(55), 11_000)]);
    let blocks = [chain.push(&[]), chain.push(&[pay]), chain.push(&[spend])];
    view.set_checked(3, ok).unwrap();
    view.reset(ok).unwrap();
    let mut more = scripts();
    more.insert(script(3), (Keychain::Receive, 2));
    for b in &blocks {
        view.apply_block(&parsed(b), &chain.view, &more, ok)
            .unwrap();
    }
    view.set_checked(3, ok).unwrap();
    assert!(view.is_empty());
    assert_eq!(view.state(), ViewState::Ready);
}

fn rich_view() -> (CoinView, Chain) {
    let mut chain = Chain::new();
    let mut view = CoinView::new(0);
    let pay = foreign(1, &[(&script(1), 50_000), (&script(2), 20_000)]);
    let b1 = chain.push(std::slice::from_ref(&pay));
    apply(&mut view, &chain, &b1);
    let spend = tx_from(op(&pay, 0), &[(&script(9), 45_000)]);
    let b2 = chain.push(&[spend]);
    apply(&mut view, &chain, &b2);
    let own = tx_from(op(&pay, 1), &[(&script(55), 5_000), (&script(9), 14_000)]);
    view.add_pending(&own, &scripts(), ok).unwrap();
    view.raise_marks(3, 2, ok).unwrap();
    view.set_checked(2, ok).unwrap();
    (view, chain)
}

#[test]
fn oc_08_the_stored_form_reads_back_to_the_same_view_and_the_same_bytes() {
    let (view, _) = rich_view();
    let bytes = view.to_bytes();
    assert_eq!(&bytes[..4], &MAGIC_OWNER_COINS);
    assert_eq!(bytes[4], 1);
    let back = CoinView::from_bytes(&bytes).unwrap();
    assert_eq!(back, view);
    assert_eq!(back.to_bytes(), bytes);
    assert_eq!(
        CoinView::from_bytes(&CoinView::new(7).to_bytes()),
        Ok(CoinView::new(7))
    );
}

#[test]
fn oc_08_the_stored_form_is_strict() {
    let (view, _) = rich_view();
    let bytes = view.to_bytes();
    // Every cut is refused.
    for cut in 0..bytes.len() {
        assert!(CoinView::from_bytes(&bytes[..cut]).is_err(), "cut at {cut}");
    }
    // Bytes after the end are refused.
    let mut more = bytes.clone();
    more.push(0);
    assert!(CoinView::from_bytes(&more).is_err());
    // Another magic or version is refused.
    let mut other = bytes.clone();
    other[0] ^= 1;
    assert!(CoinView::from_bytes(&other).is_err());
    let mut other = bytes.clone();
    other[4] = 2;
    assert!(CoinView::from_bytes(&other).is_err());
    // A flip of any one byte is refused, or reads as a view whose own
    // stored form is exactly the flipped bytes.
    for at in 0..bytes.len() {
        let mut flipped = bytes.clone();
        flipped[at] ^= 0x01;
        if let Ok(v) = CoinView::from_bytes(&flipped) {
            assert_eq!(v.to_bytes(), flipped, "flip at {at}");
        }
    }
}

fn store_err(e: ApplyError<&'static str>) -> &'static str {
    match e {
        ApplyError::Store(m) => m,
        _ => "refused",
    }
}

/// The same steps with the disk failing at one write (or at none). After a
/// failure the in-memory view is what is on the disk, a restart reads the
/// disk, and the step is tried again.
fn run_steps(fail_at: Option<usize>) -> (CoinView, usize) {
    let mut chain = Chain::new();
    let mut disk = Disk {
        fail_at,
        ..Disk::default()
    };
    let mut view = CoinView::new(0);
    let pay = foreign(1, &[(&script(1), 50_000)]);
    let spend = tx_from(op(&pay, 0), &[(&script(9), 40_000)]);
    let own = tx_from(
        op(&spend, 0),
        &[(&script(55), 10_000), (&script(9), 29_000)],
    );
    let b1 = chain.push(&[pay]);
    let b2 = chain.push(&[spend]);
    let b3 = chain.push(std::slice::from_ref(&own));
    let steps = 8;
    let mut step = 0;
    let mut failures = 0;
    while step < steps {
        let result: Result<(), &'static str> = match step {
            0 => view.raise_marks(2, 1, |b| disk.put(b)).map(|_| ()),
            1 => view
                .apply_block(&parsed(&b1), &chain.view, &scripts(), |b| disk.put(b))
                .map(|_| ())
                .map_err(store_err),
            2 => view.set_checked(1, |b| disk.put(b)),
            3 => view
                .apply_block(&parsed(&b2), &chain.view, &scripts(), |b| disk.put(b))
                .map(|_| ())
                .map_err(store_err),
            4 => view
                .add_pending(&own, &scripts(), |b| disk.put(b))
                .map(|_| ())
                .map_err(|e| match e {
                    PendingError::Store(m) => m,
                    PendingError::Full => "full",
                }),
            5 => view
                .apply_block(&parsed(&b3), &chain.view, &scripts(), |b| disk.put(b))
                .map(|_| ())
                .map_err(store_err),
            6 => view.set_checked(3, |b| disk.put(b)),
            _ => view
                .reorganize(&[b3.block_hash()], 2, |b| disk.put(b))
                .map(|_| ()),
        };
        match result {
            Ok(()) => step += 1,
            Err(_) => {
                failures += 1;
                // The view did not move, and the disk is its last value.
                if disk.bytes.is_empty() {
                    assert_eq!(view, CoinView::new(0));
                } else {
                    assert_eq!(CoinView::from_bytes(&disk.bytes).unwrap(), view);
                    // The restart.
                    view = CoinView::from_bytes(&disk.bytes).unwrap();
                }
            }
        }
        if !disk.bytes.is_empty() {
            assert_eq!(CoinView::from_bytes(&disk.bytes).unwrap(), view);
        }
    }
    (view, failures)
}

#[test]
fn oc_08_a_restart_at_every_write_point_gives_the_same_view() {
    let (clean, none) = run_steps(None);
    assert_eq!(none, 0);
    for n in 0..8 {
        let (got, failures) = run_steps(Some(n));
        assert_eq!(failures, 1, "write {n}");
        assert_eq!(got, clean, "write {n}");
    }
}

#[test]
fn oc_09_depth_counts_the_tip_block_and_a_stale_tip_is_zero() {
    let mut chain = Chain::new();
    let mut view = CoinView::new(0);
    let block = chain.push(&[foreign(1, &[(&script(1), 5_000)])]);
    apply(&mut view, &chain, &block);
    let (coin, _, depth) = offered(&view, 1)[0];
    assert_eq!(depth, 1);
    assert_eq!(coin.vout, 0);
    // A tip below the coin's block (a stale tip) is 0, not a wrap.
    assert_eq!(offered(&view, 0)[0].2, 0);
}

// ---- receipts (OC-12, OC-13) ---------------------------------------------------------

fn receipt(index: u32, tx: &Transaction, vout: u32, height: u32, block: &Block) -> Receipt {
    Receipt {
        index,
        outpoint: op(tx, vout),
        sats: tx.output[vout as usize].value.to_sat(),
        height,
        block: block.block_hash(),
    }
}

#[test]
fn oc_12_a_payment_to_a_receive_script_is_a_receipt_and_change_is_not() {
    let mut chain = Chain::new();
    let mut view = CoinView::new(0);
    // Receive index 0 and 1, change index 0, and a script nobody watches.
    let pay = foreign(
        1,
        &[
            (&script(1), 50_000),
            (&script(2), 30_000),
            (&script(9), 7_000),
            (&script(77), 1),
        ],
    );
    let block = chain.push(std::slice::from_ref(&pay));
    apply(&mut view, &chain, &block);
    assert_eq!(view.receipts_at(0), &[receipt(0, &pay, 0, 1, &block)]);
    assert_eq!(view.receipts_at(1), &[receipt(1, &pay, 1, 1, &block)]);
    assert_eq!(view.receipt_count(), 2, "the change is no receipt");
    assert_eq!(view.receipt_state(), ReceiptState::Complete);
    // Another payment to the same script: both are kept, lowest outpoint first.
    let more = foreign(2, &[(&script(1), 4_000)]);
    let b2 = chain.push(std::slice::from_ref(&more));
    apply(&mut view, &chain, &b2);
    assert_eq!(view.receipts_at(0).len(), 2);
    assert_eq!(
        view.receipts_at(0).iter().map(|r| r.sats).sum::<u64>(),
        54_000
    );
    assert!(view.receipts_at(0)[0].outpoint < view.receipts_at(0)[1].outpoint);
    assert!(view.receipts_at(5).is_empty());
}

#[test]
fn oc_12_a_receipt_stays_when_its_coin_is_spent() {
    let mut chain = Chain::new();
    let mut view = CoinView::new(0);
    let pay = foreign(1, &[(&script(1), 50_000)]);
    let b1 = chain.push(std::slice::from_ref(&pay));
    apply(&mut view, &chain, &b1);
    let spend = tx_from(op(&pay, 0), &[(&script(55), 49_000)]);
    let b2 = chain.push(std::slice::from_ref(&spend));
    apply(&mut view, &chain, &b2);
    assert!(view.is_empty(), "the coin is gone");
    assert_eq!(view.receipts_at(0), &[receipt(0, &pay, 0, 1, &b1)]);
    // A coin made and spent in one block is never held, and is a receipt.
    let pay2 = foreign(2, &[(&script(2), 20_000)]);
    let spend2 = tx_from(op(&pay2, 0), &[(&script(55), 19_000)]);
    let b3 = chain.push(&[pay2.clone(), spend2]);
    apply(&mut view, &chain, &b3);
    assert!(view.is_empty());
    assert_eq!(view.receipts_at(1), &[receipt(1, &pay2, 0, 3, &b3)]);
}

#[test]
fn oc_12_a_block_seen_twice_records_one_receipt() {
    let mut chain = Chain::new();
    let mut view = CoinView::new(0);
    let block = chain.push(&[foreign(1, &[(&script(1), 50_000)])]);
    apply(&mut view, &chain, &block);
    let once = view.clone();
    apply(&mut view, &chain, &block);
    assert_eq!(view, once);
    assert_eq!(view.receipt_count(), 1);
    // The receipt of a spent coin is not recorded again either.
    let pay = foreign(2, &[(&script(2), 5_000)]);
    let spend = tx_from(op(&pay, 0), &[(&script(55), 4_000)]);
    let b2 = chain.push(&[pay, spend]);
    apply(&mut view, &chain, &b2);
    let twice = view.clone();
    apply(&mut view, &chain, &b2);
    assert_eq!(view, twice);
    assert_eq!(view.receipt_count(), 2);
}

#[test]
fn oc_12_a_reorganization_undoes_receipts_and_the_replacement_chain_records_its_own() {
    let mut chain = Chain::new();
    let mut view = CoinView::new(0);
    let first = foreign(1, &[(&script(1), 50_000)]);
    let b1 = chain.push(std::slice::from_ref(&first));
    apply(&mut view, &chain, &b1);
    // Block 2 spends the first coin and pays the same index again.
    let second = tx_from(op(&first, 0), &[(&script(1), 20_000)]);
    let b2 = chain.push(std::slice::from_ref(&second));
    apply(&mut view, &chain, &b2);
    assert_eq!(view.receipt_count(), 2);
    // Block 2 leaves: its receipt goes, the earlier one stays, and the
    // coin that block 2 spent is back.
    let replacement = foreign(3, &[(&script(1), 9_000), (&script(2), 4_000)]);
    let (left, new) = chain.reorg(1, &[vec![replacement.clone()], vec![]]);
    assert_eq!(left, vec![b2.block_hash()]);
    view.reorganize(&left, 1, ok).unwrap();
    assert_eq!(view.receipts_at(0), &[receipt(0, &first, 0, 1, &b1)]);
    assert_eq!(view.receipt_count(), 1);
    assert_eq!(view.balance(), 50_000);
    // The replacement chain puts its own receipts in.
    for b in &new {
        apply(&mut view, &chain, b);
    }
    assert_eq!(view.receipt_count(), 3);
    assert_eq!(view.receipts_at(0).len(), 2);
    assert_eq!(
        view.receipts_at(0).iter().map(|r| r.sats).sum::<u64>(),
        59_000
    );
    assert_eq!(
        view.receipts_at(1).iter().map(|r| r.sats).sum::<u64>(),
        4_000
    );
    // The same view comes out of a chain that never held the old block.
    let mut fresh = CoinView::new(0);
    for b in [&b1, &new[0], &new[1]] {
        apply(&mut fresh, &chain, b);
    }
    assert_eq!(fresh.receipts_at(0), view.receipts_at(0));
    assert_eq!(fresh.receipts_at(1), view.receipts_at(1));
}

#[test]
fn oc_12_a_block_that_left_with_a_receipt_and_no_undo_record_is_a_deep_reorg() {
    let mut chain = Chain::new();
    let mut view = CoinView::new(0);
    let pay = foreign(1, &[(&script(1), 50_000)]);
    let b1 = chain.push(std::slice::from_ref(&pay));
    apply(&mut view, &chain, &b1);
    // The coin is spent, so no coin of the view names block 1 any more.
    let spend = tx_from(op(&pay, 0), &[(&script(55), 49_000)]);
    let b2 = chain.push(std::slice::from_ref(&spend));
    apply(&mut view, &chain, &b2);
    for _ in 0..UNDO_BLOCKS + 1 {
        chain.push(&[]);
    }
    view.set_checked(chain.height(), ok).unwrap();
    assert!(view.is_empty());
    // The undo record of block 1 has left the horizon; only its receipt
    // names it. Block 1 leaving is a deep reorganization.
    view.reorganize(&[b1.block_hash()], 0, ok).unwrap();
    assert_eq!(view.state(), ViewState::DeepReorg);
}

#[test]
fn oc_12_a_reset_drops_the_receipts_and_the_rescan_finds_them_again() {
    let mut chain = Chain::new();
    let mut view = CoinView::new(0);
    let pay = foreign(1, &[(&script(1), 50_000)]);
    let b1 = chain.push(std::slice::from_ref(&pay));
    apply(&mut view, &chain, &b1);
    view.reset(ok).unwrap();
    assert_eq!(view.receipt_count(), 0);
    apply(&mut view, &chain, &b1);
    assert_eq!(view.receipts_at(0), &[receipt(0, &pay, 0, 1, &b1)]);
}

#[test]
fn oc_13_the_log_is_bounded_and_past_the_bound_it_takes_a_named_state() {
    assert_eq!(MAX_RECEIPTS, 262_144);
    let mut chain = Chain::new();
    let mut view = CoinView::new(0);
    for n in 0..MAX_RECEIPTS {
        let n = u32::try_from(n).unwrap();
        let mut txid = [0u8; 32];
        txid[..4].copy_from_slice(&n.to_le_bytes());
        assert!(view.add_receipt_for_test(Receipt {
            index: n,
            outpoint: OutPoint::new(Txid::from_byte_array(txid), 0),
            sats: 1,
            height: 1,
            block: BlockHash::from_byte_array([1; 32]),
        }));
    }
    assert_eq!(view.receipt_count(), MAX_RECEIPTS);
    assert_eq!(view.receipt_state(), ReceiptState::Complete, "at the bound");
    // One more receipt does not fit: the named state, and nothing else
    // about the view changes.
    let pay = foreign(1, &[(&script(1), 50_000)]);
    let block = chain.push(std::slice::from_ref(&pay));
    apply(&mut view, &chain, &block);
    assert_eq!(view.receipt_state(), ReceiptState::LogFull);
    assert_eq!(view.receipt_state().to_string(), "receipt log full");
    assert_eq!(view.receipt_count(), MAX_RECEIPTS);
    assert!(view.receipts_at(0).iter().all(|r| r.sats == 1));
    // The coin is tracked, and the owner can still spend it.
    assert_eq!(view.len(), 1);
    assert!(view.ready());
    assert_eq!(offered(&view, 1), vec![(op(&pay, 0), 50_000, 1)]);
    // No facts are given while the log is full.
    view.set_checked(1, ok).unwrap();
    let sale = SaleKey {
        counter: counter(),
        index: 0,
    };
    assert_eq!(
        verified_facts(&view, &super::refunds::new_owner(), sale, 1, 1),
        Err(FactsRefused::ReceiptLogFull)
    );
    // The state is stored, and read back strictly.
    let bytes = view.to_bytes();
    let back = CoinView::from_bytes(&bytes).unwrap();
    assert_eq!(back.receipt_state(), ReceiptState::LogFull);
    assert_eq!(back.to_bytes(), bytes);
    // A second block with a receipt still records nothing.
    let more = foreign(2, &[(&script(2), 5_000)]);
    let b2 = chain.push(std::slice::from_ref(&more));
    apply(&mut view, &chain, &b2);
    assert_eq!(view.receipt_count(), MAX_RECEIPTS);
    assert_eq!(view.len(), 2);
    // A reset drops the receipts and the full state.
    view.reset(ok).unwrap();
    assert_eq!(view.receipt_state(), ReceiptState::Complete);
    assert_eq!(view.receipt_count(), 0);
}

#[test]
fn oc_12_a_receipt_that_was_spent_still_counts_for_a_sales_facts() {
    let mut chain = Chain::new();
    let mut view = CoinView::new(0);
    let pay = foreign(1, &[(&script(1), 50_000)]);
    let b1 = chain.push(std::slice::from_ref(&pay));
    apply(&mut view, &chain, &b1);
    let spend = tx_from(op(&pay, 0), &[(&script(55), 49_000)]);
    let b2 = chain.push(std::slice::from_ref(&spend));
    apply(&mut view, &chain, &b2);
    for _ in 0..4 {
        chain.push(&[]);
    }
    view.set_checked(chain.height(), ok).unwrap();
    assert!(view.is_empty());
    let sale = SaleKey {
        counter: counter(),
        index: 0,
    };
    let owner = super::refunds::new_owner();
    // The payment is in block 1 and the tip is block 6: six deep.
    assert_eq!(
        verified_facts(&view, &owner, sale, chain.height(), 6),
        Ok(openbmt_owner::SaleFacts {
            received_sats: 50_000,
            reached_refund_depth: true
        })
    );
    assert!(
        !verified_facts(&view, &owner, sale, chain.height(), 7)
            .unwrap()
            .reached_refund_depth
    );
}

/// The bytes of `rich_view`, where its receipts start, and one entry's size.
fn rich_receipts() -> (Vec<u8>, usize, usize) {
    let (view, _) = rich_view();
    let bytes = view.to_bytes();
    assert_eq!(view.receipt_count(), 2);
    let at = bytes.len() - 4 - 5 - 2 * 84;
    (bytes, at, 84)
}

#[test]
fn oc_12_the_stored_receipts_are_read_strictly() {
    let (bytes, at, size) = rich_receipts();
    let back = CoinView::from_bytes(&bytes).unwrap();
    assert_eq!(back.receipts_at(0).len(), 1);
    assert_eq!(back.receipts_at(1).len(), 1);
    // The two entries, and the pieces around them.
    let head = &bytes[..at];
    let (e0, e1) = (
        &bytes[at + 5..at + 5 + size],
        &bytes[at + 5 + size..bytes.len() - 4],
    );
    let make = |flag: u8, entries: &[&[u8]]| {
        let mut v = head.to_vec();
        v.push(flag);
        v.extend_from_slice(&u32::try_from(entries.len()).unwrap().to_le_bytes());
        for e in entries {
            v.extend_from_slice(e);
        }
        v.extend_from_slice(&0u32.to_le_bytes());
        v
    };
    assert_eq!(make(0, &[e0, e1]), bytes);
    // Out of order, and listed twice.
    assert!(CoinView::from_bytes(&make(0, &[e1, e0])).is_err());
    assert!(CoinView::from_bytes(&make(0, &[e0, e0])).is_err());
    // An unspent receive coin without its receipt, unless the log is full.
    assert!(CoinView::from_bytes(&make(0, &[e0])).is_err());
    assert!(CoinView::from_bytes(&make(0, &[])).is_err());
    assert!(CoinView::from_bytes(&make(1, &[e0])).is_ok());
    // A receipt that does not match its coin.
    let mut wrong = e1.to_vec();
    wrong[40] ^= 1;
    assert!(CoinView::from_bytes(&make(0, &[e0, &wrong])).is_err());
    // A flag that is not 0 or 1, an index out of range, a count that is
    // more than the bound.
    let mut flagged = bytes.clone();
    flagged[at] = 2;
    assert!(CoinView::from_bytes(&flagged).is_err());
    let mut high = e1.to_vec();
    high[3] = 0x80;
    assert!(CoinView::from_bytes(&make(1, &[e0, &high])).is_err());
    let mut count = bytes.clone();
    count[at + 1..at + 5].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(CoinView::from_bytes(&count).is_err());
    // A cut, and a byte after the end, in the receipts.
    for cut in at..bytes.len() {
        assert!(CoinView::from_bytes(&bytes[..cut]).is_err(), "cut {cut}");
    }
    let mut more = bytes.clone();
    more.push(0);
    assert!(CoinView::from_bytes(&more).is_err());
    // A flip of one byte of the receipts is refused or reads back the same.
    for i in at..bytes.len() {
        let mut flipped = bytes.clone();
        flipped[i] ^= 0x01;
        if let Ok(v) = CoinView::from_bytes(&flipped) {
            assert_eq!(v.to_bytes(), flipped, "flip at {i}");
        }
    }
}

// ---- pruning (OC-28, OC-29) -----------------------------------------------

/// A far tip: the best chain has a block at this height, so that receipts at
/// small heights are old.
const TALL: u32 = 60_000;

fn tall(chain: &mut Chain) {
    chain
        .view
        .apply(&[(TALL, BlockHash::from_byte_array([0xaa; 32]))], &[]);
}

/// `n` receipts of coins that are spent (no coin is held for them): index
/// `first + i`, height `base + i / per`.
fn bulk(view: &mut CoinView, n: u32, first: u32, base: u32, per: u32) {
    for i in 0..n {
        let mut txid = [0xb0; 32];
        txid[..4].copy_from_slice(&(first + i).to_le_bytes());
        assert!(view.add_receipt_for_test(Receipt {
            index: first + i,
            outpoint: OutPoint::new(Txid::from_byte_array(txid), 0),
            sats: 1,
            height: base + i / per,
            block: BlockHash::from_byte_array([1; 32]),
        }));
    }
}

/// A view with one unspent old receive coin (index 1, block 1) and one
/// spent old receipt at index 0 (block 1, spent in block 2), then bulk
/// receipts: `old` old ones from index 2 up and `recent` ones just below the
/// far tip. Returns the view, the chain and the blocks 1 and 2.
fn busy_view(old: u32, recent: u32) -> (CoinView, Chain, Block, Block) {
    let mut chain = Chain::new();
    let mut view = CoinView::new(0);
    let pay = foreign(1, &[(&script(1), 50_000), (&script(2), 40_000)]);
    let b1 = chain.push(std::slice::from_ref(&pay));
    apply(&mut view, &chain, &b1);
    let spend = tx_from(op(&pay, 0), &[(&script(55), 49_000)]);
    let b2 = chain.push(std::slice::from_ref(&spend));
    apply(&mut view, &chain, &b2);
    tall(&mut chain);
    bulk(&mut view, old, 2, 5, 100);
    bulk(&mut view, recent, 1_000_000, TALL - 5, u32::MAX);
    (view, chain, b1, b2)
}

fn start() -> u32 {
    u32::try_from(PRUNE_START).unwrap()
}

#[test]
fn oc_28_only_spent_and_old_receipts_leave_oldest_first_down_to_80_percent() {
    let old = 150_000;
    let recent = start() - old;
    let (mut view, _chain, _b1, _b2) = busy_view(old, recent);
    assert!(view.receipt_count() > PRUNE_START);
    let before = view.receipt_count();
    let freed = view.prune_receipts(TALL, ok).unwrap();
    assert_eq!(view.receipt_count(), PRUNE_TO);
    assert_eq!(freed, before - PRUNE_TO);
    assert_eq!(view.receipt_state(), ReceiptState::Complete);
    // The unspent coin's receipt stays though its block is old.
    assert_eq!(view.receipts_at(1).len(), 1);
    assert_eq!(view.len(), 1);
    // Every recent receipt stays.
    let kept_recent = (1_000_000..1_000_000 + recent)
        .filter(|i| !view.receipts_at(*i).is_empty())
        .count();
    assert_eq!(kept_recent, recent as usize);
    // The oldest left first: the receipt in block 1 (index 0) and the first
    // bulk ones; the newest old ones stay.
    assert!(view.receipts_at(0).is_empty());
    assert!(view.receipts_at(2).is_empty());
    assert!(!view.receipts_at(2 + old - 1).is_empty());
    // Two ranges: index 0, and the bulk ones from 2 (index 1 lost nothing).
    assert_eq!(view.pruned_ranges(), 2);
    assert!(view.is_pruned(0) && view.is_pruned(2));
    assert!(!view.is_pruned(1), "the unspent coin's index lost nothing");
    assert!(!view.is_pruned(2 + old - 1));
    let back = CoinView::from_bytes(&view.to_bytes()).unwrap();
    assert_eq!(back, view);
}

#[test]
fn oc_28_nothing_leaves_below_90_percent() {
    let (mut view, _chain, _b1, _b2) = busy_view(1_000, 10);
    assert!(view.receipt_count() <= PRUNE_START);
    assert_eq!(view.prune_receipts(TALL, ok).unwrap(), 0);
    assert_eq!(view.pruned_ranges(), 0);
    assert!(!view.is_pruned(0));
}

#[test]
fn oc_28_a_receipt_of_a_recent_block_never_leaves() {
    // Past 90% with everything recent: only the old spent receipt of block 1
    // may go.
    let (mut view, _chain, _b1, _b2) = busy_view(0, start());
    let before = view.receipt_count();
    assert_eq!(view.prune_receipts(TALL, ok).unwrap(), 1);
    assert_eq!(view.receipt_count(), before - 1);
    assert!(view.is_pruned(0));
    // A tip that makes every receipt recent frees nothing.
    let (mut view, _chain, _b1, _b2) = busy_view(0, start());
    assert_eq!(view.prune_receipts(RECEIPT_KEEP_BLOCKS, ok).unwrap(), 0);
    assert_eq!(view.pruned_ranges(), 0);
}

#[test]
fn oc_28_a_receipt_of_an_unspent_coin_never_leaves() {
    // The receipt of block 1 at index 1 is old and its coin is held.
    let (mut view, _chain, _b1, _b2) = busy_view(0, start());
    view.prune_receipts(TALL, ok).unwrap();
    assert_eq!(view.receipts_at(1).len(), 1, "unspent, old, kept");
    assert!(!view.is_pruned(1));
    assert_eq!(view.len(), 1);
}

#[test]
fn oc_28_a_block_applied_prunes_before_it_records_and_stores_once() {
    let old = 150_000;
    let (mut view, chain, _b1, _b2) = busy_view(old, start() - old);
    assert!(view.receipt_count() > PRUNE_START);
    // The first block is applied again, as a rescan reads blocks twice.
    let mut writes = 0;
    view.apply_block(&parsed(&chain.blocks[0]), &chain.view, &scripts(), |_| {
        writes += 1;
        Ok::<(), ()>(())
    })
    .unwrap();
    assert_eq!(writes, 1);
    // The receipt of block 1 at index 0 is seen again: its coin is held
    // again until block 2, which spends it, is read.
    assert_eq!(view.receipt_count(), PRUNE_TO + 1);
    assert!(view.is_pruned(0));
}

#[test]
fn oc_13_a_log_at_the_cap_that_cannot_make_room_takes_the_named_state() {
    // The log is at the cap with recent receipts. Applying a block makes
    // room for exactly one (the old spent receipt of block 1 goes); the
    // second does not fit.
    let (mut view, mut chain, _b1, _b2) = busy_view(0, u32::try_from(MAX_RECEIPTS).unwrap() - 2);
    assert_eq!(view.receipt_count(), MAX_RECEIPTS);
    let pay = foreign(7, &[(&script(1), 10), (&script(2), 10)]);
    let b3 = chain.push(std::slice::from_ref(&pay));
    apply(&mut view, &chain, &b3);
    assert_eq!(view.receipt_state(), ReceiptState::LogFull);
    assert_eq!(view.receipt_count(), MAX_RECEIPTS);
    assert!(view.ready());
}

#[test]
fn oc_29_a_refund_for_a_pruned_index_is_refused_as_too_old_not_answered_with_zero() {
    let old = 150_000;
    let (mut view, chain, _b1, _b2) = busy_view(old, start() - old);
    view.prune_receipts(TALL, ok).unwrap();
    view.set_checked(chain.height(), ok).unwrap();
    let owner = super::refunds::new_owner();
    let sale = |index| SaleKey {
        counter: counter(),
        index,
    };
    let refused = verified_facts(&view, &owner, sale(0), chain.height(), 1);
    assert_eq!(refused, Err(FactsRefused::TooOldForFacts));
    assert!(!FactsRefused::TooOldForFacts.waits());
    assert_eq!(
        FactsRefused::TooOldForFacts.to_string(),
        "OC-29: the sale is too old for the wallet to check what was paid"
    );
    let words = FactsRefused::TooOldForFacts.words();
    assert_eq!(
        words.what,
        "This sale is too old for the wallet to check what was paid."
    );
    assert_eq!(words.next, "Decide this refund by hand.");
    // An index that lost nothing is answered, with its receipt.
    assert_eq!(
        verified_facts(&view, &owner, sale(1), chain.height(), 1)
            .unwrap()
            .received_sats,
        40_000
    );
    // The marker survives a restart.
    let back = CoinView::from_bytes(&view.to_bytes()).unwrap();
    assert_eq!(
        verified_facts(&back, &owner, sale(0), chain.height(), 1),
        Err(FactsRefused::TooOldForFacts)
    );
    // A reset forgets it with the receipts: the rescan finds them again.
    let mut reset = back;
    reset.reset(ok).unwrap();
    assert!(!reset.is_pruned(0));
    assert_eq!(reset.pruned_ranges(), 0);
}

#[test]
fn oc_29_the_marker_is_bounded_and_joins_ranges_the_safe_way() {
    // Every second index lost a receipt: more separate ranges than the
    // bound. The list stays at the bound and still covers every index that
    // lost one.
    let n = start() + 2_000;
    let (mut view, _chain, _b1, _b2) = busy_view(0, 0);
    for i in 0..n {
        let mut txid = [0xb0; 32];
        txid[..4].copy_from_slice(&i.to_le_bytes());
        view.add_receipt_for_test(Receipt {
            index: 10 + i * 2,
            outpoint: OutPoint::new(Txid::from_byte_array(txid), 0),
            sats: 1,
            height: 5,
            block: BlockHash::from_byte_array([1; 32]),
        });
    }
    view.prune_receipts(TALL, ok).unwrap();
    assert!(view.pruned_ranges() <= MAX_PRUNED_RANGES);
    assert!(view.pruned_ranges() > 1);
    for i in 0..(n - u32::try_from(PRUNE_TO).unwrap()) {
        assert!(view.is_pruned(10 + i * 2), "index {}", 10 + i * 2);
    }
    assert!(!view.is_pruned(1));
    let bytes = view.to_bytes();
    assert_eq!(CoinView::from_bytes(&bytes).unwrap().to_bytes(), bytes);
}

#[test]
fn oc_29_the_stored_ranges_are_read_strictly() {
    let old = 150_000;
    let (mut view, _chain, _b1, _b2) = busy_view(old, start() - old);
    view.prune_receipts(TALL, ok).unwrap();
    let bytes = view.to_bytes();
    let at = bytes.len() - 4 - 16;
    assert_eq!(&bytes[at..at + 4], &2u32.to_le_bytes());
    assert!(CoinView::from_bytes(&bytes).is_ok());
    let put = |ranges: &[(u32, u32)]| {
        let mut v = bytes[..at].to_vec();
        v.extend_from_slice(&u32::try_from(ranges.len()).unwrap().to_le_bytes());
        for (s, e) in ranges {
            v.extend_from_slice(&s.to_le_bytes());
            v.extend_from_slice(&e.to_le_bytes());
        }
        v
    };
    assert!(CoinView::from_bytes(&put(&[(0, 26_000)])).is_ok());
    // Start above end, out of range, touching, overlapping, out of order,
    // too many.
    assert!(CoinView::from_bytes(&put(&[(5, 4)])).is_err());
    assert!(CoinView::from_bytes(&put(&[(0, 1 << 31)])).is_err());
    assert!(CoinView::from_bytes(&put(&[(0, 5), (6, 9)])).is_err());
    assert!(CoinView::from_bytes(&put(&[(0, 5), (3, 9)])).is_err());
    assert!(CoinView::from_bytes(&put(&[(10, 12), (0, 5)])).is_err());
    assert!(CoinView::from_bytes(&put(&[(0, 5), (7, 9)])).is_ok());
    let many: Vec<(u32, u32)> = (0..=u32::try_from(MAX_PRUNED_RANGES).unwrap())
        .map(|i| (i * 2, i * 2))
        .collect();
    assert!(CoinView::from_bytes(&put(&many)).is_err());
    // A cut and an extra byte.
    assert!(CoinView::from_bytes(&bytes[..bytes.len() - 1]).is_err());
    let mut more = bytes.clone();
    more.push(0);
    assert!(CoinView::from_bytes(&more).is_err());
}

#[test]
fn oc_28_a_reorganization_that_unspends_a_pruned_receipts_coin_keeps_the_receipt_again() {
    let old = 150_000;
    let (mut view, _chain, b1, b2) = busy_view(old, start() - old);
    view.prune_receipts(TALL, ok).unwrap();
    assert!(view.receipts_at(0).is_empty(), "pruned while it was spent");
    // The block that spent it leaves. A reorganization this far from a
    // receipt this old is not seen in practice; the view still stays whole.
    view.reorganize(&[b2.block_hash()], 1, ok).unwrap();
    assert_eq!(view.len(), 2, "the coin is back");
    let r = view.receipts_at(0);
    assert_eq!(r.len(), 1, "its receipt is kept again");
    assert_eq!(r[0].height, 1);
    assert_eq!(r[0].block, b1.block_hash());
    assert!(
        view.is_pruned(0),
        "the marker stays: the index lost receipts"
    );
    assert_eq!(view.receipt_state(), ReceiptState::Complete);
    // The stored form passes its own strict read.
    let bytes = view.to_bytes();
    assert_eq!(CoinView::from_bytes(&bytes).unwrap().to_bytes(), bytes);
}

#[test]
fn oc_28_the_same_reorganization_into_a_full_log_takes_the_named_state() {
    let old = 150_000;
    let (mut view, _chain, _b1, b2) = busy_view(old, start() - old);
    view.prune_receipts(TALL, ok).unwrap();
    // Fill the log to the cap with receipts that stay.
    let room = u32::try_from(MAX_RECEIPTS - view.receipt_count()).unwrap();
    bulk(&mut view, room, 2_000_000, TALL - 3, u32::MAX);
    assert_eq!(view.receipt_count(), MAX_RECEIPTS);
    view.reorganize(&[b2.block_hash()], 1, ok).unwrap();
    assert_eq!(view.len(), 2, "the coin is back");
    assert_eq!(view.receipt_state(), ReceiptState::LogFull);
    assert_eq!(view.receipt_count(), MAX_RECEIPTS);
    let bytes = view.to_bytes();
    assert_eq!(CoinView::from_bytes(&bytes).unwrap().to_bytes(), bytes);
}

#[test]
fn oc_53_the_stored_form_has_a_version_with_the_hash_and_reads_the_first_as_unknown() {
    let (mut view, chain) = rich_view();
    let v1 = view.to_bytes();
    assert_eq!(v1[4], 1);
    assert_eq!(CoinView::from_bytes(&v1).unwrap().checked_block(), None);
    let hash = chain.view.hash_at(2).unwrap();
    view.set_checked_at(2, Some(hash), ok).unwrap();
    assert_eq!(view.checked_block(), Some(hash));
    let v2 = view.to_bytes();
    assert_eq!(v2[4], 2);
    assert_eq!(&v2[13..45], &hash.to_byte_array());
    // The version 2 bytes are the version 1 bytes with the hash after the
    // position.
    let mut without = [&v2[..13], &v2[45..]].concat();
    without[4] = 1;
    assert_eq!(without, v1);
    let back = CoinView::from_bytes(&v2).unwrap();
    assert_eq!(back, view);
    assert_eq!(back.to_bytes(), v2);
    // Every cut is refused, and so is a version that is not read.
    for cut in 0..v2.len() {
        assert!(CoinView::from_bytes(&v2[..cut]).is_err(), "cut at {cut}");
    }
    let mut three = v2;
    three[4] = 3;
    assert!(CoinView::from_bytes(&three).is_err());
}

#[test]
fn oc_53_the_hash_follows_the_position_and_is_not_kept_when_the_position_moves_without_it() {
    let (mut view, chain) = rich_view();
    let two = chain.view.hash_at(2).unwrap();
    view.set_checked_at(2, Some(two), ok).unwrap();
    // A lower height changes nothing, and the same height with no hash
    // keeps what the view knew.
    view.set_checked_at(1, Some(chain.view.hash_at(1).unwrap()), ok)
        .unwrap();
    view.set_checked(2, ok).unwrap();
    assert_eq!(view.checked_block(), Some(two));
    // Moving up with no hash leaves it unknown.
    view.set_checked(5, ok).unwrap();
    assert_eq!((view.checked(), view.checked_block()), (5, None));
    // A reorganization that lowers the position, and a rewind of the
    // position for new scripts, leave it unknown too.
    view.set_checked_at(5, Some(two), ok).unwrap();
    view.reorganize(&[], 3, ok).unwrap();
    assert_eq!((view.checked(), view.checked_block()), (3, None));
    view.set_checked_at(3, Some(two), ok).unwrap();
    view.raise_marks_and_rescan(9, 9, 3, ok).unwrap();
    assert_eq!(view.checked(), 2);
    assert_eq!(view.checked_block(), None);
}

#[test]
fn oc_53_a_block_of_the_view_that_the_chain_no_longer_holds_is_found_with_or_without_the_hash() {
    let (mut view, mut chain) = rich_view();
    view.set_checked_at(2, Some(chain.view.hash_at(2).unwrap()), ok)
        .unwrap();
    let at = |chain: &Chain| {
        let view = chain.view.clone();
        move |h: u32| view.hash_at(h)
    };
    // The chain the view reflects: nothing left.
    assert_eq!(view.left_while_stopped(at(&chain)), None);
    let old_two = chain.blocks[1].block_hash();
    // Block 2 is replaced and the chain has grown past the view's position.
    chain.reorg(1, &[vec![], vec![], vec![]]);
    let known = view.left_while_stopped(at(&chain));
    assert_eq!(known, Some((vec![old_two], 1)));
    // A view that does not know the hash finds the same blocks from its
    // undo records.
    let mut unknown = view.clone();
    unknown.set_checked(3, ok).unwrap();
    assert_eq!(unknown.checked_block(), None);
    assert_eq!(unknown.left_while_stopped(at(&chain)), known);
    // Headers that do not reach the view's blocks say nothing.
    assert_eq!(view.left_while_stopped(|_| None), None);
    // The undo that follows is the one a reorganization the node saw has.
    let (left, fork) = known.unwrap();
    let effect = view.reorganize(&left, fork, ok).unwrap();
    assert_eq!(effect.restored, 1);
    assert_eq!(view.checked(), 1);
    assert_eq!(view.checked_block(), None);
    assert_eq!(view.balance(), 70_000);
}

#[test]
fn oc_53_with_no_undo_record_on_the_chain_the_fork_is_an_undo_horizon_below() {
    let mut chain = Chain::new();
    let mut view = CoinView::new(0);
    let pay = foreign(1, &[(&script(1), 50_000)]);
    let b1 = chain.push(std::slice::from_ref(&pay));
    apply(&mut view, &chain, &b1);
    for _ in 0..30 {
        chain.push(&[]);
    }
    view.set_checked_at(31, Some(chain.view.hash_at(31).unwrap()), ok)
        .unwrap();
    // Every block is replaced.
    chain.reorg(0, &vec![vec![]; 32]);
    let (left, fork) = view
        .left_while_stopped(|h| chain.view.hash_at(h))
        .expect("the blocks left");
    assert_eq!(left, vec![b1.block_hash()]);
    assert_eq!(fork, 0);
    // The coin's block has an undo record, so this is not the deep state.
    view.reorganize(&left, fork, ok).unwrap();
    assert_eq!(view.state(), ViewState::Ready);
    assert_eq!(view.balance(), 0);
}

#[test]
fn oc_53_a_view_in_the_deep_reorganization_state_is_not_checked_again() {
    let mut chain = Chain::new();
    let mut view = CoinView::new(0);
    let pay = foreign(1, &[(&script(1), 50_000)]);
    let b1 = chain.push(std::slice::from_ref(&pay));
    apply(&mut view, &chain, &b1);
    view.reorganize(&[b1.block_hash(); 101], 0, ok).unwrap();
    assert_eq!(view.state(), ViewState::DeepReorg);
    chain.reorg(0, &[vec![], vec![]]);
    assert_eq!(view.left_while_stopped(|h| chain.view.hash_at(h)), None);
}

#[test]
fn oc_54_a_transaction_kept_for_a_block_is_held_again_when_the_block_leaves() {
    let mut chain = Chain::new();
    let mut view = CoinView::new(0);
    let pay = foreign(1, &[(&script(1), 50_000)]);
    let b1 = chain.push(std::slice::from_ref(&pay));
    apply(&mut view, &chain, &b1);
    // A payment that was finished elsewhere: the view never held it.
    let tx = tx_from(op(&pay, 0), &[(&script(77), 40_000), (&script(9), 9_000)]);
    let txid = tx.compute_txid();
    let b2 = chain.push(std::slice::from_ref(&tx));
    apply(&mut view, &chain, &b2);
    assert!(view.confirmed_height(&txid).is_none());
    assert!(!view.retained_confirmations().contains(&txid));
    let kept = view
        .keep_confirmed(&b2.block_hash(), &tx, &chain.view, &scripts(), ok)
        .unwrap();
    assert!(kept);
    assert_eq!(view.confirmed_height(&txid), Some(2));
    assert!(view.retained_confirmations().contains(&txid));
    // Giving it again changes nothing.
    let before = view.clone();
    assert!(!view
        .keep_confirmed(&b2.block_hash(), &tx, &chain.view, &scripts(), ok)
        .unwrap());
    assert_eq!(view, before);
    // The block leaves: the transaction is held again with its change, and
    // the coin it spent is put back.
    let (left, _) = chain.reorg(1, &[vec![]]);
    let effect = view.reorganize(&left, 1, ok).unwrap();
    assert_eq!(effect.reopened, vec![txid]);
    let held = view.pending().next().unwrap();
    assert_eq!(held.tx, tx);
    assert_eq!(held.own, vec![(1, Keychain::Change, 0)]);
    assert_eq!(view.owned().count(), 1);
    assert!(view.confirmed_height(&txid).is_none());
}

#[test]
fn oc_54_a_transaction_is_kept_only_for_a_block_of_the_chain_inside_the_undo_horizon() {
    let mut chain = Chain::new();
    let mut view = CoinView::new(0);
    let tx = foreign(3, &[(&script(77), 1_000)]);
    let b1 = chain.push(std::slice::from_ref(&tx));
    // A block the chain does not hold.
    let stray = mine(chain.tip_hash(), 2, 99, &[]);
    assert!(!view
        .keep_confirmed(&stray.block_hash(), &tx, &chain.view, &scripts(), ok)
        .unwrap());
    // A block in the horizon, then the same block once the tip is an undo
    // horizon above it.
    assert!(view
        .keep_confirmed(&b1.block_hash(), &tx, &chain.view, &scripts(), ok)
        .unwrap());
    let mut late = CoinView::new(0);
    for _ in 0..UNDO_BLOCKS {
        chain.push(&[]);
    }
    assert!(!late
        .keep_confirmed(&b1.block_hash(), &tx, &chain.view, &scripts(), ok)
        .unwrap());
}
