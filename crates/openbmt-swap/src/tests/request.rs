//! SW-01 to SW-04.

use bitcoin::hashes::{sha256, Hash};
use bitcoin::hex::DisplayHex;
use bitcoin::secp256k1::PublicKey;
use openbmt_json::{Limits, Value};

use super::fixtures::*;
use crate::{Refused, Request, Secrets, MAX_DESCRIPTION_BYTES, MAX_INVOICE_SATS, MIN_INVOICE_SATS};

#[test]
fn sw_01_the_request_has_exactly_the_members_of_the_api_in_order() {
    let s = secrets();
    let r = Request::new(&s, 100_000, "Sale 17").unwrap();
    let want = format!(
        "{{\"from\":\"BTC\",\"to\":\"BTC\",\"invoiceAmount\":100000,\"preimageHash\":\"{}\",\
         \"claimPublicKey\":\"{}\",\"description\":\"Sale 17\"}}",
        s.preimage_hash().to_lower_hex_string(),
        s.claim_public().serialize().to_lower_hex_string()
    );
    assert_eq!(r.to_json(), want);
    let doc = openbmt_json::parse(r.to_json().as_bytes(), &Limits::DEFAULT).unwrap();
    let keys: Vec<&str> = doc
        .as_object()
        .unwrap()
        .iter()
        .map(|(k, _)| k.as_str())
        .collect();
    assert_eq!(
        keys,
        [
            "from",
            "to",
            "invoiceAmount",
            "preimageHash",
            "claimPublicKey",
            "description"
        ]
    );
    assert_eq!(doc.get("invoiceAmount"), Some(&Value::Int(100_000)));
    assert_eq!(r.amount_sats(), 100_000);
}

#[test]
fn sw_01_the_hash_and_key_in_the_request_are_the_secrets() {
    let s = secrets();
    let r = Request::new(&s, 50_000, "x").unwrap().to_json();
    let doc = openbmt_json::parse(r.as_bytes(), &Limits::DEFAULT).unwrap();
    let hash = doc.get("preimageHash").and_then(Value::as_str).unwrap();
    assert_eq!(
        hash,
        sha256::Hash::hash(s.preimage())
            .to_byte_array()
            .to_lower_hex_string()
    );
    assert!(!r.contains(&s.preimage().to_lower_hex_string()));
    let key = doc.get("claimPublicKey").and_then(Value::as_str).unwrap();
    assert_eq!(key.len(), 66);
    assert!(PublicKey::from_slice(&hex(key)).is_ok());
}

fn hex(s: &str) -> Vec<u8> {
    use bitcoin::hex::FromHex;
    Vec::from_hex(s).unwrap()
}

#[test]
fn sw_02_the_amount_is_inside_the_band() {
    let s = secrets();
    for bad in [0, MIN_INVOICE_SATS - 1, MAX_INVOICE_SATS + 1, u64::MAX] {
        assert_eq!(
            Request::new(&s, bad, "x").unwrap_err(),
            Refused::AmountOutOfBand
        );
    }
    assert!(Request::new(&s, MIN_INVOICE_SATS, "x").is_ok());
    assert!(Request::new(&s, MAX_INVOICE_SATS, "x").is_ok());
}

#[test]
fn sw_03_the_description_is_bounded_and_escaped() {
    let s = secrets();
    let long = "a".repeat(MAX_DESCRIPTION_BYTES + 1);
    for bad in ["", long.as_str(), "a\nb", "a\u{0}b", "tab\t"] {
        assert_eq!(
            Request::new(&s, 50_000, bad).unwrap_err(),
            Refused::BadDescription,
            "{bad:?}"
        );
    }
    let ok = "a".repeat(MAX_DESCRIPTION_BYTES);
    assert!(Request::new(&s, 50_000, &ok).is_ok());
    // A quote, a backslash and a non-ASCII letter survive the round trip.
    let r = Request::new(&s, 50_000, "caf\u{e9} \"x\" \\ y").unwrap();
    let doc = openbmt_json::parse(r.to_json().as_bytes(), &Limits::DEFAULT).unwrap();
    assert_eq!(
        doc.get("description").and_then(Value::as_str),
        Some("caf\u{e9} \"x\" \\ y")
    );
}

#[test]
fn sw_04_the_secrets_come_from_the_bytes_given_and_refuse_bad_ones() {
    assert!(Secrets::from_random([0; 32], [9; 32]).is_err());
    assert!(Secrets::from_random([7; 32], [0; 32]).is_err());
    assert!(Secrets::from_random([7; 32], [0xff; 32]).is_err());
    let a = Secrets::from_random([1; 32], [2; 32]).unwrap();
    let b = Secrets::from_random([1; 32], [2; 32]).unwrap();
    let c = Secrets::from_random([1; 32], [3; 32]).unwrap();
    assert_eq!(a, b);
    assert_ne!(a.claim_public(), c.claim_public());
    assert_eq!(
        a.preimage_hash(),
        sha256::Hash::hash(&[1; 32]).to_byte_array()
    );
    assert_eq!(a.claim_key().secret_bytes(), [2; 32]);
    assert_eq!(
        Secrets::from_random([0; 32], [9; 32]).unwrap_err().rule(),
        "SW-04"
    );
}

#[test]
fn sw_04_secrets_do_not_print() {
    let s = Secrets::from_random([0xab; 32], [0xcd; 32]).unwrap();
    let shown = format!("{s:?} {s:#?}");
    assert!(!shown.contains("ab") && !shown.contains("cd") && !shown.contains("171"));
    assert!(!shown.contains(&s.preimage_hash().to_lower_hex_string()));
}
