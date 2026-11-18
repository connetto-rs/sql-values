//! `double precision` read as `float8in` reads it, printed as `float8out` prints it and ordered as
//! `float8_cmp_internal` orders it, every answer and every refusal's message measured on
//! PostgreSQL 18.

#[path = "support/answers.rs"]
mod answers;
#[path = "support/containers.rs"]
mod containers;
#[path = "support/exit_slot.rs"]
mod exit_slot;
#[path = "support/postgres.rs"]
mod postgres;

use answers::{Answer, compare, quoted};
use sql_values::postgres::{Float8Error, PgFloat8};

static PG18: postgres::Slot = exit_slot::ExitSlot::new();

const PG18_TAG: &str = "18.6";

/// Spellings across both notations, both output forms and the boundary between them, the
/// subnormal and overflow edges, every special value, hexadecimal and the refusals.
const INPUTS: &[&str] = &[
    "0",
    "-0",
    "+0.0",
    "1",
    "-1.5",
    "  3.25  ",
    "\t4\n",
    "2.5",
    "0.1",
    "0.3",
    "100",
    ".5",
    "5.",
    "1e10",
    "1E-10",
    "1e+2",
    "0.0001",
    "0.00001",
    "0.000123",
    "1e-4",
    "1e-5",
    "1.5e-7",
    "100000000000000",
    "1000000000000000",
    "123456789012345.6",
    "12345678901234.5",
    "1e15",
    "1e16",
    "123456789012345678",
    "9007199254740993",
    "1234567890123456789012345",
    "3.141592653589793",
    "1e100",
    "1e308",
    "1.7976931348623157e308",
    "1.7976931348623159e308",
    "1e309",
    "-1e309",
    "1e-320",
    "4.9e-324",
    "2e-324",
    "1e-400",
    "-1e-400",
    "0e-400",
    "NaN",
    "nan",
    "-nan",
    "NaN(123)",
    "Infinity",
    "-Infinity",
    "+Infinity",
    "infinity",
    "inf",
    "-INF",
    "infin",
    "0x1p3",
    "0x1.8p1",
    "-0x10",
    "0x",
    "0x1p99999",
    "0x1.00000000000008p-1075",
    "0x1.8p-1075",
    "0x1p-1075",
    "0x1.00000000000008p-1076",
    "0x1.fffffffffffff8p-1023",
    "0x1.fffffffffffff8p1023",
    "0x1.fffffffffffff8p0",
    "0x1.00000000000018p0",
    "0x1.000000000000080000000000000001p0",
    "0x1.00000000000007ffffffffffffffffp0",
    "0x1p-2000",
    "0x1p-99999999999",
    "-0x1.ffffffffffffffffp-1080",
    "+5",
    "+-5",
    "--5",
    "",
    "   ",
    "abc",
    "1.2.3",
    "1e",
    "1e+",
    ".",
    "1_000",
    "1 2",
];

/// The crate's answer in PostgreSQL's words, the syntax refusal naming the input as PostgreSQL's
/// message does.
fn crate_answer(input: &str) -> Answer {
    let read = PgFloat8::parse(input);
    if let Err(error @ Float8Error::InvalidInputSyntax) = &read {
        return Answer::Error(format!("{error}: \"{input}\""));
    }
    Answer::of(read)
}

#[test]
fn float8_reads_and_prints_as_postgresql_does() {
    let mut conn = answers::connect(&PG18, PG18_TAG);
    let cases = INPUTS
        .iter()
        .map(|input| {
            let query = format!("SELECT ({}::float8)::text", quoted(input));
            (query, crate_answer(input))
        })
        .collect();
    compare(&mut conn, cases);
}

/// The doubles PostgreSQL's own `float8` regression test prints, among them the ones where the
/// shortest spelling is a midpoint `float8out` avoids, such as `1e23`'s `9.999999999999999e+22`.
const BITS: &[u64] = &[
    0x0000000000000000,
    0x0000000000000001,
    0x0000000000000002,
    0x0000000000000003,
    0x0000000000001000,
    0x00000000000f4240,
    0x00000000016e3600,
    0x0000000100000000,
    0x0000008cdcdea440,
    0x0000010000000000,
    0x0000010100000000,
    0x0000400000000000,
    0x0000400100000000,
    0x0000800000000000,
    0x0000800000000001,
    0x000ffffffffffff0,
    0x000ffffffffffff1,
    0x000ffffffffffffe,
    0x000fffffffffffff,
    0x0010000000000000,
    0x0010000000000001,
    0x0010000000000002,
    0x0018000000000000,
    0x0040000000000000,
    0x007fffffffffffff,
    0x0290000000000000,
    0x029fffffffffffff,
    0x1330000000000000,
    0x133fffffffffffff,
    0x3a6fa7161a4d6e0c,
    0x3ddb7cdfd9d7bdba,
    0x3ddb7cdfd9d7bdbb,
    0x3ddb7cdfd9d7bdbc,
    0x3e112e0be826d694,
    0x3e112e0be826d695,
    0x3e112e0be826d696,
    0x3e45798ee2308c39,
    0x3e45798ee2308c3a,
    0x3e45798ee2308c3b,
    0x3e60000000000000,
    0x3e7ad7f29abcaf47,
    0x3e7ad7f29abcaf48,
    0x3e7ad7f29abcaf49,
    0x3eb0c6f7a0b5ed8c,
    0x3eb0c6f7a0b5ed8d,
    0x3eb0c6f7a0b5ed8e,
    0x3ee4f8b588e368ef,
    0x3ee4f8b588e368f0,
    0x3ee4f8b588e368f1,
    0x3f1a36e2eb1c432c,
    0x3f1a36e2eb1c432d,
    0x3f1a36e2eb1c432e,
    0x3f50624dd2f1a9fb,
    0x3f50624dd2f1a9fc,
    0x3f50624dd2f1a9fd,
    0x3f847ae147ae147a,
    0x3f847ae147ae147b,
    0x3f847ae147ae147c,
    0x3fb9999999999999,
    0x3fb999999999999a,
    0x3fb999999999999b,
    0x3feffffffffffff0,
    0x3feffffffffffff1,
    0x3feffffffffffff2,
    0x3feffffffffffff3,
    0x3feffffffffffff4,
    0x3feffffffffffff5,
    0x3feffffffffffff6,
    0x3feffffffffffff7,
    0x3feffffffffffff8,
    0x3feffffffffffff9,
    0x3feffffffffffffa,
    0x3feffffffffffffb,
    0x3feffffffffffffc,
    0x3feffffffffffffd,
    0x3feffffffffffffe,
    0x3fefffffffffffff,
    0x3ff0000000000000,
    0x3ff0000000000001,
    0x3ff0000000000002,
    0x3ff0000000000003,
    0x3ff0000000000004,
    0x3ff0000000000005,
    0x3ff0000000000006,
    0x3ff0000000000007,
    0x3ff0000000000008,
    0x3ff0000000000009,
    0x3ff3333333333333,
    0x3ff3ae147ae147ae,
    0x3ff3be76c8b43958,
    0x3ff3c083126e978d,
    0x3ff3c0c1fc8f3238,
    0x3ff3c0c9539b8887,
    0x3ff3c0ca2a5b1d5d,
    0x3ff3c0ca4283de1b,
    0x3ff3c0ca428abd53,
    0x3ff3c0ca428c1d2b,
    0x3ff3c0ca428c51f2,
    0x3ff3c0ca428c58fc,
    0x3ff3c0ca428c59dd,
    0x3ff3c0ca428c59f8,
    0x3ff3c0ca428c59fb,
    0x3ff3c0ca43db770a,
    0x3ff921fb54442d18,
    0x4005bf0a8b14576a,
    0x400921fb54442d18,
    0x40112e0be8047a7d,
    0x40112e0be815a889,
    0x40112e0be826d695,
    0x40112e0be83804a1,
    0x40112e0be84932ad,
    0x4023ffffffffffff,
    0x4024000000000000,
    0x4024000000000001,
    0x4028b0fcd32f7076,
    0x4058ffffffffffff,
    0x4059000000000000,
    0x4059000000000001,
    0x405edd3c07fb4c93,
    0x408f3fffffffffff,
    0x408f400000000000,
    0x408f400000000001,
    0x40934a4584fd0fdc,
    0x40c387ffffffffff,
    0x40c3880000000000,
    0x40c3880000000001,
    0x40c81cd6e63c53d3,
    0x40f869ffffffffff,
    0x40f86a0000000000,
    0x40f86a0000000001,
    0x40fe240c9fcb68c8,
    0x412e847fffffffff,
    0x412e848000000000,
    0x412e848000000001,
    0x4132d687e3df217d,
    0x416312cfffffffff,
    0x416312d000000000,
    0x416312d000000001,
    0x41678c29dcd6e9dc,
    0x4197d783ffffffff,
    0x4197d78400000000,
    0x4197d78400000001,
    0x419d6f34540ca453,
    0x41cdcd64ffffffff,
    0x41cdcd6500000000,
    0x41cdcd6500000001,
    0x41d26580b487e6b4,
    0x4202a05f1fffffff,
    0x4202a05f20000000,
    0x4202a05f20000001,
    0x4206fee0e1a9e061,
    0x42374876e7ffffff,
    0x42374876e8000000,
    0x42374876e8000001,
    0x423cbe991a145879,
    0x426d1a94a1ffffff,
    0x426d1a94a2000000,
    0x426d1a94a2000001,
    0x4271f71fb04cb74c,
    0x42a2309ce53fffff,
    0x42a2309ce5400000,
    0x42a2309ce5400001,
    0x42a674e79c5fe51f,
    0x42d6bcc41e8fffff,
    0x42d6bcc41e900000,
    0x42d6bcc41e900001,
    0x42dc12218377de66,
    0x430c6bf52633ffff,
    0x430c6bf526340000,
    0x430c6bf526340001,
    0x434018601510c000,
    0x4341c37937e07fff,
    0x4341c37937e08000,
    0x4341c37937e08001,
    0x4350000000000000,
    0x4350000000000002,
    0x4350000000002e06,
    0x4352000000000003,
    0x4352000000000004,
    0x4358000000000003,
    0x4358000000000004,
    0x435f000000000020,
    0x435fffffffffffff,
    0x4376345785d89fff,
    0x4376345785d8a000,
    0x4376345785d8a001,
    0x43abc16d674ec7ff,
    0x43abc16d674ec800,
    0x43abc16d674ec801,
    0x43d055dc36f24000,
    0x43e052961c6f8000,
    0x43e158e460913cff,
    0x43e158e460913d00,
    0x43e158e460913d01,
    0x4415af1d78b58c3f,
    0x4415af1d78b58c40,
    0x4415af1d78b58c41,
    0x444b1ae4d6e2ef4f,
    0x444b1ae4d6e2ef50,
    0x444b1ae4d6e2ef51,
    0x4480f0cf064dd591,
    0x4480f0cf064dd592,
    0x4480f0cf064dd593,
    0x44b52d02c7e14af5,
    0x44b52d02c7e14af6,
    0x44b52d02c7e14af7,
    0x44ea784379d99db3,
    0x44ea784379d99db4,
    0x44ea784379d99db5,
    0x45208b2a2c280290,
    0x45208b2a2c280291,
    0x45208b2a2c280292,
    0x4830f0cf064dd592,
    0x4840f0cf064dd592,
    0x4850f0cf064dd592,
    0x7feffffffffffffe,
    0x7fefffffffffffff,
    0xc350000000000002,
    0xc350000000002e06,
    0xc352000000000003,
    0xc352000000000004,
    0xc352bd2668e077c4,
    0xc358000000000003,
    0xc358000000000004,
    0xc35f000000000020,
];

/// `bits` as a hexadecimal `strtod` reads exactly, so PostgreSQL holds the very same double.
fn hex_spelling(bits: u64) -> String {
    let sign = if bits >> 63 == 1 { "-" } else { "" };
    let exponent = (bits >> 52) & 0x7ff;
    let fraction = bits & ((1 << 52) - 1);
    if exponent == 0 {
        format!("{sign}0x0.{fraction:013x}p-1022")
    } else {
        format!(
            "{sign}0x1.{fraction:013x}p{}",
            i64::try_from(exponent).unwrap() - 1023
        )
    }
}

/// PostgreSQL's spelling of each double against the crate's, every one handed over in a
/// hexadecimal spelling `strtod` reads exactly.
fn compare_doubles(doubles: impl Iterator<Item = u64>) {
    let mut conn = answers::connect(&PG18, PG18_TAG);
    let cases = doubles
        .map(|bits| {
            let spelling = hex_spelling(bits);
            let read = PgFloat8::parse(&spelling).unwrap().value().to_bits();
            assert_eq!(read, bits, "{spelling} reads back to {bits:#018x}");
            let query = format!("SELECT ({}::float8)::text", quoted(&spelling));
            let printed = PgFloat8::from(f64::from_bits(bits)).to_string();
            (query, Answer::Value(printed))
        })
        .collect();
    compare(&mut conn, cases);
}

#[test]
fn float8_prints_postgresql_regression_doubles_as_postgresql_does() {
    compare_doubles(BITS.iter().copied());
}

/// Finite doubles spread over every exponent, and the integers and powers of two beside each
/// power of ten, where a midpoint is most often the shortest spelling.
#[test]
fn float8_prints_spread_doubles_as_postgresql_does() {
    let mut state = 0x9e37_79b9_7f4a_7c15_u64;
    let random = core::iter::from_fn(move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        Some(state)
    })
    .filter(|&bits| (bits >> 52) & 0x7ff != 0x7ff)
    .take(4000);
    let near_powers = (0..=22).flat_map(|exponent| {
        let power = 10_f64.powi(exponent);
        [power.to_bits() - 1, power.to_bits(), power.to_bits() + 1]
    });
    let powers_of_two = (-1074..=1023).map(|exponent| 2_f64.powi(exponent).to_bits());
    compare_doubles(random.chain(near_powers).chain(powers_of_two));
}

#[test]
fn float8_orders_as_postgresql_does() {
    let mut conn = answers::connect(&PG18, PG18_TAG);
    let values = [
        "-Infinity",
        "-1e308",
        "-1",
        "-0",
        "0",
        "4.9e-324",
        "0.1",
        "1",
        "1e308",
        "Infinity",
        "NaN",
    ];
    let cases = values
        .iter()
        .flat_map(|left| values.iter().map(move |right| (*left, *right)))
        .map(|(left, right)| {
            let query = format!(
                "SELECT ({l}::float8 < {r}::float8)::text || '|' || ({l}::float8 = {r}::float8)::text",
                l = quoted(left),
                r = quoted(right),
            );
            let (left, right) = (
                PgFloat8::parse(left).unwrap(),
                PgFloat8::parse(right).unwrap(),
            );
            (
                query,
                Answer::Value(format!("{}|{}", left < right, left == right)),
            )
        })
        .collect();
    compare(&mut conn, cases);
}
