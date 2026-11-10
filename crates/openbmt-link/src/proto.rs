//! The frames between a device and a relay (LK-30).
//!
//! ```text
//! OBMQ | version u8 | type u8 | body length u32 LE | body
//! ```
//!
//! One request, one answer. A device asks for a challenge, then makes one
//! request that carries it. Decoding is strict: a frame decodes only if it
//! is what the encoder would have written, and the length is checked against
//! the direction's limit before anything is read or allocated.
//!
//! Request types are 1 to 6. An answer to a request of type `t` has type
//! `0x80 | t` and a body that starts with a status byte of 0; a refusal has
//! type `0xFF` and a body of one status byte that is not 0 and names the
//! rule.

use std::fmt;

use openbmt_core::identity::{DeviceId, Sig};
use openbmt_core::wire::{DecodeError, Reader, VERSION};

use crate::seal::MAX_SEALED_LEN;

/// Magic of a relay frame.
pub const MAGIC_FRAME: [u8; 4] = *b"OBMQ";
/// Bytes before the body.
pub const FRAME_HEADER_LEN: usize = 4 + 1 + 1 + 4;
/// The longest body a relay reads: a sealed message to put.
pub const MAX_REQUEST_BODY: usize = MAX_SEALED_LEN;
/// Messages in one answer to a fetch (LK-24).
pub const FETCH_BATCH: usize = 64;
/// The most sealed bytes in one answer to a fetch (4 MiB, the size of a
/// full mailbox).
pub const FETCH_BYTES: usize = 4 * 1024 * 1024;
/// The longest body a device reads: a full fetch answer with its framing.
pub const MAX_RESPONSE_BODY: usize = FETCH_BYTES + 4096;

const T_HELLO: u8 = 1;
const T_CHALLENGE: u8 = 2;
const T_MAKE: u8 = 3;
const T_PUT: u8 = 4;
const T_FETCH: u8 = 5;
const T_ACK: u8 = 6;
const T_ANSWER: u8 = 0x80;
const T_REFUSED: u8 = 0xff;

/// Which way a frame travels, for its limit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    /// From a device to the relay.
    ToRelay,
    /// From the relay to a device.
    ToDevice,
}

impl Direction {
    /// The longest body in this direction.
    pub fn max_body(self) -> usize {
        match self {
            Self::ToRelay => MAX_REQUEST_BODY,
            Self::ToDevice => MAX_RESPONSE_BODY,
        }
    }
}

/// Why a frame was not accepted. The connection is closed on any of them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProtoError {
    /// The bytes are not a well-formed frame.
    Decode(DecodeError),
    /// The body is longer than the direction allows.
    TooLarge,
    /// The type byte is not one of this direction's.
    UnknownType(u8),
}

impl fmt::Display for ProtoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Decode(e) => write!(f, "LK-30: bad frame: {e}"),
            Self::TooLarge => write!(f, "LK-30: frame is over the limit"),
            Self::UnknownType(t) => write!(f, "LK-30: unknown frame type {t}"),
        }
    }
}

impl std::error::Error for ProtoError {}

impl From<DecodeError> for ProtoError {
    fn from(e: DecodeError) -> Self {
        Self::Decode(e)
    }
}

/// Why a relay did not do what was asked. Each names its rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// The sealed message is not well formed (LK-20).
    NotWellFormed,
    /// The sealed message's signature is not valid (LK-20).
    BadSignature,
    /// The sealed message is over the largest size (LK-20).
    TooLarge,
    /// The reader has no mailbox (LK-20).
    NoMailbox,
    /// The relay has all the mailboxes it allows (LK-21).
    MailboxesFull,
    /// The challenge was never given, or was already used (LK-23).
    ChallengeUnknown,
    /// The challenge is older than 60 seconds (LK-23).
    ChallengeExpired,
    /// The signature on the challenge is not the reader's (LK-23).
    ChallengeSignature,
    /// The relay could not write its storage (LK-41).
    Storage,
    /// The relay could not make a challenge (LK-23).
    NoRandomness,
}

impl Refusal {
    /// Every refusal.
    pub const ALL: [Refusal; 10] = [
        Refusal::NotWellFormed,
        Refusal::BadSignature,
        Refusal::TooLarge,
        Refusal::NoMailbox,
        Refusal::MailboxesFull,
        Refusal::ChallengeUnknown,
        Refusal::ChallengeExpired,
        Refusal::ChallengeSignature,
        Refusal::Storage,
        Refusal::NoRandomness,
    ];

    /// The status byte (never 0).
    pub fn to_byte(self) -> u8 {
        match self {
            Self::NotWellFormed => 1,
            Self::BadSignature => 2,
            Self::TooLarge => 3,
            Self::NoMailbox => 4,
            Self::MailboxesFull => 5,
            Self::ChallengeUnknown => 6,
            Self::ChallengeExpired => 7,
            Self::ChallengeSignature => 8,
            Self::Storage => 9,
            Self::NoRandomness => 10,
        }
    }

    /// The refusal for a status byte.
    pub fn from_byte(b: u8) -> Option<Self> {
        Self::ALL.into_iter().find(|r| r.to_byte() == b)
    }

    /// The rule this refusal comes from.
    pub fn rule(self) -> &'static str {
        match self {
            Self::NotWellFormed | Self::BadSignature | Self::TooLarge | Self::NoMailbox => "LK-20",
            Self::MailboxesFull => "LK-21",
            Self::ChallengeUnknown
            | Self::ChallengeExpired
            | Self::ChallengeSignature
            | Self::NoRandomness => "LK-23",
            Self::Storage => "LK-41",
        }
    }
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let what = match self {
            Self::NotWellFormed => "the sealed message is not well formed",
            Self::BadSignature => "the sealed message's signature is not valid",
            Self::TooLarge => "the sealed message is too large",
            Self::NoMailbox => "there is no mailbox for that reader",
            Self::MailboxesFull => "the relay has no room for another mailbox",
            Self::ChallengeUnknown => "the challenge is unknown or already used",
            Self::ChallengeExpired => "the challenge is too old",
            Self::ChallengeSignature => "the challenge signature is not the reader's",
            Self::Storage => "the relay could not store it",
            Self::NoRandomness => "the relay could not make a challenge",
        };
        write!(f, "{}: {what}", self.rule())
    }
}

impl std::error::Error for Refusal {}

/// What a device asks of a relay.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Request {
    /// Prove which relay this is: it signs this nonce (LK-32).
    Hello {
        /// A nonce the device chose.
        nonce: [u8; 32],
    },
    /// Give me a challenge to sign (LK-23).
    Challenge,
    /// Make my mailbox (LK-21).
    MakeMailbox {
        /// The reader.
        reader: DeviceId,
        /// The challenge this signature is over.
        challenge: [u8; 32],
        /// The reader's signature.
        sig: Sig,
    },
    /// Hold this sealed message (LK-20).
    Put {
        /// The sealed message.
        sealed: Vec<u8>,
    },
    /// Give me what is after the cursor (LK-24).
    Fetch {
        /// The reader.
        reader: DeviceId,
        /// The challenge this signature is over.
        challenge: [u8; 32],
        /// The reader's signature.
        sig: Sig,
        /// The relay's number of the last message the reader has.
        cursor: u64,
    },
    /// I have taken everything up to this number (LK-24).
    Ack {
        /// The reader.
        reader: DeviceId,
        /// The challenge this signature is over.
        challenge: [u8; 32],
        /// The reader's signature.
        sig: Sig,
        /// The relay's number of the last message the reader has taken.
        up_to: u64,
    },
}

/// What a relay answers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Response {
    /// The relay's key and its signature over the nonce (LK-32).
    Hello {
        /// The relay's identity key.
        relay: DeviceId,
        /// Its signature.
        sig: Sig,
    },
    /// A challenge, good once for 60 seconds.
    Challenge {
        /// The 32 bytes to sign.
        challenge: [u8; 32],
    },
    /// The mailbox exists.
    Made,
    /// The message is held, under this number.
    Stored {
        /// The relay's number for it.
        id: u64,
    },
    /// Messages after the cursor, oldest first.
    Messages {
        /// The relay's number and the sealed bytes of each.
        items: Vec<(u64, Vec<u8>)>,
        /// Whether more are waiting.
        more: bool,
    },
    /// Messages dropped.
    Acked {
        /// How many.
        dropped: u32,
    },
    /// Not done, and why.
    Refused(Refusal),
}

fn frame(ty: u8, body: &[u8], dir: Direction) -> Result<Vec<u8>, ProtoError> {
    if body.len() > dir.max_body() {
        return Err(ProtoError::TooLarge);
    }
    let len = u32::try_from(body.len()).map_err(|_| ProtoError::TooLarge)?;
    let mut out = Vec::with_capacity(FRAME_HEADER_LEN + body.len());
    out.extend_from_slice(&MAGIC_FRAME);
    out.push(VERSION);
    out.push(ty);
    out.extend_from_slice(&len.to_le_bytes());
    out.extend_from_slice(body);
    Ok(out)
}

/// Check a frame header: magic, version, and a length within the limit. The
/// type and the length it announces. Call this on the first
/// [`FRAME_HEADER_LEN`] bytes before reading the body.
pub fn parse_header(
    header: &[u8; FRAME_HEADER_LEN],
    dir: Direction,
) -> Result<(u8, usize), ProtoError> {
    let mut r = Reader::framed(header, &MAGIC_FRAME)?;
    let ty = r.u8()?;
    let len = usize::try_from(r.u32_le()?).map_err(|_| ProtoError::TooLarge)?;
    if len > dir.max_body() {
        return Err(ProtoError::TooLarge);
    }
    Ok((ty, len))
}

fn split(bytes: &[u8], dir: Direction) -> Result<(u8, &[u8]), ProtoError> {
    let header: [u8; FRAME_HEADER_LEN] = bytes
        .get(..FRAME_HEADER_LEN)
        .and_then(|h| h.try_into().ok())
        .ok_or(DecodeError::Truncated)?;
    let (ty, len) = parse_header(&header, dir)?;
    let body = bytes
        .get(FRAME_HEADER_LEN..)
        .ok_or(DecodeError::Truncated)?;
    match body.len().cmp(&len) {
        std::cmp::Ordering::Less => Err(DecodeError::Truncated.into()),
        std::cmp::Ordering::Greater => Err(DecodeError::TrailingBytes.into()),
        std::cmp::Ordering::Equal => Ok((ty, body)),
    }
}

impl Request {
    /// The whole frame.
    pub fn encode(&self) -> Result<Vec<u8>, ProtoError> {
        let d = Direction::ToRelay;
        match self {
            Self::Hello { nonce } => frame(T_HELLO, nonce, d),
            Self::Challenge => frame(T_CHALLENGE, &[], d),
            Self::MakeMailbox {
                reader,
                challenge,
                sig,
            } => frame(T_MAKE, &signed_body(reader, challenge, sig, None), d),
            Self::Put { sealed } => frame(T_PUT, sealed, d),
            Self::Fetch {
                reader,
                challenge,
                sig,
                cursor,
            } => frame(
                T_FETCH,
                &signed_body(reader, challenge, sig, Some(*cursor)),
                d,
            ),
            Self::Ack {
                reader,
                challenge,
                sig,
                up_to,
            } => frame(T_ACK, &signed_body(reader, challenge, sig, Some(*up_to)), d),
        }
    }

    /// Decode a whole frame.
    pub fn decode(bytes: &[u8]) -> Result<Self, ProtoError> {
        let (ty, body) = split(bytes, Direction::ToRelay)?;
        let mut r = Reader::new(body);
        let out = match ty {
            T_HELLO => Self::Hello { nonce: r.array()? },
            T_CHALLENGE => Self::Challenge,
            T_MAKE => {
                let (reader, challenge, sig) = read_signed(&mut r)?;
                Self::MakeMailbox {
                    reader,
                    challenge,
                    sig,
                }
            }
            T_PUT => {
                return Ok(Self::Put {
                    sealed: body.to_vec(),
                })
            }
            T_FETCH => {
                let (reader, challenge, sig) = read_signed(&mut r)?;
                Self::Fetch {
                    reader,
                    challenge,
                    sig,
                    cursor: r.u64_le()?,
                }
            }
            T_ACK => {
                let (reader, challenge, sig) = read_signed(&mut r)?;
                Self::Ack {
                    reader,
                    challenge,
                    sig,
                    up_to: r.u64_le()?,
                }
            }
            other => return Err(ProtoError::UnknownType(other)),
        };
        r.finish()?;
        Ok(out)
    }
}

fn signed_body(reader: &DeviceId, challenge: &[u8; 32], sig: &Sig, n: Option<u64>) -> Vec<u8> {
    let mut b = Vec::with_capacity(136);
    b.extend_from_slice(&reader.to_bytes());
    b.extend_from_slice(challenge);
    b.extend_from_slice(&sig.0);
    if let Some(n) = n {
        b.extend_from_slice(&n.to_le_bytes());
    }
    b
}

fn read_signed(r: &mut Reader<'_>) -> Result<(DeviceId, [u8; 32], Sig), DecodeError> {
    Ok((r.device_id()?, r.array()?, r.sig()?))
}

impl Response {
    /// The whole frame.
    pub fn encode(&self) -> Result<Vec<u8>, ProtoError> {
        let d = Direction::ToDevice;
        match self {
            Self::Hello { relay, sig } => {
                let mut b = vec![0];
                b.extend_from_slice(&relay.to_bytes());
                b.extend_from_slice(&sig.0);
                frame(T_ANSWER | T_HELLO, &b, d)
            }
            Self::Challenge { challenge } => {
                let mut b = vec![0];
                b.extend_from_slice(challenge);
                frame(T_ANSWER | T_CHALLENGE, &b, d)
            }
            Self::Made => frame(T_ANSWER | T_MAKE, &[0], d),
            Self::Stored { id } => {
                let mut b = vec![0];
                b.extend_from_slice(&id.to_le_bytes());
                frame(T_ANSWER | T_PUT, &b, d)
            }
            Self::Messages { items, more } => {
                let mut b = vec![0, u8::from(*more)];
                let count = u32::try_from(items.len()).map_err(|_| ProtoError::TooLarge)?;
                b.extend_from_slice(&count.to_le_bytes());
                for (id, sealed) in items {
                    let len = u32::try_from(sealed.len()).map_err(|_| ProtoError::TooLarge)?;
                    b.extend_from_slice(&id.to_le_bytes());
                    b.extend_from_slice(&len.to_le_bytes());
                    b.extend_from_slice(sealed);
                }
                frame(T_ANSWER | T_FETCH, &b, d)
            }
            Self::Acked { dropped } => {
                let mut b = vec![0];
                b.extend_from_slice(&dropped.to_le_bytes());
                frame(T_ANSWER | T_ACK, &b, d)
            }
            Self::Refused(why) => frame(T_REFUSED, &[why.to_byte()], d),
        }
    }

    /// Decode a whole frame.
    pub fn decode(bytes: &[u8]) -> Result<Self, ProtoError> {
        let (ty, body) = split(bytes, Direction::ToDevice)?;
        let mut r = Reader::new(body);
        if ty == T_REFUSED {
            let why =
                Refusal::from_byte(r.u8()?).ok_or(DecodeError::NonCanonical("unknown refusal"))?;
            r.finish()?;
            return Ok(Self::Refused(why));
        }
        if r.u8()? != 0 {
            return Err(DecodeError::NonCanonical("an answer's status is 0").into());
        }
        let out = match ty.checked_sub(T_ANSWER) {
            Some(T_HELLO) => Self::Hello {
                relay: r.device_id()?,
                sig: r.sig()?,
            },
            Some(T_CHALLENGE) => Self::Challenge {
                challenge: r.array()?,
            },
            Some(T_MAKE) => Self::Made,
            Some(T_PUT) => Self::Stored { id: r.u64_le()? },
            Some(T_FETCH) => {
                let more = r.flag()?;
                let count = r.count(FETCH_BATCH, 12)?;
                let mut items: Vec<(u64, Vec<u8>)> = Vec::with_capacity(count);
                for _ in 0..count {
                    let id = r.u64_le()?;
                    if items.last().is_some_and(|(prev, _)| *prev >= id) {
                        return Err(DecodeError::NonCanonical("numbers are not ascending").into());
                    }
                    let len = usize::try_from(r.u32_le()?).map_err(|_| DecodeError::TooLarge)?;
                    if len > MAX_SEALED_LEN {
                        return Err(DecodeError::TooLarge.into());
                    }
                    items.push((id, r.take(len)?.to_vec()));
                }
                Self::Messages { items, more }
            }
            Some(T_ACK) => Self::Acked {
                dropped: r.u32_le()?,
            },
            _ => return Err(ProtoError::UnknownType(ty)),
        };
        r.finish()?;
        Ok(out)
    }
}
