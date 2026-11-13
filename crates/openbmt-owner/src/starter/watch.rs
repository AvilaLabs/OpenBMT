//! What the owner node's watch says of the starter wallet (SS-36, SS-42,
//! SS-43): what it holds, and the transaction the owner phone signed for it
//! put in the node's outbox.
//!
//! The coins are the ones the node learns from checked blocks, with the
//! change of the owner's own transactions that wait for a block at 0
//! confirmations. The library has no network: the caller hands in what the
//! watch holds.

use std::collections::BTreeSet;

use bitcoin::consensus::deserialize;
use bitcoin::{OutPoint, Transaction, Txid};
use openbmt_spend::Coin;

use crate::error::{OwnerError, Refusal};
use crate::state::Owner;
use crate::types::{Deliverable, Delivery, Stage};
use crate::MAX_OUTBOX;

/// An output that pays the starter wallet, in a transaction the watch holds
/// and no block has confirmed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IncomingCoin {
    /// The output.
    pub outpoint: OutPoint,
    /// Its amount in satoshis.
    pub sats: u64,
}

/// SS-36, SS-37: what the starter wallet holds, in satoshis: its confirmed
/// coins plus the payments into it that no block holds yet. `coins` are the
/// ones the watch offers: they include, at 0 confirmations, the change of a
/// transaction the owner's own phone sent, which `incoming` may list again;
/// each output is counted once. The coins that such a transaction spends are
/// not offered, so they are not counted either.
pub fn held_sats(coins: &[Coin], incoming: &[IncomingCoin]) -> u64 {
    let mut seen: BTreeSet<OutPoint> = BTreeSet::new();
    let mut sum: u64 = 0;
    for coin in coins {
        if seen.insert(coin.outpoint) {
            let sats = coin.output().map_or(0, |o| o.value.to_sat());
            sum = sum.saturating_add(sats);
        }
    }
    for pay in incoming {
        if seen.insert(pay.outpoint) {
            sum = sum.saturating_add(pay.sats);
        }
    }
    sum
}

impl Owner {
    /// SS-30, SS-42: a finished transaction of the starter wallet goes to
    /// the outbox, where the node sends it and follows it as it does any
    /// other (AS-64). `refunds` are the queued refunds it pays, each
    /// marked signed; the move pays none. The queue is stored before the
    /// transaction is given to the outbox, and a refusal or a failed store
    /// changes nothing. Giving the same transaction again changes nothing
    /// and returns the same ID.
    pub fn take_starter<E>(
        &mut self,
        tx: &[u8],
        refunds: &[u32],
        persist: impl FnOnce(&[u8]) -> Result<(), E>,
    ) -> Result<Txid, OwnerError<E>> {
        let parsed: Transaction = deserialize(tx)
            .map_err(|_| Refusal::WrongStage("the finished transaction does not read"))?;
        let txid = parsed.compute_txid();
        let waits = |s: &crate::state::State| {
            s.outbox
                .iter()
                .any(|d| matches!(&d.what, Delivery::Transaction { txid: t, .. } if *t == txid))
        };
        if waits(&self.state) {
            return Ok(txid);
        }
        if self.state.outbox.len() >= MAX_OUTBOX {
            return Err(Refusal::OutboxFull.into());
        }
        let mut next = self.state.clone();
        for n in refunds {
            let queued = next
                .refunds
                .iter()
                .find(|r| r.id == *n)
                .is_some_and(|r| r.stage == Stage::Queued);
            if !queued {
                return Err(Refusal::UnknownEntry(*n).into());
            }
        }
        for n in refunds {
            if let Some(stage) = next.stage_mut(*n) {
                *stage = Stage::Signed { txid };
            }
        }
        let id = next.take_id()?;
        next.outbox.push(Deliverable {
            id,
            what: Delivery::Transaction {
                txid,
                tx: tx.to_vec(),
                refunds: refunds.to_vec(),
                cash_out: None,
            },
        });
        self.commit(next, persist)?;
        Ok(txid)
    }
}
