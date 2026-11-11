//! The OpenBMT counter node as a library the counter app for Android calls.
//!
//! The counter node (`openbmt-counter-node`) joins the counter's judgment of
//! a sale to the network driver and to files on disk. This crate holds no
//! rule of its own. It converts types, keeps the node behind a lock, and
//! turns every state and every refusal into the words a person is shown
//! (`docs/SCREEN-RULES.md`), taken from the crates that make them.
//!
//! - [`dispatch`] is the one entry point: a framed request in, a framed
//!   response out (`src/call.rs`, `bindings/PROTOCOL.md`). A [`Registry`]
//!   holds the running nodes by handle.
//! - On Android the one exported symbol is a JNI function that calls
//!   `dispatch` (`src/jni.rs`, the only file with `unsafe`).
//! - The app calls `pump` from one background thread in a loop and every
//!   other op from its screen thread. [`host`] says how they share the
//!   node and why a screen call never waits longer than a pump slice.
//! - [`pay`] makes the payment request the customer scans (CR-83): the BIP
//!   21 URI of a sale and its QR modules from `openbmt-qr`.
//! - Errors are one enum, [`CounterError`], with a `rule`, a `text` and the
//!   `Words` of what a person is shown. A panic in any call is caught,
//!   becomes `CounterError::Internal` and poisons the node.
//!
//! The data folder is the app's own (`noBackupFilesDir`). It holds the
//! counter's records and sales, so keeping it out of backups and device
//! transfer, and protecting the identity secret the app hands over at
//! every start, are the app's job and are not done here. The folder's lock
//! (NR-79) applies as for any other start of the node.
//!
//! This crate has had internal review only. No independent security audit
//! has been done yet. Not for real funds.

// The one `unsafe` of this crate is the JNI glue; a test checks that no
// other file uses it.
#![deny(unsafe_code)]
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

mod call;
mod codec;
mod error;
pub mod host;
#[cfg(any(target_os = "android", feature = "jni"))]
mod jni;
pub mod pay;
mod say;
mod views;

#[cfg(test)]
mod tests;

pub use call::{
    dispatch, error_response, protocol_description, Registry, MAX_NODES, REQUEST_MAGIC,
    RESPONSE_MAGIC, VERSION,
};
pub use error::{CounterError, Kind};
pub use host::{Entry, MAX_WAIT_MS, SLICE_MS};
pub use pay::{btc_amount, request_texts, Payment};
pub use say::{all_words, words_for};
pub use views::{
    ActorRole, ActorView, CashArgs, CounterNetwork, HealthView, HeartbeatOutcome, OpenArgs,
    OpenedView, OverrideArgs, PumpView, RefundArgs, RefundAskView, RefundRow, RefundStateView,
    ReportView, RequoteArgs, ScreenView, StartConfig, StateView, StatusView, WithdrawArgs,
    WordsView,
};
