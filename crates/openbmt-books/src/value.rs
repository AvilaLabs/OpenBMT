//! A field the records may not carry: known, or an explicit unknown that
//! says why and what would supply it (BK-06).

/// The fields that can be unknown, each with its reason.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnknownField {
    /// Where the quote's rate came from. The opened record carries it, so
    /// the books no longer produce this unknown; the code stays.
    RateSource = 1,
    /// The dollar value when the payment was first seen: the final record
    /// carries it.
    ValueAtFirstSeen = 2,
    /// The sales tax of the sale.
    SalesTax = 3,
    /// The satoshis received, when no event says.
    SatsNotRecorded = 4,
    /// The satoshis received, when a late payment makes the sum unclear.
    SatsLateAmbiguous = 5,
    /// The address the sale paid.
    Address = 6,
    /// The transaction IDs of the payments, which the final record names.
    Txid = 7,
    /// The dollar value of a disposal.
    DisposalValue = 8,
    /// The basis of a disposal.
    DisposalBasis = 9,
}

/// What an unknown says about itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Unknown {
    /// Why the field is not known.
    pub why: &'static str,
    /// What would supply it.
    pub next_step: &'static str,
}

impl UnknownField {
    /// Read a stored code.
    pub fn from_u8(b: u8) -> Option<Self> {
        Some(match b {
            1 => Self::RateSource,
            2 => Self::ValueAtFirstSeen,
            3 => Self::SalesTax,
            4 => Self::SatsNotRecorded,
            5 => Self::SatsLateAmbiguous,
            6 => Self::Address,
            7 => Self::Txid,
            8 => Self::DisposalValue,
            9 => Self::DisposalBasis,
            _ => return None,
        })
    }

    /// The name of the field, for a column of text.
    pub fn name(self) -> &'static str {
        match self {
            Self::RateSource => "rate source",
            Self::ValueAtFirstSeen => "value at first seen",
            Self::SalesTax => "sales tax",
            Self::SatsNotRecorded | Self::SatsLateAmbiguous => "sats received",
            Self::Address => "address",
            Self::Txid => "txid",
            Self::DisposalValue => "dollar value",
            Self::DisposalBasis => "basis",
        }
    }

    /// Why it is unknown and what would supply it.
    pub fn explain(self) -> Unknown {
        let (why, next_step) = match self {
            Self::RateSource => (
                "the sale-opened record carries no rate source",
                "the sale-opened record of this counter would have to carry a rate source",
            ),
            Self::ValueAtFirstSeen => (
                "the sale is not final; the final record carries the value at first seen",
                "wait until the sale is final: the final record carries it",
            ),
            Self::SalesTax => (
                "the counter does not know the order's tax",
                "pass the tax of the POS match as PosFacts",
            ),
            Self::SatsNotRecorded => (
                "no event records the satoshis received for this sale",
                "the amount is written when the sale is confirmed",
            ),
            Self::SatsLateAmbiguous => (
                "a late payment and a confirmation both exist and there is no final record \
                 (or it was cut short) to say whether the confirmed amount includes the late one",
                "the sale must reach final: the final record names its transactions",
            ),
            Self::Address => (
                "the records carry the sale index, not the address",
                "derive it from the wallet description at that index",
            ),
            Self::Txid => (
                "the sale is not final; the final record names its transactions",
                "wait until the sale is final: the final record carries them",
            ),
            Self::DisposalValue => (
                "the caller gave no dollar value for this disposal",
                "supply the dollar value at the time of the disposal",
            ),
            Self::DisposalBasis => (
                "the caller gave no basis and lots are not tracked",
                "supply the basis the accountant uses",
            ),
        };
        Unknown { why, next_step }
    }
}

/// A value the records carry, or an unknown that says why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value<T> {
    /// The records carry it.
    Known(T),
    /// They do not.
    Unknown(UnknownField),
}

impl<T> Value<T> {
    /// The value, if known.
    pub fn known(&self) -> Option<&T> {
        match self {
            Self::Known(v) => Some(v),
            Self::Unknown(_) => None,
        }
    }

    /// The unknown, if it is one.
    pub fn unknown(&self) -> Option<UnknownField> {
        match self {
            Self::Known(_) => None,
            Self::Unknown(f) => Some(*f),
        }
    }
}
