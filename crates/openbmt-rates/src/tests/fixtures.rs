//! RT-01, RT-15: the real replies of the four sources, fetched with curl on
//! 2026-11-11 (UTC) from the public endpoints, with no key and no cookies.
//! The expected cents are written by hand from the files.

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
