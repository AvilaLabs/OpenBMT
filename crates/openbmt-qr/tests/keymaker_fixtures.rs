//! Fixtures for the key maker's JavaScript port of the encoder
//! (`tools/keymaker/src/qr.js`). The inputs are in
//! `tools/keymaker/test/vectors/qr-inputs.txt`; for each, byte mode at
//! level M, `qr-fixtures.txt` holds the version, the mask and the matrix as
//! rows of 0 and 1. The JavaScript test compares every matrix exactly.
//! `OPENBMT_WRITE_QR_FIXTURES=1` writes the file; otherwise it is checked.
//!
//! The reader is checked the other way round: the PGM files in
//! `tools/keymaker/test/vectors/` are what the JavaScript drawing makes of
//! three of those codes, and `decode_luma` must give back the bytes.

use std::fmt::Write as _;
use std::path::PathBuf;

use openbmt_qr::{decode_luma, encode_bytes, Ecc};

fn vectors() -> PathBuf {
    let root = std::env::var_os("CARGO_MANIFEST_DIR").map(PathBuf::from);
    root.map(|r| r.join("../../tools/keymaker/test/vectors"))
        .unwrap_or_default()
}

fn from_hex(s: &str) -> Vec<u8> {
    if s == "-" {
        return Vec::new();
    }
    (0..s.len() / 2)
        .map(|i| u8::from_str_radix(&s[2 * i..2 * i + 2], 16).unwrap_or(0))
        .collect()
}

fn inputs() -> Vec<Vec<u8>> {
    let text = std::fs::read_to_string(vectors().join("qr-inputs.txt")).unwrap_or_default();
    text.lines().map(from_hex).collect()
}

fn render(inputs: &[Vec<u8>]) -> String {
    let mut out = String::new();
    for data in inputs {
        let code = encode_bytes(data, Ecc::M).unwrap_or_else(|e| panic!("{e}"));
        let hex: String = if data.is_empty() {
            "-".into()
        } else {
            data.iter().fold(String::new(), |mut s, b| {
                let _ = write!(s, "{b:02x}");
                s
            })
        };
        let _ = writeln!(out, "input {hex}");
        let _ = writeln!(out, "version {}", code.version());
        let _ = writeln!(out, "mask {}", code.mask());
        for y in 0..code.size() {
            let row: String = (0..code.size())
                .map(|x| if code.get(x, y) { '1' } else { '0' })
                .collect();
            let _ = writeln!(out, "{row}");
        }
        let _ = writeln!(out, "end");
    }
    out
}

#[test]
fn keymaker_qr_fixtures() {
    let inputs = inputs();
    assert!(inputs.len() >= 30, "the inputs file is missing or short");
    let text = render(&inputs);
    let path = vectors().join("qr-fixtures.txt");
    if std::env::var_os("OPENBMT_WRITE_QR_FIXTURES").is_some_and(|v| v == "1") {
        std::fs::write(&path, &text).unwrap_or_else(|e| panic!("{e}"));
    } else {
        let on_disk = std::fs::read_to_string(&path).unwrap_or_default();
        assert!(on_disk == text, "qr-fixtures.txt differs from the encoder");
    }
}

/// Reads a binary PGM (P5, 255) and returns width, height and the pixels.
fn read_pgm(bytes: &[u8]) -> (usize, usize, Vec<u8>) {
    let mut fields = Vec::new();
    let mut at = 0;
    while fields.len() < 4 {
        while bytes.get(at).is_some_and(u8::is_ascii_whitespace) {
            at += 1;
        }
        let start = at;
        while bytes.get(at).is_some_and(|b| !b.is_ascii_whitespace()) {
            at += 1;
        }
        fields.push(String::from_utf8_lossy(&bytes[start..at]).into_owned());
    }
    at += 1;
    assert_eq!(fields[0], "P5");
    assert_eq!(fields[3], "255");
    let w: usize = fields[1].parse().unwrap_or(0);
    let h: usize = fields[2].parse().unwrap_or(0);
    (w, h, bytes[at..].to_vec())
}

#[test]
fn keymaker_pgm_pictures_decode_to_their_bytes() {
    let inputs = inputs();
    // Input numbers: 0 the seed's multipath descriptor, 1 its receive
    // descriptor, 24 the single byte 0x41.
    let pictures = [("one-byte", 24), ("seed-receive", 1), ("seed-multipath", 0)];
    for (name, number) in pictures {
        let bytes = std::fs::read(vectors().join(format!("qr-{name}.pgm"))).unwrap_or_default();
        let (w, h, luma) = read_pgm(&bytes);
        let decoded = decode_luma(w, h, &luma).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(Some(&decoded.bytes), inputs.get(number), "{name}");
        assert_eq!(decoded.ecc, Ecc::M);
    }
}
