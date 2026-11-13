//! PostgreSQL 18's arrays, one dimension per bound and length, the elements in row-major order,
//! read from their text as `array_in` reads it, printed as `array_out` prints it under the
//! default `DateStyle` of `ISO`, ordered and equated as `array_cmp` and `array_eq` do, and cast
//! element by element.
//!
//! ```
//! use sql_values::postgres::{ArrayDim, CastTarget, PgArray, PgValue};
//!
//! let array = PgArray::parse("{1,2,NULL}", CastTarget::Integer).unwrap();
//! assert_eq!(array.to_string(), "{1,2,NULL}");
//! assert_eq!(array.dims(), &[ArrayDim::new(1, 3).unwrap()]);
//! assert_eq!(
//!     array.elements(),
//!     &[Some(PgValue::Integer(1)), Some(PgValue::Integer(2)), None],
//! );
//!
//! let lower = PgArray::with_dims(
//!     vec![ArrayDim::new(0, 2).unwrap()],
//!     vec![Some(PgValue::Integer(1)), Some(PgValue::Integer(2))],
//! )
//! .unwrap();
//! assert_eq!(lower.to_string(), "[0:1]={1,2}");
//! ```

use alloc::{
    string::{String, ToString},
    vec,
    vec::Vec,
};
use core::{
    cmp::Ordering,
    fmt,
    hash::{Hash, Hasher},
};

use super::{
    cast::{CastError, CastTarget, CastValue, Literal, cast_literal, cast_value, is_pg_space},
    integer::IntegerType,
    value::PgValue,
};

/// The most array dimensions PostgreSQL allows, `MAXDIM`.
const MAX_DIMS: usize = 6;

/// One dimension of an array, its lower bound and its length.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ArrayDim {
    lower: i32,
    length: u32,
}

impl ArrayDim {
    /// The dimension of `length` from `lower`, or `None` when `lower + length` overflows `i32`,
    /// as `ArrayCheckBounds` refuses it, so the upper bound stays below `i32::MAX`.
    ///
    /// ```
    /// use sql_values::postgres::ArrayDim;
    ///
    /// let dim = ArrayDim::new(-1, 3).unwrap();
    /// assert_eq!(dim.lower(), -1);
    /// assert_eq!(dim.length(), 3);
    /// assert!(ArrayDim::new(i32::MAX - 1, 1).is_some());
    /// assert!(ArrayDim::new(i32::MAX, 1).is_none());
    /// ```
    #[must_use]
    pub fn new(lower: i32, length: u32) -> Option<Self> {
        i32::try_from(i64::from(lower) + i64::from(length))
            .ok()
            .map(|_| Self { lower, length })
    }

    /// The dimension's lower bound.
    #[must_use]
    pub const fn lower(self) -> i32 {
        self.lower
    }

    /// The dimension's length.
    #[must_use]
    pub const fn length(self) -> u32 {
        self.length
    }
}

/// An array of [`PgValue`]s over its dimensions, in row-major order, equal, ordered and hashed
/// as `array_eq` and `btarraycmp` compare it, and printed as `array_out` prints it under the
/// default `DateStyle` of `ISO`.
///
/// ```
/// use sql_values::postgres::{ArrayDim, PgArray, PgValue};
///
/// let array = PgArray::new(vec![Some(PgValue::Integer(1)), None]);
/// assert_eq!(array.to_string(), "{1,NULL}");
/// assert_eq!(array.dims(), &[ArrayDim::new(1, 2).unwrap()]);
/// assert!(PgArray::new(vec![None]) > PgArray::new(vec![Some(PgValue::Integer(1))]));
/// ```
#[derive(Debug, Clone)]
pub struct PgArray {
    dims: Vec<ArrayDim>,
    elements: Vec<Option<PgValue>>,
}

impl PgArray {
    /// The array of `elements`, one dimension from 1, or no dimension when empty, as `ARRAY[...]`
    /// holds it.
    ///
    /// ```
    /// use sql_values::postgres::{PgArray, PgValue};
    ///
    /// assert_eq!(PgArray::new(Vec::new()).to_string(), "{}");
    /// assert_eq!(
    ///     PgArray::new(vec![Some(PgValue::Text("a b".into()))]).to_string(),
    ///     r#"{"a b"}"#,
    /// );
    /// ```
    #[must_use]
    pub fn new(elements: Vec<Option<PgValue>>) -> Self {
        let dims = if elements.is_empty() {
            Vec::new()
        } else {
            vec![
                ArrayDim::new(1, u32::try_from(elements.len()).expect("a held array"))
                    .expect("a bound"),
            ]
        };
        Self { dims, elements }
    }

    /// The array of `elements` over `dims`, or `None` when the dimensions' product differs from
    /// the element count, when there are more than 6 dimensions, or when an element is itself
    /// an array.
    ///
    /// ```
    /// use sql_values::postgres::{ArrayDim, PgArray, PgValue};
    ///
    /// let array = PgArray::with_dims(
    ///     vec![ArrayDim::new(1, 2).unwrap(), ArrayDim::new(1, 2).unwrap()],
    ///     vec![
    ///         Some(PgValue::Integer(1)),
    ///         Some(PgValue::Integer(2)),
    ///         Some(PgValue::Integer(3)),
    ///         Some(PgValue::Integer(4)),
    ///     ],
    /// )
    /// .unwrap();
    /// assert_eq!(array.to_string(), "{{1,2},{3,4}}");
    /// assert!(PgArray::with_dims(vec![ArrayDim::new(1, 2).unwrap()], Vec::new()).is_none());
    /// assert!(
    ///     PgArray::with_dims(
    ///         vec![ArrayDim::new(1, 1).unwrap(); 7],
    ///         vec![Some(PgValue::Integer(1))],
    ///     )
    ///     .is_none()
    /// );
    /// ```
    #[must_use]
    pub fn with_dims(dims: Vec<ArrayDim>, elements: Vec<Option<PgValue>>) -> Option<Self> {
        if dims.len() > MAX_DIMS {
            return None;
        }
        if elements
            .iter()
            .any(|element| matches!(element, Some(PgValue::Array(_))))
        {
            return None;
        }
        let expected = match dims.len() {
            0 => 0,
            _ => {
                let mut product: u64 = 1;
                for dim in &dims {
                    product = product.checked_mul(u64::from(dim.length()))?;
                }
                product
            }
        };
        let count = u64::try_from(elements.len()).expect("a held array");
        (expected == count).then_some(Self { dims, elements })
    }

    /// The array's dimensions.
    #[must_use]
    pub fn dims(&self) -> &[ArrayDim] {
        &self.dims
    }

    /// The array's elements, in row-major order.
    #[must_use]
    pub fn elements(&self) -> &[Option<PgValue>] {
        &self.elements
    }

    /// PostgreSQL 18's `array_in` for an array of `element`, each element read by that type's
    /// input function, `NULL` for an unquoted, unescaped spelling of `null`, and the dimensions
    /// from the text's `[l:u]` items or from the nesting.
    ///
    /// ```
    /// use sql_values::postgres::{CastError, CastTarget, PgArray};
    ///
    /// let array = PgArray::parse("[0:1]={1,2}", CastTarget::Integer).unwrap();
    /// assert_eq!(array.to_string(), "[0:1]={1,2}");
    /// assert_eq!(
    ///     PgArray::parse("{1,2}x", CastTarget::Integer),
    ///     Err(CastError::MalformedArrayLiteral),
    /// );
    /// ```
    ///
    /// # Errors
    ///
    /// The [`CastError`] PostgreSQL's `array_in` raises, the error the element's input function
    /// raises for an element, or [`CastError::Unsupported`] for an element type this crate does
    /// not hold.
    pub fn parse(text: &str, element: CastTarget) -> Result<Self, CastError> {
        supported(element)?;
        let chars: Vec<char> = text.chars().collect();
        let mut at = 0;
        let (mut ndim, bounds) = read_dimensions(&chars, &mut at)?;
        let specified = ndim > 0;
        if specified && chars.get(at).copied() != Some('=') {
            return Err(CastError::MalformedArrayLiteral);
        }
        at += usize::from(specified);
        skip_space(&chars, &mut at);
        if chars.get(at).copied() != Some('{') {
            return Err(CastError::MalformedArrayLiteral);
        }
        at += 1;

        let mut dim: [Option<i64>; MAX_DIMS] = [None; MAX_DIMS];
        let mut lbound: [i32; MAX_DIMS] = [1; MAX_DIMS];
        for (i, (lower, length)) in bounds.iter().enumerate() {
            dim[i] = Some(*length);
            lbound[i] = *lower;
        }
        let mut nelems: [usize; MAX_DIMS] = [0; MAX_DIMS];
        // The outer brace is the first level, as the token loop reads it.
        if ndim == 0 {
            ndim = 1;
        }
        let mut nest = 1;
        let mut frozen = specified;
        let mut expect_delim = false;
        let mut elements: Vec<Option<PgValue>> = Vec::new();

        loop {
            let token = read_token(&chars, &mut at)?;
            match token {
                Token::LevelStart => {
                    if expect_delim {
                        return Err(CastError::MalformedArrayLiteral);
                    }
                    if nest >= MAX_DIMS {
                        return Err(CastError::ArrayDimensionsExceeded);
                    }
                    nelems[nest] = 0;
                    nest += 1;
                    if nest > ndim {
                        if frozen {
                            return Err(CastError::MalformedArrayLiteral);
                        }
                        ndim = nest;
                    }
                }
                Token::LevelEnd => {
                    if nelems[nest - 1] > 0 && !expect_delim {
                        return Err(CastError::MalformedArrayLiteral);
                    }
                    nest -= 1;
                    if nest > 0 {
                        nelems[nest - 1] += 1;
                    }
                    let count = i64::try_from(nelems[nest]).expect("a count");
                    match dim[nest] {
                        None => dim[nest] = Some(count),
                        Some(length) if length != count => {
                            return Err(CastError::MalformedArrayLiteral);
                        }
                        Some(_) => {}
                    }
                    expect_delim = true;
                }
                Token::Delim => {
                    if !expect_delim {
                        return Err(CastError::MalformedArrayLiteral);
                    }
                    expect_delim = false;
                }
                Token::Elem(element_text) => {
                    if expect_delim {
                        return Err(CastError::MalformedArrayLiteral);
                    }
                    // The input function runs before the dimension check, as `ReadArrayStr`
                    // orders it, so its error wins.
                    let element = element_text
                        .map(|text| read_element(&text, element))
                        .transpose()?;
                    if nest != ndim {
                        return Err(CastError::MalformedArrayLiteral);
                    }
                    frozen = true;
                    elements.push(element);
                    nelems[nest - 1] += 1;
                    expect_delim = true;
                }
            }
            if nest == 0 {
                break;
            }
        }

        skip_space(&chars, &mut at);
        if at != chars.len() {
            return Err(CastError::MalformedArrayLiteral);
        }

        if elements.is_empty() {
            // `construct_empty_array`, which keeps no dimension.
            return Ok(Self {
                dims: Vec::new(),
                elements,
            });
        }
        let dims = (0..ndim)
            .map(|i| {
                ArrayDim::new(
                    lbound[i],
                    u32::try_from(dim[i].expect("a read dimension")).expect("a length"),
                )
                .expect("a bound")
            })
            .collect();
        Ok(Self { dims, elements })
    }

    /// The explicit cast of every element to `element`, as [`cast_value`] casts it, `NULL`s kept
    /// and the dimensions kept.
    ///
    /// ```
    /// use sql_values::postgres::{CastError, CastTarget, PgArray};
    ///
    /// let array = PgArray::parse("{1.5}", CastTarget::Numeric(None)).unwrap();
    /// assert_eq!(array.cast(CastTarget::Integer).unwrap().to_string(), "{2}");
    /// assert_eq!(
    ///     PgArray::parse("{1}", CastTarget::Integer)
    ///         .unwrap()
    ///         .cast(CastTarget::Bpchar(None)),
    ///     Err(CastError::Unsupported),
    /// );
    /// ```
    ///
    /// # Errors
    ///
    /// The [`CastError`] the element's cast raises for an element, or
    /// [`CastError::Unsupported`] for an element type this crate does not hold.
    pub fn cast(&self, element: CastTarget) -> Result<Self, CastError> {
        supported(element)?;
        let elements = self
            .elements
            .iter()
            .map(|value| match value {
                None => Ok(None),
                Some(value) => {
                    let value = cast_value(element_value(value)?, element)?;
                    into_value(value).map(Some)
                }
            })
            .collect::<Result<Vec<Option<PgValue>>, CastError>>()?;
        Ok(Self {
            dims: self.dims.clone(),
            elements,
        })
    }
}

impl PartialEq for PgArray {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Eq for PgArray {}

impl PartialOrd for PgArray {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// `array_cmp`, element by element with `NULL` equal to `NULL` and greater than every value,
/// then fewer elements, then fewer dimensions, then the dimension lengths, then the lower
/// bounds.
impl Ord for PgArray {
    fn cmp(&self, other: &Self) -> Ordering {
        // An element's key is its absence, then its value, so two `NULL`s are equal and a
        // `NULL` sorts after every value. Each sequence compares lexicographically, so the
        // element order falls back to the element count.
        self.elements
            .iter()
            .map(element_key)
            .cmp(other.elements.iter().map(element_key))
            .then_with(|| self.dims.len().cmp(&other.dims.len()))
            .then_with(|| {
                self.dims
                    .iter()
                    .map(|dim| dim.length())
                    .cmp(other.dims.iter().map(|dim| dim.length()))
            })
            .then_with(|| {
                self.dims
                    .iter()
                    .map(|dim| dim.lower())
                    .cmp(other.dims.iter().map(|dim| dim.lower()))
            })
    }
}

/// The element's key in the order, its absence, then its value.
fn element_key(element: &Option<PgValue>) -> (bool, &Option<PgValue>) {
    (element.is_none(), element)
}

impl Hash for PgArray {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.dims.hash(state);
        self.elements.hash(state);
    }
}

/// `array_out` under `DateStyle` `ISO`, the dimensions' bounds printed when one is not 1.
impl fmt::Display for PgArray {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.elements.is_empty() {
            return f.write_str("{}");
        }
        if self.dims.iter().any(|dim| dim.lower() != 1) {
            for dim in &self.dims {
                let upper = i64::from(dim.lower()) + i64::from(dim.length()) - 1;
                write!(f, "[{}:{upper}]", dim.lower())?;
            }
            f.write_str("=")?;
        }
        let mut reopen = 0;
        let mut index = [0u32; MAX_DIMS];
        fmt::Write::write_char(f, '{')?;
        for value in &self.elements {
            for _ in reopen..self.dims.len() - 1 {
                fmt::Write::write_char(f, '{')?;
            }
            write_element(f, value)?;
            let mut dim = self.dims.len() - 1;
            loop {
                index[dim] += 1;
                if index[dim] < self.dims[dim].length() {
                    fmt::Write::write_char(f, ',')?;
                    reopen = dim;
                    break;
                }
                index[dim] = 0;
                fmt::Write::write_char(f, '}')?;
                if dim == 0 {
                    reopen = 0;
                    break;
                }
                dim -= 1;
            }
        }
        Ok(())
    }
}

/// The element's output text, quoted as `array_out` quotes it, with `"` and `\` escaped.
fn write_element(f: &mut fmt::Formatter<'_>, value: &Option<PgValue>) -> fmt::Result {
    match value {
        None => f.write_str("NULL"),
        Some(value) => {
            let text = value.to_string();
            if text.is_empty() || text.eq_ignore_ascii_case("NULL") || text.bytes().any(quotes) {
                fmt::Write::write_char(f, '"')?;
                for c in text.chars() {
                    if matches!(c, '"' | '\\') {
                        fmt::Write::write_char(f, '\\')?;
                    }
                    fmt::Write::write_char(f, c)?;
                }
                fmt::Write::write_char(f, '"')
            } else {
                f.write_str(&text)
            }
        }
    }
}

/// Whether the byte forces `array_out` to quote the element.
fn quotes(byte: u8) -> bool {
    matches!(byte, b'"' | b'\\' | b'{' | b'}' | b',') || is_pg_space(byte)
}

/// `scanner_isspace`, the whitespace of the array literal, the C locale's `isspace`.
fn is_space(c: char) -> bool {
    u8::try_from(c).is_ok_and(is_pg_space)
}

/// The element's value as a [`CastValue`], its integer held at the width the value takes.
fn element_value(value: &PgValue) -> Result<CastValue, CastError> {
    let width = match value {
        PgValue::Integer(value) if i32::try_from(*value).is_err() => IntegerType::Bigint,
        _ => IntegerType::Integer,
    };
    value.clone().into_cast_value(width)
}

/// The element's value, its text read by the element type's input function.
fn read_element(text: &str, element: CastTarget) -> Result<PgValue, CastError> {
    into_value(cast_literal(Literal::Quoted(text), element)?)
}

/// The [`PgValue`] the crate holds for the `CastValue` an element read, or
/// `CastError::Unsupported` when it holds none.
fn into_value(value: CastValue) -> Result<PgValue, CastError> {
    PgValue::try_from(value).map_err(|_| CastError::Unsupported)
}

/// `Ok` when the crate holds `element` as an array element, `CastError::Unsupported` when it
/// does not.
fn supported(element: CastTarget) -> Result<(), CastError> {
    if matches!(element, CastTarget::Bpchar(_) | CastTarget::TimestampTz(_)) {
        Err(CastError::Unsupported)
    } else {
        Ok(())
    }
}

/// The token `array_in` reads at `at`, left past it.
enum Token {
    /// A `{` opening a level.
    LevelStart,
    /// A `}` closing a level.
    LevelEnd,
    /// A `,`.
    Delim,
    /// An element, its quotes and escapes decoded, `None` for a `NULL` element.
    Elem(Option<String>),
}

/// The token at `at`, past leading whitespace.
fn read_token(chars: &[char], at: &mut usize) -> Result<Token, CastError> {
    loop {
        let Some(&c) = chars.get(*at) else {
            return Err(CastError::MalformedArrayLiteral);
        };
        match c {
            '{' => {
                *at += 1;
                return Ok(Token::LevelStart);
            }
            '}' => {
                *at += 1;
                return Ok(Token::LevelEnd);
            }
            ',' => {
                *at += 1;
                return Ok(Token::Delim);
            }
            '"' => {
                *at += 1;
                return read_quoted(chars, at);
            }
            _ if is_space(c) => *at += 1,
            _ => return read_unquoted(chars, at),
        }
    }
}

/// The character the backslash at `at` escapes, `at` left past it.
fn read_escaped(chars: &[char], at: &mut usize) -> Result<char, CastError> {
    *at += 1;
    let Some(&escaped) = chars.get(*at) else {
        return Err(CastError::MalformedArrayLiteral);
    };
    *at += 1;
    Ok(escaped)
}

/// The `"…"` element, `at` left at the delimiter or brace following it.
fn read_quoted(chars: &[char], at: &mut usize) -> Result<Token, CastError> {
    let mut value = String::new();
    loop {
        let Some(&c) = chars.get(*at) else {
            return Err(CastError::MalformedArrayLiteral);
        };
        match c {
            '\\' => value.push(read_escaped(chars, at)?),
            '"' => {
                *at += 1;
                skip_space(chars, at);
                return match chars.get(*at) {
                    Some(&(',' | '}' | '{')) => Ok(Token::Elem(Some(value))),
                    _ => Err(CastError::MalformedArrayLiteral),
                };
            }
            _ => {
                value.push(c);
                *at += 1;
            }
        }
    }
}

/// The unquoted element, its trailing whitespace dropped, `NULL` when it spells `null` without
/// escapes.
fn read_unquoted(chars: &[char], at: &mut usize) -> Result<Token, CastError> {
    let mut value = String::new();
    let mut solid = 0;
    let mut escaped = false;
    loop {
        let Some(&c) = chars.get(*at) else {
            return Err(CastError::MalformedArrayLiteral);
        };
        match c {
            '{' | '"' => return Err(CastError::MalformedArrayLiteral),
            '\\' => {
                value.push(read_escaped(chars, at)?);
                solid = value.len();
                escaped = true;
            }
            ',' | '}' => {
                value.truncate(solid);
                let token = if escaped || !value.eq_ignore_ascii_case("NULL") {
                    Token::Elem(Some(value))
                } else {
                    Token::Elem(None)
                };
                return Ok(token);
            }
            _ => {
                value.push(c);
                if !is_space(c) {
                    solid = value.len();
                }
                *at += 1;
            }
        }
    }
}

/// `at` left past the whitespace.
fn skip_space(chars: &[char], at: &mut usize) {
    while chars.get(*at).copied().is_some_and(is_space) {
        *at += 1;
    }
}

/// The `[n]` and `[m:n]` items before the `{`, each the lower bound and length PostgreSQL reads.
fn read_dimensions(chars: &[char], at: &mut usize) -> Result<(usize, Vec<(i32, i64)>), CastError> {
    let mut ndim = 0;
    let mut dims: Vec<(i32, i64)> = Vec::new();
    loop {
        skip_space(chars, at);
        if chars.get(*at).copied() != Some('[') {
            break;
        }
        *at += 1;
        if ndim == MAX_DIMS {
            return Err(CastError::ArrayDimensionsExceeded);
        }
        let first = read_bounded(chars, at)?;
        let (lower, upper) = if chars.get(*at).copied() == Some(':') {
            *at += 1;
            (first, read_bounded(chars, at)?)
        } else {
            (1, first)
        };
        if chars.get(*at).copied() != Some(']') {
            return Err(CastError::MalformedArrayLiteral);
        }
        *at += 1;
        if upper < lower {
            return Err(CastError::UpperBoundLessThanLower);
        }
        if upper == i32::MAX {
            return Err(CastError::ArrayUpperBoundTooLarge(upper));
        }
        let length = i64::from(upper) - i64::from(lower) + 1;
        if length > i64::from(i32::MAX) {
            return Err(CastError::ArraySizeExceeded);
        }
        dims.push((lower, length));
        ndim += 1;
    }
    Ok((ndim, dims))
}

/// The bound's integer, `0` when none, `at` left where the read stopped.
fn read_bound(chars: &[char], at: &mut usize) -> Result<i32, CastError> {
    match chars.get(*at) {
        Some(&c) if c.is_ascii_digit() || c == '-' || c == '+' => {}
        _ => return Ok(0),
    }
    let mut i = *at;
    let negative = chars.get(i).copied() == Some('-');
    i += usize::from(matches!(chars.get(i), Some('-') | Some('+')));
    let digit_at = i;
    let mut magnitude: i128 = 0;
    while let Some(&digit) = chars.get(i) {
        if !digit.is_ascii_digit() {
            break;
        }
        magnitude = magnitude * 10 + i128::from(digit.to_digit(10).expect("a digit"));
        if magnitude > i128::from(i64::MAX) {
            magnitude = i128::from(i64::MAX) + 1;
        }
        i += 1;
    }
    if i == digit_at {
        return Ok(0);
    }
    *at = i;
    let value = if negative { -magnitude } else { magnitude };
    i32::try_from(value).map_err(|_| CastError::ArrayBoundOutOfRange)
}

/// The bound `at` reads, which must consume at least one character.
fn read_bounded(chars: &[char], at: &mut usize) -> Result<i32, CastError> {
    let start = *at;
    let bound = read_bound(chars, at)?;
    if *at == start {
        return Err(CastError::MalformedArrayLiteral);
    }
    Ok(bound)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::postgres::cast::PgType;

    fn dim(lower: i32, length: u32) -> ArrayDim {
        ArrayDim::new(lower, length).expect("a bound")
    }

    /// `ArrayCheckBounds` refuses a dimension whose `lower + length` overflows `i32`, as
    /// `'[2147483647:2147483647]={1}'::int[]` and `'[2147483645:2147483646]={1,2}'::int[] || 3`
    /// show on PostgreSQL 18.6.
    #[test]
    fn bounds_refuse_an_overflowing_lower_plus_length() {
        let dim = ArrayDim::new(-1, 3).unwrap();
        assert_eq!((dim.lower(), dim.length()), (-1, 3));
        assert!(ArrayDim::new(i32::MAX - 1, 1).is_some());
        assert!(ArrayDim::new(i32::MAX, 1).is_none());
        assert!(ArrayDim::new(i32::MAX - 2, 3).is_none());
        assert!(ArrayDim::new(i32::MIN, 2).is_some());
    }

    #[test]
    fn new_holds_one_dimension_from_one() {
        let array = PgArray::new(vec![Some(PgValue::Integer(1)), None]);
        assert_eq!(array.dims(), &[dim(1, 2)]);
        assert_eq!(array.elements(), &[Some(PgValue::Integer(1)), None]);
        assert_eq!(PgArray::new(Vec::new()).dims(), &[]);
    }

    #[test]
    fn with_dims_refuses_the_inconsistent_shape() {
        assert!(PgArray::with_dims(vec![dim(1, 2)], Vec::new()).is_none());
        assert!(PgArray::with_dims(vec![dim(1, 2)], vec![Some(PgValue::Integer(1))]).is_none());
        assert!(PgArray::with_dims(vec![], vec![Some(PgValue::Integer(1))]).is_none());
        assert!(PgArray::with_dims(vec![dim(1, 1); 7], vec![Some(PgValue::Integer(1))]).is_none());
        assert!(
            PgArray::with_dims(
                vec![dim(1, 1)],
                vec![Some(PgValue::Array(PgArray::new(Vec::new())))],
            )
            .is_none()
        );
        assert!(PgArray::with_dims(vec![], Vec::new()).is_some());
    }

    #[test]
    fn parse_reads_the_array_in_forms() {
        let array = PgArray::parse(" { 1 , 2 } ", CastTarget::Integer).unwrap();
        assert_eq!(array.to_string(), "{1,2}");
        assert_eq!(
            PgArray::parse("[0:1]={1,2}", CastTarget::Integer)
                .unwrap()
                .to_string(),
            "[0:1]={1,2}"
        );
        assert_eq!(
            PgArray::parse("{nUlL}", CastTarget::Integer)
                .unwrap()
                .to_string(),
            "{NULL}"
        );
        assert_eq!(
            PgArray::parse("{\"a b\"}", CastTarget::Text)
                .unwrap()
                .to_string(),
            "{\"a b\"}"
        );
        assert_eq!(
            PgArray::parse(r#"{a\b}"#, CastTarget::Text)
                .unwrap()
                .to_string(),
            "{ab}"
        );
        assert_eq!(
            PgArray::parse(r#"{a\ b c}"#, CastTarget::Text)
                .unwrap()
                .to_string(),
            r#"{"a b c"}"#
        );
        assert_eq!(
            PgArray::parse("{}[1:1]", CastTarget::Integer),
            Err(CastError::MalformedArrayLiteral)
        );
        assert!(
            PgArray::parse("{}", CastTarget::Integer)
                .unwrap()
                .dims()
                .is_empty()
        );
        assert!(
            PgArray::parse("{{}}", CastTarget::Integer)
                .unwrap()
                .dims()
                .is_empty()
        );
        assert_eq!(
            PgArray::parse("[1:2]=", CastTarget::Integer),
            Err(CastError::MalformedArrayLiteral)
        );
        assert_eq!(
            PgArray::parse("{1}", CastTarget::Bpchar(None)),
            Err(CastError::Unsupported)
        );
    }

    #[test]
    fn parse_refuses_as_array_in_refuses() {
        let cases: &[(&str, CastError)] = &[
            ("{1,}", CastError::MalformedArrayLiteral),
            ("{,1}", CastError::MalformedArrayLiteral),
            ("{1,2}x", CastError::MalformedArrayLiteral),
            ("{1", CastError::MalformedArrayLiteral),
            ("{\"a", CastError::MalformedArrayLiteral),
            ("[1:2]{1,2}", CastError::MalformedArrayLiteral),
            ("[1:0]={1,2}", CastError::UpperBoundLessThanLower),
            (
                "[1:2147483647]=",
                CastError::ArrayUpperBoundTooLarge(i32::MAX),
            ),
            ("[-1:2147483646]=", CastError::ArraySizeExceeded),
            ("[2147483648:2]=", CastError::ArrayBoundOutOfRange),
            ("[-2147483649:2]=", CastError::ArrayBoundOutOfRange),
            ("[1][1][1][1][1][1][1]=", CastError::ArrayDimensionsExceeded),
            ("{{{{{{{1}}}}}}}", CastError::ArrayDimensionsExceeded),
            ("{1,{2}}", CastError::MalformedArrayLiteral),
            ("{a\\", CastError::MalformedArrayLiteral),
            ("{\"1\"{2}}", CastError::MalformedArrayLiteral),
            ("{ {1}2 }", CastError::MalformedArrayLiteral),
            ("{\"a\"", CastError::MalformedArrayLiteral),
            ("[99999999999999999999:1]=", CastError::ArrayBoundOutOfRange),
            ("[--:1]=", CastError::MalformedArrayLiteral),
            ("[1", CastError::MalformedArrayLiteral),
            ("{abc}", CastError::InvalidInputSyntax(PgType::Integer)),
            ("{40000}", CastError::ValueOutOfRange(PgType::Smallint)),
        ];
        for (text, error) in cases {
            let target = match error {
                CastError::InvalidInputSyntax(PgType::Integer) => CastTarget::Integer,
                CastError::ValueOutOfRange(PgType::Smallint) => CastTarget::Smallint,
                _ => CastTarget::Integer,
            };
            assert_eq!(
                PgArray::parse(text, target),
                Err(*error),
                "parsing {text:?}"
            );
        }
    }

    #[test]
    fn cast_casts_every_element_as_cast_value_does() {
        let array = PgArray::parse("{1.5,NaN}", CastTarget::Numeric(None)).unwrap();
        assert_eq!(
            array.cast(CastTarget::Integer).unwrap_err(),
            CastError::CannotConvertNaN(PgType::Integer)
        );
        assert_eq!(
            PgArray::parse("{1.5,-2.5}", CastTarget::Numeric(None))
                .unwrap()
                .cast(CastTarget::Smallint)
                .unwrap()
                .to_string(),
            "{2,-3}"
        );
        assert_eq!(
            PgArray::parse("{40000}", CastTarget::Integer)
                .unwrap()
                .cast(CastTarget::Smallint),
            Err(CastError::OutOfRange(PgType::Smallint))
        );
        assert_eq!(
            PgArray::parse("{2026-01-01}", CastTarget::Date)
                .unwrap()
                .cast(CastTarget::Timestamp(None))
                .unwrap()
                .to_string(),
            "{\"2026-01-01 00:00:00\"}"
        );
        assert_eq!(
            PgArray::parse("{1,2}", CastTarget::Integer)
                .unwrap()
                .cast(CastTarget::TimestampTz(None)),
            Err(CastError::Unsupported)
        );
    }

    #[test]
    fn display_prints_as_array_out_prints() {
        assert_eq!(PgArray::new(Vec::new()).to_string(), "{}");
        assert_eq!(
            PgArray::with_dims(vec![dim(1, 0)], Vec::new())
                .unwrap()
                .to_string(),
            "{}"
        );
        assert_eq!(
            PgArray::with_dims(
                vec![dim(0, 2)],
                vec![Some(PgValue::Integer(1)), Some(PgValue::Integer(2))]
            )
            .unwrap()
            .to_string(),
            "[0:1]={1,2}"
        );
        assert_eq!(
            PgArray::with_dims(
                vec![dim(1, 2), dim(0, 2)],
                vec![
                    Some(PgValue::Integer(1)),
                    Some(PgValue::Integer(2)),
                    Some(PgValue::Integer(3)),
                    Some(PgValue::Integer(4))
                ]
            )
            .unwrap()
            .to_string(),
            "[1:2][0:1]={{1,2},{3,4}}"
        );
        assert_eq!(
            PgArray::new(vec![
                Some(PgValue::Text("a b".into())),
                Some(PgValue::Text("a\"b".into())),
                Some(PgValue::Text("a\\b".into())),
                Some(PgValue::Text("NULL".into())),
                Some(PgValue::Text("".into())),
                Some(PgValue::Text("1".into())),
            ])
            .to_string(),
            r#"{"a b","a\"b","a\\b","NULL","",1}"#
        );
    }

    #[test]
    fn order_and_hash_match_array_cmp_and_hash_array() {
        let null = PgArray::new(vec![None]);
        let one = PgArray::new(vec![Some(PgValue::Integer(1))]);
        assert!(null > one);
        assert_eq!(null, PgArray::new(vec![None]));
        let pair = PgArray::new(vec![Some(PgValue::Integer(1)), Some(PgValue::Integer(2))]);
        assert!(pair < PgArray::new(vec![Some(PgValue::Integer(1)), Some(PgValue::Integer(3))]));
        assert!(
            pair < PgArray::new(vec![
                Some(PgValue::Integer(1)),
                Some(PgValue::Integer(2)),
                Some(PgValue::Integer(3))
            ])
        );
        assert!(
            pair < PgArray::with_dims(
                vec![dim(1, 1), dim(1, 2)],
                vec![Some(PgValue::Integer(1)), Some(PgValue::Integer(2))]
            )
            .unwrap()
        );
        assert!(
            PgArray::with_dims(
                vec![dim(0, 2)],
                vec![Some(PgValue::Integer(1)), Some(PgValue::Integer(2))]
            )
            .unwrap()
                < pair
        );
        let left = PgArray::parse("{1,2}", CastTarget::Integer).unwrap();
        let right = PgArray::with_dims(
            vec![dim(1, 2)],
            vec![Some(PgValue::Integer(1)), Some(PgValue::Integer(2))],
        )
        .unwrap();
        assert_eq!(left, right);
        assert_eq!(hash(&left), hash(&right));
        // Equal elements, element counts and dimension counts order by the lengths.
        let six: Vec<_> = (1..=6).map(|value| Some(PgValue::Integer(value))).collect();
        let two_by_three = PgArray::with_dims(vec![dim(1, 2), dim(1, 3)], six.clone()).unwrap();
        let three_by_two = PgArray::with_dims(vec![dim(1, 3), dim(1, 2)], six).unwrap();
        assert!(two_by_three < three_by_two);
    }

    /// An array is a `PgValue` of its own type, after every scalar type, compared, hashed and
    /// printed as the array, read by no scalar cast, and printed by its output function unless
    /// a date or a timestamp element leaves the spelling to `DateStyle`.
    #[test]
    fn an_array_value_is_a_type_of_its_own() {
        let elements = vec![Some(PgValue::Integer(1)), None];
        let array = PgValue::Array(PgArray::new(elements.clone()));
        assert!(array > PgValue::Integer(i64::MAX));
        assert_eq!(array, PgValue::Array(PgArray::new(elements.clone())));
        assert_eq!(hash(&array), hash(&PgValue::Array(PgArray::new(elements))));
        assert_eq!(array.to_string(), "{1,NULL}");
        assert_eq!(
            array.output().map(|output| output.to_string()).as_deref(),
            Some("{1,NULL}")
        );
        let day = crate::postgres::datetime::PgDate::from_ymd(2026, 1, 1).unwrap();
        let dated = PgValue::Array(PgArray::new(vec![Some(PgValue::Date(day))]));
        assert!(dated.output().is_none());
        assert_eq!(
            array.into_cast_value(crate::postgres::integer::IntegerType::Integer),
            Err(crate::postgres::cast::CastError::Unsupported)
        );
    }

    /// A fixed seed's [`core::hash::BuildHasher`], for the no_std hash comparison.
    fn hash<T: Hash>(value: &T) -> u64 {
        let mut hasher = FixedHasher::default();
        value.hash(&mut hasher);
        hasher.finish()
    }

    #[derive(Default)]
    struct FixedHasher {
        value: u64,
    }

    impl core::hash::Hasher for FixedHasher {
        fn finish(&self) -> u64 {
            self.value
        }

        fn write(&mut self, bytes: &[u8]) {
            for &byte in bytes {
                self.value = self.value.wrapping_mul(31).wrapping_add(u64::from(byte));
            }
        }
    }
}
