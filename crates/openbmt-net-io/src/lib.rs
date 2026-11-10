//! The OpenBMT counter's network driver: the part of the peer client that
//! owns sockets and threads.
//!
//! `openbmt-net` has no sockets, no clock and no threads. This crate runs
//! its [`Manager`](openbmt_net::Manager) and
//! [`AddressBook`](openbmt_net::AddressBook) on one thread (the core thread),
//! and gives them bytes from the network, the time, randomness and a place to
//! keep the header store and the address book. It decides nothing about a
//! payment: what the manager says goes to the app as an [`AppEvent`].
//!
//! The parts are:
//!
//! 1. [`env`]: the traits for [`Storage`], [`Clock`] and [`Randomness`], and
//!    the ones for a computer with a file system: [`FileStorage`],
//!    [`SystemClock`] and [`OsRandomness`]. An app on another platform
//!    passes its own.
//! 2. [`socks`]: the client side of SOCKS5, for Tor (NR-26).
//! 3. [`dial`]: opening a connection and looking up a seed name.
//! 4. [`driver`]: [`Driver`], [`DriverConfig`], [`Handle`] and [`AppEvent`].
//!
//! Limits, all fixed in code:
//!
//! - One reader thread per connection, reading at most 64 KiB at a time with
//!   a read timeout of 1 second, so that it notices a stop.
//! - One short-lived thread per connection attempt (10 seconds to connect,
//!   20 seconds more for the SOCKS5 handshake) and one per seed name looked
//!   up (10 seconds, then the answer is ignored).
//! - At most 4 attempts in flight and at most `target_peers + 4` sockets.
//! - The channel from the reader threads to the core thread holds 256
//!   messages; the channel of events to the app holds 64. When the app does
//!   not read, the driver stops reading from the peers (see [`Handle`]).
//! - A write to a peer that takes more than 10 seconds closes the peer.
//!
//! The rules the driver decides are NR-70 and on in
//! `docs/NETWORK-RULES.md`.
//!
//! This crate has had internal review only. No independent security audit
//! has been done yet.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod dial;
pub mod driver;
pub mod env;
pub mod socks;

#[cfg(test)]
mod tests;

pub use dial::DialError;
pub use driver::{AppEvent, Driver, DriverConfig, Handle, StartError};
pub use env::{Clock, FileStorage, Key, OsRandomness, Randomness, Storage, SystemClock};
pub use socks::{SocksError, Target};
