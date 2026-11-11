//! CR-83: the payment request the customer scans.

use bitcoin::hashes::Hash;
use bitcoin::{Address, Network, PubkeyHash, ScriptBuf};
use openbmt_qr::{encode_text, Ecc, Mode};

use super::kit::p2wsh;
use crate::pay::{btc_amount, payment_for, request_texts};

#[test]
fn cr_76_the_amount_is_bitcoin_with_at_most_eight_decimals() {
    for (sats, text) in [
        (0, "0"),
        (1, "0.00000001"),
        (10, "0.0000001"),
        (5_000, "0.00005"),
        (2_500, "0.000025"),
        (99_999_999, "0.99999999"),
        (100_000_000, "1"),
        (150_000_000, "1.5"),
        (100_000_001, "1.00000001"),
        (2_100_000_000_000_000, "21000000"),
        (u64::MAX, "184467440737.09551615"),
    ] {
        assert_eq!(btc_amount(sats), text, "{sats} sats");
    }
    // No exponent, no sign, no trailing zero, no trailing point.
    for sats in [1u64, 7, 40, 123_456, 10_000_000, 12_345_678_900] {
        let text = btc_amount(sats);
        assert!(
            text.bytes().all(|b| b.is_ascii_digit() || b == b'.'),
            "{text}"
        );
        assert!(!text.ends_with('0') || !text.contains('.'), "{text}");
        assert!(!text.ends_with('.'), "{text}");
        let decimals = text.split_once('.').map_or(0, |(_, d)| d.len());
        assert!(decimals <= 8, "{text}");
    }
}

#[test]
fn cr_76_a_bech32_address_is_upper_case_in_the_code_and_lower_in_the_copy() {
    let (code, copy) = request_texts("bc1qxyz", true, 5_000);
    assert_eq!(code, "BITCOIN:BC1QXYZ?amount=0.00005");
    assert_eq!(copy, "bitcoin:bc1qxyz?amount=0.00005");
    // Nothing needed: no amount at all.
    let (code, copy) = request_texts("bc1qxyz", true, 0);
    assert_eq!(
        (code.as_str(), copy.as_str()),
        ("BITCOIN:BC1QXYZ", "bitcoin:bc1qxyz")
    );
    // A base58 address is case-sensitive: it is never changed, and neither
    // is the scheme.
    let (code, copy) = request_texts("1BoatSLRHtKNngkdXEeobR76b53LETtpyT", false, 1);
    assert_eq!(code, copy);
    assert_eq!(
        code,
        "bitcoin:1BoatSLRHtKNngkdXEeobR76b53LETtpyT?amount=0.00000001"
    );
}

fn bits_of(payment: &crate::Payment) -> Vec<bool> {
    let n = payment.qr_size as usize;
    (0..n * n)
        .map(|i| payment.qr_bits[i / 8] & (0x80 >> (i % 8)) != 0)
        .collect()
}

#[test]
fn cr_76_the_modules_are_the_qr_encoders_for_the_text_at_level_m() {
    let script = p2wsh(1);
    for sats in [0u64, 1, 5_000, 123_456_789] {
        let payment = payment_for(&script, Network::Regtest, sats).expect("a payment");
        let address = Address::from_script(&script, Network::Regtest)
            .expect("an address")
            .to_string();
        assert_eq!(payment.address, address);
        let code = encode_text(&payment.uri, Ecc::M).expect("a code");
        assert_eq!(code.ecc(), Ecc::M);
        assert_eq!(payment.qr_size as usize, code.size());
        let n = code.size();
        let expected: Vec<bool> = (0..n * n).map(|i| code.get(i % n, i / n)).collect();
        assert_eq!(bits_of(&payment), expected, "{sats} sats");
        // The padding of the last byte is 0.
        let used = (n * n) % 8;
        if used != 0 {
            let last = *payment.qr_bits.last().expect("a byte");
            assert_eq!(last & (0xff >> used), 0);
        }
        assert_eq!(payment.qr_bits.len(), (n * n).div_ceil(8));
    }
}

#[test]
fn cr_76_with_no_amount_the_code_is_alphanumeric_and_with_one_it_is_bytes() {
    let script = p2wsh(1);
    let bare = payment_for(&script, Network::Bitcoin, 0).expect("a payment");
    assert!(bare.uri.starts_with("BITCOIN:BC1Q"), "{}", bare.uri);
    assert_eq!(
        encode_text(&bare.uri, Ecc::M).expect("a code").mode(),
        Mode::Alphanumeric
    );
    let asked = payment_for(&script, Network::Bitcoin, 5_000).expect("a payment");
    assert_eq!(
        encode_text(&asked.uri, Ecc::M).expect("a code").mode(),
        Mode::Byte
    );
    // The text for copying is the same request in lower case, and a wallet
    // that reads either gets the same address and amount.
    assert_eq!(asked.copy_uri, asked.uri.to_ascii_lowercase());
    assert!(asked.copy_uri.starts_with("bitcoin:bc1q"));
    assert!(asked.copy_uri.ends_with("?amount=0.00005"));
}

#[test]
fn cr_76_a_base58_address_keeps_its_case() {
    let script = ScriptBuf::new_p2pkh(&PubkeyHash::from_byte_array([7; 20]));
    let payment = payment_for(&script, Network::Bitcoin, 1_000).expect("a payment");
    assert_eq!(payment.uri, payment.copy_uri);
    assert!(payment.uri.starts_with("bitcoin:1"), "{}", payment.uri);
    assert!(payment.address.chars().any(|c| c.is_ascii_lowercase()));
    assert!(payment.uri.contains(&payment.address));
}

#[test]
fn cr_76_a_script_with_no_address_is_refused_with_words() {
    let script = ScriptBuf::new_op_return([1, 2, 3]);
    let e = payment_for(&script, Network::Regtest, 1).expect_err("no address");
    assert_eq!(e.rule(), "pay-address");
    assert!(!e.words().heading.is_empty());
}
