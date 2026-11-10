//! SR-10 to SR-15: inputs.

use bitcoin::bip32::{DerivationPath, Xpriv, Xpub};
use bitcoin::ecdsa;
use bitcoin::psbt::Psbt;
use bitcoin::sighash::EcdsaSighashType;
use bitcoin::{Amount, PublicKey, ScriptBuf, TxOut};
use openbmt_core::wallet::Keychain;

use super::kit::*;
use crate::{Refusal, MAX_MONEY_SATS};

fn tweak(f: impl FnOnce(&mut Psbt)) -> Psbt {
    let mut p = simple(50_000, 1_000);
    f(&mut p);
    p
}

fn foreign_key() -> PublicKey {
    let master = Xpriv::new_master(NET, &[9; 32]).unwrap();
    PublicKey::new(Xpub::from_priv(crate::crypto::secp(), &master).public_key)
}

#[test]
fn sr_10_refuses_an_input_without_the_previous_transaction() {
    let s = state();
    let p = tweak(|p| p.inputs[0].non_witness_utxo = None);
    assert_refused(
        &s,
        &p,
        NOW,
        Refusal::MissingPreviousTransaction { input: 0 },
        "SR-10",
    );
    // A witness UTXO alone is not enough: it could lie about the amount.
    assert!(p.inputs[0].witness_utxo.is_some());
}

#[test]
fn sr_10_refuses_a_previous_transaction_with_another_txid() {
    let s = state();
    let p = tweak(|p| {
        let prev = p.inputs[0].non_witness_utxo.as_mut().unwrap();
        // The same output script, a larger amount: another transaction.
        prev.output[0].value = Amount::from_sat(900_000);
        p.inputs[0].witness_utxo = Some(prev.output[0].clone());
    });
    assert_refused(
        &s,
        &p,
        NOW,
        Refusal::PreviousTransactionMismatch { input: 0 },
        "SR-10",
    );
}

#[test]
fn sr_10_refuses_an_output_index_the_previous_transaction_does_not_have() {
    let s = state();
    let p = tweak(|p| p.unsigned_tx.input[0].previous_output.vout = 5);
    assert_refused(
        &s,
        &p,
        NOW,
        Refusal::PreviousOutputMissing { input: 0, index: 5 },
        "SR-10",
    );
}

#[test]
fn sr_10_takes_the_amount_from_the_previous_transaction() {
    let s = state();
    let p = tweak(|p| p.inputs[0].witness_utxo = None);
    let r = review_ok(&s, &p, NOW);
    assert_eq!(r.input_total_sats(), 51_000);
    assert_eq!(r.fee_sats(), 1_000);
}

#[test]
fn sr_11_refuses_a_witness_utxo_that_differs_from_the_previous_output() {
    let s = state();
    let p = tweak(|p| {
        let mut out = p.inputs[0].witness_utxo.clone().unwrap();
        out.value = Amount::from_sat(10_000_000);
        p.inputs[0].witness_utxo = Some(out);
    });
    assert_refused(
        &s,
        &p,
        NOW,
        Refusal::WitnessUtxoMismatch { input: 0 },
        "SR-11",
    );
    let p = tweak(|p| {
        p.inputs[0].witness_utxo = Some(TxOut {
            value: Amount::from_sat(51_000),
            script_pubkey: external(5),
        });
    });
    assert_refused(
        &s,
        &p,
        NOW,
        Refusal::WitnessUtxoMismatch { input: 0 },
        "SR-11",
    );
}

#[test]
fn sr_12_refuses_an_input_without_a_usable_hint() {
    let s = state();
    let p = tweak(|p| p.inputs[0].bip32_derivation.clear());
    assert_refused(&s, &p, NOW, Refusal::NoWalletKeyHint { input: 0 }, "SR-12");
    // Hints with an unknown fingerprint are ignored.
    let p = tweak(|p| {
        p.inputs[0].bip32_derivation.clear();
        p.inputs[0].bip32_derivation.insert(
            foreign_key().inner,
            ([1, 2, 3, 4].into(), "m/48'/1'/0'/2'/0/0".parse().unwrap()),
        );
    });
    assert_refused(&s, &p, NOW, Refusal::NoWalletKeyHint { input: 0 }, "SR-12");
}

#[test]
fn sr_12_ignores_hints_whose_path_does_not_have_the_wallet_shape() {
    let s = state();
    let slot = &kit().wallet.slots()[0];
    let key = kit().wallet.derived_keys(Keychain::Receive, 0).unwrap()[0].inner;
    for path in [
        "m/48'/1'/0'/2'/0/0'",  // hardened index
        "m/48'/1'/0'/2'/2/0",   // a third keychain
        "m/48'/1'/0'/2'/0",     // too short
        "m/48'/1'/0'/2'/0/0/0", // too long
        "m/48'/1'/1'/2'/0/0",   // another account
        "m/48'/1'/0'/2'/0'/0",  // hardened keychain
    ] {
        let p = tweak(|p| {
            p.inputs[0].bip32_derivation.clear();
            p.inputs[0].bip32_derivation.insert(
                key,
                (
                    slot.origin_fingerprint,
                    path.parse::<DerivationPath>().unwrap(),
                ),
            );
        });
        assert_refused(&s, &p, NOW, Refusal::NoWalletKeyHint { input: 0 }, "SR-12");
    }
}

#[test]
fn sr_12_one_counted_hint_is_enough() {
    let s = state();
    for keep in 0..3 {
        let p = tweak(|p| {
            let all = p.inputs[0].bip32_derivation.clone();
            p.inputs[0].bip32_derivation = all.into_iter().skip(keep).take(1).collect();
        });
        assert!(s.review(&p.serialize(), NOW).is_ok(), "keep {keep}");
    }
}

#[test]
fn sr_12_refuses_a_hint_that_names_the_wrong_key() {
    let s = state();
    let slot = &kit().wallet.slots()[0];
    let p = tweak(|p| {
        p.inputs[0].bip32_derivation.insert(
            foreign_key().inner,
            (
                slot.origin_fingerprint,
                "m/48'/1'/0'/2'/0/0".parse().unwrap(),
            ),
        );
    });
    assert_refused(&s, &p, NOW, Refusal::HintKeyMismatch { input: 0 }, "SR-12");
}

#[test]
fn sr_12_refuses_hints_that_disagree() {
    let s = state();
    let k = kit();
    let first = k.wallet.derived_keys(Keychain::Receive, 0).unwrap()[0].inner;
    for (chain, index) in [(Keychain::Receive, 1), (Keychain::Change, 0)] {
        let second = k.wallet.derived_keys(chain, index).unwrap()[1].inner;
        let p = tweak(|p| {
            let mut mixed = std::collections::BTreeMap::new();
            mixed.insert(first, hints(Keychain::Receive, 0)[&first].clone());
            mixed.insert(second, hints(chain, index)[&second].clone());
            p.inputs[0].bip32_derivation = mixed;
        });
        assert_refused(&s, &p, NOW, Refusal::HintsDisagree { input: 0 }, "SR-12");
    }
}

#[test]
fn sr_12_refuses_an_input_whose_script_is_not_the_one_derived() {
    let s = state();
    // The coin is at index 3; the hints (honest keys) claim index 4.
    let mut c = coin_at(Keychain::Receive, 3, 51_000);
    let mut psbt = build(std::slice::from_ref(&c), &[pay(50_000)]);
    psbt.inputs[0].bip32_derivation = hints(Keychain::Receive, 4);
    psbt.inputs[0].witness_script = None;
    assert_refused(
        &s,
        &psbt,
        NOW,
        Refusal::ScriptNotInWallet { input: 0 },
        "SR-12",
    );
    // An output of somebody else, with the wallet's hints on it.
    c.prev.output[0].script_pubkey = external(8);
    let psbt = build(&[c], &[pay(50_000)]);
    assert_refused(
        &s,
        &psbt,
        NOW,
        Refusal::ScriptNotInWallet { input: 0 },
        "SR-12",
    );
}

#[test]
fn sr_12_accepts_inputs_on_both_keychains() {
    let s = state();
    let p = build(
        &[
            coin_at(Keychain::Receive, 7, 30_000),
            coin_at(Keychain::Change, 2, 30_000),
        ],
        &[pay(50_000)],
    );
    assert_eq!(review_ok(&s, &p, NOW).input_count(), 2);
}

#[test]
fn sr_13_refuses_a_witness_script_that_is_not_the_derived_one() {
    let s = state();
    let p = tweak(|p| {
        p.inputs[0].witness_script =
            Some(kit().wallet.witness_script(Keychain::Receive, 1).unwrap());
    });
    assert_refused(
        &s,
        &p,
        NOW,
        Refusal::WitnessScriptMismatch { input: 0 },
        "SR-13",
    );
    let p = tweak(|p| p.inputs[0].witness_script = Some(ScriptBuf::new()));
    assert_refused(
        &s,
        &p,
        NOW,
        Refusal::WitnessScriptMismatch { input: 0 },
        "SR-13",
    );
    // The derived script, or none, is fine.
    let p = tweak(|p| p.inputs[0].witness_script = None);
    assert!(s.review(&p.serialize(), NOW).is_ok());
}

#[test]
fn sr_14_keeps_a_valid_signature_from_another_wallet_key() {
    let mut s = state();
    let mut p = simple(50_000, 1_000);
    sign_as(&mut p, 0);
    let signed = sign_ok(&mut s, &p, NOW);
    assert_eq!(signed.signatures_per_input, vec![2]);
    let tx = finalize(&signed.psbt);
    assert_eq!(tx.input[0].witness.len(), 4);
}

#[test]
fn sr_14_refuses_a_signature_for_a_key_that_is_not_the_inputs() {
    let s = state();
    let mut p = simple(50_000, 1_000);
    sign_as(&mut p, 0);
    let sig = *p.inputs[0].partial_sigs.values().next().unwrap();
    p.inputs[0].partial_sigs.clear();
    p.inputs[0].partial_sigs.insert(foreign_key(), sig);
    assert_refused(&s, &p, NOW, Refusal::ForeignSignature { input: 0 }, "SR-14");
    // A wallet key at another index is foreign to this input as well.
    let mut p = simple(50_000, 1_000);
    p.inputs[0].partial_sigs.insert(
        kit().wallet.derived_keys(Keychain::Receive, 1).unwrap()[0],
        sig,
    );
    assert_refused(&s, &p, NOW, Refusal::ForeignSignature { input: 0 }, "SR-14");
    // The right key in uncompressed form is not the input's key either.
    let mut p = simple(50_000, 1_000);
    let mut key = kit().wallet.derived_keys(Keychain::Receive, 0).unwrap()[0];
    key.compressed = false;
    p.inputs[0].partial_sigs.insert(key, sig);
    assert_refused(&s, &p, NOW, Refusal::ForeignSignature { input: 0 }, "SR-14");
}

#[test]
fn sr_14_refuses_a_signature_that_is_not_sighash_all() {
    let s = state();
    let mut p = simple(50_000, 1_000);
    sign_as(&mut p, 0);
    for sig in p.inputs[0].partial_sigs.values_mut() {
        sig.sighash_type = EcdsaSighashType::None;
    }
    assert_refused(
        &s,
        &p,
        NOW,
        Refusal::SignatureSighashType { input: 0 },
        "SR-14",
    );
}

#[test]
fn sr_14_refuses_a_signature_that_does_not_verify() {
    let s = state();
    // A signature made for another transaction.
    let mut other = simple(40_000, 1_000);
    sign_as(&mut other, 0);
    let mut p = simple(50_000, 1_000);
    p.inputs[0].partial_sigs = other.inputs[0].partial_sigs.clone();
    // Same key path, so the key matches; the message does not.
    assert_refused(&s, &p, NOW, Refusal::BadSignature { input: 0 }, "SR-14");
    // Key 3's signature under key 1's name.
    let mut a = simple(50_000, 1_000);
    let mut b = a.clone();
    sign_as(&mut a, 0);
    sign_as(&mut b, 2);
    let key1 = *a.inputs[0].partial_sigs.keys().next().unwrap();
    let sig3: ecdsa::Signature = *b.inputs[0].partial_sigs.values().next().unwrap();
    a.inputs[0].partial_sigs.insert(key1, sig3);
    assert_refused(&s, &a, NOW, Refusal::BadSignature { input: 0 }, "SR-14");
}

#[test]
fn sr_14_reports_a_file_this_device_has_already_signed() {
    let mut s = state();
    let p = simple(50_000, 1_000);
    let signed = sign_ok(&mut s, &p, NOW);
    let before = s.to_bytes();
    let got = s.review(&signed.psbt, NOW).unwrap_err();
    assert_eq!(got, Refusal::AlreadySigned);
    assert_eq!(got.rule(), "SR-14");
    assert_eq!(s.to_bytes(), before);
}

#[test]
fn sr_14_a_file_signed_on_some_inputs_only_is_signed_on_the_rest() {
    let mut s = state();
    let mut p = build(&[coin(30_000), coin(30_000)], &[pay(50_000)]);
    sign_input_as(&mut p, 0, super::kit::OWN);
    let signed = sign_ok(&mut s, &p, NOW);
    assert_eq!(signed.signatures_per_input, vec![1, 1]);
    // Signing the same transaction again gives the same file (SR-53).
    let mut again = state();
    assert_eq!(sign_ok(&mut again, &p, NOW).psbt, signed.psbt);
}

#[test]
fn sr_15_refuses_an_input_amount_above_all_bitcoin() {
    let s = state();
    let p = build(&[coin(MAX_MONEY_SATS + 1)], &[pay(50_000)]);
    assert_refused(
        &s,
        &p,
        NOW,
        Refusal::InputAmountTooLarge {
            input: 0,
            amount: MAX_MONEY_SATS + 1,
        },
        "SR-15",
    );
}

#[test]
fn sr_15_adds_the_inputs_with_overflow_checks() {
    let s = state();
    let p = build(&[coin(30_000), coin(25_000), coin(10)], &[pay(50_000)]);
    assert_eq!(review_ok(&s, &p, NOW).input_total_sats(), 55_010);
}
