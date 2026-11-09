//! A PostgreSQL 18.6 session and the answers it gives, shared by the engine tests.
//!
//! Include in a test binary next to `containers`, `exit_slot` and `postgres`:
//!
//! ```rust,ignore
//! #[path = "support/pg18.rs"]
//! mod pg18;
//! ```

use std::cmp::Ordering;

use diesel::{
    dsl::sql,
    prelude::*,
    result::Error as DieselError,
    sql_types::{Bool, Integer, Text},
};
use sql_values::{CastError, PgType};

use crate::{exit_slot::ExitSlot, postgres};

/// The 18.6 container's slot, held for the whole test binary.
pub static PG18: postgres::Slot = ExitSlot::new();

/// The image's tag.
pub const PG18_TAG: &str = "18.6";

define_sql_function!(fn set_config(setting: Text, value: Text, is_local: Bool) -> Text);

/// A PostgreSQL 18.6 session under `TimeZone` `UTC`.
pub fn connect() -> PgConnection {
    let mut conn = postgres::connect(&PG18, PG18_TAG);
    let _: String = diesel::select(set_config("TimeZone", "UTC", false))
        .get_result(&mut conn)
        .expect("set TimeZone");
    conn
}

/// `text` quoted as an SQL string literal.
pub fn quoted(text: &str) -> String {
    format!("'{}'", text.replace('\'', "''"))
}

/// The crate's error for a message PostgreSQL raises, which names the input in quotes.
pub fn pg_error(message: &str) -> CastError {
    let stem = match message.strip_prefix("value \"") {
        Some(rest) => {
            let (_, type_name) = rest
                .split_once("\" is out of range for type ")
                .unwrap_or_else(|| panic!("PostgreSQL raised {message:?}"));
            format!("value is out of range for type {type_name}")
        }
        None => message
            .split_once(": \"")
            .map_or(message, |(stem, _)| stem)
            .to_owned(),
    };
    let types = [
        PgType::Boolean,
        PgType::Smallint,
        PgType::Integer,
        PgType::Bigint,
        PgType::Numeric,
        PgType::Text,
        PgType::Varchar,
        PgType::Bpchar,
        PgType::Date,
        PgType::Timestamp,
        PgType::TimestampTz,
    ];
    let mut errors = vec![
        CastError::MalformedArrayLiteral,
        CastError::ArrayDimensionsExceeded,
        CastError::UpperBoundLessThanLower,
        CastError::ArrayUpperBoundTooLarge(i32::MAX),
        CastError::ArraySizeExceeded,
        CastError::ArrayBoundOutOfRange,
        CastError::NumericFieldOverflow,
        CastError::NumericFormatOverflow,
        CastError::DateTimeFieldOutOfRange,
        CastError::DateOutOfRange,
        CastError::TimestampOutOfRange,
        CastError::DateOutOfRangeForTimestamp,
        CastError::TimeZoneDisplacementOutOfRange,
    ];
    for pg_type in types {
        errors.push(CastError::InvalidInputSyntax(pg_type));
        errors.push(CastError::ValueOutOfRange(pg_type));
        errors.push(CastError::OutOfRange(pg_type));
        errors.push(CastError::CannotConvertNaN(pg_type));
        errors.push(CastError::CannotConvertInfinity(pg_type));
        for from in types {
            errors.push(CastError::CannotCast { from, to: pg_type });
        }
    }
    errors
        .into_iter()
        .find(|error| error.to_string() == stem)
        .unwrap_or_else(|| panic!("PostgreSQL raised {message:?}"))
}

/// The text `expression` prints through its type's output function, or the error it raises.
pub fn pg_text(conn: &mut PgConnection, expression: &str) -> Result<String, CastError> {
    // Raw SQL with the literal inline, since a bound parameter is typed `text` and casts as
    // `text` does, never as the unknown literal under test.
    match diesel::select(sql::<Text>(&format!("format('%s', {expression})"))).get_result(conn) {
        Ok(text) => Ok(text),
        Err(DieselError::DatabaseError(_, info)) => Err(pg_error(info.message())),
        Err(other) => panic!("{expression}: {other}"),
    }
}

/// The order PostgreSQL's `<` and `=` give `left` and `right`, or `None` when it has no operator
/// between their types.
pub fn pg_order(conn: &mut PgConnection, left: &str, right: &str) -> Option<Ordering> {
    let expression =
        format!("CASE WHEN {left} < {right} THEN -1 WHEN {left} = {right} THEN 0 ELSE 1 END");
    match diesel::select(sql::<Integer>(&expression)).get_result::<i32>(conn) {
        Ok(order) => Some(order.cmp(&0)),
        Err(DieselError::DatabaseError(_, info))
            if info.message().starts_with("operator does not exist") =>
        {
            None
        }
        Err(other) => panic!("{expression}: {other}"),
    }
}
