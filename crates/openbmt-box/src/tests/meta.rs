//! Ties the rules to the tests: every rule of `docs/BOX-RULES.md` has a test
//! named for it, or an entry in `NOT_HERE` that says why not.

use std::collections::BTreeSet;

const RULES: &str = include_str!("../../../../docs/BOX-RULES.md");
const ERRORS: [&str; 9] = [
    include_str!("../state.rs"),
    include_str!("../runner.rs"),
    include_str!("../files.rs"),
    include_str!("../pos.rs"),
    include_str!("../https.rs"),
    include_str!("../pos_config.rs"),
    include_str!("../vendor.rs"),
    include_str!("../till.rs"),
    include_str!("../till_listener.rs"),
];
const TEST_SOURCES: [&str; 9] = [
    include_str!("state.rs"),
    include_str!("runner.rs"),
    include_str!("pos.rs"),
    include_str!("https.rs"),
    include_str!("vendor.rs"),
    include_str!("status_page.rs"),
    include_str!("till.rs"),
    include_str!("../../tests/no_console.rs"),
    include_str!("../../tests/till.rs"),
];

/// Rules this crate does not test, each with the reason.
const NOT_HERE: [(&str, &str); 1] = [(
    "BX-17",
    "what the owner phone's screen says when there is no Key app is the \
     phone's; the bytes are the same as with one (bx_17 shows the pairing \
     does not depend on the Key app's identity)",
)];

fn all_rules() -> BTreeSet<String> {
    RULES
        .lines()
        .filter_map(|l| l.strip_prefix("- **BX-"))
        .map(|rest| format!("BX-{}", &rest[..2]))
        .collect()
}

fn defined() -> BTreeSet<String> {
    all_rules()
}

#[test]
fn the_rules_file_defines_forty_nine_rules() {
    let all = all_rules();
    assert_eq!(all.len(), 49);
    assert!(all.contains("BX-01") && all.contains("BX-43") && all.contains("BX-79"));
}

#[test]
fn section_four_has_its_ten_rules_and_each_has_tests() {
    // Section 4 is the one between its heading and the next.
    let start = RULES.find("## 4.").unwrap();
    let end = RULES.find("## 5.").unwrap();
    let section: BTreeSet<String> = RULES[start..end]
        .lines()
        .filter_map(|l| l.strip_prefix("- **BX-"))
        .map(|rest| format!("BX-{}", &rest[..2]))
        .collect();
    let expected: BTreeSet<String> = [
        "BX-40", "BX-41", "BX-42", "BX-43", "BX-44", "BX-45", "BX-46", "BX-47", "BX-48", "BX-49",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    assert_eq!(section, expected);
    let sources: String = TEST_SOURCES.concat();
    for id in &expected {
        let prefix = format!("fn bx_{}_", &id[3..]);
        assert!(
            sources.matches(&prefix).count() >= 3,
            "{id} has fewer than three tests"
        );
    }
}

#[test]
fn section_five_b_has_its_eight_rules_and_each_has_tests() {
    let start = RULES.find("## 5b.").unwrap();
    let end = RULES.find("## 5c.").unwrap();
    let section: BTreeSet<String> = RULES[start..end]
        .lines()
        .filter_map(|l| l.strip_prefix("- **BX-"))
        .map(|rest| format!("BX-{}", &rest[..2]))
        .collect();
    let expected: BTreeSet<String> = (54..=61).map(|n| format!("BX-{n}")).collect();
    assert_eq!(section, expected);
    let sources: String = TEST_SOURCES.concat();
    for id in &expected {
        let prefix = format!("fn bx_{}_", &id[3..]);
        assert!(
            sources.matches(&prefix).count() >= 2,
            "{id} has fewer than two tests"
        );
    }
}

#[test]
fn section_five_c_has_its_ten_rules_and_each_has_tests() {
    let start = RULES.find("## 5c.").unwrap();
    let end = RULES.find("## 6.").unwrap();
    let section: BTreeSet<String> = RULES[start..end]
        .lines()
        .filter_map(|l| l.strip_prefix("- **BX-"))
        .map(|rest| format!("BX-{}", &rest[..2]))
        .collect();
    let expected: BTreeSet<String> = (70..=79).map(|n| format!("BX-{n}")).collect();
    assert_eq!(section, expected);
    let sources: String = TEST_SOURCES.concat();
    for id in &expected {
        let prefix = format!("fn bx_{}_", &id[3..]);
        assert!(
            sources.matches(&prefix).count() >= 2,
            "{id} has fewer than two tests"
        );
    }
}

#[test]
fn the_rules_file_keeps_its_lines_to_eighty_characters() {
    let mut in_code = false;
    for (n, line) in RULES.lines().enumerate() {
        if line.starts_with("```") || line.trim_start().starts_with("```") {
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
fn every_rule_has_a_test_or_a_reason() {
    let sources: String = TEST_SOURCES.concat();
    for id in defined() {
        let prefix = format!("fn bx_{}_", &id[3..]);
        let tested = sources.contains(&prefix);
        let excused = NOT_HERE.iter().any(|(r, _)| *r == id);
        assert!(
            tested || excused,
            "{id} has no test and no entry in NOT_HERE"
        );
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
        if let Some(rest) = line.trim().strip_prefix("fn bx_") {
            let id = format!("BX-{}", &rest[..2]);
            assert!(ids.contains(&id), "test {line} names {id}");
            seen += 1;
        }
    }
    assert!(seen > 35, "only {seen} rule tests");
}

#[test]
fn every_error_names_a_rule_that_exists() {
    let ids = all_rules();
    let lk: BTreeSet<&str> = ["LK-02", "LK-40", "LK-41", "LK-53"].into();
    let mut seen = 0;
    for source in ERRORS {
        // Only the code, not the tests that may follow.
        let code = source.split("#[cfg(test)]").next().unwrap_or(source);
        for (at, _) in code.match_indices("\"BX-") {
            let id = &code[at + 1..at + 6];
            assert!(ids.contains(id), "an error names {id}");
            seen += 1;
        }
        for (at, _) in code.match_indices("\"LK-") {
            let id = &code[at + 1..at + 6];
            assert!(lk.contains(id) || id.starts_with("LK-"), "{id}");
        }
    }
    assert!(seen > 10, "only {seen} rule names in the errors");
}
