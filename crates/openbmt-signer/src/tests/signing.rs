//! SR-50 to SR-56: the review and the signature.

use std::cell::{Cell, RefCell};

use bitcoin::bip32::{DerivationPath, Xpriv};
use bitcoin::psbt::Psbt;
use openbmt_core::wallet::Keychain;

use super::kit::*;
use super::owner::other_wallet_state;
use crate::analysis::signed_weight;
use crate::{Ceiling, Ceilings, OutputKind, Refusal, SignError, SignerState};

fn signature_count(psbt: &[u8]) -> Vec<usize> {
    Psbt::deserialize(psbt)
        .unwrap()
        .inputs
        .iter()
        .map(|i| i.partial_sigs.len())
        .collect()
}

#[test]
fn sr_50_the_review_shows_what_the_owner_needs() {
    let script = external(40);
    let mut s = state();
    s.enroll_destination(script.clone(), "Cash-out", NOW, ok)
        .unwrap();
    let later = NOW + 86_400;
    let p = build(
        &[coin(300_000)],
        &[
            pay(50_000),
            pay_to(script.clone(), 100_000),
            change(0, 149_000),
        ],
    );
    let r = review_ok(&s, &p, later);
    assert_eq!(r.txid(), p.unsigned_tx.compute_txid());
    assert_eq!(r.outputs().len(), 3);
    let o = r.outputs();
    assert_eq!((o[0].index, o[0].amount), (0, 50_000));
    assert_eq!(o[0].script, external(1));
    assert_eq!(o[0].kind, OutputKind::Other);
    assert_eq!(
        o[1].kind,
        OutputKind::Allowlisted {
            label: "Cash-out".into()
        }
    );
    assert_eq!(o[2].kind, OutputKind::Change { index: 0 });
    for out in o {
        let a = out.address.as_ref().expect("an address");
        assert_eq!(a.script_pubkey(), out.script);
        assert!(a.to_string().starts_with("bcrt1"), "{a}");
    }
    assert_eq!(r.change(), Some((0, 149_000)));
    assert!(!r.moves_coins_inside_wallet());
    assert_eq!(r.fee_sats(), 1_000);
    assert_eq!(r.input_count(), 1);
    assert_eq!(r.input_total_sats(), 300_000);
    assert_eq!(r.other_total_sats(), 50_000);
    assert_eq!(r.allowlisted_total_sats(), 100_000);
    assert_eq!(r.weight(), signed_weight(&p.unsigned_tx));
    assert_eq!(r.vsize(), r.weight().div_ceil(4));
    assert_eq!(r.fee_rate_ceil_sat_per_vb(), 1_000u64.div_ceil(r.vsize()));
    let rem = r.remaining();
    assert_eq!(rem.other_per_tx, 50_000);
    assert_eq!(rem.other_day, 300_000 - 51_000);
    assert_eq!(rem.other_week, 1_000_000 - 51_000);
    assert_eq!(rem.allowlisted_per_tx, 4_900_000);
    assert_eq!(rem.allowlisted_day, 9_900_000);
    assert_eq!(rem.allowlisted_week, 29_900_000);
    assert_eq!(r.clock_behind_by(), None);
    assert!(!r.already_counted());
}

#[test]
fn sr_50_what_was_shown_is_what_is_signed() {
    let mut s = state();
    let p = build(&[coin(100_000)], &[pay(50_000), change(0, 49_000)]);
    let review = review_ok(&s, &p, NOW);
    let shown = review.txid();
    // The file is not read again: changing it now changes nothing.
    let mut tampered = p.clone();
    tampered.unsigned_tx.output[0].value = bitcoin::Amount::from_sat(99_000);
    let signed = s.sign(review, key2(), NOW, ok).unwrap();
    assert_eq!(signed.txid, shown);
    let out = Psbt::deserialize(&signed.psbt).unwrap();
    assert_eq!(out.unsigned_tx, p.unsigned_tx);
    assert_ne!(out.unsigned_tx, tampered.unsigned_tx);
}

#[test]
fn sr_51_a_destination_removed_since_the_review_makes_the_review_outdated() {
    let script = external(40);
    let mut s = state();
    s.enroll_destination(script.clone(), "Cash-out", NOW, ok)
        .unwrap();
    let later = NOW + 86_400;
    let review = review_ok(&s, &simple_to(script.clone(), 1_000_000, 1_000), later);
    assert!(matches!(
        review.outputs()[0].kind,
        OutputKind::Allowlisted { .. }
    ));
    s.remove_destination(&script, ok).unwrap();
    let before = s.to_bytes();
    let r = s.sign(review, key2(), later, never);
    let Err(SignError::Refused(got)) = r else {
        panic!("{r:?}")
    };
    assert_eq!(got, Refusal::ReviewOutdated { output: 0 });
    assert_eq!(got.rule(), "SR-51");
    assert_eq!(s.to_bytes(), before);
}

#[test]
fn sr_51_a_destination_that_became_active_since_the_review_makes_it_outdated() {
    let script = external(40);
    let mut s = state();
    s.enroll_destination(script.clone(), "Cash-out", NOW, ok)
        .unwrap();
    let review = review_ok(&s, &simple_to(script, 50_000, 1_000), NOW + 86_399);
    assert_eq!(review.outputs()[0].kind, OutputKind::Other);
    let before = s.to_bytes();
    let r = s.sign(review, key2(), NOW + 86_400, never);
    assert_eq!(
        r,
        Err(SignError::Refused(Refusal::ReviewOutdated { output: 0 }))
    );
    assert_eq!(s.to_bytes(), before);
}

#[test]
fn sr_51_the_ceilings_are_checked_again_against_what_was_signed_since() {
    let mut s = state();
    s.set_ceilings(
        Ceilings {
            per_tx: 100_000,
            per_day: 100_000,
            per_week: 1_000_000,
        },
        s.ceilings_allowlisted(),
        ok,
    )
    .unwrap();
    let a = simple(50_000, 1_000);
    let b = simple(60_000, 1_000);
    let ra = review_ok(&s, &a, NOW);
    let rb = review_ok(&s, &b, NOW);
    s.sign(ra, key2(), NOW, ok).unwrap();
    let before = s.to_bytes();
    let r = s.sign(rb, key2(), NOW, never);
    assert_eq!(
        r,
        Err(SignError::Refused(Refusal::OverCeiling {
            ceiling: Ceiling::OtherDay,
            amount: 112_000,
            limit: 100_000
        }))
    );
    assert_eq!(s.to_bytes(), before);
}

#[test]
fn sr_51_a_review_made_for_another_wallet_is_refused() {
    let mut other = other_wallet_state();
    let review = review_ok(&state(), &simple(50_000, 1_000), NOW);
    let before = other.to_bytes();
    let r = other.sign(review, key2(), NOW, never);
    let Err(SignError::Refused(got)) = r else {
        panic!("{r:?}")
    };
    assert_eq!(got, Refusal::ReviewForOtherWallet);
    assert_eq!(got.rule(), "SR-51");
    assert_eq!(other.to_bytes(), before);
}

fn never(_: &[u8]) -> Result<(), &'static str> {
    panic!("a refused call reached the store");
}

#[test]
fn sr_52_only_the_whole_account_key_of_the_own_slot_signs() {
    let k = kit();
    let master2 = Xpriv::new_master(NET, &[2; 32]).unwrap();
    let sibling = master2
        .derive_priv(
            crate::crypto::secp(),
            &"m/48'/1'/1'/2'".parse::<DerivationPath>().unwrap(),
        )
        .unwrap();
    let mut wrong_metadata = *key2();
    wrong_metadata.parent_fingerprint = [1, 2, 3, 4].into();
    let mut wrong_depth = *key2();
    wrong_depth.depth = 5;
    let wrong = [
        ("key 1", k.accounts[0]),
        ("key 3", k.accounts[2]),
        ("the master key", master2),
        ("a sibling account", sibling),
        ("the right key with other metadata", wrong_metadata),
        ("the right key at another depth", wrong_depth),
    ];
    for (name, key) in wrong {
        let mut s = state();
        let review = review_ok(&s, &simple(50_000, 1_000), NOW);
        let before = s.to_bytes();
        let r = s.sign(review, &key, NOW, never);
        let Err(SignError::Refused(got)) = r else {
            panic!("{name}: {r:?}")
        };
        assert_eq!(got, Refusal::WrongKey, "{name}");
        assert_eq!(got.rule(), "SR-52");
        assert_eq!(s.to_bytes(), before, "{name}");
    }
    let mut s = state();
    let review = review_ok(&s, &simple(50_000, 1_000), NOW);
    assert!(s.sign(review, key2(), NOW, ok).is_ok());
}

#[test]
fn sr_53_signatures_are_deterministic_low_s_and_over_the_bip143_hash() {
    let coins = [
        coin_at(Keychain::Receive, 4, 40_000),
        coin_at(Keychain::Change, 2, 40_000),
        coin_at(Keychain::Receive, 9, 40_000),
    ];
    let p = build(&coins, &[pay(60_000), change(0, 55_000)]);
    let mut a = state();
    let mut b = state();
    let sa = sign_ok(&mut a, &p, NOW);
    let sb = sign_ok(&mut b, &p, NOW + 5_000);
    assert_eq!(sa.psbt, sb.psbt, "the same file gives the same signatures");
    // The signatures equal those the test makes over its own BIP143 hash.
    let mut mine = p.clone();
    sign_as(&mut mine, OWN);
    let out = Psbt::deserialize(&sa.psbt).unwrap();
    for (got, want) in out.inputs.iter().zip(&mine.inputs) {
        assert_eq!(got.partial_sigs, want.partial_sigs);
    }
    // Low S, SIGHASH_ALL, and a finalized transaction that checks out.
    let mut with_key1 = p.clone();
    sign_as(&mut with_key1, 0);
    let mut c = state();
    let signed = sign_ok(&mut c, &with_key1, NOW);
    assert_eq!(signed.signatures_per_input, vec![2, 2, 2]);
    let tx = finalize(&signed.psbt);
    for input in &tx.input {
        assert_eq!(input.witness.len(), 4);
    }
}

#[test]
fn sr_53_key_two_and_key_three_also_make_a_valid_transaction() {
    let p0 = build(&[coin(100_000)], &[pay(50_000), change(0, 49_000)]);
    let mut p = p0.clone();
    sign_as(&mut p, 2);
    let mut s = state();
    let signed = sign_ok(&mut s, &p, NOW);
    let tx = finalize(&signed.psbt);
    assert_eq!(tx.compute_txid(), signed.txid);
}

#[test]
fn sr_54_every_signature_in_the_file_verifies() {
    let mut s = state();
    let p = build(
        &[coin(50_000), coin_at(Keychain::Change, 1, 50_000)],
        &[pay(90_000)],
    );
    let signed = sign_ok(&mut s, &p, NOW);
    assert_eq!(signature_count(&signed.psbt), vec![1, 1]);
    // Alone the signatures are not enough; with key 1's they finalize and
    // each is checked against the independent hash.
    let mut both = Psbt::deserialize(&signed.psbt).unwrap();
    sign_as(&mut both, 0);
    finalize(&both.serialize());
}

#[test]
fn sr_55_a_failing_store_returns_no_signature_and_keeps_the_state() {
    let mut s = state();
    let p = build(&[coin(100_000)], &[pay(50_000), change(3, 49_000)]);
    let before = s.to_bytes();
    let review = review_ok(&s, &p, NOW);
    let r = s.sign(review, key2(), NOW, fail);
    assert_eq!(r, Err(SignError::Persist("disk full")));
    assert_eq!(s.to_bytes(), before);
    assert_eq!(s.change_mark(), 0);
    assert_eq!(s.log().count(), 0);
    assert_eq!(s.latest_time_seen(), 0);
    // Nothing was lost: the same transaction can still be signed.
    sign_ok(&mut s, &p, NOW);
    assert_eq!(s.change_mark(), 4);
}

#[test]
fn sr_55_the_store_is_handed_exactly_the_state_that_follows() {
    let mut s = state();
    let p = build(&[coin(100_000)], &[pay(50_000), change(3, 49_000)]);
    let review = review_ok(&s, &p, NOW);
    let seen = RefCell::new(Vec::new());
    let signed = s
        .sign(review, key2(), NOW, |b: &[u8]| {
            seen.borrow_mut().push(b.to_vec());
            Ok::<(), &str>(())
        })
        .unwrap();
    let seen = seen.into_inner();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0], s.to_bytes());
    // The stored state has the log entry, the change mark and the time.
    let stored = SignerState::from_bytes(&seen[0], NET, &kit().xpubs[OWN]).unwrap();
    assert_eq!(stored.change_mark(), 4);
    assert_eq!(stored.latest_time_seen(), NOW);
    assert_eq!(stored.log().next().unwrap().txid(), signed.txid);
}

#[test]
fn sr_55_signing_again_stores_the_clock_and_mark_without_a_new_log_entry() {
    let mut s = state();
    let p = build(&[coin(100_000)], &[pay(50_000), change(3, 49_000)]);
    sign_ok(&mut s, &p, NOW);
    let log_before: Vec<_> = s.log().cloned().collect();
    let calls = Cell::new(0);
    let review = review_ok(&s, &p, NOW + 9);
    assert!(review.already_counted());
    s.sign(review, key2(), NOW + 9, |_: &[u8]| {
        calls.set(calls.get() + 1);
        Ok::<(), &str>(())
    })
    .unwrap();
    assert_eq!(calls.get(), 1);
    assert_eq!(s.latest_time_seen(), NOW + 9);
    assert_eq!(s.log().cloned().collect::<Vec<_>>(), log_before);
}

#[test]
fn sr_56_the_callers_key_is_not_changed_by_signing() {
    // The signer erases its own copies of the child keys (best effort). The
    // account key it was lent is the caller's and is left as it was.
    let mut s = state();
    let key = *key2();
    let review = review_ok(&s, &simple(50_000, 1_000), NOW);
    s.sign(review, &key, NOW, ok).unwrap();
    assert_eq!(key, *key2());
}

#[test]
fn the_returned_file_holds_what_the_signer_derived_and_nothing_else() {
    let mut s = state();
    let p = build(&[coin(100_000)], &[pay(50_000), change(0, 49_000)]);
    let signed = sign_ok(&mut s, &p, NOW);
    let out = Psbt::deserialize(&signed.psbt).unwrap();
    let input = &out.inputs[0];
    assert!(input.non_witness_utxo.is_some() && input.witness_utxo.is_some());
    assert_eq!(
        input.witness_script,
        Some(kit().wallet.witness_script(Keychain::Receive, 0).unwrap())
    );
    assert_eq!(input.bip32_derivation, hints(Keychain::Receive, 0));
    assert_eq!(input.partial_sigs.len(), 1);
    assert!(input.final_script_witness.is_none() && input.final_script_sig.is_none());
    assert!(input.sighash_type.is_none() && input.redeem_script.is_none());
    // The change output carries its script and hints; the payment nothing.
    assert_eq!(out.outputs[0].bip32_derivation.len(), 0);
    assert!(out.outputs[0].witness_script.is_none());
    assert_eq!(out.outputs[1].bip32_derivation, hints(Keychain::Change, 0));
    assert_eq!(
        out.outputs[1].witness_script,
        Some(kit().wallet.witness_script(Keychain::Change, 0).unwrap())
    );
}

#[test]
fn a_fee_bump_signed_end_to_end() {
    let mut s = state();
    let c = coin(100_000);
    let slow = build(std::slice::from_ref(&c), &[pay(60_000), change(0, 39_000)]);
    let fast = build(&[c], &[pay(60_000), change(0, 30_000)]);
    let mut slow_k1 = slow.clone();
    sign_as(&mut slow_k1, 0);
    let mut fast_k1 = fast.clone();
    sign_as(&mut fast_k1, 0);
    let a = sign_ok(&mut s, &slow_k1, NOW);
    let b = sign_ok(&mut s, &fast_k1, NOW + 120);
    assert_ne!(a.txid, b.txid);
    assert_eq!(finalize(&b.psbt).compute_txid(), b.txid);
    assert_eq!(s.log().count(), 1);
    assert_eq!(s.totals(NOW + 120).other_day, 60_000 + 10_000);
}

#[test]
fn the_weight_estimate_is_never_smaller_than_a_really_signed_transaction() {
    for n in [1usize, 2, 3, 6] {
        let coins: Vec<_> = (0..n as u32)
            .map(|i| {
                let chain = if i % 2 == 0 {
                    Keychain::Receive
                } else {
                    Keychain::Change
                };
                coin_at(chain, i * 3, 20_000)
            })
            .collect();
        let mut p = build(&coins, &[pay(n as u64 * 20_000 - 5_000)]);
        sign_as(&mut p, 0);
        sign_as(&mut p, OWN);
        let tx = finalize(&p.serialize());
        let real = tx.weight().to_wu();
        let estimate = signed_weight(&p.unsigned_tx);
        assert!(real <= estimate, "{n} inputs: {real} > {estimate}");
        // Two signatures per input, each at most two bytes shorter than the
        // largest size.
        assert!(
            estimate - real <= 2 * 2 * n as u64,
            "{n} inputs: estimate {estimate}, real {real}"
        );
    }
}
