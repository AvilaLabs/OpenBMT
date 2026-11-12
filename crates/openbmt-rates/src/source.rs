//! The sources of a price and the reading of their replies (RT-01 to RT-17).

use crate::error::ParseRefused;
use crate::price::parse_price_minor;
use openbmt_core::currency::ShopCurrency;
use openbmt_json::{JsonError, Limits, Value};

/// One step from the top of a reply to its price (RT-15).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// The value of this key of an object.
    Key(&'static str),
    /// The first item of an array.
    First,
}

/// What a reply that does not match a check is refused as (RT-17).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fault {
    /// The reply names another market than the source's: `WrongCurrency`.
    Market,
    /// The source says it did not answer: `SourceError`.
    Status,
}

/// A field of a reply that must be this exact text before its price is
/// read (RT-17).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Check {
    /// Where the field is.
    pub at: &'static [Step],
    /// What it must say.
    pub equals: &'static str,
    /// What a different text is refused as.
    pub fault: Fault,
}

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
    /// A price that is a string at `price`, after the field of `check`, if
    /// there is one, says what it must.
    Text {
        /// Where the price is.
        price: &'static [Step],
        /// The field that names the market or the status.
        check: Option<Check>,
    },
    /// A price that is a JSON number at `price`, read from the number's own
    /// text and never as a float (RT-15).
    Number {
        /// Where the price is.
        price: &'static [Step],
        /// The field that names the market or the status.
        check: Option<Check>,
    },
}

/// One source in the table (RT-01).
#[derive(Debug, PartialEq, Eq)]
pub struct Source {
    /// The label of the source (CR-57 charset), unique in the table.
    pub id: &'static str,
    /// One letter of the label of a combined rate (RT-35), unique among the
    /// sources of one currency.
    pub initial: char,
    /// The HTTPS address the app asks.
    pub url: &'static str,
    /// The currency of the price (RT-03).
    pub currency: ShopCurrency,
    /// Where the price is in the reply.
    pub shape: Shape,
}

use ShopCurrency::{Aud, Eur, Gbp, Idr, Inr, Jpy, Krw, Sgd, Usd};

const fn src(
    id: &'static str,
    initial: char,
    url: &'static str,
    currency: ShopCurrency,
    shape: Shape,
) -> Source {
    Source {
        id,
        initial,
        url,
        currency,
        shape,
    }
}

const MARKET_ID: Step = Step::Key("marketId");

/// RT-01: the sources, fixed at build time, a table for each currency. The
/// order is the order of the letters in a combined label. The label of a
/// source that is not for dollars ends in its currency.
pub static SOURCES: [Source; 25] = [
    // US dollar.
    src(
        "kraken",
        'k',
        "https://api.kraken.com/0/public/Ticker?pair=XBTUSD",
        Usd,
        Shape::Kraken,
    ),
    src(
        "coinbase",
        'c',
        "https://api.coinbase.com/v2/prices/BTC-USD/spot",
        Usd,
        Shape::Coinbase,
    ),
    src(
        "bitstamp",
        'b',
        "https://www.bitstamp.net/api/v2/ticker/btcusd/",
        Usd,
        Shape::Last,
    ),
    src(
        "gemini",
        'g',
        "https://api.gemini.com/v1/pubticker/btcusd",
        Usd,
        Shape::Last,
    ),
    // Euro.
    src(
        "kraken-eur",
        'k',
        "https://api.kraken.com/0/public/Ticker?pair=XBTEUR",
        Eur,
        Shape::Kraken,
    ),
    src(
        "coinbase-eur",
        'c',
        "https://api.coinbase.com/v2/prices/BTC-EUR/spot",
        Eur,
        Shape::Coinbase,
    ),
    src(
        "bitstamp-eur",
        'b',
        "https://www.bitstamp.net/api/v2/ticker/btceur/",
        Eur,
        Shape::Last,
    ),
    src(
        "gemini-eur",
        'g',
        "https://api.gemini.com/v1/pubticker/btceur",
        Eur,
        Shape::Last,
    ),
    // Pound sterling.
    src(
        "kraken-gbp",
        'k',
        "https://api.kraken.com/0/public/Ticker?pair=XBTGBP",
        Gbp,
        Shape::Kraken,
    ),
    src(
        "coinbase-gbp",
        'c',
        "https://api.coinbase.com/v2/prices/BTC-GBP/spot",
        Gbp,
        Shape::Coinbase,
    ),
    src(
        "bitstamp-gbp",
        'b',
        "https://www.bitstamp.net/api/v2/ticker/btcgbp/",
        Gbp,
        Shape::Last,
    ),
    src(
        "gemini-gbp",
        'g',
        "https://api.gemini.com/v1/pubticker/btcgbp",
        Gbp,
        Shape::Last,
    ),
    // Japanese yen.
    src(
        "kraken-jpy",
        'k',
        "https://api.kraken.com/0/public/Ticker?pair=XBTJPY",
        Jpy,
        Shape::Kraken,
    ),
    src(
        "bitflyer-jpy",
        'f',
        "https://api.bitflyer.com/v1/ticker?product_code=BTC_JPY",
        Jpy,
        Shape::Number {
            price: &[Step::Key("ltp")],
            check: Some(Check {
                at: &[Step::Key("product_code")],
                equals: "BTC_JPY",
                fault: Fault::Market,
            }),
        },
    ),
    // Australian dollar.
    src(
        "kraken-aud",
        'k',
        "https://api.kraken.com/0/public/Ticker?pair=XBTAUD",
        Aud,
        Shape::Kraken,
    ),
    src(
        "luno-aud",
        'l',
        "https://api.luno.com/api/1/ticker?pair=XBTAUD",
        Aud,
        Shape::Text {
            price: &[Step::Key("last_trade")],
            check: Some(Check {
                at: &[Step::Key("pair")],
                equals: "XBTAUD",
                fault: Fault::Market,
            }),
        },
    ),
    src(
        "btcmarkets-aud",
        'm',
        "https://api.btcmarkets.net/v3/markets/BTC-AUD/ticker",
        Aud,
        Shape::Text {
            price: &[Step::Key("lastPrice")],
            check: Some(Check {
                at: &[MARKET_ID],
                equals: "BTC-AUD",
                fault: Fault::Market,
            }),
        },
    ),
    // Indian rupee.
    src(
        "coinbase-inr",
        'c',
        "https://api.coinbase.com/v2/prices/BTC-INR/spot",
        Inr,
        Shape::Coinbase,
    ),
    src(
        "wazirx-inr",
        'w',
        "https://api.wazirx.com/sapi/v1/ticker/24hr?symbol=btcinr",
        Inr,
        Shape::Text {
            price: &[Step::Key("lastPrice")],
            check: Some(Check {
                at: &[Step::Key("symbol")],
                equals: "btcinr",
                fault: Fault::Market,
            }),
        },
    ),
    // South Korean won.
    src(
        "upbit-krw",
        'u',
        "https://api.upbit.com/v1/ticker?markets=KRW-BTC",
        Krw,
        Shape::Number {
            price: &[Step::First, Step::Key("trade_price")],
            check: Some(Check {
                at: &[Step::First, Step::Key("market")],
                equals: "KRW-BTC",
                fault: Fault::Market,
            }),
        },
    ),
    src(
        "bithumb-krw",
        't',
        "https://api.bithumb.com/public/ticker/BTC_KRW",
        Krw,
        Shape::Text {
            price: &[Step::Key("data"), Step::Key("closing_price")],
            check: Some(Check {
                at: &[Step::Key("status")],
                equals: "0000",
                fault: Fault::Status,
            }),
        },
    ),
    // Indonesian rupiah.
    src(
        "luno-idr",
        'l',
        "https://api.luno.com/api/1/ticker?pair=XBTIDR",
        Idr,
        Shape::Text {
            price: &[Step::Key("last_trade")],
            check: Some(Check {
                at: &[Step::Key("pair")],
                equals: "XBTIDR",
                fault: Fault::Market,
            }),
        },
    ),
    src(
        "indodax-idr",
        'd',
        "https://indodax.com/api/ticker/btcidr",
        Idr,
        Shape::Text {
            price: &[Step::Key("ticker"), Step::Key("last")],
            check: None,
        },
    ),
    // Singapore dollar.
    src(
        "gemini-sgd",
        'g',
        "https://api.gemini.com/v1/pubticker/btcsgd",
        Sgd,
        Shape::Last,
    ),
    src(
        "independentreserve-sgd",
        'i',
        "https://api.independentreserve.com/Public/GetMarketSummary?primaryCurrencyCode=xbt&secondaryCurrencyCode=sgd",
        Sgd,
        Shape::Number {
            price: &[Step::Key("LastPrice")],
            check: Some(Check {
                at: &[Step::Key("SecondaryCurrencyCode")],
                equals: "Sgd",
                fault: Fault::Market,
            }),
        },
    ),
];

/// RT-01: the sources of one currency, in the order of the table. Bitcoin
/// Standard has none.
pub fn sources_for(currency: ShopCurrency) -> impl Iterator<Item = &'static Source> {
    SOURCES.iter().filter(move |s| s.currency == currency)
}

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

fn step<'a>(value: &'a Value, step: &Step) -> Result<&'a Value, ParseRefused> {
    match (step, value) {
        (Step::Key(key), Value::Obj(_)) => value.get(key).ok_or(ParseRefused::MissingField),
        (Step::First, Value::Arr(items)) => items.first().ok_or(ParseRefused::MissingField),
        _ => Err(ParseRefused::WrongShape),
    }
}

fn walk<'a>(doc: &'a Value, path: &[Step]) -> Result<&'a Value, ParseRefused> {
    path.iter().try_fold(doc, step)
}

/// RT-17: the field of a check says what it must.
fn checked(doc: &Value, check: &Option<Check>) -> Result<(), ParseRefused> {
    let Some(check) = check else {
        return Ok(());
    };
    let said = walk(doc, check.at)?
        .as_str()
        .ok_or(ParseRefused::WrongShape)?;
    if said == check.equals {
        return Ok(());
    }
    Err(match check.fault {
        Fault::Market => ParseRefused::WrongCurrency,
        Fault::Status => ParseRefused::SourceError,
    })
}

/// RT-15: the text of the number a document holds at `path`. The document
/// has been read whole already (RT-13), so it is well formed and has no key
/// twice; this walks the same bytes to the same place and takes the number's
/// own digits, which the reader of `openbmt-json` does not keep. A key
/// written with an escape is not matched, so such a reply is refused as
/// missing the field and never read at another place.
fn number_text<'a>(body: &'a [u8], path: &[Step]) -> Option<&'a str> {
    let mut at = skip_white(body, 0);
    for s in path {
        at = skip_white(body, at);
        match s {
            Step::First => {
                if *body.get(at)? != b'[' {
                    return None;
                }
                at = skip_white(body, at + 1);
                if *body.get(at)? == b']' {
                    return None;
                }
            }
            Step::Key(want) => {
                if *body.get(at)? != b'{' {
                    return None;
                }
                at += 1;
                loop {
                    at = skip_white(body, at);
                    if *body.get(at)? != b'"' {
                        return None;
                    }
                    let end = skip_string(body, at)?;
                    let name = body.get(at + 1..end - 1)?;
                    at = skip_white(body, end);
                    if *body.get(at)? != b':' {
                        return None;
                    }
                    at = skip_white(body, at + 1);
                    if name == want.as_bytes() {
                        break;
                    }
                    at = skip_value(body, at, 0)?;
                    at = skip_white(body, at);
                    if *body.get(at)? != b',' {
                        return None;
                    }
                    at += 1;
                }
            }
        }
    }
    let end = skip_value(body, at, 0)?;
    let text = std::str::from_utf8(body.get(at..end)?).ok()?;
    text.starts_with(|c: char| c.is_ascii_digit())
        .then_some(text)
}

fn skip_white(body: &[u8], mut at: usize) -> usize {
    while matches!(body.get(at), Some(b' ' | b'\t' | b'\n' | b'\r')) {
        at += 1;
    }
    at
}

/// The position after the string that starts at `at` (a quote).
fn skip_string(body: &[u8], at: usize) -> Option<usize> {
    let mut i = at + 1;
    loop {
        match body.get(i)? {
            b'"' => return Some(i + 1),
            b'\\' => i += 2,
            _ => i += 1,
        }
    }
}

/// The position after the value that starts at `at`.
fn skip_value(body: &[u8], at: usize, depth: usize) -> Option<usize> {
    if depth > crate::MAX_DEPTH {
        return None;
    }
    match body.get(at)? {
        b'"' => skip_string(body, at),
        open @ (b'{' | b'[') => {
            let close = if *open == b'{' { b'}' } else { b']' };
            let mut i = skip_white(body, at + 1);
            if *body.get(i)? == close {
                return Some(i + 1);
            }
            loop {
                if *open == b'{' {
                    i = skip_string(body, skip_white(body, i))?;
                    i = skip_white(body, i);
                    if *body.get(i)? != b':' {
                        return None;
                    }
                    i += 1;
                }
                i = skip_value(body, skip_white(body, i), depth + 1)?;
                i = skip_white(body, i);
                match body.get(i)? {
                    b',' => i += 1,
                    c if *c == close => return Some(i + 1),
                    _ => return None,
                }
            }
        }
        _ => {
            let mut i = at;
            while !matches!(
                body.get(i),
                None | Some(b',' | b']' | b'}' | b' ' | b'\t' | b'\n' | b'\r')
            ) {
                i += 1;
            }
            (i > at).then_some(i)
        }
    }
}

/// RT-10 to RT-17, RT-20 to RT-24: the price in minor units per bitcoin in
/// the reply of a source, or the reason it is refused. Nothing is trusted
/// that is not read here.
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
            if text(data, "currency")? != source.currency.code() {
                return Err(ParseRefused::WrongCurrency);
            }
            text(data, "amount")?
        }
        Shape::Last => text(&doc, "last")?,
        Shape::Text { price, check } => {
            checked(&doc, &check)?;
            walk(&doc, price)?
                .as_str()
                .ok_or(ParseRefused::WrongShape)?
        }
        Shape::Number { price, check } => {
            checked(&doc, &check)?;
            if !matches!(
                walk(&doc, price)?,
                Value::Int(_) | Value::UInt(_) | Value::Number(_)
            ) {
                return Err(ParseRefused::WrongShape);
            }
            number_text(body, price).ok_or(ParseRefused::MissingField)?
        }
    };
    Ok(parse_price_minor(price, source.currency)?)
}
