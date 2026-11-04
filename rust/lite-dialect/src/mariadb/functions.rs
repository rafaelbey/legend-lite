// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! MariaDB's and MySQL's scalar functions: Java's `call` -- every `SqlFn` decided by an exhaustive match (MySql's
//! `call` delegates every one) -- with the helpers its arms name: the raise kept in a session variable, the arithmetic,
//! the strings, printf (MariaDB's SFORMAT, MySQL's pieces), the regular expressions (PCRE's and ICU's replacement),
//! the lists, the maps and the temporal helpers (MariaDb.java, "Scalar functions" to "Hooks of the base").

use super::json::{cast_target, utc_of};
use super::{
    MariaDb, P, composite, fact_element, fact_type, index_of, is_double, lambda_body, list_element, offset_of, pc,
    text, type_of,
};
use crate::ansi::{self, part};
use crate::build;
use crate::pieces::Ex;
use crate::refusal::{Refusal, arg};
use crate::renderer::AnsiSqlRenderer;
use crate::writer::SqlWriter;
use lite_javacompat::character;
use lite_sql::{AggFn, DateFmt, DateFmtPart, DatePart, Scalar, SqlExpr, SqlFn, SqlType, TypeFact};

/// The zones whose wall time is UTC's (Java's `UTC_ZONES`).
const UTC_ZONES: &[&str] = &["UTC", "GMT", "Etc/UTC", "Z"];

impl MariaDb {
    /// Java's `call`: every function as this dialect spells it.
    pub(super) fn call_arm(
        &self,
        w: &mut SqlWriter,
        f: SqlFn,
        a: &[SqlExpr],
        t: &TypeFact,
        parent_prec: i32,
    ) -> Result<(), Refusal> {
        use SqlFn as F;
        let call_type = fact_type(t);
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
            F::Xor => ansi::xor_as_and_or(self, w, a, parent_prec),
            F::UcFirst => ansi::first_char_cased(self, w, "upper", a, parent_prec),
            F::LcFirst => ansi::first_char_cased(self, w, "lower", a, parent_prec),
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
            F::Abs => function(self, w, "abs", a),
            F::Atan => function(self, w, "atan", a),
            F::Atan2 => function(self, w, "atan2", a),
            F::Cos => function(self, w, "cos", a),
            F::Cot => function(self, w, "cot", a),
            F::Sin => function(self, w, "sin", a),
            F::Tan => function(self, w, "tan", a),
            F::Degrees => function(self, w, "degrees", a),
            F::Radians => function(self, w, "radians", a),
            F::Exp => function(self, w, "exp", a),
            F::Ln => function(self, w, "ln", a),
            F::Log10 => function(self, w, "log10", a),
            F::Sqrt => function(self, w, "sqrt", a),
            F::Pow => function(self, w, "POW", a),
            F::Lower => function(self, w, "lower", a),
            F::Upper => function(self, w, "upper", a),
            F::Replace => function(self, w, "replace", a),
            F::Translate => Err(self.wall(f, &format!("{} has no translate function", self.product()))),
            F::ReverseString => function(self, w, "reverse", a),
            F::FloorRaw => function(self, w, "floor", a),
            F::Strpos => function(self, w, "INSTR", a),
            F::ListLength | F::JsonArrayLength => function(self, w, "JSON_LENGTH", a),
            F::In => {
                if a.len() == 2
                    && let SqlExpr::PlanParam { name, .. } = &a[1]
                {
                    return Err(self.list_parameter(name));
                }
                ansi::in_list(self, w, a, parent_prec)
            }
            // a Pure integer (no CAST to BIGINT here: SIGNED)
            F::Ceiling => wrapped(self, w, "CAST(CEIL(", arg(a, 0)?, ") AS SIGNED)"),
            F::Floor => wrapped(self, w, "CAST(FLOOR(", arg(a, 0)?, ") AS SIGNED)"),
            F::Sign => wrapped(self, w, "CAST(SIGN(", arg(a, 0)?, ") AS SIGNED)"),
            // PI() is shown at six digits: its DOUBLE in full
            F::Pi => {
                w.append("(PI() + 0.0E0)");
                Ok(())
            }
            // the NULL-safe comparison is <=> here (no IS [NOT] DISTINCT FROM)
            F::NullSafeEqual => {
                w.append("(").expr(self, arg(a, 0)?, 4)?.append(" <=> ").expr(self, arg(a, 1)?, 4)?.append(")");
                Ok(())
            }
            F::IsDistinctFrom | F::NullSafeNotEqual => {
                w.append("(NOT (").expr(self, arg(a, 0)?, 4)?.append(" <=> ").expr(self, arg(a, 1)?, 4)?.append("))");
                Ok(())
            }
            // no hyperbolic functions: their definitions
            F::Cosh => {
                let x = arg(a, 0)?;
                w.append("((EXP(").expr(self, x, 0)?.append(") + EXP(-(").expr(self, x, 0)?.append("))) / 2)");
                Ok(())
            }
            F::Sinh => {
                let x = arg(a, 0)?;
                w.append("((EXP(").expr(self, x, 0)?.append(") - EXP(-(").expr(self, x, 0)?.append("))) / 2)");
                Ok(())
            }
            F::Tanh => {
                let x = arg(a, 0)?;
                w.append("((EXP(2 * (").expr(self, x, 0)?.append(")) - 1) / (EXP(2 * (").expr(self, x, 0)?;
                w.append(")) + 1))");
                Ok(())
            }
            // DuckDB's ascii: the first character's code point, 0 for ''
            F::AsciiCode => {
                let x = arg(a, 0)?;
                w.append("(CASE WHEN ").expr(self, x, 4)?.append(" IS NULL THEN NULL WHEN CHAR_LENGTH(");
                w.expr(self, x, 0)?.append(") = 0 THEN 0 ELSE CAST(CONV(HEX(CONVERT(LEFT(").expr(self, x, 0)?;
                w.append(", 1) USING utf32)), 16, 10) AS SIGNED) END)");
                Ok(())
            }
            F::Chr => wrapped(self, w, "CONVERT(CHAR(", arg(a, 0)?, " USING utf32) USING utf8mb4)"),
            // outside [-1, 1] MariaDB answers NULL, DuckDB raises
            F::Acos => self.domain_guarded(w, f, a, "ACOS"),
            F::Asin => self.domain_guarded(w, f, a, "ASIN"),
            F::Coalesce => {
                if a.len() == 1 {
                    w.expr(self, &a[0], parent_prec)?;
                    Ok(())
                } else {
                    function(self, w, "coalesce", a)
                }
            }
            // never "--x": that is a comment
            F::Negate => wrapped(self, w, "-(", arg(a, 0)?, ")"),
            F::Error => {
                let raise = self.raise(a, call_type);
                w.piece(&*raise)?;
                Ok(())
            }

            // ---- arithmetic
            // Pure's divide is a DOUBLE division; x / 0 is NULL here
            F::Divide => {
                w.append("(CAST(").expr(self, arg(a, 0)?, 0)?.append(" AS DOUBLE) / CAST(").expr(
                    self,
                    arg(a, 1)?,
                    0,
                )?;
                w.append(" AS DOUBLE))");
                Ok(())
            }
            F::Mod | F::Rem => {
                let r = self.remainder(f, a);
                w.piece(&*r)?;
                Ok(())
            }
            // DuckDB's // truncates toward zero, NULL for a zero divisor: DIV
            F::IntDivide => {
                if a.iter().any(is_double) {
                    return Err(self.wall(f, "an integer division of a Float"));
                }
                w.append("(").expr(self, arg(a, 0)?, 6)?.append(" DIV ").expr(self, arg(a, 1)?, 7)?.append(")");
                Ok(())
            }
            F::RoundHalfUp => self.round_half_up(w, a),
            F::Cbrt => {
                let x = arg(a, 0)?;
                w.append("(CASE WHEN ").expr(self, x, 4)?.append(" < 0 THEN -POW(-(").expr(self, x, 0)?;
                w.append("), 1.0E0 / 3) ELSE POW(").expr(self, x, 0)?.append(", 1.0E0 / 3) END)");
                Ok(())
            }
            F::BitNot => wrapped7(self, w, "CAST(~ ", arg(a, 0)?, " AS SIGNED)"),
            F::ParseInt => {
                let p = self.parse_int(arg(a, 0)?);
                w.piece(&*p)?;
                Ok(())
            }
            F::BoolToText => {
                let t = self.text_of(self.ex(arg(a, 0)?, 0), Some(&SqlType::Scalar(Scalar::Boolean)));
                w.piece(&*t)?;
                Ok(())
            }
            // a Float's literal spelling, the value read once
            F::FloatCanon => {
                let layout = self.float_layout(self.ex(arg(a, 0)?, 0), true);
                w.piece(&*layout)?;
                Ok(())
            }
            F::FloatPrint => ansi::float_print_by_text(self, w, a, parent_prec),
            // NULL-skipping, as DuckDB's
            F::Greatest => {
                let p = self.null_skipping("GREATEST", a);
                w.piece(&*p)?;
                Ok(())
            }
            F::Least => {
                let p = self.null_skipping("LEAST", a);
                w.piece(&*p)?;
                Ok(())
            }

            // ---- strings
            // NULL-skipping, as DuckDB's concat
            F::Concat => {
                let flat = ansi::flatten_concat(a);
                w.append("CONCAT_WS('', ").join(&flat, ", ", |w, x| {
                    let t = self.as_text(x);
                    w.piece(&*t)?;
                    Ok(())
                })?;
                w.append(")");
                Ok(())
            }
            F::ConcatJoin => {
                w.append("CONCAT_WS('', ").join(a, ", ", |w, x| {
                    let t = self.as_text(x);
                    w.piece(&*t)?;
                    Ok(())
                })?;
                w.append(")");
                Ok(())
            }
            F::Length => {
                let t = self.as_text(arg(a, 0)?);
                w.append("CHAR_LENGTH(").piece(&*t)?.append(")");
                Ok(())
            }
            F::Left | F::Right => {
                let p = self.left_right(f, a);
                w.piece(&*p)?;
                Ok(())
            }
            F::Lpad => self.pad(w, "LPAD", a),
            F::Rpad => self.pad(w, "RPAD", a),
            // a NULL (a cast one is written bare) is cast to text: REPEAT of a bare NULL is binary
            F::RepeatStr => {
                let x = arg(a, 0)?;
                let null = matches!(x, SqlExpr::NullLit { .. })
                    || matches!(x, SqlExpr::Cast { value, .. } if matches!(value.as_ref(), SqlExpr::NullLit { .. }));
                if null {
                    w.append("REPEAT(CAST(").expr(self, x, 0)?.append(" AS CHAR), ");
                } else {
                    w.append("REPEAT(").expr(self, x, 0)?.append(", ");
                }
                w.expr(self, arg(a, 1)?, 0)?.append(")");
                Ok(())
            }
            F::StartsWith => {
                w.append("(LEFT(").expr(self, arg(a, 0)?, 0)?.append(", CHAR_LENGTH(").expr(self, arg(a, 1)?, 0)?;
                w.append(")) = ").expr(self, arg(a, 1)?, 5)?.append(")");
                Ok(())
            }
            F::EndsWith => {
                w.append("(RIGHT(").expr(self, arg(a, 0)?, 0)?.append(", CHAR_LENGTH(").expr(self, arg(a, 1)?, 0)?;
                w.append(")) = ").expr(self, arg(a, 1)?, 5)?.append(")");
                Ok(())
            }
            // DuckDB's trim family: spaces, or any of the given characters
            F::Trim | F::Ltrim | F::Rtrim => {
                let p = self.trim(f, a)?;
                w.piece(&*p)?;
                Ok(())
            }
            F::Substring => {
                w.append("SUBSTRING(").list(self, a)?.append(")");
                Ok(())
            }
            F::Split => {
                let p = self.split(arg(a, 0)?, arg(a, 1)?);
                w.piece(&*p)?;
                Ok(())
            }
            // DuckDB's split_part: 1-based, negative from the end, out of range ''
            F::SplitPart => {
                let (s, d) = (arg(a, 0)?, arg(a, 1)?);
                w.append("(CASE WHEN ").expr(self, s, 4)?.append(" IS NULL OR ").expr(self, d, 4)?;
                w.append(" IS NULL THEN NULL WHEN ").expr(self, arg(a, 2)?, 4)?.append(" = 0 THEN '' ELSE coalesce(");
                let n = arg(a, 2)?;
                let index = pc(move |w| {
                    w.append("CASE WHEN ").expr(self, n, 4)?.append(" < 0 THEN ").expr(self, n, 0)?.append(" ELSE ");
                    w.expr(self, n, 6)?.append(" - 1 END");
                    Ok(())
                });
                let at =
                    self.value_at(self.split(s, d), self.index_path(index), Some(&SqlType::Scalar(Scalar::Varchar)));
                w.piece(&*at)?.append(", '') END)");
                Ok(())
            }
            // PCRE: REGEXP searches; a full match anchors both ends
            F::Matches => {
                w.append("(").expr(self, arg(a, 0)?, 4)?.append(" REGEXP ");
                let p = self.pattern(a, f, false)?;
                w.append(&p).append(")");
                Ok(())
            }
            F::RegexpFullMatch => {
                w.append("(").expr(self, arg(a, 0)?, 4)?.append(" REGEXP ");
                let p = self.pattern(a, f, true)?;
                w.append(&p).append(")");
                Ok(())
            }
            F::RegexpExtract => {
                let p = self.regexp_extract(a)?;
                w.piece(&*p)?;
                Ok(())
            }
            F::RegexpExtractAll => {
                let p = self.regexp_extract_all(a)?;
                w.piece(&*p)?;
                Ok(())
            }
            F::RegexpReplace => {
                let p = self.regexp_replace(a)?;
                w.piece(&*p)?;
                Ok(())
            }
            // TO_BASE64 breaks its lines every 76 characters; FROM_BASE64 wants the padding
            F::EncodeBase64 => wrapped(self, w, "REPLACE(TO_BASE64(", arg(a, 0)?, "), CHAR(10 USING utf8mb4), '')"),
            F::DecodeBase64 => {
                let x = arg(a, 0)?;
                w.append("CONVERT(FROM_BASE64(CONCAT(")
                    .expr(self, x, 0)?
                    .append(", REPEAT('=', MOD(4 - MOD(CHAR_LENGTH(");
                w.expr(self, x, 0)?.append("), 4), 4)))) USING utf8mb4)");
                Ok(())
            }
            F::Md5 => wrapped(self, w, "MD5(", arg(a, 0)?, ")"),
            F::Sha1 => wrapped(self, w, "SHA1(", arg(a, 0)?, ")"),
            F::Sha256 => wrapped(self, w, "SHA2(", arg(a, 0)?, ", 256)"),
            F::Levenshtein => {
                let p = self.levenshtein(arg(a, 0)?, arg(a, 1)?)?;
                w.piece(&*p)?;
                Ok(())
            }
            F::JaroWinkler => {
                let p = self.jaro_winkler(arg(a, 0)?, arg(a, 1)?)?;
                w.piece(&*p)?;
                Ok(())
            }
            // UUID() is of the UUID type: its text
            F::Guid => {
                w.append("CAST(UUID() AS CHAR)");
                Ok(())
            }
            F::Random => {
                w.append("RAND()");
                Ok(())
            }
            F::Format => self.format(w, a),
            F::CurrentUserFn => {
                w.append("SUBSTRING_INDEX(CURRENT_USER(), '@', 1)");
                Ok(())
            }

            // ---- temporal
            F::Today => {
                w.append("UTC_DATE()");
                Ok(())
            }
            F::Now => {
                w.append("UTC_TIMESTAMP(6)");
                Ok(())
            }
            F::DateTruncDay => wrapped(self, w, "DATE(", arg(a, 0)?, ")"),
            F::ParseDate => {
                let p = self.parsed_date(arg(a, 0)?);
                w.piece(&*p)?;
                Ok(())
            }
            F::Extract => {
                let p = self.extract(a)?;
                w.piece(&*p)?;
                Ok(())
            }
            F::DateTrunc => {
                let p = self.date_trunc(a)?;
                w.piece(&*p)?;
                Ok(())
            }
            F::MakeDate => {
                w.append("STR_TO_DATE(CONCAT_WS('-', ").list(self, a)?.append("), '%Y-%m-%d')");
                Ok(())
            }
            F::MakeTimestamp => self.make_timestamp(w, a, f),
            // past the years 1000-9999 a date function answers NULL: raised, never a NULL value
            F::AddInterval | F::AddIntervalTemporal => {
                w.append("(CASE WHEN ").expr(self, arg(a, 1)?, 4)?.append(" IS NULL OR ").expr(self, arg(a, 2)?, 4)?;
                w.append(" IS NULL THEN NULL ELSE coalesce(");
                let added = self.add_interval(a)?;
                w.piece(&*added)?.append(", ");
                let message =
                    [build::string_lit(&format!("a date past the year 9999 has no {} value", self.product()))];
                let raise = self.raise(&message, call_type);
                w.piece(&*raise)?.append(") END)");
                Ok(())
            }
            F::DateDiff => {
                let p = self.date_diff(a)?;
                w.piece(&*p)?;
                Ok(())
            }
            F::TimeBucket => {
                let p = self.time_bucket(a)?;
                w.piece(&*p)?;
                Ok(())
            }
            // a zoned date-time is held as its naive UTC date-time: UTC's wall time is the instant's, either way
            F::Timezone => {
                let utc = matches!(arg(a, 0)?, SqlExpr::StringLit { value, .. } if UTC_ZONES.contains(&value.as_str()));
                if !utc {
                    return Err(self.wall(
                        f,
                        &format!("{} knows no named time zone but UTC without its zone tables", self.product()),
                    ));
                }
                w.expr(self, arg(a, 1)?, parent_prec)?;
                Ok(())
            }
            // TIMESTAMP(date, time): the date-time at that time of that day
            F::DateAtTime => function(self, w, "TIMESTAMP", a),
            F::EpochSeconds => {
                let m = self.micros(arg(a, 0)?);
                w.append("(").piece(&*m)?.append(" / 1000000)");
                Ok(())
            }
            F::EpochNs | F::FromEpochNs => Err(Refusal::capability(
                "a nanosecond epoch: MariaDB keeps microseconds (the Cypher island's nanosecond leg is DuckDB's)",
            )),
            F::EpochMs => {
                let m = self.micros(arg(a, 0)?);
                w.append("(").piece(&*m)?.append(" DIV 1000)");
                Ok(())
            }
            F::FromEpochSeconds => {
                w.append("(TIMESTAMP '1970-01-01 00:00:00' + INTERVAL (").expr(self, arg(a, 0)?, 0)?;
                w.append(") * 1000000 MICROSECOND)");
                Ok(())
            }
            F::FromEpochMs => {
                w.append("(TIMESTAMP '1970-01-01 00:00:00' + INTERVAL (").expr(self, arg(a, 0)?, 0)?;
                w.append(") * 1000 MICROSECOND)");
                Ok(())
            }
            F::Dayname => wrapped(self, w, "DAYNAME(", arg(a, 0)?, ")"),
            F::Monthname => wrapped(self, w, "MONTHNAME(", arg(a, 0)?, ")"),
            F::Strftime => {
                let SqlExpr::FormatLit { parts, .. } = arg(a, 1)? else {
                    return Err(self.wall(f, "a date format that is not a format literal: its codes are DuckDB's"));
                };
                let p = self.strftime(arg(a, 0)?, parts);
                w.piece(&*p)?;
                Ok(())
            }
            F::Strptime => {
                Err(self
                    .wall(f, &format!("a date parsed by a DuckDB format has no measured {} spelling", self.product())))
            }

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

            // ---- variant
            // in DuckDB's json_type vocabulary, which the lowering compares against
            F::JsonType => {
                w.append("(CASE JSON_TYPE(").expr(self, arg(a, 0)?, 0)?;
                w.append(") WHEN 'NULL' THEN 'NULL' WHEN 'STRING' THEN 'VARCHAR' WHEN 'BOOLEAN' THEN 'BOOLEAN'");
                w.append(" WHEN 'INTEGER' THEN 'BIGINT' WHEN 'DOUBLE' THEN 'DOUBLE' WHEN 'ARRAY' THEN 'ARRAY'");
                w.append(" WHEN 'OBJECT' THEN 'OBJECT' END)");
                Ok(())
            }
            F::JsonPretty => wrapped(self, w, "JSON_DETAILED(", arg(a, 0)?, ")"),
            F::JsonMergePatch => {
                w.append("JSON_MERGE_PATCH(").list(self, a)?.append(")");
                Ok(())
            }
        }
    }

    /// Java's `domainGuarded`: ACOS and ASIN, undefined outside [-1, 1], raised.
    fn domain_guarded(&self, w: &mut SqlWriter, f: SqlFn, a: &[SqlExpr], name: &str) -> Result<(), Refusal> {
        let x = arg(a, 0)?;
        w.append("(CASE WHEN ").expr(self, x, 4)?.append(" IS NULL THEN NULL ELSE coalesce(").append(name);
        w.append("(").expr(self, x, 0)?.append("), ");
        let message = [build::string_lit(&format!("{f} is undefined outside [-1,1]"))];
        let raise = self.raise(&message, Some(&SqlType::Scalar(Scalar::Double)));
        w.piece(&*raise)?.append(") END)");
        Ok(())
    }

    /// Java's `pad`: LPAD and RPAD, a space by default.
    fn pad(&self, w: &mut SqlWriter, name: &str, a: &[SqlExpr]) -> Result<(), Refusal> {
        w.append(name).append("(").expr(self, arg(a, 0)?, 0)?.append(", ").expr(self, arg(a, 1)?, 0)?.append(", ");
        if a.len() > 2 {
            w.expr(self, &a[2], 0)?;
        } else {
            w.append("' '");
        }
        w.append(")");
        Ok(())
    }

    /// Java's `equalArm`: a composite pair compared as its compact JSON.
    fn equal_arm(&self, w: &mut SqlWriter, f: SqlFn, a: &[SqlExpr], parent_prec: i32) -> Result<(), Refusal> {
        if a.len() == 2 && (composite(&a[0]) || composite(&a[1])) {
            let left = self.compact(self.ex(&a[0], 0));
            let right = self.compact(self.ex(&a[1], 0));
            w.append("(").piece(&*left)?.append(if f == SqlFn::Equal { " = " } else { " <> " }).piece(&*right)?;
            w.append(")");
            return Ok(());
        }
        ansi::infix(self, w, f, a, parent_prec)
    }

    /// Java's `raise(a, slot)`: a raised error kept in `@legend_raise`, typed as `slot`.
    pub(super) fn raise<'a>(&'a self, a: &'a [SqlExpr], slot: Option<&SqlType>) -> P<'a> {
        let raised = self.raised(a);
        match cast_target(self, slot) {
            None => raised,
            Some(t) => pc(move |w| {
                w.append("CAST(").piece(&*raised)?.append(&format!(" AS {t})"));
                Ok(())
            }),
        }
    }

    /// Java's `branch`: a CASE branch, error() raised in the CASE's own type.
    pub(super) fn branch<'a>(&'a self, e: &'a SqlExpr, slot: Option<&SqlType>) -> Result<P<'a>, Refusal> {
        if let SqlExpr::Call { r#fn: SqlFn::Error, args, r#type } = e {
            return Ok(self.raise(args, slot.or(fact_type(r#type))));
        }
        Ok(self.ex(e, 0))
    }

    /// Java's `raised`: the message enveloped, kept in the session variable as two rows fail.
    fn raised<'a>(&'a self, a: &'a [SqlExpr]) -> P<'a> {
        self.two_rows(pc(move |w| {
            w.append("CONCAT(CHAR(31 USING utf8mb4), ");
            if a.len() > 1 {
                w.expr(self, &a[1], 0)?.append(", CHAR(30 USING utf8mb4), ");
            }
            w.expr(self, arg(a, 0)?, 0)?.append(", CHAR(31 USING utf8mb4))");
            Ok(())
        }))
    }

    /// Java's `remainder`: Pure's mod (the divisor's sign) and rem (the dividend's).
    fn remainder<'a>(&'a self, f: SqlFn, a: &'a [SqlExpr]) -> P<'a> {
        let rem = pc(move |w| {
            w.append("MOD(").expr(self, arg(a, 0)?, 0)?.append(", ").expr(self, arg(a, 1)?, 0)?.append(")");
            Ok(())
        });
        if f == SqlFn::Rem {
            return rem;
        }
        pc(move |w| {
            w.append("MOD(").piece(&*rem)?.append(" + ").expr(self, arg(a, 1)?, 6)?.append(", ");
            w.expr(self, arg(a, 1)?, 0)?.append(")");
            Ok(())
        })
    }

    /// Java's `roundHalfUp`: DuckDB's round, half away from zero.
    fn round_half_up(&self, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
        let x = arg(a, 0)?;
        if !is_double(x) {
            w.append("ROUND(").list(self, a)?.append(")");
            return Ok(());
        }
        let scale = self.scale(a);
        w.append("(SIGN(").expr(self, x, 0)?.append(") * FLOOR(ABS(").expr(self, x, 0)?.append(") * ");
        w.piece(&*scale)?.append(" + 0.5) / ").piece(&*scale)?.append(")");
        Ok(())
    }

    /// `POW(10, n)` for a rounding's second argument, else `1`.
    pub(super) fn scale<'a>(&'a self, a: &'a [SqlExpr]) -> P<'a> {
        if a.len() > 1 {
            let n = &a[1];
            return pc(move |w| {
                w.append("POW(10, ").expr(self, n, 0)?.append(")");
                Ok(())
            });
        }
        text("1")
    }

    /// Java's `parseInt`: DuckDB's 64-bit parse, else an error.
    fn parse_int<'a>(&'a self, s: &'a SqlExpr) -> P<'a> {
        pc(move |w| {
            w.append("(CASE WHEN ").expr(self, s, 4)?.append(" IS NULL THEN NULL WHEN TRIM(").expr(self, s, 0)?;
            w.append(") REGEXP '^[+-]?[0-9]+$' THEN CAST(TRIM(").expr(self, s, 0)?.append(") AS SIGNED) ELSE ");
            let message = [build::call(
                SqlFn::Concat,
                vec![build::string_lit("Could not convert string '"), s.clone(), build::string_lit("' to INT64")],
            )];
            let raise = self.raise(&message, Some(&SqlType::Scalar(Scalar::Bigint)));
            w.piece(&*raise)?.append(" END)");
            Ok(())
        })
    }

    /// Java's `nullSkipping`: GREATEST/LEAST over the arguments, each NULL replaced by the next non-NULL one.
    fn null_skipping<'a>(&'a self, name: &'static str, a: &'a [SqlExpr]) -> P<'a> {
        if a.len() == 1 {
            return self.ex(&a[0], 0);
        }
        pc(move |w| {
            w.append(name).append("(");
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

    /// Java's `leftRight`: DuckDB's left/right, a negative count all but that many.
    fn left_right<'a>(&'a self, f: SqlFn, a: &'a [SqlExpr]) -> P<'a> {
        let name = if f == SqlFn::Left { "LEFT" } else { "RIGHT" };
        pc(move |w| {
            // Java reads the count first
            let n = arg(a, 1)?;
            let s = arg(a, 0)?;
            w.append("(CASE WHEN ").expr(self, n, 4)?.append(&format!(" < 0 THEN {name}(")).expr(self, s, 0)?;
            w.append(", GREATEST(CHAR_LENGTH(").expr(self, s, 0)?.append(") + ").expr(self, n, 6)?;
            w.append(&format!(", 0)) ELSE {name}(")).expr(self, s, 0)?.append(", ").expr(self, n, 0)?.append(") END)");
            Ok(())
        })
    }

    /// Java's `trim`: spaces, or every leading/trailing character of the given set (a regular expression's class).
    fn trim<'a>(&'a self, f: SqlFn, a: &'a [SqlExpr]) -> Result<P<'a>, Refusal> {
        if a.len() == 1 {
            let name = match f {
                SqlFn::Trim => "TRIM",
                SqlFn::Ltrim => "LTRIM",
                _ => "RTRIM",
            };
            let x = &a[0];
            return Ok(pc(move |w| {
                w.append(name).append("(").expr(self, x, 0)?.append(")");
                Ok(())
            }));
        }
        let chars = self.literal(arg(a, 1)?, "a trim's character set")?;
        // each character, a class's special one escaped (by UTF-16 unit in Java: a surrogate pair's halves are
        // written in order, so the class is the characters themselves)
        let mut cls = String::from("[");
        for ch in chars.chars() {
            if "\\]^-[".contains(ch) {
                cls.push('\\');
            }
            cls.push(ch);
        }
        cls.push(']');
        let p = match f {
            SqlFn::Trim => format!("^{cls}+|{cls}+$"),
            SqlFn::Ltrim => format!("^{cls}+"),
            _ => format!("{cls}+$"),
        };
        let literal = self.string_literal(&p);
        let x = arg(a, 0)?;
        Ok(pc(move |w| {
            w.append("REGEXP_REPLACE(").expr(self, x, 0)?.append(&format!(", {literal}, '')"));
            Ok(())
        }))
    }

    /// Java's `format`: printf -- MariaDB's through SFORMAT, MySQL's written out piece by piece.
    pub(super) fn format(&self, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
        if self.mysql { self.format_mysql(w, a) } else { self.format_mariadb(w, a) }
    }

    /// MariaDB's printf (Java's `MariaDb.format`): SFORMAT's fmt codes, a literal brace doubled, a `%s` argument its
    /// text.
    fn format_mariadb(&self, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
        let f = self.literal(arg(a, 0)?, "a format")?;
        let mut out = String::new();
        let mut args: Vec<P<'_>> = Vec::new();
        let mut next = 1usize;
        let mut last = 0usize;
        for m in directives(f) {
            out.push_str(&f[last..m.start].replace('{', "{{").replace('}', "}}"));
            last = m.end;
            if m.conversion == '%' {
                out.push('%');
                continue;
            }
            let conversion = match m.conversion {
                'd' | 'i' => "d".to_owned(),
                'f' | 'F' => "f".to_owned(),
                'e' | 'E' | 'g' | 'G' | 'x' | 'X' | 'o' => m.conversion.to_string(),
                's' => String::new(),
                other => {
                    return Err(Refusal::capability(format!("printf conversion %{other} reached {}", self.product())));
                }
            };
            let spec = format!(
                "{}{}{}{}{}{conversion}",
                if m.flags.contains('-') { "<" } else { "" },
                if m.flags.contains('+') { "+" } else { "" },
                if m.flags.contains('0') { "0" } else { "" },
                m.width,
                m.precision.map_or_else(String::new, |p| format!(".{p}"))
            );
            if spec.is_empty() {
                out.push_str("{}");
            } else {
                out.push_str(&format!("{{:{spec}}}"));
            }
            if next < a.len() {
                let x = &a[next];
                args.push(if m.conversion == 's' { self.as_text(x) } else { self.ex(x, 0) });
            }
            next += 1;
        }
        out.push_str(&f[last..].replace('{', "{{").replace('}', "}}"));
        w.append(&format!("SFORMAT({}", self.string_literal(&out)));
        for p in &args {
            w.append(", ").piece(&**p)?;
        }
        w.append(")");
        Ok(())
    }

    /// MySQL's printf (Java's `MySql.format`): the format's literal pieces and each conversion written out,
    /// concatenated.
    fn format_mysql(&self, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
        let f = self.literal(arg(a, 0)?, "a format")?;
        let mut parts: Vec<P<'_>> = Vec::new();
        let mut next = 1usize;
        let mut last = 0usize;
        let mut literal = String::new();
        for m in directives(f) {
            literal.push_str(&f[last..m.start]);
            last = m.end;
            if m.conversion == '%' {
                literal.push('%');
                continue;
            }
            if next >= a.len() {
                return Err(Refusal::capability("a format with more conversions than arguments reached MySQL"));
            }
            parts.push(text(self.string_literal(&literal)));
            literal.clear();
            let x = &a[next];
            next += 1;
            parts.push(self.conversion(x, m.conversion, m.flags, m.width, m.precision)?);
        }
        literal.push_str(&f[last..]);
        parts.push(text(self.string_literal(&literal)));
        w.append("CONCAT(").join(&parts, ", ", |w, p| {
            w.piece(&**p)?;
            Ok(())
        })?;
        w.append(")");
        Ok(())
    }

    /// Java's `MySql.conversion`: one printf conversion written out.
    fn conversion<'a>(
        &'a self,
        x: &'a SqlExpr,
        conversion: char,
        flags: &str,
        width: &str,
        precision: Option<&str>,
    ) -> Result<P<'a>, Refusal> {
        let digits: P<'a> = match conversion {
            's' => self.as_text(x),
            'd' | 'i' => pc(move |w| {
                w.append("CAST(CAST(").expr(self, x, 0)?.append(" AS SIGNED) AS CHAR)");
                Ok(())
            }),
            // a negative value that rounds to zero keeps its sign
            'f' | 'F' => {
                let scale = precision.unwrap_or("6").to_owned();
                pc(move |w| {
                    w.append("CONCAT(CASE WHEN ").expr(self, x, 4)?.append(" < 0 AND CAST(").expr(self, x, 0)?;
                    w.append(&format!(" AS DECIMAL(65,{scale})) = 0 THEN '-' ELSE '' END, CAST(CAST("));
                    w.expr(self, x, 0)?.append(&format!(" AS DECIMAL(65,{scale})) AS CHAR))"));
                    Ok(())
                })
            }
            'x' => pc(move |w| {
                w.append("LOWER(CONV(").expr(self, x, 0)?.append(", 10, 16))");
                Ok(())
            }),
            'X' => pc(move |w| {
                w.append("CONV(").expr(self, x, 0)?.append(", 10, 16)");
                Ok(())
            }),
            'o' => pc(move |w| {
                w.append("CONV(").expr(self, x, 0)?.append(", 10, 8)");
                Ok(())
            }),
            other => return Err(Refusal::capability(format!("printf conversion %{other} reached MySQL"))),
        };
        let plus = flags.contains('+') && matches!(conversion, 'd' | 'i' | 'f');
        let signed: P<'a> = if !plus {
            digits
        } else {
            pc(move |w| {
                w.append("CONCAT(CASE WHEN ")
                    .expr(self, x, 4)?
                    .append(" >= 0 THEN '+' ELSE '' END, ")
                    .piece(&*digits)?;
                w.append(")");
                Ok(())
            })
        };
        if width.is_empty() {
            return Ok(signed);
        }
        let n = width.to_owned();
        if flags.contains('-') {
            return Ok(pc(move |w| {
                w.append("RPAD(").piece(&*signed)?.append(&format!(", GREATEST({n}, CHAR_LENGTH(")).piece(&*signed)?;
                w.append(")), ' ')");
                Ok(())
            }));
        }
        if flags.contains('0') && conversion != 's' {
            // the zeros after the sign: -5 as %05d is -0005
            return Ok(pc(move |w| {
                w.append("(CASE WHEN LEFT(").piece(&*signed)?.append(", 1) IN ('-', '+') THEN CONCAT(LEFT(");
                w.piece(&*signed)?.append(", 1), LPAD(SUBSTRING(").piece(&*signed)?;
                w.append(&format!(", 2), GREATEST({n} - 1, CHAR_LENGTH(")).piece(&*signed)?;
                w.append(") - 1), '0')) ELSE LPAD(").piece(&*signed)?.append(&format!(", GREATEST({n}, CHAR_LENGTH("));
                w.piece(&*signed)?.append(")), '0') END)");
                Ok(())
            }));
        }
        Ok(pc(move |w| {
            w.append("LPAD(").piece(&*signed)?.append(&format!(", GREATEST({n}, CHAR_LENGTH(")).piece(&*signed)?;
            w.append(")), ' ')");
            Ok(())
        }))
    }

    // ---- regular expressions: literal patterns

    /// Java's `patternOf`: a literal pattern, no options beside it (but an extract's group).
    fn pattern_of<'e>(&self, a: &'e [SqlExpr], f: SqlFn) -> Result<&'e str, Refusal> {
        match arg(a, 1)? {
            SqlExpr::StringLit { value, .. }
                if !(a.len() > 2 && f != SqlFn::RegexpExtract && f != SqlFn::RegexpExtractAll) =>
            {
                Ok(value)
            }
            _ => Err(self.wall(f, "a pattern that is not a literal, or options beside it")),
        }
    }

    /// Java's `pattern`: the pattern's literal, anchored at both ends for a full match.
    fn pattern(&self, a: &[SqlExpr], f: SqlFn, full: bool) -> Result<String, Refusal> {
        let p = self.pattern_of(a, f)?;
        Ok(self.string_literal(&if full { format!("^(?:{p})\\z") } else { p.to_owned() }))
    }

    /// Java's `group`: an extract's group, a literal 0 to 9.
    fn group(&self, a: &[SqlExpr]) -> Result<i64, Refusal> {
        if a.len() < 3 {
            return Ok(0);
        }
        match &a[2] {
            SqlExpr::IntLit { value, .. } if (0..=9).contains(value) => Ok(*value),
            _ => {
                Err(Refusal::capability(format!("a regexp group that is not a literal 0-9 reached {}", self.product())))
            }
        }
    }

    /// Java's `regexpExtract`: DuckDB's regexp_extract, the leftmost match's group ('' when none matches).
    fn regexp_extract<'a>(&'a self, a: &'a [SqlExpr]) -> Result<P<'a>, Refusal> {
        let p = self.pattern_of(a, SqlFn::RegexpExtract)?;
        let g = self.group(a)?;
        let x = arg(a, 0)?;
        if g == 0 {
            let literal = self.string_literal(p);
            return Ok(pc(move |w| {
                w.append("(CASE WHEN ").expr(self, x, 4)?.append(" IS NULL THEN NULL ELSE REGEXP_SUBSTR(");
                w.expr(self, x, 0)?.append(&format!(", {literal}) END)"));
                Ok(())
            }));
        }
        let literal = self.string_literal(p);
        let anchored = self.string_literal(&format!("^[\\s\\S]*?(?:{p})[\\s\\S]*\\z"));
        let group = self.string_literal(&self.group_ref(g));
        Ok(pc(move |w| {
            w.append("(CASE WHEN ").expr(self, x, 4)?.append(" IS NULL THEN NULL WHEN ").expr(self, x, 4)?;
            w.append(&format!(" REGEXP {literal} THEN REGEXP_REPLACE(")).expr(self, x, 0)?;
            w.append(&format!(", {anchored}, {group}) ELSE '' END)"));
            Ok(())
        }))
    }

    /// Java's `regexpExtractAll`: DuckDB's regexp_extract_all, each non-overlapping match's group, left to right.
    fn regexp_extract_all<'a>(&'a self, a: &'a [SqlExpr]) -> Result<P<'a>, Refusal> {
        let p = self.pattern_of(a, SqlFn::RegexpExtractAll)?;
        let g = self.group(a)?;
        let groups = capture_groups(p);
        let r#ref = groups + 1 + g;
        if r#ref > 9 {
            return Err(Refusal::capability(format!(
                "regexp_extract_all of group {g} of a pattern of {groups} groups reached {} (a replacement names \
                 groups 1-9)",
                self.product()
            )));
        }
        let x = arg(a, 0)?;
        let repeat = self.string_literal(&format!("^(?:[\\s\\S]*?(?:{p})){{"));
        let kth_tail = self.string_literal(&format!("}}[\\s\\S]*?({p})[\\s\\S]*\\z"));
        let group = self.string_literal(&self.group_ref(r#ref));
        let k = text("__k.\"__n\"");
        let kth = {
            let k = k.clone();
            let repeat = repeat.clone();
            pc(move |w| {
                w.append("REGEXP_REPLACE(").expr(self, x, 0)?.append(&format!(", CONCAT({repeat}, ")).piece(&*k)?;
                w.append(&format!(" - 1, {kth_tail}), {group})"));
                Ok(())
            })
        };
        let agg = self.array_agg(false, kth, Some(k.clone()), false);
        let length = pc(move |v| {
            v.append("CHAR_LENGTH(").expr(self, x, 0)?.append(") + 1");
            Ok(())
        });
        let rows = self.rows(length, "__k");
        Ok(pc(move |w| {
            w.append("(CASE WHEN ")
                .expr(self, x, 4)?
                .append(" IS NULL THEN NULL ELSE coalesce((SELECT ")
                .piece(&*agg)?;
            w.append(" FROM ").piece(&*rows)?.append(" WHERE ").expr(self, x, 4)?;
            w.append(&format!(" REGEXP CONCAT({repeat}, ")).piece(&*k)?.append(", '}')), JSON_ARRAY()) END)");
            Ok(())
        }))
    }

    /// Java's `regexpReplace`: DuckDB's regexp_replace, the first match unless the options say 'g' -- MariaDB's as the
    /// only match after a lazy prefix, MySQL's by ICU's occurrence.
    fn regexp_replace<'a>(&'a self, a: &'a [SqlExpr]) -> Result<P<'a>, Refusal> {
        let p = self.literal(arg(a, 1)?, "a regexp_replace pattern")?;
        let r = self.literal(arg(a, 2)?, "a regexp_replace replacement")?;
        let options = if a.len() > 3 { self.literal(&a[3], "regexp_replace options")? } else { "" };
        let mut global = false;
        let mut flags = "";
        for o in options.encode_utf16() {
            match o {
                0x67 => global = true,
                0x69 => flags = "(?i)",
                0x63 => flags = "",
                other => {
                    return Err(Refusal::capability(format!(
                        "regexp_replace option '{}' reached {}",
                        String::from_utf16_lossy(&[other]),
                        self.product()
                    )));
                }
            }
        }
        let x = arg(a, 0)?;
        if self.mysql {
            let icu = icu_replacement(r);
            let pattern = self.string_literal(&format!("{flags}{p}"));
            let replacement = self.string_literal(&icu);
            let occurrence = if global { "0" } else { "1" };
            return Ok(pc(move |w| {
                w.append("REGEXP_REPLACE(").expr(self, x, 0)?;
                w.append(&format!(", {pattern}, {replacement}, 1, {occurrence})"));
                Ok(())
            }));
        }
        if global {
            let pattern = self.string_literal(&format!("{flags}{p}"));
            let replacement = self.string_literal(r);
            return Ok(pc(move |w| {
                w.append("REGEXP_REPLACE(").expr(self, x, 0)?.append(&format!(", {pattern}, {replacement})"));
                Ok(())
            }));
        }
        let shifted = self.shifted_replacement(r)?;
        let pattern = self.string_literal(&format!("{flags}^([\\s\\S]*?)({p})"));
        let replacement = self.string_literal(&shifted);
        Ok(pc(move |w| {
            w.append("REGEXP_REPLACE(").expr(self, x, 0)?.append(&format!(", {pattern}, {replacement})"));
            Ok(())
        }))
    }

    /// MariaDB's first-match replacement: each `\N` two groups on (the prefix and the match before it), by UTF-16
    /// unit, a digit as `Character.isDigit` takes it.
    fn shifted_replacement(&self, r: &str) -> Result<String, Refusal> {
        let units: Vec<u16> = r.encode_utf16().collect();
        let backslash = u16::from(b'\\');
        let mut out: Vec<u16> = "\\1".encode_utf16().collect();
        let mut i = 0;
        while i < units.len() {
            let ch = units[i];
            if ch == backslash && i + 1 < units.len() && character::is_digit(units[i + 1]) {
                i += 1;
                let k = i64::from(units[i]) - i64::from(b'0');
                if k + 2 > 9 {
                    return Err(Refusal::capability(format!(
                        "regexp_replace of group {k} (first match only) reached {}",
                        self.product()
                    )));
                }
                out.push(backslash);
                out.extend((k + 2).to_string().encode_utf16());
            } else {
                out.push(ch);
            }
            i += 1;
        }
        Ok(String::from_utf16_lossy(&out))
    }

    // ---- list calls

    /// Java's `lists`: the list functions over JSON arrays.
    pub(super) fn lists(&self, w: &mut SqlWriter, f: SqlFn, a: &[SqlExpr], t: &TypeFact) -> Result<(), Refusal> {
        use SqlFn as F;
        let e = self.param("__e");
        let o = self.param("__o");
        let call_type = fact_type(t);
        match f {
            F::ListFilter => {
                let l = self.lambda_of(arg(a, 1)?, f)?;
                let x = &l.params[0];
                let xs = arg(a, 0)?;
                let el = list_element(xs);
                let select = self.json_of(text(self.param(x)), el);
                // the body is read where the filter is written (Java's lambda)
                let r#where = pc(move |w| {
                    let body = lambda_body(l, el)?;
                    w.expr(self, &body, 0)?;
                    Ok(())
                });
                let over = self.over_elements(
                    xs,
                    x.clone(),
                    offset_of(l),
                    index_of(l).map(str::to_owned),
                    select,
                    Some(r#where),
                );
                w.piece(&*over)?;
                Ok(())
            }
            F::ListTransform => {
                let l = self.lambda_of(arg(a, 1)?, f)?;
                let xs = arg(a, 0)?;
                let body = lambda_body(l, list_element(xs))?;
                let select = self.json_owned(body);
                let over = self.over_elements(
                    xs,
                    l.params[0].clone(),
                    offset_of(l),
                    index_of(l).map(str::to_owned),
                    select,
                    None,
                );
                w.piece(&*over)?;
                Ok(())
            }
            // NULL ++ xs is xs, as DuckDB's list_concat; JSON_MERGE_PRESERVE appends arrays
            F::ListConcat => {
                w.append("(CASE WHEN ");
                for (i, x) in a.iter().enumerate() {
                    w.append(if i > 0 { " AND " } else { "" }).expr(self, x, 4)?.append(" IS NULL");
                }
                w.append(" THEN NULL ELSE JSON_MERGE_PRESERVE(");
                for (i, x) in a.iter().enumerate() {
                    w.append(if i > 0 { ", " } else { "" }).append("coalesce(").expr(self, x, 0)?;
                    w.append(", JSON_ARRAY())");
                }
                w.append(") END)");
                Ok(())
            }
            // 1-based, negative from the end, 0 and out of range NULL (DuckDB's list_extract)
            F::ListGet => {
                let xs = arg(a, 0)?;
                let n = arg(a, 1)?;
                let index = pc(move |w| {
                    w.append("CASE WHEN ").expr(self, n, 4)?.append(" < 0 THEN ").expr(self, n, 0)?.append(" ELSE ");
                    w.expr(self, n, 6)?.append(" - 1 END");
                    Ok(())
                });
                let at = self.element_at(xs, index);
                // a list whose elements claim no type: the element cast to the call's own type
                let r#type = if list_element(xs).is_none() { cast_target(self, call_type) } else { None };
                w.append("(CASE WHEN ").expr(self, n, 4)?.append(" = 0 THEN NULL ELSE ");
                match r#type {
                    None => {
                        w.piece(&*at)?;
                    }
                    Some(t) => {
                        w.append("CAST(").piece(&*at)?.append(&format!(" AS {t})"));
                    }
                }
                w.append(" END)");
                Ok(())
            }
            // DuckDB's list_position: 1-based, NULL when absent, a NULL needle found by IS
            F::ListPosition => {
                w.append(&format!("(SELECT min({o}) FROM "));
                let elements = self.elements(arg(a, 0)?, "__e", "__o", None);
                w.piece(&*elements)?.append(&format!(" WHERE {e} <=> ")).expr(self, arg(a, 1)?, 5)?.append(")");
                Ok(())
            }
            // DuckDB's list_distinct drops NULLs; the first occurrence's order
            F::ListDistinct => {
                let xs = arg(a, 0)?;
                let el = list_element(xs);
                let e2 = self.param("__e2");
                let o2 = self.param("__o2");
                let select = self.json_of(text(e.clone()), el);
                let r#where = pc(move |w| {
                    w.append(&format!("{e} IS NOT NULL AND NOT EXISTS(SELECT 1 FROM "));
                    let elements = self.elements(xs, "__e2", "__o2", None);
                    w.piece(&*elements)?.append(&format!(" WHERE {e2} = {e} AND {o2} < {o})"));
                    Ok(())
                });
                let over = self.over_elements(xs, "__e".to_owned(), "__o".to_owned(), None, select, Some(r#where));
                w.piece(&*over)?;
                Ok(())
            }
            // list_append(NULL, v) is [v]
            F::ListAppend => {
                w.append("JSON_ARRAY_APPEND(coalesce(").expr(self, arg(a, 0)?, 0)?.append(", JSON_ARRAY()), '$', ");
                let json = self.json(arg(a, 1)?);
                w.piece(&*json)?.append(")");
                Ok(())
            }
            F::ListSum => self.over_rows_of(w, a, AggFn::Sum),
            F::ListMin => self.element_reduction_of(w, a, call_type, AggFn::Min),
            F::ListMax => self.element_reduction_of(w, a, call_type, AggFn::Max),
            F::ListAvg => self.over_rows_of(w, a, AggFn::Avg),
            F::ListMedian => self.over_rows_of(w, a, AggFn::Median),
            F::ListMode => self.element_reduction_of(w, a, call_type, AggFn::Mode),
            F::ListBoolAnd => self.over_rows_of(w, a, AggFn::BoolAnd),
            F::ListBoolOr => self.over_rows_of(w, a, AggFn::BoolOr),
            // ascending / descending, NULLs last both ways; a struct by its fields in declared order
            F::ListSort | F::ListSortDesc => {
                let asc = f == F::ListSort;
                let xs = arg(a, 0)?;
                let el = list_element(xs);
                let agg = self.array_agg(
                    false,
                    self.json_of(text(e.clone()), el),
                    Some(self.element_sort(&e, el, asc)),
                    false,
                );
                let body = pc(move |w| {
                    w.append("coalesce((SELECT ").piece(&*agg)?.append(" FROM ");
                    let elements = self.elements(xs, "__e", "__o", None);
                    w.piece(&*elements)?.append("), JSON_ARRAY())");
                    Ok(())
                });
                let p = self.unless_null(xs, body);
                w.piece(&*p)?;
                Ok(())
            }
            F::ListReverse => {
                let xs = arg(a, 0)?;
                let agg = self.array_agg(
                    false,
                    self.json_of(text(e.clone()), list_element(xs)),
                    Some(text(format!("{o} DESC"))),
                    false,
                );
                let body = pc(move |w| {
                    w.append("coalesce((SELECT ").piece(&*agg)?.append(" FROM ");
                    let elements = self.elements(xs, "__e", "__o", None);
                    w.piece(&*elements)?.append("), JSON_ARRAY())");
                    Ok(())
                });
                let p = self.unless_null(xs, body);
                w.piece(&*p)?;
                Ok(())
            }
            F::ListTail => {
                let xs = arg(a, 0)?;
                let select = self.json_of(text(e.clone()), list_element(xs));
                let over = self.over_elements(
                    xs,
                    "__e".to_owned(),
                    "__o".to_owned(),
                    None,
                    select,
                    Some(text(format!("{o} > 1"))),
                );
                w.piece(&*over)?;
                Ok(())
            }
            F::ListInit => {
                let xs = arg(a, 0)?;
                let select = self.json_of(text(e.clone()), list_element(xs));
                let r#where = pc(move |w| {
                    w.append(&format!("{o} < JSON_LENGTH(")).expr(self, xs, 0)?.append(")");
                    Ok(())
                });
                let over = self.over_elements(xs, "__e".to_owned(), "__o".to_owned(), None, select, Some(r#where));
                w.piece(&*over)?;
                Ok(())
            }
            // DuckDB's list_slice: 1-based, both ends inclusive, a negative end from the end
            F::ListSlice => {
                let xs = arg(a, 0)?;
                let select = self.json_of(text(e.clone()), list_element(xs));
                let r#where = pc(move |w| {
                    let lo = self.slice_end(xs, arg(a, 1)?);
                    let hi = self.slice_end(xs, arg(a, 2)?);
                    w.append(&format!("{o} >= ")).piece(&*lo)?.append(&format!(" AND {o} <= ")).piece(&*hi)?;
                    Ok(())
                });
                let over = self.over_elements(xs, "__e".to_owned(), "__o".to_owned(), None, select, Some(r#where));
                w.piece(&*over)?;
                Ok(())
            }
            F::ListFlatten => {
                let xs = arg(a, 0)?;
                let inner = match list_element(xs) {
                    Some(SqlType::Array { element }) => Some(element.as_ref()),
                    _ => None,
                };
                let agg = self.array_agg(
                    false,
                    self.json_of(text("__f.\"v\""), inner),
                    Some(text("__f.\"o\", __f.\"i\"")),
                    false,
                );
                let spec = format!(
                    "'$[*]' COLUMNS (\"o\" FOR ORDINALITY, NESTED PATH '$[*]' COLUMNS (\"i\" FOR ORDINALITY, \"v\" {} \
                     PATH '$'))",
                    self.column_type(inner)
                );
                let table = self.json_table(self.ex(xs, 0), spec, "__f".to_owned());
                let body = pc(move |w| {
                    w.append("coalesce((SELECT ").piece(&*agg)?.append(" FROM ").piece(&*table)?;
                    w.append(" WHERE __f.\"i\" IS NOT NULL), JSON_ARRAY())");
                    Ok(())
                });
                let p = self.unless_null(xs, body);
                w.piece(&*p)?;
                Ok(())
            }
            F::ListZip => {
                let p = self.zip(a, t)?;
                w.piece(&*p)?;
                Ok(())
            }
            F::ListReduce => {
                let p = self.list_reduce(a, call_type)?;
                w.piece(&*p)?;
                Ok(())
            }
            F::ListProduct => {
                Err(self.wall(f, &format!("a list's cartesian product has no {} spelling", self.product())))
            }
            // DuckDB's range: [start, stop) by step (start 0, step 1 by default)
            F::RangeFn => self.range(w, a),
            F::RepeatValue => {
                let n = arg(a, 1)?;
                w.append("(CASE WHEN ").expr(self, n, 4)?.append(" > 0 THEN (SELECT JSON_ARRAYAGG(");
                let json = self.json(arg(a, 0)?);
                w.piece(&*json)?.append(") FROM ");
                let rows = self.rows(self.ex(n, 0), "__g");
                w.piece(&*rows)?.append(") ELSE JSON_ARRAY() END)");
                Ok(())
            }
            F::Typeof => Err(self.wall(f, "its type names are DuckDB's")),
            other => Err(Refusal::illegal_state(format!("not a list call: {other}"))),
        }
    }

    fn over_rows_of(&self, w: &mut SqlWriter, a: &[SqlExpr], f: AggFn) -> Result<(), Refusal> {
        let p = self.over_rows(arg(a, 0)?, f, &[])?;
        w.piece(&*p)?;
        Ok(())
    }

    fn element_reduction_of(
        &self,
        w: &mut SqlWriter,
        a: &[SqlExpr],
        call_type: Option<&SqlType>,
        f: AggFn,
    ) -> Result<(), Refusal> {
        let p = self.element_reduction(a, call_type, f)?;
        w.piece(&*p)?;
        Ok(())
    }

    /// [`MariaDb::json`] of an owned expression.
    fn json_owned<'a>(&'a self, x: SqlExpr) -> P<'a> {
        let x = std::rc::Rc::new(x);
        let t = type_of(&x).cloned();
        let compacted = t.is_none() && matches!(*x, SqlExpr::ArrayLit { .. } | SqlExpr::StructLit { .. });
        let boolean = t.is_none() && matches!(*x, SqlExpr::BoolLit { .. });
        let value = {
            let x = x.clone();
            pc(move |w| {
                w.expr(self, &x, 0)?;
                Ok(())
            })
        };
        if compacted {
            return self.compact(value);
        }
        let t = if boolean { Some(SqlType::Scalar(Scalar::Boolean)) } else { t };
        self.json_of(value, t.as_ref())
    }

    /// Java's `zip`: DuckDB's list_zip, a struct per position of the longest list.
    fn zip<'a>(&'a self, a: &'a [SqlExpr], t: &TypeFact) -> Result<P<'a>, Refusal> {
        let fields = match fact_element(t) {
            Some(SqlType::Struct { fields }) if fields.len() == a.len() => fields.clone(),
            _ => return Err(self.wall(SqlFn::ListZip, "a zip of unknown struct type")),
        };
        let object = pc(move |w| {
            w.append("JSON_OBJECT(");
            for (i, x) in a.iter().enumerate() {
                w.append(if i > 0 { ", " } else { "" }).append(&self.string_literal(&fields[i].name)).append(", ");
                let at = self.element_at(x, text("__z.\"__n\" - 1"));
                let json = self.json_of(at, Some(&fields[i].r#type));
                w.piece(&*json)?;
            }
            w.append(")");
            Ok(())
        });
        let agg = self.array_agg(false, object, Some(text("__z.\"__n\"")), false);
        let longest = pc(move |v| {
            v.append("GREATEST(");
            for (i, x) in a.iter().enumerate() {
                v.append(if i > 0 { ", " } else { "" })
                    .append("coalesce(JSON_LENGTH(")
                    .expr(self, x, 0)?
                    .append("), 0)");
            }
            v.append(")");
            Ok(())
        });
        let rows = self.rows(longest, "__z");
        Ok(pc(move |w| {
            w.append("coalesce((SELECT ").piece(&*agg)?.append(" FROM ").piece(&*rows)?;
            w.append(" WHERE __z.\"__n\" <= GREATEST(");
            for (i, x) in a.iter().enumerate() {
                w.append(if i > 0 { ", " } else { "" })
                    .append("coalesce(JSON_LENGTH(")
                    .expr(self, x, 0)?
                    .append("), 0)");
            }
            w.append(")), JSON_ARRAY())");
            Ok(())
        }))
    }

    /// Java's `range`: DuckDB's range, [start, stop) by step, a row per value.
    fn range(&self, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
        let start = if a.len() == 1 { text("0") } else { self.ex(arg(a, 0)?, 0) };
        let last = arg(a, if a.len() == 1 { 0 } else { 1 })?;
        let stop = self.ex(last, 0);
        let step = if a.len() > 2 { self.ex(&a[2], 0) } else { text("1") };
        let count = {
            let (start, stop, step) = (start.clone(), stop.clone(), step.clone());
            pc(move |w| {
                w.append("CEIL(((").piece(&*stop)?.append(") - (").piece(&*start)?.append(")) / (").piece(&*step)?;
                w.append("))");
                Ok(())
            })
        };
        let value = {
            let (start, step) = (start.clone(), step.clone());
            pc(move |v| {
                v.append("(").piece(&*start)?.append(") + (__g.\"__n\" - 1) * (").piece(&*step)?.append(")");
                Ok(())
            })
        };
        w.append("(CASE WHEN ((").piece(&*stop)?.append(") - (").piece(&*start)?.append(")) * (").piece(&*step)?;
        w.append(") > 0 THEN (SELECT ");
        let agg = self.array_agg(false, value, Some(text("__g.\"__n\"")), false);
        w.piece(&*agg)?.append(" FROM ");
        let rows = self.rows(count, "__g");
        w.piece(&*rows)?.append(") ELSE JSON_ARRAY() END)");
        Ok(())
    }

    /// Java's `sliceEnd`: a slice bound, 1-based, a negative one from the end.
    fn slice_end<'a>(&'a self, xs: &'a SqlExpr, i: &'a SqlExpr) -> P<'a> {
        pc(move |w| {
            w.append("(CASE WHEN ").expr(self, i, 4)?.append(" < 0 THEN JSON_LENGTH(").expr(self, xs, 0)?;
            w.append(") + ").expr(self, i, 6)?.append(" + 1 ELSE ").expr(self, i, 0)?.append(" END)");
            Ok(())
        })
    }

    /// Java's `elementSort`: the ORDER BY of a list's elements, NULLs last; a struct by its fields in declared order.
    fn element_sort(&self, x: &str, el: Option<&SqlType>, asc: bool) -> P<'static> {
        if let Some(SqlType::Struct { fields }) = el {
            let mut keys = Vec::with_capacity(fields.len());
            for f in fields {
                let field = self.json_text_of(x, &self.member_path(&f.name));
                keys.push(super::aggregates::element_sort_of(self.scalar_of_text(text(field), Some(&f.r#type)), asc));
            }
            return pc(move |w| {
                for (i, k) in keys.iter().enumerate() {
                    w.append(if i > 0 { ", " } else { "" }).piece(&**k)?;
                }
                Ok(())
            });
        }
        super::aggregates::element_sort_of(text(x.to_owned()), asc)
    }

    // ---- map calls: JSON objects (text keys)

    /// Java's `maps`: the map functions over JSON objects.
    pub(super) fn maps(&self, w: &mut SqlWriter, f: SqlFn, a: &[SqlExpr], t: &TypeFact) -> Result<(), Refusal> {
        use SqlFn as F;
        let varchar = SqlType::Scalar(Scalar::Varchar);
        match f {
            F::MapEmpty => {
                w.append("JSON_OBJECT()");
                Ok(())
            }
            F::MapFromLists => {
                let v = list_element(arg(a, 1)?);
                let keys = self.json_table(
                    self.ex(arg(a, 0)?, 0),
                    format!(
                        "'$[*]' COLUMNS (\"o\" FOR ORDINALITY, \"k\" {} PATH '$')",
                        self.column_type(Some(&varchar))
                    ),
                    "__k".to_owned(),
                );
                let values = self.json_table(
                    self.ex(arg(a, 1)?, 0),
                    format!("'$[*]' COLUMNS (\"o\" FOR ORDINALITY, \"v\" {} PATH '$')", self.column_type(v)),
                    "__v".to_owned(),
                );
                let value = self.json_of(text("__v.\"v\""), v);
                w.append("(SELECT JSON_OBJECTAGG(__k.\"k\", ").piece(&*value)?.append(") FROM ").piece(&*keys)?;
                w.append(" LEFT JOIN ").piece(&*values)?.append(" ON __v.\"o\" = __k.\"o\")");
                Ok(())
            }
            // DuckDB's map_from_entries: each entry a struct of (key, value), by position whatever their names
            F::MapFromEntries => {
                let xs = arg(a, 0)?;
                let entry = match list_element(xs) {
                    Some(SqlType::Struct { fields }) if fields.len() == 2 => fields,
                    other => {
                        return Err(self.wall(f, &format!("entries of {}", super::type_text(other))));
                    }
                };
                let vt = &entry[1].r#type;
                let spec = format!(
                    "'$[*]' COLUMNS (\"k\" {} PATH {}, \"v\" {} PATH {})",
                    self.column_type(Some(&varchar)),
                    self.member_path(&entry[0].name),
                    self.column_type(Some(vt)),
                    self.member_path(&entry[1].name)
                );
                let table = self.json_table(self.ex(xs, 0), spec, "__e".to_owned());
                let value = self.json_of(text("__e.\"v\""), Some(vt));
                w.append("(SELECT JSON_OBJECTAGG(__e.\"k\", ").piece(&*value)?.append(") FROM ").piece(&*table)?;
                w.append(")");
                Ok(())
            }
            // DuckDB's map_extract: a list of the key's value, empty when absent
            F::MapExtract => {
                let (m, k) = (arg(a, 0)?, arg(a, 1)?);
                w.append("(CASE WHEN ").expr(self, m, 4)?.append(" IS NULL THEN NULL WHEN");
                w.append(" JSON_CONTAINS_PATH(")
                    .expr(self, m, 0)?
                    .append(", 'one', CONCAT('$.\"', ")
                    .expr(self, k, 0)?;
                w.append(", '\"')) THEN JSON_ARRAY(JSON_EXTRACT(").expr(self, m, 0)?.append(", CONCAT('$.\"', ");
                w.expr(self, k, 0)?.append(", '\"'))) ELSE JSON_ARRAY() END)");
                Ok(())
            }
            F::MapKeys => {
                let m = arg(a, 0)?;
                let body = pc(move |w| {
                    w.append("JSON_KEYS(").expr(self, m, 0)?.append(")");
                    Ok(())
                });
                let p = self.unless_null(m, body);
                w.piece(&*p)?;
                Ok(())
            }
            F::MapValues => {
                let m = arg(a, 0)?;
                let body = pc(move |w| {
                    w.append("coalesce(JSON_EXTRACT(").expr(self, m, 0)?.append(", '$.*'), JSON_ARRAY())");
                    Ok(())
                });
                let p = self.unless_null(m, body);
                w.piece(&*p)?;
                Ok(())
            }
            // the later map's keys win, as DuckDB's map_concat
            F::MapConcat => {
                w.append("JSON_MERGE_PATCH(").list(self, a)?.append(")");
                Ok(())
            }
            other => {
                let _ = t;
                Err(Refusal::illegal_state(format!("not a map call: {other}")))
            }
        }
    }

    // ---- temporal helpers: a date is a DATE, a date-time a DATETIME(6)

    /// Java's `parsedDate`: Pure's parseDate of a text, a text that is no date raised.
    fn parsed_date<'a>(&'a self, x: &'a SqlExpr) -> P<'a> {
        pc(move |w| {
            w.append("(CASE WHEN ").expr(self, x, 4)?.append(" IS NULL THEN NULL ELSE coalesce(");
            let utc = utc_of(self.ex(x, 0));
            w.piece(&*utc)?.append(", ");
            let message = [build::call(
                SqlFn::Concat,
                vec![build::string_lit("Could not parse '"), x.clone(), build::string_lit("' as a date")],
            )];
            let raise = self.raise(&message, Some(&SqlType::Scalar(Scalar::Timestamp)));
            w.piece(&*raise)?.append(") END)");
            Ok(())
        })
    }

    /// Java's `micros(t)`: a date-time's microseconds since the epoch.
    fn micros<'a>(&'a self, t: &'a SqlExpr) -> P<'a> {
        pc(move |w| {
            w.append("TIMESTAMPDIFF(MICROSECOND, TIMESTAMP '1970-01-01 00:00:00', ").expr(self, t, 0)?.append(")");
            Ok(())
        })
    }

    /// Java's `extract`: a date part of a date, a Pure integer.
    fn extract<'a>(&'a self, a: &'a [SqlExpr]) -> Result<P<'a>, Refusal> {
        let p = part(arg(a, 0)?)?;
        let t = arg(a, 1)?;
        if p == DatePart::Year {
            return Ok(self.year_of(t));
        }
        let name = match p {
            DatePart::Year => Some("YEAR("),
            DatePart::Quarter => Some("QUARTER("),
            DatePart::Month => Some("MONTH("),
            DatePart::Day => Some("DAYOFMONTH("),
            DatePart::Hour => Some("HOUR("),
            DatePart::Minute => Some("MINUTE("),
            DatePart::Second => Some("SECOND("),
            DatePart::DayOfYear => Some("DAYOFYEAR("),
            DatePart::Microsecond => Some("MICROSECOND("),
            DatePart::Week | DatePart::Millisecond | DatePart::Nanosecond | DatePart::IsoDayOfWeek => None,
        };
        if let Some(name) = name {
            return Ok(wrapped_piece(self, name, t, ")"));
        }
        Ok(match p {
            DatePart::Week => wrapped_piece(self, "WEEK(", t, ", 3)"),
            DatePart::Millisecond => wrapped_piece(self, "(MICROSECOND(", t, ") DIV 1000)"),
            DatePart::IsoDayOfWeek => wrapped_piece(self, "(WEEKDAY(", t, ") + 1)"),
            DatePart::Nanosecond => {
                return Err(Refusal::capability(format!(
                    "date part 'nanosecond' has no measured {} spelling",
                    self.product()
                )));
            }
            DatePart::Year
            | DatePart::Quarter
            | DatePart::Month
            | DatePart::Day
            | DatePart::Hour
            | DatePart::Minute
            | DatePart::Second
            | DatePart::DayOfYear
            | DatePart::Microsecond => return Err(Refusal::illegal_state(format!("named: {p}"))),
        })
    }

    /// Java's `dateTrunc`: year/quarter/month/week deliver a DATE, finer parts a date-time; the week starts Monday.
    fn date_trunc<'a>(&'a self, a: &'a [SqlExpr]) -> Result<P<'a>, Refusal> {
        let p = part(arg(a, 0)?)?;
        let t = arg(a, 1)?;
        Ok(match p {
            DatePart::Year => wrapped_piece(self, "MAKEDATE(YEAR(", t, "), 1)"),
            DatePart::Quarter => pc(move |w| {
                w.append("(MAKEDATE(YEAR(").expr(self, t, 0)?.append("), 1) + INTERVAL (QUARTER(").expr(self, t, 0)?;
                w.append(") - 1) QUARTER)");
                Ok(())
            }),
            DatePart::Month => pc(move |w| {
                w.append("(DATE(").expr(self, t, 0)?.append(") - INTERVAL (DAYOFMONTH(").expr(self, t, 0)?;
                w.append(") - 1) DAY)");
                Ok(())
            }),
            DatePart::Week => pc(move |w| {
                w.append("(DATE(").expr(self, t, 0)?.append(") - INTERVAL WEEKDAY(").expr(self, t, 0)?.append(") DAY)");
                Ok(())
            }),
            DatePart::Day => wrapped_piece(self, "CAST(DATE(", t, ") AS DATETIME(6))"),
            DatePart::Hour => wrapped_piece(self, "CAST(DATE_FORMAT(", t, ", '%Y-%m-%d %H:00:00') AS DATETIME(6))"),
            DatePart::Minute => wrapped_piece(self, "CAST(DATE_FORMAT(", t, ", '%Y-%m-%d %H:%i:00') AS DATETIME(6))"),
            DatePart::Second => wrapped_piece(self, "CAST(DATE_FORMAT(", t, ", '%Y-%m-%d %H:%i:%s') AS DATETIME(6))"),
            DatePart::Millisecond => pc(move |w| {
                w.append("(CAST(").expr(self, t, 0)?.append(" AS DATETIME(6)) - INTERVAL MOD(MICROSECOND(");
                w.expr(self, t, 0)?.append("), 1000) MICROSECOND)");
                Ok(())
            }),
            DatePart::Microsecond | DatePart::Nanosecond | DatePart::DayOfYear | DatePart::IsoDayOfWeek => {
                return Err(Refusal::capability(format!(
                    "date_trunc part '{}' has no measured {} spelling",
                    p.as_str().to_lowercase(),
                    self.product()
                )));
            }
        })
    }

    /// Java's `addInterval`: a date plus an interval; milliseconds and finer are microseconds.
    fn add_interval<'a>(&'a self, a: &'a [SqlExpr]) -> Result<P<'a>, Refusal> {
        let unit = part(arg(a, 0)?)?;
        let n = arg(a, 1)?;
        let t = arg(a, 2)?;
        let (name, times) = match unit {
            DatePart::Year => ("YEAR", "1"),
            DatePart::Month => ("MONTH", "1"),
            DatePart::Week => ("WEEK", "1"),
            DatePart::Day => ("DAY", "1"),
            DatePart::Hour => ("HOUR", "1"),
            DatePart::Minute => ("MINUTE", "1"),
            DatePart::Second => ("SECOND", "1"),
            DatePart::Millisecond => ("MICROSECOND", "1000"),
            DatePart::Microsecond => ("MICROSECOND", "1"),
            DatePart::Quarter | DatePart::Nanosecond | DatePart::DayOfYear | DatePart::IsoDayOfWeek => {
                return Err(Refusal::capability(format!("interval unit {unit} has no {} spelling", self.product())));
            }
        };
        let tail = format!("){} {name})", if times == "1" { String::new() } else { format!(" * {times}") });
        Ok(pc(move |w| {
            w.append("(").expr(self, t, 6)?.append(" + INTERVAL (").expr(self, n, 0)?.append(&tail);
            Ok(())
        }))
    }

    /// Java's `dateDiff`: DuckDB's date_diff counts the boundaries crossed; its week is whole days over 7.
    fn date_diff<'a>(&'a self, a: &'a [SqlExpr]) -> Result<P<'a>, Refusal> {
        let p = part(arg(a, 0)?)?;
        let from = arg(a, 1)?;
        let to = arg(a, 2)?;
        Ok(match p {
            DatePart::Year => pc(move |w| {
                w.append("(YEAR(").expr(self, to, 0)?.append(") - YEAR(").expr(self, from, 0)?.append("))");
                Ok(())
            }),
            DatePart::Quarter => pc(move |w| {
                w.append("((YEAR(").expr(self, to, 0)?.append(") * 4 + QUARTER(").expr(self, to, 0)?;
                w.append(")) - (YEAR(").expr(self, from, 0)?.append(") * 4 + QUARTER(").expr(self, from, 0)?;
                w.append(")))");
                Ok(())
            }),
            DatePart::Month => pc(move |w| {
                w.append("((YEAR(").expr(self, to, 0)?.append(") * 12 + MONTH(").expr(self, to, 0)?;
                w.append(")) - (YEAR(").expr(self, from, 0)?.append(") * 12 + MONTH(").expr(self, from, 0)?;
                w.append(")))");
                Ok(())
            }),
            DatePart::Week => pc(move |w| {
                w.append("(DATEDIFF(").expr(self, to, 0)?.append(", ").expr(self, from, 0)?.append(") DIV 7)");
                Ok(())
            }),
            DatePart::Day => pc(move |w| {
                w.append("DATEDIFF(").expr(self, to, 0)?.append(", ").expr(self, from, 0)?.append(")");
                Ok(())
            }),
            DatePart::Hour => self.boundaries(from, to, 3_600_000_000),
            DatePart::Minute => self.boundaries(from, to, 60_000_000),
            DatePart::Second => self.boundaries(from, to, 1_000_000),
            DatePart::Millisecond => self.boundaries(from, to, 1000),
            DatePart::Microsecond => self.boundaries(from, to, 1),
            DatePart::Nanosecond | DatePart::DayOfYear | DatePart::IsoDayOfWeek => {
                return Err(Refusal::capability(format!(
                    "date_diff part '{}' has no measured {} spelling",
                    p.as_str().to_lowercase(),
                    self.product()
                )));
            }
        })
    }

    /// Java's `boundaries`: the unit boundaries between two date-times, each floored to the unit in microseconds.
    fn boundaries<'a>(&'a self, from: &'a SqlExpr, to: &'a SqlExpr, unit: i64) -> P<'a> {
        let (to_micros, from_micros) = (self.micros(to), self.micros(from));
        pc(move |w| {
            w.append("(FLOOR(").piece(&*to_micros)?.append(&format!(" / {unit}) - FLOOR(")).piece(&*from_micros)?;
            w.append(&format!(" / {unit}))"));
            Ok(())
        })
    }

    /// Java's `timeBucket`: DuckDB's time_bucket with the base's origins.
    fn time_bucket<'a>(&'a self, a: &'a [SqlExpr]) -> Result<P<'a>, Refusal> {
        let unit = part(arg(a, 0)?)?;
        let n = arg(a, 1)?;
        let t = arg(a, 2)?;
        if let Some(micros) = interval_micros(unit) {
            let origin: i64 = if unit == DatePart::Week { -3 * 86_400_000_000 } else { 0 };
            let size = pc(move |w| {
                w.append("(").expr(self, n, 6)?.append(&format!(" * {micros})"));
                Ok(())
            });
            let since = self.micros(t);
            let m = pc(move |w| {
                w.append("(").piece(&*since)?.append(&format!(" - ({origin}))"));
                Ok(())
            });
            return Ok(pc(move |w| {
                w.append("(TIMESTAMP '1970-01-01 00:00:00' + INTERVAL (");
                w.append("(FLOOR(").piece(&*m)?.append(" / ").piece(&*size)?.append(") * ").piece(&*size)?;
                w.append(&format!(" + ({origin}))"));
                w.append(") MICROSECOND)");
                Ok(())
            }));
        }
        let months = match unit {
            DatePart::Month => 1,
            DatePart::Year => 12,
            _ => 0,
        };
        if months == 0 {
            return Err(Refusal::capability(format!("a {unit} time bucket has no {} spelling", self.product())));
        }
        let k = pc(move |w| {
            w.append("((YEAR(").expr(self, t, 0)?.append(") - 1970) * 12 + MONTH(").expr(self, t, 0)?.append(") - 1)");
            Ok(())
        });
        let size = pc(move |w| {
            w.append("(").expr(self, n, 6)?.append(&format!(" * {months})"));
            Ok(())
        });
        Ok(pc(move |w| {
            w.append("CAST(DATE '1970-01-01' + INTERVAL (FLOOR(").piece(&*k)?.append(" / ").piece(&*size)?;
            w.append(") * ").piece(&*size)?.append(") MONTH AS DATETIME(6))");
            Ok(())
        }))
    }

    /// Java's `makeTimestamp`: six arguments, the seconds' fraction to microseconds.
    fn make_timestamp(&self, w: &mut SqlWriter, a: &[SqlExpr], f: SqlFn) -> Result<(), Refusal> {
        if a.len() != 6 {
            return Err(self.wall(f, &format!("make_timestamp of {} arguments", a.len())));
        }
        w.append("(STR_TO_DATE(CONCAT_WS('-', ").list(self, &a[0..3])?.append("), '%Y-%m-%d') + INTERVAL ");
        w.expr(self, &a[3], 6)?.append(" HOUR + INTERVAL ").expr(self, &a[4], 6)?.append(" MINUTE + INTERVAL ROUND((");
        w.expr(self, &a[5], 0)?.append(") * 1000000) MICROSECOND)");
        Ok(())
    }

    /// Java's `strftime`: a date-time formatted by DuckDB's strftime parts, through DATE_FORMAT.
    fn strftime<'a>(&'a self, value: &'a SqlExpr, parts: &[DateFmt]) -> P<'a> {
        let mut pieces: Vec<P<'a>> = Vec::new();
        let mut run = String::new();
        for p in parts {
            let code = match p {
                DateFmt::Text { s } => Some(s.replace('%', "%%")),
                DateFmt::Part(part) => match part {
                    DateFmtPart::Year4 => Some("%Y".to_owned()),
                    DateFmtPart::Month2 => Some("%m".to_owned()),
                    DateFmtPart::Day2 => Some("%d".to_owned()),
                    DateFmtPart::Hour2 => Some("%H".to_owned()),
                    DateFmtPart::Min2 => Some("%i".to_owned()),
                    DateFmtPart::Sec2 => Some("%s".to_owned()),
                    DateFmtPart::SubsecMicro => Some("%f".to_owned()),
                    DateFmtPart::SubsecNano => Some("%f000".to_owned()),
                    DateFmtPart::MonthAbbrev => Some("%b".to_owned()),
                    DateFmtPart::MonthName => Some("%M".to_owned()),
                    DateFmtPart::WeekdayName => Some("%W".to_owned()),
                    DateFmtPart::Hour12 => Some("%h".to_owned()),
                    DateFmtPart::Hour12Nopad => Some("%l".to_owned()),
                    DateFmtPart::Ampm => Some("%p".to_owned()),
                    DateFmtPart::SubsecMin => None,
                },
            };
            if let Some(code) = code {
                run.push_str(&code);
                continue;
            }
            if !run.is_empty() {
                pieces.push(self.date_format(value, &run));
                run.clear();
            }
            // DuckDB's %g: milliseconds, three digits
            pieces.push(wrapped_piece(self, "LEFT(DATE_FORMAT(", value, ", '%f'), 3)"));
        }
        if !run.is_empty() || pieces.is_empty() {
            pieces.push(self.date_format(value, &run));
        }
        if pieces.len() == 1 {
            return pieces.pop().expect("one piece");
        }
        pc(move |w| {
            w.append("CONCAT(").join(&pieces, ", ", |x, piece| {
                x.piece(&**piece)?;
                Ok(())
            })?;
            w.append(")");
            Ok(())
        })
    }

    /// `DATE_FORMAT(value, '<codes>')`.
    fn date_format<'a>(&'a self, value: &'a SqlExpr, codes: &str) -> P<'a> {
        let f = self.string_literal(codes);
        pc(move |w| {
            w.append("DATE_FORMAT(").expr(self, value, 0)?.append(&format!(", {f})"));
            Ok(())
        })
    }
}

/// `writer.function(name, args)`.
fn function(d: &MariaDb, w: &mut SqlWriter, name: &str, a: &[SqlExpr]) -> Result<(), Refusal> {
    w.function(d, name, a)?;
    Ok(())
}

/// `head` `x` `tail`, `x` written at precedence 0.
fn wrapped(d: &MariaDb, w: &mut SqlWriter, head: &str, x: &SqlExpr, tail: &str) -> Result<(), Refusal> {
    w.append(head).expr(d, x, 0)?.append(tail);
    Ok(())
}

/// `head` `x` `tail`, `x` written at precedence 7.
fn wrapped7(d: &MariaDb, w: &mut SqlWriter, head: &str, x: &SqlExpr, tail: &str) -> Result<(), Refusal> {
    w.append(head).expr(d, x, 7)?.append(tail);
    Ok(())
}

/// [`wrapped`] as a piece.
fn wrapped_piece<'a>(d: &'a MariaDb, head: &'static str, x: &'a SqlExpr, tail: &'static str) -> P<'a> {
    pc(move |w| {
        w.append(head).expr(d, x, 0)?.append(tail);
        Ok(())
    })
}

/// Java's `intervalMicros(unit)`: a fixed-length unit's length in microseconds; none for a calendar one.
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

/// Java's `captureGroups(p)`: the capturing groups of a pattern, by UTF-16 unit -- each '(' not escaped, not in a
/// class, not a '(?' group (but a named one).
pub(super) fn capture_groups(p: &str) -> i64 {
    let u: Vec<u16> = p.encode_utf16().collect();
    let starts = |i: usize, s: &str| {
        let s: Vec<u16> = s.encode_utf16().collect();
        u.len() >= i + s.len() && u[i..i + s.len()] == s[..]
    };
    let c = |ch: char| ch as u16;
    let mut n = 0;
    let mut in_class = false;
    let mut i = 0;
    while i < u.len() {
        let ch = u[i];
        if ch == c('\\') {
            i += 1;
        } else if in_class {
            in_class = ch != c(']');
        } else if ch == c('[') {
            in_class = true;
        } else if ch == c('(')
            && (i + 1 >= u.len()
                || u[i + 1] != c('?')
                || starts(i, "(?<") && i + 3 < u.len() && u[i + 3] != c('=') && u[i + 3] != c('!')
                || starts(i, "(?P<"))
        {
            n += 1;
        }
        i += 1;
    }
    n
}

/// MySQL's ICU replacement (Java's `MySql.regexpReplace`'s loop, by UTF-16 unit): `\N` is `$N`, an escaped character
/// kept escaped, a literal `$` or backslash escaped.
pub(super) fn icu_replacement(r: &str) -> String {
    let units: Vec<u16> = r.encode_utf16().collect();
    let backslash = u16::from(b'\\');
    let dollar = u16::from(b'$');
    let mut out: Vec<u16> = Vec::with_capacity(units.len());
    let mut i = 0;
    while i < units.len() {
        let ch = units[i];
        if ch == backslash && i + 1 < units.len() && character::is_digit(units[i + 1]) {
            i += 1;
            out.push(dollar);
            out.push(units[i]);
        } else if ch == backslash && i + 1 < units.len() {
            i += 1;
            out.push(backslash);
            out.push(units[i]);
        } else if ch == dollar || ch == backslash {
            out.push(backslash);
            out.push(ch);
        } else {
            out.push(ch);
        }
        i += 1;
    }
    String::from_utf16_lossy(&out)
}

/// One printf directive Java's `%([-+ #0]*)([0-9]*)(?:\.([0-9]+))?([a-zA-Z%])` finds: where it starts and ends (byte
/// offsets), its flags, width, precision and conversion.
pub(super) struct Directive<'f> {
    pub start: usize,
    pub end: usize,
    pub flags: &'f str,
    pub width: &'f str,
    pub precision: Option<&'f str>,
    pub conversion: char,
}

/// Each directive of a printf format, left to right (Java's `Matcher.find` over it).
pub(super) fn directives(f: &str) -> Vec<Directive<'_>> {
    let b = f.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] != b'%' {
            i += 1;
            continue;
        }
        let mut j = i + 1;
        while j < b.len() && b"-+ #0".contains(&b[j]) {
            j += 1;
        }
        let flags = &f[i + 1..j];
        let w0 = j;
        while j < b.len() && b[j].is_ascii_digit() {
            j += 1;
        }
        let width = &f[w0..j];
        let mut precision = None;
        if j + 1 < b.len() && b[j] == b'.' && b[j + 1].is_ascii_digit() {
            let p0 = j + 1;
            j += 1;
            while j < b.len() && b[j].is_ascii_digit() {
                j += 1;
            }
            precision = Some(&f[p0..j]);
        }
        if j < b.len() && (b[j].is_ascii_alphabetic() || b[j] == b'%') {
            out.push(Directive { start: i, end: j + 1, flags, width, precision, conversion: b[j] as char });
            i = j + 1;
        } else {
            i += 1;
        }
    }
    out
}
