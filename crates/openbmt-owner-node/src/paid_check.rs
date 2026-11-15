//! A counter's Paid, checked against the owner's own view of the wallet
//! (`docs/OWNER-COINS-RULES.md`, OC-60 to OC-64).
//!
//! A counter that is taken over can show Paid for a sale nobody paid, and
//! sign a record that says so. The owner phone does not take its word: it
//! folds the paid sales out of the verified copy of each counter's records
//! ([`paid_sales`]), asks the wallet's view what was received at each sale's
//! address (OC-14), and says so when a payment is missing ([`check_paid`]).
//! The checker is pure: the view is a function the caller hands in, and the
//! marks (when the owner phone first read a Paid, which alarm was raised,
//! cleared by whom) come in and go out as values. [`PaidChecks`] keeps the
//! marks in the node's directory, in a strict frame of their own, and a file
//! that does not read back, or marks that do not fit, do not stop the node or
//! the check (OC-64).
//!
//! A sale is told as paid on chain or over Lightning by the first record that
//! made it Paid: `PaidAtOnce` (CR-13), `Confirmed` and `Final` are payments to
//! the sale's own address (a swap claim is written as these, CR-221), and
//! `PaidByLightning` is a payment to the shop's own node, which has no payment
//! to the address and is not checked (OC-60). A later record of another kind,
//! a reopening or a replacement does not change that: the counter's own word
//! cannot undo a Paid it showed.
//!
//! The file is `OBM\x17`, version 2 (a version 1 file, which had no time of
//! first read, still reads: its marks take the time raised for it): the number of marks (u32 LE), then for
//! each mark the counter (32 bytes), the sale's receive index (u32 LE), the
//! state (1 raised, 2 cleared by the owner, 3 cleared by arrival, 4 first
//! read and not yet checked), the time the owner phone first read the Paid
//! (u64 LE, never 0), the time raised (u64 LE, 0 unless raised or cleared)
//! and the time cleared (u64 LE, 0 for a mark not cleared). Marks are in the
//! order of counter, then index, with none twice.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use openbmt_core::identity::DeviceId;
use openbmt_core::records::{Chain, DeviceList, Record, RecordKind};
use openbmt_core::settings::{Settings, MAX_DEVICES};
use openbmt_core::wire::{self, DecodeError, Reader};
use openbmt_counter::money::tolerance;
use openbmt_counter::Event;
use openbmt_courier::Post;
use openbmt_owner::{SaleFacts, SaleKey};
use openbmt_pos::FactsError;

use crate::facts::FactsRefused;
use crate::link::OwnerLink;
use crate::watcher::write_file;

/// The file of the Lightning counters ever named (OC-60), under the node's
/// directory.
pub const LIGHTNING_COUNTERS_FILE: &str = "lightning-counters.bin";

/// The magic of the file of the Lightning counters.
pub const MAGIC_LIGHTNING_COUNTERS: [u8; 4] = *b"OBM\x16";

/// The file under the node's directory.
pub const PAID_CHECK_FILE: &str = "paid-check.bin";

/// The magic of the file.
pub const MAGIC_PAID_CHECK: [u8; 4] = *b"OBM\x17";

/// Most marks kept (OC-64). The oldest cleared marks go first; a mark not
/// cleared is never dropped from the phone's memory, and the file holds
/// the ones that fit.
pub const MAX_PAID_MARKS: usize = 65_536;

/// Most alarms not cleared one counter may hold (OC-61, OC-64). A counter with
/// this many keeps its alarms, raises no more and is checked no further until
/// the owner clears one, so that one counter cannot fill the marks for the
/// others.
pub const MAX_OPEN_ALARMS_PER_COUNTER: usize = 256;

// A counter is a device of the settings (at most MAX_DEVICES), so the alarms
// of every counter at the cap always fit the marks a file keeps (OC-64).
const _: () =
    assert!(openbmt_core::settings::MAX_DEVICES * MAX_OPEN_ALARMS_PER_COUNTER <= MAX_PAID_MARKS);

/// How long after the owner phone first read a Paid in a counter's records a
/// sale whose payment is not in a block yet is only waiting, and not a
/// mismatch (OC-61): a payment with a low fee can wait that long. **[to
/// tune]**
pub const PAID_CHECK_WAIT_SECS: u64 = 86_400;

/// How long a sale that cleared itself by its payment arriving is listed with
/// its line (OC-62). **[to tune]**
pub const PAID_ARRIVED_SHOWN_SECS: u64 = 7 * 86_400;

/// The version of the file written.
const FILE_VERSION: u8 = 2;
const MARK_LEN: usize = 32 + 4 + 1 + 8 + 8 + 8;
/// A mark of a version 1 file: no time of first read.
const MARK_LEN_V1: usize = 32 + 4 + 1 + 8 + 8;
const MAX_FILE_BYTES: usize = 5 + 4 + MAX_PAID_MARKS * MARK_LEN;

/// OC-60: the underpayment tolerance the check uses, in basis points: the
/// policy default, whatever the settings say, since a raised tolerance must
/// not hide a missing payment.
pub fn default_tolerance_bps() -> u32 {
    Settings::defaults(0).underpayment_tolerance_bps
}

/// How a sale was made Paid by the counter's records (OC-60).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaidHow {
    /// Accepted at once (CR-13), before the payment is in a block.
    AtOnce,
    /// Confirmed or final: the payment is in a block the counter saw.
    Confirmed,
    /// Paid over the shop's own Lightning node (CR-109), recorded by the
    /// counter the signed settings name as the shop's Lightning counter
    /// (OC-60). Not checked.
    Lightning,
    /// A Lightning payment recorded by a counter that is not the shop's
    /// Lightning counter (OC-60): no payment of the shop's, a mismatch at
    /// once with nothing seen.
    LightningUnnamed,
    /// A Lightning payment recorded by a counter not in the set of Lightning
    /// counters, when that set's file did not read back (OC-60): whether
    /// earlier settings named the counter cannot be told, so it is not
    /// checkable.
    LightningUnknown,
}

/// A sale a counter's records have shown paid.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaidSale {
    /// The counter.
    pub counter: DeviceId,
    /// The sale's number: its receive index on the counter.
    pub index: u32,
    /// Who opened the sale, as the counter recorded; empty when it names
    /// nobody.
    pub cashier: String,
    /// The most the counter ever recorded as received or at risk, in
    /// satoshis. A later record of less does not lower it.
    pub recorded_sats: u64,
    /// The satoshis the sale was last quoted at: its opening or its latest
    /// requote.
    pub quoted_sats: u64,
    /// How it was first paid.
    pub how: PaidHow,
    /// The sale's underpayment tolerance, in basis points (WR-32).
    pub tolerance_bps: u32,
}

impl PaidSale {
    /// The sale's key.
    pub fn key(&self) -> SaleKey {
        SaleKey {
            counter: self.counter,
            index: self.index,
        }
    }

    /// Whether the check of OC-60 applies: a payment to the sale's address.
    pub fn on_chain(&self) -> bool {
        self.how != PaidHow::Lightning
    }

    /// OC-60: a Lightning payment is exempt only from a counter the signed
    /// settings have named as the shop's Lightning counter; from any other it is
    /// held as [`PaidHow::LightningUnnamed`], or as [`PaidHow::LightningUnknown`]
    /// when the file of the set did not read back.
    fn named_by(mut self, named: &LightningCounters) -> Self {
        if self.how == PaidHow::Lightning && !named.contains(&self.counter) {
            self.how = if named.unread {
                PaidHow::LightningUnknown
            } else {
                PaidHow::LightningUnnamed
            };
        }
        self
    }

    /// OC-60: what the check expects at the sale's address: the higher of the
    /// most recorded and the amount the sale was last quoted at.
    pub fn expected_sats(&self) -> u64 {
        self.recorded_sats.max(self.quoted_sats)
    }
}

/// Why the marks could not be changed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PaidCheckRefused {
    /// No alarm of this sale is raised.
    NoSuchAlarm,
    /// The marks could not be stored.
    Store(String),
}

impl std::fmt::Display for PaidCheckRefused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoSuchAlarm => write!(f, "OC-62: no alarm of this sale is raised"),
            Self::Store(e) => write!(f, "OC-64: the marks could not be stored: {e}"),
        }
    }
}

impl std::error::Error for PaidCheckRefused {}

struct Acc {
    cashier: String,
    /// How the first Paid that was shown was made.
    how: Option<PaidHow>,
    recorded: u64,
    quoted: u64,
}

/// The fold of one counter's records (OC-60, OC-61).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Fold {
    /// The paid sales, ordered by sale index.
    pub sales: Vec<PaidSale>,
    /// A record was passed over because it does not read as part of a sale.
    /// The records only grow, so this does not clear.
    pub passed_over: bool,
}

/// OC-60: the paid sales of one counter, folded from a copy of its records
/// that verifies, ordered by sale index. A sale stays in the list once it has
/// had a Paid: a reopening or a replacement, a later record of less, or a
/// later Lightning record changes nothing. A manager's acceptance is not a
/// payment to the address and is not listed unless a payment record follows.
///
/// OC-61: a record that does not read as part of a sale is passed over and the
/// fold goes on, with the fold noted as having passed over a record: a record
/// that does not decode, a second opening of a sale (the first stands), a
/// record of a sale never opened. A counter's sales are all
/// folded, however many it has made: the copy held bounds them. Only a copy
/// that does not verify is refused whole.
pub fn paid_sales_from_chain(
    chain: &Chain,
    devices: &DeviceList,
    counter: &DeviceId,
) -> Result<Fold, FactsError> {
    chain.verify(devices).map_err(FactsError::Chain)?;
    Ok(fold_records(chain.records(), counter))
}

/// The fold of one counter's records, which are taken as already checked.
fn fold_records(records: &[Record], counter: &DeviceId) -> Fold {
    let tolerance_bps = default_tolerance_bps();
    let mut sales: BTreeMap<u32, Acc> = BTreeMap::new();
    let mut passed_over = false;
    for record in records {
        let holds_events = matches!(
            record.kind,
            RecordKind::Sale | RecordKind::Payment | RecordKind::Override | RecordKind::Conflict
        );
        if &record.author != counter || !holds_events {
            continue;
        }
        let Ok(event) = Event::decode(&record.body) else {
            passed_over = true;
            continue;
        };
        // The records of a bill name a bill, not a sale (CR-247).
        if event.bill_number().is_some() {
            continue;
        }
        if let Event::Opened {
            index,
            by,
            needs_sats,
            ..
        } = &event
        {
            if sales.contains_key(index) {
                passed_over = true;
                continue;
            }
            sales.insert(
                *index,
                Acc {
                    cashier: by.as_ref().map(|(_, n)| n.clone()).unwrap_or_default(),
                    how: None,
                    recorded: 0,
                    quoted: *needs_sats,
                },
            );
            continue;
        }
        if let Event::TillSale { .. } = event {
            continue;
        }
        let Some(acc) = sales.get_mut(&event.index()) else {
            passed_over = true;
            continue;
        };
        let on_chain = acc.how != Some(PaidHow::Lightning);
        match event {
            Event::Requoted { needs_sats, .. } => acc.quoted = needs_sats,
            Event::PaidAtOnce { at_risk_sats, .. } => {
                if acc.how.is_none() {
                    acc.how = Some(PaidHow::AtOnce);
                }
                if on_chain {
                    acc.recorded = acc.recorded.max(at_risk_sats);
                }
            }
            Event::Confirmed { received_sats, .. } | Event::Final { received_sats, .. } => {
                if acc.how.is_none() || acc.how == Some(PaidHow::AtOnce) {
                    acc.how = Some(PaidHow::Confirmed);
                }
                if on_chain {
                    acc.recorded = acc.recorded.max(received_sats);
                }
            }
            Event::PaidByLightning { amount_msat, .. } if acc.how.is_none() => {
                acc.how = Some(PaidHow::Lightning);
                acc.recorded = amount_msat / 1_000;
            }
            _ => {}
        }
    }
    let sales = sales
        .into_iter()
        .filter_map(|(index, acc)| {
            Some(PaidSale {
                counter: *counter,
                index,
                cashier: acc.cashier,
                recorded_sats: acc.recorded,
                quoted_sats: acc.quoted,
                how: acc.how?,
                tolerance_bps,
            })
        })
        .collect();
    Fold { sales, passed_over }
}

/// The paid sales of every counter, and the counters whose records could not
/// be folded (OC-61).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PaidSales {
    /// The paid sales of the counters that fold.
    pub sales: Vec<PaidSale>,
    /// The counters with a record that does not read as part of a sale, or a
    /// copy that does not verify: some of their payments cannot be checked.
    pub unreadable: Vec<DeviceId>,
}

/// OC-61: put the folds of each counter together. A counter that passed over a
/// record, or whose copy does not verify, is listed apart; the sales that do
/// read, and every other counter, are still checked.
pub fn gather(folds: Vec<(DeviceId, Result<Fold, FactsError>)>) -> PaidSales {
    let mut out = PaidSales::default();
    for (counter, fold) in folds {
        match fold {
            Ok(fold) => {
                out.sales.extend(fold.sales);
                if fold.passed_over {
                    out.unreadable.push(counter);
                }
            }
            Err(_) => out.unreadable.push(counter),
        }
    }
    out
}

/// OC-30, OC-61: the paid sales of every counter of the link, from the
/// owner's verified copy of its records, each counter apart.
///
/// OC-60: a Lightning payment is exempt only when recorded by a counter in
/// `named`, the counters the settings in force have ever named as the shop's
/// Lightning counter.
///
/// The copy's records were checked as they were taken and again from the
/// first when the copy was opened (LK-58), so they are not checked again here.
pub fn paid_sales<R: Post>(link: &OwnerLink<R>, named: &LightningCounters) -> PaidSales {
    let mut folds = Vec::new();
    for counter in link.counters() {
        let Some(copy) = link.copy(counter) else {
            continue;
        };
        folds.push((*counter, Ok(fold_records(copy.records(), counter))));
    }
    let mut found = gather(folds);
    for sale in &mut found.sales {
        *sale = sale.clone().named_by(named);
    }
    found
}

/// OC-60: the counters the settings in force have named as the shop's
/// Lightning counter, each added when named and never taken out, so that
/// turning Shop Lightning off or moving it makes no earlier sale a mismatch.
/// They are kept in a file of their own in the node's directory, in a strict
/// frame: `OBM\x16`, version, the count (u32 LE, at most
/// [`MAX_DEVICES`]), then the counters in ascending order with none twice.
/// A file that does not read back does not stop the node and raises nothing
/// new: the set then starts empty in memory and the file is not written over;
/// a Lightning payment of a counter not in the set is then not checkable, not
/// a mismatch.
#[derive(Debug)]
pub struct LightningCounters {
    dir: PathBuf,
    named: BTreeSet<DeviceId>,
    /// The file did not read back: it is left as it is.
    broken: bool,
    /// The file was there and did not read back: which counters earlier
    /// settings named cannot be told.
    unread: bool,
    /// The last write failed, so the file is behind the memory.
    unsaved: bool,
}

impl LightningCounters {
    /// The set in `dir`, read back strictly.
    pub fn open(dir: &Path) -> Self {
        let mut out = Self {
            dir: dir.to_path_buf(),
            named: BTreeSet::new(),
            broken: false,
            unread: false,
            unsaved: false,
        };
        match read_file(dir, LIGHTNING_COUNTERS_FILE, 5 + 4 + MAX_DEVICES * 32) {
            Ok(None) => {}
            Ok(Some(bytes)) => match Self::decode(&bytes) {
                Ok(named) => out.named = named,
                Err(_) => (out.broken, out.unread) = (true, true),
            },
            Err(_) => (out.broken, out.unread) = (true, true),
        }
        out
    }

    /// An empty set that is kept nowhere, for a read of records alone.
    pub fn empty() -> Self {
        Self {
            dir: PathBuf::new(),
            named: BTreeSet::new(),
            broken: true,
            unread: false,
            unsaved: false,
        }
    }

    fn decode(bytes: &[u8]) -> Result<BTreeSet<DeviceId>, DecodeError> {
        let mut r = Reader::framed(bytes, &MAGIC_LIGHTNING_COUNTERS)?;
        let count = r.count(MAX_DEVICES, 32)?;
        let mut named = BTreeSet::new();
        let mut last: Option<DeviceId> = None;
        for _ in 0..count {
            let id = r.device_id()?;
            if last.is_some_and(|l| l >= id) {
                return Err(DecodeError::NonCanonical(
                    "the Lightning counters are not in order",
                ));
            }
            last = Some(id);
            named.insert(id);
        }
        r.finish()?;
        Ok(named)
    }

    /// The set as the file holds it.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = wire::start(&MAGIC_LIGHTNING_COUNTERS, 4 + self.named.len() * 32);
        out.extend_from_slice(&wire::len_u32(self.named.len()).to_le_bytes());
        for id in &self.named {
            out.extend_from_slice(&id.to_bytes());
        }
        out
    }

    /// Read a file's bytes back as a set, strictly (for a test or a check).
    pub fn from_bytes(bytes: &[u8]) -> Result<Vec<DeviceId>, DecodeError> {
        Self::decode(bytes).map(|s| s.into_iter().collect())
    }

    /// Whether the settings have named this counter as the Lightning counter.
    pub fn contains(&self, counter: &DeviceId) -> bool {
        self.named.contains(counter)
    }

    /// Add the Lightning counter of settings in force, when they name one.
    pub fn note_settings(&mut self, settings: &Settings) {
        if let Some(sl) = &settings.shop_lightning {
            self.note(&sl.counter);
        }
    }

    /// Add a counter the settings in force name as the Lightning counter.
    /// Nothing is ever taken out. The file is written when the set grew and
    /// can be written; a failed write is tried again at the next call, and the
    /// set holds in memory meanwhile.
    pub fn note(&mut self, counter: &DeviceId) {
        let added = self.named.len() < MAX_DEVICES && self.named.insert(*counter);
        if (added || self.unsaved) && !self.broken {
            self.unsaved =
                write_file(&self.dir, LIGHTNING_COUNTERS_FILE, &self.to_bytes()).is_err();
        }
    }
}

/// OC-61: take out the counters that hold
/// [`MAX_OPEN_ALARMS_PER_COUNTER`] alarms not cleared: they are checked no
/// further and are listed as unreadable, until the owner clears enough.
pub fn gate_open_alarms(found: PaidSales, marks: &PaidMarks) -> PaidSales {
    let open = marks.open_alarms();
    let over = |c: &DeviceId| {
        open.get(c)
            .is_some_and(|n| *n >= MAX_OPEN_ALARMS_PER_COUNTER)
    };
    let mut listed: BTreeSet<DeviceId> = found.unreadable.iter().copied().collect();
    let mut out = PaidSales {
        sales: Vec::new(),
        unreadable: found.unreadable,
    };
    for sale in found.sales {
        if !over(&sale.counter) {
            out.sales.push(sale);
        }
    }
    // A counter over the cap shows whether or not its fold has a paid sale.
    for counter in open.keys().filter(|c| over(c)) {
        if listed.insert(*counter) {
            out.unreadable.push(*counter);
        }
    }
    out
}

/// What a mark says of a sale (OC-61, OC-62, OC-64).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarkState {
    /// The mismatch was raised and stands.
    Raised,
    /// The owner cleared it.
    ClearedByOwner,
    /// The payment arrived in full and cleared it.
    ClearedByArrival,
    /// The owner phone first read the Paid at this time, and the sale is not
    /// checked yet.
    Reading,
}

impl MarkState {
    fn byte(self) -> u8 {
        match self {
            Self::Raised => 1,
            Self::ClearedByOwner => 2,
            Self::ClearedByArrival => 3,
            Self::Reading => 4,
        }
    }
}

/// One mark: a sale's reading and alarm, with the times (OC-64).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PaidMark {
    /// The sale.
    pub sale: SaleKey,
    /// Where the sale stands.
    pub state: MarkState,
    /// When the owner phone first read the Paid, by its clock.
    pub first_read: u64,
    /// When the alarm was raised, by this phone's clock; none for a sale
    /// only read.
    pub raised_at: Option<u64>,
    /// When it was cleared; none while raised or read.
    pub cleared_at: Option<u64>,
}

/// The marks, as the file holds them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PaidMarks {
    marks: BTreeMap<SaleKey, PaidMark>,
}

impl PaidMarks {
    /// No marks.
    pub fn new() -> Self {
        Self::default()
    }

    /// The mark of a sale.
    pub fn get(&self, sale: &SaleKey) -> Option<&PaidMark> {
        self.marks.get(sale)
    }

    /// OC-61: how many alarms not cleared each counter holds.
    pub fn open_alarms(&self) -> HashMap<DeviceId, usize> {
        let mut open: HashMap<DeviceId, usize> = HashMap::new();
        for m in self.marks.values().filter(|m| m.state == MarkState::Raised) {
            *open.entry(m.sale.counter).or_default() += 1;
        }
        open
    }

    /// Every mark, in the order of the file.
    pub fn iter(&self) -> impl Iterator<Item = &PaidMark> {
        self.marks.values()
    }

    /// How many marks.
    pub fn len(&self) -> usize {
        self.marks.len()
    }

    /// Whether there are none.
    pub fn is_empty(&self) -> bool {
        self.marks.is_empty()
    }

    /// OC-64: read a file back, strictly.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, DecodeError> {
        if bytes.len() > MAX_FILE_BYTES {
            return Err(DecodeError::TooLarge);
        }
        let (mut r, version) = Reader::framed_up_to(bytes, &MAGIC_PAID_CHECK, FILE_VERSION)?;
        let old = version == 1;
        let count = r.count(MAX_PAID_MARKS, if old { MARK_LEN_V1 } else { MARK_LEN })?;
        let mut marks = BTreeMap::new();
        let mut last: Option<SaleKey> = None;
        for _ in 0..count {
            let counter = r.device_id()?;
            let index = r.u32_le()?;
            let state = match r.u8()? {
                1 => MarkState::Raised,
                2 => MarkState::ClearedByOwner,
                3 => MarkState::ClearedByArrival,
                4 if !old => MarkState::Reading,
                k => return Err(DecodeError::UnknownKind(k)),
            };
            let first_read = if old { None } else { Some(r.u64_le()?) };
            let raised = r.u64_le()?;
            let cleared = r.u64_le()?;
            if first_read == Some(0) {
                return Err(DecodeError::NonCanonical(
                    "a mark has no time of first read",
                ));
            }
            let (raised_at, cleared_at) = match (state, raised, cleared) {
                (MarkState::Reading, 0, 0) => (None, None),
                (MarkState::Reading, _, _) => {
                    return Err(DecodeError::NonCanonical("a mark only read has a time"))
                }
                (MarkState::Raised, t, 0) if t != 0 => (Some(t), None),
                (MarkState::Raised, _, _) => {
                    return Err(DecodeError::NonCanonical(
                        "a raised mark is not in its form",
                    ))
                }
                (_, t, c) if t != 0 && c != 0 => (Some(t), Some(c)),
                _ => {
                    return Err(DecodeError::NonCanonical(
                        "a cleared mark is not in its form",
                    ))
                }
            };
            let sale = SaleKey { counter, index };
            if last.is_some_and(|l| l >= sale) {
                return Err(DecodeError::NonCanonical("the marks are not in order"));
            }
            last = Some(sale);
            marks.insert(
                sale,
                PaidMark {
                    sale,
                    state,
                    first_read: first_read.or(raised_at).unwrap_or(1),
                    raised_at,
                    cleared_at,
                },
            );
        }
        r.finish()?;
        let read = Self { marks };
        // Canonical form: writing it again gives the same bytes.
        if !old && read.to_bytes() != bytes {
            return Err(DecodeError::NonCanonical(
                "the marks are not in their one form",
            ));
        }
        Ok(read)
    }

    /// The marks as the file holds them. A set longer than
    /// [`MAX_PAID_MARKS`] is not a file; see [`Self::fitted`].
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(5 + 4 + self.marks.len() * MARK_LEN);
        out.extend_from_slice(&MAGIC_PAID_CHECK);
        out.push(FILE_VERSION);
        out.extend_from_slice(&wire::len_u32(self.marks.len()).to_le_bytes());
        for m in self.marks.values() {
            out.extend_from_slice(&m.sale.counter.to_bytes());
            out.extend_from_slice(&m.sale.index.to_le_bytes());
            out.push(m.state.byte());
            out.extend_from_slice(&m.first_read.to_le_bytes());
            out.extend_from_slice(&m.raised_at.unwrap_or(0).to_le_bytes());
            out.extend_from_slice(&m.cleared_at.unwrap_or(0).to_le_bytes());
        }
        out
    }

    /// OC-61, OC-62: the marks after a check at `now`. A sale read for the
    /// first time is marked as read; a sale found missing is raised, once; a
    /// payment that arrived clears its alarm; a sale that no longer waits for
    /// a check loses its reading; a first reading later than `now` is moved
    /// to `now`. The oldest cleared marks go first when there
    /// are more than [`MAX_PAID_MARKS`]; one not cleared is never dropped here.
    pub fn after(&self, result: &CheckResult, now: u64) -> Self {
        let mut next = self.clone();
        for sale in &result.read {
            next.marks.entry(*sale).or_insert(PaidMark {
                sale: *sale,
                state: MarkState::Reading,
                first_read: now,
                raised_at: None,
                cleared_at: None,
            });
        }
        for check in &result.checks {
            if let Some(m) = next.marks.get_mut(&check.sale) {
                m.first_read = m.first_read.min(now);
            }
        }
        for sale in &result.raised {
            let first_read = next.marks.get(sale).map_or(now, |m| m.first_read);
            next.marks.insert(
                *sale,
                PaidMark {
                    sale: *sale,
                    state: MarkState::Raised,
                    first_read,
                    raised_at: Some(now),
                    cleared_at: None,
                },
            );
        }
        for sale in &result.cleared {
            if let Some(m) = next.marks.get_mut(sale) {
                m.state = MarkState::ClearedByArrival;
                m.cleared_at = Some(now.max(1));
            }
        }
        for sale in &result.settled {
            if next
                .marks
                .get(sale)
                .is_some_and(|m| m.state == MarkState::Reading)
            {
                next.marks.remove(sale);
            }
        }
        next.evict_cleared();
        next
    }

    /// OC-62: the owner clears the alarm of a sale.
    pub fn cleared_by_owner(&self, sale: &SaleKey, now: u64) -> Result<Self, PaidCheckRefused> {
        match self.marks.get(sale) {
            Some(m) if m.state == MarkState::Raised => {
                let mut next = self.clone();
                if let Some(m) = next.marks.get_mut(sale) {
                    m.state = MarkState::ClearedByOwner;
                    m.cleared_at = Some(now.max(1));
                }
                Ok(next)
            }
            _ => Err(PaidCheckRefused::NoSuchAlarm),
        }
    }

    fn evict_cleared(&mut self) {
        if self.marks.len() <= MAX_PAID_MARKS {
            return;
        }
        let mut cleared: Vec<(u64, SaleKey)> = self
            .marks
            .values()
            .filter_map(|m| Some((m.cleared_at?, m.sale)))
            .collect();
        cleared.sort_unstable();
        let excess = self.marks.len() - MAX_PAID_MARKS;
        for (_, sale) in cleared.into_iter().take(excess) {
            self.marks.remove(&sale);
        }
    }

    /// Whether more marks are held than a file can keep.
    pub fn is_over(&self) -> bool {
        self.marks.len() > MAX_PAID_MARKS
    }

    /// OC-64: the marks a file can keep: all of them, or when there are too
    /// many, without the oldest cleared marks first and then the newest
    /// readings. An alarm not cleared is never left out; if the marks are
    /// still too many (not possible with the bounds on counters and alarms),
    /// the result is still over and [`Self::is_over`] says so.
    pub fn fitted(&self) -> Self {
        if !self.is_over() {
            return self.clone();
        }
        let mut next = self.clone();
        next.evict_cleared();
        if next.marks.len() > MAX_PAID_MARKS {
            let mut readings: Vec<(u64, SaleKey)> = next
                .marks
                .values()
                .filter(|m| m.state == MarkState::Reading)
                .map(|m| (m.first_read, m.sale))
                .collect();
            // The newest readings go first.
            readings.sort_unstable_by(|a, b| b.cmp(a));
            let mut excess = next.marks.len() - MAX_PAID_MARKS;
            for (_, sale) in readings {
                if excess == 0 {
                    break;
                }
                next.marks.remove(&sale);
                excess -= 1;
            }
            // An alarm not cleared is never dropped. A set still over here
            // is left to the caller, which keeps the file as it is.
        }
        next
    }
}

/// Where a paid sale stands against the wallet (OC-60, OC-61).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaidState {
    /// The wallet shows what the check expects, within the tolerance.
    Checked,
    /// First read by the owner phone less than [`PAID_CHECK_WAIT_SECS`] ago
    /// and the payment is not in a block yet.
    Waiting,
    /// The sale is too old for the wallet to say what was paid.
    NotCheckable,
    /// The wallet's receipt log is full (OC-13), so it cannot say what was
    /// paid to any sale: not checkable, and not a wait.
    LogFull,
    /// The wallet shows less than the check expects, after the wait, or the
    /// sale's index was never issued to the counter (no wait).
    Mismatch,
    /// A mismatch that was raised, whose payment has since arrived in full.
    ClearedByArrival,
    /// The view is not ready, or has not checked the chain to its tip: nothing
    /// is judged.
    NotYet,
}

/// One paid sale's state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaleCheck {
    /// The sale.
    pub sale: SaleKey,
    /// Where it stands.
    pub state: PaidState,
    /// What the wallet's view verified at the sale's address, when it could
    /// say.
    pub seen_sats: Option<u64>,
    /// When the owner phone first read the Paid, by its clock (OC-61).
    pub first_read: u64,
}

/// What a check found: each on-chain sale's state, the Paids read for the
/// first time, the alarms it raises and the alarms that clear because the
/// payment arrived (OC-62).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CheckResult {
    /// The state of every sale paid on chain, in the order given.
    pub checks: Vec<SaleCheck>,
    /// Sales whose Paid is read for the first time.
    pub read: Vec<SaleKey>,
    /// Mismatches raised for the first time.
    pub raised: Vec<SaleKey>,
    /// Alarms that cleared because the payment arrived in full.
    pub cleared: Vec<SaleKey>,
    /// Sales that need no more waiting: seen in full, or not checkable.
    pub settled: Vec<SaleKey>,
}

/// OC-60 to OC-62: check the paid sales against the wallet's view.
///
/// `facts` is the wallet's answer for a sale (OC-14). A sale first paid over
/// Lightning is not checked and has no entry. A sale is `Checked` when what the
/// view verified plus the sale's tolerance on the expected amount reaches the
/// expected amount ([`PaidSale::expected_sats`]). Short of that it is `Waiting`
/// while the owner phone first read its Paid less than [`PAID_CHECK_WAIT_SECS`]
/// before `now`, and a `Mismatch` after. A mismatch is raised once per sale,
/// with one exception: an alarm that cleared itself because the payment arrived
/// is raised again if that payment later leaves the view. An alarm the owner
/// cleared stays cleared. A raised alarm whose payment has arrived is cleared.
/// A Lightning payment of a counter that earlier settings may have named
/// ([`PaidHow::LightningUnknown`]) is not looked up: it is not checkable.
pub fn check_paid(
    sales: &[PaidSale],
    marks: &PaidMarks,
    now: u64,
    facts: impl Fn(SaleKey) -> Result<SaleFacts, FactsRefused>,
) -> CheckResult {
    let mut out = CheckResult::default();
    let mut open = marks.open_alarms();
    for sale in sales.iter().filter(|s| s.on_chain()) {
        let key = sale.key();
        let mark = marks.get(&key);
        // A reading later than the phone's clock (the clock was ahead and is
        // set right) is taken as now; `PaidMarks::after` stores it so.
        let first_read = mark.map_or(now, |m| m.first_read.min(now));
        if mark.is_none() {
            out.read.push(key);
        }
        let alarmed = mark.is_some_and(|m| m.state != MarkState::Reading);
        let expected = sale.expected_sats();
        // OC-60: a Lightning payment of a counter the settings do not name
        // has no address to look at: a mismatch at once.
        let answer = match sale.how {
            PaidHow::LightningUnnamed => Err(FactsRefused::NotThisCountersAddress),
            // OC-60: the set's file did not read back, so the sale cannot be
            // told from a named one: not checkable, and no lookup.
            PaidHow::LightningUnknown => Err(FactsRefused::TooOldForFacts),
            _ => facts(key),
        };
        // OC-61: a counter holds at most the cap of alarms not cleared (see
        // `gate_open_alarms`).
        let mut raise = |out: &mut CheckResult| {
            let n = open.entry(sale.counter).or_default();
            if *n < MAX_OPEN_ALARMS_PER_COUNTER {
                *n += 1;
                out.raised.push(key);
            }
        };
        let (state, seen) = match answer {
            Err(FactsRefused::ViewNotReady) => (PaidState::NotYet, None),
            Err(FactsRefused::ReceiptLogFull) => (PaidState::LogFull, None),
            Err(FactsRefused::TooOldForFacts) => {
                out.settled.push(key);
                (PaidState::NotCheckable, None)
            }
            // The owner never issued this index to this counter, so no
            // payment there can be the shop's: a mismatch at once, with
            // nothing seen and no wait.
            Err(FactsRefused::NotThisCountersAddress) => {
                if !alarmed {
                    raise(&mut out);
                }
                (PaidState::Mismatch, Some(0))
            }
            Ok(f) => {
                let enough = u128::from(f.received_sats)
                    + u128::from(tolerance(expected, sale.tolerance_bps))
                    >= u128::from(expected);
                let state = if enough {
                    match mark.map(|m| m.state) {
                        Some(MarkState::Raised) => {
                            out.cleared.push(key);
                            PaidState::ClearedByArrival
                        }
                        Some(MarkState::ClearedByArrival) => PaidState::ClearedByArrival,
                        _ => {
                            out.settled.push(key);
                            PaidState::Checked
                        }
                    }
                } else if !alarmed && now.saturating_sub(first_read) < PAID_CHECK_WAIT_SECS {
                    PaidState::Waiting
                } else {
                    // An alarm that cleared itself is raised again when the
                    // payment leaves the view; one the owner cleared is not.
                    if !alarmed || mark.map(|m| m.state) == Some(MarkState::ClearedByArrival) {
                        raise(&mut out);
                    }
                    PaidState::Mismatch
                };
                (state, Some(f.received_sats))
            }
        };
        out.checks.push(SaleCheck {
            sale: key,
            state,
            seen_sats: seen,
            first_read,
        });
    }
    out
}

/// OC-63: one counter's day.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DaySummary {
    /// The counter.
    pub counter: DeviceId,
    /// Sales paid on chain in the day.
    pub sales: u32,
    /// What the check expected over them, in satoshis.
    pub total_sats: u64,
    /// How many the view has seen in a block, in full.
    pub seen: u32,
    /// How many it is waiting for; a sale the view cannot judge yet counts
    /// here too.
    pub waiting: u32,
    /// How many it could not check: too old, or the receipt log is full.
    pub too_old: u32,
    /// How many it found missing.
    pub missing: u32,
    /// What the view verified over the sales it could speak for, in
    /// satoshis.
    pub seen_sats: u64,
    /// The total differs from what was seen: the counter is named.
    pub differs: bool,
}

/// OC-63: the day of `now`, from local midnight to the next, in Unix seconds;
/// `offset_secs` is the zone's offset from UTC.
pub fn day_bounds(now: u64, offset_secs: i32) -> (u64, u64) {
    const DAY: i64 = 86_400;
    let local = i64::try_from(now).unwrap_or(i64::MAX) + i64::from(offset_secs);
    let start = local.div_euclid(DAY) * DAY - i64::from(offset_secs);
    let start = u64::try_from(start).unwrap_or(0);
    (start, start.saturating_add(DAY as u64))
}

/// OC-63: the day summary of each counter that has a sale paid on chain whose
/// Paid the owner phone first read in `from..to` (the time the counter wrote
/// in the record is not used), ordered by counter. `checks` is the result of
/// [`check_paid`] over the same `sales`.
pub fn day_summary(
    sales: &[PaidSale],
    checks: &[SaleCheck],
    from: u64,
    to: u64,
) -> Vec<DaySummary> {
    let by_sale: BTreeMap<SaleKey, &SaleCheck> = checks.iter().map(|c| (c.sale, c)).collect();
    let mut days: BTreeMap<DeviceId, DaySummary> = BTreeMap::new();
    for sale in sales.iter().filter(|s| s.on_chain()) {
        let Some(check) = by_sale.get(&sale.key()) else {
            continue;
        };
        if !(from..to).contains(&check.first_read) {
            continue;
        }
        let day = days.entry(sale.counter).or_insert_with(|| DaySummary {
            counter: sale.counter,
            sales: 0,
            total_sats: 0,
            seen: 0,
            waiting: 0,
            too_old: 0,
            missing: 0,
            seen_sats: 0,
            differs: false,
        });
        day.sales = day.sales.saturating_add(1);
        day.total_sats = day.total_sats.saturating_add(sale.expected_sats());
        day.seen_sats = day.seen_sats.saturating_add(check.seen_sats.unwrap_or(0));
        let count = match check.state {
            PaidState::Checked | PaidState::ClearedByArrival => &mut day.seen,
            PaidState::Waiting | PaidState::NotYet => &mut day.waiting,
            PaidState::NotCheckable | PaidState::LogFull => &mut day.too_old,
            PaidState::Mismatch => &mut day.missing,
        };
        *count = count.saturating_add(1);
    }
    for day in days.values_mut() {
        day.differs = day.seen_sats != day.total_sats;
    }
    days.into_values().collect()
}

/// Why the marks are not kept (OC-64).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PaidTrouble {
    /// The file does not read back, or could not be written: the marks live
    /// in memory until the owner clears them.
    NotKept(String),
    /// There are more marks than a file keeps.
    Full,
}

/// The marks and where they are kept (OC-64).
#[derive(Debug)]
pub struct PaidChecks {
    dir: PathBuf,
    marks: PaidMarks,
    /// Why the file did not read back, when it did not. It is not written
    /// over until the owner clears the checks.
    broken: Option<String>,
    /// Why the last write failed, when it did.
    store_failed: Option<String>,
}

impl PaidChecks {
    /// The marks in `dir`, read back strictly. A file that does not read back
    /// does not stop the node or the checks: they run on marks held in memory,
    /// and [`Self::trouble`] says so until [`Self::reset`].
    pub fn open(dir: &Path) -> Self {
        let mut checks = Self {
            dir: dir.to_path_buf(),
            marks: PaidMarks::new(),
            broken: None,
            store_failed: None,
        };
        match read_bounded(dir) {
            Ok(None) => {}
            Ok(Some(bytes)) => match PaidMarks::from_bytes(&bytes) {
                Ok(marks) => checks.marks = marks,
                Err(e) => checks.broken = Some(e.to_string()),
            },
            Err(e) => checks.broken = Some(e),
        }
        checks
    }

    /// The marks, for reading.
    pub fn marks(&self) -> &PaidMarks {
        &self.marks
    }

    /// OC-64: why the checks cannot keep their marks, when they cannot.
    pub fn trouble(&self) -> Option<PaidTrouble> {
        if let Some(why) = self.broken.as_ref().or(self.store_failed.as_ref()) {
            return Some(PaidTrouble::NotKept(why.clone()));
        }
        self.marks.is_over().then_some(PaidTrouble::Full)
    }

    /// Whether the last write failed: the run is not counted as done.
    pub fn store_failed(&self) -> bool {
        self.store_failed.is_some()
    }

    fn store(&mut self, next: PaidMarks) {
        if self.broken.is_none() && (next != self.marks || self.store_failed.is_some()) {
            let fitted = next.fitted();
            // Still over: the file is kept as it is, and the trouble is Full.
            if !fitted.is_over() {
                let written = write_file(&self.dir, PAID_CHECK_FILE, &fitted.to_bytes());
                self.store_failed = written.err();
            }
        }
        self.marks = next;
    }

    /// OC-60 to OC-62, OC-64: run the check, and keep its marks. The check
    /// always runs and its result is always given; the marks are written when
    /// they can be, and [`Self::trouble`] says when they cannot.
    pub fn check(
        &mut self,
        sales: &[PaidSale],
        now: u64,
        facts: impl Fn(SaleKey) -> Result<SaleFacts, FactsRefused>,
    ) -> CheckResult {
        let result = check_paid(sales, &self.marks, now, facts);
        let next = self.marks.after(&result, now);
        self.store(next);
        result
    }

    /// OC-62: the owner clears the alarm of a sale. It is a change of this
    /// phone's own marks; no command is signed and the counters do not learn
    /// of it.
    pub fn clear(&mut self, sale: &SaleKey, now: u64) -> Result<(), PaidCheckRefused> {
        let next = self.marks.cleared_by_owner(sale, now)?;
        self.store(next);
        match &self.store_failed {
            Some(e) => Err(PaidCheckRefused::Store(e.clone())),
            None => Ok(()),
        }
    }

    /// OC-64: drop every mark, and the file that did not read back. The
    /// checks start again; a sale found missing before is raised again.
    pub fn reset(&mut self) -> Result<(), PaidCheckRefused> {
        match fs::remove_file(self.dir.join(PAID_CHECK_FILE)) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(PaidCheckRefused::Store(format!("{PAID_CHECK_FILE}: {e}"))),
        }
        fs::File::open(&self.dir)
            .and_then(|d| d.sync_all())
            .map_err(|e| PaidCheckRefused::Store(e.to_string()))?;
        self.marks = PaidMarks::new();
        self.broken = None;
        self.store_failed = None;
        Ok(())
    }
}

fn read_bounded(dir: &Path) -> Result<Option<Vec<u8>>, String> {
    read_file(dir, PAID_CHECK_FILE, MAX_FILE_BYTES)
}

fn read_file(dir: &Path, name: &str, max: usize) -> Result<Option<Vec<u8>>, String> {
    let path = dir.join(name);
    match fs::metadata(&path) {
        Ok(m) if m.len() > max as u64 => Err("the file is too large".to_string()),
        Ok(_) => fs::read(&path)
            .map(Some)
            .map_err(|e| format!("{name}: {e}")),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("{name}: {e}")),
    }
}
