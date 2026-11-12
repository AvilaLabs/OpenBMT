//! The calendar, against dates worked out by hand: RQ-09, RQ-17, RQ-24.

mod common;

use common::*;
use openbmt_requests::date::{civil_from_days, days_from_civil, days_in_month, is_leap, MAX_DAY};
use openbmt_requests::{day_of, end_of_day, Day};

#[test]
fn rq_09_days_from_civil_known_answers() {
    assert_eq!(days_from_civil(1970, 1, 1), 0);
    assert_eq!(days_from_civil(1969, 12, 31), -1);
    assert_eq!(days_from_civil(2000, 1, 1), 10_957);
    assert_eq!(days_from_civil(2000, 2, 29), 11_016);
    assert_eq!(days_from_civil(2000, 3, 1), 11_017);
    assert_eq!(days_from_civil(2024, 2, 29), 19_782);
    assert_eq!(days_from_civil(2024, 12, 31), 20_088);
    assert_eq!(days_from_civil(2027, 1, 1), 20_819);
    assert_eq!(days_from_civil(2027, 1, 15), 20_833);
    assert_eq!(days_from_civil(2100, 3, 1), 47_541);
    assert_eq!(MAX_DAY.civil(), (2200, 12, 31));
}

#[test]
fn rq_09_civil_from_days_is_the_inverse_for_every_day_of_the_range() {
    for d in 0..=MAX_DAY.0 {
        let (y, m, dd) = civil_from_days(i64::from(d));
        assert_eq!(days_from_civil(y, m, dd), i64::from(d), "{y}-{m}-{dd}");
        assert!((1..=days_in_month(y, m)).contains(&dd));
    }
    assert_eq!(civil_from_days(-1), (1969, 12, 31));
    assert_eq!(civil_from_days(11_016), (2000, 2, 29));
    assert_eq!(civil_from_days(19_782), (2024, 2, 29));
}

#[test]
fn rq_09_leap_years_follow_the_rule_of_four_hundred() {
    for (y, leap) in [
        (1900, false),
        (2000, true),
        (2024, true),
        (2026, false),
        (2100, false),
        (2200, false),
    ] {
        assert_eq!(is_leap(y), leap, "{y}");
    }
    assert_eq!(days_in_month(2024, 2), 29);
    assert_eq!(days_in_month(2100, 2), 28);
    assert_eq!(days_in_month(2027, 4), 30);
    assert_eq!(days_in_month(2027, 12), 31);
}

#[test]
fn rq_09_only_real_dates_are_days() {
    assert!(Day::from_civil(2027, 2, 29).is_none());
    assert!(Day::from_civil(2028, 2, 29).is_some());
    assert!(Day::from_civil(2027, 4, 31).is_none());
    assert!(Day::from_civil(2027, 13, 1).is_none());
    assert!(Day::from_civil(2027, 0, 1).is_none());
    assert!(Day::from_civil(2027, 1, 0).is_none());
    assert!(Day::from_civil(1969, 12, 31).is_none());
    assert!(Day::from_civil(2201, 1, 1).is_none());
    assert_eq!(Day::from_civil(2200, 12, 31), Some(MAX_DAY));
    assert!(MAX_DAY.plus_days(1).is_none());
}

#[test]
fn rq_09_the_weekday_and_the_text_of_a_day() {
    // 1970-01-01 was a Thursday; 2027-01-15 is a Friday; 2024-02-29 a Thursday.
    assert_eq!(Day(0).weekday(), 3);
    assert_eq!(today().weekday(), 4);
    assert_eq!(day(2024, 2, 29).weekday(), 3);
    assert_eq!(day(2024, 2, 29).to_string(), "2024-02-29");
    assert_eq!(day(2027, 1, 5).to_string(), "2027-01-05");
}

#[test]
fn rq_09_a_day_in_the_shops_time_zone() {
    assert_eq!(day_of(T0, 0), today());
    // 08:00 UTC is 03:00 in New York in winter and 17:00 in Tokyo.
    assert_eq!(day_of(T0, -5 * 3600), today());
    assert_eq!(day_of(T0, 9 * 3600), today());
    // 23:30 UTC on the 15th is the 16th east of UTC+1.
    let late = T0 - 8 * 3600 + 23 * 3600 + 1800;
    assert_eq!(day_of(late, 0), today());
    assert_eq!(day_of(late, 3600), in_days(1));
    // 00:30 UTC on the 16th is still the 15th at UTC-1.
    let early = T0 - 8 * 3600 + DAY + 1800;
    assert_eq!(day_of(early, 0), in_days(1));
    assert_eq!(day_of(early, -3600), today());
    // Before 1970 in the zone is day 0.
    assert_eq!(day_of(0, -3600), Day(0));
}

#[test]
fn rq_17_the_end_of_a_day_is_the_first_second_of_the_next() {
    // The 15th ends at 2027-01-16 00:00 UTC.
    let end = T0 - 8 * 3600 + DAY;
    assert_eq!(end_of_day(today(), 0), end);
    // In UTC+2 the day ends two hours earlier by the world's clocks.
    assert_eq!(end_of_day(today(), 7200), end - 7200);
    assert_eq!(end_of_day(today(), -7200), end + 7200);
    assert_eq!(day_of(end - 1, 0), today());
    assert_eq!(day_of(end, 0), in_days(1));
    assert_eq!(end_of_day(Day(0), 3600 * 5), 68_400);
    assert_eq!(end_of_day(Day(0), 100_000), 0);
}

#[test]
fn rq_24_months_use_the_last_day_of_a_shorter_month() {
    // 31 January: February has 28 (29 in 2028), then 31 again in March.
    assert_eq!(day(2027, 1, 31).plus_months(1), Some(day(2027, 2, 28)));
    assert_eq!(day(2028, 1, 31).plus_months(1), Some(day(2028, 2, 29)));
    assert_eq!(day(2027, 1, 31).plus_months(2), Some(day(2027, 3, 31)));
    assert_eq!(day(2027, 1, 31).plus_months(3), Some(day(2027, 4, 30)));
    assert_eq!(day(2027, 1, 30).plus_months(1), Some(day(2027, 2, 28)));
    assert_eq!(day(2027, 1, 29).plus_months(1), Some(day(2027, 2, 28)));
    assert_eq!(day(2027, 1, 28).plus_months(1), Some(day(2027, 2, 28)));
    assert_eq!(day(2027, 8, 31).plus_months(1), Some(day(2027, 9, 30)));
    assert_eq!(day(2027, 3, 31).plus_months(0), Some(day(2027, 3, 31)));
}

#[test]
fn rq_24_months_and_years_cross_a_year_end() {
    assert_eq!(day(2027, 12, 31).plus_months(1), Some(day(2028, 1, 31)));
    assert_eq!(day(2027, 12, 31).plus_months(2), Some(day(2028, 2, 29)));
    assert_eq!(day(2027, 11, 30).plus_months(3), Some(day(2028, 2, 29)));
    assert_eq!(day(2027, 12, 15).plus_months(12), Some(day(2028, 12, 15)));
    assert_eq!(day(2027, 12, 15).plus_months(25), Some(day(2030, 1, 15)));
    assert_eq!(day(2026, 12, 31).plus_days(1), Some(day(2027, 1, 1)));
}

#[test]
fn rq_24_years_use_28_february_for_a_missing_29th() {
    assert_eq!(day(2028, 2, 29).plus_years(1), Some(day(2029, 2, 28)));
    assert_eq!(day(2028, 2, 29).plus_years(4), Some(day(2032, 2, 29)));
    assert_eq!(day(2027, 6, 1).plus_years(3), Some(day(2030, 6, 1)));
    assert!(MAX_DAY.plus_years(1).is_none());
    assert!(day(2200, 1, 1).plus_months(12).is_none());
}
