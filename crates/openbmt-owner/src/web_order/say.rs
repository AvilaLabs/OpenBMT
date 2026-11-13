//! What the owner is told about a web order (`docs/WEB-ORDER-RULES.md`,
//! `docs/SCREEN-RULES.md`).
//!
//! No sentence is written yet. Every text is the placeholder that names its
//! rule, `WORDS_NEEDED_WO_<rule>_HEADING`, `_WHAT` and `_NEXT`, so that the
//! missing words show wherever the words will stand. The checks of the
//! sentences skip a placeholder, as those of the own wallet's words do
//! (`own_say`).

use openbmt_core::say::{Tone, Who, Words};

use super::check::OrderRefusal;
use super::WebOrderRefusal;

/// Words that are still to be written for a rule of the web order rules:
/// `rule` is the number of the rule without its prefix ("22" for WO-22).
pub fn needed(rule: &str) -> Words {
    Words::new(
        Tone::Stop,
        format!("WORDS_NEEDED_WO_{rule}_HEADING"),
        format!("WORDS_NEEDED_WO_{rule}_WHAT"),
        Who::Owner,
        format!("WORDS_NEEDED_WO_{rule}_NEXT"),
    )
}

/// Whether these words are a placeholder.
pub fn is_placeholder(words: &Words) -> bool {
    words.heading.starts_with("WORDS_NEEDED_")
}

/// The rule of a refusal without the prefix: "WO-43" is "43".
fn number(rule: &str) -> &str {
    rule.strip_prefix("WO-").unwrap_or(rule)
}

/// The words for a refusal of a frame, a key or a request.
pub fn refusal_words(refusal: &WebOrderRefusal) -> Words {
    needed(number(refusal.rule()))
}

/// WO-22: the words for an order the owner phone refused and shows.
pub fn order_refusal_words(refusal: &OrderRefusal) -> Words {
    needed(number(refusal.rule()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all() -> Vec<(String, Words)> {
        let refusals = [
            WebOrderRefusal::Random,
            WebOrderRefusal::PairingCode,
            WebOrderRefusal::ShopName,
            WebOrderRefusal::PairingId,
            WebOrderRefusal::Range,
            WebOrderRefusal::Notice,
            WebOrderRefusal::Signature,
            WebOrderRefusal::Request,
            WebOrderRefusal::Orders,
        ];
        let orders = [
            OrderRefusal::AddressNotSent,
            OrderRefusal::AddressUsedBefore,
            OrderRefusal::OrderChanged,
            OrderRefusal::NoAmount,
        ];
        let mut out: Vec<(String, Words)> = refusals
            .iter()
            .map(|r| (format!("{r:?}"), refusal_words(r)))
            .collect();
        out.extend(
            orders
                .iter()
                .map(|r| (format!("{r:?}"), order_refusal_words(r))),
        );
        out
    }

    /// SC-12: the words that are written pass the checks, and those that are
    /// not written are the placeholders of their rule and nothing else.
    #[test]
    fn sc_12_the_web_order_words_pass_or_are_placeholders() {
        let all = all();
        let written: Vec<(String, Words)> = all
            .iter()
            .filter(|(_, w)| !is_placeholder(w))
            .cloned()
            .collect();
        for (name, w) in &written {
            assert_eq!(openbmt_core::say::check(w), Ok(()), "{name}");
        }
        openbmt_core::say::check_distinct(&written).unwrap();
        for (name, w) in all.iter().filter(|(_, w)| is_placeholder(w)) {
            assert!(
                w.heading.starts_with("WORDS_NEEDED_WO_")
                    && w.heading.ends_with("_HEADING")
                    && w.what.ends_with("_WHAT")
                    && w.next.ends_with("_NEXT"),
                "{name}"
            );
            let stem = w.heading.trim_end_matches("_HEADING");
            assert_eq!(w.what, format!("{stem}_WHAT"), "{name}");
            assert_eq!(w.next, format!("{stem}_NEXT"), "{name}");
        }
    }

    /// WO-22, WO-43: every refusal's placeholder names the rule it is from.
    #[test]
    fn wo_22_wo_43_a_placeholder_names_its_rule() {
        assert_eq!(
            refusal_words(&WebOrderRefusal::Random).heading,
            "WORDS_NEEDED_WO_43_HEADING"
        );
        assert_eq!(
            order_refusal_words(&OrderRefusal::AddressNotSent).what,
            "WORDS_NEEDED_WO_22_WHAT"
        );
    }
}
