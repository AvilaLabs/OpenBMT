//! WO-15, WO-50: the vectors the web order plugin shares with these rules.
//! The plugin's quote is checked against the answers of this library; the
//! inputs are those of the tests of RT-20 to RT-38 and of CR-08. With
//! `OPENBMT_BLESS_WEB_ORDER=1` the test writes the file again.

use std::path::PathBuf;

use openbmt_core::currency::ShopCurrency;
use openbmt_counter::money::{sats_for, Amount, Rate};

use crate::{combine, parse_price_minor, source, Sample, SOURCES};
use crate::{PriceRefused, RatesRefused};

const NOW: u64 = 1_800_000_000;

fn path() -> PathBuf {
    PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").expect("cargo sets it when it runs tests"))
        .join("../../integrations/woocommerce/openbmt-pay/tests_standalone/vectors/rates.json")
}

fn q(text: &str) -> String {
    format!("\"{text}\"")
}

fn price_name(e: PriceRefused) -> &'static str {
    match e {
        PriceRefused::Empty => "Empty",
        PriceRefused::Sign => "Sign",
        PriceRefused::Exponent => "Exponent",
        PriceRefused::BadCharacter => "BadCharacter",
        PriceRefused::TwoDots => "TwoDots",
        PriceRefused::BadShape => "BadShape",
        PriceRefused::LeadingZero => "LeadingZero",
        PriceRefused::TooManyDecimals => "TooManyDecimals",
        PriceRefused::Zero => "Zero",
        PriceRefused::OutOfBand => "OutOfBand",
    }
}

fn table() -> String {
    let rows: Vec<String> = SOURCES
        .iter()
        .map(|s| {
            format!(
                "  {{\"id\":{},\"initial\":{},\"currency\":{},\"url\":{}}}",
                q(s.id),
                q(&s.initial.to_string()),
                q(s.currency.code()),
                q(s.url)
            )
        })
        .collect();
    rows.join(",\n")
}

fn prices() -> String {
    let long = "9".repeat(200);
    let long_fraction = format!("{long}.5");
    let texts: Vec<(&str, String)> = [
        ("USD", "73550.01"),
        ("USD", "73550"),
        ("USD", "73550.1"),
        ("USD", "73550.10000000"),
        ("USD", "100"),
        ("USD", "100000000"),
        ("USD", ""),
        ("USD", "+73550.01"),
        ("USD", "-73550.01"),
        ("USD", "7.355e4"),
        ("USD", "7.355E4"),
        ("USD", "1e5"),
        ("USD", "73550.01.1"),
        ("USD", ".."),
        ("USD", "."),
        ("USD", ".5"),
        ("USD", "73550."),
        ("USD", "073550.01"),
        ("USD", "00"),
        ("USD", "73,550.01"),
        ("USD", "73550.01 "),
        ("USD", " 73550.01"),
        ("USD", "$73550"),
        ("USD", "0x10"),
        ("USD", "NaN"),
        ("USD", "inf"),
        ("USD", "7355\u{0660}"),
        ("USD", "73\u{ff15}50"),
        ("USD", "73550.123456789"),
        ("USD", "73550.000000000"),
        ("USD", "73550.12345678"),
        ("USD", "100.004"),
        ("USD", "100.005"),
        ("USD", "100.0049999"),
        ("USD", "100.00499999"),
        ("USD", "100.00500000"),
        ("USD", "73552.175"),
        ("USD", "73552.174"),
        ("USD", "73552.9951"),
        ("USD", "0"),
        ("USD", "0.0"),
        ("USD", "0.004"),
        ("USD", "0.00499999"),
        ("USD", "99.99"),
        ("USD", "99.994"),
        ("USD", "99.995"),
        ("USD", "1"),
        ("USD", "100000000.00"),
        ("USD", "100000000.01"),
        ("USD", "100000001"),
        ("JPY", "11868632.4"),
        ("JPY", "11868632.5"),
        ("KRW", "100674000"),
        ("IDR", "1234567890.125"),
        ("INR", "7442024.5"),
        ("JPY", "100"),
        ("JPY", "1000000"),
        ("JPY", "999999"),
        ("JPY", "1000000000000"),
        ("JPY", "1000000000001"),
        ("IDR", "1000000"),
        ("IDR", "999999.99"),
        ("SAT", "73550"),
    ]
    .iter()
    .map(|(c, t)| (*c, (*t).to_string()))
    .chain([("USD", long), ("USD", long_fraction)])
    .collect();
    let rows: Vec<String> = texts
        .iter()
        .map(|(code, text)| {
            let currency = ShopCurrency::from_code(code).expect("a currency");
            let want = match parse_price_minor(text, currency) {
                Ok(minor) => format!("{{\"minor\":{minor}}}"),
                Err(e) => format!("{{\"refused\":{}}}", q(price_name(e))),
            };
            let escaped: String = text
                .chars()
                .map(|c| match c {
                    ' '..='~' => c.to_string(),
                    _ => format!("\\u{:04x}", u32::from(c)),
                })
                .collect();
            format!(
                "  {{\"currency\":{},\"text\":\"{escaped}\",\"expect\":{want}}}",
                q(code)
            )
        })
        .collect();
    rows.join(",\n")
}

type Case = (&'static str, Vec<(&'static str, u64, u64)>);

/// Each case is the time and the samples as (source, age in seconds, price).
/// An age above `NOW` is a time ahead: it is given as `NOW + ahead`.
fn combine_cases() -> Vec<Case> {
    let a = |id, age, price| (id, NOW - age, price);
    vec![
        (
            "two agree",
            vec![a("kraken", 30, 7_355_450), a("coinbase", 10, 7_355_218)],
        ),
        (
            "three give the middle",
            vec![
                a("gemini", 5, 7_356_540),
                a("kraken", 40, 7_355_450),
                a("bitstamp", 20, 7_355_001),
            ],
        ),
        (
            "four give the lower middle",
            vec![
                a("kraken", 1, 7_000_300),
                a("coinbase", 1, 7_000_100),
                a("bitstamp", 1, 7_000_400),
                a("gemini", 1, 7_000_200),
            ],
        ),
        (
            "stale leaves one",
            vec![a("kraken", 601, 7_355_450), a("coinbase", 10, 7_355_218)],
        ),
        (
            "age limit",
            vec![a("kraken", 600, 7_355_450), a("coinbase", 0, 7_355_218)],
        ),
        (
            "ahead limit",
            vec![("kraken", NOW + 60, 7_355_450), a("coinbase", 0, 7_355_218)],
        ),
        (
            "too far ahead",
            vec![("kraken", NOW + 61, 7_355_450), a("coinbase", 0, 7_355_218)],
        ),
        (
            "stale does not count in the spread",
            vec![
                a("gemini", 601, 1_000_000),
                a("kraken", 1, 7_355_450),
                a("coinbase", 1, 7_355_218),
            ],
        ),
        ("one source", vec![a("kraken", 1, 7_355_450)]),
        ("none", vec![]),
        (
            "one source twice",
            vec![a("kraken", 1, 7_355_450), a("kraken", 2, 7_355_450)],
        ),
        (
            "newest of a source",
            vec![
                a("kraken", 50, 7_000_000),
                a("kraken", 5, 7_355_450),
                a("coinbase", 10, 7_355_218),
            ],
        ),
        (
            "spread at the limit",
            vec![a("kraken", 1, 10_000_000), a("coinbase", 1, 10_200_000)],
        ),
        (
            "spread over the limit",
            vec![a("kraken", 1, 10_000_000), a("coinbase", 1, 10_200_001)],
        ),
        (
            "below the median over the limit",
            vec![
                a("kraken", 1, 9_799_999),
                a("coinbase", 1, 10_000_000),
                a("gemini", 1, 10_000_000),
            ],
        ),
        (
            "below the median at the limit",
            vec![
                a("kraken", 1, 9_800_000),
                a("coinbase", 1, 10_000_000),
                a("gemini", 1, 10_000_000),
            ],
        ),
        (
            "an outlier is not dropped",
            vec![
                a("kraken", 1, 7_355_450),
                a("coinbase", 1, 7_355_218),
                a("bitstamp", 1, 9_000_000),
            ],
        ),
        (
            "all stale",
            vec![a("kraken", 601, 7_355_450), a("coinbase", 660, 7_355_218)],
        ),
        (
            "oldest time",
            vec![
                a("kraken", 100, 7_355_450),
                a("coinbase", 500, 7_355_218),
                a("gemini", 3, 7_356_540),
            ],
        ),
        (
            "mixed currencies",
            vec![a("coinbase", 1, 7_355_218), a("kraken-eur", 1, 7_355_450)],
        ),
        (
            "yen",
            vec![
                a("kraken-jpy", 5, 11_868_000),
                a("bitflyer-jpy", 9, 11_870_000),
            ],
        ),
        (
            "sample under the band",
            vec![a("kraken", 1, 9_999), a("coinbase", 1, 7_355_218)],
        ),
        (
            "sample over the band",
            vec![a("kraken", 1, 10_000_000_001), a("coinbase", 1, 7_355_218)],
        ),
        (
            "sample at the band",
            vec![a("kraken", 1, 10_000), a("coinbase", 1, 10_000)],
        ),
    ]
}

fn combines() -> String {
    let rows: Vec<String> = combine_cases()
        .iter()
        .map(|(name, samples)| {
            let made: Result<Vec<Sample>, PriceRefused> = samples
                .iter()
                .map(|(id, at, price)| Sample::new(source(id).expect("a source"), *at, *price))
                .collect();
            let want = match made {
                Err(e) => format!("{{\"refused\":{}}}", q(price_name(e))),
                Ok(list) => match combine(&list, NOW) {
                    Ok((rate, from)) => format!(
                        "{{\"rate\":{},\"label\":{},\"oldest\":{}}}",
                        rate.minor_per_btc(),
                        q(from.label()),
                        from.fetched_at()
                    ),
                    Err(RatesRefused::TooOld) => "{\"refused\":\"TooOld\"}".to_string(),
                    Err(RatesRefused::NotEnoughSources { fresh }) => {
                        format!("{{\"refused\":\"NotEnoughSources\",\"fresh\":{fresh}}}")
                    }
                    Err(RatesRefused::PricesDisagree { lowest, highest }) => format!(
                        "{{\"refused\":\"PricesDisagree\",\"lowest\":{lowest},\"highest\":{highest}}}"
                    ),
                    Err(RatesRefused::MixedCurrencies) => {
                        "{\"refused\":\"MixedCurrencies\"}".to_string()
                    }
                },
            };
            let list: Vec<String> = samples
                .iter()
                .map(|(id, at, price)| {
                    format!("{{\"id\":{},\"fetched_at\":{at},\"price\":{price}}}", q(id))
                })
                .collect();
            format!(
                "  {{\"name\":{},\"now\":{NOW},\"samples\":[{}],\"expect\":{want}}}",
                q(name),
                list.join(",")
            )
        })
        .collect();
    rows.join(",\n")
}

fn sats() -> String {
    let cases: [(u64, u64); 12] = [
        (2_500, 7_355_218),
        (1, 7_355_218),
        (10_000, 10_000),
        (7_355_218, 7_355_218),
        (7_355_219, 7_355_218),
        (100_000_000, 7_355_218),
        (100, 10_000_000_000),
        (1, 10_000_000_000),
        (123_456_789, 1_234_567),
        (10_000_000_000, 10_000),
        (99_999_999_999, 10_000),
        (1_000_000_000_000_000, 10_000),
    ];
    let rows: Vec<String> = cases
        .iter()
        .map(|(amount, rate)| {
            let want = sats_for(Amount(*amount), Rate::new(*rate).expect("a rate"))
                .map_or("null".to_string(), |s| s.to_string());
            format!("  {{\"amount\":{amount},\"rate\":{rate},\"sats\":{want}}}")
        })
        .collect();
    rows.join(",\n")
}

fn file() -> String {
    format!(
        "{{\n\"version\":1,\n\"table\":[\n{}\n],\n\"prices\":[\n{}\n],\n\"combine\":[\n{}\n],\n\"sats\":[\n{}\n]\n}}\n",
        table(),
        prices(),
        combines(),
        sats()
    )
}

#[test]
fn wo_15_the_committed_vectors_are_the_ones_the_library_makes() {
    let text = file();
    if std::env::var_os("OPENBMT_BLESS_WEB_ORDER").is_some() {
        std::fs::create_dir_all(path().parent().expect("a folder")).expect("a directory");
        std::fs::write(path(), &text).expect("written");
    }
    let committed = std::fs::read_to_string(path()).unwrap_or_default();
    assert!(
        committed == text,
        "the vector file differs from the library: run the tests of this crate with \
         OPENBMT_BLESS_WEB_ORDER=1 to write it again"
    );
}
