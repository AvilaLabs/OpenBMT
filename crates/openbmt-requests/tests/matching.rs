//! RQ-15 to RQ-23: what arrives, what it makes a request, due and overdue.

mod common;

use common::*;
use openbmt_requests::say::{mark_lines, overdue_line, overdue_row, state_line};
use openbmt_requests::{After, CreditRefused, End, EndRefused, Marks, RequestBook, Shop, Status};

fn status(book: &RequestBook, n: u64) -> Status {
    book.request(n).unwrap().status(5_000)
}

#[test]
fn rq_15_every_output_paid_to_the_address_is_credited_in_bitcoin_by_its_satoshis() {
    let mut book = RequestBook::new();
    let n = make_btc(&mut book, 0, 100_000, 7);
    let counted = book
        .credit(
            n,
            &[arrival(1, 10, T0, 30_000), arrival(2, 11, T0 + 600, 20_000)],
            T0 + 900,
            &shop(),
            None,
        )
        .unwrap();
    assert_eq!(counted, 2);
    let r = book.request(n).unwrap();
    assert_eq!(r.credited(), 50_000);
    assert_eq!(r.owed(), 50_000);
    assert_eq!(r.credits.len(), 2);
    assert_eq!((r.credits[0].sats, r.credits[0].value), (30_000, 30_000));
    assert_eq!(r.credits[1].block_time, T0 + 600);
}

#[test]
fn rq_15_an_output_already_counted_is_skipped_so_the_whole_history_may_be_handed_over() {
    let mut book = RequestBook::new();
    let n = make_btc(&mut book, 0, 100_000, 7);
    let all = [arrival(1, 10, T0, 30_000), arrival(2, 11, T0 + 600, 20_000)];
    assert_eq!(book.credit(n, &all, T0, &shop(), None), Ok(2));
    let before = book.clone();
    assert_eq!(book.credit(n, &all, T0, &shop(), None), Ok(0));
    assert_eq!(book, before);
    // The same transaction with another output is another payment.
    let mut other = arrival(1, 10, T0, 7_000);
    other.vout = 1;
    assert_eq!(book.credit(n, &[other], T0, &shop(), None), Ok(1));
}

#[test]
fn rq_15_the_payments_must_be_in_the_order_of_the_blocks() {
    let mut book = RequestBook::new();
    let n = make_btc(&mut book, 0, 100_000, 7);
    let before = book.clone();
    assert_eq!(
        book.credit(
            n,
            &[arrival(1, 11, T0, 30_000), arrival(2, 10, T0, 20_000)],
            T0,
            &shop(),
            None
        ),
        Err(CreditRefused::NotInBlockOrder)
    );
    assert_eq!(book, before);
    assert_eq!(
        book.credit(9, &[], T0, &shop(), None),
        Err(CreditRefused::NoSuchRequest)
    );
}

#[test]
fn rq_15_what_is_still_owed_is_never_below_zero() {
    let mut book = RequestBook::new();
    let n = make_btc(&mut book, 0, 100_000, 7);
    book.credit(n, &[arrival(1, 10, T0, 150_000)], T0, &shop(), None)
        .unwrap();
    let r = book.request(n).unwrap();
    assert_eq!(r.owed(), 0);
    assert_eq!(r.credited(), 150_000);
}

#[test]
fn rq_15_in_the_currency_a_payment_is_credited_at_the_newest_quote_before_its_block() {
    let mut book = RequestBook::new();
    let n = make_usd(&mut book, 0, 25_000, 7); // $250 at $100,000 = 250,000 sats
    book.share(n, T0, Some(&fresh(T0))).unwrap();
    let t1 = T0 + 3_600;
    book.share(n, t1, Some(&rate(2 * RATE, t1))).unwrap();
    // Between the two quotes: the first. 100,000 sats are $100.
    book.credit(n, &[arrival(1, 10, T0 + 1_800, 100_000)], t1, &shop(), None)
        .unwrap();
    // After the second: at $200,000 a bitcoin, 50,000 sats are $100.
    book.credit(
        n,
        &[arrival(2, 11, t1 + 60, 50_000)],
        t1 + 120,
        &shop(),
        None,
    )
    .unwrap();
    let r = book.request(n).unwrap();
    assert_eq!(r.credits[0].value, 10_000);
    assert_eq!(r.credits[0].rate.unwrap().minor_per_btc(), RATE);
    assert_eq!(r.credits[1].value, 10_000);
    assert_eq!(r.credits[1].rate.unwrap().minor_per_btc(), 2 * RATE);
    assert_eq!(r.owed(), 5_000);
    assert!(!r.marks().old_price);
}

#[test]
fn rq_15_a_payment_before_every_quote_is_credited_at_the_newest_and_marked() {
    let mut book = RequestBook::new();
    let n = make_usd(&mut book, 0, 25_000, 7);
    book.share(n, T0 + 1_000, Some(&fresh(T0 + 1_000))).unwrap();
    book.credit(
        n,
        &[arrival(1, 10, T0 + 10, 100_000)],
        T0 + 2_000,
        &shop(),
        None,
    )
    .unwrap();
    let r = book.request(n).unwrap();
    assert_eq!(r.credits[0].value, 10_000);
    assert!(r.credits[0].old_price);
}

#[test]
fn rq_15_with_no_quote_at_all_the_rate_of_now_values_a_payment_or_nothing_is_counted() {
    let mut book = RequestBook::new();
    let n = make_usd(&mut book, 0, 25_000, 7);
    let before = book.clone();
    assert_eq!(
        book.credit(n, &[arrival(1, 10, T0, 100_000)], T0, &shop(), None),
        Err(CreditRefused::NoRate)
    );
    // A rate that is not fresh is no rate.
    assert_eq!(
        book.credit(
            n,
            &[arrival(1, 10, T0, 100_000)],
            T0,
            &shop(),
            Some(&fresh(T0 - 700))
        ),
        Err(CreditRefused::NoRate)
    );
    assert_eq!(book, before);
    book.credit(
        n,
        &[arrival(1, 10, T0, 100_000)],
        T0,
        &shop(),
        Some(&fresh(T0)),
    )
    .unwrap();
    let r = book.request(n).unwrap();
    assert_eq!(r.credits[0].value, 10_000);
    assert!(r.credits[0].old_price);
}

#[test]
fn rq_16_a_request_is_waiting_part_paid_paid_or_overpaid_by_the_excess() {
    let mut book = RequestBook::new();
    let a = make_btc(&mut book, 0, 100_000, 7);
    assert_eq!(status(&book, a), Status::Waiting);
    book.credit(a, &[arrival(1, 10, T0, 40_000)], T0, &shop(), None)
        .unwrap();
    assert_eq!(status(&book, a), Status::PartPaid { owed: 60_000 });
    book.credit(a, &[arrival(2, 11, T0, 60_000)], T0, &shop(), None)
        .unwrap();
    assert_eq!(status(&book, a), Status::Paid);
    book.credit(a, &[arrival(3, 12, T0, 1)], T0, &shop(), None)
        .unwrap();
    // In bitcoin any excess is an overpayment.
    assert_eq!(status(&book, a), Status::Overpaid { excess: 1 });
}

#[test]
fn rq_16_in_the_currency_an_excess_under_the_smallest_sale_is_paid_and_not_overpaid() {
    let mut book = RequestBook::new();
    // $250; the smallest bitcoin sale is 5,000 sats, which is $5.
    let a = make_usd(&mut book, 0, 25_000, 7);
    let b = make_usd(&mut book, 1, 25_000, 7);
    book.share(a, T0, Some(&fresh(T0))).unwrap();
    book.share(b, T0, Some(&fresh(T0))).unwrap();
    // $4 over: Paid.
    book.credit(a, &[arrival(1, 10, T0 + 1, 254_000)], T0, &shop(), None)
        .unwrap();
    assert_eq!(status(&book, a), Status::Paid);
    // $6 over: Overpaid, by 600 cents.
    book.credit(b, &[arrival(2, 10, T0 + 1, 256_000)], T0, &shop(), None)
        .unwrap();
    assert_eq!(status(&book, b), Status::Overpaid { excess: 600 });
}

#[test]
fn rq_16_in_the_currency_a_shortfall_under_the_smallest_sale_is_almost_paid() {
    let mut book = RequestBook::new();
    let a = make_usd(&mut book, 0, 25_000, 7);
    let b = make_usd(&mut book, 1, 25_000, 7);
    book.share(a, T0, Some(&fresh(T0))).unwrap();
    book.share(b, T0, Some(&fresh(T0))).unwrap();
    // $3 short: Almost. $6 short: Part paid.
    book.credit(a, &[arrival(1, 10, T0 + 1, 247_000)], T0, &shop(), None)
        .unwrap();
    assert_eq!(status(&book, a), Status::Almost { short: 300 });
    book.credit(b, &[arrival(2, 10, T0 + 1, 244_000)], T0, &shop(), None)
        .unwrap();
    assert_eq!(status(&book, b), Status::PartPaid { owed: 600 });
    // With no floor set the dust line of 1,000 sats ($1) is the floor.
    assert_eq!(
        book.request(b).unwrap().status(0),
        Status::PartPaid { owed: 600 }
    );
    // $3 is 3,000 sats: over that floor, so it is Part paid.
    assert_eq!(
        book.request(a).unwrap().status(0),
        Status::PartPaid { owed: 300 }
    );
}

#[test]
fn rq_16_counting_an_almost_paid_request_as_paid_is_recorded_and_cannot_be_undone() {
    let mut book = RequestBook::new();
    let a = make_usd(&mut book, 0, 25_000, 7);
    let b = make_btc(&mut book, 1, 100_000, 7);
    book.share(a, T0, Some(&fresh(T0))).unwrap();
    book.credit(a, &[arrival(1, 10, T0 + 1, 247_000)], T0, &shop(), None)
        .unwrap();
    assert_eq!(
        book.count_as_paid(b, T0, &shop()),
        Err(EndRefused::NotAlmost)
    );
    assert_eq!(
        book.count_as_paid(9, T0, &shop()),
        Err(EndRefused::NoSuchRequest)
    );
    book.count_as_paid(a, T0 + 50, &shop()).unwrap();
    assert_eq!(
        book.request(a).unwrap().end,
        Some((End::CountedPaid, T0 + 50))
    );
    assert_eq!(status(&book, a), Status::Paid);
    assert!(!book.request(a).unwrap().is_open());
    // Nothing undoes it.
    assert_eq!(
        book.count_as_paid(a, T0 + 60, &shop()),
        Err(EndRefused::AlreadyEnded)
    );
    assert_eq!(book.cancel(a, T0 + 60), Err(EndRefused::AlreadyEnded));
    assert_eq!(
        book.close(a, T0 + 60, &shop()),
        Err(EndRefused::AlreadyEnded)
    );
    // And it survives the file.
    assert_eq!(RequestBook::from_bytes(&book.to_bytes()).unwrap(), book);
}

#[test]
fn rq_17_a_payment_after_the_end_of_the_due_day_marks_the_request_paid_late() {
    let mut book = RequestBook::new();
    let n = make_btc(&mut book, 0, 100_000, 1); // due the 16th
    let end_of_16th = T0 - 8 * 3600 + 2 * DAY;
    book.credit(
        n,
        &[arrival(1, 10, end_of_16th - 1, 50_000)],
        T0,
        &shop(),
        None,
    )
    .unwrap();
    assert!(!book.request(n).unwrap().marks().late);
    book.credit(n, &[arrival(2, 11, end_of_16th, 50_000)], T0, &shop(), None)
        .unwrap();
    let r = book.request(n).unwrap();
    assert!(r.marks().late);
    // It is counted as any other payment.
    assert_eq!(r.status(5_000), Status::Paid);
    assert!(r.credits[1].late && !r.credits[0].late);
}

#[test]
fn rq_17_the_due_day_is_the_day_of_the_shops_time_zone() {
    let end_of_16th_utc = T0 - 8 * 3600 + 2 * DAY;
    // At UTC+2 the 16th ended two hours earlier; at UTC-2 two hours later.
    for (tz, late) in [(7_200, true), (0, false), (-7_200, false)] {
        let mut book = RequestBook::new();
        let n = book
            .make(
                &bitcoin_request("x", 100_000, in_days(1)),
                0,
                T0,
                &shop_in(tz),
                None,
            )
            .unwrap();
        let at = end_of_16th_utc - 3_600;
        book.credit(n, &[arrival(1, 10, at, 100_000)], T0, &shop_in(tz), None)
            .unwrap();
        assert_eq!(book.request(n).unwrap().marks().late, late, "tz {tz}");
    }
}

#[test]
fn rq_18_a_payment_after_its_quote_ran_out_is_credited_at_it_and_marked_old_price() {
    let mut book = RequestBook::new();
    let n = make_usd(&mut book, 0, 25_000, 7);
    book.share(n, T0, Some(&fresh(T0))).unwrap();
    // One second before the quote ends: not old. At its end: old.
    book.credit(
        n,
        &[arrival(1, 10, T0 + DAY - 1, 100_000)],
        T0 + DAY,
        &shop(),
        None,
    )
    .unwrap();
    assert!(!book.request(n).unwrap().marks().old_price);
    book.credit(
        n,
        &[arrival(2, 11, T0 + DAY, 100_000)],
        T0 + DAY,
        &shop(),
        None,
    )
    .unwrap();
    let r = book.request(n).unwrap();
    assert!(r.marks().old_price);
    assert_eq!(r.credits[1].value, 10_000);
    assert_eq!(r.owed(), 5_000);
    assert!(!r.credits[0].old_price && r.credits[1].old_price);
}

#[test]
fn rq_18_the_owner_can_share_again_for_what_the_rate_of_now_says_is_still_owed() {
    let mut book = RequestBook::new();
    let n = make_usd(&mut book, 0, 25_000, 7);
    book.share(n, T0, Some(&fresh(T0))).unwrap();
    let late = T0 + 2 * DAY;
    book.credit(n, &[arrival(1, 10, late, 100_000)], late, &shop(), None)
        .unwrap();
    assert!(book.request(n).unwrap().marks().old_price);
    // Nothing is decided without the owner: still open, still part paid.
    assert_eq!(status(&book, n), Status::PartPaid { owed: 15_000 });
    let q = book
        .share(n, late + 10, Some(&rate(RATE / 2, late + 10)))
        .unwrap();
    // $150 at $50,000 a bitcoin is 300,000 satoshis.
    assert_eq!((q.owed, q.sats), (15_000, 300_000));
}

#[test]
fn rq_19_a_cancelled_request_stays_in_the_book_and_cannot_be_cancelled_once_paid_into() {
    let mut book = RequestBook::new();
    let a = make_btc(&mut book, 0, 100_000, 7);
    let b = make_btc(&mut book, 1, 100_000, 7);
    book.cancel(a, T0 + 5).unwrap();
    let r = book.request(a).unwrap();
    assert_eq!(r.end, Some((End::Cancelled, T0 + 5)));
    assert_eq!(book.address_indexes(), [0, 1]);
    assert_eq!(book.cancel(a, T0 + 6), Err(EndRefused::AlreadyEnded));
    book.credit(b, &[arrival(1, 10, T0, 10_000)], T0, &shop(), None)
        .unwrap();
    assert_eq!(book.cancel(b, T0 + 6), Err(EndRefused::HasPayments));
    assert_eq!(book.cancel(9, T0), Err(EndRefused::NoSuchRequest));
}

#[test]
fn rq_19_a_payment_after_the_cancel_is_listed_and_offered_for_return() {
    let mut book = RequestBook::new();
    let n = make_btc(&mut book, 0, 100_000, 7);
    book.cancel(n, T0).unwrap();
    book.credit(
        n,
        &[arrival(1, 10, T0 + 100, 60_000)],
        T0 + 200,
        &shop(),
        None,
    )
    .unwrap();
    let r = book.request(n).unwrap();
    assert!(r.marks().after_cancel);
    assert_eq!(r.credits[0].after, Some(After::Cancel));
    assert_eq!(r.credits[0].value, 0);
    assert_eq!(book.returnable_sats(n), Some(60_000));
    // It is not a payment the request counts, and the request is not open.
    assert_eq!(r.credited(), 0);
    assert!(!r.is_open());
    assert_eq!(r.status(5_000), Status::Waiting);
}

#[test]
fn rq_20_close_as_it_is_ends_a_part_paid_request_and_it_leaves_the_open_list() {
    let mut book = RequestBook::new();
    let n = make_btc(&mut book, 0, 100_000, 7);
    assert_eq!(book.close(n, T0, &shop()), Err(EndRefused::NotPartPaid));
    book.credit(n, &[arrival(1, 10, T0, 40_000)], T0, &shop(), None)
        .unwrap();
    assert_eq!(book.list(today()).waiting.len(), 1);
    book.close(n, T0 + 9, &shop()).unwrap();
    let r = book.request(n).unwrap();
    assert_eq!(r.end, Some((End::Closed, T0 + 9)));
    assert_eq!(r.credited(), 40_000);
    assert_eq!(r.owed(), 0);
    assert!(book.list(today()).waiting.is_empty());
    assert_eq!(book.open_count(), 0);
    assert_eq!(
        book.share(n, T0 + 10, None),
        Err(openbmt_requests::ShareRefused::NotOpen)
    );
}

#[test]
fn rq_20_a_payment_after_it_was_closed_is_listed_and_offered_for_return() {
    let mut book = RequestBook::new();
    let n = make_btc(&mut book, 0, 100_000, 7);
    book.credit(n, &[arrival(1, 10, T0, 40_000)], T0, &shop(), None)
        .unwrap();
    book.close(n, T0, &shop()).unwrap();
    book.credit(
        n,
        &[arrival(2, 11, T0 + 50, 25_000)],
        T0 + 60,
        &shop(),
        None,
    )
    .unwrap();
    let r = book.request(n).unwrap();
    assert!(r.marks().after_close && !r.marks().after_cancel);
    assert_eq!(r.credits[1].after, Some(After::Close));
    // The money that arrived before stays, and all of it can be returned.
    assert_eq!(r.credited(), 40_000);
    assert_eq!(book.returnable_sats(n), Some(65_000));
}

#[test]
fn rq_21_the_list_has_overdue_due_soon_and_waiting_each_newest_due_date_last() {
    let mut book = RequestBook::new();
    // Due on the 16th, 17th, 18th, 25th, 26th and 20th of January.
    let due_in = [1, 2, 3, 10, 11, 5];
    for (i, d) in due_in.iter().enumerate() {
        make_btc(&mut book, i as u32, 50_000 + i as u64, *d);
    }
    // On the 17th: the 16th is overdue; the 17th is due today (not
    // overdue), the 18th and the 20th are within 3 days, the rest wait.
    let list = book.list(in_days(2));
    let numbers = |rows: &[openbmt_requests::Row<'_>]| -> Vec<u64> {
        rows.iter().map(|r| r.request.number).collect()
    };
    assert_eq!(numbers(&list.overdue), [1]);
    assert_eq!(numbers(&list.due_soon), [2, 3, 6]);
    assert_eq!(numbers(&list.waiting), [4, 5]);
    assert!(list.ready_to_send.is_empty());
    // Three days is within; four is not.
    let list = book.list(in_days(0));
    assert_eq!(numbers(&list.due_soon), [1, 2, 3]);
    assert_eq!(numbers(&list.waiting), [6, 4, 5]);
}

#[test]
fn rq_21_a_row_shows_the_note_the_number_what_is_still_owed_and_the_due_date() {
    let mut book = RequestBook::new();
    let n = make_btc(&mut book, 0, 100_000, 2);
    book.credit(n, &[arrival(1, 10, T0, 40_000)], T0, &shop(), None)
        .unwrap();
    let list = book.list(today());
    let row = list.due_soon[0];
    assert_eq!(row.request.note, "Booking");
    assert_eq!(row.request.number, 1);
    assert_eq!(row.owed, 60_000);
    assert_eq!(row.request.due, in_days(2));
}

#[test]
fn rq_21_paid_cancelled_and_closed_requests_are_not_in_the_list() {
    let mut book = RequestBook::new();
    let paid = make_btc(&mut book, 0, 100_000, 2);
    let cancelled = make_btc(&mut book, 1, 100_000, 2);
    let closed = make_btc(&mut book, 2, 100_000, 2);
    let open = make_btc(&mut book, 3, 100_000, 2);
    book.credit(paid, &[arrival(1, 10, T0, 100_000)], T0, &shop(), None)
        .unwrap();
    book.cancel(cancelled, T0).unwrap();
    book.credit(closed, &[arrival(2, 10, T0, 10_000)], T0, &shop(), None)
        .unwrap();
    book.close(closed, T0, &shop()).unwrap();
    let list = book.list(today());
    assert_eq!(list.due_soon.len(), 1);
    assert_eq!(list.due_soon[0].request.number, open);
}

#[test]
fn rq_21_two_requests_due_on_one_day_are_in_the_order_of_their_numbers() {
    let mut book = RequestBook::new();
    for i in 0..4 {
        make_btc(&mut book, i, 50_000, 9);
    }
    let list = book.list(today());
    let numbers: Vec<u64> = list.waiting.iter().map(|r| r.request.number).collect();
    assert_eq!(numbers, [1, 2, 3, 4]);
}

#[test]
fn rq_22_home_counts_the_overdue_requests_and_send_again_makes_a_fresh_quote() {
    let mut book = RequestBook::new();
    let a = make_btc(&mut book, 0, 100_000, 1);
    make_btc(&mut book, 1, 100_000, 30);
    book.credit(a, &[arrival(1, 10, T0, 25_000)], T0, &shop(), None)
        .unwrap();
    assert_eq!(book.overdue_count(today()), 0);
    assert_eq!(book.overdue_count(in_days(2)), 1);
    assert_eq!(overdue_row(1), "1 overdue");
    let t = T0 + 2 * DAY;
    let q = book.share(a, t, None).unwrap();
    assert_eq!((q.owed, q.made_at), (75_000, t));
    assert_eq!(
        overdue_line("Dunmore Books", "75,000 sats", 1, "16 January 2027"),
        "Dunmore Books still asks for 75,000 sats. Request No. 1 was due 16 January 2027."
    );
}

#[test]
fn rq_22_the_library_sets_no_alarm_and_starts_no_job() {
    // It keeps no clock and no thread: every call is given its time.
    let src = include_str!("../src/book.rs");
    for not in ["SystemTime", "Instant", "thread::spawn", "std::process"] {
        assert!(!src.contains(not), "{not}");
    }
}

#[test]
fn rq_23_the_watcher_holds_no_request_and_a_payment_is_counted_when_the_app_follows_the_chain() {
    // The library depends on no watcher or heartbeat part; a payment is in
    // the book only after `credit`, and not before.
    let manifest = include_str!("../Cargo.toml");
    for not in ["openbmt-approver", "openbmt-owner-node", "openbmt-link"] {
        assert!(!manifest.contains(not), "{not}");
    }
    let mut book = RequestBook::new();
    let n = make_btc(&mut book, 0, 100_000, 7);
    assert!(book.request(n).unwrap().credits.is_empty());
    book.credit(
        n,
        &[arrival(1, 10, T0, 100_000)],
        T0 + 10_000,
        &shop(),
        None,
    )
    .unwrap();
    assert_eq!(status(&book, n), Status::Paid);
}

#[test]
fn rq_16_the_words_of_each_state_and_mark_come_from_the_table() {
    let amount = |n: u64| format!("{n} sats");
    assert_eq!(
        state_line(&Status::PartPaid { owed: 60_000 }, &amount),
        "60000 sats still owed"
    );
    let marks = Marks {
        late: true,
        ..Marks::default()
    };
    assert_eq!(mark_lines(&marks), ["Paid late"]);
    let _: Option<Shop> = None;
}

#[test]
fn rq_15_an_output_of_zero_satoshis_is_not_a_payment_and_the_book_stays_readable() {
    let mut book = RequestBook::new();
    let n = make_btc(&mut book, 0, 100_000, 7);
    let counted = book
        .credit(n, &[arrival(1, 10, T0, 0)], T0, &shop(), None)
        .unwrap();
    assert_eq!(counted, 0);
    assert!(book.request(n).unwrap().credits.is_empty());
    assert_eq!(book.request(n).unwrap().status(5_000), Status::Waiting);
    assert_eq!(RequestBook::from_bytes(&book.to_bytes()).unwrap(), book);
    // Mixed with a real payment, only the real one is recorded.
    let counted = book
        .credit(
            n,
            &[arrival(2, 11, T0, 0), arrival(3, 11, T0, 40_000)],
            T0,
            &shop(),
            None,
        )
        .unwrap();
    assert_eq!(counted, 1);
    assert_eq!(book.request(n).unwrap().credits.len(), 1);
    assert_eq!(RequestBook::from_bytes(&book.to_bytes()).unwrap(), book);
}

#[test]
fn rq_15_a_zero_output_after_a_cancel_or_a_close_is_not_recorded_either() {
    let mut book = RequestBook::new();
    let a = make_btc(&mut book, 0, 100_000, 7);
    book.cancel(a, T0).unwrap();
    let b = make_btc(&mut book, 1, 100_000, 7);
    book.credit(b, &[arrival(5, 10, T0, 30_000)], T0, &shop(), None)
        .unwrap();
    book.close(b, T0 + 1, &shop()).unwrap();
    for n in [a, b] {
        let before = book.request(n).unwrap().credits.len();
        assert_eq!(
            book.credit(n, &[arrival(9, 12, T0 + 9, 0)], T0 + 9, &shop(), None),
            Ok(0)
        );
        assert_eq!(book.request(n).unwrap().credits.len(), before);
    }
    assert_eq!(RequestBook::from_bytes(&book.to_bytes()).unwrap(), book);
}
