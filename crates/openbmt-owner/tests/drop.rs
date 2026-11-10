//! A dropped transaction that is asked for again must conflict with the old
//! one (AS-68, SP-13).

mod common;

use bitcoin::psbt::Psbt;
use bitcoin::{OutPoint, Transaction};
use common::*;
use openbmt_owner::{CashAmount, Options, Owner, RefundPacking, Stage};
use openbmt_session::{Item, Request};
use openbmt_spend::Coin;

fn spends(request: &[u8]) -> Vec<Transaction> {
    Request::from_bytes(request)
        .unwrap()
        .items()
        .iter()
        .filter_map(|i| match &i.item {
            Item::Spend { psbt, .. } => Some(Psbt::deserialize(psbt).unwrap().unsigned_tx),
            _ => None,
        })
        .collect()
}

fn inputs(tx: &Transaction) -> Vec<OutPoint> {
    tx.input.iter().map(|i| i.previous_output).collect()
}

fn txid_of(o: &Owner, id: u32) -> bitcoin::Txid {
    let stage = o
        .refunds()
        .iter()
        .find(|r| r.id == id)
        .map(|r| r.stage.clone())
        .or_else(|| {
            o.cash_outs()
                .iter()
                .find(|c| c.id == id)
                .map(|c| c.stage.clone())
        })
        .unwrap();
    match stage {
        Stage::Signed { txid } | Stage::Sent { txid } => txid,
        other => panic!("{other:?}"),
    }
}

/// A coin of `sats` at wallet index `index` with `confirmations`.
fn deep_coin(tag: u32, index: u32, sats: u64, confirmations: u32) -> Coin {
    let mut c = coin(tag, index, sats);
    c.confirmations = confirmations;
    c
}

/// Sign the entries with a first transaction from `c1` alone, send it, and
/// report it dropped.
fn drop_first(rig: &mut Rig, ids: &[u32], c1: &Coin) {
    rig.round(std::slice::from_ref(c1), 0);
    let txid = txid_of(&rig.owner, ids[0]);
    ok(rig.owner.mark_broadcast(&txid, nop));
    ok(rig.owner.mark_dropped(&txid, nop));
    for id in ids {
        ok(rig.owner.requeue(*id, nop));
    }
}

#[test]
fn as_68_dropped_then_asked_again_spends_an_old_outpoint() {
    let mut rig = Rig::new();
    let id = refund(&mut rig.owner, 1, 50_000, 10_000);
    let c1 = deep_coin(1, 0, 300_000, 6);
    drop_first(&mut rig, &[id], &c1);
    // The old outpoint is back among the coins, with fewer confirmations
    // than another coin, which oldest-first would choose.
    let young = deep_coin(1, 0, 300_000, 1);
    let old = deep_coin(2, 1, 300_000, 9);
    let built = rig.build(&[young.clone(), old.clone()], 1);
    let txs = spends(&built.request);
    assert_eq!(txs.len(), 1);
    assert!(inputs(&txs[0]).contains(&young.outpoint));
    assert!(built.skipped.is_empty());
    // And the whole thing goes through.
    let response = rig.answer_all(&built.request);
    ok(rig.take(&response));
    assert!(matches!(rig.owner.refunds()[0].stage, Stage::Signed { .. }));
}

#[test]
fn as_68_the_old_outpoints_are_kept_with_the_entry_and_in_the_snapshot() {
    let mut rig = Rig::new();
    let id = refund(&mut rig.owner, 1, 50_000, 10_000);
    let c1 = deep_coin(1, 0, 300_000, 6);
    drop_first(&mut rig, &[id], &c1);
    assert_eq!(rig.owner.refunds()[0].conflict, vec![c1.outpoint]);
    let again = restore_owner(&rig.owner.snapshot().unwrap()).unwrap();
    assert_eq!(again.refunds(), rig.owner.refunds());
    assert_eq!(again.snapshot().unwrap(), rig.owner.snapshot().unwrap());
    // They are added to, not replaced, by a second drop.
    let c2 = deep_coin(2, 1, 300_000, 9);
    let built = rig.build(&[c1.clone(), c2.clone()], 1);
    let response = rig.answer_all(&built.request);
    ok(rig.take(&response));
    let txid = txid_of(&rig.owner, id);
    ok(rig.owner.mark_dropped(&txid, nop));
    let mut want = vec![c1.outpoint];
    want.extend(
        spends(&built.request)
            .iter()
            .flat_map(inputs)
            .filter(|o| *o != c1.outpoint),
    );
    want.sort();
    want.dedup();
    assert_eq!(rig.owner.refunds()[0].conflict, want);
}

#[test]
fn as_68_when_every_old_outpoint_is_gone_the_entry_is_planned_freely() {
    let mut rig = Rig::new();
    let id = refund(&mut rig.owner, 1, 50_000, 10_000);
    let c1 = deep_coin(1, 0, 300_000, 6);
    drop_first(&mut rig, &[id], &c1);
    // The old coin is not among the coins passed: something else spent it.
    let other = deep_coin(2, 1, 300_000, 3);
    let built = rig.build(std::slice::from_ref(&other), 1);
    let txs = spends(&built.request);
    assert_eq!(txs.len(), 1);
    assert_eq!(inputs(&txs[0]), vec![other.outpoint]);
    assert!(built.skipped.is_empty());
}

#[test]
fn as_68_an_old_outpoint_that_is_present_but_unconfirmed_holds_the_entry_back() {
    let mut rig = Rig::new();
    let id = refund(&mut rig.owner, 1, 50_000, 10_000);
    let c1 = deep_coin(1, 0, 300_000, 6);
    drop_first(&mut rig, &[id], &c1);
    let unconfirmed = deep_coin(1, 0, 300_000, 0);
    let other = deep_coin(2, 1, 300_000, 9);
    let built = rig.build(&[unconfirmed, other], 1);
    assert!(spends(&built.request).is_empty());
    assert_eq!(built.skipped.len(), 1);
    assert_eq!(built.skipped[0].id, id);
    assert!(
        built.skipped[0].reason.starts_with("SP-13"),
        "{}",
        built.skipped[0].reason
    );
    assert_eq!(rig.owner.refunds()[0].stage, Stage::Queued);
}

#[test]
fn as_68_refunds_of_one_dropped_transaction_stay_together_in_one_transaction() {
    let mut rig = Rig::new();
    let a = refund(&mut rig.owner, 1, 50_000, 10_000);
    let b = refund(&mut rig.owner, 2, 50_000, 11_000);
    let c1 = deep_coin(1, 0, 300_000, 6);
    drop_first(&mut rig, &[a, b], &c1);
    // Even with one transaction for each refund, the two come back in one.
    let options = Options {
        packing: RefundPacking::OneEach,
        ..Options::default()
    };
    ok(rig.owner.set_options(options, nop));
    // And a third, new refund goes in a transaction of its own.
    let c = refund(&mut rig.owner, 3, 50_000, 12_000);
    let young = deep_coin(1, 0, 300_000, 1);
    let old = deep_coin(2, 1, 300_000, 9);
    let built = rig.build(&[young.clone(), old.clone()], 1);
    let txs = spends(&built.request);
    assert_eq!(txs.len(), 2);
    // The constrained pair comes first and spends the old outpoint.
    assert_eq!(txs[0].output.len(), 3, "two refunds and the change");
    assert!(inputs(&txs[0]).contains(&young.outpoint));
    assert!(!inputs(&txs[1]).contains(&young.outpoint));
    // The change may stand at any place among the outputs (AS-61).
    assert!(txs[1].output.iter().any(|o| o.value.to_sat() == 12_000));
    let _ = c;
}

#[test]
fn as_68_an_entry_with_kept_outpoints_cannot_be_cancelled() {
    let mut rig = Rig::new();
    let id = refund(&mut rig.owner, 1, 50_000, 10_000);
    let c1 = deep_coin(1, 0, 300_000, 6);
    drop_first(&mut rig, &[id], &c1);
    assert!(matches!(
        refused(rig.owner.cancel(id, nop)),
        openbmt_owner::Refusal::WrongStage(_)
    ));
    assert_eq!(rig.owner.refunded_sats(&sale(1)), 10_000);
}

#[test]
fn as_68_a_cash_out_that_was_dropped_conflicts_with_its_old_transaction_too() {
    let mut rig = Rig::new();
    let cash = ok(rig
        .owner
        .queue_cash_out(external(9), CashAmount::Sats(50_000), "Bank", nop));
    let c1 = deep_coin(1, 0, 300_000, 6);
    drop_first(&mut rig, &[cash], &c1);
    assert_eq!(rig.owner.cash_outs()[0].conflict, vec![c1.outpoint]);
    let young = deep_coin(1, 0, 300_000, 1);
    let old = deep_coin(2, 1, 300_000, 9);
    let built = rig.build(&[young.clone(), old], 1);
    let txs = spends(&built.request);
    assert_eq!(txs.len(), 1);
    assert!(inputs(&txs[0]).contains(&young.outpoint));
}

#[test]
fn as_68_an_entry_whose_outpoints_another_transaction_of_the_session_took_waits() {
    let mut rig = Rig::new();
    let cash = ok(rig
        .owner
        .queue_cash_out(external(9), CashAmount::Sats(50_000), "Bank", nop));
    let c1 = deep_coin(1, 0, 300_000, 6);
    drop_first(&mut rig, &[cash], &c1);
    // A new refund is planned first and takes the oldest coin, which is the
    // cash-out's old outpoint. Planning the cash-out freely now could pay
    // twice if its old transaction confirmed, so it waits.
    let r = refund(&mut rig.owner, 1, 50_000, 10_000);
    let old = deep_coin(1, 0, 300_000, 9);
    let other = deep_coin(2, 1, 300_000, 3);
    let built = rig.build(&[old.clone(), other], 1);
    let txs = spends(&built.request);
    assert_eq!(txs.len(), 1);
    assert!(inputs(&txs[0]).contains(&old.outpoint));
    assert_eq!(built.skipped.len(), 1);
    assert_eq!(built.skipped[0].id, cash);
    assert!(built.skipped[0].reason.starts_with("AS-68"));
    assert_eq!(rig.owner.cash_outs()[0].stage, Stage::Queued);
    let _ = r;
}

#[test]
fn as_54_a_replaced_transaction_returns_its_entries_and_keeps_no_conflict() {
    let mut rig = Rig::new();
    let id = refund(&mut rig.owner, 1, 50_000, 10_000);
    let c1 = deep_coin(1, 0, 300_000, 6);
    rig.round(std::slice::from_ref(&c1), 0);
    let txid = txid_of(&rig.owner, id);
    ok(rig.owner.mark_broadcast(&txid, nop));
    ok(rig.owner.mark_replaced(&txid, nop));
    assert!(matches!(
        rig.owner.refunds()[0].stage,
        Stage::Returned { .. }
    ));
    assert!(rig.owner.refunds()[0].conflict.is_empty());
    assert!(rig.owner.outbox().is_empty());
    // It is not followed any more.
    assert!(rig.owner.mark_replaced(&txid, nop).is_err());
}

#[test]
fn as_54_the_receive_and_change_marks_follow_what_was_handed_out() {
    let mut rig = Rig::new();
    assert_eq!(rig.owner.receive_mark(), 0);
    assert_eq!(rig.owner.change_mark(), 0);
    ok(rig.owner.queue_batch(batch(1, 0, 3), nop));
    assert_eq!(rig.owner.receive_mark(), 3);
    refund(&mut rig.owner, 1, 50_000, 10_000);
    let c1 = deep_coin(1, 0, 300_000, 6);
    let built = rig.build(std::slice::from_ref(&c1), 4);
    assert_eq!(rig.owner.change_mark(), 5);
    // The session closes with the answer, and the mark goes with it.
    let response = rig.answer_all(&built.request);
    ok(rig.take(&response));
    assert_eq!(rig.owner.change_mark(), 0);
}
