//! Ties the rules to the tests: every rule in `docs/POS-RULES.md` has a test
//! named for it, or an entry in `NOT_HERE` that says why not. Also the rules
//! file's line length, and that the stored magic is not used elsewhere.

use std::collections::BTreeSet;
use std::path::Path;

const RULES: &str = include_str!("../../../../docs/POS-RULES.md");
const LIB: &str = include_str!("../lib.rs");

const TEST_SOURCES: [&str; 7] = [
    include_str!("alerts.rs"),
    include_str!("connector.rs"),
    include_str!("facts.rs"),
    include_str!("matching.rs"),
    include_str!("model.rs"),
    include_str!("props.rs"),
    include_str!("reference.rs"),
];
const MORE_TEST_SOURCES: [&str; 2] = [include_str!("storage.rs"), include_str!("kit.rs")];

/// Rules this crate does not test, each with the reason: those of section 9
/// are the vendor readers, which are in `openbmt-pos-readers` and tested there
/// (its own `tests::meta` ties each to a test).
const NOT_HERE: [(&str, &str); 10] = [
    (
        "PS-90",
        "the vendor readers are in openbmt-pos-readers, which tests this rule",
    ),
    (
        "PS-91",
        "the vendor readers are in openbmt-pos-readers, which tests this rule",
    ),
    (
        "PS-92",
        "the vendor readers are in openbmt-pos-readers, which tests this rule",
    ),
    (
        "PS-93",
        "the vendor readers are in openbmt-pos-readers, which tests this rule",
    ),
    (
        "PS-94",
        "the vendor readers are in openbmt-pos-readers, which tests this rule",
    ),
    (
        "PS-95",
        "the vendor readers are in openbmt-pos-readers, which tests this rule",
    ),
    (
        "PS-96",
        "the vendor readers are in openbmt-pos-readers, which tests this rule",
    ),
    (
        "PS-97",
        "the vendor readers are in openbmt-pos-readers, which tests this rule",
    ),
    (
        "PS-98",
        "the vendor readers are in openbmt-pos-readers, which tests this rule",
    ),
    (
        "PS-99",
        "the vendor readers are in openbmt-pos-readers, which tests this rule",
    ),
];

fn defined() -> BTreeSet<String> {
    RULES
        .lines()
        .filter_map(|l| l.strip_prefix("- **PS-"))
        .map(|rest| format!("PS-{}", &rest[..2]))
        .collect()
}

fn sources() -> String {
    format!("{}{}", TEST_SOURCES.concat(), MORE_TEST_SOURCES.concat())
}

#[test]
fn the_rules_file_defines_its_rules() {
    let ids = defined();
    assert_eq!(ids.len(), 65, "{ids:?}");
    assert!(ids.contains("PS-01") && ids.contains("PS-82"));
}

#[test]
fn every_rule_has_a_test_or_a_reason() {
    let sources = sources();
    for id in defined() {
        let prefix = format!("fn ps_{}_", &id[3..]);
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
        if let Some(rest) = line.trim().strip_prefix("fn ps_") {
            let id = format!("PS-{}", &rest[..2]);
            assert!(ids.contains(&id), "test {line} names {id}");
        }
    }
}

#[test]
fn every_rule_the_code_names_exists() {
    let ids = defined();
    let mut seen = 0;
    for file in [
        include_str!("../alerts.rs"),
        include_str!("../connector.rs"),
        include_str!("../facts.rs"),
        include_str!("../ledger.rs"),
        include_str!("../lib.rs"),
        include_str!("../matcher.rs"),
        include_str!("../model.rs"),
        include_str!("../settings.rs"),
    ] {
        for (at, _) in file.match_indices("PS-") {
            let id = &file[at..at + 5];
            assert!(ids.contains(id), "the code names {id}");
            seen += 1;
        }
    }
    assert!(seen > 40, "only {seen} rule names in the code");
    assert!(LIB.contains("PS-01"));
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

#[test]
fn the_stored_magic_is_used_nowhere_else_in_the_workspace() {
    fn walk(dir: &Path, hits: &mut Vec<String>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(&path, hits);
            } else if path.extension().is_some_and(|e| e == "rs") {
                let text = std::fs::read_to_string(&path).unwrap();
                if text.contains("b\"OBMZ\"") {
                    hits.push(path.display().to_string());
                }
            }
        }
    }
    let crates = std::path::PathBuf::from(
        std::env::var_os("CARGO_MANIFEST_DIR")
            .expect("cargo sets CARGO_MANIFEST_DIR when it runs tests"),
    )
    .join("..");
    let mut hits = Vec::new();
    walk(&crates, &mut hits);
    assert_eq!(hits.len(), 1, "{hits:?}");
    assert!(hits[0].contains("openbmt-pos"));
}
