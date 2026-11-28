#![no_main]

use libfuzzer_sys::fuzz_target;
use sql_values::postgres::PgUuid;

/// `uuid_out`'s text of any 16 bytes reads back to them, and any text `uuid_in` reads prints its
/// own digits in lowercase and reads back to the same value.
fuzz_target!(|data: &[u8]| {
    if let Ok(bytes) = <[u8; 16]>::try_from(data) {
        let uuid = PgUuid::from(bytes);
        assert_eq!(PgUuid::parse(&uuid.to_string()), Ok(uuid), "{data:?}");
    }
    let Ok(text) = core::str::from_utf8(data) else {
        return;
    };
    if let Ok(uuid) = PgUuid::parse(text) {
        let printed = uuid.to_string();
        assert_eq!(PgUuid::parse(&printed), Ok(uuid), "{text:?} printed as {printed:?}");
        let digits: String = text
            .chars()
            .filter(char::is_ascii_hexdigit)
            .map(|digit| digit.to_ascii_lowercase())
            .collect();
        assert_eq!(digits, printed.replace('-', ""), "{text:?} printed as {printed:?}");
    }
});
