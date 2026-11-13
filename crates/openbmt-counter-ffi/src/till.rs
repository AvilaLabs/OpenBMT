//! A sale from the till through the bridge (CR-266 to CR-271): the
//! arguments and views of ops 85 to 88, and the calls of the entry behind
//! them. The words are the placeholders of the library until they are
//! written (CR-271).

use openbmt_counter::{till_cancelled_words, till_card_words, till_start_words, TillRequest};

use crate::codec::{wire_record, wire_record_get};
use crate::error::{from_rule, refused_by, CounterError};
use crate::host::{actor, rate_and_source, Entry};
use crate::pay::payment_for;
use crate::views::{ActorView, OpenedView, WordsView};

/// One request from the till that waits for the cashier (CR-267).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TillRow {
    /// The till's sale, 16 bytes. Start and Not now name it.
    pub sale: Vec<u8>,
    /// The amount in minor units of the shop's currency.
    pub amount_minor: u64,
    /// The reference the till sent, printable ASCII; empty when it sent none.
    pub reference: String,
    /// When the request lapses, in seconds.
    pub lapses_at: u64,
    /// Whole minutes left, counted up; at least 1 while it waits.
    pub minutes_left: u64,
    /// What the card says.
    pub words: WordsView,
}

wire_record!(TillRow {
    sale,
    amount_minor,
    reference,
    lapses_at,
    minutes_left,
    words
});

/// The arguments of `till_start` (CR-268): those of `open` for the
/// amount the request names, which the caller cannot change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TillStartArgs {
    /// The time now.
    pub now: u64,
    /// The till's sale, 16 bytes.
    pub sale: Vec<u8>,
    /// The rate, minor units per bitcoin.
    pub rate: u64,
    /// Where the rate came from.
    pub source_label: String,
    /// When the rate was fetched.
    pub source_fetched_at: u64,
    /// The person at the till, when one is signed in.
    pub actor: Option<ActorView>,
}

wire_record!(TillStartArgs {
    now,
    sale,
    rate,
    source_label,
    source_fetched_at,
    actor
});
wire_record_get!(TillStartArgs {
    now,
    sale,
    rate,
    source_label,
    source_fetched_at,
    actor
});

/// What a sale carries of the till (CR-268, CR-269).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TillSaleView {
    /// The till's sale, 16 bytes, when the sale came from the till.
    pub sale: Option<Vec<u8>>,
    /// Whether the till cancelled it.
    pub cancelled: bool,
    /// The line to show on the sale when the till cancelled it.
    pub words: Option<WordsView>,
}

wire_record!(TillSaleView {
    sale,
    cancelled,
    words
});

fn sale_id(bytes: &[u8]) -> Result<[u8; 16], CounterError> {
    bytes
        .try_into()
        .map_err(|_| refused_by("CR-266", "a till's sale is 16 bytes"))
}

fn row(r: &TillRequest, now: u64) -> TillRow {
    TillRow {
        sale: r.sale.to_vec(),
        amount_minor: r.amount.0,
        reference: r.reference.clone(),
        lapses_at: r.lapses_at,
        minutes_left: r.minutes_left(now),
        words: WordsView(till_card_words()),
    }
}

impl Entry {
    /// CR-267: the requests that wait, oldest first, not lapsed and not
    /// hidden by Not now.
    pub(crate) fn till_waiting(&self, now: u64) -> Result<Vec<TillRow>, CounterError> {
        self.run(Some(now), |node, _, _| {
            Ok(node.till_waiting().iter().map(|r| row(r, now)).collect())
        })
    }

    /// CR-268: Start. Opens the sale for the request's amount at the
    /// counter's own rate; the request stays when opening is refused.
    pub(crate) fn till_start(&self, a: TillStartArgs) -> Result<OpenedView, CounterError> {
        let sale = sale_id(&a.sale)?;
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
            let opened = node
                .till_start(&sale, rate, &source, by.as_ref())
                .map_err(|e| from_rule(e.rule(), &e, till_start_words(&e)))?;
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

    /// CR-267: Not now. Nothing is sent.
    pub(crate) fn till_not_now(&self, now: u64, sale: &[u8]) -> Result<(), CounterError> {
        let sale = sale_id(sale)?;
        self.run(Some(now), |node, _, _| {
            if node.till_not_now(&sale) {
                Ok(())
            } else {
                Err(refused_by(
                    "CR-266",
                    "no request from the till waits for this sale",
                ))
            }
        })
    }

    /// CR-268, CR-269: what a sale carries of the till.
    pub(crate) fn till_of_sale(&self, index: u32) -> Result<TillSaleView, CounterError> {
        self.run(None, |node, _, _| {
            let cancelled = node.till_cancelled(index);
            Ok(TillSaleView {
                sale: node.sale_till(index).map(|s| s.to_vec()),
                cancelled,
                words: cancelled.then(|| WordsView(till_cancelled_words())),
            })
        })
    }
}
