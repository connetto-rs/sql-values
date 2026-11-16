//! `jsonb` answered as PostgreSQL 18 answers it, each text by the text `jsonb_out` prints or the
//! refusal `jsonb_in` raises, and each pair by `jsonb`'s btree operators on a database whose
//! default collation is `C`.

#[path = "support/containers.rs"]
mod containers;
#[path = "support/exit_slot.rs"]
mod exit_slot;
#[path = "support/pg18.rs"]
mod pg18;
#[path = "support/postgres.rs"]
mod postgres;

use core::{cmp::Ordering, hash::BuildHasher};
use std::{hash::RandomState, sync::Once};

use diesel::{PgConnection, RunQueryDsl, sql_query};
use pg18::{PG18, PG18_TAG, connect, pg_order, pg_text, quoted};
use sql_values::postgres::{CastError, JsonbError, NumericError, PgJsonb, PgType};

/// The database whose default collation is `C`, which `jsonb` compares strings and keys under.
const C_DATABASE: &str = "jsonb_c";

/// A session on [`C_DATABASE`], created on first use.
fn connect_c() -> PgConnection {
    static CREATED: Once = Once::new();
    CREATED.call_once(|| {
        // `CREATE DATABASE` is DDL the query DSL has no form for.
        sql_query(format!(
            "CREATE DATABASE {C_DATABASE} TEMPLATE template0 LOCALE 'C'"
        ))
        .execute(&mut connect())
        .expect("create the C database");
    });
    postgres::connect_to(&PG18, PG18_TAG, C_DATABASE)
}

/// The text `'text'::jsonb` prints, or the refusal `jsonb_in` raises.
fn pg_jsonb(conn: &mut PgConnection, text: &str) -> Result<String, JsonbError> {
    pg_text(conn, &format!("{}::jsonb", quoted(text))).map_err(|error| match error {
        CastError::Jsonb(error) => error,
        CastError::NumericFormatOverflow => JsonbError::Numeric(NumericError::FormatOverflow),
        CastError::InvalidInputSyntax(PgType::Numeric) => {
            JsonbError::Numeric(NumericError::InvalidInputSyntax)
        }
        other => panic!("{text:?}::jsonb raised {other}"),
    })
}

/// Texts reaching every branch of `jsonb_in`, `jsonb_out` and each refusal.
const TEXTS: &[&str] = &[
    // Duplicate keys, key order and nesting.
    r#"{"bb":1,"a":2,"c":3,"a":4}"#,
    r#"{"aaa":1,"b":2,"aa":3,"ab":4,"a":5,"B":6}"#,
    r#"{"a":{"x":1,"x":2,"w":[]},"a":[1,{"b":null}],"c":{}}"#,
    r#"{"\u0061":1,"a":2}"#,
    r#"{"a":[{"b":null},{"a":1,"a":2}],"a":{"z":{"y":{"x":[[[]]]}}}}"#,
    r#"{"é":1,"z":2,"😀":3,"e":4,"ez":5}"#,
    // Blanks.
    " [ 1 , {\"b\" : null } ] ",
    "\t\r\n{\n\"a\"\t:\r[\n]\n}\n",
    // Numbers.
    "1e2",
    "1E-2",
    "-0.0",
    "0.000",
    "-0",
    "0e5",
    "1.0",
    "1.50",
    "-1E+3",
    "1e-20",
    "123456789012345678901234567890.123456789",
    "[1, 1.0, 1.00, 100e-2]",
    r#"{"a":1.0}"#,
    "1e-16383",
    "9e131071",
    // Scalars.
    "null",
    "true",
    "false",
    "[]",
    "{}",
    "[[[[[]]]]]",
    // Strings and escapes.
    r#""""#,
    r#""\u0001\b\t\u001f\u007f\u00e9 \ud83d\ude00 \/""#,
    r#""\"\\\/\b\f\n\r\t""#,
    r#""\uD834\uDD1E \u00FF \u0020""#,
    "\"日本 é \u{7f}\"",
    r#""\u001F""#,
    // Refused tokens.
    r#"{"a":1,}"#,
    "01",
    "-01",
    "tru",
    "truex",
    "nul",
    "True",
    "+1",
    ".5",
    "1.",
    "1.e5",
    "-",
    "-a",
    "1e",
    "1e+",
    "0x10",
    "NaN",
    "Infinity",
    "é",
    "\u{c}1",
    "'a'",
    "\"abc",
    "\"\\",
    // Refused escapes.
    r#""\u0000""#,
    r#""\ud83d""#,
    r#""\ud83d x""#,
    r#""\ud83d\n""#,
    r#""\ud83d\u0041""#,
    r#""\ude00""#,
    r#""\ud83d\ud83d""#,
    r#""\u12""#,
    r#""\u12x4""#,
    r#""\q""#,
    "\"a\nb\"",
    "\"a\u{1}b\"",
    r#"{"a":"\ud83d"}"#,
    // Refused structure.
    "",
    "   ",
    "[",
    "{",
    r#"{"a""#,
    r#"{"a":"#,
    "[1",
    "[1,",
    "[1 2]",
    "[1,,2]",
    "[,]",
    "[1,]",
    "{1:2}",
    "{,}",
    r#"{"a" 1}"#,
    r#"{"a":1 "b":2}"#,
    r#"{"a":1]"#,
    "[1}",
    "[1]]",
    "1 2",
    "{} {}",
    ":",
    // Numbers numeric_in refuses, and the order of the two refusals.
    "1e1000000",
    "1e-2000000000",
    "1e-16384",
    "[1e1000000,",
    "[1e1000000 x]",
    "1e1000000 01",
];

/// Every text reads and prints as `jsonb_in` and `jsonb_out` do, or is refused as `jsonb_in`
/// refuses it.
#[test]
fn pg18_reads_and_prints_every_text() {
    let mut conn = connect();
    let mismatches: Vec<String> = TEXTS
        .iter()
        .filter_map(|&text| {
            let got = PgJsonb::parse(text).map(|jsonb| jsonb.to_string());
            let expected = pg_jsonb(&mut conn, text);
            (got != expected).then(|| format!("{text:?}: crate {got:?}, PostgreSQL {expected:?}"))
        })
        .collect();
    assert!(mismatches.is_empty(), "{}", mismatches.join("\n"));
}

/// Values at the edges of each comparison, in no order.
const COMPARED: &[&str] = &[
    "[]",
    "null",
    r#""""#,
    r#""B""#,
    r#""a""#,
    r#""ab""#,
    r#""b""#,
    r#""é""#,
    "-1",
    "1",
    "1.0",
    "1.5",
    "1e100",
    "false",
    "true",
    "[null]",
    r#"["a"]"#,
    "[1]",
    "[1.00]",
    "[true]",
    "[[]]",
    "[{}]",
    "[1, 2]",
    "[2, 1]",
    "[1, [2]]",
    "[[1], 2]",
    "[[], 1]",
    "{}",
    r#"{"a": 1}"#,
    r#"{"a": 1.0}"#,
    r#"{"a": 2}"#,
    r#"{"a": null}"#,
    r#"{"a": []}"#,
    r#"{"a": {}}"#,
    r#"{"aa": 1}"#,
    r#"{"b": 1}"#,
    r#"{"B": 1}"#,
    r#"{"b": {}}"#,
    r#"{"a": 1, "b": 2}"#,
    r#"{"b": 1, "aa": 2}"#,
    r#"{"b": 2, "a": 1}"#,
    r#"{"a": [1, {"b": 2}], "c": 3}"#,
    r#"{"a": [1, {"b": 3}], "c": 3}"#,
];

/// Every pair of values orders as `jsonb`'s `<` and `=` order it under the `C` collation, and
/// equal pairs hash equal.
#[test]
fn pg18_compares_every_jsonb_pair() {
    let mut conn = connect_c();
    let hasher = RandomState::new();
    let values: Vec<(String, PgJsonb)> = COMPARED
        .iter()
        .map(|&text| {
            let value = PgJsonb::parse(text).unwrap_or_else(|error| panic!("{text:?}: {error}"));
            (format!("{}::jsonb", quoted(text)), value)
        })
        .collect();
    let mut mismatches = Vec::new();
    for (left_sql, left) in &values {
        for (right_sql, right) in &values {
            let got = left.cmp(right);
            let expected = pg_order(&mut conn, left_sql, right_sql);
            if Some(got) != expected {
                mismatches.push(format!(
                    "{left_sql} vs {right_sql}: crate {got:?}, PostgreSQL {expected:?}"
                ));
            }
            if got == Ordering::Equal && hasher.hash_one(left) != hasher.hash_one(right) {
                mismatches.push(format!(
                    "{left_sql} and {right_sql} are equal but hash apart"
                ));
            }
        }
    }
    assert!(mismatches.is_empty(), "{}", mismatches.join("\n"));
}
