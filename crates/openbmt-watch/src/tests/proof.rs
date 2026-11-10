//! Sections 1 and 2: the header chain and the Box's merkle proof.

use bitcoin::hashes::{sha256d, Hash};
use bitcoin::{BlockHash, Transaction, TxMerkleNode};

use super::kit::*;
use crate::{
    check_header, confirmations, verify_box_proof, HeaderChain, Refusal, MAX_PROOF_BYTES,
    MAX_TX_BYTES,
};

fn pay_script() -> bitcoin::ScriptBuf {
    script(1)
}

// Section 1.

#[test]
fn wr_01_block_the_chain_does_not_know_is_refused() {
    let s = setup(&pay_script(), 50_000);
    let empty = TestChain::new(100);
    refused(
        verify_box_proof(&empty, &s.proof, &s.coinbase, &s.payment),
        Refusal::NotOnChain,
        "WR-01",
    );
}

#[test]
fn wr_01_proof_valid_for_block_a_with_a_chain_that_knows_only_block_b() {
    let a = setup(&pay_script(), 50_000);
    let b = block(vec![coinbase(7), spend(9, &[(&pay_script(), 1)])]);
    let chain_b = TestChain::new(100).with(&b, 100);
    assert_ne!(a.block.block_hash(), b.block_hash());
    refused(
        verify_box_proof(&chain_b, &a.proof, &a.coinbase, &a.payment),
        Refusal::NotOnChain,
        "WR-01",
    );
}

#[test]
fn wr_01_a_height_above_the_tip_is_not_on_the_chain() {
    let s = setup(&pay_script(), 50_000);
    let chain = TestChain::new(99).with(&s.block, 100);
    refused(
        verify_box_proof(&chain, &s.proof, &s.coinbase, &s.payment),
        Refusal::NotOnChain,
        "WR-01",
    );
}

#[test]
fn wr_01_a_block_that_leaves_the_chain_proves_nothing_more() {
    let mut s = setup(&pay_script(), 50_000);
    assert!(verify_box_proof(&s.chain, &s.proof, &s.coinbase, &s.payment).is_ok());
    s.chain.remove(&s.block.block_hash());
    refused(
        verify_box_proof(&s.chain, &s.proof, &s.coinbase, &s.payment),
        Refusal::NotOnChain,
        "WR-01",
    );
}

#[test]
fn wr_01_a_bad_proof_is_refused_whatever_the_chain_knows() {
    // The chain is consulted last: the same refusal with and without it.
    let s = setup(&pay_script(), 50_000);
    let empty = TestChain::new(100);
    let wrong_coinbase = bytes(&coinbase(9));
    for chain in [&s.chain, &empty] {
        refused(
            verify_box_proof(chain, &s.proof, &wrong_coinbase, &s.payment),
            Refusal::CoinbaseIdMismatch,
            "WR-14",
        );
    }
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
    // The proof is otherwise whole; even a chain that knows the hash
    // does not make it count.
    let mut proof = proof_bytes(&b, &[0, 1]);
    proof.splice(0..80, bitcoin::consensus::encode::serialize(&header));
    let mut chain = TestChain::new(10);
    chain.add(header.block_hash(), 10);
    refused(
        verify_box_proof(&chain, &proof, &bytes(&b.txdata[0]), &bytes(&b.txdata[1])),
        Refusal::HeaderProofOfWork,
        "WR-03",
    );
}

// Section 2.

#[test]
fn wr_10_a_proof_over_64_kib_is_refused() {
    let s = setup(&pay_script(), 50_000);
    let big = vec![0u8; MAX_PROOF_BYTES + 1];
    refused(
        verify_box_proof(&s.chain, &big, &s.coinbase, &s.payment),
        Refusal::ProofTooLarge {
            len: MAX_PROOF_BYTES + 1,
        },
        "WR-10",
    );
    // At exactly the limit the size check passes and the bytes are read.
    let edge = vec![0u8; MAX_PROOF_BYTES];
    refused(
        verify_box_proof(&s.chain, &edge, &s.coinbase, &s.payment),
        Refusal::ProofMalformed,
        "WR-10",
    );
}

#[test]
fn wr_10_a_transaction_over_400000_bytes_is_refused() {
    let s = setup(&pay_script(), 50_000);
    let big = vec![0u8; MAX_TX_BYTES + 1];
    refused(
        verify_box_proof(&s.chain, &s.proof, &big, &s.payment),
        Refusal::CoinbaseTooLarge {
            len: MAX_TX_BYTES + 1,
        },
        "WR-10",
    );
    refused(
        verify_box_proof(&s.chain, &s.proof, &s.coinbase, &big),
        Refusal::PaymentTooLarge {
            len: MAX_TX_BYTES + 1,
        },
        "WR-10",
    );
}

#[test]
fn wr_10_a_trailing_byte_after_the_proof_is_refused() {
    let s = setup(&pay_script(), 50_000);
    let mut proof = s.proof.clone();
    proof.push(0);
    refused(
        verify_box_proof(&s.chain, &proof, &s.coinbase, &s.payment),
        Refusal::ProofMalformed,
        "WR-10",
    );
}

#[test]
fn wr_10_a_cut_proof_is_refused() {
    let s = setup(&pay_script(), 50_000);
    for cut in [0, 1, 79, 80, 84, s.proof.len() - 1] {
        refused(
            verify_box_proof(&s.chain, &s.proof[..cut], &s.coinbase, &s.payment),
            Refusal::ProofMalformed,
            "WR-10",
        );
    }
}

#[test]
fn wr_10_a_trailing_byte_after_a_transaction_is_refused() {
    let s = setup(&pay_script(), 50_000);
    let mut coinbase = s.coinbase.clone();
    coinbase.push(0);
    refused(
        verify_box_proof(&s.chain, &s.proof, &coinbase, &s.payment),
        Refusal::CoinbaseMalformed,
        "WR-14",
    );
    let mut payment = s.payment.clone();
    payment.push(0);
    refused(
        verify_box_proof(&s.chain, &s.proof, &s.coinbase, &payment),
        Refusal::PaymentMalformed,
        "WR-15",
    );
}

#[test]
fn wr_11_a_whole_proof_on_the_chain_proves_the_payment() {
    let s = setup(&pay_script(), 50_000);
    let proven = verify_box_proof(&s.chain, &s.proof, &s.coinbase, &s.payment).unwrap();
    assert_eq!(proven.block, s.block.block_hash());
    assert_eq!(proven.height, 100);
    assert_eq!(proven.position, 2);
    assert_eq!(proven.tx, s.block.txdata[2]);
}

#[test]
fn wr_11_a_block_with_two_transactions_proves_the_second() {
    let b = block(vec![coinbase(1), spend(2, &[(&pay_script(), 77)])]);
    let chain = TestChain::new(5).with(&b, 3);
    let proven = verify_box_proof(
        &chain,
        &proof_bytes(&b, &[0, 1]),
        &bytes(&b.txdata[0]),
        &bytes(&b.txdata[1]),
    )
    .unwrap();
    assert_eq!((proven.height, proven.position), (3, 1));
}

#[test]
fn wr_11_a_block_with_an_odd_number_of_transactions_proves_the_last() {
    let b = block(vec![
        coinbase(1),
        spend(2, &[(&pay_script(), 1)]),
        spend(3, &[(&pay_script(), 2)]),
    ]);
    let chain = TestChain::new(5).with(&b, 5);
    let proven = verify_box_proof(
        &chain,
        &proof_bytes(&b, &[0, 2]),
        &bytes(&b.txdata[0]),
        &bytes(&b.txdata[2]),
    )
    .unwrap();
    assert_eq!(proven.position, 2);
}

#[test]
fn wr_12_a_transaction_count_of_one_is_refused() {
    let b = block(vec![coinbase(1)]);
    let chain = TestChain::new(5).with(&b, 5);
    let proof = proof_bytes(&b, &[0]);
    refused(
        verify_box_proof(&chain, &proof, &bytes(&b.txdata[0]), &bytes(&b.txdata[0])),
        Refusal::TreeTransactionCount { count: 1 },
        "WR-12",
    );
}

#[test]
fn wr_12_a_transaction_count_of_zero_or_over_16666_is_refused() {
    let s = setup(&pay_script(), 50_000);
    let hash = [7u8; 32];
    for count in [0u32, 16_667, u32::MAX] {
        let proof = raw_proof(&s.block.header, count, &[hash], &[true]);
        refused(
            verify_box_proof(&s.chain, &proof, &s.coinbase, &s.payment),
            Refusal::TreeTransactionCount { count },
            "WR-12",
        );
    }
}

#[test]
fn wr_12_more_hashes_than_transactions_is_refused() {
    let s = setup(&pay_script(), 50_000);
    let hashes = [[1u8; 32], [2u8; 32], [3u8; 32]];
    let proof = raw_proof(&s.block.header, 2, &hashes, &[true, true, true]);
    refused(
        verify_box_proof(&s.chain, &proof, &s.coinbase, &s.payment),
        Refusal::TreeTooManyHashes,
        "WR-12",
    );
}

#[test]
fn wr_12_a_hash_or_a_flag_byte_left_unused_is_refused() {
    let s = setup(&pay_script(), 50_000);
    // The root is not a parent of a match, so it uses one hash: the second
    // hash is left over.
    let proof = raw_proof(&s.block.header, 2, &[[1u8; 32], [2u8; 32]], &[false]);
    refused(
        verify_box_proof(&s.chain, &proof, &s.coinbase, &s.payment),
        Refusal::TreeBitsAndHashes,
        "WR-12",
    );
    // A whole flag byte after the bits that the walk uses.
    let mut bits = vec![false];
    bits.extend([false; 8]);
    let proof = raw_proof(&s.block.header, 2, &[[1u8; 32]], &bits);
    refused(
        verify_box_proof(&s.chain, &proof, &s.coinbase, &s.payment),
        Refusal::TreeBitsAndHashes,
        "WR-12",
    );
}

#[test]
fn wr_12_running_out_of_hashes_or_bits_is_refused() {
    let s = setup(&pay_script(), 50_000);
    // Two matches need two hashes; only one is given.
    let proof = raw_proof(&s.block.header, 2, &[[1u8; 32]], &[true, true, true]);
    refused(
        verify_box_proof(&s.chain, &proof, &s.coinbase, &s.payment),
        Refusal::TreeBitsAndHashes,
        "WR-12",
    );
}

#[test]
fn wr_12_spare_bits_that_are_not_zero_are_refused() {
    let b = block(vec![coinbase(1), spend(2, &[(&pay_script(), 5)])]);
    let chain = TestChain::new(5).with(&b, 5);
    let txids: Vec<[u8; 32]> = b
        .txdata
        .iter()
        .map(|t| t.compute_txid().to_byte_array())
        .collect();
    let mut bits = vec![true, true, true];
    let good = raw_proof(&b.header, 2, &txids, &bits);
    // Padding bits all zero: the proof is the same as the library's own.
    assert_eq!(good, proof_bytes(&b, &[0, 1]));
    assert!(verify_box_proof(&chain, &good, &bytes(&b.txdata[0]), &bytes(&b.txdata[1])).is_ok());
    // The same proof with one spare bit set is another encoding of it.
    bits.extend([false, false, false, false, true]);
    let padded = raw_proof(&b.header, 2, &txids, &bits);
    refused(
        verify_box_proof(&chain, &padded, &bytes(&b.txdata[0]), &bytes(&b.txdata[1])),
        Refusal::TreePadding,
        "WR-12",
    );
}

#[test]
fn wr_12_a_duplicated_last_pair_is_refused() {
    // CVE-2012-2459: a block of three transactions has the same merkle root
    // as the block that repeats the last one. The proof is written for the
    // repeated list but carries the three-transaction block's header.
    let three = block(vec![
        coinbase(1),
        spend(2, &[(&pay_script(), 1)]),
        spend(3, &[(&pay_script(), 2)]),
    ]);
    let txids: Vec<_> = three.txdata.iter().map(Transaction::compute_txid).collect();
    let repeated = [txids[0], txids[1], txids[2], txids[2]];
    assert_eq!(
        three.header.merkle_root,
        bitcoin::merkle_tree::calculate_root(
            repeated
                .iter()
                .map(|t| TxMerkleNode::from_byte_array(t.to_byte_array()))
        )
        .unwrap()
    );
    let txn =
        bitcoin::merkle_tree::PartialMerkleTree::from_txids(&repeated, &[true, false, false, true]);
    let proof = bitcoin::consensus::encode::serialize(&bitcoin::MerkleBlock {
        header: three.header,
        txn,
    });
    let chain = TestChain::new(5).with(&three, 5);
    refused(
        verify_box_proof(
            &chain,
            &proof,
            &bytes(&three.txdata[0]),
            &bytes(&three.txdata[2]),
        ),
        Refusal::TreeEqualChildren,
        "WR-12",
    );
}

#[test]
fn wr_12_a_wrong_merkle_root_is_refused() {
    let b = block(vec![coinbase(1), spend(2, &[(&pay_script(), 5)])]);
    let other = mine_header(TxMerkleNode::from_byte_array([9; 32]), 1_000);
    let mut proof = proof_bytes(&b, &[0, 1]);
    proof.splice(0..80, bitcoin::consensus::encode::serialize(&other));
    let chain = TestChain::new(5).with(&b, 5);
    let mut chain = chain;
    chain.add(other.block_hash(), 5);
    refused(
        verify_box_proof(&chain, &proof, &bytes(&b.txdata[0]), &bytes(&b.txdata[1])),
        Refusal::TreeRootMismatch,
        "WR-12",
    );
}

#[test]
fn wr_13_a_proof_for_the_payment_only_is_refused() {
    let s = setup(&pay_script(), 50_000);
    let proof = proof_bytes(&s.block, &[2]);
    refused(
        verify_box_proof(&s.chain, &proof, &s.coinbase, &s.payment),
        Refusal::ProofCoversWrongCount { found: 1 },
        "WR-13",
    );
}

#[test]
fn wr_13_a_proof_for_three_transactions_is_refused() {
    let s = setup(&pay_script(), 50_000);
    let proof = proof_bytes(&s.block, &[0, 2, 3]);
    refused(
        verify_box_proof(&s.chain, &proof, &s.coinbase, &s.payment),
        Refusal::ProofCoversWrongCount { found: 3 },
        "WR-13",
    );
}

#[test]
fn wr_13_a_proof_for_nothing_is_refused() {
    let s = setup(&pay_script(), 50_000);
    let proof = proof_bytes(&s.block, &[]);
    refused(
        verify_box_proof(&s.chain, &proof, &s.coinbase, &s.payment),
        Refusal::ProofCoversWrongCount { found: 0 },
        "WR-13",
    );
}

#[test]
fn wr_13_the_first_proven_transaction_must_be_at_position_0() {
    let s = setup(&pay_script(), 50_000);
    let proof = proof_bytes(&s.block, &[1, 2]);
    refused(
        verify_box_proof(&s.chain, &proof, &bytes(&s.block.txdata[1]), &s.payment),
        Refusal::CoinbaseNotFirst { position: 1 },
        "WR-13",
    );
}

#[test]
fn wr_14_coinbase_bytes_that_are_not_the_transaction_at_position_0() {
    let s = setup(&pay_script(), 50_000);
    let other = bytes(&coinbase(9));
    refused(
        verify_box_proof(&s.chain, &s.proof, &other, &s.payment),
        Refusal::CoinbaseIdMismatch,
        "WR-14",
    );
    // The payment's bytes in the coinbase's place.
    refused(
        verify_box_proof(&s.chain, &s.proof, &s.payment, &s.payment),
        Refusal::CoinbaseIdMismatch,
        "WR-14",
    );
}

#[test]
fn wr_14_a_coinbase_that_is_not_a_coinbase_is_refused() {
    let b = block(vec![
        spend(1, &[(&pay_script(), 9)]),
        spend(2, &[(&pay_script(), 5)]),
    ]);
    let chain = TestChain::new(5).with(&b, 5);
    refused(
        verify_box_proof(
            &chain,
            &proof_bytes(&b, &[0, 1]),
            &bytes(&b.txdata[0]),
            &bytes(&b.txdata[1]),
        ),
        Refusal::NotACoinbase,
        "WR-14",
    );
}

#[test]
fn wr_14_bytes_that_are_not_a_transaction_are_refused() {
    let s = setup(&pay_script(), 50_000);
    refused(
        verify_box_proof(&s.chain, &s.proof, &[1, 2, 3], &s.payment),
        Refusal::CoinbaseMalformed,
        "WR-14",
    );
}

#[test]
fn wr_15_a_payment_that_is_not_the_transaction_proven_is_refused() {
    let s = setup(&pay_script(), 50_000);
    let other = bytes(&spend(8, &[(&pay_script(), 50_000)]));
    refused(
        verify_box_proof(&s.chain, &s.proof, &s.coinbase, &other),
        Refusal::PaymentIdMismatch,
        "WR-15",
    );
}

#[test]
fn wr_15_a_payment_that_is_a_coinbase_is_refused() {
    let b = block(vec![coinbase(1), coinbase(2)]);
    let chain = TestChain::new(5).with(&b, 5);
    refused(
        verify_box_proof(
            &chain,
            &proof_bytes(&b, &[0, 1]),
            &bytes(&b.txdata[0]),
            &bytes(&b.txdata[1]),
        ),
        Refusal::PaymentIsCoinbase,
        "WR-15",
    );
}

#[test]
fn wr_15_bytes_that_are_not_a_transaction_are_refused() {
    let s = setup(&pay_script(), 50_000);
    refused(
        verify_box_proof(&s.chain, &s.proof, &s.coinbase, &[]),
        Refusal::PaymentMalformed,
        "WR-15",
    );
}

#[test]
fn wr_16_a_64_byte_payment_is_refused() {
    let b = block(vec![coinbase(1), sixty_four(false, 4)]);
    let chain = TestChain::new(5).with(&b, 5);
    refused(
        verify_box_proof(
            &chain,
            &proof_bytes(&b, &[0, 1]),
            &bytes(&b.txdata[0]),
            &bytes(&b.txdata[1]),
        ),
        Refusal::PaymentSixtyFourBytes,
        "WR-16",
    );
}

#[test]
fn wr_16_a_64_byte_transaction_at_position_0_is_refused() {
    let b = block(vec![sixty_four(true, 2), spend(2, &[(&pay_script(), 5)])]);
    let chain = TestChain::new(5).with(&b, 5);
    assert!(b.txdata[0].is_coinbase());
    refused(
        verify_box_proof(
            &chain,
            &proof_bytes(&b, &[0, 1]),
            &bytes(&b.txdata[0]),
            &bytes(&b.txdata[1]),
        ),
        Refusal::CoinbaseSixtyFourBytes,
        "WR-16",
    );
}

#[test]
fn wr_16_the_depth_attack_with_two_inner_nodes_read_as_transactions() {
    // A block of four leaves L0..L3. An attacker who can choose the leaves
    // makes L0||L1 the bytes of a coinbase and L2||L3 the bytes of a
    // payment, both 64 bytes long. The inner nodes N01 and N23 are then the
    // IDs of those two "transactions", and a proof with a transaction count
    // of 2 shows them at position 0 and 1 under the real merkle root.
    let fake_coinbase = sixty_four(true, 2);
    let fake_payment = sixty_four(false, 4);
    let a = bytes(&fake_coinbase);
    let p = bytes(&fake_payment);
    assert_eq!((a.len(), p.len()), (64, 64));
    let leaf = |bytes: &[u8]| -> [u8; 32] { bytes.try_into().unwrap() };
    let leaves = [
        leaf(&a[..32]),
        leaf(&a[32..]),
        leaf(&p[..32]),
        leaf(&p[32..]),
    ];
    let inner = |l: &[u8; 32], r: &[u8; 32]| -> [u8; 32] {
        let mut both = l.to_vec();
        both.extend(r);
        sha256d::Hash::hash(&both).to_byte_array()
    };
    let n01 = inner(&leaves[0], &leaves[1]);
    let n23 = inner(&leaves[2], &leaves[3]);
    assert_eq!(n01, fake_coinbase.compute_txid().to_byte_array());
    assert_eq!(n23, fake_payment.compute_txid().to_byte_array());
    let root = inner(&n01, &n23);
    let header = mine_header(TxMerkleNode::from_byte_array(root), 1_000);
    let mut chain = TestChain::new(5);
    chain.add(header.block_hash(), 5);
    // The proof is well formed, the root is right, the header is on the
    // chain, the IDs match and the coinbase is a coinbase. Only the length
    // of the two "transactions" gives the attack away.
    let proof = raw_proof(&header, 2, &[n01, n23], &[true, true, true]);
    refused(
        verify_box_proof(&chain, &proof, &a, &p),
        Refusal::CoinbaseSixtyFourBytes,
        "WR-16",
    );
    // With only the payment forged, the payment is what is refused.
    let b = block(vec![coinbase(1), fake_payment.clone()]);
    let chain = TestChain::new(5).with(&b, 5);
    refused(
        verify_box_proof(&chain, &proof_bytes(&b, &[0, 1]), &bytes(&b.txdata[0]), &p),
        Refusal::PaymentSixtyFourBytes,
        "WR-16",
    );
}

#[test]
fn wr_16_a_real_coinbase_cannot_be_at_the_wrong_depth() {
    // In a block of four transactions, the proof for positions 0 and 3
    // puts both at depth 2. The inner node of the right half is given as a
    // plain hash and can not be marked as a transaction.
    let b = block(vec![
        coinbase(1),
        spend(2, &[(&script(0x70), 1)]),
        spend(3, &[(&script(0x71), 1)]),
        spend(4, &[(&pay_script(), 9)]),
    ]);
    let chain = TestChain::new(5).with(&b, 5);
    let proven = verify_box_proof(
        &chain,
        &proof_bytes(&b, &[0, 3]),
        &bytes(&b.txdata[0]),
        &bytes(&b.txdata[3]),
    )
    .unwrap();
    assert_eq!(proven.position, 3);
    // Claiming a count of 2 over the same tree gives another root.
    let txids: Vec<[u8; 32]> = b
        .txdata
        .iter()
        .map(|t| t.compute_txid().to_byte_array())
        .collect();
    let short = raw_proof(&b.header, 2, &[txids[0], txids[3]], &[true, true, true]);
    refused(
        verify_box_proof(&chain, &short, &bytes(&b.txdata[0]), &bytes(&b.txdata[3])),
        Refusal::TreeRootMismatch,
        "WR-12",
    );
}

#[test]
fn wr_16_the_checks_run_in_the_order_of_the_rules() {
    // A header that fails its own proof of work, a tree of one transaction
    // and a coinbase that does not match: the earliest rule is named.
    let b = block(vec![coinbase(1)]);
    let header = unmined_header(b.header.merkle_root);
    let proof = raw_proof(&header, 1, &[[0; 32]], &[true]);
    refused(
        verify_box_proof(&TestChain::new(1), &proof, &bytes(&coinbase(5)), &[0]),
        Refusal::PaymentMalformed,
        "WR-15",
    );
    refused(
        verify_box_proof(
            &TestChain::new(1),
            &proof,
            &bytes(&coinbase(5)),
            &bytes(&coinbase(6)),
        ),
        Refusal::HeaderProofOfWork,
        "WR-03",
    );
}
