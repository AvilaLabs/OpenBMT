//! What the person at the Box console is told (`docs/SCREEN-RULES.md`,
//! SC-01 to SC-12).
//!
//! Every state and every refusal of the Box becomes one
//! [`Words`](openbmt_core::say::Words). The `Display` texts with rule
//! numbers stay for logs; the console prints [`render`]: the heading, what
//! happened, "Next: " and what to do, then "Details: " with the rule and the
//! detail, then "When: " with a time and a line for text that came from
//! elsewhere. Values a script needs keep their own stable lines
//! (`state: `, `members: `, `member: `, `pos: `, `note: `,
//! `invitation: `), printed after the words.
//!
//! The Box's reader is the Box keeper, who knows how to type a command.

use std::io;

use openbmt_core::boxpair::InviteTextError;
use openbmt_core::identity::DeviceId;
use openbmt_core::say::{Quoted, Tone, Who, Words};
use openbmt_core::settings::SettingsError;
use openbmt_courier::StartError;
use openbmt_net_io::env::LockError;
use openbmt_pos::SettingsError as PosSettingsError;

use crate::pos::{PosOpenError, PosState, Reading, POLL_SECS};
use crate::runner::{PosView, StartFailure};
use crate::state::{
    BoxError, BoxStatus, INVITE_SECS, MAX_MEMBERS, MAX_TOKEN_BYTES, POS_TOKEN_FILE,
};

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

/// The words for a POS token that is empty, too long, not one line or not
/// text (BX-41).
fn token_words(fault: TokenFault) -> Words {
    match fault {
        TokenFault::Empty => stop(
            "POS token is empty",
            "The POS token is empty.".into(),
            Who::BoxKeeper,
            "Type pos-token, then type the token on the next line.",
        ),
        TokenFault::TooLong => stop(
            "POS token is too long",
            format!("The POS token is longer than the {MAX_TOKEN_BYTES} bytes the Box keeps."),
            Who::BoxKeeper,
            "Copy only the token from the till system, then use pos-token again.",
        ),
        TokenFault::NotOneLine => stop(
            "POS token not one line",
            "The POS token has more than one line or a character that cannot be shown.".into(),
            Who::BoxKeeper,
            "Paste only the token, on one line, after the pos-token command.",
        ),
        TokenFault::NotText => stop(
            "POS token file not text",
            format!("The file {POS_TOKEN_FILE} is not plain text."),
            Who::BoxKeeper,
            "Enter the token again with the pos-token command, which replaces the file.",
        ),
    }
    .code("BX-41")
}

/// How a POS token was found wanting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TokenFault {
    Empty,
    TooLong,
    NotOneLine,
    NotText,
}

impl TokenFault {
    /// The fault for a reason the Box gives (`BoxError::BadToken`). A reason
    /// not known is the fault that covers the rest: not one line of plain
    /// text.
    fn of(why: &str) -> Self {
        match why {
            "empty" => Self::Empty,
            "too long" => Self::TooLong,
            "not text" => Self::NotText,
            _ => Self::NotOneLine,
        }
    }
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
            Self::TokenMode { mode } => stop(
                "POS token file too open",
                format!("The file {POS_TOKEN_FILE} can be read by other users, so the Box does not read the POS."),
                Who::BoxKeeper,
                format!("Type chmod 600 {POS_TOKEN_FILE} in the Box's data folder; the Box then reads it by itself."),
            )
            .code("BX-41")
            .detail(format!("mode {mode:04o}")),
            Self::BadToken(why) => token_words(TokenFault::of(why)),
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
        SettingsError::Invalid(_) => stop(
            "Settings break a rule",
            "The new settings break one of the rules every settings must keep.".into(),
            Who::Owner,
            "Change the settings on the owner's phone and send them again.",
        ),
    }
    .code("BX-21")
    .detail(detail)
}

/// What a person is told about POS settings that are not valid.
fn pos_settings_words(e: &PosSettingsError) -> Words {
    let detail = e.to_string();
    let (what, code) = match e {
        PosSettingsError::TenderNames => (
            "The till link is switched off because the POS settings have no usable name for bitcoin payments.",
            Some("PS-10"),
        ),
        PosSettingsError::Currencies => (
            "The till link is switched off because the POS settings list no currency, too many, or one twice.",
            Some("PS-13"),
        ),
        PosSettingsError::Locations => (
            "The till link is switched off because a till location is listed twice or a counter is in two places.",
            Some("PS-14"),
        ),
        PosSettingsError::Timing => (
            "The till link is switched off because a timing setting for the till link is zero or out of order.",
            None,
        ),
    };
    let words = stop(
        "Till link off: settings",
        what.into(),
        Who::BoxKeeper,
        "Ask whoever set up the till link to correct this, then start the Box again.",
    )
    .detail(detail);
    match code {
        Some(code) => words.code(code),
        None => words,
    }
}

impl PosOpenError {
    /// What a person is told when the POS side could not be opened. The
    /// relay runs on.
    pub fn words(&self) -> Words {
        match self {
            Self::Damaged { file, why } => stop(
                "Till link off: file damaged",
                format!("The till link is switched off because {file} is damaged, and the Box did not replace it."),
                Who::BoxKeeper,
                "Restore the file from a copy, or erase the data folder and pair the Box again.",
            )
            .code("BX-51")
            .detail(plain(why)),
            Self::Io { file, kind } => stop(
                "Till link off: file unread",
                format!("The till link is switched off because the Box could not read the file {file}."),
                Who::BoxKeeper,
                "Check that the disk works and this user may read the file, then start the Box again.",
            )
            .code("BX-50")
            .detail(format!("{kind}")),
            Self::Settings(e) => pos_settings_words(e),
        }
    }
}

impl Reading {
    /// What a person is told about the reading of the POS. `status` says
    /// why a Box that is `Stopped` is stopped: unpaired, turned off, or
    /// paired and not started yet.
    pub fn words(&self, status: BoxStatus) -> Words {
        match self {
            Self::Stopped => match status {
                BoxStatus::Unpaired | BoxStatus::InvitationOpen => Words::new(
                    Tone::Info,
                    "Till not read: not paired",
                    "The Box does not read the till system because it is not paired with a shop.",
                    Who::BoxKeeper,
                    "Pair the Box from the owner's phone first; the state command shows how far it is.",
                )
                .code("BX-03"),
                BoxStatus::TurnedOff => stop(
                    "Till not read: turned off",
                    "The Box does not read the till system because the owner's settings turned it off."
                        .into(),
                    Who::BoxKeeper,
                    "Erase the data folder and pair the Box again from the owner's phone.",
                )
                .code("BX-23"),
                BoxStatus::Paired => Words::new(
                    Tone::Info,
                    "Till link not started yet",
                    "The Box is paired but its till link has not started reading yet.",
                    Who::Nobody,
                    "Nothing to do. Run pos again in a few seconds.",
                )
                .code("BX-40"),
            },
            Self::NoToken => Words::new(
                Tone::Info,
                "No POS token yet",
                "The Box has no token for the till system, so it does not read it.",
                Who::BoxKeeper,
                "Type pos-token, then type the token on the next line.",
            )
            .code("BX-41"),
            Self::TokenRefused(why) => token_refused_words(why),
            Self::Reading => Words::new(
                Tone::Info,
                "Reading the till system",
                format!("The Box has its token and reads the till system every {POLL_SECS} seconds."),
                Who::Nobody,
                "Nothing to do. Run pos to see how the last reading went.",
            )
            .code("BX-40"),
        }
    }
}

/// The words for a token the Box refused, from the text the Box gave. A
/// reason that is known has the words of the refusal itself, so that one
/// fault reads one way; any other reason is shown under Details.
fn token_refused_words(why: &str) -> Words {
    let known = [
        BoxError::BadToken("empty"),
        BoxError::BadToken("too long"),
        BoxError::BadToken("not text"),
        BoxError::BadToken("more than one line or holds a control character"),
    ];
    if let Some(e) = known.iter().find(|e| e.to_string() == why) {
        return e.words();
    }
    let mode_prefix =
        format!("BX-41: {POS_TOKEN_FILE} can be read by its group or by others (mode ");
    if let Some(mode) = why
        .strip_prefix(&mode_prefix)
        .and_then(|rest| rest.split_once(')'))
        .and_then(|(mode, _)| u32::from_str_radix(mode, 8).ok())
    {
        return BoxError::TokenMode { mode }.words();
    }
    stop(
        "POS token not usable",
        format!(
            "The Box could not use the file {POS_TOKEN_FILE}, so it does not read the till system."
        ),
        Who::BoxKeeper,
        "Check the file, or enter the token again with the pos-token command.",
    )
    .code("BX-41")
    .detail(plain(why))
}

impl PosState {
    /// What a person is told about the POS side: how it is reading, when it
    /// last read, and the till system's own text if the last reading failed.
    pub fn words(&self, status: BoxStatus) -> Words {
        let base = self.reading.words(status);
        let words = match (&self.reading, &self.last_error, self.last_poll_at) {
            (Reading::Reading, Some(error), _) => Words::new(
                Tone::Stop,
                "Till reading failed",
                "The Box tried to read the till system and could not.",
                Who::BoxKeeper,
                "Check that the till system works and the Box can reach it; the Box tries again by itself.",
            )
            .code("BX-40")
            .quoted(Quoted::new("The Box noted", error)),
            (Reading::Reading, None, None) => Words::new(
                Tone::Info,
                "Waiting to read the till",
                "The Box has its token and has not read the till system yet.",
                Who::Nobody,
                format!("Nothing to do. It reads within {POLL_SECS} seconds."),
            )
            .code("BX-40"),
            _ => base,
        };
        match self.last_poll_at {
            Some(at) => words.at(at),
            None => words,
        }
    }

    /// The stable lines `pos` prints for scripts, one fact on each, without
    /// the token: the same as `Display`, except that a stopped Box says
    /// which of the three conditions it is in.
    pub fn console_lines(&self, status: BoxStatus) -> Vec<String> {
        let text = self.to_string();
        let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
        if self.reading == Reading::Stopped {
            let why = match status {
                BoxStatus::Unpaired | BoxStatus::InvitationOpen => "not paired",
                BoxStatus::TurnedOff => "turned off",
                BoxStatus::Paired => "not started",
            };
            if let Some(first) = lines.first_mut() {
                *first = format!("reading: no ({why})");
            }
        }
        lines
    }
}

impl PosView {
    /// What a person is told about the POS side of the Box.
    pub fn words(&self, status: BoxStatus) -> Words {
        match self {
            Self::NotInstalled => Words::new(
                Tone::Ended,
                "No till connector",
                "This Box program has no connector to a till system, so it reads none.",
                Who::Nobody,
                "Nothing to do here. Only a Box program built with a till connector reads a till system.",
            ),
            Self::Disabled(e) => e.words(),
            Self::State(state) => state.words(status),
        }
    }

    /// The stable lines `pos` prints for scripts.
    pub fn console_lines(&self, status: BoxStatus) -> Vec<String> {
        match self {
            Self::NotInstalled | Self::Disabled(_) => {
                self.to_string().lines().map(str::to_string).collect()
            }
            Self::State(state) => state.console_lines(status),
        }
    }
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
    /// `pos-token` stored the token.
    PosTokenStored,
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
            Self::PosTokenStored => Words::new(
                Tone::Good,
                "POS token stored",
                "The Box stored the POS token and will not print it.",
                Who::Nobody,
                "Nothing to do. Run pos to see whether the Box reads the till system.",
            )
            .code("BX-41"),
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
                "Type one of invite, state, pos-token, pos, members, notes or quit.",
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
                "The Box needs a data folder and an address to listen on, and may take an invitation file and --no-console.".into(),
                Who::BoxKeeper,
                "Type the data folder and the address; add --invitation-file and a file, or --no-console to run as a service.",
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

/// How a paid notice was refused (BX-40), as the Box's note says it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoticeWhy {
    /// The notice did not read.
    DoesNotRead,
    /// It names another counter than the one that sent it.
    OtherCounter,
    /// The key is not a counter in the settings.
    NotACounter,
    /// The signature does not check.
    BadSignature,
    /// The number is not above the last one.
    NumberNotAbove,
    /// The Box keeps numbers for too many counters.
    TooManyCounters,
    /// It disagrees with earlier notices about the sale.
    DoesNotAgree,
    /// The Box holds as many sales as it can.
    SalesFull,
    /// Any other reason.
    Other,
}

impl NoticeWhy {
    /// The reason a note gives.
    pub fn of(why: &str) -> Self {
        if why.starts_with("it does not read") {
            Self::DoesNotRead
        } else if why.starts_with("it names another counter") {
            Self::OtherCounter
        } else if why.starts_with("its key is not a counter") {
            Self::NotACounter
        } else if why.starts_with("its signature does not check") {
            Self::BadSignature
        } else if why.starts_with("its number ") {
            Self::NumberNotAbove
        } else if why.starts_with("the Box keeps numbers for too many") {
            Self::TooManyCounters
        } else if why.starts_with("BX-40: the notice gives the sale") {
            Self::DoesNotAgree
        } else if why.starts_with("PS-72: the Box holds") {
            Self::SalesFull
        } else {
            Self::Other
        }
    }

    fn words(self, detail: String) -> Words {
        let (what, who, next) = match self {
            Self::DoesNotRead => (
                "A counter's paid notice did not read, so the Box ignored it.",
                Who::Nobody,
                "Nothing to do. The counter sends the whole sale again with its next notice.",
            ),
            Self::OtherCounter => (
                "A paid notice named another counter than the one that sent it, so the Box ignored it.",
                Who::Owner,
                "Tell the owner: a device sent another counter's notice.",
            ),
            Self::NotACounter => (
                "A paid notice came from a device that is not a counter in the settings, so the Box ignored it.",
                Who::Owner,
                "If that device is a counter, tell the owner to add it to the settings.",
            ),
            Self::BadSignature => (
                "A paid notice carried a signature that does not check, so the Box ignored it.",
                Who::Owner,
                "Tell the owner if it keeps happening.",
            ),
            Self::NumberNotAbove => (
                "A paid notice repeated or went back in its numbering, so the Box ignored it.",
                Who::Nobody,
                "Nothing to do. An old or repeated notice changes nothing.",
            ),
            Self::TooManyCounters => (
                "The Box keeps numbers for as many counters as it can and ignored a notice from one more.",
                Who::Owner,
                "Tell the owner to remove counters the shop no longer uses.",
            ),
            Self::DoesNotAgree => (
                "A paid notice gave a sale another amount, currency or time than earlier notices, so it was ignored.",
                Who::Owner,
                "Tell the owner: a counter's records of one sale disagree.",
            ),
            Self::SalesFull => (
                "The Box holds as many sales as it can and could not make room for a new one.",
                Who::Owner,
                "Tell the owner: this Box is full of sales that are too recent to drop.",
            ),
            Self::Other => (
                "A paid notice was refused for a reason this Box has no plain words for.",
                Who::Owner,
                "Tell the owner what is under Details.",
            ),
        };
        // A notice that needs nobody ended with nothing received: not a stop.
        let tone = if who == Who::Nobody {
            Tone::Ended
        } else {
            Tone::Stop
        };
        let words = Words::new(tone, "Paid notice refused", what, who, next).code("BX-40");
        if detail.is_empty() {
            words
        } else {
            words.detail(detail)
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
    /// Paid notices arrived and the Box has no POS.
    PaidNoticesNoPos,
    /// The reports for the owner were not stored.
    ReportsNotStored,
    /// A report could not be sent to the owner.
    PosNoticeNotSent,
    /// A message of a kind the Box does not handle.
    MessageNotHandled,
    /// Settings from the owner were refused.
    SettingsRefused,
    /// A paid notice was refused.
    PaidNoticeRefused(NoticeWhy),
    /// The paid notices were not stored.
    PaidNoticesNotStored,
    /// The POS was not read.
    PosNotRead,
    /// The POS named no order to close.
    NoOrderToClose,
    /// A close was refused for a sale not paid now.
    CloseRefused,
    /// The order was already closed.
    OrderAlreadyClosed,
    /// The order was not closed.
    OrderNotClosed,
    /// The close attempt could not be saved, so the Box did not call the till.
    CloseAttemptNotSaved,
    /// The till closed the order, but the Box could not save that result.
    CloseResultNotSaved,
    /// Reports were dropped for room.
    ReportsDropped,
    /// The POS token could not be erased.
    TokenNotErased,
    /// The Box was turned off.
    TurnedOff,
    /// The POS side was left off at the start.
    PosSideOff,
    /// Too many closes are recorded.
    TooManyCloses,
    /// A note with no kind of its own, such as a message of the link.
    Other,
}

impl NoteKind {
    /// The kind of a stored note, and what follows the fixed part of its
    /// text. The notes are written by the runner and the POS side; each
    /// kind is the start of the text its writer gives.
    pub fn of(line: &str) -> (Self, &str) {
        let after = |prefix: &str| line.strip_prefix(prefix);
        if let Some(rest) = after("the member list was not replaced: ") {
            (Self::MemberListNotReplaced, rest)
        } else if let Some(rest) = after("the Box's link files: ") {
            (Self::LinkFiles, rest)
        } else if line.ends_with(" paid notices arrived and this Box has no POS: dropped") {
            (Self::PaidNoticesNoPos, line)
        } else if let Some(rest) = after("BX-50: the reports were not stored: ") {
            (Self::ReportsNotStored, rest)
        } else if let Some(rest) = after("a POS notice could not be sent to the owner: ") {
            (Self::PosNoticeNotSent, rest)
        } else if let Some(rest) = after("a message of kind ") {
            (Self::MessageNotHandled, rest)
        } else if let Some(rest) = after("settings from the owner refused: ") {
            (Self::SettingsRefused, rest)
        } else if let Some(rest) = after("BX-40: a paid notice from ") {
            let why = rest.split_once(" refused: ").map_or("", |(_, why)| why);
            (Self::PaidNoticeRefused(NoticeWhy::of(why)), rest)
        } else if let Some(rest) =
            after("BX-50: the paid notices were not stored and are dropped: ")
        {
            (Self::PaidNoticesNotStored, rest)
        } else if let Some(rest) = after("the POS was not read: ") {
            (Self::PosNotRead, rest)
        } else if let Some(rest) = after("the POS did not name an order to close: ") {
            (Self::NoOrderToClose, rest)
        } else if let Some(rest) = after("PS-81: an order is closed only") {
            (Self::CloseRefused, rest)
        } else if let Some(rest) = after("BX-43: order ") {
            if rest.contains(" was already closed for sale ") {
                (Self::OrderAlreadyClosed, rest)
            } else if rest.contains(" was not closed for sale ") {
                (Self::OrderNotClosed, rest)
            } else {
                (Self::Other, line)
            }
        } else if let Some(rest) = after("BX-50: the close attempt was not saved: ") {
            (Self::CloseAttemptNotSaved, rest)
        } else if let Some(rest) =
            after("BX-50: the close result was not saved after the till closed the order: ")
        {
            (Self::CloseResultNotSaved, rest)
        } else if let Some(rest) = after("BX-42: more than ") {
            (Self::ReportsDropped, rest)
        } else if let Some(rest) = after("BX-23: the POS token could not be erased: ") {
            (Self::TokenNotErased, rest)
        } else if line.starts_with("BX-23: the settings no longer list this Box") {
            (Self::TurnedOff, "")
        } else if let Some(rest) = after("the POS side is off: ") {
            (Self::PosSideOff, rest)
        } else if line.starts_with("PS-72: too many closes are recorded") {
            (Self::TooManyCloses, "")
        } else {
            (Self::Other, line)
        }
    }

    /// What a person is told about a note of this kind. `rest` is what
    /// [`NoteKind::of`] gave: the stored text after the fixed part. Text the
    /// till system gave is quoted, not part of the sentences.
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
            Self::PaidNoticesNoPos => {
                let count = rest
                    .split_whitespace()
                    .next()
                    .and_then(|n| n.parse::<u64>().ok());
                let what = match count {
                    Some(1) => "A paid notice arrived and was dropped, because this Box has no till link.".to_string(),
                    Some(n) => format!("{n} paid notices arrived and were dropped, because this Box has no till link."),
                    None => "Paid notices arrived and were dropped, because this Box has no till link.".to_string(),
                };
                Words::new(
                    Tone::Ended,
                    "Paid notices dropped",
                    what,
                    Who::Nobody,
                    "Nothing to do. Use a Box program built with a till connector to keep them.",
                )
            }
            Self::ReportsNotStored => stop(
                "Reports not saved",
                "The Box could not save the reports it keeps for the owner.".into(),
                Who::BoxKeeper,
                "Check that the disk has room and the data folder can be written.",
            )
            .code("BX-50")
            .detail(rest),
            Self::PosNoticeNotSent => Words::new(
                Tone::Info,
                "Report not sent",
                "The Box could not send a report about the till to the owner's phone.",
                Who::Nobody,
                "Nothing to do now. The Box sends it again by itself.",
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
            Self::PaidNoticeRefused(why) => {
                let sender = rest
                    .strip_prefix("DeviceId(")
                    .and_then(|r| r.split_once(".."))
                    .map(|(key, _)| format!("sender key {key}"))
                    .unwrap_or_default();
                why.words(sender)
            }
            Self::PaidNoticesNotStored => stop(
                "Paid notices not saved",
                "The Box could not save the paid notices that arrived, so it dropped them.".into(),
                Who::BoxKeeper,
                "Check that the disk has room and the data folder can be written.",
            )
            .code("BX-50")
            .detail(rest),
            Self::PosNotRead => Words::new(
                Tone::Stop,
                "Till system not read",
                "The Box could not read the till system on its last try.",
                Who::BoxKeeper,
                "Check that the till system works; the Box tries again by itself.",
            )
            .quoted(Quoted::new("The till system said", &rest)),
            Self::NoOrderToClose => Words::new(
                Tone::Info,
                "Order not named",
                "The till system did not name an order to close for a paid sale.",
                Who::Nobody,
                "Nothing to do. The Box asks again at its next turn.",
            )
            .quoted(Quoted::new("The till system said", &rest)),
            Self::CloseRefused => Words::new(
                Tone::Ended,
                "Order not closed",
                "The Box did not close an order, because the sale is not reported paid now.",
                Who::Nobody,
                "Nothing to do. An order is closed only for a sale that is paid.",
            )
            .code("PS-81"),
            Self::OrderAlreadyClosed => {
                let (order, sale) = rest
                    .split_once(" was already closed for sale ")
                    .unwrap_or((rest.as_str(), ""));
                let words = Words::new(
                    Tone::Ended,
                    "Order already closed",
                    "The till system says an order was already closed, so the Box did not close it again.",
                    Who::Nobody,
                    "Nothing to do. The order is closed.",
                )
                .code("BX-43")
                .quoted(Quoted::new("The till system's order", order));
                if sale.is_empty() {
                    words
                } else {
                    words.detail(format!("sale {sale}"))
                }
            }
            Self::OrderNotClosed => {
                let (order, tail) = rest
                    .split_once(" was not closed for sale ")
                    .unwrap_or((rest.as_str(), ""));
                let (sale, error) = tail.split_once(": ").unwrap_or((tail, ""));
                let words = Words::new(
                    Tone::Stop,
                    "Order not closed",
                    "The till system did not close an order for a paid sale.",
                    Who::BoxKeeper,
                    "Check that the till system works; the Box asks again by itself.",
                )
                .code("BX-43")
                .quoted(Quoted::new(
                    "The till system said",
                    &format!("order {order}: {error}"),
                ));
                if sale.is_empty() {
                    words
                } else {
                    words.detail(format!("sale {sale}"))
                }
            }
            Self::CloseAttemptNotSaved => stop(
                "Close attempt not saved",
                "The Box could not save its record before asking the till to close the order, so it did not ask the till.".into(),
                Who::BoxKeeper,
                "Check that the disk has room and the data folder can be written; the Box tries again by itself.",
            )
            .code("BX-50")
            .detail(rest),
            Self::CloseResultNotSaved => stop(
                "Close result not saved",
                "The till closed the order, but the Box could not save that result.".into(),
                Who::BoxKeeper,
                "Free disk space or fix folder access; the Box retries the same request. Check the till before closing by hand.",
            )
            .code("BX-50")
            .detail(rest),
            Self::ReportsDropped => Words::new(
                Tone::Stop,
                "Reports dropped",
                "More reports were waiting for the owner's phone than the Box keeps, so the oldest were dropped.",
                Who::Owner,
                "Check that the owner's phone is on and can reach the Box.",
            )
            .code("BX-42"),
            Self::TokenNotErased => stop(
                "POS token not erased",
                "The Box is turned off but could not erase the POS token it stored.".into(),
                Who::BoxKeeper,
                format!("Delete the file {POS_TOKEN_FILE} in the Box's data folder."),
            )
            .code("BX-23")
            .detail(rest),
            Self::TurnedOff => turned_off(),
            Self::PosSideOff => stop(
                "Till link off",
                "The Box switched its till link off when it started.".into(),
                Who::BoxKeeper,
                "Run pos to see why, then fix that.",
            )
            .detail(rest),
            Self::TooManyCloses => Words::new(
                Tone::Stop,
                "Too many closes kept",
                "The Box keeps as many closed orders as it can and made no new close.",
                Who::Owner,
                "Tell the owner: this Box's record of closed orders is full.",
            )
            .code("PS-72"),
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
// the example that adds a POS connector. Each gives the lines to print.

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

/// The answer to `pos-token`. No line after the command is the fault of an
/// empty token: `Err(BoxError::BadToken("empty"))`.
pub fn pos_token_lines(result: Result<(), BoxError>) -> Vec<String> {
    match result {
        Ok(()) => lines_of(&ConsoleAnswer::PosTokenStored.words()),
        Err(e) => lines_of(&e.words()),
    }
}

/// The answer to `pos`: the words, then the stable lines prefixed `pos: `.
pub fn pos_lines(view: &PosView, status: BoxStatus) -> Vec<String> {
    let mut lines = lines_of(&view.words(status));
    lines.extend(
        view.console_lines(status)
            .into_iter()
            .map(|l| format!("pos: {l}")),
    );
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

/// A sample state for the list of words: a `Reading` state that has been
/// read, with a time and nothing wrong.
fn sample_state(reading: Reading) -> PosState {
    PosState {
        reading,
        can_close: false,
        last_poll_at: Some(1_700_000_000),
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

/// SC-12: the words of every state and every refusal the Box console can
/// show, at least one for each variant and for each variant inside it.
/// A name that holds " and " stands for several states with one sentence
/// (SC-07: one fault reached two ways is one sentence); a name that holds
/// ": " says which sample of a variant it is.
pub fn all_words() -> Vec<(String, Words)> {
    use crate::pos::{LEDGER_FILE, NOTICES_FILE};
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
            "SettingsError::Invalid",
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
        "BoxError::TokenMode",
        BoxError::TokenMode { mode: 0o644 }.words(),
    );
    for (name, why) in [
        ("empty", "empty"),
        ("too long", "too long"),
        (
            "not one line",
            "more than one line or holds a control character",
        ),
        ("not text", "not text"),
    ] {
        add(
            &format!("BoxError::BadToken: {name}"),
            BoxError::BadToken(why).words(),
        );
    }

    let stopped = [
        ("unpaired", BoxStatus::Unpaired),
        ("turned off", BoxStatus::TurnedOff),
        ("not started", BoxStatus::Paired),
    ];
    for (name, status) in stopped {
        add(
            &format!("Reading::Stopped: {name}"),
            Reading::Stopped.words(status),
        );
    }
    add(
        "Reading::NoToken",
        Reading::NoToken.words(BoxStatus::Paired),
    );
    add(
        "Reading::TokenRefused",
        Reading::TokenRefused("BX-50: could not read pos.token: permission denied".into())
            .words(BoxStatus::Paired),
    );
    add(
        "PosView::State and Reading::Reading and PosState: reading",
        PosView::State(Box::new(sample_state(Reading::Reading))).words(BoxStatus::Paired),
    );
    let mut failing = sample_state(Reading::Reading);
    failing.last_error = Some("the POS was not read: the POS is not reachable".into());
    add(
        "PosState: reading, last reading failed",
        failing.words(BoxStatus::Paired),
    );
    let mut waiting = sample_state(Reading::Reading);
    waiting.last_poll_at = None;
    add(
        "PosState: reading, not read yet",
        waiting.words(BoxStatus::Paired),
    );
    add(
        "PosView::NotInstalled",
        PosView::NotInstalled.words(BoxStatus::Paired),
    );

    add(
        "PosOpenError::Damaged and PosView::Disabled",
        PosOpenError::Damaged {
            file: NOTICES_FILE,
            why: "message is cut short".into(),
        }
        .words(),
    );
    add(
        "PosOpenError::Io",
        PosOpenError::Io {
            file: LEDGER_FILE,
            kind: io::ErrorKind::PermissionDenied,
        }
        .words(),
    );
    for (name, e) in [
        ("TenderNames", PosSettingsError::TenderNames),
        ("Currencies", PosSettingsError::Currencies),
        ("Locations", PosSettingsError::Locations),
        ("Timing", PosSettingsError::Timing),
    ] {
        add(
            &format!("PosOpenError::Settings and PosSettingsError::{name}"),
            PosOpenError::Settings(e).words(),
        );
    }

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
        ("PosTokenStored", ConsoleAnswer::PosTokenStored),
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
const SAMPLE_NOTES: [(&str, &str); 29] = [
    (
        "NoteKind::MemberListNotReplaced",
        "the member list was not replaced: could not write members: permission denied",
    ),
    (
        "NoteKind::LinkFiles",
        "the Box's link files: LK-53: link-inbox does not read back",
    ),
    (
        "NoteKind::PaidNoticesNoPos",
        "2 paid notices arrived and this Box has no POS: dropped",
    ),
    (
        "NoteKind::ReportsNotStored",
        "BX-50: the reports were not stored: permission denied",
    ),
    (
        "NoteKind::PosNoticeNotSent",
        "a POS notice could not be sent to the owner: the relay refused",
    ),
    (
        "NoteKind::MessageNotHandled",
        "a message of kind PaidNotice is not handled yet",
    ),
    (
        "NoteKind::SettingsRefused",
        "settings from the owner refused: BX-21: owner signature missing or wrong",
    ),
    (
        "NoteKind::PaidNoticeRefused and NoticeWhy::DoesNotRead",
        "BX-40: a paid notice from DeviceId(ab12cd34..) refused: it does not read: cut short",
    ),
    (
        "NoteKind::PaidNoticeRefused and NoticeWhy::OtherCounter",
        "BX-40: a paid notice from DeviceId(ab12cd34..) refused: it names another counter than the one that sent it",
    ),
    (
        "NoteKind::PaidNoticeRefused and NoticeWhy::NotACounter",
        "BX-40: a paid notice from DeviceId(ab12cd34..) refused: its key is not a counter in the settings in force",
    ),
    (
        "NoteKind::PaidNoticeRefused and NoticeWhy::BadSignature",
        "BX-40: a paid notice from DeviceId(ab12cd34..) refused: its signature does not check",
    ),
    (
        "NoteKind::PaidNoticeRefused and NoticeWhy::NumberNotAbove",
        "BX-40: a paid notice from DeviceId(ab12cd34..) refused: its number 4 is not above 5",
    ),
    (
        "NoteKind::PaidNoticeRefused and NoticeWhy::TooManyCounters",
        "BX-40: a paid notice from DeviceId(ab12cd34..) refused: the Box keeps numbers for too many counters",
    ),
    (
        "NoteKind::PaidNoticeRefused and NoticeWhy::DoesNotAgree",
        "BX-40: a paid notice from DeviceId(ab12cd34..) refused: BX-40: the notice gives the sale another amount than earlier notices did",
    ),
    (
        "NoteKind::PaidNoticeRefused and NoticeWhy::SalesFull",
        "BX-40: a paid notice from DeviceId(ab12cd34..) refused: PS-72: the Box holds 16384 sales and none can be dropped",
    ),
    (
        "NoteKind::PaidNoticeRefused and NoticeWhy::Other",
        "BX-40: a paid notice from DeviceId(ab12cd34..) refused: something new",
    ),
    (
        "NoteKind::PaidNoticesNotStored",
        "BX-50: the paid notices were not stored and are dropped: permission denied",
    ),
    (
        "NoteKind::PosNotRead",
        "the POS was not read: the POS is not reachable (0 pages were kept)",
    ),
    (
        "NoteKind::NoOrderToClose",
        "the POS did not name an order to close: the POS is not reachable",
    ),
    (
        "NoteKind::CloseRefused",
        "PS-81: an order is closed only for a sale reported paid now (CR-30)",
    ),
    (
        "NoteKind::OrderAlreadyClosed",
        "BX-43: order o2 was already closed for sale 2",
    ),
    (
        "NoteKind::OrderNotClosed",
        "BX-43: order o2 was not closed for sale 2: the POS refused the close",
    ),
    (
        "NoteKind::CloseAttemptNotSaved",
        "BX-50: the close attempt was not saved: permission denied",
    ),
    (
        "NoteKind::CloseResultNotSaved",
        "BX-50: the close result was not saved after the till closed the order: permission denied",
    ),
    (
        "NoteKind::ReportsDropped",
        "BX-42: more than 256 reports are waiting for the relay: the oldest were dropped",
    ),
    (
        "NoteKind::TokenNotErased",
        "BX-23: the POS token could not be erased: permission denied",
    ),
    (
        "NoteKind::PosSideOff",
        "the POS side is off: BX-51: pos-notices is damaged (message is cut short): the Box does not run the POS",
    ),
    (
        "NoteKind::TooManyCloses",
        "PS-72: too many closes are recorded: none is made",
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
