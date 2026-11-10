//! Checks that the tests and the errors line up with the rules file.

mod common;

use bitcoin::OutPoint;
use common::*;
use openbmt_spend::{CoinError, CombineError, FinalizeError, PlanError, PsbtError, Source};

const RULES: &str = include_str!("../../../docs/SPEND-RULES.md");
const TEST_SOURCES: [&str; 8] = [
    include_str!("coins.rs"),
    include_str!("choice.rs"),
    include_str!("outputs.rs"),
    include_str!("fee.rs"),
    include_str!("psbt.rs"),
    include_str!("combine.rs"),
    include_str!("finalize.rs"),
    include_str!("hostile.rs"),
];

/// Every rule ID that the rules file defines (in bold at the start of a
/// bullet).
fn rule_ids() -> Vec<String> {
    let mut ids = Vec::new();
    for line in RULES.lines() {
        if let Some(rest) = line.trim_start().strip_prefix("- **SP-") {
            let id: String = rest.chars().take(2).collect();
            if id.chars().all(|c| c.is_ascii_digit()) {
                ids.push(format!("SP-{id}"));
            }
        }
    }
    ids
}

#[test]
fn the_rules_file_defines_the_rules_the_tests_name() {
    let ids = rule_ids();
    assert_eq!(ids.len(), 37);
    assert!(ids.contains(&"SP-01".to_string()) && ids.contains(&"SP-71".to_string()));
    let mut sorted = ids.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(sorted.len(), ids.len(), "a rule ID is used twice");
}

#[test]
fn every_rule_has_at_least_one_test_named_for_it() {
    let missing: Vec<_> = rule_ids()
        .into_iter()
        .filter(|id| {
            let needle = format!("fn sp_{}_", &id[3..]);
            !TEST_SOURCES.iter().any(|src| src.contains(&needle))
        })
        .collect();
    assert!(missing.is_empty(), "rules without a test: {missing:?}");
}

#[test]
fn every_test_is_named_for_a_rule_of_the_file() {
    let ids = rule_ids();
    for src in TEST_SOURCES {
        for line in src.lines() {
            let Some(rest) = line.trim_start().strip_prefix("fn sp_") else {
                continue;
            };
            let id = format!("SP-{}", rest.chars().take(2).collect::<String>());
            assert!(ids.contains(&id), "a test names {id}, which is not a rule");
        }
    }
}

#[test]
fn every_error_says_which_rule_in_words() {
    let outpoint = OutPoint::null();
    let (key, _) = signature_by(&run_base(), 0, 0);
    let texts: Vec<String> = vec![
        CoinError::PreviousTransactionMismatch { outpoint }.to_string(),
        CoinError::OutputMissing { outpoint }.to_string(),
        CoinError::ScriptNotInWallet { outpoint }.to_string(),
        PlanError::DuplicateCoin(outpoint).to_string(),
        PlanError::NoPayments.to_string(),
        PlanError::SendAllNeedsOnePayment.to_string(),
        PlanError::SendAllAmountGiven.to_string(),
        PlanError::MustSpendNotEligible {
            outpoint,
            confirmations: 0,
            needed: 1,
        }
        .to_string(),
        PlanError::ZeroFeeRate.to_string(),
        PlanError::PaymentScriptKind { payment: 1 }.to_string(),
        PlanError::PaymentBelowDust {
            payment: 1,
            sats: 5,
            dust: 294,
        }
        .to_string(),
        PlanError::PaymentToOwnScript { payment: 1 }.to_string(),
        PlanError::LockTimeNotHeight {
            lock_height: 500_000_000,
        }
        .to_string(),
        PlanError::TooManyOutputs.to_string(),
        PlanError::TooManyInputs.to_string(),
        PlanError::NoEligibleCoins.to_string(),
        PlanError::SendAllBelowDust { sats: 1, dust: 294 }.to_string(),
        PlanError::FeeTooHigh { fee: 3, limit: 2 }.to_string(),
        PlanError::TooHeavy {
            weight: 5,
            limit: 4,
        }
        .to_string(),
        PlanError::Overflow.to_string(),
        PlanError::AmountTooLarge.to_string(),
        CombineError::DifferentTransaction { copy: 0 }.to_string(),
        CombineError::DifferentInputData { copy: 0, input: 1 }.to_string(),
        CombineError::InputAlreadyFinal {
            source: Source::Base,
            input: 0,
        }
        .to_string(),
        CombineError::SighashType {
            source: Source::Copy(2),
            input: 0,
        }
        .to_string(),
        CombineError::ForeignKey {
            source: Source::Copy(0),
            input: 0,
            key,
        }
        .to_string(),
        CombineError::BadSignature {
            source: Source::Copy(0),
            input: 0,
            key,
        }
        .to_string(),
        FinalizeError::NoWalletKeyHint { input: 0 }.to_string(),
        FinalizeError::NotEnoughSignatures { input: 0, have: 1 }.to_string(),
        FinalizeError::TooHeavy { weight: 9 }.to_string(),
        FinalizeError::FinalCheckFailed("x").to_string(),
        FinalizeError::LockTimeNotHeight {
            lock_time: 500_000_000,
        }
        .to_string(),
        FinalizeError::NotThePlan.to_string(),
        FinalizeError::FeeNotThePlan {
            found: 1,
            planned: 2,
        }
        .to_string(),
        PsbtError::Derivation.to_string(),
    ];
    let ids = rule_ids();
    for t in &texts {
        assert!(!t.is_empty() && t.is_ascii(), "{t}");
        assert!(!t.contains("{") && !t.contains("Error"), "{t}");
        if let Some(rest) = t.strip_prefix("SP-") {
            let id = format!("SP-{}", &rest[..2]);
            assert!(ids.contains(&id), "{t} names {id}, not a rule");
        }
    }
    // Most of them name a rule at the start.
    assert!(texts.iter().filter(|t| t.starts_with("SP-")).count() >= 25);
}

fn run_base() -> bitcoin::psbt::Psbt {
    let p = openbmt_spend::plan(
        wallet(),
        &[coin(1, 0, 300_000)],
        &request(vec![(external(1), 100_000)], 1000),
    )
    .unwrap();
    p.to_psbt(wallet()).unwrap()
}
