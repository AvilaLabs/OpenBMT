//! The QR reader of `openbmt-qr` for the Android apps: one function that
//! takes the Y plane of a camera frame and answers with a framed reply.
//!
//! Reply framing, the same for every call:
//!
//! - byte 0 is [`DECODED`] (0), then the bytes of the code, or
//! - byte 0 is [`REFUSED`] (1), then the rule code of the reader's refusal
//!   as ASCII text, for example `QR-12` when the frame holds no code.
//!
//! [`reply`] is safe code and is what the tests run. `src/jni.rs` is the
//! one exported symbol, `Java_org_openbmt_common_QrNative_decode`, and the
//! only `unsafe` of the crate.
//!
//! The library keeps nothing between calls: no global state, no file, no
//! network, no thread of its own. The frame is copied out of the Java
//! array, read, and the copy is zeroed before the call returns.

// The one `unsafe` of this crate is the JNI glue in `jni.rs`.
#![deny(unsafe_code)]
#![deny(missing_docs)]
#![cfg_attr(
    not(test),
    deny(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing
    )
)]

mod jni;

use openbmt_qr::decode_luma_strided;

/// The first byte of a reply that holds the bytes of a code.
pub const DECODED: u8 = 0;
/// The first byte of a reply that holds a rule code.
pub const REFUSED: u8 = 1;

/// The largest frame buffer read, in bytes. The reader's own limits (a
/// side of at most 4096 and a stride of at most a million) allow more;
/// no camera frame of a phone comes near this.
pub const MAX_FRAME: usize = 32 * 1024 * 1024;

/// The rule code for a frame the call itself cannot take: a negative
/// size, a missing array or a buffer over [`MAX_FRAME`]. It is the
/// reader's rule for a frame that does not fit.
const BAD_FRAME: &str = "QR-10";

fn refused(code: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(1 + code.len());
    out.push(REFUSED);
    out.extend_from_slice(code.as_bytes());
    out
}

/// The rule code at the start of a refusal's text (`QR-12 no code ...`).
fn code_of(text: &str) -> &str {
    text.split_whitespace().next().unwrap_or(BAD_FRAME)
}

/// Read the frame and frame the answer. Sizes are the JNI `int`s, so a
/// negative one is refused as a frame that does not fit.
pub fn reply(width: i32, height: i32, stride: i32, luma: &[u8]) -> Vec<u8> {
    let (Ok(w), Ok(h), Ok(s)) = (
        usize::try_from(width),
        usize::try_from(height),
        usize::try_from(stride),
    ) else {
        return refused(BAD_FRAME);
    };
    if luma.len() > MAX_FRAME {
        return refused(BAD_FRAME);
    }
    match decode_luma_strided(w, h, s, luma) {
        Ok(decoded) => {
            let mut out = Vec::with_capacity(1 + decoded.bytes.len());
            out.push(DECODED);
            out.extend_from_slice(&decoded.bytes);
            out
        }
        Err(e) => refused(code_of(&e.to_string())),
    }
}

/// The refusal an exported call gives when it cannot take its arguments
/// (a null array, a read that failed): the same rule as a frame that does
/// not fit.
pub(crate) fn bad_call() -> Vec<u8> {
    refused(BAD_FRAME)
}

#[cfg(test)]
mod tests;
