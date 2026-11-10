//! The shop's messages over the link ([rules](../../../docs/LINK-RULES.md),
//! section 6).
//!
//! - [`courier`]: [`Courier`], one device's end of the link: it numbers,
//!   seals and sends, and polls, opens and acknowledges (LK-50 to LK-53). It
//!   reaches the relay through the [`Post`] trait; [`RelayPost`] is the real
//!   one, on `openbmt_link::Client`, and [`memory`] has a relay in memory for
//!   tests.
//! - [`wire`]: the contents of the Records and Records received kinds
//!   (LK-54, LK-55).
//!
//! The two nodes use this crate for the rest of section 6: the counter node's
//! `link` module sends the chain, check-ins and receives heartbeats, commands
//! and batches; the owner node's `link` module keeps a verified copy of each
//! counter's chain.
//!
//! The courier gives no authority (LK-09). It hands over bytes; the call that
//! takes them checks them.
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

pub mod courier;
pub mod memory;
pub mod wire;

#[cfg(test)]
mod tests;

pub use courier::{
    Courier, CourierConfig, FirstStart, PollError, Post, Received, RelayPost, SendError,
    StartError, StoreError, INBOX_FILE, MAX_ROUNDS, NOTE_LOG, OUTBOX_FILE,
};
pub use memory::{MemoryPost, MemoryRelay};
pub use wire::{
    decode_received, decode_records, head_at, received_content, records_content, WireError,
    RECEIVED_LEN,
};
