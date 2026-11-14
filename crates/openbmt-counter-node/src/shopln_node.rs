//! Shop Lightning's node at the counter: the node of
//! `openbmt-lightning-node` started on the counter that the settings name,
//! its second copy kept on the Box over the shop's own link, its invoices
//! given to the sales and its paid events given back to them
//! (`docs/SHOP-LIGHTNING-RULES.md` SL-70 to SL-78).
//!
//! The counter's node is synchronous and the Lightning node is not, so this
//! module owns a small runtime and never waits on it: a start, an invoice and
//! an acknowledgement are tasks, and what they find is read at the next call.
//!
//! ## One instance at a time
//!
//! An instance is a running node and what it was started from. It is in one
//! of three phases:
//!
//! - serving: the counter offers invoices (SL-40, SL-41);
//! - closing: the settings stopped naming the node, so no invoice is offered,
//!   every channel is closed cooperatively (SL-36) and the node goes on
//!   storing twice until every channel is closed and its money is in the
//!   shop's wallet; then the seed is deleted (SL-77);
//! - stopping: the node is stopped to be started again from other settings
//!   of the same node (another provider, another limit).
//!
//! [`ShopLn::reconcile`] moves between them from the settings in force, and
//! is called at every pump. A node that did not start (the Box away, the
//! chain not reachable) is started again every [`RESTART_SECS`] seconds.
//!
//! Where the code is more exact than the rules:
//!
//! - The sweep of SL-33 is begun here: once the node's balance is over the
//!   cap, everything over half the cap goes to the wallet, at most one try
//!   every [`SWEEP_EVERY_SECS`] seconds. The address is the last index of the
//!   pairing's first range of the shop's own wallet, which the owner's
//!   wallet already watches; a counter paired with the Key app holds no
//!   description of the wallet, so Shop Lightning does not start on it
//!   ([`Why::NoWallet`]).
//! - A channel the provider opens may be used before its funding transaction
//!   is confirmed (`zero_conf`): the provider funds it, and the shop holds
//!   nothing in it until a payment comes.
//! - The invoices are kept in the book, in plain, until their payment is
//!   acknowledged, so that a paid event after a restart finds its sale. A
//!   paid event of an invoice that is not in the book is noted and
//!   acknowledged: the counter has no sale to give it to.
//! - What a sale is credited is what arrived plus the provider's opening fee
//!   (SL-29): the customer paid the whole invoice, and the shop bears the
//!   fee.

use std::collections::HashMap;
use std::fs;
use std::io;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use bitcoin::secp256k1::PublicKey;
use bitcoin::Network;
use openbmt_core::identity::{DeviceId, Identity};
use openbmt_core::relay::RelayHint;
use openbmt_core::settings::{Settings, ShopLightning};
use openbmt_lightning::bolt11::Checked;
use openbmt_lightning::PaidFacts;
use openbmt_lightning_node::bip85::NodeSeed;
use openbmt_lightning_node::cap::sweep_amount_sat;
use openbmt_lightning_node::chain::ChainSource;
use openbmt_lightning_node::device::{DeviceError, DeviceFuture, DeviceState, SecondDevice};
use openbmt_lightning_node::device_link::{LinkDevice, Persist, StoreTransport};
use openbmt_lightning_node::filters::FilterConfig;
use openbmt_lightning_node::node::{
    InvoiceError, Node as LnNode, NodeConfig as LnConfig, Paid, Receive, StartError,
    MAINNET_ALLOWED,
};
use openbmt_lightning_node::provider::ProviderConfig;
use openbmt_lightning_node::room::OnChainReason;
use openbmt_lightning_node::state::StartFrom;
use openbmt_link::numbers::{STORE_INBOX_FILE, STORE_OUTBOX_FILE};
use openbmt_link::shopln::{LightningReport, NotOffered};
use openbmt_link::{Client, ClientConfig, Inbox, Outbox, Route};
use openbmt_net_io::DriverConfig;
use tokio::runtime::{Builder, Runtime};

use crate::lightning::SaleLightning;
use crate::relaylink::link_route;
use crate::shopln::{
    role, take_seed, Book, InvoiceEntry, Role, SeedNotTaken, SeedRecord, Why, MAX_INVOICES,
};

/// How often a node that did not start is started again, in seconds.
pub const RESTART_SECS: u64 = 10;
/// The fewest seconds between two tries to sweep (SL-33).
pub const SWEEP_EVERY_SECS: u64 = 600;
/// Seconds an invoice may wait on a write to the Box before the sale says
/// Lightning is not offered for now (SL-41).
const WRITE_WAIT_SECS: u64 = 8;
/// How long an invoice that could not be offered is not asked for again, in
/// seconds.
pub const RETRY_OFFER_SECS: u64 = 5;
/// The description on the invoices. It names neither the shop nor the sale.
pub const DESCRIPTION: &str = crate::swap::DESCRIPTION;
/// The folder of the node's state, in the folder of Shop Lightning.
const NODE_DIR: &str = "node";
/// The folder of the node's chain watcher.
const CHAIN_DIR: &str = "chain";

/// What the app gives the counter at its start for Shop Lightning.
pub struct ShopLnConfig {
    /// The sealed seed record's bytes, as the app kept them, if it has them.
    pub seed_record: Option<Vec<u8>>,
    /// Makes the counter's identity, which seals and signs the Store
    /// messages (SL-74). It is made again at each start of the node.
    pub identity: Box<dyn Fn() -> Option<Identity> + Send>,
    /// The relay of the pairing, which must be the Box named as the second
    /// device (SL-73).
    pub relay: Option<RelayHint>,
    /// The proxy the link to the relay goes through, if it needs one.
    pub link_proxy: Option<SocketAddr>,
}

/// What the node of the counter hands over for Shop Lightning at its start.
pub struct ShopLnArgs {
    /// This counter.
    pub me: DeviceId,
    /// The folder of the counter's data.
    pub dir: PathBuf,
    /// The network.
    pub network: Network,
    /// The driver's settings as the counter's other chain users have them:
    /// the same peers, and the same proxy.
    pub driver: DriverConfig,
    /// Whether Tor is on in the settings (SL-32).
    pub tor_on: bool,
    /// The shop's pinned wallet and the index the sweep and the close pay to
    /// (SL-33), when the counter holds a description of the wallet.
    pub wallet: Option<(String, u32)>,
    /// From the app.
    pub config: ShopLnConfig,
}

/// What a sale's invoice is.
enum Slot {
    Pending,
    Ready(Checked),
    Off(Why),
}

struct SaleSlot {
    sats: u64,
    state: Arc<Mutex<Slot>>,
    asked_at: u64,
}

/// What the book and its folder are, shared with the tasks.
struct Shared {
    dir: PathBuf,
    book: Mutex<Book>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl Shared {
    fn save(&self, book: &Book) -> io::Result<()> {
        book.save(&self.dir)
    }
}

/// What a start found.
struct InstShared {
    node: Mutex<Option<Arc<LnNode>>>,
    failed: Mutex<Option<Why>>,
    stopped: AtomicBool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Serving,
    Closing,
    Stopping,
}

struct Instance {
    sl: ShopLightning,
    phase: Phase,
    shared: Arc<InstShared>,
    /// When the last start failed, to start again after [`RESTART_SECS`].
    failed_at: Option<u64>,
    /// Whether the closes were begun, and when.
    close_begun_at: Option<u64>,
    /// The last time a sweep was tried.
    sweep_tried_at: Option<u64>,
    /// The node has been told to stop for good, to delete the seed.
    finishing: bool,
}

/// A paid sale for the counter to record (SL-21).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PaidSale {
    /// The sale.
    pub sale: u32,
    /// What the counter is told, in the form its Lightning path takes.
    pub facts: PaidFacts,
}

/// What Home and the app read about Shop Lightning.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShopLnStatus {
    /// This counter is the Lightning counter and holds no seed: the app
    /// shows the screen that scans it (SL-76).
    pub needs_seed: bool,
    /// The seed is gone: the app deletes what it sealed (SL-77).
    pub delete_seed: bool,
    /// The reason Lightning is not offered, if it is not.
    pub why: Option<Why>,
}

/// Shop Lightning at the counter.
pub struct ShopLn {
    me: DeviceId,
    network: Network,
    driver: DriverConfig,
    tor_on: bool,
    wallet: Option<(String, u32)>,
    config: ShopLnConfig,
    shared: Arc<Shared>,
    seed: Option<SeedRecord>,
    seed_wiped: bool,
    inst: Option<Instance>,
    rt: Option<Runtime>,
    slots: HashMap<u32, SaleSlot>,
    pending: Vec<Paid>,
    notes: Vec<String>,
    /// Whether the settings in force name this counter (SL-70).
    named: bool,
    /// Why no instance runs while the settings name this counter, or while a
    /// node is to be closed.
    blocked: Option<Why>,
    /// When an instance was last tried, so that one that cannot be made is
    /// not tried at every pump.
    last_try: Option<u64>,
    dir: PathBuf,
}

impl ShopLn {
    /// Reads the book and the seed record the app handed over. A seed record
    /// that does not read, or a book that does not, is noted and left out:
    /// nothing is started from it.
    pub fn new(mut args: ShopLnArgs) -> Self {
        let dir = args.dir.join("shop-lightning");
        let mut notes = Vec::new();
        let book = Book::load(&dir).unwrap_or_else(|e| {
            notes.push(format!("SL-77: {e}"));
            Book::default()
        });
        let seed = args.config.seed_record.take().and_then(|bytes| {
            SeedRecord::from_bytes(&bytes)
                .map_err(|e| notes.push(format!("SL-76: the stored Lightning key: {e}")))
                .ok()
        });
        Self {
            me: args.me,
            network: args.network,
            driver: args.driver,
            tor_on: args.tor_on,
            wallet: args.wallet,
            config: args.config,
            shared: Arc::new(Shared {
                dir: dir.clone(),
                book: Mutex::new(book),
            }),
            seed,
            seed_wiped: false,
            inst: None,
            rt: None,
            slots: HashMap::new(),
            pending: Vec::new(),
            notes,
            named: false,
            blocked: None,
            last_try: None,
            dir,
        }
    }

    /// The lines for the node's log since the last call.
    pub fn take_notes(&mut self) -> Vec<String> {
        std::mem::take(&mut self.notes)
    }

    fn rt(&mut self) -> Option<&Runtime> {
        if self.rt.is_none() {
            match Builder::new_multi_thread()
                .worker_threads(2)
                .thread_name("shop-lightning")
                .enable_all()
                .build()
            {
                Ok(rt) => self.rt = Some(rt),
                Err(e) => self.notes.push(format!("SL-02: no runtime: {e}")),
            }
        }
        self.rt.as_ref()
    }

    /// Whether a seed is held.
    pub fn has_seed(&self) -> bool {
        self.seed.is_some()
    }

    /// SL-72, SL-76: a seed code a manager scanned. The seed is checked
    /// against the pinned node ID first and nothing is stored on a mismatch.
    /// On success the node holds the seed and the bytes are for the app to
    /// seal and keep.
    pub fn take_seed(
        &mut self,
        text: &str,
        settings: &Settings,
        now: u64,
    ) -> Result<Vec<u8>, SeedNotTaken> {
        let record = take_seed(text, settings, &self.me, self.seed.is_some())?;
        let bytes = record.to_bytes();
        self.seed = Some(record);
        self.seed_wiped = false;
        self.last_try = None;
        self.reconcile(settings, now);
        Ok(bytes)
    }

    /// SL-70, SL-72, SL-77: bring the instance to what the settings in force
    /// say. Called at every pump; it does no waiting.
    pub fn reconcile(&mut self, settings: &Settings, now: u64) {
        let want = match role(settings, &self.me) {
            Role::Here(sl) => Some(*sl),
            _ => None,
        };
        self.named = want.is_some();
        enum Do {
            Nothing,
            Restart,
            Close,
        }
        let act = match (&self.inst, want) {
            (Some(inst), Some(w)) if inst.phase == Phase::Serving && w == inst.sl => Do::Nothing,
            (Some(inst), Some(w))
                if inst.phase == Phase::Serving
                    && w.counter == inst.sl.counter
                    && w.node_id == inst.sl.node_id =>
            {
                // The same node with other terms: stop it, start it again.
                Do::Restart
            }
            // SL-77: off, or another counter, or another node.
            (Some(inst), _) if inst.phase == Phase::Serving => Do::Close,
            _ => Do::Nothing,
        };
        match act {
            Do::Nothing => {}
            Do::Restart => {
                if let Some(inst) = &mut self.inst {
                    inst.phase = Phase::Stopping;
                }
                self.stop_instance_task();
            }
            Do::Close => {
                if let Some(inst) = &mut self.inst {
                    inst.phase = Phase::Closing;
                }
                self.slots.clear();
            }
        }
        self.advance(now);
        if self.inst.is_none() {
            self.start_from_settings(want, now);
        }
    }

    /// The part of a reconcile that moves a running instance on.
    fn advance(&mut self, now: u64) {
        let Some(inst) = &self.inst else {
            return;
        };
        match inst.phase {
            Phase::Stopping => {
                if inst.shared.stopped.load(Ordering::SeqCst) {
                    self.inst = None;
                    self.last_try = None;
                }
            }
            Phase::Serving | Phase::Closing => {
                self.note_failed(now);
                self.retry_start(now);
                match self.inst.as_ref().map(|i| i.phase) {
                    Some(Phase::Serving) => self.sweep(now),
                    Some(Phase::Closing) => self.close(now),
                    _ => {}
                }
            }
        }
    }

    /// Starts again a node that did not start, after [`RESTART_SECS`].
    fn retry_start(&mut self, now: u64) {
        let Some(inst) = &mut self.inst else {
            return;
        };
        let Some(at) = inst.failed_at else {
            return;
        };
        if now < at.saturating_add(RESTART_SECS) {
            return;
        }
        let sl = inst.sl;
        let shared = Arc::clone(&inst.shared);
        inst.failed_at = None;
        *lock(&shared.failed) = None;
        match self.prepare(&sl) {
            Ok(config) => self.spawn_start(config, shared),
            Err(why) => {
                *lock(&shared.failed) = Some(why);
            }
        }
    }

    /// With no instance: make one, or clean up what a finished one left.
    fn start_from_settings(&mut self, want: Option<ShopLightning>, now: u64) {
        let plan = lock(&self.shared.book).plan;
        let target = match (want, plan) {
            // The settings name this node: start it, when the seed is there
            // and gives the pinned ID.
            (Some(sl), plan) => match &self.seed {
                None => {
                    self.blocked = Some(Why::NoSeed);
                    return;
                }
                Some(seed) if seed.node_id() == sl.node_id => Some((sl, Phase::Serving)),
                // A seed of another node is still held: the old node is
                // closed first (SL-13, SL-77).
                Some(_) => match plan {
                    Some(old) => Some((old, Phase::Closing)),
                    None => {
                        self.wipe_seed();
                        self.blocked = Some(Why::NoSeed);
                        return;
                    }
                },
            },
            // The settings do not name it, and a node was run: close it from
            // the plan the book kept (SL-77).
            (None, Some(old)) if self.seed.is_some() => Some((old, Phase::Closing)),
            // A node was run and its seed is gone: nothing is left to close
            // from here.
            (None, Some(_)) => {
                self.forget_plan();
                None
            }
            // A seed that never ran a node is deleted at once.
            (None, None) if self.seed.is_some() => {
                self.wipe_seed();
                None
            }
            (None, None) => None,
        };
        let Some((sl, phase)) = target else {
            self.blocked = None;
            return;
        };
        if self
            .last_try
            .is_some_and(|t| now < t.saturating_add(RESTART_SECS))
        {
            return;
        }
        self.last_try = Some(now);
        self.begin(sl, phase);
    }

    /// Makes an instance of `sl` and starts its node. A start that cannot be
    /// made leaves the reason in `blocked` and no instance.
    fn begin(&mut self, sl: ShopLightning, phase: Phase) {
        let config = match self.prepare(&sl) {
            Ok(config) => config,
            Err(why) => {
                self.blocked = Some(why);
                return;
            }
        };
        // The plan is written before the node runs, so that a node being
        // closed after the settings stopped naming it is found again.
        {
            let mut book = lock(&self.shared.book);
            if book.plan != Some(sl) {
                book.plan = Some(sl);
                if let Err(e) = self.shared.save(&book) {
                    self.notes
                        .push(format!("SL-77: the book was not written: {e}"));
                    self.blocked = Some(Why::NotStarted(e.to_string()));
                    return;
                }
            }
        }
        self.blocked = None;
        let shared = Arc::new(InstShared {
            node: Mutex::new(None),
            failed: Mutex::new(None),
            stopped: AtomicBool::new(false),
        });
        self.spawn_start(config, Arc::clone(&shared));
        self.inst = Some(Instance {
            sl,
            phase,
            shared,
            failed_at: None,
            close_begun_at: None,
            sweep_tried_at: None,
            finishing: false,
        });
    }

    /// The checks of SL-60, SL-73 and SL-33, and the node's configuration.
    /// `Err` is the reason the node is not started.
    fn prepare(&self, sl: &ShopLightning) -> Result<LnConfig, Why> {
        if self.network == Network::Bitcoin && !MAINNET_ALLOWED {
            return Err(Why::Mainnet);
        }
        let seed = self.seed.as_ref().ok_or(Why::NoSeed)?;
        if seed.node_id() != sl.node_id {
            return Err(Why::NoSeed);
        }
        let (wallet, sweep_index) = self.wallet.clone().ok_or(Why::NoWallet)?;
        let hint = self.config.relay.clone().ok_or(Why::NotTheBox)?;
        let relay = hint.device().ok_or(Why::NotTheBox)?;
        if relay != sl.second_device {
            return Err(Why::NotTheBox);
        }
        let route = link_route(
            &hint,
            self.config.link_proxy.or(self.driver.proxy),
            self.tor_on,
        )
        .map_err(|_| Why::NoRoute)?;
        let identity = (self.config.identity)()
            .ok_or_else(|| Why::NotStarted("the counter's identity does not read".to_string()))?;
        let provider = PublicKey::from_slice(&sl.provider_node_id)
            .map_err(|e| Why::NotStarted(e.to_string()))?;
        let mut driver = self.driver.clone();
        if !self.tor_on {
            driver.proxy = None;
        }
        let device = RelayDevice::open(&self.dir, identity, relay, route)
            .map_err(|e| Why::NotStarted(format!("the numbers of the Store messages: {e}")))?;
        Ok(LnConfig {
            network: self.network,
            seed: NodeSeed(*seed.seed()),
            data_dir: self.dir.join(NODE_DIR),
            chain: ChainSource::Filters(FilterConfig {
                dir: self.dir.join(CHAIN_DIR),
                driver: driver.clone(),
            }),
            tor: driver.proxy,
            device: Arc::new(device),
            provider: ProviderConfig {
                node_id: provider,
                addr: sl.provider_addr,
                token: None,
                zero_conf: true,
            },
            max_opening_fee_msat: sl.max_opening_fee_sats.saturating_mul(1000),
            start_from: StartFrom::Newer,
            wallet,
            sweep_index,
        })
    }

    /// Starts the node as a task.
    fn spawn_start(&mut self, config: LnConfig, shared: Arc<InstShared>) {
        let Some(handle) = self.rt().map(|rt| rt.handle().clone()) else {
            *lock(&shared.failed) = Some(Why::NotStarted("no runtime".to_string()));
            return;
        };
        // The library's start is a future that cannot move between threads,
        // so a thread of its own drives it on the runtime.
        let spawned = std::thread::Builder::new()
            .name("shop-lightning-start".to_string())
            .spawn({
                let shared = Arc::clone(&shared);
                move || match handle.block_on(LnNode::start(config)) {
                    Ok(node) => *lock(&shared.node) = Some(Arc::new(node)),
                    Err(e) => *lock(&shared.failed) = Some(why_not_started(e)),
                }
            });
        if let Err(e) = spawned {
            *lock(&shared.failed) = Some(Why::NotStarted(e.to_string()));
        }
    }

    /// The running node, if there is one.
    fn node(&self) -> Option<Arc<LnNode>> {
        let inst = self.inst.as_ref()?;
        lock(&inst.shared.node).clone()
    }

    /// A failed start is noted once, and the instance starts again later.
    fn note_failed(&mut self, now: u64) {
        let Some(inst) = &mut self.inst else {
            return;
        };
        if inst.failed_at.is_some() {
            return;
        }
        let failed = lock(&inst.shared.failed).clone();
        if let Some(why) = failed {
            inst.failed_at = Some(now);
            self.notes
                .push(format!("SL-23: the Lightning node did not start: {why:?}"));
        }
    }

    /// SL-33: the sweep, when the balance is over the cap.
    fn sweep(&mut self, now: u64) {
        let Some(node) = self.node() else {
            return;
        };
        let Some(inst) = &mut self.inst else {
            return;
        };
        if inst
            .sweep_tried_at
            .is_some_and(|at| now < at.saturating_add(SWEEP_EVERY_SECS))
        {
            return;
        }
        let Some(amount) = sweep_amount_sat(node.balance_sat(), inst.sl.cap_sats) else {
            return;
        };
        inst.sweep_tried_at = Some(now);
        let index = self.wallet.as_ref().map_or(0, |(_, i)| *i);
        match node.sweep(index, amount) {
            Ok(()) => {
                let mut book = lock(&self.shared.book);
                book.last_sweep_at = now;
                if let Err(e) = self.shared.save(&book) {
                    self.notes
                        .push(format!("SL-77: the book was not written: {e}"));
                }
            }
            Err(e) => self.notes.push(format!("SL-33: no sweep: {e}")),
        }
    }

    /// SL-36, SL-77: close every channel, and when none is left, nothing
    /// waits to be swept and every write has reached the Box, delete the
    /// seed.
    fn close(&mut self, now: u64) {
        let Some(node) = self.node() else {
            return;
        };
        let index = self.wallet.as_ref().map_or(0, |(_, i)| *i);
        let Some(inst) = &mut self.inst else {
            return;
        };
        if inst.finishing {
            if inst.shared.stopped.load(Ordering::SeqCst) {
                self.inst = None;
                self.last_try = None;
                self.wipe_seed();
                self.forget_plan();
            }
            return;
        }
        let status = node.status();
        let begin = inst
            .close_begun_at
            .is_none_or(|at| status.channels > 0 && now >= at.saturating_add(RESTART_SECS * 6));
        if begin && status.channels > 0 {
            inst.close_begun_at = Some(now);
            match node.close_all(index) {
                Ok(n) => self
                    .notes
                    .push(format!("SL-36: closing {n} Lightning channels")),
                Err(e) => self.notes.push(format!("SL-36: the close: {e}")),
            }
            return;
        }
        let done = close_done(
            status.channels,
            node.claimable_balances(),
            node.tracked_outputs(),
            status.device.pending_writes,
            status.device.state == DeviceState::Reachable,
        );
        if done {
            inst.finishing = true;
            let shared = Arc::clone(&inst.shared);
            if let Some(rt) = self.rt.as_ref() {
                rt.spawn(async move {
                    node.stop().await;
                    shared.stopped.store(true, Ordering::SeqCst);
                });
            }
        }
    }

    /// Stops the node of the instance in the background.
    fn stop_instance_task(&mut self) {
        let Some(inst) = &self.inst else {
            return;
        };
        let shared = Arc::clone(&inst.shared);
        let node = lock(&shared.node).clone();
        match (node, self.rt.as_ref()) {
            (Some(node), Some(rt)) => {
                rt.spawn(async move {
                    node.stop().await;
                    shared.stopped.store(true, Ordering::SeqCst);
                });
            }
            _ => shared.stopped.store(true, Ordering::SeqCst),
        }
    }

    /// SL-12, SL-77: the seed is deleted from the counter. The app deletes
    /// what it sealed when the status says so.
    fn wipe_seed(&mut self) {
        if self.seed.take().is_some() {
            self.seed_wiped = true;
            self.notes
                .push("SL-77: the Lightning key was deleted from this counter".to_string());
        }
    }

    fn forget_plan(&mut self) {
        let mut book = lock(&self.shared.book);
        if book.plan.take().is_some() {
            if let Err(e) = self.shared.save(&book) {
                self.notes
                    .push(format!("SL-77: the book was not written: {e}"));
            }
        }
    }

    /// Whether a sale may be offered an invoice: the reason it may not.
    fn offer_blocked(&self) -> Option<Why> {
        let Some(inst) = self.inst.as_ref() else {
            return Some(self.blocked.clone().unwrap_or(Why::Starting));
        };
        if inst.phase != Phase::Serving {
            return Some(Why::Closing);
        }
        if lock(&inst.shared.node).is_some() {
            return None;
        }
        Some(lock(&inst.shared.failed).clone().unwrap_or(Why::Starting))
    }

    /// What a sale of `sats` shows of Lightning (SL-40 to SL-42). `Off`
    /// when this is not the Lightning counter. An invoice is asked for at the
    /// first call and read at the next.
    pub fn sale_view(&mut self, index: u32, sats: u64, now: u64) -> SaleLightning {
        if !self.named {
            return SaleLightning::Off;
        }
        if let Some(why) = self.offer_blocked() {
            return SaleLightning::Unavailable(crate::say::shopln_words(&why));
        }
        let ask = match self.slots.get(&index) {
            Some(slot) if slot.sats == sats => {
                matches!(&*lock(&slot.state), Slot::Off(_))
                    && now >= slot.asked_at.saturating_add(RETRY_OFFER_SECS)
            }
            _ => {
                self.slots.insert(
                    index,
                    SaleSlot {
                        sats,
                        state: Arc::new(Mutex::new(Slot::Pending)),
                        asked_at: now,
                    },
                );
                true
            }
        };
        if ask {
            self.ask_invoice(index, now);
        }
        // SL-41: an invoice that waits on a write the Box has not answered is
        // on-chain only for now, and shows when the Box answers.
        let stuck = self.slots.get(&index).is_some_and(|s| {
            now >= s.asked_at.saturating_add(WRITE_WAIT_SECS)
                && self
                    .node()
                    .is_some_and(|n| n.status().device.pending_writes > 0)
        });
        match self.slots.get(&index).map(|s| lock(&s.state)) {
            Some(g) => match &*g {
                Slot::Pending if stuck => SaleLightning::Unavailable(crate::say::shopln_words(
                    &Why::Node(OnChainReason::SecondDeviceAway),
                )),
                Slot::Pending => SaleLightning::Waiting,
                Slot::Ready(c) => SaleLightning::Invoice(c.clone()),
                Slot::Off(why) => SaleLightning::Unavailable(crate::say::shopln_words(why)),
            },
            None => SaleLightning::Waiting,
        }
    }

    fn ask_invoice(&mut self, index: u32, now: u64) {
        let Some(node) = self.node() else {
            return;
        };
        let Some(slot) = self.slots.get_mut(&index) else {
            return;
        };
        slot.asked_at = now;
        let state = Arc::clone(&slot.state);
        let sats = slot.sats;
        let shared = Arc::clone(&self.shared);
        let Some(rt) = self.rt.as_ref() else {
            return;
        };
        rt.spawn(async move {
            let msat = sats.saturating_mul(1000);
            let next = make_invoice(&node, &shared, index, msat, now).await;
            *lock(&state) = next;
        });
    }

    /// A sale is over: its invoice is no longer asked for or shown.
    pub fn sale_closed(&mut self, index: u32) {
        self.slots.remove(&index);
    }

    /// SL-21: the sales the node says are paid, after both copies of the
    /// state are stored. Each comes again at every call until
    /// [`ShopLn::recorded`] says the counter has it.
    pub fn take_paid(&mut self, now: u64) -> Vec<PaidSale> {
        let Some(node) = self.node() else {
            return Vec::new();
        };
        while let Some(p) = node.try_paid() {
            if !self
                .pending
                .iter()
                .any(|q| q.payment_hash == p.payment_hash)
            {
                self.pending.push(p);
            }
        }
        {
            let mut book = lock(&self.shared.book);
            book.prune(now);
        }
        let mut out = Vec::new();
        let mut unknown = Vec::new();
        {
            let book = lock(&self.shared.book);
            for p in &self.pending {
                match book.invoice(&p.payment_hash) {
                    Some(e) => out.push(PaidSale {
                        sale: e.sale,
                        facts: PaidFacts {
                            payment_hash: p.payment_hash,
                            amount_received_msat: p.amount_msat.saturating_add(e.opening_fee_msat),
                            settled_at: Some(p.received_unix),
                        },
                    }),
                    None => unknown.push(p.payment_hash),
                }
            }
        }
        for hash in unknown {
            self.notes.push(
                "SL-21: a Lightning payment was claimed for an invoice the counter has no sale \
                 for"
                .to_string(),
            );
            self.recorded(&hash);
        }
        out
    }

    /// The counter has recorded the sale of this payment: the node is told,
    /// so that the event is not given again, and the fee is counted (SL-21,
    /// SL-78).
    pub fn recorded(&mut self, hash: &[u8; 32]) {
        self.pending.retain(|p| &p.payment_hash != hash);
        {
            let mut book = lock(&self.shared.book);
            if let Some(i) = book.invoices.iter().position(|e| &e.payment_hash == hash) {
                let e = book.invoices.remove(i);
                book.opening_fees_paid_sats = book
                    .opening_fees_paid_sats
                    .saturating_add(e.opening_fee_msat / 1000);
                if let Err(e) = self.shared.save(&book) {
                    self.notes
                        .push(format!("SL-77: the book was not written: {e}"));
                }
            }
        }
        if let (Some(node), Some(rt)) = (self.node(), self.rt.as_ref()) {
            let hash = *hash;
            rt.spawn(async move {
                let _ = node.ack_paid(hash).await;
            });
        }
    }

    /// SL-78: the facts of the report that goes with a check-in. `None` when
    /// the settings do not name this counter.
    pub fn report(&self) -> Option<LightningReport> {
        if !self.named {
            return None;
        }
        let book = lock(&self.shared.book);
        let mut report = LightningReport {
            balance_sats: 0,
            receive_room_sats: 0,
            opening_fees_paid_sats: book.opening_fees_paid_sats,
            last_stored_at: 0,
            last_sweep_at: book.last_sweep_at,
            not_offered: self
                .offer_blocked()
                .map_or(NotOffered::Offered, |w| w.not_offered()),
        };
        if let Some(node) = self.node() {
            let status = node.status();
            report.balance_sats = node.balance_sat();
            report.receive_room_sats = status.room_msat / 1000;
            report.last_stored_at = status.device.last_stored_unix;
            report.not_offered = if status.device.state == DeviceState::Away {
                NotOffered::SecondDeviceAway
            } else if !status.chain_ok {
                NotOffered::ChainBehind
            } else if status.usable_channels == 0 && !status.provider_connected {
                NotOffered::ProviderSilent
            } else {
                NotOffered::Offered
            };
        }
        Some(report)
    }

    /// What the app reads (SL-41, SL-76, SL-77).
    pub fn status(&self) -> ShopLnStatus {
        let why = if self.named || self.inst.is_some() {
            self.offer_blocked()
        } else {
            None
        };
        ShopLnStatus {
            needs_seed: self.named && self.inst.is_none() && self.seed.is_none(),
            delete_seed: self.seed_wiped && self.seed.is_none(),
            why,
        }
    }

    /// Stops the node, as the counter stops. Waits for the library's last
    /// writes, a few seconds at most.
    pub fn stop(&mut self) {
        if let (Some(node), Some(rt)) = (self.node(), self.rt.as_ref()) {
            rt.block_on(async {
                let _ = tokio::time::timeout(Duration::from_secs(20), node.stop()).await;
            });
        }
        if let Some(rt) = self.rt.take() {
            rt.shutdown_background();
        }
        self.inst = None;
    }
}

impl Drop for ShopLn {
    fn drop(&mut self) {
        if let Some(rt) = self.rt.take() {
            rt.shutdown_background();
        }
    }
}

/// SL-77: whether the node being closed has nothing left to do: no channel is
/// open, no balance of a channel monitor is claimable or waits for a
/// confirmation, no spendable output is tracked, and every write has reached
/// the Box. Only then is the seed deleted.
pub fn close_done(
    channels: usize,
    claimable_balances: usize,
    tracked_outputs: usize,
    pending_writes: usize,
    device_reachable: bool,
) -> bool {
    channels == 0
        && claimable_balances == 0
        && tracked_outputs == 0
        && pending_writes == 0
        && device_reachable
}

/// Why a node that did not start did not (SL-23, SL-26, SL-60).
fn why_not_started(e: StartError) -> Why {
    match e {
        StartError::MainnetNotYet => Why::Mainnet,
        StartError::DeviceAway(_) => Why::Node(OnChainReason::SecondDeviceAway),
        StartError::Chain(_) | StartError::Filters(_) => Why::Node(OnChainReason::ChainBehind),
        other => Why::NotStarted(other.to_string()),
    }
}

/// An invoice for a sale, or the reason there is none (SL-29, SL-40 to SL-42,
/// SL-20): asked of the node, stored in the book before it is offered.
async fn make_invoice(node: &LnNode, shared: &Shared, sale: u32, msat: u64, now: u64) -> Slot {
    let quote = node.quote(msat).await;
    if let Receive::OnChainOnly(r) = &quote {
        return Slot::Off(Why::Node(*r));
    }
    let made = match node.invoice(msat, &quote, DESCRIPTION).await {
        Ok(made) => made,
        Err(InvoiceError::OnChainOnly(r)) => return Slot::Off(Why::Node(r)),
        Err(InvoiceError::NotStored) => return Slot::Off(Why::NotStored),
        Err(InvoiceError::Provider(_)) => return Slot::Off(Why::Node(OnChainReason::ProviderAway)),
        Err(InvoiceError::Library(e)) => return Slot::Off(Why::Library(e)),
    };
    let invoice = &made.invoice;
    let hash = {
        use bitcoin::hashes::Hash;
        invoice.payment_hash().to_byte_array()
    };
    let checked = Checked {
        bolt11: invoice.to_string().to_ascii_lowercase(),
        amount_msat: invoice.amount_milli_satoshis().unwrap_or(msat),
        payment_hash: hash,
        timestamp: invoice.duration_since_epoch().as_secs(),
    };
    // The invoice is kept before it is shown, so that its payment finds the
    // sale after a restart.
    let mut book = lock(&shared.book);
    book.prune(now);
    if book.invoices.len() >= MAX_INVOICES {
        return Slot::Off(Why::Library("too many invoices wait".to_string()));
    }
    book.invoices.push(InvoiceEntry {
        payment_hash: hash,
        sale,
        amount_msat: checked.amount_msat,
        opening_fee_msat: made.opening_fee_msat.unwrap_or(0),
        made_at: now,
    });
    if shared.save(&book).is_err() {
        book.invoices.pop();
        return Slot::Off(Why::NotStored);
    }
    Slot::Ready(checked)
}

/// The Box as the node's second device: Store messages sent straight to it on
/// a connection of their own (LK-82).
struct RelayDevice;

impl RelayDevice {
    /// The device, with the numbers of the Store count read from `dir` and
    /// stored there again whenever they change (LK-81).
    fn open(
        dir: &Path,
        identity: Identity,
        box_id: DeviceId,
        route: Route,
    ) -> Result<LinkDevice<RelayStore>, String> {
        fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        let inbox = match fs::read(dir.join(STORE_INBOX_FILE)) {
            Ok(b) => Inbox::from_bytes(&b).map_err(|e| e.to_string())?,
            Err(e) if e.kind() == io::ErrorKind::NotFound => Inbox::default(),
            Err(e) => return Err(e.to_string()),
        };
        let outbox = match fs::read(dir.join(STORE_OUTBOX_FILE)) {
            Ok(b) => Outbox::from_bytes(&b).map_err(|e| e.to_string())?,
            Err(e) if e.kind() == io::ErrorKind::NotFound => Outbox::default(),
            Err(e) => return Err(e.to_string()),
        };
        let persist = |file: &'static str| -> Persist {
            let dir = dir.to_path_buf();
            Arc::new(move |bytes| {
                write_whole(&dir, file, bytes).map_err(|e| DeviceError(e.to_string()))
            })
        };
        Ok(LinkDevice::new(
            identity,
            box_id,
            RelayStore {
                route,
                relay: box_id,
                cfg: ClientConfig::default(),
                conn: Arc::new(Mutex::new(None)),
            },
            (inbox, persist(STORE_INBOX_FILE)),
            (outbox, persist(STORE_OUTBOX_FILE)),
        ))
    }
}

/// Writes a file whole: temporary file, flush, rename, flush the folder.
fn write_whole(dir: &Path, file: &str, bytes: &[u8]) -> io::Result<()> {
    use std::io::Write;
    let temp = dir.join(format!("{file}.tmp"));
    let mut f = fs::File::create(&temp)?;
    f.write_all(bytes)?;
    f.sync_all()?;
    fs::rename(&temp, dir.join(file))?;
    fs::File::open(dir)?.sync_all()
}

/// The "store" request of LK-82 on a connection to the relay, kept between
/// calls. The relay closes an idle connection, so a call that fails on a
/// connection that was kept is made once more on a new one.
struct RelayStore {
    route: Route,
    relay: DeviceId,
    cfg: ClientConfig,
    conn: Arc<Mutex<Option<Client>>>,
}

impl StoreTransport for RelayStore {
    fn call(&self, sealed: Vec<u8>) -> DeviceFuture<Vec<u8>> {
        let conn = Arc::clone(&self.conn);
        let (route, relay, cfg) = (self.route.clone(), self.relay, self.cfg);
        Box::pin(async move {
            tokio::task::spawn_blocking(move || {
                let mut conn = lock(&conn);
                for attempt in 0..2 {
                    let kept = conn.is_some();
                    if conn.is_none() {
                        *conn = Some(
                            Client::connect(&route, Some(&relay), &cfg)
                                .map_err(|e| DeviceError(e.to_string()))?,
                        );
                    }
                    let Some(client) = conn.as_mut() else {
                        return Err(DeviceError("no connection".to_string()));
                    };
                    match client.store(&sealed) {
                        Ok(answer) => return Ok(answer),
                        Err(e) => {
                            *conn = None;
                            if !(kept && attempt == 0) {
                                return Err(DeviceError(e.to_string()));
                            }
                        }
                    }
                }
                Err(DeviceError("the Box did not answer".to_string()))
            })
            .await
            .map_err(|e| DeviceError(e.to_string()))?
        })
    }
}

// The second device is only ever the Box, over the link above.
const _: fn() = || {
    fn is_device<T: SecondDevice>() {}
    is_device::<LinkDevice<RelayStore>>();
};

#[cfg(test)]
mod tests {
    use super::*;
    use openbmt_core::relay::RelayRoute;
    use openbmt_core::settings::LIGHTNING_NODE_BYTES;
    use openbmt_core::shop_lightning::node_id;

    fn id(n: u8) -> Identity {
        Identity::from_secret_bytes(&[n; 32]).unwrap()
    }

    fn seed(n: u8) -> [u8; 64] {
        std::array::from_fn(|i| n.wrapping_add(i as u8))
    }

    fn sl() -> ShopLightning {
        ShopLightning {
            counter: id(3).device_id(),
            node_id: node_id(&seed(1)),
            second_device: id(8).device_id(),
            provider_node_id: node_id(&seed(99)),
            provider_addr: "127.0.0.1:9735".parse().unwrap(),
            cap_sats: 100_000,
            max_opening_fee_sats: 3_000,
        }
    }

    fn wallet() -> (String, u32) {
        use bitcoin::bip32::{DerivationPath, Xpriv, Xpub};
        use bitcoin::secp256k1::Secp256k1;
        use std::str::FromStr;
        let secp = Secp256k1::new();
        let master = Xpriv::new_master(Network::Bitcoin, &[5; 32]).unwrap();
        let account = master
            .derive_priv(&secp, &DerivationPath::from_str("m/84h/0h/0h").unwrap())
            .unwrap();
        let w = openbmt_core::wallet_single::SingleKeyWallet::new(
            master.fingerprint(&secp),
            0,
            Xpub::from_priv(&secp, &account),
        )
        .unwrap();
        (w.canonical_string().to_string(), 19)
    }

    fn hint(device: u8) -> RelayHint {
        RelayHint::new(
            RelayRoute::Direct("127.0.0.1:9".parse().unwrap()),
            id(device).device_id().to_bytes(),
        )
        .unwrap()
    }

    fn ln(
        network: Network,
        relay: Option<RelayHint>,
        wallet: Option<(String, u32)>,
        seed_n: Option<u8>,
    ) -> ShopLn {
        let dir = std::env::temp_dir().join(format!(
            "shopln-node-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        ShopLn::new(ShopLnArgs {
            me: id(3).device_id(),
            dir,
            network,
            driver: DriverConfig::new(network, 0),
            tor_on: false,
            wallet,
            config: ShopLnConfig {
                seed_record: seed_n.map(|n| SeedRecord::new(seed(n)).to_bytes()),
                identity: Box::new(|| Some(id(3))),
                relay,
                link_proxy: None,
            },
        })
    }

    fn cleanup(l: &ShopLn) {
        let _ = fs::remove_dir_all(l.dir.parent().unwrap_or(&l.dir));
    }

    #[test]
    fn sl_60_the_counter_does_not_start_the_node_on_the_real_network() {
        let l = ln(Network::Bitcoin, Some(hint(8)), Some(wallet()), Some(1));
        assert_eq!(l.prepare(&sl()).err(), Some(Why::Mainnet));
        cleanup(&l);
    }

    #[test]
    fn sl_72_a_node_whose_seed_does_not_give_the_pinned_id_does_not_start() {
        let l = ln(Network::Regtest, Some(hint(8)), Some(wallet()), Some(2));
        assert_eq!(l.prepare(&sl()).err(), Some(Why::NoSeed));
        let none = ln(Network::Regtest, Some(hint(8)), Some(wallet()), None);
        assert_eq!(none.prepare(&sl()).err(), Some(Why::NoSeed));
        cleanup(&l);
        cleanup(&none);
    }

    #[test]
    fn sl_73_the_relay_must_be_the_box_named_as_the_second_device() {
        // The relay is device 9, the second device is 8.
        let l = ln(Network::Regtest, Some(hint(9)), Some(wallet()), Some(1));
        assert_eq!(l.prepare(&sl()).err(), Some(Why::NotTheBox));
        let none = ln(Network::Regtest, None, Some(wallet()), Some(1));
        assert_eq!(none.prepare(&sl()).err(), Some(Why::NotTheBox));
        cleanup(&l);
        cleanup(&none);
    }

    #[test]
    fn sl_33_a_counter_with_no_description_of_the_wallet_does_not_start() {
        let l = ln(Network::Regtest, Some(hint(8)), None, Some(1));
        assert_eq!(l.prepare(&sl()).err(), Some(Why::NoWallet));
        cleanup(&l);
    }

    #[test]
    fn sl_32_a_relay_through_tor_with_no_proxy_does_not_start() {
        let onion = RelayHint::new(
            RelayRoute::Onion {
                name: "a".repeat(56),
                port: 80,
            },
            id(8).device_id().to_bytes(),
        );
        if let Ok(onion) = onion {
            let l = ln(Network::Regtest, Some(onion), Some(wallet()), Some(1));
            assert_eq!(l.prepare(&sl()).err(), Some(Why::NoRoute));
            cleanup(&l);
        }
    }

    #[test]
    fn sl_77_a_start_that_can_be_made_is_made_with_the_settings_terms_in_millisatoshis() {
        let l = ln(Network::Regtest, Some(hint(8)), Some(wallet()), Some(1));
        let c = l.prepare(&sl()).unwrap();
        assert_eq!(c.max_opening_fee_msat, 3_000_000);
        assert_eq!(c.sweep_index, 19);
        assert_eq!(c.provider.addr, sl().provider_addr);
        assert_eq!(c.provider.node_id.serialize(), sl().provider_node_id);
        assert_eq!(c.network, Network::Regtest);
        assert!(c.tor.is_none());
        assert_eq!(c.seed.0, seed(1));
        assert!(matches!(c.chain, ChainSource::Filters(_)));
        let _ = LIGHTNING_NODE_BYTES;
        cleanup(&l);
    }

    #[test]
    fn sl_77_a_node_is_closed_only_when_nothing_is_left() {
        assert!(close_done(0, 0, 0, 0, true));
        assert!(!close_done(1, 0, 0, 0, true), "a channel is open");
        assert!(!close_done(0, 1, 0, 0, true), "a balance waits");
        assert!(!close_done(0, 0, 1, 0, true), "an output is tracked");
        assert!(
            !close_done(0, 0, 0, 1, true),
            "a write has not reached the Box"
        );
        assert!(!close_done(0, 0, 0, 0, false), "the Box is away");
    }
}
