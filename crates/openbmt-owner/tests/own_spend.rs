//! Spends from a shop's own wallet (`docs/SESSION-RULES.md` section 6,
//! AS-100 to AS-107): the library with the wallet played by test keys, for a
//! wallet with one key and for a 2-of-3 wallet whose keys the owner holds.
//!
//! TEST ONLY, NEVER USE: the keys come from fixed bytes.

mod common;

use std::collections::BTreeSet;

use bitcoin::absolute::LockTime;
use bitcoin::bip32::{ChildNumber, Fingerprint, Xpriv, Xpub};
use bitcoin::consensus::serialize;
use bitcoin::hashes::Hash;
use bitcoin::psbt::Psbt;
use bitcoin::secp256k1::Message;
use bitcoin::sighash::{EcdsaSighashType, SighashCache};
use bitcoin::transaction::Version;
use bitcoin::{
    ecdsa, Amount, OutPoint, PublicKey, ScriptBuf, Sequence, Transaction, TxIn, TxOut, Txid,
    Witness,
};
use common::*;
use openbmt_core::wallet::Keychain;
use openbmt_core::wallet_single::{ImportedWallet, SingleKeyWallet};
use openbmt_owner::own_shop::own_approver;
use openbmt_owner::own_spend::{
    all_own_spend_words, all_refusals, compare_words, OwnBuilt, OwnFile, OwnSpendRefusal,
    TAG_OWN_SPEND,
};
use openbmt_owner::{
    CashAmount, Delivery, Options, Owner, OwnerError, RefundPacking, Refusal, Stage,
};
use openbmt_session::{Item, ItemKind, Request};
use openbmt_spend::{finalize, Coin};

const BIG: u64 = 1_000_000_000;

/// A wallet with the keys that sign for it.
struct Own {
    wallet: ImportedWallet,
    /// The keys the owner holds: (master fingerprint, account key).
    keys: Vec<(Fingerprint, Xpriv)>,
}

/// The wallet with one key, from fixed bytes.
fn single() -> Own {
    let master = Xpriv::new_master(bitcoin::Network::Bitcoin, &[0x77; 32]).unwrap();
    let path: bitcoin::bip32::DerivationPath = "m/84'/0'/0'".parse().unwrap();
    let account = master.derive_priv(secp(), &path).unwrap();
    let wallet = SingleKeyWallet::new(
        master.fingerprint(secp()),
        0,
        Xpub::from_priv(secp(), &account),
    )
    .unwrap();
    Own {
        wallet: ImportedWallet::Single(wallet),
        keys: vec![(master.fingerprint(secp()), account)],
    }
}

/// The 2-of-3 wallet of the test kit, with all three keys held.
fn multi() -> Own {
    let k = kit();
    let keys = (0..3)
        .map(|n| (k.wallet.slots()[n].origin_fingerprint, k.accounts[n]))
        .collect();
    Own {
        wallet: ImportedWallet::Multisig(k.wallet.clone()),
        keys,
    }
}

fn own_owner(own: &Own, options: Options) -> Owner {
    Owner::new(
        &OWNER_SECRET,
        own.wallet.clone(),
        own_approver(&OWNER_SECRET).unwrap(),
        policy(),
        options,
    )
    .unwrap()
}

fn restore(own: &Own, bytes: &[u8]) -> Result<Owner, openbmt_owner::RestoreError> {
    Owner::restore(
        &OWNER_SECRET,
        own.wallet.clone(),
        own_approver(&OWNER_SECRET).unwrap(),
        policy(),
        bytes,
    )
}

/// A coin of `sats` on the receive keychain at `index`, 6 deep.
fn own_coin(own: &Own, tag: u32, index: u32, sats: u64) -> Coin {
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
                script_pubkey: own.wallet.receive_script(index).unwrap(),
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
        chain: Keychain::Receive,
        index,
        confirmations: 6,
    }
}

/// What the wallet does: signs every input with the `keys` it uses.
fn wallet_signs(file: &[u8], keys: &[(Fingerprint, Xpriv)]) -> Psbt {
    let mut psbt = Psbt::deserialize(file).unwrap();
    let tx = psbt.unsigned_tx.clone();
    let mut cache = SighashCache::new(&tx);
    for (i, input) in psbt.inputs.iter_mut().enumerate() {
        let spent = match (&input.witness_utxo, &input.non_witness_utxo) {
            (Some(o), _) => o.clone(),
            (None, Some(prev)) => prev.output[tx.input[i].previous_output.vout as usize].clone(),
            _ => panic!("input {i} has no previous output"),
        };
        let derivations = input.bip32_derivation.clone();
        for (pk, (fp, path)) in derivations {
            for (mine, account) in keys {
                if *mine != fp {
                    continue;
                }
                let tail: Vec<ChildNumber> =
                    path.into_iter().rev().take(2).rev().cloned().collect();
                let sk = account.derive_priv(secp(), &tail).unwrap().private_key;
                assert_eq!(sk.public_key(secp()), pk);
                let digest = match &input.witness_script {
                    Some(ws) => cache
                        .p2wsh_signature_hash(i, ws, spent.value, EcdsaSighashType::All)
                        .unwrap()
                        .to_byte_array(),
                    None => {
                        let code =
                            ScriptBuf::new_p2wpkh(&PublicKey::new(pk).wpubkey_hash().unwrap());
                        cache
                            .p2wpkh_signature_hash(
                                i,
                                &spent.script_pubkey,
                                spent.value,
                                EcdsaSighashType::All,
                            )
                            .unwrap_or_else(|_| panic!("{code:?}"))
                            .to_byte_array()
                    }
                };
                let sig = secp().sign_ecdsa(&Message::from_digest(digest), &sk);
                input.partial_sigs.insert(
                    PublicKey::new(pk),
                    ecdsa::Signature {
                        signature: sig,
                        sighash_type: EcdsaSighashType::All,
                    },
                );
            }
        }
    }
    psbt
}

/// What a wallet that signs and finishes returns: the finished transaction.
fn finished_tx(own: &Own, file: &[u8]) -> Transaction {
    let signed = wallet_signs(file, &own.keys);
    finalize(&own.wallet, &signed).unwrap()
}

/// A PSBT in which every input is final, as a wallet that finishes returns.
fn final_file(own: &Own, file: &[u8]) -> Vec<u8> {
    let tx = finished_tx(own, file);
    let mut psbt = Psbt::deserialize(file).unwrap();
    for (input, txin) in psbt.inputs.iter_mut().zip(&tx.input) {
        input.partial_sigs.clear();
        input.final_script_witness = Some(txin.witness.clone());
    }
    psbt.serialize()
}

struct Fixture {
    own: Own,
    owner: Owner,
    store: Store,
    session: [u8; 32],
    built: OwnBuilt,
    refund: u32,
    cash_out: u32,
}

/// A refund of 40,000 and a cash-out in one session, two coins.
fn fixture(own: Own) -> Fixture {
    let mut owner = own_owner(&own, Options::default());
    let refund_id = refund(&mut owner, 1, BIG, 40_000);
    let cash = owner
        .queue_cash_out(external(9), CashAmount::Sats(100_000), "Savings", nop)
        .unwrap();
    let coins = [own_coin(&own, 1, 0, 500_000), own_coin(&own, 2, 1, 500_000)];
    let store = Store::default();
    let built = ok(owner.build_own_spend(
        &coins,
        &[refund_id, cash],
        1_000,
        0,
        850_000,
        NOW,
        &[7; 32],
        |b| store.put(b),
    ));
    let session = built.session.expect("something was frozen");
    Fixture {
        own,
        owner,
        store,
        session,
        built,
        refund: refund_id,
        cash_out: cash,
    }
}

fn file(f: &mut Fixture, number: u32) -> OwnFile {
    let store = &f.store;
    ok(f.owner.own_file(&f.session, number, |b| store.put(b)))
}

fn take(
    f: &mut Fixture,
    number: u32,
    returned: &[u8],
) -> Result<openbmt_owner::own_spend::OwnTaken, Refusal> {
    let store = &f.store;
    match f
        .owner
        .take_own(&f.session, number, returned, |b| store.put(b))
    {
        Ok(t) => Ok(t),
        Err(OwnerError::Refused(r)) => Err(r),
        Err(e) => panic!("{e}"),
    }
}

fn own_refusal(r: Refusal) -> OwnSpendRefusal {
    match r {
        Refusal::OwnSpend(x) => x,
        other => panic!("not an own-spend refusal: {other}"),
    }
}

type Maker = fn() -> Own;

fn both_kinds() -> [(&'static str, Maker); 2] {
    [("one key", single), ("2-of-3", multi)]
}

// ---- AS-100 ---------------------------------------------------------------------------

#[test]
fn as_100_the_queue_holds_refunds_and_cash_outs_and_the_rules_that_decide_them() {
    for (name, kind) in both_kinds() {
        let own = kind();
        let mut owner = own_owner(&own, Options::default());
        assert!(owner.is_own_wallet(), "{name}");
        // AS-52: never more than was received.
        refund(&mut owner, 1, 50_000, 40_000);
        let more = owner.queue_refund(sale(1), "Mia", external(2), 20_000, None, nop);
        assert!(
            matches!(refused(more), Refusal::MoreThanReceived { .. }),
            "{name}"
        );
        // AS-51: dust.
        let dust = owner.queue_cash_out(external(3), CashAmount::Sats(10), "x", nop);
        assert!(matches!(refused(dust), Refusal::BelowDust { .. }), "{name}");
        owner
            .queue_cash_out(external(3), CashAmount::All, "Savings", nop)
            .unwrap();
        assert_eq!(owner.cash_outs().len(), 1);
    }
}

#[test]
fn as_100_an_item_that_needs_the_key_app_is_refused_with_no_key_app() {
    for (name, kind) in both_kinds() {
        let own = kind();
        let mut owner = own_owner(&own, Options::default());
        let r = owner.queue_batch(batch(1, 0, 3), NOW, nop);
        assert_eq!(refused(r), Refusal::NoKeyApp, "{name}");
        assert!(Refusal::NoKeyApp.to_string().starts_with("no-key-app"));
        let r = owner.queue_settings(settings_with(2, &[1]), nop);
        assert_eq!(refused(r), Refusal::NoKeyApp, "{name}");
        let r = owner.queue_pairing(&[0; 10], nop);
        assert_eq!(refused(r), Refusal::NoKeyApp, "{name}");
        let r = owner.queue_box_claim(&[0; 10], nop);
        assert_eq!(refused(r), Refusal::NoKeyApp, "{name}");
        // Nothing is built for a Key app either.
        let random = [3; 96];
        let r = owner.build_request(&[], 1_000, 0, 850_000, NOW, &random, nop);
        assert_eq!(refused(r), Refusal::NoKeyApp, "{name}");
        let r = owner.build_request_for_signer(&[], 1_000, 0, 850_000, NOW, &random, nop);
        assert_eq!(refused(r), Refusal::NoKeyApp, "{name}");
        // The queue is as it was.
        assert!(owner.batches().is_empty() && owner.queued_settings().is_empty());
    }
}

#[test]
fn as_100_a_shop_with_a_key_app_is_not_an_own_wallet_shop() {
    let mut owner = new_owner(Options::default());
    assert!(!owner.is_own_wallet());
    owner.queue_batch(batch(1, 0, 3), NOW, nop).unwrap();
    let r = owner.build_own_spend(&[], &[1], 1_000, 0, 850_000, NOW, &[1; 32], nop);
    assert_eq!(refused(r), Refusal::OwnSpend(OwnSpendRefusal::NotOwnWallet));
}

#[test]
fn as_100_a_dropped_transaction_asked_for_again_spends_one_of_the_same_coins() {
    for (name, kind) in both_kinds() {
        let mut f = fixture(kind());
        file(&mut f, 0);
        let first = f.built.items[0].txid;
        let coins = [
            own_coin(&f.own, 1, 0, 500_000),
            own_coin(&f.own, 2, 1, 500_000),
        ];
        let own_file = file(&mut f, 0);
        let returned = serialize(&finished_tx(&f.own, &own_file.psbt));
        take(&mut f, 0, &returned).unwrap();
        // The caller reports it dropped: its entries keep the outpoints.
        f.owner.mark_dropped(&first, nop).unwrap();
        let conflict = f
            .owner
            .refunds()
            .iter()
            .find(|r| r.id == f.refund)
            .unwrap()
            .conflict
            .clone();
        assert!(!conflict.is_empty(), "{name}");
        f.owner.requeue(f.refund, nop).unwrap();
        let again =
            ok(f.owner
                .build_own_spend(&coins, &[f.refund], 1_000, 2, 850_000, NOW, &[8; 32], nop));
        let id = again.session.expect("planned again");
        let tx = Psbt::deserialize(&ok_file(&mut f.owner, &id))
            .unwrap()
            .unsigned_tx;
        let spent: BTreeSet<OutPoint> = tx.input.iter().map(|i| i.previous_output).collect();
        assert!(
            conflict.iter().any(|o| spent.contains(o)),
            "{name}: the new transaction conflicts with the old one"
        );
    }
}

fn ok_file(owner: &mut Owner, session: &[u8; 32]) -> Vec<u8> {
    ok(owner.own_file(session, 0, nop)).psbt
}

#[test]
fn as_100_a_cash_out_is_its_own_transaction_and_the_outputs_are_frozen() {
    for (name, kind) in both_kinds() {
        let f = fixture(kind());
        // One item for the refund, one for the cash-out (AS-57).
        assert_eq!(f.built.items.len(), 2, "{name}");
        let kinds: Vec<(Vec<u32>, Option<u32>)> = f
            .built
            .items
            .iter()
            .map(|i| (i.refunds.clone(), i.cash_out))
            .collect();
        assert!(kinds.contains(&(vec![f.refund], None)), "{name}");
        assert!(kinds.contains(&(vec![], Some(f.cash_out))), "{name}");
    }
}

// ---- AS-101 ---------------------------------------------------------------------------

#[test]
fn as_101_the_picked_spends_are_frozen_into_a_session_of_spends_alone() {
    for (name, kind) in both_kinds() {
        let f = fixture(kind());
        let request = Request::from_bytes(f.owner.session_request(&f.session).unwrap()).unwrap();
        // One item for each transaction, no health check, no other item.
        assert!(request
            .items()
            .iter()
            .all(|i| matches!(i.item, Item::Spend { .. }) && i.item.kind() == ItemKind::Spend));
        assert_eq!(request.items().len(), 2, "{name}");
        assert_eq!(f.owner.own_sessions(), vec![f.session]);
        // The entries wait in session with that ID, and nothing is signed.
        for entry in f
            .owner
            .refunds()
            .iter()
            .map(|r| &r.stage)
            .chain(f.owner.cash_outs().iter().map(|c| &c.stage))
        {
            assert_eq!(*entry, Stage::InSession { session: f.session }, "{name}");
        }
        assert!(f.owner.outbox().is_empty());
        let items = f.owner.own_items(&f.session).unwrap();
        assert!(items.iter().all(|i| !i.filed));
        // No request for a Key app exists in this shop.
        assert_eq!(f.owner.open_sessions().len(), 1);
    }
}

#[test]
fn as_101_only_the_picked_entries_are_frozen() {
    let own = single();
    let mut owner = own_owner(&own, Options::default());
    let a = refund(&mut owner, 1, BIG, 40_000);
    let b = refund(&mut owner, 2, BIG, 50_000);
    let coins = [own_coin(&own, 1, 0, 500_000)];
    let built = ok(owner.build_own_spend(&coins, &[b], 1_000, 0, 850_000, NOW, &[1; 32], nop));
    assert_eq!(built.items.len(), 1);
    assert_eq!(built.items[0].refunds, vec![b]);
    let stage_of = |id| {
        owner
            .refunds()
            .iter()
            .find(|r| r.id == id)
            .unwrap()
            .stage
            .clone()
    };
    assert_eq!(stage_of(a), Stage::Queued);
    assert!(matches!(stage_of(b), Stage::InSession { .. }));
}

#[test]
fn as_101_the_id_is_a_tagged_hash_of_the_frozen_items_and_the_random_bytes() {
    assert_eq!(TAG_OWN_SPEND, "OpenBMT/own-spend/v1");
    let a = fixture(single());
    let b = fixture(single());
    // The same items and the same random bytes give the same ID.
    assert_eq!(a.session, b.session);
    // Other random bytes give another ID, and other items do too.
    let own = single();
    let mut owner = own_owner(&own, Options::default());
    let r = refund(&mut owner, 1, BIG, 40_000);
    let c = owner
        .queue_cash_out(external(9), CashAmount::Sats(100_000), "Savings", nop)
        .unwrap();
    let coins = [own_coin(&own, 1, 0, 500_000), own_coin(&own, 2, 1, 500_000)];
    let built = ok(owner.build_own_spend(&coins, &[r, c], 1_000, 0, 850_000, NOW, &[9; 32], nop));
    assert_ne!(built.session, Some(a.session));
    // The ID is the hash the rule names: not a copy of the random bytes.
    assert_ne!(built.session.unwrap(), [9; 32]);
}

#[test]
fn as_101_a_refund_is_frozen_only_when_the_owner_picked_it() {
    let own = single();
    let mut owner = own_owner(&own, Options::default());
    let a = refund(&mut owner, 1, BIG, 40_000);
    let coins = [own_coin(&own, 1, 0, 500_000)];
    // Nothing picked is refused, and an entry that is not queued too.
    let none = owner.build_own_spend(&coins, &[], 1_000, 0, 850_000, NOW, &[1; 32], nop);
    assert_eq!(
        refused(none),
        Refusal::OwnSpend(OwnSpendRefusal::NothingPicked)
    );
    let unknown = owner.build_own_spend(&coins, &[99], 1_000, 0, 850_000, NOW, &[1; 32], nop);
    assert_eq!(
        refused(unknown),
        Refusal::OwnSpend(OwnSpendRefusal::NotQueued(99))
    );
    assert_eq!(owner.refunds()[0].stage, Stage::Queued);
    ok(owner.build_own_spend(&coins, &[a], 1_000, 0, 850_000, NOW, &[1; 32], nop));
    // Frozen already: it cannot be picked again.
    let again = owner.build_own_spend(&coins, &[a], 1_000, 1, 850_000, NOW, &[2; 32], nop);
    assert!(matches!(
        refused(again),
        Refusal::OwnSpend(OwnSpendRefusal::NotQueued(_))
    ));
}

#[test]
fn as_101_a_refund_that_is_not_ready_stays_queued_with_the_reason() {
    let own = single();
    let mut owner = own_owner(&own, Options::default());
    set_facts(&mut owner, sale(1), shallow(BIG));
    let id = owner
        .queue_refund(sale(1), "Mia", external(1), 40_000, None, nop)
        .unwrap();
    let coins = [own_coin(&own, 1, 0, 500_000)];
    let built = ok(owner.build_own_spend(&coins, &[id], 1_000, 0, 850_000, NOW, &[1; 32], nop));
    assert_eq!(built.session, None);
    assert_eq!(built.skipped.len(), 1);
    assert_eq!(built.skipped[0].id, id);
    assert_eq!(owner.refunds()[0].stage, Stage::Queued);
    assert!(owner.open_sessions().is_empty());
}

#[test]
fn as_101_a_failed_store_changes_nothing() {
    let own = single();
    let mut owner = own_owner(&own, Options::default());
    let a = refund(&mut owner, 1, BIG, 40_000);
    let coins = [own_coin(&own, 1, 0, 500_000)];
    let r = owner.build_own_spend(&coins, &[a], 1_000, 0, 850_000, NOW, &[1; 32], |_| {
        Err("disk full".to_string())
    });
    assert!(matches!(r, Err(OwnerError::Store(_))));
    assert_eq!(owner.refunds()[0].stage, Stage::Queued);
    assert!(owner.open_sessions().is_empty());
}

#[test]
fn as_101_one_transaction_packing_puts_the_picked_refunds_in_one_item() {
    let own = single();
    let options = Options {
        packing: RefundPacking::OneTransaction,
        ..Options::default()
    };
    let mut owner = own_owner(&own, options);
    let a = refund(&mut owner, 1, BIG, 40_000);
    let b = refund(&mut owner, 2, BIG, 50_000);
    let coins = [own_coin(&own, 1, 0, 500_000)];
    let built = ok(owner.build_own_spend(&coins, &[a, b], 1_000, 0, 850_000, NOW, &[1; 32], nop));
    assert_eq!(built.items.len(), 1);
    assert_eq!(built.items[0].refunds, vec![a, b]);
    assert_eq!(built.items[0].summary.payments.len(), 2);
}

// ---- AS-102 ---------------------------------------------------------------------------

#[test]
fn as_102_a_wallet_with_one_key_gets_the_file_of_sp_82() {
    let mut f = fixture(single());
    let file = file(&mut f, 0);
    let psbt = Psbt::deserialize(&file.psbt).unwrap();
    assert_eq!(psbt.version, 0);
    assert_eq!(psbt.unsigned_tx.compute_txid(), file.txid);
    // The account's extended public key, once, and nothing of the 2-of-3
    // file: no witness script on any input.
    assert_eq!(psbt.xpub.len(), 1);
    for (i, input) in psbt.inputs.iter().enumerate() {
        assert!(input.non_witness_utxo.is_some(), "input {i}");
        assert_eq!(input.bip32_derivation.len(), 1, "input {i}");
        assert!(input.witness_script.is_none(), "input {i}");
        assert!(input.partial_sigs.is_empty(), "input {i}");
        assert_eq!(
            input.sighash_type.map(|t| t.ecdsa_hash_ty().unwrap()),
            Some(EcdsaSighashType::All)
        );
    }
    assert_eq!(
        file.file_name,
        format!("{}.psbt", &file.txid.to_string()[..8])
    );
}

#[test]
fn as_102_a_2_of_3_wallet_gets_the_file_of_sp_45_with_no_signature() {
    let mut f = fixture(multi());
    let file = file(&mut f, 0);
    let psbt = Psbt::deserialize(&file.psbt).unwrap();
    assert_eq!(psbt.xpub.len(), 3);
    for (i, input) in psbt.inputs.iter().enumerate() {
        assert!(input.witness_script.is_some(), "input {i}");
        assert_eq!(input.bip32_derivation.len(), 3, "input {i}");
        assert!(input.partial_sigs.is_empty(), "no key signed: input {i}");
        assert_eq!(
            input.sighash_type.map(|t| t.ecdsa_hash_ty().unwrap()),
            Some(EcdsaSighashType::All)
        );
    }
}

#[test]
fn as_102_the_same_item_gives_the_same_bytes_every_time_and_stores_once() {
    for (name, kind) in both_kinds() {
        let mut f = fixture(kind());
        let calls = f.store.calls();
        let first = file(&mut f, 0);
        assert_eq!(f.store.calls(), calls + 1, "{name}: the first call stores");
        let again = file(&mut f, 0);
        assert_eq!(again, first, "{name}");
        assert_eq!(
            f.store.calls(),
            calls + 1,
            "{name}: the second stores nothing"
        );
        // The same bytes after a restart of the phone.
        let mut back = restore(&f.own, &f.store.last()).unwrap();
        assert_eq!(ok(back.own_file(&f.session, 0, nop)), first, "{name}");
    }
}

#[test]
fn as_102_the_file_is_described_for_the_person_and_the_words_say_to_compare() {
    for (name, kind) in both_kinds() {
        let mut f = fixture(kind());
        for item in f.built.items.clone() {
            let file = file(&mut f, item.number);
            assert_eq!(file.summary, item.summary, "{name}");
            let paid: u64 = file.summary.payments.iter().map(|p| p.sats).sum();
            assert!(paid == 40_000 || paid == 100_000, "{name}: {paid}");
            let psbt = Psbt::deserialize(&file.psbt).unwrap();
            let outputs: u64 = psbt
                .unsigned_tx
                .output
                .iter()
                .map(|o| o.value.to_sat())
                .sum();
            let change = file.summary.change.as_ref().map_or(0, |c| c.sats);
            assert_eq!(paid + change, outputs, "{name}");
            assert!(file.summary.fee > 0);
        }
        let words = compare_words();
        assert!(words.what.contains("each address, each amount and the fee"));
        assert_eq!(words.code, Some("AS-102"));
    }
}

#[test]
fn as_102_a_failed_store_does_not_file_the_item() {
    let mut f = fixture(single());
    let r = f
        .owner
        .own_file(&f.session, 0, |_| Err("disk full".to_string()));
    assert!(matches!(r, Err(OwnerError::Store(_))));
    assert!(!f.owner.own_items(&f.session).unwrap()[0].filed);
    assert!(f.owner.exported_transactions(None).is_empty());
}

#[test]
fn as_102_a_session_or_item_that_is_not_an_own_spend_is_refused() {
    let mut f = fixture(single());
    let store = &f.store;
    let r = f.owner.own_file(&[0; 32], 0, |b| store.put(b));
    assert_eq!(
        refused(r),
        Refusal::OwnSpend(OwnSpendRefusal::NotAnOwnSpend)
    );
    let r = f.owner.own_file(&f.session, 9, |b| store.put(b));
    assert_eq!(
        refused(r),
        Refusal::OwnSpend(OwnSpendRefusal::NotAnOwnSpend)
    );
}

// ---- AS-103, AS-104 ---------------------------------------------------------------------

#[test]
fn as_103_a_wallet_with_one_key_signs_and_the_file_is_taken() {
    let mut f = fixture(single());
    let file = file(&mut f, 0);
    let signed = wallet_signs(&file.psbt, &f.own.keys);
    let taken = take(&mut f, 0, &signed.serialize()).unwrap();
    assert_eq!(taken.txid, file.txid);
    assert!(taken.fee > 0);
}

#[test]
fn as_103_a_2_of_3_wallet_needs_two_different_keys_on_every_input() {
    let mut f = fixture(multi());
    let file = file(&mut f, 0);
    // One key is not enough.
    let one = wallet_signs(&file.psbt, &f.own.keys[..1]);
    let r = own_refusal(take(&mut f, 0, &one.serialize()).unwrap_err());
    assert!(matches!(r, OwnSpendRefusal::NotSigned(_)), "{r}");
    // Two keys are.
    let two = wallet_signs(&file.psbt, &f.own.keys[1..]);
    let taken = take(&mut f, 0, &two.serialize()).unwrap();
    assert_eq!(taken.txid, file.txid);
    // All three are too (the third is not used).
    let mut g = fixture(multi());
    let file = self::file(&mut g, 0);
    let three = wallet_signs(&file.psbt, &g.own.keys);
    take(&mut g, 0, &three.serialize()).unwrap();
}

#[test]
fn as_103_a_file_signed_by_the_wrong_key_is_refused_as_the_wallet_did_not_sign() {
    for (name, kind) in both_kinds() {
        let mut f = fixture(kind());
        let file = file(&mut f, 0);
        let stranger = Xpriv::new_master(bitcoin::Network::Bitcoin, &[0x55; 32]).unwrap();
        // The stranger signs under the fingerprint of the wallet's key, so
        // the signature is made by a key that is not the input's.
        let mut psbt = Psbt::deserialize(&file.psbt).unwrap();
        let fp = f.own.keys[0].0;
        let tx = psbt.unsigned_tx.clone();
        let mut cache = SighashCache::new(&tx);
        for (i, input) in psbt.inputs.iter_mut().enumerate() {
            let prev = input.non_witness_utxo.clone().unwrap();
            let spent = prev.output[tx.input[i].previous_output.vout as usize].clone();
            let sk = stranger.private_key;
            let pk = sk.public_key(secp());
            let digest = match &input.witness_script {
                Some(ws) => cache
                    .p2wsh_signature_hash(i, ws, spent.value, EcdsaSighashType::All)
                    .unwrap()
                    .to_byte_array(),
                None => cache
                    .p2wpkh_signature_hash(
                        i,
                        &spent.script_pubkey,
                        spent.value,
                        EcdsaSighashType::All,
                    )
                    .unwrap()
                    .to_byte_array(),
            };
            let _ = fp;
            input.partial_sigs.insert(
                PublicKey::new(pk),
                ecdsa::Signature {
                    signature: secp().sign_ecdsa(&Message::from_digest(digest), &sk),
                    sighash_type: EcdsaSighashType::All,
                },
            );
        }
        let before = f.owner.snapshot().unwrap();
        let r = own_refusal(take(&mut f, 0, &psbt.serialize()).unwrap_err());
        assert!(matches!(r, OwnSpendRefusal::NotSigned(_)), "{name}: {r}");
        // A refusal changes nothing, so the right file can be brought again.
        assert_eq!(f.owner.snapshot().unwrap(), before, "{name}");
        let right = wallet_signs(&file.psbt, &f.own.keys);
        take(&mut f, 0, &right.serialize()).unwrap();
    }
}

#[test]
fn as_103_a_signature_that_does_not_verify_is_refused() {
    for (name, kind) in both_kinds() {
        let mut f = fixture(kind());
        let file = file(&mut f, 0);
        // The signatures of the other item's transaction.
        let other = self::file(&mut f, 1);
        let wrong = wallet_signs(&other.psbt, &f.own.keys);
        let mut psbt = Psbt::deserialize(&file.psbt).unwrap();
        for (into, from) in psbt.inputs.iter_mut().zip(&wrong.inputs) {
            into.partial_sigs = from.partial_sigs.clone();
        }
        let r = own_refusal(take(&mut f, 0, &psbt.serialize()).unwrap_err());
        assert!(matches!(r, OwnSpendRefusal::NotSigned(_)), "{name}: {r}");
    }
}

#[test]
fn as_103_a_different_payment_is_refused_and_nothing_changes() {
    for (name, kind) in both_kinds() {
        let mut f = fixture(kind());
        let file = file(&mut f, 0);
        let other = self::file(&mut f, 1);
        let signed_other = wallet_signs(&other.psbt, &f.own.keys);
        let before = f.owner.snapshot().unwrap();
        let r = own_refusal(take(&mut f, 0, &signed_other.serialize()).unwrap_err());
        assert_eq!(r, OwnSpendRefusal::DifferentTransaction, "{name}");
        let tx = finished_tx(&f.own, &other.psbt);
        let r = own_refusal(take(&mut f, 0, &serialize(&tx)).unwrap_err());
        assert_eq!(r, OwnSpendRefusal::DifferentTransaction, "{name}");
        assert_eq!(f.owner.snapshot().unwrap(), before, "{name}");
        let _ = file;
    }
}

#[test]
fn as_103_what_cannot_be_read_is_refused() {
    let mut f = fixture(single());
    file(&mut f, 0);
    for bad in [&b""[..], b"not a psbt", &[0u8; 64]] {
        let r = own_refusal(take(&mut f, 0, bad).unwrap_err());
        assert!(matches!(r, OwnSpendRefusal::Unreadable(_)), "{r}");
    }
    let big = vec![0u8; (4 << 20) + 1];
    assert!(matches!(
        own_refusal(take(&mut f, 0, &big).unwrap_err()),
        OwnSpendRefusal::Unreadable(_)
    ));
}

#[test]
fn as_103_an_unsigned_file_is_refused_as_the_wallet_did_not_sign() {
    for (name, kind) in both_kinds() {
        let mut f = fixture(kind());
        let file = file(&mut f, 0);
        let r = own_refusal(take(&mut f, 0, &file.psbt).unwrap_err());
        assert!(matches!(r, OwnSpendRefusal::NotSigned(_)), "{name}: {r}");
    }
}

#[test]
fn as_103_a_file_the_wallet_finished_is_taken_as_the_transaction() {
    for (name, kind) in both_kinds() {
        // As a final file, and as a transaction in binary and in hex.
        for form in 0..3 {
            let mut f = fixture(kind());
            let file = file(&mut f, 0);
            let returned: Vec<u8> = match form {
                0 => final_file(&f.own, &file.psbt),
                1 => serialize(&finished_tx(&f.own, &file.psbt)),
                _ => bitcoin::consensus::encode::serialize_hex(&finished_tx(&f.own, &file.psbt))
                    .into_bytes(),
            };
            let taken = take(&mut f, 0, &returned).unwrap_or_else(|e| panic!("{name} {form}: {e}"));
            assert_eq!(taken.txid, file.txid, "{name} {form}");
        }
    }
}

#[test]
fn as_103_a_finished_transaction_that_fails_the_check_is_refused() {
    for (name, kind) in both_kinds() {
        let mut f = fixture(kind());
        let file = file(&mut f, 0);
        let mut tx = finished_tx(&f.own, &file.psbt);
        // A witness that is not the input's: the first input carries the
        // second's (or, with one input, a damaged signature).
        let mut bad = tx.input[0].witness.to_vec();
        if let Some(sig) = bad.iter_mut().find(|item| item.len() > 20) {
            sig[10] ^= 1;
        }
        tx.input[0].witness = Witness::from_slice(&bad);
        let r = own_refusal(take(&mut f, 0, &serialize(&tx)).unwrap_err());
        assert!(matches!(r, OwnSpendRefusal::Finished(_)), "{name}: {r}");
    }
}

#[test]
fn as_104_the_take_requires_the_file_to_have_been_given() {
    let mut f = fixture(single());
    let file = file(&mut f, 0);
    // Take item 1, which was never filed, with item 0's signatures.
    let signed = wallet_signs(&file.psbt, &f.own.keys);
    let r = own_refusal(take(&mut f, 1, &signed.serialize()).unwrap_err());
    assert_eq!(r, OwnSpendRefusal::NotFiled);
}

#[test]
fn as_104_taking_moves_the_entries_to_signed_and_the_item_leaves_the_session() {
    for (name, kind) in both_kinds() {
        let mut f = fixture(kind());
        let first = file(&mut f, 0);
        let signed = wallet_signs(&first.psbt, &f.own.keys);
        let taken = take(&mut f, 0, &signed.serialize()).unwrap();
        let item = f.built.items[0].clone();
        // The entries of the item are signed with its transaction.
        for id in item.refunds.iter().copied().chain(item.cash_out) {
            let stage = f
                .owner
                .refunds()
                .iter()
                .find(|r| r.id == id)
                .map(|r| r.stage.clone())
                .or_else(|| {
                    f.owner
                        .cash_outs()
                        .iter()
                        .find(|c| c.id == id)
                        .map(|c| c.stage.clone())
                })
                .unwrap();
            assert_eq!(stage, Stage::Signed { txid: taken.txid }, "{name}");
        }
        // The transaction waits in the outbox, finished.
        assert_eq!(f.owner.outbox().len(), 1);
        match &f.owner.outbox()[0].what {
            Delivery::Transaction { txid, tx, .. } => {
                assert_eq!(*txid, taken.txid);
                let parsed: Transaction = bitcoin::consensus::deserialize(tx).unwrap();
                assert_eq!(parsed.compute_txid(), taken.txid);
                assert!(parsed.input.iter().all(|i| !i.witness.is_empty()));
            }
            other => panic!("{other:?}"),
        }
        // The session stays with the other item and ends with its last.
        assert_eq!(f.owner.own_sessions(), vec![f.session], "{name}");
        assert_eq!(f.owner.own_items(&f.session).unwrap().len(), 1);
        let second = file(&mut f, 1);
        let signed = wallet_signs(&second.psbt, &f.own.keys);
        take(&mut f, 1, &signed.serialize()).unwrap();
        assert!(f.owner.own_sessions().is_empty(), "{name}");
        assert!(f.owner.open_sessions().is_empty(), "{name}");
        assert_eq!(f.owner.outbox().len(), 2);
    }
}

#[test]
fn as_104_a_failed_store_changes_nothing_and_the_queue_is_stored_before_the_outbox() {
    let mut f = fixture(single());
    let first = file(&mut f, 0);
    let signed = wallet_signs(&first.psbt, &f.own.keys);
    let before = f.owner.snapshot().unwrap();
    let r = f.owner.take_own(&f.session, 0, &signed.serialize(), |_| {
        Err("disk full".to_string())
    });
    assert!(matches!(r, Err(OwnerError::Store(_))));
    assert_eq!(f.owner.snapshot().unwrap(), before);
    assert!(f.owner.outbox().is_empty());
    // What was stored on success holds the transaction in the outbox.
    take(&mut f, 0, &signed.serialize()).unwrap();
    let back = restore(&f.own, &f.store.last()).unwrap();
    assert_eq!(back.outbox().len(), 1);
}

#[test]
fn as_104_the_frozen_outputs_are_required() {
    // A transaction the wallet made with other outputs is another payment.
    let mut f = fixture(single());
    let first = file(&mut f, 0);
    let mut psbt = Psbt::deserialize(&first.psbt).unwrap();
    psbt.unsigned_tx.output[0].value = Amount::from_sat(1_000);
    let signed = wallet_signs(&psbt.serialize(), &f.own.keys);
    let r = own_refusal(take(&mut f, 0, &signed.serialize()).unwrap_err());
    assert_eq!(r, OwnSpendRefusal::DifferentTransaction);
}

/// A refund of 10,000 for sale `n`, to an address made from `n`.
fn refund_to(owner: &mut Owner, n: u32) -> u32 {
    set_facts(owner, sale(n), deep(BIG));
    let mut hash = [0u8; 20];
    hash[..4].copy_from_slice(&n.to_le_bytes());
    let script = ScriptBuf::new_p2wpkh(&bitcoin::WPubkeyHash::from_byte_array(hash));
    owner
        .queue_refund(sale(n), "Mia", script, 10_000, None, nop)
        .unwrap()
}

#[test]
fn as_104_a_full_outbox_refuses_and_changes_nothing() {
    let own = single();
    let options = Options {
        packing: RefundPacking::OneEach,
        ..Options::default()
    };
    let mut owner = own_owner(&own, options);
    let mut taken = 0;
    let mut tag = 0u32;
    // Fill the outbox with 256 finished transactions: 255 refunds (the queue
    // holds 256 entries at most) and one cash-out.
    while taken < 255 {
        let mut picked = Vec::new();
        let mut coins = Vec::new();
        for _ in 0..16.min(255 - taken) {
            tag += 1;
            picked.push(refund_to(&mut owner, tag));
            coins.push(own_coin(&own, tag, tag, 100_000));
        }
        taken += picked.len();
        let mut seed = [0u8; 32];
        seed[..4].copy_from_slice(&tag.to_le_bytes());
        let built =
            ok(owner.build_own_spend(&coins, &picked, 1_000, tag, 850_000, NOW, &seed, nop));
        let session = built.session.unwrap();
        for item in &built.items {
            let file = ok(owner.own_file(&session, item.number, nop));
            let signed = wallet_signs(&file.psbt, &own.keys);
            ok(owner.take_own(&session, item.number, &signed.serialize(), nop));
        }
    }
    let cash = |owner: &mut Owner, n: u32| {
        let id = owner
            .queue_cash_out(external(9), CashAmount::Sats(50_000), "Savings", nop)
            .unwrap();
        let coins = [own_coin(&own, 9_000 + n, 9_000 + n, 100_000)];
        let built = ok(owner.build_own_spend(
            &coins,
            &[id],
            1_000,
            9_000 + n,
            850_000,
            NOW,
            &[n as u8; 32],
            nop,
        ));
        let session = built.session.unwrap();
        let file = ok(owner.own_file(&session, 0, nop));
        (session, wallet_signs(&file.psbt, &own.keys).serialize())
    };
    let (session, signed) = cash(&mut owner, 1);
    ok(owner.take_own(&session, 0, &signed, nop));
    assert_eq!(owner.outbox().len(), 256);
    let (session, signed) = cash(&mut owner, 2);
    let before = owner.snapshot().unwrap();
    let r = owner.take_own(&session, 0, &signed, nop);
    assert_eq!(refused(r), Refusal::OwnSpend(OwnSpendRefusal::OutboxFull));
    assert_eq!(owner.snapshot().unwrap(), before);
}

// ---- AS-105 ---------------------------------------------------------------------------

#[test]
fn as_105_the_transaction_is_watched_from_the_first_file() {
    for (name, kind) in both_kinds() {
        let mut f = fixture(kind());
        assert!(f.owner.exported_transactions(None).is_empty(), "{name}");
        let first = file(&mut f, 0);
        let watched = f.owner.exported_transactions(None);
        assert_eq!(watched.len(), 1, "{name}");
        assert_eq!(watched[0].txid, first.txid);
        assert!(!watched[0].inputs.is_empty());
        assert!(f.owner.own_items(&f.session).unwrap()[0].filed);
    }
}

#[test]
fn as_105_a_wallet_that_sends_the_payment_itself_marks_it_paid_when_confirmed() {
    for (name, kind) in both_kinds() {
        let mut f = fixture(kind());
        let first = file(&mut f, 0);
        file(&mut f, 1);
        f.owner.mark_exported_confirmed(&first.txid, nop).unwrap();
        let item = f.built.items[0].clone();
        for id in item.refunds.iter().copied().chain(item.cash_out) {
            let stage = f
                .owner
                .refunds()
                .iter()
                .find(|r| r.id == id)
                .map(|r| r.stage.clone())
                .or_else(|| {
                    f.owner
                        .cash_outs()
                        .iter()
                        .find(|c| c.id == id)
                        .map(|c| c.stage.clone())
                })
                .unwrap();
            assert_eq!(stage, Stage::Paid { txid: first.txid }, "{name}");
        }
        // The item left the session; the other is still there and watched.
        assert_eq!(f.owner.own_items(&f.session).unwrap().len(), 1);
        assert_eq!(f.owner.exported_transactions(None).len(), 1);
        // The last item ends the session: nothing is left to abandon.
        let second = f.owner.exported_transactions(None)[0].txid;
        f.owner.mark_exported_confirmed(&second, nop).unwrap();
        assert!(f.owner.own_sessions().is_empty(), "{name}");
        assert!(f.owner.outbox().is_empty());
        // A transaction that is not watched is refused.
        assert!(matches!(
            refused(f.owner.mark_exported_confirmed(&second, nop)),
            Refusal::UnknownTransaction(_)
        ));
    }
}

#[test]
fn as_105_another_transaction_that_spends_a_coin_leaves_the_entries_their_outpoints() {
    for (name, kind) in both_kinds() {
        let mut f = fixture(kind());
        let first = file(&mut f, 0);
        let watched = f.owner.exported_transactions(None);
        f.owner.mark_exported_replaced(&first.txid, nop).unwrap();
        let item = f.built.items[0].clone();
        for id in item.refunds.iter().copied().chain(item.cash_out) {
            let (stage, conflict) = f
                .owner
                .refunds()
                .iter()
                .find(|r| r.id == id)
                .map(|r| (r.stage.clone(), r.conflict.clone()))
                .or_else(|| {
                    f.owner
                        .cash_outs()
                        .iter()
                        .find(|c| c.id == id)
                        .map(|c| (c.stage.clone(), c.conflict.clone()))
                })
                .unwrap();
            assert_eq!(stage, Stage::Queued, "{name}");
            assert_eq!(conflict, watched[0].inputs, "{name}");
        }
        assert_eq!(f.owner.own_items(&f.session).unwrap().len(), 1);
        // A transaction that is not watched is refused, and so is one of a
        // session of the Key app (there is none in this shop).
        assert!(matches!(
            refused(f.owner.mark_exported_replaced(&first.txid, nop)),
            Refusal::UnknownTransaction(_)
        ));
    }
}

#[test]
fn as_105_an_abandon_after_the_file_keeps_the_transaction_as_signed_elsewhere() {
    for (name, kind) in both_kinds() {
        let mut f = fixture(kind());
        let first = file(&mut f, 0);
        f.owner.abandon_session(&f.session, nop).unwrap();
        assert!(f.owner.own_sessions().is_empty());
        assert_eq!(f.owner.signed_elsewhere().len(), 1, "{name}");
        assert_eq!(f.owner.signed_elsewhere()[0].txid, first.txid);
        // The entries are queued again and keep what it spends (AS-68).
        let item = &f.built.items[0];
        let entry = f
            .owner
            .refunds()
            .iter()
            .find(|r| item.refunds.contains(&r.id))
            .map(|r| (r.stage.clone(), r.conflict.clone()))
            .or_else(|| {
                f.owner
                    .cash_outs()
                    .iter()
                    .find(|c| Some(c.id) == item.cash_out)
                    .map(|c| (c.stage.clone(), c.conflict.clone()))
            })
            .unwrap();
        assert_eq!(entry.0, Stage::Queued);
        assert!(!entry.1.is_empty());
        // A confirmation is reported as for any payment signed elsewhere.
        f.owner.mark_elsewhere_confirmed(&first.txid, nop).unwrap();
    }
}

#[test]
fn as_105_an_abandon_before_any_file_keeps_nothing() {
    let mut f = fixture(single());
    f.owner.abandon_session(&f.session, nop).unwrap();
    assert!(f.owner.signed_elsewhere().is_empty());
    assert!(f
        .owner
        .refunds()
        .iter()
        .all(|r| r.stage == Stage::Queued && r.conflict.is_empty()));
}

// ---- AS-106 ---------------------------------------------------------------------------

#[test]
fn as_106_a_merge_is_one_cash_out_of_its_coins_to_the_next_change_address() {
    for (name, kind) in both_kinds() {
        let own = kind();
        let mut owner = own_owner(&own, Options::default());
        let change = own.wallet.change_script(0).unwrap();
        let id = owner
            .queue_cash_out(change.clone(), CashAmount::All, "Your own wallet", nop)
            .unwrap();
        let coins: Vec<Coin> = (0..12)
            .map(|n| own_coin(&own, 100 + n, n, 30_000))
            .collect();
        let built = ok(owner.build_own_spend(&coins, &[id], 1_000, 0, 850_000, NOW, &[4; 32], nop));
        assert_eq!(built.items.len(), 1, "{name}");
        let session = built.session.unwrap();
        let file = ok(owner.own_file(&session, 0, nop));
        let psbt = Psbt::deserialize(&file.psbt).unwrap();
        // Twelve coins in, one output, which is the wallet's change address,
        // shown as a move inside the wallet: its key hint is in the file.
        assert_eq!(psbt.unsigned_tx.input.len(), 12, "{name}");
        assert_eq!(psbt.unsigned_tx.output.len(), 1, "{name}");
        assert_eq!(psbt.unsigned_tx.output[0].script_pubkey, change);
        assert!(!psbt.outputs[0].bip32_derivation.is_empty(), "{name}");
        // The wallet signs it and the owner takes it.
        let signed = wallet_signs(&file.psbt, &own.keys);
        ok(owner.take_own(&session, 0, &signed.serialize(), nop));
        assert_eq!(owner.outbox().len(), 1);
    }
}

// ---- AS-107 ---------------------------------------------------------------------------

fn snapshot_with_sessions(kind: fn() -> Own) -> (Fixture, Vec<u8>) {
    let mut f = fixture(kind());
    file(&mut f, 0);
    let bytes = f.owner.snapshot().unwrap();
    (f, bytes)
}

#[test]
fn as_107_the_open_sessions_are_kept_in_version_5_and_read_back() {
    for (name, kind) in both_kinds() {
        let (f, bytes) = snapshot_with_sessions(kind);
        assert_eq!(&bytes[..4], b"OBM4");
        assert_eq!(bytes[4], 5, "{name}");
        let back = restore(&f.own, &bytes).unwrap();
        assert_eq!(back.own_sessions(), vec![f.session], "{name}");
        // What each froze, and which items were filed, come back.
        assert_eq!(
            back.own_items(&f.session).unwrap(),
            f.owner.own_items(&f.session).unwrap(),
            "{name}"
        );
        assert!(back.own_items(&f.session).unwrap()[0].filed);
        assert!(!back.own_items(&f.session).unwrap()[1].filed);
        assert_eq!(back.snapshot().unwrap(), bytes, "{name}");
        assert_eq!(
            back.exported_transactions(None),
            f.owner.exported_transactions(None)
        );
        // And the session can go on: the file again, and the take.
        let mut back = back;
        let again = ok(back.own_file(&f.session, 0, nop));
        let signed = wallet_signs(&again.psbt, &f.own.keys);
        ok(back.take_own(&f.session, 0, &signed.serialize(), nop));
    }
}

#[test]
fn as_107_versions_1_to_4_read_as_before_with_no_own_spend_session() {
    for (name, kind) in both_kinds() {
        let own = kind();
        // Version 1 (nothing of a Box), and version 4 (a transaction
        // watched elsewhere) of a shop that holds no own-spend session.
        let mut owner = own_owner(&own, Options::default());
        let bytes = owner.snapshot().unwrap();
        assert_eq!(bytes[4], 1, "{name}");
        let back = restore(&own, &bytes).unwrap();
        assert!(back.own_sessions().is_empty());
        let mut f = fixture(kind());
        let first = file(&mut f, 0);
        f.owner.abandon_session(&f.session, nop).unwrap();
        let v4 = f.owner.snapshot().unwrap();
        assert_eq!(v4[4], 4, "{name}");
        let back = restore(&f.own, &v4).unwrap();
        assert!(back.own_sessions().is_empty());
        assert_eq!(back.signed_elsewhere()[0].txid, first.txid);
        // An own-spend session in a version 4 file is not read.
        let (g, v5) = snapshot_with_sessions(kind);
        let mut lie = v5.clone();
        lie[4] = 4;
        assert!(restore(&g.own, &lie).is_err(), "{name}");
        let _ = &mut owner;
    }
}

#[test]
fn as_107_a_shop_with_its_own_wallet_that_holds_a_key_app_session_is_refused() {
    // A Key app shop's snapshot with a session of the Key app, read by the
    // owner of a shop with its own wallet that has the same wallet.
    let mut rig = Rig::with(Options::default(), true);
    refund(&mut rig.owner, 1, BIG, 40_000);
    rig.build(&[coin(1, 0, 500_000)], 0);
    let bytes = rig.owner.snapshot().unwrap();
    let own = Own {
        wallet: ImportedWallet::Multisig(wallet().clone()),
        keys: Vec::new(),
    };
    let err = restore(&own, &bytes).unwrap_err();
    // The snapshot is for the Key app of the Key app shop: the owner of the
    // own-wallet shop has another approver, so it does not read at all; the
    // same owner with the Key app's approver and the placeholder flag set is
    // what the library refuses below.
    assert_eq!(err, openbmt_owner::RestoreError::WrongApprover);
}

#[test]
fn as_107_a_key_app_shop_that_holds_an_own_spend_session_is_refused() {
    // The own-spend snapshot of a wallet of three keys, read by an owner
    // with a real Key app: the approver is another, so the wallet may be the
    // same and the snapshot is still refused.
    let (_, bytes) = snapshot_with_sessions(multi);
    let err = restore_owner(&bytes).unwrap_err();
    assert!(matches!(err, openbmt_owner::RestoreError::WrongApprover));
}

#[test]
fn as_107_the_two_refusals_are_the_checks_of_the_restore() {
    // The checks themselves, by flipping what the snapshot says an own
    // session is: a Key app session in the own-spend list of a Key app shop.
    // They are exercised through `snapshot`/`restore` of owners whose
    // approver is chosen to match, so the refusal is the contradiction and
    // not the approver.
    let own = multi();
    // Own owner, Key app session.
    let mut rig = Rig::with(Options::default(), true);
    refund(&mut rig.owner, 1, BIG, 40_000);
    rig.build(&[coin(1, 0, 500_000)], 0);
    let key_app_bytes = rig.owner.snapshot().unwrap();
    // Patch the approver of the snapshot to the own approver: the snapshot
    // names it, and the owner of the own shop takes it.
    let own_key = own_approver(&OWNER_SECRET).unwrap();
    let mut patched = key_app_bytes.clone();
    let approver_at = 5 + 32 + 32;
    patched[approver_at..approver_at + 32].copy_from_slice(&own_key.to_bytes());
    let err = restore(&own, &patched).unwrap_err();
    assert!(
        matches!(
            &err,
            openbmt_owner::RestoreError::Inconsistent(why) if why.contains("own wallet holds a session for the Key app")
        ),
        "{err}"
    );
    // Key app owner, own-spend session.
    let (_, own_bytes) = snapshot_with_sessions(multi);
    let mut patched = own_bytes.clone();
    patched[approver_at..approver_at + 32].copy_from_slice(&approver_id().to_bytes());
    let err = restore_owner(&patched).unwrap_err();
    assert!(
        matches!(
            &err,
            openbmt_owner::RestoreError::Inconsistent(why) if why.contains("Key app holds a session for its own wallet")
        ),
        "{err}"
    );
}

#[test]
fn as_107_decoding_is_strict_and_bounded() {
    let (f, bytes) = snapshot_with_sessions(single);
    for n in 0..bytes.len() {
        assert!(restore(&f.own, &bytes[..n]).is_err(), "cut at {n}");
    }
    let mut longer = bytes.clone();
    longer.push(0);
    assert!(restore(&f.own, &longer).is_err());
    // Every single-byte change either is refused or reads back as the same
    // snapshot: nothing reads and writes differently.
    for at in (0..bytes.len()).step_by(7) {
        let mut flipped = bytes.clone();
        flipped[at] ^= 0x40;
        if let Ok(o) = restore(&f.own, &flipped) {
            assert_eq!(o.snapshot().unwrap(), flipped, "byte {at}");
        }
    }
    // Version 5 with no own-spend session is not what encoding gives.
    let own = single();
    let owner = own_owner(&own, Options::default());
    let mut empty = owner.snapshot().unwrap();
    empty[4] = 5;
    empty.extend_from_slice(&0u32.to_le_bytes());
    assert!(restore(&own, &empty).is_err());
    // A version past 5 is refused.
    let mut six = bytes.clone();
    six[4] = 6;
    assert!(restore(&f.own, &six).is_err());
}

#[test]
fn as_107_a_queue_that_would_not_restore_is_not_written() {
    // AS-88: encode runs the check of restore. A Key app session in a shop
    // with its own wallet could only come from a caller that built one: the
    // build refuses it first.
    let own = single();
    let mut owner = own_owner(&own, Options::default());
    refund(&mut owner, 1, BIG, 40_000);
    let r = owner.build_request(
        &[own_coin(&own, 1, 0, 500_000)],
        1_000,
        0,
        850_000,
        NOW,
        &[1; 96],
        nop,
    );
    assert_eq!(refused(r), Refusal::NoKeyApp);
}

// ---- the words -----------------------------------------------------------------------------

#[test]
fn as_102_every_state_and_refusal_has_words_that_name_its_rule() {
    let all = all_own_spend_words();
    assert!(all.len() >= 13);
    for (name, w) in &all {
        assert!(!w.heading.starts_with("WORDS_NEEDED"), "{name}");
        assert!(w.code.is_some_and(|c| c.starts_with("AS-10")), "{name}");
        assert!(w.heading.chars().count() < 28, "{name}: {}", w.heading);
        assert!(w.what.ends_with('.') && w.next.ends_with('.'), "{name}");
    }
    let mut headings: Vec<&str> = all.iter().map(|(_, w)| w.heading.as_str()).collect();
    headings.sort_unstable();
    headings.dedup();
    assert_eq!(headings.len(), all.len(), "each state has its own name");
    assert_eq!(all_refusals().len(), 11);
}

// ---- the starter wallet's watch (SS-36, SS-30, SS-42) --------------------------------

/// A transaction that spends `from` and pays `to_wallet` `sats` at receive
/// index `index`.
fn pays_wallet(own: &Own, from: &Coin, index: u32, sats: u64) -> Transaction {
    Transaction {
        version: Version::TWO,
        lock_time: LockTime::ZERO,
        input: vec![TxIn {
            previous_output: from.outpoint,
            script_sig: ScriptBuf::new(),
            sequence: Sequence::MAX,
            witness: Witness::new(),
        }],
        output: vec![TxOut {
            value: Amount::from_sat(sats),
            script_pubkey: own.wallet.receive_script(index).unwrap(),
        }],
    }
}

#[test]
fn ss_36_the_held_amount_is_the_confirmed_coins_and_the_payments_on_their_way_each_counted_once() {
    use openbmt_owner::starter::{held_sats, IncomingCoin};
    let own = single();
    let confirmed = own_coin(&own, 1, 0, 40_000);
    let other = own_coin(&own, 2, 1, 2_500);
    assert_eq!(held_sats(&[], &[]), 0);
    assert_eq!(held_sats(&[confirmed.clone(), other.clone()], &[]), 42_500);
    // A payment into the wallet that no block holds yet counts.
    let incoming_tx = pays_wallet(&own, &own_coin(&own, 9, 5, 1), 2, 7_000);
    let incoming = IncomingCoin {
        outpoint: OutPoint::new(incoming_tx.compute_txid(), 0),
        sats: 7_000,
    };
    assert_eq!(
        held_sats(std::slice::from_ref(&confirmed), &[incoming]),
        47_000
    );
    // The change of the owner's own unconfirmed payment is offered as a coin
    // at 0 confirmations: it counts once, whether or not it is also listed as
    // incoming, and the coin that payment spent counts no more.
    let change_tx = pays_wallet(&own, &confirmed, 3, 30_000);
    let change = Coin {
        outpoint: OutPoint::new(change_tx.compute_txid(), 0),
        prev_tx: change_tx.clone(),
        vout: 0,
        chain: Keychain::Change,
        index: 3,
        confirmations: 0,
    };
    let listed_again = IncomingCoin {
        outpoint: change.outpoint,
        sats: 30_000,
    };
    assert_eq!(held_sats(&[other.clone(), change.clone()], &[]), 32_500);
    assert_eq!(
        held_sats(&[other.clone(), change.clone()], &[listed_again]),
        32_500
    );
    // The same incoming output given twice counts once.
    assert_eq!(held_sats(&[], &[incoming, incoming]), 7_000);
}

#[test]
fn ss_30_a_transaction_of_the_starter_wallet_waits_in_the_outbox_with_its_refund_signed() {
    let own = single();
    let mut owner = own_owner(&own, Options::default());
    let r = refund_to(&mut owner, 1);
    let coin = own_coin(&own, 1, 0, 100_000);
    let tx = pays_wallet(&own, &coin, 1, 90_000);
    let bytes = serialize(&tx);
    let store = Store::default();
    // A refund that is not queued, a transaction that does not read and a
    // failed store change nothing.
    let before = owner.snapshot().unwrap();
    assert!(matches!(
        owner.take_starter(&bytes, &[r + 50], |b| store.put(b)),
        Err(OwnerError::Refused(Refusal::UnknownEntry(_)))
    ));
    assert!(matches!(
        owner.take_starter(&[1, 2, 3], &[], |b| store.put(b)),
        Err(OwnerError::Refused(Refusal::WrongStage(_)))
    ));
    assert!(matches!(
        owner.take_starter(&bytes, &[r], |_| Err("disk full".to_string())),
        Err(OwnerError::Store(_))
    ));
    assert_eq!(owner.snapshot().unwrap(), before);
    assert!(owner.outbox().is_empty());
    // Taken: the refund is signed, the transaction is in the outbox, and it
    // was stored first.
    let txid = owner.take_starter(&bytes, &[r], |b| store.put(b)).unwrap();
    assert_eq!(txid, tx.compute_txid());
    assert_eq!(owner.outbox().len(), 1);
    assert!(matches!(
        &owner.outbox()[0].what,
        Delivery::Transaction { txid: t, refunds, cash_out: None, .. } if *t == txid && refunds == &[r]
    ));
    let stage = &owner.refunds().iter().find(|e| e.id == r).unwrap().stage;
    assert_eq!(*stage, Stage::Signed { txid });
    let back = restore(&own, &store.last()).unwrap();
    assert_eq!(back.outbox().len(), 1);
    // The same transaction again changes nothing.
    let calls = store.calls();
    assert_eq!(
        owner.take_starter(&bytes, &[r], |b| store.put(b)).unwrap(),
        txid
    );
    assert_eq!(owner.outbox().len(), 1);
    assert_eq!(store.calls(), calls);
    // The move pays no refund: it is in the outbox on its own.
    let other = own_coin(&own, 2, 2, 50_000);
    let moved = pays_wallet(&own, &other, 9, 49_000);
    owner
        .take_starter(&serialize(&moved), &[], |b| store.put(b))
        .unwrap();
    assert_eq!(owner.outbox().len(), 2);
}
