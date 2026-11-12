//! PostgreSQL 18's arrays, one dimension per bound and length, the elements in row-major order,
//! equal, ordered and hashed as `array_cmp` and `array_eq` compare them, and printed as
//! `array_out` prints them under the default `DateStyle` of `ISO`.
//!
//! ```
//! use sql_values::{ArrayDim, PgArray, PgValue};
//!
//! let array = PgArray::new(vec![Some(PgValue::Integer(1)), None]);
//! assert_eq!(array.to_string(), "{1,NULL}");
//! assert_eq!(array.dims(), &[ArrayDim::new(1, 2).unwrap()]);
//!
//! let lower = PgArray::with_dims(
//!     vec![ArrayDim::new(0, 2).unwrap()],
//!     vec![Some(PgValue::Integer(1)), Some(PgValue::Integer(2))],
//! )
//! .unwrap();
//! assert_eq!(lower.to_string(), "[0:1]={1,2}");
//! ```

use alloc::{string::ToString, vec, vec::Vec};
use core::{
    cmp::Ordering,
    fmt,
    hash::{Hash, Hasher},
};

use crate::{cast::is_pg_space, value::PgValue};

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
    /// use sql_values::ArrayDim;
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
/// use sql_values::{ArrayDim, PgArray, PgValue};
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
    /// use sql_values::{PgArray, PgValue};
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
    /// use sql_values::{ArrayDim, PgArray, PgValue};
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

#[cfg(test)]
mod tests {
    use super::*;

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
        let left = PgArray::new(vec![Some(PgValue::Integer(1)), Some(PgValue::Integer(2))]);
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
        let day = crate::datetime::PgDate::from_ymd(2026, 1, 1).unwrap();
        let dated = PgValue::Array(PgArray::new(vec![Some(PgValue::Date(day))]));
        assert!(dated.output().is_none());
        assert_eq!(
            array.into_cast_value(crate::integer::IntegerType::Integer),
            Err(crate::cast::CastError::Unsupported)
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
