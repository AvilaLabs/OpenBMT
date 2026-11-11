//! The record that says the counter holds a batch of addresses (CR-167 to
//! CR-169).
//!
//! The owner sends a signed batch again until the counter's records, which it
//! receives, show that the counter took it. The record is of kind `Setting`,
//! which no sale, payment, refund, override or cashier reader takes. Its body
//! is one tag byte (34; the sale events use 1 to 19 and the cashier changes
//! 32 and 33), the first index of the batch (u32 LE), the first index after it
//! (u32 LE) and a 32-byte digest of the signed batch. The decoder is strict,
//! and re-encoding a decoded body gives the input.

use bitcoin::hashes::{sha256, Hash, HashEngine};
use openbmt_core::batch::SignedBatch;
use openbmt_core::records::RecordKind;
use openbmt_core::wire::{DecodeError, Reader};

const TAG_BATCH_HELD: u8 = 34;

/// The domain of the digest of a signed batch.
pub const TAG_BATCH_DIGEST: &str = "OpenBMT/batch-held/v1";

/// The digest of a signed batch: SHA-256 of [`TAG_BATCH_DIGEST`] and then its
/// wire bytes, so that it covers the range, the scripts and both signatures. The owner computes the
/// same value for the batch it keeps.
pub fn batch_digest(signed: &SignedBatch) -> [u8; 32] {
    let mut engine = sha256::Hash::engine();
    engine.input(TAG_BATCH_DIGEST.as_bytes());
    engine.input(&signed.to_bytes());
    sha256::Hash::from_engine(engine).to_byte_array()
}

/// What the counter writes when it takes a batch (CR-167).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct BatchHeld {
    /// First receive index of the batch.
    pub start: u32,
    /// The first index after the batch.
    pub end: u32,
    /// [`batch_digest`] of the signed batch.
    pub digest: [u8; 32],
}

impl BatchHeld {
    /// What a signed batch is held as, or `None` for a range that does not
    /// end below the index limit.
    pub fn of(signed: &SignedBatch) -> Option<Self> {
        Some(Self {
            start: signed.batch.start,
            end: signed.batch.end()?,
            digest: batch_digest(signed),
        })
    }

    /// The kind of record it is written as: `Setting` (CR-167).
    pub fn kind(&self) -> RecordKind {
        RecordKind::Setting
    }

    /// The record body.
    pub fn encode(&self) -> Vec<u8> {
        let mut out = vec![TAG_BATCH_HELD];
        out.extend_from_slice(&self.start.to_le_bytes());
        out.extend_from_slice(&self.end.to_le_bytes());
        out.extend_from_slice(&self.digest);
        out
    }

    /// Read a record body strictly (CR-169).
    pub fn decode(bytes: &[u8]) -> Result<Self, DecodeError> {
        let mut r = Reader::new(bytes);
        if r.u8()? != TAG_BATCH_HELD {
            return Err(DecodeError::NonCanonical("a batch record's tag is not 34"));
        }
        let start = r.u32_le()?;
        let end = r.u32_le()?;
        let digest = r.array::<32>()?;
        r.finish()?;
        if end <= start {
            return Err(DecodeError::NonCanonical(
                "a batch record's end is not above its start",
            ));
        }
        Ok(Self { start, end, digest })
    }
}
