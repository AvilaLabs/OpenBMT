//! What the cashier is told about a sale from the till (CR-271), under
//! `docs/SCREEN-RULES.md`.
//!

use std::fmt::Display;

use openbmt_core::say::{Tone, Who, Words};

use crate::refusal::TillStartRefused;
use crate::till::TillDropped;

/// Words of this section for the cashier.
fn said(
    tone: Tone,
    heading: &'static str,
    what: &'static str,
    next: &'static str,
    code: &'static str,
) -> Words {
    Words::new(tone, heading, what, Who::Cashier, next).code(code)
}

/// What every dropped request tells the cashier to do instead.
const TYPE_IT: &str = "Ask the till to send it again, or type the sale on this counter.";

/// CR-267: the card on the home screen that shows the oldest request.
pub fn till_card_words() -> Words {
    said(
        Tone::Info,
        "From the till",
        "The till asks this counter to take a payment.",
        "Tap Start when the customer is ready to pay, or Not now.",
        "CR-267",
    )
}

/// CR-269: the line on a started sale that the till cancelled.
pub fn till_cancelled_words() -> Words {
    said(
        Tone::Info,
        "The till cancelled this sale",
        "The till no longer asks for this payment. A payment that arrives anyway is counted.",
        "End the sale here if the customer is not paying.",
        "CR-269",
    )
}

/// CR-266: what a request that was dropped says, for a log or a screen that
/// shows it. Nothing was sent.
pub fn till_dropped_words(dropped: TillDropped) -> Words {
    let (heading, what, next) = match dropped {
        TillDropped::OtherCurrency => (
            "Till request: wrong currency",
            "The till asked for an amount in a currency that is not the shop's.",
            "Check the currency of the till's payment method, or type the sale on this counter.",
        ),
        TillDropped::BadAmount => (
            "Till request: amount refused",
            "The till asked for nothing, or for more than this counter takes in one sale.",
            TYPE_IT,
        ),
        TillDropped::BadReference => (
            "Till request: bad reference",
            "The till's reference for the sale is too long or holds characters the counter cannot show.",
            TYPE_IT,
        ),
        TillDropped::Lapsed => (
            "Till request came too late",
            "The request reached this counter after the till's time for it ran out.",
            TYPE_IT,
        ),
        TillDropped::Full => (
            "Too many till requests",
            "Eight requests from the till are already waiting on this counter.",
            "Start the waiting ones or let them run out, then ask the till again.",
        ),
        TillDropped::Known => (
            "Till request already here",
            "The till asked again for a sale this counter already has.",
            "Nothing to do: the sale on this counter goes on.",
        ),
    };
    said(Tone::Ended, heading, what, next, dropped.rule())
}

/// CR-268: what a start that did not open a sale says. A sale refused for a
/// reason of a sale typed at the counter says what that reason says.
pub fn till_start_words<E: Display>(refused: &TillStartRefused<E>) -> Words {
    match refused {
        TillStartRefused::NotWaiting => said(
            Tone::Ended,
            "Till request no longer here",
            "The till cancelled this request, or its time ran out, before Start.",
            TYPE_IT,
            "CR-266",
        ),
        TillStartRefused::Open(e) => e.words(),
    }
}

/// Every placeholder of this section with its name, for the list of every
/// word (CR-271).
pub fn all_till_words() -> Vec<(String, Words)> {
    let mut all = vec![
        ("Till::card".to_string(), till_card_words()),
        ("Till::cancelled".to_string(), till_cancelled_words()),
    ];
    for (name, d) in [
        ("OtherCurrency", TillDropped::OtherCurrency),
        ("BadAmount", TillDropped::BadAmount),
        ("BadReference", TillDropped::BadReference),
        ("Lapsed", TillDropped::Lapsed),
        ("Full", TillDropped::Full),
        ("Known", TillDropped::Known),
    ] {
        all.push((format!("TillDropped::{name}"), till_dropped_words(d)));
    }
    all.push((
        "TillStartRefused::NotWaiting".to_string(),
        till_start_words::<String>(&TillStartRefused::NotWaiting),
    ));
    all
}
