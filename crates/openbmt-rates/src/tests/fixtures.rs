//! RT-01, RT-15: the real replies of the four dollar sources, fetched with
//! curl on 2026-11-11 (UTC) from the public endpoints, with no key and no
//! cookies. The expected cents are written by hand from the files. The
//! fixtures of the other currencies are made from the shapes the exchanges
//! document, not fetched; each is marked below.

use super::src;
use crate::{parse_response, ParseRefused};

const KRAKEN: &[u8] = include_bytes!("../../tests/fixtures/kraken.json");
const COINBASE: &[u8] = include_bytes!("../../tests/fixtures/coinbase.json");
const BITSTAMP: &[u8] = include_bytes!("../../tests/fixtures/bitstamp.json");
const GEMINI: &[u8] = include_bytes!("../../tests/fixtures/gemini.json");

#[test]
fn rt_15_kraken_fixture_is_the_last_trade_price() {
    // "c":["73554.50000", ..]
    assert_eq!(parse_response(src("kraken"), KRAKEN), Ok(7_355_450));
}

#[test]
fn rt_15_coinbase_fixture_rounds_half_up() {
    // "amount":"73552.175" is 7355217.5 cents, which rounds up (RT-22).
    assert_eq!(parse_response(src("coinbase"), COINBASE), Ok(7_355_218));
}

#[test]
fn rt_15_bitstamp_fixture_is_last() {
    // "last":"73550.01"
    assert_eq!(parse_response(src("bitstamp"), BITSTAMP), Ok(7_355_001));
}

#[test]
fn rt_15_gemini_fixture_is_last() {
    // "last":"73565.40000"; the volume object also has a "USD" key.
    assert_eq!(parse_response(src("gemini"), GEMINI), Ok(7_356_540));
}

#[test]
fn rt_15_a_reply_is_read_only_with_its_own_source_shape() {
    // Bitstamp's reply has no "data" and no "error": it is not a Coinbase or
    // a Kraken reply.
    assert_eq!(
        parse_response(src("coinbase"), BITSTAMP),
        Err(ParseRefused::MissingField)
    );
    assert_eq!(
        parse_response(src("kraken"), BITSTAMP),
        Err(ParseRefused::MissingField)
    );
    assert_eq!(
        parse_response(src("bitstamp"), COINBASE),
        Err(ParseRefused::MissingField)
    );
}

#[test]
fn rt_15_the_fixtures_are_small_enough_and_valid() {
    for f in [KRAKEN, COINBASE, BITSTAMP, GEMINI] {
        assert!(f.len() < 1024);
        assert!(std::str::from_utf8(f).is_ok());
    }
}

// Made from the documentation of each exchange (not fetched): the fields
// are the documented ones, the numbers are plausible for 2026-11-11.
const KRAKEN_JPY: &[u8] = include_bytes!("../../tests/fixtures/kraken-jpy.json");
const COINBASE_INR: &[u8] = include_bytes!("../../tests/fixtures/coinbase-inr.json");
const GEMINI_SGD: &[u8] = include_bytes!("../../tests/fixtures/gemini-sgd.json");
const BITFLYER_JPY: &[u8] = include_bytes!("../../tests/fixtures/bitflyer-jpy.json");
const BTCMARKETS_AUD: &[u8] = include_bytes!("../../tests/fixtures/btcmarkets-aud.json");
const LUNO_AUD: &[u8] = include_bytes!("../../tests/fixtures/luno-aud.json");
const LUNO_IDR: &[u8] = include_bytes!("../../tests/fixtures/luno-idr.json");
const WAZIRX_INR: &[u8] = include_bytes!("../../tests/fixtures/wazirx-inr.json");
const UPBIT_KRW: &[u8] = include_bytes!("../../tests/fixtures/upbit-krw.json");
const BITHUMB_KRW: &[u8] = include_bytes!("../../tests/fixtures/bithumb-krw.json");
const INDODAX_IDR: &[u8] = include_bytes!("../../tests/fixtures/indodax-idr.json");
const INDEPENDENTRESERVE_SGD: &[u8] =
    include_bytes!("../../tests/fixtures/independentreserve-sgd.json");

#[test]
fn rt_15_the_documented_fixtures_of_the_other_currencies_are_read_to_the_minor_unit() {
    let cases: [(&str, &[u8], u64); 12] = [
        // "c":["11868000", ..]: whole yen.
        ("kraken-jpy", KRAKEN_JPY, 11_868_000),
        // "amount":"7517549.0" rupees: paise.
        ("coinbase-inr", COINBASE_INR, 751_754_900),
        // "last":"97555.00".
        ("gemini-sgd", GEMINI_SGD, 9_755_500),
        // "ltp":11868632.0, a number: read from its own digits.
        ("bitflyer-jpy", BITFLYER_JPY, 11_868_632),
        // "lastPrice":"108539.12".
        ("btcmarkets-aud", BTCMARKETS_AUD, 10_853_912),
        // "last_trade":"108485.00".
        ("luno-aud", LUNO_AUD, 10_848_500),
        // "last_trade":"1234567890.00" rupiah: hundredths.
        ("luno-idr", LUNO_IDR, 123_456_789_000),
        // "lastPrice":"7442024.0" rupees: paise.
        ("wazirx-inr", WAZIRX_INR, 744_202_400),
        // [{"trade_price":100674000.0, ..}]: a number in an array.
        ("upbit-krw", UPBIT_KRW, 100_674_000),
        // "data":{"closing_price":"100601000"}, status "0000".
        ("bithumb-krw", BITHUMB_KRW, 100_601_000),
        // "ticker":{"last":"1234500000"} rupiah: hundredths.
        ("indodax-idr", INDODAX_IDR, 123_450_000_000),
        // "LastPrice":97550.75, a number.
        ("independentreserve-sgd", INDEPENDENTRESERVE_SGD, 9_755_075),
    ];
    for (id, body, want) in cases {
        assert_eq!(parse_response(src(id), body), Ok(want), "{id}");
    }
}

#[test]
fn rt_17_a_reply_for_another_market_is_refused() {
    // Luno's reply names its pair; the address of the dollar-sized IDR source
    // is not the one of the AUD source.
    assert_eq!(
        parse_response(src("luno-aud"), LUNO_IDR),
        Err(ParseRefused::WrongCurrency)
    );
    assert_eq!(
        parse_response(src("luno-idr"), LUNO_AUD),
        Err(ParseRefused::WrongCurrency)
    );
    let other = [
        ("bitflyer-jpy", BITFLYER_JPY, "BTC_JPY", "BTC_USD"),
        ("btcmarkets-aud", BTCMARKETS_AUD, "BTC-AUD", "BTC-NZD"),
        ("wazirx-inr", WAZIRX_INR, "btcinr", "btcusdt"),
        ("upbit-krw", UPBIT_KRW, "KRW-BTC", "USDT-BTC"),
        (
            "independentreserve-sgd",
            INDEPENDENTRESERVE_SGD,
            "Sgd",
            "Usd",
        ),
    ];
    for (id, body, was, now) in other {
        let text = std::str::from_utf8(body).expect("utf-8");
        let changed = text.replacen(&format!("\"{was}\""), &format!("\"{now}\""), 1);
        assert_ne!(changed, text, "{id}");
        assert_eq!(
            parse_response(src(id), changed.as_bytes()),
            Err(ParseRefused::WrongCurrency),
            "{id}"
        );
    }
    // A Coinbase reply for another currency.
    assert_eq!(
        parse_response(src("coinbase-eur"), COINBASE_INR),
        Err(ParseRefused::WrongCurrency)
    );
}

#[test]
fn rt_16_bithumb_that_does_not_say_ok_is_a_source_error() {
    let text = std::str::from_utf8(BITHUMB_KRW).expect("utf-8");
    let bad = text.replacen("\"0000\"", "\"5600\"", 1);
    assert_eq!(
        parse_response(src("bithumb-krw"), bad.as_bytes()),
        Err(ParseRefused::SourceError)
    );
}

#[test]
fn rt_15_a_missing_field_or_extra_nesting_is_refused_in_every_new_shape() {
    for (id, body, field) in [
        ("bitflyer-jpy", BITFLYER_JPY, "\"ltp\""),
        ("btcmarkets-aud", BTCMARKETS_AUD, "\"lastPrice\""),
        ("luno-aud", LUNO_AUD, "\"last_trade\""),
        ("wazirx-inr", WAZIRX_INR, "\"lastPrice\""),
        ("upbit-krw", UPBIT_KRW, "\"trade_price\""),
        ("bithumb-krw", BITHUMB_KRW, "\"closing_price\""),
        ("indodax-idr", INDODAX_IDR, "\"last\""),
        (
            "independentreserve-sgd",
            INDEPENDENTRESERVE_SGD,
            "\"LastPrice\"",
        ),
    ] {
        let text = std::str::from_utf8(body).expect("utf-8");
        // The key is renamed: the price is missing.
        let missing = text.replacen(field, "\"other\"", 1);
        assert_eq!(
            parse_response(src(id), missing.as_bytes()),
            Err(ParseRefused::MissingField),
            "{id}"
        );
        // The key is spelled with an escape: it is the same key to a JSON
        // reader and is never taken from another place.
        let escaped = text.replacen(field, &field.replacen(&field[1..2], "\\u00", 1), 1);
        assert!(parse_response(src(id), escaped.as_bytes()).is_err(), "{id}");
    }
    // The price one level deeper than the shape says.
    let nested = br#"{"product_code":"BTC_JPY","x":{"ltp":11868632.0}}"#;
    assert_eq!(
        parse_response(src("bitflyer-jpy"), nested),
        Err(ParseRefused::MissingField)
    );
    let nested = br#"{"ticker":{"x":{"last":"1234500000"}}}"#;
    assert_eq!(
        parse_response(src("indodax-idr"), nested),
        Err(ParseRefused::MissingField)
    );
    // An array where the shape says an object, and an empty one.
    assert_eq!(
        parse_response(src("upbit-krw"), br#"{"trade_price":100674000.0}"#),
        Err(ParseRefused::WrongShape)
    );
    assert_eq!(
        parse_response(src("upbit-krw"), b"[]"),
        Err(ParseRefused::MissingField)
    );
}

#[test]
fn rt_15_a_number_is_read_from_its_own_digits_at_its_own_place() {
    let jpy = src("bitflyer-jpy");
    // A key of the same name inside another value, and inside a string, is
    // not the price.
    let body = br#"{"a":{"ltp":1.0,"b":[{"ltp":2}]},"n":"\"ltp\": 3","product_code":"BTC_JPY","ltp":11868632.0}"#;
    assert_eq!(parse_response(jpy, body), Ok(11_868_632));
    // An integer, and a fraction that rounds half up to a whole yen.
    let body = br#"{"product_code":"BTC_JPY","ltp":11868633}"#;
    assert_eq!(parse_response(jpy, body), Ok(11_868_633));
    let body = br#"{"product_code":"BTC_JPY","ltp":11868632.5}"#;
    assert_eq!(parse_response(jpy, body), Ok(11_868_633));
}

#[test]
fn rt_15_a_number_shape_refuses_a_string_and_a_text_shape_refuses_a_number() {
    assert_eq!(
        parse_response(
            src("bitflyer-jpy"),
            br#"{"product_code":"BTC_JPY","ltp":"11868632.0"}"#
        ),
        Err(ParseRefused::WrongShape)
    );
    assert_eq!(
        parse_response(
            src("luno-aud"),
            br#"{"pair":"XBTAUD","last_trade":108485.0}"#
        ),
        Err(ParseRefused::WrongShape)
    );
}

#[test]
fn rt_20_a_number_with_an_exponent_or_a_sign_is_refused() {
    let jpy = src("bitflyer-jpy");
    let exponent = br#"{"product_code":"BTC_JPY","ltp":1.1868632e7}"#;
    assert_eq!(
        parse_response(jpy, exponent),
        Err(ParseRefused::Price(crate::PriceRefused::Exponent))
    );
    // A negative price is not read at all.
    let negative = br#"{"product_code":"BTC_JPY","ltp":-11868632.0}"#;
    assert_eq!(
        parse_response(jpy, negative),
        Err(ParseRefused::MissingField)
    );
}

#[test]
fn rt_24_a_price_in_the_wrong_unit_is_out_of_the_currency_band() {
    // A dollar-sized price in the yen source is far below the yen band.
    assert_eq!(
        parse_response(
            src("bitflyer-jpy"),
            br#"{"product_code":"BTC_JPY","ltp":73550.0}"#
        ),
        Err(ParseRefused::Price(crate::PriceRefused::OutOfBand))
    );
}

#[test]
fn rt_15_the_documented_fixtures_are_small_enough_and_valid() {
    for f in [
        KRAKEN_JPY,
        COINBASE_INR,
        GEMINI_SGD,
        BITFLYER_JPY,
        BTCMARKETS_AUD,
        LUNO_AUD,
        LUNO_IDR,
        WAZIRX_INR,
        UPBIT_KRW,
        BITHUMB_KRW,
        INDODAX_IDR,
        INDEPENDENTRESERVE_SGD,
    ] {
        assert!(f.len() < 1024);
        assert!(std::str::from_utf8(f).is_ok());
    }
}
