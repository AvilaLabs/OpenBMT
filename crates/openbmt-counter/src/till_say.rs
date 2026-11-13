//! What the cashier is told about a sale from the till (CR-271), under
//! `docs/SCREEN-RULES.md`.
//!
//! None of the words is written yet. Each state and refusal carries the
//! placeholder that names its rule, `WORDS_NEEDED_CR_<rule>_<what>`, in the
//! heading and in both sentences, so that it shows its own name wherever the
//! words would be.

use std::fmt::Display;

use openbmt_core::say::{Tone, Who, Words};

use crate::refusal::TillStartRefused;
use crate::till::TillDropped;

/// Words that are still to be written: the placeholder stands in the heading
/// and both sentences.
fn needed(placeholder: &'static str, tone: Tone, who: Who, code: &'static str) -> Words {
    Words::new(tone, placeholder, placeholder, who, placeholder).code(code)
}

/// CR-267: the card on the home screen that shows the oldest request.
pub fn till_card_words() -> Words {
    needed(
        "WORDS_NEEDED_CR_267_FROM_THE_TILL",
        Tone::Info,
        Who::Cashier,
        "CR-267",
    )
}

/// CR-269: the line on a started sale that the till cancelled.
pub fn till_cancelled_words() -> Words {
    needed(
        "WORDS_NEEDED_CR_269_CANCELLED",
        Tone::Info,
        Who::Cashier,
        "CR-269",
    )
}

/// CR-266: what a request that was dropped says, for a log or a screen that
/// shows it. Nothing was sent.
pub fn till_dropped_words(dropped: TillDropped) -> Words {
    let placeholder = match dropped {
        TillDropped::OtherCurrency => "WORDS_NEEDED_CR_266_OTHER_CURRENCY",
        TillDropped::BadAmount => "WORDS_NEEDED_CR_266_BAD_AMOUNT",
        TillDropped::BadReference => "WORDS_NEEDED_CR_266_BAD_REFERENCE",
        TillDropped::Lapsed => "WORDS_NEEDED_CR_266_LAPSED",
        TillDropped::Full => "WORDS_NEEDED_CR_266_FULL",
        TillDropped::Known => "WORDS_NEEDED_CR_266_KNOWN",
    };
    needed(placeholder, Tone::Ended, Who::Cashier, dropped.rule())
}

/// CR-268: what a start that did not open a sale says. A sale refused for a
/// reason of a sale typed at the counter says what that reason says.
pub fn till_start_words<E: Display>(refused: &TillStartRefused<E>) -> Words {
    match refused {
        TillStartRefused::NotWaiting => needed(
            "WORDS_NEEDED_CR_266_NOT_WAITING",
            Tone::Ended,
            Who::Cashier,
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
