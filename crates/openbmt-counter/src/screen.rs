//! What the cashier sees and what leaves the counter (CR-10, CR-30 to
//! CR-32).

use openbmt_core::policy::{CheckMode, InstantLabel, OffReason, WaitReason};
use openbmt_watch::Problem;

use crate::desk::Role;
use crate::money::{Amount, Currency};

/// The color of a state (CR-10).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Color {
    /// Nothing is wrong and the money is not in yet.
    Blue,
    /// The goods may be handed over.
    Green,
    /// Do not hand over the goods.
    Red,
    /// The sale ended with nothing received.
    Neutral,
}

/// The state of a sale (CR-10).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    /// Nothing has arrived (blue).
    NotPaidYet,
    /// Less than the sale needs has arrived (blue).
    PartPaid,
    /// Enough has arrived and it waits for a confirmation (blue).
    Waiting,
    /// Accepted at once (green).
    PaidInstant,
    /// Confirmed at the shop's depth (green).
    Confirmed,
    /// Confirmed at the refund depth (green).
    Final,
    /// A person accepted it (green; CR-20).
    Overridden,
    /// Paid over Lightning, which is final at once (green; CR-109).
    PaidLightning,
    /// The money is not safe (red).
    DoNotHandOver,
    /// Ended with nothing received (neutral).
    Expired,
    /// Cancelled with nothing received (neutral).
    Cancelled,
    /// The sale holds more than the counter can judge (red).
    NeedsAPerson,
}

impl State {
    /// The color of the state.
    pub fn color(&self) -> Color {
        match self {
            Self::NotPaidYet | Self::PartPaid | Self::Waiting => Color::Blue,
            Self::PaidInstant
            | Self::Confirmed
            | Self::Final
            | Self::Overridden
            | Self::PaidLightning => Color::Green,
            Self::DoNotHandOver | Self::NeedsAPerson => Color::Red,
            Self::Expired | Self::Cancelled => Color::Neutral,
        }
    }

    /// The name shown for the state.
    pub fn name(&self) -> &'static str {
        match self {
            Self::NotPaidYet => "Not paid yet",
            Self::PartPaid => "Part paid",
            Self::Waiting => "Waiting",
            Self::PaidInstant | Self::PaidLightning => "Paid",
            Self::Confirmed => "Confirmed",
            Self::Final => "Final",
            Self::Overridden => "Accepted by a person",
            Self::DoNotHandOver => "Do not hand over",
            Self::Expired => "Expired",
            Self::Cancelled => "Cancelled",
            Self::NeedsAPerson => "Needs a person",
        }
    }

    /// Whether the state is one the POS is told is paid (CR-30).
    pub fn is_paid(&self) -> bool {
        matches!(
            self,
            Self::PaidInstant
                | Self::Confirmed
                | Self::Final
                | Self::Overridden
                | Self::PaidLightning
        )
    }
}

/// Why a sale is in its state (CR-10: the first of the two texts).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Why {
    /// Nothing has arrived.
    NothingArrived,
    /// Part of the need has arrived.
    Part {
        /// What has arrived.
        received_sats: u64,
        /// What the sale needs.
        needs_sats: u64,
    },
    /// Not yet seen by enough of the network.
    NotSeenEnough,
    /// The instant decision says wait.
    Wait(WaitReason),
    /// The instant decision says instant is off.
    InstantOff(OffReason),
    /// The acceptance could not be saved (CR-13).
    NotStored,
    /// The pause after a cancel could not be saved yet (CR-14).
    PauseNotSaved,
    /// Accepted at once, with the label of the check that was done.
    Instant(InstantLabel),
    /// Confirmed at this depth.
    ConfirmedAt(u32),
    /// Final at this depth.
    FinalAt(u32),
    /// The shop's own node says the Lightning invoice was paid (CR-109).
    PaidByLightning,
    /// A person accepted it.
    OverriddenBy {
        /// Cashier or manager.
        role: Role,
    },
    /// A conflicting transaction was seen (CR-12 rule 3).
    Replaced,
    /// A block that confirmed the payment left the chain (CR-12 rule 2).
    Reopened,
    /// The check is not healthy, so the payment cannot be checked.
    CannotCheck(Option<Problem>),
    /// The watcher refused something and needs a person (WR-34).
    WatcherNeedsAPerson,
    /// The stored form of the sale does not read back (CR-42).
    StoredFormUnreadable,
    /// The sale expired with nothing received.
    EndedExpired,
    /// The sale was cancelled with nothing received.
    EndedCancelled,
}

/// What happens next (CR-10: the second of the two texts).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Next {
    /// The customer pays.
    CustomerPays,
    /// The customer pays the rest.
    CustomerPaysRest {
        /// What is still missing.
        missing_sats: u64,
    },
    /// The counter keeps watching the network.
    KeepWatching,
    /// The counter watches for a conflicting payment a few more seconds.
    WatchSeconds {
        /// Seconds left of the watch.
        secs_left: u64,
    },
    /// The sale is accepted when it has a confirmation.
    WaitForConfirmation {
        /// The shop's depth.
        depth: u32,
    },
    /// The sale becomes final at the refund depth.
    FinalAt {
        /// The refund depth.
        depth: u32,
    },
    /// The owner clears the pause or the alarm from the owner phone.
    OwnerClears,
    /// Ask the owner, or take another payment type.
    AskOwnerOrOtherPayment,
    /// A person looks at the sale.
    PersonLooks,
    /// Nothing more happens.
    Nothing,
}

/// What the screen is told (CR-10).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Screen {
    /// The state.
    pub state: State,
    /// Why (the first text).
    pub why: Why,
    /// What happens next (the second text).
    pub next: Next,
    /// What has arrived, with dust counted as nothing.
    pub received_sats: u64,
    /// What the sale needs.
    pub needs_sats: u64,
    /// CR-18: a seen payment has not confirmed for too long.
    pub stuck: bool,
    /// CR-19: a payment arrived after the sale ended.
    pub late: bool,
    /// How the standing unconfirmed payment was checked.
    pub check_mode: Option<CheckMode>,
    /// CR-105: the own node has reported the standing unconfirmed payment,
    /// whether or not it can be used now (WR-61).
    pub own_node_saw: bool,
    /// CR-105: the own node's report is what makes `check_mode` the own-node
    /// check (WR-62). False with `own_node_saw` true is the fallback to the
    /// light check (WR-64).
    pub own_node_counted: bool,
    /// A conflict has been seen on the sale.
    pub conflict_seen: bool,
    /// A block that confirmed the payment left the chain and it is not
    /// confirmed again.
    pub reopened: bool,
    /// What leaves the counter now, each stored first (CR-32). Each is
    /// returned once.
    pub reports: Vec<Report>,
}

/// Something the connector must tell the POS, the books or the owner (CR-30
/// to CR-32, CR-14, CR-16, CR-17, CR-19). Each is written as a record and
/// stored before it is returned, and returned once.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Report {
    /// CR-30: close the sale in the POS as paid.
    PosPaid {
        /// The receive index.
        index: u32,
    },
    /// CR-30: the sale was closed as paid and is not safe any more.
    PosUnpaid {
        /// The receive index.
        index: u32,
    },
    /// CR-31: the books may count the sale as money.
    BooksFinal {
        /// The receive index.
        index: u32,
        /// The amount in the shop's currency.
        amount: Amount,
        /// The currency.
        currency: Currency,
        /// What arrived, in satoshis.
        received_sats: u64,
    },
    /// CR-16: more arrived than the sale needs; the surplus is owed to the
    /// customer.
    SurplusOwed {
        /// The receive index.
        index: u32,
        /// The surplus.
        sats: u64,
    },
    /// CR-19: a payment arrived for a sale that had ended; flagged for the
    /// owner.
    LatePayment {
        /// The receive index.
        index: u32,
        /// What it pays.
        sats: u64,
    },
    /// CR-14: the cancel alarm went up.
    Alarm {
        /// The receive index.
        index: u32,
    },
    /// CR-17: what arrived is below the dust line.
    Dust {
        /// The receive index.
        index: u32,
        /// What arrived.
        sats: u64,
    },
}

/// The words for a problem with the check.
pub fn problem_text(p: Option<Problem>) -> &'static str {
    match p {
        Some(Problem::TooFewPeerGroups) => {
            "the counter is connected to too few parts of the network"
        }
        Some(Problem::TooFewFilterPeers) => "too few peers can serve the counter's checks",
        Some(Problem::TipIsOld) => "the counter's newest block is too old",
        Some(Problem::HeadersBehind) => "the counter is behind the network",
        Some(Problem::ClockMismatch) => "the counter's clock does not match the network's",
        Some(Problem::BlocksBehind) => "the counter is waiting for a new block",
        None => "the check is not working",
    }
}

fn wait_text(w: WaitReason) -> String {
    match w {
        WaitReason::ConflictSeen => "a conflicting payment was seen".to_string(),
        WaitReason::RemoteOrder => "it is a remote order".to_string(),
        WaitReason::Underpaid => "less arrived than the sale needs".to_string(),
        WaitReason::OverSaleLimit => {
            "the sale is above the limit for accepting at once".to_string()
        }
        WaitReason::SliceFull => "this counter has too much accepted at once already".to_string(),
        WaitReason::ConflictWatch { secs_left } => {
            format!("watching for a conflicting payment, {secs_left} seconds left")
        }
    }
}

fn off_text(o: OffReason) -> String {
    match o {
        OffReason::PausedFor { secs_left } => {
            format!("accepting at once is paused for {secs_left} more seconds after a cancel")
        }
        OffReason::PausedUntilOwnerReset => {
            "accepting at once is paused until the owner resets it".to_string()
        }
        OffReason::Disabled => "accepting at once is switched off for this check".to_string(),
        OffReason::CheckUnhealthy => "the check is not healthy".to_string(),
        OffReason::NoWatcher => "accepting at once is off: no independent watcher".to_string(),
        OffReason::SharedWifiNeedsTor => "customers can join the Wi-Fi and Tor is off".to_string(),
    }
}

impl Why {
    /// The first text, in plain words.
    pub fn text(&self) -> String {
        match self {
            Self::NothingArrived => "Nothing has arrived yet.".to_string(),
            Self::Part {
                received_sats,
                needs_sats,
            } => format!("Paid {received_sats} of {needs_sats} sats."),
            Self::NotSeenEnough => "Not yet seen by enough of the network.".to_string(),
            Self::Wait(w) => format!("Waiting for a confirmation: {}.", wait_text(*w)),
            Self::InstantOff(o) => format!("Waiting for a confirmation: {}.", off_text(*o)),
            Self::NotStored => {
                "Waiting for a confirmation: the acceptance could not be saved.".to_string()
            }
            Self::PauseNotSaved => {
                "Waiting for a confirmation: the pause after a cancel is not saved yet.".to_string()
            }
            Self::Instant(label) => format!("{}. It could still be cancelled.", label.text()),
            Self::ConfirmedAt(depth) => format!("Confirmed ({depth} confirmations)."),
            Self::FinalAt(depth) => format!("Final ({depth} confirmations)."),
            Self::PaidByLightning => "Paid over Lightning. It is final.".to_string(),
            Self::OverriddenBy { role } => {
                format!("Accepted by a {}. It is not confirmed.", role.text())
            }
            Self::Replaced => "A conflicting payment was seen.".to_string(),
            Self::Reopened => {
                "A block that held the payment is no longer on the chain.".to_string()
            }
            Self::CannotCheck(p) => format!("Cannot check: {}.", problem_text(*p)),
            Self::WatcherNeedsAPerson => {
                "The sale holds more than the counter can judge.".to_string()
            }
            Self::StoredFormUnreadable => {
                "The saved record of this sale cannot be read.".to_string()
            }
            Self::EndedExpired => "The sale expired with nothing received.".to_string(),
            Self::EndedCancelled => "The sale was cancelled with nothing received.".to_string(),
        }
    }
}

impl Next {
    /// The second text, in plain words.
    pub fn text(&self) -> String {
        match self {
            Self::CustomerPays => "The customer pays the address shown.".to_string(),
            Self::CustomerPaysRest { missing_sats } => {
                format!("The customer pays the rest, {missing_sats} sats.")
            }
            Self::KeepWatching => "The counter keeps watching the network.".to_string(),
            Self::WatchSeconds { secs_left } => {
                format!("The counter watches {secs_left} more seconds for a conflicting payment.")
            }
            Self::WaitForConfirmation { depth } => {
                format!("It is accepted at {depth} confirmation(s).")
            }
            Self::FinalAt { depth } => format!("It becomes final at {depth} confirmations."),
            Self::OwnerClears => "The owner clears this from the owner phone.".to_string(),
            Self::AskOwnerOrOtherPayment => {
                "Do not hand over the goods. Ask the owner or take another payment type."
                    .to_string()
            }
            Self::PersonLooks => "A person must look at this sale.".to_string(),
            Self::Nothing => "Nothing more happens.".to_string(),
        }
    }
}

impl Screen {
    /// The first text, with the CR-18 and CR-19 additions.
    pub fn why_text(&self) -> String {
        let mut text = self.why.text();
        if self.stuck {
            text.push_str(" Stuck: not confirming.");
        }
        if self.late {
            text.push_str(" Late payment: flagged for the owner.");
        }
        text
    }

    /// The second text.
    pub fn next_text(&self) -> String {
        self.next.text()
    }
}
