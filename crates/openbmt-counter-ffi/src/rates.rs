//! The price of bitcoin, for the app that fetches it: the table of sources,
//! the reading of one reply and the making of one rate from samples. The
//! rules and the numbers are in `openbmt-rates` (`docs/RATE-RULES.md`); this
//! file converts types. It has no state, no network and no clock: the app
//! fetches (RT-40 to RT-45) and hands the bytes over.

use openbmt_rates::{combine, parse_response, source, Sample, SOURCES};

use crate::codec::{wire_record, wire_record_get};
use crate::error::{from_rule, refused_by, CounterError};

/// The most samples one `rate_combine` takes: four sources and room for the
/// samples of several turns of the schedule.
pub const MAX_SAMPLES: usize = 64;

/// One source of the table (RT-01), for the app to fetch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RateSourceView {
    /// The label of the source.
    pub id: String,
    /// The HTTPS address to ask.
    pub url: String,
    /// The currency of its price, three capital letters.
    pub currency: String,
    /// The one letter the source adds to a combined label (RT-35).
    pub initial: String,
}

wire_record!(RateSourceView {
    id,
    url,
    currency,
    initial
});

/// The arguments of `rate_parse`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RateParseArgs {
    /// The label of the source the reply came from.
    pub source: String,
    /// The reply, as read: at most the limit of RT-10.
    pub response: Vec<u8>,
    /// When the app had the whole reply, by its own clock (RT-45).
    pub fetched_at: u64,
}

wire_record!(RateParseArgs {
    source,
    response,
    fetched_at
});
wire_record_get!(RateParseArgs {
    source,
    response,
    fetched_at
});

/// One price from one source (RT-45): what `rate_parse` gives and
/// `rate_combine` takes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SampleView {
    /// The label of the source.
    pub source: String,
    /// When the app had the reply.
    pub fetched_at: u64,
    /// The price, in minor units per bitcoin.
    pub minor_per_btc: u64,
}

wire_record!(SampleView {
    source,
    fetched_at,
    minor_per_btc
});
wire_record_get!(SampleView {
    source,
    fetched_at,
    minor_per_btc
});

/// The arguments of `rate_combine`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RateCombineArgs {
    /// The samples the schedule has collected.
    pub samples: Vec<SampleView>,
    /// The time now.
    pub now: u64,
}

wire_record!(RateCombineArgs { samples, now });
wire_record_get!(RateCombineArgs { samples, now });

/// The rate a sale is opened with: the rate, and its source as the counter
/// takes it (CR-57).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RateView {
    /// The price, in minor units per bitcoin.
    pub rate: u64,
    /// The label of the source, `med.` and one letter for each source used.
    pub source_label: String,
    /// The oldest time of the samples used.
    pub source_fetched_at: u64,
}

wire_record!(RateView {
    rate,
    source_label,
    source_fetched_at
});

/// RT-01: the table of sources.
pub(crate) fn rate_sources() -> Vec<RateSourceView> {
    SOURCES
        .iter()
        .map(|s| RateSourceView {
            id: s.id.to_string(),
            url: s.url.to_string(),
            currency: s.currency.code().to_string(),
            initial: s.initial.to_string(),
        })
        .collect()
}

fn unknown_source(id: &str) -> CounterError {
    refused_by(
        "rate-source",
        format!("{id:?} is not the label of a source in the table"),
    )
}

/// RT-10 to RT-24: the price in a reply, as a sample. A refusal carries the
/// words of `openbmt-rates`.
pub(crate) fn rate_parse(args: RateParseArgs) -> Result<SampleView, CounterError> {
    let src = source(&args.source).ok_or_else(|| unknown_source(&args.source))?;
    let cents =
        parse_response(src, &args.response).map_err(|e| from_rule(e.rule(), e, e.words()))?;
    let sample =
        Sample::new(src, args.fetched_at, cents).map_err(|e| from_rule(e.rule(), e, e.words()))?;
    Ok(SampleView {
        source: src.id.to_string(),
        fetched_at: sample.fetched_at(),
        minor_per_btc: sample.minor_per_btc(),
    })
}

/// RT-30 to RT-38: the rate from the samples at `now`.
pub(crate) fn rate_combine(args: RateCombineArgs) -> Result<RateView, CounterError> {
    if args.samples.len() > MAX_SAMPLES {
        return Err(refused_by(
            "rate-samples",
            format!("at most {MAX_SAMPLES} samples are combined at once"),
        ));
    }
    let mut samples = Vec::with_capacity(args.samples.len());
    for s in &args.samples {
        let src = source(&s.source).ok_or_else(|| unknown_source(&s.source))?;
        samples.push(
            Sample::new(src, s.fetched_at, s.minor_per_btc)
                .map_err(|e| from_rule(e.rule(), e, e.words()))?,
        );
    }
    let (rate, src) = combine(&samples, args.now).map_err(|e| from_rule(e.rule(), e, e.words()))?;
    Ok(RateView {
        rate: rate.minor_per_btc(),
        source_label: src.label().to_string(),
        source_fetched_at: src.fetched_at(),
    })
}
