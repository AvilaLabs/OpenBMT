//! Why a request, a reply, a lockup, a claim or a state file is refused.
//! Every `Display` begins with the rule's number.

use std::fmt;

use openbmt_json::JsonError;
use openbmt_lightning::error::InvoiceRefused;

/// A refusal, with the rule it belongs to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refused {
    /// SW-02: the invoice amount is outside the band.
    AmountOutOfBand,
    /// SW-03: the description is empty, too long or has a control character.
    BadDescription,
    /// SW-04: the bytes are not a usable preimage or claim key.
    BadSecret,
    /// SW-10: the reply is not a JSON document within the limits.
    ReplyNotJson(JsonError),
    /// SW-11: a member of the reply is not there.
    MissingMember(&'static str),
    /// SW-11, SW-12, SW-13: a member of the reply is of the wrong kind or
    /// out of bounds.
    BadMember(&'static str),
    /// SW-20: the invoice does not read.
    InvoiceUnreadable(InvoiceRefused),
    /// SW-20: the invoice is for another payment hash.
    WrongPaymentHash,
    /// SW-21: the invoice names no amount.
    NoInvoiceAmount,
    /// SW-21: the invoice is for another amount.
    WrongInvoiceAmount {
        /// Millisatoshis asked for.
        asked: u64,
        /// Millisatoshis the invoice names.
        got: u64,
    },
    /// SW-22: the invoice is for another network.
    WrongInvoiceNetwork,
    /// SW-23: the invoice has run out or has too little time left.
    InvoiceTooShort,
    /// SW-24: the refund key is not a key, or is the claim key.
    BadRefundKey,
    /// SW-25: a leaf of the reply is not the leaf that is rebuilt.
    LeafMismatch(&'static str),
    /// SW-26: the lockup address is not the one of the rebuilt output.
    WrongLockupAddress,
    /// SW-27: the timeout is not a height or is too near.
    TimeoutTooSoon,
    /// SW-28: the amount on chain is below the floor.
    OnchainTooSmall,
    /// SW-28: the amount on chain is above the invoice amount.
    OnchainAboveInvoice,
    /// SW-28: the amount on chain is below what the quote leaves.
    OnchainBelowQuote {
        /// The least the quote allows.
        least: u64,
        /// What the service promised.
        got: u64,
    },
    /// SW-30: no output of the transaction pays the lockup address.
    NoLockupOutput,
    /// SW-30: more than one output pays the lockup address.
    TwoLockupOutputs,
    /// SW-30: the output pays less than was promised.
    LockupTooSmall,
    /// SW-40: the receive address is for another network.
    DestinationNetwork,
    /// SW-40, SW-41: the lockup output is not the swap's, or the fee or the
    /// output cannot be made.
    ClaimUnbuildable(&'static str),
    /// SW-41: the fee is more than half of the lockup or the output is
    /// dust.
    FeeTooHigh,
    /// SW-43: the finished claim does not pass the script check.
    ClaimFailsScript,
    /// SW-44: the chain is at or past the timeout.
    TooLate,
    /// SW-53: the swap is already claimed.
    AlreadyClaimed,
    /// SW-50 to SW-52: the state file is not a state.
    BadState(&'static str),
    /// SW-60: the reply of the pairs call is not usable.
    BadPairs(&'static str),
    /// SW-61: a percentage is not a decimal the shop reads exactly.
    BadPercentage,
    /// SW-62: the amount is outside the service's own live limits.
    ServiceLimit {
        /// The least invoice amount the service takes now.
        min: u64,
        /// The most invoice amount the service takes now.
        max: u64,
    },
    /// SW-63: the pairs were read too long ago, or at a later time than now.
    PairsStale,
    /// SW-70: the status reply is not usable.
    BadStatus(&'static str),
    /// SW-71: the swap is not at a point where this can be done.
    NotReady(&'static str),
    /// SW-72: the transaction reported is not the claim that was sent.
    WrongClaim,
    /// SW-73: the followed-swap file is not one.
    BadFollow(&'static str),
    /// SW-80: a request to the provider cannot be written.
    BadHttpRequest(&'static str),
    /// SW-81: the provider's reply is not an HTTP reply the shop reads.
    BadHttpReply(&'static str),
    /// SW-81: the provider answered with a status other than 200.
    HttpStatus(u16),
    /// SW-82: no provider is set; the swap service is off.
    ProviderOff,
    /// SW-82: the provider setting is not usable.
    BadProvider(&'static str),
}

impl Refused {
    /// The rule the refusal belongs to.
    pub fn rule(&self) -> &'static str {
        match self {
            Self::AmountOutOfBand => "SW-02",
            Self::BadDescription => "SW-03",
            Self::BadSecret => "SW-04",
            Self::ReplyNotJson(_) => "SW-10",
            Self::MissingMember(_) | Self::BadMember(_) => "SW-11",
            Self::InvoiceUnreadable(_) | Self::WrongPaymentHash => "SW-20",
            Self::NoInvoiceAmount | Self::WrongInvoiceAmount { .. } => "SW-21",
            Self::WrongInvoiceNetwork => "SW-22",
            Self::InvoiceTooShort => "SW-23",
            Self::BadRefundKey => "SW-24",
            Self::LeafMismatch(_) => "SW-25",
            Self::WrongLockupAddress => "SW-26",
            Self::TimeoutTooSoon => "SW-27",
            Self::OnchainTooSmall | Self::OnchainAboveInvoice | Self::OnchainBelowQuote { .. } => {
                "SW-28"
            }
            Self::NoLockupOutput | Self::TwoLockupOutputs | Self::LockupTooSmall => "SW-30",
            Self::DestinationNetwork | Self::ClaimUnbuildable(_) => "SW-40",
            Self::FeeTooHigh => "SW-41",
            Self::ClaimFailsScript => "SW-43",
            Self::TooLate => "SW-44",
            Self::AlreadyClaimed => "SW-53",
            Self::BadState(_) => "SW-52",
            Self::BadPairs(_) => "SW-60",
            Self::BadPercentage => "SW-61",
            Self::ServiceLimit { .. } => "SW-62",
            Self::PairsStale => "SW-63",
            Self::BadStatus(_) => "SW-70",
            Self::NotReady(_) => "SW-71",
            Self::WrongClaim => "SW-72",
            Self::BadFollow(_) => "SW-73",
            Self::BadHttpRequest(_) => "SW-80",
            Self::BadHttpReply(_) | Self::HttpStatus(_) => "SW-81",
            Self::ProviderOff | Self::BadProvider(_) => "SW-82",
        }
    }
}

impl fmt::Display for Refused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: ", self.rule())?;
        match self {
            Self::AmountOutOfBand => f.write_str("the invoice amount is outside the band"),
            Self::BadDescription => f.write_str("the description is not allowed"),
            Self::BadSecret => f.write_str("the bytes are not a usable preimage or claim key"),
            Self::ReplyNotJson(e) => write!(f, "the reply is refused: {e}"),
            Self::MissingMember(m) => write!(f, "the reply has no {m}"),
            Self::BadMember(m) => write!(f, "the reply's {m} is not acceptable"),
            Self::InvoiceUnreadable(e) => write!(f, "the invoice is refused: {e}"),
            Self::WrongPaymentHash => f.write_str("the invoice is for another payment hash"),
            Self::NoInvoiceAmount => f.write_str("the invoice names no amount"),
            Self::WrongInvoiceAmount { asked, got } => {
                write!(f, "the invoice is for {got} msat, not {asked} msat")
            }
            Self::WrongInvoiceNetwork => f.write_str("the invoice is for another network"),
            Self::InvoiceTooShort => f.write_str("the invoice has too little time left"),
            Self::BadRefundKey => f.write_str("the refund key is not usable"),
            Self::LeafMismatch(l) => write!(f, "the {l} leaf is not the one rebuilt"),
            Self::WrongLockupAddress => f.write_str("the lockup address is not the rebuilt one"),
            Self::TimeoutTooSoon => f.write_str("the timeout leaves too few blocks"),
            Self::OnchainTooSmall => f.write_str("the amount on chain is below the floor"),
            Self::OnchainAboveInvoice => f.write_str("the amount on chain is above the invoice"),
            Self::OnchainBelowQuote { least, got } => {
                write!(f, "the amount on chain is {got}, the quote leaves {least}")
            }
            Self::NoLockupOutput => f.write_str("no output pays the lockup address"),
            Self::TwoLockupOutputs => f.write_str("two outputs pay the lockup address"),
            Self::LockupTooSmall => f.write_str("the lockup pays less than promised"),
            Self::DestinationNetwork => f.write_str("the receive address is for another network"),
            Self::ClaimUnbuildable(w) => write!(f, "the claim cannot be built: {w}"),
            Self::FeeTooHigh => f.write_str("the fee is too high for the lockup"),
            Self::ClaimFailsScript => f.write_str("the claim does not pass the script check"),
            Self::TooLate => f.write_str("the chain is at or past the timeout"),
            Self::AlreadyClaimed => f.write_str("the swap is already claimed"),
            Self::BadState(w) => write!(f, "the state file is refused: {w}"),
            Self::BadPairs(w) => write!(f, "the service's terms are refused: {w}"),
            Self::BadPercentage => f.write_str("the percentage is not read exactly"),
            Self::ServiceLimit { min, max } => {
                write!(f, "the service's own limit is {min} to {max} satoshis")
            }
            Self::PairsStale => f.write_str("the service's terms are too old"),
            Self::BadStatus(w) => write!(f, "the status is refused: {w}"),
            Self::NotReady(w) => write!(f, "not at this point: {w}"),
            Self::WrongClaim => f.write_str("that is not the claim that was sent"),
            Self::BadFollow(w) => write!(f, "the followed swap is refused: {w}"),
            Self::BadHttpRequest(w) => write!(f, "the request cannot be written: {w}"),
            Self::BadHttpReply(w) => write!(f, "the reply is refused: {w}"),
            Self::HttpStatus(c) => write!(f, "the service answered {c}"),
            Self::ProviderOff => f.write_str("no swap service is set"),
            Self::BadProvider(w) => write!(f, "the swap service setting is refused: {w}"),
        }
    }
}

impl std::error::Error for Refused {}
