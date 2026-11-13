//! SS-41: the owner's queue moves with the re-pin. The queued refunds, the
//! paid log and the facts of the sales go to the new wallet, the re-pin
//! waits for the starter wallet's other transactions, and a start that finds
//! the change half done finishes it, so the node always starts.
//!
//! TEST ONLY, NEVER USE: the keys come from fixed bytes.

use openbmt_core::pinned_wallet::PinnedWallet;
use openbmt_core::wallet_single::ImportedWallet;
use openbmt_owner::starter::{RebindError, StarterRefusal};
use openbmt_owner::{Owner, Stage};

use super::former::{former_setup, old_script, old_wallet};
use super::own::single;
use super::*;
use crate::{former_coins_file, COINS_FILE, QUEUE_FILE};

/// The starter wallet pinned: the node before the re-pin.
fn old_setup() -> Setup {
    Setup {
        own: former_setup().own,
        own_wallet: Some(old_wallet()),
        ..Setup::default()
    }
}

fn new_script(index: u32) -> ScriptBuf {
    single().receive_script(index).unwrap()
}

/// What the owner node holds at the usual retirement point: a refund that
/// stays queued, a refund that was paid and logged, and the move to the new
/// wallet with one confirmation, in the outbox.
struct Retiring {
    rig: Rig,
    queued: u32,
}

fn retiring(dir: &Dir) -> Retiring {
    let mut rig = start(dir, &old_setup()).unwrap();
    let write = rig.node.queue_writer();
    rig.node
        .owner_mut()
        .unwrap()
        .set_sale_facts(
            sale(1),
            SaleFacts {
                received_sats: 50_000,
                reached_refund_depth: true,
            },
            &write,
        )
        .unwrap();
    let queued = queue_refund_to(&mut rig, 9);
    let paid = queue_refund_to(&mut rig, 10);
    // The wallet's coin, and the sale's payment in the same transaction.
    let fund = tx_from(
        OutPoint::new(Txid::from_byte_array([7; 32]), 0),
        &[(&old_script(0), 300_000), (&old_script(1), 50_000)],
    );
    rig.mine(std::slice::from_ref(&fund));
    rig.mine_empty(5);
    assert!(rig.node.owner().refund_ready(paid));
    let pay = tx_from(
        OutPoint::new(fund.compute_txid(), 0),
        &[(&external(10), 10_000)],
    );
    rig.node.take_starter(&serialize(&pay), &[paid]).unwrap();
    rig.mine(std::slice::from_ref(&pay));
    rig.mine_empty(5);
    assert_eq!(rig.node.paid_log().len(), 1);
    let mv = tx_from(
        OutPoint::new(fund.compute_txid(), 1),
        &[(&new_script(5), 49_000)],
    );
    let mv_id = rig.node.take_starter(&serialize(&mv), &[]).unwrap();
    rig.mine(std::slice::from_ref(&mv));
    assert_eq!(rig.node.starter_confirmations(&mv_id), Some(1));
    assert_eq!(rig.node.owner().outbox().len(), 1);
    Retiring { rig, queued }
}

fn queue_refund_to(rig: &mut Rig, to: u8) -> u32 {
    let write = rig.node.queue_writer();
    rig.node
        .owner_mut()
        .unwrap()
        .queue_refund(sale(1), "Mia", external(to), 10_000, None, &write)
        .unwrap()
}

/// What a re-pin leaves of the view: the file of the wallet that was replaced
/// goes under its own name, as the shop's start puts it.
fn rotate_view(dir: &Dir) {
    let old = PinnedWallet::new(old_wallet());
    let from = dir.path().join(COINS_FILE);
    if from.exists() {
        std::fs::rename(from, dir.path().join(former_coins_file(&old.checksum32()))).unwrap();
    }
}

fn queue_on_disk(dir: &Dir, wallet: ImportedWallet) -> Result<Owner, openbmt_owner::RestoreError> {
    Owner::restore(
        &OWNER_SECRET,
        wallet,
        own_approver(&OWNER_SECRET).unwrap(),
        policy(),
        &std::fs::read(dir.path().join(QUEUE_FILE)).unwrap(),
    )
}

/// The restarted node holds what the queue held before the re-pin.
fn assert_queue_intact(rig: &Rig, queued: u32) {
    let refunds = rig.node.owner().refunds();
    assert!(refunds
        .iter()
        .any(|r| r.id == queued && r.stage == Stage::Queued));
    assert!(refunds
        .iter()
        .any(|r| matches!(r.stage, Stage::Paid { .. })));
    assert_eq!(rig.node.paid_log().len(), 1);
    assert_eq!(rig.node.owner().outbox().len(), 1);
}

#[test]
fn ss_41_n13f_03_the_move_stored_in_the_queue_and_confirmed_the_repin_and_a_restart_start_the_node()
{
    let dir = Dir::new();
    let mut retired = retiring(&dir);
    // The re-pin: the record is stored, then the queue bound to the new
    // wallet, then (the app's step) the new wallet's file.
    assert!(retired.rig.node.starter_queue_may_move());
    retired.rig.node.rebind_queue(single()).unwrap();
    retired.rig.node.mark_repinned(single());
    let chain = retired.rig.chain.clone();
    let queued = retired.queued;
    drop(retired.rig);
    rotate_view(&dir);
    assert!(queue_on_disk(&dir, single()).is_ok());
    assert!(queue_on_disk(&dir, old_wallet()).is_err());
    // The node starts with the new wallet, with the refunds, the paid log and
    // the move in the outbox intact.
    let rig = start_at(&dir, &former_setup(), chain).unwrap();
    assert_queue_intact(&rig, queued);
}

#[test]
fn ss_41_n13f_03_a_crash_after_the_record_and_before_the_queue_is_finished_by_the_start() {
    let dir = Dir::new();
    let retired = retiring(&dir);
    let chain = retired.rig.chain.clone();
    let queued = retired.queued;
    // The queue on disk is still the starter wallet's.
    drop(retired.rig);
    rotate_view(&dir);
    assert!(queue_on_disk(&dir, single()).is_err());
    assert!(queue_on_disk(&dir, old_wallet()).is_ok());
    let rig = start_at(&dir, &former_setup(), chain.clone()).unwrap();
    assert_queue_intact(&rig, queued);
    // The start stored the queue for the new wallet, and a second start
    // reads it as it is.
    assert!(queue_on_disk(&dir, single()).is_ok());
    drop(rig);
    let rig = start_at(&dir, &former_setup(), chain).unwrap();
    assert_queue_intact(&rig, queued);
}

#[test]
fn ss_41_n13f_03_a_crash_after_the_queue_and_before_the_wallet_file_starts_with_the_new_wallet() {
    let dir = Dir::new();
    let mut retired = retiring(&dir);
    retired.rig.node.rebind_queue(single()).unwrap();
    let chain = retired.rig.chain.clone();
    let queued = retired.queued;
    drop(retired.rig);
    rotate_view(&dir);
    // Two starts in a row, each with the new wallet.
    for _ in 0..2 {
        let rig = start_at(&dir, &former_setup(), chain.clone()).unwrap();
        assert!(queue_on_disk(&dir, single()).is_ok());
        assert_queue_intact(&rig, queued);
    }
}

#[test]
fn ss_41_n13f_03_the_repin_waits_while_another_starter_transaction_is_unconfirmed() {
    let dir = Dir::new();
    let mut retired = retiring(&dir);
    // A refund's transaction is handed to the outbox and no block holds it.
    let third = queue_refund_to(&mut retired.rig, 11);
    let coin = tx_from(
        OutPoint::new(Txid::from_byte_array([9; 32]), 0),
        &[(&old_script(2), 100_000)],
    );
    retired.rig.mine(std::slice::from_ref(&coin));
    let pay = tx_from(
        OutPoint::new(coin.compute_txid(), 0),
        &[(&external(11), 10_000)],
    );
    retired
        .rig
        .node
        .take_starter(&serialize(&pay), &[third])
        .unwrap();
    assert!(!retired.rig.node.starter_queue_may_move());
    let before = std::fs::read(dir.path().join(QUEUE_FILE)).unwrap();
    assert_eq!(
        retired.rig.node.rebind_queue(single()).err(),
        Some(RebindError::Refused(StarterRefusal::Unsettled))
    );
    // The queue is as it was, still the starter wallet's.
    assert_eq!(std::fs::read(dir.path().join(QUEUE_FILE)).unwrap(), before);
    assert!(queue_on_disk(&dir, old_wallet()).is_ok());
    // A block holds it, and then its depth: the queue may move.
    retired.rig.mine(std::slice::from_ref(&pay));
    assert!(!retired.rig.node.starter_queue_may_move());
    retired.rig.mine_empty(5);
    assert!(retired.rig.node.starter_queue_may_move());
    retired.rig.node.rebind_queue(single()).unwrap();
    assert!(queue_on_disk(&dir, single()).is_ok());
}

#[test]
fn ss_41_n13f_03_a_queue_of_another_wallet_is_still_refused_at_start() {
    let dir = Dir::new();
    let retired = retiring(&dir);
    let chain = retired.rig.chain.clone();
    drop(retired.rig);
    rotate_view(&dir);
    // A queue stored for a wallet that is neither the one replaced nor the
    // one in force.
    let foreign = Owner::new(
        &OWNER_SECRET,
        kit().wallet.clone(),
        own_approver(&OWNER_SECRET).unwrap(),
        policy(),
        openbmt_owner::Options::default(),
    )
    .unwrap();
    std::fs::write(dir.path().join(QUEUE_FILE), foreign.snapshot().unwrap()).unwrap();
    let error = start_at(&dir, &former_setup(), chain).err().unwrap();
    assert!(error.to_string().contains("another wallet"), "{error}");
}
