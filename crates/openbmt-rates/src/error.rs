//! Why a reply or a set of prices is refused. Every `Display` begins with
//! the rule's number; a person is shown the words in `say.rs`.

use std::fmt;

/// RT-20 to RT-24: why the text of a price is refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PriceRefused {
    /// No text at all.
    Empty,
    /// A `+` or `-` at the start.
    Sign,
    /// An `e` or `E`: an exponent.
    Exponent,
    /// A byte that is not a digit or a dot.
    BadCharacter,
    /// More than one dot.
    TwoDots,
    /// A dot with no digit before it or none after it.
    BadShape,
    /// A whole part of two or more digits that starts with a zero.
    LeadingZero,
    /// More than 8 digits after the dot.
    TooManyDecimals,
    /// The price is zero, or rounds to zero cents.
    Zero,
    /// The price is outside the band of RT-24.
    OutOfBand,
}

impl PriceRefused {
    /// The rule the refusal belongs to.
    pub fn rule(&self) -> &'static str {
        match self {
            Self::Empty
            | Self::Sign
            | Self::Exponent
            | Self::BadCharacter
            | Self::TwoDots
            | Self::BadShape
            | Self::LeadingZero => "RT-20",
            Self::TooManyDecimals => "RT-21",
            Self::Zero => "RT-23",
            Self::OutOfBand => "RT-24",
        }
    }
}

impl fmt::Display for PriceRefused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let why = match self {
            Self::Empty => "the price is empty",
            Self::Sign => "the price has a sign",
            Self::Exponent => "the price has an exponent",
            Self::BadCharacter => "the price has a byte that is not a digit or a dot",
            Self::TwoDots => "the price has more than one dot",
            Self::BadShape => "the price has a dot with no digit on one side",
            Self::LeadingZero => "the price has a leading zero",
            Self::TooManyDecimals => "the price has more than 8 digits after the dot",
            Self::Zero => "the price is zero",
            Self::OutOfBand => "the price is outside the band",
        };
        write!(f, "{}: {why}", self.rule())
    }
}

impl std::error::Error for PriceRefused {}

/// RT-10 to RT-17: why the reply of a source is refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParseRefused {
    /// More than 64 KiB (RT-10).
    TooLong,
    /// Arrays and objects nested more than 16 deep (RT-11).
    TooDeep,
    /// Not UTF-8 (RT-12).
    NotUtf8,
    /// Not a JSON document (RT-13).
    Malformed,
    /// Bytes after the document (RT-13).
    TrailingBytes,
    /// A key twice in one object (RT-14).
    DuplicateKey,
    /// A field the source's shape needs is not there (RT-15).
    MissingField,
    /// A field is there but is not the kind of value the shape needs, or
    /// the result of a source holds other than one entry (RT-15).
    WrongShape,
    /// The reply names a currency other than the source's (RT-17).
    WrongCurrency,
    /// The source says it did not answer (RT-16).
    SourceError,
    /// The price text is refused (RT-20 to RT-24).
    Price(PriceRefused),
}

impl ParseRefused {
    /// The rule the refusal belongs to.
    pub fn rule(&self) -> &'static str {
        match self {
            Self::TooLong => "RT-10",
            Self::TooDeep => "RT-11",
            Self::NotUtf8 => "RT-12",
            Self::Malformed | Self::TrailingBytes => "RT-13",
            Self::DuplicateKey => "RT-14",
            Self::MissingField | Self::WrongShape => "RT-15",
            Self::SourceError => "RT-16",
            Self::WrongCurrency => "RT-17",
            Self::Price(p) => p.rule(),
        }
    }
}

impl fmt::Display for ParseRefused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let why = match self {
            Self::TooLong => "the reply is longer than 64 KiB",
            Self::TooDeep => "the reply is nested more than 16 deep",
            Self::NotUtf8 => "the reply is not UTF-8",
            Self::Malformed => "the reply is not a JSON document",
            Self::TrailingBytes => "the reply has bytes after the document",
            Self::DuplicateKey => "the reply has a key twice in one object",
            Self::MissingField => "the reply lacks a field its source's shape needs",
            Self::WrongShape => "a field of the reply is not of the kind its shape needs",
            Self::SourceError => "the source reports an error",
            Self::WrongCurrency => "the reply names another currency",
            Self::Price(p) => return write!(f, "{p}"),
        };
        write!(f, "{}: {why}", self.rule())
    }
}

impl std::error::Error for ParseRefused {}

impl From<PriceRefused> for ParseRefused {
    fn from(p: PriceRefused) -> Self {
        Self::Price(p)
    }
}

/// RT-30 to RT-38: why the prices cannot be made into one rate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RatesRefused {
    /// Samples were given and none is fresh (RT-37).
    TooOld,
    /// Fewer than 2 distinct sources have a fresh sample (RT-31).
    NotEnoughSources {
        /// How many distinct sources had a fresh sample.
        fresh: usize,
    },
    /// A fresh sample is further from the median than the most allowed
    /// (RT-33). Prices are in minor units per bitcoin.
    PricesDisagree {
        /// The lowest price used.
        lowest: u64,
        /// The highest price used.
        highest: u64,
    },
    /// The samples are in more than one currency (RT-38).
    MixedCurrencies,
}

impl RatesRefused {
    /// The rule the refusal belongs to.
    pub fn rule(&self) -> &'static str {
        match self {
            Self::TooOld => "RT-37",
            Self::NotEnoughSources { .. } => "RT-31",
            Self::PricesDisagree { .. } => "RT-33",
            Self::MixedCurrencies => "RT-38",
        }
    }
}

impl fmt::Display for RatesRefused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooOld => write!(f, "RT-37: no price is fresh enough"),
            Self::NotEnoughSources { fresh } => write!(
                f,
                "RT-31: {fresh} source(s) with a fresh price, at least 2 are needed"
            ),
            Self::PricesDisagree { lowest, highest } => write!(
                f,
                "RT-33: the prices disagree by more than the most allowed \
                 ({lowest} to {highest} minor units per bitcoin)"
            ),
            Self::MixedCurrencies => write!(f, "RT-38: the prices are in more than one currency"),
        }
    }
}

impl std::error::Error for RatesRefused {}
