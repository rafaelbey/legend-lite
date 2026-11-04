// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! Databricks' scalar functions: Java's `call` -- every `SqlFn` decided by an exhaustive match -- and `listCall`, with
//! the helpers their arms name (Databricks.java, "Scalar functions -- every SqlFn decided here" to "temporal helpers").

use super::{Databricks, P, lambda_arg, one_based_index, pc, text};
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

fn wall(f: SqlFn, why: &str) -> Refusal {
    Refusal::capability(format!("{f} reached Databricks: {why}"))
}

/// A unit as timestampadd names it (Java's `intervalUnit`).
fn interval_unit(unit: DatePart) -> Result<&'static str, Refusal> {
    Ok(match unit {
        DatePart::Year => "YEAR",
        DatePart::Month => "MONTH",
        DatePart::Week => "WEEK",
        DatePart::Day => "DAY",
        DatePart::Hour => "HOUR",
        DatePart::Minute => "MINUTE",
        DatePart::Second => "SECOND",
        DatePart::Millisecond => "MILLISECOND",
        DatePart::Microsecond => "MICROSECOND",
        DatePart::Quarter | DatePart::Nanosecond | DatePart::DayOfYear | DatePart::IsoDayOfWeek => {
            return Err(Refusal::capability(format!("interval unit {unit} has no Databricks spelling")));
        }
    })
}

/// A unit as date_trunc names it (Java's `truncUnit`).
fn trunc_unit(unit: DatePart) -> Result<&'static str, Refusal> {
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
        DatePart::DayOfYear | DatePart::IsoDayOfWeek | DatePart::Nanosecond => {
            return Err(Refusal::capability(format!("date_trunc to {unit} has no Databricks spelling")));
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

/// DuckDB's replacement text (`\1` a group, `$` itself) as Java's (`$1`, `\$`), each group number raised by `shift`
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

/// The floored epoch (in microseconds per unit) of `to` less `from`'s (Java's `epochBoundaries`).
fn epoch_boundaries<'a>(from: P<'a>, to: P<'a>, per_unit: i64) -> P<'a> {
    pc(move |w| {
        w.append("(floor(unix_micros(CAST(").piece(&*to)?.append(&format!(" AS TIMESTAMP)) / {per_unit}) - floor("));
        w.append("unix_micros(CAST(").piece(&*from)?.append(&format!(" AS TIMESTAMP)) / {per_unit}))"));
        Ok(())
    })
}

/// Jaro-Winkler similarity, exactly DuckDB's (Java's `jaroWinkler`): Databricks has no Jaro-Winkler function.
fn jaro_winkler<'a>(d: &'a Databricks, p: &'a SqlExpr, t: &'a SqlExpr) -> P<'a> {
    let o = "`__jo`";
    let s = "`__js`";
    let u = "`__ju`";
    let v = "`__jv`";
    let j = "`__jj`";
    let i = "`__ji`";
    let lp = format!("length({o}.p)");
    let lt = format!("length({o}.t)");
    let window = format!("greatest(0, greatest({lp}, {lt}) div 2 - 1)");
    let m = format!("CAST({v}.n AS DOUBLE)");
    let jaro = format!(
        "(({m} / CAST({lp} AS DOUBLE) + {m} / CAST({lt} AS DOUBLE) + ({m} - CAST({v}.x div 2 AS DOUBLE)) / {m}) / \
         CAST(3 AS DOUBLE))"
    );
    let prefix: Vec<String> = (1..=4)
        .map(|k| {
            format!("CASE WHEN {k} <= least({lp}, {lt}) AND left({o}.p, {k}) = left({o}.t, {k}) THEN 1 ELSE 0 END")
        })
        .collect();
    let score = format!(
        "CASE WHEN {v}.n = 0 THEN CAST(0 AS DOUBLE) WHEN {jaro} > CAST(0.7 AS DOUBLE) THEN {jaro} + CAST({} AS DOUBLE) \
         * CAST(0.1 AS DOUBLE) * (CAST(1 AS DOUBLE) - {jaro}) ELSE {jaro} END",
        prefix.join(" + ")
    );
    let take = format!(
        "aggregate(sequence(greatest(1, {j} - {window}), {j} + {window}), named_struct('f', {s}.f, 'tm', {s}.tm, 'k', \
         false), ({u}, {i}) -> CASE WHEN NOT {u}.k AND substr({u}.f, {i}, 1) = '0' AND substr({o}.p, {i}, 1) = \
         substr({o}.t, {j}, 1) THEN named_struct('f', overlay({u}.f placing '1' from {i} for 1), 'tm', concat({u}.tm, \
         substr({o}.t, {j}, 1)), 'k', true) ELSE {u} END, {u} -> named_struct('f', {u}.f, 'tm', {u}.tm))"
    );
    let counted = format!(
        "aggregate(sequence(1, {lp}), named_struct('n', 0, 'x', 0), ({v}, {i}) -> CASE WHEN substr({s}.f, {i}, 1) = \
         '1' THEN named_struct('n', {v}.n + 1, 'x', {v}.x + CASE WHEN substr({o}.p, {i}, 1) = substr({s}.tm, {v}.n + \
         1, 1) THEN 0 ELSE 1 END) ELSE {v} END, {v} -> {score})"
    );
    let body = format!(
        "CASE WHEN {o}.p IS NULL OR {o}.t IS NULL THEN CAST(NULL AS DOUBLE) WHEN {lp} = 0 OR {lt} = 0 THEN CAST(0 AS \
         DOUBLE) ELSE aggregate(sequence(1, {lt}), named_struct('f', repeat('0', {lp}), 'tm', ''), ({s}, {j}) -> \
         {take}, {s} -> {counted}) END"
    );
    pc(move |w| {
        w.append("element_at(transform(array(named_struct('p', ").expr(d, p, 0)?.append(", 't', ").expr(d, t, 0)?;
        w.append(&format!(")), {o} -> {body}), 1)"));
        Ok(())
    })
}

impl Databricks {
    /// A CONCAT argument as text: Spark's concat_ws takes strings only (Java's `asText`).
    fn as_text(&self, w: &mut SqlWriter, e: &SqlExpr) -> Result<(), Refusal> {
        if super::is_text(e) || matches!(e, SqlExpr::StringLit { .. }) {
            w.expr(self, e, 0)?;
        } else {
            w.append("CAST(").expr(self, e, 0)?.append(" AS STRING)");
        }
        Ok(())
    }

    /// A regexp extraction's group: the named one, else 0 (Java's `group`).
    fn group<'a>(&'a self, a: &'a [SqlExpr]) -> P<'a> {
        if a.len() > 2 {
            return pc(move |w| {
                w.append("CAST(").expr(self, &a[2], 0)?.append(" AS INT)");
                Ok(())
            });
        }
        text("0")
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

    /// A date part as Spark's extract field; a Pure integer (Java's `extract`).
    fn extract<'a>(&'a self, a: &'a [SqlExpr]) -> Result<P<'a>, Refusal> {
        let part = part(arg(a, 0)?)?;
        let field = match part {
            DatePart::Year => "YEAR",
            DatePart::Quarter => "QUARTER",
            DatePart::Month => "MONTH",
            DatePart::Week => "WEEK",
            DatePart::Day => "DAY",
            DatePart::Hour => "HOUR",
            DatePart::Minute => "MINUTE",
            DatePart::Second => "SECOND",
            DatePart::DayOfYear => "DOY",
            DatePart::IsoDayOfWeek => "DOW_ISO",
            DatePart::Millisecond | DatePart::Microsecond | DatePart::Nanosecond => {
                return Err(Refusal::capability(format!("date part {part} has no probed Databricks spelling")));
            }
        };
        Ok(pc(move |w| {
            w.append(&format!("CAST(floor(extract({field} FROM ")).expr(self, arg(a, 1)?, 0)?.append(")) AS BIGINT)");
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
            DatePart::Day => {
                let (from, to) = (from.clone(), to.clone());
                pc(move |w| {
                    w.append("datediff(CAST(").piece(&*to)?.append(" AS DATE), CAST(").piece(&*from)?;
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
                    w.append("(unix_micros(CAST(").piece(&*to)?.append(" AS TIMESTAMP)) - unix_micros(CAST(");
                    w.piece(&*from)?.append(" AS TIMESTAMP)))");
                    Ok(())
                })
            }
            DatePart::Week | DatePart::Nanosecond | DatePart::DayOfYear | DatePart::IsoDayOfWeek => {
                return Err(Refusal::capability(format!("date_diff part {part} has no probed Databricks spelling")));
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
            w.append("CAST(").expr(self, arg(a, 2)?, 0)?.append(" AS TIMESTAMP)");
            Ok(())
        });
        let n: P<'a> = pc(move |w| {
            w.append("CAST(").expr(self, arg(a, 1)?, 0)?.append(" AS BIGINT)");
            Ok(())
        });
        if let Some(per) = bucket_micros(unit) {
            let origin: i64 = if unit == DatePart::Week { -3 * 86_400_000_000 } else { 0 };
            return Ok(pc(move |w| {
                w.append("timestamp_micros(CAST(floor((unix_micros(").piece(&*t)?;
                w.append(&format!(") - ({origin})) / (")).piece(&*n)?;
                w.append(&format!(" * {per})) AS BIGINT) * ")).piece(&*n)?;
                w.append(&format!(" * {per} + ({origin}))"));
                Ok(())
            }));
        }
        let months = match unit {
            DatePart::Month => 1,
            DatePart::Year => 12,
            _ => return Err(Refusal::capability(format!("a {unit} time bucket has no Databricks spelling"))),
        };
        let size: P<'a> = {
            let n = n.clone();
            pc(move |w| {
                w.append("(").piece(&*n)?.append(&format!(" * {months})"));
                Ok(())
            })
        };
        Ok(pc(move |w| {
            w.append("CAST(add_months(DATE '1970-01-01', CAST(floor(((year(").piece(&*t)?;
            w.append(") - 1970) * 12 + month(").piece(&*t)?.append(") - 1) / ").piece(&*size)?.append(") * ");
            w.piece(&*size)?.append(" AS INT)) AS TIMESTAMP)");
            Ok(())
        }))
    }

    /// The `REGEXP_REPLACE` arm: DuckDB replaces the first match unless 'g'; Spark replaces every one.
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

    /// The `TRIM`, `LTRIM`, `RTRIM` arm: Spark's two-argument trims take the characters FIRST, so the standard form
    /// says which is which.
    fn trim(&self, w: &mut SqlWriter, f: SqlFn, a: &[SqlExpr]) -> Result<(), Refusal> {
        let name = match f {
            SqlFn::Trim => "trim",
            SqlFn::Ltrim => "ltrim",
            _ => "rtrim",
        };
        if a.len() == 1 {
            w.append(name).append("(").expr(self, &a[0], 0)?.append(")");
            return Ok(());
        }
        let side = match f {
            SqlFn::Trim => "BOTH",
            SqlFn::Ltrim => "LEADING",
            _ => "TRAILING",
        };
        w.append(&format!("TRIM({side} ")).expr(self, arg(a, 1)?, 0)?.append(" FROM ").expr(self, arg(a, 0)?, 0)?;
        w.append(")");
        Ok(())
    }

    /// The `MAKE_TIMESTAMP` arm.
    fn make_timestamp(&self, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
        if a.len() != 6 {
            return Err(wall(SqlFn::MakeTimestamp, &format!("make_timestamp of {} arguments", a.len())));
        }
        w.append("make_timestamp(").list(self, &a[..5])?.append(", CAST(").expr(self, &a[5], 0)?;
        w.append(" AS DECIMAL(16,6)))");
        Ok(())
    }

    /// `date_format` of `value` by the format `parts` (Java's `formatRun`).
    fn format_run(&self, w: &mut SqlWriter, value: &SqlExpr, parts: Vec<DateFmt>) -> Result<(), Refusal> {
        let format = SqlExpr::FormatLit {
            parts,
            r#type: TypeFact::Typed { r#type: SqlType::Scalar(Scalar::Varchar), nullable: false },
        };
        w.append("date_format(").expr(self, value, 0)?.append(", ").expr(self, &format, 0)?.append(")");
        Ok(())
    }

    /// The `STRFTIME` arm: `yyyy` pads to four digits and signs a year past 9999, where DuckDB's `%Y` and Pure write
    /// it bare: the year is formatted alone with that sign removed, each other run of the format as it is, in order.
    fn strftime(&self, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
        let SqlExpr::FormatLit { parts, .. } = arg(a, 1)? else {
            return Err(wall(SqlFn::Strftime, "a date format that is not a format literal: its codes are DuckDB's"));
        };
        let year = DateFmt::Part(DateFmtPart::Year4);
        if !parts.contains(&year) {
            w.append("date_format(").expr(self, &a[0], 0)?.append(", ").expr(self, &a[1], 0)?.append(")");
            return Ok(());
        }
        w.append("concat(");
        let mut run: Vec<DateFmt> = Vec::new();
        let mut first = true;
        for p in parts {
            if *p != year {
                run.push(p.clone());
                continue;
            }
            if !run.is_empty() {
                w.append(if first { "" } else { ", " });
                self.format_run(w, &a[0], std::mem::take(&mut run))?;
                first = false;
            }
            w.append(if first { "" } else { ", " }).append("regexp_replace(");
            self.format_run(w, &a[0], vec![p.clone()])?;
            w.append(", '^[+]', '')");
            first = false;
        }
        if !run.is_empty() {
            w.append(", ");
            self.format_run(w, &a[0], run)?;
        }
        w.append(")");
        Ok(())
    }

    /// The `STRPTIME` arm.
    fn strptime(&self, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
        if !matches!(arg(a, 1)?, SqlExpr::FormatLit { .. }) {
            return Err(wall(SqlFn::Strptime, "a date format that is not a format literal: its codes are DuckDB's"));
        }
        w.append("to_timestamp(").expr(self, &a[0], 0)?.append(", ").expr(self, &a[1], 0)?.append(")");
        Ok(())
    }

    /// The `RANGE_FN` arm: DuckDB's range, [start, stop) by step, as a filtered sequence.
    fn range_fn(&self, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
        let start: P<'_> = if a.len() == 1 {
            text("0L")
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
            text("1L")
        };
        w.append("(CASE WHEN (").piece(&*stop)?.append(" - ").piece(&*start)?.append(") * ").piece(&*step)?;
        w.append(" > 0 THEN filter(sequence(").piece(&*start)?.append(", ").piece(&*stop)?.append(", ");
        w.piece(&*step)?.append("), `__g` -> `__g` <> ").piece(&*stop)?;
        w.append(") ELSE CAST(array() AS ARRAY<BIGINT>) END)");
        Ok(())
    }

    /// The `MAP_CONCAT` arm: DuckDB's map_concat keeps a duplicate key's LAST value; Spark's refuses the duplicate:
    /// each map less the keys a later one sets, then concatenated.
    fn map_concat(&self, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
        w.append("map_concat(");
        for (i, m) in a.iter().enumerate() {
            w.append(if i > 0 { ", " } else { "" });
            if i == a.len() - 1 {
                w.expr(self, m, 0)?;
                continue;
            }
            w.append("map_filter(").expr(self, m, 0)?.append(", (`__k`, `__v`) -> NOT (");
            for (j, later) in a.iter().enumerate().skip(i + 1) {
                w.append(if j > i + 1 { " OR " } else { "" }).append("coalesce(map_contains_key(");
                w.expr(self, later, 0)?.append(", `__k`), FALSE)");
            }
            w.append("))");
        }
        w.append(")");
        Ok(())
    }

    /// The `JSON_TYPE` arm: DuckDB's json_type vocabulary.
    fn json_type(&self, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
        let x = arg(a, 0)?;
        let s = |w: &mut SqlWriter| -> Result<(), Refusal> {
            w.append("schema_of_variant(").expr(self, x, 0)?.append(")");
            Ok(())
        };
        w.append("(CASE WHEN ").expr(self, x, 0)?.append(" IS NULL THEN NULL WHEN ").piece(&s)?;
        w.append(" = 'VOID' THEN 'NULL' WHEN ").piece(&s)?.append(" = 'STRING' THEN 'VARCHAR' WHEN ");
        w.piece(&s)?.append(" = 'BOOLEAN' THEN 'BOOLEAN' WHEN startswith(").piece(&s)?;
        w.append(", 'ARRAY') THEN 'ARRAY' WHEN startswith(").piece(&s)?;
        w.append(", 'OBJECT') THEN 'OBJECT' WHEN ").piece(&s)?;
        w.append(" IN ('BIGINT', 'INT', 'SMALLINT', 'TINYINT') OR regexp_like(").piece(&s)?;
        w.append(r", '^DECIMAL\\([0-9]+,0\\)$') THEN 'BIGINT' ELSE 'DOUBLE' END)");
        Ok(())
    }

    /// Every `SqlFn`, as Databricks spells it (Java's `call`).
    pub(super) fn call_arm(
        &self,
        w: &mut SqlWriter,
        f: SqlFn,
        a: &[SqlExpr],
        t: &TypeFact,
        parent_prec: i32,
    ) -> Result<(), Refusal> {
        // over the empty collection the answer is NULL: spelled in its declared type, never Spark's VOID
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
        let function = |w: &mut SqlWriter, name: &str| -> Result<(), Refusal> {
            w.function(self, name, a)?;
            Ok(())
        };
        match f {
            // the ANSI operators
            SqlFn::And
            | SqlFn::Or
            | SqlFn::Equal
            | SqlFn::NotEqual
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
            // parseDate(text): the ISO text as a timestamp
            SqlFn::ParseDate => ansi::cast_as(self, w, a0()?, "TIMESTAMP"),
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
            SqlFn::DateTruncDay => ansi::cast_as(self, w, a0()?, "DATE"),
            // Pure's divide is a Float division: both operands DOUBLE
            SqlFn::Divide => ansi::divide_as_doubles(self, w, a),
            SqlFn::Mod => ansi::mod_floored(self, w, a),
            SqlFn::Rem => ansi::mod_truncated(self, w, a),
            // Pure's divide-with-scale is HALF_UP: plain ROUND (half away from zero)
            SqlFn::RoundHalfUp => function(w, "ROUND"),
            SqlFn::Error => ansi::raise_envelope(self, w, a, "raise_error"),
            SqlFn::DateTrunc => {
                let unit = self.string_lit(trunc_unit(part(a0()?)?)?)?;
                ansi::date_trunc_date_grained(self, w, a, &unit)
            }
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
            SqlFn::Abs => function(w, "abs"),
            SqlFn::Acos => function(w, "acos"),
            SqlFn::AsciiCode => function(w, "ascii"),
            SqlFn::Asin => function(w, "asin"),
            SqlFn::Atan => function(w, "atan"),
            SqlFn::Atan2 => function(w, "atan2"),
            SqlFn::Cbrt => function(w, "cbrt"),
            SqlFn::Chr => function(w, "chr"),
            SqlFn::Coalesce => function(w, "coalesce"),
            SqlFn::Cos => function(w, "cos"),
            SqlFn::Cosh => function(w, "cosh"),
            SqlFn::Cot => function(w, "cot"),
            SqlFn::Degrees => function(w, "degrees"),
            SqlFn::Exp => function(w, "exp"),
            SqlFn::Greatest => function(w, "greatest"),
            SqlFn::Least => function(w, "least"),
            SqlFn::Left => function(w, "left"),
            SqlFn::Length => function(w, "length"),
            SqlFn::Levenshtein => function(w, "levenshtein"),
            SqlFn::ListFlatten => function(w, "flatten"),
            SqlFn::Ln => function(w, "ln"),
            SqlFn::Log10 => function(w, "log10"),
            SqlFn::Lower => function(w, "lower"),
            SqlFn::MakeDate => function(w, "make_date"),
            SqlFn::MapKeys => function(w, "map_keys"),
            SqlFn::MapValues => function(w, "map_values"),
            SqlFn::Md5 => function(w, "md5"),
            SqlFn::Pow => function(w, "power"),
            SqlFn::Radians => function(w, "radians"),
            SqlFn::RepeatStr => function(w, "repeat"),
            SqlFn::Replace => function(w, "replace"),
            SqlFn::Translate => function(w, "translate"),
            SqlFn::ReverseString | SqlFn::ListReverse => function(w, "reverse"),
            SqlFn::Right => function(w, "right"),
            SqlFn::Sha1 => function(w, "sha1"),
            SqlFn::Sin => function(w, "sin"),
            SqlFn::Sinh => function(w, "sinh"),
            SqlFn::SplitPart => function(w, "split_part"),
            SqlFn::Sqrt => function(w, "sqrt"),
            SqlFn::Substring => function(w, "substr"),
            SqlFn::Tan => function(w, "tan"),
            SqlFn::Tanh => function(w, "tanh"),
            SqlFn::Upper => function(w, "upper"),
            SqlFn::StartsWith => function(w, "startswith"),
            SqlFn::EndsWith => function(w, "endswith"),
            SqlFn::Strpos => function(w, "instr"),
            SqlFn::MapFromLists => function(w, "map_from_arrays"),
            SqlFn::MapFromEntries => function(w, "map_from_entries"),

            // NULL-ignoring, as DuckDB's concat (Spark's concat is NULL if any argument is)
            SqlFn::Concat => {
                w.append("concat_ws('', ").join(&ansi::flatten_concat(a), ", ", |w, e| self.as_text(w, e))?.append(")");
                Ok(())
            }
            SqlFn::ConcatJoin => {
                w.append("concat_ws('', ").join(a, ", ", |w, e| self.as_text(w, e))?.append(")");
                Ok(())
            }
            SqlFn::IntDivide => {
                w.append("(").expr(self, a0()?, 6)?.append(" div ").expr(self, a1()?, 6)?.append(")");
                Ok(())
            }
            // the base spells VARCHAR, which needs a length here
            SqlFn::BoolToText => {
                w.append("CAST(").expr(self, a0()?, 0)?.append(" AS STRING)");
                Ok(())
            }
            SqlFn::BitNot => {
                w.append("(~ CAST(").expr(self, a0()?, 0)?.append(" AS BIGINT))");
                Ok(())
            }
            // floor without the BIGINT conversion (Spark's floor of a double is a BIGINT: past 2^63 it overflows)
            SqlFn::FloorRaw => {
                w.append("(CASE WHEN abs(").expr(self, a0()?, 0)?.append(") >= 4503599627370496 THEN ");
                w.expr(self, a0()?, 0)?.append(" ELSE CAST(floor(").expr(self, a0()?, 0)?.append(") AS DOUBLE) END)");
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
            // group 0 (the whole match) unless one is named: Spark's default is group 1
            SqlFn::RegexpExtract | SqlFn::RegexpExtractAll => {
                let name = if f == SqlFn::RegexpExtract { "regexp_extract(" } else { "regexp_extract_all(" };
                w.append(name).expr(self, a0()?, 0)?.append(", ").expr(self, a1()?, 0)?.append(", ");
                w.piece(&*self.group(a))?.append(")");
                Ok(())
            }
            SqlFn::RegexpReplace => self.regexp_replace(w, a),
            SqlFn::Split => {
                let split = self.split(a0()?, a1()?);
                w.piece(&*split)?;
                Ok(())
            }
            SqlFn::Trim | SqlFn::Ltrim | SqlFn::Rtrim => self.trim(w, f, a),
            // base64 without line breaks (Spark may chunk at 76 characters)
            SqlFn::EncodeBase64 => {
                w.append("replace(replace(base64(CAST(").expr(self, a0()?, 0)?;
                w.append(" AS BINARY)), chr(13), ''), chr(10), '')");
                Ok(())
            }
            SqlFn::DecodeBase64 => {
                w.append("CAST(unbase64(").expr(self, a0()?, 0)?.append(") AS STRING)");
                Ok(())
            }
            SqlFn::Sha256 => {
                w.append("sha2(").expr(self, a0()?, 0)?.append(", 256)");
                Ok(())
            }
            SqlFn::Guid => {
                w.append("uuid()");
                Ok(())
            }
            SqlFn::Random => {
                w.append("rand()");
                Ok(())
            }
            // format_string's %f takes no Spark Decimal: a Float literal is handed over as the double it is
            SqlFn::Format => {
                w.append("format_string(").join(a, ", ", |w, x| {
                    if matches!(x, SqlExpr::FloatLit { .. }) {
                        w.append("CAST(").expr(self, x, 0)?.append(" AS DOUBLE)");
                    } else {
                        w.expr(self, x, 0)?;
                    }
                    Ok(())
                })?;
                w.append(")");
                Ok(())
            }
            SqlFn::JaroWinkler => jaro_winkler(self, a0()?, a1()?)(w),
            SqlFn::CurrentUserFn => {
                w.append("current_user()");
                Ok(())
            }

            // ---- temporal
            SqlFn::Now => {
                w.append("current_timestamp()");
                Ok(())
            }
            SqlFn::Extract => self.extract(a)?(w),
            SqlFn::MakeTimestamp => self.make_timestamp(w, a),
            SqlFn::AddInterval | SqlFn::AddIntervalTemporal => {
                w.append("timestampadd(");
                let unit = interval_unit(part(a0()?)?)?;
                w.append(unit).append(", ").expr(self, a1()?, 0)?.append(", ").expr(self, arg(a, 2)?, 0)?.append(")");
                Ok(())
            }
            SqlFn::DateDiff => self.date_diff(a)?(w),
            SqlFn::TimeBucket => self.time_bucket(a)?(w),
            SqlFn::DateAtTime => Err(Refusal::capability("a time of day: Databricks has no TIME type")),
            // the zone's wall time of the instant t (the platform's timestamps are UTC instants)
            SqlFn::Timezone => {
                w.append("from_utc_timestamp(").expr(self, a1()?, 0)?.append(", ").expr(self, a0()?, 0)?.append(")");
                Ok(())
            }
            // DuckDB's epoch(ts) is DOUBLE seconds; epoch_ms truncates toward zero
            SqlFn::EpochSeconds => {
                w.append("(CAST(unix_micros(").expr(self, a0()?, 0)?.append(") AS DOUBLE) / 1000000)");
                Ok(())
            }
            SqlFn::EpochNs | SqlFn::FromEpochNs => Err(Refusal::capability(
                "a nanosecond epoch: Databricks keeps microseconds (the Cypher island's nanosecond leg is DuckDB's)",
            )),
            SqlFn::EpochMs => {
                w.append("(unix_micros(").expr(self, a0()?, 0)?.append(") div 1000)");
                Ok(())
            }
            SqlFn::FromEpochSeconds => {
                w.append("timestamp_seconds(").expr(self, a0()?, 0)?.append(")");
                Ok(())
            }
            SqlFn::FromEpochMs => {
                w.append("timestamp_millis(CAST(").expr(self, a0()?, 0)?.append(" AS BIGINT))");
                Ok(())
            }
            SqlFn::Dayname => {
                w.append("date_format(").expr(self, a0()?, 0)?.append(", 'EEEE')");
                Ok(())
            }
            SqlFn::Monthname => {
                w.append("date_format(").expr(self, a0()?, 0)?.append(", 'MMMM')");
                Ok(())
            }
            SqlFn::Strftime => self.strftime(w, a),
            SqlFn::Strptime => self.strptime(w, a),

            // ---- lists whose first argument is no list
            SqlFn::ListLength => {
                w.append("array_size(").piece(&*self.list(a0()?))?.append(")");
                Ok(())
            }
            // DuckDB's range: [start, stop) by step (start 0, step 1 by default)
            SqlFn::RangeFn => self.range_fn(w, a),
            SqlFn::RepeatValue => {
                w.append("array_repeat(").expr(self, a0()?, 0)?.append(", CAST(").expr(self, a1()?, 0)?;
                w.append(" AS INT))");
                Ok(())
            }

            // ---- maps
            SqlFn::MapEmpty => {
                w.append("map()");
                Ok(())
            }
            // DuckDB's map_extract: a list of the key's value, empty when absent
            SqlFn::MapExtract => {
                w.append("(CASE WHEN map_contains_key(").expr(self, a0()?, 0)?.append(", ").expr(self, a1()?, 0)?;
                w.append(") THEN array(element_at(").expr(self, a0()?, 0)?.append(", ").expr(self, a1()?, 0)?;
                w.append(")) ELSE array() END)");
                Ok(())
            }
            SqlFn::MapConcat => self.map_concat(w, a),

            // ---- variant
            SqlFn::JsonType => self.json_type(w, a),
            SqlFn::JsonArrayLength => {
                w.append("array_size(try_cast(").expr(self, a0()?, 0)?.append(" AS ARRAY<VARIANT>))");
                Ok(())
            }
            SqlFn::JsonPretty => Err(wall(f, "Databricks has no JSON pretty-printer")),
            SqlFn::JsonMergePatch => Err(wall(f, "Databricks has no JSON merge patch")),
        }
    }

    /// The list calls (Java's `listCall`).
    pub(super) fn list_call_arm(&self, w: &mut SqlWriter, f: SqlFn, args: &[SqlExpr]) -> Result<(), Refusal> {
        let xs = arg(args, 0)?;
        let list = self.list(xs);
        match f {
            SqlFn::ListFilter | SqlFn::ListTransform => {
                let name = if f == SqlFn::ListFilter { "filter(" } else { "transform(" };
                w.append(name).piece(&*list)?.append(", ");
                let l = arg(args, 1)?;
                match one_based_index(lambda_arg(l)?)? {
                    Some(shifted) => w.expr(self, &SqlExpr::Lambda(shifted), 0)?,
                    None => w.expr(self, l, 0)?,
                };
                w.append(")");
            }
            // NULL || xs is xs, as DuckDB's list_concat (array() is an ARRAY<VOID>, which coerces to any array type)
            SqlFn::ListConcat => {
                w.append("concat(").join(args, ", ", |w, a| {
                    w.append("coalesce(").piece(&*self.list(a))?.append(", array())");
                    Ok(())
                })?;
                w.append(")");
            }
            // 1-based, negative from the end, out of range NULL; 0 is out of range
            SqlFn::ListGet => {
                w.append("(CASE WHEN ").expr(self, arg(args, 1)?, 0)?.append(" = 0 THEN NULL ELSE try_element_at(");
                w.piece(&*list)?.append(", CAST(").expr(self, &args[1], 0)?.append(" AS INT)) END)");
            }
            // DuckDB's list_position: NULL when absent (array_position answers 0)
            SqlFn::ListPosition => {
                w.append("nullif(array_position(").piece(&*list)?.append(", ").expr(self, arg(args, 1)?, 0)?;
                w.append("), 0)");
            }
            // DuckDB's list_distinct drops NULLs; array_distinct keeps the first occurrence's order
            SqlFn::ListDistinct => {
                let x = self.param("__x")?;
                w.append("array_distinct(filter(").piece(&*list)?.append(", ").append(&x).append(" -> ").append(&x);
                w.append(" IS NOT NULL))");
            }
            SqlFn::ListAppend => {
                w.append("array_append(").piece(&*list)?.append(", ").expr(self, arg(args, 1)?, 0)?.append(")");
            }
            SqlFn::ListSum => {
                w.piece(&*self.over_rows(xs, AggFn::Sum, &[]))?;
            }
            SqlFn::ListMin => {
                w.append("array_min(").piece(&*list)?.append(")");
            }
            SqlFn::ListMax => {
                w.append("array_max(").piece(&*list)?.append(")");
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
                w.append("aggregate(").piece(&*list)?.append(", CAST(1 AS DOUBLE), ");
                w.append("(`__a`, `__x`) -> `__a` * coalesce(`__x`, 1))");
            }
            // array_sort: ascending, NULLs last; sort_array(xs, false): descending, NULLs last
            SqlFn::ListSort => {
                w.append("array_sort(").piece(&*list)?.append(")");
            }
            SqlFn::ListSortDesc => {
                w.append("sort_array(").piece(&*list)?.append(", false)");
            }
            SqlFn::ListReverse => {
                w.append("reverse(").piece(&*list)?.append(")");
            }
            SqlFn::ListTail => {
                w.append("filter(").piece(&*list)?.append(", (`__x`, `__i`) -> `__i` > 0)");
            }
            SqlFn::ListInit => {
                w.append("filter(").piece(&*list)?.append(", (`__x`, `__i`) -> `__i` < array_size(").piece(&*list)?;
                w.append(") - 1)");
            }
            // DuckDB's list_slice: 1-based, both ends inclusive
            SqlFn::ListSlice => {
                w.append("filter(").piece(&*list)?.append(", (`__x`, `__i`) -> `__i` + 1 >= ");
                w.expr(self, arg(args, 1)?, 6)?.append(" AND `__i` + 1 <= ").expr(self, arg(args, 2)?, 6)?.append(")");
            }
            SqlFn::ListZip => {
                return Err(Refusal::capability("list_zip reached Databricks: arrays_zip names its fields"));
            }
            // DuckDB's list_reduce: the first element is the accumulator's start, the lambda (acc, x) over the rest
            SqlFn::ListReduce => {
                w.append("aggregate(filter(").piece(&*list)?.append(", (`__x`, `__i`) -> `__i` > 0), ");
                w.append("element_at(").piece(&*list)?.append(", 1), ").expr(self, arg(args, 1)?, 0)?.append(")");
            }
            // Spark's own name of the value's type, compared only with typeofName's
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
