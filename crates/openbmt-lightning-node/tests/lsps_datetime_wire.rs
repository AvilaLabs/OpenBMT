use std::str::FromStr;
use std::time::Duration;

use lightning::io::Cursor;
use lightning::util::ser::{Readable, Writeable};
use lightning_liquidity::lsps0::ser::LSPSDateTime;

fn assert_wire_forms(value: LSPSDateTime, display: &str, json: &str) {
    assert_eq!(value.to_string(), display);
    assert_eq!(serde_json::to_string(&value).unwrap(), json);
    assert_eq!(serde_json::from_str::<LSPSDateTime>(json).unwrap(), value);

    let mut bytes = Vec::new();
    value.write(&mut bytes).unwrap();

    // LSPSDateTime's Lightning encoding is a length-prefixed String; decode
    // the actual bytes independently, then decode the datetime from the same
    // bytes to cover both sides of the public wire API.
    let mut cursor = Cursor::new(bytes.as_slice());
    let encoded_string = String::read(&mut cursor).unwrap();
    assert_eq!(encoded_string, display);
    assert_eq!(cursor.position() as usize, bytes.len());
    assert_eq!(
        LSPSDateTime::read(&mut Cursor::new(bytes.as_slice())).unwrap(),
        value
    );
}

#[test]
fn lsps_datetime_preserves_display_serde_and_lightning_wire_spellings() {
    let whole_second = LSPSDateTime::from_str("2035-05-20T08:30:45Z").unwrap();
    assert_eq!(whole_second.unix_timestamp(), 2_063_262_645);
    assert_wire_forms(
        whole_second,
        "2035-05-20T08:30:45+00:00",
        "\"2035-05-20T08:30:45Z\"",
    );

    for (duration, display, json) in [
        (
            Duration::new(2, 100_000_000),
            "1970-01-01T00:00:02.100+00:00",
            "\"1970-01-01T00:00:02.100Z\"",
        ),
        (
            Duration::new(2, 1_000),
            "1970-01-01T00:00:02.000001+00:00",
            "\"1970-01-01T00:00:02.000001Z\"",
        ),
        (
            Duration::new(2, 1),
            "1970-01-01T00:00:02.000000001+00:00",
            "\"1970-01-01T00:00:02.000000001Z\"",
        ),
    ] {
        assert_wire_forms(
            LSPSDateTime::new_from_duration_since_epoch(duration),
            display,
            json,
        );
    }
}

#[test]
fn lsps_datetime_normalizes_offsets_and_legacy_separator_cases() {
    for input in [
        "1970-01-01T01:00:02.100+01:00",
        "1970-01-01t01:00:02.100+01:00",
        "1970-01-01 01:00:02.100+01:00",
    ] {
        let value = LSPSDateTime::from_str(input).unwrap();
        assert_wire_forms(
            value,
            "1970-01-01T00:00:02.100+00:00",
            "\"1970-01-01T00:00:02.100Z\"",
        );
        assert_eq!(value.unix_timestamp(), 2);
    }

    let excess_fraction = LSPSDateTime::from_str("1970-01-01t00:00:02.1234567899z").unwrap();
    assert_eq!(
        excess_fraction.to_string(),
        "1970-01-01T00:00:02.123456789+00:00"
    );
}

#[test]
fn lsps_datetime_keeps_chrono_leap_second_acceptance_and_precision() {
    // Chrono accepts an RFC3339 leap second on this non-table date. Keep that
    // parser behavior and its AutoSi fraction width while normalizing the
    // supplied offset to UTC.
    let value = LSPSDateTime::from_str("2015-02-18T23:59:60.1234567+01:00").unwrap();
    assert_wire_forms(
        value,
        "2015-02-18T22:59:60.123456700+00:00",
        "\"2015-02-18T22:59:60.123456700Z\"",
    );
    assert_eq!(value.unix_timestamp(), 1_424_300_399);

    let whole_leap_second = LSPSDateTime::from_str("2015-02-18T23:59:60Z").unwrap();
    assert_eq!(whole_leap_second.to_string(), "2015-02-18T23:59:60+00:00");
    assert_eq!(whole_leap_second.unix_timestamp(), 1_424_303_999);
}

#[test]
fn lsps_datetime_rejects_invalid_or_pre_epoch_values_and_keeps_whole_second_deltas() {
    for input in [
        "not a date",
        "1970-02-30T00:00:00Z",
        "1970-01-01X00:00:00Z",
        "1970-01-01T00:00:00+24:00",
        "1900-01-01T00:00:00Z",
    ] {
        assert!(LSPSDateTime::from_str(input).is_err(), "accepted {input}");
    }
    assert!(serde_json::from_str::<LSPSDateTime>("\"1900-01-01T00:00:00Z\"").is_err());

    let earlier = LSPSDateTime::new_from_duration_since_epoch(Duration::new(5, 999_999_999));
    let later = LSPSDateTime::new_from_duration_since_epoch(Duration::from_secs(12));
    assert_eq!(earlier.duration_since(&later), Duration::from_secs(7));
    assert_eq!(later.duration_since(&earlier), Duration::from_secs(7));
}
