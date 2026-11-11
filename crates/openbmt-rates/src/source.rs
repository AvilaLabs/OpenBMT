//! The sources of a price and the reading of their replies (RT-01 to RT-17).

use crate::error::ParseRefused;
use crate::price::parse_price_cents;
use openbmt_json::{JsonError, Limits, Value};

/// Where a source keeps its price in the reply (RT-15).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shape {
    /// `{"error": [], "result": {<pair>: {"c": [<last trade price>, ..]}}}`.
    /// An `error` that is not an empty array is a refusal; `result` holds
    /// exactly one entry.
    Kraken,
    /// `{"data": {"amount": <price>, "currency": <code>}}`; the code must be
    /// the source's currency.
    Coinbase,
    /// `{"last": <price>}`.
    Last,
}

/// One source in the table (RT-01).
#[derive(Debug, PartialEq, Eq)]
pub struct Source {
    /// The label of the source (CR-57 charset), unique in the table.
    pub id: &'static str,
    /// One letter of the label of a combined rate (RT-35), unique in the
    /// table.
    pub initial: char,
    /// The HTTPS address the app asks.
    pub url: &'static str,
    /// The currency of the price, three upper-case letters (RT-03).
    pub currency: &'static str,
    /// Where the price is in the reply.
    pub shape: Shape,
}

/// RT-01: the sources, fixed at build time. The order is the order of the
/// letters in a combined label.
pub static SOURCES: [Source; 4] = [
    Source {
        id: "kraken",
        initial: 'k',
        url: "https://api.kraken.com/0/public/Ticker?pair=XBTUSD",
        currency: "USD",
        shape: Shape::Kraken,
    },
    Source {
        id: "coinbase",
        initial: 'c',
        url: "https://api.coinbase.com/v2/prices/BTC-USD/spot",
        currency: "USD",
        shape: Shape::Coinbase,
    },
    Source {
        id: "bitstamp",
        initial: 'b',
        url: "https://www.bitstamp.net/api/v2/ticker/btcusd/",
        currency: "USD",
        shape: Shape::Last,
    },
    Source {
        id: "gemini",
        initial: 'g',
        url: "https://api.gemini.com/v1/pubticker/btcusd",
        currency: "USD",
        shape: Shape::Last,
    },
];

/// The source with this label.
pub fn source(id: &str) -> Option<&'static Source> {
    SOURCES.iter().find(|s| s.id == id)
}

/// The place of a source in the table.
pub(crate) fn position(source: &Source) -> usize {
    SOURCES
        .iter()
        .position(|s| s.id == source.id)
        .unwrap_or(SOURCES.len())
}

/// RT-10 to RT-14: a whole document, within the bounds of a reply.
pub(crate) fn parse_document(bytes: &[u8]) -> Result<Value, ParseRefused> {
    let limits = Limits {
        max_bytes: crate::MAX_RESPONSE_BYTES,
        max_depth: crate::MAX_DEPTH,
        max_values: Limits::DEFAULT.max_values,
    };
    openbmt_json::parse(bytes, &limits).map_err(|e| match e {
        JsonError::TooLong => ParseRefused::TooLong,
        JsonError::TooDeep => ParseRefused::TooDeep,
        // At most 64 KiB, so at most 32,768 values: not reachable.
        JsonError::TooManyValues => ParseRefused::TooLong,
        JsonError::NotUtf8 => ParseRefused::NotUtf8,
        JsonError::Malformed => ParseRefused::Malformed,
        JsonError::TrailingBytes => ParseRefused::TrailingBytes,
        JsonError::DuplicateKey => ParseRefused::DuplicateKey,
    })
}

fn field<'a>(value: &'a Value, key: &str) -> Result<&'a Value, ParseRefused> {
    match value {
        Value::Obj(_) => value.get(key).ok_or(ParseRefused::MissingField),
        _ => Err(ParseRefused::WrongShape),
    }
}

fn text<'a>(value: &'a Value, key: &str) -> Result<&'a str, ParseRefused> {
    field(value, key)?.as_str().ok_or(ParseRefused::WrongShape)
}

/// RT-10 to RT-17, RT-20 to RT-24: the price in cents per bitcoin in the
/// reply of a source, or the reason it is refused. Nothing is trusted that
/// is not read here.
pub fn parse_response(source: &Source, body: &[u8]) -> Result<u64, ParseRefused> {
    let doc = parse_document(body)?;
    let price = match source.shape {
        Shape::Kraken => {
            match field(&doc, "error")? {
                Value::Arr(errors) if errors.is_empty() => {}
                Value::Arr(_) => return Err(ParseRefused::SourceError),
                _ => return Err(ParseRefused::WrongShape),
            }
            let Value::Obj(pairs) = field(&doc, "result")? else {
                return Err(ParseRefused::WrongShape);
            };
            let [(_, entry)] = pairs.as_slice() else {
                return Err(ParseRefused::WrongShape);
            };
            let Value::Arr(last) = field(entry, "c")? else {
                return Err(ParseRefused::WrongShape);
            };
            last.first()
                .and_then(Value::as_str)
                .ok_or(ParseRefused::WrongShape)?
        }
        Shape::Coinbase => {
            let data = field(&doc, "data")?;
            if text(data, "currency")? != source.currency {
                return Err(ParseRefused::WrongCurrency);
            }
            text(data, "amount")?
        }
        Shape::Last => text(&doc, "last")?,
    };
    Ok(parse_price_cents(price)?)
}
