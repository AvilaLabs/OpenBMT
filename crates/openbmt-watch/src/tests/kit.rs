//! Chains, transactions, mined blocks and proofs for the tests.
//!
//! Blocks are mined on regtest difficulty: the nonce is tried until the
//! header meets its own target. Proofs are made with `PartialMerkleTree`
//! or, when a test needs a tree that the library would never build, written
//! out byte by byte.

use std::collections::BTreeMap;

use bitcoin::absolute::LockTime;
use bitcoin::block::{Header, Version as BlockVersion};
use bitcoin::consensus::encode::{serialize, VarInt};
use bitcoin::hashes::Hash;
use bitcoin::merkle_tree::PartialMerkleTree;
use bitcoin::transaction::Version;
use bitcoin::{
    Amount, Block, BlockHash, CompactTarget, MerkleBlock, OutPoint, ScriptBuf, Sequence,
    Transaction, TxIn, TxMerkleNode, TxOut, Txid, Witness,
};

use crate::proof::ProvenPayment;
use crate::{HeaderChain, SaleStatus, SaleWatch};

/// A header chain the tests can change: block hashes with heights, a tip.
pub struct TestChain {
    heights: BTreeMap<BlockHash, u32>,
    pub tip: u32,
    pub time: u64,
}

impl TestChain {
    pub fn new(tip: u32) -> Self {
        Self {
            heights: BTreeMap::new(),
            tip,
            time: 1_700_000_000,
        }
    }

    /// A chain that knows this block at this height.
    pub fn with(mut self, block: &Block, height: u32) -> Self {
        self.heights.insert(block.block_hash(), height);
        self
    }

    pub fn add(&mut self, hash: BlockHash, height: u32) {
        self.heights.insert(hash, height);
    }

    /// The block leaves the best chain.
    pub fn remove(&mut self, hash: &BlockHash) {
        self.heights.remove(hash);
    }
}

impl HeaderChain for TestChain {
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

/// A script that is a P2WPKH-shaped output script for the byte `n`.
pub fn script(n: u8) -> ScriptBuf {
    let mut bytes = vec![0x00, 0x14];
    bytes.extend([n; 20]);
    ScriptBuf::from_bytes(bytes)
}

/// A coin that is not null.
pub fn coin(n: u8) -> OutPoint {
    OutPoint {
        txid: Txid::from_byte_array([n; 32]),
        vout: 0,
    }
}

fn input(previous_output: OutPoint, script_sig: ScriptBuf) -> TxIn {
    TxIn {
        previous_output,
        script_sig,
        sequence: Sequence::MAX,
        witness: Witness::new(),
    }
}

fn output(script: &ScriptBuf, sats: u64) -> TxOut {
    TxOut {
        value: Amount::from_sat(sats),
        script_pubkey: script.clone(),
    }
}

/// A transaction spending the coin `prev` and paying each (script, sats).
pub fn spend(prev: u8, outs: &[(&ScriptBuf, u64)]) -> Transaction {
    spend_coins(&[coin(prev)], outs)
}

/// A transaction spending these coins.
pub fn spend_coins(coins: &[OutPoint], outs: &[(&ScriptBuf, u64)]) -> Transaction {
    Transaction {
        version: Version::TWO,
        lock_time: LockTime::ZERO,
        input: coins.iter().map(|c| input(*c, ScriptBuf::new())).collect(),
        output: outs.iter().map(|(s, v)| output(s, *v)).collect(),
    }
}

/// A coinbase; `tag` makes its ID different from other coinbases.
pub fn coinbase(tag: u8) -> Transaction {
    Transaction {
        version: Version::TWO,
        lock_time: LockTime::ZERO,
        input: vec![input(
            OutPoint::null(),
            ScriptBuf::from_bytes(vec![0x02, tag, tag]),
        )],
        output: vec![output(&script(0xee), 5_000_000_000)],
    }
}

/// A transaction that is 64 bytes long without its witness: one input with
/// an empty script, one output with a script of `script_len` bytes (so the
/// input's script is `4 - script_len` bytes).
pub fn sixty_four(is_coinbase: bool, script_len: usize) -> Transaction {
    assert!(script_len <= 4);
    let prev = if is_coinbase {
        OutPoint::null()
    } else {
        coin(0x64)
    };
    let tx = Transaction {
        version: Version::ONE,
        lock_time: LockTime::ZERO,
        input: vec![input(
            prev,
            ScriptBuf::from_bytes(vec![0x51; 4 - script_len]),
        )],
        output: vec![TxOut {
            value: Amount::from_sat(1_000),
            script_pubkey: ScriptBuf::from_bytes(vec![0x51; script_len]),
        }],
    };
    assert_eq!(tx.base_size(), 64);
    tx
}

pub fn bytes(tx: &Transaction) -> Vec<u8> {
    serialize(tx)
}

/// Regtest bits.
const BITS: u32 = 0x207f_ffff;

/// A header over this merkle root, mined until it meets its own target.
pub fn mine_header(root: TxMerkleNode, time: u32) -> Header {
    let mut header = Header {
        version: BlockVersion::ONE,
        prev_blockhash: BlockHash::all_zeros(),
        merkle_root: root,
        time,
        bits: CompactTarget::from_consensus(BITS),
        nonce: 0,
    };
    while header.validate_pow(header.target()).is_err() {
        header.nonce += 1;
    }
    header
}

/// A header whose hash does not meet the target its bits state.
pub fn unmined_header(root: TxMerkleNode) -> Header {
    let mut header = Header {
        version: BlockVersion::ONE,
        prev_blockhash: BlockHash::all_zeros(),
        merkle_root: root,
        time: 1,
        bits: CompactTarget::from_consensus(0x1d00_ffff),
        nonce: 0,
    };
    while header.validate_pow(header.target()).is_ok() {
        header.nonce += 1;
    }
    header
}

/// A mined block with these transactions and the right merkle root.
pub fn block(txs: Vec<Transaction>) -> Block {
    block_at(txs, 1_000)
}

pub fn block_at(txs: Vec<Transaction>, time: u32) -> Block {
    let mut block = Block {
        header: mine_header(TxMerkleNode::all_zeros(), time),
        txdata: txs,
    };
    let root = block.compute_merkle_root().expect("has transactions");
    block.header = mine_header(root, time);
    block
}

/// A mined block with these transactions and this (usually wrong) root.
pub fn block_with_root(txs: Vec<Transaction>, root: TxMerkleNode) -> Block {
    Block {
        header: mine_header(root, 1_000),
        txdata: txs,
    }
}

/// A proof for the transactions at these positions of the block.
pub fn proof_bytes(block: &Block, positions: &[usize]) -> Vec<u8> {
    let txids: Vec<Txid> = block.txdata.iter().map(Transaction::compute_txid).collect();
    let matches: Vec<bool> = (0..txids.len()).map(|i| positions.contains(&i)).collect();
    let txn = PartialMerkleTree::from_txids(&txids, &matches);
    serialize(&MerkleBlock {
        header: block.header,
        txn,
    })
}

/// A proof written byte by byte: header, transaction count, hashes, flag
/// bits (least significant bit of each byte first).
pub fn raw_proof(header: &Header, count: u32, hashes: &[[u8; 32]], bits: &[bool]) -> Vec<u8> {
    let mut out = serialize(header);
    out.extend(count.to_le_bytes());
    out.extend(serialize(&VarInt(hashes.len() as u64)));
    for hash in hashes {
        out.extend(hash);
    }
    let flag_bytes: Vec<u8> = bits
        .chunks(8)
        .map(|chunk| {
            chunk
                .iter()
                .enumerate()
                .fold(0u8, |byte, (i, bit)| byte | (u8::from(*bit) << i))
        })
        .collect();
    out.extend(serialize(&VarInt(flag_bytes.len() as u64)));
    out.extend(flag_bytes);
    out
}

/// A block with a coinbase and a payment, the proof for both, and a chain
/// that knows the block at height 100 with the tip at 100.
pub struct Setup {
    pub block: Block,
    pub chain: TestChain,
    pub proof: Vec<u8>,
    pub coinbase: Vec<u8>,
    pub payment: Vec<u8>,
}

pub fn setup(pay_to: &ScriptBuf, sats: u64) -> Setup {
    let txs = vec![
        coinbase(1),
        spend(1, &[(&script(0x77), 1_000)]),
        spend(2, &[(pay_to, sats)]),
        spend(3, &[(&script(0x78), 1_000)]),
        spend(4, &[(&script(0x79), 1_000)]),
    ];
    let block = block(txs);
    let chain = TestChain::new(100).with(&block, 100);
    Setup {
        proof: proof_bytes(&block, &[0, 2]),
        coinbase: bytes(&block.txdata[0]),
        payment: bytes(&block.txdata[2]),
        block,
        chain,
    }
}

/// The call refused with exactly this refusal, and it names this rule.
#[track_caller]
pub fn refused<T: std::fmt::Debug>(
    result: Result<T, crate::Refusal>,
    want: crate::Refusal,
    rule: &str,
) {
    let err = result.expect_err("the call should have refused");
    assert_eq!(err, want);
    assert_eq!(err.rule(), rule);
    assert!(err.to_string().starts_with(&format!("{rule}: ")));
}

/// A proven payment as the proof check would return it.
pub fn proven(block: &Block, position: u32, height: u32) -> ProvenPayment {
    ProvenPayment {
        block: block.block_hash(),
        height,
        position,
        tx: block.txdata[position as usize].clone(),
    }
}

/// The status of the watch, after checking WR-31: what the standing
/// transactions pay adds up to the received amount, they are in ascending
/// order of id and each pays something.
pub fn status_at(w: &mut SaleWatch, chain: &TestChain, now: u64) -> SaleStatus {
    let s = w.status(chain, now);
    let standing = w.standing();
    let sum: u64 = standing.iter().map(|(_, paid)| *paid).sum();
    assert_eq!(sum, s.received_sats, "WR-31: standing adds up");
    assert!(standing.iter().all(|(_, paid)| *paid > 0));
    assert!(standing
        .windows(2)
        .all(|p| p[0].0.to_byte_array() < p[1].0.to_byte_array()));
    s
}
