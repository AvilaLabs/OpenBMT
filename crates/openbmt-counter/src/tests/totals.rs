//! Section 14: the day's totals (CR-152, CR-153).

use super::kit::*;
use crate::totals::PersonLine;
use crate::{
    Actor, Amount, CashRefund, Currency, DayTotals, LightningPaid, RefundAsk, RefundRequest, Role,
    StaffChange, StaffEvent, State, TotalsRefused, MAX_PEOPLE,
};

const DAY: u64 = 86_400;
/// The start of the day the script happens in: the phone's midnight.
const MIDNIGHT: u64 = T0;

fn who(role: Role, name: &str) -> Actor {
    Actor {
        role,
        name: name.to_string(),
    }
}

fn ann() -> Actor {
    who(Role::Cashier, "ann")
}
fn bob() -> Actor {
    who(Role::Cashier, "bob")
}
fn max() -> Actor {
    who(Role::Manager, "max")
}

fn open_by(w: &mut World, by: &Actor) -> u32 {
    let facts = w.open_facts();
    w.c.open_by(
        Amount(500),
        usd(),
        rate(),
        &fresh(facts.now),
        facts,
        Some(by),
        &mut w.m,
    )
    .unwrap()
    .index
}

/// Pay a sale in full, confirm it at the refund depth and judge it.
fn pay_final(w: &mut World, index: u32, sats: u64) {
    let script = w.c.sale(index).unwrap().script().clone();
    let t = w.pay_to(&script, sats);
    w.confirm(&[t], 95);
    assert_eq!(w.judge(index).state, State::Final);
}

fn totals(w: &World, from: u64, to: u64, people: bool) -> DayTotals {
    w.c.day_totals(from, to, people, &w.m).unwrap()
}

/// A day with a requote, a Lightning sale, an override, a refund asked and
/// withdrawn, a cash refund, and a sale opened before midnight and paid
/// after it.
fn scripted_day() -> World {
    let mut w = World::new();
    // A: opened 100 seconds before midnight, paid ten seconds after it.
    w.now = MIDNIGHT - 100;
    let a = open_by(&mut w, &ann());
    w.now = MIDNIGHT + 10;
    pay_final(&mut w, a, 5_000);
    // B: requoted to a rate that needs twice the satoshis, then paid.
    w.now = MIDNIGHT + 60;
    let b = open_by(&mut w, &ann());
    w.now = MIDNIGHT + 120;
    w.requote(b, 5_000_000).unwrap();
    pay_final(&mut w, b, 10_000);
    // C: paid over Lightning, opened by the manager.
    w.now = MIDNIGHT + 200;
    let c = open_by(&mut w, &max());
    let facts = crate::Facts {
        chain: &w.chain,
        health: w.health,
        heartbeat: heartbeat(),
        remote_order: false,
        now: w.now,
    };
    let paid = LightningPaid {
        payment_hash: [7; 32],
        amount_msat: 5_000_000,
        settled_at: None,
    };
    w.c.lightning_paid(c, paid, &facts, &mut w.m).unwrap();
    // D: part paid, then accepted by the manager.
    w.now = MIDNIGHT + 300;
    let d = open_by(&mut w, &bob());
    let script = w.c.sale(d).unwrap().script().clone();
    let t = w.pay_to(&script, 2_000);
    w.see(&t, 1);
    assert_eq!(w.judge(d).state, State::PartPaid);
    w.override_sale(d, Role::Manager, "max").unwrap();
    // E: still open.
    w.now = MIDNIGHT + 400;
    open_by(&mut w, &ann());
    // F: cancelled by bob.
    w.now = MIDNIGHT + 500;
    let f = open_by(&mut w, &bob());
    w.c.cancel_by(f, w.now, Some(&bob()), &mut w.m).unwrap();
    // G: paid, a refund asked for twice (the second withdrawn), a cash refund.
    w.now = MIDNIGHT + 600;
    let g = open_by(&mut w, &ann());
    pay_final(&mut w, g, 5_000);
    let ask = |sats: u64| RefundRequest {
        ask: RefundAsk::Sats(sats),
        destination: p2wsh(0xa1),
        actor: ann(),
        reason: "wrong size".to_string(),
    };
    w.c.request_refund(g, ask(2_000), w.now, &w.chain, &mut w.m)
        .unwrap();
    let second =
        w.c.request_refund(g, ask(1_000), w.now, &w.chain, &mut w.m)
            .unwrap();
    w.c.withdraw_refund(g, second, ann(), w.now, &mut w.m)
        .unwrap();
    let cash = CashRefund {
        actor: max(),
        minor: Amount(100),
        reason: "till refund".to_string(),
    };
    w.c.refund_in_cash(g, cash, w.now, &w.chain, &mut w.m)
        .unwrap();
    w.now = MIDNIGHT + 900;
    w
}

fn line(t: &DayTotals, name: &str) -> PersonLine {
    t.people.iter().find(|p| p.name == name).unwrap().clone()
}

// CR-153.

#[test]
fn cr_153_a_scripted_day_is_counted() {
    let w = scripted_day();
    let t = totals(&w, MIDNIGHT, MIDNIGHT + DAY, false);
    assert_eq!((t.from, t.to), (MIDNIGHT, MIDNIGHT + DAY));
    assert_eq!(t.currency, Some(usd()));
    // Paid: B (requoted, so 10,000 sats), C (Lightning), D (accepted by a
    // person, 5,000 sats at risk) and G. A was opened the day before.
    assert_eq!(
        (t.paid.count, t.paid.minor, t.paid.sats),
        (4, 2_000, 25_000)
    );
    assert_eq!(
        (t.on_chain.count, t.on_chain.minor, t.on_chain.sats),
        (3, 1_500, 20_000)
    );
    assert_eq!(
        (t.lightning.count, t.lightning.minor, t.lightning.sats),
        (1, 500, 5_000)
    );
    // Waiting: E, with what it asks for.
    assert_eq!((t.waiting.count, t.waiting.sats), (1, 5_000));
    assert_eq!((t.stopped, t.ended), (0, 1));
    assert_eq!((t.overrides.count, t.overrides.sats), (1, 5_000));
    assert_eq!((t.refunds.count, t.refunds.sats), (1, 2_000));
    assert_eq!(t.refunds_withdrawn, 1);
    assert_eq!((t.cash_refunds.count, t.cash_refunds.minor), (1, 100));
    assert_eq!(t.cash_refunds.sats, 1_000);
    assert_eq!(t.unnamed_sales, 0);
    // The lines per person are not there unless asked for.
    assert!(t.people.is_empty());
}

#[test]
fn cr_153_a_sale_opened_before_midnight_counts_on_the_day_it_opened() {
    let w = scripted_day();
    let before = totals(&w, MIDNIGHT - DAY, MIDNIGHT, true);
    // A was paid after midnight; its state as of now is paid, on its own day.
    assert_eq!(
        (before.paid.count, before.paid.minor, before.paid.sats),
        (1, 500, 5_000)
    );
    assert_eq!(before.on_chain.count, 1);
    assert_eq!((before.waiting.count, before.ended), (0, 0));
    assert_eq!(before.overrides.count + before.refunds.count, 0);
    assert_eq!(line(&before, "ann").sales, 1);
    // The next day has nothing.
    let after = totals(&w, MIDNIGHT + DAY, MIDNIGHT + 2 * DAY, true);
    assert_eq!(after.paid.count + after.waiting.count + after.ended, 0);
    assert!(after.people.is_empty());
}

#[test]
fn cr_153_the_lines_per_person() {
    let w = scripted_day();
    let t = totals(&w, MIDNIGHT, MIDNIGHT + DAY, true);
    let names: Vec<(&Role, &str)> = t
        .people
        .iter()
        .map(|p| (&p.role, p.name.as_str()))
        .collect();
    assert_eq!(
        names,
        vec![
            (&Role::Cashier, "ann"),
            (&Role::Cashier, "bob"),
            (&Role::Manager, "max")
        ]
    );
    let ann = line(&t, "ann");
    assert_eq!(
        (ann.sales, ann.overrides, ann.refunds, ann.cash_refunds),
        (3, 0, 1, 0)
    );
    let bob = line(&t, "bob");
    assert_eq!(
        (bob.sales, bob.overrides, bob.refunds, bob.cash_refunds),
        (2, 0, 0, 0)
    );
    let max = line(&t, "max");
    assert_eq!(
        (max.sales, max.overrides, max.refunds, max.cash_refunds),
        (1, 1, 0, 1)
    );
    // The totals are the same with or without the lines.
    let plain = totals(&w, MIDNIGHT, MIDNIGHT + DAY, false);
    assert_eq!(
        DayTotals {
            people: Vec::new(),
            ..t
        },
        plain
    );
}

#[test]
fn cr_153_a_sale_that_names_no_one_is_counted_apart() {
    let mut w = World::new();
    w.now = MIDNIGHT + 5;
    w.open_five();
    let t = totals(&w, MIDNIGHT, MIDNIGHT + DAY, true);
    assert_eq!((t.unnamed_sales, t.waiting.count), (1, 1));
    assert!(t.people.is_empty());
}

#[test]
fn cr_153_a_payment_that_was_replaced_is_stopped_and_not_paid() {
    let mut w = World::new();
    w.now = MIDNIGHT + 5;
    let o = w.open_five();
    let t = w.pay(&o, 5_000);
    let block = w.confirm(&[t], 100);
    assert_eq!(w.judge(o.index).state, State::Confirmed);
    let paid = totals(&w, MIDNIGHT, MIDNIGHT + DAY, false);
    assert_eq!((paid.paid.count, paid.stopped), (1, 0));
    // The block leaves the chain: the sale is reopened and is not paid.
    w.orphan(&block);
    w.tick(10);
    assert_eq!(w.judge(o.index).state, State::DoNotHandOver);
    let stopped = totals(&w, MIDNIGHT, MIDNIGHT + DAY, false);
    assert_eq!(
        (stopped.paid.count, stopped.stopped, stopped.waiting.count),
        (0, 1, 0)
    );
}

#[test]
fn cr_153_a_sale_that_expired_is_ended() {
    let mut w = World::new();
    w.now = MIDNIGHT + 5;
    let o = w.open_five();
    w.tick(1_000);
    assert_eq!(w.judge(o.index).state, State::Expired);
    let t = totals(&w, MIDNIGHT, MIDNIGHT + DAY, false);
    assert_eq!((t.ended, t.waiting.count), (1, 0));
}

#[test]
fn cr_153_sales_in_two_currencies_are_refused() {
    let mut w = World::new();
    w.now = MIDNIGHT + 5;
    w.open_five();
    let facts = w.open_facts();
    let eur = Currency::new(*b"EUR").unwrap();
    w.c.open(Amount(500), eur, rate(), &fresh(facts.now), facts, &mut w.m)
        .unwrap();
    let err =
        w.c.day_totals(MIDNIGHT, MIDNIGHT + DAY, false, &w.m)
            .unwrap_err();
    assert_eq!(err, TotalsRefused::MixedCurrencies);
    assert_eq!(err.rule(), "CR-153");
    assert!(err.to_string().starts_with("CR-153"));
}

#[test]
fn cr_153_at_most_sixty_four_persons_are_named() {
    let mut w = World::with_addresses(100);
    w.now = MIDNIGHT + 5;
    for n in 0..=MAX_PEOPLE {
        open_by(&mut w, &who(Role::Cashier, &format!("c{n:02}")));
    }
    let t = totals(&w, MIDNIGHT, MIDNIGHT + DAY, true);
    assert_eq!(t.people.len(), MAX_PEOPLE);
    assert_eq!(t.people_left_out, 1);
    assert_eq!(t.waiting.count, u32::try_from(MAX_PEOPLE).unwrap() + 1);
}

// CR-152.

#[test]
fn cr_152_a_day_that_ends_before_it_starts_is_refused() {
    let w = World::new();
    for (from, to) in [(MIDNIGHT, MIDNIGHT), (MIDNIGHT + 1, MIDNIGHT)] {
        let err = w.c.day_totals(from, to, false, &w.m).unwrap_err();
        assert_eq!(err, TotalsRefused::BadWindow);
        assert_eq!(err.rule(), "CR-152");
    }
    // A counter with no records counts nothing.
    assert_eq!(totals(&w, MIDNIGHT, MIDNIGHT + DAY, true).paid.count, 0);
}

/// Staff records of an earlier time, as many as `n`.
fn old_records(w: &mut World, n: u64, at: u64) {
    let e = StaffEvent {
        at,
        change: StaffChange::Added,
        name: "old".to_string(),
        by: "max".to_string(),
    };
    for _ in 0..n {
        w.c.record_staff(&e, &mut w.m).unwrap();
    }
}

#[test]
fn cr_152_the_read_stops_at_a_block_older_than_the_day() {
    let mut w = World::new();
    old_records(&mut w, 1_300, MIDNIGHT - 5_000);
    w.c.set_owner_mark(w.c.records_len());
    w.now = MIDNIGHT + 5;
    open_by(&mut w, &ann());
    // The newest block holds today's sale and the one before is older: only
    // records in memory were read, never the store's.
    assert!(
        w.c.records_base() > 0,
        "the oldest records are in the store"
    );
    let t = totals(&w, MIDNIGHT, MIDNIGHT + DAY, false);
    assert_eq!(t.waiting.count, 1);
    assert_eq!(w.m.reads.get(), 0, "the store was not read");
    // A day that starts before them reads on, down to the store.
    let t = totals(&w, MIDNIGHT - 10_000, MIDNIGHT + DAY, false);
    assert_eq!(t.waiting.count, 1);
    assert!(w.m.reads.get() > 0, "the store was read");
}

#[test]
fn cr_152_a_day_that_needs_too_many_records_is_refused_and_not_counted_in_part() {
    let mut w = World::new();
    old_records(&mut w, 700, MIDNIGHT + 1);
    let err =
        w.c.day_totals_within(MIDNIGHT, MIDNIGHT + DAY, false, 300, &w.m)
            .unwrap_err();
    assert_eq!(err, TotalsRefused::TooManyRecords);
    assert_eq!(err.rule(), "CR-152");
    // With room, the same day is counted.
    assert!(w
        .c
        .day_totals_within(MIDNIGHT, MIDNIGHT + DAY, false, 800, &w.m)
        .is_ok());
}

#[test]
fn cr_152_a_record_log_that_cannot_be_read_is_refused() {
    let mut w = World::new();
    old_records(&mut w, 1_300, MIDNIGHT + 1);
    w.c.set_owner_mark(w.c.records_len());
    w.now = MIDNIGHT + 5;
    open_by(&mut w, &ann());
    w.m.fail_reads = true;
    let err =
        w.c.day_totals(MIDNIGHT - 1, MIDNIGHT + DAY, false, &w.m)
            .unwrap_err();
    assert!(matches!(err, TotalsRefused::Store(_)));
    assert_eq!(err.rule(), "CR-81");
}

#[test]
fn cr_152_what_the_log_says_is_the_same_after_a_restart() {
    let mut w = scripted_day();
    let before = totals(&w, MIDNIGHT, MIDNIGHT + DAY, true);
    w.restart();
    assert_eq!(totals(&w, MIDNIGHT, MIDNIGHT + DAY, true), before);
}
