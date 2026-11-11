//! Prints a deterministic set of cases for `scripts/qr-crosscheck.sh`: the
//! input, the mode, the level, the version, the mask and the module matrix,
//! as text on standard output.
//!
//! One case is a header line, a data line (hex), a penalties line (the
//! penalty of each of the eight masks at that version, or `-` when the
//! mask was forced), and one line of `0` and `1` for each row of modules.
//! Cases are separated by a blank line.
//!
//! - `forced` cases: every version 1 to 40 at every level with every mask,
//!   in byte mode filled to the version's capacity, and in numeric and
//!   alphanumeric mode filled to it with one mask each.
//! - `auto` cases: random lengths in each mode from a fixed seed, with the
//!   version and the mask chosen by the encoder.

use openbmt_qr::{encode_segment, penalty_of, Ecc, Mode, QrCode};

/// A small generator (splitmix64): the cases must not change between runs.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
}

const ALNUM: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ $%*+-./:";

/// `n` characters the mode can carry, and that no denser mode can (so that
/// every encoder picks the same mode). `n` must be at least 1.
fn make(mode: Mode, n: usize, rng: &mut Rng) -> Vec<u8> {
    let mut v: Vec<u8> = (0..n)
        .map(|_| match mode {
            Mode::Numeric => b'0' + rng.below(10) as u8,
            Mode::Alphanumeric => ALNUM.get(rng.below(45) as usize).copied().unwrap_or(b'A'),
            Mode::Byte => rng.below(256) as u8,
        })
        .collect();
    if let Some(first) = v.first_mut() {
        match mode {
            Mode::Numeric => {}
            Mode::Alphanumeric => *first = b'A' + rng.below(26) as u8,
            Mode::Byte => *first = 0x80 | (*first & 0x7F),
        }
    }
    v
}

fn name(mode: Mode) -> &'static str {
    match mode {
        Mode::Numeric => "numeric",
        Mode::Alphanumeric => "alphanumeric",
        Mode::Byte => "byte",
    }
}

fn ecc_name(e: Ecc) -> &'static str {
    match e {
        Ecc::L => "L",
        Ecc::M => "M",
        Ecc::Q => "Q",
        Ecc::H => "H",
    }
}

fn hex(data: &[u8]) -> String {
    data.iter().map(|b| format!("{b:02x}")).collect()
}

fn emit(id: usize, kind: &str, data: &[u8], code: &QrCode, penalties: Option<[u32; 8]>) {
    println!(
        "case {id} kind={kind} mode={} ecc={} version={} mask={} len={}",
        name(code.mode()),
        ecc_name(code.ecc()),
        code.version(),
        code.mask(),
        data.len()
    );
    println!("data {}", hex(data));
    match penalties {
        Some(p) => {
            let t: Vec<String> = p.iter().map(u32::to_string).collect();
            println!("penalties {}", t.join(" "));
        }
        None => println!("penalties -"),
    }
    for y in 0..code.size() {
        let row: String = (0..code.size())
            .map(|x| if code.get(x, y) { '1' } else { '0' })
            .collect();
        println!("{row}");
    }
    println!();
}

/// The longest input of `mode` that fits version `v` at `ecc`.
fn max_len(mode: Mode, v: u8, ecc: Ecc, rng: &mut Rng) -> usize {
    let (mut lo, mut hi) = (1usize, 8000usize);
    while lo < hi {
        let mid = (lo + hi).div_ceil(2);
        let data = make(mode, mid, rng);
        if encode_segment(mode, &data, ecc, Some(v), Some(0)).is_ok() {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    lo
}

fn main() {
    let mut rng = Rng(0x0BAD_C0DE_5EED_0001);
    let mut id = 0usize;
    for v in 1..=40u8 {
        for ecc in Ecc::ALL {
            for mode in [Mode::Byte, Mode::Numeric, Mode::Alphanumeric] {
                let n = max_len(mode, v, ecc, &mut rng);
                let masks: Vec<u8> = if mode == Mode::Byte {
                    (0..8).collect()
                } else {
                    vec![(v as usize + ecc as usize) as u8 % 8]
                };
                for mask in masks {
                    let data = make(mode, n, &mut rng);
                    if let Ok(code) = encode_segment(mode, &data, ecc, Some(v), Some(mask)) {
                        emit(id, "forced", &data, &code, None);
                        id += 1;
                    }
                }
            }
        }
    }
    for mode in [Mode::Numeric, Mode::Alphanumeric, Mode::Byte] {
        let top: u64 = match mode {
            Mode::Numeric => 7089,
            Mode::Alphanumeric => 4296,
            Mode::Byte => 2953,
        };
        for _ in 0..400 {
            let ecc = Ecc::ALL
                .get(rng.below(4) as usize)
                .copied()
                .unwrap_or(Ecc::L);
            // Mostly short inputs, some long: square a uniform draw.
            let u = rng.below(1000);
            let mut n = 1 + (u * u * top / 1_000_000) as usize;
            let (data, code) = loop {
                let data = make(mode, n, &mut rng);
                match encode_segment(mode, &data, ecc, None, None) {
                    Ok(code) => break (data, code),
                    Err(_) => n = n / 2 + 1,
                }
            };
            let mut p = [0u32; 8];
            for (m, slot) in p.iter_mut().enumerate() {
                if let Ok(c) = encode_segment(mode, &data, ecc, Some(code.version()), Some(m as u8))
                {
                    *slot = penalty_of(&c);
                }
            }
            emit(id, "auto", &data, &code, Some(p));
            id += 1;
        }
    }
}
