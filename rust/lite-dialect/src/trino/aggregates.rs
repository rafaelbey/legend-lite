// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! Trino's hooks of the base (the signed hash, banker's ROUND, the bit operators), its aggregates and windows -- every
//! `SqlAgg.Fn` decided by an exhaustive match -- the frame intervals and the date format (Trino.java, "Hooks of the
//! base" to "Date format codes").

use super::{P, Trino, is_scalar, pc, text, type_of};
use crate::ansi::{self, cast_type_name};
use crate::build;
use crate::refusal::{Refusal, arg};
use crate::renderer::AnsiSqlRenderer;
use crate::writer::SqlWriter;
use lite_sql::{
    AggFn, DateFmt, DateFmtPart, Frame, NullOrder, Reducer, Scalar, SortKey, SqlAgg, SqlExpr, SqlFn, SqlType, TypeFact,
};

/// A window's partition, order and frame: what `over` writes (Java's `SqlExpr.WindowCall` less its function).
#[derive(Clone, Copy)]
struct Window<'a> {
    partition_by: &'a [SqlExpr],
    order_by: &'a [SortKey],
    frame: Option<&'a Frame>,
}

/// A frame offset as a Trino interval literal (Java's `interval`): a week as its days; refused for a unit Trino has
/// no interval of. `Math.multiplyExact`'s overflow is Java's `ArithmeticException`.
pub(super) fn interval(n: i64, unit: &str) -> Result<String, Refusal> {
    let u = unit.to_uppercase();
    let single = u.strip_suffix('S').unwrap_or(&u);
    match single {
        "WEEK" => match n.checked_mul(7) {
            Some(days) => Ok(format!("INTERVAL '{days}' DAY")),
            None => Err(Refusal { class: "java.lang.ArithmeticException", message: "long overflow".to_owned() }),
        },
        "DAY" | "HOUR" | "MINUTE" | "SECOND" | "YEAR" | "MONTH" => Ok(format!("INTERVAL '{n}' {single}")),
        _ => Err(Refusal::capability(format!("a {unit} window frame offset has no Trino interval"))),
    }
}

/// The date format in Joda's letters (`format_datetime`, `parse_datetime`; Java's `formatText`).
pub(super) fn format_text(parts: &[DateFmt]) -> String {
    let mut out = String::new();
    for p in parts {
        match p {
            DateFmt::Text { s } => {
                if !s.is_empty() {
                    out.push('\'');
                    out.push_str(&s.replace('\'', "''"));
                    out.push('\'');
                }
            }
            DateFmt::Part(part) => out.push_str(match part {
                DateFmtPart::Year4 => "yyyy",
                DateFmtPart::Month2 => "MM",
                DateFmtPart::Day2 => "dd",
                DateFmtPart::Hour2 => "HH",
                DateFmtPart::Min2 => "mm",
                DateFmtPart::Sec2 => "ss",
                DateFmtPart::SubsecMicro => "SSSSSS",
                DateFmtPart::SubsecNano => "SSSSSS'000'",
                DateFmtPart::SubsecMin => "SSS",
                DateFmtPart::MonthAbbrev => "MMM",
                DateFmtPart::MonthName => "MMMM",
                DateFmtPart::WeekdayName => "EEEE",
                DateFmtPart::Hour12 => "hh",
                DateFmtPart::Hour12Nopad => "h",
                DateFmtPart::Ampm => "a",
            }),
        }
    }
    out
}

/// `writer`, for an aggregate Trino cannot order: an ordered one is refused (Java's `unordered`).
fn unordered(r: &Reducer) -> Result<(), Refusal> {
    if !r.order_by.is_empty() {
        return Err(Refusal::capability(format!("an ordered {} reached Trino", r.r#fn)));
    }
    Ok(())
}

impl Trino {
    fn window_over(&self, w: &mut SqlWriter, window: Option<Window<'_>>) -> Result<(), Refusal> {
        if let Some(win) = window {
            ansi::over(self, w, win.partition_by, win.order_by, win.frame)?;
        }
        Ok(())
    }

    /// A value's text for the hash: a list's or a row's text is its JSON (Java's `hashText`).
    fn hash_text(&self, w: &mut SqlWriter, e: &SqlExpr) -> Result<(), Refusal> {
        if matches!(type_of(e), Some(SqlType::Array { .. } | SqlType::Struct { .. } | SqlType::Map { .. })) {
            w.append("json_format(CAST(").expr(self, e, 0)?.append(" AS JSON))");
            return Ok(());
        }
        self.as_text(w, e)
    }

    /// Pure's hashCode is a signed 64-bit integer: xxhash64 of the value's text, read as one; no value hashes the
    /// empty text (Java's `hashSigned`).
    pub(super) fn hash_signed_arm(&self, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
        w.append("from_big_endian_64(xxhash64(to_utf8(coalesce(");
        if a.len() == 1 {
            self.hash_text(w, &a[0])?;
        } else {
            w.append("concat_ws(chr(31), ").join(a, ", ", |w, e| self.hash_text(w, e))?.append(")");
        }
        w.append(", ''))))");
        Ok(())
    }

    /// Pure round is half-EVEN; Trino's round is half away from zero (doubles included): an exact .5 of the scaled
    /// value steps to its even neighbour (Java's `roundHalfEven`).
    pub(super) fn round_half_even_arm(&self, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
        let scale: P<'_> = if a.len() > 1 {
            pc(move |w| {
                w.append("CAST(").expr(self, &a[1], 0)?.append(" AS INTEGER)");
                Ok(())
            })
        } else {
            text("0")
        };
        let x = arg(a, 0)?;
        let exact = !(is_scalar(x, Scalar::Double) || matches!(x, SqlExpr::FloatLit { .. }));
        let ten = if exact { "CAST(10 AS DECIMAL(38, 0))" } else { "CAST(10 AS DOUBLE)" };
        let p: P<'_> = pc(move |w| {
            w.append(&format!("power({ten}, ")).piece(&*scale)?.append(")");
            Ok(())
        });
        if a.len() == 1 {
            w.append("(CASE WHEN ").expr(self, x, 6)?.append(" - floor(").expr(self, x, 0)?;
            w.append(") = 0.5 THEN (CASE WHEN mod(floor(").expr(self, x, 0)?.append("), 2) = 0 THEN floor(");
            w.expr(self, x, 0)?.append(") ELSE floor(").expr(self, x, 0)?.append(") + 1 END) ELSE round(");
            w.expr(self, x, 0)?.append(") END)");
            return Ok(());
        }
        let v = |w: &mut SqlWriter| -> Result<(), Refusal> {
            w.append("(").expr(self, x, 7)?.append(" * ").piece(&*p)?.append(")");
            Ok(())
        };
        w.append("(CASE WHEN ").piece(&v)?.append(" - floor(").piece(&v)?;
        w.append(") = 0.5 THEN (CASE WHEN mod(floor(").piece(&v)?.append("), 2) = 0 THEN floor(").piece(&v)?;
        w.append(") ELSE floor(").piece(&v)?.append(") + 1 END) ELSE round(").piece(&v)?.append(") END / ").piece(&*p)?;
        w.append(")");
        Ok(())
    }

    pub(super) fn bit_op_arm(&self, w: &mut SqlWriter, f: SqlFn, a: &[SqlExpr]) -> Result<(), Refusal> {
        let x = |w: &mut SqlWriter| -> Result<(), Refusal> {
            w.append("CAST(").expr(self, arg(a, 0)?, 0)?.append(" AS BIGINT)");
            Ok(())
        };
        let y = |w: &mut SqlWriter| -> Result<(), Refusal> {
            w.append("CAST(").expr(self, arg(a, 1)?, 0)?.append(" AS BIGINT)");
            Ok(())
        };
        let shift = |w: &mut SqlWriter| -> Result<(), Refusal> {
            w.append("CAST(").expr(self, arg(a, 1)?, 0)?.append(" AS INTEGER)");
            Ok(())
        };
        match f {
            SqlFn::BitAnd => w.append("bitwise_and(").piece(&x)?.append(", ").piece(&y)?.append(")"),
            SqlFn::BitOr => w.append("bitwise_or(").piece(&x)?.append(", ").piece(&y)?.append(")"),
            SqlFn::BitXor => w.append("bitwise_xor(").piece(&x)?.append(", ").piece(&y)?.append(")"),
            SqlFn::BitShiftLeft => {
                w.append("bitwise_left_shift(").piece(&x)?.append(", ").piece(&shift)?.append(")")
            }
            SqlFn::BitShiftRight => {
                w.append("bitwise_right_shift_arithmetic(").piece(&x)?.append(", ").piece(&shift)?.append(")")
            }
            other => return Err(Refusal::illegal_state(format!("not a bit op: {other}"))),
        };
        Ok(())
    }

    /// Every aggregate, as Trino spells it (Java's `reducer`).
    pub(super) fn reducer_arm(&self, w: &mut SqlWriter, r: &Reducer) -> Result<(), Refusal> {
        let plain = |w: &mut SqlWriter, name: &str| -> Result<(), Refusal> {
            unordered(r)?;
            ansi::aggregate(self, w, name, r)
        };
        match r.r#fn {
            AggFn::Sum => plain(w, "SUM"),
            AggFn::Count => plain(w, "COUNT"),
            AggFn::Avg => plain(w, "AVG"),
            AggFn::Min => plain(w, "MIN"),
            AggFn::Max => plain(w, "MAX"),
            AggFn::AnyValue => {
                w.append("arbitrary(").list(self, &r.args)?.append(")");
                Ok(())
            }
            AggFn::StddevSamp => plain(w, "STDDEV_SAMP"),
            AggFn::StddevPop => plain(w, "STDDEV_POP"),
            AggFn::VarSamp => plain(w, "VAR_SAMP"),
            AggFn::VarPop => plain(w, "VAR_POP"),
            AggFn::Median | AggFn::QuantileCont | AggFn::QuantileDisc => {
                let values = self.collected(arg(&r.args, 0)?, None);
                self.quantile(r, values)?(w)
            }
            AggFn::Mode => {
                let counts = self.counted(arg(&r.args, 0)?, None);
                mode(counts)(w)
            }
            AggFn::StringAgg => self.listagg(w, r),
            AggFn::List => self.array_agg(w, r),
            AggFn::Corr | AggFn::CovarSamp | AggFn::CovarPop => self.covariance(r, None)?(w),
            // the value at the key's greatest (least) row, rows of a NULL key ignored
            AggFn::ArgMax => self.by_extreme(w, "max_by", r),
            AggFn::ArgMin => self.by_extreme(w, "min_by", r),
            AggFn::BoolAnd => plain(w, "BOOL_AND"),
            AggFn::BoolOr => plain(w, "BOOL_OR"),
            AggFn::Variance => plain(w, "VARIANCE"),
            AggFn::Stddev => plain(w, "STDDEV"),
            AggFn::Wavg | AggFn::HashList | AggFn::IsDistinctMark | AggFn::UniqueValueOnly => {
                Err(ansi::lowering_marker(r))
            }
            // a window function is a ranking or value function, never an aggregate (windowCall writes it)
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
            | AggFn::NthValue => Err(ansi::window_function(r)),
        }
    }

    /// The `STRING_AGG` arm: `listagg(...) WITHIN GROUP (ORDER BY ...)`; one of no order `array_join(array_agg(...),
    /// sep)`, the values in the order they come (Java's `listagg`).
    fn listagg(&self, w: &mut SqlWriter, r: &Reducer) -> Result<(), Refusal> {
        if r.args.len() != 2 {
            return Err(Refusal::capability(format!("a STRING_AGG of {} arguments reached Trino", r.args.len())));
        }
        let distinct = if r.distinct { "DISTINCT " } else { "" };
        if r.order_by.is_empty() {
            w.append("array_join(array_agg(").append(distinct).expr(self, &r.args[0], 0)?.append("), ");
            w.expr(self, &r.args[1], 0)?.append(")");
            return Ok(());
        }
        w.append("listagg(").append(distinct).list(self, &r.args)?.append(") WITHIN GROUP (ORDER BY ");
        w.join(&r.order_by, ", ", |w, k| ansi::aggregate_order_key(self, w, k))?.append(")");
        Ok(())
    }

    /// The `LIST` arm (Java's `arrayAgg`).
    fn array_agg(&self, w: &mut SqlWriter, r: &Reducer) -> Result<(), Refusal> {
        if r.distinct || r.args.len() != 1 {
            return Err(Refusal::capability("a distinct LIST aggregate reached Trino"));
        }
        w.append("array_agg(").expr(self, &r.args[0], 0)?;
        if !r.order_by.is_empty() {
            w.append(" ORDER BY ").join(&r.order_by, ", ", |w, k| self.list_order_key(w, k))?;
        }
        w.append(")");
        Ok(())
    }

    /// `max_by(value, key)` / `min_by`: the value at the key's greatest (least) row (Java's `byExtreme`).
    fn by_extreme(&self, w: &mut SqlWriter, name: &str, r: &Reducer) -> Result<(), Refusal> {
        if r.args.len() != 2 || r.distinct || !r.order_by.is_empty() {
            return Err(Refusal::capability(format!("{} of this shape reached Trino", r.r#fn)));
        }
        w.append(name).append("(").list(self, &r.args)?.append(")");
        Ok(())
    }

    /// A covariance, or a correlation, its arguments DOUBLE; the correlation the population covariance over the
    /// product of the population deviations, as DuckDB's. Over `window`'s frame when it is one, each aggregate (Java's
    /// `covariance`).
    fn covariance<'a>(&'a self, r: &'a Reducer, window: Option<Window<'a>>) -> Result<P<'a>, Refusal> {
        if !r.order_by.is_empty() || r.distinct || r.args.len() != 2 {
            return Err(Refusal::capability(format!(
                "an ordered, distinct or other-shaped {} reached Trino",
                r.r#fn
            )));
        }
        let x: P<'a> = pc(move |w| {
            w.append("CAST(").expr(self, &r.args[0], 0)?.append(" AS DOUBLE)");
            Ok(())
        });
        let y: P<'a> = pc(move |w| {
            w.append("CAST(").expr(self, &r.args[1], 0)?.append(" AS DOUBLE)");
            Ok(())
        });
        let agg = move |f: &'static str, args: Vec<P<'a>>| -> P<'a> {
            pc(move |w| {
                w.append(f).append("(").join(&args, ", ", |w, p| p(w))?.append(")");
                self.window_over(w, window)
            })
        };
        Ok(match r.r#fn {
            AggFn::CovarSamp => agg("covar_samp", vec![x, y]),
            AggFn::CovarPop => agg("covar_pop", vec![x, y]),
            _ => {
                let cov = agg("covar_pop", vec![x.clone(), y.clone()]);
                let sx = agg("stddev_pop", vec![x]);
                let sy = agg("stddev_pop", vec![y]);
                pc(move |w| {
                    w.append("(").piece(&*cov)?.append(" / (").piece(&*sx)?.append(" * ").piece(&*sy)?.append("))");
                    Ok(())
                })
            }
        })
    }

    /// A key of a collected list's order: NULLs where the key says, else where they sort (largest) (Java's
    /// `listOrderKey`).
    fn list_order_key(&self, w: &mut SqlWriter, k: &SortKey) -> Result<(), Refusal> {
        w.expr(self, &k.expr, 0)?.append(if k.ascending { " ASC" } else { " DESC" });
        let nulls_first = match k.null_order {
            Some(o) => o == NullOrder::NullsFirst,
            None => !k.ascending,
        };
        w.append(if nulls_first { " NULLS FIRST" } else { " NULLS LAST" });
        Ok(())
    }

    /// A quantile of the group's values (Java's `quantile(r, values)`): the median's 0.5, else the reducer's second
    /// argument (its complement under a descending order).
    fn quantile<'a>(&'a self, r: &'a Reducer, values: P<'a>) -> Result<P<'a>, Refusal> {
        if r.r#fn == AggFn::Median {
            return Ok(self.quantile_of(values, None, 0.5, true, r));
        }
        if r.args.len() != 2 || r.distinct || r.order_by.len() > 1 {
            return Err(Refusal::capability(format!("{} of this shape reached Trino", r.r#fn)));
        }
        let desc = r.order_by.first().is_some_and(|k| !k.ascending);
        let q = if desc {
            build::call(SqlFn::Minus, vec![build::float_lit(1.0), r.args[1].clone()])
        } else {
            r.args[1].clone()
        };
        Ok(self.quantile_of(values, Some(q), 0.0, r.r#fn == AggFn::QuantileCont, r))
    }

    /// The values a quantile reads: `array_agg(x)`, over `window`'s frame when it is one (Java's `collected`).
    fn collected<'a>(&'a self, x: &'a SqlExpr, window: Option<Window<'a>>) -> P<'a> {
        pc(move |w| {
            w.append("array_agg(").expr(self, x, 0)?.append(")");
            self.window_over(w, window)
        })
    }

    /// The value counts a mode reads: `histogram(x)`, over `window`'s frame when it is one (Java's `counted`).
    fn counted<'a>(&'a self, x: &'a SqlExpr, window: Option<Window<'a>>) -> P<'a> {
        pc(move |w| {
            w.append("histogram(").expr(self, x, 0)?.append(")");
            self.window_over(w, window)
        })
    }

    /// The group's present values, sorted, bound once by a one-element transform and read: the continuous quantile
    /// interpolates between the values at `floor((n-1)q)` and `ceil((n-1)q)`, the discrete one is the value at
    /// `max(ceil(nq) - 1, 0)`; NULL over no value (Java's `quantile(values, q, fixed, cont, r)`).
    fn quantile_of<'a>(&'a self, values: P<'a>, q: Option<SqlExpr>, fixed: f64, cont: bool, r: &'a Reducer) -> P<'a> {
        let qp: P<'a> = match q {
            None => text(format!("CAST({} AS DOUBLE)", lite_javacompat::double_to_string(fixed))),
            Some(q) => pc(move |w| {
                w.append("CAST(").expr(self, &q, 0)?.append(" AS DOUBLE)");
                Ok(())
            }),
        };
        let s = "\"__qs\"";
        let n = format!("cardinality({s})");
        let ty = match &r.r#type {
            TypeFact::Typed { r#type, .. } => Some(r#type),
            _ => None,
        };
        let pos: P<'a> = {
            let (qp, n) = (qp.clone(), n.clone());
            pc(move |w| {
                w.append(&format!("(({n} - 1) * ")).piece(&*qp)?.append(")");
                Ok(())
            })
        };
        pc(move |w| {
            w.append("element_at(transform(ARRAY[filter(array_sort(").piece(&*values)?;
            w.append(&format!("), \"__x\" -> \"__x\" IS NOT NULL)], {s} -> CASE WHEN {n} = 0 THEN NULL "));
            if cont {
                let lo = |v: &mut SqlWriter| -> Result<(), Refusal> {
                    v.append(&format!("CAST(element_at({s}, CAST(floor(")).piece(&*pos)?;
                    v.append(") AS BIGINT) + 1) AS DOUBLE)");
                    Ok(())
                };
                let hi = |v: &mut SqlWriter| -> Result<(), Refusal> {
                    v.append(&format!("CAST(element_at({s}, CAST(ceiling(")).piece(&*pos)?;
                    v.append(") AS BIGINT) + 1) AS DOUBLE)");
                    Ok(())
                };
                w.append("ELSE ").piece(&lo)?.append(" + (").piece(&*pos)?.append(" - floor(").piece(&*pos)?;
                w.append(")) * (").piece(&hi)?.append(" - ").piece(&lo)?.append(") END), 1)");
            } else {
                w.append("ELSE ");
                if ty.is_some() {
                    w.append("CAST(");
                }
                w.append(&format!("element_at({s}, CAST(greatest(ceiling({n} * ")).piece(&*qp)?;
                w.append(") - 1, 0) AS BIGINT) + 1)");
                if let Some(t) = ty {
                    let name = cast_type_name(self, t)?;
                    w.append(" AS ").append(&name).append(")");
                }
                w.append(" END), 1)");
            }
            Ok(())
        })
    }

    /// A windowed median, quantile or mode reads the frame's values as its grouped form reads the group's; an average
    /// is a DOUBLE; a value or ranking function's offset is an integer literal (Java's `windowCall`).
    pub(super) fn window_call_arm(
        &self,
        w: &mut SqlWriter,
        f: &SqlAgg,
        partition_by: &[SqlExpr],
        order_by: &[SortKey],
        frame: Option<&Frame>,
    ) -> Result<(), Refusal> {
        let window = Window { partition_by, order_by, frame };
        if let SqlAgg::Reducer(r) = f
            && matches!(r.r#fn, AggFn::Median | AggFn::QuantileCont | AggFn::QuantileDisc | AggFn::Mode)
        {
            let x = arg(&r.args, 0)?;
            if r.r#fn == AggFn::Mode {
                return mode(self.counted(x, Some(window)))(w);
            }
            return self.quantile(r, self.collected(x, Some(window)))?(w);
        }
        if let SqlAgg::Reducer(r) = f
            && matches!(r.r#fn, AggFn::Corr | AggFn::CovarSamp | AggFn::CovarPop)
        {
            return self.covariance(r, Some(window))?(w);
        }
        let cast = matches!(f, SqlAgg::Reducer(r) if r.r#fn == AggFn::Avg);
        if cast {
            w.append("CAST(");
        }
        match f {
            // a windowed listagg of no order of its own concatenates in the window's
            SqlAgg::Reducer(r) if r.r#fn == AggFn::StringAgg && r.order_by.is_empty() && !order_by.is_empty() => {
                let ordered = Reducer {
                    r#fn: r.r#fn,
                    args: r.args.clone(),
                    distinct: r.distinct,
                    order_by: order_by.to_vec(),
                    r#type: r.r#type.clone(),
                };
                self.reducer(w, &ordered)?;
            }
            // a windowed listagg of no order at all: the values' own (listagg needs one)
            SqlAgg::Reducer(r) if r.r#fn == AggFn::StringAgg && r.order_by.is_empty() => {
                if r.args.len() != 2 {
                    return Err(Refusal::capability(format!(
                        "a STRING_AGG of {} arguments reached Trino",
                        r.args.len()
                    )));
                }
                w.append("listagg(").append(if r.distinct { "DISTINCT " } else { "" }).list(self, &r.args)?;
                w.append(") WITHIN GROUP (ORDER BY ").expr(self, &r.args[0], 0)?.append(")");
            }
            SqlAgg::Reducer(r) => self.reducer(w, r)?,
            SqlAgg::RankingFn { r#fn, args } | SqlAgg::ValueFn { r#fn, args } => {
                let name = self.aggregate_name(*r#fn);
                w.append(&name).append("(").list(self, args)?.append(")");
            }
        }
        ansi::over(self, w, partition_by, order_by, frame)?;
        if cast {
            w.append(" AS DOUBLE)");
        }
        Ok(())
    }
}

/// The group's most frequent value (DuckDB's mode): the value counts, the greatest count first, a tie by the smaller
/// value; NULL over no value (Java's `mode`).
fn mode(counts: P<'_>) -> P<'_> {
    pc(move |w| {
        w.append("element_at(array_sort(map_entries(").piece(&*counts)?;
        w.append("), (\"__l\", \"__r\") -> CASE WHEN \"__l\"[2] > \"__r\"[2] THEN -1 WHEN \"__l\"[2] < \"__r\"[2]");
        w.append(" THEN 1 WHEN \"__l\"[1] < \"__r\"[1] THEN -1 WHEN \"__l\"[1] > \"__r\"[1] THEN 1 ELSE 0 END), 1)[1]");
        Ok(())
    })
}
