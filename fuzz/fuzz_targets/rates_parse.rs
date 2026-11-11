#![no_main]

use libfuzzer_sys::fuzz_target;
use openbmt_rates::{
    combine, parse_price_cents, parse_response, ParseRefused, Sample, MAX_CENTS_PER_BTC,
    MAX_RESPONSE_BYTES, MIN_CENTS_PER_BTC, SOURCES,
};

fuzz_target!(|data: &[u8]| {
    let Some((&selector, body)) = data.split_first() else {
        return;
    };
    let source = &SOURCES[usize::from(selector) % SOURCES.len()];
    let result = parse_response(source, body);
    // The same bytes always give the same answer.
    assert_eq!(result, parse_response(source, body));
    match result {
        Ok(cents) => {
            assert!(body.len() <= MAX_RESPONSE_BYTES);
            assert!(std::str::from_utf8(body).is_ok());
            assert!((MIN_CENTS_PER_BTC..=MAX_CENTS_PER_BTC).contains(&cents));
            // A price that is read can be made a sample, and a rate is
            // made of two of them at the same price.
            let at = 1_800_000_000;
            let s = Sample::new(source, at, cents).expect("a price that was read is in the band");
            let other = &SOURCES[(usize::from(selector) + 1) % SOURCES.len()];
            let t = Sample::new(other, at, cents).expect("in the band");
            let (rate, label) = combine(&[s, t], at).expect("two equal prices combine");
            assert_eq!(rate.minor_per_btc(), cents);
            assert_eq!(label.fetched_at(), at);
        }
        Err(ParseRefused::TooLong) => assert!(body.len() > MAX_RESPONSE_BYTES),
        Err(ParseRefused::NotUtf8) => assert!(std::str::from_utf8(body).is_err()),
        Err(_) => {}
    }
    // The text of a price on its own: a price that is read, written back as
    // dollars and cents, is read again to the same cents.
    if let Ok(text) = std::str::from_utf8(body) {
        if let Ok(cents) = parse_price_cents(text) {
            let back = format!("{}.{:02}", cents / 100, cents % 100);
            assert_eq!(parse_price_cents(&back), Ok(cents));
        }
    }
});
