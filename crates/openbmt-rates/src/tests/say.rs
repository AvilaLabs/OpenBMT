//! SC-01 to SC-12 for the words of this crate.

use openbmt_core::say::{check_distinct, Tone, Who};

use crate::say::{all_words, screens_markdown, ALL_PARSE, ALL_PRICE, ALL_RATES};
use crate::{ParseRefused, PriceRefused, RatesRefused};

#[test]
fn sc_12_every_word_passes_the_check_and_none_repeats() {
    check_distinct(&all_words()).expect("the words pass");
}

fn price_name(p: &PriceRefused) -> &'static str {
    match p {
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

fn parse_name(p: &ParseRefused) -> &'static str {
    match p {
        ParseRefused::TooLong => "TooLong",
        ParseRefused::TooDeep => "TooDeep",
        ParseRefused::NotUtf8 => "NotUtf8",
        ParseRefused::Malformed => "Malformed",
        ParseRefused::TrailingBytes => "TrailingBytes",
        ParseRefused::DuplicateKey => "DuplicateKey",
        ParseRefused::MissingField => "MissingField",
        ParseRefused::WrongShape => "WrongShape",
        ParseRefused::WrongCurrency => "WrongCurrency",
        ParseRefused::SourceError => "SourceError",
        // Its words are those of the price refusal inside it.
        ParseRefused::Price(_) => "Price",
    }
}

fn rates_name(r: &RatesRefused) -> &'static str {
    match r {
        RatesRefused::TooOld => "TooOld",
        RatesRefused::NotEnoughSources { .. } => "NotEnoughSources",
        RatesRefused::PricesDisagree { .. } => "PricesDisagree",
        RatesRefused::MixedCurrencies => "MixedCurrencies",
    }
}

#[test]
fn sc_12_a_variant_missing_from_the_list_fails() {
    let all = all_words();
    let has = |name: String| all.iter().any(|(n, _)| *n == name);
    for p in ALL_PRICE {
        assert!(has(format!("PriceRefused::{}", price_name(&p))));
    }
    for p in ALL_PARSE {
        assert!(has(format!("ParseRefused::{}", parse_name(&p))));
        assert_ne!(parse_name(&p), "Price");
    }
    for r in ALL_RATES {
        assert!(has(format!("RatesRefused::{}", rates_name(&r))));
    }
    assert_eq!(
        all.len(),
        ALL_PRICE.len() + ALL_PARSE.len() + ALL_RATES.len()
    );
    // The words of a price refusal inside a reply refusal are the same.
    for p in ALL_PRICE {
        assert_eq!(ParseRefused::Price(p).words(), p.words());
    }
}

#[test]
fn sc_05_the_cashier_is_told_to_take_another_kind_of_payment() {
    for r in ALL_RATES {
        let w = r.words();
        assert_eq!(w.who, Who::Cashier);
        assert_eq!(w.tone, Tone::Stop);
        assert!(w.what.starts_with("The price is not available right now"));
        assert!(w.next.starts_with("Take another kind of payment."));
        assert_eq!(w.code, Some(r.rule()));
    }
}

#[test]
fn sc_05_a_source_note_stops_nothing_and_asks_nobody() {
    for (name, w) in all_words() {
        if name.starts_with("RatesRefused") {
            continue;
        }
        assert_eq!(w.who, Who::Nobody, "{name}");
        assert_eq!(w.tone, Tone::Info, "{name}");
        assert!(w.next.starts_with("Nothing to do."), "{name}");
        assert!(w.detail.is_some(), "{name}");
    }
}

#[test]
fn sc_03_the_numbers_of_the_prices_stay_out_of_the_sentences() {
    let w = RatesRefused::PricesDisagree {
        lowest: 7_355_218,
        highest: 9_000_000,
    }
    .words();
    assert!(!w.what.contains("7355218") && !w.what.contains("9000000"));
    assert!(!w.next.contains("7355218"));
    assert!(w.detail.is_some_and(|d| d.contains("7355218")));
}

#[test]
fn sc_12_the_list_is_on_disk() {
    let path = &std::path::PathBuf::from(
        std::env::var_os("CARGO_MANIFEST_DIR")
            .expect("cargo sets CARGO_MANIFEST_DIR when it runs tests"),
    )
    .join("../../docs/screens/rates.md");
    let want = screens_markdown();
    if std::env::var_os("OPENBMT_WRITE_SCREENS").is_some() {
        if let Some(dir) = std::path::Path::new(path).parent() {
            std::fs::create_dir_all(dir).expect("make docs/screens");
        }
        std::fs::write(path, &want).expect("write docs/screens/rates.md");
    }
    let have = std::fs::read_to_string(path).unwrap_or_default();
    assert!(
        have == want,
        "docs/screens/rates.md is not what screens_markdown makes: run this test with \
         OPENBMT_WRITE_SCREENS=1 to write it again"
    );
}
