mod common;

use common::{coin_tagged, wallet};
use openbmt_core::wallet::Keychain;
use openbmt_spend::{assess_maintenance, MaintenanceDeferReason, MAX_CONSOLIDATION_INPUTS};

#[test]
fn sp_88_advisory_distinguishes_balances_future_cost_and_confirmation_eligibility() {
    let coins = vec![
        coin_tagged(1, Keychain::Receive, 0, 100_000, 12),
        coin_tagged(2, Keychain::Receive, 1, 100_000, 0),
    ];
    let assessment = assess_maintenance(wallet(), &coins, 1, 1_000, 1_000).unwrap();
    assert_eq!(assessment.supplied_balance_sats, 200_000);
    assert_eq!(assessment.eligible_balance_sats, 100_000);
    assert_eq!(assessment.ineligible_balance_sats, 100_000);
    assert_eq!(assessment.batches.len(), 1);
    assert_eq!(assessment.batches[0].gross_sats, 100_000);
    assert_eq!(assessment.batches[0].estimated_fee_sats, 158);
    assert_eq!(assessment.batches[0].estimated_output_sats, Some(99_842));
    assert_eq!(assessment.batches[0].deferred, None);
    assert_eq!(assessment.estimated_future_cost_sats, 158);
}

#[test]
fn sp_88_maintenance_batches_are_bounded_and_budget_deferrals_are_explicit() {
    let coins: Vec<_> = (0..=MAX_CONSOLIDATION_INPUTS)
        .map(|i| coin_tagged(i as u32 + 1, Keychain::Receive, i as u32, 100_000, 10))
        .collect();
    let assessment = assess_maintenance(wallet(), &coins, 1, 1_000, 1).unwrap();
    assert_eq!(assessment.batches.len(), 2);
    assert_eq!(
        assessment.batches[0].outpoints.len(),
        MAX_CONSOLIDATION_INPUTS
    );
    assert_eq!(assessment.batches[1].outpoints.len(), 1);
    assert_eq!(
        assessment.batches[0].deferred,
        Some(MaintenanceDeferReason::FeeBudget)
    );
    assert_eq!(
        assessment.batches[1].deferred,
        Some(MaintenanceDeferReason::FeeBudget)
    );
    assert_eq!(assessment.batches[0].gross_sats, 40_000_000);
}

#[test]
fn sp_88_unaffordable_value_defers_as_dust_and_duplicate_coins_refuse_whole_assessment() {
    let tiny = coin_tagged(1, Keychain::Receive, 0, 200, 2);
    let assessment =
        assess_maintenance(wallet(), std::slice::from_ref(&tiny), 1, 1_000, u64::MAX).unwrap();
    assert_eq!(
        assessment.batches[0].deferred,
        Some(MaintenanceDeferReason::Dust)
    );
    assert_eq!(assessment.batches[0].estimated_output_sats, Some(42));
    assert!(assess_maintenance(wallet(), &[tiny.clone(), tiny], 1, 1_000, u64::MAX).is_err());
}

#[test]
fn sp_88_fee_consuming_value_is_distinct_and_zero_confirmation_threshold_is_refused() {
    let exact_fee = openbmt_spend::consolidation_cost_sats(1, 1_000).unwrap();
    let coin = coin_tagged(1, Keychain::Receive, 0, exact_fee, 2);
    let assessment =
        assess_maintenance(wallet(), std::slice::from_ref(&coin), 1, 1_000, u64::MAX).unwrap();
    assert_eq!(assessment.batches[0].estimated_output_sats, Some(0));
    assert_eq!(
        assessment.batches[0].deferred,
        Some(MaintenanceDeferReason::FeeConsumesValue)
    );
    assert!(assess_maintenance(wallet(), &[coin], 0, 1_000, u64::MAX).is_err());
}

#[test]
fn sp_88_large_batch_over_budget_does_not_consume_budget_needed_by_a_later_small_batch() {
    let coins: Vec<_> = (0..=MAX_CONSOLIDATION_INPUTS)
        .map(|i| coin_tagged(i as u32 + 1, Keychain::Receive, i as u32, 100_000, 10))
        .collect();
    let one_input_fee = openbmt_spend::consolidation_cost_sats(1, 1_000).unwrap();
    let assessment = assess_maintenance(wallet(), &coins, 1, 1_000, one_input_fee).unwrap();
    assert_eq!(
        assessment.batches[0].deferred,
        Some(MaintenanceDeferReason::FeeBudget)
    );
    assert_eq!(assessment.batches[1].deferred, None);
}

#[test]
fn sp_88_bad_ineligible_coin_and_fee_overflow_refuse_the_whole_assessment() {
    let mut bad = coin_tagged(1, Keychain::Receive, 0, 100_000, 0);
    bad.prev_tx.output[1].value = bitcoin::Amount::from_sat(90_000);
    assert!(assess_maintenance(wallet(), &[bad], 1, 1_000, u64::MAX).is_err());

    let valid = coin_tagged(2, Keychain::Receive, 0, 100_000, 1);
    assert!(assess_maintenance(wallet(), &[valid], 1, u64::MAX, u64::MAX).is_err());
}
