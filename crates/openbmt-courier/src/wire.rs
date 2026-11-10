//! The contents of the two kinds that carry the chain (LK-54, LK-55).
//!
//! A Records message holds whole records, oldest first, each as its length
//! (4 bytes, little endian) and the bytes of `Record::to_bytes`. A Records
//! received message is the number of records the owner holds (8 bytes, little
//! endian) and the 32-byte head after them.

use std::fmt;

use openbmt_core::records::{Record, GENESIS};
use openbmt_core::wire::{DecodeError, Reader};
use openbmt_link::seal::MAX_CONTENT_LEN;

/// Bytes of a Records received content: the count and the head.
pub const RECEIVED_LEN: usize = 8 + 32;

/// Why a content was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WireError {
    /// A Records content with no record in it.
    Empty,
    /// The content stops inside a length or a record.
    Cut,
    /// A record in the content does not parse.
    Record(DecodeError),
    /// A Records received content that is not 40 bytes.
    ReceivedLength(usize),
}

impl fmt::Display for WireError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => write!(f, "LK-54: the message holds no record"),
            Self::Cut => write!(f, "LK-54: the message stops inside a record"),
            Self::Record(e) => write!(f, "LK-54: a record in the message does not parse: {e}"),
            Self::ReceivedLength(n) => {
                write!(f, "LK-55: {n} bytes, not the 40 of a count and a head")
            }
        }
    }
}

impl std::error::Error for WireError {}

/// The content of a Records message for the front of `records`: as many whole
/// records as fit in `max_content` bytes (and in one message), and how many
/// that is. If the first record alone is too long, the count is 0 and the
/// content is empty.
pub fn records_content(records: &[Record], max_content: usize) -> (Vec<u8>, usize) {
    let limit = max_content.min(MAX_CONTENT_LEN);
    let mut out = Vec::new();
    let mut count = 0usize;
    for record in records {
        let bytes = record.to_bytes();
        let Ok(len) = u32::try_from(bytes.len()) else {
            break;
        };
        let next = out.len().saturating_add(4).saturating_add(bytes.len());
        if next > limit {
            break;
        }
        out.extend_from_slice(&len.to_le_bytes());
        out.extend_from_slice(&bytes);
        count = count.saturating_add(1);
    }
    (out, count)
}

/// Read a Records content strictly (LK-54).
pub fn decode_records(content: &[u8]) -> Result<Vec<Record>, WireError> {
    if content.is_empty() {
        return Err(WireError::Empty);
    }
    let mut r = Reader::new(content);
    let mut out = Vec::new();
    let mut left = content.len();
    while left > 0 {
        let len = r.u32_le().map_err(|_| WireError::Cut)?;
        let len = usize::try_from(len).map_err(|_| WireError::Cut)?;
        let bytes = r.take(len).map_err(|_| WireError::Cut)?;
        out.push(Record::from_bytes(bytes).map_err(WireError::Record)?);
        left = left.saturating_sub(4).saturating_sub(len);
    }
    Ok(out)
}

/// The content of a Records received message (LK-55).
pub fn received_content(count: u64, head: &[u8; 32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(RECEIVED_LEN);
    out.extend_from_slice(&count.to_le_bytes());
    out.extend_from_slice(head);
    out
}

/// Read a Records received content: the count and the head (LK-55).
pub fn decode_received(content: &[u8]) -> Result<(u64, [u8; 32]), WireError> {
    if content.len() != RECEIVED_LEN {
        return Err(WireError::ReceivedLength(content.len()));
    }
    let mut r = Reader::new(content);
    let count = r.u64_le().map_err(|_| WireError::Cut)?;
    let head = r.array::<32>().map_err(|_| WireError::Cut)?;
    Ok((count, head))
}

/// The head after the first `count` records of `records`: the hash of record
/// `count - 1`, or [`GENESIS`] for none. `None` if there are fewer than
/// `count` records.
pub fn head_at(records: &[Record], count: u64) -> Option<[u8; 32]> {
    if count == 0 {
        return Some(GENESIS);
    }
    let at = usize::try_from(count.checked_sub(1)?).ok()?;
    records.get(at).map(Record::hash)
}
