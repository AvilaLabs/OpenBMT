//! The day's totals, counted from the counter's own record log (CR-152 to
//! CR-155; `docs/DESIGN.md` §2.5).
//!
//! The caller gives two times, the start and the end of the day on the
//! phone's own clock and time zone; the library has no clock and no zone. It
//! reads the records from the newest backwards, a block at a time, until a
//! block holds events and none of them is from the start of the day on, then
//! folds the records it read, oldest first. A wallet balance is never in it:
//! the records hold sales, overrides and refunds, and nothing else.

use std::collections::{BTreeMap, BTreeSet};

use openbmt_core::records::{Record, RecordKind};

use crate::desk::Role;
use crate::events::Event;
use crate::money::Currency;
use crate::staff::StaffEvent;

/// How many records one block of the backward read holds.
pub const TOTALS_BLOCK: u64 = 256;
/// The most records one count reads. A day that needs more is refused, and
/// never counted in part.
pub const TOTALS_MAX_RECORDS: u64 = 20_000;
/// The most persons the lines name. Others are counted as left out.
pub const MAX_PEOPLE: usize = 64;

/// A count, with the sum of the amounts counted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Tally {
    /// How many.
    pub count: u32,
    /// The sum of the sales' own amounts in the shop's currency, in minor
    /// units, as they were when the sales opened: without the tips (CR-191).
    pub minor: u64,
    /// The sum of the satoshis, which are for the amount and the tip.
    pub sats: u64,
}

/// The tips of paid sales (CR-191).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TipTally {
    /// How many paid sales carried a tip.
    pub count: u32,
    /// The sum of those tips in the shop's currency, in minor units. They
    /// are part of what the customers paid and are not in the `minor` of the
    /// tallies, which hold the sales' own amounts.
    pub minor: u64,
}

/// What one person did in the day (CR-154).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PersonLine {
    /// Cashier or manager.
    pub role: Role,
    /// The name.
    pub name: String,
    /// Sales this person opened, on the day they were opened.
    pub sales: u32,
    /// Overrides this person made in the day.
    pub overrides: u32,
    /// Refunds this person asked for in the day and did not withdraw.
    pub refunds: u32,
    /// Cash refunds this person gave in the day.
    pub cash_refunds: u32,
    /// The tips of the paid sales this person opened, in minor units of the
    /// shop's currency (CR-191).
    pub tips_minor: u64,
}

/// The totals of one day.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DayTotals {
    /// The start of the day, in seconds since 1970 (counted).
    pub from: u64,
    /// The end of the day (not counted).
    pub to: u64,
    /// The currency of the sales and of the cash refunds, when there is one.
    pub currency: Option<Currency>,
    /// Sales paid: accepted at once, confirmed, final, accepted by a person
    /// or paid over Lightning, and not replaced since.
    pub paid: Tally,
    /// Of those, the ones paid on the chain.
    pub on_chain: Tally,
    /// Of those, the ones paid over Lightning.
    pub lightning: Tally,
    /// The tips of the sales counted in `paid` (CR-191).
    pub tips: TipTally,
    /// Sales still waiting: not paid, not ended, not stopped. `sats` is what
    /// they ask for, at their last quote.
    pub waiting: Tally,
    /// Sales whose payment was replaced or reopened by a reorganisation: not
    /// to be handed over.
    pub stopped: u32,
    /// Sales cancelled or expired.
    pub ended: u32,
    /// Overrides made in the day, with the satoshis put at risk.
    pub overrides: Tally,
    /// Refunds asked for in the day and not withdrawn, with the satoshis
    /// asked.
    pub refunds: Tally,
    /// Refund requests of the day that were withdrawn.
    pub refunds_withdrawn: u32,
    /// Cash refunds given in the day: the amount in minor units and the
    /// satoshis it was valued at.
    pub cash_refunds: Tally,
    /// Sales of the day whose record names no person.
    pub unnamed_sales: u32,
    /// The lines per person, when asked for; ordered by role (cashiers
    /// first) and name.
    pub people: Vec<PersonLine>,
    /// Persons beyond [`MAX_PEOPLE`] that are left out of `people`.
    pub people_left_out: u32,
}

/// Why the day could not be counted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TotalsError {
    /// The day ends before it starts, or is empty.
    BadWindow,
    /// The sales of the day are in two currencies.
    MixedCurrencies,
    /// A sum does not fit.
    Overflow,
    /// The day needs more than [`TOTALS_MAX_RECORDS`] records.
    TooManyRecords,
}

/// The time of a record's event, when the record holds one.
pub(crate) fn record_time(record: &Record) -> Option<u64> {
    match record.kind {
        RecordKind::CheckIn | RecordKind::Setting => None,
        RecordKind::Pairing => StaffEvent::decode(&record.body).ok().map(|e| e.at),
        RecordKind::Sale
        | RecordKind::Payment
        | RecordKind::Override
        | RecordKind::Conflict
        | RecordKind::Refund => Event::decode(&record.body).ok().map(|e| e.at()),
    }
}

/// How a sale's payment stands, as its records say.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Paid {
    No,
    OnChain(u64),
    Lightning(u64),
}

struct SaleFold {
    amount: u64,
    tip: u64,
    by: Option<(Role, String)>,
    needs_sats: u64,
    ended: bool,
    stopped: bool,
    paid: Paid,
}

#[derive(Default)]
struct PersonFold {
    tips_minor: u64,
    sales: u32,
    overrides: u32,
    refunds: u32,
    cash_refunds: u32,
}

struct Fold {
    from: u64,
    to: u64,
    want_people: bool,
    currency: Option<Currency>,
    sales: BTreeMap<u32, SaleFold>,
    unnamed_sales: u32,
    overrides: Tally,
    asked: BTreeMap<u64, (u64, Option<(Role, String)>)>,
    withdrawn: BTreeSet<u64>,
    cash: Tally,
    people: BTreeMap<(Role, String), PersonFold>,
    left_out: BTreeSet<(Role, String)>,
}

fn add(a: u64, b: u64) -> Result<u64, TotalsError> {
    a.checked_add(b).ok_or(TotalsError::Overflow)
}

fn bump(n: u32) -> Result<u32, TotalsError> {
    n.checked_add(1).ok_or(TotalsError::Overflow)
}

impl Fold {
    fn in_day(&self, at: u64) -> bool {
        at >= self.from && at < self.to
    }

    fn person(&mut self, role: Role, name: &str) -> Option<&mut PersonFold> {
        if !self.want_people {
            return None;
        }
        let key = (role, name.to_string());
        if !self.people.contains_key(&key) {
            if self.people.len() >= MAX_PEOPLE {
                self.left_out.insert(key);
                return None;
            }
            self.people.insert(key.clone(), PersonFold::default());
        }
        self.people.get_mut(&key)
    }

    fn apply(&mut self, event: &Event) -> Result<(), TotalsError> {
        let index = event.index();
        match event {
            // CR-245, CR-247: the till's reference and the records of a bill
            // are the books' and change no day's total.
            Event::Referenced { .. }
            | Event::BillReferenced { .. }
            | Event::BillOpened { .. }
            | Event::BillAmount { .. }
            | Event::BillPartPaid { .. }
            | Event::BillEnded { .. }
            | Event::BillPartSale { .. } => {}
            Event::Opened {
                at,
                amount,
                tip,
                currency,
                needs_sats,
                by,
                ..
            } => {
                if !self.in_day(*at) {
                    return Ok(());
                }
                if *self.currency.get_or_insert(*currency) != *currency {
                    return Err(TotalsError::MixedCurrencies);
                }
                self.sales.insert(
                    index,
                    SaleFold {
                        amount: amount.0,
                        tip: tip.0,
                        by: by.clone(),
                        needs_sats: *needs_sats,
                        ended: false,
                        stopped: false,
                        paid: Paid::No,
                    },
                );
                match by {
                    Some((role, name)) => {
                        if let Some(p) = self.person(*role, name) {
                            p.sales = bump(p.sales)?;
                        }
                    }
                    None => self.unnamed_sales = bump(self.unnamed_sales)?,
                }
            }
            Event::Requoted { needs_sats, .. } => {
                if let Some(s) = self.sales.get_mut(&index) {
                    s.needs_sats = *needs_sats;
                }
            }
            Event::Expired { .. } | Event::Cancelled { .. } => {
                if let Some(s) = self.sales.get_mut(&index) {
                    s.ended = true;
                }
            }
            Event::PaidAtOnce { at_risk_sats, .. } => self.pay(index, Paid::OnChain(*at_risk_sats)),
            Event::Confirmed { received_sats, .. } | Event::Final { received_sats, .. } => {
                self.pay(index, Paid::OnChain(*received_sats));
            }
            Event::PaidByLightning { amount_msat, .. } => {
                self.pay(index, Paid::Lightning(amount_msat / 1_000));
            }
            Event::Override {
                at,
                role,
                name,
                sats,
                ..
            } => {
                if let Some(s) = self.sales.get_mut(&index) {
                    if s.paid == Paid::No {
                        s.paid = Paid::OnChain(*sats);
                        s.stopped = false;
                    }
                }
                if self.in_day(*at) {
                    self.overrides.count = bump(self.overrides.count)?;
                    self.overrides.sats = add(self.overrides.sats, *sats)?;
                    if let Some(p) = self.person(*role, name) {
                        p.overrides = bump(p.overrides)?;
                    }
                }
            }
            Event::Replaced { .. } | Event::Reopened { .. } => {
                if let Some(s) = self.sales.get_mut(&index) {
                    s.paid = Paid::No;
                    s.stopped = true;
                }
            }
            Event::RefundRequested {
                at,
                number,
                role,
                name,
                sats,
                ..
            } => {
                if self.in_day(*at) {
                    self.asked
                        .insert(*number, (*sats, Some((*role, name.clone()))));
                }
            }
            Event::RefundWithdrawn { number, .. } => {
                if self.asked.contains_key(number) {
                    self.withdrawn.insert(*number);
                }
            }
            Event::RefundPaidInCash {
                at,
                role,
                name,
                minor,
                sats,
                ..
            } => {
                if self.in_day(*at) {
                    self.cash.count = bump(self.cash.count)?;
                    self.cash.minor = add(self.cash.minor, *minor)?;
                    self.cash.sats = add(self.cash.sats, *sats)?;
                    if let Some(p) = self.person(*role, name) {
                        p.cash_refunds = bump(p.cash_refunds)?;
                    }
                }
            }
            Event::Noted { .. }
            | Event::LatePayment { .. }
            | Event::LateLightning { .. }
            | Event::SurplusOwed { .. }
            | Event::Dust { .. }
            | Event::PosPaid { .. }
            | Event::PosUnpaid { .. } => {}
        }
        Ok(())
    }

    fn pay(&mut self, index: u32, paid: Paid) {
        if let Some(s) = self.sales.get_mut(&index) {
            s.paid = paid;
            s.stopped = false;
        }
    }

    fn finish(mut self) -> Result<DayTotals, TotalsError> {
        let mut t = DayTotals {
            from: self.from,
            to: self.to,
            currency: self.currency,
            unnamed_sales: self.unnamed_sales,
            overrides: self.overrides,
            cash_refunds: self.cash,
            ..DayTotals::default()
        };
        for s in self.sales.values() {
            match s.paid {
                Paid::OnChain(sats) => tally(&mut [&mut t.paid, &mut t.on_chain], s.amount, sats)?,
                Paid::Lightning(sats) => {
                    tally(&mut [&mut t.paid, &mut t.lightning], s.amount, sats)?;
                }
                Paid::No if s.ended => t.ended = bump(t.ended)?,
                Paid::No if s.stopped => t.stopped = bump(t.stopped)?,
                Paid::No => tally(&mut [&mut t.waiting], s.amount, s.needs_sats)?,
            }
        }
        // CR-191: the tips of the paid sales, in all and for the person who
        // opened each sale.
        let tipped: Vec<(u64, Option<(Role, String)>)> = self
            .sales
            .values()
            .filter(|s| s.tip > 0 && s.paid != Paid::No)
            .map(|s| (s.tip, s.by.clone()))
            .collect();
        for (tip, by) in tipped {
            t.tips.count = bump(t.tips.count)?;
            t.tips.minor = add(t.tips.minor, tip)?;
            if let Some((role, name)) = by {
                if let Some(p) = self.person(role, &name) {
                    p.tips_minor = add(p.tips_minor, tip)?;
                }
            }
        }
        let asked = std::mem::take(&mut self.asked);
        for (number, (sats, who)) in asked {
            if self.withdrawn.contains(&number) {
                t.refunds_withdrawn = bump(t.refunds_withdrawn)?;
                continue;
            }
            t.refunds.count = bump(t.refunds.count)?;
            t.refunds.sats = add(t.refunds.sats, sats)?;
            if let Some((role, name)) = who {
                if let Some(p) = self.person(role, &name) {
                    p.refunds = bump(p.refunds)?;
                }
            }
        }
        t.people = self
            .people
            .into_iter()
            .map(|((role, name), p)| PersonLine {
                role,
                name,
                sales: p.sales,
                overrides: p.overrides,
                refunds: p.refunds,
                cash_refunds: p.cash_refunds,
                tips_minor: p.tips_minor,
            })
            .collect();
        t.people_left_out = u32::try_from(self.left_out.len()).unwrap_or(u32::MAX);
        Ok(t)
    }
}

fn tally(into: &mut [&mut Tally], minor: u64, sats: u64) -> Result<(), TotalsError> {
    for t in into.iter_mut() {
        t.count = bump(t.count)?;
        t.minor = add(t.minor, minor)?;
        t.sats = add(t.sats, sats)?;
    }
    Ok(())
}

/// Fold records, oldest first, into the totals of the day `[from, to)`.
pub(crate) fn fold_day<'a>(
    records: impl Iterator<Item = &'a Record>,
    from: u64,
    to: u64,
    people: bool,
) -> Result<DayTotals, TotalsError> {
    if to <= from {
        return Err(TotalsError::BadWindow);
    }
    let mut fold = Fold {
        from,
        to,
        want_people: people,
        currency: None,
        sales: BTreeMap::new(),
        unnamed_sales: 0,
        overrides: Tally::default(),
        asked: BTreeMap::new(),
        withdrawn: BTreeSet::new(),
        cash: Tally::default(),
        people: BTreeMap::new(),
        left_out: BTreeSet::new(),
    };
    for record in records {
        match record.kind {
            RecordKind::Sale
            | RecordKind::Payment
            | RecordKind::Override
            | RecordKind::Conflict
            | RecordKind::Refund => {
                if let Ok(event) = Event::decode(&record.body) {
                    fold.apply(&event)?;
                }
            }
            RecordKind::Pairing | RecordKind::Setting | RecordKind::CheckIn => {}
        }
    }
    fold.finish()
}
