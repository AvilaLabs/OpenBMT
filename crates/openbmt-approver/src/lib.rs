//! The OpenBMT Key app's side of the approval session.
//!
//! The rules are in `docs/SESSION-RULES.md`, section 2 (AS-20 to AS-38);
//! the refusals and the tests name the rule they belong to. The library has
//! no network, no clock of its own and no screen. The caller passes the time
//! in seconds and a `persist` step that stores the new state before the
//! response bytes are returned.
//!
//! The steps are:
//!
//! 1. [`Approver::new`] (and [`Approver::attach_signer`] when this device
//!    holds key 2) sets up the Key app, or [`Approver::restore`] brings its
//!    stored state back.
//! 2. [`Approver::open`] reads a request and checks it. It stores and releases
//!    nothing.
//! 3. [`Session::lines`] shows what the Key app's own checks say about each
//!    item, and [`Session::decide`] takes the person's decision on each.
//! 4. [`Session::finish`] signs the approved lines, stores all new state
//!    through `persist`, and only then returns the response bytes.
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

mod describe;
mod error;
mod evaluate;
mod line;
mod say;
mod session;
mod state;

pub use error::{DecideError, FinishError, RefuseSession, RestoreError, SetupError, StateError};
pub use line::{
    BatchLine, CheckStep, Choice, Detail, HealthLine, Line, OutputLine, PairingLine, SettingChange,
    SettingsLine, SpendLine, SpendReview, StepStatus,
};
pub use say::{all_words, no_key2_words, receive_range_words, RECEIVE_MARK_REFUSED};
pub use session::Session;
pub use state::Approver;

/// AS-22: how many finished session IDs the Key app remembers.
pub const MAX_SESSION_LOG: usize = 256;
/// AS-38: the magic of the Key app's stored state.
pub const MAGIC_APPROVER_STATE: [u8; 4] = *b"OBM3";
