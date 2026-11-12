//! Days of the shop's calendar (RQ-09, RQ-17, RQ-21, RQ-24).
//!
//! A day is a count of days since 1970-01-01 in the shop's time zone. The
//! arithmetic between a day and a year, month and day is written out here
//! (no date library is a dependency of the programs): the algorithm is the
//! one that counts days in eras of 400 years, which starts every year on
//! 1 March so that a leap day is the last day of its year.

use std::fmt;

/// A day of the shop's calendar: days since 1970-01-01.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Day(pub u32);

/// Seconds in a day.
pub const DAY_SECS: u64 = 86_400;

/// The last year a day may be in.
pub const MAX_YEAR: i64 = 2200;

/// The last day a request may name: 31 December of [`MAX_YEAR`].
pub const MAX_DAY: Day = Day(days_from_civil(MAX_YEAR, 12, 31) as u32);

/// The days from 1970-01-01 to `year`-`month`-`day` of the proleptic
/// Gregorian calendar; negative before 1970. The date is not checked.
pub const fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let mp = (month as i64 + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The year, month (1 to 12) and day of the month of a count of days since
/// 1970-01-01: the inverse of [`days_from_civil`].
pub const fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = yoe + era * 400 + if month <= 2 { 1 } else { 0 };
    (year, month, day)
}

/// Whether `year` has a 29 February.
pub const fn is_leap(year: i64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

/// The days of a month (1 to 12) of a year.
pub const fn days_in_month(year: i64, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        _ => {
            if is_leap(year) {
                29
            } else {
                28
            }
        }
    }
}

impl Day {
    /// The day of a date; `None` for a date that does not exist, is before
    /// 1970 or is after [`MAX_YEAR`].
    pub fn from_civil(year: i64, month: u32, day: u32) -> Option<Self> {
        if !(1970..=MAX_YEAR).contains(&year)
            || !(1..=12).contains(&month)
            || day < 1
            || day > days_in_month(year, month)
        {
            return None;
        }
        u32::try_from(days_from_civil(year, month, day))
            .ok()
            .map(Day)
    }

    /// The year, month and day of the month.
    pub fn civil(self) -> (i64, u32, u32) {
        civil_from_days(i64::from(self.0))
    }

    /// `n` days later; `None` past [`MAX_DAY`].
    pub fn plus_days(self, n: u32) -> Option<Self> {
        let day = self.0.checked_add(n)?;
        (day <= MAX_DAY.0).then_some(Day(day))
    }

    /// The day of the week, 0 for Monday to 6 for Sunday (1970-01-01 was a
    /// Thursday).
    pub fn weekday(self) -> u32 {
        (self.0 + 3) % 7
    }

    /// The day `months` months on with the same day of the month, or the
    /// last day of the month when that month is shorter (RQ-24). The day
    /// of the month is the one of `self`, so a series that starts on the
    /// 31st keeps the 31st wherever the month has one.
    pub fn plus_months(self, months: u32) -> Option<Self> {
        let (year, month, day) = self.civil();
        let total = year * 12 + i64::from(month) - 1 + i64::from(months);
        let (year, month) = (total.div_euclid(12), (total.rem_euclid(12) + 1) as u32);
        Self::from_civil(year, month, day.min(days_in_month(year, month)))
    }

    /// The day `years` years on; 29 February becomes 28 February in a year
    /// that has no 29th.
    pub fn plus_years(self, years: u32) -> Option<Self> {
        self.plus_months(years.checked_mul(12)?)
    }
}

impl fmt::Display for Day {
    /// `2027-03-01`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (y, m, d) = self.civil();
        write!(f, "{y:04}-{m:02}-{d:02}")
    }
}

/// The day of a time, in the shop's time zone (`tz_offset_secs` east of
/// UTC). Before 1970 in that zone it is day 0.
pub fn day_of(unix_secs: u64, tz_offset_secs: i32) -> Day {
    let local = i128::from(unix_secs) + i128::from(tz_offset_secs);
    let days = local.div_euclid(i128::from(DAY_SECS));
    Day(u32::try_from(days.clamp(0, i128::from(MAX_DAY.0))).unwrap_or(0))
}

/// The first second after `day` in the shop's time zone, as Unix seconds
/// (0 if that is before 1970). A time at or after it is on a later day.
pub fn end_of_day(day: Day, tz_offset_secs: i32) -> u64 {
    let local = (i128::from(day.0) + 1) * i128::from(DAY_SECS);
    u64::try_from((local - i128::from(tz_offset_secs)).max(0)).unwrap_or(u64::MAX)
}
