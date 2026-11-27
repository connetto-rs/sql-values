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
use sql_values::postgres::{InputContext, PgTimestamp};

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

/// A connection under `lc_time` `C`, whose names `TM` prints.
fn connect() -> PgConnection {
    let mut conn = answers::connect(&PG18, PG18_TAG);
    diesel::select(set_config("lc_time", "C", false))
        .execute(&mut conn)
        .expect("set lc_time");
    conn
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
