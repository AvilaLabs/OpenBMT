//! The address ranges the owner issued, kept for good (OC-10, OC-11): which
//! counter a receive index was given to is the owner's own record, not a
//! counter's word.

mod common;

use common::*;
use openbmt_core::identity::DeviceId;
use openbmt_owner::{Delivery, IssuedRange, Refusal, RestoreError, MAX_ISSUED_RANGES};

/// The bytes of a snapshot with these issued ranges in place of the ones it
/// holds. The list is the last part of the snapshot.
fn with_ranges(snapshot: &[u8], held: usize, ranges: &[(DeviceId, u32, u32)]) -> Vec<u8> {
    let cut = snapshot.len() - 4 - held * 48;
    let mut out = snapshot[..cut].to_vec();
    out.extend_from_slice(&u32::try_from(ranges.len()).unwrap().to_le_bytes());
    for (counter, start, count) in ranges {
        out.extend_from_slice(&counter.to_bytes());
        out.extend_from_slice(&start.to_le_bytes());
        out.extend_from_slice(&count.to_le_bytes());
        out.extend_from_slice(&1_000u64.to_le_bytes());
    }
    out
}

fn c1() -> DeviceId {
    counter(1).device_id()
}

fn c2() -> DeviceId {
    counter(2).device_id()
}

#[test]
fn oc_10_the_bound_is_the_documented_one() {
    assert_eq!(MAX_ISSUED_RANGES, 65_536);
}

#[test]
fn oc_10_the_range_is_recorded_in_the_same_write_as_the_batch() {
    let mut o = new_owner(Default::default());
    let store = Store::default();
    ok(o.queue_batch(batch(1, 0, 3), 1_000, |b| store.put(b)));
    // One write holds both the queued batch and its range.
    assert_eq!(store.calls(), 1);
    let stored = restore_owner(&store.last()).unwrap();
    assert_eq!(stored.batches().len(), 1);
    assert_eq!(
        stored.issued_ranges(),
        &[IssuedRange {
            start: 0,
            count: 3,
            counter: c1(),
            signed_at: 1_000
        }]
    );
    // A write that fails records nothing.
    let mut o2 = new_owner(Default::default());
    let failing = Store::default();
    failing.fail_next(true);
    assert!(o2.queue_batch(batch(1, 0, 3), 1_000, |b| failing.put(b)).is_err());
    assert!(o2.issued_ranges().is_empty());
    assert_eq!(o2.counter_of(0), None);
    assert!(o.counter_of(0).is_some());
}

#[test]
fn oc_10_the_range_survives_delivery_and_a_restart() {
    let mut rig = Rig::new();
    ok(rig.owner.queue_settings(settings_with(1, &[1]), nop));
    ok(rig.owner.queue_batch(batch(1, 0, 3), 1_000, nop));
    let built = rig.build(&[], 0);
    let response = rig.answer_all(&built.request);
    ok(rig.take(&response));
    let waiting = rig
        .owner
        .outbox()
        .iter()
        .find(|d| matches!(d.what, Delivery::Batch(_)))
        .map(|d| d.id)
        .expect("the signed batch waits to be delivered");
    assert_eq!(rig.owner.receive_mark(), 3);
    ok(rig.owner.mark_delivered(waiting, nop));
    // Delivered: it has left the queue and the outbox, and the range stays.
    assert!(rig.owner.batches().is_empty());
    assert!(rig
        .owner
        .outbox()
        .iter()
        .all(|d| !matches!(d.what, Delivery::Batch(_))));
    assert_eq!(rig.owner.counter_of(1), Some(c1()));
    assert_eq!(rig.owner.receive_mark(), 3);
    let restored = restore_owner(&rig.owner.snapshot().unwrap()).unwrap();
    assert_eq!(restored.counter_of(1), Some(c1()));
    assert_eq!(restored.receive_mark(), 3);
    assert_eq!(restored.snapshot().unwrap(), rig.owner.snapshot().unwrap());
}

#[test]
fn oc_10_a_cancelled_batch_keeps_its_range() {
    let mut o = new_owner(Default::default());
    let id = ok(o.queue_batch(batch(1, 0, 3), 1_000, nop));
    ok(o.cancel(id, nop));
    assert!(o.batches().is_empty());
    assert_eq!(o.counter_of(0), Some(c1()));
    assert_eq!(o.receive_mark(), 3);
}

#[test]
fn oc_10_counter_of_answers_at_the_edges_of_a_range() {
    let mut o = new_owner(Default::default());
    ok(o.queue_batch(batch(1, 0, 3), 1_000, nop));
    ok(o.queue_batch(batch(2, 6, 4), 1_000, nop));
    assert_eq!(o.counter_of(0), Some(c1()), "first index of a range");
    assert_eq!(o.counter_of(2), Some(c1()), "last index of a range");
    assert_eq!(o.counter_of(3), None, "one past the range");
    assert_eq!(o.counter_of(5), None, "between two ranges");
    assert_eq!(o.counter_of(6), Some(c2()));
    assert_eq!(o.counter_of(9), Some(c2()));
    assert_eq!(o.counter_of(10), None);
    assert_eq!(o.counter_of(u32::MAX), None);
    // The ranges are in order of their start, however they were issued.
    let mut o = new_owner(Default::default());
    ok(o.queue_batch(batch(2, 6, 4), 1_000, nop));
    ok(o.queue_batch(batch(1, 0, 3), 1_000, nop));
    assert_eq!(o.counter_of(1), Some(c1()));
    assert_eq!(o.counter_of(7), Some(c2()));
    assert_eq!(o.issued_ranges().first().map(|r| r.start), Some(0));
}

#[test]
fn oc_10_the_receive_mark_stays_after_every_batch_is_delivered() {
    let mut rig = Rig::new();
    ok(rig.owner.queue_settings(settings_with(1, &[1, 2]), nop));
    ok(rig.owner.queue_batch(batch(1, 0, 3), 1_000, nop));
    ok(rig.owner.queue_batch(batch(2, 3, 3), 1_000, nop));
    let built = rig.build(&[], 0);
    let response = rig.answer_all(&built.request);
    ok(rig.take(&response));
    let waiting: Vec<u32> = rig
        .owner
        .outbox()
        .iter()
        .filter(|d| matches!(d.what, Delivery::Batch(_)))
        .map(|d| d.id)
        .collect();
    assert_eq!(waiting.len(), 2);
    for id in waiting {
        ok(rig.owner.mark_delivered(id, nop));
    }
    assert!(rig.owner.batches().is_empty());
    assert!(rig
        .owner
        .outbox()
        .iter()
        .all(|d| !matches!(d.what, Delivery::Batch(_))));
    assert_eq!(rig.owner.receive_mark(), 6);
}

#[test]
fn oc_11_a_batch_that_overlaps_an_issued_range_is_refused() {
    let mut o = new_owner(Default::default());
    ok(o.queue_batch(batch(1, 3, 3), 1_000, nop));
    let before = o.snapshot().unwrap();
    // Overlapping at the start, at the end, inside, around, and the same
    // range for another counter.
    for (start, count, n) in [(1, 3, 2), (5, 3, 2), (4, 3, 1), (2, 6, 2), (3, 3, 2)] {
        assert_eq!(
            refused(o.queue_batch(batch(n, start, count), 1_000, nop)),
            Refusal::BatchOverlaps { start, count },
            "{start}+{count}"
        );
        assert_eq!(o.snapshot().unwrap(), before);
    }
    let text = Refusal::BatchOverlaps { start: 1, count: 3 }.to_string();
    assert!(text.starts_with("OC-11:"), "{text}");
    // Touching ranges do not overlap.
    ok(o.queue_batch(batch(2, 0, 3), 1_000, nop));
    ok(o.queue_batch(batch(2, 6, 3), 1_000, nop));
    assert_eq!(o.issued_ranges().len(), 3);
}

#[test]
fn oc_11_the_range_list_is_bounded_at_65536() {
    let o = new_owner(Default::default());
    let empty = o.snapshot().unwrap();
    // A full list: 65,536 ranges of three indices, each for counter 1.
    let ranges: Vec<_> = (0..65_536u32).map(|i| (c1(), i * 3, 3)).collect();
    let full = with_ranges(&empty, 0, &ranges);
    let mut o = restore_owner(&full).unwrap();
    assert_eq!(o.issued_ranges().len(), MAX_ISSUED_RANGES);
    assert_eq!(o.receive_mark(), 65_536 * 3);
    let err = refused(o.queue_batch(batch(1, 65_536 * 3, 3), 1_000, nop));
    assert_eq!(err, Refusal::IssuedRangesFull);
    assert!(err.to_string().starts_with("OC-11:"));
    assert!(o.batches().is_empty());
    assert_eq!(o.snapshot().unwrap(), full);
    // One more than the bound is not a snapshot.
    let mut more = ranges.clone();
    more.push((c1(), 65_536 * 3, 3));
    assert!(restore_owner(&with_ranges(&empty, 0, &more)).is_err());
}

#[test]
fn oc_10_the_stored_ranges_are_read_strictly() {
    let mut o = new_owner(Default::default());
    ok(o.queue_batch(batch(1, 0, 3), 1_000, nop));
    let snap = o.snapshot().unwrap();
    // Cut and trailing byte.
    assert!(restore_owner(&snap[..snap.len() - 1]).is_err());
    let mut longer = snap.clone();
    longer.push(0);
    assert!(restore_owner(&longer).is_err());
    // Overlapping, out of order, empty and past the last index.
    for bad in [
        vec![(c1(), 0, 3), (c2(), 2, 3)],
        vec![(c1(), 6, 3), (c2(), 0, 3)],
        vec![(c1(), 0, 0)],
        vec![(c1(), u32::MAX - 1, 3)],
    ] {
        assert!(restore_owner(&with_ranges(&snap, 1, &bad)).is_err());
    }
    // A queued batch that no issued range holds.
    for other in [vec![], vec![(c2(), 0, 3)], vec![(c1(), 0, 4)]] {
        match restore_owner(&with_ranges(&snap, 1, &other)) {
            Err(RestoreError::Inconsistent(_)) => {}
            other => panic!("{other:?}"),
        }
    }
    // A range with no batch is fine: ranges outlive batches.
    let extra = [(c1(), 0, 3), (c2(), 9, 3)];
    let kept = restore_owner(&with_ranges(&snap, 1, &extra)).unwrap();
    assert_eq!(kept.counter_of(10), Some(c2()));
    // Every flipped byte of the list is refused or read back the same.
    let from = snap.len() - 44;
    for at in from..snap.len() {
        let mut flipped = snap.clone();
        flipped[at] ^= 0x01;
        if let Ok(r) = restore_owner(&flipped) {
            assert_eq!(r.snapshot().unwrap(), flipped, "byte {at}");
        }
    }
}

#[test]
fn oc_22_the_time_the_batch_was_signed_is_kept_with_its_range() {
    let mut o = new_owner(Default::default());
    let store = Store::default();
    ok(o.queue_batch(batch(1, 0, 3), 5_000, |b| store.put(b)));
    ok(o.queue_batch(batch(1, 3, 3), 9_000, |b| store.put(b)));
    let times: Vec<u64> = o.issued_ranges().iter().map(|r| r.signed_at).collect();
    assert_eq!(times, vec![5_000, 9_000]);
    // The stored form carries them, and a restart reads them back.
    let back = restore_owner(&store.last()).unwrap();
    assert_eq!(back.issued_ranges(), o.issued_ranges());
    // A snapshot cut inside the time is refused.
    let bytes = store.last();
    assert!(restore_owner(&bytes[..bytes.len() - 3]).is_err());
}
