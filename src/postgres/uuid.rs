//! PostgreSQL 18's `uuid`, read as `uuid_in` reads it and printed as `uuid_out` prints it.

use core::fmt;

/// Why `uuid_in` refuses a text, one variant per message it raises.
///
/// ```
/// use sql_values::postgres::{PgUuid, UuidError};
///
/// let error = PgUuid::parse("a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a1").unwrap_err();
/// assert_eq!(error, UuidError::InvalidInputSyntax);
/// assert_eq!(error.to_string(), "invalid input syntax for type uuid");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, thiserror::Error)]
pub enum UuidError {
    /// Text other than 32 hexadecimal digits, with at most one hyphen after each group of four
    /// but the last, optionally between `{` and `}`, and with nothing around them.
    #[error("invalid input syntax for type uuid")]
    InvalidInputSyntax,
}

/// A PostgreSQL `uuid`, ordered as `uuid_cmp` orders it, byte by byte, and printed as `uuid_out`
/// prints it, in lowercase 8-4-4-4-12 form.
///
/// ```
/// use sql_values::postgres::PgUuid;
///
/// let uuid = |text| PgUuid::parse(text).unwrap();
/// let canonical = "a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11";
/// for spelling in [
///     "A0EEBC99-9C0B-4EF8-BB6D-6BB9BD380A11",
///     "{a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11}",
///     "a0eebc999c0b4ef8bb6d6bb9bd380a11",
///     "a0ee-bc99-9c0b4ef8-bb6d-6bb9-bd38-0a11",
/// ] {
///     assert_eq!(uuid(spelling).to_string(), canonical);
/// }
/// for refused in [
///     " a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11",
///     "a0eebc9-99c0b-4ef8-bb6d-6bb9bd380a11",
///     "a0eebc99--9c0b-4ef8-bb6d-6bb9bd380a11",
///     "{a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11",
///     "a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11-",
/// ] {
///     assert!(PgUuid::parse(refused).is_err());
/// }
///
/// let bytes = [
///     0xa0, 0xee, 0xbc, 0x99, 0x9c, 0x0b, 0x4e, 0xf8, 0xbb, 0x6d, 0x6b, 0xb9, 0xbd, 0x38, 0x0a, 0x11,
/// ];
/// assert_eq!(PgUuid::from(bytes), uuid(canonical));
/// assert_eq!(uuid(canonical).as_bytes(), &bytes);
/// let low = uuid("00000000-0000-0000-0000-0000000000ff");
/// assert!(low < uuid("01000000-0000-0000-0000-000000000000"));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PgUuid([u8; 16]);

impl PgUuid {
    /// Read `text` as `string_to_uuid` reads it.
    ///
    /// # Errors
    ///
    /// [`UuidError::InvalidInputSyntax`] for text `uuid_in` refuses.
    pub fn parse(text: &str) -> Result<Self, UuidError> {
        let (mut rest, braced) = match text.as_bytes() {
            [b'{', rest @ ..] => (rest, true),
            rest => (rest, false),
        };
        let mut uuid = [0; 16];
        for (at, byte) in uuid.iter_mut().enumerate() {
            let [high, low, tail @ ..] = rest else {
                return Err(UuidError::InvalidInputSyntax);
            };
            let digit = |digit| crate::hex_digit(digit).ok_or(UuidError::InvalidInputSyntax);
            *byte = (digit(*high)? << 4) | digit(*low)?;
            rest = match tail {
                [b'-', after @ ..] if at % 2 == 1 && at < 15 => after,
                _ => tail,
            };
        }
        let rest = match (braced, rest) {
            (true, [b'}', rest @ ..]) => rest,
            (true, _) => return Err(UuidError::InvalidInputSyntax),
            (false, rest) => rest,
        };
        if rest.is_empty() {
            Ok(Self(uuid))
        } else {
            Err(UuidError::InvalidInputSyntax)
        }
    }

    /// The 16 bytes, in the order PostgreSQL stores and compares them.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 16] {
        &self.0
    }
}

impl From<[u8; 16]> for PgUuid {
    fn from(bytes: [u8; 16]) -> Self {
        Self(bytes)
    }
}

impl fmt::Display for PgUuid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.iter().enumerate().try_for_each(|(at, byte)| {
            if matches!(at, 4 | 6 | 8 | 10) {
                f.write_str("-")?;
            }
            write!(f, "{byte:02x}")
        })
    }
}
