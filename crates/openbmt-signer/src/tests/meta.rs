//! Checks that the tests and refusals line up with the rules file.

use crate::{Ceiling, ChangeRefusal, Refusal, RegisterError};
use openbmt_core::wallet::WalletError;

const RULES: &str = include_str!("../../../../docs/SIGNER-RULES.md");
const TEST_SOURCES: [&str; 10] = [
    include_str!("batch.rs"),
    include_str!("ceilings.rs"),
    include_str!("inputs.rs"),
    include_str!("outputs.rs"),
    include_str!("owner.rs"),
    include_str!("parsing.rs"),
    include_str!("signing.rs"),
    include_str!("snapshot.rs"),
    include_str!("time.rs"),
    include_str!("kit.rs"),
];

/// Every rule ID that the rules file defines (in bold at the start of a
/// bullet).
fn rule_ids() -> Vec<String> {
    let mut ids = Vec::new();
    for line in RULES.lines() {
        if let Some(rest) = line.trim_start().strip_prefix("- **SR-") {
            let id: String = rest.chars().take(2).collect();
            if id.chars().all(|c| c.is_ascii_digit()) {
                ids.push(format!("SR-{id}"));
            }
        }
    }
    ids
}

#[test]
fn the_rules_file_defines_the_rules_the_tests_name() {
    let ids = rule_ids();
    assert_eq!(ids.len(), 47);
    assert!(ids.contains(&"SR-01".to_string()) && ids.contains(&"SR-70".to_string()));
}

#[test]
fn every_rule_has_at_least_one_test_named_for_it() {
    let missing: Vec<_> = rule_ids()
        .into_iter()
        .filter(|id| {
            let needle = format!("fn sr_{}_", &id[3..]);
            !TEST_SOURCES.iter().any(|src| src.contains(&needle))
        })
        .collect();
    assert!(missing.is_empty(), "rules without a test: {missing:?}");
}

#[test]
fn every_refusal_names_a_rule_of_the_file_and_says_so_in_words() {
    let ids = rule_ids();
    let refusals = vec![
        Refusal::NotAPsbt,
        Refusal::FileTooLarge { len: 1 },
        Refusal::InputCount { found: 0 },
        Refusal::OutputCount { found: 0 },
        Refusal::TransactionVersion { found: 1 },
        Refusal::LockTimeNotHeight { lock_time: 5 },
        Refusal::RelativeTimeLock {
            input: 0,
            sequence: 1,
        },
        Refusal::DuplicateInput { input: 1 },
        Refusal::InputAlreadyFinal { input: 0 },
        Refusal::ForbiddenInputField {
            input: 0,
            field: "x",
        },
        Refusal::ForbiddenOutputField {
            output: 0,
            field: "x",
        },
        Refusal::SighashType { input: 0, found: 2 },
        Refusal::ForeignXpub,
        Refusal::MissingPreviousTransaction { input: 0 },
        Refusal::PreviousTransactionMismatch { input: 0 },
        Refusal::PreviousOutputMissing { input: 0, index: 1 },
        Refusal::WitnessUtxoMismatch { input: 0 },
        Refusal::NoWalletKeyHint { input: 0 },
        Refusal::HintKeyMismatch { input: 0 },
        Refusal::HintsDisagree { input: 0 },
        Refusal::ScriptNotInWallet { input: 0 },
        Refusal::WitnessScriptMismatch { input: 0 },
        Refusal::ForeignSignature { input: 0 },
        Refusal::SignatureSighashType { input: 0 },
        Refusal::BadSignature { input: 0 },
        Refusal::AlreadySigned,
        Refusal::InputAmountTooLarge {
            input: 0,
            amount: 1,
        },
        Refusal::InputTotalOverflow,
        Refusal::OutputHintKeyMismatch { output: 0 },
        Refusal::OutputHintsDisagree { output: 0 },
        Refusal::OutputScriptKind { output: 0 },
        Refusal::OutputAmountZero { output: 0 },
        Refusal::OutputAmountTooLarge {
            output: 0,
            amount: 1,
        },
        Refusal::OutputTotalOverflow,
        Refusal::OutputsExceedInputs {
            outputs: 2,
            inputs: 1,
        },
        Refusal::ChangeIndexTooFar {
            output: 0,
            index: 30,
            limit: 20,
        },
        Refusal::OverCeiling {
            ceiling: Ceiling::OtherPerTx,
            amount: 2,
            limit: 1,
        },
        Refusal::OverCeiling {
            ceiling: Ceiling::OtherDay,
            amount: 2,
            limit: 1,
        },
        Refusal::OverCeiling {
            ceiling: Ceiling::OtherWeek,
            amount: 2,
            limit: 1,
        },
        Refusal::OverCeiling {
            ceiling: Ceiling::AllowlistedPerTx,
            amount: 2,
            limit: 1,
        },
        Refusal::OverCeiling {
            ceiling: Ceiling::AllowlistedDay,
            amount: 2,
            limit: 1,
        },
        Refusal::OverCeiling {
            ceiling: Ceiling::AllowlistedWeek,
            amount: 2,
            limit: 1,
        },
        Refusal::TotalsOverflow,
        Refusal::SpendLogFull,
        Refusal::FeeRateTooHigh {
            fee: 9,
            vsize: 1,
            limit: 1,
        },
        Refusal::FeeTooHigh { fee: 9, limit: 1 },
        Refusal::WeightTooHigh {
            weight: 9,
            limit: 1,
        },
        Refusal::ReviewOutdated { output: 0 },
        Refusal::ReviewForOtherWallet,
        Refusal::WrongKey,
        Refusal::Internal("x"),
    ];
    for r in refusals {
        let rule = r.rule();
        assert!(
            ids.contains(&rule.to_string()),
            "{rule} is not in the rules file: {r:?}"
        );
        let text = r.to_string();
        assert!(text.starts_with(&format!("{rule}: ")), "{text}");
        assert!(text.len() > rule.len() + 4);
    }
    let changes = [
        ChangeRefusal::ScriptKind,
        ChangeRefusal::InvalidLabel,
        ChangeRefusal::AllowlistFull,
        ChangeRefusal::AlreadyEnrolled,
        ChangeRefusal::NotEnrolled,
        ChangeRefusal::CeilingAboveMoney,
        ChangeRefusal::FeeRateOutOfBounds,
        ChangeRefusal::FeeOutOfBounds,
        ChangeRefusal::MarkNotHigher,
        ChangeRefusal::MarkTooHigh,
        ChangeRefusal::ReceiveMarkNotHigher,
        ChangeRefusal::ReceiveMarkTooHigh,
    ];
    for r in changes {
        assert!(ids.contains(&r.rule().to_string()), "{r:?}");
        assert!(r.to_string().starts_with(r.rule()));
    }
    let registers = [
        RegisterError::Wallet(WalletError::NonCanonical),
        RegisterError::UnsupportedNetwork,
        RegisterError::OwnKeyNotInWallet,
        RegisterError::Key3NotInWallet,
    ];
    for r in registers {
        assert!(ids.contains(&r.rule().to_string()), "{r:?}");
        assert!(r.to_string().starts_with(r.rule()));
    }
}
