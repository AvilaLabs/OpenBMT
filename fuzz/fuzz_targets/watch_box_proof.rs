#![no_main]

use bitcoin::BlockHash;
use libfuzzer_sys::fuzz_target;
use openbmt_watch::{verify_box_proof, HeaderChain};

/// A chain that knows every block, so that the proof checks are all that
/// can refuse.
struct KnowsEveryBlock;

impl HeaderChain for KnowsEveryBlock {
    fn height_of(&self, _block: &BlockHash) -> Option<u32> {
        Some(0)
    }
    fn tip_height(&self) -> u32 {
        0
    }
    fn tip_time(&self) -> u64 {
        0
    }
}

/// Split the input into three parts: two 16-bit little-endian lengths, then
/// the proof, the coinbase and the payment. The payment takes what is left.
fn split(data: &[u8]) -> Option<(&[u8], &[u8], &[u8])> {
    let (head, rest) = data.split_at_checked(4)?;
    let proof_len = usize::from(u16::from_le_bytes([head[0], head[1]]));
    let coinbase_len = usize::from(u16::from_le_bytes([head[2], head[3]]));
    let (proof, rest) = rest.split_at_checked(proof_len)?;
    let (coinbase, payment) = rest.split_at_checked(coinbase_len)?;
    Some((proof, coinbase, payment))
}

fuzz_target!(|data: &[u8]| {
    let Some((proof, coinbase, payment)) = split(data) else {
        return;
    };
    // It must never panic. A refusal names a rule.
    match verify_box_proof(&KnowsEveryBlock, proof, coinbase, payment) {
        Ok(proven) => {
            // What passed is a transaction after the coinbase that is not a
            // coinbase and is not 64 bytes long without its witness.
            assert!(proven.position > 0);
            assert!(!proven.tx.is_coinbase());
            assert_ne!(proven.tx.base_size(), 64);
            assert_eq!(proven.height, 0);
        }
        Err(refusal) => {
            assert!(refusal.to_string().starts_with(refusal.rule()));
        }
    }
});
