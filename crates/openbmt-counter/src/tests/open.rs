//! Section 1: opening a sale, quotes, expiry.

use openbmt_watch::Problem;

use super::kit::*;
use crate::{
    Amount, CancelRefused, End, OpenFacts, OpenRefused, Quote, RequoteRefused, Slot, State,
    EXPIRY_SECS, MAX_ADDRESSES_PER_HOUR, MAX_QUOTES, QUOTE_SECS,
};

fn try_open(w: &mut World) -> Result<crate::Opened, OpenRefused<&'static str>> {
    let facts = w.open_facts();
    w.c.open(
        five_dollars(),
        usd(),
        rate(),
        &fresh(facts.now),
        facts,
        &mut w.m,
    )
}

// CR-01.

#[test]
fn cr_01_every_unhealthy_check_refuses_opening_and_names_the_problem() {
    for problem in [
        Problem::TooFewPeerGroups,
        Problem::TooFewFilterPeers,
        Problem::TipIsOld,
        Problem::HeadersBehind,
        Problem::ClockMismatch,
    ] {
        let mut w = World::new();
        w.health = unhealthy(problem);
        let err = try_open(&mut w).unwrap_err();
        assert_eq!(err, OpenRefused::Unhealthy(Some(problem)));
        assert_eq!(err.rule(), "CR-01");
        let text = err.to_string();
        assert!(text.starts_with("CR-01: "), "{text}");
        assert!(text.contains("another payment type"), "{text}");
        assert_eq!(w.m.writes, 0, "a refusal writes nothing");
        assert!(w.c.sale(0).is_none());
    }
}

#[test]
fn cr_01_a_clock_mismatch_refuses_opening_because_the_lock_and_expiry_cannot_be_trusted() {
    let mut w = World::new();
    w.health = unhealthy(Problem::ClockMismatch);
    let err = try_open(&mut w).unwrap_err();
    assert_eq!(err, OpenRefused::Unhealthy(Some(Problem::ClockMismatch)));
    assert!(err.to_string().contains("clock"), "{err}");
}

#[test]
fn cr_01_an_unhealthy_answer_without_a_named_problem_still_refuses() {
    let mut w = World::new();
    w.health.healthy = false;
    assert_eq!(try_open(&mut w).unwrap_err(), OpenRefused::Unhealthy(None));
}

// CR-02.

#[test]
fn cr_02_a_sale_gets_the_next_unused_address_and_only_that() {
    let mut w = World::new();
    let first = w.open_five();
    let second = w.open_five();
    assert_eq!((first.index, second.index), (0, 1));
    assert_eq!(first.script, p2wsh(1));
    assert_eq!(second.script, p2wsh(2));
    assert_ne!(first.script, second.script);
    assert_eq!(w.c.sale(0).unwrap().script(), &first.script);
    assert_eq!(w.c.sale(1).unwrap().script(), &second.script);
}

#[test]
fn cr_02_an_address_is_never_used_for_two_sales_even_after_a_restart() {
    let mut w = World::new();
    let first = w.open_five();
    w.restart();
    let second = w.open_five();
    assert_ne!(first.index, second.index);
    assert_ne!(first.script, second.script);
}

#[test]
fn cr_02_an_empty_batch_refuses_the_sale() {
    let mut w = World::with_addresses(1);
    w.open_five();
    let writes = w.m.writes;
    let err = try_open(&mut w).unwrap_err();
    assert_eq!(err, OpenRefused::NoAddress);
    assert_eq!(err.rule(), "CR-02");
    assert!(err.to_string().contains("do not offer Bitcoin"));
    assert_eq!(w.m.writes, writes, "a refusal writes nothing");
}

#[test]
fn cr_02_a_counter_with_no_batch_at_all_offers_no_address() {
    let mut c = crate::Counter::new(id(3), pairing(), settings());
    let mut m = Mem::default();
    let facts = OpenFacts {
        health: healthy(),
        rescan_pending: false,
        now: T0,
    };
    let err = c
        .open(
            five_dollars(),
            usd(),
            rate(),
            &fresh(facts.now),
            facts,
            &mut m,
        )
        .unwrap_err();
    assert_eq!(err, OpenRefused::NoAddress);
}

// CR-03.

#[test]
fn cr_03_the_index_is_stored_before_the_address_is_handed_out() {
    let mut w = World::new();
    w.open_five();
    assert_eq!(w.m.log, ["Desk", "Batches", "Sale(0)", "record"]);
}

#[test]
fn cr_03_if_the_store_fails_no_address_is_handed_out() {
    let mut w = World::new();
    w.m.fail_on = Some(2); // the batches
    let err = try_open(&mut w).unwrap_err();
    assert!(matches!(err, OpenRefused::AddressNotStored(_)));
    assert_eq!(err.rule(), "CR-03");
    assert!(w.c.sale(0).is_none());
    // Nothing was used: the same address is next, here and after a restart.
    w.m.reset();
    assert_eq!(w.open_five().index, 0);
    let mut w2 = World::new();
    w2.m.fail_on = Some(2);
    try_open(&mut w2).unwrap_err();
    w2.m.reset();
    w2.restart();
    assert_eq!(w2.open_five().script, p2wsh(1));
}

#[test]
fn cr_03_a_crash_after_the_index_wastes_it_and_never_reuses_it() {
    let mut w = World::new();
    w.m.fail_on = Some(3); // the sale, after the index was stored
    let err = try_open(&mut w).unwrap_err();
    assert!(matches!(err, OpenRefused::Store(_)));
    w.m.reset();
    w.restart();
    let next = w.open_five();
    assert_eq!(next.index, 1, "index 0 was wasted, not reused");
    assert_eq!(next.script, p2wsh(2));
}

// CR-04.

#[test]
fn cr_04_at_most_120_addresses_are_handed_out_an_hour() {
    let mut w = World::with_addresses(200);
    for _ in 0..MAX_ADDRESSES_PER_HOUR {
        w.open_five();
    }
    let err = try_open(&mut w).unwrap_err();
    assert_eq!(err, OpenRefused::RateLimited);
    assert_eq!(err.rule(), "CR-04");
    w.tick(3_599);
    assert_eq!(try_open(&mut w).unwrap_err(), OpenRefused::RateLimited);
    w.tick(1);
    assert!(try_open(&mut w).is_ok(), "the hour has moved on");
}

#[test]
fn cr_04_the_count_survives_a_restart() {
    let mut w = World::with_addresses(200);
    for _ in 0..MAX_ADDRESSES_PER_HOUR {
        w.open_five();
    }
    w.restart();
    assert_eq!(try_open(&mut w).unwrap_err(), OpenRefused::RateLimited);
}

#[test]
fn cr_04_old_address_times_are_pruned_when_the_desk_is_used() {
    let mut w = World::new();
    w.open_five();
    w.open_five();
    assert_eq!(w.c.desk().addresses_in_hour(w.now), 2);
    w.tick(4_000);
    w.open_five();
    let stored = crate::Desk::from_bytes(&w.m.slots[&Slot::Desk]).unwrap();
    assert_eq!(stored.addresses_in_hour(w.now), 1);
    assert_eq!(stored, *w.c.desk());
}

// CR-05.

#[test]
fn cr_05_a_quote_is_good_for_five_minutes() {
    assert_eq!(QUOTE_SECS, 300);
    let q = Quote::new(rate(), 1_000);
    assert_eq!((q.from, q.until), (1_000, 1_300));
    assert!(!q.holds(999));
    assert!(q.holds(1_000));
    assert!(q.holds(1_299));
    assert!(!q.holds(1_300));
}

#[test]
fn cr_05_a_new_quote_keeps_the_address_and_all_quotes() {
    let mut w = World::new();
    let o = w.open_five();
    w.tick(301);
    let q = w.requote(o.index, 20_000_000).unwrap();
    assert_eq!(q.rate.minor_per_btc(), 20_000_000);
    let sale = w.c.sale(o.index).unwrap();
    assert_eq!(sale.script(), &o.script, "the address does not change");
    assert_eq!(sale.quotes().len(), 2);
    assert_eq!(sale.quotes()[0], o.quote);
    assert_eq!(sale.quotes()[1], q);
    // The new quote prices the whole amount at the new rate.
    assert_eq!(sale.needs_sats(), 2_500);
}

#[test]
fn cr_05_a_requote_after_the_sale_ended_is_refused() {
    let mut w = World::new();
    let o = w.open_five();
    w.c.cancel(o.index, w.now, &mut w.m).unwrap();
    let err = w.requote(o.index, 20_000_000).unwrap_err();
    assert_eq!(err, RequoteRefused::SaleEnded);
    assert_eq!(err.rule(), "CR-05");
}

#[test]
fn cr_05_a_sale_keeps_at_most_64_quotes_and_the_last_stays_in_force() {
    let mut w = World::new();
    let o = w.open_five();
    for i in 1..MAX_QUOTES {
        w.tick(1);
        w.requote(o.index, 10_000_000 + i as u64).unwrap();
    }
    assert_eq!(w.c.sale(o.index).unwrap().quotes().len(), MAX_QUOTES);
    let err = w.requote(o.index, 30_000_000).unwrap_err();
    assert_eq!(err, RequoteRefused::TooManyQuotes);
    assert_eq!(err.rule(), "CR-05");
    let last = *w.c.sale(o.index).unwrap().quotes().last().unwrap();
    assert_eq!(last.rate.minor_per_btc(), 10_000_000 + 63);
}

#[test]
fn cr_05_a_sale_needs_an_amount_that_bitcoin_can_cover() {
    let mut w = World::new();
    let facts = w.open_facts();
    let zero =
        w.c.open(Amount(0), usd(), rate(), &fresh(facts.now), facts, &mut w.m);
    assert_eq!(zero.unwrap_err(), OpenRefused::ZeroAmount);
    let huge = w.c.open(
        Amount(u64::MAX),
        usd(),
        rate(),
        &fresh(facts.now),
        facts,
        &mut w.m,
    );
    let err = huge.unwrap_err();
    assert_eq!(err, OpenRefused::AmountTooLarge);
    assert_eq!(err.rule(), "CR-05");
    assert_eq!(w.m.writes, 0);
}

// CR-07.

#[test]
fn cr_07_a_sale_with_nothing_received_expires_after_15_minutes() {
    assert_eq!(EXPIRY_SECS, 900);
    let mut w = World::new();
    let o = w.open_five();
    w.tick(EXPIRY_SECS - 1);
    assert_eq!(w.judge(o.index).state, State::NotPaidYet);
    w.tick(1);
    let screen = w.judge(o.index);
    assert_eq!(screen.state, State::Expired);
    assert_eq!(
        w.c.sale(o.index).unwrap().ended(),
        Some((End::Expired, w.now))
    );
    assert!(w
        .m
        .events()
        .iter()
        .any(|e| matches!(e, crate::Event::Expired { index: 0, .. })));
}

#[test]
fn cr_07_an_expired_sales_address_stays_watched_for_good() {
    let mut w = World::new();
    let o = w.open_five();
    w.tick(EXPIRY_SECS);
    w.judge(o.index);
    assert!(w.c.watched_scripts().contains(&o.script));
    w.restart();
    assert!(w.c.watched_scripts().contains(&o.script));
    assert_eq!(w.judge(o.index).state, State::Expired);
}

#[test]
fn cr_07_the_cashier_can_cancel_a_sale_with_nothing_received() {
    let mut w = World::new();
    let o = w.open_five();
    w.c.cancel(o.index, w.now, &mut w.m).unwrap();
    assert_eq!(w.judge(o.index).state, State::Cancelled);
    assert!(w.c.watched_scripts().contains(&o.script));
    let err = w.c.cancel(o.index, w.now, &mut w.m).unwrap_err();
    assert_eq!(err, CancelRefused::AlreadyEnded);
    assert_eq!(err.rule(), "CR-07");
}

#[test]
fn cr_07_a_sale_with_money_on_it_cannot_be_cancelled() {
    let mut w = World::new();
    let o = w.open_five();
    let tx = w.pay(&o, 5_000);
    w.see(&tx, 1);
    let err = w.c.cancel(o.index, w.now, &mut w.m).unwrap_err();
    assert_eq!(err, CancelRefused::MoneyArrived);
    assert!(err.to_string().contains("refund"), "{err}");
    assert_eq!(w.c.sale(o.index).unwrap().ended(), None);
}

#[test]
fn cr_07_a_sale_with_money_on_it_does_not_expire() {
    let mut w = World::new();
    let o = w.open_five();
    let tx = w.pay(&o, 5_000);
    w.see(&tx, 1);
    w.tick(EXPIRY_SECS * 4);
    assert_ne!(w.judge(o.index).state, State::Expired);
}

// CR-09.

#[test]
fn cr_09_no_sale_opens_while_a_rescan_is_pending() {
    let mut w = World::new();
    let facts = OpenFacts {
        rescan_pending: true,
        ..w.open_facts()
    };
    let err =
        w.c.open(
            five_dollars(),
            usd(),
            rate(),
            &fresh(facts.now),
            facts,
            &mut w.m,
        )
        .unwrap_err();
    assert_eq!(err, OpenRefused::RescanPending);
    assert_eq!(err.rule(), "CR-09");
    assert!(err.to_string().starts_with("CR-09: "));
    assert_eq!(w.m.writes, 0);
}

// CR-75.

/// A world whose smallest bitcoin sale is `min_sats`.
fn with_floor(min_sats: u64) -> World {
    let mut w = World::new();
    let mut s = settings();
    s.min_sale_sats = min_sats;
    w.c.set_settings(s);
    w
}

fn open_cents(w: &mut World, cents: u64) -> Result<crate::Opened, OpenRefused<&'static str>> {
    let facts = w.open_facts();
    w.c.open(
        Amount(cents),
        usd(),
        rate(),
        &fresh(facts.now),
        facts,
        &mut w.m,
    )
}

#[test]
fn cr_75_a_sale_just_below_the_floor_is_refused_and_one_at_it_opens() {
    // 1,000 sats a dollar: a cent is 10 sats, so the closest amounts are
    // 10 sats apart.
    let mut w = with_floor(26_000);
    let err = open_cents(&mut w, 2_599).unwrap_err();
    assert_eq!(
        err,
        OpenRefused::BelowMinimum {
            sats: 25_990,
            min_sats: 26_000
        }
    );
    assert_eq!(err.rule(), "CR-75");
    let text = err.to_string();
    assert!(text.starts_with("CR-75: "), "{text}");
    assert!(text.contains("another payment type"), "{text}");
    assert!(open_cents(&mut w, 2_600).is_ok());
    // A floor that is not a multiple of 10: one sat below it is refused.
    let mut w = with_floor(26_001);
    assert!(open_cents(&mut w, 2_600).is_err());
    assert!(open_cents(&mut w, 2_601).is_ok());
}

#[test]
fn cr_75_a_floor_of_zero_takes_every_sale_above_dust() {
    let mut w = with_floor(0);
    assert!(open_cents(&mut w, 100).is_ok());
    assert_eq!(w.c.refused_below_minimum(), 0);
}

#[test]
fn cr_75_the_first_settings_hold_a_floor_of_26000() {
    assert_eq!(
        openbmt_core::settings::Settings::defaults(1_000).min_sale_sats,
        26_000
    );
}

#[test]
fn cr_75_a_refused_sale_writes_nothing_and_hands_out_no_address() {
    let mut w = with_floor(26_000);
    w.m.reset();
    let before_desk = w.m.slots.get(&Slot::Desk).cloned();
    let err = open_cents(&mut w, 500).unwrap_err();
    assert!(matches!(err, OpenRefused::BelowMinimum { .. }));
    assert_eq!(w.m.writes, 0, "{:?}", w.m.log);
    assert!(w.m.records.is_empty());
    assert_eq!(w.m.slots.get(&Slot::Desk).cloned(), before_desk);
    assert!(w.c.sale(0).is_none());
    // The address that would have gone to the refused sale goes to the next.
    let o = open_cents(&mut w, 2_600).unwrap();
    assert_eq!(o.index, 0);
}

#[test]
fn cr_75_a_sale_below_the_dust_line_is_refused_as_dust_not_as_below_the_floor() {
    let mut w = with_floor(26_000);
    let err = open_cents(&mut w, 40).unwrap_err();
    assert_eq!(err, OpenRefused::BelowDust { needs_sats: 400 });
    assert_eq!(w.c.refused_below_minimum(), 0);
}

#[test]
fn cr_75_the_counter_counts_the_sales_it_refused_for_the_floor() {
    let mut w = with_floor(26_000);
    assert_eq!(w.c.refused_below_minimum(), 0);
    open_cents(&mut w, 500).unwrap_err();
    open_cents(&mut w, 1_000).unwrap_err();
    open_cents(&mut w, 2_600).unwrap();
    assert_eq!(w.c.refused_below_minimum(), 2);
    // The count is in memory only: a restart begins again at 0.
    w.restart();
    assert_eq!(w.c.refused_below_minimum(), 0);
}

#[test]
fn cr_75_the_floor_is_read_at_the_quotes_rate() {
    let mut w = with_floor(26_000);
    // $26 at $100,000 a bitcoin is 26,000 sats; at $200,000 it is 13,000.
    let facts = w.open_facts();
    let err =
        w.c.open(
            Amount(2_600),
            usd(),
            crate::Rate::new(20_000_000).unwrap(),
            &fresh(facts.now),
            facts,
            &mut w.m,
        )
        .unwrap_err();
    assert_eq!(
        err,
        OpenRefused::BelowMinimum {
            sats: 13_000,
            min_sats: 26_000
        }
    );
}

#[test]
fn cr_75_a_requote_that_would_go_below_the_floor_is_refused_and_changes_nothing() {
    let mut w = with_floor(26_000);
    let o = open_cents(&mut w, 3_000).unwrap();
    w.m.reset();
    let err = w.requote(o.index, 20_000_000).unwrap_err();
    assert_eq!(
        err,
        RequoteRefused::BelowMinimum {
            sats: 15_000,
            min_sats: 26_000
        }
    );
    assert_eq!(err.rule(), "CR-75");
    assert_eq!(w.m.writes, 0, "{:?}", w.m.log);
    assert_eq!(w.c.sale(o.index).unwrap().quotes().len(), 1);
    // A requote that stays at or above the floor is taken.
    assert!(w.requote(o.index, 10_000_000).is_ok());
}

#[test]
fn cr_75_a_sale_paid_in_part_is_not_held_to_the_floor_again() {
    let mut w = with_floor(26_000);
    let o = open_cents(&mut w, 3_000).unwrap();
    let tx = w.pay(&o, 1_000);
    w.see(&tx, 1);
    assert!(w.requote(o.index, 20_000_000).is_ok());
}
