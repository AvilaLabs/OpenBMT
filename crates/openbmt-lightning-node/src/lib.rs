//! Shop Lightning: the shop's own Lightning node, built in
//! ([rules](../../../docs/SHOP-LIGHTNING-RULES.md)).
//!
//! - [`bip85`]: the node's seed, the 64-byte child of the shop's words (SL-10).
//! - [`device`], [`device_tcp`], [`seal`]: the second device and what is
//!   sealed for it (SL-20).
//! - [`store`]: the store that writes both copies, the second device first
//!   (SL-21, SL-23, SL-24); [`state`]: which copy is newer (SL-25).
//! - [`expect`]: what is expected of each invoice, across a restart; [`paid`]:
//!   the paid events kept until acknowledged; [`sweep`]: the spendable outputs
//!   swept to the shop's address.
//! - [`chain`]: chain data from the shop's own Bitcoin Core (SL-26);
//!   [`filters`]: the same from the compact-filter watcher on the counter's
//!   peer layer (SL-26, SL-27).
//! - [`provider`]: the choice among a provider's offers (SL-29, SL-30);
//!   [`room`]: the room to receive and the decision for an amount (SL-40,
//!   SL-41).
//! - [`node`]: the library wired as the rules say: receive only (SL-01), the
//!   LSPS2 client (SL-28), channels from the chosen provider alone (SL-31), a
//!   sale paid only on `Event::PaymentClaimed` (SL-21), the Tor seam
//!   (SL-32), the sweep and the close (SL-33, SL-36). [`cap`] holds the
//!   arithmetic of the sweep.
//!
//! The library is `lightning` 0.2.8 used as a library, never forked, and not
//! through `ldk-node` (SL-02). `NodeConfig` takes the 64 bytes and never the
//! words; the words stay on the stick (SL-11).
//!
//! Where the code is more exact than the rules, or adds to them:
//!
//! - The monitors, which a copy cannot lack the newest of, go the strict way:
//!   the second device first, then the counter's own copy, and the library is
//!   told the update is done only when both are. The channel manager, the
//!   liquidity state and the spendable-output descriptors go through
//!   [`store::Background`]: the counter's copy at once, the second device's as
//!   soon as it can be reached. The library's loop must not wait for a device
//!   that is away, or the peer stalls as it does with `ldk-node` (SL-02), and
//!   it reads a channel manager older than its monitors. A restore from the
//!   second device then has a manager that may be older than the monitors,
//!   and never monitors older than the counter's.
//! - A start needs the second device to be reachable: without it the two
//!   copies cannot be compared (SL-25), so the node does not start
//!   (`StartError::DeviceAway`). Once running, a device that goes away is a
//!   state, not an error (SL-23).
//! - The two copies are compared by the latest update number of each channel
//!   monitor. The channel manager's blob is brought over whole with the
//!   rest. Copies that are each ahead somewhere are refused
//!   (`StateError::Diverged`).
//! - Channels are not anchor channels: the node has no on-chain wallet to
//!   bump a fee with, so it does not offer the feature. A channel the
//!   provider opens may be used before its funding transaction has a
//!   confirmation if the shop says so (`ProviderConfig::zero_conf`); the
//!   provider funds it and the shop holds nothing in it until a payment
//!   comes.
//! - `client_trusts_lsp` is a setting of the provider's service. In
//!   `lightning-liquidity` 0.2.8 the client side has no such setting and
//!   always takes the client-trusts-LSP flow of bLIP-52.
//! - An invoice that opens a channel is checked twice against the fee the
//!   owner allows (SL-29): when the offer is chosen, and again when the
//!   payment arrives, where a payment that has had more skimmed than the offer
//!   said is refused.
//! - Only invoices this node made are paid: a payment to another hash, a
//!   keysend and a payment from the provider that is not for an invoice are
//!   failed back.
//! - Spendable outputs of a close on the chain are swept by the library's
//!   `OutputSweeper` to the address the caller gave ([`sweep`]); its state is
//!   kept the way the manager is, the counter's copy first.
//! - What is expected of each invoice (the most the provider may skim, the
//!   expiry) is one sealed blob in the store, written the strict way, second
//!   device first, before the invoice is handed back ([`expect`]). A payment
//!   is claimed only for an invoice that is in it, not expired, with the skim
//!   at or under the maximum, also after a restart.
//! - Paid events are given at least once: each is kept in the store until
//!   `Node::ack_paid`, and a start gives again every one not acknowledged,
//!   with the same payment hash ([`paid`]). The counter treats a repeat as the
//!   same payment.
//! - Nothing here listens: the node makes one connection, to the provider.
//! - With the filter watcher the node reads blocks from the peers whose
//!   filters match what it watches, sends transactions through the same peers,
//!   and has no fee source: the fee rates stay at the fixed values of
//!   [`chain::CoreFees`]. The set it watches is made again at each start from
//!   the monitors and the sweeper ([`filters`]).
//! - With Tor on (`NodeConfig::tor`) the connection to the provider and every
//!   connection of the filter watcher go through the SOCKS5 proxy, and a proxy
//!   that cannot be reached is a stop: nothing is sent over the clear net
//!   instead (SL-32, `StartError::TorUnreachable`).
//!
//! First build. No independent security audit has been done, and the library's
//! async persistence is marked beta by its authors. It runs only on regtest
//! and signet (SL-60).

#![forbid(unsafe_code)]

pub mod bip85;
pub mod cap;
pub mod chain;
pub mod close;
pub mod device;
pub mod device_tcp;
pub mod expect;
pub mod filters;
pub mod logger;
pub mod node;
pub mod paid;
pub mod provider;
pub mod room;
pub mod seal;
pub mod state;
pub mod store;
pub mod sweep;
pub mod types;
pub mod words;

#[cfg(test)]
mod testutil;
