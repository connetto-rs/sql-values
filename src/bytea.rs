//! PostgreSQL 18's `bytea`, read as `byteain` reads its hex and escape formats and printed as
//! `byteaout` prints it under either setting of `bytea_output`.

use alloc::vec::Vec;
use core::fmt;

/// Why `byteain` refuses a text, one variant per message it raises.
///
/// ```
/// use sql_values::{ByteaError, PgBytea};
///
/// assert_eq!(PgBytea::parse(r"\x0"), Err(ByteaError::OddHexDigits));
/// assert_eq!(PgBytea::parse(r"\x0g"), Err(ByteaError::InvalidHexDigit('g')));
/// assert_eq!(PgBytea::parse(r"\9"), Err(ByteaError::InvalidInputSyntax));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, thiserror::Error)]
pub enum ByteaError {
    /// A backslash of the escape format not followed by `\` or three octal digits up to `\377`.
    #[error("invalid input syntax for type bytea")]
    InvalidInputSyntax,
    /// The first character of the hex format that is no digit and no whitespace between pairs.
    #[error("invalid hexadecimal digit: \"{0}\"")]
    InvalidHexDigit(char),
    /// Hex digits that end on half a byte.
    #[error("invalid hexadecimal data: odd number of digits")]
    OddHexDigits,
}

/// A PostgreSQL `bytea`, ordered as `byteacmp` orders it and printed as `byteaout` prints it
/// under the default `bytea_output` of `hex`.
///
/// ```
/// use sql_values::PgBytea;
///
/// let bytea = |text| PgBytea::parse(text).unwrap();
/// assert_eq!(bytea(r"\x de AD "), bytea(r"\336\255"));
/// assert_eq!(bytea(r"a\\b").as_bytes(), b"a\\b");
/// assert_eq!(bytea("hi").to_string(), r"\x6869");
/// assert_eq!(PgBytea::from(b"hi".to_vec()), bytea(r"\x6869"));
/// assert_eq!(bytea(r"\150\151").into_bytes(), b"hi");
/// assert_eq!(bytea(r"\x00415c").escaped().to_string(), r"\000A\\");
/// assert!(bytea(r"\x00") < bytea(r"\x0000"));
/// assert!(bytea(r"\x0000") < bytea(r"\x01"));
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PgBytea(Vec<u8>);

impl PgBytea {
    /// Read `text` as `byteain` reads it, hex after a leading `\x`, else the escape format.
    ///
    /// # Errors
    ///
    /// The [`ByteaError`] whose message PostgreSQL raises for `text`.
    pub fn parse(text: &str) -> Result<Self, ByteaError> {
        match text.strip_prefix(r"\x") {
            Some(digits) => read_hex(digits),
            None => read_escape(text.as_bytes()),
        }
        .map(Self)
    }

    /// The bytes.
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// The bytes, owned.
    pub fn into_bytes(self) -> Vec<u8> {
        self.0
    }

    /// The value as `byteaout` prints it under `bytea_output` `escape`.
    pub fn escaped(&self) -> impl fmt::Display + '_ {
        Escaped(&self.0)
    }
}

impl From<Vec<u8>> for PgBytea {
    fn from(bytes: Vec<u8>) -> Self {
        Self(bytes)
    }
}

impl fmt::Display for PgBytea {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(r"\x")?;
        self.0.iter().try_for_each(|byte| write!(f, "{byte:02x}"))
    }
}

/// [`PgBytea::escaped`]'s printing.
struct Escaped<'a>(&'a [u8]);

impl fmt::Display for Escaped<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.iter().try_for_each(|&byte| match byte {
            b'\\' => f.write_str(r"\\"),
            b' '..=b'~' => write!(f, "{}", char::from(byte)),
            _ => write!(f, "\\{byte:03o}"),
        })
    }
}

/// `hex_decode_safe` over the digits after `\x`.
fn read_hex(digits: &str) -> Result<Vec<u8>, ByteaError> {
    let bytes = digits.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len() / 2);
    let mut at = 0;
    while at < bytes.len() {
        if matches!(bytes[at], b' ' | b'\t' | b'\n' | b'\r') {
            at += 1;
            continue;
        }
        let high = hex_value(digits, at)?;
        if at + 1 == bytes.len() {
            return Err(ByteaError::OddHexDigits);
        }
        let low = hex_value(digits, at + 1)?;
        decoded.push((high << 4) | low);
        at += 2;
    }
    Ok(decoded)
}

/// The value of the hex digit at byte `at` of `digits`, which every caller reaches over ASCII
/// alone, so `at` starts a character.
fn hex_value(digits: &str, at: usize) -> Result<u8, ByteaError> {
    char::from(digits.as_bytes()[at])
        .to_digit(16)
        .and_then(|value| u8::try_from(value).ok())
        .ok_or_else(|| ByteaError::InvalidHexDigit(digits[at..].chars().next().unwrap_or_default()))
}

/// `byteain`'s escape format.
fn read_escape(text: &[u8]) -> Result<Vec<u8>, ByteaError> {
    let mut decoded = Vec::with_capacity(text.len());
    let mut rest = text;
    loop {
        rest = match rest {
            [] => return Ok(decoded),
            [
                b'\\',
                high @ b'0'..=b'3',
                middle @ b'0'..=b'7',
                low @ b'0'..=b'7',
                tail @ ..,
            ] => {
                decoded.push(((high - b'0') << 6) | ((middle - b'0') << 3) | (low - b'0'));
                tail
            }
            [b'\\', b'\\', tail @ ..] => {
                decoded.push(b'\\');
                tail
            }
            [b'\\', ..] => return Err(ByteaError::InvalidInputSyntax),
            [byte, tail @ ..] => {
                decoded.push(*byte);
                tail
            }
        };
    }
}
