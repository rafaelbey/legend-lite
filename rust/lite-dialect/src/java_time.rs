// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! The part of Java's `java.time` the engine text's date-time constant uses (`EngineStyleH2.timestampLit`, legend-engine's
//! adjustDate): a local date-time read strictly (`LocalDateTime.parse`), taken as UTC and respelled in a connection's
//! zone (`ZoneId.of(id, ZoneId.SHORT_IDS)`, `atZone(UTC).withZoneSameInstant(zone).toLocalDateTime()`), and printed as
//! `LocalDateTime.toString` prints it.
//!
//! A zone's rules are data Java reads from its time-zone database (JDK 25: tzdb 2026b). This port carries every zone
//! whose offset never changes (an offset ID, `UTC`/`GMT`/`UT` and their offsets, the 49 fixed regions) and the two
//! region families the engine's tests name -- America/Panama's (`EST` on JDK 25) and America/Phoenix's (`MST`,
//! `US/Arizona`) -- with their whole history; for an ID Java does not know it answers as Java does (the shift is not
//! made); a region Java knows whose rules are not carried here is refused by name ([`shift_to_zone`]).

mod zone_ids;

use crate::refusal::Refusal;
use zone_ids::ZONE_IDS;

/// Java's `LocalDateTime` to the second (the engine text reads 19 characters: never a fraction).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LocalDateTime {
    pub year: i64,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
}

/// `Year.MAX_VALUE`: a year past it is refused by `LocalDateTime`.
const MAX_YEAR: i64 = 999_999_999;

impl LocalDateTime {
    /// `LocalDateTime.parse(text)`: `ISO_LOCAL_DATE_TIME`, strict and case-insensitive -- a year of four digits (more
    /// with a `+`, four or more with a `-`), `-MM-dd`, `T`, `HH:mm`, then `:ss` and a fraction optionally, the whole
    /// text. `None` where Java throws its `DateTimeParseException`. A fraction is read and dropped: the engine text
    /// reads 19 characters, which hold none.
    pub fn parse(text: &str) -> Option<LocalDateTime> {
        let b = text.as_bytes();
        let mut at = 0;
        let sign = match b.first() {
            Some(b'+') => {
                at += 1;
                1
            }
            Some(b'-') => {
                at += 1;
                -1
            }
            _ => 0,
        };
        let start = at;
        let mut year: i64 = 0;
        while at < b.len() && at - start < 10 && b[at].is_ascii_digit() {
            year = year * 10 + i64::from(b[at] - b'0');
            at += 1;
        }
        let digits = at - start;
        // SignStyle.EXCEEDS_PAD, strict: at least four digits, '+' only past four, none past four without one, no -0
        if digits < 4 || (sign == 1 && digits <= 4) || (sign == 0 && digits > 4) || (sign == -1 && year == 0) {
            return None;
        }
        if sign == -1 {
            year = -year;
        }
        let two = |at: &mut usize| -> Option<u32> {
            let d = b.get(*at..*at + 2)?;
            if !d.iter().all(u8::is_ascii_digit) {
                return None;
            }
            *at += 2;
            Some(u32::from(d[0] - b'0') * 10 + u32::from(d[1] - b'0'))
        };
        let literal =
            |at: &mut usize, c: u8| -> Option<()> { (b.get(*at)?.eq_ignore_ascii_case(&c)).then(|| *at += 1) };
        literal(&mut at, b'-')?;
        let month = two(&mut at)?;
        literal(&mut at, b'-')?;
        let day = two(&mut at)?;
        literal(&mut at, b'T')?;
        let hour = two(&mut at)?;
        literal(&mut at, b':')?;
        let minute = two(&mut at)?;
        let mut second = 0;
        // the optional seconds, and their optional fraction
        let mut opt = at;
        if literal(&mut opt, b':').is_some()
            && let Some(s) = two(&mut opt)
        {
            second = s;
            at = opt;
            if b.get(at) == Some(&b'.') {
                at += 1;
                let from = at;
                while at < b.len() && at - from < 9 && b[at].is_ascii_digit() {
                    at += 1;
                }
            }
        }
        if at != b.len() {
            return None;
        }
        // ResolverStyle.STRICT
        if !(1..=12).contains(&month)
            || day < 1
            || day > month_length(year, month)
            || hour > 23
            || minute > 59
            || second > 59
            || year.abs() > MAX_YEAR
        {
            return None;
        }
        Some(LocalDateTime { year, month, day, hour, minute, second })
    }

    /// The instant this local date-time is at offset 0, in seconds from the epoch.
    pub fn epoch_second(&self) -> i64 {
        days_from_civil(self.year, self.month, self.day) * 86_400
            + i64::from(self.hour) * 3_600
            + i64::from(self.minute) * 60
            + i64::from(self.second)
    }

    /// The local date-time at offset 0 of an instant (`LocalDateTime.ofEpochSecond(s, 0, UTC)`); `None` past
    /// `LocalDateTime`'s range, where Java throws its `DateTimeException`.
    pub fn of_epoch_second(s: i64) -> Option<LocalDateTime> {
        let days = s.div_euclid(86_400);
        let secs = s.rem_euclid(86_400);
        let (year, month, day) = civil_from_days(days);
        if year.abs() > MAX_YEAR {
            return None;
        }
        Some(LocalDateTime {
            year,
            month,
            day,
            hour: (secs / 3_600) as u32,
            minute: (secs % 3_600 / 60) as u32,
            second: (secs % 60) as u32,
        })
    }

    /// `LocalDateTime.toString`: `LocalDate.toString` (a year below 1000 zero-padded to four digits, a year past 9999
    /// with `+`), `T`, `LocalTime.toString` (`HH:mm`, then `:ss` only when the second is not 0).
    pub fn to_java_string(&self) -> String {
        let mut out = String::new();
        if self.year.abs() < 1000 {
            if self.year < 0 {
                out.push_str(&format!("-{:04}", -self.year));
            } else {
                out.push_str(&format!("{:04}", self.year));
            }
        } else {
            if self.year > 9999 {
                out.push('+');
            }
            out.push_str(&self.year.to_string());
        }
        out.push_str(&format!("-{:02}-{:02}T{:02}:{:02}", self.month, self.day, self.hour, self.minute));
        if self.second > 0 {
            out.push_str(&format!(":{:02}", self.second));
        }
        out
    }
}

/// ISO's proleptic leap year (`IsoChronology.isLeapYear`).
fn leap(year: i64) -> bool {
    year & 3 == 0 && (year % 100 != 0 || year % 400 == 0)
}

fn month_length(year: i64, month: u32) -> u32 {
    match month {
        2 => {
            if leap(year) {
                29
            } else {
                28
            }
        }
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// Days from 1970-01-01 to the proleptic Gregorian date (H. Hinnant's `days_from_civil`).
fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = i64::from((m + 9) % 12);
    let doy = (153 * mp + 2) / 5 + i64::from(d) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The proleptic Gregorian date `z` days from 1970-01-01 (`civil_from_days`).
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// A zone's rules: its offset before its first transition, then each transition's instant and the offset after it
/// (Java's `ZoneRules`, its savings-instant transitions; none of the carried zones has a recurring rule).
#[derive(Debug)]
pub struct ZoneRules {
    initial: i32,
    transitions: &'static [(i64, i32)],
}

impl ZoneRules {
    /// `getOffset(Instant)`: the offset after the last transition at or before the instant, else the first offset.
    pub fn offset_at(&self, epoch_second: i64) -> i32 {
        match self.transitions.iter().rev().find(|(at, _)| *at <= epoch_second) {
            Some((_, after)) => *after,
            None => self.initial,
        }
    }
}

/// America/Panama's rules (America/Atikokan, America/Cayman, America/Coral_Harbour; `EST` on JDK 25): local mean
/// time, Colón mean time from 1890, EST from 1908.
static PANAMA: ZoneRules =
    ZoneRules { initial: -19_088, transitions: &[(-2_524_502_512, -19_176), (-1_946_918_424, -18_000)] };

/// America/Phoenix's rules (America/Creston, US/Arizona; `MST` and `PNT`): local mean time, MST from 1883, and the
/// summer times of 1918-1919, 1942-1944 and 1967.
static PHOENIX: ZoneRules = ZoneRules {
    initial: -26_898,
    transitions: &[
        (-2_717_643_600, -25_200),
        (-1_633_273_200, -21_600),
        (-1_615_132_800, -25_200),
        (-1_601_823_600, -21_600),
        (-1_583_683_200, -25_200),
        (-880_210_800, -21_600),
        (-820_519_140, -25_200),
        (-812_653_140, -21_600),
        (-796_845_540, -25_200),
        (-84_380_400, -21_600),
        (-68_659_200, -25_200),
    ],
};

/// The regions of America/Panama's and America/Phoenix's rules (`ZoneRules.equals`, JDK 25).
const RULED: &[(&str, &ZoneRules)] = &[
    ("America/Atikokan", &PANAMA),
    ("America/Cayman", &PANAMA),
    ("America/Coral_Harbour", &PANAMA),
    ("America/Creston", &PHOENIX),
    ("America/Panama", &PANAMA),
    ("America/Phoenix", &PHOENIX),
    ("US/Arizona", &PHOENIX),
];

/// The regions whose offset never changes (`ZoneRules.isFixedOffset()`, JDK 25), with that offset in seconds.
const FIXED: &[(&str, i32)] = &[
    ("Etc/GMT", 0),
    ("Etc/GMT+0", 0),
    ("Etc/GMT+1", -3_600),
    ("Etc/GMT+10", -36_000),
    ("Etc/GMT+11", -39_600),
    ("Etc/GMT+12", -43_200),
    ("Etc/GMT+2", -7_200),
    ("Etc/GMT+3", -10_800),
    ("Etc/GMT+4", -14_400),
    ("Etc/GMT+5", -18_000),
    ("Etc/GMT+6", -21_600),
    ("Etc/GMT+7", -25_200),
    ("Etc/GMT+8", -28_800),
    ("Etc/GMT+9", -32_400),
    ("Etc/GMT-0", 0),
    ("Etc/GMT-1", 3_600),
    ("Etc/GMT-10", 36_000),
    ("Etc/GMT-11", 39_600),
    ("Etc/GMT-12", 43_200),
    ("Etc/GMT-13", 46_800),
    ("Etc/GMT-14", 50_400),
    ("Etc/GMT-2", 7_200),
    ("Etc/GMT-3", 10_800),
    ("Etc/GMT-4", 14_400),
    ("Etc/GMT-5", 18_000),
    ("Etc/GMT-6", 21_600),
    ("Etc/GMT-7", 25_200),
    ("Etc/GMT-8", 28_800),
    ("Etc/GMT-9", 32_400),
    ("Etc/GMT0", 0),
    ("Etc/Greenwich", 0),
    ("Etc/UCT", 0),
    ("Etc/UTC", 0),
    ("Etc/Universal", 0),
    ("Etc/Zulu", 0),
    ("GMT", 0),
    ("GMT0", 0),
    ("Greenwich", 0),
    ("SystemV/AST4", -14_400),
    ("SystemV/CST6", -21_600),
    ("SystemV/EST5", -18_000),
    ("SystemV/HST10", -36_000),
    ("SystemV/MST7", -25_200),
    ("SystemV/PST8", -28_800),
    ("SystemV/YST9", -32_400),
    ("UCT", 0),
    ("UTC", 0),
    ("Universal", 0),
    ("Zulu", 0),
];

/// `ZoneId.SHORT_IDS` on JDK 25.
const SHORT_IDS: &[(&str, &str)] = &[
    ("ACT", "Australia/Darwin"),
    ("AET", "Australia/Sydney"),
    ("AGT", "America/Argentina/Buenos_Aires"),
    ("ART", "Africa/Cairo"),
    ("AST", "America/Anchorage"),
    ("BET", "America/Sao_Paulo"),
    ("BST", "Asia/Dhaka"),
    ("CAT", "Africa/Harare"),
    ("CNT", "America/St_Johns"),
    ("CST", "America/Chicago"),
    ("CTT", "Asia/Shanghai"),
    ("EAT", "Africa/Addis_Ababa"),
    ("ECT", "Europe/Paris"),
    ("EST", "America/Panama"),
    ("HST", "Pacific/Honolulu"),
    ("IET", "America/Indiana/Indianapolis"),
    ("IST", "Asia/Kolkata"),
    ("JST", "Asia/Tokyo"),
    ("MIT", "Pacific/Apia"),
    ("MST", "America/Phoenix"),
    ("NET", "Asia/Yerevan"),
    ("NST", "Pacific/Auckland"),
    ("PLT", "Asia/Karachi"),
    ("PNT", "America/Phoenix"),
    ("PRT", "America/Puerto_Rico"),
    ("PST", "America/Los_Angeles"),
    ("SST", "Pacific/Guadalcanal"),
    ("VST", "Asia/Ho_Chi_Minh"),
];

/// What `ZoneId.of(id, SHORT_IDS)` gives.
#[derive(Debug)]
pub enum Zone {
    /// An offset that never changes, in seconds.
    Fixed(i32),
    /// A region's rules.
    Ruled(&'static ZoneRules),
    /// Java throws a `DateTimeException` (an ID of no valid form, an offset out of range, a region it does not know).
    Unknown,
    /// A region Java knows whose rules this port does not carry (its resolved ID).
    NotCarried(String),
}

/// `ZoneId.of(id, ZoneId.SHORT_IDS)`.
pub fn zone_of(id: &str) -> Zone {
    let id = SHORT_IDS.iter().find(|(short, _)| *short == id).map_or(id, |(_, region)| region);
    if id.len() <= 1 || id.starts_with('+') || id.starts_with('-') {
        return offset_of(id).map_or(Zone::Unknown, Zone::Fixed);
    }
    for prefix in ["UTC", "GMT", "UT"] {
        if let Some(rest) = id.strip_prefix(prefix) {
            // ZoneId.ofWithPrefix: the bare prefix is offset 0, a signed rest an offset, any other a region
            if rest.is_empty() {
                return Zone::Fixed(0);
            }
            if rest.starts_with('+') || rest.starts_with('-') {
                return offset_of(rest).map_or(Zone::Unknown, Zone::Fixed);
            }
            break;
        }
    }
    region(id)
}

/// `ZoneRegion.ofId(id, true)`: a region of a valid name Java's database has.
fn region(id: &str) -> Zone {
    let mut chars = id.chars();
    let valid = chars.next().is_some_and(|c| c.is_ascii_alphabetic())
        && id.len() >= 2
        && chars.all(|c| c.is_ascii_alphanumeric() || "~/._+-".contains(c));
    if !valid || ZONE_IDS.binary_search(&id).is_err() {
        return Zone::Unknown;
    }
    if let Some((_, offset)) = FIXED.iter().find(|(name, _)| *name == id) {
        return Zone::Fixed(*offset);
    }
    match RULED.iter().find(|(name, _)| *name == id) {
        Some((_, rules)) => Zone::Ruled(rules),
        None => Zone::NotCarried(id.to_owned()),
    }
}

/// `ZoneOffset.of(id)`'s total seconds: `Z`, or `+h`, `+hh`, `+hh:mm`, `+hhmm`, `+hh:mm:ss`, `+hhmmss` within
/// +-18:00; `None` where Java throws.
fn offset_of(id: &str) -> Option<i32> {
    if id == "Z" {
        return Some(0);
    }
    let b = id.as_bytes();
    let num = |at: usize, colon: bool| -> Option<i32> {
        if colon && b.get(at - 1) != Some(&b':') {
            return None;
        }
        let d = b.get(at..at + 2)?;
        d.iter().all(u8::is_ascii_digit).then(|| i32::from(d[0] - b'0') * 10 + i32::from(d[1] - b'0'))
    };
    let (hours, minutes, seconds) = match b.len() {
        // "+h" is "+0h"
        2 => (b[1].is_ascii_digit().then(|| i32::from(b[1] - b'0'))?, 0, 0),
        3 => (num(1, false)?, 0, 0),
        5 => (num(1, false)?, num(3, false)?, 0),
        6 => (num(1, false)?, num(4, true)?, 0),
        7 => (num(1, false)?, num(3, false)?, num(5, false)?),
        9 => (num(1, false)?, num(4, true)?, num(7, true)?),
        _ => return None,
    };
    let sign = match b[0] {
        b'+' => 1,
        b'-' => -1,
        _ => return None,
    };
    // ZoneOffset.ofHoursMinutesSeconds' validation
    if hours > 18 || minutes > 59 || seconds > 59 || (hours == 18 && (minutes > 0 || seconds > 0)) {
        return None;
    }
    Some(sign * (hours * 3_600 + minutes * 60 + seconds))
}

/// `EngineStyleH2.timestampLit`'s shift: `iso`'s first 19 UTF-16 units (spaces read as `T`) a local date-time at UTC,
/// respelled at `time_zone` -- `toString` with its `T` a space, then the rest of `iso`, `:00` added when that is 16
/// units long. `Ok(None)` where Java's `DateTimeException` leaves `iso` unshifted (a text that is no date-time, an
/// unknown zone); refused (`IllegalStateException`) for a region whose rules this port does not carry.
pub fn shift_to_zone(iso: &str, time_zone: &str) -> Result<Option<String>, Refusal> {
    // a digit, a separator or a T is ASCII: a prefix holding any other character is no date-time
    if iso.len() < 19 || !iso.as_bytes()[..19].is_ascii() {
        return Ok(None);
    }
    let Some(local) = LocalDateTime::parse(&iso[..19].replace(' ', "T")) else {
        return Ok(None);
    };
    let at = local.epoch_second();
    let offset = match zone_of(time_zone) {
        Zone::Fixed(offset) => offset,
        Zone::Ruled(rules) => rules.offset_at(at),
        Zone::Unknown => return Ok(None),
        Zone::NotCarried(region) => {
            return Err(Refusal::illegal_state(format!(
                "a date-time constant shifted to the connection's time zone '{time_zone}' (legend-engine's \
                 adjustDate): Java reads {region}'s rules from its time-zone database, and the Rust port carries only \
                 the fixed-offset zones', America/Panama's and America/Phoenix's"
            )));
        }
    };
    let Some(shifted) = LocalDateTime::of_epoch_second(at + i64::from(offset)) else {
        return Ok(None);
    };
    let mut out = shifted.to_java_string().replace('T', " ") + &iso[19..];
    if crate::statement::utf16_len(&out) == 16 {
        out.push_str(":00");
    }
    Ok(Some(out))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_local_date_time_reads_as_iso_local_date_time_strictly() {
        let p = |s: &str| LocalDateTime::parse(s).map(|l| l.to_java_string());
        assert_eq!(p("2019-04-08T18:00:00").as_deref(), Some("2019-04-08T18:00"));
        assert_eq!(p("2019-04-08T18:00:30").as_deref(), Some("2019-04-08T18:00:30"));
        assert_eq!(p("2019-04-08t18:00:30").as_deref(), Some("2019-04-08T18:00:30"));
        assert_eq!(p("0000-01-01T03:00:00").as_deref(), Some("0000-01-01T03:00"));
        assert_eq!(p("+123456-01-01T00:00").as_deref(), Some("+123456-01-01T00:00"));
        assert_eq!(p("-123456-01-01T00:00").as_deref(), Some("-123456-01-01T00:00"));
        assert_eq!(p("2024-02-29T00:00:00").as_deref(), Some("2024-02-29T00:00"));
        // each a DateTimeParseException in Java (measured, JDK 25)
        for bad in [
            "2019-02-30T00:00:00",
            "2019-04-08T24:00:00",
            "+2019-04-08T18:00:00",
            "2019-04-08T18:00:00Z",
            "2019-4-08T18:00:00",
            "2019-04-08T18:00:0a",
            "2019-04-08T18:00Z00",
            "1900-02-29T00:00:00",
            "-0000-01-01T00:00",
            "12345-01-01T00:00:0",
        ] {
            assert_eq!(p(bad), None, "{bad}");
        }
    }

    #[test]
    fn a_local_date_time_prints_as_java_prints_it() {
        let at = |y, mo, d, h, mi, s| LocalDateTime { year: y, month: mo, day: d, hour: h, minute: mi, second: s };
        assert_eq!(at(9999, 12, 31, 18, 0, 0).to_java_string(), "9999-12-31T18:00");
        assert_eq!(at(10000, 1, 1, 8, 0, 0).to_java_string(), "+10000-01-01T08:00");
        assert_eq!(at(0, 12, 31, 18, 41, 52).to_java_string(), "0000-12-31T18:41:52");
        assert_eq!(at(-1, 12, 31, 21, 41, 52).to_java_string(), "-0001-12-31T21:41:52");
        assert_eq!(at(-12345, 1, 1, 0, 0, 1).to_java_string(), "-12345-01-01T00:00:01");
        for s in [-62_135_596_800i64, -1, 0, 1, 951_782_400, 253_402_300_799] {
            assert_eq!(LocalDateTime::of_epoch_second(s).unwrap().epoch_second(), s);
        }
    }

    #[test]
    fn a_zone_id_resolves_as_zone_id_of_with_short_ids() {
        let fixed = |id: &str| match zone_of(id) {
            Zone::Fixed(s) => Some(s),
            _ => None,
        };
        // measured on JDK 25: the offset forms, the prefixed forms, the fixed regions
        assert_eq!(fixed("Z"), Some(0));
        assert_eq!(fixed("+05:00"), Some(18_000));
        assert_eq!(fixed("-5"), Some(-18_000));
        assert_eq!(fixed("+05"), Some(18_000));
        assert_eq!(fixed("+0530"), Some(19_800));
        assert_eq!(fixed("+05:30:15"), Some(19_815));
        assert_eq!(fixed("GMT+5"), Some(18_000));
        assert_eq!(fixed("UTC-05:30"), Some(-19_800));
        assert_eq!(fixed("UT+1"), Some(3_600));
        assert_eq!(fixed("UT-0"), Some(0));
        assert_eq!(fixed("UTC+18"), Some(64_800));
        assert_eq!(fixed("GMT"), Some(0));
        assert_eq!(fixed("UTC"), Some(0));
        assert_eq!(fixed("GMT0"), Some(0));
        assert_eq!(fixed("Etc/GMT+5"), Some(-18_000));
        // a DateTimeException each
        for unknown in ["UTC+19", "+5:00", "", "XYZ", "bad id!", "utc", "Europe/Nowhere", "Z1", "UTC+5:30"] {
            assert!(matches!(zone_of(unknown), Zone::Unknown), "{unknown}");
        }
        // SHORT_IDS: EST is America/Panama's region on JDK 25, MST and PNT America/Phoenix's
        assert!(matches!(zone_of("EST"), Zone::Ruled(r) if std::ptr::eq(r, &PANAMA)));
        assert!(matches!(zone_of("MST"), Zone::Ruled(r) if std::ptr::eq(r, &PHOENIX)));
        assert!(matches!(zone_of("US/Arizona"), Zone::Ruled(r) if std::ptr::eq(r, &PHOENIX)));
        assert!(matches!(zone_of("HST"), Zone::NotCarried(r) if r == "Pacific/Honolulu"));
        assert!(matches!(zone_of("America/New_York"), Zone::NotCarried(r) if r == "America/New_York"));
    }

    #[test]
    fn a_constant_shifts_as_the_engine_text_shifts_it() {
        let shift = |iso: &str, tz: &str| shift_to_zone(iso, tz).unwrap();
        // the oracle's: EST is UTC-5 in 2019
        assert_eq!(shift("2019-04-08 18:00:00", "EST").as_deref(), Some("2019-04-08 13:00:00"));
        assert_eq!(shift("2019-04-08 18:00:30", "EST").as_deref(), Some("2019-04-08 13:00:30"));
        // Java's toString drops a zero second; ':00' is added back only when nothing follows
        assert_eq!(shift("2019-04-08 18:00:00.000", "EST").as_deref(), Some("2019-04-08 13:00.000"));
        // measured: EST before 1890 is Panama's local mean time, -5:18:08
        assert_eq!(shift("0001-01-01 00:00:00", "EST").as_deref(), Some("0000-12-31 18:41:52"));
        assert_eq!(shift("0000-01-01 03:00:00", "EST").as_deref(), Some("-0001-12-31 21:41:52"));
        assert_eq!(shift("9999-12-31 23:00:00", "+09:00").as_deref(), Some("+10000-01-01 08:00"));
        // Arizona's 1967 summer time, and no summer time since
        assert_eq!(shift("1967-07-01 12:00:00", "US/Arizona").as_deref(), Some("1967-07-01 06:00:00"));
        assert_eq!(shift("2019-07-01 12:00:00", "US/Arizona").as_deref(), Some("2019-07-01 05:00:00"));
        // a text that is no date-time, or a zone Java does not know: unshifted
        assert_eq!(shift("2019-02-30 00:00:00", "EST"), None);
        assert_eq!(shift("2019-04-08 18:00:00", "Mars/Olympus"), None);
        let r = shift_to_zone("2019-04-08 18:00:00", "America/New_York").unwrap_err();
        assert_eq!(r.class, Refusal::ILLEGAL_STATE);
        assert!(r.message.contains("America/New_York"), "{}", r.message);
    }
}
