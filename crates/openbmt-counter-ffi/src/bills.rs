//! Bills, tabs and the till's reference through the bridge (CR-245 to
//! CR-254): the arguments and views of ops 73 to 84, and the calls of the
//! entry behind them. The words are the placeholders of the library until
//! they are written (CR-255).

use openbmt_counter::{
    Amount, Bill, BillEnd, BillKind, BillPart, BillRefused, BillView, PartRefused, PartWay,
    PosState, SaleExtras,
};
use openbmt_counter_node::Node;

use crate::codec::{wire_record, wire_record_get, wire_unit_enum};
use crate::error::{from_rule, CounterError};
use crate::host::{actor, rate_and_source, Entry, Port};
use crate::pay::payment_for;
use crate::views::{ActorView, OpenedView, WordsView};

/// A bill or a tab (CR-246).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BillKindView {
    /// A bill: settled when its parts reach its amount.
    Bill,
    /// A tab: settled when the cashier says it is finished.
    Tab,
}

wire_unit_enum!(BillKindView { Bill = 1, Tab = 2 });

/// How a part that is not bitcoin was paid (CR-247).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PartWayView {
    /// In cash.
    Cash,
    /// By card.
    Card,
    /// Another way.
    Other,
}

wire_unit_enum!(PartWayView { Cash = 1, Card = 2, Other = 3 });

/// Where a bill stands (CR-246, CR-252).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BillStateView {
    /// Open.
    Open,
    /// Settled.
    Settled,
    /// Closed short by a manager.
    ClosedShort,
    /// Cancelled.
    Cancelled,
}

wire_unit_enum!(BillStateView {
    Open = 0,
    Settled = 1,
    ClosedShort = 2,
    Cancelled = 3
});

/// The arguments of `open_bill` (CR-246).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenBillArgs {
    /// The time now.
    pub now: u64,
    /// A bill or a tab.
    pub kind: BillKindView,
    /// The amount in minor units of the shop's currency.
    pub amount_minor: u64,
    /// The till's reference, when the cashier typed one (CR-245).
    pub reference: Option<String>,
}

wire_record!(OpenBillArgs {
    now,
    kind,
    amount_minor,
    reference
});
wire_record_get!(OpenBillArgs {
    now,
    kind,
    amount_minor,
    reference
});

/// The arguments of `add_reference` (a sale) and `add_bill_reference` (a
/// bill): the time, the sale index or the bill number, and the reference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReferenceArgs {
    /// The time now.
    pub now: u64,
    /// The sale's receive index, or the bill's number.
    pub id: u32,
    /// The till's reference (CR-245).
    pub reference: String,
}

wire_record!(ReferenceArgs { now, id, reference });
wire_record_get!(ReferenceArgs { now, id, reference });

/// The arguments of `join_sale` (CR-248).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JoinSaleArgs {
    /// The time now.
    pub now: u64,
    /// The bill.
    pub number: u32,
    /// The sale, open or paid, that becomes a part of it.
    pub sale: u32,
}

wire_record!(JoinSaleArgs { now, number, sale });
wire_record_get!(JoinSaleArgs { now, number, sale });

/// The arguments of `open_bill_part` (CR-248): those of `open_with_tip` for
/// the bill's currency, with the bill's number, a reference and a note.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BillPartArgs {
    /// The time now.
    pub now: u64,
    /// The bill.
    pub number: u32,
    /// What the customer pays of the bill, in minor units.
    pub amount_minor: u64,
    /// A tip on top of it, in minor units (CR-189).
    pub tip_minor: u64,
    /// The rate, minor units per bitcoin.
    pub rate: u64,
    /// Where the rate came from.
    pub source_label: String,
    /// When the rate was fetched.
    pub source_fetched_at: u64,
    /// The person at the till, when one is signed in.
    pub actor: Option<ActorView>,
    /// The till's reference of the part (CR-245).
    pub reference: Option<String>,
    /// A note (CR-206).
    pub note: Option<String>,
}

wire_record!(BillPartArgs {
    now,
    number,
    amount_minor,
    tip_minor,
    rate,
    source_label,
    source_fetched_at,
    actor,
    reference,
    note
});
wire_record_get!(BillPartArgs {
    now,
    number,
    amount_minor,
    tip_minor,
    rate,
    source_label,
    source_fetched_at,
    actor,
    reference,
    note
});

/// The arguments of `add_other_part` (CR-247).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OtherPartArgs {
    /// The time now.
    pub now: u64,
    /// The bill.
    pub number: u32,
    /// What was paid, in minor units.
    pub amount_minor: u64,
    /// How it was paid.
    pub way: PartWayView,
}

wire_record!(OtherPartArgs {
    now,
    number,
    amount_minor,
    way
});
wire_record_get!(OtherPartArgs {
    now,
    number,
    amount_minor,
    way
});

/// The arguments of `set_tab_amount` (CR-251).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TabAmountArgs {
    /// The time now.
    pub now: u64,
    /// The tab.
    pub number: u32,
    /// The new amount in minor units.
    pub amount_minor: u64,
}

wire_record!(TabAmountArgs {
    now,
    number,
    amount_minor
});
wire_record_get!(TabAmountArgs {
    now,
    number,
    amount_minor
});

/// The arguments of `finish_tab` and `cancel_bill`: the time and the bill.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BillNumberArgs {
    /// The time now.
    pub now: u64,
    /// The bill.
    pub number: u32,
}

wire_record!(BillNumberArgs { now, number });
wire_record_get!(BillNumberArgs { now, number });

/// The arguments of `close_short` (CR-252). The app asks only after the
/// manager's PIN was right (CR-84); the library checks the role.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloseShortArgs {
    /// The time now.
    pub now: u64,
    /// The bill.
    pub number: u32,
    /// The manager who closes it.
    pub actor: ActorView,
}

wire_record!(CloseShortArgs { now, number, actor });
wire_record_get!(CloseShortArgs { now, number, actor });

/// One part of a bill (CR-247, CR-248).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BillPartView {
    /// The receive index of a bitcoin part; none for a part paid another way.
    pub sale: Option<u32>,
    /// What it is worth, in minor units.
    pub amount_minor: u64,
    /// How a part paid another way was paid; none for a bitcoin part.
    pub way: Option<PartWayView>,
    /// Whether it counts towards the amount (CR-249).
    pub counts: bool,
}

wire_record!(BillPartView {
    sale,
    amount_minor,
    way,
    counts
});

/// A bill, with what it has received (CR-246, CR-249, CR-254).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BillRow {
    /// The number.
    pub number: u32,
    /// A bill or a tab.
    pub kind: BillKindView,
    /// The amount, in minor units.
    pub amount_minor: u64,
    /// The till's reference, when it has one.
    pub reference: Option<String>,
    /// When it opened.
    pub opened_at: u64,
    /// Where it stands.
    pub state: BillStateView,
    /// When it ended; 0 while it is open.
    pub ended_at: u64,
    /// What was not paid when a manager closed it short, in minor units.
    pub unpaid_minor: u64,
    /// What the bitcoin parts that count are worth, in minor units.
    pub received_bitcoin: u64,
    /// What the parts paid another way came to, in minor units.
    pub received_other: u64,
    /// Bitcoin parts not paid yet that may still be.
    pub pending: u32,
    /// What is left of the amount, in minor units.
    pub left: u64,
    /// The parts, in the order they were added.
    pub parts: Vec<BillPartView>,
    /// What the screen says for the state.
    pub words: WordsView,
}

wire_record!(BillRow {
    number,
    kind,
    amount_minor,
    reference,
    opened_at,
    state,
    ended_at,
    unpaid_minor,
    received_bitcoin,
    received_other,
    pending,
    left,
    parts,
    words
});

fn kind_of(v: BillKindView) -> BillKind {
    match v {
        BillKindView::Bill => BillKind::Bill,
        BillKindView::Tab => BillKind::Tab,
    }
}

fn kind_view(k: BillKind) -> BillKindView {
    match k {
        BillKind::Bill => BillKindView::Bill,
        BillKind::Tab => BillKindView::Tab,
    }
}

fn way_of(v: PartWayView) -> PartWay {
    match v {
        PartWayView::Cash => PartWay::Cash,
        PartWayView::Card => PartWay::Card,
        PartWayView::Other => PartWay::Other,
    }
}

fn way_view(w: PartWay) -> PartWayView {
    match w {
        PartWay::Cash => PartWayView::Cash,
        PartWay::Card => PartWayView::Card,
        PartWay::Other => PartWayView::Other,
    }
}

fn bill_error<E: std::fmt::Display>(e: &BillRefused<E>) -> CounterError {
    from_rule(e.rule(), e, e.words())
}

fn part_error<E: std::fmt::Display>(e: &PartRefused<E>) -> CounterError {
    from_rule(e.rule(), e, e.words())
}

impl Entry {
    /// CR-245: add the till's reference to a sale.
    pub(crate) fn add_reference(&self, a: ReferenceArgs) -> Result<(), CounterError> {
        self.run(Some(a.now), |node, _, _| {
            node.add_reference(a.id, &a.reference)
                .map_err(|e| bill_error(&e))
        })
    }

    /// CR-246: open a bill or a tab; returns its number.
    pub(crate) fn open_bill(&self, a: OpenBillArgs) -> Result<u32, CounterError> {
        self.run(Some(a.now), |node, _, _| {
            node.open_bill(
                kind_of(a.kind),
                Amount(a.amount_minor),
                a.reference.as_deref(),
            )
            .map_err(|e| bill_error(&e))
        })
    }

    /// CR-245: add the till's reference to a bill.
    pub(crate) fn add_bill_reference(&self, a: ReferenceArgs) -> Result<(), CounterError> {
        self.run(Some(a.now), |node, _, _| {
            node.add_bill_reference(a.id, &a.reference)
                .map_err(|e| bill_error(&e))
        })
    }

    /// CR-248: join a sale to a bill.
    pub(crate) fn join_sale(&self, a: JoinSaleArgs) -> Result<(), CounterError> {
        self.run(Some(a.now), |node, _, _| {
            node.join_sale(a.number, a.sale).map_err(|e| bill_error(&e))
        })
    }

    /// CR-248: open a sale for a bill and make it a part.
    pub(crate) fn open_bill_part(&self, a: BillPartArgs) -> Result<OpenedView, CounterError> {
        let by = a.actor.map(actor);
        self.run(Some(a.now), |node, _, network| {
            // CR-197: the rate of a shop priced in satoshis is not read.
            let (rate, source) = if node.settings().currency.is_sats() {
                (
                    openbmt_counter::Rate::sats(),
                    openbmt_counter::RateSource::sats(a.now),
                )
            } else {
                rate_and_source(a.rate, &a.source_label, a.source_fetched_at)?
            };
            let extras = SaleExtras {
                note: a.note.as_deref(),
                request: None,
                reference: a.reference.as_deref(),
            };
            let opened = node
                .open_bill_part(
                    a.number,
                    Amount(a.amount_minor),
                    Amount(a.tip_minor),
                    rate,
                    &source,
                    by.as_ref(),
                    extras,
                )
                .map_err(|e| part_error(&e))?;
            let payment = payment_for(&opened.script, network, opened.needs_sats)?;
            Ok(OpenedView {
                index: opened.index,
                needs_sats: opened.needs_sats,
                quote_rate: opened.quote.rate.minor_per_btc(),
                quote_from: opened.quote.from,
                quote_until: opened.quote.until,
                payment,
            })
        })
    }

    /// CR-247: a part paid another way.
    pub(crate) fn add_other_part(&self, a: OtherPartArgs) -> Result<(), CounterError> {
        self.run(Some(a.now), |node, _, _| {
            node.add_other_part(a.number, Amount(a.amount_minor), way_of(a.way))
                .map_err(|e| bill_error(&e))
        })
    }

    /// CR-250: what each of `people` pays of what is left of a bill.
    pub(crate) fn bill_split(&self, number: u32, people: u8) -> Result<Vec<u64>, CounterError> {
        self.run(None, |node, _, _| {
            node.bill_split(number, people).map_err(|e| bill_error(&e))
        })
    }

    /// CR-251: change the amount of a tab.
    pub(crate) fn set_tab_amount(&self, a: TabAmountArgs) -> Result<(), CounterError> {
        self.run(Some(a.now), |node, _, _| {
            node.set_tab_amount(a.number, Amount(a.amount_minor))
                .map_err(|e| bill_error(&e))
        })
    }

    /// CR-251: a tab is finished.
    pub(crate) fn finish_tab(&self, a: BillNumberArgs) -> Result<(), CounterError> {
        self.run(Some(a.now), |node, _, _| {
            node.finish_tab(a.number).map_err(|e| bill_error(&e))
        })
    }

    /// CR-252: close a bill short; returns what was not paid, in minor units.
    pub(crate) fn close_short(&self, a: CloseShortArgs) -> Result<u64, CounterError> {
        let by = actor(a.actor);
        self.run(Some(a.now), |node, _, _| {
            node.close_short(a.number, &by)
                .map(|unpaid| unpaid.0)
                .map_err(|e| bill_error(&e))
        })
    }

    /// CR-252: cancel a bill with no part that counts.
    pub(crate) fn cancel_bill(&self, a: BillNumberArgs) -> Result<(), CounterError> {
        self.run(Some(a.now), |node, _, _| {
            node.cancel_bill(a.number).map_err(|e| bill_error(&e))
        })
    }

    /// CR-246, CR-254: the bills, open ones and those ended in the last 7
    /// days, in the order of their numbers. The stored state is read; nothing
    /// is judged.
    pub(crate) fn bills(&self, now: u64) -> Result<Vec<BillRow>, CounterError> {
        self.run(Some(now), |node, _, _| {
            let rows = node
                .bill_views()
                .into_iter()
                .map(|v| {
                    let parts = v.bill.parts().iter().map(|p| part_view(node, p)).collect();
                    row_of(&v, parts)
                })
                .collect();
            Ok(rows)
        })
    }
}

fn part_view(node: &Node<Port>, part: &BillPart) -> BillPartView {
    match part {
        BillPart::Sale(index) => {
            let sale = node.counter().sale(*index);
            BillPartView {
                sale: Some(*index),
                amount_minor: sale.map_or(0, |s| s.own_amount().0),
                way: None,
                counts: sale.is_some_and(|s| s.pos() == PosState::Paid),
            }
        }
        BillPart::Other { amount, way } => BillPartView {
            sale: None,
            amount_minor: amount.0,
            way: Some(way_view(*way)),
            counts: true,
        },
    }
}

fn state_of(bill: &Bill) -> BillStateView {
    match bill.ended().map(|e| e.how) {
        None => BillStateView::Open,
        Some(BillEnd::Settled) => BillStateView::Settled,
        Some(BillEnd::ClosedShort) => BillStateView::ClosedShort,
        Some(BillEnd::Cancelled) => BillStateView::Cancelled,
    }
}

fn row_of(v: &BillView, parts: Vec<BillPartView>) -> BillRow {
    let ended = v.bill.ended();
    BillRow {
        number: v.bill.number(),
        kind: kind_view(v.bill.kind()),
        amount_minor: v.bill.amount().0,
        reference: v.bill.reference().map(str::to_string),
        opened_at: v.bill.opened_at(),
        state: state_of(&v.bill),
        ended_at: ended.map_or(0, |e| e.at),
        unpaid_minor: ended.map_or(0, |e| e.unpaid.0),
        received_bitcoin: v.received_bitcoin,
        received_other: v.received_other,
        pending: u32::try_from(v.pending).unwrap_or(u32::MAX),
        left: v.left,
        parts,
        words: WordsView(v.words()),
    }
}
