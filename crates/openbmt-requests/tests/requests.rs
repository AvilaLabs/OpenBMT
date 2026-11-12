//! RQ-01 to RQ-14: making a request and sharing it.

mod common;

use common::*;
use openbmt_core::currency::ShopCurrency;
use openbmt_counter::money::{MAX_SATS, QUOTE_SECS};
use openbmt_requests::say::share_line;
use openbmt_requests::{
    share_link, Limit, MakeRefused, NewRequest, Pricing, RequestBook, ShareRefused,
};

const MANIFEST: &str = include_str!("../Cargo.toml");

fn direct_dependencies() -> Vec<&'static str> {
    MANIFEST
        .split("[dependencies]")
        .nth(1)
        .unwrap()
        .lines()
        .filter_map(|l| l.split('=').next())
        .map(str::trim)
        .filter(|n| !n.is_empty() && !n.starts_with('#'))
        .collect()
}

#[test]
fn rq_01_the_library_is_the_owners_and_has_nothing_of_the_key_app() {
    // Requests are made on the owner phone of a shop paid to its own wallet.
    // Whether a shop is one is the app's to know; here, no part of the Key
    // app's pairing or signer is a dependency, and the counter's short
    // request (CR-205) is not in the book.
    let deps = direct_dependencies();
    for not in ["openbmt-key-ffi", "openbmt-signer", "openbmt-approver"] {
        assert!(!deps.contains(&not), "{not}");
    }
    let mut book = RequestBook::new();
    assert!(book.requests().is_empty());
    assert_eq!(make_btc(&mut book, 0, 50_000, 7), 1);
}

#[test]
fn rq_02_the_book_takes_its_index_from_the_caller_and_keeps_no_allocation() {
    // The desk's ranges are the owner state's (WL-23, WL-25 to WL-27): the
    // book is given each index, in any order, and holds no range.
    let mut book = RequestBook::new();
    make_btc(&mut book, 1_000, 50_000, 7);
    make_btc(&mut book, 5, 50_000, 7);
    make_btc(&mut book, 2_000, 50_000, 7);
    assert_eq!(book.address_indexes(), [5, 1_000, 2_000]);
}

#[test]
fn rq_03_an_index_used_by_any_request_is_refused_even_after_a_cancel() {
    let mut book = RequestBook::new();
    let n = make_btc(&mut book, 4, 50_000, 7);
    book.cancel(n, T0).unwrap();
    let before = book.clone();
    assert_eq!(
        book.make(
            &bitcoin_request("Again", 50_000, in_days(3)),
            4,
            T0,
            &shop(),
            None
        ),
        Err(MakeRefused::IndexUsed)
    );
    assert_eq!(book, before, "a refusal writes nothing");
    // A fresh index is fine.
    assert_eq!(make_btc(&mut book, 5, 50_000, 7), 2);
}

#[test]
fn rq_04_every_address_stays_in_the_watch_set_whatever_became_of_the_request() {
    let mut book = RequestBook::new();
    let paid = make_btc(&mut book, 0, 50_000, 7);
    let cancelled = make_btc(&mut book, 1, 50_000, 7);
    let closed = make_btc(&mut book, 2, 50_000, 7);
    book.credit(paid, &[arrival(1, 10, T0, 50_000)], T0, &shop(), None)
        .unwrap();
    book.cancel(cancelled, T0).unwrap();
    book.credit(closed, &[arrival(2, 10, T0, 10_000)], T0, &shop(), None)
        .unwrap();
    book.close(closed, T0, &shop()).unwrap();
    assert_eq!(book.address_indexes(), [0, 1, 2]);
    // And it is still so after a trip through the file.
    let again = RequestBook::from_bytes(&book.to_bytes()).unwrap();
    assert_eq!(again.address_indexes(), [0, 1, 2]);
}

#[test]
fn rq_05_a_request_has_the_fields_and_a_number_that_counts_up_and_is_never_reused() {
    let mut book = RequestBook::new();
    assert_eq!(book.next_number(), 1);
    let n = book
        .make(
            &bitcoin_request("Mia, 3 March", 120_000, in_days(10)),
            9,
            T0,
            &shop(),
            None,
        )
        .unwrap();
    let r = book.request(n).unwrap();
    assert_eq!(r.number, 1);
    assert_eq!(r.note, "Mia, 3 March");
    assert_eq!(r.amount, 120_000);
    assert_eq!(r.pricing, Pricing::Bitcoin);
    assert_eq!(r.made_at, T0);
    assert_eq!(r.made, today());
    assert_eq!(r.due, in_days(10));
    assert_eq!(r.address_index, 9);
    assert_eq!(r.repeat, None);
    book.cancel(1, T0).unwrap();
    // The cancelled one keeps its number.
    assert_eq!(make_btc(&mut book, 10, 50_000, 7), 2);
    assert_eq!(make_btc(&mut book, 11, 50_000, 7), 3);
    assert_eq!(book.request(1).unwrap().note, "Mia, 3 March");
    // A refused request takes no number.
    assert!(book
        .make(
            &bitcoin_request("", 50_000, in_days(1)),
            12,
            T0,
            &shop(),
            None
        )
        .is_err());
    assert_eq!(book.next_number(), 4);
}

#[test]
fn rq_06_a_note_is_1_to_64_characters_of_plain_text() {
    let mut book = RequestBook::new();
    let mut try_note = |note: &str, index: u32| {
        book.make(
            &bitcoin_request(note, 50_000, in_days(1)),
            index,
            T0,
            &shop(),
            None,
        )
    };
    assert_eq!(try_note("", 0), Err(MakeRefused::NoteNeeded));
    assert_eq!(try_note(&"a".repeat(65), 1), Err(MakeRefused::NoteNeeded));
    assert_eq!(try_note("tab\there", 2), Err(MakeRefused::NoteNeeded));
    assert_eq!(try_note("line\nbreak", 3), Err(MakeRefused::NoteNeeded));
    assert_eq!(try_note("nul\u{0}", 4), Err(MakeRefused::NoteNeeded));
    assert_eq!(try_note("del\u{7f}", 5), Err(MakeRefused::NoteNeeded));
    assert!(try_note("x", 6).is_ok());
    assert!(try_note(&"a".repeat(64), 7).is_ok());
    // 64 characters of 4 bytes are 64 characters.
    assert!(try_note(&"\u{1f600}".repeat(64), 8).is_ok());
    assert_eq!(
        try_note(&"\u{1f600}".repeat(65), 9),
        Err(MakeRefused::NoteNeeded)
    );
    // The note is not in what is shared.
    let q = share_link(P2WPKH, 50_000, "Dunmore Books");
    assert!(!q.contains("Booking"));
}

#[test]
fn rq_07_the_way_a_request_is_priced_is_chosen_once_and_never_changes() {
    let mut book = RequestBook::new();
    let a = make_btc(&mut book, 0, 50_000, 7);
    let b = make_usd(&mut book, 1, 5_000, 7);
    assert_eq!(book.request(a).unwrap().pricing, Pricing::Bitcoin);
    assert_eq!(
        book.request(b).unwrap().pricing,
        Pricing::Currency(ShopCurrency::Usd)
    );
    book.share(a, T0, None).unwrap();
    book.share(b, T0, Some(&fresh(T0))).unwrap();
    book.credit(b, &[arrival(1, 5, T0, 1_000)], T0, &shop(), None)
        .unwrap();
    assert_eq!(book.request(a).unwrap().pricing, Pricing::Bitcoin);
    assert_eq!(
        book.request(b).unwrap().pricing,
        Pricing::Currency(ShopCurrency::Usd)
    );
}

#[test]
fn rq_07_a_shop_in_bitcoin_standard_has_requests_in_bitcoin_only() {
    let mut book = RequestBook::new();
    assert_eq!(
        book.make(
            &usd_request("x", 5_000, in_days(1)),
            0,
            T0,
            &sats_shop(),
            Some(&fresh(T0))
        ),
        Err(MakeRefused::BitcoinOnly)
    );
    assert!(book
        .make(
            &bitcoin_request("x", 50_000, in_days(1)),
            0,
            T0,
            &sats_shop(),
            None
        )
        .is_ok());
}

#[test]
fn rq_08_a_request_in_bitcoin_is_at_least_the_smallest_sale_and_the_dust_line() {
    let mut book = RequestBook::new();
    let mut ask = |sats: u64, min_sale: u64, index: u32| {
        let shop = openbmt_requests::Shop {
            min_sale_sats: min_sale,
            ..shop()
        };
        book.make(
            &bitcoin_request("x", sats, in_days(1)),
            index,
            T0,
            &shop,
            None,
        )
    };
    assert_eq!(
        ask(4_999, 5_000, 0),
        Err(MakeRefused::TooSmall { min_sats: 5_000 })
    );
    assert!(ask(5_000, 5_000, 1).is_ok());
    // No floor in the settings: the dust line of 1,000 satoshis stands.
    assert_eq!(
        ask(999, 0, 2),
        Err(MakeRefused::TooSmall { min_sats: 1_000 })
    );
    assert!(ask(1_000, 0, 3).is_ok());
    // A floor under the dust line does not lower it.
    assert_eq!(
        ask(500, 300, 4),
        Err(MakeRefused::TooSmall { min_sats: 1_000 })
    );
    assert_eq!(ask(0, 0, 5), Err(MakeRefused::TooSmall { min_sats: 1_000 }));
    // Nothing was written for a refusal.
    assert_eq!(book.requests().len(), 2);
    assert_eq!(book.next_number(), 3);
}

#[test]
fn rq_08_a_request_in_bitcoin_is_at_most_21_million_bitcoin() {
    let mut book = RequestBook::new();
    let mut ask = |sats: u64, index: u32| {
        book.make(
            &bitcoin_request("x", sats, in_days(1)),
            index,
            T0,
            &shop(),
            None,
        )
    };
    assert!(ask(MAX_SATS, 0).is_ok());
    assert_eq!(
        ask(MAX_SATS + 1, 1),
        Err(MakeRefused::TooLarge {
            max: Limit::Sats(MAX_SATS)
        })
    );
    assert!(ask(u64::MAX, 2).is_err());
}

#[test]
fn rq_08_a_request_in_the_currency_is_worked_out_at_the_rate_and_at_most_a_million_units() {
    let mut book = RequestBook::new();
    let mut ask = |cents: u64, rate: u64, index: u32| {
        book.make(
            &usd_request("x", cents, in_days(1)),
            index,
            T0,
            &shop(),
            Some(&self::rate(rate, T0)),
        )
    };
    // $1,000,000 is 100,000,000 cents.
    assert!(ask(100_000_000, RATE, 0).is_ok());
    let too_large = Err(MakeRefused::TooLarge {
        max: Limit::Units(ShopCurrency::Usd, 1_000_000),
    });
    assert_eq!(ask(100_000_001, RATE, 1), too_large);
    // $5 at $100,000 is 5,000 satoshis: the floor. $4.99 is 4,990.
    assert!(ask(500, RATE, 2).is_ok());
    assert_eq!(
        ask(499, RATE, 3),
        Err(MakeRefused::TooSmall { min_sats: 5_000 })
    );
    assert_eq!(
        ask(0, RATE, 4),
        Err(MakeRefused::TooSmall { min_sats: 5_000 })
    );
    // At a rate ten times the price, $5 is worth 500 satoshis.
    assert_eq!(
        ask(500, 10 * RATE, 5),
        Err(MakeRefused::TooSmall { min_sats: 5_000 })
    );
    // The satoshis are rounded up: $5.01 at a price of $99,999 needs more than
    // its share of a satoshi.
    assert!(ask(501, 9_999_900, 6).is_ok());
    // At a rate so low that the satoshis pass 21 million bitcoin.
    assert_eq!(ask(100_000_000, 1, 7), too_large);
}

#[test]
fn rq_08_the_limit_in_units_follows_the_minor_digits_of_the_currency() {
    let yen = openbmt_requests::Shop {
        currency: ShopCurrency::Jpy,
        ..shop()
    };
    let mut book = RequestBook::new();
    // 1,000,000 yen is 1,000,000 minor units (yen have none smaller).
    let rate = rate(15_000_000, T0); // 15,000,000 yen a bitcoin
    assert!(book
        .make(
            &usd_request("x", 1_000_000, in_days(1)),
            0,
            T0,
            &yen,
            Some(&rate)
        )
        .is_ok());
    assert_eq!(
        book.make(
            &usd_request("x", 1_000_001, in_days(1)),
            1,
            T0,
            &yen,
            Some(&rate)
        ),
        Err(MakeRefused::TooLarge {
            max: Limit::Units(ShopCurrency::Jpy, 1_000_000)
        })
    );
}

#[test]
fn rq_09_the_due_date_is_from_the_day_it_is_made_to_366_days_later() {
    let mut book = RequestBook::new();
    let mut due = |d: openbmt_requests::Day, index: u32| {
        book.make(&bitcoin_request("x", 50_000, d), index, T0, &shop(), None)
    };
    assert!(due(today(), 0).is_ok());
    assert!(due(in_days(366), 1).is_ok());
    assert_eq!(due(in_days(367), 2), Err(MakeRefused::DueOutOfRange));
    assert_eq!(
        due(openbmt_requests::Day(today().0 - 1), 3),
        Err(MakeRefused::DueOutOfRange)
    );
    // 366 days on from 15 January 2027 is 16 January 2028.
    assert_eq!(in_days(366), day(2028, 1, 16));
}

#[test]
fn rq_09_the_day_it_is_made_is_a_day_of_the_shops_time_zone() {
    let mut book = RequestBook::new();
    // 08:00 UTC is already the 16th at UTC+16.
    let far_east = shop_in(16 * 3600);
    assert_eq!(
        book.make(
            &bitcoin_request("x", 50_000, today()),
            0,
            T0,
            &far_east,
            None
        ),
        Err(MakeRefused::DueOutOfRange)
    );
    let n = book
        .make(
            &bitcoin_request("x", 50_000, in_days(1)),
            1,
            T0,
            &far_east,
            None,
        )
        .unwrap();
    assert_eq!(book.request(n).unwrap().made, in_days(1));
}

#[test]
fn rq_09_a_request_stays_open_after_its_due_date() {
    let mut book = RequestBook::new();
    let n = make_btc(&mut book, 0, 50_000, 1);
    let later = in_days(60);
    assert!(book.request(n).unwrap().is_open());
    assert_eq!(book.overdue_count(later), 1);
    assert!(book.share(n, T0 + 60 * DAY, None).is_ok());
}

#[test]
fn rq_10_only_arrivals_of_checked_blocks_are_counted_and_nothing_is_accepted_at_once() {
    // The library takes arrivals from checked blocks at the depth of the
    // settings: the caller hands over nothing else. It has no instant path
    // and no exposure ledger: a request with no arrival is Waiting.
    let mut book = RequestBook::new();
    let n = make_btc(&mut book, 0, 50_000, 7);
    book.share(n, T0, None).unwrap();
    assert_eq!(
        book.request(n).unwrap().status(5_000),
        openbmt_requests::Status::Waiting
    );
    assert!(book.request(n).unwrap().credits.is_empty());
    let deps = direct_dependencies();
    for not in ["openbmt-lightning", "openbmt-swap", "openbmt-watch"] {
        assert!(!deps.contains(&not), "{not}");
    }
}

#[test]
fn rq_11_at_most_1000_requests_are_open_at_once() {
    let mut book = RequestBook::new();
    for i in 0..1_000 {
        make_btc(&mut book, i, 50_000, 7);
    }
    assert_eq!(book.open_count(), 1_000);
    let before = book.clone();
    assert_eq!(
        book.make(
            &bitcoin_request("x", 50_000, in_days(1)),
            1_000,
            T0,
            &shop(),
            None
        ),
        Err(MakeRefused::TooManyOpen)
    );
    assert_eq!(book, before);
    // A cancel frees one.
    book.cancel(7, T0).unwrap();
    assert_eq!(make_btc(&mut book, 1_000, 50_000, 7), 1_001);
    // So does a payment in full; and a paid request is not open.
    book.credit(8, &[arrival(1, 5, T0, 50_000)], T0, &shop(), None)
        .unwrap();
    assert_eq!(book.open_count(), 999);
    assert_eq!(make_btc(&mut book, 1_001, 50_000, 7), 1_002);
    assert!(book
        .make(
            &bitcoin_request("x", 50_000, in_days(1)),
            1_002,
            T0,
            &shop(),
            None
        )
        .is_err());
}

#[test]
fn rq_12_a_quote_in_bitcoin_is_the_satoshis_still_owed() {
    let mut book = RequestBook::new();
    let n = make_btc(&mut book, 0, 80_000, 7);
    let q = book.share(n, T0, None).unwrap();
    assert_eq!(
        (q.owed, q.sats, q.rate, q.until),
        (80_000, 80_000, None, None)
    );
    assert!(q.holds_at(T0 + 1_000 * DAY));
    book.credit(n, &[arrival(1, 5, T0, 30_000)], T0, &shop(), None)
        .unwrap();
    let q = book.share(n, T0 + 10, None).unwrap();
    assert_eq!((q.owed, q.sats), (50_000, 50_000));
    assert_eq!(book.request(n).unwrap().quotes.len(), 2);
}

#[test]
fn rq_12_a_quote_in_the_currency_is_the_currency_still_owed_at_the_rate_for_24_hours() {
    let mut book = RequestBook::new();
    let n = make_usd(&mut book, 0, 25_000, 7); // $250
    let q = book.share(n, T0, Some(&fresh(T0))).unwrap();
    assert_eq!((q.owed, q.sats), (25_000, 250_000));
    assert_eq!(q.rate.unwrap().minor_per_btc(), RATE);
    assert_eq!(q.until, Some(T0 + DAY));
    assert!(q.holds_at(T0 + DAY - 1));
    assert!(!q.holds_at(T0 + DAY));
    // A longer hold than a counter's quote of five minutes.
    assert!(DAY > QUOTE_SECS);
}

#[test]
fn rq_12_a_stale_or_missing_rate_makes_no_quote_and_writes_nothing() {
    let mut book = RequestBook::new();
    let n = make_usd(&mut book, 0, 25_000, 7);
    let before = book.clone();
    assert_eq!(book.share(n, T0, None), Err(ShareRefused::StaleRate));
    // Fetched more than 10 minutes before (CR-58).
    assert_eq!(
        book.share(n, T0, Some(&fresh(T0 - 601))),
        Err(ShareRefused::StaleRate)
    );
    // Fetched more than a minute after.
    assert_eq!(
        book.share(n, T0, Some(&fresh(T0 + 61))),
        Err(ShareRefused::StaleRate)
    );
    assert_eq!(book, before);
    assert!(book.share(n, T0, Some(&fresh(T0 - 600))).is_ok());
    // A request in bitcoin needs no rate.
    let b = make_btc(&mut book, 1, 50_000, 7);
    assert!(book.share(b, T0, None).is_ok());
}

#[test]
fn rq_12_a_request_that_is_not_open_is_not_shared() {
    let mut book = RequestBook::new();
    let a = make_btc(&mut book, 0, 50_000, 7);
    let b = make_btc(&mut book, 1, 50_000, 7);
    book.cancel(a, T0).unwrap();
    book.credit(b, &[arrival(1, 5, T0, 50_000)], T0, &shop(), None)
        .unwrap();
    assert_eq!(book.share(a, T0, None), Err(ShareRefused::NotOpen));
    assert_eq!(book.share(b, T0, None), Err(ShareRefused::NotOpen));
    assert_eq!(book.share(99, T0, None), Err(ShareRefused::NoSuchRequest));
}

#[test]
fn rq_12_at_most_64_quotes_are_kept_and_the_oldest_goes_first() {
    let mut book = RequestBook::new();
    let n = make_btc(&mut book, 0, 80_000, 7);
    for i in 0..70u64 {
        book.share(n, T0 + i, None).unwrap();
    }
    let quotes = &book.request(n).unwrap().quotes;
    assert_eq!(quotes.len(), 64);
    assert_eq!(quotes[0].made_at, T0 + 6);
    assert_eq!(quotes[63].made_at, T0 + 69);
}

#[test]
fn rq_12_quotes_keep_their_order_whatever_the_clock_does() {
    let mut book = RequestBook::new();
    let n = make_btc(&mut book, 0, 80_000, 7);
    book.share(n, T0 + 100, None).unwrap();
    let q = book.share(n, T0 + 50, None).unwrap();
    assert_eq!(q.made_at, T0 + 100);
    assert!(RequestBook::from_bytes(&book.to_bytes()).is_ok());
}

#[test]
fn rq_13_what_is_shared_is_a_link_with_the_address_the_amount_and_the_shops_name() {
    assert_eq!(
        share_link(P2WPKH, 250_000, "Dunmore Books"),
        format!("bitcoin:{P2WPKH}?amount=0.0025&label=Dunmore%20Books")
    );
    assert_eq!(
        share_link(P2WPKH, 100_000_000, "A&B"),
        format!("bitcoin:{P2WPKH}?amount=1&label=A%26B")
    );
    assert_eq!(
        share_link(P2WPKH, 1, "x"),
        format!("bitcoin:{P2WPKH}?amount=0.00000001&label=x")
    );
    assert_eq!(
        share_link(P2WPKH, 0, "x"),
        format!("bitcoin:{P2WPKH}?label=x")
    );
    // The label is written byte by byte, so text from a shop's name cannot
    // add a field.
    let link = share_link(P2WPKH, 5_000, "x&amount=9&y=\u{e9}");
    assert_eq!(link.matches("amount=").count(), 1);
    assert!(link.ends_with("label=x%26amount%3D9%26y%3D%C3%A9"));
}

#[test]
fn rq_13_the_line_above_the_link_is_the_one_of_the_words() {
    assert_eq!(
        share_line("Dunmore Books", "$250", 12, "3 March 2027"),
        "Dunmore Books asks for $250. Request No. 12, due 3 March 2027."
    );
}

#[test]
fn rq_14_sharing_again_makes_a_fresh_quote_of_what_is_still_owed() {
    let mut book = RequestBook::new();
    let n = make_usd(&mut book, 0, 25_000, 7);
    let first = book.share(n, T0, Some(&fresh(T0))).unwrap();
    book.credit(
        n,
        &[arrival(1, 5, T0 + 10, 100_000)],
        T0 + 20,
        &shop(),
        None,
    )
    .unwrap();
    // The price doubled: $250 less $100 is $150, at $200,000 a bitcoin.
    let t = T0 + 2 * DAY;
    let second = book.share(n, t, Some(&rate(2 * RATE, t))).unwrap();
    assert_eq!((second.owed, second.sats), (15_000, 75_000));
    assert_ne!(first, second);
    assert_eq!(book.request(n).unwrap().quotes, [first, second]);
}

#[test]
fn rq_14_an_older_link_still_pays_the_same_address_and_what_arrives_is_counted() {
    let mut book = RequestBook::new();
    let n = make_btc(&mut book, 0, 80_000, 7);
    let a = book.share(n, T0, None).unwrap();
    book.credit(n, &[arrival(1, 5, T0, 30_000)], T0, &shop(), None)
        .unwrap();
    let b = book.share(n, T0 + 5, None).unwrap();
    assert_eq!((a.sats, b.sats), (80_000, 50_000));
    // A payment of the older amount still counts, to the same request.
    book.credit(n, &[arrival(2, 6, T0 + 10, 80_000)], T0 + 10, &shop(), None)
        .unwrap();
    let r = book.request(n).unwrap();
    assert_eq!(r.address_index, 0);
    assert_eq!(r.credited(), 110_000);
    assert_eq!(r.owed(), 0);
}

#[test]
fn rq_14_the_library_sends_nothing_and_has_no_network() {
    let deps = direct_dependencies();
    assert_eq!(deps, ["openbmt-core", "openbmt-counter", "bitcoin"]);
    let _: &NewRequest = &bitcoin_request("x", 1, today());
}
