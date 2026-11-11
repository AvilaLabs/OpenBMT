//! The owner phone's price from the sources (OA-101).

use std::path::PathBuf;

use openbmt_rates::{RatesRefused, FETCH_INTERVAL_SECS};

use crate::{agreed_price, PriceReply};

const NOW: u64 = 1_700_000_000;

fn fixture(name: &str) -> Vec<u8> {
    let path = PathBuf::from(
        std::env::var_os("CARGO_MANIFEST_DIR")
            .expect("cargo sets CARGO_MANIFEST_DIR when it runs tests"),
    )
    .join("../openbmt-rates/tests/fixtures")
    .join(format!("{name}.json"));
    std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn reply(source: &str, at: u64) -> PriceReply {
    PriceReply {
        source: source.to_string(),
        body: fixture(source),
        fetched_at: at,
    }
}

fn price_reply(source: &str, price: &str, at: u64) -> PriceReply {
    let body = match source {
        "coinbase" => format!(r#"{{"data":{{"amount":"{price}","currency":"USD"}}}}"#),
        _ => format!(r#"{{"last":"{price}"}}"#),
    };
    PriceReply {
        source: source.to_string(),
        body: body.into_bytes(),
        fetched_at: at,
    }
}

#[test]
fn oa_101_the_replies_of_the_sources_give_the_agreed_price_in_whole_dollars() {
    let replies = [reply("kraken", NOW), reply("coinbase", NOW)];
    let got = agreed_price(&replies, NOW).expect("the two real replies agree");
    assert_eq!(got.source_label, "med.kc");
    assert_eq!(got.usd_per_btc, got.minor_per_btc.saturating_add(50) / 100);
    assert!(got.minor_per_btc > 0);
}

#[test]
fn oa_101_the_dollars_are_the_median_rounded_half_up() {
    let up = [
        price_reply("bitstamp", "98500.50", NOW),
        price_reply("coinbase", "98500.50", NOW),
    ];
    let got = agreed_price(&up, NOW).expect("agree");
    assert_eq!((got.minor_per_btc, got.usd_per_btc), (9_850_050, 98_501));
    let down = [
        price_reply("bitstamp", "98500.49", NOW),
        price_reply("coinbase", "98500.49", NOW),
    ];
    let got = agreed_price(&down, NOW).expect("agree");
    assert_eq!((got.minor_per_btc, got.usd_per_btc), (9_850_049, 98_500));
}

#[test]
fn oa_101_sources_that_disagree_are_the_librarys_refusal() {
    let replies = [
        price_reply("bitstamp", "98000", NOW),
        price_reply("coinbase", "110000", NOW),
    ];
    let e = agreed_price(&replies, NOW).expect_err("a spread of more than 2%");
    assert_eq!(
        e,
        RatesRefused::PricesDisagree {
            lowest: 9_800_000,
            highest: 11_000_000,
        }
    );
    assert_eq!(e.rule(), "RT-33");
    assert!(e.to_string().starts_with("RT-33:"));
}

#[test]
fn oa_101_a_source_that_did_not_answer_or_was_refused_is_a_missing_sample() {
    let mut garbage = reply("kraken", NOW);
    garbage.body = b"not json".to_vec();
    // One good source and one refused reply: one source is not enough.
    let e = agreed_price(&[garbage.clone(), reply("coinbase", NOW)], NOW).expect_err("one");
    assert_eq!(e, RatesRefused::NotEnoughSources { fresh: 1 });
    // Nothing at all.
    let e = agreed_price(&[], NOW).expect_err("none");
    assert_eq!(e, RatesRefused::NotEnoughSources { fresh: 0 });
    // A refused reply does not stop two others from agreeing.
    let replies = [garbage, reply("coinbase", NOW), reply("bitstamp", NOW)];
    assert!(agreed_price(&replies, NOW).is_ok());
}

#[test]
fn oa_101_a_reply_older_than_the_library_allows_is_not_used() {
    let old = NOW - 10 * FETCH_INTERVAL_SECS - 1;
    let e = agreed_price(&[reply("kraken", old), reply("coinbase", old)], NOW).expect_err("old");
    assert_eq!(e, RatesRefused::TooOld);
}

#[test]
fn oa_101_a_reply_of_a_source_not_in_the_table_is_a_missing_sample() {
    let mut stray = reply("kraken", NOW);
    stray.source = "nowhere".to_string();
    let e = agreed_price(&[stray.clone(), reply("coinbase", NOW)], NOW).expect_err("one");
    assert_eq!(e, RatesRefused::NotEnoughSources { fresh: 1 });
    let replies = [stray, reply("coinbase", NOW), reply("bitstamp", NOW)];
    assert!(agreed_price(&replies, NOW).is_ok());
}
