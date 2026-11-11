//! What a person is told when a till's reply cannot be read or a request
//! cannot be made (`docs/SCREEN-RULES.md`, SC-01 to SC-12).
//!
//! A refused reply is read by the owner, who cannot fix the till's system:
//! the sentences say that nothing was changed and that the app tries again
//! later. The rule number and the name of the field are in the details only.

use openbmt_core::say::{Quoted, Tone, Who, Words};

use crate::error::{ReadRefused, RequestRefused, Shown};

const TRY_AGAIN: &str = "Nothing was changed. Try again later; if it repeats, stop and report it to the OpenBMT project with the details.";

fn stop(heading: &str, what: &str, code: &'static str, detail: String) -> Words {
    Words::new(Tone::Stop, heading, what, Who::Owner, TRY_AGAIN)
        .code(code)
        .detail(detail)
}

impl ReadRefused {
    /// What the owner is told when the reply of a till is refused.
    pub fn words(&self) -> Words {
        let (heading, what) = match self {
            Self::TooLong => (
                "Till answer too big",
                "The till's system sent an answer bigger than this app reads, so none of it was used.",
            ),
            Self::TooDeep => (
                "Till answer too tangled",
                "The till's system sent an answer nested deeper than this app reads, so none of it was used.",
            ),
            Self::TooManyValues => (
                "Till answer too crowded",
                "The till's system sent an answer with more items than this app reads, so none of it was used.",
            ),
            Self::NotUtf8 => (
                "Till answer unreadable",
                "The till's system sent an answer that is not plain text, so none of it was used.",
            ),
            Self::Malformed => (
                "Till answer not understood",
                "The till's system sent an answer that is not written as expected, so none of it was used.",
            ),
            Self::TrailingBytes => (
                "Till answer has extra bytes",
                "The till's system sent more after the end of its answer, so none of it was used.",
            ),
            Self::DuplicateKey => (
                "Till answer repeats a field",
                "The till's system named one field twice in its answer, so none of it was used.",
            ),
            Self::ServiceError => (
                "Till system reports a fault",
                "The till's system said that it could not give the orders, so none were used.",
            ),
            Self::Missing(_) => (
                "Till answer lacks a field",
                "The till's system left out a field this app needs to read an order, so none were used.",
            ),
            Self::WrongType(_) => (
                "Till field in a new form",
                "The till's system sent a field in a form this app does not know, so none of the orders were used.",
            ),
            Self::UnknownValue(_) => (
                "Till sent an unknown value",
                "The till's system sent a state or a kind of payment this app does not know, so none of the orders were used.",
            ),
            Self::UnknownResult(_) => (
                "Till payment result unknown",
                "The till's system gave a Bitcoin payment a result this app does not know, so none of the orders were used.",
            ),
            Self::OutOfRange(_) => (
                "Till number out of range",
                "The till's system sent an amount or a number that is negative, too large or not whole, so none of the orders were used.",
            ),
            Self::BadTime(_) => (
                "Till time not understood",
                "The till's system sent a time that is not written as expected, so none of the orders were used.",
            ),
            Self::BadCurrency(_) => (
                "Till sent an odd currency",
                "The till's system sent a currency that is not written as three capital letters, so none of the orders were used.",
            ),
            Self::CurrencyMismatch => (
                "Till mixed two currencies",
                "The till's system sent a payment or a refund in another currency than its order, so none of the orders were used.",
            ),
            Self::BadId(_) => (
                "Till sent an unusable id",
                "The till's system sent an id that is empty, too long or has odd characters, so none of the orders were used.",
            ),
            Self::BadText(_) => (
                "Till sent unusable text",
                "The till's system sent a name or a place that is empty, too long or has odd characters, so none of the orders were used.",
            ),
            Self::TooManyOrders => (
                "Till sent too many orders",
                "The till's system sent more orders in one answer than this app takes, so none of them were used.",
            ),
            Self::TooManyTenders => (
                "Order has too many payments",
                "The till's system sent an order with more payments than this app takes, so none of the orders were used.",
            ),
            Self::DuplicateTender => (
                "Order repeats a payment",
                "The till's system sent an order that lists the same payment twice, so none of the orders were used.",
            ),
            Self::RefundTooLarge => (
                "Refund larger than payment",
                "The till's system sent refunds that add up to more than the payment, so none of the orders were used.",
            ),
            Self::RefundWithoutTender => (
                "Refund for unknown payment",
                "The till's system sent a refund for a payment its order does not hold, so none of the orders were used.",
            ),
            Self::CursorTooLong => (
                "Till place marker too long",
                "The marker the till's system gave for reading on is longer than this app keeps, so the orders were not used.",
            ),
        };
        let words = stop(heading, what, self.rule(), self.to_string());
        match self {
            Self::UnknownResult(r) => {
                words.quoted(Quoted::new("The till's system sent", r.as_str()))
            }
            _ => words,
        }
    }
}

impl RequestRefused {
    /// What the owner is told when a request to the till cannot be made.
    pub fn words(&self) -> Words {
        let (heading, what, next, who) = match self {
            Self::NoLocations => (
                "No till location set",
                "The shop's settings name no till location to read orders from.",
                "Add the till location in the settings, then try again.",
                Who::Owner,
            ),
            Self::BadLimit => (
                "Till page size not valid",
                "The app asked for a number of orders at a time that its till link does not allow.",
                "Nothing was changed. Stop and report it to the OpenBMT project with the details.",
                Who::Owner,
            ),
            Self::BadCursor => (
                "Till place marker not valid",
                "The place where reading of the till's orders goes on is not written as expected.",
                "Nothing was changed. Stop and report it to the OpenBMT project with the details.",
                Who::Owner,
            ),
            Self::BadMerchant => (
                "Till shop number not valid",
                "The shop number set for the till has characters it may not have, or is too long.",
                "Correct the shop number in the settings, then try again.",
                Who::Owner,
            ),
        };
        Words::new(Tone::Stop, heading, what, who, next)
            .code(self.rule())
            .detail(self.to_string())
    }
}

/// Every reply refusal, with a field name where it carries one.
pub const ALL_READ: [ReadRefused; 24] = [
    ReadRefused::TooLong,
    ReadRefused::TooDeep,
    ReadRefused::TooManyValues,
    ReadRefused::NotUtf8,
    ReadRefused::Malformed,
    ReadRefused::TrailingBytes,
    ReadRefused::DuplicateKey,
    ReadRefused::ServiceError,
    ReadRefused::Missing("orders[].id"),
    ReadRefused::WrongType("orders[].id"),
    ReadRefused::UnknownValue("orders[].state"),
    ReadRefused::UnknownResult(Shown::sample("SETTLED")),
    ReadRefused::OutOfRange("orders[].total_money"),
    ReadRefused::BadTime("orders[].created_at"),
    ReadRefused::BadCurrency("orders[].total_money"),
    ReadRefused::CurrencyMismatch,
    ReadRefused::BadId("orders[].id"),
    ReadRefused::BadText("orders[].location_id"),
    ReadRefused::TooManyOrders,
    ReadRefused::TooManyTenders,
    ReadRefused::DuplicateTender,
    ReadRefused::RefundTooLarge,
    ReadRefused::RefundWithoutTender,
    ReadRefused::CursorTooLong,
];

/// Every request refusal.
pub const ALL_REQUEST: [RequestRefused; 4] = [
    RequestRefused::NoLocations,
    RequestRefused::BadLimit,
    RequestRefused::BadCursor,
    RequestRefused::BadMerchant,
];

/// The name of a reply refusal in the list, without the field.
pub fn read_name(r: &ReadRefused) -> &'static str {
    match r {
        ReadRefused::TooLong => "TooLong",
        ReadRefused::TooDeep => "TooDeep",
        ReadRefused::TooManyValues => "TooManyValues",
        ReadRefused::NotUtf8 => "NotUtf8",
        ReadRefused::Malformed => "Malformed",
        ReadRefused::TrailingBytes => "TrailingBytes",
        ReadRefused::DuplicateKey => "DuplicateKey",
        ReadRefused::ServiceError => "ServiceError",
        ReadRefused::Missing(_) => "Missing",
        ReadRefused::WrongType(_) => "WrongType",
        ReadRefused::UnknownValue(_) => "UnknownValue",
        ReadRefused::UnknownResult(_) => "UnknownResult",
        ReadRefused::OutOfRange(_) => "OutOfRange",
        ReadRefused::BadTime(_) => "BadTime",
        ReadRefused::BadCurrency(_) => "BadCurrency",
        ReadRefused::CurrencyMismatch => "CurrencyMismatch",
        ReadRefused::BadId(_) => "BadId",
        ReadRefused::BadText(_) => "BadText",
        ReadRefused::TooManyOrders => "TooManyOrders",
        ReadRefused::TooManyTenders => "TooManyTenders",
        ReadRefused::DuplicateTender => "DuplicateTender",
        ReadRefused::RefundTooLarge => "RefundTooLarge",
        ReadRefused::RefundWithoutTender => "RefundWithoutTender",
        ReadRefused::CursorTooLong => "CursorTooLong",
    }
}

/// The name of a request refusal in the list.
pub fn request_name(r: &RequestRefused) -> &'static str {
    match r {
        RequestRefused::NoLocations => "NoLocations",
        RequestRefused::BadLimit => "BadLimit",
        RequestRefused::BadCursor => "BadCursor",
        RequestRefused::BadMerchant => "BadMerchant",
    }
}

/// SC-12: the words of every refusal this crate can give, at least one value
/// for each variant.
pub fn all_words() -> Vec<(String, Words)> {
    let mut all: Vec<(String, Words)> = Vec::new();
    for r in ALL_READ {
        all.push((format!("ReadRefused::{}", read_name(&r)), r.words()));
    }
    for r in ALL_REQUEST {
        all.push((format!("RequestRefused::{}", request_name(&r)), r.words()));
    }
    all
}

/// SC-12: the list of words as the table of `docs/screens/pos-readers.md`.
pub fn screens_markdown() -> String {
    let who = |w: Who| match w {
        Who::Nobody => "nobody",
        Who::Customer => "the customer",
        Who::Cashier => "the cashier",
        Who::Manager => "a manager",
        Who::Owner => "the owner",
        Who::KeyHolder => "the key holder",
        Who::BoxKeeper => "the Box keeper",
    };
    let cell = |text: &str| text.replace('|', "/");
    let mut out = String::from(
        "# POS readers: what a person is told\n\n\
         Made by `screens_markdown` in `crates/openbmt-pos-readers/src/say.rs`. Do\n\
         not edit it by hand: run the test `sc_12_the_list_is_on_disk` with the\n\
         environment variable `OPENBMT_WRITE_SCREENS=1` to write it again.\n\n\
         | State | Heading | What happened | What to do next | Who | Code |\n\
         |---|---|---|---|---|---|\n",
    );
    for (name, words) in all_words() {
        out.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} |\n",
            cell(&name),
            cell(&words.heading),
            cell(&words.what),
            cell(&words.next),
            who(words.who),
            words.code.unwrap_or("")
        ));
    }
    out
}
