//! `extract` and `date_part` of every temporal type computed as PostgreSQL 18 computes them, every
//! answer, `NULL` and refusal's message measured on the engine.

#[path = "support/answers.rs"]
mod answers;
#[path = "support/containers.rs"]
mod containers;
#[path = "support/exit_slot.rs"]
mod exit_slot;
#[path = "support/postgres.rs"]
mod postgres;

use core::fmt::Display;

use answers::{Answer, compare, quoted};
use diesel::{
    PgConnection,
    prelude::*,
    sql_types::{Bool, Text},
};
use sql_values::postgres::{
    ExtractError, ExtractField, InputContext, PgDate, PgTime, PgTimeTz, PgTimeZone, PgTimestamp,
    PgTimestampTz, parse_interval,
};

static PG18: postgres::Slot = exit_slot::ExitSlot::new();

const PG18_TAG: &str = "18.6";

define_sql_function!(fn set_config(setting: Text, value: Text, is_local: Bool) -> Text);

const FIELDS: [ExtractField; 22] = [
    ExtractField::Microseconds,
    ExtractField::Milliseconds,
    ExtractField::Second,
    ExtractField::Minute,
    ExtractField::Hour,
    ExtractField::Day,
    ExtractField::Week,
    ExtractField::Month,
    ExtractField::Quarter,
    ExtractField::Year,
    ExtractField::Decade,
    ExtractField::Century,
    ExtractField::Millennium,
    ExtractField::DayOfWeek,
    ExtractField::IsoDayOfWeek,
    ExtractField::DayOfYear,
    ExtractField::IsoYear,
    ExtractField::Epoch,
    ExtractField::Julian,
    ExtractField::Timezone,
    ExtractField::TimezoneHour,
    ExtractField::TimezoneMinute,
];

/// Dates at both ends of the type and of `timestamp`, around year 0, ISO weeks that cross a
/// year, leap days and both infinities.
const DATES: &[&str] = &[
    "2006-01-01",
    "2005-01-02",
    "2008-12-29",
    "2009-12-31",
    "2010-01-03",
    "2024-02-29",
    "1970-01-01",
    "1969-12-31",
    "2000-01-01",
    "2001-01-01",
    "0001-01-01",
    "0001-12-31 BC",
    "0002-01-01 BC",
    "0010-06-15 BC",
    "0011-01-01 BC",
    "0100-12-31 BC",
    "0101-01-01 BC",
    "1000-12-31 BC",
    "1001-01-01 BC",
    "4713-11-24 BC",
    "4714-11-24 BC",
    "294276-12-31",
    "294277-01-01",
    "5874897-12-31",
    "infinity",
    "-infinity",
];

const TIMES: &[&str] = &[
    "00:00:00",
    "00:00:00.000001",
    "00:00:00.1235",
    "12:34:56.123456",
    "23:59:59.123456",
    "23:59:59.999999",
    "24:00:00",
];

/// Times at offsets east and west, with minutes and seconds, at the limit of 15:59:59.
const TIMETZS: &[&str] = &[
    "12:34:56.123456-05:30",
    "12:34:56.123456+05:45",
    "00:00:00+00",
    "24:00:00-15:59:59",
    "23:59:59.999999+15:59",
    "01:02:03.5-00:00:01",
];

/// Timestamps at both ends of the type, on each side of the Unix epoch and of year 0, at the
/// times whose fractions `double precision` rounds, and both infinities.
const TIMESTAMPS: &[&str] = &[
    "2006-01-01 12:34:56.123456",
    "2006-01-01 00:00:00",
    "2024-02-29 23:59:59.999999",
    "1969-12-31 23:59:59.999999",
    "1970-01-01 00:00:00.000001",
    "2000-01-01 00:00:00.5",
    "0001-01-01 00:00:00",
    "0001-12-31 23:59:59.999999 BC",
    "4714-11-24 00:00:00 BC",
    "4714-11-24 12:00:00.000001 BC",
    "262142-12-31 23:59:59.999999",
    "294247-01-10 04:00:54.775806",
    "294247-01-10 04:00:54.775807",
    "294276-12-31 23:59:59.999999",
    "infinity",
    "-infinity",
];

/// Instants around 2024's changes in each zone, at local mean time, at PostgreSQL's first
/// instant, and both infinities.
const INSTANTS: &[&str] = &[
    "2024-03-10 07:30:00.123456+00",
    "2024-11-03 05:30:00+00",
    "2024-11-03 06:30:00+00",
    "2024-03-31 00:59:59.999999+00",
    "2024-10-05 16:00:00+00",
    "1850-01-01 00:00:00+00",
    "2006-01-01 00:00:00+00",
    "4714-11-24 00:00:00+00 BC",
    "infinity",
    "-infinity",
];

/// Zones north and south, with half and quarter hours, at local mean time, a fixed offset and
/// POSIX rules.
const ZONES: &[&str] = &[
    "UTC",
    "America/New_York",
    "Europe/Rome",
    "Australia/Sydney",
    "Asia/Kathmandu",
    "America/St_Johns",
    "+02:30",
    "EST5EDT,M3.2.0,M11.1.0",
];

const FAR_INSTANTS: &[&str] = &[
    "9999-12-31 12:00:00+00",
    "9999-12-31 23:59:59.999999+00",
    "294276-12-31 23:59:59.999999+00",
    "200000-06-15 12:00:00+00",
];

/// Intervals of every sign mix, with years beyond what an epoch in microseconds holds, at
/// every field's limit, and both infinities.
const INTERVALS: &[&str] = &[
    "-14 months -3 days -25:02:03.123456",
    "1 year 2 mons 3 days 04:05:06.789",
    "9 days",
    "-9 days",
    "-1 mon",
    "1 mon",
    "1 year",
    "-00:00:00.1235",
    "00:00:00",
    "178956970 years 7 mons 2147483647 days 2562047788:00:54.775807",
    "-178956970 years -8 mons -2147483648 days -2562047788:00:54.775807",
    "-178956970 years -8 mons",
    "1000000 years 1 day",
    "infinity",
    "-infinity",
];

/// Spellings PostgreSQL reads as units, in any case, cut at 10 bytes, and words it refuses.
const SPELLINGS: &[&str] = &[
    "microseconds",
    "MicroSecondsXYZ",
    "us",
    "usecs",
    "msec",
    "ms",
    "s",
    "SECS",
    "m",
    "mm",
    "min",
    "h",
    "hrs",
    "d",
    "w",
    "mon",
    "qtr",
    "y",
    "yr",
    "dec",
    "decs",
    "c",
    "cent",
    "mil",
    "millennia",
    "dow",
    "isodow",
    "doy",
    "isoyear",
    "epoch",
    "j",
    "jd",
    "julian",
    "timezone",
    "timezone_h",
    "timezone_minute",
    "fortnight",
    "jan",
    "ago",
    "",
];

fn connect() -> PgConnection {
    answers::connect(&PG18, PG18_TAG)
}

/// The crate's answer, `None` as SQL `NULL`.
fn answer<T: Display>(result: Result<Option<T>, ExtractError>) -> Answer {
    result.transpose().map_or(Answer::Null, Answer::of)
}

/// The `extract` and `date_part` queries of every field of `literal` read as `sql_type`, with
/// the crate's answers.
fn cases<N: Display, F: Display>(
    sql_type: &str,
    literal: &str,
    extract: impl Fn(ExtractField) -> Result<Option<N>, ExtractError>,
    date_part: impl Fn(ExtractField) -> Result<Option<F>, ExtractError>,
) -> Vec<(String, Answer)> {
    let value = format!("{sql_type} {}", quoted(literal));
    FIELDS
        .iter()
        .flat_map(|&field| {
            [
                (
                    format!("SELECT extract({field} FROM {value})::text"),
                    answer(extract(field)),
                ),
                (
                    format!("SELECT date_part('{field}', {value})::text"),
                    answer(date_part(field)),
                ),
            ]
        })
        .collect()
}

fn context() -> InputContext<'static> {
    InputContext::new()
}

#[test]
fn pg18_extracts_every_field_of_dates() {
    let mut conn = connect();
    let mut all = Vec::new();
    for &literal in DATES {
        let date = PgDate::parse(literal, context()).expect(literal);
        all.extend(cases(
            "date",
            literal,
            |field| date.extract(field),
            |field| date.date_part(field),
        ));
    }
    compare(&mut conn, all);
}

#[test]
fn pg18_extracts_every_field_of_times() {
    let mut conn = connect();
    let mut all = Vec::new();
    for &literal in TIMES {
        let time = PgTime::parse(literal, context()).expect(literal);
        all.extend(cases(
            "time",
            literal,
            |field| time.extract(field),
            |field| time.date_part(field),
        ));
    }
    for &literal in TIMETZS {
        let time = PgTimeTz::parse(literal, context()).expect(literal);
        all.extend(cases(
            "timetz",
            literal,
            |field| time.extract(field),
            |field| time.date_part(field),
        ));
    }
    compare(&mut conn, all);
}

#[test]
fn pg18_extracts_every_field_of_timestamps() {
    let mut conn = connect();
    let mut all = Vec::new();
    for &literal in TIMESTAMPS {
        let at = PgTimestamp::parse(literal, context()).expect(literal);
        all.extend(cases(
            "timestamp",
            literal,
            |field| at.extract(field),
            |field| at.date_part(field),
        ));
    }
    compare(&mut conn, all);
}

#[test]
fn pg18_extracts_every_field_of_intervals() {
    let mut conn = connect();
    let mut all = Vec::new();
    for &literal in INTERVALS {
        let interval = parse_interval(literal).expect(literal);
        all.extend(cases(
            "interval",
            literal,
            |field| interval.extract(field),
            |field| interval.date_part(field),
        ));
    }
    compare(&mut conn, all);
}

/// Every field of each instant with each zone as the session's `TimeZone`.
#[test]
#[cfg_attr(not(feature = "tzdb"), ignore = "reads IANA zone names")]
fn pg18_extracts_every_field_of_timestamptz_in_each_zone() {
    let mut conn = connect();
    for &name in ZONES {
        let zone = PgTimeZone::parse(name).unwrap_or_else(|error| panic!("{name:?}: {error}"));
        let _: String = diesel::select(set_config("TimeZone", name, false))
            .get_result(&mut conn)
            .expect("set TimeZone");
        let mut all = Vec::new();
        for &literal in INSTANTS.iter().chain(FAR_INSTANTS) {
            let at = PgTimestampTz::parse(literal, context()).expect(literal);
            all.extend(cases(
                "timestamptz",
                literal,
                |field| Ok(at.extract(field, &zone)),
                |field| Ok(at.date_part(field, &zone)),
            ));
        }
        compare(&mut conn, all);
    }
}

/// Each spelling's field against `date_part` of a `timestamptz`, which has every field, where
/// PostgreSQL refuses a word that names no unit as not recognized.
#[test]
fn pg18_reads_every_unit_spelling() {
    let mut conn = connect();
    let _: String = diesel::select(set_config("TimeZone", "GMT", false))
        .get_result(&mut conn)
        .expect("set TimeZone");
    let gmt = PgTimeZone::parse("GMT").expect("GMT");
    let literal = "2006-01-01 12:34:56.123456+00";
    let at = PgTimestampTz::parse(literal, context()).expect(literal);
    let cases = SPELLINGS
        .iter()
        .map(|&spelling| {
            let expected = match ExtractField::parse(spelling) {
                Some(field) => answer(Ok(at.date_part(field, &gmt))),
                None => Answer::Error(format!(
                    "unit \"{}\" not recognized for type timestamp with time zone",
                    spelling.to_lowercase()
                )),
            };
            (
                format!(
                    "SELECT date_part({}, timestamptz {})::text",
                    quoted(spelling),
                    quoted(literal)
                ),
                expected,
            )
        })
        .collect();
    compare(&mut conn, cases);
}
