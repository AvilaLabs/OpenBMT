//! Ties the rules of section 9 of `docs/POS-RULES.md` (PS-90 to PS-99) to
//! the tests: every one has a test named for it. The tests of the other
//! rules are in `openbmt-pos`, which lists these as not tested there.

use std::collections::BTreeSet;

const RULES: &str = include_str!("../../../../docs/POS-RULES.md");
const CARGO: &str = include_str!("../../Cargo.toml");
const SOURCES: [&str; 7] = [
    include_str!("clover.rs"),
    include_str!("common.rs"),
    include_str!("refusals.rs"),
    include_str!("say.rs"),
    include_str!("square.rs"),
    include_str!("time.rs"),
    include_str!("meta.rs"),
];

fn defined() -> BTreeSet<String> {
    RULES
        .lines()
        .filter_map(|l| l.strip_prefix("- **PS-9"))
        .map(|rest| format!("PS-9{}", &rest[..1]))
        .collect()
}

fn section() -> String {
    let start = RULES.find("## 9. Vendor readers").expect("section 9");
    let end = RULES[start..]
        .find("## 10.")
        .map_or(RULES.len(), |e| start + e);
    RULES[start..end].to_string()
}

#[test]
fn the_rules_file_defines_ps_90_to_ps_99() {
    let ids = defined();
    let want: BTreeSet<String> = (0..10).map(|i| format!("PS-9{i}")).collect();
    assert_eq!(ids, want);
}

#[test]
fn every_rule_of_section_9_has_a_test() {
    let sources = SOURCES.concat();
    for id in defined() {
        let prefix = format!("fn ps_{}_", &id[3..]);
        assert!(sources.contains(&prefix), "{id} has no test");
    }
}

#[test]
fn every_test_that_names_a_rule_of_section_9_names_one_that_exists() {
    let ids = defined();
    for line in SOURCES.concat().lines() {
        if let Some(rest) = line.trim().strip_prefix("fn ps_9") {
            let id = format!("PS-9{}", &rest[..1]);
            assert!(ids.contains(&id), "test {line} names {id}");
        }
    }
}

#[test]
fn every_rule_the_code_names_exists() {
    let ids = defined();
    let mut seen = 0;
    for file in [
        include_str!("../clover.rs"),
        include_str!("../common.rs"),
        include_str!("../error.rs"),
        include_str!("../lib.rs"),
        include_str!("../say.rs"),
        include_str!("../square.rs"),
        include_str!("../time.rs"),
    ] {
        for (at, _) in file.match_indices("PS-9") {
            let id = &file[at..at + 5];
            assert!(ids.contains(id), "the code names {id}");
            seen += 1;
        }
    }
    assert!(seen > 30, "only {seen} rule names in the code");
}

#[test]
fn what_the_vendors_documents_did_not_confirm_is_marked_in_the_rules() {
    let s = section();
    assert!(s.contains("[verify, E5]"));
    assert!(s.contains("[verify, E6]"));
    assert!(s.contains("written from the vendors' documentation"));
}

#[test]
fn ps_99_the_crate_depends_on_workspace_crates_by_path_and_on_nothing_else() {
    let deps: Vec<&str> = CARGO
        .lines()
        .skip_while(|l| !l.starts_with("[dependencies]"))
        .skip(1)
        .take_while(|l| !l.starts_with('['))
        .filter(|l| !l.trim().is_empty() && !l.trim_start().starts_with('#'))
        .collect();
    assert_eq!(deps.len(), 4, "{deps:?}");
    for d in deps {
        assert!(d.contains("path = \"../openbmt-"), "{d}");
    }
    assert!(!CARGO.contains("[dev-dependencies]") && !CARGO.contains("[build-dependencies]"));
}

#[test]
fn the_rules_section_keeps_its_lines_to_eighty_characters() {
    let mut in_code = false;
    for (n, line) in section().lines().enumerate() {
        if line.starts_with("```") {
            in_code = !in_code;
        }
        if in_code || line.starts_with('|') {
            continue;
        }
        assert!(line.chars().count() <= 80, "line {n}: {line}");
    }
}
