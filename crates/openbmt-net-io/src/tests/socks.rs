//! NR-26: the SOCKS5 client, against a fake proxy on a loopback socket.

use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, TcpListener, TcpStream};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crate::socks::{self, ReplyCode, SocksError, Target};

const TOTAL: Duration = Duration::from_secs(5);

/// A proxy that runs `script` on its one connection.
fn proxy(script: impl FnOnce(TcpStream) + Send + 'static) -> (SocketAddr, JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let handle = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        script(stream);
    });
    (addr, handle)
}

fn read_n(stream: &mut TcpStream, n: usize) -> Vec<u8> {
    let mut buf = vec![0u8; n];
    stream.read_exact(&mut buf).unwrap();
    buf
}

/// Read the greeting and answer with `method`.
fn greet(stream: &mut TcpStream, method: &[u8]) {
    assert_eq!(read_n(stream, 3), [5, 1, 0]);
    stream.write_all(method).unwrap();
}

fn run(addr: SocketAddr, target: &Target) -> Result<TcpStream, SocksError> {
    socks::connect_through(addr, target, Duration::from_secs(5), TOTAL)
}

fn v4_target() -> Target {
    Target::Addr(SocketAddr::new(Ipv4Addr::new(8, 8, 4, 4).into(), 8333))
}

#[test]
fn nr_26_connects_to_an_ipv4_target_and_reads_the_bound_address() {
    let (addr, handle) = proxy(|mut s| {
        greet(&mut s, &[5, 0]);
        assert_eq!(read_n(&mut s, 10), [5, 1, 0, 1, 8, 8, 4, 4, 0x20, 0x8d]);
        s.write_all(&[5, 0, 0, 1, 1, 2, 3, 4, 0, 80]).unwrap();
        s.write_all(b"tunnel").unwrap();
    });
    let mut stream = run(addr, &v4_target()).unwrap();
    assert_eq!(read_n(&mut stream, 6), b"tunnel");
    handle.join().unwrap();
}

#[test]
fn nr_26_connects_to_an_ipv6_target_and_reads_an_ipv6_bound_address() {
    let ip = Ipv6Addr::new(0x2001, 0x4860, 0, 0, 0, 0, 0, 1);
    let (addr, handle) = proxy(move |mut s| {
        greet(&mut s, &[5, 0]);
        let request = read_n(&mut s, 22);
        assert_eq!(request[..4], [5, 1, 0, 4]);
        assert_eq!(request[4..20], ip.octets());
        assert_eq!(request[20..], [0x20, 0x8d]);
        let mut reply = vec![5, 0, 0, 4];
        reply.extend_from_slice(&[0u8; 16]);
        reply.extend_from_slice(&[0, 0]);
        reply.extend_from_slice(b"tunnel");
        s.write_all(&reply).unwrap();
    });
    let mut stream = run(addr, &Target::Addr(SocketAddr::new(ip.into(), 8333))).unwrap();
    assert_eq!(read_n(&mut stream, 6), b"tunnel");
    handle.join().unwrap();
}

#[test]
fn nr_26_connects_to_a_name_and_reads_a_name_as_the_bound_address() {
    let (addr, handle) = proxy(|mut s| {
        greet(&mut s, &[5, 0]);
        let request = read_n(&mut s, 4 + 1 + 9 + 2);
        assert_eq!(request[..5], [5, 1, 0, 3, 9]);
        assert_eq!(&request[5..14], b"seed.test");
        assert_eq!(request[14..], [0x47, 0x9d]);
        // A bound address that is a name of 3 bytes.
        s.write_all(&[5, 0, 0, 3, 3, b'a', b'b', b'c', 0, 1])
            .unwrap();
        s.write_all(b"tunnel").unwrap();
    });
    let mut stream = run(addr, &Target::Name("seed.test".to_string(), 18_333)).unwrap();
    assert_eq!(read_n(&mut stream, 6), b"tunnel");
    handle.join().unwrap();
}

#[test]
fn nr_26_every_failure_reply_code_has_its_own_error() {
    let cases = [
        (1u8, ReplyCode::GeneralFailure),
        (2, ReplyCode::NotAllowed),
        (3, ReplyCode::NetworkUnreachable),
        (4, ReplyCode::HostUnreachable),
        (5, ReplyCode::ConnectionRefused),
        (6, ReplyCode::TtlExpired),
        (7, ReplyCode::CommandNotSupported),
        (8, ReplyCode::AddressTypeNotSupported),
        (9, ReplyCode::Other(9)),
        (0xf0, ReplyCode::Other(0xf0)),
    ];
    for (code, expected) in cases {
        let (addr, handle) = proxy(move |mut s| {
            greet(&mut s, &[5, 0]);
            read_n(&mut s, 10);
            s.write_all(&[5, code, 0, 1, 0, 0, 0, 0, 0, 0]).unwrap();
        });
        assert_eq!(
            run(addr, &v4_target()).unwrap_err(),
            SocksError::Refused(expected)
        );
        handle.join().unwrap();
    }
}

#[test]
fn nr_26_a_proxy_that_demands_a_method_we_did_not_offer_is_refused() {
    let (addr, handle) = proxy(|mut s| greet(&mut s, &[5, 2]));
    assert_eq!(
        run(addr, &v4_target()).unwrap_err(),
        SocksError::MethodNotOffered(2)
    );
    handle.join().unwrap();
    let (addr, handle) = proxy(|mut s| greet(&mut s, &[5, 0xff]));
    assert_eq!(
        run(addr, &v4_target()).unwrap_err(),
        SocksError::NoAcceptableMethod
    );
    handle.join().unwrap();
}

#[test]
fn nr_26_a_wrong_version_is_refused_in_either_answer() {
    let (addr, handle) = proxy(|mut s| greet(&mut s, &[4, 0]));
    assert_eq!(
        run(addr, &v4_target()).unwrap_err(),
        SocksError::BadVersion(4)
    );
    handle.join().unwrap();
    let (addr, handle) = proxy(|mut s| {
        greet(&mut s, &[5, 0]);
        read_n(&mut s, 10);
        s.write_all(&[4, 0, 0, 1, 0, 0, 0, 0, 0, 0]).unwrap();
    });
    assert_eq!(
        run(addr, &v4_target()).unwrap_err(),
        SocksError::BadVersion(4)
    );
    handle.join().unwrap();
}

#[test]
fn nr_26_a_reserved_byte_or_address_type_that_does_not_fit_is_refused() {
    let (addr, handle) = proxy(|mut s| {
        greet(&mut s, &[5, 0]);
        read_n(&mut s, 10);
        s.write_all(&[5, 0, 1, 1, 0, 0, 0, 0, 0, 0]).unwrap();
    });
    assert_eq!(
        run(addr, &v4_target()).unwrap_err(),
        SocksError::BadReserved(1)
    );
    handle.join().unwrap();
    let (addr, handle) = proxy(|mut s| {
        greet(&mut s, &[5, 0]);
        read_n(&mut s, 10);
        s.write_all(&[5, 0, 0, 2, 0, 0, 0, 0, 0, 0]).unwrap();
    });
    assert_eq!(
        run(addr, &v4_target()).unwrap_err(),
        SocksError::BadAddressType(2)
    );
    handle.join().unwrap();
}

#[test]
fn nr_26_a_truncated_answer_is_an_error_at_each_point() {
    // Cut inside the method answer.
    let (addr, handle) = proxy(|mut s| greet(&mut s, &[5]));
    assert_eq!(run(addr, &v4_target()).unwrap_err(), SocksError::Truncated);
    handle.join().unwrap();
    // Cut inside the reply head.
    let (addr, handle) = proxy(|mut s| {
        greet(&mut s, &[5, 0]);
        read_n(&mut s, 10);
        s.write_all(&[5, 0]).unwrap();
    });
    assert_eq!(run(addr, &v4_target()).unwrap_err(), SocksError::Truncated);
    handle.join().unwrap();
    // Cut inside the bound address.
    let (addr, handle) = proxy(|mut s| {
        greet(&mut s, &[5, 0]);
        read_n(&mut s, 10);
        s.write_all(&[5, 0, 0, 1, 1, 2]).unwrap();
    });
    assert_eq!(run(addr, &v4_target()).unwrap_err(), SocksError::Truncated);
    handle.join().unwrap();
    // Cut inside a bound name.
    let (addr, handle) = proxy(|mut s| {
        greet(&mut s, &[5, 0]);
        read_n(&mut s, 10);
        s.write_all(&[5, 0, 0, 3, 9, b'a']).unwrap();
    });
    assert_eq!(run(addr, &v4_target()).unwrap_err(), SocksError::Truncated);
    handle.join().unwrap();
}

#[test]
fn nr_26_a_proxy_that_never_answers_times_out_within_the_total() {
    let (addr, handle) = proxy(|mut s| {
        // Read the greeting and then say nothing until the client leaves.
        let mut buf = [0u8; 16];
        let _ = s.read(&mut buf);
        let _ = s.read(&mut buf);
    });
    let start = Instant::now();
    let mut stream = TcpStream::connect(addr).unwrap();
    let err = socks::handshake(&mut stream, &v4_target(), Duration::from_millis(400)).unwrap_err();
    let took = start.elapsed();
    assert_eq!(err, SocksError::TimedOut);
    assert!(took >= Duration::from_millis(350), "{took:?}");
    assert!(took < Duration::from_secs(3), "{took:?}");
    drop(stream);
    handle.join().unwrap();
}

#[test]
fn nr_26_a_proxy_that_answers_a_byte_at_a_time_cannot_stretch_the_total() {
    let (addr, handle) = proxy(|mut s| {
        let mut buf = [0u8; 16];
        let _ = s.read(&mut buf);
        // One byte of the method answer, then pauses past the total.
        let _ = s.write_all(&[5]);
        thread::sleep(Duration::from_millis(700));
        let _ = s.write_all(&[0]);
    });
    let mut stream = TcpStream::connect(addr).unwrap();
    let start = Instant::now();
    let err = socks::handshake(&mut stream, &v4_target(), Duration::from_millis(300)).unwrap_err();
    assert_eq!(err, SocksError::TimedOut);
    assert!(start.elapsed() < Duration::from_millis(650));
    drop(stream);
    handle.join().unwrap();
}

#[test]
fn nr_26_names_that_cannot_be_sent_are_refused_before_any_traffic() {
    let (addr, handle) = proxy(|mut s| {
        // Nothing may arrive: the client closes without writing.
        let mut buf = [0u8; 1];
        assert_eq!(s.read(&mut buf).unwrap(), 0);
    });
    assert_eq!(
        run(addr, &Target::Name(String::new(), 8333)).unwrap_err(),
        SocksError::EmptyName
    );
    handle.join().unwrap();
    let (addr, handle) = proxy(|mut s| {
        let mut buf = [0u8; 1];
        assert_eq!(s.read(&mut buf).unwrap(), 0);
    });
    assert_eq!(
        run(addr, &Target::Name("a".repeat(256), 8333)).unwrap_err(),
        SocksError::NameTooLong
    );
    handle.join().unwrap();
    // The longest name that fits.
    let (addr, handle) = proxy(|mut s| {
        greet(&mut s, &[5, 0]);
        read_n(&mut s, 4 + 1 + 255 + 2);
        s.write_all(&[5, 0, 0, 1, 0, 0, 0, 0, 0, 0]).unwrap();
    });
    run(addr, &Target::Name("a".repeat(255), 8333)).unwrap();
    handle.join().unwrap();
}

#[test]
fn nr_26_a_proxy_that_is_not_there_is_a_connect_error() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);
    assert!(matches!(
        run(addr, &v4_target()).unwrap_err(),
        SocksError::Connect(_)
    ));
}

#[test]
fn nr_26_every_error_says_its_rule_in_words() {
    let errors = [
        SocksError::Connect(std::io::ErrorKind::ConnectionRefused),
        SocksError::Io(std::io::ErrorKind::BrokenPipe),
        SocksError::TimedOut,
        SocksError::Truncated,
        SocksError::BadVersion(4),
        SocksError::NoAcceptableMethod,
        SocksError::MethodNotOffered(2),
        SocksError::Refused(ReplyCode::Other(9)),
        SocksError::BadReserved(1),
        SocksError::BadAddressType(2),
        SocksError::EmptyName,
        SocksError::NameTooLong,
    ];
    for e in errors {
        let text = e.to_string();
        assert!(text.starts_with("NR-26: "), "{text}");
        assert!(text.len() > 12);
    }
}

// ----- NR-78: a connection kept apart from the others ---------------------

fn run_isolated(addr: SocketAddr, token: u64) -> Result<TcpStream, SocksError> {
    socks::connect_isolated(
        addr,
        &v4_target(),
        Duration::from_secs(5),
        TOTAL,
        Some(token),
    )
}

/// Read the two-method greeting, pick name-and-password, and return the
/// name and password that were sent.
fn sign_in(stream: &mut TcpStream) -> (Vec<u8>, Vec<u8>) {
    assert_eq!(read_n(stream, 4), [5, 2, 0, 2]);
    stream.write_all(&[5, 2]).unwrap();
    assert_eq!(read_n(stream, 2), [1, 16]);
    let name = read_n(stream, 16);
    assert_eq!(read_n(stream, 1), [16]);
    let password = read_n(stream, 16);
    (name, password)
}

#[test]
fn nr_78_a_token_is_sent_as_name_and_password_when_the_proxy_asks() {
    let (addr, handle) = proxy(|mut s| {
        let (name, password) = sign_in(&mut s);
        assert_eq!(name, b"00000000deadbeef");
        assert_eq!(password, name);
        s.write_all(&[1, 0]).unwrap();
        assert_eq!(read_n(&mut s, 10), [5, 1, 0, 1, 8, 8, 4, 4, 0x20, 0x8d]);
        s.write_all(&[5, 0, 0, 1, 0, 0, 0, 0, 0, 0]).unwrap();
        s.write_all(b"ok").unwrap();
    });
    let mut stream = run_isolated(addr, 0xdead_beef).unwrap();
    assert_eq!(read_n(&mut stream, 2), b"ok");
    handle.join().unwrap();
}

#[test]
fn nr_78_a_proxy_that_wants_no_sign_in_is_used_as_it_is() {
    let (addr, handle) = proxy(|mut s| {
        assert_eq!(read_n(&mut s, 4), [5, 2, 0, 2]);
        s.write_all(&[5, 0]).unwrap();
        assert_eq!(read_n(&mut s, 10), [5, 1, 0, 1, 8, 8, 4, 4, 0x20, 0x8d]);
        s.write_all(&[5, 0, 0, 1, 0, 0, 0, 0, 0, 0]).unwrap();
    });
    run_isolated(addr, 7).unwrap();
    handle.join().unwrap();
}

#[test]
fn nr_78_a_refused_sign_in_and_a_wrong_version_are_errors() {
    for (answer, want) in [
        ([1u8, 1u8], SocksError::AuthRefused(1)),
        ([5, 0], SocksError::BadAuthVersion(5)),
    ] {
        let (addr, handle) = proxy(move |mut s| {
            sign_in(&mut s);
            s.write_all(&answer).unwrap();
        });
        assert_eq!(run_isolated(addr, 7).unwrap_err(), want);
        handle.join().unwrap();
    }
}

#[test]
fn nr_78_without_a_token_the_sign_in_method_is_not_offered_or_accepted() {
    let (addr, handle) = proxy(|mut s| {
        greet(&mut s, &[5, 2]);
    });
    assert_eq!(
        run(addr, &v4_target()).unwrap_err(),
        SocksError::MethodNotOffered(2)
    );
    handle.join().unwrap();
}

// ----- NR-26: a lookup through the proxy (Tor's RESOLVE) ------------------

fn lookup(addr: SocketAddr, token: u64) -> Result<IpAddr, SocksError> {
    socks::resolve(addr, "seed.test", token, TOTAL)
}

/// Sign in, read the RESOLVE request for "seed.test" and check its bytes.
fn take_lookup(stream: &mut TcpStream) -> (Vec<u8>, Vec<u8>) {
    let signed = sign_in(stream);
    stream.write_all(&[1, 0]).unwrap();
    let mut want = vec![5, 0xf0, 0, 3, 9];
    want.extend_from_slice(b"seed.test");
    want.extend_from_slice(&[0, 0]);
    assert_eq!(read_n(stream, want.len()), want);
    signed
}

/// A proxy that answers a lookup with `reply` and closes.
fn lookup_proxy(reply: Vec<u8>) -> (SocketAddr, JoinHandle<()>) {
    proxy(move |mut s| {
        take_lookup(&mut s);
        s.write_all(&reply).unwrap();
    })
}

#[test]
fn nr_26_a_lookup_sends_resolve_and_reads_an_ipv4_answer() {
    let (addr, handle) = lookup_proxy(vec![5, 0, 0, 1, 8, 8, 4, 4, 0, 0]);
    assert_eq!(
        lookup(addr, 0xabc).unwrap(),
        IpAddr::V4(Ipv4Addr::new(8, 8, 4, 4))
    );
    handle.join().unwrap();
}

#[test]
fn nr_26_a_lookup_reads_an_ipv6_answer() {
    let ip = Ipv6Addr::new(0x2606, 0x4700, 0, 0, 0, 0, 0, 1);
    let mut reply = vec![5, 0, 0, 4];
    reply.extend_from_slice(&ip.octets());
    reply.extend_from_slice(&[0, 0]);
    let (addr, handle) = lookup_proxy(reply);
    assert_eq!(lookup(addr, 1).unwrap(), IpAddr::V6(ip));
    handle.join().unwrap();
}

#[test]
fn nr_26_a_lookup_opens_no_stream_and_asks_port_zero() {
    // The request is pinned by `take_lookup`; the proxy sees nothing more
    // after it, and the client closes at once.
    let (addr, handle) = proxy(|mut s| {
        take_lookup(&mut s);
        s.write_all(&[5, 0, 0, 1, 9, 9, 9, 9, 0, 0]).unwrap();
        let mut rest = Vec::new();
        s.read_to_end(&mut rest).unwrap();
        assert!(rest.is_empty());
    });
    lookup(addr, 1).unwrap();
    handle.join().unwrap();
}

#[test]
fn nr_26_a_lookup_maps_every_reply_code_to_its_error() {
    let cases = [
        (1u8, ReplyCode::GeneralFailure),
        (2, ReplyCode::NotAllowed),
        (3, ReplyCode::NetworkUnreachable),
        (4, ReplyCode::HostUnreachable),
        (5, ReplyCode::ConnectionRefused),
        (6, ReplyCode::TtlExpired),
        (7, ReplyCode::CommandNotSupported),
        (8, ReplyCode::AddressTypeNotSupported),
        (9, ReplyCode::Other(9)),
        (0xf0, ReplyCode::Other(0xf0)),
    ];
    for (code, expected) in cases {
        let (addr, handle) = lookup_proxy(vec![5, code, 0, 1, 0, 0, 0, 0, 0, 0]);
        assert_eq!(lookup(addr, 1).unwrap_err(), SocksError::Refused(expected));
        handle.join().unwrap();
    }
}

#[test]
fn nr_26_a_lookup_refuses_a_wrong_version_reserved_byte_or_address_type() {
    let cases = [
        (
            vec![4u8, 0, 0, 1, 8, 8, 4, 4, 0, 0],
            SocksError::BadVersion(4),
        ),
        (
            vec![5, 0, 1, 1, 8, 8, 4, 4, 0, 0],
            SocksError::BadReserved(1),
        ),
        (
            vec![5, 0, 0, 2, 8, 8, 4, 4, 0, 0],
            SocksError::BadAddressType(2),
        ),
        (
            vec![5, 0, 0, 0, 8, 8, 4, 4, 0, 0],
            SocksError::BadAddressType(0),
        ),
        // A domain-name reply is refused, however well formed.
        (
            vec![5, 0, 0, 3, 3, b'a', b'b', b'c', 0, 0],
            SocksError::NameReply,
        ),
    ];
    for (reply, expected) in cases {
        let (addr, handle) = lookup_proxy(reply);
        assert_eq!(lookup(addr, 1).unwrap_err(), expected);
        handle.join().unwrap();
    }
}

#[test]
fn nr_26_a_lookup_answer_cut_short_is_an_error_at_each_point() {
    let cuts: [&[u8]; 4] = [
        &[5, 0],
        &[5, 0, 0, 1, 8, 8],
        &[5, 0, 0, 1, 8, 8, 4, 4, 0],
        &[5, 0, 0, 4, 1, 2, 3],
    ];
    for cut in cuts {
        let (addr, handle) = lookup_proxy(cut.to_vec());
        assert_eq!(lookup(addr, 1).unwrap_err(), SocksError::Truncated);
        handle.join().unwrap();
    }
}

#[test]
fn nr_26_a_lookup_that_is_never_answered_times_out_within_the_total() {
    let (addr, handle) = proxy(|mut s| {
        take_lookup(&mut s);
        let mut buf = [0u8; 16];
        let _ = s.read(&mut buf);
    });
    let start = Instant::now();
    let err = socks::resolve(addr, "seed.test", 1, Duration::from_millis(500)).unwrap_err();
    let took = start.elapsed();
    assert_eq!(err, SocksError::TimedOut);
    assert!(took >= Duration::from_millis(450), "{took:?}");
    assert!(took < Duration::from_secs(3), "{took:?}");
    handle.join().unwrap();
}

#[test]
fn nr_26_a_lookup_answer_that_is_not_publicly_routable_is_refused() {
    let v4s: [[u8; 4]; 7] = [
        [0, 0, 0, 0],
        [127, 0, 0, 1],
        [10, 1, 2, 3],
        [172, 16, 0, 1],
        [192, 168, 1, 1],
        [169, 254, 1, 1],
        [255, 255, 255, 255],
    ];
    for octets in v4s {
        let mut reply = vec![5, 0, 0, 1];
        reply.extend_from_slice(&octets);
        reply.extend_from_slice(&[0, 0]);
        let (addr, handle) = lookup_proxy(reply);
        assert_eq!(
            lookup(addr, 1).unwrap_err(),
            SocksError::NotRoutable(IpAddr::V4(Ipv4Addr::from(octets)))
        );
        handle.join().unwrap();
    }
    for text in ["::", "::1", "fe80::1", "fc00::1", "::ffff:8.8.8.8"] {
        let ip: Ipv6Addr = text.parse().unwrap();
        let mut reply = vec![5, 0, 0, 4];
        reply.extend_from_slice(&ip.octets());
        reply.extend_from_slice(&[0, 0]);
        let (addr, handle) = lookup_proxy(reply);
        assert_eq!(
            lookup(addr, 1).unwrap_err(),
            SocksError::NotRoutable(IpAddr::V6(ip))
        );
        handle.join().unwrap();
    }
}

#[test]
fn nr_78_each_lookup_sends_its_own_token_as_name_and_password() {
    let mut seen = Vec::new();
    for token in [1u64, 2, 0xdead_beef] {
        let (tx, rx) = std::sync::mpsc::channel();
        let (addr, handle) = proxy(move |mut s| {
            let signed = take_lookup(&mut s);
            tx.send(signed).unwrap();
            s.write_all(&[5, 0, 0, 1, 8, 8, 4, 4, 0, 0]).unwrap();
        });
        lookup(addr, token).unwrap();
        handle.join().unwrap();
        let (name, password) = rx.recv().unwrap();
        assert_eq!(name, format!("{token:016x}").into_bytes());
        assert_eq!(password, name);
        seen.push(name);
    }
    seen.sort();
    seen.dedup();
    assert_eq!(seen.len(), 3);
}

#[test]
fn nr_26_a_lookup_cannot_send_an_empty_or_long_name_and_a_missing_proxy_is_a_connect_error() {
    // A name that cannot be sent is refused before any connection: the
    // proxy address here has nobody listening, which would be a different
    // error.
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let gone = listener.local_addr().unwrap();
    drop(listener);
    assert_eq!(
        socks::resolve(gone, "", 1, TOTAL).unwrap_err(),
        SocksError::EmptyName
    );
    assert_eq!(
        socks::resolve(gone, &"a".repeat(256), 1, TOTAL).unwrap_err(),
        SocksError::NameTooLong
    );
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let gone = listener.local_addr().unwrap();
    drop(listener);
    assert!(matches!(
        lookup(gone, 1).unwrap_err(),
        SocksError::Connect(_)
    ));
}

#[test]
fn nr_26_a_lookup_signs_in_like_a_connection_and_names_its_errors() {
    let (addr, handle) = proxy(|mut s| {
        sign_in(&mut s);
        s.write_all(&[1, 1]).unwrap();
    });
    assert_eq!(lookup(addr, 1).unwrap_err(), SocksError::AuthRefused(1));
    handle.join().unwrap();
    for e in [
        SocksError::NotRoutable(IpAddr::V4(Ipv4Addr::LOCALHOST)),
        SocksError::NameReply,
    ] {
        let text = e.to_string();
        assert!(text.starts_with("NR-26: "), "{text}");
    }
}
