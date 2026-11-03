// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! The part of Java's `java.time` the engine text's date-time constant uses (`EngineStyleH2.timestampLit`, legend-engine's
//! adjustDate): a local date-time read strictly (`LocalDateTime.parse`), taken as UTC and respelled in a connection's
//! zone (`ZoneId.of(id, ZoneId.SHORT_IDS)`, `atZone(UTC).withZoneSameInstant(zone).toLocalDateTime()`), and printed as
//! `LocalDateTime.toString` prints it.
//!
//! A zone's rules are the JDK's time-zone database, embedded: `rust/oracles/tzdb/tzdb.bin`, written by
//! `rust/oracles/tzdb/src/main/java/tzdb/TzdbData.java` on the build's JDK (JDK 25.0.4, tzdb 2026b; its MANIFEST.tsv):
//! every region `ZoneId.of` accepts and the short IDs, each region's rules exactly as `java.time.zone.ZoneRules` holds
//! them -- the standard transitions and offsets, the savings transitions and wall offsets, the last rules. [`ZoneRules`]
//! answers over them as Java's methods do, each written from its JDK 25 source: `getOffset(Instant)`,
//! `getStandardOffset`, `getValidOffsets`, `getTransition` (a gap or an overlap), and `ZonedDateTime.of`'s resolution of
//! a local date-time; `ZoneOffsetTransitionRule.createTransition` makes a year's transitions past the last historic one,
//! for any year. Where Java throws a `DateTimeException` the answer is `None`: `timestampLit` catches it and leaves the
//! constant unshifted.

use std::sync::LazyLock;

/// The JDK's tzdb in the LTZDB format (`TzdbData.java`).
static TZDB_BYTES: &[u8] = include_bytes!("../../oracles/tzdb/tzdb.bin");

/// `Year.MAX_VALUE`: a year past it (either way) is refused by `LocalDate`.
const MAX_YEAR: i64 = 999_999_999;

// ==================================================================
// LocalDateTime
// ==================================================================

/// Java's `LocalDateTime` to the second (the engine text reads 19 characters: never a fraction). Ordered as Java's
/// `compareTo`: the year, then the month, the day, the hour, the minute, the second.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct LocalDateTime {
    pub year: i64,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
}

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

    /// The instant this local date-time is at offset 0, in seconds from the epoch (`toEpochSecond(UTC)`).
    pub fn epoch_second(&self) -> i64 {
        days_from_civil(self.year, self.month, self.day) * 86_400
            + i64::from(self.hour) * 3_600
            + i64::from(self.minute) * 60
            + i64::from(self.second)
    }

    /// `toEpochSecond(offset)`.
    pub fn to_epoch_second(&self, offset: i32) -> i64 {
        self.epoch_second() - i64::from(offset)
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

    /// `ofEpochSecond(s, 0, offset)`.
    pub fn at_offset(s: i64, offset: i32) -> Option<LocalDateTime> {
        LocalDateTime::of_epoch_second(s + i64::from(offset))
    }

    /// `plusSeconds(n)`.
    pub fn plus_seconds(&self, n: i64) -> Option<LocalDateTime> {
        LocalDateTime::of_epoch_second(self.epoch_second() + n)
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

/// The ISO day of the week of an epoch day: Monday 1 to Sunday 7 (1970-01-01 is a Thursday).
fn day_of_week(epoch_day: i64) -> i64 {
    (epoch_day + 3).rem_euclid(7) + 1
}

// ==================================================================
// ZoneRules
// ==================================================================

/// `ZoneOffsetTransition`: an instant where the wall offset changes, with the offsets before and after.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Transition {
    pub epoch_second: i64,
    pub offset_before: i32,
    pub offset_after: i32,
}

impl Transition {
    /// `new ZoneOffsetTransition(LocalDateTime transition, before, after)`: the transition's local date-time at its
    /// offset before.
    fn at_local(transition: LocalDateTime, offset_before: i32, offset_after: i32) -> Transition {
        Transition { epoch_second: transition.to_epoch_second(offset_before), offset_before, offset_after }
    }

    /// `isGap()`: the offset grows, so local date-times are skipped.
    pub fn is_gap(&self) -> bool {
        self.offset_after > self.offset_before
    }

    /// `getDateTimeBefore()` (`None` past `LocalDateTime`'s range).
    pub fn date_time_before(&self) -> Option<LocalDateTime> {
        LocalDateTime::at_offset(self.epoch_second, self.offset_before)
    }

    /// `getDateTimeAfter()`.
    pub fn date_time_after(&self) -> Option<LocalDateTime> {
        LocalDateTime::at_offset(self.epoch_second, self.offset_after)
    }

    /// `getValidOffsets()`: none in a gap, both in an overlap.
    pub fn valid_offsets(&self) -> Vec<i32> {
        if self.is_gap() { Vec::new() } else { vec![self.offset_before, self.offset_after] }
    }
}

/// `ZoneOffsetTransitionRule.TimeDefinition`, in Java's declaration order (its `ordinal()` in the data).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TimeDefinition {
    Utc,
    Wall,
    Standard,
}

/// `ZoneOffsetTransitionRule`: how a year's transition past the last historic one is made.
#[derive(Debug, Clone, PartialEq, Eq)]
struct TransitionRule {
    month: u32,
    /// `getDayOfMonthIndicator()`: a day, or (negative) one counted from the month's end
    dom: i32,
    /// `getDayOfWeek()`, ISO 1-7, or none
    dow: Option<i64>,
    second_of_day: i64,
    end_of_day: bool,
    time_definition: TimeDefinition,
    standard_offset: i32,
    offset_before: i32,
    offset_after: i32,
}

impl TransitionRule {
    /// `createTransition(year)`; `None` where Java's `LocalDate` arithmetic throws.
    fn create_transition(&self, year: i64) -> Option<Transition> {
        let mut date = if self.dom < 0 {
            let day = month_length(year, self.month) as i32 + 1 + self.dom;
            let mut d = epoch_day_of(year, self.month, day)?;
            if let Some(dow) = self.dow {
                // previousOrSame(dow)
                let diff = day_of_week(d) - dow;
                if diff != 0 {
                    d -= if diff >= 0 { diff } else { diff + 7 };
                }
            }
            d
        } else {
            let mut d = epoch_day_of(year, self.month, self.dom)?;
            if let Some(dow) = self.dow {
                // nextOrSame(dow)
                let diff = day_of_week(d) - dow;
                if diff != 0 {
                    d += if diff >= 0 { 7 - diff } else { -diff };
                }
            }
            d
        };
        if self.end_of_day {
            date += 1;
        }
        let local = LocalDateTime::of_epoch_second(date * 86_400 + self.second_of_day)?;
        let shift = match self.time_definition {
            TimeDefinition::Utc => i64::from(self.offset_before),
            TimeDefinition::Standard => i64::from(self.offset_before - self.standard_offset),
            TimeDefinition::Wall => 0,
        };
        let transition = local.plus_seconds(shift)?;
        Some(Transition::at_local(transition, self.offset_before, self.offset_after))
    }
}

/// `LocalDate.of(year, month, day)` as an epoch day; `None` for a day the month does not have.
fn epoch_day_of(year: i64, month: u32, day: i32) -> Option<i64> {
    if year.abs() > MAX_YEAR || day < 1 || day > month_length(year, month) as i32 {
        return None;
    }
    Some(days_from_civil(year, month, day as u32))
}

/// What `getOffsetInfo` answers: the one offset of a local date-time, or the transition it falls in (a gap or an
/// overlap).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OffsetInfo {
    Offset(i32),
    Transition(Transition),
}

/// `java.time.zone.ZoneRules` of a region, its arrays as the JDK holds them.
#[derive(Debug, PartialEq)]
pub struct ZoneRules {
    standard_transitions: Vec<i64>,
    standard_offsets: Vec<i32>,
    savings_instant_transitions: Vec<i64>,
    /// computed from the instants as Java's constructor computes them: per transition, a gap's local date-times before
    /// then after, an overlap's after then before
    savings_local_transitions: Vec<LocalDateTime>,
    wall_offsets: Vec<i32>,
    last_rules: Vec<TransitionRule>,
}

/// `Arrays.binarySearch`, Java's own algorithm (which of equal elements it finds is its, not Rust's): the index found,
/// else `-(insertion point) - 1`.
fn java_binary_search<T: Ord>(a: &[T], key: &T) -> i64 {
    let (mut low, mut high) = (0i64, a.len() as i64 - 1);
    while low <= high {
        let mid = (low + high) >> 1;
        match a[mid as usize].cmp(key) {
            std::cmp::Ordering::Less => low = mid + 1,
            std::cmp::Ordering::Greater => high = mid - 1,
            std::cmp::Ordering::Equal => return mid,
        }
    }
    -(low + 1)
}

impl ZoneRules {
    fn new(
        standard_transitions: Vec<i64>,
        standard_offsets: Vec<i32>,
        savings_instant_transitions: Vec<i64>,
        wall_offsets: Vec<i32>,
        last_rules: Vec<TransitionRule>,
    ) -> ZoneRules {
        let mut locals = Vec::with_capacity(savings_instant_transitions.len() * 2);
        for (i, &epoch_second) in savings_instant_transitions.iter().enumerate() {
            let t = Transition { epoch_second, offset_before: wall_offsets[i], offset_after: wall_offsets[i + 1] };
            let (before, after) = (t.date_time_before(), t.date_time_after());
            let (before, after) = before.zip(after).expect("a historic transition is within LocalDateTime's range");
            if t.is_gap() {
                locals.push(before);
                locals.push(after);
            } else {
                locals.push(after);
                locals.push(before);
            }
        }
        ZoneRules {
            standard_transitions,
            standard_offsets,
            savings_instant_transitions,
            savings_local_transitions: locals,
            wall_offsets,
            last_rules,
        }
    }

    /// `findTransitionArray(year)`: the year's transitions by the last rules.
    fn transition_array(&self, year: i64) -> Option<Vec<Transition>> {
        self.last_rules.iter().map(|r| r.create_transition(year)).collect()
    }

    /// `getOffset(Instant)`.
    pub fn offset_at(&self, epoch_second: i64) -> Option<i32> {
        let savings = &self.savings_instant_transitions;
        let Some(&last) = savings.last() else {
            return Some(self.wall_offsets[0]);
        };
        if !self.last_rules.is_empty() && epoch_second > last {
            let year = find_year(epoch_second, *self.wall_offsets.last().expect("a wall offset"));
            let transitions = self.transition_array(year)?;
            for t in &transitions {
                if epoch_second < t.epoch_second {
                    return Some(t.offset_before);
                }
            }
            return Some(transitions.last().expect("a last rule").offset_after);
        }
        let index = java_binary_search(savings, &epoch_second);
        let index = if index < 0 { -index - 2 } else { index };
        Some(self.wall_offsets[(index + 1) as usize])
    }

    /// `getStandardOffset(Instant)`.
    pub fn standard_offset_at(&self, epoch_second: i64) -> i32 {
        if self.standard_transitions.is_empty() {
            return self.standard_offsets[0];
        }
        let index = java_binary_search(&self.standard_transitions, &epoch_second);
        let index = if index < 0 { -index - 2 } else { index };
        self.standard_offsets[(index + 1) as usize]
    }

    /// `getOffsetInfo(LocalDateTime)`: the offset, or the gap or overlap the local date-time falls in.
    pub fn offset_info(&self, dt: &LocalDateTime) -> Option<OffsetInfo> {
        let locals = &self.savings_local_transitions;
        if locals.is_empty() {
            return Some(OffsetInfo::Offset(self.wall_offsets[0]));
        }
        if !self.last_rules.is_empty() && dt > locals.last().expect("a local transition") {
            let mut info = None;
            for t in &self.transition_array(dt.year)? {
                let i = find_offset_info(dt, t)?;
                if matches!(i, OffsetInfo::Transition(_)) || i == OffsetInfo::Offset(t.offset_before) {
                    return Some(i);
                }
                info = Some(i);
            }
            return info;
        }
        let index = java_binary_search(locals, dt);
        if index == -1 {
            // before the first transition
            return Some(OffsetInfo::Offset(self.wall_offsets[0]));
        }
        let index = if index < 0 {
            -index - 2
        } else if index < locals.len() as i64 - 1 && locals[index as usize] == locals[index as usize + 1] {
            // an overlap right after a gap
            index + 1
        } else {
            index
        } as usize;
        if index & 1 == 0 {
            let (before, after) = (locals[index], locals[index + 1]);
            let (offset_before, offset_after) = (self.wall_offsets[index / 2], self.wall_offsets[index / 2 + 1]);
            let at = if offset_after > offset_before { before } else { after };
            Some(OffsetInfo::Transition(Transition::at_local(at, offset_before, offset_after)))
        } else {
            Some(OffsetInfo::Offset(self.wall_offsets[index / 2 + 1]))
        }
    }
}

/// `findOffsetInfo(dt, transition)`.
fn find_offset_info(dt: &LocalDateTime, t: &Transition) -> Option<OffsetInfo> {
    let before = t.date_time_before()?;
    let after = t.date_time_after()?;
    Some(if t.is_gap() {
        if *dt < before {
            OffsetInfo::Offset(t.offset_before)
        } else if *dt < after {
            OffsetInfo::Transition(*t)
        } else {
            OffsetInfo::Offset(t.offset_after)
        }
    } else if *dt >= before {
        OffsetInfo::Offset(t.offset_after)
    } else if *dt < after {
        OffsetInfo::Offset(t.offset_before)
    } else {
        OffsetInfo::Transition(*t)
    })
}

/// `findYear(epochSecond, offset)`: the year of the local date at that offset, capped at `Year.MAX_VALUE`.
fn find_year(epoch_second: i64, offset: i32) -> i64 {
    let local = epoch_second + i64::from(offset);
    civil_from_days(local.div_euclid(86_400)).0.min(MAX_YEAR)
}

// ==================================================================
// The embedded tzdb
// ==================================================================

/// The JDK's time-zone database: its versions, its rule sets, its regions (sorted by Java's `String` order, ASCII) and
/// `ZoneId.SHORT_IDS`.
#[derive(Debug)]
pub struct Tzdb {
    /// `Runtime.version()` of the JDK that wrote it
    pub java: String,
    /// the tzdb version (`ZoneRulesProvider.getVersions`)
    pub version: String,
    rules: Vec<ZoneRules>,
    regions: Vec<(String, usize)>,
    short_ids: Vec<(String, String)>,
}

impl Tzdb {
    /// The rules of a region Java's provider has (`ZoneRulesProvider.getRules(id)`).
    pub fn region(&self, id: &str) -> Option<&ZoneRules> {
        let at = self.regions.binary_search_by(|(r, _)| r.as_str().cmp(id)).ok()?;
        Some(&self.rules[self.regions[at].1])
    }

    /// Every region's ID, sorted.
    pub fn region_ids(&self) -> impl Iterator<Item = &str> {
        self.regions.iter().map(|(r, _)| r.as_str())
    }

    /// The distinct rule sets.
    pub fn rule_sets(&self) -> usize {
        self.rules.len()
    }

    /// `ZoneId.SHORT_IDS.get(id)`.
    pub fn short_id(&self, id: &str) -> Option<&str> {
        self.short_ids.iter().find(|(k, _)| k == id).map(|(_, v)| v.as_str())
    }

    /// Reads the LTZDB format (`TzdbData.java`): unsigned LEB128 varints, a signed value zigzag-encoded, a string its
    /// UTF-8 length then its bytes.
    fn read(bytes: &[u8]) -> Result<Tzdb, String> {
        let mut r = Reader { bytes, at: 0 };
        if r.take(5)? != b"LTZDB" || r.u()? != 1 {
            return Err("not an LTZDB file of version 1".to_owned());
        }
        let java = r.str()?;
        let version = r.str()?;
        let mut rules = Vec::new();
        for _ in 0..r.u()? {
            let (standard_transitions, standard_offsets) = r.transitions()?;
            let (savings, wall_offsets) = r.transitions()?;
            let mut last_rules = Vec::new();
            for _ in 0..r.u()? {
                let month = r.u()? as u32;
                let dom = r.s()? as i32;
                let dow = match r.u()? {
                    0 => None,
                    d => Some(d as i64),
                };
                let second_of_day = r.u()? as i64;
                let flags = r.u()?;
                let time_definition = match flags >> 1 {
                    0 => TimeDefinition::Utc,
                    1 => TimeDefinition::Wall,
                    2 => TimeDefinition::Standard,
                    other => return Err(format!("time definition {other}")),
                };
                last_rules.push(TransitionRule {
                    month,
                    dom,
                    dow,
                    second_of_day,
                    end_of_day: flags & 1 == 1,
                    time_definition,
                    standard_offset: r.s()? as i32,
                    offset_before: r.s()? as i32,
                    offset_after: r.s()? as i32,
                });
            }
            rules.push(ZoneRules::new(standard_transitions, standard_offsets, savings, wall_offsets, last_rules));
        }
        let mut regions = Vec::new();
        for _ in 0..r.u()? {
            let id = r.str()?;
            let set = r.u()? as usize;
            if set >= rules.len() {
                return Err(format!("{id}'s rule set {set}"));
            }
            regions.push((id, set));
        }
        let mut short_ids = Vec::new();
        for _ in 0..r.u()? {
            short_ids.push((r.str()?, r.str()?));
        }
        if r.at != bytes.len() {
            return Err(format!("{} bytes after the data", bytes.len() - r.at));
        }
        if !regions.windows(2).all(|w| w[0].0 < w[1].0) {
            return Err("the regions are not sorted".to_owned());
        }
        Ok(Tzdb { java, version, rules, regions, short_ids })
    }
}

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn take(&mut self, n: usize) -> Result<&[u8], String> {
        let b = self.bytes.get(self.at..self.at + n).ok_or("the data ends early")?;
        self.at += n;
        Ok(b)
    }

    fn u(&mut self) -> Result<u64, String> {
        let mut v = 0u64;
        for shift in (0..64).step_by(7) {
            let b = self.take(1)?[0];
            v |= u64::from(b & 0x7f) << shift;
            if b < 0x80 {
                return Ok(v);
            }
        }
        Err("a varint past 64 bits".to_owned())
    }

    fn s(&mut self) -> Result<i64, String> {
        let v = self.u()?;
        Ok((v >> 1) as i64 ^ -((v & 1) as i64))
    }

    fn str(&mut self) -> Result<String, String> {
        let n = self.u()? as usize;
        String::from_utf8(self.take(n)?.to_vec()).map_err(|e| e.to_string())
    }

    /// A transition list: its count, the offset before them all, then each epoch second (the first absolute, each
    /// next a delta) and the offset after it.
    fn transitions(&mut self) -> Result<(Vec<i64>, Vec<i32>), String> {
        let n = self.u()? as usize;
        let mut offsets = vec![self.s()? as i32];
        let mut transitions = Vec::with_capacity(n);
        let mut previous = 0i64;
        for i in 0..n {
            let t = if i == 0 { self.s()? } else { previous + self.u()? as i64 };
            transitions.push(t);
            previous = t;
            offsets.push(self.s()? as i32);
        }
        Ok((transitions, offsets))
    }
}

static TZDB: LazyLock<Tzdb> =
    LazyLock::new(|| Tzdb::read(TZDB_BYTES).unwrap_or_else(|e| panic!("the embedded tzdb is unreadable: {e}")));

/// The embedded tzdb, read once.
pub fn tzdb() -> &'static Tzdb {
    &TZDB
}

/// The embedded tzdb's bytes, as committed (`rust/oracles/tzdb/tzdb.bin`): the differential holds them to the
/// generator's output on the build's JDK, so a JDK bump that changes the rules fails until they are regenerated.
pub fn tzdb_bytes() -> &'static [u8] {
    TZDB_BYTES
}

// ==================================================================
// ZoneId
// ==================================================================

/// What `ZoneId.of(id, SHORT_IDS)` gives.
#[derive(Debug, Clone, Copy)]
pub enum Zone {
    /// An offset that never changes, in seconds (an offset ID, or `UTC`/`GMT`/`UT` and an offset).
    Fixed(i32),
    /// A region's rules.
    Region(&'static ZoneRules),
    /// Java throws a `DateTimeException`: an ID of no valid form, an offset out of range, a region it does not know.
    Unknown,
}

impl Zone {
    /// `getRules().getOffset(Instant)`; `None` for an unknown zone, or where Java throws.
    pub fn offset_at(&self, epoch_second: i64) -> Option<i32> {
        match self {
            Zone::Fixed(o) => Some(*o),
            Zone::Region(r) => r.offset_at(epoch_second),
            Zone::Unknown => None,
        }
    }

    /// `getRules().getStandardOffset(Instant)`.
    pub fn standard_offset_at(&self, epoch_second: i64) -> Option<i32> {
        match self {
            Zone::Fixed(o) => Some(*o),
            Zone::Region(r) => Some(r.standard_offset_at(epoch_second)),
            Zone::Unknown => None,
        }
    }

    fn offset_info(&self, dt: &LocalDateTime) -> Option<OffsetInfo> {
        match self {
            Zone::Fixed(o) => Some(OffsetInfo::Offset(*o)),
            Zone::Region(r) => r.offset_info(dt),
            Zone::Unknown => None,
        }
    }

    /// `getRules().getValidOffsets(LocalDateTime)`.
    pub fn valid_offsets(&self, dt: &LocalDateTime) -> Option<Vec<i32>> {
        Some(match self.offset_info(dt)? {
            OffsetInfo::Offset(o) => vec![o],
            OffsetInfo::Transition(t) => t.valid_offsets(),
        })
    }

    /// `getRules().getTransition(LocalDateTime)`: the gap or overlap it falls in, if any.
    pub fn transition(&self, dt: &LocalDateTime) -> Option<Option<Transition>> {
        Some(match self.offset_info(dt)? {
            OffsetInfo::Offset(_) => None,
            OffsetInfo::Transition(t) => Some(t),
        })
    }

    /// `ZonedDateTime.of(dt, zone)`: the local date-time (a gap's moved later by the gap's length) and its offset (an
    /// overlap's earlier one).
    pub fn resolve(&self, dt: &LocalDateTime) -> Option<(LocalDateTime, i32)> {
        let valid = self.valid_offsets(dt)?;
        match valid.as_slice() {
            [] => {
                let t = self.transition(dt)??;
                Some((dt.plus_seconds(i64::from(t.offset_after - t.offset_before))?, t.offset_after))
            }
            [first, ..] => Some((*dt, *first)),
        }
    }
}

/// `ZoneId.of(id, ZoneId.SHORT_IDS)`.
pub fn zone_of(id: &str) -> Zone {
    let data = tzdb();
    let id = data.short_id(id).unwrap_or(id);
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
    // ZoneRegion.ofId(id, true): a valid name the provider has
    let mut chars = id.chars();
    let valid = chars.next().is_some_and(|c| c.is_ascii_alphabetic())
        && id.len() >= 2
        && chars.all(|c| c.is_ascii_alphanumeric() || "~/._+-".contains(c));
    match data.region(id) {
        Some(rules) if valid => Zone::Region(rules),
        _ => Zone::Unknown,
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
/// units long. `None` where Java's `DateTimeException` leaves `iso` unshifted (a text that is no date-time, an unknown
/// zone, a date past `LocalDateTime`'s range).
pub fn shift_to_zone(iso: &str, time_zone: &str) -> Option<String> {
    // a digit, a separator or a T is ASCII: a prefix holding any other character is no date-time
    if iso.len() < 19 || !iso.as_bytes()[..19].is_ascii() {
        return None;
    }
    let local = LocalDateTime::parse(&iso[..19].replace(' ', "T"))?;
    let at = local.epoch_second();
    let shifted = LocalDateTime::at_offset(at, zone_of(time_zone).offset_at(at)?)?;
    let mut out = shifted.to_java_string().replace('T', " ") + &iso[19..];
    if crate::statement::utf16_len(&out) == 16 {
        out.push_str(":00");
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(y: i64, mo: u32, d: u32, h: u32, mi: u32, s: u32) -> LocalDateTime {
        LocalDateTime { year: y, month: mo, day: d, hour: h, minute: mi, second: s }
    }

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
        assert_eq!(at(9999, 12, 31, 18, 0, 0).to_java_string(), "9999-12-31T18:00");
        assert_eq!(at(10000, 1, 1, 8, 0, 0).to_java_string(), "+10000-01-01T08:00");
        assert_eq!(at(0, 12, 31, 18, 41, 52).to_java_string(), "0000-12-31T18:41:52");
        assert_eq!(at(-1, 12, 31, 21, 41, 52).to_java_string(), "-0001-12-31T21:41:52");
        assert_eq!(at(-12345, 1, 1, 0, 0, 1).to_java_string(), "-12345-01-01T00:00:01");
        for s in [-62_135_596_800i64, -1, 0, 1, 951_782_400, 253_402_300_799] {
            assert_eq!(LocalDateTime::of_epoch_second(s).unwrap().epoch_second(), s);
        }
        // 1970-01-01 is a Thursday; 2026-11-03 a Tuesday
        assert_eq!(day_of_week(0), 4);
        assert_eq!(day_of_week(days_from_civil(2026, 11, 3)), 2);
    }

    #[test]
    fn the_embedded_tzdb_is_the_build_jdks() {
        let data = tzdb();
        assert_eq!((data.java.as_str(), data.version.as_str()), ("25.0.4+7-LTS", "2026b"));
        assert_eq!(data.region_ids().count(), 604);
        assert_eq!(data.rule_sets(), 345);
        assert_eq!(data.short_id("EST"), Some("America/Panama"));
        assert_eq!(data.short_id("MST"), Some("America/Phoenix"));
        assert_eq!(data.short_id("UTC"), None);
        // links share their rules
        assert!(std::ptr::eq(data.region("US/Arizona").unwrap(), data.region("America/Phoenix").unwrap()));
    }

    #[test]
    fn a_zone_id_resolves_as_zone_id_of_with_short_ids() {
        let fixed = |id: &str| match zone_of(id) {
            Zone::Fixed(s) => Some(s),
            _ => None,
        };
        // measured on JDK 25: the offset forms and the prefixed forms
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
        // a region by its name; GMT0 and Etc/GMT+5 are the provider's
        for region in ["GMT0", "Etc/GMT+5", "EST", "HST", "America/New_York", "US/Arizona"] {
            assert!(matches!(zone_of(region), Zone::Region(_)), "{region}");
        }
        assert_eq!(zone_of("Etc/GMT+5").offset_at(0), Some(-18_000));
        // a DateTimeException each
        for unknown in ["UTC+19", "+5:00", "", "XYZ", "bad id!", "utc", "Europe/Nowhere", "Z1", "UTC+5:30"] {
            assert!(matches!(zone_of(unknown), Zone::Unknown), "{unknown}");
        }
    }

    #[test]
    fn rules_answer_gaps_and_overlaps_as_java_does() {
        let ny = zone_of("America/New_York");
        // 2019-03-10 02:00 EST jumps to 03:00 EDT: 02:30 is in the gap, resolved an hour later
        let gap = at(2019, 3, 10, 2, 30, 0);
        assert_eq!(ny.valid_offsets(&gap), Some(vec![]));
        let t = ny.transition(&gap).unwrap().unwrap();
        assert_eq!((t.epoch_second, t.offset_before, t.offset_after), (1_552_201_200, -18_000, -14_400));
        assert_eq!(ny.resolve(&gap), Some((at(2019, 3, 10, 3, 30, 0), -14_400)));
        // 2019-11-03 02:00 EDT falls back to 01:00 EST: 01:30 is in the overlap, its earlier offset kept
        let overlap = at(2019, 11, 3, 1, 30, 0);
        assert_eq!(ny.valid_offsets(&overlap), Some(vec![-14_400, -18_000]));
        assert_eq!(ny.resolve(&overlap), Some((overlap, -14_400)));
        // far past the last historic transition, the last rules make the year's
        assert_eq!(ny.offset_at(at(2500, 7, 1, 12, 0, 0).epoch_second()), Some(-14_400));
        assert_eq!(ny.offset_at(at(2500, 1, 1, 12, 0, 0).epoch_second()), Some(-18_000));
        assert_eq!(ny.standard_offset_at(at(2500, 7, 1, 12, 0, 0).epoch_second()), Some(-18_000));
        // Lord Howe's half-hour summer time; Dublin's, which the JDK holds in tzdb's rearguard form (measured: standard
        // offset 0 all year, the summer's hour its daylight saving)
        assert_eq!(zone_of("Australia/Lord_Howe").offset_at(at(2019, 1, 1, 0, 0, 0).epoch_second()), Some(39_600));
        assert_eq!(zone_of("Australia/Lord_Howe").offset_at(at(2019, 7, 1, 0, 0, 0).epoch_second()), Some(37_800));
        let dublin = zone_of("Europe/Dublin");
        let summer = at(2020, 7, 15, 12, 0, 0).epoch_second();
        assert_eq!((dublin.offset_at(summer), dublin.standard_offset_at(summer)), (Some(3_600), Some(0)));
    }

    #[test]
    fn a_constant_shifts_as_the_engine_text_shifts_it() {
        let shift = |iso: &str, tz: &str| shift_to_zone(iso, tz);
        // the oracle's: EST is America/Panama, UTC-5 since 1908
        assert_eq!(shift("2019-04-08 18:00:00", "EST").as_deref(), Some("2019-04-08 13:00:00"));
        assert_eq!(shift("2019-04-08 18:00:30", "EST").as_deref(), Some("2019-04-08 13:00:30"));
        // Java's toString drops a zero second; ':00' is added back only when nothing follows
        assert_eq!(shift("2019-04-08 18:00:00.000", "EST").as_deref(), Some("2019-04-08 13:00.000"));
        // measured: EST before 1890 is Panama's local mean time, -5:18:08
        assert_eq!(shift("0001-01-01 00:00:00", "EST").as_deref(), Some("0000-12-31 18:41:52"));
        assert_eq!(shift("0000-01-01 03:00:00", "EST").as_deref(), Some("-0001-12-31 21:41:52"));
        assert_eq!(shift("9999-12-31 23:00:00", "+09:00").as_deref(), Some("+10000-01-01 08:00"));
        // Arizona's 1967 summer time, and no summer time since; New York's
        assert_eq!(shift("1967-07-01 12:00:00", "US/Arizona").as_deref(), Some("1967-07-01 06:00:00"));
        assert_eq!(shift("2019-07-01 12:00:00", "US/Arizona").as_deref(), Some("2019-07-01 05:00:00"));
        assert_eq!(shift("2019-04-08 18:00:00", "America/New_York").as_deref(), Some("2019-04-08 14:00:00"));
        // a text that is no date-time, or a zone Java does not know: unshifted
        assert_eq!(shift("2019-02-30 00:00:00", "EST"), None);
        assert_eq!(shift("2019-04-08 18:00:00", "Mars/Olympus"), None);
    }
}
