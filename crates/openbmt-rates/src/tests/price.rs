//! RT-20 to RT-24: the text of a price.

use crate::price::{MAX_CENTS_PER_BTC, MIN_CENTS_PER_BTC};
use crate::{parse_price_cents, PriceRefused};

#[test]
fn rt_20_plain_prices_are_read_exactly() {
    assert_eq!(parse_price_cents("73550.01"), Ok(7_355_001));
    assert_eq!(parse_price_cents("73550"), Ok(7_355_000));
    assert_eq!(parse_price_cents("73550.1"), Ok(7_355_010));
    assert_eq!(parse_price_cents("73550.10000000"), Ok(7_355_010));
    assert_eq!(parse_price_cents("100"), Ok(MIN_CENTS_PER_BTC));
    assert_eq!(parse_price_cents("100000000"), Ok(MAX_CENTS_PER_BTC));
}

#[test]
fn rt_20_shapes_that_are_not_plain_digits_are_refused() {
    let cases = [
        ("", PriceRefused::Empty),
        ("+73550.01", PriceRefused::Sign),
        ("-73550.01", PriceRefused::Sign),
        ("7.355e4", PriceRefused::Exponent),
        ("7.355E4", PriceRefused::Exponent),
        ("1e5", PriceRefused::Exponent),
        ("73550.01.1", PriceRefused::TwoDots),
        ("73.550.01", PriceRefused::TwoDots),
        ("..", PriceRefused::TwoDots),
        (".", PriceRefused::BadShape),
        (".5", PriceRefused::BadShape),
        ("73550.", PriceRefused::BadShape),
        ("073550.01", PriceRefused::LeadingZero),
        ("00", PriceRefused::LeadingZero),
        ("73,550.01", PriceRefused::BadCharacter),
        ("73550.01 ", PriceRefused::BadCharacter),
        (" 73550.01", PriceRefused::BadCharacter),
        ("$73550", PriceRefused::BadCharacter),
        ("0x10", PriceRefused::BadCharacter),
        ("NaN", PriceRefused::BadCharacter),
        ("inf", PriceRefused::BadCharacter),
        ("7355\u{0660}", PriceRefused::BadCharacter),
        ("73\u{ff15}50", PriceRefused::BadCharacter),
    ];
    for (text, want) in cases {
        assert_eq!(parse_price_cents(text), Err(want), "{text:?}");
    }
}

#[test]
fn rt_21_nine_fractional_digits_are_refused_and_eight_are_not() {
    assert_eq!(
        parse_price_cents("73550.123456789"),
        Err(PriceRefused::TooManyDecimals)
    );
    assert_eq!(
        parse_price_cents("73550.000000000"),
        Err(PriceRefused::TooManyDecimals)
    );
    assert_eq!(parse_price_cents("73550.12345678"), Ok(7_355_012));
}

#[test]
fn rt_22_the_cent_is_rounded_half_up() {
    assert_eq!(parse_price_cents("100.004"), Ok(10_000));
    assert_eq!(parse_price_cents("100.005"), Ok(10_001));
    assert_eq!(parse_price_cents("100.0049999"), Ok(10_000));
    assert_eq!(parse_price_cents("100.00499999"), Ok(10_000));
    assert_eq!(parse_price_cents("100.00500000"), Ok(10_001));
    assert_eq!(parse_price_cents("73552.175"), Ok(7_355_218));
    assert_eq!(parse_price_cents("73552.174"), Ok(7_355_217));
    assert_eq!(parse_price_cents("73552.9951"), Ok(7_355_300));
}

#[test]
fn rt_23_zero_and_what_rounds_to_zero_are_refused() {
    for z in ["0", "0.0", "0.00", "0.000000", "0.004", "0.00499999"] {
        assert_eq!(parse_price_cents(z), Err(PriceRefused::Zero), "{z:?}");
    }
}

#[test]
fn rt_24_a_price_outside_the_band_is_refused() {
    assert_eq!(parse_price_cents("99.99"), Err(PriceRefused::OutOfBand));
    assert_eq!(parse_price_cents("99.994"), Err(PriceRefused::OutOfBand));
    assert_eq!(parse_price_cents("99.995"), Ok(MIN_CENTS_PER_BTC));
    assert_eq!(parse_price_cents("1"), Err(PriceRefused::OutOfBand));
    assert_eq!(parse_price_cents("100000000.00"), Ok(MAX_CENTS_PER_BTC));
    assert_eq!(
        parse_price_cents("100000000.01"),
        Err(PriceRefused::OutOfBand)
    );
    assert_eq!(parse_price_cents("100000001"), Err(PriceRefused::OutOfBand));
    // Far too many digits never overflow.
    let huge = "9".repeat(60_000);
    assert_eq!(parse_price_cents(&huge), Err(PriceRefused::OutOfBand));
    let huge = format!("{huge}.5");
    assert_eq!(parse_price_cents(&huge), Err(PriceRefused::OutOfBand));
}

#[test]
fn rt_20_every_refusal_names_its_rule() {
    for e in crate::say::ALL_PRICE {
        assert!(e.to_string().starts_with(e.rule()));
        assert!(e.rule().starts_with("RT-"));
    }
}
