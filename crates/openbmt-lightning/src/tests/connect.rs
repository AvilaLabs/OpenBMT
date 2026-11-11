//! LN-05, LN-06, LN-09, LN-20 and LN-04 as the setup shows them.

use super::*;
use crate::connect::{Connected, ConnectionTest, Next};
use crate::error::{ConnectionRefused as C, InvoiceRefused, ReplyRefused};
use crate::request::Method;

const TEST_ID: u64 = 5;
const TEST_HASH_SEED: u8 = 3;

fn test_invoice(key: u8, msat: u64) -> String {
    let mut m = Make::good(hash(TEST_HASH_SEED));
    m.msat = Some(msat);
    m.description = Some("OpenBMT test".to_string());
    m.key = secret(key);
    m.write()
}

fn denied_lnd() -> Reply {
    reply(
        500,
        "{\"code\":2,\"message\":\"permission denied\",\"details\":[]}",
    )
}

fn denied_cln() -> Reply {
    reply(
        401,
        "{\"code\":1502,\"message\":\"Not permitted: method is not equal to getinfo\",\"data\":null}",
    )
}

fn created(c: &Credential, bolt11: &str) -> Reply {
    match c {
        Credential::Lnd(_) => lnd_created(&hash(TEST_HASH_SEED), bolt11),
        Credential::Cln(_) => cln_created(&hash(TEST_HASH_SEED), bolt11),
    }
}

fn start(c: &Credential) -> ConnectionTest {
    ConnectionTest::new(c.clone(), NETWORK, node_key(1), COUNTER, TEST_ID)
}

/// Feed the replies in order; the result is what the last one gives.
fn run(c: &Credential, replies: &[Reply]) -> Result<Connected, ConnectionRefused> {
    let mut test = start(c);
    for r in replies {
        match test.accept(r)? {
            Next::More(next) => test = next,
            Next::Done(done) => return Ok(done),
        }
    }
    panic!("the test did not finish");
}

type ConnectionRefused = crate::ConnectionRefused;

#[test]
fn ln_20_a_good_connection_passes_two_probes_and_a_test_invoice() {
    for (c, denied) in [(lnd(), denied_lnd()), (cln(), denied_cln())] {
        let invoice = test_invoice(1, 1_000);
        let done = run(&c, &[denied.clone(), denied, created(&c, &invoice)]).expect("good");
        assert_eq!(done.test_invoice.amount_msat, 1_000);
        assert_eq!(done.test_invoice.payment_hash, hash(TEST_HASH_SEED));
        assert_eq!(done.test_invoice.bolt11, invoice);
    }
}

#[test]
fn ln_20_the_requests_come_in_order_and_the_last_is_a_test_invoice_of_one_satoshi() {
    for c in [lnd(), cln()] {
        let denied = match c {
            Credential::Lnd(_) => denied_lnd(),
            Credential::Cln(_) => denied_cln(),
        };
        let t = start(&c);
        let first = t.request();
        let Next::More(t) = t.accept(&denied).unwrap() else {
            panic!("more")
        };
        let second = t.request();
        let Next::More(t) = t.accept(&denied).unwrap() else {
            panic!("more")
        };
        let third = t.request();
        assert_ne!(first.path, second.path);
        assert_ne!(second.path, third.path);
        // Only the third makes an invoice, and it is for 1 satoshi that ends
        // by itself.
        let body = third.body.as_deref().unwrap();
        assert!(body.contains("1000") && body.contains("300"), "{body}");
        assert!(body.contains("OpenBMT test"));
        for r in [&first, &second] {
            assert!(!r.path.contains("invoice"), "{}", r.path);
        }
        // The credential is in the header of each.
        for r in [&first, &second, &third] {
            assert!(!r.header_value.is_empty());
        }
    }
}

#[test]
fn ln_06_the_stock_invoice_macaroon_is_caught_by_the_balance_probe() {
    // The stock invoice.macaroon may not read the node's information, but it
    // has onchain:read, so the second probe works.
    let balance = reply(200, "{\"total_balance\":\"0\",\"confirmed_balance\":\"0\"}");
    let e = run(&lnd(), &[denied_lnd(), balance]).unwrap_err();
    assert_eq!(
        e,
        C::CredentialTooStrong {
            call: "GET /v1/balance/blockchain",
            rule: "LN-06"
        }
    );
    // An admin macaroon is caught by the first.
    let info = reply(200, "{\"identity_pubkey\":\"02abc\"}");
    assert_eq!(
        run(&lnd(), &[info]).unwrap_err(),
        C::CredentialTooStrong {
            call: "GET /v1/getinfo",
            rule: "LN-06"
        }
    );
}

#[test]
fn ln_06_a_probe_that_proves_nothing_refuses_the_setup() {
    let e = run(&lnd(), &[reply(404, "{\"message\":\"not found\"}")]).unwrap_err();
    assert_eq!(
        e,
        C::ProbeUnclear {
            call: "GET /v1/getinfo",
            status: 404,
            rule: "LN-06"
        }
    );
    let e = run(&lnd(), &[denied_lnd(), reply(503, "")]).unwrap_err();
    assert!(matches!(e, C::ProbeUnclear { status: 503, .. }));
    // A macaroon the node does not accept at all is no proof either.
    let bad = reply(
        500,
        "{\"message\":\"verification failed: signature mismatch\"}",
    );
    assert!(matches!(
        run(&lnd(), &[bad]).unwrap_err(),
        C::ProbeUnclear { .. }
    ));
}

#[test]
fn ln_09_a_rune_that_can_read_the_node_or_its_funds_refuses_the_setup() {
    let info = reply(200, "{\"id\":\"02abc\",\"alias\":\"shop\"}");
    assert_eq!(
        run(&cln(), &[info]).unwrap_err(),
        C::CredentialTooStrong {
            call: "POST /v1/getinfo",
            rule: "LN-09"
        }
    );
    let funds = reply(200, "{\"outputs\":[],\"channels\":[]}");
    assert_eq!(
        run(&cln(), &[denied_cln(), funds]).unwrap_err(),
        C::CredentialTooStrong {
            call: "POST /v1/listfunds",
            rule: "LN-09"
        }
    );
    let e = run(&cln(), &[reply(500, "{\"message\":\"busy\"}")]).unwrap_err();
    assert!(matches!(e, C::ProbeUnclear { rule: "LN-09", .. }));
}

#[test]
fn ln_09_a_rune_of_another_node_or_a_revoked_one_has_its_own_refusal() {
    let other = reply(
        401,
        "{\"code\":1501,\"message\":\"Not authorized: Not derived from master\",\"data\":null}",
    );
    let revoked = reply(
        401,
        "{\"code\":1503,\"message\":\"Not authorized: Blacklisted rune\",\"data\":null}",
    );
    for (r, want) in [(other, C::RuneNotFromNode), (revoked, C::RuneRevoked)] {
        // On either probe.
        assert_eq!(run(&cln(), std::slice::from_ref(&r)).unwrap_err(), want);
        assert_eq!(run(&cln(), &[denied_cln(), r]).unwrap_err(), want);
        assert_eq!(want.rule(), "LN-09");
    }
    let (a, b) = (C::RuneNotFromNode.words(), C::RuneRevoked.words());
    assert_eq!(a.heading, "Rune not from this node");
    assert_eq!(b.heading, "Rune revoked on the node");
}

#[test]
fn ln_09_nothing_the_setup_calls_moves_money() {
    for c in [lnd(), cln()] {
        let denied = match c {
            Credential::Lnd(_) => denied_lnd(),
            Credential::Cln(_) => denied_cln(),
        };
        let mut t = start(&c);
        let mut seen = Vec::new();
        for _ in 0..2 {
            let r = t.request();
            seen.push((r.method, r.path.clone()));
            let Next::More(next) = t.accept(&denied).unwrap() else {
                panic!("more")
            };
            t = next;
        }
        seen.push((t.request().method, t.request().path));
        for (_, path) in &seen {
            for money in [
                "pay",
                "send",
                "withdraw",
                "fundchannel",
                "close",
                "channel",
                "keysend",
            ] {
                assert!(!path.contains(money), "{path}");
            }
        }
        // The probes of LND only read.
        if matches!(c, Credential::Lnd(_)) {
            assert_eq!(seen[0].0, Method::Get);
            assert_eq!(seen[1].0, Method::Get);
        }
    }
}

#[test]
fn ln_20_a_failure_of_the_test_invoice_says_which() {
    for c in [lnd(), cln()] {
        let denied = match c {
            Credential::Lnd(_) => denied_lnd(),
            Credential::Cln(_) => denied_cln(),
        };
        let probes = [denied.clone(), denied];
        let go = |last: Reply| {
            let mut all = probes.to_vec();
            all.push(last);
            run(&c, &all).unwrap_err()
        };
        // The node made no invoice.
        let e = go(reply(500, "{\"code\":2,\"message\":\"wallet locked\"}"));
        assert!(matches!(
            e,
            C::NoInvoice(ReplyRefused::NodeSaid { status: 500, .. })
        ));
        // The invoice is signed by another node: the settings name the first.
        assert_eq!(go(created(&c, &test_invoice(2, 1_000))), C::WrongNode);
        // The invoice is for another amount.
        assert_eq!(
            go(created(&c, &test_invoice(1, 2_000))),
            C::BadInvoice(InvoiceRefused::WrongAmount {
                asked: 1_000,
                got: 2_000
            })
        );
        // Not an invoice.
        assert_eq!(
            go(created(&c, "lnbcrt1nothing")),
            C::BadInvoice(InvoiceRefused::NotAnInvoice)
        );
        // Another network.
        let mut m = Make::good(hash(TEST_HASH_SEED));
        m.msat = Some(1_000);
        m.description = Some("OpenBMT test".to_string());
        m.network = "bc";
        assert_eq!(
            go(created(&c, &m.write())),
            C::BadInvoice(InvoiceRefused::WrongNetwork)
        );
    }
}

#[test]
fn ln_20_a_label_that_is_used_is_told_apart() {
    let used = reply(500, "{\"code\":900,\"message\":\"Duplicate label\"}");
    let e = run(&cln(), &[denied_cln(), denied_cln(), used]).unwrap_err();
    assert_eq!(e, C::LabelUsed);
}

#[test]
fn ln_20_the_test_invoice_has_a_label_of_its_own() {
    let a = Ask::test(COUNTER, 5);
    let b = Ask::test(COUNTER, 6);
    assert_eq!(a.label(), "obmt-c0ffee01-test-5");
    assert_ne!(a.label(), b.label());
    // No sale's label is the label of a test.
    assert_ne!(a.label(), ask().label());
    assert_eq!(a.amount_msat(), 1_000);
    assert_eq!(a.expiry_secs(), 300);
    assert_eq!(a.description(), "OpenBMT test");
}

#[test]
fn ln_04_a_limited_key_is_not_trusted_for_being_limited_on_its_face() {
    // A rune that reads as limited to invoices still has to be refused the
    // calls it must not do: the node, not the text of the rune, enforces it.
    assert!(matches!(cln(), Credential::Cln(_)));
    let funds = reply(200, "{\"outputs\":[]}");
    assert!(run(&cln(), &[denied_cln(), funds]).is_err());
    // And the words tell the owner to update the node when a probe is unclear.
    let w = C::ProbeUnclear {
        call: "GET /v1/getinfo",
        status: 404,
        rule: "LN-06",
    }
    .words();
    assert!(w.next.contains("Update the node"));
    // The rules name the releases that matter.
    let rules = include_str!("../../../../docs/LIGHTNING-RULES.md");
    for text in ["26.06.10", "v0.21.6-beta", "runes and commando"] {
        assert!(rules.contains(text), "{text}");
    }
}
