//! SR-33 and SR-35 to SR-43: the spend log, the ceilings and the fee.

use bitcoin::hashes::Hash;
use bitcoin::{OutPoint, ScriptBuf, Txid};

use super::kit::*;
use crate::{
    Ceiling, Ceilings, ChangeError, ChangeRefusal, FeeCeilings, LogEntry, OutputKind, Refusal,
    SignerState, FIRST_ALLOWLISTED, FIRST_FEE, FIRST_OTHER, MAX_FEE_RATE_SAT_PER_VB, MAX_FEE_SATS,
    MAX_MONEY_SATS, WEEK_SECS,
};

fn set_other(s: &mut SignerState, c: Ceilings) {
    s.set_ceilings(c, s.ceilings_allowlisted(), ok).unwrap();
}

fn set_allowlisted(s: &mut SignerState, c: Ceilings) {
    s.set_ceilings(s.ceilings_other(), c, ok).unwrap();
}

/// A state with `script` allowlisted and active at the returned time.
fn with_destination(script: &ScriptBuf) -> (SignerState, u64) {
    let mut s = state();
    s.enroll_destination(script.clone(), "Cash-out", NOW, ok)
        .unwrap();
    (s, NOW + 86_400)
}

#[test]
fn sr_35_other_destinations_per_transaction() {
    let s = state();
    assert!(s.review(&simple(100_000, 1_000).serialize(), NOW).is_ok());
    assert_refused(
        &s,
        &simple(100_001, 1_000),
        NOW,
        Refusal::OverCeiling {
            ceiling: Ceiling::OtherPerTx,
            amount: 100_001,
            limit: 100_000,
        },
        "SR-35",
    );
}

#[test]
fn sr_35_allowlisted_destinations_per_transaction() {
    let script = external(40);
    let (s, later) = with_destination(&script);
    assert!(s
        .review(
            &simple_to(script.clone(), 5_000_000, 1_000).serialize(),
            later
        )
        .is_ok());
    assert_refused(
        &s,
        &simple_to(script, 5_000_001, 1_000),
        later,
        Refusal::OverCeiling {
            ceiling: Ceiling::AllowlistedPerTx,
            amount: 5_000_001,
            limit: 5_000_000,
        },
        "SR-35",
    );
}

#[test]
fn sr_35_holds_the_two_kinds_apart() {
    // 90,000 to each kind is under each per-transaction ceiling.
    let script = external(40);
    let (s, later) = with_destination(&script);
    let p = build(&[coin(200_000)], &[pay(90_000), pay_to(script, 90_000)]);
    let r = review_ok(&s, &p, later);
    assert_eq!(
        (r.other_total_sats(), r.allowlisted_total_sats()),
        (90_000, 90_000)
    );
}

/// Pay `amount` (with a fee of 1,000) to other destinations.
fn other_tx(amount: u64) -> bitcoin::psbt::Psbt {
    simple(amount, 1_000)
}

#[test]
fn sr_36_other_destinations_and_fee_per_day() {
    let mut s = state();
    set_other(
        &mut s,
        Ceilings {
            per_tx: 500_000,
            per_day: 300_000,
            per_week: 10_000_000,
        },
    );
    sign_ok(&mut s, &other_tx(199_000), NOW);
    // 200,000 so far (amount and fee). 100,000 more is exactly the ceiling.
    assert!(s.review(&other_tx(99_000).serialize(), NOW).is_ok());
    assert_refused(
        &s,
        &other_tx(99_001),
        NOW,
        Refusal::OverCeiling {
            ceiling: Ceiling::OtherDay,
            amount: 300_001,
            limit: 300_000,
        },
        "SR-36",
    );
}

#[test]
fn sr_36_other_destinations_and_fee_per_week() {
    let mut s = state();
    set_other(
        &mut s,
        Ceilings {
            per_tx: 500_000,
            per_day: 10_000_000,
            per_week: 300_000,
        },
    );
    sign_ok(&mut s, &other_tx(199_000), NOW);
    assert!(s.review(&other_tx(99_000).serialize(), NOW).is_ok());
    assert_refused(
        &s,
        &other_tx(99_001),
        NOW,
        Refusal::OverCeiling {
            ceiling: Ceiling::OtherWeek,
            amount: 300_001,
            limit: 300_000,
        },
        "SR-36",
    );
}

#[test]
fn sr_36_allowlisted_destinations_per_day() {
    let script = external(40);
    let (mut s, later) = with_destination(&script);
    set_allowlisted(
        &mut s,
        Ceilings {
            per_tx: 5_000_000,
            per_day: 6_000_000,
            per_week: 30_000_000,
        },
    );
    sign_ok(&mut s, &simple_to(script.clone(), 4_000_000, 1_000), later);
    assert!(s
        .review(
            &simple_to(script.clone(), 2_000_000, 1_000).serialize(),
            later
        )
        .is_ok());
    assert_refused(
        &s,
        &simple_to(script, 2_000_001, 1_000),
        later,
        Refusal::OverCeiling {
            ceiling: Ceiling::AllowlistedDay,
            amount: 6_000_001,
            limit: 6_000_000,
        },
        "SR-36",
    );
}

#[test]
fn sr_36_allowlisted_destinations_per_week() {
    let script = external(40);
    let (mut s, later) = with_destination(&script);
    set_allowlisted(
        &mut s,
        Ceilings {
            per_tx: 5_000_000,
            per_day: 30_000_000,
            per_week: 6_000_000,
        },
    );
    sign_ok(&mut s, &simple_to(script.clone(), 4_000_000, 1_000), later);
    assert!(s
        .review(
            &simple_to(script.clone(), 2_000_000, 1_000).serialize(),
            later
        )
        .is_ok());
    assert_refused(
        &s,
        &simple_to(script, 2_000_001, 1_000),
        later,
        Refusal::OverCeiling {
            ceiling: Ceiling::AllowlistedWeek,
            amount: 6_000_001,
            limit: 6_000_000,
        },
        "SR-36",
    );
}

#[test]
fn sr_36_the_fee_counts_with_other_destinations_and_not_with_allowlisted() {
    let script = external(40);
    let (mut s, later) = with_destination(&script);
    sign_ok(&mut s, &simple(50_000, 5_000), later);
    let t = s.totals(later);
    assert_eq!((t.other_day, t.other_week), (55_000, 55_000));
    assert_eq!((t.allowlisted_day, t.allowlisted_week), (0, 0));
    sign_ok(&mut s, &simple_to(script, 1_000_000, 2_000), later);
    let t = s.totals(later);
    assert_eq!((t.other_day, t.other_week), (57_000, 57_000));
    assert_eq!(
        (t.allowlisted_day, t.allowlisted_week),
        (1_000_000, 1_000_000)
    );
}

#[test]
fn sr_36_the_review_says_what_would_remain_under_each_ceiling() {
    let script = external(40);
    let (mut s, later) = with_destination(&script);
    sign_ok(&mut s, &simple(20_000, 1_000), later);
    let p = build(
        &[coin(1_540_000)],
        &[pay(30_000), pay_to(external(40), 1_500_000)],
    );
    let r = review_ok(&s, &p, later);
    assert_eq!(r.fee_sats(), 10_000);
    let rem = r.remaining();
    assert_eq!(rem.other_per_tx, 100_000 - 30_000);
    assert_eq!(rem.other_day, 300_000 - (21_000 + 30_000 + 10_000));
    assert_eq!(rem.other_week, 1_000_000 - (21_000 + 30_000 + 10_000));
    assert_eq!(rem.allowlisted_per_tx, 5_000_000 - 1_500_000);
    assert_eq!(rem.allowlisted_day, 10_000_000 - 1_500_000);
    assert_eq!(rem.allowlisted_week, 30_000_000 - 1_500_000);
}

#[test]
fn sr_33_signing_the_same_transaction_again_counts_once() {
    let mut s = state();
    // The ceiling is exactly what one signature uses.
    set_other(
        &mut s,
        Ceilings {
            per_tx: 100_000,
            per_day: 51_000,
            per_week: 51_000,
        },
    );
    let p = other_tx(50_000);
    let first = sign_ok(&mut s, &p, NOW);
    let review = review_ok(&s, &p, NOW + 5);
    assert!(review.already_counted());
    let second = s.sign(review, key2(), NOW + 5, ok).unwrap();
    assert_eq!(second.psbt, first.psbt);
    assert_eq!(s.totals(NOW + 5).other_day, 51_000);
    assert_eq!(s.log().count(), 1);
    // The date of the entry is that of the first signature.
    assert_eq!(s.log().next().unwrap().time(), NOW);
    // The time seen still moves.
    assert_eq!(s.latest_time_seen(), NOW + 5);
}

#[test]
fn sr_33_a_fee_bump_counts_once_at_the_larger_amounts() {
    let mut s = state();
    set_other(
        &mut s,
        Ceilings {
            per_tx: 100_000,
            per_day: 65_000,
            per_week: 65_000,
        },
    );
    let c = coin(100_000);
    let a = build(std::slice::from_ref(&c), &[pay(60_000), change(0, 39_000)]);
    let b = build(&[c], &[pay(60_000), change(0, 35_000)]);
    let sa = sign_ok(&mut s, &a, NOW);
    assert_eq!(s.totals(NOW).other_day, 61_000);
    let r = review_ok(&s, &b, NOW + 60);
    assert!(!r.already_counted());
    assert_eq!(r.remaining().other_day, 0);
    let sb = s.sign(r, key2(), NOW + 60, ok).unwrap();
    assert_ne!(sa.txid, sb.txid);
    // 60,000 and the larger fee of 5,000 count once.
    assert_eq!(s.totals(NOW + 60).other_day, 65_000);
    let entries: Vec<_> = s.log().collect();
    assert_eq!(entries.len(), 1);
    let e = entries[0];
    assert_eq!(e.txid(), sb.txid);
    assert_eq!(e.time(), NOW + 60);
    assert_eq!(e.other_sats(), 60_000);
    assert_eq!(e.fee_sats(), 5_000);
    assert_eq!(e.inputs().len(), 1);
}

#[test]
fn sr_33_a_replacement_with_smaller_amounts_keeps_the_larger_ones() {
    let mut s = state();
    let c = coin(100_000);
    let a = build(std::slice::from_ref(&c), &[pay(60_000), change(0, 39_000)]);
    let b = build(&[c], &[pay(50_000), change(0, 47_000)]);
    sign_ok(&mut s, &a, NOW);
    sign_ok(&mut s, &b, NOW + 1);
    let e = s.log().next().unwrap();
    assert_eq!((e.other_sats(), e.fee_sats()), (60_000, 3_000));
    assert_eq!(s.log().count(), 1);
}

#[test]
fn sr_33_two_earlier_transactions_are_not_both_absorbed() {
    let mut s = state();
    let (x, y) = (coin(60_000), coin(60_000));
    let a = build(std::slice::from_ref(&x), &[pay(59_000)]);
    let b = build(std::slice::from_ref(&y), &[pay(58_000)]);
    let c = build(&[x, y], &[pay(99_000)]);
    let sa = sign_ok(&mut s, &a, NOW);
    let sb = sign_ok(&mut s, &b, NOW + 10);
    let sc = sign_ok(&mut s, &c, NOW + 20);
    // C is joined with B, the most recent, only. A stays in full.
    let ids: Vec<_> = s.log().map(|e| e.txid()).collect();
    assert_eq!(ids.len(), 2);
    assert!(ids.contains(&sa.txid) && ids.contains(&sc.txid));
    assert!(!ids.contains(&sb.txid));
    // A: 59,000 and fee 1,000. C: 99,000 and fee 21,000, the larger
    // amounts of C and B.
    let t = s.totals(NOW + 20);
    assert_eq!(t.other_day, (59_000 + 1_000) + (99_000 + 21_000));
}

#[test]
fn sr_33_with_equal_times_the_larger_transaction_id_is_the_one_joined() {
    let mut s = state();
    let (x, y) = (coin(60_000), coin(60_000));
    let a = build(std::slice::from_ref(&x), &[pay(59_000)]);
    let b = build(std::slice::from_ref(&y), &[pay(58_000)]);
    let c = build(&[x, y], &[pay(99_000)]);
    let sa = sign_ok(&mut s, &a, NOW);
    let sb = sign_ok(&mut s, &b, NOW);
    sign_ok(&mut s, &c, NOW);
    let (kept, joined) = if sa.txid.to_byte_array() > sb.txid.to_byte_array() {
        (sb.txid, sa.txid)
    } else {
        (sa.txid, sb.txid)
    };
    let ids: Vec<_> = s.log().map(|e| e.txid()).collect();
    assert!(ids.contains(&kept));
    assert!(!ids.contains(&joined));
}

#[test]
fn sr_33_a_transaction_that_covers_only_some_inputs_of_an_earlier_one_counts_in_full() {
    let mut s = state();
    let (x, y) = (coin(60_000), coin(60_000));
    let a = build(&[x.clone(), y], &[pay(99_000)]);
    let d = build(&[x], &[pay(59_000)]);
    sign_ok(&mut s, &a, NOW);
    sign_ok(&mut s, &d, NOW + 1);
    assert_eq!(s.log().count(), 2);
    assert_eq!(
        s.totals(NOW + 1).other_day,
        (99_000 + 21_000) + (59_000 + 1_000)
    );
}

#[test]
fn sr_33_transactions_with_no_input_in_common_count_in_full() {
    let mut s = state();
    sign_ok(&mut s, &other_tx(40_000), NOW);
    sign_ok(&mut s, &other_tx(40_000), NOW + 1);
    assert_eq!(s.log().count(), 2);
    assert_eq!(s.totals(NOW + 1).other_day, 82_000);
}

#[test]
fn sr_33_the_spend_log_refuses_when_it_has_no_room_for_one_more_entry() {
    let mut s = state();
    for n in 0..crate::MAX_LOG_ENTRIES as u32 {
        let mut id = [0u8; 32];
        id[..4].copy_from_slice(&(n + 1).to_be_bytes());
        s.log.insert(
            id,
            LogEntry {
                txid: Txid::from_byte_array(id),
                time: NOW,
                other_sats: 0,
                allowlisted_sats: 0,
                fee_sats: 0,
                inputs: vec![OutPoint {
                    txid: Txid::from_byte_array(id),
                    vout: 0,
                }],
            },
        );
    }
    s.latest_time_seen = NOW;
    assert_refused(&s, &other_tx(1_000), NOW, Refusal::SpendLogFull, "SR-33");
    // Entries a week old have aged out and make room.
    let later = NOW + WEEK_SECS;
    assert!(s.review(&other_tx(1_000).serialize(), later).is_ok());
}

#[test]
fn sr_33_the_spend_log_refuses_when_it_holds_too_many_inputs() {
    let mut s = state();
    for n in 0..1_000u32 {
        let mut id = [0u8; 32];
        id[..4].copy_from_slice(&(n + 1).to_be_bytes());
        let inputs = (0..100u32)
            .map(|v| OutPoint {
                txid: Txid::from_byte_array([9; 32]),
                vout: n * 100 + v,
            })
            .collect();
        s.log.insert(
            id,
            LogEntry {
                txid: Txid::from_byte_array(id),
                time: NOW,
                other_sats: 0,
                allowlisted_sats: 0,
                fee_sats: 0,
                inputs,
            },
        );
    }
    s.latest_time_seen = NOW;
    assert_refused(&s, &other_tx(1_000), NOW, Refusal::SpendLogFull, "SR-33");
}

#[test]
fn sr_37_ceilings_above_all_bitcoin_are_refused() {
    let mut s = state();
    let before = s.to_bytes();
    let big = Ceilings {
        per_tx: 1,
        per_day: MAX_MONEY_SATS + 1,
        per_week: 1,
    };
    let r = s.set_ceilings(big, FIRST_ALLOWLISTED, ok);
    assert_eq!(
        r,
        Err(ChangeError::Refused(ChangeRefusal::CeilingAboveMoney))
    );
    let r = s.set_ceilings(FIRST_OTHER, big, ok);
    assert_eq!(
        r,
        Err(ChangeError::Refused(ChangeRefusal::CeilingAboveMoney))
    );
    assert_eq!(ChangeRefusal::CeilingAboveMoney.rule(), "SR-37");
    assert_eq!(s.to_bytes(), before);
    let top = Ceilings {
        per_tx: MAX_MONEY_SATS,
        per_day: MAX_MONEY_SATS,
        per_week: MAX_MONEY_SATS,
    };
    assert!(s.set_ceilings(top, top, ok).is_ok());
}

#[test]
fn sr_37_lowering_and_raising_take_effect_at_once() {
    let mut s = state();
    let p = other_tx(50_000);
    assert!(s.review(&p.serialize(), NOW).is_ok());
    set_other(
        &mut s,
        Ceilings {
            per_tx: 10_000,
            per_day: 10_000,
            per_week: 10_000,
        },
    );
    assert!(matches!(
        s.review(&p.serialize(), NOW),
        Err(Refusal::OverCeiling {
            ceiling: Ceiling::OtherPerTx,
            ..
        })
    ));
    set_other(
        &mut s,
        Ceilings {
            per_tx: 60_000,
            per_day: 60_000,
            per_week: 60_000,
        },
    );
    assert!(s.review(&p.serialize(), NOW).is_ok());
}

#[test]
fn sr_38_the_first_values() {
    let s = state();
    assert_eq!(s.ceilings_other(), FIRST_OTHER);
    assert_eq!(s.ceilings_allowlisted(), FIRST_ALLOWLISTED);
    assert_eq!(s.fee_ceilings(), FIRST_FEE);
    assert_eq!(
        (
            FIRST_OTHER.per_tx,
            FIRST_OTHER.per_day,
            FIRST_OTHER.per_week
        ),
        (100_000, 300_000, 1_000_000)
    );
    assert_eq!(
        (
            FIRST_ALLOWLISTED.per_tx,
            FIRST_ALLOWLISTED.per_day,
            FIRST_ALLOWLISTED.per_week
        ),
        (5_000_000, 10_000_000, 30_000_000)
    );
    assert_eq!(
        (FIRST_FEE.sat_per_vb, FIRST_FEE.absolute_sats),
        (150, 200_000)
    );
}

#[test]
fn sr_39_restore_mode_starts_empty_with_the_first_values_and_they_apply() {
    let s = state();
    assert_eq!(s.receive_mark(), 0);
    assert_eq!(s.change_mark(), 0);
    assert_eq!(s.latest_time_seen(), 0);
    assert_eq!(s.allowlist().count(), 0);
    assert_eq!(s.log().count(), 0);
    // Registering again from the same description gives the same state.
    assert_eq!(s.to_bytes(), state().to_bytes());
    assert_refused(
        &s,
        &other_tx(100_001),
        NOW,
        Refusal::OverCeiling {
            ceiling: Ceiling::OtherPerTx,
            amount: 100_001,
            limit: 100_000,
        },
        "SR-35",
    );
}

/// The signed size in virtual bytes of a one-input, one-output payment.
fn one_in_one_out_vsize() -> u64 {
    review_ok(&state(), &simple(50_000, 0), NOW).vsize()
}

#[test]
fn sr_40_the_fee_rate_is_held_to_the_ceiling_without_division() {
    let vsize = one_in_one_out_vsize();
    assert!((100..200).contains(&vsize), "{vsize}");
    let mut s = state();
    s.set_fee_ceilings(
        FeeCeilings {
            sat_per_vb: 10,
            absolute_sats: 200_000,
        },
        ok,
    )
    .unwrap();
    let at = review_ok(&s, &simple(50_000, 10 * vsize), NOW);
    assert_eq!(at.fee_rate_ceil_sat_per_vb(), 10);
    assert_eq!(at.vsize(), vsize);
    assert_refused(
        &s,
        &simple(50_000, 10 * vsize + 1),
        NOW,
        Refusal::FeeRateTooHigh {
            fee: 10 * vsize + 1,
            vsize,
            limit: 10,
        },
        "SR-40",
    );
    // The rate is rounded up for the screen.
    let over = review_ok(&state(), &simple(50_000, 10 * vsize + 1), NOW);
    assert_eq!(over.fee_rate_ceil_sat_per_vb(), 11);
}

#[test]
fn sr_41_the_fee_is_held_to_the_absolute_ceiling() {
    let mut s = state();
    s.set_fee_ceilings(
        FeeCeilings {
            sat_per_vb: 1_000,
            absolute_sats: 1_000,
        },
        ok,
    )
    .unwrap();
    assert!(s.review(&simple(50_000, 1_000).serialize(), NOW).is_ok());
    assert_refused(
        &s,
        &simple(50_000, 1_001),
        NOW,
        Refusal::FeeTooHigh {
            fee: 1_001,
            limit: 1_000,
        },
        "SR-41",
    );
}

#[test]
fn sr_42_the_fee_ceilings_stay_inside_their_compiled_in_bounds() {
    assert_eq!(MAX_FEE_RATE_SAT_PER_VB, 1_000);
    assert_eq!(MAX_FEE_SATS, 2_000_000);
    let mut s = state();
    let before = s.to_bytes();
    for (fee, why) in [
        (
            FeeCeilings {
                sat_per_vb: 0,
                absolute_sats: 1,
            },
            ChangeRefusal::FeeRateOutOfBounds,
        ),
        (
            FeeCeilings {
                sat_per_vb: 1_001,
                absolute_sats: 1,
            },
            ChangeRefusal::FeeRateOutOfBounds,
        ),
        (
            FeeCeilings {
                sat_per_vb: 1,
                absolute_sats: 0,
            },
            ChangeRefusal::FeeOutOfBounds,
        ),
        (
            FeeCeilings {
                sat_per_vb: 1,
                absolute_sats: 2_000_001,
            },
            ChangeRefusal::FeeOutOfBounds,
        ),
    ] {
        let rule = why.rule();
        assert_eq!(s.set_fee_ceilings(fee, ok), Err(ChangeError::Refused(why)));
        assert!(rule == "SR-40" || rule == "SR-41");
    }
    assert_eq!(s.to_bytes(), before);
    for fee in [
        FeeCeilings {
            sat_per_vb: 1,
            absolute_sats: 1,
        },
        FeeCeilings {
            sat_per_vb: 1_000,
            absolute_sats: 2_000_000,
        },
    ] {
        assert!(s.set_fee_ceilings(fee, ok).is_ok());
        assert_eq!(s.fee_ceilings(), fee);
    }
}

fn consolidation(inputs: usize) -> bitcoin::psbt::Psbt {
    let coins: Vec<_> = (0..inputs).map(|_| coin(1_000)).collect();
    build(&coins, &[change(0, inputs as u64 * 1_000 - 1_000)])
}

#[test]
fn sr_43_the_signed_weight_is_held_to_the_limit() {
    let s = state();
    let ok_review = review_ok(&s, &consolidation(900), NOW);
    assert!(ok_review.weight() <= 400_000);
    let heavy = s
        .review(&consolidation(1_000).serialize(), NOW)
        .unwrap_err();
    assert!(
        matches!(heavy, Refusal::WeightTooHigh { limit: 400_000, weight } if weight > 400_000),
        "{heavy:?}"
    );
    assert_eq!(heavy.rule(), "SR-43");
}

/// A transaction of `inputs` inputs and one output per script, signed weight.
fn bare_weight(inputs: usize, outs: &[ScriptBuf]) -> u64 {
    let tx = bitcoin::Transaction {
        version: bitcoin::transaction::Version::TWO,
        lock_time: bitcoin::absolute::LockTime::ZERO,
        input: vec![bitcoin::TxIn::default(); inputs],
        output: outs
            .iter()
            .map(|s| bitcoin::TxOut {
                value: bitcoin::Amount::from_sat(1_000),
                script_pubkey: s.clone(),
            })
            .collect(),
    };
    crate::analysis::signed_weight(&tx)
}

/// Distinct payment scripts of four lengths (31, 32, 34 and 43 bytes).
fn script_of_kind(kind: usize, n: u8) -> ScriptBuf {
    match kind {
        0 => external(n),
        1 => ScriptBuf::new_p2sh(&bitcoin::ScriptHash::from_byte_array([n; 20])),
        2 => ScriptBuf::new_p2pkh(&bitcoin::PubkeyHash::from_byte_array([n; 20])),
        _ => ScriptBuf::new_p2wsh(&bitcoin::WScriptHash::from_byte_array([n; 32])),
    }
}

/// A transaction of signed weight exactly `target` (or the next weight above
/// it that can be built), made of paying outputs only so that no earlier
/// rule refuses it. Returns the transaction and its weight.
fn of_weight(target: u64) -> (bitcoin::psbt::Psbt, u64) {
    // An output adds 4 times its serialized size: 8 value bytes, a length
    // byte and the script.
    let weight_of = |kind: usize| {
        bare_weight(1, &[script_of_kind(kind, 0), script_of_kind(0, 1)])
            - bare_weight(1, &[script_of_kind(0, 1)])
    };
    let unit = [0, 1, 2, 3].map(weight_of);
    for wanted in target.. {
        for inputs in (700..=1_000).rev() {
            let base = bare_weight(inputs, &[script_of_kind(0, 0)]);
            let Some(left) = wanted.checked_sub(base) else {
                continue;
            };
            for kinds in [
                [0usize; 0].as_slice(),
                &[1],
                &[2],
                &[3],
                &[1, 2],
                &[1, 3],
                &[2, 3],
            ] {
                let extra: u64 = kinds.iter().map(|k| unit[*k]).sum();
                let Some(rest) = left.checked_sub(extra) else {
                    continue;
                };
                if rest % unit[0] != 0 || rest / unit[0] > 95 {
                    continue;
                }
                let plain = (rest / unit[0]) as usize;
                let mut outs: Vec<ScriptBuf> =
                    (0..=plain).map(|i| script_of_kind(0, i as u8)).collect();
                outs.extend(
                    kinds
                        .iter()
                        .enumerate()
                        .map(|(i, k)| script_of_kind(*k, 200 + i as u8)),
                );
                assert_eq!(bare_weight(inputs, &outs), wanted);
                let coins: Vec<_> = (0..inputs).map(|_| coin(1_000)).collect();
                let fee = 1_000;
                let others = outs.len() as u64 - 1;
                let first = inputs as u64 * 1_000 - fee - 1_000 * others;
                let outs: Vec<Out> = outs
                    .into_iter()
                    .enumerate()
                    .map(|(i, s)| pay_to(s, if i == 0 { first } else { 1_000 }))
                    .collect();
                return (build(&coins, &outs), wanted);
            }
        }
    }
    unreachable!()
}

#[test]
fn sr_43_a_transaction_of_exactly_the_weight_limit_is_not_refused_for_weight() {
    let s = state();
    let (exact, weight) = of_weight(400_000);
    assert_eq!(weight, 400_000);
    let r = s.review(&exact.serialize(), NOW);
    assert!(!matches!(r, Err(Refusal::WeightTooHigh { .. })), "{r:?}");
    if let Ok(review) = r {
        assert_eq!(review.weight(), 400_000);
    }
    // The next weight above it that can be built is refused.
    let (above, weight) = of_weight(400_001);
    assert!(weight > 400_000 && weight < 400_100);
    assert_refused(
        &s,
        &above,
        NOW,
        Refusal::WeightTooHigh {
            weight,
            limit: 400_000,
        },
        "SR-43",
    );
}

#[test]
fn outputs_kind_shows_in_the_review_of_a_mixed_payment() {
    let script = external(40);
    let (s, later) = with_destination(&script);
    let p = build(
        &[coin(300_000)],
        &[pay(50_000), pay_to(script, 100_000), change(0, 149_000)],
    );
    let r = review_ok(&s, &p, later);
    let kinds: Vec<_> = r.outputs().iter().map(|o| o.kind.clone()).collect();
    assert_eq!(
        kinds,
        vec![
            OutputKind::Other,
            OutputKind::Allowlisted {
                label: "Cash-out".into()
            },
            OutputKind::Change { index: 0 }
        ]
    );
}
