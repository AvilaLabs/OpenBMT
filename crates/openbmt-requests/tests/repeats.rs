//! RQ-24, RQ-25: repeats.

mod common;

use common::*;
use openbmt_requests::{Day, EndRefused, Every, RepeatRefused, RequestBook};

const TZ: i32 = 0;

fn series_book(first_due: Day, every: Every, now: u64) -> RequestBook {
    let mut book = RequestBook::new();
    let new = repeating(bitcoin_request("Stall fee", 50_000, first_due), every);
    book.make(&new, 0, now, &shop(), None).unwrap();
    book
}

/// Make everything due on `today`, with the next indexes from `next_index`.
fn run(book: &mut RequestBook, today: Day, now: u64, next_index: &mut u32) -> Vec<u64> {
    let due = book.repeats_due(today);
    let mut made = Vec::new();
    for d in &due {
        made.push(book.make_repeat(d, *next_index, now, TZ).unwrap());
        *next_index += 1;
    }
    made
}

#[test]
fn rq_24_a_request_may_repeat_every_week_every_month_or_every_year_from_its_due_date() {
    let first = day(2027, 2, 15);
    assert_eq!(Every::Week.due(first, 0), Some(first));
    assert_eq!(Every::Week.due(first, 1), Some(day(2027, 2, 22)));
    assert_eq!(Every::Week.due(first, 2), Some(day(2027, 3, 1)));
    assert_eq!(Every::Month.due(first, 1), Some(day(2027, 3, 15)));
    assert_eq!(Every::Month.due(first, 11), Some(day(2028, 1, 15)));
    assert_eq!(Every::Year.due(first, 1), Some(day(2028, 2, 15)));
    assert_eq!(Every::Year.due(first, 3), Some(day(2030, 2, 15)));
}

#[test]
fn rq_24_a_month_that_has_no_such_day_uses_its_last_day_and_the_series_keeps_its_day() {
    let first = day(2027, 1, 31);
    let due: Vec<_> = (0..5)
        .map(|k| Every::Month.due(first, k).unwrap().to_string())
        .collect();
    assert_eq!(
        due,
        [
            "2027-01-31",
            "2027-02-28",
            "2027-03-31",
            "2027-04-30",
            "2027-05-31"
        ]
    );
    let leap = day(2028, 1, 31);
    assert_eq!(Every::Month.due(leap, 1), Some(day(2028, 2, 29)));
    // A year from 29 February.
    assert_eq!(Every::Year.due(day(2028, 2, 29), 1), Some(day(2029, 2, 28)));
    assert_eq!(Every::Year.due(day(2028, 2, 29), 4), Some(day(2032, 2, 29)));
}

#[test]
fn rq_24_a_repeat_is_recorded_on_the_request_with_its_series() {
    let book = series_book(in_days(10), Every::Month, T0);
    let r = book.request(1).unwrap().repeat.unwrap();
    assert_eq!(
        (r.every, r.series, r.index, r.first_due),
        (Every::Month, 1, 0, in_days(10))
    );
    assert_eq!(r.lead, 10);
}

#[test]
fn rq_25_the_next_request_is_made_on_or_after_its_due_date_less_the_lead() {
    // Made on the 15th, due on the 25th: a lead of 10 days. A month on, the
    // next is due 25 February, so it is made from 15 February.
    let mut book = series_book(day(2027, 1, 25), Every::Month, T0);
    assert!(book.repeats_due(day(2027, 2, 14)).is_empty());
    let due = book.repeats_due(day(2027, 2, 15));
    assert_eq!(due.len(), 1);
    assert_eq!(
        (due[0].series, due[0].index, due[0].due),
        (1, 1, day(2027, 2, 25))
    );
    let n = book.make_repeat(&due[0], 1, T0 + 31 * DAY, TZ).unwrap();
    assert_eq!(n, 2);
    let r = book.request(2).unwrap();
    // A new number, a new address, the same note, amount and pricing, the next due date.
    assert_eq!((r.number, r.address_index), (2, 1));
    assert_eq!((r.note.as_str(), r.amount), ("Stall fee", 50_000));
    assert_eq!(r.pricing, book.request(1).unwrap().pricing);
    assert_eq!(r.due, day(2027, 2, 25));
    assert_eq!(r.repeat.unwrap().index, 1);
    assert_eq!(r.repeat.unwrap().series, 1);
    // And the one after is not due yet.
    assert!(book.repeats_due(day(2027, 2, 15)).is_empty());
}

#[test]
fn rq_25_the_lead_is_the_due_date_less_the_day_made_and_at_most_31_days() {
    let book = series_book(in_days(100), Every::Year, T0);
    assert_eq!(book.request(1).unwrap().repeat.unwrap().lead, 31);
    let book = series_book(in_days(0), Every::Week, T0);
    assert_eq!(book.request(1).unwrap().repeat.unwrap().lead, 0);
    // A lead of 0: the next is made on its due date, not before.
    assert!(book.repeats_due(in_days(6)).is_empty());
    assert_eq!(book.repeats_due(in_days(7)).len(), 1);
}

#[test]
fn rq_25_the_new_request_is_ready_to_send_until_it_is_shared_once() {
    let mut book = series_book(in_days(5), Every::Week, T0);
    let mut index = 1;
    // The first request is not "ready to send": it was made by hand.
    assert!(book.list(today()).ready_to_send.is_empty());
    let made = run(&mut book, in_days(7), T0 + 7 * DAY, &mut index);
    assert_eq!(made, [2]);
    let list = book.list(in_days(7));
    assert_eq!(list.ready_to_send.len(), 1);
    assert_eq!(list.ready_to_send[0].request.number, 2);
    assert!(!list.waiting.iter().any(|r| r.request.number == 2));
    book.share(2, T0 + 8 * DAY, None).unwrap();
    let list = book.list(in_days(7));
    assert!(list.ready_to_send.is_empty());
    assert!(list
        .overdue
        .iter()
        .chain(&list.due_soon)
        .chain(&list.waiting)
        .any(|r| r.request.number == 2));
}

#[test]
fn rq_25_requests_missed_while_the_app_was_closed_are_all_made_oldest_first() {
    let mut book = series_book(in_days(0), Every::Week, T0);
    let mut index = 1;
    // Opened 5 weeks later: weeks 1 to 5 are due.
    let made = run(&mut book, in_days(35), T0 + 35 * DAY, &mut index);
    assert_eq!(made, [2, 3, 4, 5, 6]);
    let dues: Vec<_> = made.iter().map(|n| book.request(*n).unwrap().due).collect();
    assert_eq!(
        dues,
        [
            in_days(7),
            in_days(14),
            in_days(21),
            in_days(28),
            in_days(35)
        ]
    );
    // Numbers and addresses are in the same order.
    let indexes: Vec<_> = made
        .iter()
        .map(|n| book.request(*n).unwrap().address_index)
        .collect();
    assert_eq!(indexes, [1, 2, 3, 4, 5]);
}

#[test]
fn rq_25_at_most_12_are_made_at_a_time() {
    let mut book = series_book(in_days(0), Every::Week, T0);
    let mut index = 1;
    // 30 weeks later 30 are missed: 12, then 12, then 6.
    let today = in_days(210);
    let first = run(&mut book, today, T0 + 210 * DAY, &mut index);
    assert_eq!(first.len(), 12);
    assert_eq!(book.request(13).unwrap().due, in_days(84));
    assert_eq!(run(&mut book, today, T0 + 210 * DAY, &mut index).len(), 12);
    assert_eq!(run(&mut book, today, T0 + 210 * DAY, &mut index).len(), 6);
    assert!(run(&mut book, today, T0 + 210 * DAY, &mut index).is_empty());
    assert_eq!(book.request(31).unwrap().due, in_days(210));
}

#[test]
fn rq_25_twelve_at_a_time_is_across_all_series_oldest_first() {
    let mut book = RequestBook::new();
    let a = repeating(bitcoin_request("A", 50_000, in_days(0)), Every::Week);
    let b = repeating(bitcoin_request("B", 50_000, in_days(3)), Every::Week);
    book.make(&a, 0, T0, &shop(), None).unwrap();
    book.make(&b, 1, T0, &shop(), None).unwrap();
    let due = book.repeats_due(in_days(70));
    assert_eq!(due.len(), 12);
    let order: Vec<(u64, u32)> = due.iter().map(|d| (d.series, d.index)).collect();
    // A's weeks fall on 7, 14 ...; B's on 10, 17 ...: they interleave.
    assert_eq!(&order[..4], [(1, 1), (2, 1), (1, 2), (2, 2)]);
    assert!(due.windows(2).all(|w| w[0].due <= w[1].due));
}

#[test]
fn rq_25_stop_repeating_on_any_of_them_stops_the_series_and_the_requests_made_stay() {
    let mut book = series_book(in_days(0), Every::Week, T0);
    let mut index = 1;
    run(&mut book, in_days(14), T0 + 14 * DAY, &mut index);
    assert_eq!(book.requests().len(), 3);
    // Stopped from the second request.
    book.stop_repeating(2).unwrap();
    assert!(book.is_stopped(1));
    assert!(book.repeats_due(in_days(70)).is_empty());
    assert_eq!(book.requests().len(), 3);
    assert!(book.requests().iter().all(|r| r.is_open()));
    // Stopping twice changes nothing; a request that does not repeat has nothing to stop.
    let before = book.clone();
    book.stop_repeating(3).unwrap();
    assert_eq!(book, before);
    let n = make_btc(&mut book, 50, 50_000, 3);
    assert_eq!(book.stop_repeating(n), Err(EndRefused::NoRepeat));
    assert_eq!(book.stop_repeating(99), Err(EndRefused::NoSuchRequest));
}

#[test]
fn rq_25_a_series_that_was_stopped_makes_no_request_even_from_an_old_list() {
    let mut book = series_book(in_days(0), Every::Week, T0);
    let due = book.repeats_due(in_days(7));
    book.stop_repeating(1).unwrap();
    assert_eq!(
        book.make_repeat(&due[0], 1, T0, TZ),
        Err(RepeatRefused::NotDue)
    );
}

#[test]
fn rq_25_a_repeat_is_made_once_and_in_order() {
    let mut book = series_book(in_days(0), Every::Week, T0);
    let due = book.repeats_due(in_days(14));
    assert_eq!(due.len(), 2);
    // The second before the first is refused; the first twice is refused.
    assert_eq!(
        book.make_repeat(&due[1], 1, T0, TZ),
        Err(RepeatRefused::NotDue)
    );
    book.make_repeat(&due[0], 1, T0, TZ).unwrap();
    assert_eq!(
        book.make_repeat(&due[0], 2, T0, TZ),
        Err(RepeatRefused::NotDue)
    );
    book.make_repeat(&due[1], 2, T0, TZ).unwrap();
    // An index in use is refused, whatever else is right.
    let due = book.repeats_due(in_days(21));
    assert_eq!(
        book.make_repeat(&due[0], 2, T0, TZ),
        Err(RepeatRefused::IndexUsed)
    );
}

#[test]
fn rq_25_a_repeat_in_the_currency_keeps_its_pricing_and_a_month_end_series_its_day() {
    let mut book = RequestBook::new();
    let new = repeating(usd_request("Rent", 90_000, day(2027, 1, 31)), Every::Month);
    book.make(&new, 0, T0, &shop(), Some(&fresh(T0))).unwrap();
    let mut index = 1;
    let made = run(&mut book, day(2027, 4, 30), T0 + 100 * DAY, &mut index);
    let dues: Vec<_> = made
        .iter()
        .map(|n| book.request(*n).unwrap().due.to_string())
        .collect();
    assert_eq!(dues, ["2027-02-28", "2027-03-31", "2027-04-30"]);
    for n in made {
        let r = book.request(n).unwrap();
        assert_eq!(r.pricing, book.request(1).unwrap().pricing);
        assert_eq!(r.amount, 90_000);
        assert!(r.quotes.is_empty());
    }
}

#[test]
fn rq_25_the_new_request_is_refused_when_1000_are_open() {
    let mut book = series_book(in_days(0), Every::Week, T0);
    for i in 1..1_000 {
        make_btc(&mut book, i, 50_000, 7);
    }
    let due = book.repeats_due(in_days(7));
    assert_eq!(
        book.make_repeat(&due[0], 5_000, T0, TZ),
        Err(RepeatRefused::TooManyOpen)
    );
    book.cancel(2, T0).unwrap();
    assert!(book.make_repeat(&due[0], 5_000, T0, TZ).is_ok());
}
