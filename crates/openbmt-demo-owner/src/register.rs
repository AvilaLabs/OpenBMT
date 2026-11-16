//! The shop's own register, read by the owner's phone: one order of the
//! register and the counter's sale go through the matcher of `openbmt-pos`
//! (PS-30 to PS-48), and what comes out is what the till screen shows. The
//! page plays the register and gives the order; the sale is the counter's,
//! read by the page from the counter's own state.

use openbmt_core::identity::{DeviceId, Identity};
use openbmt_counter::{Amount, Currency};
use openbmt_pos::{
    Id, Ledger, LocationCounters, OrderState, PosOrder, PosTender, SaleFact, Settings, TenderKind,
    TenderState, Text,
};

/// The location the demo register names.
const LOCATION: &str = "Register";
/// The tender the demo shop's register uses for bitcoin.
const TENDER_NAME: &str = "Bitcoin";

/// The register's one order, as the owner's phone reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Order {
    /// The order's total, in cents of a dollar.
    pub total_minor: u64,
    /// When the order was opened.
    pub opened_at: u64,
    /// When the cashier closed it with the Bitcoin tender; `None` while it is open.
    pub closed_at: Option<u64>,
}

/// The counter's sale, as the page read it from the counter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sale {
    /// The sale's receive index.
    pub index: u32,
    /// The sale's amount, in cents of a dollar.
    pub amount_minor: u64,
    /// When the sale opened.
    pub opened_at: u64,
    /// When the counter first showed it paid.
    pub paid_at: Option<u64>,
}

/// What the matcher says of the order and the sale.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Verdict {
    /// Sales matched to a Bitcoin tender.
    pub matches: u32,
    /// Of those, matched by order of time and not exactly.
    pub by_order: u32,
    /// The headings of the open alerts.
    pub alerts: Vec<String>,
}

fn counter() -> Result<DeviceId, String> {
    Identity::from_secret_bytes(&[3; 32])
        .map(|i| i.device_id())
        .map_err(|e| e.to_string())
}

fn text(s: &str) -> Result<Text, String> {
    Text::new(s).map_err(|e| e.to_string())
}

/// Reads `order` and `sale` at `now` as the owner's phone reads a till and the
/// counters' sales: the matches and the open alerts.
pub fn verdict(order: &Order, sale: Option<&Sale>, now: u64) -> Result<Verdict, String> {
    let counter = counter()?;
    let usd = Currency::new(*b"USD").ok_or("the currency is not valid")?;
    let settings = Settings::new(
        vec![text(TENDER_NAME)?],
        vec![LocationCounters {
            location: text(LOCATION)?,
            counters: vec![counter],
        }],
        vec![usd],
    )
    .map_err(|e| e.to_string())?;
    let tenders = match order.closed_at {
        Some(at) => vec![PosTender {
            id: Id::new("1").map_err(|e| e.to_string())?,
            kind: TenderKind::Other,
            name: text(TENDER_NAME)?,
            amount_minor: order.total_minor,
            tip_minor: 0,
            state: TenderState::Paid,
            at,
            reference: None,
        }],
        None => Vec::new(),
    };
    let posted = PosOrder {
        id: Id::new("1042").map_err(|e| e.to_string())?,
        location: text(LOCATION)?,
        state: if order.closed_at.is_some() {
            OrderState::Closed
        } else {
            OrderState::Open
        },
        currency: usd,
        total_minor: order.total_minor,
        tenders,
        opened_at: order.opened_at,
        closed_at: order.closed_at,
        updated_at: order.closed_at.unwrap_or(order.opened_at),
        reference: None,
    };
    let facts: Vec<SaleFact> = sale
        .map(|s| {
            let mut fact =
                SaleFact::opened(counter, s.index, Amount(s.amount_minor), usd, s.opened_at);
            fact.paid_at = s.paid_at;
            fact
        })
        .into_iter()
        .collect();
    let (ledger, _) = Ledger::new()
        .updated(&[posted], Default::default(), &facts, &settings, now)
        .map_err(|e| e.to_string())?;
    let summary = ledger.summary();
    Ok(Verdict {
        matches: u32::try_from(summary.matches).unwrap_or(u32::MAX),
        by_order: u32::try_from(summary.matches_by_order).unwrap_or(u32::MAX),
        alerts: ledger
            .open_alerts()
            .iter()
            .map(|a| a.words().heading)
            .collect(),
    })
}

/// The page's request as text of numbers: `total opened closed index amount
/// sale_opened sale_paid now`, where `closed`, `index` and `sale_paid` are 0
/// for none and `index` is the sale's index plus 1. The verdict as text:
/// `matches=N;by_order=N;alerts=N`, then `;alert=heading` for each alert.
pub fn verdict_of_text(request: &str) -> Result<String, String> {
    let n: Vec<u64> = request
        .split_whitespace()
        .map(|t| {
            t.parse::<u64>()
                .map_err(|_| format!("`{t}` is not a number"))
        })
        .collect::<Result<_, _>>()?;
    let [total, opened, closed, index, amount, sale_opened, sale_paid, now] = n[..] else {
        return Err("eight numbers are needed".to_string());
    };
    let order = Order {
        total_minor: total,
        opened_at: opened,
        closed_at: (closed != 0).then_some(closed),
    };
    let sale = (index != 0).then(|| Sale {
        index: u32::try_from(index - 1).unwrap_or(u32::MAX),
        amount_minor: amount,
        opened_at: sale_opened,
        paid_at: (sale_paid != 0).then_some(sale_paid),
    });
    let v = verdict(&order, sale.as_ref(), now)?;
    let mut out = format!(
        "matches={};by_order={};alerts={}",
        v.matches,
        v.by_order,
        v.alerts.len()
    );
    for heading in &v.alerts {
        out.push_str(";alert=");
        out.push_str(heading);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const T0: u64 = 1_700_000_000;

    fn order(closed: Option<u64>) -> Order {
        Order {
            total_minor: 800,
            opened_at: T0,
            closed_at: closed,
        }
    }

    fn sale(paid: Option<u64>) -> Sale {
        Sale {
            index: 0,
            amount_minor: 800,
            opened_at: T0 + 20,
            paid_at: paid,
        }
    }

    /// PS-31 to PS-33: a Bitcoin tender of 8.00 USD closed after a counter sale of 8.00 USD
    /// was paid matches it, and nothing is raised.
    #[test]
    fn a_tender_closed_after_the_payment_matches_the_sale() {
        let v =
            verdict(&order(Some(T0 + 60)), Some(&sale(Some(T0 + 40))), T0 + 90).expect("a verdict");
        assert_eq!(
            v,
            Verdict {
                matches: 1,
                by_order: 0,
                alerts: Vec::new()
            }
        );
    }

    /// PS-40: a Bitcoin tender with no counter sale is an alert once the grace has passed.
    #[test]
    fn a_tender_with_no_sale_is_an_alert() {
        let late = T0 + 60 + openbmt_pos::GRACE_SECS + 1;
        let v = verdict(&order(Some(T0 + 60)), None, late).expect("a verdict");
        assert_eq!(v.matches, 0);
        assert_eq!(v.alerts.len(), 1, "{v:?}");
    }

    /// An order that is still open has no tender, so nothing matches and nothing is raised.
    #[test]
    fn an_open_order_matches_nothing_and_raises_nothing() {
        let v = verdict(&order(None), Some(&sale(Some(T0 + 40))), T0 + 90).expect("a verdict");
        assert_eq!((v.matches, v.alerts.len()), (0, 0));
    }

    #[test]
    fn the_text_of_a_request_gives_the_text_of_a_verdict() {
        let line = format!(
            "800 {T0} {} 1 800 {} {} {}",
            T0 + 60,
            T0 + 20,
            T0 + 40,
            T0 + 90
        );
        assert_eq!(
            verdict_of_text(&line).as_deref(),
            Ok("matches=1;by_order=0;alerts=0")
        );
        assert!(verdict_of_text("800 1").is_err());
    }
}
