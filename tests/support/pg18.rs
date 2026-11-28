//! A PostgreSQL 18.6 session and the answers it gives, shared by the engine tests.
//!
//! Include in a test binary next to `containers`, `exit_slot` and `postgres`:
//!
//! ```rust,ignore
//! #[path = "support/pg18.rs"]
//! mod pg18;
//! ```

use core::cmp::Ordering;

use diesel::{
    dsl::sql,
    prelude::*,
    result::Error as DieselError,
    sql_types::{Bool, Integer, Nullable, Text},
};
use sql_values::postgres::{CastError, JsonbError, JsonbKind, PgType};

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

/// The `jsonb_in` refusal PostgreSQL reports with `message` and `detail`, or `None` for another
/// message.
pub fn pg_jsonb_error(message: &str, detail: Option<&str>) -> Option<JsonbError> {
    if message != "invalid input syntax for type json"
        && message != "unsupported Unicode escape sequence"
    {
        return None;
    }
    let detail = detail.unwrap_or_else(|| panic!("PostgreSQL raised {message:?} without detail"));
    let errors = [
        ("Token \"", JsonbError::InvalidToken),
        ("Escape sequence \"", JsonbError::InvalidEscape),
        (
            "Character with value ",
            JsonbError::UnescapedControlCharacter,
        ),
        (
            "\"\\u\" must be followed by four hexadecimal digits.",
            JsonbError::UnicodeEscapeFormat,
        ),
        (
            "Unicode high surrogate must not follow a high surrogate.",
            JsonbError::HighSurrogateRepeated,
        ),
        (
            "Unicode low surrogate must follow a high surrogate.",
            JsonbError::LowSurrogateExpected,
        ),
        (
            "\\u0000 cannot be converted to text.",
            JsonbError::CodePointZero,
        ),
        ("Expected JSON value, but found ", JsonbError::ExpectedValue),
        ("Expected string, but found ", JsonbError::ExpectedString),
        (
            "Expected string or \"}\", but found ",
            JsonbError::ExpectedStringOrObjectEnd,
        ),
        ("Expected \":\", but found ", JsonbError::ExpectedColon),
        (
            "Expected \",\" or \"}\", but found ",
            JsonbError::ExpectedCommaOrObjectEnd,
        ),
        (
            "Expected \",\" or \"]\", but found ",
            JsonbError::ExpectedCommaOrArrayEnd,
        ),
        ("Expected end of input, but found ", JsonbError::ExpectedEnd),
        (
            "The input string ended unexpectedly.",
            JsonbError::UnexpectedEnd,
        ),
    ];
    let error = errors
        .into_iter()
        .find(|(prefix, _)| detail.starts_with(prefix))
        .map(|(_, error)| error)
        .unwrap_or_else(|| panic!("PostgreSQL raised {message:?} with detail {detail:?}"));
    Some(error)
}

/// The crate's error for a message PostgreSQL raises, which names the input in quotes, and its
/// detail.
pub fn pg_error(message: &str, detail: Option<&str>) -> CastError {
    if let Some(error) = pg_jsonb_error(message, detail) {
        return CastError::Jsonb(error);
    }
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
        PgType::Time,
        PgType::TimeTz,
        PgType::Jsonb,
        PgType::Uuid,
        PgType::Interval,
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
        CastError::IntervalFieldValueOutOfRange,
        CastError::IntervalOutOfRange,
        CastError::CannotConvertInfiniteIntervalToTime,
    ];
    let kinds = [
        JsonbKind::Null,
        JsonbKind::String,
        JsonbKind::Numeric,
        JsonbKind::Boolean,
        JsonbKind::Array,
        JsonbKind::Object,
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
        for from in kinds {
            errors.push(CastError::CannotCastJsonb { from, to: pg_type });
        }
    }
    errors
        .into_iter()
        .find(|error| error.to_string() == stem)
        .unwrap_or_else(|| panic!("PostgreSQL raised {message:?}"))
}

/// The text `expression` prints through its type's output function, `None` for `NULL`, or the
/// error it raises.
pub fn pg_nullable_text(
    conn: &mut PgConnection,
    expression: &str,
) -> Result<Option<String>, CastError> {
    // Raw SQL with the literal inline, since a bound parameter is typed `text` and casts as
    // `text` does, never as the unknown literal under test.
    let printed =
        format!("CASE WHEN ({expression}) IS NULL THEN NULL ELSE format('%s', {expression}) END");
    match diesel::select(sql::<Nullable<Text>>(&printed)).get_result(conn) {
        Ok(text) => Ok(text),
        Err(DieselError::DatabaseError(_, info)) => Err(pg_error(info.message(), info.details())),
        Err(other) => panic!("{expression}: {other}"),
    }
}

/// The text `expression` prints through its type's output function, empty for `NULL` as
/// `format` prints it, or the error it raises.
pub fn pg_text(conn: &mut PgConnection, expression: &str) -> Result<String, CastError> {
    pg_nullable_text(conn, expression).map(Option::unwrap_or_default)
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
