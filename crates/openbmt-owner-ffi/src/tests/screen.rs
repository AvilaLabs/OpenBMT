//! The words of this layer under `docs/SCREEN-RULES.md` (SC-12), the check
//! that every rule string has words, and the count of `unsafe`.

use std::collections::BTreeSet;

use openbmt_core::say::{check, Quoted, Tone, Who, Words};

use super::wire::Words as Wire;
use crate::say::rule_strings;
use crate::{all_words, words_for, Kind};

/// The words a response carried, as the rules of SC-12 take them.
pub fn check_wire_words(w: &Wire) -> Result<(), String> {
    let leak = |s: &str| -> &'static str { Box::leak(s.to_string().into_boxed_str()) };
    let tone = match w.tone {
        0 => Tone::Info,
        1 => Tone::Good,
        2 => Tone::Stop,
        3 => Tone::Ended,
        t => return Err(format!("tone {t}")),
    };
    let who = match w.who {
        0 => Who::Nobody,
        1 => Who::Customer,
        2 => Who::Cashier,
        3 => Who::Manager,
        4 => Who::Owner,
        5 => Who::KeyHolder,
        6 => Who::BoxKeeper,
        t => return Err(format!("who {t}")),
    };
    let words = Words {
        tone,
        heading: w.heading.clone(),
        what: w.what.clone(),
        next: w.next.clone(),
        who,
        code: w.code.as_deref().map(leak),
        at: w.at,
        quoted: w.quoted.as_ref().map(|(from, text)| Quoted {
            from: leak(from),
            text: text.clone(),
        }),
        detail: w.detail.clone(),
    };
    check(&words)
}

#[test]
fn the_words_of_this_layer_keep_the_rules_and_none_reads_like_another() {
    let all = all_words();
    assert!(all.len() > 30);
    let mut seen = BTreeSet::new();
    for (name, words) in &all {
        check(words).unwrap_or_else(|why| panic!("{name}: {why}"));
        assert!(
            seen.insert((words.heading.clone(), words.what.clone())),
            "{name} reads like another"
        );
    }
}

#[test]
fn a_rule_string_without_words_gets_the_words_of_its_kind() {
    for kind in [Kind::Refused, Kind::Store, Kind::State, Kind::Internal] {
        let own = words_for(kind, "a-rule-nobody-wrote-words-for");
        check(&own).unwrap_or_else(|why| panic!("{kind:?}: {why}"));
        assert_eq!(own.detail.as_deref(), Some("a-rule-nobody-wrote-words-for"));
    }
    // A known rule string in the wrong kind is not that rule's words.
    let own = words_for(Kind::Refused, "node-limit");
    assert_ne!(own.heading, words_for(Kind::State, "node-limit").heading);
}

/// The rule strings that the source passes to `refused_by`, `store_by` and
/// `wrong_state`.
fn rules_in_source() -> BTreeSet<String> {
    let dir = std::path::PathBuf::from(
        std::env::var_os("CARGO_MANIFEST_DIR")
            .expect("cargo sets CARGO_MANIFEST_DIR when it runs tests"),
    )
    .join("src");
    let mut out = BTreeSet::new();
    for entry in std::fs::read_dir(dir).expect("a directory") {
        let path = entry.expect("an entry").path();
        if path.extension().is_none_or(|e| e != "rs") || path.ends_with("say.rs") {
            continue;
        }
        let text = std::fs::read_to_string(&path).expect("readable");
        // Without the test modules.
        let text = text.split("\n#[cfg(test)]").next().expect("a first part");
        for call in ["refused_by(", "store_by(", "wrong_state("] {
            for (at, _) in text.match_indices(call) {
                let rest = text[at + call.len()..].trim_start();
                if let Some(rest) = rest.strip_prefix('"') {
                    if let Some(end) = rest.find('"') {
                        out.insert(rest[..end].to_string());
                    }
                }
            }
        }
    }
    out
}

#[test]
fn every_rule_string_the_source_makes_has_words_of_its_own() {
    let own: BTreeSet<String> = rule_strings().map(str::to_string).collect();
    for rule in rules_in_source() {
        assert!(own.contains(&rule), "{rule} has no words in say.rs");
    }
    // And no rule string of the table is left that nothing makes: each is
    // spelled somewhere in the code outside say.rs.
    let dir = std::path::PathBuf::from(
        std::env::var_os("CARGO_MANIFEST_DIR")
            .expect("cargo sets CARGO_MANIFEST_DIR when it runs tests"),
    )
    .join("src");
    let mut code = String::new();
    for name in [
        "boxnet.rs",
        "call.rs",
        "codec.rs",
        "error.rs",
        "host.rs",
        "jni.rs",
        "settings_edit.rs",
        "setup_lock.rs",
        "views.rs",
        "watch.rs",
    ] {
        let text = std::fs::read_to_string(dir.join(name)).expect("readable");
        code.push_str(text.split("\n#[cfg(test)]").next().expect("a first part"));
    }
    for rule in &own {
        assert!(
            code.contains(&format!("\"{rule}\"")),
            "{rule} is in say.rs and nothing makes it"
        );
    }
}

/// The lines of a file that are code: not blank, not a comment.
fn code_lines(text: &str) -> impl Iterator<Item = (usize, &str)> {
    text.lines().enumerate().filter_map(|(n, l)| {
        let t = l.trim_start();
        (!t.is_empty() && !t.starts_with("//")).then_some((n + 1, l))
    })
}

#[test]
fn the_only_unsafe_is_the_jni_glue_and_it_is_counted() {
    // The crate denies `unsafe`; `jni.rs` allows it for itself. No other
    // file may hold the word as code. The glue's own count is pinned: every
    // block is one JNI call or one read of the function table, and adding one
    // means changing the numbers below on purpose.
    let dir = std::path::PathBuf::from(
        std::env::var_os("CARGO_MANIFEST_DIR")
            .expect("cargo sets CARGO_MANIFEST_DIR when it runs tests"),
    )
    .join("src");
    let mut dirs = vec![dir];
    let mut seen = 0;
    while let Some(d) = dirs.pop() {
        for entry in std::fs::read_dir(d).expect("a directory") {
            let path = entry.expect("an entry").path();
            if path.is_dir() {
                dirs.push(path);
                continue;
            }
            if path.extension().is_none_or(|e| e != "rs") {
                continue;
            }
            seen += 1;
            let text = std::fs::read_to_string(&path).expect("readable");
            if path.ends_with("src/jni.rs") {
                // The glue, without its test module.
                let glue = text.split("\n#[cfg(test)]").next().expect("a first part");
                let blocks = code_lines(glue)
                    .filter(|(_, l)| l.contains("unsafe {"))
                    .count();
                let impls = code_lines(glue)
                    .filter(|(_, l)| l.contains("unsafe impl"))
                    .count();
                assert_eq!(blocks, 8, "unsafe blocks in jni.rs");
                assert_eq!(impls, 0, "unsafe impls in jni.rs");
                assert_eq!(
                    code_lines(glue)
                        .filter(|(_, l)| l.contains("unsafe"))
                        .count(),
                    blocks + impls + 1,
                    "only blocks, impls and the allow of the file"
                );
                continue;
            }
            // This file spells the word in its own checks.
            if path.ends_with("tests/screen.rs") {
                continue;
            }
            for (n, line) in code_lines(&text) {
                if line.contains("unsafe") && !line.contains("deny(unsafe_code)") {
                    panic!("unsafe at {}:{n}", path.display());
                }
            }
        }
    }
    assert!(seen >= 12);
}

#[test]
fn the_glue_keeps_no_reference_and_no_environment_between_calls() {
    // No global reference, no cached environment, no thread of its own.
    let glue = std::fs::read_to_string(
        std::path::PathBuf::from(
            std::env::var_os("CARGO_MANIFEST_DIR")
                .expect("cargo sets CARGO_MANIFEST_DIR when it runs tests"),
        )
        .join("src/jni.rs"),
    )
    .expect("readable");
    let glue = glue.split("\n#[cfg(test)]").next().expect("a first part");
    for word in [
        "NewGlobalRef",
        "NewWeakGlobalRef",
        "GetJavaVM",
        "thread::spawn",
    ] {
        assert!(
            code_lines(glue).all(|(_, l)| !l.contains(word)),
            "jni.rs uses {word}"
        );
    }
    // The one static is the registry.
    let statics: Vec<&str> = code_lines(glue)
        .map(|(_, l)| l)
        .filter(|l| l.trim_start().starts_with("static "))
        .collect();
    assert_eq!(statics.len(), 1, "{statics:?}");
    assert!(statics[0].contains("REGISTRY"));
}
