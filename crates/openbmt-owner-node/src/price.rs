//! The price of a bitcoin for the owner phone (`docs/OWNER-APP-RULES.md`
//! OA-101).
//!
//! A phone with no settings yet needs today's price in whole dollars to
//! work out the starting limits. The app asks the sources of
//! `docs/RATE-RULES.md` for it, through Tor, and hands the replies here. The
//! reading of a reply, the agreement between the sources and the refusals
//! are the rates library's, the same as the counter uses; this module adds
//! nothing to them but the rounding to whole dollars.

use openbmt_rates::{combine, parse_response, source, RatesRefused, Sample};

/// One reply of one source, as the app got it (RT-45).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PriceReply {
    /// The label of the source the reply came from (RT-01).
    pub source: String,
    /// The reply, as read.
    pub body: Vec<u8>,
    /// When the app had the whole reply, by its own clock.
    pub fetched_at: u64,
}

/// OA-101: the price the sources agree on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgreedPrice {
    /// The price of one bitcoin in whole dollars, rounded half up.
    pub usd_per_btc: u64,
    /// The price in minor units per bitcoin, as the library made it.
    pub minor_per_btc: u64,
    /// The label of the sources used (RT-35).
    pub source_label: String,
}

/// OA-101: the price the replies agree on at `now`.
///
/// A reply the library refuses is a missing sample (RT-44) and is left out,
/// and so is a reply that names a source that is not in the table (the app
/// sends the table's labels, so none does); the rest are combined by RT-30
/// to RT-38, so too few sources, a price too old and prices that disagree
/// are the library's refusals.
pub fn agreed_price(replies: &[PriceReply], now: u64) -> Result<AgreedPrice, RatesRefused> {
    let mut samples = Vec::with_capacity(replies.len());
    for r in replies {
        let Some(src) = source(&r.source) else {
            continue;
        };
        let Ok(cents) = parse_response(src, &r.body) else {
            continue;
        };
        if let Ok(sample) = Sample::new(src, r.fetched_at, cents) {
            samples.push(sample);
        }
    }
    let (rate, src) = combine(&samples, now)?;
    let minor = rate.minor_per_btc();
    Ok(AgreedPrice {
        usd_per_btc: minor.saturating_add(50) / 100,
        minor_per_btc: minor,
        source_label: src.label().to_string(),
    })
}
