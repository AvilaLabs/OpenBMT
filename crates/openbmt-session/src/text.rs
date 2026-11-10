//! Short texts (AS-07, AS-12) and the small helpers the encoders share.

use openbmt_core::wire::{DecodeError, Reader};

use crate::error::EncodeError;
use crate::{MAX_LABEL_BYTES, MAX_LABEL_CHARS, MAX_REASON_BYTES};

/// AS-07: 1 to 64 characters, at most 255 bytes, no control character.
pub fn label_text_ok(s: &str) -> bool {
    let chars = s.chars().count();
    (1..=MAX_LABEL_CHARS).contains(&chars)
        && s.len() <= MAX_LABEL_BYTES
        && !s.chars().any(char::is_control)
}

/// AS-12: 1 to 160 bytes, no control character.
pub fn reason_text_ok(s: &str) -> bool {
    (1..=MAX_REASON_BYTES).contains(&s.len()) && !s.chars().any(char::is_control)
}

/// A length as the u32 of the wire form.
pub(crate) fn put_len(out: &mut Vec<u8>, len: usize) -> Result<(), EncodeError> {
    let n = u32::try_from(len).map_err(|_| EncodeError::TooLarge)?;
    out.extend_from_slice(&n.to_le_bytes());
    Ok(())
}

/// A label text: one length byte, then the bytes.
pub(crate) fn put_label_text(out: &mut Vec<u8>, s: &str) -> Result<(), EncodeError> {
    if !label_text_ok(s) {
        return Err(EncodeError::BadText("label text"));
    }
    let n = u8::try_from(s.len()).map_err(|_| EncodeError::BadText("label text"))?;
    out.push(n);
    out.extend_from_slice(s.as_bytes());
    Ok(())
}

/// A decline text: one length byte, then the bytes.
pub(crate) fn put_reason_text(out: &mut Vec<u8>, s: &str) -> Result<(), EncodeError> {
    if !reason_text_ok(s) {
        return Err(EncodeError::BadText("decline text"));
    }
    let n = u8::try_from(s.len()).map_err(|_| EncodeError::BadText("decline text"))?;
    out.push(n);
    out.extend_from_slice(s.as_bytes());
    Ok(())
}

fn read_utf8(r: &mut Reader<'_>) -> Result<String, DecodeError> {
    let n = usize::from(r.u8()?);
    let bytes = r.take(n)?;
    std::str::from_utf8(bytes)
        .map(str::to_owned)
        .map_err(|_| DecodeError::NonCanonical("text is not UTF-8"))
}

pub(crate) fn read_label_text(r: &mut Reader<'_>) -> Result<String, DecodeError> {
    let s = read_utf8(r)?;
    if label_text_ok(&s) {
        Ok(s)
    } else {
        Err(DecodeError::NonCanonical("label text breaks the text rule"))
    }
}

pub(crate) fn read_reason_text(r: &mut Reader<'_>) -> Result<String, DecodeError> {
    let s = read_utf8(r)?;
    if reason_text_ok(&s) {
        Ok(s)
    } else {
        Err(DecodeError::NonCanonical(
            "decline text breaks the text rule",
        ))
    }
}
