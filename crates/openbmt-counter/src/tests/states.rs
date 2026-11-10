//! Section 2: what the cashier sees (CR-10 to CR-19).

use openbmt_core::command::Action;
use openbmt_core::policy::{Exposure, OffReason, PauseStatus, WaitReason};
use openbmt_watch::Problem;

use super::kit::*;
use crate::{
    sale_payment_id, Color, Event, Next, Report, Role, Screen, Slot, State, Why, EXPIRY_SECS,
    STUCK_SECS,
};

fn conflict_with(w: &mut World, tx: &bitcoin::Transaction) {
    let cancel = cancel_of(tx);
    w.c.sighting(&bytes(&cancel), peer(30), w.now, &mut w.m)
        .unwrap();
}

// CR-10.

#[test]
fn cr_10_every_state_has_its_color_and_two_texts() {
    let mut w = World::new();
    let not_paid = w.open_five();
    let part = w.open_five();
    let t = w.pay(&part, 2_000);
    w.see(&t, 1);
    let waiting = w.open_five();
    let t = w.pay(&waiting, 5_000);
    w.see(&t, 1);
    let confirmed = w.open_five();
    let t = w.pay(&confirmed, 5_000);
    w.confirm(&[t], 100);
    let fin = w.open_five();
    let t = w.pay(&fin, 5_000);
    w.confirm(&[t], 95);
    let red = w.open_five();
    let t = w.pay(&red, 5_000);
    w.see(&t, 1);
    conflict_with(&mut w, &t);
    let person = w.open_five();
    let huge = huge_paying(&person.script);
    w.c.sighting(&bytes(&huge), peer(31), w.now, &mut w.m)
        .unwrap();
    let overridden = w.open_five();
    let t = w.pay(&overridden, 5_000);
    w.see(&t, 1);
    w.override_sale(overridden.index, Role::Cashier, "ann")
        .unwrap();
    let instant = w.open_five();
    let t = w.pay(&instant, 5_000);
    w.accept_at_once(&instant, &t);

    let cases = [
        (
            not_paid.index,
            State::NotPaidYet,
            Color::Blue,
            "Nothing has arrived",
        ),
        (
            part.index,
            State::PartPaid,
            Color::Blue,
            "Paid 2000 of 5000",
        ),
        (
            waiting.index,
            State::Waiting,
            Color::Blue,
            "enough of the network",
        ),
        (
            instant.index,
            State::PaidInstant,
            Color::Green,
            "Paid (light check)",
        ),
        (confirmed.index, State::Confirmed, Color::Green, "Confirmed"),
        (overridden.index, State::Overridden, Color::Green, "cashier"),
        (
            red.index,
            State::DoNotHandOver,
            Color::Red,
            "conflicting payment",
        ),
        (
            person.index,
            State::NeedsAPerson,
            Color::Red,
            "more than the counter can judge",
        ),
    ];
    for (index, state, color, words) in cases {
        let screen = w.judge(index);
        assert_eq!(screen.state, state, "sale {index}");
        assert_eq!(screen.state.color(), color);
        assert!(screen.why_text().contains(words), "{}", screen.why_text());
        assert!(!screen.next_text().is_empty(), "{state:?} has a next text");
    }
    let screen = w.judge(fin.index);
    assert_eq!(screen.state, State::Final);
    assert_eq!(screen.state.color(), Color::Green);

    let mut w = World::new();
    let a = w.open_five();
    let b = w.open_five();
    w.c.cancel(b.index, w.now, &mut w.m).unwrap();
    w.tick(EXPIRY_SECS);
    let expired = w.judge(a.index);
    assert_eq!(expired.state, State::Expired);
    assert_eq!(expired.state.color(), Color::Neutral);
    let cancelled = w.judge(b.index);
    assert_eq!(cancelled.state, State::Cancelled);
    assert_eq!(cancelled.state.color(), Color::Neutral);
}

#[test]
fn cr_10_part_paid_is_shown_as_paid_x_of_y() {
    let mut w = World::new();
    let o = w.open_five();
    let t = w.pay(&o, 2_000);
    w.see(&t, 1);
    let screen = w.judge(o.index);
    assert_eq!(
        screen.why,
        Why::Part {
            received_sats: 2_000,
            needs_sats: 5_000
        }
    );
    assert_eq!(
        screen.next,
        Next::CustomerPaysRest {
            missing_sats: 3_000
        }
    );
    assert_eq!(screen.why.text(), "Paid 2000 of 5000 sats.");
}

#[test]
fn cr_10_waiting_carries_the_reason_from_the_instant_decision() {
    let mut w = World::new();
    w.remote_order = true;
    let o = w.open_five();
    let t = w.pay(&o, 5_000);
    w.tick(10);
    w.see(&t, 3);
    w.tick(8);
    let screen = w.judge(o.index);
    assert_eq!(screen.state, State::Waiting);
    assert_eq!(screen.why, Why::Wait(WaitReason::RemoteOrder));
    assert!(screen.why_text().contains("remote order"));
}

#[test]
fn cr_10_paid_instant_names_the_check_that_was_done() {
    let mut w = World::new();
    let o = w.open_five();
    let t = w.pay(&o, 5_000);
    let screen = w.accept_at_once(&o, &t);
    assert!(screen.why_text().contains("Paid (light check)"));
    assert!(screen.why_text().contains("could still be cancelled"));
}

// CR-11.

#[test]
fn cr_11_the_state_is_worked_out_again_every_time_it_is_asked_for() {
    let mut w = World::new();
    let o = w.open_five();
    let t = w.pay(&o, 5_000);
    w.confirm(&[t], 100);
    assert_eq!(w.judge(o.index).state, State::Confirmed);
    // The shop asks for a deeper confirmation: nothing is shown from memory.
    let mut s = settings();
    s.min_confirmations = 3;
    s.final_confirmations = 6;
    w.c.set_settings(s);
    assert_eq!(w.judge(o.index).state, State::Waiting);
    w.c.set_settings(settings());
    assert_eq!(w.judge(o.index).state, State::Confirmed);
    w.chain.tip = 105;
    assert_eq!(w.judge(o.index).state, State::Final);
    w.chain.tip = 100;
    assert_eq!(w.judge(o.index).state, State::Confirmed);
}

#[test]
fn cr_11_a_payment_that_arrives_between_two_calls_changes_the_screen() {
    let mut w = World::new();
    let o = w.open_five();
    assert_eq!(w.judge(o.index).state, State::NotPaidYet);
    let t = w.pay(&o, 2_000);
    w.see(&t, 1);
    assert_eq!(w.judge(o.index).state, State::PartPaid);
    let t = w.pay(&o, 3_000);
    w.see(&t, 1);
    assert_eq!(w.judge(o.index).state, State::Waiting);
}

// CR-12: the order of judgment. The earlier rule wins.

fn world_with_confirmed_sale(
    height: u32,
) -> (World, crate::Opened, bitcoin::Transaction, bitcoin::Block) {
    let mut w = World::new();
    let o = w.open_five();
    let t = w.pay(&o, 5_000);
    w.see(&t, 1);
    let b = w.confirm(std::slice::from_ref(&t), height);
    // The watcher compares with its answer of the last call to see a block
    // leave the chain, so the sale is judged once here.
    w.judge(o.index);
    (w, o, t, b)
}

/// Rules 1 and 2 hold: the watcher needs a person and a block left the chain.
fn rule_1_and_2() -> Screen {
    let (mut w, o, _, b) = world_with_confirmed_sale(100);
    let huge = huge_paying(&o.script);
    w.c.sighting(&bytes(&huge), peer(31), w.now, &mut w.m)
        .unwrap();
    w.orphan(&b);
    w.judge(o.index)
}

/// Rules 2 and 3 hold: a block left the chain and a conflict was seen.
fn rule_2_and_3() -> Screen {
    let (mut w, o, t, b) = world_with_confirmed_sale(100);
    conflict_with(&mut w, &t);
    w.orphan(&b);
    w.judge(o.index)
}

/// Rules 3 and 4 hold: a conflict was seen and the payment is at the refund
/// depth. The conflict does not count against a confirmed sale.
fn rule_3_and_4() -> Screen {
    let mut w = World::new();
    let o = w.open_five();
    let t = w.pay(&o, 5_000);
    w.see(&t, 1);
    conflict_with(&mut w, &t);
    w.confirm(&[t], 95);
    w.judge(o.index)
}

/// Rules 4 and 5 hold: six confirmations are also one.
fn rule_4_and_5() -> Screen {
    let (mut w, o, _, _) = world_with_confirmed_sale(95);
    w.judge(o.index)
}

/// Rules 5 and 6 hold: accepted at once and now confirmed.
fn rule_5_and_6() -> Screen {
    let mut w = World::new();
    let o = w.open_five();
    let t = w.pay(&o, 5_000);
    w.accept_at_once(&o, &t);
    w.confirm(&[t], 100);
    w.judge(o.index)
}

/// Rules 6 and 7 hold: accepted at once, and now the check is not healthy.
fn rule_6_and_7() -> Screen {
    let mut w = World::new();
    let o = w.open_five();
    let t = w.pay(&o, 5_000);
    w.accept_at_once(&o, &t);
    w.health = unhealthy(Problem::TipIsOld);
    w.judge(o.index)
}

/// Rules 7 and 8 hold: enough has arrived, so it is not "part paid".
fn rule_7_and_8() -> Screen {
    let mut w = World::new();
    let o = w.open_five();
    let t = w.pay(&o, 5_000);
    w.see(&t, 1);
    w.judge(o.index)
}

/// Rules 8 and 9 hold: something has arrived, so it is not "not paid yet".
fn rule_8_and_9() -> Screen {
    let mut w = World::new();
    let o = w.open_five();
    let t = w.pay(&o, 2_000);
    w.see(&t, 1);
    w.tick(EXPIRY_SECS - 1);
    w.judge(o.index)
}

#[test]
fn cr_12_each_rule_wins_over_the_one_after_it() {
    #[allow(clippy::type_complexity)]
    let table: [(&str, fn() -> Screen, State, Option<Why>); 8] = [
        (
            "1 over 2",
            rule_1_and_2,
            State::NeedsAPerson,
            Some(Why::WatcherNeedsAPerson),
        ),
        (
            "2 over 3",
            rule_2_and_3,
            State::DoNotHandOver,
            Some(Why::Reopened),
        ),
        ("3 over 4", rule_3_and_4, State::Final, None),
        ("4 over 5", rule_4_and_5, State::Final, None),
        ("5 over 6", rule_5_and_6, State::Confirmed, None),
        ("6 over 7", rule_6_and_7, State::PaidInstant, None),
        ("7 over 8", rule_7_and_8, State::Waiting, None),
        ("8 over 9", rule_8_and_9, State::PartPaid, None),
    ];
    for (name, scenario, state, why) in table {
        let screen = scenario();
        assert_eq!(screen.state, state, "{name}");
        if let Some(why) = why {
            assert_eq!(screen.why, why, "{name}");
        }
    }
}

#[test]
fn cr_12_a_conflict_on_a_sale_that_is_not_confirmed_is_do_not_hand_over() {
    let mut w = World::new();
    let o = w.open_five();
    let t = w.pay(&o, 5_000);
    w.see(&t, 1);
    conflict_with(&mut w, &t);
    let screen = w.judge(o.index);
    assert_eq!(screen.state, State::DoNotHandOver);
    assert_eq!(screen.why, Why::Replaced);
    assert!(screen.next_text().contains("Do not hand over"));
}

#[test]
fn cr_12_an_unhealthy_check_makes_an_unaccepted_sale_do_not_hand_over() {
    let mut w = World::new();
    let o = w.open_five();
    let t = w.pay(&o, 5_000);
    w.tick(10);
    w.see(&t, 3);
    w.tick(8);
    w.health = unhealthy(Problem::TooFewPeerGroups);
    let screen = w.judge(o.index);
    assert_eq!(screen.state, State::DoNotHandOver);
    assert_eq!(
        screen.why,
        Why::CannotCheck(Some(Problem::TooFewPeerGroups))
    );
}

#[test]
fn cr_12_a_payment_seen_by_too_few_groups_waits_and_says_why() {
    let mut w = World::new();
    let o = w.open_five();
    let t = w.pay(&o, 5_000);
    w.see(&t, 2);
    let screen = w.judge(o.index);
    assert_eq!(screen.state, State::Waiting);
    assert_eq!(screen.why, Why::NotSeenEnough);
    assert_eq!(screen.why_text(), "Not yet seen by enough of the network.");
}

#[test]
fn cr_12_a_block_that_left_the_chain_reopens_until_the_payment_is_confirmed_again() {
    let (mut w, o, t, b) = world_with_confirmed_sale(100);
    assert_eq!(w.judge(o.index).state, State::Confirmed);
    w.orphan(&b);
    let screen = w.judge(o.index);
    assert_eq!(
        (screen.state, screen.why),
        (State::DoNotHandOver, Why::Reopened)
    );
    // The latch holds on the next call too, though the watcher reports it once.
    assert_eq!(w.judge(o.index).state, State::DoNotHandOver);
    w.chain.tip = 101;
    w.confirm(&[t], 101);
    assert_eq!(w.judge(o.index).state, State::Confirmed);
}

// CR-13.

#[test]
fn cr_13_the_exposure_is_counted_under_the_sale_and_stored_before_paid_is_returned() {
    let id7 = sale_payment_id(7).0;
    assert_eq!(&id7[..4], b"sale");
    assert_eq!(&id7[4..8], &7u32.to_le_bytes());
    assert!(id7[8..].iter().all(|b| *b == 0));

    let mut w = World::new();
    let o = w.open_five();
    let t = w.pay(&o, 5_000);
    w.tick(10);
    w.see(&t, 3);
    w.tick(8);
    w.m.reset();
    let screen = w.judge(o.index);
    assert_eq!(screen.state, State::PaidInstant);
    // PaidAtOnce and PosPaid are two records, both before the sale.
    assert_eq!(w.m.log, ["Exposure", "record", "record", "Sale(0)"]);
    let mut expected = Exposure::new();
    expected
        .accept_instant(sale_payment_id(0), 5_000, 50_000, |_| Ok::<(), ()>(()))
        .unwrap();
    assert_eq!(w.m.slots[&Slot::Exposure], expected.to_bytes());
    assert_eq!(w.c.exposure_outstanding(), 5_000);
    let events = w.m.events();
    assert!(matches!(
        events.iter().rev().nth(1).unwrap(),
        Event::PaidAtOnce {
            index: 0,
            at_risk_sats: 5_000,
            ..
        }
    ));
    assert!(matches!(
        events.last().unwrap(),
        Event::PosPaid { index: 0, .. }
    ));
}

#[test]
fn cr_13_a_fee_bump_does_not_change_the_exposure_entry() {
    let mut w = World::new();
    let o = w.open_five();
    let t = w.pay(&o, 5_000);
    w.accept_at_once(&o, &t);
    let before = w.m.slots[&Slot::Exposure].clone();
    let bumped = bump(&t, 5_500);
    let sighted =
        w.c.sighting(&bytes(&bumped), peer(40), w.now, &mut w.m)
            .unwrap();
    assert!(matches!(
        sighted.results.as_slice(),
        [(0, Ok(openbmt_watch::Sighted::Replaced))]
    ));
    let screen = w.judge(o.index);
    assert_eq!(screen.state, State::PaidInstant);
    assert_eq!(w.m.slots[&Slot::Exposure], before);
    assert_eq!(w.c.exposure_outstanding(), 5_000);
}

#[test]
fn cr_13_a_worse_later_decision_does_not_take_the_green_screen_back() {
    let mut w = World::new();
    let o = w.open_five();
    let t = w.pay(&o, 5_000);
    w.accept_at_once(&o, &t);
    // Every fact the decision reads turns bad.
    w.health = unhealthy(Problem::HeadersBehind);
    w.remote_order = true;
    let mut s = settings();
    s.light_per_sale.sats = 0;
    w.c.set_settings(s);
    let screen = w.judge(o.index);
    assert_eq!(screen.state, State::PaidInstant);
    assert!(screen.reports.is_empty());
}

#[test]
fn cr_13_only_rules_1_to_3_take_the_green_screen_back() {
    let mut w = World::new();
    let o = w.open_five();
    let t = w.pay(&o, 5_000);
    w.accept_at_once(&o, &t);
    conflict_with(&mut w, &t);
    assert_eq!(w.judge(o.index).state, State::DoNotHandOver);
}

#[test]
fn cr_13_a_failed_store_gives_waiting_and_no_report() {
    let mut w = World::new();
    let o = w.open_five();
    let t = w.pay(&o, 5_000);
    w.tick(10);
    w.see(&t, 3);
    w.tick(8);
    w.m.reset();
    w.m.fail_on = Some(1); // the exposure ledger
    let screen = w.judge(o.index);
    assert_eq!(screen.state, State::Waiting);
    assert_eq!(screen.why, Why::NotStored);
    assert!(screen.reports.is_empty());
    assert_eq!(w.c.exposure_outstanding(), 0);
    assert_eq!(w.c.sale(0).unwrap().accepted(), None);
    w.m.reset();
    let screen = w.judge(o.index);
    assert_eq!(screen.state, State::PaidInstant);
    assert_eq!(screen.reports, vec![Report::PosPaid { index: 0 }]);
}

#[test]
fn cr_13_an_earlier_attempt_that_stored_the_exposure_and_stopped_is_carried_on() {
    let mut w = World::new();
    let o = w.open_five();
    let t = w.pay(&o, 5_000);
    w.tick(10);
    w.see(&t, 3);
    w.tick(8);
    w.m.reset();
    w.m.fail_on = Some(2); // the record, after the ledger
    let screen = w.judge(o.index);
    assert_eq!(screen.state, State::Waiting);
    assert!(screen.reports.is_empty());
    assert_eq!(w.c.exposure_outstanding(), 5_000);
    w.m.reset();
    let screen = w.judge(o.index);
    assert_eq!(screen.state, State::PaidInstant);
    assert_eq!(screen.reports, vec![Report::PosPaid { index: 0 }]);
    assert_eq!(w.c.exposure_outstanding(), 5_000, "not counted twice");
}

#[test]
fn cr_13_a_ledger_with_no_room_gives_waiting() {
    let mut w = World::new();
    let mut s = settings();
    s.light_budget.sats = 4_000;
    w.c.set_settings(s);
    let o = w.open_five();
    let t = w.pay(&o, 5_000);
    w.tick(10);
    w.see(&t, 3);
    w.tick(8);
    let screen = w.judge(o.index);
    assert_eq!(screen.state, State::Waiting);
    assert_eq!(screen.why, Why::Wait(WaitReason::SliceFull));
    assert_eq!(w.c.exposure_outstanding(), 0);
}

#[test]
fn cr_13_the_slice_of_the_mode_the_decision_used_is_the_one_charged() {
    // A Box report only: its slice is $50 as well, so shrink only that one.
    let mut w = World::new();
    let mut s = settings();
    s.box_budget.sats = 4_000;
    w.c.set_settings(s);
    let o = w.open_five();
    let t = w.pay(&o, 5_000);
    w.tick(10);
    w.see_by_box(&t);
    w.tick(8);
    let screen = w.judge(o.index);
    assert_eq!(
        screen.check_mode,
        Some(openbmt_core::policy::CheckMode::BoxReportsOnly)
    );
    assert_eq!(screen.why, Why::Wait(WaitReason::SliceFull));
}

// CR-14.

fn cancelled_world() -> (World, crate::Opened, bitcoin::Transaction) {
    let mut w = World::new();
    let o = w.open_five();
    let t = w.pay(&o, 5_000);
    w.accept_at_once(&o, &t);
    conflict_with(&mut w, &t);
    (w, o, t)
}

#[test]
fn cr_14_a_conflict_after_acceptance_is_a_cancel_with_a_pause_and_an_alarm() {
    let (mut w, o, _) = cancelled_world();
    assert!(!w.c.alarm());
    let screen = w.judge(o.index);
    assert_eq!(
        (screen.state, screen.why),
        (State::DoNotHandOver, Why::Replaced)
    );
    assert!(screen.reports.contains(&Report::Alarm { index: 0 }));
    assert!(screen.reports.contains(&Report::PosUnpaid { index: 0 }));
    assert!(w.c.alarm());
    assert_eq!(
        w.c.pause_status(w.now),
        PauseStatus::PausedFor { secs_left: 3_600 }
    );
    assert!(w
        .m
        .events()
        .iter()
        .any(|e| matches!(e, Event::Replaced { index: 0, .. })));
}

#[test]
fn cr_14_the_cancel_is_recorded_in_the_pause_tracker_once_per_sale() {
    let (mut w, o, _) = cancelled_world();
    for _ in 0..4 {
        w.judge(o.index);
        w.tick(5);
    }
    // A second count would have locked the pause until the owner resets it.
    assert!(matches!(
        w.c.pause_status(w.now),
        PauseStatus::PausedFor { .. }
    ));
    let conflicts =
        w.m.events()
            .iter()
            .filter(|e| matches!(e, Event::Replaced { .. }))
            .count();
    assert_eq!(conflicts, 1);
}

#[test]
fn cr_14_the_alarm_stays_through_judge_calls_and_through_a_reload() {
    let (mut w, o, _) = cancelled_world();
    w.judge(o.index);
    w.tick(1_000_000);
    w.judge(o.index);
    assert!(w.c.alarm(), "nothing at the counter clears it");
    w.restart();
    assert!(w.c.alarm());
    let screen = w.judge(o.index);
    assert_eq!(screen.state, State::DoNotHandOver);
    assert!(
        screen.reports.is_empty(),
        "reported once, not again after a restart"
    );
    assert!(w.c.alarm());
}

#[test]
fn cr_14_only_a_signed_command_clears_the_alarm_and_a_replay_is_refused() {
    let (mut w, o, _) = cancelled_world();
    w.judge(o.index);
    w.command(1, Action::ClearCancelAlarm).unwrap();
    assert!(!w.c.alarm());
    w.restart();
    assert!(!w.c.alarm(), "the clearing was stored");
    // The same command again.
    let err = w.command(1, Action::ClearCancelAlarm).unwrap_err();
    assert_eq!(
        err,
        crate::CommandRefused::Refused(openbmt_core::command::CommandError::Stale {
            last: 1,
            got: 1
        })
    );
    assert_eq!(err.rule(), "CR-14");
    // An older number too.
    assert!(w.command(0, Action::ClearCancelAlarm).is_err());
    // The sale is still not safe, and the alarm is not raised a second time.
    let screen = w.judge(o.index);
    assert_eq!(screen.state, State::DoNotHandOver);
    assert!(!w.c.alarm());
}

#[test]
fn cr_14_a_command_must_be_for_this_counter_and_signed_by_the_owner() {
    let (mut w, o, _) = cancelled_world();
    w.judge(o.index);
    // Signed by the Key app, not the owner.
    let mut forged = w.owner_command(5, Action::ClearCancelAlarm);
    forged.owner_sig = forged.command.sign(&id(2));
    let err = w.c.command(&forged, w.now, &mut w.m).unwrap_err();
    assert_eq!(
        err,
        crate::CommandRefused::Refused(openbmt_core::command::CommandError::OwnerSignatureInvalid)
    );
    // For another counter.
    let mut other = w.owner_command(5, Action::ClearCancelAlarm);
    other.command.counter = id(4).device_id();
    other.owner_sig = other.command.sign(&id(1));
    let err = w.c.command(&other, w.now, &mut w.m).unwrap_err();
    assert_eq!(
        err,
        crate::CommandRefused::Refused(openbmt_core::command::CommandError::WrongCounter)
    );
    assert!(w.c.alarm());
}

#[test]
fn cr_14_resetting_a_pause_and_releasing_exposure_are_the_owners_alone() {
    let (mut w, o, _) = cancelled_world();
    w.judge(o.index);
    assert!(matches!(
        w.c.pause_status(w.now),
        PauseStatus::PausedFor { .. }
    ));
    assert_eq!(w.c.exposure_outstanding(), 5_000);
    w.tick(10_000);
    w.judge(o.index);
    assert_eq!(w.c.exposure_outstanding(), 5_000, "no timeout releases it");

    w.command(1, Action::ResetPause).unwrap();
    assert_eq!(w.c.pause_status(w.now), PauseStatus::Running);
    w.command(2, Action::ReleaseExposure(sale_payment_id(0)))
        .unwrap();
    assert_eq!(w.c.exposure_outstanding(), 0);
    w.restart();
    assert_eq!(w.c.pause_status(w.now), PauseStatus::Running);
    assert_eq!(w.c.exposure_outstanding(), 0);
    assert_eq!(w.c.desk().last_command_seq(), 2);
}

#[test]
fn cr_14_a_second_cancel_within_a_day_pauses_until_the_owner_resets() {
    let (mut w, o, _) = cancelled_world();
    w.judge(o.index);
    w.tick(3_601);
    let b = w.open_five();
    let t = w.pay(&b, 5_000);
    w.accept_at_once(&b, &t);
    conflict_with(&mut w, &t);
    w.judge(b.index);
    assert_eq!(w.c.pause_status(w.now), PauseStatus::UntilOwnerReset);
    w.tick(30_000);
    let c = w.open_five();
    let t = w.pay(&c, 5_000);
    w.tick(10);
    w.see(&t, 3);
    w.tick(8);
    let screen = w.judge(c.index);
    assert_eq!(
        screen.why,
        Why::InstantOff(OffReason::PausedUntilOwnerReset)
    );
    assert_eq!(screen.next, Next::OwnerClears);
    w.command(1, Action::ResetPause).unwrap();
    assert_eq!(w.judge(c.index).state, State::PaidInstant);
}

#[test]
fn cr_14_a_pause_that_could_not_be_saved_is_written_again_before_the_next_acceptance() {
    let (mut w, o, _) = cancelled_world();
    w.m.reset();
    w.m.fail_on = Some(1); // the pause is the first write of a cancel
    let screen = w.judge(o.index);
    assert_eq!(screen.state, State::DoNotHandOver);
    assert!(w.c.pause_unsaved());
    assert!(!w.m.slots.contains_key(&Slot::Pause));
    assert!(w.c.alarm(), "the rest of the cancel still happened");

    // The pause has run out in memory, but it was never saved: the next
    // acceptance is held until it is.
    w.tick(3_601);
    let b = w.open_five();
    let t = w.pay(&b, 5_000);
    w.tick(10);
    w.see(&t, 3);
    w.tick(8);
    w.m.reset();
    w.m.fail_on = Some(1); // the retry of the pause fails again
    let screen = w.judge(b.index);
    assert_eq!(screen.state, State::Waiting);
    assert_eq!(screen.why, Why::PauseNotSaved);
    // Sale A's exposure is still counted; B's is not.
    assert_eq!(w.c.exposure_outstanding(), 5_000);
    assert_eq!(w.c.sale(1).unwrap().accepted(), None);

    w.m.reset();
    let screen = w.judge(b.index);
    assert_eq!(screen.state, State::PaidInstant);
    assert!(!w.c.pause_unsaved());
    assert!(w.m.slots.contains_key(&Slot::Pause));
}

// CR-15.

#[test]
fn cr_15_exposure_leaves_the_ledger_at_the_first_confirmation() {
    let mut w = World::new();
    let o = w.open_five();
    let t = w.pay(&o, 5_000);
    w.accept_at_once(&o, &t);
    assert_eq!(w.c.exposure_outstanding(), 5_000);
    w.confirm(&[t], 100);
    let screen = w.judge(o.index);
    assert_eq!(screen.state, State::Confirmed);
    assert_eq!(w.c.exposure_outstanding(), 0);
    assert!(w.c.sale(0).unwrap().exposure_released());
    w.restart();
    assert_eq!(w.c.exposure_outstanding(), 0, "the release was stored");
}

#[test]
fn cr_15_the_first_confirmation_releases_even_when_the_shop_wants_more() {
    let mut w = World::new();
    let mut s = settings();
    s.min_confirmations = 2;
    w.c.set_settings(s);
    let o = w.open_five();
    let t = w.pay(&o, 5_000);
    w.accept_at_once(&o, &t);
    w.confirm(&[t], 100);
    let screen = w.judge(o.index);
    assert_eq!(
        screen.state,
        State::PaidInstant,
        "one is not the shop's depth"
    );
    assert_eq!(w.c.exposure_outstanding(), 0);
}

#[test]
fn cr_15_never_on_a_timeout() {
    let mut w = World::new();
    let o = w.open_five();
    let t = w.pay(&o, 5_000);
    w.accept_at_once(&o, &t);
    w.tick(10 * STUCK_SECS);
    let screen = w.judge(o.index);
    assert_eq!(screen.state, State::PaidInstant);
    assert_eq!(w.c.exposure_outstanding(), 5_000);
}

#[test]
fn cr_15_and_not_when_the_sale_turns_red() {
    let (mut w, o, _) = cancelled_world();
    assert_eq!(w.judge(o.index).state, State::DoNotHandOver);
    assert_eq!(w.c.exposure_outstanding(), 5_000);
}

// CR-18.

#[test]
fn cr_18_a_payment_not_confirming_for_two_hours_is_stuck_and_nothing_else_changes() {
    let mut w = World::new();
    let o = w.open_five();
    let t = w.pay(&o, 5_000);
    w.accept_at_once(&o, &t);
    // The clock started when the third group saw it, 8 seconds ago.
    w.tick(STUCK_SECS - 9);
    let screen = w.judge(o.index);
    assert!(!screen.stuck);
    w.tick(1);
    let screen = w.judge(o.index);
    assert!(screen.stuck);
    assert_eq!(
        screen.state,
        State::PaidInstant,
        "the state does not change"
    );
    assert!(screen.why_text().contains("Stuck: not confirming"));
    assert_eq!(
        w.c.sale(0).unwrap().ended(),
        None,
        "the sale does not close"
    );
    // Confirmed, it is no longer stuck.
    w.confirm(&[t], 100);
    assert!(!w.judge(o.index).stuck);
}

// CR-19.

#[test]
fn cr_19_a_payment_to_an_expired_sale_is_recorded_flagged_and_shown_as_late() {
    let mut w = World::new();
    let a = w.open_five();
    let b = w.open_five();
    w.tick(EXPIRY_SECS);
    assert_eq!(w.judge(a.index).state, State::Expired);

    let t = w.pay(&a, 5_000);
    let sighted = w.see(&t, 1);
    assert_eq!(
        sighted.reports,
        vec![Report::LatePayment {
            index: 0,
            sats: 5_000
        }]
    );
    assert!(sighted.results.is_empty(), "no live sale took it");
    let screen = w.judge(a.index);
    assert_eq!(screen.state, State::Expired);
    assert!(screen.late);
    assert!(screen.why_text().contains("Late payment"));
    assert!(w.c.sale(0).unwrap().is_late());
    assert!(w.m.events().iter().any(|e| matches!(
        e,
        Event::LatePayment {
            index: 0,
            sats: 5_000,
            ..
        }
    )));

    // It never changes another sale.
    let other = w.c.sale(b.index).unwrap();
    assert_eq!(other.watch().transactions().count(), 0);
    assert_eq!(w.judge(b.index).state, State::Expired);
}

#[test]
fn cr_19_a_late_payment_is_reported_once_however_often_it_is_seen_or_mined() {
    let mut w = World::new();
    let a = w.open_five();
    w.c.cancel(a.index, w.now, &mut w.m).unwrap();
    let t = w.pay(&a, 5_000);
    let first = w.see(&t, 1);
    assert_eq!(first.reports.len(), 1);
    let again = w.see(&t, 3);
    assert!(again.reports.is_empty());
    let block = w.mine_at(std::slice::from_ref(&t), 100);
    assert!(w.offer(&block).reports.is_empty());
    let late: Vec<_> =
        w.m.events()
            .into_iter()
            .filter(|e| matches!(e, Event::LatePayment { .. }))
            .collect();
    assert_eq!(late.len(), 1);
    // A different payment is a second late payment.
    let t2 = w.pay(&a, 1_500);
    w.chain.tip = 101;
    let block = w.mine_at(&[t2], 101);
    let taken = w.offer(&block);
    assert_eq!(
        taken.reports,
        vec![Report::LatePayment {
            index: 0,
            sats: 1_500
        }]
    );
    assert_eq!(w.c.sale(0).unwrap().late_payments().len(), 2);
}

#[test]
fn cr_19_a_late_payment_survives_a_restart_and_is_not_reported_again() {
    let mut w = World::new();
    let a = w.open_five();
    w.c.cancel(a.index, w.now, &mut w.m).unwrap();
    let t = w.pay(&a, 5_000);
    w.see(&t, 1);
    w.restart();
    assert!(w.judge(a.index).late);
    assert!(w.see(&t, 1).reports.is_empty());
}

#[test]
fn cr_19_the_sale_s_own_payment_seen_again_after_final_is_not_late() {
    let mut w = World::new();
    let o = w.open_five();
    let t = w.pay(&o, 5_000);
    w.confirm(std::slice::from_ref(&t), 95);
    let screen = w.judge(o.index);
    assert_eq!(screen.state, State::Final);
    assert!(w.c.sale(0).unwrap().books_final());
    assert!(w.see(&t, 3).reports.is_empty());
    assert!(!w.judge(o.index).late);
    // A second, different payment to the address is.
    let t2 = w.pay(&o, 700);
    let sighted = w.see(&t2, 1);
    assert_eq!(
        sighted.reports,
        vec![Report::LatePayment {
            index: 0,
            sats: 700
        }]
    );
    assert!(w.judge(o.index).late);
    assert_eq!(w.judge(o.index).state, State::Final);
}
