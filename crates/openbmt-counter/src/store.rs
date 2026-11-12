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
    /// The ranges of a counter paired with the shop's own wallet (`OBM;`,
    /// CR-234).
    Ranges,
    /// The exposure ledger (`OBME`).
    Exposure,
    /// The pause tracker (`OBMP`).
    Pause,
    /// The counter's own state (`OBMG`).
    Desk,
    /// One sale (`OBML`), by its receive index.
    Sale(u32),
    /// One swap of a sale (CR-220), by the sale's receive index and the
    /// swap's number within the sale. What it holds is the node's own record;
    /// the counter's library never reads it.
    Swap(u32, u32),
}

/// The caller's storage. A write that returns `Ok` is durable: the bytes will
/// be read back after a restart. A write that returns an error stored
/// nothing the library may rely on.
pub trait Store {
    /// Why a write failed.
    type Error;

    /// Replace the bytes kept in a slot.
    fn put(&mut self, slot: Slot, bytes: &[u8]) -> Result<(), Self::Error>;

    /// Remove what is kept in a slot (CR-77). A slot that holds nothing is
    /// removed without error. A call that returns `Ok` is durable: after a
    /// restart the slot reads back as empty.
    fn delete(&mut self, slot: Slot) -> Result<(), Self::Error>;

    /// Add one record (its wire form, `OBMR`) to the end of the record log.
    fn append_record(&mut self, bytes: &[u8]) -> Result<(), Self::Error>;

    /// Read records back from the record log (CR-81): the wire forms of the
    /// records numbered `from` up, oldest first, at most `max` of them. Fewer
    /// come back where the log ends; none when `from` is past the end. The
    /// caller checks what it gets (CR-42).
    fn read_records(&self, from: u64, max: usize) -> Result<Vec<Vec<u8>>, Self::Error>;
}

/// Why a write did not happen (CR-40).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoreFault<E> {
    /// The store refused the bytes.
    Write(E),
    /// The store could not read records back (CR-81).
    Read(E),
    /// A record read back from the store is not what the counter wrote
    /// (CR-42, CR-81): it does not parse, is out of place, does not follow
    /// the one before, or is not signed by this counter.
    Damaged {
        /// The number of the record, as asked for.
        seq: u64,
        /// What is wrong with it.
        why: &'static str,
    },
    /// The record could not be made (its body is too large). The counter's
    /// own records are small, so this does not arise in practice.
    Record(ChainError),
}

impl<E: fmt::Display> fmt::Display for StoreFault<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Write(e) => write!(f, "could not store it: {e}"),
            Self::Read(e) => write!(f, "could not read the records back: {e}"),
            Self::Damaged { seq, why } => {
                write!(f, "stored record {seq} is damaged: {why}")
            }
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
