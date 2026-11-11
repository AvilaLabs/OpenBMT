//! Checks that the tests and the errors line up with the rules file.

use openbmt_approver::{DecideError, FinishError, RefuseSession, RestoreError, SetupError};
use openbmt_core::identity::IdentityError;
use openbmt_core::wire::DecodeError;

const RULES: &str = include_str!("../../../docs/SESSION-RULES.md");
const TEST_SOURCES: [&str; 5] = [
    include_str!("box_pairing.rs"),
    include_str!("open.rs"),
    include_str!("lines.rs"),
    include_str!("decide.rs"),
    include_str!("finish.rs"),
];

/// Every rule ID of the rules file, as a number.
fn rule_ids() -> Vec<u32> {
    let mut ids = Vec::new();
    for line in RULES.lines() {
        if let Some(rest) = line.trim_start().strip_prefix("- **AS-") {
            if let Ok(n) = rest.chars().take(2).collect::<String>().parse::<u32>() {
                ids.push(n);
            }
        }
    }
    ids
}

/// The rules of this crate: section 2, AS-20 to AS-38, and the Key app's part
/// of adding a Box, AS-80 to AS-82.
fn own_rules() -> Vec<u32> {
    rule_ids()
        .into_iter()
        .filter(|n| (20..50).contains(n) || (80..=82).contains(n))
        .collect()
}

#[test]
fn the_rules_file_defines_the_rules_the_tests_name() {
    assert_eq!(own_rules().len(), 22);
    assert!(own_rules().contains(&20) && own_rules().contains(&82));
}

#[test]
fn every_rule_has_at_least_one_test_named_for_it() {
    let missing: Vec<_> = own_rules()
        .into_iter()
        .filter(|n| {
            let needle = format!("fn as_{n:02}_");
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
            let Some(rest) = line.trim_start().strip_prefix("fn as_") else {
                continue;
            };
            let n: u32 = rest.chars().take(2).collect::<String>().parse().unwrap();
            assert!(ids.contains(&n), "a test names AS-{n:02}, not a rule");
        }
    }
}

#[test]
fn every_error_says_which_rule_in_words() {
    let ids = rule_ids();
    let texts: Vec<String> = vec![
        RefuseSession::TooLarge.to_string(),
        RefuseSession::Frame(DecodeError::Truncated).to_string(),
        RefuseSession::WrongOwner.to_string(),
        RefuseSession::BadSignature.to_string(),
        RefuseSession::WrongWallet.to_string(),
        RefuseSession::SeenBefore.to_string(),
        DecideError::Finished.to_string(),
        DecideError::UnknownItem(1).to_string(),
        DecideError::CannotApprove("x".into()).to_string(),
        DecideError::BadReason.to_string(),
        FinishError::<String>::Finished.to_string(),
        FinishError::Store("x".to_string()).to_string(),
        SetupError::Identity(IdentityError::InvalidSecret).to_string(),
        SetupError::SameRoot.to_string(),
        SetupError::SignerWallet.to_string(),
        SetupError::SignerKey.to_string(),
        SetupError::SignerAttached.to_string(),
        RestoreError::Decode(DecodeError::Truncated).to_string(),
        RestoreError::WrongWallet.to_string(),
        RestoreError::WrongOwner.to_string(),
        RestoreError::SignerMismatch.to_string(),
        RestoreError::BadSettings.to_string(),
    ];
    for t in &texts {
        assert!(t.is_ascii() && !t.contains('{'), "{t}");
        let rest = t.strip_prefix("AS-").unwrap_or_else(|| panic!("{t}"));
        let n: u32 = rest[..2].parse().unwrap();
        assert!(ids.contains(&n), "{t} names a rule that is not in the file");
    }
}
