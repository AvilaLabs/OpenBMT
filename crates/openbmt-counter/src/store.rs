//! The caller's storage (CR-40).
//!
//! The library keeps no files. Everything it must remember is handed to a
//! [`Store`] before the step it belongs to takes effect.

use std::fmt;

use openbmt_core::records::ChainError;

/// Where a piece of the counter's state is kept.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Slot {
    /// The address batches (`OBMC`).
    Batches,
    /// The exposure ledger (`OBME`).
    Exposure,
    /// The pause tracker (`OBMP`).
    Pause,
    /// The counter's own state (`OBMG`).
    Desk,
    /// One sale (`OBML`), by its receive index.
    Sale(u32),
}

/// The caller's storage. A write that returns `Ok` is durable: the bytes will
/// be read back after a restart. A write that returns an error stored
/// nothing the library may rely on.
pub trait Store {
    /// Why a write failed.
    type Error;

    /// Replace the bytes kept in a slot.
    fn put(&mut self, slot: Slot, bytes: &[u8]) -> Result<(), Self::Error>;

    /// Add one record (its wire form, `OBMR`) to the end of the record log.
    fn append_record(&mut self, bytes: &[u8]) -> Result<(), Self::Error>;
}

/// Why a write did not happen (CR-40).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoreFault<E> {
    /// The store refused the bytes.
    Write(E),
    /// The record could not be made (its body is too large). The counter's
    /// own records are small, so this does not arise in practice.
    Record(ChainError),
}

impl<E: fmt::Display> fmt::Display for StoreFault<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Write(e) => write!(f, "could not store it: {e}"),
            Self::Record(e) => write!(f, "could not write the record: {e}"),
        }
    }
}

impl<E: fmt::Debug + fmt::Display> std::error::Error for StoreFault<E> {}

impl<E> From<E> for StoreFault<E> {
    fn from(e: E) -> Self {
        Self::Write(e)
    }
}
