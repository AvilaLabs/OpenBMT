//! SP-10 to SP-39: the planner's limits, each at the exact edge and one
//! past it: the amounts, the weight, the fee, the dust limit, the number of
//! inputs.

mod common;

use bitcoin::hashes::Hash;
use bitcoin::{PubkeyHash, ScriptBuf, ScriptHash, WScriptHash};
use common::*;
use openbmt_signer::{MAX_FEE_SATS, MAX_MONEY_SATS, MAX_WEIGHT};
use openbmt_spend::{
    consolidation_cost_sats, consolidation_size, plan, receipt_spend_cost_sats,
    receipt_spend_cost_sats_single, Coin, PlanError,
};

fn plan_of(
    coins: &[Coin],
    payments: Vec<(ScriptBuf, u64)>,
    rate: u64,
) -> Result<openbmt_spend::Plan, PlanError> {
    plan(wallet(), coins, &request(payments, rate))
}

// ---- amounts -------------------------------------------------------------

#[test]
fn sp_34_payments_that_total_the_most_money_there_is_are_not_refused_for_their_amount() {
    let coins = [coin(1, 0, 300_000)];
    // Exactly the most: the coins do not cover it, which is another error.
    let at = plan_of(&coins, vec![(external(1), MAX_MONEY_SATS)], 1000).unwrap_err();
    assert!(matches!(at, PlanError::InsufficientFunds { .. }), "{at:?}");
    // One more is too large, whatever the coins hold.
    assert_eq!(
        plan_of(&coins, vec![(external(1), MAX_MONEY_SATS + 1)], 1000).unwrap_err(),
        PlanError::AmountTooLarge
    );
    // Two payments that together are the most, and one past it.
    let half = MAX_MONEY_SATS / 2;
    let at = plan_of(
        &coins,
        vec![(external(1), half), (external(2), MAX_MONEY_SATS - half)],
        1000,
    )
    .unwrap_err();
    assert!(matches!(at, PlanError::InsufficientFunds { .. }), "{at:?}");
    assert_eq!(
        plan_of(
            &coins,
            vec![
                (external(1), half),
                (external(2), MAX_MONEY_SATS - half + 1)
            ],
            1000
        )
        .unwrap_err(),
        PlanError::AmountTooLarge
    );
}

#[test]
fn sp_34_a_coin_of_the_most_money_there_is_is_spent_and_one_past_it_is_refused() {
    let p = plan_of(
        &[coin(1, 0, MAX_MONEY_SATS)],
        vec![(external(1), 100_000)],
        1000,
    )
    .unwrap();
    assert_eq!(p.total_in_sats(), MAX_MONEY_SATS);
    assert_eq!(p.total_out_sats() + p.fee_sats(), MAX_MONEY_SATS);
    assert_eq!(
        plan_of(
            &[coin(1, 0, MAX_MONEY_SATS + 1)],
            vec![(external(1), 100_000)],
            1000
        )
        .unwrap_err(),
        PlanError::AmountTooLarge
    );
}

/// The fee of a plan of two coins, a payment and change, at 1 sat/vB.
fn two_input_fee() -> u64 {
    let mut r = request(vec![(external(1), 100_000)], 1000);
    let coins = [coin(1, 0, 300_000), coin(2, 1, 300_000)];
    r.must_spend = coins.iter().map(|c| c.outpoint).collect();
    let p = plan(wallet(), &coins, &r).unwrap();
    assert_eq!(p.inputs().len(), 2);
    assert!(p.change().is_some());
    p.fee_sats()
}

#[test]
fn sp_34_outputs_that_total_the_most_money_there_is_are_made_and_one_more_is_not() {
    let fee = two_input_fee();
    let with = |second: u64| {
        let coins = [coin(1, 0, MAX_MONEY_SATS), coin(2, 1, second)];
        let mut r = request(vec![(external(1), 100_000)], 1000);
        r.must_spend = coins.iter().map(|c| c.outpoint).collect();
        plan(wallet(), &coins, &r)
    };
    // Inputs are the most plus the fee: outputs are exactly the most.
    let at = with(fee).unwrap();
    assert_eq!(at.total_out_sats(), MAX_MONEY_SATS);
    assert_eq!(at.fee_sats(), fee);
    // A satoshi more in the coins and the change takes it: outputs above the
    // most money there is are not a plan.
    let past = with(fee + 1).unwrap_err();
    assert!(matches!(past, PlanError::Internal(_)), "{past:?}");
}

// ---- dust ------------------------------------------------------------------

#[test]
fn sp_22_a_payment_at_the_dust_limit_is_made_and_one_below_is_not() {
    let coins = [coin(1, 0, 300_000)];
    for (script, dust) in [
        (external(1), 294),
        (
            ScriptBuf::new_p2wsh(&WScriptHash::from_byte_array([1; 32])),
            330,
        ),
        (
            ScriptBuf::new_p2pkh(&PubkeyHash::from_byte_array([1; 20])),
            546,
        ),
        (
            ScriptBuf::new_p2sh(&ScriptHash::from_byte_array([1; 20])),
            540,
        ),
    ] {
        assert_eq!(openbmt_spend::dust_limit(&script), Some(dust));
        let p = plan_of(&coins, vec![(script.clone(), dust)], 1000).unwrap();
        assert_eq!(p.payments()[0].sats, dust);
        assert_eq!(
            plan_of(&coins, vec![(script, dust - 1)], 1000).unwrap_err(),
            PlanError::PaymentBelowDust {
                payment: 0,
                sats: dust - 1,
                dust
            }
        );
    }
}

// ---- the fee ---------------------------------------------------------------

/// A rate at which `vsize` virtual bytes cost exactly `fee` satoshis, if one
/// exists.
fn rate_for(vsize: u64, fee: u64) -> Option<u64> {
    let rate = fee * 1000 / vsize;
    (vsize * rate).div_ceil(1000).eq(&fee).then_some(rate)
}

/// A consolidation of `n` coins at the rate that makes its fee `fee`.
fn consolidation_at_fee(fee: u64) -> Result<openbmt_spend::Plan, PlanError> {
    // At least 2000 virtual bytes, so that the fee is not above 1,000 sat/vB.
    for n in 25..120usize {
        let (_, vsize) = consolidation_size(n).unwrap();
        let Some(rate) = rate_for(vsize, fee) else {
            continue;
        };
        let coins: Vec<Coin> = (0..n as u32).map(|i| coin(i + 1, 0, 200_000)).collect();
        let mut r = request(vec![(wallet().change_script(0).unwrap(), 0)], rate);
        r.send_all = true;
        let result = plan(wallet(), &coins, &r);
        if let Ok(p) = &result {
            assert_eq!(p.fee_sats(), fee);
            assert_eq!(consolidation_cost_sats(n, rate), Some(fee));
        }
        return result;
    }
    panic!("no number of coins gives a rate for the fee");
}

#[test]
fn sp_32_a_fee_of_exactly_the_ceiling_is_paid_and_one_more_satoshi_is_refused() {
    let at = consolidation_at_fee(MAX_FEE_SATS).unwrap();
    assert_eq!(at.fee_sats(), MAX_FEE_SATS);
    assert_eq!(
        consolidation_at_fee(MAX_FEE_SATS + 1).unwrap_err(),
        PlanError::FeeTooHigh {
            fee: MAX_FEE_SATS + 1,
            limit: MAX_FEE_SATS
        }
    );
}

#[test]
fn sp_30_the_cost_of_one_receipt_is_its_size_at_the_rate_rounded_up() {
    // One input of one P2WPKH key: 4 * 41 + (1 + 73 + 34) = 272 units, 68 vB.
    assert_eq!(receipt_spend_cost_sats_single(1000), 68);
    assert_eq!(receipt_spend_cost_sats_single(2000), 136);
    assert_eq!(receipt_spend_cost_sats_single(1500), 102);
    assert_eq!(receipt_spend_cost_sats_single(1001), 69);
    assert_eq!(receipt_spend_cost_sats_single(0), 0);
    // One input of the 2-of-3 wallet: 4 * 41 + 254 = 418 units, 105 vB.
    assert_eq!(receipt_spend_cost_sats(1000), 105);
    assert_eq!(receipt_spend_cost_sats(2000), 210);
    assert_eq!(receipt_spend_cost_sats(1500), 158);
    assert_eq!(receipt_spend_cost_sats(0), 0);
}

#[test]
fn sp_38_the_cost_of_a_consolidation_is_the_size_at_the_rate_rounded_up() {
    // 158 vB for one coin, 105 more for each.
    assert_eq!(consolidation_cost_sats(1, 1000), Some(158));
    assert_eq!(consolidation_cost_sats(1, 1001), Some(159));
    let (_, two) = consolidation_size(2).unwrap();
    assert_eq!(two, 263);
    assert_eq!(consolidation_cost_sats(2, 1000), Some(263));
    assert_eq!(consolidation_cost_sats(2, 2500), Some(658));
    // Rounds: 400 coins and 1 coin are two consolidations.
    let (_, full) = consolidation_size(400).unwrap();
    assert_eq!(consolidation_cost_sats(401, 1000), Some(full + 158));
}

// ---- the weight ------------------------------------------------------------

/// The weight of a transaction of `n` inputs of the 2-of-3 wallet and
/// outputs whose scripts have `lens`, from the shape, with n of 253 or more
/// and fewer than 253 outputs (the compact sizes are 3 and 1 bytes).
fn weight_of(n: u64, lens: &[u64]) -> u64 {
    let outputs: u64 = lens.iter().map(|l| 8 + 1 + l).sum();
    let base = 4 + 3 + 1 + 4 + 41 * n + outputs;
    base * 4 + 254 * n + 2
}

/// A payment script of the kind with a script of `len` bytes: 22, 34, 25 or 23.
fn script_of_len(len: u64, n: u8) -> ScriptBuf {
    match len {
        22 => external(n),
        34 => ScriptBuf::new_p2wsh(&WScriptHash::from_byte_array([n; 32])),
        25 => ScriptBuf::new_p2pkh(&PubkeyHash::from_byte_array([n; 20])),
        23 => ScriptBuf::new_p2sh(&ScriptHash::from_byte_array([n; 20])),
        _ => unreachable!(),
    }
}

/// `payments` payments to scripts of the lengths `22 x a, 34 x b, 25 x c,
/// 23 x d`, each of `each` satoshis.
fn payments_of(a: u8, b: u8, c: u8, d: u8, each: u64) -> (Vec<(ScriptBuf, u64)>, Vec<u64>) {
    let mut payments = Vec::new();
    let mut lens = Vec::new();
    let mut n = 0u8;
    for (count, len) in [(a, 22u64), (b, 34), (c, 25), (d, 23)] {
        for _ in 0..count {
            n += 1;
            payments.push((script_of_len(len, n), each));
            lens.push(len);
        }
    }
    (payments, lens)
}

fn coins_of(n: u32, sats: u64) -> Vec<Coin> {
    (0..n).map(|i| coin(i + 1, 0, sats)).collect()
}

#[test]
fn sp_33_a_transaction_of_exactly_the_weight_limit_is_made_and_one_input_more_is_not() {
    // 100 payments, so no change: 44 to P2WPKH, 53 to P2WSH, 2 to P2PKH, 1 to
    // P2SH. 921 coins of 1,000 satoshis, a fee of 100,000, payments of
    // 821,000: the coins are all needed, and the weight is the limit.
    let (payments, lens) = payments_of(44, 53, 2, 1, 8_210);
    assert_eq!(payments.len(), 100);
    assert_eq!(weight_of(921, &lens), MAX_WEIGHT);
    let at = plan_of(&coins_of(925, 1_000), payments.clone(), 1000).unwrap();
    assert_eq!(at.weight(), MAX_WEIGHT);
    assert_eq!(at.inputs().len(), 921);
    assert_eq!(at.fee_sats(), 100_000);
    assert!(at.change().is_none());
    // One satoshi more to pay: another input, one over the limit.
    let (past, _) = payments_of(44, 53, 2, 1, 8_210);
    let mut past = past;
    past[0].1 += 1;
    let e = plan_of(&coins_of(925, 1_000), past, 1000).unwrap_err();
    assert_eq!(
        e,
        PlanError::TooHeavy {
            weight: MAX_WEIGHT + 418,
            limit: MAX_WEIGHT
        }
    );
}

#[test]
fn sp_33_a_transaction_with_its_change_of_exactly_the_weight_limit_is_made() {
    // 10 payments and change: 3 to P2WPKH, 3 to P2WSH, 3 to P2PKH, 1 to
    // P2SH, and the change to P2WSH. 953 coins of 1,000, a fee of 100,000,
    // change of 500, payments of 852,500.
    let (payments, mut lens) = payments_of(3, 3, 3, 1, 85_250);
    lens.push(34);
    assert_eq!(weight_of(953, &lens), MAX_WEIGHT);
    let at = plan_of(&coins_of(955, 1_000), payments, 1000).unwrap();
    assert_eq!(at.weight(), MAX_WEIGHT);
    assert_eq!(at.inputs().len(), 953);
    assert_eq!(at.fee_sats(), 100_000);
    assert_eq!(at.change().unwrap().sats, 500);
}

#[test]
fn sp_33_a_transaction_whose_change_makes_it_one_unit_too_heavy_is_refused() {
    // The same, with one payment to P2SH in place of one to P2WPKH: the
    // payments alone weigh 399,832, the payments and change 400,004.
    let (payments, mut lens) = payments_of(2, 3, 3, 2, 85_250);
    let without = weight_of(953, &lens);
    assert_eq!(without, MAX_WEIGHT - 168);
    lens.push(34);
    assert_eq!(weight_of(953, &lens), MAX_WEIGHT + 4);
    let e = plan_of(&coins_of(955, 1_000), payments, 1000).unwrap_err();
    assert_eq!(
        e,
        PlanError::TooHeavy {
            weight: MAX_WEIGHT + 4,
            limit: MAX_WEIGHT
        }
    );
}

// ---- the compact size of the number of inputs --------------------------------

/// The bytes of a compact size, written out from the specification.
fn compact_size(n: u64) -> u64 {
    if n < 0xfd {
        1
    } else if n <= 0xffff {
        3
    } else if n <= 0xffff_ffff {
        5
    } else {
        9
    }
}

#[test]
fn sp_30_the_count_of_inputs_is_written_as_a_compact_size() {
    for n in [
        1u64,
        252,
        253,
        254,
        0xffff,
        0x1_0000,
        0x1_0001,
        0xffff_ffff,
        0x1_0000_0000,
    ] {
        // Version, the count of inputs, the count of outputs (one), the
        // inputs, the output (value, length, a 34-byte script), lock time.
        let base = 4 + compact_size(n) + 1 + 41 * n + (8 + 1 + 34) + 4;
        let weight = base * 4 + 254 * n + 2;
        assert_eq!(
            consolidation_size(usize::try_from(n).unwrap()),
            Some((weight, weight.div_ceil(4))),
            "{n} inputs"
        );
    }
}

#[test]
fn sp_22_a_cash_out_that_leaves_exactly_the_dust_limit_is_made_and_one_less_is_not() {
    let send_all = |sats: u64| {
        let mut r = request(vec![(external(1), 0)], 1000);
        r.send_all = true;
        plan(wallet(), &[coin(1, 0, sats)], &r)
    };
    let fee = send_all(300_000).unwrap().fee_sats();
    let at = send_all(fee + 294).unwrap();
    assert_eq!(at.payments()[0].sats, 294);
    assert_eq!(
        send_all(fee + 293).unwrap_err(),
        PlanError::SendAllBelowDust {
            sats: 293,
            dust: 294
        }
    );
}
