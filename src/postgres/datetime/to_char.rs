//! PostgreSQL 18's `to_char` of a date or time, `DCH_to_char` printing the fields of a value in
//! the template `parse_format` reads, with the English names of the `C` locale.

use alloc::string::String;
use core::fmt::Write;

use super::{PgTimestamp, date2j, split_clock, timestamp2tm};

const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

const DAYS: [&str; 7] = [
    "Sunday",
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
];

/// `rm_months_upper`, December first, as `DCH_to_char` indexes it.
const ROMAN_MONTHS: [&str; 12] = [
    "XII", "XI", "X", "IX", "VIII", "VII", "VI", "V", "IV", "III", "II", "I",
];

/// `fmt_tm` and `TmToChar`, the fields of a value `DCH_to_char` prints.
struct Fields {
    year: i32,
    month: i32,
    day: i32,
    hour: i64,
    minute: i64,
    second: i64,
    microsecond: i64,
    weekday: i32,
    year_day: i32,
}

impl Fields {
    /// The fields `timestamp2tm` gives a clock `micros` from 2000-01-01 without a zone, with the
    /// weekday and day of the year `timestamp_to_char` adds.
    fn civil(micros: i64) -> Self {
        let (year, month, day, clock) = timestamp2tm(micros);
        let (hour, minute, second, microsecond) = split_clock(clock);
        let julian = date2j(year, month, day);
        Self {
            year,
            month,
            day,
            hour,
            minute,
            second,
            microsecond,
            weekday: (julian + 1) % 7,
            year_day: julian - date2j(year, 1, 1) + 1,
        }
    }
}

/// How a name keyword cases its name.
#[derive(Debug, Clone, Copy)]
enum Case {
    Upper,
    Capitalized,
    Lower,
}

/// A keyword of `DCH_keywords`, the spellings that print alike sharing one.
#[derive(Debug, Clone, Copy)]
enum Keyword {
    /// The morning and the afternoon marker.
    Meridiem([&'static str; 2]),
    /// The marker of the years from 1 AD and of those before.
    Era([&'static str; 2]),
    Hour12,
    Hour24,
    Minute,
    Second,
    /// The leading digits of the microseconds.
    Fraction(u8),
    SecondsOfDay,
    /// `TZ` and `tz`, whose abbreviation a timestamp without a zone lacks.
    ZoneAbbreviation,
    ZoneHours,
    ZoneMinutes,
    UtcOffset,
    MonthName(Case),
    MonthAbbreviation(Case),
    Month,
    DayName(Case),
    DayAbbreviation(Case),
    DayOfYear,
    IsoDayOfYear,
    DayOfMonth,
    DayOfWeek,
    IsoDayOfWeek,
    WeekOfYear,
    IsoWeek,
    Quarter,
    Century,
    YearWithComma,
    Year {
        digits: usize,
    },
    IsoYear {
        digits: usize,
    },
    RomanMonth {
        upper: bool,
    },
    WeekOfMonth,
    Julian,
    FixedFormat,
}

/// `DCH_keywords` in its order, where the first that starts the template wins.
const KEYWORDS: &[(&str, Keyword)] = {
    use Case::{Capitalized, Lower, Upper};
    use Keyword::*;
    &[
        ("A.D.", Era(["A.D.", "B.C."])),
        ("A.M.", Meridiem(["A.M.", "P.M."])),
        ("AD", Era(["AD", "BC"])),
        ("AM", Meridiem(["AM", "PM"])),
        ("B.C.", Era(["A.D.", "B.C."])),
        ("BC", Era(["AD", "BC"])),
        ("CC", Century),
        ("DAY", DayName(Upper)),
        ("DDD", DayOfYear),
        ("DD", DayOfMonth),
        ("DY", DayAbbreviation(Upper)),
        ("Day", DayName(Capitalized)),
        ("Dy", DayAbbreviation(Capitalized)),
        ("D", DayOfWeek),
        ("FF1", Fraction(1)),
        ("FF2", Fraction(2)),
        ("FF3", Fraction(3)),
        ("FF4", Fraction(4)),
        ("FF5", Fraction(5)),
        ("FF6", Fraction(6)),
        ("FX", FixedFormat),
        ("HH24", Hour24),
        ("HH12", Hour12),
        ("HH", Hour12),
        ("IDDD", IsoDayOfYear),
        ("ID", IsoDayOfWeek),
        ("IW", IsoWeek),
        ("IYYY", IsoYear { digits: 4 }),
        ("IYY", IsoYear { digits: 3 }),
        ("IY", IsoYear { digits: 2 }),
        ("I", IsoYear { digits: 1 }),
        ("J", Julian),
        ("MI", Minute),
        ("MM", Month),
        ("MONTH", MonthName(Upper)),
        ("MON", MonthAbbreviation(Upper)),
        ("MS", Fraction(3)),
        ("Month", MonthName(Capitalized)),
        ("Mon", MonthAbbreviation(Capitalized)),
        ("OF", UtcOffset),
        ("P.M.", Meridiem(["A.M.", "P.M."])),
        ("PM", Meridiem(["AM", "PM"])),
        ("Q", Quarter),
        ("RM", RomanMonth { upper: true }),
        ("SSSSS", SecondsOfDay),
        ("SSSS", SecondsOfDay),
        ("SS", Second),
        ("TZH", ZoneHours),
        ("TZM", ZoneMinutes),
        ("TZ", ZoneAbbreviation),
        ("US", Fraction(6)),
        ("WW", WeekOfYear),
        ("W", WeekOfMonth),
        ("Y,YYY", YearWithComma),
        ("YYYY", Year { digits: 4 }),
        ("YYY", Year { digits: 3 }),
        ("YY", Year { digits: 2 }),
        ("Y", Year { digits: 1 }),
        ("a.d.", Era(["a.d.", "b.c."])),
        ("a.m.", Meridiem(["a.m.", "p.m."])),
        ("ad", Era(["ad", "bc"])),
        ("am", Meridiem(["am", "pm"])),
        ("b.c.", Era(["a.d.", "b.c."])),
        ("bc", Era(["ad", "bc"])),
        ("cc", Century),
        ("day", DayName(Lower)),
        ("ddd", DayOfYear),
        ("dd", DayOfMonth),
        ("dy", DayAbbreviation(Lower)),
        ("d", DayOfWeek),
        ("ff1", Fraction(1)),
        ("ff2", Fraction(2)),
        ("ff3", Fraction(3)),
        ("ff4", Fraction(4)),
        ("ff5", Fraction(5)),
        ("ff6", Fraction(6)),
        ("fx", FixedFormat),
        ("hh24", Hour24),
        ("hh12", Hour12),
        ("hh", Hour12),
        ("iddd", IsoDayOfYear),
        ("id", IsoDayOfWeek),
        ("iw", IsoWeek),
        ("iyyy", IsoYear { digits: 4 }),
        ("iyy", IsoYear { digits: 3 }),
        ("iy", IsoYear { digits: 2 }),
        ("i", IsoYear { digits: 1 }),
        ("j", Julian),
        ("mi", Minute),
        ("mm", Month),
        ("month", MonthName(Lower)),
        ("mon", MonthAbbreviation(Lower)),
        ("ms", Fraction(3)),
        ("of", UtcOffset),
        ("p.m.", Meridiem(["a.m.", "p.m."])),
        ("pm", Meridiem(["am", "pm"])),
        ("q", Quarter),
        ("rm", RomanMonth { upper: false }),
        ("sssss", SecondsOfDay),
        ("ssss", SecondsOfDay),
        ("ss", Second),
        ("tzh", ZoneHours),
        ("tzm", ZoneMinutes),
        ("tz", ZoneAbbreviation),
        ("us", Fraction(6)),
        ("ww", WeekOfYear),
        ("w", WeekOfMonth),
        ("y,yyy", YearWithComma),
        ("yyyy", Year { digits: 4 }),
        ("yyy", Year { digits: 3 }),
        ("yy", Year { digits: 2 }),
        ("y", Year { digits: 1 }),
    ]
};

/// The suffix `TH` or `th` that appends a number's English ordinal ending.
#[derive(Debug, Clone, Copy)]
enum Ordinal {
    Upper,
    Lower,
}

/// A keyword and the modifiers `parse_format` read around it.
#[derive(Debug, Clone, Copy)]
struct Field {
    keyword: Keyword,
    /// `FM`, dropping the padding.
    fill: bool,
    /// `TM`, the locale's names without padding.
    translate: bool,
    ordinal: Option<Ordinal>,
}

/// One node of a template, text copied as it is or a field.
#[derive(Debug, Clone, Copy)]
enum Node<'a> {
    Text(&'a str),
    Field(Field),
}

/// `parse_format`'s reading of a template into nodes, as a cursor over the borrowed text.
struct Nodes<'a> {
    rest: &'a str,
    quoted: bool,
}

impl<'a> Nodes<'a> {
    const fn new(template: &'a str) -> Self {
        Self {
            rest: template,
            quoted: false,
        }
    }

    /// Moves past `prefix` when the text starts with it.
    fn eat(&mut self, prefix: &str) -> bool {
        match self.rest.strip_prefix(prefix) {
            Some(rest) => {
                self.rest = rest;
                true
            }
            None => false,
        }
    }

    /// The next character as text, past a backslash `parse_format` drops before it.
    fn character(&mut self, escaped: bool) -> &'a str {
        if escaped {
            self.rest = &self.rest[1..];
        }
        let width = self.rest.chars().next().map_or(0, char::len_utf8);
        let (character, rest) = self.rest.split_at(width);
        self.rest = rest;
        character
    }
}

impl<'a> Iterator for Nodes<'a> {
    type Item = Node<'a>;

    fn next(&mut self) -> Option<Node<'a>> {
        loop {
            if self.rest.is_empty() {
                return None;
            }
            if self.quoted {
                if self.eat("\"") {
                    self.quoted = false;
                    continue;
                }
                // A backslash quotes the character after it, and a last one is itself.
                let escaped = self.rest.starts_with('\\') && self.rest.len() > 1;
                return Some(Node::Text(self.character(escaped)));
            }
            // A prefix is read, and lost, even where no keyword follows it.
            let fill = self.eat("FM") || self.eat("fm");
            let translate = !fill && (self.eat("TM") || self.eat("tm"));
            if let Some(&(name, keyword)) = KEYWORDS
                .iter()
                .find(|(name, _)| self.rest.starts_with(name))
            {
                self.rest = &self.rest[name.len()..];
                let ordinal = if self.eat("TH") {
                    Some(Ordinal::Upper)
                } else if self.eat("th") {
                    Some(Ordinal::Lower)
                } else {
                    // `SP` is read and prints nothing.
                    self.eat("SP");
                    None
                };
                return Some(Node::Field(Field {
                    keyword,
                    fill,
                    translate,
                    ordinal,
                }));
            }
            if self.rest.is_empty() {
                return None;
            }
            if self.eat("\"") {
                self.quoted = true;
                continue;
            }
            // Outside quotes a backslash is special only before a double quote.
            let escaped = self.rest.starts_with("\\\"");
            return Some(Node::Text(self.character(escaped)));
        }
    }
}

/// `DCH_to_char`, the text of `fields` in `template`.
fn format(template: &str, fields: &Fields) -> String {
    let mut out = String::with_capacity(template.len());
    for node in Nodes::new(template) {
        match node {
            Node::Text(text) => out.push_str(text),
            Node::Field(field) => field.write(fields, &mut out),
        }
    }
    out
}

/// `ADJUST_YEAR`, the year counted from 1 in its era.
const fn era_year(year: i32) -> i32 {
    if year <= 0 { 1 - year } else { year }
}

/// `j2day`, the weekday of a Julian day from Sunday at 0.
const fn j2day(julian: i32) -> i32 {
    julian.wrapping_add(1).rem_euclid(7)
}

/// `isoweek2j(year, 1)`, the Julian day of the Monday that starts the first ISO week of `year`.
const fn iso_week_start(year: i32) -> i32 {
    let day4 = date2j(year, 1, 4);
    day4.wrapping_sub(j2day(day4.wrapping_sub(1)))
}

/// `date2isoweek`, the ISO week of a date.
const fn date2isoweek(year: i32, month: i32, day: i32) -> i32 {
    let julian = date2j(year, month, day);
    let mut start = iso_week_start(year);
    if julian < start {
        start = iso_week_start(year.wrapping_sub(1));
    }
    let mut week = julian.wrapping_sub(start) / 7 + 1;
    if week >= 52 {
        let next = iso_week_start(year.wrapping_add(1));
        if julian >= next {
            week = julian.wrapping_sub(next) / 7 + 1;
        }
    }
    week
}

/// `date2isoyear`, the ISO year of a date, 0 being 1 BC.
const fn date2isoyear(year: i32, month: i32, day: i32) -> i32 {
    let julian = date2j(year, month, day);
    let mut year = year;
    let mut start = iso_week_start(year);
    if julian < start {
        year = year.wrapping_sub(1);
        start = iso_week_start(year);
    }
    if julian.wrapping_sub(start) / 7 + 1 >= 52 && julian >= iso_week_start(year.wrapping_add(1)) {
        year = year.wrapping_add(1);
    }
    year
}

/// The widest of `width` for a value from zero up, one more for a negative one.
const fn signed_width(fill: bool, negative: bool, width: usize) -> usize {
    if fill {
        0
    } else if negative {
        width + 1
    } else {
        width
    }
}

impl Field {
    /// Appends the field's text for `fields` to `out`.
    fn write(self, fields: &Fields, out: &mut String) {
        let start = out.len();
        let fill = self.fill;
        let width = |width| if fill { 0 } else { width };
        match self.keyword {
            Keyword::Meridiem([morning, afternoon]) => {
                out.push_str(if fields.hour % 24 >= 12 {
                    afternoon
                } else {
                    morning
                });
            }
            Keyword::Era([common, before]) => {
                out.push_str(if fields.year <= 0 { before } else { common });
            }
            Keyword::Hour12 => {
                let hour = match fields.hour % 12 {
                    0 => 12,
                    hour => hour,
                };
                number(out, hour, signed_width(fill, fields.hour < 0, 2));
            }
            Keyword::Hour24 => number(out, fields.hour, signed_width(fill, fields.hour < 0, 2)),
            Keyword::Minute => {
                number(out, fields.minute, signed_width(fill, fields.minute < 0, 2));
            }
            Keyword::Second => {
                number(out, fields.second, signed_width(fill, fields.second < 0, 2));
            }
            Keyword::Fraction(digits) => {
                let divisor = 10_i64.pow(u32::from(6 - digits));
                number(out, fields.microsecond / divisor, usize::from(digits));
            }
            Keyword::SecondsOfDay => {
                number(
                    out,
                    fields.hour * 3_600 + fields.minute * 60 + fields.second,
                    0,
                );
            }
            Keyword::ZoneAbbreviation => {}
            // A timestamp without a zone prints the offset of UTC.
            Keyword::ZoneHours => out.push_str("+00"),
            Keyword::ZoneMinutes => out.push_str("00"),
            Keyword::UtcOffset => {
                out.push('+');
                number(out, 0, width(2));
            }
            Keyword::MonthName(case) => {
                self.name(out, MONTHS[month_index(fields.month)], case, 9);
            }
            Keyword::MonthAbbreviation(case) => {
                self.name(out, &MONTHS[month_index(fields.month)][..3], case, 0);
            }
            Keyword::Month => {
                let month = fields.month.into();
                number(out, month, signed_width(fill, month < 0, 2));
            }
            Keyword::DayName(case) => self.name(out, DAYS[day_index(fields.weekday)], case, 9),
            Keyword::DayAbbreviation(case) => {
                self.name(out, &DAYS[day_index(fields.weekday)][..3], case, 0);
            }
            Keyword::DayOfYear => number(out, fields.year_day.into(), width(3)),
            Keyword::IsoDayOfYear => {
                let year = date2isoyear(fields.year, fields.month, fields.day);
                let julian = date2j(fields.year, fields.month, fields.day);
                let day = julian.wrapping_sub(iso_week_start(year)).wrapping_add(1);
                number(out, day.into(), width(3));
            }
            Keyword::DayOfMonth => number(out, fields.day.into(), width(2)),
            Keyword::DayOfWeek => number(out, (fields.weekday + 1).into(), 0),
            Keyword::IsoDayOfWeek => {
                let day = if fields.weekday == 0 {
                    7
                } else {
                    fields.weekday
                };
                number(out, day.into(), 0);
            }
            Keyword::WeekOfYear => {
                number(out, ((fields.year_day - 1) / 7 + 1).into(), width(2));
            }
            Keyword::IsoWeek => {
                let week = date2isoweek(fields.year, fields.month, fields.day);
                number(out, week.into(), width(2));
            }
            Keyword::Quarter => number(out, ((fields.month - 1) / 3 + 1).into(), 0),
            Keyword::Century => {
                let century = if fields.year > 0 {
                    (fields.year - 1) / 100 + 1
                } else {
                    fields.year / 100 - 1
                };
                let width = if (-99..=99).contains(&century) {
                    signed_width(fill, century < 0, 2)
                } else {
                    0
                };
                number(out, century.into(), width);
            }
            Keyword::YearWithComma => {
                let year = era_year(fields.year);
                let thousands = year / 1_000;
                number(out, thousands.into(), 0);
                out.push(',');
                number(out, (year - thousands * 1_000).into(), 3);
            }
            Keyword::Year { digits } => {
                self.year(out, era_year(fields.year), fields.year, digits);
            }
            Keyword::IsoYear { digits } => {
                let year = date2isoyear(fields.year, fields.month, fields.day);
                self.year(out, era_year(year), fields.year, digits);
            }
            Keyword::RomanMonth { upper } => {
                let index = usize::try_from(12 - fields.month).expect("a month of a date");
                let numeral = ROMAN_MONTHS[index];
                let padding = if fill { 0 } else { 4 };
                let numeral_start = out.len();
                if upper {
                    out.push_str(numeral);
                } else {
                    out.extend(numeral.chars().map(|c| c.to_ascii_lowercase()));
                }
                pad(out, numeral_start, padding);
                return;
            }
            Keyword::WeekOfMonth => number(out, ((fields.day - 1) / 7 + 1).into(), 0),
            Keyword::Julian => {
                number(out, date2j(fields.year, fields.month, fields.day).into(), 0);
            }
            Keyword::FixedFormat => return,
        }
        if let Some(ordinal) = self.ordinal
            && self.keyword.is_number()
        {
            append_ordinal(out, start, ordinal);
        }
    }

    /// A day or month name in `case`, padded to `padding` unless `FM` or `TM` is given.
    fn name(self, out: &mut String, name: &str, case: Case, padding: usize) {
        let start = out.len();
        match case {
            Case::Upper => out.extend(name.chars().map(|c| c.to_ascii_uppercase())),
            Case::Capitalized => out.push_str(name),
            Case::Lower => out.extend(name.chars().map(|c| c.to_ascii_lowercase())),
        }
        if !self.fill && !self.translate {
            pad(out, start, padding);
        }
    }

    /// The last `digits` of the era's `year`, widened by one when `calendar_year`'s era year is
    /// negative.
    fn year(self, out: &mut String, year: i32, calendar_year: i32, digits: usize) {
        let (value, width) = match digits {
            4 => (year, 4),
            3 => (year % 1_000, 3),
            2 => (year % 100, 2),
            _ => {
                number(out, (year % 10).into(), 1);
                return;
            }
        };
        let negative = era_year(calendar_year) < 0;
        number(out, value.into(), signed_width(self.fill, negative, width));
    }
}

impl Keyword {
    /// Whether `TH` and `th` append an ordinal ending to the keyword's text.
    const fn is_number(self) -> bool {
        !matches!(
            self,
            Self::Meridiem { .. }
                | Self::Era { .. }
                | Self::ZoneAbbreviation
                | Self::ZoneHours
                | Self::ZoneMinutes
                | Self::UtcOffset
                | Self::MonthName(_)
                | Self::MonthAbbreviation(_)
                | Self::DayName(_)
                | Self::DayAbbreviation(_)
                | Self::RomanMonth { .. }
                | Self::FixedFormat
        )
    }
}

/// The index of a calendar month in [`MONTHS`].
fn month_index(month: i32) -> usize {
    usize::try_from(month - 1).expect("a calendar month from 1 to 12")
}

/// The index of a weekday in [`DAYS`].
fn day_index(weekday: i32) -> usize {
    usize::try_from(weekday).expect("a weekday from 0 to 6")
}

/// C's `%0*d`, `value` zero-padded after its sign to `width` characters.
fn number(out: &mut String, value: i64, width: usize) {
    write!(out, "{value:0width$}").expect("a String takes any text");
}

/// C's `%-*s`, the text from `start` padded with blanks to `width` characters.
fn pad(out: &mut String, start: usize, width: usize) {
    let written = out[start..].chars().count();
    out.extend(core::iter::repeat_n(' ', width.saturating_sub(written)));
}

/// `str_numth`, the English ordinal ending of the number from `start`, `TH` for each number
/// ending in 11 to 13.
fn append_ordinal(out: &mut String, start: usize, ordinal: Ordinal) {
    let digits = &out.as_bytes()[start..];
    let last = digits.last().copied();
    let teen = digits.len() > 1 && digits[digits.len() - 2] == b'1';
    let ending = match (last, teen) {
        (Some(b'1'), false) => "st",
        (Some(b'2'), false) => "nd",
        (Some(b'3'), false) => "rd",
        _ => "th",
    };
    match ordinal {
        Ordinal::Upper => out.extend(ending.chars().map(|c| c.to_ascii_uppercase())),
        Ordinal::Lower => out.push_str(ending),
    }
}

impl PgTimestamp {
    /// PostgreSQL's `to_char(timestamp, text)` under `lc_time` `C`, or `None`, PostgreSQL's SQL
    /// `NULL`, for an empty template or an infinite timestamp.
    ///
    /// The template's keywords print the fields of the timestamp, `FM` before one drops its
    /// padding, `TM` prints a name unpadded, and `TH` or `th` after a number its ordinal ending.
    /// Every other character is copied, and a double-quoted run is copied whole.
    ///
    /// ```
    /// use sql_values::postgres::{PgDate, PgTime, PgTimestamp};
    ///
    /// let date = PgDate::from_ymd(2024, 2, 29).unwrap();
    /// let at = PgTimestamp::new(date, PgTime::from_hms_micro(13, 4, 5, 123_456).unwrap()).unwrap();
    /// assert_eq!(
    ///     at.to_char("FMDay|FMMonth|DDTH|J").as_deref(),
    ///     Some("Thursday|February|29TH|2460370"),
    /// );
    /// assert_eq!(at.to_char("HH24:MI:SS.US").as_deref(), Some("13:04:05.123456"));
    /// assert_eq!(at.to_char("Day|\"YYYY\"").as_deref(), Some("Thursday |YYYY"));
    /// assert_eq!(at.to_char(""), None);
    /// assert_eq!(PgTimestamp::INFINITY.to_char("YYYY"), None);
    /// ```
    #[must_use]
    pub fn to_char(self, template: &str) -> Option<String> {
        if template.is_empty() || !self.is_finite() {
            return None;
        }
        Some(format(template, &Fields::civil(self.pg_micros())))
    }
}
