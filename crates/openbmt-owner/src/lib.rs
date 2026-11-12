//! The OpenBMT owner phone's side of the approval session.
//!
//! The rules are in `docs/SESSION-RULES.md`, section 3 (AS-50 to AS-77);
//! the refusals and the tests name the rule they belong to. The library has
//! no network, no clock of its own, no randomness of its own and no screen.
//! The caller passes the time in seconds, the random bytes, the coins and
//! the sales' facts, and a `persist` step that stores new state before it
//! takes effect.
//!
//! The steps are:
//!
//! 1. [`Owner::new`] sets up an empty queue, or [`Owner::restore`] brings a
//!    stored one back.
//! 2. [`Owner::set_sale_facts`], [`Owner::queue_refund`],
//!    [`Owner::queue_cash_out`], [`Owner::queue_batch`],
//!    [`Owner::queue_settings`] and [`Owner::queue_pairing`] fill the queue.
//!    [`Owner::queue_counter_request`] and [`Owner::queue_counter`] do it for
//!    a counter that is being added, and [`Owner::counter_bundle`] makes what
//!    the counter takes at the end.
//! 3. [`Owner::build_request`] freezes what is ready into one session and
//!    returns the request bytes for the Key app.
//! 4. [`Owner::take_response`] checks the Key app's answer against the
//!    request, adds key 1 to each approved transaction, finishes it, and
//!    stores the results before it returns them.
//! 5. [`Owner::mark_broadcast`], [`Owner::mark_confirmed`],
//!    [`Owner::mark_dropped`] and [`Owner::mark_delivered`] follow what the
//!    caller does with the finished objects.
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

mod boxes;
mod build;
pub mod ceremony;
mod codec;
mod counter;
mod edit;
mod error;
mod follow;
pub mod hardware;
pub mod say;
mod state;
mod take;
mod types;

pub use boxes::{BoxHandover, BoxQueued, BoxRequest, PendingBox, WaitingBox};
pub use counter::{
    equal_shares, BundleParts, CounterBundleMade, CounterQueued, CounterRequest, WaitingCounter,
};
pub use edit::{
    parse_lightning_node, price_band, sats_per_usd_at, LightningEdit, SettingsEdit,
    SettingsPreview, WatchEdit, MAX_EDIT_CONFIRMATIONS, MAX_USD_PER_BTC, MIN_USD_PER_BTC,
};
pub use error::{BoxGap, BundleGap, NodeKeyFault, OwnerError, Refusal, RestoreError, SetupError};
pub use state::Owner;
pub use types::{
    Built, CashAmount, CashOut, Deliverable, Delivery, FeePayer, IssuedRange, ItemOutcome, Options,
    QueuedBatch, QueuedBoxClaim, QueuedPairing, QueuedSettings, RateNote, Refund, RefundBooks,
    RefundPacking, SaleFacts, SaleKey, SettingsQueued, Skipped, Stage, Taken,
};

/// AS-65: the magic of the owner phone's stored queue.
pub const MAGIC_OWNER_QUEUE: [u8; 4] = *b"OBM4";
/// AS-50: most refunds in the queue.
pub const MAX_REFUNDS: usize = 256;
/// AS-50: most cash-outs in the queue.
pub const MAX_CASH_OUTS: usize = 16;
/// AS-50: most batches in the queue.
pub const MAX_BATCHES: usize = 16;
/// AS-50: most settings changes in the queue.
pub const MAX_SETTINGS: usize = 16;
/// AS-50: most pairings in the queue.
pub const MAX_PAIRINGS: usize = 16;
/// AS-50: most Box claims in the queue.
pub const MAX_BOX_CLAIMS: usize = 4;
/// AS-50: most sales whose facts are kept.
pub const MAX_SALES: usize = 1024;
/// OC-11, AS-50: most address ranges kept as issued.
pub const MAX_ISSUED_RANGES: usize = 65_536;
/// AS-50: most open sessions.
pub const MAX_OPEN_SESSIONS: usize = 8;
/// AS-50: most finished objects waiting to be delivered.
pub const MAX_OUTBOX: usize = 256;
/// AS-50: most refunds in one transaction.
pub const MAX_REFUNDS_PER_TX: usize = 99;
/// AS-50: most transactions in one session.
pub const MAX_TXS_PER_SESSION: usize = 16;
/// AS-68, AS-50: most outpoints kept with a refund or a cash-out from
/// transactions that were dropped.
pub const MAX_CONFLICT_OUTPOINTS: usize = 256;
/// AS-04, AS-50: most items in one session.
pub const MAX_ITEMS_PER_SESSION: usize = openbmt_session::MAX_ITEMS;
/// AS-02, AS-50: the largest request an open session keeps.
pub const MAX_SESSION_REQUEST: usize = openbmt_session::MAX_SESSION_BYTES;
/// AS-65, AS-50: the largest snapshot: eight requests of 8 MiB and room for
/// the queue.
pub const MAX_SNAPSHOT_BYTES: usize = 80 * 1024 * 1024;
