//! Ties the rules to the tests: every rule in `docs/COUNTER-RULES.md` has a
//! test named for it, or an entry in `NOT_HERE` that says why not.

use std::collections::BTreeSet;

const RULES: &str = include_str!("../../../../docs/COUNTER-RULES.md");
const REFUSALS: &str = include_str!("../refusal.rs");

const TEST_SOURCES: [&str; 13] = [
    include_str!("encodings.rs"),
    include_str!("heartbeat.rs"),
    include_str!("open.rs"),
    include_str!("overrides.rs"),
    include_str!("recordlog.rs"),
    include_str!("records.rs"),
    include_str!("refunds.rs"),
    include_str!("reports.rs"),
    include_str!("settle.rs"),
    include_str!("states.rs"),
    include_str!("storage.rs"),
    include_str!("valuation.rs"),
    include_str!("kit.rs"),
];

/// Rules this crate does not test, each with the reason. The counter
/// implements every rule except the ones that only state a limit and the
/// ones that belong to the app it is built into.
const NOT_HERE: [(&str, &str); 11] = [
    (
        "CR-56",
        "it states what the heartbeat does not guard against (a bad build of the same software \
         family); there is nothing for the counter to enforce or to test",
    ),
    (
        "CR-74",
        "it states what the refund rules do not do (prove who the customer is, stop a wrong \
         destination, learn whether a refund was paid); there is nothing to enforce or to test",
    ),
    (
        "CR-82",
        "it is the node's rule, when `Counter::settle` is called; its tests are in \
         `openbmt-counter-node`",
    ),
    (
        "CR-83",
        "the payment request is made where the counter is put in an app, from the sale's script \
         and its satoshis; it is tested in openbmt-counter-ffi",
    ),
    (
        "CR-84",
        "the manager's PIN is kept by the Android app; its tests are the app's JVM tests \
         (`apps/counter-android`)",
    ),
    (
        "CR-85",
        "the PIN check and its delays are the Android app's; its tests are the app's JVM tests",
    ),
    (
        "CR-86",
        "which actions ask for the PIN is the Android app's; its tests are the app's JVM tests",
    ),
    (
        "CR-87",
        "the start without a proxy is refused in openbmt-counter-ffi and tested there; the app's \
         Tor gate is tested in its JVM tests",
    ),
    (
        "CR-88",
        "when the node runs is the Android activity's; it needs a device to test",
    ),
    (
        "CR-89",
        "the app fetches the price; the fetch limits are tested in openbmt-counter-ffi against \
         openbmt-rates",
    ),
    (
        "CR-90",
        "the camera is the Android app's; its permission list is checked on the built APK \
         (`apps/counter-android/tests/check-manifest.sh`) and the scan's pure parts and native \
         reader are tested in `apps/android-common`; it needs a device to test",
    ),
];

fn defined() -> BTreeSet<String> {
    RULES
        .lines()
        .filter_map(|l| l.strip_prefix("- **CR-"))
        .map(|rest| format!("CR-{}", &rest[..2]))
        .collect()
}

#[test]
fn the_rules_file_defines_sixty_nine_rules() {
    let ids = defined();
    assert_eq!(ids.len(), 69);
    assert!(
        ids.contains("CR-01")
            && ids.contains("CR-42")
            && ids.contains("CR-56")
            && ids.contains("CR-74")
            && ids.contains("CR-75")
            && ids.contains("CR-82")
            && ids.contains("CR-83")
            && ids.contains("CR-89")
            && ids.contains("CR-90")
    );
}

#[test]
fn every_rule_has_a_test_or_a_reason() {
    let sources: String = TEST_SOURCES.concat();
    for id in defined() {
        let prefix = format!("fn cr_{}_", &id[3..]);
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
    for line in sources.lines() {
        if let Some(rest) = line.trim().strip_prefix("fn cr_") {
            let id = format!("CR-{}", &rest[..2]);
            assert!(ids.contains(&id), "test {line} names {id}");
        }
    }
}

#[test]
fn every_refusal_names_a_rule_that_exists() {
    let ids = defined();
    let mut seen = 0;
    for (at, _) in REFUSALS.match_indices("\"CR-") {
        let id = &REFUSALS[at + 1..at + 6];
        assert!(ids.contains(id), "refusal names {id}");
        seen += 1;
    }
    assert!(seen > 20, "only {seen} rule names in the refusals");
}
