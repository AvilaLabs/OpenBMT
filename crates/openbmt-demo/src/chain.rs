//! The chain the demo simulates: a mempool and blocks, kept in memory. The
//! blocks are real ones, mined at the regtest target, so the counter's own
//! checks of a block run on them as they would on a block from the network.

use std::collections::BTreeMap;

use bitcoin::absolute::LockTime;
use bitcoin::block::{Header, Version as BlockVersion};
use bitcoin::blockdata::constants::genesis_block;
use bitcoin::hashes::Hash;
use bitcoin::transaction::Version;
use bitcoin::{
    Amount as Sats, Block, BlockHash, CompactTarget, Network, OutPoint, ScriptBuf, Sequence,
    Transaction, TxIn, TxMerkleNode, TxOut, Txid, WScriptHash, Witness,
};
use openbmt_watch::HeaderChain;

/// The height of the chain's tip when the demo starts.
pub const START_HEIGHT: u32 = 100;

/// The regtest target: about one try in two meets it.
const REGTEST_BITS: u32 = 0x207f_ffff;

/// The blocks, the headers' heights and the transactions that wait.
#[derive(Debug)]
pub struct SimChain {
    heights: BTreeMap<BlockHash, u32>,
    tip: u32,
    tip_hash: BlockHash,
    time: u64,
    mempool: Vec<Transaction>,
    mined: u32,
    blocks: Vec<MinedBlock>,
}

/// What the page is told of a mined block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MinedBlock {
    /// The height.
    pub height: u32,
    /// The ids of its transactions after the coinbase.
    pub txids: Vec<Txid>,
}

impl SimChain {
    /// A chain whose tip is at [`START_HEIGHT`], at time `now`.
    pub fn new(now: u64) -> Self {
        Self {
            heights: BTreeMap::new(),
            tip: START_HEIGHT,
            tip_hash: BlockHash::all_zeros(),
            time: now,
            mempool: Vec::new(),
            mined: 0,
            blocks: Vec::new(),
        }
    }

    /// A chain that is only the genesis block of `network`, at time `now`:
    /// the chain a counter's own header view starts from, so that the blocks
    /// mined here are the ones it is told of.
    pub fn on_genesis(network: Network, now: u64) -> Self {
        let genesis = genesis_block(network).block_hash();
        Self {
            heights: BTreeMap::from([(genesis, 0)]),
            tip: 0,
            tip_hash: genesis,
            time: now,
            mempool: Vec::new(),
            mined: 0,
            blocks: Vec::new(),
        }
    }

    /// The hash of the tip.
    pub fn tip_hash(&self) -> BlockHash {
        self.tip_hash
    }

    /// Put a transaction in the mempool.
    pub fn broadcast(&mut self, tx: Transaction) {
        self.mempool.push(tx);
    }

    /// How many transactions wait in the mempool.
    pub fn mempool_len(&self) -> usize {
        self.mempool.len()
    }

    /// The transactions that wait: each id and what it pays in all.
    pub fn pool(&self) -> Vec<(Txid, u64)> {
        self.mempool
            .iter()
            .map(|t| {
                (
                    t.compute_txid(),
                    t.output.iter().map(|o| o.value.to_sat()).sum(),
                )
            })
            .collect()
    }

    /// The blocks mined so far, oldest first.
    pub fn blocks(&self) -> &[MinedBlock] {
        &self.blocks
    }

    /// Mine a block of everything in the mempool, at time `now`, and make it
    /// the tip. The block is returned.
    pub fn mine(&mut self, now: u64) -> Block {
        self.mined += 1;
        let mut txdata = vec![coinbase(self.mined)];
        txdata.append(&mut self.mempool);
        let mut block = Block {
            header: header(self.tip_hash, TxMerkleNode::all_zeros(), now),
            txdata,
        };
        if let Some(root) = block.compute_merkle_root() {
            block.header = header(self.tip_hash, root, now);
        }
        self.tip += 1;
        self.blocks.push(MinedBlock {
            height: self.tip,
            txids: block
                .txdata
                .iter()
                .skip(1)
                .map(Transaction::compute_txid)
                .collect(),
        });
        self.tip_hash = block.block_hash();
        self.time = now;
        self.heights.insert(self.tip_hash, self.tip);
        block
    }
}

impl HeaderChain for SimChain {
    fn height_of(&self, block: &BlockHash) -> Option<u32> {
        self.heights.get(block).copied()
    }
    fn tip_height(&self) -> u32 {
        self.tip
    }
    fn tip_time(&self) -> u64 {
        self.time
    }
}

/// A header on `prev` that meets the regtest target.
fn header(prev: BlockHash, root: TxMerkleNode, now: u64) -> Header {
    let mut header = Header {
        version: BlockVersion::ONE,
        prev_blockhash: prev,
        merkle_root: root,
        time: u32::try_from(now).unwrap_or(u32::MAX),
        bits: CompactTarget::from_consensus(REGTEST_BITS),
        nonce: 0,
    };
    while header.validate_pow(header.target()).is_err() {
        header.nonce = header.nonce.wrapping_add(1);
    }
    header
}

/// The script the demo's blocks pay their reward to. Nothing can spend it
/// and nothing in the demo does.
fn miner_script() -> ScriptBuf {
    ScriptBuf::new_p2wsh(&WScriptHash::from_byte_array([0xee; 32]))
}

/// A coinbase; `tag` makes its id differ from the other blocks'.
fn coinbase(tag: u32) -> Transaction {
    let [a, b, c, d] = tag.to_le_bytes();
    Transaction {
        version: Version::TWO,
        lock_time: LockTime::ZERO,
        input: vec![TxIn {
            previous_output: OutPoint::null(),
            script_sig: ScriptBuf::from_bytes(vec![0x04, a, b, c, d]),
            sequence: Sequence::MAX,
            witness: Witness::new(),
        }],
        output: vec![TxOut {
            value: Sats::from_sat(5_000_000_000),
            script_pubkey: miner_script(),
        }],
    }
}

/// The customer's payment: a transaction that spends the coin `coin` and
/// pays `script` the `sats`. It carries no signature: the counter reads what
/// a transaction pays, and the demo's chain does not check spending.
pub fn payment(coin: [u8; 32], script: &ScriptBuf, sats: u64) -> Transaction {
    Transaction {
        version: Version::TWO,
        lock_time: LockTime::ZERO,
        input: vec![TxIn {
            previous_output: OutPoint {
                txid: Txid::from_byte_array(coin),
                vout: 0,
            },
            script_sig: ScriptBuf::new(),
            sequence: Sequence::MAX,
            witness: Witness::new(),
        }],
        output: vec![TxOut {
            value: Sats::from_sat(sats),
            script_pubkey: script.clone(),
        }],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_chain_on_genesis_mines_on_the_genesis_block() {
        let mut chain = SimChain::on_genesis(Network::Regtest, 1_000);
        let genesis = genesis_block(Network::Regtest).block_hash();
        assert_eq!(chain.tip_height(), 0);
        assert_eq!(chain.height_of(&genesis), Some(0));
        let block = chain.mine(1_010);
        assert_eq!(block.header.prev_blockhash, genesis);
        assert_eq!(chain.height_of(&block.block_hash()), Some(1));
    }

    #[test]
    fn a_mined_block_takes_the_mempool_and_becomes_the_tip() {
        let mut chain = SimChain::new(1_000);
        let script = miner_script();
        chain.broadcast(payment([5; 32], &script, 700));
        assert_eq!(chain.mempool_len(), 1);
        let block = chain.mine(1_010);
        assert_eq!(chain.mempool_len(), 0);
        assert_eq!(block.txdata.len(), 2);
        assert_eq!(chain.tip_height(), START_HEIGHT + 1);
        assert_eq!(chain.height_of(&block.block_hash()), Some(START_HEIGHT + 1));
        let next = chain.mine(1_020);
        assert_eq!(chain.blocks().len(), 2);
        assert_eq!(chain.blocks().first().map(|b| b.txids.len()), Some(1));
        assert_eq!(next.header.prev_blockhash, block.block_hash());
        assert_ne!(next.block_hash(), block.block_hash());
    }
}
