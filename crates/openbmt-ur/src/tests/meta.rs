//! Ties the rules to the tests: every rule in `docs/UR-RULES.md` has a test
//! named for it, or an entry in `NOT_HERE` that says why not. Also the rules
//! file's line length.

use std::collections::BTreeSet;

const RULES: &str = include_str!("../../../../docs/UR-RULES.md");

const TEST_SOURCES: [&str; 4] = [
    include_str!("key.rs"),
    include_str!("refusals.rs"),
    include_str!("roundtrip.rs"),
    include_str!("vectors.rs"),
];

const CODE_SOURCES: [&str; 14] = [
    include_str!("../base58.rs"),
    include_str!("../bytewords.rs"),
    include_str!("../cbor.rs"),
    include_str!("../cbor_read.rs"),
    include_str!("../decoder.rs"),
    include_str!("../encoder.rs"),
    include_str!("../error.rs"),
    include_str!("../fountain.rs"),
    include_str!("../json_read.rs"),
    include_str!("../key.rs"),
    include_str!("../lib.rs"),
    include_str!("../point.rs"),
    include_str!("../psbt.rs"),
    include_str!("../hash.rs"),
];

/// Rules this crate does not test, each with the reason. The others are
/// tested here.
const NOT_HERE: [(&str, &str); 4] = [
    (
        "UR-20",
        "the apps' call is in openbmt-qr-jni, which tests it",
    ),
    (
        "UR-21",
        "the apps' call is in openbmt-qr-jni, which tests it",
    ),
    (
        "UR-22",
        "the apps' call is in openbmt-qr-jni, which tests it",
    ),
    (
        "UR-31",
        "the apps' call is in openbmt-qr-jni, which tests it",
    ),
];

fn defined() -> BTreeSet<String> {
    RULES
        .lines()
        .filter_map(|l| l.strip_prefix("- **UR-"))
        .map(|rest| format!("UR-{}", rest.get(..2).unwrap_or("")))
        .collect()
}

fn sources() -> String {
    TEST_SOURCES.concat()
}

#[test]
fn the_rules_file_defines_its_rules() {
    let ids = defined();
    assert_eq!(ids.len(), 31, "{ids:?}");
    assert!(ids.contains("UR-01") && ids.contains("UR-31"));
}

#[test]
fn every_rule_has_a_test_or_a_reason() {
    let sources = sources();
    for id in defined() {
        let prefix = format!("fn ur_{}_", &id[3..]);
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
    for line in sources().lines() {
        if let Some(rest) = line.trim().strip_prefix("fn ur_") {
            let id = format!("UR-{}", rest.get(..2).unwrap_or(""));
            assert!(ids.contains(&id), "test {line} names {id}");
        }
    }
}

#[test]
fn every_rule_the_code_names_exists() {
    let ids = defined();
    let mut seen = 0;
    for file in CODE_SOURCES {
        for (at, _) in file.match_indices("UR-") {
            let id = file.get(at..at + 5).unwrap_or("");
            // Rule codes only: the prefix `UR-` followed by two digits.
            if id
                .get(3..)
                .is_some_and(|d| d.bytes().all(|b| b.is_ascii_digit()))
            {
                assert!(ids.contains(id), "the code names {id}");
                seen += 1;
            }
        }
    }
    assert!(seen > 20, "only {seen} rule names in the code");
}

#[test]
fn the_rules_file_keeps_its_lines_to_eighty_characters() {
    let mut in_code = false;
    for (n, line) in RULES.lines().enumerate() {
        if line.starts_with("```") {
            in_code = !in_code;
        }
        if in_code || line.starts_with('|') {
            continue;
        }
        assert!(
            line.chars().count() <= 80,
            "line {} is {} long",
            n + 1,
            line.chars().count()
        );
    }
}
