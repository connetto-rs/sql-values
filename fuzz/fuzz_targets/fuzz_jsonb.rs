#![no_main]

use core::hash::BuildHasher;
use std::hash::RandomState;

use libfuzzer_sys::fuzz_target;
use sql_values::postgres::PgJsonb;

/// `jsonb_out`'s text reads back to an equal value that hashes alike and prints the same text.
fuzz_target!(|data: &[u8]| {
    let Ok(text) = core::str::from_utf8(data) else {
        return;
    };
    let Ok(value) = PgJsonb::parse(text) else {
        return;
    };
    let printed = value.to_string();
    let reread = PgJsonb::parse(&printed)
        .unwrap_or_else(|error| panic!("{text:?} printed as {printed:?}: {error}"));
    assert_eq!(reread, value, "{text:?} printed as {printed:?}");
    assert_eq!(reread.to_string(), printed, "{text:?} printed as {printed:?}");
    let hasher = RandomState::new();
    assert_eq!(hasher.hash_one(&reread), hasher.hash_one(&value), "{text:?}");
});
