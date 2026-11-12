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
//! 2. [`socks`]: the client side of SOCKS5, for Tor (NR-26): connecting, and
//!    Tor's name lookup.
//! 3. [`dial`]: opening a connection and looking up a seed name.
//! 4. [`driver`]: [`Driver`], [`DriverConfig`], [`Handle`] and [`AppEvent`].
//!
//! Limits, all fixed in code:
//!
//! - One reader thread per connection, reading at most 64 KiB at a time with
//!   a read timeout of 1 second, so that it notices a stop.
//! - One short-lived thread per connection attempt (10 seconds to connect,
//!   20 seconds more for the SOCKS5 handshake) and one per seed name looked
//!   up (10 seconds, then the answer is ignored). With Tor on, a seed round
//!   asks the filter names through the proxy, 3 lookups a seed and at most
//!   4 at once (NR-72).
//! - At most 4 attempts in flight and at most `target_peers + 4` sockets.
//! - The channel from the reader threads to the core thread holds 256
//!   messages; the channel of events to the app holds 64 and at most 32 MiB
//!   of blocks and transactions. When the app does not read, the driver
//!   stops reading from the peers (see [`Handle`]).
//! - A write to a peer that takes more than 3 seconds closes the peer.
//! - The own node, when one is set (NR-98 to NR-103), is one more socket
//!   and one more reader thread, apart from the limits above, and its events
//!   go to a channel of their own that holds 256 and never makes the core
//!   thread wait.
//!
//! With [`NetMode::NodeOnly`] the own node is the one peer (NR-105 to
//! NR-111): nothing else is dialed, and the state of the node, with its
//! faults, goes to the app on the own-node channel as
//! [`OwnNodeEvent::NodeOnly`]. The caller gives the mode; the driver reads
//! no settings.
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
pub use driver::{
    core_seeds, AppEvent, Driver, DriverConfig, EventReceiver, Handle, NetMode, OwnNodeEvent,
    StartError, EVENT_BYTES, FALLBACK_STOP_SECS,
};
pub use env::{
    Clock, DirLock, FileStorage, Key, LockError, OsRandomness, Randomness, Storage, SystemClock,
    LOCK_FILE,
};
/// Most scripts the network manager can watch for payment checks.
pub use openbmt_net::manager::MAX_WATCHED_SCRIPTS;
pub use openbmt_net::RelayState;
/// The merchant's own node, and why one is refused (NR-98).
pub use openbmt_net::{OwnNode, OwnNodeHost, OwnNodeRefused};
pub use socks::{SocksError, Target};
