//! Why a credential, a reply, an invoice or a code is refused. Every `Display`
//! begins with the rule's number; a person is shown the words in `say.rs`.
//! A `&'static str` of a refusal names the field it is about.

use std::fmt;

use openbmt_json::JsonError;

/// LN-05, LN-08, LN-11, LN-12: why a credential or a node address is
/// refused, before anything is sent to the node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CredentialRefused {
    /// The node address is not a host name or address, a colon and a port
    /// (LN-11).
    BadAddress,
    /// The connect string is not `lndconnect://host:port?cert=..&macaroon=..`
    /// (LN-11).
    BadConnectString,
    /// The connect string names no certificate, so there is nothing to pin
    /// (LN-12).
    NoCertificate,
    /// The certificate is not the DER bytes of a certificate (LN-12).
    BadCertificate,
    /// The certificate fingerprint is not 64 hex digits (LN-12).
    BadFingerprint,
    /// The macaroon is empty, too long, not base64 or not a macaroon (LN-05).
    BadMacaroon,
    /// The rune is not base64, or is cut short (LN-08).
    RuneNotBase64,
    /// The rune holds no restriction at all, so it can do everything (LN-08).
    RuneUnrestricted,
    /// The rune's restrictions are not text (LN-08).
    RuneNotText,
    /// The rune's restrictions are written in a way this crate does not
    /// know, with the part that is not understood named (LN-08).
    RuneSyntax(&'static str),
    /// One restriction of the rune names the method together with
    /// something else, or with another operator than `=`, so it can allow
    /// more than the methods it lists (LN-08).
    RuneMixesMethod,
    /// The rune allows a method other than the two this crate uses (LN-08).
    RuneAllowsMethod(String),
    /// The rune does not limit the method, or leaves out one of the two
    /// methods this crate needs (LN-08).
    RuneLacksMethod,
}

impl CredentialRefused {
    /// The rule the refusal belongs to.
    pub fn rule(&self) -> &'static str {
        match self {
            Self::BadAddress | Self::BadConnectString => "LN-11",
            Self::NoCertificate | Self::BadCertificate | Self::BadFingerprint => "LN-12",
            Self::BadMacaroon => "LN-05",
            Self::RuneNotBase64
            | Self::RuneUnrestricted
            | Self::RuneNotText
            | Self::RuneSyntax(_)
            | Self::RuneMixesMethod
            | Self::RuneAllowsMethod(_)
            | Self::RuneLacksMethod => "LN-08",
        }
    }

    fn why(&self) -> &'static str {
        match self {
            Self::BadAddress => "the node address is not a host name or address and a port",
            Self::BadConnectString => "the connect string is not an lndconnect address",
            Self::NoCertificate => "no certificate is given, so there is nothing to pin",
            Self::BadCertificate => "the certificate is not DER bytes",
            Self::BadFingerprint => "the fingerprint is not 64 hex digits",
            Self::BadMacaroon => "the macaroon is empty, too long or not a macaroon",
            Self::RuneNotBase64 => "the rune is not base64 or is cut short",
            Self::RuneUnrestricted => "the rune has no restrictions",
            Self::RuneNotText => "the rune's restrictions are not text",
            Self::RuneSyntax(_) => "the rune's restrictions are written in an unknown way",
            Self::RuneMixesMethod => "a restriction of the rune mixes the method with more",
            Self::RuneAllowsMethod(_) => "the rune allows a method other than the invoice ones",
            Self::RuneLacksMethod => "the rune does not limit the method to the invoice ones",
        }
    }
}

impl fmt::Display for CredentialRefused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.rule(), self.why())?;
        match self {
            Self::RuneSyntax(what) => write!(f, " ({what})"),
            Self::RuneAllowsMethod(m) => write!(f, " ({m})"),
            _ => Ok(()),
        }
    }
}

impl std::error::Error for CredentialRefused {}

/// LN-07, LN-10, LN-14, LN-15: why a reply of the node is refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReplyRefused {
    /// The node answered with an error. The text is the node's own, cleaned
    /// and cut short; it is for the details only (LN-07, LN-10).
    NodeSaid {
        /// The HTTP status.
        status: u16,
        /// The node's own error text.
        message: String,
    },
    /// More bytes, depth or values than the reader takes (LN-14).
    TooLarge,
    /// Not JSON, or JSON with a key twice (LN-14).
    Unreadable,
    /// A field the reader needs is not there (LN-14).
    Missing(&'static str),
    /// A field is not of the type the node's documents give (LN-14).
    WrongType(&'static str),
    /// A number that does not fit or is not whole (LN-14).
    OutOfRange(&'static str),
    /// The reply is about another payment than the one asked for (LN-14).
    OtherInvoice,
    /// More than one invoice where one was asked for (LN-14).
    Several,
    /// Core Lightning has an invoice with the label that is not the one this
    /// sale made: its amount, state or description differ (LN-15).
    LabelTaken,
    /// The node has no invoice with the label although it said it had one
    /// (LN-15).
    LabelGone,
}

impl ReplyRefused {
    /// The rule the refusal belongs to.
    pub fn rule(&self) -> &'static str {
        match self {
            Self::NodeSaid { .. } => "LN-10",
            Self::LabelTaken | Self::LabelGone => "LN-15",
            Self::TooLarge
            | Self::Unreadable
            | Self::Missing(_)
            | Self::WrongType(_)
            | Self::OutOfRange(_)
            | Self::OtherInvoice
            | Self::Several => "LN-14",
        }
    }

    fn why(&self) -> &'static str {
        match self {
            Self::NodeSaid { .. } => "the node answered with an error",
            Self::TooLarge => "the reply is larger than the reader takes",
            Self::Unreadable => "the reply is not a JSON document",
            Self::Missing(_) => "a field the reader needs is not there",
            Self::WrongType(_) => "a field is not of the documented type",
            Self::OutOfRange(_) => "a number does not fit or is not whole",
            Self::OtherInvoice => "the reply is about another payment than the one asked for",
            Self::Several => "the reply holds more than one invoice",
            Self::LabelTaken => "the label belongs to an invoice that is not this sale's",
            Self::LabelGone => "the node has no invoice with the label",
        }
    }

    /// The field the refusal is about, when it names one.
    pub fn field(&self) -> Option<&'static str> {
        match self {
            Self::Missing(f) | Self::WrongType(f) | Self::OutOfRange(f) => Some(f),
            _ => None,
        }
    }
}

impl fmt::Display for ReplyRefused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.rule(), self.why())?;
        match self {
            Self::NodeSaid { status, message } => write!(f, " (status {status}: {message})"),
            other => match other.field() {
                Some(field) => write!(f, " ({field})"),
                None => Ok(()),
            },
        }
    }
}

impl std::error::Error for ReplyRefused {}

impl From<JsonError> for ReplyRefused {
    fn from(e: JsonError) -> Self {
        match e {
            JsonError::TooLong | JsonError::TooDeep | JsonError::TooManyValues => Self::TooLarge,
            JsonError::NotUtf8
            | JsonError::Malformed
            | JsonError::TrailingBytes
            | JsonError::DuplicateKey => Self::Unreadable,
        }
    }
}

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

/// LN-20: why the test of a connection fails.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectionRefused {
    /// A call that the credential must not be allowed was allowed: the
    /// credential can do more than invoices (LN-06, LN-09).
    CredentialTooStrong {
        /// The call.
        call: &'static str,
        /// The rule of the node software the credential is for.
        rule: &'static str,
    },
    /// A call that the credential must not be allowed was refused, but not
    /// with the node's permission error, so it is not known whether the
    /// credential is limited (LN-06, LN-09).
    ProbeUnclear {
        /// The call.
        call: &'static str,
        /// The HTTP status.
        status: u16,
        /// The rule of the node software the credential is for.
        rule: &'static str,
    },
    /// The node says the rune is not one it made, or is missing (LN-09).
    RuneNotFromNode,
    /// The node says the rune has been revoked (LN-09).
    RuneRevoked,
    /// The test invoice was not made (LN-20).
    NoInvoice(ReplyRefused),
    /// The label of the test invoice is already used (LN-20).
    LabelUsed,
    /// The node made an invoice for another node than the shop's, which
    /// settings name (LN-20).
    WrongNode,
    /// The node made an invoice that does not pass the check (LN-20).
    BadInvoice(InvoiceRefused),
}

impl ConnectionRefused {
    /// The rule the refusal belongs to.
    pub fn rule(&self) -> &'static str {
        match self {
            Self::CredentialTooStrong { rule, .. } | Self::ProbeUnclear { rule, .. } => rule,
            Self::RuneNotFromNode | Self::RuneRevoked => "LN-09",
            Self::NoInvoice(_) | Self::LabelUsed | Self::WrongNode | Self::BadInvoice(_) => "LN-20",
        }
    }
}

impl fmt::Display for ConnectionRefused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CredentialTooStrong { call, .. } => write!(
                f,
                "{}: the credential was allowed {call}, so it can do more than invoices",
                self.rule()
            ),
            Self::ProbeUnclear { call, status, .. } => write!(
                f,
                "{}: {call} was refused with status {status}, not with the permission error",
                self.rule()
            ),
            Self::RuneNotFromNode => write!(
                f,
                "{}: the node says the rune is not one of its own",
                self.rule()
            ),
            Self::RuneRevoked => write!(f, "{}: the node says the rune is revoked", self.rule()),
            Self::NoInvoice(e) => write!(f, "{}: no test invoice: {e}", self.rule()),
            Self::LabelUsed => write!(f, "{}: the test invoice's label is used", self.rule()),
            Self::WrongNode => write!(
                f,
                "{}: the test invoice is signed by another node than the settings name",
                self.rule()
            ),
            Self::BadInvoice(e) => write!(f, "{}: the test invoice fails: {e}", self.rule()),
        }
    }
}

impl std::error::Error for ConnectionRefused {}
