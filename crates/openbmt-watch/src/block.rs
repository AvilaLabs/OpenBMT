//! Section 3: a confirmation the light client finds in a full block.
//!
//! A filter match alone proves nothing. The light client downloads the
//! block, and this module believes only what the whole block proves.

use std::collections::BTreeSet;

use bitcoin::consensus::deserialize;
use bitcoin::{Block, BlockHash, OutPoint, Script, Txid};

use crate::chain::{check_header, on_chain};
use crate::proof::ProvenPayment;
use crate::{HeaderChain, Refusal, FORBIDDEN_TX_BASE_SIZE, MAX_BLOCK_BYTES};

/// A block that passed every check that does not need the chain: its size,
/// its header on its own (WR-03) and its transactions (WR-21).
///
/// Nothing in it is believed yet. [`WellFormedBlock::payments`] and
/// [`WellFormedBlock::spends`] ask the chain (WR-01) before they answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WellFormedBlock {
    block: Block,
    hash: BlockHash,
}

impl WellFormedBlock {
    /// WR-20, WR-03, WR-21: read a block and check it on its own.
    ///
    /// Also refused: two transactions with one ID. A block that repeats its
    /// last transactions has the same merkle root as the block without the
    /// repeat (CVE-2012-2459), and so is not the block it claims to be.
    pub fn parse(block_bytes: &[u8]) -> Result<Self, Refusal> {
        if block_bytes.len() > MAX_BLOCK_BYTES {
            return Err(Refusal::BlockTooLarge {
                len: block_bytes.len(),
            });
        }
        let block: Block = deserialize(block_bytes).map_err(|_| Refusal::BlockMalformed)?;
        let hash = check_header(&block.header)?;
        if !block.check_merkle_root() {
            return Err(Refusal::BlockMerkleRoot);
        }
        match block.txdata.first() {
            Some(first) if first.is_coinbase() => {}
            _ => return Err(Refusal::BlockNoCoinbase),
        }
        let mut seen = BTreeSet::new();
        for (position, tx) in block.txdata.iter().enumerate() {
            let position = position as u32;
            if position > 0 && tx.is_coinbase() {
                return Err(Refusal::BlockExtraCoinbase { position });
            }
            if !seen.insert(tx.compute_txid()) {
                return Err(Refusal::BlockDuplicateTransaction { position });
            }
        }
        Ok(Self { block, hash })
    }

    /// The block's hash.
    pub fn hash(&self) -> BlockHash {
        self.hash
    }

    /// WR-30, WR-22, WR-01: every transaction after the coinbase that pays
    /// `script` more than zero, in block order. An empty list is not an
    /// error (WR-23); it proves nothing.
    pub fn payments<C: HeaderChain + ?Sized>(
        &self,
        chain: &C,
        script: &Script,
    ) -> Result<Vec<ProvenPayment>, Refusal> {
        let mut found = Vec::new();
        for (position, tx) in self.block.txdata.iter().enumerate().skip(1) {
            let paid = tx
                .output
                .iter()
                .filter(|o| o.script_pubkey.as_script() == script)
                .fold(0u64, |sum, o| sum.saturating_add(o.value.to_sat()));
            if paid == 0 {
                continue;
            }
            let position = position as u32;
            if tx.base_size() == FORBIDDEN_TX_BASE_SIZE {
                return Err(Refusal::BlockPaymentSixtyFourBytes { position });
            }
            found.push((position, tx));
        }
        let height = on_chain(chain, &self.hash)?;
        Ok(found
            .into_iter()
            .map(|(position, tx)| ProvenPayment {
                block: self.hash,
                height,
                position,
                tx: tx.clone(),
            })
            .collect())
    }

    /// WR-01 and the last sentence of WR-42: which transactions of the block
    /// spend any of these coins. Each answer is the coin, the transaction
    /// that spends it and the block.
    pub fn spends<C: HeaderChain + ?Sized>(
        &self,
        chain: &C,
        outpoints: &[OutPoint],
    ) -> Result<Vec<(OutPoint, Txid, BlockHash)>, Refusal> {
        on_chain(chain, &self.hash)?;
        let wanted: BTreeSet<&OutPoint> = outpoints.iter().collect();
        let mut found = Vec::new();
        for tx in &self.block.txdata {
            for input in &tx.input {
                if wanted.contains(&input.previous_output) {
                    found.push((input.previous_output, tx.compute_txid(), self.hash));
                }
            }
        }
        Ok(found)
    }
}

/// Read a block and return the payments to `script` that it proves
/// (WR-20 to WR-23, WR-30, WR-01). The checks run in this order: size,
/// parsing, the header on its own, the merkle root and coinbase rules, the
/// payments, and last the chain.
pub fn payments_in_block<C: HeaderChain + ?Sized>(
    chain: &C,
    block_bytes: &[u8],
    script: &Script,
) -> Result<Vec<ProvenPayment>, Refusal> {
    WellFormedBlock::parse(block_bytes)?.payments(chain, script)
}

/// Read a block and return which of its transactions spend any of the given
/// coins (the last sentence of WR-42). The same checks as
/// [`payments_in_block`] run first. Feed each answer to
/// `SaleWatch::spent_elsewhere`.
pub fn spends_in_block<C: HeaderChain + ?Sized>(
    chain: &C,
    block_bytes: &[u8],
    outpoints: &[OutPoint],
) -> Result<Vec<(OutPoint, Txid, BlockHash)>, Refusal> {
    WellFormedBlock::parse(block_bytes)?.spends(chain, outpoints)
}
