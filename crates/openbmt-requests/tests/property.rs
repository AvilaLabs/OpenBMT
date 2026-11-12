//! Properties over random sequences of calls, from fixed seeds (no new
//! dependency): after every call the book is one the strict reader accepts,
//! and a refused call changes nothing.

mod common;

use common::*;
use openbmt_requests::date::{civil_from_days, days_from_civil, days_in_month, MAX_DAY};
use openbmt_requests::{check_return_address, Day, Every, RequestBook, ReturnAmount, Status};

fn random_every(rng: &mut Rng) -> Every {
    [Every::Week, Every::Month, Every::Year][rng.range(0, 2) as usize]
}

struct World {
    book: RequestBook,
    now: u64,
    next_index: u32,
    next_tx: u32,
    height: u32,
}

impl World {
    fn new() -> Self {
        Self {
            book: RequestBook::new(),
            now: T0,
            next_index: 0,
            next_tx: 1,
            height: 100,
        }
    }

    fn today(&self) -> Day {
        openbmt_requests::day_of(self.now, 0)
    }

    fn index(&mut self) -> u32 {
        self.next_index += 1;
        self.next_index - 1
    }

    fn tx(&mut self, sats: u64, time: u64) -> openbmt_requests::Arrival {
        self.next_tx += 1;
        self.height += 1;
        let mut txid = [0u8; 32];
        txid[..4].copy_from_slice(&self.next_tx.to_le_bytes());
        openbmt_requests::Arrival {
            txid,
            vout: 0,
            height: self.height,
            block_time: time,
            sats,
        }
    }

    /// One random call. A refused call must leave the book as it was.
    fn step(&mut self, rng: &mut Rng) {
        let before = self.book.clone();
        let count = self.book.requests().len() as u64;
        let pick = if count == 0 { 0 } else { rng.range(1, count) };
        let s = shop();
        let ok = match rng.range(0, 13) {
            0 | 1 => {
                let index = self.index();
                let due = Day(self.today().0 + rng.range(0, 40) as u32);
                let mut new = if rng.range(0, 1) == 0 {
                    bitcoin_request("Req", rng.range(500, 400_000), due)
                } else {
                    usd_request("Req", rng.range(100, 90_000), due)
                };
                if rng.range(0, 3) == 0 {
                    new.repeat = Some(random_every(rng));
                }
                self.book
                    .make(&new, index, self.now, &s, Some(&fresh(self.now)))
                    .is_ok()
            }
            2 | 3 => self
                .book
                .share(pick, self.now, Some(&fresh(self.now)))
                .is_ok(),
            4..=6 => {
                let sats = rng.range(1_000, 300_000);
                let time = self.now - rng.range(0, 7_200);
                let a = self.tx(sats, time);
                self.book
                    .credit(pick, &[a], self.now, &s, Some(&fresh(self.now)))
                    .is_ok()
            }
            7 => self.book.cancel(pick, self.now).is_ok(),
            8 => self.book.close(pick, self.now, &s).is_ok(),
            9 => self.book.count_as_paid(pick, self.now, &s).is_ok(),
            10 => {
                let script =
                    script_of([P2WPKH, P2WSH, P2TR, P2PKH, P2SH][rng.range(0, 4) as usize]);
                let amount = if rng.range(0, 1) == 0 {
                    ReturnAmount::Sats(rng.range(200, 200_000))
                } else {
                    ReturnAmount::Units(rng.range(50, 20_000))
                };
                self.book
                    .queue_return(pick, amount, script, self.now, Some(&fresh(self.now)))
                    .is_ok()
            }
            11 => {
                let ids = self.book.queued_return_ids();
                let take = ids.len().min(rng.range(0, 5) as usize);
                let txid = [rng.range(1, 255) as u8; 32];
                if self.book.mark_returns_sent(&ids[..take], txid) > 0 {
                    if rng.range(0, 1) == 0 {
                        self.book.mark_returns_done(txid, self.now);
                    } else {
                        self.book.requeue_returns(txid);
                    }
                }
                true
            }
            _ => {
                // Open the app some days later: make what the series are due.
                self.now += rng.range(0, 20) * DAY;
                let today = self.today();
                let due = self.book.repeats_due(today);
                assert!(due.len() <= 12);
                let mut all = true;
                for d in due {
                    let index = self.index();
                    all &= self.book.make_repeat(&d, index, self.now, 0).is_ok();
                }
                all
            }
        };
        if !ok {
            assert_eq!(self.book, before, "a refused call changed the book");
        }
        self.now += rng.range(0, 3_000);
    }

    fn check(&self) {
        let book = &self.book;
        // The strict reader accepts it and it encodes to the same bytes.
        let bytes = book.to_bytes();
        let back = RequestBook::from_bytes(&bytes).expect("the book reads back");
        assert_eq!(&back, book);
        assert_eq!(back.to_bytes(), bytes);
        assert!(book.open_count() <= 1_000);
        // Numbers count up from 1 with none missing; indexes are never shared.
        for (i, r) in book.requests().iter().enumerate() {
            assert_eq!(r.number, i as u64 + 1);
        }
        let mut indexes = book.address_indexes();
        let n = indexes.len();
        indexes.dedup();
        assert_eq!(indexes.len(), n);
        for r in book.requests() {
            // What is owed is never below zero and never above the amount.
            assert!(r.owed() <= r.amount);
            let status = r.status(5_000);
            match status {
                Status::Waiting => assert!(r.credits.iter().all(|c| c.after.is_some())),
                Status::PartPaid { owed } | Status::Almost { short: owed } => {
                    if r.end.is_none() {
                        assert_eq!(owed, r.owed());
                    }
                }
                Status::Paid | Status::Overpaid { .. } => {}
            }
            if let Status::Overpaid { excess } = status {
                assert_eq!(excess, r.credited() - r.amount);
            }
            // Money returned is never more than what arrived.
            let returned: u64 = book
                .returns()
                .iter()
                .filter(|x| x.request == r.number)
                .map(|x| x.sats)
                .sum();
            assert!(returned <= r.received_sats());
            assert_eq!(
                book.returnable_sats(r.number),
                Some(r.received_sats() - returned)
            );
            assert!(r.quotes.len() <= 64);
            // A cancelled request never had a payment counted.
            if matches!(r.end, Some((openbmt_requests::End::Cancelled, _))) {
                assert_eq!(r.credited(), 0);
            }
        }
        assert!(book.queued_return_ids().len() <= 99);
        assert_eq!(book.returns_to_pay().len(), book.queued_return_ids().len());
    }
}

#[test]
fn rq_30_random_sequences_of_calls_always_leave_a_book_the_strict_reader_accepts() {
    // What the sequences reached, so that the property is known to bite.
    let mut seen = [false; 8];
    for seed in 1..=40u64 {
        let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15));
        let mut world = World::new();
        for _ in 0..120 {
            world.step(&mut rng);
            world.check();
        }
        use openbmt_requests::{End, ReturnState};
        for r in world.book.requests() {
            seen[0] |= matches!(r.end, Some((End::Cancelled, _)));
            seen[1] |= matches!(r.end, Some((End::Closed, _)));
            seen[2] |= r.credits.iter().any(|c| c.after.is_some());
            seen[3] |= matches!(r.status(5_000), Status::Overpaid { .. });
            seen[4] |= r.repeat.is_some_and(|x| x.index > 0);
            seen[5] |= r.credits.iter().any(|c| c.old_price);
        }
        seen[6] |= world
            .book
            .returns()
            .iter()
            .any(|r| matches!(r.state, ReturnState::Done { .. }));
        seen[7] |= world
            .book
            .returns()
            .iter()
            .any(|r| r.state == ReturnState::Queued);
    }
    assert!(seen.iter().all(|s| *s), "{seen:?}");
}

#[test]
fn rq_03_random_sequences_never_give_one_address_index_to_two_requests() {
    let mut rng = Rng(0x1D);
    let mut book = RequestBook::new();
    let mut used = std::collections::BTreeSet::new();
    for _ in 0..600 {
        let index = rng.range(0, 150) as u32;
        let new = bitcoin_request("x", 50_000, in_days(3));
        let made = book.make(&new, index, T0, &shop(), None).is_ok();
        assert_eq!(made, used.insert(index), "index {index}");
        if rng.range(0, 4) == 0 && !book.requests().is_empty() {
            let n = rng.range(1, book.requests().len() as u64);
            let _ = book.cancel(n, T0);
        }
    }
    assert_eq!(book.address_indexes().len(), used.len());
}

#[test]
fn rq_15_crediting_the_same_arrivals_again_changes_nothing_and_order_of_calls_does_not_matter() {
    let mut rng = Rng(0xC0DE);
    for _ in 0..100 {
        let mut book = RequestBook::new();
        let n = make_usd(&mut book, 0, rng.range(10_000, 90_000), 7);
        book.share(n, T0, Some(&fresh(T0))).unwrap();
        let mut arrivals = Vec::new();
        let mut time = T0;
        for i in 0..rng.range(1, 8) {
            time += rng.range(0, 40_000);
            arrivals.push(arrival(
                i as u8 + 1,
                100 + i as u32,
                time,
                rng.range(1_000, 200_000),
            ));
        }
        let mut once = book.clone();
        once.credit(n, &arrivals, time, &shop(), None).unwrap();
        // All at once equals one by one equals the whole list handed over again.
        let mut each = book.clone();
        for a in &arrivals {
            each.credit(n, std::slice::from_ref(a), time, &shop(), None)
                .unwrap();
        }
        assert_eq!(once, each);
        let mut twice = once.clone();
        assert_eq!(twice.credit(n, &arrivals, time, &shop(), None), Ok(0));
        assert_eq!(twice, once);
        // The credit is the satoshis at the rate: never more than their value.
        for c in &once.request(n).unwrap().credits {
            assert!(c.value <= c.sats / 10 + 1);
        }
    }
}

#[test]
fn rq_24_random_series_make_consecutive_requests_with_rising_due_dates_and_at_most_12_at_a_time() {
    let mut rng = Rng(0x5E71);
    for _ in 0..60 {
        let mut book = RequestBook::new();
        let mut next_index = 0;
        for _ in 0..rng.range(1, 3) {
            let first = Day(today().0 + rng.range(0, 60) as u32);
            let new = repeating(bitcoin_request("S", 50_000, first), random_every(&mut rng));
            book.make(&new, next_index, T0, &shop(), None).unwrap();
            next_index += 1;
        }
        let mut now = today();
        for _ in 0..6 {
            now = Day((now.0 + rng.range(0, 500) as u32).min(MAX_DAY.0));
            let due = book.repeats_due(now);
            assert!(due.len() <= 12);
            assert!(due.windows(2).all(|w| w[0].due <= w[1].due), "oldest first");
            for d in &due {
                let n = book
                    .make_repeat(d, next_index, T0 + now.0 as u64 * DAY, 0)
                    .unwrap();
                next_index += 1;
                assert_eq!(book.request(n).unwrap().due, d.due);
            }
        }
        // Within a series the indexes and due dates rise by one step.
        for series in 1..=3u64 {
            let mut last: Option<(u32, Day)> = None;
            for r in book
                .requests()
                .iter()
                .filter(|r| r.repeat.is_some_and(|x| x.series == series))
            {
                let rep = r.repeat.unwrap();
                if let Some((i, d)) = last {
                    assert_eq!(rep.index, i + 1);
                    assert!(r.due > d);
                }
                assert_eq!(rep.every.due(rep.first_due, rep.index), Some(r.due));
                last = Some((rep.index, r.due));
            }
        }
        assert!(RequestBook::from_bytes(&book.to_bytes()).is_ok());
    }
}

#[test]
fn rq_24_the_date_arithmetic_holds_for_random_days_and_months() {
    let mut rng = Rng(0xDA7E);
    for _ in 0..5_000 {
        let d = Day(rng.range(0, 60_000) as u32);
        let (y, m, dd) = d.civil();
        assert_eq!(days_from_civil(y, m, dd), i64::from(d.0));
        assert_eq!(civil_from_days(i64::from(d.0)), (y, m, dd));
        let k = rng.range(0, 40) as u32;
        if let Some(next) = d.plus_months(k) {
            let (ny, nm, nd) = next.civil();
            // The same day of the month, or the last day of a shorter month.
            assert_eq!(nd, dd.min(days_in_month(ny, nm)));
            assert_eq!(
                ny * 12 + i64::from(nm),
                y * 12 + i64::from(m) + i64::from(k)
            );
            // Later months are never earlier.
            if let Some(after) = d.plus_months(k + 1) {
                assert!(after > next);
            }
        }
    }
}

#[test]
fn rq_28_an_accepted_address_is_always_of_a_standard_kind_and_never_the_shops_own() {
    let mut rng = Rng(0xADD2);
    let own = script_of(P2WPKH);
    for _ in 0..4_000 {
        let mut bytes = [P2WPKH, P2TR, P2SH, P2PKH, P2WSH][rng.range(0, 4) as usize]
            .as_bytes()
            .to_vec();
        for _ in 0..rng.range(0, 2) {
            let at = rng.range(0, bytes.len() as u64 - 1) as usize;
            bytes[at] = b"0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHJKLMNPQRSTUVWXYZ"
                [rng.range(0, 59) as usize];
        }
        let text = String::from_utf8(bytes).unwrap();
        if let Ok(script) = check_return_address(&text, |s| *s == *own.as_script()) {
            assert!(
                script.is_p2wpkh()
                    || script.is_p2wsh()
                    || script.is_p2tr()
                    || script.is_p2pkh()
                    || script.is_p2sh()
            );
            assert_ne!(script, own);
        }
    }
}
