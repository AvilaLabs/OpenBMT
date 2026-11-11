//! The reader's side of `scripts/qr-crosscheck.sh`.
//!
//! - `decodecheck gen DIR N` writes N images of codes made by this encoder
//!   as `ours_clean_I.pgm` (module size 2 to 6, quiet zone 4, turned by a
//!   multiple of 90 degrees, or mirrored) and `ours_hard_I.pgm` (lower
//!   contrast, blur and noise, off-axis), each with `.txt` holding the
//!   data in hex.
//! - `decodecheck read DIR` decodes every `.pgm` in DIR and writes
//!   `NAME.out`: `OK` and the data in hex, or `ERR` and the error text.
//!
//! The Java harness makes images of its own codes, and decodes ours.

use std::fs;
use std::path::Path;

use openbmt_qr::{decode_luma, encode_with, Ecc};

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
}

const ALNUM: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ $%*+-./:";

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// A square of gray levels.
struct Img {
    w: usize,
    h: usize,
    px: Vec<f32>,
}

impl Img {
    fn rotate90(&self) -> Img {
        let mut px = vec![0.0; self.px.len()];
        for y in 0..self.h {
            for x in 0..self.w {
                px[x * self.h + (self.h - 1 - y)] = self.px[y * self.w + x];
            }
        }
        Img {
            w: self.h,
            h: self.w,
            px,
        }
    }

    fn mirror(&self) -> Img {
        let mut px = self.px.clone();
        for y in 0..self.h {
            for x in 0..self.w {
                px[y * self.w + x] = self.px[y * self.w + self.w - 1 - x];
            }
        }
        Img {
            w: self.w,
            h: self.h,
            px,
        }
    }

    fn blur(&self) -> Img {
        let mut px = self.px.clone();
        for y in 0..self.h {
            for x in 0..self.w {
                let mut sum = 0.0;
                for dy in -1i64..=1 {
                    for dx in -1i64..=1 {
                        let xx = (x as i64 + dx).clamp(0, self.w as i64 - 1) as usize;
                        let yy = (y as i64 + dy).clamp(0, self.h as i64 - 1) as usize;
                        sum += self.px[yy * self.w + xx];
                    }
                }
                px[y * self.w + x] = sum / 9.0;
            }
        }
        Img {
            w: self.w,
            h: self.h,
            px,
        }
    }

    fn write(&self, path: &Path) {
        let mut out = format!("P5\n{} {}\n255\n", self.w, self.h).into_bytes();
        out.extend(self.px.iter().map(|&v| v.clamp(0.0, 255.0).round() as u8));
        fs::write(path, out).expect("write image");
    }
}

fn gen(dir: &Path, n: usize) {
    fs::create_dir_all(dir).expect("create dir");
    let mut rng = Rng(0x0BD7_0001);
    for i in 0..n {
        let kind = i % 3;
        let len = 1 + rng.below(match kind {
            0 => 400,
            1 => 250,
            _ => 150,
        });
        let data: Vec<u8> = (0..len)
            .map(|_| match kind {
                0 => b'0' + rng.below(10) as u8,
                1 => ALNUM[rng.below(45)],
                _ => 0x80 + rng.below(0x80) as u8,
            })
            .collect();
        let ecc = Ecc::ALL[rng.below(4)];
        let code = encode_with(&data, ecc, None, None).expect("encode");
        let scale = 2 + rng.below(5);
        let n_mod = code.size();
        let side = (n_mod + 8) * scale;
        let mut img = Img {
            w: side,
            h: side,
            px: vec![255.0; side * side],
        };
        for y in 0..n_mod {
            for x in 0..n_mod {
                if code.get(x, y) {
                    for dy in 0..scale {
                        for dx in 0..scale {
                            img.px[((y + 4) * scale + dy) * side + (x + 4) * scale + dx] = 0.0;
                        }
                    }
                }
            }
        }
        for _ in 0..rng.below(4) {
            img = img.rotate90();
        }
        if rng.below(4) == 0 {
            img = img.mirror();
        }
        img.write(&dir.join(format!("ours_clean_{i}.pgm")));
        fs::write(dir.join(format!("ours_clean_{i}.txt")), hex(&data)).expect("write");
        // The same code with a poorer picture of it.
        let mut hard = img.blur();
        for p in &mut hard.px {
            let noise = (0..4)
                .map(|_| rng.below(1000) as f32 / 1000.0 - 0.5)
                .sum::<f32>()
                * 10.0;
            *p = 70.0 + *p * 0.55 + noise;
        }
        hard.write(&dir.join(format!("ours_hard_{i}.pgm")));
        fs::write(dir.join(format!("ours_hard_{i}.txt")), hex(&data)).expect("write");
    }
}

fn read_pgm(path: &Path) -> Option<(usize, usize, Vec<u8>)> {
    let bytes = fs::read(path).ok()?;
    let mut pos = 0;
    let mut tokens = Vec::new();
    while tokens.len() < 4 {
        while bytes.get(pos)?.is_ascii_whitespace() {
            pos += 1;
        }
        let start = pos;
        while !bytes.get(pos)?.is_ascii_whitespace() {
            pos += 1;
        }
        tokens.push(String::from_utf8_lossy(&bytes[start..pos]).to_string());
    }
    pos += 1;
    let w: usize = tokens.get(1)?.parse().ok()?;
    let h: usize = tokens.get(2)?.parse().ok()?;
    Some((w, h, bytes.get(pos..)?.to_vec()))
}

fn read(dir: &Path) {
    let mut names: Vec<_> = fs::read_dir(dir)
        .expect("read dir")
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "pgm"))
        .collect();
    names.sort();
    for path in names {
        let line = match read_pgm(&path) {
            Some((w, h, px)) => match decode_luma(w, h, &px) {
                Ok(d) => format!("OK {}", hex(&d.bytes)),
                Err(e) => format!("ERR {e}"),
            },
            None => "ERR unreadable image".to_string(),
        };
        fs::write(path.with_extension("out"), line).expect("write result");
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    match (args.get(1).map(String::as_str), args.get(2)) {
        (Some("gen"), Some(dir)) => gen(
            Path::new(dir),
            args.get(3).and_then(|n| n.parse().ok()).unwrap_or(300),
        ),
        (Some("read"), Some(dir)) => read(Path::new(dir)),
        _ => {
            eprintln!("usage: decodecheck gen DIR N | decodecheck read DIR");
            std::process::exit(2);
        }
    }
}
