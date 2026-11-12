//! The OpenBMT counter's Bitcoin peer client: the wire framing, the
//! single-peer session and the manager of many peers.
//!
//! The rules are in `docs/NETWORK-RULES.md`; each has an ID (NR-01 and on),
//! and the code, the drops and the tests name the rule they belong to.
//! This library has no sockets, no clock, no threads and no randomness of
//! its own. The caller passes in the bytes received, the time in seconds
//! and the nonce, and takes out the bytes to send and the events.
//!
//! The parts are:
//!
//! 1. [`book`]: [`AddressBook`], the peers the counter may dial, with the
//!    limits on what it keeps, the draw of the next peer and the bans.
//! 2. [`frame`]: [`FrameReader`], which cuts a byte stream into messages
//!    and decides from each 24-byte header alone whether to read on, and
//!    [`encode`], which writes a message.
//! 3. [`session`]: [`Session`], one peer from the first `version` to the
//!    last byte: the handshake, the timers, and the [`Event`]s a good peer
//!    produces.
//! 4. [`v2`]: [`Transport`], which a session reads and writes through: the
//!    old transport, or the encrypted one of BIP324 (NR-80 to NR-85), with
//!    the cryptography of the `bip324` crate.
//! 5. [`manager`]: [`Manager`], which runs several sessions at once, feeds
//!    the headers to the watcher's header store, checks blocks by filters
//!    and fetches announced transactions, and turns what peers send into
//!    [`Output`]s for the watcher.
//! 6. [`own_node`]: [`OwnNode`], the merchant's own node, which is only ever
//!    a Tor v3 address (NR-98, NR-99); the manager keeps its session apart
//!    from the peers and reports what it holds and how far it has got
//!    (NR-100 to NR-102). With `node_only` on (NR-106 to NR-111) the own
//!    node is the manager's one peer instead, with the checks of any peer.
//!
//! This crate has had internal review only. No independent security audit
//! has been done yet.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod book;
pub mod frame;
pub mod manager;
pub mod onion;
pub mod own_node;
mod reason;
pub mod session;
mod sha3;
pub mod v2;

#[cfg(test)]
mod tests;

pub use book::{Added, AddressBook, BookError, OnionDraw, Picked, Refusal, Source};
pub use frame::{encode, Command, Frame, FrameReader, Message, PeerAddress};
pub use manager::{
    BroadcastRefused, ConnectRefused, Manager, ManagerConfig, NodeOnlyView, Output, OwnNodeView,
    PeerId, RelayState, TransportChoice, Wanted, MAX_BROADCAST_WEIGHT, MAX_HELD, MAX_OWN_ANNOUNCED,
    MAX_OWN_BLOCKS, MAX_OWN_WANTED, NODE_LIMITED_BLOCKS, NOT_SEEN_SECS, OWN_ANSWER_SECS,
    SEEN_BACK_GROUPS,
};
pub use own_node::{OwnNode, OwnNodeHost, OwnNodeRefused};
pub use reason::Drop;
pub use session::{Event, Request, Session, SessionConfig};
pub use v2::{Transport, V2Entropy};

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
