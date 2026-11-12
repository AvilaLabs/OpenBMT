#![no_main]

use std::sync::OnceLock;

use bitcoin::ScriptBuf;
use libfuzzer_sys::fuzz_target;
use openbmt_core::currency::ShopCurrency;
use openbmt_counter::money::{Rate, RateSource};
use openbmt_requests::{
    day_of, Arrival, CheckedRate, Every, NewRequest, PriceIn, RequestBook, ReturnAmount, Shop,
};

const T0: u64 = 1_800_000_000;

/// The bytes of a valid book with a little of everything in it, so that
/// mutated bytes get past the header and the counts.
fn valid() -> &'static [u8] {
    static BOOK: OnceLock<Vec<u8>> = OnceLock::new();
    BOOK.get_or_init(|| {
        let shop = Shop {
            currency: ShopCurrency::Usd,
            min_sale_sats: 5_000,
            tz_offset_secs: 0,
        };
        let rate = CheckedRate {
            rate: Rate::new(10_000_000).unwrap(),
            source: RateSource::new("fuzz", T0).unwrap(),
        };
        let today = day_of(T0, 0);
        let new = |note: &str, amount: u64, usd: bool, repeat: Option<Every>| NewRequest {
            note: note.to_string(),
            amount,
            price_in: if usd {
                PriceIn::ShopCurrency
            } else {
                PriceIn::Bitcoin
            },
            due: openbmt_requests::Day(today.0 + 5),
            repeat,
        };
        let arrival = |n: u8, h: u32, t: u64, sats: u64| Arrival {
            txid: [n; 32],
            vout: 0,
            height: h,
            block_time: t,
            sats,
        };
        let mut book = RequestBook::new();
        book.make(&new("a", 100_000, false, None), 0, T0, &shop, None).unwrap();
        book.credit(1, &[arrival(1, 10, T0, 130_000)], T0, &shop, None).unwrap();
        let script = ScriptBuf::from_bytes(
            [&[0x00, 0x14][..], &[7u8; 20][..]].concat(),
        );
        book.queue_return(1, ReturnAmount::Sats(5_000), script, T0, None).unwrap();
        book.make(&new("b", 25_000, true, None), 1, T0, &shop, Some(&rate)).unwrap();
        book.share(2, T0, Some(&rate)).unwrap();
        book.credit(2, &[arrival(2, 11, T0 + 5, 60_000)], T0 + 9, &shop, None).unwrap();
        book.make(&new("c", 50_000, false, None), 2, T0, &shop, None).unwrap();
        book.cancel(3, T0).unwrap();
        book.credit(3, &[arrival(3, 12, T0 + 5, 9_000)], T0 + 9, &shop, None).unwrap();
        book.make(&new("d", 50_000, false, Some(Every::Week)), 3, T0, &shop, None).unwrap();
        book.stop_repeating(4).unwrap();
        book.to_bytes()
    })
}

fuzz_target!(|data: &[u8]| {
    // The input as it is, and as a list of changes (a position of two
    // bytes and a value) to a valid book.
    let mut inputs = vec![data.to_vec()];
    let mut patched = valid().to_vec();
    for edit in data.chunks_exact(3) {
        let at = usize::from(u16::from_le_bytes([edit[0], edit[1]])) % patched.len();
        patched[at] = edit[2];
    }
    inputs.push(patched);
    for bytes in inputs {
        if let Ok(book) = RequestBook::from_bytes(&bytes) {
            assert_eq!(book.to_bytes(), bytes);
            // What is read can be used without a panic.
            let today = day_of(T0, 0);
            let _ = book.list(today);
            let _ = book.overdue_count(today);
            let _ = book.repeats_due(openbmt_requests::Day(today.0 + 400));
            let _ = book.ledger_lines();
            let _ = book.payments_by_day(3_600);
            let _ = book.returns_to_pay();
            let _ = book.address_indexes();
            for r in book.requests() {
                let _ = r.status(5_000);
                let _ = r.marks();
                let _ = book.returnable_sats(r.number);
            }
        }
    }
});
