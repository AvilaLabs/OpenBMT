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
//! The [`requests`] module joins the payment-request library to the node
//! (`docs/REQUEST-RULES.md`, RQ-01 to RQ-37): the request book in the node's
//! folder, the desk's addresses in the live watch, the payments credited from
//! the blocks the coin view applies at the shop's confirmation depth, the
//! blocks that leave the chain, and the returns on the owner's refund queue.
//!
//! The [`paid_log`] module keeps what the owner paid out, a refund or a
//! cash-out at a time, with its value in the shop's currency, for the books
//! (`docs/OWNER-COINS-RULES.md`, OC-48 to OC-51).
//!
//! The [`price`] module is the owner phone's own ask of the price sources
//! (`docs/OWNER-APP-RULES.md` OA-101, on `docs/RATE-RULES.md`): the app
//! fetches the replies, the library reads them and agrees on one price.
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

pub mod batch_outbox;
pub mod coins;
pub mod copy;
pub mod deep;
pub mod facts;
pub mod link;
pub mod live;
pub mod node;
pub mod own;
pub mod paid_log;
pub mod pos;
pub mod price;
pub mod range_outbox;
pub mod receipts;
pub mod refunds;
pub mod requests;
pub mod say;
pub mod settings_roster;
pub mod settle_owed;
pub mod switch_outbox;
mod take_journal;
pub mod watch;
pub mod watcher;
pub mod web_orders;
pub mod web_visit;

#[cfg(test)]
mod tests;

pub use batch_outbox::{BatchOutbox, BatchOutboxError, PendingBatch, MAX_PENDING_BATCHES};
pub use copy::{CheckInTaken, CopyError, CounterCopy, Stop, Taken};
pub use facts::{verified_facts, FactsRefused};
pub use link::{
    Alert, BatchesWaiting, BoxReport, CounterState, IndexUse, OwnerLink, OwnerLinkConfig,
    OwnerLinkError, RangesWaiting, SettingsNotCarried, SettingsWaiting, BATCH_ALERT_SECS,
    BATCH_RESEND_SECS, MAX_BOX_REPORTS, MAX_CARRIED_SETTINGS,
};
pub use live::{
    DeepLine, Spans, WatchLimits, DEEP_CHECK_SECS, DEEP_RETRY_SECS, DEEP_STALL_SECS,
    OWNER_LIVE_WATCH, QUIET_SECS,
};
pub use node::{
    FloatCoins, FloatParts, FormerHeld, FormerParts, NodeError, OwnerNode, OwnerParts, OwnerPort,
    SendRefused, SendState, BROADCAST_DIR, COINS_FILE, NO_TOR, QUEUE_FILE, TAKE_JOURNAL_FILE,
};
pub use own::{
    former_coins_file, own_approver, NotWatching, OwnNode, OwnNodeError, OwnTick, ToppedUp,
    WaitingStep, WaitingView, Watching, COINS_OF_FILE, OWN_STATE_FILE, OWN_WAITING_FILE,
};
pub use paid_log::{
    standing, value_at, PaidEntry, PaidLog, PaidValue, PaidWhat, Undone, MAGIC_PAID_LOG,
    MAX_PAID_ENTRIES, PAID_ENTRY_LEN, PAID_LOG_FILE,
};
pub use pos::{
    counter_facts, OwnerPos, PosMethod, PosPolled, PosRefused, PosRequest, PosSetup, PosVendor,
    PosView, POS_LEDGER_FILE, POS_SETUP_FILE,
};
pub use price::{agreed_price, AgreedPrice, PriceReply};
pub use range_outbox::{PendingRange, RangeOutbox, RangeOutboxError, MAX_PENDING_RANGES};
pub use receipts::{MergeOffer, MergeRefused, ReceiptsLine, FEE_FILE, MERGE_MIN_COINS};
pub use refunds::{
    Applied, CashRefundSeen, Evidence, IntakeError, NoteKind, RefundIntake, RefundNote, Stopped,
};
pub use requests::{
    return_label, MadeRequest, PendingArrival, RequestContext, RequestError, RequestsOff,
    ReturnAdded, Shared, MAX_ARRIVALS, REQUEST_ARRIVALS_FILE, REQUEST_BOOK_FILE,
};
pub use settings_roster::{RosterEntry, RosterError, SettingsRoster};
pub use settle_owed::{
    Owed, OwedError, OwedKind, SettleOwed, MAGIC_SETTLE_OWED, MAX_OWED, SETTLE_OWED_FILE,
};
pub use switch_outbox::{
    PendingSwitch, SwitchOutbox, SwitchOutboxError, MAX_PENDING_SWITCHES, SWITCH_OUTBOX_FILE,
};
pub use watch::{
    WatchConfig, WatchLink, WatchLinkConfig, WatchNode, WatchStartError, WatchState, WATCH_LINK_DIR,
};
pub use watcher::{OwnerConfig, OwnerStartError, OwnerWatcher, HEARTBEAT_FILE, NOTE_LOG};
pub use web_orders::{
    check_store_url, PairState, RefusedOrder, RefusedWhy, WebError, LOW_ADDRESSES,
    MAGIC_WEB_ORDERS, MAX_ORDERS, MAX_RANGES, MAX_REFUSALS, WEB_ORDERS_FILE,
};
pub use web_visit::{
    trouble_words, OrderStand, WebCall, WebCallKind, WebFailure, WebPairShown, WebStateInfo,
    WebStep, BACKOFF_FIRST_SECS, BACKOFF_MAX_SECS,
};
