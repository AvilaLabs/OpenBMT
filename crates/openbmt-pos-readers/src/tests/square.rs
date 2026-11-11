//! PS-95, PS-96: the Square request and the reading of its reply.

use openbmt_json::{parse, Limits, Value};
use openbmt_pos::{
    Id, OrderState, Page, PosOrder, PosTender, TenderKind, TenderState, MAX_PAGE_ORDERS,
};

use crate::square::{Request, SEARCH_PATH};
use crate::{cursor_at, ReadRefused, RequestRefused};

use super::{settings, text, usd, LOC};

const PAGE1: &[u8] = include_bytes!("../../tests/fixtures/square_orders_page1.json");
const LAST: &[u8] = include_bytes!("../../tests/fixtures/square_orders_last.json");
const PAYMENTS: &[u8] = include_bytes!("../../tests/fixtures/square_payments.json");
const EMPTY: &[u8] = include_bytes!("../../tests/fixtures/square_orders_empty.json");

/// 2026-11-10T13:56:40Z, before every order of the fixtures.
const SINCE: u64 = 1_794_319_000;

fn request() -> Request {
    Request::new(&settings(), 500, &cursor_at(SINCE).expect("a cursor")).expect("a request")
}

fn id(s: &str) -> Id {
    Id::new(s).expect("an id")
}

fn body_of(r: &Request) -> Value {
    parse(r.body().as_bytes(), &Limits::DEFAULT).expect("the body is JSON")
}

fn get<'a>(v: &'a Value, path: &[&str]) -> &'a Value {
    path.iter()
        .fold(v, |v, k| v.get(k).unwrap_or_else(|| panic!("no {k}")))
}

#[test]
fn ps_95_the_body_asks_for_closed_and_cancelled_orders_by_update_time() {
    let r = request();
    let body = body_of(&r);
    assert_eq!(SEARCH_PATH, "/v2/orders/search");
    assert_eq!(
        get(&body, &["location_ids"]).as_array(),
        Some(&[Value::Str(LOC.into()), Value::Str(super::MERCHANT.into())][..])
    );
    assert_eq!(get(&body, &["limit"]).as_u64(), Some(500));
    assert!(body.get("cursor").is_none());
    let filter = get(&body, &["query", "filter"]);
    assert_eq!(
        get(filter, &["state_filter", "states"]).as_array(),
        Some(
            &[
                Value::Str("COMPLETED".into()),
                Value::Str("CANCELED".into())
            ][..]
        )
    );
    assert_eq!(
        get(filter, &["date_time_filter", "updated_at", "start_at"]).as_str(),
        Some("2026-11-10T13:56:40Z")
    );
    let sort = get(&body, &["query", "sort"]);
    assert_eq!(get(sort, &["sort_field"]).as_str(), Some("UPDATED_AT"));
    assert_eq!(get(sort, &["sort_order"]).as_str(), Some("ASC"));
    // Nothing else is asked.
    assert_eq!(body.as_object().map(<[_]>::len), Some(3));
    assert_eq!(filter.as_object().map(<[_]>::len), Some(2));
}

#[test]
fn ps_95_the_body_is_the_same_text_every_time_and_has_no_blank() {
    let a = request().body();
    assert_eq!(a, request().body());
    assert!(!a.contains(' ') && !a.contains('\n'));
}

#[test]
fn ps_95_the_vendors_cursor_goes_back_in_the_body_and_text_is_escaped() {
    let marker = cursor_text("1794319000|Ab_c-D.9");
    let r = Request::new(&settings(), 25, &marker).expect("a request");
    let body = body_of(&r);
    assert_eq!(get(&body, &["cursor"]).as_str(), Some("Ab_c-D.9"));
    assert_eq!(get(&body, &["limit"]).as_u64(), Some(25));
    // A location with a quote, a backslash and a non-ASCII letter.
    let odd = "L\"\\\u{e9}";
    let mut s = settings();
    s.locations[0].location = text(odd);
    let r = Request::new(&s, 10, &cursor_at(0).expect("a cursor")).expect("a request");
    let body = body_of(&r);
    assert_eq!(
        get(&body, &["location_ids"])
            .as_array()
            .and_then(|a| a.first()),
        Some(&Value::Str(odd.into()))
    );
    assert_eq!(
        get(
            &body,
            &[
                "query",
                "filter",
                "date_time_filter",
                "updated_at",
                "start_at"
            ]
        )
        .as_str(),
        Some("1970-01-01T00:00:00Z")
    );
}

fn cursor_text(s: &str) -> openbmt_pos::Cursor {
    openbmt_pos::Cursor::new(s.as_bytes().to_vec()).expect("a cursor")
}

#[test]
fn ps_95_a_request_needs_a_location_a_page_size_and_a_cursor_in_form() {
    let mut none = settings();
    none.locations.clear();
    let start = openbmt_pos::Cursor::start();
    assert_eq!(
        Request::new(&none, 10, &start),
        Err(RequestRefused::NoLocations)
    );
    for limit in [0, MAX_PAGE_ORDERS + 1, usize::MAX] {
        assert_eq!(
            Request::new(&settings(), limit, &start),
            Err(RequestRefused::BadLimit)
        );
    }
    assert!(Request::new(&settings(), 1, &start).is_ok());
    assert!(Request::new(&settings(), MAX_PAGE_ORDERS, &start).is_ok());
    assert_eq!(
        Request::new(&settings(), 10, &cursor_text("x")),
        Err(RequestRefused::BadCursor)
    );
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
fn ps_96_the_first_page_of_the_fixture_is_read_into_the_model() {
    let page = request().parse(PAGE1, None).expect("reads");
    assert_eq!(page.orders.len(), 3);
    assert!(page.more);
    assert_eq!(
        page.next.as_bytes(),
        b"1794319000|PAGE2OPAQUE-cursor_value.1"
    );

    let o = order(&page, "SQORDER0001");
    assert_eq!(
        *o,
        PosOrder {
            id: id("SQORDER0001"),
            location: text(LOC),
            state: OrderState::Closed,
            currency: usd(),
            total_minor: 1250,
            tenders: vec![
                PosTender {
                    id: id("TENDERA01"),
                    kind: TenderKind::Cash,
                    name: text("Cash"),
                    amount_minor: 300,
                    tip_minor: 0,
                    state: TenderState::Paid,
                    at: 1_794_319_420,
                    reference: None,
                },
                PosTender {
                    id: id("TENDERA02"),
                    kind: TenderKind::Other,
                    name: text("Bitcoin"),
                    // 1100 with the tip of 150, without the tip.
                    amount_minor: 950,
                    tip_minor: 150,
                    state: TenderState::Paid,
                    at: 1_794_319_421,
                    reference: Some(text("Bitcoin")),
                },
            ],
            opened_at: 1_794_319_205,
            closed_at: Some(1_794_319_421),
            updated_at: 1_794_319_421,
            reference: Some(text("Table 4")),
        }
    );

    // A cancelled order is a voided one; it has no tenders here.
    let c = order(&page, "SQORDER0002");
    assert_eq!(c.state, OrderState::Voided);
    assert_eq!(
        (c.opened_at, c.updated_at, c.closed_at),
        (1_794_319_800, 1_794_319_920, None)
    );
    assert!(c.tenders.is_empty() && c.reference.is_none());

    // Times with an offset are taken to UTC; the approved refund counts and
    // the rejected one does not.
    let d = order(&page, "SQORDER0003");
    assert_eq!(d.state, OrderState::Closed);
    assert_eq!((d.opened_at, d.updated_at), (1_794_320_400, 1_794_320_460));
    assert_eq!(d.reference, Some(text("ext-77")));
    let card = tender(d, "TENDERC01");
    assert_eq!(
        (card.kind, card.state, card.at),
        (
            TenderKind::Card,
            TenderState::Refunded { amount_minor: 500 },
            1_794_320_430
        )
    );
    for o in &page.orders {
        assert_eq!(o.validate(), Ok(()));
    }
}

#[test]
fn ps_96_which_tenders_are_bitcoin_is_the_settings_exact_name() {
    let s = settings();
    let page = request().parse(PAGE1, None).expect("reads");
    let a = order(&page, "SQORDER0001");
    assert!(s.is_bitcoin(tender(a, "TENDERA02")));
    assert!(!s.is_bitcoin(tender(a, "TENDERA01")));
    let last = request().parse(LAST, Some(PAYMENTS)).expect("reads");
    let d = order(&last, "SQORDER0004");
    // The source of the payment names it, whatever the note says.
    assert!(s.is_bitcoin(tender(d, "TENDERD01")));
    // The note, compared without case and spaces, names it.
    assert!(s.is_bitcoin(tender(d, "TENDERD02")));
    // Neither names it.
    assert!(!s.is_bitcoin(tender(d, "TENDERD03")));
}

#[test]
fn ps_96_the_payments_of_the_reply_give_the_source_the_state_and_the_note() {
    let page = request().parse(LAST, Some(PAYMENTS)).expect("reads");
    assert!(!page.more);
    // No cursor: the next poll starts at the newest update.
    assert_eq!(page.next.as_bytes(), b"1794322920");
    let o = order(&page, "SQORDER0004");
    let t1 = tender(o, "TENDERD01");
    assert_eq!((t1.kind, t1.name.as_str()), (TenderKind::Other, "Bitcoin"));
    // The tender's own note is the reference; the payment's would be used if
    // it had none.
    assert_eq!(t1.reference, Some(text("gift")));
    assert_eq!(t1.state, TenderState::Paid);
    let t2 = tender(o, "TENDERD02");
    assert_eq!(t2.name.as_str(), "Bitcoin");
    assert_eq!(t2.reference, Some(text("bitcoin")));
    // A cancelled payment is a voided tender.
    assert_eq!(t2.state, TenderState::Voided);
    let t3 = tender(o, "TENDERD03");
    assert_eq!((t3.kind, t3.name.as_str()), (TenderKind::Other, "Other"));
    assert_eq!(t3.state, TenderState::Paid);
}

#[test]
fn ps_96_without_payments_only_the_note_can_name_a_bitcoin_tender() {
    let page = request().parse(LAST, None).expect("reads");
    let o = order(&page, "SQORDER0004");
    assert_eq!(tender(o, "TENDERD01").name.as_str(), "Other");
    assert_eq!(tender(o, "TENDERD02").name.as_str(), "Bitcoin");
    assert_eq!(tender(o, "TENDERD02").state, TenderState::Paid);
    assert_eq!(tender(o, "TENDERD03").name.as_str(), "Other");
}

#[test]
fn ps_96_a_payment_note_is_the_reference_when_the_tender_has_none() {
    let orders = br#"{"orders":[{"id":"O1","location_id":"LOCSQ1","state":"COMPLETED",
        "created_at":"2026-11-10T14:00:00Z","updated_at":"2026-11-10T14:01:00Z",
        "total_money":{"amount":100,"currency":"USD"},
        "tenders":[{"id":"T1","type":"OTHER","created_at":"2026-11-10T14:00:30Z",
        "amount_money":{"amount":100,"currency":"USD"},"payment_id":"P1"}]}]}"#;
    let payments = br#"{"payments":[{"id":"P1","status":"COMPLETED",
        "note":"btc K7M2QX","external_details":{"type":"CRYPTO","source":"Bitcoin"}}]}"#;
    let page = request().parse(orders, Some(payments)).expect("reads");
    let t = tender(order(&page, "O1"), "T1");
    assert_eq!(t.reference, Some(text("btc K7M2QX")));
    assert_eq!(t.name.as_str(), "Bitcoin");
}

#[test]
fn ps_96_a_reply_with_nothing_found_is_an_empty_page_that_keeps_its_place() {
    let page = request().parse(EMPTY, None).expect("reads");
    assert!(page.orders.is_empty() && !page.more);
    assert_eq!(page.next.as_bytes(), b"1794319000");
    let page = request().parse(br#"{"orders":[]}"#, None).expect("reads");
    assert!(page.orders.is_empty() && !page.more);
    assert_eq!(page.next, cursor_at(SINCE).expect("a cursor"));
    let page = request().parse(br#"{"errors":[]}"#, None).expect("reads");
    assert!(page.orders.is_empty());
}

#[test]
fn ps_96_a_cursor_in_the_reply_means_more_and_keeps_the_time() {
    let r = request();
    let page = r.parse(br#"{"cursor":"abc"}"#, None).expect("reads");
    assert!(page.more);
    assert_eq!(page.next.as_bytes(), b"1794319000|abc");
    // Reading on from there asks with that marker and the same time.
    let r2 = Request::new(&settings(), 500, &page.next).expect("a request");
    let body = body_of(&r2);
    assert_eq!(get(&body, &["cursor"]).as_str(), Some("abc"));
    assert_eq!(
        get(
            &body,
            &[
                "query",
                "filter",
                "date_time_filter",
                "updated_at",
                "start_at"
            ]
        )
        .as_str(),
        Some("2026-11-10T13:56:40Z")
    );
}

#[test]
fn ps_96_the_next_time_never_goes_back() {
    // Orders older than where the page was read from do not move it back.
    let late = Request::new(
        &settings(),
        500,
        &cursor_at(1_800_000_000).expect("a cursor"),
    )
    .expect("a request");
    let page = late.parse(LAST, None).expect("reads");
    assert_eq!(page.next.as_bytes(), b"1800000000");
}

#[test]
fn ps_96_every_documented_tender_type_is_read_and_another_is_refused() {
    let tender_of = |kind: &str| {
        format!(
            r#"{{"orders":[{{"id":"O1","location_id":"LOCSQ1","state":"OPEN",
            "created_at":"2026-11-10T14:00:00Z","updated_at":"2026-11-10T14:01:00Z",
            "total_money":{{"amount":100,"currency":"USD"}},
            "tenders":[{{"id":"T1","type":"{kind}","created_at":"2026-11-10T14:00:30Z",
            "amount_money":{{"amount":100,"currency":"USD"}}}}]}}]}}"#
        )
    };
    let kind_of = |kind: &str| {
        request()
            .parse(tender_of(kind).as_bytes(), None)
            .map(|p| (p.orders[0].state, p.orders[0].tenders[0].kind))
    };
    assert_eq!(kind_of("CARD"), Ok((OrderState::Open, TenderKind::Card)));
    assert_eq!(
        kind_of("THIRD_PARTY_CARD"),
        Ok((OrderState::Open, TenderKind::Card))
    );
    assert_eq!(kind_of("CASH"), Ok((OrderState::Open, TenderKind::Cash)));
    for k in [
        "OTHER",
        "SQUARE_GIFT_CARD",
        "BANK_ACCOUNT",
        "WALLET",
        "BUY_NOW_PAY_LATER",
        "SQUARE_ACCOUNT",
        "NO_SALE",
    ] {
        assert_eq!(kind_of(k), Ok((OrderState::Open, TenderKind::Other)), "{k}");
    }
    // A type that is not on the list is an "Other" tender, never Bitcoin.
    for k in ["", "cash", "Card", "CRYPTO", "BITCOIN"] {
        assert_eq!(kind_of(k), Ok((OrderState::Open, TenderKind::Other)), "{k}");
    }
}

#[test]
fn ps_91_an_unknown_tender_type_beside_a_bitcoin_tender_reads_fine() {
    let body = br#"{"orders":[{"id":"O1","location_id":"LOCSQ1","state":"COMPLETED",
        "created_at":"2026-11-10T14:00:00Z","updated_at":"2026-11-10T14:01:00Z",
        "total_money":{"amount":300,"currency":"USD"},
        "tenders":[
        {"id":"T1","type":"OTHER","created_at":"2026-11-10T14:00:30Z","note":"Bitcoin",
         "amount_money":{"amount":100,"currency":"USD"}},
        {"id":"T2","type":"CRYPTO_FUTURE","created_at":"2026-11-10T14:00:31Z",
         "amount_money":{"amount":200,"currency":"USD"}}]}]}"#;
    let page = request().parse(body, None).expect("reads");
    let o = order(&page, "O1");
    assert_eq!(o.tenders.len(), 2);
    let unknown = tender(o, "T2");
    assert_eq!(
        (unknown.kind, unknown.name.as_str()),
        (TenderKind::Other, "Other (CRYPTO_FUTURE)")
    );
    let s = settings();
    assert!(s.is_bitcoin(tender(o, "T1")));
    assert!(!s.is_bitcoin(unknown));
    // The text of the type is bounded and has no control character.
    let long = body.iter().copied().collect::<Vec<u8>>();
    let long = String::from_utf8(long)
        .expect("text")
        .replace("CRYPTO_FUTURE", &format!("{}\\n", "X".repeat(100)));
    let page = request().parse(long.as_bytes(), None).expect("reads");
    let name = tender(order(&page, "O1"), "T2").name.as_str().to_string();
    assert!(name.len() <= 64 && name.starts_with("Other (XXX") && !name.contains('\n'));
}

#[test]
fn ps_91_an_unknown_status_matters_only_to_a_bitcoin_tender() {
    let r = request();
    // The status of the payment of the Bitcoin tender: refused.
    let btc = br#"{"payments":[{"id":"PAYD01","status":"PAUSED",
        "external_details":{"source":"Bitcoin"}}]}"#;
    assert_eq!(
        r.parse(LAST, Some(btc)),
        Err(ReadRefused::UnknownValue("payments[].status"))
    );
    // The status of the payment of the check: the tender is read as paid.
    let check = br#"{"payments":[{"id":"PAYD01","status":"COMPLETED",
        "external_details":{"source":"Bitcoin"}},{"id":"PAYD03","status":"PAUSED"}]}"#;
    let page = r.parse(LAST, Some(check)).expect("reads");
    let t = tender(order(&page, "SQORDER0004"), "TENDERD03");
    assert_eq!(t.state, TenderState::Paid);
    // An order state that is not known: left out when no Bitcoin tender.
    let cash = br#"{"orders":[{"id":"O1","location_id":"LOCSQ1","state":"ON_HOLD",
        "created_at":"2026-11-10T14:00:00Z","updated_at":"2026-11-10T14:01:00Z",
        "total_money":{"amount":100,"currency":"USD"},
        "tenders":[{"id":"T1","type":"CASH","created_at":"2026-11-10T14:00:30Z",
        "amount_money":{"amount":100,"currency":"USD"}}]}]}"#;
    let page = r.parse(cash, None).expect("reads");
    assert!(page.orders.is_empty());
    let btc_order = String::from_utf8(cash.to_vec())
        .expect("text")
        .replace(r#""type":"CASH""#, r#""type":"OTHER","note":"Bitcoin""#);
    assert_eq!(
        r.parse(btc_order.as_bytes(), None),
        Err(ReadRefused::UnknownValue("orders[].state"))
    );
    // A refund status that is not known: ignored for cash, refused for Bitcoin.
    let refund = |note: &str| {
        let body = format!(
            r#"{{"orders":[{{"id":"O1","location_id":"LOCSQ1","state":"COMPLETED",
            "created_at":"2026-11-10T14:00:00Z","updated_at":"2026-11-10T14:01:00Z",
            "total_money":{{"amount":100,"currency":"USD"}},
            "tenders":[{{"id":"T1","type":"OTHER","created_at":"2026-11-10T14:00:30Z",
            "note":"{note}","amount_money":{{"amount":100,"currency":"USD"}}}}],
            "refunds":[{{"tender_id":"T1","status":"HELD","amount_money":{{"amount":5,"currency":"USD"}}}}]}}]}}"#
        );
        r.parse(body.as_bytes(), None)
    };
    assert_eq!(refund("cheque").expect("reads").orders.len(), 1);
    assert_eq!(
        refund("Bitcoin"),
        Err(ReadRefused::UnknownValue("orders[].refunds[].status"))
    );
}

#[test]
fn ps_96_an_open_or_draft_order_is_open() {
    for state in ["OPEN", "DRAFT"] {
        let body = format!(
            r#"{{"orders":[{{"id":"O1","location_id":"LOCSQ1","state":"{state}",
            "created_at":"2026-11-10T14:00:00Z","updated_at":"2026-11-10T14:01:00Z",
            "closed_at":"not a time","total_money":{{"amount":0,"currency":"USD"}}}}]}}"#
        );
        let page = request().parse(body.as_bytes(), None).expect("reads");
        // A closing time on an order that is not closed is not read.
        assert_eq!(page.orders[0].state, OrderState::Open);
        assert_eq!(page.orders[0].closed_at, None);
    }
}

#[test]
fn ps_96_refunds_add_up_on_their_tender_and_a_voided_tender_stays_voided() {
    let order = |refunds: &str, payments_status: &str| {
        let orders = format!(
            r#"{{"orders":[{{"id":"O1","location_id":"LOCSQ1","state":"COMPLETED",
            "created_at":"2026-11-10T14:00:00Z","updated_at":"2026-11-10T14:01:00Z",
            "closed_at":"2026-11-10T14:01:00Z",
            "total_money":{{"amount":1000,"currency":"USD"}},
            "tenders":[{{"id":"T1","type":"OTHER","created_at":"2026-11-10T14:00:30Z",
            "note":"Bitcoin","amount_money":{{"amount":1000,"currency":"USD"}},"payment_id":"P1"}}],
            "refunds":[{refunds}]}}]}}"#
        );
        let payments = format!(r#"{{"payments":[{{"id":"P1","status":"{payments_status}"}}]}}"#);
        request().parse(orders.as_bytes(), Some(payments.as_bytes()))
    };
    let refund = |id: &str, status: &str, amount: u64| {
        format!(
            r#"{{"id":"{id}","tender_id":"T1","status":"{status}","amount_money":{{"amount":{amount},"currency":"USD"}}}}"#
        )
    };
    let two = [
        refund("R1", "APPROVED", 300),
        refund("R2", "PENDING", 200),
        refund("R3", "FAILED", 900),
    ]
    .join(",");
    let page = order(&two, "COMPLETED").expect("reads");
    assert_eq!(
        page.orders[0].tenders[0].state,
        TenderState::Refunded { amount_minor: 500 }
    );
    // A refund on a voided tender does not undo the void.
    let page = order(&refund("R1", "APPROVED", 300), "CANCELED").expect("reads");
    assert_eq!(page.orders[0].tenders[0].state, TenderState::Voided);
    // Only refunds that did not happen: not refunded.
    let page = order(&refund("R1", "REJECTED", 300), "COMPLETED").expect("reads");
    assert_eq!(page.orders[0].tenders[0].state, TenderState::Paid);
    // More than the tender: refused whole (PS-94).
    assert_eq!(
        order(&refund("R1", "APPROVED", 1001), "COMPLETED"),
        Err(ReadRefused::RefundTooLarge)
    );
    let big = [refund("R1", "APPROVED", 600), refund("R2", "APPROVED", 401)].join(",");
    assert_eq!(order(&big, "COMPLETED"), Err(ReadRefused::RefundTooLarge));
    // For a tender the order does not have.
    let other = refund("R1", "APPROVED", 5).replace("\"T1\"", "\"T9\"");
    assert_eq!(
        order(&other, "COMPLETED"),
        Err(ReadRefused::RefundWithoutTender)
    );
    // Not for nothing.
    assert_eq!(
        order(&refund("R1", "APPROVED", 0), "COMPLETED"),
        Err(ReadRefused::OutOfRange("orders[].refunds[].amount_money"))
    );
    assert_eq!(
        order(&refund("R1", "REVERSED", 5), "COMPLETED"),
        Err(ReadRefused::UnknownValue("orders[].refunds[].status"))
    );
}

#[test]
fn ps_96_the_payments_reply_is_read_as_strictly_as_the_orders() {
    let r = request();
    for (payments, want) in [
        (
            r#"{"payments":[{"status":"COMPLETED"}]}"#,
            ReadRefused::Missing("payments[].id"),
        ),
        (
            r#"{"payments":[{"id":"P1"}]}"#,
            ReadRefused::Missing("payments[].status"),
        ),
        (r#"{"payments":{}}"#, ReadRefused::WrongType("payments")),
        (r#"[]"#, ReadRefused::WrongType("payments reply")),
        (r#"{"errors":[{"code":"X"}]}"#, ReadRefused::ServiceError),
        (
            r#"{"payments":[{"id":"P1","status":"COMPLETED","note":5}]}"#,
            ReadRefused::WrongType("payments[].note"),
        ),
        (
            r#"{"payments":[{"id":"P1","status":"COMPLETED","external_details":{"source":5}}]}"#,
            ReadRefused::WrongType("payments[].external_details.source"),
        ),
        (
            r#"{"payments":[{"id":"P1","status":"COMPLETED","external_details":[]}]}"#,
            ReadRefused::WrongType("payments[].external_details"),
        ),
        (r#"{"payments":[1]}"#, ReadRefused::WrongType("payments[]")),
        ("{", ReadRefused::Malformed),
    ] {
        assert_eq!(
            r.parse(EMPTY, Some(payments.as_bytes())),
            Err(want),
            "{payments}"
        );
    }
    // Extra fields and payments of no order are ignored.
    let ok = br#"{"payments":[{"id":"PX","status":"APPROVED","receipt_url":"x","extra":{"a":[1]}}],"cursor":"ignored"}"#;
    assert!(r.parse(EMPTY, Some(ok)).is_ok());
}

#[test]
fn ps_96_nothing_here_closes_or_changes_an_order() {
    // The reader makes one request, a search, and its body names no payment
    // and no change of an order (`docs/DESIGN.md` section 5.6).
    let body = request().body();
    assert!(body.contains("\"query\""));
    for word in ["payment", "EXTERNAL", "CRYPTO", "amount_money"] {
        assert!(!body.contains(word), "{word}");
    }
}
