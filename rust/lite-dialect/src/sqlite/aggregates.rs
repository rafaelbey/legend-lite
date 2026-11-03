// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! SQLite's aggregates and windows, and the base's hooks it spells (SQLite.java, "Hooks of the base" and "Aggregates
//! and windows"): the signed hash, banker's ROUND, the bit operators, every reducer -- the statistics extension's
//! sample moments, the population ones from them, covariance and correlation from their sums, the ordered sets over a
//! sorted JSON array -- and the windows, a windowed moment its sums over the window.

use super::{P, Sqlite, digests, pc, scalar_is, type_of};
use crate::ansi;
use crate::build;
use crate::java::sql_type_text;
use crate::refusal::{Refusal, arg};
use crate::renderer::AnsiSqlRenderer;
use crate::writer::SqlWriter;
use lite_sql::{AggFn, Bound, Frame, FrameKind, NullOrder, Reducer, Scalar, SortKey, SqlAgg, SqlExpr, SqlFn};

/// Java's `ORDERED_SETS`.
const ORDERED_SETS: [AggFn; 4] = [AggFn::Median, AggFn::Mode, AggFn::QuantileCont, AggFn::QuantileDisc];

/// Java's `MOMENTS`.
const MOMENTS: [AggFn; 9] = [
    AggFn::VarSamp,
    AggFn::Variance,
    AggFn::VarPop,
    AggFn::StddevSamp,
    AggFn::Stddev,
    AggFn::StddevPop,
    AggFn::CovarPop,
    AggFn::CovarSamp,
    AggFn::Corr,
];

/// A DurationUnit name's length in microseconds (Java's `FRAME_MICROS`; months and years have no fixed length).
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

/// An interval frame offset in days of a Julian day key (Java's `intervalDays`).
fn interval_days(n: i64, unit: &str, days: bool) -> Result<String, Refusal> {
    let micros = frame_micros(&unit.to_uppercase());
    match micros {
        Some(m) if days => Ok(lite_javacompat::double_to_string(n as f64 * m as f64 / 86_400_000_000.0)),
        _ => Err(Refusal::capability(format!("a {unit} window frame offset over this key reached SQLite"))),
    }
}

fn interval_bound(b: &Bound) -> bool {
    matches!(b, Bound::IntervalPreceding { .. } | Bound::IntervalFollowing { .. })
}

/// A window's partition, order and frame.
#[derive(Clone, Copy)]
struct Window<'a> {
    partition_by: &'a [SqlExpr],
    order_by: &'a [SortKey],
    frame: Option<&'a Frame>,
}

/// `max(ss - s * s / n, 0.0)`: a windowed sum of squares about the mean.
fn square<'a>(s: P<'a>, ss: P<'a>, n: P<'a>) -> P<'a> {
    pc(move |v: &mut SqlWriter| {
        v.append("max(");
        ss(v)?;
        v.append(" - ");
        s(v)?;
        v.append(" * ");
        s(v)?;
        v.append(" / ");
        n(v)?;
        v.append(", 0.0)");
        Ok(())
    })
}

impl Sqlite {
    /// `[DISTINCT ]args` (Java's `aggregateArgs`).
    fn aggregate_args<'a>(&'a self, r: &'a Reducer) -> P<'a> {
        pc(move |w| {
            w.append(if r.distinct { "DISTINCT " } else { "" }).list(self, &r.args)?;
            Ok(())
        })
    }

    /// Pure's hashCode, a signed 64-bit integer: the first 64 bits of the MD5 of the values' text as a JSON array,
    /// read signed (Java's `hashSigned`).
    pub(super) fn hash_signed_spelled(&self, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
        let digest = digests::md5(pc(move |w| {
            w.append("json_array(");
            w.join(a, ", ", |v, x| {
                v.append("CAST(").expr(self, x, 0)?.append(" AS TEXT)");
                Ok(())
            })?;
            w.append(")");
            Ok(())
        }));
        let mut bits = String::new();
        for i in 0..16 {
            bits.push_str(if i == 0 { "" } else { " | " });
            bits.push_str(&format!(
                "((instr('0123456789abcdef', substr(__h.d, {}, 1)) - 1) << {})",
                i + 1,
                60 - 4 * i
            ));
        }
        w.append(&format!("(SELECT {bits} FROM (SELECT "));
        digest(w)?;
        w.append(" AS d) AS __h)");
        Ok(())
    }

    /// Pure round is half-EVEN: the value scaled, rounded half away from zero, and a tie that rounded to an odd
    /// number taken back one toward zero (Java's `roundHalfEven`).
    pub(super) fn round_half_even_spelled(&self, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
        let scale: P<'_> = if a.len() > 1 {
            pc(move |w| {
                w.append("pow(10, ").expr(self, &a[1], 0)?.append(")");
                Ok(())
            })
        } else {
            super::text("1")
        };
        let s1 = scale.clone();
        let scaled = pc(move |w: &mut SqlWriter| {
            w.append("(").expr(self, arg(a, 0)?, 6)?.append(" * ");
            s1(w)?;
            w.append(")");
            Ok(())
        });
        let sc = scaled.clone();
        let r = pc(move |w: &mut SqlWriter| {
            w.append("round(");
            sc(w)?;
            w.append(")");
            Ok(())
        });
        w.append("((CASE WHEN abs(");
        scaled(w)?;
        w.append(" - trunc(");
        scaled(w)?;
        w.append(")) = 0.5 AND CAST(");
        r(w)?;
        w.append(" AS INTEGER) % 2 <> 0 THEN ");
        r(w)?;
        w.append(" - sign(");
        scaled(w)?;
        w.append(") ELSE ");
        r(w)?;
        w.append(" END) / ");
        scale(w)?;
        w.append(")");
        Ok(())
    }

    /// 64-bit integer bit operators; no XOR operator (`(a | b) - (a & b)`); `>>` is arithmetic (Java's `bitOp`).
    pub(super) fn bit_op_spelled(&self, w: &mut SqlWriter, f: SqlFn, a: &[SqlExpr]) -> Result<(), Refusal> {
        let x = self.arg_ex(a, 0, 7);
        let y = self.arg_ex(a, 1, 7);
        let binary = |w: &mut SqlWriter, op: &str| -> Result<(), Refusal> {
            w.append("(");
            x(w)?;
            w.append(op);
            y(w)?;
            w.append(")");
            Ok(())
        };
        match f {
            SqlFn::BitAnd => binary(w, " & "),
            SqlFn::BitOr => binary(w, " | "),
            SqlFn::BitXor => {
                w.append("((");
                x(w)?;
                w.append(" | ");
                y(w)?;
                w.append(") - (");
                x(w)?;
                w.append(" & ");
                y(w)?;
                w.append("))");
                Ok(())
            }
            SqlFn::BitShiftLeft => binary(w, " << "),
            SqlFn::BitShiftRight => binary(w, " >> "),
            other => Err(Refusal::illegal_state(format!("not a bit op: {other}"))),
        }
    }

    /// Every aggregate, as SQLite (and its driver's statistics extension) spells it (Java's `reducer`).
    pub(super) fn reducer_spelled(&self, w: &mut SqlWriter, r: &Reducer) -> Result<(), Refusal> {
        match r.r#fn {
            AggFn::Sum => ansi::aggregate(self, w, "SUM", r),
            AggFn::Count => ansi::aggregate(self, w, "COUNT", r),
            AggFn::Avg => ansi::aggregate(self, w, "AVG", r),
            AggFn::Min => ansi::aggregate(self, w, "MIN", r),
            AggFn::Max => ansi::aggregate(self, w, "MAX", r),
            // any non-NULL value: the least (SQLite has no any_value)
            AggFn::AnyValue => self.wrapped(w, "min(", r),
            // the driver's statistics extension: stdev and variance are the sample ones
            AggFn::StddevSamp | AggFn::Stddev => self.wrapped(w, "stdev(", r),
            AggFn::VarSamp | AggFn::Variance => self.wrapped(w, "variance(", r),
            AggFn::VarPop => self.population_variance(r)?(w),
            AggFn::StddevPop => {
                w.append("sqrt(");
                self.population_variance(r)?(w)?;
                w.append(")");
                Ok(())
            }
            AggFn::CovarPop | AggFn::CovarSamp | AggFn::Corr => self.moments(r)?(w),
            AggFn::BoolAnd => self.wrapped(w, "min(", r),
            AggFn::BoolOr => self.wrapped(w, "max(", r),
            AggFn::StringAgg => {
                w.append("group_concat(");
                self.aggregate_args(r)(w)?;
                self.aggregate_order(w, &r.order_by)?;
                w.append(")");
                Ok(())
            }
            AggFn::List => {
                if r.args.len() != 1 {
                    return Err(Refusal::capability(format!(
                        "a LIST aggregate of {} arguments reached SQLite",
                        r.args.len()
                    )));
                }
                w.append("json_group_array(").append(if r.distinct { "DISTINCT " } else { "" });
                self.json(&r.args[0])(w)?;
                self.aggregate_order(w, &r.order_by)?;
                w.append(")");
                Ok(())
            }
            AggFn::Median => {
                if r.args.len() != 1 || r.distinct {
                    return Err(Refusal::capability("a median of this shape reached SQLite"));
                }
                self.quantile(AggFn::QuantileCont, &r.args[0], super::text("0.5"), r)(w)
            }
            AggFn::QuantileCont | AggFn::QuantileDisc => {
                if r.args.len() != 2 || r.distinct || r.order_by.len() > 1 {
                    return Err(Refusal::capability(format!("{} of this shape reached SQLite", r.r#fn)));
                }
                self.quantile(r.r#fn, &r.args[0], self.ex(&r.args[1], 0), r)(w)
            }
            // the most frequent non-NULL value, the least among ties
            AggFn::Mode => {
                w.append("(SELECT __m.value FROM json_each(json_group_array(");
                self.json(arg(&r.args, 0)?)(w)?;
                w.append(
                    ")) AS __m WHERE __m.value IS NOT NULL GROUP BY __m.value ORDER BY count(*) DESC, min(__m.key) \
                     LIMIT 1)",
                );
                Ok(())
            }
            // the value at the key's greatest (least) row, rows of a NULL key ignored
            AggFn::ArgMax => self.arg_extreme(w, r, " DESC"),
            AggFn::ArgMin => self.arg_extreme(w, r, " ASC"),
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

    /// `name([DISTINCT ]args)`.
    fn wrapped(&self, w: &mut SqlWriter, head: &str, r: &Reducer) -> Result<(), Refusal> {
        w.append(head);
        self.aggregate_args(r)(w)?;
        w.append(")");
        Ok(())
    }

    /// ` ORDER BY <keys>`, each an aggregate's own order key, when there are any.
    fn aggregate_order(&self, w: &mut SqlWriter, keys: &[SortKey]) -> Result<(), Refusal> {
        if !keys.is_empty() {
            w.append(" ORDER BY ");
            w.join(keys, ", ", |w, k| ansi::aggregate_order_key(self, w, k))?;
        }
        Ok(())
    }

    /// The population variance from the sample one: NULL over no value, 0 over one (Java's `populationVariance`).
    fn population_variance<'a>(&'a self, r: &'a Reducer) -> Result<P<'a>, Refusal> {
        let x = arg(&r.args, 0)?;
        Ok(pc(move |w| {
            w.append("(CASE count(").expr(self, x, 0)?.append(") WHEN 0 THEN NULL WHEN 1 THEN 0.0 ELSE variance(");
            w.expr(self, x, 0)?.append(") * (count(").expr(self, x, 0)?.append(") - 1) / count(").expr(self, x, 0)?;
            w.append(") END)");
            Ok(())
        }))
    }

    /// The value at the first row in the key's `direction`, rows of a NULL key ignored (Java's `argExtreme`).
    fn arg_extreme(&self, w: &mut SqlWriter, r: &Reducer, direction: &str) -> Result<(), Refusal> {
        if r.args.len() != 2 || r.distinct || !r.order_by.is_empty() {
            return Err(Refusal::capability(format!("{} of this shape reached SQLite", r.r#fn)));
        }
        let (v, k) = (&r.args[0], &r.args[1]);
        w.append("((json_group_array(");
        self.json(v)(w)?;
        w.append(" ORDER BY ").expr(self, k, 0)?.append(direction).append(") FILTER (WHERE ").expr(self, k, 4)?;
        w.append(" IS NOT NULL)) ->> 0)");
        Ok(())
    }

    /// Covariance and correlation over the pairs with both values: their sums (Java's `moments`).
    fn moments<'a>(&'a self, r: &'a Reducer) -> Result<P<'a>, Refusal> {
        let y = arg(&r.args, 0)?;
        let x = arg(&r.args, 1)?;
        let n = pc(move |w: &mut SqlWriter| {
            w.append("count(").expr(self, x, 6)?.append(" * ").expr(self, y, 6)?.append(")");
            Ok(())
        });
        let sxy = pc(move |w: &mut SqlWriter| {
            w.append("sum(").expr(self, x, 6)?.append(" * ").expr(self, y, 6)?.append(")");
            Ok(())
        });
        let sx = pc(move |w: &mut SqlWriter| {
            w.append("sum(").expr(self, x, 0)?.append(") FILTER (WHERE ").expr(self, y, 4)?.append(" IS NOT NULL)");
            Ok(())
        });
        let sy = pc(move |w: &mut SqlWriter| {
            w.append("sum(").expr(self, y, 0)?.append(") FILTER (WHERE ").expr(self, x, 4)?.append(" IS NOT NULL)");
            Ok(())
        });
        let sxx = pc(move |w: &mut SqlWriter| {
            w.append("sum(").expr(self, x, 6)?.append(" * ").expr(self, x, 6)?.append(") FILTER (WHERE ");
            w.expr(self, y, 4)?.append(" IS NOT NULL)");
            Ok(())
        });
        let syy = pc(move |w: &mut SqlWriter| {
            w.append("sum(").expr(self, y, 6)?.append(" * ").expr(self, y, 6)?.append(") FILTER (WHERE ");
            w.expr(self, x, 4)?.append(" IS NOT NULL)");
            Ok(())
        });
        let (n1, sx1, sy1) = (n.clone(), sx.clone(), sy.clone());
        let cxy = pc(move |w: &mut SqlWriter| {
            w.append("(");
            sxy(w)?;
            w.append(" - CAST(");
            sx1(w)?;
            w.append(" AS REAL) * ");
            sy1(w)?;
            w.append(" / ");
            n1(w)?;
            w.append(")");
            Ok(())
        });
        Ok(match r.r#fn {
            AggFn::CovarPop => pc(move |w| {
                w.append("(CASE WHEN ");
                n(w)?;
                w.append(" > 0 THEN ");
                cxy(w)?;
                w.append(" / ");
                n(w)?;
                w.append(" END)");
                Ok(())
            }),
            AggFn::CovarSamp => pc(move |w| {
                w.append("(CASE WHEN ");
                n(w)?;
                w.append(" > 1 THEN ");
                cxy(w)?;
                w.append(" / (");
                n(w)?;
                w.append(" - 1) END)");
                Ok(())
            }),
            _ => pc(move |w| {
                w.append("(CASE WHEN ");
                n(w)?;
                w.append(" > 1 THEN ");
                cxy(w)?;
                w.append(" / sqrt((");
                sxx(w)?;
                w.append(" - CAST(");
                sx(w)?;
                w.append(" AS REAL) * ");
                sx(w)?;
                w.append(" / ");
                n(w)?;
                w.append(") * (");
                syy(w)?;
                w.append(" - CAST(");
                sy(w)?;
                w.append(" AS REAL) * ");
                sy(w)?;
                w.append(" / ");
                n(w)?;
                w.append(")) END)");
                Ok(())
            }),
        })
    }

    /// A grouped quantile: the group's non-NULL values sorted and indexed (Java's `quantile`).
    fn quantile<'a>(&'a self, f: AggFn, x: &'a SqlExpr, q: P<'a>, r: &'a Reducer) -> P<'a> {
        let desc = r.order_by.first().is_some_and(|k| !k.ascending);
        let t = type_of(x);
        let text = scalar_is(t, Scalar::Varchar) || scalar_is(t, Scalar::Date) || scalar_is(t, Scalar::Timestamp);
        let held: P<'a> = if text {
            self.ex(x, 0)
        } else {
            pc(move |w| {
                w.append("json(printf('%!.17g', ").expr(self, x, 0)?.append("))");
                Ok(())
            })
        };
        let sorted = pc(move |w: &mut SqlWriter| {
            w.append("json_group_array(");
            held(w)?;
            w.append(" ORDER BY ").expr(self, x, 0)?.append(if desc { " DESC)" } else { " ASC)" });
            w.append(" FILTER (WHERE ").expr(self, x, 4)?.append(" IS NOT NULL)");
            Ok(())
        });
        let n = pc(move |w: &mut SqlWriter| {
            w.append("count(").expr(self, x, 0)?.append(")");
            Ok(())
        });
        let at = pc(move |w: &mut SqlWriter| {
            w.append("json_extract(");
            sorted(w)?;
            w.append(", '$[' || ");
            Ok(())
        });
        if f == AggFn::QuantileDisc {
            return pc(move |w| {
                at(w)?;
                w.append("max(CAST(ceil(");
                n(w)?;
                w.append(" * (");
                q(w)?;
                w.append(")) AS INTEGER) - 1, 0) || ']')");
                Ok(())
            });
        }
        let pos = pc(move |w: &mut SqlWriter| {
            w.append("((");
            q(w)?;
            w.append(") * (");
            n(w)?;
            w.append(" - 1))");
            Ok(())
        });
        let (at1, pos1) = (at.clone(), pos.clone());
        let lo = pc(move |w: &mut SqlWriter| {
            at1(w)?;
            w.append("CAST(floor(");
            pos1(w)?;
            w.append(") AS INTEGER) || ']')");
            Ok(())
        });
        let pos2 = pos.clone();
        let hi = pc(move |w: &mut SqlWriter| {
            at(w)?;
            w.append("CAST(ceil(");
            pos2(w)?;
            w.append(") AS INTEGER) || ']')");
            Ok(())
        });
        pc(move |w| {
            w.append("(");
            lo(w)?;
            w.append(" + (");
            hi(w)?;
            w.append(" - ");
            lo(w)?;
            w.append(") * (");
            pos(w)?;
            w.append(" - floor(");
            pos(w)?;
            w.append(")))");
            Ok(())
        })
    }

    /// A window function: SQLite's are the standard ones; a windowed moment is its sums over the window; an
    /// aggregate's own ORDER BY over a whole partition is the window's order over every row of it (Java's
    /// `windowCall`).
    pub(super) fn window_call_spelled(
        &self,
        w: &mut SqlWriter,
        f: &SqlAgg,
        partition_by: &[SqlExpr],
        order_by: &[SortKey],
        frame: Option<&Frame>,
    ) -> Result<(), Refusal> {
        match f {
            SqlAgg::Reducer(r) if ORDERED_SETS.contains(&r.r#fn) => {
                return Err(Refusal::capability(format!(
                    "{} over an ordered or framed window reached SQLite",
                    r.r#fn
                )));
            }
            SqlAgg::Reducer(r) if MOMENTS.contains(&r.r#fn) => {
                return self.window_moments(w, r, partition_by, order_by, frame);
            }
            SqlAgg::Reducer(r) if matches!(r.r#fn, AggFn::ArgMax | AggFn::ArgMin | AggFn::Mode) => {
                return Err(Refusal::capability(format!("a windowed {} reached SQLite", r.r#fn)));
            }
            SqlAgg::Reducer(r) if !r.order_by.is_empty() => {
                if !order_by.is_empty() || frame.is_some() {
                    return Err(Refusal::capability(format!(
                        "an ordered {} over an ordered or framed window reached SQLite",
                        r.r#fn
                    )));
                }
                let plain = build::reducer(r.r#fn, r.args.clone(), r.distinct, Vec::new());
                self.reducer(w, &plain)?;
                let whole = Frame { kind: FrameKind::Rows, from: Bound::UnboundedPreceding, to: Bound::UnboundedFollowing };
                return self.window_over(w, partition_by, &r.order_by, Some(&whole));
            }
            SqlAgg::Reducer(r) => self.reducer(w, r)?,
            SqlAgg::RankingFn { r#fn, args } | SqlAgg::ValueFn { r#fn, args } => {
                let name = self.aggregate_name(*r#fn);
                w.append(&name).append("(").list(self, args)?.append(")");
            }
        }
        self.window_over(w, partition_by, order_by, frame)
    }

    /// A windowed variance, deviation, covariance or correlation from its sums over the window (Java's
    /// `windowMoments`).
    fn window_moments<'a>(
        &'a self,
        w: &mut SqlWriter,
        r: &'a Reducer,
        partition_by: &'a [SqlExpr],
        order_by: &'a [SortKey],
        frame: Option<&'a Frame>,
    ) -> Result<(), Refusal> {
        let pair = r.args.len() == 2;
        let y = arg(&r.args, 0)?;
        let x = if pair { &r.args[1] } else { y };
        let window = Window { partition_by, order_by, frame };
        let both = pc(move |v: &mut SqlWriter| {
            v.append("(CASE WHEN ").expr(self, x, 4)?.append(" IS NOT NULL AND ").expr(self, y, 4)?;
            v.append(" IS NOT NULL THEN 1 END)");
            Ok(())
        });
        let n = self.summed(window, both.clone());
        let sx = self.summed(window, self.real_times(x, None, both.clone()));
        let sy = self.summed(window, self.real_times(y, None, both.clone()));
        let sxy = self.summed(window, self.real_times(x, Some(y), both.clone()));
        let sxx = self.summed(window, self.real_times(x, Some(x), both.clone()));
        let syy = self.summed(window, self.real_times(y, Some(y), both));
        let (n1, sx1, sy1) = (n.clone(), sx.clone(), sy.clone());
        let cxy = pc(move |v: &mut SqlWriter| {
            v.append("(");
            sxy(v)?;
            v.append(" - ");
            sx1(v)?;
            v.append(" * ");
            sy1(v)?;
            v.append(" / ");
            n1(v)?;
            v.append(")");
            Ok(())
        });
        let cxx = square(sx.clone(), sxx, n.clone());
        let cyy = square(sy.clone(), syy, n.clone());
        let (head, body, tail): (&str, P<'_>, &str) = match r.r#fn {
            AggFn::VarPop => (" > 0 THEN ", cxx.clone(), " / "),
            AggFn::StddevPop => (" > 0 THEN sqrt(", cxx.clone(), " / "),
            AggFn::VarSamp | AggFn::Variance => (" > 1 THEN ", cxx.clone(), " / ("),
            AggFn::StddevSamp | AggFn::Stddev => (" > 1 THEN sqrt(", cxx.clone(), " / ("),
            AggFn::CovarPop => (" > 0 THEN ", cxy.clone(), " / "),
            AggFn::CovarSamp => (" > 1 THEN ", cxy.clone(), " / ("),
            _ => {
                w.append("(CASE WHEN ");
                n(w)?;
                w.append(" > 1 THEN ");
                cxy(w)?;
                w.append(" / sqrt(");
                cxx(w)?;
                w.append(" * ");
                cyy(w)?;
                w.append(") END)");
                return Ok(());
            }
        };
        w.append("(CASE WHEN ");
        n(w)?;
        w.append(head);
        body(w)?;
        w.append(tail);
        n(w)?;
        w.append(match r.r#fn {
            AggFn::VarPop | AggFn::CovarPop => " END)",
            AggFn::StddevPop => ") END)",
            AggFn::VarSamp | AggFn::Variance | AggFn::CovarSamp => " - 1) END)",
            _ => " - 1)) END)",
        });
        Ok(())
    }

    /// `sum(arg) OVER (...)` (Java's `over` in `windowMoments`).
    fn summed<'a>(&'a self, window: Window<'a>, arg: P<'a>) -> P<'a> {
        pc(move |v| {
            v.append("sum(");
            arg(v)?;
            v.append(")");
            self.window_over(v, window.partition_by, window.order_by, window.frame)
        })
    }

    /// `CAST(a AS REAL) * [b * ]both`.
    fn real_times<'a>(&'a self, a: &'a SqlExpr, b: Option<&'a SqlExpr>, both: P<'a>) -> P<'a> {
        pc(move |v: &mut SqlWriter| {
            v.append("CAST(").expr(self, a, 0)?.append(" AS REAL) * ");
            if let Some(b) = b {
                v.expr(self, b, 6)?.append(" * ");
            }
            both(v)
        })
    }

    /// A window's `OVER (...)`. A RANGE frame with an offset takes ONE numeric key: a date or date-time key is its
    /// Julian day number (Java's `windowOver`).
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
            if interval {
                if order_by.len() != 1 {
                    return Err(Refusal::capability(format!(
                        "a RANGE frame over {} keys reached SQLite",
                        order_by.len()
                    )));
                }
                let k = &order_by[0];
                let kt = type_of(&k.expr);
                if !scalar_is(kt, Scalar::Date) && !scalar_is(kt, Scalar::Timestamp) {
                    return Err(Refusal::capability(format!(
                        "an interval RANGE frame over a {} key reached SQLite",
                        kt.map_or_else(|| "null".to_owned(), sql_type_text)
                    )));
                }
                days = true;
                let nulls_first = match k.null_order {
                    Some(o) => o == NullOrder::NullsFirst,
                    None => !k.ascending,
                };
                w.append("julianday(").expr(self, &k.expr, 0)?.append(")").append(if k.ascending { "" } else { " DESC" });
                w.append(if nulls_first { " NULLS FIRST" } else { " NULLS LAST" });
            } else {
                w.join(order_by, ", ", |w, k| self.sort_key(w, k))?;
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

    fn frame_bound(&self, b: &Bound, days: bool) -> Result<String, Refusal> {
        match b {
            Bound::IntervalPreceding { n, unit } => Ok(format!("{} PRECEDING", interval_days(*n, unit, days)?)),
            Bound::IntervalFollowing { n, unit } => Ok(format!("{} FOLLOWING", interval_days(*n, unit, days)?)),
            other => self.bound(other),
        }
    }
}
