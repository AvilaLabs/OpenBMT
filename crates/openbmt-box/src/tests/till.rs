//! The till listener (BX-70 to BX-79): the reader, the key, the sales, the
//! counters' notices and the whole of it over real sockets on the loopback.
//! BX-70 and BX-71 as a process are tested in `tests/till.rs`.

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::sync::Arc;
use std::time::{Duration, Instant};

use openbmt_core::identity::{DeviceId, Identity};
use openbmt_core::settings::DeviceRole;
use openbmt_link::{open, Kind, Role};
use openbmt_pos::{NoticeDraft, NoticeState, PaidNotice, SaleRequest, TillMessage};

use super::kit::*;
use super::pos_kit::{roles, usd, TestConnector};
use super::runner::{
    connect, eventually, mailbox, pair_ok, put_notice, start, start_in, TestClock,
};
use crate::files::{BoxFiles, DirFiles, MemFiles};
use crate::say::{
    all_words, render, till_key_lines, till_samples, TillAnswer, TILL_OPENED, TILL_PAID,
    TILL_WAITING,
};
use crate::till::{
    clean_reference, counter_code, find_counter, hex, make_till_key, parse_amount, request_id_ok,
    till_key_hash, till_key_matches, NewSale, SaleState, TillRefusal, TillTable, KEEP_SECS,
    MAX_WAITING, OPEN_SECS, REQUEST_SECS, TILL_KEY_FILE,
};
use crate::till_listener::{
    parse_object, parse_request, refusal_response, reject_response, sale_body, CreateBody, Reject,
    Request, Route, TillBackend, TillConfig, TillListener, MAX_REQUEST,
};
use crate::BoxRunner;

// ---- the reader (BX-70, BX-75) -----------------------------------------

fn post_sales(extra: &str, body: &str) -> Vec<u8> {
    format!(
        "POST /till/v1/sales HTTP/1.1\r\nHost: box\r\n{extra}\
         Content-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    )
    .into_bytes()
}

fn get_sale(path: &str, extra: &str) -> Vec<u8> {
    format!("GET {path} HTTP/1.1\r\nHost: box\r\n{extra}\r\n").into_bytes()
}

fn read(bytes: &[u8]) -> Result<Option<Request>, Reject> {
    parse_request(bytes)
}

const SALE: &str = "00112233445566778899aabbccddeeff";

#[test]
fn bx_70_the_reader_reads_the_three_routes() {
    let key = "k".repeat(64);
    let auth = format!("Authorization: Bearer {key}\r\n");
    let r = read(&post_sales(&auth, "{\"a\":\"b\"}")).unwrap().unwrap();
    assert_eq!(r.route, Route::Create);
    assert_eq!(r.key.as_deref(), Some(key.as_str()));
    assert_eq!(r.body, b"{\"a\":\"b\"}");
    let want = [0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77];
    let r = read(&get_sale(&format!("/till/v1/sales/{SALE}"), &auth))
        .unwrap()
        .unwrap();
    match r.route {
        Route::Read(Some(id)) => assert_eq!(id.get(..8), Some(&want[..])),
        other => panic!("{other:?}"),
    }
    let cancel = format!("POST /till/v1/sales/{SALE}/cancel HTTP/1.1\r\nHost: box\r\n{auth}\r\n")
        .into_bytes();
    let r = read(&cancel).unwrap().unwrap();
    assert!(matches!(r.route, Route::Cancel(Some(_))));
    // An id that is not 32 lowercase hex still reaches the route, as no
    // sale.
    let r = read(&get_sale("/till/v1/sales/nope", "")).unwrap().unwrap();
    assert_eq!(r.route, Route::Read(None));
    let r = read(&get_sale(
        &format!("/till/v1/sales/{}", SALE.to_uppercase()),
        "",
    ))
    .unwrap()
    .unwrap();
    assert_eq!(r.route, Route::Read(None));
    // Not whole yet: more is needed.
    assert_eq!(read(b"GET /till/v1/sales/x HTTP/1.1\r\nHo"), Ok(None));
    let whole = post_sales("", "{\"a\":\"b\"}");
    assert_eq!(read(&whole[..whole.len() - 1]), Ok(None));
    // A header that is not a bearer token is a key that is no key.
    let r = read(&get_sale(
        "/till/v1/sales/x",
        "Authorization: Basic abc\r\n",
    ))
    .unwrap()
    .unwrap();
    assert_eq!(r.key.as_deref(), Some(""));
}

#[test]
fn bx_70_the_reader_holds_a_request_to_four_kib() {
    // A body that would pass the limit is refused from its length alone.
    let long = post_sales("", &"x".repeat(MAX_REQUEST));
    assert_eq!(read(&long), Err(Reject::TooLarge));
    let head = "POST /till/v1/sales HTTP/1.1\r\nHost: box\r\nContent-Type: application/json\r\n\
                Content-Length: 3000\r\n\r\n";
    assert_eq!(read(head.as_bytes()), Ok(None));
    // A body that fits, up to the last byte of the limit, and one more.
    let room = (1..MAX_REQUEST)
        .rev()
        .find(|n| post_sales("", &"x".repeat(*n)).len() <= MAX_REQUEST)
        .unwrap();
    let exact = post_sales("", &"x".repeat(room));
    assert_eq!(exact.len(), MAX_REQUEST);
    assert!(read(&exact).unwrap().is_some());
    assert_eq!(
        read(&post_sales("", &"x".repeat(room + 1))),
        Err(Reject::TooLarge)
    );
    // A head that does not end inside the limit.
    let mut big = b"GET /till/v1/sales/x HTTP/1.1\r\nHost: box\r\n".to_vec();
    while big.len() <= MAX_REQUEST {
        big.extend_from_slice(b"X-Pad: aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\r\n");
    }
    assert_eq!(read(&big), Err(Reject::TooLarge));
    let mut done = big.clone();
    done.extend_from_slice(b"\r\n");
    assert_eq!(read(&done), Err(Reject::TooLarge));
    // And the response for it.
    let answer = String::from_utf8(reject_response(Reject::TooLarge)).unwrap();
    assert!(answer.starts_with("HTTP/1.1 413 "), "{answer}");
}

#[test]
fn bx_70_the_reader_refuses_what_is_not_a_clean_request() {
    let refused = |bytes: &[u8]| read(bytes).unwrap_err();
    // Not HTTP/1.1, no method, a bare line feed, a control character, a
    // folded header, a body in a framing the reader does not read.
    assert_eq!(
        refused(b"GET /till/v1/sales/x HTTP/1.0\r\n\r\n"),
        Reject::BadRequest
    );
    assert_eq!(refused(b"GET /till/v1/sales/x\r\n\r\n"), Reject::BadRequest);
    assert_eq!(
        refused(b"GET /till/v1/sales/x HTTP/1.1\nHost: b\r\n\r\n"),
        Reject::BadRequest
    );
    assert_eq!(
        refused(b"GET /till/v1/sales/x HTTP/1.1\r\nHost: b\x01\r\n\r\n"),
        Reject::BadRequest
    );
    assert_eq!(
        refused(b"GET /till/v1/sales/x HTTP/1.1\r\nHost: b\r\n folded\r\n\r\n"),
        Reject::BadRequest
    );
    assert_eq!(
        refused(&post_sales("Transfer-Encoding: chunked\r\n", "{}")),
        Reject::BadRequest
    );
    assert_eq!(
        refused(&post_sales("Content-Encoding: gzip\r\n", "{}")),
        Reject::BadRequest
    );
    // Two keys, two lengths, a length that is not a number.
    assert_eq!(
        refused(&get_sale(
            "/till/v1/sales/x",
            "Authorization: Bearer a\r\nAuthorization: Bearer b\r\n"
        )),
        Reject::BadRequest
    );
    assert_eq!(
        refused(&post_sales("Content-Length: 2\r\n", "{}")),
        Reject::BadRequest
    );
    assert_eq!(
        refused(b"POST /till/v1/sales HTTP/1.1\r\nContent-Type: application/json\r\nContent-Length: x\r\n\r\n"),
        Reject::BadRequest
    );
    // A create with no body, no type or the wrong one; a get with a body.
    assert_eq!(
        refused(b"POST /till/v1/sales HTTP/1.1\r\nContent-Type: application/json\r\n\r\n"),
        Reject::BadRequest
    );
    assert_eq!(
        refused(b"POST /till/v1/sales HTTP/1.1\r\nContent-Length: 2\r\n\r\n{}"),
        Reject::BadRequest
    );
    assert_eq!(
        refused(
            b"POST /till/v1/sales HTTP/1.1\r\nContent-Type: text/plain\r\nContent-Length: 2\r\n\r\n{}"
        ),
        Reject::BadRequest
    );
    assert_eq!(
        refused(b"GET /till/v1/sales/x HTTP/1.1\r\nContent-Length: 2\r\n\r\n{}"),
        Reject::BadRequest
    );
}

#[test]
fn bx_75_any_other_path_or_method_is_a_404_with_no_words() {
    for (method, path) in [
        ("GET", "/"),
        ("GET", "/till/v1/sales"),
        ("POST", "/till/v1/sales/x"),
        ("DELETE", "/till/v1/sales/x"),
        ("GET", "/till/v1/sales/x/cancel"),
        ("POST", "/till/v1/sales/x/other"),
        ("GET", "/till/v1/sales?x=1"),
        ("POST", "/till/v1/sales/"),
        ("PUT", "/till/v1/sales"),
        ("GET", "/till/v2/sales/x"),
        ("GET", "/till/v1/sales/x/"),
    ] {
        let bytes = format!("{method} {path} HTTP/1.1\r\nHost: b\r\n\r\n");
        assert_eq!(
            read(bytes.as_bytes()),
            Err(Reject::NotFound),
            "{method} {path}"
        );
    }
    let answer = String::from_utf8(reject_response(Reject::NotFound)).unwrap();
    assert!(answer.starts_with("HTTP/1.1 404 "));
    assert!(answer.contains("Content-Type: application/json"));
    assert!(answer.ends_with("{\"error\":\"tillNotFound\"}"));
    assert!(!answer.contains("words"));
    for reject in [Reject::BadRequest, Reject::TooLarge] {
        let answer = String::from_utf8(reject_response(reject)).unwrap();
        assert!(!answer.contains("words"));
    }
}

#[test]
fn bx_72_the_body_is_an_object_of_strings() {
    let got = parse_object(br#" {"a":"b","c":"d\"e\\f\u00e9\ud83d\ude00"} "#).unwrap();
    assert_eq!(got.get("a").map(String::as_str), Some("b"));
    assert_eq!(
        got.get("c").map(String::as_str),
        Some("d\"e\\f\u{e9}\u{1f600}")
    );
    assert_eq!(parse_object(b"{}").unwrap().len(), 0);
    for bad in [
        &b""[..],
        b"[]",
        b"{",
        b"{\"a\":1}",
        b"{\"a\":null}",
        b"{\"a\":{\"b\":\"c\"}}",
        b"{\"a\":\"b\",\"a\":\"c\"}",
        b"{\"a\":\"b\"} x",
        b"{\"a\":\"b\",}",
        b"{\"a\":\"\\q\"}",
        b"{\"a\":\"\\ud83d\"}",
        b"{\"a\":\"\\ud83d\\u0041\"}",
        b"{\"a\":\"line\nbreak\"}",
        b"{\"a\":\"\xff\"}",
        b"{'a':'b'}",
    ] {
        assert_eq!(
            parse_object(bad),
            None,
            "{:?}",
            String::from_utf8_lossy(bad)
        );
    }
}

// ---- the key (BX-71) ----------------------------------------------------

#[test]
fn bx_71_a_till_key_is_stored_only_as_a_hash_and_a_new_one_replaces_it() {
    let mut files = MemFiles::new();
    let first = make_till_key(&mut files).unwrap();
    assert_eq!(first.len(), 64);
    assert!(first
        .bytes()
        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)));
    let stored = files.read(TILL_KEY_FILE).unwrap().unwrap();
    assert_eq!(stored.len(), 32);
    // The file does not hold the key, in either form.
    let key_bytes: Vec<u8> = (0..32)
        .map(|i| u8::from_str_radix(&first[i * 2..i * 2 + 2], 16).unwrap())
        .collect();
    assert_ne!(stored, key_bytes);
    assert!(!String::from_utf8_lossy(&stored).contains(&first));
    assert_eq!(
        stored,
        till_key_hash(&key_bytes.clone().try_into().unwrap())
    );
    assert!(till_key_matches(&stored, &first));
    // A new key replaces the old, which stops matching.
    let second = make_till_key(&mut files).unwrap();
    assert_ne!(first, second);
    let stored = files.read(TILL_KEY_FILE).unwrap().unwrap();
    assert!(till_key_matches(&stored, &second));
    assert!(!till_key_matches(&stored, &first));
}

#[test]
fn bx_71_a_key_that_is_not_the_key_never_matches() {
    let mut files = MemFiles::new();
    let key = make_till_key(&mut files).unwrap();
    let stored = files.read(TILL_KEY_FILE).unwrap().unwrap();
    for wrong in [
        String::new(),
        key[..63].to_string(),
        format!("{key}0"),
        key.to_uppercase(),
        "g".repeat(64),
        "0".repeat(64),
        format!(" {key}"),
    ] {
        assert!(!till_key_matches(&stored, &wrong), "{wrong}");
    }
    // A stored hash of the wrong length matches nothing.
    assert!(!till_key_matches(&[], &key));
    assert!(!till_key_matches(&stored[..31], &key));
}

#[test]
fn bx_71_the_key_file_is_written_whole_with_mode_0600_and_the_words_name_no_key() {
    let dir = TempDir::new("tillkey");
    let mut files = DirFiles::new(dir.path());
    let key = make_till_key(&mut files).unwrap();
    assert_eq!(
        files.mode(TILL_KEY_FILE).unwrap().map(|m| m & 0o777),
        Some(0o600)
    );
    let on_disk = std::fs::read(dir.path().join(TILL_KEY_FILE)).unwrap();
    assert_eq!(on_disk.len(), 32);
    assert!(!dir.path().join(format!("{TILL_KEY_FILE}.tmp")).exists());
    // The words tell the keeper to keep it out of sight and print the key on
    // its own line, after them.
    let lines = till_key_lines(&key);
    assert_eq!(lines.first().map(String::as_str), Some("New till key"));
    assert_eq!(lines.last(), Some(&format!("till key: {key}")));
    assert!(lines
        .iter()
        .take(lines.len() - 1)
        .all(|l| !l.contains(&key)));
}

#[test]
fn bx_71_a_failed_write_is_a_refusal_with_words_and_no_key() {
    let dir = TempDir::new("tillkeyfail");
    let mut files = DirFiles::new(dir.path().join("missing"));
    let err = make_till_key(&mut files).unwrap_err();
    assert!(err.to_string().contains("till-key"), "{err}");
}

// ---- the amounts and the other fields (BX-72) ---------------------------

#[test]
fn bx_72_an_amount_is_a_decimal_string_in_the_money_s_precision() {
    // Two decimals.
    for (text, want) in [
        ("12.50", Some(1250)),
        ("12.5", Some(1250)),
        ("12", Some(1200)),
        ("0.01", Some(1)),
        ("0.5", Some(50)),
        ("007.50", Some(750)),
        ("184467440737095516.15", Some(u64::MAX)),
    ] {
        assert_eq!(parse_amount(text, 2), want, "{text}");
    }
    for text in [
        "0",
        "0.00",
        "-1",
        "+1",
        "1.234",
        ".5",
        "5.",
        "1,50",
        "1e3",
        " 1",
        "1 ",
        "",
        "abc",
        "1.2.3",
        "٣",
        "184467440737095516.16",
        "99999999999999999999999",
    ] {
        assert_eq!(parse_amount(text, 2), None, "{text:?}");
    }
    // No decimals (yen, won, satoshis): a point is more than the money has.
    assert_eq!(parse_amount("500", 0), Some(500));
    for text in ["500.0", "500.", "0", "0.5"] {
        assert_eq!(parse_amount(text, 0), None, "{text:?}");
    }
}

#[test]
fn bx_72_the_reference_is_cut_and_cleaned_and_the_request_id_is_checked() {
    assert_eq!(
        clean_reference("Order 00012-003-0004"),
        "Order 00012-003-0004"
    );
    assert_eq!(clean_reference("caf\u{e9} \u{1f600}\ttab\nnl"), "caf tabnl");
    assert_eq!(clean_reference(&"a".repeat(80)), "a".repeat(40));
    assert_eq!(clean_reference("\u{e9}\u{e9}"), "");
    // Cut after the dropped characters, so that 40 printable ones remain.
    assert_eq!(
        clean_reference(&format!("\u{e9}{}", "b".repeat(50))),
        "b".repeat(40)
    );
    assert!(request_id_ok("a"));
    assert!(request_id_ok(&"Z_-9".repeat(16)));
    for bad in ["", "a b", "a/b", "é", "a.b", &"a".repeat(65)] {
        assert!(!request_id_ok(bad), "{bad:?}");
    }
}

#[test]
fn bx_72_a_counter_is_found_by_the_first_four_bytes_of_its_key() {
    let (c1, c2) = (counter1().device_id(), counter2().device_id());
    let code = counter_code(&c1);
    assert_eq!(code.len(), 8);
    assert_eq!(code, hex(&c1.to_bytes()[..4]));
    assert_eq!(find_counter([c1, c2], &code), Some(c1));
    assert_eq!(find_counter([c1, c2], &counter_code(&c2)), Some(c2));
    assert_eq!(find_counter([c1, c2], "00000000"), None);
    assert_eq!(find_counter([c1, c2], &code.to_uppercase()), None);
    assert_eq!(find_counter([c1, c2], &code[..6]), None);
    // A code two counters share is no counter.
    assert_eq!(find_counter([c1, c1], &code), None);
    assert_eq!(find_counter([], &code), None);
}

// ---- the table: sales, time, notices (BX-72 to BX-78) -------------------

fn new_sale(by: &Identity, cents: u64, request: &str) -> NewSale {
    NewSale {
        counter: by.device_id(),
        amount: cents,
        currency: usd(),
        reference: "Order 1".into(),
        request: request.into(),
    }
}

fn tn(
    by: &Identity,
    number: u64,
    sale: [u8; 16],
    state: NoticeState,
    cents: u64,
) -> (DeviceId, Vec<u8>) {
    let draft = NoticeDraft {
        index: 1,
        amount: openbmt_counter::Amount(cents),
        currency: usd(),
        opened_at: NOW,
        state,
        state_at: NOW + 1,
        till_sale: Some(sale),
    };
    (
        by.device_id(),
        PaidNotice::sign(by, number, &draft).unwrap().to_bytes(),
    )
}

fn make(table: &mut TillTable, now: u64, n: u8, request: &str) -> [u8; 16] {
    table
        .create(now, [n; 16], new_sale(&counter1(), 1250, request))
        .unwrap()
        .0
}

fn take(table: &mut TillTable, now: u64, item: &(DeviceId, Vec<u8>)) -> Result<bool, String> {
    table.take_notice(now, &item.0, &item.1, &roles())
}

#[test]
fn bx_72_the_same_request_answers_the_same_sale_and_sends_nothing_new() {
    let mut t = TillTable::new();
    let (id, state) = t
        .create(NOW, [1; 16], new_sale(&counter1(), 1250, "req-1"))
        .unwrap();
    assert_eq!((id, state), ([1; 16], SaleState::Waiting));
    assert_eq!(t.outgoing().len(), 1);
    // The same request again, even with another id offered and other
    // fields, is the same sale.
    let again = t
        .create(NOW + 5, [2; 16], new_sale(&counter2(), 9999, "req-1"))
        .unwrap();
    assert_eq!(again, ([1; 16], SaleState::Waiting));
    assert_eq!(t.len(), 1);
    assert_eq!(t.outgoing().len(), 1);
    // Once the sale has moved, the same request says how it stands.
    take(
        &mut t,
        NOW + 6,
        &tn(&counter1(), 1, [1; 16], NoticeState::Opened, 1250),
    )
    .unwrap();
    let again = t
        .create(NOW + 7, [3; 16], new_sale(&counter1(), 1250, "req-1"))
        .unwrap();
    assert_eq!(again, ([1; 16], SaleState::Opened));
    // Another request is another sale.
    let other = make(&mut t, NOW + 8, 4, "req-2");
    assert_ne!(other, [1; 16]);
    assert_eq!(t.len(), 2);
}

#[test]
fn bx_76_the_request_the_counter_gets_says_what_the_till_asked() {
    let mut t = TillTable::new();
    let id = make(&mut t, NOW, 7, "req-1");
    let out = t.outgoing();
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].counter, counter1().device_id());
    assert_eq!(
        out[0].message,
        TillMessage::Request(SaleRequest {
            sale: id,
            amount: openbmt_counter::Amount(1250),
            currency: usd(),
            reference: "Order 1".into(),
            expires_at: NOW + REQUEST_SECS,
        })
    );
    // Over the wire it reads back.
    let bytes = out[0].message.to_bytes();
    assert_eq!(TillMessage::from_bytes(&bytes).unwrap(), out[0].message);
    // Sent: off the list.
    t.sent(&[out[0].seq]);
    assert!(t.outgoing().is_empty());
}

#[test]
fn bx_76_a_sale_nobody_starts_expires_after_five_minutes() {
    let mut t = TillTable::new();
    let id = make(&mut t, NOW, 1, "req-1");
    assert_eq!(REQUEST_SECS, 300);
    assert_eq!(t.state(NOW + 299, &id), Some(SaleState::Waiting));
    assert_eq!(t.state(NOW + 300, &id), Some(SaleState::Expired));
    // The request that was not sent is not sent now: it can no longer be
    // started.
    assert!(t.outgoing().is_empty());
    // A late "opened" does not bring an expired sale back.
    assert_eq!(
        take(
            &mut t,
            NOW + 301,
            &tn(&counter1(), 1, id, NoticeState::Opened, 1250)
        ),
        Ok(false)
    );
    assert_eq!(t.state(NOW + 302, &id), Some(SaleState::Expired));
    // An expired sale is kept an hour and then dropped, and its request id
    // can be used again.
    assert_eq!(
        t.state(NOW + 300 + KEEP_SECS - 1, &id),
        Some(SaleState::Expired)
    );
    assert_eq!(t.state(NOW + 300 + KEEP_SECS, &id), None);
    assert!(t.is_empty());
    let again = make(&mut t, NOW + 300 + KEEP_SECS, 9, "req-1");
    assert_eq!(again, [9; 16]);
}

#[test]
fn bx_76_at_most_thirty_two_sales_wait_at_once() {
    let mut t = TillTable::new();
    assert_eq!(MAX_WAITING, 32);
    for n in 0..32u8 {
        make(&mut t, NOW, n, &format!("req-{n}"));
    }
    assert_eq!(t.waiting(), 32);
    let refused = t.create(NOW, [99; 16], new_sale(&counter1(), 100, "one-more"));
    assert_eq!(refused, Err(TillRefusal::Busy));
    // The same request as one already held is not one more.
    assert!(t
        .create(NOW, [98; 16], new_sale(&counter1(), 100, "req-0"))
        .is_ok());
    // A sale that opens, or is cancelled, makes room.
    take(
        &mut t,
        NOW,
        &tn(&counter1(), 1, [0; 16], NoticeState::Opened, 1250),
    )
    .unwrap();
    assert_eq!(t.waiting(), 31);
    assert!(t
        .create(NOW, [99; 16], new_sale(&counter1(), 100, "one-more"))
        .is_ok());
    assert_eq!(
        t.create(NOW, [97; 16], new_sale(&counter1(), 100, "and-one")),
        Err(TillRefusal::Busy)
    );
    t.cancel(NOW, &[5; 16]).unwrap();
    assert!(t
        .create(NOW, [97; 16], new_sale(&counter1(), 100, "and-one"))
        .is_ok());
}

#[test]
fn bx_76_sales_are_in_memory_only_and_ended_ones_are_dropped_after_an_hour() {
    let mut t = TillTable::new();
    let id = make(&mut t, NOW, 1, "req-1");
    t.cancel(NOW + 1, &id).unwrap();
    assert_eq!(t.state(NOW + KEEP_SECS, &id), Some(SaleState::Cancelled));
    assert_eq!(t.state(NOW + 1 + KEEP_SECS, &id), None);
    // A table that is new knows nothing: what a restart does.
    let mut fresh = TillTable::new();
    assert_eq!(fresh.state(NOW, &[1; 16]), None);
    assert_eq!(fresh.cancel(NOW, &[1; 16]), None);
}

#[test]
fn bx_74_a_cancel_stops_a_sale_unless_it_is_paid_and_tells_the_counter() {
    let mut t = TillTable::new();
    let id = make(&mut t, NOW, 1, "req-1");
    let first = t.outgoing()[0].seq;
    t.sent(&[first]);
    assert_eq!(t.cancel(NOW + 1, &id), Some(SaleState::Cancelled));
    let out = t.outgoing();
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].counter, counter1().device_id());
    assert_eq!(
        out[0].message,
        TillMessage::Cancel(openbmt_pos::SaleCancel { sale: id })
    );
    // Again: still cancelled, told once.
    assert_eq!(t.cancel(NOW + 2, &id), Some(SaleState::Cancelled));
    assert_eq!(t.outgoing().len(), 1);
    // A paid sale stays paid.
    let paid = make(&mut t, NOW + 3, 2, "req-2");
    take(
        &mut t,
        NOW + 4,
        &tn(&counter1(), 1, paid, NoticeState::Paid, 1250),
    )
    .unwrap();
    assert_eq!(t.cancel(NOW + 5, &paid), Some(SaleState::Paid));
    assert!(t
        .outgoing()
        .iter()
        .all(|o| o.message != TillMessage::Cancel(openbmt_pos::SaleCancel { sale: paid })));
    // A sale the Box does not know.
    assert_eq!(t.cancel(NOW + 6, &[42; 16]), None);
}

#[test]
fn bx_78_a_counters_notices_move_the_sale() {
    let mut t = TillTable::new();
    let id = make(&mut t, NOW, 1, "req-1");
    let n = |number, state| tn(&counter1(), number, id, state, 1250);
    assert_eq!(take(&mut t, NOW + 1, &n(1, NoticeState::Opened)), Ok(true));
    assert_eq!(t.state(NOW + 1, &id), Some(SaleState::Opened));
    // Said again: nothing moves.
    assert_eq!(take(&mut t, NOW + 2, &n(2, NoticeState::Opened)), Ok(false));
    assert_eq!(take(&mut t, NOW + 3, &n(3, NoticeState::Paid)), Ok(true));
    assert_eq!(t.state(NOW + 3, &id), Some(SaleState::Paid));
    // Unpaid again after paid does not change what the till was told.
    assert_eq!(take(&mut t, NOW + 4, &n(4, NoticeState::Unpaid)), Ok(false));
    assert_eq!(t.state(NOW + 4, &id), Some(SaleState::Paid));
    // Final is paid too and changes nothing now.
    assert_eq!(take(&mut t, NOW + 5, &n(5, NoticeState::Final)), Ok(false));
    assert_eq!(t.state(NOW + 5, &id), Some(SaleState::Paid));
}

#[test]
fn bx_78_paid_and_final_come_from_the_counters_word_alone() {
    // Final with no paid before it.
    let mut t = TillTable::new();
    let id = make(&mut t, NOW, 1, "req-1");
    let n = |number, state| tn(&counter1(), number, id, state, 1250);
    assert_eq!(take(&mut t, NOW, &n(1, NoticeState::Final)), Ok(true));
    assert_eq!(t.state(NOW, &id), Some(SaleState::Paid));
    // Paid with no opened before it (the opened notice was lost).
    let id2 = make(&mut t, NOW, 2, "req-2");
    let n2 = |number, state| tn(&counter1(), number, id2, state, 1250);
    assert_eq!(take(&mut t, NOW, &n2(2, NoticeState::Paid)), Ok(true));
    assert_eq!(t.state(NOW, &id2), Some(SaleState::Paid));
    // A sale the till cancelled or let expire, and that was paid anyway, is
    // paid: the money came.
    let id3 = make(&mut t, NOW, 3, "req-3");
    t.cancel(NOW + 1, &id3).unwrap();
    assert_eq!(
        take(
            &mut t,
            NOW + 2,
            &tn(&counter1(), 3, id3, NoticeState::Paid, 1250)
        ),
        Ok(true)
    );
    assert_eq!(t.state(NOW + 2, &id3), Some(SaleState::Paid));
    let id4 = make(&mut t, NOW, 4, "req-4");
    assert_eq!(t.state(NOW + REQUEST_SECS, &id4), Some(SaleState::Expired));
    assert_eq!(
        take(
            &mut t,
            NOW + 301,
            &tn(&counter1(), 4, id4, NoticeState::Paid, 1250)
        ),
        Ok(true)
    );
    assert_eq!(t.state(NOW + 302, &id4), Some(SaleState::Paid));
}

#[test]
fn bx_78_unpaid_again_before_paid_moves_the_sale_to_unpaid() {
    let mut t = TillTable::new();
    let id = make(&mut t, NOW, 1, "req-1");
    let n = |number, state| tn(&counter1(), number, id, state, 1250);
    take(&mut t, NOW, &n(1, NoticeState::Opened)).unwrap();
    assert_eq!(take(&mut t, NOW + 1, &n(2, NoticeState::Unpaid)), Ok(true));
    assert_eq!(t.state(NOW + 1, &id), Some(SaleState::Unpaid));
    // From waiting as well.
    let id2 = make(&mut t, NOW, 2, "req-2");
    assert_eq!(
        take(
            &mut t,
            NOW,
            &tn(&counter1(), 3, id2, NoticeState::Unpaid, 1250)
        ),
        Ok(true)
    );
    assert_eq!(t.state(NOW, &id2), Some(SaleState::Unpaid));
    // And an opened sale nothing comes of for an hour is unpaid too.
    let id3 = make(&mut t, NOW, 3, "req-3");
    take(
        &mut t,
        NOW,
        &tn(&counter1(), 4, id3, NoticeState::Opened, 1250),
    )
    .unwrap();
    assert_eq!(t.state(NOW + OPEN_SECS - 1, &id3), Some(SaleState::Opened));
    assert_eq!(t.state(NOW + OPEN_SECS, &id3), Some(SaleState::Unpaid));
}

#[test]
fn bx_78_a_notice_that_does_not_fit_the_sale_is_ignored_with_a_note() {
    let mut t = TillTable::new();
    let id = make(&mut t, NOW, 1, "req-1");
    let ignored = |r: Result<bool, String>| {
        let line = r.unwrap_err();
        assert!(line.starts_with("BX-78: "), "{line}");
    };
    // Another amount, another currency.
    ignored(take(
        &mut t,
        NOW,
        &tn(&counter1(), 1, id, NoticeState::Paid, 1251),
    ));
    let other_money = {
        let draft = NoticeDraft {
            index: 1,
            amount: openbmt_counter::Amount(1250),
            currency: openbmt_counter::Currency::new(*b"EUR").unwrap(),
            opened_at: NOW,
            state: NoticeState::Paid,
            state_at: NOW,
            till_sale: Some(id),
        };
        (
            counter1().device_id(),
            PaidNotice::sign(&counter1(), 2, &draft).unwrap().to_bytes(),
        )
    };
    ignored(take(&mut t, NOW, &other_money));
    // A counter that was not asked.
    ignored(take(
        &mut t,
        NOW,
        &tn(&counter2(), 1, id, NoticeState::Paid, 1250),
    ));
    // A sale the Box holds no record of.
    ignored(take(
        &mut t,
        NOW,
        &tn(&counter1(), 3, [77; 16], NoticeState::Paid, 1250),
    ));
    // A key that is not a counter.
    ignored(take(
        &mut t,
        NOW,
        &tn(&stranger(), 1, id, NoticeState::Paid, 1250),
    ));
    ignored(take(
        &mut t,
        NOW,
        &tn(&owner(), 1, id, NoticeState::Paid, 1250),
    ));
    // A notice that names another counter than the one that sent it.
    let (_, bytes) = tn(&counter1(), 4, id, NoticeState::Paid, 1250);
    ignored(t.take_notice(NOW, &counter2().device_id(), &bytes, &roles()));
    // A signature that does not check.
    let mut forged = tn(&counter1(), 5, id, NoticeState::Paid, 1250);
    let at = forged.1.len() - 70;
    forged.1[at] ^= 1;
    ignored(take(&mut t, NOW, &forged));
    // The notice that is not signed for this sale: the sale is another one.
    let swapped = {
        let (_, mut bytes) = tn(&counter1(), 6, id, NoticeState::Paid, 1250);
        let at = bytes.len() - 64 - 16;
        bytes[at..at + 16].copy_from_slice(&[0xee; 16]);
        (counter1().device_id(), bytes)
    };
    ignored(take(&mut t, NOW, &swapped));
    // Through all of that the sale never moved.
    assert_eq!(t.state(NOW, &id), Some(SaleState::Waiting));
    // And none of those used up a number: the notice with number 1 works.
    assert_eq!(
        take(
            &mut t,
            NOW,
            &tn(&counter1(), 1, id, NoticeState::Paid, 1250)
        ),
        Ok(true)
    );
}

#[test]
fn bx_78_a_number_is_used_once_and_notices_without_a_till_sale_are_not_the_tills() {
    let mut t = TillTable::new();
    let id = make(&mut t, NOW, 1, "req-1");
    let first = tn(&counter1(), 5, id, NoticeState::Opened, 1250);
    assert_eq!(take(&mut t, NOW, &first), Ok(true));
    // The same notice again, and an older number.
    assert!(take(&mut t, NOW, &first).is_err());
    assert!(take(
        &mut t,
        NOW,
        &tn(&counter1(), 4, id, NoticeState::Paid, 1250)
    )
    .is_err());
    assert_eq!(t.state(NOW, &id), Some(SaleState::Opened));
    // A notice with no till sale is the POS side's: the table passes over
    // it and a note is not made.
    let plain = PaidNotice::sign(
        &counter1(),
        9,
        &NoticeDraft {
            index: 1,
            amount: openbmt_counter::Amount(1250),
            currency: usd(),
            opened_at: NOW,
            state: NoticeState::Paid,
            state_at: NOW,
            till_sale: None,
        },
    )
    .unwrap();
    assert_eq!(
        t.take_notice(NOW, &counter1().device_id(), &plain.to_bytes(), &roles()),
        Ok(false)
    );
    assert_eq!(t.state(NOW, &id), Some(SaleState::Opened));
    // Bytes that are not a notice are passed over too.
    assert_eq!(
        t.take_notice(NOW, &counter1().device_id(), b"junk", &roles()),
        Ok(false)
    );
    // A counter taken off the list stops being heard.
    let mut gone: BTreeMap<DeviceId, DeviceRole> = roles();
    gone.remove(&counter1().device_id());
    let n = tn(&counter1(), 10, id, NoticeState::Paid, 1250);
    assert!(t.take_notice(NOW, &n.0, &n.1, &gone).is_err());
    assert_eq!(t.state(NOW, &id), Some(SaleState::Opened));
}

// ---- the answers (BX-73, BX-75) -----------------------------------------

#[test]
fn bx_73_a_sale_is_told_with_its_state_and_for_the_ended_ones_its_words() {
    let id = [0xab; 16];
    let hexid = "ab".repeat(16);
    for (state, word) in [
        (SaleState::Waiting, "waiting"),
        (SaleState::Opened, "opened"),
    ] {
        assert_eq!(
            sale_body(&id, state),
            format!("{{\"sale\":\"{hexid}\",\"state\":\"{word}\"}}")
        );
    }
    let paid = sale_body(&id, SaleState::Paid);
    assert!(paid.contains("\"state\":\"paid\""));
    assert!(
        paid.contains("\"words\":{\"heading\":\"\",\"what\":\"Paid in bitcoin.\",\"next\":\"\"}")
    );
    for (state, answer) in [
        (SaleState::Unpaid, TillAnswer::Unpaid),
        (SaleState::Expired, TillAnswer::Expired),
        (SaleState::Cancelled, TillAnswer::Cancelled),
    ] {
        let w = answer.words();
        let body = sale_body(&id, state);
        assert!(
            body.contains(&format!("\"state\":\"{}\"", state.word())),
            "{body}"
        );
        assert!(
            body.contains(&format!("\"heading\":\"{}\"", w.heading)),
            "{body}"
        );
        assert!(body.contains(&format!("\"what\":\"{}\"", w.what)), "{body}");
        assert!(body.contains(&format!("\"next\":\"{}\"", w.next)), "{body}");
    }
    assert!(SaleState::Paid.ended() && !SaleState::Opened.ended());
}

#[test]
fn bx_75_a_refusal_is_its_status_its_key_and_its_words() {
    let cases = [
        (TillRefusal::KeyWrong, 401, "tillKeyWrong"),
        (TillRefusal::NotPaired, 503, "tillNotPaired"),
        (
            TillRefusal::UnknownCounter("1a2b3c4d".into()),
            404,
            "tillUnknownCounter",
        ),
        (
            TillRefusal::WrongMoney {
                shop: "USD".into(),
                asked: "EUR".into(),
            },
            409,
            "tillWrongMoney",
        ),
        (TillRefusal::BadAmount, 400, "tillBadAmount"),
        (TillRefusal::Busy, 429, "tillBusy"),
        (TillRefusal::Lost, 404, "tillLost"),
    ];
    for (refusal, status, key) in cases {
        let text = String::from_utf8(refusal_response(&refusal)).unwrap();
        assert!(text.starts_with(&format!("HTTP/1.1 {status} ")), "{text}");
        assert!(text.contains(&format!("\"error\":\"{key}\"")), "{text}");
        assert!(text.contains("\"words\":{\"heading\":"), "{text}");
        assert!(text.contains("Content-Type: application/json"));
        // The Content-Length is the body's.
        let (head, body) = text.split_once("\r\n\r\n").unwrap();
        assert!(head.contains(&format!("Content-Length: {}", body.len())));
    }
    for (refusal, status, key) in [
        (TillRefusal::BadRequest, 400, "tillBadRequest"),
        (TillRefusal::Fault, 500, "tillFault"),
    ] {
        let text = String::from_utf8(refusal_response(&refusal)).unwrap();
        assert!(text.starts_with(&format!("HTTP/1.1 {status} ")), "{text}");
        assert!(
            text.ends_with(&format!("{{\"error\":\"{key}\"}}")),
            "{text}"
        );
    }
    // Text from the till is cleaned before it reaches the words and the
    // JSON.
    let text = String::from_utf8(refusal_response(&TillRefusal::UnknownCounter(
        "a\"b\\c\u{202e}\n".into(),
    )))
    .unwrap();
    assert!(text.contains("the code a\\\"b\\\\c"), "{text}");
    assert!(!text.contains('\u{202e}'));
}

// ---- the words (SC-01 to SC-12) -----------------------------------------

#[test]
fn sc_01_the_tills_words_are_the_words_as_written() {
    let words = |a: &TillAnswer| {
        let w = a.words();
        (w.heading, w.what, w.next)
    };
    let s = |a: &str, b: &str, c: &str| (a.to_string(), b.to_string(), c.to_string());
    assert_eq!(
        words(&TillAnswer::KeyWrong),
        s(
            "Wrong till key",
            "The key in this payment method is not the Box's till key.",
            "Copy the till key from the Box again into this payment method."
        )
    );
    assert_eq!(
        words(&TillAnswer::Off),
        s(
            "Till listener is off",
            "The Box was started without its till listener.",
            "Start the Box with the till listener on. The handbook shows how."
        )
    );
    assert_eq!(
        words(&TillAnswer::NotPaired),
        s(
            "The Box is not set up yet",
            "The Box has not joined the shop, so it cannot reach a counter.",
            "Finish setting up the Box from the owner app."
        )
    );
    assert_eq!(
        words(&TillAnswer::UnknownCounter("1a2b3c4d".into())),
        s(
            "Counter not found",
            "The shop has no counter with the code 1a2b3c4d.",
            "Check the counter code in this payment method. The owner app shows it after the word Counter."
        )
    );
    assert_eq!(
        words(&TillAnswer::WrongMoney {
            shop: "USD".into(),
            asked: "EUR".into()
        }),
        s(
            "Wrong currency",
            "This sale is in EUR, and the shop's counters take USD.",
            "Take this payment on the counter by hand."
        )
    );
    assert_eq!(
        words(&TillAnswer::BadAmount),
        s(
            "This amount cannot be taken",
            "The amount is zero, below zero, or has more decimals than the money has.",
            "Check the sale in the till."
        )
    );
    assert_eq!(
        words(&TillAnswer::Expired),
        s(
            "Sale not started",
            "Nobody pressed Start on the counter within 5 minutes.",
            "Try again, or take the payment on the counter and pick Bitcoin by hand."
        )
    );
    assert_eq!(
        words(&TillAnswer::Busy),
        s(
            "Too many sales are waiting",
            "32 sales from the till are waiting for a counter.",
            "Finish or cancel some of them, then try again."
        )
    );
    assert_eq!(
        words(&TillAnswer::Lost),
        s(
            "The Box restarted",
            "The Box no longer knows this sale. If the customer paid, the counter shows the sale as paid.",
            "Check the counter. If it was paid, pick Bitcoin by hand in the till."
        )
    );
    assert_eq!(
        words(&TillAnswer::Unpaid),
        s(
            "The payment did not arrive",
            "The counter's sale ended without being paid.",
            "Ask the customer to try again, or pick another way to pay."
        )
    );
    assert_eq!(
        words(&TillAnswer::Cancelled),
        s(
            "Sale cancelled",
            "The till cancelled this sale before it was paid.",
            "Nothing to do."
        )
    );
    assert_eq!(
        words(&TillAnswer::KeyMade),
        s(
            "New till key",
            "Put this key in the till's payment method. The old key no longer works.",
            "Keep it out of sight. Anyone with it can ask a counter to start a sale, but cannot move money."
        )
    );
    assert_eq!(
        TILL_WAITING,
        "Waiting for the counter. Press Start on the counter."
    );
    assert_eq!(TILL_OPENED, "The customer can pay now.");
    assert_eq!(TILL_PAID, "Paid in bitcoin.");
}

#[test]
fn sc_01_every_till_answer_is_in_the_list_and_the_key_is_its_name() {
    let names: Vec<String> = all_words().into_iter().map(|(n, _)| n).collect();
    for a in till_samples() {
        let name = format!("TillAnswer::{}", a.key());
        assert!(names.contains(&name), "{name}");
        // The match is exhaustive: a new variant fails to compile here.
        match a {
            TillAnswer::KeyWrong
            | TillAnswer::Off
            | TillAnswer::NotPaired
            | TillAnswer::UnknownCounter(_)
            | TillAnswer::WrongMoney { .. }
            | TillAnswer::BadAmount
            | TillAnswer::Expired
            | TillAnswer::Busy
            | TillAnswer::Lost
            | TillAnswer::Unpaid
            | TillAnswer::Cancelled
            | TillAnswer::KeyMade => {}
        }
    }
    assert_eq!(till_samples().len(), 12);
    // No two of them say the same.
    let mut seen = std::collections::BTreeSet::new();
    for a in till_samples() {
        let w = a.words();
        assert!(seen.insert((w.heading, w.what)), "{}", a.key());
    }
    // Who takes the next step is named, and no stop names nobody.
    for a in till_samples() {
        let w = a.words();
        assert!(
            !(w.tone == openbmt_core::say::Tone::Stop && w.who == openbmt_core::say::Who::Nobody)
        );
    }
    // The console prints them as the others.
    assert_eq!(
        render(&TillAnswer::Busy.words())
            .first()
            .map(String::as_str),
        Some("Too many sales are waiting")
    );
}

// ---- the listener on real sockets (BX-70 to BX-75) ----------------------

/// A raw request and the status and body of the answer.
fn http(addr: SocketAddr, raw: &str) -> (u16, String) {
    let mut s = TcpStream::connect(addr).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    s.write_all(raw.as_bytes()).unwrap();
    let mut buf = Vec::new();
    s.read_to_end(&mut buf).unwrap();
    let text = String::from_utf8(buf).unwrap();
    let status = text.split(' ').nth(1).unwrap().parse().unwrap();
    let (head, body) = text.split_once("\r\n\r\n").unwrap();
    assert!(head.contains("Content-Type: application/json"), "{head}");
    assert!(head.contains("Connection: close"), "{head}");
    assert!(
        head.contains(&format!("Content-Length: {}", body.len())),
        "{head}"
    );
    (status, body.to_string())
}

fn call(
    addr: SocketAddr,
    method: &str,
    path: &str,
    key: Option<&str>,
    body: Option<&str>,
) -> (u16, String) {
    let auth = key.map_or(String::new(), |k| format!("Authorization: Bearer {k}\r\n"));
    let raw = match body {
        Some(b) => format!(
            "{method} {path} HTTP/1.1\r\nHost: box\r\n{auth}Content-Type: application/json\r\n\
             Content-Length: {}\r\n\r\n{b}",
            b.len()
        ),
        None => format!("{method} {path} HTTP/1.1\r\nHost: box\r\n{auth}\r\n"),
    };
    http(addr, &raw)
}

/// The text of a string field of a flat JSON body.
fn field(body: &str, name: &str) -> String {
    let at = body
        .find(&format!("\"{name}\":\""))
        .unwrap_or_else(|| panic!("{name} in {body}"));
    let rest = &body[at + name.len() + 4..];
    rest[..rest.find('"').unwrap()].to_string()
}

fn sale_json(
    counter: &Identity,
    amount: &str,
    currency: &str,
    reference: &str,
    request: &str,
) -> String {
    format!(
        "{{\"counter\":\"{}\",\"amount\":\"{amount}\",\"currency\":\"{currency}\",\
         \"reference\":\"{reference}\",\"request\":\"{request}\"}}",
        counter_code(&counter.device_id())
    )
}

struct Till {
    r: BoxRunner,
    clock: Arc<TestClock>,
    addr: SocketAddr,
    key: String,
    _dir: TempDir,
}

/// A paired Box with its till listener on and a till key made, and the
/// counters' mailboxes on its relay.
fn till_box(name: &str) -> Till {
    let dir = TempDir::new(name);
    let (mut r, clock) = start(&dir);
    pair_ok(&r);
    let key = r.make_till_key().unwrap();
    let addr = r
        .serve_till("127.0.0.1:0".parse().unwrap(), TillConfig::default())
        .unwrap();
    mailbox(&r, &counter1()).unwrap();
    mailbox(&r, &counter2()).unwrap();
    Till {
        r,
        clock,
        addr,
        key,
        _dir: dir,
    }
}

impl Till {
    fn ask(&self, method: &str, path: &str, body: Option<&str>) -> (u16, String) {
        call(self.addr, method, path, Some(&self.key), body)
    }

    fn sale(&self, request: &str) -> String {
        let (status, body) = self.ask(
            "POST",
            "/till/v1/sales",
            Some(&sale_json(
                &counter1(),
                "12.50",
                "USD",
                "Order 00012-003-0004",
                request,
            )),
        );
        assert_eq!(status, 200, "{body}");
        assert_eq!(field(&body, "state"), "waiting");
        field(&body, "sale")
    }

    fn state(&self, sale: &str) -> (u16, String) {
        self.ask("GET", &format!("/till/v1/sales/{sale}"), None)
    }

    /// What the counter has been sent, opened with its own key.
    fn counter_gets(&self, who: &Identity) -> Vec<TillMessage> {
        let box_key = self.r.device_id();
        connect(&self.r)
            .fetch(who, 0)
            .unwrap()
            .items
            .iter()
            .map(|(_, bytes)| {
                let opened = open(
                    who,
                    Role::Counter,
                    bytes,
                    |d| (*d == box_key).then_some(Role::Box),
                    |_| 0,
                )
                .unwrap();
                assert_eq!(opened.kind, Kind::PosNotice);
                TillMessage::from_bytes(&opened.content).unwrap()
            })
            .collect()
    }

    /// The counter's notice for the sale, sealed to the Box and put on its
    /// relay.
    fn counter_says(&self, link: u64, number: u64, sale: &str, state: NoticeState, cents: u64) {
        let mut id = [0u8; 16];
        for (i, b) in id.iter_mut().enumerate() {
            *b = u8::from_str_radix(&sale[i * 2..i * 2 + 2], 16).unwrap();
        }
        let (_, bytes) = tn(&counter1(), number, id, state, cents);
        put_notice(&self.r, &counter1(), link, &bytes);
    }

    fn wait_for(&self, sale: &str, state: &str) {
        eventually(&format!("the sale is {state}"), || {
            field(&self.state(sale).1, "state") == state
        });
    }
}

#[test]
fn bx_70_a_till_sale_goes_from_the_till_to_the_counter_and_back_to_paid() {
    let t = till_box("tillflow");
    let sale = t.sale("order-1");
    assert_eq!(sale.len(), 32);
    // The counter is asked, once.
    eventually("the counter has the request", || {
        !t.counter_gets(&counter1()).is_empty()
    });
    let got = t.counter_gets(&counter1());
    assert_eq!(got.len(), 1);
    match &got[0] {
        TillMessage::Request(r) => {
            assert_eq!(hex(&r.sale), sale);
            assert_eq!(r.amount.0, 1250);
            assert_eq!(r.currency, usd());
            assert_eq!(r.reference, "Order 00012-003-0004");
            assert_eq!(r.expires_at, NOW + 300);
        }
        other => panic!("{other:?}"),
    }
    // The other counter is not asked.
    assert!(t.counter_gets(&counter2()).is_empty());
    // Waiting, then opened, then paid, each on the counter's signed word.
    let (status, body) = t.state(&sale);
    assert_eq!((status, field(&body, "state").as_str()), (200, "waiting"));
    assert!(!body.contains("words"));
    t.counter_says(1, 1, &sale, NoticeState::Opened, 1250);
    t.wait_for(&sale, "opened");
    assert!(!t.state(&sale).1.contains("words"));
    t.counter_says(2, 2, &sale, NoticeState::Paid, 1250);
    t.wait_for(&sale, "paid");
    let body = t.state(&sale).1;
    assert_eq!(field(&body, "what"), "Paid in bitcoin.");
    assert_eq!(field(&body, "heading"), "");
    // Said again, the same request id answers the sale and sends no more.
    let (_, again) = t.ask(
        "POST",
        "/till/v1/sales",
        Some(&sale_json(
            &counter1(),
            "12.50",
            "USD",
            "Order 00012-003-0004",
            "order-1",
        )),
    );
    assert_eq!(field(&again, "sale"), sale);
    assert_eq!(t.counter_gets(&counter1()).len(), 1);
}

#[test]
fn bx_72_every_refusal_of_a_sale_has_its_status_and_words() {
    let t = till_box("tillrefuse");
    let post = |body: &str| t.ask("POST", "/till/v1/sales", Some(body));
    let n = std::cell::Cell::new(0u32);
    let good = |counter: &str, amount: &str, currency: &str| {
        n.set(n.get() + 1);
        format!(
            "{{\"counter\":\"{counter}\",\"amount\":\"{amount}\",\"currency\":\"{currency}\",\
             \"reference\":\"r\",\"request\":\"q-{}\"}}",
            n.get()
        )
    };
    let code = counter_code(&counter1().device_id());
    // The counter is not one of the shop's.
    let (status, body) = post(&good("00000000", "1.00", "USD"));
    assert_eq!(status, 404);
    assert_eq!(field(&body, "error"), "tillUnknownCounter");
    assert_eq!(field(&body, "heading"), "Counter not found");
    // A code that is the owner's, a Box's or a stranger's is no counter.
    for who in [owner(), the_box(), stranger()] {
        let (status, body) = post(&good(&counter_code(&who.device_id()), "1.00", "USD"));
        assert_eq!(
            (status, field(&body, "error").as_str()),
            (404, "tillUnknownCounter")
        );
    }
    // The money is not the shop's.
    let (status, body) = post(&good(&code, "1.00", "EUR"));
    assert_eq!(status, 409);
    assert_eq!(field(&body, "error"), "tillWrongMoney");
    assert_eq!(field(&body, "heading"), "Wrong currency");
    let (status, _) = post(&good(&code, "1.00", "usd"));
    assert_eq!(status, 409);
    // The amount.
    for amount in ["0", "0.00", "-5", "1.005", "abc", "", "1e3", ".5"] {
        let (status, body) = post(&good(&code, amount, "USD"));
        assert_eq!(
            (status, field(&body, "error").as_str()),
            (400, "tillBadAmount"),
            "{amount:?}"
        );
        assert!(body.contains("\"words\""));
    }
    // A body that does not read: no words.
    for body in [
        "{}",
        "[]",
        "not json",
        "{\"counter\":\"x\"}",
        "{\"counter\":1,\"amount\":\"1\",\"currency\":\"USD\",\"request\":\"q\"}",
        "{\"counter\":\"x\",\"amount\":\"1\",\"currency\":\"USD\",\"request\":\"bad id\"}",
        "{\"counter\":\"x\",\"amount\":\"1\",\"currency\":\"USD\",\"request\":\"\"}",
    ] {
        let (status, answer) = post(body);
        assert_eq!(status, 400, "{body}");
        assert_eq!(answer, "{\"error\":\"tillBadRequest\"}");
    }
    // None of those made a sale or sent the counter anything.
    std::thread::sleep(Duration::from_millis(500));
    assert!(t.counter_gets(&counter1()).is_empty());
    // And a reference with a character outside printable ASCII is cleaned.
    let (status, _) = post(&format!(
        "{{\"counter\":\"{code}\",\"amount\":\"2\",\"currency\":\"USD\",\"reference\":\"caf\\u00e9 {}\",\"request\":\"q-clean\"}}",
        "z".repeat(60)
    ));
    assert_eq!(status, 200);
    eventually("the request arrives", || {
        !t.counter_gets(&counter1()).is_empty()
    });
    match &t.counter_gets(&counter1())[0] {
        TillMessage::Request(r) => {
            assert_eq!(r.reference.chars().count(), 40);
            assert!(r.reference.starts_with("caf "));
            assert_eq!(r.amount.0, 200);
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn bx_71_a_request_without_the_till_key_is_refused_with_words() {
    let t = till_box("tillkey401");
    let body = sale_json(&counter1(), "1.00", "USD", "r", "q");
    for key in [
        None,
        Some("x"),
        Some(&"0".repeat(64)[..]),
        Some(&t.key.to_uppercase()[..]),
    ] {
        for (method, path, b) in [
            ("POST", "/till/v1/sales", Some(body.as_str())),
            ("GET", &format!("/till/v1/sales/{SALE}")[..], None),
            ("POST", &format!("/till/v1/sales/{SALE}/cancel")[..], None),
        ] {
            let (status, answer) = call(t.addr, method, path, key, b);
            assert_eq!(status, 401, "{method} {path} {key:?}");
            assert_eq!(field(&answer, "error"), "tillKeyWrong");
            assert_eq!(field(&answer, "heading"), "Wrong till key");
        }
    }
    // A scheme other than Bearer.
    let raw = format!(
        "GET /till/v1/sales/{SALE} HTTP/1.1\r\nHost: b\r\nAuthorization: Basic {}\r\n\r\n",
        t.key
    );
    assert_eq!(http(t.addr, &raw).0, 401);
    // And nothing was made.
    assert_eq!(t.state(SALE).0, 404);
    // A new key replaces the old: the old one stops at the next request.
    let old = t.key.clone();
    let new = t.r.make_till_key().unwrap();
    assert_ne!(old, new);
    assert_eq!(
        call(
            t.addr,
            "GET",
            &format!("/till/v1/sales/{SALE}"),
            Some(&old),
            None
        )
        .0,
        401
    );
    assert_eq!(
        call(
            t.addr,
            "GET",
            &format!("/till/v1/sales/{SALE}"),
            Some(&new),
            None
        )
        .0,
        404
    );
}

#[test]
fn bx_71_a_box_with_no_till_key_made_refuses_every_request() {
    let dir = TempDir::new("tillnokey");
    let (mut r, _) = start(&dir);
    pair_ok(&r);
    let addr = r
        .serve_till("127.0.0.1:0".parse().unwrap(), TillConfig::default())
        .unwrap();
    let (status, body) = call(
        addr,
        "GET",
        &format!("/till/v1/sales/{SALE}"),
        Some(&"0".repeat(64)),
        None,
    );
    assert_eq!(
        (status, field(&body, "error").as_str()),
        (401, "tillKeyWrong")
    );
    assert!(!dir.path().join(TILL_KEY_FILE).exists());
}

#[test]
fn bx_72_a_box_that_has_not_joined_the_shop_says_so() {
    let dir = TempDir::new("tillunpaired");
    let (mut r, _) = start(&dir);
    let key = r.make_till_key().unwrap();
    let addr = r
        .serve_till("127.0.0.1:0".parse().unwrap(), TillConfig::default())
        .unwrap();
    for (method, path, body) in [
        (
            "POST",
            "/till/v1/sales".to_string(),
            Some(sale_json(&counter1(), "1", "USD", "r", "q")),
        ),
        ("GET", format!("/till/v1/sales/{SALE}"), None),
        ("POST", format!("/till/v1/sales/{SALE}/cancel"), None),
    ] {
        let (status, answer) = call(addr, method, &path, Some(&key), body.as_deref());
        assert_eq!(status, 503, "{path}");
        assert_eq!(field(&answer, "error"), "tillNotPaired");
        assert_eq!(field(&answer, "heading"), "The Box is not set up yet");
    }
    // Without the key it is still the key that is refused.
    assert_eq!(
        call(addr, "GET", &format!("/till/v1/sales/{SALE}"), None, None).0,
        401
    );
}

#[test]
fn bx_73_the_states_of_a_sale_and_the_sale_the_box_does_not_know() {
    let t = till_box("tillstates");
    let (status, body) = t.state(SALE);
    assert_eq!(status, 404);
    assert_eq!(field(&body, "error"), "tillLost");
    assert_eq!(field(&body, "heading"), "The Box restarted");
    // An id that is not an id is a sale the Box does not know.
    let (status, body) = t.ask("GET", "/till/v1/sales/not-an-id", None);
    assert_eq!((status, field(&body, "error").as_str()), (404, "tillLost"));
    // A sale the counter says is not paid.
    let sale = t.sale("order-unpaid");
    t.counter_says(1, 1, &sale, NoticeState::Opened, 1250);
    t.wait_for(&sale, "opened");
    t.counter_says(2, 2, &sale, NoticeState::Unpaid, 1250);
    t.wait_for(&sale, "unpaid");
    let body = t.state(&sale).1;
    assert_eq!(field(&body, "heading"), "The payment did not arrive");
    // A sale nobody starts.
    let sale = t.sale("order-late");
    t.clock.advance(300);
    t.wait_for(&sale, "expired");
    let body = t.state(&sale).1;
    assert_eq!(field(&body, "heading"), "Sale not started");
    assert_eq!(
        field(&body, "what"),
        "Nobody pressed Start on the counter within 5 minutes."
    );
}

#[test]
fn bx_74_a_cancel_over_the_socket_reaches_the_counter_and_a_paid_sale_stays_paid() {
    let t = till_box("tillcancel");
    let sale = t.sale("order-c");
    eventually("the request arrives", || {
        !t.counter_gets(&counter1()).is_empty()
    });
    let (status, body) = t.ask("POST", &format!("/till/v1/sales/{sale}/cancel"), None);
    assert_eq!(status, 200, "{body}");
    assert_eq!(field(&body, "state"), "cancelled");
    assert_eq!(field(&body, "heading"), "Sale cancelled");
    eventually("the cancel arrives", || {
        t.counter_gets(&counter1()).len() == 2
    });
    match &t.counter_gets(&counter1())[1] {
        TillMessage::Cancel(c) => assert_eq!(hex(&c.sale), sale),
        other => panic!("{other:?}"),
    }
    assert_eq!(field(&t.state(&sale).1, "state"), "cancelled");
    // A paid sale stays paid.
    let paid = t.sale("order-p");
    t.counter_says(1, 1, &paid, NoticeState::Paid, 1250);
    t.wait_for(&paid, "paid");
    let (status, body) = t.ask("POST", &format!("/till/v1/sales/{paid}/cancel"), None);
    assert_eq!((status, field(&body, "state").as_str()), (200, "paid"));
    // A sale the Box does not know.
    let (status, body) = t.ask("POST", &format!("/till/v1/sales/{SALE}/cancel"), None);
    assert_eq!((status, field(&body, "error").as_str()), (404, "tillLost"));
}

#[test]
fn bx_75_the_wrong_path_or_method_says_nothing_about_the_key() {
    let t = till_box("tillpaths");
    for (method, path) in [
        ("GET", "/"),
        ("GET", "/till/v1/sales"),
        ("DELETE", &format!("/till/v1/sales/{SALE}")[..]),
        ("GET", "/till/v1/other"),
    ] {
        for key in [None, Some(t.key.as_str())] {
            let (status, body) = call(t.addr, method, path, key, None);
            assert_eq!(status, 404, "{method} {path}");
            assert_eq!(body, "{\"error\":\"tillNotFound\"}");
        }
    }
    // Over the limit.
    let raw = format!(
        "POST /till/v1/sales HTTP/1.1\r\nHost: b\r\nContent-Type: application/json\r\n\
         Content-Length: {}\r\n\r\n",
        MAX_REQUEST
    );
    let (status, body) = http(t.addr, &raw);
    assert_eq!(
        (status, body.as_str()),
        (413, "{\"error\":\"tillTooLarge\"}")
    );
}

#[test]
fn bx_70_a_slow_connection_is_closed_and_the_connections_are_capped() {
    // The listener on its own, over a backend that answers at once, with
    // short limits.
    struct Fixed;
    impl TillBackend for Fixed {
        fn key_ok(&self, _: Option<&str>) -> bool {
            true
        }
        fn create(&self, _: CreateBody) -> Result<([u8; 16], SaleState), TillRefusal> {
            Err(TillRefusal::Busy)
        }
        fn read(&self, _: &[u8; 16]) -> Result<SaleState, TillRefusal> {
            Ok(SaleState::Waiting)
        }
        fn cancel(&self, _: &[u8; 16]) -> Result<SaleState, TillRefusal> {
            Ok(SaleState::Cancelled)
        }
    }
    let cfg = TillConfig {
        max_connections: 2,
        read: Duration::from_millis(600),
        write: Duration::from_secs(5),
    };
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let till = TillListener::start(listener, Arc::new(Fixed), cfg).unwrap();
    let addr = till.local_addr();
    // Two connections that say nothing hold both slots.
    let quiet: Vec<TcpStream> = (0..2).map(|_| TcpStream::connect(addr).unwrap()).collect();
    std::thread::sleep(Duration::from_millis(100));
    // A third is closed at once, with no answer.
    let mut third = TcpStream::connect(addr).unwrap();
    third
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    let _ = third.write_all(format!("GET /till/v1/sales/{SALE} HTTP/1.1\r\n\r\n").as_bytes());
    let mut got = Vec::new();
    let _ = third.read_to_end(&mut got);
    assert!(got.is_empty(), "{}", String::from_utf8_lossy(&got));
    // The quiet ones are answered 400 when their time is up and let go.
    let started = Instant::now();
    for mut q in quiet {
        q.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let mut text = String::new();
        q.read_to_string(&mut text).unwrap();
        assert!(text.starts_with("HTTP/1.1 400 "), "{text}");
    }
    assert!(started.elapsed() < Duration::from_secs(3));
    // The slots are free again.
    let (status, _) = http(addr, &format!("GET /till/v1/sales/{SALE} HTTP/1.1\r\n\r\n"));
    assert_eq!(status, 200);
    // A request that trickles in, a byte at a time, is cut off the same way.
    let mut slow = TcpStream::connect(addr).unwrap();
    slow.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    for b in b"GET /till/v1/sales/".iter() {
        if slow.write_all(&[*b]).is_err() {
            break;
        }
        std::thread::sleep(Duration::from_millis(60));
    }
    let mut text = String::new();
    let _ = slow.read_to_string(&mut text);
    // The answer may be lost to a reset if the last bytes were still on
    // their way; when it comes it is the 400.
    assert!(
        text.is_empty() || text.starts_with("HTTP/1.1 400 "),
        "{text}"
    );
    till.stop();
}

#[test]
fn bx_70_the_relay_and_the_pairing_work_while_the_till_listener_is_full_of_slow_connections() {
    let t = till_box("tillslow");
    let _held: Vec<TcpStream> = (0..64)
        .filter_map(|_| TcpStream::connect(t.addr).ok())
        .collect();
    // The relay still serves and the Box still counts as paired.
    assert!(connect(&t.r).fetch(&counter1(), 0).is_ok());
    assert_eq!(t.r.status(), crate::state::BoxStatus::Paired);
}

#[test]
fn bx_76_a_restart_forgets_the_sales_and_the_till_is_told_so() {
    let dir = TempDir::new("tillrestart");
    let clock = TestClock::new(NOW);
    let r = start_in(&dir, &clock).unwrap();
    pair_ok(&r);
    let key = r.make_till_key().unwrap();
    let mut r = r;
    let addr = r
        .serve_till("127.0.0.1:0".parse().unwrap(), TillConfig::default())
        .unwrap();
    let (_, body) = call(
        addr,
        "POST",
        "/till/v1/sales",
        Some(&key),
        Some(&sale_json(&counter1(), "3.00", "USD", "r", "q-1")),
    );
    let sale = field(&body, "sale");
    r.stop();
    // The same data directory, the Box started again: the key still works,
    // the sale is gone.
    let mut r = start_in(&dir, &clock).unwrap();
    let addr = r
        .serve_till("127.0.0.1:0".parse().unwrap(), TillConfig::default())
        .unwrap();
    let (status, body) = call(
        addr,
        "GET",
        &format!("/till/v1/sales/{sale}"),
        Some(&key),
        None,
    );
    assert_eq!((status, field(&body, "error").as_str()), (404, "tillLost"));
    // The same request id is a new sale now.
    let (status, body) = call(
        addr,
        "POST",
        "/till/v1/sales",
        Some(&key),
        Some(&sale_json(&counter1(), "3.00", "USD", "r", "q-1")),
    );
    assert_eq!(status, 200);
    assert_ne!(field(&body, "sale"), sale);
}

#[test]
fn bx_76_thirty_two_waiting_sales_and_the_thirty_third_is_busy_over_the_socket() {
    let t = till_box("tillbusy");
    for n in 0..32 {
        t.sale(&format!("order-{n}"));
    }
    let (status, body) = t.ask(
        "POST",
        "/till/v1/sales",
        Some(&sale_json(&counter1(), "1.00", "USD", "r", "order-33")),
    );
    assert_eq!(status, 429);
    assert_eq!(field(&body, "error"), "tillBusy");
    assert_eq!(
        field(&body, "what"),
        "32 sales from the till are waiting for a counter."
    );
}

#[test]
fn bx_76_a_request_the_relay_cannot_take_yet_is_sent_at_the_next_step() {
    // The counter has no mailbox when the request is made: the Box cannot
    // put it there until the counter makes one.
    let dir = TempDir::new("tillretry");
    let (mut r, _) = start(&dir);
    pair_ok(&r);
    let key = r.make_till_key().unwrap();
    let addr = r
        .serve_till("127.0.0.1:0".parse().unwrap(), TillConfig::default())
        .unwrap();
    let (status, _) = call(
        addr,
        "POST",
        "/till/v1/sales",
        Some(&key),
        Some(&sale_json(&counter1(), "3.00", "USD", "r", "q-1")),
    );
    assert_eq!(status, 200);
    std::thread::sleep(Duration::from_millis(600));
    mailbox(&r, &counter1()).unwrap();
    let box_key = r.device_id();
    eventually("the request reaches the counter once it can", || {
        connect(&r)
            .fetch(&counter1(), 0)
            .is_ok_and(|f| !f.items.is_empty())
    });
    // Once, and as a request the counter can open.
    let f = connect(&r).fetch(&counter1(), 0).unwrap();
    assert_eq!(f.items.len(), 1);
    let opened = open(
        &counter1(),
        Role::Counter,
        &f.items[0].1,
        |d| (*d == box_key).then_some(Role::Box),
        |_| 0,
    )
    .unwrap();
    assert!(matches!(
        TillMessage::from_bytes(&opened.content),
        Ok(TillMessage::Request(_))
    ));
}

#[test]
fn bx_77_an_old_paid_notice_with_no_till_sale_is_still_taken_by_the_pos_side() {
    use super::pos_kit::{draft, notice, pos};
    let conn = TestConnector::default();
    let mut p = pos(&conn);
    let n = notice(&counter1(), 1, &draft(1, NoticeState::Paid, NOW, 500));
    assert_eq!(n.till_sale, None);
    assert_eq!(n.to_bytes().len(), openbmt_pos::PAID_NOTICE_LEN);
    let lines = p.take_notices(&[(counter1().device_id(), n.to_bytes())], &roles(), NOW);
    assert!(lines.is_empty(), "{lines:?}");
    assert_eq!(p.sales().len(), 1);
    // A notice that names a till sale is a notice like any other to it.
    let mut d = draft(2, NoticeState::Paid, NOW, 500);
    d.till_sale = Some([7; 16]);
    let n = notice(&counter1(), 2, &d);
    assert_eq!(n.to_bytes().len(), openbmt_pos::PAID_NOTICE_TILL_LEN);
    let lines = p.take_notices(&[(counter1().device_id(), n.to_bytes())], &roles(), NOW);
    assert!(lines.is_empty(), "{lines:?}");
    assert_eq!(p.sales().len(), 2);
}

#[test]
fn bx_78_a_notice_from_another_counter_or_with_another_amount_moves_nothing_over_the_socket() {
    let t = till_box("tillmismatch");
    let sale = t.sale("order-m");
    // Counter 2 was not asked; the amount of the notice is not the sale's.
    let mut id = [0u8; 16];
    for (i, b) in id.iter_mut().enumerate() {
        *b = u8::from_str_radix(&sale[i * 2..i * 2 + 2], 16).unwrap();
    }
    put_notice(
        &t.r,
        &counter2(),
        1,
        &tn(&counter2(), 1, id, NoticeState::Paid, 1250).1,
    );
    t.counter_says(1, 1, &sale, NoticeState::Paid, 1);
    eventually("the notices were read and noted", || {
        t.r.notes()
            .iter()
            .filter(|n| n.starts_with("BX-78: "))
            .count()
            >= 2
    });
    assert_eq!(field(&t.state(&sale).1, "state"), "waiting");
    // The right notice, after them, still works: no number was used up.
    t.counter_says(2, 1, &sale, NoticeState::Paid, 1250);
    t.wait_for(&sale, "paid");
}

#[test]
fn bx_79_a_paid_till_sale_closes_no_order_by_itself() {
    use super::pos_kit::{pos_settings, TestConnector};
    use crate::pos::PosSetup;
    use openbmt_link::ServerConfig;
    use openbmt_net_io::Clock;
    let dir = TempDir::new("tillnoclose");
    let clock = TestClock::new(NOW);
    let conn = TestConnector::default();
    let mut r = BoxRunner::start_with(
        dir.path(),
        "127.0.0.1:0".parse::<SocketAddr>().unwrap(),
        Arc::clone(&clock) as Arc<dyn Clock + Send + Sync>,
        ServerConfig::default(),
        Some(PosSetup::new(pos_settings(), Box::new(conn.clone()))),
    )
    .unwrap();
    pair_ok(&r);
    let key = r.make_till_key().unwrap();
    let addr = r
        .serve_till("127.0.0.1:0".parse().unwrap(), TillConfig::default())
        .unwrap();
    mailbox(&r, &counter1()).unwrap();
    let (_, body) = call(
        addr,
        "POST",
        "/till/v1/sales",
        Some(&key),
        Some(&sale_json(&counter1(), "12.50", "USD", "r", "q-1")),
    );
    let sale = field(&body, "sale");
    let mut id = [0u8; 16];
    for (i, b) in id.iter_mut().enumerate() {
        *b = u8::from_str_radix(&sale[i * 2..i * 2 + 2], 16).unwrap();
    }
    put_notice(
        &r,
        &counter1(),
        1,
        &tn(&counter1(), 1, id, NoticeState::Paid, 1250).1,
    );
    eventually("the till reads paid", || {
        field(
            &call(
                addr,
                "GET",
                &format!("/till/v1/sales/{sale}"),
                Some(&key),
                None,
            )
            .1,
            "state",
        ) == "paid"
    });
    // The connector cannot close, so BX-43 closes nothing; and the till
    // sale took no part in any attempt: the connector was never asked for
    // an order or to close one on its account.
    std::thread::sleep(Duration::from_millis(600));
    let probe = conn.probe();
    assert!(probe.closes.is_empty());
    assert_eq!(probe.find_calls, 0);
    drop(probe);
    r.stop();
}

#[test]
fn bx_70_a_till_sale_waits_on_a_box_that_reads_no_pos() {
    // The default program has no connector: the till still works.
    let t = till_box("tillnopos");
    let sale = t.sale("order-n");
    t.counter_says(1, 1, &sale, NoticeState::Paid, 1250);
    t.wait_for(&sale, "paid");
    assert_eq!(t.r.pos_view(), crate::runner::PosView::NotInstalled);
}

#[test]
fn bx_71_a_till_key_made_while_the_listener_runs_reaches_the_next_request() {
    let t = till_box("tillrotate");
    let sale = t.sale("order-r");
    let old = t.key.clone();
    let new = t.r.make_till_key().unwrap();
    let path = format!("/till/v1/sales/{sale}");
    assert_eq!(call(t.addr, "GET", &path, Some(&old), None).0, 401);
    assert_eq!(call(t.addr, "GET", &path, Some(&new), None).0, 200);
}

#[test]
fn bx_71_the_stored_value_is_a_tagged_hash_of_the_key() {
    // The stored value is not derived by a plain SHA-256 of the key alone:
    // it is the tagged hash, so that the file cannot be matched against
    // other hashes of the same bytes.
    let key = [7u8; 32];
    let tagged = till_key_hash(&key);
    assert_ne!(tagged.to_vec(), key.to_vec());
    assert_eq!(tagged.len(), 32);
}

#[test]
fn bx_79_bx_43_is_as_it_was_a_notice_that_names_a_till_sale_closes_the_same_as_one_that_does_not() {
    use super::pos_kit::{pos, TestConnector};
    // The same sale, once with a till sale named and once without, closes
    // one order each, on the paid notice and the key of PS-81 alone.
    for till_sale in [None, Some([9u8; 16])] {
        let conn = TestConnector::closing("o9");
        let mut p = pos(&conn);
        let d = |state, at| NoticeDraft {
            index: 0,
            amount: openbmt_counter::Amount(500),
            currency: usd(),
            opened_at: NOW,
            state,
            state_at: at,
            till_sale,
        };
        let items = [
            (
                counter1().device_id(),
                PaidNotice::sign(&counter1(), 1, &d(NoticeState::Opened, NOW))
                    .unwrap()
                    .to_bytes(),
            ),
            (
                counter1().device_id(),
                PaidNotice::sign(&counter1(), 2, &d(NoticeState::Paid, NOW + 10))
                    .unwrap()
                    .to_bytes(),
            ),
        ];
        assert!(p.take_notices(&items, &roles(), NOW + 11).is_empty());
        // The sale is only "opened" on the Box until the counter says paid:
        // nothing is closed on the strength of an opened notice.
        p.tick(NOW + 12, super::pos_kit::token());
        assert_eq!(conn.probe().closes.len(), 1, "{till_sale:?}");
    }
    // A till sale that is only opened closes nothing.
    let conn = TestConnector::closing("o9");
    let mut p = pos(&conn);
    let opened = NoticeDraft {
        index: 0,
        amount: openbmt_counter::Amount(500),
        currency: usd(),
        opened_at: NOW,
        state: NoticeState::Opened,
        state_at: NOW,
        till_sale: Some([9; 16]),
    };
    let items = [(
        counter1().device_id(),
        PaidNotice::sign(&counter1(), 1, &opened)
            .unwrap()
            .to_bytes(),
    )];
    assert!(p.take_notices(&items, &roles(), NOW + 1).is_empty());
    p.tick(NOW + 12, super::pos_kit::token());
    assert!(conn.probe().closes.is_empty());
}

#[test]
fn bx_77_a_till_sale_that_was_changed_after_signing_is_refused_by_the_pos_side() {
    use super::pos_kit::{draft, pos};
    let conn = TestConnector::default();
    let mut p = pos(&conn);
    let mut d = draft(3, NoticeState::Paid, NOW, 500);
    d.till_sale = Some([7; 16]);
    let mut bytes = PaidNotice::sign(&counter1(), 1, &d).unwrap().to_bytes();
    let at = bytes.len() - 64 - 1;
    bytes[at] ^= 1;
    let lines = p.take_notices(&[(counter1().device_id(), bytes)], &roles(), NOW);
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert!(lines[0].contains("signature"), "{lines:?}");
    assert!(p.sales().is_empty());
}
