use openbmt_qr::{encode_bytes, encode_text, Ecc};

use crate::{reply, DECODED, MAX_FRAME, REFUSED};

/// Draws a code at `scale` pixels a module with a quiet zone of four, into a
/// buffer whose rows are `stride` bytes apart (the padding is mid-gray).
fn draw(code: &openbmt_qr::QrCode, scale: usize, stride: usize) -> (usize, Vec<u8>) {
    let n = code.size();
    let side = (n + 8) * scale;
    assert!(stride >= side);
    let mut px = vec![128u8; stride * side];
    for y in 0..side {
        for x in 0..side {
            let (mx, my) = (x / scale, y / scale);
            let dark =
                (4..n + 4).contains(&mx) && (4..n + 4).contains(&my) && code.get(mx - 4, my - 4);
            px[y * stride + x] = if dark { 0 } else { 255 };
        }
    }
    (side, px)
}

fn refusal(code: &str) -> Vec<u8> {
    [&[REFUSED][..], code.as_bytes()].concat()
}

#[test]
fn qr_jni_decoded_reply_is_zero_then_the_bytes() {
    let code = encode_bytes(b"openbmt \x00\xff bytes", Ecc::M).expect("fits");
    let (side, px) = draw(&code, 4, (code.size() + 8) * 4);
    let out = reply(side as i32, side as i32, side as i32, &px);
    assert_eq!(out[0], DECODED);
    assert_eq!(&out[1..], b"openbmt \x00\xff bytes");
}

#[test]
fn qr_jni_a_stride_wider_than_the_width_is_read() {
    let code = encode_text("BITCOIN:BC1QXYZ", Ecc::M).expect("fits");
    let width = (code.size() + 8) * 4;
    let (side, px) = draw(&code, 4, width + 24);
    let out = reply(side as i32, side as i32, (width + 24) as i32, &px);
    assert_eq!(out[0], DECODED);
    assert_eq!(&out[1..], b"BITCOIN:BC1QXYZ");
}

#[test]
fn qr_jni_a_frame_without_a_code_is_refused_with_qr_12() {
    let out = reply(64, 64, 64, &vec![200u8; 64 * 64]);
    assert_eq!(out, refusal("QR-12"));
}

#[test]
fn qr_jni_a_frame_that_does_not_fit_is_refused_with_qr_10() {
    assert_eq!(reply(64, 64, 64, &[0u8; 100]), refusal("QR-10"));
    assert_eq!(reply(8, 8, 8, &[0u8; 64]), refusal("QR-10"));
    // A stride under the width.
    assert_eq!(reply(64, 64, 32, &vec![0u8; 64 * 64]), refusal("QR-10"));
}

#[test]
fn qr_jni_negative_sizes_are_refused_with_qr_10() {
    let px = vec![0u8; 64 * 64];
    assert_eq!(reply(-1, 64, 64, &px), refusal("QR-10"));
    assert_eq!(reply(64, -1, 64, &px), refusal("QR-10"));
    assert_eq!(reply(64, 64, -64, &px), refusal("QR-10"));
    assert_eq!(reply(i32::MIN, i32::MIN, i32::MIN, &px), refusal("QR-10"));
}

#[test]
fn qr_jni_sides_over_the_reader_limit_are_refused_with_qr_11() {
    // 4097 wide: the reader refuses from the size, before it reads a pixel.
    let px = vec![0u8; 4097 * 16];
    assert_eq!(reply(4097, 16, 4097, &px), refusal("QR-11"));
    assert_eq!(reply(i32::MAX, 16, i32::MAX, &px), refusal("QR-11"));
}

#[test]
fn qr_jni_a_buffer_over_the_limit_is_refused_before_it_is_read() {
    let px = vec![0u8; MAX_FRAME + 1];
    assert_eq!(reply(1280, 720, 1280, &px), refusal("QR-10"));
}

#[test]
fn qr_jni_the_rule_code_has_no_trailing_text() {
    for (w, h, s, len) in [
        (64, 64, 64, 4096),
        (8, 8, 8, 64),
        (4097, 16, 4097, 4097 * 16),
    ] {
        let out = reply(w, h, s, &vec![255u8; len]);
        assert_eq!(out[0], REFUSED);
        let code = std::str::from_utf8(&out[1..]).expect("ascii");
        assert!(code.starts_with("QR-") && code.len() == 5, "{code}");
    }
}
