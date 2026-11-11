//! A counter that talks to its owner through a Box, started the way the
//! counter app starts it (`scripts/link-e2e-core.sh` runs it).
//!
//! ```text
//! counter_box_link <data dir> <box addr> <box key hex> <peer addr>
//! ```
//!
//! The Key app, the owner phone and the counter's pairing are played with the
//! core functions, as `box_pair` plays them for the Box: the devices are
//! `owner`, `approver` and `c1`, whose keys are made from their names (for
//! tests only), so that the Box that `box_pair` paired knows the owner and
//! counter 1. The counter is paired by the whole ceremony, from a bundle that
//! carries the Box as its relay (CR-130), and its record is started through
//! `dispatch` with `start`, the call the app makes (CR-134). From then on
//! the example does what the app's link thread does: `link_step` every
//! second (CR-135). It also plays the owner, with a courier on the Box:
//! it sends a heartbeat to the counter and reads what the counter sends back.
//!
//! Then it plays the owner's settings (CR-160 to CR-166, LK-64): a tightening
//! signed by the owner alone and a loosening co-signed by the approver both
//! go to the counter through the Box; the counter applies them and the owner
//! reads the `Setting` records among the records it is sent. A last settings
//! removes the counter from the shop: the counter says so on Home and refuses
//! to open a sale.
//!
//! It prints one line for each thing it checked and exits 0 when all of it
//! held; otherwise it prints why not and exits 1.

use std::collections::BTreeSet;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use bitcoin::blockdata::constants::genesis_block;
use bitcoin::hashes::Hash;
use bitcoin::Network;
use openbmt_core::batch::PinnedRoots;
use openbmt_core::bundle::CounterBundle;
use openbmt_core::heartbeat::WatcherHeartbeat;
use openbmt_core::identity::{tagged_hash, DeviceId, Identity};
use openbmt_core::pairing::{check_request, owner_cosign, PairOffer, PairRequest};
use openbmt_core::records::RecordKind;
use openbmt_core::relay::{RelayHint, RelayRoute};
use openbmt_core::settings::{Limit, SettingNote, Settings, SignedSettings};
use openbmt_counter_ffi::{dispatch, Registry};
use openbmt_counter_node::{batch_policy, Ceremony, PairingRecord};
use openbmt_courier::{decode_records, Courier, CourierConfig, FirstStart, RelayPost};
use openbmt_link::{ClientConfig, Kind, Role, Route};

const WALLET: [u8; 32] = [9; 32];
const OP_START: u16 = 1;
const OP_STOP: u16 = 2;
const OP_OPEN: u16 = 11;
const OP_LINK_STEP: u16 = 24;
const OP_LINK_STATE: u16 = 25;

fn identity(name: &str) -> Identity {
    let secret = tagged_hash("OpenBMT/example/box-pair/v1", name.as_bytes());
    Identity::from_secret_bytes(&secret).expect("a key")
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

fn hex_key(text: &str) -> Result<DeviceId, String> {
    if text.len() != 64 || !text.is_ascii() {
        return Err(format!("{text} is not a key"));
    }
    let mut bytes = [0u8; 32];
    for (i, b) in bytes.iter_mut().enumerate() {
        *b = u8::from_str_radix(&text[2 * i..2 * i + 2], 16)
            .map_err(|_| format!("{text} is not a key"))?;
    }
    DeviceId::from_bytes(&bytes).map_err(|e| e.to_string())
}

/// The settings the counter is paired with.
fn base_settings(counter: DeviceId) -> Settings {
    let mut settings = Settings::defaults(1_000);
    settings.version = 1;
    settings.customer_wifi = false;
    settings.min_sale_sats = 0;
    settings.devices.insert(counter);
    settings.counters.insert(counter, 10_000);
    settings
}

/// The counter's pairing, by the whole ceremony, from a bundle that holds
/// the Box as its relay.
fn pair(relay: RelayHint, peer: &str) -> Result<(PairingRecord, Vec<u8>), String> {
    let (owner, approver, counter) = (identity("owner"), identity("approver"), identity("c1"));
    let offer = PairOffer::new(
        owner.device_id(),
        approver.device_id(),
        WALLET,
        Network::Regtest,
    )
    .map_err(|e| e.to_string())?;
    let secret = tagged_hash("OpenBMT/example/box-pair/v1", b"c1");
    let (mut ceremony, started) =
        Ceremony::begin(&offer.to_bytes(), "Front counter", secret, [7; 32], now())
            .map_err(|e| e.to_string())?;
    let request = PairRequest::from_bytes(&started.request).map_err(|e| e.to_string())?;
    let pending = check_request(&offer, &request).map_err(|e| e.to_string())?;
    let (entry, sig) = pending.approve(&approver, 0).map_err(|e| e.to_string())?;
    let pairing = owner_cosign(&offer, entry, sig, &owner).map_err(|e| e.to_string())?;
    let settings = base_settings(counter.device_id());
    let signed = SignedSettings {
        owner_sig: settings.sign(&owner),
        approver_sig: Some(settings.sign(&approver)),
        settings,
    };
    let bundle = CounterBundle::new(pairing, signed, None, vec![peer.to_string()])
        .map_err(|e| e.to_string())?
        .with_relay(Some(relay));
    let bytes = bundle.to_bytes();
    ceremony.confirm(now()).map_err(|e| e.to_string())?;
    let record = ceremony
        .finish(&bytes, &batch_policy(), now())
        .map_err(|e| e.to_string())?;
    let roots =
        PinnedRoots::new(owner.device_id(), approver.device_id()).map_err(|e| e.to_string())?;
    if record.roots != roots {
        return Err("the record pinned other roots".to_string());
    }
    println!("bundle: version {}, {} bytes", bytes[4], bytes.len());
    Ok((record, bytes))
}

// What the app's calls look like on the wire.

fn request(op: u16, handle: u64, fields: &[u8]) -> Vec<u8> {
    let mut out = b"OBMm".to_vec();
    out.push(1);
    out.extend_from_slice(&op.to_le_bytes());
    out.extend_from_slice(&handle.to_le_bytes());
    out.extend_from_slice(fields);
    out
}

fn put_bytes(out: &mut Vec<u8>, bytes: &[u8]) {
    out.extend_from_slice(&u32::try_from(bytes.len()).expect("fits").to_le_bytes());
    out.extend_from_slice(bytes);
}

/// The payload of an ok response, or why not.
fn payload(response: &[u8]) -> Result<&[u8], String> {
    if response.get(..4) != Some(b"OBMn".as_slice()) || response.get(4) != Some(&1) {
        return Err("not a response".to_string());
    }
    match response.get(5) {
        Some(0) => Ok(&response[6..]),
        status => Err(format!("status {status:?}")),
    }
}

/// What `link_state` and `link_step` say, the part this example checks.
#[derive(Debug)]
struct Link {
    heading: String,
    connected: bool,
    heartbeats_taken: u64,
    settings_version: u32,
    restart_needed: bool,
    /// The heading of the settings line of Home, when there is one.
    settings_line: Option<String>,
}

struct Cursor<'a>(&'a [u8]);

impl<'a> Cursor<'a> {
    fn take(&mut self, n: usize) -> &'a [u8] {
        let (head, rest) = self.0.split_at(n);
        self.0 = rest;
        head
    }

    fn u8(&mut self) -> u8 {
        self.take(1)[0]
    }

    fn u32(&mut self) -> u32 {
        u32::from_le_bytes(self.take(4).try_into().expect("4"))
    }

    fn u64(&mut self) -> u64 {
        u64::from_le_bytes(self.take(8).try_into().expect("8"))
    }

    fn str16(&mut self) -> String {
        let n = usize::from(u16::from_le_bytes(self.take(2).try_into().expect("2")));
        String::from_utf8_lossy(self.take(n)).into_owned()
    }

    fn opt(&mut self, f: impl FnOnce(&mut Self)) {
        if self.u8() == 1 {
            f(self);
        }
    }

    /// A words value; its heading.
    fn words(&mut self) -> String {
        let _ = (self.u8(), self.u8());
        let heading = self.str16();
        let _ = (self.str16(), self.str16());
        self.opt(|c| drop(c.str16()));
        self.opt(|c| {
            let _ = c.u64();
        });
        self.opt(|c| drop((c.str16(), c.str16())));
        self.opt(|c| drop(c.str16()));
        heading
    }

    fn link(&mut self) -> Link {
        let heading = self.words();
        let connected = self.u8() == 1;
        self.opt(|c| {
            let _ = c.u64();
        });
        let _refused = self.u8();
        let heartbeats_taken = self.u64();
        let _owner_records = self.u64();
        self.opt(|c| {
            let _ = c.u64();
        });
        let settings_version = self.u32();
        let restart_needed = self.u8() == 1;
        let settings_line = (self.u8() == 1).then(|| self.words());
        Link {
            heading,
            connected,
            heartbeats_taken,
            settings_version,
            restart_needed,
            settings_line,
        }
    }
}

fn link_step(registry: &Registry, handle: u64) -> Result<Link, String> {
    let mut fields = now().to_le_bytes().to_vec();
    fields.push(0);
    let response = dispatch(registry, &request(OP_LINK_STEP, handle, &fields));
    Ok(Cursor(payload(&response)?).link())
}

fn link_state(registry: &Registry, handle: u64) -> Result<Option<Link>, String> {
    let response = dispatch(registry, &request(OP_LINK_STATE, handle, &[]));
    let mut c = Cursor(payload(&response)?);
    Ok((c.u8() == 1).then(|| c.link()))
}

fn run(dir: PathBuf, box_addr: SocketAddr, box_key: DeviceId, peer: &str) -> Result<(), String> {
    let relay = RelayHint::new(RelayRoute::Direct(box_addr), box_key.to_bytes())
        .map_err(|e| e.to_string())?;
    let (record, _) = pair(relay.clone(), peer)?;
    if record.relay != Some(relay) {
        return Err("the record does not hold the bundle's relay".to_string());
    }
    println!("paired: the record holds the relay {box_addr}");

    // The counter, started as the app starts it.
    let counter_dir = dir.join("counter");
    std::fs::create_dir_all(&counter_dir).map_err(|e| e.to_string())?;
    let registry = Registry::new();
    let mut fields = Vec::new();
    put_bytes(
        &mut fields,
        counter_dir
            .to_str()
            .ok_or("the folder is not UTF-8")?
            .as_bytes(),
    );
    fields.extend_from_slice(&now().to_le_bytes());
    put_bytes(&mut fields, &record.to_bytes());
    fields.extend_from_slice(&[0, 0, 0, 0]);
    let response = dispatch(&registry, &request(OP_START, 0, &fields));
    let handle = u64::from_le_bytes(
        payload(&response)?
            .get(..8)
            .ok_or("no handle")?
            .try_into()
            .expect("8"),
    );
    println!("started: handle {handle}");
    let before = link_state(&registry, handle)?.ok_or("the link has no line")?;
    println!("link before the first turn: {:?}", before.heading);

    // The owner, with a courier on the Box.
    let owner_dir = dir.join("owner");
    std::fs::create_dir_all(&owner_dir).map_err(|e| e.to_string())?;
    let owner = identity("owner");
    let counter = identity("c1").device_id();
    let mut courier = Courier::start(
        CourierConfig {
            dir: owner_dir,
            identity: identity("owner"),
            role: Role::Owner,
            peers: [(counter, Role::Counter)].into_iter().collect(),
            route: Route::Direct(box_addr),
            relay: box_key,
            client: ClientConfig::default(),
        },
        FirstStart::Yes,
    )
    .map_err(|e| e.to_string())?;
    courier
        .poll()
        .map_err(|e| format!("the owner's mailbox: {e}"))?;

    let tip = genesis_block(Network::Regtest).block_hash().to_byte_array();
    let mut sent = false;
    let mut seq = 0;
    let mut seen = Seen::default();
    let mut link = before;
    let end = Instant::now() + Duration::from_secs(60);
    while Instant::now() < end {
        link = link_step(&registry, handle)?;
        // The counter's mailbox exists once its first turn has polled the
        // Box; the heartbeat can be sent from then on.
        if link.connected && !sent {
            seq += 1;
            let heartbeat = WatcherHeartbeat {
                counter,
                seq,
                time: now(),
                tip_height: 0,
                tip_hash: tip,
                healthy: true,
            }
            .sign(&owner)
            .to_bytes();
            if courier.send(&counter, Kind::Heartbeat, &heartbeat).is_ok() {
                sent = true;
                println!("owner: heartbeat {seq} sent through the Box");
            }
        }
        seen.take(&mut courier);
        if sent && link.heartbeats_taken >= 1 && seen.records > 0 && seen.check_ins > 0 {
            break;
        }
        std::thread::sleep(Duration::from_secs(1));
    }
    println!(
        "link: {:?}, connected {}, heartbeats taken by the counter {}",
        link.heading, link.connected, link.heartbeats_taken
    );
    println!(
        "owner: {} messages of records, {} check-ins received through the Box",
        seen.records, seen.check_ins
    );
    if !(link.heartbeats_taken >= 1 && seen.records > 0 && seen.check_ins > 0) {
        let _ = dispatch(&registry, &request(OP_STOP, handle, &[]));
        return Err("the exchange through the Box did not complete".to_string());
    }
    println!("counter link: ok");

    let result = settings_phase(&registry, handle, &mut courier, &mut seen, &counter);
    let _ = dispatch(&registry, &request(OP_STOP, handle, &[]));
    result
}

/// What the owner has been sent through the Box.
#[derive(Default)]
struct Seen {
    records: u32,
    check_ins: u32,
    /// The versions that `Setting` records among the records name.
    setting_versions: BTreeSet<u32>,
}

impl Seen {
    fn take(&mut self, courier: &mut Courier<RelayPost>) {
        let Ok(got) = courier.poll() else { return };
        for m in got {
            match m.kind {
                Kind::Records => {
                    self.records += 1;
                    for r in decode_records(&m.content).unwrap_or_default() {
                        if r.kind == RecordKind::Setting {
                            if let Ok(note) = SettingNote::from_body(&r.body) {
                                self.setting_versions.insert(note.version);
                            }
                        }
                    }
                }
                Kind::CheckIn => self.check_ins += 1,
                _ => {}
            }
        }
    }
}

fn signed_alone(settings: Settings, owner: &Identity) -> SignedSettings {
    SignedSettings {
        owner_sig: settings.sign(owner),
        approver_sig: None,
        settings,
    }
}

fn signed_by_both(settings: Settings, owner: &Identity, approver: &Identity) -> SignedSettings {
    SignedSettings {
        owner_sig: settings.sign(owner),
        approver_sig: Some(settings.sign(approver)),
        settings,
    }
}

/// Send `signed` to the counter through the Box, and step the link until
/// the counter has applied it and the owner holds its record.
fn deliver_settings(
    registry: &Registry,
    handle: u64,
    courier: &mut Courier<RelayPost>,
    seen: &mut Seen,
    counter: &DeviceId,
    signed: &SignedSettings,
) -> Result<Link, String> {
    let version = signed.settings.version;
    courier
        .send(counter, Kind::Settings, &signed.to_bytes())
        .map_err(|e| format!("settings version {version} were not sent: {e}"))?;
    println!("owner: settings version {version} sent through the Box");
    let end = Instant::now() + Duration::from_secs(30);
    while Instant::now() < end {
        let link = link_step(registry, handle)?;
        seen.take(courier);
        if link.settings_version >= version && seen.setting_versions.contains(&version) {
            println!(
                "counter: settings version {version} applied; the owner holds its Setting record"
            );
            return Ok(link);
        }
        std::thread::sleep(Duration::from_secs(1));
    }
    Err(format!(
        "settings version {version} did not reach the counter and come back as a record"
    ))
}

fn settings_phase(
    registry: &Registry,
    handle: u64,
    courier: &mut Courier<RelayPost>,
    seen: &mut Seen,
    counter: &DeviceId,
) -> Result<(), String> {
    let (owner, approver) = (identity("owner"), identity("approver"));
    // The counter records the settings it was paired with at its first start
    // (CR-162), and the owner is sent that record.
    let end = Instant::now() + Duration::from_secs(20);
    while Instant::now() < end && !seen.setting_versions.contains(&1) {
        link_step(registry, handle)?;
        seen.take(courier);
        std::thread::sleep(Duration::from_secs(1));
    }
    if !seen.setting_versions.contains(&1) {
        return Err("the owner was not sent the Setting record of the paired settings".to_string());
    }
    // A tightening, signed by the owner alone.
    let mut tighter = base_settings(*counter);
    tighter.version = 2;
    tighter.refund_cap = Limit::from_usd(50, 1_000);
    deliver_settings(
        registry,
        handle,
        courier,
        seen,
        counter,
        &signed_alone(tighter.clone(), &owner),
    )?;
    // A loosening needs the approver's signature as well.
    let mut looser = tighter;
    looser.version = 3;
    looser.refund_cap = Limit::from_usd(150, 1_000);
    let link = deliver_settings(
        registry,
        handle,
        courier,
        seen,
        counter,
        &signed_by_both(looser.clone(), &owner, &approver),
    )?;
    if link.restart_needed || link.settings_line.is_some() {
        return Err(format!("home has a settings line it should not: {link:?}"));
    }
    println!(
        "owner: Setting records for versions {:?} received through the Box",
        seen.setting_versions
    );
    println!("counter settings: ok");

    // Settings that remove the counter.
    let mut gone = looser;
    gone.version = 4;
    gone.devices.remove(counter);
    gone.counters.clear();
    let link = deliver_settings(
        registry,
        handle,
        courier,
        seen,
        counter,
        &signed_by_both(gone, &owner, &approver),
    )?;
    match &link.settings_line {
        Some(heading) => println!("counter: Home says {heading:?}"),
        None => return Err("a removed counter has no line on Home".to_string()),
    }
    let mut fields = now().to_le_bytes().to_vec();
    fields.extend_from_slice(&1_000u64.to_le_bytes());
    put_bytes(&mut fields, b"USD");
    fields.extend_from_slice(&10_000_000u64.to_le_bytes());
    put_bytes(&mut fields, b"test");
    fields.extend_from_slice(&now().to_le_bytes());
    let response = dispatch(registry, &request(OP_OPEN, handle, &fields));
    if payload(&response).is_ok() || !response.windows(6).any(|w| w == b"CR-165") {
        return Err(format!(
            "a removed counter did not refuse a new sale with CR-165: {response:?}"
        ));
    }
    println!("counter: a new sale is refused, the counter was removed from the shop (CR-165)");
    println!("counter removal: ok");
    Ok(())
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [dir, addr, key, peer] = args.as_slice() else {
        eprintln!("usage: counter_box_link <data dir> <box addr> <box key hex> <peer addr>");
        return ExitCode::from(2);
    };
    let parsed = addr
        .parse::<SocketAddr>()
        .map_err(|_| format!("{addr} is not an address"))
        .and_then(|a| hex_key(key).map(|k| (a, k)));
    let result = parsed.and_then(|(a, k)| run(PathBuf::from(dir), a, k, peer));
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            println!("counter link: FAILED: {e}");
            ExitCode::from(1)
        }
    }
}
