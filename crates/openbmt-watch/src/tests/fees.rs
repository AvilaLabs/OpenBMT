//! `docs/FEE-RULES.md`: one sample from a checked block, and the history.

use bitcoin::hashes::Hash;
use bitcoin::{Amount, Block, BlockHash, Network, Transaction, TxMerkleNode};
use openbmt_core::wire::DecodeError;

use super::kit::*;
use crate::block_fee_sample;
use crate::fees::{subsidy, FeeError, FeeHistory, FeeSample, MAGIC_FEE_HISTORY, MAX_FEE_SAMPLES};

const MAIN: Network = Network::Bitcoin;

/// A coinbase that claims `sats` in all.
fn coinbase_of(tag: u8, sats: u64) -> Transaction {
    let mut tx = coinbase(tag);
    tx.output[0].value = Amount::from_sat(sats);
    tx
}

fn hash(n: u32) -> BlockHash {
    let mut b = [0u8; 32];
    b[..4].copy_from_slice(&n.to_le_bytes());
    BlockHash::from_byte_array(b)
}

fn sample(height: u32, rate: Option<u64>) -> FeeSample {
    FeeSample {
        height,
        block: hash(height),
        fees_sats: rate.unwrap_or(0),
        vsize: 1_000,
        claimed_less: false,
        rate_milli: rate,
    }
}

fn history_of(samples: &[FeeSample]) -> FeeHistory {
    let mut h = FeeHistory::new();
    for s in samples {
        assert!(h.insert(*s));
    }
    h
}

/// The stored form written by hand, in the order given.
fn written(samples: &[FeeSample]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&MAGIC_FEE_HISTORY);
    out.push(1);
    out.extend_from_slice(&(samples.len() as u32).to_le_bytes());
    for s in samples {
        out.extend_from_slice(&s.height.to_le_bytes());
        out.extend_from_slice(&s.block.to_byte_array());
        out.extend_from_slice(&s.fees_sats.to_le_bytes());
        out.extend_from_slice(&s.vsize.to_le_bytes());
        out.push(u8::from(s.claimed_less));
        match s.rate_milli {
            Some(r) => {
                out.push(1);
                out.extend_from_slice(&r.to_le_bytes());
            }
            None => out.push(0),
        }
    }
    out
}

#[test]
fn fe_03_subsidy_by_height_on_mainnet() {
    let at = |h| subsidy(h, MAIN);
    assert_eq!(at(0), 5_000_000_000);
    assert_eq!(at(209_999), 5_000_000_000);
    assert_eq!(at(210_000), 2_500_000_000);
    assert_eq!(at(839_999), 625_000_000);
    assert_eq!(at(840_000), 312_500_000);
    assert_eq!(at(210_000 * 64), 0);
    assert_eq!(at(u32::MAX), 0);
    for n in [Network::Testnet, Network::Testnet4, Network::Signet] {
        assert_eq!(subsidy(210_000, n), 2_500_000_000);
    }
}

#[test]
fn fe_03_regtest_halves_every_150_blocks() {
    let at = |h| subsidy(h, Network::Regtest);
    assert_eq!(at(149), 5_000_000_000);
    assert_eq!(at(150), 2_500_000_000);
    assert_eq!(at(299), 2_500_000_000);
    assert_eq!(at(300), 1_250_000_000);
    assert_eq!(at(150 * 64), 0);
}

#[test]
fn fe_01_a_fee_paying_transaction_gives_its_fee() {
    let fee = 1_234;
    let pay = spend(1, &[(&script(0x70), 1_000)]);
    let b = block(vec![coinbase_of(1, 5_000_000_000 + fee), pay]);
    let s = block_fee_sample(&b, 10, MAIN).unwrap();
    assert_eq!(s.height, 10);
    assert_eq!(s.block, b.block_hash());
    assert_eq!(s.fees_sats, fee);
    assert!(!s.claimed_less);
    // Weight by construction: 80 header bytes and one count byte, then each
    // transaction.
    let weight = 4 * 81 + b.txdata.iter().map(|t| t.weight().to_wu()).sum::<u64>();
    assert_eq!(b.weight().to_wu(), weight);
    assert_eq!(u64::from(s.vsize), weight.div_ceil(4));
    let others = u64::from(s.vsize) - b.txdata[0].vsize() as u64;
    assert_eq!(s.rate_milli, Some(fee * 1000 / others));
}

#[test]
fn fe_04_a_coinbase_below_the_subsidy_gives_no_fees() {
    let b = block(vec![
        coinbase_of(1, 4_999_999_999),
        spend(1, &[(&script(0x70), 1_000)]),
    ]);
    let s = block_fee_sample(&b, 10, MAIN).unwrap();
    assert_eq!(s.fees_sats, 0);
    assert!(s.claimed_less);
    assert_eq!(s.rate_milli, Some(0));
    // Exactly the subsidy is not "less".
    let b = block(vec![coinbase(1), spend(1, &[(&script(0x70), 1_000)])]);
    let s = block_fee_sample(&b, 10, MAIN).unwrap();
    assert!(!s.claimed_less);
    assert_eq!(s.fees_sats, 0);
}

#[test]
fn fe_04_the_subsidy_of_the_height_is_used() {
    let b = block(vec![
        coinbase_of(1, 2_500_000_050),
        spend(1, &[(&script(0x70), 1_000)]),
    ]);
    assert_eq!(block_fee_sample(&b, 210_000, MAIN).unwrap().fees_sats, 50);
    // One block earlier the subsidy is the higher one.
    let s = block_fee_sample(&b, 209_999, MAIN).unwrap();
    assert_eq!(s.fees_sats, 0);
    assert!(s.claimed_less);
    // On regtest the halving is at 150.
    let s = block_fee_sample(&b, 150, Network::Regtest).unwrap();
    assert_eq!(s.fees_sats, 50);
}

#[test]
fn fe_04_a_sum_that_overflows_is_refused() {
    let mut cb = coinbase_of(1, u64::MAX);
    cb.output.push(cb.output[0].clone());
    let b = block(vec![cb]);
    let e = block_fee_sample(&b, 10, MAIN).unwrap_err();
    assert_eq!(e, FeeError::CoinbaseOverflow);
    assert_eq!(e.rule(), "FE-04");
    assert!(e.to_string().starts_with("FE-04"));
}

#[test]
fn fe_06_a_block_with_only_a_coinbase_has_no_rate() {
    let b = block(vec![coinbase_of(1, 5_000_000_100)]);
    let s = block_fee_sample(&b, 10, MAIN).unwrap();
    assert_eq!(s.rate_milli, None);
    assert_eq!(s.fees_sats, 100);
    assert!(s.vsize > 0);
}

#[test]
fn fe_02_a_wrong_merkle_root_is_refused() {
    let b = block_with_root(
        vec![coinbase(1), spend(1, &[(&script(0x70), 1_000)])],
        TxMerkleNode::from_byte_array([7; 32]),
    );
    let e = block_fee_sample(&b, 10, MAIN).unwrap_err();
    assert_eq!(e, FeeError::MerkleRoot);
    assert_eq!(e.rule(), "FE-02");
}

#[test]
fn fe_02_a_first_transaction_that_is_not_a_coinbase_is_refused() {
    let b = block(vec![spend(1, &[(&script(0x70), 1_000)]), coinbase(1)]);
    let e = block_fee_sample(&b, 10, MAIN).unwrap_err();
    assert_eq!(e, FeeError::NoCoinbase);
    assert_eq!(e.rule(), "FE-02");
    // No transactions at all: the root cannot match.
    let empty = Block {
        header: b.header,
        txdata: vec![],
    };
    assert_eq!(
        block_fee_sample(&empty, 10, MAIN),
        Err(FeeError::MerkleRoot)
    );
}

#[test]
fn fe_10_samples_are_kept_in_height_order() {
    let h = history_of(&[sample(30, Some(1)), sample(10, Some(2)), sample(20, None)]);
    let heights: Vec<u32> = h.samples().iter().map(|s| s.height).collect();
    assert_eq!(heights, vec![10, 20, 30]);
}

#[test]
fn fe_10_a_history_holds_at_most_512() {
    let mut h = FeeHistory::new();
    for n in 0..MAX_FEE_SAMPLES as u32 {
        assert!(h.insert(sample(100 + n, Some(1))));
    }
    assert_eq!(h.samples().len(), 512);
    // Above the lowest: the lowest goes.
    assert!(h.insert(sample(10_000, Some(1))));
    assert_eq!(h.samples().len(), 512);
    assert_eq!(h.samples()[0].height, 101);
    // Between two held heights: the lowest goes as well.
    assert!(h.insert(sample(10_500, Some(1))));
    assert_eq!(h.samples()[0].height, 102);
    assert!(h.samples().windows(2).all(|w| w[0].height < w[1].height));
    // Below the lowest: not kept.
    assert!(!h.insert(sample(5, Some(1))));
    assert_eq!(h.samples()[0].height, 102);
    assert_eq!(h.samples().len(), 512);
    // The lowest itself is held already, with the same block.
    assert!(!h.insert(sample(102, Some(1))));
}

#[test]
fn fe_11_a_different_block_at_a_height_replaces_the_first() {
    let mut h = history_of(&[sample(10, Some(5)), sample(11, Some(6))]);
    // The same block again: nothing changes.
    assert!(!h.insert(sample(10, Some(99))));
    assert_eq!(h.samples()[0].rate_milli, Some(5));
    // Another block: a reorganization.
    let mut other = sample(10, Some(7));
    other.block = hash(9_999);
    assert!(h.insert(other));
    assert_eq!(h.samples().len(), 2);
    assert_eq!(h.samples()[0], other);
}

#[test]
fn fe_15_forget_from_drops_that_height_and_above() {
    let mut h = history_of(&[sample(10, None), sample(20, None), sample(30, None)]);
    h.forget_from(31);
    assert_eq!(h.samples().len(), 3);
    h.forget_from(20);
    assert_eq!(h.samples().len(), 1);
    h.forget_from(11);
    assert_eq!(h.samples().len(), 1);
    h.forget_from(10);
    assert!(h.samples().is_empty());
    h.forget_from(0);
}

#[test]
fn fe_13_recent_rate_needs_three_samples_with_a_rate() {
    let mut h = history_of(&[sample(1_000, Some(10)), sample(1_001, Some(30))]);
    assert_eq!(h.recent_rate(1_001), None);
    h.insert(sample(1_002, None));
    assert_eq!(h.recent_rate(1_002), None, "no rate does not count");
    h.insert(sample(1_003, Some(20)));
    assert_eq!(h.recent_rate(1_003), Some(20));
    // An even count takes the lower middle.
    h.insert(sample(1_004, Some(40)));
    assert_eq!(h.recent_rate(1_004), Some(20));
}

#[test]
fn fe_13_recent_rate_looks_at_144_blocks() {
    let h = history_of(&[
        sample(100, Some(1)),
        sample(101, Some(2)),
        sample(102, Some(3)),
    ]);
    // tip - 143 = 100: all three are in.
    assert_eq!(h.recent_rate(243), Some(2));
    // tip - 143 = 101: one has left.
    assert_eq!(h.recent_rate(244), None);
    // A sample above the tip is not counted.
    assert_eq!(h.recent_rate(101), None);
    // Near the start of the chain the window does not wrap.
    assert_eq!(h.recent_rate(102), Some(2));
}

const TIP: u32 = 10_000;

/// 17 older samples at heights from `first`, with rates 200 and up, and
/// three recent ones at the tip, each with `recent`.
fn window(first: u32, recent: u64) -> FeeHistory {
    let mut h = FeeHistory::new();
    for i in 0..17u32 {
        h.insert(sample(first + i, Some(200 + u64::from(i))));
    }
    for i in 0..3u32 {
        h.insert(sample(TIP - i, Some(recent)));
    }
    h
}

#[test]
fn fe_14_is_low_needs_20_samples_with_a_rate() {
    let h = window(6_000, 100);
    assert_eq!(h.samples().len(), 20);
    assert_eq!(h.is_low(TIP), Some(true));
    // 19: no answer.
    let mut short = FeeHistory::new();
    for s in h.samples().iter().skip(1) {
        short.insert(*s);
    }
    assert_eq!(short.samples().len(), 19);
    assert_eq!(short.is_low(TIP), None);
    // A sample without a rate does not count towards the 20.
    let mut padded = short.clone();
    padded.insert(sample(6_500, None));
    assert_eq!(padded.samples().len(), 20);
    assert_eq!(padded.is_low(TIP), None);
}

#[test]
fn fe_14_is_low_needs_a_recent_rate() {
    let mut h = FeeHistory::new();
    for i in 0..25u32 {
        h.insert(sample(5_000 + i, Some(200)));
    }
    assert_eq!(h.recent_rate(TIP), None);
    assert_eq!(h.is_low(TIP), None);
}

#[test]
fn fe_14_is_low_is_at_or_below_the_quartile() {
    // 20 rates; the fifth lowest is the 25th percentile (rank 5).
    // Old rates 200..=216, recent rate 204: sorted 200..=203, 204, 204...
    assert_eq!(window(6_000, 204).recent_rate(TIP), Some(204));
    assert_eq!(window(6_000, 204).is_low(TIP), Some(true));
    // One above: the fifth lowest is 204, so 205 is not low.
    assert_eq!(window(6_000, 205).is_low(TIP), Some(false));
    assert_eq!(window(6_000, 5_000).is_low(TIP), Some(false));
    assert_eq!(window(6_000, 1).is_low(TIP), Some(true));
}

#[test]
fn fe_14_the_window_is_4032_blocks() {
    // 5,969 is TIP - 4,031: inside.
    assert_eq!(window(5_969, 100).is_low(TIP), Some(true));
    // One lower start: the lowest-height sample is outside, leaving 19.
    assert_eq!(window(5_968, 100).is_low(TIP), None);
}

#[test]
fn fe_12_a_history_reads_back_as_it_was_written() {
    let mut claimed = sample(12, Some(0));
    claimed.claimed_less = true;
    let h = history_of(&[sample(10, Some(5)), claimed, sample(14, None)]);
    let bytes = h.to_bytes();
    assert_eq!(&bytes[..4], b"OBMj");
    assert_eq!(bytes[4], 1);
    assert_eq!(bytes, written(h.samples()));
    let back = FeeHistory::from_bytes(&bytes).unwrap();
    assert_eq!(back, h);
    assert_eq!(back.to_bytes(), bytes);
    let empty = FeeHistory::new();
    assert_eq!(FeeHistory::from_bytes(&empty.to_bytes()), Ok(empty));
}

#[test]
fn fe_12_a_cut_history_is_refused_at_every_length() {
    let bytes = history_of(&[sample(10, Some(5)), sample(11, None)]).to_bytes();
    for len in 0..bytes.len() {
        assert!(FeeHistory::from_bytes(&bytes[..len]).is_err(), "{len}");
    }
}

#[test]
fn fe_12_a_trailing_byte_is_refused() {
    let mut bytes = history_of(&[sample(10, Some(5))]).to_bytes();
    bytes.push(0);
    assert_eq!(
        FeeHistory::from_bytes(&bytes),
        Err(DecodeError::TrailingBytes)
    );
}

#[test]
fn fe_12_the_magic_and_version_are_checked() {
    let bytes = history_of(&[sample(10, Some(5))]).to_bytes();
    let mut wrong = bytes.clone();
    wrong[3] = b'J';
    assert_eq!(FeeHistory::from_bytes(&wrong), Err(DecodeError::BadMagic));
    let mut wrong = bytes;
    wrong[4] = 2;
    assert_eq!(
        FeeHistory::from_bytes(&wrong),
        Err(DecodeError::UnsupportedVersion(2))
    );
}

#[test]
fn fe_12_no_flipped_byte_reads_as_another_history() {
    let mut claimed = sample(12, Some(0));
    claimed.claimed_less = true;
    let h = history_of(&[sample(10, Some(5)), claimed, sample(14, None)]);
    let bytes = h.to_bytes();
    for at in 0..bytes.len() {
        for bit in 0..8 {
            let mut flipped = bytes.clone();
            flipped[at] ^= 1 << bit;
            if let Ok(other) = FeeHistory::from_bytes(&flipped) {
                assert_eq!(other.to_bytes(), flipped, "{at}/{bit}");
                assert_ne!(other, h, "{at}/{bit}");
            }
        }
    }
}

#[test]
fn fe_12_heights_must_ascend() {
    let down = written(&[sample(11, Some(5)), sample(10, Some(5))]);
    assert!(matches!(
        FeeHistory::from_bytes(&down),
        Err(DecodeError::NonCanonical(_))
    ));
    let twice = written(&[sample(10, Some(5)), sample(10, Some(5))]);
    assert!(matches!(
        FeeHistory::from_bytes(&twice),
        Err(DecodeError::NonCanonical(_))
    ));
    let up = written(&[sample(10, Some(5)), sample(11, Some(5))]);
    assert!(FeeHistory::from_bytes(&up).is_ok());
}

#[test]
fn fe_12_more_than_512_samples_are_refused() {
    let mut out = written(&[]);
    out[5..9].copy_from_slice(&513u32.to_le_bytes());
    assert_eq!(FeeHistory::from_bytes(&out), Err(DecodeError::TooLarge));
    // 512 is allowed.
    let all: Vec<FeeSample> = (0..512).map(|n| sample(n, Some(1))).collect();
    assert_eq!(
        FeeHistory::from_bytes(&written(&all))
            .unwrap()
            .samples()
            .len(),
        512
    );
}

#[test]
fn fe_12_a_claim_of_less_with_fees_is_refused() {
    let mut s = sample(10, Some(5));
    s.claimed_less = true;
    s.fees_sats = 5;
    assert!(matches!(
        FeeHistory::from_bytes(&written(&[s])),
        Err(DecodeError::NonCanonical(_))
    ));
}

#[test]
fn fe_12_a_size_above_a_block_is_refused() {
    let mut s = sample(10, Some(5));
    s.vsize = 1_000_001;
    assert_eq!(
        FeeHistory::from_bytes(&written(&[s])),
        Err(DecodeError::TooLarge)
    );
    s.vsize = 1_000_000;
    assert!(FeeHistory::from_bytes(&written(&[s])).is_ok());
}
