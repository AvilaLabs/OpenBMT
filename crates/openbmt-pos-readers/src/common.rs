//! What both readers share: the strict reading of fields, money, times,
//! currencies, ids and text, and the cursor (PS-90 to PS-94).

use openbmt_counter::Currency;
use openbmt_json::{JsonError, Limits, Value};
use openbmt_pos::{Cursor, Id, ModelError, Text, MAX_CURSOR_BYTES, MAX_TEXT_BYTES};

use crate::error::{ReadRefused, RequestRefused};
use crate::time::{millis_to_secs, parse_rfc3339, MAX_TIME};

/// PS-90: a reply, read within the default bounds of `openbmt-json`.
pub(crate) fn parse_doc(bytes: &[u8]) -> Result<Value, ReadRefused> {
    openbmt_json::parse(bytes, &Limits::DEFAULT).map_err(|e: JsonError| e.into())
}

/// PS-91: the value is an object.
pub(crate) fn object<'a>(v: &'a Value, what: &'static str) -> Result<&'a Value, ReadRefused> {
    match v {
        Value::Obj(_) => Ok(v),
        _ => Err(ReadRefused::WrongType(what)),
    }
}

/// PS-91: a field that is there and is not `null`.
pub(crate) fn opt<'a>(v: &'a Value, key: &str) -> Option<&'a Value> {
    v.get(key).filter(|x| !x.is_null())
}

/// PS-91: a field the reader needs.
pub(crate) fn req<'a>(
    v: &'a Value,
    key: &str,
    what: &'static str,
) -> Result<&'a Value, ReadRefused> {
    opt(v, key).ok_or(ReadRefused::Missing(what))
}

/// PS-91: a string.
pub(crate) fn string<'a>(v: &'a Value, what: &'static str) -> Result<&'a str, ReadRefused> {
    v.as_str().ok_or(ReadRefused::WrongType(what))
}

/// PS-91: an array.
pub(crate) fn array<'a>(v: &'a Value, what: &'static str) -> Result<&'a [Value], ReadRefused> {
    v.as_array().ok_or(ReadRefused::WrongType(what))
}

/// PS-92: a whole number from 0 to `i64::MAX`. A negative one or one above
/// is out of range; a number with a fraction or an exponent, or anything
/// that is not a number, is of the wrong type.
pub(crate) fn amount(v: &Value, what: &'static str) -> Result<u64, ReadRefused> {
    match v {
        Value::Int(i) => u64::try_from(*i).map_err(|_| ReadRefused::OutOfRange(what)),
        Value::UInt(_) => Err(ReadRefused::OutOfRange(what)),
        _ => Err(ReadRefused::WrongType(what)),
    }
}

/// PS-93: a currency of three capital letters.
pub(crate) fn currency(s: &str, what: &'static str) -> Result<Currency, ReadRefused> {
    let code: [u8; 3] = s
        .as_bytes()
        .try_into()
        .map_err(|_| ReadRefused::BadCurrency(what))?;
    Currency::new(code).ok_or(ReadRefused::BadCurrency(what))
}

/// PS-92: an RFC 3339 time in a string field.
pub(crate) fn rfc3339(v: &Value, what: &'static str) -> Result<u64, ReadRefused> {
    parse_rfc3339(string(v, what)?).ok_or(ReadRefused::BadTime(what))
}

/// PS-92: a time in milliseconds, in whole seconds.
pub(crate) fn millis(v: &Value, what: &'static str) -> Result<u64, ReadRefused> {
    millis_to_secs(amount(v, what)?).ok_or(ReadRefused::BadTime(what))
}

/// PS-93: an id, refused rather than cut.
pub(crate) fn id(s: &str, what: &'static str) -> Result<Id, ReadRefused> {
    Id::new(s).map_err(|_| ReadRefused::BadId(what))
}

/// PS-93: a text that is refused rather than cut (a location).
pub(crate) fn whole_text(s: &str, what: &'static str) -> Result<Text, ReadRefused> {
    Text::new(s).map_err(|_| ReadRefused::BadText(what))
}

/// `s` cut to at most 64 bytes at a character boundary.
pub(crate) fn cut(s: &str) -> &str {
    let mut n = s.len().min(MAX_TEXT_BYTES);
    while !s.is_char_boundary(n) {
        n -= 1;
    }
    s.get(..n).unwrap_or("")
}

/// PS-93: a name (a tender's), cut to 64 bytes at a character boundary and
/// then refused if it is empty or holds a control character (PS-03).
pub(crate) fn name(s: &str, what: &'static str) -> Result<Text, ReadRefused> {
    Text::new(cut(s)).map_err(|_| ReadRefused::BadText(what))
}

/// PS-93: a note typed by a person: each control character becomes a space,
/// the ends are trimmed, it is cut to 64 bytes at a character boundary, and
/// nothing left is no note.
pub(crate) fn note(s: &str) -> Option<Text> {
    let spaced: String = s
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let trimmed = cut(spaced.trim_matches(' ')).trim_matches(' ');
    Text::new(trimmed).ok()
}

/// PS-10: whether `candidate` equals `name` without regard to ASCII case and
/// surrounding spaces, as the settings compare.
pub(crate) fn same_name(candidate: &str, name: &Text) -> bool {
    candidate
        .trim_ascii()
        .eq_ignore_ascii_case(name.as_str().trim_ascii())
}

/// PS-94: a cursor is the time in whole seconds from which orders are read
/// (decimal, no leading zero), then, when a page has more, a bar and the
/// vendor's own marker (1 or more printable ASCII characters, no space).
pub(crate) fn split_cursor(c: &Cursor) -> Result<(u64, Option<String>), RequestRefused> {
    let text = std::str::from_utf8(c.as_bytes()).map_err(|_| RequestRefused::BadCursor)?;
    if text.is_empty() {
        return Ok((0, None));
    }
    let (time, vendor) = match text.split_once('|') {
        Some((t, v)) => (t, Some(v)),
        None => (text, None),
    };
    let canonical = !time.is_empty()
        && time.bytes().all(|b| b.is_ascii_digit())
        && (time == "0" || !time.starts_with('0'));
    let since: u64 = time.parse().map_err(|_| RequestRefused::BadCursor)?;
    if !canonical || since > MAX_TIME {
        return Err(RequestRefused::BadCursor);
    }
    if let Some(v) = vendor {
        if v.is_empty() || !v.bytes().all(|b| (0x21..=0x7e).contains(&b)) {
            return Err(RequestRefused::BadCursor);
        }
    }
    Ok((since, vendor.map(str::to_string)))
}

/// PS-94: the cursor for a time and, if there is one, the vendor's marker.
/// Refused above 256 bytes.
pub(crate) fn join_cursor(since: u64, vendor: Option<&str>) -> Result<Cursor, ReadRefused> {
    let text = match vendor {
        Some(v) => format!("{since}|{v}"),
        None => since.to_string(),
    };
    if text.len() > MAX_CURSOR_BYTES {
        return Err(ReadRefused::CursorTooLong);
    }
    Cursor::new(text.into_bytes()).map_err(|e: ModelError| e.into())
}

/// PS-94: the cursor that starts reading at a time, in whole seconds since
/// 1970. Refused after the year 9999.
pub fn cursor_at(since: u64) -> Result<Cursor, RequestRefused> {
    if since > MAX_TIME {
        return Err(RequestRefused::BadCursor);
    }
    join_cursor(since, None).map_err(|_| RequestRefused::BadCursor)
}

/// PS-94: a page size of 1 to 500.
pub(crate) fn check_limit(limit: usize) -> Result<(), RequestRefused> {
    if (1..=openbmt_pos::MAX_PAGE_ORDERS).contains(&limit) {
        Ok(())
    } else {
        Err(RequestRefused::BadLimit)
    }
}

/// The largest of the times of a page and the time it was read from.
pub(crate) fn newest(since: u64, times: impl Iterator<Item = u64>) -> u64 {
    times.fold(since, u64::max)
}
