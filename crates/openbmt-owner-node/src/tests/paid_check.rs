//! A counter's Paid, checked against the owner's own view of the wallet
//! (`docs/OWNER-COINS-RULES.md`, OC-60 to OC-65).

use openbmt_core::records::RecordKind;
use openbmt_core::wire::DecodeError;
use openbmt_counter::{Amount, Event, Role};
use openbmt_owner::{SaleFacts, SaleKey};
use openbmt_pos::FactsError;

use super::pos::{at_once, link_with, opened};
use super::refunds::{new_owner, view_with, DEPTH, TIP};
use super::*;
use crate::coins::CoinView;
use crate::facts::{verified_facts, FactsRefused};
use crate::paid_check::{
    check_paid, day_summary, default_tolerance_bps, gate_open_alarms, gather, paid_sales,
    paid_sales_from_chain, CheckResult, Fold, LightningCounters, MarkState, PaidCheckRefused,
    PaidChecks, PaidHow, PaidMarks, PaidSale, PaidSales, PaidState, PaidTrouble,
    LIGHTNING_COUNTERS_FILE, MAGIC_LIGHTNING_COUNTERS, MAGIC_PAID_CHECK,
    MAX_OPEN_ALARMS_PER_COUNTER, MAX_PAID_MARKS, PAID_CHECK_FILE, PAID_CHECK_WAIT_SECS,
};

const PAID_AT: u64 = 1_800_000_000;
/// The length of a mark in the file.
const MARK: usize = 61;

fn key(index: u32) -> SaleKey {
    SaleKey {
        counter: counter(),
        index,
    }
}

fn sale(index: u32, recorded: u64, how: PaidHow) -> PaidSale {
    PaidSale {
        counter: counter(),
        index,
        cashier: "Ana".to_string(),
        recorded_sats: recorded,
        quoted_sats: 0,
        how,
        tolerance_bps: 50,
    }
}

/// The wallet's answer from a view that holds `paid`, as `verified_facts`
/// gives it.
fn from_view(view: &CoinView) -> impl Fn(SaleKey) -> Result<SaleFacts, FactsRefused> + '_ {
    let owner = new_owner();
    move |k| verified_facts(view, &owner, k, TIP, DEPTH)
}

fn state_of(result: &CheckResult, index: u32) -> PaidState {
    result
        .checks
        .iter()
        .find(|c| c.sale == key(index))
        .map(|c| c.state)
        .unwrap()
}

/// The marks after the owner phone first read these sales at `at`, with the
/// view not ready: nothing is judged, the readings are marked.
fn read_at(sales: &[PaidSale], at: u64) -> PaidMarks {
    let result = check_paid(sales, &PaidMarks::new(), at, |_| {
        Err(FactsRefused::ViewNotReady)
    });
    PaidMarks::new().after(&result, at)
}

#[test]
fn oc_61_a_paid_with_no_receipt_raises_the_alarm_after_the_wait_and_not_before() {
    let view = view_with(&[]);
    let sales = [sale(0, 5_000, PaidHow::AtOnce)];
    let read = PAID_AT;
    let marks = read_at(&sales, read);
    // One second short of the wait: listed as waiting, nothing raised.
    let early = check_paid(
        &sales,
        &marks,
        read + PAID_CHECK_WAIT_SECS - 1,
        from_view(&view),
    );
    assert_eq!(state_of(&early, 0), PaidState::Waiting);
    assert!(early.raised.is_empty());
    // At the wait it is a mismatch, raised.
    let late = check_paid(
        &sales,
        &marks,
        read + PAID_CHECK_WAIT_SECS,
        from_view(&view),
    );
    assert_eq!(state_of(&late, 0), PaidState::Mismatch);
    assert_eq!(late.raised, vec![key(0)]);
    assert_eq!(late.checks[0].seen_sats, Some(0));
    assert_eq!(late.checks[0].first_read, read);
}

#[test]
fn oc_61_a_first_reading_ahead_of_the_phones_clock_is_taken_as_now() {
    let view = view_with(&[]);
    let sales = [sale(0, 5_000, PaidHow::AtOnce)];
    let now = PAID_AT;
    let ahead = now + 10 * 365 * 86_400;
    let marks = read_at(&sales, ahead);
    assert_eq!(marks.get(&key(0)).unwrap().first_read, ahead);
    // The clock is set right: the mark is rewritten to now, and it waits.
    let result = check_paid(&sales, &marks, now, from_view(&view));
    assert_eq!(state_of(&result, 0), PaidState::Waiting);
    assert_eq!(result.checks[0].first_read, now);
    let marks = marks.after(&result, now);
    assert_eq!(marks.get(&key(0)).unwrap().first_read, now);
    // A run a day and a second later raises the alarm.
    let late = check_paid(
        &sales,
        &marks,
        now + PAID_CHECK_WAIT_SECS + 1,
        from_view(&view),
    );
    assert_eq!(state_of(&late, 0), PaidState::Mismatch);
    assert_eq!(late.raised, vec![key(0)]);
}

#[test]
fn oc_61_the_wait_starts_at_the_first_read_by_the_owners_clock() {
    let view = view_with(&[]);
    let sales = [sale(0, 5_000, PaidHow::AtOnce)];
    // The first check reads the Paid: the clock starts there, and it is kept.
    let dir = Dir::new();
    let mut checks = PaidChecks::open(dir.path());
    let first = PAID_AT + 5 * PAID_CHECK_WAIT_SECS;
    let result = checks.check(&sales, first, from_view(&view));
    assert_eq!(state_of(&result, 0), PaidState::Waiting);
    assert_eq!(result.read, vec![key(0)]);
    assert!(
        result.raised.is_empty(),
        "a Paid old by the counter's clock still waits"
    );
    let mark = *checks.marks().get(&key(0)).unwrap();
    assert_eq!((mark.state, mark.first_read), (MarkState::Reading, first));
    // A new start reads it back, and the wait is still counted from the first read.
    let mut again = PaidChecks::open(dir.path());
    let result = again.check(&sales, first + PAID_CHECK_WAIT_SECS - 1, from_view(&view));
    assert_eq!(state_of(&result, 0), PaidState::Waiting);
    assert!(result.read.is_empty(), "not read again");
    let result = again.check(&sales, first + PAID_CHECK_WAIT_SECS, from_view(&view));
    assert_eq!(state_of(&result, 0), PaidState::Mismatch);
    assert_eq!(result.raised, vec![key(0)]);
    // The raised mark keeps the time of the first read.
    let mark = *again.marks().get(&key(0)).unwrap();
    assert_eq!(mark.first_read, first);
    assert_eq!(mark.raised_at, Some(first + PAID_CHECK_WAIT_SECS));
}

#[test]
fn oc_61_the_wait_is_a_day_and_is_marked_to_tune() {
    assert_eq!(PAID_CHECK_WAIT_SECS, 86_400);
}

#[test]
fn oc_62_a_mismatch_is_raised_once_per_sale() {
    let view = view_with(&[]);
    let sales = [sale(0, 5_000, PaidHow::Confirmed)];
    let marks = read_at(&sales, PAID_AT);
    let now = PAID_AT + PAID_CHECK_WAIT_SECS;
    let first = check_paid(&sales, &marks, now, from_view(&view));
    let marks = marks.after(&first, now);
    let again = check_paid(&sales, &marks, now + 60, from_view(&view));
    assert_eq!(state_of(&again, 0), PaidState::Mismatch);
    assert!(again.raised.is_empty(), "the second check raises nothing");
    assert_eq!(marks.get(&key(0)).unwrap().state, MarkState::Raised);
}

#[test]
fn oc_62_a_payment_arriving_later_clears_the_alarm_itself() {
    let sales = [sale(0, 5_000, PaidHow::AtOnce)];
    let marks = read_at(&sales, PAID_AT);
    let now = PAID_AT + PAID_CHECK_WAIT_SECS;
    let missing = check_paid(&sales, &marks, now, from_view(&view_with(&[])));
    let marks = marks.after(&missing, now);
    // The payment arrives in full, in a block.
    let paid = view_with(&[(0, 5_000, 2)]);
    let arrived = check_paid(&sales, &marks, now + 600, from_view(&paid));
    assert_eq!(state_of(&arrived, 0), PaidState::ClearedByArrival);
    assert_eq!(arrived.cleared, vec![key(0)]);
    let marks = marks.after(&arrived, now + 600);
    let mark = marks.get(&key(0)).unwrap();
    assert_eq!(mark.state, MarkState::ClearedByArrival);
    assert_eq!(mark.cleared_at, Some(now + 600));
    // It stays cleared on the next check, and is not cleared a second time.
    let next = check_paid(&sales, &marks, now + 1_200, from_view(&paid));
    assert_eq!(state_of(&next, 0), PaidState::ClearedByArrival);
    assert!(next.cleared.is_empty() && next.raised.is_empty());
    // A payment that goes missing again is raised again (see the tests below).
    let gone = check_paid(&sales, &marks, now + 1_800, from_view(&view_with(&[])));
    assert_eq!(state_of(&gone, 0), PaidState::Mismatch);
    assert_eq!(gone.raised, vec![key(0)]);
}

/// A sale raised, then cleared by its payment arriving: the marks and the time.
fn cleared_by_arrival(sales: &[PaidSale]) -> (PaidMarks, u64) {
    let marks = read_at(sales, PAID_AT);
    let now = PAID_AT + PAID_CHECK_WAIT_SECS;
    let missing = check_paid(sales, &marks, now, from_view(&view_with(&[])));
    let marks = marks.after(&missing, now);
    let paid = view_with(&[(0, 5_000, 2)]);
    let arrived = check_paid(sales, &marks, now + 600, from_view(&paid));
    (marks.after(&arrived, now + 600), now + 600)
}

#[test]
fn oc_62_an_alarm_cleared_by_arrival_is_raised_again_when_the_payment_leaves_the_view() {
    let sales = [sale(0, 5_000, PaidHow::AtOnce)];
    let (marks, at) = cleared_by_arrival(&sales);
    assert_eq!(
        marks.get(&key(0)).unwrap().state,
        MarkState::ClearedByArrival
    );
    let first_read = marks.get(&key(0)).unwrap().first_read;
    assert_eq!(first_read, PAID_AT);
    // Its block is replaced: the verified amount drops below what is expected.
    let later = at + 1_200;
    let gone = check_paid(&sales, &marks, later, from_view(&view_with(&[])));
    assert_eq!(state_of(&gone, 0), PaidState::Mismatch);
    assert_eq!(gone.raised, vec![key(0)]);
    assert!(gone.cleared.is_empty());
    let marks = marks.after(&gone, later);
    let mark = marks.get(&key(0)).unwrap();
    assert_eq!(mark.state, MarkState::Raised);
    assert_eq!(mark.raised_at, Some(later));
    assert_eq!(mark.cleared_at, None);
    assert_eq!(mark.first_read, first_read);
    // Raised once more, not on every check.
    let again = check_paid(&sales, &marks, later + 60, from_view(&view_with(&[])));
    assert_eq!(state_of(&again, 0), PaidState::Mismatch);
    assert!(again.raised.is_empty());
}

#[test]
fn oc_62_an_alarm_the_owner_cleared_stays_cleared_when_the_payment_leaves() {
    let sales = [sale(0, 5_000, PaidHow::AtOnce)];
    let marks = read_at(&sales, PAID_AT);
    let now = PAID_AT + PAID_CHECK_WAIT_SECS;
    let missing = check_paid(&sales, &marks, now, from_view(&view_with(&[])));
    let marks = marks.after(&missing, now);
    let marks = marks.cleared_by_owner(&key(0), now + 10).unwrap();
    let gone = check_paid(&sales, &marks, now + 600, from_view(&view_with(&[])));
    assert_eq!(state_of(&gone, 0), PaidState::Mismatch);
    assert!(gone.raised.is_empty());
    let marks = marks.after(&gone, now + 600);
    assert_eq!(marks.get(&key(0)).unwrap().state, MarkState::ClearedByOwner);
}

#[test]
fn oc_62_a_counter_at_the_alarm_cap_does_not_raise_a_payment_that_leaves() {
    // Through `check_paid` directly: `gate_open_alarms` holds a counter at the
    // cap back before the check, so this is the check's own guard.
    let cap = MAX_OPEN_ALARMS_PER_COUNTER as u32;
    let sales: Vec<PaidSale> = (0..=cap).map(|i| sale(i, 5_000, PaidHow::AtOnce)).collect();
    let (marks, at) = cleared_by_arrival(&sales[..1]);
    // The cap's worth of other sales are raised.
    let rest = &sales[1..];
    let marks = {
        let read = check_paid(rest, &marks, at, |_| Err(FactsRefused::ViewNotReady));
        marks.after(&read, at)
    };
    let later = at + PAID_CHECK_WAIT_SECS + 1;
    let raised = check_paid(rest, &marks, later, from_view(&view_with(&[])));
    let marks = marks.after(&raised, later);
    assert_eq!(marks.open_alarms()[&counter()], cap as usize);
    assert_eq!(
        marks.get(&key(0)).unwrap().state,
        MarkState::ClearedByArrival
    );
    // Sale 0's payment leaves: the counter is at the cap, so nothing is raised.
    let gone = check_paid(&sales, &marks, later + 60, from_view(&view_with(&[])));
    assert_eq!(state_of(&gone, 0), PaidState::Mismatch);
    assert!(gone.raised.is_empty());
    let marks = marks.after(&gone, later + 60);
    assert_eq!(
        marks.get(&key(0)).unwrap().state,
        MarkState::ClearedByArrival
    );
}

#[test]
fn oc_62_after_a_second_alarm_the_payment_arriving_again_clears_it_again() {
    let sales = [sale(0, 5_000, PaidHow::AtOnce)];
    let (marks, at) = cleared_by_arrival(&sales);
    let gone = check_paid(&sales, &marks, at + 100, from_view(&view_with(&[])));
    let marks = marks.after(&gone, at + 100);
    assert_eq!(marks.get(&key(0)).unwrap().state, MarkState::Raised);
    let paid = view_with(&[(0, 5_000, 2)]);
    let back = check_paid(&sales, &marks, at + 200, from_view(&paid));
    assert_eq!(state_of(&back, 0), PaidState::ClearedByArrival);
    assert_eq!(back.cleared, vec![key(0)]);
    let marks = marks.after(&back, at + 200);
    let mark = marks.get(&key(0)).unwrap();
    assert_eq!(mark.state, MarkState::ClearedByArrival);
    assert_eq!(mark.cleared_at, Some(at + 200));
}

#[test]
fn oc_60_a_partial_payment_within_the_tolerance_is_not_a_mismatch_and_one_outside_is() {
    let now = PAID_AT + PAID_CHECK_WAIT_SECS + 1;
    // 0.5 per cent of 10,000 is 50: 9,950 is within it, 9,949 is not.
    let view = view_with(&[(0, 9_950, DEPTH), (1, 9_949, DEPTH)]);
    let sales = [
        sale(0, 10_000, PaidHow::Confirmed),
        sale(1, 10_000, PaidHow::Confirmed),
    ];
    let marks = read_at(&sales, PAID_AT);
    let result = check_paid(&sales, &marks, now, from_view(&view));
    assert_eq!(state_of(&result, 0), PaidState::Checked);
    assert_eq!(state_of(&result, 1), PaidState::Mismatch);
    assert_eq!(result.raised, vec![key(1)]);
    // The sale's own tolerance counts, not a fixed one.
    let mut strict = sale(0, 10_000, PaidHow::Confirmed);
    strict.tolerance_bps = 0;
    let result = check_paid(&[strict], &marks, now, from_view(&view));
    assert_eq!(state_of(&result, 0), PaidState::Mismatch);
}

#[test]
fn oc_60_more_received_than_recorded_is_not_a_mismatch() {
    let now = PAID_AT + PAID_CHECK_WAIT_SECS + 1;
    let view = view_with(&[(0, 7_000, DEPTH)]);
    let sales = [sale(0, 5_000, PaidHow::Confirmed)];
    let result = check_paid(&sales, &read_at(&sales, PAID_AT), now, from_view(&view));
    assert_eq!(state_of(&result, 0), PaidState::Checked);
    assert!(result.raised.is_empty());
}

#[test]
fn oc_60_the_expected_amount_is_the_higher_of_the_recorded_and_the_quoted() {
    let mut quoted = sale(0, 5_000, PaidHow::AtOnce);
    quoted.quoted_sats = 8_000;
    assert_eq!(quoted.expected_sats(), 8_000);
    quoted.quoted_sats = 100;
    assert_eq!(quoted.expected_sats(), 5_000);
    // The wallet shows the recorded amount only: short of the quote.
    let mut sales = [sale(0, 5_000, PaidHow::AtOnce)];
    sales[0].quoted_sats = 8_000;
    let view = view_with(&[(0, 5_000, DEPTH)]);
    let result = check_paid(
        &sales,
        &read_at(&sales, PAID_AT),
        PAID_AT + PAID_CHECK_WAIT_SECS,
        from_view(&view),
    );
    assert_eq!(state_of(&result, 0), PaidState::Mismatch);
}

#[test]
fn oc_60_a_lightning_sale_is_not_checked() {
    let now = PAID_AT + 10 * PAID_CHECK_WAIT_SECS;
    let sales = [
        sale(0, 5_000, PaidHow::Lightning),
        sale(1, 5_000, PaidHow::AtOnce),
    ];
    let marks = read_at(&sales, PAID_AT);
    let result = check_paid(&sales, &marks, now, from_view(&view_with(&[])));
    assert_eq!(result.checks.len(), 1, "only the on-chain sale has a state");
    assert_eq!(result.checks[0].sale, key(1));
    assert_eq!(result.raised, vec![key(1)]);
    // Nor does it count in the day.
    let day = day_summary(&sales, &result.checks, PAID_AT - 10, PAID_AT + 10);
    assert_eq!((day[0].sales, day[0].total_sats), (1, 5_000));
}

#[test]
fn oc_61_a_view_that_is_not_ready_judges_nothing() {
    let now = PAID_AT + 10 * PAID_CHECK_WAIT_SECS;
    let owner = new_owner();
    let not_ready = CoinView::new(0);
    let sales = [sale(0, 5_000, PaidHow::AtOnce)];
    let result = check_paid(&sales, &PaidMarks::new(), now, |k| {
        verified_facts(&not_ready, &owner, k, TIP, DEPTH)
    });
    assert_eq!(state_of(&result, 0), PaidState::NotYet);
    assert!(result.raised.is_empty() && result.cleared.is_empty());
    assert_eq!(result.checks[0].seen_sats, None);
    // A view that has not reached the tip judges nothing either.
    let behind = view_with(&[]);
    let result = check_paid(&sales, &PaidMarks::new(), now, |k| {
        verified_facts(&behind, &owner, k, TIP + 1, DEPTH)
    });
    assert_eq!(state_of(&result, 0), PaidState::NotYet);
    // And a raised alarm is neither cleared nor changed by it.
    let raised = PaidMarks::new().after(
        &CheckResult {
            raised: vec![key(0)],
            ..CheckResult::default()
        },
        now,
    );
    let result = check_paid(&sales, &raised, now + 1, |_| {
        Err(FactsRefused::ViewNotReady)
    });
    assert_eq!(state_of(&result, 0), PaidState::NotYet);
    assert!(result.cleared.is_empty());
}

#[test]
fn oc_61_a_sale_too_old_for_facts_is_not_checkable() {
    let now = PAID_AT + 10 * PAID_CHECK_WAIT_SECS;
    let sales = [sale(0, 5_000, PaidHow::Confirmed)];
    let result = check_paid(&sales, &PaidMarks::new(), now, |_| {
        Err(FactsRefused::TooOldForFacts)
    });
    assert_eq!(state_of(&result, 0), PaidState::NotCheckable);
    assert!(result.raised.is_empty());
    assert_eq!(result.checks[0].seen_sats, None);
}

#[test]
fn oc_61_a_full_receipt_log_lists_sales_as_not_checkable_and_not_as_waiting() {
    let sales = [
        sale(0, 5_000, PaidHow::AtOnce),
        sale(1, 6_000, PaidHow::Confirmed),
    ];
    let marks = read_at(&sales, PAID_AT);
    // Well past the wait: with a log that cannot say what was paid, nothing is
    // a mismatch.
    let now = PAID_AT + 10 * PAID_CHECK_WAIT_SECS;
    let result = check_paid(&sales, &marks, now, |_| Err(FactsRefused::ReceiptLogFull));
    assert_eq!(state_of(&result, 0), PaidState::LogFull);
    assert_eq!(state_of(&result, 1), PaidState::LogFull);
    assert!(result.raised.is_empty() && result.cleared.is_empty());
    assert!(result.checks.iter().all(|c| c.seen_sats.is_none()));
    // The day counts them as not checked, never as waiting.
    let day = day_summary(&sales, &result.checks, PAID_AT - 10, PAID_AT + 10);
    assert_eq!(
        (
            day[0].sales,
            day[0].seen,
            day[0].waiting,
            day[0].too_old,
            day[0].missing
        ),
        (2, 0, 0, 2, 0)
    );
    // A view that is not ready is still a wait.
    let result = check_paid(&sales, &marks, now, |_| Err(FactsRefused::ViewNotReady));
    let day = day_summary(&sales, &result.checks, PAID_AT - 10, PAID_AT + 10);
    assert_eq!((day[0].waiting, day[0].too_old), (2, 0));
}

#[test]
fn oc_63_the_day_summary_counts_each_counters_sales_by_the_first_read() {
    let day_start = PAID_AT;
    let now = day_start + PAID_CHECK_WAIT_SECS + 100;
    let view = view_with(&[(0, 4_000, DEPTH), (1, 6_000, DEPTH), (2, 1_000, DEPTH)]);
    let sales = [
        sale(0, 4_000, PaidHow::Confirmed),
        sale(1, 6_000, PaidHow::Confirmed),
        sale(2, 3_000, PaidHow::AtOnce),
        sale(3, 2_000, PaidHow::AtOnce),
        sale(4, 800, PaidHow::Confirmed),
        sale(5, 9_000, PaidHow::Confirmed),
        sale(6, 700, PaidHow::Lightning),
    ];
    // Sales 0, 1, 2 and 4 were first read at the start of the day, 5 the day
    // before, and 3 only now.
    let mut marks = PaidMarks::new();
    for (i, at) in [
        (5, day_start - 86_400),
        (0, day_start),
        (1, day_start),
        (2, day_start),
        (4, day_start),
        (3, now),
    ] {
        let result = check_paid(&[sales[i].clone()], &marks, at, |_| {
            Err(FactsRefused::ViewNotReady)
        });
        marks = marks.after(&result, at);
    }
    let facts = |k: SaleKey| {
        if k.index == 4 {
            Err(FactsRefused::TooOldForFacts)
        } else {
            from_view(&view)(k)
        }
    };
    let result = check_paid(&sales, &marks, now, facts);
    let days = day_summary(&sales, &result.checks, day_start - 1, day_start + 1 + 100);
    assert_eq!(days.len(), 1);
    let d = &days[0];
    assert_eq!(d.counter, counter());
    assert_eq!(d.sales, 4, "0, 1, 2 and 4: not Lightning, not another day");
    assert_eq!(d.total_sats, 4_000 + 6_000 + 3_000 + 800);
    assert_eq!((d.seen, d.waiting, d.too_old, d.missing), (2, 0, 1, 1));
    assert_eq!(d.seen_sats, 4_000 + 6_000 + 1_000);
    assert!(
        d.differs,
        "the total is not what was seen: the counter is named"
    );
    // A window that holds the young sale counts it as waiting.
    let days = day_summary(&sales, &result.checks, now - 100, now + 1);
    assert_eq!((days[0].sales, days[0].waiting), (1, 1));
    // A counter whose total is what was seen is not named.
    let exact = [sale(0, 4_000, PaidHow::Confirmed)];
    let result = check_paid(&exact, &marks, now, from_view(&view));
    let days = day_summary(&exact, &result.checks, day_start - 1, day_start + 1);
    assert!(!days[0].differs);
    // No sale in the window, no line.
    assert!(day_summary(&exact, &result.checks, 0, 10).is_empty());
}

// ---- the records ---------------------------------------------------------------------

fn named(index: u32, at: u64, name: &str) -> Event {
    let Event::Opened {
        at: _,
        amount,
        tip,
        currency,
        needs_sats,
        minor_per_btc,
        source,
        ..
    } = opened(index, at, 100)
    else {
        unreachable!()
    };
    Event::Opened {
        index,
        at,
        amount,
        tip,
        currency,
        needs_sats,
        minor_per_btc,
        source,
        by: Some((Role::Cashier, name.to_string())),
    }
}

/// A sale opened at a quote of `needs` satoshis.
fn quoted(index: u32, at: u64, needs: u64) -> Event {
    match opened(index, at, 100) {
        Event::Opened {
            amount,
            tip,
            currency,
            minor_per_btc,
            source,
            by,
            ..
        } => Event::Opened {
            index,
            at,
            amount,
            tip,
            currency,
            needs_sats: needs,
            minor_per_btc,
            source,
            by,
        },
        _ => unreachable!(),
    }
}

fn requoted(index: u32, at: u64, needs: u64) -> Event {
    Event::Requoted {
        index,
        at,
        needs_sats: needs,
        minor_per_btc: 1,
        source: openbmt_counter::RateSource::new("test-fixed", at).unwrap(),
    }
}

fn confirmed(index: u32, at: u64, sats: u64) -> Event {
    Event::Confirmed {
        index,
        at,
        received_sats: sats,
        depth: 1,
    }
}

fn lightning(index: u32, at: u64, sats: u64) -> Event {
    Event::PaidByLightning {
        index,
        at,
        payment_hash: [index as u8; 32],
        amount_msat: sats * 1_000,
        settled_at: None,
        valued: Amount(0),
    }
}

fn at_once_for(index: u32, at: u64, sats: u64) -> Event {
    match at_once(index, at) {
        Event::PaidAtOnce { label, mode, .. } => Event::PaidAtOnce {
            index,
            at,
            at_risk_sats: sats,
            label,
            mode,
        },
        _ => unreachable!(),
    }
}

/// The paid sales of the test counter, as `(index, how, recorded, quoted)`.
fn folded(events: &[Event]) -> Vec<(u32, PaidHow, u64, u64)> {
    let (link, _a, _b) = link_with(events);
    let found = paid_sales(&link, &named_counters(&[counter()]));
    assert!(found.unreadable.is_empty());
    found
        .sales
        .iter()
        .map(|s| (s.index, s.how, s.recorded_sats, s.quoted_sats))
        .collect()
}

#[test]
fn oc_60_the_records_tell_an_on_chain_payment_from_lightning() {
    let (link, _a, _b) = link_with(&[
        named(0, PAID_AT - 50, "Ana"),
        at_once_for(0, PAID_AT, 5_000),
        opened(1, PAID_AT - 50, 100),
        lightning(1, PAID_AT + 1, 700),
        opened(2, PAID_AT - 50, 100),
        confirmed(2, PAID_AT + 2, 9_000),
        // Accepted at once, then confirmed: the amount is the confirmed one.
        opened(3, PAID_AT - 50, 100),
        at_once_for(3, PAID_AT + 3, 100),
        confirmed(3, PAID_AT + 400, 1_200),
        // Never paid.
        opened(4, PAID_AT - 50, 100),
    ]);
    let found = paid_sales(&link, &named_counters(&[counter()]));
    assert!(found.unreadable.is_empty());
    let sales = found.sales;
    let by: Vec<(u32, PaidHow, u64)> = sales
        .iter()
        .map(|s| (s.index, s.how, s.recorded_sats))
        .collect();
    assert_eq!(
        by,
        vec![
            (0, PaidHow::AtOnce, 5_000),
            (1, PaidHow::Lightning, 700),
            (2, PaidHow::Confirmed, 9_000),
            (3, PaidHow::Confirmed, 1_200),
        ]
    );
    assert_eq!(sales[0].cashier, "Ana");
    assert_eq!(sales[1].cashier, "");
    assert!(sales
        .iter()
        .all(|s| s.tolerance_bps == 50 && s.counter == counter()));
    assert!(!sales[1].on_chain() && sales[0].on_chain());
}

#[test]
fn oc_60_a_reopened_or_replaced_sale_stays_checked_and_a_manager_acceptance_is_not_a_payment() {
    let sales = folded(&[
        opened(0, PAID_AT - 50, 100),
        at_once_for(0, PAID_AT, 5_000),
        Event::Replaced {
            index: 0,
            at: PAID_AT + 1,
        },
        opened(1, PAID_AT - 50, 100),
        confirmed(1, PAID_AT, 5_000),
        Event::Reopened {
            index: 1,
            at: PAID_AT + 1,
        },
        // Paid again after the reopening: the first Paid still stands.
        at_once_for(1, PAID_AT + 100, 4_000),
        // A manager's acceptance is not a payment to the address.
        opened(2, PAID_AT - 50, 100),
        Event::Override {
            index: 2,
            at: PAID_AT,
            role: Role::Manager,
            name: "Bo".to_string(),
            sats: 5_000,
        },
        // A payment confirmed, then a replacement: still paid.
        opened(3, PAID_AT - 50, 100),
        confirmed(3, PAID_AT, 4_000),
        Event::Replaced {
            index: 3,
            at: PAID_AT + 1,
        },
        // Reopened and never paid again: still checked for what was shown.
        opened(4, PAID_AT - 50, 100),
        at_once_for(4, PAID_AT, 2_000),
        Event::Reopened {
            index: 4,
            at: PAID_AT + 1,
        },
    ]);
    assert_eq!(
        sales,
        vec![
            (0, PaidHow::AtOnce, 5_000, 1),
            (1, PaidHow::Confirmed, 5_000, 1),
            (3, PaidHow::Confirmed, 4_000, 1),
            (4, PaidHow::AtOnce, 2_000, 1),
        ]
    );
}

#[test]
fn oc_60_a_reopened_after_a_paid_keeps_the_sale_checked() {
    let (link, _a, _b) = link_with(&[
        opened(0, PAID_AT - 50, 100),
        at_once_for(0, PAID_AT, 5_000),
        Event::Reopened {
            index: 0,
            at: PAID_AT + 1,
        },
    ]);
    let sales = paid_sales(&link, &LightningCounters::empty()).sales;
    assert_eq!(sales.len(), 1, "the reopening does not remove the sale");
    let dir = Dir::new();
    let mut checks = PaidChecks::open(dir.path());
    let view = view_with(&[]);
    checks.check(&sales, PAID_AT, from_view(&view));
    let result = checks.check(&sales, PAID_AT + PAID_CHECK_WAIT_SECS, from_view(&view));
    assert_eq!(state_of(&result, 0), PaidState::Mismatch);
    assert_eq!(result.raised, vec![key(0)]);
}

#[test]
fn oc_60_a_later_confirmed_with_a_lower_amount_does_not_clear_the_alarm() {
    let (link, _a, _b) = link_with(&[
        opened(0, PAID_AT - 50, 100),
        at_once_for(0, PAID_AT, 5_000),
        // The counter, taken over, writes a smaller amount afterwards.
        confirmed(0, PAID_AT + 10, 100),
        Event::Final {
            index: 0,
            at: PAID_AT + 20,
            received_sats: 50,
            depth: 6,
            first_seen_at: PAID_AT,
            valued: Amount(0),
            payments: Vec::new(),
            more: false,
        },
    ]);
    let sales = paid_sales(&link, &LightningCounters::empty()).sales;
    assert_eq!(
        sales[0].recorded_sats, 5_000,
        "the most ever recorded stands"
    );
    assert_eq!(sales[0].expected_sats(), 5_000);
    let dir = Dir::new();
    let mut checks = PaidChecks::open(dir.path());
    // The wallet shows 100: that is not what the counter showed first.
    let view = view_with(&[(0, 100, DEPTH)]);
    checks.check(&sales, PAID_AT, from_view(&view));
    let result = checks.check(&sales, PAID_AT + PAID_CHECK_WAIT_SECS, from_view(&view));
    assert_eq!(state_of(&result, 0), PaidState::Mismatch);
    assert_eq!(result.raised, vec![key(0)]);
}

#[test]
fn oc_60_a_sale_paid_on_chain_stays_on_chain_when_lightning_follows_and_the_reverse_is_not_checked()
{
    let sales = folded(&[
        opened(0, PAID_AT - 50, 100),
        at_once_for(0, PAID_AT, 5_000),
        lightning(0, PAID_AT + 1, 9_999),
        opened(1, PAID_AT - 50, 100),
        lightning(1, PAID_AT, 700),
        at_once_for(1, PAID_AT + 1, 5_000),
        confirmed(1, PAID_AT + 2, 5_000),
    ]);
    assert_eq!(
        sales,
        vec![
            (0, PaidHow::AtOnce, 5_000, 1),
            (1, PaidHow::Lightning, 700, 1),
        ]
    );
}

#[test]
fn oc_60_the_quoted_amount_is_read_from_the_opening_and_the_latest_requote() {
    let sales = folded(&[
        quoted(0, PAID_AT - 50, 8_000),
        at_once_for(0, PAID_AT, 5_000),
        quoted(1, PAID_AT - 50, 8_000),
        requoted(1, PAID_AT - 40, 9_000),
        at_once_for(1, PAID_AT, 5_000),
        quoted(2, PAID_AT - 50, 8_000),
        requoted(2, PAID_AT - 40, 9_000),
        requoted(2, PAID_AT - 30, 6_000),
        at_once_for(2, PAID_AT, 5_000),
    ]);
    assert_eq!(
        sales,
        vec![
            (0, PaidHow::AtOnce, 5_000, 8_000),
            (1, PaidHow::AtOnce, 5_000, 9_000),
            (2, PaidHow::AtOnce, 5_000, 6_000),
        ]
    );
    // The check expects the higher of the two, and a later lower record
    // never lowers it.
    let (link, _a, _b) = link_with(&[
        quoted(0, PAID_AT - 50, 8_000),
        at_once_for(0, PAID_AT, 5_000),
        confirmed(0, PAID_AT + 5, 100),
    ]);
    let found = paid_sales(&link, &LightningCounters::empty()).sales;
    assert_eq!(found[0].expected_sats(), 8_000);
}

#[test]
fn oc_61_the_counters_time_in_the_future_does_not_delay_the_alarm() {
    let far = PAID_AT + 20 * 365 * 86_400;
    let (link, _a, _b) = link_with(&[opened(0, PAID_AT - 50, 100), at_once_for(0, far, 5_000)]);
    let sales = paid_sales(&link, &LightningCounters::empty()).sales;
    let dir = Dir::new();
    let mut checks = PaidChecks::open(dir.path());
    let view = view_with(&[]);
    let first = PAID_AT + 100;
    let result = checks.check(&sales, first, from_view(&view));
    assert_eq!(state_of(&result, 0), PaidState::Waiting);
    // A day on the owner's clock: the alarm, whatever the counter wrote.
    let result = checks.check(&sales, first + PAID_CHECK_WAIT_SECS, from_view(&view));
    assert_eq!(state_of(&result, 0), PaidState::Mismatch);
    assert_eq!(result.raised, vec![key(0)]);
    // And the counter's time does not place the sale in a day either.
    let days = day_summary(&sales, &result.checks, first - 10, first + 10);
    assert_eq!(days.len(), 1, "the day of the first read");
}

#[test]
fn oc_60_a_raised_tolerance_in_settings_does_not_hide_a_mismatch() {
    // The tolerance is the policy default, however the settings are set.
    let mut settings = openbmt_core::settings::Settings::defaults(0);
    assert_eq!(default_tolerance_bps(), settings.underpayment_tolerance_bps);
    assert_eq!(default_tolerance_bps(), 50);
    settings.underpayment_tolerance_bps = 10_000;
    assert_ne!(default_tolerance_bps(), settings.underpayment_tolerance_bps);
    let (link, _a, _b) = link_with(&[opened(0, PAID_AT - 50, 100), confirmed(0, PAID_AT, 10_000)]);
    let sales = paid_sales(&link, &LightningCounters::empty()).sales;
    assert_eq!(sales[0].tolerance_bps, 50);
    // Half of it is a mismatch, though settings that allowed all would pass it.
    let view = view_with(&[(0, 5_000, DEPTH)]);
    let result = check_paid(
        &sales,
        &read_at(&sales, PAID_AT),
        PAID_AT + PAID_CHECK_WAIT_SECS,
        from_view(&view),
    );
    assert_eq!(state_of(&result, 0), PaidState::Mismatch);
}

#[test]
fn oc_61_one_counter_with_a_bad_record_alarms_while_another_is_still_checked() {
    let other = id(60).device_id();
    let bad = counter();
    let mut other_sale = sale(3, 5_000, PaidHow::AtOnce);
    other_sale.counter = other;
    let found = gather(vec![
        (bad, Err(FactsError::UnknownSale { index: 9 })),
        (
            other,
            Ok(Fold {
                sales: vec![other_sale.clone()],
                passed_over: false,
            }),
        ),
    ]);
    assert_eq!(found.unreadable, vec![bad]);
    assert_eq!(found.sales, vec![other_sale.clone()]);
    // The other counter is still checked, and its sale raised.
    let dir = Dir::new();
    let mut checks = PaidChecks::open(dir.path());
    let view = view_with(&[]);
    let result = checks.check(&found.sales, PAID_AT, from_view(&view));
    assert_eq!(result.checks.len(), 1);
    // (The test view issued no address of the other counter's: at once.)
    assert_eq!(result.raised.len(), 1);
    // Every kind of unreadable copy is one counter's, not the whole.
    for e in [
        FactsError::OpenedTwice { index: 1 },
        FactsError::TooManySales,
    ] {
        let found = gather(vec![(bad, Err(e)), (other, Ok(Fold::default()))]);
        assert_eq!(found.unreadable, vec![bad]);
    }
    // A counter that passed over a record is listed, with the sales it read.
    let passed = Fold {
        sales: vec![sale(0, 1, PaidHow::AtOnce)],
        passed_over: true,
    };
    let found = gather(vec![(bad, Ok(passed)), (other, Ok(Fold::default()))]);
    assert_eq!(found.unreadable, vec![bad]);
    assert_eq!(found.sales.len(), 1);
    // From the records: a copy that does not fold is listed, with no sale.
    let (link, _a, _b) = link_with(&[opened(0, PAID_AT - 50, 100), at_once_for(9, PAID_AT, 1)]);
    let found = paid_sales(&link, &LightningCounters::empty());
    assert_eq!(found.unreadable, vec![counter()]);
    assert!(found.sales.is_empty());
}

#[test]
fn oc_62_the_refusals_read_as_their_texts() {
    assert_eq!(
        PaidCheckRefused::NoSuchAlarm.to_string(),
        "OC-62: no alarm of this sale is raised"
    );
    assert_eq!(
        PaidCheckRefused::Store("disk full".to_string()).to_string(),
        "OC-64: the marks could not be stored: disk full"
    );
}

#[test]
fn oc_61_a_record_of_no_event_kind_by_the_counter_is_not_passed_over() {
    let fold = fold_of(&[
        event_record(&opened(0, PAID_AT - 50, 100)),
        // A settings record holds no sale events: it is skipped, not read.
        (RecordKind::Setting, vec![0xff, 0xff, 0xff]),
        event_record(&at_once_for(0, PAID_AT, 5_000)),
    ]);
    assert!(!fold.passed_over);
    assert_eq!(fold.sales.len(), 1);
    assert_eq!(fold.sales[0].recorded_sats, 5_000);
}

#[test]
fn oc_61_an_event_record_by_another_device_is_not_read_as_the_counters() {
    let other = id(60);
    let mut chain = openbmt_core::records::Chain::new();
    let (kind, body) = event_record(&opened(0, PAID_AT - 50, 100));
    chain.append(&id(3), kind, body).unwrap();
    // A well-formed sale record from another device on the list.
    let (kind, body) = event_record(&opened(5, PAID_AT - 40, 100));
    chain.append(&other, kind, body).unwrap();
    let (kind, body) = event_record(&at_once_for(5, PAID_AT, 9_000));
    chain.append(&other, kind, body).unwrap();
    let (kind, body) = event_record(&at_once_for(0, PAID_AT, 5_000));
    chain.append(&id(3), kind, body).unwrap();
    let devices = openbmt_core::records::DeviceList::new([counter(), other.device_id()]);
    let fold = paid_sales_from_chain(&chain, &devices, &counter()).unwrap();
    assert!(!fold.passed_over);
    assert_eq!(fold.sales.len(), 1);
    assert_eq!(fold.sales[0].index, 0);
}

#[test]
fn oc_61_a_counter_copy_of_two_thousand_records_folds_as_the_chain_does() {
    let mut events = Vec::new();
    for index in 0..1_000 {
        events.push(opened(index, PAID_AT - 50, 100));
        events.push(at_once_for(index, PAID_AT, 900 + u64::from(index)));
    }
    let (link, _a, _b) = link_with(&events);
    let copy = link.copy(&counter()).unwrap();
    assert_eq!(copy.records().len(), 2_000);
    let found = paid_sales(&link, &LightningCounters::empty());
    let chain = openbmt_core::records::Chain::from_records(copy.records().to_vec());
    let fold = paid_sales_from_chain(&chain, link.devices(), &counter()).unwrap();
    assert!(!fold.passed_over);
    assert_eq!(fold.sales.len(), 1_000);
    assert_eq!(
        found,
        PaidSales {
            sales: fold.sales,
            unreadable: Vec::new(),
        }
    );
}

#[test]
fn oc_30_a_chain_that_does_not_verify_gives_no_paid_sales() {
    let chain = openbmt_core::records::Chain::new();
    let devices = openbmt_core::records::DeviceList::new([]);
    assert_eq!(
        paid_sales_from_chain(&chain, &devices, &counter()).unwrap(),
        Fold::default()
    );
}

/// The fold of the test counter's records, events and raw records alike.
fn fold_of(records: &[(RecordKind, Vec<u8>)]) -> Fold {
    let mut chain = openbmt_core::records::Chain::new();
    for (kind, body) in records {
        chain.append(&id(3), *kind, body.clone()).unwrap();
    }
    let devices = openbmt_core::records::DeviceList::new([counter()]);
    paid_sales_from_chain(&chain, &devices, &counter()).unwrap()
}

fn event_record(e: &Event) -> (RecordKind, Vec<u8>) {
    (e.kind(), e.encode())
}

#[test]
fn oc_61_a_paid_for_a_sale_never_opened_is_passed_over_and_the_next_sale_is_judged() {
    let (link, _a, _b) = link_with(&[
        // A Paid for an index that was never opened, then a real sale.
        at_once_for(9, PAID_AT, 1),
        opened(0, PAID_AT - 50, 100),
        at_once_for(0, PAID_AT, 5_000),
    ]);
    let found = paid_sales(&link, &named_counters(&[counter()]));
    assert_eq!(found.unreadable, vec![counter()]);
    assert_eq!(found.sales.len(), 1);
    // The sale has no payment: waiting, then its alarm after the wait.
    let view = view_with(&[]);
    let dir = Dir::new();
    let mut checks = PaidChecks::open(dir.path());
    let first = checks.check(&found.sales, PAID_AT, from_view(&view));
    assert_eq!(state_of(&first, 0), PaidState::Waiting);
    let late = checks.check(
        &found.sales,
        PAID_AT + PAID_CHECK_WAIT_SECS,
        from_view(&view),
    );
    assert_eq!(state_of(&late, 0), PaidState::Mismatch);
    assert_eq!(late.raised, vec![key(0)]);
}

#[test]
fn oc_61_a_second_opening_is_passed_over_and_the_first_stands() {
    let quoted = |at: u64, cents: u64, sats: u64| match opened(0, at, cents) {
        Event::Opened {
            index,
            at,
            amount,
            tip,
            currency,
            minor_per_btc,
            source,
            by,
            ..
        } => Event::Opened {
            index,
            at,
            amount,
            tip,
            currency,
            needs_sats: sats,
            minor_per_btc,
            source,
            by,
        },
        _ => unreachable!(),
    };
    let first = quoted(PAID_AT - 50, 100, 7_000);
    let second = quoted(PAID_AT - 40, 5, 1);
    let fold = fold_of(&[
        event_record(&first),
        event_record(&second),
        event_record(&at_once_for(0, PAID_AT, 6_000)),
    ]);
    assert!(fold.passed_over);
    assert_eq!(fold.sales.len(), 1);
    assert_eq!(fold.sales[0].quoted_sats, 7_000);
    assert_eq!(fold.sales[0].recorded_sats, 6_000);
    assert_eq!(fold.sales[0].expected_sats(), 7_000);
}

#[test]
fn oc_61_a_record_that_does_not_read_is_passed_over() {
    let fold = fold_of(&[
        event_record(&opened(0, PAID_AT - 50, 100)),
        (RecordKind::Sale, vec![0xff, 0xff, 0xff]),
        event_record(&at_once_for(0, PAID_AT, 5_000)),
    ]);
    assert!(fold.passed_over);
    assert_eq!(fold.sales.len(), 1);
    assert_eq!(fold.sales[0].recorded_sats, 5_000);
    // A clean copy passes over nothing.
    let clean = fold_of(&[
        event_record(&opened(0, PAID_AT - 50, 100)),
        event_record(&at_once_for(0, PAID_AT, 5_000)),
    ]);
    assert!(!clean.passed_over);
}

#[test]
fn oc_61_every_sale_of_a_busy_counter_is_judged() {
    let n = 20_000u32;
    let mut records = Vec::new();
    for index in 0..n {
        records.push(event_record(&opened(index, PAID_AT - 50, 100)));
    }
    records.push(event_record(&at_once_for(n - 1, PAID_AT, 900)));
    let fold = fold_of(&records);
    assert!(!fold.passed_over);
    assert_eq!(fold.sales.len(), 1);
    assert_eq!(fold.sales[0].index, n - 1);
    let none = |_: SaleKey| {
        Ok(SaleFacts {
            received_sats: 0,
            reached_refund_depth: false,
        })
    };
    let marks = read_at(&fold.sales, PAID_AT);
    let later = PAID_AT + PAID_CHECK_WAIT_SECS;
    let result = check_paid(&fold.sales, &marks, later, none);
    assert_eq!(result.raised, vec![fold.sales[0].key()]);
}

#[test]
fn oc_61_a_counter_that_passed_over_a_record_does_not_blind_another() {
    let bad = counter();
    let other = id(60).device_id();
    let mut other_sale = sale(3, 5_000, PaidHow::AtOnce);
    other_sale.counter = other;
    let found = gather(vec![
        (
            bad,
            Ok(Fold {
                sales: vec![sale(0, 5_000, PaidHow::AtOnce)],
                passed_over: true,
            }),
        ),
        (
            other,
            Ok(Fold {
                sales: vec![other_sale],
                passed_over: false,
            }),
        ),
    ]);
    assert_eq!(found.unreadable, vec![bad]);
    assert_eq!(found.sales.len(), 2);
}

// ---- the marks -----------------------------------------------------------------------

fn raised_marks(from: u32, to: u32, now: u64) -> PaidMarks {
    PaidMarks::new().after(
        &CheckResult {
            raised: (from..to).map(key).collect(),
            ..CheckResult::default()
        },
        now,
    )
}

#[test]
fn oc_64_the_marks_read_back_strictly() {
    let marks = raised_marks(0, 3, 1_000);
    let marks = marks.cleared_by_owner(&key(1), 2_000).unwrap();
    // A sale only read has its own state.
    let marks = marks.after(
        &CheckResult {
            read: vec![key(5)],
            ..CheckResult::default()
        },
        3_000,
    );
    let bytes = marks.to_bytes();
    assert_eq!(&bytes[..4], &MAGIC_PAID_CHECK);
    assert_eq!(&MAGIC_PAID_CHECK, b"OBM\x17");
    assert_eq!(bytes[4], 2, "version 2");
    assert_eq!(PaidMarks::from_bytes(&bytes).unwrap(), marks);
    // An empty set is a frame and a count.
    assert_eq!(
        PaidMarks::from_bytes(&PaidMarks::new().to_bytes())
            .unwrap()
            .len(),
        0
    );

    // A byte left over, or short.
    let mut more = bytes.clone();
    more.push(0);
    assert!(PaidMarks::from_bytes(&more).is_err());
    assert!(PaidMarks::from_bytes(&bytes[..bytes.len() - 1]).is_err());
    // The magic and the version.
    let mut bad = bytes.clone();
    bad[0] ^= 1;
    assert!(matches!(
        PaidMarks::from_bytes(&bad),
        Err(DecodeError::BadMagic)
    ));
    let mut bad = bytes.clone();
    bad[4] = 3;
    assert!(matches!(
        PaidMarks::from_bytes(&bad),
        Err(DecodeError::UnsupportedVersion(3))
    ));
    // A count that is more than the bytes hold, or more than the cap.
    let mut bad = bytes.clone();
    bad[5..9].copy_from_slice(&5u32.to_le_bytes());
    assert!(PaidMarks::from_bytes(&bad).is_err());
    let mut bad = bytes.clone();
    bad[5..9].copy_from_slice(&(MAX_PAID_MARKS as u32 + 1).to_le_bytes());
    assert!(PaidMarks::from_bytes(&bad).is_err());
    // A state that is none of the four.
    let state_at = 9 + 32 + 4;
    let mut bad = bytes.clone();
    bad[state_at] = 9;
    assert!(matches!(
        PaidMarks::from_bytes(&bad),
        Err(DecodeError::UnknownKind(9))
    ));
    // A raised mark with a time cleared, and a cleared one with none.
    let mut bad = bytes.clone();
    bad[state_at + 17..state_at + 25].copy_from_slice(&5u64.to_le_bytes());
    assert!(PaidMarks::from_bytes(&bad).is_err());
    let mut bad = bytes.clone();
    bad[state_at] = 2;
    assert!(PaidMarks::from_bytes(&bad).is_err());
    // A mark with no time of first read.
    let mut bad = bytes.clone();
    bad[state_at + 1..state_at + 9].copy_from_slice(&0u64.to_le_bytes());
    assert!(PaidMarks::from_bytes(&bad).is_err());
    // Marks out of order, and the same mark twice.
    let entry = |i: usize| bytes[9 + i * MARK..9 + (i + 1) * MARK].to_vec();
    let mut swapped = bytes[..9].to_vec();
    for i in [1, 0, 2, 3] {
        swapped.extend(entry(i));
    }
    assert!(PaidMarks::from_bytes(&swapped).is_err());
    let mut twice = bytes[..9].to_vec();
    for i in [0, 0, 2, 3] {
        twice.extend(entry(i));
    }
    assert!(PaidMarks::from_bytes(&twice).is_err());
    // A file larger than the cap's whole frame.
    assert!(matches!(
        PaidMarks::from_bytes(&vec![0; 5 + 4 + MAX_PAID_MARKS * MARK + 1]),
        Err(DecodeError::TooLarge)
    ));
}

#[test]
fn oc_64_a_version_1_file_still_reads_with_the_time_raised_as_the_first_read() {
    // One raised mark, as version 1 wrote it: no time of first read.
    let mut old = openbmt_core::wire::start(&MAGIC_PAID_CHECK, 0);
    old.extend_from_slice(&1u32.to_le_bytes());
    old.extend_from_slice(&counter().to_bytes());
    old.extend_from_slice(&7u32.to_le_bytes());
    old.push(1);
    old.extend_from_slice(&1_234u64.to_le_bytes());
    old.extend_from_slice(&0u64.to_le_bytes());
    let marks = PaidMarks::from_bytes(&old).unwrap();
    let m = marks.get(&key(7)).unwrap();
    assert_eq!(
        (m.state, m.first_read, m.raised_at, m.cleared_at),
        (MarkState::Raised, 1_234, Some(1_234), None)
    );
    // Written again, it is version 2.
    assert_eq!(marks.to_bytes()[4], 2);
    // A version 1 file has no state 4.
    let mut bad = old.clone();
    bad[9 + 32 + 4] = 4;
    assert!(PaidMarks::from_bytes(&bad).is_err());
}

/// A version 1 file: the count, then per mark the counter, the index, the
/// state, the time raised and the time cleared, with no time of first read.
fn v1_file(count: u32, marks: &[(u32, u8, u64, u64)]) -> Vec<u8> {
    let mut out = openbmt_core::wire::start(&MAGIC_PAID_CHECK, 0);
    out.extend_from_slice(&count.to_le_bytes());
    for (index, state, raised, cleared) in marks {
        out.extend_from_slice(&counter().to_bytes());
        out.extend_from_slice(&index.to_le_bytes());
        out.push(*state);
        out.extend_from_slice(&raised.to_le_bytes());
        out.extend_from_slice(&cleared.to_le_bytes());
    }
    out
}

#[test]
fn oc_64_a_version_1_file_of_two_marks_reads_and_one_short_of_its_count_is_refused() {
    let file = v1_file(2, &[(3, 1, 1_000, 0), (7, 3, 2_000, 2_500)]);
    assert_eq!(file.len(), 9 + 2 * 53);
    let marks = PaidMarks::from_bytes(&file).unwrap();
    assert_eq!(marks.len(), 2);
    let a = marks.get(&key(3)).unwrap();
    assert_eq!(
        (a.state, a.first_read, a.raised_at, a.cleared_at),
        (MarkState::Raised, 1_000, Some(1_000), None)
    );
    let b = marks.get(&key(7)).unwrap();
    assert_eq!(
        (b.state, b.first_read, b.raised_at, b.cleared_at),
        (MarkState::ClearedByArrival, 2_000, Some(2_000), Some(2_500))
    );
    // The count says three and the file holds two.
    let short = v1_file(3, &[(3, 1, 1_000, 0), (7, 3, 2_000, 2_500)]);
    assert!(PaidMarks::from_bytes(&short).is_err());
    // One byte cut from the last mark is refused too.
    assert!(PaidMarks::from_bytes(&file[..file.len() - 1]).is_err());
}

#[test]
fn oc_64_the_oldest_cleared_marks_go_first_and_a_raised_one_is_never_dropped_from_memory() {
    let full = raised_marks(0, MAX_PAID_MARKS as u32, 100);
    assert_eq!(full.len(), MAX_PAID_MARKS);
    assert!(!full.is_over());
    // Every mark is a raised alarm: another is held in memory, but over what a file keeps.
    let one_more = CheckResult {
        raised: vec![key(MAX_PAID_MARKS as u32)],
        ..CheckResult::default()
    };
    let over = full.after(&one_more, 200);
    assert_eq!(over.len(), MAX_PAID_MARKS + 1);
    assert!(over.is_over());
    // Nothing but alarms: none is dropped, so the fitted set is still over.
    assert_eq!(over.fitted().len(), MAX_PAID_MARKS + 1);
    assert!(over.fitted().is_over());
    // Clear three, the later one first: the two oldest go, then the third.
    let marks = full
        .cleared_by_owner(&key(10), 3_000)
        .unwrap()
        .cleared_by_owner(&key(20), 1_000)
        .unwrap()
        .cleared_by_owner(&key(30), 2_000)
        .unwrap();
    let two_more = CheckResult {
        raised: vec![key(70_000), key(70_001)],
        ..CheckResult::default()
    };
    let next = marks.after(&two_more, 5_000);
    assert_eq!(next.len(), MAX_PAID_MARKS);
    assert!(next.get(&key(20)).is_none() && next.get(&key(30)).is_none());
    assert!(next.get(&key(10)).is_some(), "the newest cleared stays");
    assert!(next.get(&key(70_000)).is_some() && next.get(&key(70_001)).is_some());
    // Every alarm not cleared is still there.
    assert_eq!(
        next.iter().filter(|m| m.state == MarkState::Raised).count(),
        MAX_PAID_MARKS - 3 + 2
    );
    // The whole file reads back.
    assert_eq!(PaidMarks::from_bytes(&next.to_bytes()).unwrap(), next);
}

#[test]
fn oc_62_only_a_raised_alarm_is_cleared_and_by_the_owner() {
    let dir = Dir::new();
    let mut checks = PaidChecks::open(dir.path());
    let sales = [sale(0, 5_000, PaidHow::AtOnce)];
    let now = PAID_AT + PAID_CHECK_WAIT_SECS;
    let view = view_with(&[]);
    checks.check(&sales, PAID_AT, from_view(&view));
    let result = checks.check(&sales, now, from_view(&view));
    assert_eq!(result.raised, vec![key(0)]);
    assert_eq!(
        checks.clear(&key(9), now),
        Err(PaidCheckRefused::NoSuchAlarm),
        "no alarm of that sale"
    );
    checks.clear(&key(0), now + 5).unwrap();
    let mark = *checks.marks().get(&key(0)).unwrap();
    assert_eq!(mark.state, MarkState::ClearedByOwner);
    assert_eq!(
        (mark.raised_at, mark.cleared_at),
        (Some(now), Some(now + 5))
    );
    assert_eq!(
        checks.clear(&key(0), now + 6),
        Err(PaidCheckRefused::NoSuchAlarm)
    );
    // A cleared alarm is not raised again, and the sale is still missing.
    let result = checks.check(&sales, now + 60, from_view(&view));
    assert_eq!(state_of(&result, 0), PaidState::Mismatch);
    assert!(result.raised.is_empty());
    // The marks are in the file, and a new start reads them back.
    let again = PaidChecks::open(dir.path());
    assert_eq!(again.marks(), checks.marks());
    assert!(again.trouble().is_none());
}

#[test]
fn oc_64_a_sale_seen_in_full_loses_its_reading() {
    let dir = Dir::new();
    let mut checks = PaidChecks::open(dir.path());
    let sales = [sale(0, 5_000, PaidHow::AtOnce)];
    let result = checks.check(&sales, PAID_AT, from_view(&view_with(&[])));
    assert_eq!(state_of(&result, 0), PaidState::Waiting);
    assert_eq!(checks.marks().len(), 1);
    let paid = view_with(&[(0, 5_000, DEPTH)]);
    let result = checks.check(&sales, PAID_AT + 60, from_view(&paid));
    assert_eq!(state_of(&result, 0), PaidState::Checked);
    assert!(
        checks.marks().is_empty(),
        "nothing is kept for a sale that is settled"
    );
}

#[test]
fn oc_64_a_file_that_does_not_read_back_does_not_stop_the_node_or_the_check() {
    let dir = Dir::new();
    std::fs::write(dir.path().join(PAID_CHECK_FILE), b"OBM\x17\x01garbage").unwrap();
    let mut checks = PaidChecks::open(dir.path());
    assert!(matches!(checks.trouble(), Some(PaidTrouble::NotKept(_))));
    assert!(checks.marks().is_empty());
    let sales = [sale(0, 5_000, PaidHow::AtOnce)];
    let view = view_with(&[]);
    // The check runs on marks held in memory, and shows its mismatch.
    checks.check(&sales, PAID_AT, from_view(&view));
    let result = checks.check(&sales, PAID_AT + PAID_CHECK_WAIT_SECS, from_view(&view));
    assert_eq!(state_of(&result, 0), PaidState::Mismatch);
    assert_eq!(result.raised, vec![key(0)]);
    // The owner can clear it, in memory.
    checks
        .clear(&key(0), PAID_AT + PAID_CHECK_WAIT_SECS + 1)
        .unwrap();
    // Nothing was written over the file.
    assert_eq!(
        std::fs::read(dir.path().join(PAID_CHECK_FILE)).unwrap(),
        b"OBM\x17\x01garbage"
    );
    // The owner clears the checks, and they are kept again.
    checks.reset().unwrap();
    assert!(checks.trouble().is_none());
    assert!(!dir.path().join(PAID_CHECK_FILE).exists());
    checks.check(&sales, PAID_AT, from_view(&view));
    assert!(dir.path().join(PAID_CHECK_FILE).exists());
    // A file too large for its bound is not read either.
    let big = Dir::new();
    std::fs::write(
        big.path().join(PAID_CHECK_FILE),
        vec![0u8; 5 + 4 + MAX_PAID_MARKS * MARK + 1],
    )
    .unwrap();
    assert!(PaidChecks::open(big.path()).trouble().is_some());
}

#[test]
fn oc_64_marks_full_still_show_the_mismatch() {
    let dir = Dir::new();
    // A file of the most marks, every one an alarm raised for another sale.
    // (Under the cap of OC-61 for one counter this takes many counters; the
    // file is read as it is, whatever the bounds that make it.)
    let full = PaidMarks::new().after(
        &CheckResult {
            raised: (0..256u32)
                .flat_map(|c| {
                    let mut secret = [7u8; 32];
                    secret[0] = c as u8;
                    let device = Identity::from_secret_bytes(&secret).unwrap().device_id();
                    (0..MAX_OPEN_ALARMS_PER_COUNTER as u32).map(move |i| key_of(device, i))
                })
                .collect(),
            ..CheckResult::default()
        },
        100,
    );
    assert_eq!(full.len(), MAX_PAID_MARKS);
    std::fs::write(dir.path().join(PAID_CHECK_FILE), full.to_bytes()).unwrap();
    let mut checks = PaidChecks::open(dir.path());
    assert!(checks.trouble().is_none(), "full is not over");
    let sales = [sale(0, 5_000, PaidHow::AtOnce)];
    let view = view_with(&[]);
    checks.check(&sales, PAID_AT, from_view(&view));
    let result = checks.check(&sales, PAID_AT + PAID_CHECK_WAIT_SECS, from_view(&view));
    assert_eq!(state_of(&result, 0), PaidState::Mismatch);
    assert_eq!(
        result.raised,
        vec![key(0)],
        "the mismatch is shown and raised"
    );
    assert_eq!(checks.trouble(), Some(PaidTrouble::Full));
    // The file still holds what fits, and reads back.
    let bytes = std::fs::read(dir.path().join(PAID_CHECK_FILE)).unwrap();
    assert_eq!(PaidMarks::from_bytes(&bytes).unwrap().len(), MAX_PAID_MARKS);
}

#[test]
fn oc_64_marks_that_cannot_be_stored_are_held_and_the_write_is_tried_again() {
    let dir = Dir::new();
    let folder = dir.path().join("later");
    let mut checks = PaidChecks::open(&folder);
    let sales = [sale(0, 5_000, PaidHow::AtOnce)];
    let view = view_with(&[]);
    checks.check(&sales, PAID_AT, from_view(&view));
    let result = checks.check(&sales, PAID_AT + PAID_CHECK_WAIT_SECS, from_view(&view));
    assert_eq!(result.raised, vec![key(0)], "the check ran");
    assert!(checks.store_failed());
    assert!(matches!(checks.trouble(), Some(PaidTrouble::NotKept(_))));
    assert_eq!(checks.marks().len(), 1, "held in memory");
    // The folder is there now: the next run writes what it held.
    std::fs::create_dir_all(&folder).unwrap();
    let result = checks.check(
        &sales,
        PAID_AT + PAID_CHECK_WAIT_SECS + 60,
        from_view(&view),
    );
    assert!(result.raised.is_empty());
    assert!(!checks.store_failed() && checks.trouble().is_none());
    let again = PaidChecks::open(&folder);
    assert_eq!(again.marks(), checks.marks());
    assert_eq!(again.marks().get(&key(0)).unwrap().state, MarkState::Raised);
}

#[test]
fn oc_63_the_day_runs_from_local_midnight() {
    use crate::paid_check::day_bounds;
    let midnight = 1_800_000_000 - 1_800_000_000 % 86_400;
    assert_eq!(day_bounds(midnight + 5, 0), (midnight, midnight + 86_400));
    // At two hours east, local midnight is two hours before UTC midnight.
    assert_eq!(
        day_bounds(midnight + 5, 7_200),
        (midnight - 7_200, midnight + 86_400 - 7_200)
    );
    // At five hours west, 03:00 UTC is still the day before.
    assert_eq!(
        day_bounds(midnight + 3 * 3_600, -5 * 3_600),
        (midnight - 86_400 + 5 * 3_600, midnight + 5 * 3_600)
    );
}

#[test]
fn oc_61_an_index_the_owner_never_issued_to_the_counter_is_a_mismatch_at_once() {
    let owner = new_owner();
    let view = view_with(&[]);
    // Index 20 is past the range of 20 addresses the owner issued.
    let sales = [
        sale(20, 5_000, PaidHow::AtOnce),
        sale(0, 5_000, PaidHow::AtOnce),
    ];
    // Paid a second ago: the wait does not apply to the unissued one.
    let result = check_paid(&sales, &PaidMarks::new(), PAID_AT + 1, |k| {
        verified_facts(&view, &owner, k, TIP, DEPTH)
    });
    assert_eq!(state_of(&result, 20), PaidState::Mismatch);
    assert_eq!(result.checks[0].seen_sats, Some(0));
    assert_eq!(
        state_of(&result, 0),
        PaidState::Waiting,
        "an issued one waits"
    );
    assert_eq!(result.raised, vec![key(20)]);
    // It is raised once: a mark stands.
    let marks = PaidMarks::new().after(&result, PAID_AT + 1);
    let again = check_paid(&sales, &marks, PAID_AT + 2, |k| {
        verified_facts(&view, &owner, k, TIP, DEPTH)
    });
    assert_eq!(state_of(&again, 20), PaidState::Mismatch);
    assert!(again.raised.is_empty());
}

fn other_counter() -> DeviceId {
    id(4).device_id()
}

fn key_of(counter: DeviceId, index: u32) -> SaleKey {
    SaleKey { counter, index }
}

#[test]
fn oc_60_a_lightning_paid_is_exempt_only_from_a_counter_the_settings_have_named() {
    let events = [opened(1, PAID_AT - 50, 100), lightning(1, PAID_AT + 1, 700)];
    let (link, _a, _b) = link_with(&events);
    let now = PAID_AT + 10;
    // The recording counter is named: exempt, no state.
    let found = paid_sales(&link, &named_counters(&[counter()]));
    assert_eq!(found.sales[0].how, PaidHow::Lightning);
    let result = check_paid(&found.sales, &PaidMarks::new(), now, |_| {
        panic!("a named counter's Lightning sale is not looked up")
    });
    assert!(result.checks.is_empty() && result.raised.is_empty());
    // Only another counter is named, or none: a mismatch at once, nothing
    // seen, and the fold goes on (the counter is not unreadable).
    for set in [named_counters(&[other_counter()]), named_counters(&[])] {
        let found = paid_sales(&link, &set);
        assert!(found.unreadable.is_empty());
        assert_eq!(found.sales[0].how, PaidHow::LightningUnnamed);
        let result = check_paid(&found.sales, &PaidMarks::new(), now, |_| {
            panic!("no address to look at")
        });
        assert_eq!(state_of(&result, 1), PaidState::Mismatch);
        assert_eq!(result.checks[0].seen_sats, Some(0));
        assert_eq!(result.raised, vec![key(1)]);
        // Raised once.
        let marks = PaidMarks::new().after(&result, now);
        let again = check_paid(&found.sales, &marks, now + 1, |_| {
            panic!("no address to look at")
        });
        assert!(again.raised.is_empty());
    }
}

/// A set of Lightning counters, kept in a folder of its own.
fn named_counters(ids: &[DeviceId]) -> LightningCounters {
    let dir = Dir::new();
    let mut set = LightningCounters::open(dir.path());
    for id in ids {
        set.note(id);
    }
    // The folder lives as long as the process: the set is kept in memory.
    std::mem::forget(dir);
    set
}

#[test]
fn oc_60_a_counter_once_named_stays_exempt_when_shop_lightning_is_removed_or_moved() {
    let dir = Dir::new();
    let mut set = LightningCounters::open(dir.path());
    // Named, then the settings drop Shop Lightning: nothing is taken out.
    set.note(&counter());
    // Moved to another counter: both are exempt.
    set.note(&other_counter());
    set.note(&other_counter());
    let reopened = LightningCounters::open(dir.path());
    assert!(reopened.contains(&counter()) && reopened.contains(&other_counter()));
    let events = [opened(1, PAID_AT - 50, 100), lightning(1, PAID_AT + 1, 700)];
    let (link, _a, _b) = link_with(&events);
    assert_eq!(
        paid_sales(&link, &reopened).sales[0].how,
        PaidHow::Lightning
    );
    // A counter never named is still a mismatch.
    let never = LightningCounters::open(Dir::new().path());
    assert!(!never.contains(&counter()));
    let only_other = {
        let d = Dir::new();
        let mut s = LightningCounters::open(d.path());
        s.note(&other_counter());
        s
    };
    assert_eq!(
        paid_sales(&link, &only_other).sales[0].how,
        PaidHow::LightningUnnamed
    );
}

#[test]
fn oc_60_the_file_of_lightning_counters_reads_back_strictly() {
    let (a, b) = {
        let mut two = [counter(), other_counter()];
        two.sort();
        (two[0], two[1])
    };
    let dir = Dir::new();
    let mut set = LightningCounters::open(dir.path());
    set.note(&b);
    set.note(&a);
    let bytes = std::fs::read(dir.path().join(LIGHTNING_COUNTERS_FILE)).unwrap();
    assert_eq!(&bytes[..4], &MAGIC_LIGHTNING_COUNTERS);
    assert_eq!(LightningCounters::from_bytes(&bytes).unwrap(), vec![a, b]);
    assert_eq!(set.to_bytes(), bytes, "written as it reads");
    // Truncated, trailing byte, out of order, twice, too many: refused.
    assert!(LightningCounters::from_bytes(&bytes[..bytes.len() - 1]).is_err());
    let mut trailing = bytes.clone();
    trailing.push(0);
    assert!(LightningCounters::from_bytes(&trailing).is_err());
    let mut unsorted = bytes[..9].to_vec();
    unsorted.extend_from_slice(&b.to_bytes());
    unsorted.extend_from_slice(&a.to_bytes());
    assert!(LightningCounters::from_bytes(&unsorted).is_err());
    let mut twice = bytes[..9].to_vec();
    twice.extend_from_slice(&a.to_bytes());
    twice.extend_from_slice(&a.to_bytes());
    assert!(LightningCounters::from_bytes(&twice).is_err());
    let mut many = bytes[..5].to_vec();
    many.extend_from_slice(&(openbmt_core::settings::MAX_DEVICES as u32 + 1).to_le_bytes());
    assert!(LightningCounters::from_bytes(&many).is_err());
    assert!(LightningCounters::from_bytes(&[]).is_err());
    // A file that does not read: the node starts, the set is empty, the file
    // is left as it is, and a counter named now is held in memory.
    for bad in [&trailing, &unsorted] {
        let broken = Dir::new();
        std::fs::write(broken.path().join(LIGHTNING_COUNTERS_FILE), bad).unwrap();
        let mut set = LightningCounters::open(broken.path());
        assert!(!set.contains(&a) && !set.contains(&b));
        set.note(&a);
        assert!(set.contains(&a));
        assert_eq!(
            &std::fs::read(broken.path().join(LIGHTNING_COUNTERS_FILE)).unwrap(),
            bad
        );
    }
}

/// `n` sales of `counter`, paid at once, whose payments are nowhere.
fn missing_sales(counter: DeviceId, n: u32) -> Vec<PaidSale> {
    (0..n)
        .map(|i| PaidSale {
            counter,
            ..sale(i, 5_000, PaidHow::AtOnce)
        })
        .collect()
}

#[test]
fn oc_61_a_counter_at_the_alarm_cap_is_checked_no_further_until_the_owner_clears() {
    let cap = MAX_OPEN_ALARMS_PER_COUNTER as u32;
    let now = PAID_AT + PAID_CHECK_WAIT_SECS;
    let none = |_: SaleKey| {
        Ok(SaleFacts {
            received_sats: 0,
            reached_refund_depth: false,
        })
    };
    let sales = missing_sales(counter(), cap + 40);
    let other = missing_sales(other_counter(), 2);
    let all: Vec<PaidSale> = sales.iter().chain(&other).cloned().collect();
    let marks = read_at(&all, PAID_AT);
    // One run raises up to the cap; the other counter is checked.
    let result = check_paid(&all, &marks, now, none);
    assert_eq!(
        result
            .raised
            .iter()
            .filter(|k| k.counter == counter())
            .count(),
        cap as usize
    );
    assert_eq!(
        result
            .raised
            .iter()
            .filter(|k| k.counter == other_counter())
            .count(),
        2
    );
    let marks = marks.after(&result, now);
    assert_eq!(marks.open_alarms()[&counter()], cap as usize);
    // The next run: the counter shows as unreadable, none of its sales are
    // checked, and its alarms stay.
    let found = gate_open_alarms(
        PaidSales {
            sales: all.clone(),
            unreadable: vec![],
        },
        &marks,
    );
    assert_eq!(found.unreadable, vec![counter()]);
    assert!(found.sales.iter().all(|s| s.counter == other_counter()));
    let again = check_paid(&found.sales, &marks, now + 60, none);
    assert!(again.raised.is_empty());
    assert_eq!(
        marks.after(&again, now + 60).open_alarms()[&counter()],
        cap as usize
    );
    // Clearing one resumes the checks of the counter.
    let cleared = marks.cleared_by_owner(&key(0), now + 100).unwrap();
    assert_eq!(cleared.open_alarms()[&counter()], cap as usize - 1);
    let found = gate_open_alarms(
        PaidSales {
            sales: all,
            unreadable: vec![],
        },
        &cleared,
    );
    assert!(found.unreadable.is_empty());
    assert!(found.sales.iter().any(|s| s.counter == counter()));
    // With the sales checked again, the counter raises one more at most.
    let resumed = check_paid(&found.sales, &cleared, now + 200, none);
    assert_eq!(
        resumed
            .raised
            .iter()
            .filter(|k| k.counter == counter())
            .count(),
        1
    );
}

#[test]
fn oc_64_the_alarms_of_every_counter_at_the_cap_fit_the_marks() {
    const {
        assert!(openbmt_core::settings::MAX_DEVICES * MAX_OPEN_ALARMS_PER_COUNTER <= MAX_PAID_MARKS)
    };
}

#[test]
fn oc_64_eviction_drops_cleared_then_readings_and_never_an_alarm() {
    // Readings, cleared marks and alarms of another counter, over the limit.
    let other = other_counter();
    let mut raised = raised_marks(0, MAX_PAID_MARKS as u32 - 10, 100);
    // Ten cleared (oldest first) and ten readings beyond the limit.
    let extra = CheckResult {
        raised: (0..10).map(|i| key_of(other, i)).collect(),
        read: (10..20).map(|i| key_of(other, i)).collect(),
        ..CheckResult::default()
    };
    raised = raised.after(&extra, 500);
    for i in 0..10 {
        raised = raised
            .cleared_by_owner(&key_of(other, i), 1_000 + u64::from(i))
            .unwrap();
    }
    assert_eq!(raised.len(), MAX_PAID_MARKS + 10);
    let fitted = raised.fitted();
    assert_eq!(fitted.len(), MAX_PAID_MARKS);
    // The ten cleared went first; the readings stayed.
    assert!((0..10).all(|i| fitted.get(&key_of(other, i)).is_none()));
    assert!((10..20).all(|i| fitted.get(&key_of(other, i)).is_some()));
    // With no cleared marks, the newest readings go, and the alarms stay.
    let mut more = raised_marks(0, MAX_PAID_MARKS as u32 - 2, 100);
    more = more.after(
        &CheckResult {
            read: vec![key_of(other, 1), key_of(other, 2), key_of(other, 3)],
            ..CheckResult::default()
        },
        10,
    );
    let more = more.after(&CheckResult::default(), 11);
    assert_eq!(more.len(), MAX_PAID_MARKS + 1);
    // The marks were first read at 10 each; the newest sorts by key then.
    let fitted = more.fitted();
    assert_eq!(fitted.len(), MAX_PAID_MARKS);
    assert_eq!(
        fitted
            .iter()
            .filter(|m| m.state == MarkState::Raised)
            .count(),
        MAX_PAID_MARKS - 2
    );
}

#[test]
fn oc_64_marks_that_stay_over_keep_the_file_and_report_full() {
    let dir = Dir::new();
    let full = raised_marks(1, MAX_PAID_MARKS as u32 + 1, 100);
    std::fs::write(dir.path().join(PAID_CHECK_FILE), full.to_bytes()).unwrap();
    let before = std::fs::read(dir.path().join(PAID_CHECK_FILE)).unwrap();
    let mut checks = PaidChecks::open(dir.path());
    let sales = [sale(0, 5_000, PaidHow::AtOnce)];
    let view = view_with(&[]);
    checks.check(&sales, PAID_AT, from_view(&view));
    checks.check(&sales, PAID_AT + PAID_CHECK_WAIT_SECS, from_view(&view));
    // Nothing but alarms: no alarm is dropped, the file is as it was.
    assert_eq!(checks.trouble(), Some(PaidTrouble::Full));
    assert_eq!(checks.marks().len(), MAX_PAID_MARKS + 1);
    assert_eq!(
        std::fs::read(dir.path().join(PAID_CHECK_FILE)).unwrap(),
        before
    );
}

fn settings_naming(counter: Option<DeviceId>) -> openbmt_core::settings::Settings {
    let mut s = openbmt_core::settings::Settings::defaults(1_000);
    s.shop_lightning = counter.map(|counter| openbmt_core::settings::ShopLightning {
        counter,
        node_id: [2; 33],
        second_device: id(9).device_id(),
        provider_node_id: [3; 33],
        provider_addr: "203.0.113.7:9735".parse().unwrap(),
        cap_sats: 1,
        max_opening_fee_sats: 1,
    });
    s
}

#[test]
fn oc_60_settings_in_force_add_their_lightning_counter_and_later_settings_take_none_out() {
    let dir = Dir::new();
    let mut set = LightningCounters::open(dir.path());
    // Settings that name no Lightning counter add nothing.
    set.note_settings(&settings_naming(None));
    assert!(!dir.path().join(LIGHTNING_COUNTERS_FILE).exists());
    // Named, then moved, then removed, with no check between: both stay.
    set.note_settings(&settings_naming(Some(counter())));
    set.note_settings(&settings_naming(Some(other_counter())));
    set.note_settings(&settings_naming(None));
    let reopened = LightningCounters::open(dir.path());
    assert!(reopened.contains(&counter()) && reopened.contains(&other_counter()));
}

/// A folder holding a file of Lightning counters that does not read, and the
/// bytes it holds.
fn unreadable_counters() -> (Dir, Vec<u8>) {
    let dir = Dir::new();
    let bad = vec![1u8, 2, 3];
    std::fs::write(dir.path().join(LIGHTNING_COUNTERS_FILE), &bad).unwrap();
    (dir, bad)
}

fn lightning_link_sales(set: &LightningCounters) -> Vec<PaidSale> {
    let events = [opened(1, PAID_AT - 50, 100), lightning(1, PAID_AT + 1, 700)];
    let (link, _a, _b) = link_with(&events);
    paid_sales(&link, set).sales
}

#[test]
fn oc_60_an_unreadable_file_of_lightning_counters_gives_no_false_alarm() {
    let (dir, bad) = unreadable_counters();
    let mut set = LightningCounters::open(dir.path());
    let sales = lightning_link_sales(&set);
    assert_eq!(sales[0].how, PaidHow::LightningUnknown);
    let now = PAID_AT + PAID_CHECK_WAIT_SECS;
    let result = check_paid(&sales, &PaidMarks::new(), now, |_| {
        panic!("an unknown Lightning sale is not looked up")
    });
    assert_eq!(state_of(&result, 1), PaidState::NotCheckable);
    assert_eq!(result.checks[0].seen_sats, None);
    assert!(result.raised.is_empty());
    assert_eq!(result.settled, vec![key(1)]);
    // The file is left as it is when a new counter is noted.
    set.note(&other_counter());
    assert_eq!(
        std::fs::read(dir.path().join(LIGHTNING_COUNTERS_FILE)).unwrap(),
        bad
    );
}

#[test]
fn oc_60_a_good_file_that_does_not_name_the_counter_is_a_mismatch() {
    let set = named_counters(&[other_counter()]);
    let sales = lightning_link_sales(&set);
    assert_eq!(sales[0].how, PaidHow::LightningUnnamed);
    let now = PAID_AT + 10;
    let result = check_paid(&sales, &PaidMarks::new(), now, |_| {
        panic!("no address to look at")
    });
    assert_eq!(state_of(&result, 1), PaidState::Mismatch);
    assert_eq!(result.raised, vec![key(1)]);
}

#[test]
fn oc_60_a_good_file_naming_the_counter_leaves_the_sale_out_of_the_checks() {
    let set = named_counters(&[counter()]);
    let sales = lightning_link_sales(&set);
    assert_eq!(sales[0].how, PaidHow::Lightning);
    let result = check_paid(&sales, &PaidMarks::new(), PAID_AT + 10, |_| {
        panic!("a named counter's Lightning sale is not looked up")
    });
    assert!(result.checks.is_empty() && result.raised.is_empty());
}

#[test]
fn oc_60_with_an_unreadable_file_a_counter_named_in_this_run_is_still_exempt() {
    let (dir, _bad) = unreadable_counters();
    let mut set = LightningCounters::open(dir.path());
    set.note_settings(&settings_naming(Some(counter())));
    let sales = lightning_link_sales(&set);
    assert_eq!(sales[0].how, PaidHow::Lightning);
}

#[test]
fn oc_60_the_day_summary_counts_an_unknown_lightning_sale_as_not_checked() {
    let (dir, _bad) = unreadable_counters();
    let set = LightningCounters::open(dir.path());
    let sales = lightning_link_sales(&set);
    let now = PAID_AT + PAID_CHECK_WAIT_SECS;
    let result = check_paid(&sales, &PaidMarks::new(), now, |_| panic!("not looked up"));
    let days = day_summary(&sales, &result.checks, now - 1, now + 1);
    assert_eq!(days.len(), 1);
    let d = &days[0];
    assert_eq!(d.sales, 1);
    assert_eq!((d.seen, d.waiting, d.too_old, d.missing), (0, 0, 1, 0));
}
