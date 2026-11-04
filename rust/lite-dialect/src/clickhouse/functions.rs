// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! ClickHouse's scalar functions: Java's `call` -- every `SqlFn` decided by an exhaustive match -- `listCall`, the list
//! reductions, the several-unnests select, `struct_insert`, the variant hooks and the temporal arms, with the helpers
//! their arms name (ClickHouse.java, "Lists" to "Hooks of the base").

use super::{ClickHouse, P, is_raise, is_scalar, lambda_arg, list_element, output_name, pc, stripped_scale, text, type_of};
use crate::ansi::{self, cast_type_name, part};
use crate::build;
use crate::refusal::{Refusal, arg};
use crate::renderer::AnsiSqlRenderer;
use crate::writer::SqlWriter;
use lite_sql::{AggFn, DateFmt, DatePart, Scalar, SqlExpr, SqlFn, SqlSelect, SqlSource, SqlType, StructField, TypeFact};

/// The list functions that answer NULL for a NULL list (DuckDB's), each over its first argument (Java's
/// `NULL_STRICT_LISTS`).
const NULL_STRICT_LISTS: &[SqlFn] = &[
    SqlFn::ListSlice,
    SqlFn::ListFilter,
    SqlFn::ListTransform,
    SqlFn::ListSort,
    SqlFn::ListSortDesc,
    SqlFn::ListReverse,
    SqlFn::ListDistinct,
    SqlFn::ListTail,
    SqlFn::ListInit,
];

/// The message a date past ClickHouse's years raises (Java's `DATE_RANGE`).
const DATE_RANGE: &str =
    "a date outside the years 1900 to 2299 has no ClickHouse value (it clamps one to the end it passed)";

fn wall(f: SqlFn, why: &str) -> Refusal {
    Refusal::capability(format!("{f} reached ClickHouse: {why}"))
}

/// `a.subList(0, n)`, refused as `subList` words an end past the size.
fn head(a: &[SqlExpr], n: usize) -> Result<&[SqlExpr], Refusal> {
    a.get(..n).ok_or_else(|| Refusal { class: Refusal::INDEX_OUT_OF_BOUNDS, message: format!("toIndex = {n}") })
}

/// A printf template's conversions in order, `%%` none (Java's `conversions`).
fn conversions(format: &str) -> Vec<char> {
    let cs: Vec<char> = format.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < cs.len() {
        if cs[i] != '%' {
            i += 1;
            continue;
        }
        let mut j = i + 1;
        while j < cs.len() && "-+ #0123456789.".contains(cs[j]) {
            j += 1;
        }
        if j < cs.len() && cs[j] != '%' {
            out.push(cs[j]);
        }
        i = j + 1;
    }
    out
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

/// A unit as ClickHouse's dateDiff names it (Java's `diffUnit`).
fn diff_unit(unit: DatePart) -> Result<&'static str, Refusal> {
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
            return Err(Refusal::capability(format!("dateDiff of {unit} has no ClickHouse spelling")));
        }
    })
}

/// `e`'s scale where it is a Decimal (a Float literal is one by its digits), else none (Java's `decimalScale`).
fn decimal_scale(e: &SqlExpr) -> Result<Option<i64>, Refusal> {
    if let SqlExpr::FloatLit { value, .. } = e
        && let Some(plain) = crate::java::float_decimal(value.value())?
    {
        return Ok(Some(stripped_scale(&plain).max(0)));
    }
    Ok(match type_of(e) {
        Some(SqlType::Decimal { scale, .. }) => Some(i64::from(*scale)),
        _ => None,
    })
}

/// The modulo for these operands: moduloOrZero, plain modulo over a Decimal (Java's `modulo`).
fn modulo(a: &[SqlExpr]) -> Result<&'static str, Refusal> {
    let (a0, a1) = (arg(a, 0)?, arg(a, 1)?);
    let decimal = |e: &SqlExpr| matches!(type_of(e), Some(SqlType::Decimal { .. }));
    let float = |e: &SqlExpr| matches!(e, SqlExpr::FloatLit { .. });
    Ok(if decimal(a0) || decimal(a1) || float(a0) || float(a1) { "modulo" } else { "moduloOrZero" })
}

/// A quantile's level when it is a literal number (an aggregate's parameter, spelled in its name) (Java's
/// `literalLevel`).
fn literal_level(extras: &[SqlExpr]) -> Option<String> {
    if extras.len() != 1 {
        return None;
    }
    match &extras[0] {
        SqlExpr::FloatLit { value, .. } => Some(lite_javacompat::double_to_string(value.value())),
        SqlExpr::DecimalLit { value, .. } => Some(crate::java::big_decimal_plain(value.as_str())),
        SqlExpr::IntLit { value, .. } => Some(value.to_string()),
        _ => None,
    }
}

/// A quantile's one level (Java's `level`).
fn level(extras: &[SqlExpr]) -> Result<&SqlExpr, Refusal> {
    if extras.len() != 1 {
        return Err(Refusal::capability(format!("a quantile of {} levels reached ClickHouse", extras.len())));
    }
    Ok(&extras[0])
}

/// The JSON value a list was read from, when the list is NULL exactly where that value is a JSON null (Java's
/// `nullOrigin`).
fn null_origin(e: &SqlExpr) -> Option<&SqlExpr> {
    match e {
        SqlExpr::Cast { value, target: SqlType::Array { .. }, .. } if is_scalar(value, Scalar::Json) => Some(value),
        SqlExpr::Call { r#fn, args, .. } if NULL_STRICT_LISTS.contains(r#fn) && !args.is_empty() => {
            null_origin(&args[0])
        }
        SqlExpr::Case { whens, otherwise, .. } => {
            let mut origin: Option<&SqlExpr> = None;
            let branches = whens.iter().map(|w| &w.then).chain(otherwise.as_deref());
            for b in branches {
                if is_raise(b) {
                    continue;
                }
                let o = null_origin(b)?;
                if origin.is_some_and(|x| x != o) {
                    return None;
                }
                origin = Some(o);
            }
            origin
        }
        _ => None,
    }
}

/// A modulo's operand: beside a Decimal, both are Decimal128s of the larger scale (Java's `operand`).
fn operand<'a>(d: &'a ClickHouse, a: &'a [SqlExpr], i: usize) -> Result<P<'a>, Refusal> {
    let s0 = decimal_scale(arg(a, 0)?)?;
    let s1 = decimal_scale(arg(a, 1)?)?;
    let e = arg(a, i)?;
    if s0.is_none() && s1.is_none() {
        return Ok(d.ex(e, 0));
    }
    let scale = s0.unwrap_or(0).max(s1.unwrap_or(0));
    Ok(pc(move |w| {
        w.append("toDecimal128(").expr(d, e, 0)?.append(&format!(", {scale})"));
        Ok(())
    }))
}

impl ClickHouse {
    // ==================================================================
    // Lists
    // ==================================================================

    /// An aggregate over a list's elements (`arrayReduce('fn', xs)`): NULLs skipped, NULL over no element; a quantile
    /// reads them as Float64 (Java's `reduced`).
    fn reduced<'a>(&'a self, f: &str, xs: &'a SqlExpr) -> P<'a> {
        let or_null = match f.find('(') {
            None => format!("{f}OrNull"),
            Some(paren) => format!("{}OrNull{}", &f[..paren], &f[paren..]),
        };
        let quantile = f.starts_with("quantile");
        let list = self.list(xs);
        pc(move |w| {
            let lit = self.string_lit(&or_null)?;
            if quantile {
                w.append(&format!("arrayReduce({lit}, arrayMap(\"__x\" -> toFloat64(\"__x\"), "));
                w.piece(&*list)?.append("))");
            } else {
                w.append(&format!("arrayReduce({lit}, ")).piece(&*list)?.append(")");
            }
            Ok(())
        })
    }

    /// The most frequent of the values: by count, a tie by the smaller value; NULL over none (Java's `modeOf`).
    pub(super) fn mode_of<'a>(values: P<'a>) -> P<'a> {
        pc(move |w| {
            w.append("arrayElement(arraySort(\"__v\" -> (-countEqual(").piece(&*values)?;
            w.append(", \"__v\"), \"__v\"), arrayDistinct(").piece(&*values)?.append(")), 1)");
            Ok(())
        })
    }

    /// A list's continuous quantile at a level that is no literal (Java's `interpolated`).
    fn interpolated<'a>(&'a self, xs: &'a SqlExpr, q: &'a SqlExpr) -> P<'a> {
        let list = self.list(xs);
        let s: P<'a> = pc(move |w| {
            w.append("arraySort(arrayMap(\"__x\" -> toFloat64(\"__x\"), arrayFilter(\"__x\" -> \"__x\" IS NOT NULL, ");
            w.piece(&*list)?.append(")))");
            Ok(())
        });
        let pos: P<'a> = {
            let s = s.clone();
            pc(move |w| {
                w.append("((length(").piece(&*s)?.append(") - 1) * ").expr(self, q, 6)?.append(")");
                Ok(())
            })
        };
        let at = |round: &'static str| -> P<'a> {
            let s = s.clone();
            let pos = pos.clone();
            pc(move |w| {
                w.append("arrayElement(").piece(&*s)?.append(&format!(", toUInt64({round}("));
                w.piece(&*pos)?.append(")) + 1)");
                Ok(())
            })
        };
        let (floor, ceil) = (at("floor"), at("ceil"));
        pc(move |w| {
            w.append("if(empty(").piece(&*s)?.append("), NULL, ").piece(&*floor)?.append(" + (").piece(&*pos)?;
            w.append(" - floor(").piece(&*pos)?.append(")) * (").piece(&*ceil)?.append(" - ").piece(&*floor)?;
            w.append("))");
            Ok(())
        })
    }

    pub(super) fn list_call_arm(&self, w: &mut SqlWriter, f: SqlFn, args: &[SqlExpr]) -> Result<(), Refusal> {
        let xs = arg(args, 0)?;
        let a1 = || arg(args, 1);
        match f {
            SqlFn::ListFilter | SqlFn::ListTransform => {
                w.append(if f == SqlFn::ListFilter { "arrayFilter(" } else { "arrayMap(" });
                w.expr(self, a1()?, 0)?.append(", ");
                let arrays = self.lambda_arrays(xs, lambda_arg(a1()?)?);
                w.piece(&*arrays)?.append(")");
            }
            SqlFn::ListConcat => {
                w.append("arrayConcat(").join(args, ", ", |w, a| {
                    w.piece(&*self.list(a))?;
                    Ok(())
                })?;
                w.append(")");
            }
            // 1-based, negative from the end, out of range NULL; a Tuple is never NULL: a struct is its default
            SqlFn::ListGet => {
                let list = self.list(xs);
                if matches!(type_of(xs), Some(SqlType::Array { element }) if matches!(**element, SqlType::Struct { .. }))
                {
                    w.append("arrayElement(").piece(&*list)?.append(", ").expr(self, a1()?, 0)?.append(")");
                } else {
                    w.append("(CASE WHEN ").expr(self, a1()?, 0)?.append(" = 0 OR abs(").expr(self, a1()?, 0)?;
                    w.append(") > length(").piece(&*list)?.append(") THEN NULL ELSE arrayElement(").piece(&*list)?;
                    w.append(", ").expr(self, a1()?, 0)?.append(") END)");
                }
            }
            SqlFn::ListPosition => {
                w.append("nullIf(toInt64(indexOf(").piece(&*self.list(xs))?.append(", ").expr(self, a1()?, 0)?;
                w.append(")), 0)");
            }
            SqlFn::ListDistinct => {
                w.append("arrayDistinct(").piece(&*self.present(xs))?.append(")");
            }
            SqlFn::ListAppend => {
                w.append("arrayPushBack(").piece(&*self.list(xs))?.append(", ").expr(self, a1()?, 0)?.append(")");
            }
            SqlFn::ListSum => {
                w.piece(&*self.reduced("sum", xs))?;
            }
            SqlFn::ListMin => {
                w.piece(&*self.reduced("min", xs))?;
            }
            SqlFn::ListMax => {
                w.piece(&*self.reduced("max", xs))?;
            }
            SqlFn::ListAvg => {
                w.append("CAST(").piece(&*self.reduced("avg", xs))?.append(" AS Nullable(Float64))");
            }
            SqlFn::ListMedian => {
                w.piece(&*self.reduced("quantileExactInclusive(0.5)", xs))?;
            }
            SqlFn::ListMode => {
                w.piece(&*ClickHouse::mode_of(self.present(xs)))?;
            }
            SqlFn::ListBoolAnd | SqlFn::ListBoolOr => {
                let present = self.present(xs);
                let all = if f == SqlFn::ListBoolAnd { "arrayAll" } else { "arrayExists" };
                w.append("if(empty(").piece(&*present)?.append(&format!("), NULL, {all}(\"__x\" -> \"__x\", "));
                w.piece(&*present)?.append("))");
            }
            SqlFn::ListProduct => {
                w.append("arrayProduct(arrayMap(\"__x\" -> coalesce(\"__x\", 1), ").piece(&*self.list(xs))?;
                w.append("))");
            }
            SqlFn::ListSort => {
                w.append("arraySort(").piece(&*self.list(xs))?.append(")");
            }
            SqlFn::ListSortDesc => {
                w.append("arrayReverseSort(").piece(&*self.list(xs))?.append(")");
            }
            SqlFn::ListReverse => {
                w.append("arrayReverse(").piece(&*self.list(xs))?.append(")");
            }
            SqlFn::ListTail => {
                w.append("arraySlice(").piece(&*self.list(xs))?.append(", 2)");
            }
            SqlFn::ListInit => {
                w.append("arrayPopBack(").piece(&*self.list(xs))?.append(")");
            }
            // DuckDB's list_slice: 1-based, both ends inclusive
            SqlFn::ListSlice => {
                w.append("arraySlice(").piece(&*self.list(xs))?.append(", greatest(").expr(self, a1()?, 0)?;
                w.append(", 1), greatest(").expr(self, arg(args, 2)?, 6)?.append(" - greatest(");
                w.expr(self, a1()?, 0)?.append(", 1) + 1, 0))");
            }
            // DuckDB's list_zip: pairs by position; each list cut to the shorter (truncate) or padded to the longer
            SqlFn::ListZip => {
                let last = args.last();
                let flagged = args.len() > 2 && matches!(last, Some(SqlExpr::BoolLit { .. }));
                let truncate = flagged && matches!(last, Some(SqlExpr::BoolLit { value: true, .. }));
                let lists = if flagged { &args[..args.len() - 1] } else { args };
                let n = |w: &mut SqlWriter| -> Result<(), Refusal> {
                    w.append(if truncate { "least(" } else { "greatest(" }).join(lists, ", ", |v, l| {
                        v.append("length(").piece(&*self.list(l))?.append(")");
                        Ok(())
                    })?;
                    w.append(")");
                    Ok(())
                };
                w.append("arrayZip(").join(lists, ", ", |w, l| {
                    w.append("arrayResize(arrayMap(\"__z\" -> toNullable(\"__z\"), ").piece(&*self.list(l))?;
                    w.append("), ").piece(&n)?.append(", NULL)");
                    Ok(())
                })?;
                w.append(")");
            }
            // DuckDB's list_reduce: the first element the accumulator's start, the lambda (acc, x) over the rest
            SqlFn::ListReduce => {
                let element = list_element(xs);
                let list = self.list(xs);
                w.append("arrayFold(").expr(self, a1()?, 0)?.append(", arraySlice(").piece(&*list)?.append(", 2), ");
                match element {
                    None => {
                        w.append("arrayElement(").piece(&*list)?.append(", 1))");
                    }
                    Some(element) => {
                        w.append("CAST(arrayElement(").piece(&*list)?;
                        let t = cast_type_name(self, element)?;
                        w.append(&format!(", 1) AS {t}))"));
                    }
                }
            }
            // the value's type name, its Nullable wrapper dropped
            SqlFn::Typeof => {
                w.append("replaceRegexpOne(toTypeName(").expr(self, xs, 0)?;
                w.append(r"), '^Nullable\((.*)\)$', '\1')");
            }
            SqlFn::RangeFn | SqlFn::RepeatValue => {
                return Err(Refusal::illegal_state(format!("{f} is rendered by call")));
            }
            other => return Err(Refusal::illegal_state(format!("not a list call: {other}"))),
        }
        Ok(())
    }

    pub(super) fn reduce_collection_arm(
        &self,
        w: &mut SqlWriter,
        reducer: AggFn,
        collection: &SqlExpr,
        extras: &[SqlExpr],
    ) -> Result<(), Refusal> {
        let present = self.present(collection);
        match reducer {
            // NULL over no present element, as string_agg over no row
            AggFn::StringAgg => {
                if extras.len() != 1 {
                    return Err(Refusal::capability(format!(
                        "a list's STRING_AGG of {} extras reached ClickHouse",
                        extras.len()
                    )));
                }
                w.append("if(empty(").piece(&*present)?.append("), NULL, arrayStringConcat(arrayMap(\"__x\" ->");
                w.append(" toString(\"__x\"), ").piece(&*present)?.append("), ").expr(self, &extras[0], 0)?;
                w.append("))");
            }
            AggFn::Count => {
                w.append("toInt64(length(").piece(&*present)?.append("))");
            }
            AggFn::Min | AggFn::BoolAnd => {
                w.piece(&*self.reduced("min", collection))?;
            }
            AggFn::Max | AggFn::BoolOr => {
                w.piece(&*self.reduced("max", collection))?;
            }
            AggFn::Sum => {
                w.piece(&*self.reduced("sum", collection))?;
            }
            AggFn::Avg => {
                w.piece(&*self.reduced("avg", collection))?;
            }
            AggFn::StddevSamp | AggFn::Stddev => {
                w.piece(&*self.reduced("stddevSampStable", collection))?;
            }
            AggFn::StddevPop => {
                w.piece(&*self.reduced("stddevPopStable", collection))?;
            }
            AggFn::VarSamp | AggFn::Variance => {
                w.piece(&*self.reduced("varSampStable", collection))?;
            }
            AggFn::VarPop => {
                w.piece(&*self.reduced("varPopStable", collection))?;
            }
            AggFn::Median => {
                w.piece(&*self.reduced("quantileExactInclusive(0.5)", collection))?;
            }
            // a parametric aggregate's level is a constant of its name
            AggFn::QuantileCont => match literal_level(extras) {
                Some(l) => {
                    w.piece(&*self.reduced(&format!("quantileExactInclusive({l})"), collection))?;
                }
                None => {
                    w.piece(&*self.interpolated(collection, level(extras)?))?;
                }
            },
            AggFn::Mode => {
                w.piece(&*ClickHouse::mode_of(present))?;
            }
            other => return Err(Refusal::capability(format!("a list reduced by {other} reached ClickHouse"))),
        }
        Ok(())
    }

    /// Several lists exploded in one select align: one ARRAY JOIN of the lists zipped, each padded with NULL to the
    /// longest, every column its position of the tuple (Java's `select`).
    pub(super) fn select_arm(&self, w: &mut SqlWriter, s: &SqlSelect, depth: usize) -> Result<(), Refusal> {
        let unnests =
            s.projections.iter().filter(|p| matches!(p.expr, SqlExpr::Call { r#fn: SqlFn::Unnest, .. })).count();
        if unnests <= 1 {
            return ansi::select(self, w, s, depth);
        }
        if unnests != s.projections.len()
            || s.distinct
            || s.r#where.is_some()
            || !s.group_by.is_empty()
            || s.having.is_some()
            || s.qualify.is_some()
            || !s.order_by.is_empty()
            || s.limit.is_some()
            || s.offset.is_some()
        {
            return Err(Refusal::capability(
                "several unnests in one select, beside other columns or clauses, reached ClickHouse: its arrayJoins \
                 cross, where DuckDB's unnests align",
            ));
        }
        let mut lists = Vec::with_capacity(s.projections.len());
        for p in &s.projections {
            let SqlExpr::Call { args, .. } = &p.expr else { unreachable!("an unnest") };
            if args.len() != 1 {
                return Err(Refusal::capability(format!("an unnest of {} lists reached ClickHouse", args.len())));
            }
            lists.push(&args[0]);
        }
        w.append("SELECT ");
        for (i, p) in s.projections.iter().enumerate() {
            w.append(if i > 0 { ", " } else { "" }).append(&format!("tupleElement(\"__zj\", {})", i + 1));
            if let Some(name) = output_name(p) {
                let a = self.alias_ident(&name);
                w.append(" AS ").append(&a);
            }
        }
        self.nl(w, depth);
        w.append("FROM ");
        if matches!(s.from, SqlSource::Dual) {
            w.append("system.one");
        } else {
            self.source(w, &s.from, depth)?;
        }
        let n = |w: &mut SqlWriter| -> Result<(), Refusal> {
            w.append("greatest(").join(&lists, ", ", |v, l| {
                v.append("length(").piece(&*self.list(l))?.append(")");
                Ok(())
            })?;
            w.append(")");
            Ok(())
        };
        self.nl(w, depth);
        w.append("ARRAY JOIN arrayZip(").join(&lists, ", ", |w, l| {
            w.append("arrayResize(arrayMap(\"__z\" -> toNullable(\"__z\"), ").piece(&*self.list(l))?;
            w.append("), ").piece(&n)?.append(", NULL)");
            Ok(())
        })?;
        w.append(") AS \"__zj\"");
        Ok(())
    }

    /// A Tuple of the struct's other fields and the new one, named (Java's `structInsert`).
    pub(super) fn struct_insert_arm(&self, w: &mut SqlWriter, args: &[SqlExpr]) -> Result<(), Refusal> {
        let untyped = || Refusal::capability("struct_insert over an untyped struct reached ClickHouse");
        let Some(SqlType::Struct { fields }) = type_of(arg(args, 0)?) else {
            return Err(untyped());
        };
        let SqlExpr::StringLit { value: name, .. } = arg(args, 1)? else {
            return Err(untyped());
        };
        let Some(added) = type_of(arg(args, 2)?) else {
            return Err(Refusal::capability("struct_insert of a value of unknown type reached ClickHouse"));
        };
        let kept: Vec<&StructField> = fields.iter().filter(|f| f.name != *name).collect();
        let mut all: Vec<StructField> = kept.iter().map(|f| (*f).clone()).collect();
        all.push(StructField { name: name.clone(), r#type: added.clone() });
        w.append("CAST(tuple(");
        for f in &kept {
            let lit = self.string_lit(&f.name)?;
            w.append("tupleElement(").expr(self, &args[0], 0)?.append(", ").append(&lit).append("), ");
        }
        w.expr(self, &args[2], 0)?.append(") AS ");
        let t = cast_type_name(self, &SqlType::Struct { fields: all })?;
        w.append(&t).append(")");
        Ok(())
    }

    // ==================================================================
    // Variant: a JSON value's text (String)
    // ==================================================================

    pub(super) fn variant_construct_arm(&self, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
        if a.len() != 1 {
            return Err(Refusal::capability(format!("toVariant of {} arguments reached ClickHouse", a.len())));
        }
        // a JSON value is its own; a NULL stays SQL's NULL
        if is_scalar(&a[0], Scalar::Json) {
            w.expr(self, &a[0], 0)?;
            return Ok(());
        }
        match null_origin(&a[0]) {
            Some(origin) => self.null_aware_json(w, &a[0], origin),
            None => {
                w.append("toJSONString(").expr(self, &a[0], 0)?.append(")");
                Ok(())
            }
        }
    }

    /// `x`'s JSON, NULL where its list is DuckDB's NULL list (Java's `nullAwareJson`).
    fn null_aware_json(&self, w: &mut SqlWriter, x: &SqlExpr, origin: &SqlExpr) -> Result<(), Refusal> {
        if let SqlExpr::Case { whens, otherwise, .. } = x {
            w.append("(CASE");
            for wh in whens {
                w.append(" WHEN ").expr(self, &wh.condition, 0)?.append(" THEN ");
                self.branch_json(w, &wh.then, origin)?;
            }
            if let Some(o) = otherwise {
                w.append(" ELSE ");
                self.branch_json(w, o, origin)?;
            }
            w.append(" END)");
            return Ok(());
        }
        w.append("if(coalesce(JSONType(").expr(self, origin, 0)?.append("), 'Null') = 'Null', NULL, toJSONString(");
        w.expr(self, x, 0)?.append("))");
        Ok(())
    }

    fn branch_json(&self, w: &mut SqlWriter, branch: &SqlExpr, origin: &SqlExpr) -> Result<(), Refusal> {
        if is_raise(branch) {
            w.append("CAST(").expr(self, branch, 0)?.append(" AS Nullable(String))");
            return Ok(());
        }
        self.null_aware_json(w, branch, origin)
    }

    /// A key or a 0-based index of a JSON value's text: the member's raw JSON (Java's `variantGet`).
    pub(super) fn variant_get_arm(&self, w: &mut SqlWriter, args: &[SqlExpr]) -> Result<(), Refusal> {
        let key = arg(args, 1)?;
        if let SqlExpr::StringLit { value, .. } = key
            && value == "$"
        {
            w.expr(self, arg(args, 0)?, 0)?;
            return Ok(());
        }
        w.append("nullIf(JSONExtractRaw(").expr(self, arg(args, 0)?, 0)?.append(", ");
        if is_scalar(key, Scalar::Varchar) || matches!(key, SqlExpr::StringLit { .. }) {
            w.expr(self, key, 0)?;
        } else {
            // JSON's indexes are 1-based, a negative one from the end
            w.append("if(").expr(self, key, 0)?.append(" >= 0, ").expr(self, key, 6)?.append(" + 1, ");
            w.expr(self, key, 0)?.append(")");
        }
        w.append("), '')");
        Ok(())
    }

    /// Every cast names the nullable type; the casts ClickHouse reads differently from DuckDB, each its own (Java's
    /// `variantAwareCast`).
    pub(super) fn variant_aware_cast_arm(
        &self,
        w: &mut SqlWriter,
        value: &SqlExpr,
        target: &SqlType,
        conform: bool,
    ) -> Result<(), Refusal> {
        let target_is = |s: Scalar| matches!(target, SqlType::Scalar(t) if *t == s);
        // an array is never NULL here: no list is the empty one
        if matches!(value, SqlExpr::NullLit { .. }) && matches!(target, SqlType::Array { .. }) {
            let t = cast_type_name(self, target)?;
            w.append(&format!("CAST([] AS {t})"));
            return Ok(());
        }
        // a zoned timestamp from text: ISO 8601's T, Z and offsets
        if target_is(Scalar::Timestamptz) && is_scalar(value, Scalar::Varchar) {
            w.append("parseDateTime64BestEffort(").expr(self, value, 0)?.append(", 6, 'UTC')");
            return Ok(());
        }
        if let SqlType::Decimal { precision, scale } = target {
            // text to a decimal rounds at the scale, as DuckDB's
            if is_scalar(value, Scalar::Varchar) {
                w.append("CAST(round(toDecimal128(").expr(self, value, 0)?;
                w.append(&format!(", 18), {scale}) AS Nullable(Decimal({precision}, {scale})))"));
                return Ok(());
            }
            w.append("CAST(").expr(self, value, 0)?.append(&format!(" AS Nullable(Decimal({precision}, {scale})))"));
            return Ok(());
        }
        // a Float literal's text: its shortest digits, one fraction digit at least
        if target_is(Scalar::Varchar)
            && let SqlExpr::FloatLit { value: f, .. } = value
            && let Some(plain) = crate::java::float_decimal(f.value())?
        {
            w.append("toDecimalString(").expr(self, value, 0)?;
            w.append(&format!(", {})", stripped_scale(&plain).max(1)));
            return Ok(());
        }
        // a Decimal's text at its scale
        if target_is(Scalar::Varchar)
            && let Some(SqlType::Decimal { scale, .. }) = type_of(value)
        {
            if matches!(value, SqlExpr::FloatLit { .. } | SqlExpr::DecimalLit { .. }) {
                w.append("toDecimalString(").expr(self, value, 0)?.append(&format!(", {scale})"));
                return Ok(());
            }
            w.append("toDecimalString(").expr(self, value, 0)?.append(", toUInt8(extract(toTypeName(");
            w.expr(self, value, 0)?.append(r"), ', (\\d+)\\)')))");
            return Ok(());
        }
        // a double's text: its shortest digits, and a fraction digit where it has none
        if target_is(Scalar::Varchar) && is_scalar(value, Scalar::Double) {
            w.append("replaceRegexpOne(toString(").expr(self, value, 0)?.append(r"), '^(-?[0-9]+)$', '\\1.0')");
            return Ok(());
        }
        // a Variant's member as text: a JSON string unquoted, any other value its JSON
        if is_scalar(value, Scalar::Json) && target_is(Scalar::Varchar) {
            if !matches!(value, SqlExpr::Call { r#fn: SqlFn::VariantGet, .. }) {
                w.expr(self, value, 0)?;
                return Ok(());
            }
            w.append("if(JSONType(").expr(self, value, 0)?.append(") = 'String', JSONExtractString(");
            w.expr(self, value, 0)?.append("), ").expr(self, value, 0)?.append(")");
            return Ok(());
        }
        // a Variant as a scalar: extracted to the type
        if is_scalar(value, Scalar::Json)
            && (target_is(Scalar::Bigint)
                || target_is(Scalar::Integer)
                || target_is(Scalar::Double)
                || target_is(Scalar::Boolean))
        {
            w.append("JSONExtract(").expr(self, value, 0)?.append(", ");
            let lit = self.string_lit(&cast_type_name(self, target)?)?;
            w.append(&lit).append(")");
            return Ok(());
        }
        // a Variant as a list or a map: extracted to the type, from the JSON null where there is no value
        if is_scalar(value, Scalar::Json) && matches!(target, SqlType::Array { .. } | SqlType::Map { .. }) {
            w.append("JSONExtract(coalesce(").expr(self, value, 0)?.append(", 'null'), ");
            let lit = self.string_lit(&cast_type_name(self, target)?)?;
            w.append(&lit).append(")");
            return Ok(());
        }
        ansi::variant_aware_cast(self, w, value, target, conform)
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
            // two instances' equality: each instance in a list (an array compares its elements NULL-safe)
            SqlFn::Equal | SqlFn::NotEqual => {
                if a.len() == 2
                    && matches!(type_of(&a[0]), Some(SqlType::Struct { .. }))
                    && matches!(type_of(&a[1]), Some(SqlType::Struct { .. }))
                {
                    w.append("([").expr(self, &a[0], 0)?.append(if f == SqlFn::Equal { "] = [" } else { "] != [" });
                    w.expr(self, &a[1], 0)?.append("])");
                    return Ok(());
                }
                ansi::infix(self, w, f, a, parent_prec)
            }
            // the shared spellings (the base's named helpers)
            SqlFn::Not => ansi::not_prefix(self, w, a, parent_prec),
            SqlFn::Negate => ansi::negation(self, w, a),
            SqlFn::IsNull => ansi::is_null_test(self, w, a),
            SqlFn::IsNotNull => ansi::is_not_null_test(self, w, a),
            SqlFn::In => ansi::in_list(self, w, a, parent_prec),
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
            // Pure's divide is a Float division: both operands DOUBLE
            SqlFn::Divide => ansi::divide_as_doubles(self, w, a),
            SqlFn::Xor => ansi::xor_as_and_or(self, w, a, parent_prec),
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
            SqlFn::Acos => plain(w, "acos"),
            SqlFn::Asin => plain(w, "asin"),
            SqlFn::Atan => plain(w, "atan"),
            SqlFn::Atan2 => plain(w, "atan2"),
            SqlFn::Cbrt => plain(w, "cbrt"),
            SqlFn::Coalesce => plain(w, "coalesce"),
            SqlFn::Cos => plain(w, "cos"),
            SqlFn::Cosh => plain(w, "cosh"),
            SqlFn::Degrees => plain(w, "degrees"),
            SqlFn::Log10 => plain(w, "log10"),
            SqlFn::Pow => plain(w, "pow"),
            SqlFn::Radians => plain(w, "radians"),
            SqlFn::Sin => plain(w, "sin"),
            SqlFn::Sinh => plain(w, "sinh"),
            SqlFn::Sqrt => plain(w, "sqrt"),
            SqlFn::Tan => plain(w, "tan"),
            SqlFn::ListFlatten => plain(w, "arrayFlatten"),
            SqlFn::MapKeys => plain(w, "mapKeys"),
            SqlFn::MapValues => plain(w, "mapValues"),
            SqlFn::StartsWith => plain(w, "startsWith"),
            SqlFn::EndsWith => plain(w, "endsWith"),
            SqlFn::ListReverse => plain(w, "arrayReverse"),
            SqlFn::JaroWinkler => plain(w, "jaroWinklerSimilarity"),
            SqlFn::Chr => plain(w, "char"),
            // ClickHouse's current_date is a function: the bare keyword is an unknown identifier
            SqlFn::Today => Err(Refusal::capability("today() reached ClickHouse, which has no bare current_date")),
            // an integer's abs is a UInt64 here: back to an Int64, raising where it does not fit
            SqlFn::Abs => {
                if is_scalar(a0()?, Scalar::Bigint) || is_scalar(a0()?, Scalar::Integer) {
                    w.append("accurateCast(abs(").expr(self, a0()?, 0)?.append("), 'Int64')");
                } else {
                    w.append("abs(").expr(self, a0()?, 0)?.append(")");
                }
                Ok(())
            }
            // a raise: a failed cast of its message's text raises it, the platform's markers around it
            SqlFn::Error => self.error(w, a),
            SqlFn::IsDistinctFrom | SqlFn::NullSafeNotEqual | SqlFn::NullSafeEqual => {
                if f != SqlFn::NullSafeEqual {
                    w.append("NOT ");
                }
                w.append("coalesce(").expr(self, a0()?, 4)?.append(" = ").expr(self, a1()?, 4)?;
                w.append(", isNull(").expr(self, a0()?, 0)?.append(") AND isNull(").expr(self, a1()?, 0)?.append("))");
                Ok(())
            }
            // NULL-ignoring, as DuckDB's concat
            SqlFn::Concat => {
                w.append("concat(").join(&ansi::flatten_concat(a), ", ", |w, e| self.as_text(w, e))?.append(")");
                Ok(())
            }
            SqlFn::ConcatJoin => {
                w.append("concat(").join(a, ", ", |w, e| self.as_text(w, e))?.append(")");
                Ok(())
            }
            // each its non-raising form under the guard; Pure's mod takes the divisor's sign
            SqlFn::Rem => {
                w.append("(CASE WHEN ").expr(self, a1()?, 0)?.append(" = 0 THEN NULL ELSE ").append(modulo(a)?);
                w.append("(").piece(&*operand(self, a, 0)?)?.append(", ").piece(&*operand(self, a, 1)?)?;
                w.append(") END)");
                Ok(())
            }
            SqlFn::Mod => {
                let m = modulo(a)?;
                w.append("(CASE WHEN ").expr(self, a1()?, 0)?.append(" = 0 THEN NULL ELSE ").append(m);
                w.append("(").append(m).append("(").piece(&*operand(self, a, 0)?)?.append(", ");
                w.piece(&*operand(self, a, 1)?)?.append(") + ").piece(&*operand(self, a, 1)?)?.append(", ");
                w.piece(&*operand(self, a, 1)?)?.append(") END)");
                Ok(())
            }
            SqlFn::IntDivide => {
                w.append("(CASE WHEN ").expr(self, a1()?, 0)?.append(" = 0 THEN NULL ELSE intDivOrZero(");
                w.expr(self, a0()?, 0)?.append(", ").expr(self, a1()?, 0)?.append(") END)");
                Ok(())
            }
            SqlFn::BoolToText => self.wrap(w, "toString(", a0()?, ")"),
            SqlFn::BitNot => self.wrap(w, "bitNot(toInt64(", a0()?, "))"),
            SqlFn::FloorRaw => self.wrap(w, "floor(", a0()?, ")"),
            SqlFn::Cot => self.wrap(w, "(1 / tan(", a0()?, "))"),
            SqlFn::RoundHalfUp => {
                w.append("round(toDecimal128(").expr(self, a0()?, 0)?.append(", 18)");
                if a.len() > 1 {
                    w.append(", ").expr(self, &a[1], 0)?;
                }
                w.append(")");
                Ok(())
            }
            // exp and log are approximations here: pow and log2 are exact
            SqlFn::Exp => self.wrap(w, "pow(e(), toFloat64(", a0()?, "))"),
            SqlFn::Ln => self.wrap(w, "(log2(toFloat64(", a0()?, ")) / log2(e()))"),
            SqlFn::Tanh => self.wrap(w, "(1 - 2 / (pow(e(), 2 * toFloat64(", a0()?, ")) + 1))"),
            // a Decimal and a Float64 have no supertype here: unified
            SqlFn::Greatest | SqlFn::Least => {
                let unify = super::needs_unifying(a);
                w.append(if f == SqlFn::Greatest { "greatest(" } else { "least(" });
                w.join(a, ", ", |w, e| self.unified_member(w, e, unify))?.append(")");
                Ok(())
            }
            // ---- strings (UTF-8: the plain ones count bytes)
            SqlFn::Length => self.wrap(w, "toInt64(lengthUTF8(", a0()?, "))"),
            SqlFn::Lower => self.wrap(w, "lowerUTF8(", a0()?, ")"),
            SqlFn::Upper => self.wrap(w, "upperUTF8(", a0()?, ")"),
            SqlFn::UcFirst | SqlFn::LcFirst => {
                let case = if f == SqlFn::UcFirst { "upperUTF8" } else { "lowerUTF8" };
                w.append(&format!("concat({case}(substringUTF8(")).expr(self, a0()?, 0)?;
                w.append(", 1, 1)), substringUTF8(").expr(self, a0()?, 0)?.append(", 2))");
                Ok(())
            }
            SqlFn::Substring => self.listed(w, "substringUTF8(", a, ")"),
            SqlFn::Left => self.listed(w, "leftUTF8(", a, ")"),
            SqlFn::Right => self.listed(w, "rightUTF8(", a, ")"),
            SqlFn::Strpos => self.listed(w, "toInt64(positionUTF8(", a, "))"),
            SqlFn::ReverseString => self.wrap(w, "reverseUTF8(", a0()?, ")"),
            SqlFn::Replace => self.listed(w, "replaceAll(", a, ")"),
            // translateUTF8 maps any character
            SqlFn::Translate => plain(w, "translateUTF8"),
            SqlFn::Levenshtein => self.listed(w, "toInt64(editDistanceUTF8(", a, "))"),
            SqlFn::RepeatStr => {
                w.append("repeat(").expr(self, a0()?, 0)?.append(", toUInt64(greatest(").expr(self, a1()?, 0)?;
                w.append(", 0)))");
                Ok(())
            }
            SqlFn::AsciiCode => self.wrap(w, "toInt64(ascii(", a0()?, "))"),
            // DuckDB's split_part: the empty string past the end; the text past NULL, its answer NULL's
            SqlFn::SplitPart => {
                w.append("if(isNull(").expr(self, a0()?, 0)?.append("), NULL, coalesce(arrayElement(");
                w.append("splitByString(").expr(self, a1()?, 0)?.append(", ifNull(").expr(self, a0()?, 0)?;
                w.append(", '')), ").expr(self, arg(a, 2)?, 0)?.append("), ''))");
                Ok(())
            }
            SqlFn::Matches => {
                w.append("match(").expr(self, a0()?, 0)?.append(", ").expr(self, a1()?, 0)?.append(")");
                Ok(())
            }
            SqlFn::RegexpFullMatch => {
                w.append("match(").expr(self, a0()?, 0)?.append(", ");
                let anchored = self.anchored(a1()?)?;
                w.piece(&*anchored)?.append(")");
                Ok(())
            }
            SqlFn::RegexpExtract => {
                w.append("regexpExtract(").expr(self, a0()?, 0)?.append(", ").expr(self, a1()?, 0)?;
                if a.len() > 2 && !matches!(a[2], SqlExpr::IntLit { value: 0, .. }) {
                    w.append(", ").expr(self, &a[2], 0)?.append(")");
                } else {
                    w.append(", 0)");
                }
                Ok(())
            }
            // extractAll answers the first group: the pattern in one group of its own is the whole match
            SqlFn::RegexpExtractAll => {
                w.append("if(isNull(").expr(self, a0()?, 0)?.append("), [], ");
                let text = |w: &mut SqlWriter| -> Result<(), Refusal> {
                    w.append("ifNull(").expr(self, arg(a, 0)?, 0)?.append(", '')");
                    Ok(())
                };
                if a.len() > 2 && !matches!(a[2], SqlExpr::IntLit { value: 0, .. }) {
                    w.append("arrayMap(\"__g\" -> arrayElement(\"__g\", ").expr(self, &a[2], 0)?;
                    w.append("), extractAllGroupsVertical(").piece(&text)?.append(", ").expr(self, a1()?, 0)?;
                    w.append("))");
                } else {
                    w.append("extractAll(").piece(&text)?.append(", concat('(', ").expr(self, a1()?, 0)?;
                    w.append(", ')'))");
                }
                w.append(")");
                Ok(())
            }
            // DuckDB replaces the first match unless 'g'
            SqlFn::RegexpReplace => {
                let global = a.len() > 3 && matches!(&a[3], SqlExpr::StringLit { value, .. } if value.contains('g'));
                w.append(if global { "replaceRegexpAll(" } else { "replaceRegexpOne(" }).list(self, head(a, 3)?)?;
                w.append(")");
                Ok(())
            }
            SqlFn::Split => {
                w.append("if(isNull(").expr(self, a0()?, 0)?.append("), [], splitByString(").expr(self, a1()?, 0)?;
                w.append(", ifNull(").expr(self, a0()?, 0)?.append(", '')))");
                Ok(())
            }
            SqlFn::Trim | SqlFn::Ltrim | SqlFn::Rtrim => {
                let side = match f {
                    SqlFn::Trim => "BOTH",
                    SqlFn::Ltrim => "LEADING",
                    _ => "TRAILING",
                };
                if a.len() == 1 {
                    w.append(&format!("trim({side} ' ' FROM ")).expr(self, &a[0], 0)?.append(")");
                } else {
                    w.append(&format!("trim({side} ")).expr(self, a1()?, 0)?.append(" FROM ").expr(self, a0()?, 0)?;
                    w.append(")");
                }
                Ok(())
            }
            SqlFn::EncodeBase64 => self.wrap(w, "base64Encode(", a0()?, ")"),
            SqlFn::DecodeBase64 => self.wrap(w, "base64Decode(", a0()?, ")"),
            SqlFn::Md5 => self.wrap(w, "lower(hex(MD5(", a0()?, ")))"),
            SqlFn::Sha1 => self.wrap(w, "lower(hex(SHA1(", a0()?, ")))"),
            SqlFn::Sha256 => self.wrap(w, "lower(hex(SHA256(", a0()?, ")))"),
            SqlFn::Guid => {
                w.append("toString(generateUUIDv4())");
                Ok(())
            }
            SqlFn::Random => {
                w.append("randCanonical()");
                Ok(())
            }
            // printf takes native numbers and strings: each value as printf takes it
            SqlFn::Format => self.format_call(w, a),
            SqlFn::CurrentUserFn => {
                w.append("currentUser()");
                Ok(())
            }
            // ---- temporal
            SqlFn::Now
            | SqlFn::DateTruncDay
            | SqlFn::DateTrunc
            | SqlFn::ParseDate
            | SqlFn::Extract
            | SqlFn::MakeDate
            | SqlFn::MakeTimestamp
            | SqlFn::AddInterval
            | SqlFn::AddIntervalTemporal
            | SqlFn::DateDiff
            | SqlFn::TimeBucket
            | SqlFn::Timezone
            | SqlFn::DateAtTime
            | SqlFn::EpochSeconds
            | SqlFn::EpochMs
            | SqlFn::EpochNs
            | SqlFn::FromEpochNs
            | SqlFn::FromEpochSeconds
            | SqlFn::FromEpochMs
            | SqlFn::Dayname
            | SqlFn::Monthname
            | SqlFn::Strftime
            | SqlFn::Strptime => self.temporal(w, f, a, t),
            // ---- lists whose first argument is no list
            SqlFn::ListLength => {
                w.append("toInt64(length(").piece(&*self.list(a0()?))?.append("))");
                Ok(())
            }
            // DuckDB's range: [start, stop) by step
            SqlFn::RangeFn => {
                w.append("arrayMap(\"__g\" -> toInt64(\"__g\"), range(");
                match a.len() {
                    1 => {
                        w.append("toInt64(").expr(self, &a[0], 0)?.append(")))");
                    }
                    3 => {
                        // a constant step is folded before its guard runs
                        w.expr(self, &a[0], 0)?.append(", ").expr(self, &a[1], 0)?.append(", materialize(");
                        w.expr(self, &a[2], 0)?.append(")))");
                    }
                    _ => {
                        w.list(self, a)?.append("))");
                    }
                }
                Ok(())
            }
            SqlFn::RepeatValue => {
                w.append("arrayWithConstant(toUInt64(greatest(").expr(self, a1()?, 0)?.append(", 0)), ");
                w.expr(self, a0()?, 0)?.append(")");
                Ok(())
            }
            // ---- maps
            SqlFn::MapEmpty => {
                w.append("map()");
                Ok(())
            }
            SqlFn::MapFromLists => self.listed(w, "mapFromArrays(", a, ")"),
            // the entries are (key, value) tuples
            SqlFn::MapFromEntries => {
                w.append("mapFromArrays(arrayMap(\"__e\" -> tupleElement(\"__e\", 1), ").expr(self, a0()?, 0)?;
                w.append("), arrayMap(\"__e\" -> tupleElement(\"__e\", 2), ").expr(self, a0()?, 0)?.append("))");
                Ok(())
            }
            SqlFn::MapExtract => {
                w.append("if(mapContains(").expr(self, a0()?, 0)?.append(", ").expr(self, a1()?, 0)?.append("), [");
                w.expr(self, a0()?, 0)?.append("[").expr(self, a1()?, 0)?.append("]], [])");
                Ok(())
            }
            SqlFn::MapConcat => self.listed(w, "mapUpdate(", a, ")"),
            // ---- variant: in DuckDB's json_type vocabulary
            SqlFn::JsonType => {
                let x = a0()?;
                w.append("multiIf(isNull(").expr(self, x, 0)?.append("), NULL, JSONType(").expr(self, x, 0)?;
                w.append(") = 'Null', 'NULL', JSONType(").expr(self, x, 0)?;
                w.append(") = 'String', 'VARCHAR', JSONType(").expr(self, x, 0)?;
                w.append(") = 'Bool', 'BOOLEAN', JSONType(").expr(self, x, 0)?;
                w.append(") IN ('Int64', 'UInt64'), 'BIGINT', JSONType(").expr(self, x, 0)?;
                w.append(") = 'Array', 'ARRAY', JSONType(").expr(self, x, 0)?.append(") = 'Object', 'OBJECT', 'DOUBLE')");
                Ok(())
            }
            SqlFn::JsonArrayLength => self.wrap(w, "JSONLength(", a0()?, ")"),
            SqlFn::JsonPretty => Err(wall(f, "ClickHouse has no JSON pretty-printer")),
            SqlFn::JsonMergePatch => Err(wall(f, "ClickHouse has no JSON merge patch")),
        }
    }

    /// `open` `x` `close`.
    fn wrap(&self, w: &mut SqlWriter, open: &str, x: &SqlExpr, close: &str) -> Result<(), Refusal> {
        w.append(open).expr(self, x, 0)?.append(close);
        Ok(())
    }

    /// `open` the arguments, comma-separated, `close`.
    fn listed(&self, w: &mut SqlWriter, open: &str, a: &[SqlExpr], close: &str) -> Result<(), Refusal> {
        w.append(open).list(self, a)?.append(close);
        Ok(())
    }

    /// A CONCAT argument as text (ClickHouse's concat is NULL if any argument is: a NULL is '').
    fn as_text(&self, w: &mut SqlWriter, e: &SqlExpr) -> Result<(), Refusal> {
        w.append("coalesce(toString(").expr(self, e, 0)?.append("), '')");
        Ok(())
    }

    /// The `ERROR` arm: its text cast to an integer raises it, the platform's markers around it (Java's `error`).
    fn error(&self, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
        w.append("toInt64(materialize(concat(char(31), ");
        if a.len() > 1 {
            w.expr(self, &a[1], 0)?.append(", char(30), ");
        }
        // each quote travels as the envelope's quote mark
        w.append("replaceAll(toString(").expr(self, arg(a, 0)?, 0)?.append(r"), '\'', char(29)), char(31))))");
        Ok(())
    }

    fn anchored<'a>(&'a self, p: &'a SqlExpr) -> Result<P<'a>, Refusal> {
        if let SqlExpr::StringLit { value, .. } = p {
            return Ok(text(self.string_lit(&format!("^(?:{value})$"))?));
        }
        Ok(pc(move |w| {
            w.append("concat('^(?:', ").expr(self, p, 0)?.append(", ')$')");
            Ok(())
        }))
    }

    /// format(template, values...) as printf: each value as printf takes it (Java's `formatCall`).
    fn format_call(&self, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
        let template = arg(a, 0)?;
        let conversions = match template {
            SqlExpr::StringLit { value, .. } => Some(conversions(value)),
            _ => None,
        };
        w.append("printf(").expr(self, template, 0)?;
        for (i, x) in a.iter().enumerate().skip(1) {
            let real = matches!(type_of(x), Some(SqlType::Decimal { .. }))
                || matches!(x, SqlExpr::FloatLit { .. } | SqlExpr::DecimalLit { .. })
                || is_scalar(x, Scalar::Double);
            w.append(", ");
            let as_text = conversions.as_ref().is_some_and(|c| i - 1 < c.len() && c[i - 1] == 's');
            if real && as_text {
                let canon = crate::float_text::canon_of(build::cast(x.clone(), SqlType::Scalar(Scalar::Double)));
                w.expr(self, &canon, 0)?;
            } else if as_text
                && (is_scalar(x, Scalar::Boolean) || is_scalar(x, Scalar::Bigint) || is_scalar(x, Scalar::Integer))
            {
                // %s takes only a String: its text
                w.append("toString(").expr(self, x, 0)?.append(")");
            } else if real && !is_scalar(x, Scalar::Double) {
                w.append("toFloat64(").expr(self, x, 0)?.append(")");
            } else {
                w.expr(self, x, 0)?;
            }
        }
        w.append(")");
        Ok(())
    }

    /// Whether `date` moved by `n` of `unit` leaves ClickHouse's dates (Java's `outsideDates`).
    pub(super) fn outside_dates<'a>(&'a self, unit: DatePart, date: &'a SqlExpr, n: &'a SqlExpr) -> Result<P<'a>, Refusal> {
        let micros: i64 = match unit {
            DatePart::Year | DatePart::Month => 0,
            DatePart::Week => 604_800_000_000,
            DatePart::Day => 86_400_000_000,
            DatePart::Hour => 3_600_000_000,
            DatePart::Minute => 60_000_000,
            DatePart::Second => 1_000_000,
            DatePart::Millisecond => 1_000,
            DatePart::Microsecond => 1,
            DatePart::Quarter | DatePart::Nanosecond | DatePart::DayOfYear | DatePart::IsoDayOfWeek => {
                return Err(Refusal::illegal_state(format!(
                    "the unit {unit} reached the date range check: refused before it"
                )));
            }
        };
        Ok(match unit {
            DatePart::Year => pc(move |w| {
                w.append("(toInt128(toYear(").expr(self, date, 0)?.append(")) + toInt128(").expr(self, n, 0)?;
                w.append(")) NOT BETWEEN 1900 AND 2299");
                Ok(())
            }),
            DatePart::Month => pc(move |w| {
                w.append("intDiv(toInt128(toYear(").expr(self, date, 0)?.append(")) * 12 + toMonth(");
                w.expr(self, date, 0)?.append(") - 1 + toInt128(").expr(self, n, 0)?;
                w.append("), 12) NOT BETWEEN 1900 AND 2299");
                Ok(())
            }),
            _ => pc(move |w| {
                w.append("(toInt128(toUnixTimestamp64Micro(toDateTime64(").expr(self, date, 0)?;
                w.append(", 6, 'UTC'))) + toInt128(").expr(self, n, 0)?;
                w.append(&format!(") * {micros}) NOT BETWEEN -2208988800000000 AND 10413791999999999"));
                Ok(())
            }),
        })
    }

    /// DuckDB's time_bucket: a fixed unit binned from 1970-01-01 (a week from the Monday 1969-12-29) in epoch
    /// microseconds; a month or a year by the month index since 1970-01 (Java's `timeBucket`).
    fn time_bucket<'a>(&'a self, a: &'a [SqlExpr]) -> Result<P<'a>, Refusal> {
        let unit = part(arg(a, 0)?)?;
        let n: P<'a> = pc(move |w| {
            w.append("toInt64(").expr(self, arg(a, 1)?, 0)?.append(")");
            Ok(())
        });
        if let Some(micros) = bucket_micros(unit) {
            let origin: i64 = if unit == DatePart::Week { -3 * 86_400_000_000 } else { 0 };
            let x: P<'a> = pc(move |w| {
                w.append("(toUnixTimestamp64Micro(toDateTime64(").expr(self, arg(a, 2)?, 0)?;
                w.append(&format!(", 6, 'UTC')) - ({origin}))"));
                Ok(())
            });
            let u: P<'a> = {
                let n = n.clone();
                pc(move |w| {
                    w.append("(").piece(&*n)?.append(&format!(" * {micros})"));
                    Ok(())
                })
            };
            return Ok(pc(move |w| {
                w.append("fromUnixTimestamp64Micro(").piece(&*x)?.append(" - ((").piece(&*x)?.append(" % ");
                w.piece(&*u)?.append(") + ").piece(&*u)?.append(") % ").piece(&*u)?;
                w.append(&format!(" + ({origin}), 'UTC')"));
                Ok(())
            }));
        }
        let months = match unit {
            DatePart::Month => 1,
            DatePart::Year => 12,
            other => return Err(Refusal::capability(format!("a {other} time bucket reached ClickHouse"))),
        };
        let m: P<'a> = pc(move |w| {
            w.append("((toYear(").expr(self, arg(a, 2)?, 0)?.append(") - 1970) * 12 + toMonth(");
            w.expr(self, arg(a, 2)?, 0)?.append(") - 1)");
            Ok(())
        });
        let size: P<'a> = pc(move |w| {
            w.append("(").piece(&*n)?.append(&format!(" * {months})"));
            Ok(())
        });
        Ok(pc(move |w| {
            w.append("toDateTime64(addMonths(toDate32('1970-01-01'), ").piece(&*m)?.append(" - ((").piece(&*m)?;
            w.append(" % ").piece(&*size)?.append(") + ").piece(&*size)?.append(") % ").piece(&*size)?;
            w.append("), 6, 'UTC')");
            Ok(())
        }))
    }

    /// A date part as ClickHouse's function of it (Java's `extract`).
    fn extract(&self, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
        let p = part(arg(a, 0)?)?;
        let f = match p {
            DatePart::Year => "toYear",
            DatePart::Quarter => "toQuarter",
            DatePart::Month => "toMonth",
            DatePart::Week => "toISOWeek",
            DatePart::Day => "toDayOfMonth",
            DatePart::Hour => "toHour",
            DatePart::Minute => "toMinute",
            DatePart::Second => "toSecond",
            DatePart::DayOfYear => "toDayOfYear",
            DatePart::IsoDayOfWeek => "toDayOfWeek",
            DatePart::Millisecond | DatePart::Microsecond | DatePart::Nanosecond => {
                return Err(Refusal::capability(format!("date part {p} has no probed ClickHouse spelling")));
            }
        };
        w.append(&format!("toInt64({f}(")).expr(self, arg(a, 1)?, 0)?.append("))");
        Ok(())
    }

    /// The temporal arms of `call` (Java's `temporal`).
    pub(super) fn temporal(&self, w: &mut SqlWriter, f: SqlFn, a: &[SqlExpr], t: &TypeFact) -> Result<(), Refusal> {
        let a0 = || arg(a, 0);
        let a1 = || arg(a, 1);
        match f {
            SqlFn::Now => {
                w.append("now64(6, 'UTC')");
            }
            SqlFn::DateTruncDay => {
                w.append("toDate32(").expr(self, a0()?, 0)?.append(")");
            }
            SqlFn::DateTrunc => {
                let p = part(a0()?)?;
                let (open, close) = match p {
                    DatePart::Year => ("toDate32(toStartOfYear(", "))"),
                    DatePart::Quarter => ("toDate32(toStartOfQuarter(", "))"),
                    DatePart::Month => ("toDate32(toStartOfMonth(", "))"),
                    DatePart::Week => ("toDate32(toMonday(", "))"),
                    DatePart::Day => ("toDateTime64(toDate32(", "), 6, 'UTC')"),
                    DatePart::Hour => ("toStartOfHour(", ")"),
                    DatePart::Minute => ("toStartOfMinute(", ")"),
                    DatePart::Second => ("toStartOfSecond(", ")"),
                    DatePart::Millisecond
                    | DatePart::Microsecond
                    | DatePart::Nanosecond
                    | DatePart::DayOfYear
                    | DatePart::IsoDayOfWeek => {
                        return Err(Refusal::capability(format!("date_trunc {p} has no ClickHouse spelling")));
                    }
                };
                w.append(open).expr(self, a1()?, 0)?.append(close);
            }
            SqlFn::ParseDate => {
                w.append("parseDateTime64BestEffort(").expr(self, a0()?, 0)?.append(", 6, 'UTC')");
            }
            SqlFn::Extract => self.extract(w, a)?,
            SqlFn::MakeDate => {
                w.append("makeDate32(").list(self, a)?.append(")");
            }
            SqlFn::MakeTimestamp => {
                if a.len() != 6 {
                    return Err(wall(f, &format!("make_timestamp of {} arguments", a.len())));
                }
                w.append("addMicroseconds(toDateTime64(makeDateTime(").list(self, &a[..5])?;
                w.append(", 0, 'UTC'), 6, 'UTC'), toInt64(round(toFloat64(").expr(self, &a[5], 0)?;
                w.append(") * 1000000)))");
            }
            SqlFn::AddInterval | SqlFn::AddIntervalTemporal => {
                let unit = part(a0()?)?;
                let name = match unit {
                    DatePart::Year => "addYears",
                    DatePart::Month => "addMonths",
                    DatePart::Week => "addWeeks",
                    DatePart::Day => "addDays",
                    DatePart::Hour => "addHours",
                    DatePart::Minute => "addMinutes",
                    DatePart::Second => "addSeconds",
                    DatePart::Millisecond => "addMilliseconds",
                    DatePart::Microsecond => "addMicroseconds",
                    DatePart::Quarter | DatePart::Nanosecond | DatePart::DayOfYear | DatePart::IsoDayOfWeek => {
                        return Err(wall(f, &format!("the unit {unit}")));
                    }
                };
                // the date the arithmetic reaches, in a wider integer, is checked first and raised outside them
                // Java's `error(x, raised(DATE_RANGE))`: the raise's one argument, its message
                let raised = build::string_lit(DATE_RANGE);
                w.append("CASE WHEN ");
                let outside = self.outside_dates(unit, arg(a, 2)?, a1()?)?;
                w.piece(&*outside)?.append(" THEN ");
                match super::fact_type(t) {
                    None => self.error(w, std::slice::from_ref(&raised))?,
                    Some(slot) => {
                        w.append("CAST(");
                        self.error(w, std::slice::from_ref(&raised))?;
                        let ty = cast_type_name(self, slot)?;
                        w.append(&format!(" AS {ty})"));
                    }
                }
                w.append(&format!(" ELSE {name}(")).expr(self, arg(a, 2)?, 0)?.append(", ").expr(self, a1()?, 0)?;
                w.append(") END");
            }
            // DuckDB's date_diff counts boundaries crossed, as ClickHouse's dateDiff does
            SqlFn::DateDiff => {
                let unit = self.string_lit(diff_unit(part(a0()?)?)?)?;
                w.append("dateDiff(").append(&unit).append(", ");
                match a1()? {
                    SqlExpr::DateLit { iso, .. } if iso == "0001-01-07" => {
                        let lit = self.date_text("1900-01-07")?;
                        w.append(&lit);
                    }
                    other => {
                        w.expr(self, other, 0)?;
                    }
                }
                w.append(", ").expr(self, arg(a, 2)?, 0)?.append(")");
            }
            SqlFn::TimeBucket => {
                let bucket = self.time_bucket(a)?;
                w.piece(&*bucket)?;
            }
            SqlFn::DateAtTime => {
                return Err(Refusal::capability("a time of day: ClickHouse's Time type is experimental"));
            }
            SqlFn::Timezone => {
                w.append("addSeconds(toDateTime64(").expr(self, a1()?, 0)?;
                w.append(", 6, 'UTC'), timeZoneOffset(toTimeZone(toDateTime64(").expr(self, a1()?, 0)?;
                w.append(", 6, 'UTC'), ").expr(self, a0()?, 0)?.append(")))");
            }
            SqlFn::EpochSeconds => {
                w.append("(toFloat64(toUnixTimestamp64Micro(toDateTime64(").expr(self, a0()?, 0)?;
                w.append(", 6, 'UTC'))) / 1000000)");
            }
            SqlFn::EpochNs | SqlFn::FromEpochNs => {
                return Err(Refusal::capability(
                    "a nanosecond epoch: ClickHouse keeps microseconds (the Cypher island's nanosecond leg is \
                     DuckDB's)",
                ));
            }
            SqlFn::EpochMs => {
                w.append("toUnixTimestamp64Milli(toDateTime64(").expr(self, a0()?, 0)?.append(", 6, 'UTC'))");
            }
            SqlFn::FromEpochSeconds => {
                w.append("fromUnixTimestamp64Micro(toInt64(").expr(self, a0()?, 0)?.append(" * 1000000), 'UTC')");
            }
            SqlFn::FromEpochMs => {
                w.append("fromUnixTimestamp64Milli(toInt64(").expr(self, a0()?, 0)?.append("), 'UTC')");
            }
            SqlFn::Dayname => {
                w.append("dateName('weekday', ").expr(self, a0()?, 0)?.append(")");
            }
            SqlFn::Monthname => {
                w.append("dateName('month', ").expr(self, a0()?, 0)?.append(")");
            }
            SqlFn::Strftime => {
                let SqlExpr::FormatLit { parts, .. } = a1()? else {
                    return Err(wall(f, "a date format that is not a format literal: its codes are DuckDB's"));
                };
                let formatted = self.formatted(a0()?, parts)?;
                w.piece(&*formatted)?;
            }
            SqlFn::Strptime => {
                if !matches!(a1()?, SqlExpr::FormatLit { .. }) {
                    return Err(wall(f, "a date format that is not a format literal: its codes are DuckDB's"));
                }
                w.append("toDateTime64(parseDateTime(").expr(self, a0()?, 0)?.append(", ").expr(self, a1()?, 0)?;
                w.append(", 'UTC'), 6, 'UTC')");
            }
            other => return Err(Refusal::illegal_state(format!("not a temporal call: {other}"))),
        }
        Ok(())
    }

    /// A date formatted: formatDateTime's letters, Joda's for a format with an unpadded 12-hour clock; a millisecond
    /// field the first three digits of the microseconds (Java's `formatted`).
    pub(super) fn formatted<'a>(&'a self, value: &'a SqlExpr, parts: &[DateFmt]) -> Result<P<'a>, Refusal> {
        let mut pieces: Vec<P<'a>> = Vec::new();
        let mut run: Vec<DateFmt> = Vec::new();
        for p in parts {
            if matches!(p, DateFmt::Part(lite_sql::DateFmtPart::SubsecMin)) {
                if !run.is_empty() {
                    pieces.push(self.formatted_run(value, std::mem::take(&mut run))?);
                }
                pieces.push(pc(move |w| {
                    w.append("substring(formatDateTime(").expr(self, value, 0)?.append(", '%f'), 1, 3)");
                    Ok(())
                }));
            } else {
                run.push(p.clone());
            }
        }
        if !run.is_empty() {
            pieces.push(self.formatted_run(value, run)?);
        }
        if pieces.len() == 1 {
            return Ok(pieces.pop().expect("one piece"));
        }
        Ok(pc(move |w| {
            w.append("concat(").join(&pieces, ", ", |w, p| p(w))?.append(")");
            Ok(())
        }))
    }

    /// A run of fields with no millisecond one, its format read when it is written (Java's `formattedRun`).
    fn formatted_run<'a>(&'a self, value: &'a SqlExpr, parts: Vec<DateFmt>) -> Result<P<'a>, Refusal> {
        let joda = parts.contains(&DateFmt::Part(lite_sql::DateFmtPart::Hour12Nopad));
        Ok(pc(move |w| {
            w.append(if joda { "formatDateTimeInJodaSyntax(" } else { "formatDateTime(" }).expr(self, value, 0)?;
            let format =
                if joda { super::aggregates::joda_text(&parts)? } else { super::aggregates::format_text(&parts)? };
            let lit = self.string_lit(&format)?;
            w.append(", ").append(&lit).append(")");
            Ok(())
        }))
    }
}
