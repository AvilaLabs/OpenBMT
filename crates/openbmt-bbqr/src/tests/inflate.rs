//! BQ-11, BQ-12: the strict inflate. The compressed vectors were written by
//! zlib at a window of 10 bits with no header (Python's
//! `zlib.compressobj(level, zlib.DEFLATED, -10)`), the way the format says.

use super::unhex;
use crate::inflate::inflate;
use crate::{BbqrError, MAX_DECODED};

const FIXED: &str = "cb48cdc9c957c8c0470200";
const STORED: &str = "011400ebff000102030405060708090a0b0c0d0e0f10111213";
const DYNAMIC: &str = "ed924756c3401005af42ce5122e79c73ce41c686015b1eac1132b7e775759f819557d3af5efd5a4de1425aceb2b6acd4703ef4bad4e761a45038d9f479b290381f47eb51bd12efa9740ab9417daee5d5e02adc357102a0dd0a7df68eea720a6fd1e086e4f7899dc9f456a517c83b6a4a2de7ee10a71f306685697b9774b9897760f05cf277c45e65faa1521df283da496d807b5c9c19c0b215b6ec3dd4e505debdc144f28e989769a15217641035a236cbbd22ce36e0c80a97f63ee8b284f769f05bf24d62dd321d522986cca1ae52dbe13e16e70af06885377bbf74d9c0fb35d823f9616213329d57690db28b7a42ed9afb499c32a06a85ac68fd9cd6cff9f79ff307";
const ZLIB_DYNAMIC: &str = "78daed924756c3401005af42ce5122e79c73ce41c686015b1eac1132b7e775759f819557d3af5efd5a4de1425aceb2b6acd4703ef4bad4e761a45038d9f479b290381f47eb51bd12efa9740ab9417daee5d5e02adc357102a0dd0a7df68eea720a6fd1e086e4f7899dc9f456a517c83b6a4a2de7ee10a71f306685697b9774b9897760f05cf277c45e65faa1521df283da496d807b5c9c19c0b215b6ec3dd4e505debdc144f28e989769a15217641035a236cbbd22ce36e0c80a97f63ee8b284f769f05bf24d62dd321d522986cca1ae52dbe13e16e70af06885377bbf74d9c0fb35d823f9616213329d57690db28b7a42ed9afb499c32a06a85ac68fd9cd6cff9f79ff307870bd472";

/// The 1200 bytes the dynamic vector holds.
fn dynamic_data() -> Vec<u8> {
    let words: [&[u8]; 10] = [
        b"witness", b"script", b"sorted", b"multi", b"input", b"output", b"bip32", b"xpub",
        b"2of3", b"fee",
    ];
    let mut out: Vec<u8> = Vec::new();
    let mut i: usize = 0;
    while out.len() < 1200 {
        out.extend_from_slice(words[(i * i + i / 3) % 10]);
        out.push(u8::try_from(32 + (i * 7) % 90).unwrap_or(32));
        i += 1;
    }
    out.truncate(1200);
    out
}

/// Bits, least significant first, for building streams by hand.
#[derive(Default)]
struct BitWriter {
    bytes: Vec<u8>,
    used: u32,
}

impl BitWriter {
    fn bit(&mut self, b: u32) {
        if self.used == 0 {
            self.bytes.push(0);
        }
        if let Some(last) = self.bytes.last_mut() {
            *last |= u8::try_from(b & 1).unwrap_or(0) << self.used;
        }
        self.used = (self.used + 1) % 8;
    }

    /// `n` bits of `v`, least significant first (a number).
    fn num(&mut self, v: u32, n: u32) {
        for i in 0..n {
            self.bit(v >> i);
        }
    }

    /// `n` bits of `v`, most significant first (a Huffman code).
    fn code(&mut self, v: u32, n: u32) {
        for i in (0..n).rev() {
            self.bit(v >> i);
        }
    }
}

fn zlib_header_ok(data: &[u8]) -> bool {
    data.first() == Some(&0x78)
}

#[test]
fn bq_11_reads_a_fixed_block() {
    let out = inflate(&unhex(FIXED), MAX_DECODED).expect("fixed");
    assert_eq!(out, b"hello hello hello hello hello hello");
}

#[test]
fn bq_11_reads_a_stored_block() {
    let out = inflate(&unhex(STORED), MAX_DECODED).expect("stored");
    assert_eq!(out, (0u8..20).collect::<Vec<_>>());
}

#[test]
fn bq_11_reads_a_dynamic_block() {
    let out = inflate(&unhex(DYNAMIC), MAX_DECODED).expect("dynamic");
    assert_eq!(out, dynamic_data());
}

#[test]
fn bq_11_reads_a_zlib_wrapper_when_the_raw_reading_fails() {
    let wrapped = unhex(ZLIB_DYNAMIC);
    assert!(zlib_header_ok(&wrapped));
    let out = inflate(&wrapped, MAX_DECODED).expect("wrapped");
    assert_eq!(out, dynamic_data());
}

#[test]
fn bq_11_refuses_a_zlib_wrapper_with_a_wrong_checksum() {
    let mut wrapped = unhex(ZLIB_DYNAMIC);
    let n = wrapped.len();
    wrapped[n - 1] ^= 1;
    assert!(matches!(
        inflate(&wrapped, MAX_DECODED),
        Err(BbqrError::Inflate(_))
    ));
}

#[test]
fn bq_11_refuses_a_stream_that_ends_early() {
    let data = unhex(DYNAMIC);
    for cut in [0, 1, 2, 10, data.len() / 2, data.len() - 1] {
        let short = data.get(..cut).unwrap_or(&[]);
        assert!(
            matches!(inflate(short, MAX_DECODED), Err(BbqrError::Inflate(_))),
            "cut at {cut}"
        );
    }
}

#[test]
fn bq_11_refuses_bytes_after_the_last_block() {
    let mut data = unhex(FIXED);
    data.push(0);
    assert!(matches!(
        inflate(&data, MAX_DECODED),
        Err(BbqrError::Inflate(_))
    ));
}

#[test]
fn bq_11_refuses_a_block_type_that_does_not_exist() {
    // Final block, type 3.
    assert!(matches!(
        inflate(&[0b0000_0111, 0, 0], MAX_DECODED),
        Err(BbqrError::Inflate(_))
    ));
}

#[test]
fn bq_11_refuses_a_stored_block_whose_length_check_fails() {
    let mut data = unhex(STORED);
    data[3] ^= 1;
    assert!(matches!(
        inflate(&data, MAX_DECODED),
        Err(BbqrError::Inflate(_))
    ));
}

#[test]
fn bq_11_refuses_padding_bits_that_are_not_zero() {
    // The fixed block ends inside its last byte; set a padding bit.
    let mut data = unhex(FIXED);
    let n = data.len();
    data[n - 1] |= 0x80;
    assert!(matches!(
        inflate(&data, MAX_DECODED),
        Err(BbqrError::Inflate(_))
    ));
}

#[test]
fn bq_11_refuses_a_distance_before_the_start() {
    let mut w = BitWriter::default();
    w.num(1, 1); // final
    w.num(1, 2); // fixed
    w.code(0x30 + 0x61, 8); // literal 'a'
    w.code(1, 7); // length symbol 257: length 3
    w.code(1, 5); // distance symbol 1: distance 2
    w.code(0, 7); // end of block
    assert!(matches!(
        inflate(&w.bytes, MAX_DECODED),
        Err(BbqrError::Inflate(_))
    ));
}

#[test]
fn bq_11_reads_a_match_inside_the_output() {
    let mut w = BitWriter::default();
    w.num(1, 1);
    w.num(1, 2);
    w.code(0x30 + 0x61, 8); // 'a'
    w.code(1, 7); // length 3
    w.code(0, 5); // distance 1
    w.code(0, 7);
    assert_eq!(inflate(&w.bytes, MAX_DECODED).expect("match"), b"aaaa");
}

fn dynamic_header_with(code_length_lengths: [u32; 4]) -> Vec<u8> {
    let mut w = BitWriter::default();
    w.num(1, 1); // final
    w.num(2, 2); // dynamic
    w.num(0, 5); // 257 length codes
    w.num(0, 5); // 1 distance code
    w.num(0, 4); // 4 code length codes: 16, 17, 18, 0
    for l in code_length_lengths {
        w.num(l, 3);
    }
    w.num(0, 16);
    w.bytes
}

#[test]
fn bq_11_refuses_an_over_subscribed_code() {
    // Three codes of one bit each.
    assert!(matches!(
        inflate(&dynamic_header_with([1, 1, 1, 0]), MAX_DECODED),
        Err(BbqrError::Inflate(_))
    ));
}

#[test]
fn bq_11_refuses_an_incomplete_code() {
    // One code of one bit: half the code space is unused.
    assert!(matches!(
        inflate(&dynamic_header_with([1, 0, 0, 0]), MAX_DECODED),
        Err(BbqrError::Inflate(_))
    ));
}

#[test]
fn bq_11_refuses_a_repeat_with_nothing_before_it() {
    // Code lengths 16 (repeat) and 0 of one bit each; the first symbol
    // read is a repeat.
    let mut w = BitWriter::default();
    w.num(1, 1);
    w.num(2, 2);
    w.num(0, 5);
    w.num(0, 5);
    w.num(0, 4);
    for l in [1u32, 0, 0, 1] {
        w.num(l, 3);
    }
    // Symbol 0 has the code 0 and symbol 16 the code 1, then two bits.
    w.code(1, 1);
    w.num(0, 2);
    w.num(0, 16);
    assert!(matches!(
        inflate(&w.bytes, MAX_DECODED),
        Err(BbqrError::Inflate(_))
    ));
}

#[test]
fn bq_12_refuses_an_output_past_the_bound() {
    assert_eq!(inflate(&unhex(STORED), 19), Err(BbqrError::InflateTooBig));
    assert!(inflate(&unhex(STORED), 20).is_ok());
}

#[test]
fn bq_12_a_small_stream_cannot_make_a_large_output() {
    // A literal and then many copies of 258 bytes: a few bytes in, a
    // quarter of a million out.
    let mut w = BitWriter::default();
    w.num(1, 1);
    w.num(1, 2);
    w.code(0x30 + 0x61, 8);
    for _ in 0..1000 {
        w.code(0xc5, 8); // length symbol 285 (258): code 0xc5 of 8 bits
        w.code(0, 5); // distance 1
    }
    w.code(0, 7);
    let all = inflate(&w.bytes, 1 << 20).expect("many copies");
    assert_eq!(all.len(), 1 + 1000 * 258);
    assert_eq!(
        inflate(&w.bytes, 1000),
        Err(BbqrError::InflateTooBig),
        "stopped at the bound"
    );
}
