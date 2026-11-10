//! Checks that the tests and drops line up with the rules file.

use crate::Drop;

const RULES: &str = include_str!("../../../../docs/NETWORK-RULES.md");
const TEST_SOURCES: [&str; 3] = [
    include_str!("events.rs"),
    include_str!("frame.rs"),
    include_str!("session.rs"),
];

/// The rules this slice of the crate implements. The address book, the
/// peer manager and the driver are later slices.
const IMPLEMENTED: [&str; 9] = [
    "NR-01", "NR-02", "NR-03", "NR-04", "NR-05", "NR-11", "NR-12", "NR-13", "NR-14",
];

/// Every rule ID that the rules file defines (in bold at the start of a
/// bullet).
fn rule_ids() -> Vec<String> {
    let mut ids = Vec::new();
    for line in RULES.lines() {
        if let Some(rest) = line.trim_start().strip_prefix("- **NR-") {
            let id: String = rest.chars().take(2).collect();
            if id.chars().all(|c| c.is_ascii_digit()) {
                ids.push(format!("NR-{id}"));
            }
        }
    }
    ids
}

/// One of every kind of drop.
fn every_drop() -> Vec<Drop> {
    vec![
        Drop::WrongMagic,
        Drop::BadCommand,
        Drop::BadChecksum,
        Drop::TooLong {
            command: "tx",
            len: 2,
            limit: 1,
        },
        Drop::UnknownTooLong { len: 1 },
        Drop::Malformed { command: "tx" },
        Drop::TooManyItems {
            command: "inv",
            count: 2,
            limit: 1,
        },
        Drop::WrongFilterType { filter_type: 1 },
        Drop::ReceiveBacklog,
        Drop::SendBacklog,
        Drop::FirstNotVersion,
        Drop::VersionTooOld { version: 1 },
        Drop::MissingServices { services: 1 },
        Drop::OwnNonce,
        Drop::UserAgentTooLong { len: 300 },
        Drop::BeforeHandshake { command: "tx" },
        Drop::DuplicateVersion,
        Drop::DuplicateVerack,
        Drop::NotReady,
        Drop::HandshakeTimeout,
        Drop::PingUnanswered,
    ]
}

#[test]
fn the_rules_file_defines_the_rules_this_slice_implements() {
    let ids = rule_ids();
    assert!(ids.len() >= 37, "{}", ids.len());
    for rule in IMPLEMENTED {
        assert!(ids.contains(&rule.to_string()), "{rule} is not in the file");
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
fn every_drop_names_an_implemented_rule_of_the_file_and_says_so_in_words() {
    let ids = rule_ids();
    for d in every_drop() {
        let rule = d.rule();
        assert!(
            IMPLEMENTED.contains(&rule),
            "{rule} is not implemented: {d:?}"
        );
        assert!(ids.contains(&rule.to_string()), "{rule} is not in the file");
        let text = d.to_string();
        assert!(text.starts_with(&format!("{rule}: ")), "{text}");
        assert!(text.len() > rule.len() + 4);
    }
}

#[test]
fn every_drop_kind_is_in_the_list() {
    // A match with no wildcard: a new kind of drop does not compile until
    // it is added here and to `every_drop`.
    for d in every_drop() {
        match d {
            Drop::WrongMagic
            | Drop::BadCommand
            | Drop::BadChecksum
            | Drop::TooLong { .. }
            | Drop::UnknownTooLong { .. }
            | Drop::Malformed { .. }
            | Drop::TooManyItems { .. }
            | Drop::WrongFilterType { .. }
            | Drop::ReceiveBacklog
            | Drop::SendBacklog
            | Drop::FirstNotVersion
            | Drop::VersionTooOld { .. }
            | Drop::MissingServices { .. }
            | Drop::OwnNonce
            | Drop::UserAgentTooLong { .. }
            | Drop::BeforeHandshake { .. }
            | Drop::DuplicateVersion
            | Drop::DuplicateVerack
            | Drop::NotReady
            | Drop::HandshakeTimeout
            | Drop::PingUnanswered => {}
        }
    }
}
