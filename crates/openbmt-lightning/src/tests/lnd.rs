//! LN-01, LN-06, LN-07 for LND.

use openbmt_json::{parse, Limits};

use super::*;
use crate::connect::Probe;
use crate::error::ReplyRefused;
use crate::request::Method;
use crate::{lnd as node, CreateOutcome};

#[test]
fn ln_01_both_kinds_of_node_have_a_credential_and_their_own_header() {
    let ask = ask();
    let a = lnd().create_request(&ask);
    let b = cln().create_request(&ask);
    assert_eq!(a.header_name, "Grpc-Metadata-macaroon");
    assert_eq!(b.header_name, "Rune");
    // The kinds are matched without a catch-all arm, so a third kind cannot
    // be added without this test being written again.
    for c in [lnd(), cln()] {
        match c {
            Credential::Lnd(_) | Credential::Cln(_) => {}
        }
        assert_eq!(c.endpoint().host_port(), "node.local:8080");
    }
}

#[test]
fn ln_07_an_invoice_is_made_with_post_invoices_and_the_macaroon_in_hex() {
    let r = lnd().create_request(&ask());
    assert_eq!(r.method, Method::Post);
    assert_eq!(r.path, "/v1/invoices");
    assert_eq!(r.header_value, hex(&macaroon()));
    let body = parse(r.body.as_deref().unwrap().as_bytes(), &Limits::DEFAULT).unwrap();
    assert_eq!(body.get("memo").and_then(|v| v.as_str()), Some("Sale 17"));
    // 64-bit integers are texts in REST.
    assert_eq!(
        body.get("value_msat").and_then(|v| v.as_str()),
        Some("25000000")
    );
    assert_eq!(body.get("expiry").and_then(|v| v.as_str()), Some("300"));
    assert_eq!(body.as_object().map(<[_]>::len), Some(3));
}

#[test]
fn ln_07_the_reply_to_an_invoice_gives_the_hash_and_the_invoice() {
    let h = hash(1);
    let made = lnd().parse_create(&lnd_created(&h, "lnbcrt1abc")).unwrap();
    assert_eq!(
        made,
        CreateOutcome::Created(Created {
            payment_hash: h,
            bolt11: "lnbcrt1abc".to_string()
        })
    );
    // The hash may come in the URL alphabet of base64, with no padding.
    let url = reply(
        200,
        &format!(
            "{{\"r_hash\":\"{}\",\"payment_request\":\"x\"}}",
            base64(&h, Alphabet::Url).trim_end_matches('=')
        ),
    );
    assert!(lnd().parse_create(&url).is_ok());
    let bad = |body: &str| lnd().parse_create(&reply(200, body)).unwrap_err();
    assert_eq!(
        bad("{\"payment_request\":\"x\"}"),
        ReplyRefused::Missing("r_hash")
    );
    assert_eq!(
        bad("{\"r_hash\":\"AAAA\",\"payment_request\":\"x\"}"),
        ReplyRefused::OutOfRange("r_hash")
    );
    assert_eq!(
        bad(&format!(
            "{{\"r_hash\":\"{}\"}}",
            base64(&h, Alphabet::Standard)
        )),
        ReplyRefused::Missing("payment_request")
    );
    assert_eq!(
        bad(&format!(
            "{{\"r_hash\":\"{}\",\"payment_request\":5}}",
            base64(&h, Alphabet::Standard)
        )),
        ReplyRefused::WrongType("payment_request")
    );
    assert_eq!(bad("not json"), ReplyRefused::Unreadable);
    assert_eq!(bad("[]"), ReplyRefused::Missing("r_hash"));
}

#[test]
fn ln_07_a_node_error_is_a_refusal_with_its_own_text() {
    let e = lnd()
        .parse_create(&reply(
            500,
            "{\"code\":2,\"message\":\"wallet locked\\n\\u202e\"}",
        ))
        .unwrap_err();
    assert_eq!(
        e,
        ReplyRefused::NodeSaid {
            status: 500,
            message: "wallet locked".to_string()
        }
    );
    // A body that is not JSON still gives the status.
    let e = lnd().parse_create(&reply(502, "<html>")).unwrap_err();
    assert_eq!(
        e,
        ReplyRefused::NodeSaid {
            status: 502,
            message: String::new()
        }
    );
}

#[test]
fn ln_07_the_lookup_is_the_first_version_by_hash_in_hex() {
    let h = hash(1);
    let r = lnd().lookup_request(&h);
    assert_eq!(r.method, Method::Get);
    assert_eq!(r.path, format!("/v1/invoice/{}", hex(&h)));
    assert!(!r.path.contains("v2"));
    assert_eq!(r.body, None);
    assert_eq!(r.header_name, "Grpc-Metadata-macaroon");
}

#[test]
fn ln_06_a_macaroon_that_is_refused_with_permission_denied_is_limited() {
    // The exact answer of LND: HTTP 500, code 2 (Unknown), the message of
    // the macaroon checker.
    let r = reply(
        500,
        "{\"code\":2,\"message\":\"permission denied\",\"details\":[]}",
    );
    assert_eq!(node::judge_probe(&r), Probe::Denied);
    // Without the empty details the code and the message still make it.
    let r = reply(500, "{\"code\":2,\"message\":\"permission denied\"}");
    assert_eq!(node::judge_probe(&r), Probe::Denied);
}

#[test]
fn ln_06_only_the_exact_answer_is_a_denial() {
    let body = "{\"code\":2,\"message\":\"permission denied\",\"details\":[]}";
    // The same body with another status is not the answer LND gives.
    for status in [400, 401, 403, 404, 502] {
        assert_eq!(
            node::judge_probe(&reply(status, body)),
            Probe::Unclear,
            "{status}"
        );
    }
    // Another code, or another text, is not either.
    for body in [
        "{\"code\":7,\"message\":\"permission denied\",\"details\":[]}",
        "{\"code\":2,\"message\":\"permission denied: more\"}",
        "{\"code\":2,\"message\":\"Permission denied\"}",
        "{\"message\":\"permission denied\"}",
        "{\"code\":\"2\",\"message\":\"permission denied\"}",
    ] {
        assert_eq!(
            node::judge_probe(&reply(500, body)),
            Probe::Unclear,
            "{body}"
        );
    }
}

#[test]
fn ln_06_a_call_that_works_means_the_macaroon_can_do_more() {
    let info = reply(200, "{\"identity_pubkey\":\"02abc\",\"alias\":\"shop\"}");
    assert_eq!(node::judge_probe(&info), Probe::Allowed);
    let balance = reply(200, "{\"total_balance\":\"0\"}");
    assert_eq!(node::judge_probe(&balance), Probe::Allowed);
}

#[test]
fn ln_06_any_other_failure_proves_nothing() {
    for r in [
        reply(404, "{\"code\":5,\"message\":\"not found\"}"),
        reply(
            500,
            "{\"code\":2,\"message\":\"verification failed: signature mismatch\"}",
        ),
        reply(500, "{\"code\":2,\"message\":\"the permission denied\"}"),
        reply(403, "{\"code\":2,\"message\":\"permission denied\"}"),
        reply(503, ""),
        reply(500, "not json"),
        reply(500, "{\"message\":5}"),
    ] {
        assert_eq!(node::judge_probe(&r), Probe::Unclear, "{r:?}");
    }
}

#[test]
fn ln_06_the_two_probes_are_the_read_only_calls_of_the_brief() {
    let Credential::Lnd(c) = lnd() else {
        panic!("lnd")
    };
    let probes = node::probes(&c);
    assert_eq!(probes[0].0, "GET /v1/getinfo");
    assert_eq!(probes[0].1.path, "/v1/getinfo");
    assert_eq!(probes[1].0, "GET /v1/balance/blockchain");
    assert_eq!(probes[1].1.path, "/v1/balance/blockchain");
    for (_, r) in &probes {
        assert_eq!(r.method, Method::Get);
        assert_eq!(r.body, None);
    }
}
