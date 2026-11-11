//! The owner node on a thread of its own, and every call the app can make
//! on it.
//!
//! The owner node owns sockets and drivers, and some of what it holds (the
//! clock, the random source, the deep check's spawner) is not `Send`. So the
//! node, the link, the refund list and Key 1's state live on one thread that
//! this layer starts, and every call is a job sent to it. A job runs to its
//! end before the next one starts, which is what the lock of the counter
//! library does there.
//!
//! - `pump` is taken in slices: its wait is cut to [`MAX_WAIT_MS`] and each
//!   slice is one job of at most [`SLICE_MS`] of waiting for the driver's
//!   events. Between two slices the thread takes the next job in line, so a
//!   call from the screen thread waits for at most one slice and the work of
//!   the calls before it.
//! - The thread is the only one that touches the node. Nothing here keeps a
//!   JNI reference or the environment of a call.
//!
//! The node's clock is the app's: every call that takes a time raises the
//! clock to it, and the clock never goes back, so two threads that read
//! their clocks a moment apart cannot make the node's time step back.
//!
//! A panic in a job is caught, becomes an internal error and poisons the
//! entry: every later call returns the same error until the node is stopped.

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use bitcoin::bip32::{Xpriv, Xpub};
use bitcoin::secp256k1::Secp256k1;
use bitcoin::{Address, BlockHash, Network, ScriptBuf, Txid};
use openbmt_core::batch::{BatchBuilder, BatchPolicy, SignedBatch};
use openbmt_core::identity::{DeviceId, Identity};
use openbmt_core::records::DeviceList;
use openbmt_core::settings::SignedSettings;
use openbmt_core::wallet::WalletDescription;
use openbmt_counter_node::DriverPort;
use openbmt_courier::{Courier, CourierConfig, FirstStart};
use openbmt_link::{ClientConfig, Role, Route};
use openbmt_net_io::{
    Clock, Driver, DriverConfig, EventReceiver, FileStorage, OsRandomness, SystemClock,
};
use openbmt_owner::say::EntryKind;
use openbmt_owner::{
    CashAmount, Delivery, ItemOutcome, OwnerError, SaleFacts, SaleKey, SettingsQueued, Stage,
};
use openbmt_owner_node::coins::ViewState;
use openbmt_owner_node::live::DeepLine;
use openbmt_owner_node::say::Alert as NodeAlert;
use openbmt_owner_node::watcher::write_file;
use openbmt_owner_node::{
    verified_facts, Alert, FactsRefused, MergeRefused, NodeError, OwnerConfig, OwnerLink,
    OwnerLinkConfig, OwnerNode, OwnerParts, OwnerPort, OwnerStartError, ReceiptsLine, RefundIntake,
    SendState,
};
use openbmt_signer::SignerState;
use openbmt_watch::HeaderChain;

use crate::error::{
    from_words, internal, poisoned, refused_by, rule_in, store_by, wrong_state, AppError, Kind,
};
use crate::say::{counter_words, link_alert_words, words_for};
use crate::views::{
    AlertKind, AlertView, BatchArgs, BuiltView, CashAmountView, CashOutArgs, CashOutRow, ClearArgs,
    CounterView, DeepRequestView, DeliverView, EntryKindView, EntryRow, HaltedView, HealthView,
    LinkConfig, LinkStepView, LinkView, MergeOfferView, NoteKindView, OldAddressesView, OldKind,
    OutcomeKind, OutcomeView, PumpView, QueueView, QueuedRefund, ReceiptsKind, ReceiptsView,
    RefundArgs, RefundNoteView, RefundRow, RouteView, SendKind, SendRow, SettingsQueuedView,
    SkippedView, StageKind, StageView, StartConfig, StatusView, TakenView, ViewKind, WordsView,
};

/// The longest a `pump` waits, whatever the app asks for, so that a call
/// from the screen thread is never kept longer than this.
pub const MAX_WAIT_MS: u32 = 250;
/// How long one slice of a `pump` waits for the driver's events.
pub const SLICE_MS: u32 = 20;

/// The file Key 1's state is kept in, in the data folder.
pub const KEY1_FILE: &str = "key1.bin";

/// The driver as the node uses it, boxed so that the node has one type
/// whether it runs on the real driver or on a test's.
pub(crate) struct Port(pub(crate) Box<dyn OwnerPort + Send>);

impl DriverPort for Port {
    fn events(&self) -> &EventReceiver {
        self.0.events()
    }

    fn watch_script(&self, script: ScriptBuf) {
        self.0.watch_script(script);
    }

    fn set_all_blocks(&self, on: bool) {
        self.0.set_all_blocks(on);
    }

    fn shutdown(&mut self) {
        self.0.shutdown();
    }
}

impl OwnerPort for Port {
    fn broadcast(&self, tx: Vec<u8>) {
        self.0.broadcast(tx);
    }

    fn forget(&self, txid: Txid) {
        self.0.forget(txid);
    }

    fn unwatch_scripts(&self, scripts: Vec<ScriptBuf>) {
        self.0.unwatch_scripts(scripts);
    }

    fn rescan_from(&self, height: u32) {
        self.0.rescan_from(height);
    }

    fn set_fetch_transactions(&self, on: bool) {
        self.0.set_fetch_transactions(on);
    }

    fn fetch_block(&self, hash: BlockHash) {
        self.0.fetch_block(hash);
    }
}

/// How the node starts a driver: given the driver's settings, its folder and
/// the scripts it starts with, the driver, or why not. The node starts one
/// for watching and, with a proxy for sending, one for sending.
pub(crate) type Spawn =
    Box<dyn FnMut(DriverConfig, PathBuf, Vec<ScriptBuf>) -> Result<Port, String> + Send>;

/// The real drivers, on the files of the data folder.
pub(crate) fn real_spawn() -> Spawn {
    Box::new(
        |driver: DriverConfig, dir: PathBuf, scripts: Vec<ScriptBuf>| {
            let storage = FileStorage::new(&dir).map_err(|e| e.to_string())?;
            let random = OsRandomness::new().map_err(|e| e.to_string())?;
            let handle = Driver::spawn(driver, storage, SystemClock, random, scripts)
                .map_err(|e| e.to_string())?;
            Ok(Port(Box::new(handle)))
        },
    )
}

/// The node's clock: the latest time any call has given it.
struct AppClock(Arc<AtomicU64>);

impl Clock for AppClock {
    fn now(&self) -> u64 {
        self.0.load(Ordering::SeqCst)
    }
}

/// What lives on the node's thread.
struct State {
    node: OwnerNode<Port>,
    link: Option<OwnerLink>,
    intake: RefundIntake,
    key1: SignerState,
    key1_account: Xpriv,
    policy: BatchPolicy,
    dir: PathBuf,
    network: Network,
}

type Job = Box<dyn FnOnce(&mut Option<State>) + Send>;

/// One running node.
pub struct Entry {
    jobs: Mutex<Option<Sender<Job>>>,
    thread: Mutex<Option<JoinHandle<()>>>,
    poisoned: AtomicBool,
    clock: Arc<AtomicU64>,
}

fn unknown_handle() -> AppError {
    refused_by(
        "handle-unknown",
        "no node has this handle: it was never started or has been stopped",
    )
}

fn bytes32(what: &str, rule: &str, bytes: &[u8]) -> Result<[u8; 32], AppError> {
    <[u8; 32]>::try_from(bytes).map_err(|_| {
        refused_by(
            rule,
            format!("{what} is not 32 bytes, it is {}", bytes.len()),
        )
    })
}

fn device(what: &str, rule: &str, bytes: &[u8]) -> Result<DeviceId, AppError> {
    DeviceId::from_bytes(&bytes32(what, rule, bytes)?)
        .map_err(|e| refused_by(rule, format!("{what}: {e}")))
}

/// What a node call refused with, as the error the app gets.
pub(crate) fn node_error(e: &NodeError) -> AppError {
    let (text, words) = (e.to_string(), e.words());
    match e {
        NodeError::Start(OwnerStartError::Locked(_)) => {
            from_words(Kind::Refused, "NR-79", text, words)
        }
        NodeError::Start(_) => from_words(Kind::Store, "start-owner", text, words),
        NodeError::Files(_) => from_words(Kind::Store, "owner-files", text, words),
        NodeError::Config(_) => from_words(Kind::Refused, "owner-config", text, words),
        NodeError::NotReady(_) => {
            let rule = rule_in(&text, "not-ready");
            from_words(Kind::State, rule, text, words)
        }
        NodeError::Owner(OwnerError::Refused(_)) => {
            let rule = rule_in(&text, "queue-refused");
            from_words(Kind::Refused, rule, text, words)
        }
        NodeError::Owner(OwnerError::Store(_)) => {
            from_words(Kind::Store, "queue-store", text, words)
        }
        NodeError::WatchLimit { .. } => from_words(Kind::Refused, "watch-limit", text, words),
        NodeError::KeyRecoveryRequired => from_words(Kind::State, "key-recovery", text, words),
    }
}

fn queue_error(e: &OwnerError<String>) -> AppError {
    node_error(&NodeError::from(e.clone()))
}

fn facts_error(e: FactsRefused) -> AppError {
    let kind = if e.waits() {
        Kind::State
    } else {
        Kind::Refused
    };
    let text = e.to_string();
    from_words(kind, rule_in(&text, "OC-14"), text, e.words())
}

fn merge_error(e: &MergeRefused) -> AppError {
    let (text, words) = (e.to_string(), e.words());
    match e {
        MergeRefused::NoOffer => from_words(Kind::State, "OC-19", text, words),
        MergeRefused::OtherPaymentsWaiting => from_words(Kind::State, "OC-20", text, words),
        MergeRefused::NotBuilt(_) => from_words(Kind::Refused, "OC-20", text, words),
        MergeRefused::Node(n) => node_error(n),
    }
}

fn words(w: openbmt_core::say::Words) -> WordsView {
    WordsView(w)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// An address of this network, as the script it pays to. The check is
/// here, never in the app.
fn script_of(address: &str, network: Network) -> Result<ScriptBuf, AppError> {
    Address::from_str(address)
        .map_err(|e| refused_by("address-invalid", format!("{address:?}: {e}")))?
        .require_network(network)
        .map(|a| a.script_pubkey())
        .map_err(|e| refused_by("address-invalid", format!("{address:?}: {e}")))
}

fn address_of(script: &ScriptBuf, network: Network) -> String {
    Address::from_script(script, network).map_or_else(|_| hex(script.as_bytes()), |a| a.to_string())
}

fn send_row(txid: &Txid, state: SendState) -> SendRow {
    let (kind, n) = match state {
        SendState::Waiting => (SendKind::Waiting, 0),
        SendState::Sent => (SendKind::Sent, 0),
        SendState::SeenBack { groups } => (
            SendKind::SeenBack,
            u32::try_from(groups).unwrap_or(u32::MAX),
        ),
        SendState::NotSeen => (SendKind::NotSeen, 0),
        SendState::Confirmed { depth } => (SendKind::Confirmed, depth),
        SendState::Done => (SendKind::Done, 0),
        SendState::Replaced => (SendKind::Replaced, 0),
        SendState::Unresolved => (SendKind::Unresolved, 0),
    };
    SendRow {
        txid: txid.to_string(),
        state: kind,
        n,
        words: words(state.words()),
    }
}

fn stage_view(stage: &Stage, kind: EntryKind) -> StageView {
    let (k, detail) = match stage {
        Stage::Queued => (StageKind::Queued, None),
        Stage::InSession { session } => (StageKind::InSession, Some(hex(session))),
        Stage::Returned { reason } => (StageKind::Returned, Some(reason.clone())),
        Stage::Signed { txid } => (StageKind::Signed, Some(txid.to_string())),
        Stage::Sent { txid } => (StageKind::Sent, Some(txid.to_string())),
        Stage::Paid { txid } => (StageKind::Paid, Some(txid.to_string())),
    };
    StageView {
        kind: k,
        detail,
        words: words(stage.words(kind)),
    }
}

fn cash_view(a: CashAmount) -> CashAmountView {
    match a {
        CashAmount::Sats(sats) => CashAmountView::Sats { sats },
        CashAmount::All => CashAmountView::All,
    }
}

fn session_id(bytes: &[u8]) -> Result<[u8; 32], AppError> {
    bytes32("a session number", "id-length", bytes)
}

fn built_view(built: openbmt_owner::Built) -> BuiltView {
    BuiltView {
        session_id: built.session_id.to_vec(),
        request: built.request,
        skipped: built
            .skipped
            .iter()
            .map(|s| SkippedView {
                id: s.id,
                reason: s.reason.clone(),
                words: words(s.words()),
            })
            .collect(),
    }
}

fn route_of(route: &RouteView) -> Result<Route, AppError> {
    let addr = |text: &str| -> Result<SocketAddr, AppError> {
        text.parse()
            .map_err(|e| refused_by("start-link", format!("the address {text:?}: {e}")))
    };
    Ok(match route {
        RouteView::Direct { addr: a } => Route::Direct(addr(a)?),
        RouteView::Onion { proxy, host, port } => Route::Onion {
            proxy: addr(proxy)?,
            host: host.clone(),
            port: u16::try_from(*port)
                .map_err(|_| refused_by("start-link", "the onion port is not a port"))?,
        },
    })
}

/// The link to the counters, made as the regtest command of the owner node
/// makes it: the courier on the data folder, a device list of the owner and
/// the counters.
fn start_link(
    cfg: &LinkConfig,
    dir: &std::path::Path,
    identity: Identity,
    owner: DeviceId,
) -> Result<OwnerLink, AppError> {
    let relay = device("the relay key", "start-link", &cfg.relay_key)?;
    let mut counters = Vec::new();
    for c in &cfg.counters {
        counters.push(device("a counter's root", "start-link", c)?);
    }
    let first = if ["link-inbox", "link-outbox"]
        .iter()
        .any(|f| dir.join(f).exists())
    {
        FirstStart::No
    } else {
        FirstStart::Yes
    };
    let peers: BTreeMap<DeviceId, Role> = counters.iter().map(|c| (*c, Role::Counter)).collect();
    let courier = Courier::start(
        CourierConfig {
            dir: dir.to_path_buf(),
            identity,
            role: Role::Owner,
            peers,
            route: route_of(&cfg.route)?,
            relay,
            client: ClientConfig::default(),
        },
        first,
    )
    .map_err(|e| store_by("start-link-files", e))?;
    let mut link_cfg = OwnerLinkConfig::default();
    if cfg.heartbeat_every_secs > 0 {
        link_cfg.heartbeat_every_secs = cfg.heartbeat_every_secs;
    }
    if cfg.check_in_grace_secs > 0 {
        link_cfg.check_in_grace_secs = cfg.check_in_grace_secs;
    }
    let devices = DeviceList::new(std::iter::once(owner).chain(counters.iter().copied()));
    OwnerLink::new(courier, dir, devices, counters, link_cfg)
        .map_err(|e| store_by("start-link-files", e))
}

/// Everything `start` makes, on the node's thread.
fn build(req: StartConfig, spawn: Spawn, clock: &Arc<AtomicU64>) -> Result<State, AppError> {
    if req.dir.is_empty() {
        return Err(refused_by("start-dir", "the data folder is empty"));
    }
    let network = req.network.network();
    let secret = bytes32("the owner secret", "start-secret", &req.owner_secret)?;
    let identity = Identity::from_secret_bytes(&secret)
        .map_err(|e| refused_by("start-secret", format!("the owner secret: {e}")))?;
    let approver = device("the Key app root", "start-roots", &req.approver_root)?;
    let wallet = WalletDescription::parse(&req.wallet, network)
        .map_err(|e| refused_by("start-wallet", format!("the wallet: {e}")))?;
    let secp = Secp256k1::new();
    let key1_account = Xpriv::decode(&req.key1_account)
        .map_err(|e| refused_by("start-key1", format!("the account key: {e}")))?;
    let key1_xpub = Xpub::from_priv(&secp, &key1_account);
    let key3 = req
        .key3_xpub
        .as_deref()
        .map(Xpub::from_str)
        .transpose()
        .map_err(|e| refused_by("start-key1", format!("Key 3's public key: {e}")))?;
    let mut driver = DriverConfig::new(network, 0);
    driver.proxy = req
        .proxy
        .as_deref()
        .map(str::parse)
        .transpose()
        .map_err(|e| refused_by("start-proxy", format!("the proxy address: {e}")))?;
    for peer in &req.fixed_peers {
        driver
            .fixed_peers
            .push(peer.parse().map_err(|e| {
                refused_by("start-peer", format!("the peer address {peer:?}: {e}"))
            })?);
    }
    let broadcast_proxy = req
        .broadcast_proxy
        .as_deref()
        .map(str::parse)
        .transpose()
        .map_err(|e| refused_by("start-proxy", format!("the sending proxy address: {e}")))?;
    let policy = BatchPolicy {
        min_count: req.batch_min_count,
    };
    clock.fetch_max(req.now, Ordering::SeqCst);
    let dir = PathBuf::from(&req.dir);
    let mut config = OwnerConfig::new(dir.clone(), identity, driver);
    config.clock = Box::new(AppClock(Arc::clone(clock)));
    config.birthday = req.birthday;
    config.broadcast_proxy = broadcast_proxy;
    config.broadcast_without_tor = req.without_tor;
    let mut parts =
        OwnerParts::new(secret, wallet.clone(), approver, policy).map_err(|e| node_error(&e))?;
    parts.refund_depth = req.refund_depth;
    let mut node = OwnerNode::start_with(config, parts, spawn).map_err(|e| node_error(&e))?;
    // From here on a failure lets the node go before it is reported.
    let rest = (|| {
        let key1 = match std::fs::read(dir.join(KEY1_FILE)) {
            Ok(bytes) => SignerState::from_bytes(&bytes, network, &key1_xpub)
                .map_err(|e| refused_by("start-key1", format!("{KEY1_FILE}: {e}")))?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => SignerState::register(
                wallet.canonical_string(),
                network,
                &key1_xpub,
                key3.as_ref(),
            )
            .map_err(|e| refused_by("start-key1", format!("registering Key 1: {e}")))?,
            Err(e) => return Err(store_by("owner-files", format!("{KEY1_FILE}: {e}"))),
        };
        let link = req
            .link
            .as_ref()
            .map(|cfg| {
                let identity = Identity::from_secret_bytes(&secret)
                    .map_err(|e| refused_by("start-secret", format!("the owner secret: {e}")))?;
                let owner = identity.device_id();
                start_link(cfg, &dir, identity, owner)
            })
            .transpose()?;
        let intake = RefundIntake::open(&dir).map_err(|e| store_by("start-intake", e))?;
        Ok((key1, link, intake))
    })();
    match rest {
        Ok((key1, link, intake)) => Ok(State {
            node,
            link,
            intake,
            key1,
            key1_account,
            policy,
            dir,
            network,
        }),
        Err(e) => {
            node.shutdown();
            Err(e)
        }
    }
}

fn run_thread(
    req: StartConfig,
    spawn: Spawn,
    clock: Arc<AtomicU64>,
    ready: Sender<Result<(), AppError>>,
    jobs: std::sync::mpsc::Receiver<Job>,
) {
    let built = catch_unwind(AssertUnwindSafe(|| build(req, spawn, &clock)))
        .unwrap_or_else(|_| Err(internal("caught a panic: [redacted]")));
    let mut slot = match built {
        Ok(state) => {
            let _ = ready.send(Ok(()));
            Some(state)
        }
        Err(e) => {
            let _ = ready.send(Err(e));
            return;
        }
    };
    while let Ok(job) = jobs.recv() {
        job(&mut slot);
    }
    // The entry was let go without a stop: the node goes with the thread.
    if let Some(state) = slot.as_mut() {
        state.node.shutdown();
    }
}

impl Entry {
    /// Start a node from the app's request.
    pub(crate) fn start(req: StartConfig, spawn: Spawn) -> Result<Self, AppError> {
        let clock = Arc::new(AtomicU64::new(0));
        let (jobs_tx, jobs_rx) = channel::<Job>();
        let (ready_tx, ready_rx) = channel();
        let thread_clock = Arc::clone(&clock);
        let thread = std::thread::Builder::new()
            .name("openbmt-owner-node".to_string())
            .spawn(move || run_thread(req, spawn, thread_clock, ready_tx, jobs_rx))
            .map_err(|e| {
                from_words(
                    Kind::Internal,
                    "start-thread",
                    e,
                    words_for(Kind::Internal, "start-thread"),
                )
            })?;
        let ready = ready_rx
            .recv()
            .unwrap_or_else(|_| Err(internal("the node's thread ended before it started")));
        if let Err(e) = ready {
            let _ = thread.join();
            return Err(e);
        }
        Ok(Self {
            jobs: Mutex::new(Some(jobs_tx)),
            thread: Mutex::new(Some(thread)),
            poisoned: AtomicBool::new(false),
            clock,
        })
    }

    /// Run `f` on the node's thread and wait for its result. The time, when
    /// the call has one, raises the clock first.
    fn run<R: Send + 'static>(
        &self,
        now: Option<u64>,
        f: impl FnOnce(&mut State) -> Result<R, AppError> + Send + 'static,
    ) -> Result<R, AppError> {
        if self.poisoned.load(Ordering::SeqCst) {
            return Err(poisoned());
        }
        if let Some(now) = now {
            self.clock.fetch_max(now, Ordering::SeqCst);
        }
        let (tx, rx) = channel();
        let job: Job = Box::new(move |slot: &mut Option<State>| {
            let out = match slot.as_mut() {
                None => Ok(Err(unknown_handle())),
                Some(state) => catch_unwind(AssertUnwindSafe(|| f(state))),
            };
            let _ = tx.send(out);
        });
        let sender = self
            .jobs
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
            .ok_or_else(unknown_handle)?;
        sender.send(job).map_err(|_| unknown_handle())?;
        match rx.recv() {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => {
                self.poisoned.store(true, Ordering::SeqCst);
                Err(internal("caught a panic: [redacted]"))
            }
            Err(_) => Err(unknown_handle()),
        }
    }

    /// Stop the drivers and let go of the data folder. Returns what the
    /// last events reported. The entry takes no call after this.
    pub(crate) fn stop(&self) -> Result<PumpView, AppError> {
        let (tx, rx) = channel();
        let job: Job = Box::new(move |slot: &mut Option<State>| {
            let out = slot.take().map(|mut state| {
                catch_unwind(AssertUnwindSafe(|| {
                    state.node.shutdown();
                    PumpView {
                        notes: state.node.watcher_mut().take_notes(),
                        stopped: state.node.watcher().stopped(),
                    }
                }))
            });
            // The node, and with it the data folder's lock, is let go here.
            drop(slot.take());
            let _ = tx.send(out);
        });
        let sender = self
            .jobs
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
            .ok_or_else(unknown_handle)?;
        sender.send(job).map_err(|_| unknown_handle())?;
        drop(sender);
        let result = rx.recv();
        if let Some(thread) = self.thread.lock().unwrap_or_else(|e| e.into_inner()).take() {
            let _ = thread.join();
        }
        match result {
            Ok(Some(Ok(view))) => Ok(view),
            Ok(Some(Err(_))) => Err(internal("caught a panic: [redacted]")),
            Ok(None) | Err(_) => Err(unknown_handle()),
        }
    }

    /// Take the driver's events for up to `wait_ms` (at most
    /// [`MAX_WAIT_MS`]) and return the lines they logged. Returns as soon as
    /// there is a line or the driver has stopped.
    pub(crate) fn pump(&self, now: u64, wait_ms: u32) -> Result<PumpView, AppError> {
        let wait = Duration::from_millis(u64::from(wait_ms.min(MAX_WAIT_MS)));
        let deadline = Instant::now() + wait;
        let mut out = PumpView {
            notes: Vec::new(),
            stopped: false,
        };
        loop {
            let slice = deadline
                .saturating_duration_since(Instant::now())
                .min(Duration::from_millis(u64::from(SLICE_MS)));
            let (notes, stopped) = self.run(Some(now), move |st| {
                st.node.pump(slice);
                Ok((
                    st.node.watcher_mut().take_notes(),
                    st.node.watcher().stopped(),
                ))
            })?;
            out.notes.extend(notes);
            out.stopped = stopped;
            if !out.notes.is_empty() || stopped || Instant::now() >= deadline {
                break;
            }
            // Let a call that waits take its turn.
            std::thread::sleep(Duration::from_millis(1));
        }
        out.notes.truncate(openbmt_owner_node::NOTE_LOG);
        Ok(out)
    }

    /// One turn of the link, and the refund requests it brought, applied to
    /// the queue on the wallet's own facts.
    pub(crate) fn link_step(&self, now: u64) -> Result<LinkStepView, AppError> {
        self.run(Some(now), move |st| {
            let State {
                node, link, intake, ..
            } = st;
            let link = link
                .as_mut()
                .ok_or_else(|| wrong_state("link-none", "the node was started without a link"))?;
            link.step(node.watcher_mut(), now);
            let done = node
                .run_refund_intake(intake, link)
                .map_err(|e| node_error(&e))?;
            let mut view = LinkStepView {
                notes: link.take_notes(),
                queued: 0,
                refused: 0,
                withdrawn: 0,
                held: 0,
                waiting: None,
                stopped_early: None,
                reached: link.courier().reachable(),
            };
            for (_, applied) in done {
                view.queued = view.queued.saturating_add(applied.queued);
                view.refused = view.refused.saturating_add(applied.refused);
                view.withdrawn = view.withdrawn.saturating_add(applied.withdrawn);
                view.held = view.held.saturating_add(applied.held);
                if view.waiting.is_none() {
                    view.waiting = applied.waiting.map(|w| words(w.words()));
                }
                if view.stopped_early.is_none() {
                    view.stopped_early = applied.stopped.map(|s| s.to_string());
                }
            }
            view.notes.truncate(openbmt_owner_node::NOTE_LOG);
            Ok(view)
        })
    }

    pub(crate) fn status(&self) -> Result<StatusView, AppError> {
        self.run(None, |st| {
            let node = &st.node;
            let state = node.state();
            let (view, rescan_to) = match state {
                ViewState::Ready => (ViewKind::Ready, None),
                ViewState::Rescanning { to } => (ViewKind::Rescanning, Some(to)),
                ViewState::DeepReorg => (ViewKind::DeepReorg, None),
            };
            let health = match node.watcher().health() {
                None => HealthView {
                    known: false,
                    healthy: false,
                    behind: false,
                    words: words(NodeAlert::NoHealthAnswer.words()),
                },
                Some(h) => HealthView {
                    known: true,
                    healthy: h.healthy,
                    behind: h.behind,
                    words: words(h.words()),
                },
            };
            let recovery_pending = node.take_recovery_pending();
            Ok(StatusView {
                view,
                rescan_to,
                view_words: words(state.words()),
                receipts_words: words(node.view().receipt_state().words()),
                checked_height: node.view().checked(),
                tip_height: node.watcher().chain().tip_height(),
                halted: node.halted().map(|text| HaltedView {
                    text: text.to_string(),
                    words: words(NodeAlert::ChainHalted.words()),
                }),
                health,
                balance_sats: node.balance(),
                recovery_pending,
                recovery_words: recovery_pending
                    .then(|| words(NodeError::KeyRecoveryRequired.words())),
                stopped: node.watcher().stopped(),
            })
        })
    }

    pub(crate) fn receipts_line(&self) -> Result<ReceiptsView, AppError> {
        self.run(None, |st| {
            let line = st.node.receipts_line();
            let (kind, payments, cost_sats, rate_per_kvb) = match line {
                ReceiptsLine::NotReady => (ReceiptsKind::NotReady, 0, 0, 0),
                ReceiptsLine::NoCoins => (ReceiptsKind::NoCoins, 0, 0, 0),
                ReceiptsLine::FeeUnknown { payments } => (ReceiptsKind::FeeUnknown, payments, 0, 0),
                ReceiptsLine::Known {
                    payments,
                    cost_sats,
                    rate_per_kvb,
                } => (ReceiptsKind::Known, payments, cost_sats, rate_per_kvb),
            };
            Ok(ReceiptsView {
                kind,
                payments: u64::try_from(payments).unwrap_or(u64::MAX),
                cost_sats,
                rate_per_kvb,
                words: words(line.words()),
            })
        })
    }

    pub(crate) fn merge_offer(&self) -> Result<Option<MergeOfferView>, AppError> {
        self.run(None, |st| {
            Ok(st.node.merge_offer().map(|o| MergeOfferView {
                coins: u64::try_from(o.coins).unwrap_or(u64::MAX),
                of: u64::try_from(o.of).unwrap_or(u64::MAX),
                rate_per_kvb: o.rate_per_kvb,
                cost_sats: o.cost_sats,
                usual_cost_sats: o.usual_cost_sats,
                saving_sats: o.saving_sats(),
                words: words(o.words()),
            }))
        })
    }

    pub(crate) fn send_states(&self) -> Result<Vec<SendRow>, AppError> {
        self.run(None, |st| {
            Ok(st
                .node
                .send_states()
                .iter()
                .map(|(txid, state)| send_row(txid, *state))
                .collect())
        })
    }

    pub(crate) fn build_request(&self, now: u64, fee_rate: u64) -> Result<BuiltView, AppError> {
        self.run(Some(now), move |st| {
            st.node
                .build_request(fee_rate, now)
                .map(built_view)
                .map_err(|e| node_error(&e))
        })
    }

    pub(crate) fn approve_merge(&self, now: u64) -> Result<BuiltView, AppError> {
        self.run(Some(now), move |st| {
            st.node
                .approve_merge(now)
                .map(built_view)
                .map_err(|e| merge_error(&e))
        })
    }

    /// Take the Key app's response. Key 1's state is written to its file in
    /// the same step, as the node's journal asks.
    pub(crate) fn take(&self, now: u64, response: Vec<u8>) -> Result<TakenView, AppError> {
        self.run(Some(now), move |st| {
            let State {
                node,
                key1,
                key1_account,
                dir,
                ..
            } = st;
            let persist_dir = dir.clone();
            let taken = node
                .take(&response, key1, key1_account, now, move |bytes| {
                    write_file(&persist_dir, KEY1_FILE, bytes)
                })
                .map_err(|e| node_error(&e))?;
            Ok(TakenView {
                session_id: taken.session_id.to_vec(),
                outcomes: taken
                    .outcomes
                    .iter()
                    .map(|(item, outcome)| outcome_view(*item, outcome))
                    .collect(),
                sends: node
                    .send_states()
                    .iter()
                    .map(|(txid, state)| send_row(txid, *state))
                    .collect(),
            })
        })
    }

    pub(crate) fn sessions(&self) -> Result<Vec<Vec<u8>>, AppError> {
        self.run(None, |st| {
            Ok(st
                .node
                .owner()
                .open_sessions()
                .iter()
                .map(|id| id.to_vec())
                .collect())
        })
    }

    pub(crate) fn session_request(&self, id: Vec<u8>) -> Result<Vec<u8>, AppError> {
        let id = session_id(&id)?;
        self.run(None, move |st| {
            st.node
                .owner()
                .session_request(&id)
                .map(<[u8]>::to_vec)
                .ok_or_else(|| refused_by("session-unknown", "no open approval has this number"))
        })
    }

    pub(crate) fn abandon_session(&self, id: Vec<u8>) -> Result<(), AppError> {
        let id = session_id(&id)?;
        self.run(None, move |st| {
            if !st.node.owner().open_sessions().contains(&id) {
                return Err(refused_by(
                    "session-unknown",
                    "no open approval has this number",
                ));
            }
            let write = st.node.queue_writer();
            st.node
                .owner_mut()
                .map_err(|e| node_error(&e))?
                .abandon_session(&id, &write)
                .map_err(|e| queue_error(&e))
        })
    }

    pub(crate) fn queue(&self) -> Result<QueueView, AppError> {
        self.run(None, |st| {
            let network = st.network;
            let owner = st.node.owner();
            let refunds = owner
                .refunds()
                .iter()
                .map(|r| RefundRow {
                    id: r.id,
                    counter: r.sale.counter.to_bytes().to_vec(),
                    sale: r.sale.index,
                    cashier: r.cashier.clone(),
                    address: address_of(&r.script, network),
                    sats: r.sats,
                    stage: stage_view(&r.stage, EntryKind::Refund),
                    waiting_for_depth: owner.waiting_for_depth(r.id),
                    received_sats: owner.sale_facts(&r.sale).map(|f| f.received_sats),
                })
                .collect();
            let cash_outs = owner
                .cash_outs()
                .iter()
                .map(|c| CashOutRow {
                    id: c.id,
                    address: address_of(&c.script, network),
                    amount: cash_view(c.amount),
                    label: c.label.clone(),
                    stage: stage_view(&c.stage, EntryKind::CashOut),
                })
                .collect();
            let mut others: Vec<EntryRow> = Vec::new();
            others.extend(owner.batches().iter().map(|b| EntryRow {
                id: b.id,
                kind: EntryKindView::Batch,
                stage: stage_view(&b.stage, EntryKind::Batch),
            }));
            others.extend(owner.queued_settings().iter().map(|s| EntryRow {
                id: s.id,
                kind: EntryKindView::SettingsChange,
                stage: stage_view(&s.stage, EntryKind::SettingsChange),
            }));
            others.extend(owner.pairings().iter().map(|p| EntryRow {
                id: p.id,
                kind: EntryKindView::Pairing,
                stage: stage_view(&p.stage, EntryKind::Pairing),
            }));
            Ok(QueueView {
                refunds,
                cash_outs,
                others,
            })
        })
    }

    /// Queue a refund on the facts of the wallet's own view (OC-14): what
    /// the sale received and whether it is at the refund depth come from the
    /// checked chain, never from the counter's records or from the app.
    pub(crate) fn queue_refund(&self, now: u64, a: RefundArgs) -> Result<QueuedRefund, AppError> {
        let counter = device("the counter's root", "id-length", &a.counter)?;
        self.run(Some(now), move |st| {
            let script = script_of(&a.address, st.network)?;
            let key = SaleKey {
                counter,
                index: a.sale,
            };
            let tip = st.node.watcher().chain().tip_height();
            let facts: SaleFacts = verified_facts(
                st.node.view(),
                st.node.owner(),
                key,
                tip,
                st.node.refund_depth(),
            )
            .map_err(facts_error)?;
            let write = st.node.queue_writer();
            let owner = st.node.owner_mut().map_err(|e| node_error(&e))?;
            owner
                .set_sale_facts(key, facts, &write)
                .map_err(|e| queue_error(&e))?;
            let id = owner
                .queue_refund(key, &a.cashier, script, a.sats, None, &write)
                .map_err(|e| queue_error(&e))?;
            Ok(QueuedRefund {
                id,
                received_sats: facts.received_sats,
                reached_refund_depth: facts.reached_refund_depth,
            })
        })
    }

    pub(crate) fn cancel(&self, id: u32) -> Result<(), AppError> {
        self.run(None, move |st| {
            let write = st.node.queue_writer();
            st.node
                .owner_mut()
                .map_err(|e| node_error(&e))?
                .cancel(id, &write)
                .map_err(|e| queue_error(&e))
        })
    }

    pub(crate) fn requeue(&self, id: u32) -> Result<(), AppError> {
        self.run(None, move |st| {
            let write = st.node.queue_writer();
            st.node
                .owner_mut()
                .map_err(|e| node_error(&e))?
                .requeue(id, &write)
                .map_err(|e| queue_error(&e))
        })
    }

    pub(crate) fn cash_out(&self, a: CashOutArgs) -> Result<u32, AppError> {
        self.run(None, move |st| {
            let script = script_of(&a.address, st.network)?;
            let amount = match a.amount {
                CashAmountView::Sats { sats } => CashAmount::Sats(sats),
                CashAmountView::All => CashAmount::All,
            };
            let write = st.node.queue_writer();
            st.node
                .owner_mut()
                .map_err(|e| node_error(&e))?
                .queue_cash_out(script, amount, &a.label, &write)
                .map_err(|e| queue_error(&e))
        })
    }

    /// Sign a batch of addresses for a counter and queue it. The wallet
    /// derives the batch; the owner node watches the new addresses.
    pub(crate) fn sign_batch(&self, now: u64, a: BatchArgs) -> Result<u32, AppError> {
        let counter = device("the counter's root", "id-length", &a.counter)?;
        self.run(Some(now), move |st| {
            let batch = BatchBuilder::new(st.policy)
                .derive(st.node.owner().wallet(), counter, a.start, a.count)
                .map_err(|e| refused_by("batch-not-derived", e))?;
            let write = st.node.queue_writer();
            let id = st
                .node
                .owner_mut()
                .map_err(|e| node_error(&e))?
                .queue_batch(batch, now, &write)
                .map_err(|e| queue_error(&e))?;
            st.node.refresh_marks();
            Ok(id)
        })
    }

    /// Queue a change of the shop's settings. The bytes are a
    /// `SignedSettings`; its signatures are ignored and the owner signs the
    /// settings it holds (AS-59).
    pub(crate) fn queue_settings(&self, bytes: Vec<u8>) -> Result<SettingsQueuedView, AppError> {
        let signed = SignedSettings::from_bytes(&bytes)
            .map_err(|e| refused_by("settings-bytes", format!("not settings: {e}")))?;
        self.run(None, move |st| {
            let write = st.node.queue_writer();
            let queued = st
                .node
                .owner_mut()
                .map_err(|e| node_error(&e))?
                .queue_settings(signed.settings, &write)
                .map_err(|e| queue_error(&e))?;
            Ok(match queued {
                SettingsQueued::Queued(id) => SettingsQueuedView {
                    tightening: false,
                    id,
                    signed: None,
                },
                SettingsQueued::Tightening { id, signed } => SettingsQueuedView {
                    tightening: true,
                    id,
                    signed: Some(signed.to_bytes()),
                },
            })
        })
    }

    /// Hand every signed batch in the outbox to the link, and mark it
    /// delivered once the courier holds it.
    pub(crate) fn deliver(&self, now: u64) -> Result<DeliverView, AppError> {
        self.run(Some(now), move |st| {
            let State { node, link, .. } = st;
            let link = link
                .as_mut()
                .ok_or_else(|| wrong_state("link-none", "the node was started without a link"))?;
            let mut view = DeliverView {
                sent: 0,
                waiting: 0,
                notes: Vec::new(),
            };
            let items: Vec<(u32, Delivery)> = node
                .owner()
                .outbox()
                .iter()
                .map(|d| (d.id, d.what.clone()))
                .collect();
            for (id, what) in items {
                match what {
                    Delivery::Batch(bytes) => {
                        let batch = match SignedBatch::from_bytes(&bytes) {
                            Ok(b) => b,
                            Err(e) => {
                                view.notes
                                    .push(format!("batch {id} does not read back: {e}"));
                                continue;
                            }
                        };
                        if let Err(e) = link.send_batch(&batch.batch.counter, &batch) {
                            view.notes
                                .push(format!("batch {id} was not handed to the link: {e}"));
                            continue;
                        }
                        let write = node.queue_writer();
                        match node
                            .owner_mut()
                            .map_err(|e| node_error(&e))
                            .and_then(|o| o.mark_delivered(id, &write).map_err(|e| queue_error(&e)))
                        {
                            Ok(()) => view.sent += 1,
                            Err(e) => view
                                .notes
                                .push(format!("batch {id} could not be marked: {e}")),
                        }
                    }
                    Delivery::Settings(_) | Delivery::Pairing(_) => view.waiting += 1,
                    Delivery::Transaction { .. } => {}
                }
            }
            Ok(view)
        })
    }

    pub(crate) fn link_state(&self) -> Result<LinkView, AppError> {
        self.run(None, |st| {
            let link = st
                .link
                .as_ref()
                .ok_or_else(|| wrong_state("link-none", "the node was started without a link"))?;
            let alerts = link
                .alerts()
                .iter()
                .map(|a| {
                    let (kind, counter, value) = match a {
                        Alert::MissedCheckIn { counter, last_at } => {
                            (AlertKind::MissedCheckIn, counter, *last_at)
                        }
                        Alert::Diverged { counter, seq } => (AlertKind::Diverged, counter, *seq),
                    };
                    AlertView {
                        kind,
                        counter: counter.to_bytes().to_vec(),
                        value,
                        words: words(link_alert_words(a)),
                    }
                })
                .collect();
            let counters = link
                .counters()
                .iter()
                .filter_map(|c| {
                    let s = link.state(c)?;
                    Some(CounterView {
                        counter: c.to_bytes().to_vec(),
                        records: s.records,
                        head: s.head.to_vec(),
                        check_in_seq: s.check_in_seq,
                        check_in_held: s.check_in_held,
                        last_check_in_at: s.last_check_in_at,
                        words: words(counter_words(
                            s.check_in_seq,
                            s.check_in_held,
                            s.last_check_in_at,
                        )),
                    })
                })
                .collect();
            Ok(LinkView {
                reachable: link.courier().reachable(),
                refused: link.courier().refused(),
                alerts,
                counters,
            })
        })
    }

    pub(crate) fn clear_alert(&self, a: ClearArgs) -> Result<(), AppError> {
        let counter = device("the counter's root", "id-length", &a.counter)?;
        self.run(None, move |st| {
            let link = st
                .link
                .as_mut()
                .ok_or_else(|| wrong_state("link-none", "the node was started without a link"))?;
            let alert = match a.kind {
                AlertKind::MissedCheckIn => Alert::MissedCheckIn {
                    counter,
                    last_at: a.value,
                },
                AlertKind::Diverged => Alert::Diverged {
                    counter,
                    seq: a.value,
                },
            };
            if !link.alerts().contains(&alert) {
                return Err(refused_by("alert-unknown", "the alert is not raised"));
            }
            link.clear(&alert);
            Ok(())
        })
    }

    pub(crate) fn refund_notes(&self) -> Result<Vec<RefundNoteView>, AppError> {
        self.run(None, |st| {
            Ok(st
                .intake
                .notes()
                .iter()
                .enumerate()
                .map(|(i, n)| RefundNoteView {
                    index: u32::try_from(i).unwrap_or(u32::MAX),
                    counter: n.counter.to_bytes().to_vec(),
                    seq: n.seq,
                    number: n.number,
                    sale: n.sale,
                    kind: match n.kind {
                        openbmt_owner_node::NoteKind::Refused => NoteKindView::Refused,
                        openbmt_owner_node::NoteKind::WithdrawalNotApplied => {
                            NoteKindView::WithdrawalNotApplied
                        }
                        openbmt_owner_node::NoteKind::Interrupted => NoteKindView::Interrupted,
                        openbmt_owner_node::NoteKind::ClaimedMoreThanSeen => {
                            NoteKindView::ClaimedMoreThanSeen
                        }
                    },
                    withdrawn: n.withdrawn,
                    words: words(n.words()),
                })
                .collect())
        })
    }

    pub(crate) fn dismiss_refund_note(&self, index: u32) -> Result<(), AppError> {
        self.run(None, move |st| {
            let at = usize::try_from(index).unwrap_or(usize::MAX);
            if at >= st.intake.notes().len() {
                return Err(refused_by("note-unknown", "no such line"));
            }
            st.intake
                .dismiss(at)
                .map_err(|e| store_by("start-intake", e))
        })
    }

    pub(crate) fn notes(&self) -> Result<Vec<String>, AppError> {
        self.run(None, |st| {
            let mut notes = st.node.watcher_mut().take_notes();
            if let Some(link) = st.link.as_mut() {
                notes.extend(link.take_notes());
            }
            notes.truncate(openbmt_owner_node::NOTE_LOG);
            Ok(notes)
        })
    }

    /// Check again from the wallet's first block.
    pub(crate) fn rescan(&self) -> Result<(), AppError> {
        self.run(None, |st| st.node.rescan().map_err(|e| node_error(&e)))
    }

    /// Roll a committed approval forward into Key 1's state.
    pub(crate) fn recover_key1(&self) -> Result<(), AppError> {
        self.run(None, |st| {
            let State {
                node,
                key1,
                key1_account,
                dir,
                ..
            } = st;
            let persist_dir = dir.clone();
            node.recover_key1(key1, key1_account, move |bytes| {
                write_file(&persist_dir, KEY1_FILE, bytes)
            })
            .map_err(|e| node_error(&e))
        })
    }

    /// Archive the paid rows that no reorganization can reopen.
    pub(crate) fn archive_paid(&self) -> Result<u32, AppError> {
        self.run(None, |st| {
            st.node
                .archive_paid()
                .map(|n| u32::try_from(n).unwrap_or(u32::MAX))
                .map_err(|e| node_error(&e))
        })
    }

    /// OC-25: what the owner is told about the old addresses.
    pub(crate) fn old_addresses(&self) -> Result<OldAddressesView, AppError> {
        self.run(None, |st| {
            let running = st.node.deep_running();
            let line = st.node.old_addresses_line();
            let (kind, count, days_ago, next) = match line {
                None => (OldKind::None, 0, 0, None),
                Some(DeepLine::FirstCheckWaiting { count, next }) => {
                    (OldKind::FirstCheckWaiting, count, 0, Some(next))
                }
                Some(DeepLine::Monthly {
                    count,
                    days_ago,
                    next,
                }) => (OldKind::Monthly, count, days_ago, Some(next)),
                Some(DeepLine::Checking { count }) => (OldKind::Checking, count, 0, None),
                Some(DeepLine::Failed { count, next }) => (OldKind::Failed, count, 0, Some(next)),
            };
            Ok(OldAddressesView {
                kind,
                count,
                days_ago,
                next,
                running,
                words: line.map(|l| words(l.words())),
            })
        })
    }

    /// OC-24: ask for a deep check of the old addresses now.
    pub(crate) fn request_deep_check(&self, now: u64) -> Result<DeepRequestView, AppError> {
        self.run(Some(now), |st| {
            let asked = st.node.request_deep_check().map_err(|e| node_error(&e))?;
            let words = match st.node.old_addresses_line() {
                Some(line) if asked => line.words(),
                _ => words_for(Kind::State, "deep-none"),
            };
            Ok(DeepRequestView {
                asked,
                words: WordsView(words),
            })
        })
    }

    /// Run `f` as a call on the node's thread, for a test of what a panic
    /// does.
    #[cfg(test)]
    pub(crate) fn run_for_test(&self, f: impl FnOnce() + Send + 'static) -> Result<(), AppError> {
        self.run(None, move |_| {
            f();
            Ok(())
        })
    }
}

fn outcome_view(item: u32, outcome: &ItemOutcome) -> OutcomeView {
    let (kind, finished, reason) = match outcome {
        ItemOutcome::Delivered(id) => (OutcomeKind::Delivered, *id, None),
        ItemOutcome::Returned(reason) => (OutcomeKind::Returned, 0, Some(reason.clone())),
        ItemOutcome::Healthy => (OutcomeKind::Healthy, 0, None),
    };
    OutcomeView {
        item,
        kind,
        finished,
        reason,
        words: words(outcome.words()),
    }
}
