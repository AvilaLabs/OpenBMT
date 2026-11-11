//! Lightning receipts through reverse swaps (`docs/SWAP-RULES.md`).
//!
//! A swap service makes the invoice the customer pays and locks its own
//! bitcoin in a Taproot output the shop can claim with a preimage and a
//! claim key. The claim pays the shop's own wallet. This library has no
//! network, no clock and no randomness: the caller passes the replies, the
//! time, the chain height and the random bytes.
//!
//! - [`Request`]: the request for a swap (SW-01 to SW-04);
//! - [`Reply`] and [`check`]: the reply, read strictly, and the check that
//!   decides whether to go on (SW-10 to SW-28);
//! - [`check_lockup`]: the service's lockup transaction (SW-30, SW-31);
//! - [`build_claim`]: the signed, verified claim (SW-40 to SW-45);
//! - [`SwapState`]: the state file (SW-50 to SW-55).
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

pub mod claim;
pub mod error;
pub mod keys;
pub mod lockup;
pub mod reply;
pub mod request;
pub mod state;
pub mod tree;

pub use claim::build_claim;
pub use error::Refused;
pub use keys::{key_agg, Secrets};
pub use lockup::{check_lockup, LockupSeen, Seen};
pub use reply::{check, Expect, Quote, Reply, Swap};
pub use request::Request;
pub use state::{Phase, SwapState, MAGIC_SWAP_STATE, STATE_VERSION};
pub use tree::{KeyOrder, SwapTree};

/// SW-02: the least invoice amount, in satoshis.
pub const MIN_INVOICE_SATS: u64 = 10_000;
/// SW-02: the most invoice amount, in satoshis.
pub const MAX_INVOICE_SATS: u64 = 25_000_000;
/// SW-03: the most bytes of a description.
pub const MAX_DESCRIPTION_BYTES: usize = 100;
/// SW-10: the most bytes of a reply that are read.
pub const MAX_REPLY_BYTES: usize = 16 * 1024;
/// SW-10: the most levels of arrays and objects in a reply.
pub const MAX_REPLY_DEPTH: usize = 6;
/// SW-13: the most bytes of a leaf script.
pub const MAX_LEAF_BYTES: usize = 128;
/// SW-23: the least seconds an invoice must have left.
pub const MIN_INVOICE_LIFE_SECS: u64 = 300;
/// SW-27: the least blocks between the chain tip and the timeout, for the
/// claim to confirm. **[to tune]**
pub const MIN_CLAIM_BLOCKS: u64 = 36;
/// SW-28: the least amount on chain, in satoshis.
pub const MIN_ONCHAIN_SATS: u64 = 1_000;

#[cfg(test)]
mod tests;
