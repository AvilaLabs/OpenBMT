//! The owner phone's own ask of the price sources (`docs/OWNER-APP-RULES.md`
//! OA-101, OA-102): the table of sources for the app to fetch, and the one
//! call that takes what the sources answered and gives the price they agree
//! on, or the rates library's refusal with its words. The rules and the numbers are in
//! `openbmt-rates` (`docs/RATE-RULES.md`) and `openbmt-owner-node`; this
//! file converts types. It has no state, no network and no clock: the app
//! fetches, through Tor, and hands the bytes over. Neither op takes a node.

use openbmt_core::currency::ShopCurrency;
use openbmt_owner_node::{agreed_price, PriceReply};
use openbmt_rates::{source, sources_for};

use crate::codec::{wire_record, wire_record_get};
use crate::error::{from_words, refused_by, AppError, Kind};

/// One source of the table (RT-01), for the app to fetch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PriceSourceView {
    /// The label of the source.
    pub id: String,
    /// The HTTPS address to ask.
    pub url: String,
}

wire_record!(PriceSourceView { id, url });

/// One reply of one source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PriceReplyView {
    /// The label of the source the reply came from.
    pub source: String,
    /// The reply, as read: at most the limit of RT-10.
    pub body: Vec<u8>,
    /// When the app had the whole reply, by its own clock.
    pub fetched_at: u64,
}

wire_record!(PriceReplyView {
    source,
    body,
    fetched_at
});
wire_record_get!(PriceReplyView {
    source,
    body,
    fetched_at
});

/// The arguments of `price_agree`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PriceArgs {
    /// What the sources answered. A source that did not answer has no
    /// reply here.
    pub replies: Vec<PriceReplyView>,
    /// The time now, by the phone's clock.
    pub now: u64,
}

wire_record!(PriceArgs { replies, now });
wire_record_get!(PriceArgs { replies, now });

/// The price the sources agree on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgreedPriceView {
    /// The price of one bitcoin in whole dollars, rounded half up.
    pub usd_per_btc: u64,
    /// The price in minor units per bitcoin.
    pub minor_per_btc: u64,
    /// The label of the sources used.
    pub source_label: String,
}

wire_record!(AgreedPriceView {
    usd_per_btc,
    minor_per_btc,
    source_label
});

/// RT-01, RT-40: the table of sources, the dollar ones: a phone with no
/// settings prices in dollars, and the sources of another currency are not
/// asked.
pub(crate) fn sources() -> Vec<PriceSourceView> {
    sources_for(ShopCurrency::Usd)
        .map(|s| PriceSourceView {
            id: s.id.to_string(),
            url: s.url.to_string(),
        })
        .collect()
}

/// OA-101: the price the replies agree on at `now`. A refusal of the rates
/// library carries its own words.
pub(crate) fn agree(args: PriceArgs) -> Result<AgreedPriceView, AppError> {
    let replies: Vec<PriceReply> = args
        .replies
        .into_iter()
        .map(|r| PriceReply {
            source: r.source,
            body: r.body,
            fetched_at: r.fetched_at,
        })
        .collect();
    agreed_price(&replies, args.now)
        .map(|p| AgreedPriceView {
            usd_per_btc: p.usd_per_btc,
            minor_per_btc: p.minor_per_btc,
            source_label: p.source_label,
        })
        .map_err(|e| from_words(Kind::Refused, e.rule(), e, e.words()))
}

/// OA-102: the arguments of `price_agree_for`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PriceForArgs {
    /// The shop's currency: three capital letters, not `SAT`.
    pub currency: String,
    /// What the sources answered.
    pub replies: Vec<PriceReplyView>,
    /// The time now, by the phone's clock.
    pub now: u64,
}

wire_record!(PriceForArgs {
    currency,
    replies,
    now
});
wire_record_get!(PriceForArgs {
    currency,
    replies,
    now
});

/// OA-102: the price the sources agree on, in the shop's currency.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgreedPriceInView {
    /// The price of one bitcoin in whole units of the currency, rounded
    /// half up.
    pub whole_per_btc: u64,
    /// The price in minor units per bitcoin.
    pub minor_per_btc: u64,
    /// The label of the sources used.
    pub source_label: String,
}

wire_record!(AgreedPriceInView {
    whole_per_btc,
    minor_per_btc,
    source_label
});

/// The currency a call names, one that has price sources.
fn priced(code: &str) -> Result<ShopCurrency, AppError> {
    ShopCurrency::from_code(code)
        .filter(|c| !c.is_sats())
        .ok_or_else(|| {
            refused_by(
                "request-malformed",
                format!("{code:?} is not a currency that has price sources"),
            )
        })
}

/// OA-102, RT-01, RT-40: the table of sources of the shop's currency. Bitcoin
/// Standard has none and is refused: it asks for no price.
pub(crate) fn sources_for_currency(code: String) -> Result<Vec<PriceSourceView>, AppError> {
    let currency = priced(&code)?;
    Ok(sources_for(currency)
        .map(|s| PriceSourceView {
            id: s.id.to_string(),
            url: s.url.to_string(),
        })
        .collect())
}

/// OA-102: the price the replies agree on at `now`, in whole units of the
/// shop's currency as well as in minor units. A refusal of the rates library
/// carries its own words.
pub(crate) fn agree_for(args: PriceForArgs) -> Result<AgreedPriceInView, AppError> {
    let currency = priced(&args.currency)?;
    let replies: Vec<PriceReply> = args
        .replies
        .into_iter()
        .filter(|r| source(&r.source).is_some_and(|s| s.currency == currency))
        .map(|r| PriceReply {
            source: r.source,
            body: r.body,
            fetched_at: r.fetched_at,
        })
        .collect();
    let agreed = agreed_price(&replies, args.now)
        .map_err(|e| from_words(Kind::Refused, e.rule(), e, e.words()))?;
    let scale = 10u64.pow(currency.minor_digits());
    Ok(AgreedPriceInView {
        whole_per_btc: agreed
            .minor_per_btc
            .saturating_add(scale / 2)
            .checked_div(scale)
            .unwrap_or(0),
        minor_per_btc: agreed.minor_per_btc,
        source_label: agreed.source_label,
    })
}
