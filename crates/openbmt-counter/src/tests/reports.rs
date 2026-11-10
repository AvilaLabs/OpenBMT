//! Section 4: what leaves the counter (CR-30 to CR-32).

use super::kit::*;
use crate::{Amount, Event, JudgeError, Report, Role, State};

fn kinds(w: &World) -> Vec<&'static str> {
    w.m.events()
        .iter()
        .map(|e| match e {
            Event::Opened { .. } => "opened",
            Event::Expired { .. } => "expired",
            Event::Cancelled { .. } => "cancelled",
            Event::PaidAtOnce { .. } => "paid-at-once",
            Event::Confirmed { .. } => "confirmed",
            Event::Final { .. } => "final",
            Event::LatePayment { .. } => "late",
            Event::SurplusOwed { .. } => "surplus",
            Event::Dust { .. } => "dust",
            Event::Override { .. } => "override",
            Event::Replaced { .. } => "replaced",
            Event::Reopened { .. } => "reopened",
            Event::PosPaid { .. } => "pos-paid",
            Event::PosUnpaid { .. } => "pos-unpaid",
        })
        .collect()
}

/// The record that belongs to a report is among the stored records.
fn has_record(w: &World, report: &Report) -> bool {
    let events = w.m.events();
    match report {
        Report::PosPaid { index } => events
            .iter()
            .any(|e| e.index() == *index && matches!(e, Event::PosPaid { .. })),
        Report::PosUnpaid { index } => events
            .iter()
            .any(|e| e.index() == *index && matches!(e, Event::PosUnpaid { .. })),
        Report::Alarm { index } => events.iter().any(|e| {
            e.index() == *index && matches!(e, Event::Replaced { .. } | Event::Reopened { .. })
        }),
        Report::BooksFinal { index, .. } => events
            .iter()
            .any(|e| e.index() == *index && matches!(e, Event::Final { .. })),
        Report::SurplusOwed { index, sats } => events.iter().any(|e| {
            matches!(e, Event::SurplusOwed { index: i, sats: s, .. } if i == index && s == sats)
        }),
        Report::LatePayment { index, sats } => events.iter().any(|e| {
            matches!(e, Event::LatePayment { index: i, sats: s, .. } if i == index && s == sats)
        }),
        Report::Dust { index, sats } => events.iter().any(|e| {
            matches!(e, Event::Dust { index: i, sats: s, .. } if i == index && s == sats)
        }),
    }
}

// The whole journey.

#[test]
fn cr_32_the_whole_journey_reports_each_change_once_after_its_record() {
    let mut w = World::new();
    let mut all: Vec<Report> = Vec::new();
    let mut take = |w: &World, reports: Vec<Report>| {
        for r in &reports {
            assert!(has_record(w, r), "no record yet for {r:?}");
        }
        all.extend(reports);
    };

    let o = w.open_five();
    assert_eq!(kinds(&w), ["opened"]);
    let t = w.pay(&o, 5_000);
    w.tick(10);
    w.see(&t, 3); // three network groups
    let s = w.judge(o.index);
    assert_eq!(s.state, State::Waiting, "the 8-second conflict watch runs");
    assert!(s.reports.is_empty());
    w.tick(8);
    let s = w.judge(o.index);
    assert_eq!(s.state, State::PaidInstant);
    assert_eq!(s.reports, vec![Report::PosPaid { index: 0 }]);
    take(&w, s.reports);
    assert_eq!(w.c.exposure_outstanding(), 5_000);
    let s = w.judge(o.index);
    assert!(s.reports.is_empty(), "once");

    w.confirm(&[t], 100);
    let s = w.judge(o.index);
    assert_eq!(s.state, State::Confirmed);
    assert!(s.reports.is_empty(), "already closed as paid in the POS");
    assert_eq!(
        w.c.exposure_outstanding(),
        0,
        "the confirmation released it"
    );
    take(&w, s.reports);

    w.chain.tip = 105;
    let s = w.judge(o.index);
    assert_eq!(s.state, State::Final);
    assert_eq!(
        s.reports,
        vec![Report::BooksFinal {
            index: 0,
            amount: Amount(500),
            currency: usd(),
            received_sats: 5_000
        }]
    );
    take(&w, s.reports);
    let s = w.judge(o.index);
    assert!(s.reports.is_empty());

    assert_eq!(all.len(), 2);
    assert_eq!(
        kinds(&w),
        ["opened", "paid-at-once", "pos-paid", "confirmed", "final"]
    );
}

// CR-30.

#[test]
fn cr_30_the_pos_closes_the_sale_on_a_paid_state_and_on_nothing_else() {
    let mut w = World::new();
    let o = w.open_five();
    assert!(w.judge(o.index).reports.is_empty());
    let first = w.pay(&o, 2_000);
    w.see(&first, 1);
    let s = w.judge(o.index);
    assert_eq!(s.state, State::PartPaid);
    assert!(s.reports.is_empty());
    let second = w.pay(&o, 3_000);
    w.see(&second, 1);
    // Enough, but only waiting.
    let s = w.judge(o.index);
    assert_eq!(s.state, State::Waiting);
    assert!(s.reports.is_empty());
    // Confirmed: now.
    w.confirm(&[first, second], 100);
    let s = w.judge(o.index);
    assert_eq!(s.state, State::Confirmed);
    assert_eq!(s.reports, vec![Report::PosPaid { index: 0 }]);
}

#[test]
fn cr_30_a_sale_that_turns_do_not_hand_over_after_it_closed_is_reported_unpaid_again() {
    let mut w = World::new();
    let o = w.open_five();
    let t = w.pay(&o, 5_000);
    let block = w.confirm(std::slice::from_ref(&t), 100);
    let s = w.judge(o.index);
    assert_eq!(s.reports, vec![Report::PosPaid { index: 0 }]);
    w.orphan(&block);
    let s = w.judge(o.index);
    assert_eq!(s.state, State::DoNotHandOver);
    assert_eq!(s.reports, vec![Report::PosUnpaid { index: 0 }]);
    assert!(w.judge(o.index).reports.is_empty());
    // Confirmed again: closed as paid again, with a record.
    w.chain.tip = 101;
    w.confirm(&[t], 101);
    let s = w.judge(o.index);
    assert_eq!(s.state, State::Confirmed);
    assert_eq!(s.reports, vec![Report::PosPaid { index: 0 }]);
    assert!(has_record(&w, &s.reports[0]));
    assert_eq!(kinds(&w).iter().filter(|k| **k == "confirmed").count(), 2);
}

#[test]
fn cr_30_an_overridden_sale_is_closed_as_paid_by_the_override() {
    let mut w = World::new();
    let o = w.open_five();
    let t = w.pay(&o, 5_000);
    w.see(&t, 1);
    let reports = w.override_sale(o.index, Role::Manager, "max").unwrap();
    assert_eq!(reports, vec![Report::PosPaid { index: 0 }]);
    assert!(has_record(&w, &reports[0]));
    assert!(w.judge(o.index).reports.is_empty());
}

#[test]
fn cr_30_a_sale_with_nothing_paid_reports_nothing() {
    let mut w = World::new();
    let o = w.open_five();
    assert!(w.judge(o.index).reports.is_empty());
    w.tick(crate::EXPIRY_SECS);
    let s = w.judge(o.index);
    assert_eq!(s.state, State::Expired);
    assert!(s.reports.is_empty());
}

// CR-31.

#[test]
fn cr_31_the_books_count_a_sale_as_money_only_at_final() {
    let mut w = World::new();
    let o = w.open_five();
    let t = w.pay(&o, 5_000);
    w.confirm(&[t], 100);
    for tip in [100u32, 103, 104] {
        w.chain.tip = tip;
        let s = w.judge(o.index);
        assert_ne!(s.state, State::Final, "tip {tip}");
        assert!(
            !s.reports
                .iter()
                .any(|r| matches!(r, Report::BooksFinal { .. })),
            "tip {tip}"
        );
    }
    w.chain.tip = 105;
    let s = w.judge(o.index);
    assert_eq!(s.state, State::Final);
    assert_eq!(s.reports.len(), 1);
    assert!(matches!(s.reports[0], Report::BooksFinal { index: 0, .. }));
    assert!(w.c.sale(0).unwrap().books_final());
}

#[test]
fn cr_31_a_sale_mined_deep_at_once_is_closed_and_final_in_the_same_call() {
    let mut w = World::new();
    let o = w.open_five();
    let t = w.pay(&o, 5_000);
    w.confirm(&[t], 95);
    let s = w.judge(o.index);
    assert_eq!(s.state, State::Final);
    assert_eq!(s.reports.len(), 2);
    assert_eq!(s.reports[0], Report::PosPaid { index: 0 });
    assert!(matches!(s.reports[1], Report::BooksFinal { .. }));
    for r in &s.reports {
        assert!(has_record(&w, r));
    }
}

// CR-32.

#[test]
fn cr_32_every_report_has_its_record_stored() {
    let mut w = World::new();
    // Surplus, dust and alarm along the way.
    let dusty = w.open_five();
    let d = w.pay(&dusty, 600);
    w.see(&d, 1);
    let mut reports = w.judge(dusty.index).reports;

    let o = w.open_five();
    let t = w.pay(&o, 6_000);
    w.tick(10);
    w.see(&t, 3);
    w.tick(8);
    reports.extend(w.judge(o.index).reports);
    w.c.sighting(&bytes(&cancel_of(&t)), peer(30), w.now, &mut w.m)
        .unwrap();
    reports.extend(w.judge(o.index).reports);

    let late = w.open_five();
    w.c.cancel(late.index, w.now, &mut w.m).unwrap();
    let l = w.pay(&late, 2_000);
    reports.extend(w.see(&l, 1).reports);

    let kinds_seen: Vec<&str> = reports
        .iter()
        .map(|r| match r {
            Report::PosPaid { .. } => "pos-paid",
            Report::PosUnpaid { .. } => "pos-unpaid",
            Report::BooksFinal { .. } => "books",
            Report::SurplusOwed { .. } => "surplus",
            Report::LatePayment { .. } => "late",
            Report::Alarm { .. } => "alarm",
            Report::Dust { .. } => "dust",
        })
        .collect();
    for want in ["pos-paid", "pos-unpaid", "surplus", "late", "alarm", "dust"] {
        assert!(kinds_seen.contains(&want), "{want} in {kinds_seen:?}");
    }
    for r in &reports {
        assert!(has_record(&w, r), "{r:?}");
    }
}

#[test]
fn cr_32_a_report_is_not_made_when_its_record_cannot_be_stored() {
    let mut w = World::new();
    let o = w.open_five();
    let t = w.pay(&o, 5_000);
    w.confirm(&[t], 100);
    w.m.reset();
    w.m.fail_on = Some(1); // the record
    let err = w.c.judge(
        o.index,
        &crate::Facts {
            chain: &w.chain,
            health: w.health,
            heartbeat: heartbeat(),
            remote_order: false,
            now: w.now,
        },
        &mut w.m,
    );
    let err = err.unwrap_err();
    assert!(matches!(err, JudgeError::Store(_)));
    assert_eq!(err.rule(), "CR-40");
    assert_eq!(w.c.sale(0).unwrap().pos(), crate::PosState::Nothing);
    // Asked again, it is reported once.
    w.m.reset();
    let s = w.judge(o.index);
    assert_eq!(s.reports, vec![Report::PosPaid { index: 0 }]);
    assert!(w.judge(o.index).reports.is_empty());
}

#[test]
fn cr_32_a_record_without_its_sale_is_written_again_and_reported_once() {
    let mut w = World::new();
    let o = w.open_five();
    let t = w.pay(&o, 5_000);
    w.confirm(&[t], 100);
    w.m.reset();
    w.m.fail_on = Some(2); // the sale, after the record
    let facts = crate::Facts {
        chain: &w.chain,
        health: w.health,
        heartbeat: heartbeat(),
        remote_order: false,
        now: w.now,
    };
    assert!(w.c.judge(o.index, &facts, &mut w.m).is_err());
    w.m.reset();
    let s = w.judge(o.index);
    assert_eq!(s.reports, vec![Report::PosPaid { index: 0 }]);
    // The record was written twice, which is the safe side of a lost write.
    assert_eq!(kinds(&w).iter().filter(|k| **k == "confirmed").count(), 2);
    assert!(w.judge(o.index).reports.is_empty());
}
