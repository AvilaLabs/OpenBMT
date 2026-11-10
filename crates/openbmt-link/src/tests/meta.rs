//! Ties the rules to the tests: every rule in `docs/LINK-RULES.md` has a
//! test named for it, or an entry in `NOT_HERE` that says why not. The
//! rules of section 6 (LK-50 and up) are the courier's and the nodes', and
//! `crates/openbmt-courier` ties those.

use std::collections::BTreeSet;

const RULES: &str = include_str!("../../../../docs/LINK-RULES.md");
const ERRORS: [&str; 5] = [
    include_str!("../seal.rs"),
    include_str!("../proto.rs"),
    include_str!("../numbers.rs"),
    include_str!("../client.rs"),
    include_str!("../relay.rs"),
];

const TEST_SOURCES: [&str; 8] = [
    include_str!("seal.rs"),
    include_str!("numbers.rs"),
    include_str!("relay.rs"),
    include_str!("store.rs"),
    include_str!("net.rs"),
    include_str!("e2e.rs"),
    include_str!("proto.rs"),
    include_str!("kit.rs"),
];

/// Rules this crate does not test, each with the reason. Empty: the crate
/// implements every rule.
const NOT_HERE: [(&str, &str); 0] = [];

fn defined() -> BTreeSet<String> {
    RULES
        .lines()
        .filter_map(|l| l.strip_prefix("- **LK-"))
        .map(|rest| format!("LK-{}", &rest[..2]))
        .filter(|id| id.as_str() < "LK-50")
        .collect()
}

#[test]
fn the_rules_file_defines_twenty_three_rules() {
    let ids = defined();
    assert_eq!(ids.len(), 23);
    assert!(ids.contains("LK-01") && ids.contains("LK-41"));
}

#[test]
fn every_rule_has_a_test_or_a_reason() {
    let sources: String = TEST_SOURCES.concat();
    for id in defined() {
        let prefix = format!("fn lk_{}_", &id[3..]);
        let tested = sources.contains(&prefix);
        let excused = NOT_HERE.iter().any(|(r, _)| *r == id);
        assert!(
            tested || excused,
            "{id} has no test and no entry in NOT_HERE"
        );
        assert!(!(tested && excused), "{id} is tested and also in NOT_HERE");
    }
}

#[test]
fn not_here_names_only_defined_rules_and_gives_a_reason() {
    let ids = defined();
    for (rule, reason) in NOT_HERE {
        assert!(ids.contains(rule), "{rule} is not a rule");
        assert!(!reason.trim().is_empty(), "{rule} has no reason");
    }
}

#[test]
fn every_test_names_a_rule_that_exists() {
    let ids = defined();
    let sources: String = TEST_SOURCES.concat();
    let mut seen = 0;
    for line in sources.lines() {
        if let Some(rest) = line.trim().strip_prefix("fn lk_") {
            let id = format!("LK-{}", &rest[..2]);
            assert!(ids.contains(&id), "test {line} names {id}");
            seen += 1;
        }
    }
    assert!(seen > 40, "only {seen} rule tests");
}

#[test]
fn every_error_names_a_rule_that_exists() {
    let ids = defined();
    let mut seen = 0;
    for source in ERRORS {
        for (at, _) in source.match_indices("\"LK-") {
            let id = &source[at + 1..at + 6];
            assert!(ids.contains(id), "an error names {id}");
            seen += 1;
        }
    }
    assert!(seen > 20, "only {seen} rule names in the errors");
}
