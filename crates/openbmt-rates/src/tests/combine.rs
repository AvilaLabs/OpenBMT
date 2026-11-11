//! RT-30 to RT-38: several prices into one rate.

use openbmt_counter::money::{MAX_RATE_AGE_SECS, MAX_RATE_AHEAD_SECS, MAX_SOURCE_LABEL_BYTES};

use super::src;
use crate::{combine, label_for, sources_for, RatesRefused, Sample};
use openbmt_core::currency::ShopCurrency;
use openbmt_counter::money::RateSource;

const NOW: u64 = 1_800_000_000;

fn s(id: &str, age: u64, price: u64) -> Sample {
    Sample::new(src(id), NOW - age, price).expect("in the band")
}

fn rate_of(
    r: Result<(openbmt_counter::money::Rate, RateSource), RatesRefused>,
) -> (u64, String, u64) {
    let (rate, source) = r.expect("a rate");
    (
        rate.minor_per_btc(),
        source.label().to_string(),
        source.fetched_at(),
    )
}

#[test]
fn rt_32_two_agreeing_sources_give_the_lower_price() {
    let got = combine(
        &[s("kraken", 30, 7_355_450), s("coinbase", 10, 7_355_218)],
        NOW,
    );
    // Even count: the lower middle (RT-32). Oldest time (RT-36).
    assert_eq!(rate_of(got), (7_355_218, "med.kc".to_string(), NOW - 30));
}

#[test]
fn rt_32_three_sources_give_the_middle_price() {
    let got = combine(
        &[
            s("gemini", 5, 7_356_540),
            s("kraken", 40, 7_355_450),
            s("bitstamp", 20, 7_355_001),
        ],
        NOW,
    );
    // Label letters follow the table, not the order given.
    assert_eq!(rate_of(got), (7_355_450, "med.kbg".to_string(), NOW - 40));
}

#[test]
fn rt_32_four_sources_give_the_lower_of_the_two_middle_prices() {
    let got = combine(
        &[
            s("kraken", 1, 7_000_300),
            s("coinbase", 1, 7_000_100),
            s("bitstamp", 1, 7_000_400),
            s("gemini", 1, 7_000_200),
        ],
        NOW,
    );
    assert_eq!(rate_of(got), (7_000_200, "med.kcbg".to_string(), NOW - 1));
}

#[test]
fn rt_30_a_stale_sample_is_not_used_and_one_left_is_not_enough() {
    let got = combine(
        &[
            s("kraken", MAX_RATE_AGE_SECS + 1, 7_355_450),
            s("coinbase", 10, 7_355_218),
        ],
        NOW,
    );
    assert_eq!(
        got.unwrap_err(),
        RatesRefused::NotEnoughSources { fresh: 1 }
    );
}

#[test]
fn rt_30_the_age_limits_are_those_of_cr_58() {
    let at_limit = combine(
        &[
            s("kraken", MAX_RATE_AGE_SECS, 7_355_450),
            s("coinbase", 0, 7_355_218),
        ],
        NOW,
    );
    assert!(at_limit.is_ok());
    // A little in the future is allowed, too far is not.
    let ahead = |secs: u64| Sample::new(src("kraken"), NOW + secs, 7_355_450).expect("band");
    let ok = combine(
        &[ahead(MAX_RATE_AHEAD_SECS), s("coinbase", 0, 7_355_218)],
        NOW,
    );
    assert!(ok.is_ok());
    let no = combine(
        &[ahead(MAX_RATE_AHEAD_SECS + 1), s("coinbase", 0, 7_355_218)],
        NOW,
    );
    assert_eq!(no.unwrap_err(), RatesRefused::NotEnoughSources { fresh: 1 });
}

#[test]
fn rt_30_a_stale_sample_does_not_count_toward_the_spread() {
    // The stale price is far off, and is dropped for age, not refused.
    let got = combine(
        &[
            s("gemini", MAX_RATE_AGE_SECS + 1, 1_000_000),
            s("kraken", 1, 7_355_450),
            s("coinbase", 1, 7_355_218),
        ],
        NOW,
    );
    assert_eq!(rate_of(got).1, "med.kc");
}

#[test]
fn rt_31_one_source_is_not_enough_and_nothing_is_not_enough() {
    assert_eq!(
        combine(&[s("kraken", 1, 7_355_450)], NOW).unwrap_err(),
        RatesRefused::NotEnoughSources { fresh: 1 }
    );
    assert_eq!(
        combine(&[], NOW).unwrap_err(),
        RatesRefused::NotEnoughSources { fresh: 0 }
    );
}

#[test]
fn rt_31_two_samples_of_one_source_are_one_source() {
    let got = combine(&[s("kraken", 1, 7_355_450), s("kraken", 2, 7_355_450)], NOW);
    assert_eq!(
        got.unwrap_err(),
        RatesRefused::NotEnoughSources { fresh: 1 }
    );
    // The newest sample of a source is the one used.
    let got = combine(
        &[
            s("kraken", 50, 7_000_000),
            s("kraken", 5, 7_355_450),
            s("coinbase", 10, 7_355_218),
        ],
        NOW,
    );
    assert_eq!(rate_of(got), (7_355_218, "med.kc".to_string(), NOW - 10));
}

#[test]
fn rt_33_the_spread_limit_is_two_percent_of_the_median() {
    // Median (lower of two) 10,000,000: the other may be up to 10,200,000.
    let at = combine(
        &[s("kraken", 1, 10_000_000), s("coinbase", 1, 10_200_000)],
        NOW,
    );
    assert!(at.is_ok());
    let over = combine(
        &[s("kraken", 1, 10_000_000), s("coinbase", 1, 10_200_001)],
        NOW,
    );
    assert_eq!(
        over.unwrap_err(),
        RatesRefused::PricesDisagree {
            lowest: 10_000_000,
            highest: 10_200_001
        }
    );
    // Below the median counts as well.
    let under = combine(
        &[
            s("kraken", 1, 9_799_999),
            s("coinbase", 1, 10_000_000),
            s("gemini", 1, 10_000_000),
        ],
        NOW,
    );
    assert!(matches!(under, Err(RatesRefused::PricesDisagree { .. })));
    let under_ok = combine(
        &[
            s("kraken", 1, 9_800_000),
            s("coinbase", 1, 10_000_000),
            s("gemini", 1, 10_000_000),
        ],
        NOW,
    );
    assert!(under_ok.is_ok());
}

#[test]
fn rt_34_one_outlier_among_three_is_refused_not_dropped() {
    let got = combine(
        &[
            s("kraken", 1, 7_355_450),
            s("coinbase", 1, 7_355_218),
            s("bitstamp", 1, 9_000_000),
        ],
        NOW,
    );
    assert_eq!(
        got.unwrap_err(),
        RatesRefused::PricesDisagree {
            lowest: 7_355_218,
            highest: 9_000_000
        }
    );
}

#[test]
fn rt_37_samples_that_are_all_stale_are_too_old() {
    let got = combine(
        &[
            s("kraken", MAX_RATE_AGE_SECS + 1, 7_355_450),
            s("coinbase", MAX_RATE_AGE_SECS + 60, 7_355_218),
        ],
        NOW,
    );
    assert_eq!(got.unwrap_err(), RatesRefused::TooOld);
}

#[test]
fn rt_35_the_label_fits_for_every_subset_of_each_currencys_table() {
    for currency in ShopCurrency::ALL {
        let table: Vec<_> = sources_for(currency).collect();
        for mask in 1u32..(1 << table.len()) {
            let picked: Vec<_> = table
                .iter()
                .enumerate()
                .filter(|(i, _)| mask & (1 << i) != 0)
                .map(|(_, s)| *s)
                .collect();
            let label = label_for(&picked);
            assert!(label.len() <= MAX_SOURCE_LABEL_BYTES, "{label}");
            assert!(label.starts_with("med."));
            assert!(RateSource::new(&label, 0).is_ok(), "{label}");
            // And the label tells which sources were used.
            let letters = label.strip_prefix("med.").expect("prefix");
            for (i, s) in table.iter().enumerate() {
                assert_eq!(letters.contains(s.initial), mask & (1 << i) != 0, "{label}");
            }
        }
    }
}

#[test]
fn rt_35_the_label_is_the_same_whatever_the_order_given() {
    let a = label_for(&[src("gemini"), src("kraken")]);
    let b = label_for(&[src("kraken"), src("gemini")]);
    assert_eq!(a, b);
    assert_eq!(a, "med.kg");
}

#[test]
fn rt_36_the_time_is_the_oldest_used() {
    let got = combine(
        &[
            s("kraken", 100, 7_355_450),
            s("coinbase", 500, 7_355_218),
            s("gemini", 3, 7_356_540),
        ],
        NOW,
    );
    assert_eq!(rate_of(got).2, NOW - 500);
}

#[test]
fn rt_36_the_rate_made_is_fresh_under_cr_58() {
    let (_, source) = combine(
        &[
            s("kraken", MAX_RATE_AGE_SECS, 7_355_450),
            s("coinbase", 0, 7_355_218),
        ],
        NOW,
    )
    .expect("a rate");
    assert!(source.is_fresh_at(NOW));
}

#[test]
fn rt_24_a_sample_outside_the_band_cannot_be_made() {
    assert!(Sample::new(src("kraken"), NOW, 0).is_err());
    assert!(Sample::new(src("kraken"), NOW, 9_999).is_err());
    assert!(Sample::new(src("kraken"), NOW, 10_000_000_001).is_err());
    assert!(Sample::new(src("kraken"), NOW, 10_000).is_ok());
}

#[test]
fn rt_38_prices_in_two_currencies_are_refused() {
    let a = Sample::new(src("coinbase"), NOW - 1, 7_355_218).expect("band");
    let b = Sample::new(src("kraken-eur"), NOW - 1, 7_355_450).expect("band");
    assert_eq!(
        combine(&[a, b], NOW).unwrap_err(),
        RatesRefused::MixedCurrencies
    );
}

#[test]
fn rt_30_each_currency_combines_its_own_sources() {
    // Two yen prices, in whole yen, in the yen band.
    let got = combine(
        &[
            s("kraken-jpy", 5, 11_868_000),
            s("bitflyer-jpy", 9, 11_870_000),
        ],
        NOW,
    );
    assert_eq!(rate_of(got), (11_868_000, "med.kf".to_string(), NOW - 9));
    // A price that fits the yen band is not a dollar price.
    assert!(Sample::new(src("kraken"), NOW, 11_868_000).is_ok());
    assert!(Sample::new(src("kraken-jpy"), NOW, 7_355_450).is_ok());
    assert!(Sample::new(src("kraken-jpy"), NOW, 999_999).is_err());
}

#[test]
fn rt_31_every_refusal_names_its_rule() {
    for r in crate::say::ALL_RATES {
        assert!(r.to_string().starts_with(r.rule()));
    }
}
