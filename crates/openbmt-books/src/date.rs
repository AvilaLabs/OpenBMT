//! Civil dates from Unix seconds, by the days-from-civil method (BK-16).

use std::fmt;

/// The last second this library takes as a time: 9999-12-31T23:59:59Z.
pub const MAX_UNIX: u64 = 253_402_300_799;
/// The lowest shop offset from UTC, in minutes.
pub const MIN_OFFSET_MINUTES: i32 = -720;
/// The highest shop offset from UTC, in minutes.
pub const MAX_OFFSET_MINUTES: i32 = 840;

const SECS_PER_DAY: i64 = 86_400;

/// BK-16: the civil date of a day count since 1970-01-01 (may be negative).
pub fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    // `d` is 1..=31 and `m` is 1..=12 by construction.
    (
        y,
        u32::try_from(m).unwrap_or(1),
        u32::try_from(d).unwrap_or(1),
    )
}

/// BK-16: the day count since 1970-01-01 of a civil date.
pub fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let y = year - i64::from(month <= 2);
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let m = i64::from(month);
    let mp = if m > 2 { m - 3 } else { m + 9 };
    let doy = (153 * mp + 2) / 5 + i64::from(day) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// A civil date.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Date {
    /// The year.
    pub year: i32,
    /// The month, 1 to 12.
    pub month: u8,
    /// The day, 1 to 31.
    pub day: u8,
}

/// A calendar month.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct YearMonth {
    /// The year.
    pub year: i32,
    /// The month, 1 to 12.
    pub month: u8,
}

impl fmt::Display for Date {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }
}

impl fmt::Display for YearMonth {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:04}-{:02}", self.year, self.month)
    }
}

impl Date {
    /// The UTC date of Unix seconds; `None` after [`MAX_UNIX`].
    pub fn from_unix(secs: u64) -> Option<Self> {
        Self::from_unix_offset(secs, 0)
    }

    /// The date at a fixed offset from UTC, in minutes east; `None` after
    /// [`MAX_UNIX`] or for an offset outside [`MIN_OFFSET_MINUTES`] to
    /// [`MAX_OFFSET_MINUTES`].
    pub fn from_unix_offset(secs: u64, offset_minutes: i32) -> Option<Self> {
        if secs > MAX_UNIX || !(MIN_OFFSET_MINUTES..=MAX_OFFSET_MINUTES).contains(&offset_minutes) {
            return None;
        }
        let local = i64::try_from(secs).ok()? + i64::from(offset_minutes) * 60;
        let (y, m, d) = civil_from_days(local.div_euclid(SECS_PER_DAY));
        Some(Self {
            year: i32::try_from(y).ok()?,
            month: u8::try_from(m).ok()?,
            day: u8::try_from(d).ok()?,
        })
    }

    /// The month this date is in.
    pub fn year_month(&self) -> YearMonth {
        YearMonth {
            year: self.year,
            month: self.month,
        }
    }
}

/// `YYYY-MM-DDTHH:MM:SSZ` for Unix seconds; `None` after [`MAX_UNIX`].
pub fn iso_time(secs: u64) -> Option<String> {
    let date = Date::from_unix(secs)?;
    let in_day = secs % 86_400;
    Some(format!(
        "{date}T{:02}:{:02}:{:02}Z",
        in_day / 3_600,
        in_day % 3_600 / 60,
        in_day % 60
    ))
}
