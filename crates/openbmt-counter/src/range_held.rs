//! The record that says the counter holds a range of indexes (CR-235).
//!
//! It is the range's counterpart of [`crate::batch_held`] and is written in
//! the same order and read as strictly (CR-167 to CR-169). The record is of
//! kind `Setting`. Its body is one tag byte (35; 34 is a held batch), the
//! first index of the range (u32 LE), the first index after it (u32 LE) and
//! a 32-byte digest of the signed range.

use bitcoin::hashes::{sha256, Hash, HashEngine};
use openbmt_core::pinned_wallet::SignedRange;
use openbmt_core::records::RecordKind;
use openbmt_core::wire::{DecodeError, Reader};

const TAG_RANGE_HELD: u8 = 35;

/// The domain of the digest of a signed range.
pub const TAG_RANGE_DIGEST: &str = "OpenBMT/range-held/v1";

/// The digest of a signed range: SHA-256 of [`TAG_RANGE_DIGEST`] and then
/// its wire bytes (`OBM*`), so that it covers the counter, the range and the
/// signature. The owner computes the same value for the range it keeps.
pub fn range_digest(signed: &SignedRange) -> [u8; 32] {
    let mut engine = sha256::Hash::engine();
    engine.input(TAG_RANGE_DIGEST.as_bytes());
    engine.input(&signed.to_bytes());
    sha256::Hash::from_engine(engine).to_byte_array()
}

/// What the counter writes when it takes a range (CR-235).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct RangeHeld {
    /// First index of the range.
    pub start: u32,
    /// The first index after the range.
    pub end: u32,
    /// [`range_digest`] of the signed range.
    pub digest: [u8; 32],
}

impl RangeHeld {
    /// What a signed range is held as.
    pub fn of(signed: &SignedRange) -> Self {
        Self {
            start: signed.range.start,
            end: signed.range.end,
            digest: range_digest(signed),
        }
    }

    /// The kind of record it is written as: `Setting` (CR-235).
    pub fn kind(&self) -> RecordKind {
        RecordKind::Setting
    }

    /// The record body.
    pub fn encode(&self) -> Vec<u8> {
        let mut out = vec![TAG_RANGE_HELD];
        out.extend_from_slice(&self.start.to_le_bytes());
        out.extend_from_slice(&self.end.to_le_bytes());
        out.extend_from_slice(&self.digest);
        out
    }

    /// Read a record body strictly (CR-235, CR-169).
    pub fn decode(bytes: &[u8]) -> Result<Self, DecodeError> {
        let mut r = Reader::new(bytes);
        if r.u8()? != TAG_RANGE_HELD {
            return Err(DecodeError::NonCanonical("a range record's tag is not 35"));
        }
        let start = r.u32_le()?;
        let end = r.u32_le()?;
        let digest = r.array::<32>()?;
        r.finish()?;
        if end <= start {
            return Err(DecodeError::NonCanonical(
                "a range record's end is not above its start",
            ));
        }
        Ok(Self { start, end, digest })
    }
}
