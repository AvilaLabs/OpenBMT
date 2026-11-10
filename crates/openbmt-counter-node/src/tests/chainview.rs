//! `ChainView`: built from a header store, extended, reorganized, and equal
//! to a real header store fed the same headers.

use bitcoin::{BlockHash, Network};
use openbmt_watch::{Accepted, HeaderChain, HeaderStore};

use super::kit::*;
use crate::ChainView;

const NOW: u64 = u64::MAX / 2;

/// A header store that has these blocks' headers.
fn store_of(blocks: &[bitcoin::Block]) -> HeaderStore {
    let mut store = HeaderStore::new(Network::Regtest).unwrap();
    for block in blocks {
        store.accept(&block.header, NOW).unwrap();
    }
    store
}

/// `chain` answers every question as `store` does, for these hashes.
fn same(chain: &ChainView, store: &HeaderStore, hashes: &[BlockHash]) {
    assert_eq!(chain.tip_height(), store.tip_height());
    assert_eq!(chain.tip_time(), store.tip_time());
    for hash in hashes {
        assert_eq!(chain.height_of(hash), store.height_of(hash), "{hash}");
    }
    for height in 0..=store.tip_height() + 2 {
        assert_eq!(
            chain.hash_at(height),
            store.header_at(height).map(|h| h.block_hash()),
            "height {height}"
        );
    }
}

fn chain_of(n: u32) -> Vec<bitcoin::Block> {
    let mut blocks = Blocks::default();
    for _ in 0..n {
        blocks.extend(&[]);
    }
    (1..=n).map(|h| blocks.at(h).clone()).collect()
}

#[test]
fn a_new_store_gives_a_view_of_the_start_block_alone() {
    let store = HeaderStore::new(Network::Regtest).unwrap();
    let view = ChainView::from_store(&store);
    assert_eq!(view.len(), 1);
    assert!(!view.is_empty());
    assert_eq!(view.height_of(&genesis()), Some(0));
    same(&view, &store, &[genesis()]);
}

#[test]
fn a_view_is_built_from_the_stored_best_chain() {
    let blocks = chain_of(30);
    let store = store_of(&blocks);
    let view = ChainView::from_store(&store);
    assert_eq!(view.len(), 31);
    let mut hashes: Vec<BlockHash> = blocks.iter().map(|b| b.block_hash()).collect();
    hashes.push(genesis());
    same(&view, &store, &hashes);
    // What the driver would read back from its file gives the same view.
    let reread = HeaderStore::from_bytes(Network::Regtest, &store.to_bytes()).unwrap();
    assert_eq!(ChainView::from_store(&reread), view);
}

#[test]
fn a_view_is_extended_by_the_chain_events() {
    let blocks = chain_of(12);
    let mut store = store_of(&blocks[..5]);
    let mut view = ChainView::from_store(&store);
    // Seven blocks join, as the driver would send them: lowest first.
    for chunk in blocks[5..].chunks(3) {
        let first = store.tip_height() + 1;
        let joined: Vec<(u32, BlockHash)> = chunk
            .iter()
            .enumerate()
            .map(|(i, b)| (first + i as u32, b.block_hash()))
            .collect();
        for block in chunk {
            assert_eq!(
                store.accept(&block.header, NOW).unwrap(),
                Accepted::Extended
            );
        }
        view.apply(&joined, &[]);
        let tip = store.tip_height();
        view.set_tip_time(tip, u64::from(store.header_at(tip).unwrap().time));
        let all: Vec<BlockHash> = blocks.iter().map(|b| b.block_hash()).collect();
        same(&view, &store, &all);
    }
    assert_eq!(view.tip_height(), 12);
}

#[test]
fn a_reorganization_takes_the_left_blocks_out_and_puts_the_joined_in() {
    let mut blocks = Blocks::default();
    for _ in 0..8 {
        blocks.extend(&[]);
    }
    let old: Vec<bitcoin::Block> = (1..=8).map(|h| blocks.at(h).clone()).collect();
    let mut store = store_of(&old);
    let mut view = ChainView::from_store(&store);

    // A branch from height 5 that has more work: heights 6, 7, 8 and 9.
    let mut parent = old[4].block_hash();
    let mut branch = Vec::new();
    for height in 6..=9 {
        let block = mine(parent, height, 1, &[]);
        parent = block.block_hash();
        branch.push(block);
    }
    let mut left = Vec::new();
    for (i, block) in branch.iter().enumerate() {
        match store.accept(&block.header, NOW).unwrap() {
            Accepted::Side => assert!(i < 3),
            Accepted::Reorganized { left: l } => left = l,
            other => panic!("{other:?}"),
        }
    }
    // The driver says: old tip first on the left, lowest first on the right.
    assert_eq!(left.len(), 3);
    let joined: Vec<(u32, BlockHash)> = branch
        .iter()
        .enumerate()
        .map(|(i, b)| (6 + i as u32, b.block_hash()))
        .collect();
    view.apply(&joined, &left);
    // The time comes with the next Peers event, not with the chain event.
    assert_ne!(view.tip_time(), store.tip_time());
    view.set_tip_time(store.tip_height(), store.tip_time());

    let mut hashes: Vec<BlockHash> = old.iter().map(|b| b.block_hash()).collect();
    hashes.extend(branch.iter().map(|b| b.block_hash()));
    same(&view, &store, &hashes);
    // The blocks that left answer nothing now.
    for hash in &left {
        assert_eq!(view.height_of(hash), None);
    }
    assert_eq!(view.tip_height(), 9);

    // A reorganization to a branch with fewer blocks, as a chain event can
    // say it: the tip comes down.
    view.apply(&[], &[branch[3].block_hash(), branch[2].block_hash()]);
    assert_eq!(view.tip_height(), 7);
    assert_eq!(view.height_of(&branch[1].block_hash()), Some(7));
}

#[test]
fn the_tip_time_is_taken_only_for_the_tip() {
    let blocks = chain_of(3);
    let store = store_of(&blocks);
    let mut view = ChainView::from_store(&store);
    let before = view.tip_time();
    view.set_tip_time(2, 5);
    assert_eq!(view.tip_time(), before);
    view.set_tip_time(3, 77);
    assert_eq!(view.tip_time(), 77);
}

#[test]
fn a_block_that_moves_to_another_height_is_found_there_only() {
    let blocks = chain_of(3);
    let mut view = ChainView::from_store(&HeaderStore::new(Network::Regtest).unwrap());
    let hash = blocks[0].block_hash();
    view.apply(&[(1, hash)], &[]);
    view.apply(&[(2, hash)], &[]);
    assert_eq!(view.height_of(&hash), Some(2));
    assert_eq!(view.hash_at(1), None);
}
