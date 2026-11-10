//! SP-30 to SP-37: size, fee and limits.

mod common;

use bitcoin::absolute::LockTime;
use bitcoin::transaction::Version;
use common::*;
use openbmt_spend::{combine, finalize_plan, plan, PlanError};

#[test]
fn sp_30_the_weight_is_the_known_shape() {
    // One input and one P2WPKH output (22-byte script), no change:
    //   non-witness bytes 4 + 1 + 41 + 1 + (8 + 1 + 22) + 4 = 82
    //   witness bytes 2 + (1 + 1 + 73 + 73 + 106) = 256
    //   weight 82 * 4 + 256 = 584, 146 vB.
    // Exactly spending the coin leaves no change.
    let r = request(vec![(external(1), 50_000)], 1000);
    let p = plan(wallet(), &[coin(1, 0, 50_000 + 146)], &r).unwrap();
    assert!(p.change().is_none());
    assert_eq!((p.weight(), p.vsize(), p.fee_sats()), (584, 146, 146));
    // The same with a P2WSH change output (34-byte script): 125 base bytes,
    // 500 + 256 = 756, 189 vB.
    let p = plan(wallet(), &[coin(2, 0, 1_000_000)], &r).unwrap();
    assert_eq!((p.weight(), p.vsize(), p.fee_sats()), (756, 189, 189));
    // Two inputs add 41 * 4 + 254 = 418 each.
    let two = [coin(3, 0, 30_000), coin(4, 1, 30_000)];
    let p = plan(wallet(), &two, &r).unwrap();
    assert_eq!(p.inputs().len(), 2);
    assert_eq!(p.weight(), 756 + 418);
}

#[test]
fn sp_30_the_fee_is_the_rate_times_the_size_rounded_up() {
    let coins = [coin(1, 0, 1_000_000)];
    // 189 vB.
    for (rate, fee) in [
        (1000u64, 189u64),
        (1500, 284), // 283.5
        (2500, 473), // 472.5
        (1234, 234), // 233.226
        (1, 1),      // 0.189
        (10_000, 1890),
    ] {
        let p = plan(
            wallet(),
            &coins,
            &request(vec![(external(1), 50_000)], rate),
        )
        .unwrap();
        assert_eq!(p.vsize(), 189);
        assert_eq!(p.fee_sats(), fee, "rate {rate}");
        assert_eq!(p.requested_rate(), rate);
    }
}

#[test]
fn sp_31_the_signed_transaction_pays_at_least_the_asked_rate() {
    let coins = [
        coin(1, 0, 300_000),
        coin(2, 1, 400_000),
        coin(3, 2, 500_000),
    ];
    for rate in [1000u64, 2500, 7777] {
        let r = run(&coins, vec![(external(1), 650_000)], rate);
        let signed = combine(&r.base, &[r.by1.clone(), r.by2.clone()]).unwrap();
        let tx = finalize_plan(wallet(), &signed, &r.plan).unwrap();
        let real_vsize = tx.weight().to_wu().div_ceil(4);
        let n = tx.input.len() as u64;
        let fee = r.plan.fee_sats();
        // Never below the asked rate.
        assert!(fee * 1000 >= rate * real_vsize, "rate {rate}");
        // And not above it by more than the slack: each signature is at
        // most 4 bytes shorter than the longest (2 per input), a quarter of
        // a virtual byte each, and the fee is rounded up by one satoshi.
        assert!(real_vsize <= r.plan.vsize());
        assert!(r.plan.vsize() - real_vsize <= 2 * n + 1);
        assert!(
            fee * 1000 <= rate * (real_vsize + 2 * n + 1) + 999,
            "rate {rate}"
        );
        assert_eq!(r.plan.fee_sats(), (r.plan.vsize() * rate).div_ceil(1000));
    }
}

#[test]
fn sp_32_a_zero_rate_and_rates_above_the_signers_bounds_are_refused() {
    let coins = [coin(1, 0, 1_000_000)];
    assert_eq!(
        plan(wallet(), &coins, &request(vec![(external(1), 50_000)], 0)),
        Err(PlanError::ZeroFeeRate)
    );
    // 1,000 sat/vB is the highest the signer's owner can set.
    assert!(plan(
        wallet(),
        &coins,
        &request(vec![(external(1), 50_000)], 1_000_000)
    )
    .is_ok());
    let r = plan(
        wallet(),
        &coins,
        &request(vec![(external(1), 50_000)], 1_000_001),
    );
    assert_eq!(
        r,
        Err(PlanError::FeeRateTooHigh {
            fee: 189_001,
            vsize: 189,
            limit: 1000
        })
    );
    assert!(r.unwrap_err().to_string().starts_with("SP-32"));
}

#[test]
fn sp_32_a_fee_above_two_million_satoshis_is_refused() {
    let coins: Vec<_> = (1..=30).map(|t| coin(t, 0, 1_000_000)).collect();
    let r = plan(
        wallet(),
        &coins,
        &request(vec![(external(1), 19_000_000)], 1_000_000),
    );
    assert!(
        matches!(
            r,
            Err(PlanError::FeeTooHigh {
                limit: 2_000_000,
                ..
            })
        ),
        "{r:?}"
    );
    // The same payment at a sane rate is fine.
    assert!(plan(
        wallet(),
        &coins,
        &request(vec![(external(1), 19_000_000)], 2000)
    )
    .is_ok());
}

#[test]
fn sp_32_the_dropped_change_counts_in_the_fee_that_is_checked() {
    // A coin that is all fee when no change is made: 50,000 paid, 3,000,000
    // left over and no room for change would need 100 outputs; instead the
    // rule is checked on the fee that is paid. 100 payments leave no change.
    let pays: Vec<_> = (0..100u8).map(|i| (external(i), 10_000)).collect();
    let r = plan(wallet(), &[coin(1, 0, 4_000_000)], &request(pays, 1000));
    assert!(matches!(r, Err(PlanError::FeeTooHigh { .. })), "{r:?}");
}

#[test]
fn sp_33_more_weight_than_standard_is_refused() {
    // 960 inputs weigh 960 * 418 + the rest, above 400,000.
    let coins: Vec<_> = (1..=1000).map(|t| coin(t, 0, 20_000)).collect();
    let mut r = request(vec![(external(1), 0)], 1000);
    r.send_all = true;
    let e = plan(wallet(), &coins, &r).unwrap_err();
    assert!(
        matches!(e, PlanError::TooHeavy { limit: 400_000, .. }),
        "{e:?}"
    );
    assert!(e.to_string().starts_with("SP-33"));
    // 900 inputs are under the limit and are taken.
    let p = plan(wallet(), &coins[..900], &r).unwrap();
    assert!(p.weight() <= 400_000);
    assert_eq!(p.inputs().len(), 900);
}

#[test]
fn sp_34_amounts_are_checked() {
    // Inputs equal outputs plus the fee.
    let coins = [coin(1, 0, 123_456), coin(2, 1, 654_321)];
    let p = plan(
        wallet(),
        &coins,
        &request(vec![(external(1), 700_000)], 3000),
    )
    .unwrap();
    let outs: u64 = p
        .unsigned_tx()
        .output
        .iter()
        .map(|o| o.value.to_sat())
        .sum();
    assert_eq!(p.total_in_sats(), 123_456 + 654_321);
    assert_eq!(p.total_out_sats(), outs);
    assert_eq!(p.total_in_sats(), p.total_out_sats() + p.fee_sats());
    // A coin above all bitcoin is refused.
    let huge = coin(3, 0, 21_000_000 * 100_000_000 + 1);
    assert_eq!(
        plan(
            wallet(),
            &[huge],
            &request(vec![(external(1), 50_000)], 1000)
        ),
        Err(PlanError::AmountTooLarge)
    );
    // Not enough.
    let r = plan(
        wallet(),
        &coins[..1],
        &request(vec![(external(1), 700_000)], 1000),
    );
    assert!(matches!(
        r,
        Err(PlanError::InsufficientFunds {
            available: 123_456,
            ..
        })
    ));
}

#[test]
fn sp_35_version_lock_time_and_sequence_are_fixed() {
    let coins = [coin(1, 0, 300_000), coin(2, 1, 400_000)];
    let p = plan(
        wallet(),
        &coins,
        &request(vec![(external(1), 500_000)], 1000),
    )
    .unwrap();
    let tx = p.unsigned_tx();
    assert_eq!(tx.version, Version::TWO);
    assert_eq!(tx.lock_time, LockTime::ZERO);
    for input in &tx.input {
        assert_eq!(input.sequence.0, 0xFFFF_FFFD);
        assert!(input.script_sig.is_empty() && input.witness.is_empty());
    }
    assert!(tx.is_explicitly_rbf());
}

#[test]
fn sp_36_the_plan_exposes_what_a_person_is_shown() {
    let coins = [coin(1, 0, 300_000), coin(2, 1, 400_000)];
    let mut r = request(vec![(external(1), 500_000), (external(2), 20_000)], 2000);
    r.change_index = 3;
    let p = plan(wallet(), &coins, &r).unwrap();
    assert_eq!(p.payments().len(), 2);
    assert_eq!(p.payments()[0].sats, 500_000);
    assert_eq!(p.payments()[1].script, external(2));
    assert_eq!(p.change().unwrap().index, 3);
    assert_eq!(p.inputs().len(), 2);
    assert_eq!(
        p.inputs()[0].amount_sats + p.inputs()[1].amount_sats,
        700_000
    );
    assert!(p.fee_sats() > 0 && p.vsize() > 0 && p.weight() > 0);
    assert_eq!(p.fee_rate_per_kvb(), p.fee_sats() * 1000 / p.vsize());
    assert!(p.fee_rate_per_kvb() >= 2000);
    assert_eq!(p.total_in_sats(), 700_000);
    assert_eq!(p.total_out_sats(), 700_000 - p.fee_sats());
    assert_eq!(p.txid(), p.unsigned_tx().compute_txid());
}

#[test]
fn sp_37_the_lock_time_is_the_height_asked_for() {
    let coins = [coin(1, 0, 300_000), coin(2, 1, 400_000)];
    let mut r = request(vec![(external(1), 500_000)], 1000);
    let none = plan(wallet(), &coins, &r).unwrap();
    assert_eq!(none.lock_height(), 0);
    assert_eq!(none.unsigned_tx().lock_time, LockTime::ZERO);
    for h in [1, 850_123, 499_999_999] {
        r.lock_height = h;
        let p = plan(wallet(), &coins, &r).unwrap();
        assert_eq!(p.lock_height(), h);
        assert_eq!(p.unsigned_tx().lock_time, LockTime::from_height(h).unwrap());
        // Every sequence is below 0xFFFFFFFF, so the lock time takes effect,
        // and the transaction still signals replacement.
        for input in &p.unsigned_tx().input {
            assert_eq!(input.sequence.0, 0xFFFF_FFFD);
            assert!(input.sequence.0 < 0xFFFF_FFFF);
        }
        assert!(p.unsigned_tx().is_explicitly_rbf());
        // Nothing else moves: the same coins, fee and outputs.
        assert_eq!(p.fee_sats(), none.fee_sats());
        assert_eq!(p.unsigned_tx().output, none.unsigned_tx().output);
        assert_eq!(p.inputs(), none.inputs());
    }
}

#[test]
fn sp_37_a_lock_time_that_is_a_time_is_refused() {
    let coins = [coin(1, 0, 300_000)];
    for h in [500_000_000, 500_000_001, 1_700_000_000, u32::MAX] {
        let mut r = request(vec![(external(1), 100_000)], 1000);
        r.lock_height = h;
        let e = plan(wallet(), &coins, &r).unwrap_err();
        assert_eq!(e, PlanError::LockTimeNotHeight { lock_height: h });
        assert!(e.to_string().starts_with("SP-37"));
    }
}
