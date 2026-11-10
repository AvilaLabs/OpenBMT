//! Section 1: the header chain.
//!
//! The light client builds the chain and checks it. The library asks three
//! questions about it and repeats none of that work.

use bitcoin::block::Header;
use bitcoin::BlockHash;

use crate::Refusal;

/// The best header chain, as the caller's light client knows it right now.
pub trait HeaderChain {
    /// Height of this block on the best chain, if it is there now.
    fn height_of(&self, block: &BlockHash) -> Option<u32>;
    /// Height of the tip.
    fn tip_height(&self) -> u32;
    /// The time in the tip's header, in seconds.
    fn tip_time(&self) -> u64;
}

/// WR-02: the confirmations of a payment in this block: the tip height minus
/// the block's height, plus one. `None` when the block is not on the best
/// chain (WR-01), or when the chain answers a height above its own tip.
pub fn confirmations<C: HeaderChain + ?Sized>(chain: &C, block: &BlockHash) -> Option<u32> {
    let height = chain.height_of(block)?;
    let tip = chain.tip_height();
    if height > tip {
        return None;
    }
    (tip - height).checked_add(1)
}

/// WR-03: the header's hash meets the target its own bits field states.
/// Returns the block hash.
pub fn check_header(header: &Header) -> Result<BlockHash, Refusal> {
    header
        .validate_pow(header.target())
        .map_err(|_| Refusal::HeaderProofOfWork)
}

/// WR-01: the height of the block if it is on the best chain now.
pub(crate) fn on_chain<C: HeaderChain + ?Sized>(
    chain: &C,
    block: &BlockHash,
) -> Result<u32, Refusal> {
    match (confirmations(chain, block), chain.height_of(block)) {
        (Some(_), Some(height)) => Ok(height),
        _ => Err(Refusal::NotOnChain),
    }
}
