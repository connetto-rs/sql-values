//! Arrays answered as PostgreSQL 18 answers them, each input by the text its output function
//! prints, each order by its comparison operators, each cast by the element's cast function,
//! and each refusal by the message PostgreSQL raises.

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
use sql_values::{
    ArrayDim, CastError, CastTarget, CharLength, InputContext, NumericTypmod, PgArray, PgDate,
    PgTimestamp, PgValue,
};

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

/// The array `literal` holds as `type_name`, or the panic naming the literal and the type.
fn parsed(literal: &str, target: CastTarget, type_name: &str) -> PgArray {
    PgArray::parse(literal, target)
        .unwrap_or_else(|error| panic!("{literal:?}::{type_name}: {error}"))
}

/// Element types and the SQL that names each.
fn element_targets() -> Vec<(&'static str, CastTarget)> {
    vec![
        ("smallint", CastTarget::Smallint),
        ("integer", CastTarget::Integer),
        ("bigint", CastTarget::Bigint),
        ("numeric", CastTarget::Numeric(None)),
        (
            "numeric(3,1)",
            CastTarget::Numeric(Some(NumericTypmod::new(3, 1).expect("a typmod"))),
        ),
        ("text", CastTarget::Text),
        (
            "varchar(3)",
            CastTarget::Varchar(Some(CharLength::new(3).expect("a length"))),
        ),
        ("date", CastTarget::Date),
        ("timestamp", CastTarget::Timestamp(None)),
        ("boolean", CastTarget::Boolean),
    ]
}

/// Array literals reaching every branch of `array_in`.
const LITERALS: &[&str] = &[
    // Empty and whitespace.
    "{}",
    "{{}}",
    "{ }",
    "",
    "  ",
    // Simple elements.
    "{1}",
    "{1,2}",
    "{1,2,3}",
    " { 1 , 2 } ",
    "{1,2} ",
    "{-1}",
    "{+5}",
    // NULL spellings.
    "{nUlL}",
    "{null }",
    "{ NULL }",
    "{NULL,1}",
    "{1,NULL,2}",
    // Integer values and refusals.
    "{40000}",
    "{2147483647}",
    "{-2147483648}",
    "{2147483648}",
    "{99999999999999999999999}",
    "{0x1F}",
    "{1_000}",
    "{1 2}",
    "{abc}",
    "{1.5}",
    "{1e2}",
    "{-}",
    "{t}",
    // Element quoting.
    "{\"\"}",
    "{\"1\"}",
    "{\" 1 \"}",
    "{\"a\"}",
    "{\"a\" , 1}",
    "{\"a\" \"b\"}",
    "{a\"b}",
    "{\"x\"y}",
    "{\"a\\b\"}",
    "{a\\ b c}",
    "{\\}",
    // Single quotes are characters.
    "{'a'}",
    // Junk and structure refusals.
    "{1,}",
    "{,1}",
    "{1,,2}",
    "{1,2}x",
    "{1,2}{}",
    "{1",
    "{\"a",
    "{",
    "{a\\",
    "{\"1\"{2}}",
    "{ {1}2 }",
    "{\"a\"",
    // Nesting.
    "{{1,2},{3,4}}",
    "{ {1,2}, {3} }",
    "{1,{2,3}}",
    "{{1,2},{3}}",
    "{{{{{{1}}}}}}",
    "{{{{{{{1}}}}}}}",
    // Explicit dimensions.
    "[0:1]={1,2}",
    "[2:3]={1,2}",
    "[-2:-1]={1,2}",
    "[+1:2]={1,2}",
    "[1:2]={1,2}",
    "[1:2]={1}",
    "[1:2][1:2]={{1,2},{3,4}}",
    "[1:1][1:2]={{1,2},{3,4}}",
    "[1:1][1:1]=",
    "[1:0]={1,2}",
    "[-1:2147483646]=",
    "[1:2147483647]=",
    "[2147483648:2]=",
    "[-2147483649:2]=",
    "[99999999999999999999:1]=",
    "[--:1]=",
    "[1][1][1][1][1][1][1]=",
    "[1][1][1][1][1][6]={1,2,3,4,5,6}",
    "[1:2]{1,2}",
    "[1] = {1}",
    "[1] [2] {1,2,3,4}",
    "[]",
    "[a]=",
    "[1.5:2]=",
    "[1+2:3]=",
    "[1_2:3]=",
    "[1:]=",
    "[ ]=",
    "{}[1:1]",
    "{1,2}[1:1]",
    // Dates and times as elements.
    "{2026-01-01}",
    "{2026-01-01,4714-11-24 BC}",
    "{2026-13-01}",
    "{infinity}",
    "{today}",
    "{2026-01-01 23:00}",
    "{2026-01-01 24:00}",
    "{294277-01-01}",
    "{4714-11-23 23:00-02 BC}",
    "{294276-12-31 23:59:59.999999}",
    // Numerics and booleans as elements.
    "{NaN}",
    "{Infinity}",
    "{1e131072}",
    "{12.345}",
    "{-0.00}",
    "{1_000.5}",
    "{t,f}",
    "{TRUE,false}",
    "{1,0}",
    "{tr}",
    "{of}",
    "{onn}",
];

/// Every literal under every element type, skipping the answers the crate declines.
#[test]
fn pg18_reads_every_array_literal_with_every_element_type() {
    let mut conn = connect();
    let mut mismatches = Vec::new();
    let mut answered = 0;
    let targets = element_targets();
    for literal in LITERALS {
        for &(type_name, target) in &targets {
            let got = match PgArray::parse(literal, target) {
                Err(CastError::Unsupported) => continue,
                parsed => parsed.map(|array| array.to_string()),
            };
            answered += 1;
            let expression = format!("{}::{type_name}[]", quoted(literal));
            check(&mut conn, &mut mismatches, &expression, got);
        }
    }
    assert!(mismatches.is_empty(), "{}", mismatches.join("\n"));
    assert!(answered > 500, "only {answered} arrays answered");
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

/// The array literals and their crate values, text under the `C` collation.
fn ordered() -> Vec<(&'static str, String, PgArray)> {
    let literals: &[(&str, CastTarget, &str)] = &[
        ("{}", CastTarget::Integer, "int[]"),
        ("{{}}", CastTarget::Integer, "int[]"),
        ("{1}", CastTarget::Integer, "int[]"),
        ("{1,2}", CastTarget::Integer, "int[]"),
        ("{1,2,3}", CastTarget::Integer, "int[]"),
        ("{0}", CastTarget::Integer, "int[]"),
        ("{2}", CastTarget::Integer, "int[]"),
        ("{-1}", CastTarget::Integer, "int[]"),
        ("{10}", CastTarget::Integer, "int[]"),
        ("{1,10}", CastTarget::Integer, "int[]"),
        ("{NULL}", CastTarget::Integer, "int[]"),
        ("{1,NULL}", CastTarget::Integer, "int[]"),
        ("{NULL,1}", CastTarget::Integer, "int[]"),
        ("{NULL,2}", CastTarget::Integer, "int[]"),
        ("{{1,2},{3,4}}", CastTarget::Integer, "int[]"),
        ("{{1,2},{3,5}}", CastTarget::Integer, "int[]"),
        ("{{1}}", CastTarget::Integer, "int[]"),
        ("{{1,2}}", CastTarget::Integer, "int[]"),
        ("{{{1,2}}}", CastTarget::Integer, "int[]"),
        ("[0:0]={7}", CastTarget::Integer, "int[]"),
        ("[0:1]={1,2}", CastTarget::Integer, "int[]"),
        ("[1:2]={1,2}", CastTarget::Integer, "int[]"),
        ("[2:3]={1,2}", CastTarget::Integer, "int[]"),
        ("[-2:-1]={1,2}", CastTarget::Integer, "int[]"),
        ("{a}", CastTarget::Text, "text[]"),
        ("{\"a\"}", CastTarget::Text, "text[]"),
        ("{a,b}", CastTarget::Text, "text[]"),
        ("{a,b,c}", CastTarget::Text, "text[]"),
        ("{b}", CastTarget::Text, "text[]"),
        ("{A}", CastTarget::Text, "text[]"),
        ("{a b}", CastTarget::Text, "text[]"),
        ("{\"a b\"}", CastTarget::Text, "text[]"),
        ("{NULL}", CastTarget::Text, "text[]"),
        ("{nUlL}", CastTarget::Text, "text[]"),
        ("{NULL,a}", CastTarget::Text, "text[]"),
        ("{a,NULL}", CastTarget::Text, "text[]"),
        ("{a,a}", CastTarget::Text, "text[]"),
        ("{\"\"}", CastTarget::Text, "text[]"),
    ];
    literals
        .iter()
        .map(|&(literal, target, type_name)| {
            let array = parsed(literal, target, type_name);
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

/// Every source array under every target element type, skipping the casts the crate declines.
#[test]
fn pg18_casts_every_array_to_every_element_type() {
    let length = |n| Some(CharLength::new(n).expect("a length"));
    let typmod = |p, s| Some(NumericTypmod::new(p, s).expect("a typmod"));
    let sources: &[(&str, &str, CastTarget)] = &[
        ("{1,2}", "int[]", CastTarget::Integer),
        ("{40000}", "int[]", CastTarget::Integer),
        ("{2147483647}", "int[]", CastTarget::Integer),
        ("{-2147483648}", "int[]", CastTarget::Integer),
        ("{NULL,2}", "int[]", CastTarget::Integer),
        ("{{1,2},{3,4}}", "int[]", CastTarget::Integer),
        ("[0:1][0:1]={{1,2},{3,4}}", "int[]", CastTarget::Integer),
        ("{32767,-32768}", "smallint[]", CastTarget::Smallint),
        (
            "{9223372036854775807,-9223372036854775808}",
            "bigint[]",
            CastTarget::Bigint,
        ),
        ("{1.5,-2.5}", "numeric[]", CastTarget::Numeric(None)),
        ("{NaN}", "numeric[]", CastTarget::Numeric(None)),
        ("{Infinity}", "numeric[]", CastTarget::Numeric(None)),
        ("{12.345}", "numeric[]", CastTarget::Numeric(None)),
        ("{1.5,abc,t,2026-01-01}", "text[]", CastTarget::Text),
        ("{\"a b\"}", "text[]", CastTarget::Text),
        ("{t,f}", "boolean[]", CastTarget::Boolean),
        ("{2026-01-01,4714-11-24 BC}", "date[]", CastTarget::Date),
        (
            "{2026-01-01 23:00:00.6}",
            "timestamp[]",
            CastTarget::Timestamp(None),
        ),
        (
            "{2026-01-01 10:00:00.5}",
            "varchar(5)[]",
            CastTarget::Varchar(length(5)),
        ),
    ];
    let targets: &[(&str, CastTarget)] = &[
        ("smallint[]", CastTarget::Smallint),
        ("integer[]", CastTarget::Integer),
        ("bigint[]", CastTarget::Bigint),
        ("numeric[]", CastTarget::Numeric(None)),
        ("numeric(2,1)[]", CastTarget::Numeric(typmod(2, 1))),
        ("text[]", CastTarget::Text),
        ("varchar(3)[]", CastTarget::Varchar(length(3))),
        ("date[]", CastTarget::Date),
        ("timestamp[]", CastTarget::Timestamp(None)),
        ("boolean[]", CastTarget::Boolean),
    ];
    let mut conn = connect();
    let mut mismatches = Vec::new();
    let mut answered = 0;
    for &(literal, source_name, source) in sources {
        let array = parsed(literal, source, source_name);
        for &(target_name, target) in targets {
            // PostgreSQL casts only `integer` to `boolean`, and the value model holds a
            // `smallint` element as an `integer`.
            if source_name == "smallint[]" && target_name == "boolean[]" {
                continue;
            }
            let got = match array.cast(target) {
                Err(CastError::Unsupported) | Err(CastError::CannotCast { .. }) => continue,
                casted => casted.map(|array| array.to_string()),
            };
            answered += 1;
            let expression = format!("({}::{source_name})::{target_name}", quoted(literal));
            check(&mut conn, &mut mismatches, &expression, got);
        }
    }
    assert!(mismatches.is_empty(), "{}", mismatches.join("\n"));
    assert!(answered > 100, "only {answered} casts answered");
}
