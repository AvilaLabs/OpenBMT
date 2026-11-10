//! A sale's facts from the owner's own view (OC-14).
//!
//! The owner's limit on a refund rests on what the owner's checked view of
//! the chain shows paid to the sale's address, and on the address ranges the
//! owner signed. It does not rest on what a counter says it received: a
//! counter that is taken over can sign a record that claims any amount.

use std::fmt;

use openbmt_owner::{Owner, SaleFacts, SaleKey};

use crate::coins::{CoinView, ReceiptState};

/// Why no facts were given for a sale (OC-14).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FactsRefused {
    /// The view is not ready, or has not checked the chain up to the tip.
    /// This is not a refusal of the request: it waits and is tried again.
    ViewNotReady,
    /// The view's receipt log is full (OC-13), so it cannot say what a sale
    /// received. The request waits like one for a view that is not ready,
    /// until the view is reset and rescanned.
    ReceiptLogFull,
    /// The sale's index was never issued to the counter that names it. The
    /// request is refused.
    NotThisCountersAddress,
}

impl FactsRefused {
    /// Whether the request waits and is tried again, instead of being
    /// refused.
    pub fn waits(self) -> bool {
        matches!(self, Self::ViewNotReady | Self::ReceiptLogFull)
    }
}

impl fmt::Display for FactsRefused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ViewNotReady => write!(
                f,
                "OC-14: the wallet's view has not checked the chain up to the tip yet"
            ),
            Self::ReceiptLogFull => write!(
                f,
                "OC-13: the wallet's receipt log is full, so it cannot say what a sale received"
            ),
            Self::NotThisCountersAddress => write!(
                f,
                "OC-14: the owner never gave this counter the address of the sale"
            ),
        }
    }
}

impl std::error::Error for FactsRefused {}

/// OC-14: the facts of a sale from the owner's own view.
///
/// The view must be ready and have checked the chain up to `tip`, and its
/// receipt log must not be full. The sale's index must lie in a range the
/// owner issued to the sale's counter. The amount received is the sum of the
/// receipts at the index that are in a block at or below `tip`, spent or
/// not. The sale has reached the refund depth when something was received
/// and every one of those receipts has at least `refund_depth`
/// confirmations (the tip block counts as one).
pub fn verified_facts(
    view: &CoinView,
    owner: &Owner,
    sale: SaleKey,
    tip: u32,
    refund_depth: u32,
) -> Result<SaleFacts, FactsRefused> {
    if !view.ready() || view.checked() < tip {
        return Err(FactsRefused::ViewNotReady);
    }
    if view.receipt_state() == ReceiptState::LogFull {
        return Err(FactsRefused::ReceiptLogFull);
    }
    if owner.counter_of(sale.index) != Some(sale.counter) {
        return Err(FactsRefused::NotThisCountersAddress);
    }
    let mut received: u64 = 0;
    let mut deep = true;
    for r in view
        .receipts_at(sale.index)
        .iter()
        .filter(|r| r.height <= tip)
    {
        received = received.saturating_add(r.sats);
        let depth = tip - r.height + 1;
        deep &= depth >= refund_depth;
    }
    Ok(SaleFacts {
        received_sats: received,
        reached_refund_depth: received > 0 && deep,
    })
}
