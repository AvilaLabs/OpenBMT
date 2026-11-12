//! Section 15: the record that says the counter holds a batch (CR-167 to
//! CR-169).

use openbmt_core::batch::{AddError, AddressBatch, BatchError, SignedBatch};
use openbmt_core::records::{Record, RecordKind};
use openbmt_core::wire::DecodeError;

use super::kit::*;
use crate::{batch_digest, BatchHeld, BatchTakeRefused, Counter};

/// A batch for the same range as `batch(start, count)` with other addresses,
/// signed by the same roots.
fn other(start: u32, count: u32) -> SignedBatch {
    let scripts = (start..start + count)
        .map(|i| p2wsh(i as u8 ^ 0x80))
        .collect();
    let batch = AddressBatch {
        wallet_checksum: [9; 32],
        counter: id(3).device_id(),
        start,
        count,
        scripts,
    };
    SignedBatch {
        owner_sig: batch.sign(&id(1)),
        approver_sig: batch.sign(&id(2)),
        batch,
    }
}

fn held_of(b: &SignedBatch) -> BatchHeld {
    BatchHeld::of(b).unwrap()
}

// CR-167.

#[test]
fn cr_167_a_taken_batch_writes_one_setting_record_between_the_check_and_the_store() {
    let mut w = World::new();
    let before = w.c.records_len();
    let b = batch(100, 50);
    w.c.take_batch(b.clone(), &mut w.m).unwrap();
    assert_eq!(w.c.records_len(), before + 1);
    assert_eq!(
        w.m.log,
        vec!["record".to_string(), "Batches".to_string()],
        "the record first, the batches last"
    );
    let stored = Record::from_bytes(w.m.records.last().unwrap()).unwrap();
    assert_eq!(stored.kind, RecordKind::Setting);
    let held = BatchHeld::decode(&stored.body).unwrap();
    assert_eq!((held.start, held.end), (100, 150));
    assert_eq!(held.digest, batch_digest(&b));
    assert_eq!(held, held_of(&b));
    assert_eq!(held.kind(), RecordKind::Setting);
}

#[test]
fn cr_167_the_digest_covers_the_range_and_both_signatures() {
    let b = batch(100, 50);
    assert_eq!(batch_digest(&b), batch_digest(&b.clone()));
    assert_ne!(batch_digest(&b), batch_digest(&batch(101, 50)));
    assert_ne!(batch_digest(&b), batch_digest(&other(100, 50)));
    let mut swapped = b.clone();
    std::mem::swap(&mut swapped.owner_sig, &mut swapped.approver_sig);
    assert_ne!(batch_digest(&b), batch_digest(&swapped));
}

#[test]
fn cr_167_a_refused_batch_writes_no_record() {
    let mut w = World::new();
    let before = w.c.records_len();
    // Overlaps the first batch (0..100).
    let err = w.c.take_batch(batch(50, 100), &mut w.m).unwrap_err();
    assert_eq!(
        err,
        BatchTakeRefused::Add(AddError::Refused(BatchError::OverlapsExisting))
    );
    assert_eq!(err.rule(), "CR-168");
    assert!(err.to_string().starts_with("CR-168"));
    // Another counter's batch.
    let mut wrong = batch(100, 50);
    wrong.batch.counter = id(4).device_id();
    assert!(w.c.take_batch(wrong, &mut w.m).is_err());
    assert_eq!(w.c.records_len(), before);
    assert!(w.m.records.is_empty());
    assert!(w.m.log.is_empty(), "nothing was written: {:?}", w.m.log);
}

#[test]
fn cr_167_a_record_that_cannot_be_written_leaves_everything_as_it_was() {
    let mut w = World::new();
    let before = w.c.records_len();
    w.m.fail_on = Some(1);
    let err = w.c.take_batch(batch(100, 50), &mut w.m).unwrap_err();
    assert!(matches!(err, BatchTakeRefused::Record(_)));
    assert_eq!(err.rule(), "CR-167");
    assert!(err.to_string().starts_with("CR-167"));
    assert_eq!(w.c.records_len(), before);
    w.m.reset();
    // The batch can still be taken, and then is recorded once.
    w.c.take_batch(batch(100, 50), &mut w.m).unwrap();
    assert_eq!(w.c.records_len(), before + 1);
}

// CR-168.

#[test]
fn cr_168_the_same_batch_again_changes_nothing_and_writes_no_second_record() {
    let mut w = World::new();
    let b = batch(100, 50);
    w.c.take_batch(b.clone(), &mut w.m).unwrap();
    let records = w.c.records_len();
    let head = w.c.head();
    let batches = w.c.batches().to_bytes();
    w.m.reset();
    w.c.take_batch(b.clone(), &mut w.m).unwrap();
    assert!(w.m.log.is_empty(), "nothing written: {:?}", w.m.log);
    assert_eq!(w.c.records_len(), records);
    assert_eq!(w.c.head(), head);
    assert_eq!(w.c.batches().to_bytes(), batches);
}

#[test]
fn cr_168_a_restart_does_not_change_it() {
    let mut w = World::new();
    let b = batch(100, 50);
    w.c.take_batch(b.clone(), &mut w.m).unwrap();
    let records = w.c.records_len();
    let mut again = Counter::load(id(3), pairing(), settings(), w.m.stored(), &w.m).unwrap();
    w.m.reset();
    again.take_batch(b, &mut w.m).unwrap();
    assert_eq!(again.records_len(), records);
    assert!(w.m.log.is_empty(), "nothing written: {:?}", w.m.log);
}

#[test]
fn cr_168_a_batch_that_overlaps_a_held_one_and_differs_is_refused() {
    let mut w = World::new();
    w.c.take_batch(batch(100, 50), &mut w.m).unwrap();
    let records = w.c.records_len();
    w.m.reset();
    // The same range with other addresses, and a range that overlaps it.
    for b in [other(100, 50), other(120, 50), batch(120, 50)] {
        let err = w.c.take_batch(b, &mut w.m).unwrap_err();
        assert_eq!(
            err,
            BatchTakeRefused::Add(AddError::Refused(BatchError::OverlapsExisting))
        );
    }
    assert_eq!(w.c.records_len(), records);
    assert!(w.m.log.is_empty());
}

#[test]
fn cr_168_a_store_that_failed_after_the_record_is_finished_by_the_second_try() {
    let mut w = World::new();
    let b = batch(100, 50);
    let before = w.c.records_len();
    // The record is write 1, the batches write 2.
    w.m.fail_on = Some(2);
    let err = w.c.take_batch(b.clone(), &mut w.m).unwrap_err();
    assert!(matches!(err, BatchTakeRefused::Add(AddError::Persist(_))));
    assert_eq!(w.c.records_len(), before + 1, "the record stays");
    w.m.reset();
    w.c.take_batch(b.clone(), &mut w.m).unwrap();
    assert_eq!(w.c.records_len(), before + 1, "and is not written twice");
    assert_eq!(w.m.log, vec!["Batches".to_string()]);
    // Now it is held: a third try is nothing.
    w.m.reset();
    w.c.take_batch(b, &mut w.m).unwrap();
    assert!(w.m.log.is_empty());
}

// CR-169.

#[test]
fn cr_169_the_body_is_read_strictly_and_reencodes_byte_for_byte() {
    let good = held_of(&batch(100, 50)).encode();
    assert_eq!(good.len(), 41);
    assert_eq!(good[0], 34);
    assert_eq!(good[1..5], 100u32.to_le_bytes());
    assert_eq!(good[5..9], 150u32.to_le_bytes());
    let read = BatchHeld::decode(&good).unwrap();
    assert_eq!(read.encode(), good);
    for n in 0..good.len() {
        assert!(BatchHeld::decode(&good[..n]).is_err(), "cut at {n}");
    }
    let mut longer = good.clone();
    longer.push(0);
    assert_eq!(BatchHeld::decode(&longer), Err(DecodeError::TrailingBytes));
    for tag in (0..=255u8).filter(|t| *t != 34) {
        let mut bad = good.clone();
        bad[0] = tag;
        assert!(BatchHeld::decode(&bad).is_err(), "tag {tag}");
    }
    // The 97-byte body of a confirmation (CR-243) is not a batch's.
    assert!(BatchHeld::decode(&[36u8; 97]).is_err());
    // An end that is not above the start.
    for (start, end) in [(5u32, 5u32), (9, 5)] {
        let mut bad = good.clone();
        bad[1..5].copy_from_slice(&start.to_le_bytes());
        bad[5..9].copy_from_slice(&end.to_le_bytes());
        assert!(BatchHeld::decode(&bad).is_err());
    }
}

#[test]
fn cr_169_the_record_is_not_read_as_a_sale_or_a_cashier_event() {
    let body = held_of(&batch(100, 50)).encode();
    assert!(crate::Event::decode(&body).is_err());
    assert!(crate::StaffEvent::decode(&body).is_err());
    // A counter that has the record in its log starts and counts nothing
    // from it.
    let mut w = World::new();
    w.c.take_batch(batch(100, 50), &mut w.m).unwrap();
    let again = Counter::load(id(3), pairing(), settings(), w.m.stored(), &w.m).unwrap();
    assert_eq!(again.records_len(), w.c.records_len());
    assert_eq!(again.head(), w.c.head());
}
