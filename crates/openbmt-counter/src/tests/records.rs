//! What the records say: the rate source (CR-57, CR-58), the record of a new
//! quote (CR-59) and what the record `Final` names (CR-32).

use bitcoin::hashes::Hash;
use bitcoin::Transaction;

use super::kit::*;
use crate::money::{MAX_RATE_AGE_SECS, MAX_RATE_AHEAD_SECS};
use crate::{
    sats_for, Amount, Event, OpenRefused, RateSource, RequoteRefused, State, StoreFault,
    MAX_FINAL_PAYMENTS, MAX_SOURCE_LABEL_BYTES,
};

// CR-57: the source.

#[test]
fn cr_57_a_label_is_one_to_thirty_two_bytes_of_the_allowed_set() {
    for ok in ["regtest-fixed", "a", "x.y+z-9", &"a".repeat(32)] {
        assert_eq!(RateSource::new(ok, 5).unwrap().label(), ok);
    }
    assert_eq!(RateSource::new("a", 77).unwrap().fetched_at(), 77);
    assert_eq!(MAX_SOURCE_LABEL_BYTES, 32);
    for bad in ["", "A", "a b", "a_b", "a/b", "é", "a\n", &"a".repeat(33)] {
        let err = RateSource::new(bad, 5).unwrap_err();
        assert_eq!(err.rule(), "CR-57", "{bad:?}");
        assert!(err.to_string().starts_with("CR-57: "));
    }
}

#[test]
fn cr_57_the_source_of_each_quote_is_written_down() {
    let mut w = World::new();
    let facts = w.open_facts();
    let first = RateSource::new("first-one", facts.now - 20).unwrap();
    let o =
        w.c.open(five_dollars(), usd(), rate(), &first, facts, &mut w.m)
            .unwrap();
    w.tick(400);
    let second = RateSource::new("second.one", w.now - 5).unwrap();
    w.c.requote(
        o.index,
        crate::Rate::new(20_000_000).unwrap(),
        &second,
        w.now,
        &w.chain,
        &mut w.m,
    )
    .unwrap();
    let events = w.m.events();
    assert!(matches!(&events[0], Event::Opened { source, .. } if *source == first));
    assert!(matches!(&events[1], Event::Requoted { source, .. } if *source == second));
}

// CR-58: a stale rate.

#[test]
fn cr_58_a_rate_fetched_too_long_ago_or_in_the_future_is_refused_at_open() {
    for (fetched, ok) in [
        (T0, true),
        (T0 - MAX_RATE_AGE_SECS, true),
        (T0 - MAX_RATE_AGE_SECS - 1, false),
        (T0 + MAX_RATE_AHEAD_SECS, true),
        (T0 + MAX_RATE_AHEAD_SECS + 1, false),
        (0, false),
        (u64::MAX, false),
    ] {
        let mut w = World::new();
        let facts = w.open_facts();
        let source = RateSource::new("regtest-fixed", fetched).unwrap();
        let result =
            w.c.open(five_dollars(), usd(), rate(), &source, facts, &mut w.m);
        if ok {
            assert!(result.is_ok(), "fetched {fetched}");
        } else {
            let err = result.unwrap_err();
            assert_eq!(
                err,
                OpenRefused::StaleRate {
                    fetched_at: fetched,
                    now: T0
                }
            );
            assert_eq!(err.rule(), "CR-58");
            assert!(err.to_string().starts_with("CR-58: "));
            assert_eq!(w.m.writes, 0, "nothing is written for fetched {fetched}");
            assert!(w.m.events().is_empty());
        }
    }
}

#[test]
fn cr_58_a_rate_fetched_too_long_ago_or_in_the_future_is_refused_at_requote() {
    for (fetched_back, ok) in [
        (0i64, true),
        (MAX_RATE_AGE_SECS as i64, true),
        (MAX_RATE_AGE_SECS as i64 + 1, false),
        (-(MAX_RATE_AHEAD_SECS as i64), true),
        (-(MAX_RATE_AHEAD_SECS as i64) - 1, false),
    ] {
        let mut w = World::new();
        let o = w.open_five();
        w.tick(301);
        let source =
            RateSource::new("regtest-fixed", (w.now as i64 - fetched_back) as u64).unwrap();
        w.m.reset();
        let result = w.c.requote(
            o.index,
            crate::Rate::new(20_000_000).unwrap(),
            &source,
            w.now,
            &w.chain,
            &mut w.m,
        );
        if ok {
            assert!(result.is_ok(), "back {fetched_back}");
        } else {
            let err = result.unwrap_err();
            assert!(matches!(err, RequoteRefused::StaleRate { .. }));
            assert_eq!(err.rule(), "CR-58");
            assert!(err.to_string().starts_with("CR-58: "));
            assert_eq!(w.m.writes, 0, "nothing is written for {fetched_back}");
            assert_eq!(w.c.sale(o.index).unwrap().quotes().len(), 1);
        }
    }
}

#[test]
fn cr_58_the_existing_refusals_keep_their_place() {
    // A stale rate does not hide the refusals that come first.
    let mut w = World::new();
    let stale = RateSource::new("regtest-fixed", 0).unwrap();
    let facts = w.open_facts();
    let zero =
        w.c.open(Amount(0), usd(), rate(), &stale, facts, &mut w.m)
            .unwrap_err();
    assert_eq!(zero, OpenRefused::ZeroAmount);
    let o = w.open_five();
    w.c.cancel(o.index, w.now, &mut w.m).unwrap();
    let ended =
        w.c.requote(o.index, rate(), &stale, w.now, &w.chain, &mut w.m)
            .unwrap_err();
    assert_eq!(ended, RequoteRefused::SaleEnded);
}

// CR-59: the record of a new quote.

#[test]
fn cr_59_a_new_quote_is_written_and_can_be_replayed() {
    let mut w = World::new();
    let o = w.open_five();
    w.tick(301);
    let q = w.requote(o.index, 20_000_000).unwrap();
    let events = w.m.events();
    assert_eq!(events.len(), 2);
    let Event::Requoted {
        index,
        at,
        needs_sats,
        minor_per_btc,
        source,
    } = &events[1]
    else {
        panic!("not a requote: {:?}", events[1]);
    };
    assert_eq!((*index, *at), (o.index, w.now));
    assert_eq!(*minor_per_btc, 20_000_000);
    assert_eq!(*needs_sats, sats_for(five_dollars(), q.rate).unwrap());
    assert_eq!(*needs_sats, 2_500);
    assert_eq!(source.label(), "test-fixed");
    assert_eq!(events[1].kind(), openbmt_core::records::RecordKind::Sale);
    // The chain reads back and a counter started from it holds both quotes.
    let again = w.m.reload();
    assert_eq!(again.sale(o.index).unwrap().quotes().len(), 2);
    assert_eq!(again.records().len(), w.c.records().len());
}

#[test]
fn cr_59_a_stop_between_the_record_and_the_sale_leaves_a_record_for_a_quote_not_held() {
    let mut w = World::new();
    let o = w.open_five();
    w.tick(301);
    w.m.reset();
    w.m.fail_on = Some(2); // the record is the first write, the sale the second
    let err = w.requote(o.index, 20_000_000).unwrap_err();
    assert!(matches!(err, RequoteRefused::Store(StoreFault::Write(_))));
    assert_eq!(err.rule(), "CR-40");
    assert_eq!(
        w.m.log,
        vec!["record".to_string(), "FAILED Sale(0)".to_string()]
    );
    // The record is there, the sale has one quote, in memory and stored.
    assert!(matches!(w.m.events().last(), Some(Event::Requoted { .. })));
    assert_eq!(w.c.sale(o.index).unwrap().quotes().len(), 1);
    assert_eq!(w.m.reload().sale(o.index).unwrap().quotes().len(), 1);
    // Asking again takes the quote and writes a second record.
    w.m.reset();
    w.requote(o.index, 20_000_000).unwrap();
    assert_eq!(w.c.sale(o.index).unwrap().quotes().len(), 2);
    let requotes =
        w.m.events()
            .iter()
            .filter(|e| matches!(e, Event::Requoted { .. }))
            .count();
    assert_eq!(requotes, 2);
    // A record that cannot be written leaves nothing at all.
    let mut v = World::new();
    let o = v.open_five();
    v.tick(301);
    v.m.reset();
    v.m.fail_on = Some(1);
    assert!(v.requote(o.index, 20_000_000).is_err());
    assert_eq!(v.m.events().len(), 1);
    assert_eq!(v.c.sale(o.index).unwrap().quotes().len(), 1);
}

// CR-32: what Final names.

fn finals(w: &World) -> Vec<Event> {
    w.m.events()
        .into_iter()
        .filter(|e| matches!(e, Event::Final { .. }))
        .collect()
}

fn txids(txs: &[&Transaction]) -> Vec<[u8; 32]> {
    let mut ids: Vec<[u8; 32]> = txs
        .iter()
        .map(|t| t.compute_txid().to_byte_array())
        .collect();
    ids.sort_unstable();
    ids
}

/// The time, first-seen time, value, payments and `more` of a `Final`.
type Taken = (u64, u64, Amount, Vec<([u8; 32], u64)>, bool);

/// The one `Final` record of the sale's run, taken apart.
fn the_final(w: &World) -> Taken {
    let all = finals(w);
    assert_eq!(all.len(), 1, "one Final record");
    let Event::Final {
        at,
        received_sats,
        first_seen_at,
        valued,
        payments,
        more,
        ..
    } = all[0].clone()
    else {
        unreachable!()
    };
    assert_eq!(received_sats, payments.iter().map(|p| p.1).sum::<u64>());
    (at, first_seen_at, valued, payments, more)
}

#[test]
fn cr_32_final_names_the_one_payment_of_a_sale() {
    let mut w = World::new();
    let o = w.open_five();
    let t = w.pay(&o, 5_000);
    w.tick(10);
    w.see(&t, 3);
    w.tick(50);
    w.confirm(std::slice::from_ref(&t), 100);
    w.chain.tip = 105;
    assert_eq!(w.judge(o.index).state, State::Final);
    let (at, first_seen, valued, payments, more) = the_final(&w);
    assert_eq!(payments, vec![(t.compute_txid().to_byte_array(), 5_000)]);
    assert!(!more);
    assert_eq!(at, T0 + 60);
    assert_eq!(first_seen, T0 + 10);
    assert_eq!(valued, Amount(500));
}

#[test]
fn cr_32_final_names_both_payments_of_a_sale_paid_in_two_parts_in_order_of_id() {
    let mut w = World::new();
    let o = w.open_five();
    let a = w.pay(&o, 3_000);
    let b = w.pay(&o, 2_000);
    w.see(&a, 3);
    w.tick(7);
    w.see(&b, 3);
    w.confirm(&[a.clone(), b.clone()], 100);
    w.chain.tip = 105;
    assert_eq!(w.judge(o.index).state, State::Final);
    let (_, first_seen, valued, payments, more) = the_final(&w);
    let ids: Vec<[u8; 32]> = payments.iter().map(|p| p.0).collect();
    assert_eq!(ids, txids(&[&a, &b]));
    let by_id = |t: &Transaction| {
        payments
            .iter()
            .find(|p| p.0 == t.compute_txid().to_byte_array())
            .unwrap()
            .1
    };
    assert_eq!((by_id(&a), by_id(&b)), (3_000, 2_000));
    assert!(!more);
    assert_eq!(first_seen, T0);
    assert_eq!(valued, Amount(500));
}

#[test]
fn cr_32_final_names_the_payment_that_took_the_place_of_a_replaced_one() {
    let mut w = World::new();
    let o = w.open_five();
    let old = w.pay(&o, 5_000);
    w.see(&old, 3);
    w.tick(30);
    let new = bump(&old, 5_100);
    w.see(&new, 3);
    w.tick(5);
    w.confirm(std::slice::from_ref(&new), 100);
    w.chain.tip = 105;
    assert_eq!(w.judge(o.index).state, State::Final);
    let (_, first_seen, _, payments, more) = the_final(&w);
    assert_eq!(payments, vec![(new.compute_txid().to_byte_array(), 5_100)]);
    assert!(!more);
    // The time starts again from the replacement (WR-42).
    assert_eq!(first_seen, T0 + 30);
}

#[test]
fn cr_32_final_names_every_transaction_up_to_the_watchers_limit() {
    let mut w = World::new();
    let o = w.open(20_000); // $200: 200,000 sats
    let txs: Vec<Transaction> = (0..20).map(|_| w.pay(&o, 10_000)).collect();
    for t in &txs {
        w.see(t, 3);
    }
    w.confirm(&txs, 100);
    w.chain.tip = 105;
    assert_eq!(w.judge(o.index).state, State::Final);
    let (_, _, _, payments, more) = the_final(&w);
    assert_eq!(payments.len(), 20);
    assert!(!more);
    assert!(payments.windows(2).all(|p| p[0].0 < p[1].0));
}

#[test]
fn cr_32_a_sale_with_more_standing_transactions_than_the_record_holds_is_still_final() {
    // The watcher keeps at most 20 transactions, so the counter cannot be
    // brought here; the choice is made by `final_payments`, which gets
    // 33 standing transactions.
    use crate::events::final_payments;
    let standing: Vec<([u8; 32], u64)> = (1..=33u8).map(|i| ([i; 32], u64::from(i) * 10)).collect();
    let (kept, more) = final_payments(&standing);
    assert!(more);
    assert_eq!(kept.len(), MAX_FINAL_PAYMENTS);
    // The smallest amount is the one left out.
    assert!(kept.iter().all(|(t, _)| *t != [1; 32]));
    let record = Event::Final {
        index: 0,
        at: 1,
        received_sats: standing.iter().map(|p| p.1).sum(),
        depth: 6,
        first_seen_at: 1,
        valued: Amount(1),
        payments: kept,
        more,
    };
    assert_eq!(Event::decode(&record.encode()).unwrap(), record);
}

#[test]
fn cr_32_a_payment_seen_only_in_a_block_has_the_time_the_block_was_learned() {
    let mut w = World::new();
    let o = w.open_five();
    let t = w.pay(&o, 5_000);
    w.tick(30);
    w.confirm(std::slice::from_ref(&t), 100);
    w.tick(20);
    w.chain.tip = 105;
    assert_eq!(w.judge(o.index).state, State::Final);
    let (at, first_seen, _, payments, _) = the_final(&w);
    assert_eq!(at, T0 + 50);
    assert_eq!(first_seen, T0 + 30);
    assert_eq!(payments.len(), 1);
}

#[test]
fn cr_32_the_value_is_worked_out_by_the_quotes_the_satoshis_were_valued_at() {
    let mut w = World::new();
    let o = w.open(1_000); // $10: 10,000 sats
    let part = w.pay(&o, 4_000);
    w.see(&part, 1);
    w.judge(o.index);
    // The price of bitcoin doubles and the rest is priced again.
    w.tick(301);
    w.requote(o.index, 20_000_000).unwrap();
    let rest = w.pay(&o, 3_000);
    w.see(&rest, 1);
    w.judge(o.index);
    w.confirm(&[part, rest], 100);
    w.chain.tip = 105;
    assert_eq!(w.judge(o.index).state, State::Final);
    let (_, _, valued, payments, _) = the_final(&w);
    // 4,000 sats at the first rate are 400 cents; 3,000 at the second 600.
    assert_eq!(valued, Amount(1_000));
    assert_eq!(payments.iter().map(|p| p.1).sum::<u64>(), 7_000);
}

#[test]
fn cr_32_the_record_final_is_written_once_and_a_late_payment_does_not_change_it() {
    let mut w = World::new();
    let o = w.open_five();
    let t = w.pay(&o, 5_000);
    w.confirm(std::slice::from_ref(&t), 95);
    assert_eq!(w.judge(o.index).state, State::Final);
    let late = w.pay(&o, 700);
    w.see(&late, 3);
    w.judge(o.index);
    assert_eq!(finals(&w).len(), 1);
    let (_, _, _, payments, _) = the_final(&w);
    assert_eq!(payments, vec![(t.compute_txid().to_byte_array(), 5_000)]);
}
