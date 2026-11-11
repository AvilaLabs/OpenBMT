//! Writes a camera-like frame of a code made by this repository's encoder,
//! for the JVM harness test of `apps/android-common`.
//!
//! ```text
//! cargo run --example make_frame -p openbmt-qr-jni -- OUT.bin TEXT [STRIDE_PAD]
//! ```
//!
//! OUT.bin is 12 bytes (width, height, stride as little-endian `u32`)
//! followed by the Y plane, rows `stride` bytes apart. The frame is 1280 by
//! 720 with the code at the centre; the padding after each row is mid-gray.

use openbmt_qr::{encode_text, Ecc};

fn main() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let out = args
        .next()
        .ok_or("usage: make_frame OUT TEXT [STRIDE_PAD]")?;
    let text = args
        .next()
        .ok_or("usage: make_frame OUT TEXT [STRIDE_PAD]")?;
    let pad: usize = match args.next() {
        Some(p) => p.parse().map_err(|_| "STRIDE_PAD is not a number")?,
        None => 0,
    };
    let code = encode_text(&text, Ecc::M).map_err(|e| e.to_string())?;
    let (width, height) = (1280usize, 720usize);
    let stride = width + pad;
    let n = code.size();
    let scale = (height - 80) / (n + 8);
    let side = (n + 8) * scale;
    let (ox, oy) = ((width - side) / 2, (height - side) / 2);
    let mut frame = vec![255u8; stride * height];
    for y in 0..height {
        for x in width..stride {
            if let Some(p) = frame.get_mut(y * stride + x) {
                *p = 128;
            }
        }
    }
    for y in 0..n {
        for x in 0..n {
            if !code.get(x, y) {
                continue;
            }
            for dy in 0..scale {
                for dx in 0..scale {
                    let (px, py) = (ox + (x + 4) * scale + dx, oy + (y + 4) * scale + dy);
                    if let Some(p) = frame.get_mut(py * stride + px) {
                        *p = 0;
                    }
                }
            }
        }
    }
    let mut bytes = Vec::with_capacity(12 + frame.len());
    for v in [width, height, stride] {
        bytes.extend_from_slice(&(v as u32).to_le_bytes());
    }
    bytes.extend_from_slice(&frame);
    std::fs::write(&out, bytes).map_err(|e| e.to_string())
}
