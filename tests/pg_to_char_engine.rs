//! `to_char` of dates and times answered as PostgreSQL 18 answers it under `lc_time` `C`, every
//! template printed by both or refused with PostgreSQL's message.

#[path = "support/answers.rs"]
mod answers;
#[path = "support/containers.rs"]
mod containers;
#[path = "support/exit_slot.rs"]
mod exit_slot;
#[path = "support/postgres.rs"]
mod postgres;

use core::{convert::Infallible, fmt::Display};

use answers::{Answer, compare, quoted};
use diesel::{
    PgConnection, RunQueryDsl, define_sql_function,
    sql_types::{Bool, Text},
};
use sql_values::postgres::{InputContext, PgDate, PgTimeZone, PgTimestamp, PgTimestampTz};

static PG18: postgres::Slot = exit_slot::ExitSlot::new();

const PG18_TAG: &str = "18.6";

define_sql_function!(fn set_config(setting: Text, value: Text, is_local: Bool) -> Text);

/// Templates naming every keyword in each of its spellings, with and without each modifier, run
/// together, quoted, escaped and cut short.
const TEMPLATES: &[&str] = &[
    "",
    "YYYY MM DD HH24 HH12 MI SS MS US FF1 FF2 FF3 FF4 FF5 FF6",
    "[Day|Month|DAY|MONTH|day|month|Dy|Mon|DY|MON|dy|mon]",
    "FMDay|FMMonth|FMDD|DDTH|DDth|DDSP|\"literal YYYY\"|Q|RM|rm|J",
    "YYYY IYYY IYY IY I IW ID IDDD D DDD WW W CC Y,YYY YYY YY Y",
    "yyyy iyyy iyy iy i iw id iddd d ddd ww w cc y,yyy yyy yy y j q",
    "HH HH12 HH24 AM am A.M. a.m. PM pm P.M. p.m. SSSS SSSSS",
    "AD ad A.D. a.d. BC bc B.C. b.c.",
    "hh hh12 hh24 mi ss ms us ff1 ff2 ff3 ff4 ff5 ff6 ssss sssss",
    "FMHH FMHH12 FMHH24 FMMI FMSS FMMS FMUS FMFF3 FMSSSS FMMM FMDDD FMIDDD FMDD FMWW FMIW",
    "FMCC FMYYYY FMIYYY FMYYY FMIYY FMYY FMIY FMY FMI FMY,YYY FMRM FMrm FMQ FMJ FMW FMD FMID",
    "HH24TH MITH SSth USTH SSSSth MMTH DDDTH IDDDth DTH IDth WWTH IWth QTH CCth",
    "Y,YYYTH YYYYth IYYYTH YYYth YYTH YTH ITH WTH JTH HHth HH12TH FF1th MSTH",
    "FMDDTH FMMMth FMHH12TH FMDDDth FMCCTH",
    "TMDay TMMonth TMDAY TMMONTH TMday TMmonth TMDy TMMon TMdy TMmon",
    "FMTMMonth| TMFMMonth| tmmonth| fmmonth| TMDDTH",
    "TZ|tz|TZH|TZM|OF|FMOF|tzh|tzm|of",
    "FX DD FXMM fxDD SPDD DDsp DDSPTH",
    "\"escaped \\\" quote\" \\\"YYYY\\\" \\YYYY \"a\\\\b\" \"unterminated YYYY",
    "FM",
    "TM",
    "FMx",
    "\"\"",
    "\"",
    "\\",
    "\"\\",
    "日本 YYYY年MM月DD日 Ünïcödé",
    "Mon.DD,YYYY at HH12:MI:SS a.m.",
    "MonthDDYYYY",
    "MONTHMON",
    "DAYDY",
    "Y,YYYY",
    "IYYYY",
    "HH24HH12HH",
    "SSSSSSSSSS",
    "DDDDD",
    "MMMM",
    "TZHTZM",
    "AMPM",
    "FMMonthTH DayTH",
    "RMth",
    "TMRM",
    "Ddd dD mOn mONTH yYYY Hh24 Mi",
    "E G K L N V X Z e g k l n v x z 0 9",
];

/// Timestamps at both ends of the range, around year zero and the ISO year boundaries, a leap
/// day, noon and midnight, and the two infinities.
const TIMESTAMPS: &[&str] = &[
    "4714-11-24 00:00:00 BC",
    "4713-01-01 23:59:59.999999 BC",
    "1000-03-01 12:00:00 BC",
    "0101-12-31 BC",
    "0100-01-01 BC",
    "0001-01-01 00:00:00 BC",
    "0001-12-31 23:59:59 BC",
    "0001-01-01 00:00:00",
    "0099-12-31 11:59:59.5",
    "0100-01-01 12:00:00",
    "0101-01-01",
    "1999-12-31 12:00:00",
    "2000-01-01 00:00:00.5",
    "2001-01-01 00:00:00.000001",
    "2020-12-31 13:00:00",
    "2021-01-03 01:00:00",
    "2021-01-04 23:00:00",
    "2024-01-01",
    "2024-02-29 13:04:05.123456",
    "2024-02-29 00:04:05",
    "2024-02-29 23:59:59.999999",
    "2024-12-30",
    "2026-11-11 11:11:11.111111",
    "2026-12-22 22:22:22.222222",
    "9999-12-31 23:59:59.999999",
    "12345-06-07 00:00:00.000001",
    "294276-12-31 23:59:59.999999",
    "infinity",
    "-infinity",
];

/// IANA zones with gaps and repeats, half-hour and 45-minute offsets and a local mean time of
/// seconds, offsets in hours, POSIX rules, one on daylight saving time all year, and
/// abbreviations of 24 and 25 bytes, the second too long for `TZ`.
const ZONES: &[&str] = &[
    "UTC",
    "America/New_York",
    "America/Sao_Paulo",
    "America/St_Johns",
    "Asia/Kathmandu",
    "Australia/Lord_Howe",
    "Pacific/Kiritimati",
    "5.5",
    "-167",
    "167",
    "XXX3YYY",
    "<-0330>3:30<-0230>,M3.2.0,M11.1.0",
    "XXX3YYY,0/0,365/26",
    "<AAAAAAAAAAAAAAAAAAAAAAAA>5",
    "<AAAAAAAAAAAAAAAAAAAAAAAAA>5",
];

/// The zones whose offsets hold over PostgreSQL's whole range, beyond jiff's years.
const RULE_ZONES: &[&str] = &[
    "UTC",
    "5.5",
    "-167",
    "167",
    "XXX3YYY",
    "<-0330>3:30<-0230>,M3.2.0,M11.1.0",
    "XXX3YYY,0/0,365/26",
    "<AAAAAAAAAAAAAAAAAAAAAAAA>5",
    "<AAAAAAAAAAAAAAAAAAAAAAAAA>5",
];

/// Instants around daylight saving time changes, ISO year boundaries and a leap day, and both
/// infinities.
const INSTANTS: &[&str] = &[
    "1900-01-01 00:00:00+00",
    "1937-07-01 12:00:00+00",
    "2018-11-04 02:59:59.999999+00",
    "2018-11-04 03:00:00+00",
    "2024-02-29 00:00:00+00",
    "2024-03-10 06:59:59+00",
    "2024-03-10 07:00:00+00",
    "2024-11-03 05:30:00+00",
    "2024-11-03 06:30:00+00",
    "2024-12-30 12:00:00.5+00",
    "2021-01-03 23:30:00+00",
    "0001-01-01 00:00:00+00",
    "infinity",
    "-infinity",
];

/// Instants at the ends of PostgreSQL's range, outside jiff's years.
const FAR_INSTANTS: &[&str] = &[
    "4714-11-24 00:00:00+00 BC",
    "4714-11-25 12:00:00+00 BC",
    "294276-12-31 23:59:59.999999+00",
];

/// Dates whose midnight a zone skips, at the ends of the timestamp range and past it, a leap day
/// and both infinities.
const DATES: &[&str] = &[
    "2018-11-04",
    "2024-02-29",
    "2024-03-10",
    "2024-12-30",
    "294277-01-01",
    "5874897-12-31",
    "infinity",
    "-infinity",
];

/// Dates at the ends of the timestamp range, outside jiff's years.
const FAR_DATES: &[&str] = &["4714-11-24 BC", "4714-11-25 BC", "294276-12-31"];

/// Templates of every field a zone moves, with an empty one.
const ZONE_TEMPLATES: &[&str] = &[
    "",
    "YYYY-MM-DD HH24:MI:SS.US TZ tz TZH TZM OF FMOF",
    "Day DDD D ID IDDD IW IYYY WW W J Q CC AD am Y,YYY",
    "HH12 SSSS FMHH24 TZHTZM FMTZ tzTH OFTH",
    "TZ",
    "tz",
];

/// A connection under `lc_time` `C`, whose names `TM` prints.
fn connect() -> PgConnection {
    let mut conn = answers::connect(&PG18, PG18_TAG);
    diesel::select(set_config("lc_time", "C", false))
        .execute(&mut conn)
        .expect("set lc_time");
    conn
}

/// Sets `name` as the session's `TimeZone`, answering the zone as the crate reads it.
fn connect_in(conn: &mut PgConnection, name: &str) -> PgTimeZone {
    diesel::select(set_config("TimeZone", name, false))
        .execute(conn)
        .expect("set the zone");
    PgTimeZone::parse(name).unwrap_or_else(|error| panic!("{name:?}: {error}"))
}

/// The crate's answer in PostgreSQL's words, SQL `NULL` for no text.
fn answer<E: Display>(result: Result<Option<String>, E>) -> Answer {
    result.transpose().map_or(Answer::Null, Answer::of)
}

/// `to_char(timestamp, text)` of every timestamp with every template.
#[test]
fn pg18_formats_every_timestamp_with_every_template() {
    let mut conn = connect();
    let mut cases = Vec::new();
    for &text in TIMESTAMPS {
        let timestamp = PgTimestamp::parse(text, InputContext::new())
            .unwrap_or_else(|error| panic!("{text:?}: {error}"));
        for &template in TEMPLATES {
            let answer = answer(Ok::<_, Infallible>(timestamp.to_char(template)));
            cases.push((
                format!(
                    "SELECT to_char(timestamp {}, {})",
                    quoted(text),
                    quoted(template)
                ),
                answer,
            ));
        }
    }
    compare(&mut conn, cases);
}

/// Whether `template` names the weekday of a local date before 4714-11-23 BC, whose negative
/// weekday PostgreSQL looks up outside its table of names.
fn names_weekday_before_julian_day_zero(zone: &str, instant: &str, template: &str) -> bool {
    zone == "-167"
        && instant.ends_with(" BC")
        && ["DAY", "Day", "day", "DY", "Dy", "dy"]
            .iter()
            .any(|name| template.contains(name))
}

/// `to_char(timestamptz, text)` of every instant with every zone template in each zone.
#[test]
#[cfg_attr(not(feature = "tzdb"), ignore = "reads IANA zone names")]
fn pg18_formats_every_timestamptz_in_each_zone() {
    let mut conn = connect();
    let mut cases = Vec::new();
    for &name in ZONES {
        let zone = connect_in(&mut conn, name);
        let far: &[&str] = if RULE_ZONES.contains(&name) {
            FAR_INSTANTS
        } else {
            &[]
        };
        for &text in INSTANTS.iter().chain(far) {
            let instant = PgTimestampTz::parse(text, InputContext::new())
                .unwrap_or_else(|error| panic!("{text:?}: {error}"));
            for &template in ZONE_TEMPLATES.iter().chain(TEMPLATES) {
                if names_weekday_before_julian_day_zero(name, text, template) {
                    continue;
                }
                cases.push((
                    format!(
                        "SELECT to_char(timestamptz {}, {})",
                        quoted(text),
                        quoted(template)
                    ),
                    answer(instant.to_char(template, &zone)),
                ));
            }
        }
        compare(&mut conn, core::mem::take(&mut cases));
    }
}

/// `to_char(date, text)` of every date with every zone template in each zone.
#[test]
#[cfg_attr(not(feature = "tzdb"), ignore = "reads IANA zone names")]
fn pg18_formats_every_date_in_each_zone() {
    let mut conn = connect();
    let mut cases = Vec::new();
    for &name in ZONES {
        let zone = connect_in(&mut conn, name);
        let far: &[&str] = if RULE_ZONES.contains(&name) {
            FAR_DATES
        } else {
            &[]
        };
        for &text in DATES.iter().chain(far) {
            let date = PgDate::parse(text, InputContext::new())
                .unwrap_or_else(|error| panic!("{text:?}: {error}"));
            for &template in ZONE_TEMPLATES {
                cases.push((
                    format!(
                        "SELECT to_char(date {}, {})",
                        quoted(text),
                        quoted(template)
                    ),
                    answer(date.to_char(template, &zone)),
                ));
            }
        }
        compare(&mut conn, core::mem::take(&mut cases));
    }
}
