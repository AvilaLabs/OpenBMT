//! Why a session ends.
//!
//! Every drop names the rule of `docs/NETWORK-RULES.md` that it enforces
//! ([`Drop::rule`]). The `Display` text starts with the rule ID. A drop
//! means the connection must be closed; the session returns the same drop
//! to every later call.

use std::fmt;

/// A reason the counter ends a connection to a peer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Drop {
    /// NR-01: the message's magic is not the counter's network.
    WrongMagic,
    /// NR-01: the command field is not a plain ASCII name padded with NULs.
    BadCommand,
    /// NR-01: the checksum does not match the payload.
    BadChecksum,
    /// NR-02: the payload length is over the limit for a command the
    /// counter reads.
    TooLong {
        /// The command.
        command: &'static str,
        /// The length the header states.
        len: u32,
        /// The limit for the command.
        limit: u32,
    },
    /// NR-03: the payload of a command the counter does not read is over
    /// the limit.
    UnknownTooLong {
        /// The length the header states.
        len: u32,
    },
    /// NR-04: the payload does not parse as its command, or leaves bytes
    /// over.
    Malformed {
        /// The command.
        command: &'static str,
    },
    /// NR-04: the message holds more items than a message may.
    TooManyItems {
        /// The command.
        command: &'static str,
        /// The count the message states.
        count: u64,
        /// The most a message may hold.
        limit: usize,
    },
    /// NR-04: a `cfilter` of a type other than 0.
    WrongFilterType {
        /// The filter type.
        filter_type: u8,
    },
    /// NR-05: more bytes were pushed than one message in progress can
    /// need.
    ReceiveBacklog,
    /// NR-05: too many bytes are waiting to be sent to the peer.
    SendBacklog,
    /// NR-12: the first message of the peer was not `version`.
    FirstNotVersion,
    /// NR-12: the peer's protocol version is below 70016.
    VersionTooOld {
        /// The version the peer states.
        version: u32,
    },
    /// NR-12: the peer does not offer `WITNESS` and `NETWORK` or
    /// `NETWORK_LIMITED`.
    MissingServices {
        /// The service bits the peer states.
        services: u64,
    },
    /// NR-12: the peer's nonce is the counter's own.
    OwnNonce,
    /// NR-12: the peer's user agent is longer than 256 bytes.
    UserAgentTooLong {
        /// The length in bytes.
        len: usize,
    },
    /// NR-13: a message that belongs after the handshake arrived before
    /// it ended.
    BeforeHandshake {
        /// The command.
        command: &'static str,
    },
    /// NR-13: the peer sent `version` a second time.
    DuplicateVersion,
    /// NR-13: the peer sent `verack` a second time.
    DuplicateVerack,
    /// NR-13: the caller made a request before the handshake ended.
    NotReady,
    /// NR-14: the handshake did not finish in 30 seconds.
    HandshakeTimeout,
    /// NR-14: the last ping was still unanswered when the next was due.
    PingUnanswered,
}

impl Drop {
    /// The rule of `docs/NETWORK-RULES.md` this drop enforces.
    pub fn rule(&self) -> &'static str {
        match self {
            Self::WrongMagic | Self::BadCommand | Self::BadChecksum => "NR-01",
            Self::TooLong { .. } => "NR-02",
            Self::UnknownTooLong { .. } => "NR-03",
            Self::Malformed { .. } | Self::TooManyItems { .. } | Self::WrongFilterType { .. } => {
                "NR-04"
            }
            Self::ReceiveBacklog | Self::SendBacklog => "NR-05",
            Self::FirstNotVersion
            | Self::VersionTooOld { .. }
            | Self::MissingServices { .. }
            | Self::OwnNonce
            | Self::UserAgentTooLong { .. } => "NR-12",
            Self::BeforeHandshake { .. }
            | Self::DuplicateVersion
            | Self::DuplicateVerack
            | Self::NotReady => "NR-13",
            Self::HandshakeTimeout | Self::PingUnanswered => "NR-14",
        }
    }
}

impl fmt::Display for Drop {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: ", self.rule())?;
        match self {
            Self::WrongMagic => write!(f, "the message is for another network"),
            Self::BadCommand => write!(f, "the command field is not a plain name"),
            Self::BadChecksum => write!(f, "the checksum does not match the payload"),
            Self::TooLong {
                command,
                len,
                limit,
            } => write!(
                f,
                "{command} payload of {len} bytes is over the limit {limit}"
            ),
            Self::UnknownTooLong { len } => {
                write!(
                    f,
                    "payload of {len} bytes of a command not read is too long"
                )
            }
            Self::Malformed { command } => {
                write!(f, "{command} payload does not parse, or leaves bytes over")
            }
            Self::TooManyItems {
                command,
                count,
                limit,
            } => write!(f, "{command} holds {count} items, the most is {limit}"),
            Self::WrongFilterType { filter_type } => {
                write!(f, "cfilter of type {filter_type}, only type 0 is read")
            }
            Self::ReceiveBacklog => write!(f, "more bytes pushed than a message can need"),
            Self::SendBacklog => write!(f, "the peer is not reading what is queued for it"),
            Self::FirstNotVersion => write!(f, "the first message was not version"),
            Self::VersionTooOld { version } => {
                write!(f, "protocol version {version} is below 70016")
            }
            Self::MissingServices { services } => write!(
                f,
                "services {services:#x} lack witness or network or network-limited"
            ),
            Self::OwnNonce => write!(f, "the peer's nonce is the counter's own"),
            Self::UserAgentTooLong { len } => {
                write!(f, "user agent of {len} bytes is longer than 256")
            }
            Self::BeforeHandshake { command } => {
                write!(f, "{command} arrived before the handshake ended")
            }
            Self::DuplicateVersion => write!(f, "a second version"),
            Self::DuplicateVerack => write!(f, "a second verack"),
            Self::NotReady => write!(f, "a request before the handshake ended"),
            Self::HandshakeTimeout => write!(f, "the handshake took 30 seconds or more"),
            Self::PingUnanswered => write!(f, "the last ping was not answered"),
        }
    }
}

impl std::error::Error for Drop {}
