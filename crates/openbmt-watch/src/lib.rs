//! The OpenBMT counter's payment watcher: what each kind of evidence about a
//! payment may prove.
//!
//! The rules are in `docs/WATCHER-RULES.md`; each has an ID (WR-01 and on),
//! and the code, the refusals and the tests name the rule they belong to.
//! The library has no network, no clock and no storage. The caller passes in
//! what was received, from whom, and the time in seconds, and answers three
//! questions about the header chain ([`HeaderChain`]).
//!
//! The parts are:
//!
//! 1. [`chain`]: the header chain as the library sees it, confirmations and
//!    the header check on its own.
//!    [`headers`]: [`HeaderStore`], which builds and checks that chain.
//! 2. [`proof`]: a confirmation the Box reports, believed only with a merkle
//!    proof, the coinbase and the payment in full.
//! 3. [`block`]: a confirmation the light client finds in a full block.
//! 4. [`netgroup`]: how peers are counted.
//! 5. [`sale`]: [`SaleWatch`], the evidence for one sale, and the
//!    [`SaleStatus`] the rules derive from it. The status holds the facts
//!    that `openbmt_core::policy::evaluate` reads; this library does not
//!    call it.
//! 6. [`health`]: whether the check is working.
//! 7. [`fees`]: a fee rate checked from full blocks (`docs/FEE-RULES.md`).
//! 8. [`own_node`]: the merchant's own node, kept apart from the peers.
//!
//! This crate has had internal review only. No independent security audit
//! has been done yet.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod block;
pub mod chain;
pub mod fees;
pub mod headers;
pub mod health;
pub mod netgroup;
pub mod own_node;
pub mod proof;
mod refusal;
pub mod sale;
pub mod say;

#[cfg(test)]
mod tests;

pub use block::{payments_in_block, spends_in_block, WellFormedBlock};
pub use chain::{check_header, confirmations, HeaderChain};
pub use fees::{block_fee_sample, FeeError, FeeHistory, FeeSample};
pub use headers::{Accepted, HeaderStore};
pub use health::{check_health, Health, HealthInput, Peer, PeerTransport, Problem};
pub use netgroup::{host_group, network_group, NetGroup, PeerAddr};
pub use own_node::{
    check_own_node, OwnNodeHealth, OwnNodeInput, OwnNodeLink, OwnNodeProblem, OwnNodeTracker,
};
pub use proof::{verify_box_proof, ProvenPayment};
pub use refusal::Refusal;
pub use sale::{SaleStatus, SaleWatch, Sighted, Source};

/// WR-10: largest merkle proof in bytes.
pub const MAX_PROOF_BYTES: usize = 64 * 1024;
/// WR-10, WR-34: largest transaction kept or accepted in a proof, in bytes.
pub const MAX_TX_BYTES: usize = 400_000;
/// WR-12: most transactions a proof may state. One transaction weighs at
/// least 240 weight units and a block at most 4,000,000.
pub const MAX_TREE_TRANSACTIONS: u32 = 16_666;
/// WR-12: fewest transactions a proof may state (a coinbase and a payment).
pub const MIN_TREE_TRANSACTIONS: u32 = 2;
/// WR-16, WR-22: the length without witness that no transaction may have.
pub const FORBIDDEN_TX_BASE_SIZE: usize = 64;
/// WR-20: largest block in bytes.
pub const MAX_BLOCK_BYTES: usize = 4_000_000;
/// WR-34: most transactions kept for one sale.
pub const MAX_SALE_TRANSACTIONS: usize = 20;
/// Most network groups kept for one transaction. More are not recorded;
/// the count only matters up to [`SEEN_GROUPS`].
pub const MAX_ENTRY_GROUPS: usize = 64;
/// Most blocks kept for one transaction. When a transaction has been in more,
/// the oldest is forgotten.
pub const MAX_ENTRY_BLOCKS: usize = 64;
/// Longest script a stored sale watch may hold, in bytes (the consensus
/// limit for a script).
pub const MAX_SCRIPT_BYTES: usize = 10_000;
/// WR-42: most replaced transactions a sale remembers.
pub const MAX_REPLACED_REMEMBERED: usize = 64;
/// WR-32: the most a tolerance can be, in basis points.
pub const MAX_TOLERANCE_BPS: u32 = 10_000;
/// WR-44: network groups that must report a transaction for it to count as
/// seen by the light client.
pub const SEEN_GROUPS: usize = 3;
/// WR-50: network groups the light client must be connected to.
pub const MIN_PEER_GROUPS: usize = 3;
/// WR-50: network groups that must have a peer serving compact filters.
pub const MIN_FILTER_GROUPS: usize = 2;
/// WR-51: seconds the tip's header time may be behind the clock.
pub const MAX_TIP_AGE_SECS: u64 = 7_200;
/// WR-52: blocks a peer's best height may be above the tip before the
/// counter counts as behind.
pub const BEHIND_BLOCKS: u32 = 2;
/// WR-52: seconds the counter may stay behind.
pub const MAX_BEHIND_SECS: u64 = 600;
/// WR-53: seconds the clock may be behind the tip's header time.
pub const MAX_CLOCK_BEHIND_SECS: u64 = 7_200;
/// WR-63: blocks the counter's tip may be above the own node's best height,
/// for at most `OWN_NODE_LAG_SECS`, before the node counts as behind. A new
/// block reaches the counter's peers before it reaches the own node, so one
/// block of lag is normal for a moment; without this the shown check would
/// flip to the light check and back at every block.
pub const OWN_NODE_LAG_BLOCKS: u32 = 1;
/// WR-63: seconds the own node may stay `OWN_NODE_LAG_BLOCKS` behind.
pub const OWN_NODE_LAG_SECS: u64 = 300;
/// WR-63: seconds the own node may be silent while connected.
pub const OWN_NODE_SILENT_SECS: u64 = 600;
/// WR-63: seconds after a start, a retry or a new setting during which a node
/// that has not yet been connected is "connecting", not "not reachable".
pub const OWN_NODE_CONNECT_SECS: u64 = 120;
