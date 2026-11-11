#![no_main]

use libfuzzer_sys::fuzz_target;
use openbmt_core::currency::ShopCurrency;
use openbmt_rates::{
    band, combine, parse_price_minor, parse_response, sources_for, ParseRefused, Sample,
    MAX_RESPONSE_BYTES, SOURCES,
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
            let (low, high) = band(source.currency).expect("a source has a band");
            assert!((low..=high).contains(&cents));
            // A price that is read can be made a sample, and a rate is
            // made of two of them at the same price.
            let at = 1_800_000_000;
            let s = Sample::new(source, at, cents).expect("a price that was read is in the band");
            let other = sources_for(source.currency)
                .find(|o| o.id != source.id)
                .expect("a currency has two sources");
            let t = Sample::new(other, at, cents).expect("in the band");
            let (rate, label) = combine(&[s, t], at).expect("two equal prices combine");
            assert_eq!(rate.minor_per_btc(), cents);
            assert_eq!(label.fetched_at(), at);
        }
        Err(ParseRefused::TooLong) => assert!(body.len() > MAX_RESPONSE_BYTES),
        Err(ParseRefused::NotUtf8) => assert!(std::str::from_utf8(body).is_err()),
        Err(_) => {}
    }
    // The text of a price on its own, in each currency: a price that is
    // read, written back as whole units and minor units, is read again to
    // the same amount.
    if let Ok(text) = std::str::from_utf8(body) {
        for currency in ShopCurrency::ALL {
            if let Ok(minor) = parse_price_minor(text, currency) {
                let unit = 10u64.pow(currency.minor_digits());
                let back = if unit == 1 {
                    format!("{minor}")
                } else {
                    format!("{}.{:02}", minor / unit, minor % unit)
                };
                assert_eq!(parse_price_minor(&back, currency), Ok(minor));
            }
        }
    }
});
