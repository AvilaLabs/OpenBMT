//! Ties the rules to the tests: every rule in `docs/BOOKS-RULES.md` has a
//! test named for it, every test names a rule, every rule the code names
//! exists, every error says its rule, and the rules file keeps its line
//! length.

use std::collections::BTreeSet;

use openbmt_core::identity::DeviceId;
use openbmt_core::records::ChainError;
use openbmt_core::wire::DecodeError;

use super::fixture::*;
use crate::{BooksError, ExportError, OpenError, SaleKey, SealError};

const RULES: &str = include_str!("../../../../docs/BOOKS-RULES.md");

const TEST_SOURCES: [&str; 10] = [
    include_str!("input.rs"),
    include_str!("lightning.rs"),
    include_str!("sale.rs"),
    include_str!("records.rs"),
    include_str!("real.rs"),
    include_str!("outflow.rs"),
    include_str!("totals.rs"),
    include_str!("export.rs"),
    include_str!("sealed.rs"),
    include_str!("staff.rs"),
];
const MORE_TEST_SOURCES: [&str; 1] = [include_str!("hostile.rs")];

fn defined() -> BTreeSet<String> {
    RULES
        .lines()
        .filter_map(|l| l.strip_prefix("- **BK-"))
        .map(|rest| format!("BK-{}", &rest[..2]))
        .collect()
}

fn sources() -> String {
    format!("{}{}", TEST_SOURCES.concat(), MORE_TEST_SOURCES.concat())
}

#[test]
fn the_rules_file_defines_its_rules() {
    let ids = defined();
    assert_eq!(ids.len(), 37, "{ids:?}");
    assert!(ids.contains("BK-01") && ids.contains("BK-37"));
    let numbers: Vec<u32> = ids
        .iter()
        .map(|i| i[3..].parse().expect("a number"))
        .collect();
    assert_eq!(numbers, (1..=37).collect::<Vec<u32>>());
}

#[test]
fn every_rule_has_a_test_named_for_it() {
    let sources = sources();
    for id in defined() {
        let prefix = format!("fn bk_{}_", &id[3..]);
        assert!(sources.contains(&prefix), "{id} has no test");
    }
}

#[test]
fn every_test_names_a_rule_that_exists() {
    let ids = defined();
    let mut seen = 0;
    for line in sources().lines() {
        if let Some(rest) = line.trim().strip_prefix("fn bk_") {
            let id = format!("BK-{}", &rest[..2]);
            assert!(ids.contains(&id), "test {line} names {id}");
            seen += 1;
        }
    }
    assert!(seen > 50, "only {seen} tests");
}

#[test]
fn every_rule_the_code_names_exists() {
    let ids = defined();
    let mut seen = 0;
    for file in [
        include_str!("../books.rs"),
        include_str!("../bundle.rs"),
        include_str!("../csv.rs"),
        include_str!("../date.rs"),
        include_str!("../error.rs"),
        include_str!("../export.rs"),
        include_str!("../fsio.rs"),
        include_str!("../lib.rs"),
        include_str!("../model.rs"),
        include_str!("../money.rs"),
        include_str!("../sealed.rs"),
        include_str!("../value.rs"),
    ] {
        for (at, _) in file.match_indices("BK-") {
            let id = &file[at..at + 5];
            assert!(ids.contains(id), "the code names {id}");
            seen += 1;
        }
    }
    assert!(seen > 40, "only {seen} rule names in the code");
}

#[test]
fn the_rules_file_keeps_its_lines_short_outside_tables() {
    for (n, line) in RULES.lines().enumerate() {
        if line.starts_with('|') {
            continue;
        }
        assert!(
            line.chars().count() <= 80,
            "line {} is {} characters",
            n + 1,
            line.chars().count()
        );
    }
}

#[test]
fn the_rules_say_the_export_is_not_tax_advice_and_covers_us_rules_only() {
    assert!(RULES.contains("not tax advice") && RULES.contains("US rules only"));
    assert!(RULES.contains("## 10. What these rules do not do"));
}

#[test]
fn the_stored_magics_differ_and_the_rules_name_them() {
    assert_ne!(crate::MAGIC_BUNDLE, crate::MAGIC_EXPORT);
    assert_eq!(&crate::MAGIC_BUNDLE, b"OBM6");
    assert_eq!(&crate::MAGIC_EXPORT, b"OBMY");
    for magic in ["OBM6", "OBMY"] {
        assert!(RULES.contains(magic));
    }
}

fn some_key() -> DeviceId {
    identity(1).device_id()
}

#[test]
fn every_error_says_which_rule_in_words() {
    let c = some_key();
    let key = SaleKey {
        counter: c,
        index: 1,
    };
    let texts: Vec<String> = vec![
        BooksError::CounterNotOnList { counter: c }.to_string(),
        BooksError::DuplicateCounter { counter: c }.to_string(),
        BooksError::BadChain {
            counter: c,
            error: ChainError::BadSignature { seq: 2 },
        }
        .to_string(),
        BooksError::BadEvent {
            counter: c,
            seq: 1,
            error: DecodeError::Truncated,
        }
        .to_string(),
        BooksError::KindMismatch { counter: c, seq: 1 }.to_string(),
        BooksError::UnknownSale {
            counter: c,
            seq: 1,
            index: 2,
        }
        .to_string(),
        BooksError::OpenedTwice {
            counter: c,
            seq: 1,
            index: 2,
        }
        .to_string(),
        BooksError::TimeOutOfRange { at: 1 }.to_string(),
        BooksError::SaleNotInBooks { sale: key }.to_string(),
        BooksError::Overflow.to_string(),
        BooksError::TooMany.to_string(),
        BooksError::BadOffset { minutes: 1 }.to_string(),
        ExportError::DisposalValueUnknown { position: 1 }.to_string(),
        ExportError::MixedCurrencies.to_string(),
        ExportError::TimeOutOfRange { at: 1 }.to_string(),
        ExportError::SaleMissing { sale: key }.to_string(),
        ExportError::Overflow.to_string(),
        SealError::EmptyPassphrase.to_string(),
        SealError::Randomness.to_string(),
        SealError::Key.to_string(),
        SealError::Cipher.to_string(),
        OpenError::EmptyPassphrase.to_string(),
        OpenError::WrongPassphraseOrDamaged.to_string(),
    ];
    let ids = defined();
    for t in texts {
        let id = &t[..5];
        assert!(t.starts_with("BK-") && ids.contains(id), "{t}");
        assert!(t.len() > 25, "{t}");
    }
}
