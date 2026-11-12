//! SW-80 to SW-82: the bytes of a request and a reply, and the provider.

use super::fixtures::*;
use crate::{parse_response, Provider, Refused, Request, Setting, MAX_HTTP_BYTES};

/// The onion name published for a provider, as read on 2026-11-12.
const ONION: &str = "w2mqd2fcbgiop7oyxoes2ozmdnvgon22oipj7vzh2rquepf7cxauauad.onion";

fn provider() -> Provider {
    Provider::new("Provider", ONION, "/api").unwrap()
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8(bytes.to_vec()).unwrap()
}

#[test]
fn sw_80_a_request_has_the_headers_it_needs_and_no_others() {
    let p = provider();
    assert_eq!(
        text(&p.get("/v2/swap/reverse").unwrap()),
        format!("GET /api/v2/swap/reverse HTTP/1.1\r\nHost: {ONION}\r\nConnection: close\r\n\r\n")
    );
    assert_eq!(
        p.pairs_request().unwrap(),
        p.get("/v2/swap/reverse").unwrap()
    );
    let body = b"{\"a\":1}";
    assert_eq!(
        text(&p.post("/v2/swap/reverse", body).unwrap()),
        format!(
            "POST /api/v2/swap/reverse HTTP/1.1\r\nHost: {ONION}\r\n\
             Content-Type: application/json\r\nContent-Length: 7\r\nConnection: close\r\n\r\n\
             {{\"a\":1}}"
        )
    );
    // The create request carries the body of SW-01 and nothing about the
    // shop: no user agent, no accept header, nothing else.
    let s = secrets();
    let req = Request::new(&s, 50_000, "swap").unwrap();
    let bytes = p.create_request(&req).unwrap();
    let t = text(&bytes);
    assert!(t.ends_with(&req.to_json()));
    let head = t.split("\r\n\r\n").next().unwrap();
    let names: Vec<&str> = head
        .split("\r\n")
        .skip(1)
        .map(|l| l.split(':').next().unwrap())
        .collect();
    assert_eq!(
        names,
        ["Host", "Content-Type", "Content-Length", "Connection"]
    );
    assert!(!t.to_lowercase().contains("user-agent"));
    // The status call.
    assert!(text(&p.status_request("aBc-1_x").unwrap()).starts_with("GET /api/v2/swap/aBc-1_x "));
    assert!(p.status_request("").is_err());
    assert!(p.status_request("a/b").is_err());
    assert!(p.status_request(&"a".repeat(65)).is_err());
    // No prefix: the path is as given.
    let bare = Provider::new("Provider", ONION, "").unwrap();
    assert!(text(&bare.pairs_request().unwrap()).starts_with("GET /v2/swap/reverse HTTP/1.1"));
    // Paths that are refused.
    for path in [
        "v2/swap",
        "/v2/swap/",
        "/v2//swap",
        "/v2/../x",
        "/v2/swap?x=1",
        "/v2/swap#x",
        "/v2/sw ap",
        "/v2/swap\r\nX: 1",
        "/é",
        &format!("/{}", "a".repeat(200)),
    ] {
        let err = p.get(path).unwrap_err();
        assert_eq!(err, Refused::BadHttpRequest("path"), "{path}");
        assert_eq!(err.rule(), "SW-80");
    }
    assert!(p.post("/v2/swap", &vec![b'x'; MAX_HTTP_BYTES + 1]).is_err());
}

fn ok(body: &str) -> Vec<u8> {
    format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    )
    .into_bytes()
}

#[test]
fn sw_81_a_reply_is_read_strictly_by_length_or_by_chunks() {
    let r = parse_response(&ok("{\"a\":1}")).unwrap();
    assert_eq!((r.status(), r.body()), (200, &b"{\"a\":1}"[..]));
    assert_eq!(r.ok_body().unwrap(), b"{\"a\":1}");
    // Header names in any case, values trimmed; a reason may be missing.
    let r = parse_response(b"HTTP/1.1 201\r\ncontent-LENGTH:  2 \r\nServer: x\r\n\r\nhi").unwrap();
    assert_eq!((r.status(), r.body()), (201, &b"hi"[..]));
    assert_eq!(r.ok_body().unwrap(), b"hi");
    let r = parse_response(b"HTTP/1.1 202 Accepted\r\nContent-Length: 2\r\n\r\nhi").unwrap();
    assert_eq!(r.ok_body(), Err(Refused::HttpStatus(202)));
    // Chunks.
    let chunked = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n\
                    5\r\nhello\r\nA\r\n, world!!!\r\n0\r\n\r\n";
    assert_eq!(parse_response(chunked).unwrap().body(), b"hello, world!!!");
    let empty = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: Chunked\r\n\r\n0\r\n\r\n";
    assert_eq!(parse_response(empty).unwrap().body(), b"");
    // A status other than 200 gives the status and no success.
    let err = parse_response(b"HTTP/1.1 400 Bad Request\r\nContent-Length: 2\r\n\r\n{}")
        .unwrap()
        .ok_body()
        .unwrap_err();
    assert_eq!(err, Refused::HttpStatus(400));
    assert_eq!(err.rule(), "SW-81");
    // What is refused.
    let long = format!("HTTP/1.1 200 OK\r\nContent-Length: {MAX_HTTP_BYTES}\r\n\r\n");
    let mut too_long = long.into_bytes();
    too_long.extend(std::iter::repeat_n(b'x', MAX_HTTP_BYTES));
    let cases: Vec<(&str, Vec<u8>)> = vec![
        ("empty", b"".to_vec()),
        (
            "no end of headers",
            b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n".to_vec(),
        ),
        (
            "version 1.0",
            b"HTTP/1.0 200 OK\r\nContent-Length: 0\r\n\r\n".to_vec(),
        ),
        (
            "version 2",
            b"HTTP/2 200\r\nContent-Length: 0\r\n\r\n".to_vec(),
        ),
        (
            "lower case",
            b"http/1.1 200 OK\r\nContent-Length: 0\r\n\r\n".to_vec(),
        ),
        (
            "short code",
            b"HTTP/1.1 20 OK\r\nContent-Length: 0\r\n\r\n".to_vec(),
        ),
        (
            "signed code",
            b"HTTP/1.1 +20 OK\r\nContent-Length: 0\r\n\r\n".to_vec(),
        ),
        (
            "informational",
            b"HTTP/1.1 100 Continue\r\nContent-Length: 0\r\n\r\n".to_vec(),
        ),
        (
            "code 600",
            b"HTTP/1.1 600 X\r\nContent-Length: 0\r\n\r\n".to_vec(),
        ),
        (
            "no length",
            b"HTTP/1.1 200 OK\r\nServer: x\r\n\r\nbody".to_vec(),
        ),
        (
            "length twice",
            b"HTTP/1.1 200 OK\r\nContent-Length: 1\r\nContent-Length: 1\r\n\r\nx".to_vec(),
        ),
        (
            "length signed",
            b"HTTP/1.1 200 OK\r\nContent-Length: +1\r\n\r\nx".to_vec(),
        ),
        (
            "length empty",
            b"HTTP/1.1 200 OK\r\nContent-Length:\r\n\r\n".to_vec(),
        ),
        (
            "length huge",
            b"HTTP/1.1 200 OK\r\nContent-Length: 99999999999999999999999\r\n\r\n".to_vec(),
        ),
        (
            "cut short",
            b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nabc".to_vec(),
        ),
        (
            "bytes after",
            b"HTTP/1.1 200 OK\r\nContent-Length: 1\r\n\r\nabc".to_vec(),
        ),
        (
            "both",
            b"HTTP/1.1 200 OK\r\nContent-Length: 1\r\nTransfer-Encoding: chunked\r\n\r\n0\r\n\r\n"
                .to_vec(),
        ),
        (
            "gzip chunks",
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: gzip, chunked\r\n\r\n0\r\n\r\n".to_vec(),
        ),
        (
            "encoded",
            b"HTTP/1.1 200 OK\r\nContent-Encoding: gzip\r\nContent-Length: 0\r\n\r\n".to_vec(),
        ),
        (
            "folded header",
            b"HTTP/1.1 200 OK\r\nX: a\r\n b\r\nContent-Length: 0\r\n\r\n".to_vec(),
        ),
        (
            "no colon",
            b"HTTP/1.1 200 OK\r\nX\r\nContent-Length: 0\r\n\r\n".to_vec(),
        ),
        (
            "space in name",
            b"HTTP/1.1 200 OK\r\nX Y: 1\r\nContent-Length: 0\r\n\r\n".to_vec(),
        ),
        (
            "bare newline",
            b"HTTP/1.1 200 OK\r\nX: a\nY: b\r\nContent-Length: 0\r\n\r\n".to_vec(),
        ),
        (
            "non ascii",
            "HTTP/1.1 200 OK\r\nX: \u{e9}\r\nContent-Length: 0\r\n\r\n"
                .as_bytes()
                .to_vec(),
        ),
        (
            "chunk size empty",
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n\r\nab\r\n0\r\n\r\n".to_vec(),
        ),
        (
            "chunk size not hex",
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\nxz\r\nab\r\n0\r\n\r\n".to_vec(),
        ),
        (
            "chunk extension",
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n2;x=1\r\nab\r\n0\r\n\r\n"
                .to_vec(),
        ),
        (
            "chunk cut short",
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nab\r\n0\r\n\r\n".to_vec(),
        ),
        (
            "chunk no end",
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n2\r\nabXX0\r\n\r\n".to_vec(),
        ),
        (
            "chunks never end",
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n2\r\nab\r\n".to_vec(),
        ),
        (
            "trailer",
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n0\r\nX: 1\r\n\r\n".to_vec(),
        ),
        (
            "no final crlf",
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n0\r\n".to_vec(),
        ),
        (
            "bytes after chunks",
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n0\r\n\r\nzz".to_vec(),
        ),
        ("too long", too_long),
    ];
    for (what, bytes) in cases {
        let err = parse_response(&bytes).unwrap_err();
        assert!(matches!(err, Refused::BadHttpReply(_)), "{what}: {err}");
        assert_eq!(err.rule(), "SW-81", "{what}");
    }
    // Too many headers.
    let mut many = String::from("HTTP/1.1 200 OK\r\n");
    for i in 0..65 {
        many.push_str(&format!("X-{i}: 1\r\n"));
    }
    many.push_str("Content-Length: 0\r\n\r\n");
    assert!(parse_response(many.as_bytes()).is_err());
}

#[test]
fn sw_82_the_provider_is_a_pinned_setting_that_is_off_by_default() {
    // Off by default, and no provider is built in.
    let off = Setting::default();
    assert!(!off.is_on());
    assert_eq!(off.provider(), Err(Refused::ProviderOff));
    assert_eq!(Refused::ProviderOff.rule(), "SW-82");
    let on = Setting::on(provider());
    assert!(on.is_on());
    let p = on.provider().unwrap();
    assert_eq!(
        (p.name(), p.onion(), p.path_prefix()),
        ("Provider", ONION, "/api")
    );
    // A pinned v3 name, checksum and version included.
    let mut flipped = ONION.to_string();
    flipped.replace_range(10..11, if &ONION[10..11] == "a" { "b" } else { "a" });
    for host in [
        "",
        "example.com",
        &flipped,
        &ONION.to_uppercase(),
        &ONION.replace(".onion", ""),
        &ONION[1..],
        &format!("{ONION}."),
        &format!("http://{ONION}"),
    ] {
        assert_eq!(
            Provider::new("Provider", host, "/api"),
            Err(Refused::BadProvider("onion name")),
            "{host}"
        );
    }
    for prefix in ["api", "/api/", "/a b", "/..", "//", "/api?x"] {
        assert_eq!(
            Provider::new("Provider", ONION, prefix),
            Err(Refused::BadProvider("path prefix")),
            "{prefix}"
        );
    }
    for name in ["", "x\ny", &"n".repeat(33), "caf\u{e9}"] {
        assert_eq!(
            Provider::new(name, ONION, ""),
            Err(Refused::BadProvider("name"))
        );
    }
}

#[test]
fn sw_82_the_copy_of_the_onion_check_agrees_with_the_one_of_the_net_crate() {
    use openbmt_net::onion as net;
    // Names the net crate writes, and the same names damaged.
    let mut names = vec![ONION.to_string()];
    for seed in 0..40u8 {
        let mut key = [seed; 32];
        key[0] = seed.wrapping_mul(31);
        key[31] = seed.wrapping_add(7);
        let name = net::onion_name(&key);
        assert_eq!(crate::onion::parse_onion_name(&name), Some(key));
        names.push(name);
    }
    let mut damaged = Vec::new();
    for name in &names {
        for at in [0usize, 20, 54, 55] {
            for c in ['a', 'z', '2', '7', 'A', '-'] {
                let mut s = name.clone();
                s.replace_range(at..=at, &c.to_string());
                damaged.push(s);
            }
        }
        damaged.push(name.to_uppercase());
        damaged.push(name.replace(".onion", ".onio"));
        damaged.push(name[..name.len() - 1].to_string());
        damaged.push(format!("{name}x"));
        damaged.push(format!("x{name}"));
    }
    for name in names.iter().chain(&damaged) {
        assert_eq!(
            crate::onion::parse_onion_name(name),
            net::parse_onion_name(name),
            "{name}"
        );
    }
    // SHA3-256 of FIPS 202, through the checksum it makes.
    assert_eq!(
        crate::onion::sha3_256_hex(b""),
        "a7ffc6f8bf1ed76651c14756a061d662f580ff4de43b49fa82d80a4b80f8434a"
    );
    assert_eq!(
        crate::onion::sha3_256_hex(b"abc"),
        "3a985da74fe225b2045c172d6bd390bd855f086e3e9d525b46bfe24511431532"
    );
}
