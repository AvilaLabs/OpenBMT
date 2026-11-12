//! RT-01 to RT-03: the table of sources.

use openbmt_counter::money::{RateSource, MAX_SOURCE_LABEL_BYTES};

use super::src;
use crate::{source, sources_for, Shape, FETCH_INTERVAL_SECS, FETCH_TIMEOUT_SECS, SOURCES};
use openbmt_core::currency::ShopCurrency;
use openbmt_counter::money::MAX_RATE_AGE_SECS;

#[test]
fn rt_01_the_dollar_table_is_the_four_sources() {
    let ids: Vec<&str> = sources_for(ShopCurrency::Usd).map(|s| s.id).collect();
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
fn rt_02_ids_are_labels_ids_are_unique_and_letters_are_unique_per_currency() {
    for s in &SOURCES {
        assert!(RateSource::new(s.id, 0).is_ok(), "{}", s.id);
        assert!(s.initial.is_ascii_lowercase());
    }
    for (i, a) in SOURCES.iter().enumerate() {
        for b in SOURCES.iter().skip(i + 1) {
            assert_ne!(a.id, b.id);
            if a.currency == b.currency {
                assert_ne!(a.initial, b.initial, "{} and {}", a.id, b.id);
            }
        }
    }
}

#[test]
fn rt_03_every_source_names_a_currency_its_address_names_too() {
    for s in &SOURCES {
        assert!(!s.currency.is_sats(), "{}", s.id);
        let code = s.currency.code();
        assert!(code.len() == 3 && code.bytes().all(|b| b.is_ascii_uppercase()));
        // The address is the one statement of the currency where the reply
        // has none (RT-17), so it must say it. NDAX's documented ticker
        // (`GetLevel1`) takes the market as an instrument number, 1 for
        // BTCCAD in its `GetInstruments`, and its reply carries that number
        // only; the label of the source and the band say the currency.
        assert!(
            s.id == "ndax-cad"
                || s.url
                    .to_ascii_lowercase()
                    .contains(&code.to_ascii_lowercase()),
            "{}",
            s.id
        );
        // A source of a currency other than the dollar says so in its label.
        if s.currency != ShopCurrency::Usd {
            assert!(
                s.id.ends_with(&format!("-{}", code.to_ascii_lowercase())),
                "{}",
                s.id
            );
        }
    }
}

#[test]
fn rt_03_every_currency_but_bitcoin_standard_has_at_least_two_sources() {
    for c in ShopCurrency::ALL {
        let n = sources_for(c).count();
        if c.is_sats() {
            assert_eq!(n, 0);
        } else {
            assert!(n >= crate::MIN_SOURCES, "{}", c.code());
        }
    }
    // The currencies of the table, and how many sources each has.
    let counts: Vec<(&str, usize)> = ShopCurrency::ALL
        .iter()
        .map(|c| (c.code(), sources_for(*c).count()))
        .collect();
    assert_eq!(
        counts,
        [
            ("USD", 4),
            ("EUR", 4),
            ("GBP", 4),
            ("JPY", 2),
            ("AUD", 3),
            ("INR", 2),
            ("KRW", 2),
            ("IDR", 2),
            ("SGD", 2),
            ("CAD", 2),
            ("MXN", 2),
            ("BRL", 3),
            ("TRY", 3),
            ("THB", 2),
            ("ZAR", 2),
            ("SAT", 0)
        ]
    );
}

#[test]
fn rt_01_the_added_currencies_have_their_sources_from_different_companies() {
    let ids = |c| -> Vec<&str> { sources_for(c).map(|s| s.id).collect() };
    assert_eq!(ids(ShopCurrency::Cad), ["kraken-cad", "ndax-cad"]);
    assert_eq!(ids(ShopCurrency::Mxn), ["bitso-mxn", "binance-mxn"]);
    assert_eq!(
        ids(ShopCurrency::Brl),
        ["foxbit-brl", "okx-brl", "binance-brl"]
    );
    assert_eq!(
        ids(ShopCurrency::Try),
        ["btcturk-try", "okx-try", "binance-try"]
    );
    assert_eq!(ids(ShopCurrency::Thb), ["bitkub-thb", "binance-thb"]);
    assert_eq!(ids(ShopCurrency::Zar), ["luno-zar", "valr-zar"]);
    // A company has one source per currency: the host of every source of a
    // currency differs.
    for c in ShopCurrency::ALL {
        let hosts: Vec<&str> = sources_for(c)
            .map(|s| {
                s.url
                    .trim_start_matches("https://")
                    .split('/')
                    .next()
                    .unwrap()
            })
            .collect();
        for (i, a) in hosts.iter().enumerate() {
            assert!(!hosts[i + 1..].contains(a), "{} {a}", c.code());
        }
    }
}

#[test]
fn rt_03_coinbase_spot_is_used_only_for_the_currencies_with_a_coinbase_market() {
    for s in SOURCES.iter().filter(|s| s.shape == Shape::Coinbase) {
        assert!(
            matches!(
                s.currency,
                ShopCurrency::Usd | ShopCurrency::Eur | ShopCurrency::Gbp | ShopCurrency::Inr
            ),
            "{}",
            s.id
        );
    }
}

#[test]
fn rt_35_the_longest_label_fits_with_room_to_spare() {
    // `med.` and one letter per source of a currency.
    for c in ShopCurrency::ALL {
        assert!(crate::LABEL_PREFIX.len() + sources_for(c).count() <= MAX_SOURCE_LABEL_BYTES);
    }
}

#[test]
fn rt_40_a_price_is_never_older_than_a_few_fetches() {
    // A sample is at most an interval and a timeout old when it is the
    // newest the app has; it takes several failed rounds to make it stale.
    const { assert!((FETCH_INTERVAL_SECS + FETCH_TIMEOUT_SECS) * 4 <= MAX_RATE_AGE_SECS) };
    const { assert!(FETCH_TIMEOUT_SECS < FETCH_INTERVAL_SECS) };
}
