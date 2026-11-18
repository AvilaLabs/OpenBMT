//! Why an invoice, an ask or a code is refused. Every `Display` begins with
//! the rule's number.

use std::fmt;

/// LN-16, LN-17: why the invoice a node returned is refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InvoiceRefused {
    /// Longer than the reader takes (LN-16).
    TooLong,
    /// Upper and lower case letters together (LN-16).
    MixedCase,
    /// Not a bech32 text with a good checksum, or not starting with `ln`
    /// (LN-16).
    NotAnInvoice,
    /// A network prefix that is not one of the four (LN-16).
    UnknownNetwork,
    /// The amount is not written as BOLT 11 writes it (LN-16).
    BadAmount,
    /// The data ends before a field or the signature does (LN-16).
    Truncated,
    /// A field is of the wrong length or holds what it may not (LN-16).
    BadField(&'static str),
    /// A field that may be there once is there twice (LN-16).
    DuplicateField(&'static str),
    /// A field this crate needs is not there (LN-16).
    MissingField(&'static str),
    /// The signature does not give a public key (LN-16).
    BadSignature,
    /// The invoice is for another network than the shop's (LN-17).
    WrongNetwork,
    /// The invoice names no amount (LN-17).
    NoAmount,
    /// The invoice is for another amount than was asked for (LN-17).
    WrongAmount {
        /// Millisatoshis asked for.
        asked: u64,
        /// Millisatoshis the invoice names.
        got: u64,
    },
    /// The invoice's payment hash is not the one the node reported (LN-17).
    WrongPaymentHash,
    /// The invoice runs for another time than was asked for (LN-17).
    WrongExpiry {
        /// Seconds asked for.
        asked: u64,
        /// Seconds the invoice runs for.
        got: u64,
    },
    /// The description is not the one this sale made, or is a hash of one
    /// (LN-17).
    WrongDescription,
    /// The invoice is signed by another node than the shop's (LN-17).
    WrongNode,
}

impl InvoiceRefused {
    /// The rule the refusal belongs to.
    pub fn rule(&self) -> &'static str {
        match self {
            Self::TooLong
            | Self::MixedCase
            | Self::NotAnInvoice
            | Self::UnknownNetwork
            | Self::BadAmount
            | Self::Truncated
            | Self::BadField(_)
            | Self::DuplicateField(_)
            | Self::MissingField(_)
            | Self::BadSignature => "LN-16",
            Self::WrongNetwork
            | Self::NoAmount
            | Self::WrongAmount { .. }
            | Self::WrongPaymentHash
            | Self::WrongExpiry { .. }
            | Self::WrongDescription
            | Self::WrongNode => "LN-17",
        }
    }

    fn why(&self) -> &'static str {
        match self {
            Self::TooLong => "the invoice is longer than the reader takes",
            Self::MixedCase => "the invoice mixes upper and lower case",
            Self::NotAnInvoice => "the text is not a BOLT 11 invoice",
            Self::UnknownNetwork => "the network prefix is not one of the four",
            Self::BadAmount => "the amount is not written as BOLT 11 writes it",
            Self::Truncated => "the invoice ends inside a field",
            Self::BadField(_) => "a field is of the wrong length or holds what it may not",
            Self::DuplicateField(_) => "a field is there twice",
            Self::MissingField(_) => "a field the reader needs is not there",
            Self::BadSignature => "the signature gives no public key",
            Self::WrongNetwork => "the invoice is for another network",
            Self::NoAmount => "the invoice names no amount",
            Self::WrongAmount { .. } => "the invoice is for another amount",
            Self::WrongPaymentHash => "the invoice has another payment hash than the node reported",
            Self::WrongExpiry { .. } => "the invoice runs for another time",
            Self::WrongDescription => "the invoice has another description",
            Self::WrongNode => "the invoice is signed by another node than the shop's",
        }
    }
}

impl fmt::Display for InvoiceRefused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.rule(), self.why())?;
        match self {
            Self::BadField(x) | Self::DuplicateField(x) | Self::MissingField(x) => {
                write!(f, " ({x})")
            }
            Self::WrongAmount { asked, got } => write!(f, " (asked {asked} msat, got {got})"),
            Self::WrongExpiry { asked, got } => write!(f, " (asked {asked} s, got {got})"),
            _ => Ok(()),
        }
    }
}

impl std::error::Error for InvoiceRefused {}

/// LN-13: why an invoice is not asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AskRefused {
    /// The amount is 0 or more than all the bitcoin there is.
    BadAmount,
    /// The lifetime is 0 or longer than a day.
    BadExpiry,
}

impl AskRefused {
    /// The rule the refusal belongs to.
    pub fn rule(&self) -> &'static str {
        "LN-13"
    }
}

impl fmt::Display for AskRefused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BadAmount => write!(f, "LN-13: the amount is 0 or more than all the bitcoin"),
            Self::BadExpiry => write!(f, "LN-13: the lifetime is 0 or longer than a day"),
        }
    }
}

impl std::error::Error for AskRefused {}

/// LN-18, LN-19: why a payment code is not made.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UriRefused {
    /// The address is not a bitcoin address of the shop's network (LN-18).
    BadAddress,
    /// The amount is 0 or not a whole number of satoshis the invoice names
    /// too (LN-18).
    AmountMismatch,
    /// The code cannot be made in capitals, because the address is not
    /// bech32 (LN-19).
    NotBech32,
}

impl UriRefused {
    /// The rule the refusal belongs to.
    pub fn rule(&self) -> &'static str {
        match self {
            Self::BadAddress | Self::AmountMismatch => "LN-18",
            Self::NotBech32 => "LN-19",
        }
    }
}

impl fmt::Display for UriRefused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let why = match self {
            Self::BadAddress => "the address is not an address of the shop's network",
            Self::AmountMismatch => "the amount is not the one the invoice names",
            Self::NotBech32 => "the address is not bech32, so the code is not made in capitals",
        };
        write!(f, "{}: {why}", self.rule())
    }
}

impl std::error::Error for UriRefused {}
