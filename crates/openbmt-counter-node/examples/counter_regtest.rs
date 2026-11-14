//! The counter node against a Bitcoin node on regtest, as a command that
//! reads simple lines on its standard input and prints one line for each
//! thing it does.
//!
//! ```text
//! counter_regtest <data dir> <node address> [--regtest-single-peer]
//!     [--heartbeat-max-age <seconds>] [--customers-off-wifi]
//!     [--regtest-own-node] [--own-wallet <description>]
//!     [--relay <address> --relay-key <hex>] [--box-key <hex>] [--box-owner-fixed]
//!     [--shop-ln <node id> <provider id> <provider address> <cap sats> <max fee sats>]
//!     [--shop-ln-other]
//! counter_regtest --shop-ln-id <128 hex>
//! ```
//!
//! `scripts/counter-e2e-core.sh` uses this to check the whole receive path
//! against Bitcoin Core, `scripts/instant-e2e-core.sh` to check the owner's
//! heartbeat, and `scripts/link-e2e-core.sh` to check that the owner's
//! heartbeat, and the counter's check-ins and records, travel through a relay
//! (`openbmt-link`'s `relay` example). The node is the one fixed local peer of
//! the driver.
//!
//! `--relay <address> --relay-key <hex>` starts the link (`CounterLink`,
//! `docs/LINK-RULES.md` section 6) next to the node: the relay's address and
//! its key (the first line the relay example prints). The owner is the
//! example owner's fixed test key (`owner_regtest`). The link step runs in
//! the loop between commands; heartbeats that arrive through it are taken by
//! the node as the `heartbeat` command would, and what they report is
//! printed as usual. The two number files of the link (LK-53) are kept in the
//! data directory: the start is a first start if neither is there, and any
//! other start needs both.
//!
//! `--box-key <hex>` makes the relay a Box (BX-20): the Box is added to the
//! signed settings as a device and a Box, to the courier's table as a Box,
//! and the owner of the link is the owner `box_pair` paired the Box with;
//! the counter sends it paid notices (BX-40) and takes its POS notices
//! (CR-266). `scripts/till-e2e-core.sh` uses it for a sale from the till.
//! `--box-owner-fixed` keeps the fixed key of `owner_regtest` as the owner of
//! the link when there is a Box, for a Box that an own-wallet shop paired with
//! that key (`scripts/own-box-e2e-core.sh`).
//!
//! `--heartbeat-max-age` sets the signed setting `heartbeat_max_age_secs`
//! (60 to 3600; the default settings say 600), so that a check of an old
//! heartbeat need not wait ten minutes.
//!
//! `--customers-off-wifi` sets the signed setting `customer_wifi` to false:
//! the customers cannot join the counter's network, as on a counter on its
//! own cellular line. With the default (true) the light check needs Tor to
//! accept anything at once, and the example has no Tor.
//!
//! `--own-wallet <description>` pairs the counter with the shop's own wallet
//! instead of the Key app (CR-227 to CR-235), with an offer marked as a
//! starter wallet's (CR-256), so that the counter takes one switch to a new
//! wallet (CR-257 to CR-263). The example plays the owner phone with the
//! fixed test owner key, and the app with a file: the pairing record is made
//! on the first run and kept in `own-record.bin` in the data directory. The
//! commands `switch-take`, `switch-state`, `switch-same`, `switch-kept` and
//! `switch-different` below drive the switch.
//!
//! `--shop-ln ...` puts a `shop_lightning` block in the settings the
//! own-wallet record is made with (SL-70, SL-71): this counter is the
//! Lightning counter (with `--shop-ln-other`, a counter that is not this one), the Box
//! of `--box-key` is the second device, and the node ID and the provider are
//! the ones given. The counter's seed record is the file `shop-seed.bin` in
//! the data directory, which the app would keep sealed. The wallet of
//! `--own-wallet test` is a fixed single-key wallet. `--shop-ln-id` prints
//! the node ID of a seed and stops. Commands: `shop-seed <128 hex>` (SL-72,
//! SL-76), `shop-status` (SL-41, SL-76, SL-77; it deletes the file when the
//! library says the seed is gone), `lightning <index>` (SL-40, SL-42),
//! `shop-report` (SL-78) and `shop-settings <on|off> <version>`
//! (SL-77).
//!
//! **Test only, never use.** The three identities and the three wallet keys
//! are made from fixed bytes that anyone can read here. The money they would
//! guard is worth nothing, and must stay so.
//!
//! On the first run in a data directory the example makes a 2-of-3 wallet
//! description from three fixed test keys, and a signed batch of 20
//! addresses derived from it by the core crate. Later runs read both back
//! from the directory.
//!
//! `--regtest-single-peer` replaces the answer of the health rules with
//! "healthy". The rules ask for peers in three network groups and two filter
//! peers (WR-50), which one local node cannot be. The switch is here and not
//! in the library, it is printed loudly at the start, and what the real rules
//! say for the same peers is printed by the `health` command. A run with the
//! switch is not a check of the health rules.
//!
//! `--regtest-own-node` stands in for an own node (CR-100 to CR-107), which
//! the library reaches only through Tor, and which `openbmt-net-io` lets no
//! other crate reach on a clear-net address (NR-99). The node is started as
//! if it had an own node (a fixed test onion name in the driver's settings),
//! and the real driver is started without it. What the own node would say
//! is said by the script, with the `own` commands below, from what Core
//! really holds: the script says `own has` only for a transaction that is in
//! Core's mempool, and `own down` while Core is stopped. Two more stand-ins,
//! both printed at the start: every payment the driver sees is offered as
//! seen from three network groups (one local node is one group, and the
//! counter's own check needs three), and `heartbeat self` makes the owner's
//! heartbeat from the fixed test owner key (the owner watcher is not run).
//! A run with the switch is a check of the counter's wiring of the own
//! node, not of the own node's connection.
//!
//! Commands, one per line:
//!
//! - `open <minor units> <rate in minor units per bitcoin>`
//! - `judge <index>`
//! - `refund <index> <sats> <address>`: a cashier asks for a refund of a
//!   sale, to the address as a customer's wallet would show it
//!   (`refund_destination`)
//! - `heartbeat <hex>`: the bytes of a signed heartbeat from the owner's
//!   watcher (`owner_regtest`), taken by the node; `heartbeat self` makes
//!   one from the fixed test owner key, for the node's own tip
//! - `own up`, `own down`, `own has <txid>`: what the own node would say
//!   (`--regtest-own-node` only); `own status` prints the health line and
//!   how often the node asked
//! - `switch-take <description> <first index> <count>`: the owner's switch
//!   message (CR-257) to a new wallet with that first range, taken as the
//!   link takes it (LK-78): `switch taken` or `switch refused <why>`
//!   (`--own-wallet` only)
//! - `switch-state`: `switch pending address=<address> kind=<kind>` or
//!   `switch none`
//! - `switch-same`: the manager said "Same address" after the PIN (CR-261):
//!   `switch same record_bytes=<n>`; the new record is held in memory
//! - `switch-kept`: the app keeps that record in `own-record.bin`, and says
//!   so: `switch kept` (CR-261, steps three and four)
//! - `switch-different`: `switch different` (CR-260)
//! - `till-waiting`: the requests the home screen shows, one line each
//!   `till sale=<hex> amount=<minor> reference=<text> minutes_left=<n>`,
//!   then `till waiting=<n>` (CR-267)
//! - `till-start <sale hex> <rate>`: Start (CR-268); `opened ...` or `refused ...`
//! - `till-not-now <sale hex>`: `till not-now ok` or `till not-now none`
//! - `till-of <index>`: `till-of index=<i> sale=<hex|none> cancelled=<bool>`
//!   (CR-269)
//! - `limits`: the signed limits of the two checks
//! - `settings <hex>`: signed settings the owner sent, taken as the link
//!   takes them (CR-160 to CR-162): `settings taken version=<n>
//!   change=<Tightening|Loosening> watch_key=<hex|none>`, `settings dropped
//!   ...` or `settings refused <why>`
//! - `hbstate`
//! - `linkstate`
//! - `batches`
//! - `pump <seconds>`
//! - `health`
//! - `groups`
//! - `status`
//! - `quit`
//!
//! Between commands the node keeps taking the driver's events. Every command
//! ends with a line `done <n>`, where `n` counts the commands.
//!
//! Lines printed:
//!
//! - `descriptor <wallet description>`
//! - `warning ...` for the single-peer switch
//! - `opened index=<i> address=<address> sats=<n>` or `refused <text>`
//! - `sale <i> state=<State> received=<sats> needs=<sats> mode=<mode>
//!   check=<name|none> own_saw=<bool> own_counted=<bool> why=<text>`
//! - `own usable=<bool> heading=<text> asked=<n> last_asked=<txid|none>
//!   retried=<n>` and `limits light_per_sale_usd=<n> node_per_sale_usd=<n>`
//! - `refund requested index=<i> number=<n>` or `refund refused <rule and
//!   reason>`
//! - `report <Kind> index=<i> ...`, once for each report
//! - `heartbeat taken view=<Verified|NotVerified> ...` or
//!   `heartbeat refused <rule and reason>`
//! - `hbstate view=<none|Verified|NotVerified> policy_verified=<bool>
//!   policy_age=<secs> last_seq=<n>`
//! - `batches remaining=<n> records=<n> ranges=<a-b,...|none>`: the addresses
//!   the counter still has, and the records that say it took a batch (CR-167)
//!   with the range of each, in the order the records are in
//! - `linkstate owner_records=<n> owner_head=<hex> last_check_in_at=<secs|none>
//!   refused=<n> marks_ignored=<n> record_messages_sent=<n> heartbeats_taken=<n>
//!   node_refusals=<n> reachable=<bool> chain_records=<n> chain_head=<hex>`: the
//!   owner's mark (how many of the counter's records the owner says it holds),
//!   the last check-in, how many messages the courier refused, and the
//!   counter's own chain
//! - `health used=<healthy|unhealthy> real=<ok|problem> peers=<n>`
//! - `groups peers=<n> groups=<n> filter_groups=<n>`: the network groups
//!   (WR-41) of the ready peers at the last check
//! - `status tip=<h> checked=<h> peers=<n>`
//! - `note <text>`
//! - `stopped`

use std::collections::{BTreeMap, BTreeSet};
use std::io::{BufRead, Write};
use std::net::SocketAddr;
use std::path::Path;
use std::process::ExitCode;
use std::str::FromStr;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender, TryRecvError};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use bitcoin::bip32::{DerivationPath, Xpriv, Xpub};
use bitcoin::hashes::Hash;
use bitcoin::secp256k1::Secp256k1;
use bitcoin::{Address, Network, ScriptBuf, Txid};
use openbmt_core::batch::{BatchBuilder, BatchPolicy, PinnedRoots, SignedBatch};
use openbmt_core::heartbeat::WatcherHeartbeat;
use openbmt_core::identity::{DeviceId, Identity};
use openbmt_core::pairing_own::{OwnBundle, OwnOffer, OwnRequest, SwitchOwn};
use openbmt_core::pinned_wallet::{IndexRange, PinnedWallet, SignedRange};
use openbmt_core::records::RecordKind;
use openbmt_core::settings::{Settings, SignedSettings};
use openbmt_core::wallet::{KeySlot, WalletDescription};
use openbmt_counter::{
    refund_destination, Actor, Amount, BatchHeld, Currency, HeartbeatView, Pairing, Rate,
    RateSource, RefundAsk, RefundRequest, Report, Role as CashierRole,
};
use openbmt_counter_node::{
    CounterLink, CounterLinkConfig, DriverPort, Node, NodeConfig, OwnCeremony, OwnPairingRecord,
    MAX_OWN_ASKS,
};
use openbmt_courier::{Courier, CourierConfig, FirstStart};
use openbmt_link::{ClientConfig, Role, Route};
use openbmt_net_io::{
    AppEvent, Driver, DriverConfig, EventReceiver, FileStorage, Handle, OsRandomness, OwnNode,
    OwnNodeEvent, SystemClock,
};
use openbmt_watch::{
    check_health, network_group, HeaderChain, Health, HealthInput, OwnNodeInput, OwnNodeLink,
    PeerAddr,
};

const USAGE: &str = "usage: counter_regtest <data dir> <node address> [--regtest-single-peer] \
                     [--heartbeat-max-age <seconds>] [--customers-off-wifi] \
                     [--regtest-own-node] [--own-wallet <description>] \
                     [--relay <address> --relay-key <hex>] [--box-key <hex>] \
                     [--box-owner-fixed] \
                     [--shop-ln <node id> <provider id> <provider address> <cap sats> \
                     <max fee sats>] [--shop-ln-other]\n       counter_regtest --shop-ln-id <128 hex>";
const ADDRESSES: u32 = 20;

/// The Box that is the relay, from `--box-key`.
static BOX_KEY: OnceLock<DeviceId> = OnceLock::new();
/// Set by `--box-owner-fixed`: the owner of the link is `owner_regtest`'s even with a Box.
static BOX_OWNER_FIXED: OnceLock<()> = OnceLock::new();

/// What `--shop-ln` asks for (SL-71), and where the seed record is kept.
struct ShopLnPlan {
    node_id: [u8; 33],
    provider_id: [u8; 33],
    provider_addr: SocketAddr,
    cap_sats: u64,
    max_fee_sats: u64,
    /// The settings name another counter as the Lightning counter.
    other: std::sync::atomic::AtomicBool,
    dir: OnceLock<String>,
}

static SHOP_LN: OnceLock<ShopLnPlan> = OnceLock::new();

/// The file the app would keep the sealed seed record in.
const SHOP_SEED_FILE: &str = "shop-seed.bin";

/// TEST ONLY: a second counter, which the settings name when `--shop-ln ...
/// other` says this one is not the Lightning counter.
const OTHER_COUNTER_SECRET: [u8; 32] = [0x14; 32];

/// TEST ONLY: the shop's wallet of `--own-wallet test`, a single-key wallet
/// at 84h/0h/0h under a master key of fixed bytes.
fn test_wallet_text() -> Result<String, String> {
    let secp = Secp256k1::new();
    let master = Xpriv::new_master(Network::Bitcoin, &KEY_SEEDS[0]).map_err(|e| e.to_string())?;
    let path = DerivationPath::from_str("m/84h/0h/0h").map_err(|e| e.to_string())?;
    let account = master
        .derive_priv(&secp, &path)
        .map_err(|e| e.to_string())?;
    let wallet = openbmt_core::wallet_single::SingleKeyWallet::new(
        master.fingerprint(&secp),
        0,
        Xpub::from_priv(&secp, &account),
    )
    .map_err(|e| e.to_string())?;
    Ok(wallet.canonical_string().to_string())
}

/// SL-71: the Box as a device and a Box of the settings, and the
/// `shop_lightning` block, when `--shop-ln` was given.
fn shop_ln_settings(settings: &mut Settings, counter: &Identity) -> Result<(), String> {
    let (Some(plan), Some(box_key)) = (SHOP_LN.get(), BOX_KEY.get()) else {
        return Ok(());
    };
    settings.devices.insert(*box_key);
    settings.boxes.insert(*box_key);
    settings.customer_wifi = false;
    let named = if plan.other.load(Ordering::SeqCst) {
        let other = identity(&OTHER_COUNTER_SECRET)?.device_id();
        settings.devices.insert(other);
        settings.counters.insert(counter.device_id(), 9_000);
        settings.counters.insert(other, 1_000);
        other
    } else {
        counter.device_id()
    };
    settings.shop_lightning = Some(openbmt_core::settings::ShopLightning {
        counter: named,
        node_id: plan.node_id,
        second_device: *box_key,
        provider_node_id: plan.provider_id,
        provider_addr: plan.provider_addr,
        cap_sats: plan.cap_sats,
        max_opening_fee_sats: plan.max_fee_sats,
    });
    Ok(())
}

/// TEST ONLY: the name the node is told its own node has (a valid v3 onion
/// name; nothing is ever dialled at it).
const OWN_NODE_NAME: &str = "a4dqobyha4dqobyha4dqobyha4dqobyha4dqobyha4dqobyha4dwc6ad.onion";

/// TEST ONLY, NEVER USE: the secret bytes of the three identities.
const OWNER_SECRET: [u8; 32] = [0x11; 32];
const APPROVER_SECRET: [u8; 32] = [0x12; 32];
const COUNTER_SECRET: [u8; 32] = [0x13; 32];
/// TEST ONLY, NEVER USE: the seeds of the three wallet keys.
const KEY_SEEDS: [[u8; 32]; 3] = [[0xa1; 32], [0xa2; 32], [0xa3; 32]];

/// What the health function saw last, for the `health` and `status` lines.
#[derive(Debug, Default)]
struct Seen {
    real: Option<Health>,
    peers: usize,
    groups: usize,
    filter_groups: usize,
}

fn identity(secret: &[u8; 32]) -> Result<Identity, String> {
    Identity::from_secret_bytes(secret).map_err(|e| format!("identity: {e}"))
}

/// One of the three test keys, on the wallet's account path for regtest.
fn test_slot(index: u8, seed: &[u8; 32]) -> Result<KeySlot, String> {
    let secp = Secp256k1::new();
    let master = Xpriv::new_master(Network::Regtest, seed).map_err(|e| e.to_string())?;
    let path = DerivationPath::from_str("m/48h/1h/0h/2h").map_err(|e| e.to_string())?;
    let account = master
        .derive_priv(&secp, &path)
        .map_err(|e| e.to_string())?;
    Ok(KeySlot {
        index,
        origin_fingerprint: master.fingerprint(&secp),
        origin_path: path,
        xpub: Xpub::from_priv(&secp, &account),
    })
}

struct Setup {
    /// The wallet's description, as the `descriptor` line shows it.
    wallet: String,
    config: NodeConfig,
    settings: Settings,
}

/// The file of the pairing record, which the app keeps for a counter paired
/// with the shop's own wallet (`--own-wallet`).
const OWN_RECORD_FILE: &str = "own-record.bin";

/// What the switch commands share: the data directory, and the record the
/// last "Same address" made, until the app keeps it.
struct OwnFixture {
    dir: String,
    next_record: Mutex<Option<Vec<u8>>>,
}

static OWN_FIXTURE: OnceLock<OwnFixture> = OnceLock::new();

/// TEST ONLY: the real driver with the own node's channel fed by hand, and
/// every payment it sees offered as seen from three network groups. The
/// events of the driver are taken in `events`, which the node calls between
/// its own steps.
struct OwnPort {
    handle: Handle,
    events: EventReceiver,
    forward: Sender<AppEvent>,
    own: Receiver<OwnNodeEvent>,
    control: Arc<OwnControl>,
}

/// What the commands of the example share with the port.
struct OwnControl {
    say: Sender<OwnNodeEvent>,
    asked: Mutex<Vec<Txid>>,
    retried: AtomicU32,
}

fn three_groups(event: AppEvent) -> AppEvent {
    match event {
        AppEvent::Sighting { tx, .. } => AppEvent::Sighting {
            tx,
            from: (20..23)
                .map(|n| PeerAddr::Ip(format!("{n}.{n}.1.1").parse().expect("an address")))
                .collect(),
        },
        other => other,
    }
}

impl DriverPort for OwnPort {
    fn events(&self) -> &EventReceiver {
        while let Ok(event) = self.handle.events().try_recv() {
            let _ = self.forward.send(three_groups(event));
        }
        &self.events
    }

    fn watch_script(&self, script: ScriptBuf) {
        self.handle.watch_script(script);
    }

    fn set_all_blocks(&self, on: bool) {
        self.handle.set_all_blocks(on);
    }

    fn shutdown(&mut self) {
        self.handle.shutdown();
    }

    fn own_node_events(&self) -> Option<&Receiver<OwnNodeEvent>> {
        Some(&self.own)
    }

    fn ask_own_node(&self, txid: Txid) {
        if let Ok(mut asked) = self.control.asked.lock() {
            if asked.len() < MAX_OWN_ASKS {
                asked.push(txid);
            }
        }
    }

    fn retry_own_node(&self) {
        self.control.retried.fetch_add(1, Ordering::SeqCst);
    }
}

fn start_with_own_port(config: NodeConfig) -> Result<(Node<OwnPort>, Arc<OwnControl>), String> {
    let (say, own) = channel();
    let (forward, events) = channel();
    let control = Arc::new(OwnControl {
        say,
        asked: Mutex::new(Vec::new()),
        retried: AtomicU32::new(0),
    });
    let shared = control.clone();
    let node = Node::start_with(config, move |mut launch| {
        // The real driver has no own node: it would need Tor.
        launch.driver.own_node = None;
        let storage = FileStorage::new(&launch.dir).map_err(|e| e.to_string())?;
        let random = OsRandomness::new().map_err(|e| e.to_string())?;
        let handle = Driver::spawn(launch.driver, storage, SystemClock, random, launch.scripts)
            .map_err(|e| e.to_string())?;
        Ok(OwnPort {
            handle,
            events: events.into(),
            forward,
            own,
            control: shared,
        })
    })
    .map_err(|e| e.to_string())?;
    Ok((node, control))
}

/// The next heartbeat number of `heartbeat self`: the clock, then up by one.
static SELF_HEARTBEAT_SEQ: AtomicU64 = AtomicU64::new(0);

/// A heartbeat signed with the fixed test owner key, for the node's own tip.
fn self_heartbeat<P: DriverPort>(node: &Node<P>) -> Result<Vec<u8>, String> {
    let (height, hash) = {
        let chain = node.chain();
        let height = chain.tip_height();
        (height, chain.hash_at(height).ok_or("the node has no tip")?)
    };
    let seq = now_secs().max(SELF_HEARTBEAT_SEQ.load(Ordering::SeqCst) + 1);
    SELF_HEARTBEAT_SEQ.store(seq, Ordering::SeqCst);
    let heartbeat = WatcherHeartbeat {
        counter: identity(&COUNTER_SECRET)?.device_id(),
        seq,
        time: now_secs(),
        tip_height: height,
        tip_hash: hash.to_byte_array(),
        healthy: true,
    };
    Ok(heartbeat.sign(&identity(&OWNER_SECRET)?).to_bytes())
}

fn setup(
    dir: &str,
    peer: SocketAddr,
    single_peer: bool,
    heartbeat_max_age: Option<u32>,
    customers_off_wifi: bool,
    own_node: bool,
    seen: &Arc<Mutex<Seen>>,
) -> Result<Setup, String> {
    let owner = identity(&OWNER_SECRET)?;
    let approver = identity(&APPROVER_SECRET)?;
    let counter = identity(&COUNTER_SECRET)?;

    let slots = [
        test_slot(1, &KEY_SEEDS[0])?,
        test_slot(2, &KEY_SEEDS[1])?,
        test_slot(3, &KEY_SEEDS[2])?,
    ];
    let wallet = WalletDescription::new(Network::Regtest, slots).map_err(|e| e.to_string())?;
    let policy = BatchPolicy {
        min_count: ADDRESSES,
    };
    let pairing = Pairing {
        roots: PinnedRoots::new(owner.device_id(), approver.device_id())
            .map_err(|e| e.to_string())?,
        wallet_checksum: wallet.checksum32(),
        batch_policy: policy,
    };

    // The default settings, with this counter given the whole of every slice.
    let mut settings = Settings::defaults(1_000);
    settings.devices.insert(counter.device_id());
    settings.counters.insert(counter.device_id(), 10_000);
    if let Some(box_key) = BOX_KEY.get() {
        settings.devices.insert(*box_key);
        settings.boxes.insert(*box_key);
    }
    if customers_off_wifi {
        settings.customer_wifi = false;
    }
    if let Some(secs) = heartbeat_max_age {
        settings.heartbeat_max_age_secs = secs;
    }

    let batch = BatchBuilder::new(policy)
        .derive(&wallet, counter.device_id(), 0, ADDRESSES)
        .map_err(|e| e.to_string())?;
    let first_batch = SignedBatch {
        owner_sig: batch.sign(&owner),
        approver_sig: batch.sign(&approver),
        batch,
    };

    let mut driver = DriverConfig::new(Network::Regtest, 0);
    driver.allow_local = true;
    driver.fixed_peers = vec![peer];
    if own_node {
        // The node reads this as an own node being set; the real driver is
        // started without it (see `OwnPort`).
        driver.own_node = Some(OwnNode::parse(OWN_NODE_NAME, 18_444).map_err(|e| e.to_string())?);
    }
    let shown = settings.clone();
    let mut config = NodeConfig::new(dir, counter, pairing, settings, driver);
    config.first_batch = Some(first_batch);
    config.health = health_fn(seen, single_peer);
    Ok(Setup {
        wallet: wallet.canonical_string().to_string(),
        config,
        settings: shown,
    })
}

/// The health answer: the real rules, recorded for the `health` line, and
/// with `single_peer` replaced by "healthy" (regtest with one local node).
fn health_fn(seen: &Arc<Mutex<Seen>>, single_peer: bool) -> openbmt_counter_node::HealthFn {
    let seen = seen.clone();
    Box::new(move |input: &HealthInput<'_>| {
        let real = check_health(input);
        if let Ok(mut s) = seen.lock() {
            s.real = Some(real);
            s.peers = input.peers.len();
            s.groups = input
                .peers
                .iter()
                .map(|p| network_group(&p.addr))
                .collect::<BTreeSet<_>>()
                .len();
            s.filter_groups = input
                .peers
                .iter()
                .filter(|p| p.serves_filters)
                .map(|p| network_group(&p.addr))
                .collect::<BTreeSet<_>>()
                .len();
        }
        if single_peer {
            // Regtest with one local node only. Not the rules.
            Health {
                healthy: true,
                problem: None,
                behind: real.behind,
            }
        } else {
            real
        }
    })
}

/// The pairing record of a counter paired with the shop's own wallet, from
/// the file when there is one, and otherwise made by the whole ceremony of
/// CR-227 to CR-232: the offer of a starter wallet (CR-256), the counter's
/// request, the owner's bundle (the owner is the fixed test key) and the
/// counter's finish.
fn own_record(dir: &str, wallet: &PinnedWallet) -> Result<OwnPairingRecord, String> {
    let path = Path::new(dir).join(OWN_RECORD_FILE);
    if let Ok(bytes) = std::fs::read(&path) {
        return OwnPairingRecord::from_bytes(&bytes).map_err(|e| format!("the record: {e}"));
    }
    let owner = identity(&OWNER_SECRET)?;
    let counter = identity(&COUNTER_SECRET)?;
    let now = now_secs();
    let offer = OwnOffer::new(Network::Bitcoin, owner.device_id(), 0, wallet.clone())
        .map_err(|e| format!("the offer: {e}"))?
        .with_starter(true);
    let (mut ceremony, shown) =
        OwnCeremony::begin(&offer.to_bytes(), now).map_err(|e| e.to_string())?;
    say(format!(
        "offer version={} starter={} first_address={}",
        offer.to_bytes().get(4).copied().unwrap_or(0),
        offer.starter,
        shown.address
    ));
    let started = ceremony
        .address_same("Front counter", COUNTER_SECRET, [7; 32], now)
        .map_err(|e| e.to_string())?;
    ceremony.confirm(now).map_err(|e| e.to_string())?;
    let request = OwnRequest::from_bytes(&started.request).map_err(|e| format!("request: {e}"))?;
    let mut settings = Settings::defaults(1_000);
    settings.version = 1;
    settings.devices.insert(counter.device_id());
    settings.counters.insert(counter.device_id(), 10_000);
    shop_ln_settings(&mut settings, &counter)?;
    let signed = SignedSettings {
        owner_sig: settings.sign(&owner),
        approver_sig: None,
        settings,
    };
    let first = SignedRange::sign(
        &owner,
        IndexRange::new(counter.device_id(), 0, 1_000).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    let bundle = OwnBundle::build(&owner, &offer, &request, signed, first, Vec::new(), None)
        .map_err(|e| format!("the bundle: {e}"))?;
    let policy = BatchPolicy {
        min_count: openbmt_core::batch::DEFAULT_MIN_COUNT,
    };
    let record = ceremony
        .finish(&bundle.to_bytes(), &policy, now)
        .map_err(|e| e.to_string())?;
    std::fs::write(&path, record.to_bytes()).map_err(|e| format!("write the record: {e}"))?;
    Ok(record)
}

/// `--own-wallet`: the setup of a counter paired with the shop's own wallet.
fn setup_own(
    dir: &str,
    peer: SocketAddr,
    single_peer: bool,
    wallet_text: &str,
    seen: &Arc<Mutex<Seen>>,
) -> Result<Setup, String> {
    let wallet = PinnedWallet::from_text(wallet_text).map_err(|e| format!("the wallet: {e}"))?;
    let record = own_record(dir, &wallet)?;
    let counter = identity(&COUNTER_SECRET)?;
    let policy = BatchPolicy {
        min_count: openbmt_core::batch::DEFAULT_MIN_COUNT,
    };
    let settings = record.settings.settings.clone();
    let mut driver = DriverConfig::new(Network::Regtest, 0);
    driver.allow_local = true;
    driver.fixed_peers = vec![peer];
    let mut config = NodeConfig::new_own(
        dir,
        counter,
        record.own_pairing(policy),
        settings.clone(),
        driver,
    );
    config.own_record = Some(record);
    config.health = health_fn(seen, single_peer);
    let _ = OWN_FIXTURE.set(OwnFixture {
        dir: dir.to_string(),
        next_record: Mutex::new(None),
    });
    Ok(Setup {
        wallet: wallet_text.to_string(),
        config,
        settings,
    })
}

fn say(line: impl AsRef<str>) {
    println!("{}", line.as_ref());
    let _ = std::io::stdout().flush();
}

fn describe(report: &Report) -> String {
    match report {
        Report::PosPaid { index } => format!("report PosPaid index={index}"),
        Report::PosUnpaid { index } => format!("report PosUnpaid index={index}"),
        Report::BooksFinal {
            index,
            amount,
            currency,
            received_sats,
        } => format!(
            "report BooksFinal index={index} amount={} {currency} sats={received_sats}",
            amount.0
        ),
        Report::SurplusOwed { index, sats } => {
            format!("report SurplusOwed index={index} sats={sats}")
        }
        Report::LatePayment { index, sats } => {
            format!("report LatePayment index={index} sats={sats}")
        }
        Report::Alarm { index } => format!("report Alarm index={index}"),
        Report::Dust { index, sats } => format!("report Dust index={index} sats={sats}"),
    }
}

fn show<P: DriverPort>(node: &mut Node<P>, reports: &[Report]) {
    for report in reports {
        say(describe(report));
    }
    for note in node.take_notes() {
        say(format!("note {note}"));
    }
}

fn unhex(word: Option<&str>) -> Result<Vec<u8>, String> {
    let text = word.ok_or("the hex is missing")?;
    if text.len() % 2 != 0 || !text.is_ascii() {
        return Err("the hex is not whole bytes".to_string());
    }
    (0..text.len() / 2)
        .map(|i| {
            text.get(2 * i..2 * i + 2)
                .and_then(|pair| u8::from_str_radix(pair, 16).ok())
                .ok_or_else(|| "the hex has a character that is not a digit".to_string())
        })
        .collect()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn number<T: FromStr>(word: Option<&str>, what: &str) -> Result<T, String> {
    word.ok_or_else(|| format!("{what} is missing"))?
        .parse()
        .map_err(|_| format!("{what} is not a number"))
}

/// Run one command. `Ok(false)` is `quit`.
fn command<P: DriverPort>(
    node: &mut Node<P>,
    link: Option<&CounterLink>,
    own: Option<&OwnControl>,
    settings: &Settings,
    line: &str,
    seen: &Arc<Mutex<Seen>>,
) -> Result<bool, String> {
    let mut words = line.split_whitespace();
    match words.next() {
        Some("open") => {
            let minor: u64 = number(words.next(), "the amount")?;
            let rate: u64 = number(words.next(), "the rate")?;
            let rate = Rate::new(rate).ok_or("the rate is zero")?;
            let usd = Currency::new(*b"USD").ok_or("currency")?;
            let source = RateSource::new("regtest-fixed", now_secs()).map_err(|e| e.to_string())?;
            match node.open(Amount(minor), usd, rate, &source) {
                Ok(opened) => {
                    let address = Address::from_script(&opened.script, Network::Regtest)
                        .map_err(|e| e.to_string())?;
                    say(format!(
                        "opened index={} address={address} sats={}",
                        opened.index, opened.needs_sats
                    ));
                }
                Err(e) => say(format!("refused {e}")),
            }
        }
        Some("judge") => {
            let index: u32 = number(words.next(), "the index")?;
            match node.judge(index) {
                Ok(screen) => {
                    say(format!(
                        "sale {index} state={:?} received={} needs={} mode={:?} check={} \
                         own_saw={} own_counted={} why={}",
                        screen.state,
                        screen.received_sats,
                        screen.needs_sats,
                        screen.check_mode,
                        screen
                            .check_line()
                            .map_or("none", openbmt_counter::say::check_line_name),
                        screen.own_node_saw,
                        screen.own_node_counted,
                        screen.why_text()
                    ));
                    if let Some(words) = screen.check_words() {
                        say(format!("check {}: {}", words.heading, words.what));
                    }
                    show(node, &screen.reports);
                }
                Err(e) => say(format!("error {e}")),
            }
        }
        Some("refund") => {
            let index: u32 = number(words.next(), "the index")?;
            let sats: u64 = number(words.next(), "the amount")?;
            let text = words.next().ok_or("the address is missing")?;
            let destination = match refund_destination(text, Network::Regtest) {
                Ok(script) => script,
                Err(e) => {
                    say(format!("refund refused {e}"));
                    return Ok(true);
                }
            };
            let request = RefundRequest {
                ask: RefundAsk::Sats(sats),
                destination,
                actor: Actor {
                    role: CashierRole::Cashier,
                    name: "ann".to_string(),
                },
                reason: "regtest check".to_string(),
            };
            match node.request_refund(index, request) {
                Ok(number) => say(format!("refund requested index={index} number={number}")),
                Err(e) => say(format!("refund refused {e}")),
            }
        }
        Some("limits") => say(format!(
            "limits light_per_sale_usd={} node_per_sale_usd={}",
            settings.light_per_sale.display_usd, settings.node_per_sale.display_usd
        )),
        Some("own") => {
            let own = own.ok_or("there is no own node: give --regtest-own-node")?;
            match words.next() {
                Some("up") => {
                    let input = OwnNodeInput {
                        link: OwnNodeLink::Connected,
                        best_height: Some(node.chain().tip_height()),
                        last_heard: Some(now_secs()),
                    };
                    own.say
                        .send(OwnNodeEvent::State(input))
                        .map_err(|_| "the node is gone".to_string())?;
                }
                Some("down") => {
                    let input = OwnNodeInput {
                        link: OwnNodeLink::Down { since: now_secs() },
                        best_height: None,
                        last_heard: None,
                    };
                    own.say
                        .send(OwnNodeEvent::State(input))
                        .map_err(|_| "the node is gone".to_string())?;
                }
                Some("has") => {
                    let text = words.next().ok_or("the txid is missing")?;
                    let txid = Txid::from_str(text).map_err(|_| "the txid is not a txid")?;
                    own.say
                        .send(OwnNodeEvent::Has(txid))
                        .map_err(|_| "the node is gone".to_string())?;
                }
                Some("status") => {
                    let health = node.own_node_health();
                    let heading = health.words().map_or("none".to_string(), |w| w.heading);
                    let asked = own.asked.lock().map_err(|_| "lock".to_string())?;
                    say(format!(
                        "own usable={} heading={} asked={} last_asked={} retried={}",
                        health.usable,
                        heading,
                        asked.len(),
                        asked.last().map_or("none".to_string(), |t| t.to_string()),
                        own.retried.load(Ordering::SeqCst)
                    ));
                }
                _ => return Err("own: up, down, has <txid> or status".to_string()),
            }
        }
        Some("heartbeat") => {
            let word = words.next();
            let bytes = if word == Some("self") {
                self_heartbeat(node)?
            } else {
                unhex(word)?
            };
            match node.heartbeat(&bytes) {
                Ok(taken) => {
                    match taken.view {
                        HeartbeatView::Verified { sent_at } => {
                            say(format!("heartbeat taken view=Verified sent_at={sent_at}"))
                        }
                        HeartbeatView::NotVerified(why) => say(format!(
                            "heartbeat taken view=NotVerified rule={} disagreement={} why={why}",
                            why.rule(),
                            why.is_disagreement()
                        )),
                    }
                    show(node, &taken.reports);
                }
                Err(e) => say(format!("heartbeat refused {e}")),
            }
        }
        Some("settings") => {
            let bytes = unhex(words.next())?;
            let signed = SignedSettings::from_bytes(&bytes).map_err(|e| e.to_string())?;
            match node.take_settings(&signed) {
                Ok(Some(change)) => say(format!(
                    "settings taken version={} change={change:?} watch_key={}",
                    node.settings().version,
                    node.settings()
                        .watch_key
                        .map_or("none".to_string(), |k| hex(&k.to_bytes()))
                )),
                Ok(None) => say("settings dropped: not newer than the ones in force"),
                Err(e) => say(format!("settings refused {e}")),
            }
        }
        Some("shop-seed") => {
            let text = words.next().ok_or("the seed is missing")?;
            match node.shop_lightning_take_seed(text) {
                Ok(bytes) => {
                    let dir = SHOP_LN
                        .get()
                        .and_then(|p| p.dir.get())
                        .ok_or("no data dir")?;
                    std::fs::write(Path::new(dir).join(SHOP_SEED_FILE), &bytes)
                        .map_err(|e| format!("write the seed record: {e}"))?;
                    say(format!("shop-seed kept bytes={}", bytes.len()));
                }
                Err(e) => say(format!("shop-seed refused {} {e}", e.rule())),
            }
        }
        Some("shop-status") => match node.shop_lightning_status() {
            None => say("shop none"),
            Some(s) => {
                say(format!(
                    "shop needs_seed={} delete_seed={} why={}",
                    s.needs_seed,
                    s.delete_seed,
                    s.why
                        .as_ref()
                        .map_or("none".to_string(), |w| format!("{w:?}"))
                ));
                if s.delete_seed {
                    let dir = SHOP_LN
                        .get()
                        .and_then(|p| p.dir.get())
                        .ok_or("no data dir")?;
                    let _ = std::fs::remove_file(Path::new(dir).join(SHOP_SEED_FILE));
                    say("shop-seed deleted");
                }
            }
        },
        Some("shop-report") => match node.shop_lightning_report() {
            None => say("shop-report none"),
            Some(r) => say(format!(
                "shop-report balance={} room={} fees={} stored_at={} swept_at={} not_offered={:?}",
                r.balance_sats,
                r.receive_room_sats,
                r.opening_fees_paid_sats,
                r.last_stored_at,
                r.last_sweep_at,
                r.not_offered
            )),
        },
        Some("lightning") => {
            let index: u32 = number(words.next(), "the index")?;
            match node.lightning_sale(index) {
                openbmt_counter_node::SaleLightning::Off => say("lightning off"),
                openbmt_counter_node::SaleLightning::Waiting => say("lightning waiting"),
                openbmt_counter_node::SaleLightning::Invoice(c) => say(format!(
                    "lightning invoice hash={} msat={} bolt11={}",
                    hex(&c.payment_hash),
                    c.amount_msat,
                    c.bolt11
                )),
                openbmt_counter_node::SaleLightning::Unavailable(w) => {
                    say(format!("lightning unavailable {}", w.heading))
                }
                other => say(format!("lightning other {other:?}")),
            }
        }
        Some("shop-settings") => {
            let on = match words.next() {
                Some("on") => true,
                Some("off") => false,
                _ => return Err("on or off".to_string()),
            };
            let version: u32 = number(words.next(), "the version")?;
            let counter = identity(&COUNTER_SECRET)?;
            let mut next = node.settings().clone();
            next.version = version;
            if on {
                shop_ln_settings(&mut next, &counter)?;
            } else {
                next.shop_lightning = None;
            }
            let signed = SignedSettings {
                owner_sig: next.sign(&identity(&OWNER_SECRET)?),
                approver_sig: None,
                settings: next,
            };
            match node.take_settings(&signed) {
                Ok(Some(change)) => say(format!(
                    "settings taken version={} change={change:?}",
                    node.settings().version
                )),
                Ok(None) => say("settings dropped: not newer than the ones in force"),
                Err(e) => say(format!("settings refused {e}")),
            }
        }
        Some("till-waiting") => {
            let now = now_secs();
            let waiting = node.till_waiting();
            for r in &waiting {
                say(format!(
                    "till sale={} amount={} reference={} minutes_left={}",
                    hex(&r.sale),
                    r.amount.0,
                    r.reference,
                    r.minutes_left(now)
                ));
            }
            say(format!("till waiting={}", waiting.len()));
        }
        Some("till-start") => {
            let sale: [u8; 16] = unhex(words.next())?
                .try_into()
                .map_err(|_| "the sale is not 16 bytes".to_string())?;
            let rate: u64 = number(words.next(), "the rate")?;
            let rate = Rate::new(rate).ok_or("the rate is zero")?;
            let source = RateSource::new("regtest-fixed", now_secs()).map_err(|e| e.to_string())?;
            match node.till_start(&sale, rate, &source, None) {
                Ok(opened) => {
                    let address = Address::from_script(&opened.script, Network::Regtest)
                        .map_err(|e| e.to_string())?;
                    say(format!(
                        "opened index={} address={address} sats={}",
                        opened.index, opened.needs_sats
                    ));
                }
                Err(e) => say(format!("refused {e:?}")),
            }
        }
        Some("till-not-now") => {
            let sale: [u8; 16] = unhex(words.next())?
                .try_into()
                .map_err(|_| "the sale is not 16 bytes".to_string())?;
            say(if node.till_not_now(&sale) {
                "till not-now ok"
            } else {
                "till not-now none"
            });
        }
        Some("till-of") => {
            let index: u32 = number(words.next(), "the index")?;
            say(format!(
                "till-of index={index} sale={} cancelled={}",
                node.sale_till(index)
                    .map_or("none".to_string(), |s| hex(&s)),
                node.till_cancelled(index)
            ));
        }
        Some("hbstate") => {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |d| d.as_secs());
            let view = match node.counter().heartbeat_view() {
                None => "none".to_string(),
                Some(HeartbeatView::Verified { .. }) => "Verified".to_string(),
                Some(HeartbeatView::NotVerified(_)) => "NotVerified".to_string(),
            };
            let policy = node.counter().heartbeat_for_policy(now);
            say(format!(
                "hbstate view={view} policy_verified={} policy_age={} last_seq={}",
                policy.verified,
                policy.age_secs,
                node.counter().desk().last_heartbeat_seq()
            ));
        }
        Some("linkstate") => {
            let link = link.ok_or("the link is not started: give --relay and --relay-key")?;
            let state = link.state();
            let (chain_len, chain_head) = (node.records_len(), node.records_head());
            say(format!(
                "linkstate owner_records={} owner_head={} last_check_in_at={} refused={} \
                 marks_ignored={} record_messages_sent={} heartbeats_taken={} \
                 node_refusals={} reachable={} chain_records={} chain_head={}",
                state.owner_records,
                hex(&state.owner_head),
                state
                    .last_check_in_at
                    .map_or("none".to_string(), |t| t.to_string()),
                state.refused,
                state.marks_ignored,
                state.record_messages_sent,
                state.heartbeats_taken,
                state.node_refusals,
                state.relay_reachable,
                chain_len,
                hex(&chain_head),
            ));
        }
        Some("batches") => {
            let records = node
                .records_from(0, usize::MAX)
                .map_err(|e| e.to_string())?;
            let ranges: Vec<String> = records
                .iter()
                .filter(|r| r.kind == RecordKind::Setting)
                .filter_map(|r| BatchHeld::decode(&r.body).ok())
                .map(|b| format!("{}-{}", b.start, b.end))
                .collect();
            say(format!(
                "batches remaining={} records={} ranges={}",
                node.counter().batches().remaining(),
                ranges.len(),
                if ranges.is_empty() {
                    "none".to_string()
                } else {
                    ranges.join(",")
                }
            ));
        }
        Some("pump") => {
            let secs: f64 = number(words.next(), "the seconds")?;
            let reports = node.pump(Duration::from_secs_f64(secs.clamp(0.0, 600.0)));
            show(node, &reports);
        }
        Some("health") => {
            let seen = seen.lock().map_err(|_| "lock".to_string())?;
            let used = match node.health() {
                Some(h) if h.healthy => "healthy",
                _ => "unhealthy",
            };
            let real = match seen.real {
                Some(h) => h.problem.map_or("ok".to_string(), |p| format!("{p:?}")),
                None => "none-yet".to_string(),
            };
            say(format!(
                "health used={used} real={real} peers={}",
                seen.peers
            ));
        }
        Some("groups") => {
            let seen = seen.lock().map_err(|_| "lock".to_string())?;
            say(format!(
                "groups peers={} groups={} filter_groups={}",
                seen.peers, seen.groups, seen.filter_groups
            ));
        }
        Some("status") => {
            let peers = seen.lock().map_err(|_| "lock".to_string())?.peers;
            say(format!(
                "status tip={} checked={} peers={peers}",
                node.chain().tip_height(),
                node.checked_height()
            ));
        }
        Some(word) if word.starts_with("switch-") => switch_command(node, word, &mut words)?,
        Some("quit") => return Ok(false),
        Some(other) => return Err(format!("unknown command: {other}")),
        None => {}
    }
    Ok(true)
}

/// The switch commands (`--own-wallet` only), as the owner's link and the
/// app would drive them.
fn switch_command<P: DriverPort>(
    node: &mut Node<P>,
    word: &str,
    words: &mut std::str::SplitWhitespace<'_>,
) -> Result<(), String> {
    let fixture = OWN_FIXTURE
        .get()
        .ok_or("the switch commands need --own-wallet")?;
    match word {
        "switch-take" => {
            let text = words.next().ok_or("the new wallet is missing")?;
            let first: u32 = number(words.next(), "the first index")?;
            let count: u32 = number(words.next(), "the count")?;
            let new = PinnedWallet::from_text(text).map_err(|e| format!("the new wallet: {e}"))?;
            let owner = identity(&OWNER_SECRET)?;
            let counter = identity(&COUNTER_SECRET)?.device_id();
            let old = fixture_old_wallet(fixture)?;
            let range = SignedRange::sign(
                &owner,
                IndexRange::new(counter, first, first + count).map_err(|e| e.to_string())?,
            )
            .map_err(|e| e.to_string())?;
            let sw = SwitchOwn::sign(&owner, counter, old.checksum32(), new, range);
            match node.take_switch(&sw.to_bytes()) {
                Ok(()) => say("switch taken"),
                Err(e) => say(format!("switch refused {e}")),
            }
        }
        "switch-state" => match node.pending_switch() {
            Some(p) => say(format!(
                "switch pending address={} kind={:?}",
                p.address, p.kind
            )),
            None => say("switch none"),
        },
        "switch-same" => match node.switch_same() {
            Ok(record) => {
                say(format!("switch same record_bytes={}", record.len()));
                *fixture.next_record.lock().map_err(|e| e.to_string())? = Some(record);
            }
            Err(e) => say(format!("switch refused {e}")),
        },
        "switch-kept" => {
            let record = fixture
                .next_record
                .lock()
                .map_err(|e| e.to_string())?
                .take()
                .ok_or("no record waits to be kept")?;
            std::fs::write(Path::new(&fixture.dir).join(OWN_RECORD_FILE), &record)
                .map_err(|e| format!("keep the record: {e}"))?;
            match node.switch_record_kept() {
                Ok(()) => say("switch kept"),
                Err(e) => say(format!("switch refused {e}")),
            }
        }
        "switch-different" => match node.switch_different() {
            Ok(()) => say("switch different"),
            Err(e) => say(format!("switch refused {e}")),
        },
        other => return Err(format!("unknown command: {other}")),
    }
    Ok(())
}

/// The wallet the record in the app's file pins now.
fn fixture_old_wallet(fixture: &OwnFixture) -> Result<PinnedWallet, String> {
    let bytes = std::fs::read(Path::new(&fixture.dir).join(OWN_RECORD_FILE))
        .map_err(|e| format!("the record: {e}"))?;
    Ok(OwnPairingRecord::from_bytes(&bytes)
        .map_err(|e| format!("the record: {e}"))?
        .wallet)
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// The link to the owner through the relay. The owner is the fixed test key
/// of `owner_regtest`.
fn start_link(dir: &str, addr: SocketAddr, relay_key: DeviceId) -> Result<CounterLink, String> {
    // With a Box, the owner the link talks to is the owner of `box_pair`,
    // the one the Box is paired with.
    let owner = if BOX_KEY.get().is_some() && BOX_OWNER_FIXED.get().is_none() {
        let secret = openbmt_core::identity::tagged_hash("OpenBMT/example/box-pair/v1", b"owner");
        identity(&secret)?.device_id()
    } else {
        identity(&OWNER_SECRET)?.device_id()
    };
    let first = if ["link-inbox", "link-outbox"]
        .iter()
        .any(|f| Path::new(dir).join(f).exists())
    {
        FirstStart::No
    } else {
        FirstStart::Yes
    };
    let courier = Courier::start(
        CourierConfig {
            dir: dir.into(),
            identity: identity(&COUNTER_SECRET)?,
            role: Role::Counter,
            peers: BTreeMap::from([(owner, Role::Owner)])
                .into_iter()
                .chain(BOX_KEY.get().map(|b| (*b, Role::Box)))
                .collect(),
            route: Route::Direct(addr),
            relay: relay_key,
            client: ClientConfig::default(),
        },
        first,
    )
    .map_err(|e| e.to_string())?;
    Ok(CounterLink::new(
        courier,
        owner,
        CounterLinkConfig::default(),
    ))
}

fn run(args: &[String]) -> Result<(), String> {
    let mut single_peer = false;
    let mut heartbeat_max_age = None;
    let mut customers_off_wifi = false;
    let mut own_node = false;
    let mut own_wallet: Option<String> = None;
    let mut shop_ln_other = false;
    let mut relay: Option<SocketAddr> = None;
    let mut relay_key: Option<DeviceId> = None;
    let mut rest = Vec::new();
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        if arg == "--relay" {
            let addr = args.next().ok_or(USAGE)?;
            relay = Some(addr.parse().map_err(|_| format!("relay address: {addr}"))?);
        } else if arg == "--relay-key" {
            let key = args.next().ok_or(USAGE)?;
            let bytes: [u8; 32] = unhex(Some(key))?
                .try_into()
                .map_err(|_| "the relay key is not 32 bytes".to_string())?;
            relay_key = Some(DeviceId::from_bytes(&bytes).map_err(|e| format!("relay key: {e}"))?);
        } else if arg == "--box-key" {
            let key = args.next().ok_or(USAGE)?;
            let bytes: [u8; 32] = unhex(Some(key))?
                .try_into()
                .map_err(|_| "the Box key is not 32 bytes".to_string())?;
            let key = DeviceId::from_bytes(&bytes).map_err(|e| format!("Box key: {e}"))?;
            let _ = BOX_KEY.set(key);
        } else if arg == "--box-owner-fixed" {
            let _ = BOX_OWNER_FIXED.set(());
        } else if arg == "--regtest-single-peer" {
            single_peer = true;
        } else if arg == "--customers-off-wifi" {
            customers_off_wifi = true;
        } else if arg == "--regtest-own-node" {
            own_node = true;
        } else if arg == "--own-wallet" {
            let text = args.next().ok_or(USAGE)?;
            own_wallet = Some(if text == "test" {
                test_wallet_text()?
            } else {
                text.clone()
            });
        } else if arg == "--shop-ln-id" {
            let text = args.next().ok_or(USAGE)?;
            let seed: [u8; 64] = unhex(Some(text))?
                .try_into()
                .map_err(|_| "the seed is not 64 bytes".to_string())?;
            say(hex(&openbmt_core::shop_lightning::node_id(&seed)));
            return Ok(());
        } else if arg == "--shop-ln" {
            let mut next = || args.next().ok_or(USAGE.to_string());
            let key = |t: &str| -> Result<[u8; 33], String> {
                unhex(Some(t))?
                    .try_into()
                    .map_err(|_| "a node ID is not 33 bytes".to_string())
            };
            let node_id = key(next()?)?;
            let provider_id = key(next()?)?;
            let provider_addr: SocketAddr = next()?
                .parse()
                .map_err(|_| "the provider address".to_string())?;
            let cap_sats: u64 = next()?.parse().map_err(|_| "the cap".to_string())?;
            let max_fee_sats: u64 = next()?.parse().map_err(|_| "the fee".to_string())?;
            let _ = SHOP_LN.set(ShopLnPlan {
                node_id,
                provider_id,
                provider_addr,
                cap_sats,
                max_fee_sats,
                other: std::sync::atomic::AtomicBool::new(false),
                dir: OnceLock::new(),
            });
        } else if arg == "--shop-ln-other" {
            shop_ln_other = true;
        } else if arg == "--heartbeat-max-age" {
            let secs = args.next().ok_or(USAGE)?;
            heartbeat_max_age = Some(secs.parse().map_err(|_| format!("seconds: {secs}"))?);
        } else {
            rest.push(arg.as_str());
        }
    }
    let [dir, peer] = rest.as_slice() else {
        return Err(USAGE.to_string());
    };
    if relay.is_some() != relay_key.is_some() {
        return Err("--relay and --relay-key go together".to_string());
    }
    let peer: SocketAddr = peer.parse().map_err(|_| format!("node address: {peer}"))?;
    if let Some(plan) = SHOP_LN.get() {
        plan.other.store(shop_ln_other, Ordering::SeqCst);
        let _ = plan.dir.set(dir.to_string());
    }
    let seen = Arc::new(Mutex::new(Seen::default()));
    let Setup {
        wallet,
        mut config,
        settings,
    } = match own_wallet.as_deref() {
        Some(text) => setup_own(dir, peer, single_peer, text, &seen)?,
        None => setup(
            dir,
            peer,
            single_peer,
            heartbeat_max_age,
            customers_off_wifi,
            own_node,
            &seen,
        )?,
    };

    if SHOP_LN.get().is_some() {
        // SL-70: the counter's seed record is the file the app would keep.
        let hint = match (relay, relay_key) {
            (Some(addr), Some(key)) => Some(
                openbmt_core::relay::RelayHint::new(
                    openbmt_core::relay::RelayRoute::Direct(addr),
                    key.to_bytes(),
                )
                .map_err(|e| format!("the relay hint: {e}"))?,
            ),
            _ => None,
        };
        config.shop_lightning = Some(openbmt_counter_node::ShopLnConfig {
            seed_record: std::fs::read(Path::new(dir).join(SHOP_SEED_FILE)).ok(),
            identity: Box::new(|| identity(&COUNTER_SECRET).ok()),
            relay: hint,
            link_proxy: None,
        });
    }

    say(format!("descriptor {wallet}"));
    if single_peer {
        say("warning --regtest-single-peer: the health answer is replaced by \"healthy\"");
        say("warning one local node cannot meet WR-50 (3 groups, 2 filter peers):");
        say("warning THIS RUN IS NOT A CHECK OF THE HEALTH RULES");
    }
    if own_node {
        say(
            "warning --regtest-own-node: the own node is said by hand (own up, own down, own has),",
        );
        say("warning every payment is offered as seen from three network groups,");
        say("warning and heartbeat self is signed with the test owner key");
        let (node, control) = start_with_own_port(config)?;
        return serve(
            node,
            Some(&control),
            &settings,
            relay,
            relay_key,
            dir,
            &seen,
        );
    }
    let node = Node::start(config).map_err(|e| e.to_string())?;
    serve(node, None, &settings, relay, relay_key, dir, &seen)
}

/// Start the link if asked, then take the commands until `quit`.
fn serve<P: DriverPort>(
    mut node: Node<P>,
    own: Option<&Arc<OwnControl>>,
    settings: &Settings,
    relay: Option<SocketAddr>,
    relay_key: Option<DeviceId>,
    dir: &str,
    seen: &Arc<Mutex<Seen>>,
) -> Result<(), String> {
    say(format!(
        "started checked={} tip={}",
        node.checked_height(),
        node.chain().tip_height()
    ));
    let mut link = match (relay, relay_key) {
        (Some(addr), Some(key)) => Some(start_link(dir, addr, key)?),
        _ => None,
    };
    if let (Some(link), Some(_)) = (link.as_mut(), BOX_KEY.get()) {
        link.enable_notices(identity(&COUNTER_SECRET)?, dir, &node)
            .map_err(|e| format!("the notices: {e}"))?;
    }

    let (lines, input) = channel();
    std::thread::spawn(move || {
        for line in std::io::stdin().lock().lines() {
            match line {
                Ok(l) => {
                    if lines.send(l).is_err() {
                        return;
                    }
                }
                Err(_) => return,
            }
        }
    });

    let mut done = 0u64;
    loop {
        match input.try_recv() {
            Ok(line) => {
                match command(
                    &mut node,
                    link.as_ref(),
                    own.map(|c| &**c),
                    settings,
                    line.trim(),
                    seen,
                ) {
                    Ok(true) => {}
                    Ok(false) => break,
                    Err(e) => say(format!("error {e}")),
                }
                done += 1;
                say(format!("done {done}"));
            }
            Err(TryRecvError::Empty) => {
                let mut reports = node.pump(Duration::from_millis(100));
                if let Some(link) = link.as_mut() {
                    reports.extend(link.step(&mut node, now_secs()));
                    for note in link.take_notes() {
                        say(format!("note link {note}"));
                    }
                }
                show(&mut node, &reports);
            }
            Err(TryRecvError::Disconnected) => break,
        }
    }
    let reports = node.shutdown();
    show(&mut node, &reports);
    say("stopped");
    Ok(())
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(why) => {
            say(format!("failed {why}"));
            ExitCode::from(1)
        }
    }
}
