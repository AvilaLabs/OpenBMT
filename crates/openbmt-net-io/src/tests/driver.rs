//! The driver against fake peers on loopback sockets.

use std::io::{ErrorKind, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use bitcoin::hashes::Hash;
use bitcoin::p2p::address::AddrV2;
use bitcoin::p2p::ServiceFlags;
use bitcoin::{Network, ScriptBuf};
use openbmt_net::{AddressBook, Source};
use openbmt_watch::{HeaderStore, PeerAddr};

use super::kit::*;
use crate::dial::{DialError, Net};
use crate::driver::{
    longest_connected, AppEvent, Driver, DriverConfig, Handle, ANCHOR_SECS, HEADER_SAVE_SECS,
    WRITE_TIMEOUT_SECS,
};
use crate::env::{Key, SystemClock};
use crate::socks::Target;

fn config(peer: &FakePeer, pause: u64) -> DriverConfig {
    let mut c = DriverConfig::new(Network::Regtest, 0);
    c.allow_local = true;
    c.fixed_peers = vec![peer.addr];
    c.redial_pause_secs = pause;
    // The fake peers speak the old transport alone; v2 has tests of its own.
    c.v2 = false;
    c
}

/// A config with no fixed peer, for tests that reach peers by the book or
/// the seeds.
fn bare() -> DriverConfig {
    let mut c = DriverConfig::new(Network::Regtest, 0);
    c.v2 = false;
    c
}

fn start(config: DriverConfig, storage: &MemStorage, net: Net) -> Handle {
    Driver::spawn_with(
        config,
        storage.clone(),
        SystemClock,
        CountingRandomness(1),
        Vec::new(),
        net,
    )
    .unwrap()
}

/// Start with a clock the test sets and these watched scripts.
fn start_clocked(
    config: DriverConfig,
    storage: &MemStorage,
    net: Net,
    clock: &TestClock,
    scripts: Vec<ScriptBuf>,
) -> Handle {
    Driver::spawn_with(
        config,
        storage.clone(),
        clock.clone(),
        CountingRandomness(1),
        scripts,
        net,
    )
    .unwrap()
}

/// A script that no block pays, watched so that blocks are fetched (NR-42).
fn watched() -> Vec<ScriptBuf> {
    vec![ScriptBuf::from_bytes(vec![0x51])]
}

fn is_block(event: &AppEvent) -> bool {
    matches!(event, AppEvent::Block { .. })
}

/// Let the driver's loop pass a few times at the time the clock reads.
fn settle() {
    thread::sleep(Duration::from_millis(700));
}

fn start_system(config: DriverConfig, storage: &MemStorage) -> Handle {
    start(config, storage, Net::system())
}

fn v4(a: u8, b: u8, c: u8, d: u8) -> AddrV2 {
    AddrV2::Ipv4(Ipv4Addr::new(a, b, c, d))
}

fn sock(a: u8, b: u8, c: u8, d: u8, port: u16) -> SocketAddr {
    SocketAddr::new(Ipv4Addr::new(a, b, c, d).into(), port)
}

/// A stored book with these addresses (all in different groups).
fn book_bytes(entries: &[(u8, u8, u8, u8, u16)], anchors: &[(u8, u8, u8, u8, u16)]) -> Vec<u8> {
    let mut book = AddressBook::new();
    for (a, b, c, d, port) in entries {
        let added = book.add(
            &v4(*a, *b, *c, *d),
            *port,
            ServiceFlags::NETWORK | ServiceFlags::WITNESS,
            Source::Own,
            1,
        );
        assert_eq!(added, openbmt_net::Added::New);
    }
    let anchors: Vec<(AddrV2, u16)> = anchors
        .iter()
        .map(|(a, b, c, d, port)| (v4(*a, *b, *c, *d), *port))
        .collect();
    book.set_anchors(&anchors);
    book.to_bytes()
}

fn stored_book(storage: &MemStorage) -> AddressBook {
    AddressBook::from_bytes(&storage.get(Key::Book).unwrap(), 0).unwrap()
}

fn stored_headers(storage: &MemStorage) -> HeaderStore {
    HeaderStore::from_bytes(Network::Regtest, &storage.get(Key::Headers).unwrap()).unwrap()
}

fn tip_is(height: u32) -> impl Fn(&AppEvent) -> bool {
    move |e| matches!(e, AppEvent::Peers(_, _, (h, _)) if *h == height)
}

#[test]
fn nr_71_a_fixed_peer_is_dialed_and_the_handshake_ends_in_a_peers_event() {
    let peer = FakePeer::start(Behavior::Serve(Vec::new()));
    let storage = MemStorage::default();
    let mut handle = start_system(config(&peer, 2), &storage);
    let seen = wait_for(&handle, 10, has_peer).expect("no ready peer");
    // The first event is the start: no peers, the genesis tip.
    match seen.first() {
        Some(AppEvent::Peers(peers, None, tip)) => {
            assert!(peers.is_empty());
            assert_eq!(*tip, (0, u64::from(GENESIS_TIME)));
        }
        other => panic!("{other:?}"),
    }
    match seen.last() {
        Some(AppEvent::Peers(peers, _, _)) => {
            assert_eq!(peers.len(), 1);
            assert_eq!(
                peers.first().map(|p| (p.addr, p.best_height)),
                Some((PeerAddr::Ip(Ipv4Addr::LOCALHOST.into()), PEER_HEIGHT))
            );
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(peer.accept_count(), 1);
    handle.shutdown();
}

#[test]
fn nr_70_a_peer_that_closes_is_forgotten_and_redialed_after_the_pause() {
    let peer = FakePeer::start(Behavior::CloseAfterHandshake);
    let storage = MemStorage::default();
    let mut handle = start_system(config(&peer, 2), &storage);
    wait_for(&handle, 10, has_peer).expect("no ready peer");
    // The peer is forgotten ...
    wait_for(
        &handle,
        10,
        |e| matches!(e, AppEvent::Peers(peers, ..) if peers.is_empty()),
    )
    .expect("the closed peer was not forgotten");
    // ... and is not dialed again at once ...
    assert_eq!(peer.accept_count(), 1);
    // ... but after the pause.
    assert!(until(10, || peer.accept_count() >= 2));
    let at = peer.accepted_at.lock().unwrap().clone();
    let gap = at[1].duration_since(at[0]);
    assert!(gap >= Duration::from_millis(900), "{gap:?}");
    handle.shutdown();
}

#[test]
fn nr_25_a_peer_that_sends_garbage_is_dropped_and_not_dialed_again() {
    let peer = FakePeer::start(Behavior::Garbage);
    let storage = MemStorage::default();
    let mut handle = start_system(config(&peer, 1), &storage);
    let seen = wait_for(&handle, 10, is_note).expect("no note");
    match seen.last() {
        Some(AppEvent::Note(text)) => {
            assert!(text.contains("dropped") && text.contains("NR-01"), "{text}");
        }
        other => panic!("{other:?}"),
    }
    assert!(until(5, || peer.closed_by_driver.load(Ordering::SeqCst) == 1));
    // A peer on this machine is not in the book, so the driver keeps the
    // ban itself: the pause is over after 1 s and still nothing dials.
    thread::sleep(Duration::from_millis(3_500));
    assert_eq!(peer.accept_count(), 1);
    handle.shutdown();
}

#[test]
fn nr_25_a_book_peer_that_misbehaves_is_marked_in_the_book() {
    let peer = FakePeer::start(Behavior::Garbage);
    let storage = MemStorage::with(Key::Book, book_bytes(&[(8, 8, 8, 8, 8333)], &[]));
    let (net, log, _) = net_to(peer.addr, Vec::new());
    let mut handle = start(bare(), &storage, net);
    wait_for(&handle, 10, is_note).expect("no note");
    // The book keeps the ban across the pause and the restart.
    thread::sleep(Duration::from_millis(1_500));
    assert_eq!(log.lock().unwrap().len(), 1);
    handle.shutdown();
    let book = stored_book(&storage);
    let entry = book.get(&v4(8, 8, 8, 8), 8333).unwrap();
    assert!(entry.banned_until.is_some());
    assert!(book.anchors().is_empty());
}

#[test]
fn nr_23_the_next_address_is_drawn_from_the_book_with_the_callers_randomness() {
    let peer = FakePeer::start(Behavior::Serve(Vec::new()));
    let storage = MemStorage::with(Key::Book, book_bytes(&[(8, 8, 8, 8, 8333)], &[]));
    let (net, log, _) = net_to(peer.addr, Vec::new());
    let mut handle = start(bare(), &storage, net);
    wait_for(&handle, 10, has_peer).expect("no ready peer");
    assert_eq!(
        log.lock().unwrap().first().cloned(),
        Some((Target::Addr(sock(8, 8, 8, 8, 8333)), None))
    );
    handle.shutdown();
    // The handshake is recorded in the book.
    let book = stored_book(&storage);
    let entry = book.get(&v4(8, 8, 8, 8), 8333).unwrap();
    assert!(entry.last_good.is_some());
    assert_eq!(entry.failures, 0);
    assert_eq!(
        entry.services,
        ServiceFlags::NETWORK | ServiceFlags::WITNESS
    );
}

#[test]
fn nr_24_the_two_peers_connected_longest_are_stored_as_anchors_at_shutdown() {
    let peer = FakePeer::start(Behavior::Serve(Vec::new()));
    let storage = MemStorage::with(Key::Book, book_bytes(&[(8, 8, 8, 8, 8333)], &[]));
    let (net, _, _) = net_to(peer.addr, Vec::new());
    let mut handle = start(bare(), &storage, net);
    wait_for(&handle, 10, has_peer).expect("no ready peer");
    handle.shutdown();
    assert_eq!(
        stored_book(&storage).anchors(),
        vec![(v4(8, 8, 8, 8), 8333)]
    );
}

#[test]
fn nr_24_the_oldest_two_ready_peers_win() {
    let ready = [
        (v4(1, 1, 1, 1), 1, 300),
        (v4(2, 2, 2, 2), 2, 100),
        (v4(3, 3, 3, 3), 3, 200),
        (v4(4, 4, 4, 4), 4, 100),
    ];
    assert_eq!(
        longest_connected(&ready),
        vec![(v4(2, 2, 2, 2), 2), (v4(4, 4, 4, 4), 4)]
    );
    assert_eq!(longest_connected(&ready[..1]), vec![(v4(1, 1, 1, 1), 1)]);
    assert!(longest_connected(&[]).is_empty());
}

#[test]
fn nr_71_anchors_come_first_then_fixed_peers_then_the_book() {
    let entries = [(8, 8, 8, 8, 8333), (9, 9, 9, 9, 8333), (11, 1, 1, 1, 8333)];
    let storage = MemStorage::with(Key::Book, book_bytes(&entries, &[(11, 1, 1, 1, 8333)]));
    let peer = FakePeer::start(Behavior::Silent);
    // Every dial fails, one at a time, so that the order is the order of
    // the log.
    let log: DialLog = Arc::default();
    let l = log.clone();
    let net = Net {
        dial: Arc::new(move |target, proxy, _| {
            l.lock().unwrap().push((target.clone(), proxy));
            Err(DialError::Connect(std::io::ErrorKind::TimedOut))
        }),
        resolve: Arc::new(|_, _| Ok(Vec::new())),
    };
    let mut cfg = config(&peer, 30);
    cfg.target_peers = 1;
    let mut handle = start(cfg, &storage, net);
    assert!(until(10, || log.lock().unwrap().len() >= 4));
    handle.shutdown();
    let log = log.lock().unwrap();
    let order: Vec<&Target> = log.iter().map(|(t, _)| t).collect();
    assert_eq!(order[0], &Target::Addr(sock(11, 1, 1, 1, 8333)));
    assert_eq!(order[1], &Target::Addr(peer.addr));
    let rest: Vec<&&Target> = order[2..4].iter().collect();
    assert!(rest.contains(&&&Target::Addr(sock(8, 8, 8, 8, 8333))));
    assert!(rest.contains(&&&Target::Addr(sock(9, 9, 9, 9, 8333))));
    // A failed dial is counted against the book entry.
    assert_eq!(
        stored_book(&storage)
            .get(&v4(11, 1, 1, 1), 8333)
            .unwrap()
            .failures,
        1
    );
}

#[test]
fn nr_71_at_most_four_dials_are_in_flight() {
    let entries: Vec<(u8, u8, u8, u8, u16)> = (1..=10).map(|i| (20 + i, 1, 1, 1, 8333)).collect();
    let storage = MemStorage::with(Key::Book, book_bytes(&entries, &[]));
    let current = Arc::new(AtomicUsize::new(0));
    let most = Arc::new(AtomicUsize::new(0));
    let (c, m) = (current.clone(), most.clone());
    let net = Net {
        dial: Arc::new(move |_, _, _| {
            let now = c.fetch_add(1, Ordering::SeqCst) + 1;
            m.fetch_max(now, Ordering::SeqCst);
            thread::sleep(Duration::from_millis(300));
            c.fetch_sub(1, Ordering::SeqCst);
            Err(DialError::Connect(std::io::ErrorKind::TimedOut))
        }),
        resolve: Arc::new(|_, _| Ok(Vec::new())),
    };
    let mut handle = start(bare(), &storage, net);
    thread::sleep(Duration::from_millis(2_000));
    handle.shutdown();
    assert_eq!(most.load(Ordering::SeqCst), 4);
}

#[test]
fn nr_71_a_connection_the_manager_refuses_is_closed_at_once() {
    let peer = FakePeer::start(Behavior::Serve(Vec::new()));
    let storage = MemStorage::default();
    // Local peers are not allowed.
    let mut cfg = config(&peer, 30);
    cfg.allow_local = false;
    let mut handle = start_system(cfg, &storage);
    let seen = wait_for(&handle, 10, is_note).expect("no note");
    match seen.last() {
        Some(AppEvent::Note(text)) => assert!(text.contains("local"), "{text}"),
        other => panic!("{other:?}"),
    }
    assert!(until(5, || peer.closed_by_driver.load(Ordering::SeqCst) == 1));
    handle.shutdown();
}

#[test]
fn nr_76_a_tor_v3_address_in_the_book_is_not_dialed() {
    let mut book = AddressBook::new();
    let added = book.add(
        &AddrV2::TorV3([7; 32]),
        8333,
        ServiceFlags::NETWORK,
        Source::Own,
        1,
    );
    assert_eq!(added, openbmt_net::Added::New);
    let storage = MemStorage::with(Key::Book, book.to_bytes());
    let peer = FakePeer::start(Behavior::Silent);
    let (net, log, _) = net_to(peer.addr, Vec::new());
    let mut handle = start(bare(), &storage, net);
    thread::sleep(Duration::from_millis(1_200));
    assert!(log.lock().unwrap().is_empty());
    handle.shutdown();
}

#[test]
fn nr_22_addresses_a_peer_tells_of_go_to_the_book() {
    let told = sock(8, 8, 4, 4, 8333);
    let peer = FakePeer::start(Behavior::ServeAndTell(chain(1), vec![told]));
    let storage = MemStorage::default();
    let mut handle = start_system(config(&peer, 30), &storage);
    // The headers follow the addresses, so their event comes after.
    wait_for(&handle, 10, |e| matches!(e, AppEvent::Chain { .. })).expect("no headers");
    handle.shutdown();
    let book = stored_book(&storage);
    let entry = book.get(&v4(8, 8, 4, 4), 8333).unwrap();
    assert!(matches!(entry.source, Source::Peer(_)));
    assert_eq!(book.len(), 1);
}

#[test]
fn nr_72_seeds_are_asked_when_the_book_is_empty() {
    let peer = FakePeer::start(Behavior::Serve(Vec::new()));
    let storage = MemStorage::default();
    let (net, log, lookups) = net_to(peer.addr, vec![sock(8, 8, 8, 8, 18_444)]);
    let mut cfg = bare();
    cfg.seeds = vec!["seed.test".to_string()];
    let mut handle = start(cfg, &storage, net);
    wait_for(&handle, 10, has_peer).expect("no ready peer");
    // The name itself and the name for filter peers.
    assert_eq!(lookups.load(Ordering::SeqCst), 2);
    assert_eq!(
        log.lock().unwrap().first().cloned(),
        Some((Target::Addr(sock(8, 8, 8, 8, 18_444)), None))
    );
    handle.shutdown();
    let book = stored_book(&storage);
    let entry = book.get(&v4(8, 8, 8, 8), 18_444).unwrap();
    assert_eq!(entry.source, Source::Seed);
    assert!(entry.last_good.is_some());
}

#[test]
fn nr_72_a_seed_is_also_asked_for_filter_peers_and_says_which_they_are() {
    let storage = MemStorage::default();
    let asked = Arc::new(Mutex::new(Vec::new()));
    let seen = asked.clone();
    let net = Net {
        dial: Arc::new(|_, _, _| Err(DialError::Connect(std::io::ErrorKind::ConnectionRefused))),
        resolve: Arc::new(move |name, port| {
            seen.lock().unwrap().push(name.to_string());
            if name.starts_with("x49.") {
                Ok(vec![sock(9, 9, 9, 9, port)])
            } else {
                Ok(vec![sock(8, 8, 8, 8, port)])
            }
        }),
    };
    let mut cfg = bare();
    cfg.seeds = vec!["seed.test".to_string()];
    let mut handle = start(cfg, &storage, net);
    wait_for(&handle, 2, |e| matches!(e, AppEvent::Peers(..))).expect("no event");
    thread::sleep(Duration::from_millis(800));
    handle.shutdown();
    let mut names = asked.lock().unwrap().clone();
    names.sort();
    assert_eq!(names, vec!["seed.test", "x49.seed.test"]);
    let book = stored_book(&storage);
    let plain = book.get(&v4(8, 8, 8, 8), 18_444).unwrap();
    let filter = book.get(&v4(9, 9, 9, 9), 18_444).unwrap();
    assert!(!plain.services.has(ServiceFlags::COMPACT_FILTERS));
    assert!(filter.services.has(ServiceFlags::COMPACT_FILTERS));
    assert_eq!(filter.source, Source::Seed);
}

#[test]
fn core_seeds_are_the_names_bitcoin_core_ships_and_regtest_has_none() {
    let main = crate::core_seeds(bitcoin::Network::Bitcoin);
    assert_eq!(main.len(), 7);
    assert!(main.iter().all(|n| !n.ends_with('.') && !n.contains(' ')));
    assert!(crate::core_seeds(bitcoin::Network::Regtest).is_empty());
    assert!(DriverConfig::new(bitcoin::Network::Regtest, 0)
        .seeds
        .is_empty());
}

#[test]
fn nr_72_a_hung_lookup_does_not_hang_the_driver() {
    let storage = MemStorage::default();
    let net = Net {
        dial: Arc::new(|_, _, _| Err(DialError::NameWithoutProxy)),
        resolve: Arc::new(|_, _| {
            thread::sleep(Duration::from_secs(20));
            Ok(Vec::new())
        }),
    };
    let mut cfg = bare();
    cfg.seeds = vec!["hang.test".to_string()];
    let mut handle = start(cfg, &storage, net);
    // The driver still answers: the first event is there at once.
    wait_for(&handle, 2, |e| matches!(e, AppEvent::Peers(..))).expect("no event");
    thread::sleep(Duration::from_millis(500));
    let start = Instant::now();
    handle.shutdown();
    assert!(start.elapsed() < Duration::from_secs(5));
}

#[test]
fn nr_26_with_a_proxy_a_seed_name_is_dialed_through_it_and_never_resolved() {
    let peer = FakePeer::start(Behavior::Serve(Vec::new()));
    let storage = MemStorage::default();
    let (net, log, lookups) = net_to(peer.addr, vec![sock(8, 8, 8, 8, 18_444)]);
    let proxy = sock(127, 0, 0, 1, 9_050);
    let mut cfg = bare();
    cfg.proxy = Some(proxy);
    cfg.seeds = vec!["seed.test".to_string()];
    let mut handle = start(cfg, &storage, net);
    let seen = wait_for(&handle, 10, has_peer).expect("no ready peer");
    match seen.last() {
        Some(AppEvent::Peers(peers, ..)) => {
            assert_eq!(peers.first().map(|p| p.addr), Some(PeerAddr::Named));
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(lookups.load(Ordering::SeqCst), 0);
    assert_eq!(
        log.lock().unwrap().first().cloned(),
        Some((Target::Name("seed.test".to_string(), 18_444), Some(proxy)))
    );
    handle.shutdown();
}

#[test]
fn nr_26_the_driver_reaches_a_peer_through_a_socks5_proxy_end_to_end() {
    // A proxy that checks the request, then acts as the peer.
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let proxy = listener.local_addr().unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let (asked_tx, asked_rx) = mpsc::channel();
    let s = stop.clone();
    let server = thread::spawn(move || {
        use std::io::{Read, Write};
        let (mut stream, _) = listener.accept().unwrap();
        let mut greeting = [0u8; 4];
        stream.read_exact(&mut greeting).unwrap();
        stream.write_all(&[5, 0]).unwrap();
        let mut head = [0u8; 5];
        stream.read_exact(&mut head).unwrap();
        let mut rest = vec![0u8; usize::from(head[4]) + 2];
        stream.read_exact(&mut rest).unwrap();
        asked_tx.send((greeting, head, rest)).unwrap();
        stream
            .write_all(&[5, 0, 0, 1, 127, 0, 0, 1, 0x48, 0x5c])
            .unwrap();
        serve_stream(stream, &Behavior::Serve(Vec::new()), &s);
    });
    let storage = MemStorage::default();
    let mut cfg = bare();
    cfg.proxy = Some(proxy);
    cfg.seeds = vec!["seed.test".to_string()];
    let mut handle = start_system(cfg, &storage);
    wait_for(&handle, 10, has_peer).expect("no ready peer");
    let (greeting, head, rest) = asked_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(greeting, [5, 2, 0, 2]);
    assert_eq!(head, [5, 1, 0, 3, 9]);
    assert_eq!(&rest[..9], b"seed.test");
    // The regtest port, 18444.
    assert_eq!(rest[9..], 18_444u16.to_be_bytes());
    handle.shutdown();
    stop.store(true, Ordering::SeqCst);
    server.join().unwrap();
}

#[test]
fn nr_73_headers_are_stored_at_shutdown_and_loaded_again() {
    let headers = chain(5);
    let peer = FakePeer::start(Behavior::Serve(headers.clone()));
    let storage = MemStorage::default();
    let mut handle = start_system(config(&peer, 2), &storage);
    wait_for(&handle, 10, tip_is(5)).expect("the headers did not arrive");
    handle.shutdown();
    let store = stored_headers(&storage);
    assert_eq!(store.tip(), (5, headers[4].block_hash()));
    // A new driver over the same storage starts at that tip, and asks for
    // headers from it.
    let again = FakePeer::start(Behavior::Serve(headers.clone()));
    let mut handle = start_system(config(&again, 2), &storage);
    let seen = wait_for(&handle, 10, has_peer).expect("no ready peer");
    match seen.first() {
        Some(AppEvent::Peers(_, _, tip)) => {
            assert_eq!(*tip, (5, u64::from(headers[4].time)));
        }
        other => panic!("{other:?}"),
    }
    assert!(until(5, || !again.getheaders.lock().unwrap().is_empty()));
    let payload = again.getheaders.lock().unwrap()[0].clone();
    // Version (4 bytes), a count, then the newest hash first.
    assert_eq!(payload[4], 6);
    assert_eq!(payload[5..37], headers[4].block_hash().to_byte_array());
    // Nothing joined: the five headers it sent back were known.
    thread::sleep(Duration::from_millis(300));
    while let Ok(event) = handle.events().try_recv() {
        assert!(!matches!(event, AppEvent::Chain { .. }), "{event:?}");
    }
    handle.shutdown();
}

#[test]
fn nr_73_stored_files_that_do_not_read_back_are_replaced_and_noted() {
    let storage = MemStorage::default();
    storage
        .values
        .lock()
        .unwrap()
        .insert(Key::Headers, b"not a header store".to_vec());
    storage
        .values
        .lock()
        .unwrap()
        .insert(Key::Book, b"not an address book".to_vec());
    let mut handle = start(
        bare(),
        &storage,
        net_to(sock(127, 0, 0, 1, 1), Vec::new()).0,
    );
    let first = handle
        .events()
        .recv_timeout(Duration::from_secs(5))
        .unwrap();
    let second = handle
        .events()
        .recv_timeout(Duration::from_secs(5))
        .unwrap();
    match (first, second) {
        (AppEvent::Note(a), AppEvent::Note(b)) => {
            assert!(a.contains("headers"), "{a}");
            assert!(b.contains("address book"), "{b}");
        }
        other => panic!("{other:?}"),
    }
    handle.shutdown();
    // Both were replaced by files that read back.
    assert_eq!(stored_headers(&storage).tip().0, 0);
    assert!(stored_book(&storage).is_empty());
}

#[test]
fn nr_73_a_storage_that_cannot_be_read_stops_the_start() {
    struct Broken;
    impl crate::env::Storage for Broken {
        type Error = String;
        fn load(&mut self, _: Key) -> Result<Option<Vec<u8>>, String> {
            Err("disk gone".to_string())
        }
        fn save(&mut self, _: Key, _: &[u8]) -> Result<(), String> {
            Err("disk gone".to_string())
        }
    }
    let err = Driver::spawn(
        bare(),
        Broken,
        SystemClock,
        CountingRandomness(1),
        Vec::new(),
    )
    .unwrap_err();
    assert!(err.to_string().contains("disk gone"), "{err}");
    let mut cfg = bare();
    cfg.target_peers = 0;
    let err = Driver::spawn(
        cfg,
        MemStorage::default(),
        SystemClock,
        CountingRandomness(1),
        Vec::new(),
    )
    .unwrap_err();
    assert!(err.to_string().starts_with("NR-20"), "{err}");
}

#[test]
fn nr_77_shutdown_ends_every_thread_within_five_seconds_with_a_silent_peer() {
    let peer = FakePeer::start(Behavior::Silent);
    let storage = MemStorage::default();
    let mut handle = start_system(config(&peer, 30), &storage);
    assert!(until(10, || peer.accept_count() == 1));
    thread::sleep(Duration::from_millis(300));
    let start = Instant::now();
    handle.shutdown();
    assert!(
        start.elapsed() < Duration::from_secs(5),
        "{:?}",
        start.elapsed()
    );
    // The socket was closed by the driver.
    assert!(until(5, || peer.closed_by_driver.load(Ordering::SeqCst) == 1));
    // The files were saved, and a second call does nothing.
    assert!(storage.get(Key::Headers).is_some());
    assert!(storage.get(Key::Book).is_some());
    handle.shutdown();
    // The last event is the end.
    let mut last = None;
    while let Ok(event) = handle.events().try_recv() {
        last = Some(event);
    }
    assert_eq!(last, Some(AppEvent::Stopped));
}

#[test]
fn nr_77_a_handle_that_is_dropped_stops_the_driver() {
    let peer = FakePeer::start(Behavior::Silent);
    let storage = MemStorage::default();
    let handle = start_system(config(&peer, 30), &storage);
    assert!(until(10, || peer.accept_count() == 1));
    drop(handle);
    assert!(until(5, || peer.closed_by_driver.load(Ordering::SeqCst) == 1));
    assert!(storage.get(Key::Headers).is_some());
}

#[test]
fn nr_74_an_app_that_does_not_read_stops_the_readers() {
    let headers = chain(1_000);
    let peer = FakePeer::start(Behavior::Flood(headers));
    let storage = MemStorage::default();
    let mut handle = start_system(config(&peer, 30), &storage);
    // The peer sends for about two seconds; the app reads nothing.
    assert!(until(10, || !peer.getheaders.lock().unwrap().is_empty()));
    thread::sleep(Duration::from_millis(3_000));
    let shared = handle.shared().clone();
    let first = shared.chunks_read.load(Ordering::Relaxed);
    thread::sleep(Duration::from_millis(500));
    let second = shared.chunks_read.load(Ordering::Relaxed);
    // The readers have stopped: the channels hold what they can, and the
    // rest of the 1,000 messages waits in the sockets.
    assert_eq!(first, second);
    assert!(first < 400, "{first} chunks were read");
    // Reading the events lets it go on, and nothing is lost.
    let mut joined = 0usize;
    let end = Instant::now() + Duration::from_secs(30);
    while joined < 1_000 && Instant::now() < end {
        if let Ok(AppEvent::Chain { joined: j, .. }) =
            handle.events().recv_timeout(Duration::from_millis(200))
        {
            joined += j.len();
        }
    }
    assert_eq!(joined, 1_000);
    handle.shutdown();
}

#[test]
fn nr_75_peers_is_sent_at_the_start_on_change_and_every_ten_seconds() {
    let storage = MemStorage::default();
    let mut handle = start(
        bare(),
        &storage,
        net_to(sock(127, 0, 0, 1, 1), Vec::new()).0,
    );
    let first = Instant::now();
    wait_for(&handle, 5, |e| matches!(e, AppEvent::Peers(..))).expect("no first event");
    let first_at = first.elapsed();
    // Nothing changes, and the next one comes by itself.
    let waited = Instant::now();
    wait_for(&handle, 13, |e| matches!(e, AppEvent::Peers(..))).expect("no second event");
    let gap = waited.elapsed();
    assert!(gap >= Duration::from_secs(8), "{gap:?} after {first_at:?}");
    assert!(gap <= Duration::from_secs(11), "{gap:?}");
    handle.shutdown();
}

// ----- NR-74: the byte budget ----------------------------------------------

/// Wait until the bytes waiting for the app stop changing, and return them.
fn settled_queue(handle: &Handle) -> usize {
    let mut last = usize::MAX;
    let mut same = 0;
    let end = Instant::now() + Duration::from_secs(20);
    while Instant::now() < end {
        thread::sleep(Duration::from_millis(300));
        let now = handle.events().queued_bytes();
        same = if now == last { same + 1 } else { 0 };
        last = now;
        if same >= 5 && now > 0 {
            return now;
        }
    }
    panic!("the queue did not settle: {last} bytes");
}

#[test]
fn nr_74_a_stalled_app_stops_the_readers_at_the_byte_budget_not_at_the_count() {
    // Twenty blocks of 400 kB are 20 events, far below the 64 that the
    // count allows, but 8 MB. The budget is 1 MB.
    let peer = FakePeer::start(Behavior::ServeBlocks(blocks(20, 400_000)));
    let storage = MemStorage::default();
    let mut cfg = config(&peer, 30);
    cfg.event_bytes = 1_000_000;
    let mut handle = start_clocked(
        cfg,
        &storage,
        Net::system(),
        &TestClock::at(1_700_000_000),
        watched(),
    );
    let queued = settled_queue(&handle);
    // Two blocks fit; a third would pass the budget.
    assert!(queued > 800_000 && queued <= 1_000_000, "{queued} bytes");
    // The readers have stopped.
    let shared = handle.shared().clone();
    let first = shared.chunks_read.load(Ordering::Relaxed);
    thread::sleep(Duration::from_millis(500));
    assert_eq!(first, shared.chunks_read.load(Ordering::Relaxed));
    // Taking the events lets them run again, and every block arrives.
    let mut got = 0usize;
    let end = Instant::now() + Duration::from_secs(60);
    while got < 20 && Instant::now() < end {
        if let Ok(AppEvent::Block { .. }) = handle.events().recv_timeout(Duration::from_millis(200))
        {
            got += 1;
        }
    }
    assert_eq!(got, 20);
    assert_eq!(handle.events().queued_bytes(), 0);
    handle.shutdown();
}

#[test]
fn nr_74_one_event_larger_than_the_budget_is_still_handed_over() {
    // The budget is 100 kB and each block is 400 kB. The first block goes
    // through an empty queue; the second waits until the app has taken it.
    let peer = FakePeer::start(Behavior::ServeBlocks(blocks(2, 400_000)));
    let storage = MemStorage::default();
    let mut cfg = config(&peer, 30);
    cfg.event_bytes = 100_000;
    let mut handle = start_clocked(
        cfg,
        &storage,
        Net::system(),
        &TestClock::at(1_700_000_000),
        watched(),
    );
    let queued = settled_queue(&handle);
    assert!(queued > 400_000 && queued < 500_000, "{queued} bytes");
    wait_for(&handle, 10, is_block).expect("the large block was not handed over");
    wait_for(&handle, 10, is_block).expect("the second block did not follow");
    assert_eq!(handle.events().queued_bytes(), 0);
    handle.shutdown();
}

// ----- NR-73: when the files are saved ------------------------------------

#[test]
fn nr_73_the_headers_are_stored_before_a_block_is_handed_to_the_app() {
    let chain_blocks = blocks(1, 100);
    let hash = chain_blocks[0].block_hash();
    let peer = FakePeer::start(Behavior::ServeBlocks(chain_blocks));
    let storage = MemStorage::default();
    // The clock never moves, so the minute rule cannot be what saves them.
    let clock = TestClock::at(1_700_000_000);
    let mut handle = start_clocked(
        config(&peer, 30),
        &storage,
        Net::system(),
        &clock,
        watched(),
    );
    wait_for(&handle, 10, is_block).expect("no block reached the app");
    // At the moment the app has the block, the stored headers end at it.
    let store = stored_headers(&storage);
    assert_eq!(store.tip(), (1, hash));
    assert_eq!(storage.save_count(Key::Headers), 1);
    handle.shutdown();
}

#[test]
fn nr_73_a_second_block_at_the_same_tip_does_not_save_the_headers_again() {
    // Both headers come in one message, so the tip is the second block's
    // when the first block is handed over.
    let chain_blocks = blocks(2, 100);
    let tip = chain_blocks[1].block_hash();
    let peer = FakePeer::start(Behavior::ServeBlocks(chain_blocks));
    let storage = MemStorage::default();
    let clock = TestClock::at(1_700_000_000);
    let mut handle = start_clocked(
        config(&peer, 30),
        &storage,
        Net::system(),
        &clock,
        watched(),
    );
    wait_for(&handle, 10, is_block).expect("no first block");
    assert_eq!(stored_headers(&storage).tip(), (2, tip));
    wait_for(&handle, 10, is_block).expect("no second block");
    settle();
    // One save for both blocks. The save at shutdown comes after this.
    assert_eq!(storage.save_count(Key::Headers), 1);
    handle.shutdown();
}

// ----- NR-73: when the files are saved ------------------------------------

#[test]
fn nr_73_the_headers_are_saved_a_minute_after_the_last_save_if_the_tip_moved() {
    let headers = chain(5);
    let peer = FakePeer::start(Behavior::Serve(headers.clone()));
    let storage = MemStorage::default();
    let start_time = 1_700_000_000;
    let clock = TestClock::at(start_time);
    let mut handle = start_clocked(
        config(&peer, 30),
        &storage,
        Net::system(),
        &clock,
        Vec::new(),
    );
    // The tip has moved, and nothing is saved yet.
    wait_for(&handle, 10, tip_is(5)).expect("the headers did not arrive");
    settle();
    assert_eq!(storage.save_count(Key::Headers), 0);
    // Not before a minute has passed ...
    clock.set(start_time + HEADER_SAVE_SECS - 1);
    settle();
    assert_eq!(storage.save_count(Key::Headers), 0);
    // ... and then at once.
    clock.set(start_time + HEADER_SAVE_SECS);
    assert!(until(5, || storage.save_count(Key::Headers) == 1));
    assert_eq!(stored_headers(&storage).tip(), (5, headers[4].block_hash()));
    // The tip has not moved since, so the next minutes save nothing.
    clock.set(start_time + 3 * HEADER_SAVE_SECS);
    settle();
    assert_eq!(storage.save_count(Key::Headers), 1);
    handle.shutdown();
}

#[test]
fn nr_73_the_anchors_are_set_and_the_book_saved_every_ten_minutes() {
    let peer = FakePeer::start(Behavior::Honest(Vec::new()));
    let storage = MemStorage::with(Key::Book, book_bytes(&[(8, 8, 8, 8, 8333)], &[]));
    let (net, _, _) = net_to(peer.addr, Vec::new());
    let start_time = 1_700_000_000;
    let clock = TestClock::at(start_time);
    let mut handle = start_clocked(bare(), &storage, net, &clock, Vec::new());
    wait_for(&handle, 10, has_peer).expect("no ready peer");
    // Nothing is saved while the ten minutes run, and the clock is moved in
    // steps so that the manager's own timers see an ordinary peer.
    for step in [100, 300, ANCHOR_SECS - 1] {
        clock.set(start_time + step);
        settle();
    }
    assert_eq!(storage.save_count(Key::Book), 0);
    assert_eq!(storage.save_count(Key::Headers), 0);
    // At ten minutes the book is saved, with the connected peer as the
    // anchor.
    clock.set(start_time + ANCHOR_SECS);
    assert!(until(5, || storage.save_count(Key::Book) == 1));
    assert_eq!(
        stored_book(&storage).anchors(),
        vec![(v4(8, 8, 8, 8), 8333)]
    );
    // Headers are not part of it: the tip has not moved.
    assert_eq!(storage.save_count(Key::Headers), 0);
    // And again ten minutes later, not before.
    clock.set(start_time + ANCHOR_SECS + 300);
    settle();
    assert_eq!(storage.save_count(Key::Book), 1);
    clock.set(start_time + 2 * ANCHOR_SECS);
    assert!(until(5, || storage.save_count(Key::Book) == 2));
    handle.shutdown();
}

// ----- the write timeout ---------------------------------------------------

#[test]
fn nr_73_a_peer_that_stops_reading_is_dropped_after_the_write_timeout() {
    // A connected pair whose far end never reads, filled until a write
    // blocks, so that the first thing the driver writes to it (its version
    // message) cannot be sent.
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let mut near = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    let (mut far, _) = listener.accept().unwrap();
    near.set_write_timeout(Some(Duration::from_millis(200)))
        .unwrap();
    let chunk = [0u8; 65_536];
    let mut filled = 0usize;
    while let Ok(n) = near.write(&chunk) {
        filled += n;
    }
    while near.write(&[0u8]).is_ok() {
        filled += 1;
    }
    assert!(filled > 0);

    let stalled: Arc<Mutex<Option<TcpStream>>> = Arc::new(Mutex::new(Some(near)));
    let dials: Arc<Mutex<Vec<Instant>>> = Arc::default();
    let (take, log) = (stalled.clone(), dials.clone());
    let net = Net {
        dial: Arc::new(move |_, _, _| {
            log.lock().unwrap().push(Instant::now());
            match take.lock().unwrap().take() {
                Some(stream) => Ok(stream),
                None => Err(DialError::Connect(ErrorKind::ConnectionRefused)),
            }
        }),
        resolve: Arc::new(|_, _| Ok(Vec::new())),
    };
    let mut cfg = bare();
    cfg.allow_local = true;
    cfg.fixed_peers = vec![sock(127, 0, 0, 1, 9)];
    cfg.redial_pause_secs = 1;
    let storage = MemStorage::default();
    let mut handle = start(cfg, &storage, net);

    // The driver is blocked in the write for the timeout, then drops the
    // peer, pauses it for a second and dials again. The gap between the two
    // dials is therefore the timeout and the pause, and no more.
    assert!(until(20, || dials.lock().unwrap().len() >= 2));
    let at = dials.lock().unwrap().clone();
    let gap = at[1].duration_since(at[0]);
    let timeout = Duration::from_secs(WRITE_TIMEOUT_SECS);
    assert!(gap >= timeout, "the write did not block: {gap:?}");
    assert!(
        gap < timeout + Duration::from_secs(1 + 2),
        "the driver stalled for {gap:?}"
    );
    // The driver closed its end: the far end reads what was written, then
    // the end of the stream (or a reset).
    far.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    let mut sink = vec![0u8; 65_536];
    let ended = loop {
        match far.read(&mut sink) {
            Ok(0) => break true,
            Ok(_) => {}
            Err(e) => break !matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut),
        }
    };
    assert!(ended, "the driver did not close the peer");
    handle.shutdown();
}
