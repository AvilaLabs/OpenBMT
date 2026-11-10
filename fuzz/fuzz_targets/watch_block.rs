#![no_main]

use bitcoin::{BlockHash, ScriptBuf};
use libfuzzer_sys::fuzz_target;
use openbmt_watch::{payments_in_block, HeaderChain};

/// A chain that knows every block, so that the block checks are all that
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

fuzz_target!(|data: &[u8]| {
    // The script is the first bytes of the input, so that a block that
    // pays it can be found; the block is the rest.
    let Some((&len, rest)) = data.split_first() else {
        return;
    };
    let Some((script, block)) = rest.split_at_checked(usize::from(len % 40)) else {
        return;
    };
    let script = ScriptBuf::from_bytes(script.to_vec());
    // It must never panic. A refusal names a rule.
    match payments_in_block(&KnowsEveryBlock, block, &script) {
        Ok(found) => {
            let mut last = 0;
            for payment in found {
                // After the coinbase, in block order, paying the script,
                // and not 64 bytes long without its witness.
                assert!(payment.position > last);
                last = payment.position;
                assert!(!payment.tx.is_coinbase());
                assert_ne!(payment.tx.base_size(), 64);
                assert!(payment.tx.output.iter().any(|o| o.script_pubkey == script));
            }
        }
        Err(refusal) => {
            assert!(refusal.to_string().starts_with(refusal.rule()));
        }
    }
});
