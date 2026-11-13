//! What the stick must show for given input, from the Rust core, for
//! `stick/test-qemu.sh` (SK-40). Prints `key value` lines.
//!
//!   expected dice <rolls as digits>      dice alone (SK-22)
//!   expected words <12 words>            the public result of words (SK-26)

use openbmt_qr::{Ecc, QrCode};
use openbmt_stick::mix::{dice_alone, dice_values};
use openbmt_stick::outputs::{public_of, words_of};
use openbmt_stick::seedqr::compact_seedqr;

fn rows(label: &str, qr: &QrCode) {
    println!("{label}size {}", qr.size());
    for y in 0..qr.size() {
        let row: String = (0..qr.size())
            .map(|x| if qr.get(x, y) { '#' } else { '.' })
            .collect();
        println!("{label}row {row}");
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let words: Vec<&str> = match args.first().map(String::as_str) {
        Some("dice") => {
            let rolls: Vec<u8> = args[1].bytes().map(|b| b - b'0').collect();
            let values = dice_values(&rolls).expect("rolls");
            let entropy = dice_alone(&values).expect("50 values");
            let w = words_of(&entropy).expect("words");
            println!("words {}", w.join(" "));
            let q = compact_seedqr(&entropy).expect("seedqr");
            rows("seedqr", &q);
            w
        }
        Some("words") => args[1..].iter().map(String::as_str).collect(),
        _ => {
            eprintln!("usage: expected dice <rolls> | expected words <12 words>");
            std::process::exit(2);
        }
    };
    let p = public_of(&words).expect("public");
    println!("fingerprint {}", p.fingerprint);
    println!("address {}", p.address);
    println!("descriptor {}", p.descriptor);
    let q = openbmt_qr::encode_text(&p.descriptor, Ecc::M).expect("qr");
    rows("descriptorqr", &q);
    if args[0] == "words" {
        let m = bip39::Mnemonic::parse_normalized(&words.join(" ")).expect("words");
        let mut e = [0u8; 16];
        e.copy_from_slice(&m.to_entropy());
        rows("seedqr", &compact_seedqr(&e).expect("seedqr"));
    }
}
