//! Checks that the tests and the errors line up with the rules file.

use openbmt_session::{BindError, EncodeError, VerifyError};

const RULES: &str = include_str!("../../../docs/SESSION-RULES.md");
const TEST_SOURCES: [&str; 1] = [include_str!("file.rs")];

/// Every rule ID of the rules file, as a number (in bold at the start of a
/// bullet).
fn rule_ids() -> Vec<u32> {
    let mut ids = Vec::new();
    for line in RULES.lines() {
        if let Some(rest) = line.trim_start().strip_prefix("- **AS-") {
            let id: String = rest.chars().take_while(char::is_ascii_digit).collect();
            if let Ok(n) = id.parse::<u32>() {
                ids.push(n);
            }
        }
    }
    ids
}

/// The rules of this crate: section 1, AS-01 to AS-15, and the file's part
/// of adding a Box, AS-78 and AS-79.
fn own_rules() -> Vec<u32> {
    rule_ids()
        .into_iter()
        .filter(|n| *n < 20 || (78..=79).contains(n))
        .collect()
}

#[test]
fn the_rules_file_defines_the_rules_the_tests_name() {
    let ids = rule_ids();
    let mut sorted = ids.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(sorted.len(), ids.len(), "a rule ID is used twice");
    assert_eq!(own_rules().len(), 17);
    assert!(own_rules().contains(&1) && own_rules().contains(&79));
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
        EncodeError::TooLarge.to_string(),
        EncodeError::ItemCount.to_string(),
        EncodeError::ItemOrder.to_string(),
        EncodeError::BadText("x").to_string(),
        EncodeError::BadItem("x").to_string(),
        EncodeError::BadPayload("x").to_string(),
        VerifyError::WrongOwner.to_string(),
        VerifyError::BadSignature.to_string(),
        BindError::WrongApprover.to_string(),
        BindError::BadSignature.to_string(),
        BindError::WrongSession.to_string(),
        BindError::WrongRequest.to_string(),
        BindError::UnknownItem { item: 1 }.to_string(),
        BindError::MissingItem { item: 1 }.to_string(),
        BindError::KindMismatch { item: 1 }.to_string(),
        BindError::BadHealthProof { item: 1 }.to_string(),
    ];
    for t in &texts {
        assert!(t.is_ascii() && !t.contains('{'), "{t}");
        let rest = t.strip_prefix("AS-").unwrap_or_else(|| panic!("{t}"));
        let n: u32 = rest[..2].parse().unwrap();
        assert!(ids.contains(&n), "{t} names a rule that is not in the file");
    }
}
