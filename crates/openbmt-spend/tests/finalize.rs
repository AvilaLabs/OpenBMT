//! SP-60 to SP-64: finishing.

mod common;

use bitcoin::absolute::LockTime;
use bitcoin::bip32::DerivationPath;
use bitcoin::psbt::Psbt;
use bitcoin::secp256k1::Message;
use bitcoin::transaction::Version;
use bitcoin::{Amount, ScriptBuf, Sequence, Transaction, TxIn, TxOut, Witness};
use common::*;
use openbmt_core::wallet::Keychain;
use openbmt_spend::{combine, finalize, finalize_plan, plan, ChangePosition, FinalizeError};

fn two_input_run() -> Run {
    run(
        &[coin(1, 0, 300_000), coin(2, 3, 300_000)],
        vec![(external(1), 500_000)],
        2000,
    )
}

#[test]
fn sp_60_two_signatures_make_a_witness_in_script_order() {
    let r = two_input_run();
    let signed = combine(&r.base, &[r.by1.clone(), r.by2.clone()]).unwrap();
    let tx = finalize(wallet(), &signed).unwrap();
    assert_eq!(tx.compute_txid(), r.plan.txid());
    for (i, txin) in tx.input.iter().enumerate() {
        let items: Vec<&[u8]> = txin.witness.iter().collect();
        assert_eq!(items.len(), 4);
        assert!(items[0].is_empty());
        let chain_index = r.plan.inputs()[i].coin.index;
        let ws = wallet()
            .witness_script(Keychain::Receive, chain_index)
            .unwrap();
        assert_eq!(items[3], ws.as_bytes());
        // The keys in the sorted script, and which of them signed.
        let mut keys = wallet()
            .derived_keys(Keychain::Receive, chain_index)
            .unwrap();
        keys.sort_by_key(|k| k.inner.serialize());
        let hash = bip143(&tx, i, &ws, r.plan.inputs()[i].amount_sats);
        let message = Message::from_digest(hash);
        let sigs: Vec<_> = [items[1], items[2]]
            .iter()
            .map(|s| bitcoin::ecdsa::Signature::from_slice(s).unwrap())
            .collect();
        let mut at = Vec::new();
        for sig in &sigs {
            let pos = keys
                .iter()
                .position(|k| {
                    secp()
                        .verify_ecdsa(&message, &sig.signature, &k.inner)
                        .is_ok()
                })
                .expect("each signature is by a key of the script");
            at.push(pos);
        }
        assert!(at[0] < at[1], "signatures follow the key order: {at:?}");
    }
}

#[test]
fn sp_60_a_third_signature_is_not_used_and_the_order_still_holds() {
    let r = two_input_run();
    let mut all = r.base.clone();
    sign_as(&mut all, 0);
    sign_as(&mut all, 1);
    sign_as(&mut all, 2);
    assert!(all.inputs.iter().all(|i| i.partial_sigs.len() == 3));
    let tx = finalize(wallet(), &all).unwrap();
    for txin in &tx.input {
        assert_eq!(txin.witness.len(), 4);
    }
    // Any two of the three finish, in any order of signing.
    for pair in [(0, 1), (0, 2), (1, 2), (2, 0)] {
        let mut p = r.base.clone();
        sign_as(&mut p, pair.0);
        sign_as(&mut p, pair.1);
        let t = finalize(wallet(), &p).unwrap();
        assert_eq!(t.compute_txid(), r.plan.txid());
    }
}

#[test]
fn sp_60_one_signature_or_one_signer_twice_is_not_enough() {
    let r = two_input_run();
    let one = combine(&r.base, std::slice::from_ref(&r.by1)).unwrap();
    assert_eq!(
        finalize(wallet(), &one).unwrap_err(),
        FinalizeError::NotEnoughSignatures { input: 0, have: 1 }
    );
    let twice = combine(&r.base, &[r.by1.clone(), r.by1.clone()]).unwrap();
    assert_eq!(
        finalize(wallet(), &twice).unwrap_err(),
        FinalizeError::NotEnoughSignatures { input: 0, have: 1 }
    );
    // Two signatures on one input and one on the other.
    let mut uneven = combine(&r.base, &[r.by1.clone(), r.by2.clone()]).unwrap();
    uneven.inputs[1].partial_sigs.clear();
    let (k, s) = signature_by(&uneven, 1, 0);
    uneven.inputs[1].partial_sigs.insert(k, s);
    assert_eq!(
        finalize(wallet(), &uneven).unwrap_err(),
        FinalizeError::NotEnoughSignatures { input: 1, have: 1 }
    );
    assert!(finalize(wallet(), &r.base)
        .unwrap_err()
        .to_string()
        .starts_with("SP-60"));
}

#[test]
fn sp_60_a_signature_by_another_input_makes_finalize_refuse() {
    // Both inputs at index 0: the keys are the same, the hashes differ.
    let r = run(
        &[coin(1, 0, 300_000), coin(2, 0, 300_000)],
        vec![(external(1), 500_000)],
        1000,
    );
    let mut signed = combine(&r.base, &[r.by1.clone(), r.by2.clone()]).unwrap();
    // Replace input 0's signature by key 1 with input 1's.
    let (key1, _) = signature_by(&r.base, 0, 0);
    let (_, other) = signature_by(&r.base, 1, 0);
    signed.inputs[0].partial_sigs.insert(key1, other);
    assert!(matches!(
        finalize(wallet(), &signed),
        Err(FinalizeError::BadSignature { input: 0, .. })
    ));
}

#[test]
fn sp_61_the_input_must_be_the_wallets() {
    let r = two_input_run();
    let signed = combine(&r.base, &[r.by1.clone(), r.by2.clone()]).unwrap();
    // A witness script that is not the wallet's.
    let mut other_script = signed.clone();
    other_script.inputs[0].witness_script =
        Some(wallet().witness_script(Keychain::Receive, 9).unwrap());
    assert_eq!(
        finalize(wallet(), &other_script).unwrap_err(),
        FinalizeError::WitnessScriptMismatch { input: 0 }
    );
    // No hints.
    let mut bare = signed.clone();
    bare.inputs[1].bip32_derivation.clear();
    assert_eq!(
        finalize(wallet(), &bare).unwrap_err(),
        FinalizeError::NoWalletKeyHint { input: 1 }
    );
    // Hints for another index: the script spent is not that one's.
    let mut moved = signed.clone();
    let path: DerivationPath = "m/48'/1'/0'/2'/0/7".parse().unwrap();
    for (_, p) in moved.inputs[0].bip32_derivation.values_mut() {
        *p = path.clone();
    }
    assert!(matches!(
        finalize(wallet(), &moved),
        Err(FinalizeError::HintsInconsistent { input: 0 })
            | Err(FinalizeError::ScriptNotInWallet { input: 0 })
    ));
    // The previous transaction is not the one the input spends.
    let mut swapped = signed.clone();
    swapped.inputs[0].non_witness_utxo = signed.inputs[1].non_witness_utxo.clone();
    assert_eq!(
        finalize(wallet(), &swapped).unwrap_err(),
        FinalizeError::PreviousTransactionMismatch { input: 0 }
    );
}

#[test]
fn sp_62_the_finished_transaction_is_the_planned_one() {
    let r = run(
        &[
            coin(1, 0, 300_000),
            coin(2, 1, 300_000),
            coin(3, 2, 300_000),
        ],
        vec![(external(1), 650_000), (external(2), 40_000)],
        3000,
    );
    let signed = combine(&r.base, &[r.by2.clone(), r.by1.clone()]).unwrap();
    let tx = finalize_plan(wallet(), &signed, &r.plan).unwrap();
    // The transaction ID is the unsigned transaction's.
    assert_eq!(tx.compute_txid(), r.plan.txid());
    assert_eq!(tx.version, r.plan.unsigned_tx().version);
    assert_eq!(tx.lock_time, r.plan.unsigned_tx().lock_time);
    assert_eq!(tx.output, r.plan.unsigned_tx().output);
    for (a, b) in tx.input.iter().zip(&r.plan.unsigned_tx().input) {
        assert_eq!(
            (a.previous_output, a.sequence),
            (b.previous_output, b.sequence)
        );
        assert!(a.script_sig.is_empty());
    }
    // The fee is inputs minus outputs, from the finished transaction.
    let total_in: u64 = r.plan.inputs().iter().map(|i| i.amount_sats).sum();
    let total_out: u64 = tx.output.iter().map(|o| o.value.to_sat()).sum();
    assert_eq!(total_in - total_out, r.plan.fee_sats());
    // Every signature in it verifies against the independent hash.
    for (i, txin) in tx.input.iter().enumerate() {
        let items: Vec<&[u8]> = txin.witness.iter().collect();
        let ws = ScriptBuf::from_bytes(items[3].to_vec());
        let message = Message::from_digest(bip143(&tx, i, &ws, r.plan.inputs()[i].amount_sats));
        let keys = wallet()
            .derived_keys(Keychain::Receive, r.plan.inputs()[i].coin.index)
            .unwrap();
        for item in [items[1], items[2]] {
            let sig = bitcoin::ecdsa::Signature::from_slice(item).unwrap();
            assert!(keys.iter().any(|k| secp()
                .verify_ecdsa(&message, &sig.signature, &k.inner)
                .is_ok()));
            assert!(sig.signature.serialize_compact()[32] < 0x80, "low S");
        }
    }
}

#[test]
fn sp_62_signatures_over_a_changed_transaction_do_not_finish() {
    let r = two_input_run();
    let mut signed = combine(&r.base, &[r.by1.clone(), r.by2.clone()]).unwrap();
    // One output is made smaller after signing: the fee grows, and every
    // signature is now over another transaction.
    let out = &mut signed.unsigned_tx.output[0];
    out.value = Amount::from_sat(out.value.to_sat() - 1000);
    assert!(matches!(
        finalize(wallet(), &signed),
        Err(FinalizeError::BadSignature { input: 0, .. })
    ));
}

/// A file with `n` inputs at index 0 and one output, signed by keys 1 and
/// 2, built by hand: `plan` would not make it.
fn big_psbt(n: u32) -> Psbt {
    let coins: Vec<_> = (1..=n).map(|t| coin(t, 0, 20_000)).collect();
    let tx = Transaction {
        version: Version::TWO,
        lock_time: LockTime::ZERO,
        input: coins
            .iter()
            .map(|c| TxIn {
                previous_output: c.outpoint,
                script_sig: ScriptBuf::new(),
                sequence: Sequence(0xFFFF_FFFD),
                witness: Witness::new(),
            })
            .collect(),
        output: vec![TxOut {
            value: Amount::from_sat(u64::from(n) * 20_000 - 100_000),
            script_pubkey: external(1),
        }],
    };
    let mut psbt = Psbt::from_unsigned_tx(tx).unwrap();
    let hints = {
        let p = plan(
            wallet(),
            &[coin(9_999_999, 0, 100_000)],
            &request(vec![(external(1), 10_000)], 1000),
        )
        .unwrap();
        p.to_psbt(wallet()).unwrap().inputs[0]
            .bip32_derivation
            .clone()
    };
    let ws = wallet().witness_script(Keychain::Receive, 0).unwrap();
    for (entry, c) in psbt.inputs.iter_mut().zip(&coins) {
        entry.non_witness_utxo = Some(c.prev_tx.clone());
        entry.witness_utxo = c.output().cloned();
        entry.witness_script = Some(ws.clone());
        entry.bip32_derivation = hints.clone();
    }
    sign_as(&mut psbt, 0);
    sign_as(&mut psbt, 1);
    psbt
}

#[test]
fn sp_63_a_transaction_heavier_than_the_limit_is_not_finished() {
    // 960 inputs weigh more than 400,000 units; 900 do not.
    let heavy = big_psbt(960);
    let e = finalize(wallet(), &heavy).unwrap_err();
    assert!(
        matches!(e, FinalizeError::TooHeavy { weight } if weight > 400_000),
        "{e:?}"
    );
    assert!(e.to_string().starts_with("SP-63"));
    let fine = big_psbt(900);
    let tx = finalize(wallet(), &fine).unwrap();
    assert!(tx.weight().to_wu() <= 400_000);
}

#[test]
fn sp_64_the_file_must_be_the_plans() {
    let r = two_input_run();
    let signed = combine(&r.base, &[r.by1.clone(), r.by2.clone()]).unwrap();
    assert!(finalize_plan(wallet(), &signed, &r.plan).is_ok());
    // Another plan over the same coins.
    let other = plan(
        wallet(),
        &[coin(1, 0, 300_000), coin(2, 3, 300_000)],
        &request(vec![(external(1), 450_000)], 2000),
    )
    .unwrap();
    assert_eq!(
        finalize_plan(wallet(), &signed, &other).unwrap_err(),
        FinalizeError::NotThePlan
    );
    // Same coins and payment at another rate: another fee, another file.
    let faster = plan(
        wallet(),
        &[coin(1, 0, 300_000), coin(2, 3, 300_000)],
        &request(vec![(external(1), 500_000)], 4000),
    )
    .unwrap();
    assert_eq!(
        finalize_plan(wallet(), &signed, &faster).unwrap_err(),
        FinalizeError::NotThePlan
    );
}

#[test]
fn sp_60_a_file_with_no_input_or_no_output_is_never_finished() {
    // Found by the fuzzer: with no inputs there is nothing to check, and a
    // transaction used to come out.
    let empty = Transaction {
        version: Version::TWO,
        lock_time: LockTime::ZERO,
        input: vec![],
        output: vec![],
    };
    let psbt = Psbt::from_unsigned_tx(empty).unwrap();
    assert_eq!(
        finalize(wallet(), &psbt).unwrap_err(),
        FinalizeError::NotVersionZero
    );
    assert!(combine(&psbt, &[]).is_err());

    // Inputs but no output.
    let r = two_input_run();
    let mut tx = r.base.unsigned_tx.clone();
    tx.output.clear();
    let mut no_outputs = r.base.clone();
    no_outputs.unsigned_tx = tx;
    no_outputs.outputs.clear();
    assert_eq!(
        finalize(wallet(), &no_outputs).unwrap_err(),
        FinalizeError::NotVersionZero
    );
    assert!(combine(&no_outputs, &[]).is_err());
}

/// A request for one payment of 500,000 from two coins, with the change and
/// the lock time as the test asks.
fn shaped(position: ChangePosition, lock_height: u32) -> openbmt_spend::SpendRequest {
    let mut r = request(vec![(external(1), 500_000)], 2000);
    r.change_position = position;
    r.lock_height = lock_height;
    r
}

fn two_coins() -> [openbmt_spend::Coin; 2] {
    [coin(1, 0, 300_000), coin(2, 3, 300_000)]
}

/// A seed whose place for the plan above is the first.
fn seed_for_first() -> ChangePosition {
    (0..=255u8)
        .map(|n| ChangePosition::Seed([n; 32]))
        .find(|pos| {
            plan(wallet(), &two_coins(), &shaped(*pos, 0))
                .unwrap()
                .change_position()
                == Some(0)
        })
        .expect("a seed puts the change first")
}

#[test]
fn sp_62_any_block_height_lock_time_is_finished_and_the_file_must_be_the_plans() {
    for h in [0, 1, 850_000, 499_999_999] {
        let r = run_request(&two_coins(), &shaped(ChangePosition::Last, h));
        let signed = combine(&r.base, &[r.by1.clone(), r.by2.clone()]).unwrap();
        let tx = finalize(wallet(), &signed).unwrap();
        assert_eq!(tx.lock_time, LockTime::from_height(h).unwrap());
        // SP-64: the plan's lock time is held too.
        assert_eq!(finalize_plan(wallet(), &signed, &r.plan).unwrap(), tx);
    }
    // A file with another lock time is not the plan's.
    let a = run_request(&two_coins(), &shaped(ChangePosition::Last, 100));
    let b = run_request(&two_coins(), &shaped(ChangePosition::Last, 101));
    let signed = combine(&b.base, &[b.by1.clone(), b.by2.clone()]).unwrap();
    assert_eq!(
        finalize_plan(wallet(), &signed, &a.plan).unwrap_err(),
        FinalizeError::NotThePlan
    );
}

#[test]
fn sp_62_a_lock_time_that_is_a_time_is_refused() {
    let r = run_request(&two_coins(), &shaped(ChangePosition::Last, 100));
    for t in [500_000_000, 1_700_000_000, u32::MAX] {
        let mut file = r.base.clone();
        file.unsigned_tx.lock_time = LockTime::from_consensus(t);
        let e = finalize(wallet(), &file).unwrap_err();
        assert_eq!(e, FinalizeError::LockTimeNotHeight { lock_time: t });
        assert!(e.to_string().starts_with("SP-37"));
        // The signer refuses it too (SR-03).
        let refusal = signer(1).review(&file.serialize(), NOW).unwrap_err();
        assert_eq!(refusal.rule(), "SR-03");
        // Signatures on the changed file do not help.
        sign_as(&mut file, 0);
        sign_as(&mut file, 1);
        assert_eq!(
            finalize(wallet(), &file).unwrap_err(),
            FinalizeError::LockTimeNotHeight { lock_time: t }
        );
    }
}

#[test]
fn sp_62_a_plan_with_the_change_first_finalizes_and_matches_finalize_plan() {
    let first = seed_for_first();
    let r = run_request(&two_coins(), &shaped(first, 850_000));
    assert_eq!(r.plan.change_position(), Some(0));
    let signed = combine(&r.base, &[r.by1.clone(), r.by2.clone()]).unwrap();
    let tx = finalize(wallet(), &signed).unwrap();
    let planned = finalize_plan(wallet(), &signed, &r.plan).unwrap();
    assert_eq!(tx, planned);
    assert_eq!(tx.compute_txid(), r.plan.txid());
    assert_eq!(
        tx.output[0].script_pubkey,
        wallet().change_script(0).unwrap()
    );
    assert_eq!(tx.output[1].script_pubkey, external(1));
    assert_eq!(tx.output[1].value.to_sat(), 500_000);
}

#[test]
fn sp_64_a_file_whose_outputs_were_reordered_is_not_the_plans() {
    // Two plans of the same request that differ only in where the change
    // is; the file of the one, fully and validly signed, is refused for the
    // other.
    let last = run_request(&two_coins(), &shaped(ChangePosition::Last, 850_000));
    let first = run_request(&two_coins(), &shaped(seed_for_first(), 850_000));
    assert_eq!(last.plan.fee_sats(), first.plan.fee_sats());
    assert_ne!(last.plan.txid(), first.plan.txid());
    let signed_first = combine(&first.base, &[first.by1.clone(), first.by2.clone()]).unwrap();
    assert!(finalize(wallet(), &signed_first).is_ok());
    assert_eq!(
        finalize_plan(wallet(), &signed_first, &last.plan).unwrap_err(),
        FinalizeError::NotThePlan
    );
    assert!(finalize_plan(wallet(), &signed_first, &first.plan).is_ok());
    // The same file with its outputs swapped by hand: the signatures no
    // longer verify, and the plan check refuses it before that.
    let mut swapped = signed_first.clone();
    swapped.unsigned_tx.output.swap(0, 1);
    assert_eq!(
        finalize_plan(wallet(), &swapped, &first.plan).unwrap_err(),
        FinalizeError::NotThePlan
    );
}
