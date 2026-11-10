//! The counter's sales as the matcher needs them (PS-05 to PS-07), folded
//! from the counter's signed events.

use std::collections::BTreeMap;
use std::fmt;

use openbmt_core::identity::DeviceId;
use openbmt_core::records::{Chain, ChainError, DeviceList, RecordKind};
use openbmt_core::wire::DecodeError;
use openbmt_counter::{Amount, Currency, Event};

use crate::reference::RefCode;
use crate::MAX_SALES;

/// Which sale: the counter's key and the sale index.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SaleKey {
    /// The counter.
    pub counter: DeviceId,
    /// The sale index.
    pub index: u32,
}

/// What the matcher needs of a counter sale (PS-05).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaleFact {
    /// The counter that rang the sale.
    pub counter: DeviceId,
    /// The sale index.
    pub index: u32,
    /// The amount, in minor units of the currency.
    pub amount: Amount,
    /// The currency.
    pub currency: Currency,
    /// When the sale opened.
    pub opened_at: u64,
    /// When the sale was first reported paid, if ever.
    pub paid_at: Option<u64>,
    /// It was reported unpaid after that and has not been reported paid
    /// again.
    pub unpaid_after: bool,
    /// The sale is final (CR-31).
    pub is_final: bool,
    /// The sale's reference (PS-20).
    pub reference: Option<RefCode>,
}

impl SaleFact {
    /// A sale that is open and unpaid, with its reference.
    pub fn opened(
        counter: DeviceId,
        index: u32,
        amount: Amount,
        currency: Currency,
        opened_at: u64,
    ) -> Self {
        Self {
            counter,
            index,
            amount,
            currency,
            opened_at,
            paid_at: None,
            unpaid_after: false,
            is_final: false,
            reference: Some(RefCode::for_sale(&counter, index)),
        }
    }

    /// Which sale this is.
    pub fn key(&self) -> SaleKey {
        SaleKey {
            counter: self.counter,
            index: self.index,
        }
    }

    /// The sale is reported paid now: it was, and was not reported unpaid
    /// since.
    pub fn is_paid_now(&self) -> bool {
        self.paid_at.is_some() && !self.unpaid_after
    }
}

/// Why the facts could not be built.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FactsError {
    /// PS-05: the chain does not verify.
    Chain(ChainError),
    /// PS-06: the body of a record does not decode.
    BadEvent {
        /// The record.
        seq: u64,
        /// Why.
        error: DecodeError,
    },
    /// PS-06: an event is for a sale that was never opened.
    UnknownSale {
        /// The sale index.
        index: u32,
    },
    /// PS-06: a sale was opened twice.
    OpenedTwice {
        /// The sale index.
        index: u32,
    },
    /// PS-72: more sales than the matcher takes.
    TooManySales,
}

impl fmt::Display for FactsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Chain(e) => write!(f, "PS-05: the counter's records do not verify: {e}"),
            Self::BadEvent { seq, error } => {
                write!(f, "PS-06: record {seq} is not a counter event: {error}")
            }
            Self::UnknownSale { index } => {
                write!(
                    f,
                    "PS-06: an event is for sale {index}, which was never opened"
                )
            }
            Self::OpenedTwice { index } => write!(f, "PS-06: sale {index} was opened twice"),
            Self::TooManySales => write!(f, "PS-72: more sales than the matcher takes"),
        }
    }
}

impl std::error::Error for FactsError {}

/// A sale being folded.
#[derive(Debug, Clone)]
struct Folding {
    /// The fact as inferred from the older events (PS-06).
    fact: SaleFact,
    confirmed_now: bool,
    /// The counter wrote `PosPaid` or `PosUnpaid` for this sale (PS-09).
    explicit: bool,
    explicit_paid_at: Option<u64>,
    explicit_unpaid: bool,
}

impl Folding {
    /// The fact as it stands after the events folded so far (PS-09 over
    /// PS-06).
    fn current(&self) -> SaleFact {
        let mut fact = self.fact.clone();
        if self.explicit {
            fact.paid_at = self.explicit_paid_at.or(fact.paid_at);
            fact.unpaid_after = self.explicit_unpaid;
        }
        fact
    }
}

/// The counter's events folded into sale facts one at a time (PS-06,
/// PS-09). [`facts_from_chain`] runs one over a verified chain; the counter
/// node runs one to know when a sale changes state (BX-40).
#[derive(Debug, Clone)]
pub struct FactFold {
    counter: DeviceId,
    sales: BTreeMap<u32, Folding>,
}

impl FactFold {
    /// An empty fold for this counter's events.
    pub fn new(counter: DeviceId) -> Self {
        Self {
            counter,
            sales: BTreeMap::new(),
        }
    }

    /// Fold one event. A refused event changes nothing.
    pub fn apply(&mut self, event: &Event) -> Result<(), FactsError> {
        if self.sales.len() >= MAX_SALES && !self.sales.contains_key(&event.index()) {
            return Err(FactsError::TooManySales);
        }
        fold_event(&mut self.sales, &self.counter, event)
    }

    /// The fact of one sale as it stands now.
    pub fn fact(&self, index: u32) -> Option<SaleFact> {
        self.sales.get(&index).map(Folding::current)
    }

    /// Every fact, ordered by sale index.
    pub fn facts(&self) -> Vec<SaleFact> {
        self.sales.values().map(Folding::current).collect()
    }
}

/// PS-05, PS-06, PS-09: fold a counter's record chain into one fact per sale,
/// ordered by sale index. The chain is verified whole first against
/// `devices`. Only the records the counter itself wrote are read, and of
/// those only the kinds that hold sale events. Nothing is guessed.
pub fn facts_from_chain(
    chain: &Chain,
    devices: &DeviceList,
    counter: &DeviceId,
) -> Result<Vec<SaleFact>, FactsError> {
    chain.verify(devices).map_err(FactsError::Chain)?;
    let mut fold = FactFold::new(*counter);
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
        fold.apply(&event)?;
    }
    Ok(fold.facts())
}

fn fold_event(
    sales: &mut BTreeMap<u32, Folding>,
    counter: &DeviceId,
    event: &Event,
) -> Result<(), FactsError> {
    let index = event.index();
    if let Event::Opened {
        at,
        amount,
        currency,
        ..
    } = event
    {
        if sales.contains_key(&index) {
            return Err(FactsError::OpenedTwice { index });
        }
        sales.insert(
            index,
            Folding {
                fact: SaleFact::opened(*counter, index, *amount, *currency, *at),
                confirmed_now: false,
                explicit: false,
                explicit_paid_at: None,
                explicit_unpaid: false,
            },
        );
        return Ok(());
    }
    let s = sales
        .get_mut(&index)
        .ok_or(FactsError::UnknownSale { index })?;
    let paid = |s: &mut Folding, at: u64| {
        s.fact.paid_at.get_or_insert(at);
        s.fact.unpaid_after = false;
    };
    match event {
        Event::PaidAtOnce { at, .. } | Event::Override { at, .. } => paid(s, *at),
        Event::Confirmed { at, .. } => {
            paid(s, *at);
            s.confirmed_now = true;
        }
        Event::Final { at, .. } => {
            paid(s, *at);
            s.confirmed_now = true;
            s.fact.is_final = true;
        }
        Event::Reopened { .. } => {
            if s.fact.paid_at.is_some() {
                s.fact.unpaid_after = true;
            }
            s.confirmed_now = false;
            s.fact.is_final = false;
        }
        Event::Replaced { .. } => {
            if s.fact.paid_at.is_some() && !s.confirmed_now {
                s.fact.unpaid_after = true;
            }
        }
        Event::PosPaid { at, .. } => {
            s.explicit = true;
            s.explicit_paid_at.get_or_insert(*at);
            s.explicit_unpaid = false;
        }
        Event::PosUnpaid { .. } => {
            s.explicit = true;
            s.explicit_unpaid = true;
        }
        Event::Opened { .. }
        | Event::Requoted { .. }
        | Event::Expired { .. }
        | Event::Cancelled { .. }
        | Event::LatePayment { .. }
        | Event::SurplusOwed { .. }
        | Event::Dust { .. }
        | Event::RefundRequested { .. }
        | Event::RefundWithdrawn { .. }
        | Event::RefundPaidInCash { .. } => {}
    }
    Ok(())
}
