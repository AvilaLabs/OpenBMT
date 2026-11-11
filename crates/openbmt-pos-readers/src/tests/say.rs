//! SC-01 to SC-12 for the words of this crate.

use openbmt_core::say::{check_distinct, Tone, Who};
use openbmt_pos::PosError;

use crate::say::{all_words, read_name, request_name, screens_markdown, ALL_READ, ALL_REQUEST};
use crate::{ReadRefused, RequestRefused};

#[test]
fn sc_12_every_word_passes_the_check_and_none_repeats() {
    check_distinct(&all_words()).expect("the words pass");
}

/// Written out again, with no catch-all arm, so that a new variant stops
/// this from compiling until it has a value in the list.
fn every_read_variant_has_a_value(r: &ReadRefused) -> bool {
    match r {
        ReadRefused::TooLong
        | ReadRefused::TooDeep
        | ReadRefused::TooManyValues
        | ReadRefused::NotUtf8
        | ReadRefused::Malformed
        | ReadRefused::TrailingBytes
        | ReadRefused::DuplicateKey
        | ReadRefused::ServiceError
        | ReadRefused::Missing(_)
        | ReadRefused::WrongType(_)
        | ReadRefused::UnknownValue(_)
        | ReadRefused::UnknownResult(_)
        | ReadRefused::OutOfRange(_)
        | ReadRefused::BadTime(_)
        | ReadRefused::BadCurrency(_)
        | ReadRefused::CurrencyMismatch
        | ReadRefused::BadId(_)
        | ReadRefused::BadText(_)
        | ReadRefused::TooManyOrders
        | ReadRefused::TooManyTenders
        | ReadRefused::DuplicateTender
        | ReadRefused::RefundTooLarge
        | ReadRefused::RefundWithoutTender
        | ReadRefused::CursorTooLong => ALL_READ
            .iter()
            .any(|v| std::mem::discriminant(v) == std::mem::discriminant(r)),
    }
}

#[test]
fn sc_12_a_variant_missing_from_the_list_fails() {
    let all = all_words();
    for r in ALL_READ {
        assert!(every_read_variant_has_a_value(&r));
        assert!(all
            .iter()
            .any(|(n, _)| *n == format!("ReadRefused::{}", read_name(&r))));
    }
    for r in ALL_REQUEST {
        let name = match r {
            RequestRefused::NoLocations
            | RequestRefused::BadLimit
            | RequestRefused::BadCursor
            | RequestRefused::BadMerchant => request_name(&r),
        };
        assert!(all
            .iter()
            .any(|(n, _)| *n == format!("RequestRefused::{name}")));
    }
    assert_eq!(all.len(), ALL_READ.len() + ALL_REQUEST.len());
    // No two in the list are the same variant.
    for (i, a) in ALL_READ.iter().enumerate() {
        for b in ALL_READ.iter().skip(i + 1) {
            assert_ne!(std::mem::discriminant(a), std::mem::discriminant(b));
        }
    }
}

#[test]
fn sc_05_a_refused_reply_is_for_the_owner_and_stops_with_nothing_changed() {
    for r in ALL_READ {
        let w = r.words();
        assert_eq!(w.who, Who::Owner, "{r}");
        assert_eq!(w.tone, Tone::Stop, "{r}");
        assert!(w.next.starts_with("Nothing was changed."), "{r}");
        assert_eq!(w.code, Some(r.rule()), "{r}");
        assert!(w.detail.is_some_and(|d| d.starts_with(r.rule())), "{r}");
    }
    for r in ALL_REQUEST {
        let w = r.words();
        assert_eq!(w.who, Who::Owner);
        assert_eq!(w.code, Some(r.rule()));
    }
}

#[test]
fn sc_03_the_name_of_a_field_stays_out_of_the_sentences() {
    let r = ReadRefused::Missing("orders[].total_money");
    let w = r.words();
    for text in [&w.heading, &w.what, &w.next] {
        assert!(!text.contains("total_money") && !text.contains("orders"));
    }
    assert!(w.detail.is_some_and(|d| d.contains("orders[].total_money")));
}

#[test]
fn sc_07_the_same_cause_reads_the_same_and_different_causes_differ() {
    // Fields that differ only in name read the same; the field is in the
    // details.
    assert_eq!(
        ReadRefused::Missing("a").words().what,
        ReadRefused::Missing("b").words().what
    );
    assert_ne!(
        ReadRefused::Missing("a").words().heading,
        ReadRefused::WrongType("a").words().heading
    );
}

#[test]
fn ps_80_a_refused_reply_is_an_unreadable_answer_for_the_connector() {
    for r in ALL_READ {
        match PosError::from(r) {
            PosError::BadResponse(why) => assert!(!why.is_empty()),
            other => panic!("{other:?}"),
        }
    }
    assert!(matches!(
        PosError::from(RequestRefused::BadLimit),
        PosError::BadResponse(_)
    ));
}

#[test]
fn sc_12_the_list_is_on_disk() {
    let path = &std::path::PathBuf::from(
        std::env::var_os("CARGO_MANIFEST_DIR")
            .expect("cargo sets CARGO_MANIFEST_DIR when it runs tests"),
    )
    .join("../../docs/screens/pos-readers.md");
    let want = screens_markdown();
    if std::env::var_os("OPENBMT_WRITE_SCREENS").is_some() {
        if let Some(dir) = std::path::Path::new(path).parent() {
            std::fs::create_dir_all(dir).expect("make docs/screens");
        }
        std::fs::write(path, &want).expect("write docs/screens/pos-readers.md");
    }
    let have = std::fs::read_to_string(path).unwrap_or_default();
    assert!(
        have == want,
        "docs/screens/pos-readers.md is not what screens_markdown makes: run this test with \
         OPENBMT_WRITE_SCREENS=1 to write it again"
    );
}
