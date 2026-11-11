//! Tests of the reader: round trips through rendered frames, transformed
//! frames with their success rates (run with `--nocapture` to see them),
//! corrupted codes, refused segments and bounded input.

use crate::matrix::{draw, function_modules, with_error_correction};
use crate::sample::Homography;
use crate::{decode_luma, decode_luma_strided, encode_with, rs, tables, DecodeError, Ecc, QrCode};

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        // xorshift64*
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() >> 11) as usize % n.max(1)
    }

    fn unit(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }
}

/// A frame as floating-point gray levels.
#[derive(Clone)]
struct Img {
    w: usize,
    h: usize,
    px: Vec<f32>,
}

impl Img {
    fn new(w: usize, h: usize, v: f32) -> Img {
        Img {
            w,
            h,
            px: vec![v; w * h],
        }
    }

    fn at(&self, x: usize, y: usize) -> f32 {
        self.px[y * self.w + x]
    }

    /// Bilinear sample; light outside the frame.
    fn sample(&self, x: f64, y: f64) -> f32 {
        if x < 0.0 || y < 0.0 || x > (self.w - 1) as f64 || y > (self.h - 1) as f64 {
            return 255.0;
        }
        let (x0, y0) = (x.floor() as usize, y.floor() as usize);
        let (x1, y1) = ((x0 + 1).min(self.w - 1), (y0 + 1).min(self.h - 1));
        let (fx, fy) = ((x - x0 as f64) as f32, (y - y0 as f64) as f32);
        let top = self.at(x0, y0) * (1.0 - fx) + self.at(x1, y0) * fx;
        let bottom = self.at(x0, y1) * (1.0 - fx) + self.at(x1, y1) * fx;
        top * (1.0 - fy) + bottom * fy
    }

    fn luma(&self) -> Vec<u8> {
        self.px
            .iter()
            .map(|&v| v.clamp(0.0, 255.0).round() as u8)
            .collect()
    }

    fn decode(&self) -> Result<Vec<u8>, DecodeError> {
        decode_luma(self.w, self.h, &self.luma()).map(|d| d.bytes)
    }
}

/// Draws modules at `scale` pixels with a quiet zone of four.
fn render(code: &QrCode, scale: usize) -> Img {
    render_modules(code.size(), scale, |x, y| code.get(x, y))
}

fn render_modules(n: usize, scale: usize, dark: impl Fn(usize, usize) -> bool) -> Img {
    let w = (n + 8) * scale;
    let mut img = Img::new(w, w, 255.0);
    for y in 0..n {
        for x in 0..n {
            if dark(x, y) {
                for dy in 0..scale {
                    for dx in 0..scale {
                        img.px[((y + 4) * scale + dy) * w + (x + 4) * scale + dx] = 0.0;
                    }
                }
            }
        }
    }
    img
}

fn rotate(img: &Img, degrees: f64) -> Img {
    let diag = ((img.w * img.w + img.h * img.h) as f64).sqrt().ceil() as usize + 4;
    let mut out = Img::new(diag, diag, 255.0);
    let (s, c) = degrees.to_radians().sin_cos();
    let (ox, oy) = (diag as f64 / 2.0, diag as f64 / 2.0);
    let (sx, sy) = (img.w as f64 / 2.0, img.h as f64 / 2.0);
    for y in 0..diag {
        for x in 0..diag {
            let (rx, ry) = (x as f64 - ox, y as f64 - oy);
            let u = c * rx + s * ry + sx;
            let v = -s * rx + c * ry + sy;
            out.px[y * diag + x] = img.sample(u, v);
        }
    }
    out
}

/// A mild perspective: the output frame looks at a quadrilateral of the
/// source that is wider at the bottom and a little turned.
fn perspective(img: &Img, strength: f64) -> Img {
    let (w, h) = (img.w as f64, img.h as f64);
    let k = strength;
    let out = Img::new(img.w, img.h, 255.0);
    let src = [
        (-0.03 * w * k, -0.02 * h * k),
        (w * (1.0 + 0.03 * k), 0.01 * h * k),
        (w * (1.0 - 0.02 * k), h * (1.0 + 0.04 * k)),
        (-0.01 * w * k, h * (1.0 + 0.01 * k)),
    ];
    let dst = [(0.0, 0.0), (w, 0.0), (w, h), (0.0, h)];
    let homography = Homography::from_points(dst, src).unwrap();
    let mut out = out;
    for y in 0..img.h {
        for x in 0..img.w {
            let (u, v) = homography.apply(x as f64, y as f64);
            out.px[y * img.w + x] = img.sample(u, v);
        }
    }
    out
}

fn box_blur(img: &Img) -> Img {
    let mut out = img.clone();
    for y in 0..img.h {
        for x in 0..img.w {
            let mut sum = 0.0;
            for dy in -1i64..=1 {
                for dx in -1i64..=1 {
                    let xx = (x as i64 + dx).clamp(0, img.w as i64 - 1) as usize;
                    let yy = (y as i64 + dy).clamp(0, img.h as i64 - 1) as usize;
                    sum += img.at(xx, yy);
                }
            }
            out.px[y * img.w + x] = sum / 9.0;
        }
    }
    out
}

fn noise(img: &Img, rng: &mut Rng, amplitude: f64) -> Img {
    let mut out = img.clone();
    for p in &mut out.px {
        // Four uniforms: roughly Gaussian, standard deviation of the
        // amplitude over 3.
        let g: f64 = (0..4).map(|_| rng.unit() - 0.5).sum::<f64>() * amplitude / 2.0;
        *p += g as f32;
    }
    out
}

fn contrast(img: &Img, factor: f32) -> Img {
    let mut out = img.clone();
    for p in &mut out.px {
        *p = 128.0 + (*p - 128.0) * factor;
    }
    out
}

/// Darker on one side, with the dark modules lifted too.
fn gradient(img: &Img) -> Img {
    let mut out = img.clone();
    for y in 0..img.h {
        for x in 0..img.w {
            let t = (x + y) as f32 / (img.w + img.h) as f32;
            let p = &mut out.px[y * img.w + x];
            *p = (30.0 + *p * 0.7) * (0.45 + 0.55 * (1.0 - t));
        }
    }
    out
}

fn flip_h(img: &Img) -> Img {
    let mut out = img.clone();
    for y in 0..img.h {
        for x in 0..img.w {
            out.px[y * img.w + x] = img.at(img.w - 1 - x, y);
        }
    }
    out
}

fn transpose_img(img: &Img) -> Img {
    let mut out = Img::new(img.h, img.w, 0.0);
    for y in 0..img.h {
        for x in 0..img.w {
            out.px[x * img.h + y] = img.at(x, y);
        }
    }
    out
}

fn byte_capacity(version: u8, ecc: Ecc) -> usize {
    let count_bits = if version <= 9 { 8 } else { 16 };
    (tables::data_codewords(version, ecc) * 8 - 4 - count_bits) / 8
}

/// A code of the version, level and mask filled with random bytes.
fn random_code(rng: &mut Rng, version: u8, ecc: Ecc, mask: u8) -> (QrCode, Vec<u8>) {
    let data: Vec<u8> = (0..byte_capacity(version, ecc))
        .map(|_| rng.below(256) as u8)
        .collect();
    let code = encode_with(&data, ecc, Some(version), Some(mask)).unwrap();
    (code, data)
}

fn level(i: usize) -> Ecc {
    Ecc::ALL[i % 4]
}

#[test]
fn qr_10_clean_round_trip_every_version_and_level() {
    let mut rng = Rng(0x1234_5678_9ABC_DEF1);
    let mut total = 0;
    for version in 1..=40u8 {
        for (e, ecc) in Ecc::ALL.into_iter().enumerate() {
            // Every mask at the smaller versions, a rotation of them above.
            let masks: Vec<u8> = if version <= 3 || version % 13 == 0 {
                (0..8).collect()
            } else {
                vec![
                    ((usize::from(version) + e) % 8) as u8,
                    ((usize::from(version) * 3 + e) % 8) as u8,
                ]
            };
            for mask in masks {
                let (code, data) = random_code(&mut rng, version, ecc, mask);
                let scale = 2 + (usize::from(version) + usize::from(mask) + e) % 5;
                let img = render(&code, scale);
                let got = decode_luma(img.w, img.h, &img.luma());
                let d = got.unwrap_or_else(|e| panic!("v{version} {ecc:?} m{mask} x{scale}: {e}"));
                assert_eq!(d.bytes, data, "v{version} {ecc:?} m{mask} x{scale}");
                assert_eq!(
                    (d.version, d.ecc, d.mask, d.corrected_errors),
                    (version, ecc, mask, 0)
                );
                total += 1;
            }
        }
    }
    println!("clean round trips: {total}/{total}");
}

#[test]
fn qr_10_every_module_size_at_every_version_one_to_forty_is_read() {
    let mut rng = Rng(77);
    let mut count = 0;
    for scale in 2..=6 {
        for version in [1u8, 2, 6, 7, 14, 21, 28, 35, 40] {
            let ecc = level(usize::from(version) + scale);
            let (code, data) = random_code(&mut rng, version, ecc, version % 8);
            let img = render(&code, scale);
            assert_eq!(img.decode().unwrap(), data, "v{version} x{scale}");
            count += 1;
        }
    }
    println!("module sizes 2 to 6: {count}/{count}");
}

/// The cases used for the transformations: a spread of versions, levels,
/// masks and module sizes.
fn cases(rng: &mut Rng, n: usize, min_scale: usize) -> Vec<(QrCode, Vec<u8>, usize)> {
    let versions = [1u8, 2, 3, 4, 6, 7, 9, 10, 13, 15, 18, 22, 27, 33, 40];
    (0..n)
        .map(|i| {
            let v = versions[i % versions.len()];
            let (code, data) = random_code(rng, v, level(i / 2 + usize::from(v)), (i % 8) as u8);
            let scale = min_scale + (i * 7 / 3) % (7 - min_scale);
            (code, data, scale)
        })
        .collect()
}

fn rate(name: &str, ok: usize, total: usize, at_least: f64) {
    let r = ok as f64 / total as f64;
    println!("{name}: {ok}/{total} ({:.1}%)", r * 100.0);
    assert!(r >= at_least, "{name}: {ok}/{total} is under {at_least}");
}

#[test]
fn qr_10_rates_rotation() {
    let mut rng = Rng(5);
    let all = cases(&mut rng, 30, 3);
    for angle in [90.0, 180.0, 270.0] {
        let ok = all
            .iter()
            .filter(|(code, data, scale)| {
                rotate(&render(code, *scale), angle).decode().as_deref() == Ok(&data[..])
            })
            .count();
        rate(&format!("rotation {angle}"), ok, all.len(), 1.0);
    }
    let small = &all[..15];
    for angle in [
        -37.0, -24.5, -11.0, -3.0, 4.0, 9.5, 17.0, 29.0, 45.0, 61.0, 133.0,
    ] {
        let ok = small
            .iter()
            .filter(|(code, data, scale)| {
                rotate(&render(code, *scale), angle).decode().as_deref() == Ok(&data[..])
            })
            .count();
        let floor = if angle.abs() <= 30.0 { 0.85 } else { 0.5 };
        rate(&format!("rotation {angle}"), ok, small.len(), floor);
    }
}

#[test]
fn qr_10_rates_blur_noise_contrast_light_perspective() {
    let mut rng = Rng(6);
    let all = cases(&mut rng, 30, 3);
    let run = |name: &str, floor: f64, f: &mut dyn FnMut(&Img) -> Img| {
        let ok = all
            .iter()
            .filter(|(code, data, scale)| {
                f(&render(code, *scale)).decode().as_deref() == Ok(&data[..])
            })
            .count();
        rate(name, ok, all.len(), floor);
    };
    run("blur (box 3x3 twice)", 0.9, &mut |i| box_blur(&box_blur(i)));
    let mut r = Rng(11);
    run("noise (sd 22 on 0..255)", 0.9, &mut |i| {
        noise(i, &mut r, 75.0)
    });
    run("contrast x0.3", 1.0, &mut |i| contrast(i, 0.3));
    run("contrast x0.2", 1.0, &mut |i| contrast(i, 0.2));
    run("contrast x0.15 (no floor)", 0.0, &mut |i| contrast(i, 0.15));
    run("uneven light", 0.9, &mut |i| gradient(i));
    run("perspective 1", 0.9, &mut |i| perspective(i, 1.0));
    run("perspective 2", 0.8, &mut |i| perspective(i, 2.0));
    run("perspective 3", 0.0, &mut |i| perspective(i, 3.0));
    let mut r = Rng(13);
    run("noise (sd 40)", 0.0, &mut |i| noise(i, &mut r, 140.0));
    let mut r = Rng(12);
    run(
        "rotation 8 + blur + uneven light + noise sd 6",
        0.9,
        &mut |i| noise(&gradient(&box_blur(&rotate(i, 8.0))), &mut r, 20.0),
    );
    let mut r = Rng(14);
    run(
        "rotation 8 + blur twice + uneven light + noise sd 12",
        0.0,
        &mut |i| {
            noise(
                &gradient(&box_blur(&box_blur(&rotate(i, 8.0)))),
                &mut r,
                40.0,
            )
        },
    );
}

#[test]
fn qr_10_mirrored_and_transposed_codes_read() {
    let mut rng = Rng(8);
    let all = cases(&mut rng, 20, 3);
    let (mut a, mut b, mut c) = (0, 0, 0);
    for (code, data, scale) in &all {
        let img = render(code, *scale);
        a += usize::from(flip_h(&img).decode().as_deref() == Ok(&data[..]));
        b += usize::from(transpose_img(&img).decode().as_deref() == Ok(&data[..]));
        c += usize::from(rotate(&flip_h(&img), 33.0).decode().as_deref() == Ok(&data[..]));
    }
    rate("mirrored left to right", a, all.len(), 1.0);
    rate("transposed", b, all.len(), 1.0);
    rate("mirrored and rotated 33", c, all.len(), 0.85);
}

/// Placement-order bit positions of the codewords: for each codeword, the
/// eight module positions, and the block it belongs to.
fn codeword_modules(version: u8, ecc: Ecc) -> Vec<([(usize, usize); 8], usize)> {
    let n = tables::side(version);
    let func = function_modules(version);
    let mut spots: Vec<(usize, usize)> = Vec::new();
    let mut right = n as i64 - 1;
    while right >= 1 {
        if right == 6 {
            right = 5;
        }
        for vert in 0..n as i64 {
            for j in 0..2 {
                let x = (right - j) as usize;
                let upward = ((right + 1) & 2) == 0;
                let y = (if upward { n as i64 - 1 - vert } else { vert }) as usize;
                if !func[y * n + x] {
                    spots.push((x, y));
                }
            }
        }
        right -= 2;
    }
    let n_blocks = tables::blocks(version, ecc);
    let ecc_len = tables::ecc_per_block(version, ecc);
    let raw = tables::total_codewords(version);
    let n_short = n_blocks - raw % n_blocks;
    let short_len = raw / n_blocks;
    let mut owner = Vec::new();
    for i in 0..short_len - ecc_len + 1 {
        for b in 0..n_blocks {
            if i < short_len - ecc_len + usize::from(b >= n_short) {
                owner.push(b);
            }
        }
    }
    for _ in 0..ecc_len {
        owner.extend(0..n_blocks);
    }
    owner
        .into_iter()
        .enumerate()
        .map(|(k, b)| {
            let mut eight = [(0, 0); 8];
            eight.copy_from_slice(&spots[8 * k..8 * k + 8]);
            (eight, b)
        })
        .collect()
}

/// Flips one module in each of `per_block(b)` codewords of every block.
fn corrupt(
    code: &QrCode,
    rng: &mut Rng,
    per_block: &dyn Fn(usize, usize) -> usize,
) -> (Vec<bool>, usize) {
    let n = code.size();
    let mut grid: Vec<bool> = (0..n * n).map(|i| code.get(i % n, i / n)).collect();
    let words = codeword_modules(code.version(), code.ecc());
    let n_blocks = tables::blocks(code.version(), code.ecc());
    let ecc_len = tables::ecc_per_block(code.version(), code.ecc());
    let mut flipped = 0;
    for b in 0..n_blocks {
        let mut mine: Vec<usize> = (0..words.len()).filter(|&k| words[k].1 == b).collect();
        for _ in 0..per_block(b, ecc_len).min(mine.len()) {
            let k = mine.swap_remove(rng.below(mine.len()));
            let (x, y) = words[k].0[rng.below(8)];
            grid[y * n + x] = !grid[y * n + x];
            flipped += 1;
        }
    }
    (grid, flipped)
}

fn render_grid(grid: &[bool], n: usize, scale: usize) -> Img {
    render_modules(n, scale, |x, y| grid[y * n + x])
}

#[test]
fn qr_14_errors_up_to_capacity_are_corrected() {
    let mut rng = Rng(9);
    let mut ok = 0;
    let mut total = 0;
    for version in [1u8, 2, 5, 7, 10, 16, 25, 40] {
        for ecc in Ecc::ALL {
            let (code, data) = random_code(&mut rng, version, ecc, version % 8);
            let (grid, flipped) = corrupt(&code, &mut rng, &|_, e| e / 2);
            let img = render_grid(&grid, code.size(), 3);
            let d = decode_luma(img.w, img.h, &img.luma());
            total += 1;
            let d = d.unwrap_or_else(|e| panic!("v{version} {ecc:?}: {e}"));
            assert_eq!(d.bytes, data);
            assert_eq!(d.corrected_errors, flipped, "v{version} {ecc:?}");
            ok += 1;
        }
    }
    rate("errors at capacity", ok, total, 1.0);
}

#[test]
fn qr_14_errors_over_capacity_are_refused_not_misread() {
    let mut rng = Rng(10);
    let (mut refused, mut wrong, mut total) = (0, 0, 0);
    for version in [1u8, 2, 3, 5, 7, 10, 16, 25, 40] {
        for ecc in Ecc::ALL {
            for round in 0..3 {
                let (code, data) = random_code(
                    &mut rng,
                    version,
                    ecc,
                    ((usize::from(version) + round) % 8) as u8,
                );
                // One block gets one error more than it can correct, and
                // the others stay at their limit.
                let (grid, _) = corrupt(&code, &mut rng, &|b, e| {
                    if b == 0 {
                        e / 2 + 1
                    } else {
                        e / 2
                    }
                });
                let img = render_grid(&grid, code.size(), 3);
                total += 1;
                match decode_luma(img.w, img.h, &img.luma()) {
                    Err(DecodeError::TooManyErrors) => refused += 1,
                    Err(e) => panic!("v{version} {ecc:?}: {e}"),
                    Ok(d) if d.bytes == data => panic!("v{version} {ecc:?} decoded past capacity"),
                    Ok(_) => wrong += 1,
                }
            }
        }
    }
    println!("over capacity: {refused} refused, {wrong} misread of {total}");
    // A miscorrection is possible in theory, and rare where a block has
    // many error-correction codewords.
    assert!(wrong * 20 <= total, "{wrong} of {total}");
}

#[test]
fn qr_14_heavy_damage_is_refused() {
    let mut rng = Rng(21);
    let (code, _) = random_code(&mut rng, 10, Ecc::M, 3);
    let (grid, _) = corrupt(&code, &mut rng, &|_, e| e);
    let img = render_grid(&grid, code.size(), 4);
    assert_eq!(img.decode(), Err(DecodeError::TooManyErrors));
}

#[test]
fn qr_12_no_code_in_blank_or_noisy_frames() {
    let blank = Img::new(200, 120, 255.0);
    assert_eq!(blank.decode(), Err(DecodeError::NotFound));
    let black = Img::new(64, 64, 0.0);
    assert_eq!(black.decode(), Err(DecodeError::NotFound));
    let mut rng = Rng(3);
    let mut noisy = Img::new(300, 200, 0.0);
    for p in &mut noisy.px {
        *p = rng.below(256) as f32;
    }
    assert!(matches!(
        noisy.decode(),
        Err(DecodeError::NotFound | DecodeError::FormatUnreadable | DecodeError::TooManyErrors)
    ));
}

#[test]
fn qr_10_frame_bounds_and_stride() {
    let ok = vec![255u8; 16 * 16];
    assert_eq!(decode_luma(16, 16, &ok), Err(DecodeError::NotFound));
    assert_eq!(decode_luma(15, 16, &ok), Err(DecodeError::BadFrame));
    assert_eq!(decode_luma(16, 15, &ok), Err(DecodeError::BadFrame));
    assert_eq!(decode_luma(0, 0, &[]), Err(DecodeError::BadFrame));
    assert_eq!(decode_luma(16, 16, &ok[1..]), Err(DecodeError::BadFrame));
    assert_eq!(decode_luma(4097, 16, &ok), Err(DecodeError::TooLarge));
    assert_eq!(decode_luma(16, 4097, &ok), Err(DecodeError::TooLarge));
    assert_eq!(
        decode_luma(usize::MAX, usize::MAX, &ok),
        Err(DecodeError::TooLarge)
    );
    assert_eq!(
        decode_luma_strided(16, 16, 15, &ok),
        Err(DecodeError::BadFrame)
    );
    assert_eq!(
        decode_luma_strided(16, 16, 17, &ok),
        Err(DecodeError::BadFrame)
    );
    assert_eq!(
        decode_luma_strided(16, 16, usize::MAX, &ok),
        Err(DecodeError::BadFrame)
    );
    // A stride and padding, the last row short.
    let mut rng = Rng(4);
    let (code, data) = random_code(&mut rng, 9, Ecc::Q, 5);
    let img = render(&code, 4);
    let stride = img.w + 13;
    let luma = img.luma();
    let mut padded = Vec::new();
    for y in 0..img.h {
        padded.extend_from_slice(&luma[y * img.w..(y + 1) * img.w]);
        if y + 1 < img.h {
            padded.extend((0..13).map(|i| (i * 19) as u8));
        }
    }
    let d = decode_luma_strided(img.w, img.h, stride, &padded).unwrap();
    assert_eq!(d.bytes, data);
}

#[test]
fn qr_16_a_huge_frame_of_noise_stays_in_budget() {
    let mut rng = Rng(99);
    let n = 4096;
    let mut luma = vec![0u8; n * n];
    // Stripes in the 1:1:3:1:1 ratio on every row, to draw confirmations.
    for (i, p) in luma.iter_mut().enumerate() {
        let x = i % n;
        let dark = [1usize, 0, 1, 1, 1, 0, 1][(x / 3) % 7] == 1;
        *p = if dark ^ (rng.below(7) == 0) { 0 } else { 255 };
    }
    let r = decode_luma(n, n, &luma);
    assert!(matches!(
        r,
        Err(DecodeError::NotFound
            | DecodeError::Budget
            | DecodeError::FormatUnreadable
            | DecodeError::TooManyErrors)
    ));
}

/// A code from hand-made segment bits.
fn code_from_bits(bits: &[bool], version: u8, ecc: Ecc, mask: u8) -> QrCode {
    let capacity = tables::data_codewords(version, ecc);
    let mut bits = bits.to_vec();
    let room = capacity * 8;
    bits.extend(std::iter::repeat_n(
        false,
        room.saturating_sub(bits.len()).min(4),
    ));
    while !bits.len().is_multiple_of(8) {
        bits.push(false);
    }
    let mut data: Vec<u8> = bits
        .chunks(8)
        .map(|c| c.iter().fold(0u8, |a, &b| (a << 1) | u8::from(b)))
        .collect();
    let mut pad = [0xECu8, 0x11].into_iter().cycle();
    while data.len() < capacity {
        data.extend(pad.next());
    }
    let all = with_error_correction(&data, version, ecc);
    let (modules, m) = draw(version, ecc, &all, Some(mask));
    QrCode {
        size: tables::side(version),
        version,
        mask: m,
        ecc,
        mode: crate::Mode::Byte,
        modules,
    }
}

fn push(bits: &mut Vec<bool>, value: u32, n: u32) {
    for i in (0..n).rev() {
        bits.push((value >> i) & 1 != 0);
    }
}

fn byte_segment(bits: &mut Vec<bool>, version: u8, data: &[u8]) {
    push(bits, 4, 4);
    push(bits, data.len() as u32, if version <= 9 { 8 } else { 16 });
    for &b in data {
        push(bits, u32::from(b), 8);
    }
}

fn read_back(bits: &[bool], version: u8) -> Result<Vec<u8>, DecodeError> {
    let code = code_from_bits(bits, version, Ecc::M, 2);
    render(&code, 4).decode()
}

#[test]
fn qr_15_eci_utf8_and_latin1_are_read() {
    let mut bits = Vec::new();
    push(&mut bits, 7, 4);
    push(&mut bits, 26, 8);
    byte_segment(&mut bits, 3, "h\u{e9}llo \u{20bf}".as_bytes());
    assert_eq!(
        read_back(&bits, 3).unwrap(),
        "h\u{e9}llo \u{20bf}".as_bytes()
    );

    let mut bits = Vec::new();
    push(&mut bits, 7, 4);
    push(&mut bits, 3, 8);
    byte_segment(&mut bits, 3, &[b'c', b'a', b'f', 0xE9]);
    assert_eq!(read_back(&bits, 3).unwrap(), "caf\u{e9}".as_bytes());

    // A two-byte designator for ECI 26 is also read.
    let mut bits = Vec::new();
    push(&mut bits, 7, 4);
    push(&mut bits, 0x8000 | 26, 16);
    byte_segment(&mut bits, 3, b"ok");
    assert_eq!(read_back(&bits, 3).unwrap(), b"ok");
}

#[test]
fn qr_15_other_eci_and_modes_are_refused() {
    for designator in [20u32, 1, 4, 899] {
        let mut bits = Vec::new();
        push(&mut bits, 7, 4);
        if designator < 128 {
            push(&mut bits, designator, 8);
        } else {
            push(&mut bits, 0x8000 | designator, 16);
        }
        byte_segment(&mut bits, 3, b"x");
        assert_eq!(
            read_back(&bits, 3),
            Err(DecodeError::UnsupportedSegment),
            "{designator}"
        );
    }
    // Kanji, structured append, FNC1 in both positions.
    for mode in [8u32, 3, 5, 9] {
        let mut bits = Vec::new();
        push(&mut bits, mode, 4);
        push(&mut bits, 0, 16);
        assert_eq!(
            read_back(&bits, 3),
            Err(DecodeError::UnsupportedSegment),
            "mode {mode}"
        );
    }
}

#[test]
fn qr_17_malformed_segments_are_refused() {
    // A byte segment that claims more than the code holds.
    let mut bits = Vec::new();
    push(&mut bits, 4, 4);
    push(&mut bits, 255, 8);
    push(&mut bits, 0x41, 8);
    assert_eq!(read_back(&bits, 1), Err(DecodeError::BadSegment));
    // A numeric group of three digits over 999.
    let mut bits = Vec::new();
    push(&mut bits, 1, 4);
    push(&mut bits, 3, 10);
    push(&mut bits, 1023, 10);
    assert_eq!(read_back(&bits, 1), Err(DecodeError::BadSegment));
    // An alphanumeric pair over 2024.
    let mut bits = Vec::new();
    push(&mut bits, 2, 4);
    push(&mut bits, 2, 9);
    push(&mut bits, 2047, 11);
    assert_eq!(read_back(&bits, 1), Err(DecodeError::BadSegment));
    // A designator of four bytes.
    let mut bits = Vec::new();
    push(&mut bits, 7, 4);
    push(&mut bits, 0xFF, 8);
    assert_eq!(read_back(&bits, 1), Err(DecodeError::BadSegment));
}

#[test]
fn qr_10_several_segments_join() {
    let mut bits = Vec::new();
    push(&mut bits, 1, 4);
    push(&mut bits, 5, 10);
    push(&mut bits, 123, 10);
    push(&mut bits, 45, 7);
    push(&mut bits, 2, 4);
    push(&mut bits, 3, 9);
    push(&mut bits, 10 * 45 + 11, 11); // "AB"
    push(&mut bits, 44, 6); // ":"
    byte_segment(&mut bits, 3, b"-x");
    assert_eq!(read_back(&bits, 3).unwrap(), b"12345AB:-x");
}

#[test]
fn qr_10_numeric_and_alphanumeric_codes_round_trip() {
    let mut rng = Rng(31);
    for i in 0..24 {
        let version = 1 + (i * 5 % 40) as u8;
        let numeric = i % 2 == 0;
        let len = 1 + rng.below(14);
        let data: Vec<u8> = (0..len)
            .map(|_| {
                if numeric {
                    b'0' + rng.below(10) as u8
                } else {
                    b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ $%*+-./:"[rng.below(45)]
                }
            })
            .collect();
        let code = encode_with(&data, level(i), Some(version), None).unwrap();
        let img = render(&code, 2 + i % 4);
        assert_eq!(img.decode().unwrap(), data, "{i}");
    }
    let code = encode_with(b"", Ecc::L, None, None).unwrap();
    assert_eq!(render(&code, 4).decode().unwrap(), b"");
}

#[test]
fn qr_14_reed_solomon_corrects_up_to_half_the_check_symbols() {
    let mut rng = Rng(41);
    for ecc_len in [7usize, 10, 13, 17, 20, 28, 30] {
        let div = rs::divisor(ecc_len);
        for _ in 0..40 {
            let data: Vec<u8> = (0..(20 + rng.below(100)))
                .map(|_| rng.below(256) as u8)
                .collect();
            let mut block = data.clone();
            block.extend(rs::remainder(&data, &div));
            let clean = block.clone();
            let n_err = rng.below(ecc_len / 2 + 1);
            let mut places: Vec<usize> = (0..block.len()).collect();
            for _ in 0..n_err {
                let at = places.swap_remove(rng.below(places.len()));
                block[at] ^= 1 + rng.below(255) as u8;
            }
            assert_eq!(rs::correct(&mut block, ecc_len), Some(n_err));
            assert_eq!(block, clean);
        }
    }
}

#[test]
fn qr_14_reed_solomon_refuses_without_changing_into_garbage() {
    let mut block = vec![0u8; 40];
    assert_eq!(rs::correct(&mut block, 0), None);
    assert_eq!(rs::correct(&mut block, 40), None);
    assert_eq!(rs::correct(&mut [], 10), None);
    assert_eq!(rs::correct(&mut vec![1u8; 300], 10), None);
}

#[test]
fn qr_10_error_texts_start_with_their_code() {
    for (e, code) in [
        (DecodeError::BadFrame, "QR-10"),
        (DecodeError::TooLarge, "QR-11"),
        (DecodeError::NotFound, "QR-12"),
        (DecodeError::FormatUnreadable, "QR-13"),
        (DecodeError::TooManyErrors, "QR-14"),
        (DecodeError::UnsupportedSegment, "QR-15"),
        (DecodeError::Budget, "QR-16"),
        (DecodeError::BadSegment, "QR-17"),
    ] {
        assert!(e.to_string().starts_with(code), "{e}");
    }
}

/// A frame of 1280 by 720 holding a version 10 code at 4 pixels a module.
#[test]
#[ignore = "timing; run with --release --ignored --nocapture"]
fn qr_10_time_one_frame() {
    let mut rng = Rng(1);
    let (code, data) = random_code(&mut rng, 10, Ecc::M, 4);
    let small = render(&code, 4);
    let mut frame = Img::new(1280, 720, 230.0);
    let (ox, oy) = (311, 197);
    for y in 0..small.h {
        for x in 0..small.w {
            frame.px[(y + oy) * 1280 + x + ox] = small.at(x, y) * 0.8 + 30.0;
        }
    }
    let frame = noise(&frame, &mut rng, 20.0);
    let luma = frame.luma();
    let start = std::time::Instant::now();
    let runs = 20;
    for _ in 0..runs {
        assert_eq!(decode_luma(1280, 720, &luma).unwrap().bytes, data);
    }
    let each = start.elapsed() / runs;
    println!("1280x720, version 10 at 4 px: {each:?} per frame");
    let blank = vec![230u8; 1280 * 720];
    let start = std::time::Instant::now();
    for _ in 0..runs {
        let _ = decode_luma(1280, 720, &blank);
    }
    println!(
        "1280x720 with no code: {:?} per frame",
        start.elapsed() / runs
    );
}
