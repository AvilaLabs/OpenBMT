//! From a grid of modules to bytes: format and version information with
//! error correction, the codewords in placement order, the blocks, and the
//! segments.

use crate::decode::DecodeError;
use crate::matrix::{format_bits, function_modules, mask_bit, version_bits};
use crate::{rs, tables, Ecc};

fn grid_bit(grid: &[bool], size: usize, x: usize, y: usize) -> bool {
    grid.get(y * size + x).copied().unwrap_or(false)
}

/// The 15 format bits of both copies, as written by the encoder.
fn read_format(grid: &[bool], n: usize) -> (u32, u32) {
    let bit = |x: usize, y: usize, i: u32| u32::from(grid_bit(grid, n, x, y)) << i;
    let mut first = 0;
    for i in 0..6 {
        first |= bit(8, i as usize, i);
    }
    first |= bit(8, 7, 6) | bit(8, 8, 7) | bit(7, 8, 8);
    for i in 9..15 {
        first |= bit(14 - i as usize, 8, i);
    }
    let mut second = 0;
    for i in 0..8 {
        second |= bit(n - 1 - i as usize, 8, i);
    }
    for i in 8..15 {
        second |= bit(8, n - 15 + i as usize, i);
    }
    (first, second)
}

/// The level and mask of the nearest valid format word within distance 3
/// of each copy, the closer first.
pub(crate) fn formats(grid: &[bool], n: usize) -> Vec<(Ecc, u8)> {
    let (a, b) = read_format(grid, n);
    let mut found: Vec<(u32, Ecc, u8)> = Vec::new();
    for word in [a, b] {
        let mut best: Option<(u32, Ecc, u8)> = None;
        for ecc in Ecc::ALL {
            for mask in 0..8u8 {
                let d = (word ^ u32::from(format_bits(ecc, mask))).count_ones();
                if d <= 3 && best.is_none_or(|(bd, _, _)| d < bd) {
                    best = Some((d, ecc, mask));
                }
            }
        }
        if let Some(f) = best {
            if !found.iter().any(|&(_, e, m)| (e, m) == (f.1, f.2)) {
                found.push(f);
            }
        }
    }
    found.sort_by_key(|&(d, _, _)| d);
    found.into_iter().map(|(_, e, m)| (e, m)).collect()
}

/// The version written in the two version-information areas (versions 7
/// and up), by the nearest valid word within distance 3 of either copy.
pub(crate) fn read_version(grid: &[bool], n: usize) -> Option<u8> {
    let mut a = 0u32;
    let mut b = 0u32;
    for i in 0..18u32 {
        let p = n - 11 + (i % 3) as usize;
        let q = (i / 3) as usize;
        a |= u32::from(grid_bit(grid, n, p, q)) << i;
        b |= u32::from(grid_bit(grid, n, q, p)) << i;
    }
    let mut best: Option<(u32, u8)> = None;
    for word in [a, b] {
        for v in 7..=40u8 {
            let d = (word ^ version_bits(v)).count_ones();
            if d <= 3 && best.is_none_or(|(bd, _)| d < bd) {
                best = Some((d, v));
            }
        }
    }
    best.map(|(_, v)| v)
}

/// The grid with rows and columns swapped.
pub(crate) fn transpose(grid: &[bool], n: usize) -> Vec<bool> {
    let mut out = vec![false; n * n];
    for y in 0..n {
        for x in 0..n {
            if let Some(m) = out.get_mut(x * n + y) {
                *m = grid_bit(grid, n, x, y);
            }
        }
    }
    out
}

/// The codewords of a symbol, unmasked, in the order they were placed.
fn read_codewords(grid: &[bool], version: u8, mask: u8) -> Vec<u8> {
    let n = tables::side(version);
    let func = function_modules(version);
    let total = tables::total_codewords(version);
    let mut out = vec![0u8; total];
    let mut i = 0usize;
    let ni = n as i64;
    let mut right = ni - 1;
    while right >= 1 {
        if right == 6 {
            right = 5;
        }
        for vert in 0..ni {
            for j in 0..2 {
                let x = (right - j) as usize;
                let upward = ((right + 1) & 2) == 0;
                let y = (if upward { ni - 1 - vert } else { vert }) as usize;
                if func.get(y * n + x).copied().unwrap_or(true) {
                    continue;
                }
                if i < total * 8 {
                    let bit = grid_bit(grid, n, x, y) ^ mask_bit(mask, x, y);
                    if let Some(byte) = out.get_mut(i >> 3) {
                        *byte |= u8::from(bit) << (7 - (i & 7));
                    }
                    i += 1;
                }
            }
        }
        right -= 2;
    }
    out
}

/// De-interleaves the codewords, corrects every block, and returns the data
/// codewords in order with the number of codewords corrected.
fn correct_blocks(
    codewords: &[u8],
    version: u8,
    ecc: Ecc,
) -> Result<(Vec<u8>, usize), DecodeError> {
    let n_blocks = tables::blocks(version, ecc);
    let ecc_len = tables::ecc_per_block(version, ecc);
    let raw = tables::total_codewords(version);
    if n_blocks == 0 || codewords.len() != raw {
        return Err(DecodeError::TooManyErrors);
    }
    let n_short = n_blocks - raw % n_blocks;
    let short_len = raw / n_blocks;
    let data_len = |b: usize| short_len - ecc_len + usize::from(b >= n_short);
    let mut blocks: Vec<Vec<u8>> = vec![Vec::new(); n_blocks];
    let mut it = codewords.iter().copied();
    let longest = short_len - ecc_len + 1;
    for i in 0..longest {
        for (b, block) in blocks.iter_mut().enumerate() {
            if i < data_len(b) {
                block.extend(it.next());
            }
        }
    }
    for _ in 0..ecc_len {
        for block in blocks.iter_mut() {
            block.extend(it.next());
        }
    }
    let mut data = Vec::with_capacity(raw);
    let mut fixed = 0;
    for (b, block) in blocks.iter_mut().enumerate() {
        fixed += rs::correct(block, ecc_len).ok_or(DecodeError::TooManyErrors)?;
        data.extend(block.iter().take(data_len(b)));
    }
    Ok((data, fixed))
}

/// What decoding one grid with one format word gives.
pub(crate) struct Read {
    pub(crate) bytes: Vec<u8>,
    pub(crate) corrected: usize,
}

/// Decodes a grid of a known version with a given level and mask.
pub(crate) fn decode_with(
    grid: &[bool],
    version: u8,
    ecc: Ecc,
    mask: u8,
) -> Result<Read, DecodeError> {
    let codewords = read_codewords(grid, version, mask);
    let (data, corrected) = correct_blocks(&codewords, version, ecc)?;
    let bytes = segments(&data, version)?;
    Ok(Read { bytes, corrected })
}

struct Bits<'a> {
    data: &'a [u8],
    pos: usize,
}

impl Bits<'_> {
    fn left(&self) -> usize {
        (self.data.len() * 8).saturating_sub(self.pos)
    }

    fn take(&mut self, n: usize) -> Option<u32> {
        if n > self.left() || n > 24 {
            return None;
        }
        let mut v = 0u32;
        for _ in 0..n {
            let byte = self.data.get(self.pos >> 3).copied()?;
            v = (v << 1) | u32::from((byte >> (7 - (self.pos & 7))) & 1);
            self.pos += 1;
        }
        Some(v)
    }
}

const ALNUM: &[u8; 45] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ $%*+-./:";

/// The bytes of the segments. Numeric and alphanumeric text is ASCII; byte
/// segments are as written, except that after an ECI of ISO-8859-1 they
/// are turned into UTF-8. ECI 26 (UTF-8) and 3 (ISO-8859-1) are the only
/// ones accepted.
fn segments(data: &[u8], version: u8) -> Result<Vec<u8>, DecodeError> {
    let mut bits = Bits { data, pos: 0 };
    let mut out: Vec<u8> = Vec::new();
    let mut latin1 = false;
    let small = version <= 9;
    let large = version >= 27;
    let count_bits = |mode: u32| -> usize {
        let (a, b, c) = match mode {
            1 => (10, 12, 14),
            2 => (9, 11, 13),
            _ => (8, 16, 16),
        };
        if small {
            a
        } else if large {
            c
        } else {
            b
        }
    };
    loop {
        if bits.left() < 4 {
            return Ok(out);
        }
        let mode = bits.take(4).ok_or(DecodeError::BadSegment)?;
        match mode {
            0 => return Ok(out),
            1 => {
                let mut count = bits.take(count_bits(1)).ok_or(DecodeError::BadSegment)?;
                while count > 0 {
                    let n = count.min(3);
                    let width = match n {
                        1 => 4,
                        2 => 7,
                        _ => 10,
                    };
                    let v = bits.take(width).ok_or(DecodeError::BadSegment)?;
                    if v >= 10u32.pow(n) {
                        return Err(DecodeError::BadSegment);
                    }
                    let digits = format!("{v:0width$}", width = n as usize);
                    out.extend(digits.bytes());
                    count -= n;
                }
            }
            2 => {
                let mut count = bits.take(count_bits(2)).ok_or(DecodeError::BadSegment)?;
                while count > 0 {
                    if count >= 2 {
                        let v = bits.take(11).ok_or(DecodeError::BadSegment)?;
                        if v >= 45 * 45 {
                            return Err(DecodeError::BadSegment);
                        }
                        out.extend(ALNUM.get((v / 45) as usize));
                        out.extend(ALNUM.get((v % 45) as usize));
                        count -= 2;
                    } else {
                        let v = bits.take(6).ok_or(DecodeError::BadSegment)?;
                        if v >= 45 {
                            return Err(DecodeError::BadSegment);
                        }
                        out.extend(ALNUM.get(v as usize));
                        count -= 1;
                    }
                }
            }
            4 => {
                let count = bits.take(count_bits(4)).ok_or(DecodeError::BadSegment)?;
                if (count as usize) * 8 > bits.left() {
                    return Err(DecodeError::BadSegment);
                }
                for _ in 0..count {
                    let b = bits.take(8).ok_or(DecodeError::BadSegment)? as u8;
                    if latin1 && b >= 0x80 {
                        out.extend(char::from(b).to_string().bytes());
                    } else {
                        out.push(b);
                    }
                }
            }
            7 => {
                let first = bits.take(8).ok_or(DecodeError::BadSegment)?;
                let designator = if first & 0x80 == 0 {
                    first
                } else if first & 0xC0 == 0x80 {
                    let rest = bits.take(8).ok_or(DecodeError::BadSegment)?;
                    ((first & 0x3F) << 8) | rest
                } else if first & 0xE0 == 0xC0 {
                    let rest = bits.take(16).ok_or(DecodeError::BadSegment)?;
                    ((first & 0x1F) << 16) | rest
                } else {
                    return Err(DecodeError::BadSegment);
                };
                match designator {
                    26 => latin1 = false,
                    3 => latin1 = true,
                    _ => return Err(DecodeError::UnsupportedSegment),
                }
            }
            // Kanji, structured append, FNC1 and anything unassigned.
            _ => return Err(DecodeError::UnsupportedSegment),
        }
    }
}
