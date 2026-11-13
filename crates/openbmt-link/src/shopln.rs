//! What the Shop Lightning kinds carry (LK-83, LK-84).
//!
//! ```text
//! Store:   OBM^ | version | request ID 16 | action u8 | body
//! Stored:  OBM_ | version | request ID 16 | action u8 | result u8 | data
//! Report:  OBM\ | version | six fields (LK-84)
//! ```
//!
//! Decoding is strict, as in [`openbmt_core::wire`]: bytes decode only if
//! they are what the encoder writes, every length is checked against its
//! limit before anything is allocated, and every combination of action and
//! result that LK-83 does not name is refused.

use std::fmt;

use openbmt_core::wire::{self, DecodeError, Reader};

/// Magic of a Store message's content.
pub const MAGIC_STORE: [u8; 4] = *b"OBM^";
/// Magic of a Stored message's content.
pub const MAGIC_STORED: [u8; 4] = *b"OBM_";
/// Magic of a Lightning report's content.
pub const MAGIC_LIGHTNING_REPORT: [u8; 4] = *b"OBM\\";

/// The longest part of a blob, in bytes (LK-83).
pub const MAX_PART_LEN: usize = 60_000;
/// The largest blob, in bytes: 16 MiB (LK-83).
pub const MAX_BLOB_LEN: usize = 16 * 1024 * 1024;
/// The most parts of a blob: enough for [`MAX_BLOB_LEN`] in parts of
/// [`MAX_PART_LEN`].
pub const MAX_PARTS: u32 = (MAX_BLOB_LEN.div_ceil(MAX_PART_LEN)) as u32;
/// The longest name (LK-83).
pub const MAX_NAME_LEN: usize = 255;
/// The longest name the Box stores: its hexadecimal and the `.tmp` of a write
/// in progress are a file name of at most 255 bytes (BX-93).
pub const MAX_STORED_NAME_LEN: usize = 125;
/// The most bytes of names in one answer to a list: each name and its
/// length byte count.
pub const MAX_LIST_BYTES: usize = 60_000;

/// Whether `name` is a name of the node (LK-83): 1 to 255 of the
/// characters `A-Z a-z 0-9 _ - ~`.
pub fn name_ok(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_NAME_LEN
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'~'))
}

/// What a Store asks for (LK-83).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Write one part of a blob.
    Write,
    /// Read one part of a blob.
    Read,
    /// List the names after a name.
    List,
    /// Remove a blob.
    Remove,
}

impl Action {
    /// The byte.
    pub fn to_byte(self) -> u8 {
        match self {
            Self::Write => 1,
            Self::Read => 2,
            Self::List => 3,
            Self::Remove => 4,
        }
    }

    /// The action for a byte.
    pub fn from_byte(b: u8) -> Option<Self> {
        [Self::Write, Self::Read, Self::List, Self::Remove]
            .into_iter()
            .find(|a| a.to_byte() == b)
    }
}

/// One part of a blob, with what names the whole.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Part {
    /// This part's number, from 0.
    pub number: u32,
    /// The number of parts, from 1 to [`MAX_PARTS`].
    pub parts: u32,
    /// The SHA-256 of the whole blob.
    pub sha256: [u8; 32],
    /// The bytes, at most [`MAX_PART_LEN`].
    pub bytes: Vec<u8>,
}

/// What a Store carries after its request ID (LK-83).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoreBody {
    /// One part of the blob called `name`.
    Write {
        /// The name.
        name: String,
        /// The part.
        part: Part,
    },
    /// The part of the blob called `name` with this number.
    Read {
        /// The name.
        name: String,
        /// The part's number.
        part: u32,
    },
    /// The names after `after`, in byte order; empty `after` lists from the
    /// start.
    List {
        /// The name to list after, or empty.
        after: String,
    },
    /// Remove the blob called `name`.
    Remove {
        /// The name.
        name: String,
    },
}

/// The content of a Store (LK-83).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreMessage {
    /// 16 random bytes the answer names again.
    pub request_id: [u8; 16],
    /// What is asked.
    pub body: StoreBody,
}

fn put_name(out: &mut Vec<u8>, name: &str) {
    out.push(u8::try_from(name.len()).unwrap_or(u8::MAX));
    out.extend_from_slice(name.as_bytes());
}

fn read_name(r: &mut Reader<'_>, may_be_empty: bool) -> Result<String, DecodeError> {
    let n = usize::from(r.u8()?);
    let name = std::str::from_utf8(r.take(n)?)
        .map_err(|_| DecodeError::NonCanonical("a name is not text"))?;
    if name.is_empty() && may_be_empty {
        return Ok(String::new());
    }
    if !name_ok(name) {
        return Err(DecodeError::NonCanonical(
            "a name is not 1 to 255 of the node's characters",
        ));
    }
    Ok(name.to_string())
}

fn put_part_len(out: &mut Vec<u8>, bytes: &[u8]) {
    out.extend_from_slice(&wire::len_u32(bytes.len()).to_le_bytes());
    out.extend_from_slice(bytes);
}

fn read_part_bytes(r: &mut Reader<'_>) -> Result<Vec<u8>, DecodeError> {
    let n = usize::try_from(r.u32_le()?).map_err(|_| DecodeError::TooLarge)?;
    if n > MAX_PART_LEN {
        return Err(DecodeError::TooLarge);
    }
    Ok(r.take(n)?.to_vec())
}

/// The numbers of a part: below the count, the count from 1, and a part that
/// is empty only when it is the whole blob.
fn check_part_numbers(number: u32, parts: u32, len: usize) -> Result<(), DecodeError> {
    if parts == 0 || parts > MAX_PARTS {
        return Err(DecodeError::NonCanonical(
            "the number of parts is not 1 to 280",
        ));
    }
    if number >= parts {
        return Err(DecodeError::NonCanonical(
            "the part's number is not below the count",
        ));
    }
    if len == 0 && parts != 1 {
        return Err(DecodeError::NonCanonical("an empty part of a longer blob"));
    }
    Ok(())
}

impl StoreMessage {
    /// The content bytes. A message built by hand that breaks a limit is
    /// not checked here; [`StoreMessage::from_bytes`] refuses it.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = wire::start(&MAGIC_STORE, 64);
        out.extend_from_slice(&self.request_id);
        match &self.body {
            StoreBody::Write { name, part } => {
                out.push(Action::Write.to_byte());
                put_name(&mut out, name);
                out.extend_from_slice(&part.number.to_le_bytes());
                out.extend_from_slice(&part.parts.to_le_bytes());
                out.extend_from_slice(&part.sha256);
                put_part_len(&mut out, &part.bytes);
            }
            StoreBody::Read { name, part } => {
                out.push(Action::Read.to_byte());
                put_name(&mut out, name);
                out.extend_from_slice(&part.to_le_bytes());
            }
            StoreBody::List { after } => {
                out.push(Action::List.to_byte());
                put_name(&mut out, after);
            }
            StoreBody::Remove { name } => {
                out.push(Action::Remove.to_byte());
                put_name(&mut out, name);
            }
        }
        out
    }

    /// Read the content strictly.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, DecodeError> {
        let mut r = Reader::framed(bytes, &MAGIC_STORE)?;
        let request_id = r.array::<16>()?;
        let action = Action::from_byte(r.u8()?)
            .ok_or(DecodeError::NonCanonical("the action is not a choice"))?;
        let body = match action {
            Action::Write => {
                let name = read_name(&mut r, false)?;
                let number = r.u32_le()?;
                let parts = r.u32_le()?;
                let sha256 = r.array::<32>()?;
                let bytes = read_part_bytes(&mut r)?;
                check_part_numbers(number, parts, bytes.len())?;
                StoreBody::Write {
                    name,
                    part: Part {
                        number,
                        parts,
                        sha256,
                        bytes,
                    },
                }
            }
            Action::Read => {
                let name = read_name(&mut r, false)?;
                let part = r.u32_le()?;
                if part >= MAX_PARTS {
                    return Err(DecodeError::NonCanonical(
                        "the part asked for is past the last",
                    ));
                }
                StoreBody::Read { name, part }
            }
            Action::List => StoreBody::List {
                after: read_name(&mut r, true)?,
            },
            Action::Remove => StoreBody::Remove {
                name: read_name(&mut r, false)?,
            },
        };
        r.finish()?;
        Ok(Self { request_id, body })
    }

    /// The action.
    pub fn action(&self) -> Action {
        match self.body {
            StoreBody::Write { .. } => Action::Write,
            StoreBody::Read { .. } => Action::Read,
            StoreBody::List { .. } => Action::List,
            StoreBody::Remove { .. } => Action::Remove,
        }
    }
}

/// Why a Store was refused, as the Box says it (LK-83).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoreCause {
    /// Eight writes are in progress already (BX-91).
    TooManyWrites,
    /// The write would pass 1 GiB (BX-92).
    StoreFull,
    /// The write would pass 10,000 names (BX-92).
    TooManyNames,
    /// The blob the parts make does not have the SHA-256 the parts name
    /// (BX-91).
    WrongSha,
    /// The part is not the one that follows the last, or names another
    /// count or SHA-256 than the write in progress (BX-91).
    PartOutOfOrder,
    /// The blob would pass 16 MiB (LK-83).
    BlobTooLarge,
    /// The Box could not write or read its files.
    Storage,
    /// The name is too long to be a file name in hexadecimal (BX-93): more
    /// than [`MAX_STORED_NAME_LEN`] bytes.
    NameTooLong,
}

impl StoreCause {
    /// Every cause.
    pub const ALL: [StoreCause; 8] = [
        StoreCause::TooManyWrites,
        StoreCause::StoreFull,
        StoreCause::TooManyNames,
        StoreCause::WrongSha,
        StoreCause::PartOutOfOrder,
        StoreCause::BlobTooLarge,
        StoreCause::Storage,
        StoreCause::NameTooLong,
    ];

    /// The code (never 0).
    pub fn to_byte(self) -> u8 {
        match self {
            Self::TooManyWrites => 1,
            Self::StoreFull => 2,
            Self::TooManyNames => 3,
            Self::WrongSha => 4,
            Self::PartOutOfOrder => 5,
            Self::BlobTooLarge => 6,
            Self::Storage => 7,
            Self::NameTooLong => 8,
        }
    }

    /// The cause for a code.
    pub fn from_byte(b: u8) -> Option<Self> {
        Self::ALL.into_iter().find(|c| c.to_byte() == b)
    }
}

impl fmt::Display for StoreCause {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let what = match self {
            Self::TooManyWrites => "BX-91: eight writes are in progress already",
            Self::StoreFull => "BX-92: the store holds 1 GiB",
            Self::TooManyNames => "BX-92: the store holds 10,000 names",
            Self::WrongSha => "BX-91: the blob does not have the SHA-256 the parts name",
            Self::PartOutOfOrder => "LK-83: the part is not the one that follows",
            Self::BlobTooLarge => "LK-83: the blob is over 16 MiB",
            Self::Storage => "BX-91: the Box could not write its files",
            Self::NameTooLong => "BX-93: the name is too long for a file name in hexadecimal",
        };
        write!(f, "{what}")
    }
}

/// What a Stored carries after its request ID and action (LK-83).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoredBody {
    /// A part of a write is taken; more follow.
    Taken {
        /// The part's number.
        part: u32,
    },
    /// The last part is in, the blob is whole and in place: the SHA-256 of
    /// what the Box stored.
    Written {
        /// The SHA-256.
        sha256: [u8; 32],
    },
    /// A part of a read.
    Part(Part),
    /// A read of a name the Box does not hold.
    None,
    /// The names that fit, in byte order, and whether more follow.
    Names {
        /// The names.
        names: Vec<String>,
        /// Whether a list after the last name has more.
        more: bool,
    },
    /// The blob is gone, or was not there.
    Removed,
    /// Refused, and why.
    Refused(StoreCause),
}

/// The content of a Stored (LK-83).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredMessage {
    /// The request ID of the Store this answers.
    pub request_id: [u8; 16],
    /// The action of the Store this answers.
    pub action: Action,
    /// The answer.
    pub body: StoredBody,
}

const R_TAKEN: u8 = 1;
const R_WRITTEN: u8 = 2;
const R_PART: u8 = 3;
const R_NONE: u8 = 4;
const R_NAMES: u8 = 5;
const R_REMOVED: u8 = 6;
const R_REFUSED: u8 = 7;

impl StoredBody {
    fn result(&self) -> u8 {
        match self {
            Self::Taken { .. } => R_TAKEN,
            Self::Written { .. } => R_WRITTEN,
            Self::Part(_) => R_PART,
            Self::None => R_NONE,
            Self::Names { .. } => R_NAMES,
            Self::Removed => R_REMOVED,
            Self::Refused(_) => R_REFUSED,
        }
    }

    /// Whether LK-83 names this answer for this action.
    fn fits(&self, action: Action) -> bool {
        matches!(
            (action, self),
            (Action::Write, Self::Taken { .. } | Self::Written { .. })
                | (Action::Read, Self::Part(_) | Self::None)
                | (Action::List, Self::Names { .. })
                | (Action::Remove, Self::Removed)
                | (_, Self::Refused(_))
        )
    }
}

impl StoredMessage {
    /// The content bytes.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = wire::start(&MAGIC_STORED, 64);
        out.extend_from_slice(&self.request_id);
        out.push(self.action.to_byte());
        out.push(self.body.result());
        match &self.body {
            StoredBody::Taken { part } => out.extend_from_slice(&part.to_le_bytes()),
            StoredBody::Written { sha256 } => out.extend_from_slice(sha256),
            StoredBody::Part(p) => {
                out.extend_from_slice(&p.number.to_le_bytes());
                out.extend_from_slice(&p.parts.to_le_bytes());
                out.extend_from_slice(&p.sha256);
                put_part_len(&mut out, &p.bytes);
            }
            StoredBody::None | StoredBody::Removed => {}
            StoredBody::Names { names, more } => {
                out.extend_from_slice(&wire::len_u32(names.len()).to_le_bytes());
                for n in names {
                    put_name(&mut out, n);
                }
                out.push(u8::from(*more));
            }
            StoredBody::Refused(cause) => out.push(cause.to_byte()),
        }
        out
    }

    /// Read the content strictly.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, DecodeError> {
        let mut r = Reader::framed(bytes, &MAGIC_STORED)?;
        let request_id = r.array::<16>()?;
        let action = Action::from_byte(r.u8()?)
            .ok_or(DecodeError::NonCanonical("the action is not a choice"))?;
        let body = match r.u8()? {
            R_TAKEN => StoredBody::Taken { part: r.u32_le()? },
            R_WRITTEN => StoredBody::Written {
                sha256: r.array::<32>()?,
            },
            R_PART => {
                let number = r.u32_le()?;
                let parts = r.u32_le()?;
                let sha256 = r.array::<32>()?;
                let bytes = read_part_bytes(&mut r)?;
                check_part_numbers(number, parts, bytes.len())?;
                StoredBody::Part(Part {
                    number,
                    parts,
                    sha256,
                    bytes,
                })
            }
            R_NONE => StoredBody::None,
            R_NAMES => {
                // Every name is at least 2 bytes with its length.
                let n = r.count(MAX_LIST_BYTES / 2, 2)?;
                let mut names: Vec<String> = Vec::with_capacity(n);
                let mut total = 0usize;
                for _ in 0..n {
                    let name = read_name(&mut r, false)?;
                    total += 1 + name.len();
                    if total > MAX_LIST_BYTES {
                        return Err(DecodeError::TooLarge);
                    }
                    if names.last().is_some_and(|prev| *prev >= name) {
                        return Err(DecodeError::NonCanonical(
                            "names are not in ascending order",
                        ));
                    }
                    names.push(name);
                }
                StoredBody::Names {
                    names,
                    more: r.flag()?,
                }
            }
            R_REMOVED => StoredBody::Removed,
            R_REFUSED => StoredBody::Refused(
                StoreCause::from_byte(r.u8()?)
                    .ok_or(DecodeError::NonCanonical("the cause is not a choice"))?,
            ),
            _ => return Err(DecodeError::NonCanonical("the result is not a choice")),
        };
        if !body.fits(action) {
            return Err(DecodeError::NonCanonical(
                "the answer is not one of this action's",
            ));
        }
        r.finish()?;
        Ok(Self {
            request_id,
            action,
            body,
        })
    }
}

/// Why Lightning is not offered, as the report says it (LK-84, SL-41).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotOffered {
    /// It is offered.
    Offered,
    /// The second device is away.
    SecondDeviceAway,
    /// The chain data is behind.
    ChainBehind,
    /// No channel can be used.
    NoUsableChannel,
    /// The provider does not answer.
    ProviderSilent,
}

impl NotOffered {
    /// The `u16` code: 0 when offered.
    pub fn to_code(self) -> u16 {
        match self {
            Self::Offered => 0,
            Self::SecondDeviceAway => 1,
            Self::ChainBehind => 2,
            Self::NoUsableChannel => 3,
            Self::ProviderSilent => 4,
        }
    }

    /// The cause for a code.
    pub fn from_code(code: u16) -> Option<Self> {
        [
            Self::Offered,
            Self::SecondDeviceAway,
            Self::ChainBehind,
            Self::NoUsableChannel,
            Self::ProviderSilent,
        ]
        .into_iter()
        .find(|c| c.to_code() == code)
    }
}

/// The facts the Lightning counter reports with each check-in (LK-84,
/// SL-78). The cap and the provider are not in it: the owner phone shows
/// those of its own settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LightningReport {
    /// The Lightning balance, in satoshis.
    pub balance_sats: u64,
    /// The room to receive, in satoshis.
    pub receive_room_sats: u64,
    /// The opening fees paid so far, in satoshis.
    pub opening_fees_paid_sats: u64,
    /// When the Box last stored the state, in seconds since 1970; 0 if it
    /// never has.
    pub last_stored_at: u64,
    /// The time of the last sweep, in seconds since 1970; 0 if there was
    /// none.
    pub last_sweep_at: u64,
    /// Why Lightning is not offered, or that it is.
    pub not_offered: NotOffered,
}

impl LightningReport {
    /// The content bytes.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = wire::start(&MAGIC_LIGHTNING_REPORT, 48);
        for n in [
            self.balance_sats,
            self.receive_room_sats,
            self.opening_fees_paid_sats,
            self.last_stored_at,
            self.last_sweep_at,
        ] {
            out.extend_from_slice(&n.to_le_bytes());
        }
        out.extend_from_slice(&self.not_offered.to_code().to_le_bytes());
        out
    }

    /// Read the content strictly.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, DecodeError> {
        let mut r = Reader::framed(bytes, &MAGIC_LIGHTNING_REPORT)?;
        let report = Self {
            balance_sats: r.u64_le()?,
            receive_room_sats: r.u64_le()?,
            opening_fees_paid_sats: r.u64_le()?,
            last_stored_at: r.u64_le()?,
            last_sweep_at: r.u64_le()?,
            not_offered: NotOffered::from_code(r.u16_le()?)
                .ok_or(DecodeError::NonCanonical("the cause code is not a choice"))?,
        };
        r.finish()?;
        Ok(report)
    }
}
