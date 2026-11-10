//! Section 2: a confirmation the Box reports.
//!
//! The Box can hold information back; it must not be able to make up a
//! confirmation. A confirmation is believed only with a proof of four parts:
//! the block header and a partial merkle tree (BIP37, the format of
//! `gettxoutproof`), the block's coinbase in full and the payment in full.

use bitcoin::consensus::deserialize;
use bitcoin::merkle_tree::{MerkleBlockError, PartialMerkleTree};
use bitcoin::{BlockHash, MerkleBlock, Transaction, Txid};

use crate::chain::{check_header, on_chain};
use crate::{
    HeaderChain, Refusal, FORBIDDEN_TX_BASE_SIZE, MAX_PROOF_BYTES, MAX_TREE_TRANSACTIONS,
    MAX_TX_BYTES, MIN_TREE_TRANSACTIONS,
};

/// A payment proven to be in a block that is on the best chain.
///
/// The proof says the block held the transaction. Whether the transaction
/// pays a sale is for the sale to work out (WR-30), and whether the block is
/// still on the chain is asked again at every status (WR-33).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProvenPayment {
    /// The block that holds the transaction.
    pub block: BlockHash,
    /// The block's height when it was checked.
    pub height: u32,
    /// The transaction's position in the block (never 0).
    pub position: u32,
    /// The transaction in full.
    pub tx: Transaction,
}

/// Check a confirmation the Box reports (WR-10 to WR-16, WR-01).
///
/// The checks run in this order: sizes, parsing, the header on its own
/// (WR-03), the tree (WR-12), what the tree proves (WR-13), the coinbase
/// (WR-14), the payment (WR-15), the 64-byte rule (WR-16) and last the chain
/// (WR-01), so that a refusal for a bad proof never depends on the state of
/// the chain.
pub fn verify_box_proof<C: HeaderChain + ?Sized>(
    chain: &C,
    proof_bytes: &[u8],
    coinbase_bytes: &[u8],
    payment_bytes: &[u8],
) -> Result<ProvenPayment, Refusal> {
    // WR-10: sizes.
    if proof_bytes.len() > MAX_PROOF_BYTES {
        return Err(Refusal::ProofTooLarge {
            len: proof_bytes.len(),
        });
    }
    if coinbase_bytes.len() > MAX_TX_BYTES {
        return Err(Refusal::CoinbaseTooLarge {
            len: coinbase_bytes.len(),
        });
    }
    if payment_bytes.len() > MAX_TX_BYTES {
        return Err(Refusal::PaymentTooLarge {
            len: payment_bytes.len(),
        });
    }

    // Parsing. `deserialize` refuses bytes left over.
    let proof: MerkleBlock = deserialize(proof_bytes).map_err(|_| Refusal::ProofMalformed)?;
    let coinbase: Transaction =
        deserialize(coinbase_bytes).map_err(|_| Refusal::CoinbaseMalformed)?;
    let payment: Transaction = deserialize(payment_bytes).map_err(|_| Refusal::PaymentMalformed)?;

    // WR-03.
    let block = check_header(&proof.header)?;

    // WR-12: the tree.
    let (txids, positions) = extract_tree(&proof.txn, proof.header.merkle_root)?;

    // WR-13: exactly two transactions, the first at position 0.
    if txids.len() != 2 {
        return Err(Refusal::ProofCoversWrongCount { found: txids.len() });
    }
    if positions[0] != 0 {
        return Err(Refusal::CoinbaseNotFirst {
            position: positions[0],
        });
    }

    // WR-14: the coinbase.
    if coinbase.compute_txid() != txids[0] {
        return Err(Refusal::CoinbaseIdMismatch);
    }
    if !coinbase.is_coinbase() {
        return Err(Refusal::NotACoinbase);
    }

    // WR-15: the payment.
    if payment.compute_txid() != txids[1] {
        return Err(Refusal::PaymentIdMismatch);
    }
    if payment.is_coinbase() {
        return Err(Refusal::PaymentIsCoinbase);
    }

    // WR-16.
    if coinbase.base_size() == FORBIDDEN_TX_BASE_SIZE {
        return Err(Refusal::CoinbaseSixtyFourBytes);
    }
    if payment.base_size() == FORBIDDEN_TX_BASE_SIZE {
        return Err(Refusal::PaymentSixtyFourBytes);
    }

    // WR-01, last.
    let height = on_chain(chain, &block)?;
    Ok(ProvenPayment {
        block,
        height,
        position: positions[1],
        tx: payment,
    })
}

/// WR-12: read the partial merkle tree. Returns the proven transaction IDs
/// and their positions, in order.
///
/// `PartialMerkleTree::extract_matches` enforces: at least one transaction
/// and at most 16,666; no more hashes than transactions; at least as many
/// flag bits as hashes; no running out of bits or hashes while walking the
/// tree; every hash used; every flag byte used; and no node with two equal
/// children. This function adds: at least 2 transactions, spare bits after
/// the last flag bit all zero, and the comparison of the root with the
/// header's.
fn extract_tree(
    tree: &PartialMerkleTree,
    header_root: bitcoin::TxMerkleNode,
) -> Result<(Vec<Txid>, Vec<u32>), Refusal> {
    let count = tree.num_transactions();
    if !(MIN_TREE_TRANSACTIONS..=MAX_TREE_TRANSACTIONS).contains(&count) {
        return Err(Refusal::TreeTransactionCount { count });
    }
    let mut txids = Vec::new();
    let mut positions = Vec::new();
    let root = tree
        .extract_matches(&mut txids, &mut positions)
        .map_err(|e| match e {
            MerkleBlockError::NoTransactions | MerkleBlockError::TooManyTransactions => {
                Refusal::TreeTransactionCount { count }
            }
            MerkleBlockError::TooManyHashes => Refusal::TreeTooManyHashes,
            MerkleBlockError::IdenticalHashesFound => Refusal::TreeEqualChildren,
            MerkleBlockError::MerkleRootMismatch => Refusal::TreeRootMismatch,
            _ => Refusal::TreeBitsAndHashes,
        })?;
    // Bits that the walk did not use and that are not zero would let one
    // proof have many encodings.
    match flag_bits_used(count, tree.bits()) {
        Some(used) if tree.bits()[used..].iter().all(|bit| !bit) => {}
        _ => return Err(Refusal::TreePadding),
    }
    if root != header_root {
        return Err(Refusal::TreeRootMismatch);
    }
    Ok((txids, positions))
}

/// How many flag bits a walk of the tree consumes: one per visited node.
fn flag_bits_used(count: u32, bits: &[bool]) -> Option<usize> {
    fn width(count: u32, height: u32) -> u32 {
        (count + (1 << height) - 1) >> height
    }
    fn walk(count: u32, height: u32, pos: u32, bits: &[bool], used: &mut usize) -> Option<()> {
        let bit = *bits.get(*used)?;
        *used += 1;
        if height == 0 || !bit {
            return Some(());
        }
        walk(count, height - 1, pos * 2, bits, used)?;
        if pos * 2 + 1 < width(count, height - 1) {
            walk(count, height - 1, pos * 2 + 1, bits, used)?;
        }
        Some(())
    }
    let mut height = 0;
    while width(count, height) > 1 {
        height += 1;
    }
    let mut used = 0;
    walk(count, height, 0, bits, &mut used)?;
    Some(used)
}
