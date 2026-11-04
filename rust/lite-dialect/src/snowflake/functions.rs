// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! Snowflake's scalar functions: Java's `call` -- every `SqlFn` decided by an exhaustive match -- with the helpers its
//! arms name (Snowflake.java, "Lists" to "temporal helpers"): the list functions over semi-structured ARRAYs, the
//! collection reductions, the fold, the maps as OBJECTs, the variant reads and casts, error() as a cast that cannot
//! parse, printf unrolled, the regular expressions as POSIX reads them, the date parts, truncations, differences,
//! buckets and formats.

use super::{
    Param, Snowflake, fact_type, is_double, is_integer, is_json, is_text, is_timestamp, lambda_arg,
    list_element, param, param_at, type_of, type_text,
};
use crate::ansi::{self, cast_type_name, part};
use crate::pieces::{Ex, P, pc, text};
use crate::refusal::{Refusal, arg};
use crate::renderer::AnsiSqlRenderer;
use crate::writer::SqlWriter;
use lite_sql::{AggFn, DateFmt, DateFmtPart, DatePart, Scalar, SqlExpr, SqlFn, SqlType, TypeFact};

fn wall(f: SqlFn, why: &str) -> Refusal {
    Refusal::capability(format!("{f} reached Snowflake: {why}"))
}

/// Java's `negativeLiteral(e)`.
fn negative_literal(e: &SqlExpr) -> bool {
    matches!(e, SqlExpr::IntLit { value, .. } if *value < 0)
}

/// Java's `isNumber`-ish check of a list median's or quantile's element: an integer, a DOUBLE or a decimal.
fn numeric(t: Option<&SqlType>) -> bool {
    is_integer(t) || matches!(t, Some(SqlType::Scalar(Scalar::Double)) | Some(SqlType::Decimal { .. }))
}

/// Whether a timestamp is an instant (Java's `instant`): TIMESTAMPTZ-typed, or a timezone() of a naive one.
fn instant(e: &SqlExpr) -> bool {
    if let SqlExpr::Call { r#fn: SqlFn::Timezone, args, .. } = e
        && args.len() == 2
    {
        return !instant(&args[1]);
    }
    matches!(type_of(e), Some(SqlType::Scalar(Scalar::Timestamptz)))
}

/// A unit as Snowflake's DATE_PART, DATE_TRUNC, DATEADD and DATEDIFF name it (Java's `unit`).
fn unit(part: DatePart) -> Result<&'static str, Refusal> {
    Ok(match part {
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
        DatePart::Nanosecond | DatePart::DayOfYear | DatePart::IsoDayOfWeek => {
            return Err(Refusal::capability(format!("a {part} unit has no Snowflake spelling")));
        }
    })
}

/// An interval's DATEADD unit (Java's `intervalUnit`).
fn interval_unit(part: DatePart) -> Result<&'static str, Refusal> {
    if part == DatePart::Quarter {
        return Err(Refusal::capability(format!("interval unit {part} has no Snowflake spelling")));
    }
    unit(part)
}

/// One part as a TO_CHAR / TO_TIMESTAMP format element (Java's `formatCode`).
fn format_code(p: &DateFmt) -> Result<String, Refusal> {
    Ok(match p {
        DateFmt::Text { s } => {
            if s.is_empty() {
                String::new()
            } else {
                format!("\"{s}\"")
            }
        }
        DateFmt::Part(part) => match part {
            DateFmtPart::Year4 => "YYYY",
            DateFmtPart::Month2 => "MM",
            DateFmtPart::Day2 => "DD",
            DateFmtPart::Hour2 => "HH24",
            DateFmtPart::Min2 => "MI",
            DateFmtPart::Sec2 => "SS",
            DateFmtPart::SubsecMicro => "FF6",
            DateFmtPart::SubsecNano => "FF9",
            // DuckDB's %g: milliseconds, three digits (Pure's SSS)
            DateFmtPart::SubsecMin => "FF3",
            DateFmtPart::MonthAbbrev => "MON",
            DateFmtPart::MonthName => "MMMM",
            DateFmtPart::Hour12 => "HH12",
            DateFmtPart::Ampm => "AM",
            DateFmtPart::WeekdayName => {
                return Err(Refusal::capability(
                    "a full weekday name in a Snowflake date format (DY is three letters)",
                ));
            }
            DateFmtPart::Hour12Nopad => {
                return Err(Refusal::capability("an unpadded 12-hour clock in a Snowflake date format"));
            }
        }
        .to_owned(),
    })
}

/// The format of a date format written as a literal (Java's `formatText`): TO_TIMESTAMP's parse format.
pub(super) fn format_text(parts: &[DateFmt]) -> Result<String, Refusal> {
    let mut out = String::new();
    for p in parts {
        if let DateFmt::Text { s } = p
            && s.contains('"')
        {
            return Err(Refusal::capability("a double quote in a Snowflake date format's text"));
        }
        out.push_str(&format_code(p)?);
    }
    Ok(out)
}

/// The full English weekday name (Java's `weekdayName`).
fn weekday_name(x: P<'_>) -> P<'_> {
    pc(move |w| {
        w.append("DECODE(DAYOFWEEKISO(").piece(&*x)?.append("), 1, 'Monday', 2, 'Tuesday', 3, 'Wednesday',");
        w.append(" 4, 'Thursday', 5, 'Friday', 6, 'Saturday', 7, 'Sunday')");
        Ok(())
    })
}

// ---- printf

/// A printf directive (Java's `DIRECTIVE`, `%(%|(0?)([0-9]*)(?:\.([0-9]+))?([sdf]))`) found from `from`: its start,
/// its end, and -- unless it is `%%` -- its zero flag, width digits, precision digits and conversion.
struct Directive<'s> {
    start: usize,
    end: usize,
    spec: Option<(bool, &'s str, Option<&'s str>, char)>,
}

/// Java's `Matcher.find()` of [`Directive`]'s pattern from byte `from` (every character it reads is ASCII).
fn find_directive(f: &str, from: usize) -> Option<Directive<'_>> {
    let b = f.as_bytes();
    let digits = |at: usize| {
        let mut end = at;
        while end < b.len() && b[end].is_ascii_digit() {
            end += 1;
        }
        end
    };
    let mut i = from;
    while i < b.len() {
        if b[i] == b'%' {
            if i + 1 < b.len() && b[i + 1] == b'%' {
                return Some(Directive { start: i, end: i + 2, spec: None });
            }
            // (0?)([0-9]*): greedy, and no shorter split matches where the greedy one does not
            let mut at = i + 1;
            let zero = at < b.len() && b[at] == b'0';
            if zero {
                at += 1;
            }
            let width_end = digits(at);
            let width = &f[at..width_end];
            at = width_end;
            let mut precision = None;
            if at + 1 < b.len() && b[at] == b'.' && b[at + 1].is_ascii_digit() {
                let end = digits(at + 1);
                precision = Some(&f[at + 1..end]);
                at = end;
            }
            if at < b.len() && matches!(b[at], b's' | b'd' | b'f') {
                return Some(Directive {
                    start: i,
                    end: at + 1,
                    spec: Some((zero, width, precision, b[at] as char)),
                });
            }
        }
        i += 1;
    }
    None
}

/// Java's `Integer.parseInt` of a run of ASCII digits.
fn parse_int(digits: &str) -> Result<i32, Refusal> {
    digits.parse::<i32>().map_err(|_| Refusal {
        class: Refusal::NUMBER_FORMAT,
        message: format!("For input string: \"{digits}\""),
    })
}

// ---- regular expressions

/// A pattern as Snowflake reads it, and its parameters (Java's `Pattern`).
struct Pattern<'a> {
    text: P<'a>,
    params: String,
}

/// Java's `RE2_FLAGS.matcher(s).lookingAt()` (`\(\?([ims]+)\)` at the start): the flags and the match's end.
fn re2_flags(s: &str) -> Option<(&str, usize)> {
    let rest = s.strip_prefix("(?")?;
    let n = rest.bytes().take_while(|c| matches!(c, b'i' | b'm' | b's')).count();
    if n == 0 || rest.as_bytes().get(n) != Some(&b')') {
        return None;
    }
    Some((&rest[..n], 2 + n + 1))
}

/// Java's `literal.matches("(?s).*(\\*\\?|\\+\\?|\\?\\?|\\}\\?).*")`: a lazy quantifier anywhere.
fn lazy(literal: &str) -> bool {
    ["*?", "+?", "??", "}?"].iter().any(|q| literal.contains(q))
}

/// The index of the first `)` closing no group the text opens, escapes and bracket expressions skipped (Java's
/// `closingParen`; a byte index here).
fn closing_paren(pattern: &str) -> Option<usize> {
    let mut depth = 0;
    let mut in_class = false;
    let mut chars = pattern.char_indices();
    while let Some((i, ch)) = chars.next() {
        if ch == '\\' {
            chars.next();
        } else if in_class {
            in_class = ch != ']';
        } else if ch == '[' {
            in_class = true;
        } else if ch == '(' {
            depth += 1;
        } else if ch == ')' {
            if depth == 0 {
                return Some(i);
            }
            depth -= 1;
        }
    }
    None
}

/// The capturing groups a pattern opens, escapes and bracket expressions skipped (Java's `capturingGroups`).
fn capturing_groups(pattern: &str) -> i64 {
    let mut n = 0;
    let mut in_class = false;
    let mut chars = pattern.char_indices();
    while let Some((i, ch)) = chars.next() {
        if ch == '\\' {
            chars.next();
        } else if in_class {
            in_class = ch != ']';
        } else if ch == '[' {
            in_class = true;
        } else if ch == '(' && !pattern[i..].starts_with("(?") {
            n += 1;
        }
    }
    n
}

/// Whether a non-capturing group opens before the `group`-th capturing group (Java's `opensBeforeGroup`).
fn opens_before_group(pattern: &str, group: i32) -> bool {
    let mut capturing = 0;
    let mut in_class = false;
    let mut chars = pattern.char_indices();
    while let Some((i, ch)) = chars.next() {
        if ch == '\\' {
            chars.next();
        } else if in_class {
            in_class = ch != ']';
        } else if ch == '[' {
            in_class = true;
        } else if ch == '(' {
            if pattern[i..].starts_with("(?:") {
                return true;
            }
            if pattern[i..].starts_with("(?") {
                continue;
            }
            capturing += 1;
            if capturing >= group {
                return false;
            }
        }
    }
    false
}

/// The group an extraction reads (Java's `groupRead`): its literal index, 0 by default, every group when computed.
fn group_read(a: &[SqlExpr]) -> i32 {
    if a.len() <= 2 {
        return 0;
    }
    match &a[2] {
        SqlExpr::IntLit { value, .. } => (*value).min(i64::from(i32::MAX)) as i32,
        _ => i32::MAX,
    }
}

impl Snowflake {
    pub(super) fn call_arm(
        &self,
        w: &mut SqlWriter,
        f: SqlFn,
        a: &[SqlExpr],
        t: &TypeFact,
        parent_prec: i32,
    ) -> Result<(), Refusal> {
        let a0 = || arg(a, 0);
        let plain = |w: &mut SqlWriter, name: &str| -> Result<(), Refusal> {
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
            SqlFn::IsDistinctFrom | SqlFn::NullSafeNotEqual => ansi::distinct_from(self, w, a),
            SqlFn::NullSafeEqual => ansi::not_distinct_from(self, w, a),
            SqlFn::Xor => ansi::xor_as_and_or(self, w, a, parent_prec),
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
            SqlFn::BoolToText => ansi::cast_as(self, w, a0()?, "VARCHAR"),
            // Pure's divide is a Float division: both operands DOUBLE
            SqlFn::Divide => ansi::divide_as_doubles(self, w, a),
            SqlFn::Mod => ansi::mod_floored(self, w, a),
            SqlFn::Rem => ansi::mod_truncated(self, w, a),
            SqlFn::FloatCanon => self.float_canon(w, a0()?),
            // a double from its digits; any other value through the database's own text of it
            SqlFn::FloatPrint => {
                if is_double(a0()?) {
                    self.float_print(w, a0()?)
                } else {
                    ansi::float_print_by_text(self, w, a, parent_prec)
                }
            }
            // the hooks: this dialect's own
            SqlFn::Round => self.round_half_even(w, a),
            SqlFn::Hash => self.hash_signed(w, a),
            SqlFn::BitAnd | SqlFn::BitOr | SqlFn::BitXor | SqlFn::BitShiftLeft | SqlFn::BitShiftRight => {
                self.bit_op(w, f, a)
            }
            SqlFn::StructInsert => self.struct_insert(w, a),
            SqlFn::PureSplitPart => self.split_part_call(w, a),
            SqlFn::ListExists => self.list_exists(w, a),
            SqlFn::AllDistinct => self.all_distinct(w, a),
            SqlFn::ListForAll => self.list_for_all(w, a),
            SqlFn::ToVariant => self.variant_construct(w, a),
            SqlFn::VariantElements => self.variant_elements(w, a),
            SqlFn::VariantGet => self.variant_get(w, a),
            SqlFn::Unnest => self.unnest_projection(w, a),
            // this database's plain spellings: name(args)
            SqlFn::Abs => plain(w, "abs"),
            SqlFn::AsciiCode => plain(w, "ascii"),
            SqlFn::Atan => plain(w, "atan"),
            SqlFn::Atan2 => plain(w, "atan2"),
            SqlFn::Cbrt => plain(w, "cbrt"),
            SqlFn::Chr => plain(w, "chr"),
            // a text operand beside a JSON one is parsed, and each JSON one read as a VARIANT
            SqlFn::Coalesce => {
                if a.iter().any(is_json) && a.iter().any(is_text) {
                    w.append("coalesce(").join(a, ", ", |w, e| {
                        if is_text(e) {
                            w.append("PARSE_JSON(").expr(self, e, 0)?.append(")");
                        } else if is_json(e) {
                            w.append("TO_VARIANT(").expr(self, e, 0)?.append(")");
                        } else {
                            w.expr(self, e, 0)?;
                        }
                        Ok(())
                    })?;
                    w.append(")");
                    Ok(())
                } else {
                    plain(w, "coalesce")
                }
            }
            SqlFn::Cos => plain(w, "cos"),
            SqlFn::Cosh => plain(w, "cosh"),
            SqlFn::Cot => plain(w, "cot"),
            SqlFn::Degrees => plain(w, "degrees"),
            SqlFn::Exp => plain(w, "exp"),
            SqlFn::FloorRaw => plain(w, "floor"),
            SqlFn::Greatest => plain(w, "greatest_ignore_nulls"),
            SqlFn::Least => plain(w, "least_ignore_nulls"),
            SqlFn::Left => plain(w, "left"),
            SqlFn::Length => plain(w, "length"),
            SqlFn::Levenshtein => plain(w, "editdistance"),
            SqlFn::Ln => plain(w, "ln"),
            SqlFn::Lower => plain(w, "lower"),
            SqlFn::Ltrim => plain(w, "ltrim"),
            SqlFn::MakeDate => plain(w, "date_from_parts"),
            SqlFn::Md5 => plain(w, "md5"),
            SqlFn::Pow => plain(w, "power"),
            SqlFn::Radians => plain(w, "radians"),
            SqlFn::RepeatStr => plain(w, "repeat"),
            SqlFn::Replace => plain(w, "replace"),
            SqlFn::Translate => plain(w, "translate"),
            SqlFn::ReverseString => plain(w, "reverse"),
            SqlFn::Right => plain(w, "right"),
            SqlFn::Rtrim => plain(w, "rtrim"),
            SqlFn::Sha1 => plain(w, "sha1"),
            SqlFn::Sin => plain(w, "sin"),
            SqlFn::Sinh => plain(w, "sinh"),
            SqlFn::SplitPart => plain(w, "split_part"),
            SqlFn::Sqrt => plain(w, "sqrt"),
            SqlFn::StartsWith => plain(w, "startswith"),
            SqlFn::EndsWith => plain(w, "endswith"),
            SqlFn::Substring => plain(w, "substr"),
            SqlFn::Tan => plain(w, "tan"),
            SqlFn::Tanh => plain(w, "tanh"),
            SqlFn::Trim => plain(w, "trim"),
            SqlFn::Upper => plain(w, "upper"),
            SqlFn::ListLength | SqlFn::JsonArrayLength => plain(w, "array_size"),
            SqlFn::ListReverse => plain(w, "array_reverse"),
            SqlFn::MapKeys => plain(w, "object_keys"),
            SqlFn::EncodeBase64 => plain(w, "base64_encode"),
            SqlFn::DecodeBase64 => plain(w, "base64_decode_string"),

            // a list parameter is bound as one array on the base (= ANY(?)): Snowflake binds none
            SqlFn::In => {
                if a.len() == 2
                    && let SqlExpr::PlanParam { name, .. } = &a[1]
                {
                    return Err(Refusal::capability(format!(
                        "list parameter '{name}' reached Snowflake, which binds no array (= ANY(?))"
                    )));
                }
                ansi::in_list(self, w, a, parent_prec)
            }
            // NULL-ignoring, as DuckDB's concat
            SqlFn::Concat => w.piece(&*self.concat(ansi::flatten_concat(a))).map(|_| ()),
            SqlFn::ConcatJoin => w.piece(&*self.concat(a.iter().collect())).map(|_| ()),
            // the engine's out-of-domain answer is NaN; Snowflake raises
            SqlFn::Acos | SqlFn::Asin => {
                let name = if f == SqlFn::Acos { "ACOS" } else { "ASIN" };
                w.append("(CASE WHEN (").expr(self, a0()?, 0)?.append(&format!(") BETWEEN -1 AND 1 THEN {name}("));
                w.expr(self, a0()?, 0)?.append(") ELSE CAST('NaN' AS DOUBLE) END)");
                Ok(())
            }
            SqlFn::Log10 => {
                w.append("LOG(10, ").expr(self, a0()?, 0)?.append(")");
                Ok(())
            }
            // Pure's divide-with-scale is HALF_UP: ROUND over a NUMBER; over a FLOAT in binary
            SqlFn::RoundHalfUp => self.round_half_up(w, a),
            // DuckDB's // truncates toward zero; x // 0 is NULL
            SqlFn::IntDivide => {
                let integers = is_integer(type_of(a0()?)) && is_integer(type_of(arg(a, 1)?));
                w.append(if integers { "CAST(TRUNC(" } else { "TRUNC(" }).expr(self, a0()?, 6)?;
                w.append(" / NULLIF(").expr(self, arg(a, 1)?, 0)?.append(", 0))");
                if integers {
                    w.append(" AS BIGINT)");
                }
                Ok(())
            }
            SqlFn::BitNot => {
                w.append("BITNOT(").expr(self, a0()?, 0)?.append(")");
                Ok(())
            }
            // a 64-bit integer text: CAST rounds a decimal text, so the text is checked first
            SqlFn::ParseInt => {
                w.append("CAST(CASE WHEN REGEXP_LIKE(").expr(self, a0()?, 0)?.append(", ");
                let lit = self.string_lit("\\s*[-+]?[0-9]+\\s*")?;
                w.append(&lit).append(") THEN ").expr(self, a0()?, 0)?.append(" ELSE ").expr(self, a0()?, 0)?;
                w.append(" || ' is not an integer' END AS BIGINT)");
                Ok(())
            }
            // an ISO text with an offset is its UTC instant
            SqlFn::ParseDate => {
                w.append("CAST(CONVERT_TIMEZONE('UTC', TO_TIMESTAMP_TZ(").expr(self, a0()?, 0)?;
                w.append(")) AS TIMESTAMP_NTZ)");
                Ok(())
            }
            SqlFn::Error => w.piece(&*self.raise(a, None)?).map(|_| ()),

            // ---- strings
            SqlFn::Matches => self.matches(w, a),
            SqlFn::RegexpFullMatch => self.full_match(w, a),
            SqlFn::RegexpExtract => self.regexp_extract(w, a),
            SqlFn::RegexpExtractAll => self.regexp_extract_all(w, a),
            SqlFn::RegexpReplace => self.regexp_replace(w, a),
            SqlFn::Split => {
                let p = self.split(a0()?, arg(a, 1)?);
                w.piece(&*p)?;
                Ok(())
            }
            SqlFn::Strpos => {
                w.append("CHARINDEX(").expr(self, arg(a, 1)?, 0)?.append(", ").expr(self, a0()?, 0)?.append(")");
                Ok(())
            }
            SqlFn::Sha256 => {
                w.append("SHA2(").expr(self, a0()?, 0)?.append(", 256)");
                Ok(())
            }
            SqlFn::Guid => {
                w.append("UUID_STRING()");
                Ok(())
            }
            // an integer of 53 bits over 2^53, in [0, 1)
            SqlFn::Random => {
                w.append("(UNIFORM(0, 9007199254740991, RANDOM()) / 9007199254740992::FLOAT)");
                Ok(())
            }
            SqlFn::Format => {
                let p = self.format(a)?;
                w.piece(&*p)?;
                Ok(())
            }
            SqlFn::JaroWinkler => {
                let p = self.jaro_winkler(a0()?, arg(a, 1)?);
                w.piece(&*p)?;
                Ok(())
            }
            SqlFn::CurrentUserFn => {
                w.append("CURRENT_USER()");
                Ok(())
            }

            // ---- temporal
            SqlFn::Now => {
                w.append("CAST(CURRENT_TIMESTAMP() AS TIMESTAMP_NTZ)");
                Ok(())
            }
            SqlFn::Extract => {
                let p = self.extract(a)?;
                w.piece(&*p)?;
                Ok(())
            }
            SqlFn::DateTrunc => {
                let p = self.date_trunc(a)?;
                w.piece(&*p)?;
                Ok(())
            }
            // seconds as a DOUBLE: whole seconds, then the fraction's microseconds
            SqlFn::MakeTimestamp => {
                if a.len() != 6 {
                    return Err(wall(f, &format!("make_timestamp of {} arguments", a.len())));
                }
                w.append("DATEADD(MICROSECOND, ROUND(").expr(self, &a[5], 6)?;
                w.append(" * 1000000), TIMESTAMP_FROM_PARTS(").list(self, &a[..5])?.append(", 0))");
                Ok(())
            }
            // DuckDB's date + interval is a TIMESTAMP at every unit
            SqlFn::AddInterval | SqlFn::AddIntervalTemporal => {
                w.append("CAST(DATEADD(");
                let u = interval_unit(part(a0()?)?)?;
                w.append(u).append(", ").expr(self, arg(a, 1)?, 0)?.append(", ").expr(self, arg(a, 2)?, 0)?;
                w.append(") AS TIMESTAMP_NTZ)");
                Ok(())
            }
            SqlFn::DateDiff => {
                let p = self.date_diff(a)?;
                w.piece(&*p)?;
                Ok(())
            }
            SqlFn::TimeBucket => {
                let p = self.time_bucket(a)?;
                w.piece(&*p)?;
                Ok(())
            }
            // TIMESTAMP_FROM_PARTS(date, time): the date-time at that time of that day
            SqlFn::DateAtTime => plain(w, "TIMESTAMP_NTZ_FROM_PARTS"),
            // DuckDB's timezone(zone, t): a naive t read in the zone; an instant to the zone's time
            SqlFn::Timezone => {
                if instant(arg(a, 1)?) {
                    w.append("CONVERT_TIMEZONE('UTC', ").expr(self, a0()?, 0)?.append(", CAST(");
                    w.expr(self, &a[1], 0)?.append(" AS TIMESTAMP_NTZ))");
                } else {
                    w.append("CAST(CONVERT_TIMEZONE(").expr(self, a0()?, 0)?.append(", 'UTC', CAST(");
                    w.expr(self, &a[1], 0)?.append(" AS TIMESTAMP_NTZ)) AS TIMESTAMP_LTZ)");
                }
                Ok(())
            }
            // DuckDB's epoch(ts) is DOUBLE seconds; epoch_ms truncates toward zero
            SqlFn::EpochSeconds => {
                w.append("(CAST(DATE_PART(EPOCH_MICROSECOND, ").piece(&*self.naive(a0()?))?;
                w.append(") AS DOUBLE) / 1000000)");
                Ok(())
            }
            SqlFn::EpochNs | SqlFn::FromEpochNs => Err(Refusal::capability(
                "a nanosecond epoch: Snowflake keeps microseconds (the Cypher island's nanosecond leg is DuckDB's)",
            )),
            SqlFn::EpochMs => {
                w.append("CAST(TRUNC(DATE_PART(EPOCH_MICROSECOND, ").piece(&*self.naive(a0()?))?;
                w.append(") / 1000) AS BIGINT)");
                Ok(())
            }
            SqlFn::FromEpochSeconds => {
                w.append("TO_TIMESTAMP_NTZ(CAST(ROUND(").expr(self, a0()?, 6)?.append(" * 1000000) AS BIGINT), 6)");
                Ok(())
            }
            SqlFn::FromEpochMs => {
                w.append("TO_TIMESTAMP_NTZ(CAST(").expr(self, a0()?, 0)?.append(" AS BIGINT), 3)");
                Ok(())
            }
            // DAYNAME is 'Sun' here: the full name by the ISO weekday
            SqlFn::Dayname => w.piece(&*weekday_name(self.naive(a0()?))).map(|_| ()),
            SqlFn::Monthname => {
                w.append("TO_CHAR(").piece(&*self.naive(a0()?))?.append(", 'MMMM')");
                Ok(())
            }
            SqlFn::Strftime => {
                let SqlExpr::FormatLit { parts, .. } = arg(a, 1)? else {
                    return Err(wall(f, "a date format that is not a format literal: its codes are DuckDB's"));
                };
                let p = self.strftime(a0()?, parts)?;
                w.piece(&*p)?;
                Ok(())
            }
            SqlFn::Strptime => {
                if !matches!(arg(a, 1)?, SqlExpr::FormatLit { .. }) {
                    return Err(wall(f, "a date format that is not a format literal: its codes are DuckDB's"));
                }
                w.append("TO_TIMESTAMP_NTZ(").expr(self, a0()?, 0)?.append(", ").expr(self, &a[1], 0)?.append(")");
                Ok(())
            }

            // ---- lists
            SqlFn::ListFilter
            | SqlFn::ListTransform
            | SqlFn::ListConcat
            | SqlFn::ListGet
            | SqlFn::ListPosition
            | SqlFn::ListFlatten
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
            | SqlFn::Typeof => self.list(w, f, a, fact_type(t)),
            // DuckDB's range: [start, stop) by step, start 0 and step 1 by default
            SqlFn::RangeFn => {
                if a.len() == 1 {
                    w.append("ARRAY_GENERATE_RANGE(0, ").expr(self, &a[0], 0)?.append(")");
                    Ok(())
                } else {
                    plain(w, "ARRAY_GENERATE_RANGE")
                }
            }
            SqlFn::RepeatValue => {
                let r = self.fresh_name("r");
                w.append("TRANSFORM(ARRAY_GENERATE_RANGE(0, ").expr(self, arg(a, 1)?, 0)?.append("), ");
                let lambda = self.lambda_of(vec![param(&r, Some("INTEGER"))], Vec::new(), self.ex(a0()?, 0));
                w.piece(&*lambda)?.append(")");
                Ok(())
            }

            // ---- maps: OBJECTs (string keys only)
            SqlFn::MapEmpty => {
                w.append("OBJECT_CONSTRUCT()");
                Ok(())
            }
            SqlFn::MapFromLists => self.map_from_lists(w, f, a),
            // DuckDB's map_from_entries: each entry a struct of (key, value), by position whatever their names
            SqlFn::MapFromEntries => self.map_from_entries(w, f, a),
            // DuckDB's map_extract: a list of the key's value, empty when absent
            SqlFn::MapExtract => {
                w.append("(CASE WHEN ").expr(self, a0()?, 4)?.append(" IS NULL THEN NULL WHEN");
                w.append(" ARRAY_CONTAINS(TO_VARIANT(").expr(self, arg(a, 1)?, 0)?.append("), OBJECT_KEYS(");
                w.expr(self, &a[0], 0)?.append(")) THEN ARRAY_CONSTRUCT(COALESCE(GET(").expr(self, &a[0], 0)?;
                w.append(", ").expr(self, &a[1], 0)?.append("), PARSE_JSON('null'))) ELSE ARRAY_CONSTRUCT() END)");
                Ok(())
            }
            // in the keys' order (OBJECT_KEYS sorts them)
            SqlFn::MapValues => {
                let k = self.fresh_name("k");
                let x = a0()?;
                w.append("TRANSFORM(OBJECT_KEYS(").expr(self, x, 0)?.append("), ");
                let dk = self.delimited(&k);
                let body = pc(move |w| {
                    w.append("GET(").expr(self, x, 0)?.append(&format!(", {dk})"));
                    Ok(())
                });
                w.piece(&*self.lambda_of(vec![param(&k, Some("VARCHAR"))], Vec::new(), body))?.append(")");
                Ok(())
            }
            // the later map's keys win, as DuckDB's map_concat
            SqlFn::MapConcat => self.map_concat(w, a),

            // ---- variant
            // in DuckDB's json_type vocabulary; Snowflake reads '1.0' as an INTEGER, so a whole decimal is a BIGINT
            SqlFn::JsonType => {
                w.append("(CASE WHEN ").expr(self, a0()?, 4)?.append(" IS NULL THEN NULL ELSE DECODE(TYPEOF(");
                w.append("TO_VARIANT(").expr(self, &a[0], 0)?;
                w.append(")), 'NULL_VALUE', 'NULL', 'BOOLEAN', 'BOOLEAN',");
                w.append(" 'INTEGER', 'BIGINT', 'DECIMAL', 'DOUBLE', 'DOUBLE', 'DOUBLE', 'ARRAY', 'ARRAY',");
                w.append(" 'OBJECT', 'OBJECT', 'VARCHAR') END)");
                Ok(())
            }
            SqlFn::JsonPretty => Err(wall(f, "Snowflake has no JSON pretty-printer in SQL")),
            // RFC 7386 with a literal object patch of non-null values: each key replaced (OBJECT_INSERT)
            SqlFn::JsonMergePatch => self.json_merge_patch(w, f, a),
        }
    }

    /// Every list function whose first argument is a list (Java's `list`); `result` is the call's own type when known.
    pub(super) fn list(
        &self,
        w: &mut SqlWriter,
        f: SqlFn,
        a: &[SqlExpr],
        result: Option<&SqlType>,
    ) -> Result<(), Refusal> {
        let xs = arg(a, 0)?;
        let element = list_element(xs);
        let xs_ex = || self.ex(xs, 0);
        match f {
            SqlFn::ListFilter => {
                let fl = lambda_arg(arg(a, 1)?)?;
                let carry = self.captured(fl);
                if !carry.is_empty() && fl.params.len() == 1 {
                    let p = self.carried(xs, fl, carry, true)?;
                    w.piece(&*p)?;
                    return Ok(());
                }
                let r = self.ranged(xs, fl, |b| b)?;
                if !r.positions {
                    w.append("FILTER(").piece(&*r.over)?.append(", ").piece(&*r.lambda)?.append(")");
                    return Ok(());
                }
                // the kept positions, then their elements (as the list holds them)
                let j = self.delimited(&self.fresh_name("j"));
                w.append("TRANSFORM(FILTER(").piece(&*r.over)?.append(", ").piece(&*r.lambda)?.append("), ");
                w.append(&format!("{j} INTEGER -> GET(")).expr(self, xs, 0)?.append(&format!(", {j}))"));
                Ok(())
            }
            SqlFn::ListTransform => {
                let tl = lambda_arg(arg(a, 1)?)?;
                let carry = self.captured(tl);
                if !carry.is_empty() && tl.params.len() == 1 {
                    let p = self.carried(xs, tl, carry, false)?;
                    w.piece(&*p)?;
                    return Ok(());
                }
                let r = self.ranged(xs, tl, |b| b)?;
                w.append("TRANSFORM(").piece(&*r.over)?.append(", ").piece(&*r.lambda)?.append(")");
                Ok(())
            }
            // NULL ++ xs is xs, as DuckDB's list_concat
            SqlFn::ListConcat => {
                let mut acc: P<'_> = pc(move |w| {
                    w.append("COALESCE(").expr(self, xs, 0)?.append(", ARRAY_CONSTRUCT())");
                    Ok(())
                });
                for next in &a[1..] {
                    let left = acc;
                    acc = pc(move |w| {
                        w.append("ARRAY_CAT(").piece(&*left)?.append(", COALESCE(").expr(self, next, 0)?;
                        w.append(", ARRAY_CONSTRUCT()))");
                        Ok(())
                    });
                }
                w.piece(&*acc)?;
                Ok(())
            }
            // 1-based, negative from the end, out of range (0 included) NULL, as DuckDB's list_extract
            SqlFn::ListGet => {
                let get = self.list_get(xs, arg(a, 1)?);
                let p = self.typed(get, element.or(result))?;
                w.piece(&*p)?;
                Ok(())
            }
            // DuckDB's list_position: 1-based, NULL when absent
            SqlFn::ListPosition => {
                w.append("(ARRAY_POSITION(TO_VARIANT(").expr(self, arg(a, 1)?, 0)?.append("), ").expr(self, xs, 0)?;
                w.append(") + 1)");
                Ok(())
            }
            // DuckDB's list_distinct drops NULLs
            SqlFn::ListDistinct => {
                w.append("ARRAY_DISTINCT(ARRAY_COMPACT(").expr(self, xs, 0)?.append("))");
                Ok(())
            }
            SqlFn::ListAppend => {
                w.append("ARRAY_APPEND(").expr(self, xs, 0)?.append(", ").expr(self, arg(a, 1)?, 0)?.append(")");
                Ok(())
            }
            SqlFn::ListSum => {
                let p = self.non_empty(xs, self.sum(xs, element, result)?);
                w.piece(&*p)?;
                Ok(())
            }
            SqlFn::ListMin | SqlFn::ListMax => {
                let name = if f == SqlFn::ListMin { "ARRAY_MIN(" } else { "ARRAY_MAX(" };
                let x = xs_ex();
                let read = pc(move |w| {
                    w.append(name).piece(&*x)?.append(")");
                    Ok(())
                });
                let p = self.typed(read, element.or(result))?;
                w.piece(&*p)?;
                Ok(())
            }
            SqlFn::ListAvg => {
                let p = self.non_empty(xs, self.avg_of(xs));
                w.piece(&*p)?;
                Ok(())
            }
            SqlFn::ListMedian => {
                let p = self.median(xs, element, result)?;
                w.piece(&*p)?;
                Ok(())
            }
            SqlFn::ListMode => {
                let p = self.mode(xs, element)?;
                w.piece(&*p)?;
                Ok(())
            }
            SqlFn::ListBoolAnd | SqlFn::ListBoolOr => {
                let p = self.non_empty(xs, self.bool_reduce(xs, f == SqlFn::ListBoolAnd));
                w.piece(&*p)?;
                Ok(())
            }
            SqlFn::ListProduct => {
                let dbl = SqlType::Scalar(Scalar::Double);
                let p = self.non_empty(xs, self.reduce(xs, Some(&dbl), &dbl, "1", "*")?);
                w.piece(&*p)?;
                Ok(())
            }
            // ascending, NULLs last
            SqlFn::ListSort | SqlFn::ListSortDesc => {
                let desc = f == SqlFn::ListSortDesc;
                if let Some(SqlType::Struct { fields }) = element {
                    let p = self.struct_sort(xs, fields, desc)?;
                    w.piece(&*p)?;
                } else {
                    w.append("ARRAY_SORT(").expr(self, xs, 0)?.append(if desc { ", FALSE, FALSE)" } else { ")" });
                }
                Ok(())
            }
            SqlFn::ListTail => {
                w.append("ARRAY_SLICE(").expr(self, xs, 0)?.append(", 1, ARRAY_SIZE(").expr(self, xs, 0)?;
                w.append("))");
                Ok(())
            }
            SqlFn::ListInit => {
                w.append("ARRAY_SLICE(").expr(self, xs, 0)?.append(", 0, -1)");
                Ok(())
            }
            // DuckDB's array_slice: 1-based, both ends inclusive; ARRAY_SLICE is 0-based, its end exclusive
            SqlFn::ListSlice => {
                if negative_literal(arg(a, 1)?) || negative_literal(arg(a, 2)?) {
                    return Err(wall(
                        f,
                        "a slice counted from the end (DuckDB's end is inclusive there, Snowflake's not)",
                    ));
                }
                w.append("ARRAY_SLICE(").expr(self, xs, 0)?.append(", GREATEST(").expr(self, &a[1], 6)?;
                w.append(" - 1, 0), ").expr(self, &a[2], 0)?.append(")");
                Ok(())
            }
            SqlFn::ListFlatten => {
                w.append("ARRAY_FLATTEN(ARRAY_COMPACT(").expr(self, xs, 0)?.append("))");
                Ok(())
            }
            SqlFn::ListZip => Err(wall(f, "ARRAYS_ZIP names its fields $1, $2 (the IR's are 1, 2)")),
            // DuckDB's list_reduce: the first element starts the accumulator, the lambda runs over the rest
            SqlFn::ListReduce => {
                let l = match arg(a, 1)? {
                    SqlExpr::Lambda(l) if l.params.len() == 2 => l,
                    _ => return Err(wall(f, "a reduction lambda of another shape")),
                };
                let Some(t) = self.param_type(element)? else {
                    return Err(wall(f, "a reduction over a list of unknown element type"));
                };
                w.append("REDUCE(ARRAY_SLICE(").expr(self, xs, 0)?.append(", 1, ARRAY_SIZE(").expr(self, xs, 0)?;
                w.append(")), ");
                let first = pc(move |w| {
                    w.append("GET(").expr(self, xs, 0)?.append(", 0)");
                    Ok(())
                });
                w.piece(&*self.typed(first, element)?)?.append(", ");
                let tt = t.clone();
                let body = pc(move |w| {
                    w.append("CAST(").expr(self, &l.body, 0)?.append(&format!(" AS {tt})"));
                    Ok(())
                });
                let params = vec![param(&l.params[0], Some(&t)), param(&l.params[1], Some(&t))];
                w.piece(&*self.lambda_of(params, Vec::new(), body))?.append(")");
                Ok(())
            }
            // TYPEOF of the value as a VARIANT ('DATE', 'TIMESTAMP_NTZ')
            SqlFn::Typeof => {
                w.append("TYPEOF(TO_VARIANT(").expr(self, xs, 0)?.append("))");
                Ok(())
            }
            other => Err(Refusal::illegal_state(format!("not a list call: {other}"))),
        }
    }

    /// `ARRAY_CONTAINS` of FALSE (TRUE) absent (present): DuckDB's bool_and (bool_or) over a list.
    fn bool_reduce<'a>(&'a self, xs: &'a SqlExpr, and: bool) -> P<'a> {
        pc(move |w| {
            w.append(if and { "NOT ARRAY_CONTAINS(TO_VARIANT(FALSE), " } else { "ARRAY_CONTAINS(TO_VARIANT(TRUE), " });
            w.expr(self, xs, 0)?.append(")");
            Ok(())
        })
    }

    /// A list's average: its DOUBLE sum over its non-NULL count (the sum made when written, as Java's piece makes it).
    fn avg_of<'a>(&'a self, xs: &'a SqlExpr) -> P<'a> {
        pc(move |w| {
            let dbl = SqlType::Scalar(Scalar::Double);
            let sum = self.sum(xs, Some(&dbl), Some(&dbl))?;
            w.piece(&*sum)?.append(" / ARRAY_SIZE(ARRAY_COMPACT(").expr(self, xs, 0)?.append("))");
            Ok(())
        })
    }

    /// A list of structs sorted field by field in their declared order, each element sorted as an ARRAY of its
    /// fields carrying the element last (Java's `structSort`).
    fn struct_sort<'a>(
        &'a self,
        xs: &'a SqlExpr,
        fields: &[lite_sql::StructField],
        desc: bool,
    ) -> Result<P<'a>, Refusal> {
        let e = self.delimited(&self.fresh_name("e"));
        let p = self.delimited(&self.fresh_name("p"));
        let mut keys = String::from("ARRAY_CONSTRUCT(");
        for f in fields {
            keys.push_str(&format!("GET({e}, {}), ", self.string_lit(&f.name)?));
        }
        keys.push_str(&e);
        keys.push(')');
        let n = fields.len();
        Ok(pc(move |w| {
            w.append("TRANSFORM(ARRAY_SORT(TRANSFORM(").expr(self, xs, 0)?;
            w.append(&format!(
                ", {e} OBJECT -> {keys}){}), {p} ARRAY -> GET({p}, {n}))",
                if desc { ", FALSE, FALSE" } else { "" }
            ));
            Ok(())
        }))
    }

    /// DuckDB's list_extract (Java's `listGet`): GET is 0-based and raises on a negative index.
    fn list_get<'a>(&'a self, xs: &'a SqlExpr, i: &'a SqlExpr) -> P<'a> {
        if let SqlExpr::IntLit { value: n, .. } = i {
            let n = *n;
            if n > 0 {
                return pc(move |w| {
                    w.append("GET(").expr(self, xs, 0)?.append(&format!(", {})", n - 1));
                    Ok(())
                });
            }
            if n == 0 {
                return text("NULL");
            }
            return pc(move |w| {
                w.append("(CASE WHEN ARRAY_SIZE(").expr(self, xs, 0)?.append(&format!(") + ({n}) >= 0 THEN GET("));
                w.expr(self, xs, 0)?.append(", ARRAY_SIZE(").expr(self, xs, 0)?.append(&format!(") + ({n})) END)"));
                Ok(())
            });
        }
        pc(move |w| {
            w.append("(CASE WHEN ").expr(self, i, 0)?.append(" > 0 THEN GET(").expr(self, xs, 0)?.append(", ");
            w.expr(self, i, 6)?.append(" - 1) WHEN ").expr(self, i, 0)?.append(" < 0 AND ARRAY_SIZE(");
            w.expr(self, xs, 0)?.append(") + ").expr(self, i, 6)?.append(" >= 0 THEN GET(").expr(self, xs, 0)?;
            w.append(", ARRAY_SIZE(").expr(self, xs, 0)?.append(") + ").expr(self, i, 6)?.append(") END)");
            Ok(())
        })
    }

    /// `body` when `xs` holds an element, else NULL (Java's `nonEmpty`).
    fn non_empty<'a>(&'a self, xs: &'a SqlExpr, body: P<'a>) -> P<'a> {
        pc(move |w| {
            w.append("(CASE WHEN ARRAY_SIZE(ARRAY_COMPACT(").expr(self, xs, 0)?.append(")) > 0 THEN ");
            w.piece(&*body)?.append(" END)");
            Ok(())
        })
    }

    /// The sum of `xs`'s non-NULL elements in `acc`, the element's type when the call's is unknown (Java's `sum`).
    fn sum<'a>(&'a self, xs: &'a SqlExpr, element: Option<&SqlType>, acc: Option<&SqlType>) -> Result<P<'a>, Refusal> {
        let Some(t) = acc.or(element) else {
            return Err(Refusal::capability("a sum over a list of unknown element type reached Snowflake"));
        };
        self.reduce(xs, element, t, "0", "+")
    }

    /// `REDUCE(ARRAY_COMPACT(xs), init, (acc T, x E) -> acc op x)` (Java's `reduce`).
    fn reduce<'a>(
        &'a self,
        xs: &'a SqlExpr,
        element: Option<&SqlType>,
        acc: &SqlType,
        init: &str,
        op: &str,
    ) -> Result<P<'a>, Refusal> {
        let t = cast_type_name(self, acc)?;
        let e = self.param_type(element)?;
        let head = format!(
            "), CAST({init} AS {t}), (\"__a\" {t}, \"__x\" {}) -> \"__a\" {op} \"__x\")",
            e.as_deref().unwrap_or(&t)
        );
        Ok(pc(move |w| {
            w.append("REDUCE(ARRAY_COMPACT(").expr(self, xs, 0)?.append(&head);
            Ok(())
        }))
    }

    /// DuckDB's list_median over numbers (Java's `median`).
    fn median<'a>(
        &'a self,
        xs: &'a SqlExpr,
        element: Option<&SqlType>,
        result: Option<&SqlType>,
    ) -> Result<P<'a>, Refusal> {
        if !numeric(element) {
            return Err(Refusal::capability(format!(
                "a list median of {} reached Snowflake (numbers only)",
                type_text(element)
            )));
        }
        let num = match element {
            Some(d @ SqlType::Decimal { .. }) => cast_type_name(self, d)?,
            _ => "DOUBLE".to_owned(),
        };
        let sorted = pc(move |w| {
            w.append("ARRAY_SORT(ARRAY_COMPACT(").expr(self, xs, 0)?.append("))");
            Ok(())
        });
        let n = pc(move |w| {
            w.append("ARRAY_SIZE(ARRAY_COMPACT(").expr(self, xs, 0)?.append("))");
            Ok(())
        });
        let at = {
            let (sorted, n, num) = (sorted.clone(), n.clone(), num.clone());
            pc(move |w| {
                w.append("CAST(GET(").piece(&*sorted)?.append(", TRUNC(").piece(&*n)?;
                w.append(&format!(" / 2)) AS {num})"));
                Ok(())
            })
        };
        let middle = {
            let num = num.clone();
            pc(move |w| {
                w.append("(CASE WHEN ").piece(&*n)?.append(" = 0 THEN NULL WHEN MOD(").piece(&*n)?;
                w.append(", 2) = 1 THEN ").piece(&*at)?.append(" ELSE (CAST(GET(").piece(&*sorted)?;
                w.append(", TRUNC(").piece(&*n)?.append(&format!(" / 2) - 1) AS {num}) + ")).piece(&*at)?;
                w.append(") / 2 END)");
                Ok(())
            })
        };
        let out = match result {
            Some(r) => cast_type_name(self, r)?,
            None => num.clone(),
        };
        if out == num {
            return Ok(middle);
        }
        Ok(pc(move |w| {
            w.append("CAST(").piece(&*middle)?.append(&format!(" AS {out})"));
            Ok(())
        }))
    }

    /// DuckDB's list mode: the most frequent non-NULL element, the earliest-seen on a tie (Java's `mode`).
    fn mode<'a>(&'a self, xs: &'a SqlExpr, element: Option<&SqlType>) -> Result<P<'a>, Refusal> {
        let Some(t) = self.param_type(element)? else {
            return Err(Refusal::capability("a list mode of unknown element type reached Snowflake"));
        };
        let i = self.delimited(&self.fresh_name("i"));
        let a = self.delimited(&self.fresh_name("a"));
        let p = self.delimited(&self.fresh_name("p"));
        let same = format!("(CAST(GET({a}, 4) AS INTEGER) > 0 AND GET({p}, 0) = GET({a}, 3))");
        let count = format!("IFF({same}, CAST(GET({a}, 4) AS INTEGER) + 1, 1)");
        let first = format!("IFF({same}, CAST(GET({a}, 5) AS INTEGER), CAST(GET({p}, 1) AS INTEGER))");
        let better = format!(
            "({count} > CAST(GET({a}, 1) AS INTEGER) OR ({count} = CAST(GET({a}, 1) AS INTEGER) AND {first} < \
             CAST(GET({a}, 2) AS INTEGER)))"
        );
        let step = format!(
            "ARRAY_CONSTRUCT(IFF({better}, GET({p}, 0), GET({a}, 0)), IFF({better}, {count}, CAST(GET({a}, 1) AS \
             INTEGER)), IFF({better}, {first}, CAST(GET({a}, 2) AS INTEGER)), GET({p}, 0), {count}, {first})"
        );
        let compact = pc(move |w| {
            w.append("ARRAY_COMPACT(").expr(self, xs, 0)?.append(")");
            Ok(())
        });
        Ok(pc(move |w| {
            w.append("CAST(GET(REDUCE(ARRAY_SORT(TRANSFORM(ARRAY_GENERATE_RANGE(0, ARRAY_SIZE(").piece(&*compact)?;
            w.append(&format!(")), {i} INTEGER -> ARRAY_CONSTRUCT(CAST(GET(")).piece(&*compact)?;
            w.append(&format!(
                ", {i}) AS {t}), {i}))), ARRAY_CONSTRUCT(NULL, 0, 0, NULL, 0, 0), ({a} ARRAY, {p} ARRAY) -> {step}), \
                 0) AS {t})"
            ));
            Ok(())
        }))
    }

    /// DuckDB's list quantile_cont (Java's `quantile`).
    fn quantile<'a>(&'a self, xs: &'a SqlExpr, element: Option<&SqlType>, q: &'a SqlExpr) -> Result<P<'a>, Refusal> {
        if !numeric(element) {
            return Err(Refusal::capability(format!(
                "a list quantile of {} reached Snowflake (numbers only)",
                type_text(element)
            )));
        }
        let sorted = pc(move |w| {
            w.append("ARRAY_SORT(ARRAY_COMPACT(").expr(self, xs, 0)?.append("))");
            Ok(())
        });
        let n = pc(move |w| {
            w.append("ARRAY_SIZE(ARRAY_COMPACT(").expr(self, xs, 0)?.append("))");
            Ok(())
        });
        let rn = {
            let n = n.clone();
            pc(move |w| {
                w.append("(CAST(").piece(&*n)?.append(" - 1 AS DOUBLE) * CAST(").expr(self, q, 0)?;
                w.append(" AS DOUBLE))");
                Ok(())
            })
        };
        let bound = |f: &'static str| {
            let (sorted, rn) = (sorted.clone(), rn.clone());
            pc(move |w| {
                w.append("CAST(GET(").piece(&*sorted)?.append(&format!(", CAST({f}(")).piece(&*rn)?;
                w.append(") AS INTEGER)) AS DOUBLE)");
                Ok(())
            })
        };
        let lo = bound("FLOOR");
        let hi = bound("CEIL");
        Ok(pc(move |w| {
            w.append("(CASE WHEN ").piece(&*n)?.append(" = 0 THEN NULL ELSE ").piece(&*lo)?.append(" + (");
            w.piece(&*hi)?.append(" - ").piece(&*lo)?.append(") * (").piece(&*rn)?.append(" - FLOOR(").piece(&*rn)?;
            w.append(")) END)");
            Ok(())
        }))
    }

    /// A named aggregate over a list's non-NULL elements, in list order (Java's `reduceCollection`); `t` is the
    /// node's own type.
    pub(super) fn reduce_collection_arm(
        &self,
        w: &mut SqlWriter,
        reducer: AggFn,
        xs: &SqlExpr,
        extras: &[SqlExpr],
        t: &TypeFact,
    ) -> Result<(), Refusal> {
        let element = list_element(xs);
        let result = fact_type(t);
        let p: P<'_> = match reducer {
            AggFn::Sum => self.non_empty(xs, self.sum(xs, element, result)?),
            AggFn::Min | AggFn::Max => {
                let name = if reducer == AggFn::Min { "ARRAY_MIN(" } else { "ARRAY_MAX(" };
                let read = pc(move |w| {
                    w.append(name).expr(self, xs, 0)?.append(")");
                    Ok(())
                });
                self.typed(read, element.or(result))?
            }
            AggFn::Avg => self.non_empty(xs, self.avg_of(xs)),
            AggFn::Count => pc(move |w| {
                w.append("ARRAY_SIZE(ARRAY_COMPACT(").expr(self, xs, 0)?.append("))");
                Ok(())
            }),
            AggFn::Median => self.median(xs, element, result)?,
            AggFn::Mode => self.mode(xs, element)?,
            AggFn::QuantileCont => {
                if extras.len() != 1 {
                    return Err(Refusal::capability(format!(
                        "a list quantile of {} extra arguments reached Snowflake",
                        extras.len()
                    )));
                }
                self.quantile(xs, element, &extras[0])?
            }
            AggFn::BoolAnd | AggFn::BoolOr => self.non_empty(xs, self.bool_reduce(xs, reducer == AggFn::BoolAnd)),
            // joinStrings: the elements' text in list order; NULL over no element, as DuckDB's string_agg
            AggFn::StringAgg => {
                if extras.len() != 1 {
                    return Err(Refusal::capability(format!(
                        "a list string_agg of {} extra arguments reached Snowflake",
                        extras.len()
                    )));
                }
                let sep = &extras[0];
                self.non_empty(
                    xs,
                    pc(move |w| {
                        w.append("ARRAY_TO_STRING(ARRAY_COMPACT(").expr(self, xs, 0)?.append("), ");
                        w.expr(self, sep, 0)?.append(")");
                        Ok(())
                    }),
                )
            }
            AggFn::StddevSamp
            | AggFn::StddevPop
            | AggFn::VarSamp
            | AggFn::VarPop
            | AggFn::Variance
            | AggFn::Stddev => self.moments(xs, reducer),
            AggFn::AnyValue
            | AggFn::List
            | AggFn::QuantileDisc
            | AggFn::Corr
            | AggFn::CovarSamp
            | AggFn::CovarPop
            | AggFn::ArgMax
            | AggFn::ArgMin
            | AggFn::RowNumber
            | AggFn::Rank
            | AggFn::DenseRank
            | AggFn::PercentRank
            | AggFn::CumeDist
            | AggFn::Ntile
            | AggFn::Lag
            | AggFn::Lead
            | AggFn::FirstValue
            | AggFn::LastValue
            | AggFn::NthValue => {
                return Err(Refusal::capability(format!(
                    "a list reduction '{reducer}' reached Snowflake, which reduces a list only with REDUCE and the \
                     ARRAY_* functions"
                )));
            }
            AggFn::Wavg | AggFn::HashList | AggFn::IsDistinctMark | AggFn::UniqueValueOnly => {
                return Err(Refusal::illegal_state(format!("lowering marker {reducer} reached the renderer")));
            }
        };
        w.piece(&*p)?;
        Ok(())
    }

    /// A list's variance or standard deviation, DuckDB's own Welford update (Java's `moments`).
    fn moments<'a>(&'a self, xs: &'a SqlExpr, f: AggFn) -> P<'a> {
        let n = "CAST(GET(\"__a\", 0) AS DOUBLE)";
        let mean = "CAST(GET(\"__a\", 1) AS DOUBLE)";
        let m2 = "CAST(GET(\"__a\", 2) AS DOUBLE)";
        let count = format!("({n} + 1)");
        let next = format!("({mean} + (\"__x\" - {mean}) / {count})");
        let step = format!("ARRAY_CONSTRUCT({count}, {next}, {m2} + (\"__x\" - {next}) * (\"__x\" - {mean}))");
        let read = move |i: usize| -> P<'a> {
            let step = step.clone();
            pc(move |w| {
                w.append("CAST(GET(REDUCE(ARRAY_COMPACT(").expr(self, xs, 0)?;
                w.append(&format!(
                    "), ARRAY_CONSTRUCT(0, 0, 0), (\"__a\" ARRAY, \"__x\" DOUBLE) -> {step}), {i}) AS DOUBLE)"
                ));
                Ok(())
            })
        };
        let sample = matches!(f, AggFn::StddevSamp | AggFn::VarSamp | AggFn::Variance | AggFn::Stddev);
        let root = matches!(f, AggFn::StddevSamp | AggFn::StddevPop | AggFn::Stddev);
        let variance = pc(move |w| {
            w.append("(CASE WHEN ").piece(&*read(0))?.append(" > 1 THEN ").piece(&*read(2))?;
            if sample {
                w.append(" / (").piece(&*read(0))?.append(" - 1) END)");
            } else {
                w.append(" / ").piece(&*read(0))?.append(" WHEN ").piece(&*read(0))?.append(" = 1 THEN 0E0 END)");
            }
            Ok(())
        });
        if !root {
            return variance;
        }
        pc(move |w| {
            w.append("SQRT(").piece(&*variance)?.append(")");
            Ok(())
        })
    }

    /// Pure's fold as `REDUCE(xs, init, (acc T, x E) -> body)` (Java's `foldCall`).
    pub(super) fn fold_call_arm(&self, w: &mut SqlWriter, fold: &SqlExpr) -> Result<(), Refusal> {
        let SqlExpr::FoldCall { source, lambda: l, init, r#type, .. } = fold else { unreachable!("a fold") };
        let Some(t) = fact_type(r#type) else {
            return Err(Refusal::capability("a fold of unknown type reached Snowflake"));
        };
        let ty = cast_type_name(self, t)?;
        let element = list_element(source);
        let acc = param_at(l, 1)?.to_owned();
        let elem_type = self.param_type(element)?;
        let params = vec![param(&acc, Some(&ty)), Param { name: param_at(l, 0)?.to_owned(), r#type: elem_type }];
        w.append("REDUCE(COALESCE(").expr(self, source, 0)?.append(", ARRAY_CONSTRUCT()), CAST(");
        w.expr(self, init, 0)?.append(&format!(" AS {ty}), "));
        let body = {
            let ty = ty.clone();
            pc(move |w| {
                w.append("CAST(").expr(self, &l.body, 0)?.append(&format!(" AS {ty})"));
                Ok(())
            })
        };
        w.piece(&*self.lambda_of(params, Vec::new(), body))?.append(")");
        Ok(())
    }

    /// Pure's splitPart: the non-empty tokens, 1-based, NULL past the end (Java's `splitPartCall`).
    pub(super) fn split_part_arm(&self, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
        let t = self.delimited(&self.fresh_name("t"));
        w.append("(CASE WHEN ").expr(self, arg(a, 2)?, 0)?.append(" > 0 THEN CAST(GET(FILTER(");
        let split = self.split(arg(a, 0)?, arg(a, 1)?);
        w.piece(&*split)?.append(&format!(", {t} VARCHAR -> {t} <> ''), ")).expr(self, &a[2], 6)?;
        w.append(" - 1) AS VARCHAR) END)");
        Ok(())
    }

    /// DuckDB's string_split: a literal delimiter; the empty one splits into characters (Java's `split`).
    fn split<'a>(&'a self, s: &'a SqlExpr, d: &'a SqlExpr) -> P<'a> {
        if let SqlExpr::StringLit { value, .. } = d
            && value.is_empty()
        {
            return pc(move |w| {
                w.append("REGEXP_SUBSTR_ALL(").expr(self, s, 0)?.append(", '.', 1, 1, 's')");
                Ok(())
            });
        }
        pc(move |w| {
            w.append("SPLIT(").expr(self, s, 0)?.append(", ").expr(self, d, 0)?.append(")");
            Ok(())
        })
    }

    /// The `ROUND_HALF_UP` arm (Java's `roundHalfUp`): ROUND over a NUMBER; over a FLOAT in binary.
    fn round_half_up(&self, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
        if !is_double(arg(a, 0)?) {
            w.function(self, "ROUND", a)?;
            return Ok(());
        }
        let (v, scale) = self.scaled(a);
        w.append("(SIGN(").piece(&*v)?.append(") * FLOOR(ABS(").piece(&*v)?.append(") + 0.5)");
        if let Some(scale) = &scale {
            w.append(" / ").piece(&**scale)?;
        }
        w.append(")");
        Ok(())
    }

    /// A Float rounding's operand: `a[0]`, scaled by `POWER(10, a[1])` when a scale is given, and the scale.
    pub(super) fn scaled<'a>(&'a self, a: &'a [SqlExpr]) -> (P<'a>, Option<P<'a>>) {
        let scale: Option<P<'a>> = (a.len() > 1).then(|| {
            pc(move |w| {
                w.append("POWER(10, ").expr(self, &a[1], 0)?.append(")");
                Ok(())
            })
        });
        let v = match &scale {
            None => self.ex(&a[0], 6),
            Some(scale) => {
                let scale = scale.clone();
                pc(move |w| {
                    w.append("(").expr(self, &a[0], 6)?.append(" * ").piece(&*scale)?.append(")");
                    Ok(())
                })
            }
        };
        (v, scale)
    }

    /// A map from a list of text keys and a list of values (Java's `mapFromLists`).
    fn map_from_lists(&self, w: &mut SqlWriter, f: SqlFn, a: &[SqlExpr]) -> Result<(), Refusal> {
        let keys = arg(a, 0)?;
        if !matches!(list_element(keys), Some(SqlType::Scalar(Scalar::Varchar))) {
            return Err(wall(
                f,
                &format!("a map keyed by {} (an OBJECT's keys are text)", type_text(list_element(keys))),
            ));
        }
        let m = self.fresh_name("m");
        let i = self.fresh_name("i");
        w.append("REDUCE(ARRAY_GENERATE_RANGE(0, ARRAY_SIZE(").expr(self, keys, 0)?.append(")), OBJECT_CONSTRUCT(), ");
        let (dm, di) = (self.delimited(&m), self.delimited(&i));
        let values = arg(a, 1);
        let body = pc(move |w| {
            w.append(&format!("OBJECT_INSERT({dm}, CAST(GET(")).expr(self, keys, 0)?;
            w.append(&format!(", {di}) AS VARCHAR), COALESCE(GET(")).expr(self, values.clone()?, 0)?;
            w.append(&format!(", {di}), PARSE_JSON('null')), TRUE)"));
            Ok(())
        });
        w.piece(&*self.lambda_of(vec![param(&m, Some("OBJECT")), param(&i, Some("INTEGER"))], Vec::new(), body))?;
        w.append(")");
        Ok(())
    }

    /// A map from a list of (key, value) structs, by position (Java's `mapFromEntries`).
    fn map_from_entries(&self, w: &mut SqlWriter, f: SqlFn, a: &[SqlExpr]) -> Result<(), Refusal> {
        let entries = arg(a, 0)?;
        let fields = match list_element(entries) {
            Some(SqlType::Struct { fields })
                if fields.len() == 2 && fields[0].r#type == SqlType::Scalar(Scalar::Varchar) =>
            {
                fields
            }
            other => {
                return Err(wall(f, &format!("entries of {} (an OBJECT's keys are text)", type_text(other))));
            }
        };
        let m = self.fresh_name("m");
        let e = self.fresh_name("e");
        let key = self.string_lit(&fields[0].name)?;
        let value = self.string_lit(&fields[1].name)?;
        w.append("REDUCE(COALESCE(").expr(self, entries, 0)?.append(", ARRAY_CONSTRUCT()), OBJECT_CONSTRUCT(), ");
        let (dm, de) = (self.delimited(&m), self.delimited(&e));
        let body = text(format!(
            "OBJECT_INSERT({dm}, CAST(GET({de}, {key}) AS VARCHAR), COALESCE(GET({de}, {value}), PARSE_JSON('null')), \
             TRUE)"
        ));
        w.piece(&*self.lambda_of(vec![param(&m, Some("OBJECT")), param(&e, Some("OBJECT"))], Vec::new(), body))?;
        w.append(")");
        Ok(())
    }

    /// Maps concatenated, the later map's keys winning (Java's `mapConcat`).
    fn map_concat(&self, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
        let first = arg(a, 0)?;
        let mut acc: P<'_> = pc(move |w| {
            w.append("COALESCE(").expr(self, first, 0)?.append(", OBJECT_CONSTRUCT())");
            Ok(())
        });
        for next in &a[1..] {
            let left = acc;
            let m = self.fresh_name("m");
            let k = self.fresh_name("k");
            let (dm, dk) = (self.delimited(&m), self.delimited(&k));
            let body = pc(move |v| {
                v.append(&format!("OBJECT_INSERT({dm}, {dk}, COALESCE(GET(")).expr(self, next, 0)?;
                v.append(&format!(", {dk}), PARSE_JSON('null')), TRUE)"));
                Ok(())
            });
            let lambda = self.lambda_of(vec![param(&m, Some("OBJECT")), param(&k, Some("VARCHAR"))], Vec::new(), body);
            acc = pc(move |w| {
                w.append("REDUCE(COALESCE(OBJECT_KEYS(").expr(self, next, 0)?.append("), ARRAY_CONSTRUCT()), ");
                w.piece(&*left)?.append(", ").piece(&*lambda)?.append(")");
                Ok(())
            });
        }
        w.piece(&*acc)?;
        Ok(())
    }

    /// RFC 7386 with a literal object patch of non-null values (Java's `jsonMergePatch`).
    fn json_merge_patch(&self, w: &mut SqlWriter, f: SqlFn, a: &[SqlExpr]) -> Result<(), Refusal> {
        let kv = match arg(a, 1)? {
            SqlExpr::JsonObject { kv, .. } if kv.len() % 2 == 0 => kv,
            _ => return Err(wall(f, "a merge patch that is not a literal object")),
        };
        let target = arg(a, 0)?;
        let mut acc: P<'_> = self.ex(target, 0);
        for pair in kv.chunks(2) {
            let (k, val) = (&pair[0], &pair[1]);
            if !matches!(k, SqlExpr::StringLit { .. }) || matches!(val, SqlExpr::NullLit { .. }) {
                return Err(wall(f, "a merge patch with a computed key or a null value"));
            }
            let left = acc;
            acc = pc(move |w| {
                w.append("OBJECT_INSERT(").piece(&*left)?.append(", ").expr(self, k, 0)?.append(", ");
                w.expr(self, val, 0)?.append(", TRUE)");
                Ok(())
            });
        }
        w.piece(&*acc)?;
        Ok(())
    }

    /// DuckDB's concat: each argument as text, a NULL one skipped (Java's `concat`).
    fn concat<'a>(&'a self, args: Vec<&'a SqlExpr>) -> P<'a> {
        pc(move |w| {
            w.append("(");
            for (i, x) in args.iter().enumerate() {
                if i > 0 {
                    w.append(" || ");
                }
                if matches!(x, SqlExpr::StringLit { .. }) {
                    w.expr(self, x, 0)?;
                } else if is_text(x) {
                    w.append("COALESCE(").expr(self, x, 0)?.append(", '')");
                } else {
                    w.append("COALESCE(CAST(").expr(self, x, 0)?.append(" AS VARCHAR), '')");
                }
            }
            if args.is_empty() {
                w.append("''");
            }
            w.append(")");
            Ok(())
        })
    }

    /// error(msg[, 'line:col']): the sentinel-wrapped message cast to a type it cannot parse, typed as its slot where
    /// a CASE branch or a projection IS the raise (Java's `raise`).
    pub(super) fn raise<'a>(&'a self, a: &'a [SqlExpr], slot: Option<&SqlType>) -> Result<P<'a>, Refusal> {
        let text_piece = pc(move |w| {
            w.append("CHR(31) || ");
            if a.len() > 1 {
                w.expr(self, &a[1], 0)?.append(" || CHR(30) || ");
            }
            w.append("(").expr(self, arg(a, 0)?, 0)?.append(") || CHR(31)");
            Ok(())
        });
        let number = {
            let text_piece = text_piece.clone();
            pc(move |w| {
                w.append("CAST(").piece(&*text_piece)?.append(" AS INTEGER)");
                Ok(())
            })
        };
        let cast_text = |t: SqlType| -> P<'a> {
            let text_piece = text_piece.clone();
            pc(move |w| {
                w.append("CAST(").piece(&*text_piece)?.append(" AS ");
                let name = cast_type_name(self, &t)?;
                w.append(&name).append(")");
                Ok(())
            })
        };
        let wrap = |head: &'static str, tail: &'static str| -> P<'a> {
            let number = number.clone();
            pc(move |w| {
                w.append(head).piece(&*number)?.append(tail);
                Ok(())
            })
        };
        let Some(slot) = slot else {
            return Ok(wrap("CAST(", " AS VARCHAR)"));
        };
        Ok(match slot {
            SqlType::Scalar(s) => match s {
                Scalar::Integer
                | Scalar::Bigint
                | Scalar::Hugeint
                | Scalar::Double
                | Scalar::Boolean
                | Scalar::Date
                | Scalar::Timestamp
                | Scalar::Timestamptz
                | Scalar::Time => cast_text(slot.clone()),
                Scalar::Json => wrap("TO_VARIANT(", ")"),
                Scalar::Varchar | Scalar::TemporalText | Scalar::DecimalText => wrap("CAST(", " AS VARCHAR)"),
            },
            SqlType::Decimal { .. } => cast_text(slot.clone()),
            SqlType::Array { .. } => wrap("TO_ARRAY(", ")"),
            SqlType::Map { .. } | SqlType::Struct { .. } => wrap("OBJECT_CONSTRUCT('error', ", ")"),
            SqlType::Tagged { .. } => {
                return Err(Refusal::illegal_state(
                    "a tagged value is never raised as: its tag and slots are its query's columns",
                ));
            }
        })
    }

    // ---- format

    /// DuckDB's printf over a LITERAL format, unrolled (Java's `format`).
    fn format<'a>(&'a self, a: &'a [SqlExpr]) -> Result<P<'a>, Refusal> {
        let SqlExpr::StringLit { value: f, .. } = arg(a, 0)? else {
            return Err(Refusal::capability(
                "a computed format reached Snowflake, which has no printf: only a literal one is unrolled",
            ));
        };
        let mut parts: Vec<P<'a>> = Vec::new();
        let mut at = 0;
        let mut next_arg = 1;
        let mut text_run = String::new();
        let mut from = 0;
        while let Some(d) = find_directive(f, from) {
            text_run.push_str(&f[at..d.start]);
            at = d.end;
            from = d.end;
            let Some((zero, width, precision, conversion)) = d.spec else {
                text_run.push('%');
                continue;
            };
            if next_arg >= a.len() {
                return Err(Refusal::capability("a format with more directives than arguments reached Snowflake"));
            }
            if !text_run.is_empty() {
                parts.push(text(self.string_lit(&text_run)?));
                text_run.clear();
            }
            let x = &a[next_arg];
            next_arg += 1;
            let width = if width.is_empty() { 0 } else { parse_int(width)? };
            let value: P<'a> = match conversion {
                's' => {
                    if is_text(x) {
                        self.ex(x, 0)
                    } else {
                        pc(move |w| {
                            w.append("TO_VARCHAR(").expr(self, x, 0)?.append(")");
                            Ok(())
                        })
                    }
                }
                'd' => pc(move |w| {
                    w.append("TO_VARCHAR(").expr(self, x, 0)?.append(")");
                    Ok(())
                }),
                'f' => {
                    let precision = match precision {
                        None => 6,
                        Some(p) => parse_int(p)?,
                    };
                    // a negative value rounding to zero keeps its sign in Java's %f (-0.00); a NUMBER has no -0
                    let number = format!(" AS NUMBER(38, {precision}))");
                    pc(move |w| {
                        w.append("(CASE WHEN ").expr(self, x, 0)?.append(" < 0 AND CAST(").expr(self, x, 0)?;
                        w.append(&number).append(" = 0 THEN '-' ELSE '' END || TO_VARCHAR(CAST(").expr(self, x, 0)?;
                        w.append(&number).append("))");
                        Ok(())
                    })
                }
                other => {
                    return Err(Refusal::illegal_state(format!("the directive pattern admits s, d, f: {other}")));
                }
            };
            if width > 0 && conversion == 'd' && zero {
                let digits = pc(move |w| {
                    w.append("TO_VARCHAR(ABS(").expr(self, x, 0)?.append("))");
                    Ok(())
                });
                parts.push(pc(move |w| {
                    w.append("(CASE WHEN ").expr(self, x, 0)?.append(" < 0 THEN '-' || LPAD(").piece(&*digits)?;
                    w.append(&format!(", {}, '0') ELSE LPAD(", width - 1)).piece(&*digits)?;
                    w.append(&format!(", {width}, '0') END)"));
                    Ok(())
                }));
            } else if width > 0 {
                parts.push(pc(move |w| {
                    w.append("LPAD(").piece(&*value)?.append(&format!(", {width}, ' ')"));
                    Ok(())
                }));
            } else {
                parts.push(value);
            }
        }
        let rest = &f[at..];
        text_run.push_str(rest);
        if rest.contains('%') || f.replace("%%", "").chars().filter(|&c| c == '%').count() != a.len() - 1 {
            return Err(Refusal::capability(format!(
                "a format directive other than %s, %d and %f reached Snowflake: {f}"
            )));
        }
        if !text_run.is_empty() || parts.is_empty() {
            parts.push(text(self.string_lit(&text_run)?));
        }
        Ok(pc(move |w| {
            w.append("(").join(&parts, " || ", |v, part| part(v))?.append(")");
            Ok(())
        }))
    }

    // ---- regular-expression calls (each pattern through `pattern`)

    fn matches(&self, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
        let p = self.pattern(arg(a, 1)?, 0)?;
        w.append("(REGEXP_INSTR(").expr(self, arg(a, 0)?, 0)?.append(", ").piece(&*p.text)?;
        let params = self.string_lit(&p.params)?;
        w.append(", 1, 1, 0, ").append(&params).append(") > 0)");
        Ok(())
    }

    fn full_match(&self, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
        let p = self.pattern(arg(a, 1)?, 0)?;
        w.append("REGEXP_LIKE(").expr(self, arg(a, 0)?, 0)?.append(", ").piece(&*p.text)?.append(", ");
        let params = self.string_lit(&p.params)?;
        w.append(&params).append(")");
        Ok(())
    }

    /// DuckDB's regexp_extract(s, p[, g]): group g of the first match, '' when none (Java's `regexpExtract`).
    fn regexp_extract(&self, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
        if let Some(before) = self.text_before_match(a)? {
            w.piece(&*before)?;
            return Ok(());
        }
        let p = self.pattern(arg(a, 1)?, group_read(a))?;
        w.append("COALESCE(REGEXP_SUBSTR(").expr(self, arg(a, 0)?, 0)?.append(", ").piece(&*p.text)?;
        let params = self.string_lit(&format!("{}e", p.params))?;
        w.append(", 1, 1, ").append(&params).append(", ");
        if a.len() > 2 {
            w.expr(self, &a[2], 0)?;
        } else {
            w.append("0");
        }
        w.append("), '')");
        Ok(())
    }

    fn regexp_extract_all(&self, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
        let p = self.pattern(arg(a, 1)?, group_read(a))?;
        w.append("REGEXP_SUBSTR_ALL(").expr(self, arg(a, 0)?, 0)?.append(", ").piece(&*p.text)?;
        let params = self.string_lit(&format!("{}e", p.params))?;
        w.append(", 1, 1, ").append(&params).append(", ");
        if a.len() > 2 {
            w.expr(self, &a[2], 0)?;
        } else {
            w.append("0");
        }
        w.append(")");
        Ok(())
    }

    /// DuckDB replaces the first match unless 'g'; \1 is a group in both (Java's `regexpReplace`).
    fn regexp_replace(&self, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
        let options = match a.get(3) {
            Some(SqlExpr::StringLit { value, .. }) => value.as_str(),
            _ => "",
        };
        if a.len() > 3 && !matches!(a[3], SqlExpr::StringLit { .. }) {
            return Err(wall(SqlFn::RegexpReplace, "computed regexp options"));
        }
        // a replacement that reads no group reads none of the pattern's numbering
        let reads_groups = match arg(a, 2)? {
            SqlExpr::StringLit { value, .. } => {
                value.as_bytes().windows(2).any(|p| p[0] == b'\\' && p[1].is_ascii_digit())
            }
            _ => true,
        };
        let p = self.pattern(arg(a, 1)?, if reads_groups { i32::MAX } else { 0 })?;
        let params = format!("{}{}", p.params, if options.contains('i') { "i" } else { "" });
        w.append("REGEXP_REPLACE(").expr(self, arg(a, 0)?, 0)?.append(", ").piece(&*p.text)?.append(", ");
        w.expr(self, &a[2], 0)?.append(&format!(", 1, {}, ", if options.contains('g') { 0 } else { 1 }));
        let lit = self.string_lit(&params)?;
        w.append(&lit).append(")");
        Ok(())
    }

    /// A platform pattern (RE2's) as Snowflake's POSIX ERE reads it (Java's `pattern`): inline flags as parameters, a
    /// non-capturing group a plain one where no read group follows it; a lazy quantifier, a lookaround or a named group
    /// refused by name.
    fn pattern<'a>(&'a self, p: &'a SqlExpr, group: i32) -> Result<Pattern<'a>, Refusal> {
        let mut literal = match p {
            SqlExpr::StringLit { value, .. } => Some(value.as_str()),
            _ => None,
        };
        let mut flags = "";
        let mut body = p;
        if let Some(l) = literal
            && let Some((f, end)) = re2_flags(l)
        {
            flags = f;
            literal = Some(&l[end..]);
        } else if let SqlExpr::Call { r#fn: SqlFn::Concat, args, .. } = p
            && args.len() == 2
            && let SqlExpr::StringLit { value, .. } = &args[0]
            && let Some((f, end)) = re2_flags(value)
            && end == value.len()
        {
            flags = f;
            body = &args[1];
        }
        let params = format!(
            "c{}{}{}",
            if flags.contains('i') { "i" } else { "" },
            if flags.contains('m') { "m" } else { "" },
            if flags.contains('s') { "s" } else { "" }
        );
        let Some(literal) = literal else {
            return Ok(Pattern { text: self.ex(body, 0), params });
        };
        if lazy(literal) {
            return Err(Refusal::capability(format!(
                "a lazy quantifier reached Snowflake, whose POSIX regular expressions are greedy (measured): {literal}"
            )));
        }
        let mut plain = literal.to_owned();
        if plain.contains("(?:") {
            // a non-capturing group made a plain one renumbers only the groups opened after it
            if group > 0 && opens_before_group(&plain, group) {
                return Err(Refusal::capability(format!(
                    "a non-capturing group in a pattern whose groups are numbered reached Snowflake (no (?: here, \
                     measured): {literal}"
                )));
            }
            plain = plain.replace("(?:", "(");
        }
        if plain.contains("(?") {
            return Err(Refusal::capability(format!(
                "a lookaround, flag group or named group reached Snowflake's POSIX regular expressions: {literal}"
            )));
        }
        Ok(Pattern { text: text(self.string_lit(&plain)?), params })
    }

    /// The text before a match, or before a group of it: the lowering's regexpIndexOf as the subject cut at
    /// REGEXP_INSTR's position (Java's `textBeforeMatch`); none for any other extraction.
    fn text_before_match<'a>(&'a self, a: &'a [SqlExpr]) -> Result<Option<P<'a>>, Refusal> {
        if a.len() != 3 {
            return Ok(None);
        }
        let (SqlExpr::IntLit { value: 1, .. }, SqlExpr::StringLit { value: lit, .. }) = (&a[2], &a[1]) else {
            return Ok(None);
        };
        let mut s = lit.as_str();
        let mut flags = String::new();
        while let Some((f, end)) = re2_flags(s) {
            flags.push_str(f);
            s = &s[end..];
        }
        let head = "^((?:.*?)";
        let Some(rest) = s.strip_prefix(head) else {
            return Ok(None);
        };
        let Some(close) = closing_paren(rest) else {
            return Ok(None);
        };
        let b = &rest[..close];
        let f = &rest[close + 1..];
        let whole = b.is_empty() && f.starts_with("(?:") && f.ends_with(')');
        let original = if whole { f[3..f.len() - 1].to_owned() } else { format!("{b}{f}") };
        let group = if whole { 0 } else { capturing_groups(b) + 1 };
        let prefix = if flags.is_empty() { String::new() } else { format!("(?{flags})") };
        let rebuilt = crate::build::string_lit(&format!("{prefix}{original}"));
        let group = group as i32;
        let p = self.pattern_owned(rebuilt, group)?;
        let x = &a[0];
        let params = self.string_lit(&p.1)?;
        let tail = if group > 0 { format!(", {group}") } else { String::new() };
        let pattern_text = p.0;
        Ok(Some(pc(move |w| {
            w.append("COALESCE(LEFT(").expr(self, x, 0)?.append(", NULLIF(REGEXP_INSTR(").expr(self, x, 0)?;
            w.append(", ").append(&pattern_text).append(", 1, 1, 0, ").append(&params).append(&tail);
            w.append("), 0) - 1), '')");
            Ok(())
        })))
    }

    /// [`Snowflake::pattern`] of a string literal the caller made: its spelled text and its parameters.
    fn pattern_owned(&self, lit: SqlExpr, group: i32) -> Result<(String, String), Refusal> {
        let p = self.pattern(&lit, group)?;
        let mut w = SqlWriter::new();
        w.piece(&*p.text)?;
        Ok((w.into_text()?, p.params))
    }

    // ---- variant

    /// A key (a member) or a 0-based index (-1 the last, past the end NULL) of a variant (Java's `variantGet`).
    pub(super) fn variant_get_arm(&self, w: &mut SqlWriter, args: &[SqlExpr]) -> Result<(), Refusal> {
        let v = arg(args, 0)?;
        let key = arg(args, 1)?;
        if let SqlExpr::StringLit { value, .. } = key
            && value == "$"
        {
            w.expr(self, v, 0)?;
            return Ok(());
        }
        if let SqlExpr::StringLit { value, .. } = key
            && value == "$.*"
        {
            let k = self.delimited(&self.fresh_name("k"));
            w.append("(CASE WHEN IS_OBJECT(TO_VARIANT(").expr(self, v, 0)?.append(")) THEN TRANSFORM(OBJECT_KEYS(");
            w.expr(self, v, 0)?.append(&format!("), {k} VARCHAR -> GET(")).expr(self, v, 0)?;
            w.append(&format!(", {k})) ELSE AS_ARRAY(TO_VARIANT(")).expr(self, v, 0)?.append(")) END)");
            return Ok(());
        }
        if is_text(key) {
            w.append("GET(").expr(self, v, 0)?.append(", ").expr(self, key, 0)?.append(")");
            return Ok(());
        }
        // a literal index is checked when the statement compiles, in a branch never taken too: its branch is chosen
        if let SqlExpr::IntLit { value: n, .. } = key {
            let n = *n;
            if n >= 0 {
                w.append("GET(").expr(self, v, 0)?.append(&format!(", {n})"));
            } else {
                w.append("(CASE WHEN ARRAY_SIZE(").expr(self, v, 0)?.append(&format!(") + ({n}) >= 0 THEN GET("));
                w.expr(self, v, 0)?.append(", ARRAY_SIZE(").expr(self, v, 0)?.append(&format!(") + ({n})) END)"));
            }
            return Ok(());
        }
        w.append("(CASE WHEN ").expr(self, key, 0)?.append(" >= 0 THEN GET(").expr(self, v, 0)?.append(", ");
        w.expr(self, key, 0)?.append(") WHEN ARRAY_SIZE(").expr(self, v, 0)?.append(") + ").expr(self, key, 6)?;
        w.append(" >= 0 THEN GET(").expr(self, v, 0)?.append(", ARRAY_SIZE(").expr(self, v, 0)?.append(") + ");
        w.expr(self, key, 6)?.append(") END)");
        Ok(())
    }

    /// A cast, with a VARIANT on either side (Java's `variantAwareCast`).
    pub(super) fn variant_aware_cast_arm(
        &self,
        w: &mut SqlWriter,
        v: &SqlExpr,
        target: &SqlType,
        conform: bool,
    ) -> Result<(), Refusal> {
        let target_is = |s: Scalar| matches!(target, SqlType::Scalar(t) if *t == s);
        if target_is(Scalar::TemporalText) || target_is(Scalar::DecimalText) {
            return ansi::variant_aware_cast(self, w, v, target, conform);
        }
        // text parses into a VARIANT; any other value converts
        if target_is(Scalar::Json) {
            w.append(if is_text(v) { "PARSE_JSON(" } else { "TO_VARIANT(" }).expr(self, v, 0)?.append(")");
            return Ok(());
        }
        if is_json(v) {
            let member = matches!(v, SqlExpr::Call { r#fn: SqlFn::VariantGet, .. });
            // the whole value as text is its JSON; a member's text is its own: a string unquoted
            if target_is(Scalar::Varchar) && !member {
                w.append("TO_JSON(").expr(self, v, 0)?.append(")");
                return Ok(());
            }
            if matches!(target, SqlType::Array { .. }) {
                w.append("AS_ARRAY(").expr(self, v, 0)?.append(")");
                return Ok(());
            }
            if matches!(target, SqlType::Map { .. } | SqlType::Struct { .. }) {
                w.append("AS_OBJECT(").expr(self, v, 0)?.append(")");
                return Ok(());
            }
            return ansi::variant_aware_cast(self, w, v, target, conform);
        }
        if let SqlType::Array { element } = target {
            let from = list_element(v);
            if Some(element.as_ref()) == from || **element == SqlType::Scalar(Scalar::Json) {
                w.expr(self, v, 0)?;
                return Ok(());
            }
            // each element converted (the elements are VARIANTs: a read casts, but the cell must hold the type)
            let e = self.delimited(&self.fresh_name("c"));
            w.append("TRANSFORM(").expr(self, v, 0)?;
            let name = cast_type_name(self, element)?;
            w.append(&format!(", {e} -> CAST({e} AS {name}))"));
            return Ok(());
        }
        if matches!(target, SqlType::Map { .. } | SqlType::Struct { .. }) {
            w.expr(self, v, 0)?;
            return Ok(());
        }
        // text to an instant: DuckDB reads a +HHMM offset, Snowflake only +HH:MM
        if target_is(Scalar::Timestamptz) && is_text(v) {
            w.append("CAST(REGEXP_REPLACE(").expr(self, v, 0)?;
            w.append(r", '([+-][0-9]{2})([0-9]{2})$', '\\1:\\2') AS TIMESTAMP_LTZ)");
            return Ok(());
        }
        if target_is(Scalar::Varchar) && is_double(v) {
            return self.float_text(w, v);
        }
        // DuckDB's timestamp text: the seconds' fraction to its last nonzero digit, none when zero
        if target_is(Scalar::Varchar) && is_timestamp(v) {
            w.append("REGEXP_REPLACE(TO_CHAR(").expr(self, v, 0)?;
            w.append(r", 'YYYY-MM-DD HH24:MI:SS.FF6'), '\\.?0+$', '')");
            return Ok(());
        }
        ansi::variant_aware_cast(self, w, v, target, conform)
    }

    // ---- temporal helpers

    /// A temporal operand as a TIMESTAMP_NTZ (Java's `naive`).
    fn naive<'a>(&'a self, e: &'a SqlExpr) -> P<'a> {
        if is_timestamp(e) {
            return self.ex(e, 0);
        }
        pc(move |w| {
            w.append("CAST(").expr(self, e, 0)?.append(" AS TIMESTAMP_NTZ)");
            Ok(())
        })
    }

    /// A date part of a date, a Pure integer (Java's `extract`). Weeks are ISO's.
    fn extract<'a>(&'a self, a: &'a [SqlExpr]) -> Result<P<'a>, Refusal> {
        let p = part(arg(a, 0)?)?;
        let x = arg(a, 1)?;
        Ok(match p {
            DatePart::Year | DatePart::Quarter | DatePart::Month | DatePart::Day => {
                let head = format!("DATE_PART({}, ", unit(p)?);
                pc(move |w| {
                    w.append(&head).expr(self, x, 0)?.append(")");
                    Ok(())
                })
            }
            DatePart::Week => pc(move |w| {
                w.append("WEEKISO(").expr(self, x, 0)?.append(")");
                Ok(())
            }),
            DatePart::DayOfYear => pc(move |w| {
                w.append("DAYOFYEAR(").expr(self, x, 0)?.append(")");
                Ok(())
            }),
            DatePart::IsoDayOfWeek => pc(move |w| {
                w.append("DAYOFWEEKISO(").expr(self, x, 0)?.append(")");
                Ok(())
            }),
            DatePart::Hour | DatePart::Minute | DatePart::Second => {
                let head = format!("DATE_PART({}, ", unit(p)?);
                pc(move |w| {
                    w.append(&head).piece(&*self.naive(x))?.append(")");
                    Ok(())
                })
            }
            // DuckDB counts the second's milli/microseconds with the seconds
            DatePart::Millisecond => pc(move |w| {
                w.append("(DATE_PART(SECOND, ").piece(&*self.naive(x))?;
                w.append(") * 1000 + TRUNC(DATE_PART(NANOSECOND, ").piece(&*self.naive(x))?.append(") / 1000000))");
                Ok(())
            }),
            DatePart::Microsecond => pc(move |w| {
                w.append("(DATE_PART(SECOND, ").piece(&*self.naive(x))?;
                w.append(") * 1000000 + TRUNC(DATE_PART(NANOSECOND, ").piece(&*self.naive(x))?.append(") / 1000))");
                Ok(())
            }),
            DatePart::Nanosecond => {
                return Err(Refusal::capability(format!("date part {p} has no measured Snowflake spelling")));
            }
        })
    }

    /// A date-grained truncation a DATE, a finer one a TIMESTAMP (Java's `dateTrunc`). WEEK starts on Monday.
    fn date_trunc<'a>(&'a self, a: &'a [SqlExpr]) -> Result<P<'a>, Refusal> {
        let p = part(arg(a, 0)?)?;
        let x = arg(a, 1)?;
        let u = unit(p)?.to_lowercase();
        Ok(match p {
            DatePart::Year | DatePart::Quarter | DatePart::Month | DatePart::Week => pc(move |w| {
                w.append(&format!("CAST(DATE_TRUNC('{u}', ")).expr(self, x, 0)?.append(") AS DATE)");
                Ok(())
            }),
            DatePart::Day | DatePart::Hour | DatePart::Minute | DatePart::Second | DatePart::Millisecond
            | DatePart::Microsecond => pc(move |w| {
                w.append(&format!("DATE_TRUNC('{u}', ")).piece(&*self.naive(x))?.append(")");
                Ok(())
            }),
            DatePart::Nanosecond | DatePart::DayOfYear | DatePart::IsoDayOfWeek => {
                return Err(Refusal::capability(format!("date_trunc part {p} has no probed Snowflake spelling")));
            }
        })
    }

    /// DuckDB's date_diff counts boundaries crossed, as DATEDIFF does; its week is the day count over 7 (Java's
    /// `dateDiff`).
    fn date_diff<'a>(&'a self, a: &'a [SqlExpr]) -> Result<P<'a>, Refusal> {
        let p = part(arg(a, 0)?)?;
        let from = arg(a, 1)?;
        let to = arg(a, 2)?;
        Ok(match p {
            DatePart::Year | DatePart::Quarter | DatePart::Month | DatePart::Day => {
                let head = format!("DATEDIFF({}, ", unit(p)?);
                pc(move |w| {
                    w.append(&head).expr(self, from, 0)?.append(", ").expr(self, to, 0)?.append(")");
                    Ok(())
                })
            }
            DatePart::Week => pc(move |w| {
                w.append("CAST(TRUNC(DATEDIFF(DAY, ").expr(self, from, 0)?.append(", ").expr(self, to, 0)?;
                w.append(") / 7) AS BIGINT)");
                Ok(())
            }),
            DatePart::Hour | DatePart::Minute | DatePart::Second | DatePart::Millisecond | DatePart::Microsecond => {
                let head = format!("DATEDIFF({}, ", unit(p)?);
                pc(move |w| {
                    w.append(&head).piece(&*self.naive(from))?.append(", ").piece(&*self.naive(to))?.append(")");
                    Ok(())
                })
            }
            DatePart::Nanosecond | DatePart::DayOfYear | DatePart::IsoDayOfWeek => {
                return Err(Refusal::capability(format!("date_diff part {p} has no probed Snowflake spelling")));
            }
        })
    }

    /// DuckDB's time_bucket over its origins is TIME_SLICE's own (Java's `timeBucket`).
    fn time_bucket<'a>(&'a self, a: &'a [SqlExpr]) -> Result<P<'a>, Refusal> {
        let u = part(arg(a, 0)?)?;
        let slice = match u {
            DatePart::Year
            | DatePart::Month
            | DatePart::Week
            | DatePart::Day
            | DatePart::Hour
            | DatePart::Minute
            | DatePart::Second => unit(u)?,
            DatePart::Quarter
            | DatePart::Millisecond
            | DatePart::Microsecond
            | DatePart::Nanosecond
            | DatePart::DayOfYear
            | DatePart::IsoDayOfWeek => {
                return Err(Refusal::capability(format!("a {u} time bucket has no Snowflake TIME_SLICE unit")));
            }
        };
        Ok(pc(move |w| {
            w.append("TIME_SLICE(").piece(&*self.naive(arg(a, 2)?))?.append(", ").expr(self, arg(a, 1)?, 0)?;
            w.append(&format!(", '{slice}', 'START')"));
            Ok(())
        }))
    }

    /// A DuckDB strftime as TO_CHAR pieces (Java's `strftime`): a run of codes and text is one TO_CHAR; a weekday
    /// name or an unpadded hour (no TO_CHAR code) is its own piece.
    fn strftime<'a>(&'a self, value: &'a SqlExpr, parts: &[DateFmt]) -> Result<P<'a>, Refusal> {
        let x = self.naive(value);
        let mut pieces: Vec<P<'a>> = Vec::new();
        let mut run = String::new();
        for p in parts {
            match p {
                DateFmt::Part(DateFmtPart::WeekdayName) => {
                    self.flush_run(&mut pieces, &mut run, &x)?;
                    pieces.push(weekday_name(x.clone()));
                }
                DateFmt::Part(DateFmtPart::Hour12Nopad) => {
                    self.flush_run(&mut pieces, &mut run, &x)?;
                    let x = x.clone();
                    pieces.push(pc(move |w| {
                        w.append("LTRIM(TO_CHAR(").piece(&*x)?.append(", 'HH12'), '0')");
                        Ok(())
                    }));
                }
                DateFmt::Text { s } if s.contains('"') => {
                    self.flush_run(&mut pieces, &mut run, &x)?;
                    pieces.push(text(self.string_lit(s)?));
                }
                other => run.push_str(&format_code(other)?),
            }
        }
        self.flush_run(&mut pieces, &mut run, &x)?;
        if pieces.is_empty() {
            return Ok(text("''"));
        }
        Ok(pc(move |w| {
            w.append("(").join(&pieces, " || ", |v, p| p(v))?.append(")");
            Ok(())
        }))
    }

    fn flush_run<'a>(&self, pieces: &mut Vec<P<'a>>, run: &mut String, x: &P<'a>) -> Result<(), Refusal> {
        if !run.is_empty() {
            let fmt = self.string_lit(run)?;
            let x = x.clone();
            pieces.push(pc(move |w| {
                w.append("TO_CHAR(").piece(&*x)?.append(&format!(", {fmt})"));
                Ok(())
            }));
            run.clear();
        }
        Ok(())
    }
}
