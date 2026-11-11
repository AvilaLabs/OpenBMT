//! PS-90 to PS-94, PS-99: what both readers refuse, and that they refuse
//! the same way. Each case makes one change to a good reply.

use openbmt_pos::{Cursor, Page};

use crate::{clover, cursor_at, square, ReadRefused};

use super::{settings, MERCHANT};

fn sq(body: &str) -> Result<Page, ReadRefused> {
    sq_bytes(body.as_bytes())
}

fn sq_bytes(body: &[u8]) -> Result<Page, ReadRefused> {
    square::Request::new(&settings(), 500, &Cursor::start())
        .expect("a request")
        .parse(body, None)
}

fn cl(body: &str) -> Result<Page, ReadRefused> {
    cl_bytes(body.as_bytes())
}

fn cl_bytes(body: &[u8]) -> Result<Page, ReadRefused> {
    clover::Request::new(MERCHANT, 500, &Cursor::start())
        .expect("a request")
        .parse(body)
}

/// A good Square order with one Bitcoin tender.
const SQ: &str = r#"{"id":"O1","location_id":"LOCSQ1","state":"COMPLETED","created_at":"2026-11-10T14:00:00Z","updated_at":"2026-11-10T14:01:00Z","total_money":{"amount":100,"currency":"USD"},"tenders":[{"id":"T1","type":"OTHER","created_at":"2026-11-10T14:00:30Z","note":"Bitcoin","amount_money":{"amount":100,"currency":"USD"}}]}"#;

/// A good Clover order with one Bitcoin payment.
const CL: &str = r#"{"id":"C1","currency":"USD","total":100,"paymentState":"PAID","createdTime":1794319200000,"modifiedTime":1794319260000,"payments":{"elements":[{"id":"P1","tender":{"label":"Bitcoin"},"amount":100,"result":"SUCCESS","createdTime":1794319230000}]}}"#;

fn sq_page(order: &str) -> String {
    format!(r#"{{"orders":[{order}]}}"#)
}

fn cl_page(order: &str) -> String {
    format!(r#"{{"elements":[{order}]}}"#)
}

/// The order with `from` replaced by `to`, which must be in it.
fn changed(order: &str, from: &str, to: &str) -> String {
    assert!(order.contains(from), "{from} is not in the order");
    order.replacen(from, to, 1)
}

#[test]
fn the_good_orders_are_read() {
    assert_eq!(sq(&sq_page(SQ)).expect("reads").orders.len(), 1);
    assert_eq!(cl(&cl_page(CL)).expect("reads").orders.len(), 1);
}

#[test]
fn ps_90_a_reply_past_the_bounds_or_not_json_is_refused_by_both_readers() {
    let big = vec![b' '; 8 * 1024 * 1024 + 1];
    assert_eq!(sq_bytes(&big), Err(ReadRefused::TooLong));
    assert_eq!(cl_bytes(&big), Err(ReadRefused::TooLong));
    // A reply of exactly 8 MiB is read.
    let mut at = br#"{"orders":[]}"#.to_vec();
    at.resize(8 * 1024 * 1024, b' ');
    assert!(sq_bytes(&at).is_ok());
    let deep = format!(
        r#"{{"orders":[],"x":{}{}}}"#,
        "[".repeat(64),
        "]".repeat(64)
    );
    assert_eq!(sq(&deep), Err(ReadRefused::TooDeep));
    let deep = format!(
        r#"{{"elements":[],"x":{}{}}}"#,
        "[".repeat(64),
        "]".repeat(64)
    );
    assert_eq!(cl(&deep), Err(ReadRefused::TooDeep));
    let ok = format!(
        r#"{{"elements":[],"x":{}{}}}"#,
        "[".repeat(62),
        "]".repeat(62)
    );
    assert!(cl(&ok).is_ok());
    let many = format!(r#"{{"orders":[],"x":[{}0]}}"#, "0,".repeat(400_000));
    assert_eq!(sq(&many), Err(ReadRefused::TooManyValues));
    assert_eq!(
        sq_bytes(b"{\"orders\":[],\"x\":\"\xff\"}"),
        Err(ReadRefused::NotUtf8)
    );
    assert_eq!(cl_bytes(b"\xff"), Err(ReadRefused::NotUtf8));
    for (body, want) in [
        ("", ReadRefused::Malformed),
        ("{", ReadRefused::Malformed),
        ("{\"orders\":[]}x", ReadRefused::TrailingBytes),
        ("{\"orders\":[],\"orders\":[]}", ReadRefused::DuplicateKey),
        (
            "{\"orders\":[],\"o\\u0072ders\":[]}",
            ReadRefused::DuplicateKey,
        ),
        (
            "{\"orders\":[{\"id\":\"a\",\"id\":\"b\"}]}",
            ReadRefused::DuplicateKey,
        ),
    ] {
        assert_eq!(sq(body), Err(want), "{body}");
    }
    assert_eq!(cl("{\"elements\":[]} []"), Err(ReadRefused::TrailingBytes));
    assert_eq!(
        cl("{\"elements\":[],\"elements\":[]}"),
        Err(ReadRefused::DuplicateKey)
    );
}

#[test]
fn ps_91_a_field_that_is_missing_null_or_of_the_wrong_type_is_a_refusal_in_square() {
    for (from, to, want) in [
        ("\"id\":\"O1\",", "", ReadRefused::Missing("orders[].id")),
        (
            "\"id\":\"O1\",",
            "\"id\":null,",
            ReadRefused::Missing("orders[].id"),
        ),
        (
            "\"id\":\"O1\"",
            "\"id\":5",
            ReadRefused::WrongType("orders[].id"),
        ),
        (
            "\"location_id\":\"LOCSQ1\",",
            "",
            ReadRefused::Missing("orders[].location_id"),
        ),
        (
            "\"location_id\":\"LOCSQ1\"",
            "\"location_id\":[]",
            ReadRefused::WrongType("orders[].location_id"),
        ),
        (
            "\"state\":\"COMPLETED\",",
            "",
            ReadRefused::Missing("orders[].state"),
        ),
        (
            "\"state\":\"COMPLETED\"",
            "\"state\":true",
            ReadRefused::WrongType("orders[].state"),
        ),
        (
            "\"created_at\":\"2026-11-10T14:00:00Z\",",
            "",
            ReadRefused::Missing("orders[].created_at"),
        ),
        (
            "\"updated_at\":\"2026-11-10T14:01:00Z\",",
            "",
            ReadRefused::Missing("orders[].updated_at"),
        ),
        (
            "\"updated_at\":\"2026-11-10T14:01:00Z\"",
            "\"updated_at\":1794319260",
            ReadRefused::WrongType("orders[].updated_at"),
        ),
        (
            "\"total_money\":{\"amount\":100,\"currency\":\"USD\"},",
            "",
            ReadRefused::Missing("orders[].total_money"),
        ),
        (
            "\"total_money\":{\"amount\":100,\"currency\":\"USD\"}",
            "\"total_money\":5",
            ReadRefused::WrongType("orders[].total_money"),
        ),
        (
            "{\"amount\":100,\"currency\":\"USD\"},\"tenders\"",
            "{\"currency\":\"USD\"},\"tenders\"",
            ReadRefused::Missing("orders[].total_money"),
        ),
        (
            "{\"amount\":100,\"currency\":\"USD\"},\"tenders\"",
            "{\"amount\":100},\"tenders\"",
            ReadRefused::Missing("orders[].total_money"),
        ),
        (
            "\"tenders\":[",
            "\"tenders\":{},\"x\":[",
            ReadRefused::WrongType("orders[].tenders"),
        ),
        (
            "\"id\":\"T1\"",
            "\"id\":9",
            ReadRefused::WrongType("orders[].tenders[].id"),
        ),
        (
            "\"id\":\"T1\",",
            "",
            ReadRefused::Missing("orders[].tenders[].id"),
        ),
        (
            "\"type\":\"OTHER\",",
            "",
            ReadRefused::Missing("orders[].tenders[].type"),
        ),
        (
            "\"type\":\"OTHER\"",
            "\"type\":1",
            ReadRefused::WrongType("orders[].tenders[].type"),
        ),
        (
            "\"created_at\":\"2026-11-10T14:00:30Z\",",
            "",
            ReadRefused::Missing("orders[].tenders[].created_at"),
        ),
        (
            "\"note\":\"Bitcoin\"",
            "\"note\":7",
            ReadRefused::WrongType("orders[].tenders[].note"),
        ),
        (
            ",\"amount_money\":{\"amount\":100,\"currency\":\"USD\"}}]",
            "}]",
            ReadRefused::Missing("orders[].tenders[].amount_money"),
        ),
        (
            "\"note\":\"Bitcoin\"",
            "\"payment_id\":3",
            ReadRefused::WrongType("orders[].tenders[].payment_id"),
        ),
        (
            "\"state\":\"COMPLETED\"",
            "\"state\":\"COMPLETED\",\"ticket_name\":3",
            ReadRefused::WrongType("orders[].ticket_name or reference_id"),
        ),
        (
            "\"state\":\"COMPLETED\"",
            "\"state\":\"COMPLETED\",\"reference_id\":[]",
            ReadRefused::WrongType("orders[].ticket_name or reference_id"),
        ),
        (
            "\"state\":\"COMPLETED\"",
            "\"state\":\"COMPLETED\",\"version\":\"4\"",
            ReadRefused::WrongType("orders[].version"),
        ),
        (
            "\"state\":\"COMPLETED\"",
            "\"state\":\"COMPLETED\",\"version\":4.5",
            ReadRefused::WrongType("orders[].version"),
        ),
        (
            "\"state\":\"COMPLETED\"",
            "\"state\":\"COMPLETED\",\"version\":-1",
            ReadRefused::OutOfRange("orders[].version"),
        ),
        (
            "\"state\":\"COMPLETED\"",
            "\"state\":\"COMPLETED\",\"version\":2147483648",
            ReadRefused::OutOfRange("orders[].version"),
        ),
        (
            "\"state\":\"COMPLETED\"",
            "\"state\":\"COMPLETED\",\"closed_at\":\"soon\"",
            ReadRefused::BadTime("orders[].closed_at"),
        ),
    ] {
        let order = changed(SQ, from, to);
        assert_eq!(sq(&sq_page(&order)), Err(want), "{from} -> {to}");
    }
    // The reply and its parts are what the vendor says they are.
    assert_eq!(sq("[]"), Err(ReadRefused::WrongType("reply")));
    assert_eq!(sq("5"), Err(ReadRefused::WrongType("reply")));
    assert_eq!(
        sq(r#"{"orders":{}}"#),
        Err(ReadRefused::WrongType("orders"))
    );
    assert_eq!(
        sq(r#"{"orders":[5]}"#),
        Err(ReadRefused::WrongType("orders[]"))
    );
    assert_eq!(
        sq(&sq_page(&changed(SQ, "\"tenders\":[", "\"tenders\":[5,"))),
        Err(ReadRefused::WrongType("orders[].tenders[]"))
    );
    // A version that is there and right is not a problem.
    assert!(sq(&sq_page(&changed(
        SQ,
        "\"state\":\"COMPLETED\"",
        "\"state\":\"COMPLETED\",\"version\":2147483647"
    )))
    .is_ok());
}

#[test]
fn ps_91_a_field_that_is_missing_null_or_of_the_wrong_type_is_a_refusal_in_clover() {
    for (from, to, want) in [
        ("\"id\":\"C1\",", "", ReadRefused::Missing("elements[].id")),
        (
            "\"id\":\"C1\"",
            "\"id\":1",
            ReadRefused::WrongType("elements[].id"),
        ),
        (
            "\"currency\":\"USD\",",
            "",
            ReadRefused::Missing("elements[].currency"),
        ),
        (
            "\"currency\":\"USD\"",
            "\"currency\":840",
            ReadRefused::WrongType("elements[].currency"),
        ),
        (
            "\"total\":100,",
            "",
            ReadRefused::Missing("elements[].total"),
        ),
        (
            "\"total\":100",
            "\"total\":\"100\"",
            ReadRefused::WrongType("elements[].total"),
        ),
        (
            "\"total\":100",
            "\"total\":null",
            ReadRefused::Missing("elements[].total"),
        ),
        (
            "\"paymentState\":\"PAID\",",
            "",
            ReadRefused::Missing("elements[].paymentState"),
        ),
        (
            "\"paymentState\":\"PAID\"",
            "\"paymentState\":1",
            ReadRefused::WrongType("elements[].paymentState"),
        ),
        (
            "\"createdTime\":1794319200000,",
            "",
            ReadRefused::Missing("elements[].createdTime"),
        ),
        (
            "\"createdTime\":1794319200000",
            "\"createdTime\":\"1794319200000\"",
            ReadRefused::WrongType("elements[].createdTime"),
        ),
        (
            "\"modifiedTime\":1794319260000,",
            "",
            ReadRefused::Missing("elements[].modifiedTime"),
        ),
        (
            "\"modifiedTime\":1794319260000",
            "\"modifiedTime\":1794319260000.5",
            ReadRefused::WrongType("elements[].modifiedTime"),
        ),
        (
            "\"paymentState\":\"PAID\"",
            "\"paymentState\":\"PAID\",\"state\":5",
            ReadRefused::WrongType("elements[].state"),
        ),
        (
            "\"paymentState\":\"PAID\"",
            "\"paymentState\":\"PAID\",\"note\":5",
            ReadRefused::WrongType("elements[].note"),
        ),
        (
            "\"payments\":{",
            "\"payments\":5,\"x\":{",
            ReadRefused::WrongType("elements[].payments"),
        ),
        (
            "\"elements\":[{\"id\":\"P1\"",
            "\"elements\":[5,{\"id\":\"P1\"",
            ReadRefused::WrongType("payments[]"),
        ),
        ("\"id\":\"P1\",", "", ReadRefused::Missing("payments[].id")),
        (
            "\"id\":\"P1\"",
            "\"id\":1",
            ReadRefused::WrongType("payments[].id"),
        ),
        (
            "\"tender\":{\"label\":\"Bitcoin\"},",
            "",
            ReadRefused::Missing("payments[].tender"),
        ),
        (
            "\"tender\":{\"label\":\"Bitcoin\"}",
            "\"tender\":\"Bitcoin\"",
            ReadRefused::WrongType("payments[].tender"),
        ),
        (
            "{\"label\":\"Bitcoin\"}",
            "{}",
            ReadRefused::Missing("payments[].tender.label"),
        ),
        (
            "{\"label\":\"Bitcoin\"}",
            "{\"label\":5}",
            ReadRefused::WrongType("payments[].tender.label"),
        ),
        (
            "{\"label\":\"Bitcoin\"}",
            "{\"label\":\"Bitcoin\",\"id\":5}",
            ReadRefused::WrongType("payments[].tender.id"),
        ),
        (
            "{\"label\":\"Bitcoin\"}",
            "{\"label\":\"Bitcoin\",\"labelKey\":[]}",
            ReadRefused::WrongType("payments[].tender.labelKey"),
        ),
        (
            "\"amount\":100,\"result\"",
            "\"result\"",
            ReadRefused::Missing("payments[].amount"),
        ),
        (
            "\"amount\":100,\"result\"",
            "\"amount\":\"100\",\"result\"",
            ReadRefused::WrongType("payments[].amount"),
        ),
        (
            "\"amount\":100,\"result\"",
            "\"amount\":100,\"tipAmount\":\"1\",\"result\"",
            ReadRefused::WrongType("payments[].tipAmount"),
        ),
        (
            "\"amount\":100,\"result\"",
            "\"amount\":100,\"externalPaymentId\":9,\"result\"",
            ReadRefused::WrongType("payments[].externalPaymentId"),
        ),
        (
            "\"result\":\"SUCCESS\",",
            "",
            ReadRefused::Missing("payments[].result"),
        ),
        (
            "\"result\":\"SUCCESS\"",
            "\"result\":1",
            ReadRefused::WrongType("payments[].result"),
        ),
        (
            "\"createdTime\":1794319230000",
            "\"createdTime\":null",
            ReadRefused::Missing("payments[].createdTime"),
        ),
        (
            "\"amount\":100,\"result\"",
            "\"amount\":100,\"note\":4,\"result\"",
            ReadRefused::WrongType("payments[].note"),
        ),
    ] {
        let order = changed(CL, from, to);
        assert_eq!(cl(&cl_page(&order)), Err(want), "{from} -> {to}");
    }
    assert_eq!(cl(r#"{}"#), Err(ReadRefused::Missing("elements")));
    assert_eq!(
        cl(r#"{"elements":null}"#),
        Err(ReadRefused::Missing("elements"))
    );
    assert_eq!(
        cl(r#"{"elements":{}}"#),
        Err(ReadRefused::WrongType("elements"))
    );
    assert_eq!(cl("[]"), Err(ReadRefused::WrongType("reply")));
    assert_eq!(
        cl(r#"{"elements":[5]}"#),
        Err(ReadRefused::WrongType("elements[]"))
    );
    // Clover answers an error with a message and no elements.
    assert_eq!(
        cl(r#"{"message":"401 Unauthorized"}"#),
        Err(ReadRefused::Missing("elements"))
    );
}

#[test]
fn ps_91_a_service_error_is_a_refusal_even_with_orders_beside_it() {
    let page = sq_page(SQ);
    let with = page.replacen(
        '{',
        r#"{"errors":[{"category":"API_ERROR","code":"X"}],"#,
        1,
    );
    assert_eq!(sq(&with), Err(ReadRefused::ServiceError));
    let none = page.replacen('{', r#"{"errors":[],"#, 1);
    assert!(sq(&none).is_ok());
    let wrong = page.replacen('{', r#"{"errors":"bad","#, 1);
    assert_eq!(sq(&wrong), Err(ReadRefused::WrongType("errors")));
}

#[test]
fn ps_91_a_state_or_kind_the_reader_does_not_know_is_a_refusal() {
    for state in ["completed", "DONE", "", "PAID", "COMPLETED "] {
        let order = changed(SQ, "COMPLETED", state);
        assert_eq!(
            sq(&sq_page(&order)),
            Err(ReadRefused::UnknownValue("orders[].state")),
            "{state:?}"
        );
    }
    for result in ["SUCCESSFUL", "success", ""] {
        let order = changed(CL, "SUCCESS", result);
        assert_eq!(
            cl(&cl_page(&order)),
            Err(ReadRefused::UnknownValue("payments[].result")),
            "{result:?}"
        );
    }
}

#[test]
fn ps_91_fields_the_readers_do_not_need_are_ignored_whatever_they_are() {
    let extra = r#""zzz":{"a":[1,2.5,-0,1e400,null,true,{"b":"c"}]},"#;
    let order = changed(SQ, "{\"id\":\"O1\",", &format!("{{{extra}\"id\":\"O1\","));
    let page = sq(&sq_page(&order)).expect("reads");
    assert_eq!(page.orders.len(), 1);
    let order = changed(CL, "{\"id\":\"C1\",", &format!("{{{extra}\"id\":\"C1\","));
    assert_eq!(cl(&cl_page(&order)).expect("reads").orders.len(), 1);
    // In the reply too, and in a tender.
    let tender = changed(
        SQ,
        "\"type\":\"OTHER\"",
        "\"type\":\"OTHER\",\"card_details\":{\"x\":[]}",
    );
    assert!(sq(&format!(r#"{{"extra":1,"orders":[{tender}]}}"#)).is_ok());
    assert!(cl(&format!(r#"{{"href":"x","elements":[{CL}]}}"#)).is_ok());
}

#[test]
fn ps_92_an_amount_is_a_whole_number_of_minor_units_from_zero() {
    let amount = |v: &str| {
        let order = changed(
            SQ,
            "{\"amount\":100,\"currency\":\"USD\"},\"tenders\"",
            &format!("{{\"amount\":{v},\"currency\":\"USD\"}},\"tenders\""),
        );
        sq(&sq_page(&order)).map(|p| p.orders[0].total_minor)
    };
    assert_eq!(amount("0"), Ok(0));
    assert_eq!(amount("9223372036854775807"), Ok(i64::MAX as u64));
    for (v, want) in [
        ("-1", ReadRefused::OutOfRange("orders[].total_money")),
        ("-0", ReadRefused::WrongType("orders[].total_money")),
        (
            "9223372036854775808",
            ReadRefused::OutOfRange("orders[].total_money"),
        ),
        (
            "18446744073709551615",
            ReadRefused::OutOfRange("orders[].total_money"),
        ),
        (
            "18446744073709551616",
            ReadRefused::WrongType("orders[].total_money"),
        ),
        ("1.5", ReadRefused::WrongType("orders[].total_money")),
        ("100.0", ReadRefused::WrongType("orders[].total_money")),
        ("1e2", ReadRefused::WrongType("orders[].total_money")),
        ("\"100\"", ReadRefused::WrongType("orders[].total_money")),
        ("true", ReadRefused::WrongType("orders[].total_money")),
    ] {
        assert_eq!(amount(v), Err(want), "{v}");
    }
    // The same in Clover, for the total, an amount and a tip.
    let total = |v: &str| {
        cl(&cl_page(&changed(
            CL,
            "\"total\":100",
            &format!("\"total\":{v}"),
        )))
        .map(|p| p.orders[0].total_minor)
    };
    assert_eq!(total("7"), Ok(7));
    assert_eq!(
        total("-7"),
        Err(ReadRefused::OutOfRange("elements[].total"))
    );
    assert_eq!(
        total("7.0"),
        Err(ReadRefused::WrongType("elements[].total"))
    );
    assert_eq!(
        total("7e0"),
        Err(ReadRefused::WrongType("elements[].total"))
    );
    assert_eq!(
        total("9223372036854775808"),
        Err(ReadRefused::OutOfRange("elements[].total"))
    );
    let tip = |v: &str| {
        cl(&cl_page(&changed(
            CL,
            "\"amount\":100,\"result\"",
            &format!("\"amount\":100,\"tipAmount\":{v},\"result\""),
        )))
        .map(|p| p.orders[0].tenders[0].tip_minor)
    };
    assert_eq!(tip("5"), Ok(5));
    assert_eq!(tip("null"), Ok(0));
    assert_eq!(
        tip("-5"),
        Err(ReadRefused::OutOfRange("payments[].tipAmount"))
    );
}

#[test]
fn ps_92_a_tip_larger_than_the_amount_that_holds_it_is_refused_in_square() {
    let with_tip = |tip: u32| {
        let order = changed(
            SQ,
            "\"note\":\"Bitcoin\"",
            &format!(
                "\"note\":\"Bitcoin\",\"tip_money\":{{\"amount\":{tip},\"currency\":\"USD\"}}"
            ),
        );
        sq(&sq_page(&order)).map(|p| {
            (
                p.orders[0].tenders[0].amount_minor,
                p.orders[0].tenders[0].tip_minor,
            )
        })
    };
    assert_eq!(with_tip(0), Ok((100, 0)));
    assert_eq!(with_tip(100), Ok((0, 100)));
    assert_eq!(
        with_tip(101),
        Err(ReadRefused::OutOfRange("orders[].tenders[].tip_money"))
    );
}

#[test]
fn ps_92_a_time_that_is_not_strictly_a_time_is_a_refusal() {
    for bad in [
        "2026-11-10 14:00:00Z",
        "2026-11-10T14:00:00",
        "2026-02-30T14:00:00Z",
        "yesterday",
        "1970-01-01T00:00:00+01:00",
    ] {
        let order = changed(SQ, "2026-11-10T14:00:00Z", bad);
        assert_eq!(
            sq(&sq_page(&order)),
            Err(ReadRefused::BadTime("orders[].created_at")),
            "{bad}"
        );
    }
    // Milliseconds past the year 9999.
    let late = changed(CL, "1794319200000", "253402300800000");
    assert_eq!(
        cl(&cl_page(&late)),
        Err(ReadRefused::BadTime("elements[].createdTime"))
    );
    let neg = changed(CL, "1794319200000", "-1");
    assert_eq!(
        cl(&cl_page(&neg)),
        Err(ReadRefused::OutOfRange("elements[].createdTime"))
    );
}

#[test]
fn ps_93_a_currency_that_is_not_three_capitals_or_that_differs_in_an_order_is_a_refusal() {
    for bad in ["usd", "US", "USDT", "", "U$D", "840"] {
        let order = changed(
            SQ,
            "\"total_money\":{\"amount\":100,\"currency\":\"USD\"}",
            &format!("\"total_money\":{{\"amount\":100,\"currency\":\"{bad}\"}}"),
        );
        assert_eq!(
            sq(&sq_page(&order)),
            Err(ReadRefused::BadCurrency("orders[].total_money")),
            "{bad:?}"
        );
        let order = changed(
            CL,
            "\"currency\":\"USD\"",
            &format!("\"currency\":\"{bad}\""),
        );
        assert_eq!(
            cl(&cl_page(&order)),
            Err(ReadRefused::BadCurrency("elements[].currency")),
            "{bad:?}"
        );
    }
    // A tender in another currency than its order.
    let order = changed(
        SQ,
        "\"note\":\"Bitcoin\",\"amount_money\":{\"amount\":100,\"currency\":\"USD\"}",
        "\"note\":\"Bitcoin\",\"amount_money\":{\"amount\":100,\"currency\":\"EUR\"}",
    );
    assert_eq!(sq(&sq_page(&order)), Err(ReadRefused::CurrencyMismatch));
    // And a tip in another than its tender.
    let order = changed(
        SQ,
        "\"note\":\"Bitcoin\"",
        "\"note\":\"Bitcoin\",\"tip_money\":{\"amount\":1,\"currency\":\"EUR\"}",
    );
    assert_eq!(sq(&sq_page(&order)), Err(ReadRefused::CurrencyMismatch));
    // A currency of two capital letters of another kind is still three.
    let eur = changed(CL, "\"currency\":\"USD\"", "\"currency\":\"EUR\"");
    assert_eq!(
        cl(&cl_page(&eur)).expect("reads").orders[0]
            .currency
            .to_string(),
        "EUR"
    );
}

#[test]
fn ps_93_an_id_or_a_location_out_of_bounds_is_a_refusal_and_a_name_or_a_note_is_cut() {
    for bad in ["", &"x".repeat(65), "caf\u{e9}", "a\\nb"] {
        let order = changed(SQ, "\"id\":\"O1\"", &format!("\"id\":\"{bad}\""));
        assert_eq!(
            sq(&sq_page(&order)),
            Err(ReadRefused::BadId("orders[].id")),
            "{bad:?}"
        );
        let order = changed(CL, "\"id\":\"C1\"", &format!("\"id\":\"{bad}\""));
        assert_eq!(
            cl(&cl_page(&order)),
            Err(ReadRefused::BadId("elements[].id")),
            "{bad:?}"
        );
    }
    let order = changed(
        SQ,
        "\"id\":\"T1\"",
        &format!("\"id\":\"{}\"", "t".repeat(65)),
    );
    assert_eq!(
        sq(&sq_page(&order)),
        Err(ReadRefused::BadId("orders[].tenders[].id"))
    );
    // A location is refused, not cut: a cut one would be another place.
    for bad in ["", &"L".repeat(65), "a\\tb"] {
        let order = changed(
            SQ,
            "\"location_id\":\"LOCSQ1\"",
            &format!("\"location_id\":\"{bad}\""),
        );
        assert_eq!(
            sq(&sq_page(&order)),
            Err(ReadRefused::BadText("orders[].location_id")),
            "{bad:?}"
        );
    }
    // A name is cut; an empty one or one with a control character is not.
    let long = "N".repeat(100);
    let order = changed(
        CL,
        "\"label\":\"Bitcoin\"",
        &format!("\"label\":\"{long}\""),
    );
    assert_eq!(
        cl(&cl_page(&order)).expect("reads").orders[0].tenders[0]
            .name
            .as_str()
            .len(),
        64
    );
    for bad in ["", "a\\u0007b", "a\\nb"] {
        let order = changed(CL, "\"label\":\"Bitcoin\"", &format!("\"label\":\"{bad}\""));
        assert_eq!(
            cl(&cl_page(&order)),
            Err(ReadRefused::BadText("payments[].tender.label")),
            "{bad:?}"
        );
    }
    // A note with a control character is read with a space in its place;
    // one that is only white space is no note.
    let order = changed(
        CL,
        "\"amount\":100,\"result\"",
        "\"amount\":100,\"note\":\"a\\nb\\u0000c\",\"result\"",
    );
    assert_eq!(
        cl(&cl_page(&order)).expect("reads").orders[0].tenders[0]
            .reference
            .as_ref()
            .map(|t| t.to_string()),
        Some("a b c".to_string())
    );
    let order = changed(
        CL,
        "\"amount\":100,\"result\"",
        "\"amount\":100,\"note\":\" \\n \",\"result\"",
    );
    assert_eq!(
        cl(&cl_page(&order)).expect("reads").orders[0].tenders[0].reference,
        None
    );
}

#[test]
fn ps_94_more_than_500_orders_in_a_page_are_refused_whole_and_500_are_read() {
    let many = |n: usize| sq_page(&vec![SQ; n].join(","));
    assert_eq!(sq(&many(500)).expect("reads").orders.len(), 500);
    assert_eq!(sq(&many(501)), Err(ReadRefused::TooManyOrders));
    let cl_many = |n: usize| cl_page(&vec![CL; n].join(","));
    assert_eq!(cl(&cl_many(500)).expect("reads").orders.len(), 500);
    assert_eq!(cl(&cl_many(501)), Err(ReadRefused::TooManyOrders));
    // One bad order refuses the page whole.
    let bad = changed(SQ, "\"state\":\"COMPLETED\"", "\"state\":\"X\"");
    assert_eq!(
        sq(&sq_page(&format!("{SQ},{bad},{SQ}"))),
        Err(ReadRefused::UnknownValue("orders[].state"))
    );
}

#[test]
fn ps_94_an_order_with_more_than_16_tenders_or_two_with_one_id_is_refused() {
    let tender = |i: usize| {
        format!(
            r#"{{"id":"T{i}","type":"CASH","created_at":"2026-11-10T14:00:30Z","amount_money":{{"amount":1,"currency":"USD"}}}}"#
        )
    };
    let order = |n: usize| {
        let tenders: Vec<String> = (0..n).map(tender).collect();
        changed(
            SQ,
            &SQ[SQ.find("\"tenders\":[").expect("tenders")..SQ.len() - 1],
            &format!("\"tenders\":[{}]", tenders.join(",")),
        )
    };
    assert_eq!(
        sq(&sq_page(&order(16))).expect("reads").orders[0]
            .tenders
            .len(),
        16
    );
    assert_eq!(sq(&sq_page(&order(17))), Err(ReadRefused::TooManyTenders));
    let dup = changed(&order(2), "\"id\":\"T1\"", "\"id\":\"T0\"");
    assert_eq!(sq(&sq_page(&dup)), Err(ReadRefused::DuplicateTender));
    // Clover: the payments of an order.
    let payment = |id: &str| {
        format!(
            r#"{{"id":"{id}","tender":{{"label":"Cash"}},"amount":1,"result":"SUCCESS","createdTime":1794319230000}}"#
        )
    };
    let with = |ids: Vec<String>| {
        let payments: Vec<String> = ids.iter().map(|id| payment(id)).collect();
        let start = CL.find("\"payments\":").expect("payments");
        cl_page(&format!(
            "{}\"payments\":[{}]}}",
            &CL[..start],
            payments.join(",")
        ))
    };
    let ids = |n: usize| (0..n).map(|i| format!("P{i}")).collect::<Vec<_>>();
    assert_eq!(
        cl(&with(ids(16))).expect("reads").orders[0].tenders.len(),
        16
    );
    assert_eq!(cl(&with(ids(17))), Err(ReadRefused::TooManyTenders));
    assert_eq!(
        cl(&with(vec!["P1".into(), "P1".into()])),
        Err(ReadRefused::DuplicateTender)
    );
}

#[test]
fn ps_94_a_refund_beyond_its_tender_or_for_none_is_refused() {
    // Covered with each vendor's own refunds in the square and clover tests;
    // here a refund that equals the tender and its tip is read.
    let order = changed(
        SQ,
        "\"note\":\"Bitcoin\"",
        "\"note\":\"Bitcoin\",\"tip_money\":{\"amount\":10,\"currency\":\"USD\"}",
    );
    let refunds = r#","refunds":[{"tender_id":"T1","status":"APPROVED","amount_money":{"amount":100,"currency":"USD"}}]"#;
    let with = format!("{}{refunds}}}", &order[..order.len() - 1]);
    assert!(sq(&sq_page(&with)).is_ok());
    let over = with.replace(
        "\"amount\":100,\"currency\":\"USD\"}}]}",
        "\"amount\":101,\"currency\":\"USD\"}}]}",
    );
    assert_eq!(sq(&sq_page(&over)), Err(ReadRefused::RefundTooLarge));
}

#[test]
fn ps_99_the_same_bytes_give_the_same_answer_every_time() {
    let body = sq_page(SQ);
    assert_eq!(sq(&body), sq(&body));
    let bad = sq_page(&changed(SQ, "COMPLETED", "X"));
    assert_eq!(sq(&bad), sq(&bad));
    let body = cl_page(CL);
    assert_eq!(cl(&body), cl(&body));
    // And the request is the same text, whatever the state of the machine.
    let c = cursor_at(1_700_000_000).expect("a cursor");
    let a = square::Request::new(&settings(), 10, &c).expect("a request");
    assert_eq!(a.body(), a.clone().body());
    let b = clover::Request::new(MERCHANT, 10, &c).expect("a request");
    assert_eq!(b.path_and_query(), b.clone().path_and_query());
}
