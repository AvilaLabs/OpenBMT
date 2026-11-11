//! Several prices into one rate (RT-30 to RT-38).

use openbmt_counter::money::{Rate, RateSource};

use crate::error::{PriceRefused, RatesRefused};
use crate::price::{MAX_CENTS_PER_BTC, MIN_CENTS_PER_BTC};
use crate::source::{position, Source};

/// RT-33: the most a price used may differ from the median, in hundredths
/// of a percent (200 is 2%). **[to tune]**
pub const MAX_SPREAD_BPS: u64 = 200;
/// RT-31: the fewest distinct sources a rate is made from.
pub const MIN_SOURCES: usize = 2;
/// RT-35: the start of the label of a combined rate.
pub const LABEL_PREFIX: &str = "med.";

/// One price from one source, as the app got it (RT-45).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sample {
    source: &'static Source,
    fetched_at: u64,
    minor_per_btc: u64,
}

impl Sample {
    /// A sample: the source, when the app got the reply (Unix seconds) and
    /// the price in minor units per bitcoin. A price outside the band of
    /// RT-24 is refused.
    pub fn new(
        source: &'static Source,
        fetched_at: u64,
        minor_per_btc: u64,
    ) -> Result<Self, PriceRefused> {
        if !(MIN_CENTS_PER_BTC..=MAX_CENTS_PER_BTC).contains(&minor_per_btc) {
            return Err(PriceRefused::OutOfBand);
        }
        Ok(Self {
            source,
            fetched_at,
            minor_per_btc,
        })
    }

    /// The source.
    pub fn source(&self) -> &'static Source {
        self.source
    }

    /// When the app got the reply, in Unix seconds.
    pub fn fetched_at(&self) -> u64 {
        self.fetched_at
    }

    /// The price, in minor units per bitcoin.
    pub fn minor_per_btc(&self) -> u64 {
        self.minor_per_btc
    }

    /// RT-30: whether CR-58 lets a rate fetched at this time be used at
    /// `now`.
    fn is_fresh_at(&self, now: u64) -> bool {
        RateSource::new(self.source.id, self.fetched_at).is_ok_and(|s| s.is_fresh_at(now))
    }
}

/// RT-35: the label of a rate made from these sources: `med.` and the
/// letter of each, in the order of the table. For the table of RT-01 it is
/// at most 8 bytes.
pub fn label_for(sources: &[&Source]) -> String {
    let mut order: Vec<&&Source> = sources.iter().collect();
    order.sort_by_key(|s| position(s));
    let mut label = String::from(LABEL_PREFIX);
    label.extend(order.iter().map(|s| s.initial));
    label
}

/// RT-30 to RT-38: the rate from the samples at the time `now`.
///
/// Only samples CR-58 allows are used; of one source the newest is. At
/// least [`MIN_SOURCES`] distinct sources are needed. The rate is the
/// median, the lower of the two middle prices when the count is even, and
/// every price used must be within [`MAX_SPREAD_BPS`] of it, or the call is
/// refused: no outlier is dropped. The source is labelled `med.` and the
/// letters of the sources used, and its time is the oldest of theirs.
pub fn combine(samples: &[Sample], now: u64) -> Result<(Rate, RateSource), RatesRefused> {
    let mut used: Vec<&Sample> = Vec::new();
    for s in samples.iter().filter(|s| s.is_fresh_at(now)) {
        match used.iter_mut().find(|u| u.source.id == s.source.id) {
            Some(u) if u.fetched_at < s.fetched_at => *u = s,
            Some(_) => {}
            None => used.push(s),
        }
    }
    if used.is_empty() && !samples.is_empty() {
        return Err(RatesRefused::TooOld);
    }
    let not_enough = RatesRefused::NotEnoughSources { fresh: used.len() };
    if used.len() < MIN_SOURCES {
        return Err(not_enough);
    }
    if used.iter().any(|u| {
        used.first()
            .is_some_and(|f| f.source.currency != u.source.currency)
    }) {
        return Err(RatesRefused::MixedCurrencies);
    }
    let mut prices: Vec<u64> = used.iter().map(|u| u.minor_per_btc).collect();
    prices.sort_unstable();
    let median = prices
        .get((prices.len().saturating_sub(1)) / 2)
        .copied()
        .ok_or(not_enough)?;
    let lowest = prices.first().copied().ok_or(not_enough)?;
    let highest = prices.last().copied().ok_or(not_enough)?;
    let limit = u128::from(median) * u128::from(MAX_SPREAD_BPS);
    if prices
        .iter()
        .any(|p| u128::from(p.abs_diff(median)) * 10_000 > limit)
    {
        return Err(RatesRefused::PricesDisagree { lowest, highest });
    }
    let sources: Vec<&Source> = used.iter().map(|u| u.source).collect();
    let oldest = used.iter().map(|u| u.fetched_at).min().ok_or(not_enough)?;
    let rate = Rate::new(median).ok_or(not_enough)?;
    let source = RateSource::new(&label_for(&sources), oldest).map_err(|_| not_enough)?;
    Ok((rate, source))
}
