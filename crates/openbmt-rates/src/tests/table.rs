//! RT-01 to RT-03: the table of sources.

use openbmt_counter::money::{RateSource, MAX_SOURCE_LABEL_BYTES};

use super::src;
use crate::{source, Shape, FETCH_INTERVAL_SECS, FETCH_TIMEOUT_SECS, SOURCES};
use openbmt_counter::money::MAX_RATE_AGE_SECS;

#[test]
fn rt_01_the_table_is_the_four_sources() {
    let ids: Vec<&str> = SOURCES.iter().map(|s| s.id).collect();
    assert_eq!(ids, ["kraken", "coinbase", "bitstamp", "gemini"]);
    assert_eq!(
        src("kraken").url,
        "https://api.kraken.com/0/public/Ticker?pair=XBTUSD"
    );
    assert_eq!(
        src("coinbase").url,
        "https://api.coinbase.com/v2/prices/BTC-USD/spot"
    );
    assert_eq!(
        src("bitstamp").url,
        "https://www.bitstamp.net/api/v2/ticker/btcusd/"
    );
    assert_eq!(
        src("gemini").url,
        "https://api.gemini.com/v1/pubticker/btcusd"
    );
    assert_eq!(src("kraken").shape, Shape::Kraken);
    assert_eq!(src("coinbase").shape, Shape::Coinbase);
    assert_eq!(src("bitstamp").shape, Shape::Last);
    assert_eq!(src("gemini").shape, Shape::Last);
    assert!(source("nobody").is_none());
}

#[test]
fn rt_01_every_address_is_https_with_no_user_data() {
    for s in &SOURCES {
        assert!(s.url.starts_with("https://"), "{}", s.id);
        assert!(!s.url.contains('@') && !s.url.contains('#'), "{}", s.id);
    }
}

#[test]
fn rt_02_ids_are_labels_and_ids_and_letters_are_unique() {
    for s in &SOURCES {
        assert!(RateSource::new(s.id, 0).is_ok(), "{}", s.id);
        assert!(s.initial.is_ascii_lowercase());
    }
    for (i, a) in SOURCES.iter().enumerate() {
        for b in SOURCES.iter().skip(i + 1) {
            assert_ne!(a.id, b.id);
            assert_ne!(a.initial, b.initial);
        }
    }
}

#[test]
fn rt_03_every_source_names_a_currency() {
    for s in &SOURCES {
        assert_eq!(s.currency, "USD", "{}", s.id);
        assert!(s.currency.len() == 3 && s.currency.bytes().all(|b| b.is_ascii_uppercase()));
    }
}

#[test]
fn rt_35_the_longest_label_fits_with_room_to_spare() {
    // `med.` and one letter per source.
    assert!(crate::LABEL_PREFIX.len() + SOURCES.len() <= MAX_SOURCE_LABEL_BYTES);
}

#[test]
fn rt_40_a_price_is_never_older_than_a_few_fetches() {
    // A sample is at most an interval and a timeout old when it is the
    // newest the app has; it takes several failed rounds to make it stale.
    const { assert!((FETCH_INTERVAL_SECS + FETCH_TIMEOUT_SECS) * 4 <= MAX_RATE_AGE_SECS) };
    const { assert!(FETCH_TIMEOUT_SECS < FETCH_INTERVAL_SECS) };
}
