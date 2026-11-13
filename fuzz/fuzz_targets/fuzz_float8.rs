#![no_main]

use libfuzzer_sys::fuzz_target;
use sql_values::postgres::PgFloat8;

/// `float8out`'s text reads back to the same double, bit for bit apart from `NaN`'s payload.
fn assert_round_trips(value: f64) {
    let printed = PgFloat8::from(value).to_string();
    let read = PgFloat8::parse(&printed)
        .unwrap_or_else(|error| panic!("{value:?} printed {printed:?}: {error}"))
        .value();
    if value.is_nan() {
        assert!(read.is_nan(), "{value:?} printed {printed:?}");
    } else {
        assert_eq!(read.to_bits(), value.to_bits(), "{value:?} printed {printed:?}");
    }
}

fuzz_target!(|data: &[u8]| {
    if let Ok(bytes) = <[u8; 8]>::try_from(data) {
        assert_round_trips(f64::from_bits(u64::from_le_bytes(bytes)));
    }
    let Ok(text) = core::str::from_utf8(data) else {
        return;
    };
    if let Ok(value) = PgFloat8::parse(text) {
        assert_round_trips(value.value());
    }
});
