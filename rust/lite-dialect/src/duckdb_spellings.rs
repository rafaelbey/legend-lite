// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! Java's `DuckDbSpellings`: DuckDB's spellings of the functions only DuckDB runs, written once for the two dialects
//! that name them (DuckDB, and the engine-H2 text the corpus reads on DuckDB too).

use crate::ansi::part;
use crate::refusal::{Refusal, arg};
use crate::renderer::AnsiSqlRenderer;
use crate::writer::SqlWriter;
use lite_sql::{DatePart, SqlExpr};

/// `~x` as `xor(x, -1)`: no negation overflow at MIN_LONG.
pub fn bit_not<D: AnsiSqlRenderer>(d: &D, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
    w.append("xor(").expr(d, arg(a, 0)?, 0)?.append(", -1)");
    Ok(())
}

pub fn map_empty(w: &mut SqlWriter) -> Result<(), Refusal> {
    w.append("MAP {}");
    Ok(())
}

pub fn json_merge_patch<D: AnsiSqlRenderer>(d: &D, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
    w.function(d, "json_merge_patch", a)?;
    Ok(())
}

pub fn encode_base64<D: AnsiSqlRenderer>(d: &D, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
    w.append("to_base64(CAST(").expr(d, arg(a, 0)?, 0)?.append(" AS BLOB))");
    Ok(())
}

/// decode(blob): a CAST of the blob to VARCHAR escapes quotes and non-printables, never the text itself.
pub fn decode_base64<D: AnsiSqlRenderer>(d: &D, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
    w.append("decode(from_base64(").expr(d, arg(a, 0)?, 0)?.append("))");
    Ok(())
}

/// make_timestamp wants DOUBLE seconds; (part, value) otherwise.
pub fn make_timestamp<D: AnsiSqlRenderer>(d: &D, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
    if a.len() == 6 {
        w.append("make_timestamp(").list(d, &a[..5])?.append(", CAST(").expr(d, &a[5], 0)?.append(" AS DOUBLE))");
    } else {
        w.function(d, "make_timestamp", a)?;
    }
    Ok(())
}

/// Week buckets align to the Monday on or before the epoch (1969-12-29); every other unit to the 1970 epoch.
pub fn time_bucket<D: AnsiSqlRenderer>(d: &D, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
    let unit = part(arg(a, 0)?)?;
    let f = interval_function(unit)?;
    w.append("time_bucket(").append(f).append("(").expr(d, arg(a, 1)?, 0)?.append("), ").expr(d, arg(a, 2)?, 0)?;
    w.append(if unit == DatePart::Week {
        ", TIMESTAMP '1969-12-29 00:00:00'"
    } else {
        ", TIMESTAMP '1970-01-01 00:00:00'"
    })
    .append(")");
    Ok(())
}

/// A date part as DuckDB's `date_part`, `date_trunc` and `date_diff` name it.
pub fn date_part(p: DatePart) -> &'static str {
    match p {
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
        DatePart::Nanosecond => "nanosecond",
        DatePart::DayOfYear => "doy",
        DatePart::IsoDayOfWeek => "isodow",
    }
}

/// The function DuckDB builds an interval of `p` with (`to_days(n)`).
pub fn interval_function(p: DatePart) -> Result<&'static str, Refusal> {
    Ok(match p {
        DatePart::Year => "to_years",
        DatePart::Month => "to_months",
        DatePart::Week => "to_weeks",
        DatePart::Day => "to_days",
        DatePart::Hour => "to_hours",
        DatePart::Minute => "to_minutes",
        DatePart::Second => "to_seconds",
        DatePart::Millisecond => "to_milliseconds",
        DatePart::Microsecond => "to_microseconds",
        DatePart::Quarter | DatePart::Nanosecond | DatePart::DayOfYear | DatePart::IsoDayOfWeek => {
            return Err(Refusal::capability(format!(
                "an interval of {p} reached DuckDB, which builds none (nanoseconds are added over the epoch: EPOCH_NS)"
            )));
        }
    })
}

pub fn from_epoch_ms<D: AnsiSqlRenderer>(d: &D, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
    w.append("epoch_ms(CAST(").expr(d, arg(a, 0)?, 0)?.append(" AS BIGINT))");
    Ok(())
}

pub fn int_divide<D: AnsiSqlRenderer>(d: &D, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
    w.append("(").expr(d, arg(a, 0)?, 6)?.append(" // ").expr(d, arg(a, 1)?, 6)?.append(")");
    Ok(())
}
