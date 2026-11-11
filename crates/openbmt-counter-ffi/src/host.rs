//! The node behind a lock, and every call the app can make on it.
//!
//! The counter node owns sockets and a driver thread. The app calls
//! [`Entry::pump`] in a loop from one background thread and every other call
//! from its screen thread, so one [`Node`] is shared by two threads.
//!
//! - The node sits in a `Mutex`. Every call takes it, runs, and lets it go.
//! - `pump` never holds it for long: its wait is cut to [`MAX_WAIT_MS`] and
//!   is taken in slices of [`SLICE_MS`]. The node can wait for the driver's
//!   events only while it holds the lock, so each slice holds it for at
//!   most [`SLICE_MS`] and then lets it go and sleeps a moment, which is
//!   when a call from the screen thread gets in. A call from the screen
//!   thread waits for at most one slice and the work of the call before it.
//! - Nothing here keeps a JNI reference or the environment of a call.
//!
//! The node's clock is the app's: every call that takes a time raises the
//! clock to it, and the clock never goes back, so two threads that read
//! their clocks a moment apart cannot make the node's time step back.
//!
//! A panic in a call is caught, becomes an internal error and poisons the
//! entry: every later call returns the same error until the node is stopped.

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use std::sync::mpsc::Receiver;

use bitcoin::{Network, ScriptBuf, Txid};
use openbmt_core::batch::{BatchPolicy, SignedBatch};
use openbmt_core::command::SignedCommand;
use openbmt_counter::{
    refund_destination, Actor, Amount, CashRefund, Currency, Rate, RateSource, RefundAsk,
    RefundRequest, Report, Screen, StaffChange, StaffEvent,
};
use openbmt_counter_node::{effective_settings, own_node_setting};
use openbmt_counter_node::{
    DirStore, DriverPort, HeartbeatNotTaken, Launch, Node, NodeConfig, PairRefused, PairingRecord,
    SaleLightning, SettingsFault, SetupStep, StartError, NOTE_LOG,
};
use openbmt_lightning::{
    CertPin, ClnCredential, Credential, Endpoint, LndCredential, Reply as LnReply,
};
use openbmt_net_io::{
    Clock, Driver, DriverConfig, EventReceiver, FileStorage, OsRandomness, OwnNodeEvent,
    SystemClock,
};
use openbmt_watch::HeaderChain;

use crate::error::{from_rule, internal, poisoned, refused_by, wrong_state, CounterError, Kind};
use crate::pair::refused;
use crate::pay::{payment_for, payment_with_invoice, Payment};
use crate::relay::{LinkPlan, LinkSlot, LEASE_WAIT};
use crate::say::words_for;
use crate::views::{
    ActorView, CancelArgs, CashArgs, DayTotalsArgs, DayTotalsView, HealthView, HeartbeatOutcome,
    LightningSaleView, LightningStatusView, LinkStepView, LinkView, LnFailArgs, LnKind,
    LnReplyArgs, LnRequestView, LnSetupArgs, LnStepView, OpenArgs, OpenedView, OverrideArgs,
    OwnNodeView, PumpView, RefundArgs, RefundAskView, RefundRow, ReportView, RequoteArgs,
    ScreenView, StaffArgs, StartConfig, StatusView, WithdrawArgs, WordsView,
};

/// The longest a `pump` waits, whatever the app asks for, so that a call
/// from the screen thread is never kept longer than this.
pub const MAX_WAIT_MS: u32 = 250;
/// How long one slice of a `pump` holds the node's lock.
pub const SLICE_MS: u32 = 20;

/// The driver as the node uses it, boxed so that the node has one type
/// whether it runs on the real driver or on a test's.
pub(crate) struct Port(pub(crate) Box<dyn DriverPort + Send>);

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

    fn own_node_events(&self) -> Option<&Receiver<OwnNodeEvent>> {
        self.0.own_node_events()
    }

    fn ask_own_node(&self, txid: Txid) {
        self.0.ask_own_node(txid);
    }

    fn retry_own_node(&self) {
        self.0.retry_own_node();
    }
}

/// How the node starts its driver: given what the node says it must be
/// started with, the driver, or why not.
pub(crate) type Spawn = Box<dyn FnOnce(Launch) -> Result<Port, String> + Send>;

/// The real driver, on the files of the data folder.
pub(crate) fn real_spawn() -> Spawn {
    Box::new(|launch: Launch| {
        let storage = FileStorage::new(&launch.dir).map_err(|e| e.to_string())?;
        let random = OsRandomness::new().map_err(|e| e.to_string())?;
        let handle = Driver::spawn(launch.driver, storage, SystemClock, random, launch.scripts)
            .map_err(|e| e.to_string())?;
        Ok(Port(Box::new(handle)))
    })
}

/// The node's clock: the latest time any call has given it.
struct AppClock(Arc<AtomicU64>);

impl Clock for AppClock {
    fn now(&self) -> u64 {
        self.0.load(Ordering::SeqCst)
    }
}

struct Inner {
    node: Option<Node<Port>>,
    /// Reports that a call made and could not return, because the call was
    /// then refused. The next `pump` returns them.
    held: Vec<Report>,
}

/// One running node.
pub struct Entry {
    inner: Mutex<Inner>,
    poisoned: AtomicBool,
    network: Network,
    clock: Arc<AtomicU64>,
    own_refused: Option<WordsView>,
    /// The link to the owner, when the pairing record holds a relay
    /// (CR-134).
    link: Option<LinkSlot>,
    /// The entry has been stopped: a step of the link that is at the relay
    /// sends nothing more.
    closing: AtomicBool,
}

fn unknown_handle() -> CounterError {
    refused_by(
        "handle-unknown",
        "no node has this handle: it was never started or has been stopped",
    )
}

fn no_link() -> CounterError {
    wrong_state(
        "link-none",
        "the pairing has no relay, so the counter has no link",
    )
}

fn reports(list: Vec<Report>) -> Vec<ReportView> {
    list.into_iter().map(ReportView).collect()
}

fn actor(a: ActorView) -> Actor {
    Actor {
        role: a.role.role(),
        name: a.name,
    }
}

fn rate_and_source(
    rate: u64,
    label: &str,
    fetched_at: u64,
) -> Result<(Rate, RateSource), CounterError> {
    let rate = Rate::new(rate).ok_or_else(|| refused_by("sale-rate", "the rate is zero"))?;
    let source = RateSource::new(label, fetched_at)
        .map_err(|e| refused_by("sale-source", format!("the rate's source: {e}")))?;
    Ok((rate, source))
}

fn start_error(e: &StartError) -> CounterError {
    let (rule, kind) = match e {
        StartError::Locked(_) => ("NR-79", Kind::Refused),
        StartError::Dir(_) => ("start-folder", Kind::Store),
        StartError::Headers(_) => ("start-headers", Kind::Refused),
        StartError::Counter(l) => (l.rule(), Kind::Refused),
        StartError::Settings(f) => (f.rule(), Kind::Refused),
        StartError::FirstBatch(_) => ("start-first-batch", Kind::Refused),
        StartError::Checked(_) => ("start-checked", Kind::Store),
        StartError::Driver(_) => ("start-driver", Kind::Refused),
    };
    let (text, words) = (e.to_string(), e.words());
    let rule = rule.to_string();
    match kind {
        Kind::Store => CounterError::Store {
            rule,
            text,
            words: Box::new(words),
        },
        _ => CounterError::Refused {
            rule,
            text,
            words: Box::new(words),
        },
    }
}

/// What `start` makes a node of, checked: the pairing and the settings the
/// counter's identity and the owner's phone agreed on.
struct Checked {
    config: NodeConfig,
    network: Network,
    /// CR-100, CR-102: why the own node was not used, when the start was
    /// given one and it was refused.
    own_refused: Option<WordsView>,
    /// CR-134: what the link is made from, when the record holds a relay.
    link: Option<LinkPlan>,
}

fn check_start(
    req: StartConfig,
    policy: BatchPolicy,
    clock: &Arc<AtomicU64>,
) -> Result<Checked, CounterError> {
    if req.dir.is_empty() {
        return Err(refused_by("start-dir", "the data folder is empty"));
    }
    let record =
        PairingRecord::from_bytes(&req.record).map_err(|e| refused(PairRefused::Record(e)))?;
    // CR-94 to CR-96 again, on what was kept: the settings' signatures, the
    // lists, the version and the first batch.
    record.check(&policy).map_err(refused)?;
    let identity = record.identity().map_err(refused)?;
    // CR-163: the settings the owner sent after pairing, from the file, when
    // they are newer than the pairing's; a file that does not read is a fault.
    let settings = DirStore::open(&req.dir)
        .map_err(SettingsFault::Read)
        .and_then(|store| effective_settings(&store, &record))
        .map_err(|f| CounterError::Refused {
            rule: f.rule().to_string(),
            text: f.to_string(),
            words: Box::new(f.words()),
        })?
        .signed
        .settings;
    // CR-87: with Tor on in the verified settings, the node never starts
    // without the proxy, whatever the app's own reading of the settings said.
    if settings.tor_on && req.proxy.is_none() {
        return Err(refused_by(
            "CR-87",
            "CR-87: the settings have Tor on and the start has no proxy",
        ));
    }
    let network = record.network;
    let mut driver = DriverConfig::new(network, 0);
    driver.proxy = req
        .proxy
        .as_deref()
        .map(str::parse)
        .transpose()
        .map_err(|e| refused_by("start-proxy", format!("the proxy address: {e}")))?;
    // CR-134: the link goes through the node's proxy when there is one (the
    // settings have Tor on), and otherwise through Orbot's, which the app
    // gives only for a relay that needs it.
    let link = match record.relay.clone() {
        None => None,
        Some(hint) => {
            let proxy = req
                .link_proxy
                .as_deref()
                .map(str::parse)
                .transpose()
                .map_err(|e| refused_by("start-proxy", format!("the link's proxy address: {e}")))?;
            Some(LinkPlan {
                hint,
                identity: record.identity().map_err(refused)?,
                owner: record.roots.owner,
                proxy: driver.proxy.or(proxy),
                tor_on: settings.tor_on,
                dir: std::path::PathBuf::from(&req.dir),
            })
        }
    };
    // CR-99: the peers are hints; a bad one is dropped by the record's own
    // reading, so none is left here that does not parse.
    for peer in &record.peers {
        driver
            .fixed_peers
            .push(peer.parse().map_err(|e| {
                refused_by("start-peer", format!("the peer address {peer:?}: {e}"))
            })?);
    }
    // CR-100 to CR-102: an own node that is refused, or that has no proxy,
    // leaves the node without one and the status says why; the start itself
    // does not fail for a hint.
    let mut own_refused = None;
    if let Some(text) = req.own_node.as_deref() {
        match own_node_setting(text, record.network, req.proxy.is_some()) {
            Ok(setting) => driver.own_node = Some(setting.node),
            Err(e) => own_refused = Some(WordsView(e.words())),
        }
    }
    let mut pairing = record.pairing();
    pairing.batch_policy = policy;
    clock.fetch_max(req.now, Ordering::SeqCst);
    let mut config = NodeConfig::new(req.dir, identity, pairing, settings, driver);
    // CR-162: a counter paired with a relay tells the owner which settings it
    // holds.
    config.announce_settings = record.relay.is_some();
    config.first_batch = record.first_batch.clone();
    config.clock = Box::new(AppClock(Arc::clone(clock)));
    // CR-115: the saved Lightning connection, if the app has one.
    config.lightning = req.lightning;
    Ok(Checked {
        config,
        network,
        own_refused,
        link,
    })
}

impl Entry {
    /// Start a node from the app's request.
    pub(crate) fn start(
        req: StartConfig,
        spawn: Spawn,
        policy: BatchPolicy,
    ) -> Result<Self, CounterError> {
        let clock = Arc::new(AtomicU64::new(0));
        let Checked {
            config,
            network,
            own_refused,
            link,
        } = check_start(req, policy, &clock)?;
        let node = Node::start_with(config, spawn).map_err(|e| start_error(&e))?;
        let link = link.map(|plan| LinkSlot::start(plan, LEASE_WAIT));
        Ok(Self {
            inner: Mutex::new(Inner {
                node: Some(node),
                held: Vec::new(),
            }),
            poisoned: AtomicBool::new(false),
            network,
            clock,
            own_refused,
            link,
            closing: AtomicBool::new(false),
        })
    }

    /// Run `f` on the node with the lock held. The time, when the call
    /// has one, raises the clock first.
    fn run<R>(
        &self,
        now: Option<u64>,
        f: impl FnOnce(&mut Node<Port>, &mut Vec<Report>, Network) -> Result<R, CounterError>,
    ) -> Result<R, CounterError> {
        if self.poisoned.load(Ordering::SeqCst) {
            return Err(poisoned());
        }
        let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let Inner { node, held } = &mut *guard;
        let node = node.as_mut().ok_or_else(unknown_handle)?;
        if let Some(now) = now {
            self.clock.fetch_max(now, Ordering::SeqCst);
        }
        let network = self.network;
        match catch_unwind(AssertUnwindSafe(|| f(node, held, network))) {
            Ok(result) => result,
            Err(_) => {
                self.poisoned.store(true, Ordering::SeqCst);
                Err(internal("caught a panic: [redacted]"))
            }
        }
    }

    /// Stop the driver and let go of the data folder. Returns what the
    /// last events reported. The entry takes no call after this.
    pub(crate) fn stop(&self) -> Result<PumpView, CounterError> {
        self.closing.store(true, Ordering::SeqCst);
        let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let Inner { node, held } = &mut *guard;
        let mut node = node.take().ok_or_else(unknown_handle)?;
        let result = catch_unwind(AssertUnwindSafe(|| {
            let mut out = std::mem::take(held);
            out.extend(node.shutdown());
            let notes = node.take_notes();
            (out, notes, node.stopped())
        }));
        // The node, and with it the data folder's lock, is let go here.
        drop(node);
        match result {
            Ok((out, notes, stopped)) => Ok(PumpView {
                reports: reports(out),
                notes,
                stopped,
            }),
            Err(_) => Err(internal("caught a panic: [redacted]")),
        }
    }

    /// Take the driver's events for up to `wait_ms` (at most
    /// [`MAX_WAIT_MS`]) and return what must leave the counter. Returns as
    /// soon as there is a report.
    pub(crate) fn pump(&self, now: u64, wait_ms: u32) -> Result<PumpView, CounterError> {
        let wait = Duration::from_millis(u64::from(wait_ms.min(MAX_WAIT_MS)));
        let deadline = Instant::now() + wait;
        let mut out = PumpView {
            reports: Vec::new(),
            notes: Vec::new(),
            stopped: false,
        };
        loop {
            let slice = deadline
                .saturating_duration_since(Instant::now())
                .min(Duration::from_millis(u64::from(SLICE_MS)));
            let step = self.run(Some(now), |node, held, _| {
                let mut got = std::mem::take(held);
                got.extend(node.pump(slice));
                Ok((got, node.take_notes(), node.stopped()))
            })?;
            let (got, notes, stopped) = step;
            out.reports.extend(reports(got));
            out.notes.extend(notes);
            out.stopped = stopped;
            if !out.reports.is_empty() || stopped || Instant::now() >= deadline {
                break;
            }
            // Let a call that waits for the lock take it.
            std::thread::sleep(Duration::from_millis(1));
        }
        // The node's log keeps a bounded number of lines; so does a result.
        out.notes.truncate(NOTE_LOG);
        Ok(out)
    }

    fn payment(&self, node: &Node<Port>, index: u32, sats: u64) -> Result<Payment, CounterError> {
        let sale = node
            .counter()
            .sale(index)
            .ok_or_else(|| refused_by("handle-unknown", "the node has no sale with this index"))?;
        payment_for(sale.script(), self.network, sats)
    }

    pub(crate) fn open(&self, a: OpenArgs) -> Result<OpenedView, CounterError> {
        let code: [u8; 3] = a
            .currency
            .as_bytes()
            .try_into()
            .map_err(|_| refused_by("sale-currency", "a currency is three letters"))?;
        let currency = Currency::new(code)
            .ok_or_else(|| refused_by("sale-currency", "a currency is three capital letters"))?;
        let (rate, source) = rate_and_source(a.rate, &a.source_label, a.source_fetched_at)?;
        let by = a.actor.map(actor);
        self.run(Some(a.now), |node, _, network| {
            let opened = node
                .open_by(Amount(a.amount_minor), currency, rate, &source, by.as_ref())
                .map_err(|e| from_rule(e.rule(), &e, e.words()))?;
            let payment = payment_for(&opened.script, network, opened.needs_sats)?;
            Ok(OpenedView {
                index: opened.index,
                needs_sats: opened.needs_sats,
                quote_rate: opened.quote.rate.minor_per_btc(),
                quote_from: opened.quote.from,
                quote_until: opened.quote.until,
                payment,
            })
        })
    }

    fn screen(&self, node: &mut Node<Port>, index: u32, screen: Screen) -> ScreenView {
        let live = node.counter().sale(index).is_some_and(|s| s.is_live());
        // CR-113: the on-chain code is there at once; when the invoice has
        // passed the checks it becomes the code of LN-18. A code that cannot
        // be made leaves the screen without one; the reports it carries
        // must not be lost with it.
        let mut payment = None;
        let mut lightning = None;
        if live {
            payment = self.payment(node, index, screen.needs_sats).ok();
            match node.lightning_sale(index) {
                SaleLightning::Off => {}
                SaleLightning::Waiting => {
                    lightning = Some(LightningSaleView {
                        invoice: false,
                        waiting: true,
                        unavailable: None,
                    });
                }
                SaleLightning::Invoice(checked) => {
                    let code = node.counter().sale(index).and_then(|s| {
                        payment_with_invoice(s.script(), self.network, screen.needs_sats, &checked)
                            .ok()
                    });
                    lightning = Some(LightningSaleView {
                        invoice: code.is_some(),
                        waiting: false,
                        unavailable: None,
                    });
                    if code.is_some() {
                        payment = code;
                    }
                }
                SaleLightning::Unavailable(words) => {
                    lightning = Some(LightningSaleView {
                        invoice: false,
                        waiting: false,
                        unavailable: Some(WordsView(words)),
                    });
                }
            }
        }
        ScreenView {
            state: screen.state.into(),
            words: WordsView(screen.words()),
            flags: screen.flag_words().into_iter().map(WordsView).collect(),
            check: screen.check_words().map(WordsView),
            received_sats: screen.received_sats,
            needs_sats: screen.needs_sats,
            stuck: screen.stuck,
            late: screen.late,
            reopened: screen.reopened,
            conflict_seen: screen.conflict_seen,
            reports: reports(screen.reports),
            payment,
            lightning,
        }
    }

    pub(crate) fn judge(&self, now: u64, index: u32) -> Result<ScreenView, CounterError> {
        self.run(Some(now), |node, _, _| {
            let screen = node
                .judge(index)
                .map_err(|e| from_rule(e.rule(), &e, e.words()))?;
            Ok(self.screen(node, index, screen))
        })
    }

    pub(crate) fn requote(&self, a: RequoteArgs) -> Result<OpenedView, CounterError> {
        let (rate, source) = rate_and_source(a.rate, &a.source_label, a.source_fetched_at)?;
        self.run(Some(a.now), |node, _, _| {
            let quote = node
                .requote(a.index, rate, &source)
                .map_err(|e| from_rule(e.rule(), &e, e.words()))?;
            let needs = node.counter().sale(a.index).map_or(0, |s| s.needs_sats());
            Ok(OpenedView {
                index: a.index,
                needs_sats: needs,
                quote_rate: quote.rate.minor_per_btc(),
                quote_from: quote.from,
                quote_until: quote.until,
                payment: self.payment(node, a.index, needs)?,
            })
        })
    }

    pub(crate) fn cancel(&self, a: CancelArgs) -> Result<(), CounterError> {
        let by = a.actor.map(actor);
        self.run(Some(a.now), |node, _, _| {
            node.cancel_by(a.index, by.as_ref())
                .map_err(|e| from_rule(e.rule(), &e, e.words()))
        })
    }

    /// CR-143: a change to the cashiers becomes a record the owner receives.
    pub(crate) fn staff_record(&self, a: StaffArgs) -> Result<(), CounterError> {
        let event = StaffEvent {
            at: a.now,
            change: if a.added {
                StaffChange::Added
            } else {
                StaffChange::Removed
            },
            name: a.name,
            by: a.by,
        };
        self.run(Some(a.now), |node, _, _| {
            node.record_staff(&event)
                .map(|_| ())
                .map_err(|e| from_rule(e.rule(), &e, e.words()))
        })
    }

    /// CR-152: the totals of a day from the counter's own records.
    pub(crate) fn day_totals(&self, a: DayTotalsArgs) -> Result<DayTotalsView, CounterError> {
        self.run(None, |node, _, _| {
            node.day_totals(a.from, a.to, a.people)
                .map(|t| DayTotalsView::of(&t))
                .map_err(|e| from_rule(e.rule(), &e, e.words()))
        })
    }

    pub(crate) fn override_sale(&self, a: OverrideArgs) -> Result<Vec<ReportView>, CounterError> {
        self.run(Some(a.now), |node, held, _| {
            let done = node.override_sale(a.index, actor(a.actor));
            match done.result {
                Ok(()) => Ok(reports(done.reports)),
                Err(e) => {
                    // What the judging before the override reported is not
                    // lost with the refusal: the next pump returns it.
                    held.extend(done.reports);
                    Err(from_rule(e.rule(), &e, e.words()))
                }
            }
        })
    }

    pub(crate) fn request_refund(&self, a: RefundArgs) -> Result<u64, CounterError> {
        let ask = match a.ask {
            RefundAskView::Sats { sats } => RefundAsk::Sats(sats),
            RefundAskView::Value {
                minor,
                rate,
                source_label,
                source_fetched_at,
            } => {
                let (rate, source) = rate_and_source(rate, &source_label, source_fetched_at)?;
                RefundAsk::Value {
                    minor,
                    rate,
                    source,
                }
            }
        };
        self.run(Some(a.now), |node, _, network| {
            let destination = refund_destination(&a.destination, network)
                .map_err(|e| from_rule(e.rule(), e.to_string(), e.words()))?;
            let request = RefundRequest {
                ask,
                destination,
                actor: actor(a.actor),
                reason: a.reason,
            };
            node.request_refund(a.index, request)
                .map_err(|e| from_rule(e.rule(), &e, e.words()))
        })
    }

    pub(crate) fn withdraw_refund(&self, a: WithdrawArgs) -> Result<(), CounterError> {
        self.run(Some(a.now), |node, _, _| {
            node.withdraw_refund(a.index, a.number, actor(a.actor))
                .map_err(|e| from_rule(e.rule(), &e, e.words()))
        })
    }

    pub(crate) fn refund_in_cash(&self, a: CashArgs) -> Result<(), CounterError> {
        self.run(Some(a.now), |node, _, _| {
            let cash = CashRefund {
                actor: actor(a.actor),
                minor: Amount(a.minor),
                reason: a.reason,
            };
            node.refund_in_cash(a.index, cash)
                .map_err(|e| from_rule(e.rule(), &e, e.words()))
        })
    }

    pub(crate) fn refunds(&self, index: u32) -> Result<Vec<RefundRow>, CounterError> {
        self.run(None, |node, _, _| {
            Ok(node.refund_views(index).iter().map(RefundRow::of).collect())
        })
    }

    pub(crate) fn heartbeat(
        &self,
        now: u64,
        bytes: &[u8],
    ) -> Result<HeartbeatOutcome, CounterError> {
        self.run(Some(now), |node, _, _| match node.heartbeat(bytes) {
            Ok(taken) => {
                let (verified, sent_at) = match &taken.view {
                    openbmt_counter::HeartbeatView::Verified { sent_at } => (true, Some(*sent_at)),
                    openbmt_counter::HeartbeatView::NotVerified(_) => (false, None),
                };
                Ok(HeartbeatOutcome {
                    verified,
                    sent_at,
                    words: WordsView(taken.view.words()),
                    reports: reports(taken.reports),
                })
            }
            Err(e) => {
                let rule = match &e {
                    HeartbeatNotTaken::Bytes(_) => "CR-50",
                    HeartbeatNotTaken::Refused(r) => r.rule(),
                };
                Err(from_rule(rule, &e, e.words()))
            }
        })
    }

    pub(crate) fn command(&self, now: u64, bytes: &[u8]) -> Result<(), CounterError> {
        let signed = SignedCommand::from_bytes(bytes)
            .map_err(|e| refused_by("command-bytes", format!("not a command: {e}")))?;
        self.run(Some(now), |node, _, _| {
            node.command(&signed)
                .map_err(|e| from_rule(e.rule(), &e, e.words()))
        })
    }

    pub(crate) fn check_in(&self, now: u64) -> Result<Vec<u8>, CounterError> {
        self.run(Some(now), |node, _, _| {
            node.check_in()
                .map(|r| r.to_bytes())
                .map_err(|e| from_rule("CR-40", &e, words_for(Kind::Store, "CR-40")))
        })
    }

    pub(crate) fn add_batch(&self, now: u64, bytes: &[u8]) -> Result<(), CounterError> {
        let signed = SignedBatch::from_bytes(bytes)
            .map_err(|e| refused_by("batch-bytes", format!("not a batch: {e}")))?;
        self.run(Some(now), |node, _, _| {
            node.add_batch(signed).map_err(|e| match e {
                openbmt_counter::BatchTakeRefused::Add(openbmt_core::batch::AddError::Refused(
                    r,
                )) => refused_by("batch-refused", r),
                openbmt_counter::BatchTakeRefused::Add(openbmt_core::batch::AddError::Persist(
                    p,
                )) => from_rule("CR-40", p, words_for(Kind::Store, "CR-40")),
                openbmt_counter::BatchTakeRefused::Record(f) => {
                    from_rule("CR-40", f, words_for(Kind::Store, "CR-40"))
                }
            })
        })
    }

    /// CR-135: one turn of the link to the owner. With `again`, the relay is
    /// called now whatever the pause after the last call was (the manager's
    /// Try again). A step that is under way on another thread is not
    /// waited for: the link as it is known now is returned.
    ///
    /// The node is held only while what came from the relay is handed to it
    /// and what goes out is decided; the calls to the relay are made with
    /// no lock of the node held. What a heartbeat made the node report is
    /// held for the next `pump`, like what an override held.
    pub(crate) fn link_step(&self, now: u64, again: bool) -> Result<LinkStepView, CounterError> {
        let slot = self.link.as_ref().ok_or_else(no_link)?;
        if self.poisoned.load(Ordering::SeqCst) {
            return Err(poisoned());
        }
        let Ok(mut guard) = slot.link.try_lock() else {
            return Ok(slot.step_view(Vec::new()));
        };
        let Some(link) = guard.as_mut() else {
            return Ok(slot.step_view(Vec::new()));
        };
        if self.closing.load(Ordering::SeqCst) {
            return Err(unknown_handle());
        }
        if again {
            link.retry_now();
        }
        let polled = link.poll_step(now);
        let called = polled.is_some();
        let out = self.run(Some(now), |node, held, _| {
            let (reports, out) = link.take_step(node, polled, now);
            held.extend(reports);
            Ok(out)
        })?;
        link.send_step(out, now);
        let mut notes = link.take_notes();
        notes.truncate(NOTE_LOG);
        if called {
            slot.update(link.state(), now);
        }
        Ok(slot.step_view(notes))
    }

    /// CR-136: the link as the screen shows it, or none when the counter
    /// has no relay. It does not wait for a step.
    pub(crate) fn link_state(&self) -> Result<Option<LinkView>, CounterError> {
        Ok(self.link.as_ref().map(LinkSlot::view))
    }

    /// CR-107: dial the own node again.
    pub(crate) fn own_node_retry(&self) -> Result<(), CounterError> {
        self.run(None, |node, _, _| {
            node.retry_own_node();
            Ok(())
        })
    }

    pub(crate) fn status(&self) -> Result<StatusView, CounterError> {
        self.run(None, |node, _, _| {
            let own = node.own_node_health();
            let own_node = match own.words() {
                Some(words) => Some(OwnNodeView {
                    usable: own.usable,
                    words: WordsView(words),
                }),
                None => self.own_refused.clone().map(|words| OwnNodeView {
                    usable: false,
                    words,
                }),
            };
            Ok(StatusView {
                checked_height: node.checked_height(),
                tip_height: node.chain().tip_height(),
                health: node.health().map(|h| HealthView {
                    healthy: h.healthy,
                    behind: h.behind,
                    words: WordsView(h.words()),
                }),
                own_node,
                sales: node.sales().collect(),
                refused_below_minimum: node.counter().refused_below_minimum(),
                stopped: node.stopped(),
                lightning: node.lightning_status().map(|s| LightningStatusView {
                    ready: s.ready,
                    words: WordsView(s.words),
                }),
            })
        })
    }

    // Lightning (CR-112 to CR-116).

    /// CR-115: begin the test of a connection a manager scanned or typed.
    pub(crate) fn ln_setup(&self, a: LnSetupArgs) -> Result<LnRequestView, CounterError> {
        let refused = |e: openbmt_lightning::CredentialRefused| from_rule(e.rule(), &e, e.words());
        let credential = match a.kind {
            LnKind::Lnd => {
                Credential::Lnd(LndCredential::from_lndconnect(a.address.trim()).map_err(refused)?)
            }
            LnKind::Cln => {
                let cert = a.cert.trim();
                let pin = if cert.contains("-----BEGIN") {
                    CertPin::from_pem(cert)
                } else {
                    CertPin::from_sha256_hex(cert)
                }
                .map_err(refused)?;
                let endpoint = Endpoint::parse(a.address.trim(), pin).map_err(refused)?;
                Credential::Cln(ClnCredential::new(endpoint, a.secret.trim()).map_err(refused)?)
            }
        };
        self.run(None, |node, _, _| {
            node.lightning_setup(credential)
                .map(LnRequestView::from)
                .map_err(|e| from_rule(e.rule(), &e, e.words()))
        })
    }

    /// CR-112, CR-115: the answer to a request. For a setup request it is
    /// the next one of the test or the record to keep; for the node's own
    /// requests it changes the sale and returns nothing.
    pub(crate) fn ln_reply(&self, a: LnReplyArgs) -> Result<LnStepView, CounterError> {
        let status = u16::try_from(a.status)
            .map_err(|_| refused_by("ln-status", "an HTTP status is at most 65535"))?;
        let reply = LnReply {
            status,
            body: a.body,
        };
        self.run(None, |node, _, _| {
            if !node.lightning_is_setup(a.id) {
                node.lightning_reply(a.id, &reply);
                return Ok(LnStepView {
                    next: None,
                    record: None,
                });
            }
            match node.lightning_setup_answer(a.id, &reply) {
                Ok(SetupStep::More(next)) => Ok(LnStepView {
                    next: Some(next.into()),
                    record: None,
                }),
                Ok(SetupStep::Done(record)) => Ok(LnStepView {
                    next: None,
                    record: Some(record),
                }),
                Err(e) => Err(from_rule(e.rule(), &e, e.words())),
            }
        })
    }

    /// CR-114: the request got no answer. A setup request says why the test
    /// stopped; the node's own requests are tried again by the driver.
    pub(crate) fn ln_failed(&self, a: LnFailArgs) -> Result<(), CounterError> {
        self.run(None, |node, _, _| {
            if node.lightning_is_setup(a.id) {
                let e = node.lightning_setup_failed(a.id, a.why.of());
                return Err(from_rule(e.rule(), &e, e.words()));
            }
            node.lightning_failed(a.id, a.why.of());
            Ok(())
        })
    }

    /// CR-114: whether the certificate the node showed is the pinned one for
    /// the request with this number.
    pub(crate) fn ln_pin(&self, id: u64, der: &[u8]) -> Result<bool, CounterError> {
        self.run(None, |node, _, _| {
            Ok(node.lightning_pin(id).is_some_and(|pin| pin.accepts(der)))
        })
    }

    /// CR-112: the next request the node must send, if one is due.
    pub(crate) fn ln_next(&self, now: u64) -> Result<Option<LnRequestView>, CounterError> {
        self.run(Some(now), |node, _, _| {
            Ok(node.lightning_next().map(LnRequestView::from))
        })
    }

    /// CR-115: remove the Lightning connection at once.
    pub(crate) fn ln_remove(&self) -> Result<(), CounterError> {
        self.run(None, |node, _, _| {
            node.lightning_remove();
            Ok(())
        })
    }

    pub(crate) fn notes(&self) -> Result<Vec<String>, CounterError> {
        self.run(None, |node, _, _| Ok(node.take_notes()))
    }

    /// Run `f` as a call on the node, for a test of what a panic does.
    #[cfg(test)]
    pub(crate) fn run_for_test(&self, f: impl FnOnce()) -> Result<(), CounterError> {
        self.run(None, |_, _, _| {
            f();
            Ok(())
        })
    }
}
