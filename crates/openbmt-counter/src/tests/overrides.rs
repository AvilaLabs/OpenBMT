//! Section 3: overrides (CR-20 to CR-22).

use super::kit::*;
use crate::{Event, OverrideRefused, Report, Role, State, Why, OVERRIDE_WINDOW_SECS};

/// A sale of `cents` with `sats` seen by one group (so it waits), judged once
/// so that a later call writes nothing.
fn waiting_sale(w: &mut World, cents: u64, sats: u64) -> u32 {
    let o = w.open(cents);
    let t = w.pay(&o, sats);
    w.see(&t, 1);
    w.judge(o.index);
    o.index
}

// CR-20.

#[test]
fn cr_20_a_person_can_accept_a_waiting_sale() {
    let mut w = World::new();
    let i = waiting_sale(&mut w, 500, 5_000);
    assert_eq!(w.judge(i).state, State::Waiting);
    let reports = w.override_sale(i, Role::Cashier, "ann").unwrap();
    assert_eq!(reports, vec![Report::PosPaid { index: i }]);
    let screen = w.judge(i);
    assert_eq!(screen.state, State::Overridden);
    assert_eq!(screen.state.color(), crate::Color::Green);
    assert_eq!(
        screen.why,
        Why::OverriddenBy {
            role: Role::Cashier
        }
    );
    assert!(screen.why_text().contains("cashier"));
}

#[test]
fn cr_20_a_person_can_accept_a_part_paid_sale() {
    let mut w = World::new();
    let i = waiting_sale(&mut w, 500, 2_000);
    assert_eq!(w.judge(i).state, State::PartPaid);
    w.override_sale(i, Role::Manager, "max").unwrap();
    assert_eq!(w.judge(i).state, State::Overridden);
    // The amount at risk is what the sale needs.
    assert_eq!(w.c.exposure_outstanding(), 5_000);
}

#[test]
fn cr_20_a_person_can_accept_a_do_not_hand_over_sale() {
    let mut w = World::new();
    let o = w.open_five();
    let t = w.pay(&o, 5_000);
    w.see(&t, 1);
    w.c.sighting(&bytes(&cancel_of(&t)), peer(30), w.now, &mut w.m)
        .unwrap();
    assert_eq!(w.judge(o.index).state, State::DoNotHandOver);
    w.override_sale(o.index, Role::Manager, "max").unwrap();
    let screen = w.judge(o.index);
    assert_eq!(
        screen.state,
        State::Overridden,
        "the cause that was overridden stays overridden"
    );
}

#[test]
fn cr_20_no_other_state_can_be_accepted() {
    let mut w = World::new();
    let not_paid = w.open_five().index;
    let err = w.override_sale(not_paid, Role::Manager, "max").unwrap_err();
    assert_eq!(err, OverrideRefused::WrongState(State::NotPaidYet));
    assert_eq!(err.rule(), "CR-20");

    let o = w.open_five();
    let t = w.pay(&o, 5_000);
    w.accept_at_once(&o, &t);
    assert_eq!(
        w.override_sale(o.index, Role::Manager, "max").unwrap_err(),
        OverrideRefused::WrongState(State::PaidInstant)
    );
    w.confirm(&[t], 100);
    assert_eq!(
        w.override_sale(o.index, Role::Manager, "max").unwrap_err(),
        OverrideRefused::WrongState(State::Confirmed)
    );

    let o = w.open_five();
    let huge = huge_paying(&o.script);
    w.c.sighting(&bytes(&huge), peer(31), w.now, &mut w.m)
        .unwrap();
    assert_eq!(
        w.override_sale(o.index, Role::Manager, "max").unwrap_err(),
        OverrideRefused::WrongState(State::NeedsAPerson)
    );

    assert_eq!(
        w.override_sale(77, Role::Manager, "max").unwrap_err(),
        OverrideRefused::UnknownSale(77)
    );
    assert_eq!(
        w.c.exposure_outstanding(),
        0,
        "the confirmed sale was released"
    );
}

#[test]
fn cr_20_the_ledger_is_stored_before_the_screen_changes() {
    let mut w = World::new();
    let i = waiting_sale(&mut w, 500, 5_000);
    w.m.reset();
    w.override_sale(i, Role::Cashier, "ann").unwrap();
    assert_eq!(w.m.log, ["Exposure", "Desk", "record", "Sale(0)"]);
    assert_eq!(w.c.exposure_outstanding(), 5_000);
}

#[test]
fn cr_20_a_ledger_with_no_room_refuses_the_override() {
    let mut w = World::new();
    let mut s = settings();
    s.light_budget.sats = 4_000;
    w.c.set_settings(s);
    let i = waiting_sale(&mut w, 500, 5_000);
    let err = w.override_sale(i, Role::Manager, "max").unwrap_err();
    assert_eq!(err, OverrideRefused::NoRoom);
    assert_eq!(err.rule(), "CR-20");
    assert_eq!(w.c.exposure_outstanding(), 0);
    assert_eq!(w.judge(i).state, State::Waiting);
    assert!(w.c.desk().overrides().is_empty());
}

#[test]
fn cr_20_a_sale_already_counted_adds_nothing_but_the_limits_still_count() {
    // Accepted at once, turned red by a conflict, then accepted by a person.
    let mut w = World::new();
    let o = w.open_five();
    let t = w.pay(&o, 5_000);
    w.accept_at_once(&o, &t);
    w.c.sighting(&bytes(&cancel_of(&t)), peer(30), w.now, &mut w.m)
        .unwrap();
    assert_eq!(w.judge(o.index).state, State::DoNotHandOver);
    assert_eq!(w.c.exposure_outstanding(), 5_000);
    w.override_sale(o.index, Role::Cashier, "ann").unwrap();
    assert_eq!(w.c.exposure_outstanding(), 5_000, "not counted twice");
    assert_eq!(
        w.c.desk().override_total(Role::Cashier, "ann", w.now),
        5_000
    );
}

// CR-21.

#[test]
fn cr_21_a_cashier_is_refused_above_the_per_sale_instant_limit() {
    let mut w = World::new();
    let i = waiting_sale(&mut w, 500, 11_000);
    let err = w.override_sale(i, Role::Cashier, "ann").unwrap_err();
    assert_eq!(
        err,
        OverrideRefused::OverSaleLimit {
            sats: 11_000,
            limit: 10_000
        }
    );
    assert_eq!(err.rule(), "CR-21");
    assert_eq!(w.c.exposure_outstanding(), 0);
    // A manager is not held to the per-sale limit.
    w.override_sale(i, Role::Manager, "max").unwrap();
    assert_eq!(w.judge(i).state, State::Overridden);
}

#[test]
fn cr_21_a_cashier_is_held_to_the_daily_cap_over_24_hours() {
    let mut w = World::new();
    let ids: Vec<u32> = (0..3).map(|_| waiting_sale(&mut w, 700, 7_000)).collect();
    w.override_sale(ids[0], Role::Cashier, "ann").unwrap();
    w.override_sale(ids[1], Role::Cashier, "ann").unwrap();
    let err = w.override_sale(ids[2], Role::Cashier, "ann").unwrap_err();
    assert_eq!(
        err,
        OverrideRefused::OverDailyCap {
            sats: 7_000,
            already: 14_000,
            cap: 20_000
        }
    );
    assert_eq!(err.rule(), "CR-21");
    // Another cashier has a cap of her own.
    w.override_sale(ids[2], Role::Cashier, "bob").unwrap();
}

#[test]
fn cr_21_a_manager_is_held_to_the_ceiling() {
    let mut w = World::new();
    let mut s = settings();
    s.manager_override_ceiling.sats = 10_000;
    w.c.set_settings(s);
    let a = waiting_sale(&mut w, 800, 8_000);
    let b = waiting_sale(&mut w, 800, 8_000);
    w.override_sale(a, Role::Manager, "max").unwrap();
    let err = w.override_sale(b, Role::Manager, "max").unwrap_err();
    assert_eq!(
        err,
        OverrideRefused::OverCeiling {
            sats: 8_000,
            already: 8_000,
            ceiling: 10_000
        }
    );
    assert_eq!(err.rule(), "CR-21");
    // A cashier's total is not the manager's: names and roles are kept apart.
    w.override_sale(b, Role::Cashier, "max").unwrap();
}

#[test]
fn cr_21_the_window_forgets_after_24_hours() {
    let mut w = World::new();
    let ids: Vec<u32> = (0..3)
        .map(|_| waiting_sale(&mut w, 1_000, 10_000))
        .collect();
    w.override_sale(ids[0], Role::Cashier, "ann").unwrap();
    w.override_sale(ids[1], Role::Cashier, "ann").unwrap();
    assert!(w.override_sale(ids[2], Role::Cashier, "ann").is_err());
    w.tick(OVERRIDE_WINDOW_SECS - 1);
    assert!(w.override_sale(ids[2], Role::Cashier, "ann").is_err());
    w.tick(1);
    w.override_sale(ids[2], Role::Cashier, "ann").unwrap();
    // The two old entries were pruned when the desk was written.
    assert_eq!(w.c.desk().overrides().len(), 1);
    assert_eq!(
        w.c.desk().override_total(Role::Cashier, "ann", w.now),
        10_000
    );
}

#[test]
fn cr_21_the_totals_survive_a_restart() {
    let mut w = World::new();
    let ids: Vec<u32> = (0..3)
        .map(|_| waiting_sale(&mut w, 1_000, 10_000))
        .collect();
    w.override_sale(ids[0], Role::Cashier, "ann").unwrap();
    w.override_sale(ids[1], Role::Cashier, "ann").unwrap();
    w.restart();
    assert!(matches!(
        w.override_sale(ids[2], Role::Cashier, "ann"),
        Err(OverrideRefused::OverDailyCap { .. })
    ));
}

#[test]
fn cr_21_the_person_needs_a_name() {
    let mut w = World::new();
    let i = waiting_sale(&mut w, 500, 5_000);
    let long = "x".repeat(65);
    for name in ["", long.as_str()] {
        let err = w.override_sale(i, Role::Manager, name).unwrap_err();
        assert_eq!(err, OverrideRefused::BadName);
        assert_eq!(err.rule(), "CR-22");
    }
    assert!(w.override_sale(i, Role::Manager, &"x".repeat(64)).is_ok());
}

// CR-22.

#[test]
fn cr_22_an_override_is_recorded_with_who_when_and_how_much() {
    let mut w = World::new();
    let i = waiting_sale(&mut w, 500, 5_000);
    w.tick(33);
    w.override_sale(i, Role::Cashier, "ann").unwrap();
    let event = w.m.events().pop().unwrap();
    assert_eq!(
        event,
        Event::Override {
            index: i,
            at: w.now,
            role: Role::Cashier,
            name: "ann".to_string(),
            sats: 5_000
        }
    );
    let entry = &w.c.desk().overrides()[0];
    assert_eq!(
        (entry.role, entry.name.as_str(), entry.at, entry.sats),
        (Role::Cashier, "ann", w.now, 5_000)
    );
    let mark = w.c.sale(i).unwrap().overridden().unwrap();
    assert_eq!(
        (mark.role, mark.name.as_str(), mark.at),
        (Role::Cashier, "ann", w.now)
    );
}

#[test]
fn cr_22_an_override_does_not_clear_a_cancel_alarm() {
    let mut w = World::new();
    let a = w.open_five();
    let t = w.pay(&a, 5_000);
    w.accept_at_once(&a, &t);
    w.c.sighting(&bytes(&cancel_of(&t)), peer(30), w.now, &mut w.m)
        .unwrap();
    w.judge(a.index);
    assert!(w.c.alarm());
    let b = waiting_sale(&mut w, 500, 5_000);
    w.override_sale(b, Role::Manager, "max").unwrap();
    assert!(w.c.alarm());
    w.override_sale(a.index, Role::Manager, "max").unwrap();
    assert!(w.c.alarm());
}

#[test]
fn cr_22_an_override_does_not_make_the_sale_confirmed() {
    let mut w = World::new();
    let o = w.open_five();
    let t = w.pay(&o, 5_000);
    w.see(&t, 1);
    w.override_sale(o.index, Role::Manager, "max").unwrap();
    let screen = w.judge(o.index);
    assert_eq!(screen.state, State::Overridden);
    assert_ne!(screen.state, State::Confirmed);
    assert_eq!(w.c.exposure_outstanding(), 5_000, "it stays in the ledger");
    w.confirm(&[t], 100);
    assert_eq!(w.judge(o.index).state, State::Confirmed);
    assert_eq!(w.c.exposure_outstanding(), 0);
}

#[test]
fn cr_22_an_overridden_sale_goes_red_again_only_for_a_new_cause() {
    let mut w = World::new();
    let o = w.open_five();
    let t = w.pay(&o, 5_000);
    w.see(&t, 1);
    w.c.sighting(&bytes(&cancel_of(&t)), peer(30), w.now, &mut w.m)
        .unwrap();
    assert_eq!(w.judge(o.index).state, State::DoNotHandOver);
    w.override_sale(o.index, Role::Manager, "max").unwrap();
    assert_eq!(w.judge(o.index).state, State::Overridden);
    assert!(!w.c.alarm(), "the old conflict is not a cancel");

    // The payment is mined, then the block leaves the chain: a new cause.
    let block = w.confirm(&[t], 100);
    assert_eq!(w.judge(o.index).state, State::Confirmed);
    w.orphan(&block);
    let screen = w.judge(o.index);
    assert_eq!(
        (screen.state, screen.why),
        (State::DoNotHandOver, Why::Reopened)
    );
    assert!(screen.reports.contains(&Report::Alarm { index: o.index }));
    assert!(screen
        .reports
        .contains(&Report::PosUnpaid { index: o.index }));
    assert!(w.c.alarm());
}
