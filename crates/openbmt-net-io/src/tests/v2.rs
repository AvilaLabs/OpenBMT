//! NR-80 to NR-85 at the level of the driver: a peer that speaks the
//! encrypted transport (written here with the `bip324` crate), peers that
//! speak the old one only, and the driver's choice between them on loopback
//! sockets.

use std::io::{ErrorKind, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use bip324::{
    CipherSession, FillBytes, GarbageResult, Handshake, PacketType, ReceivedKey, Role,
    VersionResult, NUM_LENGTH_BYTES,
};
use bitcoin::block::Header;
use bitcoin::consensus::encode::serialize;
use bitcoin::p2p::address::Address;
use bitcoin::p2p::message::NetworkMessage;
use bitcoin::p2p::message_network::VersionMessage;
use bitcoin::p2p::ServiceFlags;
use bitcoin::secp256k1::Secp256k1;
use bitcoin::Network;
use openbmt_watch::PeerTransport;

use super::kit::*;
use crate::driver::{AppEvent, Driver, DriverConfig, Handle};
use crate::env::SystemClock;

/// The two values the key exchange draws.
struct Fixed {
    secret: [u8; 32],
    aux: [u8; 32],
    drawn: u8,
}

impl FillBytes for Fixed {
    fn fill_bytes(&mut self, dest: &mut [u8; 32]) {
        *dest = if self.drawn == 0 {
            self.secret
        } else {
            self.aux
        };
        self.drawn += 1;
    }
}

/// What a fake encrypted peer does.
#[derive(Clone)]
enum Kind {
    /// The handshake, then these headers for every `getheaders`.
    Serve(Vec<Header>),
    /// A key and then garbage with no end.
    EndlessGarbage,
}

/// Read some bytes into the buffer; false when the stream ended or the test
/// stops.
fn fill(stream: &mut TcpStream, buf: &mut Vec<u8>, stop: &AtomicBool) -> bool {
    let mut chunk = [0u8; 4096];
    while !stop.load(Ordering::SeqCst) {
        match stream.read(&mut chunk) {
            Ok(0) => return false,
            Ok(n) => {
                buf.extend_from_slice(&chunk[..n]);
                return true;
            }
            Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {}
            Err(_) => return false,
        }
    }
    false
}

fn version_message() -> VersionMessage {
    let silent = Address {
        services: ServiceFlags::NONE,
        address: [0; 8],
        port: 0,
    };
    VersionMessage {
        version: 70_016,
        services: ServiceFlags::NETWORK | ServiceFlags::WITNESS | ServiceFlags::P2P_V2,
        timestamp: 1_700_000_000,
        receiver: silent.clone(),
        sender: silent,
        nonce: 42,
        user_agent: "/fake:2/".to_string(),
        start_height: i32::try_from(PEER_HEIGHT).unwrap(),
        relay: true,
    }
}

/// A packet with these contents.
fn packet(cipher: &mut CipherSession, contents: &[u8]) -> Vec<u8> {
    let mut out = vec![0u8; contents.len() + 20];
    cipher
        .outbound()
        .encrypt(contents, &mut out, PacketType::Genuine, None)
        .unwrap();
    out
}

/// Contents of a message with a long-form command.
fn long(name: &str, payload: &[u8]) -> Vec<u8> {
    let mut out = vec![0u8];
    let mut field = [0u8; 12];
    field[..name.len()].copy_from_slice(name.as_bytes());
    out.extend_from_slice(&field);
    out.extend_from_slice(payload);
    out
}

fn serve_v2(
    mut stream: TcpStream,
    kind: &Kind,
    stop: &AtomicBool,
    getheaders: &AtomicUsize,
    ids: &Mutex<Vec<[u8; 32]>>,
) {
    stream
        .set_read_timeout(Some(Duration::from_millis(50)))
        .unwrap();
    let mut buf = Vec::new();
    while buf.len() < 64 {
        if !fill(&mut stream, &mut buf, stop) {
            return;
        }
    }
    let headers = match kind {
        Kind::EndlessGarbage => {
            // A key, then bytes that never hold the terminator.
            let _ = stream.write_all(&[0x31; 64 + 4_200]);
            while fill(&mut stream, &mut buf, stop) {}
            return;
        }
        Kind::Serve(headers) => headers,
    };
    let their: [u8; 64] = buf[..64].try_into().unwrap();
    let mut feed = Fixed {
        secret: [3; 32],
        aux: [4; 32],
        drawn: 0,
    };
    let h = Handshake::new_with_rng(
        magic().to_bytes(),
        Role::Responder,
        &mut feed,
        &Secp256k1::new(),
    )
    .unwrap();
    let mut out = vec![0u8; 64];
    let h = h.send_key(None, &mut out).unwrap();
    let h = h.receive_key(their).unwrap();
    let mut tail = vec![0u8; Handshake::<ReceivedKey<'_>>::send_version_len(None)];
    let h = h.send_version(&mut tail, None).unwrap();
    out.extend_from_slice(&tail);
    stream.write_all(&out).unwrap();
    // The counter's terminator and version packet: 36 bytes.
    buf.drain(..64);
    while buf.len() < 36 {
        if !fill(&mut stream, &mut buf, stop) {
            return;
        }
    }
    let GarbageResult::FoundGarbage {
        mut handshake,
        consumed_bytes,
    } = h.receive_garbage(&buf).unwrap()
    else {
        return;
    };
    let rest = handshake
        .decrypt_packet_len(buf[consumed_bytes..consumed_bytes + 3].try_into().unwrap())
        .unwrap();
    let mut first = buf[consumed_bytes + 3..consumed_bytes + 3 + rest].to_vec();
    let VersionResult::Complete { mut cipher } = handshake.receive_version(&mut first).unwrap()
    else {
        return;
    };
    ids.lock().unwrap().push(cipher.id().to_bytes());
    buf.drain(..consumed_bytes + 3 + rest);
    let version = serialize(&NetworkMessage::Version(version_message()));
    let mut hello = packet(&mut cipher, &long("version", &version));
    hello.extend(packet(&mut cipher, &long("verack", &[])));
    stream.write_all(&hello).unwrap();
    loop {
        let Some(head) = buf.first_chunk::<NUM_LENGTH_BYTES>().copied() else {
            if !fill(&mut stream, &mut buf, stop) {
                return;
            }
            continue;
        };
        let mut probe = cipher.clone();
        let rest = probe.inbound().decrypt_packet_len(head);
        if buf.len() < 3 + rest {
            if !fill(&mut stream, &mut buf, stop) {
                return;
            }
            continue;
        }
        cipher = probe;
        let mut body = buf[3..3 + rest].to_vec();
        buf.drain(..3 + rest);
        let (_, plain) = cipher.inbound().decrypt_in_place(&mut body, None).unwrap();
        let contents = &plain[1..];
        match contents.first() {
            // ping, answered with a pong that has the same nonce.
            Some(18) => {
                let mut pong = vec![19u8];
                pong.extend_from_slice(&contents[1..]);
                let _ = stream.write_all(&packet(&mut cipher, &pong));
            }
            // getheaders, answered with the headers.
            Some(12) => {
                getheaders.fetch_add(1, Ordering::SeqCst);
                let mut reply = vec![13u8];
                reply.extend(serialize(&NetworkMessage::Headers(headers.clone())));
                let _ = stream.write_all(&packet(&mut cipher, &reply));
            }
            _ => {}
        }
    }
}

/// A peer on a loopback socket that speaks the encrypted transport.
struct V2Peer {
    addr: SocketAddr,
    accepts: Arc<AtomicUsize>,
    getheaders: Arc<AtomicUsize>,
    ids: Arc<Mutex<Vec<[u8; 32]>>>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl V2Peer {
    fn start(kind: Kind) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let mut peer = Self {
            addr: listener.local_addr().unwrap(),
            accepts: Arc::default(),
            getheaders: Arc::default(),
            ids: Arc::default(),
            stop: Arc::default(),
            thread: None,
        };
        let (accepts, getheaders, ids, stop) = (
            peer.accepts.clone(),
            peer.getheaders.clone(),
            peer.ids.clone(),
            peer.stop.clone(),
        );
        peer.thread = Some(thread::spawn(move || {
            let mut handlers = Vec::new();
            while !stop.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        accepts.fetch_add(1, Ordering::SeqCst);
                        let (kind, g, i, s) =
                            (kind.clone(), getheaders.clone(), ids.clone(), stop.clone());
                        handlers.push(thread::spawn(move || serve_v2(stream, &kind, &s, &g, &i)));
                    }
                    Err(_) => thread::sleep(Duration::from_millis(10)),
                }
            }
            for h in handlers {
                let _ = h.join();
            }
        }));
        peer
    }
}

impl Drop for V2Peer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn config_for(addr: SocketAddr, pause: u64) -> DriverConfig {
    let mut c = DriverConfig::new(Network::Regtest, 0);
    c.allow_local = true;
    c.fixed_peers = vec![addr];
    c.redial_pause_secs = pause;
    c
}

fn start(config: DriverConfig) -> Handle {
    Driver::spawn(
        config,
        MemStorage::default(),
        SystemClock,
        CountingRandomness(1),
        Vec::new(),
    )
    .unwrap()
}

fn tip_is(height: u32) -> impl Fn(&AppEvent) -> bool {
    move |e| matches!(e, AppEvent::Peers(_, _, (h, _)) if *h == height)
}

/// The transport of the one ready peer in an event, if there is one.
fn transport_of(event: &AppEvent) -> Option<PeerTransport> {
    match event {
        AppEvent::Peers(peers, ..) => peers.first().map(|p| p.transport),
        _ => None,
    }
}

fn notes(events: &[AppEvent]) -> Vec<String> {
    events
        .iter()
        .filter_map(|e| match e {
            AppEvent::Note(text) => Some(text.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn nr_80_the_driver_offers_the_encrypted_transport_first_and_a_v2_peer_is_served_over_it() {
    let headers = chain(5);
    let peer = V2Peer::start(Kind::Serve(headers.clone()));
    let mut handle = start(config_for(peer.addr, 2));
    // The headers came over the encrypted transport.
    let seen = wait_for(&handle, 10, tip_is(5)).expect("the headers did not arrive");
    assert!(until(5, || peer.getheaders.load(Ordering::SeqCst) >= 1));
    // One connection, and the peer is listed as a v2 peer (NR-82).
    assert_eq!(peer.accepts.load(Ordering::SeqCst), 1);
    assert_eq!(peer.ids.lock().unwrap().len(), 1);
    let last = seen
        .iter()
        .rev()
        .find(|e| has_peer(e))
        .expect("a peers event");
    assert_eq!(transport_of(last), Some(PeerTransport::V2));
    handle.shutdown();
}

#[test]
fn nr_80_with_the_encrypted_transport_off_the_old_one_is_used_at_once() {
    let peer = FakePeer::start(Behavior::Serve(Vec::new()));
    let mut cfg = config_for(peer.addr, 2);
    cfg.v2 = false;
    let mut handle = start(cfg);
    let seen = wait_for(&handle, 10, has_peer).expect("no ready peer");
    assert_eq!(seen.last().and_then(transport_of), Some(PeerTransport::V1));
    assert_eq!(peer.accept_count(), 1);
    assert!(notes(&seen).is_empty());
    handle.shutdown();
}

/// Randomness that has gone bad: a secret key from it would be no secret.
struct Guessable(CountingRandomness);

impl crate::env::Randomness for Guessable {
    fn next_u64(&mut self) -> u64 {
        self.0.next_u64()
    }

    fn unpredictable(&self) -> bool {
        false
    }
}

#[test]
fn nr_80_randomness_that_may_be_guessed_gets_no_key() {
    let peer = FakePeer::start(Behavior::Serve(Vec::new()));
    let cfg = config_for(peer.addr, 2);
    let mut handle = Driver::spawn(
        cfg,
        MemStorage::default(),
        SystemClock,
        Guessable(CountingRandomness(1)),
        Vec::new(),
    )
    .unwrap();
    let seen = wait_for(&handle, 10, has_peer).expect("no ready peer");
    assert_eq!(seen.last().and_then(transport_of), Some(PeerTransport::V1));
    assert_eq!(peer.accept_count(), 1);
    handle.shutdown();
}

/// A listener that closes the first connection after 64 bytes and serves
/// every later one as an old-transport peer. Records whether each
/// connection began with the network magic.
struct Refuser {
    addr: SocketAddr,
    began_old: Arc<Mutex<Vec<bool>>>,
    at: Arc<Mutex<Vec<Instant>>>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Refuser {
    fn start(after: Behavior) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let mut me = Self {
            addr: listener.local_addr().unwrap(),
            began_old: Arc::default(),
            at: Arc::default(),
            stop: Arc::default(),
            thread: None,
        };
        let (began, at, stop) = (me.began_old.clone(), me.at.clone(), me.stop.clone());
        let thread = thread::spawn(move || {
            let mut handlers = Vec::new();
            while !stop.load(Ordering::SeqCst) {
                let Ok((mut stream, _)) = listener.accept() else {
                    thread::sleep(Duration::from_millis(10));
                    continue;
                };
                at.lock().unwrap().push(Instant::now());
                let first = began.lock().unwrap().is_empty();
                if first {
                    // Read a key and close: not a peer that speaks v2.
                    began.lock().unwrap().push(false);
                    let mut key = [0u8; 64];
                    stream
                        .set_read_timeout(Some(Duration::from_secs(2)))
                        .unwrap();
                    let _ = stream.read_exact(&mut key);
                    began.lock().unwrap().pop();
                    began.lock().unwrap().push(key[..4] == magic().to_bytes());
                    continue;
                }
                // Look at the first bytes without taking them.
                let mut head = [0u8; 4];
                let mut seen = 0;
                for _ in 0..100 {
                    match stream.peek(&mut head) {
                        Ok(n) if n >= 4 => {
                            seen = n;
                            break;
                        }
                        _ => thread::sleep(Duration::from_millis(20)),
                    }
                }
                began
                    .lock()
                    .unwrap()
                    .push(seen >= 4 && head == magic().to_bytes());
                let (b, s) = (after.clone(), stop.clone());
                handlers.push(thread::spawn(move || serve_stream(stream, &b, &s)));
            }
            for h in handlers {
                let _ = h.join();
            }
        });
        me.thread = Some(thread);
        me
    }
}

impl Drop for Refuser {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

#[test]
fn nr_81_a_peer_that_closes_on_the_key_gets_the_old_transport_at_once_and_after_that_directly() {
    // The pause for a peer that closed is long: a retry within it is the
    // fallback, not the redial of NR-70.
    let peer = Refuser::start(Behavior::CloseAfterHandshake);
    let mut handle = start(config_for(peer.addr, 3));
    let seen = wait_for(&handle, 10, has_peer).expect("no ready peer on the old transport");
    assert_eq!(seen.last().and_then(transport_of), Some(PeerTransport::V1));
    assert!(notes(&seen)
        .iter()
        .any(|n| n.contains("does not speak the encrypted transport")));
    {
        let at = peer.at.lock().unwrap();
        assert_eq!(at.len(), 2);
        assert!(at[1].duration_since(at[0]) < Duration::from_millis(2_000));
    }
    assert_eq!(peer.began_old.lock().unwrap()[..2], [false, true]);
    // The old-transport connection ends after the handshake. The next
    // connection (after the pause of NR-70) goes straight to v1: it is
    // the third, it begins with the magic, and the driver says nothing more
    // about a peer that does not speak v2.
    assert!(until(10, || peer.at.lock().unwrap().len() >= 3));
    assert!(until(5, || peer.began_old.lock().unwrap().len() >= 3));
    assert!(peer.began_old.lock().unwrap()[2]);
    let mut more = Vec::new();
    while let Ok(event) = handle.events().try_recv() {
        more.push(event);
    }
    let again = notes(&more)
        .iter()
        .filter(|n| n.contains("does not speak the encrypted transport"))
        .count();
    assert_eq!(again, 0);
    handle.shutdown();
}

#[test]
fn nr_81_a_peer_that_answers_with_an_old_version_gets_the_old_transport_at_once() {
    // The kit's fake peer sends its `version` before it reads anything.
    let peer = FakePeer::start(Behavior::Serve(chain(3)));
    let mut handle = start(config_for(peer.addr, 3600));
    let seen = wait_for(&handle, 10, tip_is(3)).expect("the headers did not arrive");
    assert_eq!(seen.last().and_then(transport_of), Some(PeerTransport::V1));
    assert_eq!(peer.accept_count(), 2);
    // Not a misbehaviour: no ban, no note of one.
    assert!(notes(&seen).iter().all(|n| !n.contains("24 h")));
    handle.shutdown();
}

#[test]
fn nr_82_the_peers_event_names_the_transport_of_each_peer() {
    let v2 = V2Peer::start(Kind::Serve(Vec::new()));
    let mut handle = start(config_for(v2.addr, 2));
    let seen = wait_for(&handle, 10, has_peer).expect("no ready peer");
    assert_eq!(seen.last().and_then(transport_of), Some(PeerTransport::V2));
    handle.shutdown();
    let v1 = FakePeer::start(Behavior::Serve(Vec::new()));
    let mut cfg = config_for(v1.addr, 2);
    cfg.v2 = false;
    let mut handle = start(cfg);
    let seen = wait_for(&handle, 10, has_peer).expect("no ready peer");
    assert_eq!(seen.last().and_then(transport_of), Some(PeerTransport::V1));
    handle.shutdown();
}

#[test]
fn nr_83_a_peer_with_endless_garbage_is_dropped_without_a_ban_and_without_the_old_transport() {
    let peer = V2Peer::start(Kind::EndlessGarbage);
    let mut handle = start(config_for(peer.addr, 1));
    let seen = wait_for(
        &handle,
        10,
        |e| matches!(e, AppEvent::Note(n) if n.contains("NR-83")),
    )
    .expect("no drop for the garbage");
    let all = notes(&seen);
    assert!(all.iter().all(|n| !n.contains("24 h")), "{all:?}");
    assert!(all.iter().all(|n| !n.contains("does not speak")), "{all:?}");
    // The peer is dialed again, with the encrypted transport again.
    assert!(until(10, || peer.accepts.load(Ordering::SeqCst) >= 2));
    let more: Vec<AppEvent> = std::iter::from_fn(|| handle.events().try_recv().ok()).collect();
    assert!(notes(&more).iter().all(|n| !n.contains("does not speak")));
    handle.shutdown();
}

#[test]
fn nr_85_through_a_socks5_proxy_the_encrypted_transport_is_used_the_same_way() {
    let headers = chain(4);
    let peer = V2Peer::start(Kind::Serve(headers));
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let proxy = listener.local_addr().unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let s = stop.clone();
    let target = peer.addr;
    let server = thread::spawn(move || {
        let (mut client, _) = listener.accept().unwrap();
        let mut greeting = [0u8; 4];
        client.read_exact(&mut greeting).unwrap();
        client.write_all(&[5, 0]).unwrap();
        let mut head = [0u8; 5];
        client.read_exact(&mut head).unwrap();
        let mut rest = vec![0u8; usize::from(head[4]) + 2];
        client.read_exact(&mut rest).unwrap();
        client
            .write_all(&[5, 0, 0, 1, 127, 0, 0, 1, 0x48, 0x5c])
            .unwrap();
        // Carry the bytes both ways between the driver and the peer.
        let mut upstream = TcpStream::connect(target).unwrap();
        let mut down = upstream.try_clone().unwrap();
        let mut up = client.try_clone().unwrap();
        let back = thread::spawn(move || {
            let _ = std::io::copy(&mut down, &mut up);
        });
        client
            .set_read_timeout(Some(Duration::from_millis(100)))
            .unwrap();
        let mut buf = [0u8; 4096];
        while !s.load(Ordering::SeqCst) {
            match client.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    if upstream.write_all(&buf[..n]).is_err() {
                        break;
                    }
                }
                Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {}
                Err(_) => break,
            }
        }
        let _ = upstream.shutdown(std::net::Shutdown::Both);
        let _ = client.shutdown(std::net::Shutdown::Both);
        let _ = back.join();
    });
    let mut cfg = DriverConfig::new(Network::Regtest, 0);
    cfg.proxy = Some(proxy);
    cfg.seeds = vec!["seed.test".to_string()];
    let mut handle = start(cfg);
    let seen = wait_for(&handle, 15, tip_is(4)).expect("the headers did not arrive");
    let last = seen
        .iter()
        .rev()
        .find(|e| has_peer(e))
        .expect("a peers event");
    assert_eq!(transport_of(last), Some(PeerTransport::V2));
    assert_eq!(peer.ids.lock().unwrap().len(), 1);
    handle.shutdown();
    stop.store(true, Ordering::SeqCst);
    server.join().unwrap();
    let _ = Ipv4Addr::LOCALHOST;
}
