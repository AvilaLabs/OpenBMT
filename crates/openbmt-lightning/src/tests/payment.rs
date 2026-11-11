//! LN-13, LN-14, LN-21.

use super::*;
use crate::error::{AskRefused, ReplyRefused};
use crate::invoice::{sale_description, MAX_EXPIRY_SECS, MAX_MSAT};
use crate::{NotPaid, PaidFacts, Payment};

#[test]
fn ln_13_the_description_is_the_sale_number_and_nothing_else() {
    assert_eq!(sale_description(17), "Sale 17");
    assert_eq!(sale_description(0), "Sale 0");
    assert_eq!(sale_description(u64::MAX), "Sale 18446744073709551615");
    assert_eq!(ask().description(), "Sale 17");
}

#[test]
fn ln_13_the_amount_is_in_millisatoshis_and_the_lifetime_is_the_quotes() {
    let a = ask();
    assert_eq!(a.amount_msat(), 25_000_000);
    assert_eq!(a.expiry_secs(), 300);
    // The requests carry both, for either node.
    let lnd = lnd().create_request(&a).body.unwrap();
    let cln = cln().create_request(&a).body.unwrap();
    assert!(lnd.contains("25000000") && lnd.contains("\"300\""));
    assert!(cln.contains("25000000") && cln.contains(":300"));
}

#[test]
fn ln_13_an_amount_or_a_lifetime_out_of_range_is_not_asked_for() {
    let sale = |msat, secs| Ask::sale(COUNTER, 1, 0, msat, secs);
    assert!(sale(1, 1).is_ok());
    assert!(sale(MAX_MSAT, MAX_EXPIRY_SECS).is_ok());
    assert_eq!(sale(0, 300).unwrap_err(), AskRefused::BadAmount);
    assert_eq!(sale(MAX_MSAT + 1, 300).unwrap_err(), AskRefused::BadAmount);
    assert_eq!(sale(u64::MAX, 300).unwrap_err(), AskRefused::BadAmount);
    assert_eq!(sale(1_000, 0).unwrap_err(), AskRefused::BadExpiry);
    assert_eq!(
        sale(1_000, MAX_EXPIRY_SECS + 1).unwrap_err(),
        AskRefused::BadExpiry
    );
}

fn lnd_lookup(state: &str, paid: Option<&str>, settled: Option<&str>) -> Reply {
    let h = hash(1);
    let mut body = format!(
        "{{\"r_hash\":\"{}\",\"value_msat\":\"25000000\",\"state\":\"{state}\"",
        base64(&h, Alphabet::Standard)
    );
    if let Some(p) = paid {
        body.push_str(&format!(",\"amt_paid_msat\":\"{p}\""));
    }
    if let Some(s) = settled {
        body.push_str(&format!(",\"settle_date\":\"{s}\""));
    }
    body.push('}');
    reply(200, &body)
}

fn cln_lookup(status: &str, received: Option<u64>, paid_at: Option<u64>) -> Reply {
    let mut item = format!(
        "{{\"label\":\"x\",\"payment_hash\":\"{}\",\"status\":\"{status}\",\
         \"amount_msat\":25000000",
        hex(&hash(1))
    );
    if let Some(r) = received {
        item.push_str(&format!(",\"amount_received_msat\":{r}"));
    }
    if let Some(t) = paid_at {
        item.push_str(&format!(",\"paid_at\":{t}"));
    }
    item.push('}');
    reply(200, &format!("{{\"invoices\":[{item}]}}"))
}

fn lookup(c: &Credential, r: &Reply) -> Result<Payment, ReplyRefused> {
    c.parse_lookup(r, &hash(1), 25_000_000)
}

#[test]
fn ln_14_lnd_is_paid_only_when_settled_for_at_least_the_amount() {
    let paid = lookup(
        &lnd(),
        &lnd_lookup("SETTLED", Some("25000000"), Some("1700000123")),
    );
    assert_eq!(
        paid,
        Ok(Payment::Paid(PaidFacts {
            payment_hash: hash(1),
            amount_received_msat: 25_000_000,
            settled_at: Some(1_700_000_123)
        }))
    );
    // More is paid; the facts say how much.
    let over = lookup(&lnd(), &lnd_lookup("SETTLED", Some("26000000"), None)).unwrap();
    assert_eq!(
        over,
        Payment::Paid(PaidFacts {
            payment_hash: hash(1),
            amount_received_msat: 26_000_000,
            settled_at: None
        })
    );
    // A settle time of 0 is no time.
    let zero = lookup(&lnd(), &lnd_lookup("SETTLED", Some("25000000"), Some("0"))).unwrap();
    assert!(matches!(
        zero,
        Payment::Paid(PaidFacts {
            settled_at: None,
            ..
        })
    ));
    // Less is not.
    assert_eq!(
        lookup(&lnd(), &lnd_lookup("SETTLED", Some("24999999"), None)),
        Ok(Payment::NotPaid(NotPaid::TooLittle {
            asked: 25_000_000,
            received: 24_999_999
        }))
    );
    // Without the amount received a settled invoice is not read.
    assert_eq!(
        lookup(&lnd(), &lnd_lookup("SETTLED", None, None)),
        Err(ReplyRefused::Missing("amt_paid_msat"))
    );
}

#[test]
fn ln_14_lnd_states_other_than_settled_are_not_paid() {
    for (state, want) in [
        ("OPEN", NotPaid::Waiting),
        ("CANCELED", NotPaid::Cancelled),
        ("ACCEPTED", NotPaid::Held),
        ("HOLD", NotPaid::Unknown("HOLD".to_string())),
        ("settled", NotPaid::Unknown("settled".to_string())),
        ("", NotPaid::Unknown(String::new())),
    ] {
        // Even with an amount that would be enough.
        let r = lnd_lookup(state, Some("25000000"), Some("1700000123"));
        assert_eq!(lookup(&lnd(), &r), Ok(Payment::NotPaid(want)), "{state}");
    }
    // A state with characters that hide text is cleaned.
    let r = lnd_lookup("A\\u202eB", None, None);
    assert_eq!(
        lookup(&lnd(), &r),
        Ok(Payment::NotPaid(NotPaid::Unknown("AB".to_string())))
    );
    // No state, a state that is not text.
    let no_state = reply(
        200,
        &format!(
            "{{\"r_hash\":\"{}\"}}",
            base64(&hash(1), Alphabet::Standard)
        ),
    );
    assert_eq!(
        lookup(&lnd(), &no_state),
        Err(ReplyRefused::Missing("state"))
    );
}

#[test]
fn ln_14_cln_is_paid_only_when_paid_for_at_least_the_amount() {
    assert_eq!(
        lookup(
            &cln(),
            &cln_lookup("paid", Some(25_000_000), Some(1_700_000_123))
        ),
        Ok(Payment::Paid(PaidFacts {
            payment_hash: hash(1),
            amount_received_msat: 25_000_000,
            settled_at: Some(1_700_000_123)
        }))
    );
    // Core Lightning may receive more than the invoice named.
    assert!(matches!(
        lookup(&cln(), &cln_lookup("paid", Some(30_000_000), None)),
        Ok(Payment::Paid(PaidFacts {
            amount_received_msat: 30_000_000,
            ..
        }))
    ));
    assert_eq!(
        lookup(&cln(), &cln_lookup("paid", Some(24_999_999), None)),
        Ok(Payment::NotPaid(NotPaid::TooLittle {
            asked: 25_000_000,
            received: 24_999_999
        }))
    );
    assert_eq!(
        lookup(&cln(), &cln_lookup("paid", None, None)),
        Err(ReplyRefused::Missing("amount_received_msat"))
    );
}

#[test]
fn ln_14_cln_states_other_than_paid_are_not_paid() {
    for (status, want) in [
        ("unpaid", NotPaid::Waiting),
        ("expired", NotPaid::Expired),
        ("pending", NotPaid::Unknown("pending".to_string())),
        ("PAID", NotPaid::Unknown("PAID".to_string())),
    ] {
        assert_eq!(
            lookup(&cln(), &cln_lookup(status, Some(25_000_000), Some(5))),
            Ok(Payment::NotPaid(want)),
            "{status}"
        );
    }
    // No invoice listed: the node has none.
    assert_eq!(
        lookup(&cln(), &reply(200, "{\"invoices\":[]}")),
        Ok(Payment::NotPaid(NotPaid::NotFound))
    );
    // Two invoices where one was asked for.
    let item = format!(
        "{{\"payment_hash\":\"{}\",\"status\":\"unpaid\"}}",
        hex(&hash(1))
    );
    let two = reply(200, &format!("{{\"invoices\":[{item},{item}]}}"));
    assert_eq!(lookup(&cln(), &two), Err(ReplyRefused::Several));
    // A number as a text of digits is not what Core Lightning writes.
    let text = reply(
        200,
        &format!(
            "{{\"invoices\":[{{\"payment_hash\":\"{}\",\"status\":\"paid\",\
             \"amount_msat\":25000000,\"amount_received_msat\":\"25000000msat\"}}]}}",
            hex(&hash(1))
        ),
    );
    assert_eq!(
        lookup(&cln(), &text),
        Err(ReplyRefused::WrongType("amount_received_msat"))
    );
}

#[test]
fn ln_14_a_reply_about_another_payment_or_an_error_is_never_paid() {
    let other = hash(2);
    let lnd_other = reply(
        200,
        &format!(
            "{{\"r_hash\":\"{}\",\"state\":\"SETTLED\",\"amt_paid_msat\":\"25000000\"}}",
            base64(&other, Alphabet::Standard)
        ),
    );
    assert_eq!(lookup(&lnd(), &lnd_other), Err(ReplyRefused::OtherInvoice));
    let cln_other = reply(
        200,
        &format!(
            "{{\"invoices\":[{{\"payment_hash\":\"{}\",\"status\":\"paid\",\
             \"amount_received_msat\":25000000}}]}}",
            hex(&other)
        ),
    );
    assert_eq!(lookup(&cln(), &cln_other), Err(ReplyRefused::OtherInvoice));
    for c in [lnd(), cln()] {
        for r in [
            reply(500, "{\"message\":\"x\"}"),
            reply(200, "not json"),
            reply(200, ""),
            reply(200, "null"),
            reply(200, "{}"),
        ] {
            assert!(lookup(&c, &r).is_err(), "{r:?}");
        }
    }
    // LND answers 404 with the code 5 for an invoice it has not got, in two
    // texts, and only so.
    for message in ["unable to locate invoice", "there are no existing invoices"] {
        assert_eq!(
            lookup(
                &lnd(),
                &reply(
                    404,
                    &format!("{{\"code\":5,\"message\":\"{message}\",\"details\":[]}}")
                )
            ),
            Ok(Payment::NotPaid(NotPaid::NotFound)),
            "{message}"
        );
    }
    for r in [
        reply(
            404,
            "{\"code\":5,\"message\":\"page not found\",\"details\":[]}",
        ),
        reply(
            404,
            "{\"code\":12,\"message\":\"unable to locate invoice\"}",
        ),
        reply(404, ""),
        reply(500, "{\"code\":5,\"message\":\"unable to locate invoice\"}"),
        reply(
            500,
            "{\"code\":2,\"message\":\"payment hash must be exactly 32 bytes, is instead 3\"}",
        ),
    ] {
        assert!(
            matches!(lookup(&lnd(), &r), Err(ReplyRefused::NodeSaid { .. })),
            "{r:?}"
        );
    }
}

#[test]
fn ln_14_lnd_writes_int64_as_text_and_a_number_there_is_refused() {
    let h = hash(1);
    let body = |paid: &str, settled: &str| {
        reply(
            200,
            &format!(
                "{{\"r_hash\":\"{}\",\"value_msat\":\"25000000\",\"state\":\"SETTLED\",\
                 \"amt_paid_msat\":{paid},\"settle_date\":{settled}}}",
                base64(&h, Alphabet::Standard)
            ),
        )
    };
    assert!(lookup(&lnd(), &body("\"25000000\"", "\"1700000123\"")).is_ok());
    assert_eq!(
        lookup(&lnd(), &body("25000000", "\"1700000123\"")),
        Err(ReplyRefused::WrongType("amt_paid_msat"))
    );
    assert_eq!(
        lookup(&lnd(), &body("\"25000000\"", "1700000123")),
        Err(ReplyRefused::WrongType("settle_date"))
    );
    for text in [
        "\"\"",
        "\"-1\"",
        "\"2.5e7\"",
        "\"25000000msat\"",
        "\"99999999999999999999\"",
    ] {
        assert_eq!(
            lookup(&lnd(), &body(text, "\"1700000123\"")),
            Err(ReplyRefused::OutOfRange("amt_paid_msat")),
            "{text}"
        );
    }
}

#[test]
fn ln_14_a_reply_too_big_or_too_deep_is_refused() {
    let big = reply(200, &format!("{{\"state\":\"{}\"}}", "x".repeat(300_000)));
    assert_eq!(lookup(&lnd(), &big), Err(ReplyRefused::TooLarge));
    let deep = reply(200, &format!("{}{}", "[".repeat(40), "]".repeat(40)));
    assert_eq!(lookup(&cln(), &deep), Err(ReplyRefused::TooLarge));
    let twice = reply(200, "{\"state\":\"OPEN\",\"state\":\"SETTLED\"}");
    assert_eq!(lookup(&lnd(), &twice), Err(ReplyRefused::Unreadable));
}

#[test]
fn ln_21_a_paid_invoice_is_final_and_gives_its_facts() {
    let Ok(Payment::Paid(facts)) = lookup(
        &lnd(),
        &lnd_lookup("SETTLED", Some("25000000"), Some("1700000123")),
    ) else {
        panic!("paid")
    };
    // The hash, the amount received and the time the node settled it.
    assert_eq!(facts.payment_hash, hash(1));
    assert_eq!(facts.amount_received_msat, 25_000_000);
    assert_eq!(facts.settled_at, Some(1_700_000_123));
    // Its words tell the cashier to hand over the goods, in green.
    let w = facts.words();
    assert_eq!(w.tone, openbmt_core::say::Tone::Good);
    assert!(w.next.contains("Hand over the goods"));
    assert_eq!(w.at, Some(1_700_000_123));
    // No state of an invoice that is not paid says so.
    for n in crate::say::all_not_paid() {
        assert_ne!(n.words().tone, openbmt_core::say::Tone::Good, "{n:?}");
    }
}
