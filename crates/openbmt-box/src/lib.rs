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
//! - [`pos`]: [`BoxPos`], the POS on the Box: the counters' paid notices, the
//!   timer that reads the POS through a connector, the matching, the reports
//!   to the owner and the closing of orders (BX-40 to BX-43).
//!
//! - [`https`], [`pos_config`] and [`vendor`]: how the Box reaches a POS
//!   vendor: the system's `curl` as a child process, the Box's own file that
//!   names the vendor, and the vendor's reader behind the connector (BX-44
//!   to BX-49).
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
pub mod https;
pub mod pos;
pub mod pos_config;
pub mod runner;
pub mod say;
pub mod state;
pub mod vendor;

#[cfg(test)]
mod tests;

pub use files::{BoxFiles, DirFiles, MemFiles};
pub use https::{CurlRunner, HttpsRefusal, HttpsReply, HttpsRequest, HttpsRunner, Transport};
pub use pos::{
    BoxPos, PosOpenError, PosSetup, PosState, Reading, LEDGER_FILE, NOTICES_FILE, POLL_SECS,
};
pub use pos_config::{PosConfig, PosConfigError, Vendor, POS_CONFIG_FILE};
pub use runner::{read_invitation_file, BoxRunner, LocalPost, PosView, StartFailure, RELAY_DIR};
pub use state::{
    BoxCore, BoxError, BoxStatus, SettingsTaken, INVITATION_FILE, INVITE_SECS, KEY_FILE,
    MAX_MEMBERS, MAX_TOKEN_BYTES, POS_TOKEN_FILE, ROOTS_FILE, SETTINGS_FILE,
};
pub use vendor::VendorConnector;
