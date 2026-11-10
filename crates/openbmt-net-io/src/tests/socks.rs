//! NR-26: the SOCKS5 client, against a fake proxy on a loopback socket.

use std::io::{Read, Write};
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr, TcpListener, TcpStream};
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
