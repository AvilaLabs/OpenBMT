//! LN-18, LN-19.
//!
//! The forms and the amounts are those of the BIP 321 text: an address in
//! the path, `amount` as a decimal of BTC with a period (`0.00001`, `20.3`),
//! `lightning` as a key of the query, and all capitals for a QR code. The
//! long invoice of the BIP's own example was not copied: it could not be
//! checked here, so the tests use invoices this crate's tests sign.

use std::str::FromStr;

use bitcoin::Address;

use super::*;
use crate::bip321::{btc_decimal, PaymentUri};
use crate::bolt11::{check, Checked, Expectation};
use crate::error::UriRefused as R;

// An address of the BIP's example, which the BIP writes in capitals.
const MAIN: &str = "bc1qylh3u67j673h6y6alv70m0pl2yz53tzhvxgg7u";

fn checked(msat: u64, network: &'static str) -> Checked {
    let mut m = Make::good(hash(1));
    m.msat = Some(msat);
    m.network = network;
    m.description = Some("Sale 17".to_string());
    let ask = Ask::sale(COUNTER, 17, 0, msat, 300).unwrap();
    check(
        &m.write(),
        &Expectation {
            network: match network {
                "bc" => Network::Bitcoin,
                _ => Network::Regtest,
            },
            node: node_key(1),
            payment_hash: hash(1),
            ask,
        },
    )
    .expect("an invoice that passes")
}

#[test]
fn ln_18_amounts_are_btc_decimals_with_a_period() {
    assert_eq!(btc_decimal(1), "0.00000001");
    assert_eq!(btc_decimal(1_000), "0.00001");
    assert_eq!(btc_decimal(25_000), "0.00025");
    assert_eq!(btc_decimal(2_030_000_000), "20.3");
    assert_eq!(btc_decimal(100_000_000), "1");
    assert_eq!(btc_decimal(150_000_000), "1.5");
    assert_eq!(btc_decimal(2_100_000_000_000_000), "21000000");
    assert_eq!(btc_decimal(12_345_678), "0.12345678");
    for sats in [1u64, 999, 1_000, 25_000, 99_999_999, 100_000_001] {
        let text = btc_decimal(sats);
        assert!(!text.contains(','));
        // It reads back to the same number of satoshis.
        let (whole, frac) = text.split_once('.').unwrap_or((&text, ""));
        let back = whole.parse::<u64>().unwrap() * 100_000_000
            + format!("{frac:0<8}").parse::<u64>().unwrap();
        assert_eq!(back, sats);
    }
}

#[test]
fn ln_18_the_code_is_the_address_the_amount_and_the_invoice() {
    let addr = Address::from_str(MAIN)
        .unwrap()
        .assume_checked()
        .to_string();
    assert_eq!(addr, MAIN);
    // 1,000 satoshis on the main chain, as in the example of the BIP.
    let mut main = checked(1_000_000, "bc");
    main.amount_msat = 1_000_000;
    let uri = PaymentUri::new(MAIN, Network::Bitcoin, 1_000, &main).unwrap();
    assert_eq!(
        uri.text(),
        format!("bitcoin:{MAIN}?amount=0.00001&lightning={}", main.bolt11)
    );
    // The same address in capitals, as the BIP writes it, reads the same.
    let upper = MAIN.to_ascii_uppercase();
    let again = PaymentUri::new(&upper, Network::Bitcoin, 1_000, &main).unwrap();
    assert_eq!(again, uri);
    // The query keys in the BIP's order and spelling.
    let text = uri.text();
    assert!(text.starts_with("bitcoin:bc1"));
    let (_, query) = text.split_once('?').unwrap();
    let keys: Vec<&str> = query
        .split('&')
        .map(|p| p.split_once('=').unwrap().0)
        .collect();
    assert_eq!(keys, ["amount", "lightning"]);
    assert!(!text.contains(' ') && !text.contains(','));
}

#[test]
fn ln_18_the_amount_of_the_code_is_the_amount_of_the_invoice() {
    let invoice = checked(25_000_000, "bc");
    // The invoice names 25,000 satoshis.
    assert!(PaymentUri::new(MAIN, Network::Bitcoin, 25_000, &invoice).is_ok());
    for sats in [0u64, 24_999, 25_001, 1_000_000] {
        assert_eq!(
            PaymentUri::new(MAIN, Network::Bitcoin, sats, &invoice).unwrap_err(),
            R::AmountMismatch,
            "{sats}"
        );
    }
    // An amount that overflows is no match.
    assert_eq!(
        PaymentUri::new(MAIN, Network::Bitcoin, u64::MAX, &invoice).unwrap_err(),
        R::AmountMismatch
    );
}

#[test]
fn ln_18_the_address_is_of_the_shops_network() {
    let invoice = checked(25_000_000, "bc");
    for bad in [
        "",
        "not an address",
        // A regtest address on the main chain, and the reverse.
        "bcrt1qw508d6qejxtdg4y5r3zarvary0c5xw7kygt080",
        // A bad checksum.
        "bc1qylh3u67j673h6y6alv70m0pl2yz53tzhvxgg7v",
    ] {
        assert_eq!(
            PaymentUri::new(bad, Network::Bitcoin, 25_000, &invoice).unwrap_err(),
            R::BadAddress,
            "{bad}"
        );
    }
    assert_eq!(
        PaymentUri::new(MAIN, Network::Regtest, 25_000, &invoice).unwrap_err(),
        R::BadAddress
    );
}

#[test]
fn ln_19_capitals_only_for_a_bech32_address() {
    let invoice = checked(25_000_000, "bc");
    let uri = PaymentUri::new(MAIN, Network::Bitcoin, 25_000, &invoice).unwrap();
    let qr = uri.qr_text().unwrap();
    assert_eq!(qr, uri.text().to_ascii_uppercase());
    assert!(qr.starts_with("BITCOIN:BC1"));
    assert!(qr.contains("?AMOUNT=0.00025&LIGHTNING=LNBC"));
    // Every character is in the QR alphanumeric alphabet except what the BIP
    // keeps: the colon, the question mark, the equals sign and the ampersand
    // are in it too.
    assert!(qr
        .chars()
        .all(|c| c.is_ascii_digit() || c.is_ascii_uppercase() || ":?=&.".contains(c)));
    // Both are read back the same by the readers of the two formats.
    assert_eq!(qr.to_ascii_lowercase(), uri.text());
    // Taproot is bech32m and is in capitals too.
    let taproot = "bc1p0xlxvlhemja6c4dqv22uapctqupfhlxm9h8z3k2e72q4k9hcz7vqzk5jj0";
    let t = PaymentUri::new(taproot, Network::Bitcoin, 25_000, &invoice).unwrap();
    assert!(t.qr_text().is_ok());
}

#[test]
fn ln_19_a_base58_address_is_never_put_in_capitals() {
    let invoice = checked(25_000_000, "bc");
    for legacy in [
        "1A1zP1eP5QGefi2DMPTfTL5SLmv7DivfNa",
        "3J98t1WpEZ73CNmQviecrnyiWrnqRhWNLy",
    ] {
        let uri = PaymentUri::new(legacy, Network::Bitcoin, 25_000, &invoice).unwrap();
        assert!(uri.text().starts_with(&format!("bitcoin:{legacy}?amount=")));
        assert_eq!(uri.qr_text().unwrap_err(), R::NotBech32);
    }
}
