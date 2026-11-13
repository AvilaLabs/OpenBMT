//! The starter line through the bridge (CR-272 to CR-276): the views and the
//! arguments of ops 204 and 205, and the calls of the entry behind them. The
//! words are the placeholders of the node until they are written (CR-276).

use openbmt_counter_node::say::{
    starter_cards_words, starter_stopped_words, starter_unchecked_words,
};
use openbmt_counter_node::{StarterCard, StopDecision};

use crate::codec::{wire_record, wire_record_get, wire_unit_enum};
use crate::error::{from_rule, CounterError};
use crate::host::Entry;
use crate::views::WordsView;

/// What "New sale" does (CR-273).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StarterDecisionView {
    /// Offer bitcoin and say nothing.
    Open,
    /// Offer no bitcoin and say why.
    Stopped,
    /// Offer bitcoin, and Home says that the limit cannot be checked.
    Unchecked,
}

wire_unit_enum!(StarterDecisionView {
    Open = 0,
    Stopped = 1,
    Unchecked = 2
});

/// One payment out of the starter wallet (CR-274).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StarterPaymentView {
    /// The transaction ID, 32 bytes.
    pub txid: Vec<u8>,
    /// What it paid out of the wallet, in sats.
    pub sats: u64,
    /// The time the owner phone first saw it, in seconds since 1970.
    pub time: u64,
    /// The owner phone made it.
    pub by_owner: bool,
    /// Someone at the counter tapped that they have seen it.
    pub seen: bool,
}

wire_record!(StarterPaymentView {
    txid,
    sats,
    time,
    by_owner,
    seen
});

impl StarterPaymentView {
    fn of(c: &StarterCard) -> Self {
        Self {
            txid: c.payment.txid.to_vec(),
            sats: c.payment.sats,
            time: c.payment.time,
            by_owner: c.payment.by_owner,
            seen: c.seen,
        }
    }
}

/// What `starter_state` returns (CR-273, CR-274): the stop decision with its
/// words, what the last line said, and every payment kept, newest first. The
/// app shows the unseen ones as cards and the ones of the day in Today.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StarterStateView {
    /// Whether "New sale" offers bitcoin.
    pub decision: StarterDecisionView,
    /// The words of a stop or of a limit that cannot be checked; none when
    /// `decision` is open.
    pub words: Option<WordsView>,
    /// The heading of the cards on Home, when some payment is unseen.
    pub cards: Option<WordsView>,
    /// What the last line said the wallet holds, in sats.
    pub held_sats: Option<u64>,
    /// The limit the last line named, in sats.
    pub limit_sats: Option<u64>,
    /// The time of the last line, in seconds since 1970.
    pub line_time: Option<u64>,
    /// The payments kept, newest first.
    pub payments: Vec<StarterPaymentView>,
}

wire_record!(StarterStateView {
    decision,
    words,
    cards,
    held_sats,
    limit_sats,
    line_time,
    payments
});

/// The arguments of `starter_seen`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StarterSeenArgs {
    /// The time now.
    pub now: u64,
    /// The transaction ID of the payment, 32 bytes.
    pub txid: Vec<u8>,
}

wire_record!(StarterSeenArgs { now, txid });
wire_record_get!(StarterSeenArgs { now, txid });

impl Entry {
    /// CR-273, CR-274: what the last starter line says to "New sale", and
    /// the payments kept.
    pub(crate) fn starter_state(&self, now: u64) -> Result<StarterStateView, CounterError> {
        self.run(Some(now), |node, _, _| {
            let decision = node.starter_decision();
            let payments = node.starter_payments();
            let line = node.starter_line();
            Ok(StarterStateView {
                decision: match decision {
                    StopDecision::Open => StarterDecisionView::Open,
                    StopDecision::Stopped => StarterDecisionView::Stopped,
                    StopDecision::Unchecked => StarterDecisionView::Unchecked,
                },
                words: match decision {
                    StopDecision::Open => None,
                    StopDecision::Stopped => Some(WordsView(starter_stopped_words())),
                    StopDecision::Unchecked => Some(WordsView(starter_unchecked_words())),
                },
                cards: payments
                    .iter()
                    .any(|c| !c.seen)
                    .then(|| WordsView(starter_cards_words())),
                held_sats: line.map(|l| l.held_sats),
                limit_sats: line.map(|l| l.limit_sats),
                line_time: line.map(|l| l.time),
                payments: payments.iter().map(StarterPaymentView::of).collect(),
            })
        })
    }

    /// CR-274: someone at the counter has seen this payment.
    pub(crate) fn starter_seen(&self, a: StarterSeenArgs) -> Result<(), CounterError> {
        self.run(Some(a.now), |node, _, _| {
            let txid: [u8; 32] = a.txid.as_slice().try_into().map_err(|_| {
                crate::error::refused_by("request-malformed", "a transaction ID is 32 bytes")
            })?;
            node.starter_seen(&txid)
                .map_err(|e| from_rule(e.rule(), &e, e.words()))
        })
    }
}
