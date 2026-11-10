//! What a person is told about the watcher (`docs/SCREEN-RULES.md`, SC-01 to
//! SC-12).
//!
//! Two things reach a person. A [`Problem`] with the check is what the owner
//! looks at when a cashier says "the counter cannot check payments". A
//! watcher [`Refusal`] reaches people only as a line in the counter's notes;
//! the 49 variants are sorted into [`RefusalGroup`]s, one group for each
//! thing a person would do differently, and each group has one [`Words`].

use openbmt_core::say::{Tone, Who, Words};

use crate::health::{Health, Problem};
use crate::refusal::Refusal;

impl Problem {
    /// What the owner is told about one problem with the check. The cashier
    /// is only told that the counter cannot check payments.
    pub fn words(&self) -> Words {
        let (heading, what, next) = match self {
            Self::TooFewPeerGroups => (
                "Too few network links",
                "This counter is connected to too few separate parts of the network to trust what it sees.",
                "Check the counter's internet connection and wait a few minutes.",
            ),
            Self::TooFewFilterPeers => (
                "Too few sources for checks",
                "Too few of this counter's connections can answer its payment checks.",
                "Check the counter's internet connection and wait a few minutes.",
            ),
            Self::TipIsOld => (
                "Newest block is too old",
                "The newest block this counter knows is more than two hours old.",
                "Check the counter's internet connection; it catches up by itself.",
            ),
            Self::HeadersBehind => (
                "Counter is behind",
                "This counter has stayed behind the rest of the network for more than ten minutes.",
                "Check the counter's internet connection; it catches up by itself.",
            ),
            Self::ClockMismatch => (
                "Counter's clock is wrong",
                "This counter's clock is more than two hours behind the network's.",
                "Set the counter's clock to the right time.",
            ),
        };
        Words::new(Tone::Stop, heading, what, Who::Owner, next).code(match self {
            Self::TooFewPeerGroups | Self::TooFewFilterPeers => "WR-50",
            Self::TipIsOld => "WR-51",
            Self::HeadersBehind => "WR-52",
            Self::ClockMismatch => "WR-53",
        })
    }
}

impl Health {
    /// What the owner is told about the answer of the health check.
    pub fn words(&self) -> Words {
        if let Some(problem) = self.problem {
            return problem.words();
        }
        if !self.healthy {
            return Words::new(
                Tone::Info,
                "No answer from the network",
                "The counter has not yet heard from enough of the network to say whether it can check payments.",
                Who::Nobody,
                "Nothing to do. It starts by itself once the counter is connected.",
            )
            .code("WR-50");
        }
        if self.behind {
            return Words::new(
                Tone::Info,
                "Counter may be catching up",
                "Some of the network is a few blocks ahead of this counter, which can still check payments.",
                Who::Nobody,
                "Nothing to do. If it lasts more than ten minutes the counter stops checking payments.",
            )
            .code("WR-52");
        }
        Words::new(
            Tone::Info,
            "Checking works",
            "This counter can check payments.",
            Who::Nobody,
            "Nothing to do.",
        )
        .code("WR-50")
    }
}

/// The groups the 49 watcher refusals fall into. Two refusals are in one
/// group when a person would do the same about both.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefusalGroup {
    /// The block that held a payment is not on the best chain now (WR-01).
    LeftTheChain,
    /// A block header from the network did not pass the checks (WR-03,
    /// WR-05, WR-07).
    HeaderNotBelieved,
    /// A block header is dated too far ahead of the clock (WR-06).
    HeaderFromTheFuture,
    /// The saved headers cannot be used (WR-04, WR-08).
    SavedHeadersUnusable,
    /// A proof of a payment did not check out (WR-10 to WR-16).
    ProofNotBelieved,
    /// A whole block did not check out (WR-20 to WR-22).
    BlockNotBelieved,
    /// A transaction pays the sale nothing (WR-30).
    PaysNothing,
    /// The sale holds more than the counter can judge (WR-34).
    NeedsAPerson,
    /// Bytes offered as a transaction are not one (WR-40).
    NotATransaction,
}

impl RefusalGroup {
    /// Every group.
    pub const ALL: [RefusalGroup; 9] = [
        Self::LeftTheChain,
        Self::HeaderNotBelieved,
        Self::HeaderFromTheFuture,
        Self::SavedHeadersUnusable,
        Self::ProofNotBelieved,
        Self::BlockNotBelieved,
        Self::PaysNothing,
        Self::NeedsAPerson,
        Self::NotATransaction,
    ];

    /// The name of the group.
    pub fn name(&self) -> &'static str {
        match self {
            Self::LeftTheChain => "LeftTheChain",
            Self::HeaderNotBelieved => "HeaderNotBelieved",
            Self::HeaderFromTheFuture => "HeaderFromTheFuture",
            Self::SavedHeadersUnusable => "SavedHeadersUnusable",
            Self::ProofNotBelieved => "ProofNotBelieved",
            Self::BlockNotBelieved => "BlockNotBelieved",
            Self::PaysNothing => "PaysNothing",
            Self::NeedsAPerson => "NeedsAPerson",
            Self::NotATransaction => "NotATransaction",
        }
    }

    /// The words for the group.
    pub fn words(&self) -> Words {
        match self {
            Self::LeftTheChain => Words::new(
                Tone::Info,
                "Block left the chain",
                "The block that held this payment is no longer on the best chain, so it was not believed.",
                Who::Nobody,
                "Nothing to do. The counter looks again when the next block comes.",
            )
            .code("WR-01"),
            Self::HeaderNotBelieved => Words::new(
                Tone::Ended,
                "Block header not accepted",
                "A block header from the network did not pass the counter's checks, so it was left out.",
                Who::Nobody,
                "Nothing to do. The counter keeps its own chain and asks again.",
            )
            .code("WR-05"),
            Self::HeaderFromTheFuture => Words::new(
                Tone::Info,
                "Header dated in the future",
                "A block header is dated more than two hours ahead of this counter's clock, so it was left out for now.",
                Who::Owner,
                "If this repeats, check that the counter's clock is right. The header is tried again later.",
            )
            .code("WR-06"),
            Self::SavedHeadersUnusable => Words::new(
                Tone::Stop,
                "Saved headers not usable",
                "The block headers this counter saved cannot be used, so nothing was changed and it cannot build on them.",
                Who::Owner,
                "Restart the counter. If this comes back, the saved headers need to be replaced.",
            )
            .code("WR-08"),
            Self::ProofNotBelieved => Words::new(
                Tone::Ended,
                "Payment proof not believed",
                "A proof of a payment did not check out, so nothing was believed and nothing was recorded.",
                Who::Nobody,
                "Nothing to do. The sale keeps waiting for a proof that does check out.",
            )
            .code("WR-10"),
            Self::BlockNotBelieved => Words::new(
                Tone::Ended,
                "Block not believed",
                "A whole block sent to the counter did not check out, so nothing was believed.",
                Who::Nobody,
                "Nothing to do. The counter asks again, and the sale keeps waiting.",
            )
            .code("WR-20"),
            Self::PaysNothing => Words::new(
                Tone::Ended,
                "Payment not for this sale",
                "A transaction was offered that does not pay this sale and clashes with nothing the sale knows.",
                Who::Nobody,
                "Nothing to do. The sale goes on as before.",
            )
            .code("WR-30"),
            Self::NeedsAPerson => Words::new(
                Tone::Stop,
                "Needs a person",
                "This sale holds more than the counter can judge, so the counter stops judging it.",
                Who::Owner,
                "Do not hand over the goods. The owner looks at this sale on the owner's phone.",
            )
            .code("WR-34"),
            Self::NotATransaction => Words::new(
                Tone::Ended,
                "Not a payment",
                "Something offered as a payment was not a transaction, so it was ignored.",
                Who::Nobody,
                "Nothing to do.",
            )
            .code("WR-40"),
        }
    }
}

impl Refusal {
    /// The group a refusal belongs to: every variant is placed by an
    /// exhaustive match, so a new variant must choose its group.
    pub fn group(&self) -> RefusalGroup {
        match self {
            Self::NotOnChain => RefusalGroup::LeftTheChain,
            Self::HeaderProofOfWork
            | Self::UnknownParent
            | Self::WrongBits { .. }
            | Self::ChainProofOfWork
            | Self::TimeNotAfterMedian { .. }
            | Self::AncestorMissing
            | Self::HeightOverflow
            | Self::SideHeaderTooDeep
            | Self::SideHeadersFull
            | Self::WorkOverflow => RefusalGroup::HeaderNotBelieved,
            Self::TimeTooFarAhead { .. } => RefusalGroup::HeaderFromTheFuture,
            Self::NetworkNotSupported
            | Self::StoreMalformed(_)
            | Self::StoreWrongNetwork
            | Self::StoreWrongStart
            | Self::StoreNotChained
            | Self::StoreHeaderRefused { .. } => RefusalGroup::SavedHeadersUnusable,
            Self::ProofTooLarge { .. }
            | Self::ProofMalformed
            | Self::CoinbaseTooLarge { .. }
            | Self::PaymentTooLarge { .. }
            | Self::TreeTransactionCount { .. }
            | Self::TreeTooManyHashes
            | Self::TreeBitsAndHashes
            | Self::TreeEqualChildren
            | Self::TreePadding
            | Self::TreeRootMismatch
            | Self::ProofCoversWrongCount { .. }
            | Self::CoinbaseNotFirst { .. }
            | Self::CoinbaseMalformed
            | Self::CoinbaseIdMismatch
            | Self::NotACoinbase
            | Self::PaymentMalformed
            | Self::PaymentIdMismatch
            | Self::PaymentIsCoinbase
            | Self::CoinbaseSixtyFourBytes
            | Self::PaymentSixtyFourBytes => RefusalGroup::ProofNotBelieved,
            Self::BlockTooLarge { .. }
            | Self::BlockMalformed
            | Self::BlockMerkleRoot
            | Self::BlockNoCoinbase
            | Self::BlockExtraCoinbase { .. }
            | Self::BlockDuplicateTransaction { .. }
            | Self::BlockPaymentSixtyFourBytes { .. } => RefusalGroup::BlockNotBelieved,
            Self::PaysNothing => RefusalGroup::PaysNothing,
            Self::TooManyTransactions | Self::TransactionTooLarge { .. } => {
                RefusalGroup::NeedsAPerson
            }
            Self::NotATransaction => RefusalGroup::NotATransaction,
        }
    }

    /// The words for a refusal: its group's words, with the refusal's own
    /// text for the small print.
    pub fn words(&self) -> Words {
        self.group().words().detail(self.to_string())
    }
}

/// Every one of these words, for SC-12: one value for each problem, for each
/// answer of the health check and, because the 49 refusals share one
/// sentence for each group, one for each group of refusals (see
/// [`Refusal::group`]).
pub fn all_words() -> Vec<(String, Words)> {
    let mut all = Vec::new();
    for problem in [
        Problem::TooFewPeerGroups,
        Problem::TooFewFilterPeers,
        Problem::TipIsOld,
        Problem::HeadersBehind,
        Problem::ClockMismatch,
    ] {
        all.push((format!("Problem::{}", problem_name(problem)), problem.words()));
    }
    for (name, health) in sample_health() {
        all.push((format!("Health::{name}"), health.words()));
    }
    for group in RefusalGroup::ALL {
        all.push((format!("RefusalGroup::{}", group.name()), group.words()));
    }
    all
}

/// The name of a variant, by an exhaustive match.
pub fn problem_name(p: Problem) -> &'static str {
    match p {
        Problem::TooFewPeerGroups => "TooFewPeerGroups",
        Problem::TooFewFilterPeers => "TooFewFilterPeers",
        Problem::TipIsOld => "TipIsOld",
        Problem::HeadersBehind => "HeadersBehind",
        Problem::ClockMismatch => "ClockMismatch",
    }
}

/// One answer of the health check for each thing `Health::words` tells.
fn sample_health() -> Vec<(&'static str, Health)> {
    vec![
        (
            "Healthy",
            Health {
                healthy: true,
                problem: None,
                behind: false,
            },
        ),
        (
            "HealthyButBehind",
            Health {
                healthy: true,
                problem: None,
                behind: true,
            },
        ),
        (
            "NoAnswer",
            Health {
                healthy: false,
                problem: None,
                behind: false,
            },
        ),
    ]
}
