//! SP-50 to SP-54: joining signatures.

mod common;

use bitcoin::bip32::Xpriv;
use bitcoin::ecdsa;
use bitcoin::psbt::{Psbt, PsbtSighashType};
use bitcoin::secp256k1::Message;
use bitcoin::sighash::EcdsaSighashType;
use bitcoin::{PublicKey, Witness};
use common::*;
use openbmt_spend::{combine, CombineError, Source};

/// Two coins at the same index: their inputs have the same keys, so that a
/// signature of one input can be put on the other.
fn same_index_run(rate: u64) -> Run {
    run(
        &[coin(1, 0, 300_000), coin(2, 0, 300_000)],
        vec![(external(1), 500_000)],
        rate,
    )
}

/// A key that is not in the wallet, and its public key.
fn outsider() -> (Xpriv, PublicKey) {
    let k = Xpriv::new_master(NET, &[77; 32]).unwrap();
    let public = PublicKey::new(k.private_key.public_key(secp()));
    (k, public)
}

#[test]
fn sp_50_a_copy_with_another_unsigned_transaction_is_refused() {
    let one = same_index_run(1000);
    let two = run(
        &[coin(1, 0, 300_000), coin(2, 0, 300_000)],
        vec![(external(1), 400_000)],
        1000,
    );
    assert_ne!(one.plan.txid(), two.plan.txid());
    let e = combine(&one.base, &[one.by1.clone(), two.by2.clone()]).unwrap_err();
    assert_eq!(e, CombineError::DifferentTransaction { copy: 1 });
    assert!(e.to_string().starts_with("SP-50"));
}

#[test]
fn sp_50_a_copy_with_other_input_data_is_refused() {
    let r = same_index_run(1000);
    let mut no_prev = r.by1.clone();
    no_prev.inputs[1].non_witness_utxo = None;
    assert_eq!(
        combine(&r.base, &[no_prev]).unwrap_err(),
        CombineError::DifferentInputData { copy: 0, input: 1 }
    );
    let mut other_script = r.by1.clone();
    other_script.inputs[0].witness_script = Some(
        wallet()
            .witness_script(openbmt_core::wallet::Keychain::Receive, 3)
            .unwrap(),
    );
    assert_eq!(
        combine(&r.base, &[other_script]).unwrap_err(),
        CombineError::DifferentInputData { copy: 0, input: 0 }
    );
    let mut other_utxo = r.by1.clone();
    if let Some(u) = other_utxo.inputs[0].witness_utxo.as_mut() {
        u.value = bitcoin::Amount::from_sat(u.value.to_sat() + 1);
    }
    assert_eq!(
        combine(&r.base, &[other_utxo]).unwrap_err(),
        CombineError::DifferentInputData { copy: 0, input: 0 }
    );
}

#[test]
fn sp_50_a_previous_transaction_without_its_witnesses_is_the_same_one() {
    // Bitcoin Core writes the previous transactions of a PSBT without their
    // witnesses. The ID is the same, and so are all the outputs.
    let r = same_index_run(1000);
    let mut with_witness = r.by1.clone();
    for input in &mut with_witness.inputs {
        let prev = input.non_witness_utxo.as_mut().unwrap();
        let mut w = Witness::new();
        w.push([7u8; 64]);
        prev.input[0].witness = w;
    }
    assert!(with_witness.inputs[0].non_witness_utxo != r.base.inputs[0].non_witness_utxo);
    let joined = combine(&r.base, &[with_witness]).unwrap();
    assert!(joined.inputs.iter().all(|i| i.partial_sigs.len() == 1));
    // A previous transaction with another output is another transaction.
    let mut other = r.by1.clone();
    other.inputs[0].non_witness_utxo.as_mut().unwrap().output[0].value =
        bitcoin::Amount::from_sat(2);
    assert_eq!(
        combine(&r.base, &[other]).unwrap_err(),
        CombineError::DifferentInputData { copy: 0, input: 0 }
    );
}

#[test]
fn sp_50_a_finalized_input_is_refused() {
    let r = same_index_run(1000);
    let mut done = r.by1.clone();
    let mut w = Witness::new();
    w.push([1u8]);
    done.inputs[0].final_script_witness = Some(w);
    assert_eq!(
        combine(&r.base, &[done]).unwrap_err(),
        CombineError::InputAlreadyFinal {
            source: Source::Copy(0),
            input: 0
        }
    );
    let mut v2 = r.base.clone();
    v2.version = 2;
    assert_eq!(
        combine(&v2, &[]).unwrap_err(),
        CombineError::NotVersionZero {
            source: Source::Base
        }
    );
}

#[test]
fn sp_50_a_base_that_is_not_consistent_is_refused() {
    let r = same_index_run(1000);
    let mut no_prev = r.base.clone();
    no_prev.inputs[0].non_witness_utxo = None;
    assert_eq!(
        combine(&no_prev, &[]).unwrap_err(),
        CombineError::MissingPreviousTransaction { input: 0 }
    );
    let mut no_script = r.base.clone();
    no_script.inputs[0].witness_script = None;
    assert_eq!(
        combine(&no_script, &[]).unwrap_err(),
        CombineError::WitnessScriptShape { input: 0 }
    );
    // A witness script that is of the right shape but not the one spent.
    let mut other = r.base.clone();
    other.inputs[1].witness_script = Some(
        wallet()
            .witness_script(openbmt_core::wallet::Keychain::Receive, 9)
            .unwrap(),
    );
    assert_eq!(
        combine(&other, &[]).unwrap_err(),
        CombineError::WitnessScriptNotSpent { input: 1 }
    );
}

#[test]
fn sp_51_a_signature_by_a_key_outside_the_script_is_refused_by_name() {
    let r = same_index_run(1000);
    let (k, public) = outsider();
    let hash = bip143(
        &r.base.unsigned_tx,
        0,
        r.base.inputs[0].witness_script.as_ref().unwrap(),
        amount_of(&r.base, 0),
    );
    let sig = ecdsa::Signature {
        signature: secp().sign_ecdsa(&Message::from_digest(hash), &k.private_key),
        sighash_type: EcdsaSighashType::All,
    };
    // A good signature over the right hash, by a key that is not the
    // wallet's.
    let mut copy = r.by1.clone();
    copy.inputs[0].partial_sigs.insert(public, sig);
    let e = combine(&r.base, &[copy]).unwrap_err();
    assert_eq!(
        e,
        CombineError::ForeignKey {
            source: Source::Copy(0),
            input: 0,
            key: public
        }
    );
    assert!(e.to_string().contains(&public.to_string()));
    // A key of the wallet but at another index is foreign to this input.
    let other_index = wallet()
        .derived_keys(openbmt_core::wallet::Keychain::Receive, 8)
        .unwrap()[0];
    let mut copy = r.by1.clone();
    copy.inputs[0].partial_sigs.insert(other_index, sig);
    assert!(matches!(
        combine(&r.base, &[copy]),
        Err(CombineError::ForeignKey { .. })
    ));
}

#[test]
fn sp_52_a_hash_type_other_than_all_is_refused() {
    let r = same_index_run(1000);
    let mut copy = r.by1.clone();
    let (public, sig) = signature_by(&r.base, 0, 2);
    copy.inputs[0].partial_sigs.insert(
        public,
        ecdsa::Signature {
            signature: sig.signature,
            sighash_type: EcdsaSighashType::None,
        },
    );
    assert_eq!(
        combine(&r.base, &[copy]).unwrap_err(),
        CombineError::SighashType {
            source: Source::Copy(0),
            input: 0
        }
    );
    // The same in the input's hash-type field.
    let mut field = r.by1.clone();
    field.inputs[1].sighash_type = Some(PsbtSighashType::from(EcdsaSighashType::Single));
    assert_eq!(
        combine(&r.base, &[field]).unwrap_err(),
        CombineError::SighashType {
            source: Source::Copy(0),
            input: 1
        }
    );
    // ALL stated in the field is fine.
    let mut all = r.by1.clone();
    all.inputs[1].sighash_type = Some(PsbtSighashType::from(EcdsaSighashType::All));
    assert!(combine(&r.base, &[all]).is_ok());
}

#[test]
fn sp_53_a_signature_over_another_transaction_is_refused_by_name() {
    let one = same_index_run(1000);
    let two = run(
        &[coin(1, 0, 300_000), coin(2, 0, 300_000)],
        vec![(external(1), 400_000)],
        1000,
    );
    // Key 3 signs transaction two; the signature goes into copy of one.
    let (public, sig) = signature_by(&two.base, 0, 2);
    let mut copy = one.by1.clone();
    copy.inputs[0].partial_sigs.insert(public, sig);
    let e = combine(&one.base, &[copy]).unwrap_err();
    assert_eq!(
        e,
        CombineError::BadSignature {
            source: Source::Copy(0),
            input: 0,
            key: public
        }
    );
    assert!(e.to_string().starts_with("SP-53"));
    assert!(e.to_string().contains(&public.to_string()));
}

#[test]
fn sp_53_signatures_swapped_between_inputs_are_refused() {
    let r = same_index_run(1000);
    // Same index, so the same keys: input 0's signature is by the right key
    // but over the wrong hash for input 1.
    let (public, sig0) = signature_by(&r.base, 0, 2);
    let mut copy = r.by1.clone();
    copy.inputs[1].partial_sigs.insert(public, sig0);
    assert_eq!(
        combine(&r.base, &[copy]).unwrap_err(),
        CombineError::BadSignature {
            source: Source::Copy(0),
            input: 1,
            key: public
        }
    );
    // Inputs at different indexes: the key is not even the input's.
    let d = run(
        &[coin(1, 0, 300_000), coin(2, 1, 300_000)],
        vec![(external(1), 500_000)],
        1000,
    );
    let mut copy = d.by1.clone();
    let (k0, s0) = signature_by(&d.base, 0, 2);
    copy.inputs[1].partial_sigs.insert(k0, s0);
    assert!(matches!(
        combine(&d.base, &[copy]),
        Err(CombineError::ForeignKey { .. })
    ));
}

#[test]
fn sp_53_a_high_s_signature_is_refused() {
    let r = same_index_run(1000);
    let (public, sig) = signature_by(&r.base, 0, 2);
    let flipped = high_s(&sig);
    assert_ne!(flipped.signature, sig.signature);
    let mut copy = r.by1.clone();
    copy.inputs[0].partial_sigs.insert(public, flipped);
    assert_eq!(
        combine(&r.base, &[copy]).unwrap_err(),
        CombineError::BadSignature {
            source: Source::Copy(0),
            input: 0,
            key: public
        }
    );
}

#[test]
fn sp_53_a_signature_that_is_not_strict_der_does_not_parse() {
    let r = same_index_run(1000);
    let bytes = r.by1.serialize();
    let (_, sig) = {
        let (public, sig) = (
            r.by1.inputs[0].partial_sigs.keys().next().unwrap(),
            r.by1.inputs[0].partial_sigs.values().next().unwrap(),
        );
        (public, sig.to_vec())
    };
    let at = bytes
        .windows(sig.len())
        .position(|w| w == sig.as_slice())
        .expect("the signature is in the file");
    // The DER sequence length byte is the second byte.
    let mut broken = bytes.clone();
    broken[at + 1] ^= 0x01;
    assert!(Psbt::deserialize(&broken).is_err());
    // A padded integer (a non-minimal DER encoding) does not parse either.
    let mut lax = bytes.clone();
    lax[at + 3] = lax[at + 3].wrapping_add(1);
    assert!(Psbt::deserialize(&lax).is_err());
}

#[test]
fn sp_53_a_bad_signature_in_the_base_is_refused_too() {
    let r = same_index_run(1000);
    let (public, sig) = signature_by(&r.base, 1, 2);
    let mut base = r.base.clone();
    // Input 0 gets input 1's... same keys, other hash.
    base.inputs[0].partial_sigs.insert(public, sig);
    assert_eq!(
        combine(&base, &[]).unwrap_err(),
        CombineError::BadSignature {
            source: Source::Base,
            input: 0,
            key: public
        }
    );
}

#[test]
fn sp_54_two_signers_give_two_signatures_per_input() {
    let r = same_index_run(1000);
    let both = combine(&r.base, &[r.by1.clone(), r.by2.clone()]).unwrap();
    assert!(both.inputs.iter().all(|i| i.partial_sigs.len() == 2));
    assert_eq!(both.unsigned_tx, r.base.unsigned_tx);
    // The base gains only signatures.
    let mut stripped = both.clone();
    for i in &mut stripped.inputs {
        i.partial_sigs.clear();
    }
    assert_eq!(stripped, r.base);
}

#[test]
fn sp_54_the_same_signer_twice_is_kept_once_and_order_does_not_matter() {
    let r = same_index_run(1000);
    let twice = combine(&r.base, &[r.by1.clone(), r.by1.clone()]).unwrap();
    assert!(twice.inputs.iter().all(|i| i.partial_sigs.len() == 1));
    let ab = combine(&r.base, &[r.by1.clone(), r.by2.clone()]).unwrap();
    let ba = combine(&r.base, &[r.by2.clone(), r.by1.clone()]).unwrap();
    assert_eq!(ab, ba);
    // Combining in two steps gives the same.
    let step = combine(
        &combine(&r.base, std::slice::from_ref(&r.by1)).unwrap(),
        std::slice::from_ref(&r.by2),
    )
    .unwrap();
    assert_eq!(step, ab);
    // A second, different but valid signature by one key: the smaller one
    // wins whatever the order. Both are valid, so this is not an error.
    let (public, sig) = signature_by(&r.base, 0, 0);
    let other = {
        // Another valid low-S signature by the same key over the same hash:
        // sign with extra nonce data.
        let (_, i) = (0, 0usize);
        let hash = bip143(
            &r.base.unsigned_tx,
            i,
            r.base.inputs[i].witness_script.as_ref().unwrap(),
            amount_of(&r.base, i),
        );
        let child = kit().accounts[0]
            .derive_priv(
                secp(),
                &[
                    bitcoin::bip32::ChildNumber::Normal { index: 0 },
                    bitcoin::bip32::ChildNumber::Normal { index: 0 },
                ],
            )
            .unwrap();
        ecdsa::Signature {
            signature: secp().sign_ecdsa_with_noncedata(
                &Message::from_digest(hash),
                &child.private_key,
                &[9u8; 32],
            ),
            sighash_type: EcdsaSighashType::All,
        }
    };
    assert_ne!(other.signature, sig.signature);
    let mut a = r.base.clone();
    a.inputs[0].partial_sigs.insert(public, sig);
    let mut b = r.base.clone();
    b.inputs[0].partial_sigs.insert(public, other);
    assert_eq!(
        combine(&r.base, &[a.clone(), b.clone()]).unwrap(),
        combine(&r.base, &[b, a]).unwrap()
    );
}
