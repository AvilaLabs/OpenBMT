//! What a cashier or a manager is told (`docs/SCREEN-RULES.md`, SC-01 to
//! SC-12).
//!
//! Every state of the sale screen and every refusal of the counter becomes
//! one [`Words`]. The `Display` texts with rule numbers stay for logs and
//! tests; a person is shown these.
//!
//! Who reads what: the cashier reads the sale screen and the refusals of
//! selling, cancelling and refunding. The manager reads the refusals of
//! accepting a sale by hand and of a large refund. The owner reads what the
//! cashier is told to pass on: the refusals of the owner's own commands and
//! check-ins, and the counter's own faults. A cashier is never told to fix
//! the counter's connection, clock or peers; the cashier takes another kind
//! of payment and tells the owner, and the owner-facing detail goes in
//! `detail`.

use std::fmt::Display;

use openbmt_core::command::CommandError;
use openbmt_core::heartbeat::HeartbeatError;
use openbmt_core::policy::{CheckMode, InstantLabel, OffReason, WaitReason};
use openbmt_core::say::{sats, Quoted, Tone, Who, Words};
use openbmt_watch::say::RefusalGroup;
use openbmt_watch::Problem;

use crate::bill_say::all_bill_words;
use crate::desk::Role;
use crate::heartbeat::{HeartbeatView, NotVerified};
use crate::refund::{RefundState, RefundView};
use crate::refusal::{
    BatchTakeRefused, BlockError, CancelRefused, CommandRefused, DestinationRefused,
    HeartbeatRefused, JudgeError, LightningRefused, LoadError, NoteRefused, OpenRefused,
    OverrideRefused, RangeTakeRefused, RefundRefused, RequoteRefused, SightingError, StaffRefused,
    TotalsRefused, WithdrawRefused,
};
use crate::screen::{Color, Next, Screen, State, Why};
use crate::store::StoreFault;

/// What to do about a price that is too old.
const STALE_NEXT: &str =
    "Wait for a new price, or check the internet connection. If it stays old, tell the owner.";
/// What to do when a sale cannot be found.
const NOT_FOUND_NEXT: &str =
    "Look the sale up again. If it is missing, tell the owner: the counter lost a sale.";
/// What to do about a request that asked for too much.
const CHECK_AMOUNT_NEXT: &str = "Check the amount and enter it again.";
/// The goods stay, another kind of payment is taken, and the owner is told.
const OTHER_PAYMENT_NEXT: &str =
    "Take another kind of payment. Tell the owner: the counter cannot check payments.";

/// CR-276: a placeholder for words that are not written yet. Its name is in
/// the heading and both sentences, and names the rule.
fn needed(placeholder: &'static str, who: Who, code: &'static str) -> Words {
    Words::new(Tone::Ended, placeholder, placeholder, who, placeholder).code(code)
}

/// Words for a refusal: nothing happened, so the tone is neutral.
pub(crate) fn refused(
    heading: &str,
    what: impl Into<String>,
    who: Who,
    next: &str,
    code: &'static str,
) -> Words {
    Words::new(Tone::Ended, heading, what, who, next).code(code)
}

/// SC-10: the program's own fault, a write that did not happen. `undone`
/// says which step was not done ("the sale was not started"), so the cases
/// differ; the system's text goes in the detail.
pub(crate) fn store_failed<E: Display>(
    fault: &StoreFault<E>,
    heading: &str,
    undone: &str,
    tone: Tone,
    who: Who,
    next: &str,
) -> Words {
    let how = match fault {
        StoreFault::Write(_) => "could not save it",
        StoreFault::Read(_) => "could not read its saved records",
        StoreFault::Damaged { .. } => "found a saved record damaged",
        StoreFault::Record(_) => "could not write its record",
    };
    Words::new(
        tone,
        heading,
        format!("The counter {how}, so {undone}."),
        who,
        next,
    )
    .code("CR-40")
    .detail(fault.to_string())
}

/// What to do after a failed write when the step can be tried once more.
pub(crate) const RETRY_NEXT: &str =
    "Try once more. If it fails again, tell the owner: the counter cannot save. See Details.";
/// The same, for a step that sells.
const RETRY_OTHER_NEXT: &str = "Try once more. If it fails again, take another kind of payment and tell the owner: the counter cannot save. See Details.";

/// The tone of a colour (CR-10, SC-06).
fn tone_of(color: Color) -> Tone {
    match color {
        Color::Blue => Tone::Info,
        Color::Green => Tone::Good,
        Color::Red => Tone::Stop,
        Color::Neutral => Tone::Ended,
    }
}

/// The red state of a payment that cannot be handed over.
fn do_not_hand_over(what: &str, tell: &str, code: &'static str) -> Words {
    Words::new(
        Tone::Stop,
        "Do not hand over",
        what,
        Who::Cashier,
        format!("Take another kind of payment and tell the owner: {tell}. If the goods are gone, tell the owner now."),
    )
    .code(code)
}

impl Why {
    /// What the sale screen says for this reason. The colour is the
    /// state's ([`Screen::words`] sets it); the text is the reason's.
    pub fn words(&self) -> Words {
        match self {
            Self::NothingArrived => Words::new(
                Tone::Info,
                "Not paid yet",
                "Nothing has arrived yet.",
                Who::Customer,
                "The customer scans the code on the screen and pays.",
            )
            .code("CR-10"),
            Self::Part {
                received_sats,
                needs_sats,
            } => Words::new(
                Tone::Info,
                "Part paid",
                format!(
                    "The customer has paid part of it and is short by {}.",
                    sats(needs_sats.saturating_sub(*received_sats))
                ),
                Who::Customer,
                "The customer pays the rest to the same code.",
            )
            .code("CR-10"),
            Self::NotSeenEnough => Words::new(
                Tone::Info,
                "Waiting to be seen",
                "The payment has been seen, but not by enough of the network yet.",
                Who::Nobody,
                "Nothing to do. The counter keeps watching, and it turns green when the payment is in a block.",
            )
            .code("CR-10"),
            Self::Wait(reason) => reason.words(),
            Self::InstantOff(reason) => reason.words(),
            Self::NotStored => Words::new(
                Tone::Info,
                "Waiting: not saved",
                "The counter could not save that it accepted this payment at once, so this sale waits for a block.",
                Who::Cashier,
                "Wait for a block, in about ten minutes. Tell the owner: the counter cannot save.",
            )
            .code("CR-13"),
            Self::PauseNotSaved => Words::new(
                Tone::Info,
                "Waiting: pause not saved",
                "The pause after a cancel is not saved yet, so this sale waits for a block.",
                Who::Cashier,
                "Wait for a block, in about ten minutes. Tell the owner: the counter cannot save.",
            )
            .code("CR-14"),
            Self::Instant(label) => label.words(),
            Self::PaidByLightning => Words::new(
                Tone::Good,
                "Paid over Lightning",
                "The customer paid over Lightning and the shop's node has the money. It is final.",
                Who::Cashier,
                "Hand over the goods. A refund for it is given in cash, never over Lightning.",
            )
            .code("CR-109"),
            Self::ConfirmedAt(_) => Words::new(
                Tone::Good,
                "Paid",
                "The payment is in a block.",
                Who::Cashier,
                "Hand over the goods. It is settled after a few more blocks; refunds are paid out only then.",
            )
            .code("CR-10"),
            Self::FinalAt(_) => Words::new(
                Tone::Good,
                "Paid",
                "The payment is settled, so a refund can be paid out if the customer asks.",
                Who::Cashier,
                "Hand over the goods if that is not done yet. Nothing else is left to do.",
            )
            .code("CR-10"),
            Self::OverriddenBy { role } => Words::new(
                Tone::Good,
                "Accepted by a person",
                format!(
                    "A {} accepted this sale by hand, and the payment is not in a block yet.",
                    role.text()
                ),
                Who::Cashier,
                "Hand over the goods. It is settled when the payment is in a block, in about ten minutes.",
            )
            .code("CR-20"),
            Self::Replaced => do_not_hand_over(
                "Another payment took this one's place.",
                "this payment was replaced",
                "CR-12",
            ),
            Self::Reopened => do_not_hand_over(
                "The block that held this payment was dropped by the network.",
                "the block of this payment was dropped",
                "CR-12",
            ),
            Self::CannotCheck(problem) => {
                let words = do_not_hand_over(
                    "This counter cannot check payments right now.",
                    "the counter cannot check payments",
                    "CR-10",
                );
                match problem {
                    Some(p) => {
                        let owner = p.words();
                        words.detail(format!("{}: {} {}", owner.code.unwrap_or("WR-50"), owner.what, owner.next))
                    }
                    None => words.detail("The counter has no answer yet about the network."),
                }
            }
            Self::WatcherNeedsAPerson => RefusalGroup::NeedsAPerson.words(),
            Self::StoredFormUnreadable => Words::new(
                Tone::Stop,
                "Needs a person",
                "The saved record of this sale cannot be read, so the counter cannot judge it and nothing was changed.",
                Who::Owner,
                "Do not hand over the goods. The owner looks at this sale on the owner's phone.",
            )
            .code("CR-42"),
            Self::EndedExpired => Words::new(
                Tone::Ended,
                "Expired",
                "The sale expired with nothing received.",
                Who::Cashier,
                "Nothing to do. Start a new sale if the customer still wants to pay.",
            )
            .code("CR-07"),
            Self::EndedCancelled => Words::new(
                Tone::Ended,
                "Cancelled",
                "The sale was cancelled with nothing received.",
                Who::Cashier,
                "Nothing to do. Start a new sale if the customer still wants to pay.",
            )
            .code("CR-07"),
        }
    }
}

impl Screen {
    /// What the sale screen says: the text is the reason's, the colour is
    /// the state's. `next` is not read, because the second sentence follows
    /// from the reason (every `Next` the counter builds goes with one
    /// reason); the notes for `stuck` and `late` are in
    /// [`Screen::flag_words`].
    pub fn words(&self) -> Words {
        let mut words = self.why.words();
        words.tone = tone_of(self.state.color());
        words
    }

    /// CR-105: which check was done on a payment that waits. Only a sale
    /// that is waiting has one: an accepted sale says it in its own words
    /// (`Why::Instant`), and the sales that wait for nothing say nothing.
    pub fn check_line(&self) -> Option<CheckLine> {
        if self.state == State::PaidLightning {
            return Some(CheckLine::LightningFinal);
        }
        if self.state != State::Waiting {
            return None;
        }
        match (self.check_mode, self.own_node_saw, self.own_node_counted) {
            (Some(CheckMode::Light), true, _) => Some(CheckLine::LightOwnNodeUnusable),
            (Some(CheckMode::Light), false, _) => Some(CheckLine::Light),
            (Some(CheckMode::NodeAlsoSeenByLight), _, true) => Some(CheckLine::YourNodeAndLight),
            (Some(CheckMode::NodeAlsoSeenByLight), _, false) => Some(CheckLine::BoxAndLight),
            (Some(CheckMode::BoxReportsOnly), _, _) => Some(CheckLine::BoxOnly),
            (Some(CheckMode::OwnNodeOnly), _, _) => Some(CheckLine::YourNodeOnly),
            (None, true, _) => Some(CheckLine::OwnNodeWaitingForLight),
            (None, false, _) => None,
        }
    }

    /// The words of [`Screen::check_line`], for the line under the state.
    pub fn check_words(&self) -> Option<Words> {
        self.check_line().map(|line| line.words())
    }

    /// The extra lines for the flags of the screen: a payment that is stuck
    /// (CR-18) and a late payment (CR-19).
    pub fn flag_words(&self) -> Vec<Words> {
        let mut out = Vec::new();
        if self.stuck {
            out.push(stuck_words());
        }
        if self.late {
            out.push(late_words());
        }
        out
    }
}

/// CR-105: which check was done on a payment that waits. The screen always
/// says it: a fallback from the own node to the light check is a line of its
/// own and never silent (WR-64).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckLine {
    /// The light check alone, and the own node has not reported the payment.
    Light,
    /// The own node reported the payment and cannot be used now: the light
    /// check is what counts.
    LightOwnNodeUnusable,
    /// The own node reported the payment, can be used, and the light check
    /// saw it too.
    YourNodeAndLight,
    /// The own node reported the payment and the light check has not seen
    /// it from enough of the network yet.
    OwnNodeWaitingForLight,
    /// The Box and the light check.
    BoxAndLight,
    /// The Box alone.
    BoxOnly,
    /// The shop's own node alone, with `node_only` on (WR-66).
    YourNodeOnly,
    /// The payment arrived over Lightning, which is final at once
    /// (CR-109). It is the line of a sale that is paid, not of one that
    /// waits.
    LightningFinal,
}

/// Every [`CheckLine`], for SC-01.
pub const ALL_CHECK_LINES: [CheckLine; 8] = [
    CheckLine::Light,
    CheckLine::LightOwnNodeUnusable,
    CheckLine::YourNodeAndLight,
    CheckLine::OwnNodeWaitingForLight,
    CheckLine::BoxAndLight,
    CheckLine::BoxOnly,
    CheckLine::YourNodeOnly,
    CheckLine::LightningFinal,
];

impl CheckLine {
    /// What the screen says about the check.
    pub fn words(&self) -> Words {
        let (heading, what, who, next) = match self {
            Self::Light => (
                "Light check",
                "This payment was seen by the counter's own light check.",
                Who::Nobody,
                "Nothing to do.",
            ),
            Self::LightOwnNodeUnusable => (
                "Light check, not your node",
                "Your node saw this payment but cannot be used just now, so only the light check counted.",
                Who::Owner,
                "Nothing to do for this sale. The owner can see why on the counter's status screen.",
            ),
            Self::YourNodeAndLight => (
                "Your node and light check",
                "Your own node and the counter's light check both saw this payment.",
                Who::Nobody,
                "Nothing to do.",
            ),
            Self::OwnNodeWaitingForLight => (
                "Your node saw it",
                "Your own node holds this payment, and the light check has not seen it from enough of the network yet.",
                Who::Nobody,
                "Nothing to do. The counter keeps watching for the light check.",
            ),
            Self::BoxAndLight => (
                "Your Box and light check",
                "Your Box and the counter's light check both saw this payment.",
                Who::Nobody,
                "Nothing to do.",
            ),
            Self::BoxOnly => (
                "Your Box reports it",
                "Only your Box reported this payment; the light check has not seen it from enough of the network.",
                Who::Nobody,
                "Nothing to do. The counter keeps watching for the light check.",
            ),
            Self::YourNodeOnly => (
                "Your node checked it",
                "Your own node saw this payment. The shop checks payments with its node alone.",
                Who::Nobody,
                "Nothing to do.",
            ),
            Self::LightningFinal => (
                "Lightning, final",
                "This payment arrived over Lightning, through the shop's node, so no block is needed.",
                Who::Nobody,
                "Nothing to do.",
            ),
        };
        Words::new(Tone::Info, heading, what, who, next).code("CR-105")
    }
}

/// The name of a check line, by an exhaustive match.
pub fn check_line_name(line: CheckLine) -> &'static str {
    match line {
        CheckLine::Light => "Light",
        CheckLine::LightOwnNodeUnusable => "LightOwnNodeUnusable",
        CheckLine::YourNodeAndLight => "YourNodeAndLight",
        CheckLine::OwnNodeWaitingForLight => "OwnNodeWaitingForLight",
        CheckLine::BoxAndLight => "BoxAndLight",
        CheckLine::BoxOnly => "BoxOnly",
        CheckLine::YourNodeOnly => "YourNodeOnly",
        CheckLine::LightningFinal => "LightningFinal",
    }
}

/// CR-18: the payment was seen long ago and is still not in a block.
pub fn stuck_words() -> Words {
    Words::new(
        Tone::Info,
        "Slow to confirm",
        "The payment was seen a long time ago and is still not in a block.",
        Who::Cashier,
        "Tell the owner that this payment is slow; the owner can look at it on the owner's phone.",
    )
    .code("CR-18")
}

/// CR-19: a payment arrived after the sale had ended.
pub fn late_words() -> Words {
    Words::new(
        Tone::Info,
        "Late payment",
        "A payment arrived after this sale had ended.",
        Who::Nobody,
        "Nothing to do. The counter has told the owner, and the customer may be owed a refund.",
    )
    .code("CR-19")
}

/// CR-188: the screen where the customer chooses a tip, before the code
/// shows. `amount` is the sale's amount as the app writes it in the shop's
/// currency.
pub fn tip_choose_words(amount: &str) -> Words {
    Words::new(
        Tone::Info,
        "Add a tip?",
        format!("The sale is {amount}. A tip goes to the shop with it."),
        Who::Customer,
        "Choose a tip, or No tip.",
    )
    .code("CR-188")
}

/// CR-188: the button of a suggested tip: "15% · $1.50". `tip` is the tip as
/// the app writes it in the shop's currency.
pub fn tip_button_label(percent: u8, tip: &str) -> String {
    format!("{percent}% \u{b7} {tip}")
}

/// CR-188: the button for a tip the customer types.
pub const TIP_OTHER_LABEL: &str = "Other amount";
/// CR-188: the button for no tip.
pub const TIP_NONE_LABEL: &str = "No tip";
/// CR-191: the line of Today with the total of the tips.
pub const TIPS_LINE_LABEL: &str = "Tips";

/// CR-191: the line of Today with the tips taken by one person. The name is
/// text from the counter's own list of cashiers.
pub fn tips_person_label(name: &str) -> String {
    format!("Tips taken by {name}")
}

impl<E: Display> OpenRefused<E> {
    /// What the cashier is told when a sale cannot be started.
    pub fn words(&self) -> Words {
        match self {
            Self::Removed => refused(
                "This counter was removed",
                "The owner removed this counter from the shop, so it cannot start a sale.",
                Who::Cashier,
                "Take another kind of payment. Ask the owner: this counter is no longer on the shop's list.",
                "CR-165",
            ),
            Self::RescanPending => refused(
                "Bitcoin not ready yet",
                "The counter is still looking back over the network for payments, so it cannot start a bitcoin sale.",
                Who::Cashier,
                "Take another kind of payment. Tell the owner: the counter is still catching up.",
                "CR-09",
            ),
            Self::Unhealthy(problem) => {
                let words = refused(
                    "Cannot take bitcoin now",
                    "This counter cannot check payments right now.",
                    Who::Cashier,
                    OTHER_PAYMENT_NEXT,
                    "CR-01",
                );
                match problem {
                    Some(p) => {
                        let owner = p.words();
                        words.detail(format!("{}: {} {}", owner.code.unwrap_or("WR-50"), owner.what, owner.next))
                    }
                    None => words.detail("The counter has no answer yet about the network."),
                }
            }
            Self::ZeroAmount => refused(
                "Sale needs an amount",
                "The sale has no amount, so no payment code was made.",
                Who::Cashier,
                "Enter the amount and start the sale again.",
                "CR-05",
            ),
            Self::AmountTooLarge => refused(
                "Sale amount too large",
                "That is more than any real payment could be, so the sale was not started.",
                Who::Cashier,
                CHECK_AMOUNT_NEXT,
                "CR-05",
            ),
            Self::TipTooLarge => refused(
                "Tip not accepted",
                "The tip is more than the sale itself, so it was not added.",
                Who::Cashier,
                "Type a smaller tip, or choose No tip.",
                "CR-190",
            ),
            Self::BelowDust { needs_sats } => refused(
                "Sale too small for bitcoin",
                format!(
                    "The sale is only {}, too small to be worth sending by bitcoin.",
                    sats(*needs_sats)
                ),
                Who::Cashier,
                "Take another kind of payment.",
                "CR-17",
            ),
            Self::BelowMinimum { sats: _, min_sats } => refused(
                "Below the bitcoin minimum",
                format!(
                    "This shop takes bitcoin for sales of {} and up.",
                    sats(*min_sats)
                ),
                Who::Cashier,
                "Take another kind of payment.",
                "CR-75",
            ),
            Self::StaleRate { fetched_at, .. } => refused(
                "Price is too old",
                "The exchange price is too old to start a sale with.",
                Who::Cashier,
                STALE_NEXT,
                "CR-58",
            )
            .at(*fetched_at),
            Self::RateLimited => refused(
                "Too many sales this hour",
                "The counter has handed out as many payment codes as it may in one hour.",
                Who::Cashier,
                "Take another kind of payment, or try again later. Tell the owner if it happens often.",
                "CR-04",
            ),
            Self::NoAddress => refused(
                "No payment code left",
                "The counter has no payment code left to hand out.",
                Who::Cashier,
                "Take another kind of payment. Tell the owner: this counter needs new addresses.",
                "CR-02",
            ),
            Self::AddressInUse => refused(
                "Payment code in use",
                "The next payment code is already used by another sale.",
                Who::Cashier,
                "Take another kind of payment and tell the owner: a payment code was used twice.",
                "CR-02",
            ),
            Self::BadName => refused(
                "Sale not opened",
                "The name of the person at the till must be plain text of 1 to 64 characters, so the sale was not opened.",
                Who::Cashier,
                "Sign in again, or ask a manager to rename the cashier.",
                "CR-61",
            ),
            Self::BadNote => note_not_read(),
            Self::BadReference => refused(
                "Sale not opened",
                "The till reference has a sign or a space that is not allowed, or is over 32 characters, so the sale was not opened.",
                Who::Cashier,
                "Type the reference again as the till shows it, without spaces.",
                "CR-245",
            ),
            Self::AddressNotStored(fault) => store_failed(
                fault,
                "Payment code not saved",
                "no payment code was handed out",
                Tone::Ended,
                Who::Cashier,
                RETRY_OTHER_NEXT,
            )
            .code("CR-03"),
            Self::Store(fault) => store_failed(
                fault,
                "Sale not started",
                "the sale was not started",
                Tone::Ended,
                Who::Cashier,
                RETRY_OTHER_NEXT,
            ),
        }
    }
}

impl<E: Display> JudgeError<E> {
    /// What the cashier is told when the counter could not judge a sale.
    pub fn words(&self) -> Words {
        match self {
            Self::UnknownSale(i) => refused(
                "Sale not found",
                "The counter has no such sale to check.",
                Who::Cashier,
                NOT_FOUND_NEXT,
                "CR-11",
            )
            .detail(format!("Sale number {i}.")),
            Self::Store(fault) => store_failed(
                fault,
                "Sale not checked",
                "the check of this sale was not recorded",
                Tone::Stop,
                Who::Cashier,
                "Do not hand over the goods yet. Ask for the check again. If it fails again, tell the owner: the counter cannot save. See Details.",
            ),
        }
    }
}

impl<E: Display> LightningRefused<E> {
    /// What the cashier is told when a Lightning payment was reported for a
    /// sale and the counter did not add it (CR-109).
    pub fn words(&self) -> Words {
        match self {
            Self::UnknownSale(_) => refused(
                "Sale not found",
                "A Lightning payment was reported for a sale this counter does not hold.",
                Who::Cashier,
                NOT_FOUND_NEXT,
                "CR-11",
            ),
            Self::SaleClosed => refused(
                "Lightning payment not added",
                "The shop's node got a Lightning payment after this sale ended, so the counter did not add it.",
                Who::Cashier,
                "Take another kind of payment or give the money back. Tell the owner: a Lightning payment came late.",
                "CR-109",
            ),
            Self::PaidAnotherWay => refused(
                "Paid twice",
                "This sale was paid another way and also over Lightning, so the counter kept the first payment.",
                Who::Cashier,
                "Tell the owner: this customer paid twice, and the Lightning payment is in the shop's node.",
                "CR-109",
            ),
            Self::TooLittle { .. } => refused(
                "Lightning payment too small",
                "The shop's node got less over Lightning than this sale asks, so it was not taken as payment.",
                Who::Cashier,
                "Take another kind of payment. Tell the owner: the Lightning payment was too small.",
                "CR-109",
            ),
            Self::Judge(_) => refused(
                "Lightning payment not added",
                "The counter could not check this sale just now, so it did not add the Lightning payment.",
                Who::Cashier,
                "Do not hand over the goods yet. The counter asks again by itself; tell the owner if this stays.",
                "CR-109",
            ),
            Self::Store(fault) => store_failed(
                fault,
                "Lightning payment not saved",
                "the Lightning payment was not added to the sale",
                Tone::Stop,
                Who::Cashier,
                "Do not hand over the goods yet. The counter asks again by itself; tell the owner if this stays.",
            ),
        }
    }
}

impl<E: Display> RequoteRefused<E> {
    /// What the cashier is told when a sale cannot be given a new price.
    pub fn words(&self) -> Words {
        match self {
            Self::UnknownSale(i) => refused(
                "Sale not found",
                "The counter has no such sale to give a new price.",
                Who::Cashier,
                NOT_FOUND_NEXT,
                "CR-11",
            )
            .detail(format!("Sale number {i}.")),
            Self::SaleEnded => refused(
                "Sale already ended",
                "The sale has ended, so it takes no new price.",
                Who::Cashier,
                "Start a new sale.",
                "CR-05",
            ),
            Self::AlreadyEnough => refused(
                "Already paid enough",
                "Enough has arrived for this sale, so it takes no new price.",
                Who::Nobody,
                "Nothing to do. The sale screen says when to hand over the goods.",
                "CR-08",
            ),
            Self::TooManyQuotes => refused(
                "Too many new prices",
                "This sale has had as many new prices as it can keep.",
                Who::Cashier,
                "Start a new sale. If the customer already paid on this one, tell the owner.",
                "CR-05",
            ),
            Self::IsRequest => refused(
                "Request keeps its price",
                "A request to send keeps the price it was made with, so it takes no new price.",
                Who::Nobody,
                "Nothing to do.",
                "CR-207",
            ),
            Self::AmountTooLarge => refused(
                "New price too large",
                "That is more than any real payment could be, so the price was not changed.",
                Who::Cashier,
                CHECK_AMOUNT_NEXT,
                "CR-05",
            ),
            Self::BelowMinimum { sats: _, min_sats } => refused(
                "Below the bitcoin minimum",
                format!(
                    "At the new price this sale is under the smallest bitcoin sale this shop takes, {}.",
                    sats(*min_sats)
                ),
                Who::Cashier,
                "Cancel this sale and take another kind of payment.",
                "CR-75",
            ),
            Self::StaleRate { fetched_at, .. } => refused(
                "Price too old to renew",
                "The exchange price is too old to give this sale a new price.",
                Who::Cashier,
                STALE_NEXT,
                "CR-58",
            )
            .at(*fetched_at),
            Self::Store(fault) => store_failed(
                fault,
                "New price not saved",
                "the sale keeps its old price",
                Tone::Ended,
                Who::Cashier,
                RETRY_NEXT,
            ),
        }
    }
}

/// CR-206: a note that is not plain text of 1 to 64 characters.
fn note_not_read() -> Words {
    refused(
        "Note not accepted",
        "A note must be plain text of 1 to 64 characters, so it was not saved.",
        Who::Cashier,
        "Type it again, shorter.",
        "CR-206",
    )
}

impl<E: Display> NoteRefused<E> {
    /// What the cashier is told when a note is not added to a sale.
    pub fn words(&self) -> Words {
        match self {
            Self::UnknownSale(i) => refused(
                "Sale not found",
                "The counter has no such sale to add a note to.",
                Who::Cashier,
                NOT_FOUND_NEXT,
                "CR-11",
            )
            .detail(format!("Sale number {i}.")),
            Self::BadNote => note_not_read(),
            Self::AlreadyNoted => refused(
                "Note already added",
                "This sale has a note already, so the new one was not saved.",
                Who::Nobody,
                "Nothing to do.",
                "CR-206",
            ),
            Self::Store(fault) => store_failed(
                fault,
                "Note not saved",
                "the note was not added",
                Tone::Ended,
                Who::Cashier,
                RETRY_NEXT,
            ),
        }
    }
}

impl<E: Display> CancelRefused<E> {
    /// What the cashier is told when a sale cannot be cancelled.
    pub fn words(&self) -> Words {
        match self {
            Self::UnknownSale(i) => refused(
                "Sale not found",
                "The counter has no such sale to cancel.",
                Who::Cashier,
                NOT_FOUND_NEXT,
                "CR-11",
            )
            .detail(format!("Sale number {i}.")),
            Self::AlreadyEnded => refused(
                "Sale already ended",
                "The sale has already ended, so there is nothing to cancel.",
                Who::Nobody,
                "Nothing to do.",
                "CR-07",
            ),
            Self::MoneyArrived => refused(
                "Cannot cancel: money arrived",
                "Something has arrived for this sale, so it cannot be cancelled.",
                Who::Cashier,
                "If the customer should get it back, start a refund on this sale.",
                "CR-07",
            ),
            Self::BadName => refused(
                "Sale not cancelled",
                "The name of the person at the till must be plain text of 1 to 64 characters, so the sale was not cancelled.",
                Who::Cashier,
                "Sign in again, or ask a manager to rename the cashier.",
                "CR-61",
            ),
            Self::Store(fault) => store_failed(
                fault,
                "Cancel not saved",
                "the sale was not cancelled",
                Tone::Ended,
                Who::Cashier,
                RETRY_NEXT,
            ),
        }
    }
}

/// The words for a sale that cannot be accepted by hand in this state. The
/// state names its own words, so the match is exhaustive.
fn wrong_state(state: State) -> Words {
    let name = state.name();
    let heading = "Cannot accept by hand";
    match state {
        State::NotPaidYet => refused(
            heading,
            "Nothing has arrived for this sale, so there is nothing to accept.",
            Who::Customer,
            "The customer pays, or take another kind of payment.",
            "CR-20",
        ),
        State::PartPaid | State::Waiting | State::DoNotHandOver => refused(
            heading,
            format!("A sale that shows \"{name}\" can be accepted by hand, but the counter would not do it now."),
            Who::Cashier,
            "Try once more. If it fails again, tell the owner.",
            "CR-20",
        ),
        State::PaidLightning => refused(
            heading,
            "This sale was paid over Lightning, so nobody needs to accept it.",
            Who::Nobody,
            "Nothing to do. The sale screen says when to hand over the goods.",
            "CR-20",
        ),
        State::PaidInstant | State::Confirmed | State::Final | State::Overridden => refused(
            heading,
            format!("This sale already shows \"{name}\", so nobody needs to accept it."),
            Who::Nobody,
            "Nothing to do. The sale screen says when to hand over the goods.",
            "CR-20",
        ),
        State::Expired | State::Cancelled => refused(
            heading,
            format!("This sale shows \"{name}\": it ended with nothing received."),
            Who::Cashier,
            "Start a new sale if the customer still wants to pay.",
            "CR-20",
        ),
        State::NeedsAPerson => refused(
            heading,
            "This sale needs a person to look at it, so it cannot be accepted by hand.",
            Who::Owner,
            "Do not hand over the goods. The owner looks at this sale on the owner's phone.",
            "CR-20",
        ),
    }
}

impl<E: Display> OverrideRefused<E> {
    /// What the cashier or manager is told when a sale cannot be accepted by
    /// hand.
    pub fn words(&self) -> Words {
        match self {
            Self::UnknownSale(i) => refused(
                "Sale not found",
                "The counter has no such sale to accept by hand.",
                Who::Cashier,
                NOT_FOUND_NEXT,
                "CR-20",
            )
            .detail(format!("Sale number {i}.")),
            Self::BadName => refused(
                "Name needed",
                "A person who accepts a sale by hand needs a short name.",
                Who::Cashier,
                "Type the name and try again.",
                "CR-22",
            ),
            Self::WrongState(state) => wrong_state(*state),
            Self::OverSaleLimit { sats: amount, limit } => refused(
                "Over the cashier's limit",
                format!(
                    "{} is more than the {} a cashier may accept by hand.",
                    sats(*amount),
                    sats(*limit)
                ),
                Who::Manager,
                "Ask a manager to accept it.",
                "CR-21",
            ),
            Self::OverDailyCap {
                sats: amount,
                already,
                cap,
            } => refused(
                "Over the cashier's daily cap",
                format!(
                    "Accepting {} by hand, on top of {} in the last day, is over the cashier cap of {}.",
                    sats(*amount),
                    sats(*already),
                    sats(*cap)
                ),
                Who::Manager,
                "Ask a manager to accept it.",
                "CR-21",
            ),
            Self::OverCeiling {
                sats: amount,
                already,
                ceiling,
            } => refused(
                "Over the manager's ceiling",
                format!(
                    "Accepting {} by hand, on top of {} in the last day, is over the manager ceiling of {}.",
                    sats(*amount),
                    sats(*already),
                    sats(*ceiling)
                ),
                Who::Cashier,
                "Take another kind of payment. Tell the owner: this sale is over the manager's limit.",
                "CR-21",
            ),
            Self::TooManyOverrides => refused(
                "Too many accepted by hand",
                "Too many sales were accepted by hand in the last 24 hours.",
                Who::Cashier,
                "Take another kind of payment and tell the owner: too many sales were accepted by hand.",
                "CR-21",
            ),
            Self::NoRoom => refused(
                "No room to accept this",
                "This counter already holds as much accepted before a block as it is allowed, so it cannot take more.",
                Who::Cashier,
                "Wait until earlier sales settle, or take another kind of payment and tell the owner.",
                "CR-20",
            ),
            Self::Judge(JudgeError::UnknownSale(i)) => refused(
                "Sale not found",
                "The counter has no such sale to check before accepting it by hand.",
                Who::Cashier,
                NOT_FOUND_NEXT,
                "CR-11",
            )
            .detail(format!("Sale number {i}.")),
            Self::Judge(JudgeError::Store(fault)) => store_failed(
                fault,
                "Sale not checked",
                "the sale was not checked and nothing was accepted by hand",
                Tone::Stop,
                Who::Cashier,
                "Do not hand over the goods yet. Try once more. If it fails again, tell the owner: the counter cannot save. See Details.",
            ),
            Self::Store(fault) => store_failed(
                fault,
                "Acceptance not saved",
                "the sale was not accepted by hand",
                Tone::Stop,
                Who::Cashier,
                "Do not hand over the goods. Try once more, or tell the owner: the counter cannot save. See Details.",
            ),
        }
    }
}

/// The three ways the owner's signed message can be refused, as the owner
/// is told. `what` names the message: the command or the check-in.
struct Signed {
    wrong_counter: (&'static str, &'static str, &'static str),
    stale: (&'static str, &'static str, &'static str),
    bad_signature: (&'static str, &'static str, &'static str),
}

const COMMAND: Signed = Signed {
    wrong_counter: (
        "Command for another counter",
        "The owner's command names a different counter, so this counter did nothing.",
        "Send the command again from the owner app, to the right counter.",
    ),
    stale: (
        "Command already used",
        "The command's number is not above the last one this counter took, so it was ignored.",
        "If it was not carried out, send it again from the owner app.",
    ),
    bad_signature: (
        "Command not from the owner",
        "The command is not signed by this shop's owner, so this counter ignored it.",
        "Send it again from the owner app. If it keeps failing, check that this counter is paired with your phone.",
    ),
};

const HEARTBEAT: Signed = Signed {
    wrong_counter: (
        "Check-in for another counter",
        "The owner's phone check-in names a different counter, so this counter did not take it.",
        "Nothing to do if the phone also checks in with the right counter. Otherwise check the pairing.",
    ),
    stale: (
        "Check-in already used",
        "The check-in's number is not above the last one this counter took, so it was not taken.",
        "Nothing to do. The phone sends a new check-in by itself.",
    ),
    bad_signature: (
        "Check-in not from the owner",
        "The check-in is not signed by this shop's owner, so this counter did not take it.",
        "Check that this counter is paired with the owner's phone.",
    ),
};

/// The words for the owner's signed message, by the three causes the
/// shared enums name.
fn signed_words(set: &Signed, code: &'static str, cause: SignedCause, detail: String) -> Words {
    let (heading, what, next) = match cause {
        SignedCause::WrongCounter => set.wrong_counter,
        SignedCause::Stale => set.stale,
        SignedCause::BadSignature => set.bad_signature,
    };
    refused(heading, what, Who::Owner, next, code).detail(detail)
}

/// Which of the three causes a signed message was refused for.
#[derive(Clone, Copy)]
enum SignedCause {
    WrongCounter,
    Stale,
    BadSignature,
}

fn command_cause(e: &CommandError) -> SignedCause {
    match e {
        CommandError::WrongCounter => SignedCause::WrongCounter,
        CommandError::Stale { .. } => SignedCause::Stale,
        CommandError::OwnerSignatureInvalid => SignedCause::BadSignature,
    }
}

fn heartbeat_cause(e: &HeartbeatError) -> SignedCause {
    match e {
        HeartbeatError::WrongCounter => SignedCause::WrongCounter,
        HeartbeatError::Stale { .. } => SignedCause::Stale,
        HeartbeatError::OwnerSignatureInvalid => SignedCause::BadSignature,
    }
}

impl<E: Display> CommandRefused<E> {
    /// What the owner is told when the counter ignored a command.
    pub fn words(&self) -> Words {
        match self {
            Self::Refused(e) => signed_words(&COMMAND, "CR-14", command_cause(e), e.to_string()),
            Self::Store(fault) => store_failed(
                fault,
                "Command not carried out",
                "the owner's command was not carried out",
                Tone::Ended,
                Who::Owner,
                "Send the command again. If it fails again, the counter's storage needs a look. See Details.",
            ),
        }
    }
}

impl<E: Display> HeartbeatRefused<E> {
    /// What the owner is told when the counter did not take a check-in.
    pub fn words(&self) -> Words {
        match self {
            Self::Refused(e) => signed_words(&HEARTBEAT, "CR-50", heartbeat_cause(e), e.to_string()),
            Self::FromTheFuture { ahead_secs } => refused(
                "Check-in is from the future",
                "The owner's phone check-in is dated ahead of this counter's clock, so it was not taken.",
                Who::Owner,
                "Check the clock on this counter and on the owner's phone.",
                "CR-51",
            )
            .detail(format!("The check-in is dated {ahead_secs} seconds ahead.")),
            Self::Store(fault) => store_failed(
                fault,
                "Check-in not saved",
                "the owner's phone check-in was not taken",
                Tone::Ended,
                Who::Owner,
                "Nothing to do for now; the phone sends it again. If it keeps failing, the counter's storage needs a look. See Details.",
            )
            .code("CR-54"),
        }
    }
}

impl NotVerified {
    /// What the owner is told when a check-in was taken but does not count.
    pub fn words(&self) -> Words {
        match self {
            Self::FromTheFuture { ahead_secs } => Words::new(
                Tone::Info,
                "Check-in dated ahead",
                "The owner's phone check-in is dated ahead of this counter's clock, so it does not count.",
                Who::Owner,
                "Check the clock on this counter and on the owner's phone.",
            )
            .code("CR-51")
            .detail(format!("The check-in is dated {ahead_secs} seconds ahead.")),
            Self::OwnerUnhealthy => Words::new(
                Tone::Info,
                "Phone check not working",
                "The owner's phone says its own check of the network is not working, so its check-in does not count.",
                Who::Owner,
                "Open the owner app and see why its check is not working.",
            )
            .code("CR-52"),
            Self::TipNotYetHeld => Words::new(
                Tone::Info,
                "Phone sees a newer block",
                "The owner's phone has seen a newer block than this counter has, so its check-in does not count yet.",
                Who::Nobody,
                "Nothing to do. It counts again when this counter catches up.",
            )
            .code("CR-53"),
            Self::TipTooFarAhead => Words::new(
                Tone::Info,
                "Phone is far ahead",
                "The owner's phone is many blocks ahead of this counter, so its check-in does not count.",
                Who::Owner,
                "Check that this counter is online and up to date.",
            )
            .code("CR-53"),
            Self::TipBehind { blocks } => Words::new(
                Tone::Info,
                "Phone is behind",
                "The owner's phone is behind this counter, so its check-in does not count.",
                Who::Owner,
                "Check that the owner's phone is online.",
            )
            .code("CR-53")
            .detail(format!("The phone is {blocks} blocks behind.")),
            Self::TipDisagrees => Words::new(
                Tone::Info,
                "Counter and phone disagree",
                "The owner's phone and this counter hold different blocks, so its check-in does not count.",
                Who::Owner,
                "Wait a few minutes and look again. Until then this counter waits for a block on every sale.",
            )
            .code("CR-53"),
        }
    }
}

impl HeartbeatView {
    /// What the owner is told about the owner's phone check-in.
    pub fn words(&self) -> Words {
        match self {
            Self::Verified { sent_at } => Words::new(
                Tone::Info,
                "Owner phone is watching",
                "The owner's phone has checked in, and its check counts.",
                Who::Nobody,
                "Nothing to do.",
            )
            .code("CR-51")
            .at(*sent_at),
            Self::NotVerified(why) => why.words(),
        }
    }
}

impl<E: Display> SightingError<E> {
    /// What is noted when a payment that was seen could not be taken.
    pub fn words(&self) -> Words {
        match self {
            Self::Refused(refusal) => refusal.words(),
            Self::Store(fault) => store_failed(
                fault,
                "Payment not saved",
                "the payment that was seen was not recorded",
                Tone::Ended,
                Who::Owner,
                "Nothing to do for now; the counter sees it again. If it repeats, the counter's storage needs a look. See Details.",
            ),
        }
    }
}

impl<E: Display> BlockError<E> {
    /// What is noted when a block could not be taken.
    pub fn words(&self) -> Words {
        match self {
            Self::Refused(refusal) => refusal.words(),
            Self::Store(fault) => store_failed(
                fault,
                "Block not saved",
                "the block was not recorded",
                Tone::Ended,
                Who::Owner,
                "Nothing to do for now; the counter takes the next block. If it repeats, the counter's storage needs a look. See Details.",
            ),
        }
    }
}

impl LoadError {
    /// What the owner is told when the counter will not start because a
    /// saved file does not read back (SC-10).
    pub fn words(&self) -> Words {
        let (heading, what, detail) = match self {
            // CR-234: the saved ranges of a counter that makes its own
            // addresses are the same case as saved batches, and are told the
            // same.
            Self::Bills(e) => {
                return Words::new(
                    Tone::Stop,
                    "Saved bills unreadable",
                    "The counter's saved bills and tabs do not read back, so it did not start and nothing was changed.",
                    Who::Owner,
                    "Restart the counter. If it stays, the saved files need to be put back from a copy. See Details.",
                )
                .code("CR-253")
                .detail(e.to_string())
            }
            Self::Batches(e) | Self::Ranges(e) => (
                "Saved addresses unreadable",
                "The counter's saved payment codes do not read back, so it did not start and nothing was changed.",
                e.to_string(),
            ),
            Self::Exposure(e) => (
                "Saved totals unreadable",
                "The counter's saved total of payments accepted before a block does not read back, so it did not start.",
                e.to_string(),
            ),
            Self::Pause(e) => (
                "Saved pause unreadable",
                "The counter's saved pause state does not read back, so it did not start and nothing was changed.",
                e.to_string(),
            ),
            Self::Desk(e) => (
                "Saved counter unreadable",
                "The counter's own saved state does not read back, so it did not start and nothing was changed.",
                e.to_string(),
            ),
            Self::Records(e) => (
                "Saved records do not match",
                "The counter's saved sale records do not check out, so it did not start and nothing was changed.",
                e.to_string(),
            ),
            Self::RecordsUnread(e) => (
                "Saved records unreadable",
                "The counter's saved sale records could not be read back, so it did not start and nothing was changed.",
                e.clone(),
            ),
        };
        Words::new(
            Tone::Stop,
            heading,
            what,
            Who::Owner,
            "Restart the counter. If it stays, the saved files need to be put back from a copy. See Details.",
        )
        .code("CR-42")
        .detail(detail)
    }
}

impl<E> BatchTakeRefused<E> {
    /// What the owner is told when a batch is sent to a counter that makes
    /// its own addresses from the shop's wallet (CR-235). The other refusals
    /// of a batch are told by the app that sent it.
    pub fn own_wallet_words() -> Words {
        Words::new(
            Tone::Info,
            "Batch not needed",
            "This counter makes its own addresses from the shop's wallet, so a batch of addresses was not taken.",
            Who::Owner,
            "Nothing to do. If this keeps showing, update the owner phone.",
        )
        .code("CR-235")
    }
}

impl<E: Display> RangeTakeRefused<E> {
    /// What the owner is told when a range of indexes was not taken
    /// (CR-235). Nothing changed.
    pub fn words(&self) -> Words {
        match self {
            Self::Range(e) => Words::new(
                Tone::Ended,
                "Address range refused",
                "The owner phone sent addresses that this counter has used or already holds, or that were not signed for this counter. Nothing changed.",
                Who::Owner,
                "Open the owner phone so that it sends a new range.",
            )
            .code("CR-235")
            .detail(e.to_string()),
            Self::KeyApp => Words::new(
                Tone::Info,
                "Range not taken",
                "This counter was paired with the Key app and takes address batches, not ranges.",
                Who::Owner,
                "Pair the counter again from the owner phone if the shop now uses its own wallet.",
            )
            .code("CR-235"),
            Self::Record(_) | Self::Store(_) => Words::new(
                Tone::Ended,
                "Addresses not saved",
                "The counter could not save the new addresses from the owner phone, so it did not take them.",
                Who::Owner,
                "Check that the counter has free storage. The owner phone sends them again by itself.",
            )
            .code("CR-235")
            .detail(self.to_string()),
        }
    }
}

impl DestinationRefused {
    /// What the cashier is told when a scanned refund code cannot be used.
    pub fn words(&self) -> Words {
        let (heading, what, code) = match self {
            Self::TooLong => (
                "Code is too long",
                "The scanned code is too long to be an address.",
                "CR-62",
            ),
            Self::NotPrintable => (
                "Code has odd characters",
                "The scanned code holds characters that an address does not use.",
                "CR-62",
            ),
            Self::NotAnAddress => (
                "Not an address",
                "No bitcoin address could be read from the scanned code.",
                "CR-62",
            ),
            Self::WrongNetwork => (
                "Address for another network",
                "The address is for a different bitcoin network than this shop uses.",
                "CR-63",
            ),
            Self::ScriptKind => (
                "Address type not supported",
                "This counter cannot send a refund to that kind of address.",
                "CR-63",
            ),
        };
        refused(
            heading,
            what,
            Who::Cashier,
            "Scan the code again, or ask the customer for an address from their own wallet.",
            code,
        )
    }
}

impl<E: Display> RefundRefused<E> {
    /// What the cashier or manager is told when a refund cannot be asked
    /// for.
    pub fn words(&self) -> Words {
        match self {
            Self::BadText(what) => refused(
                "Name or reason not usable",
                format!("The {what} must be plain text of 1 to 64 characters."),
                Who::Cashier,
                "Type it again.",
                "CR-61",
            ),
            Self::DestinationKind => refused(
                "Refund address not usable",
                "The refund request holds an address of a kind this counter cannot pay.",
                Who::Cashier,
                "Ask the customer for another address from their own wallet.",
                "CR-63",
            ),
            Self::UnknownSale(i) => refused(
                "Sale not found",
                "The counter has no such sale to refund.",
                Who::Cashier,
                NOT_FOUND_NEXT,
                "CR-64",
            )
            .detail(format!("Sale number {i}.")),
            Self::StaleRate { fetched_at, .. } => refused(
                "Price too old for refund",
                "The exchange price is too old to work out the refund.",
                Who::Cashier,
                STALE_NEXT,
                "CR-58",
            )
            .at(*fetched_at),
            Self::ZeroAmount => refused(
                "Refund needs an amount",
                "A refund of nothing cannot be asked for.",
                Who::Cashier,
                "Enter the amount to refund.",
                "CR-67",
            ),
            Self::AmountTooLarge => refused(
                "Refund amount too large",
                "That is more than any real payment could be, so no refund was asked for.",
                Who::Cashier,
                CHECK_AMOUNT_NEXT,
                "CR-67",
            ),
            Self::BelowDust { sats: amount, dust } => refused(
                "Refund too small to send",
                format!(
                    "{} is too small to be worth sending; the smallest is {}.",
                    sats(*amount),
                    sats(*dust)
                ),
                Who::Cashier,
                "Ask for a bigger amount, or refund it in cash.",
                "CR-67",
            ),
            Self::OwnScript => refused(
                "That is the shop's address",
                "The address is one of this shop's own, so the refund would go back to the shop.",
                Who::Cashier,
                "Ask the customer for an address from their own wallet.",
                "CR-68",
            ),
            Self::NothingReceived => refused(
                "Nothing to refund",
                "Nothing has arrived for this sale, so there is nothing to refund.",
                Who::Cashier,
                "Check that this is the right sale.",
                "CR-65",
            ),
            Self::TooManyRefunds => refused(
                "Too many refund entries",
                "This sale holds as many refund entries as it can keep.",
                Who::Cashier,
                "Tell the owner: this sale cannot take more refund entries.",
                "CR-67",
            ),
            Self::MoreThanReceived {
                received,
                asked,
                sats: amount,
            } => refused(
                "Refund is more than paid",
                format!(
                    "{} on top of {} already asked is more than the {} that arrived.",
                    sats(*amount),
                    sats(*asked),
                    sats(*received)
                ),
                Who::Cashier,
                "Ask for a smaller amount.",
                "CR-67",
            ),
            Self::WithdrawnMayBePaid { .. } => refused(
                "Refund may still be paid",
                "A withdrawn request for this sale may already be in the owner's approval, so a cash refund on top of it is not allowed.",
                Who::Cashier,
                "Ask the owner whether the request was dropped. If it was, a manager can give the cash refund.",
                "CR-119",
            ),
            Self::LightningCashOnly { .. } => refused(
                "Refund in cash",
                "This sale was paid over Lightning, and the owner's phone can only check payments it sees on the chain, so a bitcoin refund cannot be sent.",
                Who::Cashier,
                "Give the refund in cash from the till.",
                "CR-217",
            ),
            Self::NotSettled { .. } => needed(
                "WORDS_NEEDED_CR_71_NOT_SETTLED",
                Who::Cashier,
                "CR-71",
            ),
            Self::NeedsManagerSingle {
                sats: amount,
                limit,
            } => refused(
                "Needs a manager to refund",
                format!(
                    "{} is more than the {} a cashier may refund.",
                    sats(*amount),
                    sats(*limit)
                ),
                Who::Manager,
                "Ask a manager to ask for this refund.",
                "CR-69",
            ),
            Self::NeedsManagerDaily {
                sats: amount,
                already,
                cap,
            } => refused(
                "Refund over the daily cap",
                format!(
                    "Refunding {}, on top of {} in the last day, is over the cashier cap of {}.",
                    sats(*amount),
                    sats(*already),
                    sats(*cap)
                ),
                Who::Manager,
                "Ask a manager to ask for this refund.",
                "CR-69",
            ),
            Self::Store(fault) => store_failed(
                fault,
                "Refund not recorded",
                "the refund was not recorded",
                Tone::Ended,
                Who::Cashier,
                RETRY_NEXT,
            ),
        }
    }
}

impl<E: Display> WithdrawRefused<E> {
    /// What the cashier is told when a refund request cannot be withdrawn.
    pub fn words(&self) -> Words {
        match self {
            Self::BadName => refused(
                "Name not usable",
                "The name must be plain text of 1 to 64 characters.",
                Who::Cashier,
                "Type the name again.",
                "CR-61",
            ),
            Self::UnknownSale(i) => refused(
                "Sale not found",
                "The counter has no such sale to withdraw a refund from.",
                Who::Cashier,
                NOT_FOUND_NEXT,
                "CR-64",
            )
            .detail(format!("Sale number {i}.")),
            Self::UnknownRequest(n) => refused(
                "Refund request not found",
                "This sale has no such refund request to withdraw.",
                Who::Cashier,
                "Look at the sale's list of refunds and choose again.",
                "CR-70",
            )
            .detail(format!("Request number {n}.")),
            Self::AlreadyWithdrawn(n) => refused(
                "Already withdrawn",
                "This refund request was withdrawn before.",
                Who::Nobody,
                "Nothing to do.",
                "CR-70",
            )
            .detail(format!("Request number {n}.")),
            Self::Store(fault) => store_failed(
                fault,
                "Withdrawal not recorded",
                "the refund request was not withdrawn",
                Tone::Ended,
                Who::Cashier,
                RETRY_NEXT,
            ),
        }
    }
}

impl<E: Display> StaffRefused<E> {
    /// What the manager is told when a change to the cashiers was not
    /// written down for the owner (CR-143).
    pub fn words(&self) -> Words {
        match self {
            Self::BadName(which) => refused(
                "Name not usable",
                format!("The {which} must be plain text of 1 to 64 characters, so the change was not written down."),
                Who::Manager,
                "Type the name again.",
                "CR-143",
            ),
            Self::Store(fault) => store_failed(
                fault,
                "Change not written down",
                "the change to the cashiers was not written down for the owner",
                Tone::Ended,
                Who::Manager,
                RETRY_NEXT,
            ),
        }
    }
}

impl<E: Display> TotalsRefused<E> {
    /// What the cashier is told when the day's totals could not be counted
    /// (CR-152, CR-153).
    pub fn words(&self) -> Words {
        match self {
            Self::BadWindow => refused(
                "Day not counted",
                "The start of the day is not before its end, so nothing was counted.",
                Who::Cashier,
                "Close the app and open it again. If it happens again, tell the owner: the phone's clock is wrong.",
                "CR-152",
            ),
            Self::MixedCurrencies => refused(
                "Two currencies today",
                "Today's sales are in two currencies, and the totals cannot add them together.",
                Who::Cashier,
                "Tell the owner. The owner's phone has the full books.",
                "CR-153",
            ),
            Self::Overflow => refused(
                "Day too large to count",
                "A sum of today's sales is too large to count, so nothing is shown.",
                Who::Cashier,
                "Tell the owner. The owner's phone has the full books.",
                "CR-153",
            ),
            Self::TooManyRecords => refused(
                "Too many records today",
                "Today needs more of the counter's records than one count can read, so nothing is shown.",
                Who::Cashier,
                "Tell the owner. The owner's phone has the full books.",
                "CR-152",
            ),
            Self::Store(fault) => store_failed(
                fault,
                "Totals not read",
                "today's totals could not be counted",
                Tone::Ended,
                Who::Cashier,
                RETRY_NEXT,
            ),
        }
    }
}

/// The words for a refund state; `asked` is the amount when the state has
/// one.
fn refund_state_words(state: RefundState, asked: Option<u64>) -> Words {
    let amount = |tail: &str| match asked {
        Some(n) => format!("The request for {} {tail}", sats(n)),
        None => format!("The request {tail}"),
    };
    match state {
        RefundState::Requested => Words::new(
            Tone::Info,
            "Refund requested",
            amount("went to the owner, and this counter is not told when it is paid."),
            Who::Owner,
            "Nothing to do here. The customer is paid from the owner's side; ask the owner to see if it is done.",
        )
        .code("CR-74"),
        RefundState::WaitingForDepth => Words::new(
            Tone::Info,
            "Refund waiting",
            amount("is made, but the sale is not settled yet, so it cannot be paid yet."),
            Who::Nobody,
            "Nothing to do. It can be paid when the sale is settled, or the request can be withdrawn.",
        )
        .code("CR-66"),
        RefundState::Withdrawn => Words::new(
            Tone::Ended,
            "Refund withdrawn",
            amount("was withdrawn."),
            Who::Nobody,
            "Nothing to do. If the owner already paid it, the withdrawal changes nothing.",
        )
        .code("CR-70"),
        RefundState::PaidInCash => Words::new(
            Tone::Ended,
            "Refunded in cash",
            amount("was paid in cash from the till."),
            Who::Nobody,
            "Nothing to do.",
        )
        .code("CR-71"),
    }
}

impl RefundState {
    /// What the cashier is told about a refund in this state.
    pub fn words(&self) -> Words {
        refund_state_words(*self, None)
    }
}

impl RefundView {
    /// What the cashier is told about one refund of a sale: its state, the
    /// amount, when, and the name of who asked as text the cashier wrote.
    pub fn words(&self) -> Words {
        let from = match self.by.role {
            Role::Cashier => "The cashier wrote",
            Role::Manager => "The manager wrote",
        };
        let mut words = refund_state_words(self.state, Some(self.asked)).at(self.at);
        words = words.quoted(Quoted::new(from, &self.by.name));
        match self.number {
            Some(n) => words.detail(format!("Sale number {}, request {n}.", self.sale)),
            None => words.detail(format!("Sale number {}, paid in cash.", self.sale)),
        }
    }
}

/// The name of a variant, by an exhaustive match: a new variant does not
/// compile until it is named here, and the test of SC-01 then wants words
/// for it.
pub fn state_name(s: State) -> &'static str {
    match s {
        State::NotPaidYet => "NotPaidYet",
        State::PartPaid => "PartPaid",
        State::Waiting => "Waiting",
        State::PaidInstant => "PaidInstant",
        State::Confirmed => "Confirmed",
        State::Final => "Final",
        State::Overridden => "Overridden",
        State::PaidLightning => "PaidLightning",
        State::DoNotHandOver => "DoNotHandOver",
        State::Expired => "Expired",
        State::Cancelled => "Cancelled",
        State::NeedsAPerson => "NeedsAPerson",
    }
}

/// The name of a variant (see [`state_name`]).
pub fn why_name(w: &Why) -> &'static str {
    match w {
        Why::NothingArrived => "NothingArrived",
        Why::Part { .. } => "Part",
        Why::NotSeenEnough => "NotSeenEnough",
        Why::Wait(_) => "Wait",
        Why::InstantOff(_) => "InstantOff",
        Why::NotStored => "NotStored",
        Why::PauseNotSaved => "PauseNotSaved",
        Why::Instant(_) => "Instant",
        Why::ConfirmedAt(_) => "ConfirmedAt",
        Why::FinalAt(_) => "FinalAt",
        Why::PaidByLightning => "PaidByLightning",
        Why::OverriddenBy { .. } => "OverriddenBy",
        Why::Replaced => "Replaced",
        Why::Reopened => "Reopened",
        Why::CannotCheck(_) => "CannotCheck",
        Why::WatcherNeedsAPerson => "WatcherNeedsAPerson",
        Why::StoredFormUnreadable => "StoredFormUnreadable",
        Why::EndedExpired => "EndedExpired",
        Why::EndedCancelled => "EndedCancelled",
    }
}

/// The name of a variant (see [`state_name`]).
pub fn next_name(n: &Next) -> &'static str {
    match n {
        Next::CustomerPays => "CustomerPays",
        Next::CustomerPaysRest { .. } => "CustomerPaysRest",
        Next::KeepWatching => "KeepWatching",
        Next::WatchSeconds { .. } => "WatchSeconds",
        Next::WaitForConfirmation { .. } => "WaitForConfirmation",
        Next::FinalAt { .. } => "FinalAt",
        Next::OwnerClears => "OwnerClears",
        Next::AskOwnerOrOtherPayment => "AskOwnerOrOtherPayment",
        Next::PersonLooks => "PersonLooks",
        Next::Nothing => "Nothing",
    }
}

/// The name of a variant (see [`state_name`]).
pub fn refund_state_name(s: RefundState) -> &'static str {
    match s {
        RefundState::Requested => "Requested",
        RefundState::WaitingForDepth => "WaitingForDepth",
        RefundState::Withdrawn => "Withdrawn",
        RefundState::PaidInCash => "PaidInCash",
    }
}

fn screen(state: State, why: Why, next: Next) -> Screen {
    Screen {
        state,
        why,
        next,
        received_sats: 0,
        needs_sats: 0,
        stuck: false,
        late: false,
        check_mode: None,
        own_node_saw: false,
        own_node_counted: false,
        conflict_seen: false,
        reopened: false,
        reports: Vec::new(),
    }
}

/// One screen for every (state, reason, next) combination that the counter
/// builds (`Counter::judge`), with every nested reason. The name says which.
pub fn sample_screens() -> Vec<(String, Screen)> {
    let wait = Next::WaitForConfirmation { depth: 1 };
    let mut all: Vec<(String, Screen)> = vec![
        (
            "NotPaidYet".to_string(),
            screen(State::NotPaidYet, Why::NothingArrived, Next::CustomerPays),
        ),
        (
            "PartPaid".to_string(),
            screen(
                State::PartPaid,
                Why::Part {
                    received_sats: 4_000,
                    needs_sats: 10_000,
                },
                Next::CustomerPaysRest {
                    missing_sats: 6_000,
                },
            ),
        ),
        (
            "Waiting/NotSeenEnough".to_string(),
            screen(State::Waiting, Why::NotSeenEnough, Next::KeepWatching),
        ),
        (
            "Waiting/NotStored".to_string(),
            screen(State::Waiting, Why::NotStored, wait),
        ),
        (
            "Waiting/PauseNotSaved".to_string(),
            screen(State::Waiting, Why::PauseNotSaved, wait),
        ),
        (
            "Confirmed".to_string(),
            screen(
                State::Confirmed,
                Why::ConfirmedAt(1),
                Next::FinalAt { depth: 6 },
            ),
        ),
        (
            "Final".to_string(),
            screen(State::Final, Why::FinalAt(6), Next::Nothing),
        ),
        (
            "DoNotHandOver/Replaced".to_string(),
            screen(
                State::DoNotHandOver,
                Why::Replaced,
                Next::AskOwnerOrOtherPayment,
            ),
        ),
        (
            "DoNotHandOver/Reopened".to_string(),
            screen(
                State::DoNotHandOver,
                Why::Reopened,
                Next::AskOwnerOrOtherPayment,
            ),
        ),
        (
            "DoNotHandOver/CannotCheck".to_string(),
            screen(
                State::DoNotHandOver,
                Why::CannotCheck(None),
                Next::AskOwnerOrOtherPayment,
            ),
        ),
        (
            "NeedsAPerson/WatcherNeedsAPerson".to_string(),
            screen(
                State::NeedsAPerson,
                Why::WatcherNeedsAPerson,
                Next::PersonLooks,
            ),
        ),
        (
            "NeedsAPerson/StoredFormUnreadable".to_string(),
            screen(
                State::NeedsAPerson,
                Why::StoredFormUnreadable,
                Next::PersonLooks,
            ),
        ),
        (
            "PaidLightning".to_string(),
            screen(State::PaidLightning, Why::PaidByLightning, Next::Nothing),
        ),
        (
            "Expired".to_string(),
            screen(State::Expired, Why::EndedExpired, Next::Nothing),
        ),
        (
            "Cancelled".to_string(),
            screen(State::Cancelled, Why::EndedCancelled, Next::Nothing),
        ),
    ];
    for role in [Role::Cashier, Role::Manager] {
        all.push((
            format!("Overridden/{}", role.text()),
            screen(State::Overridden, Why::OverriddenBy { role }, wait),
        ));
    }
    for label in [
        InstantLabel::LightCheck,
        InstantLabel::YourNode,
        InstantLabel::YourBox,
    ] {
        all.push((
            format!(
                "PaidInstant/{}",
                openbmt_core::say_policy::instant_label_name(label)
            ),
            screen(State::PaidInstant, Why::Instant(label), wait),
        ));
    }
    for reason in [
        WaitReason::ConflictSeen,
        WaitReason::RemoteOrder,
        WaitReason::Underpaid,
        WaitReason::OverSaleLimit,
        WaitReason::SliceFull,
        WaitReason::ConflictWatch { secs_left: 5 },
    ] {
        let next = match reason {
            WaitReason::ConflictWatch { secs_left } => Next::WatchSeconds { secs_left },
            WaitReason::ConflictSeen
            | WaitReason::RemoteOrder
            | WaitReason::Underpaid
            | WaitReason::OverSaleLimit
            | WaitReason::SliceFull => wait,
        };
        all.push((
            format!(
                "Waiting/Wait/{}",
                openbmt_core::say_policy::wait_reason_name(reason)
            ),
            screen(State::Waiting, Why::Wait(reason), next),
        ));
    }
    for reason in [
        OffReason::PausedFor { secs_left: 600 },
        OffReason::PausedUntilOwnerReset,
        OffReason::Disabled,
        OffReason::CheckUnhealthy,
        OffReason::NoWatcher,
        OffReason::SharedWifiNeedsTor,
    ] {
        let next = match reason {
            OffReason::PausedUntilOwnerReset => Next::OwnerClears,
            OffReason::PausedFor { .. }
            | OffReason::Disabled
            | OffReason::CheckUnhealthy
            | OffReason::NoWatcher
            | OffReason::SharedWifiNeedsTor => wait,
        };
        all.push((
            format!(
                "Waiting/InstantOff/{}",
                openbmt_core::say_policy::off_reason_name(reason)
            ),
            screen(State::Waiting, Why::InstantOff(reason), next),
        ));
    }
    all
}

/// A store fault for the samples: the platform's text as the caller gave it.
fn write_fault() -> StoreFault<String> {
    StoreFault::Write("the disk is full".to_string())
}

/// A fault of the other kind: the record could not be made.
fn record_fault() -> StoreFault<String> {
    StoreFault::Record(openbmt_core::records::ChainError::BodyTooLarge)
}

/// Every one of these words, for SC-12: at least one value for every
/// variant of every type of this crate and of its nested enums. The words
/// of the policy enums and the watcher are in their own crates' lists.
pub fn all_words() -> Vec<(String, Words)> {
    let mut all: Vec<(String, Words)> = Vec::new();
    for (name, s) in sample_screens() {
        all.push((format!("Screen::{name}"), s.words()));
    }
    for line in ALL_CHECK_LINES {
        all.push((
            format!("CheckLine::{}", check_line_name(line)),
            line.words(),
        ));
    }
    all.push(("Screen::tipChoose".to_string(), tip_choose_words("$12.50")));
    all.push(("Screen::stuck".to_string(), stuck_words()));
    all.push(("Screen::late".to_string(), late_words()));
    all.extend(crate::till_say::all_till_words());

    let w = write_fault;
    let open: Vec<(&str, OpenRefused<String>)> = vec![
        ("Removed", OpenRefused::Removed),
        ("RescanPending", OpenRefused::RescanPending),
        ("Unhealthy", OpenRefused::Unhealthy(Some(Problem::TipIsOld))),
        ("ZeroAmount", OpenRefused::ZeroAmount),
        ("AmountTooLarge", OpenRefused::AmountTooLarge),
        ("TipTooLarge", OpenRefused::TipTooLarge),
        ("BelowDust", OpenRefused::BelowDust { needs_sats: 600 }),
        (
            "BelowMinimum",
            OpenRefused::BelowMinimum {
                sats: 20_000,
                min_sats: 26_000,
            },
        ),
        (
            "StaleRate",
            OpenRefused::StaleRate {
                fetched_at: 1_700_000_000,
                now: 1_700_009_000,
            },
        ),
        ("RateLimited", OpenRefused::RateLimited),
        ("NoAddress", OpenRefused::NoAddress),
        ("AddressInUse", OpenRefused::AddressInUse),
        ("BadName", OpenRefused::BadName),
        ("BadNote", OpenRefused::BadNote),
        ("BadReference", OpenRefused::BadReference),
        ("AddressNotStored", OpenRefused::AddressNotStored(w())),
        ("Store", OpenRefused::Store(w())),
        ("Store/Record", OpenRefused::Store(record_fault())),
        (
            "Store/Read",
            OpenRefused::Store(StoreFault::Read("the disk did not answer".to_string())),
        ),
        (
            "Store/Damaged",
            OpenRefused::Store(StoreFault::Damaged {
                seq: 7,
                why: "it is not signed by this counter",
            }),
        ),
    ];
    for (name, r) in open {
        all.push((format!("OpenRefused::{name}"), r.words()));
    }
    let judge: Vec<(&str, JudgeError<String>)> = vec![
        ("UnknownSale", JudgeError::UnknownSale(3)),
        ("Store", JudgeError::Store(w())),
    ];
    for (name, r) in judge {
        all.push((format!("JudgeError::{name}"), r.words()));
    }
    let requote: Vec<(&str, RequoteRefused<String>)> = vec![
        ("UnknownSale", RequoteRefused::UnknownSale(3)),
        ("SaleEnded", RequoteRefused::SaleEnded),
        ("AlreadyEnough", RequoteRefused::AlreadyEnough),
        ("TooManyQuotes", RequoteRefused::TooManyQuotes),
        ("IsRequest", RequoteRefused::IsRequest),
        ("AmountTooLarge", RequoteRefused::AmountTooLarge),
        (
            "BelowMinimum",
            RequoteRefused::BelowMinimum {
                sats: 20_000,
                min_sats: 26_000,
            },
        ),
        (
            "StaleRate",
            RequoteRefused::StaleRate {
                fetched_at: 1_700_000_000,
                now: 1_700_009_000,
            },
        ),
        ("Store", RequoteRefused::Store(w())),
    ];
    for (name, r) in requote {
        all.push((format!("RequoteRefused::{name}"), r.words()));
    }
    let note: Vec<(&str, NoteRefused<String>)> = vec![
        ("UnknownSale", NoteRefused::UnknownSale(3)),
        // `NoteRefused::BadNote` says what `OpenRefused::BadNote` says.
        ("AlreadyNoted", NoteRefused::AlreadyNoted),
        ("Store", NoteRefused::Store(w())),
    ];
    for (name, r) in note {
        all.push((format!("NoteRefused::{name}"), r.words()));
    }
    let cancel: Vec<(&str, CancelRefused<String>)> = vec![
        ("UnknownSale", CancelRefused::UnknownSale(3)),
        ("AlreadyEnded", CancelRefused::AlreadyEnded),
        ("MoneyArrived", CancelRefused::MoneyArrived),
        ("BadName", CancelRefused::BadName),
        ("Store", CancelRefused::Store(w())),
    ];
    for (name, r) in cancel {
        all.push((format!("CancelRefused::{name}"), r.words()));
    }
    let lightning: Vec<(&str, LightningRefused<String>)> = vec![
        ("UnknownSale", LightningRefused::UnknownSale(3)),
        ("SaleClosed", LightningRefused::SaleClosed),
        ("PaidAnotherWay", LightningRefused::PaidAnotherWay),
        (
            "TooLittle",
            LightningRefused::TooLittle {
                sats: 900,
                least: 1_000,
            },
        ),
        ("Judge", LightningRefused::Judge(JudgeError::UnknownSale(3))),
        ("Store", LightningRefused::Store(w())),
    ];
    for (name, r) in lightning {
        all.push((format!("LightningRefused::{name}"), r.words()));
    }
    let mut overrides: Vec<(String, OverrideRefused<String>)> = vec![
        ("UnknownSale".to_string(), OverrideRefused::UnknownSale(3)),
        ("BadName".to_string(), OverrideRefused::BadName),
        (
            "OverSaleLimit".to_string(),
            OverrideRefused::OverSaleLimit {
                sats: 50_000,
                limit: 20_000,
            },
        ),
        (
            "OverDailyCap".to_string(),
            OverrideRefused::OverDailyCap {
                sats: 50_000,
                already: 100_000,
                cap: 120_000,
            },
        ),
        (
            "OverCeiling".to_string(),
            OverrideRefused::OverCeiling {
                sats: 500_000,
                already: 900_000,
                ceiling: 1_000_000,
            },
        ),
        (
            "TooManyOverrides".to_string(),
            OverrideRefused::TooManyOverrides,
        ),
        ("NoRoom".to_string(), OverrideRefused::NoRoom),
        (
            "Judge/UnknownSale".to_string(),
            OverrideRefused::Judge(JudgeError::UnknownSale(3)),
        ),
        (
            "Judge/Store".to_string(),
            OverrideRefused::Judge(JudgeError::Store(w())),
        ),
        ("Store".to_string(), OverrideRefused::Store(w())),
    ];
    for state in ALL_STATES {
        overrides.push((
            format!("WrongState/{}", state_name(state)),
            OverrideRefused::WrongState(state),
        ));
    }
    for (name, r) in overrides {
        all.push((format!("OverrideRefused::{name}"), r.words()));
    }
    for (name, e) in [
        ("WrongCounter", CommandError::WrongCounter),
        ("Stale", CommandError::Stale { last: 5, got: 3 }),
        ("OwnerSignatureInvalid", CommandError::OwnerSignatureInvalid),
    ] {
        all.push((
            format!("CommandRefused::Refused/{name}"),
            CommandRefused::<String>::Refused(e).words(),
        ));
    }
    all.push((
        "CommandRefused::Store".to_string(),
        CommandRefused::Store(w()).words(),
    ));
    for (name, e) in [
        ("WrongCounter", HeartbeatError::WrongCounter),
        ("Stale", HeartbeatError::Stale { last: 5, got: 3 }),
        (
            "OwnerSignatureInvalid",
            HeartbeatError::OwnerSignatureInvalid,
        ),
    ] {
        all.push((
            format!("HeartbeatRefused::Refused/{name}"),
            HeartbeatRefused::<String>::Refused(e).words(),
        ));
    }
    all.push((
        "HeartbeatRefused::FromTheFuture".to_string(),
        HeartbeatRefused::<String>::FromTheFuture { ahead_secs: 300 }.words(),
    ));
    all.push((
        "HeartbeatRefused::Store".to_string(),
        HeartbeatRefused::Store(w()).words(),
    ));
    for (name, v) in [
        (
            "FromTheFuture",
            NotVerified::FromTheFuture { ahead_secs: 300 },
        ),
        ("OwnerUnhealthy", NotVerified::OwnerUnhealthy),
        ("TipNotYetHeld", NotVerified::TipNotYetHeld),
        ("TipTooFarAhead", NotVerified::TipTooFarAhead),
        ("TipBehind", NotVerified::TipBehind { blocks: 5 }),
        ("TipDisagrees", NotVerified::TipDisagrees),
    ] {
        all.push((format!("NotVerified::{name}"), v.words()));
    }
    all.push((
        "HeartbeatView::Verified".to_string(),
        HeartbeatView::Verified {
            sent_at: 1_700_000_000,
        }
        .words(),
    ));
    all.push((
        "SightingError::Refused".to_string(),
        SightingError::<String>::Refused(openbmt_watch::Refusal::PaysNothing).words(),
    ));
    all.push((
        "SightingError::Store".to_string(),
        SightingError::Store(w()).words(),
    ));
    all.push((
        "BlockError::Refused".to_string(),
        BlockError::<String>::Refused(openbmt_watch::Refusal::BlockMalformed).words(),
    ));
    all.push((
        "BlockError::Store".to_string(),
        BlockError::Store(w()).words(),
    ));
    use openbmt_core::wire::DecodeError;
    for (name, e) in [
        ("Batches", LoadError::Batches(DecodeError::Truncated)),
        ("Exposure", LoadError::Exposure(DecodeError::Truncated)),
        ("Pause", LoadError::Pause(DecodeError::Truncated)),
        ("Desk", LoadError::Desk(DecodeError::Truncated)),
        ("Bills", LoadError::Bills(DecodeError::Truncated)),
        (
            "Records",
            LoadError::Records(openbmt_core::records::ChainError::NotACheckIn),
        ),
        (
            "RecordsUnread",
            LoadError::RecordsUnread("the disk did not answer".to_string()),
        ),
    ] {
        all.push((format!("LoadError::{name}"), e.words()));
    }
    for (name, e) in [
        ("TooLong", DestinationRefused::TooLong),
        ("NotPrintable", DestinationRefused::NotPrintable),
        ("NotAnAddress", DestinationRefused::NotAnAddress),
        ("WrongNetwork", DestinationRefused::WrongNetwork),
        ("ScriptKind", DestinationRefused::ScriptKind),
    ] {
        all.push((format!("DestinationRefused::{name}"), e.words()));
    }
    all.push((
        "BatchTakeRefused::OwnWallet".to_string(),
        BatchTakeRefused::<String>::own_wallet_words(),
    ));
    for (name, e) in [
        (
            "Range",
            RangeTakeRefused::<String>::Range(openbmt_core::pairing_own::OwnPairError::Range(
                openbmt_core::pinned_wallet::RangeError::OverlapsUsed,
            )),
        ),
        ("KeyApp", RangeTakeRefused::KeyApp),
        ("Store", RangeTakeRefused::Store("disk full".to_string())),
    ] {
        all.push((format!("RangeTakeRefused::{name}"), e.words()));
    }
    let refund: Vec<(&str, RefundRefused<String>)> = vec![
        ("BadText", RefundRefused::BadText("name")),
        ("DestinationKind", RefundRefused::DestinationKind),
        ("UnknownSale", RefundRefused::UnknownSale(3)),
        (
            "StaleRate",
            RefundRefused::StaleRate {
                fetched_at: 1_700_000_000,
                now: 1_700_009_000,
            },
        ),
        ("ZeroAmount", RefundRefused::ZeroAmount),
        ("AmountTooLarge", RefundRefused::AmountTooLarge),
        (
            "BelowDust",
            RefundRefused::BelowDust {
                sats: 100,
                dust: 546,
            },
        ),
        ("OwnScript", RefundRefused::OwnScript),
        ("NothingReceived", RefundRefused::NothingReceived),
        ("TooManyRefunds", RefundRefused::TooManyRefunds),
        (
            "MoreThanReceived",
            RefundRefused::MoreThanReceived {
                received: 10_000,
                asked: 6_000,
                sats: 5_000,
            },
        ),
        (
            "WithdrawnMayBePaid",
            RefundRefused::WithdrawnMayBePaid {
                received: 10_000,
                asked: 6_000,
                sats: 5_000,
            },
        ),
        (
            "LightningCashOnly",
            RefundRefused::LightningCashOnly {
                room: 0,
                sats: 5_000,
            },
        ),
        (
            "NotSettled",
            RefundRefused::NotSettled {
                received: 5_000,
                settled: 0,
                sats: 5_000,
            },
        ),
        (
            "NeedsManagerSingle",
            RefundRefused::NeedsManagerSingle {
                sats: 50_000,
                limit: 20_000,
            },
        ),
        (
            "NeedsManagerDaily",
            RefundRefused::NeedsManagerDaily {
                sats: 50_000,
                already: 100_000,
                cap: 120_000,
            },
        ),
        ("Store", RefundRefused::Store(w())),
    ];
    for (name, r) in refund {
        all.push((format!("RefundRefused::{name}"), r.words()));
    }
    let withdraw: Vec<(&str, WithdrawRefused<String>)> = vec![
        ("BadName", WithdrawRefused::BadName),
        ("UnknownSale", WithdrawRefused::UnknownSale(3)),
        ("UnknownRequest", WithdrawRefused::UnknownRequest(2)),
        ("AlreadyWithdrawn", WithdrawRefused::AlreadyWithdrawn(2)),
        ("Store", WithdrawRefused::Store(w())),
    ];
    for (name, r) in withdraw {
        all.push((format!("WithdrawRefused::{name}"), r.words()));
    }
    let staff: Vec<(&str, StaffRefused<String>)> = vec![
        ("BadName", StaffRefused::BadName("cashier's name")),
        ("Store", StaffRefused::Store(w())),
    ];
    for (name, r) in staff {
        all.push((format!("StaffRefused::{name}"), r.words()));
    }
    let totals: Vec<(&str, TotalsRefused<String>)> = vec![
        ("BadWindow", TotalsRefused::BadWindow),
        ("MixedCurrencies", TotalsRefused::MixedCurrencies),
        ("Overflow", TotalsRefused::Overflow),
        ("TooManyRecords", TotalsRefused::TooManyRecords),
        ("Store", TotalsRefused::Store(w())),
    ];
    for (name, r) in totals {
        all.push((format!("TotalsRefused::{name}"), r.words()));
    }
    all.extend(all_bill_words());
    for state in ALL_REFUND_STATES {
        all.push((
            format!("RefundState::{}", refund_state_name(state)),
            state.words(),
        ));
        all.push((
            format!("RefundView::{}", refund_state_name(state)),
            sample_refund_view(state).words(),
        ));
    }
    all
}

/// Every state, for the samples and for the test of SC-01.
pub const ALL_STATES: [State; 12] = [
    State::NotPaidYet,
    State::PartPaid,
    State::Waiting,
    State::PaidInstant,
    State::Confirmed,
    State::Final,
    State::Overridden,
    State::PaidLightning,
    State::DoNotHandOver,
    State::Expired,
    State::Cancelled,
    State::NeedsAPerson,
];

/// Every refund state.
pub const ALL_REFUND_STATES: [RefundState; 4] = [
    RefundState::Requested,
    RefundState::WaitingForDepth,
    RefundState::Withdrawn,
    RefundState::PaidInCash,
];

fn sample_refund_view(state: RefundState) -> RefundView {
    RefundView {
        sale: 3,
        number: match state {
            RefundState::PaidInCash => None,
            RefundState::Requested | RefundState::WaitingForDepth | RefundState::Withdrawn => {
                Some(1)
            }
        },
        asked: 5_000,
        value_minor: None,
        tip_minor: 0,
        state,
        by: crate::counter::Actor {
            role: Role::Cashier,
            name: "Ann \u{202e}Lee".to_string(),
        },
        at: 1_700_000_000,
    }
}
