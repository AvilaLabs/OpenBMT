//! Section 27: a sale from the till (CR-266 to CR-271).

use super::kit::*;
use crate::say::all_words;
use crate::{
    Amount, Event, TillCancelled, TillDropped, TillRequest, TillStartRefused, TillWaiting,
    MAX_TILL_WAITING,
};

const SALE: [u8; 16] = [7; 16];

fn request(sale: u8, cents: u64, lapses_at: u64) -> TillRequest {
    TillRequest {
        sale: [sale; 16],
        amount: Amount(cents),
        currency: usd(),
        reference: "POS/0001".to_string(),
        lapses_at,
    }
}

impl World {
    fn till_request(&mut self, sale: u8, cents: u64) -> Result<(), TillDropped> {
        let lapses = self.now + 300;
        self.c.till_take(request(sale, cents, lapses), self.now)
    }

    fn till_start(&mut self, sale: u8) -> Result<crate::Opened, TillStartRefused<&'static str>> {
        let facts = self.open_facts();
        self.c.till_start(
            &[sale; 16],
            rate(),
            &fresh(facts.now),
            facts,
            None,
            &mut self.m,
        )
    }
}

#[test]
fn cr_266_a_request_with_the_shops_currency_a_good_amount_and_time_waits() {
    let mut w = World::new();
    w.till_request(1, 500).unwrap();
    let shown = w.c.till_waiting(w.now);
    assert_eq!(shown.len(), 1);
    assert_eq!(shown[0].amount, Amount(500));
    assert_eq!(shown[0].reference, "POS/0001");
}

#[test]
fn cr_266_a_request_that_does_not_fit_is_dropped_with_its_reason() {
    let mut w = World::new();
    let mut other = request(1, 500, w.now + 300);
    other.currency = crate::Currency::new(*b"EUR").unwrap();
    assert_eq!(w.c.till_take(other, w.now), Err(TillDropped::OtherCurrency));
    assert_eq!(w.till_request(1, 0), Err(TillDropped::BadAmount));
    assert_eq!(
        w.till_request(1, crate::MAX_BILL_AMOUNT + 1),
        Err(TillDropped::BadAmount)
    );
    let mut long = request(1, 500, w.now + 300);
    long.reference = "x".repeat(41);
    assert_eq!(w.c.till_take(long, w.now), Err(TillDropped::BadReference));
    let mut high = request(1, 500, w.now + 300);
    high.reference = "caf\u{e9}".to_string();
    assert_eq!(w.c.till_take(high, w.now), Err(TillDropped::BadReference));
    // A lapse time that is now or past.
    assert_eq!(
        w.c.till_take(request(1, 500, w.now), w.now),
        Err(TillDropped::Lapsed)
    );
    assert_eq!(
        w.c.till_take(request(1, 500, w.now - 1), w.now),
        Err(TillDropped::Lapsed)
    );
    assert!(w.c.till_waiting(w.now).is_empty());
    assert!(w.m.records.is_empty(), "nothing is written for a drop");
}

#[test]
fn cr_266_the_largest_amount_is_taken_and_one_more_is_not() {
    let mut w = World::new();
    w.till_request(1, crate::MAX_BILL_AMOUNT).unwrap();
    assert_eq!(
        w.till_request(2, crate::MAX_BILL_AMOUNT + 1),
        Err(TillDropped::BadAmount)
    );
}

#[test]
fn cr_266_at_most_eight_wait_and_a_ninth_is_dropped() {
    let mut w = World::new();
    for n in 0..MAX_TILL_WAITING as u8 {
        w.till_request(n + 1, 100).unwrap();
    }
    assert_eq!(w.till_request(100, 100), Err(TillDropped::Full));
    assert_eq!(w.c.till_waiting(w.now).len(), MAX_TILL_WAITING);
    // A lapsed request leaves its room.
    w.tick(301);
    assert_eq!(w.c.till_waiting(w.now).len(), 0);
    w.till_request(100, 100).unwrap();
}

#[test]
fn cr_266_the_same_sale_is_not_taken_twice() {
    let mut w = World::new();
    w.till_request(1, 500).unwrap();
    assert_eq!(w.till_request(1, 500), Err(TillDropped::Known));
    w.till_start(1).unwrap();
    // Nor after it started.
    assert_eq!(w.till_request(1, 500), Err(TillDropped::Known));
}

#[test]
fn cr_267_the_oldest_request_that_has_not_lapsed_is_shown_first() {
    let mut w = World::new();
    w.c.till_take(request(1, 100, w.now + 60), w.now).unwrap();
    w.c.till_take(request(2, 200, w.now + 600), w.now).unwrap();
    w.c.till_take(request(3, 300, w.now + 600), w.now).unwrap();
    let shown: Vec<u64> = w.c.till_waiting(w.now).iter().map(|r| r.amount.0).collect();
    assert_eq!(shown, vec![100, 200, 300]);
    w.tick(61);
    let shown: Vec<u64> = w.c.till_waiting(w.now).iter().map(|r| r.amount.0).collect();
    assert_eq!(shown, vec![200, 300]);
}

#[test]
fn cr_267_minutes_left_count_up_and_are_zero_when_lapsed() {
    let r = request(1, 100, 1_000);
    assert_eq!(r.minutes_left(1_000 - 300), 5);
    assert_eq!(r.minutes_left(1_000 - 299), 5);
    assert_eq!(r.minutes_left(1_000 - 61), 2);
    assert_eq!(r.minutes_left(1_000 - 60), 1);
    assert_eq!(r.minutes_left(1_000 - 1), 1);
    assert_eq!(r.minutes_left(1_000), 0);
    assert_eq!(r.minutes_left(2_000), 0);
}

#[test]
fn cr_267_not_now_hides_the_request_and_writes_nothing() {
    let mut w = World::new();
    w.till_request(1, 500).unwrap();
    w.till_request(2, 600).unwrap();
    assert!(w.c.till_not_now(&[1; 16]));
    let shown: Vec<u64> = w.c.till_waiting(w.now).iter().map(|r| r.amount.0).collect();
    assert_eq!(shown, vec![600]);
    assert!(w.m.records.is_empty());
    assert!(!w.c.till_not_now(&[9; 16]));
}

#[test]
fn cr_268_start_opens_an_ordinary_sale_for_the_amount_and_names_the_till_sale() {
    let mut w = World::new();
    w.till_request(7, 500).unwrap();
    let o = w.till_start(7).unwrap();
    // The sale is the amount of the request at the counter's own rate.
    let plain = {
        let mut other = World::new();
        other.open(500)
    };
    assert_eq!(o.needs_sats, plain.needs_sats);
    assert_eq!(o.index, plain.index);
    assert_eq!(w.c.sale_till(o.index), Some(&SALE));
    // The request is gone from the list.
    assert!(w.c.till_waiting(w.now).is_empty());
    // The records: the till's sale, then Opened, then the reference.
    let events = w.m.events();
    assert_eq!(
        events[0],
        Event::TillSale {
            index: o.index,
            at: w.now,
            sale: SALE
        }
    );
    assert!(matches!(events[1], Event::Opened { .. }));
    assert!(matches!(events[2], Event::Referenced { .. }));
    assert_eq!(w.c.sale_reference(o.index), Some("POS/0001"));
}

#[test]
fn cr_268_the_till_sale_is_stored_before_the_sale_is_given_back() {
    let mut w = World::new();
    w.till_request(7, 500).unwrap();
    let o = w.till_start(7).unwrap();
    // A start from the stored state names the same till sale.
    let again = w.m.reload();
    assert_eq!(again.sale_till(o.index), Some(&SALE));
    // A write that fails before the record is stored gives no sale and the
    // request keeps waiting.
    let mut w = World::new();
    w.till_request(7, 500).unwrap();
    w.m.reset();
    w.m.fail_all = true;
    assert!(matches!(
        w.till_start(7),
        Err(TillStartRefused::Open(
            crate::OpenRefused::AddressNotStored(_)
        )) | Err(TillStartRefused::Open(crate::OpenRefused::Store(_)))
    ));
    w.m.reset();
    assert_eq!(w.c.till_waiting(w.now).len(), 1);
}

#[test]
fn cr_268_a_reference_that_fits_becomes_the_sales_and_one_that_does_not_is_only_shown() {
    let mut w = World::new();
    let mut spaced = request(1, 500, w.now + 300);
    spaced.reference = "Table 4, tab 2".to_string();
    w.c.till_take(spaced, w.now).unwrap();
    assert_eq!(
        w.c.till_waiting(w.now)[0].reference,
        "Table 4, tab 2",
        "it is shown"
    );
    let o = w.till_start(1).unwrap();
    assert_eq!(w.c.sale_reference(o.index), None);
    assert!(w
        .m
        .events()
        .iter()
        .all(|e| !matches!(e, Event::Referenced { .. })));

    let mut empty = request(2, 500, w.now + 300);
    empty.reference = String::new();
    w.c.till_take(empty, w.now).unwrap();
    let o = w.till_start(2).unwrap();
    assert_eq!(w.c.sale_reference(o.index), None);

    // Forty characters are taken from the till but a sale's reference is 32.
    let mut long = request(3, 500, w.now + 300);
    long.reference = "a".repeat(40);
    w.c.till_take(long, w.now).unwrap();
    let o = w.till_start(3).unwrap();
    assert_eq!(w.c.sale_reference(o.index), None);

    let mut fits = request(4, 500, w.now + 300);
    fits.reference = "a".repeat(32);
    w.c.till_take(fits, w.now).unwrap();
    let o = w.till_start(4).unwrap();
    assert_eq!(w.c.sale_reference(o.index), Some("a".repeat(32).as_str()));
}

#[test]
fn cr_268_a_start_for_a_request_that_is_not_there_or_has_lapsed_opens_nothing() {
    let mut w = World::new();
    assert_eq!(w.till_start(1).unwrap_err(), TillStartRefused::NotWaiting);
    w.till_request(1, 500).unwrap();
    w.tick(301);
    assert_eq!(w.till_start(1).unwrap_err(), TillStartRefused::NotWaiting);
    assert!(w.m.records.is_empty());
}

#[test]
fn cr_268_a_start_the_counter_refuses_leaves_the_request_waiting() {
    let mut w = World::new();
    w.till_request(1, 5).unwrap();
    // Five cents is below the dust line at the test rate.
    let refused = w.till_start(1).unwrap_err();
    assert!(
        matches!(
            refused,
            TillStartRefused::Open(crate::OpenRefused::BelowDust { .. })
        ),
        "{refused}"
    );
    assert_eq!(refused.rule(), "CR-17");
    assert_eq!(w.c.till_waiting(w.now).len(), 1);
    assert!(w.m.events().is_empty());
}

#[test]
fn cr_268_a_sale_that_came_from_the_till_is_not_a_plain_sale() {
    let mut w = World::new();
    let plain = w.open(500);
    assert_eq!(w.c.sale_till(plain.index), None);
    assert!(!w.c.till_cancelled(plain.index));
}

#[test]
fn cr_269_a_cancel_for_a_request_not_started_removes_it() {
    let mut w = World::new();
    w.till_request(1, 500).unwrap();
    assert_eq!(w.c.till_cancel(&[1; 16]), TillCancelled::Removed);
    assert!(w.c.till_waiting(w.now).is_empty());
    assert_eq!(w.till_start(1).unwrap_err(), TillStartRefused::NotWaiting);
    assert!(w.m.records.is_empty());
    // A request hidden by Not now is removed too.
    w.till_request(2, 500).unwrap();
    w.c.till_not_now(&[2; 16]);
    assert_eq!(w.c.till_cancel(&[2; 16]), TillCancelled::Removed);
    assert_eq!(w.c.till_cancel(&[2; 16]), TillCancelled::Unknown);
}

#[test]
fn cr_269_a_cancel_for_a_started_sale_marks_it_and_does_not_end_it() {
    let mut w = World::new();
    w.till_request(1, 500).unwrap();
    let o = w.till_start(1).unwrap();
    let writes = w.m.records.len();
    assert_eq!(w.c.till_cancel(&[1; 16]), TillCancelled::Marked(o.index));
    assert!(w.c.till_cancelled(o.index));
    assert_eq!(w.m.records.len(), writes, "nothing is written");
    // The sale goes on: it is still open, and a payment is a payment.
    assert_eq!(w.judge(o.index).state, crate::State::NotPaidYet);
    let t = w.pay(&o, o.needs_sats);
    w.confirm(&[t], 100);
    assert_eq!(w.judge(o.index).state, crate::State::Confirmed);
    assert!(w.c.till_cancelled(o.index));
}

#[test]
fn cr_269_the_cashier_ends_a_sale_the_till_cancelled_as_any_sale_is_ended() {
    let mut w = World::new();
    w.till_request(1, 500).unwrap();
    let o = w.till_start(1).unwrap();
    w.c.till_cancel(&[1; 16]);
    let now = w.now;
    w.c.cancel(o.index, now, &mut w.m).unwrap();
    assert_eq!(w.judge(o.index).state, crate::State::Cancelled);
}

#[test]
fn cr_269_a_cancel_for_a_sale_nobody_knows_does_nothing() {
    let mut w = World::new();
    assert_eq!(w.c.till_cancel(&[5; 16]), TillCancelled::Unknown);
    assert!(w.m.records.is_empty());
}

#[test]
fn cr_270_a_restart_forgets_the_waiting_requests_and_the_cancel_but_not_the_till_sale() {
    let mut w = World::new();
    w.till_request(1, 500).unwrap();
    w.till_request(2, 600).unwrap();
    let o = w.till_start(1).unwrap();
    w.c.till_cancel(&[1; 16]);
    w.restart();
    assert!(w.c.till_waiting(w.now).is_empty());
    assert!(!w.c.till_cancelled(o.index));
    assert_eq!(w.c.sale_till(o.index), Some(&[1; 16]));
    // The sale goes on naming it: a cancel after the restart finds it.
    assert_eq!(w.c.till_cancel(&[1; 16]), TillCancelled::Marked(o.index));
    // And the same sale is not taken again.
    assert_eq!(w.till_request(1, 500), Err(TillDropped::Known));
}

#[test]
fn cr_268_the_record_of_the_till_sale_is_tag_38_and_reads_strictly() {
    let e = Event::TillSale {
        index: 3,
        at: 9,
        sale: SALE,
    };
    let body = e.encode();
    assert_eq!(body.len(), 1 + 4 + 8 + 16);
    assert_eq!(body[0], 38);
    assert_eq!(Event::decode(&body), Ok(e.clone()));
    assert_eq!(e.kind(), openbmt_core::records::RecordKind::Sale);
    assert_eq!(e.index(), 3);
    assert_eq!(e.at(), 9);
    for n in 0..body.len() {
        assert!(Event::decode(&body[..n]).is_err(), "cut at {n}");
    }
    let mut longer = body.clone();
    longer.push(0);
    assert!(Event::decode(&longer).is_err());
}

#[test]
fn cr_270_the_waiting_list_is_a_value_that_keeps_its_order() {
    let mut list = TillWaiting::new();
    assert!(list.is_empty());
    for n in 1..=3u8 {
        list.take(request(n, 100, 1_000), usd(), false, 0).unwrap();
    }
    assert_eq!(list.len(), 3);
    assert!(list.remove(&[2; 16]));
    let order: Vec<u8> = list.shown(0).iter().map(|r| r.sale[0]).collect();
    assert_eq!(order, vec![1, 3]);
}

#[test]
fn cr_271_every_state_and_refusal_of_the_section_has_words_and_names_its_rule() {
    let all = all_words();
    let till: Vec<_> = all.iter().filter(|(n, _)| n.starts_with("Till")).collect();
    assert!(till.len() >= 9, "{}", till.len());
    let mut seen = std::collections::BTreeSet::new();
    for (name, w) in till {
        assert!(!w.heading.starts_with("WORDS_NEEDED"), "{name}");
        assert_eq!(openbmt_core::say::check(w), Ok(()), "{name}");
        assert!(seen.insert(w.heading.clone()), "{name} repeats");
        assert!(w.code.is_some_and(|c| c.starts_with("CR-26")), "{name}");
    }
}
