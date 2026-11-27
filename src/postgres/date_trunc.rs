//! PostgreSQL 18's `date_trunc`, `timestamp_trunc`, `timestamptz_trunc_internal` and
//! `interval_trunc`, with the unit `DecodeUnits` reads from text.

use super::datetime::{
    POSTGRES_EPOCH_JDATE, USECS_PER_DAY, USECS_PER_HOUR, USECS_PER_MINUTE, USECS_PER_SEC, date2j,
    is_valid_julian, j2date, local_offset, timestamp2tm, tm2timestamp,
    tokens::{
        DTK_CENTURY, DTK_DAY, DTK_DECADE, DTK_HOUR, DTK_MICROSEC, DTK_MILLENNIUM, DTK_MILLISEC,
        DTK_MINUTE, DTK_MONTH, DTK_QUARTER, DTK_SECOND, DTK_WEEK, DTK_YEAR, TOKEN_MAX_LEN, UNITS,
    },
    utc_offset,
};
use super::interval::decode_units;
use super::{DateTimeError, PgInterval, PgTimeZone, PgTimestamp, PgTimestampTz};

/// A unit `date_trunc` truncates a `timestamp`, `timestamptz` or `interval` to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DateTruncField {
    /// `microseconds`, which leaves the value as it is.
    Microseconds,
    /// `milliseconds`, dropping the microseconds of the millisecond.
    Milliseconds,
    /// `second`.
    Second,
    /// `minute`.
    Minute,
    /// `hour`.
    Hour,
    /// `day`.
    Day,
    /// `week`, the Monday that starts the ISO week, which an interval refuses.
    Week,
    /// `month`.
    Month,
    /// `quarter`, the first month of the quarter.
    Quarter,
    /// `year`.
    Year,
    /// `decade`, as `timestamp_trunc` counts decades, 2020 for 2024 and 11 BC for 2 BC.
    Decade,
    /// `century`, its first year, 2001 for 2024 and 100 BC for 1 BC.
    Century,
    /// `millennium`, its first year, 2001 for 2024 and 1000 BC for 1 BC.
    Millennium,
}

/// Why PostgreSQL refuses the unit of a `date_trunc`, one variant per message it raises, which
/// quotes the lowercased unit and names the input type, as in `unit "epoch" not recognized for
/// type timestamp without time zone`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, thiserror::Error)]
pub enum DateTruncError {
    /// Text that names no unit, `epoch` and `dow` among them.
    #[error("unit not recognized")]
    UnitNotRecognized,
    /// A unit `date_trunc` does not truncate the type to, `timezone` for every type and
    /// [`DateTruncField::Week`] for an interval.
    #[error("unit not supported")]
    UnitNotSupported,
}

impl DateTruncField {
    /// The unit PostgreSQL's `date_trunc` reads from `unit`, ASCII letters in either case, with
    /// the first ten characters deciding as `DecodeUnits` compares them.
    ///
    /// # Errors
    ///
    /// [`DateTruncError::UnitNotRecognized`] for text `DecodeUnits` does not know, and
    /// [`DateTruncError::UnitNotSupported`] for the `timezone` units it knows.
    ///
    /// ```
    /// use sql_values::postgres::{DateTruncError, DateTruncField};
    ///
    /// assert_eq!(DateTruncField::parse("Hours"), Ok(DateTruncField::Hour));
    /// assert_eq!(DateTruncField::parse("microseconds"), Ok(DateTruncField::Microseconds));
    /// assert_eq!(DateTruncField::parse("epoch"), Err(DateTruncError::UnitNotRecognized));
    /// assert_eq!(DateTruncField::parse("timezone"), Err(DateTruncError::UnitNotSupported));
    /// ```
    pub fn parse(unit: &str) -> Result<Self, DateTruncError> {
        let mut lowered = [0; TOKEN_MAX_LEN];
        let prefix = &unit.as_bytes()[..unit.len().min(TOKEN_MAX_LEN)];
        for (low, byte) in lowered.iter_mut().zip(prefix) {
            *low = byte.to_ascii_lowercase();
        }
        match decode_units(&lowered[..prefix.len()]) {
            (UNITS, DTK_MICROSEC) => Ok(Self::Microseconds),
            (UNITS, DTK_MILLISEC) => Ok(Self::Milliseconds),
            (UNITS, DTK_SECOND) => Ok(Self::Second),
            (UNITS, DTK_MINUTE) => Ok(Self::Minute),
            (UNITS, DTK_HOUR) => Ok(Self::Hour),
            (UNITS, DTK_DAY) => Ok(Self::Day),
            (UNITS, DTK_WEEK) => Ok(Self::Week),
            (UNITS, DTK_MONTH) => Ok(Self::Month),
            (UNITS, DTK_QUARTER) => Ok(Self::Quarter),
            (UNITS, DTK_YEAR) => Ok(Self::Year),
            (UNITS, DTK_DECADE) => Ok(Self::Decade),
            (UNITS, DTK_CENTURY) => Ok(Self::Century),
            (UNITS, DTK_MILLENNIUM) => Ok(Self::Millennium),
            (UNITS, _) => Err(DateTruncError::UnitNotSupported),
            _ => Err(DateTruncError::UnitNotRecognized),
        }
    }

    /// Whether the truncated clock is a local midnight, whose offset `timestamptz_trunc`
    /// determines again.
    const fn reaches_midnight(self) -> bool {
        !matches!(
            self,
            Self::Microseconds | Self::Milliseconds | Self::Second | Self::Minute | Self::Hour
        )
    }

    /// `timestamp2tm`'s year, month, day and clock of the local clock `local`, microseconds from
    /// 2000-01-01, truncated as `timestamp_trunc` truncates them.
    fn truncate_local(self, local: i64) -> (i32, i32, i32, i64) {
        let (mut year, mut month, mut day, clock) = timestamp2tm(local);
        let clock_step = match self {
            Self::Microseconds => 1,
            Self::Milliseconds => 1_000,
            Self::Second => USECS_PER_SEC,
            Self::Minute => USECS_PER_MINUTE,
            Self::Hour => USECS_PER_HOUR,
            _ => USECS_PER_DAY,
        };
        let clock = clock - clock % clock_step;
        match self {
            Self::Week => {
                // The Julian day of a Monday is a multiple of seven.
                let julian_day = date2j(year, month, day);
                (year, month, day) = j2date(julian_day - julian_day.rem_euclid(7));
            }
            Self::Month => day = 1,
            Self::Quarter => (month, day) = (month - (month - 1) % 3, 1),
            Self::Year => (month, day) = (1, 1),
            Self::Decade => {
                year = if year > 0 {
                    year / 10 * 10
                } else {
                    -((8 - (year - 1)) / 10) * 10
                };
                (month, day) = (1, 1);
            }
            Self::Century => (year, month, day) = (first_year_of(year, 100), 1, 1),
            Self::Millennium => (year, month, day) = (first_year_of(year, 1_000), 1, 1),
            _ => {}
        }
        (year, month, day, clock)
    }
}

/// The first year of the `span`-year period holding `year`, where year `Y` BC is year `1 - Y`,
/// periods starting at year 1 AD and ending at year 1 BC, as `timestamp_trunc` counts centuries
/// and millennia.
const fn first_year_of(year: i32, span: i32) -> i32 {
    if year > 0 {
        (year + span - 1) / span * span - span + 1
    } else {
        -((span - 1 - (year - 1)) / span) * span + 1
    }
}

impl PgTimestamp {
    /// PostgreSQL's `date_trunc(field, timestamp)`, the timestamp with every field finer than
    /// `field` reset, an infinity left as it is.
    ///
    /// # Errors
    ///
    /// [`DateTimeError::TimestampOutOfRange`] where the truncated timestamp leaves PostgreSQL's
    /// range, as a year of 4714 BC does, or as every unit of the stored 294277-01-01 that
    /// `timestamp(0)` rounds the last second up to does.
    ///
    /// ```
    /// use sql_values::postgres::{DateTruncField, InputContext, PgTimestamp};
    ///
    /// let at = |text| PgTimestamp::parse(text, InputContext::new()).unwrap();
    /// let late = at("2024-02-29 23:59:59.999500");
    /// assert_eq!(late.date_trunc(DateTruncField::Milliseconds), Ok(at("2024-02-29 23:59:59.999")));
    /// assert_eq!(late.date_trunc(DateTruncField::Week), Ok(at("2024-02-26 00:00:00")));
    /// let first_bc = at("0001-12-31 23:59:59.999999 BC");
    /// assert_eq!(first_bc.date_trunc(DateTruncField::Century), Ok(at("0100-01-01 BC")));
    /// ```
    pub fn date_trunc(self, field: DateTruncField) -> Result<Self, DateTimeError> {
        if !self.is_finite() {
            return Ok(self);
        }
        let (year, month, day, clock) = field.truncate_local(self.pg_micros());
        tm2timestamp(year, month, day, clock, 0)
            .map(Self::from_stored)
            .ok_or(DateTimeError::TimestampOutOfRange)
    }
}

impl PgTimestampTz {
    /// PostgreSQL's `date_trunc(field, timestamptz, zone)`, the instant whose local clock in
    /// `zone` has every field finer than `field` reset, an infinity left as it is.
    ///
    /// Truncating to an hour or finer keeps the offset the instant had, so each hour a fall-back
    /// repeats truncates to its own start. Truncating to a day or coarser places the truncated
    /// clock in `zone` as [`PgTimestamp::to_timestamptz`] does.
    ///
    /// The two-argument `date_trunc(field, timestamptz)` passes the session's `TimeZone` as
    /// `zone`. PostgreSQL has no `date_trunc` of a `date`, and reads one as the `timestamptz`
    /// of its midnight in the session's `TimeZone`,
    /// [`PgDate::to_timestamp`](super::PgDate::to_timestamp) then
    /// [`PgTimestamp::to_timestamptz`], before truncating it in `zone`.
    ///
    /// # Errors
    ///
    /// [`DateTimeError::TimestampOutOfRange`] where the local clock or the truncated instant
    /// falls outside PostgreSQL's range, and [`DateTimeError::OutsideJiffRange`] where an IANA
    /// zone is asked about a clock outside jiff's years.
    ///
    /// ```
    /// use sql_values::postgres::{DateTruncField, InputContext, PgDate, PgTimeZone, PgTimestampTz};
    ///
    /// #[cfg(feature = "tzdb")]
    /// {
    ///     let at = |text| PgTimestampTz::parse(text, InputContext::new()).unwrap();
    ///     let new_york = PgTimeZone::parse("America/New_York").unwrap();
    ///     let (first, second) = (at("2024-11-03 05:30:00+00"), at("2024-11-03 06:30:00+00"));
    ///     let hour = |at: PgTimestampTz| at.date_trunc(DateTruncField::Hour, &new_york);
    ///     assert_eq!(hour(first), Ok(at("2024-11-03 05:00:00+00")));
    ///     assert_eq!(hour(second), Ok(at("2024-11-03 06:00:00+00")));
    ///     let day = second.date_trunc(DateTruncField::Day, &new_york);
    ///     assert_eq!(day, Ok(at("2024-11-03 04:00:00+00")));
    ///
    ///     // date_trunc('day', date '2024-02-29', 'Australia/Sydney') in a New York session.
    ///     let sydney = PgTimeZone::parse("Australia/Sydney").unwrap();
    ///     let leap = PgDate::from_ymd(2024, 2, 29).unwrap();
    ///     let midnight = leap.to_timestamp().unwrap().to_timestamptz(&new_york).unwrap();
    ///     let day = midnight.date_trunc(DateTruncField::Day, &sydney);
    ///     assert_eq!(day, Ok(at("2024-02-28 08:00:00-05")));
    /// }
    /// ```
    pub fn date_trunc(
        self,
        field: DateTruncField,
        zone: &PgTimeZone,
    ) -> Result<Self, DateTimeError> {
        if !self.is_finite() {
            return Ok(self);
        }
        let offset = utc_offset(zone, self.pg_micros())?;
        let local = self.pg_micros() + i64::from(offset) * USECS_PER_SEC;
        let (year, month, day, clock) = field.truncate_local(local);
        let offset = if field.reaches_midnight() {
            if !is_valid_julian(year, month) {
                return Err(DateTimeError::TimestampOutOfRange);
            }
            let days = i64::from(date2j(year, month, day) - POSTGRES_EPOCH_JDATE);
            local_offset(zone, days * USECS_PER_DAY + clock)?
        } else {
            offset
        };
        tm2timestamp(year, month, day, clock, offset)
            .map(Self::from_stored)
            .ok_or(DateTimeError::TimestampOutOfRange)
    }
}

impl PgInterval {
    /// PostgreSQL's `date_trunc(field, interval)`, every field finer than `field` reset, each
    /// toward zero, where a year is twelve months and a month never folds into days. An infinity
    /// is left as it is.
    ///
    /// # Errors
    ///
    /// [`DateTruncError::UnitNotSupported`] for [`DateTruncField::Week`], since a month holds no
    /// whole number of weeks, infinities included.
    ///
    /// ```
    /// use sql_values::postgres::{DateTruncError, DateTruncField, parse_interval};
    ///
    /// let span = parse_interval("-14 months -3 days -25:02:03.123500").unwrap();
    /// let truncated = span.date_trunc(DateTruncField::Milliseconds).unwrap();
    /// assert_eq!(truncated.to_string(), "-1 years -2 mons -3 days -25:02:03.123");
    /// let quarter = span.date_trunc(DateTruncField::Quarter).unwrap();
    /// assert_eq!(quarter.to_string(), "-1 years");
    /// assert_eq!(span.date_trunc(DateTruncField::Week), Err(DateTruncError::UnitNotSupported));
    /// ```
    pub fn date_trunc(self, field: DateTruncField) -> Result<Self, DateTruncError> {
        if !self.is_finite() {
            return match field {
                DateTruncField::Week => Err(DateTruncError::UnitNotSupported),
                _ => Ok(self),
            };
        }
        let Self {
            months,
            days,
            microseconds,
        } = self;
        let clock = |step: i64| Self {
            microseconds: microseconds - microseconds % step,
            ..self
        };
        let calendar = |step: i32| Self {
            months: months - months % step,
            days: 0,
            microseconds: 0,
        };
        let years = |step: i32| {
            let years = months / 12;
            Self {
                months: (years - years % step) * 12,
                days: 0,
                microseconds: 0,
            }
        };
        Ok(match field {
            DateTruncField::Microseconds => self,
            DateTruncField::Milliseconds => clock(1_000),
            DateTruncField::Second => clock(USECS_PER_SEC),
            DateTruncField::Minute => clock(USECS_PER_MINUTE),
            DateTruncField::Hour => clock(USECS_PER_HOUR),
            DateTruncField::Day => Self {
                months,
                days,
                microseconds: 0,
            },
            DateTruncField::Week => return Err(DateTruncError::UnitNotSupported),
            DateTruncField::Month => calendar(1),
            DateTruncField::Quarter => calendar(3),
            DateTruncField::Year => calendar(12),
            DateTruncField::Decade => years(10),
            DateTruncField::Century => years(100),
            DateTruncField::Millennium => years(1_000),
        })
    }
}
