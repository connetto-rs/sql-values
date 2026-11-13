//! `double precision` read as `float8in` reads it, printed as `float8out` prints it and ordered as
//! `float8_cmp_internal` orders it, every answer and every refusal's message measured on
//! PostgreSQL 18.

#[path = "support/answers.rs"]
mod answers;
#[path = "support/containers.rs"]
mod containers;
#[path = "support/exit_slot.rs"]
mod exit_slot;
#[path = "support/postgres.rs"]
mod postgres;

use answers::{Answer, compare, quoted};
use sql_values::postgres::{Float8Error, PgFloat8};

static PG18: postgres::Slot = exit_slot::ExitSlot::new();

const PG18_TAG: &str = "18.6";

/// Spellings across both notations, both output forms and the boundary between them, the
/// subnormal and overflow edges, every special value, hexadecimal and the refusals.
const INPUTS: &[&str] = &[
    "0",
    "-0",
    "+0.0",
    "1",
    "-1.5",
    "  3.25  ",
    "\t4\n",
    "2.5",
    "0.1",
    "0.3",
    "100",
    ".5",
    "5.",
    "1e10",
    "1E-10",
    "1e+2",
    "0.0001",
    "0.00001",
    "0.000123",
    "1e-4",
    "1e-5",
    "1.5e-7",
    "100000000000000",
    "1000000000000000",
    "123456789012345.6",
    "12345678901234.5",
    "1e15",
    "1e16",
    "123456789012345678",
    "9007199254740993",
    "1234567890123456789012345",
    "3.141592653589793",
    "1e100",
    "1e308",
    "1.7976931348623157e308",
    "1.7976931348623159e308",
    "1e309",
    "-1e309",
    "1e-320",
    "4.9e-324",
    "2e-324",
    "1e-400",
    "-1e-400",
    "0e-400",
    "NaN",
    "nan",
    "-nan",
    "NaN(123)",
    "Infinity",
    "-Infinity",
    "+Infinity",
    "infinity",
    "inf",
    "-INF",
    "infin",
    "0x1p3",
    "0x1.8p1",
    "-0x10",
    "0x",
    "0x1p99999",
    "+5",
    "+-5",
    "--5",
    "",
    "   ",
    "abc",
    "1.2.3",
    "1e",
    "1e+",
    ".",
    "1_000",
    "1 2",
];

/// The crate's answer in PostgreSQL's words, the syntax refusal naming the input as PostgreSQL's
/// message does.
fn crate_answer(input: &str) -> Answer {
    let read = PgFloat8::parse(input);
    if let Err(error @ Float8Error::InvalidInputSyntax) = &read {
        return Answer::Error(format!("{error}: \"{input}\""));
    }
    Answer::of(read)
}

#[test]
fn float8_reads_and_prints_as_postgresql_does() {
    let mut conn = answers::connect(&PG18, PG18_TAG);
    let cases = INPUTS
        .iter()
        .map(|input| {
            let query = format!("SELECT ({}::float8)::text", quoted(input));
            (query, crate_answer(input))
        })
        .collect();
    compare(&mut conn, cases);
}

#[test]
fn float8_orders_as_postgresql_does() {
    let mut conn = answers::connect(&PG18, PG18_TAG);
    let values = [
        "-Infinity",
        "-1e308",
        "-1",
        "-0",
        "0",
        "4.9e-324",
        "0.1",
        "1",
        "1e308",
        "Infinity",
        "NaN",
    ];
    let cases = values
        .iter()
        .flat_map(|left| values.iter().map(move |right| (*left, *right)))
        .map(|(left, right)| {
            let query = format!(
                "SELECT ({l}::float8 < {r}::float8)::text || '|' || ({l}::float8 = {r}::float8)::text",
                l = quoted(left),
                r = quoted(right),
            );
            let (left, right) = (
                PgFloat8::parse(left).unwrap(),
                PgFloat8::parse(right).unwrap(),
            );
            (
                query,
                Answer::Value(format!("{}|{}", left < right, left == right)),
            )
        })
        .collect();
    compare(&mut conn, cases);
}
