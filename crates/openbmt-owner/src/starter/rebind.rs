//! SS-41: the owner's queue moves with the re-pin. The snapshot is bound to
//! the wallet's checksum, so a queue stored for the starter wallet is stored
//! again for the new wallet before the new wallet's file is written, and a
//! start that finds the re-pin recorded finishes it.

use bitcoin::Txid;
use openbmt_core::wallet_single::ImportedWallet;

use super::StarterRefusal;
use crate::codec;
use crate::error::RestoreError;
use crate::state::Owner;
use crate::types::{Delivery, Stage};

/// Why a queue was not rebound: a rule refused, or the store failed (the
/// queue is as it was then).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RebindError<E> {
    /// SS-41: the starter wallet still has work in flight.
    Refused(StarterRefusal),
    /// The store failed.
    Store(E),
}

impl Owner {
    /// SS-41: what keeps the queue from moving to the new wallet: a
    /// transaction of the starter wallet other than the move that is waiting
    /// or has no confirmation, or work that is bound to the starter wallet
    /// itself (an open session, a pairing that waits). `confirmed` says
    /// whether the node holds a transaction at its confirmation. A move is a
    /// transaction that pays no refund and no cash-out. `None` when the
    /// queue may move.
    pub fn rebind_blocker(&self, confirmed: impl Fn(&Txid) -> bool) -> Option<&'static str> {
        for d in &self.state.outbox {
            if let Delivery::Transaction {
                txid,
                refunds,
                cash_out,
                ..
            } = &d.what
            {
                let a_move = refunds.is_empty() && cash_out.is_none();
                if !(a_move && confirmed(txid)) {
                    return Some("a transaction of the starter wallet is waiting or unconfirmed");
                }
            }
        }
        let in_flight = |s: &Stage| matches!(s, Stage::Signed { .. } | Stage::Sent { .. });
        if self.state.refunds.iter().any(|r| in_flight(&r.stage))
            || self.state.cash_outs.iter().any(|c| in_flight(&c.stage))
        {
            return Some("a payment of the starter wallet is signed and not confirmed");
        }
        if !self.state.sessions.is_empty() || !self.state.pairings.is_empty() {
            return Some("a session or a pairing is open for the starter wallet");
        }
        None
    }

    /// SS-41: the queue, stored for `new` before the new wallet's file is.
    /// The queued refunds, the paid facts of the sales and the rest of the
    /// queue stay as they are; the confirmed move and the settled entries
    /// stay where they are. With work in flight (see
    /// [`Owner::rebind_blocker`]) nothing changes and the refusal says so.
    /// A failed store changes nothing.
    pub fn rebind_wallet<E>(
        &mut self,
        new: impl Into<ImportedWallet>,
        confirmed: impl Fn(&Txid) -> bool,
        persist: impl FnOnce(&[u8]) -> Result<(), E>,
    ) -> Result<(), RebindError<E>> {
        if self.rebind_blocker(confirmed).is_some() {
            return Err(RebindError::Refused(StarterRefusal::Unsettled));
        }
        self.bind_to(new.into(), persist).map_err(|e| match e {
            BindFail::Store(e) => RebindError::Store(e),
            BindFail::Inconsistent(_) => RebindError::Refused(StarterRefusal::Unsettled),
        })
    }

    /// SS-41: a start that finds the re-pin recorded and the queue still
    /// stored for the starter wallet stores it for `new`, whatever waits:
    /// the node always starts. Only the snapshot's wallet changes.
    pub fn finish_rebind<E>(
        &mut self,
        new: impl Into<ImportedWallet>,
        persist: impl FnOnce(&[u8]) -> Result<(), E>,
    ) -> Result<(), RebindFinish<E>> {
        self.bind_to(new.into(), persist).map_err(|e| match e {
            BindFail::Store(e) => RebindFinish::Store(e),
            BindFail::Inconsistent(r) => RebindFinish::Restore(r),
        })
    }

    fn bind_to<E>(
        &mut self,
        new: ImportedWallet,
        persist: impl FnOnce(&[u8]) -> Result<(), E>,
    ) -> Result<(), BindFail<E>> {
        let (old_wallet, old_id) = (self.wallet.clone(), self.wallet_id);
        let no_key_app = self.no_key_app;
        self.wallet_id = new.checksum32();
        self.no_key_app = no_key_app || matches!(new, ImportedWallet::Single(_));
        self.wallet = new;
        let stored = codec::encode(self, &self.state)
            .map_err(|_| RestoreError::Inconsistent("the queue does not encode"))
            .and_then(|bytes| codec::decode(self, &bytes).map(|_| bytes));
        let result = match stored {
            Ok(bytes) => persist(&bytes).map_err(BindFail::Store),
            Err(e) => Err(BindFail::Inconsistent(e)),
        };
        if result.is_err() {
            self.wallet = old_wallet;
            self.wallet_id = old_id;
            self.no_key_app = no_key_app;
        }
        result
    }
}

/// Why a start could not store the queue for the new wallet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RebindFinish<E> {
    /// What is queued does not read back under the new wallet.
    Restore(RestoreError),
    /// The store failed.
    Store(E),
}

enum BindFail<E> {
    Store(E),
    Inconsistent(RestoreError),
}
