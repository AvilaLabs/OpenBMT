//! What the owner is told when a counter will not start or pairing stops,
//! and the list of every word the counter program can show
//! (`docs/SCREEN-RULES.md`, SC-01 to SC-12).
//!
//! The owner reads these while setting up a counter. The cashier's own words
//! are in `openbmt_counter::say`, the instant-acceptance words in
//! `openbmt_core::say_policy` and the watcher's in `openbmt_watch::say`;
//! [`program_words`] joins the four and [`screens_markdown`] writes them as
//! one table (`docs/screens/counter.md`).

use openbmt_core::pairing::PairError;
use openbmt_core::say::{Tone, Who, Words};
use openbmt_core::settings::SettingsError;

use crate::dirstore::DirStoreError;
use crate::node::{HeartbeatNotTaken, StartError};
use crate::paired::PairRefused;

impl DirStoreError {
    /// What the owner is told about a file of the counter's data folder.
    pub fn words(&self) -> Words {
        let (heading, what, next) = match self {
            Self::Io { .. } => (
                "Cannot use data folder",
                "The operating system refused an action on a file in the counter's data folder, so it did not start.",
                "Check that the folder exists, has room and may be written to, then start again. See Details.",
            ),
            Self::Corrupt { .. } => (
                "Data file unreadable",
                "A file in the counter's data folder does not read back as it was written, so it did not start.",
                "Restart the counter. If it stays, the file needs to be put back from a copy. See Details.",
            ),
            Self::RecordTooLarge(_) => (
                "Saved record too large",
                "A saved record is longer than its file takes, so the counter did not start.",
                "Restart the counter. If it stays, the saved records need a look. See Details.",
            ),
        };
        Words::new(Tone::Stop, heading, what, Who::Owner, next).detail(self.to_string())
    }
}

impl StartError {
    /// What the owner is told when the counter will not start. Nothing was
    /// changed in any of these cases that the sentence does not say.
    pub fn words(&self) -> Words {
        match self {
            Self::Locked(e) => Words::new(
                Tone::Stop,
                "Counter already running",
                "Another copy of the counter, or another start, is using this counter's data folder.",
                Who::Owner,
                "Close the other copy and start the counter again.",
            )
            .code("NR-79")
            .detail(e.clone()),
            Self::Dir(e) => e.words(),
            Self::Headers(e) => Words::new(
                Tone::Stop,
                "Saved headers unreadable",
                "The counter's saved block headers could not be read at all, so it did not start.",
                Who::Owner,
                "Restart the counter. If it stays, the saved headers need to be replaced. See Details.",
            )
            .detail(e.clone()),
            Self::Counter(e) => e.words(),
            Self::FirstBatch(e) => Words::new(
                Tone::Stop,
                "First addresses not taken",
                "The first batch of payment codes the owner signed was not taken, so the counter did not start.",
                Who::Owner,
                "Check that the counter was given the owner's batch, then start it again. See Details.",
            )
            .detail(e.clone()),
            Self::Checked(e) => Words::new(
                Tone::Stop,
                "Checked height not saved",
                "The counter could not write how far it has checked the chain, so it did not start.",
                Who::Owner,
                "Check that the data folder has room and may be written to, then start again. See Details.",
            )
            .detail(e.to_string()),
            Self::Driver(e) => Words::new(
                Tone::Stop,
                "Network part did not start",
                "The part of the counter that connects to the network did not start, so the counter did not start.",
                Who::Owner,
                "Check the counter's internet connection and start it again. See Details.",
            )
            .detail(e.clone()),
        }
    }
}

impl HeartbeatNotTaken {
    /// What the owner is told when the counter did not take a check-in.
    pub fn words(&self) -> Words {
        match self {
            Self::Bytes(e) => Words::new(
                Tone::Ended,
                "Check-in not readable",
                "What was sent as a check-in is not a check-in, so nothing was changed.",
                Who::Owner,
                "Nothing to do. The owner's phone sends a new check-in by itself.",
            )
            .code("CR-50")
            .detail(e.to_string()),
            Self::Refused(e) => e.words(),
        }
    }
}

/// The words for a step of pairing that stopped. Nothing was paired.
pub fn pair_error_words(e: &PairError) -> Words {
    let stop = |heading: &str, what: &str, next: &str| {
        Words::new(Tone::Ended, heading, what, Who::Owner, next)
    };
    match e {
        PairError::SameRoot => stop(
            "Owner and Key app are alike",
            "The owner's phone and the Key app are the same key, so pairing stopped.",
            "Set the Key app up on a different device from the owner's phone, then pair again.",
        ),
        PairError::CounterIsRoot => stop(
            "Counter key is not its own",
            "This counter's key is the owner's or the Key app's, so pairing stopped.",
            "Set this counter up again with a key of its own, then pair again.",
        ),
        PairError::InvalidName => stop(
            "Counter name not allowed",
            "The counter's name must be 1 to 32 characters of plain text, so pairing stopped.",
            "Choose another name and pair again.",
        ),
        PairError::UnsupportedNetwork => stop(
            "Network not supported",
            "The Key app's offer names a bitcoin network that a counter cannot be paired on, so pairing stopped.",
            "Use a Key app set up on the main network, a test network or regtest, then pair again.",
        ),
        PairError::Decode(inner) => stop(
            "Pairing code unreadable",
            "The scanned pairing code could not be read, so pairing stopped.",
            "Show the code again and scan it again.",
        )
        .detail(inner.to_string()),
        PairError::BindingMismatch => stop(
            "Request is for other keys",
            "The counter's request was made for other keys or another wallet than the Key app's offer, so pairing stopped.",
            "Start pairing again from the Key app and scan its new offer.",
        ),
        PairError::RequestSignatureInvalid => stop(
            "Request not from counter",
            "The request's signature does not match the counter's key, so pairing stopped.",
            "Start pairing again.",
        ),
        PairError::NotConfirmed => stop(
            "Words not confirmed",
            "The words on the two screens were not confirmed as the same, so pairing stopped.",
            "Start pairing again and check that both screens show the same four words.",
        ),
        PairError::ClockWentBack => stop(
            "Counter's clock went back",
            "The counter's clock is earlier than when pairing started, so pairing stopped.",
            "Check the counter's clock and pair again.",
        ),
        PairError::Expired => stop(
            "Pairing took too long",
            "Pairing took longer than ten minutes, so it stopped.",
            "Start pairing again and finish within ten minutes.",
        ),
        PairError::WrongWallet => stop(
            "Pairing for another wallet",
            "The pairing answer is for a different wallet than the one this counter was offered, so pairing stopped.",
            "Start pairing again with the right Key app.",
        ),
        PairError::WrongCounter => stop(
            "Pairing for another counter",
            "The pairing answer names a different counter, so pairing stopped.",
            "Start pairing again and scan the answer on the counter that showed the request.",
        ),
        PairError::WrongNonce => stop(
            "Pairing answer is old",
            "The pairing answer belongs to an earlier try, so pairing stopped.",
            "Start pairing again.",
        ),
        PairError::WrongName => stop(
            "Pairing name differs",
            "The pairing answer carries a different counter name, so pairing stopped.",
            "Start pairing again.",
        ),
        PairError::ApproverSignatureInvalid => stop(
            "Key app signature rejected",
            "The Key app's signature on the pairing answer does not check out, so pairing stopped.",
            "Start pairing again with the right Key app.",
        ),
        PairError::OwnerSignatureInvalid => stop(
            "Owner signature rejected",
            "The owner's signature on the pairing answer does not check out, so pairing stopped.",
            "Start pairing again with the right owner's phone.",
        ),
        PairError::NotTheApprover => stop(
            "Wrong Key app",
            "This is not the Key app named in the offer, so pairing stopped.",
            "Use the Key app that showed the offer.",
        ),
        PairError::NotTheOwner => stop(
            "Wrong owner phone",
            "This is not the owner's phone named in the offer, so pairing stopped.",
            "Use the owner's phone named in the offer.",
        ),
    }
}

/// The steps of pairing, as the counter's screen tells the owner (the
/// ceremony in `openbmt_core::pairing`, CR-91 to CR-99).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PairStep {
    /// The Key app shows an offer and the counter scans it.
    ScanOffer,
    /// The counter shows its code and four words, and waits for the person
    /// to compare them with the Key app's.
    CompareWords,
    /// The person said the words match; the counter scans the owner's phone.
    ScanOwner,
    /// The counter has taken the signed pairing.
    Paired,
    /// The person said the words differ; the counter forgot the pairing.
    Differ,
}

impl PairStep {
    /// Every step.
    pub const ALL: [PairStep; 5] = [
        Self::ScanOffer,
        Self::CompareWords,
        Self::ScanOwner,
        Self::Paired,
        Self::Differ,
    ];

    /// The name of the step.
    pub fn name(&self) -> &'static str {
        match self {
            Self::ScanOffer => "ScanOffer",
            Self::CompareWords => "CompareWords",
            Self::ScanOwner => "ScanOwner",
            Self::Paired => "Paired",
            Self::Differ => "Differ",
        }
    }

    /// What the owner is told at this step.
    pub fn words(&self) -> Words {
        match self {
            Self::ScanOffer => Words::new(
                Tone::Info,
                "Scan the Key app's code",
                "The counter is waiting for the offer that the Key app shows on its Pair a counter screen.",
                Who::Owner,
                "Hold the counter's camera to the Key app's screen.",
            ),
            Self::CompareWords => Words::new(
                Tone::Info,
                "Compare the four words",
                "The counter shows a code and four words. The owner's phone carries the code to the Key app, which shows four words too.",
                Who::Owner,
                "Scan this code with the owner's phone. If the Key app shows the same four words, tap They match here.",
            ),
            Self::ScanOwner => Words::new(
                Tone::Info,
                "Scan the owner's phone",
                "The words match. The counter waits for the code the owner's phone shows once the Key app has approved.",
                Who::Owner,
                "Hold the counter's camera to the owner's phone and keep it there until the code is read.",
            ),
            Self::Paired => Words::new(
                Tone::Good,
                "Counter is paired",
                "The counter took the pairing signed by the Key app and the owner's phone, and the shop's settings.",
                Who::Nobody,
                "Nothing to do. If no addresses came with it, the owner's phone sends them next.",
            ),
            Self::Differ => Words::new(
                Tone::Stop,
                "Words differ: stopped",
                "The four words did not match, so the counter forgot its key and its number and did not pair. Someone may have changed the code on the way.",
                Who::Owner,
                "Do not approve it on the Key app. Start again from the Key app: open Pair a counter and scan its code.",
            )
            .code("CR-92"),
        }
    }
}

impl PairRefused {
    /// What the owner is told when a step of pairing, or a saved pairing, is
    /// not accepted. Nothing was paired.
    pub fn words(&self) -> Words {
        let again = "Start again from the Key app: open Pair a counter and scan its code.";
        let stop = |heading: &str, what: &str, next: &str| {
            Words::new(Tone::Ended, heading, what, Who::Owner, next).code(self.rule())
        };
        match self {
            Self::NoCeremony => stop(
                "No pairing in progress",
                "The counter has no pairing under way. It may have been closed, restarted or used up, which forgets a pairing that was not finished.",
                again,
            ),
            Self::NoRandomness => stop(
                "Counter could not make a key",
                "The phone gave the counter no random numbers it can trust, so it did not make a key and pairing did not start.",
                "Try again. If it fails again, restart the phone and then pair from the Key app.",
            ),
            Self::Pair(e) => pair_error_words(e).code(self.rule()),
            Self::Bundle(e) => stop(
                "Owner code not readable",
                "The code from the owner's phone is not what the counter expects to finish pairing, so pairing is not finished.",
                "Show the code on the owner's phone again and scan it again.",
            )
            .detail(e.to_string()),
            Self::Settings(e) => stop(
                "Settings not signed",
                "The shop's settings that came with the pairing are not signed by both the owner's phone and the Key app, so pairing stopped.",
                again,
            )
            .detail(e.to_string()),
            Self::SettingsOld { .. } => stop(
                "Settings are too old",
                "The shop's settings that came with the pairing are older than the pairing itself, so pairing stopped.",
                again,
            ),
            Self::NotOnDevices => stop(
                "Counter not in settings",
                "The shop's settings do not list this counter, so pairing stopped.",
                "Have the owner's phone add this counter to the settings, then start again from the Key app.",
            ),
            Self::NotOnCounters => stop(
                "Counter has no share",
                "The shop's settings list this counter as a device but not among the counters that take payments, so pairing stopped.",
                "Have the owner's phone give this counter a share in the settings, then start again from the Key app.",
            ),
            Self::Batch(e) => stop(
                "First addresses refused",
                "The first batch of addresses does not fit this counter or this wallet, or is smaller than a counter takes, so pairing stopped.",
                "Have the owner's phone send a batch made for this counter, then start again from the Key app.",
            )
            .detail(e.to_string()),
            Self::Record(e) => stop(
                "Saved pairing not readable",
                "The pairing this counter saved does not read back, so the counter did not start.",
                "Restore the counter from its backup, or pair it again from the Key app.",
            )
            .detail(e.to_string()),
            Self::RecordKey => stop(
                "Saved pairing has no key",
                "The pairing this counter saved holds a secret that is not a key, so the counter did not start.",
                "Restore the counter from its backup, or pair it again from the Key app.",
            ),
        }
    }
}

/// Every one of the words that only this crate gives, for SC-12: one value
/// for every variant of the start errors, of the data folder errors, of the
/// pairing errors and of the pairing steps.
pub fn all_words() -> Vec<(String, Words)> {
    use openbmt_core::wire::DecodeError;
    use openbmt_counter::refusal::{HeartbeatRefused, LoadError};
    let mut all: Vec<(String, Words)> = Vec::new();
    let io = || DirStoreError::Io {
        file: "records.bin".to_string(),
        op: "write",
        kind: std::io::ErrorKind::PermissionDenied,
    };
    let corrupt = || DirStoreError::Corrupt {
        file: "desk.bin".to_string(),
        why: "the file is cut short",
    };
    for (name, e) in [
        ("Io", io()),
        ("Corrupt", corrupt()),
        ("RecordTooLarge", DirStoreError::RecordTooLarge(9_999_999)),
    ] {
        all.push((
            format!("StartError::Dir/{name}"),
            StartError::Dir(e).words(),
        ));
    }
    let starts = [
        (
            "Locked",
            StartError::Locked("the folder is locked".to_string()),
        ),
        ("Headers", StartError::Headers("cut short".to_string())),
        (
            "Counter",
            StartError::Counter(LoadError::Desk(DecodeError::Truncated)),
        ),
        (
            "FirstBatch",
            StartError::FirstBatch("not signed".to_string()),
        ),
        ("Checked", StartError::Checked(io())),
        ("Driver", StartError::Driver("no network".to_string())),
    ];
    for (name, e) in starts {
        all.push((format!("StartError::{name}"), e.words()));
    }
    all.push((
        "HeartbeatNotTaken::Bytes".to_string(),
        HeartbeatNotTaken::Bytes(DecodeError::Truncated).words(),
    ));
    all.push((
        "HeartbeatNotTaken::Refused".to_string(),
        HeartbeatNotTaken::Refused(HeartbeatRefused::Store(openbmt_counter::StoreFault::Write(
            corrupt(),
        )))
        .words(),
    ));
    for (name, e) in every_pair_error() {
        all.push((format!("PairError::{name}"), pair_error_words(&e)));
    }
    for step in PairStep::ALL {
        all.push((format!("PairStep::{}", step.name()), step.words()));
    }
    for (name, e) in every_pair_refused() {
        all.push((format!("PairRefused::{name}"), e.words()));
    }
    all
}

/// One value of every refusal of the pairing steps, with its name. The
/// `Pair` variant carries a pairing error, whose words are listed with the
/// pairing errors.
pub fn every_pair_refused() -> Vec<(&'static str, PairRefused)> {
    use openbmt_core::batch::BatchError;
    use openbmt_core::wire::DecodeError;
    vec![
        ("NoCeremony", PairRefused::NoCeremony),
        ("NoRandomness", PairRefused::NoRandomness),
        ("Bundle", PairRefused::Bundle(DecodeError::Truncated)),
        (
            "Settings",
            PairRefused::Settings(SettingsError::ApproverRequired),
        ),
        (
            "SettingsOld",
            PairRefused::SettingsOld { floor: 4, got: 4 },
        ),
        ("NotOnDevices", PairRefused::NotOnDevices),
        ("NotOnCounters", PairRefused::NotOnCounters),
        (
            "Batch",
            PairRefused::Batch(BatchError::CountTooSmall { min: 1_000, got: 3 }),
        ),
        ("Record", PairRefused::Record(DecodeError::Truncated)),
        ("RecordKey", PairRefused::RecordKey),
    ]
}

/// One value of every pairing error, with its name.
pub fn every_pair_error() -> Vec<(&'static str, PairError)> {
    use openbmt_core::wire::DecodeError;
    vec![
        ("SameRoot", PairError::SameRoot),
        ("CounterIsRoot", PairError::CounterIsRoot),
        ("InvalidName", PairError::InvalidName),
        ("UnsupportedNetwork", PairError::UnsupportedNetwork),
        ("Decode", PairError::Decode(DecodeError::Truncated)),
        ("BindingMismatch", PairError::BindingMismatch),
        (
            "RequestSignatureInvalid",
            PairError::RequestSignatureInvalid,
        ),
        ("NotConfirmed", PairError::NotConfirmed),
        ("ClockWentBack", PairError::ClockWentBack),
        ("Expired", PairError::Expired),
        ("WrongWallet", PairError::WrongWallet),
        ("WrongCounter", PairError::WrongCounter),
        ("WrongNonce", PairError::WrongNonce),
        ("WrongName", PairError::WrongName),
        (
            "ApproverSignatureInvalid",
            PairError::ApproverSignatureInvalid,
        ),
        ("OwnerSignatureInvalid", PairError::OwnerSignatureInvalid),
        ("NotTheApprover", PairError::NotTheApprover),
        ("NotTheOwner", PairError::NotTheOwner),
    ]
}

/// Every word the counter program can show: the counter's, this crate's, the
/// watcher's and the policy enums'. A value whose heading and first sentence
/// are already in the list under another name (the policy words are shown on
/// the sale screen, for one) is listed once, under the first name.
pub fn program_words() -> Vec<(String, Words)> {
    let mut all: Vec<(String, Words)> = Vec::new();
    let lists = [
        openbmt_counter::say::all_words(),
        all_words(),
        openbmt_watch::say::all_words(),
        openbmt_core::say_policy::all_words(),
    ];
    for (name, words) in lists.into_iter().flatten() {
        if !all
            .iter()
            .any(|(_, w)| w.heading == words.heading && w.what == words.what)
        {
            all.push((name, words));
        }
    }
    all
}

fn who_text(who: Who) -> &'static str {
    match who {
        Who::Nobody => "nobody",
        Who::Customer => "the customer",
        Who::Cashier => "the cashier",
        Who::Manager => "a manager",
        Who::Owner => "the owner",
        Who::KeyHolder => "the key holder",
        Who::BoxKeeper => "the Box keeper",
    }
}

fn cell(text: &str) -> String {
    text.replace('|', "/")
}

/// The list of every word the counter program can show, as one Markdown
/// table (`docs/screens/counter.md`). Run the test `sc_12_the_list_on_disk_is_current`
/// with `OPENBMT_WRITE_SCREENS=1` to write it again.
pub fn screens_markdown() -> String {
    let mut out = String::new();
    out.push_str("# Counter: every screen\n\n");
    out.push_str("What the counter program tells the cashier, a manager and the owner\n");
    out.push_str("(`docs/SCREEN-RULES.md`). The list is made from the code; do not edit it\n");
    out.push_str("by hand. To write it again, run the test\n");
    out.push_str("`sc_12_the_list_on_disk_is_current` of `openbmt-counter-node` with\n");
    out.push_str("`OPENBMT_WRITE_SCREENS=1`.\n\n");
    out.push_str("| state | heading | what happened | what to do next | who | code |\n");
    out.push_str("|---|---|---|---|---|---|\n");
    for (name, w) in program_words() {
        out.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} |\n",
            cell(&name),
            cell(&w.heading),
            cell(&w.what),
            cell(&w.next),
            who_text(w.who),
            w.code.unwrap_or("")
        ));
    }
    out
}
