//! Words shown by the owner's device program (SC-01 to SC-12).
//!
//! System text and peer notes belong in the small print only. The sentences
//! here describe the known state and give the owner a next step.

use openbmt_core::say::{Tone, Who, Words};
use openbmt_watch::say::RefusalGroup;
use openbmt_watch::Problem;

use crate::coins::{ApplyError, PendingError, ReceiptState, ViewState};
use crate::facts::FactsRefused;
use crate::live::DeepLine;
use crate::node::{NodeError, SendRefused, SendState};
use crate::pos::PosRefused;
use crate::receipts::{MergeOffer, MergeRefused, ReceiptsLine};
use crate::refunds::{NoteKind, RefundNote};
use crate::watcher::OwnerStartError;

/// An alert the owner can receive from the watcher.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Alert {
    /// The watcher has not yet received peer information.
    NoHealthAnswer,
    /// The watcher stopped.
    Stopped,
    /// The chain view cannot continue after a storage or block failure.
    ChainHalted,
    /// A deep reorganization requires a full rescan.
    DeepReorg,
    /// Peer health found too few network groups.
    TooFewPeerGroups,
    /// Peer health found too few groups serving filters.
    TooFewFilterPeers,
    /// The chain tip is old.
    TipIsOld,
    /// Peers are consistently ahead of the local chain.
    HeadersBehind,
    /// The device clock is behind the chain tip.
    ClockMismatch,
}

impl Alert {
    /// The owner-facing words for an alert.
    pub fn words(self) -> Words {
        match self {
            Self::NoHealthAnswer => Words::new(Tone::Info, "Checking the network", "The watcher has not heard enough from its peers to check the chain yet.", Who::Nobody, "Nothing to do. The watcher checks again when peers report in."),
            Self::Stopped => Words::new(Tone::Stop, "Watcher stopped", "The watcher has stopped following the chain.", Who::Owner, "Restart the owner app. If it stops again, tell the OpenBMT project."),
            Self::ChainHalted => Words::new(Tone::Stop, "Chain check stopped", "The coin list was left at its last saved position because a block or file could not be used.", Who::Owner, "Restart the owner app. If this returns, tell the OpenBMT project."),
            Self::DeepReorg => Words::new(Tone::Stop, "Deep chain change", "The chain changed farther back than the saved coin list can follow.", Who::Owner, "Start a fresh check from the wallet's first block."),
            Self::TooFewPeerGroups => Words::new(Tone::Info, "Too few network peers", "The watcher is connected to peers from too few parts of the network.", Who::Nobody, "Nothing to do. The watcher checks again as peers change.").code("WR-50"),
            Self::TooFewFilterPeers => Words::new(Tone::Info, "Too few payment peers", "Too few parts of the network have peers that can find wallet payments.", Who::Nobody, "Nothing to do. The watcher checks again as peers change.").code("WR-50"),
            Self::TipIsOld => Words::new(Tone::Info, "Chain is behind", "The newest block the watcher knows about is old.", Who::Nobody, "Nothing to do. The watcher checks again when a newer block arrives.").code("WR-51"),
            Self::HeadersBehind => Words::new(Tone::Info, "Peers are ahead", "Peers have reported blocks ahead of the watcher's chain for too long.", Who::Nobody, "Nothing to do. The watcher checks again as the chain catches up.").code("WR-52"),
            Self::ClockMismatch => Words::new(Tone::Stop, "Clock needs attention", "The device clock is behind the newest block the watcher knows about.", Who::Owner, "Set the device clock to the correct time, then restart the owner app.").code("WR-53"),
        }
    }
}

impl From<Problem> for Alert {
    fn from(problem: Problem) -> Self {
        match problem {
            Problem::TooFewPeerGroups => Self::TooFewPeerGroups,
            Problem::TooFewFilterPeers => Self::TooFewFilterPeers,
            Problem::TipIsOld => Self::TipIsOld,
            Problem::HeadersBehind => Self::HeadersBehind,
            Problem::ClockMismatch => Self::ClockMismatch,
        }
    }
}

impl ViewState {
    /// The words for coin readiness.
    pub fn words(self) -> Words {
        match self {
            Self::Ready => Words::new(
                Tone::Good,
                "Saved coins are ready",
                "The saved coin list is ready for the next chain check.",
                Who::Nobody,
                "Nothing to do. New blocks are checked as they arrive.",
            ),
            Self::Rescanning { .. } => Words::new(
                Tone::Info,
                "Checking older blocks",
                "The wallet is still checking blocks from before its current position.",
                Who::Nobody,
                "Nothing to do. The wallet is ready when the earlier check is complete.",
            ),
            Self::DeepReorg => Words::new(
                Tone::Stop,
                "Wallet needs a fresh check",
                "The chain changed farther back than the saved coin list can follow.",
                Who::Owner,
                "Start a fresh check from the wallet's first block.",
            ),
        }
    }
}

impl ReceiptState {
    /// OC-13: the words for the state of the record of payments received.
    pub fn words(self) -> Words {
        match self {
            Self::Complete => Words::new(
                Tone::Good,
                "Payments are recorded",
                "The wallet keeps a record of every payment it has seen for its addresses.",
                Who::Nobody,
                "Nothing to do. New payments are recorded as blocks arrive.",
            )
            .code("OC-13"),
            Self::LogFull => Words::new(
                Tone::Stop,
                "Payment record is full",
                "The wallet cannot keep more records of payments, so it cannot say what a sale received.",
                Who::Owner,
                "Report this limit to the OpenBMT project. You can still spend from the wallet.",
            )
            .code("OC-13"),
        }
    }
}

impl FactsRefused {
    /// OC-14: the words for a sale whose amount received cannot be given.
    pub fn words(self) -> Words {
        match self {
            Self::ViewNotReady => Words::new(
                Tone::Info,
                "Wallet catching up",
                "Waiting for the wallet to catch up with the chain before this refund request is checked.",
                Who::Nobody,
                "Nothing to do. The request is checked again by itself.",
            )
            .code("OC-14"),
            Self::ReceiptLogFull => ReceiptState::LogFull.words(),
            Self::TooOldForFacts => Words::new(
                Tone::Stop,
                "Sale too old to check",
                "This sale is too old for the wallet to check what was paid.",
                Who::Owner,
                "Decide this refund by hand.",
            )
            .code("OC-29"),
            Self::NotThisCountersAddress => Words::new(
                Tone::Stop,
                "Address not given out",
                "The counter asked for a refund on an address this wallet never gave to it.",
                Who::Owner,
                "Do not refund this sale. Check this counter.",
            )
            .code("OC-14"),
        }
    }
}

impl NoteKind {
    /// The words for a line of the refund requests' list.
    pub fn words(self) -> Words {
        match self {
            Self::Refused => Words::new(
                Tone::Stop,
                "Refund request refused",
                "The queue did not accept a refund request from a counter.",
                Who::Owner,
                "Read the reason in the details, then dismiss the line.",
            ),
            Self::WithdrawalNotApplied => Words::new(
                Tone::Info,
                "Withdrawal not applied",
                "A counter withdrew a refund request, but the refund is already past the point where it can be removed.",
                Who::Owner,
                "Check the refund and decide whether to go on with it.",
            ),
            Self::Interrupted => Words::new(
                Tone::Info,
                "Request may be half done",
                "The app stopped while it was taking in a refund request, so the result is not known.",
                Who::Owner,
                "Check the queue for this request, then dismiss the line.",
            ),
            Self::ClaimedMoreThanSeen => Words::new(
                Tone::Stop,
                "Counter claims too much",
                "The counter says this sale received more than the wallet's own checks show, so the smaller amount was used.",
                Who::Owner,
                "Check this counter before approving any refund for the sale.",
            )
            .code("OC-15"),
        }
    }
}

impl RefundNote {
    /// The words for this line, with its own text as the details.
    pub fn words(&self) -> Words {
        self.kind
            .words()
            .detail(openbmt_core::say::clean_foreign(&self.text))
    }
}

impl SendState {
    /// The words for a finished payment's sending state.
    pub fn words(self) -> Words {
        match self {
            Self::Waiting => Words::new(Tone::Info, "Payment waiting to send", "The finished payment is held on this device and has not been sent.", Who::Owner, "Set up a proxy for sending, or turn on sending without Tor."),
            Self::Sent => Words::new(Tone::Info, "Payment sent", "The payment was announced to peers and has not yet been seen back.", Who::Nobody, "Nothing to do. Wait for peers to report it or for it to appear in a block."),
            Self::SeenBack { .. } => Words::new(Tone::Info, "Payment seen by peers", "Peers outside the ones asked to send it have reported this payment.", Who::Nobody, "Nothing to do. Wait for it to appear in a block."),
            Self::NotSeen => Words::new(Tone::Info, "Payment sent again", "Peers did not report the payment back, so it was announced again.", Who::Nobody, "Nothing to do. Wait for peers to report it or for it to appear in a block."),
            Self::Confirmed { .. } => Words::new(Tone::Info, "Payment in the light check", "The payment is in a block found by the owner's light check, but it has not reached the chosen depth.", Who::Nobody, "Nothing to do. Wait for more blocks to arrive."),
            Self::Done => Words::new(Tone::Good, "Payment depth reached", "The payment reached the chosen number of blocks in the owner's light check.", Who::Nobody, "Nothing to do."),
            Self::Replaced => Words::new(Tone::Ended, "Payment replaced", "Another payment spent the same coins, so this payment will not complete.", Who::Owner, "Review the returned entries and decide whether to sign them again."),
            Self::Unresolved => Words::new(Tone::Stop, "Payment needs a fresh check", "The saved view cannot tell whether this payment completed or was replaced.", Who::Owner, "Start a fresh check from the wallet's first block."),
        }
    }
}

/// A count with separators: "1,200" (SC-09).
fn grouped(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// "1 separate payment", "12 separate payments".
fn separate_payments(n: usize) -> String {
    if n == 1 {
        "1 separate payment".to_string()
    } else {
        format!("{} separate payments", grouped(n))
    }
}

impl ReceiptsLine {
    /// OC-18: the words of the line about receipts. The shop's currency is
    /// not at hand on this device, so the cost is in sats (SC-09).
    pub fn words(self) -> Words {
        match self {
            Self::NotReady => Words::new(
                Tone::Info,
                "Payments not counted yet",
                "The wallet is still catching up with the chain, so its payments cannot be counted yet.",
                Who::Nobody,
                "Nothing to do. The count appears once the wallet has caught up.",
            )
            .code("OC-18"),
            Self::NoCoins => Words::new(
                Tone::Info,
                "No payments to spend",
                "The wallet holds no payments at the moment.",
                Who::Nobody,
                "Nothing to do. This line shows the cost again after a payment arrives.",
            )
            .code("OC-18"),
            Self::FeeUnknown { payments } => Words::new(
                Tone::Info,
                "Spending cost not known yet",
                format!(
                    "The wallet holds {}. The fee of recent blocks is not known yet, so no cost is shown.",
                    separate_payments(payments)
                ),
                Who::Nobody,
                "Nothing to do. The cost shows once the owner app has looked at enough recent blocks.",
            )
            .code("OC-18"),
            Self::Known {
                payments,
                cost_sats,
                ..
            } => Words::new(
                Tone::Info,
                "Cost of spending payments",
                format!(
                    "The wallet holds {}. Spending all of them now would cost about {}.",
                    separate_payments(payments),
                    openbmt_core::say::sats(cost_sats)
                ),
                Who::Nobody,
                "Nothing to do. This is a guide, and the fee is fixed when a payment is made.",
            )
            .code("OC-18"),
        }
    }
}

/// "1 old address is", "12 old addresses are".
fn old_addresses(n: u64) -> String {
    let count = usize::try_from(n).unwrap_or(usize::MAX);
    if count == 1 {
        "1 old address".to_string()
    } else {
        format!("{} old addresses", grouped(count))
    }
}

impl DeepLine {
    /// OC-25: the words of the line about old addresses that are checked
    /// once a month. The time of the next check goes in `at`; the last one is
    /// said in whole days, never in seconds (SC-03).
    pub fn words(self) -> Words {
        match self {
            Self::FirstCheckWaiting { count, next } => Words::new(
                Tone::Info,
                "Old addresses wait for check",
                format!(
                    "{} checked once a month, not all the time, so a payment to one shows up at the next check.",
                    old_addresses_are(count)
                ),
                Who::Nobody,
                "Nothing to do. No check has run yet; the first is at the time shown.",
            )
            .code("OC-25")
            .at(next),
            Self::Monthly { count, days_ago, next } => Words::new(
                Tone::Info,
                "Old addresses: monthly check",
                format!(
                    "{} checked once a month, not all the time, so a payment to one shows up at the next check.",
                    old_addresses_are(count)
                ),
                Who::Nobody,
                format!(
                    "Nothing to do. The last check was {}; the next is at the time shown.",
                    days_text(days_ago)
                ),
            )
            .code("OC-25")
            .at(next),
            Self::Checking { count } => Words::new(
                Tone::Info,
                "Checking old addresses",
                format!(
                    "The wallet is checking {} for payments that arrived since it last looked.",
                    old_addresses(count)
                ),
                Who::Nobody,
                "Nothing to do. A payment that is found is added to the wallet.",
            )
            .code("OC-26"),
            Self::Failed { count, next } => Words::new(
                Tone::Info,
                "Old address check failed",
                format!(
                    "The last check of {} did not finish, so a payment to one may not show yet.",
                    old_addresses(count)
                ),
                Who::Nobody,
                "Nothing to do. The wallet tries again at the time shown.",
            )
            .code("OC-26")
            .at(next),
        }
    }
}

/// "1 old address is", "12 old addresses are", as the start of a sentence.
fn old_addresses_are(n: u64) -> String {
    let what = old_addresses(n);
    if n == 1 {
        format!("{what} is")
    } else {
        format!("{what} are")
    }
}

/// "today", "1 day ago", "12 days ago".
fn days_text(days: u64) -> String {
    match days {
        0 => "today".to_string(),
        1 => "1 day ago".to_string(),
        n => format!("{n} days ago"),
    }
}

impl MergeOffer {
    /// OC-19: the words of the offer to merge. They give the cost now and
    /// at the usual fee, and say that a merge shows on the chain that the
    /// payments belong to one wallet.
    pub fn words(&self) -> Words {
        let which = if self.coins == self.of {
            format!("your {} payments", grouped(self.coins))
        } else {
            format!(
                "{} of your {} payments",
                grouped(self.coins),
                grouped(self.of)
            )
        };
        Words::new(
            Tone::Info,
            "Merge payments now?",
            format!(
                "Merging {which} into one costs about {} now, against about {} at the usual fee.",
                openbmt_core::say::sats(self.cost_sats),
                openbmt_core::say::sats(self.usual_cost_sats)
            ),
            Who::Owner,
            "Merging shows on the blockchain that these payments belong to one wallet. Approve it only if that is fine.",
        )
        .code("OC-19")
    }
}

impl MergeRefused {
    /// OC-19, OC-20: the words for a merge that was not built.
    pub fn words(&self) -> Words {
        match self {
            Self::NoOffer => Words::new(
                Tone::Ended,
                "No merge to offer",
                "Merging is not on offer now, so nothing was built.",
                Who::Nobody,
                "Nothing to do. The offer shows again when fees are low and enough payments wait.",
            )
            .code("OC-19"),
            Self::OtherPaymentsWaiting => Words::new(
                Tone::Info,
                "Other payments are waiting",
                "A refund or a cash-out is waiting for the Key app, and a merge would take the same payments.",
                Who::Owner,
                "Finish or cancel the waiting payments, then approve the merge again.",
            )
            .code("OC-20"),
            Self::NotBuilt(why) => Words::new(
                Tone::Stop,
                "Merge not built",
                "The merge could not be put in a session for the Key app, and the queue is as it was.",
                Who::Owner,
                "Try again later. If this returns, tell the OpenBMT project and include the Details text.",
            )
            .code("OC-20")
            .detail(openbmt_core::say::clean_foreign(why)),
            Self::Node(error) => error.words(),
        }
    }
}

impl SendRefused {
    /// The words for a refusal to send a payment.
    pub fn words(&self) -> Words {
        match self {
            Self::Malformed => Words::new(
                Tone::Stop,
                "Payment could not be sent",
                "The finished payment could not be read as one payment.",
                Who::Owner,
                "Nothing was sent. Tell the OpenBMT project and include the details.",
            ),
            Self::NoTor => Words::new(
                Tone::Info,
                "Choose how to send",
                "The finished payment is held because sending is not set up.",
                Who::Owner,
                "Set up a proxy for sending, or turn on sending without Tor.",
            ),
        }
    }
}

impl OwnerStartError {
    /// The owner-facing words for a watcher start failure.
    pub fn words(&self) -> Words {
        match self {
            Self::Locked(detail) => Words::new(
                Tone::Stop,
                "Owner app already open",
                "Another owner app is using this data folder.",
                Who::Owner,
                "Close the other owner app, then try again.",
            )
            .detail(openbmt_core::say::clean_foreign(detail)),
            Self::Dir(detail) => {
                fault("Owner folder unavailable").detail(openbmt_core::say::clean_foreign(detail))
            }
            Self::Headers(detail) => {
                fault("Saved chain unavailable").detail(openbmt_core::say::clean_foreign(detail))
            }
            Self::Driver(detail) => {
                fault("Watcher could not start").detail(openbmt_core::say::clean_foreign(detail))
            }
        }
    }
}

impl NodeError {
    /// The owner-facing words for a node refusal or failure.
    pub fn words(&self) -> Words {
        match self {
            Self::Start(error) => error.words(),
            Self::Files(detail) => fault("Owner data not saved").detail(openbmt_core::say::clean_foreign(detail)),
            Self::Config(detail) => Words::new(Tone::Stop, "Check owner settings", "The owner app cannot use one of its settings.", Who::Owner, "Review the owner settings and try again.").detail(openbmt_core::say::clean_foreign(detail)),
            Self::NotReady(detail) => Words::new(Tone::Info, "Wallet is not ready", "The wallet cannot build a payment from its current coin list.", Who::Owner, "Check the wallet status for the next step before trying again.").detail(openbmt_core::say::clean_foreign(detail)),
            Self::Owner(error) => error.words(),
            Self::KeyRecoveryRequired => Words::new(Tone::Stop, "Approval needs recovery", "The owner app saved an approval but has not finished restoring what it needs to approve payments.", Who::Owner, "Do not approve another payment until recovery is complete; report this issue to the OpenBMT project."),
            Self::WatchLimit { .. } => Words::new(Tone::Stop, "Address watch limit reached", "This device needs to watch more wallet addresses than it can follow.", Who::Owner, "Stop issuing addresses and update this device or report the limit to the OpenBMT project."),
        }
    }
}

/// Owner-device wording for every grouped refusal from the shared block
/// checker. These are owner coin-check screens, not counter payment screens.
fn owner_refusal_words(group: RefusalGroup) -> Words {
    match group {
        RefusalGroup::LeftTheChain => Words::new(
            Tone::Info,
            "Block left the chain",
            "A block that held a payment is no longer in the chain this device follows.",
            Who::Nobody,
            "Nothing to do. The owner app checks blocks from the chain it follows.",
        )
        .code("WR-01"),
        RefusalGroup::HeaderNotBelieved => Words::new(
            Tone::Ended,
            "Block header not accepted",
            "A block header did not pass the owner's chain checks, so it was left out.",
            Who::Nobody,
            "Nothing to do. The owner app keeps checking the chain.",
        )
        .code("WR-05"),
        RefusalGroup::HeaderFromTheFuture => Words::new(
            Tone::Info,
            "Block time is ahead",
            "A block's time is too far ahead of this device's clock, so it was left out.",
            Who::Owner,
            "Check this device's clock. The owner app tries the block again later.",
        )
        .code("WR-06"),
        RefusalGroup::SavedHeadersUnusable => Words::new(
            Tone::Stop,
            "Saved chain needs repair",
            "The saved chain data could not be used, so the owner app cannot continue from it.",
            Who::Owner,
            "Restart the owner app. If the problem returns, report it to the OpenBMT project.",
        )
        .code("WR-08"),
        RefusalGroup::HeaderStoreFull => Words::new(
            Tone::Stop,
            "Saved block list is full",
            "The owner app cannot save more blocks, so it stopped checking new ones.",
            Who::Owner,
            "Stop relying on new wallet checks and report this limit to the OpenBMT project.",
        )
        .code("WR-08"),
        RefusalGroup::ProofNotBelieved => Words::new(
            Tone::Ended,
            "Block proof not accepted",
            "A block proof did not pass the checks, so the owner app did not use it.",
            Who::Nobody,
            "Nothing to do. The owner app checks later blocks as they arrive.",
        )
        .code("WR-10"),
        RefusalGroup::BlockNotBelieved => Words::new(
            Tone::Ended,
            "Block not accepted",
            "A block did not pass the checks, so the saved coin list was left unchanged.",
            Who::Nobody,
            "Nothing to do. The owner app checks the next block.",
        )
        .code("WR-20"),
        RefusalGroup::PaysNothing => Words::new(
            Tone::Ended,
            "Payment not recognized",
            "The reported payment did not match the wallet's payment checks, so it was left out.",
            Who::Nobody,
            "Nothing to do. The owner app keeps checking the chain.",
        )
        .code("WR-30"),
        RefusalGroup::NeedsAPerson => Words::new(
            Tone::Stop,
            "Wallet check needs review",
            "The owner app could not finish checking the reported payment data.",
            Who::Owner,
            "Pause wallet actions and report this check to the OpenBMT project.",
        )
        .code("WR-34"),
        RefusalGroup::NotATransaction => Words::new(
            Tone::Ended,
            "Payment data not accepted",
            "The data offered for a payment could not be read as a payment, so it was ignored.",
            Who::Nobody,
            "Nothing to do. The owner app keeps checking the chain.",
        )
        .code("WR-40"),
    }
}

impl<E: std::fmt::Display> ApplyError<E> {
    /// The owner-facing words for a block that could not update the wallet.
    pub fn words(&self) -> Words {
        match self {
            Self::Refused(refusal) => owner_refusal_words(refusal.group()),
            Self::Full => Words::new(
                Tone::Stop,
                "Wallet needs attention",
                "The wallet has reached the limit for saved coins, so this block was not applied.",
                Who::Owner,
                "Tell the OpenBMT project and include the details.",
            ),
            Self::Store(detail) => Words::new(
                Tone::Stop,
                "Wallet was not updated",
                "The wallet could not save its new coin list, so this block was not applied.",
                Who::Owner,
                "Try again. If this returns, tell the OpenBMT project and include the details.",
            )
            .detail(openbmt_core::say::clean_foreign(&detail.to_string())),
        }
    }
}

impl<E: std::fmt::Display> PendingError<E> {
    /// The owner-facing words for change that could not be held.
    pub fn words(&self) -> Words {
        match self {
            Self::Full => Words::new(
                Tone::Stop,
                "Payment could not be held",
                "The wallet already holds the most unfinished payments it can keep.",
                Who::Owner,
                "Wait for an unfinished payment to settle, then try again.",
            ),
            Self::Store(detail) => Words::new(
                Tone::Stop,
                "Payment was not held",
                "The wallet could not save the unfinished payment, so it was not held.",
                Who::Owner,
                "Try again. If this returns, tell the OpenBMT project and include the details.",
            )
            .detail(openbmt_core::say::clean_foreign(&detail.to_string())),
        }
    }
}

fn fault(heading: &'static str) -> Words {
    Words::new(
        Tone::Stop,
        heading,
        "The owner app could not confirm the saved state because it could not use its data folder.",
        Who::Owner,
        "Try again. If this returns, tell the OpenBMT project and include the Details text.",
    )
}

/// SC-12: a sample of every externally visible owner-node status and refusal.
pub fn all_words() -> Vec<(String, Words)> {
    let mut all = Vec::new();
    let mut add = |name: &str, words: Words| all.push((name.to_string(), words));
    for (name, state) in [
        ("Ready", ViewState::Ready),
        ("Rescanning", ViewState::Rescanning { to: 20 }),
        ("DeepReorg", ViewState::DeepReorg),
    ] {
        add(&format!("ViewState::{name}"), state.words());
    }
    for (name, state) in [
        ("Waiting", SendState::Waiting),
        ("Sent", SendState::Sent),
        ("SeenBack", SendState::SeenBack { groups: 2 }),
        ("NotSeen", SendState::NotSeen),
        ("Confirmed", SendState::Confirmed { depth: 2 }),
        ("Done", SendState::Done),
        ("Replaced", SendState::Replaced),
        ("Unresolved", SendState::Unresolved),
    ] {
        add(&format!("SendState::{name}"), state.words());
    }
    for (name, state) in [
        ("Complete", ReceiptState::Complete),
        ("LogFull", ReceiptState::LogFull),
    ] {
        add(&format!("ReceiptState::{name}"), state.words());
    }
    for (name, why) in [
        ("ViewNotReady", FactsRefused::ViewNotReady),
        ("TooOldForFacts", FactsRefused::TooOldForFacts),
        (
            "NotThisCountersAddress",
            FactsRefused::NotThisCountersAddress,
        ),
    ] {
        add(&format!("FactsRefused::{name}"), why.words());
    }
    for (name, kind) in [
        ("Refused", NoteKind::Refused),
        ("WithdrawalNotApplied", NoteKind::WithdrawalNotApplied),
        ("Interrupted", NoteKind::Interrupted),
        ("ClaimedMoreThanSeen", NoteKind::ClaimedMoreThanSeen),
    ] {
        add(&format!("NoteKind::{name}"), kind.words());
    }
    let offer = MergeOffer {
        coins: 12,
        of: 12,
        rate_per_kvb: 1_000,
        cost_sats: 1_600,
        usual_cost_sats: 4_000,
    };
    for (name, line) in [
        ("NotReady", ReceiptsLine::NotReady),
        ("NoCoins", ReceiptsLine::NoCoins),
        ("FeeUnknown", ReceiptsLine::FeeUnknown { payments: 12 }),
        (
            "Known",
            ReceiptsLine::Known {
                payments: 12,
                cost_sats: 4_000,
                rate_per_kvb: 2_500,
            },
        ),
    ] {
        add(&format!("ReceiptsLine::{name}"), line.words());
    }
    add("MergeOffer::All", offer.words());
    add(
        "MergeOffer::Part",
        MergeOffer {
            coins: 400,
            of: 1_000,
            ..offer
        }
        .words(),
    );
    for (name, why) in [
        ("NoOffer", MergeRefused::NoOffer),
        ("OtherPaymentsWaiting", MergeRefused::OtherPaymentsWaiting),
        ("NotBuilt", MergeRefused::NotBuilt(String::new())),
    ] {
        add(&format!("MergeRefused::{name}"), why.words());
    }
    for (name, line) in [
        (
            "FirstCheckWaiting",
            DeepLine::FirstCheckWaiting {
                count: 1_200,
                next: 1_700_000_000,
            },
        ),
        (
            "Monthly",
            DeepLine::Monthly {
                count: 1_200,
                days_ago: 12,
                next: 1_700_000_000,
            },
        ),
        ("Checking", DeepLine::Checking { count: 1_200 }),
        (
            "Failed",
            DeepLine::Failed {
                count: 1_200,
                next: 1_700_000_000,
            },
        ),
    ] {
        add(&format!("DeepLine::{name}"), line.words());
    }
    add("SendRefused::Malformed", SendRefused::Malformed.words());
    add("SendRefused::NoTor", SendRefused::NoTor.words());
    for (name, error) in [
        ("Locked", OwnerStartError::Locked(String::new())),
        ("Dir", OwnerStartError::Dir(String::new())),
        ("Headers", OwnerStartError::Headers(String::new())),
        ("Driver", OwnerStartError::Driver(String::new())),
    ] {
        add(&format!("OwnerStartError::{name}"), error.words());
    }
    for (name, error) in [
        ("Files", NodeError::Files(String::new())),
        ("Config", NodeError::Config(String::new())),
        ("NotReady", NodeError::NotReady(String::new())),
        (
            "Owner",
            NodeError::Owner(openbmt_owner::OwnerError::Store(String::new())),
        ),
        ("KeyRecoveryRequired", NodeError::KeyRecoveryRequired),
        (
            "WatchLimit",
            NodeError::WatchLimit {
                needed: 1,
                limit: 1,
            },
        ),
    ] {
        add(&format!("NodeError::{name}"), error.words());
    }
    for (name, error) in [
        ("Full", ApplyError::<String>::Full),
        ("Store", ApplyError::<String>::Store(String::new())),
    ] {
        add(&format!("ApplyError::{name}"), error.words());
    }
    for group in RefusalGroup::ALL {
        add(
            &format!("BlockRefusal::{}", group.name()),
            owner_refusal_words(group),
        );
    }
    for (name, error) in [
        ("Full", PendingError::<String>::Full),
        ("Store", PendingError::<String>::Store(String::new())),
    ] {
        add(&format!("PendingError::{name}"), error.words());
    }
    // The refusals that wrap another crate's refusal have its words and are
    // not listed again; the ones that are the owner node's own are.
    for (name, error) in [
        ("NotSetUp", PosRefused::NotSetUp),
        ("NoCounters", PosRefused::NoCounters),
        ("TooSoon", PosRefused::TooSoon { wait_secs: 30 }),
        ("NothingAsked", PosRefused::NothingAsked),
        ("TooManyPayments", PosRefused::TooManyPayments),
        ("SignInRefused", PosRefused::SignInRefused { status: 401 }),
        ("RateLimited", PosRefused::RateLimited),
        ("Unavailable", PosRefused::Unavailable { status: 500 }),
        ("Store", PosRefused::Store(String::new())),
        (
            "Corrupt",
            PosRefused::Corrupt {
                file: crate::pos::POS_LEDGER_FILE,
                why: String::new(),
            },
        ),
    ] {
        add(&format!("PosRefused::{name}"), error.words());
    }
    for (name, alert) in [
        ("NoHealthAnswer", Alert::NoHealthAnswer),
        ("Stopped", Alert::Stopped),
        ("ChainHalted", Alert::ChainHalted),
        ("DeepReorg", Alert::DeepReorg),
        ("TooFewPeerGroups", Alert::TooFewPeerGroups),
        ("TooFewFilterPeers", Alert::TooFewFilterPeers),
        ("TipIsOld", Alert::TipIsOld),
        ("HeadersBehind", Alert::HeadersBehind),
        ("ClockMismatch", Alert::ClockMismatch),
    ] {
        add(&format!("Alert::{name}"), alert.words());
    }
    all
}

/// Rows for `docs/screens/owner-node.md`, in [`all_words`] order.
pub fn screens_markdown() -> String {
    let cell = |text: &str| text.replace('|', "/");
    let mut out = String::new();
    for (name, words) in all_words() {
        out.push_str(&format!(
            "| {name} | {} | {} | {} | {:?} | {} |\n",
            cell(&words.heading),
            cell(&words.what),
            cell(&words.next),
            words.who,
            words.code.unwrap_or("")
        ));
    }
    out
}
