//! SR-01 to SR-09: reading the transaction file.

use bitcoin::absolute::LockTime;
use bitcoin::bip32::{DerivationPath, Xpriv, Xpub};
use bitcoin::consensus::encode::serialize;
use bitcoin::hashes::{hash160, ripemd160, sha256, sha256d, Hash};
use bitcoin::psbt::{raw, Input, Output, Psbt, PsbtSighashType};
use bitcoin::secp256k1::XOnlyPublicKey;
use bitcoin::sighash::{EcdsaSighashType, TapSighashType};
use bitcoin::taproot::{self, ControlBlock, TapLeafHash, TapNodeHash};
use bitcoin::transaction::Version;
use bitcoin::{OutPoint, ScriptBuf, Sequence, TxIn, Witness};

use super::kit::*;
use crate::{Refusal, MAX_PSBT_BYTES};

fn tweak(f: impl FnOnce(&mut Psbt)) -> Psbt {
    let mut p = simple(50_000, 1_000);
    f(&mut p);
    p
}

fn xonly() -> XOnlyPublicKey {
    kit().wallet.derived_keys(bitcoin_chain(), 0).unwrap()[0]
        .inner
        .x_only_public_key()
        .0
}

fn bitcoin_chain() -> openbmt_core::wallet::Keychain {
    openbmt_core::wallet::Keychain::Receive
}

/// Insert one raw key-value pair into the global map of serialized `psbt`.
fn with_global_pair(psbt: &Psbt, pair: &[u8]) -> Vec<u8> {
    let bytes = psbt.serialize();
    let tx_len = serialize(&psbt.unsigned_tx).len();
    let len_bytes = serialize(&bitcoin::consensus::encode::VarInt(tx_len as u64)).len();
    // magic and separator, key length, key type, value length, value.
    let end = 5 + 1 + 1 + len_bytes + tx_len;
    let mut out = bytes[..end].to_vec();
    out.extend_from_slice(pair);
    out.extend_from_slice(&bytes[end..]);
    out
}

#[test]
fn sr_01_refuses_bytes_that_are_not_a_psbt() {
    let s = state();
    for bytes in [&[][..], b"psbt", b"psbt\xff", b"not a psbt at all"] {
        assert_eq!(s.review(bytes, NOW).unwrap_err(), Refusal::NotAPsbt);
    }
    assert_eq!(Refusal::NotAPsbt.rule(), "SR-01");
}

#[test]
fn sr_01_refuses_psbt_version_2() {
    let s = state();
    let p = simple(50_000, 1_000);
    // PSBT_GLOBAL_VERSION (0xfb), value 2 as a little-endian u32.
    let bytes = with_global_pair(&p, &[0x01, 0xfb, 0x04, 2, 0, 0, 0]);
    let got = s.review(&bytes, NOW).unwrap_err();
    assert_eq!(got, Refusal::NotAPsbt);
    assert_eq!(got.rule(), "SR-01");
    // The same file with version 0 spelled out is a version 0 file.
    let zero = with_global_pair(&p, &[0x01, 0xfb, 0x04, 0, 0, 0, 0]);
    assert!(s.review(&zero, NOW).is_ok());
}

#[test]
fn sr_01_refuses_a_duplicate_key() {
    let s = state();
    let p = simple(50_000, 1_000);
    let bytes = p.serialize();
    let tx_len = serialize(&p.unsigned_tx).len();
    let len_bytes = serialize(&bitcoin::consensus::encode::VarInt(tx_len as u64)).len();
    let end = 5 + 1 + 1 + len_bytes + tx_len;
    // The unsigned transaction pair, a second time.
    let dup = with_global_pair(&p, &bytes[5..end]);
    let got = s.review(&dup, NOW).unwrap_err();
    assert_eq!(got, Refusal::NotAPsbt);
    assert_eq!(got.rule(), "SR-01");
    // A repeated unknown global key is refused as well.
    let unknown = [0x02, 0x77, 0x01, 0x01, 0x05];
    let mut twice = with_global_pair(&p, &unknown);
    let tx_end = end;
    twice.splice(tx_end..tx_end, unknown.iter().copied());
    assert_eq!(s.review(&twice, NOW).unwrap_err(), Refusal::NotAPsbt);
}

#[test]
fn sr_02_refuses_a_file_over_the_size_limit_before_parsing() {
    let s = state();
    let big = vec![0u8; MAX_PSBT_BYTES + 1];
    let got = s.review(&big, NOW).unwrap_err();
    assert_eq!(
        got,
        Refusal::FileTooLarge {
            len: MAX_PSBT_BYTES + 1
        }
    );
    assert_eq!(got.rule(), "SR-02");
    // At the limit the bytes are read, and these are not a PSBT.
    let at_limit = vec![0u8; MAX_PSBT_BYTES];
    assert_eq!(s.review(&at_limit, NOW).unwrap_err(), Refusal::NotAPsbt);
}

#[test]
fn sr_02_refuses_no_inputs_and_too_many_inputs() {
    let s = state();
    let none = tweak(|p| {
        p.unsigned_tx.input.clear();
        p.inputs.clear();
    });
    assert_refused(&s, &none, NOW, Refusal::InputCount { found: 0 }, "SR-02");
    let many = tweak(|p| {
        for n in 0..1_001u32 {
            p.unsigned_tx.input.push(TxIn {
                previous_output: OutPoint {
                    txid: bitcoin::Txid::from_byte_array([7; 32]),
                    vout: n,
                },
                script_sig: ScriptBuf::new(),
                sequence: Sequence::MAX,
                witness: Witness::new(),
            });
            p.inputs.push(Input::default());
        }
    });
    assert_refused(
        &s,
        &many,
        NOW,
        Refusal::InputCount { found: 1_002 },
        "SR-02",
    );
}

#[test]
fn sr_02_refuses_no_outputs_and_too_many_outputs() {
    let s = state();
    let none = tweak(|p| {
        p.unsigned_tx.output.clear();
        p.outputs.clear();
    });
    assert_refused(&s, &none, NOW, Refusal::OutputCount { found: 0 }, "SR-02");
    let many = tweak(|p| {
        for _ in 0..100 {
            p.unsigned_tx.output.push(p.unsigned_tx.output[0].clone());
            p.outputs.push(Output::default());
        }
    });
    assert_refused(&s, &many, NOW, Refusal::OutputCount { found: 101 }, "SR-02");
}

#[test]
fn sr_02_accepts_exactly_one_hundred_outputs() {
    let s = state();
    let outs: Vec<_> = (0..100).map(|_| pay(1_000)).collect();
    let p = build(&[coin(101_000)], &outs);
    let r = review_ok(&s, &p, NOW);
    assert_eq!(r.outputs().len(), 100);
    assert_eq!(r.other_total_sats(), 100_000);
}

#[test]
fn sr_03_refuses_other_transaction_versions() {
    let s = state();
    for v in [1, 3, 0, -1] {
        let p = tweak(|p| p.unsigned_tx.version = Version(v));
        assert_refused(
            &s,
            &p,
            NOW,
            Refusal::TransactionVersion { found: v },
            "SR-03",
        );
    }
}

#[test]
fn sr_03_refuses_a_lock_time_that_is_a_time() {
    let s = state();
    let p = tweak(|p| p.unsigned_tx.lock_time = LockTime::from_consensus(500_000_000));
    assert_refused(
        &s,
        &p,
        NOW,
        Refusal::LockTimeNotHeight {
            lock_time: 500_000_000,
        },
        "SR-03",
    );
    // A block height is fine, the highest one included.
    for h in [0, 1, 499_999_999] {
        let p = tweak(|p| p.unsigned_tx.lock_time = LockTime::from_consensus(h));
        assert!(s.review(&p.serialize(), NOW).is_ok(), "height {h}");
    }
}

#[test]
fn sr_03_accepts_a_tip_height_lock_time_with_the_sequence_the_spend_library_sets() {
    // The spend library sets the lock time to the chain tip (SP-37) and the
    // sequence to 0xFFFFFFFD on every input: both are accepted.
    let s = state();
    for h in [850_000, 1_200_000] {
        let p = tweak(|p| {
            p.unsigned_tx.lock_time = LockTime::from_consensus(h);
            for input in &mut p.unsigned_tx.input {
                input.sequence = Sequence(0xFFFF_FFFD);
            }
        });
        assert!(s.review(&p.serialize(), NOW).is_ok(), "height {h}");
    }
}

#[test]
fn sr_03_refuses_a_relative_time_lock_in_any_input() {
    let s = state();
    for seq in [0, 1, 0x0040_0000, 0xFFFF_FFFC] {
        let p = tweak(|p| p.unsigned_tx.input[0].sequence = Sequence(seq));
        assert_refused(
            &s,
            &p,
            NOW,
            Refusal::RelativeTimeLock {
                input: 0,
                sequence: seq,
            },
            "SR-03",
        );
    }
    for seq in [0xFFFF_FFFD, 0xFFFF_FFFE, 0xFFFF_FFFF] {
        let p = tweak(|p| p.unsigned_tx.input[0].sequence = Sequence(seq));
        assert!(s.review(&p.serialize(), NOW).is_ok(), "{seq:#x}");
    }
    // The second input is checked too.
    let mut p = build(&[coin(30_000), coin(30_000)], &[pay(50_000)]);
    p.unsigned_tx.input[1].sequence = Sequence(5);
    assert_refused(
        &s,
        &p,
        NOW,
        Refusal::RelativeTimeLock {
            input: 1,
            sequence: 5,
        },
        "SR-03",
    );
}

#[test]
fn sr_04_refuses_an_input_that_appears_twice() {
    let s = state();
    let c = coin(60_000);
    let p = build(&[c.clone(), c], &[pay(50_000)]);
    assert_refused(&s, &p, NOW, Refusal::DuplicateInput { input: 1 }, "SR-04");
}

#[test]
fn sr_05_refuses_an_input_that_is_already_final() {
    let s = state();
    let p = tweak(|p| p.inputs[0].final_script_witness = Some(Witness::new()));
    assert_refused(
        &s,
        &p,
        NOW,
        Refusal::InputAlreadyFinal { input: 0 },
        "SR-05",
    );
    let p = tweak(|p| p.inputs[0].final_script_sig = Some(ScriptBuf::new()));
    assert_refused(
        &s,
        &p,
        NOW,
        Refusal::InputAlreadyFinal { input: 0 },
        "SR-05",
    );
}

#[test]
fn sr_06_refuses_forbidden_input_fields() {
    let s = state();
    let sig = taproot::Signature {
        signature: bitcoin::secp256k1::schnorr::Signature::from_slice(&[1; 64]).unwrap(),
        sighash_type: TapSighashType::Default,
    };
    let leaf = TapLeafHash::from_byte_array([3; 32]);
    let mut control = vec![0xc0];
    control.extend_from_slice(&xonly().serialize());
    let control = ControlBlock::decode(&control).unwrap();
    type Edit = Box<dyn Fn(&mut Input)>;
    let cases: Vec<(&str, Edit)> = vec![
        (
            "a redeem script",
            Box::new(|i| i.redeem_script = Some(ScriptBuf::new())),
        ),
        (
            "a Taproot key signature",
            Box::new(move |i| i.tap_key_sig = Some(sig)),
        ),
        (
            "Taproot script signatures",
            Box::new(move |i| {
                i.tap_script_sigs.insert((xonly(), leaf), sig);
            }),
        ),
        (
            "Taproot scripts",
            Box::new(move |i| {
                i.tap_scripts.insert(
                    control.clone(),
                    (ScriptBuf::new(), taproot::LeafVersion::TapScript),
                );
            }),
        ),
        (
            "Taproot key origins",
            Box::new(|i| {
                i.tap_key_origins.insert(
                    xonly(),
                    (vec![], ([0; 4].into(), DerivationPath::default())),
                );
            }),
        ),
        (
            "a Taproot internal key",
            Box::new(|i| i.tap_internal_key = Some(xonly())),
        ),
        (
            "a Taproot Merkle root",
            Box::new(|i| i.tap_merkle_root = Some(TapNodeHash::from_byte_array([4; 32]))),
        ),
        (
            "a RIPEMD-160 preimage",
            Box::new(|i| {
                i.ripemd160_preimages
                    .insert(ripemd160::Hash::hash(b"x"), b"x".to_vec());
            }),
        ),
        (
            "a SHA-256 preimage",
            Box::new(|i| {
                i.sha256_preimages
                    .insert(sha256::Hash::hash(b"x"), b"x".to_vec());
            }),
        ),
        (
            "a HASH160 preimage",
            Box::new(|i| {
                i.hash160_preimages
                    .insert(hash160::Hash::hash(b"x"), b"x".to_vec());
            }),
        ),
        (
            "a HASH256 preimage",
            Box::new(|i| {
                i.hash256_preimages
                    .insert(sha256d::Hash::hash(b"x"), b"x".to_vec());
            }),
        ),
    ];
    for (field, f) in cases {
        let p = tweak(|p| f(&mut p.inputs[0]));
        assert_refused(
            &s,
            &p,
            NOW,
            Refusal::ForbiddenInputField { input: 0, field },
            "SR-06",
        );
    }
}

#[test]
fn sr_06_refuses_forbidden_output_fields() {
    let s = state();
    let p = tweak(|p| p.outputs[0].redeem_script = Some(ScriptBuf::new()));
    assert_refused(
        &s,
        &p,
        NOW,
        Refusal::ForbiddenOutputField {
            output: 0,
            field: "a redeem script",
        },
        "SR-06",
    );
    let p = tweak(|p| p.outputs[0].tap_internal_key = Some(xonly()));
    assert_refused(
        &s,
        &p,
        NOW,
        Refusal::ForbiddenOutputField {
            output: 0,
            field: "a Taproot internal key",
        },
        "SR-06",
    );
    let p = tweak(|p| {
        p.outputs[0].tap_key_origins.insert(
            xonly(),
            (vec![], ([0; 4].into(), DerivationPath::default())),
        );
    });
    assert_refused(
        &s,
        &p,
        NOW,
        Refusal::ForbiddenOutputField {
            output: 0,
            field: "Taproot key origins",
        },
        "SR-06",
    );
}

#[test]
fn sr_07_refuses_a_sighash_type_other_than_all() {
    let s = state();
    for t in [
        EcdsaSighashType::None,
        EcdsaSighashType::Single,
        EcdsaSighashType::AllPlusAnyoneCanPay,
    ] {
        let p = tweak(|p| p.inputs[0].sighash_type = Some(PsbtSighashType::from(t)));
        assert_refused(
            &s,
            &p,
            NOW,
            Refusal::SighashType {
                input: 0,
                found: t as u32,
            },
            "SR-07",
        );
    }
    let p = tweak(|p| p.inputs[0].sighash_type = Some(PsbtSighashType::from_u32(0)));
    assert_refused(
        &s,
        &p,
        NOW,
        Refusal::SighashType { input: 0, found: 0 },
        "SR-07",
    );
    let p = tweak(|p| {
        p.inputs[0].sighash_type = Some(PsbtSighashType::from(EcdsaSighashType::All));
    });
    assert!(s.review(&p.serialize(), NOW).is_ok());
}

fn foreign_xpub() -> Xpub {
    let master = Xpriv::new_master(NET, &[9; 32]).unwrap();
    Xpub::from_priv(crate::crypto::secp(), &master)
}

#[test]
fn sr_08_accepts_a_global_xpub_of_the_wallet_with_its_origin() {
    let s = state();
    let p = tweak(|p| {
        for (slot, x) in kit().wallet.slots().iter().zip(kit().xpubs) {
            p.xpub
                .insert(x, (slot.origin_fingerprint, slot.origin_path.clone()));
        }
    });
    assert!(s.review(&p.serialize(), NOW).is_ok());
}

#[test]
fn sr_08_refuses_a_global_xpub_that_is_not_the_wallets() {
    let s = state();
    let slot = &kit().wallet.slots()[0];
    let p = tweak(|p| {
        p.xpub.insert(
            foreign_xpub(),
            (slot.origin_fingerprint, slot.origin_path.clone()),
        );
    });
    assert_refused(&s, &p, NOW, Refusal::ForeignXpub, "SR-08");
    // A key of the wallet with another slot's origin.
    let other = &kit().wallet.slots()[1];
    let p = tweak(|p| {
        p.xpub.insert(
            kit().xpubs[0],
            (other.origin_fingerprint, other.origin_path.clone()),
        );
    });
    assert_refused(&s, &p, NOW, Refusal::ForeignXpub, "SR-08");
    // A key of the wallet with another path.
    let p = tweak(|p| {
        p.xpub.insert(
            kit().xpubs[0],
            (slot.origin_fingerprint, "m/48'/1'/1'/2'".parse().unwrap()),
        );
    });
    assert_refused(&s, &p, NOW, Refusal::ForeignXpub, "SR-08");
}

#[test]
fn sr_09_nothing_from_the_file_is_passed_on_except_what_the_signer_checked() {
    let honest = simple(50_000, 1_000);
    let mut noisy = honest.clone();
    let key = |n: u8| raw::Key {
        type_value: 0x77,
        key: vec![n],
    };
    let prop = |n: u8| raw::ProprietaryKey {
        prefix: b"junk".to_vec(),
        subtype: n,
        key: vec![n],
    };
    noisy.unknown.insert(key(1), vec![1, 2, 3]);
    noisy.proprietary.insert(prop(1), vec![1]);
    noisy.inputs[0].unknown.insert(key(2), vec![4]);
    noisy.inputs[0].proprietary.insert(prop(2), vec![5]);
    noisy.outputs[0].unknown.insert(key(3), vec![6]);
    noisy.outputs[0].proprietary.insert(prop(3), vec![7]);
    // A hint for a key that is not the wallet's, and a counted extra
    // witness script on an output that is not the wallet's.
    noisy.inputs[0].bip32_derivation.insert(
        foreign_xpub().public_key,
        ([1, 2, 3, 4].into(), DerivationPath::default()),
    );
    noisy.outputs[0].witness_script = Some(ScriptBuf::from_bytes(vec![0x51]));
    let mut a = state();
    let mut b = state();
    let clean = sign_ok(&mut a, &honest, NOW);
    let from_noisy = sign_ok(&mut b, &noisy, NOW);
    assert_eq!(clean.psbt, from_noisy.psbt);
    let out = Psbt::deserialize(&from_noisy.psbt).unwrap();
    assert!(out.unknown.is_empty() && out.proprietary.is_empty() && out.xpub.is_empty());
    assert!(out.inputs[0].unknown.is_empty() && out.inputs[0].proprietary.is_empty());
    assert_eq!(out.inputs[0].bip32_derivation.len(), 3);
    assert!(out.outputs[0].unknown.is_empty() && out.outputs[0].witness_script.is_none());
}
