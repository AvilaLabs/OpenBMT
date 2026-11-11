//! The counter's link to the owner through the calls the app makes
//! (CR-130 to CR-136): a relay on the loopback, a courier standing in for
//! the owner, and a node on a fake driver.

use std::net::{SocketAddr, TcpListener};
use std::sync::Arc;
use std::time::{Duration, Instant};

use bitcoin::hashes::Hash;
use openbmt_core::heartbeat::WatcherHeartbeat;
use openbmt_core::identity::Identity;
use openbmt_core::relay::{RelayHint, RelayRoute, SignedRelayHint};
use openbmt_courier::{Courier, CourierConfig, FirstStart, Received, RelayPost};
use openbmt_link::{
    ClientConfig, Kind, MemoryStore, Relay, RelayConfig, Role, Route, Server, ServerConfig,
};
use openbmt_net_io::SystemClock;

use super::kit::{genesis, id, peers_event, record_with, signed_settings, Rig, Scratch, T0};
use super::wire::{LinkV, Rd, W};
use crate::StartConfig;

const OP_PUMP: u16 = 10;
const OP_LINK_STEP: u16 = 24;
const OP_LINK_STATE: u16 = 25;
const OP_TAKE_RELAY: u16 = 56;

const ONION: &str = "a4dqobyha4dqobyha4dqobyha4dqobyha4dqobyha4dqobyha4dwc6ad";

/// A relay on the loopback whose members are the owner (1) and the counter
/// (3); its own key is 99.
fn relay() -> Server {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let mut relay = Relay::new(id(99), RelayConfig::default(), MemoryStore::new()).unwrap();
    relay
        .set_members(&[id(1).device_id(), id(3).device_id()])
        .unwrap();
    Server::start(
        listener,
        relay,
        Arc::new(SystemClock),
        ServerConfig::default(),
    )
    .unwrap()
}

fn hint_for(addr: SocketAddr) -> RelayHint {
    RelayHint::new(RelayRoute::Direct(addr), id(99).device_id().to_bytes()).unwrap()
}

/// The owner's end: a courier on the relay, with the counter as its peer.
fn owner_courier(addr: SocketAddr, dir: &Scratch) -> Courier<RelayPost> {
    Courier::start(
        CourierConfig {
            dir: dir.path().to_path_buf(),
            identity: id(1),
            role: Role::Owner,
            peers: [(id(3).device_id(), Role::Counter)].into_iter().collect(),
            route: Route::Direct(addr),
            relay: id(99).device_id(),
            client: ClientConfig::default(),
        },
        FirstStart::Yes,
    )
    .unwrap()
}

fn heartbeat(seq: u64) -> Vec<u8> {
    WatcherHeartbeat {
        counter: id(3).device_id(),
        seq,
        time: T0,
        tip_height: 0,
        tip_hash: genesis().to_byte_array(),
        healthy: true,
    }
    .sign(&id(1))
    .to_bytes()
}

fn config_for(rig: &Rig, relay: Option<RelayHint>, link_proxy: Option<&str>) -> StartConfig {
    let mut record = record_with(signed_settings());
    record.relay = relay;
    StartConfig {
        dir: rig.dir().to_str().unwrap().to_string(),
        now: T0,
        record: record.to_bytes(),
        proxy: None,
        own_node: None,
        lightning: None,
        link_proxy: link_proxy.map(str::to_string),
    }
}

fn pump(rig: &Rig, handle: u64) {
    let response = rig.call(OP_PUMP, handle, &W::new().u64(T0).u32(50).0);
    let mut r = Rd::ok(&response);
    let _ = r.pump();
    r.done();
}

fn step(rig: &Rig, handle: u64, now: u64, again: bool) -> (LinkV, Vec<String>) {
    let response = rig.call(
        OP_LINK_STEP,
        handle,
        &W::new().u64(now).u8(u8::from(again)).0,
    );
    let mut r = Rd::ok(&response);
    let link = r.link();
    let notes = r.list(Rd::string);
    r.done();
    (link, notes)
}

fn state(rig: &Rig, handle: u64) -> Option<LinkV> {
    let response = rig.call(OP_LINK_STATE, handle, &[]);
    let mut r = Rd::ok(&response);
    let link = r.opt(Rd::link);
    r.done();
    link
}

fn of_kind(got: &[Received], kind: Kind) -> usize {
    got.iter().filter(|r| r.kind == kind).count()
}

#[test]
fn cr_135_a_step_takes_the_owner_s_heartbeat_and_sends_the_counter_s_records() {
    let server = relay();
    let owner_dir = Scratch::new();
    let mut owner = owner_courier(server.local_addr(), &owner_dir);
    // The owner's mailbox exists before the counter sends to it.
    owner.poll().unwrap();
    let rig = Rig::new();
    let handle = rig.start_with(&config_for(&rig, Some(hint_for(server.local_addr())), None));
    rig.send(vec![peers_event()]);
    pump(&rig, handle);
    // Before any step the line says it is connecting, and is not connected.
    let before = state(&rig, handle).expect("a link");
    assert!(!before.connected && before.since.is_none() && !before.refused);
    assert_eq!(before.words.heading, "Connecting to the Box");

    // The first step makes the counter's mailbox at the relay and sends the
    // counter's records; the owner's heartbeat can then be left for it.
    let (first, _) = step(&rig, handle, T0, false);
    assert!(first.connected, "{first:?}");
    assert_eq!(first.words.heading, "Connected to the Box");
    assert_eq!(first.heartbeats_taken, 0);
    // The counter's records came to the owner, among them a check-in.
    let got = owner.poll().unwrap();
    assert!(of_kind(&got, Kind::CheckIn) > 0, "{got:?}");
    owner
        .send(&id(3).device_id(), Kind::Heartbeat, &heartbeat(1))
        .unwrap();
    let (link, _) = step(&rig, handle, T0 + 5, false);
    assert!(link.connected, "{link:?}");
    assert_eq!(link.heartbeats_taken, 1);
    assert_eq!(state(&rig, handle), Some(link));
}

#[test]
fn cr_135_a_pairing_with_no_relay_has_no_link_and_says_so() {
    let rig = Rig::new();
    let handle = rig.start_with(&config_for(&rig, None, None));
    assert_eq!(state(&rig, handle), None);
    let response = rig.call(OP_LINK_STEP, handle, &W::new().u64(T0).u8(0).0);
    let fault = Rd::error(&response).expect("a refusal");
    assert_eq!(fault.rule, "link-none");
    assert_eq!(fault.words.heading, "No link to the Box");
}

#[test]
fn cr_136_a_relay_that_is_not_there_is_not_reachable_since_the_first_failure() {
    // A port nothing listens on.
    let addr = {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap()
    };
    let rig = Rig::new();
    let handle = rig.start_with(&config_for(&rig, Some(hint_for(addr)), None));
    let (link, _) = step(&rig, handle, T0, false);
    assert!(!link.connected && !link.refused);
    assert_eq!(link.since, Some(T0));
    assert_eq!(link.words.heading, "Box not reachable");
    // The time stays that of the first failure; Try again calls at once.
    let (link, _) = step(&rig, handle, T0 + 100, true);
    assert_eq!(link.since, Some(T0));
    assert_eq!(state(&rig, handle), Some(link));
}

#[test]
fn cr_136_a_relay_that_does_not_know_the_counter_is_refused_and_not_unreachable() {
    // The relay has other members: the counter is refused.
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let mut relay = Relay::new(id(99), RelayConfig::default(), MemoryStore::new()).unwrap();
    relay
        .set_members(&[id(1).device_id(), id(5).device_id()])
        .unwrap();
    let server = Server::start(
        listener,
        relay,
        Arc::new(SystemClock),
        ServerConfig::default(),
    )
    .unwrap();
    let rig = Rig::new();
    let handle = rig.start_with(&config_for(&rig, Some(hint_for(server.local_addr())), None));
    let (link, _) = step(&rig, handle, T0, false);
    assert!(!link.connected);
    assert!(link.refused, "{link:?}");
    assert_eq!(link.words.heading, "Box refused this counter");
}

#[test]
fn cr_134_an_onion_relay_with_no_proxy_starts_the_counter_and_says_orbot_is_needed() {
    let hint = RelayHint::new(
        RelayRoute::Onion {
            name: ONION.to_string(),
            port: 48333,
        },
        id(99).device_id().to_bytes(),
    )
    .unwrap();
    let rig = Rig::new();
    let handle = rig.start_with(&config_for(&rig, Some(hint), None));
    let link = state(&rig, handle).expect("a link line");
    assert!(!link.connected);
    assert_eq!(link.words.heading, "Orbot needed for the Box");
    // A step has nothing to call and changes nothing.
    let (after, _) = step(&rig, handle, T0, false);
    assert_eq!(after.words.heading, "Orbot needed for the Box");
    assert!(!rig.dir().join("link-inbox").exists());
}

#[test]
fn cr_135_a_step_that_waits_for_the_box_does_not_hold_up_the_counter() {
    // A relay that accepts and says nothing until the test lets go.
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let (held_tx, held_rx) = std::sync::mpsc::channel();
    let (go_tx, go_rx) = std::sync::mpsc::channel::<()>();
    let silent = std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        held_tx.send(()).unwrap();
        let _ = go_rx.recv();
        drop(stream);
    });
    let rig = Rig::new();
    let handle = rig.start_with(&config_for(&rig, Some(hint_for(addr)), None));
    rig.send(vec![peers_event()]);
    std::thread::scope(|s| {
        let stepper = s.spawn(|| step(&rig, handle, T0, false));
        held_rx.recv_timeout(Duration::from_secs(10)).unwrap();
        std::thread::sleep(Duration::from_millis(200));
        let began = Instant::now();
        // The screen's line, the pump and a second step answer at once.
        assert!(state(&rig, handle).is_some());
        pump(&rig, handle);
        let (again, _) = step(&rig, handle, T0 + 1, false);
        assert!(!again.connected);
        assert!(
            began.elapsed() < Duration::from_secs(5),
            "waited {:?}",
            began.elapsed()
        );
        go_tx.send(()).unwrap();
        let (done, _) = stepper.join().unwrap();
        assert!(!done.connected);
    });
    silent.join().unwrap();
}

fn signed(hint: RelayHint, by: &Identity) -> Vec<u8> {
    SignedRelayHint::sign(hint, by).to_bytes()
}

fn take(record: &[u8], frame: &[u8]) -> Vec<u8> {
    rig_call(OP_TAKE_RELAY, &W::new().bytes(record).bytes(frame).0)
}

fn rig_call(op: u16, fields: &[u8]) -> Vec<u8> {
    let rig = Rig::new();
    rig.call(op, 0, fields)
}

#[test]
fn cr_132_the_owner_s_relay_for_counters_is_kept_with_the_pairing_record() {
    let addr: SocketAddr = "192.168.1.20:48333".parse().unwrap();
    let old = record_with(signed_settings()).to_bytes();
    let frame = signed(hint_for(addr), &id(1));
    let response = take(&old, &frame);
    let new = Rd::ok(&response).bytes();
    assert_ne!(new, old);
    let record = openbmt_counter_node::PairingRecord::from_bytes(&new).unwrap();
    assert_eq!(record.relay, Some(hint_for(addr)));
    // The same record, with the same relay, is taken again as it is.
    let again = Rd::ok(&take(&new, &frame)).bytes();
    assert_eq!(again, new);
}

#[test]
fn cr_132_a_relay_not_signed_by_the_pinned_owner_or_not_readable_is_refused() {
    let addr: SocketAddr = "192.168.1.20:48333".parse().unwrap();
    let old = record_with(signed_settings()).to_bytes();
    // Signed by the Key app's key, or by a stranger.
    for by in [id(2), id(7)] {
        let response = take(&old, &signed(hint_for(addr), &by));
        let fault = Rd::error(&response).expect("a refusal");
        assert_eq!(fault.rule, "CR-132");
        assert_eq!(fault.words.heading, "Box code not from the owner");
    }
    // Bytes that are not a frame.
    let mut frame = signed(hint_for(addr), &id(1));
    frame.push(0);
    for bad in [frame, vec![1, 2, 3], Vec::new()] {
        let response = take(&old, &bad);
        let fault = Rd::error(&response).expect("a refusal");
        assert_eq!(fault.rule, "CR-132");
        assert_eq!(fault.words.heading, "Box code not readable");
    }
    // A record that is not a pairing is refused by the record's own rule.
    let response = take(&[0, 1, 2], &signed(hint_for(addr), &id(1)));
    let fault = Rd::error(&response).expect("a refusal");
    assert_eq!(fault.rule, "CR-97");
}
