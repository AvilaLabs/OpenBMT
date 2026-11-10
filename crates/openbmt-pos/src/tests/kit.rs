//! Builders for the tests: counters, sales, orders, settings and a small
//! generator with a fixed seed (no dependency).

use std::convert::Infallible;

use openbmt_core::identity::{DeviceId, Identity};
use openbmt_counter::{Amount, Currency};

use crate::*;

/// The first moment.
pub const T0: u64 = 1_000_000;

/// A numbered counter identity.
pub fn identity(n: u8) -> Identity {
    Identity::from_secret_bytes(&[n; 32]).unwrap()
}

/// A numbered counter's key.
pub fn counter(n: u8) -> DeviceId {
    identity(n).device_id()
}

pub fn usd() -> Currency {
    Currency::new(*b"USD").unwrap()
}

pub fn eur() -> Currency {
    Currency::new(*b"EUR").unwrap()
}

pub fn text(s: &str) -> Text {
    Text::new(s).unwrap()
}

pub fn oid(s: &str) -> Id {
    Id::new(s).unwrap()
}

/// Counter 3 serves "Main", counter 4 serves "Back", counter 9 has no POS.
pub fn settings() -> Settings {
    let mut s = Settings::new(
        vec![text("Bitcoin")],
        vec![
            LocationCounters {
                location: text("Main"),
                counters: vec![counter(3)],
            },
            LocationCounters {
                location: text("Back"),
                counters: vec![counter(4)],
            },
        ],
        vec![usd()],
    )
    .unwrap();
    s.no_pos = vec![counter(9)];
    s
}

/// An open sale of counter `c`.
pub fn sale(c: u8, index: u32, cents: u64, opened_at: u64) -> SaleFact {
    SaleFact::opened(counter(c), index, Amount(cents), usd(), opened_at)
}

/// The same sale, first reported paid at `at`.
pub fn paid(mut s: SaleFact, at: u64) -> SaleFact {
    s.paid_at = Some(at);
    s
}

/// A paid Bitcoin tender named "Bitcoin".
pub fn tender(id: &str, cents: u64, at: u64) -> PosTender {
    PosTender {
        id: oid(id),
        kind: TenderKind::Other,
        name: text("Bitcoin"),
        amount_minor: cents,
        tip_minor: 0,
        state: TenderState::Paid,
        at,
        reference: None,
    }
}

pub fn cash(id: &str, cents: u64, at: u64) -> PosTender {
    PosTender {
        kind: TenderKind::Cash,
        name: text("Cash"),
        ..tender(id, cents, at)
    }
}

/// A closed order at `loc` with these tenders, last changed at the latest
/// tender's time.
pub fn order(id: &str, loc: &str, tenders: Vec<PosTender>) -> PosOrder {
    let updated = tenders.iter().map(|t| t.at).max().unwrap_or(T0);
    PosOrder {
        id: oid(id),
        location: text(loc),
        state: OrderState::Closed,
        currency: usd(),
        total_minor: tenders.iter().map(|t| t.amount_minor).sum(),
        tenders,
        opened_at: T0,
        closed_at: Some(updated),
        updated_at: updated,
        reference: None,
    }
}

/// A closed order of one Bitcoin tender `t-<id>` of `cents` at `at`.
pub fn simple(id: &str, loc: &str, cents: u64, at: u64) -> PosOrder {
    order(id, loc, vec![tender(&format!("t-{id}"), cents, at)])
}

pub fn ok<T: ?Sized>(_: &T) -> Result<(), Infallible> {
    Ok(())
}

/// One poll with everything, stored in nothing.
pub fn step(ledger: &mut Ledger, orders: &[PosOrder], facts: &[SaleFact], now: u64) -> Delta {
    ledger
        .update_stored(
            orders,
            ledger.cursor().clone(),
            facts,
            &settings(),
            now,
            |_| Ok::<(), Infallible>(()),
        )
        .unwrap()
}

/// The kinds of the open alerts, sorted.
pub fn kinds(ledger: &Ledger) -> Vec<AlertKind> {
    let mut v: Vec<AlertKind> = ledger.open_alerts().iter().map(|a| a.key.kind).collect();
    v.sort();
    v
}

pub fn tkey(order: &str, tender: &str) -> TenderKey {
    TenderKey {
        order: oid(order),
        tender: oid(tender),
    }
}

/// A fixed-seed generator (xorshift64*).
pub struct Rng(pub u64);

impl Rng {
    pub fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }

    /// Shuffle in place (Fisher-Yates).
    pub fn shuffle<T>(&mut self, v: &mut [T]) {
        for i in (1..v.len()).rev() {
            let j = usize::try_from(self.below(u64::try_from(i + 1).unwrap())).unwrap();
            v.swap(i, j);
        }
    }
}
