//! Ties the rules to the tests: every rule of sections 1, 2, 3 and 5 of
//! `docs/BOX-RULES.md` has a test named for it, or an entry in `NOT_HERE`
//! that says why not. The rules of section 4 (the POS on the Box) are not in
//! this crate yet and are listed in `NOT_YET`.

use std::collections::BTreeSet;

const RULES: &str = include_str!("../../../../docs/BOX-RULES.md");
const ERRORS: [&str; 3] = [
    include_str!("../state.rs"),
    include_str!("../runner.rs"),
    include_str!("../files.rs"),
];
const TEST_SOURCES: [&str; 2] = [include_str!("state.rs"), include_str!("runner.rs")];

/// Rules of section 4: the POS on the Box. Not in this crate yet.
const NOT_YET: [&str; 4] = ["BX-40", "BX-41", "BX-42", "BX-43"];

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
        .into_iter()
        .filter(|id| !NOT_YET.contains(&id.as_str()))
        .collect()
}

#[test]
fn the_rules_file_defines_nineteen_rules_and_four_are_not_here_yet() {
    let all = all_rules();
    assert_eq!(all.len(), 23);
    for id in NOT_YET {
        assert!(all.contains(id), "{id} is not a rule");
    }
    assert_eq!(defined().len(), 19);
    assert!(defined().contains("BX-01") && defined().contains("BX-51"));
}

#[test]
fn the_not_yet_rules_are_exactly_section_four() {
    // Section 4 is the one between its heading and the next.
    let start = RULES.find("## 4.").unwrap();
    let end = RULES.find("## 5.").unwrap();
    let section: BTreeSet<String> = RULES[start..end]
        .lines()
        .filter_map(|l| l.strip_prefix("- **BX-"))
        .map(|rest| format!("BX-{}", &rest[..2]))
        .collect();
    let listed: BTreeSet<String> = NOT_YET.iter().map(|s| s.to_string()).collect();
    assert_eq!(section, listed);
    // And no test pretends to prove one of them.
    let sources: String = TEST_SOURCES.concat();
    for id in NOT_YET {
        let prefix = format!("fn bx_{}_", &id[3..]);
        assert!(!sources.contains(&prefix), "{id} has a test");
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
