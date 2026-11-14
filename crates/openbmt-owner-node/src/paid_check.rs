//! A counter's Paid, checked against the owner's own view of the wallet
//! (`docs/OWNER-COINS-RULES.md`, OC-60 to OC-64).
//!
//! A counter that is taken over can show Paid for a sale nobody paid, and
//! sign a record that says so. The owner phone does not take its word: it
//! folds the paid sales out of the verified copy of each counter's records
//! ([`paid_sales`]), asks the wallet's view what was received at each sale's
//! address (OC-14), and says so when a payment is missing ([`check_paid`]).
//! The checker is pure: the view is a function the caller hands in, and the
//! marks (which alarm was raised, cleared by whom) come in and go out as
//! values. [`PaidChecks`] keeps the marks in the node's directory, in a
//! strict frame of their own, and a file that does not read back does not
//! stop the node (OC-64).
//!
//! A sale is told as paid on chain or over Lightning by the record that made
//! it Paid: `PaidAtOnce` (CR-13), `Confirmed` and `Final` are payments to the
//! sale's own address (a swap claim is written as these, CR-221), and
//! `PaidByLightning` is a payment to the shop's own node, which has no payment
//! to the address and is not checked (OC-60).
//!
//! The file is `OBM\x17`, version 1: the number of marks (u32 LE), then for
//! each mark the counter (32 bytes), the sale's receive index (u32 LE), the
//! state (1 raised, 2 cleared by the owner, 3 cleared by arrival), the time
//! raised (u64 LE) and the time cleared (u64 LE, 0 for a mark not cleared).
//! Marks are in the order of counter, then index, with none twice.

use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use openbmt_core::identity::DeviceId;
use openbmt_core::records::{Chain, DeviceList, RecordKind};
use openbmt_core::wire::{self, DecodeError, Reader};
use openbmt_counter::money::tolerance;
use openbmt_counter::Event;
use openbmt_courier::Post;
use openbmt_owner::{SaleFacts, SaleKey};
use openbmt_pos::{FactsError, MAX_SALES};

use crate::facts::FactsRefused;
use crate::link::OwnerLink;
use crate::watcher::write_file;

/// The file under the node's directory.
pub const PAID_CHECK_FILE: &str = "paid-check.bin";

/// The magic of the file.
pub const MAGIC_PAID_CHECK: [u8; 4] = *b"OBM\x17";

/// Most marks kept (OC-64). The oldest cleared marks go first; a mark not
/// cleared is never dropped.
pub const MAX_PAID_MARKS: usize = 65_536;

/// How long after its Paid record a sale whose payment is not in a block yet
/// is only waiting, and not a mismatch (OC-61): a payment with a low fee can
/// wait that long. **[to tune]**
pub const PAID_CHECK_WAIT_SECS: u64 = 86_400;

/// How long a sale that cleared itself by its payment arriving is listed with
/// its line (OC-62). **[to tune]**
pub const PAID_ARRIVED_SHOWN_SECS: u64 = 7 * 86_400;

const MARK_LEN: usize = 32 + 4 + 1 + 8 + 8;
const MAX_FILE_BYTES: usize = 5 + 4 + MAX_PAID_MARKS * MARK_LEN;

/// How a sale was made Paid by the counter's records (OC-60).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaidHow {
    /// Accepted at once (CR-13), before the payment is in a block.
    AtOnce,
    /// Confirmed or final: the payment is in a block the counter saw.
    Confirmed,
    /// Paid over the shop's own Lightning node (CR-109). Not checked.
    Lightning,
}

/// A sale a counter's records show paid.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaidSale {
    /// The counter.
    pub counter: DeviceId,
    /// The sale's number: its receive index on the counter.
    pub index: u32,
    /// Who opened the sale, as the counter recorded; empty when it names
    /// nobody.
    pub cashier: String,
    /// The amount the counter recorded as received, in satoshis.
    pub recorded_sats: u64,
    /// The time of the Paid record, by the counter's clock.
    pub paid_at: u64,
    /// How it was paid.
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
}

/// Why the paid sales could not be folded (OC-30).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PaidCheckRefused {
    /// A counter's copy does not fold; nothing is checked on part of the
    /// counters.
    Facts {
        /// The counter.
        counter: DeviceId,
        /// Why.
        error: FactsError,
    },
    /// The marks do not read back (OC-64): the check cannot keep them until
    /// the owner clears them.
    NotKept {
        /// Why the file was refused.
        why: String,
    },
    /// Every mark kept is an alarm not cleared, so nothing can be dropped.
    MarksFull,
    /// No alarm of this sale is raised.
    NoSuchAlarm,
    /// The marks could not be stored.
    Store(String),
}

impl fmt::Display for PaidCheckRefused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Facts { error, .. } => {
                write!(f, "OC-30: a counter's records do not fold: {error}")
            }
            Self::NotKept { why } => write!(
                f,
                "OC-64: the record of the counter checks does not read back: {why}"
            ),
            Self::MarksFull => write!(
                f,
                "OC-64: every mark kept is an alarm not cleared, so no more can be kept"
            ),
            Self::NoSuchAlarm => write!(f, "OC-62: no alarm of this sale is raised"),
            Self::Store(e) => write!(f, "OC-64: the marks could not be stored: {e}"),
        }
    }
}

impl std::error::Error for PaidCheckRefused {}

struct Acc {
    cashier: String,
    /// The first Paid that stands: its time, how, and the amount recorded.
    paid: Option<(u64, PaidHow, u64)>,
    confirmed_now: bool,
}

/// OC-60: the paid sales of one counter, folded from a copy of its records
/// that verifies, ordered by sale index. A sale that was reopened or whose
/// payment was replaced before it was confirmed is not paid. A manager's
/// acceptance is not a payment to the address and is not listed unless a
/// payment record follows.
pub fn paid_sales_from_chain(
    chain: &Chain,
    devices: &DeviceList,
    counter: &DeviceId,
    tolerance_bps: u32,
) -> Result<Vec<PaidSale>, FactsError> {
    chain.verify(devices).map_err(FactsError::Chain)?;
    let mut sales: BTreeMap<u32, Acc> = BTreeMap::new();
    for record in chain.records() {
        let holds_events = matches!(
            record.kind,
            RecordKind::Sale | RecordKind::Payment | RecordKind::Override | RecordKind::Conflict
        );
        if &record.author != counter || !holds_events {
            continue;
        }
        let event = Event::decode(&record.body).map_err(|error| FactsError::BadEvent {
            seq: record.seq,
            error,
        })?;
        // The records of a bill name a bill, not a sale (CR-247).
        if event.bill_number().is_some() {
            continue;
        }
        if let Event::Opened { index, by, .. } = &event {
            if sales.len() >= MAX_SALES && !sales.contains_key(index) {
                return Err(FactsError::TooManySales);
            }
            if sales.contains_key(index) {
                return Err(FactsError::OpenedTwice { index: *index });
            }
            sales.insert(
                *index,
                Acc {
                    cashier: by.as_ref().map(|(_, n)| n.clone()).unwrap_or_default(),
                    paid: None,
                    confirmed_now: false,
                },
            );
            continue;
        }
        if let Event::TillSale { .. } = event {
            continue;
        }
        let Some(acc) = sales.get_mut(&event.index()) else {
            return Err(FactsError::UnknownSale {
                index: event.index(),
            });
        };
        match event {
            Event::PaidAtOnce {
                at, at_risk_sats, ..
            } => {
                if acc.paid.is_none() {
                    acc.paid = Some((at, PaidHow::AtOnce, at_risk_sats));
                }
            }
            Event::Confirmed {
                at, received_sats, ..
            }
            | Event::Final {
                at, received_sats, ..
            } => {
                let first = acc.paid.map_or(at, |(t, _, _)| t);
                if !matches!(acc.paid, Some((_, PaidHow::Lightning, _))) {
                    acc.paid = Some((first, PaidHow::Confirmed, received_sats));
                }
                acc.confirmed_now = true;
            }
            Event::PaidByLightning {
                at, amount_msat, ..
            } => {
                if acc.paid.is_none() {
                    acc.paid = Some((at, PaidHow::Lightning, amount_msat / 1_000));
                }
                acc.confirmed_now = true;
            }
            Event::Reopened { .. } => {
                acc.paid = None;
                acc.confirmed_now = false;
            }
            Event::Replaced { .. } if !acc.confirmed_now => acc.paid = None,
            _ => {}
        }
    }
    Ok(sales
        .into_iter()
        .filter_map(|(index, acc)| {
            let (paid_at, how, recorded_sats) = acc.paid?;
            Some(PaidSale {
                counter: *counter,
                index,
                cashier: acc.cashier,
                recorded_sats,
                paid_at,
                how,
                tolerance_bps,
            })
        })
        .collect())
}

/// OC-30: the paid sales of every counter of the link, from the owner's
/// verified copy of its records. A copy that does not fold gives no sales at
/// all, so that a partial list never raises an alarm.
pub fn paid_sales<R: Post>(
    link: &OwnerLink<R>,
    tolerance_bps: u32,
) -> Result<Vec<PaidSale>, PaidCheckRefused> {
    let mut all = Vec::new();
    for counter in link.counters() {
        let Some(copy) = link.copy(counter) else {
            continue;
        };
        let chain = Chain::from_records(copy.records().to_vec());
        let sales = paid_sales_from_chain(&chain, link.devices(), counter, tolerance_bps).map_err(
            |error| PaidCheckRefused::Facts {
                counter: *counter,
                error,
            },
        )?;
        all.extend(sales);
    }
    Ok(all)
}

/// What a mark says of an alarm (OC-62, OC-64).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarkState {
    /// The mismatch was raised and stands.
    Raised,
    /// The owner cleared it.
    ClearedByOwner,
    /// The payment arrived in full and cleared it.
    ClearedByArrival,
}

impl MarkState {
    fn byte(self) -> u8 {
        match self {
            Self::Raised => 1,
            Self::ClearedByOwner => 2,
            Self::ClearedByArrival => 3,
        }
    }
}

/// One mark: a sale's alarm, with the times (OC-64).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PaidMark {
    /// The sale.
    pub sale: SaleKey,
    /// Where the alarm stands.
    pub state: MarkState,
    /// When it was raised, by the owner phone's clock.
    pub raised_at: u64,
    /// When it was cleared; none while raised.
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
        let mut r = Reader::framed(bytes, &MAGIC_PAID_CHECK)?;
        let count = r.count(MAX_PAID_MARKS, MARK_LEN)?;
        let mut marks = BTreeMap::new();
        let mut last: Option<SaleKey> = None;
        for _ in 0..count {
            let counter = r.device_id()?;
            let index = r.u32_le()?;
            let state = match r.u8()? {
                1 => MarkState::Raised,
                2 => MarkState::ClearedByOwner,
                3 => MarkState::ClearedByArrival,
                k => return Err(DecodeError::UnknownKind(k)),
            };
            let raised_at = r.u64_le()?;
            let cleared = r.u64_le()?;
            let cleared_at = match (state, cleared) {
                (MarkState::Raised, 0) => None,
                (MarkState::Raised, _) => {
                    return Err(DecodeError::NonCanonical(
                        "a raised mark has a time cleared",
                    ))
                }
                (_, 0) => {
                    return Err(DecodeError::NonCanonical("a cleared mark has no time"));
                }
                (_, t) => Some(t),
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
                    raised_at,
                    cleared_at,
                },
            );
        }
        r.finish()?;
        let read = Self { marks };
        // Canonical form: writing it again gives the same bytes.
        if read.to_bytes() != bytes {
            return Err(DecodeError::NonCanonical(
                "the marks are not in their one form",
            ));
        }
        Ok(read)
    }

    /// The marks as the file holds them.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = wire::start(&MAGIC_PAID_CHECK, 5 + 4 + self.marks.len() * MARK_LEN);
        out.extend_from_slice(&wire::len_u32(self.marks.len()).to_le_bytes());
        for m in self.marks.values() {
            out.extend_from_slice(&m.sale.counter.to_bytes());
            out.extend_from_slice(&m.sale.index.to_le_bytes());
            out.push(m.state.byte());
            out.extend_from_slice(&m.raised_at.to_le_bytes());
            out.extend_from_slice(&m.cleared_at.unwrap_or(0).to_le_bytes());
        }
        out
    }

    /// OC-62, OC-64: the marks after a check at `now`. The oldest cleared
    /// marks go first when there are more than [`MAX_PAID_MARKS`]; a mark not
    /// cleared is never dropped, and when that cannot be kept the marks are
    /// refused whole.
    pub fn after(&self, result: &CheckResult, now: u64) -> Result<Self, PaidCheckRefused> {
        let mut next = self.clone();
        for sale in &result.raised {
            next.marks.insert(
                *sale,
                PaidMark {
                    sale: *sale,
                    state: MarkState::Raised,
                    raised_at: now,
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
        next.evict()?;
        Ok(next)
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

    fn evict(&mut self) -> Result<(), PaidCheckRefused> {
        if self.marks.len() <= MAX_PAID_MARKS {
            return Ok(());
        }
        let mut cleared: Vec<(u64, SaleKey)> = self
            .marks
            .values()
            .filter_map(|m| Some((m.cleared_at?, m.sale)))
            .collect();
        cleared.sort_unstable();
        let excess = self.marks.len() - MAX_PAID_MARKS;
        if cleared.len() < excess {
            return Err(PaidCheckRefused::MarksFull);
        }
        for (_, sale) in cleared.into_iter().take(excess) {
            self.marks.remove(&sale);
        }
        Ok(())
    }
}

/// Where a paid sale stands against the wallet (OC-60, OC-61).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaidState {
    /// The wallet shows what the counter recorded, within the tolerance.
    Checked,
    /// Paid less than [`PAID_CHECK_WAIT_SECS`] ago and the payment is not in
    /// a block yet.
    Waiting,
    /// The sale is too old for the wallet to say what was paid, or its index
    /// is not one the owner issued to the counter.
    NotCheckable,
    /// The wallet shows less than the counter recorded, after the wait.
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
}

/// What a check found: each on-chain sale's state, the alarms it raises and
/// the alarms that clear because the payment arrived (OC-62).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CheckResult {
    /// The state of every sale paid on chain, in the order given.
    pub checks: Vec<SaleCheck>,
    /// Mismatches raised for the first time.
    pub raised: Vec<SaleKey>,
    /// Alarms that cleared because the payment arrived in full.
    pub cleared: Vec<SaleKey>,
}

/// OC-60 to OC-62: check the paid sales against the wallet's view.
///
/// `facts` is the wallet's answer for a sale (OC-14). A sale paid over
/// Lightning is not checked and has no entry. A sale is `Checked` when what
/// the view verified plus the sale's tolerance on the recorded amount reaches
/// the recorded amount. Short of that it is `Waiting` while its Paid record is
/// younger than [`PAID_CHECK_WAIT_SECS`] by `now`, and a `Mismatch` after. A
/// mismatch is raised once per sale: a sale with a mark, whatever its state,
/// is not raised again. A raised alarm whose payment has arrived is cleared.
pub fn check_paid(
    sales: &[PaidSale],
    marks: &PaidMarks,
    now: u64,
    facts: impl Fn(SaleKey) -> Result<SaleFacts, FactsRefused>,
) -> CheckResult {
    let mut out = CheckResult::default();
    for sale in sales.iter().filter(|s| s.on_chain()) {
        let key = sale.key();
        let mark = marks.get(&key);
        let (state, seen) = match facts(key) {
            Err(FactsRefused::ViewNotReady | FactsRefused::ReceiptLogFull) => {
                (PaidState::NotYet, None)
            }
            Err(FactsRefused::TooOldForFacts | FactsRefused::NotThisCountersAddress) => {
                (PaidState::NotCheckable, None)
            }
            Ok(f) => {
                let enough = u128::from(f.received_sats)
                    + u128::from(tolerance(sale.recorded_sats, sale.tolerance_bps))
                    >= u128::from(sale.recorded_sats);
                let state = if enough {
                    match mark.map(|m| m.state) {
                        Some(MarkState::Raised) => {
                            out.cleared.push(key);
                            PaidState::ClearedByArrival
                        }
                        Some(MarkState::ClearedByArrival) => PaidState::ClearedByArrival,
                        _ => PaidState::Checked,
                    }
                } else if mark.is_none() && now.saturating_sub(sale.paid_at) < PAID_CHECK_WAIT_SECS
                {
                    PaidState::Waiting
                } else {
                    if mark.is_none() {
                        out.raised.push(key);
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
    /// What the counter recorded as received over them, in satoshis.
    pub total_sats: u64,
    /// How many the view has seen in a block, in full.
    pub seen: u32,
    /// How many it is waiting for; a sale the view cannot judge yet counts
    /// here too.
    pub waiting: u32,
    /// How many it could not check.
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

/// OC-63: the day summary of each counter that has a sale paid on chain with
/// its Paid time in `from..to`, ordered by counter. `checks` is the result of
/// [`check_paid`] over the same `sales`.
pub fn day_summary(
    sales: &[PaidSale],
    checks: &[SaleCheck],
    from: u64,
    to: u64,
) -> Vec<DaySummary> {
    let by_sale: BTreeMap<SaleKey, &SaleCheck> = checks.iter().map(|c| (c.sale, c)).collect();
    let mut days: BTreeMap<DeviceId, DaySummary> = BTreeMap::new();
    for sale in sales
        .iter()
        .filter(|s| s.on_chain() && (from..to).contains(&s.paid_at))
    {
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
        day.total_sats = day.total_sats.saturating_add(sale.recorded_sats);
        let Some(check) = by_sale.get(&sale.key()) else {
            day.waiting = day.waiting.saturating_add(1);
            continue;
        };
        day.seen_sats = day.seen_sats.saturating_add(check.seen_sats.unwrap_or(0));
        let count = match check.state {
            PaidState::Checked | PaidState::ClearedByArrival => &mut day.seen,
            PaidState::Waiting | PaidState::NotYet => &mut day.waiting,
            PaidState::NotCheckable => &mut day.too_old,
            PaidState::Mismatch => &mut day.missing,
        };
        *count = count.saturating_add(1);
    }
    for day in days.values_mut() {
        day.differs = day.seen_sats != day.total_sats;
    }
    days.into_values().collect()
}

/// The marks and where they are kept (OC-64).
#[derive(Debug)]
pub struct PaidChecks {
    dir: PathBuf,
    marks: PaidMarks,
    broken: Option<String>,
}

impl PaidChecks {
    /// The marks in `dir`, read back strictly. A file that does not read
    /// back does not stop the node: the check then refuses with
    /// [`PaidCheckRefused::NotKept`] until [`Self::reset`].
    pub fn open(dir: &Path) -> Self {
        let mut checks = Self {
            dir: dir.to_path_buf(),
            marks: PaidMarks::new(),
            broken: None,
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

    /// Why the marks cannot be kept, when the file did not read back.
    pub fn not_kept(&self) -> Option<&str> {
        self.broken.as_deref()
    }

    fn ready(&self) -> Result<(), PaidCheckRefused> {
        match &self.broken {
            Some(why) => Err(PaidCheckRefused::NotKept { why: why.clone() }),
            None => Ok(()),
        }
    }

    fn store(&mut self, next: PaidMarks) -> Result<(), PaidCheckRefused> {
        if next != self.marks {
            write_file(&self.dir, PAID_CHECK_FILE, &next.to_bytes())
                .map_err(PaidCheckRefused::Store)?;
            self.marks = next;
        }
        Ok(())
    }

    /// OC-60 to OC-62, OC-64: run the check, and keep its marks. The marks
    /// are stored before they take effect; when they cannot be stored the
    /// check is refused and nothing is raised.
    pub fn check(
        &mut self,
        sales: &[PaidSale],
        now: u64,
        facts: impl Fn(SaleKey) -> Result<SaleFacts, FactsRefused>,
    ) -> Result<CheckResult, PaidCheckRefused> {
        self.ready()?;
        let result = check_paid(sales, &self.marks, now, facts);
        let next = self.marks.after(&result, now)?;
        self.store(next)?;
        Ok(result)
    }

    /// OC-62: the owner clears the alarm of a sale. It is a change of this
    /// phone's own marks; no command is signed and the counters do not learn
    /// of it.
    pub fn clear(&mut self, sale: &SaleKey, now: u64) -> Result<(), PaidCheckRefused> {
        self.ready()?;
        let next = self.marks.cleared_by_owner(sale, now)?;
        self.store(next)
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
        Ok(())
    }
}

fn read_bounded(dir: &Path) -> Result<Option<Vec<u8>>, String> {
    let path = dir.join(PAID_CHECK_FILE);
    match fs::metadata(&path) {
        Ok(m) if m.len() > MAX_FILE_BYTES as u64 => Err("the file is too large".to_string()),
        Ok(_) => fs::read(&path)
            .map(Some)
            .map_err(|e| format!("{PAID_CHECK_FILE}: {e}")),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("{PAID_CHECK_FILE}: {e}")),
    }
}
