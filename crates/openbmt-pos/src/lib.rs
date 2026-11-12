//! The vendor-neutral POS side of OpenBMT: what a shop's point-of-sale
//! system says about its orders, the matching of every Bitcoin tender to a
//! sale the counter rang, and the alerts for what does not match.
//!
//! The rules are in `docs/POS-RULES.md`; each has an ID (PS-01 and on), and
//! the tests name the rule they prove. The flow they describe works on every
//! POS and needs only reading from it: the cashier rings up the order, the
//! customer pays at the counter, the cashier closes the order with the shop's
//! "Bitcoin" tender, and the owner's device reads the orders and matches
//! each Bitcoin tender to a counter sale.
//!
//! It has no network, no clock, no file access, no randomness and no vendor
//! code. The caller passes in the orders (through a [`PosReader`]), the
//! counter's sales and the time in seconds, and hands every change to a
//! `persist` closure before it takes effect.
//!
//! The parts are:
//!
//! 1. [`model`]: orders, tenders and the bounded text they hold.
//! 2. [`settings`]: what the shop decides (tender names, locations, timing).
//! 3. [`reference`]: the sale reference and the idempotency key.
//! 4. [`facts`]: the counter's sales as the matcher needs them, folded from
//!    the counter's signed events.
//! 5. [`alerts`]: the alerts, each with its why and its next step.
//! 6. [`matcher`]: the pure matching and the alert rules.
//! 7. [`ledger`]: what is kept between polls and its stored form (`OBMZ`).
//! 8. [`connector`]: the traits a vendor adds, a test double and [`sync`].
//! 9. [`notice`]: the counter's signed paid notice, the Box's signed POS
//!    notice, and the sale facts built from paid notices (BX-40, BX-42).
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

pub mod alerts;
pub mod connector;
pub mod facts;
pub mod ledger;
pub mod matcher;
pub mod model;
pub mod notice;
pub mod reference;
pub mod say;
pub mod settings;
pub mod till;

#[cfg(test)]
mod tests;

pub use alerts::{Alert, AlertKey, AlertKind, Subject, TenderKey};
pub use connector::{
    sync, CloseOutcome, CloseRefused, CloseRequest, Connector, Cursor, MemoryCloser, MemoryPos,
    Page, PosCloser, PosError, PosReader, SyncError, SyncFailed, SyncReport,
};
pub use facts::{facts_from_chain, FactFold, FactsError, SaleFact, SaleKey};
pub use ledger::{
    Acknowledged, Blocker, BooksLine, Delta, Ledger, LedgerError, LedgerWriteError, Summary,
};
pub use matcher::{Match, MatchHow};
pub use model::{
    Id, ModelError, OrderId, OrderState, PosOrder, PosTender, TenderId, TenderKind, TenderState,
    Text,
};
pub use notice::{
    facts_from_notices, NoticeDraft, NoticeMaker, NoticeState, PaidNotice, PosItem, PosNotice,
    SaleMismatch, MAGIC_PAID_NOTICE, MAGIC_POS_NOTICE, PAID_NOTICE_LEN, PAID_NOTICE_TILL_LEN,
};
pub use reference::{find_reference, IdempotencyKey, RefCode};
pub use settings::{LocationCounters, Settings, SettingsError, Timing};
pub use till::{
    reference_ok, SaleCancel, SaleRequest, TillMessage, MAGIC_TILL_SALE, MAX_TILL_REFERENCE,
};

/// PS-01: longest order or tender id, in bytes.
pub const MAX_ID_BYTES: usize = 64;
/// PS-03: longest text (a location, a tender name, a note), in bytes.
pub const MAX_TEXT_BYTES: usize = 64;
/// PS-02: most tenders on one order.
pub const MAX_TENDERS: usize = 16;
/// PS-72: longest POS cursor, in bytes.
pub const MAX_CURSOR_BYTES: usize = 1_024;
/// PS-80: most orders on one page.
pub const MAX_PAGE_ORDERS: usize = 500;
/// PS-80: most pages one sync pulls.
pub const MAX_PAGES: usize = 64;
/// PS-72: most orders the ledger tracks.
pub const MAX_TRACKED_ORDERS: usize = 4_096;
/// PS-72: most matches the ledger keeps.
pub const MAX_MATCHES: usize = 16_384;
/// PS-72: most alerts the ledger keeps, open and acknowledged each.
pub const MAX_ALERTS: usize = 4_096;
/// PS-72: most counter sales handed to the matcher at once.
pub const MAX_SALES: usize = 16_384;
/// PS-14: most locations in the settings.
pub const MAX_LOCATIONS: usize = 32;
/// PS-14: most counters at one location.
pub const MAX_COUNTERS_PER_LOCATION: usize = 16;
/// PS-14: most counters in no-POS mode.
pub const MAX_NO_POS: usize = 64;
/// PS-10: most Bitcoin tender names in the settings.
pub const MAX_TENDER_NAMES: usize = 8;
/// PS-13: most currencies in the settings.
pub const MAX_CURRENCIES: usize = 4;
/// PS-33: seconds after a sale was first reported paid within which a tender
/// may still match it **[to tune]**.
pub const MATCH_WINDOW_SECS: u64 = 1_800;
/// PS-33: seconds after opening that a sale never reported paid can be
/// matched, before the match window is added **[to tune]**. It mirrors the
/// counter's expiry (CR-07).
pub const NEVER_PAID_SPAN_SECS: u64 = 900;
/// PS-40, PS-42: seconds a tender or a paid sale may wait for the other side
/// before it is an alert **[to tune]**.
pub const GRACE_SECS: u64 = 600;
/// PS-41: seconds by which a tender may precede the sale's paid time before
/// it is an alert **[to tune]**.
pub const CLOSED_BEFORE_PAID_SECS: u64 = 120;
/// PS-08: seconds before a sale opened that a tender may be timed and still
/// match it, for the difference between the POS's clock and the counter's
/// **[to tune]**.
pub const CLOCK_SLACK_SECS: u64 = 120;
/// PS-76: most blocking items a `Full` refusal names.
pub const MAX_NAMED_BLOCKERS: usize = 10;
/// PS-73: seconds an order must stay unchanged before its settled matches
/// retire **[to tune]**.
pub const RETIRE_SECS: u64 = 7 * 86_400;
