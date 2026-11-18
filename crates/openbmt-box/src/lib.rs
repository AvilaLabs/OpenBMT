//! The OpenBMT Box: the merchant's own small server, first half
//! ([rules](../../../docs/BOX-RULES.md)).
//!
//! - [`state`]: [`BoxCore`], the Box's key, invitation, pinned roots and
//!   settings, and what changes them (BX-01 to BX-24, BX-50, BX-51). No
//!   sockets, no clock: the time and the randomness are passed in.
//! - [`files`]: where the state is stored, whole files written by rename
//!   (BX-50).
//! - [`runner`]: [`BoxRunner`], the state wired to the link's relay and
//!   server, with the Box reading its own mailbox.
//!
//! - [`status_page`]: the optional web page that shows the Box's state and
//!   takes an invitation, on its own listener (BX-54 to BX-61).
//!
//! - [`lightning`]: [`ShopStore`], the second copy of Shop Lightning's state
//!   that the Box keeps for the Lightning counter (BX-90 to BX-95).
//!
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
pub mod lightning;
pub mod runner;
pub mod say;
pub mod state;
pub mod status_page;

#[cfg(test)]
mod tests;

pub use files::{BoxFiles, DirFiles, MemFiles};
pub use lightning::{ShopStore, StoreError};
pub use runner::{
    invitation_text, read_invitation_file, BoxRunner, LocalPost, StartFailure, RELAY_DIR,
};
pub use state::{
    BoxCore, BoxError, BoxStatus, SettingsTaken, INVITATION_FILE, INVITE_SECS, KEY_FILE,
    MAX_MEMBERS, PAIRED_WITH_FILE, ROOTS_FILE, SETTINGS_FILE,
};
pub use status_page::{PageBackend, PageConfig, PageView, StatusPage};
