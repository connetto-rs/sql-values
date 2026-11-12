#![no_main]

use libfuzzer_sys::fuzz_target;
use sql_values::PgBytea;

/// Both of `byteaout`'s spellings of `bytea` read back to it.
fn assert_round_trips(bytea: &PgBytea, origin: &dyn core::fmt::Debug) {
    for spelling in [bytea.to_string(), bytea.escaped().to_string()] {
        assert_eq!(
            PgBytea::parse(&spelling).as_ref(),
            Ok(bytea),
            "{origin:?} as {spelling:?}"
        );
    }
}

fuzz_target!(|data: &[u8]| {
    assert_round_trips(&PgBytea::from(data.to_vec()), &data);
    let Ok(text) = core::str::from_utf8(data) else {
        return;
    };
    if let Ok(bytea) = PgBytea::parse(text) {
        assert_round_trips(&bytea, &text);
    }
});
