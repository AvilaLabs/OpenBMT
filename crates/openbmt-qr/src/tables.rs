//! The standard's tables (ISO/IEC 18004, Tables 9 and 13 to 22), by level.

use crate::Ecc;

/// Error-correction codewords per block, versions 1 to 40, by level
/// (L, M, Q, H).
const ECC_PER_BLOCK: [[u8; 40]; 4] = [
    [
        7, 10, 15, 20, 26, 18, 20, 24, 30, 18, 20, 24, 26, 30, 22, 24, 28, 30, 28, 28, 28, 28, 30,
        30, 26, 28, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30,
    ],
    [
        10, 16, 26, 18, 24, 16, 18, 22, 22, 26, 30, 22, 22, 24, 24, 28, 28, 26, 26, 26, 26, 28, 28,
        28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28,
    ],
    [
        13, 22, 18, 26, 18, 24, 18, 22, 20, 24, 28, 26, 24, 20, 30, 24, 28, 28, 26, 30, 28, 30, 30,
        30, 30, 28, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30,
    ],
    [
        17, 28, 22, 16, 22, 28, 26, 26, 24, 28, 24, 28, 22, 24, 24, 30, 28, 28, 26, 28, 30, 24, 30,
        30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30,
    ],
];

/// Number of error-correction blocks, versions 1 to 40, by level.
const BLOCKS: [[u8; 40]; 4] = [
    [
        1, 1, 1, 1, 1, 2, 2, 2, 2, 4, 4, 4, 4, 4, 6, 6, 6, 6, 7, 8, 8, 9, 9, 10, 12, 12, 12, 13,
        14, 15, 16, 17, 18, 19, 19, 20, 21, 22, 24, 25,
    ],
    [
        1, 1, 1, 2, 2, 4, 4, 4, 5, 5, 5, 8, 9, 9, 10, 10, 11, 13, 14, 16, 17, 17, 18, 20, 21, 23,
        25, 26, 28, 29, 31, 33, 35, 37, 38, 40, 43, 45, 47, 49,
    ],
    [
        1, 1, 2, 2, 4, 4, 6, 6, 8, 8, 8, 10, 12, 16, 12, 17, 16, 18, 21, 20, 23, 23, 25, 27, 29,
        34, 34, 35, 38, 40, 43, 45, 48, 51, 53, 56, 59, 62, 65, 68,
    ],
    [
        1, 1, 2, 4, 4, 4, 5, 6, 8, 8, 11, 11, 16, 16, 18, 16, 19, 21, 25, 25, 25, 34, 30, 32, 35,
        37, 40, 42, 45, 48, 51, 54, 57, 60, 63, 66, 70, 74, 77, 81,
    ],
];

fn row(ecc: Ecc) -> usize {
    match ecc {
        Ecc::L => 0,
        Ecc::M => 1,
        Ecc::Q => 2,
        Ecc::H => 3,
    }
}

fn pick(table: &[[u8; 40]; 4], version: u8, ecc: Ecc) -> usize {
    let i = usize::from(version).saturating_sub(1);
    table
        .get(row(ecc))
        .and_then(|r| r.get(i))
        .copied()
        .map_or(0, usize::from)
}

/// Error-correction codewords in each block.
pub(crate) fn ecc_per_block(version: u8, ecc: Ecc) -> usize {
    pick(&ECC_PER_BLOCK, version, ecc)
}

/// Number of blocks.
pub(crate) fn blocks(version: u8, ecc: Ecc) -> usize {
    pick(&BLOCKS, version, ecc)
}

/// Modules of a version's side.
pub(crate) fn side(version: u8) -> usize {
    usize::from(version) * 4 + 17
}

/// Modules that can carry codeword bits: everything but the function
/// patterns, format and version information.
pub(crate) fn raw_modules(version: u8) -> usize {
    let v = usize::from(version);
    let mut n = (16 * v + 128) * v + 64;
    if v >= 2 {
        let a = v / 7 + 2;
        n -= (25 * a - 10) * a - 55;
        if v >= 7 {
            n -= 36;
        }
    }
    n
}

/// All codewords of a symbol, data and error correction.
pub(crate) fn total_codewords(version: u8) -> usize {
    raw_modules(version) / 8
}

/// Data codewords at a version and level.
pub(crate) fn data_codewords(version: u8, ecc: Ecc) -> usize {
    total_codewords(version).saturating_sub(ecc_per_block(version, ecc) * blocks(version, ecc))
}

/// Centres of the alignment patterns along one axis (empty for version 1).
pub(crate) fn alignment_positions(version: u8) -> Vec<usize> {
    if version <= 1 {
        return Vec::new();
    }
    let v = usize::from(version);
    let n = v / 7 + 2;
    let step = if v == 32 {
        26
    } else {
        (v * 4 + n * 2 + 1) / (n * 2 - 2) * 2
    };
    let mut out = vec![6];
    let mut pos = side(version) - 7;
    for _ in 1..n {
        out.insert(1, pos);
        pos = pos.saturating_sub(step);
    }
    out
}
