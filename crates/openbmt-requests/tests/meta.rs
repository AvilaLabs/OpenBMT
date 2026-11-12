//! Checks that the tests and the errors line up with the rules file.

mod common;

use openbmt_core::currency::ShopCurrency;
use openbmt_requests::{
    AddressRefused, BookError, CreditRefused, EndRefused, Limit, MakeRefused, RepeatRefused,
    ReturnRefused, ShareRefused,
};

const RULES: &str = include_str!("../../../docs/REQUEST-RULES.md");
const TEST_SOURCES: [&str; 9] = [
    include_str!("dates.rs"),
    include_str!("requests.rs"),
    include_str!("matching.rs"),
    include_str!("repeats.rs"),
    include_str!("returns.rs"),
    include_str!("bookfile.rs"),
    include_str!("property.rs"),
    include_str!("../src/codec_tests.rs"),
    include_str!("../src/say_tests.rs"),
];

/// Every rule ID that the rules file defines (in bold at the start of a
/// bullet).
fn rule_ids() -> Vec<String> {
    let mut ids = Vec::new();
    for line in RULES.lines() {
        if let Some(rest) = line.trim_start().strip_prefix("- **RQ-") {
            let id: String = rest.chars().take(2).collect();
            if id.chars().all(|c| c.is_ascii_digit()) {
                ids.push(format!("RQ-{id}"));
            }
        }
    }
    ids
}

#[test]
fn the_rules_file_defines_the_rules_the_tests_name() {
    let ids = rule_ids();
    assert_eq!(ids.len(), 36);
    assert!(ids.contains(&"RQ-01".to_string()) && ids.contains(&"RQ-36".to_string()));
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
            let needle = format!("fn rq_{}_", &id[3..]);
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
            let Some(rest) = line.trim_start().strip_prefix("fn rq_") else {
                continue;
            };
            let id = format!("RQ-{}", rest.chars().take(2).collect::<String>());
            assert!(ids.contains(&id), "a test names {id}, which is not a rule");
        }
    }
}

#[test]
fn every_error_says_which_rule_in_words() {
    let texts: Vec<String> = vec![
        MakeRefused::NoteNeeded.to_string(),
        MakeRefused::BitcoinOnly.to_string(),
        MakeRefused::DueOutOfRange.to_string(),
        MakeRefused::TooManyOpen.to_string(),
        MakeRefused::TooSmall { min_sats: 1 }.to_string(),
        MakeRefused::TooLarge {
            max: Limit::Units(ShopCurrency::Usd, 1),
        }
        .to_string(),
        MakeRefused::StaleRate.to_string(),
        MakeRefused::IndexUsed.to_string(),
        MakeRefused::BookFull.to_string(),
        ShareRefused::NoSuchRequest.to_string(),
        ShareRefused::NotOpen.to_string(),
        ShareRefused::StaleRate.to_string(),
        CreditRefused::NoSuchRequest.to_string(),
        CreditRefused::NotInBlockOrder.to_string(),
        CreditRefused::NoRate.to_string(),
        CreditRefused::TooManyPayments.to_string(),
        EndRefused::NoSuchRequest.to_string(),
        EndRefused::AlreadyEnded.to_string(),
        EndRefused::HasPayments.to_string(),
        EndRefused::NotPartPaid.to_string(),
        EndRefused::NotAlmost.to_string(),
        EndRefused::NoRepeat.to_string(),
        RepeatRefused::NotDue.to_string(),
        RepeatRefused::TooManyOpen.to_string(),
        RepeatRefused::IndexUsed.to_string(),
        RepeatRefused::BookFull.to_string(),
        ReturnRefused::NoSuchRequest.to_string(),
        ReturnRefused::TooMuch { most_sats: 1 }.to_string(),
        ReturnRefused::TooSmall { min_sats: 1 }.to_string(),
        ReturnRefused::StaleRate.to_string(),
        ReturnRefused::BadScript.to_string(),
        ReturnRefused::QueueFull.to_string(),
        ReturnRefused::BookFull.to_string(),
        AddressRefused::NotAnAddress.to_string(),
        AddressRefused::TestNetwork.to_string(),
        AddressRefused::Own.to_string(),
        BookError::Io("x".into()).to_string(),
        BookError::Corrupt("x").to_string(),
    ];
    let ids = rule_ids();
    for t in &texts {
        assert!(!t.is_empty() && t.is_ascii(), "{t}");
        assert!(!t.contains('{') && !t.contains("Error"), "{t}");
        let rest = t.strip_prefix("RQ-").unwrap_or_else(|| panic!("{t}"));
        let id = format!("RQ-{}", &rest[..2]);
        assert!(ids.contains(&id), "{t} names {id}, not a rule");
    }
}

#[test]
fn the_magic_is_the_one_the_rules_name() {
    assert!(RULES.contains("magic `OBM=`"));
    assert_eq!(openbmt_requests::MAGIC_REQUEST_BOOK, *b"OBM=");
}
