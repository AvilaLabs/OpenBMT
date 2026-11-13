//! The line of a request in the full ledger (RQ-32), as a plain struct the
//! books crate can take.

use std::collections::BTreeMap;

use crate::book::RequestBook;
use crate::date::{day_of, Day};
use crate::model::{End, Marks, Pricing, Request, ReturnState};

/// One payment on a ledger line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LedgerPayment {
    /// The block's time, in Unix seconds.
    pub block_time: u64,
    /// The satoshis.
    pub sats: u64,
    /// What it was credited, in the request's unit.
    pub credit: u64,
}

/// One return on a ledger line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LedgerReturn {
    /// The satoshis.
    pub sats: u64,
    /// When it was queued, in Unix seconds.
    pub queued_at: u64,
    /// When its spend was confirmed, if it was.
    pub done_at: Option<u64>,
}

/// A request as a line of the full ledger (RQ-32).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LedgerLine {
    /// The number.
    pub number: u64,
    /// The note.
    pub note: String,
    /// When it was made, in Unix seconds.
    pub made_at: u64,
    /// The day it was made.
    pub made: Day,
    /// The due date.
    pub due: Day,
    /// How it is priced.
    pub priced_in: Pricing,
    /// The amount: satoshis, or minor units of the currency.
    pub amount: u64,
    /// Each payment, in the order of the blocks.
    pub payments: Vec<LedgerPayment>,
    /// The marks of RQ-16 to RQ-20.
    pub marks: Marks,
    /// How it ended, and when.
    pub ended: Option<(End, u64)>,
    /// Each return.
    pub returns: Vec<LedgerReturn>,
}

impl RequestBook {
    /// RQ-32: the line of one request.
    pub fn ledger_line(&self, request: &Request) -> LedgerLine {
        LedgerLine {
            number: request.number,
            note: request.note.clone(),
            made_at: request.made_at,
            made: request.made,
            due: request.due,
            priced_in: request.pricing,
            amount: request.amount,
            payments: request
                .credits
                .iter()
                .map(|c| LedgerPayment {
                    block_time: c.block_time,
                    sats: c.sats,
                    credit: c.value,
                })
                .collect(),
            marks: request.marks(),
            ended: request.end,
            returns: self
                .returns
                .iter()
                .filter(|r| r.request == request.number && r.state.counts())
                .map(|r| LedgerReturn {
                    sats: r.sats,
                    queued_at: r.queued_at,
                    done_at: match r.state {
                        ReturnState::Done { at, .. } => Some(at),
                        _ => None,
                    },
                })
                .collect(),
        }
    }

    /// RQ-32: every request as a line, by number.
    pub fn ledger_lines(&self) -> Vec<LedgerLine> {
        self.requests.iter().map(|r| self.ledger_line(r)).collect()
    }

    /// RQ-32: the satoshis that arrived for requests on each day, counted on
    /// the day of the payment's block in the shop's time zone, for the
    /// end-of-day totals.
    pub fn payments_by_day(&self, tz_offset_secs: i32) -> BTreeMap<Day, u64> {
        let mut days = BTreeMap::new();
        for c in self.requests.iter().flat_map(|r| &r.credits) {
            let total = days
                .entry(day_of(c.block_time, tz_offset_secs))
                .or_insert(0u64);
            *total = total.saturating_add(c.sats);
        }
        days
    }
}
