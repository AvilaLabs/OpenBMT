//! Attacks from outside: call sequences and files built to make the signer
//! do what the rules forbid.

use std::collections::BTreeSet;

use bitcoin::hashes::Hash;
use bitcoin::psbt::Psbt;
use bitcoin::secp256k1::SecretKey;
use bitcoin::{OutPoint, ScriptBuf, Txid};
use openbmt_core::batch::{Approver, BatchBuilder, BatchPolicy};
use openbmt_core::identity::Identity;
use openbmt_core::wallet::Keychain;

use super::kit::*;
use crate::{
    Ceilings, FeeCeilings, LogEntry, OutputKind, Refusal, SignError, SignerState, DAY_SECS,
    MAX_MONEY_SATS, WEEK_SECS,
};

/// Ceilings so high that only the fee rules and the log matter.
fn open_ceilings(s: &mut SignerState) {
    let all = Ceilings {
        per_tx: MAX_MONEY_SATS,
        per_day: MAX_MONEY_SATS,
        per_week: MAX_MONEY_SATS,
    };
    s.set_ceilings(all, all, ok).unwrap();
    s.set_fee_ceilings(
        FeeCeilings {
            sat_per_vb: 1_000,
            absolute_sats: 2_000_000,
        },
        ok,
    )
    .unwrap();
}

/// The same bytes after a trip through the snapshot, or a panic if the
/// snapshot of a live state is refused.
fn survives_snapshot(s: &SignerState) {
    let bytes = s.to_bytes();
    let back = SignerState::from_bytes(&bytes, NET, &kit().xpubs[OWN])
        .unwrap_or_else(|e| panic!("a live state was refused on reading: {e:?}"));
    assert_eq!(back.to_bytes(), bytes);
}

/// The coins pay `amount` to `to`; the rest is the fee.
fn spending(coins: &[Coin], to: ScriptBuf, amount: u64) -> Psbt {
    build(coins, &[pay_to(to, amount)])
}

fn day(s: &SignerState, now: u64) -> (u64, u64) {
    let t = s.totals(now);
    (t.other_day, t.allowlisted_day)
}

fn week(s: &SignerState, now: u64) -> (u64, u64) {
    let t = s.totals(now);
    (t.other_week, t.allowlisted_week)
}

/// Enroll `script` and return the first time it counts as allowlisted.
fn enroll_active(s: &mut SignerState, script: &ScriptBuf, at: u64) -> u64 {
    s.enroll_destination(script.clone(), "Cash-out", at, ok)
        .unwrap();
    at + DAY_SECS
}

// ---- reading the file -----------------------------------------------------

#[test]
fn sr_01_refuses_bytes_after_the_end_of_the_file() {
    let s = state();
    let mut bytes = simple(50_000, 1_000).serialize();
    bytes.extend_from_slice(&[0, 0, 0]);
    assert_eq!(s.review(&bytes, NOW).unwrap_err(), Refusal::NotAPsbt);
}

#[test]
fn sr_05_refuses_an_unsigned_transaction_that_already_carries_a_script() {
    let s = state();
    let mut p = simple(50_000, 1_000);
    p.unsigned_tx.input[0].script_sig = ScriptBuf::from_bytes(vec![0x51]);
    assert_eq!(
        s.review(&p.serialize(), NOW).unwrap_err(),
        Refusal::NotAPsbt
    );
}

#[test]
fn sr_14_refuses_a_high_s_signature_of_a_wallet_key() {
    let s = state();
    let mut p = simple(50_000, 1_000);
    sign_as(&mut p, 0);
    let (key, sig) = p.inputs[0]
        .partial_sigs
        .iter()
        .next()
        .map(|(k, v)| (*k, *v))
        .unwrap();
    // The same signature with S replaced by n - S: valid ECDSA, not low S.
    let mut compact = sig.signature.serialize_compact();
    let s_scalar = SecretKey::from_slice(&compact[32..]).unwrap();
    compact[32..].copy_from_slice(&s_scalar.negate().secret_bytes());
    let high = bitcoin::secp256k1::ecdsa::Signature::from_compact(&compact).unwrap();
    assert_ne!(high, sig.signature);
    p.inputs[0].partial_sigs.insert(
        key,
        bitcoin::ecdsa::Signature {
            signature: high,
            sighash_type: sig.sighash_type,
        },
    );
    assert_eq!(
        s.review(&p.serialize(), NOW).unwrap_err(),
        Refusal::BadSignature { input: 0 }
    );
}

// ---- hints (SR-12, SR-20, SR-23) -------------------------------------------

#[test]
fn sr_20_a_change_script_without_hints_is_another_destination_and_moves_no_mark() {
    let mut s = state();
    let mut bare = change(0, 49_000);
    bare.change = None;
    let p = build(&[coin(100_000)], &[pay(50_000), bare]);
    let r = review_ok(&s, &p, NOW);
    assert_eq!(r.change(), None);
    assert_eq!(r.outputs()[1].kind, OutputKind::Other);
    assert_eq!(r.other_total_sats(), 99_000);
    sign_ok(&mut s, &p, NOW);
    assert_eq!(s.change_mark(), 0);
}

#[test]
fn sr_20_hints_naming_the_receive_keychain_never_make_change() {
    // The script is the change script of index 0, but the hints (right keys
    // for receive index 0) name keychain 0.
    let mut s = state();
    let mut p = build(&[coin(100_000)], &[pay(50_000), change(0, 49_000)]);
    p.outputs[1].bip32_derivation = hints(Keychain::Receive, 0);
    let r = review_ok(&s, &p, NOW);
    assert_eq!(r.change(), None);
    assert_eq!(r.outputs()[1].kind, OutputKind::Other);
    sign_ok(&mut s, &p, NOW);
    assert_eq!(s.change_mark(), 0);
}

#[test]
fn sr_20_far_change_hints_on_a_foreign_script_are_not_a_refusal() {
    // Both conditions of SR-20 must hold before the bound applies.
    let s = state();
    let mut far = pay_to(external(7), 49_000);
    far.change = Some(500);
    let p = build(&[coin(100_000)], &[pay(50_000), far]);
    let r = review_ok(&s, &p, NOW);
    assert_eq!(r.outputs()[1].kind, OutputKind::Other);
}

#[test]
fn sr_20_a_change_script_that_is_also_allowlisted_is_change_once_and_allowlisted_after() {
    let mut s = state();
    let script = kit().wallet.change_script(3).unwrap();
    let at = enroll_active(&mut s, &script, NOW);
    let p = build(
        &[coin(300_000)],
        &[change(3, 100_000), pay_to(script, 100_000), pay(90_000)],
    );
    let r = review_ok(&s, &p, at);
    assert_eq!(r.outputs()[0].kind, OutputKind::Change { index: 3 });
    assert!(matches!(
        r.outputs()[1].kind,
        OutputKind::Allowlisted { .. }
    ));
    assert_eq!(r.outputs()[2].kind, OutputKind::Other);
    assert_eq!(r.allowlisted_total_sats(), 100_000);
    assert_eq!(r.other_total_sats(), 90_000);
}

// ---- SR-33 ------------------------------------------------------------------

#[test]
fn sr_33_a_chain_of_replacements_counts_once_at_the_largest_of_each_amount() {
    let mut s = state();
    s.set_fee_ceilings(
        FeeCeilings {
            sat_per_vb: 1_000,
            absolute_sats: 200_000,
        },
        ok,
    )
    .unwrap();
    let c = coin(200_000);
    // 90,000 with fee 1,000 -> 50,000 with fee 41,000 -> 10,000 with 81,000.
    let a = build(std::slice::from_ref(&c), &[pay(90_000), change(0, 109_000)]);
    let b = build(std::slice::from_ref(&c), &[pay(50_000), change(0, 109_000)]);
    let cc = build(std::slice::from_ref(&c), &[pay(10_000), change(0, 109_000)]);
    // Same change amount makes the fees 1,000 / 41,000 / 81,000.
    let mut last = 0;
    for (p, want) in [(&a, 91_000), (&b, 90_000 + 41_000), (&cc, 90_000 + 81_000)] {
        sign_ok(&mut s, p, NOW);
        let now = s.totals(NOW).other_day;
        assert_eq!(now, want);
        assert!(now >= last, "signing lowered the day total");
        last = now;
    }
    assert_eq!(s.log().count(), 1);
    survives_snapshot(&s);
}

#[test]
fn sr_33_a_replacement_a_day_later_still_counts_the_larger_earlier_amount_once() {
    let mut s = state();
    let c = coin(100_000);
    let a = build(std::slice::from_ref(&c), &[pay(90_000), change(0, 9_000)]);
    let b = build(std::slice::from_ref(&c), &[pay(80_000), change(0, 19_000)]);
    sign_ok(&mut s, &a, NOW);
    let later = NOW + DAY_SECS;
    assert_eq!(day(&s, later), (0, 0));
    assert_eq!(week(&s, later), (91_000, 0));
    sign_ok(&mut s, &b, later);
    // Dated at the later signature, at the larger of each amount: the 90,000
    // of the earlier one and the fee of 1,000.
    assert_eq!(day(&s, later), (91_000, 0));
    assert_eq!(week(&s, later), (91_000, 0));
    assert_eq!(s.log().count(), 1);
}

#[test]
fn sr_33_a_replacement_after_a_clock_correction_joins_the_re_dated_entry() {
    let mut s = state();
    let c = coin(100_000);
    let a = build(std::slice::from_ref(&c), &[pay(60_000), change(0, 39_000)]);
    let b = build(&[c], &[pay(60_000), change(0, 35_000)]);
    let far = NOW + 100_000;
    sign_ok(&mut s, &a, far);
    s.correct_clock(NOW, ok).unwrap();
    assert_eq!(s.log().next().unwrap().time(), NOW);
    sign_ok(&mut s, &b, NOW + 5);
    assert_eq!(s.log().count(), 1);
    assert_eq!(day(&s, NOW + 5), (65_000, 0));
    assert_eq!(s.log().next().unwrap().time(), NOW + 5);
}

#[test]
fn sr_33_the_same_transaction_is_free_until_the_week_is_over_and_not_after() {
    let mut s = state();
    let p = simple(50_000, 1_000);
    sign_ok(&mut s, &p, NOW);
    // Nothing may be sent any more, but the same file still gets its
    // signatures while the first one is in the log.
    s.set_ceilings(
        Ceilings {
            per_tx: 0,
            per_day: 0,
            per_week: 0,
        },
        s.ceilings_allowlisted(),
        ok,
    )
    .unwrap();
    let r = review_ok(&s, &p, NOW + WEEK_SECS - 1);
    assert!(r.already_counted());
    s.sign(r, key2(), NOW + WEEK_SECS - 1, ok).unwrap();
    assert_eq!(s.log().count(), 1);
    assert_eq!(s.log().next().unwrap().time(), NOW);
    assert!(matches!(
        s.review(&p.serialize(), NOW + WEEK_SECS).unwrap_err(),
        Refusal::OverCeiling { .. }
    ));
}

#[test]
fn sr_33_a_replacement_that_changes_the_kind_of_an_output_counts_both_kinds_at_their_larger() {
    let script = external(40);
    let mut s = state();
    let at = enroll_active(&mut s, &script, NOW);
    let c = coin(100_000);
    // Earlier: 90,000 to the allowlisted destination.
    let a = build(
        std::slice::from_ref(&c),
        &[pay_to(script.clone(), 90_000), change(0, 9_000)],
    );
    sign_ok(&mut s, &a, at);
    assert_eq!(day(&s, at), (1_000, 90_000));
    // The destination is removed; the replacement pays the same script, now
    // another destination.
    s.remove_destination(&script, ok).unwrap();
    let b = build(&[c], &[pay_to(script, 90_000), change(0, 8_000)]);
    sign_ok(&mut s, &b, at + 1);
    let e = s.log().next().unwrap();
    assert_eq!(s.log().count(), 1);
    assert_eq!(
        (e.other_sats(), e.allowlisted_sats(), e.fee_sats()),
        (90_000, 90_000, 2_000)
    );
}

#[test]
fn sr_33_a_replacement_to_an_allowlisted_destination_keeps_the_earlier_other_amount() {
    let script = external(40);
    let mut s = state();
    let at = enroll_active(&mut s, &script, NOW);
    let c = coin(100_000);
    let a = build(
        std::slice::from_ref(&c),
        &[pay_to(external(2), 90_000), change(0, 9_000)],
    );
    sign_ok(&mut s, &a, at);
    let b = build(&[c], &[pay_to(script, 90_000), change(0, 9_000)]);
    sign_ok(&mut s, &b, at + 1);
    let e = s.log().next().unwrap();
    assert_eq!(
        (e.other_sats(), e.allowlisted_sats(), e.fee_sats()),
        (90_000, 90_000, 1_000)
    );
}

#[test]
fn sr_33_a_different_output_of_the_same_earlier_transaction_is_not_the_same_input() {
    // Two outputs of one funding transaction; the inputs differ in vout only.
    let mut funding = coin(60_000).prev;
    funding.output.push(funding.output[0].clone());
    let one = |vout: u32| Coin {
        prev: funding.clone(),
        vout,
        chain: Keychain::Receive,
        index: 0,
    };
    let mut s = state();
    sign_ok(&mut s, &spending(&[one(0)], external(1), 59_000), NOW);
    sign_ok(&mut s, &spending(&[one(1)], external(1), 59_000), NOW);
    assert_eq!(s.log().count(), 2);
    assert_eq!(day(&s, NOW), (2 * 60_000, 0));
}

#[test]
fn sr_33_signing_the_replaced_transaction_again_cannot_lower_or_inflate_the_totals() {
    let mut s = state();
    let c = coin(100_000);
    let a = build(std::slice::from_ref(&c), &[pay(60_000), change(0, 39_000)]);
    let b = build(&[c], &[pay(60_000), change(0, 35_000)]);
    sign_ok(&mut s, &a, NOW);
    sign_ok(&mut s, &b, NOW + 1);
    let before = day(&s, NOW + 2);
    // Back and forth: the file that was joined is not in the log any more,
    // but it spends every input of the one that is.
    for (i, p) in [&a, &b, &a, &b].into_iter().enumerate() {
        sign_ok(&mut s, p, NOW + 2 + i as u64);
        assert_eq!(day(&s, NOW + 10), before);
        assert_eq!(s.log().count(), 1);
    }
}

#[test]
fn sr_33_signing_a_replaced_transaction_that_does_not_cover_the_newer_one_counts_in_full() {
    let mut s = state();
    open_ceilings(&mut s);
    let (x, y) = (coin(60_000), coin(60_000));
    let a = spending(std::slice::from_ref(&x), external(1), 59_000);
    let b = spending(&[x, y], external(1), 118_000);
    sign_ok(&mut s, &a, NOW);
    sign_ok(&mut s, &b, NOW + 1);
    assert_eq!(s.log().count(), 1);
    let joined = day(&s, NOW + 1);
    // `a` is no longer in the log; it spends only part of what `b` spends.
    sign_ok(&mut s, &a, NOW + 2);
    assert_eq!(s.log().count(), 2);
    assert_eq!(day(&s, NOW + 2).0, joined.0 + 60_000);
}

#[test]
fn sr_33_equal_times_after_a_correction_join_the_larger_transaction_id() {
    let mut s = state();
    let (x, y) = (coin(30_000), coin(30_000));
    let px = spending(std::slice::from_ref(&x), external(1), 29_000);
    let py = spending(std::slice::from_ref(&y), external(2), 29_000);
    let both = spending(&[x, y], external(3), 58_000);
    let far = NOW + 1_000;
    let tx = sign_ok(&mut s, &px, far).txid;
    let ty = sign_ok(&mut s, &py, far + 1).txid;
    // The correction gives both the same date.
    s.correct_clock(NOW, ok).unwrap();
    assert!(s.log().all(|e| e.time() == NOW));
    sign_ok(&mut s, &both, NOW + 1);
    let kept: Vec<Txid> = s.log().map(|e| e.txid()).collect();
    let joined = tx.max(ty);
    let left = tx.min(ty);
    assert!(kept.contains(&left), "{kept:?}");
    assert!(!kept.contains(&joined));
    assert_eq!(kept.len(), 2);
}

#[test]
fn sr_33_totals_never_fall_when_a_transaction_is_signed_and_every_state_can_be_read_back() {
    // A pool of coins that random transactions draw from, so that many of
    // them conflict, with time moving by minutes and sometimes hours.
    let mut s = state();
    open_ceilings(&mut s);
    let dest = [external(1), external(2), external(3)];
    enroll_active(&mut s, &dest[0], NOW);
    let pool: Vec<Coin> = (0..6).map(|_| coin(60_000)).collect();
    let mut seed = 0x9E37_79B9_7F4A_7C15u64;
    let mut next = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    let mut now = NOW;
    let mut signed = 0;
    for _ in 0..300 {
        now += match next() % 4 {
            0 => 0,
            1 => 60,
            2 => 3_600,
            _ => 20_000,
        };
        let take = 1 + (next() % 3) as usize;
        let mut picked: Vec<Coin> = Vec::new();
        while picked.len() < take {
            let c = pool[(next() % pool.len() as u64) as usize].clone();
            if !picked.iter().any(|p| p.outpoint() == c.outpoint()) {
                picked.push(c);
            }
        }
        let funds: u64 = picked.len() as u64 * 60_000;
        let fee = 500 + next() % 4_000;
        let to = dest[(next() % 3) as usize].clone();
        let p = if next() % 3 == 0 {
            let idx = s.change_mark();
            let half = (funds - fee) / 2;
            build(
                &picked,
                &[pay_to(to, half), change(idx, funds - fee - half)],
            )
        } else {
            spending(&picked, to, funds - fee)
        };
        let Ok(r) = s.review(&p.serialize(), now) else {
            continue;
        };
        let before = s.totals(now);
        s.sign(r, key2(), now, ok).unwrap();
        let after = s.totals(now);
        assert!(after.other_day >= before.other_day);
        assert!(after.other_week >= before.other_week);
        assert!(after.allowlisted_day >= before.allowlisted_day);
        assert!(after.allowlisted_week >= before.allowlisted_week);
        signed += 1;
        survives_snapshot(&s);
    }
    assert!(signed > 100, "only {signed} transactions were signed");
}

// ---- the spend log at its limits ---------------------------------------------

/// Fill the log with made-up entries of `inputs` inputs each, until it holds
/// `entries` entries.
fn pad_log(s: &mut SignerState, entries: usize, inputs: usize) {
    let mut n = 0u32;
    while s.log.len() < entries {
        n += 1;
        let mut id = [0u8; 32];
        id[..4].copy_from_slice(&n.to_be_bytes());
        id[31] = 0xEE;
        let mut ops: Vec<OutPoint> = (0..inputs as u32)
            .map(|v| OutPoint {
                txid: Txid::from_byte_array([0xA0 ^ (n % 200) as u8; 32]),
                vout: n * 100 + v,
            })
            .collect();
        ops.sort_by_key(|o| (o.txid.to_byte_array(), o.vout));
        s.log.insert(
            id,
            LogEntry {
                txid: Txid::from_byte_array(id),
                time: NOW,
                other_sats: 1,
                allowlisted_sats: 0,
                fee_sats: 0,
                inputs: ops,
            },
        );
    }
}

#[test]
fn a_spend_log_exactly_at_both_limits_is_a_live_state_that_reads_back_and_still_replaces() {
    let mut s = state();
    open_ceilings(&mut s);
    let coins: Vec<Coin> = (0..10).map(|_| coin(10_000)).collect();
    let a = spending(&coins, external(1), 99_000);
    let b = spending(&coins, external(1), 98_000);
    sign_ok(&mut s, &a, NOW);
    pad_log(&mut s, crate::MAX_LOG_ENTRIES, 10);
    assert_eq!(s.log.len(), crate::MAX_LOG_ENTRIES);
    assert_eq!(
        s.log.values().map(|e| e.inputs.len()).sum::<usize>(),
        crate::MAX_LOG_INPUTS
    );
    survives_snapshot(&s);
    // One more transaction would take the log past its size.
    let extra = simple(10_000, 1_000);
    assert_eq!(
        s.review(&extra.serialize(), NOW + 1).unwrap_err(),
        Refusal::SpendLogFull
    );
    // A replacement takes the place of an entry, so there is room for it.
    sign_ok(&mut s, &b, NOW + 1);
    assert_eq!(s.log.len(), crate::MAX_LOG_ENTRIES);
    survives_snapshot(&s);
}

// ---- SR-51 --------------------------------------------------------------------

#[test]
fn sr_51_a_destination_removed_and_enrolled_again_makes_the_review_outdated() {
    let script = external(40);
    let mut s = state();
    let at = enroll_active(&mut s, &script, NOW);
    let review = review_ok(&s, &simple_to(script.clone(), 100_000, 1_000), at);
    assert!(matches!(
        review.outputs()[0].kind,
        OutputKind::Allowlisted { .. }
    ));
    s.remove_destination(&script, ok).unwrap();
    s.enroll_destination(script, "Cash-out", at, ok).unwrap();
    let before = s.to_bytes();
    let r = s.sign(review, key2(), at, ok);
    assert_eq!(
        r.unwrap_err(),
        SignError::Refused(Refusal::ReviewOutdated { output: 0 })
    );
    assert_eq!(s.to_bytes(), before);
}

#[test]
fn sr_51_a_clock_that_is_earlier_at_signing_than_at_review_makes_the_review_outdated() {
    let script = external(40);
    let mut s = state();
    let at = enroll_active(&mut s, &script, NOW);
    let review = review_ok(&s, &simple_to(script, 100_000, 1_000), at);
    let before = s.to_bytes();
    // The latest time seen is the enrollment; the clock is behind it.
    let r = s.sign(review, key2(), 0, ok);
    assert_eq!(
        r.unwrap_err(),
        SignError::Refused(Refusal::ReviewOutdated { output: 0 })
    );
    assert_eq!(s.to_bytes(), before);
}

#[test]
fn sr_51_a_clock_correction_since_the_review_makes_the_review_outdated() {
    let script = external(40);
    let mut s = state();
    let far = NOW + 10 * DAY_SECS;
    s.enroll_destination(script.clone(), "Cash-out", far, ok)
        .unwrap();
    let review = review_ok(&s, &simple_to(script, 100_000, 1_000), far + DAY_SECS);
    assert!(matches!(
        review.outputs()[0].kind,
        OutputKind::Allowlisted { .. }
    ));
    // The true clock is read again: the wait starts over from there.
    s.correct_clock(NOW, ok).unwrap();
    let r = s.sign(review, key2(), NOW + 100, ok);
    assert!(matches!(
        r,
        Err(SignError::Refused(Refusal::ReviewOutdated { .. }))
    ));
}

#[test]
fn sr_34_a_review_is_refused_by_a_state_whose_change_mark_is_lower() {
    let mut s = state();
    let before = s.clone();
    s.raise_change_mark(30, ok).unwrap();
    let p = build(&[coin(100_000)], &[pay(50_000), change(40, 49_000)]);
    let review = review_ok(&s, &p, NOW);
    let mut older = before;
    let r = older.sign(review, key2(), NOW, ok);
    assert!(matches!(
        r,
        Err(SignError::Refused(Refusal::ChangeIndexTooFar {
            index: 40,
            ..
        }))
    ));
    assert_eq!(older.change_mark(), 0);
}

#[test]
fn sr_51_a_review_signed_after_a_twin_was_signed_gives_the_same_file_and_counts_once() {
    let mut s = state();
    let p = simple(50_000, 1_000);
    let first = review_ok(&s, &p, NOW);
    let second = review_ok(&s, &p, NOW);
    let a = s.sign(first, key2(), NOW, ok).unwrap();
    let b = s.sign(second, key2(), NOW + 5, ok).unwrap();
    assert_eq!(a.psbt, b.psbt);
    assert_eq!(s.log().count(), 1);
    assert_eq!(day(&s, NOW + 5), (51_000, 0));
}

// ---- extreme values -------------------------------------------------------------

#[test]
fn a_clock_of_zero_and_of_the_largest_number_break_nothing() {
    let mut s = state();
    let p = simple(50_000, 1_000);
    assert!(s.review(&p.serialize(), 0).is_ok());
    let r = s.review(&p.serialize(), u64::MAX).unwrap();
    s.sign(r, key2(), u64::MAX, ok).unwrap();
    assert_eq!(s.latest_time_seen(), u64::MAX);
    survives_snapshot(&s);
    // The clock is now far behind; everything is still counted.
    assert_eq!(s.totals(0).other_day, 51_000);
    let q = simple(20_000, 1_000);
    let r = s.review(&q.serialize(), 0).unwrap();
    assert_eq!(r.clock_behind_by(), Some(u64::MAX));
    s.sign(r, key2(), 0, ok).unwrap();
    assert_eq!(s.totals(0).other_day, 72_000);
    survives_snapshot(&s);
    // The owner confirms the correction: nothing is lost, and the entries
    // age from the true time.
    s.correct_clock(NOW, ok).unwrap();
    assert_eq!(s.totals(NOW + DAY_SECS).other_day, 0);
    assert_eq!(s.totals(NOW + DAY_SECS - 1).other_day, 72_000);
    survives_snapshot(&s);
}

#[test]
fn sr_30_an_entry_enrolled_at_the_largest_time_still_waits() {
    let script = external(40);
    let mut s = state();
    s.enroll_destination(script.clone(), "Cash-out", u64::MAX, ok)
        .unwrap();
    let r = review_ok(&s, &simple_to(script, 100_000, 1_000), u64::MAX);
    assert_eq!(r.outputs()[0].kind, OutputKind::Other);
}

#[test]
fn the_largest_change_index_is_signed_for_and_the_mark_stops_at_two_to_the_31() {
    let top = (1u32 << 31) - 1;
    let mut s = state();
    let p = build(
        &[coin_at(Keychain::Change, top, 100_000)],
        &[pay(50_000), change(top, 49_000)],
    );
    assert!(matches!(
        s.review(&p.serialize(), NOW).unwrap_err(),
        Refusal::ChangeIndexTooFar { index, .. } if index == top
    ));
    s.raise_change_mark(1 << 31, ok).unwrap();
    sign_ok(&mut s, &p, NOW);
    assert_eq!(s.change_mark(), 1 << 31);
    survives_snapshot(&s);
    // The same file again, with the mark at its top.
    sign_ok(&mut s, &p, NOW + 1);
    assert_eq!(s.change_mark(), 1 << 31);
}

#[test]
fn the_receive_mark_reaches_two_to_the_31_through_a_batch_and_reads_back() {
    let policy = BatchPolicy { min_count: 3 };
    let approver = Approver::new(Identity::from_secret_bytes(&[1; 32]).unwrap(), policy);
    let owner = Identity::from_secret_bytes(&[10; 32]).unwrap();
    let counter = Identity::from_secret_bytes(&[20; 32]).unwrap().device_id();
    let approved = BTreeSet::from([counter]);
    let mut s = state();
    s.raise_receive_mark((1 << 31) - 3, ok).unwrap();
    let batch = BatchBuilder::new(policy)
        .derive(s.wallet(), counter, (1 << 31) - 3, 3)
        .unwrap();
    s.cosign_batch(
        &approver,
        &batch,
        &approved,
        &owner.device_id(),
        &batch.sign(&owner),
        ok,
    )
    .unwrap();
    assert_eq!(s.receive_mark(), 1 << 31);
    survives_snapshot(&s);
}

#[test]
fn amounts_at_all_the_bitcoin_there_can_be_are_signed_counted_and_read_back() {
    let mut s = state();
    open_ceilings(&mut s);
    let big = || coin(MAX_MONEY_SATS);
    sign_ok(
        &mut s,
        &spending(&[big()], external(1), MAX_MONEY_SATS - 1_000),
        NOW,
    );
    survives_snapshot(&s);
    // A second one would take the week past the ceiling, which is all the
    // bitcoin there can be.
    let again = spending(&[big()], external(2), MAX_MONEY_SATS - 1_000);
    assert!(matches!(
        s.review(&again.serialize(), NOW + 1).unwrap_err(),
        Refusal::OverCeiling { .. }
    ));
    // Three inputs of that size add up without overflow; the fee is then
    // what the rules refuse.
    let three = spending(&[big(), big(), big()], external(3), 1_000);
    assert!(matches!(
        s.review(&three.serialize(), NOW + 1).unwrap_err(),
        Refusal::FeeRateTooHigh { .. } | Refusal::FeeTooHigh { .. }
    ));
}

#[test]
fn sr_12_a_hint_with_one_slots_fingerprint_and_another_slots_key_is_refused() {
    let s = state();
    let k = kit();
    let mut p = simple(50_000, 1_000);
    let other_slot_key = k.wallet.derived_keys(Keychain::Receive, 0).unwrap()[1].inner;
    p.inputs[0].bip32_derivation.clear();
    p.inputs[0].bip32_derivation.insert(
        other_slot_key,
        (
            k.wallet.slots()[0].origin_fingerprint,
            k.wallet.slots()[0]
                .origin_path
                .child(bitcoin::bip32::ChildNumber::Normal { index: 0 })
                .child(bitcoin::bip32::ChildNumber::Normal { index: 0 }),
        ),
    );
    assert_eq!(
        s.review(&p.serialize(), NOW).unwrap_err(),
        Refusal::HintKeyMismatch { input: 0 }
    );
}

#[test]
fn sr_30_sixty_four_labels_of_the_longest_kind_are_a_live_state_that_reads_back() {
    // 63 four-byte characters and one more: 64 characters in 253 bytes.
    let label = format!("{}x", "\u{1D11E}".repeat(63));
    assert_eq!((label.chars().count(), label.len()), (64, 253));
    let mut s = state();
    for n in 0..64u8 {
        s.enroll_destination(external(n), &label, NOW + u64::from(n), ok)
            .unwrap();
    }
    survives_snapshot(&s);
}

#[test]
fn sr_41_a_snapshot_with_a_logged_fee_above_the_fixed_bound_is_refused() {
    let mut s = state();
    sign_ok(&mut s, &simple(50_000, 1_000), NOW);
    let key = *s.log.keys().next().unwrap();
    s.log.get_mut(&key).unwrap().fee_sats = crate::MAX_FEE_SATS + 1;
    let bytes = s.to_bytes();
    assert!(SignerState::from_bytes(&bytes, NET, &kit().xpubs[OWN]).is_err());
}

#[test]
fn a_store_that_panics_leaves_the_state_as_it_was() {
    let mut s = state();
    let before = s.to_bytes();
    let review = review_ok(&s, &simple(50_000, 1_000), NOW);
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = s.sign(review, key2(), NOW, |_: &[u8]| -> Result<(), ()> {
            panic!("power cut")
        });
    }));
    assert!(outcome.is_err());
    assert_eq!(s.to_bytes(), before);
}

#[test]
fn sr_02_exactly_one_thousand_inputs_pass_the_count_and_meet_the_weight_limit() {
    let coins: Vec<Coin> = (0..1_000).map(|_| coin(1_000)).collect();
    let p = spending(&coins, external(1), 900_000);
    let r = refused(&state(), &p, NOW);
    assert!(matches!(r, Refusal::WeightTooHigh { .. }), "{r:?}");
}

#[test]
fn no_change_to_one_byte_of_a_valid_file_gets_a_signature_for_anything_but_the_real_coin() {
    // Every file that still reviews after one byte is replaced must spend the
    // one real coin at its real amount, and the signature it gets must check
    // against a hash written out again here.
    let coin0 = coin(200_000);
    let base = build(
        std::slice::from_ref(&coin0),
        &[pay(50_000), change(0, 149_000)],
    )
    .serialize();
    let real = coin0.outpoint();
    let mut accepted = 0;
    for i in 0..base.len() {
        for v in [0x00u8, 0xFF, base[i] ^ 0x01, base[i].wrapping_add(1)] {
            if v == base[i] {
                continue;
            }
            let mut m = base.clone();
            m[i] = v;
            let mut s = state();
            let Ok(review) = s.review(&m, NOW) else {
                continue;
            };
            accepted += 1;
            let signed = s.sign(review, key2(), NOW, ok).unwrap();
            let mut psbt = Psbt::deserialize(&signed.psbt).unwrap();
            let tx = psbt.unsigned_tx.clone();
            assert_eq!(tx.input.len(), 1, "byte {i}");
            assert_eq!(tx.input[0].previous_output, real, "byte {i}");
            assert_eq!(tx.version, bitcoin::transaction::Version::TWO, "byte {i}");
            assert!(tx.lock_time.is_block_height(), "byte {i}");
            assert!(tx.input[0].sequence.0 >= 0xFFFF_FFFD, "byte {i}");
            let out: u64 = tx.output.iter().map(|o| o.value.to_sat()).sum();
            assert!(out <= 200_000 && tx.output.iter().all(|o| o.value.to_sat() > 0));
            // A second signature from key 1 and the independent hash.
            sign_as(&mut psbt, 0);
            finalize(&psbt.serialize());
        }
    }
    assert!(accepted > 0);
}
