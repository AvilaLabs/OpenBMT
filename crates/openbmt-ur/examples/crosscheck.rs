//! The OpenBMT side of the cross-check against Hummingbird
//! (`scripts/ur-crosscheck.sh`).
//!
//!   crosscheck gen                      the cases, with our parts, on stdout
//!   crosscheck read CASES THEIRS        decode Hummingbird's parts
//!
//! `gen` prints, for each of several hundred messages from a fixed seed,
//! `case <n> <type> <max fragment> <hex of the CBOR payload>`, then `ours
//! <part>` for each of the first max(4 * seqLen, 32) parts (one for a single part),
//! then `end`. `read` takes the file the Java side wrote from the same
//! cases (`ordered` and `lossy` parts of Hummingbird's encoder) and decodes
//! each group; the type and the bytes must be ours.

use openbmt_ur::cbor::encode_bytes;
use openbmt_ur::{Decoder, Encoder};

/// The same small generator everywhere: SplitMix64, seeded.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn unhex(s: &str) -> Vec<u8> {
    (0..s.len() / 2)
        .filter_map(|i| u8::from_str_radix(s.get(2 * i..2 * i + 2)?, 16).ok())
        .collect()
}

const CASES: u64 = 400;

fn gen() {
    let mut rng = Rng(0x0b3d_0000_5552);
    for idx in 0..CASES {
        // Sizes from 1 byte to 20 KB, thick at the small end.
        let len = match idx % 5 {
            0 => 1 + rng.below(40),
            1 => 1 + rng.below(400),
            2 => 1 + rng.below(3000),
            3 => 1 + rng.below(20_000),
            _ => 20_000,
        } as usize;
        let mut data: Vec<u8> = (0..len).map(|_| rng.below(256) as u8).collect();
        let ur_type = if idx % 2 == 0 { "crypto-psbt" } else { "bytes" };
        if ur_type == "crypto-psbt" && idx % 4 == 0 {
            // PSBT-shaped: the magic, then a global map's worth of zeros and
            // random bytes.
            let magic = b"psbt\xff";
            for (d, m) in data.iter_mut().zip(magic) {
                *d = *m;
            }
        }
        let payload = encode_bytes(&data);
        let options = [10usize, 20, 50, 100, 200, 500, 1000, 2000];
        let pick = options
            .get(rng.below(options.len() as u64) as usize)
            .copied()
            .unwrap_or(100);
        // No more than about 250 fragments, to keep the files small.
        let max = pick.max((payload.len() / 250).clamp(10, 2000));
        let Ok(mut e) = Encoder::new(ur_type, &payload, max, 10) else {
            continue;
        };
        println!("case {idx} {ur_type} {max} {}", hex(&payload));
        let total = if e.is_single_part() {
            1
        } else {
            (4 * e.seq_len()).max(32)
        };
        for _ in 0..total {
            println!("ours {}", e.next_part());
        }
        println!("end");
    }
}

/// A case as the file gives it: the index, the type, the CBOR payload.
fn read_cases(path: &str) -> Vec<(String, String, Vec<u8>)> {
    let text = std::fs::read_to_string(path).unwrap_or_default();
    text.lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.split(' ').collect();
            match f.as_slice() {
                ["case", idx, ty, _max, hexed] => {
                    Some((idx.to_string(), ty.to_string(), unhex(hexed)))
                }
                _ => None,
            }
        })
        .collect()
}

fn read(cases: &str, theirs: &str) -> bool {
    let cases = read_cases(cases);
    let text = std::fs::read_to_string(theirs).unwrap_or_default();
    let mut failures = 0;
    let mut checked = (0, 0);
    let mut at = 0usize;
    let mut ordered: Vec<&str> = Vec::new();
    let mut lossy: Vec<&str> = Vec::new();
    for line in text.lines() {
        if line.starts_with("case ") {
            ordered.clear();
            lossy.clear();
        } else if let Some(p) = line.strip_prefix("ordered ") {
            ordered.push(p);
        } else if let Some(p) = line.strip_prefix("lossy ") {
            lossy.push(p);
        } else if line == "end" {
            let Some((idx, ty, payload)) = cases.get(at) else {
                println!("FAIL  more cases in theirs than in ours");
                return false;
            };
            at += 1;
            for (name, parts, n) in [
                ("in order", &ordered, &mut checked.0),
                ("with losses", &lossy, &mut checked.1),
            ] {
                let mut d = Decoder::new();
                for p in parts.iter() {
                    if let Err(e) = d.receive(p) {
                        println!("FAIL  case {idx} {name}: refused a part: {e}\n   {p}");
                        failures += 1;
                        break;
                    }
                }
                match d.result() {
                    Some((t, got)) if &t == ty && &got == payload => *n += 1,
                    other => {
                        println!(
                            "FAIL  case {idx} {name}: got {:?}",
                            other.map(|(t, g)| (t, g.len()))
                        );
                        failures += 1;
                    }
                }
            }
        }
    }
    if at != cases.len() {
        println!("FAIL  {at} cases read of {}", cases.len());
        failures += 1;
    }
    println!(
        "Hummingbird's parts decoded by openbmt-ur: in order {}, with 30% lost {} (of {})",
        checked.0,
        checked.1,
        cases.len()
    );
    failures == 0
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        ["gen"] => gen(),
        ["read", cases, theirs] => {
            if read(cases, theirs) {
                println!("PASS  openbmt-ur side");
            } else {
                std::process::exit(1);
            }
        }
        _ => {
            eprintln!("usage: crosscheck gen | crosscheck read CASES THEIRS");
            std::process::exit(2);
        }
    }
}
