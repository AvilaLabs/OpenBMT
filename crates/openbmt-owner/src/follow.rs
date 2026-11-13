//! Following what the caller does with the finished objects (AS-54, AS-64).

use std::collections::BTreeSet;

use bitcoin::consensus::{deserialize, serialize};
use bitcoin::hashes::Hash;
use bitcoin::{OutPoint, Transaction, Txid};

use crate::error::{OwnerError, Refusal};
use crate::state::{Owner, State};
use crate::types::{Delivery, Held, HeldEntry, SignedElsewhere, Stage};
use crate::{MAX_CONFLICT_OUTPOINTS, MAX_SIGNED_ELSEWHERE};

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

/// AS-89: take `entry` out of the waiting transaction `held`, which cannot
/// confirm any more. The transaction goes, and its other entries are returned
/// as AS-54 does when another transaction spent their coins.
fn drop_waiting(st: &mut State, held: &Txid, entry: u32) {
    let Some((at, ids)) = waiting(st, held) else {
        return;
    };
    for id in ids.into_iter().filter(|n| *n != entry) {
        if let Some(stage) = st.stage_mut(id) {
            *stage = Stage::Returned {
                reason: "its coins were spent by another transaction".to_string(),
            };
        }
    }
    st.outbox.remove(at);
}

/// OC-55: keep `record`, a beaten transaction, in the list of those that
/// are watched, in the order of the list. A record of the same ID is
/// replaced.
fn insert_held(st: &mut State, record: SignedElsewhere) -> Result<(), Refusal> {
    if let Some(at) = st.elsewhere.iter().position(|e| e.txid == record.txid) {
        if let Some(slot) = st.elsewhere.get_mut(at) {
            *slot = record;
        }
        return Ok(());
    }
    if st.elsewhere.len() >= MAX_SIGNED_ELSEWHERE {
        return Err(Refusal::Full("the list of payments signed elsewhere"));
    }
    let place = st
        .elsewhere
        .partition_point(|e| e.txid.to_byte_array() < record.txid.to_byte_array());
    st.elsewhere.insert(place, record);
    Ok(())
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

    /// AS-54: a transaction that was reported paid left the best chain while
    /// its undo record was still available. Restore any matching paid rows to
    /// sent and put the exact signed transaction back in the outbox. Returns
    /// `false` when all rows have been archived or removed; the caller can
    /// still resend the transaction from its coin view.
    pub fn reopen_paid<E>(
        &mut self,
        tx: &Transaction,
        persist: impl FnOnce(&[u8]) -> Result<(), E>,
    ) -> Result<bool, OwnerError<E>> {
        let txid = tx.compute_txid();
        let bytes = serialize(tx);
        if waiting(&self.state, &txid).is_some() {
            return Ok(false);
        }
        let refunds: Vec<u32> = self
            .state
            .refunds
            .iter()
            .filter(|r| matches!(&r.stage, Stage::Paid { txid: paid } if *paid == txid))
            .map(|r| r.id)
            .collect();
        let cash_outs: Vec<u32> = self
            .state
            .cash_outs
            .iter()
            .filter(|c| matches!(&c.stage, Stage::Paid { txid: paid } if *paid == txid))
            .map(|c| c.id)
            .collect();
        if refunds.is_empty() && cash_outs.is_empty() {
            return Ok(false);
        }
        if cash_outs.len() > 1 {
            return Err(Refusal::WrongStage(
                "one transaction cannot reopen more than one cash-out",
            )
            .into());
        }
        if self.state.outbox.len() >= crate::MAX_OUTBOX {
            return Err(Refusal::OutboxFull.into());
        }
        let mut next = self.state.clone();
        for id in &refunds {
            if let Some(entry) = next.refunds.iter_mut().find(|r| r.id == *id) {
                entry.stage = Stage::Sent { txid };
            }
        }
        for id in &cash_outs {
            if let Some(entry) = next.cash_outs.iter_mut().find(|c| c.id == *id) {
                entry.stage = Stage::Sent { txid };
            }
        }
        let id = next.take_id()?;
        next.outbox.push(crate::types::Deliverable {
            id,
            what: Delivery::Transaction {
                txid,
                tx: bytes,
                refunds,
                cash_out: cash_outs.first().copied(),
            },
        });
        self.commit(next, persist)?;
        Ok(true)
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

    /// AS-54: the caller saw another transaction spend the coins this one
    /// spends. Its refunds go back to returned, and no conflict is kept,
    /// because the coins are gone for good.
    pub fn mark_replaced<E>(
        &mut self,
        txid: &Txid,
        persist: impl FnOnce(&[u8]) -> Result<(), E>,
    ) -> Result<(), OwnerError<E>> {
        let mut next = self.state.clone();
        let (at, ids) = waiting(&next, txid).ok_or(Refusal::UnknownTransaction(*txid))?;
        for id in ids {
            if let Some(stage) = next.stage_mut(id) {
                *stage = Stage::Returned {
                    reason: "its coins were spent by another transaction".to_string(),
                };
            }
        }
        next.outbox.remove(at);
        self.commit(next, persist)
    }

    /// AS-89: a transaction that was given to a hardware signer and whose
    /// approval was abandoned was seen confirmed. Its entries are paid with
    /// its ID, as if it had been taken. An entry that a new approval holds
    /// has left that approval's item, and one that a new transaction holds
    /// is paid and that transaction is dropped, since the two conflict. The
    /// IDs of the dropped transactions are returned, so that the caller stops
    /// sending them.
    pub fn mark_elsewhere_confirmed<E>(
        &mut self,
        txid: &Txid,
        persist: impl FnOnce(&[u8]) -> Result<(), E>,
    ) -> Result<Vec<Txid>, OwnerError<E>> {
        let mut next = self.state.clone();
        let at = next
            .elsewhere
            .iter()
            .position(|e| e.txid == *txid)
            .ok_or(Refusal::UnknownTransaction(*txid))?;
        let record = next.elsewhere.remove(at);
        let mut dropped: Vec<Txid> = Vec::new();
        for id in record.refunds.iter().copied().chain(record.cash_out) {
            let stage = match next.stage_mut(id) {
                Some(s) => s.clone(),
                None => continue,
            };
            match stage {
                Stage::Paid { .. } => continue,
                Stage::Queued | Stage::Returned { .. } => {}
                Stage::InSession { session } => self.leave_item(&mut next, &session, id)?,
                Stage::Signed { txid: held } | Stage::Sent { txid: held } => {
                    drop_waiting(&mut next, &held, id);
                    dropped.push(held);
                }
            }
            if let Some(stage) = next.stage_mut(id) {
                *stage = Stage::Paid { txid: *txid };
            }
        }
        self.commit(next, persist)?;
        Ok(dropped)
    }

    /// AS-89: a transaction that was given to a hardware signer was seen
    /// confirmed while its approval is still open. Its entries are paid with
    /// its ID exactly as taking it would have left them, its item leaves the
    /// approval, and the approval ends with its last item, so there is
    /// nothing left to abandon.
    pub fn mark_exported_confirmed<E>(
        &mut self,
        txid: &Txid,
        persist: impl FnOnce(&[u8]) -> Result<(), E>,
    ) -> Result<(), OwnerError<E>> {
        let mut next = self.state.clone();
        let found = next.sessions.iter().find_map(|s| {
            let exported = crate::state::exported_in(s).ok()?;
            let record = exported.into_iter().find(|e| e.txid == *txid)?;
            let number = s.frozen.iter().find_map(|(n, f)| match f {
                crate::state::Frozen::Spend { txid: t, .. } if t == txid => Some(*n),
                _ => None,
            })?;
            Some((s.id, number, record))
        });
        let (session, number, record) = found.ok_or(Refusal::UnknownTransaction(*txid))?;
        for id in record.refunds.iter().copied().chain(record.cash_out) {
            if let Some(stage) = next.stage_mut(id) {
                *stage = Stage::Paid { txid: *txid };
            }
        }
        self.remove_item(&mut next, &session, number)?;
        self.commit(next, persist)
    }

    /// AS-105: another transaction spent a coin of a transaction whose file
    /// was given to the shop's own wallet, while its session is open, so the
    /// transaction can no longer confirm. Its item leaves the session and its
    /// entries are queued again, keeping the outpoints it spent as AS-68
    /// says: while any of them is among the coins a new transaction spends
    /// it, and when none is the entry is planned freely.
    pub fn mark_exported_replaced<E>(
        &mut self,
        txid: &Txid,
        persist: impl FnOnce(&[u8]) -> Result<(), E>,
    ) -> Result<(), OwnerError<E>> {
        let next = self.exported_replaced_next(txid, None)?;
        self.commit(next, persist)
    }

    /// The state after an open own-wallet transaction was beaten, with it
    /// held when `held` is given.
    fn exported_replaced_next(&self, txid: &Txid, held: Option<Held>) -> Result<State, Refusal> {
        let mut next = self.state.clone();
        let found = next.sessions.iter().find_map(|s| {
            s.own.as_ref()?;
            let exported = crate::state::exported_in(s).ok()?;
            let record = exported.into_iter().find(|e| e.txid == *txid)?;
            let number = s.frozen.iter().find_map(|(n, f)| match f {
                crate::state::Frozen::Spend { txid: t, .. } if t == txid => Some(*n),
                _ => None,
            })?;
            Some((s.id, number, record))
        });
        let (session, number, record) = found.ok_or(Refusal::UnknownTransaction(*txid))?;
        for id in record.refunds.iter().copied().chain(record.cash_out) {
            if let Some(stage) = next.stage_mut(id) {
                *stage = Stage::Queued;
            }
            if let Some(list) = next.conflict_mut(id) {
                list.extend(record.inputs.iter().copied());
                list.sort();
                list.dedup();
                if list.len() > MAX_CONFLICT_OUTPOINTS {
                    return Err(Refusal::Full("the list of outpoints kept for a refund"));
                }
            }
        }
        self.remove_item(&mut next, &session, number)?;
        if held.is_some() {
            insert_held(&mut next, SignedElsewhere { held, ..record })?;
        }
        Ok(next)
    }

    /// AS-89: another transaction spent a coin of the transaction that was
    /// given to a hardware signer, so it can no longer confirm. It is no
    /// longer watched. The outpoints kept with its entries stay: while any
    /// of them is among the coins, a new transaction spends it (AS-68), and
    /// when none is, the entry is planned freely.
    pub fn mark_elsewhere_replaced<E>(
        &mut self,
        txid: &Txid,
        persist: impl FnOnce(&[u8]) -> Result<(), E>,
    ) -> Result<(), OwnerError<E>> {
        let mut next = self.state.clone();
        let at = next
            .elsewhere
            .iter()
            .position(|e| e.txid == *txid)
            .ok_or(Refusal::UnknownTransaction(*txid))?;
        next.elsewhere.remove(at);
        self.commit(next, persist)
    }

    /// OC-55: a block beat a transaction of the owner's own outbox. It
    /// leaves the outbox and its entries are returned, but held: the
    /// transaction stays watched, as one signed elsewhere is, until `held`'s
    /// block is deep. Held again with the same ID is stored once.
    pub fn mark_replaced_held<E>(
        &mut self,
        txid: &Txid,
        held: Held,
        persist: impl FnOnce(&[u8]) -> Result<(), E>,
    ) -> Result<(), OwnerError<E>> {
        let mut next = self.state.clone();
        let (at, ids) = waiting(&next, txid).ok_or(Refusal::UnknownTransaction(*txid))?;
        let (mut inputs, refunds, cash_out) = match next.outbox.get(at).map(|d| &d.what) {
            Some(Delivery::Transaction {
                tx,
                refunds,
                cash_out,
                ..
            }) => (
                deserialize::<Transaction>(tx)
                    .map(|t| t.input.iter().map(|i| i.previous_output).collect())
                    .unwrap_or_else(|_| Vec::new()),
                refunds.clone(),
                *cash_out,
            ),
            _ => return Err(Refusal::UnknownTransaction(*txid).into()),
        };
        inputs.sort();
        inputs.dedup();
        for id in ids {
            if let Some(stage) = next.stage_mut(id) {
                *stage = Stage::Returned {
                    reason: "its coins were spent by another transaction".to_string(),
                };
            }
        }
        next.outbox.remove(at);
        insert_held(
            &mut next,
            SignedElsewhere {
                txid: *txid,
                refunds,
                cash_out,
                inputs,
                held: Some(held),
            },
        )?;
        self.commit(next, persist)
    }

    /// OC-55: a block beat a transaction that was given to a hardware
    /// signer and is watched. It stays watched and its entries are held. A
    /// transaction that is held already takes the new block only when the
    /// block it had is no longer on the chain, which the caller says in
    /// `old_block_gone`.
    pub fn hold_elsewhere<E>(
        &mut self,
        txid: &Txid,
        held: Held,
        old_block_gone: bool,
        persist: impl FnOnce(&[u8]) -> Result<(), E>,
    ) -> Result<(), OwnerError<E>> {
        let mut next = self.state.clone();
        let record = next
            .elsewhere
            .iter_mut()
            .find(|e| e.txid == *txid)
            .ok_or(Refusal::UnknownTransaction(*txid))?;
        match record.held {
            Some(old) if !old_block_gone || old.block == held.block => return Ok(()),
            _ => record.held = Some(held),
        }
        self.commit(next, persist)
    }

    /// OC-55: a block beat a transaction whose file was given to the shop's
    /// own wallet, while its session is open (AS-105). As
    /// [`Owner::mark_exported_replaced`], and the transaction stays watched
    /// with its entries held.
    pub fn mark_exported_replaced_held<E>(
        &mut self,
        txid: &Txid,
        held: Held,
        persist: impl FnOnce(&[u8]) -> Result<(), E>,
    ) -> Result<(), OwnerError<E>> {
        let next = self.exported_replaced_next(txid, Some(held))?;
        self.commit(next, persist)
    }

    /// OC-55: the transaction `txid` is no longer watched and its entries
    /// are no longer held: the block that beat it is deep. The entries are
    /// as they were returned. An error when it is not held.
    pub fn release_held<E>(
        &mut self,
        txid: &Txid,
        persist: impl FnOnce(&[u8]) -> Result<(), E>,
    ) -> Result<(), OwnerError<E>> {
        let mut next = self.state.clone();
        let at = next
            .elsewhere
            .iter()
            .position(|e| e.txid == *txid && e.held.is_some())
            .ok_or(Refusal::UnknownTransaction(*txid))?;
        next.elsewhere.remove(at);
        self.commit(next, persist)
    }

    /// OC-55: every held entry, with the transaction that may still confirm
    /// and the block that beat it, ascending by entry number.
    pub fn held_entries(&self) -> Vec<HeldEntry> {
        let mut out: Vec<HeldEntry> = Vec::new();
        for e in &self.state.elsewhere {
            let Some(h) = e.held else { continue };
            for id in e.refunds.iter().copied().chain(e.cash_out) {
                out.push(HeldEntry {
                    id,
                    txid: e.txid,
                    block: h.block,
                    height: h.height,
                });
            }
        }
        out.sort_by_key(|h| (h.id, h.height));
        out
    }

    /// AS-89: an entry that is in an open approval is paid by another
    /// transaction. The item that holds it leaves the approval and the other
    /// entries of the item are queued again.
    fn leave_item(&self, next: &mut State, session: &[u8; 32], entry: u32) -> Result<(), Refusal> {
        let held = next
            .sessions
            .iter()
            .find(|s| s.id == *session)
            .and_then(|s| {
                s.frozen.iter().find_map(|(number, f)| match f {
                    crate::state::Frozen::Spend {
                        refunds, cash_out, ..
                    } if refunds.contains(&entry) || *cash_out == Some(entry) => {
                        Some((*number, refunds.clone(), *cash_out))
                    }
                    _ => None,
                })
            });
        let Some((number, refunds, cash_out)) = held else {
            return Ok(());
        };
        self.remove_item(next, session, number)?;
        for other in refunds.into_iter().chain(cash_out).filter(|n| *n != entry) {
            if let Some(stage) = next.stage_mut(other) {
                *stage = Stage::Queued;
            }
        }
        Ok(())
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
    /// `retained_confirmations` must contain every paid transaction whose
    /// confirmation can still be undone by the caller's chain view. Omitting
    /// one can archive a row that should be reopened after a reorganization.
    pub fn archive_paid<E>(
        &mut self,
        retained_confirmations: &BTreeSet<Txid>,
        persist: impl FnOnce(&[u8]) -> Result<(), E>,
    ) -> Result<usize, OwnerError<E>> {
        let mut next = self.state.clone();
        let mut gone = 0usize;
        let paid: Vec<(u32, crate::types::SaleKey, u64)> = next
            .refunds
            .iter()
            .filter(|r| {
                matches!(&r.stage, Stage::Paid { txid } if !retained_confirmations.contains(txid))
            })
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
        next.cash_outs.retain(
            |c| !matches!(&c.stage, Stage::Paid { txid } if !retained_confirmations.contains(txid)),
        );
        gone += before - next.cash_outs.len();
        self.commit(next, persist)?;
        Ok(gone)
    }
}
