//! What the owner is told about the till (the POS), its alerts and its
//! refusals (`docs/SCREEN-RULES.md`, SC-01 to SC-12).
//!
//! The reader is the owner. The word for a POS is "the till", for a POS
//! order "the order at the till", and for a Bitcoin tender "a bitcoin
//! payment". Order and payment numbers come from the till's system: they
//! are text from elsewhere, so they are only ever quoted
//! (`Quoted::new("The till's order number", ...)`), never part of a
//! sentence. Amounts are in the shop's currency as the alert holds them.
//! An alert names the first thing to check: the order at the till, the
//! cashier, or the counter.

use openbmt_core::records::ChainError;
use openbmt_core::say::{Quoted, Tone, Who, Words};
use openbmt_core::wire::DecodeError;

use crate::alerts::{Alert, AlertKind, Subject};
use crate::connector::{CloseRefused, PosError, SyncError};
use crate::facts::FactsError;
use crate::ledger::{Blocker, LedgerError, LedgerWriteError};
use crate::matcher::{Match, MatchHow};
use crate::model::{ModelError, OrderId};
use crate::notice::{PosItem, SaleMismatch};
use crate::settings::SettingsError;
use crate::{MAX_CURRENCIES, MAX_TENDERS, MAX_TENDER_NAMES};

fn stop(heading: &str, what: impl Into<String>, who: Who, next: impl Into<String>) -> Words {
    Words::new(Tone::Stop, heading, what, who, next)
}

/// An amount in minor units with its currency, as the alert holds it.
fn money(minor: u64, currency: Option<openbmt_counter::Currency>) -> String {
    let code = currency.map_or_else(String::new, |c| format!(" {c}"));
    format!("{}.{:02}{code}", minor / 100, minor % 100)
}

fn minutes(secs: u64) -> String {
    match secs / 60 {
        1 => "1 minute".to_string(),
        n => format!("{n} minutes"),
    }
}

/// The words with the till's order number quoted.
fn with_order(words: Words, order: &OrderId) -> Words {
    words.quoted(Quoted::new("The till's order number", order.as_str()))
}

impl Alert {
    /// What the owner is told about one alert (PS-40 to PS-48). Each says
    /// what the till and the counter show, and what the owner checks next.
    /// The time is that of the thing at issue; the order number, when there
    /// is one, is quoted.
    pub fn words(&self) -> Words {
        let tender = money(self.tender_minor, self.currency);
        let sale = money(self.sale_minor, self.currency);
        let words = match self.key.kind {
            AlertKind::TenderWithoutPayment => stop(
                "Bitcoin payment, no sale",
                format!("The till recorded a bitcoin payment of {tender}, and no bitcoin sale matches it."),
                Who::Owner,
                "Ask the cashier whether the customer paid, and check the order's till location is set up in the settings.",
            )
            .code("PS-40"),
            AlertKind::ClosedBeforePaid => stop(
                "Order closed too early",
                format!(
                    "The till closed the order with a bitcoin payment {} before the counter showed the payment as paid.",
                    minutes(self.secs)
                ),
                Who::Owner,
                "Ask the cashier whether the goods were handed over only after the counter turned green.",
            )
            .code("PS-41"),
            AlertKind::PaymentWithoutTender => stop(
                "Sale paid, no till order",
                format!("The counter showed a sale of {sale} as paid, and no bitcoin payment at the till matches it."),
                Who::Owner,
                "Find the order at the till and close it with bitcoin, or refund the customer; a counter with no till is set so in the settings.",
            )
            .code("PS-42"),
            AlertKind::AmountDiffers => stop(
                "Amounts do not match",
                format!("A bitcoin payment of {tender} at the till names a counter sale that was for {sale}."),
                Who::Owner,
                "Compare the order with the counter's sale; the customer may be owed the difference, or the cashier typed the wrong sale number.",
            )
            .code("PS-43"),
            AlertKind::TenderOnUnpaidSale => stop(
                "Till payment, sale unpaid",
                format!("The till recorded a bitcoin payment of {tender} for a sale that has not been paid."),
                Who::Owner,
                "Tell the cashier not to hand over the goods, and look at the sale on the counter; a replaced payment must be paid again.",
            )
            .code("PS-44"),
            AlertKind::VoidedAfterPayment => stop(
                "Order undone after payment",
                format!("The till voided or refunded the order or its bitcoin payment of {tender}, and the counter sale of {sale} is still paid."),
                Who::Owner,
                "Ask the cashier and check the refund queue: either the customer is owed a refund or the cashier undid the order.",
            )
            .code("PS-45"),
            AlertKind::DuplicateReference => stop(
                "Sale named by two payments",
                format!("A bitcoin payment of {tender} at the till names a counter sale that another till payment already holds."),
                Who::Owner,
                "Ask the cashier which order the payment was for, then void the other order at the till.",
            )
            .code("PS-46"),
            AlertKind::BitcoinTip => stop(
                "Tip on a bitcoin payment",
                format!("A bitcoin payment at the till carries a tip of {tender}, but the counter takes no tip."),
                Who::Owner,
                "Ask the cashier how the tip was paid; it did not come through the counter.",
            )
            .code("PS-47"),
            AlertKind::UnknownCurrency => stop(
                "Payment in another currency",
                format!("A bitcoin payment of {tender} at the till is in a currency the shop has not set up."),
                Who::Owner,
                "Add the currency in the settings if the shop uses it, or ask the cashier why the order is in it.",
            )
            .code("PS-48"),
        };
        let words = words.at(self.at);
        match &self.key.subject {
            Subject::Tender(t) => with_order(words, &t.order),
            Subject::Sale(_) => words,
        }
    }
}

impl SaleMismatch {
    /// What the owner is told when a paid notice from the counter gives a
    /// sale another amount, currency or opening time than earlier notices.
    /// The notice was not used.
    pub fn words(&self) -> Words {
        let (heading, what) = match self {
            Self::Amount => (
                "Sale amount changed",
                "A notice from the Box gives this sale another amount than earlier notices did.",
            ),
            Self::Currency => (
                "Sale currency changed",
                "A notice from the Box gives this sale another currency than earlier notices did.",
            ),
            Self::OpenedAt => (
                "Sale opening time changed",
                "A notice from the Box gives this sale another opening time than earlier notices did.",
            ),
        };
        stop(
            heading,
            what,
            Who::BoxKeeper,
            "The notice was not used. Ask the Box keeper to check the Box and the counter: the notices for one sale disagree.",
        )
        .code("BX-40")
    }
}

impl PosError {
    /// What the owner is told when the till's system could not be read.
    /// Nothing was changed.
    pub fn words(&self) -> Words {
        match self {
            Self::Unavailable => stop(
                "Till system not reachable",
                "The Box could not reach the till's system.",
                Who::Owner,
                "Check that the till system is on and online, then try again.",
            ),
            Self::SignInRefused => stop(
                "Till refused the sign-in",
                "The till's system rejected the Box's sign-in: it was withdrawn or has run out.",
                Who::Owner,
                "Make a new sign-in in the till's system and give it to the Box keeper to store on the Box.",
            ),
            Self::RateLimited { retry_after_secs } => Words::new(
                Tone::Info,
                "Till system asks to wait",
                "The till's system asked the Box to read from it less often.",
                Who::Nobody,
                "Nothing to do. The Box reads again after the wait the till's system asked for.",
            )
            .detail(format!("wait {retry_after_secs} seconds")),
            Self::BadResponse(why) => stop(
                "Till answer not readable",
                "The till's system answered with something the Box could not read.",
                Who::Owner,
                "Try again later; if it repeats, stop and tell support, and read them the details.",
            )
            .detail(*why),
            Self::Unsupported => stop(
                "Not done by this till link",
                "The link to this till's system does not do that.",
                Who::Owner,
                "Do it at the till by hand. Nothing was changed.",
            ),
        }
    }
}

impl CloseRefused {
    /// What the owner is told when the Box was asked to close a till order
    /// for a sale that is not paid. Nothing was changed.
    pub fn words(&self) -> Words {
        match self {
            Self::SaleNotPaid => stop(
                "Order not closed",
                "The Box closes an order at the till only for a sale the counter shows as paid now.",
                Who::Owner,
                "Wait until the counter shows the sale as paid, then close the order again.",
            )
            .code("PS-81"),
        }
    }
}

/// The records of the till could not be stored: the same fault whether it
/// came while reading the till or while changing the ledger (SC-10).
fn store_fault(text: &str) -> Words {
    stop(
        "Till records not saved",
        "The Box could not store the till's records, so the last change was not kept.",
        Who::Owner,
        "Try again; if it repeats, stop and tell support. The system's own text is under Details.",
    )
    .code("PS-70")
    .detail(text)
}

impl<E: std::fmt::Display> SyncError<E> {
    /// What the owner is told when a read from the till's system stopped.
    /// What was stored before it stopped stays stored.
    pub fn words(&self) -> Words {
        match self {
            Self::Pos(e) => e.words(),
            Self::PageTooLarge => stop(
                "Till sent too much at once",
                "The till's system sent more orders in one page than the Box takes.",
                Who::Owner,
                "Try again later; if it repeats, stop and tell support: the till sends pages that are too large.",
            )
            .code("PS-80"),
            Self::NoProgress => stop(
                "Till is not moving on",
                "The till's system says there are more orders, but it does not move on to the next page.",
                Who::Owner,
                "Try again later; if it repeats, stop and tell support: the till does not move on.",
            )
            .code("PS-80"),
            Self::Ledger(e) => e.words(),
            Self::Persist(e) => store_fault(&e.to_string()),
        }
    }
}

impl<E: std::fmt::Display> LedgerWriteError<E> {
    /// What the owner is told when a change to the till's records was not
    /// made. The records are unchanged.
    pub fn words(&self) -> Words {
        match self {
            Self::Refused(e) => e.words(),
            Self::Persist(e) => store_fault(&e.to_string()),
        }
    }
}

impl SettingsError {
    /// What the owner is told when the till settings are refused. Nothing
    /// was saved.
    pub fn words(&self) -> Words {
        match self {
            Self::TenderNames => stop(
                "Bitcoin payment names",
                format!("The settings need from 1 to {MAX_TENDER_NAMES} names for bitcoin payments at the till, none left blank."),
                Who::Owner,
                "Correct the names in the till settings and save them again.",
            )
            .code("PS-10"),
            Self::Currencies => stop(
                "Currencies",
                format!("The settings need from 1 to {MAX_CURRENCIES} different currencies, each listed once."),
                Who::Owner,
                "Correct the currencies in the till settings and save them again.",
            )
            .code("PS-13"),
            Self::Locations => stop(
                "Till locations",
                "Each till location may be listed once, and each counter may belong to only one of them.",
                Who::Owner,
                "Correct the locations in the till settings and save them again.",
            )
            .code("PS-14"),
            Self::Timing => stop(
                "Timing figures",
                "A timing figure in the till settings is zero or out of order.",
                Who::Owner,
                "Correct the timing figures, or put the standard ones back, and save again.",
            ),
        }
    }
}

impl ModelError {
    /// What the owner is told when the till's system gave something the Box
    /// refuses. Nothing was saved from it.
    pub fn words(&self) -> Words {
        let next = "Nothing was saved from it. Stop and tell support what the till sends; the details are under Details.";
        match self {
            Self::BadId => stop(
                "Unusable order number",
                "The till sent an order or payment number that is empty, too long or has unusual characters.",
                Who::Owner,
                next,
            )
            .code("PS-01"),
            Self::BadText => stop(
                "Unusable text from the till",
                "The till sent a text that is empty, too long or holds a control character.",
                Who::Owner,
                next,
            )
            .code("PS-03"),
            Self::TooManyTenders => stop(
                "Too many payments on order",
                format!("An order at the till has more than {MAX_TENDERS} payments on it."),
                Who::Owner,
                next,
            )
            .code("PS-02"),
            Self::DuplicateTender => stop(
                "Two payments, one number",
                "Two payments on one order at the till have the same number.",
                Who::Owner,
                next,
            )
            .code("PS-02"),
            Self::RefundTooLarge => stop(
                "Refund larger than payment",
                "A refund at the till is larger than the payment it refunds with its tip.",
                Who::Owner,
                next,
            )
            .code("PS-02"),
            Self::CursorTooLong => stop(
                "Till's place marker too long",
                "The marker the till gives for where to read on is longer than the Box can keep.",
                Who::Owner,
                next,
            )
            .code("PS-72"),
        }
        .detail(self.to_string())
    }
}

/// How a counter's chain of records that does not check out is described.
fn chain_cause(e: &ChainError) -> &'static str {
    match e {
        ChainError::UnknownAuthor { .. } => "a record is from a device the shop does not know",
        ChainError::BadSignature { .. } => "a record's signature does not match it",
        ChainError::SeqMismatch { .. } => "a record is missing or out of place",
        ChainError::PrevMismatch { .. } => "a record does not follow the one before it",
        ChainError::BadCheckIn { .. } => "a check-in does not match the records around it",
        ChainError::Truncated { .. } => "records were cut off the end",
        ChainError::Diverged { .. } => "an earlier record was changed",
        ChainError::BodyTooLarge => "a record is larger than allowed",
        ChainError::NotACheckIn => "the app compared a record that is not a check-in",
    }
}

/// How a record that could not be read is described.
fn read_cause(e: &DecodeError) -> &'static str {
    match e {
        DecodeError::Truncated => "ends too early",
        DecodeError::TrailingBytes => "has extra data after its end",
        DecodeError::BadMagic => "is not a kind of record the Box knows",
        DecodeError::UnsupportedVersion(_) => "comes from a version of the app the Box does not read",
        DecodeError::TooLarge => "is larger than the Box allows",
        DecodeError::NonCanonical(_) => "is written in a form the Box does not accept",
        DecodeError::InvalidKey => "holds a key that is not valid",
        DecodeError::UnknownKind(_) => "holds an event the Box does not know",
    }
}

/// More sales than the matcher takes: the same fault whether it is found
/// while reading the counter's records or while changing the ledger.
fn too_many_sales() -> Words {
    stop(
        "Too many sales to match",
        "The counter has more sales than the Box can match against the till at once.",
        Who::Owner,
        "Stop and tell support: the counter has more sales than the Box can match.",
    )
    .code("PS-72")
}

impl FactsError {
    /// What the owner is told when the counter's sales could not be read
    /// for matching. Nothing was changed.
    pub fn words(&self) -> Words {
        match self {
            Self::Chain(e) => stop(
                "Counter's records refused",
                format!("The counter's records do not check out: {}.", chain_cause(e)),
                Who::Owner,
                "Copy the records from the counter again; if they still fail, stop and tell support.",
            )
            .code("PS-05")
            .detail(e.to_string()),
            Self::BadEvent { seq, error } => stop(
                "A record could not be read",
                format!("A record of the counter could not be read as a sale event: it {}.", read_cause(error)),
                Who::Owner,
                "Copy the records from the counter again; if it repeats, stop and tell support.",
            )
            .code("PS-06")
            .detail(format!("record {seq}: {error}")),
            Self::UnknownSale { index } => stop(
                "Event for an unopened sale",
                "The counter has an event for a sale that was never opened.",
                Who::Owner,
                "Copy the records from the counter again; if it repeats, stop and tell support.",
            )
            .code("PS-06")
            .detail(format!("sale {index}")),
            Self::OpenedTwice { index } => stop(
                "Sale opened twice",
                "The counter opens the same sale twice.",
                Who::Owner,
                "Copy the records from the counter again; if it repeats, stop and tell support.",
            )
            .code("PS-06")
            .detail(format!("sale {index}")),
            Self::TooManySales => too_many_sales(),
        }
    }
}

/// What a blocker is, in a clause, whether the owner can act on it, and the
/// order it names.
fn blocker_view(b: &Blocker) -> (&'static str, bool, Option<&OrderId>) {
    match b {
        Blocker::OpenAlert(key) => (
            "an alert nobody has acknowledged",
            true,
            match &key.subject {
                Subject::Tender(t) => Some(&t.order),
                Subject::Sale(_) => None,
            },
        ),
        Blocker::SaleUnsettled { order, .. } => {
            ("an order still waiting for its sale to be final", false, Some(order))
        }
        Blocker::TooRecent { order } => ("an order that changed a moment ago", false, Some(order)),
    }
}

/// What the bound that was reached holds, in words.
fn full_things(what: &str) -> &'static str {
    match what {
        "tracked orders" => "orders",
        "matches" => "matched payments",
        "alerts" => "alerts",
        _ => "records",
    }
}

impl LedgerError {
    /// What the owner is told when a change to the till's records was
    /// refused. The records are unchanged. A refusal because they are full
    /// names the oldest thing in the way and quotes its order number.
    pub fn words(&self) -> Words {
        match self {
            Self::Settings(e) => e.words(),
            Self::Order(e) => e.words(),
            Self::TooManySales => too_many_sales(),
            Self::DuplicateSale(k) => stop(
                "A sale is listed twice",
                "Two records name the same sale of the same counter.",
                Who::Owner,
                "Copy the records from the counter again; if it repeats, stop and tell support.",
            )
            .code("PS-72")
            .detail(format!("sale {}", k.index)),
            Self::Full { what, oldest } => {
                let things = full_things(what);
                let heading = "Till records are full";
                match oldest.first() {
                    None => stop(
                        heading,
                        format!("The till's {things} are full, and nothing in particular is in the way."),
                        Who::Owner,
                        "Stop and tell support: the till records are full.",
                    )
                    .code("PS-72")
                    .detail(*what),
                    Some(b) => {
                        let (clause, owner_acts, order) = blocker_view(b);
                        let what_text = format!(
                            "The till's {things} are full, and the oldest thing in the way is {clause}."
                        );
                        let words = if owner_acts {
                            stop(
                                heading,
                                what_text,
                                Who::Owner,
                                "Acknowledge the open alerts, then try again.",
                            )
                        } else {
                            Words::new(
                                Tone::Info,
                                heading,
                                what_text,
                                Who::Nobody,
                                "Nothing to do. Room frees up by itself once those orders are final or old enough.",
                            )
                        }
                        .code("PS-72")
                        .detail(format!("{what}, {} things named", oldest.len()));
                        match order {
                            Some(o) => with_order(words, o),
                            None => words,
                        }
                    }
                }
            }
            Self::NotOpen => Words::new(
                Tone::Ended,
                "Alert already dealt with",
                "That alert is not open: it was acknowledged or has cleared.",
                Who::Nobody,
                "Nothing to do. Reload the alert list to see what is open.",
            )
            .code("PS-52"),
        }
    }
}

impl Match {
    /// What the owner is told about a match the Box found between a bitcoin
    /// payment at the till and a counter sale. The time is when the pair was
    /// first made.
    pub fn words(&self) -> Words {
        let (heading, what, next) = match (self.how, self.by_order) {
            (MatchHow::Reference, false) => (
                "Payment matched by number",
                "The Box matched a bitcoin payment at the till to a counter sale by the sale number the cashier typed.",
                "Nothing to do.",
            ),
            (MatchHow::Reference, true) => (
                "Matched by number, in turn",
                "The Box matched a payment to a sale by the number typed, but more than one fitted and they were paired in time order.",
                "Check the pairing at the till and the counter if the amounts matter.",
            ),
            (MatchHow::AmountAndTime, false) => (
                "Payment matched by amount",
                "The Box matched a bitcoin payment at the till to a counter sale by its amount and its time.",
                "Nothing to do.",
            ),
            (MatchHow::AmountAndTime, true) => (
                "Matched by amount, in turn",
                "The Box matched a payment to a sale by amount and time, but more than one fitted and they were paired in time order.",
                "Check the pairing at the till and the counter if the amounts matter.",
            ),
        };
        let tone = if self.by_order { Tone::Info } else { Tone::Good };
        let who = if self.by_order { Who::Owner } else { Who::Nobody };
        let next = if self.by_order {
            next.to_string()
        } else {
            "Nothing to do. It is recorded.".to_string()
        };
        Words::new(tone, heading, what, who, next)
            .at(self.matched_at)
            .code(match (self.by_order, self.how) {
                (true, _) => "PS-34",
                (false, MatchHow::Reference) => "PS-31",
                (false, MatchHow::AmountAndTime) => "PS-33",
            })
    }
}

impl PosItem {
    /// What the owner is told about one thing the Box reported: a match, an
    /// alert, an order it closed, or a sale that went unpaid after its order
    /// was closed. The till's order number is quoted.
    pub fn words(&self) -> Words {
        match self {
            Self::Match { tender, matched } => with_order(matched.words(), &tender.order),
            Self::Alert(alert) => alert.words(),
            Self::Closed { order, .. } => with_order(
                Words::new(
                    Tone::Good,
                    "Order closed at the till",
                    "The Box closed the order at the till for a sale the counter showed as paid.",
                    Who::Nobody,
                    "Nothing to do. The order is closed with the bitcoin payment.",
                )
                .code("BX-43"),
                order,
            ),
            Self::UnpaidAfterClose { order, .. } => with_order(
                stop(
                    "Unpaid after order closed",
                    "The Box closed the order at the till for this sale, and the counter has since reported the sale unpaid again.",
                    Who::Owner,
                    "Nothing was undone. Ask the cashier whether the goods were handed over, and collect or refund as needed.",
                )
                .code("BX-43"),
                order,
            ),
        }
    }
}

/// SC-12: the words of every state and refusal of this crate, with at least
/// one value for every variant and every variant inside it. Built by hand
/// so that a new variant is noticed.
pub fn all_words() -> Vec<(String, Words)> {
    use crate::alerts::{AlertKey, TenderKey};
    use crate::facts::SaleKey;
    use crate::model::Id;
    let mut bytes = [0u8; 32];
    for (b, v) in bytes.iter_mut().zip([
        0x79u8, 0xbe, 0x66, 0x7e, 0xf9, 0xdc, 0xbb, 0xac, 0x55, 0xa0, 0x62, 0x95, 0xce, 0x87, 0x0b,
        0x07, 0x02, 0x9b, 0xfc, 0xdb, 0x2d, 0xce, 0x28, 0xd9, 0x59, 0xf2, 0x81, 0x5b, 0x16, 0xf8,
        0x17, 0x98,
    ]) {
        *b = v;
    }
    let Ok(counter) = openbmt_core::identity::DeviceId::from_bytes(&bytes) else {
        return Vec::new();
    };
    let (Ok(order), Ok(tender_id)) = (Id::new("ord-1042"), Id::new("t-1")) else {
        return Vec::new();
    };
    let sale = SaleKey { counter, index: 4 };
    let tender = TenderKey {
        order: order.clone(),
        tender: tender_id,
    };
    let currency = openbmt_counter::Currency::new(*b"USD");
    let mut all: Vec<(String, Words)> = Vec::new();
    let mut add = |n: &str, w: Words| all.push((n.to_string(), w));

    for kind in ALL_KINDS {
        let subject = match kind {
            AlertKind::PaymentWithoutTender => Subject::Sale(sale),
            _ => Subject::Tender(tender.clone()),
        };
        let alert = Alert {
            key: AlertKey { kind, subject },
            sale: Some(sale),
            currency,
            tender_minor: 1_250,
            sale_minor: 1_000,
            at: 1_700_000_000,
            secs: 240,
            raised_at: 1_700_000_300,
        };
        add(&format!("Alert::{kind:?}"), alert.words());
    }
    for (n, e) in [
        ("Amount", SaleMismatch::Amount),
        ("Currency", SaleMismatch::Currency),
        ("OpenedAt", SaleMismatch::OpenedAt),
    ] {
        add(&format!("SaleMismatch::{n}"), e.words());
    }
    for (n, e) in [
        ("Unavailable", PosError::Unavailable),
        ("SignInRefused", PosError::SignInRefused),
        (
            "RateLimited",
            PosError::RateLimited {
                retry_after_secs: 30,
            },
        ),
        ("BadResponse", PosError::BadResponse("a sample")),
        ("Unsupported", PosError::Unsupported),
    ] {
        add(&format!("PosError::{n}"), e.words());
    }
    add("CloseRefused::SaleNotPaid", CloseRefused::SaleNotPaid.words());
    let sync: [(&str, SyncError<String>); 3] = [
        ("PageTooLarge", SyncError::PageTooLarge),
        ("NoProgress", SyncError::NoProgress),
        ("Persist", SyncError::Persist("disk full".to_string())),
    ];
    for (n, e) in sync {
        add(&format!("SyncError::{n}"), e.words());
    }
    for (n, e) in [
        ("TenderNames", SettingsError::TenderNames),
        ("Currencies", SettingsError::Currencies),
        ("Locations", SettingsError::Locations),
        ("Timing", SettingsError::Timing),
    ] {
        add(&format!("SettingsError::{n}"), e.words());
    }
    for (n, e) in [
        ("BadId", ModelError::BadId),
        ("BadText", ModelError::BadText),
        ("TooManyTenders", ModelError::TooManyTenders),
        ("DuplicateTender", ModelError::DuplicateTender),
        ("RefundTooLarge", ModelError::RefundTooLarge),
        ("CursorTooLong", ModelError::CursorTooLong),
    ] {
        add(&format!("ModelError::{n}"), e.words());
    }
    add("LedgerError::TooManySales", LedgerError::TooManySales.words());
    add(
        "LedgerError::DuplicateSale",
        LedgerError::DuplicateSale(sale).words(),
    );
    add("LedgerError::NotOpen", LedgerError::NotOpen.words());
    let alert_key = AlertKey {
        kind: AlertKind::BitcoinTip,
        subject: Subject::Tender(tender.clone()),
    };
    let fulls = [
        (
            "tracked orders",
            vec![Blocker::OpenAlert(alert_key)],
            "tracked orders/OpenAlert",
        ),
        (
            "matches",
            vec![Blocker::SaleUnsettled {
                order: order.clone(),
                sale,
            }],
            "matches/SaleUnsettled",
        ),
        (
            "alerts",
            vec![Blocker::TooRecent {
                order: order.clone(),
            }],
            "alerts/TooRecent",
        ),
        ("alerts", Vec::new(), "alerts/none"),
        (
            "something else",
            vec![Blocker::OpenAlert(AlertKey {
                kind: AlertKind::AmountDiffers,
                subject: Subject::Sale(sale),
            })],
            "other/OpenAlert on a sale",
        ),
    ];
    for (what, oldest, n) in fulls {
        add(
            &format!("LedgerError::Full/{n}"),
            LedgerError::Full { what, oldest }.words(),
        );
    }
    let chains = [
        ChainError::UnknownAuthor { seq: 3 },
        ChainError::BadSignature { seq: 3 },
        ChainError::SeqMismatch {
            expected: 4,
            found: 6,
        },
        ChainError::PrevMismatch { seq: 3 },
        ChainError::BadCheckIn { seq: 3 },
        ChainError::Truncated { have: 3, need: 5 },
        ChainError::Diverged { seq: 3 },
        ChainError::BodyTooLarge,
        ChainError::NotACheckIn,
    ];
    for e in chains {
        add(
            &format!("FactsError::Chain/{}", chain_name(&e)),
            FactsError::Chain(e).words(),
        );
    }
    let decodes = [
        DecodeError::Truncated,
        DecodeError::TrailingBytes,
        DecodeError::BadMagic,
        DecodeError::UnsupportedVersion(9),
        DecodeError::TooLarge,
        DecodeError::NonCanonical("a sample"),
        DecodeError::InvalidKey,
        DecodeError::UnknownKind(77),
    ];
    for error in decodes {
        add(
            &format!("FactsError::BadEvent/{}", decode_name(&error)),
            FactsError::BadEvent { seq: 7, error }.words(),
        );
    }
    add(
        "FactsError::UnknownSale",
        FactsError::UnknownSale { index: 4 }.words(),
    );
    add(
        "FactsError::OpenedTwice",
        FactsError::OpenedTwice { index: 4 }.words(),
    );
    for (n, matched) in [
        (
            "Match/Reference",
            Match {
                sale,
                how: MatchHow::Reference,
                by_order: false,
                matched_at: 1_700_000_100,
            },
        ),
        (
            "Match/Reference in turn",
            Match {
                sale,
                how: MatchHow::Reference,
                by_order: true,
                matched_at: 1_700_000_100,
            },
        ),
        (
            "Match/AmountAndTime",
            Match {
                sale,
                how: MatchHow::AmountAndTime,
                by_order: false,
                matched_at: 1_700_000_100,
            },
        ),
        (
            "Match/AmountAndTime in turn",
            Match {
                sale,
                how: MatchHow::AmountAndTime,
                by_order: true,
                matched_at: 1_700_000_100,
            },
        ),
    ] {
        add(
            &format!("PosItem::{n}"),
            PosItem::Match {
                tender: tender.clone(),
                matched,
            }
            .words(),
        );
    }
    add(
        "PosItem::Closed",
        PosItem::Closed {
            sale,
            order: order.clone(),
        }
        .words(),
    );
    add(
        "PosItem::UnpaidAfterClose",
        PosItem::UnpaidAfterClose { sale, order }.words(),
    );
    all
}

/// Every kind of alert, for the list of words.
const ALL_KINDS: [AlertKind; 9] = [
    AlertKind::TenderWithoutPayment,
    AlertKind::ClosedBeforePaid,
    AlertKind::PaymentWithoutTender,
    AlertKind::AmountDiffers,
    AlertKind::TenderOnUnpaidSale,
    AlertKind::VoidedAfterPayment,
    AlertKind::DuplicateReference,
    AlertKind::BitcoinTip,
    AlertKind::UnknownCurrency,
];

fn chain_name(e: &ChainError) -> &'static str {
    match e {
        ChainError::UnknownAuthor { .. } => "UnknownAuthor",
        ChainError::BadSignature { .. } => "BadSignature",
        ChainError::SeqMismatch { .. } => "SeqMismatch",
        ChainError::PrevMismatch { .. } => "PrevMismatch",
        ChainError::BadCheckIn { .. } => "BadCheckIn",
        ChainError::Truncated { .. } => "Truncated",
        ChainError::Diverged { .. } => "Diverged",
        ChainError::BodyTooLarge => "BodyTooLarge",
        ChainError::NotACheckIn => "NotACheckIn",
    }
}

fn decode_name(e: &DecodeError) -> &'static str {
    match e {
        DecodeError::Truncated => "Truncated",
        DecodeError::TrailingBytes => "TrailingBytes",
        DecodeError::BadMagic => "BadMagic",
        DecodeError::UnsupportedVersion(_) => "UnsupportedVersion",
        DecodeError::TooLarge => "TooLarge",
        DecodeError::NonCanonical(_) => "NonCanonical",
        DecodeError::InvalidKey => "InvalidKey",
        DecodeError::UnknownKind(_) => "UnknownKind",
    }
}

/// SC-12: the rows of this crate's words in the one table of
/// `docs/screens/owner.md`, one line each, in the order of [`all_words`].
pub fn screens_markdown() -> String {
    let cell = |t: &str| t.replace('|', "/");
    let mut out = String::new();
    for (name, w) in all_words() {
        out.push_str(&format!(
            "| pos: {name} | {} | {} | {} | {:?} | {} |\n",
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
