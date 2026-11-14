//! Sealed messages between the shop's own devices, and the relay that holds
//! them ([rules](../../../docs/LINK-RULES.md)).
//!
//! - [`seal`]: seal a message for one device and open one (LK-01 to LK-09).
//!   Pure: no sockets, no clock, randomness passed in.
//! - [`numbers`]: the message numbers a device keeps, stored before use
//!   (LK-06, LK-40).
//! - [`relay`] and [`store`]: the relay's state machine and where it keeps
//!   its mailboxes (LK-20 to LK-26, LK-41). Pure apart from the store.
//! - [`proto`]: the frames to a relay (LK-30).
//! - [`shopln`]: what the Shop Lightning kinds carry (LK-83, LK-84).
//! - [`client`] and [`server`]: blocking sockets over `std::net` (LK-30 to
//!   LK-32).
//!
//! Nothing here gives authority. A sealed command is still a command to be
//! checked as one (LK-09).
//!
//! Where the code is more exact than the rules, or adds to them:
//!
//! - The AEAD's associated data is the prefix of the header (everything
//!   the signature covers except the ciphertext hash); the hash cannot be
//!   in it because it is made from the ciphertext. The signature binds the
//!   two the other way (LK-02, LK-04).
//! - The size steps (LK-07) count the 4-byte length: the largest content
//!   is 65,532 bytes.
//! - A request that reads or empties a mailbox names the challenge it
//!   answers, and the reader's signature covers the action and its number
//!   as well as the challenge and the relay's key, so that a frame changed
//!   on the way cannot become another request (LK-23, LK-33).
//! - A challenge belongs to one connection (the relay is told when a
//!   connection closes), and a connection holds one at a time (LK-23).
//! - Each message in a mailbox has a number the relay gives it; a cursor
//!   and an acknowledgement are such numbers, and an acknowledgement drops
//!   only numbers already handed out in a fetch (LK-24).
//!
//! This crate has had internal review only. No independent security audit
//! has been done yet.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod client;
pub mod numbers;
pub mod proto;
pub mod random;
pub mod relay;
pub mod seal;
pub mod server;
pub mod shopln;
pub mod store;

#[cfg(test)]
mod tests;

pub use client::{Client, ClientConfig, ClientError, Fetched, Route};
pub use numbers::{Inbox, NextError, Outbox, TakeError};
/// HKDF-SHA256 now lives in the core crate (the Box pairing uses it too).
pub use openbmt_core::hkdf;
/// Files made with mode 0600 and directories with 0700, for the crates above.
pub use openbmt_net_io::private;
pub use proto::{Refusal, Request, Response};
pub use random::os_random;
pub use relay::{Mailbox, Relay, RelayConfig, StoreAnswer, StoreHook, WelcomeAnswer, WelcomeHook};
pub use seal::{open, seal, Kind, OpenError, Opened, Role, SealError, StoreParties};
pub use server::{Server, ServerConfig};
pub use store::{DirStore, Event, MailStore, MemoryStore};
