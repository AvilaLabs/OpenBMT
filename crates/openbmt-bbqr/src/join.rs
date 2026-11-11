//! Reading parts back (BQ-01 to BQ-10).

use crate::error::BbqrError;
use crate::{
    base32, inflate, Encoding, Kind, HEADER_CHARS, MAX_DECODED, MAX_PARTS, MAX_PART_CHARS,
};

/// How far a scan has come.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Progress {
    /// Distinct parts held.
    pub have: usize,
    /// Parts in the set.
    pub total: usize,
}

impl Progress {
    /// Whether every part is held.
    pub fn complete(&self) -> bool {
        self.have == self.total
    }
}

/// A file read back from its parts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decoded {
    /// What the header said the file is.
    pub kind: Kind,
    /// The file's bytes.
    pub bytes: Vec<u8>,
}

/// What the header of the first part fixed for the rest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Fixed {
    encoding: Encoding,
    kind: Kind,
    total: usize,
}

/// The parts of one set, held until all are in (BQ-10).
#[derive(Debug, Clone)]
pub struct Joiner {
    allowed: &'static [Kind],
    fixed: Option<Fixed>,
    parts: Vec<Option<String>>,
    have: usize,
    /// The length of a part that is not the last, once one is held.
    full_len: Option<usize>,
}

fn digit(c: char) -> Option<usize> {
    c.to_digit(36).and_then(|d| usize::try_from(d).ok())
}

impl Joiner {
    /// A joiner that takes files of these kinds (BQ-03) and no other.
    pub fn new(allowed: &'static [Kind]) -> Self {
        Self {
            allowed,
            fixed: None,
            parts: Vec::new(),
            have: 0,
            full_len: None,
        }
    }

    /// How far the set has come, once a part has been taken.
    pub fn progress(&self) -> Option<Progress> {
        self.fixed.map(|f| Progress {
            have: self.have,
            total: f.total,
        })
    }

    /// BQ-01 to BQ-09: take the text of one part. A part that is refused
    /// changes nothing (BQ-06). A repeat of a part held is accepted and
    /// not counted again. Letters may be in either case (BQ-01).
    pub fn add(&mut self, text: &str) -> Result<Progress, BbqrError> {
        if text.len() > MAX_PART_CHARS {
            return Err(BbqrError::Size("a part of more than 4296 characters"));
        }
        if !text.is_ascii() {
            return Err(BbqrError::Header("a character outside ASCII"));
        }
        let text = text.to_ascii_uppercase();
        let head: Vec<char> = text.chars().take(HEADER_CHARS).collect();
        let Ok([b, d, e, k, t1, t2, i1, i2]) = <[char; 8]>::try_from(head.as_slice()) else {
            return Err(BbqrError::Header("shorter than the eight-character header"));
        };
        if b != 'B' || d != '$' {
            return Err(BbqrError::Header("it does not start with B$"));
        }
        let encoding = Encoding::from_code(e).ok_or(BbqrError::Encoding)?;
        let kind = Kind::from_code(k).ok_or(BbqrError::Kind)?;
        if !self.allowed.contains(&kind) {
            return Err(BbqrError::Kind);
        }
        let digits = |a: char, b: char| Some(digit(a)? * 36 + digit(b)?);
        let (Some(total), Some(index)) = (digits(t1, t2), digits(i1, i2)) else {
            return Err(BbqrError::Header("a count or number that is not base 36"));
        };
        if total == 0 || total > MAX_PARTS || index >= total {
            return Err(BbqrError::Count);
        }
        let payload = text.get(HEADER_CHARS..).unwrap_or("");
        let fixed = Fixed {
            encoding,
            kind,
            total,
        };
        if self.fixed.is_some_and(|first| first != fixed) {
            return Err(BbqrError::Disagrees(
                "another encoding, file type or part count",
            ));
        }
        if payload.is_empty() {
            return Err(BbqrError::Size("a part with no data"));
        }
        let last = index + 1 == total;
        check_payload(encoding, payload, last)?;
        // BQ-07: every part but the last has one length, and the last is
        // no longer.
        let mut full_len = self.full_len;
        if !last {
            if full_len.is_some_and(|n| n != payload.len()) {
                return Err(BbqrError::Shape("parts of different lengths"));
            }
            full_len = Some(payload.len());
        }
        if let Some(n) = full_len {
            let last_len = if last {
                Some(payload.len())
            } else {
                self.parts
                    .get(total - 1)
                    .and_then(Option::as_ref)
                    .map(String::len)
            };
            if last_len.is_some_and(|l| l > n) {
                return Err(BbqrError::Shape("a last part longer than the others"));
            }
        }
        // BQ-06: a repeat must be the same data.
        let held = self.parts.get(index).and_then(Option::as_ref);
        if held.is_some_and(|old| old != payload) {
            return Err(BbqrError::Disagrees("the same part with other data"));
        }
        let is_new = held.is_none();
        // Nothing is refused from here on.
        if self.parts.is_empty() {
            self.parts = vec![None; total];
        }
        if is_new {
            if let Some(slot) = self.parts.get_mut(index) {
                *slot = Some(payload.to_string());
                self.have += 1;
            }
        }
        self.full_len = full_len;
        self.fixed = Some(fixed);
        Ok(Progress {
            have: self.have,
            total,
        })
    }

    /// BQ-10: the file, once every part is in. Nothing is consumed: a
    /// refusal leaves the parts as they were.
    pub fn finish(&self) -> Result<Decoded, BbqrError> {
        let fixed = self.fixed.ok_or(BbqrError::Missing { have: 0, total: 1 })?;
        if self.have != fixed.total {
            return Err(BbqrError::Missing {
                have: self.have,
                total: fixed.total,
            });
        }
        let mut joined: Vec<u8> = Vec::new();
        for part in &self.parts {
            let part = part.as_deref().ok_or(BbqrError::Missing {
                have: self.have,
                total: fixed.total,
            })?;
            let bytes = match fixed.encoding {
                Encoding::Hex => decode_hex(part)?,
                Encoding::Base32 | Encoding::Zlib => base32::decode(part)?,
            };
            if joined.len().saturating_add(bytes.len()) > MAX_DECODED {
                return Err(BbqrError::Size("more than 2 MiB"));
            }
            joined.extend_from_slice(&bytes);
        }
        let bytes = if fixed.encoding == Encoding::Zlib {
            inflate(&joined, MAX_DECODED)?
        } else {
            joined
        };
        if bytes.is_empty() {
            return Err(BbqrError::Size("a file with no bytes"));
        }
        Ok(Decoded {
            kind: fixed.kind,
            bytes,
        })
    }
}

/// BQ-07, BQ-08, BQ-09: the data characters of one part. Every part but the
/// last is whole groups of eight base32 characters (or any even number of
/// hex digits); the last one only has to be whole bytes.
fn check_payload(encoding: Encoding, payload: &str, last: bool) -> Result<(), BbqrError> {
    match encoding {
        Encoding::Hex => {
            if !payload.len().is_multiple_of(2) || !payload.bytes().all(|c| c.is_ascii_hexdigit()) {
                return Err(BbqrError::Hex);
            }
        }
        Encoding::Base32 | Encoding::Zlib => {
            if !last && !payload.len().is_multiple_of(8) {
                return Err(BbqrError::Shape(
                    "a part that is not a whole number of groups of eight",
                ));
            }
            base32::decode(payload)?;
        }
    }
    Ok(())
}

fn decode_hex(text: &str) -> Result<Vec<u8>, BbqrError> {
    let digits: Vec<u8> = text
        .bytes()
        .map(|c| {
            char::from(c)
                .to_digit(16)
                .and_then(|d| u8::try_from(d).ok())
        })
        .collect::<Option<_>>()
        .ok_or(BbqrError::Hex)?;
    if !digits.len().is_multiple_of(2) {
        return Err(BbqrError::Hex);
    }
    Ok(digits
        .chunks(2)
        .map(|p| match p {
            [h, l] => (h << 4) | l,
            _ => 0,
        })
        .collect())
}
