// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! Trino's scalar functions: Java's `call` -- every `SqlFn` decided by an exhaustive match -- and `listCall`, with the
//! helpers their arms name (Trino.java, "Scalar functions -- every SqlFn decided here" to "temporal helpers").

use super::{P, Trino, is_json, is_text, lambda_arg, pc, rows, text, type_of};
use crate::ansi::{self, cast_type_name, part};
use crate::refusal::{Refusal, arg};
use crate::renderer::AnsiSqlRenderer;
use crate::writer::SqlWriter;
use lite_javacompat::character;
use lite_sql::{AggFn, DateFmt, DateFmtPart, DatePart, Scalar, SqlExpr, SqlFn, SqlType, TypeFact};

/// The list functions whose answer over the empty collection (a NULL list) is NULL, as DuckDB's are (Java's
/// `NULL_OVER_EMPTY`).
const NULL_OVER_EMPTY: &[SqlFn] = &[
    SqlFn::ListFilter,
    SqlFn::ListTransform,
    SqlFn::ListGet,
    SqlFn::ListPosition,
    SqlFn::ListDistinct,
    SqlFn::ListSum,
    SqlFn::ListMin,
    SqlFn::ListMax,
    SqlFn::ListAvg,
    SqlFn::ListMedian,
    SqlFn::ListMode,
    SqlFn::ListSort,
    SqlFn::ListSortDesc,
    SqlFn::ListReverse,
    SqlFn::ListTail,
    SqlFn::ListInit,
    SqlFn::ListSlice,
    SqlFn::ListProduct,
    SqlFn::ListLength,
    SqlFn::ListFlatten,
];

/// The zone abbreviations Trino does not know, as its tz database's fixed-offset zones (Java's `FIXED_ZONES`).
fn fixed_zone(name: &str) -> Option<&'static str> {
    match name {
        "EST" => Some("-05:00"),
        "MST" => Some("-07:00"),
        "HST" => Some("-10:00"),
        _ => None,
    }
}

fn wall(f: SqlFn, why: &str) -> Refusal {
    Refusal::capability(format!("{f} reached Trino: {why}"))
}

/// A unit as Trino's date_add and date_trunc name it (Java's `intervalUnit`).
fn interval_unit(unit: DatePart) -> Result<&'static str, Refusal> {
    Ok(match unit {
        DatePart::Year => "year",
        DatePart::Quarter => "quarter",
        DatePart::Month => "month",
        DatePart::Week => "week",
        DatePart::Day => "day",
        DatePart::Hour => "hour",
        DatePart::Minute => "minute",
        DatePart::Second => "second",
        DatePart::Millisecond => "millisecond",
        DatePart::Microsecond => "microsecond",
        DatePart::Nanosecond | DatePart::DayOfYear | DatePart::IsoDayOfWeek => {
            return Err(Refusal::capability(format!("interval unit {unit} has no Trino spelling")));
        }
    })
}

/// A fixed-length bucket's length in microseconds; none for a calendar one (Java's `bucketMicros`).
fn bucket_micros(unit: DatePart) -> Option<i64> {
    match unit {
        DatePart::Week => Some(7 * 86_400_000_000),
        DatePart::Day => Some(86_400_000_000),
        DatePart::Hour => Some(3_600_000_000),
        DatePart::Minute => Some(60_000_000),
        DatePart::Second => Some(1_000_000),
        DatePart::Millisecond => Some(1000),
        DatePart::Microsecond => Some(1),
        DatePart::Year
        | DatePart::Quarter
        | DatePart::Month
        | DatePart::Nanosecond
        | DatePart::DayOfYear
        | DatePart::IsoDayOfWeek => None,
    }
}

/// `Character.digit(unit, 10)` of a unit `Character.isDigit` takes: its place in its script's run of ten digits (every
/// run of decimal digits Unicode assigns in the BMP is ten code points, from its zero).
fn digit_value(unit: u16) -> u32 {
    let cp = u32::from(unit);
    let mut zero = cp;
    while zero > 0 && character::is_digit(zero - 1) {
        zero -= 1;
    }
    (cp - zero) % 10
}

/// DuckDB's replacement text (`\1` a group, `$` itself) as Trino's (`$1`, `\$`), each group number raised by `shift`
/// (Java's `javaReplacement`; read by UTF-16 unit, a digit as `Character.isDigit` takes it).
pub(super) fn java_replacement(duck: &str, shift: u32) -> String {
    let units: Vec<u16> = duck.encode_utf16().collect();
    let backslash = u16::from(b'\\');
    let dollar = u16::from(b'$');
    let mut out: Vec<u16> = Vec::with_capacity(units.len());
    let mut i = 0;
    while i < units.len() {
        let ch = units[i];
        if ch == backslash && i + 1 < units.len() && character::is_digit(units[i + 1]) {
            i += 1;
            out.push(dollar);
            out.extend((digit_value(units[i]) + shift).to_string().encode_utf16());
        } else if ch == backslash && i + 1 < units.len() {
            i += 1;
            out.push(backslash);
            out.push(units[i]);
        } else if ch == dollar {
            out.push(backslash);
            out.push(dollar);
        } else {
            out.push(ch);
        }
        i += 1;
    }
    String::from_utf16_lossy(&out)
}

/// Java's `p.matches("(?s).*\\\\[0-9].*")`: a backslash followed by an ASCII digit anywhere.
fn has_back_reference(p: &str) -> bool {
    p.as_bytes().windows(2).any(|w| w[0] == b'\\' && w[1].is_ascii_digit())
}

/// `t`'s microseconds since 1970 (BIGINT): its whole seconds (floored) and its six fraction digits, read from its text
/// (Java's `micros`).
fn micros(t: P<'_>) -> P<'_> {
    pc(move |w| {
        w.append(
            "(date_diff('second', TIMESTAMP '1970-01-01 00:00:00', date_trunc('second', CAST(",
        )
        .piece(&*t)?
        .append(" AS TIMESTAMP(6)))) * 1000000 + CAST(substr(CAST(CAST(")
        .piece(&*t)?
        .append(" AS TIMESTAMP(6)) AS VARCHAR), 21, 6) AS BIGINT))");
        Ok(())
    })
}

/// The naive UTC timestamp `x` microseconds after 1970 (Java's `fromMicros`).
fn from_micros(x: P<'_>) -> P<'_> {
    pc(move |w| {
        w.append("CAST(at_timezone(from_unixtime_nanos((").piece(&*x)?.append(") * 1000), 'UTC') AS TIMESTAMP(6))");
        Ok(())
    })
}

/// The floored epoch (in microseconds per unit) of `to` less `from`'s (Java's `epochBoundaries`).
fn epoch_boundaries<'a>(from: P<'a>, to: P<'a>, per_unit: i64) -> P<'a> {
    pc(move |w| {
        w.append("(floor(CAST(").piece(&*micros(to.clone()))?;
        w.append(&format!(" AS DOUBLE) / {per_unit}) - floor(CAST("));
        w.piece(&*micros(from.clone()))?.append(&format!(" AS DOUBLE) / {per_unit}))"));
        Ok(())
    })
}

/// `d`, NULL where it is zero (Java's `divisor`).
fn divisor<'a>(d: &'a Trino, x: &'a SqlExpr) -> P<'a> {
    pc(move |w| {
        w.append("CASE WHEN ").expr(d, x, 0)?.append(" = 0 THEN NULL ELSE ").expr(d, x, 0)?.append(" END");
        Ok(())
    })
}

/// A regexp extraction's group: the named one, else 0 (Java's `group`).
fn group<'a>(d: &'a Trino, a: &'a [SqlExpr]) -> P<'a> {
    if a.len() > 2 { d.arg_ex(a, 2, 0) } else { text("0") }
}

/// Jaro-Winkler similarity, exactly DuckDB's (Java's `jaroWinkler`): Trino has no Jaro-Winkler function.
fn jaro_winkler<'a>(d: &'a Trino, p: &'a SqlExpr, t: &'a SqlExpr) -> P<'a> {
    let o = "\"__jo\"";
    let s = "\"__js\"";
    let u = "\"__ju\"";
    let v = "\"__jv\"";
    let j = "\"__jj\"";
    let i = "\"__ji\"";
    let ftm = "ROW(f VARCHAR, tm VARCHAR)";
    let ftmk = "ROW(f VARCHAR, tm VARCHAR, k BOOLEAN)";
    let nx = "ROW(n BIGINT, x BIGINT)";
    let lp = format!("length({o}.p)");
    let lt = format!("length({o}.t)");
    let window = format!("greatest(0, greatest({lp}, {lt}) / 2 - 1)");
    let m = format!("CAST({v}.n AS DOUBLE)");
    let jaro = format!(
        "(({m} / CAST({lp} AS DOUBLE) + {m} / CAST({lt} AS DOUBLE) + ({m} - CAST({v}.x / 2 AS DOUBLE)) / {m}) / CAST(3 \
         AS DOUBLE))"
    );
    let prefix: Vec<String> = (1..=4)
        .map(|k| {
            format!(
                "CASE WHEN {k} <= least({lp}, {lt}) AND substr({o}.p, 1, {k}) = substr({o}.t, 1, {k}) THEN 1 ELSE 0 \
                 END"
            )
        })
        .collect();
    let score = format!(
        "CASE WHEN {v}.n = 0 THEN CAST(0 AS DOUBLE) WHEN {jaro} > CAST(0.7 AS DOUBLE) THEN {jaro} + CAST({} AS DOUBLE) \
         * CAST(0.1 AS DOUBLE) * (CAST(1 AS DOUBLE) - {jaro}) ELSE {jaro} END",
        prefix.join(" + ")
    );
    let take = format!(
        "reduce(sequence(greatest(1, {j} - {window}), {j} + {window}), CAST(ROW({s}.f, {s}.tm, false) AS {ftmk}), \
         ({u}, {i}) -> CASE WHEN NOT {u}.k AND substr({u}.f, {i}, 1) = '0' AND substr({o}.p, {i}, 1) = substr({o}.t, \
         {j}, 1) THEN CAST(ROW(concat(substr({u}.f, 1, {i} - 1), '1', substr({u}.f, {i} + 1)), concat({u}.tm, \
         substr({o}.t, {j}, 1)), true) AS {ftmk}) ELSE {u} END, {u} -> CAST(ROW({u}.f, {u}.tm) AS {ftm}))"
    );
    let counted = format!(
        "reduce(sequence(1, {lp}), CAST(ROW(0, 0) AS {nx}), ({v}, {i}) -> CASE WHEN substr({s}.f, {i}, 1) = '1' THEN \
         CAST(ROW({v}.n + 1, {v}.x + CASE WHEN substr({o}.p, {i}, 1) = substr({s}.tm, {v}.n + 1, 1) THEN 0 ELSE 1 \
         END) AS {nx}) ELSE {v} END, {v} -> {score})"
    );
    let body = format!(
        "CASE WHEN {o}.p IS NULL OR {o}.t IS NULL THEN CAST(NULL AS DOUBLE) WHEN {lp} = 0 OR {lt} = 0 THEN CAST(0 AS \
         DOUBLE) ELSE reduce(sequence(1, {lt}), CAST(ROW(rpad('', {lp}, '0'), '') AS {ftm}), ({s}, {j}) -> {take}, \
         {s} -> {counted}) END"
    );
    pc(move |w| {
        w.append("element_at(transform(ARRAY[CAST(ROW(").expr(d, p, 0)?.append(", ").expr(d, t, 0)?;
        w.append(&format!(") AS ROW(p VARCHAR, t VARCHAR))], {o} -> {body}), 1)"));
        Ok(())
    })
}

impl Trino {
    /// A CONCAT argument as text: concat_ws takes strings only (Java's `asText`).
    pub(super) fn as_text(&self, w: &mut SqlWriter, e: &SqlExpr) -> Result<(), Refusal> {
        if is_text(e) || matches!(e, SqlExpr::StringLit { .. }) {
            w.expr(self, e, 0)?;
        } else {
            w.append("CAST(").expr(self, e, 0)?.append(" AS VARCHAR)");
        }
        Ok(())
    }

    /// The pattern anchored at both ends (Java's `anchored`).
    fn anchored<'a>(&'a self, p: &'a SqlExpr) -> Result<P<'a>, Refusal> {
        if let SqlExpr::StringLit { value, .. } = p {
            let s = self.string_lit(&format!("^(?:{value})$"))?;
            return Ok(text(s));
        }
        Ok(pc(move |w| {
            w.append("concat('^(?:', ").expr(self, p, 0)?.append(", ')$')");
            Ok(())
        }))
    }

    /// A zone: an abbreviation Trino does not know as its tz database's fixed-offset zone (Java's `zone`).
    fn zone<'a>(&'a self, z: &'a SqlExpr) -> P<'a> {
        if let SqlExpr::StringLit { value, .. } = z
            && let Some(offset) = fixed_zone(value)
        {
            return text(format!("'{offset}'"));
        }
        self.ex(z, 0)
    }

    /// A date part as Trino's function of it; a Pure integer (Java's `extract`).
    fn extract<'a>(&'a self, a: &'a [SqlExpr]) -> Result<P<'a>, Refusal> {
        let part = part(arg(a, 0)?)?;
        let f = match part {
            DatePart::Year => "year",
            DatePart::Quarter => "quarter",
            DatePart::Month => "month",
            DatePart::Week => "week",
            DatePart::Day => "day",
            DatePart::Hour => "hour",
            DatePart::Minute => "minute",
            DatePart::Second => "second",
            DatePart::DayOfYear => "day_of_year",
            DatePart::IsoDayOfWeek => "day_of_week",
            DatePart::Millisecond | DatePart::Microsecond | DatePart::Nanosecond => {
                return Err(Refusal::capability(format!("date part {part} has no probed Trino spelling")));
            }
        };
        Ok(pc(move |w| {
            w.append(&format!("CAST({f}(")).expr(self, arg(a, 1)?, 0)?.append(") AS BIGINT)");
            Ok(())
        }))
    }

    /// DuckDB's date_diff counts BOUNDARIES crossed (Java's `dateDiff`).
    fn date_diff<'a>(&'a self, a: &'a [SqlExpr]) -> Result<P<'a>, Refusal> {
        let part = part(arg(a, 0)?)?;
        let from = self.arg_ex(a, 1, 0);
        let to = self.arg_ex(a, 2, 0);
        let y = {
            let (from, to) = (from.clone(), to.clone());
            pc(move |w| {
                w.append("(year(").piece(&*to)?.append(") - year(").piece(&*from)?.append("))");
                Ok(())
            })
        };
        let body: P<'a> = match part {
            DatePart::Year => y,
            DatePart::Quarter => {
                let (from, to) = (from.clone(), to.clone());
                pc(move |w| {
                    w.piece(&*y)?.append(" * 4 + (quarter(").piece(&*to)?.append(") - quarter(").piece(&*from)?;
                    w.append("))");
                    Ok(())
                })
            }
            DatePart::Month => {
                let (from, to) = (from.clone(), to.clone());
                pc(move |w| {
                    w.piece(&*y)?.append(" * 12 + (month(").piece(&*to)?.append(") - month(").piece(&*from)?;
                    w.append("))");
                    Ok(())
                })
            }
            DatePart::Week => {
                let (from, to) = (from.clone(), to.clone());
                pc(move |w| {
                    w.append("floor(CAST(date_diff('day', DATE '1969-12-29', CAST(").piece(&*to)?;
                    w.append(" AS DATE)) AS DOUBLE) / 7) - floor(CAST(date_diff('day', DATE '1969-12-29', CAST(");
                    w.piece(&*from)?.append(" AS DATE)) AS DOUBLE) / 7)");
                    Ok(())
                })
            }
            DatePart::Day => {
                let (from, to) = (from.clone(), to.clone());
                pc(move |w| {
                    w.append("date_diff('day', CAST(").piece(&*from)?.append(" AS DATE), CAST(").piece(&*to)?;
                    w.append(" AS DATE))");
                    Ok(())
                })
            }
            DatePart::Hour => epoch_boundaries(from, to, 3_600_000_000),
            DatePart::Minute => epoch_boundaries(from, to, 60_000_000),
            DatePart::Second => epoch_boundaries(from, to, 1_000_000),
            DatePart::Millisecond => epoch_boundaries(from, to, 1000),
            DatePart::Microsecond => {
                let (from, to) = (from.clone(), to.clone());
                pc(move |w| {
                    w.append("(").piece(&*micros(to.clone()))?.append(" - ").piece(&*micros(from.clone()))?.append(")");
                    Ok(())
                })
            }
            DatePart::Nanosecond | DatePart::DayOfYear | DatePart::IsoDayOfWeek => {
                return Err(Refusal::capability(format!("date_diff part {part} has no probed Trino spelling")));
            }
        };
        Ok(pc(move |w| {
            w.append("CAST(").piece(&*body)?.append(" AS BIGINT)");
            Ok(())
        }))
    }

    /// DuckDB's time_bucket (Java's `timeBucket`): fixed units binned from 1970-01-01 (weeks from the Monday
    /// 1969-12-29) in microseconds, floored; months and years by the month index since 1970-01.
    fn time_bucket<'a>(&'a self, a: &'a [SqlExpr]) -> Result<P<'a>, Refusal> {
        let unit = part(arg(a, 0)?)?;
        let t: P<'a> = pc(move |w| {
            w.append("CAST(").expr(self, arg(a, 2)?, 0)?.append(" AS TIMESTAMP(6))");
            Ok(())
        });
        let n: P<'a> = pc(move |w| {
            w.append("CAST(").expr(self, arg(a, 1)?, 0)?.append(" AS BIGINT)");
            Ok(())
        });
        if let Some(per) = bucket_micros(unit) {
            let origin: i64 = if unit == DatePart::Week { -3 * 86_400_000_000 } else { 0 };
            let inner = pc(move |v| {
                v.append("CAST(floor(CAST(").piece(&*micros(t.clone()))?;
                v.append(&format!(" - ({origin}) AS DOUBLE) / (")).piece(&*n)?;
                v.append(&format!(" * {per})) AS BIGINT) * ")).piece(&*n)?;
                v.append(&format!(" * {per} + ({origin})"));
                Ok(())
            });
            return Ok(from_micros(inner));
        }
        let months = match unit {
            DatePart::Month => 1,
            DatePart::Year => 12,
            _ => return Err(Refusal::capability(format!("a {unit} time bucket has no Trino spelling"))),
        };
        let size: P<'a> = pc(move |w| {
            w.append("(").piece(&*n)?.append(&format!(" * {months})"));
            Ok(())
        });
        Ok(pc(move |w| {
            w.append("CAST(date_add('month', CAST(floor(CAST((year(").piece(&*t)?;
            w.append(") - 1970) * 12 + month(").piece(&*t)?.append(") - 1 AS DOUBLE) / ").piece(&*size)?;
            w.append(") * ").piece(&*size)?.append(" AS BIGINT), DATE '1970-01-01') AS TIMESTAMP(6))");
            Ok(())
        }))
    }

    /// The `DATE_TRUNC` arm.
    fn date_trunc(&self, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
        let part = part(arg(a, 0)?)?;
        let to_date = match part {
            DatePart::Month | DatePart::Year | DatePart::Week | DatePart::Quarter => true,
            DatePart::Day
            | DatePart::Hour
            | DatePart::Minute
            | DatePart::Second
            | DatePart::Millisecond
            | DatePart::Microsecond => false,
            DatePart::Nanosecond | DatePart::DayOfYear | DatePart::IsoDayOfWeek => {
                return Err(Refusal::capability(format!("date_trunc to {part} has no Trino spelling")));
            }
        };
        w.append(if to_date { "CAST(date_trunc(" } else { "date_trunc(" });
        let unit = self.string_lit(interval_unit(part)?)?;
        w.append(&unit).append(", ").expr(self, arg(a, 1)?, 0)?.append(")");
        if to_date {
            w.append(" AS DATE)");
        }
        Ok(())
    }

    /// The `MAKE_TIMESTAMP` arm: its text, cast (date_add has no microsecond unit), the seconds to six digits.
    fn make_timestamp(&self, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
        if a.len() != 6 {
            return Err(wall(SqlFn::MakeTimestamp, &format!("make_timestamp of {} arguments", a.len())));
        }
        w.append("CAST(format('%04d-%02d-%02d %02d:%02d:%09.6f', ").list(self, &a[..5])?.append(", CAST(");
        w.expr(self, &a[5], 0)?.append(" AS DOUBLE)) AS TIMESTAMP(6))");
        Ok(())
    }

    /// The `ADD_INTERVAL`, `ADD_INTERVAL_TEMPORAL` arm: date_add has no microsecond unit, so microseconds go through
    /// the epoch's.
    fn add_interval(&self, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
        let part = part(arg(a, 0)?)?;
        let unit = interval_unit(part)?;
        if part == DatePart::Microsecond {
            let t = self.arg_ex(a, 2, 0);
            let x = pc(move |w| {
                w.piece(&*micros(t.clone()))?.append(" + ").expr(self, arg(a, 1)?, 6)?;
                Ok(())
            });
            return from_micros(x)(w);
        }
        let unit = self.string_lit(unit)?;
        w.append("date_add(").append(&unit).append(", ").expr(self, arg(a, 1)?, 0)?.append(", ");
        w.expr(self, arg(a, 2)?, 0)?.append(")");
        Ok(())
    }

    /// The `TIMEZONE` arm: the zone's wall time of the instant.
    fn timezone(&self, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
        w.append("CAST(at_timezone(");
        let t = arg(a, 1)?;
        if matches!(type_of(t), Some(SqlType::Scalar(Scalar::Timestamptz))) {
            w.expr(self, t, 0)?;
        } else {
            w.append("with_timezone(").expr(self, t, 0)?.append(", 'UTC')");
        }
        let z = self.zone(arg(a, 0)?);
        w.append(", ").piece(&*z)?.append(") AS TIMESTAMP(6))");
        Ok(())
    }

    /// The `STRFTIME` arm: `format_datetime`, whose Joda letters carry no digit finer than a millisecond, so a
    /// microsecond or nanosecond part is the value's own six digits, the parts around it `format_datetime`'s.
    fn strftime(&self, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
        let x = arg(a, 0)?;
        let SqlExpr::FormatLit { parts, .. } = arg(a, 1)? else {
            return Err(wall(SqlFn::Strftime, "a date format that is not a format literal: its codes are DuckDB's"));
        };
        let format_datetime = |run: Vec<DateFmt>| self.format_datetime(x, run);
        let mut pieces: Vec<P<'_>> = Vec::new();
        let mut run: Vec<DateFmt> = Vec::new();
        for p in parts {
            if let DateFmt::Part(part @ (DateFmtPart::SubsecMicro | DateFmtPart::SubsecNano)) = p {
                if !run.is_empty() {
                    pieces.push(format_datetime(std::mem::take(&mut run)));
                }
                pieces.push(pc(move |w| {
                    w.append("substr(CAST(CAST(").expr(self, x, 0)?.append(" AS TIMESTAMP(6)) AS VARCHAR), -6)");
                    Ok(())
                }));
                if *part == DateFmtPart::SubsecNano {
                    pieces.push(text("'000'"));
                }
            } else {
                run.push(p.clone());
            }
        }
        if !run.is_empty() || pieces.is_empty() {
            pieces.push(format_datetime(run));
        }
        if pieces.len() == 1 {
            return pieces[0](w);
        }
        w.append("concat(").join(&pieces, ", ", |w, p| p(w))?.append(")");
        Ok(())
    }

    /// `format_datetime(x, format)` (Java's `formatDatetime`).
    fn format_datetime<'a>(&'a self, x: &'a SqlExpr, run: Vec<DateFmt>) -> P<'a> {
        let format = SqlExpr::FormatLit {
            parts: run,
            r#type: TypeFact::Typed { r#type: SqlType::Scalar(Scalar::Varchar), nullable: false },
        };
        pc(move |w| {
            w.append("format_datetime(").expr(self, x, 0)?.append(", ").expr(self, &format, 0)?.append(")");
            Ok(())
        })
    }

    /// The `STRPTIME` arm.
    fn strptime(&self, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
        if !matches!(arg(a, 1)?, SqlExpr::FormatLit { .. }) {
            return Err(wall(SqlFn::Strptime, "a date format that is not a format literal: its codes are DuckDB's"));
        }
        w.append("CAST(parse_datetime(").expr(self, &a[0], 0)?.append(", ").expr(self, &a[1], 0)?;
        w.append(") AS TIMESTAMP(6))");
        Ok(())
    }

    /// The `JSON_TYPE` arm: DuckDB's json_type vocabulary.
    fn json_type(&self, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
        let x = arg(a, 0)?;
        let s = |w: &mut SqlWriter| -> Result<(), Refusal> {
            w.append("json_format(").expr(self, x, 0)?.append(")");
            Ok(())
        };
        w.append("(CASE WHEN ").expr(self, x, 0)?.append(" IS NULL THEN NULL WHEN ").piece(&s)?;
        w.append(" = 'null' THEN 'NULL' WHEN ").piece(&s)?.append(" IN ('true', 'false') THEN 'BOOLEAN'");
        w.append(" WHEN substr(").piece(&s)?.append(", 1, 1) = '\"' THEN 'VARCHAR' WHEN substr(").piece(&s)?;
        w.append(", 1, 1) = '[' THEN 'ARRAY' WHEN substr(").piece(&s)?;
        w.append(", 1, 1) = '{' THEN 'OBJECT' WHEN regexp_like(").piece(&s)?;
        w.append(", '^-?[0-9]+$') THEN 'BIGINT' ELSE 'DOUBLE' END)");
        Ok(())
    }

    /// DuckDB's range: [start, stop) by step, as a filtered sequence (Java's `rangeOf`).
    fn range_of(&self, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
        let start: P<'_> = if a.len() == 1 {
            text("BIGINT '0'")
        } else {
            pc(move |w| {
                w.append("CAST(").expr(self, arg(a, 0)?, 0)?.append(" AS BIGINT)");
                Ok(())
            })
        };
        let stop: P<'_> = pc(move |w| {
            w.append("CAST(").expr(self, arg(a, if a.len() == 1 { 0 } else { 1 })?, 0)?.append(" AS BIGINT)");
            Ok(())
        });
        let step: P<'_> = if a.len() > 2 {
            pc(move |w| {
                w.append("CAST(").expr(self, arg(a, 2)?, 0)?.append(" AS BIGINT)");
                Ok(())
            })
        } else {
            text("BIGINT '1'")
        };
        w.append("(CASE WHEN (").piece(&*stop)?.append(" - ").piece(&*start)?.append(") * ").piece(&*step)?;
        w.append(" > 0 THEN filter(sequence(").piece(&*start)?.append(", ").piece(&*stop)?.append(", ");
        w.piece(&*step)?.append("), \"__g\" -> \"__g\" <> ").piece(&*stop)?;
        w.append(") ELSE CAST(ARRAY[] AS ARRAY(BIGINT)) END)");
        Ok(())
    }

    /// DuckDB's regexp_replace on Trino: the first match alone unless 'g' (Java's `regexpReplace`).
    fn regexp_replace(&self, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
        let global = a.len() > 3 && matches!(&a[3], SqlExpr::StringLit { value, .. } if value.contains('g'));
        let SqlExpr::StringLit { value: r, .. } = arg(a, 2)? else {
            return Err(wall(SqlFn::RegexpReplace, "a computed regexp replacement"));
        };
        if global {
            w.append("regexp_replace(").expr(self, &a[0], 0)?.append(", ").expr(self, &a[1], 0)?.append(", ");
            let lit = self.string_lit(&java_replacement(r, 0))?;
            w.append(&lit).append(")");
            return Ok(());
        }
        if let SqlExpr::StringLit { value: p, .. } = &a[1]
            && has_back_reference(p)
        {
            return Err(wall(SqlFn::RegexpReplace, "a first-match replacement whose pattern has a back reference"));
        }
        w.append("regexp_replace(").expr(self, &a[0], 0)?.append(", ");
        if let SqlExpr::StringLit { value: p, .. } = &a[1] {
            let lit = self.string_lit(&format!("^(?s:(.*?))(?:{p})"))?;
            w.append(&lit);
        } else {
            w.append("concat('^(?s:(.*?))(?:', ").expr(self, &a[1], 0)?.append(", ')')");
        }
        let lit = self.string_lit(&format!("$1{}", java_replacement(r, 1)))?;
        w.append(", ").append(&lit).append(")");
        Ok(())
    }

    /// Every `SqlFn`, as Trino spells it (Java's `call`).
    pub(super) fn call_arm(
        &self,
        w: &mut SqlWriter,
        f: SqlFn,
        a: &[SqlExpr],
        t: &TypeFact,
        parent_prec: i32,
    ) -> Result<(), Refusal> {
        // over the empty collection the answer is NULL: spelled in its declared type, never Trino's unknown
        if NULL_OVER_EMPTY.contains(&f)
            && matches!(a.first(), Some(SqlExpr::NullLit { .. }))
            && let TypeFact::Typed { r#type, .. } = t
        {
            let ty = cast_type_name(self, r#type)?;
            w.append("CAST(NULL AS ").append(&ty).append(")");
            return Ok(());
        }
        let a0 = || arg(a, 0);
        let a1 = || arg(a, 1);
        match f {
            // two ROWs compare field by field, null-safe
            SqlFn::Equal => {
                if rows(a) {
                    ansi::not_distinct_from(self, w, a)
                } else {
                    ansi::infix(self, w, f, a, parent_prec)
                }
            }
            SqlFn::NotEqual => {
                if rows(a) {
                    ansi::distinct_from(self, w, a)
                } else {
                    ansi::infix(self, w, f, a, parent_prec)
                }
            }
            SqlFn::And
            | SqlFn::Or
            | SqlFn::Less
            | SqlFn::LessEqual
            | SqlFn::Greater
            | SqlFn::GreaterEqual
            | SqlFn::Plus
            | SqlFn::Minus
            | SqlFn::Times => ansi::infix(self, w, f, a, parent_prec),
            // the shared spellings (the base's named helpers)
            SqlFn::Not => ansi::not_prefix(self, w, a, parent_prec),
            SqlFn::Negate => ansi::negation(self, w, a),
            SqlFn::IsNull => ansi::is_null_test(self, w, a),
            SqlFn::IsNotNull => ansi::is_not_null_test(self, w, a),
            SqlFn::In => ansi::in_list(self, w, a, parent_prec),
            SqlFn::IsDistinctFrom | SqlFn::NullSafeNotEqual => ansi::distinct_from(self, w, a),
            SqlFn::NullSafeEqual => ansi::not_distinct_from(self, w, a),
            SqlFn::Xor => ansi::xor_as_and_or(self, w, a, parent_prec),
            // 64-bit parse
            SqlFn::ParseInt => ansi::cast_as(self, w, a0()?, "BIGINT"),
            SqlFn::Pi => {
                w.append("pi()");
                Ok(())
            }
            SqlFn::Ceiling => ansi::bigint_of(self, w, "ceil", a),
            SqlFn::Floor => ansi::bigint_of(self, w, "floor", a),
            SqlFn::Sign => ansi::bigint_of(self, w, "sign", a),
            SqlFn::Lpad => ansi::pad_with_space(self, w, "lpad", a),
            SqlFn::Rpad => ansi::pad_with_space(self, w, "rpad", a),
            SqlFn::UcFirst => ansi::first_char_cased(self, w, "upper", a, parent_prec),
            SqlFn::LcFirst => ansi::first_char_cased(self, w, "lower", a, parent_prec),
            SqlFn::Today => {
                w.append("current_date");
                Ok(())
            }
            // Pure's divide is a Float division: both operands DOUBLE
            SqlFn::Divide => ansi::divide_as_doubles(self, w, a),
            SqlFn::FloatCanon => ansi::float_canon_by_text(self, w, a, parent_prec),
            SqlFn::FloatPrint => ansi::float_print_by_text(self, w, a, parent_prec),
            // the hooks: this dialect's own (or the base's refusal)
            SqlFn::Round => self.round_half_even(w, a),
            SqlFn::Hash => self.hash_signed(w, a),
            SqlFn::BitAnd | SqlFn::BitOr | SqlFn::BitXor | SqlFn::BitShiftLeft | SqlFn::BitShiftRight => {
                self.bit_op(w, f, a)
            }
            SqlFn::Unnest => self.unnest_projection(w, a),
            SqlFn::ListFilter
            | SqlFn::ListTransform
            | SqlFn::ListConcat
            | SqlFn::ListGet
            | SqlFn::ListPosition
            | SqlFn::ListZip
            | SqlFn::ListDistinct
            | SqlFn::ListAppend
            | SqlFn::ListSum
            | SqlFn::ListMin
            | SqlFn::ListMax
            | SqlFn::ListAvg
            | SqlFn::ListMedian
            | SqlFn::ListMode
            | SqlFn::ListSort
            | SqlFn::ListSortDesc
            | SqlFn::ListTail
            | SqlFn::ListInit
            | SqlFn::ListProduct
            | SqlFn::ListReduce
            | SqlFn::ListSlice
            | SqlFn::ListBoolAnd
            | SqlFn::ListBoolOr
            | SqlFn::Typeof => self.list_call(w, f, a),
            SqlFn::StructInsert => self.struct_insert(w, a),
            SqlFn::PureSplitPart => self.split_part_call(w, a),
            SqlFn::ListExists => self.list_exists(w, a),
            SqlFn::AllDistinct => self.all_distinct(w, a),
            SqlFn::ListForAll => self.list_for_all(w, a),
            SqlFn::ToVariant => self.variant_construct(w, a),
            SqlFn::VariantElements => self.variant_elements(w, a),
            SqlFn::VariantGet => self.variant_get(w, a),
            // this database's plain spellings: name(args)
            SqlFn::Abs => function(self, w, "abs", a),
            SqlFn::Acos => function(self, w, "acos", a),
            SqlFn::Asin => function(self, w, "asin", a),
            SqlFn::Atan => function(self, w, "atan", a),
            SqlFn::Atan2 => function(self, w, "atan2", a),
            SqlFn::Cbrt => function(self, w, "cbrt", a),
            SqlFn::Chr => function(self, w, "chr", a),
            // Trino unifies no JSON with a varchar: a text operand beside a JSON one is parsed
            SqlFn::Coalesce => {
                if a.iter().any(is_json) && a.iter().any(is_text) {
                    w.append("coalesce(").join(a, ", ", |w, e| {
                        if is_text(e) {
                            w.append("json_parse(").expr(self, e, 0)?.append(")");
                        } else {
                            w.expr(self, e, 0)?;
                        }
                        Ok(())
                    })?;
                    w.append(")");
                    Ok(())
                } else {
                    function(self, w, "coalesce", a)
                }
            }
            SqlFn::Cos => function(self, w, "cos", a),
            SqlFn::Cosh => function(self, w, "cosh", a),
            SqlFn::Degrees => function(self, w, "degrees", a),
            SqlFn::Exp => function(self, w, "exp", a),
            SqlFn::Greatest => function(self, w, "greatest", a),
            SqlFn::Least => function(self, w, "least", a),
            SqlFn::Length => function(self, w, "length", a),
            SqlFn::Levenshtein => function(self, w, "levenshtein_distance", a),
            SqlFn::ListFlatten => function(self, w, "flatten", a),
            SqlFn::Ln => function(self, w, "ln", a),
            SqlFn::Log10 => function(self, w, "log10", a),
            SqlFn::Lower => function(self, w, "lower", a),
            SqlFn::MapKeys => function(self, w, "map_keys", a),
            SqlFn::MapValues => function(self, w, "map_values", a),
            SqlFn::Pow => function(self, w, "power", a),
            SqlFn::Radians => function(self, w, "radians", a),
            SqlFn::Replace => function(self, w, "replace", a),
            SqlFn::Translate => function(self, w, "translate", a),
            SqlFn::ReverseString | SqlFn::ListReverse => function(self, w, "reverse", a),
            SqlFn::Sin => function(self, w, "sin", a),
            SqlFn::Sinh => function(self, w, "sinh", a),
            SqlFn::Sqrt => function(self, w, "sqrt", a),
            SqlFn::Substring => function(self, w, "substr", a),
            SqlFn::Tan => function(self, w, "tan", a),
            SqlFn::Tanh => function(self, w, "tanh", a),
            SqlFn::Upper => function(self, w, "upper", a),
            SqlFn::StartsWith => function(self, w, "starts_with", a),
            SqlFn::Strpos => function(self, w, "strpos", a),
            SqlFn::MapFromEntries => function(self, w, "map_from_entries", a),

            // fail(message): the raise, its message the platform's envelope
            SqlFn::Error => ansi::raise_envelope(self, w, a, "fail"),
            // NULL-ignoring, as DuckDB's concat (Trino's concat is NULL if any argument is)
            SqlFn::Concat => {
                w.append("concat_ws('', ").join(&ansi::flatten_concat(a), ", ", |w, e| self.as_text(w, e))?.append(")");
                Ok(())
            }
            SqlFn::ConcatJoin => {
                w.append("concat_ws('', ").join(a, ", ", |w, e| self.as_text(w, e))?.append(")");
                Ok(())
            }
            // integer division truncates toward zero, as DuckDB's //; a zero divisor is NULL
            SqlFn::IntDivide => {
                w.append("(").expr(self, a0()?, 6)?.append(" / ").piece(&*divisor(self, a1()?))?.append(")");
                Ok(())
            }
            SqlFn::Rem => {
                w.append("MOD(").expr(self, a0()?, 0)?.append(", ").piece(&*divisor(self, a1()?))?.append(")");
                Ok(())
            }
            SqlFn::Mod => {
                w.append("MOD(MOD(").expr(self, a0()?, 0)?.append(", ").piece(&*divisor(self, a1()?))?.append(") + ");
                w.expr(self, a1()?, 6)?.append(", ").piece(&*divisor(self, a1()?))?.append(")");
                Ok(())
            }
            SqlFn::BoolToText => {
                w.append("CAST(").expr(self, a0()?, 0)?.append(" AS VARCHAR)");
                Ok(())
            }
            SqlFn::BitNot => {
                w.append("bitwise_not(CAST(").expr(self, a0()?, 0)?.append(" AS BIGINT))");
                Ok(())
            }
            // floor without the BIGINT conversion
            SqlFn::FloorRaw => {
                w.append("floor(").expr(self, a0()?, 0)?.append(")");
                Ok(())
            }
            // the first character's code point (codepoint takes a VARCHAR(1), and refuses the empty one)
            SqlFn::AsciiCode => {
                w.append("(CASE WHEN length(").expr(self, a0()?, 0)?.append(") = 0 THEN 0 ELSE");
                w.append(" codepoint(CAST(substr(").expr(self, a0()?, 0)?.append(", 1, 1) AS VARCHAR(1))) END)");
                Ok(())
            }
            SqlFn::Cot => {
                w.append("(1 / tan(").expr(self, a0()?, 0)?.append("))");
                Ok(())
            }
            SqlFn::Left => {
                w.append("substr(").expr(self, a0()?, 0)?.append(", 1, greatest(").expr(self, a1()?, 0)?;
                w.append(", 0))");
                Ok(())
            }
            SqlFn::Right => {
                w.append("(CASE WHEN ").expr(self, a1()?, 0)?.append(" <= 0 THEN '' ELSE substr(");
                w.expr(self, a0()?, 0)?.append(", greatest(length(").expr(self, a0()?, 0)?.append(") - ");
                w.expr(self, a1()?, 6)?.append(" + 1, 1)) END)");
                Ok(())
            }
            SqlFn::EndsWith => {
                w.append("(substr(").expr(self, a0()?, 0)?.append(", greatest(length(").expr(self, a0()?, 0)?;
                w.append(") - length(").expr(self, a1()?, 0)?.append(") + 1, 1)) = ").expr(self, a1()?, 0)?;
                w.append(" AND length(").expr(self, a0()?, 0)?.append(") >= length(").expr(self, a1()?, 0)?;
                w.append("))");
                Ok(())
            }
            // array_join skips a NULL element: no string repeated is NULL, not ''
            SqlFn::RepeatStr => {
                w.append("(CASE WHEN ").expr(self, a0()?, 0)?.append(" IS NULL THEN NULL ELSE");
                w.append(" array_join(repeat(").expr(self, a0()?, 0)?.append(", greatest(").expr(self, a1()?, 0)?;
                w.append(", 0)), '') END)");
                Ok(())
            }
            // DuckDB's split_part: the empty string past the end (Trino's is NULL)
            SqlFn::SplitPart => {
                w.append("coalesce(split_part(").list(self, a)?.append("), '')");
                Ok(())
            }
            SqlFn::RoundHalfUp => {
                w.append("round(").expr(self, a0()?, 0)?;
                if a.len() > 1 {
                    w.append(", CAST(").expr(self, &a[1], 0)?.append(" AS INTEGER)");
                }
                w.append(")");
                Ok(())
            }

            // ---- strings
            SqlFn::Matches => {
                w.append("regexp_like(").expr(self, a0()?, 0)?.append(", ").expr(self, a1()?, 0)?.append(")");
                Ok(())
            }
            SqlFn::RegexpFullMatch => {
                w.append("regexp_like(").expr(self, a0()?, 0)?.append(", ");
                let p = self.anchored(a1()?)?;
                w.piece(&*p)?.append(")");
                Ok(())
            }
            SqlFn::RegexpExtract | SqlFn::RegexpExtractAll => {
                let name = if f == SqlFn::RegexpExtract { "regexp_extract(" } else { "regexp_extract_all(" };
                w.append(name).expr(self, a0()?, 0)?.append(", ").expr(self, a1()?, 0)?.append(", ");
                w.piece(&*group(self, a))?.append(")");
                Ok(())
            }
            SqlFn::RegexpReplace => self.regexp_replace(w, a),
            // split takes the delimiter as text
            SqlFn::Split => {
                w.append("split(").expr(self, a0()?, 0)?.append(", ").expr(self, a1()?, 0)?.append(")");
                Ok(())
            }
            SqlFn::Trim | SqlFn::Ltrim | SqlFn::Rtrim => {
                let name = match f {
                    SqlFn::Trim => "trim",
                    SqlFn::Ltrim => "ltrim",
                    _ => "rtrim",
                };
                w.append(name).append("(").expr(self, a0()?, 0)?;
                if a.len() != 1 {
                    w.append(", ").expr(self, a1()?, 0)?;
                }
                w.append(")");
                Ok(())
            }
            SqlFn::EncodeBase64 => {
                w.append("to_base64(to_utf8(").expr(self, a0()?, 0)?.append("))");
                Ok(())
            }
            SqlFn::DecodeBase64 => {
                w.append("from_utf8(from_base64(").expr(self, a0()?, 0)?.append("))");
                Ok(())
            }
            SqlFn::Md5 | SqlFn::Sha1 | SqlFn::Sha256 => {
                let name = match f {
                    SqlFn::Md5 => "md5",
                    SqlFn::Sha1 => "sha1",
                    _ => "sha256",
                };
                w.append(&format!("lower(to_hex({name}(to_utf8(")).expr(self, a0()?, 0)?.append("))))");
                Ok(())
            }
            SqlFn::Guid => {
                w.append("CAST(uuid() AS VARCHAR)");
                Ok(())
            }
            SqlFn::Random => {
                w.append("random()");
                Ok(())
            }
            SqlFn::Format => {
                w.append("format(").list(self, a)?.append(")");
                Ok(())
            }
            SqlFn::JaroWinkler => jaro_winkler(self, a0()?, a1()?)(w),
            SqlFn::CurrentUserFn => {
                w.append("current_user");
                Ok(())
            }

            // ---- temporal
            SqlFn::Now => {
                w.append("CAST(current_timestamp AS TIMESTAMP(6))");
                Ok(())
            }
            SqlFn::DateTruncDay => {
                w.append("CAST(").expr(self, a0()?, 0)?.append(" AS DATE)");
                Ok(())
            }
            SqlFn::DateTrunc => self.date_trunc(w, a),
            SqlFn::ParseDate => {
                w.append("CAST(").expr(self, a0()?, 0)?.append(" AS TIMESTAMP(6))");
                Ok(())
            }
            SqlFn::Extract => self.extract(a)?(w),
            SqlFn::MakeDate => {
                w.append("date_add('day', ").expr(self, arg(a, 2)?, 0)?.append(" - 1, date_add('month', ");
                w.expr(self, a1()?, 0)?.append(" - 1, date_add('year', ").expr(self, a0()?, 0)?;
                w.append(" - 1970, DATE '1970-01-01')))");
                Ok(())
            }
            SqlFn::MakeTimestamp => self.make_timestamp(w, a),
            SqlFn::AddInterval | SqlFn::AddIntervalTemporal => self.add_interval(w, a),
            SqlFn::DateDiff => self.date_diff(a)?(w),
            SqlFn::TimeBucket => self.time_bucket(a)?(w),
            SqlFn::Timezone => self.timezone(w, a),
            // Trino adds no date and time: the date-time is read from their texts
            SqlFn::DateAtTime => {
                w.append("CAST(CAST(").expr(self, a0()?, 0)?.append(" AS VARCHAR) || ' ' || CAST(");
                w.expr(self, a1()?, 0)?.append(" AS VARCHAR) AS TIMESTAMP(6))");
                Ok(())
            }
            SqlFn::EpochSeconds => {
                w.append("to_unixtime(with_timezone(").expr(self, a0()?, 0)?.append(", 'UTC'))");
                Ok(())
            }
            SqlFn::EpochNs | SqlFn::FromEpochNs => Err(Refusal::capability(
                "a nanosecond epoch: Trino keeps microseconds (the Cypher island's nanosecond leg is DuckDB's)",
            )),
            SqlFn::EpochMs => {
                w.append("CAST(truncate(to_unixtime(with_timezone(").expr(self, a0()?, 0)?;
                w.append(", 'UTC')) * 1000) AS BIGINT)");
                Ok(())
            }
            SqlFn::FromEpochSeconds => {
                w.append("CAST(from_unixtime(").expr(self, a0()?, 0)?.append(", 'UTC') AS TIMESTAMP(6))");
                Ok(())
            }
            SqlFn::FromEpochMs => {
                w.append("CAST(from_unixtime(CAST(").expr(self, a0()?, 0)?;
                w.append(" AS DOUBLE) / 1000, 'UTC') AS TIMESTAMP(6))");
                Ok(())
            }
            SqlFn::Dayname => {
                w.append("format_datetime(").expr(self, a0()?, 0)?.append(", 'EEEE')");
                Ok(())
            }
            SqlFn::Monthname => {
                w.append("format_datetime(").expr(self, a0()?, 0)?.append(", 'MMMM')");
                Ok(())
            }
            SqlFn::Strftime => self.strftime(w, a),
            SqlFn::Strptime => self.strptime(w, a),

            // ---- lists whose first argument is no list
            SqlFn::ListLength => {
                w.append("cardinality(").piece(&*self.list(a0()?))?.append(")");
                Ok(())
            }
            // DuckDB's range: [start, stop) by step
            SqlFn::RangeFn => self.range_of(w, a),
            SqlFn::RepeatValue => {
                w.append("repeat(").expr(self, a0()?, 0)?.append(", greatest(").expr(self, a1()?, 0)?.append(", 0))");
                Ok(())
            }

            // ---- maps
            SqlFn::MapEmpty => {
                w.append("MAP()");
                Ok(())
            }
            SqlFn::MapFromLists => {
                w.append("map(").list(self, a)?.append(")");
                Ok(())
            }
            // DuckDB's map_extract: a list of the key's value, empty when absent
            SqlFn::MapExtract => {
                w.append("(CASE WHEN contains(map_keys(").expr(self, a0()?, 0)?.append("), ").expr(self, a1()?, 0)?;
                w.append(") THEN ARRAY[element_at(").expr(self, a0()?, 0)?.append(", ").expr(self, a1()?, 0)?;
                w.append(")] ELSE ARRAY[] END)");
                Ok(())
            }
            // a duplicate key keeps its LAST value, as DuckDB's
            SqlFn::MapConcat => {
                w.append("map_concat(").list(self, a)?.append(")");
                Ok(())
            }

            // ---- variant
            SqlFn::JsonType => self.json_type(w, a),
            SqlFn::JsonArrayLength => {
                w.append("json_array_length(").expr(self, a0()?, 0)?.append(")");
                Ok(())
            }
            SqlFn::JsonPretty => Err(wall(f, "Trino has no JSON pretty-printer")),
            SqlFn::JsonMergePatch => Err(wall(f, "Trino has no JSON merge patch")),
        }
    }

    /// The list calls (Java's `listCall`).
    pub(super) fn list_call_arm(&self, w: &mut SqlWriter, f: SqlFn, args: &[SqlExpr]) -> Result<(), Refusal> {
        let xs = arg(args, 0)?;
        let list = self.list(xs);
        match f {
            // a lambda over the element and its 1-based index (DuckDB's): zip_with binds both, over the positions
            SqlFn::ListFilter => {
                if lambda_arg(arg(args, 1)?)?.params.len() > 1 {
                    w.append("transform(filter(zip(").piece(&*list)?.append(", zip_with(").piece(&*list)?;
                    w.append(", ").piece(&*self.positions(xs))?.append(", ").expr(self, &args[1], 0)?;
                    w.append(")), \"__z\" -> \"__z\"[2]), \"__z\" -> \"__z\"[1])");
                } else {
                    w.append("filter(").piece(&*list)?.append(", ").expr(self, &args[1], 0)?.append(")");
                }
            }
            SqlFn::ListTransform => {
                if lambda_arg(arg(args, 1)?)?.params.len() > 1 {
                    w.append("zip_with(").piece(&*list)?.append(", ").piece(&*self.positions(xs))?.append(", ");
                    w.expr(self, &args[1], 0)?.append(")");
                } else {
                    w.append("transform(").piece(&*list)?.append(", ").expr(self, &args[1], 0)?.append(")");
                }
            }
            // a NULL or empty literal list adds nothing, and an untyped one leaves concat no candidate: only the
            // others are concatenated
            SqlFn::ListConcat => {
                let parts: Vec<&SqlExpr> = args
                    .iter()
                    .filter(|a| {
                        !matches!(a, SqlExpr::NullLit { .. })
                            && !matches!(a, SqlExpr::ArrayLit { elements, .. } if elements.is_empty())
                    })
                    .collect();
                if parts.is_empty() {
                    w.append("ARRAY[]");
                } else if parts.len() == 1 {
                    w.append("coalesce(").piece(&*self.list(parts[0]))?.append(", ARRAY[])");
                } else {
                    w.append("concat(").join(&parts, ", ", |w, a| {
                        w.append("coalesce(").piece(&*self.list(a))?.append(", ARRAY[])");
                        Ok(())
                    })?;
                    w.append(")");
                }
            }
            // 1-based, negative from the end, out of range NULL; 0 is out of range
            SqlFn::ListGet => {
                w.append("(CASE WHEN ").expr(self, arg(args, 1)?, 0)?.append(" = 0 THEN NULL ELSE element_at(");
                w.piece(&*list)?.append(", ").expr(self, &args[1], 0)?.append(") END)");
            }
            // DuckDB's list_position: NULL when absent (array_position answers 0)
            SqlFn::ListPosition => {
                w.append("(CASE WHEN array_position(").piece(&*list)?.append(", ").expr(self, arg(args, 1)?, 0)?;
                w.append(") = 0 THEN NULL ELSE array_position(").piece(&*list)?.append(", ").expr(self, &args[1], 0)?;
                w.append(") END)");
            }
            // DuckDB's list_distinct drops NULLs; array_distinct keeps the first occurrence's order
            SqlFn::ListDistinct => {
                w.append("array_distinct(").piece(&*self.present(xs))?.append(")");
            }
            SqlFn::ListAppend => {
                w.append("concat(").piece(&*list)?.append(", ARRAY[").expr(self, arg(args, 1)?, 0)?.append("])");
            }
            SqlFn::ListSum => {
                w.piece(&*self.over_rows(xs, AggFn::Sum, &[]))?;
            }
            // array_min and array_max are NULL when an element is: the present elements
            SqlFn::ListMin => {
                w.append("array_min(").piece(&*self.present(xs))?.append(")");
            }
            SqlFn::ListMax => {
                w.append("array_max(").piece(&*self.present(xs))?.append(")");
            }
            SqlFn::ListAvg => {
                w.append("CAST(").piece(&*self.over_rows(xs, AggFn::Avg, &[]))?.append(" AS DOUBLE)");
            }
            SqlFn::ListMedian => {
                w.piece(&*self.over_rows(xs, AggFn::Median, &[]))?;
            }
            SqlFn::ListMode => {
                w.piece(&*self.over_rows(xs, AggFn::Mode, &[]))?;
            }
            SqlFn::ListBoolAnd => {
                w.piece(&*self.over_rows(xs, AggFn::BoolAnd, &[]))?;
            }
            SqlFn::ListBoolOr => {
                w.piece(&*self.over_rows(xs, AggFn::BoolOr, &[]))?;
            }
            SqlFn::ListProduct => {
                w.append("reduce(").piece(&*list)?.append(", CAST(1 AS DOUBLE), ");
                w.append("(\"__a\", \"__x\") -> \"__a\" * coalesce(\"__x\", 1), \"__a\" -> \"__a\")");
            }
            // array_sort: ascending, NULLs last; descending, NULLs last
            SqlFn::ListSort => {
                w.append("array_sort(").piece(&*list)?.append(")");
            }
            SqlFn::ListSortDesc => {
                w.append("concat(reverse(array_sort(").piece(&*self.present(xs))?.append(")), filter(");
                w.piece(&*list)?.append(", \"__x\" -> \"__x\" IS NULL))");
            }
            SqlFn::ListReverse => {
                w.append("reverse(").piece(&*list)?.append(")");
            }
            SqlFn::ListTail => {
                w.append("slice(").piece(&*list)?.append(", 2, greatest(cardinality(").piece(&*list)?;
                w.append(") - 1, 0))");
            }
            SqlFn::ListInit => {
                w.append("slice(").piece(&*list)?.append(", 1, greatest(cardinality(").piece(&*list)?;
                w.append(") - 1, 0))");
            }
            // DuckDB's list_slice: 1-based, both ends inclusive
            SqlFn::ListSlice => {
                w.append("slice(").piece(&*list)?.append(", greatest(").expr(self, arg(args, 1)?, 0)?;
                w.append(", 1), greatest(").expr(self, arg(args, 2)?, 6)?.append(" - greatest(");
                w.expr(self, &args[1], 0)?.append(", 1) + 1, 0))");
            }
            // DuckDB's list_zip: pairs by position; truncated to the shorter list when asked, else the shorter padded
            // with NULL, as Trino's zip does
            SqlFn::ListZip => {
                let last = args.last();
                let truncate = args.len() > 2 && matches!(last, Some(SqlExpr::BoolLit { value: true, .. }));
                let lists = if args.len() > 2 && matches!(last, Some(SqlExpr::BoolLit { .. })) {
                    &args[..args.len() - 1]
                } else {
                    args
                };
                if !truncate {
                    w.append("zip(").join(lists, ", ", |w, l| {
                        w.piece(&*self.list(l))?;
                        Ok(())
                    })?;
                    w.append(")");
                } else {
                    let n = |v: &mut SqlWriter| -> Result<(), Refusal> {
                        v.append("least(").join(lists, ", ", |v, l| {
                            v.append("coalesce(cardinality(").piece(&*self.list(l))?.append("), 0)");
                            Ok(())
                        })?;
                        v.append(")");
                        Ok(())
                    };
                    w.append("zip(").join(lists, ", ", |w, l| {
                        w.append("slice(").piece(&*self.list(l))?.append(", 1, ").piece(&n)?.append(")");
                        Ok(())
                    })?;
                    w.append(")");
                }
            }
            // DuckDB's list_reduce: the first element is the accumulator's start, the lambda (acc, x) over the rest
            SqlFn::ListReduce => {
                w.append("reduce(slice(").piece(&*list)?.append(", 2, greatest(cardinality(").piece(&*list)?;
                w.append(") - 1, 0)), element_at(").piece(&*list)?.append(", 1), ").expr(self, arg(args, 1)?, 0)?;
                w.append(", \"__s\" -> \"__s\")");
            }
            // Trino's own name of the value's type, compared only with typeofName's
            SqlFn::Typeof => {
                w.function(self, "typeof", args)?;
            }
            SqlFn::RangeFn | SqlFn::RepeatValue => {
                return Err(Refusal::illegal_state(format!("{f} is rendered by call")));
            }
            _ => return Err(Refusal::illegal_state(format!("not a list call: {f}"))),
        }
        Ok(())
    }
}

/// Java's `writer.function(name, args)`.
fn function(d: &Trino, w: &mut SqlWriter, name: &str, a: &[SqlExpr]) -> Result<(), Refusal> {
    w.function(d, name, a)?;
    Ok(())
}
