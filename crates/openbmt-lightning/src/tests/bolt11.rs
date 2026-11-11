//! LN-16, LN-17.

use super::*;
use crate::bolt11::{check, decode, network_prefix, Description, Expectation, MAX_BOLT11_CHARS};
use crate::error::InvoiceRefused as R;

fn expect() -> Expectation {
    Expectation {
        network: NETWORK,
        node: node_key(1),
        payment_hash: hash(1),
        ask: ask(),
    }
}

fn good() -> Make {
    Make::good(hash(1))
}

#[test]
fn ln_16_an_invoice_is_read_field_by_field() {
    let d = decode(&good().write()).expect("reads");
    assert_eq!(d.network, "bcrt");
    assert_eq!(d.amount_msat, Some(25_000_000));
    assert_eq!(d.timestamp, 1_700_000_000);
    assert_eq!(d.payment_hash, hash(1));
    assert_eq!(
        d.description,
        Some(Description::Text("Sale 17".to_string()))
    );
    assert_eq!(d.expiry_secs, 300);
    assert_eq!(d.payee, None);
    // The signer is recovered from the signature, not read from the text.
    assert_eq!(d.signer, node_key(1));
}

#[test]
fn ln_16_the_four_networks_are_the_four_prefixes() {
    for (name, network) in [
        ("bc", Network::Bitcoin),
        ("tb", Network::Testnet),
        ("tbs", Network::Signet),
        ("bcrt", Network::Regtest),
    ] {
        let mut m = good();
        m.network = name;
        let d = decode(&m.write()).expect(name);
        assert_eq!(d.network, name);
        assert_eq!(network_prefix(network), Some(name));
    }
    // A prefix that is none of them: the simnet one, and a made-up one.
    for name in ["sb", "xx", "ltc"] {
        let mut m = good();
        m.network = name;
        assert_eq!(decode(&m.write()).unwrap_err(), R::UnknownNetwork, "{name}");
    }
    assert_eq!(decode("lightning1qqqqqq").unwrap_err(), R::NotAnInvoice);
}

#[test]
fn ln_16_the_amount_is_written_as_bolt_11_writes_it() {
    // 1 BTC, 0.001 BTC, 10 microbitcoin, 1 nanobitcoin, 25 msat.
    for msat in [
        100_000_000_000u64,
        100_000_000,
        1_000_000,
        100,
        25,
        2_100_000_000_000_000_000,
    ] {
        let mut m = good();
        m.msat = Some(msat);
        assert_eq!(
            decode(&m.write()).unwrap().amount_msat,
            Some(msat),
            "{msat}"
        );
    }
    // No amount at all.
    let mut m = good();
    m.msat = None;
    assert_eq!(decode(&m.write()).unwrap().amount_msat, None);
    // Hand-written prefixes: a leading zero, a multiplier alone, a picobitcoin
    // that is not a whole number of millisatoshis, a letter that is none, and
    // an amount that does not fit.
    for bad in [
        "0u",
        "m",
        "05m",
        "25x",
        "15p",
        "99999999999999999999",
        "-5u",
    ] {
        let hrp = format!("lnbcrt{bad}");
        let text = bech32_write(&hrp, &[0u8; 111]);
        assert_eq!(decode(&text).unwrap_err(), R::BadAmount, "{bad}");
    }
}

#[test]
fn ln_16_the_signature_gives_the_signer() {
    let mut m = good();
    m.key = secret(2);
    assert_eq!(decode(&m.write()).unwrap().signer, node_key(2));
    // A recovery number that is not 0 to 3.
    let mut m = good();
    m.recid_delta = 4;
    assert_eq!(decode(&m.write()).unwrap_err(), R::BadSignature);
    // A signature that gives no key.
    let hrp = "lnbcrt1";
    let mut data = vec![0u8; 7];
    data.extend(field(1, &groups_of(&hash(1))));
    data.extend(vec![0u8; 104]);
    assert_eq!(
        decode(&bech32_write(hrp, &data)).unwrap_err(),
        R::BadSignature
    );
}

#[test]
fn ln_16_a_text_that_is_not_a_good_bech32_invoice_is_refused() {
    let text = good().write();
    // Upper case throughout is fine; the invoice then reads the same.
    assert_eq!(
        decode(&text.to_ascii_uppercase()).unwrap(),
        decode(&text).unwrap()
    );
    // Both cases is not.
    let mut mixed = text.clone();
    mixed.replace_range(..1, "L");
    assert_eq!(decode(&mixed).unwrap_err(), R::MixedCase);
    // A letter changed breaks the checksum.
    let mut broken = text.clone().into_bytes();
    let at = broken.len() - 10;
    broken[at] = if broken[at] == b'q' { b'p' } else { b'q' };
    assert_eq!(
        decode(&String::from_utf8(broken).unwrap()).unwrap_err(),
        R::NotAnInvoice
    );
    for bad in ["", "1", "lnbcrt", "bcrt1qqqqqq", "lnbcrt1ä"] {
        assert!(decode(bad).is_err(), "{bad}");
    }
    // Not an invoice but another bech32 text: an address.
    assert_eq!(
        decode("bc1qw508d6qejxtdg4y5r3zarvary0c5xw7kv8f3t4").unwrap_err(),
        R::NotAnInvoice
    );
}

#[test]
fn ln_16_an_invoice_longer_than_the_checksum_covers_is_refused() {
    let long = "q".repeat(MAX_BOLT11_CHARS + 1);
    assert_eq!(decode(&long).unwrap_err(), R::TooLong);
    // One with a lot of route hints still reads, up to the limit.
    let mut m = good();
    m.extra.push(field(3, &vec![1u8; 700]));
    let text = m.write();
    assert!(text.len() > 900 && text.len() <= MAX_BOLT11_CHARS);
    assert!(decode(&text).is_ok());
    m.extra.push(field(3, &vec![1u8; 300]));
    assert_eq!(decode(&m.write()).unwrap_err(), R::TooLong);
}

#[test]
fn ln_16_fields_are_cut_short_or_wrong_in_length_or_twice_are_refused() {
    let mut data = vec![0u8; 7];
    data.extend([1, 1, 20, 0, 0, 0]);
    let mut m = good();
    m.raw = Some(data[7..].to_vec());
    assert_eq!(decode(&m.write()).unwrap_err(), R::Truncated);
    // Fewer groups than a time and a signature.
    assert_eq!(
        decode(&bech32_write("lnbcrt1", &[0; 50])).unwrap_err(),
        R::Truncated
    );
    // A field header cut in two.
    let mut m = good();
    m.raw = Some(vec![1, 1]);
    assert_eq!(decode(&m.write()).unwrap_err(), R::Truncated);

    let with = |raw: Vec<u8>| {
        let mut m = good();
        m.raw = Some(raw);
        decode(&m.write()).unwrap_err()
    };
    let p = field(1, &groups_of(&hash(1)));
    // A payment hash of the wrong length; a node key of the wrong length.
    assert_eq!(
        with(field(1, &groups_of(&[1; 30]))),
        R::BadField("payment hash")
    );
    assert_eq!(
        with([p.clone(), field(19, &groups_of(&[2; 32]))].concat()),
        R::BadField("node key")
    );
    assert_eq!(
        with([p.clone(), field(23, &groups_of(&[2; 31]))].concat()),
        R::BadField("description hash")
    );
    // Not UTF-8.
    assert_eq!(
        with([p.clone(), field(13, &groups_of(&[0xff, 0xfe]))].concat()),
        R::BadField("description")
    );
    // An expiry too long to be a number of 7 groups.
    assert_eq!(
        with([p.clone(), field(6, &[1; 8])].concat()),
        R::BadField("expiry")
    );
    // Twice.
    for (twice, name) in [
        ("hash", "payment hash"),
        ("description", "description"),
        ("expiry", "expiry"),
    ] {
        let mut m = good();
        m.twice = Some(twice);
        assert_eq!(decode(&m.write()).unwrap_err(), R::DuplicateField(name));
    }
    let mut both = good();
    both.description_hash = Some([3; 32]);
    assert_eq!(
        decode(&both.write()).unwrap_err(),
        R::DuplicateField("description")
    );
    // No payment hash.
    assert_eq!(
        with(field(13, &groups_of(b"Sale 17"))),
        R::MissingField("payment hash")
    );
    // A group above 31 in the data cannot be written, but fields of tags the
    // reader does not know are skipped whatever they hold.
    let mut unknown = good();
    unknown.extra.push(field(31, &[1, 2, 3]));
    unknown.extra.push(field(5, &[]));
    assert!(decode(&unknown.write()).is_ok());
}

#[test]
fn ln_16_an_invoice_with_no_expiry_lasts_an_hour() {
    let mut m = good();
    m.expiry = None;
    assert_eq!(decode(&m.write()).unwrap().expiry_secs, 3_600);
    let mut m = good();
    m.expiry = Some(86_400);
    assert_eq!(decode(&m.write()).unwrap().expiry_secs, 86_400);
}

#[test]
fn ln_17_an_invoice_that_is_right_in_every_way_passes() {
    let text = good().write();
    let ok = check(&text, &expect()).expect("passes");
    assert_eq!(ok.bolt11, text);
    assert_eq!(ok.amount_msat, 25_000_000);
    assert_eq!(ok.payment_hash, hash(1));
    assert_eq!(ok.timestamp, 1_700_000_000);
    // In capitals it passes too, and is kept in lower case.
    let upper = check(&text.to_ascii_uppercase(), &expect()).unwrap();
    assert_eq!(upper.bolt11, text);
    // The invoice may name the node's key as the payee.
    let mut m = good();
    m.payee = Some(node_key(1));
    assert!(check(&m.write(), &expect()).is_ok());
}

#[test]
fn ln_17_each_mismatch_refuses_the_invoice() {
    let refused = |m: &Make| check(&m.write(), &expect()).unwrap_err();
    // Another network.
    let mut m = good();
    m.network = "bc";
    assert_eq!(refused(&m), R::WrongNetwork);
    // Another node signed it.
    let mut m = good();
    m.key = secret(2);
    assert_eq!(refused(&m), R::WrongNode);
    // The right node signed it, but it names another as the payee.
    let mut m = good();
    m.payee = Some(node_key(2));
    assert_eq!(refused(&m), R::WrongNode);
    // A recovery number that gives another key.
    let mut m = good();
    m.recid_delta = 1;
    assert!(matches!(
        check(&m.write(), &expect()),
        Err(R::WrongNode) | Err(R::BadSignature)
    ));
    // No amount, or another.
    let mut m = good();
    m.msat = None;
    assert_eq!(refused(&m), R::NoAmount);
    let mut m = good();
    m.msat = Some(25_000_001);
    assert_eq!(
        refused(&m),
        R::WrongAmount {
            asked: 25_000_000,
            got: 25_000_001
        }
    );
    // Another payment hash than the node reported.
    assert_eq!(refused(&Make::good(hash(2))), R::WrongPaymentHash);
    // Another lifetime, and none (an hour).
    let mut m = good();
    m.expiry = Some(3_600);
    assert_eq!(
        refused(&m),
        R::WrongExpiry {
            asked: 300,
            got: 3_600
        }
    );
    let mut m = good();
    m.expiry = None;
    assert_eq!(
        refused(&m),
        R::WrongExpiry {
            asked: 300,
            got: 3_600
        }
    );
    // Another description, a longer one, a hash of one, none.
    for d in [
        Some("Sale 18"),
        Some("Sale 17 "),
        Some("Joe's Cafe: latte"),
        None,
    ] {
        let mut m = good();
        m.description = d.map(str::to_string);
        assert_eq!(refused(&m), R::WrongDescription, "{d:?}");
    }
    let mut m = good();
    m.description = None;
    m.description_hash = Some([5; 32]);
    assert_eq!(refused(&m), R::WrongDescription);
}

#[test]
fn ln_17_the_signer_is_the_node_of_the_settings_not_the_one_the_node_names() {
    // The node of the settings differs from the one that signed.
    let mut e = expect();
    e.node = node_key(2);
    assert_eq!(check(&good().write(), &e).unwrap_err(), R::WrongNode);
    // The check is made before the amount, so a foreign node is named first.
    let mut m = good();
    m.key = secret(3);
    m.msat = Some(1);
    assert_eq!(check(&m.write(), &expect()).unwrap_err(), R::WrongNode);
}

#[test]
fn ln_17_the_check_is_for_the_ask_made() {
    // The test invoice of a connection test has its own amount and text.
    let test = Ask::test(COUNTER, 5);
    let mut m = good();
    m.msat = Some(1_000);
    m.description = Some("OpenBMT test".to_string());
    let e = Expectation {
        ask: test,
        ..expect()
    };
    assert!(check(&m.write(), &e).is_ok());
    // A sale's invoice does not pass as a test.
    assert!(check(&good().write(), &e).is_err());
}
