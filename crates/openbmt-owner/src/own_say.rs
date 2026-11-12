//! What the owner is told about pairing a counter with the shop's own wallet
//! (`docs/OWNER-APP-RULES.md` OA-105 to OA-114, `docs/SCREEN-RULES.md`).
//!
//! The sentences are the ones in the approved words file for this part of
//! the app. A state or refusal that file does not cover carries the
//! placeholder constant that names the rule, `WORDS_NEEDED_OA_xxx`, until
//! its words are written (OA-114).

use openbmt_core::say::{Tone, Who, Words};

use crate::own_shop::OwnRefusal;
use crate::say::stop;

/// OA-106: the words of a book that gave no range are still to be written.
pub const WORDS_NEEDED_OA_106: &str = "WORDS_NEEDED_OA_106";
/// OA-107: the words of a book that is past what the phone watches.
pub const WORDS_NEEDED_OA_107: &str = "WORDS_NEEDED_OA_107";
/// OA-108: the words of an offer that is not this shop's, and of a clock
/// that went back.
pub const WORDS_NEEDED_OA_108: &str = "WORDS_NEEDED_OA_108";
/// OA-109: the words of a counter name that is not plain text.
pub const WORDS_NEEDED_OA_109_NAME: &str = "WORDS_NEEDED_OA_109_NAME";
/// OA-110: the words of a counter that is added already, and of a step of
/// the ceremony that failed after the checks.
pub const WORDS_NEEDED_OA_110: &str = "WORDS_NEEDED_OA_110";
/// OA-111: the words of a bundle that does not wait, and of bundles that
/// wait for as many counters as the shop keeps.
pub const WORDS_NEEDED_OA_111: &str = "WORDS_NEEDED_OA_111";
/// OA-112: the words of a range for a counter the book does not know.
pub const WORDS_NEEDED_OA_112: &str = "WORDS_NEEDED_OA_112";

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

impl OwnRefusal {
    /// What the owner is told (OA-109, OA-114).
    pub fn words(&self) -> Words {
        match self {
            Self::OfferEnded => offer_ended(),
            Self::ClockWentBack | Self::NotThisShopsOffer => needed(WORDS_NEEDED_OA_108),
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
            Self::BadName => needed(WORDS_NEEDED_OA_109_NAME),
            Self::AlreadyAdded | Self::Pairing(_) => needed(WORDS_NEEDED_OA_110),
            Self::Settings(r) => r.words(),
            Self::Book(crate::own_book::BookError::WatchBudget) => needed(WORDS_NEEDED_OA_107),
            Self::Book(_) => needed(WORDS_NEEDED_OA_106),
            Self::NoBundle | Self::TooManyWaiting => needed(WORDS_NEEDED_OA_111),
            Self::UnknownCounter => needed(WORDS_NEEDED_OA_112),
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
            "OwnRefusal::Book(WatchBudget)",
            OwnRefusal::Book(BookError::WatchBudget),
        ),
        (
            "OwnRefusal::Book(TooManyDevices)",
            OwnRefusal::Book(BookError::TooManyDevices),
        ),
        (
            "OwnRefusal::Book(Corrupt)",
            OwnRefusal::Book(BookError::Corrupt("a sample")),
        ),
        ("OwnRefusal::NoBundle", OwnRefusal::NoBundle),
        ("OwnRefusal::UnknownCounter", OwnRefusal::UnknownCounter),
        ("OwnRefusal::TooManyWaiting", OwnRefusal::TooManyWaiting),
        (
            "OwnRefusal::Pairing",
            OwnRefusal::Pairing(OwnPairError::NotTheOwner),
        ),
    ];
    for (name, refusal) in refusals {
        add(name, refusal.words());
    }
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
                w.heading == w.what
                    && w.what == w.next
                    && w.heading.starts_with("WORDS_NEEDED_OA_"),
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
                OwnRefusal::Settings(_) => "OwnRefusal::Settings",
                OwnRefusal::Book(_) => "OwnRefusal::Book(Full)",
                OwnRefusal::NoBundle => "OwnRefusal::NoBundle",
                OwnRefusal::UnknownCounter => "OwnRefusal::UnknownCounter",
                OwnRefusal::TooManyWaiting => "OwnRefusal::TooManyWaiting",
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
            OwnRefusal::NoBundle,
            OwnRefusal::UnknownCounter,
            OwnRefusal::TooManyWaiting,
        ] {
            assert!(names.iter().any(|n| n == covered(&r)), "{r:?}");
        }
    }
}
