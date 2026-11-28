//! `uuid` read as `uuid_in` reads it, printed as `uuid_out` prints it, and ordered as `uuid_cmp`
//! orders it, every answer and every refusal's message measured on PostgreSQL 18.

#[path = "support/answers.rs"]
mod answers;
#[path = "support/containers.rs"]
mod containers;
#[path = "support/exit_slot.rs"]
mod exit_slot;
#[path = "support/postgres.rs"]
mod postgres;

use answers::{Answer, compare, quoted};
use sql_values::postgres::PgUuid;

static PG18: postgres::Slot = exit_slot::ExitSlot::new();

const PG18_TAG: &str = "18.6";

const INPUTS: &[&str] = &[
    "a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11",
    "A0EEBC99-9C0B-4EF8-BB6D-6BB9BD380A11",
    "A0eEbC99-9c0B-4Ef8-bB6d-6Bb9bD380a11",
    "{a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11}",
    "{A0EEBC99-9C0B-4EF8-BB6D-6BB9BD380A11}",
    "a0eebc999c0b4ef8bb6d6bb9bd380a11",
    "{a0eebc999c0b4ef8bb6d6bb9bd380a11}",
    "a0ee-bc99-9c0b-4ef8-bb6d-6bb9-bd38-0a11",
    "a0ee-bc99-9c0b4ef8-bb6d-6bb9-bd38-0a11",
    "a0eebc99-9c0b4ef8-bb6d6bb9-bd380a11",
    "00000000-0000-0000-0000-000000000000",
    "ffffffff-ffff-ffff-ffff-ffffffffffff",
    " a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11",
    "a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11 ",
    "a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11\n",
    "a0eebc9-99c0b-4ef8-bb6d-6bb9bd380a11",
    "a0eebc99--9c0b-4ef8-bb6d-6bb9bd380a11",
    "-a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11",
    "a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11-",
    "a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a-11",
    "{a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11",
    "a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11}",
    "{a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11-}",
    "{{a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11}}",
    "{a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11}}",
    "(a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11)",
    "a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a1",
    "a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a111",
    "a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a1100",
    "g0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11",
    "a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a1g",
    "a0eebc99_9c0b-4ef8-bb6d-6bb9bd380a11",
    "a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a1é",
    "é0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11",
    "0xa0eebc999c0b4ef8bb6d6bb9bd380a11",
    "urn:uuid:a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11",
    "",
    "{}",
    "-",
    "a",
];

/// The crate's printed value for `input`, or its refusal with the input PostgreSQL quotes after
/// the message.
fn crate_answer(input: &str) -> Answer {
    Answer::of(PgUuid::parse(input).map_err(|error| format!("{error}: \"{input}\"")))
}

#[test]
fn uuid_reads_and_prints_as_postgresql_does() {
    let mut conn = answers::connect(&PG18, PG18_TAG);
    let cases = INPUTS
        .iter()
        .map(|input| {
            let query = format!("SELECT format('%s', {}::uuid)", quoted(input));
            (query, crate_answer(input))
        })
        .collect();
    compare(&mut conn, cases);
}

#[test]
fn uuid_orders_as_postgresql_does() {
    let mut conn = answers::connect(&PG18, PG18_TAG);
    let values = [
        "00000000-0000-0000-0000-000000000000",
        "00000000-0000-0000-0000-000000000001",
        "00000000-0000-0000-0000-0000000000ff",
        "00000000-0000-0000-0000-000000000100",
        "7fffffff-ffff-ffff-ffff-ffffffffffff",
        "80000000-0000-0000-0000-000000000000",
        "a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11",
        "ffffffff-ffff-ffff-ffff-ffffffffffff",
    ];
    let cases = values
        .iter()
        .flat_map(|left| values.iter().map(move |right| (*left, *right)))
        .map(|(left, right)| {
            let query = format!(
                "SELECT ({l}::uuid < {r}::uuid)::text || '|' || ({l}::uuid = {r}::uuid)::text",
                l = quoted(left),
                r = quoted(right),
            );
            let (left, right) = (PgUuid::parse(left).unwrap(), PgUuid::parse(right).unwrap());
            (
                query,
                Answer::Value(format!("{}|{}", left < right, left == right)),
            )
        })
        .collect();
    compare(&mut conn, cases);
}
