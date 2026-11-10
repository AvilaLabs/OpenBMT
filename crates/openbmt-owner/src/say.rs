//! What the owner is told on the owner's phone about the queue, the
//! approvals and the set-up (`docs/SCREEN-RULES.md`, SC-01 to SC-12).
//!
//! The reader is the owner: they know the shop and its money and some
//! bitcoin. A queue entry is named by its kind ([`EntryKind`]), so that
//! the words say "refund" or "cash-out" and not "entry". The key holder's
//! reason for sending an entry back is text from another device: it is
//! quoted ("The key holder wrote"), never put in a sentence. Entry
//! numbers, transaction numbers and the decoder's text go in the details.
//! Every refusal of a call leaves the queue as it was; the words say so
//! where that is not plain.

use bitcoin::Txid;
use openbmt_core::identity::IdentityError;
use openbmt_core::say::{Quoted, Tone, Who, Words};
use openbmt_core::wire::DecodeError;
use openbmt_session::{BindError, EncodeError};

use crate::error::{OwnerError, Refusal, RestoreError, SetupError};
use crate::follow::REASON_DROPPED;
use crate::types::{ItemOutcome, Skipped, Stage};

/// The kinds of entry in the owner's queue.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    /// A refund to a customer.
    Refund,
    /// A cash-out of the shop's money.
    CashOut,
    /// A batch of new receiving addresses for a counter.
    Batch,
    /// A change of the shop's settings.
    SettingsChange,
    /// The pairing of a counter.
    Pairing,
}

/// Every kind, for the list of words.
pub const ALL_KINDS: [EntryKind; 5] = [
    EntryKind::Refund,
    EntryKind::CashOut,
    EntryKind::Batch,
    EntryKind::SettingsChange,
    EntryKind::Pairing,
];

impl EntryKind {
    /// The thing, as a sentence names it after "the".
    fn noun(self) -> &'static str {
        match self {
            Self::Refund => "refund",
            Self::CashOut => "cash-out",
            Self::Batch => "batch of addresses",
            Self::SettingsChange => "settings change",
            Self::Pairing => "pairing",
        }
    }

    /// The thing, as a heading starts with it.
    fn label(self) -> &'static str {
        match self {
            Self::Refund => "Refund",
            Self::CashOut => "Cash-out",
            Self::Batch => "Address batch",
            Self::SettingsChange => "Settings change",
            Self::Pairing => "Pairing",
        }
    }

    /// Whether the thing is paid out in a transaction on the network. The
    /// others are files that are handed on.
    fn is_payment(self) -> bool {
        match self {
            Self::Refund | Self::CashOut => true,
            Self::Batch | Self::SettingsChange | Self::Pairing => false,
        }
    }
}

fn stop(heading: &str, what: impl Into<String>, who: Who, next: impl Into<String>) -> Words {
    Words::new(Tone::Stop, heading, what, who, next)
}

fn info(heading: &str, what: impl Into<String>, who: Who, next: impl Into<String>) -> Words {
    Words::new(Tone::Info, heading, what, who, next)
}

/// A fault of the app that the owner cannot fix (SC-10): nothing was
/// changed, and support is told what to look at.
fn fault(heading: &str, what: impl Into<String>, code: &'static str) -> Words {
    stop(
        heading,
        what,
        Who::Owner,
        "Nothing was changed. Try again; if it repeats, stop and tell support, and read them the details.",
    )
    .code(code)
}

impl Stage {
    /// What the owner is told about one queue entry of this `kind` at this
    /// stage. A returned entry is told two ways: one the network dropped,
    /// which can be queued again but not cancelled, and one the key holder
    /// sent back, whose reason is quoted.
    pub fn words(&self, kind: EntryKind) -> Words {
        let n = kind.noun();
        let label = kind.label();
        let payment = kind.is_payment();
        match self {
            Self::Queued => info(
                &format!("{label}: queued"),
                format!("The {n} is in the queue and has not been sent for approval yet."),
                Who::Owner,
                "Bring this phone and the Key app together and start an approval.",
            )
            .code("AS-54"),
            Self::InSession { .. } => info(
                &format!("{label}: in approval"),
                format!("The {n} is part of an approval that is open now."),
                Who::Owner,
                "Finish the approval on the Key app, then bring its response back to this phone.",
            )
            .code("AS-61"),
            Self::Returned { reason } if reason == REASON_DROPPED => {
                let what = if payment {
                    format!("The network dropped the payment this {n} was in.")
                } else {
                    format!("The delivery of this {n} was dropped.")
                };
                stop(
                    &format!("{label}: dropped"),
                    what,
                    Who::Owner,
                    "Queue it again. It cannot be cancelled, because the dropped payment could still confirm.",
                )
                .code("AS-68")
            }
            Self::Returned { reason } => stop(
                &format!("{label}: sent back"),
                format!("The Key app sent the {n} back without approving it."),
                Who::Owner,
                "Read the key holder's reason, then queue it again or cancel it.",
            )
            .code("AS-63")
            .quoted(Quoted::new("The key holder wrote", reason)),
            Self::Signed { txid } => {
                let (what, next) = if payment {
                    (
                        format!("Both keys have signed the {n}, and its payment has not been sent to the network yet."),
                        "Send the payment to the network, then report it as sent here.",
                    )
                } else {
                    (
                        format!(
                            "Both approvals are on the {n}, and it has not been delivered yet."
                        ),
                        "Deliver it, then mark it delivered here.",
                    )
                };
                info(&format!("{label}: signed"), what, Who::Owner, next)
                    .code("AS-64")
                    .detail(format!("transaction {txid}"))
            }
            Self::Sent { txid } => {
                let (what, who, next) = if payment {
                    (
                        format!("The payment for the {n} was sent to the network and is not in a block yet."),
                        Who::Nobody,
                        "Nothing to do. It turns confirmed when the payment is in a block, in about ten minutes.",
                    )
                } else {
                    (
                        format!("The {n} was reported as delivered."),
                        Who::Nobody,
                        "Nothing to do. It leaves the list of finished items by itself.",
                    )
                };
                info(&format!("{label}: sent"), what, who, next)
                    .code("AS-54")
                    .detail(format!("transaction {txid}"))
            }
            Self::Paid { txid } => {
                let what = if payment {
                    format!("The payment for the {n} is in a block.")
                } else {
                    format!("The {n} is done.")
                };
                Words::new(
                    Tone::Good,
                    format!("{label}: confirmed"),
                    what,
                    Who::Nobody,
                    "Nothing to do. Clear it from the queue when you like.",
                )
                .code("AS-54")
                .detail(format!("transaction {txid}"))
            }
        }
    }
}

/// How a file that could not be read is described.
fn read_cause(e: &DecodeError) -> &'static str {
    match e {
        DecodeError::Truncated => "ends too early",
        DecodeError::TrailingBytes => "has extra data after its end",
        DecodeError::BadMagic => "is not a file of this kind",
        DecodeError::UnsupportedVersion(_) => {
            "comes from a version of the app this phone does not read"
        }
        DecodeError::TooLarge => "is larger than allowed",
        DecodeError::NonCanonical(_) => "is written in a form this phone does not accept",
        DecodeError::InvalidKey => "holds a key that is not valid",
        DecodeError::UnknownKind(_) => "holds a record this phone does not know",
    }
}

/// The words for a bound that was reached, by what is full.
fn full_words(what: &str) -> Words {
    let (heading, sentence, next): (&str, &str, &str) = match what {
        "the numbering" => (
            "Entry numbers used up",
            "The phone has used up all the entry numbers it can give.",
            "Nothing was changed. Stop and tell support: the owner phone's numbering is full.",
        ),
        "the refund queue" => (
            "Refund queue is full",
            "The refund queue is full, so nothing was added.",
            "Finish or cancel some refunds, then add this one again.",
        ),
        "the cash-out queue" => (
            "Cash-out queue is full",
            "The cash-out queue is full, so nothing was added.",
            "Finish or cancel some cash-outs, then add this one again.",
        ),
        "the batch queue" => (
            "Batch queue is full",
            "The queue of address batches is full, so nothing was added.",
            "Finish or cancel some batches, then add this one again.",
        ),
        "the pairing queue" => (
            "Pairing queue is full",
            "The queue of pairings is full, so nothing was added.",
            "Finish or cancel some pairings, then add this one again.",
        ),
        "the settings queue" => (
            "Settings queue is full",
            "The queue of settings changes is full, so nothing was added.",
            "Finish or cancel some settings changes, then add this one again.",
        ),
        "the list of sales' facts" => (
            "Sales list is full",
            "The phone keeps the details of as many sales as it can hold.",
            "Archive the paid refunds to free room, then try again.",
        ),
        "the list of open sessions" => (
            "Too many open approvals",
            "The phone already holds as many open approvals as it can.",
            "Take the Key app's response for one, or abandon one, then start this approval again.",
        ),
        "the list of outpoints kept for a refund" => (
            "Dropped-payment list full",
            "The phone cannot keep track of more coins from dropped payments for this refund.",
            "Nothing was changed. Stop and tell support: the dropped-payment list is full.",
        ),
        _ => (
            "A list is full",
            "A list the phone keeps is full, so nothing was added.",
            "Nothing was changed. Free some room by finishing or cancelling entries, then try again.",
        ),
    };
    stop(heading, sentence, Who::Owner, next)
        .code("AS-50")
        .detail(what)
}

/// The words for a text that is not allowed, by which text it is.
fn bad_text_words(what: &str) -> Words {
    let (heading, sentence, next): (&str, &str, &str) = match what {
        "cashier" => (
            "Cashier's name not usable",
            "The cashier's name is empty, over 64 characters or holds a control character.",
            "Correct the name at the counter, then queue the refund again.",
        ),
        "cash-out label" => (
            "Cash-out label not usable",
            "The label is empty, over 64 characters or holds a line break or other control character.",
            "Write a short plain label and try again.",
        ),
        "currency code" => (
            "Currency code not usable",
            "The currency code is empty, too long or holds a character that cannot be shown.",
            "Enter the currency's code again, for example three capital letters.",
        ),
        _ => (
            "A text is not usable",
            "A text is empty, too long or holds a character that cannot be shown.",
            "Write it again in plain words and try again.",
        ),
    };
    stop(heading, sentence, Who::Owner, next)
        .code("AS-07")
        .detail(what)
}

/// The words for a call that the entry's stage does not allow, by the
/// reason the call gave.
fn wrong_stage_words(what: &str) -> Words {
    let (heading, sentence, next): (&str, &str, &str) = match what {
        "it is in a session; abandon the session first" => (
            "Entry is in an approval",
            "This entry is part of an open approval, so it cannot be cancelled yet.",
            "Finish the approval on the Key app, or abandon it here, then cancel the entry.",
        ),
        "it was signed; its transaction decides its end" => (
            "Entry is already signed",
            "Both keys signed this entry, so it cannot be cancelled.",
            "Report its payment as confirmed or dropped when you know which; that ends the entry.",
        ),
        "its dropped transaction may still confirm; ask for it again" => (
            "Dropped payment may confirm",
            "The payment this entry was in was dropped but could still confirm, so the entry cannot be cancelled.",
            "Queue the entry again: its new payment spends the same coins, so only one of the two can happen.",
        ),
        "it is not returned" => (
            "Entry was not sent back",
            "Only an entry the Key app sent back or the network dropped can be queued again, and this one is not.",
            "Nothing to do for this entry. Look at its stage in the queue.",
        ),
        "a transaction is followed with broadcast, confirmed or dropped" => (
            "Report the payment instead",
            "A payment is not marked as delivered; it ends when it is confirmed or dropped.",
            "Report the payment as sent, confirmed or dropped, as it happens.",
        ),
        _ => (
            "Entry cannot be changed now",
            "The entry is at a stage that does not allow this.",
            "Look at the entry's stage in the queue, then try again when it allows it.",
        ),
    };
    stop(heading, sentence, Who::Owner, next)
        .code("AS-54")
        .detail(what)
}

/// The words for a request that could not be built.
fn encode_words(e: &EncodeError) -> Words {
    match e {
        EncodeError::TooLarge => stop(
            "Approval is too big",
            "The approval would be larger than 8 MB, which is more than the Key app takes.",
            Who::Owner,
            "Queue fewer entries at a time, then start the approval again.",
        )
        .code("AS-02"),
        EncodeError::ItemCount => stop(
            "Approval entry count wrong",
            "An approval holds from 1 to 64 entries, and this one does not.",
            Who::Owner,
            "Add an entry or take some out, then start the approval again.",
        )
        .code("AS-04"),
        EncodeError::ItemOrder => fault(
            "Entries out of order",
            "The phone put the entries of the approval in an order the Key app would refuse.",
            "AS-04",
        ),
        EncodeError::BadText(why) => stop(
            "An entry has unusable text",
            "A name or label in the approval is empty, too long or holds a control character.",
            Who::Owner,
            "Shorten or simplify the names and labels, then start the approval again.",
        )
        .code("AS-07")
        .detail(*why),
        EncodeError::BadItem(why) => stop(
            "An entry cannot be asked",
            "One entry is not something the Key app can be asked to approve.",
            Who::Owner,
            "Read the details, cancel the entry they name, then start the approval again.",
        )
        .code("AS-06")
        .detail(*why),
        EncodeError::BadPayload(why) => fault(
            "Approval part not accepted",
            "The phone built a part of the approval that the Key app's file form does not allow.",
            "AS-12",
        )
        .detail(*why),
    }
}

/// The words for a response that is not bound to the request.
fn bind_words(e: &BindError) -> Words {
    let next_again = "Do not use this response. Ask the key holder to answer the approval again.";
    let w = match e {
        BindError::WrongApprover => stop(
            "Response from other Key app",
            "The response was signed by a Key app this phone is not paired with.",
            Who::KeyHolder,
            "Answer from the Key app this phone was set up with, and bring that response here.",
        ),
        BindError::BadSignature => stop(
            "Response signature is wrong",
            "The Key app's signature on the response does not check out.",
            Who::KeyHolder,
            next_again,
        ),
        BindError::WrongSession => stop(
            "Wrong approval for response",
            "The response belongs to a different approval than the one it was offered for.",
            Who::Owner,
            "Check which approval this response is for, and offer it to that one.",
        ),
        BindError::WrongRequest => stop(
            "Response for another request",
            "The response answers an earlier or changed version of this approval.",
            Who::KeyHolder,
            next_again,
        ),
        BindError::UnknownItem { .. } => stop(
            "Response has an extra answer",
            "The response answers an entry that this approval did not ask about.",
            Who::KeyHolder,
            next_again,
        ),
        BindError::MissingItem { .. } => stop(
            "Response misses an answer",
            "The response has no answer for one of the entries of the approval.",
            Who::KeyHolder,
            next_again,
        ),
        BindError::KindMismatch { .. } => stop(
            "Answer does not fit entry",
            "One answer in the response is of a kind that does not fit the entry it answers.",
            Who::KeyHolder,
            next_again,
        ),
        BindError::BadHealthProof { .. } => stop(
            "Health check answer is wrong",
            "The Key app's signed answer to the health check does not check out.",
            Who::KeyHolder,
            "Run the health check again; if it fails again, check that this is the Key app you paired.",
        ),
    };
    let code = match e {
        BindError::BadHealthProof { .. } => "AS-14",
        BindError::WrongApprover
        | BindError::BadSignature
        | BindError::WrongSession
        | BindError::WrongRequest
        | BindError::UnknownItem { .. }
        | BindError::MissingItem { .. }
        | BindError::KindMismatch { .. } => "AS-13",
    };
    w.code(code).detail(e.to_string())
}

impl Refusal {
    /// What the owner is told when a call was refused. Nothing was changed
    /// and nothing was stored.
    pub fn words(&self) -> Words {
        match self {
            Self::Full(what) => full_words(what),
            Self::BadText(what) => bad_text_words(what),
            Self::DestinationKind => stop(
                "That address cannot be paid",
                "The address is not of a kind this wallet will pay.",
                Who::Owner,
                "Ask the recipient for a standard bitcoin address and enter it again.",
            )
            .code("AS-51"),
            Self::BelowDust { sats, dust } => stop(
                "Amount too small to send",
                format!(
                    "{} is below the smallest amount this address can take, {}.",
                    openbmt_core::say::sats(*sats),
                    openbmt_core::say::sats(*dust)
                ),
                Who::Owner,
                "Enter a larger amount and try again.",
            )
            .code("AS-51"),
            Self::AmountTooLarge => stop(
                "Amount is too large",
                "The amount is more than the 21 million bitcoin that can ever exist.",
                Who::Owner,
                "Check the digits and enter the amount again.",
            )
            .code("AS-51"),
            Self::UnknownSale => stop(
                "Sale not known yet",
                "This phone has no record of that sale yet, so it cannot tell what was received.",
                Who::Owner,
                "Wait for the counter's records to reach this phone, then queue the refund again.",
            )
            .code("AS-52"),
            Self::MoreThanReceived { received, wanted } => stop(
                "Refund is more than was paid",
                format!(
                    "The refunds of this sale would add up to {}, and only {} was received.",
                    openbmt_core::say::sats(*wanted),
                    openbmt_core::say::sats(*received)
                ),
                Who::Owner,
                "Lower the amount, or cancel a refund already queued for this sale.",
            )
            .code("AS-52"),
            Self::SaleInUse => stop(
                "Sale still has refunds",
                "Refunds are counted against this sale, so its record stays for now.",
                Who::Owner,
                "Finish or cancel the refunds of this sale, then remove it again.",
            )
            .code("AS-52"),
            Self::UnknownEntry(n) => stop(
                "Entry not found",
                "The queue has no entry with that number.",
                Who::Owner,
                "Pick the entry from the queue list and try again.",
            )
            .code("AS-54")
            .detail(format!("entry {n}")),
            Self::WrongStage(what) => wrong_stage_words(what),
            Self::BatchNotDerived => stop(
                "Address batch not accepted",
                "The addresses are not the ones this wallet makes for that counter and range.",
                Who::Owner,
                "Make the batch again from this wallet for the same counter, then queue it.",
            )
            .code("AS-58"),
            Self::PairingRequest(why) => stop(
                "Pairing request not accepted",
                "The Key app's offer does not accept this counter's pairing request.",
                Who::Owner,
                "Ask the counter to make a new pairing request, then queue it again.",
            )
            .code("AS-58")
            .detail(why.as_str()),
            Self::SettingsInvalid(why) => stop(
                "Settings not accepted",
                "The new settings do not pass the wallet's rules, so they were not queued.",
                Who::Owner,
                "Check each value against the limits shown on the settings screen and save again.",
            )
            .code("AS-59")
            .detail(why.as_str()),
            Self::SettingsVersion { known, got } => stop(
                "Settings are out of date",
                "These settings are not newer than the ones already in force.",
                Who::Owner,
                "Open the current settings, change them there and save them as a new version.",
            )
            .code("AS-59")
            .detail(format!("version {got}, highest known {known}")),
            Self::ZeroFeeRate => stop(
                "Fee rate is zero",
                "The fee rate is zero, so the payment would never be picked up by the network.",
                Who::Owner,
                "Choose a fee rate above zero and start the approval again.",
            )
            .code("AS-61"),
            Self::Encode(e) => {
                let words = encode_words(e);
                if words.detail.is_some() {
                    words
                } else {
                    words.detail(e.to_string())
                }
            }
            Self::SessionIdUsed => stop(
                "Approval number already used",
                "An open approval already has the number this one was given.",
                Who::Owner,
                "Try again; the phone picks a new number each time.",
            )
            .code("AS-61"),
            Self::Frame(e) => stop(
                "Response file not readable",
                format!("The Key app's response could not be read: it {}.", read_cause(e)),
                Who::KeyHolder,
                "Ask the key holder to send the response again; if it repeats, start a new approval.",
            )
            .code("AS-62")
            .detail(e.to_string()),
            Self::UnknownSession => stop(
                "No open approval for it",
                "No open approval on this phone matches that response.",
                Who::Owner,
                "Check that it was made for an approval started here and not already taken or abandoned; if not, start again.",
            )
            .code("AS-62"),
            Self::Bind(e) => bind_words(e),
            Self::OutboxFull => stop(
                "Too many items waiting",
                "Too many signed items are waiting to be sent or handed on, so the response was not taken.",
                Who::Owner,
                "Send or hand on the waiting items and report them, then take the response again.",
            )
            .code("AS-64"),
            Self::UnknownTransaction(txid) => stop(
                "Payment not waiting",
                "No signed payment on this phone is waiting under that number.",
                Who::Owner,
                "Look in the list of finished items; the payment may already be reported.",
            )
            .code("AS-64")
            .detail(format!("transaction {txid}")),
            Self::SnapshotTooLarge => stop(
                "Saved data would be too big",
                "The phone's saved data would grow too large to keep, so the change was not made.",
                Who::Owner,
                "Nothing was changed. Archive paid entries and cancel old ones, then try again.",
            )
            .code("AS-65"),
            Self::Key1Wallet => stop(
                "Wallet does not match",
                "The owner key's saved state belongs to a different wallet than this response.",
                Who::Owner,
                "Do not use this response. Check that this phone and the Key app are paired for the same wallet.",
            )
            .code("AS-62"),
        }
    }
}

impl<E: std::fmt::Display> OwnerError<E> {
    /// What the owner is told when a call did not go through. Nothing was
    /// changed. A store that failed is a fault of the phone (SC-10): the
    /// system's own text is in the details.
    pub fn words(&self) -> Words {
        match self {
            Self::Refused(r) => r.words(),
            Self::Store(e) => stop(
                "Change could not be saved",
                "The phone could not store the new state, so nothing was changed.",
                Who::Owner,
                "Try again; if it repeats, stop and tell support. The system's own text is under Details.",
            )
            .code("AS-64")
            .detail(e.to_string()),
        }
    }
}

impl SetupError {
    /// What the owner is told when the owner phone could not be set up.
    pub fn words(&self) -> Words {
        match self {
            Self::Identity(e) => {
                let (heading, what) = match e {
                    IdentityError::InvalidSecret => (
                        "Owner secret not usable",
                        "The secret this phone was given is not a usable owner key.",
                    ),
                    IdentityError::InvalidPublicKey => (
                        "A public key is not usable",
                        "A public key this phone was given is not a usable key.",
                    ),
                };
                stop(
                    heading,
                    what,
                    Who::Owner,
                    "Make a new owner secret on this phone and set it up again.",
                )
                .code("AS-65")
                .detail(e.to_string())
            }
            Self::SameRoot => stop(
                "Phones share one secret",
                "The owner's phone and the Key app were set up from the same secret.",
                Who::KeyHolder,
                "Set the Key app up again on its own device from its own new secret, then pair the two.",
            )
            .code("AS-65"),
        }
    }
}

impl RestoreError {
    /// What the owner is told when stored data could not be brought back.
    /// The phone was not changed.
    pub fn words(&self) -> Words {
        match self {
            Self::Setup(e) => e.words(),
            Self::Decode(e) => stop(
                "Saved data cannot be read",
                format!("The saved data on this phone cannot be read: it {}.", read_cause(e)),
                Who::Owner,
                "Nothing was changed. Restore the last good backup; if there is none, stop and tell support.",
            )
            .code("AS-65")
            .detail(e.to_string()),
            Self::WrongWallet => stop(
                "Saved data: another wallet",
                "The saved data was made for a different wallet than the one this phone is set up for.",
                Who::Owner,
                "Nothing was changed. Restore the backup made for this wallet.",
            )
            .code("AS-65"),
            Self::WrongOwner => stop(
                "Saved data: another owner",
                "The saved data was made by a different owner key than the one on this phone.",
                Who::Owner,
                "Nothing was changed. Restore the backup made by this phone.",
            )
            .code("AS-65"),
            Self::WrongApprover => stop(
                "Saved data: another Key app",
                "The saved data was made for a different Key app than the one this phone is paired with.",
                Who::Owner,
                "Nothing was changed. Restore the backup made for the Key app you use.",
            )
            .code("AS-65"),
            Self::Inconsistent(why) => stop(
                "Saved data disagrees",
                "The saved data on this phone does not agree with itself, so it was not loaded.",
                Who::Owner,
                "Nothing was changed. Restore the last good backup; if there is none, stop and tell support.",
            )
            .code("AS-65")
            .detail(*why),
        }
    }
}

impl Skipped {
    /// What the owner is told when an entry stayed queued and was left out
    /// of the approval just built. The entry is unchanged. The reasons are
    /// matched by the rule number the reason starts with; any other reason
    /// is a payment the wallet could not plan, and its text goes in the
    /// details.
    pub fn words(&self) -> Words {
        let r = self.reason.as_str();
        let words = if r.starts_with("AS-50:") {
            info(
                "Left for the next approval",
                "The approval already holds as many entries as it can, so this one stays queued.",
                Who::Owner,
                "Nothing to do now. Start another approval after this one is finished.",
            )
            .code("AS-50")
        } else if r.starts_with("AS-68:") {
            info(
                "Waiting for another payment",
                "The coins of the payment that was dropped are used by another payment of this approval.",
                Who::Owner,
                "Nothing to do now. Start another approval after this one is finished.",
            )
            .code("AS-68")
        } else if r.starts_with("SP-21:") {
            fault(
                "No change address left",
                "The wallet has no unused change address to give this payment.",
                "SP-21",
            )
        } else if r.starts_with("AS-56:") && r.contains("fee did not settle") {
            stop(
                "Fee could not be shared",
                "The wallet could not settle on a fee that every refund in this group can share.",
                Who::Owner,
                "Try again with a different fee rate, or set the shop to pay the fee, or cancel the refund.",
            )
            .code("AS-56")
        } else if r.starts_with("AS-56:") && r.contains("every refund") {
            stop(
                "Refunds too small after fee",
                "Every refund in this group is too small to send once it pays its share of the fee.",
                Who::Owner,
                "Set the shop to pay the fee, or cancel these refunds.",
            )
            .code("AS-56")
        } else if r.starts_with("AS-56:") {
            stop(
                "Refund too small after fee",
                "This refund is too small to send once it pays its share of the fee, so it stays queued.",
                Who::Owner,
                "Set the shop to pay the fee, or cancel the refund.",
            )
            .code("AS-56")
        } else {
            stop(
                "Entry left in the queue",
                "The wallet could not build a payment for this entry, so it stays queued.",
                Who::Owner,
                "Read the details, change the amount, the fee or the packing to match, then try again.",
            )
        };
        words.detail(format!("entry {}: {}", self.id, self.reason))
    }
}

impl ItemOutcome {
    /// What the owner is told about one request of an approval when the
    /// Key app's response was taken. The key holder's reason, when there
    /// is one, is quoted.
    pub fn words(&self) -> Words {
        match self {
            Self::Delivered(id) => Words::new(
                Tone::Good,
                "Approved and signed",
                "The Key app approved this request, and its signatures were checked.",
                Who::Owner,
                "Send the payment or hand on the file, then report it here.",
            )
            .code("AS-62")
            .detail(format!("finished item {id}")),
            Self::Returned(reason) => stop(
                "The Key app sent it back",
                "The Key app did not approve this request, and it is back in the queue.",
                Who::Owner,
                "Read the key holder's reason, then queue it again or cancel it.",
            )
            .code("AS-63")
            .quoted(Quoted::new("The key holder wrote", reason)),
            Self::Healthy => Words::new(
                Tone::Good,
                "Key app answered the check",
                "The Key app paired with this phone answered the health check correctly.",
                Who::Nobody,
                "Nothing to do.",
            )
            .code("AS-14"),
        }
    }
}

/// SC-12: the words of every state and refusal of this crate, with at least
/// one value for every variant and every variant inside it. Built by hand
/// so that a new variant is noticed.
pub fn all_words() -> Vec<(String, Words)> {
    use bitcoin::hashes::Hash;
    let txid = Txid::all_zeros();
    let mut all: Vec<(String, Words)> = Vec::new();
    let mut add = |n: &str, w: Words| all.push((n.to_string(), w));

    for kind in ALL_KINDS {
        let k = format!("{kind:?}");
        let stages = [
            ("Queued", Stage::Queued),
            ("InSession", Stage::InSession { session: [0; 32] }),
            (
                "Returned",
                Stage::Returned {
                    reason: "The amount does not match the receipt".to_string(),
                },
            ),
            (
                "Dropped",
                Stage::Returned {
                    reason: REASON_DROPPED.to_string(),
                },
            ),
            ("Signed", Stage::Signed { txid }),
            ("Sent", Stage::Sent { txid }),
            ("Paid", Stage::Paid { txid }),
        ];
        for (n, s) in stages {
            add(&format!("Stage::{n}/{k}"), s.words(kind));
        }
    }

    for what in FULLS {
        add(
            &format!("Refusal::Full/{what}"),
            Refusal::Full(what).words(),
        );
    }
    for what in BAD_TEXTS {
        add(
            &format!("Refusal::BadText/{what}"),
            Refusal::BadText(what).words(),
        );
    }
    add("Refusal::DestinationKind", Refusal::DestinationKind.words());
    add(
        "Refusal::BelowDust",
        Refusal::BelowDust {
            sats: 100,
            dust: 294,
        }
        .words(),
    );
    add("Refusal::AmountTooLarge", Refusal::AmountTooLarge.words());
    add("Refusal::UnknownSale", Refusal::UnknownSale.words());
    add(
        "Refusal::MoreThanReceived",
        Refusal::MoreThanReceived {
            received: 10_000,
            wanted: 12_345,
        }
        .words(),
    );
    add("Refusal::SaleInUse", Refusal::SaleInUse.words());
    add("Refusal::UnknownEntry", Refusal::UnknownEntry(7).words());
    for what in WRONG_STAGES {
        add(
            &format!("Refusal::WrongStage/{what}"),
            Refusal::WrongStage(what).words(),
        );
    }
    add("Refusal::BatchNotDerived", Refusal::BatchNotDerived.words());
    add(
        "Refusal::PairingRequest",
        Refusal::PairingRequest("a sample".to_string()).words(),
    );
    add(
        "Refusal::SettingsInvalid",
        Refusal::SettingsInvalid("a sample".to_string()).words(),
    );
    add(
        "Refusal::SettingsVersion",
        Refusal::SettingsVersion { known: 5, got: 4 }.words(),
    );
    add("Refusal::ZeroFeeRate", Refusal::ZeroFeeRate.words());
    for (n, e) in [
        ("TooLarge", EncodeError::TooLarge),
        ("ItemCount", EncodeError::ItemCount),
        ("ItemOrder", EncodeError::ItemOrder),
        ("BadText", EncodeError::BadText("a sample")),
        ("BadItem", EncodeError::BadItem("a sample")),
        ("BadPayload", EncodeError::BadPayload("a sample")),
    ] {
        add(&format!("Refusal::Encode/{n}"), Refusal::Encode(e).words());
    }
    add("Refusal::SessionIdUsed", Refusal::SessionIdUsed.words());
    for (n, e) in decodes() {
        add(&format!("Refusal::Frame/{n}"), Refusal::Frame(e).words());
    }
    add("Refusal::UnknownSession", Refusal::UnknownSession.words());
    for (n, e) in [
        ("WrongApprover", BindError::WrongApprover),
        ("BadSignature", BindError::BadSignature),
        ("WrongSession", BindError::WrongSession),
        ("WrongRequest", BindError::WrongRequest),
        ("UnknownItem", BindError::UnknownItem { item: 2 }),
        ("MissingItem", BindError::MissingItem { item: 2 }),
        ("KindMismatch", BindError::KindMismatch { item: 2 }),
        ("BadHealthProof", BindError::BadHealthProof { item: 2 }),
    ] {
        add(&format!("Refusal::Bind/{n}"), Refusal::Bind(e).words());
    }
    add("Refusal::OutboxFull", Refusal::OutboxFull.words());
    add(
        "Refusal::UnknownTransaction",
        Refusal::UnknownTransaction(txid).words(),
    );
    add(
        "Refusal::SnapshotTooLarge",
        Refusal::SnapshotTooLarge.words(),
    );
    add("Refusal::Key1Wallet", Refusal::Key1Wallet.words());
    add(
        "OwnerError::Store",
        OwnerError::<String>::Store("disk full".to_string()).words(),
    );

    add(
        "SetupError::Identity/InvalidSecret",
        SetupError::Identity(IdentityError::InvalidSecret).words(),
    );
    add(
        "SetupError::Identity/InvalidPublicKey",
        SetupError::Identity(IdentityError::InvalidPublicKey).words(),
    );
    add("SetupError::SameRoot", SetupError::SameRoot.words());
    for (n, e) in decodes() {
        add(
            &format!("RestoreError::Decode/{n}"),
            RestoreError::Decode(e).words(),
        );
    }
    add(
        "RestoreError::WrongWallet",
        RestoreError::WrongWallet.words(),
    );
    add("RestoreError::WrongOwner", RestoreError::WrongOwner.words());
    add(
        "RestoreError::WrongApprover",
        RestoreError::WrongApprover.words(),
    );
    add(
        "RestoreError::Inconsistent",
        RestoreError::Inconsistent("a queued batch is not derived").words(),
    );

    for (n, reason) in SKIPPED {
        add(
            &format!("Skipped::{n}"),
            Skipped {
                id: 3,
                reason: reason.to_string(),
            }
            .words(),
        );
    }
    add("ItemOutcome::Delivered", ItemOutcome::Delivered(5).words());
    add(
        "ItemOutcome::Returned",
        ItemOutcome::Returned("The amount does not match the receipt".to_string()).words(),
    );
    add("ItemOutcome::Healthy", ItemOutcome::Healthy.words());
    all
}

/// The things a bound can be about, as the code names them; the last is one
/// the words do not know.
pub const FULLS: [&str; 10] = [
    "the numbering",
    "the refund queue",
    "the cash-out queue",
    "the batch queue",
    "the pairing queue",
    "the settings queue",
    "the list of sales' facts",
    "the list of open sessions",
    "the list of outpoints kept for a refund",
    "a list that is not known",
];

/// The texts that can be refused, as the code names them; the last is one
/// the words do not know.
pub const BAD_TEXTS: [&str; 4] = [
    "cashier",
    "cash-out label",
    "currency code",
    "something else",
];

/// The reasons a call can give for a stage that does not allow it.
pub const WRONG_STAGES: [&str; 6] = [
    "it is in a session; abandon the session first",
    "it was signed; its transaction decides its end",
    "its dropped transaction may still confirm; ask for it again",
    "it is not returned",
    "a transaction is followed with broadcast, confirmed or dropped",
    "a reason that is not known",
];

/// The reasons an entry stays queued when a session is built, as the
/// session builder words them, with a name for each.
pub const SKIPPED: [(&str, &str); 7] = [
    ("Full", "AS-50: the session holds no more items"),
    (
        "Blocked",
        "AS-68: the outpoints its dropped transaction spent are used by another transaction of \
         this session; it waits for the next",
    ),
    ("NoChangeIndex", "SP-21: no change index is left"),
    ("FeeNotSettled", "AS-56: the fee did not settle"),
    (
        "EveryRefundTooSmall",
        "AS-56: every refund is below the dust limit once it pays its share of the fee",
    ),
    (
        "RefundTooSmall",
        "AS-56: its amount after its share of the fee is below the dust limit",
    ),
    (
        "PlanRefused",
        "SP-12: 400 sat is left after the fee, below the dust limit of 546 sat",
    ),
];

/// One value of every kind of decoder refusal, with a name for each.
fn decodes() -> [(&'static str, DecodeError); 8] {
    [
        ("Truncated", DecodeError::Truncated),
        ("TrailingBytes", DecodeError::TrailingBytes),
        ("BadMagic", DecodeError::BadMagic),
        ("UnsupportedVersion", DecodeError::UnsupportedVersion(9)),
        ("TooLarge", DecodeError::TooLarge),
        ("NonCanonical", DecodeError::NonCanonical("a sample")),
        ("InvalidKey", DecodeError::InvalidKey),
        ("UnknownKind", DecodeError::UnknownKind(77)),
    ]
}

/// SC-12: the rows of this crate's words in the one table of
/// `docs/screens/owner.md`, one line each, in the order of [`all_words`].
pub fn screens_markdown() -> String {
    let cell = |t: &str| t.replace('|', "/");
    let mut out = String::new();
    for (name, w) in all_words() {
        out.push_str(&format!(
            "| owner: {name} | {} | {} | {} | {:?} | {} |\n",
            cell(&w.heading),
            cell(&w.what),
            cell(&w.next),
            w.who,
            w.code.unwrap_or("")
        ));
    }
    out
}

#[cfg(test)]
#[path = "say_tests.rs"]
mod tests;
