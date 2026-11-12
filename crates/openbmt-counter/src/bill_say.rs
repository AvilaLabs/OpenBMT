//! What a cashier is told about bills, tabs and the till's reference
//! (CR-255). The words are not written yet: each state and refusal carries
//! the placeholder constant that names its rule, `WORDS_NEEDED_CR_xxx`, in
//! its heading and both sentences, until the words are written under
//! `docs/SCREEN-RULES.md`.

use std::fmt::Display;

use openbmt_core::say::{Tone, Who, Words};

use crate::bills::{BillEnd, BillKind, SplitError};
use crate::counter::BillView;
use crate::refusal::{BillRefused, PartRefused};

/// CR-245: a reference that does not fit (it is also the words of a sale
/// opened with one that does not fit).
pub const WORDS_NEEDED_CR_245_BAD: &str = "WORDS_NEEDED_CR_245_BAD";
/// CR-245: a sale or a bill that has a reference already.
pub const WORDS_NEEDED_CR_245_TWICE: &str = "WORDS_NEEDED_CR_245_TWICE";
/// CR-246: no bill with that number.
pub const WORDS_NEEDED_CR_246_NOBILL: &str = "WORDS_NEEDED_CR_246_NOBILL";
/// CR-246: an amount of nothing, or too large.
pub const WORDS_NEEDED_CR_246_AMOUNT: &str = "WORDS_NEEDED_CR_246_AMOUNT";
/// CR-246: the bill has ended.
pub const WORDS_NEEDED_CR_246_ENDED: &str = "WORDS_NEEDED_CR_246_ENDED";
/// CR-246: state of a bill: open.
pub const WORDS_NEEDED_CR_246_OPEN: &str = "WORDS_NEEDED_CR_246_OPEN";
/// CR-246: state of a tab: open.
pub const WORDS_NEEDED_CR_246_TAB: &str = "WORDS_NEEDED_CR_246_TAB";
/// CR-246: state of a bill: settled.
pub const WORDS_NEEDED_CR_246_SETTLED: &str = "WORDS_NEEDED_CR_246_SETTLED";
/// CR-246: state of a bill: closed short.
pub const WORDS_NEEDED_CR_246_SHORT: &str = "WORDS_NEEDED_CR_246_SHORT";
/// CR-246: state of a bill: cancelled.
pub const WORDS_NEEDED_CR_246_CANCEL: &str = "WORDS_NEEDED_CR_246_CANCEL";
/// CR-248: no sale with that index.
pub const WORDS_NEEDED_CR_248_NOSALE: &str = "WORDS_NEEDED_CR_248_NOSALE";
/// CR-248: the sale is a part of a bill already.
pub const WORDS_NEEDED_CR_248_INBILL: &str = "WORDS_NEEDED_CR_248_INBILL";
/// CR-248: the sale ended with nothing received.
pub const WORDS_NEEDED_CR_248_SALEEND: &str = "WORDS_NEEDED_CR_248_SALEEND";
/// CR-248: the sale is in another currency.
pub const WORDS_NEEDED_CR_248_CURR: &str = "WORDS_NEEDED_CR_248_CURR";
/// CR-250: the number of people is not from 2 to 20.
pub const WORDS_NEEDED_CR_250_PEOPLE: &str = "WORDS_NEEDED_CR_250_PEOPLE";
/// CR-250: too little left to split that way.
pub const WORDS_NEEDED_CR_250_LITTLE: &str = "WORDS_NEEDED_CR_250_LITTLE";
/// CR-251: only a tab's amount changes.
pub const WORDS_NEEDED_CR_251_NOTTAB: &str = "WORDS_NEEDED_CR_251_NOTTAB";
/// CR-251: a tab is not lowered below what it has received.
pub const WORDS_NEEDED_CR_251_BELOW: &str = "WORDS_NEEDED_CR_251_BELOW";
/// CR-251: a tab is finished only when it has received its amount.
pub const WORDS_NEEDED_CR_251_NOTYET: &str = "WORDS_NEEDED_CR_251_NOTYET";
/// CR-251: a bill that is not a tab is settled by what it receives.
pub const WORDS_NEEDED_CR_251_NOFIN: &str = "WORDS_NEEDED_CR_251_NOFIN";
/// CR-252: nothing is short, so there is nothing to close short.
pub const WORDS_NEEDED_CR_252_NOSHORT: &str = "WORDS_NEEDED_CR_252_NOSHORT";
/// CR-252: closing short needs the manager's PIN.
pub const WORDS_NEEDED_CR_252_PIN: &str = "WORDS_NEEDED_CR_252_PIN";
/// CR-252: a part counts or is pending, so the bill is not cancelled.
pub const WORDS_NEEDED_CR_252_HASPART: &str = "WORDS_NEEDED_CR_252_HASPART";
/// CR-253: 256 bills are open already.
pub const WORDS_NEEDED_CR_253_OPEN256: &str = "WORDS_NEEDED_CR_253_OPEN256";
/// CR-253: the bill has 32 parts already.
pub const WORDS_NEEDED_CR_253_PARTS32: &str = "WORDS_NEEDED_CR_253_PARTS32";
/// CR-255: a write of the bills failed.
pub const WORDS_NEEDED_CR_255_STORE: &str = "WORDS_NEEDED_CR_255_STORE";
/// CR-253: the saved bills do not read back.
pub const WORDS_NEEDED_CR_253_UNREAD: &str = "WORDS_NEEDED_CR_253_UNREAD";

/// Words that are still to be written: the placeholder stands in the heading
/// and, as a sentence, in both lines, so that it shows wherever the words
/// would.
pub fn needed(placeholder: &'static str) -> Words {
    Words::new(
        Tone::Ended,
        placeholder,
        format!("{placeholder}_WHAT."),
        Who::Cashier,
        format!("{placeholder}_NEXT."),
    )
}

/// Whether these words are a placeholder.
pub fn is_placeholder(words: &Words) -> bool {
    words.heading.starts_with("WORDS_NEEDED_")
}

impl<E: Display> BillRefused<E> {
    /// What the cashier is told when a bill, a tab or a reference is refused:
    /// the placeholder of the rule (CR-255).
    pub fn words(&self) -> Words {
        needed(match self {
            Self::UnknownSale(_) => WORDS_NEEDED_CR_248_NOSALE,
            Self::UnknownBill(_) => WORDS_NEEDED_CR_246_NOBILL,
            Self::BadReference => WORDS_NEEDED_CR_245_BAD,
            Self::AlreadyReferenced => WORDS_NEEDED_CR_245_TWICE,
            Self::BadAmount => WORDS_NEEDED_CR_246_AMOUNT,
            Self::TooManyOpen => WORDS_NEEDED_CR_253_OPEN256,
            Self::TooManyParts => WORDS_NEEDED_CR_253_PARTS32,
            Self::NotOpen => WORDS_NEEDED_CR_246_ENDED,
            Self::SaleInABill(_) => WORDS_NEEDED_CR_248_INBILL,
            Self::SaleEnded => WORDS_NEEDED_CR_248_SALEEND,
            Self::OtherCurrency => WORDS_NEEDED_CR_248_CURR,
            Self::NotATab => WORDS_NEEDED_CR_251_NOTTAB,
            Self::BelowReceived { .. } => WORDS_NEEDED_CR_251_BELOW,
            Self::NotReached { .. } => WORDS_NEEDED_CR_251_NOTYET,
            Self::NotAFinishableBill => WORDS_NEEDED_CR_251_NOFIN,
            Self::NothingShort => WORDS_NEEDED_CR_252_NOSHORT,
            Self::NeedsManager => WORDS_NEEDED_CR_252_PIN,
            Self::HasPart => WORDS_NEEDED_CR_252_HASPART,
            Self::BadSplit(SplitError::People) => WORDS_NEEDED_CR_250_PEOPLE,
            Self::BadSplit(SplitError::TooLittle) => WORDS_NEEDED_CR_250_LITTLE,
            Self::Store(_) => WORDS_NEEDED_CR_255_STORE,
        })
        .code(self.rule())
        .detail(self.to_string())
    }
}

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
    /// The state of a bill (CR-255): the placeholder of the state.
    pub fn words(&self) -> Words {
        needed(match (self.bill.ended().map(|e| e.how), self.bill.kind()) {
            (None, BillKind::Bill) => WORDS_NEEDED_CR_246_OPEN,
            (None, BillKind::Tab) => WORDS_NEEDED_CR_246_TAB,
            (Some(BillEnd::Settled), _) => WORDS_NEEDED_CR_246_SETTLED,
            (Some(BillEnd::ClosedShort), _) => WORDS_NEEDED_CR_246_SHORT,
            (Some(BillEnd::Cancelled), _) => WORDS_NEEDED_CR_246_CANCEL,
        })
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
