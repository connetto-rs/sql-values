//! A finite `numeric` converts to the `BigDecimal` its printed text spells, scale included, and
//! `NaN` and the infinities refuse.
#![cfg(feature = "decimal")]

use core::str::FromStr;

use bigdecimal::BigDecimal;
use sql_values::postgres::{NonFiniteNumeric, PgNumeric};

#[test]
fn a_finite_numeric_is_the_big_decimal_its_text_spells() {
    for text in [
        "0",
        "0.000",
        "-0.0",
        "1",
        "-1",
        "1.50",
        "-7.5",
        "1000",
        "1e3",
        "1.000e3",
        "0.001",
        "-0.000120",
        "12345678901234567890.123456789",
        "9999999999999999999999999999999999999999",
        "1e-100",
        "-1.5e100",
    ] {
        let numeric = PgNumeric::parse(text).unwrap();
        let printed = numeric.to_string();
        let converted = BigDecimal::try_from(&numeric).unwrap();
        let expected = BigDecimal::from_str(&printed).unwrap();
        assert_eq!(converted, expected, "{text}");
        assert_eq!(converted.to_string(), expected.to_string(), "{text} scale");
    }
}

#[test]
fn nan_and_the_infinities_have_no_big_decimal() {
    for text in ["NaN", "Infinity", "-Infinity"] {
        let numeric = PgNumeric::parse(text).unwrap();
        assert_eq!(
            BigDecimal::try_from(&numeric),
            Err(NonFiniteNumeric),
            "{text}"
        );
    }
}
