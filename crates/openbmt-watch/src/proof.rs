//! A payment proven to be in a block that is on the best chain.

use bitcoin::{BlockHash, Transaction};

/// A payment proven to be in a block that is on the best chain.
///
/// The proof says the block held the transaction. Whether the transaction
/// pays a sale is for the sale to work out (WR-30), and whether the block is
/// still on the chain is asked again at every status (WR-33).
///
/// Only the block checks make one, so a sale cannot be handed a confirmation
/// that nothing checked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProvenPayment {
    pub(crate) block: BlockHash,
    pub(crate) height: u32,
    pub(crate) position: u32,
    pub(crate) tx: Transaction,
}

impl ProvenPayment {
    /// The block that holds the transaction.
    pub fn block(&self) -> BlockHash {
        self.block
    }

    /// The block's height when it was checked.
    pub fn height(&self) -> u32 {
        self.height
    }

    /// The transaction's position in the block (never 0).
    pub fn position(&self) -> u32 {
        self.position
    }

    /// The transaction in full.
    pub fn tx(&self) -> &Transaction {
        &self.tx
    }
}
