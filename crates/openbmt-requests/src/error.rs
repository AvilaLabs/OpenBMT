//! Why a call was refused. Every error names its rule at the start of its
//! text, for logs and tests; a person is shown the words of [`crate::say`].

use std::fmt;

use openbmt_core::currency::ShopCurrency;

/// An amount limit, as the refusal says it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Limit {
    /// Satoshis.
    Sats(u64),
    /// Whole units of the shop's currency.
    Units(ShopCurrency, u64),
}

/// Why a request was not made (RQ-03, RQ-06 to RQ-09, RQ-11).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MakeRefused {
    /// RQ-06: the note is not 1 to 64 characters of plain text.
    NoteNeeded,
    /// RQ-07: a shop in Bitcoin Standard has requests in bitcoin only.
    BitcoinOnly,
    /// RQ-09: the due date is before the day of the request or more than
    /// 366 days after it.
    DueOutOfRange,
    /// RQ-11: 1,000 requests are open.
    TooManyOpen,
    /// RQ-08: the amount is under the smallest bitcoin sale or the dust
    /// line, or is zero.
    TooSmall {
        /// The least a request may ask for, in satoshis.
        min_sats: u64,
    },
    /// RQ-08: the amount is over the limit.
    TooLarge {
        /// The most a request may ask for.
        max: Limit,
    },
    /// RQ-12: the request is in the currency and there is no fresh rate.
    StaleRate,
    /// RQ-03: the address index is already a request's.
    IndexUsed,
    /// RQ-30: the book holds as many requests as a book may.
    BookFull,
}

/// Why a quote was not made (RQ-12).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShareRefused {
    /// There is no such request.
    NoSuchRequest,
    /// The request ended or is paid: nothing is asked.
    NotOpen,
    /// RQ-12: the request is in the currency and there is no fresh rate.
    StaleRate,
}

/// Why payments were not counted (RQ-15).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CreditRefused {
    /// There is no such request.
    NoSuchRequest,
    /// The payments are not in the order of the blocks.
    NotInBlockOrder,
    /// RQ-15: a payment in the currency, and no quote and no fresh rate to
    /// value it at. Nothing was counted; try again when there is a rate.
    NoRate,
    /// The request already holds as many payments as it may.
    TooManyPayments,
}

/// Why a request was not cancelled, closed or counted as paid
/// (RQ-16, RQ-19, RQ-20).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EndRefused {
    /// There is no such request.
    NoSuchRequest,
    /// It has ended already.
    AlreadyEnded,
    /// RQ-19: something has arrived, so it cannot be cancelled.
    HasPayments,
    /// RQ-20: it is not part paid, so there is nothing to close as it is.
    NotPartPaid,
    /// RQ-16: it is not almost paid.
    NotAlmost,
    /// It does not repeat.
    NoRepeat,
}

/// Why a repeat was not made (RQ-25).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepeatRefused {
    /// The series was stopped, or this is not its next request.
    NotDue,
    /// RQ-11: 1,000 requests are open.
    TooManyOpen,
    /// RQ-03: the address index is already a request's.
    IndexUsed,
    /// RQ-30: the book holds as many requests as a book may.
    BookFull,
}

/// Why a return was not queued (RQ-26 to RQ-29).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReturnRefused {
    /// There is no such request.
    NoSuchRequest,
    /// RQ-26: more than the request received less what was returned.
    TooMuch {
        /// The most that may be returned, in satoshis.
        most_sats: u64,
    },
    /// RQ-26: the amount is under what the address's kind can be paid.
    TooSmall {
        /// The least that may be paid to it, in satoshis.
        min_sats: u64,
    },
    /// RQ-26: the amount is in the currency and there is no fresh rate.
    StaleRate,
    /// RQ-28: the script is not one of a standard kind.
    BadScript,
    /// RQ-29: as many returns are queued or in a spend not yet confirmed as one
    /// spend takes.
    QueueFull,
    /// RQ-30: the book holds as many returns as a book may.
    BookFull,
}

/// Why a pasted or scanned address was refused (RQ-28, RQ-33).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddressRefused {
    /// Not an address of a standard kind, or too long.
    NotAnAddress,
    /// An address of a test network.
    TestNetwork,
    /// An address of the shop's own wallet.
    Own,
}

/// Why the request book could not be used (RQ-30, RQ-31).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BookError {
    /// The file could not be read or written.
    Io(String),
    /// The bytes are not the book, as the text says.
    Corrupt(&'static str),
}

impl fmt::Display for MakeRefused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoteNeeded => write!(f, "RQ-06: the note is 1 to 64 characters of plain text"),
            Self::BitcoinOnly => {
                write!(f, "RQ-07: a shop in Bitcoin Standard asks in bitcoin only")
            }
            Self::DueOutOfRange => write!(f, "RQ-09: the due date is today or up to 366 days on"),
            Self::TooManyOpen => write!(f, "RQ-11: 1,000 requests are open"),
            Self::TooSmall { min_sats } => {
                write!(f, "RQ-08: a request asks for at least {min_sats} sats")
            }
            Self::TooLarge { max } => write!(f, "RQ-08: a request asks for at most {max:?}"),
            Self::StaleRate => write!(f, "RQ-12: there is no fresh rate to price it with"),
            Self::IndexUsed => write!(f, "RQ-03: the address index is already a request's"),
            Self::BookFull => write!(f, "RQ-30: the book holds no more requests"),
        }
    }
}

impl fmt::Display for ShareRefused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoSuchRequest => write!(f, "RQ-12: there is no such request"),
            Self::NotOpen => write!(f, "RQ-12: the request is not open, so nothing is asked"),
            Self::StaleRate => write!(f, "RQ-12: there is no fresh rate to quote with"),
        }
    }
}

impl fmt::Display for CreditRefused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoSuchRequest => write!(f, "RQ-15: there is no such request"),
            Self::NotInBlockOrder => write!(f, "RQ-15: the payments are not in block order"),
            Self::NoRate => write!(f, "RQ-15: no quote and no fresh rate to credit at"),
            Self::TooManyPayments => {
                write!(f, "RQ-15: the request holds as many payments as it may")
            }
        }
    }
}

impl fmt::Display for EndRefused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoSuchRequest => write!(f, "RQ-19: there is no such request"),
            Self::AlreadyEnded => write!(f, "RQ-19: the request has ended"),
            Self::HasPayments => write!(f, "RQ-19: something has arrived, so it is not cancelled"),
            Self::NotPartPaid => write!(f, "RQ-20: only a part paid request is closed as it is"),
            Self::NotAlmost => write!(f, "RQ-16: only an almost paid request is counted as paid"),
            Self::NoRepeat => write!(f, "RQ-25: the request does not repeat"),
        }
    }
}

impl fmt::Display for RepeatRefused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotDue => write!(f, "RQ-25: that repeat is not the next of a running series"),
            Self::TooManyOpen => write!(f, "RQ-11: 1,000 requests are open"),
            Self::IndexUsed => write!(f, "RQ-03: the address index is already a request's"),
            Self::BookFull => write!(f, "RQ-30: the book holds no more requests"),
        }
    }
}

impl fmt::Display for ReturnRefused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoSuchRequest => write!(f, "RQ-26: there is no such request"),
            Self::TooMuch { most_sats } => {
                write!(f, "RQ-26: at most {most_sats} sats may be returned")
            }
            Self::TooSmall { min_sats } => {
                write!(f, "RQ-26: the address takes at least {min_sats} sats")
            }
            Self::StaleRate => write!(f, "RQ-26: there is no fresh rate to value the amount at"),
            Self::BadScript => write!(f, "RQ-28: the address is not of a standard kind"),
            Self::QueueFull => write!(f, "RQ-29: as many returns wait as one payment takes"),
            Self::BookFull => write!(f, "RQ-30: the book holds no more returns"),
        }
    }
}

impl fmt::Display for AddressRefused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotAnAddress => write!(f, "RQ-28: not an address of a standard kind"),
            Self::TestNetwork => write!(f, "RQ-28: an address of a test network"),
            Self::Own => write!(f, "RQ-28: an address of the shop's own wallet"),
        }
    }
}

impl fmt::Display for BookError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "RQ-30: the request book: {e}"),
            Self::Corrupt(why) => write!(f, "RQ-31: the request book is bad: {why}"),
        }
    }
}

macro_rules! errors {
    ($($t:ty),*) => { $(impl std::error::Error for $t {})* };
}
errors!(
    MakeRefused,
    ShareRefused,
    CreditRefused,
    EndRefused,
    RepeatRefused,
    ReturnRefused,
    AddressRefused,
    BookError
);

/// Why a return could not be ended as cancelled (RQ-29). The owner never
/// sees it: the book follows the queue, which only offers a cancel for a
/// refund that waits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CancelReturnRefused {
    /// There is no such return.
    NoSuchReturn,
    /// The return is not waiting in the queue.
    NotQueued,
}

impl fmt::Display for CancelReturnRefused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoSuchReturn => write!(f, "RQ-29: there is no such return"),
            Self::NotQueued => write!(f, "RQ-29: the return is not waiting in the queue"),
        }
    }
}

impl std::error::Error for CancelReturnRefused {}
