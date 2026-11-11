//! A fee rate the device checks by itself, from blocks it has fully
//! checked (`docs/FEE-RULES.md`, FE-01 and on).
//!
//! The coinbase of a block claims the subsidy and every fee in it, and the
//! subsidy is fixed by the height. So the fees of a block are what its
//! coinbase claims above the subsidy, and with the block's size that gives
//! its average fee rate. [`FeeHistory`] keeps a bounded list of such
//! samples and answers by block height, never by clock time.

use std::fmt;

use bitcoin::hashes::Hash;
use bitcoin::{Block, BlockHash, Network};
use openbmt_core::wire::{self, DecodeError, Reader};

/// Magic of a stored [`FeeHistory`] (FE-12).
pub const MAGIC_FEE_HISTORY: [u8; 4] = *b"OBMj";
/// FE-10: most samples a history holds.
pub const MAX_FEE_SAMPLES: usize = 512;
/// FE-13: blocks, counting the tip, that `recent_rate` looks at.
pub const RECENT_BLOCKS: u32 = 144;
/// FE-13: fewest samples with a rate that `recent_rate` needs.
pub const MIN_RECENT_SAMPLES: usize = 3;
/// FE-14: blocks, counting the tip, that `is_low` compares against.
pub const LOW_WINDOW_BLOCKS: u32 = 4_032;
/// FE-14: fewest samples with a rate that `is_low` needs.
pub const MIN_WINDOW_SAMPLES: usize = 20;
/// FE-20: blocks after which one more full block is fetched.
pub const SAMPLE_EVERY_BLOCKS: u32 = 24;
/// FE-12: largest block size in virtual bytes a stored sample may state.
pub const MAX_SAMPLE_VSIZE: u32 = 1_000_000;

/// Satoshis in 50 bitcoin.
const INITIAL_SUBSIDY: u64 = 50 * 100_000_000;
/// Bytes of one stored sample without its rate: height, hash, fees, size,
/// the flag for a coinbase that claimed less, and the flag for a rate.
const SAMPLE_MIN_BYTES: usize = 4 + 32 + 8 + 4 + 1 + 1;

/// Why a block gave no fee sample.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FeeError {
    /// FE-02: the transactions do not hash to the header's merkle root.
    MerkleRoot,
    /// FE-02: the block has no transactions or its first is not a coinbase.
    NoCoinbase,
    /// FE-04: the coinbase's outputs do not add up to a 64-bit amount.
    CoinbaseOverflow,
}

impl FeeError {
    /// The rule that the refusal enforces.
    pub fn rule(&self) -> &'static str {
        match self {
            Self::MerkleRoot | Self::NoCoinbase => "FE-02",
            Self::CoinbaseOverflow => "FE-04",
        }
    }
}

impl fmt::Display for FeeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let rule = self.rule();
        match self {
            Self::MerkleRoot => write!(
                f,
                "{rule}: the block's transactions do not match its header"
            ),
            Self::NoCoinbase => {
                write!(f, "{rule}: the block's first transaction is not a coinbase")
            }
            Self::CoinbaseOverflow => write!(f, "{rule}: the coinbase outputs overflow"),
        }
    }
}

impl std::error::Error for FeeError {}

/// FE-03: the subsidy of a block, in satoshis.
pub fn subsidy(height: u32, network: Network) -> u64 {
    let interval: u32 = match network {
        Network::Bitcoin | Network::Testnet | Network::Testnet4 | Network::Signet => 210_000,
        Network::Regtest => 150,
    };
    let halvings = height / interval;
    if halvings >= 64 {
        0
    } else {
        INITIAL_SUBSIDY >> halvings
    }
}

/// What one fully checked block says about fees (FE-01).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FeeSample {
    /// The block's height, as the caller stated it.
    pub height: u32,
    /// The block's hash.
    pub block: BlockHash,
    /// What the coinbase claims above the subsidy, in satoshis.
    pub fees_sats: u64,
    /// The block's size in virtual bytes (weight divided by 4, rounded up).
    pub vsize: u32,
    /// The coinbase claimed less than the subsidy, so `fees_sats` is 0.
    pub claimed_less: bool,
    /// Thousandths of a satoshi per virtual byte, over every transaction
    /// but the coinbase. None for a block with only a coinbase (FE-06).
    pub rate_milli: Option<u64>,
}

/// FE-01 to FE-06: the fee sample of one block.
///
/// The caller passes a block that already passed the watcher's full check
/// (`WR-20` to `WR-22`) and states its height. The merkle root and the
/// coinbase are checked again here, since the sum depends on them.
pub fn block_fee_sample(
    block: &Block,
    height: u32,
    network: Network,
) -> Result<FeeSample, FeeError> {
    if !block.check_merkle_root() {
        return Err(FeeError::MerkleRoot);
    }
    let coinbase = match block.txdata.first() {
        Some(first) if first.is_coinbase() => first,
        _ => return Err(FeeError::NoCoinbase),
    };
    let subsidy = subsidy(height, network);
    let mut claimed: u64 = 0;
    for out in &coinbase.output {
        claimed = claimed
            .checked_add(out.value.to_sat())
            .ok_or(FeeError::CoinbaseOverflow)?;
    }
    let claimed_less = claimed < subsidy;
    let fees_sats = claimed.saturating_sub(subsidy);
    let vsize_wide = block.weight().to_vbytes_ceil();
    let vsize = u32::try_from(vsize_wide).unwrap_or(u32::MAX);
    let rate_milli = if block.txdata.len() > 1 {
        // FE-05: the weight of the transactions that pay a fee, without the
        // coinbase and without the header and count (as Core's `avgfeerate`).
        let others: u128 = block
            .txdata
            .iter()
            .skip(1)
            .map(|tx| u128::from(tx.weight().to_wu()))
            .sum();
        (u128::from(fees_sats) * 4000)
            .checked_div(others)
            .and_then(|r| u64::try_from(r).ok())
    } else {
        None
    };
    Ok(FeeSample {
        height,
        block: block.block_hash(),
        fees_sats,
        vsize,
        claimed_less,
        rate_milli,
    })
}

/// FE-10 to FE-16: samples kept by height, at most one for each block.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FeeHistory {
    /// In ascending order of height, one for each height.
    samples: Vec<FeeSample>,
}

impl FeeHistory {
    /// A history with no samples.
    pub fn new() -> Self {
        Self::default()
    }

    /// The samples, in ascending order of height.
    pub fn samples(&self) -> &[FeeSample] {
        &self.samples
    }

    /// FE-10, FE-11: add a sample. Returns whether the history changed.
    ///
    /// A second sample for a height replaces the first only when its block
    /// hash differs, which means the chain was reorganized. A full history
    /// drops its lowest sample for one above it, and does not keep one
    /// below it.
    pub fn insert(&mut self, sample: FeeSample) -> bool {
        match self
            .samples
            .binary_search_by_key(&sample.height, |s| s.height)
        {
            Ok(at) => match self.samples.get_mut(at) {
                Some(held) if held.block != sample.block => {
                    *held = sample;
                    true
                }
                _ => false,
            },
            Err(at) => {
                if self.samples.len() >= MAX_FEE_SAMPLES {
                    if at == 0 {
                        return false;
                    }
                    self.samples.remove(0);
                    self.samples.insert(at - 1, sample);
                } else {
                    self.samples.insert(at, sample);
                }
                true
            }
        }
    }

    /// FE-15: drop the samples at and above `height`.
    pub fn forget_from(&mut self, height: u32) {
        let keep = self.samples.partition_point(|s| s.height < height);
        self.samples.truncate(keep);
    }

    /// The rates, in ascending order, of the samples within `blocks` blocks
    /// of the tip, counting the tip.
    fn rates(&self, tip: u32, blocks: u32) -> Vec<u64> {
        let from = tip.saturating_sub(blocks - 1);
        let mut rates: Vec<u64> = self
            .samples
            .iter()
            .filter(|s| s.height >= from && s.height <= tip)
            .filter_map(|s| s.rate_milli)
            .collect();
        rates.sort_unstable();
        rates
    }

    /// FE-13: the median rate of the last 144 blocks, in thousandths of a
    /// satoshi per virtual byte. None with fewer than 3 samples with a rate.
    pub fn recent_rate(&self, tip: u32) -> Option<u64> {
        let rates = self.rates(tip, RECENT_BLOCKS);
        if rates.len() < MIN_RECENT_SAMPLES {
            return None;
        }
        rates.get((rates.len() - 1) / 2).copied()
    }

    /// FE-14: whether the recent rate is at or below the 25th percentile of
    /// the last 4,032 blocks. None when there is too little history.
    pub fn is_low(&self, tip: u32) -> Option<bool> {
        let recent = self.recent_rate(tip)?;
        let rates = self.rates(tip, LOW_WINDOW_BLOCKS);
        if rates.len() < MIN_WINDOW_SAMPLES {
            return None;
        }
        let rank = rates.len().div_ceil(4);
        let quartile = rates.get(rank - 1).copied()?;
        Some(recent <= quartile)
    }

    /// FE-17: the usual rate, the median rate of the last 4,032 blocks, in
    /// thousandths of a satoshi per virtual byte. None with fewer than 20
    /// samples with a rate in that window.
    pub fn usual_rate(&self, tip: u32) -> Option<u64> {
        let rates = self.rates(tip, LOW_WINDOW_BLOCKS);
        if rates.len() < MIN_WINDOW_SAMPLES {
            return None;
        }
        rates.get((rates.len() - 1) / 2).copied()
    }

    /// FE-12: the stored form.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = wire::start(&MAGIC_FEE_HISTORY, 4 + self.samples.len() * 55);
        out.extend_from_slice(&wire::len_u32(self.samples.len()).to_le_bytes());
        for s in &self.samples {
            out.extend_from_slice(&s.height.to_le_bytes());
            out.extend_from_slice(&s.block.to_byte_array());
            out.extend_from_slice(&s.fees_sats.to_le_bytes());
            out.extend_from_slice(&s.vsize.to_le_bytes());
            out.push(u8::from(s.claimed_less));
            wire::put_opt_u64(&mut out, s.rate_milli);
        }
        out
    }

    /// FE-12, FE-16: read the stored form, strictly. Nothing is returned
    /// unless every byte is accounted for.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, DecodeError> {
        let mut r = Reader::framed(bytes, &MAGIC_FEE_HISTORY)?;
        let count = r.count(MAX_FEE_SAMPLES, SAMPLE_MIN_BYTES)?;
        let mut samples: Vec<FeeSample> = Vec::with_capacity(count);
        for _ in 0..count {
            let height = r.u32_le()?;
            let block = BlockHash::from_byte_array(r.array()?);
            let fees_sats = r.u64_le()?;
            let vsize = r.u32_le()?;
            if vsize > MAX_SAMPLE_VSIZE {
                return Err(DecodeError::TooLarge);
            }
            let claimed_less = r.flag()?;
            if claimed_less && fees_sats != 0 {
                return Err(DecodeError::NonCanonical("claimed less but fees are not 0"));
            }
            let rate_milli = r.opt_u64()?;
            if samples.last().is_some_and(|last| last.height >= height) {
                return Err(DecodeError::NonCanonical("heights are not ascending"));
            }
            samples.push(FeeSample {
                height,
                block,
                fees_sats,
                vsize,
                claimed_less,
                rate_milli,
            });
        }
        r.finish()?;
        Ok(Self { samples })
    }
}
