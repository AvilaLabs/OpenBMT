//! A payment received, the line that shows it, a refund queued on the
//! wallet's own facts, an approval built for the Key app, the Key app's
//! answer taken and the finished payment handed to the driver, through
//! `dispatch` alone, and a restart after it.

use super::kit::*;
use super::screen::check_wire_words;
use super::wire::{Rd, Send, Words, W};

const KIND_FEE_UNKNOWN: u8 = 2;
const STAGE_QUEUED: u8 = 0;
const STAGE_SIGNED: u8 = 3;
const STAGE_SENT: u8 = 4;
const OUTCOME_DELIVERED: u8 = 0;
const SEND_SENT: u8 = 1;

fn words(w: &Words) {
    check_wire_words(w).unwrap_or_else(|why| panic!("{why}: {w:?}"));
}

fn send_states(rig: &Rig, handle: u64) -> Vec<Send> {
    let response = rig.call(23, handle, &[]);
    let mut r = Rd::ok(&response);
    let rows = r.list(Rd::send);
    r.done();
    rows
}

fn queue(rig: &Rig, handle: u64) -> super::wire::Queue {
    let response = rig.call(40, handle, &[]);
    let mut r = Rd::ok(&response);
    let q = r.queue();
    r.done();
    q
}

#[test]
fn a_receipt_a_refund_an_approval_and_a_payment_to_the_driver() {
    let rig = Rig::new();
    let handle = rig.start();
    assert_eq!(handle, 1);

    // Before the network answers: the view is not at the tip and the health
    // answer is not known. Both come with words.
    let response = rig.call(20, handle, &[]);
    let mut r = Rd::ok(&response);
    let status = r.status();
    r.done();
    assert!(!status.health.0, "no health answer yet");
    words(&status.view_words);
    words(&status.receipts_words);
    words(&status.health.3);

    rig.send(vec![peers_event()]);
    assert_eq!(rig.pump(handle), (vec![], false));

    // The owner signs a batch for the counter; the wallet watches it.
    let batch_id = rig.queue_batch(handle);
    let q = queue(&rig, handle);
    assert_eq!(q.others.len(), 1);
    assert_eq!((q.others[0].0, q.others[0].1), (batch_id, 0), "a batch");
    assert_eq!(q.others[0].2.kind, STAGE_QUEUED);
    words(&q.others[0].2.words);

    // A customer pays receive address 0 and the payment is buried.
    let pay = rig.fund(handle, 300_000);
    let response = rig.call(20, handle, &[]);
    let mut r = Rd::ok(&response);
    let status = r.status();
    r.done();
    assert_eq!(status.view, 0, "ready");
    assert_eq!(status.checked, 6);
    assert_eq!(status.tip, 6);
    assert_eq!(status.balance, 300_000);
    assert!(status.halted.is_none() && !status.recovery_pending);
    words(&status.view_words);
    words(&status.receipts_words);

    // The receipts line shows the payment.
    let response = rig.call(21, handle, &[]);
    let mut r = Rd::ok(&response);
    let (kind, payments, _, _, line) = r.receipts();
    r.done();
    assert_eq!((kind, payments), (KIND_FEE_UNKNOWN, 1));
    words(&line);

    // A refund, on the wallet's own facts: what the sale received comes from
    // the checked chain, whatever the counter said.
    let response = rig.call(
        41,
        handle,
        &W::new()
            .refund(T0, &counter_bytes(), 0, "Mia", &external_address(9), 10_000)
            .0,
    );
    let mut r = Rd::ok(&response);
    let (refund_id, received, deep) = (r.u32(), r.u64(), r.bool());
    r.done();
    assert_eq!((received, deep), (300_000, true));
    let q = queue(&rig, handle);
    assert_eq!(q.refunds.len(), 1);
    let row = &q.refunds[0];
    assert_eq!((row.id, row.sale, row.sats), (refund_id, 0, 10_000));
    assert_eq!(row.cashier, "Mia");
    assert_eq!(row.address, external_address(9));
    assert_eq!(row.counter, counter_bytes());
    assert_eq!(row.stage.kind, STAGE_QUEUED);
    assert_eq!(row.received, Some(300_000));
    words(&row.stage.words);

    // The approval for the Key app.
    let response = rig.call(30, handle, &W::new().u64(T0).u64(1_000).0);
    let mut r = Rd::ok(&response);
    let (session, request, skipped) = r.built();
    r.done();
    assert_eq!(session.len(), 32);
    assert!(!request.is_empty());
    for (_, _, w) in &skipped {
        words(w);
    }
    let response = rig.call(33, handle, &[]);
    let mut r = Rd::ok(&response);
    assert_eq!(r.list(Rd::bytes), vec![session.clone()]);
    r.done();
    let response = rig.call(34, handle, &W::new().bytes(&session).0);
    let mut r = Rd::ok(&response);
    assert_eq!(r.bytes(), request);
    r.done();
    assert_eq!(queue(&rig, handle).refunds[0].stage.kind, 1, "in a session");

    // The Key app answers; the response is taken.
    let mut approver = key_app();
    let answer = answer(&mut approver, &request, T0);
    let response = rig.call(32, handle, &W::new().u64(T0).bytes(&answer).0);
    let mut r = Rd::ok(&response);
    let (taken_session, outcomes, sends) = r.taken();
    r.done();
    assert_eq!(taken_session, session);
    assert!(
        outcomes.iter().any(|o| o.1 == OUTCOME_DELIVERED),
        "{outcomes:?}"
    );
    for o in &outcomes {
        words(&o.4);
    }
    assert_eq!(sends.len(), 1);
    assert_eq!(sends[0].state, SEND_SENT);
    words(&sends[0].words);

    // The finished payment went to the driver, and the node follows it.
    let txid = sends[0].txid.clone();
    assert!(
        rig.calls().contains(&format!("main broadcast {txid}")),
        "{:?}",
        rig.calls()
    );
    assert_eq!(send_states(&rig, handle), sends);
    let q = queue(&rig, handle);
    let stage = &q.refunds[0].stage;
    assert!(
        matches!(stage.kind, STAGE_SIGNED | STAGE_SENT),
        "{:?}",
        stage
    );
    assert_eq!(stage.detail.as_deref(), Some(txid.as_str()));
    // Key 1's state was written as the response was taken.
    let key1 = std::fs::read(rig.dir().join("key1.bin")).expect("Key 1's file");
    assert!(!key1.is_empty());
    // The same response is not taken twice.
    let again = Rd::error(&rig.call(32, handle, &W::new().u64(T0).bytes(&answer).0))
        .expect("refused the second time");
    words(&again.words);

    // The payment spends the wallet's coin, and the paid receipt stays.
    assert_ne!(txid, pay.compute_txid().to_string());

    // A restart on the same folder keeps the queue, the outbox and Key 1.
    Rd::ok(&rig.call(2, handle, &[])).pump();
    rig.arm();
    let handle = rig.start();
    let q = queue(&rig, handle);
    assert_eq!(q.refunds.len(), 1);
    assert_eq!(q.refunds[0].stage.detail.as_deref(), Some(txid.as_str()));
    assert_eq!(
        std::fs::read(rig.dir().join("key1.bin")).expect("file"),
        key1
    );
    Rd::ok(&rig.call(2, handle, &[])).pump();
}
