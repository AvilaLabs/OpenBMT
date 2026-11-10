//! The whole approval session with the real crates on both sides: the
//! owner phone's queue, key 1, the Key app with key 2, the core crate's
//! pairing, settings and batch checks, and the spend library's finish.

mod common;

use bitcoin::consensus::deserialize;
use bitcoin::Transaction;
use common::*;
use openbmt_approver::{Approver, Choice};
use openbmt_core::batch::{PinnedRoots, SignedBatch};
use openbmt_core::pairing::SignedPairing;
use openbmt_core::settings::{SettingsStore, SignedSettings};
use openbmt_owner::{CashAmount, Delivery, ItemOutcome, Options, Owner, RefundPacking, Stage};
use openbmt_session::ItemKind;
use openbmt_signer::SignerState;

fn transactions(owner: &Owner) -> Vec<Transaction> {
    owner
        .outbox()
        .iter()
        .filter_map(|d| match &d.what {
            Delivery::Transaction { tx, .. } => Some(deserialize(tx).unwrap()),
            _ => None,
        })
        .collect()
}

fn stage_of(owner: &Owner, id: u32) -> Stage {
    owner
        .refunds()
        .iter()
        .find(|r| r.id == id)
        .map(|r| r.stage.clone())
        .or_else(|| {
            owner
                .cash_outs()
                .iter()
                .find(|c| c.id == id)
                .map(|c| c.stage.clone())
        })
        .expect("the entry exists")
}

/// Both sides are brought back from what they stored, as after a restart.
fn restart(rig: &mut Rig) {
    rig.owner = restore_owner(&rig.owner_store.last()).unwrap();
    rig.key1 = SignerState::from_bytes(&rig.key1_store.last(), NET, &kit().xpubs[0]).unwrap();
    rig.approver = Approver::restore(
        &APPROVER_SECRET,
        wallet().clone(),
        owner_id(),
        policy(),
        Some(kit().accounts[1]),
        &rig.approver_store.last(),
    )
    .unwrap();
}

#[test]
fn the_whole_story_pairing_settings_batch_refunds_and_a_cash_out() {
    let options = Options {
        packing: RefundPacking::OneEach,
        ..Options::default()
    };
    let mut rig = Rig::with(options, true);
    let roots = PinnedRoots::new(owner_id(), approver_id()).unwrap();

    // ---- Session 1: a counter is paired, added by settings and given a batch.
    let (pairing, request) = pair_request(&rig.owner, 1);
    let mut pairing = Some(pairing);
    let pair_id = ok(rig
        .owner
        .queue_pairing(&request, |b| rig.owner_store.put(b)));
    ok(rig
        .owner
        .queue_settings(settings_with(1, &[1]), |b| rig.owner_store.put(b)));
    ok(rig
        .owner
        .queue_batch(batch(1, 0, 3), |b| rig.owner_store.put(b)));
    let built = rig.build(&[], 0);
    // The Key app's screen: four words for the pairing, which the counter
    // must show as well.
    let mut session = rig.approver.open(&built.request, NOW).unwrap();
    let lines = session.lines();
    assert_eq!(
        lines.iter().map(|l| l.kind).collect::<Vec<_>>(),
        vec![
            ItemKind::Pairing,
            ItemKind::Settings,
            ItemKind::BatchRefill,
            ItemKind::HealthCheck
        ]
    );
    // The batch is approvable only once the settings, which come before it
    // in the session, are approved: they add its counter (AS-32).
    assert!(lines[0].can_approve && lines[1].can_approve && lines[3].can_approve);
    assert!(!lines[2].can_approve);
    for l in &lines {
        session.decide(l.item, Choice::Approve).unwrap();
    }
    let response = session.finish(|b| rig.approver_store.put(b)).unwrap();
    let taken = ok(rig.take(&response));
    assert_eq!(taken.outcomes.len(), 4);
    assert_eq!(taken.outcomes[3].1, ItemOutcome::Healthy);
    assert!(rig.owner.pairings().iter().all(|p| p.id != pair_id));
    // The counter takes what came back.
    for d in rig.owner.outbox().to_vec() {
        match d.what {
            Delivery::Pairing(bytes) => {
                let signed = SignedPairing::from_bytes(&bytes).unwrap();
                let paired = pairing
                    .take()
                    .unwrap()
                    .finish(&signed, true, NOW + 5)
                    .unwrap();
                assert_eq!(paired.roots(), roots);
                let mut store = SettingsStore::with_floor(roots, paired.settings_floor());
                let settings = rig
                    .owner
                    .outbox()
                    .iter()
                    .find_map(|d| match &d.what {
                        Delivery::Settings(b) => Some(SignedSettings::from_bytes(b).unwrap()),
                        _ => None,
                    })
                    .unwrap();
                store.apply(settings).unwrap();
                assert!(store
                    .current()
                    .unwrap()
                    .devices
                    .contains(&counter(1).device_id()));
            }
            Delivery::Batch(bytes) => {
                SignedBatch::from_bytes(&bytes)
                    .unwrap()
                    .verify(&roots, &policy())
                    .unwrap();
            }
            Delivery::Settings(_) => {}
            Delivery::Transaction { .. } => panic!("no transaction yet"),
        }
        ok(rig.owner.mark_delivered(d.id, |b| rig.owner_store.put(b)));
    }
    assert!(rig.owner.outbox().is_empty());

    // ---- Session 2: three refunds, one cash-out; the Key app declines one.
    let a = refund(&mut rig.owner, 1, 50_000, 10_000);
    let b = refund(&mut rig.owner, 2, 50_000, 11_000);
    let c = refund(&mut rig.owner, 3, 50_000, 12_000);
    let cash = ok(rig
        .owner
        .queue_cash_out(external(9), CashAmount::Sats(20_000), "Bank", nop));
    let coins: Vec<_> = (0..4).map(|i| coin(i + 1, i, 300_000)).collect();
    let built = rig.build(&coins, 0);
    // The second refund is item 1.
    let response = rig.answer(&built.request, |l| {
        if l.item == 1 {
            Choice::Decline("this is not the one".to_string())
        } else {
            Choice::Approve
        }
    });
    // Both sides are restarted between the Key app's answer and the take.
    restart(&mut rig);
    let taken = ok(rig.take(&response));
    assert!(matches!(taken.outcomes[0].1, ItemOutcome::Delivered(_)));
    assert!(matches!(taken.outcomes[1].1, ItemOutcome::Returned(_)));
    assert!(matches!(taken.outcomes[2].1, ItemOutcome::Delivered(_)));
    assert!(matches!(taken.outcomes[3].1, ItemOutcome::Delivered(_)));
    assert_eq!(taken.outcomes[4].1, ItemOutcome::Healthy);
    assert!(matches!(stage_of(&rig.owner, a), Stage::Signed { .. }));
    assert!(matches!(stage_of(&rig.owner, b), Stage::Returned { .. }));
    assert!(matches!(stage_of(&rig.owner, c), Stage::Signed { .. }));
    assert!(matches!(stage_of(&rig.owner, cash), Stage::Signed { .. }));

    // The finished transactions pay what was frozen, and nothing else.
    let txs = transactions(&rig.owner);
    assert_eq!(txs.len(), 3);
    assert_eq!(pays(&txs[0])[0].script_pubkey, external(1));
    assert_eq!(pays(&txs[0])[0].value.to_sat(), 10_000);
    assert_eq!(pays(&txs[1])[0].script_pubkey, external(3));
    assert_eq!(pays(&txs[1])[0].value.to_sat(), 12_000);
    assert_eq!(pays(&txs[2])[0].script_pubkey, external(9));
    assert_eq!(pays(&txs[2])[0].value.to_sat(), 20_000);
    assert!(txs
        .iter()
        .all(|t| t.output.len() == 2 && t.input.iter().all(|i| !i.witness.is_empty())));

    // They are sent and confirmed.
    for t in &txs {
        let txid = t.compute_txid();
        ok(rig.owner.mark_broadcast(&txid, |b| rig.owner_store.put(b)));
        ok(rig.owner.mark_confirmed(&txid, |b| rig.owner_store.put(b)));
    }
    assert!(rig.owner.outbox().is_empty());
    assert!(matches!(stage_of(&rig.owner, a), Stage::Paid { .. }));

    // ---- Session 3: the declined refund is asked for again and approved.
    ok(rig.owner.requeue(b, |x| rig.owner_store.put(x)));
    let taken = rig.round(&[coin(9, 4, 300_000)], 3);
    assert!(matches!(taken.outcomes[0].1, ItemOutcome::Delivered(_)));
    let txs = transactions(&rig.owner);
    assert_eq!(txs.len(), 1);
    assert_eq!(pays(&txs[0])[0].value.to_sat(), 11_000);
    let txid = txs[0].compute_txid();
    ok(rig.owner.mark_confirmed(&txid, |x| rig.owner_store.put(x)));
    assert!(rig
        .owner
        .refunds()
        .iter()
        .all(|r| matches!(r.stage, Stage::Paid { .. })));
    assert_eq!(ok(rig.owner.archive_paid(|x| rig.owner_store.put(x))), 4);

    // Each sale was refunded once, and no sale more than was received.
    for (n, sats) in [(1, 10_000), (2, 11_000), (3, 12_000)] {
        assert_eq!(rig.owner.refunded_sats(&sale(n)), sats);
        assert!(sats <= rig.owner.sale_facts(&sale(n)).unwrap().received_sats);
    }
    // Key 1 and key 2 each counted four spends once, in their own logs.
    assert_eq!(rig.key1.log().count(), 4);
    assert_eq!(rig.approver.signer().unwrap().log().count(), 4);
    // The queue is empty and restores.
    assert!(rig.owner.cash_outs().is_empty());
    let again = restore_owner(&rig.owner_store.last()).unwrap();
    assert_eq!(again.snapshot().unwrap(), rig.owner.snapshot().unwrap());
}

#[test]
fn one_transaction_for_all_refunds_is_approved_or_declined_whole() {
    let mut rig = Rig::new();
    let a = refund(&mut rig.owner, 1, 50_000, 10_000);
    let b = refund(&mut rig.owner, 2, 50_000, 11_000);
    let built = rig.build(&[coin(1, 0, 300_000)], 0);
    // Declined: both come back.
    let response = rig.answer(&built.request, |l| {
        if l.kind == ItemKind::Spend {
            Choice::Decline("no refunds today".to_string())
        } else {
            Choice::Approve
        }
    });
    ok(rig.take(&response));
    assert!(matches!(stage_of(&rig.owner, a), Stage::Returned { .. }));
    assert!(matches!(stage_of(&rig.owner, b), Stage::Returned { .. }));
    assert!(transactions(&rig.owner).is_empty());
    // Requeued, they go together and are approved.
    ok(rig.owner.requeue(a, nop));
    ok(rig.owner.requeue(b, nop));
    rig.round(&[coin(2, 1, 300_000)], 1);
    let txs = transactions(&rig.owner);
    assert_eq!(txs.len(), 1);
    assert_eq!(txs[0].output.len(), 3);
    assert!(matches!(stage_of(&rig.owner, a), Stage::Signed { .. }));
    assert!(matches!(stage_of(&rig.owner, b), Stage::Signed { .. }));
}

#[test]
fn a_refund_the_key_app_refuses_by_its_own_ceiling_is_returned() {
    let mut rig = Rig::new();
    // 150,000 is above the Key app's first ceiling of 100,000 per
    // transaction for other destinations.
    let id = refund(&mut rig.owner, 1, 500_000, 150_000);
    let built = rig.build(&[coin(1, 0, 600_000)], 0);
    let mut session = rig.approver.open(&built.request, NOW).unwrap();
    let lines = session.lines();
    assert!(!lines[0].can_approve);
    assert!(lines[0].failure.as_ref().unwrap().starts_with("SR-"));
    session
        .decide(0, Choice::Decline("over my ceiling".to_string()))
        .unwrap();
    session.decide(1, Choice::Approve).unwrap();
    let response = session.finish(|b| rig.approver_store.put(b)).unwrap();
    let taken = ok(rig.take(&response));
    let ItemOutcome::Returned(reason) = &taken.outcomes[0].1 else {
        panic!("{:?}", taken.outcomes)
    };
    assert!(reason.contains("over my ceiling"), "{reason}");
    assert!(matches!(stage_of(&rig.owner, id), Stage::Returned { .. }));
}
