//! `date_trunc` of `timestamp`, `timestamptz`, `date` and `interval` computed as PostgreSQL 18
//! computes it, every answer and every refusal's message measured on the engine.

#[path = "support/answers.rs"]
mod answers;
#[path = "support/containers.rs"]
mod containers;
#[path = "support/exit_slot.rs"]
mod exit_slot;
#[path = "support/postgres.rs"]
mod postgres;

use answers::{Answer, compare, quoted};
use diesel::{
    PgConnection, RunQueryDsl, define_sql_function,
    sql_types::{Bool, Text},
};
use sql_values::postgres::{
    CastTarget, CastValue, DateTimeError, DateTruncError, DateTruncField, InputContext, Literal,
    PgDate, PgTimeZone, PgTimestamp, PgTimestampTz, cast_literal, parse_interval,
};

static PG18: postgres::Slot = exit_slot::ExitSlot::new();

const PG18_TAG: &str = "18.6";

define_sql_function!(fn set_config(setting: Text, value: Text, is_local: Bool) -> Text);

/// Every unit name, aliases in mixed case, a long spelling whose first ten letters decide, the
/// units `DecodeUnits` knows that `date_trunc` refuses, and words it does not know.
const UNITS: &[&str] = &[
    "microseconds",
    "milliseconds",
    "second",
    "minute",
    "hour",
    "day",
    "week",
    "month",
    "quarter",
    "year",
    "decade",
    "century",
    "millennium",
    "us",
    "MSec",
    "millisecondsss",
    "S",
    "Mins",
    "h",
    "Days",
    "w",
    "mons",
    "qtr",
    "Y",
    "decs",
    "c",
    "mils",
    "timezone",
    "timezone_m",
    "epoch",
    "dow",
    "julian",
    "ago",
    "@",
    "",
];

/// Timestamps on a leap day, before the epoch, around 1 BC, on ISO weeks that straddle a year,
/// on the decade, century and millennium boundaries before 1 AD, at both ends of the range, the
/// stored 294277-01-01 that `timestamp(0)` rounds the last second up to, and both infinities,
/// each with the SQL type that makes it.
const TIMESTAMPS: &[(&str, &str)] = &[
    ("2024-02-29 23:59:59.999500", "timestamp"),
    ("1969-12-31 23:59:59.999500", "timestamp"),
    ("2000-01-01 00:00:00.1235", "timestamp"),
    ("2021-01-03 10:00:00", "timestamp"),
    ("2019-12-30 00:00:00", "timestamp"),
    ("0001-01-01 00:00:00", "timestamp"),
    ("0001-12-31 23:59:59.999999 BC", "timestamp"),
    ("0002-06-15 12:00:00 BC", "timestamp"),
    ("0010-06-15 12:00:00 BC", "timestamp"),
    ("0011-06-15 12:00:00 BC", "timestamp"),
    ("1000-06-15 12:00:00 BC", "timestamp"),
    ("1001-06-15 12:00:00 BC", "timestamp"),
    ("4714-11-24 00:00:00 BC", "timestamp"),
    ("4714-11-30 12:00:00 BC", "timestamp"),
    ("294276-12-31 23:59:59.999999", "timestamp"),
    ("294276-12-31 23:59:59.5", "timestamp(0)"),
    ("infinity", "timestamp"),
    ("-infinity", "timestamp"),
];

/// Zones with daylight saving in the north and in the south, one that changed at midnight, one
/// at a half-hour offset, a fixed offset and a POSIX rule.
const ZONES: &[&str] = &[
    "UTC",
    "America/New_York",
    "Australia/Sydney",
    "America/Sao_Paulo",
    "Asia/Kolkata",
    "Europe/Rome",
    "-03:30",
    "EST5EDT,M3.2.0,M11.1.0",
];

/// Instants on both sides of New York's 2024 changes, Rome's and Sydney's 2026 changes, São
/// Paulo's 2018 changes at midnight, at local mean time, near PostgreSQL's first instant, and
/// both infinities.
const INSTANTS: &[&str] = &[
    "2024-03-10 07:30:00.123456+00",
    "2024-11-03 05:30:00.123456+00",
    "2024-11-03 06:30:00.123456+00",
    "2026-03-29 01:30:00+00",
    "2026-10-25 00:30:00+00",
    "2026-10-25 01:30:00+00",
    "2026-04-04 15:30:00+00",
    "2026-10-03 16:30:00+00",
    "2018-11-04 12:00:00+00",
    "2018-02-18 01:30:00+00",
    "2018-02-18 02:30:00+00",
    "1850-01-01 00:00:00+00",
    "4714-11-24 00:30:00+00 BC",
    "4714-11-25 03:00:00+00 BC",
    "infinity",
    "-infinity",
];

const FAR: &[&str] = &[
    "9999-12-31 12:00:00+00",
    "9999-12-31 23:59:59.999999+00",
    "294276-12-31 23:59:59.999999+00",
    "200000-06-15 12:00:00+00",
];

/// Intervals of both signs with every field, a lone fraction, years past a millennium, the
/// largest and smallest finite ones, and both infinities.
const INTERVALS: &[&str] = &[
    "-14 months -3 days -25:02:03.123500",
    "14 months 9 days 25:02:03.123500",
    "-00:00:00.123500",
    "1234 years 11 months",
    "-1234 years -11 months",
    "178956970 years 7 months 2147483647 days 2562047788:00:54.775807",
    "-178956970 years -8 months -2147483648 days -2562047788:00:54.775807",
    "infinity",
    "-infinity",
];

/// A session under `TimeZone` `zone`.
fn session(zone: &str) -> PgConnection {
    let mut conn = answers::connect(&PG18, PG18_TAG);
    set_zone(&mut conn, zone);
    conn
}

fn set_zone(conn: &mut PgConnection, zone: &str) {
    let _: String = diesel::select(set_config("TimeZone", zone, false))
        .get_result(conn)
        .unwrap_or_else(|error| panic!("set TimeZone {zone}: {error}"));
}

fn zone(name: &str) -> PgTimeZone {
    PgTimeZone::parse(name).unwrap_or_else(|error| panic!("{name:?}: {error}"))
}

fn instant(text: &str) -> PgTimestampTz {
    PgTimestampTz::parse(text, InputContext::new())
        .unwrap_or_else(|error| panic!("{text:?}: {error}"))
}

fn timestamp(text: &str, type_name: &str) -> PgTimestamp {
    let target = match type_name {
        "timestamp" => CastTarget::Timestamp(None),
        _ => CastTarget::Timestamp(Some(0)),
    };
    match cast_literal(Literal::Quoted(text), target) {
        Ok(CastValue::Timestamp(at)) => at,
        other => panic!("{text:?}::{type_name}: {other:?}"),
    }
}

/// PostgreSQL's refusal of `unit` for `type_name`, which quotes the unit lowercased.
fn refusal(error: DateTruncError, unit: &str, type_name: &str) -> Answer {
    let verb = match error {
        DateTruncError::UnitNotRecognized => "recognized",
        DateTruncError::UnitNotSupported => "supported",
    };
    let unit = unit.to_ascii_lowercase();
    Answer::Error(format!("unit \"{unit}\" not {verb} for type {type_name}"))
}

/// The crate's answer for `date_trunc(unit, …)` of a type named `type_name`, the value
/// `truncate` gives for the unit, printed in UTC.
fn answer<T: std::fmt::Display>(
    unit: &str,
    type_name: &str,
    truncate: impl FnOnce(DateTruncField) -> Result<T, DateTimeError>,
) -> Answer {
    match DateTruncField::parse(unit) {
        Ok(field) => Answer::of(truncate(field)),
        Err(error) => refusal(error, unit, type_name),
    }
}

#[test]
fn pg18_truncates_every_timestamp_to_every_unit() {
    let mut conn = session("UTC");
    let mut cases = Vec::new();
    for &(text, type_name) in TIMESTAMPS {
        let value = timestamp(text, type_name);
        for &unit in UNITS {
            cases.push((
                format!(
                    "SELECT date_trunc({}, {}::{type_name})::text",
                    quoted(unit),
                    quoted(text)
                ),
                answer(unit, "timestamp without time zone", |field| {
                    value.date_trunc(field)
                }),
            ));
        }
    }
    compare(&mut conn, cases);
}

#[test]
#[cfg_attr(not(feature = "tzdb"), ignore = "reads IANA zone names")]
fn pg18_truncates_every_instant_in_each_session_zone() {
    let mut conn = session("UTC");
    let utc = zone("UTC");
    for &name in ZONES {
        set_zone(&mut conn, name);
        let in_zone = zone(name);
        let mut cases = Vec::new();
        for &text in INSTANTS.iter().chain(FAR) {
            let value = instant(text);
            for &unit in UNITS {
                cases.push((
                    format!(
                        "SELECT (date_trunc({}, {}::timestamptz) AT TIME ZONE 'UTC')::text",
                        quoted(unit),
                        quoted(text)
                    ),
                    answer(unit, "timestamp with time zone", |field| {
                        value
                            .date_trunc(field, &in_zone)
                            .and_then(|at| at.to_timestamp(&utc))
                    }),
                ));
            }
        }
        compare(&mut conn, cases);
    }
}

#[test]
#[cfg_attr(not(feature = "tzdb"), ignore = "reads IANA zone names")]
fn pg18_truncates_a_date_at_its_session_midnight_in_an_explicit_zone() {
    let mut conn = session("America/New_York");
    let (session_zone, sydney, utc) = (
        zone("America/New_York"),
        zone("Australia/Sydney"),
        zone("UTC"),
    );
    let mut cases = Vec::new();
    for text in [
        "2024-02-29",
        "2024-11-03",
        "4714-11-24 BC",
        "infinity",
        "-infinity",
    ] {
        let date = PgDate::parse(text, InputContext::new())
            .unwrap_or_else(|error| panic!("{text:?}: {error}"));
        let midnight = date
            .to_timestamp()
            .unwrap_or_else(|error| panic!("{text:?}: {error}"))
            .to_timestamptz(&session_zone);
        for &unit in UNITS {
            for (truncation_zone, call) in [
                (
                    &session_zone,
                    format!("{}, {}::date", quoted(unit), quoted(text)),
                ),
                (
                    &sydney,
                    format!(
                        "{}, {}::date, 'Australia/Sydney'",
                        quoted(unit),
                        quoted(text)
                    ),
                ),
            ] {
                cases.push((
                    format!("SELECT (date_trunc({call}) AT TIME ZONE 'UTC')::text"),
                    answer(unit, "timestamp with time zone", |field| {
                        midnight
                            .and_then(|at| at.date_trunc(field, truncation_zone))
                            .and_then(|at| at.to_timestamp(&utc))
                    }),
                ));
            }
        }
    }
    compare(&mut conn, cases);
}

#[test]
fn pg18_truncates_every_interval_to_every_unit() {
    let mut conn = session("UTC");
    let mut cases = Vec::new();
    for &text in INTERVALS {
        let value = parse_interval(text).unwrap_or_else(|error| panic!("{text:?}: {error}"));
        for &unit in UNITS {
            let got = match DateTruncField::parse(unit).and_then(|field| value.date_trunc(field)) {
                Ok(truncated) => Answer::Value(truncated.to_string()),
                Err(error) => refusal(error, unit, "interval"),
            };
            cases.push((
                format!(
                    "SELECT date_trunc({}, {}::interval)::text",
                    quoted(unit),
                    quoted(text)
                ),
                got,
            ));
        }
    }
    compare(&mut conn, cases);
}
