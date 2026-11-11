//! The QR reader of `openbmt-qr` and the QR drawing and UR sequences of
//! `openbmt-ur` for the Android apps, as two exported functions.
//!
//! The first takes the Y plane of a camera frame and answers with a framed
//! reply. Its reply framing:
//!
//! - byte 0 is [`DECODED`] (0), then the bytes of the code, or
//! - byte 0 is [`REFUSED`] (1), then the rule code of the reader's refusal
//!   as ASCII text, for example `QR-12` when the frame holds no code.
//!
//! [`reply`] is safe code and is what the tests run. The second takes one
//! framed request and answers with one framed reply: drawing a text as a QR
//! code, cutting a message into the parts of a UR, reading the parts of
//! a UR back, and reading a cosigner key out of a finished UR; see [`call`]. `src/jni.rs` holds the two exported symbols,
//! `Java_org_openbmt_common_QrNative_decode` and
//! `Java_org_openbmt_common_UrNative_call`, and is the only `unsafe` of the
//! crate.
//!
//! The reader keeps nothing between calls. The second call keeps one thing:
//! the table of at most four open UR decoders, each with the bounds of
//! `openbmt-ur`. There is no file, no network and no thread of its own. A
//! frame or a request is copied out of the Java array, used, and the copy is
//! zeroed before the call returns.

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

pub mod call;
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
mod call_tests;
#[cfg(test)]
mod tests;
