//! Ties the rules to the tests: every rule in `docs/COUNTER-RULES.md` has a
//! test named for it, or an entry in `NOT_HERE` that says why not.

use std::collections::BTreeSet;

const RULES: &str = include_str!("../../../../docs/COUNTER-RULES.md");
const REFUSALS: &str = include_str!("../refusal.rs");

const TEST_SOURCES: [&str; 19] = [
    include_str!("encodings.rs"),
    include_str!("heartbeat.rs"),
    include_str!("lightning.rs"),
    include_str!("open.rs"),
    include_str!("overrides.rs"),
    include_str!("own_node.rs"),
    include_str!("recordlog.rs"),
    include_str!("records.rs"),
    include_str!("refunds.rs"),
    include_str!("reports.rs"),
    include_str!("settings.rs"),
    include_str!("settle.rs"),
    include_str!("batch_held.rs"),
    include_str!("staff.rs"),
    include_str!("states.rs"),
    include_str!("storage.rs"),
    include_str!("totals.rs"),
    include_str!("valuation.rs"),
    include_str!("kit.rs"),
];

/// Rules this crate does not test, each with the reason. The counter
/// implements every rule except the ones that only state a limit and the
/// ones that belong to the app it is built into.
const NOT_HERE: [(&str, &str); 58] = [
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
    (
        "CR-91",
        "the ceremony is the node's, behind openbmt-counter-ffi; its tests are in \
         `openbmt-counter-node` (`tests/paired.rs`) and `openbmt-counter-ffi`",
    ),
    (
        "CR-92",
        "the match, the window and the bundle checks are the node's; its tests are in \
         `openbmt-counter-node` and, end to end, in `openbmt-counter-ffi` (`tests/pairing.rs`)",
    ),
    (
        "CR-93",
        "the offer's roots, wallet and network are fixed in the node's ceremony; tested in \
         `openbmt-counter-node`, `openbmt-core` and `openbmt-approver`",
    ),
    (
        "CR-94",
        "the settings checks are made when the node takes the bundle and again at start; \
         tested in `openbmt-counter-node` and `openbmt-counter-ffi`",
    ),
    (
        "CR-95",
        "the device and counter lists are checked by the node; tested in \
         `openbmt-counter-node` and `openbmt-counter-ffi`",
    ),
    (
        "CR-96",
        "the first batch goes through this crate's `CounterBatches::add` from the node, with \
         the node's own smallest size; tested in `openbmt-counter-node` and \
         `openbmt-counter-ffi`",
    ),
    (
        "CR-97",
        "the stored record is the node's encoding; tested in `openbmt-counter-node`, with its \
         fuzz target `counter_pairing_record_decode`",
    ),
    (
        "CR-98",
        "the pairing screens and the sealed record are the Android app's; its tests are the \
         app's JVM tests (`apps/counter-android`)",
    ),
    (
        "CR-99",
        "the peers are the bundle's hints and the node's to use; tested in \
         `openbmt-counter-node` and `openbmt-core` (`bundle`)",
    ),
    (
        "CR-100",
        "the address is read and refused in `openbmt-counter-node` (`ownnode`) and kept by the \
         Android app behind the manager's PIN; tested in `openbmt-counter-node`, \
         `openbmt-counter-ffi` and the app's JVM tests",
    ),
    (
        "CR-101",
        "a home-network address is refused where the address is read; tested in \
         `openbmt-counter-node` (`tests/ownnode.rs`) and `openbmt-counter-ffi`",
    ),
    (
        "CR-102",
        "the Tor proxy is the start's and the app's; tested in `openbmt-counter-node`, \
         `openbmt-counter-ffi` and the app's JVM tests",
    ),
    (
        "CR-103",
        "what the node does with the driver's own-node channel is the node's; tested in \
         `openbmt-counter-node` (`tests/own_node.rs`)",
    ),
    (
        "CR-107",
        "the health line and Try again are the node's and the app's; tested in \
         `openbmt-counter-node`, `openbmt-counter-ffi` and the app's JVM tests",
    ),
    (
        "CR-112",
        "what the driver asks the node for, and when, is the node's; tested in \
         `openbmt-counter-node` (`tests/lightning.rs`)",
    ),
    (
        "CR-113",
        "the sale code and the line about Lightning are made in `openbmt-counter-node` and \
         `openbmt-counter-ffi` and shown by the app; tested there and in the app's JVM tests",
    ),
    (
        "CR-114",
        "the transport is the Android app's; its tests are the app's JVM tests \
         (`apps/counter-android`)",
    ),
    (
        "CR-115",
        "the credential is read and tested in `openbmt-counter-node`, kept sealed by the \
         Android app behind the manager's PIN; tested in `openbmt-counter-node`, \
         `openbmt-counter-ffi` and the app's JVM tests",
    ),
    (
        "CR-116",
        "the Lightning health line and the check of the settings' node key are the node's and \
         the app's; tested in `openbmt-counter-node`, `openbmt-counter-ffi` and the app's JVM \
         tests",
    ),
    (
        "CR-118",
        "the bound is the driver's; tested in `openbmt-counter-node` (`tests/lightning.rs`)",
    ),
    (
        "CR-130",
        "the bundle's relay is `openbmt-core`'s encoding and the owner fills it; tested in \
         `openbmt-core` (`bundle`, `relay`) and `openbmt-owner-ffi`, with the fuzz target \
         `counter_bundle_decode`",
    ),
    (
        "CR-131",
        "the relay in the stored record is the node's encoding; tested in \
         `openbmt-counter-node` (`tests/paired.rs`), with the fuzz target \
         `counter_pairing_record_decode`",
    ),
    (
        "CR-132",
        "the signed relay for counters is `openbmt-core`'s frame and the node takes it; tested \
         in `openbmt-core` (`relay`), `openbmt-counter-node`, `openbmt-counter-ffi` and \
         `openbmt-owner-ffi`, with the fuzz target `relay_for_counters_decode`",
    ),
    (
        "CR-133",
        "the relay is made and read in `openbmt-core` (`relay`) and filled by the owner's \
         calls; tested in `openbmt-core` and `openbmt-owner-ffi`",
    ),
    (
        "CR-134",
        "the start and the route of the link are the node's and the start's; tested in \
         `openbmt-counter-node` (`tests/relaylink.rs`), `openbmt-counter-ffi` and \
         `openbmt-link` (LK-31)",
    ),
    (
        "CR-135",
        "the turn of the link is the node's and the ffi's; the thread is the app's; tested in \
         `openbmt-counter-node` (`tests/link.rs`), `openbmt-counter-ffi` and the app's JVM \
         tests",
    ),
    (
        "CR-136",
        "the link line is the node's words and the app's screen; tested in \
         `openbmt-counter-node`, `openbmt-counter-ffi` and the app's JVM tests",
    ),
    (
        "CR-161",
        "the settings file is the node's; tested in `openbmt-counter-node` \
         (`tests/settings.rs`), with the fuzz target `counter_settings_file_decode`",
    ),
    (
        "CR-163",
        "the start reads the record and the file; tested in `openbmt-counter-node` and \
         `openbmt-counter-ffi`",
    ),
    (
        "CR-164",
        "the restart is the node's state and the app's act; tested in `openbmt-counter-node`, \
         `openbmt-counter-ffi` and the app's JVM tests",
    ),
    (
        "CR-185",
        "the settings field is `openbmt_core::settings`'s; tested in `openbmt-core` \
         (`cr_185_*` in `settings.rs`)",
    ),
    (
        "CR-166",
        "the device list message is dropped by the link; tested in `openbmt-counter-node` \
         (`tests/link.rs`)",
    ),
    (
        "CR-140",
        "the list of cashiers and its file are the Android app's; its tests are the app's JVM \
         tests (`apps/counter-android`), and the manifest and the extraction rules are checked \
         there",
    ),
    (
        "CR-141",
        "a cashier's name and PIN are kept by the Android app; its tests are the app's JVM tests",
    ),
    (
        "CR-142",
        "the Staff screen is the Android app's; the record it asks for is tested here (CR-143) \
         and its steps in the app's JVM tests",
    ),
    (
        "CR-144",
        "the owner's reading of the record is `openbmt-books`'s (BK-35 to BK-37); tested there",
    ),
    (
        "CR-145",
        "it states that adding cashiers loosens nothing: the limits it names are CR-20 and \
         CR-21, tested above; there is nothing more to enforce or to test",
    ),
    (
        "CR-146",
        "the till lock turning on and off with the list is the Android app's; its tests are the \
         app's JVM tests",
    ),
    (
        "CR-147",
        "the sign-in screen is the Android app's; its tests are the app's JVM tests",
    ),
    (
        "CR-148",
        "who is named as the actor is the Android app's; the library keeps the name it is \
         given (CR-151, CR-61); the app's JVM tests cover the choice",
    ),
    (
        "CR-149",
        "when the app locks is the Android app's; its tests are the app's JVM tests, and the \
         use of the activity's visibility needs a device to test",
    ),
    (
        "CR-150",
        "the hold on a PIN is the Android app's (CR-85); its tests are the app's JVM tests",
    ),
    (
        "CR-154",
        "which totals need the manager's PIN is the Android app's; the library gives the lines \
         only when asked (CR-153); its tests are the app's JVM tests",
    ),
    (
        "CR-155",
        "the Today screen and its words are the Android app's; its tests are the app's JVM tests",
    ),
    (
        "CR-156",
        "it states what the cashier list does not do; there is nothing to enforce or to test",
    ),
    (
        "CR-177",
        "which screen comes back is the Android app's; its tests are the app's JVM tests",
    ),
    (
        "CR-178",
        "the redraw after a change of light and dark is the Android app's; the choice of screen \
         it draws is tested in the app's JVM tests, and the window needs a device to test",
    ),
];

/// The rule number at the start of `rest` (what follows `CR-`): every digit,
/// so that `CR-07` and `CR-100` both read.
fn number(rest: &str) -> String {
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    format!("CR-{digits}")
}

fn defined() -> BTreeSet<String> {
    RULES
        .lines()
        .filter_map(|l| l.strip_prefix("- **CR-"))
        .map(number)
        .collect()
}

#[test]
fn the_rules_file_defines_one_hundred_and_thirty_seven_rules() {
    let ids = defined();
    assert_eq!(ids.len(), 137);
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
            && ids.contains("CR-99")
            && ids.contains("CR-100")
            && ids.contains("CR-108")
            && ids.contains("CR-109")
            && ids.contains("CR-116")
            && ids.contains("CR-117")
            && ids.contains("CR-118")
            && ids.contains("CR-130")
            && ids.contains("CR-136")
            && ids.contains("CR-140")
            && ids.contains("CR-156")
            && ids.contains("CR-160")
            && ids.contains("CR-166")
            && ids.contains("CR-167")
            && ids.contains("CR-169")
            && ids.contains("CR-178")
            && ids.contains("CR-185")
            && ids.contains("CR-186")
            && ids.contains("CR-119")
            && ids.contains("CR-120")
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
            let id = number(rest);
            assert!(ids.contains(&id), "test {line} names {id}");
        }
    }
}

#[test]
fn every_refusal_names_a_rule_that_exists() {
    let ids = defined();
    let mut seen = 0;
    for (at, _) in REFUSALS.match_indices("\"CR-") {
        let id = number(&REFUSALS[at + 4..]);
        assert!(ids.contains(&id), "refusal names {id}");
        seen += 1;
    }
    assert!(seen > 20, "only {seen} rule names in the refusals");
}
