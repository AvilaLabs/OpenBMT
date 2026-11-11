//! BBQr, Coinkite's format for a file in a sequence of QR codes, for the
//! owner phone to show a PSBT to a hardware signer and read the signed one
//! back (`docs/BBQR-RULES.md`).
//!
//! - [`encode`] cuts a file into parts of base32 text, each with the
//!   8-character header `B$`, the encoding, the file type, the part count and
//!   the part number;
//! - [`Joiner`] takes parts in any order, with repeats, reads the hex,
//!   base32 and zlib encodings, and gives the file back once every part is
//!   in;
//! - [`inflate()`] is the strict decompressor behind the `Z` encoding.
//!
//! No dependencies, no clock, no storage and no screen: the app draws each
//! part as a QR code in alphanumeric mode at the lowest error correction and
//! shows how far a scan has come.

#![forbid(unsafe_code)]
#![deny(missing_docs)]
#![cfg_attr(
    not(test),
    deny(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing
    )
)]

mod base32;
mod error;
pub mod inflate;
mod join;
mod split;

pub use error::{BbqrError, BbqrWords, ALL_REFUSALS};
pub use inflate::inflate;
pub use join::{Decoded, Joiner, Progress};
pub use split::encode;

/// BQ-01: characters in the header of every part.
pub const HEADER_CHARS: usize = 8;
/// BQ-05: the most characters in one part, the capacity of a version 40 QR
/// code in alphanumeric mode at the lowest error correction.
pub const MAX_PART_CHARS: usize = 4296;
/// BQ-05: the fewest characters a writer's part may be limited to: the header
/// and one group of eight base32 characters.
pub const MIN_PART_CHARS: usize = 16;
/// BQ-04: the most parts in a set (`ZZ` in base 36).
pub const MAX_PARTS: usize = 1295;
/// BQ-05: the most bytes a file may have, joined or unpacked: above the
/// 2,000,000 bytes a Coldcard takes in a PSBT.
pub const MAX_DECODED: usize = 2 * 1024 * 1024;

/// BQ-03: what the file is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// `P`: a PSBT.
    Psbt,
    /// `T`: a Bitcoin transaction ready to send.
    Transaction,
    /// `J`: JSON.
    Json,
    /// `C`: CBOR.
    Cbor,
    /// `U`: text in UTF-8.
    Text,
    /// `B`: bytes of no known kind.
    Binary,
    /// `X`: an executable.
    Executable,
}

impl Kind {
    /// The one character of the header.
    pub fn code(self) -> char {
        match self {
            Self::Psbt => 'P',
            Self::Transaction => 'T',
            Self::Json => 'J',
            Self::Cbor => 'C',
            Self::Text => 'U',
            Self::Binary => 'B',
            Self::Executable => 'X',
        }
    }

    /// The kind of a header character, in upper case.
    pub fn from_code(c: char) -> Option<Self> {
        Some(match c {
            'P' => Self::Psbt,
            'T' => Self::Transaction,
            'J' => Self::Json,
            'C' => Self::Cbor,
            'U' => Self::Text,
            'B' => Self::Binary,
            'X' => Self::Executable,
            _ => return None,
        })
    }
}

/// BQ-02: how the bytes of a file are written in the parts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Encoding {
    /// `H`: hex.
    Hex,
    /// `2`: base32, RFC 4648, no padding.
    Base32,
    /// `Z`: deflate at a window of 10 bits with no header, then base32.
    Zlib,
}

impl Encoding {
    /// The one character of the header.
    pub fn code(self) -> char {
        match self {
            Self::Hex => 'H',
            Self::Base32 => '2',
            Self::Zlib => 'Z',
        }
    }

    pub(crate) fn from_code(c: char) -> Option<Self> {
        Some(match c {
            'H' => Self::Hex,
            '2' => Self::Base32,
            'Z' => Self::Zlib,
            _ => return None,
        })
    }
}

/// Two base-36 digits (0-9, A-Z) of `n`, which is at most 1295.
pub(crate) fn base36_pair(n: usize) -> Option<[char; 2]> {
    let digit =
        |v: usize| char::from_digit(u32::try_from(v).ok()?, 36).map(|c| c.to_ascii_uppercase());
    (n <= MAX_PARTS).then_some(())?;
    Some([digit(n / 36)?, digit(n % 36)?])
}

#[cfg(test)]
mod tests;
