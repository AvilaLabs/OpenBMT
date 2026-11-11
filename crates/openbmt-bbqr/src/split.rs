//! Writing a file as parts (BQ-13).

use crate::error::BbqrError;
use crate::{base32, base36_pair, Encoding, Kind, HEADER_CHARS};
use crate::{MAX_DECODED, MAX_PARTS, MAX_PART_CHARS, MIN_PART_CHARS};

/// BQ-13: `data` of the given kind as the text of the parts of a BBQr set,
/// each of at most `max_part_chars` characters with its header, in base32
/// (encoding `2`).
///
/// Every part but the last has the same length, a whole number of groups of
/// eight characters; the last is no longer. The parts are spread as evenly
/// as that allows, and a file that fits one part is one part, with its
/// header. The text is upper case, for the QR alphanumeric mode.
pub fn encode(kind: Kind, data: &[u8], max_part_chars: usize) -> Result<Vec<String>, BbqrError> {
    if data.is_empty() || data.len() > MAX_DECODED {
        return Err(BbqrError::Size("a file of 1 byte to 2 MiB"));
    }
    if !(MIN_PART_CHARS..=MAX_PART_CHARS).contains(&max_part_chars) {
        return Err(BbqrError::Size("a part of 16 to 4296 characters"));
    }
    let text = base32::encode(data);
    let room = (max_part_chars - HEADER_CHARS) / 8 * 8;
    let wanted = text.len().div_ceil(room);
    // Spread the characters over `wanted` parts, each a whole number of
    // groups of eight.
    let each = text.len().div_ceil(wanted).div_ceil(8) * 8;
    let pieces: Vec<&str> = text
        .as_bytes()
        .chunks(each)
        .map(|c| std::str::from_utf8(c).unwrap_or(""))
        .collect();
    if pieces.len() > MAX_PARTS {
        return Err(BbqrError::Count);
    }
    let [t1, t2] = base36_pair(pieces.len()).ok_or(BbqrError::Count)?;
    let mut parts = Vec::with_capacity(pieces.len());
    for (n, piece) in pieces.iter().enumerate() {
        let [i1, i2] = base36_pair(n).ok_or(BbqrError::Count)?;
        parts.push(format!(
            "B${}{}{t1}{t2}{i1}{i2}{piece}",
            Encoding::Base32.code(),
            kind.code()
        ));
    }
    Ok(parts)
}
