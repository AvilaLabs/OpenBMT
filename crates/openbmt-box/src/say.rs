//! What the person at the Box console is told (`docs/SCREEN-RULES.md`,
//! SC-01 to SC-12).
//!
//! Every state and every refusal of the Box becomes one
//! [`Words`](openbmt_core::say::Words). The `Display` texts with rule
//! numbers stay for logs; the console prints [`render`]: the heading, what
//! happened, "Next: " and what to do, then "Details: " with the rule and the
//! detail, then "When: " with a time and a line for text that came from
//! elsewhere. Values a script needs keep their own stable lines
//! (`state: `, `members: `, `member: `, `note: `, `invitation: `), printed
//! after the words.
//!
//! The Box's reader is the Box keeper, who knows how to type a command.

use std::io;

use openbmt_core::boxpair::InviteTextError;
use openbmt_core::confirmed::ConfirmedError;
use openbmt_core::identity::DeviceId;
use openbmt_core::say::{Quoted, Tone, Who, Words};
use openbmt_core::settings::SettingsError;
use openbmt_courier::StartError;
use openbmt_net_io::env::LockError;

use crate::runner::StartFailure;
use crate::state::{BoxError, BoxStatus, INVITE_SECS, MAX_MEMBERS};

/// The text of a `Details` line may hold no control character.
fn plain(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect::<String>()
        .trim()
        .to_string()
}

/// Seconds since 1970 as an ISO-8601 date and time in UTC, such as
/// `2026-11-10T14:03:09Z`. The console has no clock formatting of its own.
pub fn iso_utc(secs: u64) -> String {
    let days = i64::try_from(secs / 86_400).unwrap_or(i64::MAX / 2);
    let rest = secs % 86_400;
    // Days since 1970 to a civil date (proleptic Gregorian).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rest / 3_600,
        rest % 3_600 / 60,
        rest % 60
    )
}

/// The lines the console prints for one answer (SC-11): the heading, what
/// happened, `Next: ` and what to do, `Details: ` with the rule and the
/// detail when there is one, `When: ` with the time in UTC, and the text
/// from elsewhere on its own line as `<from>: "<text>"`.
pub fn render(words: &Words) -> Vec<String> {
    let mut lines = vec![
        words.heading.clone(),
        words.what.clone(),
        format!("Next: {}", words.next),
    ];
    match (words.code, &words.detail) {
        (Some(code), Some(detail)) => lines.push(format!("Details: {code}: {detail}")),
        (Some(code), None) => lines.push(format!("Details: {code}")),
        (None, Some(detail)) => lines.push(format!("Details: {detail}")),
        (None, None) => {}
    }
    if let Some(at) = words.at {
        lines.push(format!("When: {}", iso_utc(at)));
    }
    if let Some(q) = &words.quoted {
        lines.push(format!("{}: \"{}\"", q.from, q.text));
    }
    lines
}

fn stop(heading: &str, what: String, who: Who, next: impl Into<String>) -> Words {
    Words::new(Tone::Stop, heading, what, who, next)
}

/// Whether these words are a placeholder, a text still to be written.
pub fn is_placeholder(words: &Words) -> bool {
    words.heading.starts_with("WORDS_NEEDED_")
}

/// The words for a file that is damaged: the Box did not start over it and
/// did not replace it (SC-10).
fn damaged_words(file: &str, why: &str) -> Words {
    stop(
        "File damaged",
        format!("The file {file} is damaged, so the Box did not start and did not replace it."),
        Who::BoxKeeper,
        "Restore the file from a copy, or erase the data folder and pair the Box again.",
    )
    .code("BX-51")
    .detail(plain(why))
}

impl BoxStatus {
    /// What the `state` command tells the person (BX-03, BX-12, BX-16,
    /// BX-23).
    pub fn words(self) -> Words {
        match self {
            Self::Unpaired => Words::new(
                Tone::Info,
                "Not paired yet",
                "This Box is not paired with a shop and holds no invitation.",
                Who::BoxKeeper,
                "Make an invitation in the Key app, then type invite and the invitation here.",
            )
            .code("BX-03"),
            Self::InvitationOpen => Words::new(
                Tone::Info,
                "Invitation open",
                "This Box holds an invitation and waits for the owner's phone to pair it.",
                Who::Owner,
                format!(
                    "Finish pairing from the owner's phone within {} minutes of typing the invitation.",
                    INVITE_SECS / 60
                ),
            )
            .code("BX-12"),
            Self::Paired => Words::new(
                Tone::Good,
                "Paired",
                "This Box is paired and the shop's settings list it.",
                Who::Nobody,
                "Nothing to do. It passes messages between the shop's devices.",
            )
            .code("BX-16"),
            Self::TurnedOff => turned_off(),
        }
    }
}

impl BoxStatus {
    /// What the status page tells the person (BX-55). It is the console's
    /// words, except for a Box that waits for an invitation: the page has a
    /// box to type it in, where the console has a command.
    pub fn page_words(self) -> Words {
        match self {
            Self::Unpaired => Words::new(
                Tone::Info,
                "Waiting for an invitation",
                "This Box is not paired with a shop and is waiting for an invitation.",
                Who::BoxKeeper,
                "Make an invitation in the Key app, then type it in the box below.",
            )
            .code("BX-03"),
            other => other.words(),
        }
    }
}

/// What the status page tells a person that the console does not (BX-57,
/// BX-58).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageAnswer {
    /// An invitation was posted while the Box already holds one.
    InvitationHeld,
    /// A form was posted from another site, or from none the Box can tell.
    NotFromThisPage,
    /// A form was posted with a setup code that is not the Box's (BX-62).
    SetupWrong,
}

impl PageAnswer {
    /// What a person is told.
    pub fn words(self) -> Words {
        match self {
            Self::InvitationHeld => stop(
                "Invitation already held",
                "This Box already holds an invitation that has not run out, so it took no other."
                    .into(),
                Who::BoxKeeper,
                format!(
                    "Finish pairing from the owner's phone, or wait until the invitation runs out after {} minutes and enter a new one.",
                    INVITE_SECS / 60
                ),
            )
            .code("BX-57"),
            Self::NotFromThisPage => stop(
                "Not sent from this page",
                "The invitation did not come from this page, so the Box did not take it.".into(),
                Who::BoxKeeper,
                "Open this page in the browser again and type the invitation in the box on it.",
            )
            .code("BX-58"),
            Self::SetupWrong => stop(
                "Setup code wrong",
                "The setup code does not match the one in this Box's log.".into(),
                Who::BoxKeeper,
                "Wait ten seconds, then type the code from the log again.",
            )
            .code("BX-62"),
        }
    }
}

/// The one condition behind `BoxStatus::TurnedOff` and
/// `BoxError::TurnedOff`: settings signed by the owner no longer list this
/// Box (BX-23). The Box does not keep who sent them.
fn turned_off() -> Words {
    stop(
        "Turned off",
        "The owner's settings no longer list this Box, so it is turned off.".into(),
        Who::BoxKeeper,
        "To use it again, erase its data folder and pair it from the owner's phone with a new invitation.",
    )
    .code("BX-23")
}

impl BoxError {
    /// What a person is told about this refusal.
    pub fn words(&self) -> Words {
        match self {
            Self::Damaged { file, why } => damaged_words(file, why),
            Self::Io { file, op, kind } => stop(
                "Storage problem",
                format!("The Box could not {op} the file {file} and kept what it had."),
                Who::BoxKeeper,
                "Check that the disk has room and this user may use the data folder, then try again.",
            )
            .code("BX-50")
            .detail(format!("{kind}")),
            Self::NoRandomness => stop(
                "No random numbers",
                "The computer gave the Box no random numbers, so it changed nothing.".into(),
                Who::BoxKeeper,
                "Try the command again; if it keeps happening, the computer's random source needs repair.",
            )
            .code("BX-01"),
            Self::AlreadyPaired => stop(
                "Already paired",
                "This Box is already paired with a shop, so it took no invitation.".into(),
                Who::BoxKeeper,
                "To pair it again, erase its data folder and start the Box again.",
            )
            .code("BX-16"),
            Self::Invitation(e) => invitation_words(e),
            Self::NoInvitation => stop(
                "No invitation open",
                "This Box holds no invitation, so it cannot take a welcome.".into(),
                Who::BoxKeeper,
                "Type invite and an invitation from the Key app, then pair from the owner's phone.",
            )
            .code("BX-12"),
            Self::InvitationExpired => stop(
                "Invitation ran out",
                format!(
                    "The invitation was older than {} minutes, so the Box forgot it.",
                    INVITE_SECS / 60
                ),
                Who::BoxKeeper,
                "Make a new invitation in the Key app and type it with the invite command.",
            )
            .code("BX-12"),
            Self::BadWelcome(e) => stop(
                "Welcome not read",
                "What the owner's phone sent to pair this Box is not a welcome.".into(),
                Who::Owner,
                "Pair again from the owner's phone while the invitation is still open.",
            )
            .code("BX-15")
            .detail(e.to_string()),
            Self::WelcomeMac => stop(
                "Welcome is not for this Box",
                "The welcome was not made for the invitation typed into this Box.".into(),
                Who::Owner,
                "Check that the owner's phone used the invitation typed here, then pair again.",
            )
            .code("BX-15"),
            Self::BadSettings(e) => stop(
                "Settings not read",
                "The settings that arrived are not settings the Box can read.".into(),
                Who::Owner,
                "Send the settings again from the owner's phone.",
            )
            .code("BX-15")
            .detail(e.to_string()),
            Self::Settings(e) => settings_words(e),
            // BX-18: no words of their own yet; the reasons are the same
            // ones (a signature, a version, a rule, or a confirmation that
            // is not a counter's of the settings in force).
            Self::Confirmed(ConfirmedError::Settings(e)) => settings_words(e),
            Self::Confirmed(e @ ConfirmedError::Confirmation(_)) => {
                settings_break_a_rule().code("BX-18").detail(e.to_string())
            }
            Self::NotListed => stop(
                "Box not in the settings",
                "The settings the owner sent do not list this Box as a Box, so pairing did not finish.".into(),
                Who::Owner,
                "Add this Box to the shop's devices on the owner's phone, then type a new invitation here.",
            )
            .code("BX-15"),
            Self::TooManyMembers => stop(
                "Too many devices",
                format!("The shop's device list is longer than the {MAX_MEMBERS} devices a Box can serve."),
                Who::Owner,
                "Remove a device on the owner's phone and send the settings again.",
            )
            .code("BX-22"),
            Self::NotPaired => stop(
                "Pair the Box first",
                "This Box is not paired with a shop, so it did nothing.".into(),
                Who::BoxKeeper,
                "Pair it first: type invite and the invitation, then approve it from the owner's phone.",
            )
            .code("BX-21"),
            Self::TurnedOff => turned_off(),
        }
    }
}

/// What a person is told about an invitation that did not read (BX-10).
pub fn invitation_words(e: &InviteTextError) -> Words {
    match e {
        InviteTextError::BadLength(n) => stop(
            "Invitation not read",
            format!("An invitation has 30 characters and this one has {n}."),
            Who::BoxKeeper,
            "Type it again exactly as the Key app shows it.",
        )
        .code("BX-10"),
        InviteTextError::BadCharacter(c) => stop(
            "Invitation not read",
            "One character is not a character an invitation uses.".into(),
            Who::BoxKeeper,
            "Type it again exactly as the Key app shows it.",
        )
        .code("BX-10")
        .detail(format!("{c:?}")),
        InviteTextError::BadCheck => stop(
            "Invitation not read",
            "The last characters of the invitation do not match the rest, so one was mistyped."
                .into(),
            Who::BoxKeeper,
            "Type it again, checking each group of five characters.",
        )
        .code("BX-10"),
    }
}

/// What a person is told about settings that were refused (BX-15, BX-21).
fn settings_words(e: &SettingsError) -> Words {
    let detail = e.to_string();
    match e {
        SettingsError::OwnerSignatureInvalid => stop(
            "Settings not signed",
            "The new settings do not carry the owner's signature, so the Box kept its own.".into(),
            Who::Owner,
            "Send the settings again from the owner's phone.",
        ),
        SettingsError::ApproverRequired => stop(
            "Settings need the Key app",
            "The new settings loosen something and need the Key app's approval too.".into(),
            Who::Owner,
            "Approve the change in the Key app, then send the settings again.",
        ),
        SettingsError::ApproverSignatureInvalid => stop(
            "Key app approval is wrong",
            "The Key app's approval on the new settings does not check, so the Box kept its own."
                .into(),
            Who::Owner,
            "Approve the change in the Key app again, then send the settings again.",
        ),
        SettingsError::StaleVersion { .. } => stop(
            "Settings are not newer",
            "The new settings are no newer than the settings the Box already has.".into(),
            Who::Owner,
            "Send the newest settings from the owner's phone.",
        ),
        // A Box paired with the Key app is not asked for a counter's
        // confirmation, and if one came it breaks a rule. A Box of a shop
        // with its own wallet (BX-18) gets these words until its own are
        // written.
        SettingsError::Invalid(_)
        | SettingsError::ConfirmationRequired(_)
        | SettingsError::RedirectAtPairing
        | SettingsError::WalletSigNotWanted
        | SettingsError::RedirectStarter
        | SettingsError::WalletSig(_) => settings_break_a_rule(),
    }
    .code("BX-21")
    .detail(detail)
}

fn settings_break_a_rule() -> Words {
    stop(
        "Settings break a rule",
        "The new settings break one of the rules every settings must keep.".into(),
        Who::Owner,
        "Change the settings on the owner's phone and send them again.",
    )
}

impl StartFailure {
    /// What a person is told when the Box did not start. The Box did not
    /// run, so nothing is listening.
    pub fn words(&self) -> Words {
        match self {
            Self::Lock(LockError::Busy(path)) => stop(
                "Data folder in use",
                "Another OpenBMT program is already using this Box's data folder.".into(),
                Who::BoxKeeper,
                "Stop the other program, or give this Box its own data folder, then start it again.",
            )
            .code("BX-02")
            .detail(plain(&path.display().to_string())),
            Self::Lock(LockError::Io(e)) => stop(
                "Data folder not locked",
                "The Box could not lock its data folder, so it did not start.".into(),
                Who::BoxKeeper,
                "Check that the folder exists and this user may write in it, then start the Box again.",
            )
            .code("BX-02")
            .detail(plain(e)),
            Self::State(e) => e.words(),
            Self::Relay(e) => stop(
                "Relay files not opened",
                "The Box could not open the files of its message relay, so it did not start.".into(),
                Who::BoxKeeper,
                "Check the relay folder in the data folder and that the disk has room, then start the Box again.",
            )
            .code("LK-41")
            .detail(plain(e)),
            Self::Courier(e) => courier_words(e),
            Self::Listen(e) => stop(
                "Box could not listen",
                "The Box could not listen on the address it was given, so it did not start.".into(),
                Who::BoxKeeper,
                "Check that no other program uses that port and the address is this computer's, then start again.",
            )
            .detail(plain(&e.to_string())),
        }
    }
}

/// What a person is told when the Box's own link files stop it starting.
fn courier_words(e: &StartError) -> Words {
    match e {
        StartError::NeedsNewPairing { file } => stop(
            "Link file missing",
            format!("The link file {file} is missing, so the Box did not start."),
            Who::BoxKeeper,
            "Erase the data folder and pair the Box again from the owner's phone.",
        )
        .code("LK-53"),
        StartError::Corrupt { file } => stop(
            "Link file damaged",
            format!("The link file {file} does not read back, so the Box did not start and did not replace it."),
            Who::BoxKeeper,
            "Restore the file from a copy, or erase the data folder and pair the Box again.",
        )
        .code("LK-53"),
        StartError::Store(s) => stop(
            "Link files not usable",
            format!("The Box could not {} the link file {}, so it did not start.", s.op, s.file),
            Who::BoxKeeper,
            "Check that the disk works and this user may use the data folder, then start the Box again.",
        )
        .code("LK-40")
        .detail(format!("{}", s.kind)),
    }
}

/// The answers of the console that are not the words of a type of the Box.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConsoleAnswer {
    /// `invite` or the invitation file was taken.
    InvitationTaken,
    /// `members`: how many devices the Box serves.
    Members(usize),
    /// `notes` when the Box has kept none.
    NoNotes,
    /// A command the console does not know; the word typed.
    UnknownCommand(String),
    /// The invitation file named at the start could not be read.
    InvitationFileUnreadable {
        /// The file named.
        path: String,
        /// What the system said.
        kind: io::ErrorKind,
    },
    /// The Box was started with the wrong arguments.
    BadArguments,
    /// The address to listen on is not an address and a port; what was
    /// given.
    BadAddress(String),
}

impl ConsoleAnswer {
    /// What a person is told.
    pub fn words(&self) -> Words {
        match self {
            Self::InvitationTaken => Words::new(
                Tone::Info,
                "Invitation taken",
                format!(
                    "The Box took the invitation and keeps it for {} minutes.",
                    INVITE_SECS / 60
                ),
                Who::Owner,
                "Finish pairing from the owner's phone before the time is up.",
            )
            .code("BX-12"),
            Self::Members(0) => Words::new(
                Tone::Info,
                "No devices served",
                "The Box serves no devices, because it is not paired or it is turned off.",
                Who::BoxKeeper,
                "Run the state command to see which, and follow what it says.",
            )
            .code("BX-22"),
            Self::Members(n) => Words::new(
                Tone::Info,
                "Devices on the list",
                format!(
                    "The Box serves {n} {}.",
                    if *n == 1 { "device" } else { "devices" }
                ),
                Who::Nobody,
                "Nothing to do. The owner's settings decide who is on the list.",
            )
            .code("BX-22"),
            Self::NoNotes => Words::new(
                Tone::Info,
                "No notes",
                "The Box has kept no notes.",
                Who::Nobody,
                "Nothing to do. Run notes again later to see new ones.",
            ),
            Self::UnknownCommand(word) => Words::new(
                Tone::Stop,
                "Command not known",
                "The Box does not know that command.",
                Who::BoxKeeper,
                "Type one of invite, state, members, notes or quit.",
            )
            .quoted(Quoted::new("You typed", word)),
            Self::InvitationFileUnreadable { path, kind } => stop(
                "Invitation file not read",
                "The Box could not read the invitation file it was given.".into(),
                Who::BoxKeeper,
                "Check the file name and that this user may read it, then use the invite command.",
            )
            .code("BX-12")
            .detail(plain(&format!("{path}: {kind}"))),
            Self::BadArguments => stop(
                "Wrong arguments",
                "The Box needs a data folder and an address to listen on, and may take other options.".into(),
                Who::BoxKeeper,
                "Type the data folder and the address, then add --invitation-file, --status-page or --no-console as needed.",
            ),
            Self::BadAddress(text) => stop(
                "Address not understood",
                "The address to listen on is not an address and a port.".into(),
                Who::BoxKeeper,
                "Give it as an address, a colon and a port, such as 127.0.0.1:8080.",
            )
            .quoted(Quoted::new("You typed", text)),
        }
    }
}

/// The kind of a note the Box kept. A note is one line of text; what is
/// stored does not change, and the words are made when it is shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoteKind {
    /// The relay's member list was not replaced.
    MemberListNotReplaced,
    /// The Box's link files could not be used.
    LinkFiles,
    /// A message of a kind the Box does not handle.
    MessageNotHandled,
    /// Settings from the owner were refused.
    SettingsRefused,
    /// The Box was turned off.
    TurnedOff,
    /// A note with no kind of its own, such as a message of the link.
    Other,
}

impl NoteKind {
    /// The kind of a stored note, and what follows the fixed part of its
    /// text. The notes are written by the runner; each kind is the start of
    /// the text its writer gives.
    pub fn of(line: &str) -> (Self, &str) {
        let after = |prefix: &str| line.strip_prefix(prefix);
        if let Some(rest) = after("the member list was not replaced: ") {
            (Self::MemberListNotReplaced, rest)
        } else if let Some(rest) = after("the Box's link files: ") {
            (Self::LinkFiles, rest)
        } else if let Some(rest) = after("a message of kind ") {
            (Self::MessageNotHandled, rest)
        } else if let Some(rest) = after("settings from the owner refused: ") {
            (Self::SettingsRefused, rest)
        } else if line.starts_with("BX-23: the settings no longer list this Box") {
            (Self::TurnedOff, "")
        } else {
            (Self::Other, line)
        }
    }

    /// What a person is told about a note of this kind. `rest` is what
    /// [`NoteKind::of`] gave: the stored text after the fixed part.
    pub fn words(self, rest: &str) -> Words {
        let rest = plain(rest);
        match self {
            Self::MemberListNotReplaced => stop(
                "Device list not updated",
                "The Box could not change the list of devices it serves, so the old list stays.".into(),
                Who::BoxKeeper,
                "Check the disk and the data folder; the Box tries again at its next step.",
            )
            .detail(rest),
            Self::LinkFiles => stop(
                "Link files not usable",
                "The Box could not use its link files, so it is not passing messages.".into(),
                Who::BoxKeeper,
                "Check the disk; if it keeps happening, erase the data folder and pair the Box again.",
            )
            .detail(rest),
            Self::MessageNotHandled => Words::new(
                Tone::Ended,
                "Message not handled",
                "A message of a kind this Box does not handle arrived, and it was dropped.",
                Who::Nobody,
                "Nothing to do. This version of the Box does not use that kind of message.",
            )
            .detail(rest),
            Self::SettingsRefused => stop(
                "Settings refused",
                "New settings from the owner were refused, so the Box kept the settings it had.".into(),
                Who::Owner,
                "Tell the owner that the Box refused them, and say what is under Details.",
            )
            .detail(rest),
            Self::TurnedOff => turned_off(),
            Self::Other => Words::new(
                Tone::Info,
                "Note from the Box",
                "The Box kept a note that has no plain words yet.",
                Who::BoxKeeper,
                "Keep the text under Details for whoever looks after this Box.",
            )
            .detail(rest),
        }
    }
}

/// The answer of the `notes` command for one stored note.
pub fn note_words(line: &str) -> Words {
    let (kind, rest) = NoteKind::of(line);
    kind.words(rest)
}

// Console answers, one function for each command, shared by the program and
// the examples. Each gives the lines to print.

fn lines_of(words: &Words) -> Vec<String> {
    render(words)
}

/// The answer to `invite`, or to the invitation file at the start.
pub fn invite_lines(result: &Result<(), BoxError>) -> Vec<String> {
    match result {
        Ok(()) => {
            let mut lines = lines_of(&ConsoleAnswer::InvitationTaken.words());
            lines.push(format!("invitation: open for {} minutes", INVITE_SECS / 60));
            lines
        }
        Err(e) => lines_of(&e.words()),
    }
}

/// The answer to `state`: the words, then the stable line.
pub fn state_lines(status: BoxStatus) -> Vec<String> {
    let mut lines = lines_of(&status.words());
    lines.push(format!("state: {}", status.text()));
    lines
}

/// The answer to `members`: the words, then `members: N` and one `member: `
/// line with the key for each.
pub fn members_lines(members: &[DeviceId]) -> Vec<String> {
    let mut lines = lines_of(&ConsoleAnswer::Members(members.len()).words());
    lines.push(format!("members: {}", members.len()));
    for m in members {
        let hex: String = m.to_bytes().iter().map(|b| format!("{b:02x}")).collect();
        lines.push(format!("member: {hex}"));
    }
    lines
}

/// The answer to `notes`: each note as words, a stable `note: ` line after
/// it, and a blank line between notes.
pub fn notes_lines(notes: &[String]) -> Vec<String> {
    if notes.is_empty() {
        return lines_of(&ConsoleAnswer::NoNotes.words());
    }
    let mut lines = Vec::new();
    for (i, n) in notes.iter().enumerate() {
        if i > 0 {
            lines.push(String::new());
        }
        lines.extend(lines_of(&note_words(n)));
        lines.push(format!("note: {n}"));
    }
    lines
}

/// The answer to a command the console does not know.
pub fn unknown_lines(word: &str) -> Vec<String> {
    lines_of(&ConsoleAnswer::UnknownCommand(word.to_string()).words())
}

/// SC-12: the words of every state and every refusal the Box console can
/// show, at least one for each variant and for each variant inside it.
/// A name that holds " and " stands for several states with one sentence
/// (SC-07: one fault reached two ways is one sentence); a name that holds
/// ": " says which sample of a variant it is.
pub fn all_words() -> Vec<(String, Words)> {
    use crate::state::{KEY_FILE, ROOTS_FILE, SETTINGS_FILE};
    use openbmt_core::wire::DecodeError;

    let mut all: Vec<(String, Words)> = Vec::new();
    let mut add = |name: &str, words: Words| all.push((name.to_string(), words));

    add("BoxStatus::Unpaired", BoxStatus::Unpaired.words());
    add(
        "BoxStatus::InvitationOpen",
        BoxStatus::InvitationOpen.words(),
    );
    add("BoxStatus::Paired", BoxStatus::Paired.words());
    add(
        "BoxStatus::Unpaired on the page",
        BoxStatus::Unpaired.page_words(),
    );
    add(
        "PageAnswer::InvitationHeld",
        PageAnswer::InvitationHeld.words(),
    );
    add(
        "PageAnswer::NotFromThisPage",
        PageAnswer::NotFromThisPage.words(),
    );
    add("PageAnswer::SetupWrong", PageAnswer::SetupWrong.words());
    add(
        "BoxStatus::TurnedOff and BoxError::TurnedOff and NoteKind::TurnedOff",
        BoxStatus::TurnedOff.words(),
    );

    add(
        "BoxError::Damaged and StartFailure::State: key",
        BoxError::Damaged {
            file: KEY_FILE,
            why: "missing",
        }
        .words(),
    );
    add(
        "BoxError::Damaged and StartFailure::State: roots",
        BoxError::Damaged {
            file: ROOTS_FILE,
            why: "not the two roots",
        }
        .words(),
    );
    add(
        "BoxError::Io and StartFailure::State",
        BoxError::Io {
            file: SETTINGS_FILE,
            op: "write",
            kind: io::ErrorKind::PermissionDenied,
        }
        .words(),
    );
    add("BoxError::NoRandomness", BoxError::NoRandomness.words());
    add("BoxError::AlreadyPaired", BoxError::AlreadyPaired.words());
    add(
        "BoxError::Invitation and InviteTextError::BadLength",
        BoxError::Invitation(InviteTextError::BadLength(12)).words(),
    );
    add(
        "BoxError::Invitation and InviteTextError::BadCharacter",
        BoxError::Invitation(InviteTextError::BadCharacter('U')).words(),
    );
    add(
        "BoxError::Invitation and InviteTextError::BadCheck",
        BoxError::Invitation(InviteTextError::BadCheck).words(),
    );
    add("BoxError::NoInvitation", BoxError::NoInvitation.words());
    add(
        "BoxError::InvitationExpired",
        BoxError::InvitationExpired.words(),
    );
    add(
        "BoxError::BadWelcome",
        BoxError::BadWelcome(DecodeError::Truncated).words(),
    );
    add("BoxError::WelcomeMac", BoxError::WelcomeMac.words());
    add(
        "BoxError::BadSettings",
        BoxError::BadSettings(DecodeError::BadMagic).words(),
    );
    let settings = [
        (
            "SettingsError::OwnerSignatureInvalid",
            SettingsError::OwnerSignatureInvalid,
        ),
        (
            "SettingsError::ApproverRequired",
            SettingsError::ApproverRequired,
        ),
        (
            "SettingsError::ApproverSignatureInvalid",
            SettingsError::ApproverSignatureInvalid,
        ),
        (
            "SettingsError::StaleVersion",
            SettingsError::StaleVersion { current: 5, got: 3 },
        ),
        (
            // BX-18: a confirmation that is not a counter's of the settings
            // in force has these words too.
            "SettingsError::Invalid and BoxError::Confirmed",
            SettingsError::Invalid("a device is listed twice"),
        ),
    ];
    for (name, e) in settings {
        add(
            &format!("BoxError::Settings and {name}"),
            BoxError::Settings(e).words(),
        );
    }
    add("BoxError::NotListed", BoxError::NotListed.words());
    add("BoxError::TooManyMembers", BoxError::TooManyMembers.words());
    add("BoxError::NotPaired", BoxError::NotPaired.words());

    add(
        "StartFailure::Lock and LockError::Busy",
        StartFailure::Lock(LockError::Busy("/srv/box".into())).words(),
    );
    add(
        "StartFailure::Lock and LockError::Io",
        StartFailure::Lock(LockError::Io("permission denied".into())).words(),
    );
    add(
        "StartFailure::Relay",
        StartFailure::Relay("could not make relay: permission denied".into()).words(),
    );
    add(
        "StartFailure::Courier and StartError::NeedsNewPairing",
        StartFailure::Courier(StartError::NeedsNewPairing { file: "link-inbox" }).words(),
    );
    add(
        "StartFailure::Courier and StartError::Corrupt",
        StartFailure::Courier(StartError::Corrupt {
            file: "link-outbox",
        })
        .words(),
    );
    add(
        "StartFailure::Courier and StartError::Store",
        StartFailure::Courier(StartError::Store(openbmt_courier::StoreError {
            file: "link-inbox",
            op: "read",
            kind: io::ErrorKind::PermissionDenied,
        }))
        .words(),
    );
    add(
        "StartFailure::Listen",
        StartFailure::Listen(io::Error::from(io::ErrorKind::AddrInUse)).words(),
    );

    let answers = [
        ("InvitationTaken", ConsoleAnswer::InvitationTaken),
        ("Members: none", ConsoleAnswer::Members(0)),
        ("Members: some", ConsoleAnswer::Members(3)),
        ("NoNotes", ConsoleAnswer::NoNotes),
        (
            "UnknownCommand",
            ConsoleAnswer::UnknownCommand("help".into()),
        ),
        (
            "InvitationFileUnreadable",
            ConsoleAnswer::InvitationFileUnreadable {
                path: "/home/box/invitation.txt".into(),
                kind: io::ErrorKind::NotFound,
            },
        ),
        ("BadArguments", ConsoleAnswer::BadArguments),
        ("BadAddress", ConsoleAnswer::BadAddress("localhost".into())),
    ];
    for (name, answer) in answers {
        add(&format!("ConsoleAnswer::{name}"), answer.words());
    }

    for (name, line) in SAMPLE_NOTES {
        add(name, note_words(line));
    }
    all
}

/// One stored note for every kind of note, as its writer words it, with the
/// name of the entry in [`all_words`].
const SAMPLE_NOTES: [(&str, &str); 5] = [
    (
        "NoteKind::MemberListNotReplaced",
        "the member list was not replaced: could not write members: permission denied",
    ),
    (
        "NoteKind::LinkFiles",
        "the Box's link files: LK-53: link-inbox does not read back",
    ),
    (
        "NoteKind::MessageNotHandled",
        "a message of kind Records is not handled yet",
    ),
    (
        "NoteKind::SettingsRefused",
        "settings from the owner refused: BX-21: owner signature missing or wrong",
    ),
    ("NoteKind::Other", "the relay did not answer"),
];

/// The text of `docs/screens/box.md`: one table with a row for every
/// entry of [`all_words`].
pub fn screens_markdown() -> String {
    let who = |w: Who| match w {
        Who::Nobody => "nobody",
        Who::Customer => "the customer",
        Who::Cashier => "the cashier",
        Who::Manager => "a manager",
        Who::Owner => "the owner",
        Who::KeyHolder => "the key holder",
        Who::BoxKeeper => "the Box keeper",
    };
    let cell = |text: &str| text.replace('|', "/");
    let mut out = String::from(
        "# Box console: what a person is told\n\n\
         Made by `screens_markdown` in `crates/openbmt-box/src/say.rs`. Do not\n\
         edit it by hand: run the test `sc_12_the_list_is_on_disk` with the\n\
         environment variable `OPENBMT_WRITE_SCREENS=1` to write it again.\n\n\
         | State | Heading | What happened | What to do next | Who | Code |\n\
         |---|---|---|---|---|---|\n",
    );
    for (name, words) in all_words() {
        out.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} |\n",
            cell(&name),
            cell(&words.heading),
            cell(&words.what),
            cell(&words.next),
            who(words.who),
            words.code.unwrap_or("")
        ));
    }
    out
}
