//! The OpenBMT counter's Bitcoin peer client: the wire framing and the
//! single-peer session.
//!
//! The rules are in `docs/NETWORK-RULES.md`; each has an ID (NR-01 and on),
//! and the code, the drops and the tests name the rule they belong to.
//! This library has no sockets, no clock, no threads and no randomness of
//! its own. The caller passes in the bytes received, the time in seconds
//! and the nonce, and takes out the bytes to send and the events.
//!
//! The parts are:
//!
//! 1. [`frame`]: [`FrameReader`], which cuts a byte stream into messages
//!    and decides from each 24-byte header alone whether to read on, and
//!    [`encode`], which writes a message.
//! 2. [`session`]: [`Session`], one peer from the first `version` to the
//!    last byte: the handshake, the timers, and the [`Event`]s a good peer
//!    produces.
//!
//! This crate has had internal review only. No independent security audit
//! has been done yet.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod frame;
mod reason;
pub mod session;

#[cfg(test)]
mod tests;

pub use frame::{encode, Command, Frame, FrameReader, Message, PeerAddress};
pub use reason::Drop;
pub use session::{Event, Request, Session, SessionConfig};

/// NR-02: largest `block` payload in bytes.
pub const MAX_BLOCK_PAYLOAD: u32 = 4_000_000;
/// NR-02: largest `tx` payload in bytes.
pub const MAX_TX_PAYLOAD: u32 = 400_000;
/// NR-02: largest `cfilter` payload in bytes.
pub const MAX_CFILTER_PAYLOAD: u32 = 1_000_000;
/// NR-02: largest `headers` payload in bytes (2,000 headers of 81 bytes
/// and a 3-byte count).
pub const MAX_HEADERS_PAYLOAD: u32 = 162_003;
/// NR-02: largest `inv` or `notfound` payload in bytes (50,000 entries of
/// 36 bytes and a 3-byte count).
pub const MAX_INV_PAYLOAD: u32 = 1_800_003;
/// NR-02: largest `addr` or `addrv2` payload in bytes.
pub const MAX_ADDR_PAYLOAD: u32 = 50_003;
/// NR-02: largest `version` payload in bytes.
pub const MAX_VERSION_PAYLOAD: u32 = 400;
/// NR-02: largest payload of every other command the counter reads.
pub const MAX_SMALL_PAYLOAD: u32 = 100;
/// NR-03: largest payload of a command the counter does not read.
pub const MAX_UNKNOWN_PAYLOAD: u32 = 100_000;
/// NR-04: most headers in one `headers` message.
pub const MAX_HEADERS: usize = 2_000;
/// NR-04: most entries in one `inv` or `notfound` message.
pub const MAX_INVENTORY: usize = 50_000;
/// NR-04: most addresses in one `addr` or `addrv2` message.
pub const MAX_ADDRESSES: usize = 1_000;
/// NR-05: most bytes waiting to be sent to one peer.
pub const MAX_OUTGOING_BYTES: usize = 2_000_000;
/// NR-05: bytes beyond the current message that one push may leave in the
/// reader.
pub const PUSH_SLACK: usize = 65_536;
/// NR-11, NR-12: the protocol version the counter speaks and requires.
pub const PROTOCOL_VERSION: u32 = 70_016;
/// NR-12: longest user agent the counter accepts, in bytes.
pub const MAX_USER_AGENT: usize = 256;
/// NR-14: seconds a handshake may take.
pub const HANDSHAKE_SECS: u64 = 30;
/// NR-14: seconds between the counter's pings.
pub const PING_SECS: u64 = 120;
