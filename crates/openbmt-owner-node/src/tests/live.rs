//! The set of scripts set aside, the plan that picks them, their stored form
//! in the coin view and the words of the line about them
//! (`docs/OWNER-COINS-RULES.md`, OC-22 to OC-27).

use std::collections::BTreeSet;

use openbmt_owner::IssuedRange;

use super::counter;
use crate::coins::{CoinView, ViewState, GAP};
use crate::live::{
    plan_release, DeepLine, Spans, WatchLimits, DEEP_CHECK_SECS, DEEP_RETRY_SECS, DEEP_STALL_SECS,
    MAX_RELEASED_SPANS, OWNER_LIVE_WATCH, QUIET_SECS,
};

const DAY: u64 = 86_400;

fn ok(_: &[u8]) -> Result<(), ()> {
    Ok(())
}

fn range(start: u32, count: u32, signed_at: u64) -> IssuedRange {
    IssuedRange {
        start,
        count,
        counter: counter(),
        signed_at,
    }
}

#[test]
fn oc_22_the_numbers_are_the_documented_ones() {
    assert_eq!(OWNER_LIVE_WATCH, 90_000);
    assert_eq!(QUIET_SECS, 60 * DAY);
    assert_eq!(DEEP_CHECK_SECS, 30 * DAY);
    assert_eq!(DEEP_RETRY_SECS, 3_600);
    assert_eq!(DEEP_STALL_SECS, 3_600);
    // Room under the driver's limit for change and growth.
    assert!(OWNER_LIVE_WATCH + 10_000 <= openbmt_net_io::MAX_WATCHED_SCRIPTS);
    let d = WatchLimits::default();
    assert_eq!(
        (d.live, d.quiet_secs, d.deep_secs),
        (OWNER_LIVE_WATCH, QUIET_SECS, DEEP_CHECK_SECS)
    );
    assert!(d.check().is_ok());
    assert!(WatchLimits { live: 0, ..d }.check().is_err());
    assert!(WatchLimits {
        live: openbmt_net_io::MAX_WATCHED_SCRIPTS + 1,
        ..d
    }
    .check()
    .is_err());
}

#[test]
fn oc_22_runs_merge_split_and_count() {
    let mut s = Spans::new();
    assert!(s.is_empty());
    for i in [5, 3, 4, 10, 8, 9, 7] {
        assert!(s.insert(i), "{i}");
    }
    assert!(!s.insert(4), "a second insert is no change");
    assert_eq!(s.pairs().collect::<Vec<_>>(), vec![(3, 6), (7, 11)]);
    assert_eq!(s.len(), 7);
    assert_eq!(s.count_below(5), 2);
    assert_eq!(s.count_below(100), 7);
    assert_eq!(s.count_below(3), 0);
    assert!(s.contains(3) && s.contains(10) && !s.contains(6) && !s.contains(11));
    // Closing the hole joins the runs, and removing from the middle splits.
    assert!(s.insert(6));
    assert_eq!(s.pairs().collect::<Vec<_>>(), vec![(3, 11)]);
    assert!(s.remove(6));
    assert!(!s.remove(6));
    assert!(!s.remove(0));
    assert_eq!(s.pairs().collect::<Vec<_>>(), vec![(3, 6), (7, 11)]);
    assert!(s.remove(3) && s.remove(10));
    assert_eq!(s.iter().collect::<Vec<_>>(), vec![4, 5, 7, 8, 9]);
    // The last index of the range is the highest allowed.
    let mut top = Spans::new();
    assert!(top.insert((1 << 31) - 1));
    assert!(!top.insert(1 << 31));
}

#[test]
fn oc_22_runs_are_read_strictly() {
    assert!(Spans::from_pairs(&[(1, 3), (4, 9)]).is_some());
    // Empty, touching, overlapping, out of order, past the last index.
    assert!(Spans::from_pairs(&[(3, 3)]).is_none());
    assert!(Spans::from_pairs(&[(1, 3), (3, 5)]).is_none());
    assert!(Spans::from_pairs(&[(1, 4), (2, 6)]).is_none());
    assert!(Spans::from_pairs(&[(5, 6), (1, 3)]).is_none());
    assert!(Spans::from_pairs(&[(0, (1 << 31) + 1)]).is_none());
    let many: Vec<(u32, u32)> = (0..=u32::try_from(MAX_RELEASED_SPANS).unwrap())
        .map(|i| (i * 2, i * 2 + 1))
        .collect();
    assert!(Spans::from_pairs(&many).is_none());
}

#[test]
fn oc_23_the_plan_takes_the_oldest_quiet_scripts_and_never_one_with_a_coin() {
    let now = 1_000 * DAY;
    let ranges = [
        range(40, 10, 100 * DAY),
        range(0, 10, 200 * DAY),
        // Signed 10 days ago: not quiet.
        range(10, 10, now - 10 * DAY),
        // Signed exactly 60 days ago: quiet.
        range(20, 10, now - 60 * DAY),
    ];
    let held: BTreeSet<u32> = [41, 22].into_iter().collect();
    let plan = plan_release(&ranges, &Spans::new(), &held, now, 60 * DAY, 1_000);
    // The range signed at 100 days first (without 41), then 200 days, then
    // the one 60 days old (without 22). The recent one is left out.
    let mut want: Vec<u32> = (40..50).filter(|i| *i != 41).collect();
    want.extend(0..10);
    want.extend((20..30).filter(|i| *i != 22));
    assert_eq!(plan, want);
    // At most what is needed, oldest first.
    assert_eq!(
        plan_release(&ranges, &Spans::new(), &held, now, 60 * DAY, 3),
        vec![40, 42, 43]
    );
    // What is set aside already is not chosen again.
    let mut aside = Spans::new();
    for i in 40..45 {
        aside.insert(i);
    }
    assert_eq!(
        plan_release(&ranges, &aside, &held, now, 60 * DAY, 3),
        vec![45, 46, 47]
    );
    assert!(plan_release(&ranges, &Spans::new(), &held, now, 60 * DAY, 0).is_empty());
    // A time before the quiet time has passed gives nothing.
    assert!(plan_release(&ranges, &Spans::new(), &held, 50 * DAY, 60 * DAY, 9).is_empty());
}

#[test]
fn oc_22_a_plan_is_stored_with_the_marks_in_one_write() {
    let mut view = CoinView::new(0);
    let mut writes = 0;
    let changed = view
        .watch_plan(100, 3, 0, &[1, 2, 3, 7], 5_000, |_| {
            writes += 1;
            Ok::<(), ()>(())
        })
        .unwrap();
    assert!(changed);
    assert_eq!(writes, 1);
    assert_eq!(view.receive_next(), 100);
    assert_eq!(view.released().iter().collect::<Vec<_>>(), vec![1, 2, 3, 7]);
    let deep = view.deep();
    assert_eq!((deep.due, deep.last, deep.failed, deep.from), (5_000, 0, false, 0));
    // 120 receive and 23 change watched, 4 set aside.
    assert_eq!(view.live_count(), 120 - 4 + 23);
    // The same plan again changes nothing and writes nothing.
    let again = view
        .watch_plan(100, 3, 0, &[1, 2], 9_999, |_| {
            writes += 1;
            Ok::<(), ()>(())
        })
        .unwrap();
    assert!(!again);
    assert_eq!(writes, 1);
    assert_eq!(view.deep().due, 5_000, "the first due time stays");
    // A write that fails changes nothing.
    assert!(view.watch_plan(100, 3, 0, &[9], 1, |_| Err(())).is_err());
    assert!(!view.released().contains(9));
    // Setting scripts aside needs no rescan; growing the marks does.
    assert_eq!(view.state(), ViewState::Ready);
    view.set_checked(50, ok).unwrap();
    view.watch_plan(100, 3, 50, &[11], 1, ok).unwrap();
    assert_eq!(view.state(), ViewState::Ready);
    assert_eq!(view.checked(), 50);
    view.watch_plan(150, 3, 50, &[], 1, ok).unwrap();
    assert!(matches!(view.state(), ViewState::Rescanning { to: 50 }));
}

#[test]
fn oc_22_the_height_from_which_set_aside_scripts_are_unchecked_only_goes_down() {
    let mut view = CoinView::new(0);
    view.watch_plan(100, 0, 0, &[], 0, ok).unwrap();
    view.set_checked(80, ok).unwrap();
    view.watch_plan(100, 0, 80, &[1], 5, ok).unwrap();
    assert_eq!(view.deep().from, 80);
    view.set_checked(120, ok).unwrap();
    view.watch_plan(100, 0, 120, &[2], 5, ok).unwrap();
    assert_eq!(view.deep().from, 80, "an older script keeps it low");
    // A reorganization takes it down to the fork.
    view.reorganize(&[], 60, ok).unwrap();
    assert_eq!(view.deep().from, 60);
    // A finished check lifts it to its target and no higher.
    view.deep_finished(900, 900 + DEEP_CHECK_SECS, 110, ok).unwrap();
    let deep = view.deep();
    assert_eq!(
        (deep.from, deep.last, deep.due, deep.failed),
        (110, 900, 900 + DEEP_CHECK_SECS, false)
    );
}

#[test]
fn oc_24_a_return_puts_scripts_back_and_rewinds_for_them() {
    let mut view = CoinView::new(0);
    view.set_checked(100, ok).unwrap();
    view.watch_plan(100, 0, 100, &[1, 2, 3], 5, ok).unwrap();
    view.deep_finished(10, 20, 100, ok).unwrap();
    view.set_checked(130, ok).unwrap();
    // Script 2 was paid. The blocks above 100 are read for it by the live
    // watch, and the view is catching up until it has.
    assert!(view.return_to_live(&[2], 100, 130, ok).unwrap());
    assert_eq!(view.released().iter().collect::<Vec<_>>(), vec![1, 3]);
    assert_eq!(view.checked(), 100);
    assert_eq!(view.state(), ViewState::Rescanning { to: 130 });
    // A script that was not set aside is no change.
    assert!(!view.return_to_live(&[2, 50], 0, 130, ok).unwrap());
    // The last one back ends the schedule.
    view.set_checked(130, ok).unwrap();
    assert!(view.return_to_live(&[1, 3], 130, 130, ok).unwrap());
    assert!(view.released().is_empty());
    assert_eq!(view.state(), ViewState::Ready);
    let deep = view.deep();
    assert_eq!((deep.due, deep.from, deep.failed), (0, 0, false));
    assert_eq!(deep.last, 10, "when the last check finished is kept");
    assert!(view.deep_request(ok).unwrap() == false);
}

#[test]
fn oc_26_a_failure_and_a_request_are_kept_in_the_schedule() {
    let mut view = CoinView::new(0);
    view.watch_plan(100, 0, 0, &[4], 5_000, ok).unwrap();
    view.deep_failed(7_000, ok).unwrap();
    assert!(view.deep().failed);
    assert_eq!(view.deep().due, 7_000);
    // The owner asks: due at once, and the failure stays until a check ends.
    assert!(view.deep_request(ok).unwrap());
    assert_eq!(view.deep().due, 1);
    assert!(view.deep().failed);
    view.deep_finished(8_000, 9_000, 5, ok).unwrap();
    assert!(!view.deep().failed);
    assert_eq!(view.deep().due, 9_000);
}

#[test]
fn oc_24_a_wallet_checked_again_from_the_birthday_has_its_deep_check_due_at_once() {
    let mut view = CoinView::new(7);
    view.set_checked(500, ok).unwrap();
    view.watch_plan(100, 0, 500, &[4], 99_999, ok).unwrap();
    view.deep_finished(10, 99_999, 500, ok).unwrap();
    view.reset(ok).unwrap();
    let deep = view.deep();
    assert_eq!((deep.from, deep.due, deep.failed), (7, 1, false));
    assert_eq!(view.released().len(), 1);
}

fn view_with_aside() -> CoinView {
    let mut view = CoinView::new(3);
    view.set_checked(50, ok).unwrap();
    view.watch_plan(100, 4, 50, &[1, 2, 3, 40, 41], 7_000, ok)
        .unwrap();
    view.deep_finished(100, 7_000, 50, ok).unwrap();
    view
}

#[test]
fn oc_27_the_stored_form_holds_the_set_and_the_schedule_and_reads_back() {
    let view = view_with_aside();
    let bytes = view.to_bytes();
    let back = CoinView::from_bytes(&bytes).unwrap();
    assert_eq!(back, view);
    assert_eq!(back.released().pairs().collect::<Vec<_>>(), vec![(1, 4), (40, 42)]);
    assert_eq!(back.deep(), view.deep());
    assert_eq!(back.to_bytes(), bytes);
    // A view with nothing set aside has the same fields, empty.
    let empty = CoinView::new(3);
    assert_eq!(CoinView::from_bytes(&empty.to_bytes()).unwrap(), empty);
}

#[test]
fn oc_27_the_stored_set_is_read_strictly() {
    let view = view_with_aside();
    let bytes = view.to_bytes();
    // Where the set starts: the header is 4 magic, 1 version, 4 birthday,
    // 4 checked, 4 receive, 4 change and 5 for the state.
    let at = 4 + 1 + 16 + 5;
    assert_eq!(&bytes[at..at + 4], &2u32.to_le_bytes());
    // Every cut and every byte after the end is refused.
    for cut in 0..bytes.len() {
        assert!(CoinView::from_bytes(&bytes[..cut]).is_err(), "cut {cut}");
    }
    let mut more = bytes.clone();
    more.push(0);
    assert!(CoinView::from_bytes(&more).is_err());
    // Runs that touch, overlap, are empty or out of order are refused.
    let with = |pairs: &[(u32, u32)]| {
        let mut v = bytes[..at].to_vec();
        v.extend_from_slice(&u32::try_from(pairs.len()).unwrap().to_le_bytes());
        for (s, e) in pairs {
            v.extend_from_slice(&s.to_le_bytes());
            v.extend_from_slice(&e.to_le_bytes());
        }
        v.extend_from_slice(&bytes[at + 4 + 16..]);
        v
    };
    assert_eq!(with(&[(1, 4), (40, 42)]), bytes);
    for bad in [
        vec![(1, 4), (4, 6)],
        vec![(1, 5), (3, 6)],
        vec![(40, 42), (1, 4)],
        vec![(2, 2), (40, 42)],
        vec![(1, (1 << 31) + 1)],
    ] {
        assert!(CoinView::from_bytes(&with(&bad)).is_err(), "{bad:?}");
    }
    // No run at all, while the schedule says something is due.
    assert!(CoinView::from_bytes(&with(&[])).is_err());
    // A failed flag that is not 0 or 1.
    let flag = at + 4 + 16 + 16;
    let mut odd = bytes.clone();
    odd[flag] = 2;
    assert!(CoinView::from_bytes(&odd).is_err());
    // A schedule with nothing set aside, and a set with no due time.
    let mut none = CoinView::new(3).to_bytes();
    let spot = at + 4 + 8;
    none[spot] = 1;
    assert!(CoinView::from_bytes(&none).is_err());
    let mut no_due = bytes.clone();
    for b in &mut no_due[at + 4 + 16 + 8..at + 4 + 16 + 16] {
        *b = 0;
    }
    assert!(CoinView::from_bytes(&no_due).is_err());
    // A flipped byte is refused or reads back as itself.
    for i in at..at + 4 + 16 + 17 {
        let mut flipped = bytes.clone();
        flipped[i] ^= 0x01;
        if let Ok(v) = CoinView::from_bytes(&flipped) {
            assert_eq!(v.to_bytes(), flipped, "flip at {i}");
        }
    }
}

#[test]
fn oc_22_the_gap_is_still_watched_beyond_the_marks() {
    let mut view = CoinView::new(0);
    view.watch_plan(10, 0, 0, &[], 0, ok).unwrap();
    assert_eq!(view.receive_watch(), 10 + GAP);
    assert_eq!(view.live_count(), u64::from(10 + GAP + GAP));
}

// ---- the words (OC-25, OC-26) -----------------------------------------------------

fn words(line: DeepLine) -> openbmt_core::say::Words {
    line.words()
}

#[test]
fn oc_25_the_line_says_how_many_are_checked_monthly_and_when_the_last_and_next_are() {
    let w = words(DeepLine::Monthly {
        count: 1_200,
        days_ago: 12,
        next: 1_800_000_000,
    });
    assert!(w.what.contains("1,200 old addresses are checked once a month"));
    assert!(w.what.contains("shows up at the next check"));
    assert!(w.next.contains("12 days ago"));
    // The next check is a time for the app to show, never seconds in a
    // sentence.
    assert_eq!(w.at, Some(1_800_000_000));
    assert!(!w.what.contains("1800000000") && !w.next.contains("1800000000"));
    assert_eq!(w.code, Some("OC-25"));
    // One address, the singular; and the same day.
    let one = words(DeepLine::Monthly {
        count: 1,
        days_ago: 0,
        next: 5,
    });
    assert!(one.what.starts_with("1 old address is checked"));
    assert!(one.next.contains("today"));
    assert!(words(DeepLine::Monthly {
        count: 2,
        days_ago: 1,
        next: 5
    })
    .next
    .contains("1 day ago"));
    let first = words(DeepLine::FirstCheckWaiting { count: 3, next: 9 });
    assert!(first.next.contains("No check has run yet"));
    assert_eq!(first.at, Some(9));
}

#[test]
fn oc_26_a_check_in_progress_and_a_check_that_failed_have_their_own_words() {
    let busy = words(DeepLine::Checking { count: 40 });
    assert!(busy.what.contains("checking 40 old addresses"));
    assert_eq!(busy.at, None);
    let failed = words(DeepLine::Failed {
        count: 40,
        next: 1_800_000_000,
    });
    assert!(failed.what.contains("did not finish"));
    assert!(failed.next.contains("tries again"));
    assert_eq!(failed.at, Some(1_800_000_000));
    // Four lines, four headings.
    let headings: BTreeSet<String> = [
        DeepLine::FirstCheckWaiting { count: 1, next: 1 },
        DeepLine::Monthly {
            count: 1,
            days_ago: 1,
            next: 1,
        },
        DeepLine::Checking { count: 1 },
        DeepLine::Failed { count: 1, next: 1 },
    ]
    .into_iter()
    .map(|l| l.words().heading)
    .collect();
    assert_eq!(headings.len(), 4);
    for line in [
        DeepLine::FirstCheckWaiting { count: 1, next: 1 },
        DeepLine::Monthly {
            count: 5,
            days_ago: 5,
            next: 1,
        },
        DeepLine::Checking { count: 1_200_000 },
        DeepLine::Failed { count: 1, next: 1 },
    ] {
        openbmt_core::say::check(&line.words()).unwrap();
    }
}
