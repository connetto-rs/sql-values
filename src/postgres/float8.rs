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
///
/// let distinct: std::collections::HashSet<_> =
///     ["0", "-0", "1", "1.0", "NaN", "-nan"].map(|text| float8(text)).into_iter().collect();
/// assert_eq!(distinct.len(), 3);
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
        let (digits, last_power) = shortest(value.abs());
        let mut text = StackText::default();
        fmt::Write::write_fmt(&mut text, format_args!("{digits}"))?;
        let length = i32::try_from(text.length).expect("a u64 has at most 20 digits");
        write_decimal(f, text.as_str(), last_power + length - 1)
    }
}

/// The decimal `float8out` prints for a positive finite nonzero `value`, as digits and the power
/// of ten of the last. PostgreSQL's Ryu takes the fewest digits strictly between the midpoints to
/// the neighbouring doubles, never a midpoint itself, and among those the one nearest `value`
/// (ties to even). The fewest digits never end in a zero, which a shorter spelling would drop.
fn shortest(value: f64) -> (u64, i32) {
    // Rust's shortest form also accepts a midpoint, so no spelling with fewer digits exists.
    let fewest = decimal(value, None).0.ilog10() + 1;
    let found = (fewest..=17).find_map(|count| {
        let nearest = decimal(value, Some(count));
        [nearest, next(nearest, count), previous(nearest, count)]
            .into_iter()
            .find(|&candidate| strictly_between_midpoints(value, candidate))
    });
    found.expect("17 digits always fit strictly between the midpoints")
}

/// `value` as Rust spells it in exponential notation, the shortest form that reads back when
/// `count` is `None`, else rounded to `count` significant digits with ties to even, as digits
/// and the power of ten of the last.
fn decimal(value: f64, count: Option<u32>) -> (u64, i32) {
    let mut text = StackText::default();
    let written = match count {
        Some(count) => {
            let fraction = usize::try_from(count - 1).expect("at most 16 fraction digits");
            fmt::Write::write_fmt(&mut text, format_args!("{value:.fraction$e}"))
        }
        None => fmt::Write::write_fmt(&mut text, format_args!("{value:e}")),
    };
    written.expect("a double's exponential form fits 32 bytes");
    let (significand, exponent) = text
        .as_str()
        .split_once('e')
        .expect("an exponent is written");
    let exponent: i32 = exponent.parse().expect("the exponent is an integer");
    let (mut digits, mut fraction_digits) = (0_u64, 0);
    for (index, digit) in significand.bytes().filter(u8::is_ascii_digit).enumerate() {
        digits = digits * 10 + u64::from(digit - b'0');
        fraction_digits = index;
    }
    let fraction_digits = i32::try_from(fraction_digits).expect("at most 16 fraction digits");
    (digits, exponent - fraction_digits)
}

/// The `count`-digit decimal one unit above `(digits, last_power)`.
fn next((digits, last_power): (u64, i32), count: u32) -> (u64, i32) {
    if digits + 1 == 10_u64.pow(count) {
        (10_u64.pow(count - 1), last_power + 1)
    } else {
        (digits + 1, last_power)
    }
}

/// The `count`-digit decimal one unit below `(digits, last_power)`.
fn previous((digits, last_power): (u64, i32), count: u32) -> (u64, i32) {
    if digits == 10_u64.pow(count - 1) {
        (10_u64.pow(count) - 1, last_power - 1)
    } else {
        (digits - 1, last_power)
    }
}

/// Whether the decimal reads back to `value` without lying on a midpoint to a neighbour.
fn strictly_between_midpoints(value: f64, (digits, last_power): (u64, i32)) -> bool {
    let mut text = StackText::default();
    fmt::Write::write_fmt(&mut text, format_args!("{digits}e{last_power}"))
        .expect("a decimal of at most 17 digits fits 32 bytes");
    let reads_back = text.as_str().parse::<f64>() == Ok(value);
    let bits = value.to_bits();
    let (fraction, biased) = (bits & ((1 << 52) - 1), (bits >> 52) & 0x7ff);
    // `value` is `significand * 2^power`, and a midpoint is an odd multiple of a lower power.
    let (significand, power) = if biased == 0 {
        (fraction, -1074)
    } else {
        let biased = i32::try_from(biased).expect("an 11-bit exponent");
        (fraction | 1 << 52, biased - 1075)
    };
    let above = (2 * significand + 1, power - 1);
    // The gap below a power of two is half the gap above it.
    let below = if fraction == 0 && biased > 1 {
        (4 * significand - 1, power - 2)
    } else {
        (2 * significand - 1, power - 1)
    };
    reads_back
        && !equals_dyadic((digits, last_power), above)
        && !equals_dyadic((digits, last_power), below)
}

/// Whether `digits * 10^last_power` equals `odd * 2^power`, comparing the parts of each that
/// are prime to ten and their powers of two and of five.
fn equals_dyadic((digits, last_power): (u64, i32), (odd, power): (u64, i32)) -> bool {
    let (digits, twos, digit_fives) = split_two_five(digits);
    let (odd, _, odd_fives) = split_two_five(odd);
    digits == odd && twos + last_power == power && digit_fives + last_power == odd_fives
}

/// `value` as `rest * 2^twos * 5^fives` with `rest` prime to ten.
fn split_two_five(mut value: u64) -> (u64, i32, i32) {
    let (mut twos, mut fives) = (0, 0);
    while value.is_multiple_of(2) {
        value /= 2;
        twos += 1;
    }
    while value.is_multiple_of(5) {
        value /= 5;
        fives += 1;
    }
    (value, twos, fives)
}

/// ASCII text of at most 32 bytes, room for any decimal this module writes.
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
        let slot = self.bytes.get_mut(self.length..end).ok_or(fmt::Error)?;
        slot.copy_from_slice(text.as_bytes());
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
