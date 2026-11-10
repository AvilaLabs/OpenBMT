//! BK-15 and BK-16: end-of-day totals and civil dates.

use super::fixture::*;
use super::outflow::fee;
use crate::date::{
    civil_from_days, days_from_civil, iso_time, MAX_OFFSET_MINUTES, MAX_UNIX, MIN_OFFSET_MINUTES,
};
use crate::{BooksError, Date, YearMonth};

fn d(year: i32, month: u8, day: u8) -> Date {
    Date { year, month, day }
}

#[test]
fn bk_15_totals_per_utc_day() {
    let totals = rich_kit().books().utc_day_totals().expect("totals");
    let days: Vec<Date> = totals.iter().map(|t| t.day).collect();
    assert_eq!(days, vec![d(2025, 1, 31), d(2025, 2, 1), d(2025, 2, 2)]);
    let jan = &totals[0];
    assert_eq!((jan.sales, jan.waiting), (1, 0));
    assert_eq!(jan.minor.get(&usd()), Some(&1850));
    assert_eq!((jan.sats, jan.sats_unknown_sales), (SATS_PAID, 0));
    // The late payment is on the day it was written, and is waiting.
    let feb1 = &totals[1];
    assert_eq!((feb1.sales, feb1.waiting), (1, 1));
    assert_eq!(feb1.minor.get(&usd()), Some(&900));
    assert_eq!(feb1.sats, SATS_LATE);
    // Sale 4 is waiting, 5 is final; 6 (replaced) and 7 (dust) are not sales
    // that received funds.
    let feb2 = &totals[2];
    assert_eq!((feb2.sales, feb2.waiting), (2, 1));
    assert_eq!(feb2.minor.get(&usd()), Some(&6700));
    assert_eq!(feb2.sats, SATS_PAID * 2 + SATS_SURPLUS);
}

#[test]
fn bk_15_totals_per_shop_local_day_for_a_fixed_offset() {
    let kit = rich_kit();
    let books = kit.books();
    // 23:59:30 UTC on 31 January is already 1 February one hour east.
    let east = books.local_day_totals(60).expect("totals");
    assert_eq!(east[0].day, d(2025, 2, 1));
    assert_eq!(east[0].sales, 2);
    // Five hours west the same instant is still 31 January, 18:59.
    let west = books.local_day_totals(-300).expect("totals");
    assert_eq!(west[0].day, d(2025, 1, 31));
    assert_eq!(books.local_day_totals(0), books.utc_day_totals());
}

#[test]
fn bk_15_totals_are_of_sales_only_and_outflows_do_not_change_them() {
    let kit = rich_kit();
    let before = kit.books().utc_day_totals().expect("totals");
    let after = kit
        .books()
        .with_disposals(vec![fee(FEB_2_NOON, 1_000_000, Some(500))])
        .expect("disposals")
        .utc_day_totals()
        .expect("totals");
    assert_eq!(before, after);
}

#[test]
fn bk_15_a_sale_with_unknown_sats_is_counted_apart() {
    let kit = kit_with(&[
        opened(0, 10, 100),
        openbmt_counter::Event::PaidAtOnce {
            index: 0,
            at: 11,
            at_risk_sats: 9,
            label: openbmt_core::policy::InstantLabel::YourNode,
            mode: openbmt_core::policy::CheckMode::Light,
        },
    ]);
    let t = kit.books().utc_day_totals().expect("totals");
    assert_eq!((t[0].sales, t[0].waiting), (1, 1));
    assert_eq!((t[0].sats, t[0].sats_unknown_sales), (0, 1));
}

#[test]
fn bk_16_an_offset_outside_the_range_is_refused() {
    let books = rich_kit().books();
    for minutes in [
        MIN_OFFSET_MINUTES - 1,
        MAX_OFFSET_MINUTES + 1,
        i32::MAX,
        i32::MIN,
    ] {
        assert_eq!(
            books.local_day_totals(minutes),
            Err(BooksError::BadOffset { minutes })
        );
    }
    assert!(books.local_day_totals(MIN_OFFSET_MINUTES).is_ok());
    assert!(books.local_day_totals(MAX_OFFSET_MINUTES).is_ok());
}

#[test]
fn bk_16_dates_across_leap_years_and_the_known_boundaries() {
    let cases: [(u64, (i32, u8, u8)); 10] = [
        (0, (1970, 1, 1)),
        (86_399, (1970, 1, 1)),
        (86_400, (1970, 1, 2)),
        (951_782_400, (2000, 2, 29)),
        (951_868_800, (2000, 3, 1)),
        (1_709_164_800, (2024, 2, 29)),
        (1_709_251_200, (2024, 3, 1)),
        (2_147_483_647, (2038, 1, 19)),
        (4_102_444_800, (2100, 1, 1)),
        (MAX_UNIX, (9999, 12, 31)),
    ];
    for (secs, (y, m, day)) in cases {
        assert_eq!(Date::from_unix(secs), Some(d(y, m, day)), "{secs}");
    }
    assert_eq!(Date::from_unix(MAX_UNIX + 1), None);
    // 2100 is not a leap year; 2000 is.
    assert_eq!(
        days_from_civil(2100, 3, 1) - days_from_civil(2100, 2, 28),
        1
    );
    assert_eq!(
        days_from_civil(2000, 3, 1) - days_from_civil(2000, 2, 28),
        2
    );
    assert_eq!(days_from_civil(1970, 1, 1), 0);
}

#[test]
fn bk_16_every_day_from_1970_to_2110_follows_the_one_before() {
    let mut prev = civil_from_days(0);
    assert_eq!(prev, (1970, 1, 1));
    for day in 1..51_500i64 {
        let cur = civil_from_days(day);
        assert_eq!(days_from_civil(cur.0, cur.1, cur.2), day);
        let (y, m, dd) = prev;
        let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
        let len = match m {
            2 if leap => 29,
            2 => 28,
            4 | 6 | 9 | 11 => 30,
            _ => 31,
        };
        let next = if dd < len {
            (y, m, dd + 1)
        } else if m < 12 {
            (y, m + 1, 1)
        } else {
            (y + 1, 1, 1)
        };
        assert_eq!(cur, next, "day {day}");
        prev = cur;
    }
}

#[test]
fn bk_16_offsets_move_the_date_and_dates_print_as_iso() {
    assert_eq!(
        Date::from_unix_offset(JAN_31_235930, 60).map(|x| x.to_string()),
        Some("2025-02-01".to_string())
    );
    assert_eq!(
        Date::from_unix_offset(0, -720).map(|x| x.to_string()),
        Some("1969-12-31".to_string())
    );
    assert_eq!(Date::from_unix_offset(0, 841), None);
    assert_eq!(
        iso_time(JAN_31_235930),
        Some("2025-01-31T23:59:30Z".to_string())
    );
    assert_eq!(iso_time(MAX_UNIX + 1), None);
    assert_eq!(
        YearMonth {
            year: 2025,
            month: 2
        }
        .to_string(),
        "2025-02"
    );
}
