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
//! - [`rates`] converts the price of bitcoin: the table of sources, the
//!   reading of one reply and one rate made from samples. The app fetches
//!   (`docs/RATE-RULES.md`, RT-40 to RT-45); the library decides.
//! - [`shop`] reads the pairing record before the node starts (the network,
//!   Tor on or off, the smallest sale); `start` checks the signatures. It
//!   also reads the own node's address a manager typed (`own_node_check`,
//!   CR-100 to CR-102).
//! - The pairing ceremony (`pair_begin`, `pair_match`, `pair_differ`,
//!   `pair_cancel`, `pair_finish`) runs in the [`Registry`]: one ceremony
//!   at a time, in memory only, with the counter's key and number drawn from
//!   the system's random source here (CR-91 to CR-96). `pair_finish` returns
//!   the pairing record the app keeps and hands back at every `start`.
//! - The link to the owner (`link_step`, `link_state`, `take_relay`,
//!   CR-130 to CR-136) runs on the relay the pairing record holds. The app
//!   calls `link_step` from a thread of its own; the node is held only while
//!   the step hands what came to it, never while the relay is called.
//! - Errors are one enum, [`CounterError`], with a `rule`, a `text` and the
//!   `Words` of what a person is shown. A panic in any call is caught,
//!   becomes `CounterError::Internal` and poisons the node.
//!
//! The data folder is the app's own (`noBackupFilesDir`). It holds the
//! counter's records and sales, so keeping it out of backups and device
//! transfer, and sealing the pairing record (it holds the identity secret)
//! that the app hands over at every start, are the app's job and are not
//! done here. The folder's lock
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
mod pair;
pub mod pay;
pub mod rates;
mod relay;
mod say;
pub mod shop;
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
pub use rates::{RateCombineArgs, RateParseArgs, RateSourceView, RateView, SampleView};
pub use say::{all_words, words_for};
pub use shop::SettingsView;
pub use views::{
    ActorRole, ActorView, CashArgs, CounterNetwork, HealthView, HeartbeatOutcome, OpenArgs,
    LinkStepView, LinkView, OpenedView, OverrideArgs, OwnNodeCheckArgs, OwnNodeView,
    PairBeginArgs, PairStartedView, PairingView, PumpView, RefundArgs, RefundAskView, RefundRow,
    RefundStateView, RelayKind, ReportView, RequoteArgs, ScreenView, StartConfig, StateView,
    StatusView, WithdrawArgs, WordsView,
};
