//! What the owner is told about pairing a counter with the shop's own wallet
//! (`docs/OWNER-APP-RULES.md` OA-105 to OA-114, `docs/SCREEN-RULES.md`).
//!
//! The sentences are the ones in the approved words file for this part of
//! the app. A state or refusal that file does not cover carries the
//! placeholder constant that names the rule, `WORDS_NEEDED_OA_xxx`, until
//! its words are written (OA-114).

use openbmt_core::say::{Tone, Who, Words};

use crate::own_shop::OwnRefusal;
use crate::own_switch::{not_ready_words, status_words, SwitchStatus};
use crate::say::stop;

/// Words that are still to be written: the placeholder stands in the
/// heading and both sentences, so that it shows wherever the words would.
pub fn needed(placeholder: &'static str) -> Words {
    Words::new(
        Tone::Stop,
        placeholder,
        placeholder,
        Who::Owner,
        placeholder,
    )
}

/// Whether these words are a placeholder (OA-114).
pub fn is_placeholder(words: &Words) -> bool {
    words.heading.starts_with("WORDS_NEEDED_")
}

/// OA-108: the offer's 10 minutes are over.
pub fn offer_ended() -> Words {
    stop(
        "Code ended",
        "This code lasts 10 minutes and has ended. Nothing was paired.",
        Who::Owner,
        "Show a new code.",
    )
    .code("OA-108")
}

/// OA-110: the owner says the words differ: nothing was paired.
pub fn they_differ() -> Words {
    stop(
        "Pairing stopped",
        "The words did not match, so nothing was paired. Someone may have changed a code on the way.",
        Who::Owner,
        "Start again from Add a counter.",
    )
    .code("OA-110")
}

/// OA-112, LK-73: a counter has not taken the range it was sent for a day.
/// `name` is the counter's name, which is text from the owner's own
/// pairing and is cleaned before it is shown.
pub fn range_undelivered(name: &str) -> Words {
    let name = openbmt_core::say::clean_foreign(name);
    Words::new(
        Tone::Info,
        "Addresses not delivered",
        format!(
            "{name} has not taken its new addresses for a day. It can sell until its current ones run out."
        ),
        Who::Owner,
        format!("Open {name} and check that it is connected."),
    )
    .code("OA-112")
}

/// OA-105: the shop's pairing file could not be read or saved.
pub fn shop_file_failed() -> Words {
    stop(
        "Shop file not saved",
        "The phone could not read or save the file that keeps the shop's counters and addresses, so nothing was changed.",
        Who::Owner,
        "Try again. Do not clear the app's data: your money is in your wallet, not in this file.",
    )
    .code("OA-105")
}

/// OA-105: the file that keeps the wallet's description does not read.
pub fn wallet_not_readable() -> Words {
    stop(
        "Wallet not readable",
        "The file that keeps your wallet's description could not be read, so no counter can be added.",
        Who::Owner,
        "Do not clear the app's data. Your money is in your wallet, not in this file.",
    )
    .code("OA-105")
}

impl OwnRefusal {
    /// The rule behind the refusal, for the small print and for logs.
    pub fn rule(&self) -> &'static str {
        match self {
            Self::OfferEnded | Self::ClockWentBack | Self::NotThisShopsOffer => "OA-108",
            Self::NotARequest
            | Self::OtherOffer
            | Self::BadSignature
            | Self::OwnKey
            | Self::BadName => "OA-109",
            Self::AlreadyAdded | Self::Pairing(_) | Self::Settings(_) => "OA-110",
            Self::SwitchSameWallet => "SS-41",
            Self::OtherNewWallet => "SS-42",
            Self::Book(_) => "OA-106",
            Self::NoBundle | Self::TooManyWaiting => "OA-111",
            Self::BoxWaits => "OA-119",
            Self::UnknownCounter => "OA-112",
        }
    }

    /// What the owner is told (OA-109, OA-114).
    pub fn words(&self) -> Words {
        match self {
            Self::OfferEnded => offer_ended(),
            Self::ClockWentBack => stop(
                "Phone clock went back",
                "The phone's clock is now earlier than when the code was made, so the code cannot be checked.",
                Who::Owner,
                "Set the phone's date and time, then show a new code.",
            )
            .code("OA-108"),
            Self::NotThisShopsOffer => stop(
                "Code not from this shop",
                "The counter's code answers a code this phone did not make for this shop, so nothing was paired.",
                Who::Owner,
                "Start again from Add a counter on both.",
            )
            .code("OA-108"),
            Self::NotARequest => stop(
                "Not a counter's code",
                "This is not the code of a counter being paired.",
                Who::Owner,
                "Scan the code the counter shows after Same address.",
            )
            .code("OA-109"),
            Self::OtherOffer => stop(
                "Code for another pairing",
                "The counter's code was made for a different code from this phone.",
                Who::Owner,
                "Start again from Add a counter on both.",
            )
            .code("OA-109"),
            Self::BadSignature => stop(
                "Counter code not signed",
                "The counter's code is not signed by the counter it names. Nothing was paired.",
                Who::Owner,
                "Start again from Add a counter.",
            )
            .code("OA-109"),
            Self::OwnKey => stop(
                "Counter key refused",
                "The counter's key is this phone's own key, which a counter may not have.",
                Who::Owner,
                "Start again from Add a counter.",
            )
            .code("OA-109"),
            Self::BadName => stop(
                "Counter name not usable",
                "A counter's name is 1 to 32 characters with no line breaks, and this one is not, so nothing was paired.",
                Who::Owner,
                "Give the counter a shorter name, then start again from Add a counter.",
            )
            .code("OA-109"),
            Self::AlreadyAdded => stop(
                "Counter already added",
                "This counter is already one of the shop's, so it was not added again.",
                Who::Owner,
                "Nothing to do. To pair it again, remove it from the shop first.",
            )
            .code("OA-110"),
            Self::Pairing(_) => stop(
                "Pairing failed",
                "A last check of the counter's code did not pass, so nothing was paired.",
                Who::Owner,
                "Start again from Add a counter.",
            )
            .code("OA-110"),
            Self::Settings(r) => r.words(),
            Self::Book(crate::own_book::BookError::Full) => stop(
                "No addresses left",
                "The wallet has given every address it can make to counters, so no more can be given.",
                Who::Owner,
                "Connect a new wallet to add counters.",
            )
            .code("OA-106"),
            Self::Book(crate::own_book::BookError::TooManyDevices) => stop(
                "Too many counters",
                "The shop has 64 counters, the most it can have.",
                Who::Owner,
                "Remove a counter that is no longer used, then add this one.",
            )
            .code("OA-106"),
            Self::Book(crate::own_book::BookError::Corrupt(_)) => stop(
                "Address book damaged",
                "The record of which addresses each counter holds could not be read, so no counter can be added.",
                Who::Owner,
                "Do not clear the app's data. Your money is in your wallet, not in this file.",
            )
            .code("OA-106"),
            Self::NoBundle => stop(
                "No code waiting",
                "No counter is waiting for its code from this phone.",
                Who::Owner,
                "Nothing to do. Start from Add a counter to pair one.",
            )
            .code("OA-111"),
            Self::TooManyWaiting => stop(
                "Too many counters waiting",
                "16 counters are waiting for their codes, the most this phone keeps.",
                Who::Owner,
                "Finish pairing a waiting counter, then add this one.",
            )
            .code("OA-111"),
            Self::BoxWaits => stop(
                "A change waits for a counter",
                "A change of the shop's settings is waiting for a counter to confirm it, so the Box was not added.",
                Who::Owner,
                "Confirm the change on a counter, or wait until it is dropped, then connect the Box again.",
            )
            .code("OA-119"),
            Self::SwitchSameWallet => stop(
                "Already the counters' wallet",
                "The wallet given is the one the counters already pay into, so no switch was made.",
                Who::Owner,
                "Connect the wallet made offline instead.",
            )
            .code("SS-41"),
            Self::OtherNewWallet => stop(
                "Another move is planned",
                "A move to a different new wallet was already planned, so nothing was moved.",
                Who::Owner,
                "Connect the same new wallet the counters switched to.",
            )
            .code("SS-42"),
            Self::UnknownCounter => stop(
                "Counter not in the shop",
                "Addresses were asked for a counter this phone did not pair, so none were given.",
                Who::Owner,
                "Nothing to do. Only paired counters get addresses.",
            )
            .code("OA-112"),
        }
    }
}

/// SC-12: the words of every state and refusal of this part, with at least
/// one value for each variant and for each variant inside it.
pub fn all_own_words() -> Vec<(String, Words)> {
    use crate::own_book::BookError;
    use openbmt_core::pairing_own::OwnPairError;

    let mut all: Vec<(String, Words)> = Vec::new();
    let mut add = |name: &str, words: Words| all.push((name.to_string(), words));
    add("they_differ", they_differ());
    add("wallet_not_readable", wallet_not_readable());
    add("shop_file_failed", shop_file_failed());
    add("range_undelivered", range_undelivered("Front counter"));
    add(
        "range_undelivered_longest_name",
        range_undelivered(&"W".repeat(32)),
    );
    let refusals = [
        ("OwnRefusal::OfferEnded", OwnRefusal::OfferEnded),
        ("OwnRefusal::ClockWentBack", OwnRefusal::ClockWentBack),
        (
            "OwnRefusal::NotThisShopsOffer",
            OwnRefusal::NotThisShopsOffer,
        ),
        ("OwnRefusal::NotARequest", OwnRefusal::NotARequest),
        ("OwnRefusal::OtherOffer", OwnRefusal::OtherOffer),
        ("OwnRefusal::BadSignature", OwnRefusal::BadSignature),
        ("OwnRefusal::OwnKey", OwnRefusal::OwnKey),
        ("OwnRefusal::BadName", OwnRefusal::BadName),
        ("OwnRefusal::AlreadyAdded", OwnRefusal::AlreadyAdded),
        (
            "OwnRefusal::Settings",
            OwnRefusal::Settings(crate::error::Refusal::NoSettingsRate),
        ),
        ("OwnRefusal::Book(Full)", OwnRefusal::Book(BookError::Full)),
        (
            "OwnRefusal::Book(TooManyDevices)",
            OwnRefusal::Book(BookError::TooManyDevices),
        ),
        (
            "OwnRefusal::Book(Corrupt)",
            OwnRefusal::Book(BookError::Corrupt("a sample")),
        ),
        ("OwnRefusal::SwitchSameWallet", OwnRefusal::SwitchSameWallet),
        ("OwnRefusal::OtherNewWallet", OwnRefusal::OtherNewWallet),
        ("OwnRefusal::NoBundle", OwnRefusal::NoBundle),
        ("OwnRefusal::UnknownCounter", OwnRefusal::UnknownCounter),
        ("OwnRefusal::TooManyWaiting", OwnRefusal::TooManyWaiting),
        ("OwnRefusal::BoxWaits", OwnRefusal::BoxWaits),
        (
            "OwnRefusal::Pairing",
            OwnRefusal::Pairing(OwnPairError::NotTheOwner),
        ),
    ];
    for (name, refusal) in refusals {
        add(name, refusal.words());
    }
    for status in [
        SwitchStatus::Switched,
        SwitchStatus::Waiting,
        SwitchStatus::Refused,
    ] {
        add(&format!("switch_status::{status:?}"), status_words(status));
    }
    add("switch_move_not_ready", not_ready_words());
    all
}

#[cfg(test)]
mod tests {
    use super::*;
    use openbmt_core::say::check;

    /// SC-12: every sentence that is written passes the checks, and the
    /// words that are not written are the placeholders and nothing else.
    #[test]
    fn sc_12_the_own_wallet_words_pass_or_are_placeholders() {
        let all = all_own_words();
        let written: Vec<(String, Words)> = all
            .iter()
            .filter(|(_, w)| !is_placeholder(w))
            .cloned()
            .collect();
        assert!(written.len() >= 7, "{}", written.len());
        for (name, w) in &written {
            assert_eq!(check(w), Ok(()), "{name}");
        }
        openbmt_core::say::check_distinct(&written).unwrap();
        for (name, w) in all.iter().filter(|(_, w)| is_placeholder(w)) {
            assert!(
                [&w.heading, &w.what, &w.next]
                    .iter()
                    .all(|t| t.starts_with("WORDS_NEEDED_OA_") || t.starts_with("WORDS_NEEDED_SS")),
                "{name}"
            );
        }
    }

    /// SC-12: a variant missing from the list fails this match.
    #[test]
    fn sc_12_every_variant_is_in_the_list() {
        let covered = |r: &OwnRefusal| -> &'static str {
            match r {
                OwnRefusal::OfferEnded => "OwnRefusal::OfferEnded",
                OwnRefusal::ClockWentBack => "OwnRefusal::ClockWentBack",
                OwnRefusal::NotThisShopsOffer => "OwnRefusal::NotThisShopsOffer",
                OwnRefusal::NotARequest => "OwnRefusal::NotARequest",
                OwnRefusal::OtherOffer => "OwnRefusal::OtherOffer",
                OwnRefusal::BadSignature => "OwnRefusal::BadSignature",
                OwnRefusal::OwnKey => "OwnRefusal::OwnKey",
                OwnRefusal::BadName => "OwnRefusal::BadName",
                OwnRefusal::AlreadyAdded => "OwnRefusal::AlreadyAdded",
                OwnRefusal::SwitchSameWallet => "OwnRefusal::SwitchSameWallet",
                OwnRefusal::OtherNewWallet => "OwnRefusal::OtherNewWallet",
                OwnRefusal::Settings(_) => "OwnRefusal::Settings",
                OwnRefusal::Book(_) => "OwnRefusal::Book(Full)",
                OwnRefusal::NoBundle => "OwnRefusal::NoBundle",
                OwnRefusal::UnknownCounter => "OwnRefusal::UnknownCounter",
                OwnRefusal::TooManyWaiting => "OwnRefusal::TooManyWaiting",
                OwnRefusal::BoxWaits => "OwnRefusal::BoxWaits",
                OwnRefusal::Pairing(_) => "OwnRefusal::Pairing",
            }
        };
        let names: Vec<String> = all_own_words().into_iter().map(|(n, _)| n).collect();
        for r in [
            OwnRefusal::OfferEnded,
            OwnRefusal::ClockWentBack,
            OwnRefusal::NotThisShopsOffer,
            OwnRefusal::NotARequest,
            OwnRefusal::OtherOffer,
            OwnRefusal::BadSignature,
            OwnRefusal::OwnKey,
            OwnRefusal::BadName,
            OwnRefusal::AlreadyAdded,
            OwnRefusal::SwitchSameWallet,
            OwnRefusal::OtherNewWallet,
            OwnRefusal::NoBundle,
            OwnRefusal::UnknownCounter,
            OwnRefusal::TooManyWaiting,
            OwnRefusal::BoxWaits,
        ] {
            assert!(names.iter().any(|n| n == covered(&r)), "{r:?}");
        }
    }
}
