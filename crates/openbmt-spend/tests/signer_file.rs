//! SP-45 to SP-48: the file for a signer someone else made, and what comes
//! back from it.

mod common;

use bitcoin::bip32::ChildNumber;
use bitcoin::psbt::{Psbt, PsbtSighashType};
use bitcoin::sighash::EcdsaSighashType;
use bitcoin::Witness;
use common::*;
use openbmt_core::wallet::Keychain;
use openbmt_spend::{
    check_finished, combine, finalize, psbt_for_signer, take_signer_signatures,
    transaction_of_final_file, CombineError, SignerFileError,
};

/// A run with two inputs, and so a change output.
fn two_inputs() -> Run {
    run(
        &[
            coin(1, 2, 300_000),
            coin_tagged(2, Keychain::Change, 4, 400_000, 6),
        ],
        vec![(external(1), 500_000)],
        1000,
    )
}

/// What the owner sends: key 1's signed file, written for the signer.
fn sent(r: &Run) -> Psbt {
    psbt_for_signer(wallet(), &r.by1).unwrap()
}

/// What a signer of another make returns: only the transaction and its
/// signatures, in a file of its own: no previous transactions, no hints.
fn returned_by(r: &Run, who: usize) -> Psbt {
    let theirs = sign_with_signer(&r.base, who);
    let mut out = Psbt::from_unsigned_tx(r.base.unsigned_tx.clone()).unwrap();
    for (into, from) in out.inputs.iter_mut().zip(&theirs.inputs) {
        into.partial_sigs = from.partial_sigs.clone();
    }
    out
}

#[test]
fn sp_45_the_file_holds_the_hash_type_and_the_global_keys() {
    let r = two_inputs();
    let out = sent(&r);
    assert_eq!(out.version, 0);
    assert_eq!(out.unsigned_tx, *r.plan.unsigned_tx());
    // The three keys, each with its fingerprint and its path.
    assert_eq!(out.xpub.len(), 3);
    for slot in wallet().slots() {
        let (fingerprint, path) = &out.xpub[&slot.xpub];
        assert_eq!(*fingerprint, slot.origin_fingerprint);
        assert_eq!(*path, slot.origin_path);
        assert_eq!(slot.xpub.depth as usize, path.len());
    }
    for (entry, planned) in out.inputs.iter().zip(r.plan.inputs()) {
        let c = &planned.coin;
        assert_eq!(entry.non_witness_utxo.as_ref(), Some(&c.prev_tx));
        assert_eq!(entry.witness_utxo.as_ref(), c.output());
        assert_eq!(
            entry.witness_script,
            Some(wallet().witness_script(c.chain, c.index).unwrap())
        );
        assert_eq!(
            entry.sighash_type,
            Some(PsbtSighashType::from(EcdsaSighashType::All))
        );
        assert_eq!(entry.bip32_derivation.len(), 3);
        for (slot, key) in wallet()
            .slots()
            .iter()
            .zip(wallet().derived_keys(c.chain, c.index).unwrap())
        {
            let (fingerprint, path) = &entry.bip32_derivation[&key.inner];
            assert_eq!(*fingerprint, slot.origin_fingerprint);
            let steps: &[ChildNumber] = path.as_ref();
            assert_eq!(steps.len(), slot.origin_path.len() + 2);
        }
        // Key 1's signature, from the owner's own signer, is kept.
        assert_eq!(entry.partial_sigs.len(), 1);
        assert!(entry.unknown.is_empty() && entry.proprietary.is_empty());
    }
    // The change output carries its script and keys; a payment carries
    // nothing.
    let change = r
        .plan
        .outputs()
        .iter()
        .position(|o| matches!(o, openbmt_spend::PlanOutput::Change(_)));
    let change = change.expect("two coins pay 500,000 with change");
    for (at, entry) in out.outputs.iter().enumerate() {
        if at == change {
            assert!(entry.witness_script.is_some());
            assert_eq!(entry.bip32_derivation.len(), 3);
        } else {
            assert!(entry.witness_script.is_none() && entry.bip32_derivation.is_empty());
        }
    }
    // It survives its own bytes.
    assert_eq!(Psbt::deserialize(&out.serialize()).unwrap(), out);
    // The same for a file with no signature yet, and the writing is
    // deterministic.
    let bare = psbt_for_signer(wallet(), &r.base).unwrap();
    assert!(bare.inputs.iter().all(|i| i.partial_sigs.is_empty()));
    assert_eq!(bare, psbt_for_signer(wallet(), &r.base).unwrap());
}

#[test]
fn sp_45_the_file_is_built_from_what_is_read_and_nothing_else() {
    let r = two_inputs();
    let mut noisy = r.by1.clone();
    noisy.inputs[0].unknown.insert(
        bitcoin::psbt::raw::Key {
            type_value: 0xf0,
            key: vec![1],
        },
        vec![1, 2, 3],
    );
    noisy.unknown.insert(
        bitcoin::psbt::raw::Key {
            type_value: 0xf1,
            key: vec![],
        },
        vec![9],
    );
    let out = psbt_for_signer(wallet(), &noisy).unwrap();
    assert!(out.unknown.is_empty());
    assert!(out.inputs.iter().all(|i| i.unknown.is_empty()));
}

#[test]
fn sp_45_an_input_that_is_not_the_wallets_is_refused() {
    let r = two_inputs();
    let mut no_prev = r.by1.clone();
    no_prev.inputs[0].non_witness_utxo = None;
    assert!(matches!(
        psbt_for_signer(wallet(), &no_prev),
        Err(SignerFileError::Input { input: 0, .. })
    ));
    let mut other_script = r.by1.clone();
    other_script.inputs[1].witness_script =
        Some(wallet().witness_script(Keychain::Receive, 9).unwrap());
    assert!(matches!(
        psbt_for_signer(wallet(), &other_script),
        Err(SignerFileError::Input { input: 1, .. })
    ));
    let mut two_hints = r.by1.clone();
    let key = *two_hints.inputs[0].bip32_derivation.keys().next().unwrap();
    two_hints.inputs[0].bip32_derivation.remove(&key);
    assert!(matches!(
        psbt_for_signer(wallet(), &two_hints),
        Err(SignerFileError::Input { input: 0, .. })
    ));
    let mut no_hints = r.by1.clone();
    no_hints.inputs[0].bip32_derivation.clear();
    assert!(matches!(
        psbt_for_signer(wallet(), &no_hints),
        Err(SignerFileError::Input { input: 0, .. })
    ));
    let mut wrong_type = r.by1.clone();
    for sig in wrong_type.inputs[0].partial_sigs.values_mut() {
        sig.sighash_type = EcdsaSighashType::None;
    }
    assert_eq!(
        psbt_for_signer(wallet(), &wrong_type).unwrap_err(),
        SignerFileError::SighashType { input: 0 }
    );
    let mut v2 = r.by1.clone();
    v2.version = 2;
    assert_eq!(
        psbt_for_signer(wallet(), &v2).unwrap_err(),
        SignerFileError::NotVersionZero
    );
    assert!(SignerFileError::NotVersionZero.to_string().len() > 5);
}

#[test]
fn sp_45_an_output_that_claims_to_be_the_wallets_must_be() {
    let r = two_inputs();
    let change = r
        .base
        .outputs
        .iter()
        .position(|o| o.witness_script.is_some())
        .unwrap();
    // The payment output with the change's keys.
    let payment = 1 - change;
    let mut forged = r.by1.clone();
    forged.outputs[payment] = forged.outputs[change].clone();
    assert_eq!(
        psbt_for_signer(wallet(), &forged).unwrap_err(),
        SignerFileError::Output { output: payment }
    );
    // The change output with a witness script of another index.
    let mut other = r.by1.clone();
    other.outputs[change].witness_script =
        Some(wallet().witness_script(Keychain::Change, 77).unwrap());
    assert_eq!(
        psbt_for_signer(wallet(), &other).unwrap_err(),
        SignerFileError::Output { output: change }
    );
    // Hints on the receive chain are not change.
    let mut receive = r.by1.clone();
    receive.outputs[change].bip32_derivation = r.base.inputs[0].bip32_derivation.clone();
    receive.outputs[change].witness_script = r.base.inputs[0].witness_script.clone();
    assert!(matches!(
        psbt_for_signer(wallet(), &receive),
        Err(SignerFileError::Output { .. })
    ));
}

#[test]
fn sp_46_what_comes_back_is_joined_and_must_carry_the_signers_signature() {
    let r = two_inputs();
    let out = sent(&r);
    let back = returned_by(&r, 1);
    let both = take_signer_signatures(wallet(), &out, &back, 2).unwrap();
    for input in &both.inputs {
        assert_eq!(input.partial_sigs.len(), 2);
    }
    // And the joined file finishes.
    let tx = finalize(wallet(), &both).unwrap();
    assert_eq!(tx.compute_txid(), r.plan.txid());
    // A returned file may also carry everything the sent one did.
    assert!(take_signer_signatures(wallet(), &out, &out_with_key2(&r), 2).is_ok());
    // The signer's own file may be the sent one, in a different order of
    // fields: the unsigned transaction and the signatures are what count.
    assert_eq!(
        take_signer_signatures(wallet(), &out, &back, 2).unwrap(),
        take_signer_signatures(wallet(), &out, &out_with_key2(&r), 2).unwrap()
    );
}

fn out_with_key2(r: &Run) -> Psbt {
    combine(&sent(r), &[sign_with_signer(&r.base, 1)]).unwrap()
}

#[test]
fn sp_46_another_transaction_is_refused() {
    let r = two_inputs();
    let out = sent(&r);
    let other = run(
        &[
            coin(1, 2, 300_000),
            coin_tagged(2, Keychain::Change, 4, 400_000, 6),
        ],
        vec![(external(1), 450_000)],
        1000,
    );
    let back = returned_by(&other, 1);
    assert_eq!(
        take_signer_signatures(wallet(), &out, &back, 2).unwrap_err(),
        SignerFileError::DifferentTransaction
    );
    // The amount of one output, changed by one satoshi.
    let mut moved = returned_by(&r, 1);
    moved.unsigned_tx.output[0].value =
        bitcoin::Amount::from_sat(moved.unsigned_tx.output[0].value.to_sat() - 1);
    assert_eq!(
        take_signer_signatures(wallet(), &out, &moved, 2).unwrap_err(),
        SignerFileError::DifferentTransaction
    );
    let mut later = returned_by(&r, 1);
    later.unsigned_tx.lock_time = bitcoin::absolute::LockTime::from_height(5).unwrap();
    assert_eq!(
        take_signer_signatures(wallet(), &out, &later, 2).unwrap_err(),
        SignerFileError::DifferentTransaction
    );
}

#[test]
fn sp_46_a_file_without_the_signers_signature_is_refused() {
    let r = two_inputs();
    let out = sent(&r);
    // No signature at all.
    let mut none = returned_by(&r, 1);
    for i in &mut none.inputs {
        i.partial_sigs.clear();
    }
    assert_eq!(
        take_signer_signatures(wallet(), &out, &none, 2).unwrap_err(),
        SignerFileError::NotSigned { input: 0 }
    );
    // Only the second input signed.
    let mut half = returned_by(&r, 1);
    half.inputs[0].partial_sigs.clear();
    assert_eq!(
        take_signer_signatures(wallet(), &out, &half, 2).unwrap_err(),
        SignerFileError::NotSigned { input: 0 }
    );
    // Key 3's signature where key 2's was expected.
    let by3 = returned_by(&r, 2);
    assert_eq!(
        take_signer_signatures(wallet(), &out, &by3, 2).unwrap_err(),
        SignerFileError::NotSigned { input: 0 }
    );
    assert!(take_signer_signatures(wallet(), &out, &by3, 3).is_ok());
    // A slot that is not one.
    for slot in [0, 4] {
        assert_eq!(
            take_signer_signatures(wallet(), &out, &by3, slot).unwrap_err(),
            SignerFileError::Slot
        );
    }
}

#[test]
fn sp_46_a_signature_that_does_not_verify_is_refused() {
    // Two coins at one index, so that a signature of one input is a key
    // of the other.
    let r = run(
        &[coin(1, 0, 300_000), coin(2, 0, 300_000)],
        vec![(external(1), 500_000)],
        1000,
    );
    let out = sent(&r);
    let mut bad = returned_by(&r, 1);
    // The signature of input 0 on input 1.
    let (key, sig) = {
        let (k, s) = bad.inputs[0].partial_sigs.iter().next().unwrap();
        (*k, *s)
    };
    bad.inputs[1].partial_sigs.clear();
    bad.inputs[1].partial_sigs.insert(key, sig);
    let e = take_signer_signatures(wallet(), &out, &bad, 2).unwrap_err();
    assert!(
        matches!(
            e,
            SignerFileError::Combine(CombineError::BadSignature { .. })
        ),
        "{e}"
    );
    assert!(e.to_string().starts_with("SP-46"));
    // A signature of another hash type.
    let mut typed = returned_by(&r, 1);
    for s in typed.inputs[0].partial_sigs.values_mut() {
        s.sighash_type = EcdsaSighashType::Single;
    }
    assert!(matches!(
        take_signer_signatures(wallet(), &out, &typed, 2),
        Err(SignerFileError::Combine(CombineError::SighashType { .. }))
    ));
}

#[test]
fn sp_47_a_finished_transaction_is_checked_against_the_file_sent() {
    let r = two_inputs();
    let out = sent(&r);
    let both = take_signer_signatures(wallet(), &out, &returned_by(&r, 1), 2).unwrap();
    let tx = finalize(wallet(), &both).unwrap();
    let fee = check_finished(wallet(), &out, &tx).unwrap();
    assert_eq!(fee, r.plan.fee_sats());
    // No witness.
    let mut bare = tx.clone();
    bare.input[0].witness = Witness::new();
    assert!(matches!(
        check_finished(wallet(), &out, &bare),
        Err(SignerFileError::Finished(_))
    ));
    // Another amount.
    let mut moved = tx.clone();
    moved.output[0].value = bitcoin::Amount::from_sat(moved.output[0].value.to_sat() - 1);
    assert!(matches!(
        check_finished(wallet(), &out, &moved),
        Err(SignerFileError::Finished(_))
    ));
    // A script signature.
    let mut script_sig = tx.clone();
    script_sig.input[1].script_sig = bitcoin::ScriptBuf::from_bytes(vec![0x51]);
    assert!(matches!(
        check_finished(wallet(), &out, &script_sig),
        Err(SignerFileError::Finished(_))
    ));
    // A signature that is another input's.
    let mut crossed = tx.clone();
    crossed.input[1].witness = tx.input[0].witness.clone();
    assert!(matches!(
        check_finished(wallet(), &out, &crossed),
        Err(SignerFileError::Finished(_))
    ));
    // A time lock where a height was.
    let mut timed = tx.clone();
    timed.lock_time = bitcoin::absolute::LockTime::from_time(1_700_000_000).unwrap();
    assert!(matches!(
        check_finished(wallet(), &out, &timed),
        Err(SignerFileError::Finished(_))
    ));
    assert!(
        SignerFileError::Finished(openbmt_spend::FinalizeError::NotThePlan)
            .to_string()
            .starts_with("SP-47")
    );
}

#[test]
fn sp_48_a_file_with_every_input_final_is_the_transaction_it_holds() {
    let r = two_inputs();
    let out = sent(&r);
    let both = take_signer_signatures(wallet(), &out, &returned_by(&r, 1), 2).unwrap();
    let tx = finalize(wallet(), &both).unwrap();
    let mut done = Psbt::from_unsigned_tx(r.base.unsigned_tx.clone()).unwrap();
    for (i, input) in done.inputs.iter_mut().enumerate() {
        input.final_script_witness = Some(tx.input[i].witness.clone());
    }
    assert_eq!(transaction_of_final_file(&done), Some(tx.clone()));
    check_finished(wallet(), &out, &transaction_of_final_file(&done).unwrap()).unwrap();
    // One input not final.
    let mut part = done.clone();
    part.inputs[1].final_script_witness = None;
    assert_eq!(transaction_of_final_file(&part), None);
    // A script signature.
    let mut with_sig = done.clone();
    with_sig.inputs[0].final_script_sig = Some(bitcoin::ScriptBuf::from_bytes(vec![0x51]));
    assert_eq!(transaction_of_final_file(&with_sig), None);
    // A file that is not final at all.
    assert_eq!(transaction_of_final_file(&out), None);
}
