//! SW-10 to SW-13 and SW-20 to SW-28.

use bitcoin::hex::DisplayHex;

use super::fixtures::*;
use crate::reply::Quote;
use crate::tree::KeyOrder;
use crate::{check, Refused, Reply, MAX_REPLY_BYTES, MIN_CLAIM_BLOCKS, MIN_INVOICE_LIFE_SECS};

fn q(s: &str) -> String {
    format!("\"{s}\"")
}

fn set_invoice(sats: u64, hrp: &'static str, timestamp: u64, expiry: u64) -> String {
    q(&invoice(
        secrets().preimage_hash(),
        sats,
        hrp,
        timestamp,
        expiry,
    ))
}

#[test]
fn a_good_reply_passes_and_keeps_what_the_claim_needs() {
    let swap = good_swap();
    assert_eq!(swap.id, "aBc123xyz");
    assert_eq!(swap.timeout, TIMEOUT);
    assert_eq!(swap.onchain_sats, 99_350);
    assert_eq!(swap.invoice_sats, INVOICE_SATS);
    assert_eq!(swap.invoice_expires_at, NOW - 10 + 3600);
    assert_eq!(swap.order, KeyOrder::RefundFirst);
    assert_eq!(
        swap.lockup_address.to_string(),
        Fx::good().address.trim_matches('"')
    );
    assert!(swap.invoice.starts_with("lntbs"));
}

// SW-10

#[test]
fn sw_10_the_reply_is_bounded_and_strict_json() {
    let good = Fx::good().json();
    assert!(Reply::parse(good.as_bytes()).is_ok());
    let big = format!("{}{}", good, " ".repeat(MAX_REPLY_BYTES));
    assert!(matches!(
        Reply::parse(big.as_bytes()),
        Err(Refused::ReplyNotJson(_))
    ));
    let deep = format!("{}{}", "[".repeat(20), "]".repeat(20));
    assert!(matches!(
        Reply::parse(deep.as_bytes()),
        Err(Refused::ReplyNotJson(_))
    ));
    assert!(Reply::parse(&[0xff, 0xfe]).is_err());
    assert!(Reply::parse(b"").is_err());
    assert!(Reply::parse(format!("{good} x").as_bytes()).is_err());
    // A member twice, however spelled.
    let mut fx = Fx::good();
    fx.extra = ",\"id\":\"other\"".into();
    assert!(matches!(fx.parse(), Err(Refused::ReplyNotJson(_))));
    assert_eq!(fx.parse().unwrap_err().rule(), "SW-10");
}

// SW-11

#[test]
fn sw_11_every_member_is_needed_with_its_type() {
    let good = Fx::good();
    type Field = fn(&mut Fx) -> &mut String;
    let fields: [(&str, Field); 7] = [
        ("id", |f| &mut f.id),
        ("invoice", |f| &mut f.invoice),
        ("lockupAddress", |f| &mut f.address),
        ("refundPublicKey", |f| &mut f.refund_key),
        ("timeoutBlockHeight", |f| &mut f.timeout),
        ("onchainAmount", |f| &mut f.onchain),
        ("claimLeaf", |f| &mut f.claim_leaf),
    ];
    for (name, field) in fields {
        // The wrong type: a number where text goes and text where a number goes.
        let mut fx = good.clone();
        let is_number = matches!(name, "timeoutBlockHeight" | "onchainAmount");
        *field(&mut fx) = if is_number {
            "\"5\"".into()
        } else {
            "5".into()
        };
        assert!(
            matches!(fx.parse(), Err(Refused::BadMember(_))),
            "{name}: {:?}",
            fx.parse()
        );
    }
    for member in [
        "id",
        "invoice",
        "swapTree",
        "lockupAddress",
        "refundPublicKey",
        "timeoutBlockHeight",
        "onchainAmount",
        "claimLeaf",
        "refundLeaf",
        "version",
        "output",
    ] {
        let json = good
            .json()
            .replacen(&format!("\"{member}\":"), "\"zz\":", 1);
        assert_eq!(
            Reply::parse(json.as_bytes()).unwrap_err(),
            Refused::MissingMember(member_static(member)),
            "{member}"
        );
    }
    // A negative or fractional number, and one past 32 bits.
    for bad in ["-1", "1.5", "4294967296", "1e3"] {
        let mut fx = good.clone();
        fx.timeout = bad.into();
        assert!(fx.parse().is_err(), "{bad}");
    }
    let mut fx = good.clone();
    fx.onchain = "-5".into();
    assert!(matches!(
        fx.parse(),
        Err(Refused::BadMember("onchainAmount"))
    ));
}

#[test]
fn sw_11_members_nobody_reads_are_ignored() {
    let mut fx = Fx::good();
    fx.extra = ",\"blindingKey\":\"00\",\"referral\":null,\"nested\":{\"a\":[1,2]}".into();
    assert!(fx.checked().is_ok());
}

#[test]
fn sw_11_the_refund_key_is_a_compressed_key() {
    for bad in [
        q(&"00".repeat(33)),
        q(&format!("05{}", "11".repeat(32))),
        q("zz"),
        // An uncompressed key.
        q(&format!("04{}", "11".repeat(64))),
        q(&service_key().serialize().to_upper_hex_string()[..64]),
    ] {
        let mut fx = Fx::good();
        fx.refund_key = bad.clone();
        assert!(
            matches!(fx.parse(), Err(Refused::BadMember("refundPublicKey"))),
            "{bad}"
        );
    }
}

fn member_static(m: &str) -> &'static str {
    [
        "id",
        "invoice",
        "swapTree",
        "lockupAddress",
        "refundPublicKey",
        "timeoutBlockHeight",
        "onchainAmount",
        "claimLeaf",
        "refundLeaf",
        "version",
        "output",
    ]
    .into_iter()
    .find(|x| *x == m)
    .unwrap()
}

// SW-12

#[test]
fn sw_12_the_id_is_short_and_plain() {
    for bad in [
        "\"\"",
        "\"has space\"",
        "\"a/b\"",
        "\"\u{e9}\"",
        &q(&"a".repeat(65)),
    ] {
        let mut fx = Fx::good();
        fx.id = bad.to_string();
        assert!(matches!(fx.parse(), Err(Refused::BadMember("id"))), "{bad}");
    }
    let mut fx = Fx::good();
    fx.id = q(&"a-B_9".repeat(12));
    assert!(fx.parse().is_ok());
}

// SW-13

#[test]
fn sw_13_a_leaf_is_tapscript_hex_and_short() {
    let mut fx = Fx::good();
    fx.claim_version = "193".into();
    assert!(matches!(fx.parse(), Err(Refused::BadMember("claimLeaf"))));
    let mut fx = Fx::good();
    fx.refund_version = "0".into();
    assert!(matches!(fx.parse(), Err(Refused::BadMember("refundLeaf"))));
    for bad in ["\"\"", "\"zz\"", "\"abc\"", &q(&"ab".repeat(129))] {
        let mut fx = Fx::good();
        fx.refund_leaf = bad.to_string();
        assert!(
            matches!(fx.parse(), Err(Refused::BadMember("refundLeaf"))),
            "{bad}"
        );
    }
}

// SW-20 to SW-23: the invoice

#[test]
fn sw_20_the_invoice_pays_the_hash_the_shop_made() {
    let mut fx = Fx::good();
    fx.invoice = q(&invoice([1; 32], INVOICE_SATS, "tbs", NOW - 10, 3600));
    assert_eq!(fx.checked().unwrap_err(), Refused::WrongPaymentHash);
    let mut fx = Fx::good();
    fx.invoice = q("not an invoice");
    assert!(matches!(
        fx.checked().unwrap_err(),
        Refused::InvoiceUnreadable(_)
    ));
    let mut fx = Fx::good();
    // A letter of the invoice changed: the checksum fails.
    let mut text = fx.invoice.clone();
    let at = text.len() - 10;
    let c = if &text[at..at + 1] == "q" { "p" } else { "q" };
    text.replace_range(at..at + 1, c);
    fx.invoice = text;
    assert_eq!(fx.checked().unwrap_err().rule(), "SW-20");
}

#[test]
fn sw_21_the_invoice_is_for_exactly_the_amount_asked() {
    for sats in [INVOICE_SATS - 1, INVOICE_SATS + 1, INVOICE_SATS * 10] {
        let mut fx = Fx::good();
        fx.invoice = set_invoice(sats, "tbs", NOW - 10, 3600);
        assert_eq!(
            fx.checked().unwrap_err(),
            Refused::WrongInvoiceAmount {
                asked: INVOICE_SATS * 1000,
                got: sats * 1000
            }
        );
    }
    // No amount at all.
    let mut make = openbmt_lightning::testing::Make::good(secrets().preimage_hash());
    make.network = "tbs";
    make.msat = None;
    make.timestamp = NOW - 10;
    make.expiry = Some(3600);
    let mut fx = Fx::good();
    fx.invoice = q(&make.write());
    assert_eq!(fx.checked().unwrap_err(), Refused::NoInvoiceAmount);
}

#[test]
fn sw_22_the_invoice_is_for_the_shops_network() {
    for hrp in ["bc", "tb", "bcrt"] {
        let mut fx = Fx::good();
        fx.invoice = set_invoice(INVOICE_SATS, hrp, NOW - 10, 3600);
        assert_eq!(
            fx.checked().unwrap_err(),
            Refused::WrongInvoiceNetwork,
            "{hrp}"
        );
    }
}

#[test]
fn sw_23_the_invoice_has_time_left() {
    // Out at the time given, one second short of the floor, and exactly at it.
    let cases = [
        (NOW - 4000, 3600, false),
        (NOW - 3600, 3600, false),
        (NOW, MIN_INVOICE_LIFE_SECS - 1, false),
        (NOW, MIN_INVOICE_LIFE_SECS, true),
        (NOW, 3600, true),
    ];
    for (timestamp, expiry, ok) in cases {
        let mut fx = Fx::good();
        fx.invoice = set_invoice(INVOICE_SATS, "tbs", timestamp, expiry);
        let got = fx.checked();
        if ok {
            assert!(got.is_ok(), "{timestamp} {expiry}");
        } else {
            assert_eq!(got.unwrap_err(), Refused::InvoiceTooShort);
        }
    }
}

// SW-24 to SW-26: the tree

#[test]
fn sw_24_the_refund_key_is_not_the_claim_key() {
    let mut fx = Fx::good();
    fx.refund_key = q(&secrets().claim_public().serialize().to_lower_hex_string());
    assert_eq!(fx.checked().unwrap_err(), Refused::BadRefundKey);
}

#[test]
fn sw_25_the_leaves_are_the_leaves_rebuilt() {
    let s = secrets();
    let (claim, refund) = leaves(TIMEOUT);
    // The layout, written out: SIZE 32 EQUALVERIFY HASH160 <20> EQUALVERIFY
    // <32> CHECKSIG, and <32> CHECKSIGVERIFY <timeout> CLTV.
    let c = claim.as_bytes();
    assert_eq!(&c[..5], [0x82, 0x01, 0x20, 0x88, 0xa9]);
    assert_eq!(c[5], 0x14);
    assert_eq!(&c[26..28], [0x88, 0x20]);
    assert_eq!(
        &c[28..60],
        s.claim_public().x_only_public_key().0.serialize()
    );
    assert_eq!(c[60], 0xac);
    assert_eq!(c.len(), 61);
    let r = refund.as_bytes();
    assert_eq!(r[0], 0x20);
    assert_eq!(r[33], 0xad);
    assert_eq!(&r[34..], [0x03, 0x20, 0xd1, 0x03, 0xb1]);
    assert_eq!(r[r.len() - 1], 0xb1);
    // A leaf changed in one place is refused, whichever place.
    for (field, leaf) in [("claim", &claim), ("refund", &refund)] {
        for i in 0..leaf.len() {
            let mut bytes = leaf.as_bytes().to_vec();
            bytes[i] ^= 1;
            let mut fx = Fx::good();
            let text = q(&bytes.to_lower_hex_string());
            if field == "claim" {
                fx.claim_leaf = text;
            } else {
                fx.refund_leaf = text;
            }
            assert_eq!(
                fx.checked().unwrap_err(),
                Refused::LeafMismatch(if field == "claim" { "claim" } else { "refund" }),
                "{field} byte {i}"
            );
        }
    }
    // A claim leaf that another hash opens, or another key spends.
    let other = crate::tree::claim_script(&service_key(), &s.preimage_hash());
    let mut fx = Fx::good();
    fx.claim_leaf = q(&other.as_bytes().to_lower_hex_string());
    assert_eq!(fx.checked().unwrap_err(), Refused::LeafMismatch("claim"));
}

#[test]
fn sw_25_a_refund_leaf_for_another_timeout_is_refused() {
    let (_, refund) = leaves(TIMEOUT - 1);
    let mut fx = Fx::good();
    fx.refund_leaf = q(&refund.as_bytes().to_lower_hex_string());
    assert_eq!(fx.checked().unwrap_err(), Refused::LeafMismatch("refund"));
}

#[test]
fn sw_26_the_address_is_the_one_of_the_rebuilt_output() {
    let other = crate::tree::SwapTree::build(
        &secrets().claim_public(),
        &service_key(),
        &[0; 32],
        TIMEOUT,
        KeyOrder::RefundFirst,
    )
    .unwrap();
    let mut fx = Fx::good();
    fx.address = q(&other.address(NET).to_string());
    assert_eq!(fx.checked().unwrap_err(), Refused::WrongLockupAddress);
    // The right output on another network, a text that is no address, and the
    // service's own key as the address.
    let good = good_swap();
    let on_main = good.tree.address(bitcoin::Network::Bitcoin).to_string();
    for bad in [on_main, "garbage".to_string(), "".to_string()] {
        let mut fx = Fx::good();
        fx.address = q(&bad);
        assert_eq!(
            fx.checked().unwrap_err(),
            Refused::WrongLockupAddress,
            "{bad}"
        );
    }
}

#[test]
fn sw_26_only_the_refund_key_first_order_is_taken() {
    let swap = Fx::good().checked().unwrap();
    assert_eq!(swap.order, KeyOrder::RefundFirst);
    // The same tree with the keys swapped: its address is refused.
    let s = secrets();
    let internal = crate::keys::key_agg(&[s.claim_public(), service_key()]).unwrap();
    let swapped = {
        let claim = crate::tree::claim_script(&s.claim_public(), &s.preimage_hash());
        let refund = crate::tree::refund_script(&service_key(), TIMEOUT);
        let info = bitcoin::taproot::TaprootBuilder::new()
            .add_leaf(1, claim)
            .unwrap()
            .add_leaf(1, refund)
            .unwrap()
            .finalize(
                &bitcoin::secp256k1::Secp256k1::verification_only(),
                internal,
            )
            .unwrap();
        bitcoin::Address::p2tr_tweaked(info.output_key(), NET)
    };
    assert_ne!(swapped.script_pubkey(), swap.lockup_address.script_pubkey());
    let mut fx = Fx::good();
    fx.address = q(&swapped.to_string());
    assert_eq!(fx.checked().unwrap_err(), Refused::WrongLockupAddress);
}

// SW-27: the timeout

#[test]
fn sw_27_the_timeout_leaves_blocks_for_the_claim() {
    let least = TIP + u32::try_from(MIN_CLAIM_BLOCKS).unwrap();
    for (timeout, ok) in [(least - 1, false), (least, true), (TIP, false), (0, false)] {
        let s = secrets();
        let tree = crate::tree::SwapTree::build(
            &s.claim_public(),
            &service_key(),
            &s.preimage_hash(),
            timeout,
            KeyOrder::RefundFirst,
        )
        .unwrap();
        let mut fx = Fx::good();
        fx.timeout = timeout.to_string();
        fx.address = q(&tree.address(NET).to_string());
        fx.refund_leaf = q(&leaves(timeout).1.as_bytes().to_lower_hex_string());
        let got = fx.checked();
        if ok {
            assert!(got.is_ok(), "{timeout}");
        } else {
            assert_eq!(got.unwrap_err(), Refused::TimeoutTooSoon, "{timeout}");
        }
    }
}

#[test]
fn sw_27_a_time_is_not_a_height() {
    let s = secrets();
    let timeout = 500_000_000u32;
    let tree = crate::tree::SwapTree::build(
        &s.claim_public(),
        &service_key(),
        &s.preimage_hash(),
        timeout,
        KeyOrder::RefundFirst,
    )
    .unwrap();
    let mut fx = Fx::good();
    fx.timeout = timeout.to_string();
    fx.address = q(&tree.address(NET).to_string());
    fx.refund_leaf = q(&leaves(timeout).1.as_bytes().to_lower_hex_string());
    assert_eq!(fx.checked().unwrap_err(), Refused::TimeoutTooSoon);
}

// SW-28: the amount

#[test]
fn sw_28_the_amount_on_chain_is_above_the_floor_and_the_quote() {
    // The quote leaves 99,350 of 100,000.
    for (onchain, want) in [
        ("99350", None),
        ("99351", None),
        (
            "99349",
            Some(Refused::OnchainBelowQuote {
                least: 99_350,
                got: 99_349,
            }),
        ),
        ("100001", Some(Refused::OnchainAboveInvoice)),
        ("100000", None),
        ("999", Some(Refused::OnchainTooSmall)),
        ("0", Some(Refused::OnchainTooSmall)),
    ] {
        let mut fx = Fx::good();
        fx.onchain = onchain.into();
        match want {
            None => assert!(fx.checked().is_ok(), "{onchain}"),
            Some(e) => assert_eq!(fx.checked().unwrap_err(), e, "{onchain}"),
        }
    }
}

#[test]
fn sw_28_the_quote_rounds_the_percentage_up() {
    let quote = Quote {
        fee_ppm: 2_500,
        lockup_fee_sats: 0,
    };
    assert_eq!(quote.least_onchain(100_000), 99_750);
    // 0.25 % of 10,001 is 25.0025, so 26 is taken off.
    assert_eq!(quote.least_onchain(10_001), 9_975);
    let all = Quote {
        fee_ppm: 1_000_000,
        lockup_fee_sats: 5,
    };
    assert_eq!(all.least_onchain(1_000), 0);
    let s = secrets();
    // A quote that leaves nothing asks nothing of the service; the floor
    // still holds, and nothing panics.
    let mut e = expect(&s);
    e.quote = all;
    let mut fx = Fx::good();
    assert!(check(&fx.parse().unwrap(), &e).is_ok());
    fx.onchain = "999".into();
    assert_eq!(
        check(&fx.parse().unwrap(), &e).unwrap_err(),
        Refused::OnchainTooSmall
    );
}

#[test]
fn sw_2x_every_refusal_names_its_rule() {
    let e = Refused::WrongPaymentHash.to_string();
    assert!(e.starts_with("SW-20: "));
    assert!(Refused::TimeoutTooSoon.to_string().starts_with("SW-27: "));
}
