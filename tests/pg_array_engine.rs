//! Array shapes printed as `array_out` prints them and ordered and equated as PostgreSQL's
//! comparison operators do, each shape and order checked against PostgreSQL 18.
//!
//! The shapes are built with `with_dims` and `new`, and the server's answer is printed for the
//! array's literal.

#[path = "support/containers.rs"]
mod containers;
#[path = "support/exit_slot.rs"]
mod exit_slot;
#[path = "support/pg18.rs"]
mod pg18;
#[path = "support/postgres.rs"]
mod postgres;

use std::{
    cmp::Ordering,
    hash::{BuildHasher, RandomState},
};

use diesel::PgConnection;
use pg18::{connect, pg_order, pg_text, quoted};
use sql_values::{ArrayDim, CastError, InputContext, PgArray, PgDate, PgTimestamp, PgValue};

/// One dimension of `lower` and `length`.
fn dim(lower: i32, length: u32) -> ArrayDim {
    ArrayDim::new(lower, length).expect("a bound")
}

/// The `integer` value for each of `values`, `None` kept.
fn ints(values: &[Option<i64>]) -> Vec<Option<PgValue>> {
    values
        .iter()
        .map(|value| value.map(PgValue::Integer))
        .collect()
}

/// The `text` value for each of `values`, `None` kept.
fn texts(values: &[Option<&str>]) -> Vec<Option<PgValue>> {
    values
        .iter()
        .map(|value| value.map(|text| PgValue::Text(text.into())))
        .collect()
}

/// The mismatch, recorded in `mismatches`, when the crate's `got` for `expression` differs from
/// PostgreSQL's answer.
fn check(
    conn: &mut PgConnection,
    mismatches: &mut Vec<String>,
    expression: &str,
    got: Result<String, CastError>,
) {
    let expected = pg_text(conn, expression);
    if got != expected {
        mismatches.push(format!(
            "{expression}: crate {got:?}, PostgreSQL {expected:?}"
        ));
    }
}

/// The shape `expression` holds on PostgreSQL 18.6 beside the crate's array for it.
fn shapes() -> Vec<(&'static str, PgArray)> {
    let day = PgDate::parse("2026-01-01", InputContext::new()).expect("a date");
    let min_day = PgDate::parse("4714-11-24 BC", InputContext::new()).expect("a date");
    let instant =
        PgTimestamp::parse("2026-01-01 23:00:00.5", InputContext::new()).expect("a timestamp");
    vec![
        (
            "array[1]",
            PgArray::with_dims(vec![dim(1, 1)], ints(&[Some(1)])).unwrap(),
        ),
        (
            "array[1,2,3]",
            PgArray::with_dims(vec![dim(1, 3)], ints(&[Some(1), Some(2), Some(3)])).unwrap(),
        ),
        (
            "array[NULL,1,2]",
            PgArray::with_dims(vec![dim(1, 3)], ints(&[None, Some(1), Some(2)])).unwrap(),
        ),
        (
            "'[0:1]={1,2}'::int[]",
            PgArray::with_dims(vec![dim(0, 2)], ints(&[Some(1), Some(2)])).unwrap(),
        ),
        (
            "'[0:0]={7}'::int[]",
            PgArray::with_dims(vec![dim(0, 1)], ints(&[Some(7)])).unwrap(),
        ),
        (
            "'[2:3]={1,2}'::int[]",
            PgArray::with_dims(vec![dim(2, 2)], ints(&[Some(1), Some(2)])).unwrap(),
        ),
        (
            "'[-2:-1]={1,2}'::int[]",
            PgArray::with_dims(vec![dim(-2, 2)], ints(&[Some(1), Some(2)])).unwrap(),
        ),
        (
            "array[array[1,2],array[3,4]]",
            PgArray::with_dims(
                vec![dim(1, 2), dim(1, 2)],
                ints(&[Some(1), Some(2), Some(3), Some(4)]),
            )
            .unwrap(),
        ),
        (
            "'[1:2][0:1]={{1,2},{3,4}}'::int[]",
            PgArray::with_dims(
                vec![dim(1, 2), dim(0, 2)],
                ints(&[Some(1), Some(2), Some(3), Some(4)]),
            )
            .unwrap(),
        ),
        (
            "array[array[array[1,2]],array[array[3,4]]]",
            PgArray::with_dims(
                vec![dim(1, 2), dim(1, 1), dim(1, 2)],
                ints(&[Some(1), Some(2), Some(3), Some(4)]),
            )
            .unwrap(),
        ),
        (
            "'{{{{{{1}}}}}}'::int[1][1][1][1][1][1]",
            PgArray::with_dims(vec![dim(1, 1); 6], ints(&[Some(1)])).unwrap(),
        ),
        ("'{}'::int[]", PgArray::new(Vec::new())),
        (
            "'{}'::int[1][2]",
            PgArray::with_dims(vec![dim(1, 0)], Vec::new()).unwrap(),
        ),
        (
            r#"array['a b','a"b','a\b','"','NULL','null','','a,b','a{b',' a','a ','1','nUll',NULL]"#,
            PgArray::with_dims(
                vec![dim(1, 14)],
                texts(&[
                    Some("a b"),
                    Some("a\"b"),
                    Some("a\\b"),
                    Some("\""),
                    Some("NULL"),
                    Some("null"),
                    Some(""),
                    Some("a,b"),
                    Some("a{b"),
                    Some(" a"),
                    Some("a "),
                    Some("1"),
                    Some("nUll"),
                    None,
                ]),
            )
            .unwrap(),
        ),
        (
            "array['2026-01-01'::date,'4714-11-24 BC'::date,NULL]",
            PgArray::with_dims(
                vec![dim(1, 3)],
                vec![Some(PgValue::Date(day)), Some(PgValue::Date(min_day)), None],
            )
            .unwrap(),
        ),
        (
            "array['2026-01-01 23:00:00.5'::timestamp,'4714-11-24 00:00:00 BC'::timestamp]",
            PgArray::with_dims(
                vec![dim(1, 2)],
                vec![
                    Some(PgValue::Timestamp(instant)),
                    Some(PgValue::Timestamp(
                        PgTimestamp::parse("4714-11-24 00:00:00 BC", InputContext::new())
                            .expect("a timestamp"),
                    )),
                ],
            )
            .unwrap(),
        ),
    ]
}

/// Every shape prints as `array_out` prints it under `DateStyle` `ISO`.
#[test]
fn pg18_prints_every_array_shape() {
    let mut conn = connect();
    let mut mismatches = Vec::new();
    for (expression, array) in shapes() {
        check(
            &mut conn,
            &mut mismatches,
            expression,
            Ok(array.to_string()),
        );
    }
    assert!(mismatches.is_empty(), "{}", mismatches.join("\n"));
}

/// The array shapes and their crate values, text under the `C` collation.
fn ordered() -> Vec<(&'static str, String, PgArray)> {
    let entries = [
        ("int[]", "{}", PgArray::new(Vec::new())),
        ("int[]", "{{}}", PgArray::new(Vec::new())),
        (
            "int[]",
            "{1}",
            PgArray::new(vec![Some(PgValue::Integer(1))]),
        ),
        (
            "int[]",
            "{1,2}",
            PgArray::new(vec![Some(PgValue::Integer(1)), Some(PgValue::Integer(2))]),
        ),
        (
            "int[]",
            "{1,2,3}",
            PgArray::new(vec![
                Some(PgValue::Integer(1)),
                Some(PgValue::Integer(2)),
                Some(PgValue::Integer(3)),
            ]),
        ),
        (
            "int[]",
            "{0}",
            PgArray::new(vec![Some(PgValue::Integer(0))]),
        ),
        (
            "int[]",
            "{2}",
            PgArray::new(vec![Some(PgValue::Integer(2))]),
        ),
        (
            "int[]",
            "{-1}",
            PgArray::new(vec![Some(PgValue::Integer(-1))]),
        ),
        (
            "int[]",
            "{10}",
            PgArray::new(vec![Some(PgValue::Integer(10))]),
        ),
        (
            "int[]",
            "{1,10}",
            PgArray::new(vec![Some(PgValue::Integer(1)), Some(PgValue::Integer(10))]),
        ),
        ("int[]", "{NULL}", PgArray::new(vec![None])),
        (
            "int[]",
            "{1,NULL}",
            PgArray::new(vec![Some(PgValue::Integer(1)), None]),
        ),
        (
            "int[]",
            "{NULL,1}",
            PgArray::new(vec![None, Some(PgValue::Integer(1))]),
        ),
        (
            "int[]",
            "{NULL,2}",
            PgArray::new(vec![None, Some(PgValue::Integer(2))]),
        ),
        (
            "int[]",
            "{{1,2},{3,4}}",
            PgArray::with_dims(
                vec![dim(1, 2), dim(1, 2)],
                ints(&[Some(1), Some(2), Some(3), Some(4)]),
            )
            .unwrap(),
        ),
        (
            "int[]",
            "{{1,2},{3,5}}",
            PgArray::with_dims(
                vec![dim(1, 2), dim(1, 2)],
                ints(&[Some(1), Some(2), Some(3), Some(5)]),
            )
            .unwrap(),
        ),
        (
            "int[]",
            "{{1}}",
            PgArray::with_dims(vec![dim(1, 1), dim(1, 1)], ints(&[Some(1)])).unwrap(),
        ),
        (
            "int[]",
            "{{1,2}}",
            PgArray::with_dims(vec![dim(1, 1), dim(1, 2)], ints(&[Some(1), Some(2)])).unwrap(),
        ),
        (
            "int[]",
            "{{{1,2}}}",
            PgArray::with_dims(
                vec![dim(1, 1), dim(1, 1), dim(1, 2)],
                ints(&[Some(1), Some(2)]),
            )
            .unwrap(),
        ),
        (
            "int[]",
            "[0:0]={7}",
            PgArray::with_dims(vec![dim(0, 1)], ints(&[Some(7)])).unwrap(),
        ),
        (
            "int[]",
            "[0:1]={1,2}",
            PgArray::with_dims(vec![dim(0, 2)], ints(&[Some(1), Some(2)])).unwrap(),
        ),
        (
            "int[]",
            "[1:2]={1,2}",
            PgArray::with_dims(vec![dim(1, 2)], ints(&[Some(1), Some(2)])).unwrap(),
        ),
        (
            "int[]",
            "[2:3]={1,2}",
            PgArray::with_dims(vec![dim(2, 2)], ints(&[Some(1), Some(2)])).unwrap(),
        ),
        (
            "int[]",
            "[-2:-1]={1,2}",
            PgArray::with_dims(vec![dim(-2, 2)], ints(&[Some(1), Some(2)])).unwrap(),
        ),
        (
            "text[]",
            "{a}",
            PgArray::new(vec![Some(PgValue::Text("a".into()))]),
        ),
        (
            "text[]",
            "{\"a\"}",
            PgArray::new(vec![Some(PgValue::Text("a".into()))]),
        ),
        (
            "text[]",
            "{a,b}",
            PgArray::new(texts(&[Some("a"), Some("b")])),
        ),
        (
            "text[]",
            "{a,b,c}",
            PgArray::new(texts(&[Some("a"), Some("b"), Some("c")])),
        ),
        (
            "text[]",
            "{b}",
            PgArray::new(vec![Some(PgValue::Text("b".into()))]),
        ),
        (
            "text[]",
            "{A}",
            PgArray::new(vec![Some(PgValue::Text("A".into()))]),
        ),
        (
            "text[]",
            "{a b}",
            PgArray::new(vec![Some(PgValue::Text("a b".into()))]),
        ),
        (
            "text[]",
            "{\"a b\"}",
            PgArray::new(vec![Some(PgValue::Text("a b".into()))]),
        ),
        ("text[]", "{NULL}", PgArray::new(vec![None])),
        ("text[]", "{nUlL}", PgArray::new(vec![None])),
        (
            "text[]",
            "{NULL,a}",
            PgArray::new(texts(&[None, Some("a")])),
        ),
        (
            "text[]",
            "{a,NULL}",
            PgArray::new(texts(&[Some("a"), None])),
        ),
        (
            "text[]",
            "{a,a}",
            PgArray::new(texts(&[Some("a"), Some("a")])),
        ),
        (
            "text[]",
            "{\"\"}",
            PgArray::new(vec![Some(PgValue::Text(String::new()))]),
        ),
    ];
    entries
        .into_iter()
        .map(|(type_name, literal, array)| {
            let collation = if type_name == "text[]" {
                " COLLATE \"C\""
            } else {
                ""
            };
            (
                type_name,
                format!("{}::{type_name}{collation}", quoted(literal)),
                array,
            )
        })
        .collect()
}

/// Every pair of one element type orders and equates as PostgreSQL's operators order and
/// equate it, and equal pairs hash equal. PostgreSQL has no operator between arrays of
/// different element types.
#[test]
fn pg18_orders_and_equates_every_array_pair() {
    let arrays = ordered();
    let mut conn = connect();
    let hasher = RandomState::new();
    let mut mismatches = Vec::new();
    for (left_type, left_sql, left) in &arrays {
        for (right_type, right_sql, right) in &arrays {
            if left_type != right_type {
                continue;
            }
            let got = left.cmp(right);
            let expected = pg_order(&mut conn, left_sql, right_sql).unwrap();
            if got != expected {
                mismatches.push(format!(
                    "{left_sql} vs {right_sql}: crate {got:?}, PostgreSQL {expected:?}",
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
