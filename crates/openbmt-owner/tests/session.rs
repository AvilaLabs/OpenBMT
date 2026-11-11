//! Building a session and taking its response (AS-53 to AS-64, AS-66,
//! AS-67), with the hostile cases.

mod common;

use bitcoin::consensus::deserialize;
use bitcoin::psbt::Psbt;
use bitcoin::{ScriptBuf, Transaction};
use common::*;
use openbmt_approver::Choice;
use openbmt_core::identity::{DeviceId, Identity};
use openbmt_owner::{
    CashAmount, Delivery, FeePayer, ItemOutcome, Options, OwnerError, RefundPacking, Refusal,
    Stage, Taken,
};
use openbmt_session::{
    DeclineCode, Item, ItemKind, ItemResult, Label, Payload, Request, Response, Verdict,
};

const BIG: u64 = 1_000_000_000;

/// The unsigned transactions of the spend items of a request.
fn spends(request: &[u8]) -> Vec<Transaction> {
    let request = Request::from_bytes(request).unwrap();
    request
        .items()
        .iter()
        .filter_map(|i| match &i.item {
            Item::Spend { psbt, .. } => Some(Psbt::deserialize(psbt).unwrap().unsigned_tx),
            _ => None,
        })
        .collect()
}

fn kinds(request: &[u8]) -> Vec<ItemKind> {
    Request::from_bytes(request)
        .unwrap()
        .items()
        .iter()
        .map(|i| i.item.kind())
        .collect()
}

fn paid(tx: &Transaction) -> Vec<(ScriptBuf, u64)> {
    tx.output
        .iter()
        .map(|o| (o.script_pubkey.clone(), o.value.to_sat()))
        .collect()
}

/// The outputs that are not change to one of the first sixteen change
/// scripts, with their amounts, in the order of the transaction (AS-61: the
/// change can stand at any place).
fn without_change(tx: &Transaction) -> Vec<(ScriptBuf, u64)> {
    pays(tx)
        .iter()
        .map(|o| (o.script_pubkey.clone(), o.value.to_sat()))
        .collect()
}

/// The position of the change to `index` in `tx`.
fn change_position(tx: &Transaction, index: u32) -> usize {
    let script = wallet().change_script(index).unwrap();
    tx.output
        .iter()
        .position(|o| o.script_pubkey == script)
        .expect("the transaction has that change")
}

fn stage_of(rig: &Rig, id: u32) -> Stage {
    rig.owner
        .refunds()
        .iter()
        .find(|r| r.id == id)
        .map(|r| r.stage.clone())
        .or_else(|| {
            rig.owner
                .cash_outs()
                .iter()
                .find(|r| r.id == id)
                .map(|r| r.stage.clone())
        })
        .expect("the entry exists")
}

// ---- AS-53 -----------------------------------------------------------------------------

#[test]
fn as_53_a_refund_waits_for_the_refund_depth() {
    let mut rig = Rig::new();
    set_facts(&mut rig.owner, sale(1), shallow(50_000));
    let id = ok(rig
        .owner
        .queue_refund(sale(1), "Mia", external(1), 10_000, None, nop));
    assert_eq!(rig.owner.waiting_for_depth(id), Some(true));
    let built = rig.build(&[coin(1, 0, 300_000)], 0);
    assert_eq!(kinds(&built.request), vec![ItemKind::HealthCheck]);
    assert_eq!(stage_of(&rig, id), Stage::Queued);
    set_facts(&mut rig.owner, sale(1), deep(50_000));
    assert_eq!(rig.owner.waiting_for_depth(id), Some(false));
    let built = rig.build(&[coin(1, 0, 300_000)], 0);
    assert_eq!(
        kinds(&built.request),
        vec![ItemKind::Spend, ItemKind::HealthCheck]
    );
    assert_eq!(rig.owner.waiting_for_depth(99), None);
}

#[test]
fn as_53_facts_that_fall_below_the_refunds_counted_make_none_ready() {
    let mut rig = Rig::new();
    let a = refund(&mut rig.owner, 1, 50_000, 30_000);
    // Later facts show less received than the refund already counted.
    set_facts(&mut rig.owner, sale(1), deep(20_000));
    assert_eq!(rig.owner.waiting_for_depth(a), Some(true));
    let built = rig.build(&[coin(1, 0, 300_000)], 0);
    assert_eq!(kinds(&built.request), vec![ItemKind::HealthCheck]);
    set_facts(&mut rig.owner, sale(1), deep(30_000));
    let built = rig.build(&[coin(1, 0, 300_000)], 0);
    assert!(!built.request.is_empty());
    assert_eq!(spends(&built.request).len(), 1);
}

// ---- AS-55 -----------------------------------------------------------------------------

#[test]
fn as_55_all_ready_refunds_go_into_one_transaction() {
    let mut rig = Rig::new();
    for n in 1..=3 {
        refund(&mut rig.owner, n, 50_000, 10_000);
    }
    let built = rig.build(&[coin(1, 0, 300_000)], 0);
    let txs = spends(&built.request);
    assert_eq!(txs.len(), 1);
    // One output for each refund, in queue order, and the change.
    assert_eq!(paid(&txs[0]).len(), 4);
    let out = without_change(&txs[0]);
    assert_eq!(out.len(), 3);
    for n in 1..=3u8 {
        assert_eq!(out[usize::from(n) - 1], (external(n), 10_000));
    }
}

#[test]
fn as_55_one_transaction_for_each_refund_is_the_owners_option() {
    let options = Options {
        packing: RefundPacking::OneEach,
        ..Options::default()
    };
    let mut rig = Rig::with(options, true);
    for n in 1..=3 {
        refund(&mut rig.owner, n, 50_000, 10_000);
    }
    let coins = [
        coin(1, 0, 300_000),
        coin(2, 1, 300_000),
        coin(3, 2, 300_000),
    ];
    let built = rig.build(&coins, 5);
    let txs = spends(&built.request);
    assert_eq!(txs.len(), 3);
    // No coin is used twice, and each transaction has its own change index.
    let mut inputs: Vec<_> = txs
        .iter()
        .flat_map(|t| t.input.iter().map(|i| i.previous_output))
        .collect();
    inputs.sort();
    inputs.dedup();
    assert_eq!(inputs.len(), 3);
    for (k, tx) in txs.iter().enumerate() {
        let change = wallet().change_script(5 + k as u32).unwrap();
        assert_eq!(
            tx.output
                .iter()
                .filter(|o| o.script_pubkey == change)
                .count(),
            1
        );
    }
}

#[test]
fn as_55_at_most_99_refunds_share_a_transaction() {
    let mut rig = Rig::new();
    for n in 1..=100u32 {
        set_facts(&mut rig.owner, sale(n), deep(5_000));
        ok(rig.owner.queue_refund(
            sale(n),
            "Mia",
            external((n % 200) as u8 + 1),
            1_000,
            None,
            nop,
        ));
    }
    let built = rig.build(&[coin(1, 0, BIG), coin(2, 1, BIG)], 0);
    let txs = spends(&built.request);
    assert_eq!(txs.len(), 2);
    assert_eq!(txs[0].output.len(), 99 + 1);
    assert_eq!(txs[1].output.len(), 1 + 1);
    assert!(built.skipped.is_empty());
}

#[test]
fn as_50_a_session_holds_at_most_16_transactions() {
    let options = Options {
        packing: RefundPacking::OneEach,
        ..Options::default()
    };
    let mut rig = Rig::with(options, true);
    for n in 1..=17 {
        refund(&mut rig.owner, n, 50_000, 10_000);
    }
    let coins: Vec<_> = (0..17).map(|i| coin(i + 1, i, 300_000)).collect();
    let built = rig.build(&coins, 0);
    assert_eq!(spends(&built.request).len(), 16);
    assert_eq!(built.skipped.len(), 1);
    assert!(built.skipped[0].reason.starts_with("AS-50"));
    // The skipped one is still queued and goes into the next session.
    let last = rig.owner.refunds().last().unwrap();
    assert_eq!(last.stage, Stage::Queued);
}

#[test]
fn as_50_a_session_holds_at_most_64_items() {
    let mut rig = Rig::new();
    for n in 1..=16u8 {
        let (_, request) = pair_request(&rig.owner, n);
        ok(rig.owner.queue_pairing(&request, nop));
    }
    for i in 0..16 {
        ok(rig.owner.queue_batch(batch(1, i * 3, 3), 1_000, nop));
    }
    let built = rig.build(&[], 0);
    // 16 pairings, 16 batches and the health check.
    assert_eq!(kinds(&built.request).len(), 33);
    assert!(built.skipped.is_empty());
}

// ---- AS-56 -----------------------------------------------------------------------------

#[test]
fn as_56_the_shop_pays_the_fee_on_top() {
    let mut rig = Rig::new();
    refund(&mut rig.owner, 1, 50_000, 10_000);
    refund(&mut rig.owner, 2, 50_000, 12_000);
    let built = rig.build(&[coin(1, 0, 300_000)], 0);
    let tx = &spends(&built.request)[0];
    assert_eq!(pays(tx)[0].value.to_sat(), 10_000);
    assert_eq!(pays(tx)[1].value.to_sat(), 12_000);
}

#[test]
fn as_56_the_customer_pays_an_equal_share_and_the_sums_agree() {
    let options = Options {
        fee_payer: FeePayer::Customer,
        ..Options::default()
    };
    let mut rig = Rig::with(options, true);
    let amounts = [10_000u64, 12_000, 8_001];
    for (n, a) in amounts.iter().enumerate() {
        refund(&mut rig.owner, n as u32 + 1, 50_000, *a);
    }
    let coin = coin(1, 0, 300_000);
    let built = rig.build(std::slice::from_ref(&coin), 0);
    let tx = &spends(&built.request)[0];
    let out: Vec<u64> = pays(tx).iter().map(|o| o.value.to_sat()).collect();
    assert_eq!(out.len(), 3);
    let total: u64 = amounts.iter().sum();
    let fee = 300_000 - tx.output.iter().map(|o| o.value.to_sat()).sum::<u64>();
    // The refund outputs and the fee add up to the refunds' amounts.
    assert_eq!(out.iter().sum::<u64>() + fee, total);
    // Equal shares; the remainder goes one satoshi at a time to the first.
    let shares: Vec<u64> = amounts.iter().zip(&out).map(|(a, o)| a - o).collect();
    let (min, max) = (*shares.iter().min().unwrap(), *shares.iter().max().unwrap());
    assert!(max - min <= 1, "{shares:?}");
    let first_bigger = shares.windows(2).all(|w| w[0] >= w[1]);
    assert!(first_bigger, "{shares:?}");
}

#[test]
fn as_56_a_refund_that_would_fall_below_dust_is_held_back_with_a_reason() {
    let options = Options {
        fee_payer: FeePayer::Customer,
        ..Options::default()
    };
    let mut rig = Rig::with(options, true);
    let small = refund(&mut rig.owner, 1, 50_000, 350);
    let big = refund(&mut rig.owner, 2, 60_000, 50_000);
    let built = rig.build(&[coin(1, 0, 300_000)], 0);
    let tx = &spends(&built.request)[0];
    // Only the big one is in the transaction, less the whole fee.
    let out = paid(tx);
    assert_eq!(out[0].0, external(2));
    assert!(out[0].1 < 50_000 && out[0].1 > 49_000);
    assert_eq!(built.skipped.len(), 1);
    assert_eq!(built.skipped[0].id, small);
    assert!(built.skipped[0].reason.starts_with("AS-56"));
    assert_eq!(stage_of(&rig, small), Stage::Queued);
    assert!(matches!(stage_of(&rig, big), Stage::InSession { .. }));
}

#[test]
fn as_56_when_every_refund_is_below_dust_after_its_share_none_is_planned() {
    let options = Options {
        fee_payer: FeePayer::Customer,
        ..Options::default()
    };
    let mut rig = Rig::with(options, true);
    let small = refund(&mut rig.owner, 1, 50_000, 300);
    let built = rig.build(&[coin(1, 0, 300_000)], 0);
    assert!(spends(&built.request).is_empty());
    assert_eq!(built.skipped.len(), 1);
    assert_eq!(built.skipped[0].id, small);
}

// ---- AS-57 -----------------------------------------------------------------------------

#[test]
fn as_57_a_cash_out_is_its_own_transaction_with_the_coins_left() {
    let mut rig = Rig::new();
    refund(&mut rig.owner, 1, 50_000, 10_000);
    let cash = ok(rig
        .owner
        .queue_cash_out(external(9), CashAmount::All, "Bank", nop));
    let c1 = coin(1, 0, 100_000);
    let c2 = coin(2, 1, 200_000);
    let built = rig.build(&[c1.clone(), c2.clone()], 0);
    let txs = spends(&built.request);
    assert_eq!(txs.len(), 2);
    // The refund took the oldest coin it needed; the cash-out took what was
    // left, all of it, with no change.
    let first: Vec<_> = txs[0].input.iter().map(|i| i.previous_output).collect();
    let second: Vec<_> = txs[1].input.iter().map(|i| i.previous_output).collect();
    assert!(first.iter().all(|o| !second.contains(o)));
    assert_eq!(txs[1].output.len(), 1);
    assert_eq!(txs[1].output[0].script_pubkey, external(9));
    assert!(matches!(stage_of(&rig, cash), Stage::InSession { .. }));
    // The labels say what each is.
    let request = Request::from_bytes(&built.request).unwrap();
    let Item::Spend { labels, .. } = &request.items()[1].item else {
        panic!()
    };
    assert_eq!(
        labels[0].label,
        Label::CashOut {
            destination: "Bank".to_string()
        }
    );
}

#[test]
fn as_57_an_amount_cash_out_has_change_and_a_label_for_it() {
    let mut rig = Rig::new();
    ok(rig
        .owner
        .queue_cash_out(external(9), CashAmount::Sats(50_000), "Bank", nop));
    let built = rig.build(&[coin(1, 0, 200_000)], 3);
    let tx = &spends(&built.request)[0];
    assert_eq!(tx.output.len(), 2);
    assert_eq!(without_change(tx), vec![(external(9), 50_000)]);
    let at = change_position(tx, 3);
    let request = Request::from_bytes(&built.request).unwrap();
    let Item::Spend { labels, .. } = &request.items()[0].item else {
        panic!()
    };
    // The labels name the real positions, in ascending order.
    assert_eq!(labels.len(), 2);
    assert_eq!((labels[0].output, labels[1].output), (0, 1));
    let cash = labels.iter().find(|l| l.output == 1 - at as u32).unwrap();
    assert_eq!(
        cash.label,
        Label::CashOut {
            destination: "Bank".to_string()
        }
    );
    let change = labels.iter().find(|l| l.output == at as u32).unwrap();
    assert_eq!(change.label, Label::Change);
}

#[test]
fn as_57_a_cash_out_that_does_not_fit_stays_queued_with_a_reason() {
    let mut rig = Rig::new();
    let cash = ok(rig
        .owner
        .queue_cash_out(external(9), CashAmount::Sats(500_000), "Bank", nop));
    let built = rig.build(&[coin(1, 0, 100_000)], 0);
    assert_eq!(built.skipped.len(), 1);
    assert_eq!(built.skipped[0].id, cash);
    assert_eq!(stage_of(&rig, cash), Stage::Queued);
}

// ---- AS-60, AS-61 -----------------------------------------------------------------------

#[test]
fn as_60_every_session_ends_with_a_health_check_from_the_random_bytes() {
    let mut rig = Rig::new();
    refund(&mut rig.owner, 1, 50_000, 10_000);
    ok(rig.owner.queue_batch(batch(1, 0, 3), 1_000, nop));
    let (_, request) = pair_request(&rig.owner, 1);
    ok(rig.owner.queue_pairing(&request, nop));
    let built = rig.build(&[coin(1, 0, 300_000)], 0);
    let request = Request::from_bytes(&built.request).unwrap();
    // Transactions, pairings, settings, batches, then the health check.
    assert_eq!(
        kinds(&built.request),
        vec![
            ItemKind::Spend,
            ItemKind::Pairing,
            ItemKind::BatchRefill,
            ItemKind::HealthCheck
        ]
    );
    let last = request.items().last().unwrap();
    let Item::HealthCheck { nonce } = &last.item else {
        panic!()
    };
    let random = {
        let mut r = Rig::new();
        r.random()
    };
    // The first session of a rig uses the same fixed random bytes.
    assert_eq!(&random[32..64], nonce);
    assert_eq!(request.session_id(), &built.session_id);
    assert_eq!(&random[..32], &built.session_id);
    // A session with nothing else is only the health check.
    let mut empty = Rig::new();
    let built = empty.build(&[], 0);
    assert_eq!(kinds(&built.request), vec![ItemKind::HealthCheck]);
}

#[test]
fn as_61_the_session_is_stored_before_the_bytes_are_returned() {
    let mut rig = Rig::new();
    refund(&mut rig.owner, 1, 50_000, 10_000);
    let before = rig.owner.snapshot().unwrap();
    // A store that fails: nothing is returned and the queue is as it was.
    let store = Store::default();
    store.fail_next(true);
    let random = rig.random();
    let err = rig
        .owner
        .build_request(&[coin(1, 0, 300_000)], 1_000, 0, 0, NOW, &random, |b| {
            store.put(b)
        })
        .expect_err("the store fails");
    assert_eq!(err, OwnerError::Store("disk full".to_string()));
    assert_eq!(rig.owner.snapshot().unwrap(), before);
    assert!(rig.owner.open_sessions().is_empty());
    // With a store that works, what it was handed restores to the session.
    store.fail_next(false);
    let built =
        ok(rig
            .owner
            .build_request(&[coin(1, 0, 300_000)], 1_000, 0, 0, NOW, &random, |b| {
                store.put(b)
            }));
    let restored = restore_owner(&store.last()).unwrap();
    assert_eq!(restored.open_sessions(), vec![built.session_id]);
    assert_eq!(
        restored.session_request(&built.session_id),
        Some(built.request.as_slice())
    );
}

#[test]
fn as_61_an_entry_that_does_not_fit_stays_queued_and_the_others_go() {
    let mut rig = Rig::new();
    let big = refund(&mut rig.owner, 1, BIG, 500_000);
    let options = Options {
        packing: RefundPacking::OneEach,
        ..Options::default()
    };
    rig.owner.set_options(options, nop).unwrap();
    let small = refund(&mut rig.owner, 2, 50_000, 10_000);
    let built = rig.build(&[coin(1, 0, 100_000)], 0);
    assert_eq!(spends(&built.request).len(), 1);
    assert_eq!(built.skipped.len(), 1);
    assert_eq!(built.skipped[0].id, big);
    assert!(matches!(stage_of(&rig, small), Stage::InSession { .. }));
    assert_eq!(stage_of(&rig, big), Stage::Queued);
}

#[test]
fn as_61_a_fee_rate_of_zero_and_a_used_session_id_are_refused() {
    let mut rig = Rig::new();
    let random = rig.random();
    assert_eq!(
        refused(rig.owner.build_request(&[], 0, 0, 0, NOW, &random, nop)),
        Refusal::ZeroFeeRate
    );
    ok(rig.owner.build_request(&[], 1_000, 0, 0, NOW, &random, nop));
    assert_eq!(
        refused(rig.owner.build_request(&[], 1_000, 0, 0, NOW, &random, nop)),
        Refusal::SessionIdUsed
    );
}

// ---- AS-62 -----------------------------------------------------------------------------

fn one_refund_rig() -> (Rig, u32, openbmt_owner::Built) {
    let mut rig = Rig::new();
    let id = refund(&mut rig.owner, 1, 50_000, 10_000);
    let built = rig.build(&[coin(1, 0, 300_000)], 0);
    (rig, id, built)
}

#[test]
fn as_62_an_honest_response_finishes_the_transaction_with_both_keys() {
    let (mut rig, id, built) = one_refund_rig();
    let response = rig.answer_all(&built.request);
    let taken = ok(rig.take(&response));
    assert_eq!(taken.session_id, built.session_id);
    assert!(matches!(taken.outcomes[0].1, ItemOutcome::Delivered(_)));
    assert_eq!(taken.outcomes[1].1, ItemOutcome::Healthy);
    let Stage::Signed { txid } = stage_of(&rig, id) else {
        panic!()
    };
    let d = rig.owner.outbox().first().unwrap();
    let Delivery::Transaction { tx, refunds, .. } = &d.what else {
        panic!()
    };
    assert_eq!(refunds, &vec![id]);
    let tx: Transaction = deserialize(tx).unwrap();
    assert_eq!(tx.compute_txid(), txid);
    // Both signatures are on every input.
    assert!(tx.input.iter().all(|i| !i.witness.is_empty()));
    // The session is closed.
    assert!(rig.owner.open_sessions().is_empty());
}

#[test]
fn as_62_a_response_for_a_session_that_is_not_open_is_refused() {
    let (mut rig, _, built) = one_refund_rig();
    let response = rig.answer_all(&built.request);
    ok(rig.take(&response));
    // Taking the same response again: the session is gone.
    let before = rig.owner.snapshot().unwrap();
    assert_eq!(refused(rig.take(&response)), Refusal::UnknownSession);
    assert_eq!(rig.owner.snapshot().unwrap(), before);
}

fn forged(
    rig: &Rig,
    built: &openbmt_owner::Built,
    who: &Identity,
    results: Vec<ItemResult>,
) -> Vec<u8> {
    let request = Request::from_bytes(&built.request).unwrap();
    let _ = rig;
    Response::new(who, built.session_id, request.hash(), results)
        .unwrap()
        .as_bytes()
        .to_vec()
}

fn health_result(number: u32, who: &Identity, built: &openbmt_owner::Built) -> ItemResult {
    let request = Request::from_bytes(&built.request).unwrap();
    let Item::HealthCheck { nonce } = &request.item(number).unwrap().item else {
        panic!()
    };
    ItemResult {
        number,
        verdict: Verdict::Approved(Payload::Health {
            sig: openbmt_session::health_proof(who, &built.session_id, number, nonce),
        }),
    }
}

#[test]
fn as_62_a_response_that_is_not_bound_to_the_request_is_refused() {
    let (mut rig, _, built) = one_refund_rig();
    let approver = identity(APPROVER_SECRET);
    let declined = |n| ItemResult {
        number: n,
        verdict: Verdict::Declined {
            code: DeclineCode::ByPerson,
            text: "no".to_string(),
        },
    };
    let before = rig.owner.snapshot().unwrap();
    let key1_before = rig.key1.to_bytes();

    // From another key.
    let other = identity([0x77; 32]);
    let response = forged(
        &rig,
        &built,
        &other,
        vec![declined(0), health_result(1, &other, &built)],
    );
    assert!(matches!(
        refused(rig.take(&response)),
        Refusal::Bind(openbmt_session::BindError::WrongApprover)
    ));
    // A missing item.
    let response = forged(&rig, &built, &approver, vec![declined(0)]);
    assert!(matches!(
        refused(rig.take(&response)),
        Refusal::Bind(openbmt_session::BindError::MissingItem { item: 1 })
    ));
    // An item that was not asked.
    let response = forged(
        &rig,
        &built,
        &approver,
        vec![
            declined(0),
            health_result(1, &approver, &built),
            declined(7),
        ],
    );
    assert!(matches!(
        refused(rig.take(&response)),
        Refusal::Bind(openbmt_session::BindError::UnknownItem { item: 7 })
    ));
    // A result of a kind that does not fit its item.
    let wrong = ItemResult {
        number: 0,
        verdict: Verdict::Approved(Payload::Health {
            sig: openbmt_core::identity::Sig([0; 64]),
        }),
    };
    let response = forged(
        &rig,
        &built,
        &approver,
        vec![wrong, health_result(1, &approver, &built)],
    );
    assert!(matches!(
        refused(rig.take(&response)),
        Refusal::Bind(openbmt_session::BindError::KindMismatch { item: 0 })
    ));
    // A health answer that does not verify.
    let bad_health = ItemResult {
        number: 1,
        verdict: Verdict::Approved(Payload::Health {
            sig: openbmt_core::identity::Sig([1; 64]),
        }),
    };
    let response = forged(&rig, &built, &approver, vec![declined(0), bad_health]);
    assert!(matches!(
        refused(rig.take(&response)),
        Refusal::Bind(openbmt_session::BindError::BadHealthProof { item: 1 })
    ));
    // A response to another request of the same session ID.
    let other_request = Request::new(
        &identity(OWNER_SECRET),
        built.session_id,
        wallet_id(),
        vec![openbmt_session::RequestItem {
            number: 0,
            item: Item::HealthCheck { nonce: [9; 32] },
        }],
    )
    .unwrap();
    let response = Response::new(
        &approver,
        built.session_id,
        other_request.hash(),
        vec![ItemResult {
            number: 0,
            verdict: Verdict::Declined {
                code: DeclineCode::ByPerson,
                text: "no".to_string(),
            },
        }],
    )
    .unwrap()
    .as_bytes()
    .to_vec();
    assert!(matches!(
        refused(rig.take(&response)),
        Refusal::Bind(openbmt_session::BindError::WrongRequest)
    ));
    // Another session's answer.
    let response = Response::new(
        &approver,
        [9; 32],
        Request::from_bytes(&built.request).unwrap().hash(),
        vec![declined(0), health_result(1, &approver, &built)],
    )
    .unwrap()
    .as_bytes()
    .to_vec();
    assert_eq!(refused(rig.take(&response)), Refusal::UnknownSession);
    // A changed byte.
    let honest = rig.answer_all(&built.request);
    let mut broken = honest.clone();
    broken[40] ^= 1;
    assert!(matches!(
        refused(rig.take(&broken)),
        Refusal::Bind(_) | Refusal::Frame(_) | Refusal::UnknownSession
    ));
    // Nothing changed, nothing was stored for any of them, and key 1 did not
    // sign.
    assert_eq!(rig.owner.snapshot().unwrap(), before);
    assert_eq!(rig.key1.to_bytes(), key1_before);
    assert_eq!(rig.key1_store.calls(), 0);
    // And the honest answer still goes through.
    ok(rig.take(&honest));
}

#[test]
fn as_62_a_file_that_is_not_a_response_is_refused() {
    let (mut rig, _, _) = one_refund_rig();
    assert!(matches!(refused(rig.take(b"junk")), Refusal::Frame(_)));
    let big = vec![0u8; openbmt_session::MAX_SESSION_BYTES + 1];
    assert!(matches!(refused(rig.take(&big)), Refusal::Frame(_)));
}

#[test]
fn as_62_a_file_for_another_transaction_is_returned_and_not_finished() {
    let (mut rig, id, built) = one_refund_rig();
    // The Key app, hostile, signs another transaction (to another address)
    // and offers it as the answer to the refund. Its signature is real.
    let approver = identity(APPROVER_SECRET);
    let coins = [coin(1, 0, 300_000)];
    let other = openbmt_spend::plan(
        wallet(),
        &coins,
        &openbmt_spend::SpendRequest {
            payments: vec![(external(77), 10_000)],
            fee_rate: 1_000,
            change_index: 0,
            min_confirmations: 1,
            send_all: false,
            must_spend: Vec::new(),
            lock_height: 0,
            change_position: openbmt_spend::ChangePosition::Last,
        },
    )
    .unwrap();
    let psbt = other.to_psbt(wallet()).unwrap().serialize();
    let mut key2 = key2_signer();
    let review = key2.review(&psbt, NOW).unwrap();
    let signed = key2
        .sign(review, &kit().accounts[1], NOW, |_| Ok::<(), String>(()))
        .unwrap();
    let results = vec![
        ItemResult {
            number: 0,
            verdict: Verdict::Approved(Payload::Spend { psbt: signed.psbt }),
        },
        health_result(1, &approver, &built),
    ];
    let response = forged(&rig, &built, &approver, results);
    let taken = ok(rig.take(&response));
    let ItemOutcome::Returned(reason) = &taken.outcomes[0].1 else {
        panic!("{:?}", taken.outcomes)
    };
    assert!(!reason.is_empty());
    assert!(matches!(stage_of(&rig, id), Stage::Returned { .. }));
    assert!(rig.owner.outbox().is_empty());
}

#[test]
fn as_62_a_spend_with_no_key_2_signature_is_returned() {
    let (mut rig, id, built) = one_refund_rig();
    let approver = identity(APPROVER_SECRET);
    // The original, unsigned file, as the "answer".
    let request = Request::from_bytes(&built.request).unwrap();
    let Item::Spend { psbt, .. } = &request.items()[0].item else {
        panic!()
    };
    let results = vec![
        ItemResult {
            number: 0,
            verdict: Verdict::Approved(Payload::Spend { psbt: psbt.clone() }),
        },
        health_result(1, &approver, &built),
    ];
    let response = forged(&rig, &built, &approver, results);
    let taken = ok(rig.take(&response));
    assert!(matches!(taken.outcomes[0].1, ItemOutcome::Returned(_)));
    assert!(matches!(stage_of(&rig, id), Stage::Returned { .. }));
    // A file that is not a PSBT at all.
    let (mut rig, id, built) = one_refund_rig();
    let results = vec![
        ItemResult {
            number: 0,
            verdict: Verdict::Approved(Payload::Spend {
                psbt: b"not a psbt".to_vec(),
            }),
        },
        health_result(1, &approver, &built),
    ];
    let response = forged(&rig, &built, &approver, results);
    let taken = ok(rig.take(&response));
    assert!(matches!(taken.outcomes[0].1, ItemOutcome::Returned(_)));
    assert!(matches!(stage_of(&rig, id), Stage::Returned { .. }));
}

#[test]
fn as_62_key_1_refusing_returns_the_line_with_its_rule() {
    let (mut rig, id, built) = one_refund_rig();
    // Key 1's own ceiling is below the refund. The Key app approves; key 1
    // refuses.
    rig.key1
        .set_ceilings(
            openbmt_signer::Ceilings {
                per_tx: 5_000,
                per_day: 5_000,
                per_week: 5_000,
            },
            openbmt_signer::FIRST_ALLOWLISTED,
            |_| Ok::<(), String>(()),
        )
        .unwrap();
    let response = rig.answer_all(&built.request);
    let taken = ok(rig.take(&response));
    let ItemOutcome::Returned(reason) = &taken.outcomes[0].1 else {
        panic!("{:?}", taken.outcomes)
    };
    assert!(reason.starts_with("SR-"), "{reason}");
    let Stage::Returned { reason: stored } = stage_of(&rig, id) else {
        panic!()
    };
    assert_eq!(&stored, reason);
}

#[test]
fn as_62_a_batch_settings_and_pairing_are_completed_and_verified() {
    let mut rig = Rig::new();
    ok(rig.owner.queue_settings(settings_with(1, &[1]), nop));
    ok(rig.owner.queue_batch(batch(1, 0, 3), 1_000, nop));
    let (pairing, request) = pair_request(&rig.owner, 1);
    let mut pairing = Some(pairing);
    ok(rig.owner.queue_pairing(&request, nop));
    let taken = rig.round(&[], 0);
    assert_eq!(taken.outcomes.len(), 4);
    assert!(taken
        .outcomes
        .iter()
        .take(3)
        .all(|(_, o)| matches!(o, ItemOutcome::Delivered(_))));
    // The batch carries both signatures.
    let roots = openbmt_core::batch::PinnedRoots::new(owner_id(), approver_id()).unwrap();
    let mut seen = 0;
    for d in rig.owner.outbox() {
        match &d.what {
            Delivery::Batch(bytes) => {
                openbmt_core::batch::SignedBatch::from_bytes(bytes)
                    .unwrap()
                    .verify(&roots, &policy())
                    .unwrap();
                seen += 1;
            }
            Delivery::Settings(bytes) => {
                let s = openbmt_core::settings::SignedSettings::from_bytes(bytes).unwrap();
                s.verify(&roots, None).unwrap();
                assert!(s.approver_sig.is_some());
                seen += 1;
            }
            Delivery::Pairing(bytes) => {
                let signed = openbmt_core::pairing::SignedPairing::from_bytes(bytes).unwrap();
                // The counter accepts it.
                pairing
                    .take()
                    .unwrap()
                    .finish(&signed, true, NOW + 1)
                    .unwrap();
                seen += 1;
            }
            Delivery::Transaction { .. } => panic!(),
        }
    }
    assert_eq!(seen, 3);
    // The finished entries left the queue.
    assert!(rig.owner.batches().is_empty());
    assert!(rig.owner.queued_settings().is_empty());
    assert!(rig.owner.pairings().is_empty());
    assert_eq!(rig.owner.known_settings().unwrap().version, 1);
}

#[test]
fn as_62_a_pairing_version_above_what_the_owner_signed_is_returned() {
    use openbmt_core::pairing::{check_request, PairOffer, PairRequest};
    let approver = identity(APPROVER_SECRET);
    for (version, ok_version) in [(99u32, false), (1, true)] {
        let mut rig = Rig::new();
        // The owner has signed settings of version 1, now in force. They
        // list another counter: one that is listed is paired already
        // (AS-69).
        ok(rig.owner.queue_settings(settings_with(1, &[2]), nop));
        rig.round(&[], 0);
        let (_, request) = pair_request(&rig.owner, 1);
        let id = ok(rig.owner.queue_pairing(&request, nop));
        let built = rig.build(&[], 0);
        let offer = PairOffer::new(owner_id(), approver_id(), wallet_id(), NET).unwrap();
        let pending = check_request(&offer, &PairRequest::from_bytes(&request).unwrap()).unwrap();
        let (_, sig) = pending.approve(&approver, version).unwrap();
        let results = vec![
            ItemResult {
                number: 0,
                verdict: Verdict::Approved(Payload::Pairing {
                    settings_version: version,
                    sig,
                }),
            },
            health_result(1, &approver, &built),
        ];
        let response = forged(&rig, &built, &approver, results);
        let taken = ok(rig.take(&response));
        if ok_version {
            assert!(matches!(taken.outcomes[0].1, ItemOutcome::Delivered(_)));
        } else {
            let ItemOutcome::Returned(reason) = &taken.outcomes[0].1 else {
                panic!("{:?}", taken.outcomes)
            };
            assert!(reason.contains("higher settings version"), "{reason}");
            let p = rig.owner.pairings().iter().find(|p| p.id == id).unwrap();
            assert!(matches!(p.stage, Stage::Returned { .. }));
            assert!(rig
                .owner
                .outbox()
                .iter()
                .all(|d| !matches!(d.what, Delivery::Pairing(_))));
        }
    }
}

// ---- AS-63 -----------------------------------------------------------------------------

#[test]
fn as_63_a_declined_line_is_returned_and_not_retried_by_itself() {
    let (mut rig, id, built) = one_refund_rig();
    let response = rig.answer(&built.request, |l| {
        if l.kind == ItemKind::Spend {
            Choice::Decline("not this one".to_string())
        } else {
            Choice::Approve
        }
    });
    let taken = ok(rig.take(&response));
    let ItemOutcome::Returned(reason) = &taken.outcomes[0].1 else {
        panic!()
    };
    assert!(reason.contains("not this one"), "{reason}");
    assert!(reason.contains("declined on the Key app"), "{reason}");
    assert!(matches!(stage_of(&rig, id), Stage::Returned { .. }));
    // The next session does not take it.
    let next = rig.build(&[coin(2, 1, 300_000)], 1);
    assert!(spends(&next.request).is_empty());
    // Requeued, it goes in the session after that.
    ok(rig.owner.requeue(id, nop));
    assert_eq!(stage_of(&rig, id), Stage::Queued);
    let again = rig.build(&[coin(2, 1, 300_000)], 1);
    assert_eq!(spends(&again.request).len(), 1);
}

#[test]
fn as_63_a_returned_refund_can_be_cancelled_and_frees_its_amount() {
    let (mut rig, id, built) = one_refund_rig();
    let response = rig.answer(&built.request, |l| {
        if l.kind == ItemKind::Spend {
            Choice::Decline("no".to_string())
        } else {
            Choice::Approve
        }
    });
    ok(rig.take(&response));
    let sale1 = sale(1);
    assert_eq!(rig.owner.refunded_sats(&sale1), 10_000);
    ok(rig.owner.cancel(id, nop));
    assert_eq!(rig.owner.refunded_sats(&sale1), 0);
}

#[test]
fn as_63_every_decline_code_comes_back_in_words() {
    for (code, words) in [
        (DeclineCode::ByPerson, "declined on the Key app"),
        (DeclineCode::ChecksFailed, "the Key app's checks failed"),
        (DeclineCode::NotOnThisDevice, "not on this device"),
        (DeclineCode::NotDecided, "not decided"),
    ] {
        let (mut rig, _, built) = one_refund_rig();
        let approver = identity(APPROVER_SECRET);
        let results = vec![
            ItemResult {
                number: 0,
                verdict: Verdict::Declined {
                    code,
                    text: "text".to_string(),
                },
            },
            health_result(1, &approver, &built),
        ];
        let response = forged(&rig, &built, &approver, results);
        let taken = ok(rig.take(&response));
        let ItemOutcome::Returned(reason) = &taken.outcomes[0].1 else {
            panic!()
        };
        assert_eq!(reason, &format!("{words}: text"));
    }
}

#[test]
fn as_63_a_key_app_without_key_2_returns_every_spend() {
    let mut rig = Rig::with(Options::default(), false);
    let id = refund(&mut rig.owner, 1, 50_000, 10_000);
    ok(rig.owner.queue_batch(batch(1, 0, 3), 1_000, nop));
    ok(rig.owner.queue_settings(settings_with(1, &[1]), nop));
    let built = rig.build(&[coin(1, 0, 300_000)], 0);
    let response = rig.answer_all(&built.request);
    let taken = ok(rig.take(&response));
    let ItemOutcome::Returned(reason) = &taken.outcomes[0].1 else {
        panic!("{:?}", taken.outcomes)
    };
    assert!(reason.starts_with("not on this device"));
    assert!(matches!(stage_of(&rig, id), Stage::Returned { .. }));
    // The settings and the batch still work on such a Key app.
    assert!(taken.outcomes[1..]
        .iter()
        .all(|(_, o)| !matches!(o, ItemOutcome::Returned(_))));
}

// ---- AS-64 -----------------------------------------------------------------------------

#[test]
fn as_64_a_failed_store_loses_nothing_and_the_same_response_can_be_taken_again() {
    let (mut rig, id, built) = one_refund_rig();
    let response = rig.answer_all(&built.request);
    let before = rig.owner.snapshot().unwrap();
    let key1_before = rig.key1.to_bytes();
    rig.owner_store.fail_next(true);
    let err = rig.take(&response).expect_err("the store fails");
    assert_eq!(err, OwnerError::Store("disk full".to_string()));
    assert_eq!(rig.owner.snapshot().unwrap(), before);
    assert_eq!(rig.key1.to_bytes(), key1_before);
    assert!(matches!(stage_of(&rig, id), Stage::InSession { .. }));
    assert_eq!(rig.owner.open_sessions(), vec![built.session_id]);
    rig.owner_store.fail_next(false);
    ok(rig.take(&response));
    assert!(matches!(stage_of(&rig, id), Stage::Signed { .. }));
}

#[test]
fn as_64_the_results_are_stored_with_the_queue_and_key_1s_state() {
    let (mut rig, id, built) = one_refund_rig();
    let response = rig.answer_all(&built.request);
    ok(rig.take(&response));
    // What was stored restores to the same queue, with the transaction
    // waiting; and key 1's stored state is the one that counted the spend.
    let restored = restore_owner(&rig.owner_store.last()).unwrap();
    assert_eq!(restored.outbox(), rig.owner.outbox());
    assert!(matches!(
        restored
            .refunds()
            .iter()
            .find(|r| r.id == id)
            .unwrap()
            .stage,
        Stage::Signed { .. }
    ));
    assert_eq!(rig.key1_store.last(), rig.key1.to_bytes());
    assert_eq!(rig.key1.log().count(), 1);
}

#[test]
fn as_54_a_refund_is_paid_only_when_its_transaction_is_confirmed() {
    let (mut rig, id, built) = one_refund_rig();
    let response = rig.answer_all(&built.request);
    ok(rig.take(&response));
    let Stage::Signed { txid } = stage_of(&rig, id) else {
        panic!()
    };
    ok(rig.owner.mark_broadcast(&txid, nop));
    assert!(matches!(stage_of(&rig, id), Stage::Sent { .. }));
    // Not paid: still counted, and not cancellable.
    assert_eq!(rig.owner.refunded_sats(&sale(1)), 10_000);
    assert!(matches!(
        refused(rig.owner.cancel(id, nop)),
        Refusal::WrongStage(_)
    ));
    // Dropped: back to returned, still counting.
    ok(rig.owner.mark_dropped(&txid, nop));
    assert!(matches!(stage_of(&rig, id), Stage::Returned { .. }));
    assert_eq!(rig.owner.refunded_sats(&sale(1)), 10_000);
    assert!(rig.owner.outbox().is_empty());
    assert_eq!(
        refused(rig.owner.mark_confirmed(&txid, nop)),
        Refusal::UnknownTransaction(txid)
    );
}

#[test]
fn as_54_a_confirmed_refund_is_paid_counts_and_can_be_archived() {
    let (mut rig, id, built) = one_refund_rig();
    let response = rig.answer_all(&built.request);
    ok(rig.take(&response));
    let Stage::Signed { txid } = stage_of(&rig, id) else {
        panic!()
    };
    let tx_bytes = rig
        .owner
        .outbox()
        .iter()
        .find_map(|d| match &d.what {
            Delivery::Transaction { txid: held, tx, .. } if *held == txid => Some(tx.clone()),
            _ => None,
        })
        .unwrap();
    let tx: Transaction = deserialize(&tx_bytes).unwrap();
    ok(rig.owner.mark_confirmed(&txid, nop));
    assert!(matches!(stage_of(&rig, id), Stage::Paid { .. }));
    assert!(rig.owner.outbox().is_empty());
    assert_eq!(rig.owner.refunded_sats(&sale(1)), 10_000);
    assert_eq!(
        ok(rig
            .owner
            .archive_paid(&std::collections::BTreeSet::from([txid]), nop)),
        0
    );
    assert!(matches!(stage_of(&rig, id), Stage::Paid { .. }));
    assert!(ok(rig.owner.reopen_paid(&tx, nop)));
    assert!(matches!(stage_of(&rig, id), Stage::Sent { txid: paid } if paid == txid));
    assert!(matches!(
        &rig.owner.outbox()[0].what,
        Delivery::Transaction { txid: held, tx: exact, .. }
            if *held == txid && exact == &tx_bytes
    ));
    ok(rig.owner.mark_confirmed(&txid, nop));
    assert!(matches!(stage_of(&rig, id), Stage::Paid { .. }));
    // More than what is left is refused, paid or not.
    assert!(matches!(
        refused(
            rig.owner
                .queue_refund(sale(1), "Mia", external(3), 40_001, None, nop)
        ),
        Refusal::MoreThanReceived { .. }
    ));
    // Archived, the record goes and the amount stays counted.
    assert_eq!(
        ok(rig
            .owner
            .archive_paid(&std::collections::BTreeSet::new(), nop)),
        1
    );
    assert!(rig.owner.refunds().is_empty());
    assert!(!ok(rig.owner.reopen_paid(&tx, nop)));
    assert_eq!(rig.owner.refunded_sats(&sale(1)), 10_000);
    assert!(matches!(
        refused(
            rig.owner
                .queue_refund(sale(1), "Mia", external(3), 40_001, None, nop)
        ),
        Refusal::MoreThanReceived { .. }
    ));
    assert_eq!(
        refused(rig.owner.forget_sale(&sale(1), nop)),
        Refusal::SaleInUse
    );
}

#[test]
fn as_64_finished_objects_wait_until_delivered() {
    let mut rig = Rig::new();
    ok(rig.owner.queue_batch(batch(1, 0, 3), 1_000, nop));
    ok(rig.owner.queue_settings(settings_with(1, &[1]), nop));
    rig.round(&[], 0);
    assert_eq!(rig.owner.outbox().len(), 2);
    let ids: Vec<u32> = rig.owner.outbox().iter().map(|d| d.id).collect();
    ok(rig.owner.mark_delivered(ids[0], nop));
    assert_eq!(rig.owner.outbox().len(), 1);
    assert_eq!(
        refused(rig.owner.mark_delivered(ids[0], nop)),
        Refusal::UnknownEntry(ids[0])
    );
}

#[test]
fn as_64_a_transaction_is_not_delivered_it_is_followed() {
    let (mut rig, _, built) = one_refund_rig();
    let response = rig.answer_all(&built.request);
    ok(rig.take(&response));
    let id = rig.owner.outbox()[0].id;
    assert!(matches!(
        refused(rig.owner.mark_delivered(id, nop)),
        Refusal::WrongStage(_)
    ));
}

// ---- AS-66 -----------------------------------------------------------------------------

#[test]
fn as_66_an_abandoned_session_puts_its_entries_back_and_a_lost_answer_costs_nothing_twice() {
    let (mut rig, id, built) = one_refund_rig();
    // The Key app finished the session, and the answer never arrived.
    let lost = rig.answer_all(&built.request);
    ok(rig
        .owner
        .abandon_session(&built.session_id, |b| rig.owner_store.put(b)));
    assert!(rig.owner.open_sessions().is_empty());
    assert_eq!(stage_of(&rig, id), Stage::Queued);
    assert_eq!(refused(rig.take(&lost)), Refusal::UnknownSession);
    // Asking again, with the same coins: the same transaction, counted once
    // by key 2 and given the same signatures.
    let again = rig.build(&[coin(1, 0, 300_000)], 0);
    // The change position comes from fresh random bytes, so the order of the
    // outputs may differ: another transaction over the same inputs, which the
    // signer counts together with the first (SR-33).
    let (first, second) = (&spends(&built.request)[0], &spends(&again.request)[0]);
    assert_eq!(first.input, second.input);
    assert_eq!(first.lock_time, second.lock_time);
    let mut a = paid(first);
    let mut b = paid(second);
    a.sort();
    b.sort();
    assert_eq!(a, b);
    let response = rig.answer_all(&again.request);
    ok(rig.take(&response));
    assert!(matches!(stage_of(&rig, id), Stage::Signed { .. }));
    // Key 2's own log counted the transaction once.
    assert_eq!(rig.approver.signer().unwrap().log().count(), 1);
    assert_eq!(
        refused(rig.owner.abandon_session(&built.session_id, nop)),
        Refusal::UnknownSession
    );
}

// ---- AS-67 -----------------------------------------------------------------------------

#[test]
fn as_67_the_outputs_are_the_frozen_lines_in_order_and_the_change() {
    let options = Options {
        packing: RefundPacking::OneTransaction,
        ..Options::default()
    };
    let mut rig = Rig::with(options, true);
    for n in 1..=3 {
        refund(&mut rig.owner, n, 50_000, 10_000 + u64::from(n));
    }
    ok(rig
        .owner
        .queue_cash_out(external(9), CashAmount::Sats(20_000), "Bank", nop));
    let taken: Taken = rig.round(&[coin(1, 0, 300_000), coin(2, 1, 300_000)], 4);
    assert_eq!(taken.outcomes.len(), 3);
    let mut txs = Vec::new();
    for d in rig.owner.outbox() {
        if let Delivery::Transaction { tx, .. } = &d.what {
            txs.push(deserialize::<Transaction>(tx).unwrap());
        }
    }
    assert_eq!(txs.len(), 2);
    assert_eq!(paid(&txs[0]).len(), 4);
    let refunds = without_change(&txs[0]);
    assert_eq!(refunds.len(), 3);
    for n in 1..=3u8 {
        assert_eq!(
            refunds[usize::from(n) - 1],
            (external(n), 10_000 + u64::from(n))
        );
    }
    change_position(&txs[0], 4);
    assert_eq!(paid(&txs[1]).len(), 2);
    assert_eq!(without_change(&txs[1]), vec![(external(9), 20_000)]);
    change_position(&txs[1], 5);
    let _: DeviceId = rig.owner.owner_id();
}

// ---- AS-61, AS-67: the lock time and the place of the change ----------------------------

/// Builds sessions, abandoning each, until the first transaction has its
/// change to change index `index` at `want`.
fn build_with_change_at(
    rig: &mut Rig,
    coins: &[openbmt_spend::Coin],
    index: u32,
    want: usize,
) -> openbmt_owner::Built {
    for _ in 0..80 {
        let built = rig.build(coins, index);
        let tx = &spends(&built.request)[0];
        if change_position(tx, index) == want {
            return built;
        }
        ok(rig
            .owner
            .abandon_session(&built.session_id, |b| rig.owner_store.put(b)));
    }
    panic!("no random bytes put the change at {want}");
}

fn one_each_rig() -> Rig {
    let options = Options {
        packing: RefundPacking::OneEach,
        ..Options::default()
    };
    Rig::with(options, true)
}

fn three_refunds_rig() -> Rig {
    let options = Options {
        packing: RefundPacking::OneTransaction,
        ..Options::default()
    };
    let mut rig = Rig::with(options, true);
    for n in 1..=3 {
        refund(&mut rig.owner, n, 50_000, 10_000 + u64::from(n));
    }
    rig
}

#[test]
fn as_61_every_transaction_has_the_lock_height_and_the_same_request_the_same_bytes() {
    let mut rig = one_each_rig();
    rig.lock_height = 851_000;
    for n in 1..=2 {
        refund(&mut rig.owner, n, 50_000, 10_000);
    }
    ok(rig
        .owner
        .queue_cash_out(external(9), CashAmount::Sats(20_000), "Bank", nop));
    let coins = [
        coin(1, 0, 300_000),
        coin(2, 1, 300_000),
        coin(3, 2, 300_000),
    ];
    let built = rig.build(&coins, 0);
    let txs = spends(&built.request);
    assert_eq!(txs.len(), 3);
    for tx in &txs {
        assert_eq!(tx.lock_time.to_consensus_u32(), 851_000);
        assert!(tx.is_explicitly_rbf());
    }
    // Another rig in the same state, with the same random bytes, asks the
    // same bytes.
    let mut twin = one_each_rig();
    twin.lock_height = 851_000;
    for n in 1..=2 {
        refund(&mut twin.owner, n, 50_000, 10_000);
    }
    ok(twin
        .owner
        .queue_cash_out(external(9), CashAmount::Sats(20_000), "Bank", nop));
    assert_eq!(twin.build(&coins, 0).request, built.request);
}

#[test]
fn as_61_the_change_lands_at_every_place_and_the_labels_name_the_real_places() {
    let mut rig = three_refunds_rig();
    let coins = [coin(1, 0, 300_000)];
    let mut seen = std::collections::BTreeSet::new();
    for _ in 0..60 {
        let built = rig.build(&coins, 2);
        let request = Request::from_bytes(&built.request).unwrap();
        let Item::Spend { psbt, labels } = &request.items()[0].item else {
            panic!()
        };
        let tx = Psbt::deserialize(psbt).unwrap().unsigned_tx;
        let at = change_position(&tx, 2);
        seen.insert(at);
        // Ascending labels: the three refunds at the places of the payments
        // and the change at its own place.
        assert_eq!(labels.len(), 4);
        assert!(labels.windows(2).all(|w| w[0].output < w[1].output));
        for (n, l) in labels.iter().enumerate() {
            assert_eq!(l.output as usize, n);
            if n == at {
                assert_eq!(l.label, Label::Change);
            } else {
                assert!(matches!(l.label, Label::Refund { .. }));
            }
        }
        ok(rig
            .owner
            .abandon_session(&built.session_id, |b| rig.owner_store.put(b)));
    }
    assert_eq!(seen.len(), 4, "every place of four was met: {seen:?}");
}

#[test]
fn as_61_two_open_sessions_do_not_reuse_inputs_and_abandon_releases_them() {
    let options = Options {
        packing: RefundPacking::OneEach,
        ..Options::default()
    };
    let mut rig = Rig::with(options, true);
    let first_id = refund(&mut rig.owner, 1, 50_000, 10_000);
    let second_id = refund(&mut rig.owner, 2, 50_000, 10_000);
    let coins = [coin(1, 0, 300_000)];

    let first = rig.build(&coins, 0);
    let first_tx = spends(&first.request).remove(0);
    assert_eq!(
        stage_of(&rig, first_id),
        Stage::InSession {
            session: first.session_id
        }
    );

    // The second session is allowed, but it cannot build a spend from an
    // input already frozen into the first open session.
    let second = rig.build(&coins, 1);
    assert!(spends(&second.request).is_empty());
    assert!(!second.skipped.is_empty());
    assert_eq!(second.skipped[0].words().heading, "Coins held for approval");
    assert_eq!(stage_of(&rig, second_id), Stage::Queued);

    ok(rig
        .owner
        .abandon_session(&first.session_id, |b| rig.owner_store.put(b)));
    let after = rig.build(&coins, 2);
    let after_spends = spends(&after.request);
    assert_eq!(after_spends.len(), 1);
    assert_eq!(after_spends[0].input, first_tx.input);
}

#[test]
fn as_61_reserved_coins_are_named_without_claiming_they_are_the_only_shortfall() {
    let options = Options {
        packing: RefundPacking::OneEach,
        ..Options::default()
    };
    let mut rig = Rig::with(options, true);
    refund(&mut rig.owner, 1, 50_000, 10_000);
    let queued_id = refund(&mut rig.owner, 2, 100_000, 100_000);

    let first = rig.build(&[coin(1, 0, 150_000)], 0);
    assert_eq!(spends(&first.request).len(), 1);

    // The remaining 50,000 sat is old enough but cannot cover the second
    // refund. The message must describe both the reserved and shortfall parts.
    let second = rig.build(&[coin(1, 0, 150_000), coin(2, 1, 50_000)], 1);
    assert!(spends(&second.request).is_empty());
    let skipped = second
        .skipped
        .iter()
        .find(|entry| entry.id == queued_id)
        .expect("the second refund remains queued");
    let words = skipped.words();
    assert_eq!(words.heading, "Some coins are held");
    assert!(words.what.contains("rest cannot cover"));
    assert!(words.next.to_lowercase().contains("lower this amount"));
}

#[test]
fn as_61_declining_a_session_releases_its_inputs_for_the_next_request() {
    let options = Options {
        packing: RefundPacking::OneEach,
        ..Options::default()
    };
    let mut rig = Rig::with(options, true);
    let declined_id = refund(&mut rig.owner, 1, 50_000, 10_000);
    let queued_id = refund(&mut rig.owner, 2, 50_000, 10_000);
    let coins = [coin(1, 0, 300_000)];

    let first = rig.build(&coins, 0);
    assert_eq!(spends(&first.request).len(), 1);
    let response = rig.answer(&first.request, |_| Choice::Decline("declined".to_string()));
    ok(rig.take(&response));
    assert!(matches!(
        stage_of(&rig, declined_id),
        Stage::Returned { .. }
    ));

    // The declined session has closed and created no waiting transaction.
    // A stale caller coin list can now fund the other queued refund.
    let next = rig.build(&coins, 1);
    let next_spends = spends(&next.request);
    assert_eq!(next_spends.len(), 1);
    assert_eq!(without_change(&next_spends[0])[0].0, external(2));
    assert_eq!(
        stage_of(&rig, queued_id),
        Stage::InSession {
            session: next.session_id
        }
    );
}

#[test]
fn as_61_a_signed_outbox_transaction_reserves_inputs_from_stale_coin_lists() {
    let options = Options {
        packing: RefundPacking::OneEach,
        ..Options::default()
    };
    let mut rig = Rig::with(options, true);
    refund(&mut rig.owner, 1, 50_000, 10_000);
    let queued_id = refund(&mut rig.owner, 2, 50_000, 10_000);
    let coins = [coin(1, 0, 300_000)];

    let first = rig.build(&coins, 0);
    let first_tx = spends(&first.request).remove(0);
    let response = rig.answer_all(&first.request);
    ok(rig.take(&response));
    assert_eq!(rig.owner.outbox().len(), 1);

    let next = rig.build(&coins, 1);
    assert!(spends(&next.request).is_empty());
    assert!(!next.skipped.is_empty());
    assert_eq!(stage_of(&rig, queued_id), Stage::Queued);
    assert_eq!(rig.owner.outbox().len(), 1);
    assert_eq!(first_tx.input.len(), 1);
}

#[test]
fn as_61_the_transactions_of_one_session_have_places_of_their_own() {
    // Each transaction takes its own seed from the session's bytes, so
    // across many sessions the second transaction's place is not tied to
    // the first's.
    let mut rig = one_each_rig();
    for n in 1..=2 {
        refund(&mut rig.owner, n, 50_000, 10_000);
    }
    let coins = [coin(1, 0, 300_000), coin(2, 1, 300_000)];
    let mut pairs = std::collections::BTreeSet::new();
    for _ in 0..80 {
        let built = rig.build(&coins, 0);
        let txs = spends(&built.request);
        assert_eq!(txs.len(), 2);
        pairs.insert((change_position(&txs[0], 0), change_position(&txs[1], 1)));
        ok(rig
            .owner
            .abandon_session(&built.session_id, |b| rig.owner_store.put(b)));
    }
    assert_eq!(pairs.len(), 4, "{pairs:?}");
}

#[test]
fn as_67_a_transaction_with_the_change_first_or_in_the_middle_is_finished() {
    for want in [0usize, 1, 2, 3] {
        let mut rig = three_refunds_rig();
        let built = build_with_change_at(&mut rig, &[coin(1, 0, 300_000)], 4, want);
        let response = rig.answer_all(&built.request);
        let taken = ok(rig.take(&response));
        assert!(
            matches!(taken.outcomes[0].1, ItemOutcome::Delivered(_)),
            "place {want}"
        );
        let mut txs = Vec::new();
        for d in rig.owner.outbox() {
            if let Delivery::Transaction { tx, .. } = &d.what {
                txs.push(deserialize::<Transaction>(tx).unwrap());
            }
        }
        assert_eq!(txs.len(), 1);
        let tx = &txs[0];
        assert_eq!(tx.lock_time.to_consensus_u32(), rig.lock_height);
        assert_eq!(change_position(tx, 4), want);
        let refunds = without_change(tx);
        assert_eq!(refunds.len(), 3);
        for n in 1..=3u8 {
            assert_eq!(
                refunds[usize::from(n) - 1],
                (external(n), 10_000 + u64::from(n))
            );
        }
    }
}

#[test]
fn as_67_a_response_for_a_transaction_with_the_change_moved_is_not_taken() {
    // Hostile: the answer is for the same inputs and payments with the
    // change at another place. It is signed validly, but it is another
    // request's answer and not the session's.
    let mut rig = three_refunds_rig();
    let coins = [coin(1, 0, 300_000)];
    let asked = build_with_change_at(&mut rig, &coins, 4, 0);
    let mut twin = three_refunds_rig();
    let moved = build_with_change_at(&mut twin, &coins, 4, 3);
    assert_ne!(
        spends(&moved.request)[0].compute_txid(),
        spends(&asked.request)[0].compute_txid()
    );
    let response = twin.answer_all(&moved.request);
    assert!(rig.take(&response).is_err());
    assert!(rig.owner.outbox().is_empty());
}
