//! PS-92: times, strictly.

use crate::time::{format_rfc3339, millis_to_secs, parse_rfc3339, MAX_TIME};

#[test]
fn ps_92_a_time_is_rfc_3339_read_to_whole_seconds_in_utc() {
    assert_eq!(parse_rfc3339("1970-01-01T00:00:00Z"), Some(0));
    assert_eq!(parse_rfc3339("2026-11-10T14:00:05Z"), Some(1_794_319_205));
    // A fraction is dropped, not rounded.
    assert_eq!(
        parse_rfc3339("2026-11-10T14:00:05.999999999Z"),
        Some(1_794_319_205)
    );
    assert_eq!(parse_rfc3339("2026-11-10T14:00:05.1Z"), Some(1_794_319_205));
    // An offset is taken off.
    assert_eq!(
        parse_rfc3339("2026-11-10T09:00:05-05:00"),
        Some(1_794_319_205)
    );
    assert_eq!(
        parse_rfc3339("2026-11-10T15:30:05+01:30"),
        Some(1_794_319_205)
    );
    assert_eq!(parse_rfc3339("9999-12-31T23:59:59Z"), Some(MAX_TIME));
    // Leap days.
    assert_eq!(parse_rfc3339("2028-02-29T00:00:00Z"), Some(1_835_395_200));
    assert_eq!(parse_rfc3339("2100-02-28T00:00:00Z"), Some(4_107_456_000));
}

#[test]
fn ps_92_a_time_that_is_not_strictly_that_is_refused() {
    for bad in [
        "",
        "2026-11-10",
        "2026-11-10T14:00:05",
        "2026-11-10 14:00:05Z",
        "2026-11-10t14:00:05Z",
        "2026-11-10T14:00:05z",
        "2026-11-10T14:00:05.Z",
        "2026-11-10T14:00:05.1234567890Z",
        "2026-11-10T14:00:05+0100",
        "2026-11-10T14:00:05+01:00 ",
        " 2026-11-10T14:00:05Z",
        "2026-11-10T14:00:05Zx",
        "2026-13-10T14:00:05Z",
        "2026-00-10T14:00:05Z",
        "2026-11-00T14:00:05Z",
        "2026-11-31T14:00:05Z",
        "2026-02-29T14:00:05Z",
        "2100-02-29T00:00:00Z",
        "2026-11-10T24:00:00Z",
        "2026-11-10T14:60:00Z",
        "2026-11-10T14:00:60Z",
        "2026-11-10T14:00:05+24:00",
        "2026-11-10T14:00:05+01:60",
        "1969-12-31T23:59:59Z",
        "1970-01-01T00:00:00+01:00",
        "10000-01-01T00:00:00Z",
        "+026-11-10T14:00:05Z",
        "2026-11-10T14:00:0\u{e9}Z",
        "2026-11-10T14:00:05\u{ff1a}00",
    ] {
        assert_eq!(parse_rfc3339(bad), None, "{bad:?}");
    }
}

#[test]
fn ps_92_a_time_is_written_and_read_back_to_itself() {
    for t in [
        0,
        1,
        86_399,
        86_400,
        951_782_400,
        1_794_319_205,
        4_107_456_000,
        MAX_TIME,
    ] {
        let text = format_rfc3339(t).expect("writes");
        assert_eq!(parse_rfc3339(&text), Some(t), "{text}");
    }
    assert_eq!(
        format_rfc3339(1_794_319_000).as_deref(),
        Some("2026-11-10T13:56:40Z")
    );
    assert_eq!(format_rfc3339(MAX_TIME + 1), None);
}

#[test]
fn ps_92_milliseconds_are_cut_to_seconds_and_not_past_the_year_9999() {
    assert_eq!(millis_to_secs(0), Some(0));
    assert_eq!(millis_to_secs(999), Some(0));
    assert_eq!(millis_to_secs(1_794_319_421_900), Some(1_794_319_421));
    assert_eq!(millis_to_secs(MAX_TIME * 1_000 + 999), Some(MAX_TIME));
    assert_eq!(millis_to_secs((MAX_TIME + 1) * 1_000), None);
    assert_eq!(millis_to_secs(u64::MAX), None);
}
