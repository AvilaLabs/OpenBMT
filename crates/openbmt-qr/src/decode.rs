//! Reading a code from a grayscale camera frame. The frame is untrusted:
//! its size is bounded, every loop is charged to a work budget, and no
//! input can make this panic.

use std::fmt;

use crate::readback::{decode_with, formats, read_version, transpose};
use crate::{binarize, finder, sample, tables, Ecc};

/// Smallest side of a frame, in pixels.
pub const MIN_SIDE: usize = 16;
/// Largest side of a frame, in pixels.
pub const MAX_SIDE: usize = 4096;

/// A code read from a frame.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Decoded {
    /// The data: the segments' bytes joined. After an ECI of ISO-8859-1
    /// the bytes of a byte segment are UTF-8; otherwise they are as
    /// written.
    pub bytes: Vec<u8>,
    /// The version, 1 to 40.
    pub version: u8,
    /// The error-correction level.
    pub ecc: Ecc,
    /// The mask, 0 to 7.
    pub mask: u8,
    /// Codewords that had to be corrected, over all blocks.
    pub corrected_errors: usize,
}

/// Why a frame gave no data. Every text starts with its code.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DecodeError {
    /// QR-10: a side under 16 or the buffer does not match the sides and
    /// the row stride.
    BadFrame,
    /// QR-11: a side over 4096.
    TooLarge,
    /// QR-12: no code found.
    NotFound,
    /// QR-13: a code was found and its format information could not be
    /// read, also not on the transposed grid.
    FormatUnreadable,
    /// QR-14: more errors than the error correction can repair.
    TooManyErrors,
    /// QR-15: a segment of a kind that is refused: Kanji, structured
    /// append, FNC1, or an ECI other than 26 and 3.
    UnsupportedSegment,
    /// QR-16: the work budget for one frame ran out.
    Budget,
    /// QR-17: the corrected data is not a well-formed segment stream.
    BadSegment,
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DecodeError::BadFrame => write!(
                f,
                "QR-10 the frame is under 16 pixels on a side, or its buffer does not fit its size"
            ),
            DecodeError::TooLarge => write!(f, "QR-11 the frame is over 4096 pixels on a side"),
            DecodeError::NotFound => write!(f, "QR-12 no code was found in the frame"),
            DecodeError::FormatUnreadable => {
                write!(f, "QR-13 the format information of the code cannot be read")
            }
            DecodeError::TooManyErrors => {
                write!(f, "QR-14 the code has more errors than it can correct")
            }
            DecodeError::UnsupportedSegment => write!(
                f,
                "QR-15 the code has a segment this reader refuses (Kanji, structured append, FNC1 or an ECI other than 26 and 3)"
            ),
            DecodeError::Budget => write!(f, "QR-16 the work budget for one frame ran out"),
            DecodeError::BadSegment => write!(f, "QR-17 the data of the code is not well formed"),
        }
    }
}

impl std::error::Error for DecodeError {}

/// The work one frame may cost, in units of about one pixel visit.
pub(crate) struct Budget {
    left: u64,
}

impl Budget {
    fn new(w: usize, h: usize) -> Self {
        Budget {
            left: 16 * (w as u64) * (h as u64) + 8_000_000,
        }
    }

    pub(crate) fn charge(&mut self, n: u64) -> Result<(), DecodeError> {
        if n > self.left {
            self.left = 0;
            return Err(DecodeError::Budget);
        }
        self.left -= n;
        Ok(())
    }
}

/// Reads a code from an 8-bit grayscale frame of exactly `width * height`
/// bytes, row by row (the Y plane of a camera frame without padding).
pub fn decode_luma(width: usize, height: usize, luma: &[u8]) -> Result<Decoded, DecodeError> {
    if luma.len() != width.saturating_mul(height) {
        return check_size(width, height).and(Err(DecodeError::BadFrame));
    }
    decode_luma_strided(width, height, width, luma)
}

fn check_size(width: usize, height: usize) -> Result<(), DecodeError> {
    if width > MAX_SIDE || height > MAX_SIDE {
        Err(DecodeError::TooLarge)
    } else if width < MIN_SIDE || height < MIN_SIDE {
        Err(DecodeError::BadFrame)
    } else {
        Ok(())
    }
}

/// Like [`decode_luma`] for a frame whose rows are `stride` bytes apart
/// (at least `width`). The last row need only be `width` bytes long.
pub fn decode_luma_strided(
    width: usize,
    height: usize,
    stride: usize,
    luma: &[u8],
) -> Result<Decoded, DecodeError> {
    check_size(width, height)?;
    let needed = (height - 1) * stride.min(1 << 20) + width;
    if stride < width || stride > 1 << 20 || luma.len() < needed {
        return Err(DecodeError::BadFrame);
    }
    let mut budget = Budget::new(width, height);
    let bin = binarize::binarize(width, height, stride, luma, &mut budget)?;
    let candidates = finder::find_candidates(&bin, &mut budget)?;
    let mut seen = DecodeError::NotFound;
    for triple in finder::triples(&candidates).into_iter().take(6) {
        match attempt(&bin, &triple, &mut budget) {
            Ok(d) => return Ok(d),
            Err(e @ (DecodeError::UnsupportedSegment | DecodeError::BadSegment)) => return Err(e),
            Err(DecodeError::Budget) => return Err(DecodeError::Budget),
            Err(e) => seen = worse(seen, e),
        }
    }
    Err(seen)
}

/// The more informative of two refusals.
fn worse(a: DecodeError, b: DecodeError) -> DecodeError {
    let rank = |e: DecodeError| match e {
        DecodeError::TooManyErrors => 2,
        DecodeError::FormatUnreadable => 1,
        _ => 0,
    };
    if rank(b) > rank(a) {
        b
    } else {
        a
    }
}

enum Tried {
    Read(Decoded),
    /// The version information names another version.
    Other(u8),
    Failed(DecodeError),
}

/// Samples the grid at one version and reads it. Refusals of the segments
/// are final and come back as `Err`.
fn try_version(
    bin: &binarize::Bin,
    frame: &sample::Frame,
    version: u8,
    refine: bool,
    budget: &mut Budget,
) -> Result<Tried, DecodeError> {
    let Some(grid) = sample::sample_grid(bin, frame, version, refine, budget)? else {
        return Ok(Tried::Failed(DecodeError::FormatUnreadable));
    };
    let n = tables::side(version);
    if version >= 7 {
        if let Some(v) = read_version(&grid, n).or_else(|| read_version(&transpose(&grid, n), n)) {
            if v != version {
                return Ok(Tried::Other(v));
            }
        }
    }
    let mut seen = DecodeError::FormatUnreadable;
    // A mirrored code reads as its transpose.
    for g in [grid.clone(), transpose(&grid, n)] {
        match decode_grid(&g, version) {
            Ok(d) => return Ok(Tried::Read(d)),
            Err(e @ (DecodeError::UnsupportedSegment | DecodeError::BadSegment)) => return Err(e),
            Err(e) => seen = worse(seen, e),
        }
    }
    Ok(Tried::Failed(seen))
}

/// Decodes the code the three finders mark.
fn attempt(
    bin: &binarize::Bin,
    triple: &[finder::Finder; 3],
    budget: &mut Budget,
) -> Result<Decoded, DecodeError> {
    let Some(frame) = sample::frame_of(bin, triple, budget)? else {
        return Err(DecodeError::NotFound);
    };
    let mut queue = sample::version_candidates(frame.dimension);
    let mut tried: Vec<u8> = Vec::new();
    let mut seen = DecodeError::FormatUnreadable;
    while !queue.is_empty() && tried.len() < 4 {
        let version = queue.remove(0);
        if tried.contains(&version) {
            continue;
        }
        tried.push(version);
        // Without, then with the alignment patterns beyond the one near
        // the corner (only versions 7 and up have more).
        for refine in [false, true] {
            if refine && version < 7 {
                break;
            }
            match try_version(bin, &frame, version, refine, budget)? {
                Tried::Read(d) => return Ok(d),
                Tried::Other(v) => {
                    queue.insert(0, v);
                    break;
                }
                Tried::Failed(e) => seen = worse(seen, e),
            }
        }
    }
    Err(seen)
}

/// Decodes one grid of a known version: each format word that reads is
/// tried in turn.
fn decode_grid(grid: &[bool], version: u8) -> Result<Decoded, DecodeError> {
    let n = tables::side(version);
    let words = formats(grid, n);
    let mut seen = DecodeError::FormatUnreadable;
    for (ecc, mask) in words {
        match decode_with(grid, version, ecc, mask) {
            Ok(r) => {
                return Ok(Decoded {
                    bytes: r.bytes,
                    version,
                    ecc,
                    mask,
                    corrected_errors: r.corrected,
                })
            }
            Err(e @ (DecodeError::UnsupportedSegment | DecodeError::BadSegment)) => return Err(e),
            Err(e) => seen = worse(seen, e),
        }
    }
    Err(seen)
}
