//! `to_timestamp(double precision)` computed as PostgreSQL 18 computes it, every answer and every
//! refusal's message measured on the engine.

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
    define_sql_function,
    prelude::*,
    sql_types::{Bool, Text},
};
use sql_values::postgres::{DateTimeError, PgFloat8, PgTimestampTz};

static PG18: postgres::Slot = exit_slot::ExitSlot::new();

const PG18_TAG: &str = "18.6";

define_sql_function!(fn set_config(setting: Text, value: Text, is_local: Bool) -> Text);

const INPUTS: &[&str] = &[
    "0",
    "-0",
    "-1.25",
    "0.0000005",
    "0.0000015",
    "-0.0000005",
    "-0.0000015",
    "0.0000006",
    "0.0000014",
    "-0.0000006",
    "946684800.0000005",
    "946684800.0000015",
    "946684800.0000025",
    "946684800.0000035",
    "0.0078125",
    "0.0234375",
    "-0.0078125",
    "-0.0234375",
    "946684800.0078125",
    "946684800.0234375",
    "Infinity",
    "-Infinity",
    "NaN",
    "-210866803200",
    "-210866803200.1",
    "9224318015999",
    "9224318015999.998",
    "9224318016000",
    "1e20",
    "-1e20",
    "1e308",
    "-1e-300",
];

/// Seconds spread over PostgreSQL's whole range and over the decades around 1970, drawn from a
/// fixed xorshift sequence, and every binary tie between two microseconds within a second of
/// five instants on both sides of 1970 and 2000.
fn sweep() -> Vec<f64> {
    let mut state: u64 = 0x9e37_79b9_7f4a_7c15;
    let mut unit = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        (state >> 11) as f64 / (1_u64 << 53) as f64
    };
    let mut seconds = Vec::new();
    for _ in 0..2_000 {
        seconds.push(-210_866_803_200.0 + 9_435_184_819_200.0 * unit());
        seconds.push(-4e9 + 8e9 * unit());
    }
    for whole in [-946_684_800.0, -1.0, 0.0, 946_684_800.0, 1_700_000_000.0] {
        for numerator in (1..256).step_by(2) {
            seconds.push(whole + f64::from(numerator) / 128.0);
        }
    }
    seconds
}

/// The crate's answer in PostgreSQL's words, whose range refusal quotes the seconds as `%g`.
fn crate_answer(seconds: f64) -> Answer {
    match PgTimestampTz::from_unix_seconds(seconds) {
        Err(error @ DateTimeError::TimestampOutOfRange) => {
            Answer::Error(format!("{error}: \"{}\"", printf_g(seconds)))
        }
        result => Answer::of(result),
    }
}

/// C's `%g` of a magnitude of at least 1e6, which it prints with an exponent.
fn printf_g(value: f64) -> String {
    assert!(value.abs() >= 1e6, "{value} prints without an exponent");
    let text = format!("{value:.5e}");
    let (mantissa, exponent) = text.split_once('e').expect("an exponent");
    let mantissa = mantissa.trim_end_matches('0').trim_end_matches('.');
    let exponent: i32 = exponent.parse().expect("a decimal exponent");
    format!("{mantissa}e{exponent:+03}")
}

#[test]
fn pg18_converts_unix_seconds_as_to_timestamp() {
    let mut conn = answers::connect(&PG18, PG18_TAG);
    let _: String = diesel::select(set_config("TimeZone", "UTC", false))
        .get_result(&mut conn)
        .expect("set TimeZone");
    let spelled = INPUTS.iter().map(|text| {
        let seconds = PgFloat8::parse(text).unwrap_or_else(|error| panic!("{text:?}: {error}"));
        (quoted(text), seconds.value())
    });
    let swept = sweep()
        .into_iter()
        .map(|seconds| (quoted(&format!("{seconds:?}")), seconds));
    let cases = spelled
        .chain(swept)
        .map(|(literal, seconds)| {
            (
                format!("SELECT to_timestamp({literal}::float8)::text"),
                crate_answer(seconds),
            )
        })
        .collect();
    compare(&mut conn, cases);
}
