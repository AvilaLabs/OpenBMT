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
//! - [`status_page`]: the optional web page that shows the Box's state and
//!   takes an invitation, on its own listener (BX-54 to BX-61).
//!
//! - [`till`] and [`till_listener`]: the optional listener that takes sale
//!   requests from a till, the till key, and the sales it keeps in memory
//!   (BX-70 to BX-79).
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
pub mod https;
pub mod lightning;
pub mod pos;
pub mod pos_config;
pub mod runner;
pub mod say;
pub mod state;
pub mod status_page;
pub mod till;
pub mod till_listener;
pub mod vendor;

#[cfg(test)]
mod tests;

pub use files::{BoxFiles, DirFiles, MemFiles};
pub use https::{CurlRunner, HttpsRefusal, HttpsReply, HttpsRequest, HttpsRunner, Transport};
pub use lightning::{ShopStore, StoreError};
pub use pos::{
    BoxPos, PosOpenError, PosSetup, PosState, Reading, LEDGER_FILE, NOTICES_FILE, POLL_SECS,
};
pub use pos_config::{PosConfig, PosConfigError, Vendor, POS_CONFIG_FILE};
pub use runner::{
    invitation_text, read_invitation_file, BoxRunner, LocalPost, PosView, StartFailure, RELAY_DIR,
};
pub use state::{
    BoxCore, BoxError, BoxStatus, SettingsTaken, INVITATION_FILE, INVITE_SECS, KEY_FILE,
    MAX_MEMBERS, MAX_TOKEN_BYTES, PAIRED_WITH_FILE, POS_TOKEN_FILE, ROOTS_FILE, SETTINGS_FILE,
};
pub use status_page::{PageBackend, PageConfig, PageView, StatusPage};
pub use till::{make_till_key, SaleState, TillRefusal, TillTable, TILL_KEY_FILE};
pub use till_listener::{TillBackend, TillConfig, TillListener};
pub use vendor::VendorConnector;
