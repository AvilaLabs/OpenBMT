//! FL-05, FL-20, FL-21, FL-35: the owner node watches the refund float as a
//! wallet of its own, with its own coin view, and takes a float payment into
//! its outbox only for the float's own coins and ready refunds.
//!
//! TEST ONLY, NEVER USE: the key comes from fixed bytes.

use openbmt_core::wallet::Keychain;

use super::former::old_wallet;
use super::stopped::replace_tip;
use super::*;
use crate::FloatParts;

fn float_script(chain: Keychain, index: u32) -> ScriptBuf {
    match chain {
        Keychain::Receive => old_wallet().receive_script(index).unwrap(),
        Keychain::Change => old_wallet().change_script(index).unwrap(),
    }
}

fn float_setup() -> Setup {
    Setup {
        refund_depth: 1,
        float: Some(FloatParts {
            wallet: old_wallet(),
            receive_end: 1,
            change_end: 1,
            birthday: 0,
        }),
        ..Setup::default()
    }
}

fn fund(rig: &mut Rig, from: u8, script: &ScriptBuf, sats: u64) -> Transaction {
    let tx = tx_from(
        OutPoint::new(Txid::from_byte_array([from; 32]), 0),
        &[(script, sats)],
    );
    rig.mine(std::slice::from_ref(&tx));
    rig.mine_empty(2);
    tx
}

/// A refund of `sats` that is ready: its sale's payment is at the depth.
fn ready_refund(rig: &mut Rig, sats: u64) -> u32 {
    let refund = rig.queue_refund(sats);
    let paid = tx_from(
        OutPoint::new(Txid::from_byte_array([8; 32]), 0),
        &[(&receive(1), 50_000)],
    );
    rig.mine(std::slice::from_ref(&paid));
    assert!(rig.node.owner().refund_ready(refund));
    refund
}

#[test]
fn fl_05_a_payment_to_a_float_address_is_the_floats_own_coin_and_no_total_of_the_shops_wallet() {
    let dir = Dir::new();
    let mut rig = start(&dir, &float_setup()).unwrap();
    // The main driver was given the float's scripts: both chains, 1 + 20.
    assert!(rig.spawned.lock().unwrap()[0].scripts >= 2 * 21);
    assert!(rig.node.float_coins().unwrap().coins.is_empty());
    fund(&mut rig, 7, &float_script(Keychain::Receive, 3), 40_000);
    fund(&mut rig, 8, &float_script(Keychain::Change, 5), 5_000);
    fund(&mut rig, 9, &receive(0), 9_000);
    let held = rig.node.float_coins().unwrap();
    assert_eq!(held.coins.len(), 2);
    assert_eq!(held.confirmed, 45_000);
    assert_eq!(held.unconfirmed_incoming, 0);
    // The shop's wallet has only its own coin, and no late-payment total of
    // a former wallet holds the float's.
    assert_eq!(rig.node.coins().len(), 1);
    assert!(rig.node.former_held().is_empty());
    // Past the highest index handed out and the gap it is not watched.
    fund(&mut rig, 10, &float_script(Keychain::Receive, 40), 7_000);
    assert_eq!(rig.node.float_coins().unwrap().confirmed, 45_000);
    // A restart keeps what the float's own view found.
    let chain = rig.chain.clone();
    drop(rig);
    let rig = start_at(&dir, &float_setup(), chain).unwrap();
    assert_eq!(rig.node.float_coins().unwrap().confirmed, 45_000);
}

#[test]
fn fl_05_raising_the_watch_finds_a_payment_past_the_old_end_without_a_restart() {
    let dir = Dir::new();
    let mut rig = start(&dir, &float_setup()).unwrap();
    // End 1: the watch goes to 21. A payment to index 40 is not seen, before
    // or after the raise.
    fund(&mut rig, 7, &float_script(Keychain::Receive, 3), 40_000);
    fund(&mut rig, 10, &float_script(Keychain::Receive, 40), 7_000);
    assert_eq!(rig.node.float_coins().unwrap().confirmed, 40_000);
    let before = rig.calls();
    let spawned = rig.spawned.lock().unwrap().len();
    // The app handed out up to index 30: the end is 31, the watch 51.
    rig.node.raise_float_watch(31, 1).unwrap();
    let new = rig.calls()[before.len()..].to_vec();
    // The 30 receive scripts that are new, and the float's own birthday read again.
    assert_eq!(new.iter().filter(|c| *c == "main watch").count(), 30);
    assert_eq!(new.iter().filter(|c| *c == "main rescan 0").count(), 1);
    assert!(
        rig.node.float_coins().is_err(),
        "the coins wait for the rescan"
    );
    rig.rescan();
    let held = rig.node.float_coins().unwrap();
    // The payment mined before the raise is found by the rescan.
    assert_eq!(held.confirmed, 47_000);
    assert_eq!(held.receive_next, 31);
    // And one mined after it, to the new end, with no restart.
    fund(&mut rig, 11, &float_script(Keychain::Receive, 50), 3_000);
    assert_eq!(rig.node.float_coins().unwrap().confirmed, 50_000);
    assert_eq!(
        rig.spawned.lock().unwrap().len(),
        spawned,
        "no driver was started"
    );
    // Past the new end it is still not watched.
    fund(&mut rig, 12, &float_script(Keychain::Receive, 52), 1_000);
    assert_eq!(rig.node.float_coins().unwrap().confirmed, 50_000);
    // The shop's wallet is untouched by all of it.
    assert!(rig.node.coins().is_empty());
    assert!(rig.node.former_held().is_empty());
    // The wider watch is what a restart reads back.
    let chain = rig.chain.clone();
    drop(rig);
    let rig = start_at(
        &dir,
        &Setup {
            float: Some(FloatParts {
                receive_end: 31,
                ..float_setup().float.unwrap()
            }),
            ..float_setup()
        },
        chain,
    )
    .unwrap();
    assert_eq!(rig.node.float_coins().unwrap().confirmed, 50_000);
}

#[test]
fn fl_05_raising_the_change_end_watches_further_change_and_a_lower_end_changes_nothing() {
    let dir = Dir::new();
    let mut rig = start(&dir, &float_setup()).unwrap();
    fund(&mut rig, 8, &float_script(Keychain::Change, 25), 5_000);
    assert_eq!(rig.node.float_coins().unwrap().confirmed, 0);
    // Not past what is watched: nothing is said and nothing is rescanned.
    let before = rig.calls();
    rig.node.raise_float_watch(1, 1).unwrap();
    rig.node.raise_float_watch(0, 0).unwrap();
    assert_eq!(rig.calls(), before);
    rig.node.raise_float_watch(1, 10).unwrap();
    rig.rescan();
    let held = rig.node.float_coins().unwrap();
    assert_eq!(held.confirmed, 5_000);
    assert_eq!(held.change_next, 10);
    // The receive end did not go down.
    assert_eq!(held.receive_next, 1);
}

#[test]
fn fl_05_a_node_with_no_float_cannot_raise_its_watch() {
    let dir = Dir::new();
    let mut rig = start(&dir, &Setup::default()).unwrap();
    assert!(matches!(
        rig.node.raise_float_watch(5, 5),
        Err(NodeError::NotReady(_))
    ));
}

#[test]
fn fl_05_a_node_with_no_float_says_so() {
    let dir = Dir::new();
    let rig = start(&dir, &Setup::default()).unwrap();
    assert!(rig.node.float_coins().is_err());
}

#[test]
fn fl_20_a_ready_refund_is_paid_from_the_floats_coin_through_the_outbox_and_settled() {
    let dir = Dir::new();
    let mut rig = start(&dir, &float_setup()).unwrap();
    let coin = fund(&mut rig, 7, &float_script(Keychain::Receive, 0), 100_000);
    let refund = ready_refund(&mut rig, 10_000);
    let pay = tx_from(
        OutPoint::new(coin.compute_txid(), 0),
        &[
            (&external(9), 10_000),
            (&float_script(Keychain::Change, 0), 89_000),
        ],
    );
    let txid = rig
        .node
        .take_float(&serialize(&pay), &[refund], None)
        .unwrap();
    assert_eq!(txid, pay.compute_txid());
    assert!(rig.calls().contains(&format!("main broadcast {txid}")));
    assert_eq!(rig.node.starter_confirmations(&txid), Some(0));
    assert!(
        matches!(refund_stage(&rig), Stage::Sent { .. }),
        "{:?}",
        refund_stage(&rig)
    );
    // The change is the float's, at no confirmation yet; the spent coin is not offered.
    let held = rig.node.float_coins().unwrap();
    assert_eq!(held.coins.len(), 1);
    assert!(rig.node.coins().is_empty() || rig.node.coins().len() == 1);
    // A block holds it; the refund is paid by it.
    rig.mine(std::slice::from_ref(&pay));
    rig.mine_empty(2);
    assert_eq!(rig.node.starter_confirmations(&txid), Some(3));
    assert!(rig.node.owner().outbox().is_empty());
    assert!(
        matches!(refund_stage(&rig), Stage::Paid { .. }),
        "{:?}",
        refund_stage(&rig)
    );
    assert_eq!(rig.node.float_coins().unwrap().confirmed, 89_000);
}

#[test]
fn fl_21_the_node_refuses_a_float_payment_that_spends_the_shops_coin_or_pays_a_refund_not_ready() {
    let dir = Dir::new();
    let mut rig = start(&dir, &float_setup()).unwrap();
    let shop_coin = fund(&mut rig, 7, &receive(0), 100_000);
    let float_coin = fund(&mut rig, 8, &float_script(Keychain::Receive, 0), 100_000);
    let refund = ready_refund(&mut rig, 10_000);
    // The shop's own coin is not the float's.
    let spends_shop = tx_from(
        OutPoint::new(shop_coin.compute_txid(), 0),
        &[
            (&external(9), 10_000),
            (&float_script(Keychain::Change, 0), 89_000),
        ],
    );
    assert!(rig
        .node
        .take_float(&serialize(&spends_shop), &[refund], None)
        .is_err());
    // A payment of the float to someone besides the refund.
    let to_other = tx_from(
        OutPoint::new(float_coin.compute_txid(), 0),
        &[
            (&external(77), 10_000),
            (&float_script(Keychain::Change, 0), 89_000),
        ],
    );
    assert!(rig
        .node
        .take_float(&serialize(&to_other), &[refund], None)
        .is_err());
    // A refund whose payment left the chain is not ready, so it is not paid.
    let ok = tx_from(
        OutPoint::new(float_coin.compute_txid(), 0),
        &[
            (&external(9), 10_000),
            (&float_script(Keychain::Change, 0), 89_000),
        ],
    );
    replace_tip(&mut rig, &[]);
    assert!(!rig.node.owner().refund_ready(refund));
    assert!(rig
        .node
        .take_float(&serialize(&ok), &[refund], None)
        .is_err());
    assert!(rig.node.owner().outbox().is_empty());
    assert_eq!(refund_stage(&rig), Stage::Queued);
}

#[test]
fn fl_35_the_move_goes_through_the_outbox_to_the_one_address_with_no_refund() {
    let dir = Dir::new();
    let mut rig = start(&dir, &float_setup()).unwrap();
    let a = fund(&mut rig, 7, &float_script(Keychain::Receive, 0), 60_000);
    let shop = receive(500);
    let mv = tx_from(OutPoint::new(a.compute_txid(), 0), &[(&shop, 59_000)]);
    // Another address is refused; the planned one is taken.
    let wrong = tx_from(
        OutPoint::new(a.compute_txid(), 0),
        &[(&external(5), 59_000)],
    );
    assert!(rig
        .node
        .take_float(&serialize(&wrong), &[], Some(&shop))
        .is_err());
    let txid = rig
        .node
        .take_float(&serialize(&mv), &[], Some(&shop))
        .unwrap();
    assert!(rig.calls().contains(&format!("main broadcast {txid}")));
    rig.mine(std::slice::from_ref(&mv));
    assert_eq!(rig.node.starter_confirmations(&txid), Some(1));
    assert!(rig.node.float_coins().unwrap().coins.is_empty());
}
