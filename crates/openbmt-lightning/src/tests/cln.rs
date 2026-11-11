//! LN-09, LN-10, LN-15 for Core Lightning.

use openbmt_json::{parse, Limits, Value};

use super::*;
use crate::connect::Probe;
use crate::error::ReplyRefused;
use crate::request::Method;
use crate::{cln as node, CreateOutcome, NotPaid, PaidFacts, Payment};

fn body_of(r: &crate::Request) -> Value {
    parse(r.body.as_deref().unwrap().as_bytes(), &Limits::DEFAULT).unwrap()
}

#[test]
fn ln_10_an_invoice_is_made_with_a_post_and_the_rune_in_its_header() {
    let r = cln().create_request(&ask());
    assert_eq!(r.method, Method::Post);
    assert_eq!(r.path, "/v1/invoice");
    assert_eq!(r.header_name, "Rune");
    assert_eq!(r.header_value, rune(INVOICE_RUNE));
    let b = body_of(&r);
    assert_eq!(
        b.get("amount_msat").and_then(Value::as_u64),
        Some(25_000_000)
    );
    assert_eq!(b.get("expiry").and_then(Value::as_u64), Some(300));
    assert_eq!(
        b.get("description").and_then(Value::as_str),
        Some("Sale 17")
    );
    assert_eq!(
        b.get("label").and_then(Value::as_str),
        Some("obmt-c0ffee01-17-0")
    );
    assert_eq!(b.as_object().map(<[_]>::len), Some(4));
}

#[test]
fn ln_10_the_reply_to_an_invoice_gives_the_hash_and_the_invoice() {
    let h = hash(1);
    let made = cln().parse_create(&cln_created(&h, "lnbcrt1abc")).unwrap();
    assert_eq!(
        made,
        CreateOutcome::Created(Created {
            payment_hash: h,
            bolt11: "lnbcrt1abc".to_string()
        })
    );
    let bad = |body: &str| cln().parse_create(&reply(200, body)).unwrap_err();
    assert_eq!(
        bad("{\"bolt11\":\"x\"}"),
        ReplyRefused::Missing("payment_hash")
    );
    assert_eq!(
        bad(&format!("{{\"payment_hash\":\"{}\"}}", hex(&h))),
        ReplyRefused::Missing("bolt11")
    );
    assert_eq!(
        bad("{\"bolt11\":\"x\",\"payment_hash\":\"abcd\"}"),
        ReplyRefused::OutOfRange("payment_hash")
    );
    assert_eq!(
        bad(&format!(
            "{{\"bolt11\":7,\"payment_hash\":\"{}\"}}",
            hex(&h)
        )),
        ReplyRefused::WrongType("bolt11")
    );
    let e = cln()
        .parse_create(&reply(500, "{\"code\":-1,\"message\":\"out of memory\"}"))
        .unwrap_err();
    assert!(matches!(e, ReplyRefused::NodeSaid { status: 500, .. }));
}

#[test]
fn ln_10_the_payment_is_polled_by_hash_and_waitinvoice_is_not_used() {
    let h = hash(1);
    let r = cln().lookup_request(&h);
    assert_eq!(r.path, "/v1/listinvoices");
    assert_eq!(
        body_of(&r).get("payment_hash").and_then(Value::as_str),
        Some(hex(&h).as_str())
    );
    let ask = ask();
    for c in [cln(), lnd()] {
        for r in [c.create_request(&ask), c.lookup_request(&h)] {
            assert!(!r.path.contains("waitinvoice"));
        }
    }
    let Credential::Cln(c) = cln() else {
        panic!("cln")
    };
    for (_, r) in node::probes(&c) {
        assert!(!r.path.contains("waitinvoice"));
    }
}

#[test]
fn ln_09_a_rune_that_is_refused_as_not_permitted_is_limited() {
    // The exact answer: HTTP 401, code 1502, the text of `checkrune`.
    for text in [
        "Not permitted: method is not equal to invoice",
        "Not permitted: method does not start with list",
    ] {
        let r = reply(
            401,
            &format!("{{\"code\":1502,\"message\":\"{text}\",\"data\":null}}"),
        );
        assert_eq!(node::judge_probe(&r), Probe::Denied, "{text}");
    }
}

#[test]
fn ln_09_only_the_exact_answer_is_a_denial() {
    let ok = "{\"code\":1502,\"message\":\"Not permitted: method is not equal to invoice\"}";
    for status in [400, 403, 500] {
        assert_eq!(
            node::judge_probe(&reply(status, ok)),
            Probe::Unclear,
            "{status}"
        );
    }
    for body in [
        "{\"code\":1502,\"message\":\"Not permitted\"}",
        "{\"code\":1502,\"message\":\"not permitted: x\"}",
        "{\"code\":1502,\"message\":\"Rune is not valid\"}",
        "{\"code\":1,\"message\":\"Not permitted: x\"}",
        "{\"message\":\"Not permitted: x\"}",
    ] {
        assert_eq!(
            node::judge_probe(&reply(401, body)),
            Probe::Unclear,
            "{body}"
        );
    }
}

#[test]
fn ln_09_a_rune_from_another_node_or_a_revoked_one_is_never_a_passed_probe() {
    // Code 1501: the rune is not from this node (or the header is missing).
    for status in [401, 403] {
        let r = reply(
            status,
            "{\"code\":1501,\"message\":\"Not authorized: Not derived from master\",\"data\":null}",
        );
        assert_eq!(node::judge_probe(&r), Probe::NotFromNode, "{status}");
    }
    // Code 1503: the rune is blacklisted.
    for status in [401, 403] {
        let r = reply(
            status,
            "{\"code\":1503,\"message\":\"Not authorized: Blacklisted rune\",\"data\":null}",
        );
        assert_eq!(node::judge_probe(&r), Probe::Revoked, "{status}");
    }
}

#[test]
fn ln_09_a_call_that_works_means_the_rune_can_do_more() {
    assert_eq!(
        node::judge_probe(&reply(200, "{\"id\":\"02abc\",\"alias\":\"shop\"}")),
        Probe::Allowed
    );
    assert_eq!(
        node::judge_probe(&reply(200, "{\"outputs\":[],\"channels\":[]}")),
        Probe::Allowed
    );
}

#[test]
fn ln_09_any_other_failure_proves_nothing() {
    for r in [
        reply(404, "{\"message\":\"unknown method\"}"),
        reply(500, "{\"code\":-1,\"message\":\"lightningd is busy\"}"),
        reply(502, ""),
        reply(500, "{\"message\":\"Not authorizedx\"}x"),
    ] {
        assert_eq!(node::judge_probe(&r), Probe::Unclear, "{r:?}");
    }
}

#[test]
fn ln_09_the_probes_only_read() {
    let Credential::Cln(c) = cln() else {
        panic!("cln")
    };
    let probes = node::probes(&c);
    assert_eq!(probes[0].1.path, "/v1/getinfo");
    assert_eq!(probes[1].1.path, "/v1/listfunds");
    for (_, r) in &probes {
        assert_eq!(r.body.as_deref(), Some("{}"));
    }
}

fn listed(label: &str, msat: u64, status: &str, description: &str, hash: &[u8; 32]) -> Reply {
    reply(
        200,
        &format!(
            "{{\"invoices\":[{{\"label\":\"{label}\",\"description\":\"{description}\",\
             \"payment_hash\":\"{}\",\"status\":\"{status}\",\"expires_at\":1700000300,\
             \"amount_msat\":{msat},\"bolt11\":\"lnbcrt1abc\",\"created_index\":3}}]}}",
            hex(hash)
        ),
    )
}

fn item(fields: &str) -> Reply {
    reply(
        200,
        &format!(
            "{{\"invoices\":[{{\"payment_hash\":\"{}\",\"status\":\"paid\"{fields}}}]}}",
            hex(&hash(1))
        ),
    )
}

#[test]
fn ln_14_cln_lists_numbers_as_numbers_and_paid_at_only_for_a_paid_invoice() {
    let h = hash(1);
    // The shape of a listinvoices item of a paid invoice.
    let paid = reply(
        200,
        &format!(
            "{{\"invoices\":[{{\"label\":\"x\",\"description\":\"Sale 17\",\
             \"payment_hash\":\"{}\",\"status\":\"paid\",\"expires_at\":1700000300,\
             \"amount_msat\":25000000,\"bolt11\":\"lnbcrt1abc\",\"pay_index\":1,\
             \"amount_received_msat\":25000000,\"paid_at\":1700000123,\
             \"payment_preimage\":\"{}\"}}]}}",
            hex(&h),
            hex(&hash(9))
        ),
    );
    assert_eq!(
        cln().parse_lookup(&paid, &h, 25_000_000),
        Ok(Payment::Paid(PaidFacts {
            payment_hash: h,
            amount_received_msat: 25_000_000,
            settled_at: Some(1_700_000_123)
        }))
    );
    // A text, or a "msat" suffix, where a number is written is refused.
    for fields in [
        ",\"amount_msat\":25000000,\"amount_received_msat\":\"25000000\"",
        ",\"amount_msat\":25000000,\"amount_received_msat\":\"25000000msat\"",
        ",\"amount_msat\":\"25000000msat\",\"amount_received_msat\":25000000",
        ",\"amount_msat\":25000000,\"amount_received_msat\":25000000,\"paid_at\":\"1700000123\"",
    ] {
        assert!(
            cln().parse_lookup(&item(fields), &h, 25_000_000).is_err(),
            "{fields}"
        );
    }
    // An invoice that takes any amount has no amount_msat: refused.
    for status in ["unpaid", "paid", "expired"] {
        let body = format!(
            "{{\"invoices\":[{{\"payment_hash\":\"{}\",\"status\":\"{status}\",\
             \"amount_received_msat\":25000000}}]}}",
            hex(&h)
        );
        assert_eq!(
            cln().parse_lookup(&reply(200, &body), &h, 25_000_000),
            Err(ReplyRefused::Missing("amount_msat")),
            "{status}"
        );
    }
    // An unpaid invoice has no paid_at and is read.
    assert_eq!(
        cln().parse_lookup(
            &listed("x", 25_000_000, "unpaid", "Sale 17", &h),
            &h,
            25_000_000
        ),
        Ok(Payment::NotPaid(NotPaid::Waiting))
    );
}

#[test]
fn ln_15_error_900_has_the_text_the_node_gives() {
    let r = reply(
        500,
        "{\"code\":900,\"message\":\"Duplicate label 'obmt-c0ffee01-17-0'\",\"data\":null}",
    );
    assert_eq!(cln().parse_create(&r), Ok(CreateOutcome::LabelExists));
}

#[test]
fn ln_15_the_label_is_made_from_the_counter_the_sale_and_the_attempt() {
    let a = Ask::sale(COUNTER, 17, 0, 25_000_000, 300).unwrap();
    let again = Ask::sale(COUNTER, 17, 0, 25_000_000, 300).unwrap();
    let next = Ask::sale(COUNTER, 17, 1, 25_000_000, 300).unwrap();
    let other_sale = Ask::sale(COUNTER, 18, 0, 25_000_000, 300).unwrap();
    let other_counter = Ask::sale([1, 2, 3, 4], 17, 0, 25_000_000, 300).unwrap();
    assert_eq!(a.label(), "obmt-c0ffee01-17-0");
    assert_eq!(a.label(), again.label());
    for other in [&next, &other_sale, &other_counter] {
        assert_ne!(a.label(), other.label());
    }
    assert_eq!(next.label(), "obmt-c0ffee01-17-1");
}

#[test]
fn ln_15_error_900_is_a_label_that_is_used_already() {
    let used = reply(
        500,
        "{\"code\":900,\"message\":\"Duplicate label 'obmt-c0ffee01-17-0'\"}",
    );
    assert_eq!(
        cln().parse_create(&used).unwrap(),
        CreateOutcome::LabelExists
    );
    // Another error is not that.
    let other = reply(500, "{\"code\":901,\"message\":\"Preimage already used\"}");
    assert!(cln().parse_create(&other).is_err());
    // LND has no such error, so the same body is only an error there.
    assert!(lnd().parse_create(&used).is_err());
    assert!(lnd().label_request(&ask()).is_none());
    let r = cln().label_request(&ask()).unwrap();
    assert_eq!(r.path, "/v1/listinvoices");
    assert_eq!(
        body_of(&r).get("label").and_then(Value::as_str),
        Some("obmt-c0ffee01-17-0")
    );
}

#[test]
fn ln_15_an_invoice_is_taken_up_again_only_if_it_is_this_sales() {
    let ask = ask();
    let h = hash(1);
    let same = listed(ask.label(), 25_000_000, "unpaid", "Sale 17", &h);
    let created = cln().parse_existing(&same, &ask).unwrap();
    assert_eq!(created.payment_hash, h);
    assert_eq!(created.bolt11, "lnbcrt1abc");
    for (what, r) in [
        (
            "amount",
            listed(ask.label(), 25_000_001, "unpaid", "Sale 17", &h),
        ),
        (
            "label",
            listed("obmt-other", 25_000_000, "unpaid", "Sale 17", &h),
        ),
        (
            "description",
            listed(ask.label(), 25_000_000, "unpaid", "Sale 18", &h),
        ),
        (
            "paid",
            listed(ask.label(), 25_000_000, "paid", "Sale 17", &h),
        ),
        (
            "expired",
            listed(ask.label(), 25_000_000, "expired", "Sale 17", &h),
        ),
    ] {
        assert_eq!(
            cln().parse_existing(&r, &ask).unwrap_err(),
            ReplyRefused::LabelTaken,
            "{what}"
        );
    }
    let none = reply(200, "{\"invoices\":[]}");
    assert_eq!(
        cln().parse_existing(&none, &ask).unwrap_err(),
        ReplyRefused::LabelGone
    );
    let two = reply(
        200,
        &format!(
            "{{\"invoices\":[{},{}]}}",
            String::from_utf8(same.body.clone())
                .unwrap()
                .trim_start_matches("{\"invoices\":[")
                .trim_end_matches("]}"),
            "{}"
        ),
    );
    assert_eq!(
        cln().parse_existing(&two, &ask).unwrap_err(),
        ReplyRefused::Several
    );
    // The hash is the node's: the check of the invoice then proves it.
    let wrong_hash = {
        let mut r = Make::good(hash(2));
        r.msat = Some(25_000_000);
        Created {
            payment_hash: h,
            bolt11: r.write(),
        }
    };
    assert_eq!(
        crate::node::check_created(&wrong_hash, &ask, NETWORK, node_key(1)),
        Err(crate::InvoiceRefused::WrongPaymentHash)
    );
}

#[test]
fn ln_15_an_invoice_that_is_found_again_still_passes_the_check_of_ln_17() {
    let ask = ask();
    let h = hash(1);
    let bolt11 = Make::good(h).write();
    let r = reply(
        200,
        &format!(
            "{{\"invoices\":[{{\"label\":\"{}\",\"description\":\"Sale 17\",\
             \"payment_hash\":\"{}\",\"status\":\"unpaid\",\"amount_msat\":25000000,\
             \"bolt11\":\"{bolt11}\"}}]}}",
            ask.label(),
            hex(&h)
        ),
    );
    let created = cln().parse_existing(&r, &ask).unwrap();
    assert!(crate::node::check_created(&created, &ask, NETWORK, node_key(1)).is_ok());
}
