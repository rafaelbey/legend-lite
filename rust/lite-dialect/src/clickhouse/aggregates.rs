// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! ClickHouse's aggregates and windows (Java's `reducerIn` and its arms, the collected lists, the RANGE frames by epoch
//! milliseconds, at the type's edge and fractional, `windowCall`), the bit operators, the JSON constructors, the date
//! formats (`formatDateTime`'s letters and Joda's), the DDL types and the wire type names (ClickHouse.java, "Hooks of
//! the base" to its end).

use super::{ClickHouse, P, is_scalar, pc, type_of};
use crate::ansi;
use crate::build;
use crate::refusal::{Refusal, arg};
use crate::renderer::AnsiSqlRenderer;
use crate::writer::SqlWriter;
use lite_sql::{
    AggFn, Bound, ColumnKind, ColumnType, DateFmt, DateFmtPart, Frame, FrameKind, JavaNumber, JsonArrayAggKey,
    NullOrder, Reducer, Scalar, SortKey, SqlAgg, SqlExpr, SqlFn, SqlType,
};

/// Java's private `SortKey` record: a key piece, its direction, where its NULLs go, and its type.
pub(super) struct Key<'a> {
    pub key: P<'a>,
    pub ascending: bool,
    pub nulls_first: bool,
    pub r#type: Option<&'a SqlType>,
}

/// The window an aggregate is written over (Java's `SqlExpr.WindowCall` handed to `reducerIn`): its partition, order
/// and frame, as the window call rewrote them.
#[derive(Clone, Copy)]
pub(super) struct Win<'a> {
    partition_by: &'a [SqlExpr],
    order_by: &'a [SortKey],
    frame: Option<&'a Frame>,
}

/// A key in descending order as a key arraySort orders ascending (Java's `descending`).
fn descending(key: &str, t: Option<&SqlType>) -> Result<String, Refusal> {
    Ok(match t {
        Some(SqlType::Scalar(Scalar::Bigint | Scalar::Integer | Scalar::Double | Scalar::Hugeint))
        | Some(SqlType::Decimal { .. }) => format!("-{key}"),
        Some(SqlType::Scalar(Scalar::Timestamp | Scalar::Timestamptz)) => {
            format!("-toUnixTimestamp64Micro(toDateTime64({key}, 6, 'UTC'))")
        }
        Some(SqlType::Scalar(Scalar::Date)) => format!("-toInt32({key})"),
        Some(SqlType::Scalar(Scalar::Boolean)) => format!("-toInt8({key})"),
        Some(SqlType::Scalar(Scalar::Varchar)) => format!(
            "arrayPushBack(arrayMap(\"__i\" -> -toInt16(reinterpretAsUInt8(substring(ifNull({key}, ''), \"__i\", 1))), \
             range(1, length(ifNull({key}, '')) + 1)), 1)"
        ),
        None => return Err(Refusal::capability("a descending sort key of no type reached ClickHouse")),
        Some(t) => {
            return Err(Refusal::capability(format!(
                "a descending sort key of {} reached ClickHouse",
                crate::java::sql_type_text(t)
            )));
        }
    })
}

/// `Math.multiplyExact`'s overflow: Java's `ArithmeticException`.
fn multiply_exact(a: i64, b: i64) -> Result<i64, Refusal> {
    a.checked_mul(b).ok_or_else(|| Refusal { class: "java.lang.ArithmeticException", message: "long overflow".to_owned() })
}

/// A unit's fixed length in milliseconds (Java's `unitMillis`; `UNIT_MILLIS`, a plural unit its singular).
fn unit_millis(unit: &str) -> Result<i64, Refusal> {
    let u = unit.to_uppercase();
    let key = u.strip_suffix('S').unwrap_or(&u);
    Ok(match key {
        "WEEK" => 604_800_000,
        "DAY" => 86_400_000,
        "HOUR" => 3_600_000,
        "MINUTE" => 60_000,
        "SECOND" => 1000,
        "MILLISECOND" => 1,
        _ => {
            return Err(Refusal::capability(format!(
                "a {unit} RANGE frame offset reached ClickHouse: no fixed length in the key's milliseconds"
            )));
        }
    })
}

/// An interval bound as its milliseconds (Java's `millis`): a `Long` offset.
fn millis(b: &Bound) -> Result<Bound, Refusal> {
    Ok(match b {
        Bound::IntervalPreceding { n, unit } => Bound::Preceding { n: JavaNumber::Long(multiply_exact(*n, unit_millis(unit)?)?) },
        Bound::IntervalFollowing { n, unit } => Bound::Following { n: JavaNumber::Long(multiply_exact(*n, unit_millis(unit)?)?) },
        other => other.clone(),
    })
}

fn is_interval(b: &Bound) -> bool {
    matches!(b, Bound::IntervalPreceding { .. } | Bound::IntervalFollowing { .. })
}

fn is_offset(b: &Bound) -> bool {
    matches!(b, Bound::Preceding { .. } | Bound::Following { .. })
}

/// A Java `Number`'s `toString`.
fn number_text(n: &JavaNumber) -> String {
    match n {
        JavaNumber::Long(v) => v.to_string(),
        JavaNumber::Integer(v) => v.to_string(),
        JavaNumber::Double(v) => lite_javacompat::double_to_string(v.value()),
        JavaNumber::BigDecimal(v) => v.as_str().to_owned(),
    }
}

/// `new BigDecimal(text)`, refused as Java's constructor refuses a NaN's or an infinity's text.
fn decimal_text(text: &str) -> Result<&str, Refusal> {
    match text.chars().find(|c| !(c.is_ascii_digit() || matches!(c, '-' | '+' | '.' | 'E' | 'e'))) {
        Some(c) => Err(Refusal {
            class: Refusal::NUMBER_FORMAT,
            message: format!(
                "Character {c} is neither a decimal digit number, decimal point, nor \"e\" notation exponential mark."
            ),
        }),
        None => Ok(text),
    }
}

/// An offset bound's number, as `new BigDecimal(n.toString())` reads it.
fn offset_number(b: &Bound) -> Option<&JavaNumber> {
    match b {
        Bound::Preceding { n } | Bound::Following { n } => Some(n),
        _ => None,
    }
}

/// Whether a bound's offset has a fraction (Java's `fractional`).
fn fractional(b: &Bound) -> Result<bool, Refusal> {
    Ok(match offset_number(b) {
        Some(n) => super::stripped_scale(decimal_text(&number_text(n))?) > 0,
        None => false,
    })
}

/// An aggregate's record `toString`, which a refusal embeds: exact for a ranking or value function of no argument (the
/// argument's record text is written by its simple name, `crate::java::expr_text`).
fn agg_text(f: &SqlAgg) -> String {
    let args = |a: &[SqlExpr]| a.iter().map(crate::java::expr_text).collect::<Vec<_>>().join(", ");
    match f {
        SqlAgg::RankingFn { r#fn, args: a } => format!("RankingFn[fn={fn}, args=[{}]]", args(a)),
        SqlAgg::ValueFn { r#fn, args: a } => format!("ValueFn[fn={fn}, args=[{}]]", args(a)),
        SqlAgg::Reducer(r) => format!(
            "Reducer[fn={}, args=[{}], distinct={}, orderBy=[...], type={}]",
            r.r#fn,
            args(&r.args),
            r.distinct,
            crate::java::type_fact_text(&r.r#type)
        ),
    }
}

/// A window's order key with NULL as its type's extreme on the NULLs' side (Java's `nullsAtEdge`).
fn nulls_at_edge(order_by: &[SortKey]) -> Result<Vec<SortKey>, Refusal> {
    if order_by.len() != 1 {
        // ClickHouse refuses a RANGE frame of offsets over several keys itself
        return Ok(order_by.to_vec());
    }
    let k = &order_by[0];
    let high = k.ascending == (k.null_order != Some(NullOrder::NullsFirst));
    let edge = match type_of(&k.expr) {
        Some(SqlType::Scalar(Scalar::Bigint | Scalar::Integer)) => {
            build::int_lit(if high { i64::MAX } else { i64::MIN })
        }
        // an infinity is no Float literal: the text cast
        Some(SqlType::Scalar(Scalar::Double)) => {
            build::cast(build::string_lit(if high { "inf" } else { "-inf" }), SqlType::Scalar(Scalar::Double))
        }
        Some(t) => {
            return Err(Refusal::capability(format!(
                "a RANGE frame of offsets over a {} key reached ClickHouse, whose frame reaches a NULL key from any \
                 value",
                crate::java::sql_type_text(t)
            )));
        }
        None => return Err(Refusal::capability("a RANGE frame of offsets over an untyped key reached ClickHouse")),
    };
    let keyed = build::sort_key(
        build::call(SqlFn::Coalesce, vec![k.expr.clone(), edge]),
        k.ascending,
        k.null_order,
        k.output_name.clone(),
    );
    Ok(vec![keyed])
}

impl ClickHouse {
    pub(super) fn bit_op_arm(&self, w: &mut SqlWriter, f: SqlFn, a: &[SqlExpr]) -> Result<(), Refusal> {
        let name = match f {
            SqlFn::BitAnd => "bitAnd",
            SqlFn::BitOr => "bitOr",
            SqlFn::BitXor => "bitXor",
            SqlFn::BitShiftLeft => "bitShiftLeft",
            SqlFn::BitShiftRight => "bitShiftRight",
            other => return Err(Refusal::illegal_state(format!("not a bit op: {other}"))),
        };
        w.append(&format!("{name}(toInt64(")).expr(self, arg(a, 0)?, 0)?.append("), ").expr(self, arg(a, 1)?, 0)?;
        w.append(")");
        Ok(())
    }

    // ==================================================================
    // Aggregates and windows
    // ==================================================================

    /// The values collected into a list in the keys' order, NULL values kept: each row's value in a tuple, sorted by
    /// the keys, then read back (Java's `collected`).
    pub(super) fn collected<'a>(&'a self, value: P<'a>, keys: Vec<Key<'a>>, win: Option<Win<'a>>) -> P<'a> {
        pc(move |w| {
            w.append("arrayMap(\"__s\" -> tupleElement(\"__s\", 1), ");
            if !keys.is_empty() {
                w.append("arraySort(\"__s\" -> (");
                for (i, k) in keys.iter().enumerate() {
                    let key = format!("tupleElement(\"__s\", {})", i + 2);
                    // NULLs where the key says: isNull sorts false (0) before true (1)
                    w.append(if i > 0 { ", " } else { "" });
                    w.append(&if k.nulls_first { format!("isNotNull({key}), ") } else { format!("isNull({key}), ") });
                    let text = if k.ascending { key } else { descending(&key, k.r#type)? };
                    w.append(&text);
                }
                w.append("), ");
            }
            w.append("groupArray(tuple(").piece(&*value)?;
            for k in &keys {
                w.append(", ").piece(&*k.key)?;
            }
            w.append("))");
            self.ov(w, win)?;
            if !keys.is_empty() {
                w.append(")");
            }
            w.append(")");
            Ok(())
        })
    }

    /// A select's sort key as a collected list's (Java's `sortKeyOf`).
    fn sort_key_of<'a>(&'a self, k: &'a SortKey) -> Key<'a> {
        let nulls_first = match k.null_order {
            Some(order) => order == NullOrder::NullsFirst,
            None => !k.ascending,
        };
        Key { key: self.ex(&k.expr, 0), ascending: k.ascending, nulls_first, r#type: type_of(&k.expr) }
    }

    /// `win`'s OVER clause after an aggregate call, when the reducer is windowed (Java's `ov`).
    fn ov(&self, w: &mut SqlWriter, win: Option<Win<'_>>) -> Result<(), Refusal> {
        match win {
            None => Ok(()),
            Some(win) => ansi::over(self, w, win.partition_by, win.order_by, win.frame),
        }
    }

    /// A reducer, windowed over `win` when it is one (Java's `reducerIn`).
    pub(super) fn reducer_in(&self, w: &mut SqlWriter, r: &Reducer, win: Option<Win<'_>>) -> Result<(), Refusal> {
        match r.r#fn {
            AggFn::Count => {
                w.append("toInt64(");
                ansi::aggregate(self, w, "count", r)?;
                self.ov(w, win)?;
                w.append(")");
            }
            // corr takes no Decimal beside an Int64: Float64 operands
            AggFn::Corr => {
                w.append("corrOrNull(toFloat64(").expr(self, arg(&r.args, 0)?, 0)?.append("), toFloat64(");
                w.expr(self, arg(&r.args, 1)?, 0)?.append("))");
                self.ov(w, win)?;
            }
            // an aggregate over no row is its OrNull form's NULL
            AggFn::Sum | AggFn::Avg | AggFn::Min | AggFn::Max => {
                let name = match r.r#fn {
                    AggFn::Sum => "sumOrNull",
                    AggFn::Avg => "avgOrNull",
                    AggFn::Min => "minOrNull",
                    _ => "maxOrNull",
                };
                if !r.order_by.is_empty() {
                    return Err(Refusal::capability(format!("an ordered {} reached ClickHouse", r.r#fn)));
                }
                ansi::aggregate(self, w, name, r)?;
                self.ov(w, win)?;
            }
            // one value's sample deviation and variance are inf here, DuckDB's NULL
            AggFn::StddevSamp | AggFn::Stddev => self.sampled(w, "stddevSampStableOrNull", r, win)?,
            AggFn::StddevPop => self.plain_ov(w, "stddevPopStableOrNull", r, win)?,
            AggFn::VarSamp | AggFn::Variance => self.sampled(w, "varSampStableOrNull", r, win)?,
            AggFn::VarPop => self.plain_ov(w, "varPopStableOrNull", r, win)?,
            // one row's sample covariance is NaN here, DuckDB's NULL
            AggFn::CovarSamp => {
                w.append("if(count(").expr(self, arg(&r.args, 0)?, 0)?.append(")");
                self.ov(w, win)?;
                w.append(" < 2, NULL, ");
                self.plain_ov(w, "covarSampStableOrNull", r, win)?;
                w.append(")");
            }
            AggFn::CovarPop => self.plain_ov(w, "covarPopStableOrNull", r, win)?,
            AggFn::BoolAnd => self.plain_ov(w, "minOrNull", r, win)?,
            AggFn::BoolOr => self.plain_ov(w, "maxOrNull", r, win)?,
            AggFn::AnyValue => self.plain_ov(w, "anyOrNull", r, win)?,
            AggFn::StringAgg => self.string_agg(w, r, win)?,
            AggFn::Median => {
                w.append("quantileExactInclusiveOrNull(0.5)(toFloat64(").expr(self, arg(&r.args, 0)?, 0)?;
                w.append("))");
                self.ov(w, win)?;
            }
            AggFn::QuantileCont => {
                let desc = descending_percentile(r)?;
                w.append("quantileExactInclusiveOrNull(");
                if desc {
                    w.append("1 - ");
                }
                w.expr(self, &r.args[1], 6)?.append(")(toFloat64(").expr(self, &r.args[0], 0)?.append("))");
                self.ov(w, win)?;
            }
            AggFn::QuantileDisc => self.quantile_disc(w, r, win)?,
            AggFn::Mode => {
                let values: P<'_> = pc(move |w| {
                    w.append("groupArray(").expr(self, arg(&r.args, 0)?, 0)?.append(")");
                    self.ov(w, win)
                });
                w.piece(&*ClickHouse::mode_of(values))?;
            }
            AggFn::List => {
                if r.distinct || r.args.len() != 1 {
                    return Err(Refusal::capability("a distinct LIST aggregate reached ClickHouse"));
                }
                let keys = r.order_by.iter().map(|k| self.sort_key_of(k)).collect();
                w.piece(&*self.collected(self.ex(&r.args[0], 0), keys, win))?;
            }
            AggFn::ArgMax | AggFn::ArgMin => {
                if r.args.len() != 2 || r.distinct || !r.order_by.is_empty() {
                    return Err(Refusal::capability(format!("{} of this shape reached ClickHouse", r.r#fn)));
                }
                let name = if r.r#fn == AggFn::ArgMax { "argMax" } else { "argMin" };
                w.append(name).append("(").list(self, &r.args)?.append(")");
                self.ov(w, win)?;
            }
            AggFn::Wavg | AggFn::HashList | AggFn::IsDistinctMark | AggFn::UniqueValueOnly => {
                return Err(ansi::lowering_marker(r));
            }
            // a window function is a ranking or value function, never an aggregate
            AggFn::RowNumber
            | AggFn::Rank
            | AggFn::DenseRank
            | AggFn::PercentRank
            | AggFn::CumeDist
            | AggFn::Ntile
            | AggFn::Lag
            | AggFn::Lead
            | AggFn::FirstValue
            | AggFn::LastValue
            | AggFn::NthValue => return Err(ansi::window_function(r)),
        }
        Ok(())
    }

    /// `fn(...)` with its OVER clause (Java's `ov(plain(writer, fn, r), win)`).
    fn plain_ov(&self, w: &mut SqlWriter, name: &str, r: &Reducer, win: Option<Win<'_>>) -> Result<(), Refusal> {
        self.plain(w, name, r)?;
        self.ov(w, win)
    }

    fn plain(&self, w: &mut SqlWriter, name: &str, r: &Reducer) -> Result<(), Refusal> {
        if !r.order_by.is_empty() {
            return Err(Refusal::capability(format!("an ordered {} reached ClickHouse", r.r#fn)));
        }
        w.append(name).append("(").append(if r.distinct { "DISTINCT " } else { "" }).list(self, &r.args)?.append(")");
        Ok(())
    }

    /// A sample statistic, NULL under two values (Java's `sampled`).
    fn sampled(&self, w: &mut SqlWriter, name: &str, r: &Reducer, win: Option<Win<'_>>) -> Result<(), Refusal> {
        w.append("if(count(").append(if r.distinct { "DISTINCT " } else { "" });
        w.expr(self, arg(&r.args, 0)?, 0)?.append(")");
        self.ov(w, win)?;
        w.append(" < 2, NULL, ");
        self.plain_ov(w, name, r, win)?;
        w.append(")");
        Ok(())
    }

    /// The `STRING_AGG` arm of `reducerIn` (Java's `stringAgg`).
    fn string_agg(&self, w: &mut SqlWriter, r: &Reducer, win: Option<Win<'_>>) -> Result<(), Refusal> {
        if r.args.len() != 2 {
            return Err(Refusal::capability(format!("a STRING_AGG of {} arguments reached ClickHouse", r.args.len())));
        }
        let v = &r.args[0];
        let values: P<'_> = if r.distinct {
            pc(move |w| {
                w.append("arrayDistinct(groupArray(").expr(self, v, 0)?.append(")");
                self.ov(w, win)?;
                w.append(")");
                Ok(())
            })
        } else if r.order_by.is_empty() {
            pc(move |w| {
                w.append("groupArray(").expr(self, v, 0)?.append(")");
                self.ov(w, win)
            })
        } else {
            let keys = r.order_by.iter().map(|k| self.sort_key_of(k)).collect();
            let collected = self.collected(self.ex(v, 0), keys, win);
            pc(move |w| {
                w.append("arrayFilter(\"__x\" -> isNotNull(\"__x\"), ").piece(&*collected)?.append(")");
                Ok(())
            })
        };
        w.append("if(empty(").piece(&*values)?.append("), NULL, arrayStringConcat(").piece(&*values)?.append(", ");
        w.expr(self, &r.args[1], 0)?.append("))");
        Ok(())
    }

    /// The `QUANTILE_DISC` arm: the sorted values' element at max(ceil(n q) - 1, 0), 0-based (Java's `quantileDisc`).
    fn quantile_disc(&self, w: &mut SqlWriter, r: &Reducer, win: Option<Win<'_>>) -> Result<(), Refusal> {
        let desc = descending_percentile(r)?;
        let sorted = |w: &mut SqlWriter| -> Result<(), Refusal> {
            w.append("arraySort(groupArray(").expr(self, &r.args[0], 0)?.append(")");
            self.ov(w, win)?;
            w.append(")");
            Ok(())
        };
        let q = |w: &mut SqlWriter| -> Result<(), Refusal> {
            if desc {
                w.append("(1 - ").expr(self, &r.args[1], 6)?.append(")");
            } else {
                w.expr(self, &r.args[1], 6)?;
            }
            Ok(())
        };
        w.append("if(empty(").piece(&sorted)?.append("), NULL, arrayElement(").piece(&sorted)?;
        w.append(", toInt64(greatest(ceil(length(").piece(&sorted)?.append(") * ").piece(&q)?;
        w.append(") - 1, 0)) + 1))");
        Ok(())
    }

    /// A RANGE frame with a fractional offset, computed over its partition's (value, key) pairs (Java's
    /// `fractionalRange`).
    fn fractional_range(
        &self,
        w: &mut SqlWriter,
        f: &SqlAgg,
        partition_by: &[SqlExpr],
        order_by: &[SortKey],
        frame: &Frame,
    ) -> Result<(), Refusal> {
        let refuse = |what: String| {
            Refusal::capability(format!(
                "a RANGE frame with a fractional offset over {what} reached ClickHouse, whose RANGE offsets are \
                 integers"
            ))
        };
        let r = match f {
            SqlAgg::Reducer(r) if !r.distinct && r.order_by.is_empty() && r.args.len() == 1 && order_by.len() == 1 => r,
            other => return Err(refuse(agg_text(other))),
        };
        let k = &order_by[0];
        let key = &k.expr;
        // the key's side of each bound: ascending, a PRECEDING bound is below the row's key; descending, above it
        let bound = |b: &Bound, start: bool| -> Result<Option<(&'static str, String)>, Refusal> {
            let (sign, n) = match b {
                Bound::Preceding { n } => (if k.ascending { " - " } else { " + " }, Some(n)),
                Bound::Following { n } => (if k.ascending { " + " } else { " - " }, Some(n)),
                Bound::CurrentRow => ("", None),
                // unbounded: no condition
                _ => return Ok(None),
            };
            let op = if start == k.ascending { " >= " } else { " <= " };
            let offset = match n {
                None => String::new(),
                Some(n) => format!("{sign}{}", crate::java::big_decimal_plain(decimal_text(&number_text(n))?)),
            };
            Ok(Some((op, offset)))
        };
        let mut conds = Vec::new();
        if let Some(c) = bound(&frame.from, true)? {
            conds.push(c);
        }
        if let Some(c) = bound(&frame.to, false)? {
            conds.push(c);
        }
        let agg = match r.r#fn {
            AggFn::Sum => "sumOrNull",
            AggFn::Avg => "avgOrNull",
            AggFn::Min => "minOrNull",
            AggFn::Max => "maxOrNull",
            AggFn::Count => "count",
            AggFn::StddevSamp | AggFn::Stddev => "stddevSampStableOrNull",
            AggFn::StddevPop => "stddevPopStableOrNull",
            AggFn::VarSamp | AggFn::Variance => "varSampStableOrNull",
            AggFn::VarPop => "varPopStableOrNull",
            AggFn::Median => "quantileExactInclusiveOrNull(0.5)",
            AggFn::BoolAnd => "minOrNull",
            AggFn::BoolOr => "maxOrNull",
            AggFn::AnyValue => "anyOrNull",
            other => return Err(refuse(other.to_string())),
        };
        // the frame's values: the partition's pairs within the bounds, their values
        let values = |x: &mut SqlWriter| -> Result<(), Refusal> {
            x.append("arrayMap(__p -> ").append(if r.r#fn == AggFn::Median { "toFloat64(__p.1)" } else { "__p.1" });
            x.append(", ");
            if !conds.is_empty() {
                x.append("arrayFilter(__p -> ");
                for (i, (op, offset)) in conds.iter().enumerate() {
                    x.append(if i > 0 { " AND " } else { "" }).append("__p.2").append(op).expr(self, key, 0)?;
                    x.append(offset);
                }
                x.append(", ");
            }
            x.append("groupArray(tuple(").expr(self, &r.args[0], 0)?.append(", ").expr(self, key, 0)?;
            x.append(")) OVER (");
            if !partition_by.is_empty() {
                x.append("PARTITION BY ").list(self, partition_by)?;
            }
            x.append(")").append(if conds.is_empty() { ")" } else { "))" });
            Ok(())
        };
        let sample = matches!(r.r#fn, AggFn::StddevSamp | AggFn::Stddev | AggFn::VarSamp | AggFn::Variance);
        if sample {
            // under two values NULL, as the native sample statistic
            w.append("if(arrayReduce('count', ").piece(&values)?.append(") < 2, NULL, ");
        }
        let count = r.r#fn == AggFn::Count;
        w.append(if count { "toInt64(" } else { "" }).append(&format!("arrayReduce('{agg}', ")).piece(&values)?;
        w.append(")").append(if count { ")" } else { "" });
        if sample {
            w.append(")");
        }
        Ok(())
    }

    /// `lag` and `lead` over the whole partition, `cume_dist` as counts, a count or a rank an Int64, an average a
    /// DOUBLE; a RANGE frame of intervals by epoch milliseconds, of offsets with NULL at the key's edge (Java's
    /// `windowCall`).
    pub(super) fn window_call_arm(
        &self,
        w: &mut SqlWriter,
        f: &SqlAgg,
        partition_by: &[SqlExpr],
        order_by: &[SortKey],
        frame: Option<&Frame>,
    ) -> Result<(), Refusal> {
        let mut keys: Vec<SortKey> = order_by.to_vec();
        let mut frame: Option<Frame> = frame.cloned();
        if let Some(fr) = &frame
            && fr.kind == FrameKind::Range
            && (is_interval(&fr.from) || is_interval(&fr.to))
        {
            // epochFramed: the window ordered by its key's epoch milliseconds, each offset its milliseconds
            if keys.len() != 1 {
                return Err(Refusal::capability(format!(
                    "an interval RANGE frame over {} order keys reached ClickHouse",
                    keys.len()
                )));
            }
            let k = &keys[0];
            let epoch = build::sort_key(
                build::call(SqlFn::EpochMs, vec![k.expr.clone()]),
                k.ascending,
                k.null_order,
                k.output_name.clone(),
            );
            let from = millis(&fr.from)?;
            let to = millis(&fr.to)?;
            keys = vec![epoch];
            frame = Some(Frame { kind: FrameKind::Range, from, to });
        }
        if let Some(fr) = &frame
            && fr.kind == FrameKind::Range
            && (is_offset(&fr.from) || is_offset(&fr.to))
        {
            keys = nulls_at_edge(&keys)?;
            if fractional(&fr.from)? || fractional(&fr.to)? {
                return self.fractional_range(w, f, partition_by, &keys, fr);
            }
        }
        let win = Win { partition_by, order_by: &keys, frame: frame.as_ref() };
        if let SqlAgg::ValueFn { r#fn: v @ (AggFn::Lag | AggFn::Lead), args } = f {
            w.append(if *v == AggFn::Lag { "lagInFrame(toNullable(" } else { "leadInFrame(toNullable(" });
            w.expr(self, arg(args, 0)?, 0)?.append(")");
            for e in &args[1..] {
                w.append(", ").expr(self, e, 0)?;
            }
            w.append(") OVER (");
            if !partition_by.is_empty() {
                w.append("PARTITION BY ").list(self, partition_by)?;
            }
            if !keys.is_empty() {
                w.append(if partition_by.is_empty() { "" } else { " " }).append("ORDER BY ");
                w.join(&keys, ", ", |w, k| self.sort_key(w, k))?;
            }
            w.append(" ROWS BETWEEN UNBOUNDED PRECEDING AND UNBOUNDED FOLLOWING)");
            return Ok(());
        }
        // cume_dist: the rows up to the current one's value over the partition's rows
        if let SqlAgg::RankingFn { r#fn: AggFn::CumeDist, .. } = f {
            w.append("(toFloat64(count() OVER (");
            if !partition_by.is_empty() {
                w.append("PARTITION BY ").list(self, partition_by)?.append(" ");
            }
            w.append("ORDER BY ").join(&keys, ", ", |w, k| self.sort_key(w, k))?;
            w.append(" RANGE BETWEEN UNBOUNDED PRECEDING AND CURRENT ROW)) / count() OVER (");
            if !partition_by.is_empty() {
                w.append("PARTITION BY ").list(self, partition_by)?;
            }
            w.append("))");
            return Ok(());
        }
        let cast = matches!(f, SqlAgg::Reducer(r) if r.r#fn == AggFn::Avg);
        // a count or a rank is unsigned here: an Int64
        let signed = matches!(f, SqlAgg::RankingFn { r#fn, .. } if *r#fn != AggFn::PercentRank)
            || matches!(f, SqlAgg::Reducer(r) if r.r#fn == AggFn::Count);
        if cast {
            w.append("toFloat64(");
        }
        if signed {
            w.append("toInt64(");
        }
        match f {
            SqlAgg::Reducer(r) => self.reducer_in(w, r, Some(win))?,
            // ntile's bucket count is unsigned
            SqlAgg::RankingFn { r#fn: AggFn::Ntile, args } => {
                w.append("ntile(toUInt64(").list(self, args)?.append("))");
            }
            SqlAgg::RankingFn { r#fn, args } => {
                let name = self.aggregate_name(*r#fn);
                w.append(&name).append("(").list(self, args)?.append(")");
            }
            // a value function reads the nullable value: past the frame it is NULL
            SqlAgg::ValueFn { r#fn, args } => {
                let name = self.aggregate_name(*r#fn);
                w.append(&name).append("(toNullable(").expr(self, arg(args, 0)?, 0)?.append(")");
                for e in &args[1..] {
                    w.append(", ").expr(self, e, 0)?;
                }
                w.append(")");
            }
        }
        if !matches!(f, SqlAgg::Reducer(_)) {
            ansi::over(self, w, partition_by, &keys, frame.as_ref())?;
        }
        if signed {
            w.append(")");
        }
        if cast {
            w.append(")");
        }
        Ok(())
    }

    // ==================================================================
    // JSON constructors
    // ==================================================================

    /// A value's JSON text: a JSON value its own text, any other toJSONString's (Java's `json`).
    fn json<'a>(&'a self, x: &'a SqlExpr) -> P<'a> {
        if let SqlExpr::NullLit { .. } = x {
            return super::text("'null'");
        }
        if is_scalar(x, Scalar::Json) {
            return pc(move |w| {
                w.append("coalesce(").expr(self, x, 0)?.append(", 'null')");
                Ok(())
            });
        }
        pc(move |w| {
            w.append("coalesce(toJSONString(").expr(self, x, 0)?.append("), 'null')");
            Ok(())
        })
    }

    pub(super) fn json_object_arm(&self, w: &mut SqlWriter, kv: &[SqlExpr]) -> Result<(), Refusal> {
        w.append("concat('{'");
        let mut i = 0;
        while i < kv.len() {
            w.append(if i > 0 { ", ','" } else { "" }).append(", ").piece(&*self.json(&kv[i]))?.append(", ':', ");
            w.piece(&*self.json(arg(kv, i + 1)?))?;
            i += 2;
        }
        w.append(", '}')");
        Ok(())
    }

    pub(super) fn json_array_arm(&self, w: &mut SqlWriter, elements: &[SqlExpr]) -> Result<(), Refusal> {
        w.append("concat('['");
        for (i, e) in elements.iter().enumerate() {
            w.append(if i > 0 { ", ','" } else { "" }).append(", ").piece(&*self.json(e))?;
        }
        w.append(", ']')");
        Ok(())
    }

    /// The group's values' JSON as an array, in the keys' order.
    pub(super) fn json_array_agg_arm(
        &self,
        w: &mut SqlWriter,
        value: &SqlExpr,
        order_keys: &[JsonArrayAggKey],
    ) -> Result<(), Refusal> {
        let values: P<'_> = if order_keys.is_empty() {
            let json = self.json(value);
            pc(move |w| {
                w.append("groupArray(").piece(&*json)?.append(")");
                Ok(())
            })
        } else {
            let keys = order_keys
                .iter()
                .map(|k| Key { key: self.ex(&k.expr, 0), ascending: !k.desc, nulls_first: false, r#type: type_of(&k.expr) })
                .collect();
            self.collected(self.json(value), keys, None)
        };
        w.append("concat('[', arrayStringConcat(").piece(&*values)?.append(", ','), ']')");
        Ok(())
    }
}

/// A percentile's shape here: a value and a level, at most one order key (its direction) (Java's
/// `descendingPercentile`).
fn descending_percentile(r: &Reducer) -> Result<bool, Refusal> {
    if r.args.len() != 2 || r.distinct || r.order_by.len() > 1 {
        return Err(Refusal::capability(format!("{} of this shape reached ClickHouse", r.r#fn)));
    }
    Ok(r.order_by.first().is_some_and(|k| !k.ascending))
}

// ==================================================================
// Date format codes (formatDateTime: MySQL's letters)
// ==================================================================

/// A date format in formatDateTime's letters (Java's `formatText`).
pub(super) fn format_text(parts: &[DateFmt]) -> Result<String, Refusal> {
    let mut out = String::new();
    for p in parts {
        match p {
            DateFmt::Text { s } => out.push_str(&s.replace('%', "%%")),
            DateFmt::Part(part) => out.push_str(match part {
                DateFmtPart::Year4 => "%Y",
                DateFmtPart::Month2 => "%m",
                DateFmtPart::Day2 => "%d",
                DateFmtPart::Hour2 => "%H",
                DateFmtPart::Min2 => "%i",
                DateFmtPart::Sec2 => "%S",
                DateFmtPart::SubsecMicro => "%f",
                DateFmtPart::SubsecNano => "%f000",
                DateFmtPart::SubsecMin => {
                    return Err(Refusal::capability(
                        "a millisecond format reached ClickHouse outside a formatted date (formatted())",
                    ));
                }
                DateFmtPart::MonthAbbrev => "%b",
                DateFmtPart::MonthName => "%M",
                DateFmtPart::WeekdayName => "%W",
                DateFmtPart::Hour12 => "%I",
                DateFmtPart::Hour12Nopad => "%l",
                DateFmtPart::Ampm => "%p",
            }),
        }
    }
    Ok(out)
}

/// A format in Joda's letters, its text quoted (Java's `jodaText`).
pub(super) fn joda_text(parts: &[DateFmt]) -> Result<String, Refusal> {
    let mut out = String::new();
    for p in parts {
        match p {
            DateFmt::Text { s } => out.push_str(&format!("'{}'", s.replace('\'', "''"))),
            DateFmt::Part(part) => out.push_str(match part {
                DateFmtPart::Year4 => "yyyy",
                DateFmtPart::Month2 => "MM",
                DateFmtPart::Day2 => "dd",
                DateFmtPart::Hour2 => "HH",
                DateFmtPart::Min2 => "mm",
                DateFmtPart::Sec2 => "ss",
                DateFmtPart::SubsecMicro => "SSSSSS",
                DateFmtPart::SubsecNano | DateFmtPart::SubsecMin => {
                    return Err(Refusal::illegal_state(format!(
                        "a {part} field reached a Joda format: formatted() writes it"
                    )));
                }
                DateFmtPart::MonthAbbrev => "MMM",
                DateFmtPart::MonthName => "MMMM",
                DateFmtPart::WeekdayName => "EEEE",
                DateFmtPart::Hour12 => "hh",
                DateFmtPart::Hour12Nopad => "h",
                DateFmtPart::Ampm => "a",
            }),
        }
    }
    Ok(out)
}

// ==================================================================
// DDL and the wire
// ==================================================================

/// ClickHouse DDL: `BIT` is a UInt64 there, so a Pure Boolean column is a `Bool` (Java's `ddlType`).
pub(super) fn ddl_type(t: &ColumnType) -> Result<String, Refusal> {
    Ok(match t {
        ColumnType::Plain { kind } => match kind {
            ColumnKind::Bigint => "BIGINT",
            ColumnKind::Smallint => "SMALLINT",
            ColumnKind::Tinyint => "TINYINT",
            ColumnKind::Integer => "INTEGER",
            ColumnKind::Float => "FLOAT",
            ColumnKind::Double => "DOUBLE",
            ColumnKind::Real => "REAL",
            ColumnKind::Bit => "Bool",
            ColumnKind::Timestamp => "TIMESTAMP",
            ColumnKind::TimestampNs => "DateTime64(9)",
            ColumnKind::Date => "DATE",
            ColumnKind::Json => "JSON",
            ColumnKind::Varchar => "VARCHAR",
            // a time of day: ClickHouse's Time type is experimental
            ColumnKind::Time | ColumnKind::Other | ColumnKind::Distinct | ColumnKind::Array | ColumnKind::Object => {
                return Err(Refusal::illegal_state(format!("no DDL spelling for declared column type {kind}")));
            }
        }
        .to_owned(),
        ColumnType::Sized { kind, size } => format!("{kind}({size})"),
        ColumnType::Scaled { kind, precision, scale } => format!("{kind}({precision}, {scale})"),
    })
}

/// Every scalar is Nullable(T); its own names for the same types (Java's `wireTypeName`).
pub(super) fn wire_type_name(driver_type_name: Option<&str>) -> String {
    let Some(name) = driver_type_name else {
        return crate::wire_names::fold(None);
    };
    let mut n = crate::java::trim(name);
    while n.starts_with("Nullable(") && n.ends_with(')') {
        n = crate::java::trim(&n["Nullable(".len()..n.len() - 1]);
    }
    if n.starts_with("Tuple(") {
        return crate::wire_names::fold(Some("STRUCT"));
    }
    if n.starts_with("DateTime") {
        return crate::wire_names::fold(Some("TIMESTAMP"));
    }
    if n.starts_with("Decimal") {
        return crate::wire_names::fold(Some("DECIMAL"));
    }
    crate::wire_names::fold(Some(match n {
        "Int64" | "UInt32" => "BIGINT",
        "Int32" | "Int16" | "Int8" | "UInt16" | "UInt8" => "INTEGER",
        "Int128" | "UInt64" => "HUGEINT",
        "Float64" => "DOUBLE",
        "Float32" => "FLOAT",
        "Date32" | "Date" => "DATE",
        "Nothing" => "NULL",
        other => other,
    }))
}

#[cfg(test)]
pub(super) fn descending_for_tests(key: &str, t: Option<&SqlType>) -> Result<String, Refusal> {
    descending(key, t)
}
