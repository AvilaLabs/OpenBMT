//! Ties the rules of section 6 of `docs/LINK-RULES.md` (LK-50 and up) to the
//! tests: every one has a test named for it, in this crate or in the link
//! tests of the two nodes, or an entry in `NOT_HERE` that says why not.
//! Sections 1 to 5 are tied by `openbmt-link`.

use std::collections::BTreeSet;

const RULES: &str = include_str!("../../../../docs/LINK-RULES.md");
const ERRORS: [&str; 3] = [
    include_str!("../courier.rs"),
    include_str!("../wire.rs"),
    include_str!("../memory.rs"),
];

const TEST_SOURCES: [&str; 4] = [
    include_str!("courier.rs"),
    include_str!("wire.rs"),
    include_str!("../../../openbmt-counter-node/src/tests/link.rs"),
    include_str!("../../../openbmt-owner-node/src/tests/link.rs"),
];

/// Rules that are not tested, each with the reason. Empty: every rule of the
/// section has a test.
const NOT_HERE: [(&str, &str); 0] = [];

fn defined() -> BTreeSet<String> {
    RULES
        .lines()
        .filter_map(|l| l.strip_prefix("- **LK-"))
        .map(|rest| format!("LK-{}", &rest[..2]))
        .filter(|id| id.as_str() >= "LK-50")
        .collect()
}

#[test]
fn section_six_defines_twenty_four_rules() {
    let ids = defined();
    assert_eq!(ids.len(), 24);
    assert!(
        ids.contains("LK-50")
            && ids.contains("LK-67")
            && ids.contains("LK-73")
            && ids.contains("LK-75")
    );
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
fn every_test_of_the_section_names_a_rule_that_exists() {
    let all: BTreeSet<String> = RULES
        .lines()
        .filter_map(|l| l.strip_prefix("- **LK-"))
        .map(|rest| format!("LK-{}", &rest[..2]))
        .collect();
    let sources: String = TEST_SOURCES.concat();
    let mut seen = 0;
    for line in sources.lines() {
        if let Some(rest) = line.trim().strip_prefix("fn lk_") {
            let id = format!("LK-{}", &rest[..2]);
            assert!(all.contains(&id), "test {line} names {id}");
            seen += 1;
        }
    }
    assert!(seen > 30, "only {seen} rule tests");
}

#[test]
fn every_error_names_a_rule_that_exists() {
    let all: BTreeSet<String> = RULES
        .lines()
        .filter_map(|l| l.strip_prefix("- **LK-"))
        .map(|rest| format!("LK-{}", &rest[..2]))
        .collect();
    let mut seen = 0;
    for source in ERRORS {
        for (at, _) in source.match_indices("\"LK-") {
            let id = &source[at + 1..at + 6];
            assert!(all.contains(id), "an error names {id}");
            seen += 1;
        }
    }
    assert!(seen > 10, "only {seen} rule names in the errors");
}
