//! Tests of the owner's words for the spend path (SC-01, SC-05, SC-07,
//! SC-12).

use super::*;
use openbmt_core::say::{check, check_distinct, Tone, Who};

#[path = "../../openbmt-owner/src/screens_file.rs"]
mod screens_file;

fn coin_name(e: &CoinError) -> &'static str {
    match e {
        CoinError::PreviousTransactionMismatch { .. } => "CoinError::PreviousTransactionMismatch",
        CoinError::OutputMissing { .. } => "CoinError::OutputMissing",
        CoinError::ScriptNotInWallet { .. } => "CoinError::ScriptNotInWallet",
        CoinError::WrongKind { .. } => "CoinError::WrongKind",
    }
}

fn plan_name(e: &PlanError) -> &'static str {
    match e {
        PlanError::Coin(c) => coin_name(c),
        PlanError::DuplicateCoin(_) => "PlanError::DuplicateCoin",
        PlanError::NoPayments => "PlanError::NoPayments",
        PlanError::SendAllNeedsOnePayment => "PlanError::SendAllNeedsOnePayment",
        PlanError::SendAllAmountGiven => "PlanError::SendAllAmountGiven",
        PlanError::ZeroFeeRate => "PlanError::ZeroFeeRate",
        PlanError::ChangeScript => "PlanError::ChangeScript",
        PlanError::PaymentScriptKind { .. } => "PlanError::PaymentScriptKind",
        PlanError::PaymentBelowDust { .. } => "PlanError::PaymentBelowDust",
        PlanError::AmountTooLarge => "PlanError::AmountTooLarge",
        PlanError::LockTimeNotHeight { .. } => "PlanError::LockTimeNotHeight",
        PlanError::PaymentToOwnScript { .. } => "PlanError::PaymentToOwnScript",
        PlanError::TooManyOutputs => "PlanError::TooManyOutputs",
        PlanError::TooManyInputs => "PlanError::TooManyInputs",
        PlanError::NoEligibleCoins => "PlanError::NoEligibleCoins",
        PlanError::InsufficientFunds { .. } => "PlanError::InsufficientFunds",
        PlanError::MustSpendNotEligible { .. } => "PlanError::MustSpendNotEligible",
        PlanError::SendAllBelowDust { .. } => "PlanError::SendAllBelowDust",
        PlanError::FeeTooHigh { .. } => "PlanError::FeeTooHigh",
        PlanError::FeeRateTooHigh { .. } => "PlanError::FeeRateTooHigh",
        PlanError::TooHeavy { .. } => "PlanError::TooHeavy",
        PlanError::Overflow => "PlanError::Overflow",
        PlanError::Internal(_) => "PlanError::Internal",
    }
}

fn psbt_name(e: &PsbtError) -> &'static str {
    match e {
        PsbtError::Coin(c) => coin_name(c),
        PsbtError::Derivation => "PsbtError::Derivation",
        PsbtError::Internal(_) => "PsbtError::Internal",
    }
}

fn combine_name(e: &CombineError) -> &'static str {
    match e {
        CombineError::NotVersionZero { .. } => "CombineError::NotVersionZero",
        CombineError::DifferentTransaction { .. } => "CombineError::DifferentTransaction",
        CombineError::DifferentInputData { .. } => "CombineError::DifferentInputData",
        CombineError::InputAlreadyFinal { .. } => "CombineError::InputAlreadyFinal",
        CombineError::MissingPreviousTransaction { .. } => {
            "CombineError::MissingPreviousTransaction"
        }
        CombineError::PreviousTransactionMismatch { .. } => {
            "CombineError::PreviousTransactionMismatch"
        }
        CombineError::PreviousOutputMissing { .. } => "CombineError::PreviousOutputMissing",
        CombineError::WitnessUtxoMismatch { .. } => "CombineError::WitnessUtxoMismatch",
        CombineError::WitnessScriptShape { .. } => "CombineError::WitnessScriptShape",
        CombineError::WitnessScriptNotSpent { .. } => "CombineError::WitnessScriptNotSpent",
        CombineError::SighashType { .. } => "CombineError::SighashType",
        CombineError::ForeignKey { .. } => "CombineError::ForeignKey",
        CombineError::BadSignature { .. } => "CombineError::BadSignature",
        // The same words as a plan's own check.
        CombineError::Internal(_) => "PlanError::Internal",
        // The same words as a coin of the other kind.
        CombineError::WrongKind { .. } => "CoinError::WrongKind",
        CombineError::KeyNotSpent { .. } => "CombineError::KeyNotSpent",
    }
}

fn finalize_name(e: &FinalizeError) -> &'static str {
    match e {
        FinalizeError::NotVersionZero => "CombineError::NotVersionZero",
        FinalizeError::MissingPreviousTransaction { .. } => {
            "CombineError::MissingPreviousTransaction"
        }
        FinalizeError::PreviousTransactionMismatch { .. } => {
            "CombineError::PreviousTransactionMismatch"
        }
        FinalizeError::PreviousOutputMissing { .. } => "CombineError::PreviousOutputMissing",
        FinalizeError::WitnessUtxoMismatch { .. } => "CombineError::WitnessUtxoMismatch",
        FinalizeError::NoWalletKeyHint { .. } => "FinalizeError::NoWalletKeyHint",
        FinalizeError::HintsInconsistent { .. } => "FinalizeError::HintsInconsistent",
        FinalizeError::ScriptNotInWallet { .. } => "FinalizeError::ScriptNotInWallet",
        FinalizeError::WitnessScriptMismatch { .. } => "FinalizeError::WitnessScriptMismatch",
        FinalizeError::InputAlreadyFinal { .. } => "CombineError::InputAlreadyFinal",
        FinalizeError::SighashType { .. } => "CombineError::SighashType",
        FinalizeError::ForeignKey { .. } => "CombineError::ForeignKey",
        FinalizeError::BadSignature { .. } => "CombineError::BadSignature",
        FinalizeError::NotEnoughSignatures { .. } => "FinalizeError::NotEnoughSignatures",
        FinalizeError::AmountOutOfRange => "FinalizeError::AmountOutOfRange",
        FinalizeError::OutputsExceedInputs => "FinalizeError::OutputsExceedInputs",
        FinalizeError::TooHeavy { .. } => "PlanError::TooHeavy",
        FinalizeError::FinalCheckFailed(_) => "FinalizeError::FinalCheckFailed",
        FinalizeError::LockTimeNotHeight { .. } => "PlanError::LockTimeNotHeight",
        FinalizeError::NotThePlan => "FinalizeError::NotThePlan",
        FinalizeError::FeeNotThePlan { .. } => "FinalizeError::FeeNotThePlan",
        FinalizeError::WrongKind { .. } => "CoinError::WrongKind",
        FinalizeError::NoSignature { .. } => "FinalizeError::NoSignature",
    }
}

#[test]
fn sc_12_every_word_is_checked_on_its_own() {
    let bad: Vec<String> = all_words()
        .iter()
        .filter_map(|(name, w)| check(w).err().map(|e| format!("{name}: {e}")))
        .collect();
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

#[test]
fn sc_12_every_word_keeps_the_rules() {
    assert_eq!(check_distinct(&all_words()), Ok(()));
}

#[test]
fn sc_01_no_variant_is_missing() {
    let names: Vec<String> = all_words().into_iter().map(|(n, _)| n).collect();
    let key = sample_key().expect("a fixed key parses");
    let outpoint = bitcoin::OutPoint::null();
    let mut wanted: Vec<&str> = vec![
        coin_name(&CoinError::PreviousTransactionMismatch { outpoint }),
        coin_name(&CoinError::OutputMissing { outpoint }),
        coin_name(&CoinError::ScriptNotInWallet { outpoint }),
        coin_name(&CoinError::WrongKind {
            outpoint,
            expected: crate::WalletKind::Single,
        }),
    ];
    let plans = [
        PlanError::Coin(CoinError::OutputMissing { outpoint }),
        PlanError::DuplicateCoin(outpoint),
        PlanError::NoPayments,
        PlanError::SendAllNeedsOnePayment,
        PlanError::SendAllAmountGiven,
        PlanError::ZeroFeeRate,
        PlanError::ChangeScript,
        PlanError::PaymentScriptKind { payment: 0 },
        PlanError::PaymentBelowDust {
            payment: 0,
            sats: 1,
            dust: 2,
        },
        PlanError::AmountTooLarge,
        PlanError::LockTimeNotHeight { lock_height: 1 },
        PlanError::PaymentToOwnScript { payment: 0 },
        PlanError::TooManyOutputs,
        PlanError::TooManyInputs,
        PlanError::NoEligibleCoins,
        PlanError::InsufficientFunds {
            available: 1,
            needed: 2,
        },
        PlanError::MustSpendNotEligible {
            outpoint,
            confirmations: 0,
            needed: 1,
        },
        PlanError::SendAllBelowDust { sats: 1, dust: 2 },
        PlanError::FeeTooHigh { fee: 2, limit: 1 },
        PlanError::FeeRateTooHigh {
            fee: 2,
            vsize: 1,
            limit: 1,
        },
        PlanError::TooHeavy {
            weight: 2,
            limit: 1,
        },
        PlanError::Overflow,
        PlanError::Internal("x"),
    ];
    wanted.extend(plans.iter().map(plan_name));
    let psbts = [
        PsbtError::Coin(CoinError::OutputMissing { outpoint }),
        PsbtError::Derivation,
        PsbtError::Internal("x"),
    ];
    wanted.extend(psbts.iter().map(psbt_name));
    let combines = [
        CombineError::NotVersionZero {
            source: Source::Base,
        },
        CombineError::DifferentTransaction { copy: 0 },
        CombineError::DifferentInputData { copy: 0, input: 0 },
        CombineError::InputAlreadyFinal {
            source: Source::Base,
            input: 0,
        },
        CombineError::MissingPreviousTransaction { input: 0 },
        CombineError::PreviousTransactionMismatch { input: 0 },
        CombineError::PreviousOutputMissing { input: 0 },
        CombineError::WitnessUtxoMismatch { input: 0 },
        CombineError::WitnessScriptShape { input: 0 },
        CombineError::WitnessScriptNotSpent { input: 0 },
        CombineError::SighashType {
            source: Source::Base,
            input: 0,
        },
        CombineError::ForeignKey {
            source: Source::Base,
            input: 0,
            key,
        },
        CombineError::BadSignature {
            source: Source::Base,
            input: 0,
            key,
        },
        CombineError::Internal("x"),
        CombineError::WrongKind {
            input: 0,
            expected: crate::WalletKind::Single,
        },
        CombineError::KeyNotSpent { input: 0 },
    ];
    wanted.extend(combines.iter().map(combine_name));
    let finals = [
        FinalizeError::NotVersionZero,
        FinalizeError::MissingPreviousTransaction { input: 0 },
        FinalizeError::PreviousTransactionMismatch { input: 0 },
        FinalizeError::PreviousOutputMissing { input: 0 },
        FinalizeError::WitnessUtxoMismatch { input: 0 },
        FinalizeError::NoWalletKeyHint { input: 0 },
        FinalizeError::HintsInconsistent { input: 0 },
        FinalizeError::ScriptNotInWallet { input: 0 },
        FinalizeError::WitnessScriptMismatch { input: 0 },
        FinalizeError::InputAlreadyFinal { input: 0 },
        FinalizeError::SighashType { input: 0 },
        FinalizeError::ForeignKey { input: 0, key },
        FinalizeError::BadSignature { input: 0, key },
        FinalizeError::NotEnoughSignatures { input: 0, have: 1 },
        FinalizeError::AmountOutOfRange,
        FinalizeError::OutputsExceedInputs,
        FinalizeError::TooHeavy { weight: 1 },
        FinalizeError::FinalCheckFailed("x"),
        FinalizeError::LockTimeNotHeight { lock_time: 1 },
        FinalizeError::NotThePlan,
        FinalizeError::FeeNotThePlan {
            found: 1,
            planned: 2,
        },
        FinalizeError::WrongKind {
            input: 0,
            expected: crate::WalletKind::Single,
        },
        FinalizeError::NoSignature { input: 0 },
    ];
    wanted.extend(finals.iter().map(finalize_name));
    for name in &wanted {
        assert!(names.iter().any(|n| n == name), "{name} has no words");
    }
    // Every entry of the list is one that some variant maps to.
    for name in &names {
        assert!(wanted.contains(&name.as_str()), "{name} is not a variant");
    }
}

#[test]
fn sc_05_someone_is_named() {
    for (name, w) in all_words() {
        assert!(
            !(w.who == Who::Nobody && w.tone == Tone::Stop),
            "{name} stops and names nobody"
        );
    }
}

#[test]
fn sc_07_same_fault_two_ways_reads_the_same() {
    let a = PlanError::TooHeavy {
        weight: 450_000,
        limit: 400_000,
    }
    .words();
    let b = FinalizeError::TooHeavy { weight: 450_000 }.words();
    assert_eq!((a.heading, a.what), (b.heading, b.what));
    let outpoint = bitcoin::OutPoint::null();
    let a = PlanError::Coin(CoinError::OutputMissing { outpoint }).words();
    let b = PsbtError::Coin(CoinError::OutputMissing { outpoint }).words();
    assert_eq!(a, b);
}

#[test]
fn sc_03_the_details_hold_the_notation() {
    let outpoint = bitcoin::OutPoint::null();
    let w = CoinError::OutputMissing { outpoint }.words();
    assert!(w.detail.is_some_and(|d| d.contains(':')));
    assert!(!w.what.contains(':'));
}

#[test]
fn sc_12_the_list_on_disk_is_this_one() {
    screens_file::check_or_write("spend", &screens_markdown());
}
