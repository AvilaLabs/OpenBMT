//! The matching of Bitcoin tenders to counter sales and the alert rules
//! (PS-30 to PS-37, PS-40 to PS-48), as one pure function.
//!
//! Given the orders the ledger tracks, the matches it already holds, the
//! counter's sales and the time, [`evaluate`] returns every match and every
//! alert that holds now. It does not remember anything and does not look at
//! the order its input came in: every collection is ordered, and every tie is
//! broken by time and then by id (PS-30). What to keep, raise and clear is the
//! ledger's business.

use std::collections::{BTreeMap, BTreeSet};

use openbmt_counter::Currency;

use crate::alerts::{Alert, AlertKey, AlertKind, Subject, TenderKey};
use crate::facts::{SaleFact, SaleKey};
use crate::model::{OrderId, OrderState, PosOrder, PosTender, TenderState};
use crate::reference::{find_reference, RefCode};
use crate::settings::Settings;

/// How a tender was matched to its sale.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchHow {
    /// PS-31: by the sale reference the cashier typed.
    Reference,
    /// PS-33: by amount and time.
    AmountAndTime,
}

/// A tender matched to a sale.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Match {
    /// The sale.
    pub sale: SaleKey,
    /// How it was matched.
    pub how: MatchHow,
    /// PS-34: more than one tender or sale was eligible, so the pairing is
    /// by order of time and not exact.
    pub by_order: bool,
    /// When the pair was first made.
    pub matched_at: u64,
}

/// What [`evaluate`] reads.
pub struct Input<'a> {
    /// The shop's settings.
    pub settings: &'a Settings,
    /// The tracked orders.
    pub orders: &'a BTreeMap<OrderId, PosOrder>,
    /// The matches held so far (PS-36).
    pub prior: &'a BTreeMap<TenderKey, Match>,
    /// The counter's sales.
    pub facts: &'a BTreeMap<SaleKey, SaleFact>,
    /// Now, in seconds.
    pub now: u64,
}

/// What [`evaluate`] returns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Output {
    /// Every match that holds now.
    pub matches: BTreeMap<TenderKey, Match>,
    /// Every alert whose condition holds now, with `raised_at` set to now.
    pub alerts: BTreeMap<AlertKey, Alert>,
}

/// A Bitcoin tender that may be matched (PS-12).
struct Candidate<'a> {
    key: TenderKey,
    order: &'a PosOrder,
    tender: &'a PosTender,
}

impl Candidate<'_> {
    fn sort_key(&self) -> (u64, &TenderKey) {
        (self.tender.at, &self.key)
    }

    fn fits(&self, sale: &SaleFact) -> bool {
        sale.amount.0 == self.tender.amount_minor && sale.currency == self.order.currency
    }
}

struct Alerts {
    now: u64,
    map: BTreeMap<AlertKey, Alert>,
}

impl Alerts {
    fn put(&mut self, kind: AlertKind, subject: Subject, f: impl FnOnce(&mut Alert)) {
        let key = AlertKey { kind, subject };
        let mut alert = Alert {
            key: key.clone(),
            sale: None,
            currency: None,
            tender_minor: 0,
            sale_minor: 0,
            at: 0,
            secs: 0,
            raised_at: self.now,
        };
        f(&mut alert);
        self.map.insert(key, alert);
    }
}

/// The latest a tender may be and still match the sale (PS-33).
fn latest_tender_time(sale: &SaleFact, s: &Settings) -> u64 {
    sale.paid_at
        .unwrap_or_else(|| sale.opened_at.saturating_add(s.timing.never_paid_span_secs))
        .saturating_add(s.timing.match_window_secs)
}

/// Work out every match and every alert that holds now.
pub fn evaluate(input: &Input<'_>) -> Output {
    let Input {
        settings,
        orders,
        prior,
        facts,
        now,
    } = *input;
    let timing = settings.timing;
    let mut alerts = Alerts {
        now,
        map: BTreeMap::new(),
    };

    // PS-12, PS-13, PS-35: the candidates, and the alerts about them that do
    // not depend on a sale.
    let mut cands: BTreeMap<TenderKey, Candidate<'_>> = BTreeMap::new();
    for order in orders.values() {
        if order.state == OrderState::Voided {
            continue;
        }
        for tender in &order.tenders {
            if tender.state != TenderState::Paid || !settings.is_bitcoin(tender) {
                continue;
            }
            let key = TenderKey {
                order: order.id.clone(),
                tender: tender.id.clone(),
            };
            let subject = Subject::Tender(key.clone());
            if tender.tip_minor > 0 {
                alerts.put(AlertKind::BitcoinTip, subject.clone(), |a| {
                    a.currency = Some(order.currency);
                    a.tender_minor = tender.tip_minor;
                    a.at = tender.at;
                });
            }
            if !settings.currencies.contains(&order.currency) {
                alerts.put(AlertKind::UnknownCurrency, subject, |a| {
                    a.currency = Some(order.currency);
                    a.tender_minor = tender.amount_minor;
                    a.at = tender.at;
                });
                continue;
            }
            cands.insert(key.clone(), Candidate { key, order, tender });
        }
    }

    // PS-36, PS-38: what stays, what is worked out again, and what is freed.
    let mut matches: BTreeMap<TenderKey, Match> = BTreeMap::new();
    let mut freed: Vec<(TenderKey, Match)> = Vec::new();
    let mut taken: BTreeSet<SaleKey> = BTreeSet::new();
    for (key, m) in prior {
        let order = orders.get(&key.order);
        let tender = order.and_then(|o| o.tender(&key.tender));
        let voided = order.is_none_or(|o| o.state == OrderState::Voided)
            || tender.is_none_or(|t| t.state == TenderState::Voided);
        if voided && facts.contains_key(&m.sale) {
            freed.push((key.clone(), m.clone()));
            continue;
        }
        let again = m.how == MatchHow::AmountAndTime
            && cands.contains_key(key)
            && facts.contains_key(&m.sale);
        if !again {
            matches.insert(key.clone(), m.clone());
            taken.insert(m.sale);
        }
    }
    let mut free: Vec<&Candidate<'_>> = cands
        .values()
        .filter(|c| !matches.contains_key(&c.key))
        .collect();
    free.sort_by(|a, b| a.sort_key().cmp(&b.sort_key()));

    // PS-31, PS-32: pass 1, by reference.
    let mut by_code: BTreeMap<RefCode, Vec<&SaleFact>> = BTreeMap::new();
    for sale in facts.values() {
        if let Some(code) = sale.reference {
            by_code.entry(code).or_default().push(sale);
        }
    }
    let mut consumed: BTreeSet<TenderKey> = BTreeSet::new();
    let mut claimed: BTreeSet<SaleKey> = BTreeSet::new();
    let mut fresh: BTreeMap<TenderKey, (SaleKey, MatchHow, bool)> = BTreeMap::new();
    for c in &free {
        let counters = settings.counters_at(&c.order.location);
        let notes = [c.tender.reference.as_ref(), c.order.reference.as_ref()];
        for code in notes
            .into_iter()
            .flatten()
            .filter_map(|t| find_reference(t.as_str()))
        {
            let named: Vec<&SaleFact> = by_code
                .get(&code)
                .map(|v| v.iter().filter(|s| counters.contains(&s.counter)).copied())
                .into_iter()
                .flatten()
                .collect();
            let Some(first) = named.first() else {
                continue;
            };
            consumed.insert(c.key.clone());
            let subject = Subject::Tender(c.key.clone());
            let equal: Vec<&&SaleFact> = named.iter().filter(|s| c.fits(s)).collect();
            if let Some(s) = equal.iter().find(|s| !taken.contains(&s.key())) {
                taken.insert(s.key());
                fresh.insert(c.key.clone(), (s.key(), MatchHow::Reference, false));
            } else if let Some(s) = equal.first() {
                let sale = s.key();
                alerts.put(AlertKind::DuplicateReference, subject, |a| {
                    a.sale = Some(sale);
                    a.currency = Some(c.order.currency);
                    a.tender_minor = c.tender.amount_minor;
                    a.sale_minor = s.amount.0;
                    a.at = c.tender.at;
                });
            } else {
                claimed.insert(first.key());
                alerts.put(AlertKind::AmountDiffers, subject, |a| {
                    a.sale = Some(first.key());
                    a.currency = Some(c.order.currency);
                    a.tender_minor = c.tender.amount_minor;
                    a.sale_minor = first.amount.0;
                    a.at = c.tender.at;
                });
            }
            break;
        }
    }

    // PS-33, PS-34: pass 2, by amount and time.
    let mut by_amount: BTreeMap<(u64, Currency), Vec<&SaleFact>> = BTreeMap::new();
    for sale in facts.values() {
        let key = sale.key();
        if !taken.contains(&key) && !claimed.contains(&key) {
            by_amount
                .entry((sale.amount.0, sale.currency))
                .or_default()
                .push(sale);
        }
    }
    for sales in by_amount.values_mut() {
        sales.sort_by_key(|s| (s.opened_at, s.key()));
    }
    let pool: Vec<&&Candidate<'_>> = free
        .iter()
        .filter(|c| !consumed.contains(&c.key) && !fresh.contains_key(&c.key))
        .collect();
    let eligible = |c: &Candidate<'_>, s: &SaleFact| {
        settings.counters_at(&c.order.location).contains(&s.counter)
            && c.tender.at.saturating_add(timing.clock_slack_secs) >= s.opened_at
            && c.tender.at <= latest_tender_time(s, settings)
    };
    let sales_for = |c: &Candidate<'_>| -> Vec<&SaleFact> {
        by_amount
            .get(&(c.tender.amount_minor, c.order.currency))
            .map(|v| v.iter().copied().filter(|s| eligible(c, s)).collect())
            .unwrap_or_default()
    };
    let mut tender_count: BTreeMap<&TenderKey, usize> = BTreeMap::new();
    let mut sale_count: BTreeMap<SaleKey, usize> = BTreeMap::new();
    for c in &pool {
        let list = sales_for(c);
        tender_count.insert(&c.key, list.len());
        for s in list {
            let n = sale_count.entry(s.key()).or_insert(0);
            *n = n.saturating_add(1);
        }
    }
    for c in &pool {
        let pick = sales_for(c).into_iter().find(|s| !taken.contains(&s.key()));
        if let Some(s) = pick {
            let ambiguous = tender_count.get(&c.key).copied().unwrap_or(0) > 1
                || sale_count.get(&s.key()).copied().unwrap_or(0) > 1;
            taken.insert(s.key());
            fresh.insert(c.key.clone(), (s.key(), MatchHow::AmountAndTime, ambiguous));
        }
    }
    for (key, (sale, how, by_order)) in fresh {
        let matched_at = prior
            .get(&key)
            .filter(|p| p.sale == sale)
            .map_or(now, |p| p.matched_at);
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

    // PS-38: a sale whose tender was voided goes to the tender that rings it
    // again, and otherwise stays with the voided one.
    for (key, m) in freed {
        if !matches.values().any(|n| n.sale == m.sale) {
            matches.insert(key, m);
        }
    }

    // PS-41, PS-44, PS-45: alerts about matches.
    for (key, m) in &matches {
        let Some(order) = orders.get(&key.order) else {
            continue;
        };
        let Some(fact) = facts.get(&m.sale) else {
            continue;
        };
        let tender = order.tender(&key.tender);
        let order_voided = order.state == OrderState::Voided;
        let live = !order_voided && tender.is_some_and(|t| t.state == TenderState::Paid);
        let subject = Subject::Tender(key.clone());
        let at = tender.map_or(order.updated_at, |t| t.at);
        let fill = |a: &mut Alert| {
            a.sale = Some(m.sale);
            a.currency = Some(order.currency);
            a.tender_minor = tender.map_or(0, |t| t.amount_minor);
            a.sale_minor = fact.amount.0;
            a.at = at;
        };
        if !live {
            // PS-45: a refund is reported at once; a void, or a tender gone
            // from the order, only once it is older than the grace time
            // without another tender taking the sale.
            let refund_only = !order_voided
                && tender.is_some_and(|t| matches!(t.state, TenderState::Refunded { .. }));
            let old_enough = now >= order.updated_at.saturating_add(timing.grace_secs);
            if fact.is_paid_now() && (refund_only || old_enough) {
                alerts.put(AlertKind::VoidedAfterPayment, subject, fill);
            }
            continue;
        }
        if let Some(paid) = fact.paid_at {
            let early = paid.saturating_sub(at);
            if early > timing.closed_before_paid_secs {
                alerts.put(AlertKind::ClosedBeforePaid, subject.clone(), |a| {
                    fill(a);
                    a.secs = early;
                });
            }
        }
        if fact.unpaid_after
            || (fact.paid_at.is_none() && now >= at.saturating_add(timing.grace_secs))
        {
            alerts.put(AlertKind::TenderOnUnpaidSale, subject, fill);
        }
    }

    // PS-40: tenders with no sale.
    for c in cands.values() {
        let unmatched = !matches.contains_key(&c.key) && !consumed.contains(&c.key);
        if unmatched && now >= c.tender.at.saturating_add(timing.grace_secs) {
            alerts.put(
                AlertKind::TenderWithoutPayment,
                Subject::Tender(c.key.clone()),
                |a| {
                    a.currency = Some(c.order.currency);
                    a.tender_minor = c.tender.amount_minor;
                    a.at = c.tender.at;
                },
            );
        }
    }

    // PS-42, PS-37: paid sales with no tender.
    let matched_sales: BTreeSet<SaleKey> = matches.values().map(|m| m.sale).collect();
    for sale in facts.values() {
        let key = sale.key();
        let Some(paid) = sale.paid_at else { continue };
        let waiting = now >= paid.saturating_add(timing.grace_secs);
        if sale.is_paid_now()
            && waiting
            && !matched_sales.contains(&key)
            && !claimed.contains(&key)
            && !settings.is_no_pos(&sale.counter)
        {
            alerts.put(AlertKind::PaymentWithoutTender, Subject::Sale(key), |a| {
                a.sale = Some(key);
                a.currency = Some(sale.currency);
                a.sale_minor = sale.amount.0;
                a.at = paid;
            });
        }
    }

    Output {
        matches,
        alerts: alerts.map,
    }
}
