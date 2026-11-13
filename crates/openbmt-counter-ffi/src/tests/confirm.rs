//! Confirming a change to the shop's settings at a counter, through
//! `dispatch` (`docs/COUNTER-RULES.md` CR-239 to CR-244): a counter paired
//! with the shop's own wallet is paired, started and linked to a relay; the
//! owner phone, which the test plays with the functions of `openbmt_core`
//! and a courier, sends a Loosen change; the counter keeps it; its manager
//! confirms it (the PIN is the app's: the call is made after it); the owner
//! phone reads the record from the counter's chain, builds `OBM<` and sends
//! it; and the counter applies it.

use std::net::{SocketAddr, TcpListener};
use std::sync::Arc;

use openbmt_core::approval::{confirm_words, CounterConfirmation};
use openbmt_core::confirmed::ConfirmedSettings;
use openbmt_core::identity::{DeviceId, Identity};
use openbmt_core::pairing_own::{OwnBundle, OwnOffer, OwnRequest};
use openbmt_core::pinned_wallet::{IndexRange, PinnedWallet, SignedRange};
use openbmt_core::records::RecordKind;
use openbmt_core::relay::{RelayHint, RelayRoute};
use openbmt_core::settings::{Limit, Settings, SignedSettings};
use openbmt_counter::ConfirmHeld;
use openbmt_counter_node::OwnPairingRecord;
use openbmt_courier::{decode_records, Courier, CourierConfig, FirstStart, RelayPost};
use openbmt_link::{
    ClientConfig, Kind, MemoryStore, Relay, RelayConfig, Role, Route, Server, ServerConfig,
};
use openbmt_net_io::SystemClock;

use super::kit::{peers_event, Rig, Scratch, T0};
use super::wire::{Rd, W};
use crate::StartConfig;

pub(super) const SINGLE: &str = "wpkh([818bc9b6/84h/0h/0h]xpub6CgURS1GrKZkAoRGBTKYX9FhLgkKnQVZ6vdNBUbiu24xtRJ6RzLKpdg4CqsVyBLzimcJsviP59hSoGJEpTJiXE8bdBLw1qPC1DXRatKehb8/<0;1>/*)#96yh9yhx";
pub(super) const FLOOR: u32 = 4;
pub(super) const OP_PUMP: u16 = 10;
const OP_LINK_STEP: u16 = 24;
const OP_PENDING_CHANGE: u16 = 71;
const OP_CONFIRM_CHANGE: u16 = 72;

pub(super) fn owner() -> Identity {
    Identity::from_secret_bytes(&[0x11; 32]).unwrap()
}

pub(super) fn relay_key() -> Identity {
    Identity::from_secret_bytes(&[0x63; 32]).unwrap()
}

pub(super) fn base(counter: &DeviceId, version: u32) -> Settings {
    let mut s = Settings::defaults(1_000);
    s.version = version;
    s.devices.insert(*counter);
    s.counters.insert(*counter, 10_000);
    s
}

pub(super) fn signed(s: Settings) -> SignedSettings {
    SignedSettings {
        owner_sig: s.sign(&owner()),
        approver_sig: None,
        settings: s,
    }
}

/// The pairing record of a counter paired with the shop's own wallet whose
/// relay is `addr`, and the counter's id.
fn paired(rig: &Rig, addr: SocketAddr, members: &Relay<MemoryStore>) -> (Vec<u8>, DeviceId) {
    paired_with(rig, addr, members, false)
}

/// [`paired`], with the offer marked as one for a starter wallet when
/// `starter` is true (CR-256).
pub(super) fn paired_with(
    rig: &Rig,
    addr: SocketAddr,
    members: &Relay<MemoryStore>,
    starter: bool,
) -> (Vec<u8>, DeviceId) {
    let _ = members;
    let offer = OwnOffer::new(
        bitcoin::Network::Bitcoin,
        owner().device_id(),
        FLOOR,
        PinnedWallet::from_text(SINGLE).unwrap(),
    )
    .unwrap()
    .with_starter(starter);
    let response = rig.call(
        44,
        0,
        &W::new()
            .bytes(&offer.to_bytes())
            .string("Front counter")
            .u64(T0)
            .0,
    );
    Rd::ok(&response);
    let response = rig.call(69, 0, &W::new().u64(T0 + 10).string("Front counter").0);
    let request = Rd::ok(&response).bytes();
    let request = OwnRequest::from_bytes(&request).unwrap();
    let counter = request.counter;
    let bundle = OwnBundle::build(
        &owner(),
        &offer,
        &request,
        signed(base(&counter, FLOOR + 1)),
        SignedRange::sign(&owner(), IndexRange::new(counter, 0, 1_000).unwrap()).unwrap(),
        vec!["127.0.0.1:8333".to_string()],
        None,
    )
    .unwrap()
    .to_bytes();
    Rd::ok(&rig.call(45, 0, &W::new().u64(T0 + 20).0));
    let response = rig.call(48, 0, &W::new().u64(T0 + 30).bytes(&bundle).0);
    let record = Rd::ok(&response).bytes();
    let mut record = OwnPairingRecord::from_bytes(&record).unwrap();
    record.relay =
        Some(RelayHint::new(RelayRoute::Direct(addr), relay_key().device_id().to_bytes()).unwrap());
    (record.to_bytes(), counter)
}

pub(super) struct World {
    pub(super) server: Server,
    pub(super) owner: Courier<RelayPost>,
    _owner_dir: Scratch,
}

pub(super) fn config(rig: &Rig, record: &[u8]) -> StartConfig {
    StartConfig {
        dir: rig.dir().to_str().unwrap().to_string(),
        now: T0 + 100,
        record: record.to_vec(),
        proxy: None,
        own_node: None,
        lightning: None,
        link_proxy: None,
    }
}

pub(super) fn step(rig: &Rig, handle: u64, now: u64) -> (u32, Vec<String>) {
    let response = rig.call(OP_LINK_STEP, handle, &W::new().u64(now).u8(0).0);
    let mut r = Rd::ok(&response);
    let link = r.link();
    let notes = r.list(Rd::string);
    r.done();
    (link.settings_version, notes)
}

/// One field of the change: its name, and the kind and number of its value
/// now and after.
type Field = (String, (u8, u64, String), (u8, u64, String));

#[derive(Debug, PartialEq)]
struct Pending {
    version: u32,
    words: Vec<String>,
    fields: Vec<Field>,
    arrived: u64,
    expires_at: u64,
    confirmed: bool,
}

fn value(r: &mut Rd<'_>) -> (u8, u64, String) {
    (r.u8(), r.u64(), r.string())
}

/// The change the app reads: the change that waits, if one does, and the
/// notice's heading, if there is one.
fn pending(rig: &Rig, handle: u64, now: u64) -> (Option<Pending>, Option<String>) {
    let response = rig.call(OP_PENDING_CHANGE, handle, &W::new().u64(now).0);
    let mut r = Rd::ok(&response);
    let p = r.opt(|r| Pending {
        version: r.u32(),
        words: r.list(Rd::string),
        fields: r.list(|r| (r.string(), value(r), value(r))),
        arrived: r.u64(),
        expires_at: r.u64(),
        confirmed: r.bool(),
    });
    let notice = r.opt(|r| r.words().heading);
    r.done();
    (p, notice)
}

pub(super) fn world(addr_server: Option<(Server, Courier<RelayPost>, Scratch)>) -> World {
    let (server, owner, dir) = addr_server.unwrap();
    World {
        server,
        owner,
        _owner_dir: dir,
    }
}

/// A relay whose members are the owner and `counter`, and the owner's
/// courier past its first poll.
pub(super) fn relay_for(counter: &DeviceId) -> (Server, Courier<RelayPost>, Scratch) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let mut relay = Relay::new(relay_key(), RelayConfig::default(), MemoryStore::new()).unwrap();
    relay
        .set_members(&[owner().device_id(), *counter], 0)
        .unwrap();
    let server = Server::start(
        listener,
        relay,
        Arc::new(SystemClock),
        ServerConfig::default(),
    )
    .unwrap();
    let dir = Scratch::new();
    let mut courier = Courier::start(
        CourierConfig {
            dir: dir.path().to_path_buf(),
            identity: owner(),
            role: Role::Owner,
            peers: [(*counter, Role::Counter)].into_iter().collect(),
            route: Route::Direct(server.local_addr()),
            relay: relay_key().device_id(),
            client: ClientConfig::default(),
        },
        FirstStart::Yes,
    )
    .unwrap();
    courier.poll().unwrap();
    (server, courier, dir)
}

/// The counter's own records the owner holds so far, read as the owner phone
/// reads them.
pub(super) fn owner_records(world: &mut World) -> Vec<openbmt_core::records::Record> {
    let mut all = Vec::new();
    for message in world.owner.poll().unwrap() {
        if message.kind == Kind::Records {
            all.extend(decode_records(&message.content).unwrap());
        }
    }
    all
}

#[test]
fn cr_239_to_cr_242_a_loosening_is_confirmed_at_the_counter_and_applied_with_the_obm_frame() {
    let rig = Rig::new();
    // The relay's members are known only after the ceremony names the
    // counter; the ceremony needs no relay, so it runs first on a relay-less
    // address that the record is given afterwards.
    let placeholder: SocketAddr = "127.0.0.1:9".parse().unwrap();
    let members = Relay::new(relay_key(), RelayConfig::default(), MemoryStore::new()).unwrap();
    let (record, counter) = paired(&rig, placeholder, &members);
    let (server, courier, dir) = relay_for(&counter);
    let mut record = OwnPairingRecord::from_bytes(&record).unwrap();
    record.relay = Some(
        RelayHint::new(
            RelayRoute::Direct(server.local_addr()),
            relay_key().device_id().to_bytes(),
        )
        .unwrap(),
    );
    let record = record.to_bytes();
    let mut w = world(Some((server, courier, dir)));
    let handle = rig.start_with(&config(&rig, &record));
    rig.send(vec![peers_event()]);
    Rd::ok(&rig.call(OP_PUMP, handle, &W::new().u64(T0 + 100).u32(50).0));
    assert_eq!(step(&rig, handle, T0 + 100).0, FLOOR + 1);
    let _ = w.server.local_addr();

    // Nothing waits.
    assert_eq!(pending(&rig, handle, T0 + 101), (None, None));

    // The owner phone sends a Loosen change: a limit raised.
    let mut loose = base(&counter, FLOOR + 2);
    let before = loose.light_per_sale.display_usd;
    loose.light_per_sale = Limit::from_usd(before + 3, 1_000);
    let change = signed(loose);
    w.owner
        .send(&counter, Kind::Settings, &change.to_bytes())
        .unwrap();
    let (version, notes) = step(&rig, handle, T0 + 110);
    // It is kept, not applied (CR-239).
    assert_eq!(version, FLOOR + 1, "{notes:?}");
    assert!(rig.dir().join("confirm.bin").exists());
    assert!(!rig.dir().join("settings.bin").exists());
    let (waiting, notice) = pending(&rig, handle, T0 + 111);
    let waiting = waiting.expect("a change waits");
    assert_eq!(notice, None);
    assert_eq!(waiting.version, FLOOR + 2);
    assert_eq!(waiting.words.len(), 4);
    assert_eq!(
        waiting.words,
        confirm_words(&change.settings.hash())
            .iter()
            .map(|w| w.to_string())
            .collect::<Vec<_>>()
    );
    assert_eq!(waiting.arrived, T0 + 110);
    assert_eq!(waiting.expires_at, T0 + 110 + 7 * 86_400);
    assert!(!waiting.confirmed);
    // Its one loosened field, with the values now and after (Units = 0).
    assert_eq!(
        waiting.fields,
        vec![(
            "light_per_sale".to_string(),
            (0, u64::from(before), String::new()),
            (0, u64::from(before + 3), String::new())
        )]
    );

    // The manager confirms (the app has checked the PIN): the words of a
    // confirmed change; a version that is not the one shown signs nothing.
    let stale = rig.call(
        OP_CONFIRM_CHANGE,
        handle,
        &W::new().u64(T0 + 120).u32(FLOOR + 3).0,
    );
    assert_eq!(Rd::error(&stale).unwrap().rule, "CR-241");
    let response = rig.call(
        OP_CONFIRM_CHANGE,
        handle,
        &W::new().u64(T0 + 120).u32(FLOOR + 2).0,
    );
    let mut r = Rd::ok(&response);
    let words = r.words();
    r.done();
    assert_eq!(words.heading, "Change confirmed");
    assert!(pending(&rig, handle, T0 + 121).0.unwrap().confirmed);
    // It is not applied on this counter by confirming it (CR-241).
    assert!(!rig.dir().join("settings.bin").exists());

    // The owner phone reads the record from the counter's records.
    step(&rig, handle, T0 + 130);
    let records = owner_records(&mut w);
    let held = records
        .iter()
        .filter(|r| r.kind == RecordKind::Setting)
        .find_map(|r| ConfirmHeld::decode(&r.body).ok())
        .expect("a confirmation record among the records the owner holds");
    assert_eq!(held.settings_hash, change.settings.hash());
    let confirmation = held.confirmation(counter);
    // It builds `OBM<` and sends it to every counter.
    let confirmed = ConfirmedSettings::new(change.clone(), confirmation);
    assert_eq!(confirmed, {
        let c = CounterConfirmation::sign(
            &Identity::from_secret_bytes(&secret_of(&record)).unwrap(),
            &change.settings.hash(),
        );
        ConfirmedSettings::new(change.clone(), c)
    });
    w.owner
        .send(&counter, Kind::Settings, &confirmed.to_bytes())
        .unwrap();
    let (version, notes) = step(&rig, handle, T0 + 140);
    assert_eq!(version, FLOOR + 2, "{notes:?}");
    // Applied: nothing waits, the file holds the frame whole.
    assert_eq!(pending(&rig, handle, T0 + 141), (None, None));
    assert!(!rig.dir().join("confirm.bin").exists());
    let kept = std::fs::read(rig.dir().join("settings.bin")).unwrap();
    assert_eq!(kept, confirmed.to_bytes());
    assert_eq!(&kept[..4], b"OBM<");

    // CR-163: a start on the same folder reads the frame.
    let _ = rig.call(2, handle, &[]);
    let handle = rig.start_with(&config(&rig, &record));
    assert_eq!(step(&rig, handle, T0 + 200).0, FLOOR + 2);
}

/// The counter's identity secret of a record, for the test's cross-check.
fn secret_of(record: &[u8]) -> [u8; 32] {
    OwnPairingRecord::from_bytes(record)
        .unwrap()
        .identity_secret
}

#[test]
fn cr_242_a_confirmation_of_a_counter_not_in_force_is_refused_and_noted_with_words() {
    let rig = Rig::new();
    let placeholder: SocketAddr = "127.0.0.1:9".parse().unwrap();
    let members = Relay::new(relay_key(), RelayConfig::default(), MemoryStore::new()).unwrap();
    let (record, counter) = paired(&rig, placeholder, &members);
    let (server, courier, dir) = relay_for(&counter);
    let mut record = OwnPairingRecord::from_bytes(&record).unwrap();
    record.relay = Some(
        RelayHint::new(
            RelayRoute::Direct(server.local_addr()),
            relay_key().device_id().to_bytes(),
        )
        .unwrap(),
    );
    let record = record.to_bytes();
    let mut w = world(Some((server, courier, dir)));
    let handle = rig.start_with(&config(&rig, &record));
    rig.send(vec![peers_event()]);
    step(&rig, handle, T0 + 100);

    let mut loose = base(&counter, FLOOR + 2);
    loose.light_per_sale = Limit::from_usd(loose.light_per_sale.display_usd + 3, 1_000);
    let change = signed(loose);
    // Counter 9 is no counter of the shop.
    let stranger = Identity::from_secret_bytes(&[9; 32]).unwrap();
    let bad = ConfirmedSettings::new(
        change.clone(),
        CounterConfirmation::sign(&stranger, &change.settings.hash()),
    );
    w.owner
        .send(&counter, Kind::Settings, &bad.to_bytes())
        .unwrap();
    let (version, notes) = step(&rig, handle, T0 + 110);
    assert_eq!(version, FLOOR + 1, "{notes:?}");
    assert!(!rig.dir().join("settings.bin").exists());
    let (waiting, notice) = pending(&rig, handle, T0 + 111);
    assert!(waiting.is_none());
    assert_eq!(notice.as_deref(), Some("Confirmation refused"));
}

#[test]
fn cr_239_a_change_dropped_after_seven_days_says_so_in_words() {
    let rig = Rig::new();
    let placeholder: SocketAddr = "127.0.0.1:9".parse().unwrap();
    let members = Relay::new(relay_key(), RelayConfig::default(), MemoryStore::new()).unwrap();
    let (record, counter) = paired(&rig, placeholder, &members);
    let (server, courier, dir) = relay_for(&counter);
    let mut record = OwnPairingRecord::from_bytes(&record).unwrap();
    record.relay = Some(
        RelayHint::new(
            RelayRoute::Direct(server.local_addr()),
            relay_key().device_id().to_bytes(),
        )
        .unwrap(),
    );
    let record = record.to_bytes();
    let mut w = world(Some((server, courier, dir)));
    let handle = rig.start_with(&config(&rig, &record));
    rig.send(vec![peers_event()]);
    step(&rig, handle, T0 + 100);
    let mut loose = base(&counter, FLOOR + 2);
    loose.tor_on = true;
    loose.min_sale_sats = 0;
    // A tightening-looking field next to a loosening one.
    loose.light_per_sale = Limit::from_usd(loose.light_per_sale.display_usd + 1, 1_000);
    w.owner
        .send(&counter, Kind::Settings, &signed(loose).to_bytes())
        .unwrap();
    step(&rig, handle, T0 + 110);
    assert!(pending(&rig, handle, T0 + 111).0.is_some());
    let later = T0 + 110 + 7 * 86_400;
    let (waiting, notice) = pending(&rig, handle, later);
    assert!(waiting.is_none());
    assert_eq!(notice.as_deref(), Some("Change not confirmed in time"));
    assert!(!rig.dir().join("confirm.bin").exists());
}
