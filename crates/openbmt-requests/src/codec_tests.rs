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
    refused(|b| b.next_return = 1);
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
    refused(|b| {
        b.returns[0].sats = 10_000_000;
        b.returns[0].state = ReturnState::Queued;
    });
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
fn rq_30_returns_queued_or_in_a_spend_together_are_at_most_99_in_the_file() {
    let mut b = RequestBook::new();
    b.make(&new("x", 60_000, false, None), 0, T0, &shop(), None)
        .unwrap();
    b.credit(1, &[arrival(1, 10, T0, 5_000_000)], T0, &shop(), None)
        .unwrap();
    for _ in 0..99 {
        b.queue_return(1, ReturnAmount::Sats(1_000), script(), T0, None)
            .unwrap();
    }
    let ids = b.queued_return_ids();
    b.mark_returns_sent(&ids[..50], [1; 32]);
    assert_eq!(RequestBook::from_bytes(&b.to_bytes()).unwrap(), b);
    let r = b.returns[0].clone();
    let mut hundred = b.clone();
    hundred.returns.push(Return {
        id: 100,
        state: ReturnState::Sent { txid: [2; 32] },
        ..r
    });
    hundred.next_return = 101;
    assert!(RequestBook::from_bytes(&hundred.to_bytes()).is_err());
    // The same hundred with one of them done reads.
    hundred.returns[99].state = ReturnState::Done {
        txid: [2; 32],
        at: T0,
    };
    assert_eq!(
        RequestBook::from_bytes(&hundred.to_bytes()).unwrap(),
        hundred
    );
}

#[test]
fn rq_37_the_read_takes_more_returned_than_received_only_with_none_queued() {
    let mut b = RequestBook::new();
    b.make(&new("x", 60_000, false, None), 0, T0, &shop(), None)
        .unwrap();
    b.credit(1, &[arrival(1, 10, T0, 100_000)], T0, &shop(), None)
        .unwrap();
    for sats in [40_000, 30_000, 30_000] {
        b.queue_return(1, ReturnAmount::Sats(sats), script(), T0, None)
            .unwrap();
    }
    b.mark_returns_sent(&[1, 2], [1; 32]);
    b.take_back_above(5);
    // Returns 1 and 2 are in a spend, 3 was taken off the queue: a gap in
    // the ids, and 70,000 returned of nothing received.
    assert_eq!(b.returns().len(), 2);
    assert_eq!(b.over_returned(1), Some(70_000));
    assert_eq!(RequestBook::from_bytes(&b.to_bytes()).unwrap(), b);
    // The gap alone is a book that reads; a queued return of that request
    // beside the excess is not.
    let mut queued = b.clone();
    queued.returns[1].state = ReturnState::Queued;
    assert!(RequestBook::from_bytes(&queued.to_bytes()).is_err());
}

#[test]
fn rq_30_a_quote_in_the_currency_asks_what_its_amount_and_rate_give() {
    assert_eq!(book().requests()[1].quotes[0].sats, 250_000);
    refused(|b| b.requests[1].quotes[0].sats += 1);
    refused(|b| b.requests[1].quotes[0].sats -= 1);
    // The amount and the rate, changed without the satoshis, are caught too.
    refused(|b| b.requests[1].quotes[0].owed -= 1);
    refused(|b| b.requests[1].quotes[0].rate = Rate::new(RATE * 2));
}

#[test]
fn rq_30_a_repeat_is_of_a_series_that_starts_with_a_first_request_the_same() {
    let b = book();
    assert!(b.requests()[5].repeat.is_some_and(|r| r.index > 0));
    // A member whose lead differs from the root's.
    refused(|b| b.requests[5].repeat.as_mut().unwrap().lead ^= 1);
    // A member with another first due day, its own due date made to agree.
    refused(|b| {
        let rep = b.requests[5].repeat.as_mut().unwrap();
        rep.first_due = Day(rep.first_due.0 + 1);
        b.requests[5].due = rep.every.due(rep.first_due, rep.index).unwrap();
    });
    // A member with another cadence, its own due date made to agree.
    refused(|b| {
        let rep = b.requests[5].repeat.as_mut().unwrap();
        rep.every = Every::Month;
        b.requests[5].due = rep.every.due(rep.first_due, rep.index).unwrap();
    });
    // A member that names a member, not the first request, as its series.
    refused(|b| b.requests[6].repeat.as_mut().unwrap().series = 6);
    // A member that names a request that does not repeat.
    refused(|b| b.requests[5].repeat.as_mut().unwrap().series = 1);
}

#[test]
fn rq_30_a_book_of_100000_requests_and_100000_returns_is_read_in_one_pass() {
    // One funded request, the rest cancelled, every return done and from
    // the funded one: the shape that a scan of the returns for each request
    // takes ten billion steps to read.
    let mut b = RequestBook::new();
    b.make(&new("x", 60_000, false, None), 0, T0, &shop(), None)
        .unwrap();
    b.credit(1, &[arrival(1, 10, T0, 100_000_000)], T0, &shop(), None)
        .unwrap();
    let template = b.requests[0].clone();
    for number in 2..=crate::MAX_REQUESTS as u64 {
        b.requests.push(Request {
            number,
            address_index: number as u32 - 1,
            end: Some((End::Cancelled, T0)),
            credits: Vec::new(),
            ..template.clone()
        });
    }
    b.next_number = crate::MAX_REQUESTS as u64 + 1;
    let ret = Return {
        id: 0,
        request: 1,
        script: script(),
        sats: 294,
        queued_at: T0,
        state: ReturnState::Done {
            txid: [4; 32],
            at: T0,
        },
    };
    for id in 1..=crate::MAX_RETURNS as u64 {
        b.returns.push(Return { id, ..ret.clone() });
    }
    b.next_return = crate::MAX_RETURNS as u64 + 1;
    let bytes = b.to_bytes();
    let started = std::time::Instant::now();
    let read = RequestBook::from_bytes(&bytes).unwrap();
    let took = started.elapsed();
    assert!(read == b);
    assert!(took.as_secs() < 5, "the read took {took:?}");
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

#[test]
fn rq_04_a_book_of_version_1_still_reads_with_no_wallet_on_its_requests() {
    let mut b = RequestBook::new();
    let n = b
        .make(&new("Rent", 5_000, false, None), 3, T0, &shop(), None)
        .unwrap();
    assert_eq!(n, 1);
    let old = b.encode(1);
    assert_eq!(old.get(4), Some(&1));
    let mut read = RequestBook::from_bytes(&old).unwrap();
    assert_eq!(read.request(1).unwrap().wallet, None);
    // The wallet pinned when it was read goes to every request without one,
    // and the book is then written as version 2 and reads back the same.
    assert_eq!(read.assign_wallet([7; 32]), 1);
    assert_eq!(read.assign_wallet([8; 32]), 0);
    let new_bytes = read.to_bytes();
    assert_eq!(new_bytes.get(4), Some(&2));
    let again = RequestBook::from_bytes(&new_bytes).unwrap();
    assert_eq!(again.request(1).unwrap().wallet, Some([7; 32]));
    assert_eq!(again.to_bytes(), new_bytes);
    // A version 1 book has no cancelled return and no wallet: a version 1
    // frame that holds either is not one.
    let mut with_wallet = old.clone();
    with_wallet.splice(0..0, []);
    assert!(RequestBook::from_bytes(&new_bytes[..4]).is_err());
    let mut v3 = new_bytes.clone();
    v3[4] = 3;
    assert!(RequestBook::from_bytes(&v3).is_err());
}

#[test]
fn rq_04_a_request_keeps_the_wallet_it_was_made_from() {
    let mut b = RequestBook::new();
    b.use_wallet(Some([1; 32]));
    b.make(&new("Old", 5_000, false, None), 0, T0, &shop(), None)
        .unwrap();
    b.use_wallet(Some([2; 32]));
    b.make(&new("New", 5_000, false, None), 1, T0, &shop(), None)
        .unwrap();
    assert_eq!(b.request(1).unwrap().wallet, Some([1; 32]));
    assert_eq!(b.request(2).unwrap().wallet, Some([2; 32]));
    let read = RequestBook::from_bytes(&b.to_bytes()).unwrap();
    assert_eq!(read.request(1).unwrap().wallet, Some([1; 32]));
    assert_eq!(read.request(2).unwrap().wallet, Some([2; 32]));
    // Giving a wallet to the unlabelled never changes one that has it.
    let mut read = read;
    assert_eq!(read.assign_wallet([9; 32]), 0);
}

#[test]
fn rq_29_a_cancelled_return_is_written_and_read_and_counts_for_nothing() {
    let mut b = RequestBook::new();
    let n = b
        .make(&new("Rent", 100_000, false, None), 0, T0, &shop(), None)
        .unwrap();
    b.credit(
        n,
        &[Arrival {
            txid: [1; 32],
            vout: 0,
            height: 10,
            block_time: T0,
            sats: 100_000,
        }],
        T0,
        &shop(),
        None,
    )
    .unwrap();
    let id = b
        .queue_return(n, ReturnAmount::Sats(40_000), script(), T0, None)
        .unwrap();
    assert_eq!(b.returnable_sats(n), Some(60_000));
    b.cancel_return(id, T0 + 5).unwrap();
    assert_eq!(b.returnable_sats(n), Some(100_000));
    assert_eq!(b.returns()[0].state, ReturnState::Cancelled { at: T0 + 5 });
    assert_eq!(b.over_returned(n), None);
    assert!(b.returns_to_pay().is_empty());
    let read = RequestBook::from_bytes(&b.to_bytes()).unwrap();
    assert_eq!(read.returns(), b.returns());
    // The same amount can be returned again, and the ledger shows only it.
    b.queue_return(n, ReturnAmount::Sats(100_000), script(), T0 + 6, None)
        .unwrap();
    assert_eq!(b.ledger_lines()[0].returns.len(), 1);
    // Only a queued return is cancelled.
    assert_eq!(
        b.cancel_return(id, T0 + 7),
        Err(CancelReturnRefused::NotQueued)
    );
    assert_eq!(
        b.cancel_return(99, T0 + 7),
        Err(CancelReturnRefused::NoSuchReturn)
    );
    b.mark_returns_sent(&[2], [3; 32]);
    assert_eq!(
        b.cancel_return(2, T0 + 8),
        Err(CancelReturnRefused::NotQueued)
    );
}
