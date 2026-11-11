//! Checks that the tests and the errors line up with the rules file.

use bitcoin::Txid;
use openbmt_core::identity::IdentityError;
use openbmt_core::wire::DecodeError;
use openbmt_owner::{OwnerError, Refusal, RestoreError, SetupError};
use openbmt_session::{BindError, EncodeError};

const RULES: &str = include_str!("../../../docs/SESSION-RULES.md");
const TEST_SOURCES: [&str; 5] = [
    include_str!("counter.rs"),
    include_str!("drop.rs"),
    include_str!("queue.rs"),
    include_str!("session.rs"),
    include_str!("property.rs"),
];

/// Every rule ID of the rules file, as a number.
fn rule_ids() -> Vec<u32> {
    let mut ids = Vec::new();
    for line in RULES.lines() {
        if let Some(rest) = line.trim_start().strip_prefix("- **AS-") {
            if let Ok(n) = rest.chars().take(2).collect::<String>().parse::<u32>() {
                ids.push(n);
            }
        }
    }
    ids
}

/// The rules of this crate: section 3, AS-50 and up.
fn own_rules() -> Vec<u32> {
    rule_ids().into_iter().filter(|n| *n >= 50).collect()
}

#[test]
fn the_rules_file_defines_the_rules_the_tests_name() {
    assert_eq!(own_rules().len(), 24);
    assert!(own_rules().contains(&50) && own_rules().contains(&73));
}

#[test]
fn every_rule_has_at_least_one_test_named_for_it() {
    let missing: Vec<_> = own_rules()
        .into_iter()
        .filter(|n| {
            let needle = format!("fn as_{n:02}_");
            !TEST_SOURCES.iter().any(|src| src.contains(&needle))
        })
        .collect();
    assert!(missing.is_empty(), "rules without a test: {missing:?}");
}

#[test]
fn every_test_is_named_for_a_rule_of_the_file() {
    let ids = rule_ids();
    for src in TEST_SOURCES {
        for line in src.lines() {
            let Some(rest) = line.trim_start().strip_prefix("fn as_") else {
                continue;
            };
            let n: u32 = rest.chars().take(2).collect::<String>().parse().unwrap();
            assert!(ids.contains(&n), "a test names AS-{n:02}, not a rule");
        }
    }
}

#[test]
fn every_error_says_which_rule_in_words() {
    let ids = rule_ids();
    let texts: Vec<String> = vec![
        Refusal::Full("x").to_string(),
        Refusal::BadText("x").to_string(),
        Refusal::DestinationKind.to_string(),
        Refusal::BelowDust { sats: 1, dust: 2 }.to_string(),
        Refusal::AmountTooLarge.to_string(),
        Refusal::UnknownSale.to_string(),
        Refusal::MoreThanReceived {
            received: 1,
            wanted: 2,
        }
        .to_string(),
        Refusal::SaleInUse.to_string(),
        Refusal::UnknownEntry(1).to_string(),
        Refusal::WrongStage("x").to_string(),
        Refusal::BatchNotDerived.to_string(),
        Refusal::PairingRequest("x".into()).to_string(),
        Refusal::PairingNotRequest("x".into()).to_string(),
        Refusal::PairingOtherShop.to_string(),
        Refusal::PairingKnown.to_string(),
        Refusal::NoSettingsRate.to_string(),
        Refusal::BundleNotReady(openbmt_owner::BundleGap::Pairing).to_string(),
        Refusal::BundleNotReady(openbmt_owner::BundleGap::Settings).to_string(),
        Refusal::SettingsInvalid("x".into()).to_string(),
        Refusal::SettingsVersion { known: 1, got: 1 }.to_string(),
        Refusal::ZeroFeeRate.to_string(),
        Refusal::Encode(EncodeError::TooLarge).to_string(),
        Refusal::SessionIdUsed.to_string(),
        Refusal::Frame(DecodeError::Truncated).to_string(),
        Refusal::UnknownSession.to_string(),
        Refusal::Bind(BindError::WrongApprover).to_string(),
        Refusal::OutboxFull.to_string(),
        Refusal::UnknownTransaction(Txid::from_raw_hash(bitcoin::hashes::Hash::all_zeros()))
            .to_string(),
        Refusal::SnapshotTooLarge.to_string(),
        Refusal::Key1Wallet.to_string(),
        OwnerError::<String>::Store("x".into()).to_string(),
        OwnerError::<String>::Refused(Refusal::ZeroFeeRate).to_string(),
        SetupError::Identity(IdentityError::InvalidSecret).to_string(),
        SetupError::SameRoot.to_string(),
        RestoreError::Decode(DecodeError::Truncated).to_string(),
        RestoreError::WrongWallet.to_string(),
        RestoreError::WrongOwner.to_string(),
        RestoreError::WrongApprover.to_string(),
        RestoreError::Inconsistent("x").to_string(),
    ];
    for t in &texts {
        assert!(t.is_ascii() && !t.contains('{'), "{t}");
        let rest = t.strip_prefix("AS-").unwrap_or_else(|| panic!("{t}"));
        let n: u32 = rest[..2].parse().unwrap();
        assert!(ids.contains(&n), "{t} names a rule that is not in the file");
    }
}
