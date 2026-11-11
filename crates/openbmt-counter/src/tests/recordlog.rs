//! Section 8: records that leave the counter's memory (CR-81).

use openbmt_core::records::{Chain, Record, GENESIS};

use super::kit::*;
use crate::{
    Actor, Counter, LoadError, RefundAsk, RefundRequest, Role, StoreFault, KEEP_RECORDS,
    MAX_UNSENT_IN_MEMORY,
};

fn grow(w: &mut World, n: u64) {
    for _ in 0..n {
        w.c.check_in(&mut w.m).unwrap();
    }
}

fn stored_records(m: &Mem) -> Vec<Record> {
    m.records
        .iter()
        .map(|b| Record::from_bytes(b).unwrap())
        .collect()
}

fn read_all(c: &Counter, m: &Mem) -> Vec<Record> {
    let mut out: Vec<Record> = Vec::new();
    loop {
        let page = c.records_from(out.len() as u64, 700, m).unwrap();
        if page.is_empty() {
            return out;
        }
        out.extend(page);
    }
}

#[test]
fn cr_81_memory_holds_the_bound_while_the_mark_moves_and_the_chain_reads_back() {
    let mut w = World::new();
    for step in 1..=50u64 {
        grow(&mut w, 100);
        // The owner takes everything now and then, and nothing in between.
        if step % 2 == 0 {
            w.c.set_owner_mark(w.c.records_len());
        }
        let held = w.c.records_in_memory() as u64;
        let above_mark = w.c.records_len() - w.c.owner_mark().unwrap_or(0);
        assert!(
            held <= KEEP_RECORDS.max(above_mark),
            "{held} held at {}",
            w.c.records_len()
        );
        assert_eq!(w.c.records_len() - w.c.records_base(), held);
    }
    assert_eq!(w.c.records_len(), 5_000);
    assert!(w.c.records_base() >= 3_000);
    // All of it reads back, equal to what was stored, and verifies.
    let all = read_all(&w.c, &w.m);
    assert_eq!(all, stored_records(&w.m));
    let devices = openbmt_core::records::DeviceList::new([id(3).device_id()]);
    assert_eq!(Chain::from_records(all).verify(&devices), Ok(()));
    // One record, from below the base and from above it.
    let low = w.c.record_at(5, &w.m).unwrap().unwrap();
    assert_eq!(low.seq, 5);
    let high = w.c.record_at(4_999, &w.m).unwrap().unwrap();
    assert_eq!(high.hash(), w.c.head());
    assert_eq!(w.c.record_at(5_000, &w.m), Ok(None));
    // A call is bounded.
    let page = w.c.records_from(0, usize::MAX, &w.m).unwrap();
    assert_eq!(page.len(), crate::MAX_READ);
}

#[test]
fn cr_81_records_above_the_mark_stay_up_to_the_cap_and_the_rest_are_read() {
    let mut w = World::new();
    grow(&mut w, MAX_UNSENT_IN_MEMORY + 600);
    let len = w.c.records_len();
    // No mark yet: the newest MAX_UNSENT_IN_MEMORY stay.
    assert_eq!(w.c.records_in_memory() as u64, MAX_UNSENT_IN_MEMORY);
    assert_eq!(w.c.records_base(), 600);
    // A mark far below does not bring them back, and the oldest are read.
    w.c.set_owner_mark(100);
    assert_eq!(w.c.records_in_memory() as u64, MAX_UNSENT_IN_MEMORY);
    let from_disk = w.c.records_from(590, 20, &w.m).unwrap();
    assert_eq!(from_disk, stored_records(&w.m)[590..610].to_vec());
    // A mark close to the head lets memory shrink to KEEP_RECORDS.
    w.c.set_owner_mark(len);
    assert_eq!(w.c.records_in_memory() as u64, KEEP_RECORDS);
    // The mark going down does not bring records back into memory.
    w.c.set_owner_mark(10);
    assert_eq!(w.c.records_in_memory() as u64, KEEP_RECORDS);
    assert_eq!(w.c.records_base(), len - KEEP_RECORDS);
}

#[test]
fn cr_81_the_hash_below_the_base_is_read_from_the_store() {
    let mut w = World::new();
    grow(&mut w, 3_000);
    w.c.set_owner_mark(3_000);
    assert_eq!(w.c.records_base(), 3_000 - KEEP_RECORDS);
    let all = stored_records(&w.m);
    assert_eq!(w.c.hash_at(0, &w.m), Ok(Some(GENESIS)));
    assert_eq!(w.c.hash_at(10, &w.m), Ok(Some(all[9].hash())));
    // Just at the base, and in memory, and at the end.
    let base = w.c.records_base();
    assert_eq!(
        w.c.hash_at(base, &w.m),
        Ok(Some(all[base as usize - 1].hash()))
    );
    assert_eq!(
        w.c.hash_at(base + 5, &w.m),
        Ok(Some(all[base as usize + 4].hash()))
    );
    assert_eq!(w.c.hash_at(3_000, &w.m), Ok(Some(w.c.head())));
    assert_eq!(w.c.hash_at(3_001, &w.m), Ok(None));
    // Memory alone answers for a hash in memory.
    let reads = w.m.reads.get();
    assert!(w.c.hash_at(2_900, &w.m).is_ok());
    assert_eq!(w.m.reads.get(), reads);
}

#[test]
fn cr_81_a_failed_or_damaged_read_below_the_base_changes_nothing_in_memory() {
    let mut w = World::new();
    grow(&mut w, 2_500);
    w.c.set_owner_mark(2_500);
    let before = (
        w.c.records_len(),
        w.c.head(),
        w.c.records_base(),
        w.c.records_in_memory(),
    );
    w.m.fail_reads = true;
    assert!(matches!(w.c.record_at(5, &w.m), Err(StoreFault::Read(_))));
    assert!(matches!(w.c.hash_at(5, &w.m), Err(StoreFault::Read(_))));
    assert!(matches!(
        w.c.records_from(0, 10, &w.m),
        Err(StoreFault::Read(_))
    ));
    // What is in memory still answers without the store.
    assert!(w.c.record_at(2_400, &w.m).unwrap().is_some());
    w.m.fail_reads = false;
    // A record that was changed on the disk is found when it is read.
    let n = w.m.records[7].len();
    w.m.records[7][n - 70] ^= 1;
    assert!(matches!(
        w.c.record_at(7, &w.m),
        Err(StoreFault::Damaged { seq: 7, .. })
    ));
    assert!(matches!(
        w.c.records_from(5, 5, &w.m),
        Err(StoreFault::Damaged { seq: 7, .. })
    ));
    // Cut short: it does not parse.
    w.m.records[8].truncate(10);
    assert!(matches!(
        w.c.record_at(8, &w.m),
        Err(StoreFault::Damaged { seq: 8, .. })
    ));
    // A record that is missing from the log.
    let kept = w.m.records.len();
    w.m.records.truncate(9);
    assert!(matches!(
        w.c.record_at(20, &w.m),
        Err(StoreFault::Damaged { seq: 20, .. })
    ));
    assert_eq!(kept, 2_500);
    let after = (
        w.c.records_len(),
        w.c.head(),
        w.c.records_base(),
        w.c.records_in_memory(),
    );
    assert_eq!(before, after);
}

#[test]
fn cr_81_the_last_record_read_must_be_the_one_memory_follows() {
    let mut w = World::new();
    grow(&mut w, 2_500);
    w.c.set_owner_mark(2_500);
    let base = w.c.records_base() as usize;
    // The record just below the base is replaced by another that this
    // counter signed: it is genuine and in place, but memory does not follow
    // it.
    let other = Record::build(
        &id(3),
        base as u64 - 1,
        stored_records(&w.m)[base - 2].hash(),
        openbmt_core::records::RecordKind::Sale,
        b"x".to_vec(),
    )
    .unwrap();
    w.m.records[base - 1] = other.to_bytes();
    assert!(matches!(
        w.c.hash_at(base as u64, &w.m),
        Err(StoreFault::Damaged { .. })
    ));
}

#[test]
fn cr_81_a_restart_streams_the_chain_and_keeps_only_the_tail() {
    let mut w = World::new();
    grow(&mut w, 5_000);
    let (len, head) = (w.c.records_len(), w.c.head());
    let reads_before = w.m.reads.get();
    w.restart();
    assert_eq!(w.c.records_len(), len);
    assert_eq!(w.c.head(), head);
    // The log was read in chunks, not all at once.
    assert!(w.m.reads.get() - reads_before >= 5_000 / 256);
    // No mark yet: all of it is above the (unknown) mark and under the cap.
    assert_eq!(w.c.records_in_memory(), 5_000);
    w.c.set_owner_mark(len);
    assert_eq!(w.c.records_in_memory() as u64, KEEP_RECORDS);
    assert_eq!(read_all(&w.c, &w.m), stored_records(&w.m));
    // More than the cap: only the newest are held after the start.
    let mut big = World::new();
    grow(&mut big, MAX_UNSENT_IN_MEMORY + 10);
    big.restart();
    assert_eq!(big.c.records_in_memory() as u64, MAX_UNSENT_IN_MEMORY);
    assert_eq!(big.c.records_len(), MAX_UNSENT_IN_MEMORY + 10);
    assert_eq!(big.c.records_base(), 10);
}

#[test]
fn cr_81_a_damaged_record_below_the_base_is_found_at_the_start() {
    let mut w = World::new();
    grow(&mut w, 3_000);
    w.c.set_owner_mark(3_000);
    // A changed record in the middle of the log.
    let mut m = Mem {
        slots: w.m.slots.clone(),
        records: w.m.records.clone(),
        ..Mem::default()
    };
    let n = m.records[40].len();
    m.records[40][n - 70] ^= 1;
    let err = Counter::load(id(3), pairing(), settings(), m.stored(), &m).unwrap_err();
    assert!(matches!(err, LoadError::Records(_)), "{err:?}");
    assert_eq!(err.rule(), "CR-42");
    // A record that does not parse.
    let mut m = Mem {
        slots: w.m.slots.clone(),
        records: w.m.records.clone(),
        ..Mem::default()
    };
    m.records[41].truncate(9);
    let err = Counter::load(id(3), pairing(), settings(), m.stored(), &m).unwrap_err();
    assert!(matches!(err, LoadError::RecordsUnread(_)), "{err:?}");
    assert!(err.to_string().starts_with("CR-42: "));
    // A record taken out.
    let mut m = Mem {
        slots: w.m.slots.clone(),
        records: w.m.records.clone(),
        ..Mem::default()
    };
    m.records.remove(500);
    let err = Counter::load(id(3), pairing(), settings(), m.stored(), &m).unwrap_err();
    assert!(matches!(err, LoadError::Records(_)), "{err:?}");
    // A store that cannot be read does not start the counter.
    let mut m = Mem {
        slots: w.m.slots.clone(),
        records: w.m.records.clone(),
        ..Mem::default()
    };
    m.fail_reads = true;
    let err = Counter::load(id(3), pairing(), settings(), m.stored(), &m).unwrap_err();
    assert!(matches!(err, LoadError::RecordsUnread(_)), "{err:?}");
}

#[test]
fn cr_73_the_refunds_of_a_sale_survive_their_records_leaving_memory() {
    let mut w = World::new();
    let o = w.open(500);
    let t = w.pay(&o, 5_000);
    w.confirm(&[t], 95);
    w.judge(o.index);
    let req = |sats| RefundRequest {
        ask: RefundAsk::Sats(sats),
        destination: p2wsh(0xa1),
        actor: Actor {
            role: Role::Cashier,
            name: "ann".to_string(),
        },
        reason: "wrong size".to_string(),
    };
    assert_eq!(
        w.c.request_refund(o.index, req(1_000), w.now, &w.chain, &mut w.m),
        Ok(1)
    );
    grow(&mut w, 3_000);
    w.c.set_owner_mark(w.c.records_len());
    assert!(w.c.records_base() > 1_000);
    // The request's record is below the base; a second request is numbered
    // after it and both are still on the sale.
    assert_eq!(
        w.c.request_refund(o.index, req(500), w.now, &w.chain, &mut w.m),
        Ok(2)
    );
    assert_eq!(w.c.refund_views(o.index).len(), 2);
    // A start from the store reads the same history.
    let again = w.m.reload();
    assert_eq!(again.refund_views(o.index), w.c.refund_views(o.index));
}
