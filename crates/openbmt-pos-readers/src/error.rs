//! Why a reply is refused and why a request is not made. Every `Display`
//! begins with the rule's number; a person is shown the words in `say.rs`.
//! The `&'static str` of a refusal names the field of the reply it is about.

use std::fmt;

use openbmt_json::JsonError;
use openbmt_pos::{ModelError, PosError};

/// PS-90 to PS-94, PS-96, PS-98: why a reply is refused whole.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadRefused {
    /// More bytes than the reader takes (PS-90).
    TooLong,
    /// Arrays and objects nested deeper than the reader takes (PS-90).
    TooDeep,
    /// More values than the reader takes (PS-90).
    TooManyValues,
    /// Not UTF-8 (PS-90).
    NotUtf8,
    /// Not a JSON document (PS-90).
    Malformed,
    /// Bytes after the document (PS-90).
    TrailingBytes,
    /// A key twice in one object (PS-90).
    DuplicateKey,
    /// The vendor says in its reply that it failed (PS-91).
    ServiceError,
    /// A field the reader needs is not there, or is `null` (PS-91).
    Missing(&'static str),
    /// A field is not of the type the vendor documents (PS-91).
    WrongType(&'static str),
    /// A state, kind or result that the reader does not know (PS-91).
    UnknownValue(&'static str),
    /// A number that is negative, too large or not a whole number (PS-92).
    OutOfRange(&'static str),
    /// A time that is not strictly what the vendor documents (PS-92).
    BadTime(&'static str),
    /// A currency that is not three capital letters (PS-93).
    BadCurrency(&'static str),
    /// A tender or a refund in another currency than its order (PS-93).
    CurrencyMismatch,
    /// An id that is not 1 to 64 printable ASCII characters (PS-93).
    BadId(&'static str),
    /// A text that is empty or holds a control character, or a location
    /// longer than 64 bytes (PS-93).
    BadText(&'static str),
    /// More than 500 orders in a page (PS-94).
    TooManyOrders,
    /// More than 16 tenders on an order (PS-94).
    TooManyTenders,
    /// Two tenders of one order with the same id (PS-94).
    DuplicateTender,
    /// Refunds of a tender larger than its amount and tip (PS-94).
    RefundTooLarge,
    /// A refund that names no tender of its order (PS-94).
    RefundWithoutTender,
    /// A cursor longer than the model allows (PS-94).
    CursorTooLong,
}

impl ReadRefused {
    /// The rule the refusal belongs to.
    pub fn rule(&self) -> &'static str {
        match self {
            Self::TooLong
            | Self::TooDeep
            | Self::TooManyValues
            | Self::NotUtf8
            | Self::Malformed
            | Self::TrailingBytes
            | Self::DuplicateKey => "PS-90",
            Self::ServiceError | Self::Missing(_) | Self::WrongType(_) | Self::UnknownValue(_) => {
                "PS-91"
            }
            Self::OutOfRange(_) | Self::BadTime(_) => "PS-92",
            Self::BadCurrency(_) | Self::CurrencyMismatch | Self::BadId(_) | Self::BadText(_) => {
                "PS-93"
            }
            Self::TooManyOrders
            | Self::TooManyTenders
            | Self::DuplicateTender
            | Self::RefundTooLarge
            | Self::RefundWithoutTender
            | Self::CursorTooLong => "PS-94",
        }
    }

    /// The field the refusal is about, when it names one.
    pub fn field(&self) -> Option<&'static str> {
        match self {
            Self::Missing(f)
            | Self::WrongType(f)
            | Self::UnknownValue(f)
            | Self::OutOfRange(f)
            | Self::BadTime(f)
            | Self::BadCurrency(f)
            | Self::BadId(f)
            | Self::BadText(f) => Some(f),
            _ => None,
        }
    }

    fn why(&self) -> &'static str {
        match self {
            Self::TooLong => "the reply is longer than the limit",
            Self::TooDeep => "the reply is nested deeper than the limit",
            Self::TooManyValues => "the reply holds more values than the limit",
            Self::NotUtf8 => "the reply is not UTF-8",
            Self::Malformed => "the reply is not a JSON document",
            Self::TrailingBytes => "the reply has bytes after the document",
            Self::DuplicateKey => "the reply has a key twice in one object",
            Self::ServiceError => "the reply reports an error",
            Self::Missing(_) => "a field the reader needs is not there",
            Self::WrongType(_) => "a field is not of the documented type",
            Self::UnknownValue(_) => "a field holds a value the reader does not know",
            Self::OutOfRange(_) => "a number is negative, too large or not whole",
            Self::BadTime(_) => "a time is not written as documented",
            Self::BadCurrency(_) => "a currency is not three capital letters",
            Self::CurrencyMismatch => "a tender or refund is in another currency than its order",
            Self::BadId(_) => "an id is not 1 to 64 printable ASCII characters",
            Self::BadText(_) => "a text is empty, holds a control character or is too long",
            Self::TooManyOrders => "a page holds more than 500 orders",
            Self::TooManyTenders => "an order holds more than 16 tenders",
            Self::DuplicateTender => "two tenders of one order share an id",
            Self::RefundTooLarge => "refunds are larger than the tender and its tip",
            Self::RefundWithoutTender => "a refund names no tender of its order",
            Self::CursorTooLong => "the cursor is too long",
        }
    }
}

impl fmt::Display for ReadRefused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.rule(), self.why())?;
        match self.field() {
            Some(field) => write!(f, " ({field})"),
            None => Ok(()),
        }
    }
}

impl std::error::Error for ReadRefused {}

impl From<JsonError> for ReadRefused {
    fn from(e: JsonError) -> Self {
        match e {
            JsonError::TooLong => Self::TooLong,
            JsonError::TooDeep => Self::TooDeep,
            JsonError::TooManyValues => Self::TooManyValues,
            JsonError::NotUtf8 => Self::NotUtf8,
            JsonError::Malformed => Self::Malformed,
            JsonError::TrailingBytes => Self::TrailingBytes,
            JsonError::DuplicateKey => Self::DuplicateKey,
        }
    }
}

impl From<ModelError> for ReadRefused {
    fn from(e: ModelError) -> Self {
        match e {
            ModelError::BadId => Self::BadId("id"),
            ModelError::BadText => Self::BadText("text"),
            ModelError::TooManyTenders => Self::TooManyTenders,
            ModelError::DuplicateTender => Self::DuplicateTender,
            ModelError::RefundTooLarge => Self::RefundTooLarge,
            ModelError::CursorTooLong => Self::CursorTooLong,
        }
    }
}

/// A connector reports a refused reply as an unreadable answer (PS-80).
impl From<ReadRefused> for PosError {
    fn from(e: ReadRefused) -> Self {
        PosError::BadResponse(e.why())
    }
}

/// PS-94, PS-95, PS-97: why a request is not made.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestRefused {
    /// The settings name no location to read (PS-95).
    NoLocations,
    /// The page size is 0 or more than 500 (PS-94).
    BadLimit,
    /// The cursor is not in the form of PS-94.
    BadCursor,
    /// The Clover merchant id is not 1 to 32 ASCII letters and digits
    /// (PS-97).
    BadMerchant,
}

impl RequestRefused {
    /// The rule the refusal belongs to.
    pub fn rule(&self) -> &'static str {
        match self {
            Self::NoLocations => "PS-95",
            Self::BadLimit | Self::BadCursor => "PS-94",
            Self::BadMerchant => "PS-97",
        }
    }

    fn why(&self) -> &'static str {
        match self {
            Self::NoLocations => "the settings name no location to read",
            Self::BadLimit => "the page size is 0 or more than 500",
            Self::BadCursor => {
                "the cursor is not a time, then optionally a bar and the vendor's own"
            }
            Self::BadMerchant => "the merchant id is not 1 to 32 ASCII letters and digits",
        }
    }
}

impl fmt::Display for RequestRefused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.rule(), self.why())
    }
}

impl std::error::Error for RequestRefused {}

impl From<RequestRefused> for PosError {
    fn from(e: RequestRefused) -> Self {
        PosError::BadResponse(e.why())
    }
}
