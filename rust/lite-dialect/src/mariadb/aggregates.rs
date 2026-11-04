// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! MariaDB's and MySQL's aggregates and windows (Java's `reducer` -- MySql's delegates every one -- with GROUP_CONCAT,
//! the JSON array of a group, the grouped quantiles, the counted and JSON modes, argMax, the moments by their sums;
//! `windowCall` with the native and the ranked quantiles, the windowed moments and pairs, `windowOver` with its RANGE
//! keys in days), the hooks of the base (`hashSigned`, `roundHalfEven`, `bitOp`), the plan parameters' holes, the DDL
//! types, the JSON kinds and the wire (MariaDb.java, "Hooks of the base" to its end).

use super::{Counted, MariaDb, P, integral, is_double, pc, text, type_of, type_text};
use crate::ansi;
use crate::pieces::Ex;
use crate::refusal::{Refusal, arg};
use crate::renderer::AnsiSqlRenderer;
use crate::statement::{Digits, TypeSpelling};
use crate::writer::SqlWriter;
use lite_sql::{
    AggFn, Bound, ColumnKind, ColumnType, Frame, FrameKind, JsonKind, NullOrder, Reducer, Scalar, SortKey, SqlAgg,
    SqlExpr, SqlFn, SqlType, ValueKind,
};

/// Java's `elementSortOf(x, asc)`: one sort key, NULLs last, its NULL placement in its direction.
pub(super) fn element_sort_of<'a>(x: P<'a>, asc: bool) -> P<'a> {
    if asc {
        return pc(move |w| {
            w.append("(").piece(&*x)?.append(" IS NULL), ").piece(&*x)?;
            Ok(())
        });
    }
    pc(move |w| {
        w.append("(").piece(&*x)?.append(" IS NOT NULL) DESC, ").piece(&*x)?.append(" DESC");
        Ok(())
    })
}

/// Java's `ORDERED_SETS`.
fn ordered_set(f: AggFn) -> bool {
    matches!(f, AggFn::Median | AggFn::Mode | AggFn::QuantileCont | AggFn::QuantileDisc)
}

/// Java's `PAIRS`.
fn pair(f: AggFn) -> bool {
    matches!(f, AggFn::CovarPop | AggFn::CovarSamp | AggFn::Corr)
}

/// Java's `MOMENTS`.
fn moment(f: AggFn) -> bool {
    matches!(f, AggFn::StddevSamp | AggFn::Stddev | AggFn::StddevPop | AggFn::VarSamp | AggFn::Variance | AggFn::VarPop)
}

fn quantile_fn(f: AggFn) -> bool {
    matches!(f, AggFn::Median | AggFn::QuantileCont | AggFn::QuantileDisc)
}

fn interval_bound(b: &Bound) -> bool {
    matches!(b, Bound::IntervalPreceding { .. } | Bound::IntervalFollowing { .. })
}

/// Java's `FRAME_MICROS`: a DurationUnit's name to microseconds (months and years have no fixed length).
fn frame_micros(unit: &str) -> Option<i64> {
    match unit {
        "WEEKS" => Some(7 * 86_400_000_000),
        "DAYS" => Some(86_400_000_000),
        "HOURS" => Some(3_600_000_000),
        "MINUTES" => Some(60_000_000),
        "SECONDS" => Some(1_000_000),
        "MILLISECONDS" => Some(1000),
        _ => None,
    }
}

impl MariaDb {
    /// Java's `hashSigned`: Pure's hashCode, the first 64 bits of the MD5 of the value's text as JSON, read signed.
    pub(super) fn hash_signed_arm(&self, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
        w.append("CAST(CONV(LEFT(MD5(JSON_ARRAY(");
        w.join(a, ", ", |w, x| {
            let t = self.as_text(x);
            w.piece(&*t)?;
            Ok(())
        })?;
        w.append(")), 16), 16, -10) AS SIGNED)");
        Ok(())
    }

    /// Java's `roundHalfEven`: a double's ROUND is already half-even; a DECIMAL's tie that rounded odd is taken back.
    pub(super) fn round_half_even_arm(&self, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
        let x = arg(a, 0)?;
        if is_double(x) {
            w.append("ROUND(").list(self, a)?.append(")");
            return Ok(());
        }
        let scale = self.scale(a);
        let scaled = {
            let scale = scale.clone();
            pc(move |w| {
                w.append("(").expr(self, x, 6)?.append(" * ").piece(&*scale)?.append(")");
                Ok(())
            })
        };
        let r = {
            let scaled = scaled.clone();
            pc(move |w| {
                w.append("ROUND(").piece(&*scaled)?.append(")");
                Ok(())
            })
        };
        w.append("((CASE WHEN ABS(").piece(&*scaled)?.append(" - TRUNCATE(").piece(&*scaled)?;
        w.append(", 0)) = 0.5 AND MOD(").piece(&*r)?.append(", 2) <> 0 THEN ").piece(&*r)?.append(" - SIGN(");
        w.piece(&*scaled)?.append(") ELSE ").piece(&*r)?.append(" END) / ").piece(&*scale)?.append(")");
        Ok(())
    }

    /// Java's `bitOp`: 64-bit bit operators read back signed; a right shift a floor division.
    pub(super) fn bit_op_arm(&self, w: &mut SqlWriter, f: SqlFn, a: &[SqlExpr]) -> Result<(), Refusal> {
        let x = pc(move |w| {
            w.expr(self, arg(a, 0)?, 7)?;
            Ok(())
        });
        let y = pc(move |w| {
            w.expr(self, arg(a, 1)?, 7)?;
            Ok(())
        });
        let op = match f {
            SqlFn::BitAnd => " & ",
            SqlFn::BitOr => " | ",
            SqlFn::BitXor => " ^ ",
            SqlFn::BitShiftLeft => " << ",
            // a floor division by 2^y in integers
            SqlFn::BitShiftRight => {
                w.append("(CASE WHEN ").piece(&*x)?.append(" >= 0 THEN ").piece(&*x)?.append(" DIV CAST(1 << ");
                w.piece(&*y)?
                    .append(" AS SIGNED) ELSE -((-(")
                    .piece(&*x)?
                    .append(") - 1) DIV CAST(1 << ")
                    .piece(&*y)?;
                w.append(" AS SIGNED)) - 1 END)");
                return Ok(());
            }
            other => return Err(Refusal::illegal_state(format!("not a bit op: {other}"))),
        };
        w.append("CAST(").piece(&*x)?.append(op).piece(&*y)?.append(" AS SIGNED)");
        Ok(())
    }

    // ---- aggregates

    /// Java's `reducer`: every aggregate as this dialect spells it (MySql's delegates every one).
    pub(super) fn reducer_arm(&self, w: &mut SqlWriter, r: &Reducer) -> Result<(), Refusal> {
        use AggFn as A;
        match r.r#fn {
            A::Sum => ansi::aggregate(self, w, "SUM", r),
            A::Count => ansi::aggregate(self, w, "COUNT", r),
            A::Min => ansi::aggregate(self, w, "MIN", r),
            A::Max => ansi::aggregate(self, w, "MAX", r),
            // DuckDB's avg is a DOUBLE
            A::Avg => {
                w.append("AVG(").append(if r.distinct { "DISTINCT " } else { "" }).append("CAST(");
                w.expr(self, arg(&r.args, 0)?, 0)?.append(" AS DOUBLE))");
                Ok(())
            }
            // any non-NULL value: the least
            A::AnyValue => self.aggregate_of(w, "MIN", r),
            A::StddevSamp | A::Stddev => self.aggregate_of(w, "STDDEV_SAMP", r),
            A::VarSamp | A::Variance => self.aggregate_of(w, "VAR_SAMP", r),
            A::VarPop => self.aggregate_of(w, "VAR_POP", r),
            A::StddevPop => self.aggregate_of(w, "STDDEV_POP", r),
            A::CovarPop | A::CovarSamp | A::Corr => {
                let m = self.moments(r)?;
                w.piece(&*m)?;
                Ok(())
            }
            A::BoolAnd => self.aggregate_of(w, "MIN", r),
            A::BoolOr => self.aggregate_of(w, "MAX", r),
            // GROUP_CONCAT's separator is a literal
            A::StringAgg => self.group_concat(w, r),
            A::List => self.list_of_values(w, r),
            A::Median => {
                if r.args.len() != 1 || r.distinct {
                    return Err(Refusal::capability(format!("a median of this shape reached {}", self.product())));
                }
                let q = self.quantile(A::QuantileCont, &r.args[0], text("0.5"), r);
                w.piece(&*q)?;
                Ok(())
            }
            A::QuantileCont | A::QuantileDisc => {
                if r.args.len() != 2 || r.distinct || r.order_by.len() > 1 {
                    return Err(Refusal::capability(format!("{} of this shape reached {}", r.r#fn, self.product())));
                }
                let q = self.quantile(r.r#fn, &r.args[0], self.ex(&r.args[1], 0), r);
                w.piece(&*q)?;
                Ok(())
            }
            // the most frequent non-NULL value, the least among ties
            A::Mode => {
                if self.grouped_modes_counted() {
                    self.mode_arm(w, Counted::Grouped(r))
                } else {
                    self.mode_over_json(w, r)
                }
            }
            // the value at the key's greatest (least) row, rows of a NULL key ignored
            A::ArgMax => self.arg_extreme(w, r, false),
            A::ArgMin => self.arg_extreme(w, r, true),
            A::Wavg | A::HashList | A::IsDistinctMark | A::UniqueValueOnly => Err(ansi::lowering_marker(r)),
            // a window function is a ranking or value function, never an aggregate
            A::RowNumber
            | A::Rank
            | A::DenseRank
            | A::PercentRank
            | A::CumeDist
            | A::Ntile
            | A::Lag
            | A::Lead
            | A::FirstValue
            | A::LastValue
            | A::NthValue => Err(ansi::window_function(r)),
        }
    }

    /// `name([DISTINCT ]args)` (Java's `aggregateArgs` in a name).
    fn aggregate_of(&self, w: &mut SqlWriter, name: &str, r: &Reducer) -> Result<(), Refusal> {
        w.append(name).append("(").append(if r.distinct { "DISTINCT " } else { "" }).list(self, &r.args)?.append(")");
        Ok(())
    }

    /// Java's `groupConcat`: GROUP_CONCAT, its separator a literal.
    fn group_concat(&self, w: &mut SqlWriter, r: &Reducer) -> Result<(), Refusal> {
        let comma = crate::build::string_lit(",");
        let sep = if r.args.len() > 1 { &r.args[1] } else { &comma };
        w.append("GROUP_CONCAT(").append(if r.distinct { "DISTINCT " } else { "" });
        let t = self.as_text(arg(&r.args, 0)?);
        w.piece(&*t)?;
        if !r.order_by.is_empty() {
            w.append(" ORDER BY ").join(&r.order_by, ", ", |w, k| self.sort_key(w, k))?;
        }
        let separator = self.literal(sep, "a string_agg separator")?;
        w.append(&format!(" SEPARATOR {})", self.string_literal(separator)));
        Ok(())
    }

    /// Java's `listOfValues`: the LIST aggregate, a JSON array.
    fn list_of_values(&self, w: &mut SqlWriter, r: &Reducer) -> Result<(), Refusal> {
        if r.args.len() != 1 {
            return Err(Refusal::capability(format!(
                "a LIST aggregate of {} arguments reached {}",
                r.args.len(),
                self.product()
            )));
        }
        let order = if r.order_by.is_empty() {
            None
        } else {
            Some(pc(move |w| {
                w.join(&r.order_by, ", ", |w, k| self.sort_key(w, k))?;
                Ok(())
            }))
        };
        let agg = self.array_agg(r.distinct, self.json(&r.args[0]), order, false);
        w.piece(&*agg)?;
        Ok(())
    }

    /// Java's `modeOverJson`: the group's values as a JSON array, grouped by a subquery over it.
    fn mode_over_json(&self, w: &mut SqlWriter, r: &Reducer) -> Result<(), Refusal> {
        let x = arg(&r.args, 0)?;
        let r#type = self.column_type(type_of(x));
        w.append("(SELECT __m.\"v\" FROM JSON_TABLE(JSON_ARRAYAGG(");
        let json = self.json(x);
        w.piece(&*json)?;
        w.append(&format!("), '$[*]' COLUMNS (\"v\" {type} PATH '$')) AS __m WHERE __m.\"v\" IS NOT NULL"));
        w.append(" GROUP BY __m.\"v\" ORDER BY count(*) DESC, __m.\"v\" LIMIT 1)");
        Ok(())
    }

    /// Java's `argExtreme`: the value at the key's greatest (`asc` false) or least row.
    fn arg_extreme(&self, w: &mut SqlWriter, r: &Reducer, asc: bool) -> Result<(), Refusal> {
        if r.args.len() != 2 || r.distinct || !r.order_by.is_empty() {
            return Err(Refusal::capability(format!("{} of this shape reached {}", r.r#fn, self.product())));
        }
        let v = &r.args[0];
        let k = &r.args[1];
        let first = self.array_agg(false, self.json(v), Some(element_sort_of(self.ex(k, 0), asc)), true);
        w.append("(CASE WHEN count(").expr(self, k, 0)?.append(") = 0 THEN NULL ELSE ");
        let at = self.value_at(first, text("'$[0]'"), type_of(v));
        w.piece(&*at)?.append(" END)");
        Ok(())
    }

    /// Java's `moments`: covariance and correlation over the pairs with both values, from their sums.
    fn moments<'a>(&'a self, r: &'a Reducer) -> Result<P<'a>, Refusal> {
        let y = arg(&r.args, 0)?;
        let x = arg(&r.args, 1)?;
        let both = pc(move |w| {
            w.append("(").expr(self, x, 4)?.append(" IS NOT NULL AND ").expr(self, y, 4)?.append(" IS NOT NULL)");
            Ok(())
        });
        let n = pc(move |w| {
            w.append("count(").expr(self, x, 6)?.append(" * ").expr(self, y, 6)?.append(")");
            Ok(())
        });
        let sxy = pc(move |w| {
            w.append("sum(CAST(").expr(self, x, 0)?.append(" AS DOUBLE) * ").expr(self, y, 6)?.append(")");
            Ok(())
        });
        let sum_of = |v: &'a SqlExpr| {
            let both = both.clone();
            pc(move |w| {
                w.append("sum(CASE WHEN ").piece(&*both)?.append(" THEN CAST(").expr(self, v, 0)?;
                w.append(" AS DOUBLE) END)");
                Ok(())
            })
        };
        let squares_of = |v: &'a SqlExpr| {
            let both = both.clone();
            pc(move |w| {
                w.append("sum(CASE WHEN ").piece(&*both)?.append(" THEN CAST(").expr(self, v, 0)?;
                w.append(" AS DOUBLE) * ").expr(self, v, 6)?.append(" END)");
                Ok(())
            })
        };
        let (sx, sy, sxx, syy) = (sum_of(x), sum_of(y), squares_of(x), squares_of(y));
        let cxy = {
            let (sxy, sx, sy, n) = (sxy.clone(), sx.clone(), sy.clone(), n.clone());
            pc(move |w| {
                w.append("(").piece(&*sxy)?.append(" - ").piece(&*sx)?.append(" * ").piece(&*sy)?.append(" / ");
                w.piece(&*n)?.append(")");
                Ok(())
            })
        };
        Ok(match r.r#fn {
            AggFn::CovarPop => pc(move |w| {
                w.append("(CASE WHEN ").piece(&*n)?.append(" > 0 THEN ").piece(&*cxy)?.append(" / ").piece(&*n)?;
                w.append(" END)");
                Ok(())
            }),
            AggFn::CovarSamp => pc(move |w| {
                w.append("(CASE WHEN ").piece(&*n)?.append(" > 1 THEN ").piece(&*cxy)?.append(" / (").piece(&*n)?;
                w.append(" - 1) END)");
                Ok(())
            }),
            _ => pc(move |w| {
                w.append("(CASE WHEN ").piece(&*n)?.append(" > 1 THEN ").piece(&*cxy)?.append(" / SQRT((");
                w.piece(&*sxx)?.append(" - ").piece(&*sx)?.append(" * ").piece(&*sx)?.append(" / ").piece(&*n)?;
                w.append(") * (").piece(&*syy)?.append(" - ").piece(&*sy)?.append(" * ").piece(&*sy)?.append(" / ");
                w.piece(&*n)?.append(")) END)");
                Ok(())
            }),
        })
    }

    /// Java's `quantile`: a grouped quantile from the group's values sorted, NULLs last, and indexed.
    fn quantile<'a>(&'a self, f: AggFn, x: &'a SqlExpr, q: P<'a>, r: &Reducer) -> P<'a> {
        let desc = r.order_by.first().is_some_and(|k| !k.ascending);
        let t = type_of(x);
        let sorted = self.array_agg(false, self.json(x), Some(element_sort_of(self.ex(x, 0), !desc)), false);
        let n = pc(move |w| {
            w.append("count(").expr(self, x, 0)?.append(")");
            Ok(())
        });
        let double = SqlType::Scalar(Scalar::Double);
        let slot = if f == AggFn::QuantileDisc { t } else { Some(&double) };
        let at = |i: P<'a>| self.value_at(sorted.clone(), self.index_path(i), slot);
        if f == AggFn::QuantileDisc {
            let (n, q) = (n.clone(), q.clone());
            return at(pc(move |w| {
                w.append("GREATEST(CEIL(").piece(&*n)?.append(" * (").piece(&*q)?.append(")) - 1, 0)");
                Ok(())
            }));
        }
        let pos = {
            let (n, q) = (n.clone(), q.clone());
            pc(move |w| {
                w.append("((").piece(&*q)?.append(") * (").piece(&*n)?.append(" - 1))");
                Ok(())
            })
        };
        interpolated(&at, pos)
    }

    /// Java's `rankedQuantile`: a whole partition's median or quantile from each row's rank.
    fn ranked_quantile<'a>(
        &'a self,
        r: &'a Reducer,
        window: Counted<'a>,
        partition_by: &'a [SqlExpr],
    ) -> Result<P<'a>, Refusal> {
        let Some(counted) = self.counted(window) else {
            return Err(Refusal::illegal_state(format!(
                "a windowed {} reached {}'s writer unranked: modesCounted did not run",
                r.r#fn,
                self.product()
            )));
        };
        let rank = std::rc::Rc::new(counted[0].clone());
        let n = std::rc::Rc::new(counted[1].clone());
        let x = arg(&r.args, 0)?;
        let q: P<'a> = if r.r#fn == AggFn::Median {
            text("0.5")
        } else {
            pc(move |v| {
                v.expr(self, arg(&r.args, 1)?, 0)?;
                Ok(())
            })
        };
        let over = pc(move |v| {
            v.append(" OVER (");
            if !partition_by.is_empty() {
                v.append("PARTITION BY ").list(self, partition_by)?;
            }
            v.append(")");
            Ok(())
        });
        let disc = r.r#fn == AggFn::QuantileDisc;
        let at = |i: P<'a>| -> P<'a> {
            let (rank, over) = (rank.clone(), over.clone());
            pc(move |v| {
                v.append("MAX(CASE WHEN ").expr(self, &rank, 0)?.append(" = ").piece(&*i)?.append(" + 1 THEN ");
                if disc {
                    v.expr(self, x, 0)?;
                } else {
                    v.append("CAST(").expr(self, x, 0)?.append(" AS DOUBLE)");
                }
                v.append(" END)").piece(&*over)?;
                Ok(())
            })
        };
        if disc {
            let (n, q) = (n.clone(), q.clone());
            return Ok(at(pc(move |v| {
                v.append("GREATEST(CEIL(").expr(self, &n, 0)?.append(" * (").piece(&*q)?.append(")) - 1, 0)");
                Ok(())
            })));
        }
        let pos = {
            let (n, q) = (n.clone(), q.clone());
            pc(move |v| {
                v.append("((").piece(&*q)?.append(") * (").expr(self, &n, 0)?.append(" - 1))");
                Ok(())
            })
        };
        Ok(interpolated(&at, pos))
    }

    // ---- windows

    /// Java's `windowCall` (MariaDb's): the standard window functions; the quantiles natively or from ranks, a MODE
    /// counted, the moments and pairs from window sums; GROUP_CONCAT, JSON_ARRAYAGG, a mode and an argMax no window
    /// functions here.
    pub(super) fn window_call_arm(
        &self,
        w: &mut SqlWriter,
        f: &SqlAgg,
        partition_by: &[SqlExpr],
        order_by: &[SortKey],
        frame: Option<&Frame>,
    ) -> Result<(), Refusal> {
        let window = Counted::Window { r#fn: f, partition_by, order_by, frame };
        let whole = order_by.is_empty() && frame.is_none();
        match f {
            SqlAgg::Reducer(r) if !self.window_quantiles_native() && quantile_fn(r.r#fn) && whole => {
                let q = self.ranked_quantile(r, window, partition_by)?;
                w.piece(&*q)?;
                return Ok(());
            }
            SqlAgg::Reducer(r) if quantile_fn(r.r#fn) && whole => {
                let x = arg(&r.args, 0)?;
                let desc = r.order_by.first().is_some_and(|k| !k.ascending);
                w.append(if r.r#fn == AggFn::QuantileDisc { "PERCENTILE_DISC(" } else { "PERCENTILE_CONT(" });
                if r.r#fn == AggFn::Median {
                    w.append("0.5");
                } else {
                    w.expr(self, arg(&r.args, 1)?, 0)?;
                }
                w.append(") WITHIN GROUP (ORDER BY ").expr(self, x, 0)?.append(if desc { " DESC)" } else { ")" });
                return self.window_over(w, partition_by, order_by, frame);
            }
            SqlAgg::Reducer(r) if r.r#fn == AggFn::Mode && whole => {
                return self.mode_arm(w, window);
            }
            SqlAgg::Reducer(r) if ordered_set(r.r#fn) => {
                return Err(Refusal::capability(format!(
                    "{} over an ordered or framed window reached {}",
                    r.r#fn,
                    self.product()
                )));
            }
            SqlAgg::Reducer(r) if pair(r.r#fn) => {
                let p = self.window_pairs(r, partition_by, order_by, frame)?;
                w.piece(&*p)?;
                return Ok(());
            }
            SqlAgg::Reducer(r) if moment(r.r#fn) && integral(arg(&r.args, 0)?) => {
                let p = self.window_moment(r, partition_by, order_by, frame)?;
                w.piece(&*p)?;
                return Ok(());
            }
            SqlAgg::Reducer(r) if matches!(r.r#fn, AggFn::ArgMax | AggFn::ArgMin | AggFn::StringAgg | AggFn::List) => {
                return Err(Refusal::capability(format!(
                    "a windowed {} reached {}, where it is no window function",
                    r.r#fn,
                    self.product()
                )));
            }
            SqlAgg::Reducer(r) if !r.order_by.is_empty() => {
                if !order_by.is_empty() || frame.is_some() {
                    return Err(Refusal::capability(format!(
                        "an ordered {} over an ordered or framed window reached {}",
                        r.r#fn,
                        self.product()
                    )));
                }
                let unordered = crate::build::reducer(r.r#fn, r.args.clone(), r.distinct, Vec::new());
                self.reducer(w, &unordered)?;
                let whole_partition =
                    Frame { kind: FrameKind::Rows, from: Bound::UnboundedPreceding, to: Bound::UnboundedFollowing };
                return self.window_over(w, partition_by, &r.order_by, Some(&whole_partition));
            }
            SqlAgg::Reducer(r) => self.reducer(w, r)?,
            SqlAgg::RankingFn { r#fn, args } | SqlAgg::ValueFn { r#fn, args } => {
                let name = self.aggregate_name(*r#fn);
                w.append(&name).append("(").list(self, args)?.append(")");
            }
        }
        self.window_over(w, partition_by, order_by, frame)
    }

    /// Java's `windowMoment`: a windowed variance or standard deviation of integers from their exact sums.
    fn window_moment<'a>(
        &'a self,
        r: &'a Reducer,
        partition_by: &'a [SqlExpr],
        order_by: &'a [SortKey],
        frame: Option<&'a Frame>,
    ) -> Result<P<'a>, Refusal> {
        let x = arg(&r.args, 0)?;
        let over = |agg: P<'a>| -> P<'a> {
            pc(move |v| {
                v.piece(&*agg)?;
                self.window_over(v, partition_by, order_by, frame)
            })
        };
        let n = over(pc(move |v| {
            v.append("count(").expr(self, x, 0)?.append(")");
            Ok(())
        }));
        let sx = over(pc(move |v| {
            v.append("sum(CAST(").expr(self, x, 0)?.append(" AS DECIMAL(65,0)))");
            Ok(())
        }));
        let sxx = over(pc(move |v| {
            v.append("sum(CAST(").expr(self, x, 0)?.append(" AS DECIMAL(65,0)) * CAST(").expr(self, x, 0)?;
            v.append(" AS DECIMAL(65,0)))");
            Ok(())
        }));
        let sample = matches!(r.r#fn, AggFn::StddevSamp | AggFn::Stddev | AggFn::VarSamp | AggFn::Variance);
        let root = matches!(r.r#fn, AggFn::StddevSamp | AggFn::Stddev | AggFn::StddevPop);
        let variance = {
            let n = n.clone();
            pc(move |v| {
                v.append("CAST((").piece(&*sxx)?.append(" * ").piece(&*n)?.append(" - ").piece(&*sx)?.append(" * ");
                v.piece(&*sx)?.append(") / (").piece(&*n)?.append(" * ");
                if sample {
                    v.append("(").piece(&*n)?.append(" - 1)");
                } else {
                    v.piece(&*n)?;
                }
                v.append(") AS DOUBLE)");
                Ok(())
            })
        };
        Ok(pc(move |v| {
            v.append("(CASE WHEN ").piece(&*n)?.append(if sample { " > 1" } else { " > 0" }).append(" THEN ");
            if root {
                v.append("SQRT(").piece(&*variance)?.append(")");
            } else {
                v.piece(&*variance)?;
            }
            v.append(" END)");
            Ok(())
        }))
    }

    /// Java's `windowPairs`: a windowed covariance or correlation from its sums over the window.
    fn window_pairs<'a>(
        &'a self,
        r: &'a Reducer,
        partition_by: &'a [SqlExpr],
        order_by: &'a [SortKey],
        frame: Option<&'a Frame>,
    ) -> Result<P<'a>, Refusal> {
        let y = arg(&r.args, 0)?;
        let x = arg(&r.args, 1)?;
        let over = |inner: P<'a>| -> P<'a> {
            pc(move |v| {
                v.append("sum(").piece(&*inner)?.append(")");
                self.window_over(v, partition_by, order_by, frame)
            })
        };
        let both = pc(move |v| {
            v.append("(CASE WHEN ").expr(self, x, 4)?.append(" IS NOT NULL AND ").expr(self, y, 4)?;
            v.append(" IS NOT NULL THEN 1 END)");
            Ok(())
        });
        let n = over(both.clone());
        let times = |a: &'a SqlExpr| {
            let both = both.clone();
            over(pc(move |v| {
                v.append("CAST(").expr(self, a, 0)?.append(" AS DOUBLE) * ").piece(&*both)?;
                Ok(())
            }))
        };
        let product = |a: &'a SqlExpr, b: &'a SqlExpr| {
            let both = both.clone();
            over(pc(move |v| {
                v.append("CAST(").expr(self, a, 0)?.append(" AS DOUBLE) * ").expr(self, b, 6)?.append(" * ");
                v.piece(&*both)?;
                Ok(())
            }))
        };
        let (sx, sy) = (times(x), times(y));
        let (sxy, sxx, syy) = (product(x, y), product(x, x), product(y, y));
        let cxy = {
            let (sx, sy, n) = (sx.clone(), sy.clone(), n.clone());
            pc(move |v| {
                v.append("(").piece(&*sxy)?.append(" - ").piece(&*sx)?.append(" * ").piece(&*sy)?.append(" / ");
                v.piece(&*n)?.append(")");
                Ok(())
            })
        };
        let spread = |ss: P<'a>, s: P<'a>| -> P<'a> {
            let n = n.clone();
            pc(move |v| {
                v.append("GREATEST(").piece(&*ss)?.append(" - ").piece(&*s)?.append(" * ").piece(&*s)?.append(" / ");
                v.piece(&*n)?.append(", 0.0E0)");
                Ok(())
            })
        };
        let (cxx, cyy) = (spread(sxx, sx), spread(syy, sy));
        Ok(match r.r#fn {
            AggFn::CovarPop => pc(move |v| {
                v.append("(CASE WHEN ").piece(&*n)?.append(" > 0 THEN ").piece(&*cxy)?.append(" / ").piece(&*n)?;
                v.append(" END)");
                Ok(())
            }),
            AggFn::CovarSamp => pc(move |v| {
                v.append("(CASE WHEN ").piece(&*n)?.append(" > 1 THEN ").piece(&*cxy)?.append(" / (").piece(&*n)?;
                v.append(" - 1) END)");
                Ok(())
            }),
            _ => pc(move |v| {
                v.append("(CASE WHEN ").piece(&*n)?.append(" > 1 THEN ").piece(&*cxy)?.append(" / SQRT(");
                v.piece(&*cxx)?.append(" * ").piece(&*cyy)?.append(") END)");
                Ok(())
            }),
        })
    }

    /// Java's `windowOver`: a window's `OVER (...)`, a RANGE frame with an interval offset over a date key in days.
    fn window_over(
        &self,
        w: &mut SqlWriter,
        partition_by: &[SqlExpr],
        order_by: &[SortKey],
        frame: Option<&Frame>,
    ) -> Result<(), Refusal> {
        w.append(" OVER (");
        let mut any = false;
        if !partition_by.is_empty() {
            w.append("PARTITION BY ").list(self, partition_by)?;
            any = true;
        }
        let mut days = false;
        if !order_by.is_empty() {
            w.append(if any { " " } else { "" }).append("ORDER BY ");
            let interval = frame.is_some_and(|f| interval_bound(&f.from) || interval_bound(&f.to));
            let range = frame.is_some_and(|f| f.kind == FrameKind::Range);
            if interval {
                if order_by.len() != 1 {
                    return Err(Refusal::capability(format!(
                        "a RANGE frame over {} keys reached {}",
                        order_by.len(),
                        self.product()
                    )));
                }
                let k = &order_by[0];
                let kt = type_of(&k.expr);
                if !matches!(kt, Some(SqlType::Scalar(Scalar::Date | Scalar::Timestamp))) {
                    return Err(Refusal::capability(format!(
                        "an interval RANGE frame over a {} key reached {}",
                        type_text(kt),
                        self.product()
                    )));
                }
                days = true;
                w.append("(TIMESTAMPDIFF(MICROSECOND, TIMESTAMP '1970-01-01 00:00:00', ").expr(self, &k.expr, 0)?;
                w.append(") / 86400000000)").append(if k.ascending { "" } else { " DESC" });
            } else if range {
                // a RANGE frame's one key carries no NULL placement key beside it: a NULL is the key past either end
                w.join(order_by, ", ", |x, k| {
                    let nulls_first = match k.null_order {
                        Some(order) => order == NullOrder::NullsFirst,
                        None => !k.ascending,
                    };
                    x.append("coalesce(").expr(self, &k.expr, 0)?;
                    x.append(if nulls_first == k.ascending { ", -1.0E308)" } else { ", 1.0E308)" });
                    x.append(if k.ascending { "" } else { " DESC" });
                    Ok(())
                })?;
            } else {
                w.join(order_by, ", ", |x, k| self.sort_key(x, k))?;
            }
            any = true;
        }
        if let Some(f) = frame {
            let from = self.frame_bound(&f.from, days)?;
            let to = self.frame_bound(&f.to, days)?;
            w.append(if any { " " } else { "" }).append(f.kind.as_str()).append(" BETWEEN ").append(&from);
            w.append(" AND ").append(&to);
        }
        w.append(")");
        Ok(())
    }

    /// Java's `frameBound`: an interval offset in days, any other bound the base's.
    fn frame_bound(&self, b: &Bound, days: bool) -> Result<String, Refusal> {
        match b {
            Bound::IntervalPreceding { n, unit } => Ok(format!("{} PRECEDING", self.interval_days(*n, unit, days)?)),
            Bound::IntervalFollowing { n, unit } => Ok(format!("{} FOLLOWING", self.interval_days(*n, unit, days)?)),
            other => self.bound(other),
        }
    }

    /// Java's `intervalDays`: an interval offset in days (a double's text).
    pub(super) fn interval_days(&self, n: i64, unit: &str, days: bool) -> Result<String, Refusal> {
        let micros = frame_micros(&unit.to_uppercase());
        let (true, Some(micros)) = (days, micros) else {
            return Err(Refusal::capability(format!(
                "a {unit} window frame offset over this key reached {}",
                self.product()
            )));
        };
        Ok(lite_javacompat::double_to_string(n as f64 * micros as f64 / 86_400_000_000.0))
    }
}

/// `lo + (hi - lo) * (pos - FLOOR(pos))`, `lo` and `hi` the values at `FLOOR(pos)` and `CEIL(pos)` (the continuous
/// quantile's interpolation, grouped or ranked).
fn interpolated<'a>(at: &dyn Fn(P<'a>) -> P<'a>, pos: P<'a>) -> P<'a> {
    let floor = {
        let pos = pos.clone();
        pc(move |w| {
            w.append("FLOOR(").piece(&*pos)?.append(")");
            Ok(())
        })
    };
    let ceil = {
        let pos = pos.clone();
        pc(move |w| {
            w.append("CEIL(").piece(&*pos)?.append(")");
            Ok(())
        })
    };
    let lo = at(floor);
    let hi = at(ceil);
    pc(move |w| {
        w.append("(").piece(&*lo)?.append(" + (").piece(&*hi)?.append(" - ").piece(&*lo)?.append(") * (");
        w.piece(&*pos)?.append(" - FLOOR(").piece(&*pos)?.append(")))");
        Ok(())
    })
}

/// Java's `holeType`: a date's and a date-time's literal types (a date-time's six digits are DATETIME(6)'s).
pub(super) fn hole_type(kind: ValueKind) -> Result<TypeSpelling, Refusal> {
    match kind {
        ValueKind::Date => Ok(TypeSpelling::new("DATE", Digits::None)),
        ValueKind::DateTime | ValueKind::DateTimeNanos => Ok(TypeSpelling::new("DATETIME(6)", Digits::None)),
        ValueKind::Integer | ValueKind::Decimal | ValueKind::Floating => ansi::hole_type(kind),
    }
}

/// Java's `ddlType`: a store column's type in MariaDB's DDL.
pub(super) fn ddl_type(t: &ColumnType) -> Result<String, Refusal> {
    Ok(match t {
        ColumnType::Plain { kind } => match kind {
            ColumnKind::Bigint => "BIGINT",
            ColumnKind::Smallint => "SMALLINT",
            ColumnKind::Tinyint => "TINYINT",
            ColumnKind::Integer => "INT",
            ColumnKind::Bit => "BOOLEAN",
            ColumnKind::Float | ColumnKind::Double | ColumnKind::Real => "DOUBLE",
            ColumnKind::Timestamp => "DATETIME(6)",
            ColumnKind::Date => "DATE",
            ColumnKind::Time => "TIME(6)",
            // a nanosecond timestamp column: MariaDB's DATETIME keeps microseconds
            ColumnKind::TimestampNs => {
                return Err(Refusal::capability("a TIMESTAMP_NS column: MariaDB keeps microseconds"));
            }
            ColumnKind::Json
            | ColumnKind::Varchar
            | ColumnKind::Other
            | ColumnKind::Distinct
            | ColumnKind::Array
            | ColumnKind::Object => "LONGTEXT",
        }
        .to_owned(),
        ColumnType::Sized { kind, size } => {
            if kind.to_uppercase().contains("BINARY") {
                "LONGBLOB".to_owned()
            } else {
                format!("VARCHAR({size})")
            }
        }
        ColumnType::Scaled { precision, scale, .. } => format!("DECIMAL({precision},{scale})"),
    })
}

/// Java's `jsonKindName`: its JSON_TYPE answers DuckDB's names.
pub(super) fn json_kind_name(kind: JsonKind) -> &'static str {
    match kind {
        JsonKind::Object => "OBJECT",
        JsonKind::Array => "ARRAY",
        JsonKind::Null => "NULL",
        JsonKind::String => "VARCHAR",
        JsonKind::Integer => "BIGINT",
        JsonKind::UnsignedInteger => "UBIGINT",
        JsonKind::Float => "DOUBLE",
        JsonKind::Boolean => "BOOLEAN",
    }
}

/// Java's `wireTypeName`: its text types and a UUID are text, its DATETIME the naive date-time.
pub(super) fn wire_type_name(driver_type_name: Option<&str>) -> String {
    crate::wire_names::fold(driver_type_name.map(|n| match n {
        "LONGTEXT" | "MEDIUMTEXT" | "TEXT" | "TINYTEXT" | "CHAR" | "UUID" => "VARCHAR",
        "DATETIME" => "TIMESTAMP",
        other => other,
    }))
}

/// Java's `deliversOwn`: a comparison and a boolean are integers; a list, struct, map or Variant JSON text; an
/// integer sum a DECIMAL of scale 0 or an unsigned BIGINT; a NULL literal's column claims no type.
pub(super) fn delivers_own(label: &SqlType, wire: &str) -> bool {
    let integral = matches!(label, SqlType::Scalar(Scalar::Bigint | Scalar::Integer | Scalar::Hugeint));
    let composite = matches!(
        label,
        SqlType::Array { .. } | SqlType::Struct { .. } | SqlType::Map { .. } | SqlType::Scalar(Scalar::Json)
    );
    wire == "NULL"
        || *label == SqlType::Scalar(Scalar::Boolean) && matches!(wire, "INTEGER" | "BIGINT" | "TINYINT")
        || integral && (wire.starts_with("DECIMAL(") && wire.ends_with(",0)") || wire == "BIGINT UNSIGNED")
        || matches!(label, SqlType::Decimal { scale, .. }
            if wire.starts_with("DECIMAL(") && wire.ends_with(&format!(",{scale})")))
        || composite && matches!(wire, "JSON" | "VARCHAR")
}
