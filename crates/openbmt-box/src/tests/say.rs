//! SC-01 to SC-12 for the Box console (`docs/SCREEN-RULES.md`).

use std::collections::BTreeSet;
use std::io;

use openbmt_core::boxpair::InviteTextError;
use openbmt_core::say::{check_distinct, Tone, Who};
use openbmt_core::settings::SettingsError;
use openbmt_courier::StartError;
use openbmt_net_io::env::LockError;

use crate::runner::StartFailure;
use crate::say::{
    all_words, invite_lines, is_placeholder, iso_utc, members_lines, note_words, notes_lines,
    render, screens_markdown, state_lines, unknown_lines, ConsoleAnswer, NoteKind, PageAnswer,
};
use crate::state::{BoxError, BoxStatus};

/// The variant names the list covers: each entry name is split at " and "
/// and at ": ".
fn covered() -> BTreeSet<String> {
    all_words()
        .iter()
        .flat_map(|(name, _)| {
            name.split(" and ")
                .map(|part| part.split(": ").next().unwrap_or("").to_string())
                .collect::<Vec<_>>()
        })
        .collect()
}

#[test]
fn sc_12_every_word_keeps_the_rules() {
    // A placeholder is not a sentence and is not read as one, but each stands
    // for one state, so each differs from every other.
    let (placeholders, written): (Vec<_>, Vec<_>) = all_words()
        .into_iter()
        .partition(|(_, w)| is_placeholder(w));
    let mut seen = BTreeSet::new();
    for (name, w) in &placeholders {
        assert!(
            seen.insert(w.heading.clone()),
            "{name} repeats a placeholder"
        );
        assert_eq!(w.heading, w.what, "{name}");
        assert_eq!(w.heading, w.next, "{name}");
    }
    assert_eq!(
        seen.into_iter().collect::<Vec<String>>(),
        Vec::<String>::new()
    );
    assert_eq!(check_distinct(&written), Ok(()));
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

fn status_name(s: BoxStatus) -> &'static str {
    match s {
        BoxStatus::Unpaired => "BoxStatus::Unpaired",
        BoxStatus::InvitationOpen => "BoxStatus::InvitationOpen",
        BoxStatus::Paired => "BoxStatus::Paired",
        BoxStatus::TurnedOff => "BoxStatus::TurnedOff",
    }
}

fn error_name(e: &BoxError) -> &'static str {
    match e {
        BoxError::Damaged { .. } => "BoxError::Damaged",
        BoxError::Io { .. } => "BoxError::Io",
        BoxError::NoRandomness => "BoxError::NoRandomness",
        BoxError::AlreadyPaired => "BoxError::AlreadyPaired",
        BoxError::Invitation(_) => "BoxError::Invitation",
        BoxError::NoInvitation => "BoxError::NoInvitation",
        BoxError::InvitationExpired => "BoxError::InvitationExpired",
        BoxError::BadWelcome(_) => "BoxError::BadWelcome",
        BoxError::WelcomeMac => "BoxError::WelcomeMac",
        BoxError::BadSettings(_) => "BoxError::BadSettings",
        BoxError::Settings(_) => "BoxError::Settings",
        BoxError::Confirmed(_) => "BoxError::Confirmed",
        BoxError::NotListed => "BoxError::NotListed",
        BoxError::TooManyMembers => "BoxError::TooManyMembers",
        BoxError::NotPaired => "BoxError::NotPaired",
        BoxError::TurnedOff => "BoxError::TurnedOff",
    }
}

fn invite_name(e: &InviteTextError) -> &'static str {
    match e {
        InviteTextError::BadLength(_) => "InviteTextError::BadLength",
        InviteTextError::BadCharacter(_) => "InviteTextError::BadCharacter",
        InviteTextError::BadCheck => "InviteTextError::BadCheck",
    }
}

fn settings_name(e: &SettingsError) -> &'static str {
    match e {
        SettingsError::OwnerSignatureInvalid => "SettingsError::OwnerSignatureInvalid",
        SettingsError::ApproverRequired => "SettingsError::ApproverRequired",
        SettingsError::ApproverSignatureInvalid => "SettingsError::ApproverSignatureInvalid",
        SettingsError::StaleVersion { .. } => "SettingsError::StaleVersion",
        SettingsError::Invalid(_) => "SettingsError::Invalid",
        SettingsError::ConfirmationRequired(_) => "SettingsError::ConfirmationRequired",
        SettingsError::RedirectAtPairing => "SettingsError::RedirectAtPairing",
        SettingsError::WalletSigNotWanted => "SettingsError::WalletSigNotWanted",
        SettingsError::RedirectStarter => "SettingsError::RedirectStarter",
        SettingsError::WalletSig(_) => "SettingsError::WalletSig",
    }
}

fn start_name(e: &StartFailure) -> &'static str {
    match e {
        StartFailure::Lock(_) => "StartFailure::Lock",
        StartFailure::State(_) => "StartFailure::State",
        StartFailure::Relay(_) => "StartFailure::Relay",
        StartFailure::Courier(_) => "StartFailure::Courier",
        StartFailure::Listen(_) => "StartFailure::Listen",
    }
}

fn lock_name(e: &LockError) -> &'static str {
    match e {
        LockError::Busy(_) => "LockError::Busy",
        LockError::Io(_) => "LockError::Io",
    }
}

fn courier_name(e: &StartError) -> &'static str {
    match e {
        StartError::NeedsNewPairing { .. } => "StartError::NeedsNewPairing",
        StartError::Corrupt { .. } => "StartError::Corrupt",
        StartError::Store(_) => "StartError::Store",
    }
}

fn answer_name(a: &ConsoleAnswer) -> &'static str {
    match a {
        ConsoleAnswer::InvitationTaken => "ConsoleAnswer::InvitationTaken",
        ConsoleAnswer::Members(_) => "ConsoleAnswer::Members",
        ConsoleAnswer::NoNotes => "ConsoleAnswer::NoNotes",
        ConsoleAnswer::UnknownCommand(_) => "ConsoleAnswer::UnknownCommand",
        ConsoleAnswer::InvitationFileUnreadable { .. } => "ConsoleAnswer::InvitationFileUnreadable",
        ConsoleAnswer::BadArguments => "ConsoleAnswer::BadArguments",
        ConsoleAnswer::BadAddress(_) => "ConsoleAnswer::BadAddress",
    }
}

fn kind_name(k: NoteKind) -> &'static str {
    match k {
        NoteKind::MemberListNotReplaced => "NoteKind::MemberListNotReplaced",
        NoteKind::LinkFiles => "NoteKind::LinkFiles",
        NoteKind::MessageNotHandled => "NoteKind::MessageNotHandled",
        NoteKind::SettingsRefused => "NoteKind::SettingsRefused",
        NoteKind::TurnedOff => "NoteKind::TurnedOff",
        NoteKind::Other => "NoteKind::Other",
    }
}

/// One value of every variant of every enum, so that the functions above
/// stay exhaustive, and every name must be in the list.
#[test]
fn sc_01_no_variant_is_missing() {
    use crate::state::KEY_FILE;
    use openbmt_core::wire::DecodeError;
    let covered = covered();
    let need = |name: &str| assert!(covered.contains(name), "no words for {name}");
    for s in [
        BoxStatus::Unpaired,
        BoxStatus::InvitationOpen,
        BoxStatus::Paired,
        BoxStatus::TurnedOff,
    ] {
        need(status_name(s));
    }
    let errors = [
        BoxError::Damaged {
            file: KEY_FILE,
            why: "missing",
        },
        BoxError::Io {
            file: KEY_FILE,
            op: "read",
            kind: io::ErrorKind::NotFound,
        },
        BoxError::NoRandomness,
        BoxError::AlreadyPaired,
        BoxError::Invitation(InviteTextError::BadCheck),
        BoxError::NoInvitation,
        BoxError::InvitationExpired,
        BoxError::BadWelcome(DecodeError::Truncated),
        BoxError::WelcomeMac,
        BoxError::BadSettings(DecodeError::Truncated),
        BoxError::Settings(SettingsError::ApproverRequired),
        BoxError::Confirmed(openbmt_core::confirmed::ConfirmedError::Confirmation(
            openbmt_core::approval::ConfirmError::NotACounter,
        )),
        BoxError::NotListed,
        BoxError::TooManyMembers,
        BoxError::NotPaired,
        BoxError::TurnedOff,
    ];
    for e in &errors {
        need(error_name(e));
    }
    for e in [
        InviteTextError::BadLength(1),
        InviteTextError::BadCharacter('u'),
        InviteTextError::BadCheck,
    ] {
        need(invite_name(&e));
    }
    for e in [
        SettingsError::OwnerSignatureInvalid,
        SettingsError::ApproverRequired,
        SettingsError::ApproverSignatureInvalid,
        SettingsError::StaleVersion { current: 2, got: 1 },
        SettingsError::Invalid("x"),
    ] {
        need(settings_name(&e));
    }
    for e in [
        StartFailure::Lock(LockError::Io(String::new())),
        StartFailure::State(BoxError::NoRandomness),
        StartFailure::Relay(String::new()),
        StartFailure::Courier(StartError::Corrupt { file: "x" }),
        StartFailure::Listen(io::Error::from(io::ErrorKind::AddrInUse)),
    ] {
        need(start_name(&e));
    }
    for e in [LockError::Busy("x".into()), LockError::Io(String::new())] {
        need(lock_name(&e));
    }
    for e in [
        StartError::NeedsNewPairing { file: "x" },
        StartError::Corrupt { file: "x" },
        StartError::Store(openbmt_courier::StoreError {
            file: "x",
            op: "read",
            kind: io::ErrorKind::NotFound,
        }),
    ] {
        need(courier_name(&e));
    }
    for a in [
        ConsoleAnswer::InvitationTaken,
        ConsoleAnswer::Members(0),
        ConsoleAnswer::NoNotes,
        ConsoleAnswer::UnknownCommand(String::new()),
        ConsoleAnswer::InvitationFileUnreadable {
            path: String::new(),
            kind: io::ErrorKind::NotFound,
        },
        ConsoleAnswer::BadArguments,
        ConsoleAnswer::BadAddress(String::new()),
    ] {
        need(answer_name(&a));
    }
    for k in [
        NoteKind::MemberListNotReplaced,
        NoteKind::LinkFiles,
        NoteKind::MessageNotHandled,
        NoteKind::SettingsRefused,
        NoteKind::TurnedOff,
        NoteKind::Other,
    ] {
        need(kind_name(k));
    }
    for a in [
        PageAnswer::InvitationHeld,
        PageAnswer::NotFromThisPage,
        PageAnswer::SetupWrong,
    ] {
        need(match a {
            PageAnswer::InvitationHeld => "PageAnswer::InvitationHeld",
            PageAnswer::NotFromThisPage => "PageAnswer::NotFromThisPage",
            PageAnswer::SetupWrong => "PageAnswer::SetupWrong",
        });
    }
}

#[test]
fn sc_12_the_list_is_on_disk() {
    let path = &std::path::PathBuf::from(
        std::env::var_os("CARGO_MANIFEST_DIR")
            .expect("cargo sets CARGO_MANIFEST_DIR when it runs tests"),
    )
    .join("../../docs/screens/box.md");
    let want = screens_markdown();
    if std::env::var_os("OPENBMT_WRITE_SCREENS").is_some() {
        if let Some(dir) = std::path::Path::new(path).parent() {
            std::fs::create_dir_all(dir).expect("make docs/screens");
        }
        std::fs::write(path, &want).expect("write docs/screens/box.md");
    }
    let have = std::fs::read_to_string(path).unwrap_or_default();
    assert!(
        have == want,
        "docs/screens/box.md is not what screens_markdown makes: run this test with \
         OPENBMT_WRITE_SCREENS=1 to write it again"
    );
}

#[test]
fn sc_11_an_answer_is_heading_what_next_details() {
    let lines = render(
        &BoxError::Damaged {
            file: "roots",
            why: "missing",
        }
        .words(),
    );
    assert_eq!(lines.len(), 4);
    assert_eq!(lines[0], "File damaged");
    assert!(lines[2].starts_with("Next: "));
    assert_eq!(lines[3], "Details: BX-51: missing");
    assert_eq!(render(&BoxError::NoRandomness.words()).len(), 4);
    let lines = render(&ConsoleAnswer::NoNotes.words());
    assert_eq!(lines.len(), 3);
    assert!(lines[2].starts_with("Next: "));
}

#[test]
fn sc_11_text_from_elsewhere_has_its_own_line() {
    let lines = render(&ConsoleAnswer::UnknownCommand("he\u{202e}lp\nme".into()).words());
    assert!(
        lines.contains(&"You typed: \"help me\"".to_string()),
        "{lines:?}"
    );
    for sentence in &lines[..3] {
        assert!(!sentence.contains("help"), "{sentence}");
    }
}

#[test]
fn sc_03_times_are_iso_8601_in_utc() {
    assert_eq!(iso_utc(0), "1970-01-01T00:00:00Z");
    assert_eq!(iso_utc(951_782_400), "2000-02-29T00:00:00Z");
    assert_eq!(iso_utc(1_700_000_000), "2023-11-14T22:13:20Z");
    assert_eq!(iso_utc(86_399), "1970-01-01T23:59:59Z");
    assert_eq!(iso_utc(4_102_444_800), "2100-01-01T00:00:00Z");
    // Never a panic, whatever the time.
    assert!(iso_utc(u64::MAX).len() > 10);
}

#[test]
fn sc_07_turned_off_is_one_sentence() {
    let status = BoxStatus::TurnedOff.words();
    assert_eq!(BoxError::TurnedOff.words(), status);
    assert_eq!(
        note_words("BX-23: the settings no longer list this Box: it is turned off"),
        status
    );
    assert!(status.next.contains("pair it"));
    assert!(status.what.contains("owner"));
}

#[test]
fn sc_07_the_invitation_is_refused_in_the_same_words_everywhere() {
    for e in [
        BoxError::AlreadyPaired,
        BoxError::Invitation(InviteTextError::BadCheck),
        BoxError::Invitation(InviteTextError::BadLength(3)),
    ] {
        assert_eq!(invite_lines(&Err(e.clone())), render(&e.words()));
    }
}

#[test]
fn sc_10_a_damaged_file_is_named_and_not_replaced() {
    let w = BoxError::Damaged {
        file: "roots",
        why: "not the two roots",
    }
    .words();
    assert!(w.what.contains("roots"));
    assert!(w.what.contains("did not start"));
    assert!(w.what.contains("did not replace"));
    assert!(w.next.contains("copy"));
    assert!(w.next.contains("pair the Box again"));
    assert_eq!(w.detail.as_deref(), Some("not the two roots"));
}

#[test]
fn sc_01_every_note_kind_is_found_in_its_text() {
    let cases = [
        (
            "the member list was not replaced: x",
            NoteKind::MemberListNotReplaced,
        ),
        ("the Box's link files: x", NoteKind::LinkFiles),
        (
            "a message of kind Settings is not handled yet",
            NoteKind::MessageNotHandled,
        ),
        (
            "settings from the owner refused: x",
            NoteKind::SettingsRefused,
        ),
        (
            "BX-23: the settings no longer list this Box: it is turned off",
            NoteKind::TurnedOff,
        ),
        ("anything else", NoteKind::Other),
    ];
    for (line, kind) in cases {
        assert_eq!(NoteKind::of(line).0, kind, "{line}");
    }
}

#[test]
fn sc_11_stable_lines_for_scripts_stay() {
    let lines = state_lines(BoxStatus::InvitationOpen);
    assert_eq!(
        lines.last().map(String::as_str),
        Some("state: invitation open")
    );
    assert_eq!(lines[0], "Invitation open");
    let ok = invite_lines(&Ok(()));
    assert_eq!(
        ok.last().map(String::as_str),
        Some("invitation: open for 30 minutes")
    );
    let key = openbmt_core::identity::Identity::from_secret_bytes(&[7; 32])
        .expect("key")
        .device_id();
    let m = members_lines(&[key]);
    assert!(m.contains(&"members: 1".to_string()));
    assert!(m
        .iter()
        .any(|l| l.starts_with("member: ") && l.len() == 8 + 64));
    let n = notes_lines(&["a".into(), "b".into()]);
    assert_eq!(n.iter().filter(|l| l.starts_with("note: ")).count(), 2);
    assert!(n.contains(&String::new()));
    let u = unknown_lines("help");
    assert_eq!(u[0], "Command not known");
    assert!(u.contains(&"You typed: \"help\"".to_string()));
}
