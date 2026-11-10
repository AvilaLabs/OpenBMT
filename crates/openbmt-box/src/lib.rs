//! The OpenBMT Box: the merchant's own small server, first half
//! ([rules](../../../docs/BOX-RULES.md), sections 1, 2, 3 and 5).
//!
//! - [`state`]: [`BoxCore`], the Box's key, invitation, pinned roots and
//!   settings, and what changes them (BX-01 to BX-24, BX-50, BX-51). No
//!   sockets, no clock: the time and the randomness are passed in.
//! - [`files`]: where the state is stored, whole files written by rename
//!   (BX-50).
//! - [`runner`]: [`BoxRunner`], the state wired to the link's relay and
//!   server, with the Box reading its own mailbox.
//!
//! Section 4 of the rules (the POS on the Box) is not in this crate yet.
//! The Box holds no bitcoin key and cannot move funds (BX-04); what it says
//! is a hint to the other devices.
//!
//! Where the code is more exact than the rules, or adds to them, the rules
//! file says so in the paragraph "What the code fixes".
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

pub mod files;
pub mod runner;
pub mod state;

#[cfg(test)]
mod tests;

pub use files::{BoxFiles, DirFiles, MemFiles};
pub use runner::{read_invitation_file, BoxRunner, LocalPost, StartFailure, RELAY_DIR};
pub use state::{
    BoxCore, BoxError, BoxStatus, SettingsTaken, INVITATION_FILE, INVITE_SECS, KEY_FILE,
    MAX_MEMBERS, POS_TOKEN_FILE, ROOTS_FILE, SETTINGS_FILE,
};
