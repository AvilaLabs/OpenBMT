//! The owner node of a shop with its own wallet spends from it: build,
//! file, take and send, and the watch of a filed item (AS-100 to AS-106).
//! The wallet is played by one test key.
//!
//! TEST ONLY, NEVER USE: the key comes from fixed bytes.

use bitcoin::bip32::{ChildNumber, Fingerprint, Xpriv, Xpub};
use bitcoin::psbt::Psbt;
use bitcoin::secp256k1::Message;
use bitcoin::sighash::{EcdsaSighashType, SighashCache};
use bitcoin::{ecdsa, PublicKey};
use openbmt_core::wallet_single::{ImportedWallet, SingleKeyWallet};
use openbmt_owner::own_spend::OwnBuilt;
use openbmt_owner::CashAmount;

use super::*;

fn key() -> (Fingerprint, Xpriv) {
    let master = Xpriv::new_master(Network::Bitcoin, &[0x77; 32]).unwrap();
    let path: DerivationPath = "m/84'/0'/0'".parse().unwrap();
    (
        master.fingerprint(secp()),
        master.derive_priv(secp(), &path).unwrap(),
    )
}

fn wallet() -> ImportedWallet {
    let (fp, account) = key();
    ImportedWallet::Single(SingleKeyWallet::new(fp, 0, Xpub::from_priv(secp(), &account)).unwrap())
}

/// What the shop's wallet does with a file: signs every input.
fn wallet_signs(file: &[u8]) -> Vec<u8> {
    let (fp, account) = key();
    let mut psbt = Psbt::deserialize(file).unwrap();
    let tx = psbt.unsigned_tx.clone();
    let mut cache = SighashCache::new(&tx);
    for (i, input) in psbt.inputs.iter_mut().enumerate() {
        let spent = match (&input.witness_utxo, &input.non_witness_utxo) {
            (Some(o), _) => o.clone(),
            (None, Some(prev)) => prev.output[tx.input[i].previous_output.vout as usize].clone(),
            _ => panic!("no previous output"),
        };
        for (pk, (f, path)) in input.bip32_derivation.clone() {
            assert_eq!(f, fp);
            let tail: Vec<ChildNumber> = path.into_iter().rev().take(2).rev().cloned().collect();
            let sk = account.derive_priv(secp(), &tail).unwrap().private_key;
            let digest = cache
                .p2wpkh_signature_hash(i, &spent.script_pubkey, spent.value, EcdsaSighashType::All)
                .unwrap()
                .to_byte_array();
            input.partial_sigs.insert(
                PublicKey::new(pk),
                ecdsa::Signature {
                    signature: secp().sign_ecdsa(&Message::from_digest(digest), &sk),
                    sighash_type: EcdsaSighashType::All,
                },
            );
        }
    }
    psbt.serialize()
}

fn own_rig(dir: &Dir) -> Rig {
    let setup = Setup {
        own: Some(AllocationBook::new()),
        own_wallet: Some(wallet()),
        ..Setup::default()
    };
    let mut rig = start(dir, &setup).unwrap();
    let mut book = AllocationBook::new();
    book.allocate(rig.node.owner().owner_id(), NOW).unwrap();
    rig.node.set_own_book(&book).unwrap();
    rig.rescan();
    rig
}

/// Fund the wallet's receive 0 with `sats` and bury it.
fn fund_own(rig: &mut Rig, from: u8, sats: u64) {
    let script = wallet().receive_script(0).unwrap();
    let pay = tx_from(
        OutPoint::new(Txid::from_byte_array([from; 32]), 0),
        &[(&script, sats)],
    );
    rig.mine(std::slice::from_ref(&pay));
    rig.mine_empty(5);
}

fn queue_own_cash_out(rig: &mut Rig) -> u32 {
    let write = rig.node.queue_writer();
    rig.node
        .owner_mut()
        .unwrap()
        .queue_cash_out(external(9), CashAmount::Sats(100_000), "Savings", &write)
        .unwrap()
}

fn built(rig: &mut Rig) -> (OwnBuilt, u32) {
    fund_own(rig, 7, 300_000);
    let refund = queue_own_cash_out(rig);
    let built = rig.node.build_own_spend(&[refund], 1_000, NOW).unwrap();
    rig.rescan();
    (built, refund)
}

#[test]
fn as_101_the_node_builds_a_spend_for_the_own_wallet_and_holds_its_coins() {
    let dir = Dir::new();
    let mut rig = own_rig(&dir);
    let (built, refund) = built(&mut rig);
    let session = built.session.expect("frozen");
    assert_eq!(built.items.len(), 1);
    assert!(rig.node.owner().open_sessions().contains(&session));
    assert!(matches!(
        rig.node.owner().cash_outs()[0].stage,
        Stage::InSession { .. }
    ));
    assert_eq!(rig.node.owner().cash_outs()[0].id, refund);
    // A restart keeps it.
    drop(rig);
    let rig = own_rig(&dir);
    assert!(rig.node.owner().open_sessions().contains(&session));
}

#[test]
fn as_103_as_104_a_signed_file_is_taken_and_sent_through_the_outbox() {
    let dir = Dir::new();
    let mut rig = own_rig(&dir);
    let (built, _) = built(&mut rig);
    let session = built.session.unwrap();
    let number = built.items[0].number;
    let file = rig.node.own_file(&session, number).unwrap();
    let taken = rig
        .node
        .take_own(&session, number, &wallet_signs(&file.psbt))
        .unwrap();
    rig.rescan();
    assert_eq!(taken.txid, file.txid);
    assert_eq!(rig.node.owner().outbox().len(), 1);
    assert!(rig
        .calls()
        .contains(&format!("main broadcast {}", taken.txid)));
    assert!(rig.node.owner().open_sessions().is_empty());
}

#[test]
fn as_105_a_wallet_that_broadcasts_itself_pays_the_entries_when_it_confirms() {
    let dir = Dir::new();
    let mut rig = own_rig(&dir);
    let (built, refund) = built(&mut rig);
    let session = built.session.unwrap();
    let number = built.items[0].number;
    let file = rig.node.own_file(&session, number).unwrap();
    // The wallet signs, finishes and sends; the file never comes back.
    let signed = wallet_signs(&file.psbt);
    let tx = openbmt_spend::finalize(&wallet(), &Psbt::deserialize(&signed).unwrap()).unwrap();
    assert_eq!(tx.compute_txid(), file.txid);
    rig.mine(std::slice::from_ref(&tx));
    let entry = rig
        .node
        .owner()
        .cash_outs()
        .iter()
        .find(|r| r.id == refund)
        .cloned()
        .unwrap();
    assert_eq!(entry.stage, Stage::Paid { txid: file.txid });
    assert!(rig.node.owner().open_sessions().is_empty());
    assert_eq!(rig.node.paid_log().len(), 1);
    // A restart keeps it paid.
    drop(rig);
    let rig = own_rig(&dir);
    assert!(matches!(
        rig.node.owner().cash_outs()[0].stage,
        Stage::Paid { .. }
    ));
}

#[test]
fn as_105_a_coin_another_transaction_spent_puts_the_entries_back_in_the_queue() {
    let dir = Dir::new();
    let mut rig = own_rig(&dir);
    let (built, _) = built(&mut rig);
    let session = built.session.unwrap();
    let number = built.items[0].number;
    let file = rig.node.own_file(&session, number).unwrap();
    let tx = Psbt::deserialize(&file.psbt).unwrap().unsigned_tx;
    let other = tx_from(tx.input[0].previous_output, &[(&external(77), 1_000)]);
    rig.mine(&[other]);
    assert!(rig.node.owner().open_sessions().is_empty());
    let entry = &rig.node.owner().cash_outs()[0];
    assert!(matches!(entry.stage, Stage::Queued));
    assert!(!entry.conflict.is_empty());
    assert!(rig.node.paid_log().is_empty());
}

#[test]
fn as_104_an_abandoned_own_spend_releases_its_entries() {
    let dir = Dir::new();
    let mut rig = own_rig(&dir);
    let (built, _) = built(&mut rig);
    let session = built.session.unwrap();
    rig.node.abandon_session(&session).unwrap();
    assert!(rig.node.owner().open_sessions().is_empty());
    assert!(matches!(
        rig.node.owner().cash_outs()[0].stage,
        Stage::Queued
    ));
}

#[test]
fn as_100_a_key_app_build_is_refused_in_an_own_wallet_shop() {
    let dir = Dir::new();
    let mut rig = own_rig(&dir);
    fund_own(&mut rig, 7, 300_000);
    let cash = queue_own_cash_out(&mut rig);
    assert!(rig.node.build_request(1_000, NOW).is_err());
    let built = rig.node.build_own_spend(&[cash], 1_000, NOW).unwrap();
    assert_eq!(built.items.len(), 1);
    assert_eq!(built.items[0].cash_out, Some(cash));
}

#[test]
fn as_106_the_merge_offer_of_an_own_wallet_is_built_signed_in_the_wallet_and_sent() {
    use super::receipts::{write_history, COIN, TIP};
    use crate::receipts::MergeRefused;
    let dir = Dir::new();
    write_history(&dir, 5_000, 1_000);
    let mut rig = own_rig(&dir);
    let outs: Vec<(ScriptBuf, u64)> = (0..12)
        .map(|i| (wallet().receive_script(i).unwrap(), COIN))
        .collect();
    let refs: Vec<(&ScriptBuf, u64)> = outs.iter().map(|(s, v)| (s, *v)).collect();
    let pay = tx_from(OutPoint::new(Txid::from_byte_array([7; 32]), 0), &refs);
    rig.mine_full(std::slice::from_ref(&pay), 5_000);
    while rig.chain.len() < usize::try_from(TIP).unwrap() {
        rig.pass_over();
    }
    let offer = rig.node.merge_offer().expect("an offer");
    // The Key app way is refused in this shop.
    assert!(rig.node.approve_merge(NOW).is_err());
    let built = rig.node.approve_merge_own(NOW).unwrap();
    rig.rescan();
    let session = built.session.unwrap();
    let number = built.items[0].number;
    let file = rig.node.own_file(&session, number).unwrap();
    let taken = rig
        .node
        .take_own(&session, number, &wallet_signs(&file.psbt))
        .unwrap();
    rig.rescan();
    let tx = rig.tx(&taken.txid);
    assert_eq!(tx.input.len(), 12);
    assert_eq!(tx.output.len(), 1);
    let paid: u64 = 12 * COIN - tx.output[0].value.to_sat();
    assert_eq!(paid, offer.cost_sats);
    assert_eq!(
        rig.node.approve_merge_own(NOW).err(),
        Some(MergeRefused::NoOffer)
    );
}
