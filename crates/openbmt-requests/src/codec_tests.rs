//! RQ-30, RQ-34: the reader refuses what the rules of the book forbid, one
//! rule at a time. These make a book that breaks one rule and write it as
//! bytes, which only code inside the crate can do.

use std::str::FromStr;

use bitcoin::{Address, ScriptBuf};
use openbmt_core::currency::ShopCurrency;
use openbmt_counter::money::{Rate, RateSource, MAX_SATS};

use crate::*;

const T0: u64 = 1_800_000_000;
const RATE: u64 = 10_000_000;

fn shop() -> Shop {
    Shop {
        currency: ShopCurrency::Usd,
        min_sale_sats: 5_000,
        tz_offset_secs: 0,
    }
}

fn fresh() -> CheckedRate {
    CheckedRate {
        rate: Rate::new(RATE).unwrap(),
        source: RateSource::new("test", T0).unwrap(),
    }
}

fn script() -> ScriptBuf {
    Address::from_str("bc1qw508d6qejxtdg4y5r3zarvary0c5xw7kv8f3t4")
        .unwrap()
        .assume_checked()
        .script_pubkey()
}

fn new(note: &str, amount: u64, usd: bool, every: Option<Every>) -> NewRequest {
    NewRequest {
        note: note.to_string(),
        amount,
        price_in: if usd {
            PriceIn::ShopCurrency
        } else {
            PriceIn::Bitcoin
        },
        due: Day(day_of(T0, 0).0 + 7),
        repeat: every,
    }
}

fn arrival(n: u8, height: u32, block_time: u64, sats: u64) -> Arrival {
    Arrival {
        txid: [n; 32],
        vout: 0,
        height,
        block_time,
        sats,
    }
}

/// A book of nine requests: part paid with returns, in the currency with
/// quotes, cancelled and paid after, closed, a series that was stopped.
fn book() -> RequestBook {
    let mut b = RequestBook::new();
    let s = shop();
    b.make(&new("one", 100_000, false, None), 0, T0, &s, None)
        .unwrap();
    b.credit(1, &[arrival(1, 10, T0, 130_000)], T0, &s, None)
        .unwrap();
    b.queue_return(1, ReturnAmount::Sats(5_000), script(), T0, None)
        .unwrap();
    b.mark_returns_sent(&[1], [3; 32]);
    b.make(&new("two", 25_000, true, None), 1, T0, &s, Some(&fresh()))
        .unwrap();
    b.share(2, T0, Some(&fresh())).unwrap();
    b.credit(2, &[arrival(2, 11, T0 + 5, 60_000)], T0 + 9, &s, None)
        .unwrap();
    b.make(&new("three", 50_000, false, None), 2, T0, &s, None)
        .unwrap();
    b.cancel(3, T0).unwrap();
    b.credit(3, &[arrival(3, 12, T0 + 5, 9_000)], T0 + 9, &s, None)
        .unwrap();
    b.make(&new("four", 50_000, false, None), 3, T0, &s, None)
        .unwrap();
    b.credit(4, &[arrival(4, 13, T0 + 5, 9_000)], T0 + 9, &s, None)
        .unwrap();
    b.close(4, T0 + 10, &s).unwrap();
    b.credit(4, &[arrival(5, 14, T0 + 20, 2_000)], T0 + 30, &s, None)
        .unwrap();
    b.make(
        &new("five", 50_000, false, Some(Every::Week)),
        4,
        T0,
        &s,
        None,
    )
    .unwrap();
    for (i, d) in b.repeats_due(Day(day_of(T0, 0).0 + 14)).iter().enumerate() {
        b.make_repeat(d, 5 + i as u32, T0, 0).unwrap();
    }
    b.stop_repeating(5).unwrap();
    b
}

#[track_caller]
fn refused(change: impl FnOnce(&mut RequestBook)) {
    let mut b = book();
    change(&mut b);
    assert!(RequestBook::from_bytes(&b.to_bytes()).is_err());
}

#[test]
fn rq_30_the_book_the_cases_start_from_reads_back() {
    let b = book();
    assert_eq!(b.requests().len(), 7);
    assert_eq!(RequestBook::from_bytes(&b.to_bytes()).unwrap(), b);
}

#[test]
fn rq_30_a_field_out_of_range_is_refused() {
    refused(|b| b.requests[0].amount = 0);
    refused(|b| b.requests[0].amount = MAX_SATS + 1);
    refused(|b| b.requests[1].amount = 100_000_001);
    refused(|b| b.requests[0].note = String::new());
    refused(|b| b.requests[0].note = "a\nb".to_string());
    refused(|b| b.requests[0].note = "x".repeat(65));
    refused(|b| b.requests[0].due = Day(b.requests[0].made.0 + 367));
    refused(|b| b.requests[0].due = Day(b.requests[0].made.0 - 1));
    refused(|b| b.requests[0].due = Day(u32::MAX));
    refused(|b| b.requests[0].credits[0].sats = 0);
    refused(|b| b.requests[0].credits[0].sats = MAX_SATS + 1);
    refused(|b| b.returns[0].sats = 1);
    refused(|b| b.returns[0].sats = MAX_SATS + 1);
    refused(|b| b.returns[0].script = ScriptBuf::new());
    refused(|b| {
        b.requests[0].repeat = Some(Repeat {
            every: Every::Week,
            series: 1,
            index: 0,
            first_due: Day(0),
            lead: 32,
        })
    });
}

#[test]
fn rq_30_fields_that_do_not_agree_with_each_other_are_refused() {
    refused(|b| b.requests[0].address_index = b.requests[1].address_index);
    refused(|b| b.requests[1].number = 9);
    refused(|b| b.next_number += 1);
    refused(|b| b.next_return += 1);
    refused(|b| b.requests[0].credits[0].value += 1);
    refused(|b| b.requests[1].credits[0].value += 1);
    refused(|b| b.requests[1].credits[0].rate = None);
    refused(|b| b.requests[1].quotes[0].until = Some(5));
    refused(|b| b.requests[1].quotes[0].rate = None);
    refused(|b| {
        b.requests[0].quotes.push(Quote {
            made_at: 1,
            owed: 1,
            sats: 1,
            rate: Rate::new(1),
            until: None,
        })
    });
    refused(|b| b.requests[0].credits[0].after = Some(After::Cancel));
    refused(|b| b.requests[2].credits[0].after = None);
    refused(|b| b.requests[2].end = None);
    refused(|b| b.requests[3].credits[1].after = Some(After::Cancel));
    refused(|b| b.requests[3].credits.swap(0, 1));
    let dup = b_credit_twice();
    assert!(RequestBook::from_bytes(&dup.to_bytes()).is_err());
    refused(|b| b.requests[0].end = Some((End::CountedPaid, 1)));
    refused(|b| b.returns[0].request = 99);
    refused(|b| b.returns[0].sats = 10_000_000);
    refused(|b| b.returns[0].id = 2);
    refused(|b| b.stopped = vec![2]);
    refused(|b| b.stopped = vec![5, 5]);
    refused(|b| b.requests[5].repeat = b.requests[4].repeat);
    refused(|b| {
        let rep = b.requests[5].repeat.as_mut().unwrap();
        rep.first_due = Day(rep.first_due.0 + 1);
    });
}

fn b_credit_twice() -> RequestBook {
    let mut b = book();
    let again = b.requests[0].credits[0];
    b.requests[0].credits.push(again);
    b
}

#[test]
fn rq_30_a_book_of_1001_open_requests_is_refused_and_one_of_1000_is_read() {
    let mut b = RequestBook::new();
    for i in 0..1_000 {
        b.make(&new("x", 50_000, false, None), i, T0, &shop(), None)
            .unwrap();
    }
    assert_eq!(RequestBook::from_bytes(&b.to_bytes()).unwrap(), b);
    let mut over = b.clone();
    let mut extra = over.requests[0].clone();
    extra.number = 1_001;
    extra.address_index = 5_000;
    over.requests.push(extra);
    over.next_number = 1_002;
    assert_eq!(
        RequestBook::from_bytes(&over.to_bytes()),
        Err(BookError::Corrupt("more requests are open than may be"))
    );
}

#[test]
fn rq_30_a_request_holds_at_most_64_quotes_in_the_file() {
    let mut b = RequestBook::new();
    b.make(&new("x", 80_000, false, None), 0, T0, &shop(), None)
        .unwrap();
    for i in 0..70u64 {
        b.share(1, T0 + i, None).unwrap();
    }
    assert_eq!(b.requests[0].quotes.len(), 64);
    assert!(RequestBook::from_bytes(&b.to_bytes()).is_ok());
    let q = b.requests[0].quotes[0];
    b.requests[0].quotes.push(q);
    assert!(RequestBook::from_bytes(&b.to_bytes()).is_err());
}

#[test]
fn rq_30_no_more_than_99_returns_wait_in_the_file() {
    let mut b = RequestBook::new();
    b.make(&new("x", 60_000, false, None), 0, T0, &shop(), None)
        .unwrap();
    b.credit(1, &[arrival(1, 10, T0, 5_000_000)], T0, &shop(), None)
        .unwrap();
    for _ in 0..99 {
        b.queue_return(1, ReturnAmount::Sats(1_000), script(), T0, None)
            .unwrap();
    }
    assert!(RequestBook::from_bytes(&b.to_bytes()).is_ok());
    let r = b.returns[0].clone();
    b.returns.push(Return { id: 100, ..r });
    b.next_return = 101;
    assert!(RequestBook::from_bytes(&b.to_bytes()).is_err());
}

#[test]
fn rq_34_the_wide_note_of_64_characters_is_read_back() {
    let mut b = RequestBook::new();
    let note = "\u{1f600}".repeat(64);
    b.make(&new(&note, 60_000, false, None), 0, T0, &shop(), None)
        .unwrap();
    assert_eq!(b.requests[0].note.len(), 256);
    assert_eq!(RequestBook::from_bytes(&b.to_bytes()).unwrap(), b);
}
