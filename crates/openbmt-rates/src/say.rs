//! What a person is told about a price that cannot be used
//! (`docs/SCREEN-RULES.md`, SC-01 to SC-12).
//!
//! A refusal to make a rate ([`RatesRefused`]) is read by the cashier, who
//! cannot fix it: the sentences say to take another kind of payment and
//! when to tell the owner. A refusal of one source's reply
//! ([`ParseRefused`]) is a note for the owner: the app asks again by
//! itself, so the sale is not stopped by it. The rule numbers and the
//! numbers of the prices are in the details only.

use openbmt_core::say::{Tone, Who, Words};

use crate::error::{ParseRefused, PriceRefused, RatesRefused};

const ASKS_AGAIN: &str = "Nothing to do. The app asks again soon. If it keeps happening, report it to the OpenBMT project.";

fn note(heading: &str, what: &str, code: &'static str) -> Words {
    Words::new(Tone::Info, heading, what, Who::Nobody, ASKS_AGAIN).code(code)
}

impl PriceRefused {
    /// What the owner is told when one source's price text is refused.
    pub fn words(&self) -> Words {
        let (heading, what) = match self {
            Self::Empty => (
                "Price missing",
                "A price source sent an empty price, so it was not used.",
            ),
            Self::Sign => (
                "Price has a sign",
                "A price source sent a price with a plus or minus sign, so it was not used.",
            ),
            Self::Exponent => (
                "Price in odd writing",
                "A price source sent a price written with an exponent, so it was not used.",
            ),
            Self::BadCharacter => (
                "Price has odd characters",
                "A price source sent a price with more than digits and a dot, so it was not used.",
            ),
            Self::TwoDots => (
                "Price has two dots",
                "A price source sent a price with more than one dot, so it was not used.",
            ),
            Self::BadShape => (
                "Price badly written",
                "A price source sent a price with a dot that has no digit beside it, so it was not used.",
            ),
            Self::LeadingZero => (
                "Price has extra zeros",
                "A price source sent a price that starts with a zero it should not have, so it was not used.",
            ),
            Self::TooManyDecimals => (
                "Price too exact",
                "A price source sent a price with more than eight digits after the dot, so it was not used.",
            ),
            Self::Zero => (
                "Price is zero",
                "A price source sent a price of nothing, so it was not used.",
            ),
            Self::OutOfBand => (
                "Price out of range",
                "A price source sent a price far above or far below what this app believes, so it was not used.",
            ),
        };
        note(heading, what, self.rule()).detail(self.to_string())
    }
}

impl ParseRefused {
    /// What the owner is told when one source's reply is refused.
    pub fn words(&self) -> Words {
        let (heading, what) = match self {
            Self::TooLong => (
                "Price reply too long",
                "A price source sent a reply longer than this app reads, so it was not used.",
            ),
            Self::TooDeep => (
                "Price reply too tangled",
                "A price source sent a reply nested deeper than this app reads, so it was not used.",
            ),
            Self::NotUtf8 => (
                "Price reply unreadable",
                "A price source sent a reply that is not plain text, so it was not used.",
            ),
            Self::Malformed => (
                "Price reply not understood",
                "A price source sent a reply that is not written as expected, so it was not used.",
            ),
            Self::TrailingBytes => (
                "Price reply has extra bytes",
                "A price source sent more after the end of its reply, so it was not used.",
            ),
            Self::DuplicateKey => (
                "Price reply repeats a field",
                "A price source named one field twice in its reply, so it was not used.",
            ),
            Self::MissingField => (
                "Price reply lacks the price",
                "A price source sent a reply without the field that holds the price, so it was not used.",
            ),
            Self::WrongShape => (
                "Price reply in a new form",
                "A price source sent a reply in a form this app does not know, so it was not used.",
            ),
            Self::WrongCurrency => (
                "Price in another currency",
                "A price source sent a price in a currency other than the shop's, so it was not used.",
            ),
            Self::SourceError => (
                "Price source reports a fault",
                "A price source said that it could not give a price, so it was not used.",
            ),
            Self::Price(p) => return p.words(),
        };
        note(heading, what, self.rule()).detail(self.to_string())
    }
}

impl RatesRefused {
    /// What the cashier is told when no rate can be made. The counter does
    /// not offer Bitcoin until a rate can.
    pub fn words(&self) -> Words {
        let (heading, what, next) = match self {
            Self::TooOld => (
                "Price out of date",
                "The price is not available right now: the last prices came in too long ago.",
                "Take another kind of payment. Tell the owner if this lasts.",
            ),
            Self::NotEnoughSources { .. } => (
                "Price not available",
                "The price is not available right now: fewer than two price sources have answered.",
                "Take another kind of payment. Try Bitcoin again in a minute.",
            ),
            Self::PricesDisagree { .. } => (
                "Prices disagree",
                "The price is not available right now: the price sources do not agree with each other.",
                "Take another kind of payment. Tell the owner if this lasts.",
            ),
            Self::MixedCurrencies => (
                "Prices in two currencies",
                "The price is not available right now: the price sources gave prices in different currencies.",
                "Take another kind of payment. Tell the owner: the price sources are set up wrongly.",
            ),
        };
        Words::new(Tone::Stop, heading, what, Who::Cashier, next)
            .code(self.rule())
            .detail(self.to_string())
    }
}

/// SC-12: the words of every refusal this crate can give, at least one
/// value for each variant and for each variant inside one.
pub fn all_words() -> Vec<(String, Words)> {
    let mut all: Vec<(String, Words)> = Vec::new();
    for p in ALL_PRICE {
        all.push((format!("PriceRefused::{p:?}"), p.words()));
    }
    for p in ALL_PARSE {
        all.push((format!("ParseRefused::{p:?}"), p.words()));
    }
    for r in ALL_RATES {
        all.push((format!("RatesRefused::{}", rates_name(&r)), r.words()));
    }
    all
}

/// Every price refusal.
pub const ALL_PRICE: [PriceRefused; 10] = [
    PriceRefused::Empty,
    PriceRefused::Sign,
    PriceRefused::Exponent,
    PriceRefused::BadCharacter,
    PriceRefused::TwoDots,
    PriceRefused::BadShape,
    PriceRefused::LeadingZero,
    PriceRefused::TooManyDecimals,
    PriceRefused::Zero,
    PriceRefused::OutOfBand,
];

/// Every reply refusal that is not about the price text.
pub const ALL_PARSE: [ParseRefused; 10] = [
    ParseRefused::TooLong,
    ParseRefused::TooDeep,
    ParseRefused::NotUtf8,
    ParseRefused::Malformed,
    ParseRefused::TrailingBytes,
    ParseRefused::DuplicateKey,
    ParseRefused::MissingField,
    ParseRefused::WrongShape,
    ParseRefused::WrongCurrency,
    ParseRefused::SourceError,
];

/// One value of every refusal to make a rate.
pub const ALL_RATES: [RatesRefused; 4] = [
    RatesRefused::TooOld,
    RatesRefused::NotEnoughSources { fresh: 1 },
    RatesRefused::PricesDisagree {
        lowest: 7_000_000,
        highest: 8_000_000,
    },
    RatesRefused::MixedCurrencies,
];

fn rates_name(r: &RatesRefused) -> &'static str {
    match r {
        RatesRefused::TooOld => "TooOld",
        RatesRefused::NotEnoughSources { .. } => "NotEnoughSources",
        RatesRefused::PricesDisagree { .. } => "PricesDisagree",
        RatesRefused::MixedCurrencies => "MixedCurrencies",
    }
}

/// SC-12: the list of words as the table of `docs/screens/rates.md`.
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
        "# Rates: what a person is told\n\n\
         Made by `screens_markdown` in `crates/openbmt-rates/src/say.rs`. Do not\n\
         edit it by hand: run the test `sc_12_the_list_is_on_disk` with the\n\
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
