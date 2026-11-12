//! The kit the tests share. Nothing here is the code under test: the dates,
//! rates and scripts are written out again.
#![allow(dead_code)]

use std::str::FromStr;

use bitcoin::{Address, ScriptBuf};
use openbmt_core::currency::ShopCurrency;
use openbmt_counter::money::{Rate, RateSource};
use openbmt_requests::{Arrival, CheckedRate, Day, Every, NewRequest, PriceIn, RequestBook, Shop};

/// 2027-01-15 08:00:00 UTC.
pub const T0: u64 = 1_800_000_000;
pub const DAY: u64 = 86_400;

/// A price of $100,000 a bitcoin: 1,000 sats are $1, 100 cents are 100,000 sats.
pub const RATE: u64 = 10_000_000;

pub fn shop() -> Shop {
    Shop {
        currency: ShopCurrency::Usd,
        min_sale_sats: 5_000,
        tz_offset_secs: 0,
    }
}

pub fn shop_in(tz_offset_secs: i32) -> Shop {
    Shop {
        tz_offset_secs,
        ..shop()
    }
}

pub fn sats_shop() -> Shop {
    Shop {
        currency: ShopCurrency::Sats,
        ..shop()
    }
}

/// A rate that was fetched at `fetched_at`.
pub fn rate(minor_per_btc: u64, fetched_at: u64) -> CheckedRate {
    CheckedRate {
        rate: Rate::new(minor_per_btc).unwrap(),
        source: RateSource::new("test", fetched_at).unwrap(),
    }
}

/// The rate of `RATE`, fetched at `at`.
pub fn fresh(at: u64) -> CheckedRate {
    rate(RATE, at)
}

pub fn day(y: i64, m: u32, d: u32) -> Day {
    Day::from_civil(y, m, d).unwrap()
}

/// The day of `T0`.
pub fn today() -> Day {
    day(2027, 1, 15)
}

pub fn in_days(n: u32) -> Day {
    Day(today().0 + n)
}

pub fn bitcoin_request(note: &str, sats: u64, due: Day) -> NewRequest {
    NewRequest {
        note: note.to_string(),
        amount: sats,
        price_in: PriceIn::Bitcoin,
        due,
        repeat: None,
    }
}

pub fn usd_request(note: &str, cents: u64, due: Day) -> NewRequest {
    NewRequest {
        price_in: PriceIn::ShopCurrency,
        ..bitcoin_request(note, cents, due)
    }
}

pub fn repeating(mut new: NewRequest, every: Every) -> NewRequest {
    new.repeat = Some(every);
    new
}

/// Make a request in bitcoin due in `due_in` days, at `T0`, with index
/// `index`.
pub fn make_btc(book: &mut RequestBook, index: u32, sats: u64, due_in: u32) -> u64 {
    book.make(
        &bitcoin_request("Booking", sats, in_days(due_in)),
        index,
        T0,
        &shop(),
        None,
    )
    .unwrap()
}

/// Make a request in dollars, in cents, at `T0` with a fresh rate.
pub fn make_usd(book: &mut RequestBook, index: u32, cents: u64, due_in: u32) -> u64 {
    book.make(
        &usd_request("Booking", cents, in_days(due_in)),
        index,
        T0,
        &shop(),
        Some(&fresh(T0)),
    )
    .unwrap()
}

pub fn arrival(n: u8, height: u32, block_time: u64, sats: u64) -> Arrival {
    Arrival {
        txid: [n; 32],
        vout: 0,
        height,
        block_time,
        sats,
    }
}

// ---- addresses (published test vectors) ------------------------------------

pub const P2WPKH: &str = "bc1qw508d6qejxtdg4y5r3zarvary0c5xw7kv8f3t4";
pub const P2WPKH_UPPER: &str = "BC1QW508D6QEJXTDG4Y5R3ZARVARY0C5XW7KV8F3T4";
pub const P2WSH: &str = "bc1qrp33g0q5c5txsp9arysrx4k6zdkfs4nce4xj0gdcccefvpysxf3qccfmv3";
pub const P2TR: &str = "bc1p0xlxvlhemja6c4dqv22uapctqupfhlxm9h8z3k2e72q4k9hcz7vqzk5jj0";
pub const P2PKH: &str = "1BvBMSEYstWetqTFn5Au4m4GFg7xJaNVN2";
pub const P2SH: &str = "3J98t1WpEZ73CNmQviecrnyiWrnqRhWNLy";
pub const TESTNET: &str = "tb1qw508d6qejxtdg4y5r3zarvary0c5xw7kxpjzsx";
/// A valid address of witness version 16: a kind that is not paid.
pub const WITNESS_V16: &str = "BC1SW50QGDZ25J";

pub fn script_of(address: &str) -> ScriptBuf {
    Address::from_str(address)
        .unwrap()
        .assume_checked()
        .script_pubkey()
}

// ---- a seeded generator ------------------------------------------------------

pub struct Rng(pub u64);

impl Rng {
    pub fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// A number in `lo..=hi`.
    pub fn range(&mut self, lo: u64, hi: u64) -> u64 {
        lo + self.next() % (hi - lo + 1)
    }
}
