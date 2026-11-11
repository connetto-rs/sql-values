//! `bytea` read as `byteain` reads it, printed as `byteaout` prints it under both settings of
//! `bytea_output`, and ordered as `byteacmp` orders it, every answer and every refusal's message
//! measured on PostgreSQL 18.

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
    RunQueryDsl,
    sql_types::{Bool, Text},
};
use sql_values::PgBytea;

static PG18: postgres::Slot = exit_slot::ExitSlot::new();

const PG18_TAG: &str = "18.6";

diesel::define_sql_function! {
    /// `set_config(setting, value, is_local)`, the function form of `SET`.
    fn set_config(setting: Text, value: Text, is_local: Bool) -> Text;
}

/// Inputs for both of `byteain`'s formats, with whitespace between and inside hex pairs, every
/// refusal, a non-ASCII digit, and hex long enough for PostgreSQL's vectorised decoder.
fn inputs() -> Vec<String> {
    let every_byte: String = (0..=255_u8).map(|byte| format!("{byte:02x}")).collect();
    let long = "0123456789abcdefABCDEF0123456789abcdef0123456789abcdef";
    let mut inputs: Vec<String> = [
        r"\x",
        r"\x00",
        r"\xDEADbeef",
        r"\x de ad ",
        "\\x\tde\nad\rbe ef",
        r"\x d e",
        r"\x0",
        r"\x0g",
        r"\xg0",
        r"\x00 0",
        r"\x4 ",
        r"\xé0",
        r"\x0é",
        r"\X41",
        r" \x41",
        "",
        "abc",
        r"a\\b",
        r"\101",
        r"\000",
        r"\377",
        r"\400",
        r"\18",
        r"\1",
        r"\\\\",
        r"\",
        r"x\",
        "héllo",
        "tab\there",
        "it's",
        "\u{7f}~ ",
    ]
    .map(str::to_owned)
    .into();
    inputs.push(format!(r"\x{every_byte}"));
    inputs.push(format!(r"\x{long}"));
    inputs.push(format!(r"\x{long}{long}"));
    inputs.push(format!(r"\x{long} {long}"));
    inputs.push(format!(r"\x{long}{long}0"));
    inputs.push(format!(r"\x{}g{long}", &long[..40]));
    inputs.push(format!(r"\x{long}{}g0", &long[..10]));
    inputs
}

/// The crate's printed value or refusal for `input` under either output format.
fn crate_answer(input: &str, escape: bool) -> Answer {
    Answer::of(PgBytea::parse(input).map(|bytea| {
        if escape {
            bytea.escaped().to_string()
        } else {
            bytea.to_string()
        }
    }))
}

#[test]
fn bytea_reads_and_prints_as_postgresql_does() {
    let mut conn = answers::connect(&PG18, PG18_TAG);
    for escape in [false, true] {
        let output = if escape { "escape" } else { "hex" };
        diesel::select(set_config("bytea_output", output, false))
            .execute(&mut conn)
            .expect("set bytea_output");
        let cases = inputs()
            .into_iter()
            .map(|input| {
                let query = format!("SELECT format('%s', {}::bytea)", quoted(&input));
                (query, crate_answer(&input, escape))
            })
            .collect();
        compare(&mut conn, cases);
    }
}

#[test]
fn bytea_orders_as_postgresql_does() {
    let mut conn = answers::connect(&PG18, PG18_TAG);
    let values = [
        r"\x", r"\x00", r"\x0000", r"\x01", r"\x7f", r"\x80", r"\xff", r"\xff00", r"\x00ff",
    ];
    let cases = values
        .iter()
        .flat_map(|left| values.iter().map(move |right| (*left, *right)))
        .map(|(left, right)| {
            let query = format!(
                "SELECT ({l}::bytea < {r}::bytea)::text || '|' || ({l}::bytea = {r}::bytea)::text",
                l = quoted(left),
                r = quoted(right),
            );
            let (left, right) = (
                PgBytea::parse(left).unwrap(),
                PgBytea::parse(right).unwrap(),
            );
            (
                query,
                Answer::Value(format!("{}|{}", left < right, left == right)),
            )
        })
        .collect();
    compare(&mut conn, cases);
}
