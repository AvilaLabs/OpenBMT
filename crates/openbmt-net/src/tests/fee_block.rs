//! NR-47: one block asked for by its hash, whatever the filters say.

use bitcoin::hashes::Hash;
use bitcoin::BlockHash;

use super::manager_kit::*;
use crate::manager::ANSWER_SECS;
use crate::Output;

/// The hashes of the blocks handed out as asked-for blocks in these outputs.
fn fee_blocks_out(out: &[Output]) -> Vec<BlockHash> {
    out.iter()
        .filter_map(|o| match o {
            Output::FeeBlock { hash, .. } => Some(*hash),
            _ => None,
        })
        .collect()
}

#[test]
fn nr_47_a_block_asked_for_by_hash_is_fetched_with_no_filter_and_handed_out_apart() {
    // Nothing is watched and every block is "done at once": no filter, no
    // download.
    let mut n = Net::new();
    let a = n.up(1);
    let blocks = blocks_on(&genesis(), vec![vec![]; 3], 1);
    let out = n.headers(a, &headers_of(&blocks));
    assert!(blocks_out(&out).is_empty());
    assert!(block_requests(&n.sent(a)).is_empty());
    let hash = blocks[1].block_hash();
    assert!(!n.m.fetching_block());
    assert!(n.m.fetch_block(hash));
    assert!(n.m.fetching_block());
    // One request, for that block, to a peer that showed it.
    assert_eq!(block_requests(&n.sent(a)), vec![hash]);
    let out = n.block(a, &blocks[1]);
    assert_eq!(fee_blocks_out(&out), vec![hash]);
    assert!(blocks_out(&out).is_empty());
    assert!(!n.m.fetching_block());
    assert_eq!(n.m.sizes().work, 0);
}

#[test]
fn nr_47_only_one_block_is_asked_for_at_a_time() {
    let mut n = Net::new();
    let a = n.up(1);
    let blocks = blocks_on(&genesis(), vec![vec![]; 3], 1);
    n.headers(a, &headers_of(&blocks));
    let (first, second) = (blocks[0].block_hash(), blocks[1].block_hash());
    assert!(n.m.fetch_block(first));
    // The same hash again changes nothing: one request went out.
    assert!(n.m.fetch_block(first));
    assert_eq!(block_requests(&n.sent(a)), vec![first]);
    // Another hash gives up the first: it is not asked for a second time,
    // and the first's late answer is tolerated and gives nothing.
    assert!(n.m.fetch_block(second));
    assert_eq!(block_requests(&n.sent(a)), vec![second]);
    let out = n.block(a, &blocks[0]);
    assert!(out.is_empty(), "{out:?}");
    assert_eq!(n.m.sizes().peers, 1);
    let out = n.block(a, &blocks[1]);
    assert_eq!(fee_blocks_out(&out), vec![second]);
    assert!(!n.m.fetching_block());
}

#[test]
fn nr_47_a_hash_that_is_not_on_the_best_chain_is_not_asked_for() {
    let mut n = Net::new();
    let a = n.up(1);
    let blocks = blocks_on(&genesis(), vec![vec![]; 2], 1);
    n.headers(a, &headers_of(&blocks));
    assert!(!n.m.fetch_block(BlockHash::from_byte_array([9; 32])));
    assert!(!n.m.fetching_block());
    assert!(block_requests(&n.sent(a)).is_empty());
}

#[test]
fn nr_47_a_block_not_delivered_in_sixty_seconds_is_asked_of_another_peer() {
    let mut n = Net::new();
    let a = n.up(1);
    let b = n.up(2);
    let blocks = blocks_on(&genesis(), vec![vec![]; 1], 1);
    n.headers(a, &headers_of(&blocks));
    let hash = blocks[0].block_hash();
    assert!(n.m.fetch_block(hash));
    assert_eq!(block_requests(&n.sent(a)), vec![hash]);
    assert!(n.tick(ANSWER_SECS - 1).is_empty());
    assert!(block_requests(&n.sent(b)).is_empty());
    n.tick(1);
    assert_eq!(block_requests(&n.sent(b)), vec![hash]);
    let out = n.block(b, &blocks[0]);
    assert_eq!(fee_blocks_out(&out), vec![hash]);
    // The slow peer's late copy is no unasked data.
    let out = n.block(a, &blocks[0]);
    assert!(out.is_empty());
    assert_eq!(n.m.sizes().peers, 2);
}

#[test]
fn nr_47_a_peer_cannot_substitute_another_block() {
    let mut n = Net::new();
    let a = n.up(1);
    let b = n.up(2);
    let blocks = blocks_on(&genesis(), vec![vec![]; 1], 1);
    n.headers(a, &headers_of(&blocks));
    let hash = blocks[0].block_hash();
    assert!(n.m.fetch_block(hash));
    n.sent(a);
    let other = block_on(&genesis(), vec![], 99);
    let out = n.block(a, &other);
    assert_eq!(disconnects(&out), vec![(a, true)]);
    assert!(reason(&out).contains("other than the one asked for"));
    assert!(fee_blocks_out(&out).is_empty());
    // The right block is asked of the other peer.
    assert_eq!(block_requests(&n.sent(b)), vec![hash]);
    let out = n.block(b, &blocks[0]);
    assert_eq!(fee_blocks_out(&out), vec![hash]);
}

#[test]
fn nr_47_a_block_that_fails_the_watchers_checks_is_a_misbehaviour() {
    let mut n = Net::new();
    let a = n.up(1);
    let b = n.up(2);
    let blocks = blocks_on(&genesis(), vec![vec![]; 1], 1);
    n.headers(a, &headers_of(&blocks));
    let hash = blocks[0].block_hash();
    assert!(n.m.fetch_block(hash));
    n.sent(a);
    // Same header, other transactions: the merkle root is wrong.
    let mut bad = blocks[0].clone();
    bad.txdata.push(pay(&script(1), 1_000, 5));
    let out = n.block(a, &bad);
    assert_eq!(disconnects(&out), vec![(a, true)]);
    assert!(reason(&out).starts_with("NR-25: block refused"));
    assert!(fee_blocks_out(&out).is_empty());
    assert_eq!(block_requests(&n.sent(b)), vec![hash]);
    let out = n.block(b, &blocks[0]);
    assert_eq!(fee_blocks_out(&out), vec![hash]);
}

#[test]
fn nr_47_a_block_that_leaves_the_chain_is_given_up() {
    let mut n = Net::new();
    let a = n.up(1);
    let b = n.up(2);
    let blocks = blocks_on(&genesis(), vec![vec![]; 2], 1);
    n.headers(a, &headers_of(&blocks));
    let hash = blocks[1].block_hash();
    assert!(n.m.fetch_block(hash));
    n.sent(a);
    // A longer chain from the other peer replaces both blocks.
    let better = extend(&genesis(), 3, 100);
    let out = n.headers(b, &better);
    assert_eq!(chain_out(&out).len(), 1);
    assert!(!n.m.fetching_block());
    // The old block's late answer is tolerated and gives nothing.
    let out = n.block(a, &blocks[1]);
    assert!(out.is_empty(), "{out:?}");
    assert_eq!(n.m.sizes().peers, 2);
    // Another can be asked for now.
    assert!(n.m.fetch_block(better[2].block_hash()));
}
