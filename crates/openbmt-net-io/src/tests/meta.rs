//! Checks that the tests line up with the rules file.

const RULES: &str = include_str!("../../../../docs/NETWORK-RULES.md");
const TEST_SOURCES: [&str; 8] = [
    include_str!("driver.rs"),
    include_str!("node_only.rs"),
    include_str!("node_only_core.rs"),
    include_str!("own_node.rs"),
    include_str!("own_node_core.rs"),
    include_str!("socks.rs"),
    include_str!("storage.rs"),
    include_str!("v2.rs"),
];

/// The rules this crate implements: those of the net crate that only a
/// driver can carry out, and the ones the driver decides itself (NR-70 and
/// on).
const IMPLEMENTED: [&str; 39] = [
    "NR-22", "NR-23", "NR-24", "NR-25", "NR-26", "NR-70", "NR-71", "NR-72", "NR-73", "NR-74",
    "NR-75", "NR-76", "NR-77", "NR-78", "NR-79", "NR-80", "NR-81", "NR-82", "NR-83", "NR-85",
    "NR-90", "NR-92", "NR-93", "NR-94", "NR-96", "NR-97", "NR-98", "NR-99", "NR-100", "NR-101",
    "NR-102", "NR-103", "NR-106", "NR-107", "NR-108", "NR-109", "NR-110", "NR-111", "NR-112",
];

/// Rules from NR-70 on that another crate alone carries out: the net crate
/// (NR-84, and the manager's NR-86 to NR-89), the owner's node (NR-91) and
/// the settings (NR-105: three fields of `openbmt-core`, which the caller
/// reads to give the driver its mode).
const IN_NET_ONLY: [&str; 9] = [
    "NR-84", "NR-86", "NR-87", "NR-88", "NR-89", "NR-91", "NR-95", "NR-104", "NR-105",
];

/// Every rule ID that the rules file defines (in bold at the start of a
/// bullet).
fn rule_ids() -> Vec<String> {
    let mut ids = Vec::new();
    for line in RULES.lines() {
        if let Some(rest) = line.trim_start().strip_prefix("- **NR-") {
            let id: String = rest.chars().take_while(char::is_ascii_digit).collect();
            if id.len() >= 2 {
                ids.push(format!("NR-{id}"));
            }
        }
    }
    ids
}

#[test]
fn the_rules_file_defines_the_rules_this_crate_implements() {
    let ids = rule_ids();
    for rule in IMPLEMENTED {
        assert!(ids.contains(&rule.to_string()), "{rule} is not in the file");
    }
}

#[test]
fn every_rule_from_nr_70_on_is_implemented_here() {
    for id in rule_ids() {
        let number: u32 = id[3..].parse().unwrap();
        if number >= 70 {
            assert!(
                IMPLEMENTED.contains(&id.as_str()) || IN_NET_ONLY.contains(&id.as_str()),
                "{id} has no test here"
            );
        }
    }
}

#[test]
fn every_implemented_rule_has_at_least_one_test_named_for_it() {
    let missing: Vec<_> = IMPLEMENTED
        .iter()
        .filter(|id| {
            let needle = format!("fn nr_{}_", &id[3..]);
            !TEST_SOURCES.iter().any(|src| src.contains(&needle))
        })
        .collect();
    assert!(missing.is_empty(), "rules without a test: {missing:?}");
}

#[test]
fn the_rules_file_keeps_its_lines_short() {
    for (i, line) in RULES.lines().enumerate() {
        assert!(line.chars().count() <= 80, "line {} is too long", i + 1);
    }
}
