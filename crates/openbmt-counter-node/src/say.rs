//! What the owner is told when a counter will not start or pairing stops,
//! and the list of every word the counter program can show
//! (`docs/SCREEN-RULES.md`, SC-01 to SC-12).
//!
//! The owner reads these while setting up a counter. The cashier's own words
//! are in `openbmt_counter::say`, the instant-acceptance words in
//! `openbmt_core::say_policy` and the watcher's in `openbmt_watch::say`;
//! [`program_words`] joins the four and [`screens_markdown`] writes them as
//! one table (`docs/screens/counter.md`).

use openbmt_core::approval::ChangeClass;
use openbmt_core::pairing::PairError;
use openbmt_core::pairing_own::OwnPairError;
use openbmt_core::say::{Tone, Who, Words};
use openbmt_core::settings::SettingsError;

use crate::confirm::{ChangeNotice, ConfirmRefused};
use crate::dirstore::DirStoreError;
use crate::node::{HeartbeatNotTaken, StartError};
use crate::paired::{PairRefused, RelayRefused};
use crate::paired_own::OwnPairRefused;
use crate::relaylink::LinkLine;
use crate::settings::{SettingsFault, SettingsLine, SettingsRefused};

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
            Self::Settings(e) => e.words(),
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

impl SettingsRefused {
    /// What the owner is told when settings the owner sent were not taken
    /// (CR-160, CR-161). Nothing was changed.
    pub fn words(&self) -> Words {
        match self {
            Self::Bytes(e) => Words::new(
                Tone::Ended,
                "Settings not readable",
                "What was sent as the shop's settings is not settings, so nothing was changed.",
                Who::Owner,
                "Nothing to do. The owner's phone sends them again by itself.",
            )
            .detail(e.to_string()),
            Self::Verify(SettingsError::ConfirmationRequired(ChangeClass::Redirect)) => {
                needs_wallet()
            }
            Self::Confirmation(_) | Self::NoConfirmationHere => bad_confirmation(),
            Self::Verify(e) => Words::new(
                Tone::Ended,
                "Settings refused",
                "The settings the owner sent are not signed as this counter needs, or do not go past the ones it has, so nothing was changed.",
                Who::Owner,
                "Check on the owner's phone that the change was approved on the Key app, then send it again.",
            )
            .detail(e.to_string()),
            Self::Store(e) => Words::new(
                Tone::Ended,
                "Settings not saved",
                "The counter could not write the owner's new settings to its data folder, so it kept the old ones.",
                Who::Owner,
                "Check that the data folder has room and may be written to. The owner's phone sends them again by itself.",
            )
            .detail(e.to_string()),
        }
        .code(self.rule())
    }
}

impl SettingsFault {
    /// What the owner is told when the counter will not start for what it
    /// holds of the settings (CR-163).
    pub fn words(&self) -> Words {
        let restore = "Put the counter's data folder back from a copy made after the last settings change, or pair the counter again from the Key app.";
        match self {
            Self::Read(e) => Words::new(
                Tone::Stop,
                "Saved settings not read",
                "The counter could not read the settings it saved from the owner, so it did not start.",
                Who::Owner,
                "Check that the data folder may be read, then start again. If it stays, restore it from a copy.",
            )
            .detail(e.to_string()),
            Self::Decode(e) => Words::new(
                Tone::Stop,
                "Saved settings unreadable",
                "The settings the counter saved from the owner do not read back, so it did not start.",
                Who::Owner,
                restore,
            )
            .detail(e.to_string()),
            Self::Verify(e) => Words::new(
                Tone::Stop,
                "Saved settings not valid",
                "The settings the counter saved from the owner are not signed as they must be, so it did not start.",
                Who::Owner,
                restore,
            )
            .detail(e.to_string()),
            Self::Rolled { .. } => Words::new(
                Tone::Stop,
                "Saved settings are old",
                "The counter's records show newer settings than the ones it saved, so it did not start rather than use old limits.",
                Who::Owner,
                restore,
            )
            .detail(self.to_string()),
        }
        .code(self.rule())
    }
}

/// CR-239: a change that sends payments somewhere else needs the wallet, not
/// a counter.
fn needs_wallet() -> Words {
    Words::new(
        Tone::Ended,
        "Change needs the wallet",
        "This change sends payments somewhere else, which only the shop's wallet can approve. That is not built yet, so nothing changed.",
        Who::Owner,
        "Keep this setting as it is for now.",
    )
    .code("CR-239")
}

/// CR-242: the confirmation that came with a change is not a counter's of
/// this shop.
fn bad_confirmation() -> Words {
    Words::new(
        Tone::Ended,
        "Confirmation refused",
        "The change came with a confirmation that is not from a counter of this shop, so nothing changed.",
        Who::Manager,
        "Ask the owner to have the change confirmed at a counter again.",
    )
    .code("CR-242")
}

impl ChangeNotice {
    /// What a manager is told about a change that came to nothing (CR-239,
    /// CR-242, CR-244).
    pub fn words(&self) -> Words {
        match self {
            Self::Dropped { .. } => Words::new(
                Tone::Ended,
                "Change not confirmed in time",
                "A change from the owner phone waited seven days and was dropped. Nothing changed.",
                Who::Manager,
                "Ask the owner to make the change again.",
            )
            .code("CR-239"),
            Self::BadConfirmation { .. } => bad_confirmation(),
            Self::NeedsWallet { .. } => needs_wallet(),
        }
    }
}

/// CR-241: what a manager is told when the confirmation was signed.
pub fn change_confirmed_words() -> Words {
    Words::new(
        Tone::Good,
        "Change confirmed",
        "The owner phone sends the change to every counter. It applies once it arrives.",
        Who::Nobody,
        "Nothing to do.",
    )
    .code("CR-241")
}

impl ConfirmRefused {
    /// What a manager is told when the confirmation was not signed.
    pub fn words(&self) -> Words {
        let words = match self {
            Self::NothingWaits => Words::new(
                Tone::Ended,
                "Nothing to confirm",
                "No change waits at this counter, or a newer one has taken its place, so nothing was confirmed.",
                Who::Manager,
                "Open Change to confirm again to see what waits now.",
            ),
            Self::NotWritten(_) => Words::new(
                Tone::Stop,
                "Confirmation not saved",
                "The counter could not save its confirmation, so nothing was confirmed and nothing changed.",
                Who::Manager,
                "Try again. If it fails again, tell the owner.",
            ),
        };
        words.code(self.rule()).detail(self.to_string())
    }
}

impl SettingsLine {
    /// What Home says about the settings (CR-164, CR-165).
    pub fn words(&self) -> Words {
        match self {
            Self::Removed => Words::new(
                Tone::Stop,
                "Counter removed",
                "The owner removed this counter from the shop. It cannot start a sale, but sales already open can finish.",
                Who::Manager,
                "Ask the owner. To use this counter again the owner must add it and pair it again.",
            )
            .code("CR-165"),
            Self::RestartNeeded => Words::new(
                Tone::Info,
                "Restart for settings",
                "The owner changed Tor or the Lightning node, which only take effect when the counter starts again.",
                Who::Nobody,
                "Nothing to do. The counter starts again by itself when no sale screen is open.",
            )
            .code("CR-164"),
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
    /// The person said the address differs from the wallet's first receive
    /// address in a pairing with the shop's own wallet (CR-228); the counter
    /// forgot the pairing before it made a key.
    AddressDiffers,
    /// The person said the four words differ in a pairing with the shop's own
    /// wallet (CR-229); the counter forgot the pairing.
    OwnDiffer,
}

impl PairStep {
    /// Every step.
    pub const ALL: [PairStep; 7] = [
        Self::ScanOffer,
        Self::CompareWords,
        Self::ScanOwner,
        Self::Paired,
        Self::Differ,
        Self::AddressDiffers,
        Self::OwnDiffer,
    ];

    /// The name of the step.
    pub fn name(&self) -> &'static str {
        match self {
            Self::ScanOffer => "ScanOffer",
            Self::CompareWords => "CompareWords",
            Self::ScanOwner => "ScanOwner",
            Self::Paired => "Paired",
            Self::Differ => "Differ",
            Self::AddressDiffers => "AddressDiffers",
            Self::OwnDiffer => "OwnDiffer",
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
            Self::AddressDiffers => Words::new(
                Tone::Stop,
                "Different wallet",
                "The address on this counter is not your wallet's first address, so the owner phone gave this counter a different wallet. Nothing was saved.",
                Who::Manager,
                "Check that the owner phone has your wallet, then start the pairing again from the owner phone.",
            )
            .code("CR-228"),
            Self::OwnDiffer => Words::new(
                Tone::Stop,
                "Words differ: stopped",
                "The four words did not match, so the counter forgot its key and did not pair. Someone may have changed a code on the way.",
                Who::Manager,
                "Tap They differ on the owner phone too, then start again from it.",
            )
            .code("CR-229"),
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
            Self::RelayNotSigned => stop(
                "Relay not signed",
                "The relay in the code from the owner's phone is not signed by that owner, so pairing stopped.",
                "Start again from the Key app: open Pair a counter and scan its code.",
            ),
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

impl OwnPairRefused {
    /// What the person at the counter or the owner is told when a step of
    /// pairing with the shop's own wallet, or a saved one, is not accepted
    /// (CR-227 to CR-233). Nothing was paired. A refusal whose words are
    /// those of a refusal of the Key app's ceremony says them.
    pub fn words(&self) -> Words {
        match self {
            Self::NoCeremony => stopped("Pairing not under way", "This counter has no pairing with the shop's wallet at this step. Closing or restarting the app forgets one not yet finished.", Who::Manager, "Start again from the owner phone: open Add a counter and scan its code."),
            Self::NoRandomness => stopped("No key made", "The phone gave no random numbers the counter can trust, so it made no key and the pairing with the shop's wallet did not start.", Who::Manager, "Try again. If it fails again, restart the phone and start again from the owner phone."),
            Self::Pair(e) => own_pair_error_words(e),
            Self::Bundle(e) => PairRefused::Bundle(e.clone()).words(),
            Self::Settings(_) => stopped("Settings not signed", "The first settings in the owner phone's code are not signed by that owner phone, or are not valid. Nothing was saved.", Who::Manager, "Start the pairing again from the owner phone."),
            Self::SettingsOld { .. } => stopped("Settings out of date", "The first settings in the owner phone's code are older than the ones it offered. Nothing was saved.", Who::Manager, "Update the owner phone, then start the pairing again."),
            Self::NotOnDevices => stopped("Counter not in the settings", "The first settings from the owner phone do not list this counter, so it could not take them. Nothing was saved.", Who::Manager, "Start the pairing again from the owner phone."),
            Self::NotOnCounters => stopped("Counter not set up to sell", "The first settings from the owner phone do not set this counter up to take sales. Nothing was saved.", Who::Manager, "Start the pairing again from the owner phone."),
            Self::RelayNotSigned => stopped("Relay not the owner's", "The relay in the owner phone's code is not signed by that owner phone, so it may not be the shop's. Nothing was saved.", Who::Manager, "Start the pairing again from the owner phone."),
            Self::Record(_) => stopped("Saved pairing damaged", "The pairing with the shop's wallet that this counter saved does not read back, so the counter did not start.", Who::Owner, "Put the counter's files back from a copy, or pair it again from the owner phone."),
            Self::RecordKey => stopped("Saved key damaged", "The pairing with the shop's wallet that this counter saved holds no usable key, so the counter did not start.", Who::Owner, "Put the counter's files back from a copy, or pair it again from the owner phone."),
        }
        .code(self.rule())
        .detail(self.to_string())
    }
}

/// Words for a step of an own-wallet pairing that stopped.
fn stopped(heading: &str, what: &str, who: Who, next: &str) -> Words {
    Words::new(Tone::Stop, heading, what, who, next)
}

/// The words for a step of an own-wallet pairing that stopped (CR-227 to
/// CR-232).
pub fn own_pair_error_words(e: &OwnPairError) -> Words {
    // The refusals the two ceremonies share say the same words.
    let shared = |e: PairError| pair_error_words(&e);
    match e {
        OwnPairError::NotMainNetwork => stopped("Not for real bitcoin", "The owner phone's code is for a test network, not for real bitcoin, so this counter did not pair.", Who::Manager, "Pair with an owner phone set up for real bitcoin."),
        OwnPairError::InvalidName => shared(PairError::InvalidName),
        OwnPairError::CounterIsOwner => stopped("Counter key refused", "The key this counter made is the owner phone's own key, which a counter may not have. Nothing was saved.", Who::Manager, "Start the pairing again from the owner phone."),
        OwnPairError::Decode(inner) => shared(PairError::Decode(inner.clone())),
        OwnPairError::BindingMismatch => stopped("Codes do not belong together", "The counter's code was made for a different code from the owner phone. Nothing was saved.", Who::Manager, "Start the pairing again from the owner phone and scan its newest code."),
        OwnPairError::RequestSignatureInvalid => shared(PairError::RequestSignatureInvalid),
        OwnPairError::NotConfirmed => shared(PairError::NotConfirmed),
        OwnPairError::ClockWentBack => shared(PairError::ClockWentBack),
        OwnPairError::Expired => shared(PairError::Expired),
        OwnPairError::WrongWallet => Words::new(
            Tone::Stop,
            "Wrong wallet",
            "The owner phone's code is for a different wallet than the one this counter checked. Nothing was saved.",
            Who::Manager,
            "Start the pairing again from the owner phone.",
        ),
        OwnPairError::WrongCounter => shared(PairError::WrongCounter),
        OwnPairError::WrongNonce => shared(PairError::WrongNonce),
        OwnPairError::WrongName => shared(PairError::WrongName),
        OwnPairError::OwnerSignatureInvalid => shared(PairError::OwnerSignatureInvalid),
        OwnPairError::NotTheOwner => shared(PairError::NotTheOwner),
        OwnPairError::Bundle(_) => shared(PairError::Decode(
            openbmt_core::wire::DecodeError::NonCanonical("the peers"),
        )),
        OwnPairError::Range(_) => stopped("Addresses not signed", "The addresses the owner phone gave this counter are not signed for it, or are not a proper range. Nothing was saved.", Who::Manager, "Start the pairing again from the owner phone."),
        OwnPairError::RangeTooSmall { .. } => Words::new(
            Tone::Stop,
            "Too few addresses",
            "The owner phone gave this counter fewer than 1,000 addresses, the least it takes. Nothing was saved.",
            Who::Manager,
            "Update the owner phone, then start the pairing again.",
        ),
    }
}

impl LinkLine {
    /// What the counter's screen says about the link to the owner (CR-136).
    /// The link never holds up a sale, and the words say so.
    pub fn words(&self) -> Words {
        match *self {
            Self::Connecting => Words::new(
                Tone::Info,
                "Connecting to the Box",
                "The counter is reaching the owner through the shop's Box. Sales do not wait for it.",
                Who::Nobody,
                "Nothing to do. This line changes when the Box answers.",
            ),
            Self::Connected => Words::new(
                Tone::Info,
                "Connected to the Box",
                "The counter can send its records to the owner and take the owner's heartbeat.",
                Who::Nobody,
                "Nothing to do.",
            ),
            Self::NotReachable { since } => {
                let words = Words::new(
                    Tone::Info,
                    "Box not reachable",
                    "The counter cannot reach the Box, so it cannot send its records to the owner or take the owner's heartbeat.",
                    Who::Owner,
                    "Check that the Box is on and on the same network. The counter tries again by itself.",
                );
                match since {
                    Some(at) => words.at(at),
                    None => words,
                }
            }
            Self::Refused => Words::new(
                Tone::Info,
                "Box refused this counter",
                "The Box answered but did not accept this counter, so nothing goes to the owner through it.",
                Who::Owner,
                "Tell the owner. The owner's phone must list this counter in the shop's settings.",
            ),
            Self::NeedsTor => Words::new(
                Tone::Info,
                "Orbot needed for the Box",
                "The Box is reached over Tor and Orbot is not running, so the counter cannot reach it.",
                Who::Manager,
                "Open Orbot and turn it on. The counter tries again by itself.",
            ),
            Self::NotStarted => Words::new(
                Tone::Info,
                "Box link did not start",
                "The counter could not open the files of its link to the Box, so it does not talk to the owner. Sales are not affected.",
                Who::Manager,
                "Close the counter and open it again. If it stays, tell the owner.",
            ),
        }
        .code("CR-136")
    }
}

impl RelayRefused {
    /// What the owner is told when the counter does not take a relay for
    /// counters (CR-132). Nothing was changed.
    pub fn words(&self) -> Words {
        match self {
            Self::Bytes(e) => Words::new(
                Tone::Ended,
                "Box code not readable",
                "The code is not what the counter expects for the owner's Box, so nothing was changed.",
                Who::Owner,
                "Show the Box code on the owner's phone again and scan it again.",
            )
            .detail(e.to_string()),
            Self::NotFromOwner => Words::new(
                Tone::Ended,
                "Box code not from the owner",
                "The code was not signed by the owner's phone this counter was paired with, so nothing was changed.",
                Who::Owner,
                "Show the Box code on the owner's own phone and scan it again.",
            ),
        }
        .code(self.rule())
    }
}

/// One value of every line the link can show.
pub fn every_link_line() -> Vec<(&'static str, LinkLine)> {
    vec![
        ("Connecting", LinkLine::Connecting),
        ("Connected", LinkLine::Connected),
        (
            "NotReachable",
            LinkLine::NotReachable {
                since: Some(1_700_000_000),
            },
        ),
        ("NotReachableNoTime", LinkLine::NotReachable { since: None }),
        ("Refused", LinkLine::Refused),
        ("NeedsTor", LinkLine::NeedsTor),
        ("NotStarted", LinkLine::NotStarted),
    ]
}

/// One value of every refusal of a relay for counters.
pub fn every_relay_refused() -> Vec<(&'static str, RelayRefused)> {
    use openbmt_core::wire::DecodeError;
    vec![
        ("Bytes", RelayRefused::Bytes(DecodeError::Truncated)),
        ("NotFromOwner", RelayRefused::NotFromOwner),
    ]
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
    // CR-112 to CR-116: the words of the Lightning driver.
    all.extend(crate::lightning::all_words());
    all.extend(crate::swap::all_words());
    for (name, e) in [
        ("NothingWaits", crate::confirm::ConfirmRefused::NothingWaits),
        (
            "NotWritten",
            crate::confirm::ConfirmRefused::NotWritten("records.bin: full".to_string()),
        ),
    ] {
        all.push((format!("ConfirmRefused::{name}"), e.words()));
    }
    for (name, e) in crate::ownnode::every_setting_refusal() {
        all.push((format!("OwnNodeSettingRefused::{name}"), e.words()));
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
    // CR-227 to CR-233: only the words that the Key app's ceremony does not
    // already say are listed again; the shared ones are listed above.
    for (name, e) in every_own_pair_refused() {
        if own_words_are_new(&e) {
            all.push((format!("OwnPairRefused::{name}"), e.words()));
        }
    }
    for (name, line) in every_link_line() {
        // The one that has no time is the same state as the one that has:
        // listed once.
        if name != "NotReachableNoTime" {
            all.push((format!("LinkLine::{name}"), line.words()));
        }
    }
    for (name, e) in every_relay_refused() {
        all.push((format!("RelayRefused::{name}"), e.words()));
    }
    for (name, e) in every_settings_refused() {
        all.push((format!("SettingsRefused::{name}"), e.words()));
    }
    for (name, e) in every_settings_fault() {
        all.push((format!("SettingsFault::{name}"), e.words()));
    }
    for line in [SettingsLine::Removed, SettingsLine::RestartNeeded] {
        all.push((format!("SettingsLine::{line:?}"), line.words()));
    }
    // CR-239, CR-241: the dropped change and the confirmation signed. The
    // other two notices say what a refusal of settings says and are listed
    // there.
    all.push((
        "ChangeNotice::Dropped".to_string(),
        ChangeNotice::Dropped { version: 2 }.words(),
    ));
    all.push(("ChangeConfirmed".to_string(), change_confirmed_words()));
    all
}

/// Whether the words of an own-wallet refusal are its own and not those of a
/// refusal of the Key app's ceremony.
pub fn own_words_are_new(e: &OwnPairRefused) -> bool {
    match e {
        OwnPairRefused::Pair(inner) => matches!(
            inner,
            OwnPairError::NotMainNetwork
                | OwnPairError::CounterIsOwner
                | OwnPairError::BindingMismatch
                | OwnPairError::WrongWallet
                | OwnPairError::Range(_)
                | OwnPairError::RangeTooSmall { .. }
        ),
        OwnPairRefused::Bundle(_) => false,
        _ => true,
    }
}

/// One value of every refusal of an own-wallet pairing step, with its name.
pub fn every_own_pair_refused() -> Vec<(&'static str, OwnPairRefused)> {
    use openbmt_core::pinned_wallet::RangeError;
    use openbmt_core::wire::DecodeError;
    vec![
        ("NoCeremony", OwnPairRefused::NoCeremony),
        ("NoRandomness", OwnPairRefused::NoRandomness),
        (
            "Pair/NotMainNetwork",
            OwnPairRefused::Pair(OwnPairError::NotMainNetwork),
        ),
        (
            "Pair/CounterIsOwner",
            OwnPairRefused::Pair(OwnPairError::CounterIsOwner),
        ),
        (
            "Pair/BindingMismatch",
            OwnPairRefused::Pair(OwnPairError::BindingMismatch),
        ),
        (
            "Pair/WrongWallet",
            OwnPairRefused::Pair(OwnPairError::WrongWallet),
        ),
        (
            "Pair/Range",
            OwnPairRefused::Pair(OwnPairError::Range(RangeError::BadSignature)),
        ),
        (
            "Pair/RangeTooSmall",
            OwnPairRefused::Pair(OwnPairError::RangeTooSmall {
                got: 999,
                need: 1_000,
            }),
        ),
        ("Bundle", OwnPairRefused::Bundle(DecodeError::Truncated)),
        (
            "Settings",
            OwnPairRefused::Settings(SettingsError::OwnerSignatureInvalid),
        ),
        (
            "SettingsOld",
            OwnPairRefused::SettingsOld { floor: 4, got: 4 },
        ),
        ("NotOnDevices", OwnPairRefused::NotOnDevices),
        ("NotOnCounters", OwnPairRefused::NotOnCounters),
        ("RelayNotSigned", OwnPairRefused::RelayNotSigned),
        ("Record", OwnPairRefused::Record(DecodeError::Truncated)),
        ("RecordKey", OwnPairRefused::RecordKey),
    ]
}

/// One value of every refusal of settings from the owner, with its name.
pub fn every_settings_refused() -> Vec<(&'static str, SettingsRefused)> {
    use openbmt_core::wire::DecodeError;
    vec![
        ("Bytes", SettingsRefused::Bytes(DecodeError::Truncated)),
        (
            "Verify",
            SettingsRefused::Verify(SettingsError::ApproverRequired),
        ),
        (
            "ConfirmationRequired",
            SettingsRefused::Verify(SettingsError::ConfirmationRequired(ChangeClass::Redirect)),
        ),
        (
            "Confirmation",
            SettingsRefused::Confirmation(openbmt_core::approval::ConfirmError::NotACounter),
        ),
        (
            "Store",
            SettingsRefused::Store(DirStoreError::Io {
                file: "settings.bin".to_string(),
                op: "write",
                kind: std::io::ErrorKind::PermissionDenied,
            }),
        ),
    ]
}

/// One value of every fault of the settings a start reads, with its name.
pub fn every_settings_fault() -> Vec<(&'static str, SettingsFault)> {
    use openbmt_core::wire::DecodeError;
    vec![
        (
            "Read",
            SettingsFault::Read(DirStoreError::Io {
                file: "settings.bin".to_string(),
                op: "read",
                kind: std::io::ErrorKind::PermissionDenied,
            }),
        ),
        ("Decode", SettingsFault::Decode(DecodeError::Truncated)),
        (
            "Verify",
            SettingsFault::Verify(SettingsError::OwnerSignatureInvalid),
        ),
        (
            "Rolled",
            SettingsFault::Rolled {
                in_force: 3,
                recorded: 5,
            },
        ),
    ]
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
        ("SettingsOld", PairRefused::SettingsOld { floor: 4, got: 4 }),
        ("RelayNotSigned", PairRefused::RelayNotSigned),
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
