//! What is kept between polls (PS-36, PS-50 to PS-52, PS-60 to PS-62,
//! PS-70 to PS-74): the tracked orders, the matches, the alerts and the POS
//! cursor, and the stored form of all of it (`OBMQ`).
//!
//! A change is made by building the new value, handing its stored form to the
//! caller's `persist`, and swapping only if that returned `Ok`.

use std::collections::BTreeMap;
use std::fmt;

use openbmt_core::wire::{self, DecodeError, Reader};
use openbmt_counter::Currency;

use crate::alerts::{Alert, AlertKey, AlertKind, Subject, TenderKey};
use crate::connector::Cursor;
use crate::facts::{SaleFact, SaleKey};
use crate::matcher::{evaluate, Input, Match, MatchHow};
use crate::model::{
    get_id, put_str, ModelError, OrderId, OrderState, PosOrder, TenderId, TenderState,
};
use crate::reference::tagged_hash;
use crate::settings::{Settings, SettingsError};
use crate::{MAX_ALERTS, MAX_CURSOR_BYTES, MAX_MATCHES, MAX_SALES, MAX_TRACKED_ORDERS};

/// Magic of a stored ledger. No other stored form in this workspace uses it.
pub const MAGIC_POS_LEDGER: [u8; 4] = *b"OBMQ";
/// Tag of the hash that ends a stored ledger.
pub const TAG_LEDGER: &str = "OpenBMT/pos-ledger/v1";

/// Why a ledger change was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LedgerError {
    /// The settings do not hold.
    Settings(SettingsError),
    /// An order handed in breaks its bounds (PS-01, PS-02).
    Order(ModelError),
    /// More sales than the matcher takes (PS-72).
    TooManySales,
    /// Two facts name the same sale.
    DuplicateSale(SaleKey),
    /// The ledger would grow past a bound (PS-72). The text says which.
    Full(&'static str),
    /// PS-52: the alert to acknowledge is not open.
    NotOpen,
}

impl fmt::Display for LedgerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Settings(e) => write!(f, "{e}"),
            Self::Order(e) => write!(f, "{e}"),
            Self::TooManySales => write!(f, "PS-72: more sales than the matcher takes"),
            Self::DuplicateSale(k) => {
                write!(f, "two facts name sale {} of the same counter", k.index)
            }
            Self::Full(what) => write!(
                f,
                "PS-72: the ledger is full ({what}); acknowledge alerts or raise the bound"
            ),
            Self::NotOpen => write!(f, "PS-52: that alert is not open"),
        }
    }
}

impl std::error::Error for LedgerError {}

/// Why a stored change did not happen (PS-70).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LedgerWriteError<E> {
    /// The change was refused. The ledger is unchanged.
    Refused(LedgerError),
    /// The caller's store failed. The ledger is unchanged.
    Persist(E),
}

impl<E: fmt::Display> fmt::Display for LedgerWriteError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Refused(e) => write!(f, "{e}"),
            Self::Persist(e) => write!(f, "could not store the ledger: {e}"),
        }
    }
}

impl<E: fmt::Debug + fmt::Display> std::error::Error for LedgerWriteError<E> {}

/// An acknowledged alert (PS-52).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Acknowledged {
    /// The alert as it was.
    pub alert: Alert,
    /// When the owner acknowledged it.
    pub at: u64,
}

/// What a change made new (PS-60).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Delta {
    /// Matches that were not there before, in tender order.
    pub new_matches: Vec<(TenderKey, Match)>,
    /// Alerts raised, in key order.
    pub new_alerts: Vec<Alert>,
    /// Alerts cleared because their condition went, in key order.
    pub cleared: Vec<AlertKey>,
    /// Matches retired into the count.
    pub retired_matches: u64,
}

/// What the owner sees at a glance (PS-61).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Summary {
    /// Alerts waiting for the owner.
    pub open_alerts: usize,
    /// Alerts the owner acknowledged.
    pub acknowledged_alerts: usize,
    /// Tracked orders.
    pub tracked_orders: usize,
    /// Matches held.
    pub matches: usize,
    /// Of those, matched by order of time and not exact (PS-34).
    pub matches_by_order: usize,
    /// Matches retired and counted (PS-73).
    pub retired_matches: u64,
}

/// A line for the books (PS-62). Nothing in it is a transaction ID, an
/// address or an amount in satoshis.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BooksLine {
    /// The POS order.
    pub order: OrderId,
    /// The POS tender.
    pub tender: TenderId,
    /// The counter sale.
    pub sale: SaleKey,
    /// The sale's amount in minor units.
    pub amount_minor: u64,
    /// The currency.
    pub currency: Currency,
}

/// The POS side's memory.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Ledger {
    cursor: Cursor,
    orders: BTreeMap<OrderId, PosOrder>,
    matches: BTreeMap<TenderKey, Match>,
    open: BTreeMap<AlertKey, Alert>,
    acked: BTreeMap<AlertKey, Acknowledged>,
    retired_matches: u64,
}

/// The order as the ledger keeps it: only Bitcoin tenders, in id order.
fn trimmed(order: &PosOrder, settings: &Settings) -> PosOrder {
    let mut o = order.clone();
    o.tenders.retain(|t| settings.is_bitcoin(t));
    o.tenders.sort_by(|a, b| a.id.cmp(&b.id));
    o
}

/// What decides which of two versions of one order is newer.
fn version(order: &PosOrder) -> (u64, Vec<u8>) {
    let mut bytes = Vec::new();
    order.encode(&mut bytes);
    (order.updated_at, bytes)
}

impl Ledger {
    /// An empty ledger, with the cursor at the start.
    pub fn new() -> Self {
        Self::default()
    }

    /// The POS cursor the next poll starts from.
    pub fn cursor(&self) -> &Cursor {
        &self.cursor
    }

    /// The tracked orders (PS-72), only their Bitcoin tenders.
    pub fn orders(&self) -> &BTreeMap<OrderId, PosOrder> {
        &self.orders
    }

    /// The matches held.
    pub fn matches(&self) -> &BTreeMap<TenderKey, Match> {
        &self.matches
    }

    /// Matches retired into the count (PS-73).
    pub fn retired_matches(&self) -> u64 {
        self.retired_matches
    }

    /// PS-61: the open alerts, oldest first.
    pub fn open_alerts(&self) -> Vec<&Alert> {
        let mut v: Vec<&Alert> = self.open.values().collect();
        v.sort_by(|a, b| (a.raised_at, &a.key).cmp(&(b.raised_at, &b.key)));
        v
    }

    /// The acknowledged alerts, in key order.
    pub fn acknowledged_alerts(&self) -> impl Iterator<Item = &Acknowledged> {
        self.acked.values()
    }

    /// Whether an alert is open.
    pub fn is_open(&self, key: &AlertKey) -> bool {
        self.open.contains_key(key)
    }

    /// Whether an alert is acknowledged.
    pub fn is_acknowledged(&self, key: &AlertKey) -> bool {
        self.acked.contains_key(key)
    }

    /// PS-61: the counts for the owner.
    pub fn summary(&self) -> Summary {
        Summary {
            open_alerts: self.open.len(),
            acknowledged_alerts: self.acked.len(),
            tracked_orders: self.orders.len(),
            matches: self.matches.len(),
            matches_by_order: self.matches.values().filter(|m| m.by_order).count(),
            retired_matches: self.retired_matches,
        }
    }

    /// PS-62: a line for each match whose sale is final in `facts` and whose
    /// tender still stands in the POS.
    pub fn books_lines(&self, facts: &[SaleFact]) -> Vec<BooksLine> {
        let by_key: BTreeMap<SaleKey, &SaleFact> = facts.iter().map(|f| (f.key(), f)).collect();
        let mut out = Vec::new();
        for (key, m) in &self.matches {
            let Some(order) = self.orders.get(&key.order) else {
                continue;
            };
            let live = order.state != OrderState::Voided
                && order
                    .tender(&key.tender)
                    .is_some_and(|t| t.state == TenderState::Paid);
            if let Some(f) = by_key.get(&m.sale).filter(|f| f.is_final && live) {
                out.push(BooksLine {
                    order: key.order.clone(),
                    tender: key.tender.clone(),
                    sale: m.sale,
                    amount_minor: f.amount.0,
                    currency: f.currency,
                });
            }
        }
        out
    }

    /// Build the ledger that follows from one poll: merge the orders, work
    /// out the matches and the alerts, raise what is new, clear what is
    /// resolved and retire what is settled. `self` is not changed.
    pub fn updated(
        &self,
        incoming: &[PosOrder],
        next: Cursor,
        facts: &[SaleFact],
        settings: &Settings,
        now: u64,
    ) -> Result<(Self, Delta), LedgerError> {
        settings.validate().map_err(LedgerError::Settings)?;
        if facts.len() > MAX_SALES {
            return Err(LedgerError::TooManySales);
        }
        let mut fact_map: BTreeMap<SaleKey, SaleFact> = BTreeMap::new();
        for f in facts {
            if fact_map.insert(f.key(), f.clone()).is_some() {
                return Err(LedgerError::DuplicateSale(f.key()));
            }
        }

        // Orders: the newest version of each wins; an older one is ignored.
        let mut newest: BTreeMap<OrderId, PosOrder> = BTreeMap::new();
        for o in incoming {
            o.validate().map_err(LedgerError::Order)?;
            let t = trimmed(o, settings);
            let better = newest
                .get(&t.id)
                .is_none_or(|have| version(&t) > version(have));
            if better {
                newest.insert(t.id.clone(), t);
            }
        }
        let mut orders = self.orders.clone();
        for (id, t) in newest {
            match orders.get(&id) {
                Some(have) if version(&t) <= version(have) => {}
                None if t.tenders.is_empty() => {}
                _ => {
                    orders.insert(id, t);
                }
            }
        }

        let out = evaluate(&Input {
            settings,
            orders: &orders,
            prior: &self.matches,
            facts: &fact_map,
            now,
        });
        let mut matches = out.matches;

        // Alerts: raised once, cleared by resolution (PS-50, PS-51).
        let mut open = self.open.clone();
        let mut acked = self.acked.clone();
        let mut delta = Delta::default();
        for (k, a) in &out.alerts {
            if open.contains_key(k) || acked.contains_key(k) {
                continue;
            }
            let too_old = k.kind == AlertKind::PaymentWithoutTender
                && now.saturating_sub(a.at) >= settings.timing.retire_secs;
            if !too_old {
                open.insert(k.clone(), a.clone());
                delta.new_alerts.push(a.clone());
            }
        }
        let resolvable = |a: &Alert| a.sale.is_none_or(|s| fact_map.contains_key(&s));
        let gone: Vec<AlertKey> = open
            .iter()
            .filter(|(k, a)| !out.alerts.contains_key(k) && resolvable(a))
            .map(|(k, _)| k.clone())
            .collect();
        for k in gone {
            open.remove(&k);
            delta.cleared.push(k);
        }
        let gone: Vec<AlertKey> = acked
            .iter()
            .filter(|(k, _)| !out.alerts.contains_key(k))
            .map(|(k, _)| k.clone())
            .collect();
        for k in gone {
            acked.remove(&k);
            delta.cleared.push(k);
        }
        delta.cleared.sort();

        // Retirement (PS-73).
        let retire = settings.timing.retire_secs;
        let retired: Vec<OrderId> = orders
            .values()
            .filter(|o| {
                let unchanged = o.updated_at.saturating_add(retire) <= now;
                let mine = |k: &TenderKey| k.order == o.id;
                let no_open = !open
                    .keys()
                    .any(|k| matches!(&k.subject, Subject::Tender(t) if mine(t)));
                let sales_settled = matches.iter().filter(|(k, _)| mine(k)).all(|(_, m)| {
                    fact_map.get(&m.sale).is_some_and(|f| {
                        f.is_final && f.paid_at.is_some_and(|p| p.saturating_add(retire) <= now)
                    })
                });
                let handled = |k: &TenderKey| {
                    acked
                        .keys()
                        .any(|a| matches!(&a.subject, Subject::Tender(t) if t == k))
                };
                let unmatched_handled = o.state == OrderState::Voided
                    || o.tenders
                        .iter()
                        .filter(|t| t.state == TenderState::Paid)
                        .map(|t| TenderKey {
                            order: o.id.clone(),
                            tender: t.id.clone(),
                        })
                        .all(|k| matches.contains_key(&k) || handled(&k));
                unchanged && no_open && sales_settled && unmatched_handled
            })
            .map(|o| o.id.clone())
            .collect();
        for id in &retired {
            orders.remove(id);
            let before = matches.len();
            matches.retain(|k, _| &k.order != id);
            let n = u64::try_from(before.saturating_sub(matches.len())).unwrap_or(u64::MAX);
            delta.retired_matches = delta.retired_matches.saturating_add(n);
            acked.retain(|k, _| !matches!(&k.subject, Subject::Tender(t) if &t.order == id));
        }

        delta.new_matches = matches
            .iter()
            .filter(|(k, m)| self.matches.get(*k).is_none_or(|p| p.sale != m.sale))
            .map(|(k, m)| (k.clone(), m.clone()))
            .collect();

        let next_ledger = Self {
            cursor: next,
            orders,
            matches,
            open,
            acked,
            retired_matches: self.retired_matches.saturating_add(delta.retired_matches),
        };
        next_ledger.check_bounds()?;
        Ok((next_ledger, delta))
    }

    /// PS-70: build the ledger that follows from one poll, hand its stored
    /// form to `persist`, and swap only if that returned `Ok`.
    pub fn update_stored<E>(
        &mut self,
        incoming: &[PosOrder],
        next: Cursor,
        facts: &[SaleFact],
        settings: &Settings,
        now: u64,
        persist: impl FnOnce(&[u8]) -> Result<(), E>,
    ) -> Result<Delta, LedgerWriteError<E>> {
        let (new, delta) = self
            .updated(incoming, next, facts, settings, now)
            .map_err(LedgerWriteError::Refused)?;
        persist(&new.to_bytes()).map_err(LedgerWriteError::Persist)?;
        *self = new;
        Ok(delta)
    }

    /// PS-52: the owner acknowledges an open alert. The change is stored as
    /// in [`Self::update_stored`]. Signing the acknowledgement as the owner's
    /// command is not done here.
    pub fn acknowledge_stored<E>(
        &mut self,
        key: &AlertKey,
        at: u64,
        persist: impl FnOnce(&[u8]) -> Result<(), E>,
    ) -> Result<(), LedgerWriteError<E>> {
        let mut new = self.clone();
        let alert = new
            .open
            .remove(key)
            .ok_or(LedgerWriteError::Refused(LedgerError::NotOpen))?;
        new.acked.insert(key.clone(), Acknowledged { alert, at });
        new.check_bounds().map_err(LedgerWriteError::Refused)?;
        persist(&new.to_bytes()).map_err(LedgerWriteError::Persist)?;
        *self = new;
        Ok(())
    }

    fn check_bounds(&self) -> Result<(), LedgerError> {
        if self.orders.len() > MAX_TRACKED_ORDERS {
            Err(LedgerError::Full("tracked orders"))
        } else if self.matches.len() > MAX_MATCHES {
            Err(LedgerError::Full("matches"))
        } else if self.open.len() > MAX_ALERTS || self.acked.len() > MAX_ALERTS {
            Err(LedgerError::Full("alerts"))
        } else {
            Ok(())
        }
    }

    /// PS-71: the stored form. Keys are in ascending order and a tagged hash
    /// of everything before it ends the bytes.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = wire::start(&MAGIC_POS_LEDGER, 256);
        let cursor = self.cursor.as_bytes();
        out.extend_from_slice(
            &u16::try_from(cursor.len())
                .unwrap_or(u16::MAX)
                .to_le_bytes(),
        );
        out.extend_from_slice(cursor);
        out.extend_from_slice(&self.retired_matches.to_le_bytes());
        put_count(&mut out, self.orders.len());
        for o in self.orders.values() {
            o.encode(&mut out);
        }
        put_count(&mut out, self.matches.len());
        for (k, m) in &self.matches {
            put_str(&mut out, k.order.as_str());
            put_str(&mut out, k.tender.as_str());
            put_sale(&mut out, &m.sale);
            out.push(match m.how {
                MatchHow::Reference => 1,
                MatchHow::AmountAndTime => 2,
            });
            out.push(u8::from(m.by_order));
            out.extend_from_slice(&m.matched_at.to_le_bytes());
        }
        put_count(&mut out, self.open.len());
        for a in self.open.values() {
            put_alert(&mut out, a);
        }
        put_count(&mut out, self.acked.len());
        for a in self.acked.values() {
            put_alert(&mut out, &a.alert);
            out.extend_from_slice(&a.at.to_le_bytes());
        }
        let tag = tagged_hash(TAG_LEDGER, &[&out]);
        out.extend_from_slice(&tag);
        out
    }

    /// Read a stored ledger strictly (PS-71). Anything that `to_bytes` would
    /// not have written is refused.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, DecodeError> {
        let split = bytes.len().checked_sub(32).ok_or(DecodeError::Truncated)?;
        let (body, tag) = bytes.split_at(split);
        if tagged_hash(TAG_LEDGER, &[body]).as_slice() != tag {
            return Err(DecodeError::NonCanonical("the check hash does not match"));
        }
        let mut r = Reader::framed(body, &MAGIC_POS_LEDGER)?;
        let cursor_len = usize::from(r.u16_le()?);
        if cursor_len > MAX_CURSOR_BYTES {
            return Err(DecodeError::TooLarge);
        }
        let cursor =
            Cursor::new(r.take(cursor_len)?.to_vec()).map_err(|_| DecodeError::TooLarge)?;
        let retired_matches = r.u64_le()?;

        let mut orders: BTreeMap<OrderId, PosOrder> = BTreeMap::new();
        for _ in 0..r.count(MAX_TRACKED_ORDERS, 40)? {
            let o = PosOrder::decode(&mut r)?;
            if orders.last_key_value().is_some_and(|(k, _)| *k >= o.id) {
                return Err(DecodeError::NonCanonical("orders are not in id order"));
            }
            orders.insert(o.id.clone(), o);
        }
        let mut matches: BTreeMap<TenderKey, Match> = BTreeMap::new();
        for _ in 0..r.count(MAX_MATCHES, 50)? {
            let key = TenderKey {
                order: get_id(&mut r)?,
                tender: get_id(&mut r)?,
            };
            let sale = get_sale(&mut r)?;
            let how = match r.u8()? {
                1 => MatchHow::Reference,
                2 => MatchHow::AmountAndTime,
                _ => return Err(DecodeError::NonCanonical("a match kind is not known")),
            };
            let by_order = r.flag()?;
            let matched_at = r.u64_le()?;
            if !orders.contains_key(&key.order) {
                return Err(DecodeError::NonCanonical(
                    "a match names an untracked order",
                ));
            }
            if matches.last_key_value().is_some_and(|(k, _)| *k >= key) {
                return Err(DecodeError::NonCanonical("matches are not in key order"));
            }
            matches.insert(
                key,
                Match {
                    sale,
                    how,
                    by_order,
                    matched_at,
                },
            );
        }
        let mut open: BTreeMap<AlertKey, Alert> = BTreeMap::new();
        for _ in 0..r.count(MAX_ALERTS, 50)? {
            let a = get_alert(&mut r, &orders)?;
            if open.last_key_value().is_some_and(|(k, _)| *k >= a.key) {
                return Err(DecodeError::NonCanonical("alerts are not in key order"));
            }
            open.insert(a.key.clone(), a);
        }
        let mut acked: BTreeMap<AlertKey, Acknowledged> = BTreeMap::new();
        for _ in 0..r.count(MAX_ALERTS, 58)? {
            let alert = get_alert(&mut r, &orders)?;
            let at = r.u64_le()?;
            if acked.last_key_value().is_some_and(|(k, _)| *k >= alert.key)
                || open.contains_key(&alert.key)
            {
                return Err(DecodeError::NonCanonical(
                    "acknowledged alerts are not canonical",
                ));
            }
            acked.insert(alert.key.clone(), Acknowledged { alert, at });
        }
        r.finish()?;
        Ok(Self {
            cursor,
            orders,
            matches,
            open,
            acked,
            retired_matches,
        })
    }
}

fn put_count(out: &mut Vec<u8>, n: usize) {
    out.extend_from_slice(&u32::try_from(n).unwrap_or(u32::MAX).to_le_bytes());
}

fn put_sale(out: &mut Vec<u8>, s: &SaleKey) {
    out.extend_from_slice(&s.counter.to_bytes());
    out.extend_from_slice(&s.index.to_le_bytes());
}

fn get_sale(r: &mut Reader<'_>) -> Result<SaleKey, DecodeError> {
    Ok(SaleKey {
        counter: r.device_id()?,
        index: r.u32_le()?,
    })
}

fn put_alert(out: &mut Vec<u8>, a: &Alert) {
    out.push(a.key.kind.to_u8());
    match &a.key.subject {
        Subject::Tender(t) => {
            out.push(1);
            put_str(out, t.order.as_str());
            put_str(out, t.tender.as_str());
        }
        Subject::Sale(s) => {
            out.push(2);
            put_sale(out, s);
        }
    }
    match &a.sale {
        Some(s) => {
            out.push(1);
            put_sale(out, s);
        }
        None => out.push(0),
    }
    match a.currency {
        Some(c) => {
            out.push(1);
            out.extend_from_slice(&c.code());
        }
        None => out.push(0),
    }
    for v in [a.tender_minor, a.sale_minor, a.at, a.secs, a.raised_at] {
        out.extend_from_slice(&v.to_le_bytes());
    }
}

fn get_alert(
    r: &mut Reader<'_>,
    orders: &BTreeMap<OrderId, PosOrder>,
) -> Result<Alert, DecodeError> {
    let kind = AlertKind::from_u8(r.u8()?)
        .ok_or(DecodeError::NonCanonical("an alert kind is not known"))?;
    let subject = match r.u8()? {
        1 => {
            let key = TenderKey {
                order: get_id(r)?,
                tender: get_id(r)?,
            };
            if !orders.contains_key(&key.order) {
                return Err(DecodeError::NonCanonical(
                    "an alert names an untracked order",
                ));
            }
            Subject::Tender(key)
        }
        2 => Subject::Sale(get_sale(r)?),
        _ => return Err(DecodeError::NonCanonical("an alert subject is not known")),
    };
    if matches!(subject, Subject::Sale(_)) != (kind == AlertKind::PaymentWithoutTender) {
        return Err(DecodeError::NonCanonical(
            "an alert kind and subject disagree",
        ));
    }
    let sale = if r.flag()? { Some(get_sale(r)?) } else { None };
    let currency = if r.flag()? {
        Some(
            Currency::new(r.array::<3>()?)
                .ok_or(DecodeError::NonCanonical("a currency code is not valid"))?,
        )
    } else {
        None
    };
    Ok(Alert {
        key: AlertKey { kind, subject },
        sale,
        currency,
        tender_minor: r.u64_le()?,
        sale_minor: r.u64_le()?,
        at: r.u64_le()?,
        secs: r.u64_le()?,
        raised_at: r.u64_le()?,
    })
}
