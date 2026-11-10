//! The refusals, each saying its rule in words.

use std::fmt;

use openbmt_core::identity::DeviceId;
use openbmt_core::records::ChainError;
use openbmt_core::wire::DecodeError;

use crate::model::SaleKey;

/// The counter's key as hex, for a message.
pub(crate) fn hex_id(id: &DeviceId) -> String {
    id.to_bytes().iter().map(|b| format!("{b:02x}")).collect()
}

/// Why the books were not built.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BooksError {
    /// BK-01: the counter is not on the signed device list.
    CounterNotOnList {
        /// The counter.
        counter: DeviceId,
    },
    /// BK-01: the same counter was given twice.
    DuplicateCounter {
        /// The counter.
        counter: DeviceId,
    },
    /// BK-01: the counter's chain did not verify.
    BadChain {
        /// The counter.
        counter: DeviceId,
        /// What failed, with the record's number.
        error: ChainError,
    },
    /// BK-03: the body is not an event the counter writes.
    BadEvent {
        /// The counter.
        counter: DeviceId,
        /// The record.
        seq: u64,
        /// What the decoder said.
        error: DecodeError,
    },
    /// BK-03: the event is of another kind than its record.
    KindMismatch {
        /// The counter.
        counter: DeviceId,
        /// The record.
        seq: u64,
    },
    /// BK-04: an event for a sale that was never opened.
    UnknownSale {
        /// The counter.
        counter: DeviceId,
        /// The record.
        seq: u64,
        /// The sale index.
        index: u32,
    },
    /// BK-04: a sale opened twice.
    OpenedTwice {
        /// The counter.
        counter: DeviceId,
        /// The record.
        seq: u64,
        /// The sale index.
        index: u32,
    },
    /// BK-04, BK-16: a time that is not a date from 1970 to 9999.
    TimeOutOfRange {
        /// The time in Unix seconds.
        at: u64,
    },
    /// BK-10, BK-12, BK-13: a line names a sale that is not in the books.
    SaleNotInBooks {
        /// The sale it names.
        sale: SaleKey,
    },
    /// BK-14: a sum does not fit.
    Overflow,
    /// BK-14: more lines than the limit.
    TooMany,
    /// BK-16: an offset outside -720 to 840 minutes.
    BadOffset {
        /// The offset given.
        minutes: i32,
    },
}

impl fmt::Display for BooksError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CounterNotOnList { counter } => write!(
                f,
                "BK-01: counter {} is not on the signed device list",
                hex_id(counter)
            ),
            Self::DuplicateCounter { counter } => {
                write!(f, "BK-01: counter {} was given twice", hex_id(counter))
            }
            Self::BadChain { counter, error } => write!(
                f,
                "BK-01: the records of counter {} are refused: {error}",
                hex_id(counter)
            ),
            Self::BadEvent {
                counter,
                seq,
                error,
            } => write!(
                f,
                "BK-03: record {seq} of counter {} is not an event: {error}",
                hex_id(counter)
            ),
            Self::KindMismatch { counter, seq } => write!(
                f,
                "BK-03: record {seq} of counter {} holds an event of another kind",
                hex_id(counter)
            ),
            Self::UnknownSale {
                counter,
                seq,
                index,
            } => write!(
                f,
                "BK-04: record {seq} of counter {} is for sale {index}, which was never opened",
                hex_id(counter)
            ),
            Self::OpenedTwice {
                counter,
                seq,
                index,
            } => write!(
                f,
                "BK-04: record {seq} of counter {} opens sale {index} a second time",
                hex_id(counter)
            ),
            Self::TimeOutOfRange { at } => write!(
                f,
                "BK-04: the time {at} is not a date from 1970 to the end of 9999"
            ),
            Self::SaleNotInBooks { sale } => write!(
                f,
                "BK-12: a line names sale {} of counter {}, which is not in the books",
                sale.index,
                hex_id(&sale.counter)
            ),
            Self::Overflow => write!(f, "BK-14: a sum does not fit in 64 bits"),
            Self::TooMany => write!(f, "BK-14: more lines than the limit"),
            Self::BadOffset { minutes } => write!(
                f,
                "BK-16: an offset of {minutes} minutes is outside -720 to 840"
            ),
        }
    }
}

impl std::error::Error for BooksError {}

/// Why an export was not made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExportError {
    /// BK-17: a disposal has no dollar value, and the file does not guess.
    DisposalValueUnknown {
        /// The disposal's place in the list, from 0.
        position: usize,
    },
    /// BK-17: lines in more than one currency, and the file has no currency
    /// column.
    MixedCurrencies,
    /// BK-16: a time that is not a date.
    TimeOutOfRange {
        /// The time in Unix seconds.
        at: u64,
    },
    /// BK-13: a return names a sale that is not in the books.
    SaleMissing {
        /// The sale it names.
        sale: SaleKey,
    },
    /// BK-14: a sum does not fit.
    Overflow,
}

impl fmt::Display for ExportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DisposalValueUnknown { position } => write!(
                f,
                "BK-17: disposal {position} has no dollar value; supply it before exporting"
            ),
            Self::MixedCurrencies => write!(
                f,
                "BK-17: the lines are in more than one currency and the file has no column for it"
            ),
            Self::TimeOutOfRange { at } => {
                write!(f, "BK-16: the time {at} is not a date from 1970 to 9999")
            }
            Self::SaleMissing { sale } => write!(
                f,
                "BK-13: a return names sale {} of counter {}, which is not in the books",
                sale.index,
                hex_id(&sale.counter)
            ),
            Self::Overflow => write!(f, "BK-14: a sum does not fit in 64 bits"),
        }
    }
}

impl std::error::Error for ExportError {}

/// Why a file was not sealed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SealError {
    /// BK-27: the passphrase is empty.
    EmptyPassphrase,
    /// BK-25: the randomness failed, so no salt was made.
    Randomness,
    /// BK-24: the key could not be derived with these parameters.
    Key,
    /// BK-26: the ciphertext could not be made.
    Cipher,
}

impl fmt::Display for SealError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyPassphrase => write!(f, "BK-27: the passphrase is empty"),
            Self::Randomness => write!(f, "BK-25: the randomness failed, so no file was made"),
            Self::Key => write!(f, "BK-24: the key could not be derived"),
            Self::Cipher => write!(f, "BK-26: the file could not be encrypted"),
        }
    }
}

impl std::error::Error for SealError {}

/// Why a file was not opened. Whatever was wrong with the file or the
/// passphrase, the answer is the same one (BK-26).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenError {
    /// BK-27: the passphrase is empty.
    EmptyPassphrase,
    /// BK-26: wrong passphrase or damaged file.
    WrongPassphraseOrDamaged,
}

impl fmt::Display for OpenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyPassphrase => write!(f, "BK-27: the passphrase is empty"),
            Self::WrongPassphraseOrDamaged => {
                write!(f, "BK-26: wrong passphrase or damaged file")
            }
        }
    }
}

impl std::error::Error for OpenError {}
