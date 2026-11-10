//! Following what the caller does with the finished objects (AS-54, AS-64).

use bitcoin::consensus::deserialize;
use bitcoin::{OutPoint, Transaction, Txid};

use crate::error::{OwnerError, Refusal};
use crate::state::{Owner, State};
use crate::types::{Delivery, Stage};
use crate::MAX_CONFLICT_OUTPOINTS;

/// AS-63, AS-68: the reason a refund or a cash-out carries when its
/// transaction was reported dropped. The words of a returned entry tell it
/// from a decline by this text.
pub(crate) const REASON_DROPPED: &str = "the transaction was dropped";

/// The refunds and the cash-out a waiting transaction pays.
fn waiting(st: &State, txid: &Txid) -> Option<(usize, Vec<u32>)> {
    st.outbox
        .iter()
        .enumerate()
        .find_map(|(at, d)| match &d.what {
            Delivery::Transaction {
                txid: t,
                refunds,
                cash_out,
                ..
            } if t == txid => Some((at, refunds.iter().copied().chain(*cash_out).collect())),
            _ => None,
        })
}

impl Owner {
    /// AS-54: the caller broadcast the transaction. Its refunds are sent.
    pub fn mark_broadcast<E>(
        &mut self,
        txid: &Txid,
        persist: impl FnOnce(&[u8]) -> Result<(), E>,
    ) -> Result<(), OwnerError<E>> {
        let mut next = self.state.clone();
        let (_, ids) = waiting(&next, txid).ok_or(Refusal::UnknownTransaction(*txid))?;
        for id in ids {
            if let Some(stage) = next.stage_mut(id) {
                *stage = Stage::Sent { txid: *txid };
            }
        }
        self.commit(next, persist)
    }

    /// AS-54: the caller saw the transaction confirmed. Its refunds are
    /// paid, and the transaction no longer waits.
    pub fn mark_confirmed<E>(
        &mut self,
        txid: &Txid,
        persist: impl FnOnce(&[u8]) -> Result<(), E>,
    ) -> Result<(), OwnerError<E>> {
        let mut next = self.state.clone();
        let (at, ids) = waiting(&next, txid).ok_or(Refusal::UnknownTransaction(*txid))?;
        for id in ids {
            if let Some(stage) = next.stage_mut(id) {
                *stage = Stage::Paid { txid: *txid };
            }
        }
        next.outbox.remove(at);
        self.commit(next, persist)
    }

    /// AS-54: the caller reports the transaction dropped. Its refunds go
    /// back to returned and keep counting against their sales.
    pub fn mark_dropped<E>(
        &mut self,
        txid: &Txid,
        persist: impl FnOnce(&[u8]) -> Result<(), E>,
    ) -> Result<(), OwnerError<E>> {
        let mut next = self.state.clone();
        let (at, ids) = waiting(&next, txid).ok_or(Refusal::UnknownTransaction(*txid))?;
        // AS-68: what the dropped transaction spent is kept with each entry
        // it paid, so that a new transaction conflicts with it.
        let spent: Vec<OutPoint> = match next.outbox.get(at).map(|d| &d.what) {
            Some(Delivery::Transaction { tx, .. }) => deserialize::<Transaction>(tx)
                .map(|t| t.input.iter().map(|i| i.previous_output).collect())
                .unwrap_or_default(),
            _ => Vec::new(),
        };
        for id in ids {
            if let Some(stage) = next.stage_mut(id) {
                *stage = Stage::Returned {
                    reason: REASON_DROPPED.to_string(),
                };
            }
            if let Some(list) = next.conflict_mut(id) {
                list.extend(spent.iter().copied());
                list.sort();
                list.dedup();
                if list.len() > MAX_CONFLICT_OUTPOINTS {
                    return Err(Refusal::Full("the list of outpoints kept for a refund").into());
                }
            }
        }
        next.outbox.remove(at);
        self.commit(next, persist)
    }

    /// AS-64: the caller delivered a signed batch, settings item or pairing.
    pub fn mark_delivered<E>(
        &mut self,
        id: u32,
        persist: impl FnOnce(&[u8]) -> Result<(), E>,
    ) -> Result<(), OwnerError<E>> {
        let mut next = self.state.clone();
        let at = next
            .outbox
            .iter()
            .position(|d| d.id == id)
            .ok_or(Refusal::UnknownEntry(id))?;
        if matches!(
            next.outbox.get(at).map(|d| &d.what),
            Some(Delivery::Transaction { .. })
        ) {
            return Err(Refusal::WrongStage(
                "a transaction is followed with broadcast, confirmed or dropped",
            )
            .into());
        }
        next.outbox.remove(at);
        self.commit(next, persist)
    }

    /// AS-50, AS-52: take the paid refunds and cash-outs out of the queue.
    /// What the paid refunds added up to stays counted against their sales.
    pub fn archive_paid<E>(
        &mut self,
        persist: impl FnOnce(&[u8]) -> Result<(), E>,
    ) -> Result<usize, OwnerError<E>> {
        let mut next = self.state.clone();
        let mut gone = 0usize;
        let paid: Vec<(u32, crate::types::SaleKey, u64)> = next
            .refunds
            .iter()
            .filter(|r| matches!(r.stage, Stage::Paid { .. }))
            .map(|r| (r.id, r.sale, r.sats))
            .collect();
        for (id, sale, sats) in paid {
            if let Some(entry) = next.sales.get_mut(&sale) {
                entry.archived_paid = entry.archived_paid.saturating_add(sats);
                next.refunds.retain(|r| r.id != id);
                gone += 1;
            }
        }
        let before = next.cash_outs.len();
        next.cash_outs
            .retain(|c| !matches!(c.stage, Stage::Paid { .. }));
        gone += before - next.cash_outs.len();
        self.commit(next, persist)?;
        Ok(gone)
    }
}
