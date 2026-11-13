//! A reorganization while the node was stopped, a payment finished on a
//! hardware signer and what it logs (`docs/OWNER-COINS-RULES.md` OC-49,
//! OC-53, OC-54), on a driver that the test feeds by hand. The headers a
//! background watch saved are written into the directory between two starts.

use openbmt_owner::{CashAmount, RateNote};
use openbmt_watch::HeaderChain;

use super::*;
use crate::facts::verified_facts;
use crate::paid_log::{PaidValue, PaidWhat};
use crate::settle_owed::{Owed, OwedKind, SettleOwed};

fn genesis_hash() -> BlockHash {
    bitcoin::blockdata::constants::genesis_block(NET).block_hash()
}

/// The first `keep` blocks of `old`, then `extra` more that are not the ones
/// `old` had, as a chain that a background watch saved.
fn fork_chain(old: &[Block], keep: usize, extra: usize, tag: u32) -> Vec<Block> {
    let mut chain: Vec<Block> = old[..keep].to_vec();
    for _ in 0..extra {
        let prev = chain.last().map_or_else(genesis_hash, Block::block_hash);
        let height = u32::try_from(chain.len()).unwrap() + 1;
        chain.push(mine(prev, height, tag, &[]));
    }
    chain
}

/// Queue a batch, so that the sales at its indices have a counter.
fn queue_batch(rig: &mut Rig) {
    let write = rig.node.queue_writer();
    rig.node
        .owner_mut()
        .unwrap()
        .queue_batch(batch(0, 3), 1_000, &write)
        .unwrap();
    rig.node.refresh_marks();
}

/// What the owner phone does to queue a refund: the facts from the view, then
/// the refund.
fn try_refund(rig: &mut Rig, sats: u64) -> Result<u32, String> {
    let tip = rig.node.watcher().chain().tip_height();
    let facts = verified_facts(
        rig.node.view(),
        rig.node.owner(),
        sale(1),
        tip,
        rig.node.refund_depth(),
    )
    .map_err(|e| e.to_string())?;
    let write = rig.node.queue_writer();
    let owner = rig.node.owner_mut().unwrap();
    owner
        .set_sale_facts(sale(1), facts, &write)
        .map_err(|e| e.to_string())?;
    owner
        .queue_refund(sale(1), "Mia", external(9), sats, None, &write)
        .map_err(|e| e.to_string())
}

fn price(minor_per_btc: u64) -> Option<RateNote> {
    Some(RateNote {
        currency: *b"USD",
        minor_per_btc,
    })
}

#[test]
fn oc_53_the_view_keeps_the_hash_of_the_block_it_last_checked() {
    let dir = Dir::new();
    let mut rig = start(&dir, &Setup::default()).unwrap();
    rig.mine_empty(3);
    assert_eq!(rig.node.view().checked(), 3);
    assert_eq!(rig.node.view().checked_block(), Some(rig.tip_hash()));
    let chain = rig.chain.clone();
    rig.node.shutdown();
    drop(rig);
    let bytes = fs::read(dir.path().join(COINS_FILE)).unwrap();
    assert_eq!(bytes[4], 2);
    let read = CoinView::from_bytes(&bytes).unwrap();
    assert_eq!(read.checked_block(), Some(chain[2].block_hash()));
}

#[test]
fn oc_53_n13_01_a_payment_a_reorganization_dropped_while_stopped_is_gone_at_start() {
    let dir = Dir::new();
    let mut rig = start(&dir, &Setup::default()).unwrap();
    queue_batch(&mut rig);
    rig.fund(300_000);
    let tip = rig.node.watcher().chain().tip_height();
    let facts = verified_facts(
        rig.node.view(),
        rig.node.owner(),
        sale(1),
        tip,
        rig.node.refund_depth(),
    )
    .unwrap();
    assert_eq!(facts.received_sats, 50_000);
    assert!(facts.reached_refund_depth);
    assert_eq!(rig.node.view().owned().count(), 2);
    let old = rig.chain.clone();
    rig.node.shutdown();
    drop(rig);

    // The background watch saved a chain without the payment that has grown
    // past the position of the view.
    let new = fork_chain(&old, 0, 8, 7);
    save_headers(&dir, &new);
    let mut rig = start_at(&dir, &Setup::default(), new).unwrap();
    assert_eq!(rig.node.view().owned().count(), 0);
    assert!(rig.node.view().ready());
    assert_eq!(rig.node.view().checked(), 8);
    assert_eq!(rig.node.view().checked_block(), Some(rig.tip_hash()));
    let notes = rig.node.watcher_mut().take_notes();
    assert!(
        notes
            .iter()
            .any(|n| n.contains("while the node was stopped")),
        "{notes:?}"
    );
    // The facts a refund would be taken from no longer count the payment,
    // and a refund against it is refused.
    let tip = rig.node.watcher().chain().tip_height();
    let facts = verified_facts(
        rig.node.view(),
        rig.node.owner(),
        sale(1),
        tip,
        rig.node.refund_depth(),
    )
    .unwrap();
    assert_eq!(facts.received_sats, 0);
    assert!(!facts.reached_refund_depth);
    assert!(try_refund(&mut rig, 10_000).is_err());
}

#[test]
fn oc_53_n13_01_a_queued_refunds_facts_stop_counting_the_dropped_payment_at_start() {
    let dir = Dir::new();
    let mut rig = start(&dir, &Setup::default()).unwrap();
    queue_batch(&mut rig);
    rig.fund(300_000);
    try_refund(&mut rig, 10_000).unwrap();
    let held = rig.node.owner().sale_facts(&sale(1)).unwrap();
    assert!(held.reached_refund_depth);
    let old = rig.chain.clone();
    rig.node.shutdown();
    drop(rig);
    let new = fork_chain(&old, 0, 8, 7);
    let rig = start_at(&dir, &Setup::default(), new).unwrap();
    let held = rig.node.owner().sale_facts(&sale(1)).unwrap();
    assert!(!held.reached_refund_depth);
}

#[test]
fn oc_53_a_reorganization_that_keeps_the_blocks_below_it_undoes_only_what_left() {
    let dir = Dir::new();
    let mut rig = start(&dir, &Setup::default()).unwrap();
    queue_batch(&mut rig);
    rig.fund(300_000);
    let later = tx_from(
        OutPoint::new(Txid::from_byte_array([8; 32]), 0),
        &[(&receive(2), 20_000)],
    );
    rig.mine(std::slice::from_ref(&later));
    rig.mine_empty(2);
    assert_eq!(rig.node.view().owned().count(), 3);
    let old = rig.chain.clone();
    rig.node.shutdown();
    drop(rig);
    // The first six blocks stay; block 7 and what follows are others.
    let new = fork_chain(&old, 6, 5, 7);
    let rig = start_at(&dir, &Setup::default(), new).unwrap();
    assert_eq!(rig.node.view().owned().count(), 2);
    assert!(rig.node.view().ready());
    assert_eq!(rig.node.view().checked(), 11);
}

#[test]
fn oc_53_n13_01_the_view_of_a_former_wallet_is_undone_at_start_too() {
    let dir = Dir::new();
    let mut rig = start(&dir, &former::former_setup()).unwrap();
    former::pay(&mut rig, 7, &former::old_script(3), 40_000);
    assert_eq!(rig.node.former_held().len(), 1);
    let old = rig.chain.clone();
    rig.node.shutdown();
    drop(rig);
    let new = fork_chain(&old, 0, 6, 7);
    let rig = start_at(&dir, &former::former_setup(), new).unwrap();
    assert!(rig.node.former_held().is_empty());
}

#[test]
fn oc_53_a_reorganization_below_the_undo_horizon_at_start_is_the_deep_reorganization() {
    let dir = Dir::new();
    let mut rig = start(&dir, &Setup::default()).unwrap();
    queue_batch(&mut rig);
    rig.fund(300_000);
    rig.mine_empty(120);
    assert_eq!(rig.node.view().owned().count(), 2);
    let old = rig.chain.clone();
    rig.node.shutdown();
    drop(rig);
    let new = fork_chain(&old, 0, 125, 7);
    let mut rig = start_at(&dir, &Setup::default(), new).unwrap();
    assert_eq!(rig.node.view().state(), ViewState::DeepReorg);
    let notes = rig.node.watcher_mut().take_notes();
    assert!(
        notes.iter().any(|n| n.contains("deep reorg: rescan")),
        "{notes:?}"
    );
    // Nothing is given from a view that must be rescanned.
    assert!(try_refund(&mut rig, 10_000).is_err());
}

#[test]
fn oc_53_a_view_that_passed_the_check_is_left_as_it_is() {
    let dir = Dir::new();
    let mut rig = start(&dir, &Setup::default()).unwrap();
    queue_batch(&mut rig);
    rig.fund(300_000);
    let before = rig.node.view().clone();
    let chain = rig.chain.clone();
    rig.node.shutdown();
    drop(rig);
    let rig = start_at(&dir, &Setup::default(), chain).unwrap();
    assert_eq!(rig.node.view(), &before);
}

#[test]
fn oc_53_an_owed_settlement_whose_block_left_while_stopped_is_not_applied() {
    let dir = Dir::new();
    let mut rig = start(&dir, &Setup::default()).unwrap();
    let (tx, _, session) = exported_open(&mut rig);
    let blocked = block_writes(&dir, crate::QUEUE_FILE);
    rig.mine(std::slice::from_ref(&tx));
    assert_eq!(owed_on_disk(&dir).items().len(), 1);
    let old = rig.chain.clone();
    rig.node.shutdown();
    drop(rig);
    fs::remove_dir(blocked).unwrap();
    // The block with the payment is replaced by another at the same height.
    let new = fork_chain(&old, old.len() - 1, 1, 7);
    let rig = start_at(&dir, &Setup::default(), new).unwrap();
    assert!(rig.node.owner().open_sessions().contains(&session));
    assert!(matches!(refund_stage(&rig), Stage::InSession { .. }));
    assert!(owed_on_disk(&dir).is_empty());
}

#[test]
fn oc_53_an_owed_settlement_from_a_block_off_the_chain_is_dropped_at_any_tip_height() {
    let dir = Dir::new();
    let mut rig = start(&dir, &Setup::default()).unwrap();
    let (tx, _, session) = exported_open(&mut rig);
    let txid = tx.compute_txid();
    let chain = rig.chain.clone();
    rig.node.shutdown();
    drop(rig);
    // A journal that names a block the chain does not hold, with the chain
    // no higher than the view's position.
    let stale = SettleOwed::new()
        .merged(&[Owed {
            kind: OwedKind::ExportedPaid,
            txid,
            block: BlockHash::from_byte_array([9; 32]),
            tx: None,
        }])
        .unwrap();
    fs::write(dir.path().join(crate::SETTLE_OWED_FILE), stale.to_bytes()).unwrap();
    let mut rig = start_at(&dir, &Setup::default(), chain).unwrap();
    assert_eq!(rig.node.view().checked(), 6);
    assert!(rig.node.paid_log().is_empty());
    assert!(rig.node.owner().open_sessions().contains(&session));
    assert!(owed_on_disk(&dir).is_empty());
    let notes = rig.node.watcher_mut().take_notes();
    assert!(
        notes.iter().any(|n| n.contains("not on the best chain")),
        "{notes:?}"
    );
}

/// The block of the transaction `tx` is replaced by `with` (or by nothing)
/// as the driver tells it: the chain event, the new block and its check.
fn replace_tip(rig: &mut Rig, with: &[Transaction]) -> Block {
    let left = rig.chain.pop().unwrap();
    let height = u32::try_from(rig.chain.len()).unwrap() + 1;
    let other = mine(rig.tip_hash(), height, 7, with);
    rig.chain.push(other.clone());
    rig.feed(vec![
        AppEvent::Chain {
            joined: vec![(height, other.block_hash())],
            left: vec![left.block_hash()],
        },
        AppEvent::Block {
            hash: other.block_hash(),
            bytes: serialize(&other),
        },
        AppEvent::Checked(height),
    ]);
    other
}

fn broadcasts(rig: &Rig, txid: &Txid) -> usize {
    let line = format!("main broadcast {txid}");
    rig.calls().iter().filter(|c| **c == line).count()
}

#[test]
fn oc_54_n13_02_an_exported_payment_dropped_by_a_reorganization_is_followed_and_sent_again() {
    let dir = Dir::new();
    let mut rig = start(&dir, &Setup::default()).unwrap();
    let (tx, _, session) = exported_open(&mut rig);
    let txid = tx.compute_txid();
    let block = rig.mine(std::slice::from_ref(&tx));
    assert_eq!(refund_stage(&rig), Stage::Paid { txid });
    assert!(!rig.node.owner().open_sessions().contains(&session));
    let height = u32::try_from(rig.chain.len()).unwrap();
    // The transaction is kept with the block that confirmed it, and its paid
    // rows cannot be archived while it is.
    assert_eq!(rig.node.view().confirmed_height(&txid), Some(height));
    assert!(rig.node.view().retained_confirmations().contains(&txid));
    assert_eq!(rig.node.archive_paid().unwrap(), 0);
    assert_eq!(broadcasts(&rig, &txid), 0);

    // The block leaves the chain and the transaction is in no other.
    replace_tip(&mut rig, &[]);
    assert_eq!(refund_stage(&rig), Stage::Sent { txid });
    assert_eq!(broadcasts(&rig, &txid), 1);
    assert!(rig.node.view().confirmed_height(&txid).is_none());
    assert!(rig
        .node
        .view()
        .pending()
        .any(|p| p.tx.compute_txid() == txid));
    // The same bytes are in the outbox, and nothing was signed.
    let sent: Vec<Vec<u8>> = rig
        .node
        .owner()
        .outbox()
        .iter()
        .filter_map(|d| match &d.what {
            Delivery::Transaction { tx: bytes, .. } => Some(bytes.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(sent, vec![serialize(&tx)]);
    assert_eq!(serialize(&block.txdata[1]), serialize(&tx));

    // A later block holds it again: it is settled again.
    rig.mine(std::slice::from_ref(&tx));
    rig.mine_empty(5);
    assert_eq!(refund_stage(&rig), Stage::Paid { txid });
    assert!(rig.node.owner().outbox().is_empty());
}

#[test]
fn oc_54_n13_02_an_exported_payment_beaten_by_the_reorganization_returns_to_the_queue() {
    let dir = Dir::new();
    let mut rig = start(&dir, &Setup::default()).unwrap();
    let (tx, _, _) = exported_open(&mut rig);
    let txid = tx.compute_txid();
    rig.mine(std::slice::from_ref(&tx));
    assert_eq!(refund_stage(&rig), Stage::Paid { txid });
    // The branch that replaces the block spends a coin of the transaction in
    // another one.
    let other = tx_from(tx.input[0].previous_output, &[(&external(77), 1_000)]);
    replace_tip(&mut rig, &[other]);
    assert_eq!(broadcasts(&rig, &txid), 1);
    assert!(matches!(refund_stage(&rig), Stage::Returned { .. }));
    assert!(rig.node.owner().outbox().is_empty());
    assert!(rig
        .node
        .view()
        .pending()
        .all(|p| p.tx.compute_txid() != txid));
}

#[test]
fn oc_54_n13_02_a_payment_signed_elsewhere_dropped_by_a_reorganization_is_followed_too() {
    let dir = Dir::new();
    let mut rig = start(&dir, &Setup::default()).unwrap();
    let (tx, _) = exported_then_abandoned(&mut rig);
    let txid = tx.compute_txid();
    rig.mine(std::slice::from_ref(&tx));
    assert!(rig.node.owner().signed_elsewhere().is_empty());
    assert_eq!(refund_stage(&rig), Stage::Paid { txid });
    assert!(rig.node.view().retained_confirmations().contains(&txid));
    replace_tip(&mut rig, &[]);
    assert_eq!(refund_stage(&rig), Stage::Sent { txid });
    assert_eq!(broadcasts(&rig, &txid), 1);
    rig.mine(std::slice::from_ref(&tx));
    rig.mine_empty(5);
    assert_eq!(refund_stage(&rig), Stage::Paid { txid });
}

#[test]
fn oc_54_a_settlement_that_is_owed_across_a_restart_keeps_its_transaction() {
    let dir = Dir::new();
    let mut rig = start(&dir, &Setup::default()).unwrap();
    let (tx, _, _) = exported_open(&mut rig);
    let txid = tx.compute_txid();
    let blocked = block_writes(&dir, crate::QUEUE_FILE);
    rig.mine(std::slice::from_ref(&tx));
    assert_eq!(owed_on_disk(&dir).items()[0].tx.as_ref(), Some(&tx));
    let chain = rig.chain.clone();
    rig.node.shutdown();
    drop(rig);
    fs::remove_dir(blocked).unwrap();
    let rig = start_at(&dir, &Setup::default(), chain).unwrap();
    assert_eq!(refund_stage(&rig), Stage::Paid { txid });
    assert!(rig.node.view().retained_confirmations().contains(&txid));
    assert!(owed_on_disk(&dir).is_empty());
}

/// A cash-out of everything to `external(8)`, with a price, built and
/// exported for the signer.
fn exported_cash_out(rig: &mut Rig, minor_per_btc: u64) -> (Transaction, u32, [u8; 32]) {
    queue_batch(rig);
    let write = rig.node.queue_writer();
    let cash = rig
        .node
        .owner_mut()
        .unwrap()
        .queue_cash_out_priced(
            external(8),
            CashAmount::All,
            "Bank",
            price(minor_per_btc),
            &write,
        )
        .unwrap();
    // Everything held is under the ceiling for other destinations.
    rig.fund(40_000);
    let (_, session) = rig.node.build_request_for_signer(1_000, NOW).unwrap();
    let session = session.expect("the cash-out is a spend for the signer");
    let mut signer = key1();
    let file = rig
        .node
        .export_for_signer(
            &session,
            0,
            &mut signer,
            &kit().accounts[0],
            NOW,
            |_| Ok(()),
        )
        .unwrap();
    let tx = bitcoin::psbt::Psbt::deserialize(&file.psbt)
        .unwrap()
        .unsigned_tx;
    (tx, cash, session)
}

fn paid_to_the_bank(tx: &Transaction) -> u64 {
    tx.output
        .iter()
        .filter(|o| o.script_pubkey == external(8))
        .map(|o| o.value.to_sat())
        .sum()
}

#[test]
fn oc_49_n13_03_a_cash_out_of_everything_finished_on_a_signer_is_logged_at_what_it_paid() {
    let dir = Dir::new();
    let mut rig = start(&dir, &Setup::default()).unwrap();
    let (tx, cash, _) = exported_cash_out(&mut rig, 9_000_000);
    let paid = paid_to_the_bank(&tx);
    assert!(paid > 0);
    rig.mine(std::slice::from_ref(&tx));
    let entry = rig
        .node
        .paid_log()
        .iter()
        .find(|e| e.what == PaidWhat::CashOut)
        .copied()
        .unwrap();
    assert_eq!(entry.number, cash);
    assert_eq!(entry.sats, paid);
    assert_eq!(
        entry.value,
        Some(PaidValue {
            currency: *b"USD",
            minor: paid * 9_000_000 / 100_000_000,
        })
    );
}

#[test]
fn oc_49_n13_03_the_same_when_the_payment_was_signed_elsewhere() {
    let dir = Dir::new();
    let mut rig = start(&dir, &Setup::default()).unwrap();
    let (tx, cash, session) = exported_cash_out(&mut rig, 9_000_000);
    let write = rig.node.queue_writer();
    rig.node
        .owner_mut()
        .unwrap()
        .abandon_session(&session, &write)
        .unwrap();
    let paid = paid_to_the_bank(&tx);
    rig.mine(std::slice::from_ref(&tx));
    let entry = rig
        .node
        .paid_log()
        .iter()
        .find(|e| e.what == PaidWhat::CashOut)
        .copied()
        .unwrap();
    assert_eq!(entry.number, cash);
    assert_eq!(entry.sats, paid);
    assert!(entry.sats > 0);
    assert_eq!(entry.value.map(|v| v.minor), Some(paid * 9 / 100));
}

#[test]
fn oc_49_a_cash_out_of_everything_with_no_figure_is_not_logged_at_zero() {
    let dir = Dir::new();
    let mut rig = start(&dir, &Setup::default()).unwrap();
    let (tx, _, _) = exported_cash_out(&mut rig, 9_000_000);
    // A journal written before it carried the transaction.
    let txid = tx.compute_txid();
    let blocked = block_writes(&dir, crate::PAID_LOG_FILE);
    rig.mine(std::slice::from_ref(&tx));
    let owed = owed_on_disk(&dir);
    let item = owed.items()[0].clone();
    let bare = SettleOwed::new()
        .merged(&[Owed { tx: None, ..item }])
        .unwrap();
    let chain = rig.chain.clone();
    rig.node.shutdown();
    drop(rig);
    fs::remove_dir(blocked).unwrap();
    fs::write(dir.path().join(crate::SETTLE_OWED_FILE), bare.to_bytes()).unwrap();
    let mut rig = start_at(&dir, &Setup::default(), chain).unwrap();
    assert!(rig
        .node
        .paid_log()
        .iter()
        .all(|e| e.what != PaidWhat::CashOut));
    assert!(rig
        .node
        .owner()
        .cash_outs()
        .iter()
        .all(|c| c.stage == Stage::Paid { txid }));
    let notes = rig.node.watcher_mut().take_notes();
    assert!(
        notes.iter().any(|n| n.contains("amount is not known")),
        "{notes:?}"
    );
}

/// A refund that was built, signed and sent by the owner's own outbox, with
/// a block that spends one of its coins in another transaction.
fn sent_and_beaten(rig: &mut Rig) -> (Transaction, Transaction) {
    rig.queue_refund(10_000);
    rig.fund(300_000);
    let txid = rig.spend();
    let tx = rig.tx(&txid);
    let other = tx_from(tx.input[0].previous_output, &[(&external(77), 1_000)]);
    (tx, other)
}

#[test]
fn oc_52_n13b_01_a_beaten_outbox_payment_is_settled_at_the_next_block_after_a_failed_write() {
    let dir = Dir::new();
    let mut rig = start(&dir, &Setup::default()).unwrap();
    let (tx, other) = sent_and_beaten(&mut rig);
    let txid = tx.compute_txid();
    let blocked = block_writes(&dir, crate::QUEUE_FILE);
    rig.mine(&[other]);
    // The queue could not be written: the row is as it was, and the block's
    // part is owed.
    assert_eq!(refund_stage(&rig), Stage::Sent { txid });
    let owed = owed_on_disk(&dir);
    assert_eq!(owed.items().len(), 1);
    assert_eq!(owed.items()[0].kind, OwedKind::OutboxBeaten);
    assert_eq!(owed.items()[0].txid, txid);
    // Still failing at the next block.
    rig.mine_empty(1);
    assert_eq!(refund_stage(&rig), Stage::Sent { txid });
    fs::remove_dir(blocked).unwrap();
    rig.mine_empty(1);
    assert!(matches!(refund_stage(&rig), Stage::Returned { .. }));
    assert!(rig.node.owner().outbox().is_empty());
    assert!(owed_on_disk(&dir).is_empty());
}

#[test]
fn oc_52_n13b_01_a_beaten_outbox_payment_is_settled_at_start() {
    let dir = Dir::new();
    let mut rig = start(&dir, &Setup::default()).unwrap();
    let (tx, other) = sent_and_beaten(&mut rig);
    let txid = tx.compute_txid();
    let blocked = block_writes(&dir, crate::QUEUE_FILE);
    rig.mine(&[other]);
    assert_eq!(refund_stage(&rig), Stage::Sent { txid });
    assert_eq!(owed_on_disk(&dir).items().len(), 1);
    let chain = rig.chain.clone();
    rig.node.shutdown();
    drop(rig);
    fs::remove_dir(blocked).unwrap();
    let rig = start_at(&dir, &Setup::default(), chain).unwrap();
    assert!(matches!(refund_stage(&rig), Stage::Returned { .. }));
    assert!(rig.node.owner().outbox().is_empty());
    assert!(owed_on_disk(&dir).is_empty());
}

#[test]
fn oc_52_n13b_01_a_beaten_outbox_payment_waits_for_key_1_recovery_and_is_settled_after_it() {
    let dir = Dir::new();
    let mut rig = start(&dir, &Setup::default()).unwrap();
    let (tx, other) = sent_and_beaten(&mut rig);
    let txid = tx.compute_txid();
    let mut rig = restart_with_journal(&dir, rig);
    rig.mine(&[other]);
    assert_eq!(refund_stage(&rig), Stage::Sent { txid });
    assert_eq!(owed_on_disk(&dir).items().len(), 1);
    recover(&mut rig);
    assert!(matches!(refund_stage(&rig), Stage::Returned { .. }));
    assert!(rig.node.owner().outbox().is_empty());
    assert!(owed_on_disk(&dir).is_empty());
}

#[test]
fn oc_52_n13b_01_a_beating_block_that_leaves_before_settlement_drops_the_item() {
    let dir = Dir::new();
    let mut rig = start(&dir, &Setup::default()).unwrap();
    let (tx, other) = sent_and_beaten(&mut rig);
    let txid = tx.compute_txid();
    let blocked = block_writes(&dir, crate::QUEUE_FILE);
    rig.mine(&[other]);
    assert_eq!(owed_on_disk(&dir).items().len(), 1);
    fs::remove_dir(blocked).unwrap();
    replace_tip(&mut rig, &[]);
    assert!(owed_on_disk(&dir).is_empty());
    // Nothing was settled, and the view follows the transaction again.
    assert_eq!(refund_stage(&rig), Stage::Sent { txid });
    assert!(rig
        .node
        .view()
        .pending()
        .any(|p| p.tx.compute_txid() == txid));
    assert!(!rig.node.owner().outbox().is_empty());
}

/// A refund of 10,000 whose fee the customer pays, built and exported for the
/// signer.
fn exported_customer_fee_refund(rig: &mut Rig) -> (Transaction, u32, [u8; 32]) {
    let refund = rig.queue_refund(10_000);
    rig.fund(300_000);
    let (_, session) = rig.node.build_request_for_signer(1_000, NOW).unwrap();
    let session = session.expect("the refund is a spend for the signer");
    let mut signer = key1();
    let file = rig
        .node
        .export_for_signer(
            &session,
            0,
            &mut signer,
            &kit().accounts[0],
            NOW,
            |_| Ok(()),
        )
        .unwrap();
    let tx = bitcoin::psbt::Psbt::deserialize(&file.psbt)
        .unwrap()
        .unsigned_tx;
    (tx, refund, session)
}

fn customer_fee_setup() -> Setup {
    Setup {
        customer_fee: true,
        ..Setup::default()
    }
}

fn paid_to(tx: &Transaction, script: &ScriptBuf) -> u64 {
    tx.output
        .iter()
        .filter(|o| o.script_pubkey == *script)
        .map(|o| o.value.to_sat())
        .sum()
}

#[test]
fn oc_49_n13_03_a_customer_fee_refund_finished_on_a_signer_is_logged_at_what_the_customer_got() {
    let dir = Dir::new();
    let mut rig = start(&dir, &customer_fee_setup()).unwrap();
    let (tx, refund, _) = exported_customer_fee_refund(&mut rig);
    let got = paid_to(&tx, &external(9));
    assert!(got > 0 && got < 10_000, "{got}");
    rig.mine(std::slice::from_ref(&tx));
    let entry = rig
        .node
        .paid_log()
        .iter()
        .find(|e| e.number == refund)
        .copied()
        .unwrap();
    assert_eq!(entry.sats, got);
}

#[test]
fn oc_49_n13_03_the_same_for_a_customer_fee_refund_signed_elsewhere() {
    let dir = Dir::new();
    let mut rig = start(&dir, &customer_fee_setup()).unwrap();
    let (tx, refund, session) = exported_customer_fee_refund(&mut rig);
    let write = rig.node.queue_writer();
    rig.node
        .owner_mut()
        .unwrap()
        .abandon_session(&session, &write)
        .unwrap();
    let got = paid_to(&tx, &external(9));
    assert!(got > 0 && got < 10_000, "{got}");
    rig.mine(std::slice::from_ref(&tx));
    let entry = rig
        .node
        .paid_log()
        .iter()
        .find(|e| e.number == refund)
        .copied()
        .unwrap();
    assert_eq!(entry.sats, got);
}
