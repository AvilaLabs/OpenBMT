//! PS-97, PS-98: the Clover request and the reading of its reply.

use openbmt_pos::{
    Cursor, Id, OrderState, Page, PosOrder, PosTender, TenderKind, TenderState, MAX_PAGE_ORDERS,
};

use crate::clover::{percent_encode, Request, MAX_MERCHANT_BYTES};
use crate::{cursor_at, ReadRefused, RequestRefused, Shown};

use super::{settings, text, usd, MERCHANT};

const ORDERS: &[u8] = include_bytes!("../../tests/fixtures/clover_orders.json");
const EMPTY: &[u8] = include_bytes!("../../tests/fixtures/clover_orders_empty.json");

/// 2026-11-10T13:56:40Z, before every order of the fixture.
const SINCE: u64 = 1_794_319_000;

fn request(limit: usize) -> Request {
    Request::new(
        MERCHANT,
        &settings(),
        limit,
        &cursor_at(SINCE).expect("a cursor"),
    )
    .expect("a request")
}

fn id(s: &str) -> Id {
    Id::new(s).expect("an id")
}

fn order<'a>(page: &'a Page, i: &str) -> &'a PosOrder {
    page.orders
        .iter()
        .find(|o| o.id == id(i))
        .unwrap_or_else(|| panic!("no order {i}"))
}

fn tender<'a>(o: &'a PosOrder, i: &str) -> &'a PosTender {
    o.tender(&id(i)).unwrap_or_else(|| panic!("no tender {i}"))
}

#[test]
fn ps_97_the_query_asks_for_orders_modified_since_the_cursor_oldest_first() {
    let q = request(500).path_and_query();
    assert_eq!(
        q,
        "/v3/merchants/CLMERCH0001/orders?filter=modifiedTime%3E%3D1794319000000\
         &expand=payments%2Cpayments.tender%2Crefunds&limit=500&offset=0\
         &orderBy=modifiedTime%20ASC"
    );
    // The same text every time.
    assert_eq!(q, request(500).path_and_query());
}

#[test]
fn ps_97_the_offset_comes_from_the_cursor() {
    let c = Cursor::new(b"1794319000|1000".to_vec()).expect("a cursor");
    let q = Request::new(MERCHANT, &settings(), 250, &c)
        .expect("a request")
        .path_and_query();
    assert!(q.contains("&limit=250&offset=1000&"), "{q}");
    assert!(q.contains("filter=modifiedTime%3E%3D1794319000000&"), "{q}");
    let start = Request::new(MERCHANT, &settings(), 1, &Cursor::start()).expect("a request");
    assert!(start
        .path_and_query()
        .contains("filter=modifiedTime%3E%3D0&"));
}

#[test]
fn ps_97_everything_but_the_unreserved_characters_is_percent_encoded() {
    assert_eq!(percent_encode("AZaz09-._~"), "AZaz09-._~");
    assert_eq!(percent_encode("a b"), "a%20b");
    assert_eq!(percent_encode(">=,"), "%3E%3D%2C");
    assert_eq!(percent_encode("%"), "%25");
    assert_eq!(percent_encode("\u{e9}"), "%C3%A9");
    assert_eq!(percent_encode("&=?#/+"), "%26%3D%3F%23%2F%2B");
    assert_eq!(percent_encode(""), "");
    // Every byte comes back from its encoding, and the text has only the
    // unreserved characters, the percent sign and capital hex digits.
    let all: String = (0u8..=127).map(char::from).collect();
    let out = percent_encode(&all);
    assert!(out.bytes().all(|b| b.is_ascii_digit()
        || b.is_ascii_uppercase()
        || b"-._~%".contains(&b)
        || b.is_ascii_lowercase()));
    let mut back = Vec::new();
    let bytes = out.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).expect("ascii");
            assert!(!hex.bytes().any(|b| b.is_ascii_lowercase()), "{hex}");
            back.push(u8::from_str_radix(hex, 16).expect("hex"));
            i += 3;
        } else {
            back.push(bytes[i]);
            i += 1;
        }
    }
    assert_eq!(back, all.as_bytes());
}

#[test]
fn ps_97_a_request_needs_a_merchant_a_page_size_and_a_cursor_in_form() {
    let start = Cursor::start();
    for bad in [
        "",
        "a/b",
        "a b",
        "a?b",
        "a%2Fb",
        "..",
        "caf\u{e9}",
        "a\nb",
        &"A".repeat(MAX_MERCHANT_BYTES + 1),
    ] {
        assert_eq!(
            Request::new(bad, &settings(), 10, &start),
            Err(RequestRefused::BadMerchant),
            "{bad:?}"
        );
    }
    assert!(Request::new(&"A".repeat(MAX_MERCHANT_BYTES), &settings(), 10, &start).is_ok());
    for limit in [0, MAX_PAGE_ORDERS + 1] {
        assert_eq!(
            Request::new(MERCHANT, &settings(), limit, &start),
            Err(RequestRefused::BadLimit)
        );
    }
    for bad in [&b"5|"[..], b"5|x", b"5|01", b"5|-1", b"x", b"5|1 "] {
        let c = Cursor::new(bad.to_vec()).expect("short");
        assert_eq!(
            Request::new(MERCHANT, &settings(), 10, &c),
            Err(RequestRefused::BadCursor),
            "{bad:?}"
        );
    }
}

#[test]
fn ps_98_the_fixture_is_read_into_the_model() {
    let page = request(500).parse(ORDERS).expect("reads");
    assert_eq!(page.orders.len(), 3);
    assert!(!page.more);
    // The last page: the next poll starts at the newest modification.
    assert_eq!(page.next.as_bytes(), b"1794320460");

    let a = order(&page, "CLORDER0001");
    assert_eq!(
        *a,
        PosOrder {
            id: id("CLORDER0001"),
            location: text(MERCHANT),
            state: OrderState::Closed,
            currency: usd(),
            total_minor: 1250,
            tenders: vec![
                PosTender {
                    id: id("CLPAY00001"),
                    kind: TenderKind::Other,
                    name: text("Cash"),
                    amount_minor: 150,
                    tip_minor: 0,
                    state: TenderState::Paid,
                    at: 1_794_319_420,
                    reference: None,
                },
                PosTender {
                    id: id("CLPAY00002"),
                    kind: TenderKind::Other,
                    name: text("Bitcoin"),
                    amount_minor: 1100,
                    tip_minor: 0,
                    state: TenderState::Paid,
                    at: 1_794_319_421,
                    reference: Some(text("line break K7M2QX")),
                },
            ],
            opened_at: 1_794_319_205,
            closed_at: None,
            updated_at: 1_794_319_421,
            reference: Some(text("btc K7M2QX")),
        }
    );

    let b = order(&page, "CLORDER0002");
    assert_eq!(b.state, OrderState::Closed);
    let t = tender(b, "CLPAY00004");
    assert_eq!(t.name.as_str(), " bitcoin ");
    assert_eq!((t.amount_minor, t.tip_minor), (1900, 100));
    assert_eq!(t.state, TenderState::Refunded { amount_minor: 500 });
    assert_eq!(b.reference, None);

    let c = order(&page, "CLORDER0003");
    assert_eq!(c.state, OrderState::Open);
    assert_eq!(tender(c, "CLPAY00005").state, TenderState::Voided);
    assert_eq!((c.opened_at, c.updated_at), (1_794_320_400, 1_794_320_460));
    for o in &page.orders {
        assert_eq!(o.validate(), Ok(()));
    }
}

#[test]
fn ps_98_which_tenders_are_bitcoin_is_the_settings_name_against_the_label() {
    let s = settings();
    let page = request(500).parse(ORDERS).expect("reads");
    assert!(s.is_bitcoin(tender(order(&page, "CLORDER0001"), "CLPAY00002")));
    assert!(!s.is_bitcoin(tender(order(&page, "CLORDER0001"), "CLPAY00001")));
    // Without regard to case and surrounding spaces.
    assert!(s.is_bitcoin(tender(order(&page, "CLORDER0002"), "CLPAY00004")));
}

#[test]
fn ps_98_a_payment_that_is_not_money_taken_and_not_a_void_is_left_out() {
    // CLPAY00003 of the first order is PENDING.
    let page = request(500).parse(ORDERS).expect("reads");
    assert!(order(&page, "CLORDER0001")
        .tender(&id("CLPAY00003"))
        .is_none());
    let one = |result: &str| {
        let body = format!(
            r#"{{"elements":[{{"id":"C1","currency":"USD","total":100,"paymentState":"PAID",
            "createdTime":1794319200000,"modifiedTime":1794319260000,
            "payments":{{"elements":[{{"id":"P1","tender":{{"label":"Bitcoin"}},"amount":100,
            "result":"{result}","createdTime":1794319230000}}]}}}}]}}"#
        );
        request(500)
            .parse(body.as_bytes())
            .map(|p| p.orders[0].tenders.first().map(|t| t.state))
    };
    assert_eq!(one("SUCCESS"), Ok(Some(TenderState::Paid)));
    assert_eq!(one("VOIDED"), Ok(Some(TenderState::Voided)));
    for left_out in [
        "FAIL",
        "INITIATED",
        "VOIDING",
        "VOID_FAILED",
        "AUTH",
        "AUTH_COMPLETED",
        "DISCOUNT",
        "OFFLINE_RETRYING",
        "PENDING",
    ] {
        assert_eq!(one(left_out), Ok(None), "{left_out}");
    }
    for unknown in ["", "success", "SETTLED", "REFUNDED"] {
        assert_eq!(
            one(unknown),
            Err(ReadRefused::UnknownResult(Shown::new(unknown))),
            "{unknown}"
        );
    }
}

#[test]
fn ps_91_an_unknown_result_on_a_cash_payment_is_left_out_and_on_a_bitcoin_payment_refuses() {
    let order = |label: &str, result: &str, refund: bool| {
        let refunds = if refund {
            r#","refunds":[{"payment":{"id":"P1"},"amount":5}]"#
        } else {
            ""
        };
        let body = format!(
            r#"{{"elements":[{{"id":"C1","currency":"USD","total":100,"paymentState":"PAID",
            "createdTime":1794319200000,"modifiedTime":1794319260000,
            "payments":[{{"id":"P1","tender":{{"label":"{label}"}},"amount":100,
            "result":"{result}","createdTime":1794319230000}},
            {{"id":"P2","tender":{{"label":"Bitcoin"}},"amount":100,
            "result":"SUCCESS","createdTime":1794319231000}}]{refunds}}}]}}"#
        );
        request(500).parse(body.as_bytes())
    };
    // Cash with a result the reader does not know: left out; the Bitcoin
    // payment beside it is still read; a refund of the cash one is ignored.
    for refund in [false, true] {
        let page = order("Cash", "SETTLED", refund).expect("reads");
        let tenders = &page.orders[0].tenders;
        assert_eq!(tenders.len(), 1);
        assert_eq!(tenders[0].id, id("P2"));
    }
    // The same result on a Bitcoin payment, whatever its spelling of the
    // name, refuses and names the result.
    for label in ["Bitcoin", " BITCOIN "] {
        assert_eq!(
            order(label, "SETTLED", false),
            Err(ReadRefused::UnknownResult(Shown::new("SETTLED"))),
            "{label}"
        );
    }
    let Err(e) = order("Bitcoin", "SETTLED", false) else {
        panic!("refused")
    };
    assert_eq!(e.rule(), "PS-91");
    assert!(e.to_string().contains("SETTLED"));
    let w = e.words();
    assert_eq!(w.quoted.map(|q| q.text), Some("SETTLED".to_string()));
    assert!(!w.what.contains("SETTLED"));
}

#[test]
fn ps_98_a_page_shorter_than_the_limit_is_the_last_and_a_full_one_has_more() {
    // Three orders, three asked for: more, from the offset after them.
    let page = request(3).parse(ORDERS).expect("reads");
    assert!(page.more);
    assert_eq!(page.next.as_bytes(), b"1794319000|3");
    let r2 = Request::new(MERCHANT, &settings(), 3, &page.next).expect("a request");
    assert!(r2.path_and_query().contains("&limit=3&offset=3&"));
    // The next page adds its length to the offset.
    let again = r2.parse(ORDERS).expect("reads");
    assert_eq!(again.next.as_bytes(), b"1794319000|6");
    // Shorter than the limit: the last. The place moves to the newest time.
    let last = request(4).parse(ORDERS).expect("reads");
    assert!(!last.more);
    assert_eq!(last.next.as_bytes(), b"1794320460");
    // Nothing: the place stays.
    let none = request(500).parse(EMPTY).expect("reads");
    assert!(none.orders.is_empty() && !none.more);
    assert_eq!(none.next.as_bytes(), b"1794319000");
}

#[test]
fn ps_98_more_than_the_limit_is_refused() {
    assert_eq!(request(2).parse(ORDERS), Err(ReadRefused::TooManyOrders));
}

#[test]
fn ps_98_an_expanded_list_may_also_be_a_plain_array() {
    let body = br#"{"elements":[{"id":"C1","currency":"USD","total":100,"paymentState":"PAID",
        "createdTime":1794319200000,"modifiedTime":1794319260000,
        "payments":[{"id":"P1","tender":{"label":"Bitcoin"},"amount":100,
        "result":"SUCCESS","createdTime":1794319230000}],
        "refunds":[]}]}"#;
    let page = request(500).parse(body).expect("reads");
    assert_eq!(page.orders[0].tenders.len(), 1);
}

#[test]
fn ps_98_an_order_without_payments_has_no_tenders() {
    let body = br#"{"elements":[{"id":"C1","currency":"EUR","total":100,"paymentState":"OPEN",
        "createdTime":1794319200000,"modifiedTime":1794319260000}]}"#;
    let page = request(500).parse(body).expect("reads");
    assert!(page.orders[0].tenders.is_empty());
    assert_eq!(page.orders[0].currency.to_string(), "EUR");
}

#[test]
fn ps_98_every_payment_state_of_an_order_is_read_and_another_is_refused() {
    let state_of = |s: &str| {
        let body = format!(
            r#"{{"elements":[{{"id":"C1","currency":"USD","total":100,"paymentState":"{s}",
            "createdTime":1794319200000,"modifiedTime":1794319260000}}]}}"#
        );
        request(500)
            .parse(body.as_bytes())
            .map(|p| p.orders[0].state)
    };
    for (s, want) in [
        ("OPEN", OrderState::Open),
        ("PARTIALLY_PAID", OrderState::Open),
        ("PAID", OrderState::Closed),
        ("PARTIALLY_REFUNDED", OrderState::Closed),
        ("REFUNDED", OrderState::Closed),
        ("CREDITED", OrderState::Closed),
    ] {
        assert_eq!(state_of(s), Ok(want), "{s}");
    }
}

#[test]
fn ps_91_an_unknown_payment_state_leaves_out_an_order_with_no_bitcoin_tender() {
    let with = |state: &str, label: &str| {
        let body = format!(
            r#"{{"elements":[{{"id":"C1","currency":"USD","total":100,"paymentState":"{state}",
            "createdTime":1794319200000,"modifiedTime":1794319260000,
            "payments":[{{"id":"P1","tender":{{"label":"{label}"}},"amount":100,
            "result":"SUCCESS","createdTime":1794319230000}}]}}]}}"#
        );
        request(500).parse(body.as_bytes())
    };
    for bad in ["", "paid", "VOIDED", "DELETED"] {
        // A cash order: left out, and the page goes on.
        let page = with(bad, "Cash").expect("reads");
        assert!(page.orders.is_empty(), "{bad}");
        assert!(!page.more);
        // An order with a Bitcoin tender: refused.
        assert_eq!(
            with(bad, "Bitcoin"),
            Err(ReadRefused::UnknownValue("elements[].paymentState")),
            "{bad}"
        );
    }
    assert_eq!(with("PAID", "Cash").expect("reads").orders.len(), 1);
}

#[test]
fn ps_98_a_label_longer_than_64_bytes_is_cut_and_the_tender_is_still_read() {
    let label = "L".repeat(70);
    let body = format!(
        r#"{{"elements":[{{"id":"C1","currency":"USD","total":100,"paymentState":"PAID",
        "createdTime":1794319200000,"modifiedTime":1794319260000,
        "payments":[{{"id":"P1","tender":{{"label":"{label}"}},"amount":100,
        "result":"SUCCESS","createdTime":1794319230000}}]}}]}}"#
    );
    let page = request(500).parse(body.as_bytes()).expect("reads");
    assert_eq!(page.orders[0].tenders[0].name.as_str(), "L".repeat(64));
}

#[test]
fn ps_98_refunds_add_up_on_their_payment() {
    let with = |refunds: &str| {
        let body = format!(
            r#"{{"elements":[{{"id":"C1","currency":"USD","total":1000,"paymentState":"PARTIALLY_REFUNDED",
            "createdTime":1794319200000,"modifiedTime":1794319260000,
            "payments":[
            {{"id":"P1","tender":{{"label":"Bitcoin"}},"amount":1000,"result":"SUCCESS","createdTime":1794319230000}},
            {{"id":"P2","tender":{{"label":"Bitcoin"}},"amount":50,"result":"VOIDED","createdTime":1794319231000}}],
            "refunds":{refunds}}}]}}"#
        );
        request(500).parse(body.as_bytes())
    };
    let state = |r: &str, t: usize| with(r).map(|p| p.orders[0].tenders[t].state);
    assert_eq!(
        state(
            r#"{"elements":[{"payment":{"id":"P1"},"amount":300},{"payment":{"id":"P1"},"amount":200}]}"#,
            0
        ),
        Ok(TenderState::Refunded { amount_minor: 500 })
    );
    // A refund on a voided payment leaves it voided.
    assert_eq!(
        state(r#"[{"payment":{"id":"P2"},"amount":10}]"#, 1),
        Ok(TenderState::Voided)
    );
    assert_eq!(
        state(r#"[{"payment":{"id":"P1"},"amount":1001}]"#, 0),
        Err(ReadRefused::RefundTooLarge)
    );
    assert_eq!(
        state(r#"[{"payment":{"id":"P9"},"amount":1}]"#, 0),
        Err(ReadRefused::RefundWithoutTender)
    );
    assert_eq!(
        state(r#"[{"payment":{"id":"P1"},"amount":0}]"#, 0),
        Err(ReadRefused::OutOfRange("refunds[].amount"))
    );
    assert_eq!(
        state(r#"[{"amount":5}]"#, 0),
        Err(ReadRefused::Missing("refunds[].payment"))
    );
    assert_eq!(
        state(r#"[{"payment":{"id":"P1"}}]"#, 0),
        Err(ReadRefused::Missing("refunds[].amount"))
    );
    assert_eq!(
        state(r#"[{"payment":"P1","amount":5}]"#, 0),
        Err(ReadRefused::WrongType("refunds[].payment"))
    );
    assert_eq!(
        state(r#"7"#, 0),
        Err(ReadRefused::WrongType("elements[].refunds"))
    );
}
