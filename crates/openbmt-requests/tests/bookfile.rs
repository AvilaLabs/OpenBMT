//! RQ-30 to RQ-36: the request book as a file, the ledger line, hostile
//! input and the words.

mod common;

use std::fs;
use std::path::PathBuf;

use common::*;
use openbmt_core::say::{check, Words};
use openbmt_requests::say::{
    book_unreadable, due_fact, empty_list, list_row, mark_lines, old_price, overdue_row,
    return_added, returned_line, state_line,
};
use openbmt_requests::{
    check_return_address, BookError, CreditRefused, EndRefused, Every, Limit, MakeRefused, Marks,
    RepeatRefused, RequestBook, ReturnAmount, ReturnRefused, ShareRefused, Status,
    MAGIC_REQUEST_BOOK, REQUEST_BOOK_FILE,
};

/// A book with every kind of thing in it.
pub fn rich_book() -> RequestBook {
    let mut book = RequestBook::new();
    let s = shop();
    // 1: bitcoin, part paid, with returns in every state.
    let a = make_btc(&mut book, 0, 100_000, 7);
    book.share(a, T0, None).unwrap();
    book.credit(a, &[arrival(1, 10, T0 + 5, 130_000)], T0 + 9, &s, None)
        .unwrap();
    for sats in [5_000, 6_000, 7_000] {
        book.queue_return(
            a,
            ReturnAmount::Sats(sats),
            script_of(P2WPKH),
            T0 + 10,
            None,
        )
        .unwrap();
    }
    book.mark_returns_sent(&[1, 2], [3; 32]);
    book.mark_returns_done([3; 32], T0 + 800);
    book.mark_returns_sent(&[3], [4; 32]);
    // 2: dollars, shared twice, an old price, late.
    let b = make_usd(&mut book, 1, 25_000, 1);
    book.share(b, T0, Some(&fresh(T0))).unwrap();
    book.share(b, T0 + 100, Some(&rate(2 * RATE, T0 + 100)))
        .unwrap();
    let late = T0 - 8 * 3600 + 3 * DAY;
    book.credit(b, &[arrival(2, 20, late, 60_000)], late, &s, None)
        .unwrap();
    // 3: cancelled, then paid.
    let c = make_btc(&mut book, 2, 50_000, 3);
    book.cancel(c, T0 + 1).unwrap();
    book.credit(c, &[arrival(3, 11, T0 + 50, 9_000)], T0 + 60, &s, None)
        .unwrap();
    // 4: closed, then paid.
    let d = make_btc(&mut book, 3, 50_000, 3);
    book.credit(d, &[arrival(4, 12, T0 + 50, 9_000)], T0 + 60, &s, None)
        .unwrap();
    book.close(d, T0 + 70, &s).unwrap();
    book.credit(d, &[arrival(5, 13, T0 + 80, 2_000)], T0 + 90, &s, None)
        .unwrap();
    // 5: almost paid, counted as paid.
    let e = make_usd(&mut book, 4, 25_000, 3);
    book.share(e, T0, Some(&fresh(T0))).unwrap();
    book.credit(e, &[arrival(6, 14, T0 + 5, 247_000)], T0 + 9, &s, None)
        .unwrap();
    book.count_as_paid(e, T0 + 20, &s).unwrap();
    // 6..: a monthly series with a catch-up, stopped.
    let new = repeating(
        bitcoin_request("Stall fee", 50_000, day(2027, 1, 31)),
        Every::Month,
    );
    book.make(&new, 5, T0, &s, None).unwrap();
    for (i, d) in book.repeats_due(day(2027, 4, 1)).iter().enumerate() {
        book.make_repeat(d, 6 + i as u32, T0 + 70 * DAY, 0).unwrap();
    }
    book.stop_repeating(6).unwrap();
    book
}

fn dir(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("obm-requests-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&path);
    fs::create_dir_all(&path).unwrap();
    path
}

#[test]
fn rq_30_the_book_has_its_own_magic_and_version() {
    assert_eq!(MAGIC_REQUEST_BOOK, *b"OBM=");
    let bytes = RequestBook::new().to_bytes();
    assert_eq!(&bytes[..4], b"OBM=");
    assert_eq!(bytes[4], 1);
    assert_eq!(RequestBook::from_bytes(&bytes).unwrap(), RequestBook::new());
}

#[test]
fn rq_30_a_book_with_everything_in_it_reads_back_the_same_and_encodes_to_the_same_bytes() {
    let book = rich_book();
    assert_eq!(book.requests().len(), 8);
    assert!(book.returns().len() == 3);
    let bytes = book.to_bytes();
    let again = RequestBook::from_bytes(&bytes).unwrap();
    assert_eq!(again, book);
    assert_eq!(again.to_bytes(), bytes);
}

#[test]
fn rq_30_another_magic_or_version_is_refused() {
    let bytes = rich_book().to_bytes();
    let mut other = bytes.clone();
    other[3] = b'!';
    assert!(RequestBook::from_bytes(&other).is_err());
    let mut other = bytes.clone();
    other[4] = 2;
    assert!(RequestBook::from_bytes(&other).is_err());
    other[4] = 0;
    assert!(RequestBook::from_bytes(&other).is_err());
    assert!(RequestBook::from_bytes(b"").is_err());
    assert!(RequestBook::from_bytes(b"OBM").is_err());
}

#[test]
fn rq_30_a_count_over_its_limit_is_refused() {
    let head = |stopped: u32, requests: u32| {
        let mut b = b"OBM=\x01".to_vec();
        b.extend_from_slice(&1u64.to_le_bytes());
        b.extend_from_slice(&1u64.to_le_bytes());
        b.extend_from_slice(&stopped.to_le_bytes());
        b.extend_from_slice(&requests.to_le_bytes());
        b
    };
    for (stopped, requests) in [(100_001, 0), (0, 100_001), (0, u32::MAX), (u32::MAX, 0)] {
        assert!(RequestBook::from_bytes(&head(stopped, requests)).is_err());
    }
    // The empty book with its returns count is the shortest that reads.
    let mut b = head(0, 0);
    b.extend_from_slice(&100_001u32.to_le_bytes());
    assert_eq!(
        RequestBook::from_bytes(&b),
        Err(BookError::Corrupt("a count is over its limit"))
    );
    b.truncate(b.len() - 4);
    b.extend_from_slice(&0u32.to_le_bytes());
    assert!(RequestBook::from_bytes(&b).is_ok());
}

#[test]
fn rq_30_bytes_left_over_are_refused() {
    let mut more = rich_book().to_bytes();
    more.push(0);
    assert_eq!(
        RequestBook::from_bytes(&more),
        Err(BookError::Corrupt("bytes follow the last return"))
    );
}

#[test]
fn rq_30_the_book_is_written_by_atomic_steps_and_read_back() {
    let d = dir("atomic");
    let book = rich_book();
    book.save(&d).unwrap();
    assert_eq!(
        fs::read(d.join(REQUEST_BOOK_FILE)).unwrap(),
        book.to_bytes()
    );
    assert!(
        !d.join(format!("{REQUEST_BOOK_FILE}.tmp")).exists(),
        "the new file was renamed"
    );
    assert_eq!(RequestBook::open(&d).unwrap(), book);
    // A second save replaces the first whole.
    let mut next = book.clone();
    make_btc(&mut next, 900, 50_000, 3);
    next.save(&d).unwrap();
    assert_eq!(RequestBook::open(&d).unwrap(), next);
    // A folder that is not there is an error and changes nothing.
    assert!(matches!(
        book.save(&d.join("missing")),
        Err(BookError::Io(_))
    ));
    assert_eq!(RequestBook::open(&d).unwrap(), next);
    fs::remove_dir_all(&d).unwrap();
}

#[test]
fn rq_30_no_file_is_an_empty_book() {
    let d = dir("empty");
    let book = RequestBook::open(&d).unwrap();
    assert_eq!(book, RequestBook::new());
    assert_eq!(book.next_number(), 1);
    fs::remove_dir_all(&d).unwrap();
}

#[test]
fn rq_31_a_book_that_cannot_be_read_is_not_replaced_and_gives_no_book_to_make_a_request_in() {
    let d = dir("unreadable");
    let mut junk = rich_book().to_bytes();
    junk.truncate(junk.len() / 2);
    fs::write(d.join(REQUEST_BOOK_FILE), &junk).unwrap();
    let err = RequestBook::open(&d).unwrap_err();
    assert!(matches!(err, BookError::Corrupt(_)));
    // Nothing was written over it, and there is no book to make a request in.
    assert_eq!(fs::read(d.join(REQUEST_BOOK_FILE)).unwrap(), junk);
    assert!(!d.join(format!("{REQUEST_BOOK_FILE}.tmp")).exists());
    // The words are those of RQ-31.
    let w = book_unreadable(&err);
    assert_eq!(w.heading, "Requests cannot be read");
    assert!(w.next.starts_with("Do not clear the app's data."));
    assert_eq!(check(&w), Ok(()));
    fs::remove_dir_all(&d).unwrap();
}

#[test]
fn rq_31_a_file_that_cannot_be_opened_at_all_is_an_error_and_not_an_empty_book() {
    let d = dir("folder");
    // The name is a folder: not "no file", so not a fresh book.
    fs::create_dir_all(d.join(REQUEST_BOOK_FILE)).unwrap();
    assert!(matches!(RequestBook::open(&d), Err(BookError::Io(_))));
    fs::remove_dir_all(&d).unwrap();
}

#[test]
fn rq_32_each_request_is_a_ledger_line_with_its_payments_marks_and_returns() {
    let book = rich_book();
    let lines = book.ledger_lines();
    assert_eq!(lines.len(), book.requests().len());
    let one = &lines[0];
    assert_eq!((one.number, one.note.as_str()), (1, "Booking"));
    assert_eq!(
        (one.amount, one.made_at, one.made, one.due),
        (100_000, T0, today(), in_days(7))
    );
    assert_eq!(one.priced_in, openbmt_requests::Pricing::Bitcoin);
    assert_eq!(one.payments.len(), 1);
    assert_eq!(
        (
            one.payments[0].block_time,
            one.payments[0].sats,
            one.payments[0].credit
        ),
        (T0 + 5, 130_000, 130_000)
    );
    assert_eq!(one.returns.len(), 3);
    assert_eq!(
        one.returns.iter().filter(|r| r.done_at.is_some()).count(),
        2
    );
    // The marks and the ends.
    let two = &lines[1];
    assert!(two.marks.late && two.marks.old_price);
    assert_eq!(
        two.priced_in,
        openbmt_requests::Pricing::Currency(openbmt_core::currency::ShopCurrency::Usd)
    );
    assert_eq!(two.payments[0].credit, 12_000);
    assert!(lines[2].marks.after_cancel && lines[2].ended.is_some());
    assert!(lines[3].marks.after_close);
    assert_eq!(
        lines[4].ended.map(|e| e.0),
        Some(openbmt_requests::End::CountedPaid)
    );
}

#[test]
fn rq_32_the_end_of_day_totals_count_a_payment_on_the_day_of_its_block() {
    let mut book = RequestBook::new();
    let n = make_btc(&mut book, 0, 100_000, 7);
    // 23:30 UTC on the 15th and 00:30 UTC on the 16th.
    let before = T0 - 8 * 3600 + 23 * 3600 + 1_800;
    let after = before + 3_600;
    book.credit(
        n,
        &[
            arrival(1, 10, before, 10_000),
            arrival(2, 11, after, 20_000),
        ],
        after,
        &shop(),
        None,
    )
    .unwrap();
    let utc = book.payments_by_day(0);
    assert_eq!(utc[&today()], 10_000);
    assert_eq!(utc[&in_days(1)], 20_000);
    // At UTC+1 both are on the 16th; at UTC-1 both are on the 15th.
    let plus = book.payments_by_day(3_600);
    assert_eq!(plus.len(), 1);
    assert_eq!(plus[&in_days(1)], 30_000);
    assert_eq!(book.payments_by_day(-3_600)[&today()], 30_000);
    // A payment is counted once, not once per share.
    assert_eq!(book.payments_by_day(0).values().sum::<u64>(), 30_000);
}

#[test]
fn rq_33_a_return_address_of_any_length_or_content_never_panics_and_is_refused_unless_it_meets_rq_28(
) {
    let mut rng = Rng(0xA11CE);
    let mut accepted = 0;
    let seeds = [P2WPKH, P2WSH, P2TR, P2PKH, P2SH, TESTNET];
    for round in 0..6_000u64 {
        let seed = seeds[(round % 6) as usize];
        let mut bytes = seed.as_bytes().to_vec();
        for _ in 0..rng.range(0, 3) {
            if bytes.is_empty() {
                break;
            }
            let at = rng.range(0, bytes.len() as u64 - 1) as usize;
            match rng.range(0, 3) {
                0 => bytes[at] = rng.next() as u8,
                1 => {
                    bytes.remove(at);
                }
                2 => bytes.insert(at, rng.next() as u8),
                _ => bytes.truncate(at),
            }
        }
        if round % 7 == 0 {
            bytes = (0..rng.range(0, 400)).map(|_| rng.next() as u8).collect();
        }
        let text = String::from_utf8_lossy(&bytes).to_string();
        if let Ok(script) = check_return_address(&text, |_| false) {
            accepted += 1;
            // It is one of the five kinds and its mainnet address reads back to it.
            let shown = openbmt_requests::address_in_groups(&script)
                .unwrap()
                .replace(' ', "");
            assert_eq!(check_return_address(&shown, |_| false), Ok(script));
        }
    }
    assert!(
        accepted > 100,
        "the seeds themselves are accepted: {accepted}"
    );
    // Long and odd inputs.
    for text in [
        "x".repeat(100_000),
        "\u{0}".repeat(50),
        "\u{202e}bc1q".to_string(),
        format!("{P2WPKH}\u{0}"),
        format!("{P2WPKH}{P2WPKH}"),
        "\u{1f600}".repeat(40),
        format!("{P2WPKH} {P2WPKH}"),
    ] {
        assert!(
            check_return_address(&text, |_| false).is_err(),
            "{:?}",
            &text[..text.len().min(20)]
        );
    }
}

#[test]
fn rq_34_a_book_of_any_bytes_never_panics_and_is_refused_unless_it_meets_rq_30() {
    let good = rich_book().to_bytes();
    let mut rng = Rng(0xB00C);
    // Every cut.
    for n in 0..good.len() {
        assert!(RequestBook::from_bytes(&good[..n]).is_err(), "cut at {n}");
    }
    // Every byte changed three ways: refused, or read and encoding to the same bytes.
    let mut read = 0;
    for at in 0..good.len() {
        for change in [|b: u8| b ^ 1, |b: u8| b ^ 0x80, |b: u8| b.wrapping_add(1)] {
            let mut bytes = good.clone();
            bytes[at] = change(bytes[at]);
            if let Ok(book) = RequestBook::from_bytes(&bytes) {
                read += 1;
                assert_eq!(book.to_bytes(), bytes, "byte {at}");
            }
        }
    }
    assert!(read > 0, "some changes are to fields that stay in range");
    // Random bytes, and random bytes behind a good header.
    for _ in 0..3_000 {
        let len = rng.range(0, 600) as usize;
        let mut bytes: Vec<u8> = (0..len).map(|_| rng.next() as u8).collect();
        let _ = RequestBook::from_bytes(&bytes);
        let mut framed = b"OBM=\x01".to_vec();
        framed.append(&mut bytes);
        let _ = RequestBook::from_bytes(&framed);
    }
}

#[test]
fn rq_35_the_owner_screens_have_their_words() {
    // Home, the list, one request's page and the return steps.
    assert_eq!(overdue_row(2), "2 overdue");
    assert_eq!(empty_list().0, "No open requests");
    assert_eq!(
        empty_list().1,
        "Requests you make show here until they are paid."
    );
    assert_eq!(
        list_row(12, "$40", "3 March"),
        "No. 12 \u{b7} $40 still owed \u{b7} due 3 March"
    );
    assert_eq!(due_fact("3 March"), "Due 3 March");
    assert_eq!(
        returned_line("$40", "3 March"),
        "Returned $40 \u{b7} 3 March"
    );
    let amount = |n: u64| format!("${n}");
    for (status, line) in [
        (Status::Waiting, "Waiting for payment"),
        (Status::PartPaid { owed: 5 }, "$5 still owed"),
        (Status::Paid, "Paid"),
        (Status::Overpaid { excess: 2 }, "Paid, with $2 extra"),
        (Status::Almost { short: 3 }, "Almost paid: $3 short"),
    ] {
        assert_eq!(state_line(&status, &amount), line);
    }
    assert_eq!(mark_lines(&Marks::default()), Vec::<&str>::new());
    assert_eq!(check(&return_added("$40", 3)), Ok(()));
    assert_eq!(check(&old_price()), Ok(()));
}

#[test]
fn rq_36_every_refusal_has_words_that_name_its_rule() {
    let said: Vec<Words> = vec![
        openbmt_requests::say::make_refused(&MakeRefused::NoteNeeded),
        openbmt_requests::say::make_refused(&MakeRefused::BitcoinOnly),
        openbmt_requests::say::make_refused(&MakeRefused::DueOutOfRange),
        openbmt_requests::say::make_refused(&MakeRefused::TooManyOpen),
        openbmt_requests::say::make_refused(&MakeRefused::TooSmall { min_sats: 5_000 }),
        openbmt_requests::say::make_refused(&MakeRefused::TooLarge {
            max: Limit::Sats(1),
        }),
        openbmt_requests::say::make_refused(&MakeRefused::StaleRate),
        openbmt_requests::say::make_refused(&MakeRefused::IndexUsed),
        openbmt_requests::say::make_refused(&MakeRefused::BookFull),
        openbmt_requests::say::share_refused(&ShareRefused::NotOpen),
        openbmt_requests::say::share_refused(&ShareRefused::StaleRate),
        openbmt_requests::say::credit_refused(&CreditRefused::NoRate),
        openbmt_requests::say::end_refused(&EndRefused::HasPayments),
        openbmt_requests::say::repeat_refused(&RepeatRefused::NotDue),
        openbmt_requests::say::return_refused(&ReturnRefused::TooMuch { most_sats: 5 }),
        openbmt_requests::say::return_refused(&ReturnRefused::TooSmall { min_sats: 294 }),
        openbmt_requests::say::return_refused(&ReturnRefused::QueueFull),
    ];
    for w in said {
        assert_eq!(check(&w), Ok(()));
        assert!(w.code.is_some_and(|c| c.starts_with("RQ-")));
    }
}
