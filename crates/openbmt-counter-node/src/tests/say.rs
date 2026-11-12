//! SC-01 to SC-12 for the words of the counter program.

use openbmt_core::pairing::PairError;
use openbmt_core::say::{check, check_distinct, Tone, Who, Words};
use openbmt_counter::bill_say::is_placeholder;

use crate::paired::PairRefused;
use crate::relaylink::LinkLine;
use crate::say::{
    all_words, every_link_line, every_pair_error, every_pair_refused, every_relay_refused,
    program_words, screens_markdown, PairStep,
};
use crate::RelayRefused;

/// The name of a pairing error, by an exhaustive match: a new variant does
/// not compile until it is named here, and the test then wants its words.
fn pair_error_name(e: &PairError) -> &'static str {
    match e {
        PairError::SameRoot => "SameRoot",
        PairError::CounterIsRoot => "CounterIsRoot",
        PairError::InvalidName => "InvalidName",
        PairError::UnsupportedNetwork => "UnsupportedNetwork",
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

/// The same for the refusals of the pairing steps. `Pair` carries a pairing
/// error, whose words are listed under `PairError`.
fn pair_refused_name(e: &PairRefused) -> &'static str {
    match e {
        PairRefused::NoCeremony => "NoCeremony",
        PairRefused::NoRandomness => "NoRandomness",
        PairRefused::Pair(_) => "Pair",
        PairRefused::Bundle(_) => "Bundle",
        PairRefused::Settings(_) => "Settings",
        PairRefused::SettingsOld { .. } => "SettingsOld",
        PairRefused::RelayNotSigned => "RelayNotSigned",
        PairRefused::NotOnDevices => "NotOnDevices",
        PairRefused::NotOnCounters => "NotOnCounters",
        PairRefused::Batch(_) => "Batch",
        PairRefused::Record(_) => "Record",
        PairRefused::RecordKey => "RecordKey",
    }
}

/// The same for the lines of the link: a new variant does not compile until
/// it is named here.
fn link_line_name(l: &LinkLine) -> &'static str {
    match l {
        LinkLine::Connecting => "Connecting",
        LinkLine::Connected => "Connected",
        LinkLine::NotReachable { .. } => "NotReachable",
        LinkLine::Refused => "Refused",
        LinkLine::NeedsTor => "NeedsTor",
        LinkLine::NotStarted => "NotStarted",
    }
}

fn relay_refused_name(e: &RelayRefused) -> &'static str {
    match e {
        RelayRefused::Bytes(_) => "Bytes",
        RelayRefused::NotFromOwner => "NotFromOwner",
    }
}

#[test]
fn cr_136_every_line_of_the_link_and_every_refused_relay_has_words() {
    let all = all_words();
    let has = |name: String| all.iter().any(|(n, _)| *n == name);
    let lines = every_link_line();
    assert_eq!(lines.len(), 7);
    for (name, line) in &lines {
        let base = link_line_name(line);
        assert!(*name == base || *name == "NotReachableNoTime");
        assert!(has(format!("LinkLine::{base}")), "{name}");
        assert_eq!(line.words().code, Some("CR-136"), "{name}");
        assert_eq!(check(&line.words()), Ok(()), "{name}");
    }
    // The time of the failures is the time of the words, and they read the
    // same without it.
    let timed = LinkLine::NotReachable { since: Some(5) }.words();
    assert_eq!(timed.at, Some(5));
    assert_eq!(LinkLine::NotReachable { since: None }.words().at, None);
    // The link never stops a sale: no line is red, and none says to stop.
    for (name, line) in &lines {
        assert_ne!(line.words().tone, Tone::Stop, "{name}");
    }
    let refusals = every_relay_refused();
    assert_eq!(refusals.len(), 2);
    for (name, e) in &refusals {
        assert_eq!(relay_refused_name(e), *name);
        assert!(has(format!("RelayRefused::{name}")), "{name}");
        assert_eq!(e.words().code, Some("CR-132"), "{name}");
    }
}

#[test]
fn sc_12_every_word_keeps_the_rules() {
    // CR-255: the words of bills are placeholders until they are written;
    // the counter's own test checks them.
    let written = |all: Vec<(String, Words)>| -> Vec<(String, Words)> {
        all.into_iter()
            .filter(|(_, w)| !is_placeholder(w))
            .collect()
    };
    assert_eq!(check_distinct(&written(all_words())), Ok(()));
    assert_eq!(check_distinct(&written(program_words())), Ok(()));
}

#[test]
fn sc_01_no_variant_is_missing() {
    let all = all_words();
    let has = |name: String| all.iter().any(|(n, _)| *n == name);
    let pair = every_pair_error();
    assert_eq!(pair.len(), 18);
    for (name, e) in &pair {
        assert_eq!(pair_error_name(e), *name);
        assert!(has(format!("PairError::{name}")));
    }
    for step in PairStep::ALL {
        assert!(has(format!("PairStep::{}", step.name())));
    }
    // Every refusal of a pairing step has words, except the one that wraps
    // a pairing error, which has the words of that error with its rule.
    let refused = every_pair_refused();
    assert_eq!(refused.len(), 11);
    for (name, e) in &refused {
        assert_eq!(pair_refused_name(e), *name);
        assert!(has(format!("PairRefused::{name}")));
    }
    let wrapped = PairRefused::Pair(PairError::Expired);
    assert_eq!(pair_refused_name(&wrapped), "Pair");
    assert_eq!(wrapped.words().code, Some("CR-92"));
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
    let path = &std::path::PathBuf::from(
        std::env::var_os("CARGO_MANIFEST_DIR")
            .expect("cargo sets CARGO_MANIFEST_DIR when it runs tests"),
    )
    .join("../../docs/screens/counter.md");
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

#[test]
fn cr_92_they_differ_says_what_happened_and_how_to_start_again() {
    let w = PairStep::Differ.words();
    assert_eq!(w.tone, Tone::Stop);
    assert_eq!(w.who, Who::Owner);
    assert!(w.what.contains("forgot its key"));
    assert!(w.next.contains("Start again from the Key app"));
    assert_eq!(check(&w), Ok(()));
}

#[test]
fn cr_91_a_lost_pairing_says_to_start_again_from_the_key_app() {
    let w = PairRefused::NoCeremony.words();
    assert!(w.what.contains("restarted"));
    assert!(w.next.contains("Start again from the Key app"));
    assert_eq!(check(&w), Ok(()));
}

#[test]
fn sc_01_every_lightning_state_has_words_for_the_right_person() {
    use crate::{LightningOff, LnTransport, SetupRefused};
    let all = all_words();
    let has = |name: String| all.iter().any(|(n, _)| *n == name);
    for t in LnTransport::ALL {
        assert!(has(format!("LnTransport::{}", t.name())), "{}", t.name());
        assert_eq!(t.words().who, Who::Owner);
        assert_eq!(t.sale_words().who, Who::Cashier);
        assert_eq!(check(&t.sale_words()), Ok(()));
    }
    for off in LightningOff::ALL {
        assert!(has(format!("LightningOff::{}", off.name())));
        assert_eq!(off.words().who, Who::Owner);
    }
    for name in ["Ready", "Unavailable", "NotReached", "KeyRefused"] {
        assert!(has(format!("Lightning::{name}")), "{name}");
    }
    assert!(has("SetupRefused::NotNamed".to_string()));
    assert_eq!(SetupRefused::NotNamed.rule(), "CR-115");
}
