//! The OpenBMT counter without a screen: the counter's judgment of a sale
//! joined to the network driver and to files on a disk.
//!
//! The pieces of the counter's receive path are separate crates. This one
//! joins them and adds no rule of its own:
//!
//! 1. [`dirstore`]: [`DirStore`], the counter's [`Store`](openbmt_counter::Store)
//!    on a directory.
//! 2. [`chainview`]: [`ChainView`], the app's copy of the best header chain,
//!    kept up to date from the driver's events.
//! 3. [`node`]: [`Node`], which owns a counter, a driver and the two above,
//!    applies the driver's events, and answers the cashier's calls.
//! 4. [`link`]: [`CounterLink`], which carries the owner's heartbeat, commands
//!    and batches to the node and the counter's check-ins and records to the
//!    owner, through a relay (`docs/LINK-RULES.md` section 6).
//! 5. [`notices`]: [`NoticeOutbox`], the paid notices the link sends to a
//!    Box (`docs/BOX-RULES.md` section 4).
//!
//! The rules the node follows are `docs/COUNTER-RULES.md` and, for what the
//! driver does, `docs/NETWORK-RULES.md` (NR-44, NR-53, NR-75 above all). The
//! way the node starts, what it does with each event and what it does not do
//! yet are in the docs of [`node`].
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

pub mod chainview;
pub mod dirstore;
pub mod link;
pub mod node;
pub mod notices;
pub mod say;

#[cfg(test)]
mod tests;

pub use chainview::ChainView;
pub use dirstore::{DirStore, DirStoreError, Loaded, TornTail};
pub use link::{CounterLink, CounterLinkConfig, LinkState};
pub use node::{
    DriverPort, HealthFn, HeartbeatNotTaken, HeartbeatTaken, Launch, Node, NodeConfig, StartError,
    MAX_EVENTS_PER_PUMP, NOTE_LOG, SETTLE_EVERY_SECS, SETTLE_MIN_GAP_SECS, TX_CACHE,
};
pub use notices::{NoticeOutbox, NoticeStoreError};
