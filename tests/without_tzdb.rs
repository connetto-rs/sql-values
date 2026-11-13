//! A build without the `tzdb` feature reads time zones as a PostgreSQL with no time zone files
//! does, so only `GMT`, fixed offsets and POSIX specifications load.
#![cfg(not(feature = "tzdb"))]

use sql_values::postgres::{InputContext, PgTimeZone, PgTimestampTz, TimeZoneError};

#[test]
fn named_zones_are_refused_without_the_database() {
    for name in ["UTC", "Europe/Rome", "america/new_york", ":Etc/GMT+5"] {
        assert_eq!(
            PgTimeZone::parse(name),
            Err(TimeZoneError::InvalidValue),
            "{name}"
        );
    }
}

#[test]
fn offsets_gmt_and_posix_zones_load_without_the_database() {
    let offset = |setting| PgTimeZone::parse(setting).unwrap().fixed_offset();
    assert_eq!(offset("GMT"), Some(0));
    assert_eq!(offset("+2"), Some(7_200));
    assert_eq!(offset("interval '-01:00'"), Some(-3_600));
    assert_eq!(offset("EST5EDT"), None);
    let rome = PgTimeZone::parse("CET-1CEST,M3.5.0,M10.5.0/3").unwrap();
    let noon = PgTimestampTz::parse(
        "2026-07-01 12:00:00",
        InputContext::new().with_time_zone(&rome),
    );
    assert_eq!(noon.unwrap().to_string(), "2026-07-01 10:00:00+00");
}

#[test]
fn timestamptz_with_a_numeric_offset_reads_without_the_database() {
    let read = |text| PgTimestampTz::parse(text, InputContext::new()).map(|at| at.to_string());
    assert_eq!(
        read("2026-07-01 12:00:00+02").unwrap(),
        "2026-07-01 10:00:00+00"
    );
    assert!(read("2026-07-01 12:00:00 Europe/Rome").is_err());
}
