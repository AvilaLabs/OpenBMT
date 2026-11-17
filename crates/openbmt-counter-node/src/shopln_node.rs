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
//!   every [`SWEEP_EVERY_SECS`] seconds. The address is an index of the
//!   shop's own wallet that the counter's ranges give to Shop Lightning the
//!   way they give one to a sale ([`SweepSource::take_index`]): written
//!   before it is used, never given to a sale and never used by two
//!   transactions. The book keeps the current one for the node's output
//!   sweeper and the next sweep or close, and a new one is taken after a
//!   transaction has paid it. The wallet is read from the counter at every
//!   pump; when it changes, the node is stopped and started again with the
//!   new one. A counter paired with the Key app holds no description of the
//!   wallet, so Shop Lightning does not start on it ([`Why::NoWallet`]); with
//!   no free index in any range it does not sweep ([`Why::NoSweepAddress`]).
//! - A channel the provider opens may be used before its funding transaction
//!   is confirmed (`zero_conf`): the provider funds it, and the shop holds
//!   nothing in it until a payment comes.
//! - The invoices are kept in the book, in plain, until their payment is
//!   acknowledged, so that a paid event after a restart finds its sale; when
//!   the sale is over, cancelled or quoted again they are taken out and the
//!   node is told to fail a payment to them back. A paid event of an invoice
//!   that is not in the book is kept in the book as a payment no sale took,
//!   and only then acknowledged.
//! - What a sale is credited is what arrived plus the fee the provider
//!   skimmed off that payment (SL-29): the customer paid the whole invoice,
//!   and the shop bears the fee. A payment that opened no channel has no
//!   skim.
//! - A book that is there and does not read is an error, not an empty book:
//!   the node is not started and the seed is kept. The seed is deleted only
//!   when the node, running, reports that nothing is left (no channel, no
//!   claimable balance, no tracked output, no write waiting).
//! - The node's folder is held by a lock file from before the node is built
//!   until it has stopped, so that two nodes never run on one seed.

use std::collections::HashMap;
use std::fs;
use std::io;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use bitcoin::hashes::{sha256, Hash};
use bitcoin::secp256k1::PublicKey;
use bitcoin::Network;
use openbmt_core::identity::{DeviceId, Identity};
use openbmt_core::relay::RelayHint;
use openbmt_core::settings::{Settings, ShopLightning};
use openbmt_counter::PaidFunding;
use openbmt_lightning::bolt11::Checked;
use openbmt_lightning::PaidFacts;
use openbmt_lightning_node::bip85::NodeSeed;
use openbmt_lightning_node::cap::sweep_amount_sat;
use openbmt_lightning_node::chain::ChainSource;
use openbmt_lightning_node::device::{DeviceError, DeviceFuture, DeviceState, SecondDevice};
use openbmt_lightning_node::device_link::{LinkDevice, Persist, StoreTransport};
use openbmt_lightning_node::filters::FilterConfig;
use openbmt_lightning_node::funding::{FundingEvent, FundingState, Outcome};
use openbmt_lightning_node::node::{
    InvoiceError, Node as LnNode, NodeConfig as LnConfig, Paid, Receive, StartError,
    MAINNET_ALLOWED,
};
use openbmt_lightning_node::provider::ProviderConfig;
use openbmt_lightning_node::room::OnChainReason;
use openbmt_lightning_node::state::StartFrom;
use openbmt_link::numbers::{STORE_INBOX_FILE, STORE_OUTBOX_FILE};
use openbmt_link::shopln::{
    LightningReport, Lsps1Offer, NotOffered, OrderState, OrderWhy, ReceivingBackupState,
    ReceivingSnapshot,
};
use openbmt_link::{Client, ClientConfig, Inbox, Outbox, Route};
use openbmt_net_io::DriverConfig;
use tokio::runtime::{Builder, Runtime};

use crate::lightning::SaleLightning;
use crate::relaylink::link_route;
use crate::shopln::{
    role, take_seed, Book, InvoiceEntry, Role, SeedNotTaken, SeedRecord, SweepAddress, Unmatched,
    Why, MAX_INVOICES,
};

/// How often a node that did not start is started again, in seconds.
pub const RESTART_SECS: u64 = 10;
/// Seconds after a close was first asked of a channel that is still listed
/// before the wind-down closes it by force (SL-36). To tune.
pub const FORCE_CLOSE_AFTER_SECS: u64 = 3600;
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
/// The file whose lock holds the node's folder.
const FOLDER_LOCK_FILE: &str = "node.lock";

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
    /// From the app.
    pub config: ShopLnConfig,
}

/// Where Shop Lightning gets the shop's wallet and the addresses of its own
/// transactions (SL-33). The counter implements it over its pinned wallet and
/// its ranges; it is read at every use, so a switch of wallet is seen.
pub trait SweepSource {
    /// The description of the wallet the counter pays into now, if it holds
    /// one.
    fn wallet(&self) -> Option<String>;
    /// The next index of the ranges, taken as a sale takes its address: it is
    /// written before it is handed back and is never given again. `Ok(None)`
    /// when no range has a free index.
    fn take_index(&mut self) -> Result<Option<u32>, String>;
}

/// What a close of channels did, as the wind-down reads it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct CloseReport {
    begun: usize,
    /// Channels already shutting down, left as they are.
    already: usize,
    /// The channels refused, and why.
    failed: Vec<([u8; 32], String)>,
}

/// The channels to close by force now: those still `listed` whose close was
/// first asked at least [`FORCE_CLOSE_AFTER_SECS`] ago. A listed channel not
/// yet in `first_asked` is entered at `now`; one no longer listed is dropped.
fn channels_to_force(
    first_asked: &mut HashMap<[u8; 32], u64>,
    listed: &[[u8; 32]],
    now: u64,
) -> Vec<[u8; 32]> {
    first_asked.retain(|id, _| listed.contains(id));
    let mut due = Vec::new();
    for id in listed {
        let at = *first_asked.entry(*id).or_insert(now);
        if now >= at.saturating_add(FORCE_CLOSE_AFTER_SECS) && !due.contains(id) {
            due.push(*id);
        }
    }
    due
}

/// A channel id as text, for the notes.
fn id_text(id: &[u8; 32]) -> String {
    id.iter().map(|b| format!("{b:02x}")).collect()
}

/// What the sweep and the close need of a running node.
trait NodeOps {
    fn balance_sat(&self) -> u64;
    fn sweep(&self, index: u32, amount_sat: u64) -> Result<(), String>;
    fn close_all(&self, index: u32) -> Result<CloseReport, String>;
    /// The ids of the channels the node lists.
    fn channel_ids(&self) -> Vec<[u8; 32]>;
    /// Closes by force the listed channels among `ids`.
    fn force_close(&self, ids: &[[u8; 32]]) -> Result<CloseReport, String>;
    fn sweeper_spends(&self) -> Vec<[u8; 32]>;
    fn sweeper_index(&self) -> u32;
    fn set_sweeper_index(&self, index: u32, rt: &Runtime);
}

impl NodeOps for Arc<LnNode> {
    fn balance_sat(&self) -> u64 {
        LnNode::balance_sat(self)
    }
    fn sweep(&self, index: u32, amount_sat: u64) -> Result<(), String> {
        LnNode::sweep(self, index, amount_sat)
    }
    fn close_all(&self, index: u32) -> Result<CloseReport, String> {
        let c = LnNode::close_all(self, index)?;
        Ok(CloseReport {
            begun: c.begun,
            already: c.already,
            failed: c.failed.into_iter().map(|(id, e)| (id.0, e)).collect(),
        })
    }
    fn channel_ids(&self) -> Vec<[u8; 32]> {
        LnNode::channel_ids(self)
    }
    fn force_close(&self, ids: &[[u8; 32]]) -> Result<CloseReport, String> {
        let f = LnNode::force_close(self, ids)?;
        Ok(CloseReport {
            begun: f.begun,
            already: 0,
            failed: f.failed.into_iter().map(|(id, e)| (id.0, e)).collect(),
        })
    }
    fn sweeper_spends(&self) -> Vec<[u8; 32]> {
        LnNode::sweeper_spends(self)
    }
    fn sweeper_index(&self) -> u32 {
        LnNode::sweep_index(self)
    }
    fn set_sweeper_index(&self, index: u32, rt: &Runtime) {
        let node = Arc::clone(self);
        rt.spawn(async move {
            let _ = node.set_sweep_index(index).await;
        });
    }
}

fn wallet_hash(wallet: &str) -> [u8; 32] {
    sha256::Hash::hash(wallet.as_bytes()).to_byte_array()
}

/// The lock on the node's folder: held from before the node is built until
/// it is fully stopped, so that no second node runs on the same seed.
struct FolderLock {
    _file: fs::File,
    #[cfg(test)]
    probe: Option<Arc<tests::Probe>>,
}

#[cfg(test)]
impl Drop for FolderLock {
    fn drop(&mut self) {
        if let Some(p) = &self.probe {
            p.leave();
        }
    }
}

/// Takes the exclusive lock on `path`, waiting for whoever holds it until
/// `give_up` is set. `None` when given up.
fn take_folder_lock(path: &Path, give_up: &AtomicBool) -> Result<Option<FolderLock>, String> {
    let file = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(path)
        .map_err(|e| format!("the node's folder lock: {e}"))?;
    loop {
        match file.try_lock() {
            Ok(()) => {
                return Ok(Some(FolderLock {
                    _file: file,
                    #[cfg(test)]
                    probe: None,
                }))
            }
            Err(fs::TryLockError::WouldBlock) => {
                if give_up.load(Ordering::SeqCst) {
                    return Ok(None);
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(fs::TryLockError::Error(e)) => return Err(format!("the node's folder lock: {e}")),
        }
    }
}

/// What a sale's invoice is.
enum Slot {
    Pending,
    /// The invoice, and whether a payment of it is final at once: it comes
    /// over channels that are open with their fundings confirmed (SL-40),
    /// not through a channel the provider opens for it.
    Ready(Checked, bool),
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
    /// A start is in progress: waiting for the folder or building the node.
    starting: AtomicBool,
    /// A stop wants the start to end without waiting for the folder any
    /// longer.
    give_up: AtomicBool,
    /// The lock on the node's folder, held until the node has stopped.
    folder: Mutex<Option<FolderLock>>,
}

impl InstShared {
    fn new() -> Self {
        Self {
            node: Mutex::new(None),
            failed: Mutex::new(None),
            stopped: AtomicBool::new(false),
            starting: AtomicBool::new(false),
            give_up: AtomicBool::new(false),
            folder: Mutex::new(None),
        }
    }

    /// The node is fully stopped, or never was: the folder is let go, and
    /// only then is the instance said to be stopped.
    fn finish_stop(&self) {
        lock(&self.folder).take();
        self.stopped.store(true, Ordering::SeqCst);
    }
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
    /// When a close was first asked of each channel, in this instance only:
    /// after a restart the clock starts again.
    close_first_asked: HashMap<[u8; 32], u64>,
    /// The last time a sweep was tried.
    sweep_tried_at: Option<u64>,
    /// The hash of the wallet description the node was started with.
    wallet: [u8; 32],
    /// The transactions of the node's output sweeper seen when the sweep
    /// address was taken; a new one has paid the address.
    spends_seen: Option<Vec<[u8; 32]>>,
    /// The index the node's output sweeper was last told to pay to.
    pointed: Option<u32>,
    /// The node has been told to stop for good, to delete the seed.
    finishing: bool,
}

/// A paid sale for the counter to record (SL-21).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaidSale {
    /// The sale.
    pub sale: u32,
    /// What the counter is told, in the form its Lightning path takes.
    pub facts: PaidFacts,
    /// `None` for a payment through channels whose funding was confirmed at
    /// the claim; otherwise what is known of the fundings of the channels it
    /// came through (SL-40, SL-27): all confirmed since, some still not, or
    /// one lost.
    pub funding: Option<PaidFunding>,
}

/// The provider's LSPS1 and the last order, as the report says them (SL-28,
/// SL-45, LK-84). With the buying stopped (a paid order brought no channel),
/// the order is said failed for that cause.
fn order_facts(
    offered: openbmt_lightning_node::lsps1::Offer,
    order: Option<&openbmt_lightning_node::lsps1::OrderRecord>,
    stopped: bool,
) -> (Lsps1Offer, OrderState, OrderWhy, u64) {
    use openbmt_lightning_node::lsps1 as ln;
    let offer = match offered {
        ln::Offer::Unknown => Lsps1Offer::Unknown,
        ln::Offer::Offered => Lsps1Offer::Offered,
        ln::Offer::NotOffered => Lsps1Offer::NotOffered,
    };
    let Some(o) = order else {
        return if stopped {
            (offer, OrderState::Failed, OrderWhy::Stopped, 0)
        } else {
            (offer, OrderState::None, OrderWhy::None, 0)
        };
    };
    if stopped {
        return (offer, OrderState::Failed, OrderWhy::Stopped, o.fee_sat);
    }
    let state = match o.state {
        ln::OrderState::Open => OrderState::Open,
        ln::OrderState::Paid => OrderState::Paid,
        ln::OrderState::Done => OrderState::Done,
        ln::OrderState::Failed => OrderState::Failed,
    };
    let why = match o.why {
        ln::OrderWhy::None => OrderWhy::None,
        ln::OrderWhy::FeeAboveMax => OrderWhy::FeeAboveMax,
        ln::OrderWhy::CannotPay => OrderWhy::CannotPay,
        ln::OrderWhy::Refused => OrderWhy::Refused,
        ln::OrderWhy::Expired => OrderWhy::Expired,
        ln::OrderWhy::PaymentFailed => OrderWhy::PaymentFailed,
        ln::OrderWhy::Refunded => OrderWhy::Refunded,
        ln::OrderWhy::NotPossible => OrderWhy::NotPossible,
        ln::OrderWhy::NoChannel => OrderWhy::NoChannel,
    };
    (offer, state, why, o.fee_sat)
}

/// Keep the serialized backup label consistent with the exact counters
/// serialized beside it. The live assessment can race with these separate
/// observations, so it supplies capacity only, never this label.
fn receiving_backup_state(
    node_running: bool,
    device_state: DeviceState,
    strict_pending_writes: u32,
    background_pending_writes: u32,
    last_device_store_unix: u64,
) -> ReceivingBackupState {
    if !node_running {
        ReceivingBackupState::Unknown
    } else if device_state == DeviceState::Away {
        ReceivingBackupState::Away
    } else if strict_pending_writes != 0
        || background_pending_writes != 0
        || last_device_store_unix == 0
    {
        ReceivingBackupState::Behind
    } else {
        ReceivingBackupState::CaughtUp
    }
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
    /// The sales paid through a channel whose funding was lost, rising
    /// (SL-27): the manager is told.
    pub funding_lost: Vec<u32>,
}

/// Shop Lightning at the counter.
pub struct ShopLn {
    me: DeviceId,
    network: Network,
    driver: DriverConfig,
    tor_on: bool,
    config: ShopLnConfig,
    shared: Arc<Shared>,
    /// The book is there and did not read: nothing is started or written
    /// until it does, and the seed is kept.
    book_error: Option<String>,
    /// Why the node does not sweep, when it does not.
    sweep_blocked: Option<Why>,
    seed: Option<SeedRecord>,
    seed_wiped: bool,
    inst: Option<Instance>,
    rt: Option<Runtime>,
    slots: HashMap<u32, SaleSlot>,
    pending: Vec<Paid>,
    /// SL-27, SL-40: what the node said of the fundings of the channels
    /// payments came through, not yet acknowledged to it.
    funding_events: Vec<FundingEvent>,
    /// The confirmations a funding needs to count as confirmed: the
    /// counter's own (SL-40), read from its settings at every pump and given
    /// to the node at its start.
    confirm_depth: u32,
    notes: Vec<String>,
    /// What the node would say of a funding named in a paid event.
    #[cfg(test)]
    node_funding_states: HashMap<[u8; 32], FundingState>,
    /// Whether the settings in force name this counter (SL-70).
    named: bool,
    /// Why no instance runs while the settings name this counter, or while a
    /// node is to be closed.
    blocked: Option<Why>,
    /// When an instance was last tried, so that one that cannot be made is
    /// not tried at every pump.
    last_try: Option<u64>,
    dir: PathBuf,
    #[cfg(test)]
    probe: Option<Arc<tests::Probe>>,
    #[cfg(test)]
    start_delay: Duration,
}

impl ShopLn {
    /// Reads the book and the seed record the app handed over. A seed record
    /// that does not read is noted and left out. A book that is there and
    /// does not read is an error state: no node is started and the seed is
    /// kept.
    pub fn new(mut args: ShopLnArgs) -> Self {
        let dir = args.dir.join("shop-lightning");
        let mut notes = Vec::new();
        let (book, book_error) = match Book::load(&dir) {
            Ok(book) => (book, None),
            Err(e) => {
                notes.push(format!("SL-77: {e}"));
                (Book::default(), Some(e))
            }
        };
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
            config: args.config,
            shared: Arc::new(Shared {
                dir: dir.clone(),
                book: Mutex::new(book),
            }),
            book_error,
            sweep_blocked: None,
            seed,
            seed_wiped: false,
            inst: None,
            rt: None,
            slots: HashMap::new(),
            pending: Vec::new(),
            funding_events: Vec::new(),
            confirm_depth: 1,
            #[cfg(test)]
            node_funding_states: HashMap::new(),
            notes,
            named: false,
            blocked: None,
            last_try: None,
            dir,
            #[cfg(test)]
            probe: None,
            #[cfg(test)]
            start_delay: Duration::ZERO,
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
        src: &mut dyn SweepSource,
    ) -> Result<Vec<u8>, SeedNotTaken> {
        let record = take_seed(text, settings, &self.me, self.seed.is_some())?;
        let bytes = record.to_bytes();
        self.seed = Some(record);
        self.seed_wiped = false;
        self.last_try = None;
        self.reconcile(settings, now, src);
        Ok(bytes)
    }

    /// SL-70, SL-72, SL-77: bring the instance to what the settings in force
    /// say. Called at every pump; it does no waiting.
    pub fn reconcile(&mut self, settings: &Settings, now: u64, src: &mut dyn SweepSource) {
        let want = match role(settings, &self.me) {
            Role::Here(sl) => Some(*sl),
            _ => None,
        };
        self.named = want.is_some();
        self.confirm_depth = u32::from(settings.min_confirmations).max(1);
        // SL-33: the wallet is read from the counter at every pump. A node
        // that runs with another wallet is stopped and started again.
        let wallet = src.wallet().map(|w| wallet_hash(&w));
        let wallet_changed = |inst: &Instance| wallet.is_some_and(|w| w != inst.wallet);
        enum Do {
            Nothing,
            Restart,
            Close,
        }
        let act = match (&self.inst, want) {
            (Some(inst), Some(w))
                if inst.phase == Phase::Serving && w == inst.sl && !wallet_changed(inst) =>
            {
                Do::Nothing
            }
            (Some(inst), Some(w))
                if inst.phase == Phase::Serving
                    && w.counter == inst.sl.counter
                    && w.node_id == inst.sl.node_id =>
            {
                // The same node with other terms or another wallet: stop it,
                // start it again.
                Do::Restart
            }
            // SL-77: off, or another counter, or another node.
            (Some(inst), _) if inst.phase == Phase::Serving => Do::Close,
            (Some(inst), _) if inst.phase == Phase::Closing && wallet_changed(inst) => Do::Restart,
            _ => Do::Nothing,
        };
        match act {
            Do::Nothing => {}
            Do::Restart => {
                let closing = self
                    .inst
                    .as_ref()
                    .is_some_and(|i| i.phase == Phase::Closing);
                if let Some(inst) = &mut self.inst {
                    inst.phase = Phase::Stopping;
                }
                if closing {
                    self.slots.clear();
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
        self.advance(now, src);
        if self.inst.is_none() {
            self.start_from_settings(want, now, src);
        }
    }

    /// The part of a reconcile that moves a running instance on.
    fn advance(&mut self, now: u64, src: &mut dyn SweepSource) {
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
                self.retry_start(now, src);
                let Some(node) = self.node() else {
                    return;
                };
                match self.inst.as_ref().map(|i| i.phase) {
                    Some(Phase::Serving) => self.sweep(&node, now, src),
                    Some(Phase::Closing) => self.close(&node, now, src),
                    _ => {}
                }
            }
        }
    }

    /// Starts again a node that did not start, after [`RESTART_SECS`].
    fn retry_start(&mut self, now: u64, src: &mut dyn SweepSource) {
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
        match self.prepare(&sl, src) {
            Ok((config, wallet)) => {
                if let Some(inst) = &mut self.inst {
                    inst.wallet = wallet;
                }
                self.spawn_start(config, shared);
            }
            Err(why) => {
                *lock(&shared.failed) = Some(why);
            }
        }
    }

    /// With no instance: make one, or clean up what a finished one left.
    fn start_from_settings(
        &mut self,
        want: Option<ShopLightning>,
        now: u64,
        src: &mut dyn SweepSource,
    ) {
        // A book that did not read is read again from time to time; until it
        // does, nothing is started and the seed is kept.
        if self.book_error.is_some() {
            if self
                .last_try
                .is_some_and(|t| now < t.saturating_add(RESTART_SECS))
            {
                return;
            }
            self.last_try = Some(now);
            match Book::load(&self.dir) {
                Ok(book) => {
                    *lock(&self.shared.book) = book;
                    self.book_error = None;
                    self.last_try = None;
                }
                Err(e) => {
                    self.blocked = Some(Why::NotStarted(e.clone()));
                    self.book_error = Some(e);
                    return;
                }
            }
        }
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
                // closed first (SL-13, SL-77). Its seed is deleted only when
                // the running node has nothing left, never because no plan
                // is kept.
                Some(seed) => Some((plan.unwrap_or_else(|| self.plan_of(seed)), Phase::Closing)),
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
            // A seed with no plan: the book may have been lost with channels
            // open, so a node is started to find out what is left, and the
            // seed is deleted only when that node reports nothing.
            (None, None) => self
                .seed
                .as_ref()
                .map(|seed| (self.plan_of(seed), Phase::Closing)),
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
        self.begin(sl, phase, src);
    }

    /// The terms to start a node of `seed` from when the book holds none:
    /// enough to run it and close its channels. The provider is not
    /// reachable on purpose (the discard port of this machine); nothing is
    /// opened or received on such a node.
    fn plan_of(&self, seed: &SeedRecord) -> ShopLightning {
        let second_device = self
            .config
            .relay
            .as_ref()
            .and_then(|h| h.device())
            .unwrap_or(self.me);
        ShopLightning {
            counter: self.me,
            node_id: seed.node_id(),
            second_device,
            provider_node_id: seed.node_id(),
            provider_addr: SocketAddr::from(([127, 0, 0, 1], 9)),
            cap_sats: 0,
            max_opening_fee_sats: 0,
        }
    }

    /// Makes an instance of `sl` and starts its node. A start that cannot be
    /// made leaves the reason in `blocked` and no instance.
    fn begin(&mut self, sl: ShopLightning, phase: Phase, src: &mut dyn SweepSource) {
        let (config, wallet) = match self.prepare(&sl, src) {
            Ok(prepared) => prepared,
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
        let shared = Arc::new(InstShared::new());
        self.spawn_start(config, Arc::clone(&shared));
        self.inst = Some(Instance {
            sl,
            phase,
            shared,
            failed_at: None,
            close_begun_at: None,
            close_first_asked: HashMap::new(),
            sweep_tried_at: None,
            wallet,
            spends_seen: None,
            pointed: None,
            finishing: false,
        });
    }

    /// The address index the sweeps, the closes and the node's output sweeper
    /// pay to now (SL-33): the one the book keeps for this wallet, or a new
    /// one from the counter's ranges, written to the book before it is used.
    fn sweep_address(&mut self, src: &mut dyn SweepSource) -> Result<(String, u32), Why> {
        let wallet = src.wallet().ok_or(Why::NoWallet)?;
        let hash = wallet_hash(&wallet);
        let mut book = lock(&self.shared.book);
        if let Some(a) = book.sweep.filter(|a| a.wallet == hash) {
            return Ok((wallet, a.index));
        }
        let index = match src.take_index() {
            Ok(Some(index)) => index,
            Ok(None) => return Err(Why::NoSweepAddress),
            Err(e) => return Err(Why::NotStarted(e)),
        };
        book.sweep = Some(SweepAddress {
            wallet: hash,
            index,
        });
        if let Err(e) = self.shared.save(&book) {
            // Not written: the index is not used, and a new one is taken at
            // the next try.
            book.sweep = None;
            self.notes
                .push(format!("SL-77: the book was not written: {e}"));
            return Err(Why::NotStarted(e.to_string()));
        }
        Ok((wallet, index))
    }

    /// A transaction has paid the sweep address: the book moves on to a new
    /// one, and the node's output sweeper is told of it.
    fn address_paid(&mut self, ops: &dyn NodeOps, src: &mut dyn SweepSource) {
        {
            let mut book = lock(&self.shared.book);
            book.sweep = None;
            if let Err(e) = self.shared.save(&book) {
                self.notes
                    .push(format!("SL-77: the book was not written: {e}"));
            }
        }
        if let Some(inst) = &mut self.inst {
            inst.spends_seen = Some(ops.sweeper_spends());
        }
        self.point_sweeper(ops, src);
    }

    /// The node's output sweeper pays to the book's sweep address. A
    /// transaction of the sweeper that was not there when the address was
    /// taken has paid it, and the book moves on.
    fn point_sweeper(&mut self, ops: &dyn NodeOps, src: &mut dyn SweepSource) {
        let spends = ops.sweeper_spends();
        let paid = match self.inst.as_mut() {
            Some(inst) => match &inst.spends_seen {
                Some(seen) => spends.iter().any(|t| !seen.contains(t)),
                None => {
                    inst.spends_seen = Some(spends.clone());
                    false
                }
            },
            None => return,
        };
        if paid {
            // Taken first, so that the paid address is not reused.
            let mut book = lock(&self.shared.book);
            book.sweep = None;
            if let Err(e) = self.shared.save(&book) {
                self.notes
                    .push(format!("SL-77: the book was not written: {e}"));
            }
            drop(book);
            if let Some(inst) = &mut self.inst {
                inst.spends_seen = Some(spends);
            }
        }
        match self.sweep_address(src) {
            Ok((_, index)) => {
                self.sweep_blocked = None;
                let Some(rt) = self.rt.as_ref() else {
                    return;
                };
                let Some(inst) = &mut self.inst else {
                    return;
                };
                if ops.sweeper_index() == index {
                    inst.pointed = Some(index);
                } else if inst.pointed != Some(index) {
                    inst.pointed = Some(index);
                    ops.set_sweeper_index(index, rt);
                }
            }
            Err(why) => self.sweep_blocked = Some(why),
        }
    }

    /// The checks of SL-60, SL-73 and SL-33, and the node's configuration:
    /// with the hash of the wallet it was made for. `Err` is the reason the
    /// node is not started.
    fn prepare(
        &mut self,
        sl: &ShopLightning,
        src: &mut dyn SweepSource,
    ) -> Result<(LnConfig, [u8; 32]), Why> {
        if self.network == Network::Bitcoin && !MAINNET_ALLOWED {
            return Err(Why::Mainnet);
        }
        let seed = self.seed.as_ref().ok_or(Why::NoSeed)?;
        if seed.node_id() != sl.node_id {
            return Err(Why::NoSeed);
        }
        if src.wallet().is_none() {
            return Err(Why::NoWallet);
        }
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
        // Last, so that an index is not taken for a node that is not
        // started.
        let (wallet, sweep_index) = self.sweep_address(src)?;
        self.sweep_blocked = None;
        let seed = self.seed.as_ref().ok_or(Why::NoSeed)?;
        let hash = wallet_hash(&wallet);
        Ok((
            LnConfig {
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
                cap_sat: sl.cap_sats,
                start_from: StartFrom::Newer,
                wallet,
                sweep_index,
                confirm_depth: self.confirm_depth,
            },
            hash,
        ))
    }

    /// Starts the node as a task. The node's folder is locked first, and the
    /// lock is kept until the node has stopped.
    fn spawn_start(&mut self, config: LnConfig, shared: Arc<InstShared>) {
        let Some(handle) = self.rt().map(|rt| rt.handle().clone()) else {
            *lock(&shared.failed) = Some(Why::NotStarted("no runtime".to_string()));
            return;
        };
        let lock_path = self.dir.join(FOLDER_LOCK_FILE);
        #[cfg(test)]
        let (probe, delay) = (self.probe.clone(), self.start_delay);
        shared.starting.store(true, Ordering::SeqCst);
        // The library's start is a future that cannot move between threads,
        // so a thread of its own drives it on the runtime.
        let spawned = std::thread::Builder::new()
            .name("shop-lightning-start".to_string())
            .spawn({
                let shared = Arc::clone(&shared);
                move || {
                    match take_folder_lock(&lock_path, &shared.give_up) {
                        Ok(Some(mut held)) => {
                            #[cfg(test)]
                            {
                                if let Some(p) = &probe {
                                    p.enter();
                                    held.probe = Some(Arc::clone(p));
                                }
                                std::thread::sleep(delay);
                            }
                            #[cfg(not(test))]
                            let _ = &mut held;
                            *lock(&shared.folder) = Some(held);
                        }
                        Ok(None) => {
                            *lock(&shared.failed) =
                                Some(Why::NotStarted("the node's folder is in use".to_string()));
                            shared.starting.store(false, Ordering::SeqCst);
                            return;
                        }
                        Err(e) => {
                            *lock(&shared.failed) = Some(Why::NotStarted(e));
                            shared.starting.store(false, Ordering::SeqCst);
                            return;
                        }
                    }
                    match handle.block_on(LnNode::start(config)) {
                        Ok(node) => *lock(&shared.node) = Some(Arc::new(node)),
                        Err(e) => {
                            // Nothing runs: the folder is let go for the next
                            // start.
                            lock(&shared.folder).take();
                            *lock(&shared.failed) = Some(why_not_started(e));
                        }
                    }
                    shared.starting.store(false, Ordering::SeqCst);
                }
            });
        if let Err(e) = spawned {
            shared.starting.store(false, Ordering::SeqCst);
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
    fn sweep(&mut self, ops: &dyn NodeOps, now: u64, src: &mut dyn SweepSource) {
        self.point_sweeper(ops, src);
        let Some(inst) = &mut self.inst else {
            return;
        };
        if inst
            .sweep_tried_at
            .is_some_and(|at| now < at.saturating_add(SWEEP_EVERY_SECS))
        {
            return;
        }
        let Some(amount) = sweep_amount_sat(ops.balance_sat(), inst.sl.cap_sats) else {
            return;
        };
        inst.sweep_tried_at = Some(now);
        // Each sweep pays an address of its own, and none is a sale's.
        let index = match self.sweep_address(src) {
            Ok((_, index)) => index,
            Err(why) => {
                self.notes.push(format!("SL-33: no sweep: {why:?}"));
                self.sweep_blocked = Some(why);
                return;
            }
        };
        match ops.sweep(index, amount) {
            Ok(()) => {
                {
                    let mut book = lock(&self.shared.book);
                    book.last_sweep_at = now;
                    if let Err(e) = self.shared.save(&book) {
                        self.notes
                            .push(format!("SL-77: the book was not written: {e}"));
                    }
                }
                self.address_paid(ops, src);
            }
            Err(e) => self.notes.push(format!("SL-33: no sweep: {e}")),
        }
    }

    /// SL-36, SL-77: close every channel, and when none is left, nothing
    /// waits to be swept and every write has reached the Box, delete the
    /// seed. The node must be running and report this itself.
    fn close(&mut self, node: &Arc<LnNode>, now: u64, src: &mut dyn SweepSource) {
        self.point_sweeper(node, src);
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
            self.close_retry(node, now, src);
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
            let node = Arc::clone(node);
            if let Some(rt) = self.rt.as_ref() {
                rt.spawn(async move {
                    node.stop().await;
                    shared.finish_stop();
                });
            }
        }
    }

    /// One try of the wind-down: closes cooperatively every channel not
    /// already closing, then closes by force those whose close was first
    /// asked [`FORCE_CLOSE_AFTER_SECS`] ago and that are still listed.
    fn close_retry(&mut self, ops: &dyn NodeOps, now: u64, src: &mut dyn SweepSource) {
        let index = match self.sweep_address(src) {
            Ok((_, index)) => index,
            Err(why) => {
                self.notes.push(format!("SL-36: no close: {why:?}"));
                self.sweep_blocked = Some(why);
                return;
            }
        };
        match ops.close_all(index) {
            Ok(c) => {
                self.notes.push(format!(
                    "SL-36: closing {} Lightning channels, {} already closing",
                    c.begun, c.already
                ));
                for (id, e) in &c.failed {
                    self.notes.push(format!(
                        "SL-36: the close of channel {} failed: {e}",
                        id_text(id)
                    ));
                }
                if c.begun > 0 {
                    self.address_paid(ops, src);
                }
            }
            Err(e) => {
                self.notes.push(format!("SL-36: the close: {e}"));
                return;
            }
        }
        let Some(inst) = &mut self.inst else {
            return;
        };
        let due = channels_to_force(&mut inst.close_first_asked, &ops.channel_ids(), now);
        if due.is_empty() {
            return;
        }
        match ops.force_close(&due) {
            Ok(f) => {
                self.notes.push(format!(
                    "SL-36: closing {} Lightning channels by force",
                    f.begun
                ));
                for (id, e) in &f.failed {
                    self.notes.push(format!(
                        "SL-36: the force close of channel {} failed: {e}",
                        id_text(id)
                    ));
                }
            }
            Err(e) => self.notes.push(format!("SL-36: the force close: {e}")),
        }
    }

    /// Stops the node of the instance in the background. A stop while the
    /// node is still starting waits for the start to end and then stops the
    /// node it made; the instance is stopped only when that is done and the
    /// folder is let go.
    fn stop_instance_task(&mut self) {
        let Some(inst) = &self.inst else {
            return;
        };
        let shared = Arc::clone(&inst.shared);
        let Some(rt) = self.rt.as_ref() else {
            shared.finish_stop();
            return;
        };
        if shared.starting.load(Ordering::SeqCst) {
            shared.give_up.store(true, Ordering::SeqCst);
        }
        rt.spawn(async move {
            while shared.starting.load(Ordering::SeqCst) {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
            let node = lock(&shared.node).clone();
            if let Some(node) = node {
                node.stop().await;
            }
            shared.finish_stop();
        });
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
                // A sale quoted again asks for a new invoice; the old one is
                // of no use.
                if self.slots.contains_key(&index) {
                    self.drop_invoices(index);
                }
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
                Slot::Ready(c, _) => SaleLightning::Invoice(c.clone()),
                Slot::Off(why) => SaleLightning::Unavailable(crate::say::shopln_words(why)),
            },
            None => SaleLightning::Waiting,
        }
    }

    /// CR-283: whether the invoice shown for the sale is one whose payment is
    /// final at once (SL-40): it is ready, and comes over channels that are
    /// open with their fundings confirmed.
    pub fn final_at_once(&self, index: u32) -> bool {
        self.slots
            .get(&index)
            .is_some_and(|s| matches!(&*lock(&s.state), Slot::Ready(_, true)))
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

    /// A sale is over or cancelled: its invoice is no longer asked for or
    /// shown, it leaves the book and the node fails a payment to it back.
    pub fn sale_closed(&mut self, index: u32) {
        self.slots.remove(&index);
        self.drop_invoices(index);
    }

    /// A sale is quoted again: its invoice is of no use, and the sale asks
    /// for a new one.
    pub fn sale_requoted(&mut self, index: u32) {
        self.slots.remove(&index);
        self.drop_invoices(index);
    }

    /// Takes the invoices of a sale out of the book, and tells the node to
    /// fail a payment to them back. An invoice whose payment is claimed and
    /// waits to be recorded stays.
    fn drop_invoices(&mut self, index: u32) {
        self.drain_paid();
        let keep: Vec<[u8; 32]> = self.pending.iter().map(|p| p.payment_hash).collect();
        let gone = {
            let mut book = lock(&self.shared.book);
            let gone = book.drop_sale(index, &keep);
            if !gone.is_empty() {
                if let Err(e) = self.shared.save(&book) {
                    self.notes
                        .push(format!("SL-77: the book was not written: {e}"));
                }
            }
            gone
        };
        if let (false, Some(node), Some(rt)) = (gone.is_empty(), self.node(), self.rt.as_ref()) {
            rt.spawn(async move {
                for hash in gone {
                    let _ = node.cancel_invoice(hash).await;
                }
            });
        }
    }

    /// Takes what the node has reported paid into the list of those waiting
    /// to be recorded.
    fn drain_paid(&mut self) {
        let Some(node) = self.node() else {
            return;
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
    }

    /// SL-27, SL-40: takes what the node says of the fundings into the list of
    /// those the counter has not dealt with yet.
    fn drain_funding(&mut self) {
        let Some(node) = self.node() else {
            return;
        };
        while let Some(e) = node.try_funding() {
            if !self.funding_events.iter().any(|k| k.txid == e.txid) {
                self.funding_events.push(e);
            }
        }
    }

    /// SL-27, SL-40: the fundings the node has settled and the counter has
    /// not acknowledged. Each comes again at every call until
    /// [`ShopLn::funding_handled`]. Call it before [`ShopLn::take_paid`], so
    /// that a payment held for a funding that is confirmed now is taken as
    /// one through a confirmed channel.
    pub fn take_funding(&mut self) -> Vec<FundingEvent> {
        if self.node().is_none() {
            return Vec::new();
        }
        self.drain_funding();
        self.funding_events.clone()
    }

    /// What is known of a funding named in a paid event (SL-27, SL-40): the
    /// node's event if one is waiting, which is the newest word; otherwise the
    /// node is asked, so that a sale is never left unconfirmed for an event
    /// that was acknowledged before the payment came. A funding the node does
    /// not know is taken as not confirmed.
    fn funding_known(&self, txid: &[u8; 32]) -> FundingState {
        if let Some(e) = self.funding_events.iter().find(|e| &e.txid == txid) {
            return match e.outcome {
                Outcome::Watching => FundingState::Watching,
                Outcome::Confirmed => FundingState::Confirmed,
                Outcome::Lost(_) => FundingState::Lost,
            };
        }
        #[cfg(test)]
        if let Some(state) = self.node_funding_states.get(txid) {
            return *state;
        }
        self.node()
            .map_or(FundingState::Unknown, |node| node.funding_state(txid))
    }

    /// What the counter is told of the fundings a paid event names: every
    /// funding confirmed makes the payment final at once, one that is lost
    /// makes it a payment through a lost funding, and else the fundings
    /// still not confirmed are listed. A payment through a channel that
    /// closed before the claim, with no funding known, is one through a lost
    /// funding (SL-27).
    fn paid_funding(&self, p: &Paid) -> Option<PaidFunding> {
        if p.unconfirmed_fundings.is_empty() && !p.more_fundings && !p.lost_unknown {
            return None;
        }
        let mut open = Vec::new();
        // A channel gone at the claim, its funding not known, is a lost one.
        let mut lost = p.lost_unknown;
        for txid in &p.unconfirmed_fundings {
            match self.funding_known(txid) {
                FundingState::Confirmed => {}
                FundingState::Lost => {
                    lost = true;
                    open.push(*txid);
                }
                FundingState::Watching | FundingState::Unknown => open.push(*txid),
            }
        }
        Some(if lost {
            PaidFunding::Lost {
                txids: open,
                more: p.more_fundings,
            }
        } else if open.is_empty() && !p.more_fundings {
            PaidFunding::Confirmed
        } else {
            PaidFunding::Unconfirmed {
                txids: open,
                more: p.more_fundings,
            }
        })
    }

    /// SL-27: the funding of a channel is lost, and these are the sales the
    /// counter holds as paid through it. They are kept in the book, for the
    /// report to the owner, before the manager is told. `false` when the book
    /// could not be written, so the event comes again.
    pub fn funding_lost(&mut self, sales: &[u32]) -> bool {
        let mut book = lock(&self.shared.book);
        let before = (book.lost_funding_sales.clone(), book.lost_funding_total);
        if !book.keep_lost_funding_sales(sales) {
            // A funding lost with no sale to name still tells the manager,
            // once: its event is dealt with here.
            if !sales.is_empty() {
                return true;
            }
            drop(book);
            self.notes.push(
                "SL-27: the funding of a Lightning channel is lost; no sale was paid through it"
                    .to_string(),
            );
            return true;
        }
        if let Err(e) = self.shared.save(&book) {
            (book.lost_funding_sales, book.lost_funding_total) = before;
            drop(book);
            self.notes
                .push(format!("SL-77: the book was not written: {e}"));
            return false;
        }
        drop(book);
        self.notes.push(format!(
            "SL-27: the funding of a Lightning channel is lost; {} sales were paid through it",
            sales.len()
        ));
        true
    }

    /// The counter has dealt with the event of this funding: the node is told,
    /// so that it is not given again, unless a payment held for it is not
    /// recorded yet (SL-21, SL-27): the node's word on the funding is what
    /// decides that payment.
    pub fn funding_handled(&mut self, txid: &[u8; 32]) {
        if self
            .pending
            .iter()
            .any(|p| p.unconfirmed_fundings.contains(txid))
        {
            return;
        }
        self.funding_events.retain(|e| &e.txid != txid);
        if let (Some(node), Some(rt)) = (self.node(), self.rt.as_ref()) {
            let txid = *txid;
            rt.spawn(async move {
                let _ = node.ack_funding(txid).await;
            });
        }
    }

    /// SL-21: the sales the node says are paid, after both copies of the
    /// state are stored. Each comes again at every call until
    /// [`ShopLn::recorded`] says the counter has it.
    pub fn take_paid(&mut self, now: u64) -> Vec<PaidSale> {
        if self.node().is_none() {
            return Vec::new();
        }
        self.drain_paid();
        self.settle_pending(now)
    }

    /// The paid events waiting, as sales to record. What a sale is credited
    /// is what arrived plus what the provider skimmed off that payment (SL-29).
    /// A payment of an invoice the book does not hold is kept in the book as
    /// one no sale took, and only then acknowledged.
    fn settle_pending(&mut self, now: u64) -> Vec<PaidSale> {
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
                            amount_received_msat: p.amount_msat.saturating_add(p.skimmed_msat),
                            settled_at: Some(p.received_unix),
                        },
                        funding: self.paid_funding(p),
                    }),
                    None => {
                        unknown.push((p.payment_hash, p.amount_msat.saturating_add(p.skimmed_msat)))
                    }
                }
            }
        }
        for (hash, amount_msat) in unknown {
            if self.keep_unmatched(None, hash, amount_msat, now) {
                self.recorded(&hash);
            }
        }
        out
    }

    /// SL-21: a claimed payment no sale took is kept in the book, with its
    /// amount and hash, before the node is told to forget it. `false` when
    /// it could not be kept: the node still has it.
    pub fn keep_unmatched(
        &mut self,
        sale: Option<u32>,
        hash: [u8; 32],
        amount_msat: u64,
        now: u64,
    ) -> bool {
        let mut book = lock(&self.shared.book);
        let before = book.unmatched.len();
        if !book.keep_unmatched(Unmatched {
            payment_hash: hash,
            sale,
            amount_msat,
            at: now,
        }) {
            drop(book);
            self.notes.push(
                "SL-21: a claimed Lightning payment no sale took could not be kept: the list is \
                 full"
                    .to_string(),
            );
            return false;
        }
        let added = book.unmatched.len() != before;
        if added {
            if let Err(e) = self.shared.save(&book) {
                book.unmatched.pop();
                drop(book);
                self.notes
                    .push(format!("SL-77: the book was not written: {e}"));
                return false;
            }
        }
        drop(book);
        if added {
            self.notes.push(format!(
                "SL-21: a claimed Lightning payment of {amount_msat} msat was kept: no sale took it"
            ));
        }
        true
    }

    /// The counter has recorded the sale of this payment: the node is told,
    /// so that the event is not given again, and the fee is counted (SL-21,
    /// SL-78).
    pub fn recorded(&mut self, hash: &[u8; 32]) {
        let skimmed_msat = self
            .pending
            .iter()
            .find(|p| &p.payment_hash == hash)
            .map(|p| p.skimmed_msat);
        self.pending.retain(|p| &p.payment_hash != hash);
        {
            let mut book = lock(&self.shared.book);
            let removed = book
                .invoices
                .iter()
                .position(|e| &e.payment_hash == hash)
                .map(|i| book.invoices.remove(i))
                .is_some();
            // The fee counted is what the provider took from this payment,
            // not what was quoted, and only once.
            if let Some(skim) = skimmed_msat {
                book.opening_fees_paid_sats =
                    book.opening_fees_paid_sats.saturating_add(skim / 1000);
            }
            if removed || skimmed_msat.is_some() {
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
    /// the settings do not name this counter and no seed or node is left.
    pub fn report(&self) -> Option<LightningReport> {
        // SL-77: while a seed is held the report goes on after the settings
        // stopped naming the node, with the reason (closing, or what
        // stopped it), so that a wind-down that stalls is seen.
        if !self.named && self.seed.is_none() && self.inst.is_none() {
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
            lsps1: Lsps1Offer::Unknown,
            order_state: OrderState::None,
            order_why: OrderWhy::None,
            order_fee_sats: 0,
            lost_funding_sales: book.lost_funding_sales.clone(),
            lost_funding_total: book.lost_funding_total,
            receiving: None,
        };
        if let Some(node) = self.node() {
            let status = node.status();
            report.balance_sats = node.balance_sat();
            report.receive_room_sats = status.room_msat / 1000;
            report.last_stored_at = status.device.last_stored_unix;
            // This is advisory only. The node's own assessment supplies
            // confirmed capacity and backup blockers; no amount or fee is
            // quoted here, and this report never gates invoice creation.
            if self.named
                && self
                    .inst
                    .as_ref()
                    .is_some_and(|i| i.phase == Phase::Serving)
            {
                let assessment = node.receiving_assessment(None, None, None);
                let pending_channels = u16::try_from(assessment.pending_or_unknown_channels).ok();
                let strict_pending = u32::try_from(status.device.pending_writes).ok();
                let background_pending = u32::try_from(node.pending_background_writes()).ok();
                let captured_at_unix = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .ok()
                    .map(|d| d.as_secs());
                if let (
                    Some(pending_channels @ 0..=4096),
                    Some(strict_pending @ 0..=1_000_000),
                    Some(background_pending @ 0..=1_000_000),
                    Some(captured_at_unix @ 1..),
                ) = (
                    pending_channels,
                    strict_pending,
                    background_pending,
                    captured_at_unix,
                ) {
                    let last_device_store_unix = status.device.last_stored_unix;
                    let node_running = !assessment
                        .not_ready
                        .contains(&openbmt_lightning_node::receiving::NotReady::NodeStopped);
                    let backup_state = receiving_backup_state(
                        node_running,
                        status.device.state,
                        strict_pending,
                        background_pending,
                        last_device_store_unix,
                    );
                    report.receiving = Some(ReceivingSnapshot {
                        captured_at_unix,
                        chain_height: status.chain_height,
                        confirmation_depth: self.confirm_depth.max(1),
                        confirmed_room_msat: assessment.confirmed_room_msat,
                        pending_or_unknown_channels: pending_channels,
                        backup_state,
                        strict_pending_writes: strict_pending,
                        background_pending_writes: background_pending,
                        last_device_store_unix,
                        chain_healthy: status.chain_ok,
                    });
                }
            }
            // SL-45: the fees of rooms bought ahead count with the openings'.
            report.opening_fees_paid_sats = report
                .opening_fees_paid_sats
                .saturating_add(status.lsps1_fees_paid_sat);
            (
                report.lsps1,
                report.order_state,
                report.order_why,
                report.order_fee_sats,
            ) = order_facts(
                status.lsps1,
                status.last_order.as_ref(),
                status.lsps1_stopped,
            );
            report.not_offered = if self
                .inst
                .as_ref()
                .is_some_and(|i| i.phase != Phase::Serving)
                || !self.named
            {
                // A node that is closing offers nothing, whatever its state.
                Why::Closing.not_offered()
            } else if status.device.state == DeviceState::Away {
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
        let why = if self.named || self.inst.is_some() || self.seed.is_some() {
            self.offer_blocked().or_else(|| self.sweep_blocked.clone())
        } else {
            None
        };
        ShopLnStatus {
            needs_seed: self.named && self.inst.is_none() && self.seed.is_none(),
            delete_seed: self.seed_wiped && self.seed.is_none(),
            why,
            funding_lost: lock(&self.shared.book).lost_funding_sales.clone(),
        }
    }

    /// Stops the node, as the counter stops. Waits for the library's last
    /// writes, a few seconds at most.
    pub fn stop(&mut self) {
        if let (Some(inst), Some(rt)) = (self.inst.as_ref(), self.rt.as_ref()) {
            let shared = Arc::clone(&inst.shared);
            shared.give_up.store(true, Ordering::SeqCst);
            // A node that is still starting is waited for, then stopped.
            rt.block_on(async {
                let _ = tokio::time::timeout(Duration::from_secs(20), async {
                    while shared.starting.load(Ordering::SeqCst) {
                        tokio::time::sleep(Duration::from_millis(20)).await;
                    }
                    let node = lock(&shared.node).clone();
                    if let Some(node) = node {
                        node.stop().await;
                    }
                })
                .await;
            });
            shared.finish_stop();
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
    // SL-40: a payment over channels whose funding has a confirmation is
    // final at once; one that opens a channel, or runs over one still
    // waiting for its funding, rests on a transaction not yet in a block.
    let final_at_once = matches!(quote, Receive::Invoice)
        && node.funding_confirmations().iter().all(Option::is_some);
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
    Slot::Ready(checked, final_at_once)
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

    #[test]
    fn report_backup_state_matches_the_serialized_queue_snapshot_and_decodes() {
        for (running, device, strict, background, last_store, expected) in [
            (
                true,
                DeviceState::Reachable,
                0,
                0,
                10,
                ReceivingBackupState::CaughtUp,
            ),
            // A write may enter the lazy queue after the assessment took its
            // earlier view. The enum follows the count actually serialized.
            (
                true,
                DeviceState::Reachable,
                0,
                1,
                10,
                ReceivingBackupState::Behind,
            ),
            (
                true,
                DeviceState::Reachable,
                1,
                0,
                10,
                ReceivingBackupState::Behind,
            ),
            (
                true,
                DeviceState::Away,
                0,
                0,
                10,
                ReceivingBackupState::Away,
            ),
            (
                false,
                DeviceState::Reachable,
                0,
                0,
                10,
                ReceivingBackupState::Unknown,
            ),
        ] {
            let backup_state =
                receiving_backup_state(running, device, strict, background, last_store);
            assert_eq!(backup_state, expected);
            let report = LightningReport {
                balance_sats: 0,
                receive_room_sats: 0,
                opening_fees_paid_sats: 0,
                last_stored_at: last_store,
                last_sweep_at: 0,
                not_offered: NotOffered::Offered,
                lsps1: Lsps1Offer::Unknown,
                order_state: OrderState::None,
                order_why: OrderWhy::None,
                order_fee_sats: 0,
                lost_funding_sales: Vec::new(),
                lost_funding_total: 0,
                receiving: Some(ReceivingSnapshot {
                    captured_at_unix: 10,
                    chain_height: 1,
                    confirmation_depth: 1,
                    confirmed_room_msat: 0,
                    pending_or_unknown_channels: 1,
                    backup_state,
                    strict_pending_writes: strict,
                    background_pending_writes: background,
                    last_device_store_unix: last_store,
                    chain_healthy: true,
                }),
            };
            assert_eq!(
                LightningReport::from_bytes(&report.to_bytes()).unwrap(),
                report
            );
        }
    }

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

    /// What the counter gives Shop Lightning in a test: a wallet and the
    /// next free index of a range.
    struct Fake {
        wallet: Option<String>,
        next: u32,
        end: u32,
        taken: Vec<u32>,
        fail: bool,
    }

    impl Fake {
        fn new(wallet: Option<(String, u32)>) -> Self {
            let (wallet, next) = match wallet {
                Some((w, i)) => (Some(w), i),
                None => (None, 0),
            };
            Self {
                wallet,
                next,
                end: u32::MAX,
                taken: Vec::new(),
                fail: false,
            }
        }
    }

    impl SweepSource for Fake {
        fn wallet(&self) -> Option<String> {
            self.wallet.clone()
        }
        fn take_index(&mut self) -> Result<Option<u32>, String> {
            if self.fail {
                return Err("disk full".to_string());
            }
            if self.next >= self.end {
                return Ok(None);
            }
            self.next += 1;
            self.taken.push(self.next - 1);
            Ok(Some(self.next - 1))
        }
    }

    /// A node that does what the sweep and the close ask, and remembers it.
    struct FakeNode {
        balance: u64,
        swept: Mutex<Vec<u32>>,
        closed: Mutex<Vec<u32>>,
        /// The channels the node lists; they never leave by themselves.
        channels: Mutex<Vec<[u8; 32]>>,
        forced: Mutex<Vec<Vec<[u8; 32]>>>,
        spends: Mutex<Vec<[u8; 32]>>,
        index: Mutex<u32>,
    }

    impl FakeNode {
        fn new(balance: u64) -> Self {
            Self {
                balance,
                swept: Mutex::new(Vec::new()),
                closed: Mutex::new(Vec::new()),
                channels: Mutex::new(Vec::new()),
                forced: Mutex::new(Vec::new()),
                spends: Mutex::new(Vec::new()),
                index: Mutex::new(0),
            }
        }
    }

    impl NodeOps for FakeNode {
        fn balance_sat(&self) -> u64 {
            self.balance
        }
        fn sweep(&self, index: u32, _amount_sat: u64) -> Result<(), String> {
            lock(&self.swept).push(index);
            Ok(())
        }
        fn close_all(&self, index: u32) -> Result<CloseReport, String> {
            lock(&self.closed).push(index);
            Ok(CloseReport {
                begun: 1,
                ..CloseReport::default()
            })
        }
        fn channel_ids(&self) -> Vec<[u8; 32]> {
            lock(&self.channels).clone()
        }
        fn force_close(&self, ids: &[[u8; 32]]) -> Result<CloseReport, String> {
            lock(&self.forced).push(ids.to_vec());
            Ok(CloseReport {
                begun: ids.len(),
                ..CloseReport::default()
            })
        }
        fn sweeper_spends(&self) -> Vec<[u8; 32]> {
            lock(&self.spends).clone()
        }
        fn sweeper_index(&self) -> u32 {
            *lock(&self.index)
        }
        fn set_sweeper_index(&self, index: u32, _rt: &Runtime) {
            *lock(&self.index) = index;
        }
    }

    /// Counts the nodes whose folder is held at once.
    #[derive(Default)]
    pub(super) struct Probe {
        live: std::sync::atomic::AtomicUsize,
        most: std::sync::atomic::AtomicUsize,
        entered: std::sync::atomic::AtomicUsize,
    }

    impl Probe {
        pub(super) fn enter(&self) {
            let now = self.live.fetch_add(1, Ordering::SeqCst) + 1;
            self.most.fetch_max(now, Ordering::SeqCst);
            self.entered.fetch_add(1, Ordering::SeqCst);
        }
        pub(super) fn leave(&self) {
            self.live.fetch_sub(1, Ordering::SeqCst);
        }
    }

    fn ln(network: Network, relay: Option<RelayHint>, seed_n: Option<u8>) -> ShopLn {
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

    /// An instance with a node that is not a real one, for the sweep.
    fn serving(l: &mut ShopLn, wallet: &str) {
        l.inst = Some(Instance {
            sl: sl(),
            phase: Phase::Serving,
            shared: Arc::new(InstShared::new()),
            failed_at: None,
            close_begun_at: None,
            close_first_asked: HashMap::new(),
            sweep_tried_at: None,
            wallet: wallet_hash(wallet),
            spends_seen: None,
            pointed: None,
            finishing: false,
        });
        let _ = l.rt();
    }

    #[test]
    fn sl_60_the_counter_does_not_start_the_node_on_the_real_network() {
        let mut l = ln(Network::Bitcoin, Some(hint(8)), Some(1));
        let mut src = Fake::new(Some(wallet()));
        assert_eq!(l.prepare(&sl(), &mut src).err(), Some(Why::Mainnet));
        cleanup(&l);
    }

    #[test]
    fn sl_72_a_node_whose_seed_does_not_give_the_pinned_id_does_not_start() {
        let mut src = Fake::new(Some(wallet()));
        let mut l = ln(Network::Regtest, Some(hint(8)), Some(2));
        assert_eq!(l.prepare(&sl(), &mut src).err(), Some(Why::NoSeed));
        let mut none = ln(Network::Regtest, Some(hint(8)), None);
        assert_eq!(none.prepare(&sl(), &mut src).err(), Some(Why::NoSeed));
        cleanup(&l);
        cleanup(&none);
    }

    #[test]
    fn sl_73_the_relay_must_be_the_box_named_as_the_second_device() {
        let mut src = Fake::new(Some(wallet()));
        // The relay is device 9, the second device is 8.
        let mut l = ln(Network::Regtest, Some(hint(9)), Some(1));
        assert_eq!(l.prepare(&sl(), &mut src).err(), Some(Why::NotTheBox));
        let mut none = ln(Network::Regtest, None, Some(1));
        assert_eq!(none.prepare(&sl(), &mut src).err(), Some(Why::NotTheBox));
        assert!(
            src.taken.is_empty(),
            "no index is taken for a node that does not start"
        );
        cleanup(&l);
        cleanup(&none);
    }

    #[test]
    fn sl_33_a_counter_with_no_description_of_the_wallet_does_not_start() {
        let mut src = Fake::new(None);
        let mut l = ln(Network::Regtest, Some(hint(8)), Some(1));
        assert_eq!(l.prepare(&sl(), &mut src).err(), Some(Why::NoWallet));
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
            let mut src = Fake::new(Some(wallet()));
            let mut l = ln(Network::Regtest, Some(onion), Some(1));
            assert_eq!(l.prepare(&sl(), &mut src).err(), Some(Why::NoRoute));
            cleanup(&l);
        }
    }

    #[test]
    fn sl_77_a_start_that_can_be_made_is_made_with_the_settings_terms_in_millisatoshis() {
        let mut src = Fake::new(Some(wallet()));
        let mut l = ln(Network::Regtest, Some(hint(8)), Some(1));
        let (c, hash) = l.prepare(&sl(), &mut src).unwrap();
        assert_eq!(c.max_opening_fee_msat, 3_000_000);
        // The index is the counter's next free one, and the book keeps it.
        assert_eq!(c.sweep_index, 19);
        assert_eq!(src.taken, vec![19]);
        assert_eq!(
            lock(&l.shared.book).sweep,
            Some(SweepAddress {
                wallet: hash,
                index: 19
            })
        );
        assert_eq!(c.provider.addr, sl().provider_addr);
        assert_eq!(c.provider.node_id.serialize(), sl().provider_node_id);
        assert_eq!(c.network, Network::Regtest);
        assert!(c.tor.is_none());
        assert_eq!(c.seed.0, seed(1));
        assert!(matches!(c.chain, ChainSource::Filters(_)));
        let _ = LIGHTNING_NODE_BYTES;
        // A start again does not take another index.
        let (again, _) = l.prepare(&sl(), &mut src).unwrap();
        assert_eq!(again.sweep_index, 19);
        assert_eq!(src.taken, vec![19]);
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

    #[test]
    fn sl_36_a_channel_is_forced_only_from_an_hour_after_its_first_ask() {
        let (a, b) = ([1u8; 32], [2u8; 32]);
        let mut asked = HashMap::new();
        assert!(channels_to_force(&mut asked, &[a], 1_000).is_empty());
        assert!(channels_to_force(&mut asked, &[a], 1_000 + FORCE_CLOSE_AFTER_SECS - 1).is_empty());
        // A second channel that shows later has its own clock.
        let t = 1_000 + FORCE_CLOSE_AFTER_SECS;
        assert_eq!(channels_to_force(&mut asked, &[a, b], t), vec![a]);
        assert_eq!(asked.get(&b), Some(&t));
        // One that is no longer listed is dropped, and starts again if it
        // comes back.
        assert!(channels_to_force(&mut asked, &[b], t + 1).is_empty());
        assert!(!asked.contains_key(&a));
        assert!(channels_to_force(&mut asked, &[a, b], t + 2).is_empty());
        assert_eq!(asked.get(&a), Some(&(t + 2)));
    }

    #[test]
    fn sl_36_a_channel_that_never_leaves_is_closed_by_force_after_an_hour() {
        let (w, first) = wallet();
        let mut src = Fake::new(Some((w.clone(), first)));
        let mut l = ln(Network::Regtest, Some(hint(8)), Some(1));
        serving(&mut l, &w);
        let node = FakeNode::new(0);
        *lock(&node.channels) = vec![[7; 32], [8; 32]];
        l.close_retry(&node, 1_000, &mut src);
        l.close_retry(&node, 1_000 + FORCE_CLOSE_AFTER_SECS - 1, &mut src);
        assert!(lock(&node.forced).is_empty());
        // One channel leaves; the other is forced, and the cooperative close
        // is still asked at every try.
        *lock(&node.channels) = vec![[7; 32]];
        l.close_retry(&node, 1_000 + FORCE_CLOSE_AFTER_SECS, &mut src);
        assert_eq!(*lock(&node.forced), vec![vec![[7; 32]]]);
        assert_eq!(lock(&node.closed).len(), 3);
        assert!(l.notes.iter().any(|n| n.starts_with("SL-36: closing 1")));
        cleanup(&l);
    }

    #[test]
    fn sl_33_two_sweeps_use_two_addresses_and_none_is_reused() {
        let (w, first) = wallet();
        let mut src = Fake::new(Some((w.clone(), first)));
        let mut l = ln(Network::Regtest, Some(hint(8)), Some(1));
        serving(&mut l, &w);
        let node = FakeNode::new(300_000);
        l.sweep(&node, 1_000, &mut src);
        l.sweep(&node, 1_000 + SWEEP_EVERY_SECS, &mut src);
        let swept = lock(&node.swept).clone();
        assert_eq!(swept.len(), 2);
        assert_ne!(swept[0], swept[1]);
        // Each came from the counter's ranges, and the output sweeper was
        // moved on to a third.
        assert_eq!(src.taken, vec![first, first + 1, first + 2]);
        assert_eq!(swept, vec![first, first + 1]);
        assert_eq!(*lock(&node.index), first + 2);
        assert_eq!(lock(&l.shared.book).sweep.unwrap().index, first + 2);
        assert_eq!(lock(&l.shared.book).last_sweep_at, 1_000 + SWEEP_EVERY_SECS);
        cleanup(&l);
    }

    #[test]
    fn sl_33_a_close_pays_an_address_of_its_own_and_the_sweeper_moves_on_after_a_spend() {
        let (w, first) = wallet();
        let mut src = Fake::new(Some((w.clone(), first)));
        let mut l = ln(Network::Regtest, Some(hint(8)), Some(1));
        serving(&mut l, &w);
        let node = FakeNode::new(0);
        l.point_sweeper(&node, &mut src);
        assert_eq!(*lock(&node.index), first);
        // The sweeper spent an output to that address: the next is new.
        lock(&node.spends).push([5; 32]);
        l.point_sweeper(&node, &mut src);
        assert_eq!(*lock(&node.index), first + 1);
        // The same spend is not counted again.
        l.point_sweeper(&node, &mut src);
        assert_eq!(*lock(&node.index), first + 1);
        assert_eq!(src.taken, vec![first, first + 1]);
        cleanup(&l);
    }

    #[test]
    fn sl_33_a_switch_of_wallet_stops_the_node_and_the_next_index_is_of_the_new_wallet() {
        let (w, first) = wallet();
        let mut src = Fake::new(Some((w.clone(), first)));
        let mut l = ln(Network::Regtest, Some(hint(8)), Some(1));
        serving(&mut l, &w);
        let node = FakeNode::new(300_000);
        l.sweep(&node, 1_000, &mut src);
        assert_eq!(lock(&node.swept).len(), 1);
        // The counter now pays into another wallet.
        let other = format!("{w}x");
        src.wallet = Some(other.clone());
        src.next = 500;
        let mut settings = Settings::defaults(1_000);
        settings.shop_lightning = Some(sl());
        // The node is stopped and started again with the new wallet, and the
        // address it pays to is an index of that wallet.
        let mut now = 2_000;
        let began = std::time::Instant::now();
        while l
            .inst
            .as_ref()
            .is_none_or(|i| i.wallet != wallet_hash(&other))
        {
            assert!(began.elapsed() < Duration::from_secs(30), "no restart");
            l.reconcile(&settings, now, &mut src);
            now += 1;
            std::thread::sleep(Duration::from_millis(20));
        }
        let book = lock(&l.shared.book).sweep.unwrap();
        assert_eq!(book.wallet, wallet_hash(&other));
        assert_eq!(book.index, 500);
        let (config, hash) = l.prepare(&sl(), &mut src).unwrap();
        assert_eq!(config.wallet, other);
        assert_eq!(hash, wallet_hash(&other));
        assert_eq!(config.sweep_index, 500);
        l.stop();
        cleanup(&l);
    }

    #[test]
    fn sl_33_with_no_free_index_nothing_is_swept_and_the_reason_is_given() {
        let (w, first) = wallet();
        let mut src = Fake::new(Some((w.clone(), first)));
        src.end = first;
        let mut l = ln(Network::Regtest, Some(hint(8)), Some(1));
        serving(&mut l, &w);
        let node = FakeNode::new(300_000);
        l.sweep(&node, 1_000, &mut src);
        assert!(lock(&node.swept).is_empty());
        assert_eq!(l.sweep_blocked, Some(Why::NoSweepAddress));
        assert_eq!(l.prepare(&sl(), &mut src).err(), Some(Why::NoSweepAddress));
        // An index appears: the sweep goes on.
        src.end = u32::MAX;
        l.sweep(&node, 1_000 + SWEEP_EVERY_SECS, &mut src);
        assert_eq!(lock(&node.swept).len(), 1);
        cleanup(&l);
    }

    #[test]
    fn sl_77_a_book_that_does_not_read_is_not_an_empty_book_and_the_seed_is_kept() {
        let (w, first) = wallet();
        let mut src = Fake::new(Some((w, first)));
        let dir = std::env::temp_dir().join(format!(
            "shopln-badbook-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("shop-lightning")).unwrap();
        fs::write(
            dir.join("shop-lightning").join(crate::shopln::BOOK_FILE),
            b"junk",
        )
        .unwrap();
        let mut l = ShopLn::new(ShopLnArgs {
            me: id(3).device_id(),
            dir: dir.clone(),
            network: Network::Regtest,
            driver: DriverConfig::new(Network::Regtest, 0),
            tor_on: false,
            config: ShopLnConfig {
                seed_record: Some(SeedRecord::new(seed(1)).to_bytes()),
                identity: Box::new(|| Some(id(3))),
                relay: Some(hint(8)),
                link_proxy: None,
            },
        });
        // Settings on, and settings off: no node, the seed stays.
        for settings in [
            {
                let mut s = Settings::defaults(1_000);
                s.shop_lightning = Some(sl());
                s
            },
            Settings::defaults(1_000),
        ] {
            for t in 0..5 {
                l.reconcile(&settings, 1_000 + t * 100, &mut src);
                assert!(l.inst.is_none());
                assert!(l.has_seed());
                assert!(!l.status().delete_seed);
                assert!(matches!(l.status().why, Some(Why::NotStarted(_))));
            }
        }
        assert!(src.taken.is_empty());
        // The book is not written over.
        assert_eq!(
            fs::read(dir.join("shop-lightning").join(crate::shopln::BOOK_FILE)).unwrap(),
            b"junk"
        );
        // It reads again once the file does.
        Book::default().save(&dir.join("shop-lightning")).unwrap();
        l.reconcile(&Settings::defaults(1_000), 9_000, &mut src);
        assert!(l.book_error.is_none());
        l.stop();
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn sl_77_the_seed_is_not_deleted_for_want_of_a_plan_but_when_a_started_node_reports_nothing() {
        let (w, first) = wallet();
        let mut src = Fake::new(Some((w.clone(), first)));
        // Settings off, a seed held, no plan in the book.
        let mut l = ln(Network::Regtest, Some(hint(8)), Some(1));
        let off = Settings::defaults(1_000);
        l.reconcile(&off, 1_000, &mut src);
        assert!(l.has_seed(), "a seed is not deleted from the book alone");
        assert!(!l.status().delete_seed);
        // A node was made to look: a closing instance, from the seed's own
        // terms.
        let inst = l.inst.as_ref().expect("a node is started to look");
        assert_eq!(inst.phase, Phase::Closing);
        assert_eq!(inst.sl.node_id, node_id(&seed(1)));
        // The node has reported nothing (it is not up): still held, at every
        // pump.
        for t in 1..5 {
            l.reconcile(&off, 1_000 + t, &mut src);
            assert!(l.has_seed());
            assert!(!l.status().delete_seed);
        }
        l.stop();
        cleanup(&l);
    }

    fn pending(hash: u8, amount_msat: u64, skimmed_msat: u64) -> Paid {
        Paid {
            payment_hash: [hash; 32],
            amount_msat,
            skimmed_msat,
            sender_intended_msat: None,
            received_unix: 50,
            unconfirmed_fundings: Vec::new(),
            more_fundings: false,
            lost_unknown: false,
        }
    }

    fn invoice(hash: u8, sale: u32, fee_msat: u64) -> InvoiceEntry {
        InvoiceEntry {
            payment_hash: [hash; 32],
            sale,
            amount_msat: 50_000_000,
            opening_fee_msat: fee_msat,
            made_at: 10,
        }
    }

    #[test]
    fn sl_29_a_sale_is_credited_what_arrived_plus_what_was_skimmed_not_the_quoted_fee() {
        let mut l = ln(Network::Regtest, Some(hint(8)), Some(1));
        // The invoice quoted a 2,000 msat fee.
        lock(&l.shared.book).invoices = vec![invoice(1, 4, 2_000_000), invoice(2, 5, 2_000_000)];
        // The first payment opened a channel and was skimmed 1,500 msat; the
        // second found a channel already open and was skimmed nothing.
        l.pending = vec![pending(1, 48_500_000, 1_500_000), pending(2, 50_000_000, 0)];
        let paid = l.settle_pending(100);
        assert_eq!(paid.len(), 2);
        assert_eq!(paid[0].sale, 4);
        assert_eq!(paid[0].facts.amount_received_msat, 50_000_000);
        assert_eq!(paid[1].sale, 5);
        assert_eq!(paid[1].facts.amount_received_msat, 50_000_000);
        l.recorded(&[1; 32]);
        l.recorded(&[2; 32]);
        // The fee counted is the skim, 1,500 sats, not 4,000 sats of quotes.
        assert_eq!(lock(&l.shared.book).opening_fees_paid_sats, 1_500);
        assert!(lock(&l.shared.book).invoices.is_empty());
        // Recording twice counts once.
        l.recorded(&[1; 32]);
        assert_eq!(lock(&l.shared.book).opening_fees_paid_sats, 1_500);
        cleanup(&l);
    }

    fn unconfirmed_of(txids: &[[u8; 32]], more: bool) -> PaidFunding {
        PaidFunding::Unconfirmed {
            txids: txids.to_vec(),
            more,
        }
    }

    #[test]
    fn sl_40_a_payment_through_an_unconfirmed_funding_says_so_until_the_node_says_it_is_confirmed()
    {
        let mut l = ln(Network::Regtest, Some(hint(8)), Some(1));
        lock(&l.shared.book).invoices = vec![invoice(1, 4, 0), invoice(2, 5, 0)];
        let funded = |hash: u8| Paid {
            unconfirmed_fundings: vec![[0xf1; 32]],
            ..pending(hash, 50_000_000, 0)
        };
        l.pending = vec![funded(1), pending(2, 50_000_000, 0)];
        let paid = l.settle_pending(100);
        assert_eq!(paid[0].funding, Some(unconfirmed_of(&[[0xf1; 32]], false)));
        assert_eq!(paid[1].funding, None);
        // The node says the funding is confirmed: the same payment is taken
        // as one through a confirmed channel.
        l.funding_events = vec![FundingEvent {
            txid: [0xf1; 32],
            outcome: Outcome::Confirmed,
        }];
        let paid = l.settle_pending(100);
        assert_eq!(paid[0].funding, Some(PaidFunding::Confirmed));
        // A loss is not a confirmation: it is a payment through a lost funding.
        l.funding_events = vec![FundingEvent {
            txid: [0xf1; 32],
            outcome: Outcome::Lost(openbmt_lightning_node::funding::LostWhy::Closed),
        }];
        let paid = l.settle_pending(100);
        assert_eq!(
            paid[0].funding,
            Some(PaidFunding::Lost {
                txids: vec![[0xf1; 32]],
                more: false
            })
        );
        cleanup(&l);
    }

    #[test]
    fn sl_27_a_payment_through_a_channel_gone_with_no_funding_known_is_a_lost_funding() {
        let mut l = ln(Network::Regtest, Some(hint(8)), Some(1));
        lock(&l.shared.book).invoices = vec![invoice(1, 4, 0)];
        l.pending = vec![Paid {
            lost_unknown: true,
            ..pending(1, 50_000_000, 0)
        }];
        // Never final at once, and no event waits for it: a lost funding
        // with nothing to name.
        assert_eq!(
            l.settle_pending(100)[0].funding,
            Some(PaidFunding::Lost {
                txids: vec![],
                more: false
            })
        );
        // The fundings the payment names that are confirmed do not change that.
        l.pending[0].unconfirmed_fundings = vec![[0xf1; 32]];
        l.funding_events = vec![FundingEvent {
            txid: [0xf1; 32],
            outcome: Outcome::Confirmed,
        }];
        assert_eq!(
            l.settle_pending(100)[0].funding,
            Some(PaidFunding::Lost {
                txids: vec![],
                more: false
            })
        );
        cleanup(&l);
    }

    #[test]
    fn sl_40_a_payment_through_several_fundings_is_confirmed_only_when_all_are() {
        let mut l = ln(Network::Regtest, Some(hint(8)), Some(1));
        lock(&l.shared.book).invoices = vec![invoice(1, 4, 0)];
        let two = Paid {
            unconfirmed_fundings: vec![[0xf1; 32], [0xf2; 32]],
            ..pending(1, 50_000_000, 0)
        };
        l.pending = vec![two];
        let confirmed = |t: u8| FundingEvent {
            txid: [t; 32],
            outcome: Outcome::Confirmed,
        };
        l.funding_events = vec![confirmed(0xf1)];
        // One confirmed, one not: the one left is listed.
        assert_eq!(
            l.settle_pending(100)[0].funding,
            Some(unconfirmed_of(&[[0xf2; 32]], false))
        );
        l.funding_events = vec![confirmed(0xf1), confirmed(0xf2)];
        assert_eq!(
            l.settle_pending(100)[0].funding,
            Some(PaidFunding::Confirmed)
        );
        // More fundings than listed are never all confirmed.
        l.pending[0].more_fundings = true;
        assert_eq!(
            l.settle_pending(100)[0].funding,
            Some(unconfirmed_of(&[], true))
        );
        // One lost among them is a loss, whatever the others did.
        l.pending[0].more_fundings = false;
        l.funding_events = vec![
            confirmed(0xf1),
            FundingEvent {
                txid: [0xf2; 32],
                outcome: Outcome::Lost(openbmt_lightning_node::funding::LostWhy::TimedOut),
            },
        ];
        assert_eq!(
            l.settle_pending(100)[0].funding,
            Some(PaidFunding::Lost {
                txids: vec![[0xf2; 32]],
                more: false
            })
        );
        cleanup(&l);
    }

    #[test]
    fn sl_27_a_paid_event_with_no_funding_event_asks_the_node_for_the_state() {
        let mut l = ln(Network::Regtest, Some(hint(8)), Some(1));
        lock(&l.shared.book).invoices = vec![invoice(1, 4, 0)];
        l.pending = vec![Paid {
            unconfirmed_fundings: vec![[0xf1; 32]],
            ..pending(1, 50_000_000, 0)
        }];
        // No event is waiting (it was acknowledged before the payment came);
        // the node holds the funding as confirmed, lost, watched or not known.
        assert!(l.funding_events.is_empty());
        for (state, want) in [
            (FundingState::Confirmed, PaidFunding::Confirmed),
            (
                FundingState::Lost,
                PaidFunding::Lost {
                    txids: vec![[0xf1; 32]],
                    more: false,
                },
            ),
            (FundingState::Watching, unconfirmed_of(&[[0xf1; 32]], false)),
            (FundingState::Unknown, unconfirmed_of(&[[0xf1; 32]], false)),
        ] {
            l.node_funding_states.insert([0xf1; 32], state);
            assert_eq!(l.settle_pending(100)[0].funding, Some(want), "{state:?}");
        }
        cleanup(&l);
    }

    #[test]
    fn sl_40_the_counters_depth_is_given_to_the_node_at_its_start() {
        let mut l = ln(Network::Regtest, Some(hint(8)), Some(1));
        let mut src = Fake::new(Some(wallet()));
        let mut settings = Settings::defaults(1_000);
        settings.min_confirmations = 3;
        l.reconcile(&settings, 1_000, &mut src);
        assert_eq!(l.confirm_depth, 3);
        let (config, _) = l.prepare(&sl(), &mut src).unwrap();
        assert_eq!(config.confirm_depth, 3);
        settings.min_confirmations = 0;
        l.reconcile(&settings, 1_001, &mut src);
        assert_eq!(l.confirm_depth, 1);
        cleanup(&l);
    }

    #[test]
    fn sl_27_the_event_of_a_funding_stays_while_a_payment_held_for_it_is_not_recorded() {
        let mut l = ln(Network::Regtest, Some(hint(8)), Some(1));
        let event = FundingEvent {
            txid: [0xf1; 32],
            outcome: Outcome::Confirmed,
        };
        l.funding_events = vec![event];
        l.pending = vec![Paid {
            unconfirmed_fundings: vec![[0xf9; 32], [0xf1; 32]],
            ..pending(1, 50_000_000, 0)
        }];
        l.funding_handled(&[0xf1; 32]);
        assert_eq!(l.funding_events, vec![event], "the payment is held for it");
        // The payment is recorded: the event is dealt with.
        l.recorded(&[1; 32]);
        l.funding_handled(&[0xf1; 32]);
        assert!(l.funding_events.is_empty());
        cleanup(&l);
    }

    #[test]
    fn sl_27_the_sales_of_a_lost_funding_are_kept_in_the_book_and_in_the_report() {
        let mut l = ln(Network::Regtest, Some(hint(8)), Some(1));
        assert!(l.status().funding_lost.is_empty());
        assert!(l.report().unwrap().lost_funding_sales.is_empty());
        assert!(l.funding_lost(&[7, 3]));
        assert_eq!(l.status().funding_lost, vec![3, 7]);
        assert_eq!(l.report().unwrap().lost_funding_sales, vec![3, 7]);
        // On disk, and said once.
        assert_eq!(Book::load(&l.dir).unwrap().lost_funding_sales, vec![3, 7]);
        assert_eq!(l.take_notes().len(), 1);
        assert!(l.funding_lost(&[3, 7]));
        assert!(l.take_notes().is_empty());
        assert_eq!(l.report().unwrap().lost_funding_total, 2);
        cleanup(&l);
    }

    #[test]
    fn sl_21_a_payment_with_no_invoice_in_the_book_is_kept_not_dropped() {
        let mut l = ln(Network::Regtest, Some(hint(8)), Some(1));
        l.pending = vec![pending(9, 4_000_000, 1_000_000)];
        assert!(l.settle_pending(100).is_empty());
        let book = lock(&l.shared.book);
        assert_eq!(
            book.unmatched,
            vec![Unmatched {
                payment_hash: [9; 32],
                sale: None,
                amount_msat: 5_000_000,
                at: 100
            }]
        );
        drop(book);
        assert!(l.pending.is_empty());
        // It is on disk.
        assert_eq!(Book::load(&l.dir).unwrap().unmatched.len(), 1);
        cleanup(&l);
    }

    #[test]
    fn sl_21_a_payment_that_cannot_be_kept_stays_with_the_node() {
        let mut l = ln(Network::Regtest, Some(hint(8)), Some(1));
        for n in 0..crate::shopln::MAX_UNMATCHED {
            let mut hash = [0u8; 32];
            hash[..8].copy_from_slice(&(n as u64).to_le_bytes());
            assert!(l.keep_unmatched(None, hash, 1_000, 1));
        }
        l.pending = vec![pending(9, 4_000_000, 0)];
        assert!(l.settle_pending(100).is_empty());
        assert_eq!(l.pending.len(), 1, "not acknowledged: the node keeps it");
        cleanup(&l);
    }

    #[test]
    fn sl_21_six_hundred_sales_opened_and_closed_never_reach_the_cap() {
        let mut l = ln(Network::Regtest, Some(hint(8)), Some(1));
        for sale in 0..600u32 {
            {
                let mut book = lock(&l.shared.book);
                assert!(book.invoices.len() < MAX_INVOICES);
                book.invoices.push(invoice(1, sale, 0));
            }
            // Closed, cancelled or quoted again: either way it leaves.
            if sale % 2 == 0 {
                l.sale_closed(sale);
            } else {
                l.sale_requoted(sale);
            }
            assert!(lock(&l.shared.book).invoices.is_empty(), "sale {sale}");
        }
        // An invoice whose payment is claimed and waits stays.
        lock(&l.shared.book).invoices = vec![invoice(7, 3, 0)];
        l.pending = vec![pending(7, 50_000_000, 0)];
        l.sale_closed(3);
        assert_eq!(lock(&l.shared.book).invoices.len(), 1);
        cleanup(&l);
    }

    #[test]
    fn sl_45_the_report_says_whether_room_is_bought_ahead_and_the_last_order() {
        use openbmt_lightning_node::lsps1 as ln;
        let record = |state, why| ln::OrderRecord {
            id: "o".to_string(),
            provider: [2; 33],
            state,
            why,
            fee_sat: 2_000,
            lsp_balance_sat: 1_000_000,
            sent_msat: 0,
            payment_hash: [1; 32],
            created_unix: 1,
            updated_unix: 2,
            prior_channels: Vec::new(),
        };
        assert_eq!(
            order_facts(ln::Offer::Unknown, None, false),
            (Lsps1Offer::Unknown, OrderState::None, OrderWhy::None, 0)
        );
        assert_eq!(
            order_facts(ln::Offer::NotOffered, None, false),
            (Lsps1Offer::NotOffered, OrderState::None, OrderWhy::None, 0)
        );
        let paid = record(ln::OrderState::Paid, ln::OrderWhy::None);
        assert_eq!(
            order_facts(ln::Offer::Offered, Some(&paid), false),
            (Lsps1Offer::Offered, OrderState::Paid, OrderWhy::None, 2_000)
        );
        let refused = record(ln::OrderState::Failed, ln::OrderWhy::FeeAboveMax);
        assert_eq!(
            order_facts(ln::Offer::Offered, Some(&refused), false),
            (
                Lsps1Offer::Offered,
                OrderState::Failed,
                OrderWhy::FeeAboveMax,
                2_000
            )
        );
        // Every state and cause of the node has its own in the report.
        for (state, expect) in [
            (ln::OrderState::Open, OrderState::Open),
            (ln::OrderState::Done, OrderState::Done),
        ] {
            let r = record(state, ln::OrderWhy::None);
            assert_eq!(order_facts(ln::Offer::Offered, Some(&r), false).1, expect);
        }
        for (why, expect) in [
            (ln::OrderWhy::CannotPay, OrderWhy::CannotPay),
            (ln::OrderWhy::Refused, OrderWhy::Refused),
            (ln::OrderWhy::Expired, OrderWhy::Expired),
            (ln::OrderWhy::PaymentFailed, OrderWhy::PaymentFailed),
            (ln::OrderWhy::Refunded, OrderWhy::Refunded),
            (ln::OrderWhy::NotPossible, OrderWhy::NotPossible),
            (ln::OrderWhy::NoChannel, OrderWhy::NoChannel),
        ] {
            let r = record(ln::OrderState::Failed, why);
            assert_eq!(order_facts(ln::Offer::Offered, Some(&r), false).2, expect);
        }
        // A buying stopped says so, whatever the last order's own cause.
        let r = record(ln::OrderState::Failed, ln::OrderWhy::NoChannel);
        assert_eq!(
            order_facts(ln::Offer::Offered, Some(&r), true),
            (
                Lsps1Offer::Offered,
                OrderState::Failed,
                OrderWhy::Stopped,
                2_000
            )
        );
        assert_eq!(
            order_facts(ln::Offer::Offered, None, true).2,
            OrderWhy::Stopped
        );
    }

    #[test]
    fn sl_78_the_report_goes_on_while_a_seed_is_held_after_the_settings_stop() {
        let l = ln(Network::Regtest, Some(hint(8)), Some(1));
        let r = l.report().expect("a seed is held");
        assert_ne!(r.not_offered, NotOffered::Offered);
        cleanup(&l);
        let none = ln(Network::Regtest, Some(hint(8)), None);
        assert!(none.report().is_none());
        cleanup(&none);
    }

    #[test]
    fn sl_23_a_stop_while_the_node_starts_waits_and_only_one_node_holds_the_folder() {
        let (w, first) = wallet();
        let mut src = Fake::new(Some((w, first)));
        let mut l = ln(Network::Regtest, Some(hint(8)), Some(1));
        let probe = Arc::new(Probe::default());
        l.probe = Some(Arc::clone(&probe));
        l.start_delay = Duration::from_millis(600);
        let mut settings = Settings::defaults(1_000);
        settings.shop_lightning = Some(sl());
        l.reconcile(&settings, 1_000, &mut src);
        assert_eq!(l.inst.as_ref().unwrap().phase, Phase::Serving);
        // The cap is lowered while the node is still starting.
        let mut lower = settings.clone();
        lower.shop_lightning = Some(ShopLightning {
            cap_sats: 50_000,
            ..sl()
        });
        let began = std::time::Instant::now();
        let mut now = 1_001;
        let mut restarted = false;
        while began.elapsed() < Duration::from_secs(60) {
            l.reconcile(&lower, now, &mut src);
            now += 1;
            if probe.entered.load(Ordering::SeqCst) >= 2 && l.inst.is_some() {
                restarted = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(restarted, "the second node started");
        assert_eq!(l.inst.as_ref().unwrap().sl.cap_sats, 50_000);
        // Never two at once, though the first start was slow.
        assert_eq!(probe.most.load(Ordering::SeqCst), 1);
        l.stop();
        assert_eq!(probe.live.load(Ordering::SeqCst), 0);
        cleanup(&l);
    }

    #[test]
    fn sl_23_a_second_holder_of_the_folder_waits_for_the_first() {
        let dir = std::env::temp_dir().join(format!("shopln-lock-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join(FOLDER_LOCK_FILE);
        let never = AtomicBool::new(false);
        let first = take_folder_lock(&path, &never).unwrap().unwrap();
        let give_up = AtomicBool::new(true);
        assert!(take_folder_lock(&path, &give_up).unwrap().is_none());
        drop(first);
        assert!(take_folder_lock(&path, &give_up).unwrap().is_some());
        let _ = fs::remove_dir_all(&dir);
    }
}
