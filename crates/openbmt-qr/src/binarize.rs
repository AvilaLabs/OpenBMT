//! Local adaptive thresholding of a grayscale frame: the mean of each 8 by
//! 8 block, smoothed over the 5 by 5 blocks around it, in the manner of
//! ZXing's hybrid binarizer. Flat blocks take the level of their
//! neighbours, so a plain light area is not turned into noise.

use crate::decode::{Budget, DecodeError};

const BLOCK: usize = 8;
/// A block whose light and dark halves (above and below its mean) are not
/// at least this far apart in mean is flat: only noise tells its pixels
/// apart.
const MIN_SPREAD: u32 = 12;

/// A frame as dark and light pixels. Outside the frame it is light.
pub(crate) struct Bin {
    pub(crate) w: usize,
    pub(crate) h: usize,
    dark: Vec<bool>,
}

impl Bin {
    pub(crate) fn dark_at(&self, x: i64, y: i64) -> bool {
        if x < 0 || y < 0 {
            return false;
        }
        let (x, y) = (x as usize, y as usize);
        if x >= self.w || y >= self.h {
            return false;
        }
        self.dark.get(y * self.w + x).copied().unwrap_or(false)
    }

    /// Pixels of one row.
    pub(crate) fn row(&self, y: usize) -> &[bool] {
        self.dark.get(y * self.w..(y + 1) * self.w).unwrap_or(&[])
    }
}

/// Thresholds a frame. `w` and `h` are at least 16 and `luma` holds every
/// row at `stride` (checked by the caller).
pub(crate) fn binarize(
    w: usize,
    h: usize,
    stride: usize,
    luma: &[u8],
    budget: &mut Budget,
) -> Result<Bin, DecodeError> {
    budget.charge(3 * (w as u64) * (h as u64))?;
    let bw = w.div_ceil(BLOCK);
    let bh = h.div_ceil(BLOCK);
    let px = |x: usize, y: usize| -> u32 { luma.get(y * stride + x).copied().map_or(0, u32::from) };
    let origin = |b: usize, len: usize| (b * BLOCK).min(len.saturating_sub(BLOCK));
    // First pass: each block's mean, lowest value and spread.
    let mut stats = vec![(0u32, 0u32, 0u32, 0u32); bw * bh];
    for by in 0..bh {
        let y0 = origin(by, h);
        for bx in 0..bw {
            let x0 = origin(bx, w);
            let mut sum = 0u32;
            let (mut min, mut max) = (255u32, 0u32);
            for dy in 0..BLOCK {
                for dx in 0..BLOCK {
                    let v = px(x0 + dx, y0 + dy);
                    sum += v;
                    min = min.min(v);
                    max = max.max(v);
                }
            }
            let mean = sum / (BLOCK * BLOCK) as u32;
            let (mut hi, mut hi_n, mut lo, mut lo_n) = (0u32, 0u32, 0u32, 0u32);
            for dy in 0..BLOCK {
                for dx in 0..BLOCK {
                    let v = px(x0 + dx, y0 + dy);
                    if v > mean {
                        hi += v;
                        hi_n += 1;
                    } else {
                        lo += v;
                        lo_n += 1;
                    }
                }
            }
            let spread = match (hi.checked_div(hi_n), lo.checked_div(lo_n)) {
                (Some(a), Some(b)) => a - b,
                _ => 0,
            };
            if let Some(slot) = stats.get_mut(by * bw + bx) {
                *slot = (mean, min, max, spread);
            }
        }
    }
    // The usual range of the blocks that have some contrast: a flat light block
    // gets a level that far under its own, so that it does not pull the
    // blocks next to it towards "everything is dark".
    let (ranged, range_sum) = stats
        .iter()
        .filter(|&&(_, _, _, spread)| spread > MIN_SPREAD)
        .fold((0u32, 0u32), |(n, s), &(_, min, max, _)| {
            (n + 1, s + max - min)
        });
    let contrast = range_sum.checked_div(ranged);
    let mut avg = vec![0u32; bw * bh];
    for by in 0..bh {
        for bx in 0..bw {
            let (mean, min, _, spread) = stats.get(by * bw + bx).copied().unwrap_or((0, 0, 0, 0));
            let at = |x: usize, y: usize| avg.get(y * bw + x).copied().unwrap_or(0);
            let a = if spread > MIN_SPREAD {
                mean
            } else {
                let mut a = match contrast {
                    Some(c) => min.saturating_sub(c / 2),
                    None => min / 2,
                };
                if by > 0 && bx > 0 {
                    let n = (at(bx, by - 1) + 2 * at(bx - 1, by) + at(bx - 1, by - 1)) / 4;
                    if min < n {
                        a = n;
                    }
                }
                a
            };
            if let Some(slot) = avg.get_mut(by * bw + bx) {
                *slot = a;
            }
        }
    }
    let mut dark = vec![false; w * h];
    for by in 0..bh {
        let y0 = origin(by, h);
        for bx in 0..bw {
            let x0 = origin(bx, w);
            let mut sum = 0u32;
            let mut count = 0u32;
            for yy in by.saturating_sub(2)..=(by + 2).min(bh - 1) {
                for xx in bx.saturating_sub(2)..=(bx + 2).min(bw - 1) {
                    sum += avg.get(yy * bw + xx).copied().unwrap_or(0);
                    count += 1;
                }
            }
            let threshold = sum / count.max(1);
            for dy in 0..BLOCK {
                for dx in 0..BLOCK {
                    if px(x0 + dx, y0 + dy) <= threshold {
                        if let Some(d) = dark.get_mut((y0 + dy) * w + x0 + dx) {
                            *d = true;
                        }
                    }
                }
            }
        }
    }
    Ok(Bin { w, h, dark })
}
