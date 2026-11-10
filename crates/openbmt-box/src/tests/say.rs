//! SC-01 to SC-12 for the Box console (`docs/SCREEN-RULES.md`).

use std::collections::BTreeSet;
use std::io;

use openbmt_core::boxpair::InviteTextError;
use openbmt_core::say::{check, check_distinct, Tone, Who};
use openbmt_core::settings::SettingsError;
use openbmt_courier::StartError;
use openbmt_net_io::env::LockError;
use openbmt_pos::SettingsError as PosSettingsError;

use crate::pos::{PosOpenError, PosState, Reading};
use crate::runner::{PosView, StartFailure};
use crate::say::{
    all_words, invite_lines, iso_utc, members_lines, note_words, notes_lines, pos_lines,
    pos_token_lines, render, screens_markdown, state_lines, unknown_lines, ConsoleAnswer, NoteKind,
    NoticeWhy,
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

fn state(reading: Reading) -> PosState {
    PosState {
        reading,
        can_close: false,
        last_poll_at: None,
        last_error: None,
        sales: 0,
        notices_taken: 0,
        notices_refused: 0,
        orders: 0,
        matches: 0,
        open_alerts: 0,
        closes: 0,
        reports_waiting: 0,
    }
}

#[test]
fn sc_12_every_word_keeps_the_rules() {
    assert_eq!(check_distinct(&all_words()), Ok(()));
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
        BoxError::NotListed => "BoxError::NotListed",
        BoxError::TooManyMembers => "BoxError::TooManyMembers",
        BoxError::NotPaired => "BoxError::NotPaired",
        BoxError::TurnedOff => "BoxError::TurnedOff",
        BoxError::TokenMode { .. } => "BoxError::TokenMode",
        BoxError::BadToken(_) => "BoxError::BadToken",
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
    }
}

fn pos_settings_name(e: &PosSettingsError) -> &'static str {
    match e {
        PosSettingsError::TenderNames => "PosSettingsError::TenderNames",
        PosSettingsError::Currencies => "PosSettingsError::Currencies",
        PosSettingsError::Locations => "PosSettingsError::Locations",
        PosSettingsError::Timing => "PosSettingsError::Timing",
    }
}

fn reading_name(r: &Reading) -> &'static str {
    match r {
        Reading::Stopped => "Reading::Stopped",
        Reading::NoToken => "Reading::NoToken",
        Reading::TokenRefused(_) => "Reading::TokenRefused",
        Reading::Reading => "Reading::Reading",
    }
}

fn view_name(v: &PosView) -> &'static str {
    match v {
        PosView::NotInstalled => "PosView::NotInstalled",
        PosView::Disabled(_) => "PosView::Disabled",
        PosView::State(_) => "PosView::State",
    }
}

fn open_name(e: &PosOpenError) -> &'static str {
    match e {
        PosOpenError::Damaged { .. } => "PosOpenError::Damaged",
        PosOpenError::Io { .. } => "PosOpenError::Io",
        PosOpenError::Settings(_) => "PosOpenError::Settings",
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
        ConsoleAnswer::PosTokenStored => "ConsoleAnswer::PosTokenStored",
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
        NoteKind::PaidNoticesNoPos => "NoteKind::PaidNoticesNoPos",
        NoteKind::ReportsNotStored => "NoteKind::ReportsNotStored",
        NoteKind::PosNoticeNotSent => "NoteKind::PosNoticeNotSent",
        NoteKind::MessageNotHandled => "NoteKind::MessageNotHandled",
        NoteKind::SettingsRefused => "NoteKind::SettingsRefused",
        NoteKind::PaidNoticeRefused(_) => "NoteKind::PaidNoticeRefused",
        NoteKind::PaidNoticesNotStored => "NoteKind::PaidNoticesNotStored",
        NoteKind::PosNotRead => "NoteKind::PosNotRead",
        NoteKind::NoOrderToClose => "NoteKind::NoOrderToClose",
        NoteKind::CloseRefused => "NoteKind::CloseRefused",
        NoteKind::OrderAlreadyClosed => "NoteKind::OrderAlreadyClosed",
        NoteKind::OrderNotClosed => "NoteKind::OrderNotClosed",
        NoteKind::CloseAttemptNotSaved => "NoteKind::CloseAttemptNotSaved",
        NoteKind::CloseResultNotSaved => "NoteKind::CloseResultNotSaved",
        NoteKind::ReportsDropped => "NoteKind::ReportsDropped",
        NoteKind::TokenNotErased => "NoteKind::TokenNotErased",
        NoteKind::TurnedOff => "NoteKind::TurnedOff",
        NoteKind::PosSideOff => "NoteKind::PosSideOff",
        NoteKind::TooManyCloses => "NoteKind::TooManyCloses",
        NoteKind::Other => "NoteKind::Other",
    }
}

fn why_name(w: NoticeWhy) -> &'static str {
    match w {
        NoticeWhy::DoesNotRead => "NoticeWhy::DoesNotRead",
        NoticeWhy::OtherCounter => "NoticeWhy::OtherCounter",
        NoticeWhy::NotACounter => "NoticeWhy::NotACounter",
        NoticeWhy::BadSignature => "NoticeWhy::BadSignature",
        NoticeWhy::NumberNotAbove => "NoticeWhy::NumberNotAbove",
        NoticeWhy::TooManyCounters => "NoticeWhy::TooManyCounters",
        NoticeWhy::DoesNotAgree => "NoticeWhy::DoesNotAgree",
        NoticeWhy::SalesFull => "NoticeWhy::SalesFull",
        NoticeWhy::Other => "NoticeWhy::Other",
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
        BoxError::NotListed,
        BoxError::TooManyMembers,
        BoxError::NotPaired,
        BoxError::TurnedOff,
        BoxError::TokenMode { mode: 0o644 },
        BoxError::BadToken("empty"),
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
        PosSettingsError::TenderNames,
        PosSettingsError::Currencies,
        PosSettingsError::Locations,
        PosSettingsError::Timing,
    ] {
        need(pos_settings_name(&e));
    }
    for r in [
        Reading::Stopped,
        Reading::NoToken,
        Reading::TokenRefused(String::new()),
        Reading::Reading,
    ] {
        need(reading_name(&r));
    }
    for v in [
        PosView::NotInstalled,
        PosView::Disabled(PosOpenError::Settings(PosSettingsError::Timing)),
        PosView::State(Box::new(state(Reading::NoToken))),
    ] {
        need(view_name(&v));
    }
    for e in [
        PosOpenError::Damaged {
            file: "pos-ledger",
            why: String::new(),
        },
        PosOpenError::Io {
            file: "pos-ledger",
            kind: io::ErrorKind::NotFound,
        },
        PosOpenError::Settings(PosSettingsError::Timing),
    ] {
        need(open_name(&e));
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
        ConsoleAnswer::PosTokenStored,
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
        NoteKind::PaidNoticesNoPos,
        NoteKind::ReportsNotStored,
        NoteKind::PosNoticeNotSent,
        NoteKind::MessageNotHandled,
        NoteKind::SettingsRefused,
        NoteKind::PaidNoticeRefused(NoticeWhy::Other),
        NoteKind::PaidNoticesNotStored,
        NoteKind::PosNotRead,
        NoteKind::NoOrderToClose,
        NoteKind::CloseRefused,
        NoteKind::OrderAlreadyClosed,
        NoteKind::OrderNotClosed,
        NoteKind::CloseAttemptNotSaved,
        NoteKind::CloseResultNotSaved,
        NoteKind::ReportsDropped,
        NoteKind::TokenNotErased,
        NoteKind::TurnedOff,
        NoteKind::PosSideOff,
        NoteKind::TooManyCloses,
        NoteKind::Other,
    ] {
        need(kind_name(k));
    }
    for w in [
        NoticeWhy::DoesNotRead,
        NoticeWhy::OtherCounter,
        NoticeWhy::NotACounter,
        NoticeWhy::BadSignature,
        NoticeWhy::NumberNotAbove,
        NoticeWhy::TooManyCounters,
        NoticeWhy::DoesNotAgree,
        NoticeWhy::SalesFull,
        NoticeWhy::Other,
    ] {
        need(why_name(w));
    }
}

#[test]
fn sc_12_the_list_is_on_disk() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/screens/box.md");
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
    let lines = render(&BoxError::TokenMode { mode: 0o644 }.words());
    assert_eq!(lines.len(), 4);
    assert_eq!(lines[0], "POS token file too open");
    assert!(lines[2].starts_with("Next: "));
    assert_eq!(lines[3], "Details: BX-41: mode 0644");
    assert_eq!(render(&BoxError::NoRandomness.words()).len(), 4);
    let lines = render(&ConsoleAnswer::NoNotes.words());
    assert_eq!(lines.len(), 3);
    assert!(lines[2].starts_with("Next: "));
}

#[test]
fn sc_11_text_from_elsewhere_and_times_have_their_own_lines() {
    let mut s = state(Reading::Reading);
    s.last_poll_at = Some(1_700_000_000);
    s.last_error = Some("the POS was not read:\nthe POS \u{202e}is down".into());
    let lines = render(&s.words(BoxStatus::Paired));
    assert!(lines.contains(&"When: 2023-11-14T22:13:20Z".to_string()));
    assert!(
        lines.contains(&"The Box noted: \"the POS was not read: the POS is down\"".to_string()),
        "{lines:?}"
    );
    for sentence in &lines[..3] {
        assert!(!sentence.contains("2023"), "{sentence}");
        assert!(!sentence.contains("is down"), "{sentence}");
    }
    s.last_poll_at = None;
    s.last_error = None;
    assert!(!render(&s.words(BoxStatus::Paired))
        .iter()
        .any(|l| l.starts_with("When: ")));
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
fn sc_07_the_three_stopped_readings_read_differently() {
    let a = Reading::Stopped.words(BoxStatus::Unpaired);
    let b = Reading::Stopped.words(BoxStatus::TurnedOff);
    let c = Reading::Stopped.words(BoxStatus::Paired);
    assert_eq!(Reading::Stopped.words(BoxStatus::InvitationOpen), a);
    for (x, y) in [(&a, &b), (&a, &c), (&b, &c)] {
        assert_ne!(x.heading, y.heading);
        assert_ne!(x.what, y.what);
        assert_ne!(x.next, y.next);
    }
    // And they differ from the other two ways the POS is off.
    let installed = PosView::NotInstalled.words(BoxStatus::Paired);
    let off = PosView::Disabled(PosOpenError::Settings(PosSettingsError::Timing))
        .words(BoxStatus::Paired);
    for x in [&a, &b, &c] {
        for y in [&installed, &off] {
            assert_ne!(x.heading, y.heading);
            assert_ne!(x.next, y.next);
        }
    }
    assert_ne!(installed.next, off.next);
}

#[test]
fn sc_07_stopped_machine_line_says_which_condition() {
    let s = state(Reading::Stopped);
    let first = |status| s.console_lines(status).remove(0);
    assert_eq!(first(BoxStatus::Unpaired), "reading: no (not paired)");
    assert_eq!(first(BoxStatus::TurnedOff), "reading: no (turned off)");
    assert_eq!(first(BoxStatus::Paired), "reading: no (not started)");
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
fn sc_07_an_empty_line_and_an_empty_token_are_one_fault() {
    // The console maps "no line followed" to this one refusal.
    let none = pos_token_lines(Err(BoxError::BadToken("empty")));
    assert_eq!(none, render(&BoxError::BadToken("empty").words()));
    // The same words when the file is read and found empty.
    assert_eq!(
        Reading::TokenRefused(BoxError::BadToken("empty").to_string()).words(BoxStatus::Paired),
        BoxError::BadToken("empty").words()
    );
}

#[test]
fn sc_04_the_token_file_fix_is_a_command_the_person_can_copy() {
    let w = BoxError::TokenMode { mode: 0o644 }.words();
    assert!(w.next.contains("chmod 600 pos.token "), "{}", w.next);
    assert_eq!(check(&w), Ok(()));
    // The same words through the reading the `pos` command shows.
    let text = BoxError::TokenMode { mode: 0o644 }.to_string();
    assert_eq!(Reading::TokenRefused(text).words(BoxStatus::Paired), w);
    // No other words hold a command.
    for (name, other) in all_words() {
        if !name.starts_with("BoxError::TokenMode") {
            assert!(!other.next.contains("chmod"), "{name}");
        }
    }
}

#[test]
fn sc_03_the_token_is_never_printed() {
    let secret = "sk_live_secret_token_value";
    let mut lines = Vec::new();
    lines.extend(pos_token_lines(Ok(())));
    lines.extend(pos_token_lines(Err(BoxError::BadToken("not text"))));
    for l in lines {
        assert!(!l.contains(secret));
    }
    for (name, w) in all_words() {
        let all = render(&w).join("\n");
        assert!(!all.contains(secret), "{name}");
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
fn sc_03_a_key_is_only_in_the_detail_and_short() {
    let line =
        "BX-40: a paid notice from DeviceId(ab12cd34..) refused: its signature does not check";
    let (kind, rest) = NoteKind::of(line);
    assert_eq!(kind, NoteKind::PaidNoticeRefused(NoticeWhy::BadSignature));
    let w = kind.words(rest);
    assert_eq!(w.detail.as_deref(), Some("sender key ab12cd34"));
    assert!(!w.what.contains("ab12"));
}

#[test]
fn sc_08_text_from_the_pos_is_quoted() {
    let w = note_words("BX-43: order o2 was not closed for sale 2: the till said \u{202e}no");
    let q = w.quoted.expect("quoted");
    assert_eq!(q.from, "The till system said");
    assert_eq!(q.text, "order o2: the till said no");
    assert_eq!(w.detail.as_deref(), Some("sale 2"));
    let w = note_words("the POS was not read: down (0 pages were kept)");
    assert!(w.quoted.is_some());
    assert!(!w.what.contains("down"));
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
            "1 paid notices arrived and this Box has no POS: dropped",
            NoteKind::PaidNoticesNoPos,
        ),
        (
            "BX-50: the reports were not stored: x",
            NoteKind::ReportsNotStored,
        ),
        (
            "a POS notice could not be sent to the owner: x",
            NoteKind::PosNoticeNotSent,
        ),
        (
            "a message of kind Settings is not handled yet",
            NoteKind::MessageNotHandled,
        ),
        (
            "settings from the owner refused: x",
            NoteKind::SettingsRefused,
        ),
        (
            "BX-50: the paid notices were not stored and are dropped: x",
            NoteKind::PaidNoticesNotStored,
        ),
        ("the POS was not read: x", NoteKind::PosNotRead),
        (
            "the POS did not name an order to close: x",
            NoteKind::NoOrderToClose,
        ),
        (
            "PS-81: an order is closed only for a sale reported paid now (CR-30)",
            NoteKind::CloseRefused,
        ),
        (
            "BX-43: order a was already closed for sale 1",
            NoteKind::OrderAlreadyClosed,
        ),
        (
            "BX-43: order a was not closed for sale 1: x",
            NoteKind::OrderNotClosed,
        ),
        (
            "BX-50: the close attempt was not saved: x",
            NoteKind::CloseAttemptNotSaved,
        ),
        (
            "BX-50: the close result was not saved after the till closed the order: x",
            NoteKind::CloseResultNotSaved,
        ),
        (
            "BX-42: more than 256 reports are waiting for the relay: the oldest were dropped",
            NoteKind::ReportsDropped,
        ),
        (
            "BX-23: the POS token could not be erased: x",
            NoteKind::TokenNotErased,
        ),
        (
            "BX-23: the settings no longer list this Box: it is turned off",
            NoteKind::TurnedOff,
        ),
        ("the POS side is off: x", NoteKind::PosSideOff),
        (
            "PS-72: too many closes are recorded: none is made",
            NoteKind::TooManyCloses,
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
    let pos = pos_lines(&PosView::NotInstalled, BoxStatus::Paired);
    assert!(pos.iter().any(|l| l.starts_with("pos: no POS connector")));
}
