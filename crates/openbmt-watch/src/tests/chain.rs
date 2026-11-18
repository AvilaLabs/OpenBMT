//! Section 1: the header chain, and what the chain says of a block.

use bitcoin::consensus::encode::serialize;
use bitcoin::hashes::Hash;
use bitcoin::BlockHash;

use super::kit::*;
use crate::{check_header, confirmations, payments_in_block, HeaderChain, Refusal};

fn pay_script() -> bitcoin::ScriptBuf {
    script(1)
}

fn pay_block() -> bitcoin::Block {
    block(vec![
        coinbase(7),
        spend(1, &[(&script(0x77), 1_000)]),
        spend(2, &[(&pay_script(), 50_000)]),
    ])
}

#[test]
fn wr_01_block_the_chain_does_not_know_is_refused() {
    let b = pay_block();
    let empty = TestChain::new(100);
    refused(
        payments_in_block(&empty, &serialize(&b), &pay_script()),
        Refusal::NotOnChain,
        "WR-01",
    );
}

#[test]
fn wr_01_block_a_with_a_chain_that_knows_only_block_b() {
    let a = pay_block();
    let b = block(vec![coinbase(8), spend(9, &[(&pay_script(), 1)])]);
    let chain_b = TestChain::new(100).with(&b, 100);
    assert_ne!(a.block_hash(), b.block_hash());
    refused(
        payments_in_block(&chain_b, &serialize(&a), &pay_script()),
        Refusal::NotOnChain,
        "WR-01",
    );
}

#[test]
fn wr_01_a_height_above_the_tip_is_not_on_the_chain() {
    let b = pay_block();
    let chain = TestChain::new(99).with(&b, 100);
    refused(
        payments_in_block(&chain, &serialize(&b), &pay_script()),
        Refusal::NotOnChain,
        "WR-01",
    );
}

#[test]
fn wr_01_a_block_that_leaves_the_chain_proves_nothing_more() {
    let b = pay_block();
    let mut chain = TestChain::new(100).with(&b, 100);
    assert!(payments_in_block(&chain, &serialize(&b), &pay_script()).is_ok());
    chain.remove(&b.block_hash());
    refused(
        payments_in_block(&chain, &serialize(&b), &pay_script()),
        Refusal::NotOnChain,
        "WR-01",
    );
}

#[test]
fn wr_02_confirmations_are_the_tip_height_minus_the_height_plus_one() {
    let b = block(vec![coinbase(1)]);
    let mut chain = TestChain::new(105).with(&b, 100);
    assert_eq!(confirmations(&chain, &b.block_hash()), Some(6));
    chain.tip = 100;
    assert_eq!(confirmations(&chain, &b.block_hash()), Some(1));
    chain.tip = 99;
    assert_eq!(confirmations(&chain, &b.block_hash()), None);
}

#[test]
fn wr_02_a_block_that_is_not_on_the_chain_has_no_confirmations() {
    let chain = TestChain::new(105);
    assert_eq!(confirmations(&chain, &BlockHash::all_zeros()), None);
}

#[test]
fn wr_02_the_largest_tip_does_not_overflow() {
    let b = block(vec![coinbase(1)]);
    let chain = TestChain::new(u32::MAX).with(&b, 0);
    assert_eq!(confirmations(&chain, &b.block_hash()), None);
    let chain = TestChain::new(u32::MAX).with(&b, 1);
    assert_eq!(confirmations(&chain, &b.block_hash()), Some(u32::MAX));
}

#[test]
fn wr_02_the_tip_time_is_what_the_chain_says() {
    let chain = TestChain::new(1);
    assert_eq!(chain.tip_time(), 1_700_000_000);
}

#[test]
fn wr_03_a_header_that_fails_its_own_proof_of_work_is_refused() {
    let b = block(vec![coinbase(1), spend(2, &[(&pay_script(), 5)])]);
    let header = unmined_header(b.header.merkle_root);
    assert_eq!(check_header(&header), Err(Refusal::HeaderProofOfWork));
    assert!(check_header(&b.header).is_ok());
}
