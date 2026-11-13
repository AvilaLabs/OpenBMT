//! Returning money (RQ-26 to RQ-29).

use bitcoin::ScriptBuf;
use openbmt_counter::refund::{dust_limit, is_payment_script};

use crate::book::{RequestBook, MAX_RETURNS};
use crate::error::ReturnRefused;
use crate::model::{sats_floor, CheckedRate, Return, ReturnState};
use openbmt_counter::money::MAX_SATS;

/// Most returns that are queued or in a spend not yet confirmed at once:
/// the most outputs a spend takes (100), less one for the change (RQ-29).
pub const MAX_QUEUED_RETURNS: usize = 99;

/// How the owner chooses the amount of a return (RQ-26).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReturnAmount {
    /// In satoshis.
    Sats(u64),
    /// In minor units of the shop's currency, at the rate of now, rounded
    /// down.
    Units(u64),
}

impl RequestBook {
    /// RQ-26: what may still be returned from a request: what it received
    /// less what was already returned from it, queued, sent or done.
    pub fn returnable_sats(&self, number: u64) -> Option<u64> {
        let request = self.request(number)?;
        let returned = self
            .returns
            .iter()
            .filter(|r| r.request == number)
            .fold(0u64, |t, r| t.saturating_add(r.sats));
        Some(request.received_sats().saturating_sub(returned))
    }

    /// RQ-26 to RQ-29: queue a return to an address the customer sent,
    /// already checked by [`check_return_address`](crate::check_return_address).
    /// Returns its id. Nothing is paid; see
    /// [`returns_to_pay`](Self::returns_to_pay).
    pub fn queue_return(
        &mut self,
        number: u64,
        amount: ReturnAmount,
        script: ScriptBuf,
        now: u64,
        rate: Option<&CheckedRate>,
    ) -> Result<u64, ReturnRefused> {
        let most_sats = self
            .returnable_sats(number)
            .ok_or(ReturnRefused::NoSuchRequest)?;
        let min_sats = dust_limit(&script)
            .filter(|_| is_payment_script(&script))
            .ok_or(ReturnRefused::BadScript)?;
        let sats = match amount {
            ReturnAmount::Sats(s) => s,
            ReturnAmount::Units(units) => {
                let rate = rate
                    .and_then(|r| r.fresh_at(now))
                    .ok_or(ReturnRefused::StaleRate)?;
                sats_floor(units, rate)
            }
        };
        if sats > most_sats || sats > MAX_SATS {
            return Err(ReturnRefused::TooMuch { most_sats });
        }
        if sats < min_sats {
            return Err(ReturnRefused::TooSmall { min_sats });
        }
        let in_flight = self
            .returns
            .iter()
            .filter(|r| matches!(r.state, ReturnState::Queued | ReturnState::Sent { .. }))
            .count();
        if in_flight >= MAX_QUEUED_RETURNS {
            return Err(ReturnRefused::QueueFull);
        }
        if self.returns.len() >= MAX_RETURNS {
            return Err(ReturnRefused::BookFull);
        }
        let id = self.next_return;
        self.returns.push(Return {
            id,
            request: number,
            script,
            sats,
            queued_at: now,
            state: ReturnState::Queued,
        });
        self.next_return += 1;
        Ok(id)
    }

    /// RQ-29: the queued returns as the payments of one spend (`payments` of
    /// the spend library): each its own output, in the order they were
    /// queued.
    pub fn returns_to_pay(&self) -> Vec<(ScriptBuf, u64)> {
        self.returns
            .iter()
            .filter(|r| r.state == ReturnState::Queued)
            .map(|r| (r.script.clone(), r.sats))
            .collect()
    }

    /// The ids of the queued returns, in the order of
    /// [`returns_to_pay`](Self::returns_to_pay).
    pub fn queued_return_ids(&self) -> Vec<u64> {
        self.returns
            .iter()
            .filter(|r| r.state == ReturnState::Queued)
            .map(|r| r.id)
            .collect()
    }

    /// RQ-29: the spend `txid` was built with these returns. They leave the
    /// queue, so another "Pay the returns" does not pay them twice. Ids that
    /// are not queued are left as they are. Returns how many changed.
    pub fn mark_returns_sent(&mut self, ids: &[u64], txid: [u8; 32]) -> usize {
        self.move_returns(ids, ReturnState::Queued, ReturnState::Sent { txid })
    }

    /// RQ-29: the spend that pays these returns is confirmed.
    pub fn mark_returns_done(&mut self, txid: [u8; 32], at: u64) -> usize {
        let mut n = 0;
        for r in &mut self.returns {
            if r.state == (ReturnState::Sent { txid }) {
                r.state = ReturnState::Done { txid, at };
                n += 1;
            }
        }
        n
    }

    /// RQ-29: the spend `txid` was replaced without them: they go back to
    /// the queue. A return that is done stays done. Returns how many are in
    /// the queue again. A request that already shows more returned than
    /// received (RQ-37) keeps none of its requeued returns that would add to
    /// it: they come off the queue again, newest first, as after a reorg.
    pub fn requeue_returns(&mut self, txid: [u8; 32]) -> usize {
        let mut n = 0usize;
        let mut touched: Vec<u64> = Vec::new();
        for r in &mut self.returns {
            if r.state == (ReturnState::Sent { txid }) {
                r.state = ReturnState::Queued;
                n += 1;
                if !touched.contains(&r.request) {
                    touched.push(r.request);
                }
            }
        }
        n.saturating_sub(self.trim_queued(&touched).len())
    }

    fn move_returns(&mut self, ids: &[u64], from: ReturnState, to: ReturnState) -> usize {
        let mut n = 0;
        for r in &mut self.returns {
            if ids.contains(&r.id) && r.state == from {
                r.state = to;
                n += 1;
            }
        }
        n
    }
}
