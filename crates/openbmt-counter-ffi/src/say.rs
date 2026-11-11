//! What the person at the counter or the owner is told when a call of this
//! layer fails (`docs/SCREEN-RULES.md`, SC-01 to SC-12), and the numbers of
//! the words in the response frames.
//!
//! The words of the counter's own states and refusals come from the crates
//! that make them (`openbmt_counter::say`, `openbmt_counter_node::say`,
//! `openbmt_watch::say`) and travel with the result or the error. This file
//! holds only the words of the errors that this layer makes: a request that
//! is not valid, a handle that does not exist, a fault. [`words_for`] gives
//! the words of a rule string; a string that has none here gets the words of
//! its kind.

use openbmt_core::say::{Tone, Who, Words};

use crate::error::Kind;

/// The number of a tone in the response frame.
pub(crate) fn tone_byte(tone: Tone) -> u8 {
    match tone {
        Tone::Info => 0,
        Tone::Good => 1,
        Tone::Stop => 2,
        Tone::Ended => 3,
    }
}

/// The number of a person in the response frame.
pub(crate) fn who_byte(who: Who) -> u8 {
    match who {
        Who::Nobody => 0,
        Who::Customer => 1,
        Who::Cashier => 2,
        Who::Manager => 3,
        Who::Owner => 4,
        Who::KeyHolder => 5,
        Who::BoxKeeper => 6,
    }
}

/// Every tone with its name and number, for the protocol description and
/// the Kotlin constants.
pub(crate) const TONES: [(Tone, &str, u8); 4] = [
    (Tone::Info, "Info", 0),
    (Tone::Good, "Good", 1),
    (Tone::Stop, "Stop", 2),
    (Tone::Ended, "Ended", 3),
];

/// Every person with its name and number.
pub(crate) const WHOS: [(Who, &str, u8); 7] = [
    (Who::Nobody, "Nobody", 0),
    (Who::Customer, "Customer", 1),
    (Who::Cashier, "Cashier", 2),
    (Who::Manager, "Manager", 3),
    (Who::Owner, "Owner", 4),
    (Who::KeyHolder, "KeyHolder", 5),
    (Who::BoxKeeper, "BoxKeeper", 6),
];

/// What the app is told to do when its own call was wrong.
const BUG: &str =
    "Try again, and if it happens again, stop and tell the owner in person; see Details.";

/// One rule string of this layer: the kind, the heading, what happened, who
/// does the next step and what they do.
type Entry = (
    &'static str,
    Kind,
    &'static str,
    &'static str,
    Who,
    &'static str,
);

/// The rule strings this layer makes, each with its own words.
const RULES: &[Entry] = &[
    (
        "request-too-long",
        Kind::Refused,
        "Request too long",
        "The counter app sent the counter a request that is longer than it takes, so nothing was done.",
        Who::Owner,
        BUG,
    ),
    (
        "request-malformed",
        Kind::Refused,
        "Request not readable",
        "The counter app sent the counter a request that is cut short or not in the shape the counter takes, so nothing was done.",
        Who::Owner,
        BUG,
    ),
    (
        "request-magic",
        Kind::Refused,
        "Request not recognised",
        "The counter app sent the counter something that is not a request, so nothing was done.",
        Who::Owner,
        BUG,
    ),
    (
        "request-version",
        Kind::Refused,
        "Request from another version",
        "The counter app and the counter library are not of the same version, so nothing was done.",
        Who::Owner,
        "Install the counter app again so that its parts match.",
    ),
    (
        "request-null",
        Kind::Refused,
        "No request",
        "The counter app called the counter with no request, so nothing was done.",
        Who::Owner,
        BUG,
    ),
    (
        "request-length",
        Kind::Refused,
        "Request has no length",
        "The counter could not tell how long the request from the counter app is, so nothing was done.",
        Who::Owner,
        BUG,
    ),
    (
        "request-read",
        Kind::Refused,
        "Request not read",
        "The counter could not read the request from the counter app, so nothing was done.",
        Who::Owner,
        BUG,
    ),
    (
        "op-unknown",
        Kind::Refused,
        "Request not known",
        "The counter app asked the counter for something it does not do, so nothing was done.",
        Who::Owner,
        "Install the counter app again so that its parts match.",
    ),
    (
        "handle-unknown",
        Kind::Refused,
        "Counter not running",
        "The counter app asked about a counter that is not running, so nothing was done.",
        Who::Cashier,
        "Start the counter again, then repeat the step.",
    ),
    (
        "handle-unexpected",
        Kind::Refused,
        "Request names a counter",
        "The counter app named a counter in a request that works on none, so nothing was done.",
        Who::Owner,
        BUG,
    ),
    (
        "start-dir",
        Kind::Refused,
        "No data folder",
        "The counter was not given a data folder to keep its files in, so it did not start.",
        Who::Owner,
        "Check the counter app's settings for its folder, then start it again.",
    ),
    (
        "start-proxy",
        Kind::Refused,
        "Privacy proxy not usable",
        "The address of the privacy proxy that the counter was given is not an address, so it did not start.",
        Who::Owner,
        "Check the proxy address in the counter app's settings, then start it again.",
    ),
    (
        "CR-87",
        Kind::Refused,
        "Tor needs its proxy",
        "The shop's settings have Tor on, but the counter was started without the privacy proxy, so it did not start and sent nothing.",
        Who::Manager,
        "Open Orbot and turn it on, then start the counter again.",
    ),
    (
        "start-peer",
        Kind::Refused,
        "Network peer not usable",
        "A network address that the counter was given to connect to is not an address, so it did not start.",
        Who::Owner,
        "Check the addresses in the counter app's settings, then start it again.",
    ),
    (
        "sale-currency",
        Kind::Refused,
        "Currency not usable",
        "The currency of the sale is not three capital letters, so no sale was opened.",
        Who::Cashier,
        "Check the shop's currency in the counter app's settings.",
    ),
    (
        "sale-rate",
        Kind::Refused,
        "Rate not usable",
        "The bitcoin price that the app gave the counter is zero, so nothing was done.",
        Who::Cashier,
        "Try again once the app has a price. If it stays, take another kind of payment.",
    ),
    (
        "sale-source",
        Kind::Refused,
        "Price source not usable",
        "The name of where the bitcoin price came from is empty or too long, so nothing was done.",
        Who::Owner,
        BUG,
    ),
    (
        "rate-source",
        Kind::Refused,
        "Price source not known",
        "The counter app named a price source that is not in the counter's list, so nothing was done.",
        Who::Owner,
        BUG,
    ),
    (
        "rate-samples",
        Kind::Refused,
        "Too many prices at once",
        "The counter app handed over more prices than the counter takes at once, so nothing was done.",
        Who::Owner,
        BUG,
    ),
    (
        "command-bytes",
        Kind::Refused,
        "Command not readable",
        "What was sent as an owner's command is not a command, so nothing was changed.",
        Who::Owner,
        "Nothing to do. The owner's phone sends the command again.",
    ),
    (
        "batch-bytes",
        Kind::Refused,
        "Batch not readable",
        "What was sent as a batch of addresses is not a batch, so nothing was changed.",
        Who::Owner,
        "Nothing to do. The owner's phone sends the batch again.",
    ),
    (
        "batch-refused",
        Kind::Refused,
        "Batch not accepted",
        "The batch of payment addresses was checked and refused, so nothing was changed.",
        Who::Owner,
        "Send a new batch from the owner's phone. See Details.",
    ),
    (
        "pay-address",
        Kind::Refused,
        "No payment address",
        "The sale's address could not be written as a payment address, so no code was made.",
        Who::Cashier,
        "Take another kind of payment and tell the owner. See Details.",
    ),
    (
        "ln-status",
        Kind::Refused,
        "Node answer not usable",
        "The answer of the Lightning node came with an HTTP status that is not a number the counter takes, so it was thrown away.",
        Who::Owner,
        BUG,
    ),
    (
        "pay-code",
        Kind::Refused,
        "Payment code not made",
        "The payment code for the customer to scan could not be made, so it is not shown.",
        Who::Cashier,
        "Read the address out to the customer, or take another kind of payment. See Details.",
    ),
    (
        "node-limit",
        Kind::State,
        "Counter already open",
        "The most counters this app holds are already running, so another was not started.",
        Who::Owner,
        "Stop one of them, then start the counter again.",
    ),
    (
        "link-none",
        Kind::State,
        "No link to the Box",
        "This counter was paired without a Box, so it has no link to the owner and nothing was done.",
        Who::Owner,
        "Nothing to do now. When the shop has a Box, the owner's phone gives the counter its code.",
    ),
    (
        "handles-used",
        Kind::State,
        "Out of numbers",
        "The counter app has used every number it can give to a running counter, so nothing was started.",
        Who::Owner,
        "Close the counter app and open it again.",
    ),
    (
        "internal",
        Kind::Internal,
        "Counter fault",
        "The counter library failed in a way it did not expect, so the call was not finished.",
        Who::Owner,
        BUG,
    ),
    (
        "poisoned",
        Kind::Internal,
        "Counter needs a restart",
        "An earlier call failed in a way the counter did not expect, so it takes no more calls until it is restarted.",
        Who::Cashier,
        "Stop the counter and start it again. If it happens again, take another kind of payment and tell the owner.",
    ),
];

/// Every rule string of this layer that has words of its own, for the
/// checks of the words and of the sources.
#[cfg(test)]
pub(crate) fn rule_strings() -> impl Iterator<Item = &'static str> {
    RULES.iter().map(|e| e.0)
}

/// What a person is told for an error of this layer: the words of the rule
/// string, or, for a string that has none, the words of its kind.
pub fn words_for(kind: Kind, rule: &str) -> Words {
    if let Some((rule, _, heading, what, who, next)) =
        RULES.iter().find(|e| e.0 == rule && e.1 == kind).copied()
    {
        let tone = match kind {
            Kind::State => Tone::Info,
            Kind::Refused | Kind::Store | Kind::Internal => Tone::Stop,
        };
        return Words::new(tone, heading, what, who, next).detail(rule);
    }
    let (heading, what, next) = match kind {
        Kind::Refused => (
            "Request not accepted",
            "The counter refused what the app asked, and the reason is not known to this screen.",
            "Try again. If it is refused again, tell the owner in person; see Details.",
        ),
        Kind::Store => (
            "Counter cannot save",
            "The counter could not write to its data folder, so what was asked was not done.",
            "Try once more. If it fails again, take another kind of payment and tell the owner. See Details.",
        ),
        Kind::State => (
            "Not possible now",
            "What the app asked cannot be done in the state the counter is in.",
            "Try again in a moment. If it is the same again, tell the owner in person; see Details.",
        ),
        Kind::Internal => (
            "Counter fault",
            "The counter library failed in a way it did not expect, so the call was not finished.",
            BUG,
        ),
    };
    let who = if kind == Kind::Store {
        Who::Cashier
    } else {
        Who::Owner
    };
    Words::new(Tone::Stop, heading, what, who, next).detail(rule)
}

/// Every words value this layer can make, for the checks of SC-12.
pub fn all_words() -> Vec<(String, Words)> {
    let mut out: Vec<(String, Words)> = RULES
        .iter()
        .map(|e| (e.0.to_string(), words_for(e.1, e.0)))
        .collect();
    // The words of a kind are those of `internal` for an internal fault.
    for (name, kind) in [
        ("kind-refused", Kind::Refused),
        ("kind-store", Kind::Store),
        ("kind-state", Kind::State),
    ] {
        out.push((name.to_string(), words_for(kind, "a-rule-without-words")));
    }
    out
}
