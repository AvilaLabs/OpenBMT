//! A QR code encoder and reader, ISO/IEC 18004 model 2, with no
//! dependencies ([rules](../README.md)). The reader is in [`decode_luma`] and
//! [`decode_luma_strided`]: it takes the gray levels of a camera frame.
//!
//! - Versions 1 to 40, levels L, M, Q and H.
//! - One segment per code: numeric, alphanumeric or byte mode.
//! - [`encode_text`] picks the densest mode the text allows;
//!   [`encode_bytes`] always uses byte mode.
//! - The smallest version that fits at the asked level is used, and the
//!   level is never raised to fill a version.
//! - The mask with the lowest penalty wins, ties to the lower number.
//!
//! Where the code is more exact than the standard, or chooses:
//!
//! - Rule 3 of the mask penalty (the 1:1:3:1:1 pattern) counts the four
//!   light modules only when they lie inside the symbol. The quiet zone is
//!   not counted as them.
//! - Rule 4 of the mask penalty takes a share that is itself a multiple of
//!   5 percent as the lower of the two multiples around it, so 45 percent
//!   costs nothing and 55 percent costs 10.
//! - Byte mode carries the bytes as given, with no ECI segment.

#![forbid(unsafe_code)]

mod binarize;
mod decode;
mod finder;
mod matrix;
mod readback;
mod rs;
mod sample;
mod tables;

use std::fmt;

pub use decode::{decode_luma, decode_luma_strided, DecodeError, Decoded, MAX_SIDE, MIN_SIDE};
pub use matrix::{format_bits, version_bits};

/// The four error-correction levels.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Ecc {
    /// About 7 percent of codewords.
    L,
    /// About 15 percent.
    M,
    /// About 25 percent.
    Q,
    /// About 30 percent.
    H,
}

impl Ecc {
    /// All four, weakest first.
    pub const ALL: [Ecc; 4] = [Ecc::L, Ecc::M, Ecc::Q, Ecc::H];
}

/// The segment modes this encoder writes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Mode {
    /// The digits 0 to 9.
    Numeric,
    /// Digits, upper-case letters and ` $%*+-./:`.
    Alphanumeric,
    /// Any bytes.
    Byte,
}

impl Mode {
    /// The densest mode that can carry `data`.
    pub fn detect(data: &[u8]) -> Mode {
        if data.iter().all(u8::is_ascii_digit) {
            Mode::Numeric
        } else if data.iter().all(|&b| alnum_value(b).is_some()) {
            Mode::Alphanumeric
        } else {
            Mode::Byte
        }
    }

    fn indicator(self) -> u32 {
        match self {
            Mode::Numeric => 0b0001,
            Mode::Alphanumeric => 0b0010,
            Mode::Byte => 0b0100,
        }
    }

    /// Bits of the character count field at a version.
    fn count_bits(self, version: u8) -> u32 {
        let (small, middle, large) = match self {
            Mode::Numeric => (10, 12, 14),
            Mode::Alphanumeric => (9, 11, 13),
            Mode::Byte => (8, 16, 16),
        };
        match version {
            0..=9 => small,
            10..=26 => middle,
            _ => large,
        }
    }

    /// Bits of the data of `count` characters.
    fn data_bits(self, count: usize) -> usize {
        match self {
            Mode::Numeric => (count / 3) * 10 + numeric_tail_bits(count % 3),
            Mode::Alphanumeric => (count / 2) * 11 + (count % 2) * 6,
            Mode::Byte => count.saturating_mul(8),
        }
    }
}

/// Why a code was not made. Every text starts with its code.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QrError {
    /// QR-01: the data does not fit the largest version asked for.
    TooLong {
        /// Bits the segment needs there, header included.
        needed_bits: usize,
        /// Bits of data that version holds at the asked level.
        max_bits: usize,
    },
    /// QR-02: a forced version outside 1 to 40.
    BadVersion,
    /// QR-03: a forced mask outside 0 to 7.
    BadMask,
    /// QR-04: the data has a character the asked mode cannot carry.
    BadData,
}

impl fmt::Display for QrError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            QrError::TooLong {
                needed_bits,
                max_bits,
            } => write!(
                f,
                "QR-01 the data needs {needed_bits} bits and the code holds {max_bits}"
            ),
            QrError::BadVersion => write!(f, "QR-02 the version is not from 1 to 40"),
            QrError::BadMask => write!(f, "QR-03 the mask is not from 0 to 7"),
            QrError::BadData => write!(f, "QR-04 the data has a character the mode cannot carry"),
        }
    }
}

impl std::error::Error for QrError {}

/// A finished code: a square of dark and light modules, without the quiet
/// zone.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QrCode {
    size: usize,
    version: u8,
    mask: u8,
    ecc: Ecc,
    mode: Mode,
    modules: Vec<bool>,
}

impl QrCode {
    /// Modules along one side: 17 + 4 * version.
    pub fn size(&self) -> usize {
        self.size
    }

    /// The version, 1 to 40.
    pub fn version(&self) -> u8 {
        self.version
    }

    /// The mask used, 0 to 7.
    pub fn mask(&self) -> u8 {
        self.mask
    }

    /// The error-correction level.
    pub fn ecc(&self) -> Ecc {
        self.ecc
    }

    /// The segment mode.
    pub fn mode(&self) -> Mode {
        self.mode
    }

    /// Whether the module at column `x`, row `y` is dark. Outside the
    /// square it is light.
    pub fn get(&self, x: usize, y: usize) -> bool {
        if x >= self.size || y >= self.size {
            return false;
        }
        self.modules
            .get(y * self.size + x)
            .copied()
            .unwrap_or(false)
    }
}

/// Bits of a group of `n` digits: 3 digits take 10, 2 take 7, 1 takes 4.
fn numeric_tail_bits(n: usize) -> usize {
    match n {
        1 => 4,
        2 => 7,
        3 => 10,
        _ => 0,
    }
}

/// The four penalty rules over a finished code, for the cross-check that
/// compares mask choices. Not part of the interface to rely on.
#[doc(hidden)]
pub fn penalty_of(code: &QrCode) -> u32 {
    matrix::penalty(code.size, &code.modules)
}

/// The value of a character in the 45-character alphanumeric set.
fn alnum_value(b: u8) -> Option<u32> {
    match b {
        b'0'..=b'9' => Some(u32::from(b - b'0')),
        b'A'..=b'Z' => Some(u32::from(b - b'A') + 10),
        b' ' => Some(36),
        b'$' => Some(37),
        b'%' => Some(38),
        b'*' => Some(39),
        b'+' => Some(40),
        b'-' => Some(41),
        b'.' => Some(42),
        b'/' => Some(43),
        b':' => Some(44),
        _ => None,
    }
}

/// Encodes text: numeric mode when all digits, else alphanumeric when
/// every character is in the 45-character set, else byte mode (the UTF-8
/// bytes, no ECI).
pub fn encode_text(text: &str, ecc: Ecc) -> Result<QrCode, QrError> {
    encode_with(text.as_bytes(), ecc, None, None)
}

/// Encodes bytes in byte mode.
pub fn encode_bytes(data: &[u8], ecc: Ecc) -> Result<QrCode, QrError> {
    encode_segment(Mode::Byte, data, ecc, None, None)
}

/// Like [`encode_text`] on bytes, with the version and the mask forced
/// when given. Empty data is numeric mode and a code of no digits.
pub fn encode_with(
    data: &[u8],
    ecc: Ecc,
    version: Option<u8>,
    mask: Option<u8>,
) -> Result<QrCode, QrError> {
    encode_segment(Mode::detect(data), data, ecc, version, mask)
}

/// Encodes `data` as one segment of the given mode, with the version and
/// the mask forced when given. The mode must be able to carry every
/// character (QR-04 otherwise).
pub fn encode_segment(
    mode: Mode,
    data: &[u8],
    ecc: Ecc,
    version: Option<u8>,
    mask: Option<u8>,
) -> Result<QrCode, QrError> {
    if version.is_some_and(|v| !(1..=40).contains(&v)) {
        return Err(QrError::BadVersion);
    }
    if mask.is_some_and(|m| m > 7) {
        return Err(QrError::BadMask);
    }
    let fits = match mode {
        Mode::Numeric => data.iter().all(u8::is_ascii_digit),
        Mode::Alphanumeric => data.iter().all(|&b| alnum_value(b).is_some()),
        Mode::Byte => true,
    };
    if !fits {
        return Err(QrError::BadData);
    }
    let (first, last) = match version {
        Some(v) => (v, v),
        None => (1, 40),
    };
    // Sizes only, nothing allocated from the input until it is known to fit.
    let count = data.len();
    let mut chosen = None;
    let mut refusal = QrError::BadVersion;
    for v in first..=last {
        let capacity = tables::data_codewords(v, ecc) * 8;
        let field_ok = u32::try_from(count).is_ok_and(|c| c >> mode.count_bits(v) == 0);
        let needed = 4usize
            .saturating_add(mode.count_bits(v) as usize)
            .saturating_add(mode.data_bits(count));
        if needed <= capacity && field_ok {
            chosen = Some(v);
            break;
        }
        refusal = QrError::TooLong {
            needed_bits: needed,
            max_bits: capacity,
        };
    }
    let Some(v) = chosen else {
        return Err(refusal);
    };
    let codewords = tables::data_codewords(v, ecc);
    let bits = segment_bits(mode, data, v, codewords * 8);
    let mut data_cw = Vec::with_capacity(codewords);
    for chunk in bits.chunks(8) {
        data_cw.push(chunk.iter().fold(0u8, |a, &b| (a << 1) | u8::from(b)));
    }
    let mut pad = [0xECu8, 0x11].into_iter().cycle();
    while data_cw.len() < codewords {
        data_cw.extend(pad.next());
    }
    let all = matrix::with_error_correction(&data_cw, v, ecc);
    let (modules, used_mask) = matrix::draw(v, ecc, &all, mask);
    Ok(QrCode {
        size: tables::side(v),
        version: v,
        mask: used_mask,
        ecc,
        mode,
        modules,
    })
}

/// Appends `n` low bits of `value`, high bit first.
fn push_bits(out: &mut Vec<bool>, value: u32, n: u32) {
    for i in (0..n).rev() {
        out.push((value >> i) & 1 != 0);
    }
}

/// The segment (indicator, count, data), the terminator and the padding to
/// a whole byte, as bits. `capacity` is known to be enough.
pub(crate) fn segment_bits(mode: Mode, data: &[u8], version: u8, capacity: usize) -> Vec<bool> {
    let mut out = Vec::with_capacity(capacity);
    push_bits(&mut out, mode.indicator(), 4);
    push_bits(
        &mut out,
        u32::try_from(data.len()).unwrap_or(0),
        mode.count_bits(version),
    );
    match mode {
        Mode::Numeric => {
            for group in data.chunks(3) {
                let value = group
                    .iter()
                    .fold(0u32, |a, &b| a * 10 + u32::from(b.wrapping_sub(b'0')));
                push_bits(&mut out, value, numeric_tail_bits(group.len()) as u32);
            }
        }
        Mode::Alphanumeric => {
            for pair in data.chunks(2) {
                let v = pair
                    .iter()
                    .fold(0u32, |a, &b| a * 45 + alnum_value(b).unwrap_or(0));
                push_bits(&mut out, v, if pair.len() == 2 { 11 } else { 6 });
            }
        }
        Mode::Byte => {
            for &b in data {
                push_bits(&mut out, u32::from(b), 8);
            }
        }
    }
    let terminator = capacity.saturating_sub(out.len()).min(4);
    out.extend(std::iter::repeat_n(false, terminator));
    while out.len() % 8 != 0 {
        out.push(false);
    }
    out
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod decode_tests;
