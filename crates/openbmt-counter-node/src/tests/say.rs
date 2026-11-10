//! SC-01 to SC-12 for the words of the counter program.

use openbmt_core::pairing::PairError;
use openbmt_core::say::{check, check_distinct, Tone, Who};

use crate::say::{all_words, every_pair_error, program_words, screens_markdown, PairStep};

/// The name of a pairing error, by an exhaustive match: a new variant does
/// not compile until it is named here, and the test then wants its words.
fn pair_error_name(e: &PairError) -> &'static str {
    match e {
        PairError::SameRoot => "SameRoot",
        PairError::CounterIsRoot => "CounterIsRoot",
        PairError::InvalidName => "InvalidName",
        PairError::Decode(_) => "Decode",
        PairError::BindingMismatch => "BindingMismatch",
        PairError::RequestSignatureInvalid => "RequestSignatureInvalid",
        PairError::NotConfirmed => "NotConfirmed",
        PairError::ClockWentBack => "ClockWentBack",
        PairError::Expired => "Expired",
        PairError::WrongWallet => "WrongWallet",
        PairError::WrongCounter => "WrongCounter",
        PairError::WrongNonce => "WrongNonce",
        PairError::WrongName => "WrongName",
        PairError::ApproverSignatureInvalid => "ApproverSignatureInvalid",
        PairError::OwnerSignatureInvalid => "OwnerSignatureInvalid",
        PairError::NotTheApprover => "NotTheApprover",
        PairError::NotTheOwner => "NotTheOwner",
    }
}

#[test]
fn sc_12_every_word_keeps_the_rules() {
    assert_eq!(check_distinct(&all_words()), Ok(()));
    assert_eq!(check_distinct(&program_words()), Ok(()));
}

#[test]
fn sc_01_no_variant_is_missing() {
    let all = all_words();
    let has = |name: String| all.iter().any(|(n, _)| *n == name);
    let pair = every_pair_error();
    assert_eq!(pair.len(), 17);
    for (name, e) in &pair {
        assert_eq!(pair_error_name(e), *name);
        assert!(has(format!("PairError::{name}")));
    }
    for step in PairStep::ALL {
        assert!(has(format!("PairStep::{}", step.name())));
    }
    for name in [
        "Locked",
        "Headers",
        "Counter",
        "FirstBatch",
        "Checked",
        "Driver",
    ] {
        assert!(has(format!("StartError::{name}")), "{name}");
    }
    for name in ["Io", "Corrupt", "RecordTooLarge"] {
        assert!(has(format!("StartError::Dir/{name}")), "{name}");
    }
    for name in ["Bytes", "Refused"] {
        assert!(has(format!("HeartbeatNotTaken::{name}")), "{name}");
    }
}

#[test]
fn sc_05_someone_is_named() {
    for (name, w) in program_words() {
        assert!(
            !(w.who == Who::Nobody && w.tone == Tone::Stop),
            "{name} stops and names nobody"
        );
    }
}

#[test]
fn sc_02_every_start_error_says_the_counter_did_not_start() {
    for (name, w) in all_words() {
        if name.starts_with("StartError::") {
            assert_eq!(w.tone, Tone::Stop, "{name}");
            assert_eq!(w.who, Who::Owner, "{name}");
            assert_eq!(check(&w), Ok(()), "{name}");
        }
    }
}

#[test]
fn sc_12_the_list_on_disk_is_current() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/screens/counter.md");
    let want = screens_markdown();
    if std::env::var_os("OPENBMT_WRITE_SCREENS").is_some() {
        std::fs::write(path, &want).expect("the list can be written");
        return;
    }
    let have = std::fs::read_to_string(path).unwrap_or_default();
    assert!(
        have == want,
        "docs/screens/counter.md is out of date; write it again with: \
         OPENBMT_WRITE_SCREENS=1 cargo test -p openbmt-counter-node \
         sc_12_the_list_on_disk_is_current"
    );
}

#[test]
fn sc_12_the_list_keeps_its_lines_short_outside_the_table() {
    for line in screens_markdown().lines() {
        if !line.starts_with('|') {
            assert!(line.chars().count() <= 80, "{line}");
        }
    }
}
