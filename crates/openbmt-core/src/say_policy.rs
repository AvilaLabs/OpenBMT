//! What a cashier is told about the instant-acceptance enums of
//! [`crate::policy`] (`docs/SCREEN-RULES.md`, SC-01 to SC-12).
//!
//! The words sit here and not in `policy.rs` so that the rules stay free of
//! text. Each enum gets a `words` method; the counter's sale screen uses
//! them for the states that wait or that accept at once.

use crate::policy::{CheckMode, InstantLabel, OffReason, WaitReason};
use crate::say::{Tone, Who, Words};

/// What a waiting sale does next: nothing, and it turns green with the
/// block, or the cashier takes another kind of payment.
const WAIT_NEXT: &str = "Nothing to do. It turns green when the payment is in a block, in about ten \
     minutes, or take another kind of payment.";

impl CheckMode {
    /// How the payment was checked, for the line under a sale.
    pub fn words(&self) -> Words {
        let (heading, what) = match self {
            Self::Light => (
                "Checked by this counter",
                "This counter checked the payment itself, through its own connection to the network.",
            ),
            Self::NodeAlsoSeenByLight => (
                "Checked by the shop's system",
                "The shop's own system and this counter both saw the payment.",
            ),
            Self::BoxReportsOnly => (
                "Checked by the Box",
                "Only the shop's Box reported the payment; this counter did not see it itself.",
            ),
        };
        Words::new(
            Tone::Info,
            heading,
            what,
            Who::Nobody,
            "Nothing to do. The sale screen says when to hand over the goods.",
        )
        .code("CR-10")
    }
}

impl InstantLabel {
    /// The words for a payment accepted at once. The heading is "Paid" for
    /// all three; the sentence says what checked it.
    pub fn words(&self) -> Words {
        let what = match self {
            Self::LightCheck => {
                "Accepted at once on this counter's own check, before a block holds it, so it could still be undone."
            }
            Self::YourNode => {
                "Accepted at once on the shop's own system, before a block holds it, so it could still be undone."
            }
            Self::YourBox => {
                "Accepted at once on the Box's report, before a block holds it, so it could still be undone."
            }
        };
        Words::new(
            Tone::Good,
            "Paid",
            what,
            Who::Cashier,
            "Hand over the goods. It is settled when the payment is in a block, in about ten minutes.",
        )
        .code("CR-13")
    }
}

impl OffReason {
    /// Why accepting at once is off, for a sale that waits for a block.
    pub fn words(&self) -> Words {
        let waiting =
            |heading: &str, what: &str| Words::new(Tone::Info, heading, what, Who::Nobody, WAIT_NEXT);
        match self {
            Self::PausedFor { .. } => waiting(
                "Waiting: instant paused",
                "Accepting at once is paused for a while after a cancel, so this sale waits for a block.",
            )
            .code("CR-14"),
            Self::PausedUntilOwnerReset => Words::new(
                Tone::Info,
                "Waiting: paused by owner",
                "Accepting at once is paused until the owner resets it, so this sale waits for a block.",
                Who::Nobody,
                "Nothing to do. It turns green in a block, in about ten minutes. The owner can reset the pause.",
            )
            .code("CR-14"),
            Self::Disabled => waiting(
                "Waiting: instant is off",
                "Accepting at once is switched off at this counter, so this sale waits for a block.",
            )
            .code("CR-10"),
            Self::CheckUnhealthy => Words::new(
                Tone::Info,
                "Waiting: cannot check yet",
                "This counter cannot check payments well enough to accept at once, so this sale waits.",
                Who::Cashier,
                "Wait for a block, or take another kind of payment. Tell the owner: the counter cannot check payments.",
            )
            .code("CR-10"),
            Self::NoWatcher => Words::new(
                Tone::Info,
                "Waiting: no word from phone",
                "The owner's phone has not confirmed that it is watching, so this sale waits for a block.",
                Who::Cashier,
                "Wait for a block, in about ten minutes. Tell the owner: the phone check-in is missing.",
            )
            .code("CR-52"),
            Self::SharedWifiNeedsTor => Words::new(
                Tone::Info,
                "Waiting: shared Wi-Fi",
                "Customers can join the Wi-Fi this counter uses, so it does not accept at once.",
                Who::Nobody,
                "Nothing to do. It turns green in a block, in about ten minutes. The owner can change the connection.",
            )
            .code("CR-10"),
        }
    }
}

impl WaitReason {
    /// Why this payment waits for a block.
    pub fn words(&self) -> Words {
        let waiting = |heading: &str, what: &str| {
            Words::new(Tone::Info, heading, what, Who::Nobody, WAIT_NEXT).code("CR-10")
        };
        match self {
            Self::ConflictSeen => waiting(
                "Waiting: second payment",
                "A second payment that spends the same money was seen, so this one waits for a block.",
            ),
            Self::RemoteOrder => waiting(
                "Waiting: remote order",
                "This is a remote order, and a remote order always waits for a block.",
            ),
            Self::Underpaid => waiting(
                "Waiting: less than needed",
                "Less arrived than the sale needs, so it waits for a block before it counts.",
            ),
            Self::OverSaleLimit => waiting(
                "Waiting: over the limit",
                "This sale is over what this counter accepts before a block.",
            ),
            Self::SliceFull => waiting(
                "Waiting: counter is full",
                "This counter already holds as much accepted before a block as it is allowed.",
            ),
            Self::ConflictWatch { .. } => Words::new(
                Tone::Info,
                "Waiting: short check",
                "The counter is making sure no second payment shows up, for a few more seconds.",
                Who::Nobody,
                "Nothing to do. It turns green when the check ends or the payment is in a block.",
            )
            .code("CR-10"),
        }
    }
}

/// Every one of these words, for SC-12. At least one value for each variant,
/// with a name for the report.
pub fn all_words() -> Vec<(String, Words)> {
    let mut all = Vec::new();
    for mode in [
        CheckMode::Light,
        CheckMode::NodeAlsoSeenByLight,
        CheckMode::BoxReportsOnly,
    ] {
        all.push((format!("CheckMode::{}", check_mode_name(mode)), mode.words()));
    }
    for label in [
        InstantLabel::LightCheck,
        InstantLabel::YourNode,
        InstantLabel::YourBox,
    ] {
        all.push((
            format!("InstantLabel::{}", instant_label_name(label)),
            label.words(),
        ));
    }
    for reason in [
        OffReason::PausedFor { secs_left: 600 },
        OffReason::PausedUntilOwnerReset,
        OffReason::Disabled,
        OffReason::CheckUnhealthy,
        OffReason::NoWatcher,
        OffReason::SharedWifiNeedsTor,
    ] {
        all.push((
            format!("OffReason::{}", off_reason_name(reason)),
            reason.words(),
        ));
    }
    for reason in [
        WaitReason::ConflictSeen,
        WaitReason::RemoteOrder,
        WaitReason::Underpaid,
        WaitReason::OverSaleLimit,
        WaitReason::SliceFull,
        WaitReason::ConflictWatch { secs_left: 5 },
    ] {
        all.push((
            format!("WaitReason::{}", wait_reason_name(reason)),
            reason.words(),
        ));
    }
    all
}

/// The name of a variant, by an exhaustive match: a new variant does not
/// compile until it is named here and given words in [`all_words`].
pub fn check_mode_name(m: CheckMode) -> &'static str {
    match m {
        CheckMode::Light => "Light",
        CheckMode::NodeAlsoSeenByLight => "NodeAlsoSeenByLight",
        CheckMode::BoxReportsOnly => "BoxReportsOnly",
    }
}

/// The name of a variant (see [`check_mode_name`]).
pub fn instant_label_name(l: InstantLabel) -> &'static str {
    match l {
        InstantLabel::LightCheck => "LightCheck",
        InstantLabel::YourNode => "YourNode",
        InstantLabel::YourBox => "YourBox",
    }
}

/// The name of a variant (see [`check_mode_name`]).
pub fn off_reason_name(r: OffReason) -> &'static str {
    match r {
        OffReason::PausedFor { .. } => "PausedFor",
        OffReason::PausedUntilOwnerReset => "PausedUntilOwnerReset",
        OffReason::Disabled => "Disabled",
        OffReason::CheckUnhealthy => "CheckUnhealthy",
        OffReason::NoWatcher => "NoWatcher",
        OffReason::SharedWifiNeedsTor => "SharedWifiNeedsTor",
    }
}

/// The name of a variant (see [`check_mode_name`]).
pub fn wait_reason_name(r: WaitReason) -> &'static str {
    match r {
        WaitReason::ConflictSeen => "ConflictSeen",
        WaitReason::RemoteOrder => "RemoteOrder",
        WaitReason::Underpaid => "Underpaid",
        WaitReason::OverSaleLimit => "OverSaleLimit",
        WaitReason::SliceFull => "SliceFull",
        WaitReason::ConflictWatch { .. } => "ConflictWatch",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::say::check_distinct;

    #[test]
    fn sc_12_every_word_keeps_the_rules() {
        assert_eq!(check_distinct(&all_words()), Ok(()));
    }

    #[test]
    fn sc_01_no_variant_is_missing() {
        let all = all_words();
        let has = |name: String| all.iter().any(|(n, _)| *n == name);
        for m in [
            CheckMode::Light,
            CheckMode::NodeAlsoSeenByLight,
            CheckMode::BoxReportsOnly,
        ] {
            assert!(has(format!("CheckMode::{}", check_mode_name(m))));
        }
        for l in [
            InstantLabel::LightCheck,
            InstantLabel::YourNode,
            InstantLabel::YourBox,
        ] {
            assert!(has(format!("InstantLabel::{}", instant_label_name(l))));
        }
        for r in [
            OffReason::PausedFor { secs_left: 1 },
            OffReason::PausedUntilOwnerReset,
            OffReason::Disabled,
            OffReason::CheckUnhealthy,
            OffReason::NoWatcher,
            OffReason::SharedWifiNeedsTor,
        ] {
            assert!(has(format!("OffReason::{}", off_reason_name(r))));
        }
        for r in [
            WaitReason::ConflictSeen,
            WaitReason::RemoteOrder,
            WaitReason::Underpaid,
            WaitReason::OverSaleLimit,
            WaitReason::SliceFull,
            WaitReason::ConflictWatch { secs_left: 1 },
        ] {
            assert!(has(format!("WaitReason::{}", wait_reason_name(r))));
        }
    }

    #[test]
    fn sc_05_someone_is_named() {
        for (name, w) in all_words() {
            assert!(
                !(w.who == Who::Nobody && w.tone == Tone::Stop),
                "{name} stops and names nobody"
            );
        }
    }

    #[test]
    fn sc_07_waiting_and_instant_off_read_differently() {
        let over = WaitReason::OverSaleLimit.words();
        let off = OffReason::Disabled.words();
        let unhealthy = OffReason::CheckUnhealthy.words();
        assert_ne!(over.what, off.what);
        assert_ne!(off.what, unhealthy.what);
        assert_ne!(over.heading, off.heading);
    }
}
