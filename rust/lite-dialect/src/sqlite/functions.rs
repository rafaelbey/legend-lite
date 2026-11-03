// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! SQLite's scalar functions: Java's `calls` -- every `SqlFn` decided by an exhaustive match -- with `lists`, `maps`
//! and the helpers their arms name (SQLite.java, "Scalar functions" to "temporal helpers").

use super::{
    P, Sqlite, digests, element_read, fact_list_element, fact_type, is_double, is_integer, json_of, json_type,
    lambda_body, lambda_of, list_element, literal, padded, param_at, pc, regex, scalar_is, text, type_of,
    type_text, wall, date_time_text, list_parameter,
};
use crate::ansi;
use crate::build;
use crate::refusal::{Refusal, arg};
use crate::renderer::AnsiSqlRenderer;
use crate::writer::SqlWriter;
use lite_sql::{AggFn, DateFmt, DateFmtPart, DatePart, Lambda, Scalar, SqlExpr, SqlFn, SqlType, TypeFact};
use std::borrow::Cow;

const UTC_ZONES: &[&str] = &["UTC", "GMT", "Etc/UTC", "Z"];

const BASE64: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

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

const DAYS: [&str; 7] = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];

/// The lower-cased name of a date part, as the refusals spell it.
fn part_name(p: DatePart) -> String {
    p.as_str().to_lowercase()
}

/// A regexp's group argument: none (0), or a literal from 0 to 99 (Java's `group`).
fn group(a: &[SqlExpr]) -> Result<i32, Refusal> {
    if a.len() < 3 {
        return Ok(0);
    }
    match &a[2] {
        SqlExpr::IntLit { value, .. } if (0..=99).contains(value) => Ok(*value as i32),
        _ => Err(Refusal::capability("a regexp group that is not a literal reached SQLite")),
    }
}

/// The JSON fields of a find's span of group `g`: its start's and end's.
fn span(g: i32) -> [String; 2] {
    if g == 0 { ["0".to_owned(), "1".to_owned()] } else { ["2".to_owned(), "3".to_owned()] }
}

/// `text`'s part from the find `f`'s span (`from` an offset into it), '' for a group that took no part.
fn part(text: &str, f: &str, at: &[String; 2], from: &str) -> String {
    format!(
        "coalesce(substr({text}, {from} + ({f} ->> {}) + 1, ({f} ->> {}) - ({f} ->> {})), '')",
        at[0], at[1], at[0]
    )
}

/// A hex digit's value, of the digit at `k` (1-based) of `hex`.
fn nibble(hex: &str, k: usize) -> String {
    format!("(instr('0123456789ABCDEF', substr({hex}, {k}, 1)) - 1)")
}

/// A fixed-length unit's length in microseconds; none for a calendar one (Java's `intervalMicros`).
fn interval_micros(unit: DatePart) -> Option<i64> {
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

/// The conversions of a printf template, in order (Java's `%[-+ #0]*[0-9]*(?:\.[0-9]+)?([a-zA-Z%])` found left to
/// right).
fn conversions(f: &str) -> Vec<char> {
    let b = f.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] != b'%' {
            i += 1;
            continue;
        }
        let mut j = i + 1;
        while j < b.len() && matches!(b[j], b'-' | b'+' | b' ' | b'#' | b'0') {
            j += 1;
        }
        while j < b.len() && b[j].is_ascii_digit() {
            j += 1;
        }
        if j + 1 < b.len() && b[j] == b'.' && b[j + 1].is_ascii_digit() {
            j += 1;
            while j < b.len() && b[j].is_ascii_digit() {
                j += 1;
            }
        }
        if j < b.len() && (b[j].is_ascii_alphabetic() || b[j] == b'%') {
            out.push(b[j] as char);
            i = j + 1;
        } else {
            // no conversion from this '%': the search moves one character on
            i += 1;
        }
    }
    out
}

/// Java's `Character.isDigit(char)`: ASCII exactly, past it Rust's numeric class (`PARITY.md`).
fn java_is_digit(c: char) -> bool {
    if c.is_ascii() { c.is_ascii_digit() } else { c.is_numeric() }
}

impl Sqlite {
    /// Every function as this dialect spells it, by name (Java's `calls`): `t` is the call's own type fact.
    pub(super) fn calls(
        &self,
        w: &mut SqlWriter,
        f: SqlFn,
        a: &[SqlExpr],
        t: &TypeFact,
        parent_prec: i32,
    ) -> Result<(), Refusal> {
        use SqlFn as F;
        match f {
            // the ANSI operators
            F::And | F::Or | F::Less | F::LessEqual | F::Greater | F::GreaterEqual | F::Plus | F::Minus | F::Times => {
                ansi::infix(self, w, f, a, parent_prec)
            }
            F::Equal | F::NotEqual => self.equal_arm(w, f, a, parent_prec),
            // the shared spellings (the base's named helpers)
            F::Not => ansi::not_prefix(self, w, a, parent_prec),
            F::IsNull => ansi::is_null_test(self, w, a),
            F::IsNotNull => ansi::is_not_null_test(self, w, a),
            F::IsDistinctFrom | F::NullSafeNotEqual => ansi::distinct_from(self, w, a),
            F::NullSafeEqual => ansi::not_distinct_from(self, w, a),
            F::Xor => ansi::xor_as_and_or(self, w, a, parent_prec),
            F::UcFirst => ansi::first_char_cased(self, w, "upper", a, parent_prec),
            F::LcFirst => ansi::first_char_cased(self, w, "lower", a, parent_prec),
            F::Ceiling => ansi::bigint_of(self, w, "ceil", a),
            F::Floor => ansi::bigint_of(self, w, "floor", a),
            F::Sign => ansi::bigint_of(self, w, "sign", a),
            F::Pi => {
                w.append("pi()");
                Ok(())
            }
            F::FloatCanon => ansi::float_canon_by_text(self, w, a, parent_prec),
            F::FloatPrint => ansi::float_print_by_text(self, w, a, parent_prec),
            // the hooks: this dialect's own
            F::Hash => self.hash_signed(w, a),
            F::Round => self.round_half_even(w, a),
            F::BitAnd | F::BitOr | F::BitXor | F::BitShiftLeft | F::BitShiftRight => self.bit_op(w, f, a),
            F::StructInsert => self.struct_insert(w, a),
            F::Unnest => self.unnest_projection(w, a),
            F::PureSplitPart => self.split_part_call(w, a),
            F::ListExists => self.list_exists(w, a),
            F::ListForAll => self.list_for_all(w, a),
            F::AllDistinct => self.all_distinct(w, a),
            F::ToVariant => self.variant_construct(w, a),
            F::VariantGet => self.variant_get(w, a),
            F::VariantElements => self.variant_elements(w, a),
            // this database's plain spellings: name(args)
            F::Abs => self.plain(w, "abs", a),
            F::Atan => self.plain(w, "atan", a),
            F::Atan2 => self.plain(w, "atan2", a),
            F::Cos => self.plain(w, "cos", a),
            F::Cosh => self.plain(w, "cosh", a),
            F::Cot => self.plain(w, "cot", a),
            F::Sin => self.plain(w, "sin", a),
            F::Sinh => self.plain(w, "sinh", a),
            F::Tan => self.plain(w, "tan", a),
            F::Tanh => self.plain(w, "tanh", a),
            F::Degrees => self.plain(w, "degrees", a),
            F::Radians => self.plain(w, "radians", a),
            F::Exp => self.plain(w, "exp", a),
            F::Ln => self.plain(w, "ln", a),
            F::Log10 => self.plain(w, "log10", a),
            F::Sqrt => self.plain(w, "sqrt", a),
            F::Pow => self.plain(w, "pow", a),
            F::Lower => self.plain(w, "lower", a),
            F::Upper => self.plain(w, "upper", a),
            F::Ltrim => self.plain(w, "ltrim", a),
            F::Rtrim => self.plain(w, "rtrim", a),
            F::Trim => self.plain(w, "trim", a),
            F::Replace => self.plain(w, "replace", a),
            F::Translate => Err(wall(f, "SQLite has no translate function")),
            F::ReverseString => self.plain(w, "reverse", a),
            F::Chr => self.plain(w, "char", a),
            F::FloorRaw => self.plain(w, "floor", a),
            F::Substring => self.plain(w, "substr", a),
            F::Strpos => self.plain(w, "instr", a),
            F::ListLength | F::JsonArrayLength => self.plain(w, "json_array_length", a),
            F::In => {
                if a.len() == 2
                    && let SqlExpr::PlanParam { name, .. } = &a[1]
                {
                    return Err(list_parameter(name));
                }
                ansi::in_list(self, w, a, parent_prec)
            }
            // DuckDB's ascii('') is 0 (unicode('') is NULL here)
            F::AsciiCode => {
                w.append("(CASE WHEN ").expr(self, arg(a, 0)?, 4)?.append(" IS NULL THEN NULL ELSE coalesce(");
                w.append("unicode(").expr(self, arg(a, 0)?, 0)?.append("), 0) END)");
                Ok(())
            }
            // outside [-1, 1] SQLite answers NULL, DuckDB raises
            F::Acos => self.domain_guarded(w, f, a, "acos"),
            F::Asin => self.domain_guarded(w, f, a, "asin"),
            F::Coalesce => {
                if a.len() == 1 {
                    w.expr(self, &a[0], parent_prec)?;
                    Ok(())
                } else {
                    self.plain(w, "coalesce", a)
                }
            }
            // never "--x": that is a comment
            F::Negate => {
                w.append("-(").expr(self, arg(a, 0)?, 0)?.append(")");
                Ok(())
            }
            F::Error => self.raise(a)(w),

            // ---- arithmetic
            // Pure's divide is a DOUBLE division; x / 0 is NULL here
            F::Divide => {
                w.append("(CAST(").expr(self, arg(a, 0)?, 0)?.append(" AS REAL) / CAST(").expr(self, arg(a, 1)?, 0)?;
                w.append(" AS REAL))");
                Ok(())
            }
            F::Mod | F::Rem => self.remainder(f, a)?(w),
            // DuckDB's // truncates toward zero, NULL for a zero divisor: SQLite's integer division
            F::IntDivide => {
                if a.iter().any(is_double) {
                    return Err(wall(f, "an integer division of a Float"));
                }
                w.append("(").expr(self, arg(a, 0)?, 6)?.append(" / ").expr(self, arg(a, 1)?, 7)?.append(")");
                Ok(())
            }
            F::RoundHalfUp => self.round_half_up(w, a, t),
            F::Cbrt => {
                w.append("(CASE WHEN ").expr(self, arg(a, 0)?, 4)?.append(" < 0 THEN -pow(-(").expr(self, &a[0], 0)?;
                w.append("), 1.0 / 3) ELSE pow(").expr(self, &a[0], 0)?.append(", 1.0 / 3) END)");
                Ok(())
            }
            F::BitNot => {
                w.append("(~ ").expr(self, arg(a, 0)?, 7)?.append(")");
                Ok(())
            }
            F::ParseInt => self.parse_int(arg(a, 0)?)(w),
            F::BoolToText => self.text_of(self.arg_ex(a, 0, 0), Some(&SqlType::Scalar(Scalar::Boolean)))(w),
            // NULL-skipping, as DuckDB's
            F::Greatest => self.null_skipping("max", a)(w),
            F::Least => self.null_skipping("min", a)(w),

            // ---- strings
            // NULL-skipping, as DuckDB's concat (SQLite's concat, 3.44)
            F::Concat => {
                w.append("concat(");
                w.join(&ansi::flatten_concat(a), ", ", |w, x| self.as_text(Cow::Borrowed(*x))(w))?;
                w.append(")");
                Ok(())
            }
            F::ConcatJoin => {
                w.append("concat(");
                w.join(a, ", ", |w, x| self.as_text(Cow::Borrowed(x))(w))?;
                w.append(")");
                Ok(())
            }
            F::Length => {
                w.append("length(");
                self.as_text(Cow::Borrowed(arg(a, 0)?))(w)?;
                w.append(")");
                Ok(())
            }
            F::Left | F::Right => self.left_right(f, a)(w),
            F::Lpad | F::Rpad => self.pad(f, a)(w),
            F::RepeatStr => {
                w.append("replace(hex(zeroblob(max(").expr(self, arg(a, 1)?, 0)?.append(", 0))), '00', ");
                w.expr(self, arg(a, 0)?, 0)?.append(")");
                Ok(())
            }
            F::StartsWith => {
                w.append("(substr(").expr(self, arg(a, 0)?, 0)?.append(", 1, length(").expr(self, arg(a, 1)?, 0)?;
                w.append(")) = ").expr(self, &a[1], 5)?.append(")");
                Ok(())
            }
            F::EndsWith => {
                w.append("(substr(").expr(self, arg(a, 0)?, 0)?.append(", length(").expr(self, &a[0], 0)?;
                w.append(") - length(").expr(self, arg(a, 1)?, 0)?.append(") + 1) = ").expr(self, &a[1], 5)?;
                w.append(")");
                Ok(())
            }
            F::Split => self.split(arg(a, 0)?, arg(a, 1)?)(w),
            // DuckDB's split_part: 1-based, negative from the end, out of range ''
            F::SplitPart => {
                w.append("(CASE WHEN ").expr(self, arg(a, 0)?, 4)?.append(" IS NULL OR ").expr(self, arg(a, 1)?, 4)?;
                w.append(" IS NULL THEN NULL ELSE coalesce(json_extract(");
                self.split(&a[0], &a[1])(w)?;
                w.append(", CASE WHEN ").expr(self, arg(a, 2)?, 4)?.append(" < 0 THEN '$[#' || ").expr(self, &a[2], 6)?;
                w.append(" || ']' ELSE '$[' || (").expr(self, &a[2], 6)?.append(" - 1) || ']' END), '') END)");
                Ok(())
            }
            // SQLite has no regular-expression function: a literal pattern's NFA is walked by the database
            F::Matches | F::RegexpFullMatch => self.matches_arm(w, f, a),
            F::RegexpExtract => self.regexp_extract(a)?(w),
            F::RegexpExtractAll => self.regexp_extract_all(a)?(w),
            F::RegexpReplace => self.regexp_replace(a)?(w),
            F::EncodeBase64 => self.base64_encoded(arg(a, 0)?)(w),
            F::DecodeBase64 => self.base64_decoded(arg(a, 0)?)(w),
            // SQLite has no digest function: each is written in its SQL
            F::Md5 => digests::md5(self.arg_ex(a, 0, 0))(w),
            F::Sha1 => digests::sha1(self.arg_ex(a, 0, 0))(w),
            F::Sha256 => digests::sha256(self.arg_ex(a, 0, 0))(w),
            F::Levenshtein => self.levenshtein(arg(a, 0)?, arg(a, 1)?)(w),
            F::JaroWinkler => self.jaro_winkler(arg(a, 0)?, arg(a, 1)?)(w),
            // random() is a signed 64-bit integer: its low 53 bits over 2^53
            F::Random => {
                w.append("((random() & 9007199254740991) / 9007199254740992.0)");
                Ok(())
            }
            F::Guid => {
                w.append(
                    "lower(hex(randomblob(4)) || '-' || hex(randomblob(2)) || '-4' || substr(hex(randomblob(2)), 2) \
                     || '-' || substr('89ab', 1 + (abs(random()) % 4), 1) || substr(hex(randomblob(2)), 2) || '-' || \
                     hex(randomblob(6)))",
                );
                Ok(())
            }
            F::Format => self.format(w, a),
            F::CurrentUserFn => Err(wall(f, "SQLite has no users")),

            // ---- temporal
            F::Today => {
                w.append("date('now')");
                Ok(())
            }
            F::Now => {
                w.append("strftime('%Y-%m-%d %H:%M:%f', 'now')");
                Ok(())
            }
            F::DateTruncDay => {
                w.append("date(").expr(self, arg(a, 0)?, 0)?.append(")");
                Ok(())
            }
            F::ParseDate => self.parsed_date(arg(a, 0)?)(w),
            F::Extract => self.extract(a)?(w),
            F::DateTrunc => self.date_trunc(a)?(w),
            F::MakeDate => {
                w.append("printf('%04d-%02d-%02d', ").list(self, a)?.append(")");
                Ok(())
            }
            F::MakeTimestamp => self.make_timestamp(w, f, a),
            // past SQLite's years 0000-9999 a date function answers NULL: raised, never a NULL value
            F::AddInterval | F::AddIntervalTemporal => {
                w.append("(CASE WHEN ").expr(self, arg(a, 1)?, 4)?.append(" IS NULL OR ").expr(self, arg(a, 2)?, 4)?;
                w.append(" IS NULL THEN NULL ELSE coalesce(");
                self.add_interval(a, t)?(w)?;
                w.append(", ");
                let past = [build::string_lit("a date past the years 0000-9999 has no SQLite value")];
                self.raise(&past)(w)?;
                w.append(") END)");
                Ok(())
            }
            F::DateDiff => self.date_diff(a)?(w),
            F::TimeBucket => self.time_bucket(a)?(w),
            // a zoned date-time is held as its naive UTC text: UTC's wall time is the instant's, either way
            F::Timezone => {
                match arg(a, 0)? {
                    SqlExpr::StringLit { value, .. } if UTC_ZONES.contains(&value.as_str()) => {}
                    _ => return Err(wall(f, "SQLite has no time zones but UTC")),
                }
                w.expr(self, arg(a, 1)?, parent_prec)?;
                Ok(())
            }
            // a date's text, a space, a time's text: the date-time text SQLite's date functions read
            F::DateAtTime => ansi::op(w, parent_prec, &|w| {
                w.expr(self, arg(a, 0)?, 3)?.append(" || ' ' || ").expr(self, arg(a, 1)?, 3)?;
                Ok(())
            }),
            F::EpochSeconds => {
                w.append("(");
                self.micros(arg(a, 0)?)(w)?;
                w.append(" / 1000000.0)");
                Ok(())
            }
            F::EpochNs | F::FromEpochNs => Err(Refusal::capability(
                "a nanosecond epoch: SQLite keeps microseconds (the Cypher island's nanosecond leg is DuckDB's)",
            )),
            F::EpochMs => {
                w.append("(");
                self.micros(arg(a, 0)?)(w)?;
                w.append(" / 1000)");
                Ok(())
            }
            F::FromEpochSeconds => {
                w.append("datetime(").expr(self, arg(a, 0)?, 0)?.append(", 'unixepoch')");
                Ok(())
            }
            F::FromEpochMs => {
                w.append("strftime('%Y-%m-%d %H:%M:%f', ").expr(self, arg(a, 0)?, 6)?.append(" / 1000.0, 'unixepoch')");
                Ok(())
            }
            F::Dayname => self.day_name(arg(a, 0)?)(w),
            F::Monthname => self.month_name(arg(a, 0)?, false)(w),
            F::Strftime => {
                let SqlExpr::FormatLit { parts, .. } = arg(a, 1)? else {
                    return Err(wall(f, "a date format that is not a format literal: its codes are DuckDB's"));
                };
                self.strftime(&a[0], parts)?(w)
            }
            F::Strptime => Err(wall(f, "SQLite parses ISO text only")),

            // ---- lists (JSON arrays; lambdas as correlated subqueries)
            F::ListFilter
            | F::ListTransform
            | F::ListConcat
            | F::ListGet
            | F::ListPosition
            | F::ListDistinct
            | F::ListAppend
            | F::ListSum
            | F::ListMin
            | F::ListMax
            | F::ListAvg
            | F::ListMedian
            | F::ListMode
            | F::ListBoolAnd
            | F::ListBoolOr
            | F::ListSort
            | F::ListSortDesc
            | F::ListTail
            | F::ListInit
            | F::ListSlice
            | F::ListReverse
            | F::ListFlatten
            | F::ListZip
            | F::ListProduct
            | F::ListReduce
            | F::RangeFn
            | F::RepeatValue
            | F::Typeof => self.lists(w, f, a, t),

            // ---- maps: JSON objects (text keys)
            F::MapFromLists
            | F::MapFromEntries
            | F::MapEmpty
            | F::MapExtract
            | F::MapKeys
            | F::MapValues
            | F::MapConcat => self.maps(w, f, a, t),

            // ---- variant, in DuckDB's json_type vocabulary
            F::JsonType => {
                w.append("(CASE json_type(").expr(self, arg(a, 0)?, 0)?;
                w.append(") WHEN 'null' THEN 'NULL' WHEN 'text' THEN 'VARCHAR' WHEN 'true' THEN 'BOOLEAN'");
                w.append(" WHEN 'false' THEN 'BOOLEAN' WHEN 'integer' THEN 'BIGINT' WHEN 'real' THEN 'DOUBLE'");
                w.append(" WHEN 'array' THEN 'ARRAY' WHEN 'object' THEN 'OBJECT' END)");
                Ok(())
            }
            F::JsonPretty => {
                w.append("json_pretty(").expr(self, arg(a, 0)?, 0)?.append(")");
                Ok(())
            }
            F::JsonMergePatch => {
                w.append("json_patch(").list(self, a)?.append(")");
                Ok(())
            }
        }
    }

    /// `name(args)`.
    fn plain(&self, w: &mut SqlWriter, name: &str, a: &[SqlExpr]) -> Result<(), Refusal> {
        w.function(self, name, a)?;
        Ok(())
    }

    /// The `ACOS` and `ASIN` arms: undefined outside [-1, 1], raised.
    fn domain_guarded(&self, w: &mut SqlWriter, f: SqlFn, a: &[SqlExpr], name: &str) -> Result<(), Refusal> {
        w.append("(CASE WHEN ").expr(self, arg(a, 0)?, 4)?.append(" IS NULL THEN NULL ELSE coalesce(").append(name);
        w.append("(").expr(self, &a[0], 0)?.append("), ");
        let undefined = [build::string_lit(&format!("{f} is undefined outside [-1,1]"))];
        self.raise(&undefined)(w)?;
        w.append(") END)");
        Ok(())
    }

    /// The `EQUAL`, `NOT_EQUAL` arm: composites compare as their JSON text.
    fn equal_arm(&self, w: &mut SqlWriter, f: SqlFn, a: &[SqlExpr], parent_prec: i32) -> Result<(), Refusal> {
        let composite = |e: &SqlExpr| {
            matches!(type_of(e), Some(SqlType::Struct { .. } | SqlType::Array { .. } | SqlType::Map { .. }))
        };
        if a.len() == 2 && (composite(&a[0]) || composite(&a[1])) {
            w.append("(json(").expr(self, &a[0], 0)?;
            w.append(if f == SqlFn::Equal { ") = json(" } else { ") <> json(" }).expr(self, &a[1], 0)?.append("))");
            return Ok(());
        }
        ansi::infix(self, w, f, a, parent_prec)
    }

    /// The `MATCHES`, `REGEXP_FULL_MATCH` arm: a literal pattern only.
    fn matches_arm(&self, w: &mut SqlWriter, f: SqlFn, a: &[SqlExpr]) -> Result<(), Refusal> {
        let pattern = match arg(a, 1)? {
            SqlExpr::StringLit { value, .. } if a.len() <= 2 => value,
            _ => {
                return Err(wall(
                    f,
                    "a pattern that is not a literal, or options beside it (SQLite matches a literal pattern only, as \
                     a table its query walks)",
                ));
            }
        };
        regex::matches(self.arg_ex(a, 0, 0), pattern, f == SqlFn::RegexpFullMatch)?(w)
    }

    /// A raised error: a bad JSON path is an error carrying its text, raised only when evaluated (Java's `raise`).
    pub(super) fn raise<'a>(&'a self, a: &'a [SqlExpr]) -> P<'a> {
        pc(move |w| {
            w.append("json_extract('{}', char(31) || ");
            if a.len() > 1 {
                w.expr(self, &a[1], 6)?.append(" || char(30) || ");
            }
            // the error text quotes the path SQL-style ('' for '): each quote travels as the envelope's quote mark
            w.append("replace(").expr(self, arg(a, 0)?, 0)?.append(", '''', char(29)) || char(31))");
            Ok(())
        })
    }

    /// A raise of a message the piece owns.
    fn raise_owned<'a>(&'a self, a: Vec<SqlExpr>) -> P<'a> {
        pc(move |w| self.raise(&a)(w))
    }

    /// Pure's mod (the divisor's sign) and rem (the dividend's): `%` over integers, `mod()` over Floats (Java's
    /// `remainder`).
    fn remainder<'a>(&'a self, f: SqlFn, a: &'a [SqlExpr]) -> Result<P<'a>, Refusal> {
        let (x, y) = (arg(a, 0)?, arg(a, 1)?);
        let dbl = !is_integer(x) || !is_integer(y);
        let rem: P<'a> = if dbl {
            pc(move |w| {
                w.append("mod(").expr(self, x, 0)?.append(", ").expr(self, y, 0)?.append(")");
                Ok(())
            })
        } else {
            pc(move |w| {
                w.append("(").expr(self, x, 7)?.append(" % ").expr(self, y, 7)?.append(")");
                Ok(())
            })
        };
        if f == SqlFn::Rem {
            return Ok(rem);
        }
        Ok(if dbl {
            pc(move |w| {
                w.append("mod(");
                rem(w)?;
                w.append(" + ").expr(self, y, 6)?.append(", ").expr(self, y, 0)?.append(")");
                Ok(())
            })
        } else {
            pc(move |w| {
                w.append("((");
                rem(w)?;
                w.append(" + ").expr(self, y, 6)?.append(") % ").expr(self, y, 7)?.append(")");
                Ok(())
            })
        })
    }

    /// DuckDB's round: half away from zero, by DuckDB's own algorithm (Java's `roundHalfUp`).
    fn round_half_up(&self, w: &mut SqlWriter, a: &[SqlExpr], t: &TypeFact) -> Result<(), Refusal> {
        if a.len() == 1 {
            let integral =
                scalar_is(fact_type(t), Scalar::Bigint) || scalar_is(fact_type(t), Scalar::Integer);
            w.append(if integral { "CAST(round(" } else { "round(" }).expr(self, &a[0], 0)?;
            w.append(if integral { ") AS INTEGER)" } else { ")" });
            return Ok(());
        }
        let places = pc(move |w: &mut SqlWriter| {
            w.append("CAST(").expr(self, arg(a, 1)?, 0)?.append(" AS INTEGER)");
            Ok(())
        });
        let p1 = places.clone();
        let up = pc(move |w: &mut SqlWriter| {
            w.append("pow(10, ");
            p1(w)?;
            w.append(")");
            Ok(())
        });
        let p2 = places.clone();
        let down = pc(move |w: &mut SqlWriter| {
            w.append("pow(10, -");
            p2(w)?;
            w.append(")");
            Ok(())
        });
        w.append("(CASE WHEN ");
        places(w)?;
        w.append(" >= 0 THEN round(").expr(self, arg(a, 0)?, 6)?.append(" * ");
        up(w)?;
        w.append(") / ");
        up(w)?;
        w.append(" ELSE round(").expr(self, &a[0], 6)?.append(" / ");
        down(w)?;
        w.append(") * ");
        down(w)?;
        w.append(" END)");
        Ok(())
    }

    /// DuckDB's 64-bit parse: an optional sign and digits, else an error (Java's `parseInt`).
    fn parse_int<'a>(&'a self, s: &'a SqlExpr) -> P<'a> {
        let digits = pc(move |w: &mut SqlWriter| {
            w.append("ltrim(trim(").expr(self, s, 0)?.append("), '+-')");
            Ok(())
        });
        let message = vec![build::call(
            SqlFn::Concat,
            vec![build::string_lit("Could not convert string '"), s.clone(), build::string_lit("' to INT64")],
        )];
        let raised = self.raise_owned(message);
        pc(move |w| {
            w.append("(CASE WHEN ").expr(self, s, 4)?.append(" IS NULL THEN NULL WHEN ");
            digits(w)?;
            w.append(" <> '' AND ");
            digits(w)?;
            w.append(" NOT GLOB '*[^0-9]*' AND length(trim(").expr(self, s, 0)?.append(")) - length(");
            digits(w)?;
            w.append(") <= 1 THEN CAST(trim(").expr(self, s, 0)?.append(") AS INTEGER) ELSE ");
            raised(w)?;
            w.append(" END)");
            Ok(())
        })
    }

    /// GREATEST/LEAST over the arguments, each replaced by the next non-NULL one when it is NULL (Java's
    /// `nullSkipping`).
    fn null_skipping<'a>(&'a self, f: &'static str, a: &'a [SqlExpr]) -> P<'a> {
        if a.len() == 1 {
            return self.ex(&a[0], 0);
        }
        pc(move |w| {
            w.append(f).append("(");
            for i in 0..a.len() {
                w.append(if i > 0 { ", coalesce(" } else { "coalesce(" }).expr(self, &a[i], 0)?;
                for (j, other) in a.iter().enumerate() {
                    if j != i {
                        w.append(", ").expr(self, other, 0)?;
                    }
                }
                w.append(")");
            }
            w.append(")");
            Ok(())
        })
    }

    /// DuckDB's left/right: a negative count is all but that many (Java's `leftRight`).
    fn left_right<'a>(&'a self, f: SqlFn, a: &'a [SqlExpr]) -> P<'a> {
        if f == SqlFn::Left {
            return pc(move |w| {
                w.append("(CASE WHEN ").expr(self, arg(a, 1)?, 4)?.append(" < 0 THEN substr(").expr(self, arg(a, 0)?, 0)?;
                w.append(", 1, max(length(").expr(self, &a[0], 0)?.append(") + ").expr(self, &a[1], 6)?;
                w.append(", 0)) ELSE substr(").expr(self, &a[0], 0)?.append(", 1, ").expr(self, &a[1], 0)?;
                w.append(") END)");
                Ok(())
            });
        }
        pc(move |w| {
            w.append("(CASE WHEN ").expr(self, arg(a, 1)?, 4)?.append(" < 0 THEN substr(").expr(self, arg(a, 0)?, 0)?;
            w.append(", 1 - ").expr(self, &a[1], 7)?.append(") WHEN ").expr(self, &a[1], 4)?;
            w.append(" = 0 THEN '' ELSE substr(").expr(self, &a[0], 0)?.append(", -").expr(self, &a[1], 7)?;
            w.append(") END)");
            Ok(())
        })
    }

    /// DuckDB's lpad/rpad: to the length, truncating a longer text; the pad (a space by default) repeated (Java's
    /// `pad`).
    fn pad<'a>(&'a self, f: SqlFn, a: &'a [SqlExpr]) -> P<'a> {
        let p: P<'a> = if a.len() > 2 { self.ex(&a[2], 0) } else { text("' '") };
        let fill = pc(move |w: &mut SqlWriter| {
            w.append("substr(replace(hex(zeroblob(max(").expr(self, arg(a, 1)?, 0)?.append(", 0))), '00', ");
            p(w)?;
            w.append("), 1, ").expr(self, &a[1], 6)?.append(" - length(").expr(self, arg(a, 0)?, 0)?.append("))");
            Ok(())
        });
        pc(move |w| {
            w.append("(CASE WHEN length(").expr(self, arg(a, 0)?, 0)?.append(") >= ").expr(self, arg(a, 1)?, 5)?;
            w.append(" THEN substr(").expr(self, &a[0], 0)?.append(", 1, ").expr(self, &a[1], 0)?.append(") ELSE ");
            if f == SqlFn::Lpad {
                fill(w)?;
                w.append(" || ").expr(self, &a[0], 6)?;
            } else {
                w.expr(self, &a[0], 6)?.append(" || ");
                fill(w)?;
            }
            w.append(" END)");
            Ok(())
        })
    }

    /// printf: a `%s` argument as its text (Java's `format`).
    fn format(&self, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
        let SqlExpr::StringLit { value, .. } = arg(a, 0)? else {
            w.append("printf(").list(self, a)?.append(")");
            return Ok(());
        };
        let mut args: Vec<P<'_>> = Vec::new();
        let mut at = 1;
        for conversion in conversions(value) {
            if conversion == '%' {
                continue;
            }
            if at < a.len() {
                let x = &a[at];
                args.push(if conversion == 's' { self.as_text(Cow::Borrowed(x)) } else { self.ex(x, 0) });
            }
            at += 1;
        }
        while at < a.len() {
            args.push(self.ex(&a[at], 0));
            at += 1;
        }
        w.append("printf(").expr(self, &a[0], 0)?;
        for p in &args {
            w.append(", ");
            p(w)?;
        }
        w.append(")");
        Ok(())
    }

    // ---- regular expressions: a literal pattern's matches found by the database

    /// DuckDB's regexp_extract: the leftmost match's group ('' when none matches; NULL for NULL).
    fn regexp_extract<'a>(&'a self, a: &'a [SqlExpr]) -> Result<P<'a>, Refusal> {
        let pattern = literal(arg(a, 1)?, "a regexp_extract pattern")?.to_owned();
        let g = group(a)?;
        let groups: Vec<i32> = if g == 0 { Vec::new() } else { vec![g] };
        let head = format!(
            "(SELECT CASE WHEN __rq.s IS NULL THEN NULL WHEN __rq.f IS NULL THEN '' ELSE {} END FROM (SELECT __rp.s AS \
             s, ",
            part("__rq.s", "__rq.f", &span(g), "0")
        );
        Ok(pc(move |w| {
            w.append(&head);
            regex::find(text("__rp.s"), &pattern, &groups)?(w)?;
            w.append(" AS f FROM (SELECT ").expr(self, &a[0], 0)?.append(" AS s) AS __rp) AS __rq)");
            Ok(())
        }))
    }

    /// DuckDB's regexp_extract_all: each non-overlapping match's group, left to right; NULL for NULL.
    fn regexp_extract_all<'a>(&'a self, a: &'a [SqlExpr]) -> Result<P<'a>, Refusal> {
        let pattern = literal(arg(a, 1)?, "a regexp_extract_all pattern")?.to_owned();
        let g = group(a)?;
        let groups: Vec<i32> = if g == 0 { Vec::new() } else { vec![g] };
        let next = "__ms.off + (__ms.f ->> 1) + CASE WHEN (__ms.f ->> 1) = (__ms.f ->> 0) THEN 1 ELSE 0 END";
        Ok(pc(move |w| {
            w.append("(SELECT CASE WHEN __rp.s IS NULL THEN NULL ELSE (WITH RECURSIVE __ms(n, off, f) AS (SELECT 0, 0, ");
            regex::find(text("__rp.s"), &pattern, &groups)?(w)?;
            w.append(&format!(" UNION ALL SELECT __ms.n + 1, {next}, "));
            regex::find(text(format!("substr(__rp.s, {next} + 1)")), &pattern, &groups)?(w)?;
            w.append(&format!(
                " FROM __ms WHERE __ms.f IS NOT NULL AND {next} <= length(__rp.s)) SELECT json_group_array({} ORDER BY \
                 __ms.n) FROM __ms WHERE __ms.f IS NOT NULL) END FROM (SELECT ",
                part("__rp.s", "__ms.f", &span(g), "__ms.off")
            ));
            w.expr(self, &a[0], 0)?.append(" AS s) AS __rp)");
            Ok(())
        }))
    }

    /// DuckDB's regexp_replace(s, p, r[, options]): the first match unless the options say 'g'; the replacement's
    /// `\0`..`\9` the match's groups (Java's `regexpReplace`).
    fn regexp_replace<'a>(&'a self, a: &'a [SqlExpr]) -> Result<P<'a>, Refusal> {
        let pattern = literal(arg(a, 1)?, "a regexp_replace pattern")?;
        let replacement = literal(arg(a, 2)?, "a regexp_replace replacement")?;
        let options = if a.len() > 3 { literal(&a[3], "regexp_replace options")? } else { "" };
        let mut global = false;
        let mut flags = "";
        for o in options.chars() {
            match o {
                'g' => global = true,
                'i' => flags = "(?i)",
                'c' => flags = "",
                _ => return Err(Refusal::capability(format!("regexp_replace option '{o}' reached SQLite"))),
            }
        }
        let p = format!("{flags}{pattern}");
        // the replacement's parts: text, or a group reference
        enum Part {
            Text(String),
            Group(i32),
        }
        let mut parts = Vec::new();
        let mut groups: Vec<i32> = Vec::new();
        let mut text_run = String::new();
        let chars: Vec<char> = replacement.chars().collect();
        let mut i = 0;
        while i < chars.len() {
            let ch = chars[i];
            if ch == '\\' && i + 1 < chars.len() && java_is_digit(chars[i + 1]) {
                if !text_run.is_empty() {
                    parts.push(Part::Text(std::mem::take(&mut text_run)));
                }
                i += 1;
                let k = chars[i] as i32 - '0' as i32;
                parts.push(Part::Group(k));
                if k > 0 && !groups.contains(&k) {
                    groups.push(k);
                }
            } else if ch == '\\' && i + 1 < chars.len() {
                i += 1;
                text_run.push(chars[i]);
            } else {
                text_run.push(ch);
            }
            i += 1;
        }
        if !text_run.is_empty() {
            parts.push(Part::Text(text_run));
        }
        let repl = |f: &str, off: &str| -> String {
            let mut pieces = Vec::new();
            for part in &parts {
                pieces.push(match part {
                    Part::Group(k) => {
                        let at = if *k == 0 {
                            span(0)
                        } else {
                            let ix = groups.iter().position(|g| g == k).map_or(-1, |i| i as i64);
                            [(2 + 2 * ix).to_string(), (3 + 2 * ix).to_string()]
                        };
                        part_text("__rp.s", f, &at, off)
                    }
                    Part::Text(t) => self.lit(t),
                });
            }
            if pieces.is_empty() { "''".to_owned() } else { pieces.join(" || ") }
        };
        if !global {
            let head = format!(
                "(SELECT CASE WHEN __rp.s IS NULL THEN NULL WHEN __rp.f IS NULL THEN __rp.s ELSE substr(__rp.s, 1, __rp.f \
                 ->> 0) || {} || substr(__rp.s, (__rp.f ->> 1) + 1) END FROM (SELECT __rr.s AS s, ",
                repl("__rp.f", "0")
            );
            return Ok(pc(move |w| {
                w.append(&head);
                regex::find(text("__rr.s"), &p, &groups)?(w)?;
                w.append(" AS f FROM (SELECT ").expr(self, &a[0], 0)?.append(" AS s) AS __rr) AS __rp)");
                Ok(())
            }));
        }
        let empty = "(__ms.f ->> 1) = (__ms.f ->> 0)";
        let next = format!("__ms.off + (__ms.f ->> 1) + CASE WHEN {empty} THEN 1 ELSE 0 END");
        let step = format!(
            " UNION ALL SELECT {next}, __ms.o || substr(__rp.s, __ms.off + 1, __ms.f ->> 0) || {} || CASE WHEN {empty} \
             THEN substr(__rp.s, __ms.off + (__ms.f ->> 1) + 1, 1) ELSE '' END, ",
            repl("__ms.f", "__ms.off")
        );
        Ok(pc(move |w| {
            w.append("(SELECT CASE WHEN __rp.s IS NULL THEN NULL ELSE (WITH RECURSIVE __ms(off, o, f) AS (SELECT 0, '', ");
            regex::find(text("__rp.s"), &p, &groups)?(w)?;
            w.append(&step);
            regex::find(text(format!("substr(__rp.s, {next} + 1)")), &p, &groups)?(w)?;
            w.append(
                " FROM __ms WHERE __ms.f IS NOT NULL AND __ms.off <= length(__rp.s)) SELECT __ms.o || substr(__rp.s, \
                 __ms.off + 1) FROM __ms ORDER BY __ms.off DESC LIMIT 1) END FROM (SELECT ",
            );
            w.expr(self, &a[0], 0)?.append(" AS s) AS __rp)");
            Ok(())
        }))
    }

    // ---- list calls

    /// The list calls: JSON arrays, lambdas as correlated subqueries (Java's `lists`).
    fn lists(&self, w: &mut SqlWriter, f: SqlFn, a: &[SqlExpr], t: &TypeFact) -> Result<(), Refusal> {
        use SqlFn as F;
        let e = self.param("__e")?;
        let o = self.param("__o")?;
        match f {
            F::ListFilter => {
                let l = lambda_of(arg(a, 1)?, f)?;
                let x = param_at(l, 0)?;
                let xs = arg(a, 0)?;
                let el = list_element(xs);
                let px = self.param(x)?;
                let select = json_of(text(px), el);
                let elc = el.cloned();
                let filter = pc(move |w: &mut SqlWriter| {
                    w.expr(self, &lambda_body(l, elc.as_ref())?, 0)?;
                    Ok(())
                });
                let index = l.params.get(1).map(String::as_str);
                self.over_elements(xs, x, &Sqlite::offset_of(l)?, index, select, Some(filter))?(w)
            }
            F::ListTransform => {
                let l = lambda_of(arg(a, 1)?, f)?;
                let xs = arg(a, 0)?;
                let body = lambda_body(l, list_element(xs))?;
                let index = l.params.get(1).map(String::as_str);
                let select = self.json_cow(Cow::Owned(body));
                self.over_elements(xs, param_at(l, 0)?, &Sqlite::offset_of(l)?, index, select, None)?(w)
            }
            // NULL ++ xs is xs, as DuckDB's list_concat
            F::ListConcat => self.concat_lists(a, t)(w),
            // 1-based, negative from the end, 0 and out of range NULL (DuckDB's list_extract)
            F::ListGet => {
                w.append("(CASE WHEN ").expr(self, arg(a, 1)?, 4)?.append(" = 0 THEN NULL ELSE ");
                let index = pc(move |w: &mut SqlWriter| {
                    w.append("CASE WHEN ").expr(self, &a[1], 4)?.append(" < 0 THEN '#' || ").expr(self, &a[1], 6)?;
                    w.append(" ELSE ").expr(self, &a[1], 6)?.append(" - 1 END");
                    Ok(())
                });
                self.element_at(Cow::Borrowed(arg(a, 0)?), index)(w)?;
                w.append(" END)");
                Ok(())
            }
            // DuckDB's list_position: 1-based, NULL when absent, a NULL needle found by IS
            F::ListPosition => {
                w.append(&format!("(SELECT min({o}) + 1 FROM "));
                self.elements(arg(a, 0)?, "__e", "__o", None)?(w)?;
                w.append(&format!(" WHERE {e} IS ")).expr(self, arg(a, 1)?, 5)?.append(")");
                Ok(())
            }
            // DuckDB's list_distinct drops NULLs; the first occurrence's order
            F::ListDistinct => {
                let xs = arg(a, 0)?;
                let held = json_of(text(e.clone()), list_element(xs));
                let body = pc(move |w: &mut SqlWriter| {
                    w.append("(SELECT json_group_array(");
                    held(w)?;
                    w.append(&format!(" ORDER BY {o}) FROM (SELECT {e}, min({o}) AS {o} FROM "));
                    self.elements(xs, "__e", "__o", None)?(w)?;
                    w.append(&format!(" WHERE {e} IS NOT NULL GROUP BY {e}))"));
                    Ok(())
                });
                self.unless_null(xs, body)(w)
            }
            // list_append(NULL, v) is [v]
            F::ListAppend => {
                w.append("json_insert(coalesce(").expr(self, arg(a, 0)?, 0)?.append(", json_array()), '$[#]', ");
                self.json(arg(a, 1)?)(w)?;
                w.append(")");
                Ok(())
            }
            F::ListSum => self.over_rows(arg(a, 0)?, AggFn::Sum, &[])?(w),
            F::ListMin => self.over_rows(arg(a, 0)?, AggFn::Min, &[])?(w),
            F::ListMax => self.over_rows(arg(a, 0)?, AggFn::Max, &[])?(w),
            F::ListAvg => self.over_rows(arg(a, 0)?, AggFn::Avg, &[])?(w),
            F::ListMedian => self.over_rows(arg(a, 0)?, AggFn::Median, &[])?(w),
            F::ListMode => self.over_rows(arg(a, 0)?, AggFn::Mode, &[])?(w),
            F::ListBoolAnd => self.over_rows(arg(a, 0)?, AggFn::BoolAnd, &[])?(w),
            F::ListBoolOr => self.over_rows(arg(a, 0)?, AggFn::BoolOr, &[])?(w),
            // ascending / descending, NULLs last both ways; a struct by its fields in declared order
            F::ListSort | F::ListSortDesc => {
                let dir = if f == F::ListSort { " ASC NULLS LAST" } else { " DESC NULLS LAST" };
                let xs = arg(a, 0)?;
                let el = list_element(xs).cloned();
                let held = json_of(text(e.clone()), el.as_ref());
                let body = pc(move |w: &mut SqlWriter| {
                    w.append("(SELECT json_group_array(");
                    held(w)?;
                    w.append(" ORDER BY ").append(&self.element_sort(&e, el.as_ref(), dir)).append(") FROM ");
                    self.elements(xs, "__e", "__o", None)?(w)?;
                    w.append(")");
                    Ok(())
                });
                self.unless_null(xs, body)(w)
            }
            F::ListReverse => {
                let xs = arg(a, 0)?;
                let held = json_of(text(e.clone()), list_element(xs));
                let body = pc(move |w: &mut SqlWriter| {
                    w.append("(SELECT json_group_array(");
                    held(w)?;
                    w.append(&format!(" ORDER BY {o} DESC) FROM "));
                    self.elements(xs, "__e", "__o", None)?(w)?;
                    w.append(")");
                    Ok(())
                });
                self.unless_null(xs, body)(w)
            }
            F::ListTail => {
                let xs = arg(a, 0)?;
                let held = json_of(text(e.clone()), list_element(xs));
                self.over_elements(xs, "__e", "__o", None, held, Some(text(format!("{o} > 0"))))?(w)
            }
            F::ListInit => {
                let xs = arg(a, 0)?;
                let held = json_of(text(e.clone()), list_element(xs));
                let filter = pc(move |w: &mut SqlWriter| {
                    w.append(&format!("{o} < json_array_length(")).expr(self, xs, 0)?.append(") - 1");
                    Ok(())
                });
                self.over_elements(xs, "__e", "__o", None, held, Some(filter))?(w)
            }
            // DuckDB's list_slice: 1-based, both ends inclusive, a negative end from the end
            F::ListSlice => {
                let xs = arg(a, 0)?;
                let held = json_of(text(e.clone()), list_element(xs));
                let filter = pc(move |w: &mut SqlWriter| {
                    w.append(&format!("{o} + 1 >= "));
                    self.slice_end(xs, arg(a, 1)?)(w)?;
                    w.append(&format!(" AND {o} + 1 <= "));
                    self.slice_end(xs, arg(a, 2)?)(w)?;
                    Ok(())
                });
                self.over_elements(xs, "__e", "__o", None, held, Some(filter))?(w)
            }
            F::ListFlatten => {
                let xs = arg(a, 0)?;
                let inner = match list_element(xs) {
                    Some(SqlType::Array { element }) => Some(element.as_ref().clone()),
                    _ => None,
                };
                let held = json_of(text(element_read(inner.as_ref(), "__fi")), inner.as_ref());
                let body = pc(move |w: &mut SqlWriter| {
                    w.append("(SELECT json_group_array(");
                    held(w)?;
                    w.append(" ORDER BY __fo.key, __fi.key) FROM json_each(").expr(self, xs, 0)?;
                    w.append(") AS __fo, json_each(__fo.value) AS __fi)");
                    Ok(())
                });
                self.unless_null(xs, body)(w)
            }
            F::ListZip => self.zip(f, a, t)?(w),
            F::ListProduct => {
                w.append(
                    "(WITH RECURSIVE __p(__i, __v) AS (SELECT 0, 1 UNION ALL SELECT __p.__i + 1, __p.__v * __x.value \
                     FROM __p JOIN json_each(",
                );
                w.expr(self, arg(a, 0)?, 0)?.append(") AS __x ON __x.key = __p.__i) SELECT CASE WHEN ");
                w.expr(self, &a[0], 4)?.append(" IS NULL THEN NULL ELSE __v END FROM __p ORDER BY __i DESC LIMIT 1)");
                Ok(())
            }
            F::ListReduce => self.list_reduce(w, a),
            // DuckDB's range: [start, stop) by step (start 0, step 1 by default)
            F::RangeFn => self.range(w, a),
            F::RepeatValue => {
                w.append("(WITH RECURSIVE __g(__n) AS (SELECT 1 UNION ALL SELECT __n + 1 FROM __g WHERE __n < ");
                w.expr(self, arg(a, 1)?, 4)?.append(") SELECT CASE WHEN ").expr(self, &a[1], 4)?;
                w.append(" > 0 THEN json_group_array(");
                self.json(arg(a, 0)?)(w)?;
                w.append(") ELSE json_array() END FROM __g)");
                Ok(())
            }
            F::Typeof => Err(wall(f, "its type names are SQLite's storage classes")),
            other => Err(Refusal::illegal_state(format!("not a list call: {other}"))),
        }
    }

    /// DuckDB's list_reduce: the first element starts the accumulator, the lambda (acc, x) runs over the rest
    /// (Java's `listReduce`).
    fn list_reduce(&self, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
        let l = match arg(a, 1)? {
            SqlExpr::Lambda(l) if l.params.len() == 2 => l,
            _ => return Err(wall(SqlFn::ListReduce, "a reduction lambda of another shape")),
        };
        let acc = &l.params[0];
        let x = &l.params[1];
        let swapped = Lambda { params: vec![x.clone(), acc.clone()], body: l.body.clone(), r#type: TypeFact::Unknown };
        let xs = arg(a, 0)?;
        let body = lambda_body(&swapped, list_element(xs))?;
        let pacc = self.param(acc)?;
        let px = self.param(x)?;
        w.append("(WITH RECURSIVE __r(__i, ").append(&pacc).append(") AS (SELECT 1, ");
        self.element_at(Cow::Borrowed(xs), text("0"))(w)?;
        w.append(" UNION ALL SELECT __r.__i + 1, ").expr(self, &body, 0)?;
        w.append(" FROM __r JOIN (SELECT key AS __k, ").append(&element_read(list_element(xs), "")).append(" AS ");
        w.append(&px).append(" FROM json_each(").expr(self, xs, 0)?;
        w.append(")) AS __x ON __x.__k = __r.__i) SELECT ").append(&pacc);
        w.append(" FROM __r ORDER BY __i DESC LIMIT 1)");
        Ok(())
    }

    /// The lists concatenated, NULL ones as empty, each element re-held as its type (Java's `concatLists`).
    fn concat_lists<'a>(&'a self, a: &'a [SqlExpr], t: &TypeFact) -> P<'a> {
        let el = fact_list_element(t).cloned();
        pc(move |w| {
            w.append("(SELECT json_group_array(");
            json_of(text("__c.__v"), el.as_ref())(w)?;
            w.append(" ORDER BY __c.__s, __c.__k) FROM (");
            let read = element_read(el.as_ref(), "");
            for (i, x) in a.iter().enumerate() {
                w.append(if i > 0 { " UNION ALL " } else { "" });
                w.append(&format!("SELECT {i} AS __s, key AS __k, {read} AS __v FROM json_each(")).expr(self, x, 0)?;
                w.append(")");
            }
            w.append(") AS __c)");
            Ok(())
        })
    }

    /// DuckDB's list_zip: a struct per position of the longest list (Java's `zip`).
    fn zip<'a>(&'a self, f: SqlFn, a: &'a [SqlExpr], t: &TypeFact) -> Result<P<'a>, Refusal> {
        let fields = match fact_list_element(t) {
            Some(SqlType::Struct { fields }) if fields.len() == a.len() => fields.clone(),
            _ => return Err(wall(f, "a zip of unknown struct type")),
        };
        Ok(pc(move |w| {
            w.append("(SELECT json_group_array(json_object(");
            for (i, x) in a.iter().enumerate() {
                let name = self.lit(&fields[i].name);
                w.append(if i > 0 { ", " } else { "" }).append(&name).append(", ");
                json_of(self.element_at(Cow::Borrowed(x), text("__z.key")), Some(&fields[i].r#type))(w)?;
            }
            w.append(") ORDER BY __z.key) FROM json_each(");
            // the longest list's positions
            w.append("(SELECT __l FROM (");
            for (i, x) in a.iter().enumerate() {
                w.append(if i > 0 { " UNION ALL " } else { "" }).append("SELECT ").expr(self, x, 0)?;
                w.append(" AS __l");
            }
            w.append(") ORDER BY coalesce(json_array_length(__l), 0) DESC LIMIT 1)) AS __z)");
            Ok(())
        }))
    }

    fn range(&self, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
        let start: P<'_> = if a.len() == 1 { text("0") } else { self.ex(&a[0], 0) };
        let last = arg(a, if a.len() == 1 { 0 } else { 1 })?;
        let stop = self.ex(last, 0);
        let step: P<'_> = if a.len() > 2 { self.ex(&a[2], 0) } else { text("1") };
        w.append("(CASE WHEN ((");
        stop(w)?;
        w.append(") - (");
        start(w)?;
        w.append(")) * (");
        step(w)?;
        w.append(") > 0 THEN (WITH RECURSIVE __g(__v) AS (SELECT ");
        start(w)?;
        w.append(" UNION ALL SELECT __v + (");
        step(w)?;
        w.append(") FROM __g WHERE (");
        step(w)?;
        w.append(" > 0 AND __v + (");
        step(w)?;
        w.append(") < ");
        stop(w)?;
        w.append(") OR (");
        step(w)?;
        w.append(" < 0 AND __v + (");
        step(w)?;
        w.append(") > ");
        stop(w)?;
        w.append(")) SELECT json_group_array(__v) FROM __g) ELSE json_array() END)");
        Ok(())
    }

    /// A slice bound, 1-based: a negative one counts from the end (Java's `sliceEnd`).
    fn slice_end<'a>(&'a self, xs: &'a SqlExpr, i: &'a SqlExpr) -> P<'a> {
        pc(move |w| {
            w.append("(CASE WHEN ").expr(self, i, 4)?.append(" < 0 THEN json_array_length(").expr(self, xs, 0)?;
            w.append(") + ").expr(self, i, 6)?.append(" + 1 ELSE ").expr(self, i, 0)?.append(" END)");
            Ok(())
        })
    }

    /// The ORDER BY of a list's elements: a struct by its fields in declared order (Java's `elementSort`).
    fn element_sort(&self, x: &str, el: Option<&SqlType>, dir: &str) -> String {
        if let Some(SqlType::Struct { fields }) = el {
            let keys: Vec<String> = fields
                .iter()
                .map(|f| self.element_sort(&format!("({x} ->> {})", self.lit(&f.name)), Some(&f.r#type), dir))
                .collect();
            return keys.join(", ");
        }
        format!("{x}{dir}")
    }

    // ---- map calls: JSON objects (text keys)

    /// The map calls (Java's `maps`).
    fn maps(&self, w: &mut SqlWriter, f: SqlFn, a: &[SqlExpr], t: &TypeFact) -> Result<(), Refusal> {
        use SqlFn as F;
        let value: Option<SqlType> = match fact_type(t) {
            Some(SqlType::Map { value, .. }) => Some(value.as_ref().clone()),
            _ => {
                let of = if a.is_empty() { fact_type(t) } else { type_of(&a[0]) };
                match of {
                    Some(SqlType::Map { value, .. }) => Some(value.as_ref().clone()),
                    _ => None,
                }
            }
        };
        let value = value.as_ref();
        match f {
            F::MapEmpty => {
                w.append("json_object()");
                Ok(())
            }
            F::MapFromLists => {
                let v = list_element(arg(a, 1)?);
                w.append("(SELECT json_group_object(__k.value, ");
                json_of(text(element_read(v, "__v")), v)(w)?;
                w.append(" ORDER BY __k.key) FROM json_each(").expr(self, arg(a, 0)?, 0)?;
                w.append(") AS __k LEFT JOIN json_each(").expr(self, &a[1], 0)?.append(") AS __v ON __v.key = __k.key)");
                Ok(())
            }
            // DuckDB's map_from_entries: each entry a struct of (key, value), by position whatever their names
            F::MapFromEntries => {
                let xs = arg(a, 0)?;
                let entry = match list_element(xs) {
                    Some(SqlType::Struct { fields }) if fields.len() == 2 => fields,
                    other => return Err(wall(f, &format!("entries of {}", type_text(other)))),
                };
                let k = self.lit(&entry[0].name);
                let v = self.lit(&entry[1].name);
                let vt = &entry[1].r#type;
                let arrow = if *vt == json_type() { "->" } else { "->>" };
                w.append(&format!("(SELECT json_group_object(__e.value ->> {k}, "));
                json_of(text(format!("(__e.value {arrow} {v})")), Some(vt))(w)?;
                w.append(" ORDER BY __e.key) FROM json_each(").expr(self, xs, 0)?.append(") AS __e)");
                Ok(())
            }
            // DuckDB's map_extract: a list of the key's value, empty when absent
            F::MapExtract => {
                w.append("(CASE WHEN ").expr(self, arg(a, 0)?, 4)?.append(" IS NULL THEN NULL ELSE coalesce((");
                w.append("SELECT json_array(");
                json_of(text(element_read(value, "")), value)(w)?;
                w.append(") FROM json_each(").expr(self, &a[0], 0)?.append(") WHERE key = ").expr(self, arg(a, 1)?, 5)?;
                w.append("), json_array()) END)");
                Ok(())
            }
            F::MapKeys => {
                let xs = arg(a, 0)?;
                let body = pc(move |w: &mut SqlWriter| {
                    w.append("(SELECT json_group_array(key ORDER BY id) FROM json_each(").expr(self, xs, 0)?;
                    w.append("))");
                    Ok(())
                });
                self.unless_null(xs, body)(w)
            }
            F::MapValues => {
                let xs = arg(a, 0)?;
                let held = json_of(text(element_read(value, "")), value);
                let body = pc(move |w: &mut SqlWriter| {
                    w.append("(SELECT json_group_array(");
                    held(w)?;
                    w.append(" ORDER BY id) FROM json_each(").expr(self, xs, 0)?.append("))");
                    Ok(())
                });
                self.unless_null(xs, body)(w)
            }
            // the later map's keys win, as DuckDB's map_concat
            F::MapConcat => {
                w.append("(SELECT json_group_object(__m.__k, ");
                json_of(text("__m.__v"), value)(w)?;
                w.append(&format!(
                    " ORDER BY __m.__s, __m.__i) FROM (SELECT key AS __k, {} AS __v, __s, id AS __i FROM (",
                    element_read(value, "")
                ));
                for (i, x) in a.iter().enumerate() {
                    w.append(if i > 0 { " UNION ALL " } else { "" });
                    w.append(&format!("SELECT {i} AS __s, j.* FROM json_each(")).expr(self, x, 0)?.append(") AS j");
                }
                w.append(") AS __u WHERE __s = (SELECT max(__s2) FROM (");
                for (i, x) in a.iter().enumerate() {
                    w.append(if i > 0 { " UNION ALL " } else { "" });
                    w.append(&format!("SELECT {i} AS __s2, key AS __k2 FROM json_each(")).expr(self, x, 0)?;
                    w.append(")");
                }
                w.append(") WHERE __k2 = __u.key)) AS __m)");
                Ok(())
            }
            other => Err(Refusal::illegal_state(format!("not a map call: {other}"))),
        }
    }

    // ---- temporal helpers: dates and date-times are ISO text

    /// Pure's parseDate of a text: its date's fields padded, its time and fraction as written; a text that is no
    /// date raises (Java's `parsedDate`).
    fn parsed_date<'a>(&'a self, x: &'a SqlExpr) -> P<'a> {
        let message = vec![build::call(
            SqlFn::Concat,
            vec![build::string_lit("Could not parse '"), x.clone(), build::string_lit("' as a date")],
        )];
        let raised = self.raise_owned(message);
        let held = padded(date_time_text(self.ex(x, 0)));
        pc(move |w| {
            w.append(
                "(SELECT CASE WHEN date(substr(__pp.t, 1, 10)) = substr(__pp.t, 1, 10) AND (length(__pp.t) = 10 OR \
                 substr(__pp.t, 11, 1) = ' ') THEN __pp.t ELSE ",
            );
            raised(w)?;
            w.append(" END FROM (SELECT ");
            held(w)?;
            w.append(" AS t) AS __pp)");
            Ok(())
        })
    }

    /// A text's base64 (of its UTF-8 bytes, padded with '='), in a recursive walk (Java's `base64Encoded`).
    fn base64_encoded<'a>(&'a self, x: &'a SqlExpr) -> P<'a> {
        let mut n = String::new();
        for k in 1..=6 {
            n.push_str(if k == 1 { "" } else { " | " });
            n.push_str(&format!("({} << {})", nibble("__g.h6", k), 4 * (6 - k)));
        }
        let a = format!("'{BASE64}'");
        let tail = format!(
            " AS v) AS __s UNION ALL SELECT substr(__b64.h, 7), __b64.o || (SELECT substr({a}, ((__q.n >> 18) & 63) + 1, \
             1) || substr({a}, ((__q.n >> 12) & 63) + 1, 1) || CASE WHEN __q.l >= 4 THEN substr({a}, ((__q.n >> 6) & 63) \
             + 1, 1) ELSE '=' END || CASE WHEN __q.l = 6 THEN substr({a}, (__q.n & 63) + 1, 1) ELSE '=' END FROM (SELECT \
             {n} AS n, __g.l AS l FROM (SELECT substr(substr(__b64.h, 1, 6) || '00000', 1, 6) AS h6, \
             length(substr(__b64.h, 1, 6)) AS l) AS __g) AS __q) FROM __b64 WHERE __b64.h <> '') SELECT o FROM __b64 \
             WHERE h = '')"
        );
        pc(move |w| {
            w.append(
                "(WITH RECURSIVE __b64(h, o) AS (SELECT hex(__s.v), CASE WHEN __s.v IS NULL THEN NULL ELSE '' END FROM \
                 (SELECT ",
            );
            w.expr(self, x, 0)?.append(&tail);
            Ok(())
        })
    }

    /// A base64 text's bytes as UTF-8 text ('=' padding optional) (Java's `base64Decoded`).
    fn base64_decoded<'a>(&'a self, x: &'a SqlExpr) -> P<'a> {
        let mut n = String::new();
        for k in 1..=4 {
            n.push_str(if k == 1 { "" } else { " | " });
            n.push_str(&format!("((instr('{BASE64}', substr(__g.q4, {k}, 1)) - 1) << {})", 6 * (4 - k)));
        }
        let tail = format!(
            " AS v) AS __s UNION ALL SELECT substr(__d64.t, 5), __d64.h || (SELECT substr(printf('%06X', __q.n), 1, 2 * \
             (__q.l - 1)) FROM (SELECT {n} AS n, __g.l AS l FROM (SELECT substr(substr(__d64.t, 1, 4) || 'AAA', 1, 4) AS \
             q4, length(substr(__d64.t, 1, 4)) AS l) AS __g) AS __q) FROM __d64 WHERE __d64.t <> '') SELECT \
             CAST(unhex(h) AS TEXT) FROM __d64 WHERE t = '')"
        );
        pc(move |w| {
            w.append(
                "(WITH RECURSIVE __d64(t, h) AS (SELECT rtrim(__s.v, '='), CASE WHEN __s.v IS NULL THEN NULL ELSE '' \
                 END FROM (SELECT ",
            );
            w.expr(self, x, 0)?.append(&tail);
            Ok(())
        })
    }

    /// The Levenshtein distance of two texts, by characters (Java's `levenshtein`).
    fn levenshtein<'a>(&'a self, x: &'a SqlExpr, y: &'a SqlExpr) -> P<'a> {
        let cell = "min((__lv.p ->> (__lv.j + 1)) + 1, (__lv.c ->> __lv.j) + 1, (__lv.p ->> __lv.j) + \
                    (substr(__lv.a, __lv.i, 1) <> substr(__lv.b, __lv.j + 1, 1)))";
        let more = "__lv.j < length(__lv.b)";
        let tail = format!(
            " AS b) AS __s UNION ALL SELECT __lv.a, __lv.b, CASE WHEN {more} THEN __lv.i ELSE __lv.i + 1 END, CASE WHEN \
             {more} THEN __lv.j + 1 ELSE 0 END, CASE WHEN {more} THEN __lv.p ELSE __lv.c END, CASE WHEN {more} THEN \
             json_insert(__lv.c, '$[#]', CASE WHEN __lv.i = 0 THEN __lv.j + 1 ELSE {cell} END) ELSE json_array(__lv.i \
             + 1) END FROM __lv WHERE NOT (__lv.j = length(__lv.b) AND __lv.i = length(__lv.a))) SELECT c ->> j FROM \
             __lv WHERE i = length(a) AND j = length(b))"
        );
        pc(move |w| {
            w.append("(WITH RECURSIVE __lv(a, b, i, j, p, c) AS (SELECT __s.a, __s.b, 0, 0, '[]', '[0]' FROM (SELECT ");
            w.expr(self, x, 0)?.append(" AS a, ").expr(self, y, 0)?.append(&tail);
            Ok(())
        })
    }

    /// The Jaro-Winkler similarity of two texts, by characters (Java's `jaroWinkler`).
    fn jaro_winkler<'a>(&'a self, x: &'a SqlExpr, y: &'a SqlExpr) -> P<'a> {
        let hit = "substr(__jw.f, __jw.i, 1) = '0' AND substr(__jw.p, __jw.i, 1) = substr(__jw.t, __jw.j, 1)";
        let next = format!("__jw.i > __jw.j + __jw.b OR __jw.i > length(__jw.p) OR ({hit})");
        let taken = format!("__jw.i <= __jw.j + __jw.b AND {hit}");
        let m = "CAST(length(__jx.m) AS REAL)";
        let jaro = format!(
            "({m} / CAST(length(__jx.p) AS REAL) + {m} / CAST(length(__jx.t) AS REAL) + ({m} - CAST(__jx.n / 2 AS \
             REAL)) / {m}) / CAST(3 AS REAL)"
        );
        let mut prefix = String::new();
        for k in 1..=4 {
            prefix.push_str(if k == 1 { "" } else { " + " });
            prefix.push_str(&format!(
                "CASE WHEN {k} <= min(length(__jx.p), length(__jx.t)) AND substr(__jx.p, 1, {k}) = substr(__jx.t, 1, \
                 {k}) THEN 1 ELSE 0 END"
            ));
        }
        let window = "max(0, max(length(__s.p), length(__s.t)) / 2 - 1)";
        let head = format!(
            "(WITH RECURSIVE __jw(p, t, b, j, i, f, m) AS (SELECT __s.p, __s.t, {window}, 1, max(1, 1 - {window}), \
             replace(hex(zeroblob(length(__s.p))), '00', '0'), '' FROM (SELECT "
        );
        let tail = format!(
            " AS t) AS __s UNION ALL SELECT __jw.p, __jw.t, __jw.b, CASE WHEN {next} THEN __jw.j + 1 ELSE __jw.j END, \
             CASE WHEN {next} THEN max(1, __jw.j + 1 - __jw.b) ELSE __jw.i + 1 END, CASE WHEN {taken} THEN \
             substr(__jw.f, 1, __jw.i - 1) || '1' || substr(__jw.f, __jw.i + 1) ELSE __jw.f END, CASE WHEN {taken} THEN \
             __jw.m || substr(__jw.t, __jw.j, 1) ELSE __jw.m END FROM __jw WHERE __jw.j <= length(__jw.t)), __jx(p, t, \
             f, m, k, q, n) AS (SELECT __jw.p, __jw.t, __jw.f, __jw.m, 1, 0, 0 FROM __jw WHERE __jw.j > length(__jw.t) \
             UNION ALL SELECT __jx.p, __jx.t, __jx.f, __jx.m, __jx.k + 1, CASE WHEN substr(__jx.f, __jx.k, 1) = '1' THEN \
             __jx.q + 1 ELSE __jx.q END, CASE WHEN substr(__jx.f, __jx.k, 1) = '1' AND substr(__jx.p, __jx.k, 1) <> \
             substr(__jx.m, __jx.q + 1, 1) THEN __jx.n + 1 ELSE __jx.n END FROM __jx WHERE __jx.k <= length(__jx.p)) \
             SELECT CASE WHEN length(__jx.m) = 0 THEN CAST(0 AS REAL) WHEN {jaro} > CAST(0.7 AS REAL) THEN {jaro} + \
             ({prefix}) * CAST(0.1 AS REAL) * (CAST(1 AS REAL) - {jaro}) ELSE {jaro} END FROM __jx WHERE __jx.k > \
             length(__jx.p))"
        );
        pc(move |w| {
            w.append(&head);
            w.expr(self, x, 0)?.append(" AS p, ").expr(self, y, 0)?.append(&tail);
            Ok(())
        })
    }

    /// A date-time's microseconds since the epoch (Java's `micros`).
    fn micros<'a>(&'a self, t: &'a SqlExpr) -> P<'a> {
        pc(move |w| {
            w.append("(unixepoch(substr(").expr(self, t, 0)?.append(", 1, 19)) * 1000000 + CASE WHEN instr(");
            w.expr(self, t, 0)?.append(", '.') > 0 THEN CAST(substr(substr(").expr(self, t, 0)?.append(", instr(");
            w.expr(self, t, 0)?.append(", '.') + 1) || '000000', 1, 6) AS INTEGER) ELSE 0 END)");
            Ok(())
        })
    }

    /// A date part of a date, a Pure integer (Java's `extract`).
    fn extract<'a>(&'a self, a: &'a [SqlExpr]) -> Result<P<'a>, Refusal> {
        let p = ansi::part(arg(a, 0)?)?;
        let code = match p {
            DatePart::Year => Some("%Y"),
            DatePart::Month => Some("%m"),
            DatePart::Day => Some("%d"),
            DatePart::Hour => Some("%H"),
            DatePart::Minute => Some("%M"),
            DatePart::Second => Some("%S"),
            DatePart::Week => Some("%V"),
            DatePart::DayOfYear => Some("%j"),
            DatePart::IsoDayOfWeek => Some("%u"),
            DatePart::Quarter => None,
            DatePart::Millisecond | DatePart::Microsecond | DatePart::Nanosecond => {
                return Err(Refusal::capability(format!(
                    "date part '{}' has no measured SQLite spelling",
                    part_name(p)
                )));
            }
        };
        Ok(match code {
            None => pc(move |w| {
                w.append("((CAST(strftime('%m', ").expr(self, arg(a, 1)?, 0)?.append(") AS INTEGER) + 2) / 3)");
                Ok(())
            }),
            Some(code) => pc(move |w| {
                w.append(&format!("CAST(strftime('{code}', ")).expr(self, arg(a, 1)?, 0)?.append(") AS INTEGER)");
                Ok(())
            }),
        })
    }

    /// The base's contract: year/quarter/month/week deliver a DATE, finer parts a date-time (Java's `dateTrunc`).
    fn date_trunc<'a>(&'a self, a: &'a [SqlExpr]) -> Result<P<'a>, Refusal> {
        let p = ansi::part(arg(a, 0)?)?;
        let t = arg(a, 1)?;
        let wrapped = |head: &'static str, tail: &'static str| -> P<'a> {
            pc(move |w| {
                w.append(head).expr(self, t, 0)?.append(tail);
                Ok(())
            })
        };
        Ok(match p {
            DatePart::Year => wrapped("strftime('%Y-01-01', ", ")"),
            DatePart::Quarter => pc(move |w| {
                w.append("printf('%s-%02d-01', strftime('%Y', ").expr(self, t, 0)?;
                w.append("), ((CAST(strftime('%m', ").expr(self, t, 0)?.append(") AS INTEGER) - 1) / 3) * 3 + 1)");
                Ok(())
            }),
            DatePart::Month => wrapped("strftime('%Y-%m-01', ", ")"),
            DatePart::Week => wrapped("date(", ", '-6 days', 'weekday 1')"),
            DatePart::Day => wrapped("(date(", ") || ' 00:00:00')"),
            DatePart::Hour => wrapped("strftime('%Y-%m-%d %H:00:00', ", ")"),
            DatePart::Minute => wrapped("strftime('%Y-%m-%d %H:%M:00', ", ")"),
            DatePart::Second => wrapped("strftime('%Y-%m-%d %H:%M:%S', ", ")"),
            DatePart::Millisecond => wrapped("strftime('%Y-%m-%d %H:%M:%f', ", ")"),
            DatePart::Microsecond | DatePart::Nanosecond | DatePart::DayOfYear | DatePart::IsoDayOfWeek => {
                return Err(Refusal::capability(format!(
                    "date_trunc part '{}' has no measured SQLite spelling",
                    part_name(p)
                )));
            }
        })
    }

    /// A date plus an interval (Java's `addInterval`): whole years and months by the date functions' modifiers, a
    /// date plus days a date, anything finer in microseconds.
    fn add_interval<'a>(&'a self, a: &'a [SqlExpr], c: &TypeFact) -> Result<P<'a>, Refusal> {
        let unit = ansi::part(arg(a, 0)?)?;
        let n = arg(a, 1)?;
        let t = arg(a, 2)?;
        let call_type = fact_type(c);
        let date = scalar_is(call_type, Scalar::Date)
            || (scalar_is(type_of(t), Scalar::Date) && !scalar_is(call_type, Scalar::Timestamp));
        let months = match unit {
            DatePart::Year => Some("years"),
            DatePart::Month => Some("months"),
            _ => None,
        };
        if let Some(months) = months {
            let modifier = pc(move |w: &mut SqlWriter| {
                w.append(&format!("printf('%+d {months}', ")).expr(self, n, 0)?.append(")");
                Ok(())
            });
            if date {
                return Ok(pc(move |w| {
                    w.append("date(").expr(self, t, 0)?.append(", ");
                    modifier(w)?;
                    w.append(", 'floor')");
                    Ok(())
                }));
            }
            return Ok(pc(move |w| {
                w.append("(datetime(substr(").expr(self, t, 0)?.append(", 1, 19), ");
                modifier(w)?;
                w.append(", 'floor') || substr(").expr(self, t, 0)?.append(", 20))");
                Ok(())
            }));
        }
        let Some(micros) = interval_micros(unit) else {
            return Err(Refusal::capability(format!("interval unit {unit} has no SQLite spelling")));
        };
        if date && micros % 86_400_000_000 == 0 {
            let days = micros / 86_400_000_000;
            return Ok(pc(move |w| {
                w.append("date(").expr(self, t, 0)?.append(", printf('%+d days', ").expr(self, n, 6)?;
                w.append(&format!(" * {days}))"));
                Ok(())
            }));
        }
        let us = self.micros(t);
        Ok(from_micros(pc(move |w| {
            w.append("(");
            us(w)?;
            w.append(" + ").expr(self, n, 6)?.append(&format!(" * {micros})"));
            Ok(())
        })))
    }

    /// DuckDB's date_diff counts BOUNDARIES crossed (Java's `dateDiff`).
    fn date_diff<'a>(&'a self, a: &'a [SqlExpr]) -> Result<P<'a>, Refusal> {
        let p = ansi::part(arg(a, 0)?)?;
        let from = arg(a, 1)?;
        let to = arg(a, 2)?;
        let c = |e: &'a SqlExpr, code: &'static str| self.component(e, code);
        Ok(match p {
            DatePart::Year => {
                let (t, f) = (c(to, "%Y"), c(from, "%Y"));
                pc(move |w| {
                    w.append("(");
                    t(w)?;
                    w.append(" - ");
                    f(w)?;
                    w.append(")");
                    Ok(())
                })
            }
            DatePart::Quarter => {
                let (ty, tm, fy, fm) = (c(to, "%Y"), c(to, "%m"), c(from, "%Y"), c(from, "%m"));
                pc(move |w| {
                    w.append("((");
                    ty(w)?;
                    w.append(" * 4 + (");
                    tm(w)?;
                    w.append(" - 1) / 3) - (");
                    fy(w)?;
                    w.append(" * 4 + (");
                    fm(w)?;
                    w.append(" - 1) / 3))");
                    Ok(())
                })
            }
            DatePart::Month => {
                let (ty, tm, fy, fm) = (c(to, "%Y"), c(to, "%m"), c(from, "%Y"), c(from, "%m"));
                pc(move |w| {
                    w.append("((");
                    ty(w)?;
                    w.append(" * 12 + ");
                    tm(w)?;
                    w.append(") - (");
                    fy(w)?;
                    w.append(" * 12 + ");
                    fm(w)?;
                    w.append("))");
                    Ok(())
                })
            }
            DatePart::Week => pc(move |w| {
                w.append("CAST((julianday(date(").expr(self, to, 0)?.append(")) - julianday(date(").expr(self, from, 0)?;
                w.append("))) / 7 AS INTEGER)");
                Ok(())
            }),
            DatePart::Day => pc(move |w| {
                w.append("CAST(julianday(date(").expr(self, to, 0)?.append(")) - julianday(date(").expr(self, from, 0)?;
                w.append(")) AS INTEGER)");
                Ok(())
            }),
            DatePart::Hour => self.boundaries(from, to, 3_600_000_000),
            DatePart::Minute => self.boundaries(from, to, 60_000_000),
            DatePart::Second => self.boundaries(from, to, 1_000_000),
            DatePart::Millisecond => self.boundaries(from, to, 1000),
            DatePart::Microsecond => self.boundaries(from, to, 1),
            DatePart::Nanosecond | DatePart::DayOfYear | DatePart::IsoDayOfWeek => {
                return Err(Refusal::capability(format!(
                    "date_diff part '{}' has no measured SQLite spelling",
                    part_name(p)
                )));
            }
        })
    }

    fn component<'a>(&'a self, t: &'a SqlExpr, code: &'static str) -> P<'a> {
        pc(move |w| {
            w.append(&format!("CAST(strftime('{code}', ")).expr(self, t, 0)?.append(") AS INTEGER)");
            Ok(())
        })
    }

    /// The unit boundaries between two date-times: each floored to the unit, in microseconds.
    fn boundaries<'a>(&'a self, from: &'a SqlExpr, to: &'a SqlExpr, unit: i64) -> P<'a> {
        let t = floor_div(self.micros(to), unit);
        let f = floor_div(self.micros(from), unit);
        pc(move |w| {
            w.append("(");
            t(w)?;
            w.append(" - ");
            f(w)?;
            w.append(")");
            Ok(())
        })
    }

    /// DuckDB's time_bucket with the base's origins (Java's `timeBucket`).
    fn time_bucket<'a>(&'a self, a: &'a [SqlExpr]) -> Result<P<'a>, Refusal> {
        let unit = ansi::part(arg(a, 0)?)?;
        let n = arg(a, 1)?;
        let t = arg(a, 2)?;
        if let Some(micros) = interval_micros(unit) {
            let origin: i64 = if unit == DatePart::Week { -3 * 86_400_000_000 } else { 0 };
            let size = pc(move |w: &mut SqlWriter| {
                w.append("(").expr(self, n, 6)?.append(&format!(" * {micros})"));
                Ok(())
            });
            let us = self.micros(t);
            let m = pc(move |w: &mut SqlWriter| {
                w.append("(");
                us(w)?;
                w.append(&format!(" - ({origin}))"));
                Ok(())
            });
            let floored = pc(move |w: &mut SqlWriter| {
                w.append("((");
                m(w)?;
                w.append(" - (((");
                m(w)?;
                w.append(") % ");
                size(w)?;
                w.append(" + ");
                size(w)?;
                w.append(") % ");
                size(w)?;
                w.append(&format!(")) + ({origin}))"));
                Ok(())
            });
            return Ok(from_micros(floored));
        }
        let months = match unit {
            DatePart::Month => 1,
            DatePart::Year => 12,
            _ => 0,
        };
        if months == 0 {
            return Err(Refusal::capability(format!("a {unit} time bucket has no SQLite spelling")));
        }
        let (y, mo) = (self.component(t, "%Y"), self.component(t, "%m"));
        let k = pc(move |w: &mut SqlWriter| {
            w.append("((");
            y(w)?;
            w.append(" - 1970) * 12 + ");
            mo(w)?;
            w.append(" - 1)");
            Ok(())
        });
        let size = pc(move |w: &mut SqlWriter| {
            w.append("(").expr(self, n, 6)?.append(&format!(" * {months})"));
            Ok(())
        });
        Ok(pc(move |w| {
            w.append("(date('1970-01-01', printf('%+d months', ((");
            k(w)?;
            w.append(") - (((");
            k(w)?;
            w.append(") % ");
            size(w)?;
            w.append(" + ");
            size(w)?;
            w.append(") % ");
            size(w)?;
            w.append(")))) || ' 00:00:00')");
            Ok(())
        }))
    }

    fn make_timestamp(&self, w: &mut SqlWriter, f: SqlFn, a: &[SqlExpr]) -> Result<(), Refusal> {
        if a.len() != 6 {
            return Err(wall(f, &format!("make_timestamp of {} arguments", a.len())));
        }
        let s = &a[5];
        w.append("(printf('%04d-%02d-%02d %02d:%02d:', ").list(self, &a[..5])?.append(") || CASE WHEN ");
        w.expr(self, s, 4)?.append(" = CAST(").expr(self, s, 0)?.append(" AS INTEGER) THEN printf('%02d', ");
        w.expr(self, s, 0)?.append(") ELSE rtrim(printf('%09.6f', ").expr(self, s, 0)?.append("), '0') END)");
        Ok(())
    }

    fn month_name<'a>(&'a self, t: &'a SqlExpr, abbreviated: bool) -> P<'a> {
        pc(move |w| {
            w.append("(CASE CAST(strftime('%m', ").expr(self, t, 0)?.append(") AS INTEGER)");
            for (i, m) in MONTHS.iter().enumerate() {
                let name = if abbreviated { &m[..3] } else { m };
                w.append(&format!(" WHEN {} THEN '{name}'", i + 1));
            }
            w.append(" END)");
            Ok(())
        })
    }

    fn day_name<'a>(&'a self, t: &'a SqlExpr) -> P<'a> {
        pc(move |w| {
            w.append("(CASE CAST(strftime('%w', ").expr(self, t, 0)?.append(") AS INTEGER)");
            for (i, d) in DAYS.iter().enumerate() {
                w.append(&format!(" WHEN {i} THEN '{d}'"));
            }
            w.append(" END)");
            Ok(())
        })
    }

    /// A date-time's fraction as `digits` digits: its text's, zero-padded (Java's `fraction`).
    fn fraction<'a>(&'a self, t: &'a SqlExpr, digits: usize) -> P<'a> {
        let zeros = "0".repeat(digits);
        pc(move |w| {
            w.append("substr(CASE WHEN instr(").expr(self, t, 0)?.append(", '.') > 0 THEN substr(").expr(self, t, 0)?;
            w.append(", instr(").expr(self, t, 0)?.append(&format!(", '.') + 1) ELSE '' END || '{zeros}', 1, {digits})"));
            Ok(())
        })
    }

    /// A date-time formatted by DuckDB's strftime parts (Java's `strftime`).
    fn strftime<'a>(&'a self, value: &'a SqlExpr, parts: &[DateFmt]) -> Result<P<'a>, Refusal> {
        let mut pieces: Vec<P<'a>> = Vec::new();
        let mut run = String::new();
        let coded = |run: &str| -> P<'a> {
            let f = self.lit(run);
            pc(move |w| {
                w.append(&format!("strftime({f}, ")).expr(self, value, 0)?.append(")");
                Ok(())
            })
        };
        for p in parts {
            let code = match p {
                DateFmt::Text { s } => Some(s.replace('%', "%%")),
                DateFmt::Part(part) => match part {
                    DateFmtPart::Year4 => Some("%Y".to_owned()),
                    DateFmtPart::Month2 => Some("%m".to_owned()),
                    DateFmtPart::Day2 => Some("%d".to_owned()),
                    DateFmtPart::Hour2 => Some("%H".to_owned()),
                    DateFmtPart::Min2 => Some("%M".to_owned()),
                    DateFmtPart::Sec2 => Some("%S".to_owned()),
                    _ => None,
                },
            };
            if let Some(code) = code {
                run.push_str(&code);
                continue;
            }
            if !run.is_empty() {
                pieces.push(coded(&run));
                run.clear();
            }
            let DateFmt::Part(part) = p else { unreachable!("a text is coded") };
            let hour = pc(move |w: &mut SqlWriter| {
                w.append("CAST(strftime('%H', ").expr(self, value, 0)?.append(") AS INTEGER)");
                Ok(())
            });
            pieces.push(match part {
                DateFmtPart::SubsecMicro => self.fraction(value, 6),
                DateFmtPart::SubsecNano => self.fraction(value, 9),
                // DuckDB's %g: milliseconds, three digits
                DateFmtPart::SubsecMin => self.fraction(value, 3),
                DateFmtPart::MonthAbbrev => self.month_name(value, true),
                DateFmtPart::MonthName => self.month_name(value, false),
                DateFmtPart::WeekdayName => self.day_name(value),
                DateFmtPart::Hour12 => pc(move |w| {
                    w.append("printf('%02d', (");
                    hour(w)?;
                    w.append(" + 11) % 12 + 1)");
                    Ok(())
                }),
                DateFmtPart::Hour12Nopad => pc(move |w| {
                    w.append("CAST((");
                    hour(w)?;
                    w.append(" + 11) % 12 + 1 AS TEXT)");
                    Ok(())
                }),
                DateFmtPart::Ampm => pc(move |w| {
                    w.append("(CASE WHEN ");
                    hour(w)?;
                    w.append(" < 12 THEN 'AM' ELSE 'PM' END)");
                    Ok(())
                }),
                coded_part => return Err(Refusal::illegal_state(format!("coded: {coded_part}"))),
            });
        }
        if !run.is_empty() || pieces.is_empty() {
            pieces.push(coded(&run));
        }
        if pieces.len() == 1 {
            return Ok(pieces.pop().expect("one piece"));
        }
        Ok(pc(move |w| {
            w.append("(");
            for (i, p) in pieces.iter().enumerate() {
                if i > 0 {
                    w.append(" || ");
                }
                p(w)?;
            }
            w.append(")");
            Ok(())
        }))
    }
}

/// `text`'s part (Java's `part`), as the regexp_replace arms name it.
fn part_text(text: &str, f: &str, at: &[String; 2], from: &str) -> String {
    part(text, f, at, from)
}

/// A date-time of the given microseconds since the epoch (Java's `fromMicros`).
fn from_micros(us: P<'_>) -> P<'_> {
    pc(move |w| {
        w.append("(datetime(((");
        us(w)?;
        w.append(" - ((");
        us(w)?;
        w.append(" % 1000000) + 1000000) % 1000000) / 1000000), 'unixepoch') || CASE WHEN (((");
        us(w)?;
        w.append(" % 1000000) + 1000000) % 1000000) = 0 THEN '' ELSE '.' || rtrim(printf('%06d', (((");
        us(w)?;
        w.append(" % 1000000) + 1000000) % 1000000)), '0') END)");
        Ok(())
    })
}

/// `a` floor-divided by a positive `b` (Java's `floorDiv`).
fn floor_div(a: P<'_>, b: i64) -> P<'_> {
    pc(move |w| {
        w.append("((");
        a(w)?;
        w.append(" - (((");
        a(w)?;
        w.append(&format!(") % {b}) + {b}) % {b}) / {b})"));
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::conversions;

    #[test]
    fn printfs_conversions_are_javas_regex_finds() {
        assert_eq!(conversions("%s and %d"), vec!['s', 'd']);
        assert_eq!(conversions("%%x %05.2f %-8s"), vec!['%', 'f', 's']);
        // a '.' with no digit after it is no precision: no conversion there
        assert_eq!(conversions("%5.%d"), vec!['d']);
        assert_eq!(conversions("100%"), Vec::<char>::new());
        assert_eq!(conversions("%+ #0x"), vec!['x']);
    }
}
