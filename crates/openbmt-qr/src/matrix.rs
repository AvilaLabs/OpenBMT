//! Drawing a symbol: function patterns, codeword placement, masks and the
//! penalty rules (ISO/IEC 18004, 7.8.3).

use crate::{rs, tables, Ecc};

/// A square of modules with a note of which are function modules.
#[derive(Clone)]
struct Grid {
    size: usize,
    dark: Vec<bool>,
    func: Vec<bool>,
}

impl Grid {
    fn new(size: usize) -> Self {
        Grid {
            size,
            dark: vec![false; size * size],
            func: vec![false; size * size],
        }
    }

    fn index(&self, x: i64, y: i64) -> Option<usize> {
        let s = self.size as i64;
        if x < 0 || y < 0 || x >= s || y >= s {
            return None;
        }
        Some(y as usize * self.size + x as usize)
    }

    /// Sets a function module. Coordinates outside the square are ignored.
    fn set_func(&mut self, x: i64, y: i64, dark: bool) {
        if let Some(i) = self.index(x, y) {
            if let Some(d) = self.dark.get_mut(i) {
                *d = dark;
            }
            if let Some(f) = self.func.get_mut(i) {
                *f = true;
            }
        }
    }

    fn is_func(&self, x: i64, y: i64) -> bool {
        self.index(x, y)
            .and_then(|i| self.func.get(i))
            .copied()
            .unwrap_or(true)
    }

    fn finder(&mut self, cx: i64, cy: i64) {
        for dy in -4i64..=4 {
            for dx in -4i64..=4 {
                let dist = dx.abs().max(dy.abs());
                self.set_func(cx + dx, cy + dy, dist != 2 && dist != 4);
            }
        }
    }

    fn alignment(&mut self, cx: i64, cy: i64) {
        for dy in -2i64..=2 {
            for dx in -2i64..=2 {
                self.set_func(cx + dx, cy + dy, dx.abs().max(dy.abs()) != 1);
            }
        }
    }

    fn draw_function_patterns(&mut self, version: u8) {
        let size = self.size as i64;
        for i in 0..size {
            self.set_func(6, i, i % 2 == 0);
            self.set_func(i, 6, i % 2 == 0);
        }
        self.finder(3, 3);
        self.finder(size - 4, 3);
        self.finder(3, size - 4);
        let pos = tables::alignment_positions(version);
        let last = pos.len().saturating_sub(1);
        for (i, &a) in pos.iter().enumerate() {
            for (j, &b) in pos.iter().enumerate() {
                let corner = (i == 0 && j == 0) || (i == 0 && j == last) || (i == last && j == 0);
                if !corner {
                    self.alignment(a as i64, b as i64);
                }
            }
        }
        // Reserve the format areas (their content comes with the mask).
        self.draw_format(Ecc::L, 0);
        self.draw_version(version);
    }

    fn draw_format(&mut self, ecc: Ecc, mask: u8) {
        let bits = u32::from(format_bits(ecc, mask));
        let bit = |i: u32| (bits >> i) & 1 != 0;
        let size = self.size as i64;
        for i in 0..6 {
            self.set_func(8, i64::from(i), bit(i));
        }
        self.set_func(8, 7, bit(6));
        self.set_func(8, 8, bit(7));
        self.set_func(7, 8, bit(8));
        for i in 9..15 {
            self.set_func(14 - i64::from(i), 8, bit(i));
        }
        for i in 0..8 {
            self.set_func(size - 1 - i64::from(i), 8, bit(i));
        }
        for i in 8..15 {
            self.set_func(8, size - 15 + i64::from(i), bit(i));
        }
        // The dark module.
        self.set_func(8, size - 8, true);
    }

    fn draw_version(&mut self, version: u8) {
        if version < 7 {
            return;
        }
        let bits = version_bits(version);
        let size = self.size as i64;
        for i in 0..18u32 {
            let dark = (bits >> i) & 1 != 0;
            let a = size - 11 + i64::from(i % 3);
            let b = i64::from(i / 3);
            self.set_func(a, b, dark);
            self.set_func(b, a, dark);
        }
    }

    /// Writes the codeword bits in the zigzag of the standard, skipping
    /// function modules. Modules left over stay light.
    fn place(&mut self, codewords: &[u8]) {
        let size = self.size as i64;
        let total_bits = codewords.len() * 8;
        let mut i = 0usize;
        let mut right = size - 1;
        while right >= 1 {
            if right == 6 {
                right = 5;
            }
            for vert in 0..size {
                for j in 0..2 {
                    let x = right - j;
                    let upward = ((right + 1) & 2) == 0;
                    let y = if upward { size - 1 - vert } else { vert };
                    if !self.is_func(x, y) && i < total_bits {
                        let byte = codewords.get(i >> 3).copied().unwrap_or(0);
                        let on = (byte >> (7 - (i & 7))) & 1 != 0;
                        if let Some(k) = self.index(x, y) {
                            if let Some(d) = self.dark.get_mut(k) {
                                *d = on;
                            }
                        }
                        i += 1;
                    }
                }
            }
            right -= 2;
        }
    }

    fn apply_mask(&mut self, mask: u8) {
        let size = self.size;
        for y in 0..size {
            for x in 0..size {
                let k = y * size + x;
                if self.func.get(k).copied().unwrap_or(true) {
                    continue;
                }
                if mask_bit(mask, x, y) {
                    if let Some(d) = self.dark.get_mut(k) {
                        *d = !*d;
                    }
                }
            }
        }
    }
}

/// Whether mask `mask` flips the module at column `x`, row `y`.
pub(crate) fn mask_bit(mask: u8, x: usize, y: usize) -> bool {
    match mask {
        0 => (x + y).is_multiple_of(2),
        1 => y.is_multiple_of(2),
        2 => x.is_multiple_of(3),
        3 => (x + y).is_multiple_of(3),
        4 => (x / 3 + y / 2).is_multiple_of(2),
        5 => x * y % 2 + x * y % 3 == 0,
        6 => (x * y % 2 + x * y % 3).is_multiple_of(2),
        7 => ((x + y) % 2 + x * y % 3).is_multiple_of(2),
        _ => false,
    }
}

/// The 15 format bits (BCH 15,5, masked with 0x5412) of a level and mask.
pub fn format_bits(ecc: Ecc, mask: u8) -> u16 {
    let level: u32 = match ecc {
        Ecc::L => 1,
        Ecc::M => 0,
        Ecc::Q => 3,
        Ecc::H => 2,
    };
    let data = (level << 3) | u32::from(mask & 7);
    let mut rem = data;
    for _ in 0..10 {
        rem = (rem << 1) ^ ((rem >> 9) * 0x537);
    }
    (((data << 10) | rem) ^ 0x5412) as u16
}

/// The 18 version bits (BCH 18,6) of a version from 7 up.
pub fn version_bits(version: u8) -> u32 {
    let v = u32::from(version);
    let mut rem = v;
    for _ in 0..12 {
        rem = (rem << 1) ^ ((rem >> 11) * 0x1F25);
    }
    (v << 12) | rem
}

/// Splits the data codewords into blocks, adds the error-correction
/// codewords and interleaves them (ISO/IEC 18004, 7.6).
pub(crate) fn with_error_correction(data: &[u8], version: u8, ecc: Ecc) -> Vec<u8> {
    let n_blocks = tables::blocks(version, ecc);
    let ecc_len = tables::ecc_per_block(version, ecc);
    let raw = tables::total_codewords(version);
    if n_blocks == 0 {
        return Vec::new();
    }
    let n_short = n_blocks - raw % n_blocks;
    let short_len = raw / n_blocks;
    let div = rs::divisor(ecc_len);
    let mut data_blocks: Vec<&[u8]> = Vec::with_capacity(n_blocks);
    let mut ecc_blocks: Vec<Vec<u8>> = Vec::with_capacity(n_blocks);
    let mut rest = data;
    for i in 0..n_blocks {
        let len = (short_len - ecc_len + usize::from(i >= n_short)).min(rest.len());
        let (block, tail) = rest.split_at(len);
        rest = tail;
        data_blocks.push(block);
        ecc_blocks.push(rs::remainder(block, &div));
    }
    let mut out = Vec::with_capacity(raw);
    let longest = data_blocks.iter().map(|b| b.len()).max().unwrap_or(0);
    for i in 0..longest {
        out.extend(data_blocks.iter().filter_map(|b| b.get(i)));
    }
    for i in 0..ecc_len {
        out.extend(ecc_blocks.iter().filter_map(|b| b.get(i)));
    }
    out
}

/// Draws the symbol for `codewords`. With no mask given, the mask with the
/// lowest penalty wins and ties go to the lower number. Returns the
/// modules (row by row) and the mask used.
pub(crate) fn draw(
    version: u8,
    ecc: Ecc,
    codewords: &[u8],
    forced_mask: Option<u8>,
) -> (Vec<bool>, u8) {
    let mut base = Grid::new(tables::side(version));
    base.draw_function_patterns(version);
    base.place(codewords);
    let candidates: Vec<u8> = match forced_mask {
        Some(m) => vec![m],
        None => (0..8).collect(),
    };
    let mut best: Option<(u32, u8, Grid)> = None;
    for m in candidates {
        let mut g = base.clone();
        g.apply_mask(m);
        g.draw_format(ecc, m);
        let p = penalty(g.size, &g.dark);
        if best.as_ref().is_none_or(|(bp, _, _)| p < *bp) {
            best = Some((p, m, g));
        }
    }
    match best {
        Some((_, m, g)) => (g.dark, m),
        None => (base.dark, 0),
    }
}

/// Rule 1 on one line: 3 for a run of five of one colour, and 1 for each
/// module more.
pub(crate) fn run_penalty(line: &[bool]) -> u32 {
    let mut total = 0;
    let mut run = 0u32;
    let mut prev: Option<bool> = None;
    for &m in line {
        if prev == Some(m) {
            run += 1;
        } else {
            run = 1;
            prev = Some(m);
        }
        if run == 5 {
            total += 3;
        } else if run > 5 {
            total += 1;
        }
    }
    total
}

/// Rule 3 on one line: 40 for each 1:1:3:1:1 pattern (dark, light, three
/// dark, light, dark) with four light modules before or after it. The four
/// light modules must lie inside the symbol; the quiet zone is not counted
/// as them.
pub(crate) fn finder_like_penalty(line: &[bool]) -> u32 {
    const CORE: [bool; 7] = [true, false, true, true, true, false, true];
    let mut total = 0;
    for start in 0..line.len().saturating_sub(6) {
        let Some(window) = line.get(start..start + 7) else {
            continue;
        };
        if window != CORE {
            continue;
        }
        let light_before = start >= 4
            && line
                .get(start - 4..start)
                .is_some_and(|s| s.iter().all(|&m| !m));
        let light_after = line
            .get(start + 7..start + 11)
            .is_some_and(|s| s.iter().all(|&m| !m));
        if light_before || light_after {
            total += 40;
        }
    }
    total
}

/// Rule 2: 3 for each 2 by 2 block of one colour.
pub(crate) fn block_penalty(size: usize, modules: &[bool]) -> u32 {
    if size == 0 {
        return 0;
    }
    let rows: Vec<&[bool]> = modules.chunks(size).collect();
    let mut total = 0;
    for pair in rows.windows(2) {
        let (Some(a), Some(b)) = (pair.first(), pair.get(1)) else {
            continue;
        };
        for (wa, wb) in a.windows(2).zip(b.windows(2)) {
            if let (Some(&p), Some(&q), Some(&r), Some(&s)) =
                (wa.first(), wa.get(1), wb.first(), wb.get(1))
            {
                if p == q && q == r && r == s {
                    total += 3;
                }
            }
        }
    }
    total
}

/// Rule 4: with the share of dark modules between two multiples of 5
/// percent, take each one's distance from 50 percent in steps of 5, the
/// smaller of the two, times 10. A share that is a multiple of 5 itself is
/// the lower of the two.
pub(crate) fn balance_penalty(dark: usize, total: usize) -> u32 {
    if total == 0 {
        return 0;
    }
    // The lower multiple, in steps of 5 percent (0 to 20).
    let low = dark * 20 / total;
    let steps = low.abs_diff(10).min((low + 1).abs_diff(10));
    (steps * 10) as u32
}

/// The four penalty rules over a whole symbol.
pub(crate) fn penalty(size: usize, modules: &[bool]) -> u32 {
    let step = size.max(1);
    let mut total = 0;
    for row in modules.chunks(step) {
        total += run_penalty(row) + finder_like_penalty(row);
    }
    for x in 0..size {
        let col: Vec<bool> = modules.iter().skip(x).step_by(step).copied().collect();
        total += run_penalty(&col) + finder_like_penalty(&col);
    }
    total += block_penalty(size, modules);
    total += balance_penalty(modules.iter().filter(|&&m| m).count(), modules.len());
    total
}
