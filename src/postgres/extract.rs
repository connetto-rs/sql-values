//! PostgreSQL 18's `extract` and `date_part` of `date`, `time`, `timetz`, `timestamp`,
//! `timestamptz` and `interval`, a field as `numeric` and as `double precision`.

use core::fmt;

use super::{
    DateTimeError, PgDate, PgFloat8, PgInterval, PgNumeric, PgTime, PgTimeTz, PgTimeZone,
    PgTimestamp, PgTimestampTz, PgType,
    datetime::{
        POSTGRES_EPOCH_JDATE, UNIX_TO_POSTGRES_SECS, USECS_PER_DAY, USECS_PER_SEC, date2j, j2date,
        split_clock, timestamp2tm,
        tokens::{
            DTK_CENTURY, DTK_DAY, DTK_DECADE, DTK_DOW, DTK_DOY, DTK_EPOCH, DTK_HOUR, DTK_ISODOW,
            DTK_ISOYEAR, DTK_JULIAN, DTK_MICROSEC, DTK_MILLENNIUM, DTK_MILLISEC, DTK_MINUTE,
            DTK_MONTH, DTK_QUARTER, DTK_SECOND, DTK_TZ, DTK_TZ_HOUR, DTK_TZ_MINUTE, DTK_WEEK,
            DTK_YEAR, RESERV, UNITS,
        },
        utc_offset,
    },
    float::int_to_f64,
    interval::decode_extract_unit,
};

const SECS_PER_DAY: i64 = 86_400;
/// `SetEpochTimestamp`, 1970-01-01 00:00:00 as a `timestamp`.
const EPOCH_MICROS: i64 = -UNIX_TO_POSTGRES_SECS * USECS_PER_SEC;

/// A field `extract` and `date_part` take, printed as PostgreSQL spells it in its errors.
///
/// ```
/// use sql_values::postgres::ExtractField;
///
/// assert_eq!(ExtractField::parse("SECS"), Some(ExtractField::Second));
/// assert_eq!(ExtractField::parse("timezone_hour"), Some(ExtractField::TimezoneHour));
/// assert_eq!(ExtractField::DayOfWeek.to_string(), "dow");
/// assert_eq!(ExtractField::parse("fortnight"), None);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ExtractField {
    /// `microseconds`, the seconds with their fraction, in microseconds.
    Microseconds,
    /// `milliseconds`, the seconds with their fraction, in milliseconds.
    Milliseconds,
    /// `second`, the seconds with their fraction.
    Second,
    /// `minute`.
    Minute,
    /// `hour`.
    Hour,
    /// `day`, of the month, or an interval's days.
    Day,
    /// `week`, the ISO 8601 week of the year, or an interval's days in whole weeks.
    Week,
    /// `month`, of the year, or an interval's months beyond its whole years.
    Month,
    /// `quarter`.
    Quarter,
    /// `year`, where 1 BC is `-1`.
    Year,
    /// `decade`, the year divided by 10.
    Decade,
    /// `century`, where the 21st starts on 2001-01-01.
    Century,
    /// `millennium`, where the 3rd starts on 2001-01-01.
    Millennium,
    /// `dow`, the day of the week from Sunday as `0`.
    DayOfWeek,
    /// `isodow`, the day of the week from Monday as `1` to Sunday as `7`.
    IsoDayOfWeek,
    /// `doy`, the day of the year from `1`.
    DayOfYear,
    /// `isoyear`, the ISO 8601 week-numbering year.
    IsoYear,
    /// `epoch`, seconds since 1970-01-01 00:00:00 UTC, or an interval's length in seconds.
    Epoch,
    /// `julian`, the Julian day with the time of day as its fraction.
    Julian,
    /// `timezone`, the offset east of UTC in seconds.
    Timezone,
    /// `timezone_hour`, the whole hours of the offset.
    TimezoneHour,
    /// `timezone_minute`, the minutes of the offset beyond its whole hours.
    TimezoneMinute,
}

impl ExtractField {
    /// The field PostgreSQL reads `text` as, lowercased and looked up by its first 10 bytes among
    /// the interval units and then the date keywords, so aliases such as `secs`, `msec`, `y`,
    /// `j` and `timezone_h` name fields too. `None` for any other word, which PostgreSQL refuses
    /// as not recognized, or as not supported by `date` and `timestamp` when it is a reserved
    /// word such as `now`.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        match decode_extract_unit(text) {
            (RESERV, DTK_EPOCH) => Some(Self::Epoch),
            (UNITS, unit) => Some(match unit {
                DTK_MICROSEC => Self::Microseconds,
                DTK_MILLISEC => Self::Milliseconds,
                DTK_SECOND => Self::Second,
                DTK_MINUTE => Self::Minute,
                DTK_HOUR => Self::Hour,
                DTK_DAY => Self::Day,
                DTK_WEEK => Self::Week,
                DTK_MONTH => Self::Month,
                DTK_QUARTER => Self::Quarter,
                DTK_YEAR => Self::Year,
                DTK_DECADE => Self::Decade,
                DTK_CENTURY => Self::Century,
                DTK_MILLENNIUM => Self::Millennium,
                DTK_DOW => Self::DayOfWeek,
                DTK_ISODOW => Self::IsoDayOfWeek,
                DTK_DOY => Self::DayOfYear,
                DTK_ISOYEAR => Self::IsoYear,
                DTK_JULIAN => Self::Julian,
                DTK_TZ => Self::Timezone,
                DTK_TZ_HOUR => Self::TimezoneHour,
                DTK_TZ_MINUTE => Self::TimezoneMinute,
                _ => return None,
            }),
            _ => None,
        }
    }

    /// Whether an infinite `date`, `timestamp` or `timestamptz` has the infinity of its sign in
    /// the field, as `NonFiniteTimestampTzPart` sorts the fields, rather than `NULL`.
    const fn grows_with_timestamp(self) -> bool {
        matches!(
            self,
            Self::Year
                | Self::Decade
                | Self::Century
                | Self::Millennium
                | Self::Julian
                | Self::IsoYear
                | Self::Epoch
        )
    }
}

impl fmt::Display for ExtractField {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Microseconds => "microseconds",
            Self::Milliseconds => "milliseconds",
            Self::Second => "second",
            Self::Minute => "minute",
            Self::Hour => "hour",
            Self::Day => "day",
            Self::Week => "week",
            Self::Month => "month",
            Self::Quarter => "quarter",
            Self::Year => "year",
            Self::Decade => "decade",
            Self::Century => "century",
            Self::Millennium => "millennium",
            Self::DayOfWeek => "dow",
            Self::IsoDayOfWeek => "isodow",
            Self::DayOfYear => "doy",
            Self::IsoYear => "isoyear",
            Self::Epoch => "epoch",
            Self::Julian => "julian",
            Self::Timezone => "timezone",
            Self::TimezoneHour => "timezone_hour",
            Self::TimezoneMinute => "timezone_minute",
        })
    }
}

/// Why PostgreSQL refuses `extract` or `date_part`, or why this crate cannot answer without a
/// wider zone database.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, thiserror::Error)]
pub enum ExtractError {
    /// A field the input's type does not have.
    #[error("unit \"{field}\" not supported for type {input_type}")]
    UnsupportedField {
        /// The field asked for.
        field: ExtractField,
        /// The type PostgreSQL extracts from.
        input_type: PgType,
    },
    /// `date_part` of a date from 294277-01-01, which it casts to `timestamp`.
    #[error("date out of range for timestamp")]
    DateOutOfRangeForTimestamp,
    /// The local clock of a `timestamptz` in a zone that cannot answer for it.
    #[error(transparent)]
    DateTime(#[from] DateTimeError),
}

/// A field as `extract` computes it in `numeric` or `date_part` in `double precision`, each
/// with PostgreSQL's own arithmetic.
trait Part: Sized {
    /// `intresult`.
    fn integer(value: i64) -> Self;

    /// The infinity of the sign of an infinite input.
    fn infinity(negative: bool) -> Self;

    /// `milliseconds` of the clock's seconds and microseconds.
    fn milliseconds(seconds: i64, micros: i64) -> Self;

    /// `second` of the clock's seconds and microseconds.
    fn seconds(seconds: i64, micros: i64) -> Self;

    /// `julian` of a `timestamp` on the Julian day `day` at `clock` microseconds after midnight.
    fn julian(day: i32, clock: i64) -> Self;

    /// `epoch` of a finite `timestamp` or `timestamptz`.
    fn timestamp_epoch(micros: i64) -> Self;

    /// `epoch` of a `time`, or of a `timetz` `zone` seconds west of UTC.
    fn clock_epoch(clock: i64, zone: i32) -> Self;

    /// `epoch` of a finite `interval`.
    fn interval_epoch(interval: PgInterval) -> Self;
}

impl Part for PgNumeric {
    fn integer(value: i64) -> Self {
        Self::from(value)
    }

    fn infinity(negative: bool) -> Self {
        if negative {
            Self::NEG_INFINITY
        } else {
            Self::INFINITY
        }
    }

    fn milliseconds(seconds: i64, micros: i64) -> Self {
        Self::from_scaled_i64(seconds * USECS_PER_SEC + micros, 3)
    }

    fn seconds(seconds: i64, micros: i64) -> Self {
        Self::from_scaled_i64(seconds * USECS_PER_SEC + micros, 6)
    }

    fn julian(day: i32, clock: i64) -> Self {
        Self::from(clock)
            .checked_div(&Self::from(USECS_PER_DAY))
            .and_then(|fraction| Self::from(day).checked_add(&fraction))
            .expect("a Julian day and its fraction fit numeric")
    }

    fn timestamp_epoch(micros: i64) -> Self {
        if micros < i64::MAX + EPOCH_MICROS {
            return Self::from_scaled_i64(micros - EPOCH_MICROS, 6);
        }
        Self::from(micros)
            .checked_sub(&Self::from(EPOCH_MICROS))
            .and_then(|span| span.checked_div(&Self::from(USECS_PER_SEC)))
            .and_then(|seconds| seconds.round(6))
            .expect("a timestamp's epoch fits numeric")
    }

    fn clock_epoch(clock: i64, zone: i32) -> Self {
        Self::from_scaled_i64(clock + i64::from(zone) * USECS_PER_SEC, 6)
    }

    fn interval_epoch(interval: PgInterval) -> Self {
        // PostgreSQL counts quarter days so that a year of 365.25 days stays whole.
        let quarter_days = 1_461 * i64::from(interval.months / 12)
            + 120 * i64::from(interval.months % 12)
            + 4 * i64::from(interval.days);
        let seconds = quarter_days * (SECS_PER_DAY / 4);
        match seconds
            .checked_mul(USECS_PER_SEC)
            .and_then(|micros| micros.checked_add(interval.microseconds))
        {
            Some(micros) => Self::from_scaled_i64(micros, 6),
            None => Self::from_scaled_i64(interval.microseconds, 6)
                .checked_add(&Self::from(seconds))
                .expect("an interval's epoch fits numeric"),
        }
    }
}

impl Part for PgFloat8 {
    fn integer(value: i64) -> Self {
        Self::from(int_to_f64(value))
    }

    fn infinity(negative: bool) -> Self {
        Self::from(if negative {
            f64::NEG_INFINITY
        } else {
            f64::INFINITY
        })
    }

    fn milliseconds(seconds: i64, micros: i64) -> Self {
        Self::from(int_to_f64(seconds) * 1_000.0 + int_to_f64(micros) / 1_000.0)
    }

    fn seconds(seconds: i64, micros: i64) -> Self {
        Self::from(int_to_f64(seconds) + int_to_f64(micros) / 1_000_000.0)
    }

    fn julian(day: i32, clock: i64) -> Self {
        let seconds =
            int_to_f64(clock / USECS_PER_SEC) + int_to_f64(clock % USECS_PER_SEC) / 1_000_000.0;
        Self::from(f64::from(day) + seconds / 86_400.0)
    }

    fn timestamp_epoch(micros: i64) -> Self {
        Self::from(if micros < i64::MAX + EPOCH_MICROS {
            int_to_f64(micros - EPOCH_MICROS) / 1_000_000.0
        } else {
            (int_to_f64(micros) - int_to_f64(EPOCH_MICROS)) / 1_000_000.0
        })
    }

    fn clock_epoch(clock: i64, zone: i32) -> Self {
        Self::from(int_to_f64(clock) / 1_000_000.0 + f64::from(zone))
    }

    fn interval_epoch(interval: PgInterval) -> Self {
        let mut seconds = int_to_f64(interval.microseconds) / 1_000_000.0;
        seconds += 365.25 * 86_400.0 * f64::from(interval.months / 12);
        seconds += 30.0 * 86_400.0 * f64::from(interval.months % 12);
        seconds += 86_400.0 * f64::from(interval.days);
        Self::from(seconds)
    }
}

impl ExtractError {
    const fn unsupported(field: ExtractField, input_type: PgType) -> Self {
        Self::UnsupportedField { field, input_type }
    }
}

/// The clock fields of `clock` microseconds, split toward zero as `time2tm` splits a time of
/// day and `interval2itm` an interval's time, or `None` for another field.
fn clock_part<P: Part>(field: ExtractField, clock: i64) -> Option<P> {
    let (hour, minute, second, micros) = split_clock(clock);
    Some(match field {
        ExtractField::Microseconds => P::integer(second * USECS_PER_SEC + micros),
        ExtractField::Milliseconds => P::milliseconds(second, micros),
        ExtractField::Second => P::seconds(second, micros),
        ExtractField::Minute => P::integer(minute),
        ExtractField::Hour => P::integer(hour),
        _ => return None,
    })
}

/// The calendar fields of a date, year `0` being 1 BC, as `extract_date` and
/// `timestamp_part_common` compute them, or `None` for another field.
fn calendar_part<P: Part>(field: ExtractField, year: i32, month: i32, day: i32) -> Option<P> {
    let julian = date2j(year, month, day);
    let value = match field {
        ExtractField::Day => day,
        ExtractField::Month => month,
        ExtractField::Quarter => (month - 1) / 3 + 1,
        ExtractField::Week => iso_year_week(year, julian).1,
        ExtractField::Year if year > 0 => year,
        ExtractField::Year => year - 1,
        ExtractField::Decade if year >= 0 => year / 10,
        ExtractField::Decade => -((8 - (year - 1)) / 10),
        ExtractField::Century if year > 0 => (year + 99) / 100,
        ExtractField::Century => -((99 - (year - 1)) / 100),
        ExtractField::Millennium if year > 0 => (year + 999) / 1_000,
        ExtractField::Millennium => -((999 - (year - 1)) / 1_000),
        ExtractField::IsoYear => match iso_year_week(year, julian).0 {
            iso_year if iso_year <= 0 => iso_year - 1,
            iso_year => iso_year,
        },
        ExtractField::DayOfWeek => j2day(julian),
        ExtractField::IsoDayOfWeek => match j2day(julian) {
            0 => 7,
            weekday => weekday,
        },
        ExtractField::DayOfYear => julian - date2j(year, 1, 1) + 1,
        _ => return None,
    };
    Some(P::integer(value.into()))
}

/// `j2day`, the day of the week of a Julian day, Sunday being `0`.
const fn j2day(julian: i32) -> i32 {
    (julian + 1).rem_euclid(7)
}

/// The Julian day of the Monday that starts week 1 of the ISO 8601 `year`.
const fn iso_year_start(year: i32) -> i32 {
    let fourth = date2j(year, 1, 4);
    fourth - j2day(fourth - 1)
}

/// `date2isoyear` and `date2isoweek`, the ISO 8601 year and week of the Julian day `julian` in
/// the calendar `year`.
const fn iso_year_week(year: i32, julian: i32) -> (i32, i32) {
    let (mut iso_year, mut start) = (year, iso_year_start(year));
    if julian < start {
        iso_year -= 1;
        start = iso_year_start(iso_year);
    }
    let week = (julian - start) / 7 + 1;
    if week >= 52 {
        let next = iso_year_start(iso_year + 1);
        if julian >= next {
            return (iso_year + 1, (julian - next) / 7 + 1);
        }
    }
    (iso_year, week)
}

/// `timestamp_part_common`, or `timestamptz_part_common` in `zone`.
fn timestamp_part<P: Part>(
    micros: i64,
    field: ExtractField,
    zone: Option<&PgTimeZone>,
) -> Result<Option<P>, ExtractError> {
    if micros == i64::MAX || micros == i64::MIN {
        return Ok(field
            .grows_with_timestamp()
            .then(|| P::infinity(micros == i64::MIN)));
    }
    if field == ExtractField::Epoch {
        return Ok(Some(P::timestamp_epoch(micros)));
    }
    let offset = match zone {
        Some(zone) => utc_offset(zone, micros)?,
        None => 0,
    };
    let (year, month, day, clock) = timestamp2tm(micros + i64::from(offset) * USECS_PER_SEC);
    let part = match field {
        ExtractField::Julian => P::julian(date2j(year, month, day), clock),
        ExtractField::Timezone if zone.is_some() => P::integer(offset.into()),
        ExtractField::TimezoneHour if zone.is_some() => P::integer((offset / 3_600).into()),
        ExtractField::TimezoneMinute if zone.is_some() => P::integer((offset / 60 % 60).into()),
        _ => clock_part(field, clock)
            .or_else(|| calendar_part(field, year, month, day))
            .ok_or_else(|| {
                let input_type = if zone.is_some() {
                    PgType::TimestampTz
                } else {
                    PgType::Timestamp
                };
                ExtractError::unsupported(field, input_type)
            })?,
    };
    Ok(Some(part))
}

/// `time_part_common`, or `timetz_part_common` of a time at `offset` seconds east of UTC.
fn time_part<P: Part>(
    clock: i64,
    offset: Option<i32>,
    field: ExtractField,
) -> Result<Option<P>, ExtractError> {
    let part = match (field, offset) {
        (ExtractField::Epoch, _) => P::clock_epoch(clock, -offset.unwrap_or(0)),
        (ExtractField::Timezone, Some(offset)) => P::integer(offset.into()),
        (ExtractField::TimezoneHour, Some(offset)) => P::integer((offset / 3_600).into()),
        (ExtractField::TimezoneMinute, Some(offset)) => P::integer((offset / 60 % 60).into()),
        _ => clock_part(field, clock).ok_or_else(|| {
            let input_type = if offset.is_some() {
                PgType::TimeTz
            } else {
                PgType::Time
            };
            ExtractError::unsupported(field, input_type)
        })?,
    };
    Ok(Some(part))
}

impl PgDate {
    /// PostgreSQL's `extract(field FROM date)`, `extract_date`, an integer in `numeric`. An
    /// infinite date has the infinity of its sign in the fields that grow with time and `None`,
    /// SQL `NULL`, in the others.
    ///
    /// # Errors
    ///
    /// [`ExtractError::UnsupportedField`] for a clock or zone field.
    ///
    /// ```
    /// use sql_values::postgres::{ExtractField, PgDate, PgNumeric};
    ///
    /// let date = PgDate::from_ymd(2006, 1, 1).unwrap();
    /// assert_eq!(date.extract(ExtractField::IsoYear).unwrap(), Some(PgNumeric::from(2005)));
    /// assert!(date.extract(ExtractField::Hour).is_err());
    /// assert_eq!(PgDate::INFINITY.extract(ExtractField::Day).unwrap(), None);
    /// ```
    pub fn extract(self, field: ExtractField) -> Result<Option<PgNumeric>, ExtractError> {
        if !self.is_finite() {
            return match field {
                ExtractField::Day
                | ExtractField::Week
                | ExtractField::Month
                | ExtractField::Quarter
                | ExtractField::DayOfWeek
                | ExtractField::IsoDayOfWeek
                | ExtractField::DayOfYear => Ok(None),
                _ if field.grows_with_timestamp() => {
                    Ok(Some(PgNumeric::infinity(self == Self::NEG_INFINITY)))
                }
                _ => Err(ExtractError::unsupported(field, PgType::Date)),
            };
        }
        let julian = self.pg_days() + POSTGRES_EPOCH_JDATE;
        let (year, month, day) = j2date(julian);
        match field {
            ExtractField::Julian => Ok(Some(PgNumeric::from(julian))),
            ExtractField::Epoch => Ok(Some(PgNumeric::from(
                i64::from(self.pg_days()) * SECS_PER_DAY + UNIX_TO_POSTGRES_SECS,
            ))),
            _ => calendar_part(field, year, month, day)
                .map(Some)
                .ok_or(ExtractError::unsupported(field, PgType::Date)),
        }
    }

    /// PostgreSQL's `date_part(field, date)`, which casts the date to `timestamp`, so a clock
    /// field is `0` and a zone field is unsupported by `timestamp`.
    ///
    /// # Errors
    ///
    /// [`ExtractError::DateOutOfRangeForTimestamp`] for a date from 294277-01-01, and
    /// [`ExtractError::UnsupportedField`] for a zone field.
    ///
    /// ```
    /// use sql_values::postgres::{ExtractField, PgDate};
    ///
    /// let date = PgDate::from_ymd(2006, 1, 1).unwrap();
    /// assert_eq!(date.date_part(ExtractField::Hour).unwrap().unwrap().value(), 0.0);
    /// ```
    pub fn date_part(self, field: ExtractField) -> Result<Option<PgFloat8>, ExtractError> {
        self.to_timestamp()
            .map_err(|_| ExtractError::DateOutOfRangeForTimestamp)?
            .date_part(field)
    }
}

impl PgTime {
    /// PostgreSQL's `extract(field FROM time)`.
    ///
    /// # Errors
    ///
    /// [`ExtractError::UnsupportedField`] for a field other than a clock field or `epoch`.
    ///
    /// ```
    /// use sql_values::postgres::{ExtractField, PgTime};
    ///
    /// let time = PgTime::from_hms_micro(23, 59, 59, 123_456).unwrap();
    /// let epoch = time.extract(ExtractField::Epoch).unwrap().unwrap();
    /// assert_eq!(epoch.to_string(), "86399.123456");
    /// ```
    pub fn extract(self, field: ExtractField) -> Result<Option<PgNumeric>, ExtractError> {
        time_part(self.micros(), None, field)
    }

    /// PostgreSQL's `date_part(field, time)`.
    ///
    /// # Errors
    ///
    /// [`ExtractError::UnsupportedField`] for a field other than a clock field or `epoch`.
    pub fn date_part(self, field: ExtractField) -> Result<Option<PgFloat8>, ExtractError> {
        time_part(self.micros(), None, field)
    }
}

impl PgTimeTz {
    /// PostgreSQL's `extract(field FROM timetz)`, the clock as written and its own offset.
    ///
    /// # Errors
    ///
    /// [`ExtractError::UnsupportedField`] for a field other than a clock or zone field or
    /// `epoch`.
    ///
    /// ```
    /// use sql_values::postgres::{ExtractField, PgNumeric, PgTime, PgTimeTz};
    ///
    /// let time = PgTime::from_hms_micro(12, 34, 56, 0).unwrap();
    /// let india = PgTimeTz::new(time, 19_800).unwrap();
    /// assert_eq!(india.extract(ExtractField::TimezoneMinute).unwrap(), Some(PgNumeric::from(30)));
    /// ```
    pub fn extract(self, field: ExtractField) -> Result<Option<PgNumeric>, ExtractError> {
        time_part(self.time().micros(), Some(self.offset()), field)
    }

    /// PostgreSQL's `date_part(field, timetz)`.
    ///
    /// # Errors
    ///
    /// [`ExtractError::UnsupportedField`] for a field other than a clock or zone field or
    /// `epoch`.
    pub fn date_part(self, field: ExtractField) -> Result<Option<PgFloat8>, ExtractError> {
        time_part(self.time().micros(), Some(self.offset()), field)
    }
}

impl PgTimestamp {
    /// PostgreSQL's `extract(field FROM timestamp)`, at PostgreSQL's display scale: six digits
    /// after the point for `second` and `epoch`, three for `milliseconds`, and the quotient's
    /// for `julian`. An infinite timestamp has the infinity of its sign in the fields that grow
    /// with time and `None`, SQL `NULL`, in the others.
    ///
    /// # Errors
    ///
    /// [`ExtractError::UnsupportedField`] for a zone field.
    ///
    /// ```
    /// use sql_values::postgres::{ExtractField, InputContext, PgNumeric, PgTimestamp};
    ///
    /// let at = PgTimestamp::parse("2006-01-01 12:34:56.123456", InputContext::new()).unwrap();
    /// let julian = at.extract(ExtractField::Julian).unwrap().unwrap();
    /// assert_eq!(julian.to_string(), "2453737.52426068814814814815");
    /// assert_eq!(at.extract(ExtractField::Week).unwrap(), Some(PgNumeric::from(52)));
    /// assert_eq!(PgTimestamp::INFINITY.extract(ExtractField::Second).unwrap(), None);
    /// ```
    pub fn extract(self, field: ExtractField) -> Result<Option<PgNumeric>, ExtractError> {
        timestamp_part(self.pg_micros(), field, None)
    }

    /// PostgreSQL's `date_part(field, timestamp)`, in `double precision` arithmetic.
    ///
    /// # Errors
    ///
    /// [`ExtractError::UnsupportedField`] for a zone field.
    ///
    /// ```
    /// use sql_values::postgres::{ExtractField, InputContext, PgTimestamp};
    ///
    /// let at = PgTimestamp::parse("0001-12-31 23:59:59.999999 BC", InputContext::new()).unwrap();
    /// assert_eq!(at.date_part(ExtractField::Epoch).unwrap().unwrap().value(), -62135596800.0);
    /// ```
    pub fn date_part(self, field: ExtractField) -> Result<Option<PgFloat8>, ExtractError> {
        timestamp_part(self.pg_micros(), field, None)
    }
}

impl PgTimestampTz {
    /// PostgreSQL's `extract(field FROM timestamptz)` with `zone` as the session's `TimeZone`:
    /// the calendar and clock fields of the local clock, the zone fields of the offset at the
    /// instant, and `epoch` of the instant alone.
    ///
    /// # Errors
    ///
    /// [`ExtractError::DateTime`] where an IANA zone is asked about an instant outside jiff's
    /// years.
    ///
    /// ```
    /// use sql_values::postgres::{ExtractField, InputContext, PgNumeric, PgTimeZone, PgTimestampTz};
    ///
    /// #[cfg(feature = "tzdb")]
    /// {
    ///     let zone = PgTimeZone::parse("America/New_York").unwrap();
    ///     let at = PgTimestampTz::parse("2024-03-10 07:30:00+00", InputContext::new()).unwrap();
    ///     assert_eq!(at.extract(ExtractField::Hour, &zone).unwrap(), Some(PgNumeric::from(3)));
    ///     let offset = at.extract(ExtractField::Timezone, &zone).unwrap();
    ///     assert_eq!(offset, Some(PgNumeric::from(-14_400)));
    /// }
    /// ```
    pub fn extract(
        self,
        field: ExtractField,
        zone: &PgTimeZone,
    ) -> Result<Option<PgNumeric>, ExtractError> {
        timestamp_part(self.pg_micros(), field, Some(zone))
    }

    /// PostgreSQL's `date_part(field, timestamptz)` with `zone` as the session's `TimeZone`.
    ///
    /// # Errors
    ///
    /// [`ExtractError::DateTime`] where an IANA zone is asked about an instant outside jiff's
    /// years.
    pub fn date_part(
        self,
        field: ExtractField,
        zone: &PgTimeZone,
    ) -> Result<Option<PgFloat8>, ExtractError> {
        timestamp_part(self.pg_micros(), field, Some(zone))
    }
}

impl PgInterval {
    /// PostgreSQL's `extract(field FROM interval)`, each field of the months, days and time
    /// split toward zero, and `epoch` with a year as 365.25 days and a month as 30. An infinite
    /// interval has the infinity of its sign in `hour`, `day`, the year fields and `epoch`, and
    /// `None`, SQL `NULL`, in the others.
    ///
    /// # Errors
    ///
    /// [`ExtractError::UnsupportedField`] for a field of a day of the calendar or a zone.
    ///
    /// ```
    /// use sql_values::postgres::{ExtractField, PgNumeric, parse_interval};
    ///
    /// let interval = parse_interval("-14 months -3 days -25:02:03.123456").unwrap();
    /// let epoch = interval.extract(ExtractField::Epoch).unwrap().unwrap();
    /// assert_eq!(epoch.to_string(), "-37090923.123456");
    /// assert_eq!(interval.extract(ExtractField::Quarter).unwrap(), Some(PgNumeric::from(-1)));
    /// ```
    pub fn extract(self, field: ExtractField) -> Result<Option<PgNumeric>, ExtractError> {
        self.part(field)
    }

    /// PostgreSQL's `date_part(field, interval)`.
    ///
    /// # Errors
    ///
    /// [`ExtractError::UnsupportedField`] for a field of a day of the calendar or a zone.
    pub fn date_part(self, field: ExtractField) -> Result<Option<PgFloat8>, ExtractError> {
        self.part(field)
    }

    /// `interval_part_common`.
    fn part<P: Part>(self, field: ExtractField) -> Result<Option<P>, ExtractError> {
        let unsupported = ExtractError::unsupported(field, PgType::Interval);
        if !self.is_finite() {
            return match field {
                ExtractField::Microseconds
                | ExtractField::Milliseconds
                | ExtractField::Second
                | ExtractField::Minute
                | ExtractField::Week
                | ExtractField::Month
                | ExtractField::Quarter => Ok(None),
                ExtractField::Hour
                | ExtractField::Day
                | ExtractField::Year
                | ExtractField::Decade
                | ExtractField::Century
                | ExtractField::Millennium
                | ExtractField::Epoch => Ok(Some(P::infinity(self.months == i32::MIN))),
                _ => Err(unsupported),
            };
        }
        let years = self.months / 12;
        let value = match field {
            ExtractField::Epoch => return Ok(Some(P::interval_epoch(self))),
            ExtractField::Day => self.days,
            ExtractField::Week => self.days / 7,
            ExtractField::Month => self.months % 12,
            ExtractField::Quarter if self.months >= 0 => self.months % 12 / 3 + 1,
            // PostgreSQL negates the months unchecked, as its `-fwrapv` build wraps.
            ExtractField::Quarter => -(self.months.wrapping_neg() % 12 / 3 + 1),
            ExtractField::Year => years,
            ExtractField::Decade => years / 10,
            ExtractField::Century => years / 100,
            ExtractField::Millennium => years / 1_000,
            _ => {
                return clock_part(field, self.microseconds)
                    .map(Some)
                    .ok_or(unsupported);
            }
        };
        Ok(Some(P::integer(value.into())))
    }
}
