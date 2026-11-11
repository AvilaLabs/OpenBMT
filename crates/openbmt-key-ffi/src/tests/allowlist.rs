//! Cash-out addresses on the Key app (`docs/SIGNER-RULES.md`, SR-78 and
//! SR-79): the call takes a bare address or a `bitcoin:` URI, refuses each
//! wrong input with its own words, and lists what it kept.

use bitcoin::address::KnownHrp;
use bitcoin::{Address, WitnessProgram, WitnessVersion};
use openbmt_signer::ALLOWLIST_WAIT_SECS;

use super::kit::*;
use crate::{words_for, Kind};

fn rule(rig: &Rig, text: &str, label: &str) -> String {
    rig.app
        .enroll_destination(text.to_string(), label.to_string())
        .expect_err("refused")
        .rule()
        .to_string()
}

fn words_of(rig: &Rig, text: &str, label: &str) -> openbmt_core::say::Words {
    let e = rig
        .app
        .enroll_destination(text.to_string(), label.to_string())
        .expect_err("refused");
    words_for(Kind::Refused, e.rule(), &e.to_string())
}

#[test]
fn sr_78_a_bare_address_and_a_uri_both_enrol_the_address_only() {
    let rig = rig();
    rig.app
        .enroll_destination(address(1), "Bare".to_string())
        .expect("a bare address");
    let uri = format!("bitcoin:{}?amount=0.5&label=Other&message=hi", address(2));
    rig.app
        .enroll_destination(uri, "Shop".to_string())
        .expect("a URI");
    let upper = format!("BITCOIN:{}", address(3));
    rig.app
        .enroll_destination(upper, "Upper".to_string())
        .expect("a URI in capitals");
    let status = rig.app.signer_status().expect("status");
    let labels: Vec<_> = status
        .allowlist
        .iter()
        .map(|e| (e.label.as_str(), e.address.clone().expect("an address")))
        .collect();
    assert_eq!(labels.len(), 3);
    for (label, n) in [("Bare", 1), ("Shop", 2), ("Upper", 3)] {
        assert!(labels.contains(&(label, address(n))), "{label}");
    }
    for e in &status.allowlist {
        assert_eq!(e.active_at, Some(e.enrolled_at + ALLOWLIST_WAIT_SECS));
    }
    // The stored state is the same after a restart.
    let again = reopen(rig.store.clone());
    assert_eq!(again.signer_status().expect("status"), status);
}

#[test]
fn sr_78_each_wrong_input_is_refused_with_its_own_words() {
    let rig = rig();
    let mainnet = Address::from_script(&external(7), bitcoin::Network::Bitcoin)
        .expect("an address")
        .to_string();
    let program = WitnessProgram::new(WitnessVersion::V2, &[1u8; 20]).expect("a program");
    let odd_kind = Address::from_witness_program(program, KnownHrp::Regtest).to_string();
    let mut seen = std::collections::BTreeSet::new();
    let mut check = |name: &str, text: String, label: &str| {
        let words = words_of(&rig, &text, label);
        assert_eq!(openbmt_core::say::check(&words), Ok(()), "{name}");
        assert!(seen.insert(words.heading.clone()), "{name} repeats words");
        words.heading
    };

    // Another network, as a bare address and inside a URI.
    assert_eq!(rule(&rig, &mainnet, "Label"), "address");
    assert_eq!(
        rule(&rig, &format!("bitcoin:{mainnet}"), "Label"),
        "address"
    );
    check("network", mainnet, "Label");
    // A kind outside SR-22.
    assert_eq!(rule(&rig, &odd_kind, "Label"), "SR-30");
    assert_eq!(
        rule(&rig, &format!("bitcoin:{odd_kind}"), "Label"),
        "address-kind"
    );
    check("kind", odd_kind, "Label");
    // A label outside SR-30.
    assert_eq!(rule(&rig, &address(1), ""), "SR-30");
    assert_eq!(rule(&rig, &address(1), &"x".repeat(65)), "SR-30");
    check("label", address(1), "");
    // A code that is not a payment code.
    assert_eq!(
        rule(&rig, &format!("bitcoin:{}?req-x=1", address(1)), "Label"),
        "deposit-required"
    );
    // Nothing was kept by any refusal.
    assert!(rig
        .app
        .signer_status()
        .expect("status")
        .allowlist
        .is_empty());

    // A repeat, also through a URI.
    rig.app
        .enroll_destination(address(1), "First".to_string())
        .expect("enrolled");
    assert_eq!(
        rule(&rig, &format!("bitcoin:{}", address(1)), "Again"),
        "SR-30"
    );
    check("duplicate", address(1), "Again");

    // The 65th entry.
    for n in 2..=64u8 {
        rig.app
            .enroll_destination(address(n), format!("Entry {n}"))
            .expect("enrolled");
    }
    assert_eq!(rig.app.signer_status().expect("status").allowlist.len(), 64);
    assert_eq!(rule(&rig, &address(65), "One too many"), "SR-30");
    check("full", address(65), "One too many");
    assert_eq!(rig.app.signer_status().expect("status").allowlist.len(), 64);
}

#[test]
fn sr_79_the_list_and_the_removal_take_effect_with_the_clock() {
    let rig = rig();
    rig.app
        .enroll_destination(address(4), "Exchange".to_string())
        .expect("enrolled");
    let status = rig.app.signer_status().expect("status");
    let entry = &status.allowlist[0];
    assert_eq!(entry.enrolled_at, NOW);
    assert_eq!(entry.active_at, Some(NOW + ALLOWLIST_WAIT_SECS));
    assert_eq!(status.effective_time_now, NOW);
    // 24 hours later by the phone's clock the entry counts.
    rig.store.set_now(NOW + ALLOWLIST_WAIT_SECS);
    let later = rig.app.signer_status().expect("status");
    assert_eq!(later.effective_time_now, NOW + ALLOWLIST_WAIT_SECS);
    assert!(later.allowlist[0].active_at.expect("a time") <= later.effective_time_now);
    // Removal is at once, and it is stored.
    let saves = rig.store.saves();
    rig.app.remove_destination(address(4)).expect("removed");
    assert_eq!(rig.store.saves(), saves + 1);
    assert!(rig
        .app
        .signer_status()
        .expect("status")
        .allowlist
        .is_empty());
    assert_eq!(
        rig.app
            .remove_destination(address(4))
            .expect_err("not there")
            .rule(),
        "SR-30"
    );
}
