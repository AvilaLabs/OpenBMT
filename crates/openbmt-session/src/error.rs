//! What can go wrong, each naming its rule.

use std::fmt;

/// Why a request or a response could not be built.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EncodeError {
    /// AS-02: the file would be above the size limit.
    TooLarge,
    /// AS-04: no items, or more than the limit.
    ItemCount,
    /// AS-04: item numbers are not strictly ascending.
    ItemOrder,
    /// AS-07, AS-12: a text breaks the text rule.
    BadText(&'static str),
    /// AS-06 to AS-11: an item is not one the file can hold.
    BadItem(&'static str),
    /// AS-12: a payload is not the size or kind its result needs.
    BadPayload(&'static str),
}

impl fmt::Display for EncodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooLarge => write!(f, "AS-02: the file would be above 8 MiB"),
            Self::ItemCount => write!(f, "AS-04: a session holds 1 to 64 items"),
            Self::ItemOrder => write!(f, "AS-04: item numbers must strictly ascend"),
            Self::BadText(why) => write!(f, "AS-07: text is not allowed: {why}"),
            Self::BadItem(why) => write!(f, "AS-06: item is not allowed: {why}"),
            Self::BadPayload(why) => write!(f, "AS-12: payload is not allowed: {why}"),
        }
    }
}

impl std::error::Error for EncodeError {}

/// Why a request does not verify.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VerifyError {
    /// AS-05: the owner key in the file is not the pinned owner root.
    WrongOwner,
    /// AS-05: the owner's signature does not verify.
    BadSignature,
}

impl fmt::Display for VerifyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WrongOwner => write!(f, "AS-05: the request is not from the pinned owner"),
            Self::BadSignature => write!(f, "AS-05: the owner's signature is not valid"),
        }
    }
}

impl std::error::Error for VerifyError {}

/// Why a response is not bound to the request it is offered for (AS-13).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BindError {
    /// AS-13: the response is from a key other than the pinned approver.
    WrongApprover,
    /// AS-13: the approver's signature does not verify.
    BadSignature,
    /// AS-13: the response is for another session.
    WrongSession,
    /// AS-13: the response names another request hash.
    WrongRequest,
    /// AS-13: a result is for an item the request does not have.
    UnknownItem {
        /// The item number.
        item: u32,
    },
    /// AS-13: the request has an item the response has no result for.
    MissingItem {
        /// The item number.
        item: u32,
    },
    /// AS-13: a result is of a kind that does not fit its item.
    KindMismatch {
        /// The item number.
        item: u32,
    },
    /// AS-14: the signed answer to a health check does not verify.
    BadHealthProof {
        /// The item number.
        item: u32,
    },
}

impl fmt::Display for BindError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WrongApprover => {
                write!(f, "AS-13: the response is not from the pinned approver")
            }
            Self::BadSignature => write!(f, "AS-13: the approver's signature is not valid"),
            Self::WrongSession => write!(f, "AS-13: the response is for another session"),
            Self::WrongRequest => write!(f, "AS-13: the response answers another request"),
            Self::UnknownItem { item } => {
                write!(
                    f,
                    "AS-13: the response answers item {item}, which was not asked"
                )
            }
            Self::MissingItem { item } => {
                write!(f, "AS-13: the response has no result for item {item}")
            }
            Self::KindMismatch { item } => {
                write!(f, "AS-13: the result for item {item} does not fit the item")
            }
            Self::BadHealthProof { item } => {
                write!(
                    f,
                    "AS-14: the health check answer for item {item} is not valid"
                )
            }
        }
    }
}

impl std::error::Error for BindError {}
