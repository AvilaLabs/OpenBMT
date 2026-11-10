//! SC-01 to SC-12 for the watcher's words.

use openbmt_core::say::{check, check_distinct, Tone, Who};
use openbmt_core::wire::DecodeError;

use crate::say::{all_words, problem_name, RefusalGroup};
use crate::{Health, Problem, Refusal};

/// One value of every watcher refusal.
fn every_refusal() -> Vec<Refusal> {
    let inner = || Box::new(Refusal::NotOnChain);
    vec![
        Refusal::NotOnChain,
        Refusal::HeaderProofOfWork,
        Refusal::NetworkNotSupported,
        Refusal::UnknownParent,
        Refusal::WrongBits {
            expected: 1,
            found: 2,
        },
        Refusal::ChainProofOfWork,
        Refusal::TimeNotAfterMedian { median: 2, time: 1 },
        Refusal::AncestorMissing,
        Refusal::HeightOverflow,
        Refusal::TimeTooFarAhead { time: 9 },
        Refusal::SideHeaderTooDeep,
        Refusal::SideHeadersFull,
        Refusal::HeaderStoreFull,
        Refusal::WorkOverflow,
        Refusal::StoreMalformed(DecodeError::Truncated),
        Refusal::StoreWrongNetwork,
        Refusal::StoreWrongStart,
        Refusal::StoreNotChained,
        Refusal::StoreHeaderRefused {
            index: 3,
            why: inner(),
        },
        Refusal::ProofTooLarge { len: 1 },
        Refusal::ProofMalformed,
        Refusal::CoinbaseTooLarge { len: 1 },
        Refusal::PaymentTooLarge { len: 1 },
        Refusal::TreeTransactionCount { count: 1 },
        Refusal::TreeTooManyHashes,
        Refusal::TreeBitsAndHashes,
        Refusal::TreeEqualChildren,
        Refusal::TreePadding,
        Refusal::TreeRootMismatch,
        Refusal::ProofCoversWrongCount { found: 1 },
        Refusal::CoinbaseNotFirst { position: 1 },
        Refusal::CoinbaseMalformed,
        Refusal::CoinbaseIdMismatch,
        Refusal::NotACoinbase,
        Refusal::PaymentMalformed,
        Refusal::PaymentIdMismatch,
        Refusal::PaymentIsCoinbase,
        Refusal::CoinbaseSixtyFourBytes,
        Refusal::PaymentSixtyFourBytes,
        Refusal::BlockTooLarge { len: 1 },
        Refusal::BlockMalformed,
        Refusal::BlockMerkleRoot,
        Refusal::BlockNoCoinbase,
        Refusal::BlockExtraCoinbase { position: 1 },
        Refusal::BlockDuplicateTransaction { position: 1 },
        Refusal::BlockPaymentSixtyFourBytes { position: 1 },
        Refusal::PaysNothing,
        Refusal::TooManyTransactions,
        Refusal::TransactionTooLarge { len: 1 },
        Refusal::NotATransaction,
    ]
}

#[test]
fn sc_12_every_word_keeps_the_rules() {
    assert_eq!(check_distinct(&all_words()), Ok(()));
}

#[test]
fn sc_01_no_variant_is_missing() {
    let all = all_words();
    let has = |name: String| all.iter().any(|(n, _)| *n == name);
    for p in [
        Problem::TooFewPeerGroups,
        Problem::TooFewFilterPeers,
        Problem::TipIsOld,
        Problem::HeadersBehind,
        Problem::ClockMismatch,
    ] {
        assert!(has(format!("Problem::{}", problem_name(p))));
    }
    for g in RefusalGroup::ALL {
        assert!(has(format!("RefusalGroup::{}", g.name())));
    }
    // Every refusal is listed (`Refusal::group` is exhaustive), each one has words that keep the rules,
    // and every group is reached by some refusal.
    let refusals = every_refusal();
    assert_eq!(refusals.len(), 50);
    let mut reached = Vec::new();
    for r in &refusals {
        assert_eq!(check(&r.words()), Ok(()), "{r}");
        assert_eq!(r.words().code, r.group().words().code);
        if !reached.contains(&r.group().name()) {
            reached.push(r.group().name());
        }
    }
    assert_eq!(reached.len(), RefusalGroup::ALL.len());
}

#[test]
fn sc_01_every_health_answer_has_words() {
    for (healthy, problem, behind) in [
        (true, None, false),
        (true, None, true),
        (false, None, false),
        (false, Some(Problem::TipIsOld), false),
    ] {
        let words = Health {
            healthy,
            problem,
            behind,
        }
        .words();
        assert_eq!(check(&words), Ok(()));
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
fn sc_03_the_refusal_text_is_only_in_the_small_print() {
    let w = Refusal::WrongBits {
        expected: 0x1d00_ffff,
        found: 0x1b04_0404,
    }
    .words();
    assert!(w.detail.is_some_and(|d| d.contains("WR-05")));
    assert!(!w.what.contains("WR-"));
}
