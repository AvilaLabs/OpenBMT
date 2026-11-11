//! Section 8: letting old sales go (CR-76 to CR-80).

use bitcoin::hashes::Hash;
use bitcoin::{ScriptBuf, WScriptHash};
use openbmt_core::batch::{AddressBatch, SignedBatch};
use openbmt_core::records::RecordKind;

use super::kit::*;
use crate::{
    Actor, Event, OpenRefused, RefundAsk, RefundRequest, RefundState, Role, Slot, StoreFault,
    EXPIRY_SECS, MAX_SETTLE_PER_CALL, SETTLE_SECS,
};

const DAY: u64 = 86_400;

fn ann() -> Actor {
    Actor {
        role: Role::Cashier,
        name: "ann".to_string(),
    }
}

/// A script for every index, however many.
fn wide_script(i: u32) -> ScriptBuf {
    let mut bytes = [0x5a; 32];
    bytes[..4].copy_from_slice(&i.to_le_bytes());
    ScriptBuf::new_p2wsh(&WScriptHash::from_byte_array(bytes))
}

fn wide_batch(count: u32) -> SignedBatch {
    let batch = AddressBatch {
        wallet_checksum: [9; 32],
        counter: id(3).device_id(),
        start: 0,
        count,
        scripts: (0..count).map(wide_script).collect(),
    };
    SignedBatch {
        owner_sig: batch.sign(&id(1)),
        approver_sig: batch.sign(&id(2)),
        batch,
    }
}

/// The owner holds every record the counter has.
fn owner_holds_all(w: &mut World) {
    let all = w.c.records_len();
    w.c.set_owner_mark(all);
}

/// A sale the cashier cancelled at once.
fn ended_sale(w: &mut World) -> u32 {
    let o = w.open_five();
    w.c.cancel(o.index, w.now, &mut w.m).unwrap();
    o.index
}

/// Settle as the world's clock says, and give back the count.
fn settle(w: &mut World) -> usize {
    w.c.settle(&mut w.m, w.now).unwrap().count
}

fn request(sats: u64) -> RefundRequest {
    RefundRequest {
        ask: RefundAsk::Sats(sats),
        destination: p2wsh(0xa1),
        actor: ann(),
        reason: "wrong size".to_string(),
    }
}

// CR-76: every condition, and each one alone blocks.

#[test]
fn cr_76_a_sale_settles_when_every_condition_holds() {
    let mut w = World::new();
    let i = ended_sale(&mut w);
    owner_holds_all(&mut w);
    w.tick(SETTLE_SECS);
    let done = w.c.settle(&mut w.m, w.now).unwrap();
    assert_eq!((done.count, done.remaining), (1, false));
    assert!(w.c.sale(i).is_none());
}

#[test]
fn cr_76_a_live_sale_is_never_settled() {
    let mut w = World::new();
    let o = w.open_five();
    owner_holds_all(&mut w);
    w.tick(10 * SETTLE_SECS);
    assert_eq!(settle(&mut w), 0);
    assert!(w.c.sale(o.index).unwrap().is_live());
    // Judged, it ends as expired and then settles.
    assert_eq!(w.judge(o.index).state, crate::State::Expired);
    owner_holds_all(&mut w);
    assert_eq!(settle(&mut w), 0, "the end is fresh");
    w.tick(SETTLE_SECS);
    assert_eq!(settle(&mut w), 1);
}

#[test]
fn cr_76_a_sale_whose_last_change_is_too_recent_is_not_settled() {
    let mut w = World::new();
    let i = ended_sale(&mut w);
    owner_holds_all(&mut w);
    w.tick(SETTLE_SECS - 1);
    assert_eq!(settle(&mut w), 0);
    assert!(w.c.sale(i).is_some());
    w.tick(1);
    assert_eq!(settle(&mut w), 1);
}

#[test]
fn cr_76_a_late_payment_starts_the_wait_again() {
    let mut w = World::new();
    let a = w.open_five();
    w.c.cancel(a.index, w.now, &mut w.m).unwrap();
    w.tick(SETTLE_SECS - DAY);
    let t = w.pay(&a, 5_000);
    assert_eq!(w.see(&t, 1).reports.len(), 1);
    owner_holds_all(&mut w);
    w.tick(DAY);
    assert_eq!(settle(&mut w), 0, "the late payment is one day old");
    w.tick(SETTLE_SECS);
    assert_eq!(settle(&mut w), 1);
}

#[test]
fn cr_76_a_sale_with_a_refund_requested_is_not_settled() {
    let mut w = World::new();
    let o = w.open_five();
    let t = w.pay(&o, 5_000);
    w.confirm(&[t], 95);
    w.judge(o.index);
    w.c.request_refund(o.index, request(1_000), w.now, &w.chain, &mut w.m)
        .unwrap();
    assert_eq!(w.c.refund_views(o.index)[0].state, RefundState::Requested);
    owner_holds_all(&mut w);
    w.tick(SETTLE_SECS + DAY);
    assert_eq!(settle(&mut w), 0);
    // Withdrawn, the entry no longer blocks; the withdrawal is a record and
    // the clock runs from it.
    w.c.withdraw_refund(o.index, 1, ann(), w.now, &mut w.m)
        .unwrap();
    owner_holds_all(&mut w);
    assert_eq!(settle(&mut w), 0);
    w.tick(SETTLE_SECS);
    assert_eq!(settle(&mut w), 1);
}

#[test]
fn cr_76_a_sale_with_a_refund_waiting_for_depth_is_not_settled() {
    let mut w = World::new();
    let o = w.open_five();
    let t = w.pay(&o, 5_000);
    let block = w.confirm(&[t], 95);
    assert_eq!(w.judge(o.index).state, crate::State::Final);
    w.orphan(&block);
    w.c.request_refund(o.index, request(1_000), w.now, &w.chain, &mut w.m)
        .unwrap();
    w.judge(o.index);
    assert_eq!(
        w.c.refund_views(o.index)[0].state,
        RefundState::WaitingForDepth
    );
    assert!(!w.c.sale(o.index).unwrap().is_live());
    owner_holds_all(&mut w);
    w.tick(SETTLE_SECS + DAY);
    assert_eq!(settle(&mut w), 0);
    assert!(w.c.sale(o.index).is_some());
}

#[test]
fn cr_76_a_record_above_the_owner_s_mark_blocks_the_sale() {
    let mut w = World::new();
    let i = ended_sale(&mut w);
    let all = w.c.records_len();
    w.tick(SETTLE_SECS);
    // The owner holds all but the last record, which is the sale's.
    w.c.set_owner_mark(all - 1);
    assert_eq!(settle(&mut w), 0);
    w.c.set_owner_mark(all);
    assert_eq!(settle(&mut w), 1);
    assert!(w.c.sale(i).is_none());
}

#[test]
fn cr_76_with_no_mark_from_the_owner_nothing_is_settled() {
    let mut w = World::new();
    ended_sale(&mut w);
    w.tick(10 * SETTLE_SECS);
    assert_eq!(w.c.owner_mark(), None);
    assert_eq!(settle(&mut w), 0);
    // A start forgets the mark.
    owner_holds_all(&mut w);
    w.restart();
    assert_eq!(w.c.owner_mark(), None);
    assert_eq!(settle(&mut w), 0);
}

#[test]
fn cr_76_a_mark_above_the_chain_is_taken_as_the_length() {
    let mut w = World::new();
    ended_sale(&mut w);
    w.c.set_owner_mark(1_000_000);
    assert_eq!(w.c.owner_mark(), Some(w.c.records_len()));
}

#[test]
fn cr_76_an_unreadable_sale_is_never_settled_and_its_slot_stays() {
    let mut w = World::new();
    let i = ended_sale(&mut w);
    // A second slot that does not read back.
    w.m.slots.insert(Slot::Sale(7), vec![1, 2, 3]);
    w.restart();
    assert_eq!(w.c.unreadable().collect::<Vec<_>>(), vec![7]);
    owner_holds_all(&mut w);
    w.tick(10 * SETTLE_SECS);
    assert_eq!(settle(&mut w), 1, "the readable one settles");
    assert!(w.c.sale(i).is_none());
    assert_eq!(w.c.unreadable().collect::<Vec<_>>(), vec![7]);
    assert!(w.m.slots.contains_key(&Slot::Sale(7)));
}

// CR-77: what settling does.

#[test]
fn cr_77_a_settled_sale_is_gone_from_memory_the_watch_list_and_the_store() {
    let mut w = World::new();
    let o = w.open_five();
    w.c.cancel(o.index, w.now, &mut w.m).unwrap();
    assert!(w.c.watched_scripts().contains(&o.script));
    owner_holds_all(&mut w);
    w.tick(SETTLE_SECS);
    let records = w.m.records.len();
    w.m.reset();
    let done = w.c.settle(&mut w.m, w.now).unwrap();
    assert_eq!((done.count, done.released), (1, 1));
    assert!(w.c.sale(o.index).is_none());
    assert_eq!(w.c.held_sales(), 0);
    assert!(!w.c.watched_scripts().contains(&o.script));
    assert!(!w.m.slots.contains_key(&Slot::Sale(o.index)));
    // The sale slot first, then the batches; no record.
    assert_eq!(w.m.log, ["delete Sale(0)", "Batches"]);
    assert_eq!(w.m.records.len(), records);
}

#[test]
fn cr_77_settling_survives_a_restart_and_the_index_is_never_handed_out_again() {
    let mut w = World::new();
    let first = ended_sale(&mut w);
    let kept = w.open_five();
    owner_holds_all(&mut w);
    w.tick(SETTLE_SECS);
    assert_eq!(settle(&mut w), 1);
    let watched = w.c.watched_scripts();
    w.restart();
    assert!(w.c.sale(first).is_none());
    assert_eq!(w.c.unreadable().count(), 0);
    assert_eq!(w.c.held_sales(), 1);
    assert_eq!(w.c.watched_scripts(), watched);
    assert!(w.c.sale(kept.index).is_some());
    // The next sales get new indexes, before and after another restart.
    let next = w.open_five();
    assert!(next.index > kept.index);
    w.restart();
    let after = w.open_five();
    assert!(after.index > next.index);
    assert_ne!(after.script, next.script);
}

#[test]
fn cr_77_a_stop_between_the_slot_and_the_memory_leaves_the_settled_state() {
    let mut w = World::new();
    let i = ended_sale(&mut w);
    // The slot is gone and the process stopped before memory changed.
    w.m.slots.remove(&Slot::Sale(i));
    w.restart();
    assert!(w.c.sale(i).is_none());
    assert_eq!(w.c.unreadable().count(), 0);
}

#[test]
fn cr_77_a_call_settles_at_most_the_bound_and_the_oldest_first() {
    let mut w = World::with_batch(wide_batch(400));
    let mut made = Vec::new();
    for _ in 0..300 {
        made.push(ended_sale(&mut w));
        // Past the hour's address allowance (CR-04) and the sale's own time.
        w.tick(3_600);
    }
    owner_holds_all(&mut w);
    w.tick(SETTLE_SECS);
    let done = w.c.settle(&mut w.m, w.now).unwrap();
    assert_eq!(done.count, MAX_SETTLE_PER_CALL);
    assert!(done.remaining);
    assert_eq!(w.c.held_sales(), 300 - MAX_SETTLE_PER_CALL);
    // The oldest went: the first 256 opened, the newest 44 stayed.
    for (n, index) in made.iter().enumerate() {
        assert_eq!(
            w.c.sale(*index).is_none(),
            n < MAX_SETTLE_PER_CALL,
            "sale {n}"
        );
    }
    let done = w.c.settle(&mut w.m, w.now).unwrap();
    assert_eq!((done.count, done.remaining), (44, false));
    assert_eq!(w.c.held_sales(), 0);
}

#[test]
fn cr_77_a_mark_that_goes_down_does_not_bring_a_settled_sale_back() {
    let mut w = World::new();
    let a = ended_sale(&mut w);
    owner_holds_all(&mut w);
    w.tick(SETTLE_SECS);
    assert_eq!(settle(&mut w), 1);
    // The owner lost its copy: the mark falls to nothing.
    w.c.set_owner_mark(0);
    assert!(w.c.sale(a).is_none());
    // A sale that has not settled yet waits for the owner to catch up.
    let b = ended_sale(&mut w);
    w.tick(SETTLE_SECS);
    assert_eq!(settle(&mut w), 0);
    owner_holds_all(&mut w);
    assert_eq!(settle(&mut w), 1);
    assert!(w.c.sale(b).is_none());
    // The records are all still here.
    assert!(w.c.records_len() >= 2);
}

// CR-78 and CR-80.

#[test]
fn cr_80_a_late_payment_to_a_settled_address_is_not_reported_by_the_counter() {
    let mut w = World::new();
    let o = w.open_five();
    w.c.cancel(o.index, w.now, &mut w.m).unwrap();
    owner_holds_all(&mut w);
    w.tick(SETTLE_SECS);
    assert_eq!(settle(&mut w), 1);
    let records = w.m.records.len();
    let t = w.pay(&o, 5_000);
    let sighted = w.see(&t, 1);
    assert!(sighted.reports.is_empty());
    assert!(sighted.results.is_empty());
    let block = w.mine_at(std::slice::from_ref(&t), 100);
    let taken = w.offer(&block);
    assert!(taken.reports.is_empty());
    assert_eq!(w.m.records.len(), records, "nothing was written");
    // Judging the settled sale is refused as for any index it does not hold.
    let facts = crate::Facts {
        chain: &w.chain,
        health: w.health,
        heartbeat: heartbeat(),
        remote_order: false,
        now: w.now,
    };
    assert!(w.c.judge(o.index, &facts, &mut w.m).is_err());
}

#[test]
fn cr_78_the_refund_numbers_in_the_records_still_count_after_a_settle() {
    let mut w = World::new();
    let o = w.open_five();
    let t = w.pay(&o, 5_000);
    w.confirm(&[t], 95);
    w.judge(o.index);
    let number =
        w.c.request_refund(o.index, request(1_000), w.now, &w.chain, &mut w.m)
            .unwrap();
    w.c.withdraw_refund(o.index, number, ann(), w.now, &mut w.m)
        .unwrap();
    owner_holds_all(&mut w);
    w.tick(SETTLE_SECS + DAY);
    assert_eq!(settle(&mut w), 1);
    w.restart();
    assert!(w.c.desk().last_refund_no() >= number);
}

// CR-79: addresses.

#[test]
fn cr_79_an_address_shown_and_never_used_leaves_the_watch_list_after_the_wait() {
    let mut w = World::new();
    // The sale's own write fails after the address was taken: the index is
    // wasted and the address was shown.
    w.m.reset();
    w.m.fail_on = Some(3);
    let facts = w.open_facts();
    let err =
        w.c.open(
            crate::Amount(500),
            usd(),
            rate(),
            &fresh(facts.now),
            facts,
            &mut w.m,
        )
        .unwrap_err();
    assert!(matches!(err, OpenRefused::Store(StoreFault::Write(_))));
    w.m.reset();
    let wasted = w.c.watched_scripts();
    assert_eq!(wasted.len(), 1);
    assert_eq!(w.c.held_sales(), 0);
    owner_holds_all(&mut w);
    w.tick(SETTLE_SECS - 1);
    let done = w.c.settle(&mut w.m, w.now).unwrap();
    assert_eq!(done.released, 0);
    assert_eq!(w.c.watched_scripts(), wasted);
    w.tick(1);
    let done = w.c.settle(&mut w.m, w.now).unwrap();
    assert_eq!((done.count, done.released), (0, 1));
    assert!(w.c.watched_scripts().is_empty());
    // The release is stored and survives a restart.
    w.restart();
    assert!(w.c.watched_scripts().is_empty());
    // The index stays issued: the next sale gets another one.
    let next = w.open_five();
    assert_eq!(next.index, 1);
    assert_ne!(next.script, wasted[0]);
}

#[test]
fn cr_79_the_address_of_a_sale_the_counter_still_holds_is_kept_however_old() {
    let mut w = World::new();
    let live = w.open_five();
    w.tick(10 * SETTLE_SECS);
    assert_eq!(w.c.settle(&mut w.m, w.now).unwrap().released, 0);
    assert!(w.c.watched_scripts().contains(&live.script));
    // Ended and recent: kept as well.
    w.c.cancel(live.index, w.now, &mut w.m).unwrap();
    owner_holds_all(&mut w);
    assert_eq!(w.c.settle(&mut w.m, w.now).unwrap().released, 0);
    assert!(w.c.watched_scripts().contains(&live.script));
}

#[test]
fn cr_79_nothing_is_released_while_a_sale_is_unreadable() {
    let mut w = World::new();
    let o = w.open_five();
    w.c.cancel(o.index, w.now, &mut w.m).unwrap();
    w.m.slots.insert(Slot::Sale(9), vec![0, 0]);
    w.restart();
    // Take the readable sale's slot away: its address is now held by no sale
    // the counter has, but the unreadable one might be the holder.
    w.m.slots.remove(&Slot::Sale(o.index));
    w.restart();
    owner_holds_all(&mut w);
    w.tick(10 * SETTLE_SECS);
    let done = w.c.settle(&mut w.m, w.now).unwrap();
    assert_eq!((done.count, done.released), (0, 0));
    assert!(w.c.watched_scripts().contains(&o.script));
}

// The stored form of the batches keeps its version.

#[test]
fn cr_79_the_batches_keep_their_magic_and_version() {
    let w = World::new();
    assert_eq!(&w.c.batches().to_bytes()[..5], b"OBMC\x01");
}

// CR-40: a store that refuses.

#[test]
fn cr_77_a_store_that_refuses_the_slot_leaves_the_sale_whole_in_memory() {
    let mut w = World::new();
    let a = ended_sale(&mut w);
    let b = ended_sale(&mut w);
    owner_holds_all(&mut w);
    w.tick(SETTLE_SECS);
    let watched = w.c.watched_scripts();
    w.m.reset();
    w.m.fail_on = Some(1);
    let err = w.c.settle(&mut w.m, w.now).unwrap_err();
    assert!(matches!(err, StoreFault::Write("disk full")));
    for i in [a, b] {
        let sale = w.c.sale(i).unwrap();
        assert!(w.m.slots.contains_key(&Slot::Sale(i)));
        // Still watched, still known by its script.
        assert!(w.c.watched_scripts().contains(sale.script()));
    }
    assert_eq!(w.c.held_sales(), 2);
    assert_eq!(w.c.watched_scripts(), watched);
    // The same call, with the store back, settles both.
    w.m.reset();
    assert_eq!(settle(&mut w), 2);
}

#[test]
fn cr_77_a_refusal_in_the_middle_keeps_what_was_settled_before_it() {
    let mut w = World::new();
    let a = ended_sale(&mut w);
    let b = ended_sale(&mut w);
    owner_holds_all(&mut w);
    w.tick(SETTLE_SECS);
    w.m.reset();
    w.m.fail_on = Some(2);
    assert!(w.c.settle(&mut w.m, w.now).is_err());
    // Oldest first: the first is gone, the second stayed whole.
    assert!(w.c.sale(a).is_none());
    assert!(!w.m.slots.contains_key(&Slot::Sale(a)));
    assert!(w.c.sale(b).is_some());
    assert!(w.m.slots.contains_key(&Slot::Sale(b)));
    w.restart();
    assert!(w.c.sale(a).is_none() && w.c.sale(b).is_some());
}

#[test]
fn cr_79_a_refusal_of_the_batches_changes_nothing_and_the_next_call_finishes() {
    let mut w = World::new();
    let o = w.open_five();
    w.c.cancel(o.index, w.now, &mut w.m).unwrap();
    owner_holds_all(&mut w);
    w.tick(SETTLE_SECS);
    w.m.reset();
    w.m.fail_on = Some(2);
    assert!(w.c.settle(&mut w.m, w.now).is_err());
    // The sale is settled; its address is still on the list.
    assert!(w.c.sale(o.index).is_none());
    assert!(w.c.watched_scripts().contains(&o.script));
    w.m.reset();
    let done = w.c.settle(&mut w.m, w.now).unwrap();
    assert_eq!((done.count, done.released), (0, 1));
    assert!(!w.c.watched_scripts().contains(&o.script));
}

// The bound over a long run.

fn run_a_shop(days: u64, per_day: u64, owner_takes: bool) -> (World, usize, usize, usize) {
    let total = u32::try_from(days * per_day).unwrap();
    let mut w = World::with_batch(wide_batch(total));
    let (mut held, mut watched, mut indexed) = (0, 0, 0);
    for _ in 0..days {
        for _ in 0..per_day {
            let o = w.open_five();
            w.tick(EXPIRY_SECS + 1);
            assert_eq!(w.judge(o.index).state, crate::State::Expired);
        }
        w.tick(DAY - per_day * (EXPIRY_SECS + 1));
        if owner_takes {
            owner_holds_all(&mut w);
        }
        // The node calls it often; the bound holds however often.
        while w.c.settle(&mut w.m, w.now).unwrap().remaining {}
        held = held.max(w.c.held_sales());
        watched = watched.max(w.c.watched_scripts().len());
        indexed = indexed.max(w.c.indexed_sales());
    }
    (w, held, watched, indexed)
}

#[test]
fn cr_76_five_thousand_sales_over_simulated_days_keep_memory_bounded() {
    // 25 sales a day for 200 days.
    let (w, held, watched, indexed) = run_a_shop(200, 25, true);
    // A month of sales plus a day, and nothing older.
    let bound = 25 * 31 + 25;
    assert!(held <= bound, "sales held: {held}");
    assert!(watched <= bound, "scripts watched: {watched}");
    assert!(indexed <= bound, "records indexed: {indexed}");
    assert!(w.c.held_sales() <= bound);
    // The records are not bounded (CR-81 is not built): every one is kept.
    assert!(w.c.records_len() >= 5_000);
    // And the store holds only what the counter holds.
    let slots =
        w.m.slots
            .keys()
            .filter(|s| matches!(s, Slot::Sale(_)))
            .count();
    assert_eq!(slots, w.c.held_sales());
    // The oldest sale is gone and a restart gives the same counter.
    assert!(w.c.sale(0).is_none());
    let watched_now = w.c.watched_scripts();
    let held_now = w.c.held_sales();
    let restarted = w.m.reload();
    assert_eq!(restarted.held_sales(), held_now);
    assert_eq!(restarted.watched_scripts(), watched_now);
}

#[test]
fn cr_76_a_counter_whose_owner_never_takes_records_keeps_everything() {
    let (w, held, watched, _) = run_a_shop(40, 25, false);
    assert_eq!(held, 1_000);
    assert_eq!(w.c.held_sales(), 1_000);
    assert_eq!(watched, 1_000);
    assert_eq!(w.c.watched_scripts().len(), 1_000);
}

#[test]
fn cr_76_the_records_of_a_settled_sale_are_still_in_the_chain() {
    let mut w = World::new();
    let i = ended_sale(&mut w);
    owner_holds_all(&mut w);
    let before = w.c.records_len();
    w.tick(SETTLE_SECS);
    settle(&mut w);
    assert_eq!(w.c.records_len(), before);
    let kinds: Vec<_> =
        w.c.records_from(0, usize::MAX, &w.m)
            .unwrap()
            .iter()
            .map(|r| (r.kind, Event::decode(&r.body).unwrap().index()))
            .collect();
    assert_eq!(kinds, vec![(RecordKind::Sale, i), (RecordKind::Sale, i)]);
}
