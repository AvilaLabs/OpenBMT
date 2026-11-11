//! SHA-256 and CRC-32 against the values `sha256sum` and Python's
//! `zlib.crc32` gave for the same inputs (checked when the tests were
//! written; the inputs are rebuilt here).

use crate::hash::{crc32, sha256};

fn hex(d: &[u8]) -> String {
    d.iter().map(|b| format!("{b:02x}")).collect()
}

fn inputs() -> Vec<Vec<u8>> {
    let seq: Vec<u8> = (0..=255u8).collect();
    let mut long = seq.repeat(4);
    long.extend_from_slice(&[b'x'; 55]);
    vec![
        Vec::new(),
        b"abc".to_vec(),
        b"hello world".to_vec(),
        seq,
        vec![b'a'; 1000],
        b"Wolf".to_vec(),
        long,
        vec![b'b'; 55],
        vec![b'b'; 56],
        vec![b'b'; 64],
    ]
}

const SHA: [&str; 10] = [
    "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
    "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
    "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9",
    "40aff2e9d2d8922e47afd4648e6967497158785fbd1da870e7110266bf944880",
    "41edece42d63e8d9bf515a9ba6932e1c20cbc9f5a5d134645adb5db1b9737ea3",
    "ea858afbf837aae714617e89a36524aced28f7de921f7798e72810fd8839a462",
    "415c3b5c395cb8896358cda2f03a8c43395029c22ed119c0e76617d75c9b65e9",
    "eb2c86e932179f4ba13fe8715a26124b77d6bad290b9b4c1cc140cf633300c19",
    "a5fc6e203a4c2b657d0d153885932414b2ffc6a93f0f8bf8b3183315e5a7212c",
    "a0fab1377f49a759b57f63318262ebe89fabfc990e8e93ceac2984561482b9d4",
];

const CRC: [u32; 10] = [
    0x0000_0000,
    0x3524_41c2,
    0x0d4a_1185,
    0x2905_8c73,
    0x9a38_da03,
    0x598c_84dc,
    0x088d_4a2f,
    0xfb36_0e4c,
    0x0e2f_d44c,
    0x0b74_7ffe,
];

#[test]
fn hash_sha256_matches_sha256sum() {
    for (input, want) in inputs().iter().zip(SHA) {
        assert_eq!(hex(&sha256(input)), want, "{} bytes", input.len());
    }
}

#[test]
fn hash_crc32_matches_zlib() {
    for (input, want) in inputs().iter().zip(CRC) {
        assert_eq!(crc32(input), want, "{} bytes", input.len());
    }
    // The standard check value of CRC-32.
    assert_eq!(crc32(b"123456789"), 0xcbf4_3926);
}
