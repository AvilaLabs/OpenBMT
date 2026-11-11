//! The books: sales folded from verified chains, plus what the caller adds
//! (BK-01 to BK-16).

use std::collections::{BTreeMap, BTreeSet};

use openbmt_core::identity::DeviceId;
use openbmt_core::records::{Chain, DeviceList, RecordKind};
use openbmt_counter::{Currency, Event, Role, StaffEvent};

use crate::date::{Date, MAX_OFFSET_MINUTES, MAX_UNIX, MIN_OFFSET_MINUTES};
use crate::error::BooksError;
use crate::model::{
    CashReturn, Counting, Disposal, DisposalKind, LatePayment, LightningPayment, OverrideNote,
    Payment, PaymentList, PaymentMode, PosFacts, PosMark, Sale, SaleId, SaleKey, SaleState,
    StaffNote, TipNote,
};
use crate::value::{UnknownField, Value};

/// The most counters in one set of books.
pub const MAX_COUNTERS: usize = 256;
/// The most sales in one set of books.
pub const MAX_SALES: usize = 1_000_000;
/// The most disposals, and the most cash returns.
pub const MAX_LINES: usize = 1_000_000;
/// The most late payments, and the most overrides, on one sale.
pub const MAX_PER_SALE: usize = 16;

/// BK-15: the totals of one day, for the end-of-day screen (DESIGN §2.5).
/// Sales only: there is no wallet balance here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DayTotals {
    /// The day.
    pub day: Date,
    /// Sales that received funds.
    pub sales: u64,
    /// Of those, the ones not final yet.
    pub waiting: u64,
    /// The sum of the amounts in minor units, per currency.
    pub minor: BTreeMap<Currency, u64>,
    /// The sum of the satoshis that are known.
    pub sats: u64,
    /// Sales whose satoshis are unknown, and so are not in `sats`.
    pub sats_unknown_sales: u64,
    /// BK-39: the sum of the tips of those sales in minor units, per
    /// currency. Not in `minor`, which holds the amounts of the sales
    /// themselves.
    pub tips_minor: BTreeMap<Currency, u64>,
    /// BK-39: the same tips by the person at the till when each sale
    /// opened, per currency. A sale whose record names no person is in
    /// `tips_minor` only.
    pub tips_by_person: BTreeMap<(Currency, Role, String), u64>,
}

/// The books.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Books {
    pub(crate) sales: Vec<Sale>,
    pub(crate) disposals: Vec<Disposal>,
    pub(crate) returns: Vec<CashReturn>,
    pub(crate) high_water: BTreeMap<DeviceId, Option<u32>>,
    pub(crate) staff: Vec<StaffNote>,
}

fn check_time(at: u64) -> Result<(), BooksError> {
    if at > MAX_UNIX {
        Err(BooksError::TimeOutOfRange { at })
    } else {
        Ok(())
    }
}

fn new_sale(key: SaleKey, event: &Event) -> Option<Sale> {
    let Event::Opened {
        at,
        amount,
        tip,
        currency,
        needs_sats,
        minor_per_btc,
        source,
        by,
        ..
    } = event
    else {
        return None;
    };
    Some(Sale {
        key,
        id: SaleId::for_sale(&key),
        opened_at: *at,
        amount_minor: amount.0,
        tip: (tip.0 > 0).then(|| TipNote {
            minor: tip.0,
            by: by.clone(),
        }),
        currency: *currency,
        first_quote_sats: *needs_sats,
        quote_rate_minor_per_btc: *minor_per_btc,
        rate_source: Value::Known(source.clone()),
        last_quote_sats: *needs_sats,
        last_quote_rate_minor_per_btc: *minor_per_btc,
        last_rate_source: Value::Known(source.clone()),
        state: SaleState::Open,
        funds_seen: false,
        reached_final: false,
        first_funds_at: None,
        sats_received: Value::Unknown(UnknownField::SatsNotRecorded),
        sales_tax_minor: Value::Unknown(UnknownField::SalesTax),
        value_at_first_seen_minor: Value::Unknown(UnknownField::ValueAtFirstSeen),
        first_seen_at: None,
        payments: Value::Unknown(UnknownField::Txid),
        own_payment: false,
        address: Value::Unknown(UnknownField::Address),
        late_payments: Vec::new(),
        at_risk_sats: None,
        surplus_owed_sats: 0,
        dust_sats: 0,
        overrides: Vec::new(),
        pos: PosMark::NotRecorded,
        payment_mode: PaymentMode::OnChain,
    })
}

/// What the final record says, kept while the events are folded.
struct FinalFacts {
    first_seen_at: u64,
    valued: u64,
    payments: PaymentList,
}

/// What a sale needs besides the sale itself while its events are folded.
#[derive(Default)]
struct Pending {
    /// The satoshis of the last confirmed or final event.
    confirmed_sats: Option<u64>,
    /// The final record, until a conflict replaces the payment.
    final_record: Option<FinalFacts>,
}

fn funds(sale: &mut Sale, at: u64) {
    sale.funds_seen = true;
    if sale.first_funds_at.is_none() {
        sale.first_funds_at = Some(at);
    }
}

/// A payment that arrived after the sale ended (CR-19): the 32 bytes are the
/// transaction ID, or the payment hash of an invoice paid over Lightning.
fn late(sale: &mut Sale, at: u64, txid: [u8; 32], sats: u64) -> Result<(), BooksError> {
    if sale.late_payments.len() >= MAX_PER_SALE {
        return Err(BooksError::TooMany);
    }
    sale.late_payments.push(LatePayment { txid, sats });
    if matches!(
        sale.state,
        SaleState::Open | SaleState::Expired | SaleState::Cancelled | SaleState::LatePayment
    ) {
        sale.state = SaleState::LatePayment;
    }
    funds(sale, at);
    Ok(())
}

/// BK-05: apply one event to its sale.
fn fold(sale: &mut Sale, pending: &mut Pending, event: &Event) -> Result<(), BooksError> {
    match event {
        // Opened is handled by the caller.
        Event::Opened { .. } => {}
        Event::Requoted {
            needs_sats,
            minor_per_btc,
            source,
            ..
        } => {
            sale.last_quote_sats = *needs_sats;
            sale.last_quote_rate_minor_per_btc = *minor_per_btc;
            sale.last_rate_source = Value::Known(source.clone());
        }
        Event::Expired { .. } => sale.state = SaleState::Expired,
        Event::Cancelled { .. } => sale.state = SaleState::Cancelled,
        Event::PaidAtOnce {
            at, at_risk_sats, ..
        } => {
            sale.state = SaleState::PaidAtOnce;
            sale.at_risk_sats = Some(*at_risk_sats);
            sale.own_payment = true;
            funds(sale, *at);
        }
        Event::Confirmed {
            at, received_sats, ..
        } => {
            sale.state = SaleState::Confirmed;
            pending.confirmed_sats = Some(*received_sats);
            sale.own_payment = true;
            funds(sale, *at);
        }
        Event::Final {
            at,
            received_sats,
            first_seen_at,
            valued,
            payments,
            more,
            ..
        } => {
            check_time(*first_seen_at)?;
            sale.state = SaleState::Final;
            sale.reached_final = true;
            sale.own_payment = true;
            pending.confirmed_sats = Some(*received_sats);
            pending.final_record = Some(FinalFacts {
                first_seen_at: *first_seen_at,
                valued: valued.0,
                payments: PaymentList {
                    payments: payments
                        .iter()
                        .map(|(txid, sats)| Payment {
                            txid: *txid,
                            sats: *sats,
                        })
                        .collect(),
                    more: *more,
                },
            });
            funds(sale, *at);
        }
        Event::PaidByLightning {
            at,
            payment_hash,
            amount_msat,
            settled_at,
            valued,
            ..
        } => {
            // BK-32: final at once, with no transaction of its own.
            if let Some(t) = settled_at {
                check_time(*t)?;
            }
            sale.state = SaleState::Final;
            sale.reached_final = true;
            sale.own_payment = true;
            sale.payment_mode = PaymentMode::Lightning(LightningPayment {
                payment_hash: *payment_hash,
                amount_msat: *amount_msat,
                settled_at: *settled_at,
            });
            pending.confirmed_sats = Some(*amount_msat / 1_000);
            pending.final_record = Some(FinalFacts {
                first_seen_at: *at,
                valued: valued.0,
                payments: PaymentList {
                    payments: Vec::new(),
                    more: false,
                },
            });
            funds(sale, *at);
        }
        Event::LatePayment { at, txid, sats, .. } => late(sale, *at, *txid, *sats)?,
        // The payment hash stands where a transaction ID does.
        Event::LateLightning {
            at,
            payment_hash,
            amount_msat,
            ..
        } => late(sale, *at, *payment_hash, (*amount_msat / 1_000).max(1))?,
        Event::SurplusOwed { sats, .. } => {
            sale.surplus_owed_sats = sale
                .surplus_owed_sats
                .checked_add(*sats)
                .ok_or(BooksError::Overflow)?;
        }
        Event::Dust { sats, .. } => {
            sale.dust_sats = sale
                .dust_sats
                .checked_add(*sats)
                .ok_or(BooksError::Overflow)?;
        }
        Event::Override {
            role, name, sats, ..
        } => {
            if sale.overrides.len() >= MAX_PER_SALE {
                return Err(BooksError::TooMany);
            }
            sale.overrides.push(OverrideNote {
                role: *role,
                name: name.clone(),
                sats: *sats,
            });
            sale.own_payment = true;
        }
        Event::Replaced { .. } => {
            sale.state = SaleState::Replaced;
            sale.funds_seen = false;
            sale.reached_final = false;
            sale.first_funds_at = None;
            sale.own_payment = false;
            pending.confirmed_sats = None;
            pending.final_record = None;
        }
        Event::Reopened { .. } => {
            sale.state = SaleState::Reopened;
            sale.reached_final = false;
        }
        Event::PosPaid { .. } => sale.pos = PosMark::Paid,
        Event::PosUnpaid { .. } => sale.pos = PosMark::Unpaid,
        // BK-02: a request and a withdrawal are asks, not money; they are
        // not a disposal and change nothing here. A cash refund becomes a
        // return line where the events are read.
        Event::RefundRequested { .. }
        | Event::RefundWithdrawn { .. }
        | Event::RefundPaidInCash { .. } => {}
    }
    Ok(())
}

/// BK-09: the satoshis received.
fn sats_of(sale: &Sale, pending: &Pending) -> Result<Value<u64>, BooksError> {
    let late = sale
        .late_payments
        .iter()
        .try_fold(0u64, |a, p| a.checked_add(p.sats))
        .ok_or(BooksError::Overflow)?;
    Ok(
        match (pending.confirmed_sats, sale.late_payments.is_empty()) {
            (Some(c), true) => Value::Known(c),
            (None, false) => Value::Known(late),
            (Some(c), false) => match &pending.final_record {
                None => Value::Unknown(UnknownField::SatsLateAmbiguous),
                Some(f) => late_beyond_final(c, sale, &f.payments)?,
            },
            (None, true) if sale.state == SaleState::Expired && !sale.funds_seen => Value::Known(0),
            (None, true) => Value::Unknown(UnknownField::SatsNotRecorded),
        },
    )
}

/// BK-09: the final record names its transactions, so a late payment is
/// either one of them (already in the amount) or not (then it is added). If
/// the record was cut short and a late payment is not among the ones it
/// holds, it may be among the ones it left out, and the amount stays unknown.
fn late_beyond_final(
    confirmed: u64,
    sale: &Sale,
    named: &PaymentList,
) -> Result<Value<u64>, BooksError> {
    let mut total = confirmed;
    for p in &sale.late_payments {
        if named.payments.iter().any(|n| n.txid == p.txid) {
            continue;
        }
        if named.more {
            return Ok(Value::Unknown(UnknownField::SatsLateAmbiguous));
        }
        total = total.checked_add(p.sats).ok_or(BooksError::Overflow)?;
    }
    Ok(Value::Known(total))
}

impl Books {
    /// BK-01 to BK-09, BK-11: verify every chain and fold its events into
    /// sales. One bad chain refuses the whole build.
    pub fn from_chains(
        devices: &DeviceList,
        chains: &[(DeviceId, &Chain)],
    ) -> Result<Self, BooksError> {
        if chains.len() > MAX_COUNTERS {
            return Err(BooksError::TooMany);
        }
        let mut books = Self::default();
        let mut seen = BTreeSet::new();
        for (counter, chain) in chains {
            if !devices.contains(counter) {
                return Err(BooksError::CounterNotOnList { counter: *counter });
            }
            if !seen.insert(*counter) {
                return Err(BooksError::DuplicateCounter { counter: *counter });
            }
            chain
                .verify(devices)
                .map_err(|error| BooksError::BadChain {
                    counter: *counter,
                    error,
                })?;
            let mut folding: BTreeMap<u32, (Sale, Pending)> = BTreeMap::new();
            let mut cash_returns: Vec<CashReturn> = Vec::new();
            let mut high: Option<u32> = None;
            for record in chain.records() {
                if &record.author != counter {
                    continue;
                }
                match record.kind {
                    RecordKind::Sale
                    | RecordKind::Payment
                    | RecordKind::Override
                    | RecordKind::Conflict
                    | RecordKind::Refund => {}
                    RecordKind::Pairing => {
                        // BK-35: a change to the cashiers, read strictly.
                        let change = StaffEvent::decode(&record.body).map_err(|error| {
                            BooksError::BadEvent {
                                counter: *counter,
                                seq: record.seq,
                                error,
                            }
                        })?;
                        check_time(change.at)?;
                        if books.staff.len() >= MAX_LINES {
                            return Err(BooksError::TooMany);
                        }
                        books.staff.push(StaffNote {
                            counter: *counter,
                            at: change.at,
                            change: change.change,
                            name: change.name,
                            by: change.by,
                        });
                        continue;
                    }
                    RecordKind::Setting | RecordKind::CheckIn => continue,
                }
                let event = Event::decode(&record.body).map_err(|error| BooksError::BadEvent {
                    counter: *counter,
                    seq: record.seq,
                    error,
                })?;
                if event.kind() != record.kind {
                    return Err(BooksError::KindMismatch {
                        counter: *counter,
                        seq: record.seq,
                    });
                }
                let index = event.index();
                let key = SaleKey {
                    counter: *counter,
                    index,
                };
                if let Some(at) = event_time(&event) {
                    check_time(at)?;
                }
                high = Some(high.map_or(index, |h| h.max(index)));
                if let Some(sale) = new_sale(key, &event) {
                    if folding.contains_key(&index) {
                        return Err(BooksError::OpenedTwice {
                            counter: *counter,
                            seq: record.seq,
                            index,
                        });
                    }
                    folding.insert(index, (sale, Pending::default()));
                } else {
                    let (sale, pending) =
                        folding.get_mut(&index).ok_or(BooksError::UnknownSale {
                            counter: *counter,
                            seq: record.seq,
                            index,
                        })?;
                    fold(sale, pending, &event)?;
                    if let Event::RefundPaidInCash { at, minor, .. } = &event {
                        // BK-13: a cash refund is a return line.
                        if books.returns.len().saturating_add(cash_returns.len()) >= MAX_LINES {
                            return Err(BooksError::TooMany);
                        }
                        cash_returns.push(CashReturn {
                            at: *at,
                            sale: key,
                            minor: *minor,
                        });
                    }
                }
                if books.sales.len().saturating_add(folding.len()) > MAX_SALES {
                    return Err(BooksError::TooMany);
                }
            }
            for (_, (mut sale, pending)) in folding {
                sale.sats_received = sats_of(&sale, &pending)?;
                if let Some(f) = pending.final_record {
                    sale.first_seen_at = Some(f.first_seen_at);
                    sale.value_at_first_seen_minor = Value::Known(f.valued);
                    sale.payments = Value::Known(f.payments);
                }
                books.sales.push(sale);
            }
            books.returns.append(&mut cash_returns);
            books.high_water.insert(*counter, high);
        }
        books.sales.sort_by_key(|s| s.key);
        books.returns.sort_by_key(|r| r.at);
        books.check_return_sums()?;
        Ok(books)
    }

    /// BK-10: add the sales tax the POS match gave. A tax amount for a sale
    /// that is not in the books refuses it.
    pub fn with_pos_facts(mut self, facts: &PosFacts) -> Result<Self, BooksError> {
        for (key, _) in facts.iter() {
            if self.sale(key).is_none() {
                return Err(BooksError::SaleNotInBooks { sale: *key });
            }
        }
        for sale in &mut self.sales {
            if let Some(tax) = facts.tax(&sale.key) {
                sale.sales_tax_minor = Value::Known(tax);
            }
        }
        Ok(self)
    }

    /// BK-12, BK-14: set the disposals. They are kept in time order. A
    /// refund must name a sale in the books, and every sum must fit.
    pub fn with_disposals(mut self, mut disposals: Vec<Disposal>) -> Result<Self, BooksError> {
        if disposals.len() > MAX_LINES {
            return Err(BooksError::TooMany);
        }
        let mut sats = 0u64;
        let mut value: BTreeMap<Currency, u64> = BTreeMap::new();
        for d in &disposals {
            check_time(d.at)?;
            if let DisposalKind::Refund { sale } = d.kind {
                if self.sale(&sale).is_none() {
                    return Err(BooksError::SaleNotInBooks { sale });
                }
            }
            sats = sats.checked_add(d.sats).ok_or(BooksError::Overflow)?;
            for v in [d.minor_value, d.basis.map(|b| b.minor)]
                .into_iter()
                .flatten()
            {
                let e = value.entry(d.currency).or_insert(0);
                *e = e.checked_add(v).ok_or(BooksError::Overflow)?;
            }
        }
        disposals.sort_by_key(|d| d.at);
        self.disposals = disposals;
        Ok(self)
    }

    /// BK-13, BK-14: add cash returns to the ones the counters' records
    /// carry (`RefundPaidInCash`). They are kept in time order. Each must name
    /// a sale in the books.
    pub fn with_cash_returns(mut self, returns: Vec<CashReturn>) -> Result<Self, BooksError> {
        if returns.len().saturating_add(self.returns.len()) > MAX_LINES {
            return Err(BooksError::TooMany);
        }
        for r in &returns {
            check_time(r.at)?;
            if self.sale(&r.sale).is_none() {
                return Err(BooksError::SaleNotInBooks { sale: r.sale });
            }
        }
        self.returns.extend(returns);
        self.returns.sort_by_key(|r| r.at);
        self.check_return_sums()?;
        Ok(self)
    }

    /// BK-14: the returns of each sale add up inside 64 bits.
    fn check_return_sums(&self) -> Result<(), BooksError> {
        let mut per_sale: BTreeMap<SaleKey, u64> = BTreeMap::new();
        for r in &self.returns {
            let e = per_sale.entry(r.sale).or_insert(0);
            *e = e.checked_add(r.minor).ok_or(BooksError::Overflow)?;
        }
        Ok(())
    }

    /// The sales, ordered by counter and sale index.
    pub fn sales(&self) -> &[Sale] {
        &self.sales
    }

    /// The disposals, in time order.
    pub fn disposals(&self) -> &[Disposal] {
        &self.disposals
    }

    /// The cash returns, in time order.
    pub fn cash_returns(&self) -> &[CashReturn] {
        &self.returns
    }

    /// BK-11: the highest sale index each counter's events name, or `None`
    /// for a counter with no events.
    pub fn high_water(&self) -> &BTreeMap<DeviceId, Option<u32>> {
        &self.high_water
    }

    /// BK-36: the changes to the cashiers the counters' records carry, in
    /// the order of each counter's chain (counters in the order they were
    /// given).
    pub fn staff_changes(&self) -> &[StaffNote] {
        &self.staff
    }

    /// One sale.
    pub fn sale(&self, key: &SaleKey) -> Option<&Sale> {
        self.sales
            .binary_search_by(|s| s.key.cmp(key))
            .ok()
            .and_then(|i| self.sales.get(i))
    }

    /// BK-15: the totals per UTC day, oldest day first.
    pub fn utc_day_totals(&self) -> Result<Vec<DayTotals>, BooksError> {
        self.day_totals(0)
    }

    /// BK-15, BK-16: the totals per shop-local day, for a fixed offset from
    /// UTC in minutes east (-720 to 840), oldest day first.
    pub fn local_day_totals(&self, offset_minutes: i32) -> Result<Vec<DayTotals>, BooksError> {
        self.day_totals(offset_minutes)
    }

    fn day_totals(&self, offset_minutes: i32) -> Result<Vec<DayTotals>, BooksError> {
        if !(MIN_OFFSET_MINUTES..=MAX_OFFSET_MINUTES).contains(&offset_minutes) {
            return Err(BooksError::BadOffset {
                minutes: offset_minutes,
            });
        }
        let mut days: BTreeMap<Date, DayTotals> = BTreeMap::new();
        for sale in &self.sales {
            let Some(at) = sale.first_funds_at.filter(|_| sale.funds_seen) else {
                continue;
            };
            let day = Date::from_unix_offset(at, offset_minutes)
                .ok_or(BooksError::TimeOutOfRange { at })?;
            let t = days.entry(day).or_insert_with(|| DayTotals {
                day,
                sales: 0,
                waiting: 0,
                minor: BTreeMap::new(),
                sats: 0,
                sats_unknown_sales: 0,
                tips_minor: BTreeMap::new(),
                tips_by_person: BTreeMap::new(),
            });
            t.sales = t.sales.checked_add(1).ok_or(BooksError::Overflow)?;
            if sale.counting() == Counting::Pending {
                t.waiting = t.waiting.checked_add(1).ok_or(BooksError::Overflow)?;
            }
            let m = t.minor.entry(sale.currency).or_insert(0);
            *m = m
                .checked_add(sale.amount_minor)
                .ok_or(BooksError::Overflow)?;
            if let Some(tip) = &sale.tip {
                let all = t.tips_minor.entry(sale.currency).or_insert(0);
                *all = all.checked_add(tip.minor).ok_or(BooksError::Overflow)?;
                if let Some((role, name)) = &tip.by {
                    let one = t
                        .tips_by_person
                        .entry((sale.currency, *role, name.clone()))
                        .or_insert(0);
                    *one = one.checked_add(tip.minor).ok_or(BooksError::Overflow)?;
                }
            }
            match sale.sats_received {
                Value::Known(s) => t.sats = t.sats.checked_add(s).ok_or(BooksError::Overflow)?,
                Value::Unknown(_) => {
                    t.sats_unknown_sales = t
                        .sats_unknown_sales
                        .checked_add(1)
                        .ok_or(BooksError::Overflow)?;
                }
            }
        }
        Ok(days.into_values().collect())
    }
}

fn event_time(event: &Event) -> Option<u64> {
    Some(match event {
        Event::Opened { at, .. }
        | Event::Requoted { at, .. }
        | Event::Expired { at, .. }
        | Event::Cancelled { at, .. }
        | Event::PaidAtOnce { at, .. }
        | Event::Confirmed { at, .. }
        | Event::Final { at, .. }
        | Event::PaidByLightning { at, .. }
        | Event::LatePayment { at, .. }
        | Event::LateLightning { at, .. }
        | Event::SurplusOwed { at, .. }
        | Event::Dust { at, .. }
        | Event::Override { at, .. }
        | Event::Replaced { at, .. }
        | Event::Reopened { at, .. }
        | Event::PosPaid { at, .. }
        | Event::PosUnpaid { at, .. }
        | Event::RefundRequested { at, .. }
        | Event::RefundWithdrawn { at, .. }
        | Event::RefundPaidInCash { at, .. } => *at,
    })
}
