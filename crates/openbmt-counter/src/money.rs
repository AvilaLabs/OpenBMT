//! Money: amounts in the shop's currency, rates, quotes and the two
//! conversions (CR-05, CR-06, CR-08).
//!
//! All arithmetic is in `u128`, wide enough for the largest product, and
//! every step is checked.

use std::fmt;

use openbmt_core::currency::ShopCurrency;

/// Satoshis in one bitcoin.
pub const SATS_PER_BTC: u64 = 100_000_000;
/// The most satoshis that can exist.
pub const MAX_SATS: u64 = 21_000_000 * SATS_PER_BTC;
/// CR-05: how long a quote is good for, in seconds.
pub const QUOTE_SECS: u64 = 300;
/// CR-58: the most seconds a rate may have been fetched before the time of
/// the quote.
pub const MAX_RATE_AGE_SECS: u64 = 600;
/// CR-58: the most seconds a rate may have been fetched after the time of
/// the quote (the clocks of two devices do not agree to the second).
pub const MAX_RATE_AHEAD_SECS: u64 = 60;
/// CR-57: the longest label of a rate source, in bytes.
pub const MAX_SOURCE_LABEL_BYTES: usize = 32;
/// CR-197: the label of the rate of Bitcoin Standard, which has no source.
pub const SATS_LABEL: &str = "sats";

/// An amount in minor units of the shop's currency (cents, for dollars;
/// satoshis, for Bitcoin Standard).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Amount(pub u64);

/// A currency code: three upper-case ASCII letters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Currency([u8; 3]);

impl Currency {
    /// A currency from its three letters; `None` unless all are upper-case
    /// ASCII letters.
    pub fn new(code: [u8; 3]) -> Option<Self> {
        code.iter()
            .all(u8::is_ascii_uppercase)
            .then_some(Self(code))
    }

    /// The three letters.
    pub fn code(&self) -> [u8; 3] {
        self.0
    }

    /// The currency of a shop's settings (CR-196): its code, `SAT` for
    /// Bitcoin Standard.
    pub fn of_shop(currency: ShopCurrency) -> Self {
        let mut code = *b"SAT";
        for (to, from) in code.iter_mut().zip(currency.code().bytes()) {
            *to = from;
        }
        Self(code)
    }
}

impl fmt::Display for Currency {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let [a, b, c] = self.0;
        write!(f, "{}{}{}", char::from(a), char::from(b), char::from(c))
    }
}

/// A rate the caller supplies: minor units of the currency per bitcoin. It is
/// never zero.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Rate {
    minor_per_btc: u64,
}

impl Rate {
    /// A rate; `None` for zero.
    pub fn new(minor_per_btc: u64) -> Option<Self> {
        (minor_per_btc > 0).then_some(Self { minor_per_btc })
    }

    /// Minor units per bitcoin.
    pub fn minor_per_btc(&self) -> u64 {
        self.minor_per_btc
    }

    /// CR-197: the rate of Bitcoin Standard, where the minor unit is the
    /// satoshi: 100,000,000 of them to a bitcoin, so an amount is its own
    /// satoshis. It is not a price and no source gave it.
    pub fn sats() -> Self {
        Self {
            minor_per_btc: SATS_PER_BTC,
        }
    }
}

/// CR-57: where a rate came from, as the caller says. The counter does not
/// fetch rates and does not read the label; it writes down what it is told.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RateSource {
    label: String,
    fetched_at: u64,
}

/// CR-57: a source label that is not allowed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BadSourceLabel;

impl BadSourceLabel {
    /// The rule the refusal belongs to.
    pub fn rule(&self) -> &'static str {
        "CR-57"
    }
}

impl fmt::Display for BadSourceLabel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "CR-57: a rate source label is 1 to 32 bytes of a-z, 0-9, - + and ."
        )
    }
}

impl std::error::Error for BadSourceLabel {}

/// Whether the bytes are a label CR-57 allows.
pub(crate) fn label_is_valid(label: &[u8]) -> bool {
    (1..=MAX_SOURCE_LABEL_BYTES).contains(&label.len())
        && label
            .iter()
            .all(|b| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'-' | b'+' | b'.'))
}

impl RateSource {
    /// A source with a label of 1 to 32 bytes from `a-z 0-9 - + .` and the
    /// time, in Unix seconds, at which the caller got the rate.
    pub fn new(label: &str, fetched_at: u64) -> Result<Self, BadSourceLabel> {
        if !label_is_valid(label.as_bytes()) {
            return Err(BadSourceLabel);
        }
        Ok(Self {
            label: label.to_string(),
            fetched_at,
        })
    }

    /// CR-197: the source of the rate of Bitcoin Standard: the label `sats`,
    /// at the time of the sale.
    pub fn sats(fetched_at: u64) -> Self {
        Self {
            label: SATS_LABEL.to_string(),
            fetched_at,
        }
    }

    /// The label.
    pub fn label(&self) -> &str {
        &self.label
    }

    /// When the caller got the rate, in Unix seconds.
    pub fn fetched_at(&self) -> u64 {
        self.fetched_at
    }

    /// CR-58: whether a rate fetched at this time may be used for a quote
    /// made at `now`.
    pub fn is_fresh_at(&self, now: u64) -> bool {
        self.fetched_at.saturating_add(MAX_RATE_AGE_SECS) >= now
            && self.fetched_at <= now.saturating_add(MAX_RATE_AHEAD_SECS)
    }
}

/// CR-08: the satoshis an amount needs at a rate, rounded up to a whole
/// satoshi: `ceil(minor * 100_000_000 / minor_per_btc)`. `None` when the
/// answer is above 21,000,000 bitcoin.
pub fn sats_for(amount: Amount, rate: Rate) -> Option<u64> {
    let numerator = u128::from(amount.0).checked_mul(u128::from(SATS_PER_BTC))?;
    let divisor = u128::from(rate.minor_per_btc);
    let rounded = numerator.checked_add(divisor.checked_sub(1)?)?;
    let sats = u64::try_from(rounded.checked_div(divisor)?).ok()?;
    (sats <= MAX_SATS).then_some(sats)
}

/// CR-06: what satoshis are worth at a rate, rounded down:
/// `floor(sats * minor_per_btc / 100_000_000)`, saturating to the largest
/// amount.
pub fn value_of(sats: u64, rate: Rate) -> Amount {
    let product = u128::from(sats).saturating_mul(u128::from(rate.minor_per_btc));
    let minor = product.checked_div(u128::from(SATS_PER_BTC)).unwrap_or(0);
    Amount(u64::try_from(minor).unwrap_or(u64::MAX))
}

/// CR-188: a tip of `percent` whole percent of `amount`, rounded half up to
/// a whole minor unit: `floor((amount * percent + 50) / 100)`. `None` only
/// when the product does not fit, which an amount of a real sale never does.
pub fn tip_for_percent(amount: Amount, percent: u8) -> Option<Amount> {
    let product = u128::from(amount.0).checked_mul(u128::from(percent))?;
    let tip = product.checked_add(50)?.checked_div(100)?;
    u64::try_from(tip).ok().map(Amount)
}

/// CR-188: one of the tips the counter offers for a sale: a suggested
/// percent of the settings and what it comes to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TipChoice {
    /// The suggested percent, from the signed settings.
    pub percent: u8,
    /// The percent of the sale's amount, rounded half up.
    pub tip: Amount,
}

/// A rate and the seconds it is good for (CR-05).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Quote {
    /// The rate.
    pub rate: Rate,
    /// First second at which the quote holds.
    pub from: u64,
    /// First second at which it no longer holds (`from` + [`QUOTE_SECS`]).
    pub until: u64,
}

impl Quote {
    /// A quote that holds from `from` for [`QUOTE_SECS`] seconds. The end
    /// saturates at the largest time.
    pub fn new(rate: Rate, from: u64) -> Self {
        Self {
            rate,
            from,
            until: from.saturating_add(QUOTE_SECS),
        }
    }

    /// A quote that holds from `from` for `secs` seconds: the one quote of a
    /// request to send (CR-204). The end saturates at the largest time.
    pub fn lasting(rate: Rate, from: u64, secs: u64) -> Self {
        Self {
            rate,
            from,
            until: from.saturating_add(secs),
        }
    }

    /// Whether the quote holds at `now`.
    pub fn holds(&self, now: u64) -> bool {
        self.from <= now && now < self.until
    }
}

/// `x * bps / 10_000`, rounded down: the tolerance on `x`. The same formula
/// `SaleWatch` uses for its required amount (WR-32).
pub fn tolerance(x: u64, bps: u32) -> u64 {
    let t = u128::from(x).saturating_mul(u128::from(bps)) / 10_000;
    u64::try_from(t).unwrap_or(u64::MAX)
}

/// CR-16: whether `received` is enough for `need`: something has arrived and
/// `received` plus the tolerance on `need` reaches it.
pub fn is_enough(received: u64, need: u64, bps: u32) -> bool {
    received > 0 && u128::from(received) + u128::from(tolerance(need, bps)) >= u128::from(need)
}
