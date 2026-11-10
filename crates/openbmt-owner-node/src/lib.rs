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
//! **What it does not do yet.** It watches no script in this slice: the
//! owner's own watching of the shop's payments comes later, and so does
//! everything that takes a screen. The library takes events from the driver
//! and gives bytes back.
//!
//! The [`link`] module carries those bytes to the counters through a relay
//! (`docs/LINK-RULES.md` section 6), and [`copy`] keeps the owner's verified
//! copy of each counter's records, which the counters send back the same way.
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

pub mod copy;
pub mod link;
pub mod watcher;

#[cfg(test)]
mod tests;

pub use copy::{CheckInTaken, CopyError, CounterCopy, Stop, Taken};
pub use link::{Alert, CounterState, OwnerLink, OwnerLinkConfig, OwnerLinkError};
pub use watcher::{OwnerConfig, OwnerStartError, OwnerWatcher, HEARTBEAT_FILE, NOTE_LOG};
