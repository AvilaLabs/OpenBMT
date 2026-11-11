#![no_main]

use libfuzzer_sys::fuzz_target;
use openbmt_qr::{encode_bytes, encode_text, encode_with, format_bits, Ecc, QrCode, QrError};

/// The 15 format bits read back from both copies in the matrix.
fn read_format(code: &QrCode) -> (u32, u32) {
    let n = code.size();
    let bit = |x: usize, y: usize, i: u32| u32::from(code.get(x, y)) << i;
    let mut first = 0;
    for i in 0..6 {
        first |= bit(8, i as usize, i);
    }
    first |= bit(8, 7, 6) | bit(8, 8, 7) | bit(7, 8, 8);
    for i in 9..15 {
        first |= bit(14 - i as usize, 8, i);
    }
    let mut second = 0;
    for i in 0..8 {
        second |= bit(n - 1 - i as usize, 8, i);
    }
    for i in 8..15 {
        second |= bit(8, n - 15 + i as usize, i);
    }
    (first, second)
}

fuzz_target!(|data: &[u8]| {
    let Some((&selector, rest)) = data.split_first() else {
        return;
    };
    let ecc = Ecc::ALL[usize::from(selector & 3)];
    // The input is capped: past version 40's capacity it is only refused,
    // and the fuzzer gains nothing from megabytes of it.
    let rest = &rest[..rest.len().min(4000)];
    let result = match (selector >> 2) & 3 {
        0 => encode_bytes(rest, ecc),
        1 => match std::str::from_utf8(rest) {
            Ok(text) => encode_text(text, ecc),
            Err(_) => return,
        },
        // Forced version and mask from the next two bytes, which may be
        // out of range.
        2 => {
            let Some((&v, tail)) = rest.split_first() else {
                return;
            };
            let Some((&m, tail)) = tail.split_first() else {
                return;
            };
            let version = (v & 0x80 == 0).then_some(v % 44);
            let mask = (m & 0x80 == 0).then_some(m % 10);
            match encode_with(tail, ecc, version, mask) {
                Err(QrError::BadVersion) => assert!(version.is_some_and(|v| !(1..=40).contains(&v))),
                Err(QrError::BadMask) => assert!(mask.is_some_and(|m| m > 7)),
                other => {
                    if let Ok(code) = &other {
                        assert!(version.is_none_or(|v| v == code.version()));
                        assert!(mask.is_none_or(|m| m == code.mask()));
                    }
                    return check(other, ecc);
                }
            }
            return;
        }
        _ => encode_bytes(rest, ecc),
    };
    check(result, ecc);
});

fn check(result: Result<QrCode, QrError>, ecc: Ecc) {
    let Ok(code) = result else {
        return;
    };
    assert_eq!(code.ecc(), ecc);
    assert_eq!(code.size(), 17 + 4 * usize::from(code.version()));
    // Every module can be read, and none outside the square is dark.
    let n = code.size();
    for y in 0..n + 2 {
        for x in 0..n + 2 {
            if x >= n || y >= n {
                assert!(!code.get(x, y));
            } else {
                let _ = code.get(x, y);
            }
        }
    }
    assert!(!code.get(usize::MAX, 0) && !code.get(0, usize::MAX));
    // The format bits in the matrix are those of the level and the mask.
    let want = u32::from(format_bits(code.ecc(), code.mask()));
    assert_eq!(read_format(&code), (want, want));
    // The dark module.
    assert!(code.get(8, n - 8));
}
