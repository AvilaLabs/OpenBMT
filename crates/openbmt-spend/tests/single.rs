//! SP-80 to SP-87: spending from a wallet with one key, and the same
//! properties as the 2-of-3 tests (SP-70, SP-71) over that path.
//!
//! The keys come from fixed seeds: test only, never use. Signatures are made
//! here over a BIP 143 hash written out again in `common`, with the
//! pay-to-key-hash script code, so the checks do not depend on the code
//! under test.

mod common;

use std::collections::BTreeMap;
use std::str::FromStr;
use std::sync::OnceLock;

use bitcoin::absolute::LockTime;
use bitcoin::bip32::{ChildNumber, DerivationPath, Fingerprint, Xpriv, Xpub};
use bitcoin::ecdsa;
use bitcoin::hashes::Hash;
use bitcoin::psbt::{Input, Output, Psbt, PsbtSighashType};
use bitcoin::secp256k1::Message;
use bitcoin::sighash::EcdsaSighashType;
use bitcoin::transaction::Version;
use bitcoin::{
    Amount, Network, OutPoint, PublicKey, ScriptBuf, Sequence, Transaction, TxIn, TxOut, Txid,
    WScriptHash, Witness,
};
use common::{amount_of, bip143, external, high_s, request, secp, Rng};
use openbmt_core::wallet::{KeySlot, Keychain, WalletDescription};
use openbmt_core::wallet_single::{ImportedWallet, SingleKeyWallet};
use openbmt_spend::{
    check_finished, combine, combine_wallet, consolidation_cost_sats_single,
    consolidation_size_single, finalize, finalize_plan, plan, psbt_for_signer,
    take_signer_signatures, take_wallet_signature, transaction_of_final_file, ChangePosition, Coin,
    CoinError, CombineError, FinalizeError, Plan, PlanError, PsbtError, SignerFileError,
    SpendRequest, WalletKind,
};

// ---- the kit -------------------------------------------------------------

struct Kit {
    /// The account key `84'/0'/0'` of the wallet with one key.
    account: Xpriv,
    /// An account key of another wallet, for signatures by a foreign key.
    other: Xpriv,
    fingerprint: Fingerprint,
    single: ImportedWallet,
    multisig: ImportedWallet,
}

fn account_of(seed: u8, path: &str) -> (Xpriv, Fingerprint) {
    let master = Xpriv::new_master(Network::Bitcoin, &[seed; 32]).unwrap();
    let path = DerivationPath::from_str(path).unwrap();
    (
        master.derive_priv(secp(), &path).unwrap(),
        master.fingerprint(secp()),
    )
}

fn kit() -> &'static Kit {
    static KIT: OnceLock<Kit> = OnceLock::new();
    KIT.get_or_init(|| {
        let (account, fingerprint) = account_of(0x51, "m/84'/0'/0'");
        let (other, _) = account_of(0x52, "m/84'/0'/0'");
        let single = SingleKeyWallet::new(fingerprint, 0, Xpub::from_priv(secp(), &account))
            .expect("a wallet with one key");
        let slots: Vec<KeySlot> = (0..3u8)
            .map(|n| {
                let (key, fp) = account_of(0x61 + n, "m/48'/0'/0'/2'");
                KeySlot {
                    index: n + 1,
                    origin_fingerprint: fp,
                    origin_path: DerivationPath::from_str("m/48'/0'/0'/2'").unwrap(),
                    xpub: Xpub::from_priv(secp(), &key),
                }
            })
            .collect();
        let multisig = WalletDescription::new(Network::Bitcoin, slots.try_into().unwrap())
            .expect("a wallet of three keys");
        Kit {
            account,
            other,
            fingerprint,
            single: ImportedWallet::Single(single),
            multisig: ImportedWallet::Multisig(multisig),
        }
    })
}

fn single() -> &'static ImportedWallet {
    &kit().single
}

fn single_inner() -> &'static SingleKeyWallet {
    match single() {
        ImportedWallet::Single(w) => w,
        ImportedWallet::Multisig(_) => unreachable!(),
    }
}

fn multisig() -> &'static ImportedWallet {
    &kit().multisig
}

fn multisig_inner() -> &'static WalletDescription {
    match multisig() {
        ImportedWallet::Multisig(w) => w,
        ImportedWallet::Single(_) => unreachable!(),
    }
}

// ---- coins ---------------------------------------------------------------

/// A coin paying `script` of `sats`, with `confirmations`. `tag` makes the
/// funding transaction unique.
fn coin_paying(
    tag: u32,
    script: ScriptBuf,
    chain: Keychain,
    index: u32,
    sats: u64,
    confirmations: u32,
) -> Coin {
    let mut funding = [0u8; 32];
    funding[..4].copy_from_slice(&tag.to_le_bytes());
    let prev = Transaction {
        version: Version::TWO,
        lock_time: LockTime::ZERO,
        input: vec![TxIn {
            previous_output: OutPoint {
                txid: Txid::from_byte_array(funding),
                vout: 0,
            },
            script_sig: ScriptBuf::new(),
            sequence: Sequence::MAX,
            witness: Witness::new(),
        }],
        output: vec![
            TxOut {
                value: Amount::from_sat(1),
                script_pubkey: external(250),
            },
            TxOut {
                value: Amount::from_sat(sats),
                script_pubkey: script,
            },
        ],
    };
    Coin {
        outpoint: OutPoint {
            txid: prev.compute_txid(),
            vout: 1,
        },
        prev_tx: prev,
        vout: 1,
        chain,
        index,
        confirmations,
    }
}

fn coin_at(tag: u32, chain: Keychain, index: u32, sats: u64, confirmations: u32) -> Coin {
    let script = single_inner().script(chain, index).unwrap();
    coin_paying(tag, script, chain, index, sats, confirmations)
}

fn coin(tag: u32, index: u32, sats: u64) -> Coin {
    coin_at(tag, Keychain::Receive, index, sats, 6)
}

fn multisig_coin(tag: u32, index: u32, sats: u64) -> Coin {
    let script = multisig_inner().script(Keychain::Receive, index).unwrap();
    coin_paying(tag, script, Keychain::Receive, index, sats, 6)
}

fn plan_single(coins: &[Coin], payments: Vec<(ScriptBuf, u64)>, rate: u64) -> Plan {
    plan(single(), coins, &request(payments, rate)).expect("a plan")
}

/// A plan over two coins, so that it has a change output, and its file.
fn two_inputs() -> (Plan, Psbt) {
    let p = plan_single(
        &[
            coin(1, 2, 300_000),
            coin_at(2, Keychain::Change, 4, 400_000, 6),
        ],
        vec![(external(1), 500_000)],
        1000,
    );
    let base = p.to_psbt(single()).unwrap();
    (p, base)
}

// ---- signing, with the independent hash -----------------------------------

/// The chain and index in the one hint of input `i`.
fn place(psbt: &Psbt, i: usize) -> (u32, u32) {
    let (_, path) = psbt.inputs[i].bip32_derivation.values().next().unwrap();
    let steps: &[ChildNumber] = path.as_ref();
    let n = steps.len();
    let (ChildNumber::Normal { index: chain }, ChildNumber::Normal { index }) =
        (steps[n - 2], steps[n - 1])
    else {
        panic!("hardened step")
    };
    (chain, index)
}

/// A signature on input `i` by the key at the input's place under `root`.
fn sign_input(psbt: &Psbt, i: usize, root: &Xpriv) -> (PublicKey, ecdsa::Signature) {
    let (chain, index) = place(psbt, i);
    let child = root
        .derive_priv(
            secp(),
            &[
                ChildNumber::Normal { index: chain },
                ChildNumber::Normal { index },
            ],
        )
        .unwrap();
    let public = PublicKey::new(child.private_key.public_key(secp()));
    let script_code = ScriptBuf::new_p2pkh(&public.pubkey_hash());
    let hash = bip143(&psbt.unsigned_tx, i, &script_code, amount_of(psbt, i));
    let signature = secp().sign_ecdsa(&Message::from_digest(hash), &child.private_key);
    (
        public,
        ecdsa::Signature {
            signature,
            sighash_type: EcdsaSighashType::All,
        },
    )
}

/// `psbt` with a signature by the wallet's key on every input.
fn signed(psbt: &Psbt) -> Psbt {
    let mut out = psbt.clone();
    for i in 0..out.inputs.len() {
        let (public, sig) = sign_input(psbt, i, &kit().account);
        out.inputs[i].partial_sigs.insert(public, sig);
    }
    out
}

/// What a wallet of another make returns: only the transaction and its
/// signatures, in a file of its own.
fn returned(psbt: &Psbt) -> Psbt {
    let mut out = Psbt::from_unsigned_tx(psbt.unsigned_tx.clone()).unwrap();
    for (into, from) in out.inputs.iter_mut().zip(&signed(psbt).inputs) {
        into.partial_sigs = from.partial_sigs.clone();
    }
    out
}

fn own_key(psbt: &Psbt, i: usize) -> PublicKey {
    sign_input(psbt, i, &kit().account).0
}

// ---- SP-80 ---------------------------------------------------------------

#[test]
fn sp_80_every_entry_point_takes_an_imported_wallet() {
    let (p, base) = two_inputs();
    // The planned file, the combine, the finish and the checks, all by the
    // one handle the owner holds.
    assert_eq!(p.inputs().len(), 2);
    let joined = combine_wallet(single(), &base, &[signed(&base)]).unwrap();
    let tx = finalize(single(), &joined).unwrap();
    assert_eq!(finalize_plan(single(), &joined, &p).unwrap(), tx);
    assert_eq!(check_finished(single(), &base, &tx).unwrap(), p.fee_sats());
    assert!(take_wallet_signature(single(), &base, &returned(&base)).is_ok());
    // A wallet of three keys, handed in the same way, is the 2-of-3 path:
    // the same plan as with the bare description.
    let coins = [multisig_coin(7, 1, 300_000), multisig_coin(8, 2, 300_000)];
    let req = request(vec![(external(1), 400_000)], 1500);
    let by_handle = plan(multisig(), &coins, &req).unwrap();
    let by_description = plan(multisig_inner(), &coins, &req).unwrap();
    assert_eq!(by_handle, by_description);
    assert_eq!(
        by_handle.to_psbt(multisig()).unwrap(),
        by_description.to_psbt(multisig_inner()).unwrap()
    );
    // Each kind of file is shaped for its kind.
    let file = by_handle.to_psbt(multisig()).unwrap();
    assert!(file.inputs[0].witness_script.is_some());
    assert!(base.inputs[0].witness_script.is_none());
}

#[test]
fn sp_80_a_coin_of_the_other_kind_is_refused_naming_the_kind() {
    let other = multisig_coin(1, 0, 100_000);
    // A coin of the 2-of-3 wallet in the wallet with one key.
    let e = other.check(single()).unwrap_err();
    assert_eq!(
        e,
        CoinError::WrongKind {
            outpoint: other.outpoint,
            expected: WalletKind::Single
        }
    );
    assert!(e.to_string().starts_with("SP-80"));
    assert!(e.to_string().contains("one key"));
    let r = plan(
        single(),
        std::slice::from_ref(&other),
        &request(vec![(external(1), 10_000)], 1000),
    );
    assert_eq!(
        r.unwrap_err(),
        PlanError::Coin(CoinError::WrongKind {
            outpoint: other.outpoint,
            expected: WalletKind::Single
        })
    );
    // A coin of the wallet with one key in the 2-of-3 wallet.
    let mine = coin(2, 0, 100_000);
    let e = mine.check(multisig()).unwrap_err();
    assert_eq!(
        e,
        CoinError::WrongKind {
            outpoint: mine.outpoint,
            expected: WalletKind::Multisig
        }
    );
    assert!(e.to_string().contains("three keys"));
    // A script that is neither wallet's stays a coin that is not the
    // wallet's.
    let mut foreign = coin(3, 0, 100_000);
    foreign.prev_tx.output[1].script_pubkey = external(9);
    foreign.outpoint.txid = foreign.prev_tx.compute_txid();
    assert!(matches!(
        foreign.check(single()),
        Err(CoinError::ScriptNotInWallet { .. })
    ));
    // The 2-of-3 description on its own keeps the refusal it had.
    assert!(matches!(
        mine.check(multisig_inner()),
        Err(CoinError::ScriptNotInWallet { .. })
    ));
}

#[test]
fn sp_80_a_file_or_a_transaction_of_the_other_kind_is_refused_naming_the_kind() {
    let (_, one_key) = two_inputs();
    let multi_plan = plan(
        multisig(),
        &[multisig_coin(1, 0, 300_000), multisig_coin(2, 1, 300_000)],
        &request(vec![(external(1), 400_000)], 1000),
    )
    .unwrap();
    let three_keys = multi_plan.to_psbt(multisig()).unwrap();

    // combine
    assert_eq!(
        combine_wallet(single(), &three_keys, &[]).unwrap_err(),
        CombineError::WrongKind {
            input: 0,
            expected: WalletKind::Single
        }
    );
    assert_eq!(
        combine_wallet(multisig(), &one_key, &[]).unwrap_err(),
        CombineError::WrongKind {
            input: 0,
            expected: WalletKind::Multisig
        }
    );
    // finalize and finalize_plan
    assert_eq!(
        finalize(single(), &three_keys).unwrap_err(),
        FinalizeError::WrongKind {
            input: 0,
            expected: WalletKind::Single
        }
    );
    assert_eq!(
        finalize(multisig(), &signed(&one_key)).unwrap_err(),
        FinalizeError::WrongKind {
            input: 0,
            expected: WalletKind::Multisig
        }
    );
    assert!(matches!(
        finalize_plan(multisig(), &signed(&one_key), &multi_plan),
        Err(FinalizeError::NotThePlan)
    ));
    // A finished transaction of one wallet in the check of the other.
    let tx = finalize(single(), &signed(&one_key)).unwrap();
    assert_eq!(
        check_finished(multisig(), &one_key, &tx).unwrap_err(),
        SignerFileError::Finished(FinalizeError::WrongKind {
            input: 0,
            expected: WalletKind::Multisig
        })
    );
    // The steps that take a wallet and a slot or no slot.
    assert_eq!(
        psbt_for_signer(single(), &one_key).unwrap_err(),
        SignerFileError::WrongKind {
            expected: WalletKind::Multisig
        }
    );
    assert_eq!(
        take_signer_signatures(single(), &one_key, &one_key, 1).unwrap_err(),
        SignerFileError::WrongKind {
            expected: WalletKind::Multisig
        }
    );
    assert_eq!(
        take_wallet_signature(multisig(), &three_keys, &three_keys).unwrap_err(),
        SignerFileError::WrongKind {
            expected: WalletKind::Single
        }
    );
    // The bare 2-of-3 functions are as they were: a file with no witness
    // script is a file with no witness script.
    assert_eq!(
        combine(&one_key, &[]).unwrap_err(),
        CombineError::WitnessScriptShape { input: 0 }
    );
    // The same wrong kind, said in words that name the rule.
    let text = SignerFileError::WrongKind {
        expected: WalletKind::Single,
    }
    .to_string();
    assert!(
        text.starts_with("SP-80") && text.contains("one key"),
        "{text}"
    );
    let text = PsbtError::Coin(CoinError::WrongKind {
        outpoint: OutPoint::null(),
        expected: WalletKind::Multisig,
    })
    .to_string();
    assert!(text.starts_with("SP-80"), "{text}");
}

// ---- SP-81 ---------------------------------------------------------------

/// The weight of a transaction of `inputs` inputs of a wallet with one key,
/// signed with the longest signatures, written out from the shape: no
/// shared code with the planner.
fn shape_weight(inputs: u64, script_lens: &[u64]) -> u64 {
    // version, input count, output count, lock time
    let mut base = 4 + 1 + 1 + 4;
    base += 41 * inputs;
    for len in script_lens {
        base += 8 + 1 + len;
    }
    // marker and flag, and per input: item count, signature, key
    let witness = 2 + inputs * (1 + (1 + 72) + (1 + 33));
    base * 4 + witness
}

#[test]
fn sp_81_the_size_is_the_shape_of_a_signature_and_a_key() {
    let (p, _) = two_inputs();
    let tx = p.unsigned_tx();
    let lens: Vec<u64> = tx
        .output
        .iter()
        .map(|o| o.script_pubkey.len() as u64)
        .collect();
    // 22-byte payment and 22-byte change scripts.
    assert_eq!(lens, vec![22, 22]);
    assert_eq!(p.weight(), shape_weight(2, &lens));
    assert_eq!(p.vsize(), p.weight().div_ceil(4));
    // Checked against the `bitcoin` crate's own weight of a transaction
    // with a witness of that shape.
    let mut shaped = tx.clone();
    for input in &mut shaped.input {
        input.witness = Witness::from_slice(&[&[0u8; 72][..], &[2u8; 33][..]]);
    }
    assert_eq!(shaped.weight().to_wu(), p.weight());
    // One input, one output of a P2WPKH payment: 110 virtual bytes.
    let one = plan(
        single(),
        &[coin(3, 0, 1_000_000)],
        &SpendRequest {
            send_all: true,
            ..request(vec![(external(1), 0)], 1000)
        },
    )
    .unwrap();
    assert_eq!(one.weight(), shape_weight(1, &[22]));
    assert_eq!(one.vsize(), 110);
    // The fee is the asked rate over that size.
    assert_eq!(one.fee_sats(), 110);
}

#[test]
fn sp_81_the_real_rate_is_at_least_the_asked_one_with_one_signature_per_input() {
    let (p, base) = two_inputs();
    let tx = finalize_plan(single(), &signed(&base), &p).unwrap();
    let real_vsize = tx.weight().to_wu().div_ceil(4);
    let n = tx.input.len() as u64;
    assert!(real_vsize <= p.vsize());
    // SP-31 with one signature per input: at most two virtual bytes per
    // input and one satoshi of rounding.
    assert!(p.vsize() - real_vsize <= 2 * n + 1);
    assert!(p.fee_sats() * 1000 >= 1000 * real_vsize);
}

#[test]
fn sp_81_a_consolidation_is_sized_without_coins() {
    for n in [1usize, 2, 7, 50, 400] {
        let coins: Vec<Coin> = (0..n)
            .map(|t| coin(100 + t as u32, t as u32 % 5, 50_000))
            .collect();
        let change = single_inner().script(Keychain::Change, 3).unwrap();
        let p = plan(
            single(),
            &coins,
            &SpendRequest {
                change_index: 3,
                send_all: true,
                ..request(vec![(change, 0)], 2000)
            },
        )
        .unwrap();
        let (weight, vsize) = consolidation_size_single(n).unwrap();
        assert_eq!((weight, vsize), (p.weight(), p.vsize()), "{n} coins");
        assert_eq!(consolidation_cost_sats_single(n, 2000), Some(p.fee_sats()));
    }
    assert_eq!(consolidation_size_single(0), None);
    // A coin of a wallet with one key is cheaper to spend than one of three
    // keys.
    let (_, multi_vsize) = openbmt_spend::consolidation_size(1).unwrap();
    assert!(consolidation_size_single(1).unwrap().1 < multi_vsize);
}

// ---- SP-82 ---------------------------------------------------------------

fn hint(
    chain: u32,
    index: u32,
) -> BTreeMap<bitcoin::secp256k1::PublicKey, (Fingerprint, DerivationPath)> {
    let key = single_inner()
        .public_key(
            if chain == 0 {
                Keychain::Receive
            } else {
                Keychain::Change
            },
            index,
        )
        .unwrap();
    let path = DerivationPath::from_str(&format!("m/84'/0'/0'/{chain}/{index}")).unwrap();
    BTreeMap::from([(key.0, (kit().fingerprint, path))])
}

#[test]
fn sp_82_the_file_holds_what_the_rule_lists_and_nothing_else() {
    let p = plan(
        single(),
        &[
            coin(1, 2, 300_000),
            coin_at(2, Keychain::Change, 4, 400_000, 6),
        ],
        &SpendRequest {
            change_index: 9,
            change_position: ChangePosition::Seed([3; 32]),
            lock_height: 850_000,
            ..request(vec![(external(1), 500_000), (external(2), 20_000)], 1000)
        },
    )
    .unwrap();
    let file = p.to_psbt(single()).unwrap();
    assert_eq!(file.version, 0);
    assert_eq!(file.unsigned_tx, *p.unsigned_tx());
    // The account key as the one global extended public key.
    let xpub = *single_inner().xpub();
    assert_eq!(
        file.xpub,
        BTreeMap::from([(
            xpub,
            (
                kit().fingerprint,
                DerivationPath::from_str("m/84'/0'/0'").unwrap()
            )
        )])
    );
    assert!(file.proprietary.is_empty() && file.unknown.is_empty());
    // Each input: the whole previous transaction, the previous output, the
    // hash type and the one hint. No witness script, no signature.
    for (entry, planned) in file.inputs.iter().zip(p.inputs()) {
        let c = &planned.coin;
        let chain = if c.chain == Keychain::Receive { 0 } else { 1 };
        let expected = Input {
            non_witness_utxo: Some(c.prev_tx.clone()),
            witness_utxo: Some(c.prev_tx.output[c.vout as usize].clone()),
            sighash_type: Some(PsbtSighashType::from(EcdsaSighashType::All)),
            bip32_derivation: hint(chain, c.index),
            ..Input::default()
        };
        assert_eq!(*entry, expected);
        assert!(entry.witness_script.is_none() && entry.partial_sigs.is_empty());
    }
    // The change output, at the place the seed gave, carries its hint; the
    // payments carry nothing.
    let at = p.change_position().unwrap();
    for (position, entry) in file.outputs.iter().enumerate() {
        if position == at {
            let expected = Output {
                bip32_derivation: hint(1, 9),
                ..Output::default()
            };
            assert_eq!(*entry, expected);
        } else {
            assert_eq!(*entry, Output::default());
        }
    }
    // It survives its own serialisation, and Core-style readers see PSBT v0.
    let back = Psbt::deserialize(&file.serialize()).unwrap();
    assert_eq!(back, file);
}

#[test]
fn sp_82_the_one_output_of_a_consolidation_carries_its_hint() {
    let change = single_inner().script(Keychain::Change, 5).unwrap();
    let p = plan(
        single(),
        &[coin(1, 0, 100_000), coin(2, 1, 100_000)],
        &SpendRequest {
            change_index: 5,
            send_all: true,
            ..request(vec![(change, 0)], 1000)
        },
    )
    .unwrap();
    let file = p.to_psbt(single()).unwrap();
    assert_eq!(file.outputs.len(), 1);
    assert_eq!(
        file.outputs[0],
        Output {
            bip32_derivation: hint(1, 5),
            ..Output::default()
        }
    );
    // A send_all to someone else carries no hint on its output.
    let p = plan(
        single(),
        &[coin(1, 0, 100_000)],
        &SpendRequest {
            send_all: true,
            ..request(vec![(external(4), 0)], 1000)
        },
    )
    .unwrap();
    assert_eq!(p.to_psbt(single()).unwrap().outputs[0], Output::default());
}

#[test]
fn sp_82_the_file_is_checked_against_the_wallet_again() {
    let p = plan_single(&[coin(1, 0, 100_000)], vec![(external(1), 10_000)], 1000);
    // A wallet of three keys cannot write a plan of one key.
    assert!(matches!(
        p.to_psbt(multisig()),
        Err(PsbtError::Coin(CoinError::WrongKind {
            expected: WalletKind::Multisig,
            ..
        }))
    ));
}

// ---- SP-83 ---------------------------------------------------------------

#[test]
fn sp_83_what_comes_back_is_read_for_its_transaction_and_its_signatures() {
    let (p, base) = two_inputs();
    // A wallet of another make returns a file of its own: no previous
    // transactions, no hints.
    let back = returned(&base);
    let both = take_wallet_signature(single(), &base, &back).unwrap();
    for (i, input) in both.inputs.iter().enumerate() {
        assert!(input.partial_sigs.contains_key(&own_key(&base, i)));
        // The rest of the sent file is kept.
        assert_eq!(input.bip32_derivation, base.inputs[i].bip32_derivation);
    }
    let tx = finalize_plan(single(), &both, &p).unwrap();
    assert_eq!(tx.compute_txid(), p.txid());

    // Another unsigned transaction is refused.
    let (_, other_base) = {
        let p = plan_single(
            &[
                coin(1, 2, 300_000),
                coin_at(2, Keychain::Change, 4, 400_000, 6),
            ],
            vec![(external(1), 450_000)],
            1000,
        );
        let f = p.to_psbt(single()).unwrap();
        (p, f)
    };
    assert_eq!(
        take_wallet_signature(single(), &base, &returned(&other_base)).unwrap_err(),
        SignerFileError::DifferentTransaction
    );
    // No signature on an input: not signed.
    let mut partial = back.clone();
    partial.inputs[1].partial_sigs.clear();
    assert_eq!(
        take_wallet_signature(single(), &base, &partial).unwrap_err(),
        SignerFileError::NotSigned { input: 1 }
    );
    // A signature that does not verify goes through combine, and is an error.
    let mut bad = back.clone();
    let key = own_key(&base, 0);
    let mut sig = bad.inputs[0].partial_sigs[&key];
    sig.signature = back.inputs[1].partial_sigs[&own_key(&base, 1)].signature;
    bad.inputs[0].partial_sigs.insert(key, sig);
    assert!(matches!(
        take_wallet_signature(single(), &base, &bad).unwrap_err(),
        SignerFileError::Combine(CombineError::BadSignature { input: 0, .. })
    ));
    // A file whose shape is wrong is not read.
    let mut shapeless = back.clone();
    shapeless.inputs.pop();
    assert_eq!(
        take_wallet_signature(single(), &base, &shapeless).unwrap_err(),
        SignerFileError::NotVersionZero
    );
}

#[test]
fn sp_83_a_file_whose_every_input_is_final_is_the_finished_transaction() {
    let (p, base) = two_inputs();
    let tx = finalize_plan(single(), &signed(&base), &p).unwrap();
    // Every input final.
    let mut done = base.clone();
    for (input, txin) in done.inputs.iter_mut().zip(&tx.input) {
        input.final_script_witness = Some(txin.witness.clone());
    }
    let read = transaction_of_final_file(&done).unwrap();
    assert_eq!(read, tx);
    assert_eq!(
        check_finished(single(), &base, &read).unwrap(),
        p.fee_sats()
    );
    // Some final and some not: not read as finished.
    let mut some = done.clone();
    some.inputs[1].final_script_witness = None;
    assert!(transaction_of_final_file(&some).is_none());
    // A script signature: not read as finished.
    let mut script_sig = done.clone();
    script_sig.inputs[0].final_script_sig = Some(ScriptBuf::from_bytes(vec![0x51]));
    assert!(transaction_of_final_file(&script_sig).is_none());
}

// ---- SP-84 ---------------------------------------------------------------

#[test]
fn sp_84_a_signature_by_the_input_own_key_over_the_p2pkh_script_code_is_taken() {
    let (_, base) = two_inputs();
    let both = combine_wallet(single(), &base, &[signed(&base)]).unwrap();
    for (i, input) in both.inputs.iter().enumerate() {
        assert_eq!(input.partial_sigs.len(), 1);
        let key = own_key(&base, i);
        let sig = input.partial_sigs[&key];
        // The same signature, made again by the independent code, verifies
        // against the independent hash.
        let script_code = ScriptBuf::new_p2pkh(&key.pubkey_hash());
        let hash = bip143(&base.unsigned_tx, i, &script_code, amount_of(&base, i));
        assert!(secp()
            .verify_ecdsa(&Message::from_digest(hash), &sig.signature, &key.inner)
            .is_ok());
    }
    // The 2-of-3 script code (the witness script) is not the one for this
    // output: a signature over it is refused.
    let mut wrong = base.clone();
    let key = own_key(&base, 0);
    let (chain, index) = place(&base, 0);
    let child = kit()
        .account
        .derive_priv(
            secp(),
            &[
                ChildNumber::Normal { index: chain },
                ChildNumber::Normal { index },
            ],
        )
        .unwrap();
    let script = single_inner().script(Keychain::Receive, index).unwrap();
    let hash = bip143(&base.unsigned_tx, 0, &script, amount_of(&base, 0));
    wrong.inputs[0].partial_sigs.insert(
        key,
        ecdsa::Signature {
            signature: secp().sign_ecdsa(&Message::from_digest(hash), &child.private_key),
            sighash_type: EcdsaSighashType::All,
        },
    );
    assert!(matches!(
        combine_wallet(single(), &base, &[wrong]).unwrap_err(),
        CombineError::BadSignature { input: 0, .. }
    ));
}

#[test]
fn sp_84_a_signature_for_any_other_key_is_an_error_naming_the_key() {
    let (_, base) = two_inputs();
    let (foreign, sig) = sign_input(&base, 0, &kit().other);
    assert_ne!(foreign, own_key(&base, 0));
    let mut copy = signed(&base);
    copy.inputs[0].partial_sigs.insert(foreign, sig);
    let e = combine_wallet(single(), &base, &[copy]).unwrap_err();
    assert_eq!(
        e,
        CombineError::ForeignKey {
            source: openbmt_spend::Source::Copy(0),
            input: 0,
            key: foreign
        }
    );
    assert!(e.to_string().contains(&foreign.to_string()));
    // In the base too.
    let mut bad_base = base.clone();
    bad_base.inputs[1]
        .partial_sigs
        .insert(foreign, sign_input(&base, 1, &kit().other).1);
    assert!(matches!(
        combine_wallet(single(), &bad_base, &[]).unwrap_err(),
        CombineError::ForeignKey {
            source: openbmt_spend::Source::Base,
            input: 1,
            ..
        }
    ));
    // The key of another input of the same wallet is another key too.
    let (other_input_key, other_sig) = sign_input(&base, 1, &kit().account);
    let mut copy = base.clone();
    copy.inputs[0]
        .partial_sigs
        .insert(other_input_key, other_sig);
    assert!(matches!(
        combine_wallet(single(), &base, &[copy]).unwrap_err(),
        CombineError::ForeignKey { input: 0, .. }
    ));
}

#[test]
fn sp_84_the_hint_must_name_the_key_whose_hash_is_the_script_spent() {
    let (_, base) = two_inputs();
    // No hint.
    let mut none = base.clone();
    none.inputs[0].bip32_derivation.clear();
    assert_eq!(
        combine_wallet(single(), &none, &[]).unwrap_err(),
        CombineError::KeyNotSpent { input: 0 }
    );
    // A hint for another place: its key is not the one derived there.
    let mut moved = base.clone();
    for (_, p) in moved.inputs[0].bip32_derivation.values_mut() {
        *p = DerivationPath::from_str("m/84'/0'/0'/0/7").unwrap();
    }
    assert_eq!(
        combine_wallet(single(), &moved, &[]).unwrap_err(),
        CombineError::KeyNotSpent { input: 0 }
    );
    // A hint of another origin.
    let mut origin = base.clone();
    for (fp, _) in origin.inputs[0].bip32_derivation.values_mut() {
        *fp = Fingerprint::from([1, 2, 3, 4]);
    }
    assert_eq!(
        combine_wallet(single(), &origin, &[]).unwrap_err(),
        CombineError::KeyNotSpent { input: 0 }
    );
    // A right hint for the wrong coin: the key does not hash to the script
    // spent.
    let mut wrong_coin = base.clone();
    wrong_coin.inputs[0].bip32_derivation = base.inputs[1].bip32_derivation.clone();
    assert_eq!(
        combine_wallet(single(), &wrong_coin, &[]).unwrap_err(),
        CombineError::KeyNotSpent { input: 0 }
    );
    // Two hints.
    let mut two = base.clone();
    let extra = base.inputs[1].bip32_derivation.clone();
    two.inputs[0].bip32_derivation.extend(extra);
    assert_eq!(
        combine_wallet(single(), &two, &[]).unwrap_err(),
        CombineError::KeyNotSpent { input: 0 }
    );
}

#[test]
fn sp_84_one_hash_type_and_one_signature_per_key_and_the_order_does_not_matter() {
    let (_, base) = two_inputs();
    let a = signed(&base);
    // SP-52: another hash type in the signature, or in the input's field.
    let mut none_hash = a.clone();
    for sig in none_hash.inputs[0].partial_sigs.values_mut() {
        sig.sighash_type = EcdsaSighashType::None;
    }
    assert!(matches!(
        combine_wallet(single(), &base, &[none_hash]).unwrap_err(),
        CombineError::SighashType { input: 0, .. }
    ));
    let mut field = a.clone();
    field.inputs[1].sighash_type = Some(PsbtSighashType::from(EcdsaSighashType::Single));
    assert!(matches!(
        combine_wallet(single(), &base, &[field]).unwrap_err(),
        CombineError::SighashType { input: 1, .. }
    ));
    // A high-S signature is one that does not verify.
    let mut high = a.clone();
    let key = own_key(&base, 0);
    let flipped = high_s(&high.inputs[0].partial_sigs[&key]);
    high.inputs[0].partial_sigs.insert(key, flipped);
    assert!(matches!(
        combine_wallet(single(), &base, &[high]).unwrap_err(),
        CombineError::BadSignature { input: 0, .. }
    ));
    // A signature made for another input is one that does not verify, and
    // the error names the input.
    let mut swapped = a.clone();
    let (k1, s1) = sign_input(&base, 1, &kit().account);
    let _ = k1;
    swapped.inputs[0].partial_sigs.insert(key, s1);
    let e = combine_wallet(single(), &base, &[swapped]).unwrap_err();
    assert_eq!(
        e,
        CombineError::BadSignature {
            source: openbmt_spend::Source::Copy(0),
            input: 0,
            key
        }
    );
    // SP-54: the same key met twice is kept once, and the order of the
    // copies is not the result.
    let b = a.clone();
    let ab = combine_wallet(single(), &base, &[a.clone(), b.clone()]).unwrap();
    let ba = combine_wallet(single(), &base, &[b, a.clone()]).unwrap();
    assert_eq!(ab, ba);
    assert_eq!(ab.inputs[0].partial_sigs.len(), 1);
    // The base's own signature is checked like the others.
    let with_sig = combine_wallet(single(), &a, &[]).unwrap();
    assert_eq!(with_sig, ab);
    let mut bad_base = a.clone();
    bad_base.inputs[0].partial_sigs.insert(key, s1_of(&base));
    assert!(matches!(
        combine_wallet(single(), &bad_base, &[]).unwrap_err(),
        CombineError::BadSignature {
            source: openbmt_spend::Source::Base,
            input: 0,
            ..
        }
    ));
}

fn s1_of(base: &Psbt) -> ecdsa::Signature {
    sign_input(base, 1, &kit().account).1
}

#[test]
fn sp_84_the_copies_must_repeat_the_base_and_not_be_final() {
    let (_, base) = two_inputs();
    // Another transaction.
    let (_, other) = {
        let p = plan_single(&[coin(1, 2, 300_000)], vec![(external(1), 10_000)], 1000);
        let f = p.to_psbt(single()).unwrap();
        (p, f)
    };
    assert!(matches!(
        combine_wallet(single(), &base, &[signed(&other)]).unwrap_err(),
        CombineError::DifferentTransaction { copy: 0 }
    ));
    // Another previous output on an input.
    let mut changed = signed(&base);
    changed.inputs[0].witness_utxo = None;
    assert!(matches!(
        combine_wallet(single(), &base, &[changed]).unwrap_err(),
        CombineError::DifferentInputData { copy: 0, input: 0 }
    ));
    // A witness script on a copy of a file that has none.
    let mut with_script = signed(&base);
    with_script.inputs[1].witness_script = Some(ScriptBuf::from_bytes(vec![0x51]));
    assert!(matches!(
        combine_wallet(single(), &base, &[with_script]).unwrap_err(),
        CombineError::DifferentInputData { copy: 0, input: 1 }
    ));
    // A final witness.
    let mut fin = signed(&base);
    fin.inputs[0].final_script_witness = Some(Witness::new());
    assert!(matches!(
        combine_wallet(single(), &base, &[fin]).unwrap_err(),
        CombineError::InputAlreadyFinal { input: 0, .. }
    ));
    // A previous transaction counts as the same by its ID: Core writes it
    // without its witnesses.
    let mut stripped = signed(&base);
    for input in &mut stripped.inputs {
        if let Some(prev) = &mut input.non_witness_utxo {
            for txin in &mut prev.input {
                txin.witness = Witness::new();
            }
        }
    }
    assert!(combine_wallet(single(), &base, &[stripped]).is_ok());
    // No input or no output: refused, with nothing to check.
    let mut empty = base.clone();
    empty.unsigned_tx.input.clear();
    empty.inputs.clear();
    assert!(matches!(
        combine_wallet(single(), &empty, &[]).unwrap_err(),
        CombineError::NotVersionZero { .. }
    ));
}

// ---- SP-85 ---------------------------------------------------------------

#[test]
fn sp_85_the_witness_is_the_signature_then_the_key() {
    let (p, base) = two_inputs();
    let joined = combine_wallet(single(), &base, &[signed(&base)]).unwrap();
    let tx = finalize_plan(single(), &joined, &p).unwrap();
    assert_eq!(tx.compute_txid(), p.txid());
    for (i, txin) in tx.input.iter().enumerate() {
        let items: Vec<&[u8]> = txin.witness.iter().collect();
        assert_eq!(items.len(), 2);
        let key = own_key(&base, i);
        assert_eq!(items[1], key.inner.serialize());
        let sig = joined.inputs[i].partial_sigs[&key];
        assert_eq!(items[0], sig.to_vec().as_slice());
        assert!(txin.script_sig.is_empty());
    }
    assert_eq!(finalize(single(), &joined).unwrap(), tx);
    // The whole transaction at its weight, the same as 2-of-3's rule.
    assert!(tx.weight().to_wu() <= 400_000);
    // A transaction of this kind at the expected size.
    assert!(tx.weight().to_wu() <= p.weight());
}

#[test]
fn sp_85_one_valid_signature_by_the_inputs_own_key_is_needed() {
    let (_, base) = two_inputs();
    // None.
    assert_eq!(
        finalize(single(), &base).unwrap_err(),
        FinalizeError::NoSignature { input: 0 }
    );
    // One input signed, the other not.
    let mut half = signed(&base);
    half.inputs[1].partial_sigs.clear();
    assert_eq!(
        finalize(single(), &half).unwrap_err(),
        FinalizeError::NoSignature { input: 1 }
    );
    // A signature by another key.
    let mut foreign = base.clone();
    let (k, s) = sign_input(&base, 0, &kit().other);
    foreign.inputs[0].partial_sigs.insert(k, s);
    assert!(matches!(
        finalize(single(), &foreign).unwrap_err(),
        FinalizeError::ForeignKey { input: 0, .. }
    ));
    // The signature made for another input.
    let mut swapped = signed(&base);
    let key = own_key(&base, 0);
    swapped.inputs[0].partial_sigs.insert(key, s1_of(&base));
    assert!(matches!(
        finalize(single(), &swapped).unwrap_err(),
        FinalizeError::BadSignature { input: 0, .. }
    ));
    // Another hash type.
    let mut none_hash = signed(&base);
    for sig in none_hash.inputs[0].partial_sigs.values_mut() {
        sig.sighash_type = EcdsaSighashType::NonePlusAnyoneCanPay;
    }
    assert!(matches!(
        finalize(single(), &none_hash).unwrap_err(),
        FinalizeError::SighashType { input: 0 }
    ));
    // An input that is already final.
    let mut fin = signed(&base);
    fin.inputs[0].final_script_witness = Some(Witness::new());
    assert_eq!(
        finalize(single(), &fin).unwrap_err(),
        FinalizeError::InputAlreadyFinal { input: 0 }
    );
}

#[test]
fn sp_85_the_input_is_held_to_the_wallet() {
    let (p, base) = two_inputs();
    let a = signed(&base);
    // No hint.
    let mut bare = a.clone();
    bare.inputs[1].bip32_derivation.clear();
    assert_eq!(
        finalize(single(), &bare).unwrap_err(),
        FinalizeError::NoWalletKeyHint { input: 1 }
    );
    // A hint for another place.
    let mut moved = a.clone();
    for (_, path) in moved.inputs[0].bip32_derivation.values_mut() {
        *path = DerivationPath::from_str("m/84'/0'/0'/0/7").unwrap();
    }
    assert_eq!(
        finalize(single(), &moved).unwrap_err(),
        FinalizeError::HintsInconsistent { input: 0 }
    );
    // The hint of the other coin: the script spent is not that key's.
    let mut crossed = a.clone();
    crossed.inputs[0].bip32_derivation = base.inputs[1].bip32_derivation.clone();
    assert_eq!(
        finalize(single(), &crossed).unwrap_err(),
        FinalizeError::ScriptNotInWallet { input: 0 }
    );
    // The previous transaction is not the one the input spends.
    let mut swapped = a.clone();
    swapped.inputs[0].non_witness_utxo = a.inputs[1].non_witness_utxo.clone();
    assert_eq!(
        finalize(single(), &swapped).unwrap_err(),
        FinalizeError::PreviousTransactionMismatch { input: 0 }
    );
    // A file with a time for a lock time.
    let mut timed = a.clone();
    timed.unsigned_tx.lock_time = LockTime::from_consensus(500_000_000);
    assert!(matches!(
        finalize(single(), &timed).unwrap_err(),
        FinalizeError::LockTimeNotHeight { .. }
    ));
    // SP-64: the file must be the plan's.
    let (p2, _) = {
        let p = plan_single(&[coin(1, 2, 300_000)], vec![(external(1), 10_000)], 1000);
        (p, ())
    };
    assert_eq!(
        finalize_plan(single(), &a, &p2).unwrap_err(),
        FinalizeError::NotThePlan
    );
    assert!(finalize_plan(single(), &a, &p).is_ok());
}

// ---- SP-86 ---------------------------------------------------------------

#[test]
fn sp_86_a_finished_transaction_is_checked_against_the_file_that_was_sent() {
    let (p, base) = two_inputs();
    let tx = finalize_plan(single(), &signed(&base), &p).unwrap();
    assert_eq!(check_finished(single(), &base, &tx).unwrap(), p.fee_sats());
    // Another output.
    let mut other_out = tx.clone();
    other_out.output[0].value = Amount::from_sat(other_out.output[0].value.to_sat() - 1);
    assert!(matches!(
        check_finished(single(), &base, &other_out).unwrap_err(),
        SignerFileError::Finished(FinalizeError::FinalCheckFailed(_))
    ));
    // A script signature.
    let mut script_sig = tx.clone();
    script_sig.input[0].script_sig = ScriptBuf::from_bytes(vec![0x51]);
    assert!(check_finished(single(), &base, &script_sig).is_err());
    // A witness that is not two items.
    let mut three = tx.clone();
    three.input[0].witness.push([1u8]);
    assert!(matches!(
        check_finished(single(), &base, &three).unwrap_err(),
        SignerFileError::Finished(FinalizeError::FinalCheckFailed(_))
    ));
    // A time, not a height.
    let mut timed = tx.clone();
    timed.lock_time = LockTime::from_consensus(500_000_001);
    assert!(matches!(
        check_finished(single(), &base, &timed).unwrap_err(),
        SignerFileError::Finished(FinalizeError::LockTimeNotHeight { .. })
    ));
}

#[test]
fn sp_86_each_signature_must_be_the_input_own_keys() {
    let (p, base) = two_inputs();
    let tx = finalize_plan(single(), &signed(&base), &p).unwrap();
    // A valid signature by another key, with that key: the check says it
    // is not the input's own.
    let (foreign, sig) = sign_input(&base, 0, &kit().other);
    let mut other_key = tx.clone();
    other_key.input[0].witness =
        Witness::from_slice(&[sig.to_vec(), foreign.inner.serialize().to_vec()]);
    assert_eq!(
        check_finished(single(), &base, &other_key).unwrap_err(),
        SignerFileError::Finished(FinalizeError::FinalCheckFailed(
            "the witness key is not the input's own key"
        ))
    );
    // The right key with a signature of another key.
    let mut wrong_sig = tx.clone();
    wrong_sig.input[0].witness =
        Witness::from_slice(&[sig.to_vec(), own_key(&base, 0).inner.serialize().to_vec()]);
    assert_eq!(
        check_finished(single(), &base, &wrong_sig).unwrap_err(),
        SignerFileError::Finished(FinalizeError::FinalCheckFailed(
            "the signature is not the input's own key's"
        ))
    );
    // The signature made for input 1 on input 0.
    let mut swapped = tx.clone();
    swapped.input.swap(0, 1);
    // The unsigned transaction no longer matches (inputs swapped).
    assert!(check_finished(single(), &base, &swapped).is_err());
    let mut witness_swapped = tx.clone();
    let w1 = tx.input[1].witness.clone();
    witness_swapped.input[0].witness = w1;
    assert!(check_finished(single(), &base, &witness_swapped).is_err());
    // A signature that is not DER, and one of another hash type.
    let mut junk = tx.clone();
    junk.input[0].witness =
        Witness::from_slice(&[vec![1u8; 10], own_key(&base, 0).inner.serialize().to_vec()]);
    assert!(check_finished(single(), &base, &junk).is_err());
    let mut hash_type = tx.clone();
    let mut sig_bytes = tx.input[0].witness.nth(0).unwrap().to_vec();
    *sig_bytes.last_mut().unwrap() = 0x02;
    hash_type.input[0].witness =
        Witness::from_slice(&[sig_bytes, own_key(&base, 0).inner.serialize().to_vec()]);
    assert!(check_finished(single(), &base, &hash_type).is_err());
}

// ---- SP-87 ---------------------------------------------------------------

#[test]
fn sp_87_the_witness_is_what_core_finishes_and_the_script_that_proves_it_exists() {
    // Core finalises a P2WPKH input as [signature, key]; written here from
    // the partial signature without the library's finish.
    let (p, base) = two_inputs();
    let joined = combine_wallet(single(), &base, &[signed(&base)]).unwrap();
    let tx = finalize_plan(single(), &joined, &p).unwrap();
    let mut by_hand = p.unsigned_tx().clone();
    for (i, txin) in by_hand.input.iter_mut().enumerate() {
        let key = own_key(&base, i);
        let sig = joined.inputs[i].partial_sigs[&key];
        let mut w = Witness::new();
        w.push(sig.to_vec());
        w.push(key.inner.serialize());
        txin.witness = w;
    }
    assert_eq!(
        bitcoin::consensus::encode::serialize(&tx),
        bitcoin::consensus::encode::serialize(&by_hand)
    );
    // The test on a private test network and the fuzz target are in the
    // repository, and the target feeds the path of a wallet with one key.
    let script = include_str!("../../../scripts/spend-single-e2e-core.sh");
    assert!(script.contains("spend-single-e2e-core-ok"));
    assert!(script.contains("testmempoolaccept"));
    assert!(script.contains("walletprocesspsbt") && script.contains("finalizepsbt"));
    let fuzz = include_str!("../../../fuzz/fuzz_targets/spend_combine.rs");
    assert!(fuzz.contains("combine_wallet") && fuzz.contains("ImportedWallet::Single"));
}

// ---- SP-70, SP-71 --------------------------------------------------------

fn script_of_kind(kind: u64, n: u8) -> ScriptBuf {
    match kind % 4 {
        0 => external(n),
        1 => ScriptBuf::new_p2wsh(&WScriptHash::from_byte_array([n; 32])),
        2 => ScriptBuf::new_p2pkh(&bitcoin::PubkeyHash::from_byte_array([n; 20])),
        _ => ScriptBuf::new_p2sh(&bitcoin::ScriptHash::from_byte_array([n; 20])),
    }
}

/// One random request over random coins of the wallet with one key.
fn random_case(rng: &mut Rng) -> (Vec<Coin>, SpendRequest) {
    let n = rng.range(1, 14);
    let coins: Vec<Coin> = (0..n)
        .map(|t| {
            let chain = if rng.range(0, 3) == 0 {
                Keychain::Change
            } else {
                Keychain::Receive
            };
            coin_at(
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
    let request = SpendRequest {
        payments,
        fee_rate: rng.range(1_000, 30_000),
        change_index: rng.range(0, 15) as u32,
        min_confirmations: rng.range(0, 2) as u32,
        send_all,
        must_spend: Vec::new(),
        lock_height: rng.range(0, 1_500_000) as u32,
        change_position: if rng.range(0, 2) == 0 {
            ChangePosition::Last
        } else {
            let mut seed = [0u8; 32];
            seed[..8].copy_from_slice(&rng.next().to_le_bytes());
            ChangePosition::Seed(seed)
        },
    };
    (coins, request)
}

fn check_success(plan_: &Plan, rate: u64) {
    // SP-34
    assert_eq!(
        plan_.total_in_sats(),
        plan_.total_out_sats() + plan_.fee_sats()
    );
    let base = plan_.to_psbt(single()).unwrap();
    let joined = combine_wallet(single(), &base, &[signed(&base)]).unwrap();
    let tx = finalize_plan(single(), &joined, plan_).unwrap();
    // The returned-file path agrees.
    let both = take_wallet_signature(single(), &base, &returned(&base)).unwrap();
    assert_eq!(finalize(single(), &both).unwrap(), tx);
    assert_eq!(
        check_finished(single(), &base, &tx).unwrap(),
        plan_.fee_sats()
    );
    // SP-31 with one signature per input.
    let real_vsize = tx.weight().to_wu().div_ceil(4);
    let n = tx.input.len() as u64;
    let fee = plan_.fee_sats();
    assert!(
        fee * 1000 >= rate * real_vsize,
        "real rate below the asked rate"
    );
    assert!(real_vsize <= plan_.vsize());
    assert!(plan_.vsize() - real_vsize <= 2 * n + 1);
    assert!(fee >= (plan_.vsize() * rate).div_ceil(1000));
    assert!(tx.weight().to_wu() <= 400_000);
    assert!(fee <= 2_000_000);
}

#[test]
fn sp_70_random_requests_are_refused_or_paid_at_the_asked_rate_and_signed() {
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    let (mut done, mut refused) = (0, 0);
    for _ in 0..60 {
        let (coins, request) = random_case(&mut rng);
        match plan(single(), &coins, &request) {
            Ok(p) => {
                check_success(&p, request.fee_rate);
                done += 1;
            }
            Err(e) => {
                assert!(!e.to_string().is_empty());
                refused += 1;
            }
        }
    }
    assert!(done >= 20, "only {done} plans; {refused} refused");
    assert!(refused >= 1, "no refusal in 60 cases");
}

#[test]
fn sp_71_the_same_arguments_give_the_same_plan_and_file() {
    let mut rng = Rng(42);
    for _ in 0..15 {
        let (coins, request) = random_case(&mut rng);
        let a = plan(single(), &coins, &request);
        let b = plan(single(), &coins, &request);
        assert_eq!(a, b);
        if let Ok(p) = a {
            assert_eq!(
                p.to_psbt(single()).unwrap().serialize(),
                p.to_psbt(single()).unwrap().serialize()
            );
            // The same plan over the same coins in another order.
            let mut reversed = coins.clone();
            reversed.reverse();
            assert_eq!(plan(single(), &reversed, &request), Ok(p));
        }
    }
}

#[test]
fn sp_70_arbitrary_bytes_never_panic_and_never_make_a_transaction() {
    let (p, base) = {
        let p = plan_single(
            &[coin(1, 0, 300_000), coin(2, 3, 300_000)],
            vec![(external(1), 500_000)],
            2000,
        );
        let f = p.to_psbt(single()).unwrap();
        (p, f)
    };
    let valid = signed(&base).serialize();
    let mut rng = Rng(0xABCD_EF01_2345_6789);
    let mut parsed = 0;
    for round in 0..4000 {
        let bytes: Vec<u8> = match round % 4 {
            0 => (0..rng.range(0, 400)).map(|_| rng.next() as u8).collect(),
            1 => {
                let mut b = b"psbt\xff".to_vec();
                b.extend((0..rng.range(0, 300)).map(|_| rng.next() as u8));
                b
            }
            2 => {
                let mut b = valid.clone();
                for _ in 0..rng.range(1, 4) {
                    let at = rng.range(0, b.len() as u64 - 1) as usize;
                    b[at] = rng.next() as u8;
                }
                b
            }
            _ => valid[..rng.range(0, valid.len() as u64) as usize].to_vec(),
        };
        let Ok(copy) = Psbt::deserialize(&bytes) else {
            continue;
        };
        parsed += 1;
        // Against the plan's file: an error, or the one transaction there is.
        if let Ok(joined) = combine_wallet(single(), &base, std::slice::from_ref(&copy)) {
            if let Ok(tx) = finalize(single(), &joined) {
                assert_eq!(tx.compute_txid(), p.txid());
            }
        }
        if let Ok(both) = take_wallet_signature(single(), &base, &copy) {
            if let Ok(tx) = finalize(single(), &both) {
                assert_eq!(tx.compute_txid(), p.txid());
            }
        }
        // As the base, and in other places: errors, never panics.
        let _ = combine_wallet(single(), &copy, std::slice::from_ref(&base));
        let _ = combine_wallet(single(), &copy, &[]);
        let _ = combine_wallet(multisig(), &copy, &[]);
        let _ = finalize(single(), &copy);
        let _ = finalize(multisig(), &copy);
        let _ = take_wallet_signature(single(), &copy, &base);
        let _ = psbt_for_signer(single(), &copy);
        if let Some(tx) = transaction_of_final_file(&copy) {
            let _ = check_finished(single(), &base, &tx);
            let _ = check_finished(single(), &copy, &tx);
        }
    }
    assert!(parsed > 50, "only {parsed} files parsed");
}

#[test]
fn sp_70_hostile_requests_are_errors_not_panics() {
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
        let r = SpendRequest {
            payments,
            fee_rate: rate,
            change_index,
            min_confirmations: u32::MAX,
            send_all,
            must_spend: Vec::new(),
            lock_height: 0,
            change_position: ChangePosition::Last,
        };
        let e = plan(single(), &coins, &r).unwrap_err();
        assert!(!e.to_string().is_empty());
        let mut zero = r.clone();
        zero.min_confirmations = 0;
        if let Err(e) = plan(single(), &coins, &zero) {
            assert!(!e.to_string().is_empty());
        }
    }
    let r = plan(single(), &[], &request(vec![(external(1), 1000)], 1000));
    assert_eq!(r, Err(PlanError::NoEligibleCoins));
    // A dust change of the wallet with one key is 294 satoshis, not 330.
    let p = plan(
        single(),
        &[coin(2, 0, 10_000)],
        &request(vec![(external(1), 9_700)], 1000),
    );
    if let Ok(p) = p {
        assert!(p.change().is_none_or(|c| c.sats >= 294));
    }
}
