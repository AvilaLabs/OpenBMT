//! Section 3: a confirmation the light client finds in a full block.

use bitcoin::consensus::encode::serialize;
use bitcoin::hashes::Hash;
use bitcoin::{Block, TxMerkleNode};

use super::kit::*;
use crate::{payments_in_block, spends_in_block, Refusal, MAX_BLOCK_BYTES};

fn shop() -> bitcoin::ScriptBuf {
    script(1)
}

fn plain_block() -> Block {
    block(vec![
        coinbase(1),
        spend(1, &[(&script(0x70), 1_000)]),
        spend(2, &[(&shop(), 40_000), (&script(0x71), 5)]),
        spend(3, &[(&script(0x72), 1_000)]),
    ])
}

#[test]
fn wr_20_a_block_over_4_million_bytes_is_refused() {
    let chain = TestChain::new(10);
    let big = vec![0u8; MAX_BLOCK_BYTES + 1];
    refused(
        payments_in_block(&chain, &big, &shop()),
        Refusal::BlockTooLarge {
            len: MAX_BLOCK_BYTES + 1,
        },
        "WR-20",
    );
}

#[test]
fn wr_20_bytes_that_are_not_a_block_are_refused() {
    let chain = TestChain::new(10);
    let b = plain_block();
    let mut with_extra = serialize(&b);
    with_extra.push(0);
    refused(
        payments_in_block(&chain, &with_extra, &shop()),
        Refusal::BlockMalformed,
        "WR-20",
    );
    let whole = serialize(&b);
    refused(
        payments_in_block(&chain, &whole[..whole.len() - 1], &shop()),
        Refusal::BlockMalformed,
        "WR-20",
    );
    refused(
        payments_in_block(&chain, &[], &shop()),
        Refusal::BlockMalformed,
        "WR-20",
    );
}

#[test]
fn wr_20_a_header_that_fails_its_own_proof_of_work_is_refused() {
    let mut b = plain_block();
    b.header = unmined_header(b.header.merkle_root);
    let mut chain = TestChain::new(10);
    chain.add(b.header.block_hash(), 10);
    refused(
        payments_in_block(&chain, &serialize(&b), &shop()),
        Refusal::HeaderProofOfWork,
        "WR-03",
    );
}

#[test]
fn wr_20_a_block_the_chain_does_not_know_is_refused() {
    let b = plain_block();
    let chain = TestChain::new(10);
    refused(
        payments_in_block(&chain, &serialize(&b), &shop()),
        Refusal::NotOnChain,
        "WR-01",
    );
    // Even when it has nothing for the shop.
    refused(
        payments_in_block(&chain, &serialize(&b), &script(0x99)),
        Refusal::NotOnChain,
        "WR-01",
    );
}

#[test]
fn wr_20_a_block_on_the_chain_gives_its_payments() {
    let b = plain_block();
    let chain = TestChain::new(12).with(&b, 10);
    let found = payments_in_block(&chain, &serialize(&b), &shop()).unwrap();
    assert_eq!(found, vec![proven(&b, 2, 10)]);
}

#[test]
fn wr_21_a_wrong_merkle_root_is_refused() {
    let b = plain_block();
    let wrong = block_with_root(b.txdata.clone(), TxMerkleNode::from_byte_array([5; 32]));
    let chain = TestChain::new(10).with(&wrong, 10);
    refused(
        payments_in_block(&chain, &serialize(&wrong), &shop()),
        Refusal::BlockMerkleRoot,
        "WR-21",
    );
}

#[test]
fn wr_21_a_block_with_no_transactions_is_refused() {
    let b = block_with_root(Vec::new(), TxMerkleNode::from_byte_array([0; 32]));
    let chain = TestChain::new(10).with(&b, 10);
    refused(
        payments_in_block(&chain, &serialize(&b), &shop()),
        Refusal::BlockMerkleRoot,
        "WR-21",
    );
}

#[test]
fn wr_21_a_block_without_a_coinbase_is_refused() {
    let b = block(vec![
        spend(1, &[(&shop(), 40_000)]),
        spend(2, &[(&shop(), 40_000)]),
    ]);
    let chain = TestChain::new(10).with(&b, 10);
    refused(
        payments_in_block(&chain, &serialize(&b), &shop()),
        Refusal::BlockNoCoinbase,
        "WR-21",
    );
}

#[test]
fn wr_21_a_second_coinbase_is_refused() {
    let b = block(vec![
        coinbase(1),
        spend(1, &[(&shop(), 40_000)]),
        coinbase(2),
    ]);
    let chain = TestChain::new(10).with(&b, 10);
    refused(
        payments_in_block(&chain, &serialize(&b), &shop()),
        Refusal::BlockExtraCoinbase { position: 2 },
        "WR-21",
    );
}

#[test]
fn wr_21_a_block_that_repeats_its_last_transaction_is_refused() {
    // CVE-2012-2459: the repeat does not change the merkle root.
    let honest = block(vec![
        coinbase(1),
        spend(1, &[(&script(0x70), 1)]),
        spend(2, &[(&shop(), 40_000)]),
    ]);
    let mut repeated = honest.txdata.clone();
    repeated.push(honest.txdata[2].clone());
    let mutated = Block {
        header: honest.header,
        txdata: repeated,
    };
    assert!(mutated.check_merkle_root());
    let chain = TestChain::new(10).with(&honest, 10);
    refused(
        payments_in_block(&chain, &serialize(&mutated), &shop()),
        Refusal::BlockDuplicateTransaction { position: 3 },
        "WR-21",
    );
    // The honest block is fine, and pays once.
    assert_eq!(
        payments_in_block(&chain, &serialize(&honest), &shop())
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn wr_21_a_bad_block_is_refused_whatever_the_chain_knows() {
    let b = block(vec![spend(1, &[(&shop(), 1)])]);
    for chain in [TestChain::new(10), TestChain::new(10).with(&b, 10)] {
        refused(
            payments_in_block(&chain, &serialize(&b), &shop()),
            Refusal::BlockNoCoinbase,
            "WR-21",
        );
    }
}

#[test]
fn wr_22_a_64_byte_payment_is_refused() {
    // The output script of a 64-byte transaction is at most 4 bytes.
    let short_script = bitcoin::ScriptBuf::from_bytes(vec![0x51; 4]);
    let fake = sixty_four(false, 4);
    assert_eq!(fake.output[0].script_pubkey, short_script);
    let b = block(vec![coinbase(1), fake]);
    let chain = TestChain::new(10).with(&b, 10);
    refused(
        payments_in_block(&chain, &serialize(&b), &short_script),
        Refusal::BlockPaymentSixtyFourBytes { position: 1 },
        "WR-22",
    );
}

#[test]
fn wr_22_a_64_byte_transaction_that_does_not_pay_the_shop_is_not_a_payment() {
    let b = block(vec![coinbase(1), sixty_four(false, 4)]);
    let chain = TestChain::new(10).with(&b, 10);
    assert_eq!(
        payments_in_block(&chain, &serialize(&b), &shop()).unwrap(),
        vec![]
    );
}

#[test]
fn wr_22_the_coinbase_is_never_a_payment() {
    let mut cb = coinbase(1);
    cb.output[0].script_pubkey = shop();
    let b = block(vec![cb]);
    let chain = TestChain::new(10).with(&b, 10);
    assert_eq!(
        payments_in_block(&chain, &serialize(&b), &shop()).unwrap(),
        vec![]
    );
}

#[test]
fn wr_23_no_payment_in_a_block_is_an_empty_list_and_not_an_error() {
    let b = plain_block();
    let chain = TestChain::new(10).with(&b, 10);
    let found = payments_in_block(&chain, &serialize(&b), &script(0x99));
    assert_eq!(found, Ok(vec![]));
}

#[test]
fn wr_30_outputs_to_the_shops_script_are_summed() {
    let b = block(vec![
        coinbase(1),
        spend(
            1,
            &[(&shop(), 10_000), (&script(0x71), 7), (&shop(), 2_500)],
        ),
    ]);
    let chain = TestChain::new(10).with(&b, 10);
    let found = payments_in_block(&chain, &serialize(&b), &shop()).unwrap();
    assert_eq!(found.len(), 1);
    let paid: u64 = found[0]
        .tx
        .output
        .iter()
        .filter(|o| o.script_pubkey == shop())
        .map(|o| o.value.to_sat())
        .sum();
    assert_eq!(paid, 12_500);
}

#[test]
fn wr_30_an_output_of_zero_to_the_shop_is_not_a_payment() {
    let b = block(vec![coinbase(1), spend(1, &[(&shop(), 0)])]);
    let chain = TestChain::new(10).with(&b, 10);
    assert_eq!(
        payments_in_block(&chain, &serialize(&b), &shop()).unwrap(),
        vec![]
    );
}

#[test]
fn wr_30_every_transaction_that_pays_the_shop_is_returned_in_block_order() {
    let b = block(vec![
        coinbase(1),
        spend(1, &[(&shop(), 1)]),
        spend(2, &[(&script(0x70), 1)]),
        spend(3, &[(&shop(), 2)]),
    ]);
    let chain = TestChain::new(10).with(&b, 10);
    let found = payments_in_block(&chain, &serialize(&b), &shop()).unwrap();
    assert_eq!(found, vec![proven(&b, 1, 10), proven(&b, 3, 10)]);
}

#[test]
fn wr_42_a_block_that_spends_the_coins_is_found() {
    let b = block(vec![
        coinbase(1),
        spend(1, &[(&script(0x70), 1)]),
        spend_coins(&[coin(2), coin(3)], &[(&script(0x71), 1)]),
    ]);
    let chain = TestChain::new(10).with(&b, 10);
    let found = spends_in_block(&chain, &serialize(&b), &[coin(3), coin(9), coin(1)]).unwrap();
    assert_eq!(
        found,
        vec![
            (coin(1), b.txdata[1].compute_txid(), b.block_hash()),
            (coin(3), b.txdata[2].compute_txid(), b.block_hash()),
        ]
    );
    assert_eq!(
        spends_in_block(&chain, &serialize(&b), &[coin(9)]).unwrap(),
        vec![]
    );
}

#[test]
fn wr_42_a_block_off_the_chain_or_malformed_proves_no_spend() {
    let b = block(vec![coinbase(1), spend(1, &[(&script(0x70), 1)])]);
    refused(
        spends_in_block(&TestChain::new(10), &serialize(&b), &[coin(1)]),
        Refusal::NotOnChain,
        "WR-01",
    );
    let wrong = block_with_root(b.txdata.clone(), TxMerkleNode::from_byte_array([5; 32]));
    let chain = TestChain::new(10).with(&wrong, 10);
    refused(
        spends_in_block(&chain, &serialize(&wrong), &[coin(1)]),
        Refusal::BlockMerkleRoot,
        "WR-21",
    );
}
