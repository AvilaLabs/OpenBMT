//! LK-30 to LK-33: real sockets on the loopback.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use openbmt_net_io::SystemClock;

use super::kit::*;
use crate::client::{Client, ClientConfig, ClientError, Route};
use crate::proto::{Refusal, Request, Response, FRAME_HEADER_LEN};
use crate::relay::{hello_message, Relay, RelayConfig, WelcomeAnswer, TAG_HELLO};
use crate::seal::{open, Kind, OpenError, Role};
use crate::server::{Server, ServerConfig};
use crate::store::MemoryStore;

pub fn relay_identity() -> openbmt_core::identity::Identity {
    id(99)
}

pub fn start(cfg: ServerConfig) -> Server {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let mut relay =
        Relay::new(relay_identity(), RelayConfig::default(), MemoryStore::new()).unwrap();
    let members = [owner(), counter(), the_box()].map(|i| i.device_id());
    relay.set_members(&members).unwrap();
    Server::start(listener, relay, Arc::new(SystemClock), cfg).unwrap()
}

pub fn connect(server: &Server) -> Client {
    Client::connect(
        &Route::Direct(server.local_addr()),
        Some(&relay_identity().device_id()),
        &ClientConfig::default(),
    )
    .unwrap()
}

fn hello_frame() -> Vec<u8> {
    Request::Hello { nonce: [1; 32] }.encode().unwrap()
}

/// Read a whole answer frame from a raw socket.
fn read_answer(s: &mut TcpStream) -> std::io::Result<Vec<u8>> {
    let mut header = [0u8; FRAME_HEADER_LEN];
    s.read_exact(&mut header)?;
    let len = u32::from_le_bytes(header[6..10].try_into().unwrap()) as usize;
    let mut body = vec![0u8; len];
    s.read_exact(&mut body)?;
    let mut all = header.to_vec();
    all.extend_from_slice(&body);
    Ok(all)
}

/// Whether the other side has closed: a read gives end of file or a reset,
/// within two seconds.
fn closed(s: &mut TcpStream) -> bool {
    s.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
    let mut b = [0u8; 1];
    match s.read(&mut b) {
        Ok(0) => true,
        Ok(_) => false,
        Err(e) => !matches!(
            e.kind(),
            std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
        ),
    }
}

#[test]
fn lk_30_a_device_puts_and_another_fetches_and_opens() {
    let server = start(ServerConfig::default());
    // The counter makes its mailbox.
    let mut c = connect(&server);
    c.make_mailbox(&counter()).unwrap();
    // The owner puts a sealed message.
    let sealed = owner_to_counter(1, Kind::Heartbeat, b"alive");
    let mut o = connect(&server);
    assert_eq!(o.put(&sealed).unwrap(), 1);
    // The counter fetches, opens, and acknowledges.
    let got = c.fetch(&counter(), 0).unwrap();
    assert_eq!(got.items.len(), 1);
    assert!(!got.more);
    let opened = open(
        &counter(),
        Role::Counter,
        &got.items[0].1,
        allowed,
        none_taken,
    )
    .unwrap();
    assert_eq!(opened.content, b"alive");
    assert_eq!(opened.sender, owner().device_id());
    assert_eq!(c.ack(&counter(), got.items[0].0).unwrap(), 1);
    assert!(c.fetch(&counter(), 0).unwrap().items.is_empty());
    // The relay's refusals reach the client with their rule.
    let e = o.put(&sealed[..100]).unwrap_err();
    assert_eq!(e, ClientError::Refused(Refusal::NotWellFormed));
    assert!(e.to_string().contains("LK-20"));
    // Another reader is not served from the counter's mailbox.
    let mut b = connect(&server);
    assert_eq!(
        b.fetch(&the_box(), 0).unwrap_err(),
        ClientError::Refused(Refusal::NoMailbox)
    );
    server.stop();
}

#[test]
fn lk_30_a_connection_that_sends_nothing_is_closed() {
    let cfg = ServerConfig {
        idle: Duration::from_millis(300),
        ..ServerConfig::default()
    };
    let server = start(cfg);
    let mut s = TcpStream::connect(server.local_addr()).unwrap();
    let started = Instant::now();
    assert!(closed(&mut s));
    assert!(started.elapsed() < Duration::from_secs(2));
    // A connection that sends half a frame and stops is closed too.
    let mut s = TcpStream::connect(server.local_addr()).unwrap();
    s.write_all(&hello_frame()[..7]).unwrap();
    assert!(closed(&mut s));
    // One that is served and then goes quiet is closed after its answer.
    let mut s = TcpStream::connect(server.local_addr()).unwrap();
    s.write_all(&hello_frame()).unwrap();
    assert!(read_answer(&mut s).is_ok());
    assert!(closed(&mut s));
    server.stop();
}

#[test]
fn lk_30_a_frame_over_the_limit_closes_the_connection() {
    let server = start(ServerConfig::default());
    for len in [crate::proto::MAX_REQUEST_BODY as u32 + 1, u32::MAX] {
        let mut s = TcpStream::connect(server.local_addr()).unwrap();
        let mut f = Vec::from(*b"OBMQ");
        f.extend_from_slice(&[1, 4]);
        f.extend_from_slice(&len.to_le_bytes());
        s.write_all(&f).unwrap();
        assert!(closed(&mut s), "len {len}");
    }
    // A bad magic, a bad version, an unknown type, a body of the wrong size
    // for its type.
    let frames: Vec<Vec<u8>> = vec![
        b"XXXX\x01\x01\x20\x00\x00\x00".to_vec(),
        b"OBMQ\x02\x02\x00\x00\x00\x00".to_vec(),
        b"OBMQ\x01\x77\x00\x00\x00\x00".to_vec(),
        b"OBMQ\x01\x02\x01\x00\x00\x00\x00".to_vec(),
    ];
    for f in frames {
        let mut s = TcpStream::connect(server.local_addr()).unwrap();
        s.write_all(&f).unwrap();
        assert!(closed(&mut s), "{f:?}");
    }
    // The server still serves.
    let mut c = connect(&server);
    c.make_mailbox(&counter()).unwrap();
    server.stop();
}

#[test]
fn lk_30_the_33rd_connection_is_refused() {
    let server = start(ServerConfig::default());
    let mut open_ones = Vec::new();
    for _ in 0..32 {
        let mut s = TcpStream::connect(server.local_addr()).unwrap();
        s.write_all(&hello_frame()).unwrap();
        read_answer(&mut s).unwrap();
        open_ones.push(s);
    }
    let mut extra = TcpStream::connect(server.local_addr()).unwrap();
    assert!(closed(&mut extra));
    // The 32 are still served.
    let first = &mut open_ones[0];
    first.write_all(&hello_frame()).unwrap();
    assert!(read_answer(first).is_ok());
    // When one leaves, a new one is served.
    open_ones.pop();
    let mut served = false;
    for _ in 0..40 {
        let mut s = TcpStream::connect(server.local_addr()).unwrap();
        s.write_all(&hello_frame()).unwrap();
        s.set_read_timeout(Some(Duration::from_millis(250)))
            .unwrap();
        if read_answer(&mut s).is_ok() {
            served = true;
            break;
        }
        thread::sleep(Duration::from_millis(50));
    }
    assert!(served);
    server.stop();
}

/// A SOCKS5 proxy that offers what Tor does, notes the name and password
/// and the name asked for, and joins the connection to `target`.
type ProxyLog = Arc<Mutex<Vec<(Vec<u8>, String)>>>;

fn fake_proxy(target: SocketAddr) -> (SocketAddr, ProxyLog) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let log: ProxyLog = Arc::new(Mutex::new(Vec::new()));
    let seen = Arc::clone(&log);
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut client) = stream else { continue };
            let seen = Arc::clone(&seen);
            thread::spawn(move || {
                let mut greeting = [0u8; 4];
                client.read_exact(&mut greeting).unwrap();
                assert_eq!(greeting, [5, 2, 0, 2]);
                client.write_all(&[5, 2]).unwrap();
                let mut auth = [0u8; 35];
                client.read_exact(&mut auth).unwrap();
                client.write_all(&[1, 0]).unwrap();
                let mut head = [0u8; 5];
                client.read_exact(&mut head).unwrap();
                assert_eq!(head[..4], [5, 1, 0, 3]);
                let mut name = vec![0u8; head[4] as usize + 2];
                client.read_exact(&mut name).unwrap();
                let host = String::from_utf8(name[..name.len() - 2].to_vec()).unwrap();
                seen.lock().unwrap().push((auth.to_vec(), host));
                client.write_all(&[5, 0, 0, 1, 0, 0, 0, 0, 0, 0]).unwrap();
                let up = TcpStream::connect(target).unwrap();
                let (mut client_read, mut up_write) =
                    (client.try_clone().unwrap(), up.try_clone().unwrap());
                let t = thread::spawn(move || {
                    let _ = std::io::copy(&mut client_read, &mut up_write);
                });
                let (mut up_read, mut client_write) = (up, client);
                let _ = std::io::copy(&mut up_read, &mut client_write);
                let _ = t.join();
            });
        }
    });
    (addr, log)
}

#[test]
fn lk_31_an_onion_service_is_reached_by_name_with_a_new_circuit_each_time() {
    let server = start(ServerConfig::default());
    let (proxy, log) = fake_proxy(server.local_addr());
    let route = Route::Onion {
        proxy,
        host: "exampleexampleexample.onion".into(),
        port: 8333,
    };
    let want = relay_identity().device_id();
    let mut one = Client::connect(&route, Some(&want), &ClientConfig::default()).unwrap();
    let two = Client::connect(&route, Some(&want), &ClientConfig::default()).unwrap();
    one.make_mailbox(&counter()).unwrap();
    assert_eq!(two.relay(), &want);
    let seen = log.lock().unwrap().clone();
    assert_eq!(seen.len(), 2);
    assert_eq!(seen[0].1, "exampleexampleexample.onion");
    // The name and password differ per connection: Tor gives each its own
    // circuit.
    assert_ne!(seen[0].0, seen[1].0);
    server.stop();
}

#[test]
fn lk_31_only_an_address_or_an_onion_name_is_accepted() {
    let proxy: SocketAddr = "127.0.0.1:9".parse().unwrap();
    for host in ["example.com", "onion", ".onion", "x.onion.example.com", ""] {
        let e = Client::connect(
            &Route::Onion {
                proxy,
                host: host.into(),
                port: 1,
            },
            None,
            &ClientConfig::default(),
        )
        .unwrap_err();
        assert!(matches!(e, ClientError::NotOnion(_)), "{host}: {e:?}");
        assert!(e.to_string().contains("LK-31"));
    }
    // A direct route holds a SocketAddr: there is nowhere to put a name.
    let _ = Route::Direct("127.0.0.1:1".parse().unwrap());
}

#[test]
fn lk_32_a_relay_with_another_key_is_refused() {
    let server = start(ServerConfig::default());
    let other = id(50).device_id();
    let e = Client::connect(
        &Route::Direct(server.local_addr()),
        Some(&other),
        &ClientConfig::default(),
    )
    .unwrap_err();
    assert_eq!(e, ClientError::WrongRelay(relay_identity().device_id()));
    // Given the right key, or no key, it connects; with none it says which.
    let c = Client::connect(
        &Route::Direct(server.local_addr()),
        None,
        &ClientConfig::default(),
    )
    .unwrap();
    assert_eq!(c.relay(), &relay_identity().device_id());
    server.stop();
}

#[test]
fn lk_32_a_hello_that_the_relay_did_not_sign_over_this_nonce_is_refused() {
    // A fake relay that answers with the real key's name but a signature made
    // for another nonce, and one made by another key.
    let claimed = id(60);
    for signer in [id(60), id(61)] {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let claimed_key = claimed.device_id();
        let handle = thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            let mut header = [0u8; FRAME_HEADER_LEN];
            s.read_exact(&mut header).unwrap();
            let mut nonce = [0u8; 32];
            s.read_exact(&mut nonce).unwrap();
            // Signed over an old nonce (replay) or by the wrong key.
            let sig = signer.sign_tagged(TAG_HELLO, &hello_message(&[0; 32], &claimed_key));
            let r = Response::Hello {
                relay: claimed_key,
                sig,
            };
            s.write_all(&r.encode().unwrap()).unwrap();
        });
        let e = Client::connect(
            &Route::Direct(addr),
            Some(&claimed.device_id()),
            &ClientConfig::default(),
        )
        .unwrap_err();
        assert_eq!(e, ClientError::HelloSignature);
        handle.join().unwrap();
    }
}

#[test]
fn lk_33_the_frames_carry_the_sealed_bytes_and_are_not_encrypted_again() {
    let sealed = owner_to_counter(1, Kind::Command, b"x");
    let frame = Request::Put {
        sealed: sealed.clone(),
    }
    .encode()
    .unwrap();
    // The sealed bytes are in the frame as they are, so the header (the
    // keys, the kind, the size) is visible to anyone on the network.
    assert_eq!(&frame[FRAME_HEADER_LEN..], &sealed[..]);
    assert!(frame
        .windows(32)
        .any(|w| w == counter().device_id().to_bytes()));
    // And over a socket that is what arrives.
    let server = start(ServerConfig::default());
    let mut c = connect(&server);
    c.make_mailbox(&counter()).unwrap();
    assert_eq!(c.put(&sealed).unwrap(), 1);
    let e = open(&counter(), Role::Counter, &sealed, allowed, |_| 1).unwrap_err();
    assert!(matches!(e, OpenError::Replay { .. }));
    server.stop();
}

#[test]
fn lk_23_a_challenge_from_one_socket_is_no_good_on_another() {
    let server = start(ServerConfig::default());
    let mut a = TcpStream::connect(server.local_addr()).unwrap();
    let mut b = TcpStream::connect(server.local_addr()).unwrap();
    a.write_all(&Request::Challenge.encode().unwrap()).unwrap();
    let Response::Challenge { challenge } =
        Response::decode(&read_answer(&mut a).unwrap()).unwrap()
    else {
        panic!()
    };
    let relay = relay_identity().device_id();
    let sig = crate::relay::sign_challenge(
        &counter(),
        &challenge,
        &relay,
        crate::relay::Action::Make,
        0,
    );
    let req = Request::MakeMailbox {
        reader: counter().device_id(),
        challenge,
        sig,
    }
    .encode()
    .unwrap();
    b.write_all(&req).unwrap();
    let r = Response::decode(&read_answer(&mut b).unwrap()).unwrap();
    assert_eq!(r, Response::Refused(Refusal::ChallengeUnknown));
    a.write_all(&req).unwrap();
    let r = Response::decode(&read_answer(&mut a).unwrap()).unwrap();
    assert_eq!(r, Response::Made);
    server.stop();
}

#[test]
fn lk_27_a_client_asks_for_a_claim_and_sends_a_welcome() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let relay = Relay::new(relay_identity(), RelayConfig::default(), MemoryStore::new()).unwrap();
    let relay = Arc::new(Mutex::new(relay));
    relay.lock().unwrap().set_claim(Some(vec![5; 101]));
    relay
        .lock()
        .unwrap()
        .set_welcome_hook(Some(Box::new(|w, _, _| {
            if w == [1, 2, 3] {
                WelcomeAnswer::Welcomed(Vec::new())
            } else {
                WelcomeAnswer::Withdrawn
            }
        })));
    let server = Server::start_shared(
        listener,
        Arc::clone(&relay),
        Arc::new(SystemClock),
        ServerConfig::default(),
    )
    .unwrap();
    let mut client = connect(&server);
    assert_eq!(client.claim(), Ok(vec![5; 101]));
    assert_eq!(
        client.welcome(&[9], &[]),
        Err(ClientError::Refused(Refusal::WelcomeRefused))
    );
    // This hook withdrew the claim.
    assert_eq!(client.claim(), Err(ClientError::Refused(Refusal::NoClaim)));
    assert_eq!(client.welcome(&[1, 2, 3], &[7]), Ok(()));
}
