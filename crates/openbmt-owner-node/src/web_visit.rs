//! The owner phone's visits to a store (`docs/WEB-ORDER-RULES.md`, WO-20 to
//! WO-32), as a part of the node driven by its pump.
//!
//! The node has no HTTP client of its own: as for the price (`price`) and the
//! till (`pos`), the app carries the bytes and the node judges them. A visit
//! is a short run of calls, one at a time:
//!
//! 1. [`OwnerNode::web_step`] says what to do next: nothing, wait for the
//!    call in the air, give the node desk indexes for a range
//!    ([`WebStep::NeedIndexes`], answered by [`OwnerNode::web_give_indexes`]),
//!    or send one POST ([`WebCall`]) to the store.
//! 2. The app sends it, over HTTPS to the store's own address, through Tor
//!    when the shop has Tor on (WO-02, WO-06), and hands back the status and
//!    the body with [`OwnerNode::web_reply`], or says it could not
//!    ([`OwnerNode::web_failed`]).
//!
//! A visit asks for the orders made since the last one (WO-20); makes a
//! request of each (WO-22, `docs/REQUEST-RULES.md`); sends the next signed
//! range when the store has fewer than 20 addresses left (WO-10, WO-11); and
//! sends a signed notice for each order whose request is Paid, Overpaid, Part
//! paid or Paid late at the shop's confirmation depth (WO-30).
//!
//! **When** (WO-21): the pump asks for a visit when the node starts and at
//! every new block while it runs, and when a notice is waiting. The app may
//! ask for one at any time. After a failure the next visit waits 30 seconds,
//! then twice as long after each failure, to one hour; a visit the owner asks
//! for does not wait. (The rules give no numbers for the waiting: these are
//! this module's.)
//!
//! **Time** is passed in by the app on every call; the pump uses the node's
//! own clock for the waiting.

use std::collections::{BTreeMap, BTreeSet};

use bitcoin::ScriptBuf;
use openbmt_owner::starter::{checked_read, KernelRandom};
use openbmt_owner::web_order::{
    orders_request_body, Notice, NoticeAction, NoticeState, OrdersReply, PairingCode, Range,
    RangeReply, RangeResult, SignedRequest, StoreOrder, WebOrderKey, WebOrderRefusal, HEADER_MAC,
    HEADER_NONCE, HEADER_TIME, MAX_NOTICE_TXIDS, RANGE_SIZE, ROUTE_NOTICE, ROUTE_ORDERS,
    ROUTE_RANGE,
};
use openbmt_requests::{day_of, NewRequest, PriceIn, Request, Status};
use openbmt_watch::HeaderChain;

use crate::node::{OwnerNode, OwnerPort};
use crate::web_orders::{
    check_store_url, open, store, web_key_seed, Loaded, NoticeKey, OrderRec, PairState, Pairing,
    RefusedOrder, RefusedWhy, SentBlock, WebError, LOW_ADDRESSES, MAX_ORDERS, MAX_RANGES,
    MAX_REFUSALS,
};

/// The seconds the first failure waits before the next visit.
pub const BACKOFF_FIRST_SECS: u64 = 30;
/// The most seconds a visit waits after failures.
pub const BACKOFF_MAX_SECS: u64 = 3_600;
/// The seconds between two looks at whether a notice is waiting.
const SCAN_SECS: u64 = 2;

/// Where a store's routes are: the REST routes of WordPress sit under this.
const REST_PREFIX: &str = "/wp-json";

/// Which call of a visit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WebCallKind {
    /// WO-20: the orders made since the last visit.
    Orders,
    /// WO-10: a signed range of addresses.
    Range,
    /// WO-30: a signed notice.
    Notice,
}

/// One POST to the store, for the app to send as it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebCall {
    /// Which call it is.
    pub kind: WebCallKind,
    /// The whole address.
    pub url: String,
    /// The headers of the request's HMAC (WO-20), name and value.
    pub headers: Vec<(String, String)>,
    /// The value of the `Content-Type` header.
    pub content_type: &'static str,
    /// The body, exactly as it must be sent.
    pub body: Vec<u8>,
}

/// What the node wants next of the app.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WebStep {
    /// Nothing: no visit is due, or none is possible.
    Idle,
    /// A call is in the air; its answer is awaited.
    Busy,
    /// This many unused indexes of the desk, for a range (WO-10).
    NeedIndexes(usize),
    /// Send this to the store.
    Call(WebCall),
}

/// Why a call did not give an answer the node could use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WebFailure {
    /// The app could not reach the store.
    NotReached,
    /// The shop has Tor on and Tor is not there.
    TorMissing,
    /// The answer was longer than the app reads.
    TooLong,
    /// The store answered 403: it does not take the pairing's requests, or it
    /// is paired no more, or its clock and ours are more than 5 minutes apart.
    Denied,
    /// The store answered with a status the owner phone has no use for.
    Status,
    /// The store's answer is not what the plugin gives.
    BadAnswer,
    /// The node could not take what the store gave yet (the shop's settings
    /// were not given, or a file could not be stored).
    NotReady,
}

impl WebFailure {
    /// The byte the app is given.
    pub fn byte(self) -> u8 {
        match self {
            Self::NotReached => 1,
            Self::TorMissing => 2,
            Self::TooLong => 3,
            Self::Denied => 4,
            Self::Status => 5,
            Self::BadAnswer => 6,
            Self::NotReady => 7,
        }
    }
}

impl WebFailure {
    /// The words for a failed visit, shown on the store's page: the
    /// placeholder of the rule where no words are written yet.
    pub fn words(self) -> openbmt_core::say::Words {
        use openbmt_owner::web_order::say::{visit_words, VisitTrouble};
        visit_words(match self {
            Self::NotReached => VisitTrouble::NotReached,
            Self::TorMissing => VisitTrouble::TorMissing,
            Self::TooLong => VisitTrouble::TooLong,
            Self::Denied => VisitTrouble::Denied,
            Self::Status => VisitTrouble::Status,
            Self::BadAnswer => VisitTrouble::BadAnswer,
            Self::NotReady => VisitTrouble::NotReady,
        })
    }
}

/// WO-10: the words for the store's refusal of a range.
pub fn trouble_words(result: RangeResult) -> openbmt_core::say::Words {
    openbmt_owner::web_order::say::range_words(result)
}

/// The call in the air.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Outstanding {
    kind: WebCallKind,
    /// The sequence of the range sent.
    seq: Option<u32>,
    /// The order and the notice sent.
    notice: Option<(u64, NoticeKey)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Orders,
    Range,
    Notices,
}

/// A visit in progress.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Visit {
    phase: Phase,
    /// The addresses the store has left, from its last answer of the visit.
    left: u64,
    /// Whether indexes were asked for in this visit already.
    asked: bool,
}

/// What the node keeps while it runs, and nowhere else.
#[derive(Debug, Default)]
pub(crate) struct Run {
    wanted: bool,
    visit: Option<Visit>,
    outstanding: Option<Outstanding>,
    fails: u32,
    retry_at: u64,
    failure: Option<WebFailure>,
    last_tip: u32,
    last_scan: u64,
    /// The scripts of the addresses given, with the number of addresses they
    /// were made for.
    scripts: Option<(usize, BTreeMap<ScriptBuf, u32>)>,
}

/// What the node keeps of web orders.
pub(crate) struct WebOrders {
    pub(crate) state: Result<Loaded, WebError>,
    pub(crate) run: Run,
}

impl WebOrders {
    /// Reads the folder's file. A store that was paired is visited when the
    /// node starts (WO-21).
    pub(crate) fn open(dir: &std::path::Path) -> Self {
        let state = open(dir);
        let wanted = state
            .as_ref()
            .is_ok_and(|l| l.state.pairing.as_ref().is_some_and(|p| p.confirmed));
        Self {
            state,
            run: Run {
                wanted,
                ..Run::default()
            },
        }
    }

    /// WO-12, RQ-04: the desk index of every address given to a store.
    pub(crate) fn indexes(&self) -> BTreeSet<u32> {
        match &self.state {
            Ok(loaded) => loaded.indexes(),
            Err(_) => BTreeSet::new(),
        }
    }
}

/// What a pairing shows (WO-05).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebPairShown {
    /// The pairing code, in text, to paste into the store's settings.
    pub code: String,
    /// The key's short fingerprint, which the plugin shows too.
    pub fingerprint: String,
    /// The shop's name the code carries.
    pub shop_name: String,
    /// The store's address as it is kept.
    pub url: String,
}

/// How an order stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrderStand {
    /// A request was made and nothing was counted for it yet.
    Waiting,
    /// A notice for the state of a payment was sent and taken.
    Told,
    /// A notice is waiting to be sent, or was refused by the store.
    NotTold,
    /// The order was refused (WO-22).
    Refused,
}

/// What the store's page shows (WO-21, WO-22).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebStateInfo {
    /// Why web orders are off, if they are.
    pub off: Option<WebError>,
    /// Whether a store is paired, or waits for the owner to confirm.
    pub pairing: Option<PairState>,
    /// The shop's name in the pairing code.
    pub shop_name: String,
    /// The store's address.
    pub url: String,
    /// The fingerprint of the key.
    pub fingerprint: String,
    /// The pairing code, while it waits to be confirmed.
    pub code: Option<String>,
    /// When the store was last visited (WO-21).
    pub last_visit: Option<u64>,
    /// Why the last call failed, if the visits are failing.
    pub failure: Option<WebFailure>,
    /// When the next visit may be, after failures.
    pub retry_at: Option<u64>,
    /// The addresses the store said it had left.
    pub left: Option<u64>,
    /// The addresses given to the store.
    pub addresses_sent: u32,
    /// What the store said of a range it did not take.
    pub trouble: Option<RangeResult>,
    /// The orders taken.
    pub orders: u32,
    /// The orders with a request that nothing was counted for yet.
    pub waiting: u32,
    /// The orders the store was told were paid or part paid.
    pub told: u32,
    /// The notices waiting to be sent or refused by the store.
    pub notices_waiting: u32,
    /// The orders refused, newest last (WO-22).
    pub refused: Vec<RefusedOrder>,
}

fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(char::from_digit(u32::from(b >> 4), 16).unwrap_or('0'));
        out.push(char::from_digit(u32::from(b & 15), 16).unwrap_or('0'));
    }
    out
}

/// WO-30: the notice a request calls for, if it calls for one: the state,
/// what was counted, the deepest and the shallowest payment, and the
/// transactions, each in the order a block explorer shows it.
fn notice_for(
    order: u64,
    address: &str,
    req: &Request,
    tip: u32,
    min_sale: u64,
) -> Option<(Notice, NoticeKey)> {
    if req.credits.is_empty() {
        return None;
    }
    let marks = req.marks();
    let after_end =
        req.credits.iter().all(|c| c.after.is_some()) || marks.after_cancel || marks.after_close;
    let state = if after_end {
        NoticeState::PaidLate
    } else {
        match req.status(min_sale) {
            Status::Waiting => return None,
            Status::PartPaid { .. } | Status::Almost { .. } => NoticeState::PartPaid,
            Status::Paid if marks.late => NoticeState::PaidLate,
            Status::Paid => NoticeState::Paid,
            Status::Overpaid { .. } if marks.late => NoticeState::PaidLate,
            Status::Overpaid { .. } => NoticeState::Overpaid,
        }
    };
    let mut txids: Vec<[u8; 32]> = Vec::new();
    for c in &req.credits {
        let mut shown = c.txid;
        shown.reverse();
        if !txids.contains(&shown) && txids.len() < MAX_NOTICE_TXIDS {
            txids.push(shown);
        }
    }
    let depth = req
        .credits
        .iter()
        .map(|c| tip.saturating_sub(c.height).saturating_add(1))
        .min()
        .unwrap_or(0);
    let counted = req.received_sats();
    let key = NoticeKey {
        state: state.byte(),
        counted,
        txids: u8::try_from(txids.len()).ok()?,
    };
    Some((
        Notice {
            order,
            address: address.to_string(),
            state,
            counted_sats: counted,
            depth,
            txids,
        },
        key,
    ))
}

impl<P: OwnerPort> OwnerNode<P> {
    // What the node knows.

    /// WO-12, RQ-04: the desk index of every address given to a store.
    pub fn web_indexes(&self) -> BTreeSet<u32> {
        self.web.indexes()
    }

    /// What the store's page shows (WO-21, WO-22).
    pub fn web_state(&self) -> WebStateInfo {
        let mut info = WebStateInfo {
            off: None,
            pairing: None,
            shop_name: String::new(),
            url: String::new(),
            fingerprint: String::new(),
            code: None,
            last_visit: None,
            failure: self.web.run.failure,
            retry_at: (self.web.run.fails > 0).then_some(self.web.run.retry_at),
            left: None,
            addresses_sent: 0,
            trouble: None,
            orders: 0,
            waiting: 0,
            told: 0,
            notices_waiting: 0,
            refused: Vec::new(),
        };
        let loaded = match &self.web.state {
            Ok(loaded) => loaded,
            Err(e) => {
                info.off = Some(e.clone());
                return info;
            }
        };
        if let Err(e) = self.requests() {
            info.off = Some(WebError::Off(e.clone()));
        }
        let s = &loaded.state;
        if let Some(p) = &s.pairing {
            let key = p.key(&self.owner_secret);
            info.pairing = Some(p.state());
            info.shop_name = p.shop_name.clone();
            info.url = p.url.clone();
            info.fingerprint = key.fingerprint();
            if !p.confirmed {
                info.code = PairingCode::new(key.public(), p.secret, &p.shop_name)
                    .ok()
                    .map(|c| c.to_text());
            }
        }
        info.last_visit = s.last_visit;
        info.left = s.left;
        info.addresses_sent = s.next_seq;
        info.trouble = s.trouble;
        info.orders = u32::try_from(s.orders.len()).unwrap_or(u32::MAX);
        info.refused = s.refusals.clone();
        let wanted = self.web_wanted_notices(loaded);
        let waiting_keys: BTreeSet<u64> = wanted.iter().map(|(o, _, _)| *o).collect();
        info.notices_waiting = u32::try_from(waiting_keys.len()).unwrap_or(u32::MAX);
        for o in &s.orders {
            match (o.request, o.acked) {
                (Some(_), Some(_)) => info.told += 1,
                (Some(_), None) if !waiting_keys.contains(&o.order) => info.waiting += 1,
                _ => {}
            }
        }
        info
    }

    /// How one order stands, for a list.
    pub fn web_order_stand(&self, order: u64) -> Option<OrderStand> {
        let loaded = self.web.state.as_ref().ok()?;
        if loaded.state.refusals.iter().any(|r| r.order == order) {
            return Some(OrderStand::Refused);
        }
        let rec = loaded.order(order)?;
        let pending = self
            .web_wanted_notices(loaded)
            .iter()
            .any(|(o, _, _)| *o == order);
        Some(match (rec.acked, pending) {
            (_, true) => OrderStand::NotTold,
            (Some(_), false) => OrderStand::Told,
            (None, false) => OrderStand::Waiting,
        })
    }

    fn web_ready(&self) -> Result<&Loaded, WebError> {
        let loaded = self.web.state.as_ref().map_err(Clone::clone)?;
        self.requests()
            .map_err(|e| WebError::Off(e.clone()))
            .map(|_| loaded)
    }

    /// Make a change on a copy, store the copy whole, and only then take it
    /// into memory. Nothing changes on a refusal or a store that fails.
    fn web_edit<T>(
        &mut self,
        change: impl FnOnce(&mut Loaded) -> Result<T, WebError>,
    ) -> Result<T, WebError> {
        let loaded = self.web.state.as_ref().map_err(Clone::clone)?;
        let mut next = loaded.clone();
        let done = change(&mut next)?;
        if next != *loaded {
            store(&self.dir, &next.state)?;
            self.web.state = Ok(next);
        }
        Ok(done)
    }

    // Pairing.

    /// WO-05, WO-06: make the key and the pairing code for the store at
    /// `url`, to be shown to the owner, who pastes it into the store's
    /// settings. The pairing waits for the owner's word that the
    /// fingerprints match ([`web_pair_confirm`](Self::web_pair_confirm)).
    /// A code that waits is replaced by a new one.
    pub fn web_pair_start(
        &mut self,
        url: &str,
        shop_name: &str,
        now: u64,
        rng: &mut dyn KernelRandom,
    ) -> Result<WebPairShown, WebError> {
        self.web_ready()?;
        let url = check_store_url(url)?;
        let secret = checked_read(rng).map_err(|_| WebError::Library(WebOrderRefusal::Random))?;
        let key = WebOrderKey::from_seed(&web_key_seed(&self.owner_secret, &secret));
        let code = PairingCode::new(key.public(), secret, shop_name).map_err(WebError::Library)?;
        let pairing = Pairing {
            secret: *code.secret(),
            shop_name: code.shop_name().to_string(),
            url: url.clone(),
            confirmed: false,
            made_at: now,
        };
        self.web_edit(|l| {
            if l.state.pairing.as_ref().is_some_and(|p| p.confirmed) {
                return Err(WebError::AlreadyPaired);
            }
            l.state.pairing = Some(pairing);
            l.state.trouble = None;
            l.state.left = None;
            Ok(())
        })?;
        self.web.run = Run {
            scripts: self.web.run.scripts.take(),
            ..Run::default()
        };
        Ok(WebPairShown {
            code: code.to_text(),
            fingerprint: key.fingerprint(),
            shop_name: code.shop_name().to_string(),
            url,
        })
    }

    /// WO-05: the owner says the fingerprint the store shows is the one the
    /// owner phone shows. The store is visited from now on.
    pub fn web_pair_confirm(&mut self) -> Result<(), WebError> {
        self.web_ready()?;
        self.web_edit(|l| {
            let p = l.state.pairing.as_mut().ok_or(WebError::NotPaired)?;
            p.confirmed = true;
            Ok(())
        })?;
        self.web.run.wanted = true;
        self.web.run.fails = 0;
        Ok(())
    }

    /// WO-07: forget the key and the secret. The addresses given stay
    /// watched and the orders stay in the book (RQ-04, WO-12).
    pub fn web_unpair(&mut self) -> Result<(), WebError> {
        self.web.state.as_ref().map_err(Clone::clone)?;
        self.web_edit(|l| {
            if l.state.pairing.take().is_none() {
                return Err(WebError::NotPaired);
            }
            l.state.trouble = None;
            l.state.left = None;
            Ok(())
        })?;
        self.web.run = Run {
            scripts: self.web.run.scripts.take(),
            ..Run::default()
        };
        Ok(())
    }

    /// WO-21: a visit is due, as when the app is opened or the owner asks.
    pub fn web_visit_soon(&mut self) {
        self.web.run.wanted = true;
    }

    // The pump.

    /// WO-21: called after the events of a pump are applied. A new block, or
    /// a notice waiting to be sent, makes a visit due.
    pub(crate) fn web_tick(&mut self) {
        let Ok(loaded) = &self.web.state else {
            return;
        };
        if !loaded.state.pairing.as_ref().is_some_and(|p| p.confirmed) {
            return;
        }
        let tip = self.watcher.chain().tip_height();
        if tip != self.web.run.last_tip {
            self.web.run.last_tip = tip;
            self.web.run.wanted = true;
        }
        let now = self.watcher.now();
        if !self.web.run.wanted
            && self.web.run.visit.is_none()
            && now >= self.web.run.last_scan.saturating_add(SCAN_SECS)
        {
            self.web.run.last_scan = now;
            if let Ok(loaded) = &self.web.state {
                if !self.web_wanted_notices(loaded).is_empty() {
                    self.web.run.wanted = true;
                }
            }
        }
    }

    /// WO-30: the notices the requests call for that the store has not taken
    /// or refused: the order, the notice and its key.
    fn web_wanted_notices(&self, loaded: &Loaded) -> Vec<(u64, Notice, NoticeKey)> {
        let (Ok(book), Some(context)) = (self.requests(), self.requests.context) else {
            return Vec::new();
        };
        let tip = self.watcher.chain().tip_height();
        let mut out = Vec::new();
        for rec in &loaded.state.orders {
            let Some(req) = rec.request.and_then(|n| book.request(n)) else {
                continue;
            };
            let Some((notice, key)) =
                notice_for(rec.order, &rec.address, req, tip, context.min_sale_sats)
            else {
                continue;
            };
            if rec.acked != Some(key) && rec.refused != Some(key) {
                out.push((rec.order, notice, key));
            }
        }
        out
    }

    // The desk's addresses, as the node's watch needs them.

    /// RQ-04, WO-12: the scripts of the addresses given to a store, by desk
    /// index, made again when more were given.
    pub(crate) fn web_script_map(&mut self) -> BTreeMap<ScriptBuf, u32> {
        let Ok(loaded) = &self.web.state else {
            return BTreeMap::new();
        };
        let count = loaded.index_of.len();
        if let Some((have, map)) = &self.web.run.scripts {
            if *have == count {
                return map.clone();
            }
        }
        let entries: Vec<(u32, [u8; 32])> = loaded.index_of.values().copied().collect();
        let mut map = BTreeMap::new();
        for (index, wallet) in entries {
            if let Some(script) = self
                .wallet_of(Some(wallet))
                .and_then(|w| w.receive_script(index).ok())
            {
                map.insert(script, index);
            }
        }
        self.web.run.scripts = Some((count, map.clone()));
        map
    }

    // The visit.

    /// WO-20, WO-21: the next step of a visit. `force` asks for a visit now,
    /// without the waiting after a failure (the owner asked, or the app was
    /// opened).
    pub fn web_step(
        &mut self,
        now: u64,
        force: bool,
        rng: &mut dyn KernelRandom,
    ) -> Result<WebStep, WebError> {
        let loaded = self.web_ready()?;
        let Some(pairing) = loaded.state.pairing.clone().filter(|p| p.confirmed) else {
            return Ok(WebStep::Idle);
        };
        if self.web.run.outstanding.is_some() {
            return Ok(WebStep::Busy);
        }
        let left = loaded.state.left.unwrap_or(0);
        if self.web.run.visit.is_none() {
            let run = &mut self.web.run;
            if !(run.wanted || force) || (!force && run.fails > 0 && now < run.retry_at) {
                return Ok(WebStep::Idle);
            }
            run.wanted = false;
            run.visit = Some(Visit {
                phase: Phase::Orders,
                left,
                asked: false,
            });
        }
        loop {
            let Some(visit) = self.web.run.visit.clone() else {
                return Ok(WebStep::Idle);
            };
            match visit.phase {
                Phase::Orders => {
                    let body = orders_request_body(self.web_since()).into_bytes();
                    let call = self.web_make_call(
                        &pairing,
                        WebCallKind::Orders,
                        ROUTE_ORDERS,
                        "application/json",
                        body,
                        now,
                        rng,
                    )?;
                    self.web.run.outstanding = Some(Outstanding {
                        kind: WebCallKind::Orders,
                        seq: None,
                        notice: None,
                    });
                    return Ok(WebStep::Call(call));
                }
                Phase::Range => {
                    let loaded = self.web_ready()?;
                    let undelivered = loaded.state.blocks.iter().find(|b| !b.delivered).cloned();
                    let trouble = loaded.state.trouble.is_some();
                    if let (Some(block), false) = (undelivered, trouble) {
                        let call = self.web_range_call(&pairing, &block, now, rng)?;
                        self.web.run.outstanding = Some(Outstanding {
                            kind: WebCallKind::Range,
                            seq: Some(block.seq),
                            notice: None,
                        });
                        return Ok(WebStep::Call(call));
                    }
                    let room = loaded.state.blocks.len() < MAX_RANGES;
                    if visit.left < LOW_ADDRESSES && !trouble && room && !visit.asked {
                        if let Some(v) = &mut self.web.run.visit {
                            v.asked = true;
                        }
                        return Ok(WebStep::NeedIndexes(RANGE_SIZE));
                    }
                    if let Some(v) = &mut self.web.run.visit {
                        v.phase = Phase::Notices;
                    }
                }
                Phase::Notices => {
                    let loaded = self.web_ready()?;
                    let next = self.web_wanted_notices(loaded).into_iter().next();
                    let Some((order, notice, key)) = next else {
                        self.web.run.visit = None;
                        return Ok(WebStep::Idle);
                    };
                    let signed = notice
                        .sign(&pairing.key(&self.owner_secret))
                        .map_err(WebError::Library)?;
                    let call = self.web_make_call(
                        &pairing,
                        WebCallKind::Notice,
                        ROUTE_NOTICE,
                        "text/plain",
                        hex(&signed).into_bytes(),
                        now,
                        rng,
                    )?;
                    self.web.run.outstanding = Some(Outstanding {
                        kind: WebCallKind::Notice,
                        seq: None,
                        notice: Some((order, key)),
                    });
                    return Ok(WebStep::Call(call));
                }
            }
        }
    }

    fn web_since(&self) -> u64 {
        self.web.state.as_ref().map_or(0, |l| l.state.since)
    }

    #[allow(clippy::too_many_arguments)]
    fn web_make_call(
        &self,
        pairing: &Pairing,
        kind: WebCallKind,
        route: &str,
        content_type: &'static str,
        body: Vec<u8>,
        now: u64,
        rng: &mut dyn KernelRandom,
    ) -> Result<WebCall, WebError> {
        let nonce = SignedRequest::nonce_from(rng).map_err(WebError::Library)?;
        let signed = SignedRequest::make(&pairing.secret, "POST", route, now, nonce, &body)
            .map_err(WebError::Library)?;
        Ok(WebCall {
            kind,
            url: format!("{}{REST_PREFIX}{route}", pairing.url),
            headers: vec![
                (HEADER_TIME.to_string(), signed.time_text()),
                (HEADER_NONCE.to_string(), signed.nonce_text()),
                (HEADER_MAC.to_string(), signed.mac_text()),
            ],
            content_type,
            body,
        })
    }

    fn web_range_call(
        &self,
        pairing: &Pairing,
        block: &SentBlock,
        now: u64,
        rng: &mut dyn KernelRandom,
    ) -> Result<WebCall, WebError> {
        let addresses: Vec<String> = block.entries.iter().map(|(_, a)| a.clone()).collect();
        let range = Range::new(block.seq, addresses).map_err(WebError::Library)?;
        let id = openbmt_owner::web_order::pairing_id_of(&pairing.secret);
        let signed = range.sign(&pairing.key(&self.owner_secret), &id);
        self.web_make_call(
            pairing,
            WebCallKind::Range,
            ROUTE_RANGE,
            "text/plain",
            hex(&signed).into_bytes(),
            now,
            rng,
        )
    }

    /// WO-10, WO-12: the desk indexes the node asked for ([`WebStep::NeedIndexes`]),
    /// which become the next range. The range is stored, and its addresses
    /// are watched for good, before any is sent.
    pub fn web_give_indexes(&mut self, indexes: &[u32]) -> Result<(), WebError> {
        self.web_ready()?;
        let distinct: BTreeSet<u32> = indexes.iter().copied().collect();
        if indexes.is_empty() || indexes.len() > RANGE_SIZE || distinct.len() != indexes.len() {
            return Err(WebError::Indexes);
        }
        let wallet = self.current_wallet();
        let taken = self.request_indexes();
        if indexes.iter().any(|i| taken.contains(i)) {
            return Err(WebError::Indexes);
        }
        let mut entries = Vec::with_capacity(indexes.len());
        for index in indexes {
            let script = self
                .wallet_of(Some(wallet))
                .and_then(|w| w.receive_script(*index).ok())
                .ok_or(WebError::Indexes)?;
            entries.push((*index, self.address_of(&script)));
        }
        self.web_edit(|l| {
            if l.state.blocks.len() >= MAX_RANGES {
                return Err(WebError::TooMany);
            }
            let seq = l.state.next_seq;
            let addresses: Vec<String> = entries.iter().map(|(_, a)| a.clone()).collect();
            let range = Range::new(seq, addresses).map_err(WebError::Library)?;
            l.sent.add_range(&range).map_err(WebError::Library)?;
            for (index, address) in &entries {
                l.index_of.insert(address.clone(), (*index, wallet));
            }
            let count = u32::try_from(entries.len()).map_err(|_| WebError::Indexes)?;
            l.state.next_seq = seq.checked_add(count).ok_or(WebError::TooMany)?;
            l.state.blocks.push(SentBlock {
                seq,
                wallet,
                delivered: false,
                entries,
            });
            Ok(())
        })
    }

    /// The call went wrong before there was an answer to use. The visit
    /// ends, and the next waits (see the module's note).
    pub fn web_failed(&mut self, failure: WebFailure, now: u64) {
        self.web.run.outstanding = None;
        self.web_fail(failure, now);
    }

    fn web_fail(&mut self, failure: WebFailure, now: u64) {
        let run = &mut self.web.run;
        run.visit = None;
        run.wanted = true;
        run.fails = run.fails.saturating_add(1);
        let shift = run.fails.saturating_sub(1).min(7);
        let wait = BACKOFF_FIRST_SECS
            .saturating_mul(1u64 << shift)
            .min(BACKOFF_MAX_SECS);
        run.retry_at = now.saturating_add(wait);
        run.failure = Some(failure);
        self.note(format!(
            "WO-21: the visit to the store failed ({failure:?}); the next waits {wait} seconds"
        ));
    }

    fn web_ok(&mut self) {
        let run = &mut self.web.run;
        run.fails = 0;
        run.failure = None;
    }

    /// The store's answer to the call in the air (WO-10, WO-20, WO-30).
    pub fn web_reply(&mut self, status: u16, body: &[u8], now: u64) -> Result<(), WebError> {
        let Some(out) = self.web.run.outstanding.take() else {
            return Err(WebError::Sequence);
        };
        if status == 403 {
            self.web_fail(WebFailure::Denied, now);
            return Ok(());
        }
        match out.kind {
            WebCallKind::Orders => self.web_orders_reply(status, body, now),
            WebCallKind::Range => self.web_range_reply(status, body, out.seq, now),
            WebCallKind::Notice => self.web_notice_reply(status, body, out.notice, now),
        }
    }

    fn web_range_reply(
        &mut self,
        status: u16,
        body: &[u8],
        seq: Option<u32>,
        now: u64,
    ) -> Result<(), WebError> {
        let reply = match (status, RangeReply::parse(body)) {
            (200 | 409, Ok(reply)) => reply,
            (200 | 409, Err(_)) => {
                self.web_fail(WebFailure::BadAnswer, now);
                return Ok(());
            }
            _ => {
                self.web_fail(WebFailure::Status, now);
                return Ok(());
            }
        };
        let held = reply.result.held();
        if held != (status == 200) {
            self.web_fail(WebFailure::BadAnswer, now);
            return Ok(());
        }
        self.web_edit(|l| {
            l.state.left = Some(reply.left);
            if held {
                if let Some(b) = l.state.blocks.iter_mut().find(|b| Some(b.seq) == seq) {
                    b.delivered = true;
                }
                l.state.trouble = None;
            } else {
                l.state.trouble = Some(reply.result);
            }
            Ok(())
        })?;
        self.web_ok();
        if let Some(v) = &mut self.web.run.visit {
            v.left = reply.left;
            // A range just taken may be followed by another when the store is
            // still low; the next visit does that (WO-11).
            v.phase = Phase::Notices;
        }
        Ok(())
    }

    fn web_notice_reply(
        &mut self,
        status: u16,
        body: &[u8],
        sent: Option<(u64, NoticeKey)>,
        now: u64,
    ) -> Result<(), WebError> {
        let Some((order, key)) = sent else {
            return Err(WebError::Sequence);
        };
        let taken = match (status, NoticeAction::parse(body)) {
            (200, Ok(NoticeAction::Refuse)) | (400, _) => false,
            (200, Ok(_)) => true,
            (200, Err(_)) => {
                self.web_fail(WebFailure::BadAnswer, now);
                return Ok(());
            }
            _ => {
                self.web_fail(WebFailure::Status, now);
                return Ok(());
            }
        };
        self.web_edit(|l| {
            if let Ok(at) = l.state.orders.binary_search_by_key(&order, |o| o.order) {
                if let Some(rec) = l.state.orders.get_mut(at) {
                    if taken {
                        rec.acked = Some(key);
                    } else {
                        rec.refused = Some(key);
                    }
                }
            }
            Ok(())
        })?;
        if !taken {
            self.note(format!(
                "WO-32: the store refused the notice of order {order}"
            ));
        }
        self.web_ok();
        Ok(())
    }

    fn web_orders_reply(&mut self, status: u16, body: &[u8], now: u64) -> Result<(), WebError> {
        if status != 200 {
            self.web_fail(WebFailure::Status, now);
            return Ok(());
        }
        let Ok(reply) = OrdersReply::parse(body) else {
            self.web_fail(WebFailure::BadAnswer, now);
            return Ok(());
        };
        let mut orders = reply.orders;
        orders.sort_by_key(|o| o.order);
        let mut next = self.web_ready()?.clone();
        let mut deferred = false;
        for order in &orders {
            if !self.web_take_order(&mut next, order, now) {
                deferred = true;
                break;
            }
        }
        next.state.left = Some(reply.left);
        next.state.last_visit = Some(now);
        if let Err(e) = store(&self.dir, &next.state) {
            self.note(format!("{e}"));
            self.web_fail(WebFailure::NotReady, now);
            return Ok(());
        }
        self.web.state = Ok(next);
        if deferred {
            self.web_fail(WebFailure::NotReady, now);
            return Ok(());
        }
        self.web_ok();
        if let Some(v) = &mut self.web.run.visit {
            v.left = reply.left;
            v.phase = Phase::Range;
        }
        Ok(())
    }

    /// WO-22: one order of the store's answer. Returns false when the order
    /// could not be taken for a reason that is not the order's (no settings
    /// yet, or a file that cannot be stored), so that it is asked for again.
    fn web_take_order(&mut self, next: &mut Loaded, order: &StoreOrder, now: u64) -> bool {
        use openbmt_owner::web_order::OrderCheck;
        let refuse = |next: &mut Loaded, why: RefusedWhy| {
            next.state.since = next.state.since.max(order.order);
            if next.state.refusals.len() >= MAX_REFUSALS {
                next.state.refusals.remove(0);
            }
            next.state.refusals.push(RefusedOrder {
                order: order.order,
                address: order.address.clone(),
                sats: order.sats,
                why,
                at: now,
            });
        };
        // What the check records stays only for an order that is taken.
        let before = next.sent.clone();
        match next.sent.check(order) {
            Err(r) => {
                refuse(next, RefusedWhy::of(r));
                true
            }
            Ok(OrderCheck::Known) => {
                next.state.since = next.state.since.max(order.order);
                true
            }
            Ok(OrderCheck::New) => {
                if next.state.orders.len() >= MAX_ORDERS {
                    // Not taken, and not lost: the store keeps the order and
                    // is asked again after the owner makes room.
                    next.sent = before;
                    return false;
                }
                let made = self.web_request_for(next, order, now);
                let request = match made {
                    Ok(number) => Some(number),
                    Err(Defer::Later) => {
                        next.sent = before;
                        return false;
                    }
                    Err(Defer::Refused) => None,
                };
                let at = next
                    .state
                    .orders
                    .binary_search_by_key(&order.order, |o| o.order)
                    .unwrap_or_else(|at| at);
                next.state.orders.insert(
                    at,
                    OrderRec {
                        order: order.order,
                        address: order.address.clone(),
                        request,
                        acked: None,
                        refused: None,
                    },
                );
                if request.is_none() {
                    refuse(next, RefusedWhy::NoRequest);
                }
                next.state.since = next.state.since.max(order.order);
                true
            }
        }
    }

    /// RQ-05, WO-22: the request of a web order: in bitcoin, the amount the
    /// store asked for, at the address the owner phone gave, due the day
    /// after it is taken in the shop's time zone (a payment is Paid late only
    /// after that day). An address that already has a request, from a visit
    /// that was cut off before its file was stored, is the order's.
    fn web_request_for(
        &mut self,
        next: &Loaded,
        order: &StoreOrder,
        now: u64,
    ) -> Result<u64, Defer> {
        let Some((index, wallet)) = next.index_of.get(&order.address).copied() else {
            return Err(Defer::Refused);
        };
        if let Ok(book) = self.requests() {
            if let Some(r) = book.requests().iter().find(|r| r.address_index == index) {
                return Ok(r.number);
            }
        }
        let Some(context) = self.requests.context else {
            return Err(Defer::Later);
        };
        // The address must be the wallet's at that index, and the wallet the
        // one pinned now, as a request's is (RQ-04).
        let derived = self
            .wallet_of(Some(wallet))
            .and_then(|w| w.receive_script(index).ok())
            .map(|s| self.address_of(&s));
        if wallet != self.current_wallet() || derived.as_deref() != Some(order.address.as_str()) {
            return Err(Defer::Refused);
        }
        let today = day_of(now, context.tz_offset_secs);
        let Some(due) = today.plus_days(1) else {
            return Err(Defer::Refused);
        };
        let new = NewRequest {
            note: format!("Web order {}", order.order),
            amount: order.sats,
            price_in: PriceIn::Bitcoin,
            due,
            repeat: None,
        };
        match self.make_request(&new, index, now) {
            Ok(made) => Ok(made.number),
            Err(
                crate::requests::RequestError::NoContext
                | crate::requests::RequestError::Off(_)
                | crate::requests::RequestError::Store(_),
            ) => Err(Defer::Later),
            Err(_) => Err(Defer::Refused),
        }
    }
}

/// Why an order was not made a request.
enum Defer {
    /// The order's own fault: refused and shown.
    Refused,
    /// Not the order's fault: asked for again at the next visit.
    Later,
}
