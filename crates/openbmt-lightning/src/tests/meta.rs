//! Ties the rules of `docs/LIGHTNING-RULES.md` (LN-01 to LN-23) to the tests:
//! every one has a test named for it, and every name in a test or in the code
//! is a rule that exists.

use std::collections::BTreeSet;

const RULES: &str = include_str!("../../../../docs/LIGHTNING-RULES.md");
const CARGO: &str = include_str!("../../Cargo.toml");
const SOURCES: [&str; 11] = [
    include_str!("bip321.rs"),
    include_str!("bolt11.rs"),
    include_str!("cln.rs"),
    include_str!("connect.rs"),
    include_str!("credential.rs"),
    include_str!("lnd.rs"),
    include_str!("payment.rs"),
    include_str!("say.rs"),
    include_str!("settings.rs"),
    include_str!("meta.rs"),
    include_str!("mod.rs"),
];
const CORE_SETTINGS: &str = include_str!("../../../openbmt-core/src/settings.rs");

fn defined() -> BTreeSet<String> {
    RULES
        .lines()
        .filter_map(|l| l.strip_prefix("- **LN-"))
        .map(|rest| format!("LN-{}", &rest[..2]))
        .collect()
}

#[test]
fn the_rules_file_defines_ln_01_to_ln_23() {
    let want: BTreeSet<String> = (1..=23).map(|i| format!("LN-{i:02}")).collect();
    assert_eq!(defined(), want);
}

#[test]
fn every_rule_has_a_test() {
    let sources = SOURCES.concat();
    for id in defined() {
        let prefix = format!("fn ln_{}_", &id[3..]);
        assert!(sources.contains(&prefix), "{id} has no test");
    }
}

#[test]
fn every_test_that_names_a_rule_names_one_that_exists() {
    let ids = defined();
    for source in SOURCES.iter().chain([&CORE_SETTINGS]) {
        for line in source.lines() {
            if let Some(rest) = line.trim().strip_prefix("fn ln_") {
                let id = format!("LN-{}", &rest[..2]);
                assert!(ids.contains(&id), "test {line} names {id}");
            }
        }
    }
}

#[test]
fn every_rule_the_code_names_exists() {
    let ids = defined();
    let mut seen = 0;
    for file in [
        include_str!("../bip321.rs"),
        include_str!("../bolt11.rs"),
        include_str!("../cln.rs"),
        include_str!("../connect.rs"),
        include_str!("../credential.rs"),
        include_str!("../error.rs"),
        include_str!("../invoice.rs"),
        include_str!("../lib.rs"),
        include_str!("../lnd.rs"),
        include_str!("../node.rs"),
        include_str!("../request.rs"),
        include_str!("../say.rs"),
        CORE_SETTINGS,
        include_str!("../../../openbmt-approver/src/describe.rs"),
    ] {
        for (at, _) in file.match_indices("LN-") {
            let id = &file[at..at + 5];
            // A rule number is LN- and two digits; other text that happens to
            // hold LN- is not a rule.
            if id[3..].bytes().all(|b| b.is_ascii_digit()) {
                assert!(ids.contains(id), "the code names {id}");
                seen += 1;
            }
        }
    }
    assert!(seen > 60, "only {seen} rule names in the code");
}

#[test]
fn what_real_nodes_confirmed_is_marked_in_the_rules() {
    assert!(!RULES.contains("[verify, E7]"));
    let flat = RULES.split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(flat.contains("run against real nodes on a private test network"));
    assert!(flat.contains("scripts/lightning-e2e-core.sh"));
    assert!(flat.contains("rest on the source alone"));
}

#[test]
fn the_rules_cite_the_releases_and_the_documents() {
    for url in [
        "https://github.com/ElementsProject/lightning/releases",
        "https://github.com/lightningnetwork/lnd/releases",
        "https://docs.corelightning.org/reference/createrune",
        "https://docs.lightning.engineering/lightning-network-tools/lnd/macaroons",
        "https://github.com/bitcoin/bips/blob/master/bip-0321.mediawiki",
        "https://github.com/lightning/bolts/blob/master/11-payment-encoding.md",
        "https://acinq.github.io/eclair/",
    ] {
        assert!(RULES.contains(url), "{url}");
    }
}

#[test]
fn ln_23_the_crate_depends_on_workspace_crates_by_path_and_on_bitcoin_only() {
    let deps: Vec<&str> = CARGO
        .lines()
        .skip_while(|l| !l.starts_with("[dependencies]"))
        .skip(1)
        .take_while(|l| !l.starts_with('['))
        .filter(|l| !l.trim().is_empty() && !l.trim_start().starts_with('#'))
        .collect();
    assert_eq!(deps.len(), 3, "{deps:?}");
    for d in &deps {
        assert!(
            d.contains("path = \"../openbmt-")
                || d.starts_with("bitcoin = { version = \"=0.32.8\""),
            "{d}"
        );
    }
    assert!(!CARGO.contains("[dev-dependencies]") && !CARGO.contains("[build-dependencies]"));
}

#[test]
fn ln_23_the_library_code_has_nothing_that_can_panic() {
    // The attributes of the crate deny it; this reads them.
    let lib = include_str!("../lib.rs");
    for lint in ["unwrap_used", "expect_used", "panic", "indexing_slicing"] {
        assert!(lib.contains(&format!("clippy::{lint}")), "{lint}");
    }
    assert!(lib.contains("#![forbid(unsafe_code)]"));
}

#[test]
fn the_rules_keep_their_lines_to_eighty_characters() {
    let mut in_code = false;
    for (n, line) in RULES.lines().enumerate() {
        if line.starts_with("```") {
            in_code = !in_code;
        }
        if in_code || line.starts_with('|') {
            continue;
        }
        assert!(line.chars().count() <= 80, "line {}: {line}", n + 1);
    }
}
