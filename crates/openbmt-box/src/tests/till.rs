//! The till listener (BX-70 to BX-79): the reader, the key, the sales, the
//! counters' notices and the whole of it over real sockets on the loopback.
//! BX-70 and BX-71 as a process are tested in `tests/till.rs`.

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::sync::Arc;
use std::time::{Duration, Instant};

use openbmt_core::hkdf::{hmac_sha256, sha256_of};
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
    check_till_key, clean_reference, counter_code, find_counter, format_amount, hex, key_file,
    make_till_key, parse_amount, reply_mac, request_id_ok, till_bearer, till_key_hash,
    till_reply_key, KeyCheck, NewSale, SaleInfo, SaleState, TillRefusal, TillTable, KEEP_SECS,
    KEY_FILE_LEN, KEY_FILE_VERSION, MAX_WAITING, OPEN_SECS, REQUEST_SECS, TILL_KEY_FILE,
};
use crate::till_listener::{
    parse_object, parse_request, refusal_response, reject_response, sale_body, CreateBody, Reject,
    Request, Route, TillBackend, TillConfig, TillListener, MAX_REQUEST,
};
use crate::BoxRunner;

// ---- the reader (BX-70, BX-75) -----------------------------------------

/// A nonce and the header that carries it (BX-80).
const NONCE: &str = "0123456789abcdef0123456789abcdef";
const NONCE_HEADER: &str = "X-OBM-Nonce: 0123456789abcdef0123456789abcdef\r\n";

fn post_sales(extra: &str, body: &str) -> Vec<u8> {
    format!(
        "POST /till/v1/sales HTTP/1.1\r\nHost: box\r\n{NONCE_HEADER}{extra}\
         Content-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    )
    .into_bytes()
}

fn get_sale(path: &str, extra: &str) -> Vec<u8> {
    format!("GET {path} HTTP/1.1\r\nHost: box\r\n{NONCE_HEADER}{extra}\r\n").into_bytes()
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
    assert_eq!(
        (r.method.as_str(), r.path.as_str(), r.nonce.as_str()),
        ("POST", "/till/v1/sales", NONCE)
    );
    assert_eq!(r.body, b"{\"a\":\"b\"}");
    let want = [0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77];
    let r = read(&get_sale(&format!("/till/v1/sales/{SALE}"), &auth))
        .unwrap()
        .unwrap();
    match r.route {
        Route::Read(Some(id)) => assert_eq!(id.get(..8), Some(&want[..])),
        other => panic!("{other:?}"),
    }
    let cancel = format!(
        "POST /till/v1/sales/{SALE}/cancel HTTP/1.1\r\nHost: box\r\n{NONCE_HEADER}{auth}\r\n"
    )
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
    let head = format!(
        "POST /till/v1/sales HTTP/1.1\r\nHost: box\r\n{NONCE_HEADER}\
         Content-Type: application/json\r\nContent-Length: 3000\r\n\r\n"
    );
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
        refused(b"GET /till/v1/sales/x HTTP/1.1\r\nX-OBM-Nonce: 0123456789abcdef0123456789abcdef\r\nHost: b\x01\r\n\r\n"),
        Reject::BadRequest
    );
    assert_eq!(
        refused(b"GET /till/v1/sales/x HTTP/1.1\r\nX-OBM-Nonce: 0123456789abcdef0123456789abcdef\r\nHost: b\r\n folded\r\n\r\n"),
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
        refused(b"POST /till/v1/sales HTTP/1.1\r\nX-OBM-Nonce: 0123456789abcdef0123456789abcdef\r\nContent-Type: application/json\r\nContent-Length: x\r\n\r\n"),
        Reject::BadRequest
    );
    // A create with no body, no type or the wrong one; a get with a body.
    assert_eq!(
        refused(b"POST /till/v1/sales HTTP/1.1\r\nX-OBM-Nonce: 0123456789abcdef0123456789abcdef\r\nContent-Type: application/json\r\n\r\n"),
        Reject::BadRequest
    );
    assert_eq!(
        refused(b"POST /till/v1/sales HTTP/1.1\r\nX-OBM-Nonce: 0123456789abcdef0123456789abcdef\r\nContent-Length: 2\r\n\r\n{}"),
        Reject::BadRequest
    );
    assert_eq!(
        refused(
            b"POST /till/v1/sales HTTP/1.1\r\nX-OBM-Nonce: 0123456789abcdef0123456789abcdef\r\nContent-Type: text/plain\r\nContent-Length: 2\r\n\r\n{}"
        ),
        Reject::BadRequest
    );
    assert_eq!(
        refused(b"GET /till/v1/sales/x HTTP/1.1\r\nX-OBM-Nonce: 0123456789abcdef0123456789abcdef\r\nContent-Length: 2\r\n\r\n{}"),
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

/// A key as 64 lowercase hex, as 32 bytes.
fn key_bytes(key: &str) -> [u8; 32] {
    let bytes: Vec<u8> = (0..32)
        .map(|i| u8::from_str_radix(&key[i * 2..i * 2 + 2], 16).unwrap())
        .collect();
    bytes.try_into().unwrap()
}

/// The bearer token a till sends for a key, as 64 lowercase hex.
fn token_of(key: &str) -> String {
    hex(&till_bearer(&key_bytes(key)))
}

#[test]
fn bx_71_a_till_key_is_stored_only_as_a_hash_and_a_new_one_replaces_it() {
    let mut files = MemFiles::new();
    let first = make_till_key(&mut files).unwrap();
    assert_eq!(first.len(), 64);
    assert!(first
        .bytes()
        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)));
    let stored = files.read(TILL_KEY_FILE).unwrap().unwrap();
    assert_eq!(stored.len(), KEY_FILE_LEN);
    // The file does not hold the key, in either form, nor the token.
    let first_bytes = key_bytes(&first);
    assert!(!stored.windows(32).any(|w| w == first_bytes));
    assert!(!stored.windows(32).any(|w| w == till_bearer(&first_bytes)));
    assert!(!String::from_utf8_lossy(&stored).contains(&first));
    assert_eq!(stored, key_file(&first_bytes));
    assert_eq!(
        check_till_key(Some(&stored), Some(&token_of(&first))),
        KeyCheck::Good(till_reply_key(&first_bytes))
    );
    // A new key replaces the old, which stops matching.
    let second = make_till_key(&mut files).unwrap();
    assert_ne!(first, second);
    let stored = files.read(TILL_KEY_FILE).unwrap().unwrap();
    assert!(matches!(
        check_till_key(Some(&stored), Some(&token_of(&second))),
        KeyCheck::Good(_)
    ));
    assert!(matches!(
        check_till_key(Some(&stored), Some(&token_of(&first))),
        KeyCheck::Wrong(Some(_))
    ));
}

#[test]
fn bx_71_a_key_that_is_not_the_key_never_matches() {
    let mut files = MemFiles::new();
    let key = make_till_key(&mut files).unwrap();
    let token = token_of(&key);
    let stored = files.read(TILL_KEY_FILE).unwrap().unwrap();
    let reply = till_reply_key(&key_bytes(&key));
    for wrong in [
        String::new(),
        token[..63].to_string(),
        format!("{token}0"),
        token.to_uppercase(),
        "g".repeat(64),
        "0".repeat(64),
        format!(" {token}"),
        // The key itself is not the token.
        key.clone(),
    ] {
        assert_eq!(
            check_till_key(Some(&stored), Some(&wrong)),
            KeyCheck::Wrong(Some(reply)),
            "{wrong}"
        );
    }
    assert_eq!(
        check_till_key(Some(&stored), None),
        KeyCheck::Wrong(Some(reply))
    );
    // No file, and a file of a shape that is not either form, match nothing
    // and hold no reply key to sign with.
    assert_eq!(check_till_key(None, Some(&token)), KeyCheck::Wrong(None));
    assert_eq!(
        check_till_key(Some(&[]), Some(&token)),
        KeyCheck::Wrong(None)
    );
    assert_eq!(
        check_till_key(Some(&stored[..64]), Some(&token)),
        KeyCheck::Wrong(None)
    );
    let mut other_version = stored.clone();
    other_version[0] = 3;
    assert_eq!(
        check_till_key(Some(&other_version), Some(&token)),
        KeyCheck::Wrong(None)
    );
}

#[test]
fn bx_80_a_key_file_of_the_earlier_form_is_old_whatever_the_token() {
    let key = [9u8; 32];
    let old = till_key_hash(&till_bearer(&key));
    for presented in [None, Some(hex(&till_bearer(&key))), Some("x".to_string())] {
        assert_eq!(
            check_till_key(Some(&old), presented.as_deref()),
            KeyCheck::Old
        );
    }
}

#[test]
fn bx_80_the_token_and_the_reply_key_are_tagged_hashes_of_the_key() {
    let key = [7u8; 32];
    assert_eq!(
        till_bearer(&key),
        openbmt_core::identity::tagged_hash("OpenBMT/till-bearer/v1", &key)
    );
    assert_eq!(
        till_reply_key(&key),
        openbmt_core::identity::tagged_hash("OpenBMT/till-reply/v1", &key)
    );
    assert_ne!(till_bearer(&key), till_reply_key(&key));
    assert_ne!(till_bearer(&key), key);
    // The stored form is the version, the hash of the token, the reply key.
    let file = key_file(&key);
    assert_eq!(file.first(), Some(&KEY_FILE_VERSION));
    assert_eq!(
        file.get(1..33),
        Some(&till_key_hash(&till_bearer(&key))[..])
    );
    assert_eq!(file.get(33..), Some(&till_reply_key(&key)[..]));
}

#[test]
fn bx_80_the_mac_is_hmac_sha256_over_the_lines_of_the_answer() {
    // RFC 4231, case 2, for the HMAC itself.
    assert_eq!(
        hex(&hmac_sha256(b"Jefe", b"what do ya want for nothing?")),
        "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
    );
    // FIPS 180 vectors for the hash of the request body: of no bytes, and
    // of "abc".
    assert_eq!(
        hex(&sha256_of(b"")),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
    assert_eq!(
        hex(&sha256_of(b"abc")),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    let key = [1u8; 32];
    let none = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
    let want = hex(&hmac_sha256(
        &key,
        format!("OBM-TILL-REPLY-2\nGET\n/till/v1/sales/{SALE}\n{NONCE}\n{none}\n404\n{{\"a\":1}}")
            .as_bytes(),
    ));
    let path = format!("/till/v1/sales/{SALE}");
    assert_eq!(
        reply_mac(&key, "GET", &path, NONCE, b"", 404, b"{\"a\":1}"),
        want
    );
    // Each part counts, the request's body too.
    for other in [
        reply_mac(&[2u8; 32], "GET", &path, NONCE, b"", 404, b"{\"a\":1}"),
        reply_mac(&key, "POST", &path, NONCE, b"", 404, b"{\"a\":1}"),
        reply_mac(&key, "GET", "/till/v1/sales", NONCE, b"", 404, b"{\"a\":1}"),
        reply_mac(
            &key,
            "GET",
            &path,
            &NONCE.replace('0', "1"),
            b"",
            404,
            b"{\"a\":1}",
        ),
        reply_mac(&key, "GET", &path, NONCE, b"x", 404, b"{\"a\":1}"),
        reply_mac(&key, "GET", &path, NONCE, b"", 200, b"{\"a\":1}"),
        reply_mac(&key, "GET", &path, NONCE, b"", 404, b"{\"a\":2}"),
    ] {
        assert_ne!(other, want);
    }
    // The status is three digits, and a body is hashed as its bytes.
    let abc = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
    assert_eq!(
        reply_mac(&key, "GET", &path, NONCE, b"abc", 99, b""),
        hex(&hmac_sha256(
            &key,
            format!("OBM-TILL-REPLY-2\nGET\n{path}\n{NONCE}\n{abc}\n099\n").as_bytes()
        ))
    );
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
    assert_eq!(on_disk.len(), KEY_FILE_LEN);
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
        digits: 2,
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
    let info = |state| SaleInfo {
        state,
        amount: "12.50".into(),
        currency: "USD".into(),
        request: "order-1".into(),
        reference: "Order \"1\"".into(),
    };
    for (state, word) in [
        (SaleState::Waiting, "waiting"),
        (SaleState::Opened, "opened"),
    ] {
        assert_eq!(
            sale_body(&id, &info(state)),
            format!(
                "{{\"sale\":\"{hexid}\",\"state\":\"{word}\",\"amount\":\"12.50\",\
                 \"currency\":\"USD\",\"request\":\"order-1\",\"reference\":\"Order \\\"1\\\"\"}}"
            )
        );
    }
    let paid = sale_body(&id, &info(SaleState::Paid));
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
        let body = sale_body(&id, &info(state));
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
            | TillAnswer::KeyOld
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
    assert_eq!(till_samples().len(), 13);
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
    let (status, body, _) = http_mac(addr, raw);
    (status, body)
}

/// A raw request and the status, the body and the `X-OBM-Reply` header of
/// the answer, if it has one.
fn http_mac(addr: SocketAddr, raw: &str) -> (u16, String, Option<String>) {
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
    let mac = head.split("\r\n").find_map(|line| {
        line.strip_prefix("X-OBM-Reply: ")
            .map(std::string::ToString::to_string)
    });
    assert!(
        head.matches("X-OBM-Reply").count() <= 1,
        "one mac at most: {head}"
    );
    (status, body.to_string(), mac)
}

/// A request of the till for the till key as the Box printed it: the bearer
/// token is made from the key, as the till makes it. A string that is not a
/// key is sent as it is. The nonce is the one given, or a fixed one.
fn request_text(
    method: &str,
    path: &str,
    key: Option<&str>,
    nonce: Option<&str>,
    body: Option<&str>,
) -> String {
    let auth = key.map_or(String::new(), |k| {
        let token = if k.len() == 64
            && k.bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        {
            token_of(k)
        } else {
            k.to_string()
        };
        format!("Authorization: Bearer {token}\r\n")
    });
    let nonce = nonce.map_or(String::new(), |n| format!("X-OBM-Nonce: {n}\r\n"));
    match body {
        Some(b) => format!(
            "{method} {path} HTTP/1.1\r\nHost: box\r\n{nonce}{auth}Content-Type: application/json\r\n\
             Content-Length: {}\r\n\r\n{b}",
            b.len()
        ),
        None => format!("{method} {path} HTTP/1.1\r\nHost: box\r\n{nonce}{auth}\r\n"),
    }
}

fn call(
    addr: SocketAddr,
    method: &str,
    path: &str,
    key: Option<&str>,
    body: Option<&str>,
) -> (u16, String) {
    let (status, body, _) = call_mac(addr, method, path, key, Some(NONCE), body);
    (status, body)
}

fn call_mac(
    addr: SocketAddr,
    method: &str,
    path: &str,
    key: Option<&str>,
    nonce: Option<&str>,
    body: Option<&str>,
) -> (u16, String, Option<String>) {
    http_mac(addr, &request_text(method, path, key, nonce, body))
}

/// Whether the mac is the one the reply key makes for the answer.
fn mac_is(
    key: &str,
    method: &str,
    path: &str,
    nonce: &str,
    request_body: &str,
    answer: &(u16, String, Option<String>),
) -> bool {
    answer.2.as_deref()
        == Some(&reply_mac(
            &till_reply_key(&key_bytes(key)),
            method,
            path,
            nonce,
            request_body.as_bytes(),
            answer.0,
            answer.1.as_bytes(),
        ))
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
        let answer = call_mac(self.addr, method, path, Some(&self.key), Some(NONCE), body);
        assert!(
            mac_is(&self.key, method, path, NONCE, body.unwrap_or(""), &answer),
            "{method} {path}: {answer:?}"
        );
        (answer.0, answer.1)
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
        "GET /till/v1/sales/{SALE} HTTP/1.1\r\nHost: b\r\n{NONCE_HEADER}Authorization: Basic {}\r\n\r\n",
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
fn bx_71_a_caller_with_the_wrong_token_never_waits_for_the_core() {
    let dir = TempDir::new("tillnolock");
    let (mut r, _) = start(&dir);
    let key = r.make_till_key().unwrap();
    let addr = r
        .serve_till("127.0.0.1:0".parse().unwrap(), TillConfig::default())
        .unwrap();
    let path = format!("/till/v1/sales/{SALE}");
    // Another part of the Box holds the core for the whole of this block.
    let core = r.core().lock().unwrap();
    let started = Instant::now();
    for bearer in ["0".repeat(64), "x".to_string(), String::new()] {
        let raw = format!(
            "GET {path} HTTP/1.1\r\nHost: b\r\n{NONCE_HEADER}Authorization: Bearer {bearer}\r\n\r\n"
        );
        let (status, body, mac) = http_mac(addr, &raw);
        assert_eq!(status, 401, "{bearer}");
        assert_eq!(field(&body, "error"), "tillKeyWrong");
        assert!(mac.is_some());
    }
    let (status, body, _) = http_mac(
        addr,
        &format!("GET {path} HTTP/1.1\r\nHost: b\r\n{NONCE_HEADER}\r\n"),
    );
    assert_eq!(
        (status, field(&body, "error").as_str()),
        (401, "tillKeyWrong")
    );
    assert!(started.elapsed() < Duration::from_secs(5));
    drop(core);
    // With the core free the right key goes on to the state.
    let (status, _) = call(addr, "GET", &path, Some(&key), None);
    assert_eq!(status, 503);
}

#[test]
fn bx_71_a_key_made_by_another_process_is_honoured_at_the_next_request() {
    let dir = TempDir::new("tillrotate");
    let (mut r, _) = start(&dir);
    let old = r.make_till_key().unwrap();
    let addr = r
        .serve_till("127.0.0.1:0".parse().unwrap(), TillConfig::default())
        .unwrap();
    let path = format!("/till/v1/sales/{SALE}");
    assert_eq!(call(addr, "GET", &path, Some(&old), None).0, 503);
    // `openbmt-box till-key` on the data folder of a Box that is running.
    let new = make_till_key(&mut DirFiles::new(dir.path())).unwrap();
    assert_eq!(call(addr, "GET", &path, Some(&old), None).0, 401);
    assert_eq!(call(addr, "GET", &path, Some(&new), None).0, 503);
    // The key the running Box makes is honoured at once, and the old one stops.
    let newer = r.make_till_key().unwrap();
    assert_eq!(call(addr, "GET", &path, Some(&new), None).0, 401);
    assert_eq!(call(addr, "GET", &path, Some(&newer), None).0, 503);
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
    fn info(state: SaleState) -> SaleInfo {
        SaleInfo {
            state,
            amount: "1.00".into(),
            currency: "USD".into(),
            request: "q".into(),
            reference: String::new(),
        }
    }
    impl TillBackend for Fixed {
        fn check_key(&self, _: Option<&str>) -> KeyCheck {
            KeyCheck::Good([3u8; 32])
        }
        fn create(&self, _: CreateBody) -> Result<([u8; 16], SaleInfo), TillRefusal> {
            Err(TillRefusal::Busy)
        }
        fn read(&self, _: &[u8; 16]) -> Result<SaleInfo, TillRefusal> {
            Ok(info(SaleState::Waiting))
        }
        fn cancel(&self, _: &[u8; 16]) -> Result<SaleInfo, TillRefusal> {
            Ok(info(SaleState::Cancelled))
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
    let (status, _) = http(
        addr,
        &format!("GET /till/v1/sales/{SALE} HTTP/1.1\r\n{NONCE_HEADER}\r\n"),
    );
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

// ---- the signed answers (BX-80) -----------------------------------------

const NONCE_B: &str = "fedcba9876543210fedcba9876543210";

#[test]
fn bx_80_every_answer_to_a_request_with_a_nonce_and_a_key_carries_the_mac() {
    let t = till_box("tillmac");
    let sale = t.sale("order-1");
    let sale_path = format!("/till/v1/sales/{sale}");
    let lost_path = format!("/till/v1/sales/{SALE}");
    let good = sale_json(&counter1(), "1.00", "USD", "r", "q");
    let wrong_money = sale_json(&counter1(), "1.00", "EUR", "r", "q2");
    let bad_json = "{\"a\":1}".to_string();
    let cases: Vec<(&str, String, Option<String>, u16)> = vec![
        ("POST", "/till/v1/sales".into(), Some(good), 200),
        ("POST", "/till/v1/sales".into(), Some(wrong_money), 409),
        ("POST", "/till/v1/sales".into(), Some(bad_json), 400),
        ("GET", sale_path.clone(), None, 200),
        ("GET", lost_path.clone(), None, 404),
        ("POST", format!("{sale_path}/cancel"), None, 200),
        ("POST", format!("{lost_path}/cancel"), None, 404),
    ];
    for (method, path, body, want) in cases {
        let answer = call_mac(
            t.addr,
            method,
            &path,
            Some(&t.key),
            Some(NONCE),
            body.as_deref(),
        );
        let sent = body.as_deref().unwrap_or("");
        assert_eq!(answer.0, want, "{method} {path}: {}", answer.1);
        assert!(
            mac_is(&t.key, method, &path, NONCE, sent, &answer),
            "{method} {path}"
        );
        let mac = answer.2.as_deref().unwrap();
        assert_eq!(mac.len(), 64);
        assert!(mac
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)));
        // Another nonce, another method or path, or another key does not
        // verify the same answer.
        assert!(!mac_is(&t.key, method, &path, NONCE_B, sent, &answer));
        assert!(!mac_is(&t.key, "PUT", &path, NONCE, sent, &answer));
        assert!(
            !mac_is(&t.key, method, "/till/v1/sales", NONCE, sent, &answer)
                || path == "/till/v1/sales"
        );
        assert!(!mac_is(
            &"5".repeat(64),
            method,
            &path,
            NONCE,
            sent,
            &answer
        ));
        // Changed status or body does not verify.
        let mut other = answer.clone();
        other.0 = if other.0 == 200 { 404 } else { 200 };
        assert!(!mac_is(&t.key, method, &path, NONCE, sent, &other));
        let mut other = answer.clone();
        other.1.push(' ');
        assert!(!mac_is(&t.key, method, &path, NONCE, sent, &other));
    }
}

#[test]
fn bx_80_the_same_request_with_another_nonce_gets_another_mac() {
    let t = till_box("tillnonce");
    let path = format!("/till/v1/sales/{SALE}");
    let a = call_mac(t.addr, "GET", &path, Some(&t.key), Some(NONCE), None);
    let b = call_mac(t.addr, "GET", &path, Some(&t.key), Some(NONCE_B), None);
    assert_eq!(a.1, b.1);
    assert_ne!(a.2, b.2);
    assert!(mac_is(&t.key, "GET", &path, NONCE, "", &a));
    assert!(!mac_is(&t.key, "GET", &path, NONCE, "", &b));
    assert!(mac_is(&t.key, "GET", &path, NONCE_B, "", &b));
}

#[test]
fn bx_80_a_wrong_token_is_refused_with_the_mac_of_the_reply_key() {
    let t = till_box("tillwrongmac");
    let path = format!("/till/v1/sales/{SALE}");
    // The key itself is not the bearer token.
    for bearer in [t.key.clone(), "0".repeat(64), "x".to_string()] {
        let answer = call_mac(t.addr, "GET", &path, Some(&bearer), Some(NONCE), None);
        // `request_text` turns a key into its token, so send the text as it
        // is.
        let raw = format!(
            "GET {path} HTTP/1.1\r\nHost: b\r\n{NONCE_HEADER}Authorization: Bearer {bearer}\r\n\r\n"
        );
        let raw = http_mac(t.addr, &raw);
        assert_eq!(raw.0, 401, "{bearer}");
        assert_eq!(field(&raw.1, "error"), "tillKeyWrong");
        assert!(mac_is(&t.key, "GET", &path, NONCE, "", &raw), "{bearer}");
        assert_eq!(answer.0 == 401, bearer != t.key);
    }
}

#[test]
fn bx_80_a_request_without_a_good_nonce_is_a_400_with_no_mac_and_says_nothing_of_the_key() {
    let t = till_box("tillnonce400");
    let path = format!("/till/v1/sales/{SALE}");
    let body = sale_json(&counter1(), "1.00", "USD", "r", "q");
    for nonce in [
        None,
        Some(""),
        Some(&NONCE[..31]),
        Some(&format!("{NONCE}0")[..]),
        Some(&NONCE.to_uppercase()[..]),
        Some("g123456789abcdef0123456789abcdef"),
    ] {
        for key in [None, Some("x"), Some(t.key.as_str())] {
            for (method, path, b) in [
                ("POST", "/till/v1/sales", Some(body.as_str())),
                ("GET", path.as_str(), None),
                ("POST", &format!("{path}/cancel")[..], None),
            ] {
                let (status, answer, mac) = call_mac(t.addr, method, path, key, nonce, b);
                assert_eq!(status, 400, "{method} {path} {nonce:?} {key:?}");
                assert_eq!(answer, "{\"error\":\"tillBadRequest\"}");
                assert_eq!(mac, None);
            }
        }
    }
    // Twice is no nonce.
    let raw = format!(
        "GET {path} HTTP/1.1\r\n{NONCE_HEADER}{NONCE_HEADER}Authorization: Bearer {}\r\n\r\n",
        token_of(&t.key)
    );
    assert_eq!(
        http_mac(t.addr, &raw),
        (400, "{\"error\":\"tillBadRequest\"}".to_string(), None)
    );
    // A path that does not exist is still a 404, with or without a nonce.
    let (status, answer, mac) = call_mac(t.addr, "GET", "/nope", Some(&t.key), None, None);
    assert_eq!(
        (status, answer.as_str(), mac),
        (404, "{\"error\":\"tillNotFound\"}", None)
    );
}

#[test]
fn bx_80_an_answer_with_no_nonce_or_no_key_to_sign_under_carries_no_mac() {
    let t = till_box("tillnomac");
    // 404 not found, 400 that does not read, 413 over the limit.
    let (status, _, mac) = call_mac(t.addr, "GET", "/", Some(&t.key), Some(NONCE), None);
    assert_eq!((status, mac), (404, None));
    let raw = format!(
        "GET /till/v1/sales/{SALE} HTTP/1.1\r\nHost: b\r\n{NONCE_HEADER}Bad Header\r\n\r\n"
    );
    let (status, _, mac) = http_mac(t.addr, &raw);
    assert_eq!((status, mac), (400, None));
    let raw = format!(
        "POST /till/v1/sales HTTP/1.1\r\nHost: b\r\n{NONCE_HEADER}Content-Type: application/json\r\n\
         Content-Length: {MAX_REQUEST}\r\n\r\n"
    );
    let (status, _, mac) = http_mac(t.addr, &raw);
    assert_eq!((status, mac), (413, None));
}

#[test]
fn bx_80_a_box_with_no_till_key_made_signs_nothing() {
    let dir = TempDir::new("tillnokeymac");
    let (mut r, _) = start(&dir);
    pair_ok(&r);
    let addr = r
        .serve_till("127.0.0.1:0".parse().unwrap(), TillConfig::default())
        .unwrap();
    let path = format!("/till/v1/sales/{SALE}");
    let (status, body, mac) =
        call_mac(addr, "GET", &path, Some(&"0".repeat(64)), Some(NONCE), None);
    assert_eq!(
        (status, field(&body, "error").as_str(), mac),
        (401, "tillKeyWrong", None)
    );
}

#[test]
fn bx_80_a_key_file_of_the_earlier_form_is_a_401_key_old_and_signs_nothing() {
    let dir = TempDir::new("tillkeyold");
    let (mut r, _) = start(&dir);
    pair_ok(&r);
    let addr = r
        .serve_till("127.0.0.1:0".parse().unwrap(), TillConfig::default())
        .unwrap();
    let key = [4u8; 32];
    // The file as it was written before: the hash of the key alone.
    std::fs::write(
        dir.path().join(TILL_KEY_FILE),
        openbmt_core::identity::tagged_hash("OpenBMT/till-key/v1", &key),
    )
    .unwrap();
    let path = format!("/till/v1/sales/{SALE}");
    for bearer in [hex(&key), hex(&till_bearer(&key)), "x".to_string()] {
        let raw = format!(
            "GET {path} HTTP/1.1\r\nHost: b\r\n{NONCE_HEADER}Authorization: Bearer {bearer}\r\n\r\n"
        );
        let (status, body, mac) = http_mac(addr, &raw);
        assert_eq!(status, 401, "{bearer}");
        assert_eq!(field(&body, "error"), "tillKeyOld");
        assert_eq!(field(&body, "heading"), "Till key too old");
        assert_eq!(mac, None);
    }
    // A new key makes the file whole again, at the next request.
    let new = r.make_till_key().unwrap();
    let (status, _, mac) = call_mac(addr, "GET", &path, Some(&new), Some(NONCE), None);
    assert_eq!(status, 404);
    assert!(mac.is_some());
}

// ---- the vectors the Odoo module's tests read (BX-80) -------------------

/// The text of `integrations/odoo/pos_openbmt/tests_standalone/
/// till_vectors.json`: a fixed key with its token, its reply key and its
/// file, and some requests with the answer and the mac the Box makes.
fn till_vectors() -> String {
    let key: [u8; 32] = std::array::from_fn(|i| u8::try_from(i).unwrap());
    let reply = till_reply_key(&key);
    let sale = "00112233445566778899aabbccddeeff";
    let created = "{\"counter\":\"1a2b3c4d\",\"amount\":\"12.50\",\"currency\":\"USD\",\"reference\":\"Order 1\",\"request\":\"order-1\"}";
    let cases: [(&str, String, &str, &str, u16, &str); 5] = [
        (
            "POST",
            "/till/v1/sales".to_string(),
            "0123456789abcdef0123456789abcdef",
            created,
            200,
            "{\"sale\":\"00112233445566778899aabbccddeeff\",\"state\":\"waiting\",\"amount\":\"12.50\",\"currency\":\"USD\",\"request\":\"order-1\",\"reference\":\"Order 1\"}",
        ),
        (
            "GET",
            format!("/till/v1/sales/{sale}"),
            "fedcba9876543210fedcba9876543210",
            "",
            200,
            "{\"sale\":\"00112233445566778899aabbccddeeff\",\"state\":\"paid\",\"amount\":\"12.50\",\"currency\":\"USD\",\"request\":\"order-1\",\"reference\":\"Order 1\",\"words\":{\"heading\":\"\",\"what\":\"Paid in bitcoin.\",\"next\":\"\"}}",
        ),
        (
            "POST",
            format!("/till/v1/sales/{sale}/cancel"),
            "00000000000000000000000000000000",
            "",
            404,
            "{\"error\":\"tillLost\"}",
        ),
        (
            "GET",
            format!("/till/v1/sales/{sale}"),
            "ffffffffffffffffffffffffffffffff",
            "",
            401,
            "{\"error\":\"tillKeyWrong\"}",
        ),
        // Bytes that are not plain ASCII, and a line feed at the end.
        (
            "GET",
            format!("/till/v1/sales/{sale}"),
            "0f1e2d3c4b5a69788796a5b4c3d2e1f0",
            "",
            200,
            "{\"a\":\"caf\u{e9}\"}\n",
        ),
    ];
    let quote = |text: &str| {
        let mut out = String::from("\"");
        for c in text.chars() {
            match c {
                '"' => out.push_str("\\\""),
                '\\' => out.push_str("\\\\"),
                '\n' => out.push_str("\\n"),
                c => out.push(c),
            }
        }
        out.push('"');
        out
    };
    let mut out = String::from("{\n");
    out.push_str(&format!("  \"key\": \"{}\",\n", hex(&key)));
    out.push_str(&format!("  \"token\": \"{}\",\n", hex(&till_bearer(&key))));
    out.push_str(&format!("  \"reply_key\": \"{}\",\n", hex(&reply)));
    out.push_str(&format!("  \"key_file\": \"{}\",\n", hex(&key_file(&key))));
    out.push_str("  \"cases\": [\n");
    for (i, (method, path, nonce, sent, status, body)) in cases.iter().enumerate() {
        let mac = reply_mac(
            &reply,
            method,
            path,
            nonce,
            sent.as_bytes(),
            *status,
            body.as_bytes(),
        );
        out.push_str(&format!(
            "    {{\"method\": {}, \"path\": {}, \"nonce\": {}, \"request_body\": {}, \"status\": {status}, \"body\": {}, \"mac\": {}}}{}\n",
            quote(method),
            quote(path),
            quote(nonce),
            quote(sent),
            quote(body),
            quote(&mac),
            if i + 1 < cases.len() { "," } else { "" }
        ));
    }
    out.push_str("  ]\n}\n");
    out
}

#[test]
fn bx_80_the_vectors_file_is_what_the_code_makes() {
    let path = std::path::PathBuf::from(
        std::env::var_os("CARGO_MANIFEST_DIR")
            .expect("cargo sets CARGO_MANIFEST_DIR when it runs tests"),
    )
    .join("../../integrations/odoo/pos_openbmt/tests_standalone/till_vectors.json");
    let want = till_vectors();
    if std::env::var_os("OPENBMT_BLESS_TILL").is_some() {
        std::fs::write(&path, &want).expect("write till_vectors.json");
    }
    let have = std::fs::read_to_string(&path).unwrap_or_default();
    assert!(
        have == want,
        "till_vectors.json is not what the Box makes: run this test with OPENBMT_BLESS_TILL=1 to write it again"
    );
}

// ---- the request is bound into the answer (BX-72, BX-73, BX-80) ---------

#[test]
fn bx_72_an_amount_is_told_with_exactly_the_minor_unit_decimals() {
    assert_eq!(format_amount(1250, 2), "12.50");
    assert_eq!(format_amount(5, 2), "0.05");
    assert_eq!(format_amount(100, 2), "1.00");
    assert_eq!(format_amount(0, 2), "0.00");
    assert_eq!(format_amount(1250, 0), "1250");
    assert_eq!(format_amount(u64::MAX, 2), "184467440737095516.15");
    // What the till wrote comes back as the same number of minor units.
    for (text, digits) in [("12.5", 2), ("12", 2), ("0.05", 2), ("700", 0)] {
        let minor = parse_amount(text, digits).unwrap();
        assert_eq!(
            parse_amount(&format_amount(minor, digits), digits),
            Some(minor)
        );
    }
}

#[test]
fn bx_73_a_sale_is_told_with_the_amount_currency_request_and_reference_the_box_took() {
    let t = till_box("tillfields");
    // The amount is written in full and the reference is cut and cleaned.
    let long = "R".repeat(60);
    let (status, body) = t.ask(
        "POST",
        "/till/v1/sales",
        Some(&sale_json(&counter1(), "7.5", "USD", &long, "q-1")),
    );
    assert_eq!(status, 200, "{body}");
    assert_eq!(field(&body, "amount"), "7.50");
    assert_eq!(field(&body, "currency"), "USD");
    assert_eq!(field(&body, "request"), "q-1");
    assert_eq!(field(&body, "reference"), "R".repeat(40));
    let sale = field(&body, "sale");
    // A status poll and a cancel tell the same four.
    let (_, polled) = t.state(&sale);
    let (_, cancelled) = t.ask("POST", &format!("/till/v1/sales/{sale}/cancel"), None);
    for text in [&polled, &cancelled] {
        assert_eq!(field(text, "amount"), "7.50");
        assert_eq!(field(text, "currency"), "USD");
        assert_eq!(field(text, "request"), "q-1");
        assert_eq!(field(text, "reference"), "R".repeat(40));
    }
    // A sale with no reference tells it empty.
    let none = "{\"counter\":\"".to_string()
        + &counter_code(&counter1().device_id())
        + "\",\"amount\":\"1.00\",\"currency\":\"USD\",\"request\":\"q-2\"}";
    let (_, body) = t.ask("POST", "/till/v1/sales", Some(&none));
    assert_eq!(field(&body, "reference"), "");
    // The same request again with another amount is told the first one, so
    // a till that compares sees the difference.
    let (_, again) = t.ask(
        "POST",
        "/till/v1/sales",
        Some(&sale_json(&counter1(), "9.00", "USD", "x", "q-2")),
    );
    assert_eq!(field(&again, "amount"), "1.00");
}

#[test]
fn bx_80_a_changed_request_body_changes_the_mac() {
    let t = till_box("tillbodymac");
    let path = "/till/v1/sales";
    let real = sale_json(&counter1(), "100.00", "USD", "Order 7", "q-body");
    let changed = sale_json(&counter1(), "10.00", "USD", "Order 7", "q-body");
    let a = call_mac(t.addr, "POST", path, Some(&t.key), Some(NONCE), Some(&real));
    // The same request again carries the same mac; one body in its place does
    // not, and the mac of one does not verify for the other.
    let again = call_mac(t.addr, "POST", path, Some(&t.key), Some(NONCE), Some(&real));
    assert_eq!(a.2, again.2);
    let b = call_mac(
        t.addr,
        "POST",
        path,
        Some(&t.key),
        Some(NONCE),
        Some(&changed),
    );
    assert_ne!(a.2, b.2);
    assert!(mac_is(&t.key, "POST", path, NONCE, &real, &a));
    assert!(mac_is(&t.key, "POST", path, NONCE, &changed, &b));
    assert!(!mac_is(&t.key, "POST", path, NONCE, &changed, &a));
    assert!(!mac_is(&t.key, "POST", path, NONCE, &real, &b));
    // The bytes as received count, not the fields: the same fields in another
    // order or with a space are another body and another mac.
    let spaced = real.replace(",\"currency\"", ", \"currency\"");
    let c = call_mac(
        t.addr,
        "POST",
        path,
        Some(&t.key),
        Some(NONCE),
        Some(&spaced),
    );
    assert_eq!(a.1, c.1);
    assert_ne!(a.2, c.2);
    assert!(mac_is(&t.key, "POST", path, NONCE, &spaced, &c));
}
