//! SR-20 to SR-25: outputs.

use std::collections::BTreeMap;

use bitcoin::hashes::Hash;
use bitcoin::key::TweakedPublicKey;
use bitcoin::psbt::Psbt;
use bitcoin::script::PushBytesBuf;
use bitcoin::{
    PubkeyHash, PublicKey, ScriptBuf, ScriptHash, WScriptHash, WitnessProgram, WitnessVersion,
};
use openbmt_core::wallet::Keychain;

use super::kit::*;
use crate::{
    Ceiling, Ceilings, ChangeError, OutputKind, Refusal, FIRST_ALLOWLISTED, MAX_MONEY_SATS,
};

fn key(slot: usize, chain: Keychain, index: u32) -> bitcoin::secp256k1::PublicKey {
    kit().wallet.derived_keys(chain, index).unwrap()[slot].inner
}

#[test]
fn sr_20_an_honest_change_output_is_change() {
    let s = state();
    let p = build(&[coin(100_000)], &[pay(50_000), change(0, 49_000)]);
    let r = review_ok(&s, &p, NOW);
    assert_eq!(r.change(), Some((0, 49_000)));
    assert_eq!(r.outputs()[0].kind, OutputKind::Other);
    assert_eq!(r.outputs()[1].kind, OutputKind::Change { index: 0 });
    assert_eq!(r.other_total_sats(), 50_000);
    assert!(!r.moves_coins_inside_wallet());
}

#[test]
fn sr_20_change_hints_on_a_script_that_is_not_the_derived_one_are_not_change() {
    let s = state();
    let mut stranger = pay_to(external(3), 49_000);
    stranger.change = Some(0);
    let p = build(&[coin(100_000)], &[pay(50_000), stranger]);
    let r = review_ok(&s, &p, NOW);
    assert_eq!(r.change(), None);
    assert_eq!(r.outputs()[1].kind, OutputKind::Other);
    assert_eq!(r.other_total_sats(), 99_000);
    // Hints for index 0 on the script of index 1 are no better.
    let mut wrong_index = change(1, 49_000);
    wrong_index.change = Some(0);
    let p = build(&[coin(100_000)], &[pay(50_000), wrong_index]);
    let r = review_ok(&s, &p, NOW);
    assert_eq!(r.change(), None);
    assert_eq!(r.outputs()[1].kind, OutputKind::Other);
}

#[test]
fn sr_20_refuses_an_output_hint_with_the_wrong_key() {
    let s = state();
    let slot = &kit().wallet.slots()[0];
    let foreign = bitcoin::bip32::Xpriv::new_master(NET, &[9; 32]).unwrap();
    let foreign = bitcoin::bip32::Xpub::from_priv(crate::crypto::secp(), &foreign).public_key;
    let mut p = build(&[coin(100_000)], &[pay(50_000), change(0, 49_000)]);
    p.outputs[1].bip32_derivation.insert(
        foreign,
        (
            slot.origin_fingerprint,
            "m/48'/1'/0'/2'/1/0".parse().unwrap(),
        ),
    );
    assert_refused(
        &s,
        &p,
        NOW,
        Refusal::OutputHintKeyMismatch { output: 1 },
        "SR-20",
    );
}

#[test]
fn sr_20_refuses_output_hints_that_disagree() {
    let s = state();
    let mut p = build(&[coin(100_000)], &[pay(50_000), change(0, 49_000)]);
    let a = key(0, Keychain::Change, 0);
    let b = key(1, Keychain::Change, 1);
    let mut mixed = BTreeMap::new();
    mixed.insert(a, hints(Keychain::Change, 0)[&a].clone());
    mixed.insert(b, hints(Keychain::Change, 1)[&b].clone());
    p.outputs[1].bip32_derivation = mixed;
    assert_refused(
        &s,
        &p,
        NOW,
        Refusal::OutputHintsDisagree { output: 1 },
        "SR-20",
    );
}

#[test]
fn sr_20_change_is_proved_by_derivation_not_by_its_place() {
    // The spend library may put the change anywhere among the outputs
    // (SP-26): first, in the middle or last, the signer shows it as change.
    let s = state();
    let layouts: [[Out; 3]; 3] = [
        [change(0, 30_000), pay(30_000), pay_to(external(2), 39_000)],
        [pay(30_000), change(0, 30_000), pay_to(external(2), 39_000)],
        [pay(30_000), pay_to(external(2), 39_000), change(0, 30_000)],
    ];
    for (at, outs) in layouts.iter().enumerate() {
        let p = build(&[coin(100_000)], outs);
        let r = review_ok(&s, &p, NOW);
        assert_eq!(r.change(), Some((0, 30_000)), "change at {at}");
        for (n, out) in r.outputs().iter().enumerate() {
            let expect = if n == at {
                OutputKind::Change { index: 0 }
            } else {
                OutputKind::Other
            };
            assert_eq!(out.kind, expect, "output {n}, change at {at}");
        }
        assert_eq!(r.other_total_sats(), 69_000);
    }
}

#[test]
fn sr_20_with_two_qualifying_outputs_the_first_is_change() {
    let mut s = state();
    let p = build(
        &[coin(100_000)],
        &[change(0, 20_000), change(1, 20_000), pay(59_000)],
    );
    let r = review_ok(&s, &p, NOW);
    assert_eq!(r.change(), Some((0, 20_000)));
    assert_eq!(r.outputs()[0].kind, OutputKind::Change { index: 0 });
    assert_eq!(r.outputs()[1].kind, OutputKind::Other);
    assert_eq!(r.other_total_sats(), 79_000);
    sign_ok(&mut s, &p, NOW);
    // Only the output taken as change moves the mark.
    assert_eq!(s.change_mark(), 1);
}

#[test]
fn sr_34_refuses_a_change_index_at_or_past_the_mark_plus_twenty() {
    let mut s = state();
    let p = build(&[coin(100_000)], &[pay(50_000), change(20, 49_000)]);
    assert_refused(
        &s,
        &p,
        NOW,
        Refusal::ChangeIndexTooFar {
            output: 1,
            index: 20,
            limit: 20,
        },
        "SR-34",
    );
    let p19 = build(&[coin(100_000)], &[pay(50_000), change(19, 49_000)]);
    assert_eq!(review_ok(&s, &p19, NOW).change(), Some((19, 49_000)));
    // The owner raises the mark on the Key app.
    s.raise_change_mark(5, ok).unwrap();
    assert!(s.review(&p.serialize(), NOW).is_ok());
    let p25 = build(&[coin(100_000)], &[pay(50_000), change(25, 49_000)]);
    assert_refused(
        &s,
        &p25,
        NOW,
        Refusal::ChangeIndexTooFar {
            output: 1,
            index: 25,
            limit: 25,
        },
        "SR-34",
    );
}

#[test]
fn sr_34_a_second_change_output_far_ahead_is_refused_too() {
    let s = state();
    let p = build(
        &[coin(100_000)],
        &[change(0, 20_000), change(500, 20_000), pay(59_000)],
    );
    assert_refused(
        &s,
        &p,
        NOW,
        Refusal::ChangeIndexTooFar {
            output: 1,
            index: 500,
            limit: 20,
        },
        "SR-34",
    );
}

#[test]
fn sr_34_signing_moves_the_mark_to_one_past_the_index_used() {
    let mut s = state();
    sign_ok(
        &mut s,
        &build(&[coin(100_000)], &[pay(50_000), change(7, 49_000)]),
        NOW,
    );
    assert_eq!(s.change_mark(), 8);
    // A lower index later does not lower it.
    sign_ok(
        &mut s,
        &build(&[coin(100_000)], &[pay(40_000), change(2, 59_000)]),
        NOW,
    );
    assert_eq!(s.change_mark(), 8);
    // The range moved with it: 8 + 19 is the last one allowed.
    let p = build(&[coin(100_000)], &[pay(40_000), change(27, 59_000)]);
    assert!(s.review(&p.serialize(), NOW).is_ok());
    let p = build(&[coin(100_000)], &[pay(40_000), change(28, 59_000)]);
    assert!(matches!(
        s.review(&p.serialize(), NOW),
        Err(Refusal::ChangeIndexTooFar { index: 28, .. })
    ));
}

#[test]
fn sr_21_an_active_allowlist_entry_is_allowlisted() {
    let mut s = state();
    let script = external(40);
    s.enroll_destination(script.clone(), "Exchange", NOW, ok)
        .unwrap();
    let later = NOW + 86_400;
    let p = simple_to(script, 1_000_000, 1_000);
    let r = review_ok(&s, &p, later);
    assert_eq!(
        r.outputs()[0].kind,
        OutputKind::Allowlisted {
            label: "Exchange".into()
        }
    );
    assert_eq!(r.allowlisted_total_sats(), 1_000_000);
    assert_eq!(r.other_total_sats(), 0);
}

#[test]
fn sr_22_accepts_the_five_script_kinds() {
    let s = state();
    let xonly = key(0, Keychain::Receive, 0).x_only_public_key().0;
    let pk = PublicKey::new(key(0, Keychain::Receive, 0));
    let _ = pk;
    let scripts = [
        ("p2wpkh", external(1)),
        (
            "p2wsh",
            ScriptBuf::new_p2wsh(&WScriptHash::from_byte_array([1; 32])),
        ),
        (
            "p2tr",
            ScriptBuf::new_p2tr_tweaked(TweakedPublicKey::dangerous_assume_tweaked(xonly)),
        ),
        (
            "p2pkh",
            ScriptBuf::new_p2pkh(&PubkeyHash::from_byte_array([1; 20])),
        ),
        (
            "p2sh",
            ScriptBuf::new_p2sh(&ScriptHash::from_byte_array([1; 20])),
        ),
    ];
    for (name, script) in scripts {
        let r = review_ok(&s, &simple_to(script, 50_000, 1_000), NOW);
        assert_eq!(r.outputs()[0].kind, OutputKind::Other, "{name}");
        assert!(r.outputs()[0].address.is_some(), "{name}");
    }
}

#[test]
fn sr_22_refuses_every_other_script() {
    let s = state();
    let k0 = PublicKey::new(key(0, Keychain::Receive, 0));
    let mut data = PushBytesBuf::new();
    data.extend_from_slice(b"hello").unwrap();
    let bare_multisig = bitcoin::script::Builder::new()
        .push_opcode(bitcoin::opcodes::all::OP_PUSHNUM_1)
        .push_key(&k0)
        .push_opcode(bitcoin::opcodes::all::OP_PUSHNUM_1)
        .push_opcode(bitcoin::opcodes::all::OP_CHECKMULTISIG)
        .into_script();
    let v2 =
        ScriptBuf::new_witness_program(&WitnessProgram::new(WitnessVersion::V2, &[1; 32]).unwrap());
    let scripts = [
        ("data output", ScriptBuf::new_op_return(&data)),
        ("bare multisig", bare_multisig),
        ("bare public key", ScriptBuf::new_p2pk(&k0)),
        ("witness version 2", v2),
        ("OP_TRUE", ScriptBuf::from_bytes(vec![0x51])),
        ("empty", ScriptBuf::new()),
    ];
    for (name, script) in scripts {
        assert_refused(
            &s,
            &simple_to(script, 50_000, 1_000),
            NOW,
            Refusal::OutputScriptKind { output: 0 },
            "SR-22",
        );
        let _ = name;
    }
}

#[test]
fn sr_23_an_output_to_a_receive_address_of_the_wallet_is_another_destination() {
    let s = state();
    let script = kit().wallet.receive_script(5).unwrap();
    let mut p = simple_to(script, 50_000, 1_000);
    // Even with honest receive-chain hints on it.
    p.outputs[0].bip32_derivation = hints(Keychain::Receive, 5);
    let r = review_ok(&s, &p, NOW);
    assert_eq!(r.outputs()[0].kind, OutputKind::Other);
    assert_eq!(r.change(), None);
    assert_eq!(r.other_total_sats(), 50_000);
}

#[test]
fn sr_23_a_second_change_output_counts_against_the_other_ceilings() {
    let s = state();
    let p = build(&[coin(100_000)], &[change(0, 40_000), change(1, 59_000)]);
    // 59,000 goes to "other" and fits under 100,000.
    assert_eq!(review_ok(&s, &p, NOW).other_total_sats(), 59_000);
    let p = build(&[coin(300_000)], &[change(0, 40_000), change(1, 259_000)]);
    assert!(matches!(
        s.review(&p.serialize(), NOW),
        Err(Refusal::OverCeiling { limit: 100_000, .. })
    ));
}

#[test]
fn sr_24_refuses_a_zero_amount_output() {
    let s = state();
    let p = build(&[coin(51_000)], &[pay(50_000), pay(0)]);
    assert_refused(
        &s,
        &p,
        NOW,
        Refusal::OutputAmountZero { output: 1 },
        "SR-24",
    );
}

#[test]
fn sr_24_refuses_an_output_amount_above_all_bitcoin() {
    let s = state();
    let big = MAX_MONEY_SATS + 1;
    let p = build(&[coin(big + 1_000)], &[pay(big)]);
    // The input is impossible too, and is reported first.
    assert_eq!(
        refused(&s, &p, NOW),
        Refusal::InputAmountTooLarge {
            input: 0,
            amount: big + 1_000
        }
    );
    let p = build(&[coin(MAX_MONEY_SATS)], &[pay(50_000)]);
    let mut p: Psbt = p;
    p.unsigned_tx.output[0].value = bitcoin::Amount::from_sat(big);
    assert_refused(
        &s,
        &p,
        NOW,
        Refusal::OutputAmountTooLarge {
            output: 0,
            amount: big,
        },
        "SR-24",
    );
}

#[test]
fn sr_24_an_output_of_exactly_all_bitcoin_is_not_refused_as_too_large() {
    let s = state();
    // An input of exactly all bitcoin pays one output of exactly all bitcoin.
    // Something later may refuse it, but not for the size of the output.
    let p = build(&[coin(MAX_MONEY_SATS)], &[pay(MAX_MONEY_SATS)]);
    let r = s.review(&p.serialize(), NOW);
    assert!(
        !matches!(r, Err(Refusal::OutputAmountTooLarge { .. })),
        "{r:?}"
    );
    // One satoshi more is refused as too large (the input stays at the limit).
    let mut p = build(&[coin(MAX_MONEY_SATS)], &[pay(MAX_MONEY_SATS)]);
    p.unsigned_tx.output[0].value = bitcoin::Amount::from_sat(MAX_MONEY_SATS + 1);
    assert_refused(
        &s,
        &p,
        NOW,
        Refusal::OutputAmountTooLarge {
            output: 0,
            amount: MAX_MONEY_SATS + 1,
        },
        "SR-24",
    );
}

#[test]
fn sr_24_refuses_outputs_that_add_up_to_more_than_the_inputs() {
    let s = state();
    let p = build(&[coin(50_000)], &[pay(30_000), pay(20_001)]);
    assert_refused(
        &s,
        &p,
        NOW,
        Refusal::OutputsExceedInputs {
            outputs: 50_001,
            inputs: 50_000,
        },
        "SR-24",
    );
    // Equal is allowed: a transaction with no fee.
    let p = build(&[coin(50_000)], &[pay(30_000), pay(20_000)]);
    assert_eq!(review_ok(&s, &p, NOW).fee_sats(), 0);
}

#[test]
fn sr_25_a_transaction_whose_only_output_is_change_is_allowed() {
    let mut s = state();
    // 300,000 would be far above the 100,000 per-transaction ceiling for
    // other destinations, but nothing leaves the wallet except the fee.
    let p = build(
        &[coin(100_000), coin(100_000), coin(100_000)],
        &[change(0, 299_000)],
    );
    let r = review_ok(&s, &p, NOW);
    assert!(r.moves_coins_inside_wallet());
    assert_eq!(r.change(), Some((0, 299_000)));
    assert_eq!(r.other_total_sats(), 0);
    assert_eq!(r.fee_sats(), 1_000);
    sign_ok(&mut s, &p, NOW);
    // Its fee is counted with the other destinations.
    assert_eq!(s.totals(NOW).other_day, 1_000);
    assert_eq!(s.totals(NOW).other_week, 1_000);
}

#[test]
fn sr_25_the_fee_of_a_consolidation_is_held_to_the_day_and_week_ceilings() {
    let mut s = state();
    s.set_ceilings(
        Ceilings {
            per_tx: 0,
            per_day: 2_000,
            per_week: 2_000,
        },
        FIRST_ALLOWLISTED,
        ok,
    )
    .unwrap();
    // A fee of 2,001 is one satoshi above the day ceiling of 2,000.
    let over = build(&[coin(100_000)], &[change(0, 97_999)]);
    assert_refused(
        &s,
        &over,
        NOW,
        Refusal::OverCeiling {
            ceiling: Ceiling::OtherDay,
            amount: 2_001,
            limit: 2_000,
        },
        "SR-36",
    );
    // A fee of exactly 2,000 fits. The per-transaction ceiling of 0 does
    // not stand in the way: nothing goes to another destination.
    let fits = build(&[coin(100_000)], &[change(0, 98_000)]);
    sign_ok(&mut s, &fits, NOW);
    assert_eq!(s.totals(NOW).other_day, 2_000);
    assert_eq!(s.totals(NOW).other_week, 2_000);
    // The day is full now, for a second one and for a payment alike.
    let again = build(&[coin(100_000)], &[change(1, 99_000)]);
    assert_eq!(
        refused(&s, &again, NOW).rule(),
        "SR-36",
        "a second consolidation"
    );
    // A day later the day has room again but the week has not.
    assert_refused(
        &s,
        &again,
        NOW + crate::DAY_SECS,
        Refusal::OverCeiling {
            ceiling: Ceiling::OtherWeek,
            amount: 3_000,
            limit: 2_000,
        },
        "SR-36",
    );
}

#[test]
fn sr_25_a_consolidation_is_held_to_the_fee_rules() {
    let mut s = state();
    s.set_fee_ceilings(
        crate::FeeCeilings {
            sat_per_vb: 1_000,
            absolute_sats: 1_000,
        },
        ok,
    )
    .unwrap();
    let p = build(&[coin(101_001)], &[change(0, 100_000)]);
    assert_refused(
        &s,
        &p,
        NOW,
        Refusal::FeeTooHigh {
            fee: 1_001,
            limit: 1_000,
        },
        "SR-41",
    );
    let p = build(&[coin(101_000)], &[change(0, 100_000)]);
    assert!(s.review(&p.serialize(), NOW).is_ok());
}

#[test]
fn sr_25_change_back_with_a_payment_is_not_a_consolidation() {
    let s = state();
    let p = build(&[coin(100_000)], &[pay(10_000), change(0, 89_000)]);
    assert!(!review_ok(&s, &p, NOW).moves_coins_inside_wallet());
}

#[test]
fn change_error_type_is_usable() {
    // The owner calls return `ChangeError`; a refusal there never reaches
    // the store.
    let mut s = state();
    let r: Result<(), ChangeError<&str>> = s.raise_change_mark(0, fail);
    assert!(matches!(r, Err(ChangeError::Refused(_))));
}
