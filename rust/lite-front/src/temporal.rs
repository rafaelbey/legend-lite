// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! The temporal literals' readers (core/'s `values.PureDateLiteral.parse` and `values.PureTimeLiteral.parse`): the
//! body after the `%`, read into its record, with the exceptions Java raises -- an `IllegalArgumentException` for a
//! malformed shape or (validating) a component out of range; a `java.time.DateTimeException` from the time-zone shift,
//! which builds a `LocalDateTime` of the components before any validation (`PureDateLiteral.shift`), in java.time's
//! own words.

use crate::jcat;
use crate::jnum::parse_int;
use crate::jtypes::JStr;
use crate::records::{
    PureDateLiteral, PureDateLiteralDateWithHour, PureDateLiteralDateWithMinute, PureDateLiteralDateWithSecond,
    PureDateLiteralDateWithSubsecond, PureDateLiteralStrictDate, PureDateLiteralYear, PureDateLiteralYearMonth,
    PureTimeLiteral, PureTimeLiteralTimeWithMinute, PureTimeLiteralTimeWithSecond, PureTimeLiteralTimeWithSubsecond,
};

/// What a temporal reader raises, by Java class.
#[derive(Clone, Debug, PartialEq)]
pub enum TemporalError {
    /// `IllegalArgumentException` (a `NumberFormatException` is wrapped in one by the readers).
    IllegalArgument(JStr),
    /// `java.time.DateTimeException`.
    DateTime(JStr),
    /// `IllegalStateException` (the shift's unreachable arms).
    IllegalState(JStr),
}

type R<T> = Result<T, TemporalError>;

fn iae<T>(m: JStr) -> R<T> {
    Err(TemporalError::IllegalArgument(m))
}

fn is_digit(c: u16) -> bool {
    (u16::from(b'0')..=u16::from(b'9')).contains(&c)
}

fn year(year: i32) -> PureDateLiteral {
    PureDateLiteral::PureDateLiteralYear(Box::new(PureDateLiteralYear { year }))
}

fn year_month(year: i32, month: i32) -> PureDateLiteral {
    PureDateLiteral::PureDateLiteralYearMonth(Box::new(PureDateLiteralYearMonth { year, month }))
}

fn strict_date(year: i32, month: i32, day: i32) -> PureDateLiteral {
    PureDateLiteral::PureDateLiteralStrictDate(Box::new(PureDateLiteralStrictDate { year, month, day }))
}

fn with_hour(year: i32, month: i32, day: i32, hour: i32) -> PureDateLiteral {
    PureDateLiteral::PureDateLiteralDateWithHour(Box::new(PureDateLiteralDateWithHour { year, month, day, hour }))
}

fn with_minute(year: i32, month: i32, day: i32, hour: i32, minute: i32) -> PureDateLiteral {
    PureDateLiteral::PureDateLiteralDateWithMinute(Box::new(PureDateLiteralDateWithMinute {
        year,
        month,
        day,
        hour,
        minute,
    }))
}

fn with_second(year: i32, month: i32, day: i32, hour: i32, minute: i32, second: i32) -> PureDateLiteral {
    PureDateLiteral::PureDateLiteralDateWithSecond(Box::new(PureDateLiteralDateWithSecond {
        year,
        month,
        day,
        hour,
        minute,
        second,
    }))
}

#[allow(clippy::too_many_arguments)]
fn with_subsecond(
    year: i32,
    month: i32,
    day: i32,
    hour: i32,
    minute: i32,
    second: i32,
    subsecond: JStr,
) -> PureDateLiteral {
    PureDateLiteral::PureDateLiteralDateWithSubsecond(Box::new(PureDateLiteralDateWithSubsecond {
        year,
        month,
        day,
        hour,
        minute,
        second,
        subsecond,
    }))
}

/// `PureDateLiteral.parse(source, validateComponents)`: the body without its `%`.
pub fn parse_date(source: &[u16], validate: bool) -> R<PureDateLiteral> {
    if source.is_empty() {
        return iae(JStr::from("empty date literal"));
    }
    let result = DateParser { src: source, pos: 0 }.parse()?;
    if validate {
        validate_components(&result)?;
    }
    Ok(result)
}

/// `PureDateLiteral.Parser`: progressive component consumption, a time zone shifting the components to GMT.
struct DateParser<'a> {
    src: &'a [u16],
    pos: usize,
}

impl DateParser<'_> {
    fn parse(&mut self) -> R<PureDateLiteral> {
        let mut year_sign = 1i32;
        if self.pos < self.src.len() && self.src[self.pos] == u16::from(b'-') {
            year_sign = -1;
            self.pos += 1;
        }
        let year_v = year_sign.wrapping_mul(self.read_digits("year")?);
        if self.at_end() {
            return Ok(year(year_v));
        }
        self.expect(b'-', "expected '-' after year")?;
        let month = self.read_digits("month")?;
        if self.at_end() {
            return Ok(year_month(year_v, month));
        }
        self.expect(b'-', "expected '-' after month")?;
        let day = self.read_digits("day")?;
        if self.at_end() {
            return Ok(strict_date(year_v, month, day));
        }
        self.expect(b'T', "expected 'T' after day")?;
        let hour = self.read_digits("hour")?;
        if self.at_end() {
            return Ok(with_hour(year_v, month, day, hour));
        }
        self.expect(b':', "expected ':' after hour")?;
        let minute = self.read_digits("minute")?;
        if self.at_end() {
            return Ok(with_minute(year_v, month, day, hour, minute));
        }
        if self.src[self.pos] != u16::from(b':') {
            let offset = self.read_time_zone()?;
            return shift(with_minute(year_v, month, day, hour, minute), offset);
        }
        self.pos += 1;
        let second = self.read_digits("second")?;
        if self.at_end() {
            return Ok(with_second(year_v, month, day, hour, minute, second));
        }
        let mut subsecond: Option<JStr> = None;
        if self.src[self.pos] == u16::from(b'.') {
            self.pos += 1;
            let sub_start = self.pos;
            while self.pos < self.src.len() && is_digit(self.src[self.pos]) {
                self.pos += 1;
            }
            if self.pos == sub_start {
                return iae(jcat!("expected digits after '.' in subsecond at position ", sub_start));
            }
            subsecond = Some(JStr::from(&self.src[sub_start..self.pos]));
        }
        let base = |sub: Option<JStr>| match sub {
            None => with_second(year_v, month, day, hour, minute, second),
            Some(s) => with_subsecond(year_v, month, day, hour, minute, second, s),
        };
        if self.at_end() {
            return Ok(base(subsecond));
        }
        let offset = self.read_time_zone()?;
        shift(base(subsecond), offset)
    }

    fn at_end(&self) -> bool {
        self.pos >= self.src.len()
    }

    fn expect(&mut self, c: u8, message: &str) -> R<()> {
        if self.at_end() || self.src[self.pos] != u16::from(c) {
            return iae(jcat!(message, " at position ", self.pos, " in '", self.src, "'"));
        }
        self.pos += 1;
        Ok(())
    }

    fn read_digits(&mut self, component: &str) -> R<i32> {
        let start = self.pos;
        while self.pos < self.src.len() && is_digit(self.src[self.pos]) {
            self.pos += 1;
        }
        if self.pos == start {
            return iae(jcat!(
                "expected digits for ",
                component,
                " at position ",
                start,
                " in '",
                self.src,
                "'"
            ));
        }
        let digits = &self.src[start..self.pos];
        parse_int(digits).or_else(|_| iae(jcat!(component, " value out of int range: '", digits, "'")))
    }

    /// `Z` or `(+|-)HHMM`, its offset in minutes east of GMT.
    fn read_time_zone(&mut self) -> R<i32> {
        if self.at_end() {
            return iae(JStr::from("expected timezone, got end of input"));
        }
        let c = self.src[self.pos];
        if c == u16::from(b'Z') {
            self.pos += 1;
            if !self.at_end() {
                return iae(jcat!(
                    "unexpected trailing text after 'Z': '",
                    &self.src[self.pos..],
                    "'"
                ));
            }
            return Ok(0);
        }
        if c != u16::from(b'+') && c != u16::from(b'-') {
            return iae(jcat!(
                "expected '+' or '-' or 'Z' for timezone at position ",
                self.pos,
                " in '",
                self.src,
                "'"
            ));
        }
        let sign = if c == u16::from(b'+') { 1 } else { -1 };
        self.pos += 1;
        if self.src.len() - self.pos != 4 {
            return iae(jcat!(
                "timezone offset must be exactly 4 digits (HHMM) in '",
                self.src,
                "'"
            ));
        }
        if self.src[self.pos..].iter().any(|&d| !is_digit(d)) {
            return iae(jcat!("non-digit in timezone offset: '", &self.src[self.pos..], "'"));
        }
        let two = |a: u16, b: u16| i32::from(a - u16::from(b'0')) * 10 + i32::from(b - u16::from(b'0'));
        let hh = two(self.src[self.pos], self.src[self.pos + 1]);
        let mm = two(self.src[self.pos + 2], self.src[self.pos + 3]);
        if hh > 23 || mm > 59 {
            return iae(jcat!("invalid timezone offset HHMM values in '", self.src, "'"));
        }
        self.pos += 4;
        Ok(sign * (hh * 60 + mm))
    }
}

/// `PureDateLiteral.shift`: a minute-bearing literal moved to GMT through `LocalDateTime.of(y, mo, d, h, mi)
/// .minusMinutes(offset)`, its second and subsecond kept.
fn shift(lit: PureDateLiteral, offset_minutes: i32) -> R<PureDateLiteral> {
    if offset_minutes == 0 {
        return Ok(lit);
    }
    match lit {
        PureDateLiteral::PureDateLiteralDateWithMinute(d) => {
            let s = LocalDateTime::of(d.year, d.month, d.day, d.hour, d.minute)?.minus_minutes(offset_minutes)?;
            Ok(with_minute(s.year, s.month, s.day, s.hour, s.minute))
        }
        PureDateLiteral::PureDateLiteralDateWithSecond(d) => {
            let s = LocalDateTime::of(d.year, d.month, d.day, d.hour, d.minute)?.minus_minutes(offset_minutes)?;
            Ok(with_second(s.year, s.month, s.day, s.hour, s.minute, d.second))
        }
        PureDateLiteral::PureDateLiteralDateWithSubsecond(d) => {
            let s = LocalDateTime::of(d.year, d.month, d.day, d.hour, d.minute)?.minus_minutes(offset_minutes)?;
            Ok(with_subsecond(
                s.year,
                s.month,
                s.day,
                s.hour,
                s.minute,
                d.second,
                d.subsecond,
            ))
        }
        PureDateLiteral::PureDateLiteralYear(_) => Err(TemporalError::IllegalState(JStr::from(
            "timezone on Year is unreachable (grammar forbids)",
        ))),
        PureDateLiteral::PureDateLiteralYearMonth(_) => Err(TemporalError::IllegalState(JStr::from(
            "timezone on YearMonth is unreachable (grammar forbids)",
        ))),
        PureDateLiteral::PureDateLiteralStrictDate(_) => Err(TemporalError::IllegalState(JStr::from(
            "timezone on StrictDate is unreachable (grammar forbids)",
        ))),
        PureDateLiteral::PureDateLiteralDateWithHour(_) => Err(TemporalError::IllegalState(JStr::from(
            "timezone on DateWithHour is unreachable (grammar forbids)",
        ))),
    }
}

/// The `java.time.LocalDateTime` the shift builds: a date and a minute of the day (second and nano zero).
struct LocalDateTime {
    year: i32,
    month: i32,
    day: i32,
    hour: i32,
    minute: i32,
}

const YEAR_MIN: i64 = -999_999_999;
const YEAR_MAX: i64 = 999_999_999;
const EPOCH_DAY_MIN: i64 = -365_243_219_162;
const EPOCH_DAY_MAX: i64 = 365_241_780_471;
const MONTHS: [&str; 12] = [
    "JANUARY",
    "FEBRUARY",
    "MARCH",
    "APRIL",
    "MAY",
    "JUNE",
    "JULY",
    "AUGUST",
    "SEPTEMBER",
    "OCTOBER",
    "NOVEMBER",
    "DECEMBER",
];

/// `ChronoField.checkValidValue`'s refusal: `Invalid value for <field> (valid values <range>): <value>`.
fn check(field: &str, range: &str, min: i64, max: i64, value: i64) -> R<()> {
    if value < min || value > max {
        return Err(TemporalError::DateTime(jcat!(
            "Invalid value for ",
            field,
            " (valid values ",
            range,
            "): ",
            value
        )));
    }
    Ok(())
}

fn is_leap(year: i64) -> bool {
    (year & 3) == 0 && (year % 100 != 0 || year % 400 == 0)
}

fn month_length(year: i64, month: i32) -> i32 {
    match month {
        2 => {
            if is_leap(year) {
                29
            } else {
                28
            }
        }
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

impl LocalDateTime {
    /// `LocalDateTime.of(year, month, dayOfMonth, hour, minute)`: `LocalDate.of`'s checks, then `LocalTime.of`'s.
    fn of(year: i32, month: i32, day: i32, hour: i32, minute: i32) -> R<LocalDateTime> {
        check("Year", "-999999999 - 999999999", YEAR_MIN, YEAR_MAX, i64::from(year))?;
        check("MonthOfYear", "1 - 12", 1, 12, i64::from(month))?;
        check("DayOfMonth", "1 - 28/31", 1, 31, i64::from(day))?;
        if day > 28 && day > month_length(i64::from(year), month) {
            let m = if day == 29 {
                jcat!("Invalid date 'February 29' as '", year, "' is not a leap year")
            } else {
                jcat!("Invalid date '", MONTHS[(month - 1) as usize], " ", day, "'")
            };
            return Err(TemporalError::DateTime(m));
        }
        check("HourOfDay", "0 - 23", 0, 23, i64::from(hour))?;
        if minute != 0 {
            check("MinuteOfHour", "0 - 59", 0, 59, i64::from(minute))?;
        }
        Ok(LocalDateTime {
            year,
            month,
            day,
            hour,
            minute,
        })
    }

    /// `minusMinutes(m)` for |m| under a day (`plusWithOverflow`): the minute of the day moved, the date by at most
    /// one day (`LocalDate.plusDays`, its year and epoch-day checks).
    fn minus_minutes(self, minutes: i32) -> R<LocalDateTime> {
        let total = i64::from(self.hour * 60 + self.minute) - i64::from(minutes);
        let days = total.div_euclid(1440);
        let of_day = total.rem_euclid(1440) as i32;
        let (year, month, day) = self.plus_days(days)?;
        Ok(LocalDateTime {
            year,
            month,
            day,
            hour: of_day / 60,
            minute: of_day % 60,
        })
    }

    fn plus_days(&self, days: i64) -> R<(i32, i32, i32)> {
        if days == 0 {
            return Ok((self.year, self.month, self.day));
        }
        let dom = i64::from(self.day) + days;
        if dom > 0 {
            if dom <= 28 {
                return Ok((self.year, self.month, dom as i32));
            }
            if dom <= 59 {
                let len = i64::from(month_length(i64::from(self.year), self.month));
                if dom <= len {
                    return Ok((self.year, self.month, dom as i32));
                } else if self.month < 12 {
                    return Ok((self.year, self.month + 1, (dom - len) as i32));
                }
                check(
                    "Year",
                    "-999999999 - 999999999",
                    YEAR_MIN,
                    YEAR_MAX,
                    i64::from(self.year) + 1,
                )?;
                return Ok((self.year + 1, 1, (dom - len) as i32));
            }
        }
        of_epoch_day(to_epoch_day(self.year, self.month, self.day) + days)
    }
}

/// `LocalDate.toEpochDay`.
fn to_epoch_day(year: i32, month: i32, day: i32) -> i64 {
    let y = i64::from(year);
    let m = i64::from(month);
    let mut total = 365 * y;
    if y >= 0 {
        total += (y + 3) / 4 - (y + 99) / 100 + (y + 399) / 400;
    } else {
        total -= y / -4 - y / -100 + y / -400;
    }
    total += (367 * m - 362) / 12;
    total += i64::from(day) - 1;
    if m > 2 {
        total -= 1;
        if !is_leap(y) {
            total -= 1;
        }
    }
    total - 719_528
}

/// `LocalDate.ofEpochDay`.
fn of_epoch_day(epoch_day: i64) -> R<(i32, i32, i32)> {
    check(
        "EpochDay",
        "-365243219162 - 365241780471",
        EPOCH_DAY_MIN,
        EPOCH_DAY_MAX,
        epoch_day,
    )?;
    let mut zero_day = epoch_day + 719_528 - 60;
    let mut adjust = 0;
    if zero_day < 0 {
        let adjust_cycles = (zero_day + 1) / 146_097 - 1;
        adjust = adjust_cycles * 400;
        zero_day += -adjust_cycles * 146_097;
    }
    let mut year_est = (400 * zero_day + 591) / 146_097;
    let mut doy_est = zero_day - (365 * year_est + year_est / 4 - year_est / 100 + year_est / 400);
    if doy_est < 0 {
        year_est -= 1;
        doy_est = zero_day - (365 * year_est + year_est / 4 - year_est / 100 + year_est / 400);
    }
    year_est += adjust;
    let march_doy0 = doy_est as i32;
    let march_month0 = (march_doy0 * 5 + 2) / 153;
    let month = (march_month0 + 2) % 12 + 1;
    let dom = march_doy0 - (march_month0 * 306 + 5) / 10 + 1;
    year_est += i64::from(march_month0 / 10);
    check("Year", "-999999999 - 999999999", YEAR_MIN, YEAR_MAX, year_est)?;
    Ok((year_est as i32, month, dom))
}

/// `PureDateLiteral.validateComponents`.
fn validate_components(d: &PureDateLiteral) -> R<()> {
    let (y, mo, da, h, mi, s, sub) = match d {
        PureDateLiteral::PureDateLiteralYear(x) => (x.year, None, None, None, None, None, None),
        PureDateLiteral::PureDateLiteralYearMonth(x) => (x.year, Some(x.month), None, None, None, None, None),
        PureDateLiteral::PureDateLiteralStrictDate(x) => (x.year, Some(x.month), Some(x.day), None, None, None, None),
        PureDateLiteral::PureDateLiteralDateWithHour(x) => {
            (x.year, Some(x.month), Some(x.day), Some(x.hour), None, None, None)
        }
        PureDateLiteral::PureDateLiteralDateWithMinute(x) => (
            x.year,
            Some(x.month),
            Some(x.day),
            Some(x.hour),
            Some(x.minute),
            None,
            None,
        ),
        PureDateLiteral::PureDateLiteralDateWithSecond(x) => (
            x.year,
            Some(x.month),
            Some(x.day),
            Some(x.hour),
            Some(x.minute),
            Some(x.second),
            None,
        ),
        PureDateLiteral::PureDateLiteralDateWithSubsecond(x) => (
            x.year,
            Some(x.month),
            Some(x.day),
            Some(x.hour),
            Some(x.minute),
            Some(x.second),
            Some(&x.subsecond),
        ),
    };
    if i64::from(y) < YEAR_MIN || i64::from(y) > YEAR_MAX {
        return iae(jcat!("invalid year: ", y));
    }
    if let Some(mo) = mo {
        if !(1..=12).contains(&mo) {
            return iae(jcat!("Invalid month: ", mo));
        }
        if let Some(da) = da {
            let dim = match mo {
                1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
                4 | 6 | 9 | 11 => 30,
                _ => {
                    if (y % 4 == 0) && ((y % 100 != 0) || (y % 400 == 0)) {
                        29
                    } else {
                        28
                    }
                }
            };
            if da < 1 || da > dim {
                return iae(jcat!("invalid day for ", y, "-", mo, ": ", da));
            }
        }
    }
    if let Some(h) = h
        && !(0..=23).contains(&h)
    {
        return iae(jcat!("invalid hour: ", h));
    }
    if let Some(mi) = mi
        && !(0..=59).contains(&mi)
    {
        return iae(jcat!("invalid minute: ", mi));
    }
    if let Some(s) = s
        && !(0..=59).contains(&s)
    {
        return iae(jcat!("invalid second: ", s));
    }
    if let Some(sub) = sub {
        if sub.is_empty() {
            return iae(JStr::from("subsecond cannot be null or empty"));
        }
        if sub.units().iter().any(|&c| !is_digit(c)) {
            return iae(jcat!("invalid subsecond (non-digit): '", sub, "'"));
        }
    }
    Ok(())
}

/// `PureTimeLiteral.parse(source)`: the body without its `%`, its records validating as their constructors do.
pub fn parse_time(source: &[u16]) -> R<PureTimeLiteral> {
    if source.is_empty() {
        return iae(JStr::from("empty time literal"));
    }
    let after_digits = |from: usize| {
        let mut p = from;
        while p < source.len() && is_digit(source[p]) {
            p += 1;
        }
        p
    };
    let read_digits = |from: usize, component: &str| -> R<i32> {
        let end = after_digits(from);
        if end == from {
            return iae(jcat!(
                "expected digits for ",
                component,
                " at position ",
                from,
                " in '",
                source,
                "'"
            ));
        }
        parse_int(&source[from..end])
            .or_else(|_| iae(jcat!(component, " value out of int range: '", &source[from..end], "'")))
    };
    let mut pos = 0;
    let hour = read_digits(pos, "hour")?;
    pos = after_digits(pos);
    if pos >= source.len() || source[pos] != u16::from(b':') {
        return iae(jcat!("expected ':' after hour in time literal '", source, "'"));
    }
    pos += 1;
    let minute = read_digits(pos, "minute")?;
    pos = after_digits(pos);
    if pos == source.len() {
        validate_time(hour, minute, None, None)?;
        return Ok(PureTimeLiteral::PureTimeLiteralTimeWithMinute(Box::new(
            PureTimeLiteralTimeWithMinute { hour, minute },
        )));
    }
    if source[pos] != u16::from(b':') {
        return iae(jcat!(
            "unexpected character after minute at position ",
            pos,
            " in '",
            source,
            "'"
        ));
    }
    pos += 1;
    let second = read_digits(pos, "second")?;
    pos = after_digits(pos);
    if pos == source.len() {
        validate_time(hour, minute, Some(second), None)?;
        return Ok(PureTimeLiteral::PureTimeLiteralTimeWithSecond(Box::new(
            PureTimeLiteralTimeWithSecond { hour, minute, second },
        )));
    }
    if source[pos] != u16::from(b'.') {
        return iae(jcat!(
            "unexpected character after second at position ",
            pos,
            " in '",
            source,
            "'"
        ));
    }
    pos += 1;
    let sub_start = pos;
    while pos < source.len() && is_digit(source[pos]) {
        pos += 1;
    }
    if pos == sub_start {
        return iae(jcat!("expected digits after '.' in subsecond in '", source, "'"));
    }
    if pos != source.len() {
        return iae(jcat!("trailing characters after subsecond: '", &source[pos..], "'"));
    }
    let subsecond = JStr::from(&source[sub_start..pos]);
    validate_time(hour, minute, Some(second), Some(&subsecond))?;
    Ok(PureTimeLiteral::PureTimeLiteralTimeWithSubsecond(Box::new(
        PureTimeLiteralTimeWithSubsecond {
            hour,
            minute,
            second,
            subsecond,
        },
    )))
}

/// The time records' compact constructors: hour, minute, second, subsecond, in that order.
fn validate_time(hour: i32, minute: i32, second: Option<i32>, subsecond: Option<&JStr>) -> R<()> {
    if !(0..=23).contains(&hour) {
        return iae(jcat!("invalid hour: ", hour));
    }
    if !(0..=59).contains(&minute) {
        return iae(jcat!("invalid minute: ", minute));
    }
    if let Some(s) = second
        && !(0..=59).contains(&s)
    {
        return iae(jcat!("invalid second: ", s));
    }
    if let Some(sub) = subsecond {
        if sub.is_empty() {
            return iae(JStr::from("subsecond cannot be null or empty"));
        }
        if sub.units().iter().any(|&c| !is_digit(c)) {
            return iae(jcat!("invalid subsecond (non-digit): '", sub, "'"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ustr::u;

    #[test]
    fn dates_read_as_java_reads_them() {
        assert_eq!(
            parse_date(&u("2024-01-15T10:30+0500"), true),
            Ok(with_minute(2024, 1, 15, 5, 30))
        );
        assert_eq!(
            parse_date(&u("2024-01-01T01:30+0200"), true),
            Ok(with_minute(2023, 12, 31, 23, 30))
        );
        assert_eq!(parse_date(&u("2024-02-30"), false), Ok(strict_date(2024, 2, 30)));
        assert!(matches!(
            parse_date(&u("2024-02-30"), true),
            Err(TemporalError::IllegalArgument(_))
        ));
        assert_eq!(
            parse_date(&u("2023-02-29T10:30+0100"), false),
            Err(TemporalError::DateTime(JStr::from(
                "Invalid date 'February 29' as '2023' is not a leap year"
            )))
        );
        assert_eq!(
            parse_date(&u("2023-13-01T10:30+0100"), false),
            Err(TemporalError::DateTime(JStr::from(
                "Invalid value for MonthOfYear (valid values 1 - 12): 13"
            )))
        );
        assert_eq!(to_epoch_day(1970, 1, 1), 0);
        assert_eq!(of_epoch_day(-1), Ok((1969, 12, 31)));
        assert!(matches!(
            parse_time(&u("24:00")),
            Err(TemporalError::IllegalArgument(_))
        ));
    }
}
