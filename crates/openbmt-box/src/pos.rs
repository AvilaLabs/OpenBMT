//! The POS on the Box (BX-40 to BX-43, [rules](../../../docs/BOX-RULES.md)
//! section 4).
//!
//! [`BoxPos`] takes the counters' paid notices, keeps the sale each one
//! describes, reads the POS through a [`Connector`] on a timer, runs the
//! matching of `openbmt-pos` on the sales the notices gave, reports what it
//! finds to the owner as signed POS notices, and, with a connector that can,
//! closes the order of a sale the counter reported paid. It has no socket and
//! no clock: the time and the POS sign-in are passed in, and its files are
//! behind [`BoxFiles`].
//!
//! The files, in the Box's data directory and each written whole (BX-50):
//!
//! | File | Holds |
//! |---|---|
//! | `pos-notices` | `OBMa`, 1, the last notice number taken from each counter, each sale as the notices gave it, the closes made, the number of the last POS notice and the POS notices not yet taken by the relay |
//! | `pos-ledger` | the ledger of `openbmt-pos` (`OBMZ`) |
//!
//! Where the rules are silent, the choices are in the paragraph "What the
//! code fixes" of the rules.

use std::collections::{BTreeMap, VecDeque};
use std::fmt;
use std::io;
use std::sync::Arc;

use openbmt_core::identity::{DeviceId, Identity};
use openbmt_core::settings::DeviceRole;
use openbmt_core::wire::{self, DecodeError, Reader};
use openbmt_counter::{Amount, Currency};
use openbmt_pos::{
    sync, Alert, CloseOutcome, CloseRequest, Connector, Cursor, Delta, Ledger, OrderId, Page,
    PaidNotice, PosError, PosItem, PosNotice, PosReader, SaleFact, SaleKey,
    Settings as PosSettings, SyncFailed, MAX_SALES,
};

use crate::files::BoxFiles;
use crate::https::Transport;
use crate::pos_config::PosConfigError;

/// The sales, the numbers and the closes (BX-40, BX-43).
pub const NOTICES_FILE: &str = "pos-notices";
/// The POS ledger (PS-71).
pub const LEDGER_FILE: &str = "pos-ledger";
/// Seconds between two readings of the POS when no notice has come.
pub const POLL_SECS: u64 = 30;
/// Most counters the Box keeps a last notice number for.
pub const MAX_COUNTERS: usize = 256;
/// Most POS notices kept for the relay to take; the oldest goes first.
pub const MAX_OUT: usize = 256;
/// Most sales dropped at once when the book is full (BX-40).
pub const PRUNE_CHUNK: usize = 1_024;
/// Most orders closed in one tick.
pub const MAX_CLOSES_PER_TICK: usize = 16;
/// Most sales the connector is asked to name an order for in one tick.
pub const MAX_FINDS_PER_TICK: usize = 16;
/// Most lines of notes one call returns.
const MAX_NOTES: usize = 64;

const MAGIC_POS_STORE: [u8; 4] = *b"OBMa";

/// What the Box is given to run the POS with.
pub struct PosSetup {
    /// What the shop decided about its POS (PS-10, PS-13, PS-14).
    pub settings: PosSettings,
    /// The vendor's connector (PS-80 to PS-82).
    pub connector: Box<dyn Connector>,
    /// Seconds between two readings; [`POLL_SECS`] by default.
    pub interval_secs: u64,
    /// Fewest seconds between two readings, a paid notice's wish for one at
    /// once included (BX-49); 0 by default.
    pub min_gap_secs: u64,
    /// Whether the counters of the shop's one location are the counters on
    /// the Box's device list, whatever the settings in force say (BX-49).
    pub counters_from_list: bool,
    /// What the connector and the POS side share, when the connector reads
    /// over HTTPS (BX-47).
    pub transport: Option<Arc<Transport>>,
}

impl PosSetup {
    /// A setup that reads every [`POLL_SECS`] seconds.
    pub fn new(settings: PosSettings, connector: Box<dyn Connector>) -> Self {
        Self {
            settings,
            connector,
            interval_secs: POLL_SECS,
            min_gap_secs: 0,
            counters_from_list: false,
            transport: None,
        }
    }
}

/// Why the POS side could not be opened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PosOpenError {
    /// A file that is there does not read back. The Box does not make a
    /// new one over it.
    Damaged {
        /// The file.
        file: &'static str,
        /// What is wrong with it.
        why: String,
    },
    /// A file could not be read.
    Io {
        /// The file.
        file: &'static str,
        /// What the system said.
        kind: io::ErrorKind,
    },
    /// The shop's POS settings are not valid.
    Settings(openbmt_pos::SettingsError),
    /// The Box's POS vendor file does not read or its values are not valid
    /// (BX-49).
    Config(PosConfigError),
}

impl fmt::Display for PosOpenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Damaged { file, why } => write!(
                f,
                "BX-51: {file} is damaged ({why}): the Box does not run the POS and does not \
                 make a new one"
            ),
            Self::Io { file, kind } => write!(f, "BX-50: could not read {file}: {kind}"),
            Self::Settings(e) => write!(f, "{e}"),
            Self::Config(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for PosOpenError {}

/// What a close has come to (BX-43).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CloseState {
    /// The attempt is recorded and the call may or may not have been made.
    Attempted,
    /// The connector said the order is closed.
    Closed,
    /// The counter reported the sale unpaid again after the close, and the
    /// owner was told.
    Alerted,
}

impl CloseState {
    fn to_byte(self) -> u8 {
        match self {
            Self::Attempted => 1,
            Self::Closed => 2,
            Self::Alerted => 3,
        }
    }

    fn from_byte(b: u8) -> Option<Self> {
        Some(match b {
            1 => Self::Attempted,
            2 => Self::Closed,
            3 => Self::Alerted,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CloseEntry {
    state: CloseState,
    order: OrderId,
}

/// Everything in `pos-notices`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Store {
    /// The number of the last POS notice made.
    out_number: u64,
    /// The last paid notice number taken from each counter.
    last: BTreeMap<DeviceId, u64>,
    /// Each sale as the notices gave it.
    sales: BTreeMap<SaleKey, SaleFact>,
    /// The closes made or begun, by sale.
    closes: BTreeMap<SaleKey, CloseEntry>,
    /// The POS notices the relay has not taken, by number.
    out: VecDeque<PosNotice>,
}

fn put_sale(out: &mut Vec<u8>, f: &SaleFact) {
    out.extend_from_slice(&f.counter.to_bytes());
    out.extend_from_slice(&f.index.to_le_bytes());
    out.extend_from_slice(&f.amount.0.to_le_bytes());
    out.extend_from_slice(&f.currency.code());
    out.extend_from_slice(&f.opened_at.to_le_bytes());
    wire::put_opt_u64(out, f.paid_at);
    out.push(u8::from(f.unpaid_after));
    out.push(u8::from(f.is_final));
}

fn get_sale(r: &mut Reader<'_>) -> Result<SaleFact, DecodeError> {
    let counter = r.device_id()?;
    let index = r.u32_le()?;
    let amount = Amount(r.u64_le()?);
    let currency = Currency::new(r.array::<3>()?)
        .ok_or(DecodeError::NonCanonical("a currency code is not valid"))?;
    let opened_at = r.u64_le()?;
    let mut fact = SaleFact::opened(counter, index, amount, currency, opened_at);
    fact.paid_at = r.opt_u64()?;
    fact.unpaid_after = r.flag()?;
    fact.is_final = r.flag()?;
    Ok(fact)
}

impl Store {
    fn to_bytes(&self) -> Vec<u8> {
        let mut out = wire::start(&MAGIC_POS_STORE, 64 + self.sales.len() * 72);
        out.extend_from_slice(&self.out_number.to_le_bytes());
        out.extend_from_slice(&wire::len_u32(self.last.len()).to_le_bytes());
        for (c, n) in &self.last {
            out.extend_from_slice(&c.to_bytes());
            out.extend_from_slice(&n.to_le_bytes());
        }
        out.extend_from_slice(&wire::len_u32(self.sales.len()).to_le_bytes());
        for f in self.sales.values() {
            put_sale(&mut out, f);
        }
        out.extend_from_slice(&wire::len_u32(self.closes.len()).to_le_bytes());
        for (k, e) in &self.closes {
            out.extend_from_slice(&k.counter.to_bytes());
            out.extend_from_slice(&k.index.to_le_bytes());
            out.push(e.state.to_byte());
            out.push(u8::try_from(e.order.as_str().len()).unwrap_or(u8::MAX));
            out.extend_from_slice(e.order.as_str().as_bytes());
        }
        out.extend_from_slice(&wire::len_u32(self.out.len()).to_le_bytes());
        for n in &self.out {
            let bytes = n.to_bytes();
            out.extend_from_slice(&u16::try_from(bytes.len()).unwrap_or(u16::MAX).to_le_bytes());
            out.extend_from_slice(&bytes);
        }
        out
    }

    fn from_bytes(bytes: &[u8]) -> Result<Self, DecodeError> {
        let mut r = Reader::framed(bytes, &MAGIC_POS_STORE)?;
        let out_number = r.u64_le()?;
        let mut last = BTreeMap::new();
        for _ in 0..r.count(MAX_COUNTERS, 40)? {
            let c = r.device_id()?;
            let n = r.u64_le()?;
            if last.last_key_value().is_some_and(|(k, _)| *k >= c) {
                return Err(DecodeError::NonCanonical("counters are not in key order"));
            }
            last.insert(c, n);
        }
        let mut sales: BTreeMap<SaleKey, SaleFact> = BTreeMap::new();
        for _ in 0..r.count(MAX_SALES, 60)? {
            let f = get_sale(&mut r)?;
            if sales.last_key_value().is_some_and(|(k, _)| *k >= f.key()) {
                return Err(DecodeError::NonCanonical("sales are not in key order"));
            }
            sales.insert(f.key(), f);
        }
        let mut closes: BTreeMap<SaleKey, CloseEntry> = BTreeMap::new();
        for _ in 0..r.count(MAX_SALES, 38)? {
            let key = SaleKey {
                counter: r.device_id()?,
                index: r.u32_le()?,
            };
            let state = CloseState::from_byte(r.u8()?)
                .ok_or(DecodeError::NonCanonical("a close state is not known"))?;
            let len = usize::from(r.u8()?);
            let text = std::str::from_utf8(r.take(len)?)
                .map_err(|_| DecodeError::NonCanonical("an order id is not text"))?;
            let order = OrderId::new(text)
                .map_err(|_| DecodeError::NonCanonical("an order id is not valid"))?;
            if closes.last_key_value().is_some_and(|(k, _)| *k >= key) {
                return Err(DecodeError::NonCanonical("closes are not in key order"));
            }
            closes.insert(key, CloseEntry { state, order });
        }
        let mut out = VecDeque::new();
        for _ in 0..r.count(MAX_OUT, 2)? {
            let len = usize::from(r.u16_le()?);
            let n = PosNotice::from_bytes(r.take(len)?)?;
            if out.back().is_some_and(|p: &PosNotice| p.number >= n.number) || n.number > out_number
            {
                return Err(DecodeError::NonCanonical(
                    "POS notice numbers are not rising",
                ));
            }
            out.push_back(n);
        }
        r.finish()?;
        Ok(Self {
            out_number,
            last,
            sales,
            closes,
            out,
        })
    }

    /// Make a signed POS notice for `item` and queue it, dropping the
    /// oldest when the queue is full. Returns whether one was dropped.
    fn push_out(&mut self, identity: &Identity, now: u64, item: PosItem) -> bool {
        let number = self.out_number.saturating_add(1);
        let Some(notice) = PosNotice::sign(identity, number, now, item) else {
            return false;
        };
        self.out_number = number;
        let dropped = self.out.len() >= MAX_OUT;
        if dropped {
            self.out.pop_front();
        }
        self.out.push_back(notice);
        dropped
    }
}

/// Whether and why the Box reads the POS.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reading {
    /// The Box is unpaired or turned off, or the POS has not been started
    /// (BX-03, BX-23).
    Stopped,
    /// There is no token, so the POS is not read (BX-41).
    NoToken,
    /// The token file was refused, with the named error (BX-41).
    TokenRefused(String),
    /// The POS is read.
    Reading,
}

/// What the Box can say about its POS side, without the token. The console
/// command `pos` prints it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PosState {
    /// Whether the POS is read.
    pub reading: Reading,
    /// The connector can close orders.
    pub can_close: bool,
    /// When the POS was last read, successfully or not.
    pub last_poll_at: Option<u64>,
    /// Why the last reading failed, if it did.
    pub last_error: Option<String>,
    /// Sales known from notices.
    pub sales: usize,
    /// Paid notices taken since the start.
    pub notices_taken: u64,
    /// Paid notices refused since the start.
    pub notices_refused: u64,
    /// Orders the ledger tracks.
    pub orders: usize,
    /// Matches held.
    pub matches: usize,
    /// Alerts open.
    pub open_alerts: usize,
    /// Orders closed or begun.
    pub closes: usize,
    /// POS notices waiting for the relay.
    pub reports_waiting: usize,
}

impl fmt::Display for PosState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.reading {
            Reading::Stopped => writeln!(f, "reading: no (unpaired, turned off or not started)")?,
            Reading::NoToken => writeln!(f, "reading: no (no POS token; use pos-token)")?,
            Reading::TokenRefused(why) => writeln!(f, "reading: no ({why})")?,
            Reading::Reading => writeln!(f, "reading: yes")?,
        }
        writeln!(f, "closing: {}", if self.can_close { "yes" } else { "no" })?;
        match self.last_poll_at {
            Some(at) => writeln!(f, "last reading: {at}")?,
            None => writeln!(f, "last reading: never")?,
        }
        if let Some(e) = &self.last_error {
            writeln!(f, "last error: {e}")?;
        }
        writeln!(f, "sales: {}", self.sales)?;
        writeln!(
            f,
            "notices: {} taken, {} refused",
            self.notices_taken, self.notices_refused
        )?;
        writeln!(f, "orders: {}", self.orders)?;
        writeln!(f, "matches: {}", self.matches)?;
        writeln!(f, "alerts open: {}", self.open_alerts)?;
        writeln!(f, "orders closed: {}", self.closes)?;
        write!(f, "reports waiting: {}", self.reports_waiting)
    }
}

/// The reader the sync is given: the connector's, and nothing else.
struct ReadOnly<'a>(&'a mut dyn Connector);

impl PosReader for ReadOnly<'_> {
    fn poll(&mut self, cursor: &Cursor) -> Result<Page, PosError> {
        self.0.poll(cursor)
    }
}

/// The POS on the Box. See the module docs.
pub struct BoxPos<F: BoxFiles> {
    files: F,
    identity: Identity,
    settings: PosSettings,
    connector: Box<dyn Connector>,
    interval: u64,
    min_gap: u64,
    counters_from_list: bool,
    follow_error: Option<String>,
    transport: Option<Arc<Transport>>,
    store: Store,
    ledger: Ledger,
    next_poll_at: u64,
    poll_now: bool,
    /// The last sale the connector was asked to name an order for: the
    /// next tick starts after it, so that no sale is always asked first.
    asked_last: Option<SaleKey>,
    reading: Reading,
    token_given: Option<String>,
    last_poll_at: Option<u64>,
    last_error: Option<String>,
    notices_taken: u64,
    notices_refused: u64,
}

fn damaged(file: &'static str, e: DecodeError) -> PosOpenError {
    PosOpenError::Damaged {
        file,
        why: e.to_string(),
    }
}

fn io_error(file: &'static str) -> impl FnOnce(io::Error) -> PosOpenError {
    move |e| PosOpenError::Io {
        file,
        kind: e.kind(),
    }
}

impl<F: BoxFiles> BoxPos<F> {
    /// Open the POS side over `files`: read the notices' file and the
    /// ledger, each if it is there, and refuse to start over one that is
    /// there and does not read back. `identity` is the Box's key, which signs
    /// the POS notices.
    pub fn open(files: F, identity: Identity, setup: PosSetup) -> Result<Self, PosOpenError> {
        let PosSetup {
            settings,
            connector,
            interval_secs,
            min_gap_secs,
            counters_from_list,
            transport,
        } = setup;
        settings.validate().map_err(PosOpenError::Settings)?;
        let store = match files.read(NOTICES_FILE).map_err(io_error(NOTICES_FILE))? {
            Some(bytes) => Store::from_bytes(&bytes).map_err(|e| damaged(NOTICES_FILE, e))?,
            None => Store::default(),
        };
        let ledger = match files.read(LEDGER_FILE).map_err(io_error(LEDGER_FILE))? {
            Some(bytes) => Ledger::from_bytes(&bytes).map_err(|e| damaged(LEDGER_FILE, e))?,
            None => Ledger::new(),
        };
        Ok(Self {
            files,
            identity,
            settings,
            connector,
            interval: interval_secs.max(1),
            min_gap: min_gap_secs,
            counters_from_list,
            follow_error: None,
            transport,
            store,
            ledger,
            next_poll_at: 0,
            poll_now: false,
            asked_last: None,
            reading: Reading::Stopped,
            token_given: None,
            last_poll_at: None,
            last_error: None,
            notices_taken: 0,
            notices_refused: 0,
        })
    }

    /// The storage, for tests.
    pub fn files(&self) -> &F {
        &self.files
    }

    /// The storage, to change it in a test.
    pub fn files_mut(&mut self) -> &mut F {
        &mut self.files
    }

    /// The ledger, for reading.
    pub fn ledger(&self) -> &Ledger {
        &self.ledger
    }

    /// The sales the notices gave, ordered by counter and index.
    pub fn sales(&self) -> Vec<SaleFact> {
        self.store.sales.values().cloned().collect()
    }

    /// The last paid notice number taken from a counter.
    pub fn last_number(&self, counter: &DeviceId) -> u64 {
        self.store.last.get(counter).copied().unwrap_or(0)
    }

    /// The number of the last POS notice made.
    pub fn out_number(&self) -> u64 {
        self.store.out_number
    }

    /// What the Box says about its POS side, without the token.
    pub fn state(&self) -> PosState {
        PosState {
            reading: self.reading.clone(),
            can_close: self.connector.can_close(),
            last_poll_at: self.last_poll_at,
            last_error: self.last_error.clone(),
            sales: self.store.sales.len(),
            notices_taken: self.notices_taken,
            notices_refused: self.notices_refused,
            orders: self.ledger.orders().len(),
            matches: self.ledger.matches().len(),
            open_alerts: self.ledger.open_alerts().len(),
            closes: self.store.closes.len(),
            reports_waiting: self.store.out.len(),
        }
    }

    /// Whether a reading is due at `now`.
    pub fn due(&self, now: u64) -> bool {
        let gap_passed = self
            .last_poll_at
            .is_none_or(|at| now >= at.saturating_add(self.min_gap));
        (self.poll_now && gap_passed) || now >= self.next_poll_at
    }

    /// BX-47: tell the connector whether the settings in force have Tor on.
    pub fn set_tor(&self, on: bool) {
        if let Some(transport) = &self.transport {
            transport.set_tor(on);
        }
    }

    /// BX-49: when the setup says so, the counters of the shop's one
    /// location become the counters on the device list in `roles`. Returns
    /// a note when the list makes settings that are not valid, once for each
    /// fault.
    pub fn follow_counters(&mut self, roles: &BTreeMap<DeviceId, DeviceRole>) -> Option<String> {
        if !self.counters_from_list {
            return None;
        }
        let counters: Vec<DeviceId> = roles
            .iter()
            .filter(|(_, role)| **role == DeviceRole::Counter)
            .map(|(id, _)| *id)
            .collect();
        let mut next = self.settings.clone();
        let place = next.locations.first_mut()?;
        if place.counters == counters {
            return None;
        }
        place.counters = counters;
        match next.validate() {
            Ok(()) => {
                self.settings = next;
                self.follow_error = None;
                None
            }
            Err(e) => {
                let line = format!("BX-49: the counters on the device list were not taken: {e}");
                if self.follow_error.as_deref() == Some(line.as_str()) {
                    None
                } else {
                    self.follow_error = Some(line.clone());
                    Some(line)
                }
            }
        }
    }

    fn persist(&mut self, next: &Store) -> Result<(), io::Error> {
        self.files.write(NOTICES_FILE, &next.to_bytes())
    }

    /// BX-40: take the paid notices that came in sealed messages, each with
    /// the key that sent it. A notice is taken only if it reads, names the
    /// key that sent it, that key is a counter in `roles`, its signature is
    /// good, its number is above the last from that counter and what it says
    /// of the sale agrees with what earlier notices said. The new value is
    /// built, stored with the last numbers in it, and only then does it take
    /// effect. Returns a line for each notice refused and for anything else
    /// worth noting.
    pub fn take_notices(
        &mut self,
        items: &[(DeviceId, Vec<u8>)],
        roles: &BTreeMap<DeviceId, DeviceRole>,
        now: u64,
    ) -> Vec<String> {
        let mut notes = Vec::new();
        let mut work = self.store.clone();
        let (mut taken, mut refused) = (0u64, 0u64);
        for (sender, bytes) in items {
            match self.take_one(&mut work, sender, bytes, roles, now) {
                Ok(()) => taken += 1,
                Err(why) => {
                    refused += 1;
                    if notes.len() < MAX_NOTES {
                        notes.push(format!(
                            "BX-40: a paid notice from {sender:?} refused: {why}"
                        ));
                    }
                }
            }
        }
        if taken > 0 {
            if let Err(e) = self.persist(&work) {
                notes.push(format!(
                    "BX-50: the paid notices were not stored and are dropped: {e}"
                ));
                self.notices_refused = self.notices_refused.saturating_add(taken + refused);
                return notes;
            }
            self.store = work;
            self.poll_now = true;
        }
        self.notices_taken = self.notices_taken.saturating_add(taken);
        self.notices_refused = self.notices_refused.saturating_add(refused);
        notes
    }

    fn take_one(
        &self,
        work: &mut Store,
        sender: &DeviceId,
        bytes: &[u8],
        roles: &BTreeMap<DeviceId, DeviceRole>,
        now: u64,
    ) -> Result<(), String> {
        let n = PaidNotice::from_bytes(bytes).map_err(|e| format!("it does not read: {e}"))?;
        if n.counter != *sender {
            return Err("it names another counter than the one that sent it".into());
        }
        if roles.get(&n.counter) != Some(&DeviceRole::Counter) {
            return Err("its key is not a counter in the settings in force".into());
        }
        if !n.verify() {
            return Err("its signature does not check".into());
        }
        let last = work.last.get(&n.counter).copied().unwrap_or(0);
        if n.number <= last {
            return Err(format!("its number {} is not above {last}", n.number));
        }
        if !work.last.contains_key(&n.counter) && work.last.len() >= MAX_COUNTERS {
            work.last.retain(|c, _| roles.contains_key(c));
            if work.last.len() >= MAX_COUNTERS {
                return Err("the Box keeps numbers for too many counters".into());
            }
        }
        match work.sales.get(&n.sale()) {
            Some(fact) => {
                fact.check_notice(&n).map_err(|m| m.to_string())?;
            }
            None => {
                if work.sales.len() >= MAX_SALES {
                    self.prune(work, now);
                }
                if work.sales.len() >= MAX_SALES {
                    return Err(format!(
                        "PS-72: the Box holds {MAX_SALES} sales and none can be dropped"
                    ));
                }
                work.sales.insert(n.sale(), SaleFact::from_notice(&n));
                work.last.insert(n.counter, n.number);
                return Ok(());
            }
        }
        if let Some(fact) = work.sales.get_mut(&n.sale()) {
            fact.apply_notice(&n);
        }
        work.last.insert(n.counter, n.number);
        Ok(())
    }

    /// BX-40: make room. Drops up to [`PRUNE_CHUNK`] of the oldest sales that
    /// are final, are not unpaid again, were first reported paid more than
    /// the matching window ago (PS-33) and have no close under way. A sale
    /// dropped is not judged again; a match to it is kept as it is (PS-36).
    /// The record of its close is dropped with it.
    fn prune(&self, work: &mut Store, now: u64) {
        let mut old: Vec<(u64, SaleKey)> = work
            .sales
            .values()
            .filter(|f| f.is_final && !f.unpaid_after)
            .filter_map(|f| f.paid_at.map(|p| (p, f.key())))
            .filter(|(p, _)| now.saturating_sub(*p) > self.settings.timing.match_window_secs)
            .filter(|(_, k)| {
                work.closes
                    .get(k)
                    .is_none_or(|e| e.state != CloseState::Attempted)
            })
            .collect();
        old.sort();
        for (_, key) in old.into_iter().take(PRUNE_CHUNK) {
            work.sales.remove(&key);
            // Its close, if one was made, goes with it: the closes stay
            // within the bound of the sales.
            work.closes.remove(&key);
        }
    }

    /// The POS notices the relay has not taken yet, by number.
    pub fn pending_reports(&self) -> Vec<PosNotice> {
        self.store.out.iter().cloned().collect()
    }

    /// The relay took the POS notices up to `number`: forget them, stored
    /// first.
    pub fn reports_sent(&mut self, number: u64) -> Result<(), io::Error> {
        let mut next = self.store.clone();
        next.out.retain(|n| n.number > number);
        if next == self.store {
            return Ok(());
        }
        self.persist(&next)?;
        self.store = next;
        Ok(())
    }

    /// Stop reading: the Box is unpaired or turned off (BX-03, BX-23). The
    /// connector is told to forget the token.
    pub fn stop(&mut self) {
        if self.token_given.take().is_some() {
            self.connector.set_token(None);
        }
        self.reading = Reading::Stopped;
    }

    /// One turn of the timer at `now`. `token` is what the Box's setup says
    /// about the POS sign-in: the token, none, or the named reason it was
    /// refused (BX-41). Without a token nothing is read and nothing is
    /// closed. When a reading is due (every `interval` seconds, and at once
    /// after a notice was taken) the POS is read through the connector, the
    /// matching runs on the sales the notices gave, and what is new is
    /// queued as POS notices for the owner (BX-42). Then the orders of
    /// sales the counter reported paid are closed, if the connector can
    /// (BX-43). Returns the lines worth noting.
    pub fn tick(&mut self, now: u64, token: Result<Option<String>, String>) -> Vec<String> {
        let mut notes = Vec::new();
        self.unpaid_after_close(now, &mut notes);
        match token {
            Err(why) => {
                self.give_token(None);
                self.reading = Reading::TokenRefused(why);
                return notes;
            }
            Ok(None) => {
                self.give_token(None);
                self.reading = Reading::NoToken;
                return notes;
            }
            Ok(Some(token)) => {
                self.give_token(Some(token));
                self.reading = Reading::Reading;
            }
        }
        if !self.due(now) {
            return notes;
        }
        self.poll_now = false;
        self.last_poll_at = Some(now);
        self.next_poll_at = now.saturating_add(self.interval);
        match self.read(now) {
            Ok(delta) => {
                self.last_error = None;
                self.report(&delta, now, &mut notes);
            }
            Err(failed) => {
                if let openbmt_pos::SyncError::Pos(PosError::RateLimited { retry_after_secs }) =
                    &failed.error
                {
                    self.next_poll_at = now.saturating_add(self.interval.max(*retry_after_secs));
                }
                let line = match (
                    self.transport.as_ref().and_then(|t| t.take()),
                    &failed.error,
                ) {
                    (Some(why), openbmt_pos::SyncError::Pos(_)) if failed.pages_kept > 0 => {
                        format!("{} ({} pages kept)", why.note(), failed.pages_kept)
                    }
                    (Some(why), openbmt_pos::SyncError::Pos(_)) => why.note(),
                    _ => format!("the POS was not read: {failed}"),
                };
                self.last_error = Some(line.clone());
                notes.push(line);
            }
        }
        self.close_pass(now, &mut notes);
        notes
    }

    fn give_token(&mut self, token: Option<String>) {
        if self.token_given != token {
            self.connector.set_token(token.as_deref());
            self.token_given = token;
        }
    }

    /// Read the POS and run the matching on the sales the notices gave.
    fn read(&mut self, now: u64) -> Result<Delta, SyncFailed<io::Error>> {
        let facts: Vec<SaleFact> = self.store.sales.values().cloned().collect();
        let files = &mut self.files;
        let mut reader = ReadOnly(self.connector.as_mut());
        sync(
            &mut reader,
            &mut self.ledger,
            &facts,
            &self.settings,
            now,
            |bytes| files.write(LEDGER_FILE, bytes),
        )
        .map(|report| report.delta)
    }

    /// BX-42: queue the matches and the alerts a reading made new.
    fn report(&mut self, delta: &Delta, now: u64, notes: &mut Vec<String>) {
        if delta.new_matches.is_empty() && delta.new_alerts.is_empty() {
            return;
        }
        let mut next = self.store.clone();
        let mut dropped = false;
        for (tender, matched) in &delta.new_matches {
            dropped |= next.push_out(
                &self.identity,
                now,
                PosItem::Match {
                    tender: tender.clone(),
                    matched: matched.clone(),
                },
            );
        }
        for alert in &delta.new_alerts {
            let alert: Alert = alert.clone();
            dropped |= next.push_out(&self.identity, now, PosItem::Alert(alert));
        }
        if dropped {
            notes.push(format!(
                "BX-42: more than {MAX_OUT} reports are waiting for the relay: the oldest were dropped"
            ));
        }
        match self.persist(&next) {
            Ok(()) => self.store = next,
            Err(e) => notes.push(format!("BX-50: the reports were not stored: {e}")),
        }
    }

    /// BX-43: a sale whose order the Box closed is reported unpaid again by
    /// the counter. Nothing is undone; the owner is told once.
    fn unpaid_after_close(&mut self, now: u64, notes: &mut Vec<String>) {
        let due: Vec<(SaleKey, OrderId)> = self
            .store
            .closes
            .iter()
            .filter(|(_, e)| e.state == CloseState::Closed)
            .filter(|(k, _)| {
                self.store
                    .sales
                    .get(k)
                    .is_some_and(|f| f.unpaid_after && !f.is_paid_now())
            })
            .map(|(k, e)| (*k, e.order.clone()))
            .collect();
        if due.is_empty() {
            return;
        }
        let mut next = self.store.clone();
        for (sale, order) in due {
            if let Some(e) = next.closes.get_mut(&sale) {
                e.state = CloseState::Alerted;
            }
            next.push_out(
                &self.identity,
                now,
                PosItem::UnpaidAfterClose { sale, order },
            );
        }
        match self.persist(&next) {
            Ok(()) => self.store = next,
            Err(e) => notes.push(format!("BX-50: the reports were not stored: {e}")),
        }
    }

    /// BX-43: close the order of each sale whose latest notice from the
    /// counter is "paid" or "final", once, and only through a connector that
    /// can. The attempt is stored before the call and the result after; an
    /// attempt found stored is made again with the same key (PS-81).
    fn close_pass(&mut self, now: u64, notes: &mut Vec<String>) {
        if !self.connector.can_close() {
            return;
        }
        let Some(tender_name) = self.settings.bitcoin_tenders.first().cloned() else {
            return;
        };
        // The attempts found stored come first. After them, the sales with
        // no close yet that were first reported paid inside the matching
        // window (PS-33): an older one has no open order left to find. They
        // are taken in key order from after the last one asked, so a sale
        // the POS has no order for does not keep the others waiting.
        let window = self.settings.timing.match_window_secs;
        let (mut paid, fresh): (Vec<SaleFact>, Vec<SaleFact>) = self
            .store
            .sales
            .values()
            .filter(|f| f.is_paid_now())
            .filter(|f| match self.store.closes.get(&f.key()) {
                Some(e) => e.state == CloseState::Attempted,
                None => f.paid_at.is_some_and(|p| now.saturating_sub(p) <= window),
            })
            .cloned()
            .partition(|f| self.store.closes.contains_key(&f.key()));
        let (before, after): (Vec<SaleFact>, Vec<SaleFact>) = fresh
            .into_iter()
            .partition(|f| self.asked_last.is_some_and(|last| f.key() <= last));
        paid.extend(after);
        paid.extend(before);
        let mut made = 0;
        let mut asked = 0;
        for sale in paid {
            if made >= MAX_CLOSES_PER_TICK {
                break;
            }
            let key = sale.key();
            let held = self.store.closes.get(&key).map(|e| e.order.clone());
            let order = match &held {
                Some(order) => order.clone(),
                None => {
                    if self.store.closes.len() >= MAX_SALES {
                        notes.push("PS-72: too many closes are recorded: none is made".into());
                        break;
                    }
                    if asked >= MAX_FINDS_PER_TICK {
                        break;
                    }
                    asked += 1;
                    self.asked_last = Some(key);
                    match self.connector.find_order(&sale) {
                        Ok(Some(order)) => order,
                        Ok(None) => continue,
                        Err(e) => {
                            notes.push(format!("the POS did not name an order to close: {e}"));
                            continue;
                        }
                    }
                }
            };
            // A sale that is not reported paid now is refused here, before
            // the connector is called.
            let request = match CloseRequest::for_sale(&sale, order.clone(), tender_name.clone()) {
                Ok(r) => r,
                Err(e) => {
                    notes.push(format!("{e}"));
                    continue;
                }
            };
            if held.is_none() {
                let mut next = self.store.clone();
                next.closes.insert(
                    key,
                    CloseEntry {
                        state: CloseState::Attempted,
                        order: order.clone(),
                    },
                );
                match self.persist(&next) {
                    Ok(()) => self.store = next,
                    Err(e) => {
                        notes.push(format!("BX-50: the close attempt was not saved: {e}"));
                        continue;
                    }
                }
            }
            made += 1;
            match self.connector.close(&request) {
                Ok(outcome) => {
                    let mut next = self.store.clone();
                    if let Some(e) = next.closes.get_mut(&key) {
                        e.state = CloseState::Closed;
                    }
                    next.push_out(
                        &self.identity,
                        now,
                        PosItem::Closed {
                            sale: key,
                            order: order.clone(),
                        },
                    );
                    if outcome == CloseOutcome::AlreadyClosed {
                        notes.push(format!(
                            "BX-43: order {order} was already closed for sale {}",
                            key.index
                        ));
                    }
                    match self.persist(&next) {
                        Ok(()) => self.store = next,
                        // The attempt stays stored: the next tick asks again
                        // with the same key.
                        Err(e) => notes.push(format!(
                            "BX-50: the close result was not saved after the till closed the order: {e}"
                        )),
                    }
                }
                Err(e) => notes.push(format!(
                    "BX-43: order {order} was not closed for sale {}: {e}",
                    key.index
                )),
            }
        }
    }
}
