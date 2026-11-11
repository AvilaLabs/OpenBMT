//! The refusals. Each text starts with the rule's code (`docs/BBQR-RULES.md`).

use std::fmt;

/// Why a part, a set of parts or a stream was refused. Nothing was changed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BbqrError {
    /// BQ-01: text that is not the shape of a part.
    Header(&'static str),
    /// BQ-02: an encoding that is not read (or not written).
    Encoding,
    /// BQ-03: a file type that is not known, or not allowed here.
    Kind,
    /// BQ-04: a part count or index outside its range.
    Count,
    /// BQ-05: a size outside the bounds.
    Size(&'static str),
    /// BQ-06: a part that disagrees with the ones held.
    Disagrees(&'static str),
    /// BQ-07: parts that are not split the way the format says.
    Shape(&'static str),
    /// BQ-08: hex digits that are not whole bytes of 0-9 and A-F.
    Hex,
    /// BQ-09: base32 that is not strict.
    Base32(&'static str),
    /// BQ-10: some parts are still missing.
    Missing {
        /// Distinct parts held.
        have: usize,
        /// Parts in the set.
        total: usize,
    },
    /// BQ-11: a compressed stream that is not strict.
    Inflate(&'static str),
    /// BQ-12: a compressed stream that would grow past the bound.
    InflateTooBig,
}

/// Every refusal once, for the tests that check each has words.
pub const ALL_REFUSALS: [BbqrError; 12] = [
    BbqrError::Header(""),
    BbqrError::Encoding,
    BbqrError::Kind,
    BbqrError::Count,
    BbqrError::Size(""),
    BbqrError::Disagrees(""),
    BbqrError::Shape(""),
    BbqrError::Hex,
    BbqrError::Base32(""),
    BbqrError::Missing { have: 0, total: 0 },
    BbqrError::Inflate(""),
    BbqrError::InflateTooBig,
];

impl BbqrError {
    /// The rule's code.
    pub fn rule(&self) -> &'static str {
        match self {
            Self::Header(_) => "BQ-01",
            Self::Encoding => "BQ-02",
            Self::Kind => "BQ-03",
            Self::Count => "BQ-04",
            Self::Size(_) => "BQ-05",
            Self::Disagrees(_) => "BQ-06",
            Self::Shape(_) => "BQ-07",
            Self::Hex => "BQ-08",
            Self::Base32(_) => "BQ-09",
            Self::Missing { .. } => "BQ-10",
            Self::Inflate(_) => "BQ-11",
            Self::InflateTooBig => "BQ-12",
        }
    }

    fn text(&self) -> String {
        match self {
            Self::Header(why) => format!("not a part of a BBQr set: {why}"),
            Self::Encoding => "an encoding other than H, 2 and Z".to_string(),
            Self::Kind => "a file type that is not known or not allowed here".to_string(),
            Self::Count => "a part count or number outside its range".to_string(),
            Self::Size(why) => format!("a size outside the bounds: {why}"),
            Self::Disagrees(why) => format!("a part that disagrees with the others: {why}"),
            Self::Shape(why) => format!("parts not split as the format says: {why}"),
            Self::Hex => "hex that is not whole bytes of 0-9 and A-F".to_string(),
            Self::Base32(why) => format!("base32 that is not strict: {why}"),
            Self::Missing { have, total } => format!("{have} of {total} parts so far"),
            Self::Inflate(why) => format!("a compressed stream that is not strict: {why}"),
            Self::InflateTooBig => "a compressed stream that expands past the bound".to_string(),
        }
    }

    /// What a person is told. Every set passes the checks of
    /// `docs/SCREEN-RULES.md` (BQ-16). The app gives the colour and says
    /// who does the step.
    pub fn words(&self) -> BbqrWords {
        let (heading, what, next) = match self {
            Self::Header(_) => (
                "Not part of a numbered set",
                "The code was read, but it is not one of the numbered codes a signer shows for a transaction.",
                "Scan the codes the signer shows for the signed transaction, or use the file instead.",
            ),
            Self::Encoding => (
                "Codes packed an unknown way",
                "The codes are numbered like a set, but they pack the data in a way that this app does not read.",
                "On the signer, choose another way to show the transaction, or save it as a file.",
            ),
            Self::Kind => (
                "Codes of the wrong kind",
                "The codes carry something other than a transaction, so nothing was taken from them.",
                "Show the signed transaction on the signer, then scan those codes.",
            ),
            Self::Count => (
                "Code number not possible",
                "A code says it is a number that cannot be right for its set, so it was left out.",
                "Start the signer's codes again from the beginning and scan them again.",
            ),
            Self::Size(_) => (
                "Codes hold too much",
                "The codes hold more data than this app takes for one transaction, so none of it was kept.",
                "Save the transaction as a file on the signer and open the file here.",
            ),
            Self::Disagrees(_) => (
                "Codes from different sets",
                "Two of the codes scanned do not belong to the same set, so the last one was left out.",
                "Scan only the codes of one signing, starting again if you scanned an older one.",
            ),
            Self::Shape(_) => (
                "Codes not cut as expected",
                "The parts of the set are not cut the way the format says, so they were not joined.",
                "Show the codes again on the signer and scan them again, or use the file.",
            ),
            Self::Hex => (
                "Code holds damaged data",
                "A code holds characters that are not whole bytes of data, so it was left out.",
                "Scan that code again, or use the file from the signer instead.",
            ),
            Self::Base32(_) => (
                "Code data is damaged",
                "A code holds data that is not packed the way its set says, so it was left out.",
                "Scan that code again, or use the file from the signer instead.",
            ),
            Self::Missing { .. } => (
                "Some codes still to scan",
                "The set is not complete yet, so the transaction cannot be read.",
                "Keep the signer's codes in view until every one has been scanned.",
            ),
            Self::Inflate(_) => (
                "Codes packed wrongly",
                "The codes were joined, but the compressed data inside them is damaged or not in the expected form.",
                "Show the codes again on the signer and scan the whole set again, or use the file.",
            ),
            Self::InflateTooBig => (
                "Codes unpack too large",
                "The codes unpack to more data than this app takes for one transaction, so nothing was kept.",
                "Save the transaction as a file on the signer and open the file here.",
            ),
        };
        BbqrWords {
            heading,
            what,
            next,
        }
    }
}

/// The three texts of a refusal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BbqrWords {
    /// A few words that name the state.
    pub heading: &'static str,
    /// What happened.
    pub what: &'static str,
    /// What to do next.
    pub next: &'static str,
}

impl fmt::Display for BbqrError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.rule(), self.text())
    }
}

impl std::error::Error for BbqrError {}
