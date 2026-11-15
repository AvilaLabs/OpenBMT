//! The library's words for a screen, as the page needs them.

use openbmt_core::say::{Tone, Who, Words};

use crate::json::Obj;

/// The tone as the apps' word files name it.
pub fn tone_text(tone: Tone) -> &'static str {
    match tone {
        Tone::Info => "info",
        Tone::Good => "good",
        Tone::Stop => "stop",
        Tone::Ended => "ended",
    }
}

/// Who does the step, as the apps' word files say it.
pub fn who_text(who: Who) -> &'static str {
    match who {
        Who::Nobody => "nobody",
        Who::Customer => "the customer",
        Who::Cashier => "the cashier",
        Who::Manager => "a manager",
        Who::Owner => "the owner",
        Who::KeyHolder => "the key holder",
        Who::BoxKeeper => "the Box keeper",
    }
}

/// The words as an object: tone, heading, what, next, who and the rule's code.
pub fn words_json(w: &Words) -> Obj {
    Obj::new()
        .str("tone", tone_text(w.tone))
        .str("heading", &w.heading)
        .str("what", &w.what)
        .str("next", &w.next)
        .str("who", who_text(w.who))
        .opt_str("code", w.code)
}
