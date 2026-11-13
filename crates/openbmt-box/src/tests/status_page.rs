//! The status page (BX-54 to BX-61): the reader, the page, the headers, the
//! invitation form, the origin check, what never appears, and the relay
//! beside it. BX-61 is tested as a process in `tests/no_console.rs`.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use openbmt_core::say::{Tone, Who, Words};

use super::kit::*;
use super::runner::{connect, eventually, mailbox, pair_ok, send_settings, start, start_in};
use crate::say::{all_words, ConsoleAnswer, PageAnswer};
use crate::state::{BoxError, BoxStatus};
use crate::status_page::{
    form_field, parse_request, refusal_response, render_home, same_origin, Entered, PageBackend,
    PageConfig, PageView, Refusal, Route, StatusPage, MAX_BODY, MAX_HEAD,
};
use crate::{BoxRunner, KEY_FILE};

// ---- the reader -------------------------------------------------------

fn get(host: &str) -> Vec<u8> {
    format!("GET / HTTP/1.1\r\nHost: {host}\r\n\r\n").into_bytes()
}

fn post(host: &str, extra: &str, body: &str) -> Vec<u8> {
    format!(
        "POST /invitation HTTP/1.1\r\nHost: {host}\r\n{extra}\
         Content-Type: application/x-www-form-urlencoded\r\n\
         Content-Length: {}\r\n\r\n{body}",
        body.len()
    )
    .into_bytes()
}

fn read(bytes: &[u8]) -> Result<Option<crate::status_page::Request>, Refusal> {
    parse_request(bytes)
}

#[test]
fn bx_54_the_reader_reads_a_get_and_a_post() {
    let r = read(&get("box.local:8987")).unwrap().unwrap();
    assert_eq!(r.route, Route::Home);
    assert_eq!(r.host, "box.local:8987");
    assert!(r.body.is_empty());
    let r = read(&post(
        "box.local",
        "Origin: http://box.local\r\n",
        "invitation=ab",
    ))
    .unwrap()
    .unwrap();
    assert_eq!(r.route, Route::Invitation);
    assert_eq!(r.origin.as_deref(), Some("http://box.local"));
    assert_eq!(r.body, b"invitation=ab");
}

#[test]
fn bx_54_the_reader_asks_for_more_until_the_request_is_whole() {
    let whole = post("h", "", "invitation=abc");
    for cut in 0..whole.len() {
        assert_eq!(read(&whole[..cut]), Ok(None), "cut at {cut}");
    }
    assert!(read(&whole).unwrap().is_some());
    // Bytes after the body are not looked at.
    let mut more = whole.clone();
    more.extend_from_slice(b"GET / HTTP/1.1\r\n\r\n\x00\xff");
    assert!(read(&more).unwrap().is_some());
}

#[test]
fn bx_54_the_reader_refuses_what_is_not_a_request_with_400() {
    let cases: Vec<(&str, Vec<u8>)> = vec![
        ("no host", b"GET / HTTP/1.1\r\n\r\n".to_vec()),
        ("empty host", b"GET / HTTP/1.1\r\nHost:\r\n\r\n".to_vec()),
        (
            "two hosts",
            b"GET / HTTP/1.1\r\nHost: a\r\nHost: b\r\n\r\n".to_vec(),
        ),
        ("http 1.0", b"GET / HTTP/1.0\r\nHost: a\r\n\r\n".to_vec()),
        ("http 2", b"GET / HTTP/2\r\nHost: a\r\n\r\n".to_vec()),
        ("no version", b"GET /\r\nHost: a\r\n\r\n".to_vec()),
        ("two spaces", b"GET  / HTTP/1.1\r\nHost: a\r\n\r\n".to_vec()),
        (
            "absolute target",
            b"GET http://a/ HTTP/1.1\r\nHost: a\r\n\r\n".to_vec(),
        ),
        (
            "bare lf in a head",
            b"GET / HTTP/1.1\r\nHost: a\nX: y\r\n\r\n".to_vec(),
        ),
        (
            "bare cr in a value",
            b"GET / HTTP/1.1\r\nHost: a\rb\r\n\r\n".to_vec(),
        ),
        (
            "folded header",
            b"GET / HTTP/1.1\r\nHost: a\r\n x: y\r\n\r\n".to_vec(),
        ),
        (
            "no colon",
            b"GET / HTTP/1.1\r\nHost: a\r\nnonsense\r\n\r\n".to_vec(),
        ),
        (
            "space in a name",
            b"GET / HTTP/1.1\r\nHost: a\r\nX Y: z\r\n\r\n".to_vec(),
        ),
        (
            "control byte",
            b"GET / HTTP/1.1\r\nHost: a\r\nX: \x01\r\n\r\n".to_vec(),
        ),
        (
            "non ascii",
            "GET / HTTP/1.1\r\nHost: a\r\nX: \u{e9}\r\n\r\n"
                .as_bytes()
                .to_vec(),
        ),
        (
            "transfer encoding",
            b"GET / HTTP/1.1\r\nHost: a\r\nTransfer-Encoding: chunked\r\n\r\n".to_vec(),
        ),
        (
            "get with a body",
            b"GET / HTTP/1.1\r\nHost: a\r\nContent-Length: 3\r\n\r\nabc".to_vec(),
        ),
        (
            "post without a length",
            b"POST /invitation HTTP/1.1\r\nHost: a\r\n\
              Content-Type: application/x-www-form-urlencoded\r\n\r\n"
                .to_vec(),
        ),
        (
            "post of another type",
            b"POST /invitation HTTP/1.1\r\nHost: a\r\nContent-Type: text/plain\r\n\
              Content-Length: 1\r\n\r\nx"
                .to_vec(),
        ),
        (
            "post without a type",
            b"POST /invitation HTTP/1.1\r\nHost: a\r\nContent-Length: 1\r\n\r\nx".to_vec(),
        ),
        (
            "two lengths",
            b"POST /invitation HTTP/1.1\r\nHost: a\r\nContent-Length: 1\r\n\
              Content-Length: 1\r\nContent-Type: application/x-www-form-urlencoded\r\n\r\nx"
                .to_vec(),
        ),
        (
            "signed length",
            b"POST /invitation HTTP/1.1\r\nHost: a\r\nContent-Length: +1\r\n\
              Content-Type: application/x-www-form-urlencoded\r\n\r\nx"
                .to_vec(),
        ),
        (
            "two origins",
            post("a", "Origin: http://a\r\nOrigin: http://a\r\n", "x"),
        ),
    ];
    for (name, bytes) in cases {
        assert_eq!(read(&bytes), Err(Refusal::BadRequest), "{name}");
    }
}

#[test]
fn bx_54_the_reader_answers_404_and_405_for_other_pages_and_methods() {
    let not_found =
        |target: &str| read(format!("GET {target} HTTP/1.1\r\nHost: a\r\n\r\n").as_bytes());
    for target in [
        "/index.html",
        "/invitation/",
        "/?x=1",
        "/%2f",
        "//",
        "/invitation?x",
    ] {
        assert_eq!(not_found(target), Err(Refusal::NotFound), "{target}");
    }
    // The target is matched exactly, so the query is another page.
    assert_eq!(
        read(b"POST / HTTP/1.1\r\nHost: a\r\n\r\n"),
        Err(Refusal::MethodNotAllowed(Route::Home))
    );
    for method in ["HEAD", "PUT", "DELETE", "OPTIONS", "PATCH"] {
        assert_eq!(
            read(format!("{method} / HTTP/1.1\r\nHost: a\r\n\r\n").as_bytes()),
            Err(Refusal::MethodNotAllowed(Route::Home)),
            "{method}"
        );
    }
    assert_eq!(
        read(b"get / HTTP/1.1\r\nHost: a\r\n\r\n"),
        Err(Refusal::MethodNotAllowed(Route::Home))
    );
    // A head with line feeds alone never ends; the timeout answers it.
    assert_eq!(read(b"GET / HTTP/1.1\nHost: a\n\n"), Ok(None));
    assert_eq!(
        read(b"GET /invitation HTTP/1.1\r\nHost: a\r\n\r\n"),
        Err(Refusal::MethodNotAllowed(Route::Invitation))
    );
    // Another method on another page is no page.
    assert_eq!(
        read(b"PUT /x HTTP/1.1\r\nHost: a\r\n\r\n"),
        Err(Refusal::NotFound)
    );
    let allow = |r: Refusal| String::from_utf8(refusal_response(r)).unwrap();
    assert!(allow(Refusal::MethodNotAllowed(Route::Home)).contains("Allow: GET\r\n"));
    assert!(allow(Refusal::MethodNotAllowed(Route::Invitation)).contains("Allow: POST\r\n"));
}

#[test]
fn bx_54_the_head_is_at_most_eight_kib_and_the_body_one_kib() {
    // A head of exactly MAX_HEAD bytes passes, one more is 413.
    let build = |len: usize| {
        let prefix = "GET / HTTP/1.1\r\nHost: a\r\nX: ";
        let fill = len - prefix.len();
        format!("{prefix}{}\r\n\r\n", "a".repeat(fill)).into_bytes()
    };
    // The head counts without the blank line that ends it.
    assert!(read(&build(MAX_HEAD)).unwrap().is_some());
    assert_eq!(read(&build(MAX_HEAD + 1)), Err(Refusal::TooLarge));
    // Without any end it is refused as soon as it cannot fit.
    let endless = vec![b'a'; MAX_HEAD + 4];
    assert_eq!(read(&endless), Err(Refusal::TooLarge));
    assert_eq!(read(&endless[..MAX_HEAD]), Ok(None));
    // The body: 1 KiB passes, one more is 413 before it is read.
    let body = "x".repeat(MAX_BODY);
    assert!(read(&post("a", "", &body)).unwrap().is_some());
    let head_only = format!(
        "POST /invitation HTTP/1.1\r\nHost: a\r\nContent-Type: \
         application/x-www-form-urlencoded\r\nContent-Length: {}\r\n\r\n",
        MAX_BODY + 1
    );
    assert_eq!(read(head_only.as_bytes()), Err(Refusal::TooLarge));
    // Too many headers.
    let mut many = String::from("GET / HTTP/1.1\r\nHost: a\r\n");
    for i in 0..70 {
        many.push_str(&format!("X-{i}: v\r\n"));
    }
    many.push_str("\r\n");
    assert_eq!(read(many.as_bytes()), Err(Refusal::BadRequest));
}

#[test]
fn bx_54_the_reader_never_panics_on_any_prefix_of_odd_input() {
    let mut seeds: Vec<Vec<u8>> = vec![get("a"), post("a", "Origin: http://a\r\n", "invitation=x")];
    seeds.push(b"\r\n\r\n".to_vec());
    seeds.push(b"\n\n\n\n".to_vec());
    seeds.push(vec![0xff; 40]);
    for seed in seeds {
        for cut in 0..=seed.len() {
            let _ = read(&seed[..cut]);
        }
    }
}

/// One exchange over a real socket: send `bytes`, read until the Box closes.
fn http(addr: SocketAddr, bytes: &[u8]) -> (u16, String, String) {
    let mut s = TcpStream::connect(addr).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    s.write_all(bytes).unwrap();
    let mut raw = Vec::new();
    let _ = s.read_to_end(&mut raw);
    let text = String::from_utf8_lossy(&raw).into_owned();
    let (head, body) = text.split_once("\r\n\r\n").unwrap_or((&text, ""));
    let status = head
        .strip_prefix("HTTP/1.1 ")
        .and_then(|r| r.get(..3))
        .and_then(|c| c.parse().ok())
        .unwrap_or(0);
    (status, head.to_string(), body.to_string())
}

fn run_with_page(name: &str, cfg: PageConfig) -> (BoxRunner, SocketAddr, TempDir) {
    let dir = TempDir::new(name);
    let (mut r, _) = start(&dir);
    let addr = r
        .serve_status_page("127.0.0.1:0".parse().unwrap(), cfg)
        .unwrap();
    (r, addr, dir)
}

fn host_of(addr: SocketAddr) -> String {
    format!("127.0.0.1:{}", addr.port())
}

#[test]
fn bx_54_one_request_is_served_for_each_connection() {
    let (r, addr, _dir) = run_with_page("onereq", PageConfig::default());
    let mut two = get(&host_of(addr));
    two.extend_from_slice(&get(&host_of(addr)));
    let (status, head, body) = http(addr, &two);
    assert_eq!(status, 200);
    assert!(head.contains("Connection: close"));
    // Exactly one answer came back.
    assert_eq!(body.matches("</html>").count(), 1);
    r.stop();
}

#[test]
fn bx_54_a_connection_that_sends_too_slowly_is_answered_400_and_closed() {
    let cfg = PageConfig {
        read: Duration::from_millis(400),
        ..PageConfig::default()
    };
    let (r, addr, _dir) = run_with_page("slowread", cfg);
    let mut s = TcpStream::connect(addr).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    s.write_all(b"GET / HT").unwrap();
    let started = Instant::now();
    let mut raw = Vec::new();
    let _ = s.read_to_end(&mut raw);
    assert!(started.elapsed() < Duration::from_secs(5));
    assert!(String::from_utf8_lossy(&raw).starts_with("HTTP/1.1 400 "));
    // A connection that sends nothing is closed at the same time.
    let mut idle = TcpStream::connect(addr).unwrap();
    idle.set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    let mut rest = Vec::new();
    let _ = idle.read_to_end(&mut rest);
    assert!(started.elapsed() < Duration::from_secs(5));
    r.stop();
}

#[test]
fn bx_54_connections_over_the_cap_are_closed_at_once() {
    let cfg = PageConfig {
        max_connections: 2,
        read: Duration::from_secs(3),
        ..PageConfig::default()
    };
    let (r, addr, _dir) = run_with_page("cap", cfg);
    let hold1 = TcpStream::connect(addr).unwrap();
    let hold2 = TcpStream::connect(addr).unwrap();
    std::thread::sleep(Duration::from_millis(300));
    let started = Instant::now();
    let mut third = TcpStream::connect(addr).unwrap();
    third
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    let mut raw = Vec::new();
    let _ = third.read_to_end(&mut raw);
    assert!(raw.is_empty(), "the third got an answer");
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "not closed at once"
    );
    // The slots come back when the holders go.
    drop(hold1);
    drop(hold2);
    eventually("a slot is free again", || {
        http(addr, &get(&host_of(addr))).0 == 200
    });
    r.stop();
}

#[test]
fn bx_54_the_live_page_answers_refusals_with_status_and_no_detail() {
    let (r, addr, _dir) = run_with_page("refusals", PageConfig::default());
    let host = host_of(addr);
    for (bytes, status, text) in [
        (b"nonsense\r\n\r\n".to_vec(), 400, "Bad request\n"),
        (
            format!("GET /nope HTTP/1.1\r\nHost: {host}\r\n\r\n").into_bytes(),
            404,
            "Not found\n",
        ),
        (
            format!("DELETE / HTTP/1.1\r\nHost: {host}\r\n\r\n").into_bytes(),
            405,
            "Method not allowed\n",
        ),
        (
            format!(
                "POST /invitation HTTP/1.1\r\nHost: {host}\r\nContent-Type: \
                 application/x-www-form-urlencoded\r\nContent-Length: 5000\r\n\r\n"
            )
            .into_bytes(),
            413,
            "Request too large\n",
        ),
    ] {
        let (got, _, body) = http(addr, &bytes);
        assert_eq!((got, body.as_str()), (status, text));
    }
    r.stop();
}

// ---- the page ---------------------------------------------------------

struct Fake {
    view: Mutex<PageView>,
    taken: Mutex<Vec<String>>,
}

impl Fake {
    fn new(status: BoxStatus) -> Arc<Self> {
        Arc::new(Self {
            view: Mutex::new(view(status)),
            taken: Mutex::new(Vec::new()),
        })
    }
}

impl PageBackend for Fake {
    fn view(&self) -> PageView {
        self.view.lock().unwrap().clone()
    }

    fn enter_invitation(&self, text: &str) -> Entered {
        self.taken.lock().unwrap().push(text.to_string());
        Entered::Taken
    }
}

fn view(status: BoxStatus) -> PageView {
    PageView {
        status,
        version: "9.9.9",
        relay_port: 8986,
        devices: 3,
        shop: matches!(status, BoxStatus::Paired | BoxStatus::TurnedOff)
            .then(|| roots().owner.fingerprint_words()),
        last_note: None,
        store_full: false,
    }
}

const STATES: [BoxStatus; 4] = [
    BoxStatus::Unpaired,
    BoxStatus::InvitationOpen,
    BoxStatus::Paired,
    BoxStatus::TurnedOff,
];

#[test]
fn bx_55_every_state_is_shown_in_its_words_with_what_to_do_next() {
    for status in STATES {
        let words = status.page_words();
        let page = render_home(&view(status), "box.local:8987", None);
        for text in [&words.heading, &words.what, &words.next] {
            assert!(
                page.contains(&text.replace('\'', "&#39;")),
                "{status:?}: {text}"
            );
        }
        assert!(page.contains("Next:"));
        assert!(page.contains("9.9.9"), "the version");
        assert!(page.contains("box.local:8986"), "the relay address");
        assert!(page.contains("<dt>Devices on the list</dt><dd>3</dd>"));
        assert_eq!(
            page.contains("<form"),
            status == BoxStatus::Unpaired,
            "{status:?}: the form"
        );
        assert_eq!(
            page.contains("<dt>Shop</dt>"),
            matches!(status, BoxStatus::Paired | BoxStatus::TurnedOff)
        );
    }
    // The words of the page are the console's, but for the one place the
    // page has a box where the console has a command.
    for status in [
        BoxStatus::InvitationOpen,
        BoxStatus::Paired,
        BoxStatus::TurnedOff,
    ] {
        assert_eq!(status.page_words(), status.words());
    }
    assert_ne!(
        BoxStatus::Unpaired.page_words(),
        BoxStatus::Unpaired.words()
    );
}

#[test]
fn bx_55_the_page_is_one_self_contained_document() {
    for status in STATES {
        let page = render_home(&view(status), "box.local", None);
        assert!(page.starts_with("<!doctype html>"));
        for banned in [
            "<script",
            "<link",
            "<img",
            "<iframe",
            "src=",
            "url(",
            "@import",
            "http://",
            "https://",
            "onclick",
            "javascript:",
        ] {
            assert!(!page.contains(banned), "{status:?}: {banned}");
        }
        assert!(page.contains("prefers-color-scheme") || page.contains("color-scheme"));
        assert!(page.contains("width=device-width"));
    }
}

#[test]
fn bx_55_the_last_note_is_shown_in_words() {
    let mut v = view(BoxStatus::Paired);
    v.last_note = Some(crate::say::note_words(
        "BX-23: the POS token could not be erased: permission denied",
    ));
    let page = render_home(&v, "box.local", None);
    assert!(page.contains("Last thing the Box noted"));
    assert!(page.contains(&v.last_note.as_ref().unwrap().heading));
}

#[test]
fn bx_55_the_live_page_shows_the_state_the_relay_port_and_the_version() {
    let (r, addr, _dir) = run_with_page("livehome", PageConfig::default());
    let (status, _, body) = http(addr, &get("shop-box.local:8987"));
    assert_eq!(status, 200);
    assert!(body.contains("Waiting for an invitation"));
    assert!(body.contains(&format!("shop-box.local:{}", r.local_addr().port())));
    assert!(body.contains("<form method=\"post\" action=\"/invitation\">"));
    assert!(body.contains(env_version()));
    // A host header that is not a name is not repeated.
    let (_, _, body) = http(addr, &get("<b>x</b>"));
    assert!(!body.contains("<b>x"));
    assert!(body.contains("this computer&#39;s address"));
    r.stop();
}

fn env_version() -> &'static str {
    option_env!("CARGO_PKG_VERSION").unwrap_or("unknown")
}

#[test]
fn bx_55_the_page_follows_the_state_of_the_box() {
    let dir = TempDir::new("follows");
    let (mut r, clock) = start(&dir);
    let addr = r
        .serve_status_page("127.0.0.1:0".parse().unwrap(), PageConfig::default())
        .unwrap();
    let page = || http(addr, &get(&host_of(addr))).2;
    assert!(page().contains(&BoxStatus::Unpaired.page_words().heading));
    r.enter_invitation(&invitation(5).to_text()).unwrap();
    assert!(page().contains(&BoxStatus::InvitationOpen.words().heading));
    // It ran out: the Box is waiting again.
    clock.advance(1_801);
    assert!(page().contains(&BoxStatus::Unpaired.page_words().heading));
    pair_ok(&r);
    let paired = page();
    assert!(paired.contains(&BoxStatus::Paired.words().heading));
    assert!(paired.contains("<dt>Devices on the list</dt><dd>4</dd>"));
    r.stop();
}

// ---- the headers ------------------------------------------------------

const HEADERS: [&str; 5] = [
    "Content-Security-Policy: default-src 'none'; style-src 'unsafe-inline'; \
     form-action 'self'; frame-ancestors 'none'\r\n",
    "X-Content-Type-Options: nosniff\r\n",
    "Referrer-Policy: no-referrer\r\n",
    "Cache-Control: no-store\r\n",
    "Connection: close\r\n",
];

fn assert_headers(head: &str, body: &str, what: &str) {
    let head = format!("{head}\r\n");
    for h in HEADERS {
        assert!(head.contains(h), "{what}: missing {h:?} in {head}");
    }
    assert!(
        head.contains(&format!("Content-Length: {}\r\n", body.len())),
        "{what}: length"
    );
}

#[test]
fn bx_56_every_answer_carries_the_headers() {
    let (r, addr, _dir) = run_with_page("headers", PageConfig::default());
    let host = host_of(addr);
    let wrong_origin = post(&host, "Origin: http://evil.example\r\n", "invitation=x");
    let wrong_check = post(&host, &format!("Origin: http://{host}\r\n"), "invitation=x");
    let cases: Vec<(&str, Vec<u8>)> = vec![
        ("200 home", get(&host)),
        ("400", b"junk\r\n\r\n".to_vec()),
        (
            "404",
            format!("GET /x HTTP/1.1\r\nHost: {host}\r\n\r\n").into_bytes(),
        ),
        (
            "405",
            format!("PUT / HTTP/1.1\r\nHost: {host}\r\n\r\n").into_bytes(),
        ),
        (
            "413",
            format!(
                "POST /invitation HTTP/1.1\r\nHost: {host}\r\nContent-Type: \
                 application/x-www-form-urlencoded\r\nContent-Length: 9999\r\n\r\n"
            )
            .into_bytes(),
        ),
        ("403 origin", wrong_origin),
        ("422 form", wrong_check),
    ];
    for (what, bytes) in cases {
        let (status, head, body) = http(addr, &bytes);
        assert!(status >= 200, "{what}");
        assert_headers(&head, &body, what);
    }
    r.stop();
}

#[test]
fn bx_56_a_built_answer_has_the_headers_the_length_and_the_type() {
    let text = String::from_utf8(crate::status_page::response(
        200,
        "text/html; charset=utf-8",
        "",
        "h\u{e9}llo",
    ))
    .unwrap();
    let (head, body) = text.split_once("\r\n\r\n").unwrap();
    assert!(head.starts_with("HTTP/1.1 200 OK\r\n"));
    assert_headers(head, body, "built");
    assert!(head.contains("Content-Type: text/html; charset=utf-8\r\n"));
    // The length counts bytes, not characters.
    assert!(head.contains("Content-Length: 6"));
}

// ---- the invitation form ----------------------------------------------

fn post_invitation(addr: SocketAddr, text: &str) -> (u16, String, String) {
    let host = host_of(addr);
    let body = format!("invitation={text}");
    http(
        addr,
        &post(&host, &format!("Origin: http://{host}\r\n"), &body),
    )
}

#[test]
fn bx_57_a_right_invitation_is_taken_and_its_claim_is_on_offer() {
    let (r, addr, _dir) = run_with_page("posttake", PageConfig::default());
    let inv = invitation(5);
    let (status, _, body) = post_invitation(addr, &inv.to_text());
    assert_eq!(status, 200);
    assert!(body.contains(&ConsoleAnswer::InvitationTaken.words().heading));
    // The state under it is the open invitation, and the form is gone.
    assert!(body.contains(&BoxStatus::InvitationOpen.words().heading));
    assert!(!body.contains("<form"));
    assert_eq!(r.status(), BoxStatus::InvitationOpen);
    // The same path the file takes: the claim is offered and checks.
    let claim = connect(&r).claim().unwrap();
    let claim = openbmt_core::boxpair::Claim::from_bytes(&claim).unwrap();
    assert!(claim.verify(&inv));
    r.stop();
}

#[test]
fn bx_57_the_form_reads_an_invitation_as_forgivingly_as_the_file_does() {
    let (r, addr, _dir) = run_with_page("postforgive", PageConfig::default());
    let text = invitation(7).to_text().to_lowercase().replace('-', "+");
    let (status, _, _) = post_invitation(addr, &text);
    assert_eq!(status, 200, "lower case, plus for a space");
    assert_eq!(r.status(), BoxStatus::InvitationOpen);
    r.stop();
}

#[test]
fn bx_57_wrong_check_characters_are_refused_with_words_and_nothing_changes() {
    let (r, addr, _dir) = run_with_page("postwrong", PageConfig::default());
    let good = invitation(5).to_text();
    // Change the last check character.
    let mut chars: Vec<char> = good.chars().collect();
    let last = chars.len() - 1;
    chars[last] = if chars[last] == '0' { '1' } else { '0' };
    let wrong: String = chars.into_iter().collect();
    let (status, _, body) = post_invitation(addr, &wrong);
    assert_eq!(status, 422);
    let words = BoxError::Invitation(openbmt_core::boxpair::InviteTextError::BadCheck).words();
    assert!(body.contains(&words.heading));
    assert!(body.contains(&words.next));
    assert_eq!(r.status(), BoxStatus::Unpaired);
    // The form is still there, and the text is not repeated.
    assert!(body.contains("<form"));
    assert!(!body.contains(&wrong));
    // A short one, and none at all.
    assert_eq!(post_invitation(addr, "ABC").0, 422);
    let host = host_of(addr);
    let none = http(
        addr,
        &post(&host, &format!("Origin: http://{host}\r\n"), "other=1"),
    );
    assert_eq!(none.0, 422);
    assert_eq!(r.status(), BoxStatus::Unpaired);
    r.stop();
}

#[test]
fn bx_57_a_box_that_holds_an_invitation_takes_no_other_from_the_page() {
    let (r, addr, _dir) = run_with_page("posttwice", PageConfig::default());
    assert_eq!(post_invitation(addr, &invitation(5).to_text()).0, 200);
    let (status, _, body) = post_invitation(addr, &invitation(6).to_text());
    assert_eq!(status, 409);
    assert!(body.contains(&PageAnswer::InvitationHeld.words().heading));
    // The first is still the one on offer.
    let claim = openbmt_core::boxpair::Claim::from_bytes(&connect(&r).claim().unwrap()).unwrap();
    assert!(claim.verify(&invitation(5)));
    r.stop();
}

#[test]
fn bx_57_a_paired_box_refuses_the_form_with_words() {
    let (r, addr, _dir) = run_with_page("postpaired", PageConfig::default());
    pair_ok(&r);
    let (status, _, body) = post_invitation(addr, &invitation(8).to_text());
    assert_eq!(status, 409);
    assert!(body.contains(&BoxError::AlreadyPaired.words().heading));
    assert_eq!(r.status(), BoxStatus::Paired);
    r.stop();
}

#[test]
fn bx_57_an_invitation_that_ran_out_lets_the_box_wait_for_another() {
    let dir = TempDir::new("postexpired");
    let (mut r, clock) = start(&dir);
    let addr = r
        .serve_status_page("127.0.0.1:0".parse().unwrap(), PageConfig::default())
        .unwrap();
    assert_eq!(post_invitation(addr, &invitation(5).to_text()).0, 200);
    clock.advance(1_801);
    // The first ran out and the Box forgot it: another is taken.
    assert_eq!(post_invitation(addr, &invitation(6).to_text()).0, 200);
    r.stop();
}

#[test]
fn bx_57_a_form_that_is_not_well_formed_or_repeats_the_field_is_a_400() {
    let (r, addr, _dir) = run_with_page("postbad", PageConfig::default());
    let host = host_of(addr);
    let origin = format!("Origin: http://{host}\r\n");
    for body in [
        "invitation=%zz",
        "invitation=%4",
        "invitation=a&invitation=b",
        "invitation=%ff",
    ] {
        let (status, _, _) = http(addr, &post(&host, &origin, body));
        assert_eq!(status, 400, "{body}");
    }
    assert_eq!(r.status(), BoxStatus::Unpaired);
    r.stop();
    assert_eq!(
        form_field(b"invitation=a%20b+c", "invitation"),
        Ok(Some("a b c".into()))
    );
    assert_eq!(form_field(b"x=1&y=2", "invitation"), Ok(None));
    assert_eq!(form_field(b"", "invitation"), Ok(None));
}

#[test]
fn bx_57_the_page_hands_the_decoded_text_to_the_backend() {
    let fake = Fake::new(BoxStatus::Unpaired);
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let page = StatusPage::start(listener, fake.clone(), PageConfig::default()).unwrap();
    let addr = page.local_addr();
    let (status, _, _) = post_invitation(addr, "AB%2DCD+EF");
    assert_eq!(status, 200);
    assert_eq!(fake.taken.lock().unwrap().as_slice(), ["AB-CD EF"]);
    page.stop();
}

// ---- the origin check -------------------------------------------------

#[test]
fn bx_58_the_origin_must_name_the_host() {
    let host = "umbrel.local:8987";
    assert!(same_origin(host, Some("http://umbrel.local:8987"), None));
    assert!(same_origin(host, Some("https://umbrel.local:8987"), None));
    assert!(same_origin(host, Some("HTTP://UMBREL.LOCAL:8987"), None));
    assert!(same_origin("[::1]:80", Some("http://[::1]:80"), None));
    for bad in [
        "http://umbrel.local:8988",
        "http://umbrel.local",
        "http://evil.example",
        "http://umbrel.local:8987.evil.example",
        "http://umbrel.local:8987/",
        "http://umbrel.local:8987/x",
        "http://user@umbrel.local:8987",
        "http://evil.example@umbrel.local:8987",
        "null",
        "",
        "ftp://umbrel.local:8987",
        "umbrel.local:8987",
        "//umbrel.local:8987",
    ] {
        assert!(!same_origin(host, Some(bad), None), "{bad}");
    }
}

#[test]
fn bx_58_the_referer_counts_only_when_there_is_no_origin() {
    let host = "box:8987";
    assert!(same_origin(host, None, Some("http://box:8987/")));
    assert!(same_origin(
        host,
        None,
        Some("http://box:8987/some/page?x=1")
    ));
    assert!(!same_origin(host, None, Some("http://evil.example/")));
    assert!(!same_origin(
        host,
        None,
        Some("http://box:8987@evil.example/")
    ));
    // An origin that is wrong is not rescued by a right referer.
    assert!(!same_origin(
        host,
        Some("http://evil.example"),
        Some("http://box:8987/")
    ));
    assert!(!same_origin(host, Some("null"), Some("http://box:8987/")));
    // With neither, the request is refused.
    assert!(!same_origin(host, None, None));
}

#[test]
fn bx_58_a_post_from_another_site_changes_nothing_and_is_refused_with_words() {
    let (r, addr, _dir) = run_with_page("origin", PageConfig::default());
    let host = host_of(addr);
    let body = format!("invitation={}", invitation(5).to_text());
    let words = PageAnswer::NotFromThisPage.words();
    for extra in [
        "Origin: http://evil.example\r\n".to_string(),
        "Origin: null\r\n".to_string(),
        "Referer: http://evil.example/page\r\n".to_string(),
        String::new(),
        format!("Origin: http://evil.example\r\nReferer: http://{host}/\r\n"),
    ] {
        let (status, _, page) = http(addr, &post(&host, &extra, &body));
        assert_eq!(status, 403, "{extra:?}");
        assert!(page.contains(&words.heading), "{extra:?}");
    }
    assert_eq!(r.status(), BoxStatus::Unpaired);
    // The same post from this page's own origin, or its referer, is taken.
    let (status, _, _) = http(
        addr,
        &post(&host, &format!("Referer: http://{host}/\r\n"), &body),
    );
    assert_eq!(status, 200);
    assert_eq!(r.status(), BoxStatus::InvitationOpen);
    r.stop();
}

// ---- nothing secret on a page -----------------------------------------

/// Every run of 24 or more hex digits in `page`.
fn long_hex(page: &str) -> Option<String> {
    let mut run = String::new();
    for c in page.chars().chain(std::iter::once(' ')) {
        if c.is_ascii_hexdigit() {
            run.push(c);
        } else {
            if run.len() >= 24 {
                return Some(run);
            }
            run.clear();
        }
    }
    None
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// What must not be on any page, in the forms a page might show it.
fn secrets(r: &BoxRunner, token: &str) -> Vec<String> {
    let mut out = vec![token.to_string()];
    let inv = invitation(5).to_text();
    out.push(inv.clone());
    out.push(inv.replace('-', ""));
    out.push(inv.to_lowercase());
    // A group of five is too short to search for; two together are not.
    out.push(inv.split('-').take(2).collect::<Vec<_>>().join("-"));
    for key in [
        r.device_id().to_bytes(),
        owner().device_id().to_bytes(),
        approver().device_id().to_bytes(),
        counter1().device_id().to_bytes(),
        counter2().device_id().to_bytes(),
    ] {
        out.push(hex(&key));
        out.push(hex(&key).to_uppercase());
    }
    out
}

fn assert_no_secret(page: &str, secrets: &[String], what: &str) {
    for s in secrets {
        assert!(!page.contains(s.as_str()), "{what}: the page holds {s}");
    }
    assert_eq!(long_hex(page), None, "{what}: a run of hex digits");
}

#[test]
fn bx_59_no_page_in_any_state_holds_a_secret() {
    let dir = TempDir::new("nosecret");
    let (mut r, _) = start(&dir);
    let addr = r
        .serve_status_page("127.0.0.1:0".parse().unwrap(), PageConfig::default())
        .unwrap();
    let token = "pos-token-SECRET-4711";
    let secrets = secrets(&r, token);
    let page = || http(addr, &get(&host_of(addr))).2;

    // Unpaired.
    assert_no_secret(&page(), &secrets, "unpaired");
    // An invitation open: the post's own answer too.
    let (_, _, answer) = post_invitation(addr, &invitation(5).to_text());
    assert_no_secret(&answer, &secrets, "the answer to a taken invitation");
    assert_no_secret(&page(), &secrets, "invitation open");
    let (_, _, answer) = post_invitation(addr, &invitation(5).to_text());
    assert_no_secret(&answer, &secrets, "the answer to a second invitation");
    // Paired, holding the POS token.
    let s = sign_both(settings_with(1, &r.device_id()));
    let mut c = connect(&r);
    let claim = openbmt_core::boxpair::Claim::from_bytes(&c.claim().unwrap()).unwrap();
    let welcome = openbmt_core::boxpair::Welcome::make(&invitation(5), &claim, roots());
    c.welcome(&welcome.to_bytes(), &s.to_bytes()).unwrap();
    r.store_pos_token(token).unwrap();
    assert_no_secret(&page(), &secrets, "paired");
    let (_, _, answer) = post_invitation(addr, &invitation(6).to_text());
    assert_no_secret(&answer, &secrets, "the answer to a post when paired");
    // Turned off.
    mailbox(&r, &counter1()).unwrap();
    let mut off = settings_with(2, &r.device_id());
    off.devices.remove(&r.device_id());
    off.boxes.clear();
    send_settings(&r, 1, &sign_owner(off));
    eventually("the Box is off", || r.status() == BoxStatus::TurnedOff);
    assert_no_secret(&page(), &secrets, "turned off");
    // The key file's bytes are not on the page either.
    let key = std::fs::read(dir.path().join(KEY_FILE)).unwrap();
    assert_no_secret(&page(), &[hex(&key)], "the key file");
    r.stop();
}

#[test]
fn bx_59_the_words_of_a_note_show_no_detail_and_no_foreign_text() {
    // A note whose detail and quoted text carry a secret.
    let mut words = Words::new(
        Tone::Stop,
        "A note",
        "Something happened.",
        Who::BoxKeeper,
        "Try again.",
    );
    words.detail = Some("token pos-token-SECRET-4711".into());
    words.quoted = Some(openbmt_core::say::Quoted::new(
        "The vendor said",
        "pos-token-SECRET-4711",
    ));
    let mut v = view(BoxStatus::Paired);
    v.last_note = Some(words);
    let page = render_home(&v, "box.local", None);
    assert!(page.contains("A note"));
    assert!(!page.contains("SECRET"));
}

#[test]
fn bx_59_all_text_is_escaped() {
    let mut v = view(BoxStatus::Paired);
    let mut words = Words::new(
        Tone::Info,
        "<b>x</b>",
        "a & b \"c\" 'd' <script>",
        Who::Nobody,
        "n\u{0}o",
    );
    words.code = None;
    v.last_note = Some(words.clone());
    let page = render_home(&v, "box.local", Some(&words));
    assert!(!page.contains("<b>x"));
    assert!(!page.contains("<script"));
    assert!(page.contains("&lt;b&gt;x&lt;/b&gt;"));
    assert!(page.contains("a &amp; b &quot;c&quot; &#39;d&#39; &lt;script&gt;"));
    assert!(!page.contains('\u{0}'));
}

// ---- the relay beside the page ----------------------------------------

#[test]
fn bx_60_the_relay_serves_while_the_page_is_full_of_slow_connections() {
    let dir = TempDir::new("flood");
    let clock = super::runner::TestClock::new(NOW);
    let mut r = start_in(&dir, &clock).unwrap();
    let cfg = PageConfig {
        max_connections: 4,
        read: Duration::from_secs(20),
        ..PageConfig::default()
    };
    let addr = r
        .serve_status_page("127.0.0.1:0".parse().unwrap(), cfg)
        .unwrap();
    // Far more slow connections than the page has slots: some send half a
    // request and stop, some send a byte at a time, some send nothing.
    let mut held = Vec::new();
    for i in 0..60 {
        if let Ok(mut s) = TcpStream::connect(addr) {
            match i % 3 {
                0 => {
                    let _ = s.write_all(b"GET / HTTP/1.1\r\nHost: a\r\nX: ");
                }
                1 => {
                    let _ = s.write_all(b"G");
                }
                _ => {}
            }
            held.push(s);
        }
    }
    // The page is full: it does not answer a good request now.
    let started = Instant::now();
    pair_ok(&r);
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "pairing was held up by the page"
    );
    assert_eq!(r.status(), BoxStatus::Paired);
    // The relay answers a fresh hello at once.
    let t = Instant::now();
    let _ = connect(&r);
    assert!(t.elapsed() < Duration::from_secs(2));
    drop(held);
    // And the page comes back when the slow ones go.
    eventually("the page answers again", || {
        http(addr, &get(&host_of(addr))).0 == 200
    });
    r.stop();
}

#[test]
fn bx_60_the_page_takes_the_boxs_state_only_after_a_whole_request() {
    // A fake whose view blocks would show a connection that reads it early:
    // with half a request sent, the backend is never asked.
    struct Counting(Mutex<u32>);
    impl PageBackend for Counting {
        fn view(&self) -> PageView {
            *self.0.lock().unwrap() += 1;
            view(BoxStatus::Unpaired)
        }
        fn enter_invitation(&self, _: &str) -> Entered {
            *self.0.lock().unwrap() += 1;
            Entered::Taken
        }
    }
    let backend = Arc::new(Counting(Mutex::new(0)));
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let page = StatusPage::start(listener, backend.clone(), PageConfig::default()).unwrap();
    let mut s = TcpStream::connect(page.local_addr()).unwrap();
    s.write_all(b"POST /invitation HTTP/1.1\r\nHost: a\r\nContent-Length: 40\r\n")
        .unwrap();
    std::thread::sleep(Duration::from_millis(400));
    assert_eq!(*backend.0.lock().unwrap(), 0);
    drop(s);
    page.stop();
}

// ---- the words --------------------------------------------------------

#[test]
fn bx_55_the_page_words_are_in_the_screen_list() {
    let names: Vec<String> = all_words().into_iter().map(|(n, _)| n).collect();
    for name in [
        "BoxStatus::Unpaired on the page",
        "PageAnswer::InvitationHeld",
        "PageAnswer::NotFromThisPage",
    ] {
        assert!(names.iter().any(|n| n == name), "{name}");
    }
}
