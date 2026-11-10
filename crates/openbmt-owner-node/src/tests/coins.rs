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

use crate::coins::{
    ApplyError, CoinView, PendingError, ScriptSet, ViewState, GAP, MAGIC_OWNER_COINS, MAX_PENDING,
    UNDO_BLOCKS,
};

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
