//! A counter's Paid, checked against the owner's own view of the wallet
//! (`docs/OWNER-COINS-RULES.md`, OC-60 to OC-65).

use openbmt_core::wire::DecodeError;
use openbmt_counter::{Amount, Event, Role};
use openbmt_owner::{SaleFacts, SaleKey};

use super::pos::{at_once, link_with, opened};
use super::refunds::{new_owner, view_with, DEPTH, TIP};
use super::*;
use crate::coins::CoinView;
use crate::facts::{verified_facts, FactsRefused};
use crate::paid_check::{
    check_paid, day_summary, paid_sales, paid_sales_from_chain, CheckResult, MarkState,
    PaidCheckRefused, PaidChecks, PaidHow, PaidMarks, PaidSale, PaidState, MAGIC_PAID_CHECK,
    MAX_PAID_MARKS, PAID_CHECK_FILE, PAID_CHECK_WAIT_SECS,
};

const PAID_AT: u64 = 1_800_000_000;

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
        paid_at: PAID_AT,
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

#[test]
fn oc_60_a_paid_with_no_receipt_raises_the_alarm_after_the_wait_and_not_before() {
    let view = view_with(&[]);
    let sales = [sale(0, 5_000, PaidHow::AtOnce)];
    let marks = PaidMarks::new();
    // One second short of the wait: listed as waiting, nothing raised.
    let early = check_paid(
        &sales,
        &marks,
        PAID_AT + PAID_CHECK_WAIT_SECS - 1,
        from_view(&view),
    );
    assert_eq!(state_of(&early, 0), PaidState::Waiting);
    assert!(early.raised.is_empty());
    // At the wait it is a mismatch, raised.
    let late = check_paid(
        &sales,
        &marks,
        PAID_AT + PAID_CHECK_WAIT_SECS,
        from_view(&view),
    );
    assert_eq!(state_of(&late, 0), PaidState::Mismatch);
    assert_eq!(late.raised, vec![key(0)]);
    assert_eq!(late.checks[0].seen_sats, Some(0));
}

#[test]
fn oc_61_the_wait_is_a_day_and_is_marked_to_tune() {
    assert_eq!(PAID_CHECK_WAIT_SECS, 86_400);
}

#[test]
fn oc_62_a_mismatch_is_raised_once_per_sale() {
    let view = view_with(&[]);
    let sales = [sale(0, 5_000, PaidHow::Confirmed)];
    let now = PAID_AT + PAID_CHECK_WAIT_SECS;
    let first = check_paid(&sales, &PaidMarks::new(), now, from_view(&view));
    let marks = PaidMarks::new().after(&first, now).unwrap();
    let again = check_paid(&sales, &marks, now + 60, from_view(&view));
    assert_eq!(state_of(&again, 0), PaidState::Mismatch);
    assert!(again.raised.is_empty(), "the second check raises nothing");
    assert_eq!(marks.get(&key(0)).unwrap().state, MarkState::Raised);
}

#[test]
fn oc_62_a_payment_arriving_later_clears_the_alarm_itself() {
    let sales = [sale(0, 5_000, PaidHow::AtOnce)];
    let now = PAID_AT + PAID_CHECK_WAIT_SECS;
    let missing = check_paid(&sales, &PaidMarks::new(), now, from_view(&view_with(&[])));
    let marks = PaidMarks::new().after(&missing, now).unwrap();
    // The payment arrives in full, in a block.
    let paid = view_with(&[(0, 5_000, 2)]);
    let arrived = check_paid(&sales, &marks, now + 600, from_view(&paid));
    assert_eq!(state_of(&arrived, 0), PaidState::ClearedByArrival);
    assert_eq!(arrived.cleared, vec![key(0)]);
    let marks = marks.after(&arrived, now + 600).unwrap();
    let mark = marks.get(&key(0)).unwrap();
    assert_eq!(mark.state, MarkState::ClearedByArrival);
    assert_eq!(mark.cleared_at, Some(now + 600));
    // It stays cleared on the next check, and is not cleared a second time.
    let next = check_paid(&sales, &marks, now + 1_200, from_view(&paid));
    assert_eq!(state_of(&next, 0), PaidState::ClearedByArrival);
    assert!(next.cleared.is_empty() && next.raised.is_empty());
    // A payment that goes missing again does not raise it a second time.
    let gone = check_paid(&sales, &marks, now + 1_800, from_view(&view_with(&[])));
    assert_eq!(state_of(&gone, 0), PaidState::Mismatch);
    assert!(gone.raised.is_empty());
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
    let result = check_paid(&sales, &PaidMarks::new(), now, from_view(&view));
    assert_eq!(state_of(&result, 0), PaidState::Checked);
    assert_eq!(state_of(&result, 1), PaidState::Mismatch);
    assert_eq!(result.raised, vec![key(1)]);
    // The sale's own tolerance counts, not a fixed one.
    let mut strict = sale(0, 10_000, PaidHow::Confirmed);
    strict.tolerance_bps = 0;
    let result = check_paid(&[strict], &PaidMarks::new(), now, from_view(&view));
    assert_eq!(state_of(&result, 0), PaidState::Mismatch);
}

#[test]
fn oc_60_more_received_than_recorded_is_not_a_mismatch() {
    let now = PAID_AT + PAID_CHECK_WAIT_SECS + 1;
    let view = view_with(&[(0, 7_000, DEPTH)]);
    let result = check_paid(
        &[sale(0, 5_000, PaidHow::Confirmed)],
        &PaidMarks::new(),
        now,
        from_view(&view),
    );
    assert_eq!(state_of(&result, 0), PaidState::Checked);
    assert!(result.raised.is_empty());
}

#[test]
fn oc_60_a_lightning_sale_is_not_checked() {
    let now = PAID_AT + 10 * PAID_CHECK_WAIT_SECS;
    let sales = [
        sale(0, 5_000, PaidHow::Lightning),
        sale(1, 5_000, PaidHow::AtOnce),
    ];
    let result = check_paid(&sales, &PaidMarks::new(), now, from_view(&view_with(&[])));
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
    let raised = PaidMarks::new()
        .after(
            &CheckResult {
                raised: vec![key(0)],
                ..CheckResult::default()
            },
            now,
        )
        .unwrap();
    let result = check_paid(&sales, &raised, now + 1, |_| {
        Err(FactsRefused::ReceiptLogFull)
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
fn oc_63_the_day_summary_counts_each_counters_sales() {
    let now = PAID_AT + PAID_CHECK_WAIT_SECS + 100;
    let view = view_with(&[(0, 4_000, DEPTH), (1, 6_000, DEPTH), (2, 1_000, DEPTH)]);
    let mut young = sale(3, 2_000, PaidHow::AtOnce);
    young.paid_at = now - 10;
    let mut yesterday = sale(5, 9_000, PaidHow::Confirmed);
    yesterday.paid_at = PAID_AT - 86_400;
    let sales = [
        sale(0, 4_000, PaidHow::Confirmed),
        sale(1, 6_000, PaidHow::Confirmed),
        sale(2, 3_000, PaidHow::AtOnce),
        young,
        sale(4, 800, PaidHow::Confirmed),
        yesterday,
        sale(6, 700, PaidHow::Lightning),
    ];
    let facts = |k: SaleKey| {
        if k.index == 4 {
            Err(FactsRefused::TooOldForFacts)
        } else {
            from_view(&view)(k)
        }
    };
    let result = check_paid(&sales, &PaidMarks::new(), now, facts);
    let days = day_summary(&sales, &result.checks, PAID_AT - 1, PAID_AT + 1 + 100);
    // The sale of 2 seconds short of now falls outside this window.
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
    let result = check_paid(&exact, &PaidMarks::new(), now, from_view(&view));
    let days = day_summary(&exact, &result.checks, PAID_AT - 1, PAID_AT + 1);
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

#[test]
fn oc_60_the_records_tell_an_on_chain_payment_from_lightning() {
    let (link, _a, _b) = link_with(&[
        named(0, PAID_AT - 50, "Ana"),
        at_once_for(0, PAID_AT, 5_000),
        opened(1, PAID_AT - 50, 100),
        lightning(1, PAID_AT + 1, 700),
        opened(2, PAID_AT - 50, 100),
        confirmed(2, PAID_AT + 2, 9_000),
        // Accepted at once, then confirmed: the time of the first Paid
        // stands, and the amount is the confirmed one.
        opened(3, PAID_AT - 50, 100),
        at_once_for(3, PAID_AT + 3, 100),
        confirmed(3, PAID_AT + 400, 1_200),
        // Never paid.
        opened(4, PAID_AT - 50, 100),
    ]);
    let sales = paid_sales(&link, 50).unwrap();
    let by: Vec<(u32, PaidHow, u64, u64)> = sales
        .iter()
        .map(|s| (s.index, s.how, s.recorded_sats, s.paid_at))
        .collect();
    assert_eq!(
        by,
        vec![
            (0, PaidHow::AtOnce, 5_000, PAID_AT),
            (1, PaidHow::Lightning, 700, PAID_AT + 1),
            (2, PaidHow::Confirmed, 9_000, PAID_AT + 2),
            (3, PaidHow::Confirmed, 1_200, PAID_AT + 3),
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
fn oc_60_a_sale_that_was_reopened_or_replaced_is_not_paid_and_a_swap_is_on_chain() {
    let (link, _a, _b) = link_with(&[
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
        // Paid again after the reopening: a new Paid.
        at_once_for(1, PAID_AT + 100, 5_000),
        // A manager's acceptance is not a payment to the address.
        opened(2, PAID_AT - 50, 100),
        Event::Override {
            index: 2,
            at: PAID_AT,
            role: Role::Manager,
            name: "Bo".to_string(),
            sats: 5_000,
        },
        // A payment confirmed, then a conflict seen: still paid.
        opened(3, PAID_AT - 50, 100),
        confirmed(3, PAID_AT, 4_000),
        Event::Replaced {
            index: 3,
            at: PAID_AT + 1,
        },
    ]);
    let sales = paid_sales(&link, 0).unwrap();
    let by: Vec<(u32, PaidHow, u64)> = sales.iter().map(|s| (s.index, s.how, s.paid_at)).collect();
    assert_eq!(
        by,
        vec![
            (1, PaidHow::AtOnce, PAID_AT + 100),
            (3, PaidHow::Confirmed, PAID_AT)
        ]
    );
}

#[test]
fn oc_30_a_copy_that_does_not_fold_gives_no_paid_sales() {
    let (link, _a, _b) = link_with(&[opened(0, PAID_AT - 50, 100), at_once_for(9, PAID_AT, 1)]);
    assert!(matches!(
        paid_sales(&link, 0),
        Err(PaidCheckRefused::Facts { .. })
    ));
    // And a chain that does not verify gives none.
    let chain = openbmt_core::records::Chain::new();
    let devices = openbmt_core::records::DeviceList::new([]);
    assert!(paid_sales_from_chain(&chain, &devices, &counter(), 0)
        .unwrap()
        .is_empty());
}

// ---- the marks -----------------------------------------------------------------------

fn raised_marks(count: u32, now: u64) -> PaidMarks {
    PaidMarks::new()
        .after(
            &CheckResult {
                raised: (0..count).map(key).collect(),
                ..CheckResult::default()
            },
            now,
        )
        .unwrap()
}

#[test]
fn oc_64_the_marks_read_back_strictly() {
    let marks = raised_marks(3, 1_000);
    let marks = marks.cleared_by_owner(&key(1), 2_000).unwrap();
    let bytes = marks.to_bytes();
    assert_eq!(&bytes[..4], &MAGIC_PAID_CHECK);
    assert_eq!(&MAGIC_PAID_CHECK, b"OBM\x17");
    assert_eq!(bytes[4], 1, "version 1");
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
    bad[4] = 2;
    assert!(matches!(
        PaidMarks::from_bytes(&bad),
        Err(DecodeError::UnsupportedVersion(2))
    ));
    // A count that is more than the bytes hold, or more than the cap.
    let mut bad = bytes.clone();
    bad[5..9].copy_from_slice(&4u32.to_le_bytes());
    assert!(PaidMarks::from_bytes(&bad).is_err());
    let mut bad = bytes.clone();
    bad[5..9].copy_from_slice(&(MAX_PAID_MARKS as u32 + 1).to_le_bytes());
    assert!(PaidMarks::from_bytes(&bad).is_err());
    // A state that is none of the three.
    let state_at = 9 + 32 + 4;
    let mut bad = bytes.clone();
    bad[state_at] = 9;
    assert!(matches!(
        PaidMarks::from_bytes(&bad),
        Err(DecodeError::UnknownKind(9))
    ));
    // A raised mark with a time cleared, and a cleared one with none.
    let mut bad = bytes.clone();
    bad[state_at + 9..state_at + 17].copy_from_slice(&5u64.to_le_bytes());
    assert!(PaidMarks::from_bytes(&bad).is_err());
    let mut bad = bytes.clone();
    bad[state_at] = 2;
    assert!(PaidMarks::from_bytes(&bad).is_err());
    // Marks out of order, and the same mark twice.
    let entry = |i: usize| bytes[9 + i * 53..9 + (i + 1) * 53].to_vec();
    let mut swapped = bytes[..9].to_vec();
    for i in [1, 0, 2] {
        swapped.extend(entry(i));
    }
    assert!(PaidMarks::from_bytes(&swapped).is_err());
    let mut twice = bytes[..9].to_vec();
    for i in [0, 0, 2] {
        twice.extend(entry(i));
    }
    assert!(PaidMarks::from_bytes(&twice).is_err());
    // A file larger than the cap's whole frame.
    assert!(matches!(
        PaidMarks::from_bytes(&vec![0; 5 + 4 + MAX_PAID_MARKS * 53 + 1]),
        Err(DecodeError::TooLarge)
    ));
}

#[test]
fn oc_64_the_oldest_cleared_marks_go_first_and_a_raised_one_is_never_dropped() {
    let full = raised_marks(MAX_PAID_MARKS as u32, 100);
    assert_eq!(full.len(), MAX_PAID_MARKS);
    // Every mark is a raised alarm: nothing can go, so another is refused.
    let one_more = CheckResult {
        raised: vec![key(MAX_PAID_MARKS as u32)],
        ..CheckResult::default()
    };
    assert_eq!(full.after(&one_more, 200), Err(PaidCheckRefused::MarksFull));
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
    let next = marks.after(&two_more, 5_000).unwrap();
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
    let result = checks.check(&sales, now, from_view(&view)).unwrap();
    assert_eq!(result.raised, vec![key(0)]);
    assert_eq!(
        checks.clear(&key(9), now),
        Err(PaidCheckRefused::NoSuchAlarm),
        "no alarm of that sale"
    );
    checks.clear(&key(0), now + 5).unwrap();
    let mark = *checks.marks().get(&key(0)).unwrap();
    assert_eq!(mark.state, MarkState::ClearedByOwner);
    assert_eq!((mark.raised_at, mark.cleared_at), (now, Some(now + 5)));
    assert_eq!(
        checks.clear(&key(0), now + 6),
        Err(PaidCheckRefused::NoSuchAlarm)
    );
    // A cleared alarm is not raised again, and the sale is still missing.
    let result = checks.check(&sales, now + 60, from_view(&view)).unwrap();
    assert_eq!(state_of(&result, 0), PaidState::Mismatch);
    assert!(result.raised.is_empty());
    // The marks are in the file, and a new start reads them back.
    let again = PaidChecks::open(dir.path());
    assert_eq!(again.marks(), checks.marks());
    assert!(again.not_kept().is_none());
}

#[test]
fn oc_64_a_file_that_does_not_read_back_does_not_stop_the_node() {
    let dir = Dir::new();
    std::fs::write(dir.path().join(PAID_CHECK_FILE), b"OBM\x17\x01garbage").unwrap();
    let mut checks = PaidChecks::open(dir.path());
    assert!(checks.not_kept().is_some(), "the state is named");
    assert!(checks.marks().is_empty());
    let sales = [sale(0, 5_000, PaidHow::AtOnce)];
    let view = view_with(&[]);
    let refused = checks.check(&sales, PAID_AT, from_view(&view)).unwrap_err();
    assert!(matches!(refused, PaidCheckRefused::NotKept { .. }));
    assert!(refused.to_string().starts_with("OC-64:"));
    assert!(matches!(
        checks.clear(&key(0), PAID_AT),
        Err(PaidCheckRefused::NotKept { .. })
    ));
    // Nothing was written over the file.
    assert_eq!(
        std::fs::read(dir.path().join(PAID_CHECK_FILE)).unwrap(),
        b"OBM\x17\x01garbage"
    );
    // The owner clears the checks, and they start again.
    checks.reset().unwrap();
    assert!(checks.not_kept().is_none());
    assert!(!dir.path().join(PAID_CHECK_FILE).exists());
    let now = PAID_AT + PAID_CHECK_WAIT_SECS;
    let result = checks.check(&sales, now, from_view(&view)).unwrap();
    assert_eq!(result.raised, vec![key(0)]);
    assert!(dir.path().join(PAID_CHECK_FILE).exists());
    // A file too large for its bound is refused too, not read.
    let big = Dir::new();
    std::fs::write(
        big.path().join(PAID_CHECK_FILE),
        vec![0u8; 5 + 4 + MAX_PAID_MARKS * 53 + 1],
    )
    .unwrap();
    assert!(PaidChecks::open(big.path()).not_kept().is_some());
}

#[test]
fn oc_64_marks_that_cannot_be_stored_take_no_effect() {
    let dir = Dir::new();
    let mut checks = PaidChecks::open(&dir.path().join("missing"));
    let sales = [sale(0, 5_000, PaidHow::AtOnce)];
    let now = PAID_AT + PAID_CHECK_WAIT_SECS;
    let err = checks
        .check(&sales, now, from_view(&view_with(&[])))
        .unwrap_err();
    assert!(matches!(err, PaidCheckRefused::Store(_)));
    assert!(checks.marks().is_empty(), "nothing was raised");
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
