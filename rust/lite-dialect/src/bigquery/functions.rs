// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! BigQuery's scalar functions: Java's `call` -- every `SqlFn` decided by an exhaustive match -- with the helpers its
//! arms name, the casts and texts of a value (`variantAwareCast`, `castTo`, `asText`, `floatText`), the maps, the
//! regular expressions and the temporal helpers (BigQuery.java, "Variant" to "Date format codes").

use super::{
    BigQuery, P, fact_element, fact_type, is_double, is_json, is_scalar, is_text, is_type, lambda_body, lambda_of,
    list_element, list_parameter, nested_list, offset_of, pc, text, type_of, type_text,
};
use crate::ansi::{self, part};
use crate::build;
use crate::explosion_markers::null_list;
use crate::refusal::{Refusal, arg};
use crate::renderer::AnsiSqlRenderer;
use crate::writer::SqlWriter;
use lite_javacompat::character;
use lite_sql::{DateFmt, DateFmtPart, DatePart, Scalar, SqlExpr, SqlFn, SqlType, TypeFact};

fn wall(f: SqlFn, why: &str) -> Refusal {
    Refusal::capability(format!("{f} reached BigQuery: {why}"))
}

/// A string literal's value, else refused naming `what` (Java's `literal`).
fn literal<'a>(e: &'a SqlExpr, what: &str) -> Result<&'a str, Refusal> {
    match e {
        SqlExpr::StringLit { value, .. } => Ok(value),
        _ => Err(Refusal::capability(format!("{what} that is not a literal reached BigQuery"))),
    }
}

/// A regexp call's group: none past two arguments, else a literal 0 to 99 (Java's `group`).
fn group(a: &[SqlExpr]) -> Result<i64, Refusal> {
    if a.len() < 3 {
        return Ok(0);
    }
    match &a[2] {
        SqlExpr::IntLit { value, .. } if (0..=99).contains(value) => Ok(*value),
        _ => Err(Refusal::capability("a regexp group that is not a literal reached BigQuery")),
    }
}

/// `p` with only its `keep`-th capturing group capturing, every other made non-capturing (Java's `onlyGroup`).
pub(super) fn only_group(p: &str, keep: i64) -> String {
    let cs: Vec<char> = p.chars().collect();
    let starts = |i: usize, s: &str| s.chars().enumerate().all(|(k, c)| cs.get(i + k) == Some(&c));
    let mut out = String::with_capacity(p.len() + 8);
    let mut group = 0;
    let mut in_class = false;
    let mut i = 0;
    while i < cs.len() {
        let ch = cs[i];
        if ch == '\\' && i + 1 < cs.len() {
            if cs[i + 1] == 'Q' {
                let end = (i + 2..cs.len().saturating_sub(1)).find(|&k| cs[k] == '\\' && cs[k + 1] == 'E');
                let end = end.map_or(cs.len(), |e| e + 2);
                out.extend(&cs[i..end]);
                i = end;
            } else {
                out.push(ch);
                out.push(cs[i + 1]);
                i += 2;
            }
            continue;
        }
        if in_class {
            in_class = ch != ']';
            out.push(ch);
            i += 1;
            continue;
        }
        if ch == '[' {
            in_class = true;
            out.push(ch);
            if i + 1 < cs.len() && cs[i + 1] == '^' {
                i += 1;
                out.push(cs[i]);
            }
            if i + 1 < cs.len() && cs[i + 1] == ']' {
                i += 1;
                out.push(cs[i]);
            }
            i += 1;
            continue;
        }
        if ch == '(' {
            let named = starts(i, "(?P<") || starts(i, "(?<");
            if starts(i, "(?") && !named {
                out.push(ch);
                i += 1;
                continue;
            }
            group += 1;
            if group == keep {
                out.push(ch);
            } else {
                out.push_str("(?:");
                if named && let Some(close) = (i..cs.len()).find(|&k| cs[k] == '>') {
                    i = close;
                }
            }
            i += 1;
            continue;
        }
        out.push(ch);
        i += 1;
    }
    out
}

/// A replacement's group references moved past the prefix group and the pattern's own (Java's `shiftGroups`; by
/// UTF-16 unit, a digit as `Character.isDigit` takes it, its number `ch - '0'`).
pub(super) fn shift_groups(r: &str) -> Result<String, Refusal> {
    let units: Vec<u16> = r.encode_utf16().collect();
    let backslash = u16::from(b'\\');
    let mut out: Vec<u16> = Vec::with_capacity(units.len());
    let mut i = 0;
    while i < units.len() {
        let ch = units[i];
        if ch == backslash && i + 1 < units.len() && character::is_digit(units[i + 1]) {
            i += 1;
            let k = i64::from(units[i]) - i64::from(b'0') + 2;
            if k > 9 {
                return Err(Refusal::capability(format!(
                    "a first-match regexp_replace referring to group {} reached BigQuery (RE2 refers to nine)",
                    k - 2
                )));
            }
            out.push(backslash);
            out.extend(k.to_string().encode_utf16());
        } else if ch == backslash && i + 1 < units.len() {
            i += 1;
            out.push(ch);
            out.push(units[i]);
        } else {
            out.push(ch);
        }
        i += 1;
    }
    Ok(String::from_utf16_lossy(&out))
}

/// The conversions of a FORMAT template, each `%` directive's last character, in order (Java's
/// `%[-+ #0]*[0-9]*(?:\.[0-9]+)?([a-zA-Z%])` found again and again).
pub(super) fn conversions(format: &str) -> Vec<char> {
    let cs: Vec<char> = format.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < cs.len() {
        if cs[i] != '%' {
            i += 1;
            continue;
        }
        let mut j = i + 1;
        while j < cs.len() && "-+ #0".contains(cs[j]) {
            j += 1;
        }
        while j < cs.len() && cs[j].is_ascii_digit() {
            j += 1;
        }
        if j + 1 < cs.len() && cs[j] == '.' && cs[j + 1].is_ascii_digit() {
            j += 1;
            while j < cs.len() && cs[j].is_ascii_digit() {
                j += 1;
            }
        }
        if j < cs.len() && (cs[j].is_ascii_alphabetic() || cs[j] == '%') {
            out.push(cs[j]);
            i = j + 1;
        } else {
            i += 1;
        }
    }
    out
}

/// A unit as BigQuery's EXTRACT, DATE_TRUNC, DATETIME_DIFF and INTERVAL name it (Java's `unit`).
fn unit(p: DatePart) -> Result<&'static str, Refusal> {
    Ok(match p {
        DatePart::Year => "YEAR",
        DatePart::Quarter => "QUARTER",
        DatePart::Month => "MONTH",
        DatePart::Week => "WEEK",
        DatePart::Day => "DAY",
        DatePart::Hour => "HOUR",
        DatePart::Minute => "MINUTE",
        DatePart::Second => "SECOND",
        DatePart::Millisecond => "MILLISECOND",
        DatePart::Microsecond => "MICROSECOND",
        DatePart::DayOfYear | DatePart::IsoDayOfWeek | DatePart::Nanosecond => {
            return Err(Refusal::capability(format!("a {p} unit has no BigQuery spelling")));
        }
    })
}

fn interval_unit(p: DatePart) -> Result<&'static str, Refusal> {
    if p == DatePart::Quarter {
        return Err(Refusal::capability(format!("interval unit {p} has no BigQuery spelling")));
    }
    unit(p)
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

/// Whether a timestamp is an instant: TIMESTAMPTZ-typed, or a timezone() of a naive one (Java's `isInstant`).
fn is_instant(e: &SqlExpr) -> bool {
    if let SqlExpr::Call { r#fn: SqlFn::Timezone, args, .. } = e
        && args.len() == 2
    {
        return !is_instant(&args[1]);
    }
    is_scalar(e, Scalar::Timestamptz)
}

fn composite(e: &SqlExpr) -> bool {
    matches!(type_of(e), Some(SqlType::Struct { .. } | SqlType::Array { .. }))
}

/// A part's FORMAT_DATETIME code, or none for one with none (Java's `code`).
fn code(p: &DateFmt) -> Option<String> {
    Some(match p {
        DateFmt::Text { s } => s.replace('%', "%%"),
        DateFmt::Part(part) => match part {
            DateFmtPart::Year4 => "%Y",
            DateFmtPart::Month2 => "%m",
            DateFmtPart::Day2 => "%d",
            DateFmtPart::Hour2 => "%H",
            DateFmtPart::Min2 => "%M",
            DateFmtPart::Sec2 => "%S",
            DateFmtPart::MonthAbbrev => "%b",
            DateFmtPart::MonthName => "%B",
            DateFmtPart::WeekdayName => "%A",
            DateFmtPart::Hour12 => "%I",
            DateFmtPart::Ampm => "%p",
            DateFmtPart::SubsecMicro | DateFmtPart::SubsecNano | DateFmtPart::SubsecMin | DateFmtPart::Hour12Nopad => {
                return None;
            }
        }
        .to_owned(),
    })
}

/// Seconds, a point and three (six, nine) fraction digits as one `%E3S` (`%E6S`) (Java's `secondsWithFraction`).
fn seconds_with_fraction(parts: &[DateFmt], i: usize) -> Option<&'static str> {
    if parts[i] == DateFmt::Part(DateFmtPart::Sec2)
        && i + 2 < parts.len()
        && matches!(&parts[i + 1], DateFmt::Text { s } if s == ".")
    {
        return match parts[i + 2] {
            DateFmt::Part(DateFmtPart::SubsecMicro) => Some("%E6S"),
            DateFmt::Part(DateFmtPart::SubsecMin) => Some("%E3S"),
            DateFmt::Part(DateFmtPart::SubsecNano) => Some("%E6S000"),
            _ => None,
        };
    }
    None
}

/// The format for PARSE_DATETIME (and any format literal): every part a code (Java's `formatText`).
pub(super) fn format_codes(parts: &[DateFmt]) -> Result<String, Refusal> {
    let mut out = String::new();
    let mut i = 0;
    while i < parts.len() {
        if let Some(combined) = seconds_with_fraction(parts, i) {
            out.push_str(combined);
            i += 3;
            continue;
        }
        match code(&parts[i]) {
            Some(c) => out.push_str(&c),
            None => {
                let part = match &parts[i] {
                    DateFmt::Part(p) => p.to_string(),
                    DateFmt::Text { s } => format!("Text[s={s}]"),
                };
                return Err(Refusal::capability(format!(
                    "date format part {part} has no BigQuery code outside a formatted date"
                )));
            }
        }
        i += 1;
    }
    Ok(out)
}

impl BigQuery {
    // ==================================================================
    // Casts and texts
    // ==================================================================

    /// A value read out of a Variant as text: a member its scalar text, the whole value its JSON (Java's
    /// `jsonAsText`).
    fn json_as_text(&self, w: &mut SqlWriter, v: &SqlExpr, member: bool) -> Result<(), Refusal> {
        if !member {
            w.append("TO_JSON_STRING(").expr(self, v, 0)?.append(")");
            return Ok(());
        }
        w.append("(CASE WHEN JSON_TYPE(").expr(self, v, 0)?.append(") IN ('object', 'array') THEN TO_JSON_STRING(");
        w.expr(self, v, 0)?.append(") ELSE JSON_VALUE(").expr(self, v, 0)?.append(") END)");
        Ok(())
    }

    pub(super) fn variant_aware_cast_arm(
        &self,
        w: &mut SqlWriter,
        v: &SqlExpr,
        target: &SqlType,
    ) -> Result<(), Refusal> {
        let target_is = |s: Scalar| matches!(target, SqlType::Scalar(t) if *t == s);
        if target_is(Scalar::TemporalText) || target_is(Scalar::DecimalText) {
            w.expr(self, v, 0)?;
            return Ok(());
        }
        if target_is(Scalar::Json) {
            if is_json(v) {
                w.expr(self, v, 0)?;
            } else if is_text(v) {
                w.append("PARSE_JSON(").expr(self, v, 0)?.append(")");
            } else {
                w.append("TO_JSON(").expr(self, v, 0)?.append(")");
            }
            return Ok(());
        }
        if is_json(v) {
            let member = matches!(v, SqlExpr::Call { r#fn: SqlFn::VariantGet, .. });
            if target_is(Scalar::Varchar) {
                return self.json_as_text(w, v, member);
            }
            if let SqlType::Array { element } = target {
                if matches!(**element, SqlType::Array { .. }) {
                    return Err(nested_list());
                }
                let j = self.param("__j");
                let jo = self.param("__jo");
                let each: P<'_> = if matches!(**element, SqlType::Scalar(Scalar::Json)) {
                    text(j.clone())
                } else {
                    self.cast_from_text(text(format!("JSON_VALUE({j})")), element)
                };
                w.append("(CASE WHEN JSON_TYPE(").expr(self, v, 0)?.append(") = 'array' THEN ARRAY(SELECT ");
                w.piece(&*each)?.append(" FROM UNNEST(JSON_QUERY_ARRAY(").expr(self, v, 0)?.append(")) AS ");
                w.append(&j).append(" WITH OFFSET AS ").append(&jo).append(" ORDER BY ").append(&jo).append(") END)");
                return Ok(());
            }
            if let SqlType::Map { key, value } = target {
                return self.variant_as_map(w, v, key, value);
            }
            if matches!(target, SqlType::Struct { .. }) {
                return Err(Refusal::capability(format!(
                    "a cast of a Variant to {} reached BigQuery",
                    crate::java::sql_type_text(target)
                )));
            }
            let value: P<'_> = pc(move |w| {
                w.append("JSON_VALUE(").expr(self, v, 0)?.append(")");
                Ok(())
            });
            return self.cast_from_text(value, target)(w);
        }
        if let SqlType::Map { key, value } = target {
            return self.map_as(v, key, value)?(w);
        }
        if let SqlType::Decimal { scale, .. } = target {
            // DuckDB's decimal cast rounds to the scale
            w.append("ROUND(CAST(").expr(self, v, 0)?.append(&format!(" AS BIGNUMERIC), {scale})"));
            return Ok(());
        }
        if let SqlType::Array { element } = target {
            let from = list_element(v);
            if matches!(v, SqlExpr::NullLit { .. }) || from.is_none() {
                w.append("CAST(").expr(self, v, 0)?.append(" AS ");
                let t = self.bq_type(target)?;
                w.append(&t).append(")");
                return Ok(());
            }
            // element-wise: BigQuery casts between arrays of compatible element types only
            let e = self.param("__c");
            let co = self.param("__co");
            let each = self.cast_to(text(e.clone()), from, element);
            let body: P<'_> = pc(move |w| {
                w.append("ARRAY(SELECT ").piece(&*each)?.append(" FROM UNNEST(").expr(self, v, 0)?;
                w.append(&format!(") AS {e} WITH OFFSET AS {co} ORDER BY {co})"));
                Ok(())
            });
            return self.unless_null(v, body)(w);
        }
        if target_is(Scalar::Varchar) {
            return self.as_text(v)(w);
        }
        self.cast_to(self.ex(v, 0), type_of(v), target)(w)
    }

    /// `value` (of type `from`, when known) cast to `to` (Java's `castTo`).
    pub(super) fn cast_to<'a>(&'a self, value: P<'a>, from: Option<&SqlType>, to: &'a SqlType) -> P<'a> {
        let to_is = |s: Scalar| matches!(to, SqlType::Scalar(t) if *t == s);
        // a text with more fraction digits than a DATETIME holds: the fraction cut to six
        if is_type(from, Scalar::Varchar) && (to_is(Scalar::Timestamp) || to_is(Scalar::Timestamptz)) {
            return pc(move |w| {
                w.append("CAST(REGEXP_REPLACE(").piece(&*value)?.append(", ");
                let pattern = self.string_lit(r"(\.[0-9]{6})[0-9]+")?;
                let group = self.string_lit(r"\1")?;
                w.append(&pattern).append(", ").append(&group).append(") AS ");
                let t = self.bq_type(to)?;
                w.append(&t).append(")");
                Ok(())
            });
        }
        if let SqlType::Decimal { scale, .. } = to {
            let scale = *scale;
            return pc(move |w| {
                w.append("ROUND(CAST(").piece(&*value)?.append(&format!(" AS BIGNUMERIC), {scale})"));
                Ok(())
            });
        }
        if to_is(Scalar::Varchar) {
            return self.text_of(value, from.cloned());
        }
        pc(move |w| {
            w.append("CAST(").piece(&*value)?.append(" AS ");
            let t = self.bq_type(to)?;
            w.append(&t).append(")");
            Ok(())
        })
    }

    /// A Variant scalar's text cast to `to` (Java's `castFromText`).
    fn cast_from_text<'a>(&'a self, text: P<'a>, to: &'a SqlType) -> P<'a> {
        if matches!(to, SqlType::Scalar(Scalar::Varchar)) {
            return text;
        }
        self.cast_to(text, Some(&SqlType::Scalar(Scalar::Varchar)), to)
    }

    /// A value as text, as DuckDB's CAST to VARCHAR prints it (Java's `asText`).
    pub(super) fn as_text<'a>(&'a self, e: &'a SqlExpr) -> P<'a> {
        if matches!(e, SqlExpr::StringLit { .. }) {
            return self.ex(e, 0);
        }
        let t = if matches!(e, SqlExpr::FloatLit { .. }) {
            Some(SqlType::Scalar(Scalar::Double))
        } else {
            type_of(e).cloned()
        };
        self.text_of(self.ex(e, 0), t)
    }

    /// DuckDB's text of a value of type `t` (Java's `textOf`).
    fn text_of<'a>(&'a self, value: P<'a>, t: Option<SqlType>) -> P<'a> {
        match t {
            Some(SqlType::Scalar(Scalar::Double)) => self.float_text(value),
            Some(SqlType::Scalar(Scalar::Timestamp)) => pc(move |w| {
                w.append("FORMAT_DATETIME('%Y-%m-%d %H:%M:%E*S', ").piece(&*value)?.append(")");
                Ok(())
            }),
            Some(SqlType::Scalar(Scalar::Timestamptz)) => pc(move |w| {
                w.append("FORMAT_TIMESTAMP('%Y-%m-%d %H:%M:%E*S+00', ").piece(&*value)?.append(", 'UTC')");
                Ok(())
            }),
            Some(SqlType::Scalar(Scalar::Json) | SqlType::Array { .. } | SqlType::Struct { .. }) => pc(move |w| {
                w.append("TO_JSON_STRING(").piece(&*value)?.append(")");
                Ok(())
            }),
            // a decimal's text has its scale's digits
            Some(SqlType::Decimal { scale, .. }) if scale > 0 => pc(move |w| {
                let f = self.string_lit(&format!("%.{scale}f"))?;
                w.append(&format!("FORMAT({f}, ")).piece(&*value)?.append(")");
                Ok(())
            }),
            _ => pc(move |w| {
                w.append("CAST(").piece(&*value)?.append(" AS STRING)");
                Ok(())
            }),
        }
    }

    /// A Float's text as DuckDB's cast writes it: the shortest digits that read back as the same double, the value
    /// bound once in a one-row derived table (Java's `floatText`).
    fn float_text<'a>(&'a self, x: P<'a>) -> P<'a> {
        let v = self.param("__fv");
        let n = self.param("__fn");
        let e = self.param("__fe");
        let r = self.param("__fr");
        pc(move |w| {
            w.append(&format!(
                "(SELECT CASE WHEN IS_NAN({v}) OR IS_INF({v}) THEN CAST({v} AS STRING) WHEN {v} = 0 OR (ABS({v}) >= \
                 1e-4 AND ABS({v}) < 1e16) THEN (CASE WHEN STRPOS({r}, '.') = 0 THEN CONCAT({r}, '.0') ELSE {r} END) \
                 ELSE CAST({v} AS STRING) END FROM (SELECT {v}, CAST("
            ));
            w.append("CASE");
            for digits in 15..=16 {
                let round = format!("ROUND({n}, {} - {e}, 'ROUND_HALF_EVEN')", digits - 1);
                w.append(&format!(" WHEN SAFE_CAST(CAST({round} AS STRING) AS FLOAT64) = {v} THEN {round}"));
            }
            w.append(&format!(" ELSE ROUND({n}, 16 - {e}, 'ROUND_HALF_EVEN') END"));
            let zeros = self.string_lit(r"^0\.(0*)")?;
            w.append(&format!(
                " AS STRING) AS {r} FROM (SELECT {v}, {n}, CASE WHEN {n} IS NULL OR {n} = 0 THEN 0 WHEN ABS({n}) >= 1 \
                 THEN LENGTH(CAST(TRUNC(ABS({n})) AS STRING)) - 1 ELSE -1 - LENGTH(REGEXP_EXTRACT(CAST(ABS({n}) AS \
                 STRING), {zeros})) END AS {e} FROM (SELECT {v}, SAFE_CAST({v} AS BIGNUMERIC) AS {n} FROM (SELECT "
            ));
            w.piece(&*x)?.append(&format!(" AS {v})))))"));
            Ok(())
        })
    }

    // ==================================================================
    // Maps: a map is its entries in order, ARRAY<STRUCT<key, value>>
    // ==================================================================

    pub(super) fn maps(&self, w: &mut SqlWriter, f: SqlFn, a: &[SqlExpr], t: &TypeFact) -> Result<(), Refusal> {
        let e = self.param("__me");
        let o = self.param("__mo");
        let key = self.ident("key");
        let value = self.ident("value");
        match f {
            SqlFn::MapEmpty => {
                let Some(m @ SqlType::Map { .. }) = fact_type(t) else {
                    return Err(wall(f, "an empty map of no known key and value types"));
                };
                let ty = self.bq_type(m)?;
                w.append(&format!("{ty}[]"));
            }
            // each entry a struct of (key, value), by position whatever their names
            SqlFn::MapFromEntries => {
                let xs = arg(a, 0)?;
                let entry = match list_element(xs) {
                    Some(SqlType::Struct { fields }) if fields.len() == 2 => fields,
                    other => return Err(wall(f, &format!("entries of {}", type_text(other)))),
                };
                let k = format!("{e}.{}", self.ident(&entry[0].name));
                let v = format!("{e}.{}", self.ident(&entry[1].name));
                let head = format!("ARRAY(SELECT AS STRUCT {k} AS {key}, {v} AS {value} FROM UNNEST(");
                let tail = format!(") AS {e} WITH OFFSET AS {o} ORDER BY {o})");
                let then: P<'_> = pc(move |w| {
                    w.append(&head).expr(self, xs, 0)?.append(&tail);
                    Ok(())
                });
                let checked = self.keys_checked(xs, e.clone(), k, then);
                w.piece(&*self.unless_null(xs, checked))?;
            }
            // DuckDB's map(ks, vs): the keys zipped with the values by position
            SqlFn::MapFromLists => {
                let ks = arg(a, 0)?;
                let vs = arg(a, 1)?;
                let (e2, o2, key2, value2) = (e.clone(), o.clone(), key.clone(), value.clone());
                let zipped: P<'_> = pc(move |w| {
                    w.append(&format!("ARRAY(SELECT AS STRUCT {e2} AS {key2}, ")).expr(self, vs, 0)?;
                    w.append(&format!("[OFFSET({o2})] AS {value2} FROM UNNEST(")).expr(self, ks, 0)?;
                    w.append(&format!(") AS {e2} WITH OFFSET AS {o2} ORDER BY {o2})"));
                    Ok(())
                });
                w.append("(CASE WHEN ").expr(self, ks, 4)?.append(" IS NULL OR ").expr(self, vs, 4)?;
                w.append(" IS NULL THEN NULL WHEN ARRAY_LENGTH(").expr(self, ks, 0)?.append(") <> ARRAY_LENGTH(");
                w.expr(self, vs, 0)?;
                w.append(") THEN ERROR('The map key list does not align with the map value list.') ELSE ");
                w.piece(&*self.keys_checked(ks, e.clone(), e.clone(), zipped))?.append(" END)");
            }
            // the entries of every map, each key at its first place with the last map's value
            SqlFn::MapConcat => {
                let u = self.param("__mu");
                let src = self.param("__ms");
                w.append(&format!(
                    "ARRAY(SELECT AS STRUCT {u}.{key} AS {key}, ARRAY_AGG(STRUCT({u}.{value} AS v) ORDER BY {u}.{src} \
                     DESC LIMIT 1)[OFFSET(0)].v AS {value} FROM ("
                ));
                for (i, m) in a.iter().enumerate() {
                    w.append(if i > 0 { " UNION ALL " } else { "" });
                    w.append(&format!(
                        "SELECT {i} AS {src}, {e}.{key} AS {key}, {e}.{value} AS {value}, {o} AS {o} FROM UNNEST("
                    ));
                    w.expr(self, m, 0)?.append(&format!(") AS {e} WITH OFFSET AS {o}"));
                }
                w.append(&format!(
                    ") AS {u} GROUP BY {u}.{key} ORDER BY MIN({u}.{src}), ARRAY_AGG({u}.{o} ORDER BY {u}.{src} LIMIT \
                     1)[OFFSET(0)])"
                ));
            }
            SqlFn::MapKeys | SqlFn::MapValues => {
                let xs = arg(a, 0)?;
                let field = if f == SqlFn::MapKeys { key } else { value };
                let body: P<'_> = pc(move |w| {
                    w.append(&format!("ARRAY(SELECT {e}.{field} FROM UNNEST(")).expr(self, xs, 0)?;
                    w.append(&format!(") AS {e} WITH OFFSET AS {o} ORDER BY {o})"));
                    Ok(())
                });
                w.piece(&*self.unless_null(xs, body))?;
            }
            SqlFn::MapExtract => {
                let xs = arg(a, 0)?;
                let body: P<'_> = pc(move |w| {
                    w.append(&format!("ARRAY(SELECT {e}.{value} FROM UNNEST(")).expr(self, xs, 0)?;
                    w.append(&format!(") AS {e} WITH OFFSET AS {o} WHERE {e}.{key} = ")).expr(self, arg(a, 1)?, 5)?;
                    w.append(&format!(" ORDER BY {o})"));
                    Ok(())
                });
                w.piece(&*self.unless_null(xs, body))?;
            }
            other => return Err(Refusal::illegal_state(format!("not a map call: {other}"))),
        }
        Ok(())
    }

    /// `then` when the keys are each non-NULL and distinct; DuckDB's error otherwise (Java's `keysChecked`).
    fn keys_checked<'a>(&'a self, xs: &'a SqlExpr, e: String, k: String, then: P<'a>) -> P<'a> {
        pc(move |w| {
            w.append(&format!("(CASE WHEN (SELECT COUNTIF({k} IS NULL) FROM UNNEST(")).expr(self, xs, 0)?;
            w.append(&format!(
                ") AS {e}) > 0 THEN ERROR('Map keys can not be NULL.') WHEN (SELECT COUNT(DISTINCT {k}) "
            ));
            w.append("FROM UNNEST(").expr(self, xs, 0)?.append(&format!(") AS {e}) < ARRAY_LENGTH("));
            w.expr(self, xs, 0)?.append(") THEN ERROR('Map keys must be unique.') ELSE ").piece(&*then)?;
            w.append(" END)");
            Ok(())
        })
    }

    /// A map cast to another map's key and value types, entry by entry (Java's `mapAs`).
    fn map_as<'a>(&'a self, m: &'a SqlExpr, to_key: &'a SqlType, to_value: &'a SqlType) -> Result<P<'a>, Refusal> {
        let to = SqlType::Map { key: Box::new(to_key.clone()), value: Box::new(to_value.clone()) };
        if matches!(m, SqlExpr::Call { r#fn: SqlFn::MapEmpty, .. }) {
            return Ok(pc(move |w| {
                let t = self.bq_type(&to)?;
                w.append(&format!("{t}[]"));
                Ok(())
            }));
        }
        if matches!(m, SqlExpr::NullLit { .. }) {
            return Ok(pc(move |w| {
                let t = self.bq_type(&to)?;
                w.append(&format!("CAST(NULL AS {t})"));
                Ok(())
            }));
        }
        let Some(from @ SqlType::Map { key: from_key, value: from_value }) = type_of(m) else {
            return Err(Refusal::capability(format!("a cast of {} to a map reached BigQuery", type_text(type_of(m)))));
        };
        if *from == to {
            return Ok(self.ex(m, 0));
        }
        let e = self.param("__me");
        let o = self.param("__mo");
        let key = self.ident("key");
        let value = self.ident("value");
        let k = self.cast_to(text(format!("{e}.{key}")), Some(from_key), to_key);
        let v = self.cast_to(text(format!("{e}.{value}")), Some(from_value), to_value);
        let body: P<'a> = pc(move |w| {
            w.append("ARRAY(SELECT AS STRUCT ").piece(&*k)?.append(&format!(" AS {key}, ")).piece(&*v)?;
            w.append(&format!(" AS {value} FROM UNNEST(")).expr(self, m, 0)?;
            w.append(&format!(") AS {e} WITH OFFSET AS {o} ORDER BY {o})"));
            Ok(())
        });
        Ok(self.unless_null(m, body))
    }

    /// A Variant object as a map: its members, each value read as the map's value type (Java's `variantAsMap`).
    fn variant_as_map(
        &self,
        w: &mut SqlWriter,
        v: &SqlExpr,
        to_key: &SqlType,
        to_value: &SqlType,
    ) -> Result<(), Refusal> {
        let k = self.param("__mk");
        let o = self.param("__mo");
        let k2 = k.clone();
        let member: P<'_> = pc(move |w| {
            w.expr(self, v, 0)?.append(&format!("[{k2}]"));
            Ok(())
        });
        let read: P<'_> = if matches!(to_value, SqlType::Scalar(Scalar::Json)) {
            member
        } else {
            let json_value: P<'_> = pc(move |w| {
                w.append("JSON_VALUE(").piece(&*member)?.append(")");
                Ok(())
            });
            self.cast_from_text(json_value, to_value)
        };
        let key_text = self.cast_to(text(k.clone()), Some(&SqlType::Scalar(Scalar::Varchar)), to_key);
        w.append("(CASE WHEN JSON_TYPE(").expr(self, v, 0)?.append(") = 'object' THEN ARRAY(SELECT AS STRUCT ");
        let key = self.ident("key");
        let value = self.ident("value");
        w.piece(&*key_text)?
            .append(&format!(" AS {key}, "))
            .piece(&*read)?
            .append(&format!(" AS {value} FROM UNNEST("));
        w.append("JSON_KEYS(").expr(self, v, 0)?.append(&format!(", 1)) AS {k} WITH OFFSET AS {o} ORDER BY {o}) END)"));
        Ok(())
    }

    // ==================================================================
    // Scalar functions -- every SqlFn decided here
    // ==================================================================

    pub(super) fn call_arm(
        &self,
        w: &mut SqlWriter,
        f: SqlFn,
        a: &[SqlExpr],
        t: &TypeFact,
        parent_prec: i32,
    ) -> Result<(), Refusal> {
        let a0 = || arg(a, 0);
        let a1 = || arg(a, 1);
        let plain = |w: &mut SqlWriter, name: &str| -> Result<(), Refusal> {
            w.function(self, name, a)?;
            Ok(())
        };
        let wrap = |w: &mut SqlWriter, open: &str, x: &SqlExpr, close: &str| -> Result<(), Refusal> {
            w.append(open).expr(self, x, 0)?.append(close);
            Ok(())
        };
        match f {
            // the ANSI operators
            SqlFn::And
            | SqlFn::Or
            | SqlFn::Less
            | SqlFn::LessEqual
            | SqlFn::Greater
            | SqlFn::GreaterEqual
            | SqlFn::Plus
            | SqlFn::Minus
            | SqlFn::Times => ansi::infix(self, w, f, a, parent_prec),
            // = between structs or arrays: their JSON texts compared
            SqlFn::Equal | SqlFn::NotEqual => {
                if a.len() == 2 && (composite(&a[0]) || composite(&a[1])) {
                    w.append("(CASE WHEN ").expr(self, &a[0], 4)?.append(" IS NULL OR ").expr(self, &a[1], 4)?;
                    w.append(" IS NULL THEN NULL ELSE TO_JSON_STRING(").expr(self, &a[0], 0)?;
                    w.append(if f == SqlFn::Equal { ") = " } else { ") <> " }).append("TO_JSON_STRING(");
                    w.expr(self, &a[1], 0)?.append(") END)");
                    return Ok(());
                }
                ansi::infix(self, w, f, a, parent_prec)
            }
            // the shared spellings (the base's named helpers)
            SqlFn::Not => ansi::not_prefix(self, w, a, parent_prec),
            SqlFn::IsNull => ansi::is_null_test(self, w, a),
            SqlFn::IsNotNull => ansi::is_not_null_test(self, w, a),
            // a NULL composite's JSON text is 'null', no composite value's is
            SqlFn::IsDistinctFrom | SqlFn::NullSafeNotEqual | SqlFn::NullSafeEqual => {
                let equal = f == SqlFn::NullSafeEqual;
                if a.len() == 2 && (composite(&a[0]) || composite(&a[1])) {
                    w.append("(TO_JSON_STRING(").expr(self, &a[0], 0)?.append(if equal { ") = " } else { ") <> " });
                    w.append("TO_JSON_STRING(").expr(self, &a[1], 0)?.append("))");
                    return Ok(());
                }
                if equal { ansi::not_distinct_from(self, w, a) } else { ansi::distinct_from(self, w, a) }
            }
            SqlFn::Xor => ansi::xor_as_and_or(self, w, a, parent_prec),
            SqlFn::Lpad => ansi::pad_with_space(self, w, "lpad", a),
            SqlFn::Rpad => ansi::pad_with_space(self, w, "rpad", a),
            SqlFn::UcFirst => ansi::first_char_cased(self, w, "upper", a, parent_prec),
            SqlFn::LcFirst => ansi::first_char_cased(self, w, "lower", a, parent_prec),
            SqlFn::DateTruncDay => ansi::cast_as(self, w, a0()?, "DATE"),
            SqlFn::Error => ansi::raise_envelope(self, w, a, "ERROR"),
            SqlFn::FloatCanon => ansi::float_canon_by_text(self, w, a, parent_prec),
            SqlFn::FloatPrint => ansi::float_print_by_text(self, w, a, parent_prec),
            // the hooks: this dialect's own (or the base's refusal)
            SqlFn::Hash => self.hash_signed(w, a),
            SqlFn::Round => self.round_half_even(w, a),
            SqlFn::BitAnd | SqlFn::BitOr | SqlFn::BitXor | SqlFn::BitShiftLeft | SqlFn::BitShiftRight => {
                self.bit_op(w, f, a)
            }
            SqlFn::StructInsert => self.struct_insert(w, a),
            SqlFn::Unnest => self.unnest_projection(w, a),
            SqlFn::PureSplitPart => self.split_part_call(w, a),
            SqlFn::ListExists => self.list_exists(w, a),
            SqlFn::ListForAll => self.list_for_all(w, a),
            SqlFn::AllDistinct => self.all_distinct(w, a),
            SqlFn::ToVariant => self.variant_construct(w, a),
            SqlFn::VariantGet => self.variant_get(w, a),
            SqlFn::VariantElements => self.variant_elements(w, a),
            // this database's plain spellings: name(args)
            SqlFn::Abs => plain(w, "ABS"),
            SqlFn::Acos => plain(w, "ACOS"),
            SqlFn::Asin => plain(w, "ASIN"),
            SqlFn::Atan => plain(w, "ATAN"),
            SqlFn::Atan2 => plain(w, "ATAN2"),
            SqlFn::Cbrt => plain(w, "CBRT"),
            SqlFn::Chr => plain(w, "CHR"),
            // a text operand beside a JSON one is parsed
            SqlFn::Coalesce => {
                if !(a.iter().any(is_json) && a.iter().any(is_text)) {
                    return plain(w, "COALESCE");
                }
                w.append("COALESCE(").join(a, ", ", |w, e| {
                    if is_text(e) {
                        w.append("PARSE_JSON(").expr(self, e, 0)?.append(")");
                    } else {
                        w.expr(self, e, 0)?;
                    }
                    Ok(())
                })?;
                w.append(")");
                Ok(())
            }
            SqlFn::Cos => plain(w, "COS"),
            SqlFn::Cosh => plain(w, "COSH"),
            SqlFn::Cot => plain(w, "COT"),
            SqlFn::Sin => plain(w, "SIN"),
            SqlFn::Sinh => plain(w, "SINH"),
            SqlFn::Tan => plain(w, "TAN"),
            SqlFn::Tanh => plain(w, "TANH"),
            SqlFn::Lower => plain(w, "LOWER"),
            SqlFn::Upper => plain(w, "UPPER"),
            SqlFn::Ltrim => plain(w, "LTRIM"),
            SqlFn::Rtrim => plain(w, "RTRIM"),
            SqlFn::Trim => plain(w, "TRIM"),
            SqlFn::Replace => plain(w, "REPLACE"),
            SqlFn::Translate => plain(w, "TRANSLATE"),
            SqlFn::StartsWith => plain(w, "STARTS_WITH"),
            SqlFn::EndsWith => plain(w, "ENDS_WITH"),
            SqlFn::Strpos => plain(w, "STRPOS"),
            SqlFn::Split => plain(w, "SPLIT"),
            SqlFn::AsciiCode => plain(w, "ASCII"),
            SqlFn::ReverseString => plain(w, "REVERSE"),
            SqlFn::RepeatStr => plain(w, "REPEAT"),
            SqlFn::Levenshtein => plain(w, "EDIT_DISTANCE"),
            SqlFn::FloorRaw => plain(w, "FLOOR"),
            SqlFn::Matches => plain(w, "REGEXP_CONTAINS"),
            SqlFn::Substring => plain(w, "SUBSTR"),
            SqlFn::ListReverse => plain(w, "ARRAY_REVERSE"),
            SqlFn::ListLength => plain(w, "ARRAY_LENGTH"),
            SqlFn::In => {
                if a.len() == 2
                    && let SqlExpr::PlanParam { name, .. } = &a[1]
                {
                    return Err(list_parameter(name));
                }
                ansi::in_list(self, w, a, parent_prec)
            }
            // never "--x": that is a comment
            SqlFn::Negate => wrap(w, "-(", a0()?, ")"),
            // ---- arithmetic: Pure's divide is a DOUBLE division, x / 0 +-Infinity or NaN
            SqlFn::Divide => {
                w.append("IEEE_DIVIDE(CAST(").expr(self, a0()?, 0)?.append(" AS FLOAT64), CAST(").expr(
                    self,
                    a1()?,
                    0,
                )?;
                w.append(" AS FLOAT64))");
                Ok(())
            }
            SqlFn::Mod | SqlFn::Rem => self.remainder(w, f, a),
            // DuckDB's // truncates toward zero and answers NULL for a zero divisor
            SqlFn::IntDivide => {
                if a.iter().any(is_double) {
                    return Err(wall(f, "DIV takes no FLOAT64"));
                }
                w.append("(CASE WHEN ").expr(self, a1()?, 4)?.append(" = 0 THEN NULL ELSE DIV(").expr(
                    self,
                    a0()?,
                    0,
                )?;
                w.append(", ").expr(self, a1()?, 0)?.append(") END)");
                Ok(())
            }
            SqlFn::Ceiling => wrap(w, "CAST(CEIL(", a0()?, ") AS INT64)"),
            SqlFn::Floor => wrap(w, "CAST(FLOOR(", a0()?, ") AS INT64)"),
            SqlFn::Sign => wrap(w, "CAST(SIGN(", a0()?, ") AS INT64)"),
            // Pure's divide-with-scale is HALF_UP
            SqlFn::RoundHalfUp => self.round_half_up(w, a),
            // a Float, as DuckDB's answers
            SqlFn::Sqrt => wrap(w, "SQRT(CAST(", a0()?, " AS FLOAT64))"),
            SqlFn::Exp => wrap(w, "EXP(CAST(", a0()?, " AS FLOAT64))"),
            SqlFn::Ln => wrap(w, "LN(CAST(", a0()?, " AS FLOAT64))"),
            SqlFn::Log10 => wrap(w, "LOG10(CAST(", a0()?, " AS FLOAT64))"),
            SqlFn::Pow => {
                w.append("POW(CAST(").expr(self, a0()?, 0)?.append(" AS FLOAT64), CAST(").expr(self, a1()?, 0)?;
                w.append(" AS FLOAT64))");
                Ok(())
            }
            SqlFn::Pi => {
                w.append("ACOS(-1)");
                Ok(())
            }
            // no DEGREES/RADIANS: DuckDB's, x * (180 / pi), x * (pi / 180)
            SqlFn::Degrees => wrap(w, "(CAST(", a0()?, " AS FLOAT64) * (180 / ACOS(-1)))"),
            SqlFn::Radians => wrap(w, "(CAST(", a0()?, " AS FLOAT64) * (ACOS(-1) / 180))"),
            SqlFn::BitNot => wrap(w, "(~ CAST(", a0()?, " AS INT64))"),
            SqlFn::ParseInt => wrap(w, "CAST(", a0()?, " AS INT64)"),
            SqlFn::BoolToText => wrap(w, "CAST(", a0()?, " AS STRING)"),
            // NULL-skipping, as DuckDB's: only all-NULL is NULL
            SqlFn::Greatest => self.null_skipping(w, "GREATEST", a),
            SqlFn::Least => self.null_skipping(w, "LEAST", a),
            // ---- strings: NULL-ignoring, as DuckDB's concat
            SqlFn::Concat => {
                w.append("CONCAT(").join(&ansi::flatten_concat(a), ", ", |w, x| {
                    w.append("IFNULL(").piece(&*self.as_text(x))?.append(", '')");
                    Ok(())
                })?;
                w.append(")");
                Ok(())
            }
            SqlFn::ConcatJoin => {
                w.append("CONCAT(").join(a, ", ", |w, x| {
                    w.append("IFNULL(").piece(&*self.as_text(x))?.append(", '')");
                    Ok(())
                })?;
                w.append(")");
                Ok(())
            }
            SqlFn::Length => {
                w.append("LENGTH(").piece(&*self.as_text(a0()?))?.append(")");
                Ok(())
            }
            // a negative count is all but that many
            SqlFn::Left | SqlFn::Right => self.left_right(w, f, a),
            // DuckDB's split_part: 1-based, negative from the end, out of range ''
            SqlFn::SplitPart => self.split_part(w, a),
            SqlFn::RegexpFullMatch => {
                w.append("REGEXP_CONTAINS(").expr(self, a0()?, 0)?.append(", ");
                let anchored = self.anchored(a1()?)?;
                w.piece(&*anchored)?.append(")");
                Ok(())
            }
            SqlFn::RegexpExtract => {
                let pattern = only_group(literal(a1()?, "a regexp_extract pattern")?, group(a)?);
                let lit = self.string_lit(&pattern)?;
                w.append("(CASE WHEN ")
                    .expr(self, a0()?, 4)?
                    .append(" IS NULL THEN NULL ELSE COALESCE(REGEXP_EXTRACT(");
                w.expr(self, a0()?, 0)?.append(&format!(", {lit}), '') END)"));
                Ok(())
            }
            SqlFn::RegexpExtractAll => self.regexp_extract_all(w, a),
            SqlFn::RegexpReplace => self.regexp_replace(w, a),
            SqlFn::EncodeBase64 => {
                w.append("TO_BASE64(CAST(").piece(&*self.as_text(a0()?))?.append(" AS BYTES))");
                Ok(())
            }
            // invalid UTF-8 raises, as DuckDB's decode does
            SqlFn::DecodeBase64 => wrap(w, "CAST(FROM_BASE64(", a0()?, ") AS STRING)"),
            SqlFn::Md5 | SqlFn::Sha1 | SqlFn::Sha256 => {
                let name = match f {
                    SqlFn::Md5 => "MD5",
                    SqlFn::Sha1 => "SHA1",
                    _ => "SHA256",
                };
                w.append(&format!("TO_HEX({name}(")).piece(&*self.as_text(a0()?))?.append("))");
                Ok(())
            }
            SqlFn::Guid => {
                w.append("GENERATE_UUID()");
                Ok(())
            }
            SqlFn::Random => {
                w.append("RAND()");
                Ok(())
            }
            // printf-like as DuckDB's: a %s argument as its text
            SqlFn::Format => self.format(w, a),
            SqlFn::JaroWinkler => Err(wall(f, "BigQuery has no Jaro-Winkler similarity")),
            SqlFn::CurrentUserFn => {
                w.append("SESSION_USER()");
                Ok(())
            }
            // ---- temporal
            SqlFn::Today => {
                w.append("CURRENT_DATE()");
                Ok(())
            }
            SqlFn::Now => {
                let zoned = is_type(fact_type(t), Scalar::Timestamptz);
                w.append(if zoned { "CURRENT_TIMESTAMP()" } else { "CURRENT_DATETIME()" });
                Ok(())
            }
            // an offset or a 'Z' converts to UTC, a naive text reads as UTC
            SqlFn::ParseDate => wrap(w, "DATETIME(CAST(", a0()?, " AS TIMESTAMP))"),
            SqlFn::Extract => self.extract(w, a),
            SqlFn::DateTrunc => self.date_trunc(w, a),
            SqlFn::MakeDate => {
                w.append("DATE(").join(a, ", ", |w, x| {
                    w.append("CAST(").expr(self, x, 0)?.append(" AS INT64)");
                    Ok(())
                })?;
                w.append(")");
                Ok(())
            }
            // seconds as a Float: whole microseconds, rounded
            SqlFn::MakeTimestamp => {
                if a.len() != 6 {
                    return Err(wall(f, &format!("make_timestamp of {} arguments", a.len())));
                }
                w.append("DATETIME_ADD(DATETIME(").join(&a[..5], ", ", |w, x| {
                    w.append("CAST(").expr(self, x, 0)?.append(" AS INT64)");
                    Ok(())
                })?;
                w.append(", 0), INTERVAL CAST(").expr(self, &a[5], 6)?.append(" * 1000000 AS INT64) MICROSECOND)");
                Ok(())
            }
            // a month past the month's end clamps; a date plus an interval is a DATETIME here
            SqlFn::AddInterval | SqlFn::AddIntervalTemporal => {
                w.append("(").expr(self, arg(a, 2)?, 5)?.append(" + INTERVAL CAST(").expr(self, a1()?, 0)?;
                w.append(" AS INT64) ").append(interval_unit(part(a0()?)?)?).append(")");
                Ok(())
            }
            SqlFn::DateDiff => self.date_diff(w, a),
            SqlFn::TimeBucket => self.time_bucket(w, a),
            // DATETIME(date, time): the date-time at that time of that day
            SqlFn::DateAtTime => plain(w, "DATETIME"),
            // DuckDB's timezone(zone, t): a naive t read in the zone; an instant to the zone's time
            SqlFn::Timezone => {
                if is_instant(a1()?) {
                    w.append("DATETIME(").expr(self, a1()?, 0)?.append(", ").expr(self, a0()?, 0)?.append(")");
                } else {
                    w.append("TIMESTAMP(CAST(").expr(self, a1()?, 0)?.append(" AS DATETIME), ").expr(self, a0()?, 0)?;
                    w.append(")");
                }
                Ok(())
            }
            // DuckDB's epoch(ts) is DOUBLE seconds; epoch_ms truncates toward zero, as DIV does
            SqlFn::EpochSeconds => {
                w.append("(CAST(UNIX_MICROS(");
                self.instant(w, a0()?)?;
                w.append(") AS FLOAT64) / 1000000)");
                Ok(())
            }
            SqlFn::EpochNs | SqlFn::FromEpochNs => Err(Refusal::capability(
                "a nanosecond epoch: BigQuery keeps microseconds (the Cypher island's nanosecond leg is DuckDB's)",
            )),
            SqlFn::EpochMs => {
                w.append("DIV(UNIX_MICROS(");
                self.instant(w, a0()?)?;
                w.append("), 1000)");
                Ok(())
            }
            SqlFn::FromEpochSeconds | SqlFn::FromEpochMs => {
                let zoned = is_type(fact_type(t), Scalar::Timestamptz);
                if !zoned {
                    w.append("DATETIME(");
                }
                if f == SqlFn::FromEpochSeconds {
                    w.append("TIMESTAMP_MICROS(CAST(").expr(self, a0()?, 6)?.append(" * 1000000 AS INT64))");
                } else {
                    w.append("TIMESTAMP_MILLIS(CAST(").expr(self, a0()?, 0)?.append(" AS INT64))");
                }
                if !zoned {
                    w.append(")");
                }
                Ok(())
            }
            SqlFn::Dayname => wrap(w, "FORMAT_DATETIME('%A', CAST(", a0()?, " AS DATETIME))"),
            SqlFn::Monthname => wrap(w, "FORMAT_DATETIME('%B', CAST(", a0()?, " AS DATETIME))"),
            SqlFn::Strftime => {
                let SqlExpr::FormatLit { parts, .. } = a1()? else {
                    return Err(wall(f, "a date format that is not a format literal: its codes are DuckDB's"));
                };
                let formatted = self.strftime(a0()?, parts)?;
                w.piece(&*formatted)?;
                Ok(())
            }
            SqlFn::Strptime => {
                let SqlExpr::FormatLit { parts, .. } = a1()? else {
                    return Err(wall(f, "a date format that is not a format literal: its codes are DuckDB's"));
                };
                let lit = self.string_lit(&format_codes(parts)?)?;
                w.append(&format!("PARSE_DATETIME({lit}, ")).expr(self, a0()?, 0)?.append(")");
                Ok(())
            }
            // ---- lists (ARRAY<T>; lambdas as correlated subqueries)
            SqlFn::ListFilter => {
                let l = lambda_of(a1()?, f)?;
                let xs = a0()?;
                let x = l.params[0].clone();
                let select = text(self.param(&x));
                let r#where: P<'_> = pc(move |w| {
                    w.expr(self, &lambda_body(xs, l)?, 0)?;
                    Ok(())
                });
                w.piece(&*self.over_elements(xs, x, offset_of(l), select, Some(r#where)))?;
                Ok(())
            }
            SqlFn::ListTransform => {
                if matches!(fact_element(t), Some(SqlType::Array { .. })) {
                    return Err(nested_list());
                }
                let l = lambda_of(a1()?, f)?;
                let xs = a0()?;
                let select: P<'_> = pc(move |w| {
                    w.expr(self, &lambda_body(xs, l)?, 0)?;
                    Ok(())
                });
                w.piece(&*self.over_elements(xs, l.params[0].clone(), offset_of(l), select, None))?;
                Ok(())
            }
            // NULL ++ xs is xs, as DuckDB's list_concat
            SqlFn::ListConcat => {
                w.append("ARRAY_CONCAT(").join(a, ", ", |w, x| {
                    let element = if list_element(x).is_some() { list_element(x) } else { fact_element(t) };
                    w.append("IFNULL(").expr(self, x, 0)?.append(", ");
                    let empty = self.empty_list(element)?;
                    w.append(&empty).append(")");
                    Ok(())
                })?;
                w.append(")");
                Ok(())
            }
            // 1-based, negative from the end, 0 and out of range NULL; a NULL list's element its type's NULL
            SqlFn::ListGet => {
                let xs = a0()?;
                if null_list(xs) {
                    let null = self.null_of(fact_type(t))?;
                    w.append(&null);
                    return Ok(());
                }
                w.append("(").expr(self, xs, 0)?.append(")[SAFE_OFFSET(CASE WHEN ").expr(self, a1()?, 4)?;
                w.append(" < 0 THEN ARRAY_LENGTH(").expr(self, xs, 0)?.append(") + CAST(").expr(self, a1()?, 0)?;
                w.append(" AS INT64) ELSE CAST(").expr(self, a1()?, 0)?.append(" AS INT64) - 1 END)]");
                Ok(())
            }
            // DuckDB's list_position: 1-based, NULL when absent, a NULL needle found by IS NOT DISTINCT FROM
            SqlFn::ListPosition => {
                let o = self.param("__o");
                let e = self.param("__e");
                w.append(&format!("(SELECT MIN({o}) + 1 FROM "));
                let elements = self.elements(a0()?, "__e", "__o")?;
                w.piece(&*elements)?.append(&format!(" WHERE {e} IS NOT DISTINCT FROM ")).expr(self, a1()?, 4)?;
                w.append(")");
                Ok(())
            }
            // DuckDB's list_distinct drops NULLs; the first occurrence's order
            SqlFn::ListDistinct => {
                let xs = a0()?;
                let e = self.param("__e");
                let o = self.param("__o");
                let body: P<'_> = pc(move |w| {
                    w.append(&format!("ARRAY(SELECT {e} FROM "));
                    let elements = self.elements(xs, "__e", "__o")?;
                    w.piece(&*elements)?;
                    w.append(&format!(" WHERE {e} IS NOT NULL GROUP BY {e} ORDER BY MIN({o}))"));
                    Ok(())
                });
                w.piece(&*self.unless_null(xs, body))?;
                Ok(())
            }
            // list_append(NULL, v) is [v]
            SqlFn::ListAppend => {
                if matches!(type_of(a1()?), Some(SqlType::Array { .. })) {
                    return Err(nested_list());
                }
                w.append("ARRAY_CONCAT(IFNULL(").expr(self, a0()?, 0)?.append(", ");
                let empty = self.empty_list(list_element(a0()?))?;
                w.append(&empty).append("), [");
                w.expr(self, a1()?, 0)?.append("])");
                Ok(())
            }
            SqlFn::ListSum
            | SqlFn::ListMin
            | SqlFn::ListMax
            | SqlFn::ListAvg
            | SqlFn::ListMedian
            | SqlFn::ListMode
            | SqlFn::ListBoolAnd
            | SqlFn::ListBoolOr => {
                let reducer = match f {
                    SqlFn::ListSum => lite_sql::AggFn::Sum,
                    SqlFn::ListMin => lite_sql::AggFn::Min,
                    SqlFn::ListMax => lite_sql::AggFn::Max,
                    SqlFn::ListAvg => lite_sql::AggFn::Avg,
                    SqlFn::ListMedian => lite_sql::AggFn::Median,
                    SqlFn::ListMode => lite_sql::AggFn::Mode,
                    SqlFn::ListBoolAnd => lite_sql::AggFn::BoolAnd,
                    _ => lite_sql::AggFn::BoolOr,
                };
                let rows = self.over_rows(a0()?, reducer, &[])?;
                w.piece(&*rows)?;
                Ok(())
            }
            // ascending / descending, NULLs last both ways; a struct by its fields in declared order
            SqlFn::ListSort | SqlFn::ListSortDesc => {
                let dir = if f == SqlFn::ListSort { " ASC NULLS LAST" } else { " DESC NULLS LAST" };
                let xs = a0()?;
                let e = self.param("__e");
                let order = self.element_sort(&e, list_element(xs), dir);
                let body: P<'_> = pc(move |w| {
                    w.append(&format!("ARRAY(SELECT {e} FROM UNNEST(")).expr(self, xs, 0)?;
                    w.append(&format!(") AS {e} ORDER BY {order})"));
                    Ok(())
                });
                w.piece(&*self.unless_null(xs, body))?;
                Ok(())
            }
            SqlFn::ListTail | SqlFn::ListInit | SqlFn::ListSlice => {
                let xs = a0()?;
                let e = self.param("__e");
                let o = self.param("__o");
                let r#where: P<'_> = match f {
                    SqlFn::ListTail => text(format!("{o} > 0")),
                    SqlFn::ListInit => pc(move |w| {
                        w.append(&format!("{o} < ARRAY_LENGTH(")).expr(self, xs, 0)?.append(") - 1");
                        Ok(())
                    }),
                    _ => pc(move |w| {
                        w.append(&format!("{o} + 1 >= ")).piece(&*self.slice_end(xs, arg(a, 1)?))?;
                        w.append(&format!(" AND {o} + 1 <= ")).piece(&*self.slice_end(xs, arg(a, 2)?))?;
                        Ok(())
                    }),
                };
                w.piece(&*self.over_elements(xs, "__e".to_owned(), "__o".to_owned(), text(e), Some(r#where)))?;
                Ok(())
            }
            // DuckDB's range: [start, stop) by step; GENERATE_ARRAY is inclusive
            SqlFn::RangeFn => self.range(w, a),
            SqlFn::RepeatValue => {
                if matches!(type_of(a0()?), Some(SqlType::Array { .. })) {
                    return Err(nested_list());
                }
                let g = self.param("__g");
                w.append("ARRAY(SELECT ").expr(self, a0()?, 0)?.append(" FROM UNNEST(GENERATE_ARRAY(1, CAST(");
                w.expr(self, a1()?, 0)?.append(&format!(" AS INT64))) AS {g})"));
                Ok(())
            }
            SqlFn::ListFlatten => Err(nested_list()),
            SqlFn::ListZip => Err(wall(f, "list_zip names its struct fields by position, unmeasured")),
            SqlFn::ListProduct => Err(wall(f, "BigQuery has no product aggregate and no fold")),
            SqlFn::ListReduce => Err(wall(
                f,
                "a reduce that reads its row is a fold: BigQuery has no lambdas (a closed one is hoisted to a \
                 top-level recursive expression)",
            )),
            SqlFn::Typeof => Err(wall(f, "its type names are BigQuery's")),
            // ---- maps: entries in order, ARRAY<STRUCT<key, value>>
            SqlFn::MapFromLists
            | SqlFn::MapFromEntries
            | SqlFn::MapEmpty
            | SqlFn::MapExtract
            | SqlFn::MapKeys
            | SqlFn::MapValues
            | SqlFn::MapConcat => self.maps(w, f, a, t),
            // ---- variant: in DuckDB's json_type vocabulary
            SqlFn::JsonType => {
                let x = a0()?;
                w.append("(CASE JSON_TYPE(").expr(self, x, 0)?;
                w.append(") WHEN 'null' THEN 'NULL' WHEN 'string' THEN 'VARCHAR' WHEN 'boolean' THEN 'BOOLEAN'");
                w.append(" WHEN 'array' THEN 'ARRAY' WHEN 'object' THEN 'OBJECT' WHEN 'number' THEN CASE WHEN");
                w.append(" REGEXP_CONTAINS(TO_JSON_STRING(").expr(self, x, 0)?;
                let digits = self.string_lit("^-?[0-9]+$")?;
                w.append(&format!("), {digits}) THEN 'BIGINT' ELSE 'DOUBLE' END END)"));
                Ok(())
            }
            SqlFn::JsonArrayLength => wrap(w, "ARRAY_LENGTH(JSON_QUERY_ARRAY(", a0()?, "))"),
            SqlFn::JsonPretty => Err(wall(f, "BigQuery has no JSON pretty-printer")),
            SqlFn::JsonMergePatch => Err(wall(f, "BigQuery has no JSON merge patch")),
        }
    }

    /// Pure's mod (the divisor's sign) and rem (the dividend's) over INT64 or BIGNUMERIC (Java's `remainder`).
    fn remainder(&self, w: &mut SqlWriter, f: SqlFn, a: &[SqlExpr]) -> Result<(), Refusal> {
        let dbl = a.iter().any(is_double);
        let operand = |w: &mut SqlWriter, i: usize| -> Result<(), Refusal> {
            if dbl {
                w.append("CAST(").expr(self, arg(a, i)?, 0)?.append(" AS BIGNUMERIC)");
            } else {
                w.expr(self, arg(a, i)?, 0)?;
            }
            Ok(())
        };
        let rem = |w: &mut SqlWriter| -> Result<(), Refusal> {
            w.append("MOD(");
            operand(w, 0)?;
            w.append(", ");
            operand(w, 1)?;
            w.append(")");
            Ok(())
        };
        w.append("(CASE WHEN ");
        operand(w, 1)?;
        w.append(" = 0 THEN NULL ELSE ");
        if f == SqlFn::Rem {
            rem(w)?;
        } else {
            w.append("MOD(");
            rem(w)?;
            w.append(" + ");
            operand(w, 1)?;
            w.append(", ");
            operand(w, 1)?;
            w.append(")");
        }
        w.append(" END)");
        Ok(())
    }

    /// GREATEST/LEAST over the arguments, each replaced by the next non-NULL one when it is NULL (Java's
    /// `nullSkipping`).
    fn null_skipping(&self, w: &mut SqlWriter, name: &str, a: &[SqlExpr]) -> Result<(), Refusal> {
        if a.len() == 1 {
            w.expr(self, &a[0], 0)?;
            return Ok(());
        }
        w.append(name).append("(");
        for i in 0..a.len() {
            w.append(if i > 0 { ", COALESCE(" } else { "COALESCE(" }).expr(self, &a[i], 0)?;
            for (j, x) in a.iter().enumerate() {
                if j != i {
                    w.append(", ").expr(self, x, 0)?;
                }
            }
            w.append(")");
        }
        w.append(")");
        Ok(())
    }

    fn left_right(&self, w: &mut SqlWriter, f: SqlFn, a: &[SqlExpr]) -> Result<(), Refusal> {
        let name = if f == SqlFn::Left { "LEFT" } else { "RIGHT" };
        if let SqlExpr::IntLit { value, .. } = arg(a, 1)?
            && *value >= 0
        {
            w.append(name).append("(").expr(self, &a[0], 0)?.append(", ").expr(self, &a[1], 0)?.append(")");
            return Ok(());
        }
        w.append("(CASE WHEN ").expr(self, &a[1], 4)?.append(" < 0 THEN ");
        if f == SqlFn::Left {
            w.append("SUBSTR(").expr(self, arg(a, 0)?, 0)?.append(", 1, GREATEST(LENGTH(").expr(self, &a[0], 0)?;
            w.append(") + CAST(").expr(self, &a[1], 0)?.append(" AS INT64), 0))");
        } else {
            w.append("SUBSTR(").expr(self, arg(a, 0)?, 0)?.append(", 1 - CAST(").expr(self, &a[1], 0)?;
            w.append(" AS INT64))");
        }
        w.append(&format!(" ELSE {name}(")).expr(self, &a[0], 0)?.append(", CAST(").expr(self, &a[1], 0)?;
        w.append(" AS INT64)) END)");
        Ok(())
    }

    /// A slice bound, 1-based: a negative one counts from the end (Java's `sliceEnd`).
    fn slice_end<'a>(&'a self, xs: &'a SqlExpr, i: &'a SqlExpr) -> P<'a> {
        pc(move |w| {
            w.append("(CASE WHEN ").expr(self, i, 4)?.append(" < 0 THEN ARRAY_LENGTH(").expr(self, xs, 0)?;
            w.append(") + ").expr(self, i, 6)?.append(" + 1 ELSE ").expr(self, i, 0)?.append(" END)");
            Ok(())
        })
    }

    /// The ORDER BY of a list's elements: a struct by its fields in declared order (Java's `elementSort`).
    fn element_sort(&self, x: &str, el: Option<&SqlType>, dir: &str) -> String {
        if let Some(SqlType::Struct { fields }) = el {
            let keys: Vec<String> = fields
                .iter()
                .map(|f| self.element_sort(&format!("{x}.{}", self.ident(&f.name)), Some(&f.r#type), dir))
                .collect();
            return keys.join(", ");
        }
        format!("{x}{dir}")
    }

    fn split_part(&self, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
        let parts = |w: &mut SqlWriter| -> Result<(), Refusal> {
            w.append("SPLIT(").expr(self, arg(a, 0)?, 0)?.append(", ").expr(self, arg(a, 1)?, 0)?.append(")");
            Ok(())
        };
        w.append("(CASE WHEN ").expr(self, arg(a, 0)?, 4)?.append(" IS NULL OR ").expr(self, arg(a, 1)?, 4)?;
        w.append(" IS NULL THEN NULL ELSE COALESCE((").piece(&parts)?.append(")[SAFE_OFFSET(CASE WHEN ");
        w.expr(self, arg(a, 2)?, 4)?.append(" < 0 THEN ARRAY_LENGTH(").piece(&parts)?.append(") + CAST(");
        w.expr(self, &a[2], 0)?.append(" AS INT64) ELSE CAST(").expr(self, &a[2], 0)?;
        w.append(" AS INT64) - 1 END)], '') END)");
        Ok(())
    }

    /// DuckDB's round: a decimal half away from zero; a double by DuckDB's own algorithm (Java's `roundHalfUp`).
    fn round_half_up(&self, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
        if is_double(arg(a, 0)?) && a.len() > 1 {
            let places = |w: &mut SqlWriter| -> Result<(), Refusal> {
                w.append("CAST(").expr(self, &a[1], 0)?.append(" AS INT64)");
                Ok(())
            };
            let up = |w: &mut SqlWriter| -> Result<(), Refusal> {
                w.append("POW(10, ").piece(&places)?.append(")");
                Ok(())
            };
            let down = |w: &mut SqlWriter| -> Result<(), Refusal> {
                w.append("POW(10, -").piece(&places)?.append(")");
                Ok(())
            };
            w.append("(CASE WHEN ").piece(&places)?.append(" >= 0 THEN ROUND(").expr(self, &a[0], 6)?.append(" * ");
            w.piece(&up)?.append(") / ").piece(&up)?.append(" ELSE ROUND(").expr(self, &a[0], 6)?.append(" / ");
            w.piece(&down)?.append(") * ").piece(&down)?.append(" END)");
            return Ok(());
        }
        w.append("ROUND(").expr(self, &a[0], 0)?;
        if a.len() > 1 {
            w.append(", CAST(").expr(self, &a[1], 0)?.append(" AS INT64)");
        }
        w.append(")");
        Ok(())
    }

    fn range(&self, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
        let start = |w: &mut SqlWriter| -> Result<(), Refusal> {
            if a.len() == 1 {
                w.append("0");
            } else {
                w.append("CAST(").expr(self, &a[0], 0)?.append(" AS INT64)");
            }
            Ok(())
        };
        let last = arg(a, if a.len() == 1 { 0 } else { 1 })?;
        let stop = |w: &mut SqlWriter| -> Result<(), Refusal> {
            w.append("CAST(").expr(self, last, 0)?.append(" AS INT64)");
            Ok(())
        };
        let step = |w: &mut SqlWriter| -> Result<(), Refusal> {
            if a.len() > 2 {
                w.append("CAST(").expr(self, &a[2], 0)?.append(" AS INT64)");
            } else {
                w.append("1");
            }
            Ok(())
        };
        let g = self.param("__g");
        let go = self.param("__go");
        w.append("(CASE WHEN (").piece(&stop)?.append(" - ").piece(&start)?.append(") * ").piece(&step)?;
        w.append(&format!(" > 0 THEN ARRAY(SELECT {g} FROM UNNEST(GENERATE_ARRAY(")).piece(&start)?.append(", ");
        w.piece(&stop)?.append(", ").piece(&step)?.append(&format!(")) AS {g} WITH OFFSET AS {go} WHERE {g} <> "));
        w.piece(&stop)?.append(&format!(" ORDER BY {go}) ELSE ARRAY<INT64>[] END)"));
        Ok(())
    }

    fn regexp_extract_all(&self, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
        let g = group(a)?;
        let pattern = self.string_lit(&only_group(literal(arg(a, 1)?, "a regexp_extract_all pattern")?, g))?;
        if g == 0 {
            w.append("REGEXP_EXTRACT_ALL(").expr(self, arg(a, 0)?, 0)?.append(&format!(", {pattern})"));
            return Ok(());
        }
        // a group that took no part in a match is '', never a NULL element
        let m = self.param("__m");
        let mo = self.param("__mo");
        w.append(&format!("ARRAY(SELECT IFNULL({m}, '') FROM UNNEST(REGEXP_EXTRACT_ALL(")).expr(self, &a[0], 0)?;
        w.append(&format!(", {pattern})) AS {m} WITH OFFSET AS {mo} ORDER BY {mo})"));
        Ok(())
    }

    /// The pattern anchored at both ends (Java's `anchored`).
    fn anchored<'a>(&'a self, p: &'a SqlExpr) -> Result<P<'a>, Refusal> {
        if let SqlExpr::StringLit { value, .. } = p {
            return Ok(text(self.string_lit(&format!("^(?:{value})$"))?));
        }
        Ok(pc(move |w| {
            let open = self.string_lit("^(?:")?;
            w.append(&format!("CONCAT({open}, ")).expr(self, p, 0)?.append(", ')$')");
            Ok(())
        }))
    }

    /// DuckDB's regexp_replace(s, p, r[, options]): the FIRST match unless 'g' (Java's `regexpReplace`).
    fn regexp_replace(&self, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
        let options = if a.len() > 3 { literal(&a[3], "regexp_replace options")? } else { "" };
        let mut global = false;
        let mut caseless = false;
        for o in options.chars() {
            match o {
                'g' => global = true,
                'i' => caseless = true,
                'c' => caseless = false,
                other => {
                    return Err(Refusal::capability(format!("regexp_replace option '{other}' reached BigQuery")));
                }
            }
        }
        let flags = if caseless { "(?i)" } else { "" };
        let s = arg(a, 0)?;
        let p = arg(a, 1)?;
        if global {
            w.append("REGEXP_REPLACE(").expr(self, s, 0)?.append(", ");
            match p {
                SqlExpr::StringLit { value, .. } => {
                    let lit = self.string_lit(&format!("{flags}{value}"))?;
                    w.append(&lit);
                }
                _ if flags.is_empty() => {
                    w.expr(self, p, 0)?;
                }
                _ => {
                    w.append(&format!("CONCAT('{flags}', ")).expr(self, p, 0)?.append(")");
                }
            }
            w.append(", ").expr(self, arg(a, 2)?, 0)?.append(")");
            return Ok(());
        }
        let replacement =
            format!(r"\1{}", shift_groups(literal(arg(a, 2)?, "a first-match regexp_replace replacement")?)?);
        w.append("REGEXP_REPLACE(").expr(self, s, 0)?.append(", ");
        match p {
            SqlExpr::StringLit { value, .. } => {
                let lit = self.string_lit(&format!("^((?s:.)*?)({flags}{value})"))?;
                w.append(&lit);
            }
            _ => {
                let open = self.string_lit(&format!("^((?s:.)*?)({flags}"))?;
                w.append(&format!("CONCAT({open}, ")).expr(self, p, 0)?.append(", ')')");
            }
        }
        let lit = self.string_lit(&replacement)?;
        w.append(", ").append(&lit).append(")");
        Ok(())
    }

    /// printf: a `%s` argument as its text (Java's `format`).
    fn format(&self, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
        let SqlExpr::StringLit { value, .. } = arg(a, 0)? else {
            w.append("FORMAT(").list(self, a)?.append(")");
            return Ok(());
        };
        let mut args = a.to_vec();
        let mut at = 1;
        for conversion in conversions(value) {
            if conversion == '%' {
                continue;
            }
            if conversion == 's' && at < args.len() {
                let x = std::mem::replace(&mut args[at], build::null_lit());
                args[at] = build::cast(x, SqlType::Scalar(Scalar::Varchar));
            }
            at += 1;
        }
        w.append("FORMAT(").list(self, &args)?.append(")");
        Ok(())
    }

    // ---- temporal helpers

    /// A temporal operand as its UTC instant (Java's `instant`).
    fn instant(&self, w: &mut SqlWriter, e: &SqlExpr) -> Result<(), Refusal> {
        if is_scalar(e, Scalar::Timestamptz) {
            w.expr(self, e, 0)?;
        } else {
            w.append("TIMESTAMP(CAST(").expr(self, e, 0)?.append(" AS DATETIME))");
        }
        Ok(())
    }

    /// A date part of a date, a Pure integer (Java's `extract`).
    fn extract(&self, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
        let p = part(arg(a, 0)?)?;
        let (open, close) = match p {
            DatePart::Year
            | DatePart::Quarter
            | DatePart::Month
            | DatePart::Day
            | DatePart::Hour
            | DatePart::Minute
            | DatePart::Second => (format!("EXTRACT({} FROM ", unit(p)?), ")"),
            DatePart::Week => ("EXTRACT(ISOWEEK FROM ".to_owned(), ")"),
            DatePart::DayOfYear => ("EXTRACT(DAYOFYEAR FROM ".to_owned(), ")"),
            DatePart::IsoDayOfWeek => ("(MOD(EXTRACT(DAYOFWEEK FROM ".to_owned(), ") + 5, 7) + 1)"),
            DatePart::Millisecond | DatePart::Microsecond | DatePart::Nanosecond => {
                return Err(Refusal::capability(format!("date part {p} has no measured BigQuery spelling")));
            }
        };
        w.append(&open).append("CAST(").expr(self, arg(a, 1)?, 0)?.append(" AS DATETIME)").append(close);
        Ok(())
    }

    /// Year, quarter, month and week deliver a DATE, finer parts a DATETIME; DuckDB's week starts on Monday (Java's
    /// `dateTrunc`).
    fn date_trunc(&self, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
        let p = part(arg(a, 0)?)?;
        match p {
            DatePart::Year | DatePart::Quarter | DatePart::Month | DatePart::Week => {
                w.append("DATE_TRUNC(CAST(").expr(self, arg(a, 1)?, 0)?;
                let u = if p == DatePart::Week { "WEEK(MONDAY)" } else { unit(p)? };
                w.append(&format!(" AS DATE), {u})"));
            }
            DatePart::Day
            | DatePart::Hour
            | DatePart::Minute
            | DatePart::Second
            | DatePart::Millisecond
            | DatePart::Microsecond => {
                w.append("DATETIME_TRUNC(CAST(").expr(self, arg(a, 1)?, 0)?;
                w.append(&format!(" AS DATETIME), {})", unit(p)?));
            }
            DatePart::DayOfYear | DatePart::IsoDayOfWeek | DatePart::Nanosecond => {
                return Err(Refusal::capability(format!("date_trunc part {p} has no measured BigQuery spelling")));
            }
        }
        Ok(())
    }

    /// DuckDB's date_diff counts boundaries crossed; its week is whole days over 7, truncated (Java's `dateDiff`).
    fn date_diff(&self, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
        let p = part(arg(a, 0)?)?;
        match p {
            DatePart::Year
            | DatePart::Quarter
            | DatePart::Month
            | DatePart::Day
            | DatePart::Hour
            | DatePart::Minute
            | DatePart::Second
            | DatePart::Millisecond
            | DatePart::Microsecond => {
                w.append("DATETIME_DIFF(CAST(").expr(self, arg(a, 2)?, 0)?.append(" AS DATETIME), CAST(");
                w.expr(self, arg(a, 1)?, 0)?.append(&format!(" AS DATETIME), {})", unit(p)?));
            }
            DatePart::Week => {
                w.append("DIV(DATE_DIFF(CAST(").expr(self, arg(a, 2)?, 0)?.append(" AS DATE), CAST(");
                w.expr(self, arg(a, 1)?, 0)?.append(" AS DATE), DAY), 7)");
            }
            DatePart::DayOfYear | DatePart::IsoDayOfWeek | DatePart::Nanosecond => {
                return Err(Refusal::capability(format!("date_diff part {p} has no measured BigQuery spelling")));
            }
        }
        Ok(())
    }

    /// DuckDB's time_bucket with the base's origins: fixed units in microseconds, months and years by the month index
    /// (Java's `timeBucket`).
    fn time_bucket(&self, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
        let u = part(arg(a, 0)?)?;
        let t = |w: &mut SqlWriter| -> Result<(), Refusal> {
            w.append("CAST(").expr(self, arg(a, 2)?, 0)?.append(" AS DATETIME)");
            Ok(())
        };
        let n = |w: &mut SqlWriter| -> Result<(), Refusal> {
            w.append("CAST(").expr(self, arg(a, 1)?, 0)?.append(" AS INT64)");
            Ok(())
        };
        if let Some(micros) = bucket_micros(u) {
            let origin: i64 = if u == DatePart::Week { -3 * 86_400_000_000 } else { 0 };
            let m = |w: &mut SqlWriter| -> Result<(), Refusal> {
                w.append("(UNIX_MICROS(TIMESTAMP(").piece(&t)?.append(&format!(")) - ({origin}))"));
                Ok(())
            };
            let size = |w: &mut SqlWriter| -> Result<(), Refusal> {
                w.append("(").piece(&n)?.append(&format!(" * {micros})"));
                Ok(())
            };
            w.append("DATETIME(TIMESTAMP_MICROS(");
            floor_div(w, &m, &size)?;
            w.append(" * ").piece(&size)?.append(&format!(" + ({origin})))"));
            return Ok(());
        }
        let months = match u {
            DatePart::Month => 1,
            DatePart::Year => 12,
            other => return Err(Refusal::capability(format!("a {other} time bucket has no BigQuery spelling"))),
        };
        let k = |w: &mut SqlWriter| -> Result<(), Refusal> {
            w.append("((EXTRACT(YEAR FROM ").piece(&t)?.append(") - 1970) * 12 + EXTRACT(MONTH FROM ").piece(&t)?;
            w.append(") - 1)");
            Ok(())
        };
        let size = |w: &mut SqlWriter| -> Result<(), Refusal> {
            w.append("(").piece(&n)?.append(&format!(" * {months})"));
            Ok(())
        };
        w.append("CAST(DATE_ADD(DATE '1970-01-01', INTERVAL ");
        floor_div(w, &k, &size)?;
        w.append(" * ").piece(&size)?.append(" MONTH) AS DATETIME)");
        Ok(())
    }

    /// A date-time formatted by DuckDB's strftime parts (Java's `strftime`).
    fn strftime<'a>(&'a self, value: &'a SqlExpr, parts: &[DateFmt]) -> Result<P<'a>, Refusal> {
        let t: P<'a> = pc(move |w| {
            w.append("CAST(").expr(self, value, 0)?.append(" AS DATETIME)");
            Ok(())
        });
        let mut pieces: Vec<P<'a>> = Vec::new();
        let mut run = String::new();
        let mut i = 0;
        while i < parts.len() {
            if let Some(combined) = seconds_with_fraction(parts, i) {
                run.push_str(combined);
                i += 3;
                continue;
            }
            let p = &parts[i];
            i += 1;
            if let Some(c) = code(p) {
                run.push_str(&c);
                continue;
            }
            if !run.is_empty() {
                pieces.push(self.formatted(&run, t.clone())?);
                run.clear();
            }
            let micro = {
                let f = self.formatted("%E6S", t.clone())?;
                pc(move |w| {
                    w.append("SUBSTR(").piece(&*f)?.append(", 4)");
                    Ok(())
                })
            };
            let milli = {
                let f = self.formatted("%E3S", t.clone())?;
                pc(move |w| {
                    w.append("SUBSTR(").piece(&*f)?.append(", 4)");
                    Ok(())
                })
            };
            pieces.push(match p {
                DateFmt::Part(DateFmtPart::SubsecMicro) => micro,
                DateFmt::Part(DateFmtPart::SubsecMin) => milli,
                DateFmt::Part(DateFmtPart::SubsecNano) => pc(move |w| {
                    w.append("CONCAT(").piece(&*micro)?.append(", '000')");
                    Ok(())
                }),
                _ => {
                    let f = self.formatted("%l", t.clone())?;
                    pc(move |w| {
                        w.append("LTRIM(").piece(&*f)?.append(")");
                        Ok(())
                    })
                }
            });
        }
        if !run.is_empty() || pieces.is_empty() {
            pieces.push(self.formatted(&run, t.clone())?);
        }
        if pieces.len() == 1 {
            return Ok(pieces.pop().expect("one piece"));
        }
        Ok(pc(move |w| {
            w.append("CONCAT(").join(&pieces, ", ", |x, piece| piece(x))?.append(")");
            Ok(())
        }))
    }

    fn formatted<'a>(&'a self, format: &str, t: P<'a>) -> Result<P<'a>, Refusal> {
        let f = self.string_lit(format)?;
        Ok(pc(move |w| {
            w.append(&format!("FORMAT_DATETIME({f}, ")).piece(&*t)?.append(")");
            Ok(())
        }))
    }
}

/// `a` floor-divided by a positive `b`, in INT64 (Java's `floorDiv`).
fn floor_div(
    w: &mut SqlWriter,
    a: &dyn Fn(&mut SqlWriter) -> Result<(), Refusal>,
    b: &dyn Fn(&mut SqlWriter) -> Result<(), Refusal>,
) -> Result<(), Refusal> {
    w.append("(DIV(").piece(a)?.append(", ").piece(b)?.append(") - IF(MOD(").piece(a)?.append(", ").piece(b)?;
    w.append(") < 0, 1, 0))");
    Ok(())
}
