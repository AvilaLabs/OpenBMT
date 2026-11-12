//! PS-100 to PS-104: the Loyverse request and the reading of its reply, and
//! the Loyverse cases of PS-91, PS-92, PS-94 and PS-99.

use openbmt_pos::{
    Cursor, Id, OrderState, Page, PosOrder, PosTender, TenderKind, TenderState, MAX_PAGE_ORDERS,
};

use crate::loyverse::{
    payment_types_path, read_merchant, sales_path, Request, MERCHANT_PATH, PAGE_LIMIT,
    RECEIPTS_PATH,
};
use crate::{cursor_at, ReadRefused, RequestRefused};

use super::{settings, text, usd};

const RECEIPTS: &[u8] = include_bytes!("../../tests/fixtures/loyverse/receipts.json");
const PAGE1: &[u8] = include_bytes!("../../tests/fixtures/loyverse/receipts_page1.json");
const EMPTY: &[u8] = include_bytes!("../../tests/fixtures/loyverse/receipts_empty.json");
const REFERENCE: &[u8] = include_bytes!("../../tests/fixtures/loyverse/receipts_reference.json");
const TYPES: &[u8] = include_bytes!("../../tests/fixtures/loyverse/payment_types.json");
const MERCHANT: &[u8] = include_bytes!("../../tests/fixtures/loyverse/merchant.json");
const REFUNDS: &[u8] = include_bytes!("../../tests/fixtures/loyverse/refunds.json");
const SALES: &[u8] = include_bytes!("../../tests/fixtures/loyverse/sales.json");

/// 2026-11-10T13:56:40Z, before every receipt of the fixtures.
const SINCE: u64 = 1_794_319_000;

/// The store of the fixtures.
const STORE: &str = "42dc2cec-6f40-11ea-bde9-1269e7c5a22d";

fn request() -> Request {
    request_with(2, &cursor_at(SINCE).expect("a cursor"))
}

fn request_with(places: u8, cursor: &Cursor) -> Request {
    Request::new(&settings(), usd(), places, cursor).expect("a request")
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

fn read(body: &str) -> Result<Page, ReadRefused> {
    request().parse(body.as_bytes(), None, None)
}

/// A good sale of 100 (a dollar) with one Bitcoin payment named in the reply.
const SALE: &str = r#"{"receipt_number":"R-1","receipt_type":"SALE","created_at":"2026-11-10T14:00:00.000Z","updated_at":"2026-11-10T14:01:00.000Z","total_money":1,"store_id":"S1","payments":[{"payment_type_id":"PT-B","name":"Bitcoin","type":"OTHER","money_amount":1,"paid_at":"2026-11-10T14:00:30.000Z"}]}"#;

fn page_of(receipts: &[&str]) -> String {
    format!(r#"{{"receipts":[{}]}}"#, receipts.join(","))
}

/// The sale with `from` replaced by `to`, which must be in it.
fn changed(from: &str, to: &str) -> String {
    assert!(SALE.contains(from), "{from} is not in the sale");
    SALE.replacen(from, to, 1)
}

#[test]
fn ps_100_the_query_asks_for_receipts_updated_since_the_cursor_at_the_largest_page() {
    let q = request().path_and_query();
    assert_eq!(
        q,
        "/v1.0/receipts?updated_at_min=2026-11-10T13%3A56%3A40.000Z&limit=250"
    );
    assert!(q.starts_with(RECEIPTS_PATH) && q.ends_with(&format!("limit={PAGE_LIMIT}")));
    // The same text every time, and the start of the epoch is writable.
    assert_eq!(q, request().path_and_query());
    let start = request_with(2, &Cursor::start()).path_and_query();
    assert_eq!(
        start,
        "/v1.0/receipts?updated_at_min=1970-01-01T00%3A00%3A00.000Z&limit=250"
    );
    // The vendor's marker of the cursor goes on as `cursor`.
    let c = Cursor::new(b"1794319000|1794319260~7f0c2a4e-6b1d-4b8e".to_vec()).expect("a cursor");
    let q = request_with(2, &c).path_and_query();
    assert_eq!(
        q,
        "/v1.0/receipts?updated_at_min=2026-11-10T13%3A56%3A40.000Z&limit=250\
         &cursor=7f0c2a4e-6b1d-4b8e"
    );
}

#[test]
fn ps_100_a_request_needs_a_location_and_a_cursor_in_form() {
    let mut none = settings();
    none.locations.clear();
    assert_eq!(
        Request::new(&none, usd(), 2, &Cursor::start()),
        Err(RequestRefused::NoLocations)
    );
    for bad in [
        &b"5|"[..],
        b"5|x y",
        b"5|\x01",
        b"05",
        b"x",
        b"5|1 ",
        // The marker is a time, a tilde and Loyverse's cursor.
        b"5|abc",
        b"5|~abc",
        b"5|6~",
        b"5|06~abc",
        b"5|-6~abc",
        b"5|253402300800~abc",
    ] {
        let c = Cursor::new(bad.to_vec()).expect("short");
        assert_eq!(
            Request::new(&settings(), usd(), 2, &c),
            Err(RequestRefused::BadCursor),
            "{bad:?}"
        );
    }
}

#[test]
fn ps_100_the_paths_of_the_payment_types_the_merchant_and_the_sales_are_read_only_gets() {
    assert_eq!(
        payment_types_path(),
        "/v1.0/payment_types?show_deleted=true"
    );
    assert_eq!(MERCHANT_PATH, "/v1.0/merchant/");
    let two = [id("R-2001"), id("2-1008")];
    assert_eq!(
        sales_path(&two).as_deref(),
        Ok("/v1.0/receipts?receipt_numbers=R-2001%2C2-1008&limit=250")
    );
    assert_eq!(sales_path(&[]), Err(RequestRefused::BadLimit));
    let many: Vec<Id> = (0..=PAGE_LIMIT).map(|i| id(&format!("R-{i}"))).collect();
    assert_eq!(sales_path(&many), Err(RequestRefused::BadLimit));
    assert!(sales_path(&many[..PAGE_LIMIT]).is_ok());
    // A number with a space or a plus is written so that it stays whole.
    assert_eq!(
        sales_path(&[id("a b+c")]).as_deref(),
        Ok("/v1.0/receipts?receipt_numbers=a%20b%2Bc&limit=250")
    );
}

#[test]
fn ps_101_the_fixture_is_read_into_the_model() {
    let page = request().parse(RECEIPTS, Some(TYPES), None).expect("reads");
    assert!(!page.more);
    // Four sales; the refund is not an order. Oldest change first, and the
    // sale that was refunded changed last, at the time of the refund.
    let ids: Vec<&str> = page.orders.iter().map(|o| o.id.as_str()).collect();
    assert_eq!(ids, ["R-1002", "R-1003", "R-1004", "R-1001"]);
    // The last page: the next poll starts at the newest change.
    assert_eq!(page.next.as_bytes(), b"1794321600");

    assert_eq!(
        *order(&page, "R-1001"),
        PosOrder {
            id: id("R-1001"),
            location: text(STORE),
            state: OrderState::Closed,
            currency: usd(),
            total_minor: 3000,
            tenders: vec![
                PosTender {
                    id: id("PT-CASH"),
                    kind: TenderKind::Cash,
                    name: text("Cash"),
                    amount_minor: 1000,
                    tip_minor: 0,
                    state: TenderState::Paid,
                    at: 1_794_319_230,
                    reference: None,
                },
                PosTender {
                    id: id("PT-BTC"),
                    kind: TenderKind::Other,
                    name: text("Bitcoin"),
                    amount_minor: 2000,
                    tip_minor: 0,
                    state: TenderState::Refunded { amount_minor: 500 },
                    at: 1_794_319_231,
                    reference: None,
                },
            ],
            opened_at: 1_794_319_200,
            closed_at: None,
            updated_at: 1_794_321_600,
            reference: Some(text("btc K7M2QX")),
        }
    );
    // The order name is the reference when there is no note.
    assert_eq!(
        order(&page, "R-1004")
            .reference
            .as_ref()
            .map(|t| t.as_str()),
        Some("ORD-7")
    );
    for o in &page.orders {
        assert_eq!(o.validate(), Ok(()));
    }
}

#[test]
fn ps_101_a_cancelled_receipt_is_a_voided_order() {
    let page = request().parse(RECEIPTS, Some(TYPES), None).expect("reads");
    let o = order(&page, "R-1002");
    assert_eq!(o.state, OrderState::Voided);
    assert_eq!((o.opened_at, o.updated_at), (1_794_319_800, 1_794_319_920));
    // Its payment is read as it was.
    assert_eq!(tender(o, "PT-BTC").state, TenderState::Paid);
    // A `cancelled_at` that is not a time is refused.
    let bad = changed(
        "\"store_id\"",
        "\"cancelled_at\":\"yesterday\",\"store_id\"",
    );
    assert_eq!(
        read(&page_of(&[&bad])),
        Err(ReadRefused::BadTime("receipts[].cancelled_at"))
    );
}

#[test]
fn ps_101_a_page_with_a_cursor_has_more_and_the_next_request_takes_it() {
    let page = request().parse(PAGE1, None, None).expect("reads");
    assert!(page.more);
    // The marker keeps the newest change of this first page.
    assert_eq!(
        page.next.as_bytes(),
        b"1794319000|1794319260~7f0c2a4e-6b1d-4b8e-9f3a-2d5c1e8b9a70"
    );
    let again = request_with(2, &page.next);
    assert!(again
        .path_and_query()
        .ends_with("&cursor=7f0c2a4e-6b1d-4b8e-9f3a-2d5c1e8b9a70"));
    // The last page of the sync takes the newest change of the first page,
    // not its own: the receipts changed while the pages were read stay ahead
    // of the place.
    let last = again.parse(EMPTY, None, None).expect("reads");
    assert!(!last.more);
    assert_eq!(last.next.as_bytes(), b"1794319260");
    let older = again
        .parse(page_of(&[SALE]).as_bytes(), None, None)
        .expect("reads");
    assert_eq!(older.next.as_bytes(), b"1794319260");
    // A middle page keeps the marker's time and moves Loyverse's cursor.
    let middle = again
        .parse(br#"{"receipts":[],"cursor":"next-one"}"#, None, None)
        .expect("reads");
    assert!(middle.more);
    assert_eq!(middle.next.as_bytes(), b"1794319000|1794319260~next-one");
    // Nothing: the place stays, and there is no more.
    let none = request().parse(EMPTY, None, None).expect("reads");
    assert!(none.orders.is_empty() && !none.more);
    assert_eq!(none.next.as_bytes(), b"1794319000");
    // A cursor that is `null` is none; one that is empty or has a space or
    // is not text is refused.
    let with = |c: &str| format!(r#"{{"receipts":[],"cursor":{c}}}"#);
    assert!(!read(&with("null")).expect("reads").more);
    for bad in [r#""""#, r#""a b""#, "5"] {
        assert!(read(&with(bad)).is_err(), "{bad}");
    }
    assert_eq!(
        read(&with(r#""a b""#)),
        Err(ReadRefused::OutOfRange("cursor"))
    );
    assert_eq!(read(&with("5")), Err(ReadRefused::WrongType("cursor")));
}

#[test]
fn ps_94_a_page_of_more_than_250_receipts_is_refused_whole_and_250_are_read() {
    let many = |n: usize| {
        let each: Vec<String> = (0..n)
            .map(|i| SALE.replacen("R-1", &format!("R-{i}"), 1))
            .collect();
        page_of(&each.iter().map(String::as_str).collect::<Vec<_>>())
    };
    assert_eq!(read(&many(250)).expect("reads").orders.len(), 250);
    assert_eq!(read(&many(251)), Err(ReadRefused::TooManyOrders));
    const { assert!(PAGE_LIMIT <= MAX_PAGE_ORDERS) };
    // More than 16 payments on a receipt, or two with one id: refused.
    let payment = r#"{"payment_type_id":"PT-B","name":"Bitcoin","money_amount":0,"paid_at":"2026-11-10T14:00:30Z"}"#;
    let mut sixteen = vec![payment; 16].join(",");
    let ok = SALE.replacen(
        &SALE[SALE.find("\"payments\":[").expect("payments") + 12..SALE.len() - 2],
        &sixteen,
        1,
    );
    assert!(read(&page_of(&[&ok])).is_ok());
    sixteen.push(',');
    sixteen.push_str(payment);
    let over = ok.replacen(
        &ok[ok.find("\"payments\":[").expect("p") + 12..ok.len() - 2],
        &sixteen,
        1,
    );
    assert_eq!(read(&page_of(&[&over])), Err(ReadRefused::TooManyTenders));
}

#[test]
fn ps_91_an_unknown_receipt_type_matters_only_to_a_receipt_that_may_carry_bitcoin() {
    // With a Bitcoin payment: refused whole, though the other receipt is good.
    let with_bitcoin = changed("\"SALE\"", "\"EXCHANGE\"");
    assert_eq!(
        read(&page_of(&[SALE, &with_bitcoin])),
        Err(ReadRefused::UnknownValue("receipts[].receipt_type"))
    );
    // A payment with no name and no payment type to name it may be Bitcoin.
    let unnamed = with_bitcoin.replace("\"name\":\"Bitcoin\",", "");
    assert!(read(&page_of(&[&unnamed])).is_err());
    // With only a payment of another name: left out, and the page goes on.
    let cash = with_bitcoin.replace("\"name\":\"Bitcoin\"", "\"name\":\"Cash\"");
    let page = read(&page_of(&[SALE, &cash])).expect("reads");
    assert_eq!(page.orders.len(), 1);
    // No payments at all: left out.
    let none = changed("\"SALE\"", "\"EXCHANGE\"").replace(
        &SALE[SALE.find("\"payments\"").expect("payments")..SALE.len() - 1],
        "\"payments\":[]",
    );
    assert!(read(&page_of(&[&none])).expect("reads").orders.is_empty());
    // A type that is not text is of the wrong type.
    assert_eq!(
        read(&page_of(&[&changed("\"SALE\"", "5")])),
        Err(ReadRefused::WrongType("receipts[].receipt_type"))
    );
}

#[test]
fn ps_91_a_field_that_is_missing_null_or_of_the_wrong_type_is_a_refusal() {
    for (from, to, want) in [
        (
            "\"receipt_number\":\"R-1\",",
            "",
            ReadRefused::Missing("receipts[].receipt_number"),
        ),
        (
            "\"receipt_number\":\"R-1\"",
            "\"receipt_number\":null",
            ReadRefused::Missing("receipts[].receipt_number"),
        ),
        (
            "\"receipt_number\":\"R-1\"",
            "\"receipt_number\":1",
            ReadRefused::WrongType("receipts[].receipt_number"),
        ),
        (
            "\"receipt_type\":\"SALE\",",
            "",
            ReadRefused::Missing("receipts[].receipt_type"),
        ),
        (
            "\"store_id\":\"S1\",",
            "",
            ReadRefused::Missing("receipts[].store_id"),
        ),
        (
            "\"store_id\":\"S1\"",
            "\"store_id\":[]",
            ReadRefused::WrongType("receipts[].store_id"),
        ),
        (
            "\"total_money\":1,",
            "",
            ReadRefused::Missing("receipts[].total_money"),
        ),
        (
            "\"total_money\":1",
            "\"total_money\":\"1\"",
            ReadRefused::WrongType("receipts[].total_money"),
        ),
        (
            "\"created_at\":\"2026-11-10T14:00:00.000Z\",",
            "",
            ReadRefused::Missing("receipts[].created_at"),
        ),
        (
            "\"updated_at\":\"2026-11-10T14:01:00.000Z\",",
            "",
            ReadRefused::Missing("receipts[].updated_at"),
        ),
        (
            "\"updated_at\":\"2026-11-10T14:01:00.000Z\"",
            "\"updated_at\":7",
            ReadRefused::WrongType("receipts[].updated_at"),
        ),
        (
            "\"payment_type_id\":\"PT-B\",",
            "",
            ReadRefused::Missing("payments[].payment_type_id"),
        ),
        (
            "\"money_amount\":1,",
            "",
            ReadRefused::Missing("payments[].money_amount"),
        ),
        (
            "\"paid_at\":\"2026-11-10T14:00:30.000Z\"",
            "\"paid_at\":null",
            ReadRefused::Missing("payments[].paid_at"),
        ),
        (
            "\"name\":\"Bitcoin\"",
            "\"name\":5",
            ReadRefused::WrongType("payments[].name"),
        ),
        (
            "\"type\":\"OTHER\"",
            "\"type\":5",
            ReadRefused::WrongType("payments[].type"),
        ),
        (
            "\"payments\":[",
            "\"payments\":{},\"x\":[",
            ReadRefused::WrongType("receipts[].payments"),
        ),
        (
            "\"S1\"",
            "\"S1\",\"note\":5",
            ReadRefused::WrongType("receipts[].note"),
        ),
        (
            "\"S1\"",
            "\"S1\",\"order\":false",
            ReadRefused::WrongType("receipts[].order"),
        ),
        (
            "\"S1\"",
            "\"S1\",\"refund_for\":5",
            ReadRefused::WrongType("receipts[].refund_for"),
        ),
        (
            "\"S1\"",
            "\"S1\",\"receipt_date\":\"later\"",
            ReadRefused::BadTime("receipts[].receipt_date"),
        ),
    ] {
        assert_eq!(read(&page_of(&[&changed(from, to)])), Err(want), "{to}");
    }
    // The reply itself: no `receipts`, or one that is not a list or an object.
    assert_eq!(read("{}"), Err(ReadRefused::Missing("receipts")));
    assert_eq!(
        read(r#"{"receipts":{}}"#),
        Err(ReadRefused::WrongType("receipts"))
    );
    assert_eq!(read("[]"), Err(ReadRefused::WrongType("reply")));
    assert_eq!(
        read(&page_of(&["5"])),
        Err(ReadRefused::WrongType("receipts[]"))
    );
    // A service error is a refusal even with receipts beside it.
    let err =
        format!(r#"{{"receipts":[{SALE}],"errors":[{{"code":"RATE_LIMITED","details":"x"}}]}}"#);
    assert_eq!(read(&err), Err(ReadRefused::ServiceError));
    assert!(read(&format!(r#"{{"receipts":[{SALE}],"errors":[]}}"#)).is_ok());
    // Fields the reader does not need are ignored whatever they hold.
    let extra = changed(
        "\"S1\"",
        "\"S1\",\"points_balance\":\"x\",\"new_field\":{\"a\":[1]}",
    );
    assert!(read(&page_of(&[&extra])).is_ok());
}

#[test]
fn ps_92_a_time_is_strict_rfc_3339_and_dropped_to_seconds() {
    // The reference's own forms: milliseconds, and none.
    let page = read(&page_of(&[&changed(
        "\"2026-11-10T14:00:00.000Z\"",
        "\"2026-11-10T14:00:00Z\"",
    )]))
    .expect("reads");
    assert_eq!(page.orders[0].opened_at, 1_794_319_200);
    let page = read(&page_of(&[&changed(
        "\"2026-11-10T14:00:00.000Z\"",
        "\"2026-11-10T14:00:00.999Z\"",
    )]))
    .expect("reads");
    assert_eq!(page.orders[0].opened_at, 1_794_319_200);
    for bad in [
        "2026-11-10 14:00:00Z",
        "2026-11-10T14:00:00",
        "2026-11-10t14:00:00z",
        "2026-13-10T14:00:00Z",
        "2026-02-30T14:00:00Z",
        "2026-11-10T14:00:60Z",
        "1969-12-31T23:59:59Z",
        "10000-01-01T00:00:00Z",
        "1794319200",
        "",
    ] {
        assert_eq!(
            read(&page_of(&[&changed(
                "\"2026-11-10T14:00:00.000Z\"",
                &format!("\"{bad}\"")
            )])),
            Err(ReadRefused::BadTime("receipts[].created_at")),
            "{bad}"
        );
    }
    assert_eq!(
        read(&page_of(&[&changed(
            "\"2026-11-10T14:00:30.000Z\"",
            "\"soon\""
        )])),
        Err(ReadRefused::BadTime("payments[].paid_at"))
    );
}

#[test]
fn ps_104_an_amount_is_whole_units_scaled_by_the_decimal_places_of_the_account() {
    // The sale of one unit: 100 minor units with two places, 1 with none and
    // 1000 with three.
    let body = page_of(&[SALE]);
    for (places, minor) in [(0u8, 1u64), (2, 100), (3, 1000)] {
        let page = request_with(places, &cursor_at(SINCE).expect("a cursor"))
            .parse(body.as_bytes(), None, None)
            .expect("reads");
        let o = &page.orders[0];
        assert_eq!(o.total_minor, minor);
        assert_eq!(o.tenders[0].amount_minor, minor);
    }
    // A negative amount and one above the largest are out of range, and one
    // that scales past it is too.
    let c = cursor_at(SINCE).expect("a cursor");
    let at = |places: u8, total: &str| {
        request_with(places, &c).parse(
            page_of(&[&changed(
                "\"total_money\":1",
                &format!("\"total_money\":{total}"),
            )])
            .as_bytes(),
            None,
            None,
        )
    };
    assert_eq!(
        at(2, "-1"),
        Err(ReadRefused::OutOfRange("receipts[].total_money"))
    );
    assert_eq!(
        at(0, "9223372036854775807").map(|p| p.orders[0].total_minor),
        Ok(i64::MAX as u64)
    );
    assert_eq!(
        at(0, "9223372036854775808"),
        Err(ReadRefused::OutOfRange("receipts[].total_money"))
    );
    assert_eq!(
        at(2, "92233720368547759"),
        Err(ReadRefused::OutOfRange("receipts[].total_money"))
    );
    assert_eq!(
        at(19, "1"),
        Err(ReadRefused::OutOfRange("receipts[].total_money"))
    );
    assert_eq!(at(2, "0").map(|p| p.orders[0].total_minor), Ok(0));
    // An amount with a fraction or an exponent cannot be read exactly (the
    // JSON reader keeps an integer only), so it is refused, never rounded.
    for bad in ["17.52", "1e2", "1.0", "-0", "null", "true", "[1]"] {
        let want = if bad == "null" {
            ReadRefused::Missing("receipts[].total_money")
        } else {
            ReadRefused::WrongType("receipts[].total_money")
        };
        assert_eq!(at(2, bad), Err(want), "{bad}");
    }
}

#[test]
fn ps_104_the_reference_example_is_refused_while_its_amounts_have_fractions() {
    // The list example of the reference, whose amounts are 17.52 and 5.84.
    assert_eq!(
        request().parse(REFERENCE, None, None),
        Err(ReadRefused::WrongType("receipts[].total_money"))
    );
}

#[test]
fn ps_104_the_merchant_gives_the_currency_and_its_decimal_places() {
    assert_eq!(read_merchant(MERCHANT), Ok((usd(), 2)));
    let with = |c: &str| format!(r#"{{"currency":{c}}}"#);
    assert_eq!(
        read_merchant(with(r#"{"code":"JPY","decimal_places":0}"#).as_bytes())
            .map(|(c, p)| (c.code(), p)),
        Ok((*b"JPY", 0))
    );
    for (body, want) in [
        ("{}", ReadRefused::Missing("currency")),
        (r#"{"currency":[]}"#, ReadRefused::WrongType("currency")),
        (
            r#"{"currency":{"decimal_places":2}}"#,
            ReadRefused::Missing("currency.code"),
        ),
        (
            r#"{"currency":{"code":"usd","decimal_places":2}}"#,
            ReadRefused::BadCurrency("currency.code"),
        ),
        (
            r#"{"currency":{"code":"USD"}}"#,
            ReadRefused::Missing("currency.decimal_places"),
        ),
        (
            r#"{"currency":{"code":"USD","decimal_places":-1}}"#,
            ReadRefused::OutOfRange("currency.decimal_places"),
        ),
        (
            r#"{"currency":{"code":"USD","decimal_places":10}}"#,
            ReadRefused::OutOfRange("currency.decimal_places"),
        ),
        (
            r#"{"currency":{"code":"USD","decimal_places":2.5}}"#,
            ReadRefused::WrongType("currency.decimal_places"),
        ),
        (
            r#"{"currency":{"code":"USD","decimal_places":2},"errors":[{}]}"#,
            ReadRefused::ServiceError,
        ),
    ] {
        assert_eq!(read_merchant(body.as_bytes()), Err(want), "{body}");
    }
}

#[test]
fn ps_102_a_payment_is_named_by_its_own_name_else_by_its_payment_type() {
    let page = request().parse(RECEIPTS, Some(TYPES), None).expect("reads");
    // R-1004's Bitcoin payment has no name: its payment type is named
    // "Bitcoin". Its other payment is of a type the reader does not know.
    let o = order(&page, "R-1004");
    let b = tender(o, "PT-BTC2");
    assert_eq!((b.name.as_str(), b.kind), ("Bitcoin", TenderKind::Other));
    assert_eq!(b.amount_minor, 2500);
    let g = tender(o, "PT-NEW");
    assert_eq!(
        (g.name.as_str(), g.kind),
        ("Gift voucher", TenderKind::Other)
    );
    // Without the payment types, a payment with no name is refused: it may be
    // a Bitcoin payment.
    assert_eq!(
        request().parse(RECEIPTS, None, None),
        Err(ReadRefused::Missing("payments[].name"))
    );
    // A payment type that is not in the list names nothing either.
    let other = br#"{"payment_types":[{"id":"PT-X","name":"Bitcoin"}]}"#;
    assert_eq!(
        request().parse(RECEIPTS, Some(other), None),
        Err(ReadRefused::Missing("payments[].name"))
    );
    // The payment's own name wins over its type's.
    let renamed = br#"{"payment_types":[{"id":"PT-B","name":"Cash","type":"CASH"}]}"#;
    let page = request()
        .parse(page_of(&[SALE]).as_bytes(), Some(renamed), None)
        .expect("reads");
    assert_eq!(page.orders[0].tenders[0].name.as_str(), "Bitcoin");
    // Which tenders are Bitcoin stays the settings' name against it (PS-10):
    // the page holds no judgment of its own, a Cash payment named like it
    // keeps the kind Cash.
    let cash = changed("\"type\":\"OTHER\"", "\"type\":\"CASH\"");
    let page = read(&page_of(&[&cash])).expect("reads");
    assert_eq!(page.orders[0].tenders[0].kind, TenderKind::Cash);
    assert_eq!(page.orders[0].tenders[0].name.as_str(), "Bitcoin");
}

#[test]
fn ps_102_the_payment_types_reply_is_read_strictly() {
    let body = page_of(&[&changed("\"name\":\"Bitcoin\",", "")]);
    let read_with = |types: &str| request().parse(body.as_bytes(), Some(types.as_bytes()), None);
    assert!(read_with(r#"{"payment_types":[{"id":"PT-B","name":"Bitcoin"}]}"#).is_ok());
    for (types, want) in [
        ("{}", ReadRefused::Missing("payment_types")),
        (
            r#"{"payment_types":{}}"#,
            ReadRefused::WrongType("payment_types"),
        ),
        (
            r#"{"payment_types":[{"name":"Bitcoin"}]}"#,
            ReadRefused::Missing("payment_types[].id"),
        ),
        (
            r#"{"payment_types":[{"id":"PT-B"}]}"#,
            ReadRefused::Missing("payment_types[].name"),
        ),
        (
            r#"{"payment_types":[{"id":"PT-B","name":"B","type":5}]}"#,
            ReadRefused::WrongType("payment_types[].type"),
        ),
        (
            r#"{"payment_types":[],"errors":[{"code":"X"}]}"#,
            ReadRefused::ServiceError,
        ),
        ("not json", ReadRefused::Malformed),
    ] {
        assert_eq!(read_with(types), Err(want), "{types}");
    }
    // A name of more than 64 bytes is cut; an empty one is refused.
    let long = format!(
        r#"{{"payment_types":[{{"id":"PT-B","name":"{}"}}]}}"#,
        "a".repeat(70)
    );
    assert_eq!(
        read_with(&long).map(|p| p.orders[0].tenders[0].name.as_str().len()),
        Ok(64)
    );
    assert_eq!(
        read_with(r#"{"payment_types":[{"id":"PT-B","name":""}]}"#),
        Err(ReadRefused::BadText("payments[].name"))
    );
}

#[test]
fn ps_102_two_bitcoin_payments_on_a_receipt_and_a_bitcoin_and_a_cash_payment_are_read() {
    let page = request().parse(RECEIPTS, Some(TYPES), None).expect("reads");
    // Two payments of one type: the second takes the id with `#2`.
    let o = order(&page, "R-1003");
    assert_eq!(o.tenders.len(), 2);
    let (a, b) = (tender(o, "PT-BTC"), tender(o, "PT-BTC#2"));
    assert_eq!((a.amount_minor, a.at), (1000, 1_794_320_430));
    assert_eq!((b.amount_minor, b.at), (500, 1_794_320_440));
    // The name is kept as the shop typed it; PS-10 compares it.
    assert_eq!((a.name.as_str(), b.name.as_str()), ("Bitcoin", " bitcoin "));
    // A Bitcoin payment and a cash payment: each is its own tender.
    let o = order(&page, "R-1001");
    assert_eq!(tender(o, "PT-CASH").kind, TenderKind::Cash);
    assert_eq!(tender(o, "PT-BTC").kind, TenderKind::Other);
    // A third makes `#3`; a type id so long that the id is too long is refused.
    let three = {
        let p = r#"{"payment_type_id":"PT-B","name":"Bitcoin","money_amount":0,"paid_at":"2026-11-10T14:00:30Z"}"#;
        changed("\"payments\":[", &format!("\"payments\":[{p},{p},"))
    };
    let page = read(&page_of(&[&three])).expect("reads");
    assert_eq!(page.orders[0].tenders.len(), 3);
    assert!(page.orders[0].tender(&id("PT-B#3")).is_some());
    let long = changed("\"PT-B\"", &format!("\"{}\"", "t".repeat(64)));
    assert_eq!(read(&page_of(&[&long])).map(|p| p.orders.len()), Ok(1));
    let longer = changed("\"PT-B\"", &format!("\"{}\"", "t".repeat(65)));
    assert_eq!(
        read(&page_of(&[&longer])),
        Err(ReadRefused::BadId("payments[].payment_type_id"))
    );
}

#[test]
fn ps_102_a_payment_of_a_type_the_reader_does_not_know_is_another_kind_never_bitcoin() {
    let odd = changed("\"type\":\"OTHER\"", "\"type\":\"SOMETHING_NEW\"")
        .replace("\"name\":\"Bitcoin\"", "\"name\":\"Voucher\"");
    let page = read(&page_of(&[&odd])).expect("reads");
    let t = &page.orders[0].tenders[0];
    assert_eq!((t.kind, t.name.as_str()), (TenderKind::Other, "Voucher"));
    // A type left out reads as another kind too, and a card type as a card.
    let none = changed("\"type\":\"OTHER\",", "");
    assert_eq!(
        read(&page_of(&[&none])).expect("reads").orders[0].tenders[0].kind,
        TenderKind::Other
    );
    let card = changed("\"type\":\"OTHER\"", "\"type\":\"NONINTEGRATEDCARD\"");
    assert_eq!(
        read(&page_of(&[&card])).expect("reads").orders[0].tenders[0].kind,
        TenderKind::Card
    );
}

#[test]
fn ps_93_a_name_is_cut_and_an_id_or_a_store_is_refused_never_cut() {
    let long = changed(
        "\"name\":\"Bitcoin\"",
        &format!("\"name\":\"{}\"", "b".repeat(70)),
    );
    let page = read(&page_of(&[&long])).expect("reads");
    assert_eq!(page.orders[0].tenders[0].name.as_str(), "b".repeat(64));
    assert_eq!(
        read(&page_of(&[&changed("\"R-1\"", "\"\"")])),
        Err(ReadRefused::BadId("receipts[].receipt_number"))
    );
    assert_eq!(
        read(&page_of(&[&changed(
            "\"R-1\"",
            &format!("\"{}\"", "r".repeat(65))
        )])),
        Err(ReadRefused::BadId("receipts[].receipt_number"))
    );
    assert_eq!(
        read(&page_of(&[&changed(
            "\"S1\"",
            &format!("\"{}\"", "s".repeat(65))
        )])),
        Err(ReadRefused::BadText("receipts[].store_id"))
    );
    assert_eq!(
        read(&page_of(&[&changed(
            "\"name\":\"Bitcoin\"",
            "\"name\":\"\""
        )])),
        Err(ReadRefused::BadText("payments[].name"))
    );
    // A note typed by a person has its control characters turned to spaces.
    let noted = changed("\"S1\"", "\"S1\",\"note\":\"btc\\nK7M2QX\\t\"");
    let page = read(&page_of(&[&noted])).expect("reads");
    assert_eq!(
        page.orders[0].reference.as_ref().map(|t| t.as_str()),
        Some("btc K7M2QX")
    );
}

/// A refund of `amount` units of the payment type `PT-B`, of the sale `R-1`.
fn refund(amount: u64) -> String {
    format!(
        r#"{{"receipt_number":"R-9","receipt_type":"REFUND","refund_for":"R-1","created_at":"2026-11-10T14:40:00.000Z","updated_at":"2026-11-10T14:40:00.000Z","total_money":{amount},"store_id":"S1","payments":[{{"payment_type_id":"PT-B","name":"Bitcoin","type":"OTHER","money_amount":{amount},"paid_at":"2026-11-10T14:40:00.000Z"}}]}}"#
    )
}

#[test]
fn ps_103_a_refund_receipt_is_a_refund_of_the_sale_it_names() {
    // The sale is 100 minor units; refunds of 100 and 200 add up on its tender.
    let sale = changed("\"total_money\":1,", "\"total_money\":4,")
        .replace("\"money_amount\":1,", "\"money_amount\":4,");
    let page = read(&page_of(&[&sale, &refund(1)])).expect("reads");
    assert_eq!(page.orders.len(), 1, "a refund is not an order");
    let o = &page.orders[0];
    assert_eq!(o.id, id("R-1"));
    assert_eq!(
        tender(o, "PT-B").state,
        TenderState::Refunded { amount_minor: 100 }
    );
    // The order changed when it was refunded.
    assert_eq!(o.updated_at, 1_794_321_600);
    let r2 = refund(2).replace("R-9", "R-10");
    let page = read(&page_of(&[&sale, &refund(1), &r2])).expect("reads");
    assert_eq!(
        tender(&page.orders[0], "PT-B").state,
        TenderState::Refunded { amount_minor: 300 }
    );
    // The same refund whatever the order the receipts come in.
    let page2 = read(&page_of(&[&r2, &refund(1), &sale])).expect("reads");
    assert_eq!(page, page2);
    // A refund older than the sale's last change leaves that change alone.
    let old = refund(1).replace(
        "2026-11-10T14:40:00.000Z\",\"updated_at\":\"2026-11-10T14:40:00.000Z",
        "2026-11-10T13:00:00.000Z\",\"updated_at\":\"2026-11-10T13:00:00.000Z",
    );
    let page = read(&page_of(&[&sale, &old])).expect("reads");
    assert_eq!(page.orders[0].updated_at, 1_794_319_260);
    // Refunding a cancelled sale is read too: the sale stays voided.
    let cancelled = sale.replace(
        "\"store_id\"",
        "\"cancelled_at\":\"2026-11-10T14:02:00Z\",\"store_id\"",
    );
    let page = read(&page_of(&[&cancelled, &refund(1)])).expect("reads");
    assert_eq!(page.orders[0].state, OrderState::Voided);
}

#[test]
fn ps_103_a_refund_beyond_its_tender_or_for_nothing_is_refused() {
    let sale = changed("\"total_money\":1,", "\"total_money\":4,")
        .replace("\"money_amount\":1,", "\"money_amount\":4,");
    // 400 minor units refunded: all of it. One more is too much.
    assert!(read(&page_of(&[&sale, &refund(4)])).is_ok());
    assert_eq!(
        read(&page_of(&[
            &sale,
            &refund(3),
            &refund(2).replace("R-9", "R-8")
        ])),
        Err(ReadRefused::RefundTooLarge)
    );
    assert_eq!(
        read(&page_of(&[&sale, &refund(5)])),
        Err(ReadRefused::RefundTooLarge)
    );
    // A refund of nothing is refused.
    assert_eq!(
        read(&page_of(&[&sale, &refund(0)])),
        Err(ReadRefused::OutOfRange("payments[].money_amount"))
    );
    // A refund that names no sale is a field the reader needs.
    assert_eq!(
        read(&page_of(&[
            &sale,
            &refund(1).replace("\"refund_for\":\"R-1\",", "")
        ])),
        Err(ReadRefused::Missing("receipts[].refund_for"))
    );
    // A refund of a payment type the sale does not hold: refused for a Bitcoin
    // payment, ignored for another.
    let other_type = refund(1).replace("\"PT-B\"", "\"PT-Z\"");
    assert_eq!(
        read(&page_of(&[&sale, &other_type])),
        Err(ReadRefused::RefundWithoutTender)
    );
    let cash = other_type.replace("\"Bitcoin\"", "\"Cash\"");
    let page = read(&page_of(&[&sale, &cash])).expect("reads");
    assert_eq!(tender(&page.orders[0], "PT-B").state, TenderState::Paid);
    // A refund that was cancelled is not a refund.
    let cancelled = refund(1).replace(
        "\"store_id\"",
        "\"cancelled_at\":\"2026-11-10T14:41:00Z\",\"store_id\"",
    );
    let page = read(&page_of(&[&sale, &cancelled])).expect("reads");
    assert_eq!(tender(&page.orders[0], "PT-B").state, TenderState::Paid);
}

#[test]
fn ps_103_a_refund_of_a_sale_that_is_not_in_the_page_finds_it_among_the_sales_asked_for() {
    let r = request();
    // The reply holds only the refund; the sale it names is to be asked for.
    let named = r.refunded_sales(REFUNDS).expect("reads");
    assert_eq!(named, vec![id("R-2001")]);
    assert_eq!(
        sales_path(&named).as_deref(),
        Ok("/v1.0/receipts?receipt_numbers=R-2001&limit=250")
    );
    // Without it, the refund of a Bitcoin payment is refused whole.
    assert_eq!(
        r.parse(REFUNDS, Some(TYPES), None),
        Err(ReadRefused::RefundWithoutTender)
    );
    // With it, the sale comes into the page, refunded; the sale that no
    // refund names does not.
    let page = r.parse(REFUNDS, Some(TYPES), Some(SALES)).expect("reads");
    assert_eq!(page.orders.len(), 1);
    let o = &page.orders[0];
    assert_eq!(o.id, id("R-2001"));
    assert_eq!(
        tender(o, "PT-BTC").state,
        TenderState::Refunded { amount_minor: 800 }
    );
    assert_eq!(tender(o, "PT-CASH").state, TenderState::Paid);
    assert_eq!(o.updated_at, 1_794_322_800);
    assert_eq!(page.next.as_bytes(), b"1794322800");
    // A refund of cash only, whose sale is nowhere, is ignored.
    let cash = String::from_utf8(REFUNDS.to_vec())
        .expect("text")
        .replace("\"PT-BTC\"", "\"PT-CASH\"")
        .replace("\"Bitcoin\"", "\"Cash\"");
    let page = r.parse(cash.as_bytes(), Some(TYPES), None).expect("reads");
    assert!(page.orders.is_empty());
    // A sales reply is read as strictly as the page.
    assert_eq!(
        r.parse(REFUNDS, Some(TYPES), Some(&b"{}"[..])),
        Err(ReadRefused::Missing("receipts"))
    );
    // The numbers of a page whose sales are all in it: none to ask for.
    assert_eq!(r.refunded_sales(RECEIPTS), Ok(vec![]));
}

#[test]
fn ps_99_the_loyverse_reader_gives_the_same_answer_every_time() {
    let r = request();
    assert_eq!(
        r.parse(RECEIPTS, Some(TYPES), None),
        r.parse(RECEIPTS, Some(TYPES), None)
    );
    assert_eq!(
        r.parse(REFERENCE, None, None),
        r.parse(REFERENCE, None, None)
    );
    assert_eq!(r.path_and_query(), r.clone().path_and_query());
    assert_eq!(
        r.parse(REFUNDS, Some(TYPES), Some(SALES)),
        r.parse(REFUNDS, Some(TYPES), Some(SALES))
    );
}

#[test]
fn ps_90_a_loyverse_reply_past_the_bounds_or_not_json_is_refused() {
    let big = vec![b' '; 8 * 1024 * 1024 + 1];
    assert_eq!(request().parse(&big, None, None), Err(ReadRefused::TooLong));
    let deep = format!(
        r#"{{"receipts":[],"x":{}{}}}"#,
        "[".repeat(64),
        "]".repeat(64)
    );
    assert_eq!(read(&deep), Err(ReadRefused::TooDeep));
    assert_eq!(read(""), Err(ReadRefused::Malformed));
    assert_eq!(
        read(r#"{"receipts":[]} x"#),
        Err(ReadRefused::TrailingBytes)
    );
    assert_eq!(
        read(r#"{"receipts":[],"receipts":[]}"#),
        Err(ReadRefused::DuplicateKey)
    );
    assert_eq!(
        request().parse(b"{\"receipts\":[],\"x\":\"\xff\"}", None, None),
        Err(ReadRefused::NotUtf8)
    );
}
