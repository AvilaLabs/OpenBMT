//! SR-31 and SR-32: the clock, the allowlist wait and the windows.

use super::kit::*;
use crate::{OutputKind, DAY_SECS, WEEK_SECS};

fn never(_: &[u8]) -> Result<(), &'static str> {
    panic!("a call that changes nothing reached the store");
}

#[test]
fn sr_31_the_current_time_is_the_larger_of_the_clock_and_the_latest_time_seen() {
    let mut s = state();
    assert_eq!(s.effective_time(100), 100);
    sign_ok(&mut s, &simple(10_000, 500), NOW);
    assert_eq!(s.latest_time_seen(), NOW);
    assert_eq!(s.effective_time(NOW - 1), NOW);
    assert_eq!(s.effective_time(NOW + 1), NOW + 1);
}

#[test]
fn sr_31_a_clock_moved_back_changes_nothing() {
    let script = external(40);
    let mut s = state();
    s.enroll_destination(script.clone(), "Cash-out", NOW, ok)
        .unwrap();
    // The clock reads a day later and something is signed.
    let day_later = NOW + DAY_SECS;
    sign_ok(&mut s, &simple(20_000, 500), day_later);
    // Then the clock is set back to the time of enrollment.
    let p = simple_to(script, 50_000, 1_000);
    let back = review_ok(&s, &p, NOW);
    let forward = review_ok(&s, &p, day_later);
    // The entry is judged at the later time: it is active.
    assert!(matches!(
        back.outputs()[0].kind,
        OutputKind::Allowlisted { .. }
    ));
    assert_eq!(back.outputs(), forward.outputs());
    assert_eq!(back.remaining(), forward.remaining());
    assert_eq!(back.clock_behind_by(), Some(DAY_SECS));
    assert_eq!(forward.clock_behind_by(), None);
    // Totals are measured from the later time too.
    assert_eq!(s.totals(NOW).other_day, 20_500);
    assert_eq!(s.totals(NOW), s.totals(day_later));
}

#[test]
fn sr_31_signing_and_enrolling_store_the_latest_time_seen_and_nothing_else_does() {
    let mut s = state();
    assert_eq!(s.latest_time_seen(), 0);
    let p = simple(10_000, 500);
    // A review is only a read.
    let _ = review_ok(&s, &p, NOW);
    assert_eq!(s.latest_time_seen(), 0);
    // Changing ceilings and marks does not move it either.
    s.set_ceilings(s.ceilings_other(), s.ceilings_allowlisted(), ok)
        .unwrap();
    s.raise_change_mark(2, ok).unwrap();
    assert_eq!(s.latest_time_seen(), 0);
    s.enroll_destination(external(1), "a", NOW, ok).unwrap();
    assert_eq!(s.latest_time_seen(), NOW);
    // A clock that is behind enrolls at the latest time seen.
    s.enroll_destination(external(2), "b", NOW - 500, ok)
        .unwrap();
    let times: Vec<_> = s.allowlist().map(|e| e.enrolled_at()).collect();
    assert_eq!(times, vec![NOW, NOW]);
    sign_ok(&mut s, &p, NOW + 7);
    assert_eq!(s.latest_time_seen(), NOW + 7);
}

#[test]
fn sr_31_correcting_a_clock_that_is_not_behind_does_nothing() {
    let mut s = state();
    s.enroll_destination(external(1), "a", NOW, ok).unwrap();
    let before = s.to_bytes();
    assert_eq!(s.correct_clock(NOW, never), Ok(()));
    assert_eq!(s.correct_clock(NOW + 1_000, never), Ok(()));
    assert_eq!(s.to_bytes(), before);
}

#[test]
fn sr_31_correcting_the_clock_restarts_waits_and_redates_the_log() {
    let far = NOW + 100_000;
    let mut s = state();
    // The clock ran far ahead; an entry was enrolled and a payment signed.
    s.enroll_destination(external(1), "late", far, ok).unwrap();
    sign_ok(&mut s, &simple(20_000, 500), far);
    // An earlier entry and payment, made at the true time, are untouched.
    let mut early = state();
    early
        .enroll_destination(external(2), "early", NOW, ok)
        .unwrap();
    sign_ok(&mut early, &simple(10_000, 500), NOW);
    early
        .enroll_destination(external(1), "late", far, ok)
        .unwrap();
    sign_ok(&mut early, &simple(20_000, 500), far);

    let true_now = NOW + 100;
    s.correct_clock(true_now, ok).unwrap();
    assert_eq!(s.latest_time_seen(), true_now);
    assert_eq!(s.allowlist().next().unwrap().enrolled_at(), true_now);
    assert_eq!(s.log().next().unwrap().time(), true_now);

    early.correct_clock(true_now, ok).unwrap();
    let enrolled: Vec<_> = early
        .allowlist()
        .map(|e| (e.label().to_string(), e.enrolled_at()))
        .collect();
    assert!(enrolled.contains(&("early".to_string(), NOW)));
    assert!(enrolled.contains(&("late".to_string(), true_now)));
    let times: Vec<_> = early.log().map(|e| e.time()).collect();
    assert!(times.contains(&NOW) && times.contains(&true_now));
    assert_eq!(times.len(), 2);

    // The wait for the corrected entry starts again from the true time.
    let p = simple_to(external(1), 50_000, 1_000);
    let kind = |t: u64| review_ok(&s, &p, t).outputs()[0].kind.clone();
    assert_eq!(kind(true_now + 86_399), OutputKind::Other);
    assert!(matches!(
        kind(true_now + 86_400),
        OutputKind::Allowlisted { .. }
    ));
    // The totals no longer stand still until the far-off date.
    assert_eq!(s.totals(true_now + DAY_SECS).other_day, 0);
    assert_eq!(s.totals(true_now + DAY_SECS).other_week, 20_500);
}

#[test]
fn sr_32_the_day_and_week_windows_have_exact_edges() {
    let mut s = state();
    sign_ok(&mut s, &simple(50_000, 1_000), NOW);
    let at = |dt: u64| s.totals(NOW + dt);
    assert_eq!(at(0).other_day, 51_000);
    assert_eq!(at(DAY_SECS - 1).other_day, 51_000);
    assert_eq!(at(DAY_SECS).other_day, 0);
    assert_eq!(at(DAY_SECS).other_week, 51_000);
    assert_eq!(at(WEEK_SECS - 1).other_week, 51_000);
    assert_eq!(at(WEEK_SECS).other_week, 0);
    assert_eq!(at(WEEK_SECS).other_day, 0);
}

#[test]
fn sr_32_allowlisted_totals_have_the_same_edges() {
    let script = external(40);
    let mut s = state();
    s.enroll_destination(script.clone(), "Cash-out", NOW, ok)
        .unwrap();
    let t0 = NOW + DAY_SECS;
    sign_ok(&mut s, &simple_to(script, 1_000_000, 1_000), t0);
    assert_eq!(s.totals(t0 + DAY_SECS - 1).allowlisted_day, 1_000_000);
    assert_eq!(s.totals(t0 + DAY_SECS).allowlisted_day, 0);
    assert_eq!(s.totals(t0 + WEEK_SECS - 1).allowlisted_week, 1_000_000);
    assert_eq!(s.totals(t0 + WEEK_SECS).allowlisted_week, 0);
}

#[test]
fn sr_32_a_ceiling_that_was_full_opens_again_when_the_day_has_passed() {
    let mut s = state();
    s.set_ceilings(
        crate::Ceilings {
            per_tx: 100_000,
            per_day: 51_000,
            per_week: 1_000_000,
        },
        s.ceilings_allowlisted(),
        ok,
    )
    .unwrap();
    sign_ok(&mut s, &simple(50_000, 1_000), NOW);
    let p = simple(50_000, 1_000);
    assert!(s.review(&p.serialize(), NOW + DAY_SECS - 1).is_err());
    assert!(s.review(&p.serialize(), NOW + DAY_SECS).is_ok());
}

#[test]
fn sr_32_entries_older_than_a_week_are_dropped_at_the_next_signature() {
    let mut s = state();
    let first = sign_ok(&mut s, &simple(50_000, 1_000), NOW);
    sign_ok(&mut s, &simple(40_000, 1_000), NOW + WEEK_SECS - 1);
    assert_eq!(s.log().count(), 2);
    let third = sign_ok(&mut s, &simple(30_000, 1_000), NOW + WEEK_SECS);
    let ids: Vec<_> = s.log().map(|e| e.txid()).collect();
    assert_eq!(ids.len(), 2);
    assert!(!ids.contains(&first.txid));
    assert!(ids.contains(&third.txid));
}

#[test]
fn sr_32_signing_the_same_transaction_after_a_week_counts_it_again() {
    let mut s = state();
    let p = simple(50_000, 1_000);
    sign_ok(&mut s, &p, NOW);
    let later = NOW + WEEK_SECS;
    let r = review_ok(&s, &p, later);
    assert!(!r.already_counted());
}
