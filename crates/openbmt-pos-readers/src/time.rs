//! Times as the vendors write them (PS-92): RFC 3339 for Square, milliseconds
//! for Clover, both read to whole seconds since 1970 UTC.

/// The latest time read or written: 9999-12-31T23:59:59Z.
pub const MAX_TIME: u64 = 253_402_300_799;

/// The days from 1970-01-01 to a civil date in the proleptic Gregorian
/// calendar (days before 1970 are negative).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The civil date of a day count from 1970-01-01.
fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d)
}

fn is_leap(y: i64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

fn days_in_month(y: i64, m: i64) -> i64 {
    match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        _ if is_leap(y) => 29,
        _ => 28,
    }
}

/// `count` ASCII digits at `at` of `b`, as a number.
fn digits(b: &[u8], at: usize, count: usize) -> Option<i64> {
    let part = b.get(at..at.checked_add(count)?)?;
    let mut n = 0i64;
    for d in part {
        if !d.is_ascii_digit() {
            return None;
        }
        n = n * 10 + i64::from(d - b'0');
    }
    Some(n)
}

/// PS-92: an RFC 3339 time, read strictly, to whole seconds since 1970 UTC.
///
/// The form is `YYYY-MM-DDTHH:MM:SS`, then optionally a dot and one to nine
/// digits (dropped, not rounded), then `Z` or an offset `+HH:MM` or `-HH:MM`.
/// The `T` and the `Z` are capitals. The date must exist, the hour is 0 to
/// 23, the minute and the second 0 to 59 (a leap second is refused), and the
/// result must be from 1970 on and not after 9999-12-31T23:59:59Z. Any other
/// text gives `None`.
pub fn parse_rfc3339(text: &str) -> Option<u64> {
    let b = text.as_bytes();
    let year = digits(b, 0, 4)?;
    let month = digits(b, 5, 2)?;
    let day = digits(b, 8, 2)?;
    let hour = digits(b, 11, 2)?;
    let minute = digits(b, 14, 2)?;
    let second = digits(b, 17, 2)?;
    if b.get(4) != Some(&b'-')
        || b.get(7) != Some(&b'-')
        || b.get(10) != Some(&b'T')
        || b.get(13) != Some(&b':')
        || b.get(16) != Some(&b':')
    {
        return None;
    }
    let mut at = 19;
    if b.get(at) == Some(&b'.') {
        let mut n = 0;
        while b.get(at + 1 + n).is_some_and(u8::is_ascii_digit) {
            n += 1;
        }
        if n == 0 || n > 9 {
            return None;
        }
        at += 1 + n;
    }
    let offset = match b.get(at)? {
        b'Z' if b.len() == at + 1 => 0,
        sign @ (b'+' | b'-') if b.len() == at + 6 && b.get(at + 3) == Some(&b':') => {
            let h = digits(b, at + 1, 2)?;
            let m = digits(b, at + 4, 2)?;
            if h > 23 || m > 59 {
                return None;
            }
            let secs = h * 3_600 + m * 60;
            if *sign == b'+' {
                secs
            } else {
                -secs
            }
        }
        _ => return None,
    };
    if !(1..=12).contains(&month)
        || day < 1
        || day > days_in_month(year, month)
        || hour > 23
        || minute > 59
        || second > 59
        || year < 1970
    {
        return None;
    }
    let local = days_from_civil(year, month, day) * 86_400 + hour * 3_600 + minute * 60 + second;
    let utc = u64::try_from(local - offset).ok()?;
    (utc <= MAX_TIME).then_some(utc)
}

/// PS-95: seconds since 1970 as `YYYY-MM-DDTHH:MM:SSZ`; `None` after the
/// year 9999.
pub fn format_rfc3339(secs: u64) -> Option<String> {
    if secs > MAX_TIME {
        return None;
    }
    let days = i64::try_from(secs / 86_400).ok()?;
    let rest = secs % 86_400;
    let (y, m, d) = civil_from_days(days);
    Some(format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rest / 3_600,
        rest % 3_600 / 60,
        rest % 60
    ))
}

/// PS-92: milliseconds since 1970 to whole seconds (dropped, not rounded);
/// `None` after the year 9999.
pub fn millis_to_secs(ms: u64) -> Option<u64> {
    let secs = ms / 1_000;
    (secs <= MAX_TIME).then_some(secs)
}
