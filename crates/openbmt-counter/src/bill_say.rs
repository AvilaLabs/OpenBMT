//! What a cashier is told about bills, tabs and the till's reference
//! (CR-255), under `docs/SCREEN-RULES.md`.

use std::fmt::Display;

use openbmt_core::say::{Tone, Who, Words};

use crate::bills::{BillEnd, BillKind, SplitError};
use crate::counter::BillView;
use crate::refusal::{BillRefused, PartRefused};
use crate::say::{refused, store_failed, RETRY_NEXT};

impl<E: Display> BillRefused<E> {
    /// What the cashier is told when a bill, a tab or a reference is refused
    /// (CR-255). Nothing was written.
    pub fn words(&self) -> Words {
        let code = self.rule();
        let c = Who::Cashier;
        let words = match self {
            Self::UnknownSale(_) => refused(
                "No such sale",
                "No sale on this counter has that number.",
                c,
                "Check the sale's number and pick it again.",
                code,
            ),
            Self::UnknownBill(_) => refused(
                "No such bill",
                "No bill on this counter has that number.",
                c,
                "Pick the bill from the list of bills.",
                code,
            ),
            Self::BadReference => refused(
                "Reference does not fit",
                "The till reference has a sign or a space that is not allowed, or is over 32 characters.",
                c,
                "Type the reference again as the till shows it, without spaces.",
                code,
            ),
            Self::AlreadyReferenced => refused(
                "Reference already set",
                "This already has a till reference, and it takes only one.",
                c,
                "Nothing to change. If the reference is wrong, tell the manager.",
                code,
            ),
            Self::BadAmount => refused(
                "Amount not possible",
                "A bill's amount is above zero and no more than the largest amount a sale can be.",
                c,
                CHECK_AMOUNT,
                code,
            ),
            Self::TooManyOpen => refused(
                "Too many open bills",
                "256 bills are open on this counter, which is the most it keeps.",
                c,
                "Settle, close or cancel an old bill first, then open this one.",
                code,
            ),
            Self::TooManyParts => refused(
                "Too many parts",
                "This bill has 32 parts, which is the most a bill can have.",
                c,
                "Take the rest of this bill as one payment.",
                code,
            ),
            Self::NotOpen => refused(
                "Bill already ended",
                "This bill is settled, closed or cancelled, so it takes no more parts or changes.",
                c,
                "Open a new bill if the customer still owes something.",
                code,
            ),
            Self::SaleInABill(_) => refused(
                "Sale is in another bill",
                "This sale is already a part of a bill, and a sale belongs to one bill only.",
                c,
                "Pick another sale, or take a new payment for this bill.",
                code,
            ),
            Self::SaleEnded => refused(
                "Sale ended unpaid",
                "This sale ended with nothing received, so it cannot pay a bill.",
                c,
                "Take a new payment for this bill.",
                code,
            ),
            Self::OtherCurrency => refused(
                "Other currency",
                "This sale was priced in another currency than the bill.",
                c,
                "Take a new payment for this bill in the shop's currency.",
                code,
            ),
            Self::NotATab => refused(
                "Only a tab changes",
                "The amount of a bill is fixed when it opens. Only a tab's amount can change.",
                c,
                "If the amount is wrong, cancel the bill and open a new one, or ask the manager.",
                code,
            ),
            Self::BelowReceived { .. } => refused(
                "Below what was paid",
                "A tab cannot be lowered below what it has already received.",
                c,
                "Enter an amount at least as large as what was paid. See Details.",
                code,
            ),
            Self::NotReached { .. } => refused(
                "Tab not fully paid",
                "This tab has not yet received its whole amount, so it stays open.",
                c,
                "Take a payment for what is left, or ask the manager to close it short. See Details.",
                code,
            ),
            Self::NotAFinishableBill => refused(
                "Settles by itself",
                "A bill that is not a tab is settled as soon as it has received its amount.",
                c,
                "Nothing to do. Take a payment for what is left.",
                code,
            ),
            Self::NothingShort => refused(
                "Nothing is short",
                "This bill has received its whole amount, so there is nothing to close short.",
                c,
                "Nothing to do.",
                code,
            ),
            Self::NeedsManager => refused(
                "Manager's PIN needed",
                "Closing a bill short leaves money unpaid, so only a manager can do it.",
                Who::Manager,
                "The manager enters their PIN to close it short.",
                code,
            ),
            Self::HasPart => refused(
                "Cannot cancel",
                "This bill has received a payment or has one on its way, so it cannot be cancelled.",
                c,
                "Ask the manager to close it short. Refund what was paid if it is to be given back.",
                code,
            ),
            Self::BadSplit(SplitError::People) => refused(
                "Split for 2 to 20",
                "What is left is split between 2 and 20 people.",
                c,
                "Enter a number of people from 2 to 20.",
                code,
            ),
            Self::BadSplit(SplitError::TooLittle) => refused(
                "Too little to split",
                "What is left is too small to give each person a share.",
                c,
                "Split it between fewer people, or take it as one payment.",
                code,
            ),
            Self::Store(fault) => {
                return store_failed(
                    fault,
                    "Bill not saved",
                    "the bill was not changed",
                    Tone::Stop,
                    c,
                    RETRY_NEXT,
                )
            }
        };
        words.detail(self.to_string())
    }
}

/// What to do about an amount that is not possible.
const CHECK_AMOUNT: &str = "Check the amount and enter it again.";

impl<E: Display> PartRefused<E> {
    /// What the cashier is told when a part is not opened for a bill.
    pub fn words(&self) -> Words {
        match self {
            Self::Bill(e) => e.words(),
            Self::Open(e) => e.words(),
        }
    }
}

impl BillView {
    /// The state of a bill (CR-246, CR-255).
    pub fn words(&self) -> Words {
        let c = Who::Cashier;
        match (self.bill.ended().map(|e| e.how), self.bill.kind()) {
            (None, BillKind::Bill) => Words::new(
                Tone::Info,
                "Bill open",
                "The bill is paid in parts. Each part is a payment of its own, in bitcoin or another way.",
                c,
                "Take the next part when its customer is ready.",
            ),
            (None, BillKind::Tab) => Words::new(
                Tone::Info,
                "Tab open",
                "The tab stays open, and its amount can be raised, until it is finished.",
                c,
                "Add to it as the customer orders. Finish it when the customer has paid the whole amount.",
            ),
            (Some(BillEnd::Settled), _) => Words::new(
                Tone::Good,
                "Bill paid",
                "The bill has received its whole amount.",
                Who::Nobody,
                "Nothing to do.",
            ),
            (Some(BillEnd::ClosedShort), _) => Words::new(
                Tone::Ended,
                "Closed short",
                "The manager closed this bill before its whole amount was paid. What was not paid is in the books.",
                Who::Nobody,
                "Nothing to do.",
            ),
            (Some(BillEnd::Cancelled), _) => Words::new(
                Tone::Ended,
                "Bill cancelled",
                "The bill was cancelled before any payment counted towards it.",
                Who::Nobody,
                "Nothing to do.",
            ),
        }
        .code("CR-246")
    }
}

/// The words of every bill state and refusal, for the test of SC-12.
pub(crate) fn all_bill_words() -> Vec<(String, Words)> {
    use crate::bills::Bill;
    use crate::money::{Amount, Currency};
    let w = || StoreFault::Write("disk full".to_string());
    let mut all: Vec<(String, Words)> = Vec::new();
    let refusals: Vec<(&str, BillRefused<String>)> = vec![
        ("UnknownSale", BillRefused::UnknownSale(3)),
        ("UnknownBill", BillRefused::UnknownBill(3)),
        ("BadReference", BillRefused::BadReference),
        ("AlreadyReferenced", BillRefused::AlreadyReferenced),
        ("BadAmount", BillRefused::BadAmount),
        ("TooManyOpen", BillRefused::TooManyOpen),
        ("TooManyParts", BillRefused::TooManyParts),
        ("NotOpen", BillRefused::NotOpen),
        ("SaleInABill", BillRefused::SaleInABill(2)),
        ("SaleEnded", BillRefused::SaleEnded),
        ("OtherCurrency", BillRefused::OtherCurrency),
        ("NotATab", BillRefused::NotATab),
        (
            "BelowReceived",
            BillRefused::BelowReceived { received: 500 },
        ),
        ("NotReached", BillRefused::NotReached { left: 500 }),
        ("NotAFinishableBill", BillRefused::NotAFinishableBill),
        ("NothingShort", BillRefused::NothingShort),
        ("NeedsManager", BillRefused::NeedsManager),
        ("HasPart", BillRefused::HasPart),
        ("BadSplit/People", BillRefused::BadSplit(SplitError::People)),
        (
            "BadSplit/TooLittle",
            BillRefused::BadSplit(SplitError::TooLittle),
        ),
        ("Store", BillRefused::Store(w())),
    ];
    for (name, r) in refusals {
        all.push((format!("BillRefused::{name}"), r.words()));
    }
    let bill = |kind, ended| BillView {
        bill: Bill {
            number: 1,
            kind,
            amount: Amount(1_000),
            currency: Currency::of_shop(openbmt_core::currency::ShopCurrency::Usd),
            opened_at: 0,
            reference: None,
            parts: Vec::new(),
            ended,
        },
        received_bitcoin: 0,
        received_other: 0,
        pending: 0,
        left: 1_000,
    };
    let end = |how| {
        Some(crate::bills::Ended {
            how,
            at: 1,
            unpaid: Amount(0),
        })
    };
    for (name, v) in [
        ("Open", bill(BillKind::Bill, None)),
        ("Tab", bill(BillKind::Tab, None)),
        ("Settled", bill(BillKind::Bill, end(BillEnd::Settled))),
        (
            "ClosedShort",
            bill(BillKind::Bill, end(BillEnd::ClosedShort)),
        ),
        ("Cancelled", bill(BillKind::Tab, end(BillEnd::Cancelled))),
    ] {
        all.push((format!("BillView::{name}"), v.words()));
    }
    all
}

use crate::store::StoreFault;
