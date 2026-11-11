//! The owner's watcher without a screen.
//!
//! The owner's device runs a watcher of its own, so that a counter that is
//! cut off, or lied to, cannot hide a problem from the shop (`docs/DESIGN.md`
//! §5.2). [`OwnerWatcher`] is that watcher's core: a network driver with its
//! own data directory, a [`ChainView`] of the best header chain, the real
//! health rules (WR-50 to WR-53) run on every `Peers` event, and the signed
//! heartbeat that tells a counter what the watcher sees
//! (`openbmt_core::heartbeat`, `docs/COUNTER-RULES.md` CR-50 to CR-56).
//!
//! [`OwnerNode`] joins that watcher to the wallet's checked coin view,
//! approval queue and sending path. The [`say`] module supplies words for
//! coin readiness, sending and recovery states. Graphical applications and
//! device testing are still to come.
//!
//! The [`refunds`] module turns the refund requests that arrive in those
//! records into queued refunds on the owner's queue (`docs/COUNTER-RULES.md`
//! CR-60 to CR-74).
//!
//! The [`link`] module carries those bytes to the counters through a relay
//! (`docs/LINK-RULES.md` section 6), and [`copy`] keeps the owner's verified
//! copy of each counter's records, which the counters send back the same way.
//!
//! The [`pos`] module is the owner's own matching of the till's orders to
//! the counters' sales (`docs/OWNER-COINS-RULES.md`, OC-30 to OC-37): the
//! app sends the requests and hands the answers in, and the sales come from
//! the verified copies of the counters' records.
//!
//! The view of the chain is the counter node's [`ChainView`], and the driver
//! is reached through the counter node's [`DriverPort`], so that the two
//! nodes read the chain the same way. This crate therefore depends on
//! `openbmt-counter-node`.
//!
//! A heartbeat is a claim by the same software family on another device. It
//! guards against a dead or cut-off owner watcher, not against a bad build.
//!
//! This crate has had internal review only. No independent security audit
//! has been done yet.

#![forbid(unsafe_code)]
#![deny(missing_docs)]
#![cfg_attr(
    not(test),
    deny(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing
    )
)]

pub mod coins;
pub mod copy;
pub mod deep;
pub mod facts;
pub mod link;
pub mod live;
pub mod node;
pub mod pos;
pub mod receipts;
pub mod refunds;
pub mod say;
mod take_journal;
pub mod watcher;

#[cfg(test)]
mod tests;

pub use copy::{CheckInTaken, CopyError, CounterCopy, Stop, Taken};
pub use facts::{verified_facts, FactsRefused};
pub use link::{
    Alert, BoxReport, CounterState, OwnerLink, OwnerLinkConfig, OwnerLinkError, MAX_BOX_REPORTS,
};
pub use live::{
    DeepLine, Spans, WatchLimits, DEEP_CHECK_SECS, DEEP_RETRY_SECS, DEEP_STALL_SECS,
    OWNER_LIVE_WATCH, QUIET_SECS,
};
pub use node::{
    NodeError, OwnerNode, OwnerParts, OwnerPort, SendRefused, SendState, BROADCAST_DIR, COINS_FILE,
    NO_TOR, QUEUE_FILE, TAKE_JOURNAL_FILE,
};
pub use pos::{
    counter_facts, OwnerPos, PosMethod, PosPolled, PosRefused, PosRequest, PosSetup, PosVendor, PosView,
    POS_LEDGER_FILE, POS_SETUP_FILE,
};
pub use receipts::{MergeOffer, MergeRefused, ReceiptsLine, FEE_FILE, MERGE_MIN_COINS};
pub use refunds::{
    Applied, CashRefundSeen, Evidence, IntakeError, NoteKind, RefundIntake, RefundNote, Stopped,
};
pub use watcher::{OwnerConfig, OwnerStartError, OwnerWatcher, HEARTBEAT_FILE, NOTE_LOG};
