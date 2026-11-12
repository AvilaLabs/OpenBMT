//! Ties the rules of `docs/SWAP-RULES.md` to the tests: every rule has a
//! test named for it, and every name in a test or in the code is a rule that
//! exists.

use std::collections::BTreeSet;

const RULES: &str = include_str!("../../../../docs/SWAP-RULES.md");
const SOURCES: [&str; 12] = [
    include_str!("claim.rs"),
    include_str!("fixtures.rs"),
    include_str!("follow.rs"),
    include_str!("http.rs"),
    include_str!("meta.rs"),
    include_str!("mod.rs"),
    include_str!("pairs.rs"),
    include_str!("pretend.rs"),
    include_str!("reply.rs"),
    include_str!("request.rs"),
    include_str!("state.rs"),
    include_str!("tree.rs"),
];
const CODE: [&str; 16] = [
    include_str!("../claim.rs"),
    include_str!("../error.rs"),
    include_str!("../follow.rs"),
    include_str!("../http.rs"),
    include_str!("../keys.rs"),
    include_str!("../lib.rs"),
    include_str!("../lockup.rs"),
    include_str!("../onion.rs"),
    include_str!("../pairs.rs"),
    include_str!("../reply.rs"),
    include_str!("../request.rs"),
    include_str!("../state.rs"),
    include_str!("../status.rs"),
    include_str!("../testing.rs"),
    include_str!("../tree.rs"),
    include_str!("../../Cargo.toml"),
];

/// The rules the file defines, which are a subset of the numbers 01 to 91.
fn defined() -> BTreeSet<String> {
    RULES
        .lines()
        .filter_map(|l| l.strip_prefix("- **SW-"))
        .map(|rest| format!("SW-{}", &rest[..2]))
        .collect()
}

fn expected() -> BTreeSet<String> {
    (1..=4)
        .chain(10..=13)
        .chain(20..=28)
        .chain(30..=31)
        .chain(40..=45)
        .chain(50..=55)
        .chain(60..=63)
        .chain(70..=74)
        .chain(80..=82)
        .chain(90..=91)
        .map(|i| format!("SW-{i:02}"))
        .collect()
}

#[test]
fn the_rules_file_defines_the_rules() {
    assert_eq!(defined(), expected());
}

#[test]
fn every_rule_has_a_test() {
    let sources = SOURCES.concat();
    for id in defined() {
        let prefix = format!("fn sw_{}_", &id[3..]);
        assert!(sources.contains(&prefix), "{id} has no test");
    }
}

#[test]
fn every_test_that_names_a_rule_names_one_that_exists() {
    let ids = defined();
    for source in SOURCES {
        for line in source.lines() {
            if let Some(rest) = line.trim().strip_prefix("fn sw_") {
                let id = format!("SW-{}", &rest[..2]);
                // `sw_2x_` names a group, not a rule.
                if rest.starts_with("2x_") {
                    continue;
                }
                assert!(ids.contains(&id), "test {line} names {id}");
            }
        }
    }
}

#[test]
fn every_rule_the_code_names_exists() {
    let ids = defined();
    let mut seen = 0;
    for file in CODE {
        for (at, _) in file.match_indices("SW-") {
            let id = &file[at..(at + 5).min(file.len())];
            if id.len() == 5 && id[3..].bytes().all(|b| b.is_ascii_digit()) {
                seen += 1;
                assert!(ids.contains(id), "the code names {id}");
            }
        }
    }
    assert!(seen > 30);
}

#[test]
fn the_rules_file_keeps_to_80_columns_outside_tables() {
    for line in RULES.lines().filter(|l| !l.starts_with('|')) {
        assert!(line.chars().count() <= 80, "{line}");
    }
}

#[test]
fn the_numbers_of_the_table_are_the_constants() {
    use crate::*;
    assert_eq!(MIN_INVOICE_SATS, 10_000);
    assert_eq!(MAX_INVOICE_SATS, 25_000_000);
    assert_eq!(MAX_DESCRIPTION_BYTES, 100);
    assert_eq!(MAX_REPLY_BYTES, 16 * 1024);
    assert_eq!(MAX_REPLY_DEPTH, 6);
    assert_eq!(MAX_LEAF_BYTES, 128);
    assert_eq!(MIN_INVOICE_LIFE_SECS, 300);
    assert_eq!(MIN_CLAIM_BLOCKS, 36);
    assert_eq!(MIN_ONCHAIN_SATS, 1_000);
    assert_eq!(PAIRS_FRESH_SECS, 600);
    assert_eq!(MAX_HTTP_BYTES, 64 * 1024);
    assert_eq!(POLL_WAITING_SECS, 10);
    assert_eq!(POLL_LOCKED_SECS, 30);
}
