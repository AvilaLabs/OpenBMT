//! RQ-37: a block that leaves the chain.

mod common;

use common::*;
use openbmt_requests::say::{over_returned_line, WORDS_NEEDED_RQ_37_OVER_RETURNED};
use openbmt_requests::{After, Reorged, RequestBook, ReturnAmount, ReturnState, Status};

fn status(book: &RequestBook, n: u64) -> Status {
    book.request(n).unwrap().status(5_000)
}

/// The book as written and read back; it must be the same book.
#[track_caller]
fn saved(book: &RequestBook) {
    assert_eq!(&RequestBook::from_bytes(&book.to_bytes()).unwrap(), book);
}

fn give_back(book: &mut RequestBook, n: u64, sats: u64) -> u64 {
    book.queue_return(n, ReturnAmount::Sats(sats), script_of(P2WPKH), T0, None)
        .unwrap()
}

#[test]
fn rq_37_a_payment_in_a_block_that_left_the_chain_is_taken_back() {
    let mut book = RequestBook::new();
    let n = make_btc(&mut book, 0, 100_000, 7);
    book.credit(n, &[arrival(1, 10, T0, 100_000)], T0, &shop(), None)
        .unwrap();
    assert_eq!(status(&book, n), Status::Paid);
    assert_eq!(book.returnable_sats(n), Some(100_000));
    let done = book.take_back_above(9);
    assert_eq!(done.credits.len(), 1);
    assert_eq!((done.credits[0].0, done.credits[0].1.sats), (n, 100_000));
    assert!(done.removed_returns.is_empty() && done.over_returned.is_empty());
    assert_eq!(status(&book, n), Status::Waiting);
    assert_eq!(book.request(n).unwrap().received_sats(), 0);
    assert_eq!(book.returnable_sats(n), Some(0));
    assert!(book.request(n).unwrap().is_open());
    saved(&book);
}

#[test]
fn rq_37_a_payment_at_or_below_the_fork_point_is_kept() {
    let mut book = RequestBook::new();
    let n = make_btc(&mut book, 0, 100_000, 7);
    book.credit(
        n,
        &[arrival(1, 10, T0, 30_000), arrival(2, 11, T0 + 600, 70_000)],
        T0,
        &shop(),
        None,
    )
    .unwrap();
    let done = book.take_back_above(10);
    assert_eq!(done.credits.len(), 1);
    assert_eq!(done.credits[0].1.height, 11);
    assert_eq!(book.request(n).unwrap().credits.len(), 1);
    assert_eq!(status(&book, n), Status::PartPaid { owed: 70_000 });
    // A fork at or above the newest block changes nothing.
    assert_eq!(book.take_back_above(10), Reorged::default());
    assert_eq!(book.take_back_above(500), Reorged::default());
    saved(&book);
}

#[test]
fn rq_37_the_state_follows_the_payments_that_remain() {
    let mut book = RequestBook::new();
    let n = make_btc(&mut book, 0, 100_000, 7);
    book.credit(
        n,
        &[arrival(1, 10, T0, 60_000), arrival(2, 20, T0 + 600, 80_000)],
        T0,
        &shop(),
        None,
    )
    .unwrap();
    assert_eq!(status(&book, n), Status::Overpaid { excess: 40_000 });
    book.take_back_above(15);
    assert_eq!(status(&book, n), Status::PartPaid { owed: 40_000 });
    book.take_back_above(5);
    assert_eq!(status(&book, n), Status::Waiting);
    saved(&book);
}

#[test]
fn rq_37_a_payment_taken_back_in_every_request_and_late_and_after_close_ones_too() {
    let mut book = RequestBook::new();
    let a = make_btc(&mut book, 0, 100_000, 7);
    let b = make_btc(&mut book, 1, 100_000, 7);
    book.credit(a, &[arrival(1, 10, T0, 100_000)], T0, &shop(), None)
        .unwrap();
    book.credit(b, &[arrival(2, 12, T0, 40_000)], T0, &shop(), None)
        .unwrap();
    book.close(b, T0 + 5, &shop()).unwrap();
    book.credit(b, &[arrival(3, 14, T0 + 9, 9_000)], T0 + 9, &shop(), None)
        .unwrap();
    // A receipt of a day long past is late.
    let c = make_btc(&mut book, 2, 100_000, 0);
    book.credit(
        c,
        &[arrival(4, 15, T0 + 3 * DAY, 20_000)],
        T0,
        &shop(),
        None,
    )
    .unwrap();
    assert!(book.request(c).unwrap().credits[0].late);
    let done = book.take_back_above(11);
    let taken: Vec<_> = done.credits.iter().map(|(n, c)| (*n, c.height)).collect();
    assert_eq!(taken, [(b, 12), (b, 14), (c, 15)]);
    assert_eq!(done.credits[1].1.after, Some(After::Close));
    assert!(book.request(b).unwrap().credits.is_empty());
    // The request stays closed: nothing is cancelled or reopened for the owner.
    assert!(book.request(b).unwrap().end.is_some());
    assert_eq!(status(&book, a), Status::Paid);
    saved(&book);
}

#[test]
fn rq_37_queued_returns_come_off_newest_first_until_they_no_longer_exceed() {
    let mut book = RequestBook::new();
    let n = make_btc(&mut book, 0, 100_000, 7);
    book.credit(
        n,
        &[arrival(1, 10, T0, 60_000), arrival(2, 20, T0 + 600, 40_000)],
        T0,
        &shop(),
        None,
    )
    .unwrap();
    for sats in [30_000, 30_000, 30_000, 10_000] {
        give_back(&mut book, n, sats);
    }
    // Received falls to 60,000; 100,000 is returned, 40,000 too much.
    let done = book.take_back_above(15);
    let ids: Vec<u64> = done.removed_returns.iter().map(|r| r.id).collect();
    assert_eq!(ids, [4, 3]);
    assert!(done.over_returned.is_empty());
    assert_eq!(book.over_returned(n), None);
    let left: Vec<u64> = book.returns().iter().map(|r| r.id).collect();
    assert_eq!(left, [1, 2]);
    assert_eq!(book.returnable_sats(n), Some(0));
    saved(&book);
    // A new return takes the next id, not one that was taken off.
    book.credit(n, &[arrival(5, 16, T0 + 900, 20_000)], T0, &shop(), None)
        .unwrap();
    assert_eq!(give_back(&mut book, n, 10_000), 5);
    saved(&book);
}

#[test]
fn rq_37_a_return_in_a_spend_stays_and_the_request_shows_more_was_returned() {
    let mut book = RequestBook::new();
    let n = make_btc(&mut book, 0, 100_000, 7);
    book.credit(n, &[arrival(1, 10, T0, 100_000)], T0, &shop(), None)
        .unwrap();
    let sent = give_back(&mut book, n, 50_000);
    let done_id = give_back(&mut book, n, 10_000);
    give_back(&mut book, n, 30_000);
    give_back(&mut book, n, 10_000);
    book.mark_returns_sent(&[done_id], [7; 32]);
    book.mark_returns_done([7; 32], T0 + 50);
    book.mark_returns_sent(&[sent], [8; 32]);
    let done = book.take_back_above(5);
    // Everything received is gone; the two queued returns come off and the
    // sent and done ones stay: 60,000 returned and nothing received.
    assert_eq!(done.removed_returns.len(), 2);
    assert_eq!(done.over_returned, [(n, 60_000)]);
    assert_eq!(book.over_returned(n), Some(60_000));
    assert_eq!(book.returnable_sats(n), Some(0));
    let states: Vec<_> = book.returns().iter().map(|r| r.state).collect();
    assert_eq!(
        states,
        [
            ReturnState::Sent { txid: [8; 32] },
            ReturnState::Done {
                txid: [7; 32],
                at: T0 + 50
            }
        ]
    );
    saved(&book);
    // The spend is replaced without its return: it goes back to the queue
    // and, still more than was received, comes off again.
    assert_eq!(book.requeue_returns([8; 32]), 0);
    assert_eq!(book.returns().len(), 1);
    assert_eq!(book.over_returned(n), Some(10_000));
    saved(&book);
}

#[test]
fn rq_37_a_request_that_was_not_over_returned_reports_nothing() {
    let mut book = RequestBook::new();
    let n = make_btc(&mut book, 0, 100_000, 7);
    assert_eq!(book.over_returned(n), None);
    assert_eq!(book.over_returned(99), None);
    book.credit(n, &[arrival(1, 10, T0, 100_000)], T0, &shop(), None)
        .unwrap();
    give_back(&mut book, n, 100_000);
    assert_eq!(book.over_returned(n), None);
    assert_eq!(over_returned_line(), WORDS_NEEDED_RQ_37_OVER_RETURNED);
}

#[test]
fn rq_37_the_same_output_in_a_block_of_the_new_chain_is_credited_again() {
    let mut book = RequestBook::new();
    let n = make_btc(&mut book, 0, 100_000, 7);
    let first = arrival(1, 10, T0, 100_000);
    book.credit(n, &[first], T0, &shop(), None).unwrap();
    // The same output is not counted twice while it stands.
    assert_eq!(book.credit(n, &[first], T0, &shop(), None), Ok(0));
    book.take_back_above(9);
    let again = openbmt_requests::Arrival {
        height: 11,
        block_time: T0 + 700,
        ..first
    };
    assert_eq!(book.credit(n, &[again], T0 + 800, &shop(), None), Ok(1));
    assert_eq!(book.request(n).unwrap().credits[0].height, 11);
    assert_eq!(status(&book, n), Status::Paid);
    saved(&book);
}

#[test]
fn rq_37_a_book_with_nothing_above_the_fork_is_left_as_it_was() {
    let mut book = RequestBook::new();
    let n = make_btc(&mut book, 0, 100_000, 7);
    book.credit(n, &[arrival(1, 10, T0, 100_000)], T0, &shop(), None)
        .unwrap();
    give_back(&mut book, n, 10_000);
    let before = book.clone();
    assert_eq!(book.take_back_above(10), Reorged::default());
    assert_eq!(book, before);
}
