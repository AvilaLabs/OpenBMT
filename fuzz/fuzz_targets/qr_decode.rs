#![no_main]

use libfuzzer_sys::fuzz_target;
use openbmt_qr::{
    decode_luma, decode_luma_strided, encode_bytes, DecodeError, Ecc, MAX_SIDE, MIN_SIDE,
};

/// What every result must satisfy, whatever the frame.
fn check(r: Result<openbmt_qr::Decoded, DecodeError>) {
    if let Ok(d) = r {
        assert!((1..=40).contains(&d.version));
        assert!(d.mask < 8);
        assert!(d.bytes.len() <= 7089 * 2);
    }
}

fuzz_target!(|data: &[u8]| {
    let Some((head, rest)) = data.split_at_checked(4) else {
        return;
    };
    let (a, b, c, d) = (head[0], head[1], head[2], head[3]);
    match a & 3 {
        // Raw pixels of a small frame, with or without row padding.
        0 | 1 => {
            if rest.is_empty() {
                return;
            }
            let w = MIN_SIDE + usize::from(b) % 113;
            let h = MIN_SIDE + usize::from(c) % 113;
            let stride = if a & 1 == 1 { w + usize::from(d) % 9 } else { w };
            let frame: Vec<u8> = rest.iter().copied().cycle().take(stride * h).collect();
            check(decode_luma_strided(w, h, stride, &frame));
            if stride == w {
                check(decode_luma(w, h, &frame));
            }
        }
        // A real code, drawn and then damaged by the rest of the input.
        2 => {
            let ecc = Ecc::ALL[usize::from(b & 3)];
            let split = rest.len().min(usize::from(c) % 48);
            let (content, noise) = rest.split_at(split);
            let Ok(code) = encode_bytes(content, ecc) else {
                return;
            };
            let scale = 2 + usize::from(d % 3);
            let n = code.size();
            let side = (n + 8) * scale;
            let mut frame = vec![255u8; side * side];
            for y in 0..n {
                for x in 0..n {
                    if code.get(x, y) {
                        for dy in 0..scale {
                            for dx in 0..scale {
                                frame[((y + 4) * scale + dy) * side + (x + 4) * scale + dx] = 0;
                            }
                        }
                    }
                }
            }
            if noise.is_empty() {
                // An undamaged drawing must read back as it was written.
                let got = decode_luma(side, side, &frame).expect("a clean code reads");
                assert_eq!(got.bytes, content);
                assert_eq!((got.version, got.ecc), (code.version(), code.ecc()));
                return;
            }
            for chunk in noise.chunks_exact(3) {
                let at = (usize::from(chunk[0]) << 8 | usize::from(chunk[1])) % frame.len();
                frame[at] = chunk[2];
            }
            check(decode_luma(side, side, &frame));
        }
        // Any sides, including ones out of range, over a buffer of any length.
        _ => {
            let w = usize::from(b) * 20 + usize::from(c % 20);
            let h = usize::from(d) * 20;
            let r = decode_luma(w, h, rest);
            if !(MIN_SIDE..=MAX_SIDE).contains(&w) || !(MIN_SIDE..=MAX_SIDE).contains(&h) {
                assert!(matches!(
                    r,
                    Err(DecodeError::BadFrame | DecodeError::TooLarge)
                ));
            }
            check(r);
        }
    }
});
