//! The owner phone's own ask of the price sources (`docs/OWNER-APP-RULES.md`
//! OA-101): the table of sources for the app to fetch, and the one call that
//! takes what the sources answered and gives the price they agree on, or the
//! rates library's refusal with its words. The rules and the numbers are in
//! `openbmt-rates` (`docs/RATE-RULES.md`) and `openbmt-owner-node`; this
//! file converts types. It has no state, no network and no clock: the app
//! fetches, through Tor, and hands the bytes over. Neither op takes a node.

use openbmt_owner_node::{agreed_price, PriceReply};
use openbmt_rates::SOURCES;

use crate::codec::{wire_record, wire_record_get};
use crate::error::{from_words, AppError, Kind};

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

/// RT-01: the table of sources.
pub(crate) fn sources() -> Vec<PriceSourceView> {
    SOURCES
        .iter()
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
