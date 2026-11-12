//! SW-60 to SW-63: the service's live terms.

use openbmt_json::Value;

use super::fixtures::*;
use crate::pairs::percentage_ppm;
use crate::{Pairs, Quote, Refused, Request, PAIRS_FRESH_SECS};

/// The reply of a live service, kept as it was read (see the comment line
/// of the file), with the comment line taken off.
fn live(file: &str) -> Vec<u8> {
    file.lines()
        .filter(|l| !l.starts_with('#'))
        .collect::<String>()
        .into_bytes()
}

fn reverse() -> Vec<u8> {
    live(include_str!("data/swap_reverse.txt"))
}

fn submarine() -> Vec<u8> {
    live(include_str!("data/swap_submarine.txt"))
}

fn with(percentage: &str, minimal: &str, maximal: &str) -> Vec<u8> {
    format!(
        "{{\"BTC\":{{\"BTC\":{{\"limits\":{{\"maximal\":{maximal},\"minimal\":{minimal}}},\
         \"fees\":{{\"percentage\":{percentage},\"minerFees\":{{\"claim\":234,\
         \"lockup\":324}}}}}}}}}}"
    )
    .into_bytes()
}

fn ppm(text: &str) -> Result<u32, Refused> {
    percentage_ppm(&openbmt_json::parse(text.as_bytes(), &Default::default()).unwrap())
}

#[test]
fn sw_60_the_live_btc_pair_is_read_and_other_pairs_are_ignored() {
    let pairs = Pairs::parse(&reverse(), NOW).unwrap();
    let t = pairs.terms();
    assert_eq!((t.min_sats, t.max_sats, t.fee_ppm), (2142, 5_000_000, 6000));
    assert_eq!((t.claim_fee_sats, t.lockup_fee_sats), (234, 324));
    assert_eq!(pairs.read_at(), NOW);
    // The quote of the live terms leaves what the note of the reading says.
    let q = Quote::from_pairs(&pairs, NOW).unwrap();
    assert_eq!(q.least_onchain(10_000), 9_616);
    assert_eq!(q.least_onchain(5_000_000), 4_969_676);
    // A broken L-BTC pair does not matter: only BTC to BTC is read.
    let mut text = String::from_utf8(reverse()).unwrap();
    text = text.replace("\"minimal\":100}", "\"minimal\":\"x\"}");
    assert_eq!(Pairs::parse(text.as_bytes(), NOW).unwrap(), pairs);
    // The reply of another call is the wrong document: no BTC to BTC pair
    // with these members.
    assert_eq!(
        Pairs::parse(&submarine(), NOW),
        Err(Refused::BadPairs("claim"))
    );
    assert_eq!(
        Pairs::parse(b"{\"L-BTC\":{\"BTC\":{}}}", NOW),
        Err(Refused::BadPairs("BTC/BTC"))
    );
    for body in [
        &with("0.6", "0", "5000000")[..],
        &with("0.6", "5000", "4000")[..],
        &with("0.6", "-1", "4000")[..],
        &with("0.6", "1.5", "4000")[..],
        &with("\"0.6\"", "1000", "4000")[..],
        b"[]",
        b"not json",
        b"{\"BTC\":{\"BTC\":{\"limits\":{}}}}",
    ] {
        let err = Pairs::parse(body, NOW).unwrap_err();
        assert!(matches!(err.rule(), "SW-60" | "SW-61"), "{err}");
        assert!(err.to_string().starts_with("SW-6"), "{err}");
    }
    assert!(Pairs::parse(&with("0.6", "1000", "4000"), NOW).is_ok());
    // Longer than the limit.
    let mut long = reverse();
    long.extend(std::iter::repeat_n(b' ', crate::MAX_HTTP_BYTES));
    assert_eq!(Pairs::parse(&long, NOW), Err(Refused::BadPairs("document")));
}

#[test]
fn sw_61_a_percentage_is_read_exactly_from_its_text() {
    for (text, want) in [
        ("0.6", 6000),
        ("0.25", 2500),
        ("0.0001", 1),
        ("0.0", 0),
        ("0", 0),
        ("1", 10_000),
        ("1.0", 10_000),
        ("0.40", 4000),
        ("2.5", 25_000),
        ("10", 100_000),
        ("10.0", 100_000),
        ("9.9999", 99_999),
    ] {
        assert_eq!(ppm(text), Ok(want), "{text}");
    }
    for text in [
        "0.00001", // more than 4 decimal places
        "0.60000",
        "1e-1", // an exponent
        "1E1",
        "6e-1",
        "-0.1", // negative
        "-1",
        "-0",
        "-0.0",
        "10.0001", // over 10 percent
        "11",
        "100",
        "99999999999",
        "\"0.6\"", // not a number
        "true",
        "null",
    ] {
        assert_eq!(ppm(text), Err(Refused::BadPercentage), "{text}");
    }
    // Nothing goes through a float: a value a float cannot hold exactly.
    assert_eq!(ppm("0.1"), Ok(1000));
    assert_eq!(ppm("0.7"), Ok(7000));
    assert_eq!(ppm("2.3"), Ok(23_000));
    // The refusal names its rule.
    assert_eq!(Refused::BadPercentage.rule(), "SW-61");
    let v = openbmt_json::parse(b"1.5", &Default::default()).unwrap();
    assert_eq!(v, Value::Number("1.5".to_string()));
}

#[test]
fn sw_62_the_band_is_both_and_the_service_limit_is_refused_before_a_request() {
    let s = secrets();
    let pairs = Pairs::parse(&reverse(), NOW).unwrap();
    // The larger minimum and the smaller maximum.
    assert_eq!(pairs.band(NOW).unwrap(), (10_000, 5_000_000));
    let wide = Pairs::parse(&with("0.6", "20000", "40000000"), NOW).unwrap();
    assert_eq!(wide.band(NOW).unwrap(), (20_000, 25_000_000));
    // Inside both: a request.
    assert!(Request::new_live(&s, 10_000, "swap", &pairs, NOW).is_ok());
    assert!(Request::new_live(&s, 5_000_000, "swap", &pairs, NOW).is_ok());
    // Above the service's live maximum, below the shop's cap: the service's
    // limit, with its numbers.
    assert_eq!(
        Request::new_live(&s, 5_000_001, "swap", &pairs, NOW),
        Err(Refused::ServiceLimit {
            min: 2142,
            max: 5_000_000
        })
    );
    let err = Request::new_live(&s, 25_000_000, "swap", &pairs, NOW).unwrap_err();
    assert_eq!(err.rule(), "SW-62");
    assert!(err.to_string().contains("service's own limit"), "{err}");
    // Below the service's minimum: the service's limit.
    let high = Pairs::parse(&with("0.6", "30000", "40000"), NOW).unwrap();
    assert_eq!(
        Request::new_live(&s, 20_000, "swap", &high, NOW),
        Err(Refused::ServiceLimit {
            min: 30_000,
            max: 40_000
        })
    );
    // The shop's own band is the shop's refusal.
    assert_eq!(
        Request::new_live(&s, 9_999, "swap", &pairs, NOW),
        Err(Refused::AmountOutOfBand)
    );
    // The two bands that do not meet leave no amount.
    let low = Pairs::parse(&with("0.6", "100", "5000"), NOW).unwrap();
    assert_eq!(
        low.band(NOW),
        Err(Refused::ServiceLimit {
            min: 100,
            max: 5000
        })
    );
}

#[test]
fn sw_63_terms_are_fresh_for_ten_minutes_and_not_from_the_future() {
    let s = secrets();
    let pairs = Pairs::parse(&reverse(), NOW).unwrap();
    assert_eq!(PAIRS_FRESH_SECS, 600);
    assert!(pairs.fresh(NOW).is_ok());
    assert!(pairs.fresh(NOW + 600).is_ok());
    for later in [NOW + 601, NOW + 100_000] {
        assert_eq!(pairs.fresh(later), Err(Refused::PairsStale));
        assert_eq!(Quote::from_pairs(&pairs, later), Err(Refused::PairsStale));
        assert_eq!(pairs.band(later), Err(Refused::PairsStale));
        assert_eq!(
            Request::new_live(&s, 50_000, "swap", &pairs, later),
            Err(Refused::PairsStale)
        );
    }
    // Read at a later time than now: the clock went back.
    assert_eq!(pairs.fresh(NOW - 1), Err(Refused::PairsStale));
    assert_eq!(Refused::PairsStale.rule(), "SW-63");
}
