//! Finder patterns: runs in the ratio 1:1:3:1:1 along rows, confirmed down
//! the column and along a diagonal, then the choice of three that can be
//! the corners of one symbol.

use crate::binarize::Bin;
use crate::decode::{Budget, DecodeError};

/// Most candidates kept. Frames with more are not codes.
const MAX_CANDIDATES: usize = 256;
/// Most candidates considered when choosing three.
const MAX_CHOSEN: usize = 10;

/// A finder pattern's centre in pixel-index coordinates.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Finder {
    pub(crate) x: f64,
    pub(crate) y: f64,
    /// Module size read along the row and column, in pixels.
    pub(crate) size: f64,
    pub(crate) count: u32,
}

/// Whether five run lengths are 1:1:3:1:1, to a tolerance of half a module
/// and `slack` pixels.
fn ratio_ok(c: &[f64; 5], slack: f64) -> bool {
    let total: f64 = c.iter().sum();
    if total < 7.0 {
        return false;
    }
    let module = total / 7.0;
    let var = module * 0.5 + slack;
    let near = |v: f64, want: f64| (want * module - v).abs() <= want * var;
    near(c[0], 1.0) && near(c[1], 1.0) && near(c[2], 3.0) && near(c[3], 1.0) && near(c[4], 1.0)
}

/// The runs along a line through a dark pixel: the light and dark rings on
/// each side of the centre run, as `[outer, light, centre, light, outer]`,
/// and the offset of the middle of the whole extent from the start, in
/// steps. The start must be dark. A run longer than `limit` fails.
fn line_runs(
    bin: &Bin,
    x0: i64,
    y0: i64,
    dx: i64,
    dy: i64,
    limit: usize,
    ops: &mut u64,
) -> Option<([f64; 5], f64)> {
    if !bin.dark_at(x0, y0) {
        return None;
    }
    let mut side = |sign: i64| -> Option<[usize; 3]> {
        let mut out = [0usize; 3];
        let mut idx = 0usize;
        let mut i = 1i64;
        loop {
            let color = idx.is_multiple_of(2);
            let c = bin.dark_at(x0 + sign * dx * i, y0 + sign * dy * i);
            *ops += 1;
            if c == color {
                let slot = out.get_mut(idx)?;
                *slot += 1;
                if *slot > limit {
                    return None;
                }
            } else {
                idx += 1;
                if idx == 3 {
                    return Some(out);
                }
                // The pixel that changed the colour starts the next run.
                *out.get_mut(idx)? += 1;
            }
            i += 1;
        }
    };
    let f = side(1)?;
    let b = side(-1)?;
    let counts = [
        b[2] as f64,
        b[1] as f64,
        (b[0] + f[0] + 1) as f64,
        f[1] as f64,
        f[2] as f64,
    ];
    let fwd = (f[0] + f[1] + f[2]) as f64;
    let bwd = (b[0] + b[1] + b[2]) as f64;
    Some((counts, (fwd - bwd) / 2.0))
}

fn near_candidate(list: &[Finder], x: f64, y: f64, size: f64) -> Option<usize> {
    list.iter()
        .position(|c| (c.x - x).abs() <= size * 1.5 && (c.y - y).abs() <= size * 1.5)
}

/// All finder-pattern candidates in the frame.
pub(crate) fn find_candidates(bin: &Bin, budget: &mut Budget) -> Result<Vec<Finder>, DecodeError> {
    let mut list: Vec<Finder> = Vec::new();
    let mut runs: Vec<(usize, usize)> = Vec::new();
    for y in 0..bin.h {
        budget.charge(bin.w as u64)?;
        let row = bin.row(y);
        runs.clear();
        let mut start = 0usize;
        for i in 1..=row.len() {
            if i == row.len() || row.get(i) != row.get(i - 1) {
                runs.push((start, i - start));
                start = i;
            }
        }
        let first_dark = row.first().copied().unwrap_or(false);
        // Index of the first dark run.
        let mut i = usize::from(!first_dark);
        while i + 4 < runs.len() {
            let window = runs.get(i..i + 5).unwrap_or(&[]);
            let mut c = [0f64; 5];
            for (slot, r) in c.iter_mut().zip(window) {
                *slot = r.1 as f64;
            }
            i += 2;
            if !ratio_ok(&c, 0.5) {
                continue;
            }
            let (s, l) = window.get(2).copied().unwrap_or((0, 0));
            let cx = s as f64 + (l as f64 - 1.0) / 2.0;
            let size = c.iter().sum::<f64>() / 7.0;
            if let Some(k) = near_candidate(&list, cx, y as f64, size) {
                if let Some(f) = list.get_mut(k) {
                    f.count += 1;
                }
                continue;
            }
            if list.len() >= MAX_CANDIDATES {
                continue;
            }
            if let Some(f) = confirm(bin, cx, y as f64, size, budget)? {
                match near_candidate(&list, f.x, f.y, f.size) {
                    Some(k) => {
                        if let Some(old) = list.get_mut(k) {
                            let n = f64::from(old.count);
                            old.x = (old.x * n + f.x) / (n + 1.0);
                            old.y = (old.y * n + f.y) / (n + 1.0);
                            old.size = (old.size * n + f.size) / (n + 1.0);
                            old.count += 1;
                        }
                    }
                    None => list.push(f),
                }
            }
        }
    }
    Ok(list)
}

/// Checks a row hit down its column, along its row again, and along a
/// diagonal; returns the refined centre.
fn confirm(
    bin: &Bin,
    cx: f64,
    cy: f64,
    size: f64,
    budget: &mut Budget,
) -> Result<Option<Finder>, DecodeError> {
    let limit = (size * 4.0) as usize + 2;
    let mut ops = 0u64;
    let result = (|| {
        let (ix, iy) = (cx.round() as i64, cy.round() as i64);
        let (v, shift) = line_runs(bin, ix, iy, 0, 1, limit, &mut ops)?;
        if !ratio_ok(&v, 0.5) {
            return None;
        }
        let y = iy as f64 + shift;
        let (h, shift) = line_runs(bin, ix, y.round() as i64, 1, 0, limit, &mut ops)?;
        if !ratio_ok(&h, 0.5) {
            return None;
        }
        let x = ix as f64 + shift;
        let (px, py) = (x.round() as i64, y.round() as i64);
        let diag = |dy: i64, ops: &mut u64| {
            line_runs(bin, px, py, 1, dy, limit, ops).is_some_and(|(d, _)| ratio_ok(&d, 1.0))
        };
        if !(diag(1, &mut ops) || diag(-1, &mut ops)) {
            return None;
        }
        let size = (v.iter().sum::<f64>() + h.iter().sum::<f64>()) / 14.0;
        Some(Finder {
            x,
            y,
            size,
            count: 1,
        })
    })();
    budget.charge(ops)?;
    Ok(result)
}

/// The chosen triples of candidates, best first. Each is ordered
/// `[corner, a, b]` with the corner the one at the right angle. Not
/// oriented: which of `a` and `b` is to the right is decided with the
/// cross product by the caller.
pub(crate) fn triples(list: &[Finder]) -> Vec<[Finder; 3]> {
    let mut sorted: Vec<Finder> = list.to_vec();
    sorted.sort_by_key(|a| std::cmp::Reverse(a.count));
    let confirmed = sorted.iter().filter(|f| f.count >= 2).count();
    sorted.truncate(if confirmed >= 3 {
        confirmed.min(MAX_CHOSEN)
    } else {
        MAX_CHOSEN
    });
    let mut out: Vec<(f64, [Finder; 3])> = Vec::new();
    let n = sorted.len();
    for i in 0..n {
        for j in i + 1..n {
            for k in j + 1..n {
                let (Some(&p), Some(&q), Some(&r)) = (sorted.get(i), sorted.get(j), sorted.get(k))
                else {
                    continue;
                };
                if let Some(t) = score_triple(p, q, r) {
                    out.push(t);
                }
            }
        }
    }
    out.sort_by(|a, b| a.0.total_cmp(&b.0));
    out.into_iter().map(|(_, t)| t).collect()
}

fn dist(a: &Finder, b: &Finder) -> f64 {
    ((a.x - b.x).powi(2) + (a.y - b.y).powi(2)).sqrt()
}

/// A score (lower is better) for three finders as the corners of a code,
/// or `None` when they cannot be.
fn score_triple(p: Finder, q: Finder, r: Finder) -> Option<(f64, [Finder; 3])> {
    // The corner is opposite the longest side.
    let (dpq, dqr, dpr) = (dist(&p, &q), dist(&q, &r), dist(&p, &r));
    let (corner, a, b) = if dqr >= dpq && dqr >= dpr {
        (p, q, r)
    } else if dpr >= dpq {
        (q, p, r)
    } else {
        (r, p, q)
    };
    let (la, lb) = (dist(&corner, &a), dist(&corner, &b));
    let sizes = [p.size, q.size, r.size];
    let smallest = sizes.iter().copied().fold(f64::MAX, f64::min);
    let largest = sizes.iter().copied().fold(0.0, f64::max);
    if smallest <= 0.0 || largest > 2.0 * smallest {
        return None;
    }
    // Centres of two finders are at least 14 modules apart.
    if la.min(lb) < 8.0 * largest {
        return None;
    }
    let cos =
        ((a.x - corner.x) * (b.x - corner.x) + (a.y - corner.y) * (b.y - corner.y)) / (la * lb);
    if cos.abs() > 0.45 {
        return None;
    }
    let legs = (la - lb).abs() / la.max(lb);
    if legs > 0.4 {
        return None;
    }
    let spread = largest / smallest - 1.0;
    let weight = 1.0 / f64::from(p.count.min(q.count).min(r.count).max(1));
    Some((
        legs + cos.abs() + spread * 0.5 + weight * 0.1,
        [corner, a, b],
    ))
}
