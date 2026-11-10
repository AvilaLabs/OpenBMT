//! Checks that the tests and refusals line up with the rules file.

use openbmt_core::wire::DecodeError;

use crate::{Problem, Refusal};

const RULES: &str = include_str!("../../../../docs/WATCHER-RULES.md");
const TEST_SOURCES: [&str; 6] = [
    include_str!("block.rs"),
    include_str!("headers.rs"),
    include_str!("health.rs"),
    include_str!("netgroup.rs"),
    include_str!("proof.rs"),
    include_str!("sale.rs"),
];

/// Every rule ID that the rules file defines (in bold at the start of a
/// bullet).
fn rule_ids() -> Vec<String> {
    let mut ids = Vec::new();
    for line in RULES.lines() {
        if let Some(rest) = line.trim_start().strip_prefix("- **WR-") {
            let id: String = rest.chars().take(2).collect();
            if id.chars().all(|c| c.is_ascii_digit()) {
                ids.push(format!("WR-{id}"));
            }
        }
    }
    ids
}

#[test]
fn the_rules_file_defines_the_rules_the_tests_name() {
    let ids = rule_ids();
    assert_eq!(ids.len(), 35);
    assert!(ids.contains(&"WR-01".to_string()) && ids.contains(&"WR-53".to_string()));
}

#[test]
fn every_rule_has_at_least_one_test_named_for_it() {
    let missing: Vec<_> = rule_ids()
        .into_iter()
        .filter(|id| {
            let needle = format!("fn wr_{}_", &id[3..]);
            !TEST_SOURCES.iter().any(|src| src.contains(&needle))
        })
        .collect();
    assert!(missing.is_empty(), "rules without a test: {missing:?}");
}

#[test]
fn every_refusal_names_a_rule_of_the_file_and_says_so_in_words() {
    let ids = rule_ids();
    let refusals = vec![
        Refusal::NotOnChain,
        Refusal::HeaderProofOfWork,
        Refusal::NetworkNotSupported,
        Refusal::UnknownParent,
        Refusal::WrongBits {
            expected: 1,
            found: 2,
        },
        Refusal::ChainProofOfWork,
        Refusal::TimeNotAfterMedian { median: 1, time: 1 },
        Refusal::AncestorMissing,
        Refusal::HeightOverflow,
        Refusal::TimeTooFarAhead { time: 1 },
        Refusal::SideHeaderTooDeep,
        Refusal::SideHeadersFull,
        Refusal::WorkOverflow,
        Refusal::StoreMalformed(DecodeError::Truncated),
        Refusal::StoreWrongNetwork,
        Refusal::StoreWrongStart,
        Refusal::StoreNotChained,
        Refusal::StoreHeaderRefused {
            index: 1,
            why: Box::new(Refusal::UnknownParent),
        },
        Refusal::ProofTooLarge { len: 1 },
        Refusal::ProofMalformed,
        Refusal::CoinbaseTooLarge { len: 1 },
        Refusal::PaymentTooLarge { len: 1 },
        Refusal::TreeTransactionCount { count: 1 },
        Refusal::TreeTooManyHashes,
        Refusal::TreeBitsAndHashes,
        Refusal::TreeEqualChildren,
        Refusal::TreePadding,
        Refusal::TreeRootMismatch,
        Refusal::ProofCoversWrongCount { found: 1 },
        Refusal::CoinbaseNotFirst { position: 1 },
        Refusal::CoinbaseMalformed,
        Refusal::CoinbaseIdMismatch,
        Refusal::NotACoinbase,
        Refusal::PaymentMalformed,
        Refusal::PaymentIdMismatch,
        Refusal::PaymentIsCoinbase,
        Refusal::CoinbaseSixtyFourBytes,
        Refusal::PaymentSixtyFourBytes,
        Refusal::BlockTooLarge { len: 1 },
        Refusal::BlockMalformed,
        Refusal::BlockMerkleRoot,
        Refusal::BlockNoCoinbase,
        Refusal::BlockExtraCoinbase { position: 1 },
        Refusal::BlockDuplicateTransaction { position: 1 },
        Refusal::BlockPaymentSixtyFourBytes { position: 1 },
        Refusal::PaysNothing,
        Refusal::TooManyTransactions,
        Refusal::TransactionTooLarge { len: 1 },
        Refusal::NotATransaction,
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
}

#[test]
fn every_problem_names_a_rule_of_the_file() {
    let ids = rule_ids();
    for p in [
        Problem::TooFewPeerGroups,
        Problem::TooFewFilterPeers,
        Problem::TipIsOld,
        Problem::HeadersBehind,
        Problem::ClockMismatch,
    ] {
        assert!(ids.contains(&p.rule().to_string()), "{p:?}");
    }
}
