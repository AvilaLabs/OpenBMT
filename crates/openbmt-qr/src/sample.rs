//! From three finder patterns to a grid of modules: the axes and module
//! size, the version estimate, the alignment pattern, the perspective
//! transform and the sampling.

use crate::binarize::Bin;
use crate::decode::{Budget, DecodeError};
use crate::finder::Finder;
use crate::tables;

/// Corners in the order of the symbol, and the geometry read from them.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Frame {
    pub(crate) tl: (f64, f64),
    pub(crate) tr: (f64, f64),
    pub(crate) bl: (f64, f64),
    /// Module size along the top edge and the left edge, in pixels.
    pub(crate) ms_x: f64,
    pub(crate) ms_y: f64,
    /// Unit vectors along the top edge and the left edge.
    pub(crate) ux: (f64, f64),
    pub(crate) uy: (f64, f64),
    /// Estimated side in modules, not rounded.
    pub(crate) dimension: f64,
}

/// Distance from the centre to the third colour change along a direction,
/// by steps of a quarter pixel. The centre must be dark: the distance is
/// then 3.5 modules.
fn extent(bin: &Bin, c: (f64, f64), d: (f64, f64), max: f64, ops: &mut u64) -> Option<f64> {
    let mut changes = 0;
    let mut dark = true;
    let mut t = 0.25;
    while t <= max {
        let x = (c.0 + d.0 * t).round() as i64;
        let y = (c.1 + d.1 * t).round() as i64;
        *ops += 1;
        let now = bin.dark_at(x, y);
        if now != dark {
            changes += 1;
            dark = now;
            if changes == 3 {
                return Some(t);
            }
        }
        t += 0.25;
    }
    None
}

fn unit(a: (f64, f64), b: (f64, f64)) -> Option<(f64, f64)> {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let len = dx.hypot(dy);
    (len > 0.0).then(|| (dx / len, dy / len))
}

/// Orders the three finders (`[corner, a, b]`), reads the axes and the
/// module size along them.
pub(crate) fn frame_of(
    bin: &Bin,
    t: &[Finder; 3],
    budget: &mut Budget,
) -> Result<Option<Frame>, DecodeError> {
    let [tl, a, b] = *t;
    let cross = (a.x - tl.x) * (b.y - tl.y) - (a.y - tl.y) * (b.x - tl.x);
    let (tr, bl) = if cross > 0.0 { (a, b) } else { (b, a) };
    let (ptl, ptr, pbl) = ((tl.x, tl.y), (tr.x, tr.y), (bl.x, bl.y));
    let (Some(ux), Some(uy)) = (unit(ptl, ptr), unit(ptl, pbl)) else {
        return Ok(None);
    };
    let mut ops = 0u64;
    let mut along = |f: &Finder, d: (f64, f64)| -> f64 {
        let c = (f.x, f.y);
        let reach = f.size * 7.0;
        let plus = extent(bin, c, d, reach, &mut ops);
        let minus = extent(bin, c, (-d.0, -d.1), reach, &mut ops);
        match (plus, minus) {
            (Some(p), Some(m)) => (p + m) / 7.0,
            _ => f.size,
        }
    };
    let ms_x = (along(&tl, ux) + along(&tr, ux)) / 2.0;
    let ms_y = (along(&tl, uy) + along(&bl, uy)) / 2.0;
    budget.charge(ops)?;
    if ms_x <= 0.0 || ms_y <= 0.0 {
        return Ok(None);
    }
    let dx = (ptr.0 - ptl.0).hypot(ptr.1 - ptl.1) / ms_x;
    let dy = (pbl.0 - ptl.0).hypot(pbl.1 - ptl.1) / ms_y;
    Ok(Some(Frame {
        tl: ptl,
        tr: ptr,
        bl: pbl,
        ms_x,
        ms_y,
        ux,
        uy,
        dimension: (dx + dy) / 2.0 + 7.0,
    }))
}

/// Versions to try, the likeliest first.
pub(crate) fn version_candidates(dimension: f64) -> Vec<u8> {
    let exact = (dimension - 17.0) / 4.0;
    let near = exact.round().clamp(1.0, 40.0);
    let (first, second) = if exact > near {
        (near + 1.0, near - 1.0)
    } else {
        (near - 1.0, near + 1.0)
    };
    let mut out = vec![near as u8];
    for v in [first, second] {
        if (1.0..=40.0).contains(&v) {
            out.push(v as u8);
        }
    }
    out
}

/// A homography from module coordinates to pixels, fitted to point pairs
/// in coordinates centred and scaled for the conditioning of the fit.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Homography {
    h: [f64; 8],
    /// Centre and scale of the module side, and of the pixel side.
    src: (f64, f64, f64),
    dst: (f64, f64, f64),
}

type Pair = ((f64, f64), (f64, f64));

impl Homography {
    /// The transform taking four points in module coordinates to four in
    /// the image, or `None` if they are degenerate.
    #[cfg(test)]
    pub(crate) fn from_points(src: [(f64, f64); 4], dst: [(f64, f64); 4]) -> Option<Self> {
        let pairs: Vec<Pair> = src.iter().copied().zip(dst.iter().copied()).collect();
        Self::fit(&pairs)
    }

    /// The least-squares transform through at least four pairs of (module,
    /// pixel) points.
    pub(crate) fn fit(pairs: &[Pair]) -> Option<Self> {
        if pairs.len() < 4 {
            return None;
        }
        let n = pairs.len() as f64;
        let centre = |f: &dyn Fn(&Pair) -> (f64, f64)| {
            let (sx, sy) = pairs
                .iter()
                .map(f)
                .fold((0.0, 0.0), |a, p| (a.0 + p.0, a.1 + p.1));
            let (cx, cy) = (sx / n, sy / n);
            let spread = pairs
                .iter()
                .map(f)
                .map(|p| (p.0 - cx).hypot(p.1 - cy))
                .sum::<f64>()
                / n;
            (cx, cy, spread)
        };
        let sc = centre(&|p| p.0);
        let dc = centre(&|p| p.1);
        if sc.2 < 1e-9 || dc.2 < 1e-9 {
            return None;
        }
        // Normal equations of the 2n by 8 system, built row by row.
        let mut m = [[0f64; 9]; 8];
        for &((x, y), (u, v)) in pairs {
            let (x, y) = ((x - sc.0) / sc.2, (y - sc.1) / sc.2);
            let (u, v) = ((u - dc.0) / dc.2, (v - dc.1) / dc.2);
            for row in [
                [x, y, 1.0, 0.0, 0.0, 0.0, -u * x, -u * y, u],
                [0.0, 0.0, 0.0, x, y, 1.0, -v * x, -v * y, v],
            ] {
                for i in 0..8 {
                    let ri = row.get(i).copied().unwrap_or(0.0);
                    let target = m.get_mut(i)?;
                    for (cell, &rj) in target.iter_mut().zip(row.iter()) {
                        *cell += ri * rj;
                    }
                }
            }
        }
        for col in 0..8 {
            let at = |r: usize, m: &[[f64; 9]; 8]| {
                m.get(r)
                    .and_then(|row| row.get(col))
                    .copied()
                    .unwrap_or(0.0)
                    .abs()
            };
            let pivot = (col..8).max_by(|&a, &b| at(a, &m).total_cmp(&at(b, &m)))?;
            if at(pivot, &m) < 1e-12 {
                return None;
            }
            m.swap(col, pivot);
            let prow = *m.get(col)?;
            let p = *prow.get(col)?;
            for row in 0..8 {
                if row == col {
                    continue;
                }
                let r = m.get_mut(row)?;
                let f = *r.get(col)? / p;
                for (cell, &pv) in r.iter_mut().zip(prow.iter()) {
                    *cell -= f * pv;
                }
            }
        }
        let mut h = [0f64; 8];
        for (i, slot) in h.iter_mut().enumerate() {
            let row = m.get(i)?;
            *slot = *row.get(8)? / *row.get(i)?;
        }
        h.iter().all(|v| v.is_finite()).then_some(Homography {
            h,
            src: sc,
            dst: dc,
        })
    }

    pub(crate) fn apply(&self, x: f64, y: f64) -> (f64, f64) {
        let g = |i: usize| self.h.get(i).copied().unwrap_or(0.0);
        let (x, y) = ((x - self.src.0) / self.src.2, (y - self.src.1) / self.src.2);
        let w = g(6) * x + g(7) * y + 1.0;
        let (u, v) = (
            (g(0) * x + g(1) * y + g(2)) / w,
            (g(3) * x + g(4) * y + g(5)) / w,
        );
        (u * self.dst.2 + self.dst.0, v * self.dst.2 + self.dst.1)
    }
}

/// Looks in a window around `guess` (pixels) for the alignment pattern:
/// the best match of the 5 by 5 template, centred on all the positions
/// that match best. `mx` and `my` are the module vectors in pixels.
fn locate_pattern(
    bin: &Bin,
    guess: (f64, f64),
    mx: (f64, f64),
    my: (f64, f64),
    radius: f64,
    min_score: i32,
    budget: &mut Budget,
) -> Result<Option<(f64, f64)>, DecodeError> {
    let ms = mx.0.hypot(mx.1).min(my.0.hypot(my.1));
    if !(ms > 0.5 && ms.is_finite()) {
        return Ok(None);
    }
    let step = (1.0 / ms).max(0.2);
    let n = (radius / step) as i64;
    budget.charge(((2 * n + 1) * (2 * n + 1)) as u64 * 25)?;
    let point = |a: f64, b: f64| (guess.0 + mx.0 * a + my.0 * b, guess.1 + mx.1 * a + my.1 * b);
    let mut best = 0;
    let mut sum = (0.0, 0.0, 0.0);
    for jy in -n..=n {
        for jx in -n..=n {
            let (a, b) = (jx as f64 * step, jy as f64 * step);
            let mut score = 0;
            for j in -2..=2i64 {
                for i in -2..=2i64 {
                    let (x, y) = point(a + i as f64, b + j as f64);
                    let want = i.abs().max(j.abs()) != 1;
                    if bin.dark_at(x.round() as i64, y.round() as i64) == want {
                        score += 1;
                    }
                }
            }
            if score > best {
                best = score;
                sum = (0.0, 0.0, 0.0);
            }
            if score == best {
                sum = (sum.0 + a, sum.1 + b, sum.2 + 1.0);
            }
        }
    }
    if best < min_score || sum.2 == 0.0 {
        return Ok(None);
    }
    Ok(Some(point(sum.0 / sum.2, sum.1 / sum.2)))
}

/// The module grid of a symbol of `version`, row by row, sampled through
/// the transform of the three finders and the alignment pattern near the
/// corner, and with `refine` every alignment pattern found.
pub(crate) fn sample_grid(
    bin: &Bin,
    f: &Frame,
    version: u8,
    refine: bool,
    budget: &mut Budget,
) -> Result<Option<Vec<bool>>, DecodeError> {
    let dim = tables::side(version);
    budget.charge((dim * dim) as u64 * 5)?;
    let d = dim as f64;
    let finders: [Pair; 3] = [
        ((3.5, 3.5), f.tl),
        ((d - 3.5, 3.5), f.tr),
        ((3.5, d - 3.5), f.bl),
    ];
    let mut pairs: Vec<Pair> = finders.to_vec();
    // The pattern nearest the bottom right corner first, from where three
    // finders put it.
    if version >= 2 {
        let k = (d - 10.0) / (d - 7.0);
        let guess = (
            f.tl.0 + (f.tr.0 - f.tl.0 + f.bl.0 - f.tl.0) * k,
            f.tl.1 + (f.tr.1 - f.tl.1 + f.bl.1 - f.tl.1) * k,
        );
        let mx = (f.ux.0 * f.ms_x, f.ux.1 * f.ms_x);
        let my = (f.uy.0 * f.ms_y, f.uy.1 * f.ms_y);
        if let Some(p) = locate_pattern(bin, guess, mx, my, 5.0, 22, budget)? {
            pairs.push(((d - 6.5, d - 6.5), p));
        }
    }
    if pairs.len() == 3 {
        pairs.push((
            (d - 3.5, d - 3.5),
            (f.tr.0 + f.bl.0 - f.tl.0, f.tr.1 + f.bl.1 - f.tl.1),
        ));
    }
    let Some(mut h) = Homography::fit(&pairs) else {
        return Ok(None);
    };
    // With more than one alignment pattern, find each where the transform
    // puts it and fit again to all of them.
    let centres = tables::alignment_positions(version);
    if refine && centres.len() > 2 {
        for _ in 0..2 {
            let mut refined: Vec<Pair> = finders.to_vec();
            let last = centres.len() - 1;
            for (i, &a) in centres.iter().enumerate() {
                for (j, &b) in centres.iter().enumerate() {
                    if (i == 0 && j == 0) || (i == 0 && j == last) || (i == last && j == 0) {
                        continue;
                    }
                    let (ma, mb) = (a as f64 + 0.5, b as f64 + 0.5);
                    let p0 = h.apply(ma, mb);
                    let px = h.apply(ma + 1.0, mb);
                    let py = h.apply(ma, mb + 1.0);
                    let (mx, my) = ((px.0 - p0.0, px.1 - p0.1), (py.0 - p0.0, py.1 - p0.1));
                    if let Some(p) = locate_pattern(bin, p0, mx, my, 2.0, 22, budget)? {
                        refined.push(((ma, mb), p));
                    }
                }
            }
            if refined.len() < 4 {
                break;
            }
            // Drop the alignment pattern that sits worst against the fit
            // while it is more than a module off: a wrong match must not
            // bend the transform.
            let mut fit = Homography::fit(&refined);
            while let Some(g) = fit {
                let off = |p: &Pair| {
                    let q = g.apply((p.0).0, (p.0).1);
                    (q.0 - (p.1).0).hypot(q.1 - (p.1).1)
                };
                let worst = (3..refined.len()).max_by(|&a, &b| {
                    let oa = refined.get(a).map_or(0.0, off);
                    let ob = refined.get(b).map_or(0.0, off);
                    oa.total_cmp(&ob)
                });
                match worst {
                    Some(w) if refined.get(w).map_or(0.0, off) > f.ms_x.min(f.ms_y) => {
                        refined.remove(w);
                        fit = Homography::fit(&refined);
                    }
                    _ => break,
                }
            }
            match fit {
                Some(better) => h = better,
                None => break,
            }
        }
    }
    let mut grid = vec![false; dim * dim];
    for y in 0..dim {
        for x in 0..dim {
            let (cx, cy) = (x as f64 + 0.5, y as f64 + 0.5);
            let mut votes = 0;
            for (ox, oy) in [
                (0.0, 0.0),
                (0.25, 0.0),
                (-0.25, 0.0),
                (0.0, 0.25),
                (0.0, -0.25),
            ] {
                let (px, py) = h.apply(cx + ox, cy + oy);
                if !(px.is_finite() && py.is_finite()) {
                    return Ok(None);
                }
                if bin.dark_at(px.round() as i64, py.round() as i64) {
                    votes += 1;
                }
            }
            if let Some(m) = grid.get_mut(y * dim + x) {
                *m = votes >= 3;
            }
        }
    }
    Ok(Some(grid))
}
