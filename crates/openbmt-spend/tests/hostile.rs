//! SP-70, SP-71 and the property tests: random requests, hostile bytes.

mod common;

use bitcoin::psbt::Psbt;
use bitcoin::{PubkeyHash, ScriptBuf, ScriptHash, WScriptHash};
use common::*;
use openbmt_core::wallet::Keychain;
use openbmt_spend::{combine, finalize, finalize_plan, plan, Coin, Plan, PlanError};

fn script_of_kind(kind: u64, n: u8) -> ScriptBuf {
    match kind % 4 {
        0 => external(n),
        1 => ScriptBuf::new_p2wsh(&WScriptHash::from_byte_array([n; 32])),
        2 => ScriptBuf::new_p2pkh(&PubkeyHash::from_byte_array([n; 20])),
        _ => ScriptBuf::new_p2sh(&ScriptHash::from_byte_array([n; 20])),
    }
}

use bitcoin::hashes::Hash;

/// One random request over random coins.
fn random_case(rng: &mut Rng) -> (Vec<Coin>, openbmt_spend::SpendRequest) {
    let n = rng.range(1, 14);
    let coins: Vec<Coin> = (0..n)
        .map(|t| {
            let chain = if rng.range(0, 3) == 0 {
                Keychain::Change
            } else {
                Keychain::Receive
            };
            coin_tagged(
                1000 + t as u32 + (rng.next() % 1_000_000) as u32 * 20,
                chain,
                rng.range(0, 8) as u32,
                rng.range(1_000, 2_000_000),
                rng.range(0, 8) as u32,
            )
        })
        .collect();
    let send_all = rng.range(0, 5) == 0;
    let payments = if send_all {
        vec![(script_of_kind(rng.next(), 11), 0)]
    } else {
        (0..rng.range(1, 3))
            .map(|i| {
                (
                    script_of_kind(rng.next(), 20 + i as u8),
                    rng.range(200, 1_200_000),
                )
            })
            .collect()
    };
    let request = openbmt_spend::SpendRequest {
        payments,
        fee_rate: rng.range(1_000, 30_000),
        change_index: rng.range(0, 15) as u32,
        min_confirmations: rng.range(0, 2) as u32,
        send_all,
        must_spend: Vec::new(),
    };
    (coins, request)
}

fn check_success(plan_: &Plan, rate: u64) {
    // SP-34
    assert_eq!(
        plan_.total_in_sats(),
        plan_.total_out_sats() + plan_.fee_sats()
    );
    let base = plan_.to_psbt(wallet()).unwrap();
    // The real signer signs it with two keys.
    let by1 = sign_with_signer(&base, 0);
    let by3 = sign_with_signer(&base, 2);
    let joined = combine(&base, &[by1, by3]).unwrap();
    let tx = finalize_plan(wallet(), &joined, plan_).unwrap();
    // SP-31: the rate of the real transaction.
    let real_vsize = tx.weight().to_wu().div_ceil(4);
    let n = tx.input.len() as u64;
    let fee = plan_.fee_sats();
    assert!(
        fee * 1000 >= rate * real_vsize,
        "real rate below the asked rate"
    );
    assert!(real_vsize <= plan_.vsize());
    assert!(plan_.vsize() - real_vsize <= 2 * n + 1);
    // The fee is at least what the asked rate needs, and when no dust
    // went into it, exactly that.
    let needed = (plan_.vsize() * rate).div_ceil(1000);
    assert!(fee >= needed);
    if fee == needed {
        assert!(fee * 1000 <= rate * (real_vsize + 2 * n + 1) + 999);
    }
    // Weight and fee limits.
    assert!(tx.weight().to_wu() <= 400_000);
    assert!(fee <= 2_000_000);
}

#[test]
fn sp_31_random_requests_are_refused_or_paid_at_the_asked_rate_and_signed() {
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    let (mut done, mut refused) = (0, 0);
    for _ in 0..60 {
        let (coins, request) = random_case(&mut rng);
        match plan(wallet(), &coins, &request) {
            Ok(p) => {
                check_success(&p, request.fee_rate);
                done += 1;
            }
            Err(e) => {
                // Refusals are errors with a plain text, never panics.
                assert!(!e.to_string().is_empty());
                refused += 1;
            }
        }
    }
    assert!(done >= 20, "only {done} plans; {refused} refused");
    assert!(refused >= 1, "no refusal in 60 cases");
}

#[test]
fn sp_34_random_requests_keep_inputs_equal_to_outputs_plus_fee() {
    let mut rng = Rng(0xD1B5_4A32_D192_ED03);
    let mut done = 0;
    for _ in 0..300 {
        let (coins, request) = random_case(&mut rng);
        if let Ok(p) = plan(wallet(), &coins, &request) {
            let tx = p.unsigned_tx();
            let out: u64 = tx.output.iter().map(|o| o.value.to_sat()).sum();
            let inp: u64 = p.inputs().iter().map(|i| i.amount_sats).sum();
            assert_eq!(inp, out + p.fee_sats());
            // The fee is never below what the asked rate needs.
            assert!(p.fee_sats() >= (p.vsize() * request.fee_rate).div_ceil(1000));
            for o in &tx.output {
                assert!(o.value.to_sat() >= 294);
            }
            done += 1;
        }
    }
    assert!(done > 100);
}

#[test]
fn sp_71_the_same_arguments_give_the_same_plan_and_file() {
    let mut rng = Rng(42);
    for _ in 0..15 {
        let (coins, request) = random_case(&mut rng);
        let a = plan(wallet(), &coins, &request);
        let b = plan(wallet(), &coins, &request);
        assert_eq!(a, b);
        if let Ok(p) = a {
            assert_eq!(
                p.to_psbt(wallet()).unwrap().serialize(),
                p.to_psbt(wallet()).unwrap().serialize()
            );
        }
    }
}

#[test]
fn sp_70_arbitrary_bytes_never_panic_and_never_make_a_transaction() {
    let r = run(
        &[coin(1, 0, 300_000), coin(2, 3, 300_000)],
        vec![(external(1), 500_000)],
        2000,
    );
    let valid = r.by1.serialize();
    let mut rng = Rng(0xABCD_EF01_2345_6789);
    let mut parsed = 0;
    for round in 0..4000 {
        let bytes: Vec<u8> = match round % 4 {
            // Pure noise.
            0 => (0..rng.range(0, 400)).map(|_| rng.next() as u8).collect(),
            // The magic, then noise.
            1 => {
                let mut b = b"psbt\xff".to_vec();
                b.extend((0..rng.range(0, 300)).map(|_| rng.next() as u8));
                b
            }
            // A valid file with a few bytes changed.
            2 => {
                let mut b = valid.clone();
                for _ in 0..rng.range(1, 4) {
                    let at = rng.range(0, b.len() as u64 - 1) as usize;
                    b[at] = rng.next() as u8;
                }
                b
            }
            // A valid file cut short.
            _ => valid[..rng.range(0, valid.len() as u64) as usize].to_vec(),
        };
        let Ok(copy) = Psbt::deserialize(&bytes) else {
            continue;
        };
        parsed += 1;
        // Against the plan's file: an error, or a combination that does not
        // finish (one signer at most), or the one transaction there is.
        if let Ok(joined) = combine(&r.base, std::slice::from_ref(&copy)) {
            if let Ok(tx) = finalize(wallet(), &joined) {
                assert_eq!(tx.compute_txid(), r.plan.txid());
            }
        }
        // As the base, and in other places: errors, never panics.
        let _ = combine(&copy, std::slice::from_ref(&r.by1));
        let _ = combine(&copy, &[]);
        let _ = finalize(wallet(), &copy);
    }
    assert!(parsed > 50, "only {parsed} files parsed");
}

#[test]
fn sp_70_hostile_requests_are_errors_not_panics() {
    // Extreme amounts, rates and indexes.
    let coins = [coin(1, 0, 1_000_000)];
    type Case = (Vec<(ScriptBuf, u64)>, u64, u32, bool);
    let cases: Vec<Case> = vec![
        (vec![(external(1), u64::MAX)], 1000, 0, false),
        (vec![(external(1), 1_000)], u64::MAX, 0, false),
        (vec![(external(1), 1_000)], 1000, u32::MAX, false),
        (vec![(external(1), 1_000)], 1000, 1 << 31, false),
        (vec![(external(1), 0)], u64::MAX, u32::MAX, true),
        (vec![], 1000, 0, false),
    ];
    for (payments, rate, change_index, send_all) in cases {
        let r = openbmt_spend::SpendRequest {
            payments,
            fee_rate: rate,
            change_index,
            min_confirmations: u32::MAX,
            send_all,
            must_spend: Vec::new(),
        };
        let e = plan(wallet(), &coins, &r).unwrap_err();
        assert!(!e.to_string().is_empty());
        let mut zero = r.clone();
        zero.min_confirmations = 0;
        if let Err(e) = plan(wallet(), &coins, &zero) {
            assert!(!e.to_string().is_empty());
        }
    }
    let r = plan(wallet(), &[], &request(vec![(external(1), 1000)], 1000));
    assert_eq!(r, Err(PlanError::NoEligibleCoins));
}
