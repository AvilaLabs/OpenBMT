//! Tests of the owner's words for the books (SC-01, SC-05, SC-07, SC-12).

use super::*;
use openbmt_core::say::{check, check_distinct};

#[path = "../../openbmt-owner/src/screens_file.rs"]
mod screens_file;

fn chain_name(e: &ChainError) -> &'static str {
    match e {
        ChainError::UnknownAuthor { .. } => "UnknownAuthor",
        ChainError::BadSignature { .. } => "BadSignature",
        ChainError::SeqMismatch { .. } => "SeqMismatch",
        ChainError::PrevMismatch { .. } => "PrevMismatch",
        ChainError::BadCheckIn { .. } => "BadCheckIn",
        ChainError::Truncated { .. } => "Truncated",
        ChainError::Diverged { .. } => "Diverged",
        ChainError::BodyTooLarge => "BodyTooLarge",
        ChainError::NotACheckIn => "NotACheckIn",
    }
}

fn decode_name(e: &DecodeError) -> &'static str {
    match e {
        DecodeError::Truncated => "Truncated",
        DecodeError::TrailingBytes => "TrailingBytes",
        DecodeError::BadMagic => "BadMagic",
        DecodeError::UnsupportedVersion(_) => "UnsupportedVersion",
        DecodeError::TooLarge => "TooLarge",
        DecodeError::NonCanonical(_) => "NonCanonical",
        DecodeError::InvalidKey => "InvalidKey",
        DecodeError::UnknownKind(_) => "UnknownKind",
    }
}

fn books_name(e: &BooksError) -> String {
    match e {
        BooksError::CounterNotOnList { .. } => "BooksError::CounterNotOnList".to_string(),
        BooksError::DuplicateCounter { .. } => "BooksError::DuplicateCounter".to_string(),
        BooksError::BadChain { error, .. } => {
            format!("BooksError::BadChain/{}", chain_name(error))
        }
        BooksError::BadEvent { error, .. } => {
            format!("BooksError::BadEvent/{}", decode_name(error))
        }
        BooksError::KindMismatch { .. } => "BooksError::KindMismatch".to_string(),
        BooksError::UnknownSale { .. } => "BooksError::UnknownSale".to_string(),
        BooksError::OpenedTwice { .. } => "BooksError::OpenedTwice".to_string(),
        BooksError::TimeOutOfRange { .. } => "BooksError::TimeOutOfRange".to_string(),
        BooksError::SaleNotInBooks { .. } => "BooksError::SaleNotInBooks".to_string(),
        BooksError::Overflow => "BooksError::Overflow".to_string(),
        BooksError::TooMany => "BooksError::TooMany".to_string(),
        BooksError::BadOffset { .. } => "BooksError::BadOffset".to_string(),
    }
}

fn export_name(e: &ExportError, kind: ExportKind) -> String {
    let k = match kind {
        ExportKind::Accounting => "Accounting",
        ExportKind::FullLedger => "FullLedger",
    };
    let n = match e {
        ExportError::DisposalValueUnknown { .. } => "DisposalValueUnknown",
        ExportError::MixedCurrencies => "MixedCurrencies",
        ExportError::TimeOutOfRange { .. } => "TimeOutOfRange",
        ExportError::SaleMissing { .. } => "SaleMissing",
        ExportError::Overflow => "Overflow",
    };
    format!("ExportError::{n}/{k}")
}

fn seal_name(e: &SealError) -> &'static str {
    match e {
        SealError::EmptyPassphrase => "SealError::EmptyPassphrase",
        SealError::Randomness => "SealError::Randomness",
        SealError::Key => "SealError::Key",
        SealError::Cipher => "SealError::Cipher",
    }
}

fn open_name(e: &OpenError) -> &'static str {
    match e {
        OpenError::EmptyPassphrase => "OpenError::EmptyPassphrase",
        OpenError::WrongPassphraseOrDamaged => "OpenError::WrongPassphraseOrDamaged",
    }
}

#[test]
fn sc_12_every_word_is_checked_on_its_own() {
    let bad: Vec<String> = all_words()
        .iter()
        .filter_map(|(name, w)| check(w).err().map(|e| format!("{name}: {e}")))
        .collect();
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

#[test]
fn sc_12_every_word_keeps_the_rules() {
    assert_eq!(check_distinct(&all_words()), Ok(()));
}

#[test]
fn sc_01_no_variant_is_missing() {
    let names: Vec<String> = all_words().into_iter().map(|(n, _)| n).collect();
    let counter = sample_device().expect("a fixed key parses");
    let sale = crate::SaleKey { counter, index: 1 };
    let mut wanted: Vec<String> = Vec::new();
    let chains = [
        ChainError::UnknownAuthor { seq: 0 },
        ChainError::BadSignature { seq: 0 },
        ChainError::SeqMismatch {
            expected: 0,
            found: 1,
        },
        ChainError::PrevMismatch { seq: 0 },
        ChainError::BadCheckIn { seq: 0 },
        ChainError::Truncated { have: 0, need: 1 },
        ChainError::Diverged { seq: 0 },
        ChainError::BodyTooLarge,
        ChainError::NotACheckIn,
    ];
    let decodes = [
        DecodeError::Truncated,
        DecodeError::TrailingBytes,
        DecodeError::BadMagic,
        DecodeError::UnsupportedVersion(1),
        DecodeError::TooLarge,
        DecodeError::NonCanonical("x"),
        DecodeError::InvalidKey,
        DecodeError::UnknownKind(1),
    ];
    let mut books = vec![
        BooksError::CounterNotOnList { counter },
        BooksError::DuplicateCounter { counter },
        BooksError::KindMismatch { counter, seq: 0 },
        BooksError::UnknownSale {
            counter,
            seq: 0,
            index: 0,
        },
        BooksError::OpenedTwice {
            counter,
            seq: 0,
            index: 0,
        },
        BooksError::TimeOutOfRange { at: 0 },
        BooksError::SaleNotInBooks { sale },
        BooksError::Overflow,
        BooksError::TooMany,
        BooksError::BadOffset { minutes: 0 },
    ];
    books.extend(
        chains
            .iter()
            .map(|error| BooksError::BadChain {
                counter,
                error: error.clone(),
            }),
    );
    books.extend(decodes.iter().map(|error| BooksError::BadEvent {
        counter,
        seq: 0,
        error: error.clone(),
    }));
    wanted.extend(books.iter().map(books_name));
    for kind in [ExportKind::Accounting, ExportKind::FullLedger] {
        for e in [
            ExportError::DisposalValueUnknown { position: 0 },
            ExportError::MixedCurrencies,
            ExportError::TimeOutOfRange { at: 0 },
            ExportError::SaleMissing { sale },
            ExportError::Overflow,
        ] {
            wanted.push(export_name(&e, kind));
        }
    }
    for e in [
        SealError::EmptyPassphrase,
        SealError::Randomness,
        SealError::Key,
        SealError::Cipher,
    ] {
        wanted.push(seal_name(&e).to_string());
    }
    for e in [OpenError::EmptyPassphrase, OpenError::WrongPassphraseOrDamaged] {
        wanted.push(open_name(&e).to_string());
    }
    for name in &wanted {
        assert!(names.contains(name), "{name} has no words");
    }
    for name in &names {
        assert!(wanted.contains(name), "{name} is not a variant");
    }
}

#[test]
fn sc_05_someone_is_named() {
    for (name, w) in all_words() {
        assert!(
            !(w.who == Who::Nobody && w.tone == Tone::Stop),
            "{name} stops and names nobody"
        );
    }
}

#[test]
fn sc_03_the_counter_is_named_by_its_name_and_the_record_in_the_details() {
    let counter = sample_device().expect("a fixed key parses");
    let w = BooksError::KindMismatch { counter, seq: 12 }.words("Corner");
    assert!(w.what.contains("\"Corner\""));
    assert!(w.detail.is_some_and(|d| d.contains("12")));
    assert!(!w.what.contains("12"));
    // Every refusal says that no file was written (or that nothing changed).
    for (name, w) in all_words() {
        assert!(
            w.next.starts_with("No file was written") || w.next.starts_with("Nothing was changed"),
            "{name} does not say whether a file was written: {}",
            w.next
        );
    }
}

#[test]
fn sc_07_the_two_time_refusals_read_differently() {
    let a = BooksError::TimeOutOfRange { at: 1 }.words("Corner");
    let b = ExportError::TimeOutOfRange { at: 1 }.words(ExportKind::FullLedger, "Corner");
    assert_ne!((a.heading, a.what), (b.heading, b.what));
}

#[test]
fn sc_12_the_list_on_disk_is_this_one() {
    screens_file::check_or_write("books", &screens_markdown());
}
