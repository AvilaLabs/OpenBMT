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
use openbmt_core::settings::{Settings, SignedSettings};
use openbmt_courier::{Courier, CourierConfig, FirstStart, MemoryRelay, Received, RelayPost};
use openbmt_link::{
    ClientConfig, Kind, MemoryStore, Relay, RelayConfig, Role, Route, Server, ServerConfig,
};
use openbmt_net_io::SystemClock;
use openbmt_pos::{NoticeState, PaidNotice, SaleRequest, TillMessage};

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
        .set_members(&[id(1).device_id(), id(3).device_id()], 0)
        .unwrap();
    Server::start(
        listener,
        relay,
        Arc::new(SystemClock),
        ServerConfig::default(),
    )
    .unwrap()
}

fn relay_with_box() -> Server {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let mut relay = Relay::new(id(99), RelayConfig::default(), MemoryStore::new()).unwrap();
    relay
        .set_members(
            &[id(1).device_id(), id(3).device_id(), id(7).device_id()],
            0,
        )
        .unwrap();
    Server::start(
        listener,
        relay,
        Arc::new(SystemClock),
        ServerConfig::default(),
    )
    .unwrap()
}

fn relay_with_boxes() -> Server {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let mut relay = Relay::new(id(99), RelayConfig::default(), MemoryStore::new()).unwrap();
    relay
        .set_members(
            &[
                id(1).device_id(),
                id(3).device_id(),
                id(7).device_id(),
                id(8).device_id(),
            ],
            0,
        )
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
        shop_seed: None,
    }
}

fn config_for_box(rig: &Rig, relay: RelayHint) -> StartConfig {
    let mut settings: Settings = signed_settings().settings;
    settings.devices.insert(id(7).device_id());
    settings.boxes.insert(id(7).device_id());
    let signed = SignedSettings {
        owner_sig: settings.sign(&id(1)),
        approver_sig: Some(settings.sign(&id(2))),
        settings,
    };
    let mut record = record_with(signed);
    record.relay = Some(relay);
    StartConfig {
        dir: rig.dir().to_str().unwrap().to_string(),
        now: T0,
        record: record.to_bytes(),
        proxy: None,
        own_node: None,
        lightning: None,
        link_proxy: None,
        shop_seed: None,
    }
}

fn box_courier(addr: SocketAddr, dir: &Scratch) -> Courier<RelayPost> {
    box_courier_for(addr, id(7), dir)
}

fn box_courier_for(addr: SocketAddr, identity: Identity, dir: &Scratch) -> Courier<RelayPost> {
    Courier::start(
        CourierConfig {
            dir: dir.path().to_path_buf(),
            identity,
            role: Role::Box,
            peers: [(id(3).device_id(), Role::Counter)].into_iter().collect(),
            route: Route::Direct(addr),
            relay: id(99).device_id(),
            client: ClientConfig::default(),
        },
        FirstStart::Yes,
    )
    .unwrap()
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
fn settings_reconcile_box_routes_without_demoting_owner_or_retaining_removed_boxes() {
    let relay = MemoryRelay::new(
        id(99),
        &[
            id(1).device_id(),
            id(3).device_id(),
            id(7).device_id(),
            id(8).device_id(),
        ],
    )
    .unwrap();
    let dir = Scratch::new();
    let mut courier = Courier::start_with(
        relay.post(),
        id(3),
        Role::Counter,
        [
            (id(1).device_id(), Role::Owner),
            (id(8).device_id(), Role::Box),
        ]
        .into_iter()
        .collect(),
        dir.path(),
        FirstStart::Yes,
    )
    .unwrap();

    // Owner precedence is explicit in settings role resolution (BX-24):
    // even if a settings Box set contains the owner key, it remains Owner.
    crate::relay::sync_box_peers(&mut courier, [id(1).device_id(), id(7).device_id()]);
    assert_eq!(courier.peers().get(&id(1).device_id()), Some(&Role::Owner));
    assert_eq!(courier.peers().get(&id(7).device_id()), Some(&Role::Box));
    assert!(!courier.peers().contains_key(&id(8).device_id()));

    // Reconciliation is idempotent and a later settings change removes the
    // old destination while adding its replacement.
    crate::relay::sync_box_peers(&mut courier, [id(7).device_id()]);
    assert_eq!(courier.peers().len(), 2);
    crate::relay::sync_box_peers(&mut courier, [id(8).device_id()]);
    assert_eq!(courier.peers().get(&id(8).device_id()), Some(&Role::Box));
    assert!(!courier.peers().contains_key(&id(7).device_id()));
    assert_eq!(courier.peers().get(&id(1).device_id()), Some(&Role::Owner));
}

#[test]
fn corrupt_notice_outbox_refuses_start_with_the_notice_words() {
    let server = relay();
    let rig = Rig::new();
    std::fs::write(rig.dir().join("notice-outbox"), b"broken").unwrap();
    let config = config_for(&rig, Some(hint_for(server.local_addr())), None);
    let response = rig.call(1, 0, &W::new().start(&config).0);
    let fault = Rd::error(&response).expect("notice outbox is refused");
    assert_eq!(fault.rule, "BX-40");
    assert_eq!(fault.words.heading, "Box notices not started");
    assert_eq!(
        fault.words.what,
        "This counter could not open its file of notices to the Box, so it did not start."
    );
    assert_eq!(
        fault.words.next,
        "Restart the app. If it keeps failing, check the phone's free space."
    );
}

#[test]
fn settings_box_receives_till_open_and_paid_notices_through_the_ffi_link() {
    const TILL_SALE: [u8; 16] = [0x47; 16];
    let server = relay_with_box();
    let owner_dir = Scratch::new();
    let mut owner = owner_courier(server.local_addr(), &owner_dir);
    owner.poll().unwrap();
    let box_dir = Scratch::new();
    let mut box_side = box_courier(server.local_addr(), &box_dir);
    box_side.poll().unwrap();

    let rig = Rig::new();
    let handle = rig.start_with(&config_for_box(&rig, hint_for(server.local_addr())));
    rig.send(vec![peers_event()]);
    pump(&rig, handle);
    step(&rig, handle, T0, false);

    // A till request handed to the node; opening it through the FFI records
    // CR-268 before the sale's Opened record.
    let entry = rig.registry().entry_for_test(handle).expect("a node");
    let request = TillMessage::Request(SaleRequest {
        sale: TILL_SALE,
        amount: openbmt_counter::Amount(500),
        currency: openbmt_counter::Currency::new(*b"USD").unwrap(),
        reference: "POS/0047".to_string(),
        expires_at: T0 + 300,
    })
    .to_bytes();
    entry
        .run(Some(T0), |node, _, _| {
            node.take_till_notice(&id(7).device_id(), &request)
                .map_err(|e| crate::error::refused_by("BX-76", e))?;
            Ok(())
        })
        .unwrap();
    let opened_response = rig.call(
        86,
        handle,
        &W::new()
            .u64(T0)
            .bytes(&TILL_SALE)
            .u64(10_000_000)
            .string("test-fixed")
            .u64(T0)
            .u8(0)
            .0,
    );
    let mut opened_reader = Rd::ok(&opened_response);
    let opened = opened_reader.opened();
    opened_reader.done();
    step(&rig, handle, T0 + 1, false);

    // Settle the opened notice at the relay, then confirm the sale. The Box
    // has not polled yet, so both notices must still be waiting for it.
    let pay = super::kit::spend(1, &[(&super::kit::script_of(opened.index), 5_000)]);
    rig.send(vec![super::kit::sighting(&pay, vec![super::kit::peer(20)])]);
    pump(&rig, handle);
    let block = super::kit::mine(super::kit::genesis(), 1, std::slice::from_ref(&pay));
    rig.send(vec![
        super::kit::joined(1, &block),
        super::kit::block_event(&block),
    ]);
    pump(&rig, handle);
    step(&rig, handle, T0 + 2, false);

    let received = box_side.poll().unwrap();
    let notices: Vec<PaidNotice> = received
        .iter()
        .filter(|message| message.kind == Kind::PaidNotice)
        .map(|message| PaidNotice::from_bytes(&message.content).unwrap())
        .collect();
    assert_eq!(
        notices.len(),
        2,
        "both notices reached the Box: {received:?}"
    );
    assert_eq!(notices[0].state, NoticeState::Opened);
    assert_eq!(notices[0].till_sale, Some(TILL_SALE));
    assert_eq!(notices[1].state, NoticeState::Paid);
    assert_eq!(notices[1].till_sale, Some(TILL_SALE));
}

#[test]
fn owner_settings_replace_box_route_before_the_same_step_sends_notices() {
    let server = relay_with_boxes();
    let owner_dir = Scratch::new();
    let mut owner = owner_courier(server.local_addr(), &owner_dir);
    owner.poll().unwrap();
    let old_box_dir = Scratch::new();
    let mut old_box = box_courier_for(server.local_addr(), id(7), &old_box_dir);
    old_box.poll().unwrap();
    let new_box_dir = Scratch::new();
    let mut new_box = box_courier_for(server.local_addr(), id(8), &new_box_dir);
    new_box.poll().unwrap();

    let rig = Rig::new();
    let handle = rig.start_with(&config_for_box(&rig, hint_for(server.local_addr())));
    rig.send(vec![peers_event()]);
    pump(&rig, handle);
    step(&rig, handle, T0, false);

    // Establish and drain one notice at the old Box.
    let first = rig.call(
        11,
        handle,
        &W::new().open(T0, 500, "USD", 10_000_000, "test-fixed").0,
    );
    let mut first_reader = Rd::ok(&first);
    first_reader.opened();
    first_reader.done();
    step(&rig, handle, T0 + 1, false);
    let first_notice = old_box.poll().unwrap();
    assert_eq!(
        of_kind(&first_notice, Kind::PaidNotice),
        1,
        "{first_notice:?}"
    );

    // The second sale is already in the node's records when the owner sends
    // a signed replacement. This same link step applies the settings and
    // sends notices, so it must route the new destination before sending.
    let second = rig.call(
        11,
        handle,
        &W::new()
            .open(T0 + 1, 500, "USD", 10_000_000, "test-fixed")
            .0,
    );
    let mut second_reader = Rd::ok(&second);
    let second_opened = second_reader.opened();
    second_reader.done();
    let mut replacement = signed_settings().settings;
    replacement.devices.remove(&id(7).device_id());
    replacement.boxes.remove(&id(7).device_id());
    replacement.devices.insert(id(8).device_id());
    replacement.boxes.insert(id(8).device_id());
    owner
        .send(
            &id(3).device_id(),
            Kind::Settings,
            &both_roots(replacement, 2),
        )
        .unwrap();
    let (link, notes) = step(&rig, handle, T0 + 2, false);
    assert_eq!(link.settings_version, 2, "{notes:?}");

    // The new Box gets the second sale in that step; the earlier notice was
    // already settled for every Box in the old settings. The removed Box has
    // no additional destination.
    let new_notices = new_box.poll().unwrap();
    assert_eq!(
        of_kind(&new_notices, Kind::PaidNotice),
        1,
        "{new_notices:?}"
    );
    let notice = PaidNotice::from_bytes(
        &new_notices
            .iter()
            .find(|message| message.kind == Kind::PaidNotice)
            .unwrap()
            .content,
    )
    .unwrap();
    assert_eq!(notice.index, second_opened.index);
    assert!(
        old_box.poll().unwrap().is_empty(),
        "removed Box still got a notice"
    );

    // The pairing record still names the old Box. Restart must initialize
    // routing from the newer settings saved by the node instead.
    let stopped = rig.call(2, handle, &[]);
    let mut stopped_reader = Rd::ok(&stopped);
    stopped_reader.pump();
    stopped_reader.done();
    rig.arm();
    let handle = rig.start_with(&config_for_box(&rig, hint_for(server.local_addr())));
    rig.send(vec![peers_event()]);
    pump(&rig, handle);
    let third = rig.call(
        11,
        handle,
        &W::new()
            .open(T0 + 3, 500, "USD", 10_000_000, "test-fixed")
            .0,
    );
    let mut third_reader = Rd::ok(&third);
    let third_opened = third_reader.opened();
    third_reader.done();
    step(&rig, handle, T0 + 4, false);
    let after_restart = new_box.poll().unwrap();
    let notices: Vec<_> = after_restart
        .iter()
        .filter(|message| message.kind == Kind::PaidNotice)
        .map(|message| PaidNotice::from_bytes(&message.content).unwrap())
        .collect();
    assert_eq!(notices.len(), 1, "{after_restart:?}");
    assert_eq!(notices[0].index, third_opened.index);
    assert!(old_box.poll().unwrap().is_empty());
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
        .set_members(&[id(1).device_id(), id(5).device_id()], 0)
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

// CR-160 to CR-165 through the calls: settings the owner sends over the link.

fn owner_alone(mut s: openbmt_core::settings::Settings, version: u32) -> Vec<u8> {
    s.version = version;
    SignedSettings {
        owner_sig: s.sign(&id(1)),
        approver_sig: None,
        settings: s,
    }
    .to_bytes()
}

fn both_roots(mut s: openbmt_core::settings::Settings, version: u32) -> Vec<u8> {
    s.version = version;
    SignedSettings {
        owner_sig: s.sign(&id(1)),
        approver_sig: Some(s.sign(&id(2))),
        settings: s,
    }
    .to_bytes()
}

/// A started counter on a relay, with the owner's courier past its first
/// poll and one step done.
fn settled() -> (Server, Courier<RelayPost>, Rig, u64, Scratch) {
    let server = relay();
    let owner_dir = Scratch::new();
    let mut owner = owner_courier(server.local_addr(), &owner_dir);
    owner.poll().unwrap();
    let rig = Rig::new();
    let handle = rig.start_with(&config_for(&rig, Some(hint_for(server.local_addr())), None));
    rig.send(vec![peers_event()]);
    pump(&rig, handle);
    step(&rig, handle, T0, false);
    (server, owner, rig, handle, owner_dir)
}

#[test]
fn cr_160_settings_the_owner_sends_are_applied_and_used_at_the_next_start() {
    let (server, mut owner, rig, handle, _dir) = settled();
    let mut tighter = signed_settings().settings;
    tighter.light_per_sale.sats -= 1;
    owner
        .send(&id(3).device_id(), Kind::Settings, &owner_alone(tighter, 2))
        .unwrap();
    let (link, notes) = step(&rig, handle, T0 + 5, false);
    assert_eq!(link.settings_version, 2, "{notes:?}");
    assert!(!link.restart_needed && link.settings_line.is_none());
    // The owner is sent the records, with the Setting record among them.
    let (again, _) = step(&rig, handle, T0 + 10, false);
    assert!(again.connected);
    let got = owner.poll().unwrap();
    assert!(of_kind(&got, Kind::Records) > 0, "{got:?}");
    // A restart on the same folder uses the saved settings.
    let _ = rig.call(2, handle, &[]);
    let handle = rig.start_with(&config_for(&rig, Some(hint_for(server.local_addr())), None));
    let (link, notes) = step(&rig, handle, T0 + 20, false);
    assert_eq!(link.settings_version, 2, "{notes:?}");
}

#[test]
fn cr_164_settings_that_change_tor_say_a_restart_is_needed_and_the_start_keeps_the_proxy_rule() {
    let (server, mut owner, rig, handle, _dir) = settled();
    let mut tor = signed_settings().settings;
    tor.tor_on = true;
    owner
        .send(&id(3).device_id(), Kind::Settings, &owner_alone(tor, 2))
        .unwrap();
    let (link, notes) = step(&rig, handle, T0 + 5, false);
    assert_eq!(link.settings_version, 2, "{notes:?}");
    assert!(link.restart_needed);
    let line = link.settings_line.expect("a line for Home");
    assert_eq!(line.code.as_deref(), Some("CR-164"));
    super::screen::check_wire_words(&line).expect("words");
    // The next start has Tor on, so it cannot start without the proxy
    // (CR-87), whatever the pairing record says.
    let _ = rig.call(2, handle, &[]);
    let response = rig.call(
        1,
        0,
        &W::new()
            .start(&config_for(&rig, Some(hint_for(server.local_addr())), None))
            .0,
    );
    let f = Rd::error(&response).expect("a refusal");
    assert_eq!(f.rule, "CR-87", "{f:?}");
}

#[test]
fn cr_163_the_app_reads_tor_from_the_settings_in_force_before_the_start() {
    let (server, mut owner, rig, handle, _dir) = settled();
    let dir = rig.dir().to_str().unwrap().to_string();
    let config = config_for(&rig, Some(hint_for(server.local_addr())), None);
    let read = |dir: &str| {
        let response = rig.call(43, 0, &W::new().bytes(&config.record).string(dir).0);
        let mut r = Rd::ok(&response);
        let _network = r.u8();
        (r.u32(), r.bool())
    };
    // The record has Tor off and no file has been kept yet.
    assert_eq!(read(&dir), (1, false));
    let mut tor = signed_settings().settings;
    tor.tor_on = true;
    owner
        .send(&id(3).device_id(), Kind::Settings, &owner_alone(tor, 2))
        .unwrap();
    step(&rig, handle, T0 + 5, false);
    // The file says Tor is on now; the record alone still says off.
    assert_eq!(read(&dir), (2, true));
    assert_eq!(read(""), (1, false));
}

#[test]
fn cr_165_settings_that_remove_the_counter_say_so_on_home() {
    let (_server, mut owner, rig, handle, _dir) = settled();
    let mut gone = signed_settings().settings;
    gone.counters.clear();
    gone.devices.remove(&id(3).device_id());
    owner
        .send(&id(3).device_id(), Kind::Settings, &both_roots(gone, 2))
        .unwrap();
    let (link, notes) = step(&rig, handle, T0 + 5, false);
    assert_eq!(link.settings_version, 2, "{notes:?}");
    let line = link.settings_line.expect("a line for Home");
    assert_eq!(line.code.as_deref(), Some("CR-165"));
    super::screen::check_wire_words(&line).expect("words");
}

#[test]
fn cr_163_a_settings_file_that_does_not_read_stops_the_start_with_words() {
    let rig = Rig::new();
    std::fs::write(rig.dir().join("settings.bin"), b"not settings").unwrap();
    let response = rig.call(1, 0, &W::new().start(&config_for(&rig, None, None)).0);
    let f = Rd::error(&response).expect("a refusal");
    assert_eq!(f.rule, "CR-163", "{f:?}");
    super::screen::check_wire_words(&f.words).expect("words");
}
