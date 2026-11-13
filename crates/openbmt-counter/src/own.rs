//! The address source of a counter paired with the shop's own wallet
//! (`docs/COUNTER-RULES.md`, CR-234, CR-235).
//!
//! Such a counter holds no batch of scripts. It holds the pinned wallet and
//! a [`RangeLedger`], and a sale's address is the wallet's receive address at
//! the next index of the ledger (WL-20, WL-27). The ledger is stored whole in
//! its own slot ([`Slot::Ranges`](crate::Slot)) before an address is shown.

use std::collections::HashSet;

use bitcoin::ScriptBuf;
use openbmt_core::batch::BatchPolicy;
use openbmt_core::identity::DeviceId;
use openbmt_core::pairing_own::{check_range, OwnPairError, RangeLedger};
use openbmt_core::pinned_wallet::{PinnedWallet, RangeError, SignedRange};
use openbmt_core::wire::DecodeError;

use crate::refusal::LoadError;

/// What a counter paired with the shop's own wallet was paired with: the
/// owner root, the wallet, the first range (used only when the slot of
/// ranges is empty, CR-234) and the counter's own smallest range (CR-232).
#[derive(Debug, Clone)]
pub struct OwnPairing {
    /// The owner root pinned at pairing.
    pub owner: DeviceId,
    /// The wallet the addresses come from.
    pub wallet: PinnedWallet,
    /// The first signed range, from the pairing.
    pub first_range: SignedRange,
    /// How many indexes a range must hold; the counter's own number.
    pub batch_policy: BatchPolicy,
}

/// The ledger, the wallet and the scripts handed out so far.
#[derive(Debug, Clone)]
pub(crate) struct OwnSource {
    pub(crate) wallet: PinnedWallet,
    pub(crate) ledger: RangeLedger,
    pub(crate) min_count: u32,
    /// Every script the ledger handed out, in the order of its indexes. They
    /// are watched (CR-234) and are the wallet's own.
    pub(crate) watched: Vec<ScriptBuf>,
    pub(crate) known: HashSet<ScriptBuf>,
}

impl OwnSource {
    /// CR-234: the source for `me`. The ledger is read from the slot's bytes
    /// and every range in it is checked again against the owner root and this
    /// counter; with no slot it is made from the pairing's first range, which
    /// must pass CR-232.
    pub(crate) fn open(
        me: DeviceId,
        pairing: &OwnPairing,
        stored: Option<&[u8]>,
    ) -> Result<Self, LoadError> {
        let min_count = pairing.batch_policy.min_count;
        let ledger = match stored {
            Some(bytes) => {
                RangeLedger::from_bytes(bytes, pairing.owner, me).map_err(LoadError::Ranges)?
            }
            None => {
                check_range(&pairing.first_range, &pairing.owner, &me, min_count)
                    .map_err(|e| LoadError::Ranges(refused_range(&e)))?;
                let mut ledger = RangeLedger::new(pairing.owner, me);
                ledger
                    .accept(&pairing.first_range)
                    .map_err(|e| LoadError::Ranges(refused_range(&OwnPairError::Range(e))))?;
                ledger
            }
        };
        // CR-261: a ledger that switched is for the wallet the counter pins
        // now, or the start is a fault.
        if ledger
            .switched_to()
            .is_some_and(|h| h != pairing.wallet.checksum32())
        {
            return Err(LoadError::Ranges(DecodeError::NonCanonical(
                "the ranges are for another wallet than the pairing's",
            )));
        }
        let mut watched = Vec::new();
        // CR-261: the old wallet's handed-out scripts stay watched from its
        // closed ranges.
        for closed in ledger.closed() {
            let old = ledger
                .closed_wallets()
                .iter()
                .find(|w| w.checksum32() == closed.wallet)
                .ok_or(LoadError::Ranges(DecodeError::NonCanonical(
                    "a closed range has no wallet",
                )))?;
            for index in closed.signed.range.start..closed.next {
                let script = old.receive_script(index).map_err(|_| {
                    LoadError::Ranges(DecodeError::NonCanonical("an index has no address"))
                })?;
                watched.push(script);
            }
        }
        for (range, next) in ledger.taken() {
            for index in range.start..next {
                let script = pairing.wallet.receive_script(index).map_err(|_| {
                    LoadError::Ranges(DecodeError::NonCanonical("an index has no address"))
                })?;
                watched.push(script);
            }
        }
        let known = watched.iter().cloned().collect();
        Ok(Self {
            wallet: pairing.wallet.clone(),
            ledger,
            min_count,
            watched,
            known,
        })
    }

    /// CR-232, CR-235: whether a range may be taken at all, before the
    /// ledger's own overlap rules.
    pub(crate) fn check(&self, signed: &SignedRange, me: &DeviceId) -> Result<(), OwnPairError> {
        check_range(signed, &self.owner(), me, self.min_count)
    }

    fn owner(&self) -> DeviceId {
        self.ledger.owner()
    }

    /// CR-261: this source after a switch to `wallet`, whose first range is
    /// `first`, on a copy. The old wallet's ranges are closed and its
    /// handed-out scripts stay watched; the caller stores the copy and then
    /// puts it in place.
    pub(crate) fn switched(
        &self,
        wallet: &PinnedWallet,
        first: &SignedRange,
    ) -> Result<Self, RangeError> {
        let mut next = self.clone();
        next.ledger
            .switch(&self.wallet, wallet.checksum32(), first)?;
        next.wallet = wallet.clone();
        Ok(next)
    }

    /// Hand out the next script on a copy: the copy, the index and the
    /// script, or `None` when no index is left. The caller stores the copy
    /// and then puts it in place.
    pub(crate) fn issue(&self) -> Option<(Self, u32, ScriptBuf)> {
        let mut next = self.clone();
        let index = match next.ledger.next() {
            Ok(index) => index,
            Err(RangeError::UsedUp) | Err(_) => return None,
        };
        let script = next.wallet.receive_script(index).ok()?;
        next.watched.push(script.clone());
        next.known.insert(script.clone());
        Some((next, index, script))
    }
}

/// A refused range as the decode error a slot that holds it reports.
fn refused_range(e: &OwnPairError) -> DecodeError {
    match e {
        OwnPairError::RangeTooSmall { .. } => {
            DecodeError::NonCanonical("the first range is below the smallest range")
        }
        _ => DecodeError::NonCanonical("the first range is refused"),
    }
}
