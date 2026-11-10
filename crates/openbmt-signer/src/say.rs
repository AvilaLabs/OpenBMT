//! What the key holder is told when the signer refuses (`docs/SCREEN-RULES.md`,
//! SC-01 to SC-12).
//!
//! The `Display` texts of the refusals are for logs and tests. A person is
//! shown the [`Words`] these functions return. Every refusal here means that
//! nothing was signed and that no state changed.

use openbmt_core::say::{sats, Tone, Who, Words};
use openbmt_core::wallet::WalletError;

use crate::{
    Ceiling, ChangeRefusal, Refusal, RegisterError, MAX_FEE_RATE_SAT_PER_VB, MAX_FEE_SATS,
};

/// What the key holder does when a request looks changed on its way or
/// does not come from the owner's phone.
const TAMPERED: &str =
    "Do not approve it, and tell the owner in person, not through the phone that sent it.";

/// A refusal that suggests the request was changed or is not what the
/// owner's phone builds.
fn tampered(heading: &str, what: &str) -> Words {
    Words::new(Tone::Stop, heading, what, Who::KeyHolder, TAMPERED)
}

/// A position in the transaction for the small print, counted from 1.
fn position(kind: &str, index: usize) -> String {
    format!("{kind} {}", index.saturating_add(1))
}

impl Ceiling {
    /// Which limit this is, for a sentence: "for one payment to other
    /// addresses".
    fn plain(self) -> &'static str {
        match self {
            Self::OtherPerTx => "for one payment to other addresses",
            Self::OtherDay => "for a day to other addresses, fee included",
            Self::OtherWeek => "for a week to other addresses, fee included",
            Self::AllowlistedPerTx => "for one payment to saved addresses",
            Self::AllowlistedDay => "for a day to saved addresses",
            Self::AllowlistedWeek => "for a week to saved addresses",
        }
    }

    /// The heading of the refusal for this limit.
    fn heading(self) -> &'static str {
        match self {
            Self::OtherPerTx => "Over the one-payment limit",
            Self::OtherDay => "Over the day limit",
            Self::OtherWeek => "Over the week limit",
            Self::AllowlistedPerTx => "Over saved one-payment limit",
            Self::AllowlistedDay => "Over saved day limit",
            Self::AllowlistedWeek => "Over saved week limit",
        }
    }
}

impl Refusal {
    /// What the key holder is shown. Nothing was signed.
    pub fn words(&self) -> Words {
        let code = self.rule();
        let words = match self {
            Self::NotAPsbt => tampered(
                "Not a payment file",
                "This file is not a payment file in the form this app reads.",
            ),
            Self::FileTooLarge { len } => tampered(
                "Payment file too big",
                "The payment file is bigger than this app will read.",
            )
            .detail(format!("{len} bytes")),
            Self::InputCount { found } => tampered(
                "Wrong number of coins",
                "The payment spends no coins, or more than this app will sign for.",
            )
            .detail(format!("{found} inputs")),
            Self::OutputCount { found } => tampered(
                "Wrong number of payees",
                "The payment pays no one, or more places than this app will sign for.",
            )
            .detail(format!("{found} outputs")),
            Self::TransactionVersion { found } => tampered(
                "Unknown payment version",
                "This payment is written in a version this app does not sign.",
            )
            .detail(format!("version {found}")),
            Self::LockTimeNotHeight { lock_time } => tampered(
                "Payment has a time lock",
                "This payment cannot be used until a date, which this app does not allow.",
            )
            .detail(format!("lock time {lock_time}")),
            Self::RelativeTimeLock { input, sequence } => tampered(
                "Payment has a delay",
                "A coin in it can only be spent after a delay, which this app does not allow.",
            )
            .detail(format!(
                "{}, sequence {sequence:#x}",
                position("input", *input)
            )),
            Self::DuplicateInput { input } => tampered(
                "Coin listed twice",
                "One coin is listed twice in this payment.",
            )
            .detail(position("input", *input)),
            Self::InputAlreadyFinal { input } => tampered(
                "Coin already finished",
                "One coin in this payment is already marked as finished.",
            )
            .detail(position("input", *input)),
            Self::ForbiddenInputField { input, field } => tampered(
                "Extra data on a coin",
                "A coin carries extra data that this app never signs with.",
            )
            .detail(format!("{} carries {field}", position("input", *input))),
            Self::ForbiddenOutputField { output, field } => tampered(
                "Extra data on a payee",
                "A payee line carries extra data that this app never signs with.",
            )
            .detail(format!("{} carries {field}", position("output", *output))),
            Self::SighashType { input, found } => tampered(
                "Unusual signing mode",
                "A coin asks to be signed in a mode other than the usual one.",
            )
            .detail(format!("{}, type {found}", position("input", *input))),
            Self::ForeignXpub => tampered(
                "Key not from this wallet",
                "The file names a key that does not belong to this wallet.",
            ),
            Self::MissingPreviousTransaction { input } => tampered(
                "Coin history missing",
                "A coin comes without the earlier payment that created it.",
            )
            .detail(position("input", *input)),
            Self::PreviousTransactionMismatch { input } => tampered(
                "Coin history does not match",
                "The earlier payment shown for a coin is not the one the coin names.",
            )
            .detail(position("input", *input)),
            Self::PreviousOutputMissing { input, index } => tampered(
                "Coin not in its history",
                "A coin points at a part of the earlier payment that is not there.",
            )
            .detail(format!("{}, output {index}", position("input", *input))),
            Self::WitnessUtxoMismatch { input } => tampered(
                "Coin details disagree",
                "The amount shown for a coin differs from what its earlier payment says.",
            )
            .detail(position("input", *input)),
            Self::NoWalletKeyHint { input } => tampered(
                "Coin not tied to wallet",
                "A coin does not say which address of this wallet it spends.",
            )
            .detail(position("input", *input)),
            Self::HintKeyMismatch { input } => tampered(
                "Coin key does not match",
                "A coin names a wallet key that is not the key at that address.",
            )
            .detail(position("input", *input)),
            Self::HintsDisagree { input } => tampered(
                "Coin addresses disagree",
                "A coin names two or more different addresses of this wallet.",
            )
            .detail(position("input", *input)),
            Self::ScriptNotInWallet { input } => tampered(
                "Coin not from this wallet",
                "A coin in this payment is not at an address of this wallet.",
            )
            .detail(position("input", *input)),
            Self::WitnessScriptMismatch { input } => tampered(
                "Coin rules do not match",
                "A coin carries signing rules that are not this wallet's.",
            )
            .detail(position("input", *input)),
            Self::ForeignSignature { input } => tampered(
                "Signature from unknown key",
                "A coin already carries a signature from a key that is not in this wallet.",
            )
            .detail(position("input", *input)),
            Self::SignatureSighashType { input } => tampered(
                "Signature of the wrong kind",
                "A coin already carries a signature that covers less than the whole payment.",
            )
            .detail(position("input", *input)),
            Self::BadSignature { input } => tampered(
                "Signature does not check",
                "A coin already carries a signature that does not check out.",
            )
            .detail(position("input", *input)),
            Self::AlreadySigned => Words::new(
                Tone::Ended,
                "Already signed",
                "This app has already signed every coin in this payment.",
                Who::Nobody,
                "Nothing to do. Use the signed copy this app gave the owner's phone before.",
            ),
            Self::InputAmountTooLarge { input, amount } => tampered(
                "Coin amount impossible",
                "A coin is given an amount above all the bitcoin that can exist.",
            )
            .detail(format!("{}, {}", position("input", *input), sats(*amount))),
            Self::InputTotalOverflow => tampered(
                "Coin total impossible",
                "The coins in this payment add up to more than can exist.",
            ),
            Self::OutputHintKeyMismatch { output } => tampered(
                "Payee key does not match",
                "A payee names a wallet key that is not the key at that address.",
            )
            .detail(position("output", *output)),
            Self::OutputHintsDisagree { output } => tampered(
                "Payee addresses disagree",
                "A payee names two or more different addresses of this wallet.",
            )
            .detail(position("output", *output)),
            Self::OutputScriptKind { output } => Words::new(
                Tone::Stop,
                "Payee address not allowed",
                "A payee is not an ordinary address of a kind this app is willing to pay.",
                Who::KeyHolder,
                "Do not approve it, and tell the owner in person that this app cannot pay it.",
            )
            .detail(position("output", *output)),
            Self::OutputAmountZero { output } => tampered(
                "Payee with no amount",
                "A payee in this payment is given no amount.",
            )
            .detail(position("output", *output)),
            Self::OutputAmountTooLarge { output, amount } => tampered(
                "Payee amount impossible",
                "A payee is given an amount above all the bitcoin that can exist.",
            )
            .detail(format!("{}, {}", position("output", *output), sats(*amount))),
            Self::OutputTotalOverflow => tampered(
                "Payee total impossible",
                "The payees in this payment add up to more than can exist.",
            ),
            Self::OutputsExceedInputs { outputs, inputs } => tampered(
                "Pays out more than it has",
                &format!(
                    "This payment pays out {} but its coins hold only {}.",
                    sats(*outputs),
                    sats(*inputs)
                ),
            ),
            Self::ChangeIndexTooFar {
                output,
                index,
                limit,
            } => Words::new(
                Tone::Ended,
                "Change address too far on",
                "This payment sends its change to an address further along than this app has seen.",
                Who::KeyHolder,
                "Open \"Wallet\" on this app, tell it how far the change addresses have got, then ask again.",
            )
            .detail(format!(
                "{}, change address {index}, allowed below {limit}",
                position("output", *output)
            )),
            Self::OverCeiling {
                ceiling,
                amount,
                limit,
            } => over_limit(*ceiling, *amount, *limit),
            Self::TotalsOverflow => tampered(
                "Spending total impossible",
                "The totals this app keeps would overflow, so it will not sign this payment.",
            ),
            Self::SpendLogFull => Words::new(
                Tone::Ended,
                "Spending record full",
                "This app's record of recent payments is full, so it cannot sign another yet.",
                Who::KeyHolder,
                "Wait until older payments drop out of the record, then ask again.",
            ),
            Self::FeeRateTooHigh { fee, vsize, limit } => Words::new(
                Tone::Stop,
                "Fee too high for the size",
                format!(
                    "The fee of {} is too high for the size of this payment.",
                    sats(*fee)
                ),
                Who::KeyHolder,
                "Ask the owner for a lower fee, or, if you both agree, raise the fee limit on this app.",
            )
            .detail(format!("size {vsize} vB, limit {limit} sats per vB")),
            Self::FeeTooHigh { fee, limit } => Words::new(
                Tone::Stop,
                "Fee too high",
                format!(
                    "The fee of {} is above this app's fee limit of {}.",
                    sats(*fee),
                    sats(*limit)
                ),
                Who::KeyHolder,
                "Ask the owner for a lower fee, or, if you both agree, raise the fee limit on this app.",
            ),
            Self::WeightTooHigh { weight, limit } => Words::new(
                Tone::Stop,
                "Payment too large",
                "Once signed, this payment would be bigger than this app allows.",
                Who::Owner,
                "Ask the owner to split it into smaller payments.",
            )
            .detail(format!("weight {weight}, limit {limit}")),
            Self::ReviewOutdated { output } => Words::new(
                Tone::Ended,
                "Payment changed",
                "A payee is no longer what the review on screen showed.",
                Who::KeyHolder,
                "Look at the review again before you approve.",
            )
            .detail(position("output", *output)),
            Self::ReviewForOtherWallet => Words::new(
                Tone::Stop,
                "Review for another wallet",
                "The review on screen was made for a different wallet.",
                Who::KeyHolder,
                "Start the review again, and if it happens again, tell the owner in person.",
            ),
            Self::WrongKey => tampered(
                "Wrong signing key",
                "The key this app signs with is not its own key in this wallet.",
            ),
            Self::Internal(what) => Words::new(
                Tone::Stop,
                "This app has a fault",
                "Nothing was signed, because this app's own check did not pass.",
                Who::KeyHolder,
                "Try again, and if it happens again, stop and tell the owner in person; see Details.",
            )
            .detail(*what),
        };
        words.code(code)
    }
}

/// The refusal for one of the six limits (SR-35, SR-36).
fn over_limit(ceiling: Ceiling, amount: u64, limit: u64) -> Words {
    let what = match ceiling {
        Ceiling::OtherPerTx | Ceiling::AllowlistedPerTx => format!(
            "This payment of {} is more than the limit of {} {}.",
            sats(amount),
            sats(limit),
            ceiling.plain()
        ),
        Ceiling::OtherDay | Ceiling::AllowlistedDay => format!(
            "This would bring the day's total to {}, over the limit of {} {}.",
            sats(amount),
            sats(limit),
            ceiling.plain()
        ),
        Ceiling::OtherWeek | Ceiling::AllowlistedWeek => format!(
            "This would bring the week's total to {}, over the limit of {} {}.",
            sats(amount),
            sats(limit),
            ceiling.plain()
        ),
    };
    Words::new(
        Tone::Ended,
        ceiling.heading(),
        what,
        Who::KeyHolder,
        "Pay less, or, if you and the owner agree, change the limit on this app's own screen.",
    )
    .code(ceiling.rule())
}

impl ChangeRefusal {
    /// What the key holder is shown when this app's own screen refuses a
    /// change. Nothing was changed.
    pub fn words(&self) -> Words {
        let code = self.rule();
        let words = match self {
            Self::ScriptKind => Words::new(
                Tone::Stop,
                "Address kind not allowed",
                "This app saves only ordinary addresses, and this one is of another kind.",
                Who::KeyHolder,
                "Check the address with the owner and try again.",
            ),
            Self::InvalidLabel => Words::new(
                Tone::Stop,
                "Name not accepted",
                "The name for an address must be 1 to 64 characters, with nothing hidden in it.",
                Who::KeyHolder,
                "Type a plain name of that length and save again.",
            ),
            Self::AllowlistFull => Words::new(
                Tone::Stop,
                "Saved addresses are full",
                "This app already holds as many saved addresses as it can.",
                Who::KeyHolder,
                "Remove one that is not used, then save this one.",
            ),
            Self::AlreadyEnrolled => Words::new(
                Tone::Ended,
                "Address already saved",
                "This address is already among the saved addresses.",
                Who::Nobody,
                "Nothing to do. A newly saved address can be paid after its first day.",
            ),
            Self::NotEnrolled => Words::new(
                Tone::Ended,
                "Address not saved",
                "This address is not among the saved addresses.",
                Who::KeyHolder,
                "Check the address; there is nothing to remove.",
            ),
            Self::CeilingAboveMoney => Words::new(
                Tone::Stop,
                "Limit above all bitcoin",
                "A limit was set above all the bitcoin there can be.",
                Who::KeyHolder,
                "Set a smaller limit.",
            ),
            Self::FeeRateOutOfBounds => Words::new(
                Tone::Stop,
                "Fee rate limit out of range",
                "The limit for the fee by size is zero or above the highest this app allows.",
                Who::KeyHolder,
                format!(
                    "Give a limit from 1 sat to {} for each unit of size.",
                    sats(MAX_FEE_RATE_SAT_PER_VB)
                ),
            ),
            Self::FeeOutOfBounds => Words::new(
                Tone::Stop,
                "Fee limit out of range",
                "The limit for one fee is zero or above the highest this app allows.",
                Who::KeyHolder,
                format!("Give a limit from 1 sat to {}.", sats(MAX_FEE_SATS)),
            ),
            Self::MarkNotHigher => Words::new(
                Tone::Stop,
                "Change number not higher",
                "This app already knows change addresses at least that far along.",
                Who::KeyHolder,
                "Give a higher number, or leave it as it is.",
            ),
            Self::MarkTooHigh => Words::new(
                Tone::Stop,
                "Change number too high",
                "That is further along than this app allows for change addresses.",
                Who::KeyHolder,
                "Check the number with the owner and try again.",
            ),
            Self::ReceiveMarkNotHigher => Words::new(
                Tone::Stop,
                "Address number not higher",
                "This app has already seen the shop's addresses at least that far along.",
                Who::KeyHolder,
                "Give a higher number, or leave it as it is.",
            ),
            Self::ReceiveMarkTooHigh => Words::new(
                Tone::Stop,
                "Address number too high",
                "That is further along than this app allows for the shop's addresses.",
                Who::KeyHolder,
                "Check the number with the owner and try again.",
            ),
        };
        words.code(code)
    }
}

/// The words of the refusal or registration error whose `Display` text is
/// `text`, for a caller that has only the text (the Key app's calls carry
/// it as a string). `None` for any other text.
pub fn words_for_text(text: &str) -> Option<Words> {
    if let Some(r) = change_refusals()
        .into_iter()
        .find(|r| r.to_string() == text)
    {
        return Some(r.words());
    }
    let fixed = [
        RegisterError::UnsupportedNetwork,
        RegisterError::OwnKeyNotInWallet,
        RegisterError::Key3NotInWallet,
    ];
    if let Some(r) = fixed.into_iter().find(|r| r.to_string() == text) {
        return Some(r.words());
    }
    text.strip_prefix("SR-60: ")
        .map(|rest| wallet_words_for_text(rest).code("SR-60"))
}

/// The words for the text of a [`WalletError`], when only the text is
/// known. A text that is none of the twelve gets words of its own.
pub fn wallet_words_for_text(text: &str) -> Words {
    let slots = (1..=3u8).flat_map(|slot| {
        [
            WalletError::BadPath { slot },
            WalletError::BadXpubDepth { slot },
            WalletError::NetworkMismatch { slot },
        ]
    });
    if let Some(e) = wallet_errors()
        .into_iter()
        .chain(slots)
        .find(|e| e.to_string() == text)
    {
        return wallet_error_words(&e);
    }
    if text.starts_with("cannot read descriptor") {
        return wallet_error_words(&WalletError::Parse(String::new())).detail(text);
    }
    if text.starts_with("key derivation failed") {
        return wallet_error_words(&WalletError::Derive(String::new())).detail(text);
    }
    Words::new(
        Tone::Stop,
        "Wallet not accepted",
        "This app did not accept the wallet it was given.",
        Who::KeyHolder,
        "Scan or copy the wallet from the owner's phone again.",
    )
    .detail(text)
}

/// What the key holder is shown when a wallet description is not accepted.
/// Shared by the setup and by the registration of the wallet. The text of
/// the error goes in the detail.
pub fn wallet_error_words(error: &WalletError) -> Words {
    let tell = "Do not use this wallet, and tell the owner in person.";
    let again = "Scan or copy the wallet from the owner's phone again.";
    let fault =
        "Try again, and if it happens again, stop and tell the owner in person; see Details.";
    let stop = |heading: &str, what: &str, next: &str| {
        Words::new(Tone::Stop, heading, what, Who::KeyHolder, next)
    };
    match error {
        WalletError::BadSlotIndex => stop(
            "Wallet keys out of order",
            "The wallet's keys are not numbered 1, 2 and 3 in order.",
            tell,
        ),
        WalletError::BadPath { .. } => stop(
            "Wallet key on a wrong path",
            "One of the wallet's keys is not on the path this app requires.",
            tell,
        ),
        WalletError::BadXpubDepth { .. } => stop(
            "Wallet key at wrong level",
            "One of the wallet's keys is not a key at the account level.",
            tell,
        ),
        WalletError::NetworkMismatch { .. } => stop(
            "Key for another network",
            "One of the wallet's keys was made for another network than the wallet.",
            tell,
        ),
        WalletError::DuplicateFingerprint => stop(
            "Two keys from one source",
            "Two of the wallet's keys come from the same source.",
            tell,
        ),
        WalletError::DuplicateXpub => stop(
            "Two keys are the same",
            "Two of the wallet's keys are identical.",
            tell,
        ),
        WalletError::ChecksumMissing => stop(
            "Wallet text has no check",
            "The wallet text has no check characters at its end.",
            again,
        ),
        WalletError::ChecksumMismatch => stop(
            "Wallet text does not check",
            "The check characters of the wallet text do not match the text.",
            again,
        ),
        WalletError::NonCanonical => stop(
            "Wallet text in wrong form",
            "The wallet text is not written in the exact form this app requires.",
            again,
        ),
        WalletError::Parse(_) => stop(
            "Wallet text not understood",
            "This app cannot read the wallet text it was given.",
            again,
        ),
        WalletError::IndexOutOfRange => stop(
            "Address number out of range",
            "Nothing was changed, because an address number was out of range.",
            fault,
        ),
        WalletError::Derive(_) => stop(
            "Key could not be worked out",
            "Nothing was changed, because this app could not work out a key.",
            fault,
        ),
    }
    .detail(error.to_string())
}

impl RegisterError {
    /// What the key holder is shown when the wallet cannot be registered.
    /// Nothing was changed.
    pub fn words(&self) -> Words {
        let code = self.rule();
        let words = match self {
            Self::Wallet(e) => wallet_error_words(e),
            Self::UnsupportedNetwork => Words::new(
                Tone::Stop,
                "Network not supported",
                "This app does not work with the network this wallet is for.",
                Who::KeyHolder,
                "Do not use this wallet, and tell the owner in person.",
            ),
            Self::OwnKeyNotInWallet => Words::new(
                Tone::Stop,
                "This app's key not in wallet",
                "None of the wallet's three keys is the key this app holds.",
                Who::KeyHolder,
                "Do not accept this wallet, and tell the owner in person.",
            ),
            Self::Key3NotInWallet => Words::new(
                Tone::Stop,
                "Key 3 not in wallet",
                "The key given for key 3 is not in this wallet.",
                Who::KeyHolder,
                "Do not accept this wallet, and tell the owner in person.",
            ),
        };
        words.code(code)
    }
}

/// An address saved on this app that cannot be paid yet (SR-30). `active_at`
/// is the time its first day ends, in seconds since 1970.
pub fn waiting_address_words(active_at: u64) -> Words {
    Words::new(
        Tone::Info,
        "Address added recently",
        "This address was added less than 24 hours ago.",
        Who::Nobody,
        "Nothing to do. It can be paid after the time shown.",
    )
    .at(active_at)
    .code("SR-30")
}

/// The phone's clock is earlier than a time this app has already recorded
/// (SR-31). `latest_seen` is that time, in seconds since 1970.
pub fn clock_behind_words(latest_seen: u64) -> Words {
    Words::new(
        Tone::Stop,
        "This phone's clock is behind",
        "This phone's clock shows a time earlier than one this app has already recorded.",
        Who::KeyHolder,
        "Set this phone's clock to the right time, then ask again.",
    )
    .at(latest_seen)
    .code("SR-31")
}

/// The words of every refusal and every state of this crate, one or more
/// for each variant and each nested variant, named after them (SC-12).
pub fn all_words() -> Vec<(String, Words)> {
    let mut all: Vec<(String, Words)> = Vec::new();
    for r in refusals() {
        all.push((format!("Refusal::{}", refusal_name(&r)), r.words()));
    }
    for c in ceilings() {
        all.push((
            format!("Ceiling::{}", ceiling_name(c)),
            over_limit(c, 1_000, 500),
        ));
    }
    for r in change_refusals() {
        all.push((format!("ChangeRefusal::{}", change_name(&r)), r.words()));
    }
    for r in register_errors() {
        // The nested wallet errors have their own entries below.
        if !matches!(r, RegisterError::Wallet(_)) {
            all.push((format!("RegisterError::{}", register_name(&r)), r.words()));
        }
    }
    for e in wallet_errors() {
        all.push((
            format!("RegisterError::Wallet({})", wallet_name(&e)),
            RegisterError::Wallet(e).words(),
        ));
    }
    all.push((
        "AllowlistEntry::Waiting".to_string(),
        waiting_address_words(1_700_086_400),
    ));
    all.push((
        "Clock::Behind".to_string(),
        clock_behind_words(1_700_000_000),
    ));
    all
}

fn ceilings() -> [Ceiling; 6] {
    [
        Ceiling::OtherPerTx,
        Ceiling::OtherDay,
        Ceiling::OtherWeek,
        Ceiling::AllowlistedPerTx,
        Ceiling::AllowlistedDay,
        Ceiling::AllowlistedWeek,
    ]
}

fn ceiling_name(c: Ceiling) -> &'static str {
    match c {
        Ceiling::OtherPerTx => "OtherPerTx",
        Ceiling::OtherDay => "OtherDay",
        Ceiling::OtherWeek => "OtherWeek",
        Ceiling::AllowlistedPerTx => "AllowlistedPerTx",
        Ceiling::AllowlistedDay => "AllowlistedDay",
        Ceiling::AllowlistedWeek => "AllowlistedWeek",
    }
}

fn refusals() -> Vec<Refusal> {
    vec![
        Refusal::NotAPsbt,
        Refusal::FileTooLarge { len: 9_000_000 },
        Refusal::InputCount { found: 1_001 },
        Refusal::OutputCount { found: 101 },
        Refusal::TransactionVersion { found: 1 },
        Refusal::LockTimeNotHeight {
            lock_time: 600_000_000,
        },
        Refusal::RelativeTimeLock {
            input: 0,
            sequence: 0xFFFF_FFFC,
        },
        Refusal::DuplicateInput { input: 1 },
        Refusal::InputAlreadyFinal { input: 0 },
        Refusal::ForbiddenInputField {
            input: 0,
            field: "a Taproot field",
        },
        Refusal::ForbiddenOutputField {
            output: 0,
            field: "a redeem script",
        },
        Refusal::SighashType { input: 0, found: 2 },
        Refusal::ForeignXpub,
        Refusal::MissingPreviousTransaction { input: 0 },
        Refusal::PreviousTransactionMismatch { input: 0 },
        Refusal::PreviousOutputMissing { input: 0, index: 5 },
        Refusal::WitnessUtxoMismatch { input: 0 },
        Refusal::NoWalletKeyHint { input: 0 },
        Refusal::HintKeyMismatch { input: 0 },
        Refusal::HintsDisagree { input: 0 },
        Refusal::ScriptNotInWallet { input: 0 },
        Refusal::WitnessScriptMismatch { input: 0 },
        Refusal::ForeignSignature { input: 0 },
        Refusal::SignatureSighashType { input: 0 },
        Refusal::BadSignature { input: 0 },
        Refusal::AlreadySigned,
        Refusal::InputAmountTooLarge {
            input: 0,
            amount: 3_000_000_000_000_000,
        },
        Refusal::InputTotalOverflow,
        Refusal::OutputHintKeyMismatch { output: 0 },
        Refusal::OutputHintsDisagree { output: 0 },
        Refusal::OutputScriptKind { output: 0 },
        Refusal::OutputAmountZero { output: 0 },
        Refusal::OutputAmountTooLarge {
            output: 0,
            amount: 3_000_000_000_000_000,
        },
        Refusal::OutputTotalOverflow,
        Refusal::OutputsExceedInputs {
            outputs: 120_000,
            inputs: 100_000,
        },
        Refusal::ChangeIndexTooFar {
            output: 1,
            index: 40,
            limit: 20,
        },
        Refusal::OverCeiling {
            ceiling: Ceiling::OtherPerTx,
            amount: 150_000,
            limit: 100_000,
        },
        Refusal::TotalsOverflow,
        Refusal::SpendLogFull,
        Refusal::FeeRateTooHigh {
            fee: 90_000,
            vsize: 200,
            limit: 150,
        },
        Refusal::FeeTooHigh {
            fee: 250_000,
            limit: 200_000,
        },
        Refusal::WeightTooHigh {
            weight: 500_000,
            limit: 400_000,
        },
        Refusal::ReviewOutdated { output: 0 },
        Refusal::ReviewForOtherWallet,
        Refusal::WrongKey,
        Refusal::Internal("own signature did not verify"),
    ]
}

/// The name of a variant. The match has no catch-all arm, so a new variant
/// does not compile until it is named here (SC-01).
fn refusal_name(r: &Refusal) -> &'static str {
    match r {
        Refusal::NotAPsbt => "NotAPsbt",
        Refusal::FileTooLarge { .. } => "FileTooLarge",
        Refusal::InputCount { .. } => "InputCount",
        Refusal::OutputCount { .. } => "OutputCount",
        Refusal::TransactionVersion { .. } => "TransactionVersion",
        Refusal::LockTimeNotHeight { .. } => "LockTimeNotHeight",
        Refusal::RelativeTimeLock { .. } => "RelativeTimeLock",
        Refusal::DuplicateInput { .. } => "DuplicateInput",
        Refusal::InputAlreadyFinal { .. } => "InputAlreadyFinal",
        Refusal::ForbiddenInputField { .. } => "ForbiddenInputField",
        Refusal::ForbiddenOutputField { .. } => "ForbiddenOutputField",
        Refusal::SighashType { .. } => "SighashType",
        Refusal::ForeignXpub => "ForeignXpub",
        Refusal::MissingPreviousTransaction { .. } => "MissingPreviousTransaction",
        Refusal::PreviousTransactionMismatch { .. } => "PreviousTransactionMismatch",
        Refusal::PreviousOutputMissing { .. } => "PreviousOutputMissing",
        Refusal::WitnessUtxoMismatch { .. } => "WitnessUtxoMismatch",
        Refusal::NoWalletKeyHint { .. } => "NoWalletKeyHint",
        Refusal::HintKeyMismatch { .. } => "HintKeyMismatch",
        Refusal::HintsDisagree { .. } => "HintsDisagree",
        Refusal::ScriptNotInWallet { .. } => "ScriptNotInWallet",
        Refusal::WitnessScriptMismatch { .. } => "WitnessScriptMismatch",
        Refusal::ForeignSignature { .. } => "ForeignSignature",
        Refusal::SignatureSighashType { .. } => "SignatureSighashType",
        Refusal::BadSignature { .. } => "BadSignature",
        Refusal::AlreadySigned => "AlreadySigned",
        Refusal::InputAmountTooLarge { .. } => "InputAmountTooLarge",
        Refusal::InputTotalOverflow => "InputTotalOverflow",
        Refusal::OutputHintKeyMismatch { .. } => "OutputHintKeyMismatch",
        Refusal::OutputHintsDisagree { .. } => "OutputHintsDisagree",
        Refusal::OutputScriptKind { .. } => "OutputScriptKind",
        Refusal::OutputAmountZero { .. } => "OutputAmountZero",
        Refusal::OutputAmountTooLarge { .. } => "OutputAmountTooLarge",
        Refusal::OutputTotalOverflow => "OutputTotalOverflow",
        Refusal::OutputsExceedInputs { .. } => "OutputsExceedInputs",
        Refusal::ChangeIndexTooFar { .. } => "ChangeIndexTooFar",
        Refusal::OverCeiling { .. } => "OverCeiling",
        Refusal::TotalsOverflow => "TotalsOverflow",
        Refusal::SpendLogFull => "SpendLogFull",
        Refusal::FeeRateTooHigh { .. } => "FeeRateTooHigh",
        Refusal::FeeTooHigh { .. } => "FeeTooHigh",
        Refusal::WeightTooHigh { .. } => "WeightTooHigh",
        Refusal::ReviewOutdated { .. } => "ReviewOutdated",
        Refusal::ReviewForOtherWallet => "ReviewForOtherWallet",
        Refusal::WrongKey => "WrongKey",
        Refusal::Internal(_) => "Internal",
    }
}

fn change_refusals() -> Vec<ChangeRefusal> {
    vec![
        ChangeRefusal::ScriptKind,
        ChangeRefusal::InvalidLabel,
        ChangeRefusal::AllowlistFull,
        ChangeRefusal::AlreadyEnrolled,
        ChangeRefusal::NotEnrolled,
        ChangeRefusal::CeilingAboveMoney,
        ChangeRefusal::FeeRateOutOfBounds,
        ChangeRefusal::FeeOutOfBounds,
        ChangeRefusal::MarkNotHigher,
        ChangeRefusal::MarkTooHigh,
        ChangeRefusal::ReceiveMarkNotHigher,
        ChangeRefusal::ReceiveMarkTooHigh,
    ]
}

fn change_name(r: &ChangeRefusal) -> &'static str {
    match r {
        ChangeRefusal::ScriptKind => "ScriptKind",
        ChangeRefusal::InvalidLabel => "InvalidLabel",
        ChangeRefusal::AllowlistFull => "AllowlistFull",
        ChangeRefusal::AlreadyEnrolled => "AlreadyEnrolled",
        ChangeRefusal::NotEnrolled => "NotEnrolled",
        ChangeRefusal::CeilingAboveMoney => "CeilingAboveMoney",
        ChangeRefusal::FeeRateOutOfBounds => "FeeRateOutOfBounds",
        ChangeRefusal::FeeOutOfBounds => "FeeOutOfBounds",
        ChangeRefusal::MarkNotHigher => "MarkNotHigher",
        ChangeRefusal::MarkTooHigh => "MarkTooHigh",
        ChangeRefusal::ReceiveMarkNotHigher => "ReceiveMarkNotHigher",
        ChangeRefusal::ReceiveMarkTooHigh => "ReceiveMarkTooHigh",
    }
}

fn wallet_errors() -> Vec<WalletError> {
    vec![
        WalletError::BadSlotIndex,
        WalletError::BadPath { slot: 2 },
        WalletError::BadXpubDepth { slot: 2 },
        WalletError::NetworkMismatch { slot: 2 },
        WalletError::DuplicateFingerprint,
        WalletError::DuplicateXpub,
        WalletError::ChecksumMissing,
        WalletError::ChecksumMismatch,
        WalletError::NonCanonical,
        WalletError::Parse("unexpected character".to_string()),
        WalletError::IndexOutOfRange,
        WalletError::Derive("overflow".to_string()),
    ]
}

fn wallet_name(e: &WalletError) -> &'static str {
    match e {
        WalletError::BadSlotIndex => "BadSlotIndex",
        WalletError::BadPath { .. } => "BadPath",
        WalletError::BadXpubDepth { .. } => "BadXpubDepth",
        WalletError::NetworkMismatch { .. } => "NetworkMismatch",
        WalletError::DuplicateFingerprint => "DuplicateFingerprint",
        WalletError::DuplicateXpub => "DuplicateXpub",
        WalletError::ChecksumMissing => "ChecksumMissing",
        WalletError::ChecksumMismatch => "ChecksumMismatch",
        WalletError::NonCanonical => "NonCanonical",
        WalletError::Parse(_) => "Parse",
        WalletError::IndexOutOfRange => "IndexOutOfRange",
        WalletError::Derive(_) => "Derive",
    }
}

fn register_errors() -> Vec<RegisterError> {
    vec![
        RegisterError::Wallet(WalletError::ChecksumMismatch),
        RegisterError::UnsupportedNetwork,
        RegisterError::OwnKeyNotInWallet,
        RegisterError::Key3NotInWallet,
    ]
}

fn register_name(e: &RegisterError) -> &'static str {
    match e {
        RegisterError::Wallet(_) => "Wallet",
        RegisterError::UnsupportedNetwork => "UnsupportedNetwork",
        RegisterError::OwnKeyNotInWallet => "OwnKeyNotInWallet",
        RegisterError::Key3NotInWallet => "Key3NotInWallet",
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use openbmt_core::say::{check, check_distinct};

    use super::*;
    use crate::ALLOWLIST_WAIT_SECS;

    /// The variant names of `enum_name` in `source`, read from the text.
    fn variants(source: &str, enum_name: &str) -> Vec<String> {
        let head = format!("pub enum {enum_name}");
        let mut inside = false;
        let mut out = Vec::new();
        for line in source.lines() {
            if line.starts_with(&head) {
                inside = true;
            } else if inside && line == "}" {
                break;
            } else if inside && line.starts_with("    ") && !line.starts_with("     ") {
                let rest = line.trim_start();
                if rest.starts_with(|c: char| c.is_ascii_uppercase()) {
                    let name: String = rest
                        .chars()
                        .take_while(|c| c.is_ascii_alphanumeric())
                        .collect();
                    out.push(name);
                }
            }
        }
        out
    }

    fn covered(prefix: &str, enum_name: &str, source: &str) {
        let names: BTreeSet<String> = all_words().into_iter().map(|(n, _)| n).collect();
        let found = variants(source, enum_name);
        assert!(!found.is_empty(), "no variants read for {enum_name}");
        for v in found {
            let own = format!("{prefix}::{v}");
            let nested = format!("{own}(");
            assert!(
                names.iter().any(|n| *n == own || n.starts_with(&nested)),
                "{own} has no words in all_words"
            );
        }
    }

    #[test]
    fn sc_12_every_word_keeps_the_rules() {
        assert_eq!(check_distinct(&all_words()), Ok(()));
    }

    #[test]
    fn sc_01_no_variant_is_missing() {
        let source = include_str!("refusal.rs");
        covered("Refusal", "Refusal", source);
        covered("Ceiling", "Ceiling", source);
        covered("ChangeRefusal", "ChangeRefusal", source);
        covered("RegisterError", "RegisterError", source);
        let names: BTreeSet<String> = all_words().into_iter().map(|(n, _)| n).collect();
        let wallet = variants(
            include_str!("../../openbmt-core/src/wallet.rs"),
            "WalletError",
        );
        assert!(!wallet.is_empty());
        for v in wallet {
            let own = format!("RegisterError::Wallet({v})");
            assert!(names.contains(&own), "{own} has no words in all_words");
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
    fn every_refusal_carries_its_rule_number() {
        for r in refusals() {
            assert_eq!(r.words().code, Some(r.rule()), "{}", refusal_name(&r));
        }
        for r in change_refusals() {
            assert_eq!(r.words().code, Some(r.rule()), "{}", change_name(&r));
        }
        for r in register_errors() {
            assert_eq!(r.words().code, Some(r.rule()), "{}", register_name(&r));
        }
    }

    #[test]
    fn a_request_that_looks_changed_is_stopped_and_told_in_person() {
        let tamper = [
            Refusal::ForeignXpub,
            Refusal::HintKeyMismatch { input: 0 },
            Refusal::HintsDisagree { input: 0 },
            Refusal::OutputHintKeyMismatch { output: 0 },
            Refusal::OutputHintsDisagree { output: 0 },
            Refusal::ScriptNotInWallet { input: 0 },
            Refusal::BadSignature { input: 0 },
        ];
        for r in tamper {
            let w = r.words();
            assert_eq!(w.tone, Tone::Stop);
            assert_eq!(w.who, Who::KeyHolder);
            assert!(w.next.contains("tell the owner in person"), "{}", w.next);
            assert!(w.next.starts_with("Do not approve"), "{}", w.next);
        }
        // The input and output forms share the instruction, not the first
        // sentence.
        let a = Refusal::HintKeyMismatch { input: 0 }.words();
        let b = Refusal::OutputHintKeyMismatch { output: 0 }.words();
        assert_eq!(a.next, b.next);
        assert_ne!(a.what, b.what);
    }

    #[test]
    fn a_limit_says_which_limit_and_where_it_is_changed() {
        let cases = [
            (Ceiling::OtherPerTx, "one payment to other addresses"),
            (Ceiling::OtherDay, "a day to other addresses"),
            (Ceiling::OtherWeek, "a week to other addresses"),
            (Ceiling::AllowlistedPerTx, "one payment to saved addresses"),
            (Ceiling::AllowlistedDay, "a day to saved addresses"),
            (Ceiling::AllowlistedWeek, "a week to saved addresses"),
        ];
        for (ceiling, text) in cases {
            let w = Refusal::OverCeiling {
                ceiling,
                amount: 2,
                limit: 1,
            }
            .words();
            assert!(w.what.contains(text), "{}", w.what);
            assert!(w.next.contains("this app's own screen"));
            assert!(w.next.contains("you and the owner agree"));
            assert_eq!(w.code, Some(ceiling.rule()));
        }
    }

    #[test]
    fn the_last_address_rules_point_to_the_wallet_screen() {
        let w = Refusal::ChangeIndexTooFar {
            output: 0,
            index: 40,
            limit: 20,
        }
        .words();
        assert!(w.next.contains("\"Wallet\""), "{}", w.next);
        assert!(!w.what.contains("mark"));
    }

    #[test]
    fn the_clock_and_the_waiting_address_put_the_time_in_at() {
        let c = clock_behind_words(1_700_000_000);
        assert_eq!(c.at, Some(1_700_000_000));
        assert!(c.next.contains("Set this phone's clock"));
        let a = waiting_address_words(1_700_086_400);
        assert_eq!(a.at, Some(1_700_086_400));
        assert!(a.what.contains("less than 24 hours ago"));
        assert_eq!(ALLOWLIST_WAIT_SECS, 24 * 3_600);
        assert_eq!(check(&a), Ok(()));
    }

    #[test]
    fn the_text_of_a_refusal_finds_its_words() {
        for r in change_refusals() {
            assert_eq!(words_for_text(&r.to_string()), Some(r.words()));
        }
        for r in register_errors() {
            assert_eq!(words_for_text(&r.to_string()), Some(r.words()));
        }
        for e in wallet_errors() {
            let w = wallet_words_for_text(&e.to_string());
            assert_eq!(w.heading, wallet_error_words(&e).heading);
            assert_eq!(w.detail, Some(e.to_string()));
        }
        // A text of no known kind has no words here, and the wallet's own
        // fallback is a valid screen.
        assert_eq!(words_for_text("something else"), None);
        assert_eq!(check(&wallet_words_for_text("something else")), Ok(()));
    }

    #[test]
    fn the_fault_of_the_signer_says_nothing_was_signed_and_where_the_detail_is() {
        let w = Refusal::Internal("own signature did not verify").words();
        assert!(w.what.starts_with("Nothing was signed"));
        assert!(w.next.contains("Details"));
        assert_eq!(w.detail.as_deref(), Some("own signature did not verify"));
    }
}
