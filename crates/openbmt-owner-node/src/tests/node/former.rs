//! SS-43: the owner node keeps watching the wallet the shop pinned before
//! its re-pin, up to the index its book had reached. A payment that arrives
//! at it later is the node's own fact about that wallet, apart from the
//! wallet in force, and the move of it is sent and followed like any other.
//!
//! TEST ONLY, NEVER USE: the key comes from fixed bytes.

use bitcoin::bip32::{Xpriv, Xpub};
use openbmt_core::pinned_wallet::PinnedWallet;
use openbmt_core::wallet_single::{ImportedWallet, SingleKeyWallet};

use super::own::single;
use super::*;
use crate::{former_coins_file, FormerParts, OwnNode, COINS_OF_FILE};

/// The wallet the shop pinned before: a test key.
fn old_wallet() -> ImportedWallet {
    let master = Xpriv::new_master(Network::Bitcoin, &[0x55; 32]).unwrap();
    let path: DerivationPath = "m/84'/0'/0'".parse().unwrap();
    let account = master.derive_priv(secp(), &path).unwrap();
    ImportedWallet::Single(
        SingleKeyWallet::new(
            master.fingerprint(secp()),
            0,
            Xpub::from_priv(secp(), &account),
        )
        .unwrap(),
    )
}

pub(super) fn old_script(index: u32) -> ScriptBuf {
    old_wallet().receive_script(index).unwrap()
}

fn new_script(index: u32) -> ScriptBuf {
    single().receive_script(index).unwrap()
}

const END: u32 = 1_000;

pub(super) fn former_setup() -> Setup {
    let parts = FormerParts {
        wallet: PinnedWallet::new(old_wallet()),
        end: END,
        file: former_coins_file(&PinnedWallet::new(old_wallet()).checksum32()),
    };
    let mut book = AllocationBook::new();
    book.allocate(id(0x11).device_id(), NOW).unwrap();
    Setup {
        own: Some(book),
        former: vec![parts],
        ..Setup::default()
    }
}

pub(super) fn pay(rig: &mut Rig, from: u8, script: &ScriptBuf, sats: u64) -> Transaction {
    let tx = tx_from(
        OutPoint::new(Txid::from_byte_array([from; 32]), 0),
        &[(script, sats)],
    );
    rig.mine(std::slice::from_ref(&tx));
    rig.mine_empty(2);
    tx
}

#[test]
fn ss_43_a_payment_to_an_old_starter_address_after_the_repin_is_seen_and_is_not_the_new_wallets_coin(
) {
    let dir = Dir::new();
    let mut rig = start(&dir, &former_setup()).unwrap();
    // The main driver was given the scripts of both wallets.
    assert!(rig.spawned.lock().unwrap()[0].scripts > 2 * END as usize);
    assert!(rig.node.former_held().is_empty());
    // A payment to an address of the old wallet, and one to the new one.
    pay(&mut rig, 7, &old_script(3), 40_000);
    pay(&mut rig, 8, &new_script(0), 9_000);
    let held = rig.node.former_held();
    assert_eq!(held.len(), 1);
    assert_eq!(held[0].sats, 40_000);
    assert_eq!(held[0].coins, 1);
    assert_eq!(held[0].wallet, PinnedWallet::new(old_wallet()).checksum32());
    // The wallet in force has only its own coin: the late payment is not in
    // what it holds.
    let coins = rig.node.coins();
    assert_eq!(coins.len(), 1);
    assert_eq!(rig.node.starter_held(), Some(9_000));
    // A payment beyond what its book handed out (the end and the gap) is not
    // watched.
    pay(&mut rig, 9, &old_script(END + 500), 5_000);
    assert_eq!(rig.node.former_held()[0].sats, 40_000);
    // A restart keeps what the view found.
    drop(rig);
    let rig = start_at(&dir, &former_setup(), Vec::new()).unwrap();
    assert_eq!(rig.node.former_held().len(), 1);
    assert_eq!(rig.node.former_held()[0].sats, 40_000);
}

#[test]
fn ss_43_the_move_of_a_late_payment_is_sent_followed_and_empties_the_former_wallet() {
    let dir = Dir::new();
    let mut rig = start(&dir, &former_setup()).unwrap();
    let late = pay(&mut rig, 7, &old_script(3), 40_000);
    let hash = PinnedWallet::new(old_wallet()).checksum32();
    let (coins, tip) = rig.node.former_coins(&hash).unwrap();
    assert_eq!(coins.len(), 1);
    assert_eq!(tip, u32::try_from(rig.chain.len()).unwrap());
    // The move pays the wallet in force, from the coin of the old one.
    let mv = tx_from(
        OutPoint::new(late.compute_txid(), 0),
        &[(&new_script(5), 39_000)],
    );
    let txid = rig.node.take_starter(&serialize(&mv), &[]).unwrap();
    assert_eq!(txid, mv.compute_txid());
    // The node sends it, and holds the coin as spent: it is not offered.
    assert!(rig.calls().contains(&format!("main broadcast {txid}")));
    assert!(rig.node.former_held().is_empty());
    assert_eq!(rig.node.starter_confirmations(&txid), Some(0));
    // A block holds it: confirmations count from the former view, and the
    // outbox follows it to the refund depth.
    rig.mine(std::slice::from_ref(&mv));
    assert_eq!(rig.node.starter_confirmations(&txid), Some(1));
    rig.mine_empty(5);
    assert_eq!(rig.node.starter_confirmations(&txid), Some(6));
    assert!(rig.node.owner().outbox().is_empty());
    assert!(rig.node.former_held().is_empty());
    // The coin it paid is the wallet in force's.
    assert_eq!(rig.node.coins().len(), 1);
    // The coins of a wallet that was never pinned are refused.
    assert!(rig.node.former_coins(&[9; 32]).is_err());
}

#[test]
fn ss_43_a_restart_sends_the_move_of_a_late_payment_again_and_keeps_the_former_wallets_view() {
    let dir = Dir::new();
    let mut rig = start(&dir, &former_setup()).unwrap();
    let late = pay(&mut rig, 7, &old_script(3), 40_000);
    let chain = rig.chain.clone();
    let mv = tx_from(
        OutPoint::new(late.compute_txid(), 0),
        &[(&new_script(5), 39_000)],
    );
    let txid = rig.node.take_starter(&serialize(&mv), &[]).unwrap();
    drop(rig);
    // The restart replays the chain above where the lowest view stood.
    let rig = start_at(&dir, &former_setup(), chain).unwrap();
    assert!(rig.calls().contains(&format!("main broadcast {txid}")));
    assert!(rig.node.former_held().is_empty());
    assert_eq!(rig.node.starter_confirmations(&txid), Some(0));
}

#[test]
fn ss_43_ss_41_the_coin_view_file_of_the_wallet_that_was_replaced_is_put_under_its_name_at_start() {
    use openbmt_owner::own_shop::OwnShop;
    let dir = Dir::new();
    let old = PinnedWallet::new(old_wallet());
    let new = PinnedWallet::new(single());
    let identity = || id(0x11);
    let state = dir.path().join(crate::OWN_STATE_FILE);
    // The shop pinned the old wallet and its node wrote a coin view.
    let mut shop = OwnShop::new(identity(), old.clone());
    shop.allocate_desk(NOW, |_: &[u8]| Ok::<(), String>(()))
        .unwrap();
    std::fs::write(&state, shop.to_bytes()).unwrap();
    let node = OwnNode::open(dir.path(), identity(), old.clone()).unwrap();
    node.settle_coins().unwrap();
    assert_eq!(
        std::fs::read(dir.path().join(COINS_OF_FILE)).unwrap(),
        old.checksum32()
    );
    std::fs::write(dir.path().join(crate::COINS_FILE), b"the old view").unwrap();
    // A start with nothing changed leaves the file as it is.
    node.settle_coins().unwrap();
    assert!(dir.path().join(crate::COINS_FILE).exists());
    drop(node);
    // The re-pin: the state is written for the new wallet.
    let mut node = OwnNode::open(dir.path(), identity(), old.clone()).unwrap();
    let (shop, store) = node.shop_and_store();
    shop.repin(&new, &[], store).unwrap();
    drop(node);
    // The next start has the wallet file of either step: the old (the crash
    // between the two writes) or the new. Both put the view under the old
    // wallet's name and leave the new wallet with none.
    for given in [old.clone(), new.clone()] {
        let node = OwnNode::open(dir.path(), identity(), given).unwrap();
        assert_eq!(node.shop().wallet(), &new);
        assert_eq!(node.former_parts().len(), 1);
        assert_eq!(node.former_parts()[0].wallet, old);
        assert_eq!(node.former_parts()[0].end, 1_000);
        node.settle_coins().unwrap();
        let of = std::fs::read(dir.path().join(COINS_OF_FILE)).unwrap();
        assert_eq!(of, new.checksum32());
        assert!(!dir.path().join(crate::COINS_FILE).exists());
        assert_eq!(
            std::fs::read(dir.path().join(former_coins_file(&old.checksum32()))).unwrap(),
            b"the old view"
        );
        // Put things back for the second round.
        std::fs::write(dir.path().join(crate::COINS_FILE), b"the old view").unwrap();
        std::fs::remove_file(dir.path().join(former_coins_file(&old.checksum32()))).unwrap();
        std::fs::write(dir.path().join(COINS_OF_FILE), old.checksum32()).unwrap();
    }
}
