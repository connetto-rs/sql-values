//! PostgreSQL 18's `double precision`, read as `float8in` reads it, printed as `float8out` prints
//! it under the default `extra_float_digits` of 1, and ordered as `float8_cmp_internal` orders it.

use alloc::string::String;
use core::{
    cmp::Ordering,
    fmt,
    hash::{Hash, Hasher},
};

use super::{cast::is_pg_space, interval::strtod};

/// Why `float8in` refuses a text, one variant per message it raises.
///
/// ```
/// use sql_values::postgres::{Float8Error, PgFloat8};
///
/// assert_eq!(PgFloat8::parse("1.5x"), Err(Float8Error::InvalidInputSyntax));
/// assert_eq!(
///     PgFloat8::parse(" 1e400 ").unwrap_err().to_string(),
///     "\"1e400\" is out of range for type double precision",
/// );
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash, thiserror::Error)]
pub enum Float8Error {
    /// Text that is not one number between optional blanks.
    #[error("invalid input syntax for type double precision")]
    InvalidInputSyntax,
    /// A number beyond the finite doubles, or a nonzero one that rounds to zero, naming the
    /// number as written.
    #[error("\"{0}\" is out of range for type double precision")]
    OutOfRange(String),
}

/// A PostgreSQL `double precision`, equal, ordered and hashed as `float8_cmp_internal` compares
/// it, with `NaN` equal to itself and above `Infinity`, and `-0` equal to `0`.
///
/// Its [`Display`](fmt::Display) is `float8out`, the shortest text that reads back to the same
/// double, in fixed notation for decimal exponents from -4 to 14 and in exponential notation
/// otherwise.
///
/// ```
/// use sql_values::postgres::PgFloat8;
///
/// let float8 = |text| PgFloat8::parse(text).unwrap();
/// assert_eq!(float8(" 0.1 ").to_string(), "0.1");
/// assert_eq!(float8("1e15").to_string(), "1e+15");
/// assert_eq!(float8("0.00001").to_string(), "1e-05");
/// assert_eq!(float8("-inf").to_string(), "-Infinity");
/// assert_eq!(float8("0x1.8p1").value(), 3.0);
/// assert_eq!(float8("-0"), float8("0"));
/// assert!(float8("NaN") > float8("Infinity"));
/// assert_eq!(PgFloat8::from(2.5).to_string(), "2.5");
/// ```
#[derive(Debug, Clone, Copy)]
pub struct PgFloat8(f64);

impl PgFloat8 {
    /// Read `text` as `float8in` reads it, C's `strtod` between blanks, refusing a nonzero number
    /// that rounds to zero and one beyond the finite doubles.
    ///
    /// # Errors
    ///
    /// The [`Float8Error`] whose message PostgreSQL raises for `text`.
    pub fn parse(text: &str) -> Result<Self, Float8Error> {
        let bytes = text.as_bytes();
        let start = bytes.iter().take_while(|&&byte| is_pg_space(byte)).count();
        let sign = usize::from(bytes.get(start) == Some(&b'+'));
        if sign == 1 && bytes.get(start + 1) == Some(&b'-') {
            return Err(Float8Error::InvalidInputSyntax);
        }
        let (value, end, out_of_range) =
            strtod(bytes, start + sign).ok_or(Float8Error::InvalidInputSyntax)?;
        if out_of_range && (value == 0.0 || value.is_infinite()) {
            return Err(Float8Error::OutOfRange(String::from(&text[start..end])));
        }
        if !bytes[end..].iter().all(|&byte| is_pg_space(byte)) {
            return Err(Float8Error::InvalidInputSyntax);
        }
        Ok(Self(value))
    }

    /// The double.
    #[must_use]
    pub const fn value(self) -> f64 {
        self.0
    }
}

impl From<f64> for PgFloat8 {
    fn from(value: f64) -> Self {
        Self(value)
    }
}

impl fmt::Display for PgFloat8 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = self.0;
        if value.is_nan() {
            return f.write_str("NaN");
        }
        if value.is_sign_negative() {
            f.write_str("-")?;
        }
        if value.is_infinite() {
            return f.write_str("Infinity");
        }
        if value == 0.0 {
            return f.write_str("0");
        }
        // Rust's shortest round-trip digits are Ryu's: the fewest that read back, the closest
        // among them, with the halfway bounds inclusive for an even significand.
        let mut shortest = StackText::default();
        fmt::Write::write_fmt(&mut shortest, format_args!("{:e}", value.abs()))?;
        let (significand, exponent) = shortest
            .as_str()
            .split_once('e')
            .expect("exponential notation has an exponent");
        let exponent: i32 = exponent.parse().expect("the exponent is an integer");
        let mut digits = StackText::default();
        for digit in significand.chars().filter(|&c| c != '.') {
            fmt::Write::write_char(&mut digits, digit)?;
        }
        write_decimal(f, digits.as_str(), exponent)
    }
}

/// ASCII text of at most 32 bytes, room for a double's shortest exponential form.
#[derive(Default)]
struct StackText {
    bytes: [u8; 32],
    length: usize,
}

impl StackText {
    fn as_str(&self) -> &str {
        core::str::from_utf8(&self.bytes[..self.length]).expect("only ASCII is written")
    }
}

impl fmt::Write for StackText {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        let end = self.length + text.len();
        self.bytes
            .get_mut(self.length..end)
            .ok_or(fmt::Error)?
            .copy_from_slice(text.as_bytes());
        self.length = end;
        Ok(())
    }
}

/// `to_chars`, `digits` with the first at power `exponent` of ten, in fixed notation for
/// exponents from -4 to 14 as `printf` chooses, else as `d.ddde±XX`.
fn write_decimal(f: &mut fmt::Formatter<'_>, digits: &str, exponent: i32) -> fmt::Result {
    let length = i32::try_from(digits.len()).expect("a double has at most 17 digits");
    if !(-4..15).contains(&exponent) {
        let (first, rest) = digits.split_at(1);
        f.write_str(first)?;
        if !rest.is_empty() {
            write!(f, ".{rest}")?;
        }
        let sign = if exponent < 0 { '-' } else { '+' };
        return write!(f, "e{sign}{:02}", exponent.unsigned_abs());
    }
    if exponent < 0 {
        f.write_str("0.")?;
        for _ in 0..(-exponent - 1) {
            f.write_str("0")?;
        }
        return f.write_str(digits);
    }
    if exponent >= length - 1 {
        f.write_str(digits)?;
        for _ in 0..(exponent - length + 1) {
            f.write_str("0")?;
        }
        return Ok(());
    }
    let point = usize::try_from(exponent + 1).expect("the exponent is nonnegative here");
    let (whole, fraction) = digits.split_at(point);
    write!(f, "{whole}.{fraction}")
}

impl PartialEq for PgFloat8 {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Eq for PgFloat8 {}

impl PartialOrd for PgFloat8 {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for PgFloat8 {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self.0.is_nan(), other.0.is_nan()) {
            (true, true) => Ordering::Equal,
            (true, false) => Ordering::Greater,
            (false, true) => Ordering::Less,
            (false, false) => self
                .0
                .partial_cmp(&other.0)
                .expect("two doubles that are not NaN are ordered"),
        }
    }
}

impl Hash for PgFloat8 {
    fn hash<H: Hasher>(&self, state: &mut H) {
        // Equal values hash alike: every NaN as one, and `-0` as `0`.
        let canonical = if self.0.is_nan() {
            f64::NAN
        } else if self.0 == 0.0 {
            0.0
        } else {
            self.0
        };
        canonical.to_bits().hash(state);
    }
}
