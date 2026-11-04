// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! Snowflake's aggregates and windows (Snowflake.java, "Hooks of the base" to "Aggregates and windows"): banker's
//! ROUND over a NUMBER (and in binary over a FLOAT), the bit operators, every aggregate -- a Float aggregate's
//! argument a DOUBLE, LISTAGG guarded over no row, the percentiles WITHIN GROUP, ARRAY_AGG keeping a NULL -- and the
//! windows: a frame only with an ORDER BY, a ranking window ordered by NULL, FIRST/LAST/NTH_VALUE under a cumulative
//! frame unrolled, an ordered string aggregate its ordered ARRAY_AGG joined, a windowed continuous quantile computed
//! over the partition's sorted values, a RANGE frame of peers over a temporal key the zero interval.

use super::{Snowflake, is_double, is_integer, keep_null, type_of};
use crate::ansi;
use crate::pieces::{P, pc};
use crate::refusal::{Refusal, arg};
use crate::renderer::AnsiSqlRenderer;
use crate::writer::SqlWriter;
use lite_sql::{AggFn, Bound, Frame, FrameKind, Reducer, Scalar, SortKey, SqlAgg, SqlExpr, SqlFn, SqlType};

/// BITAND and friends over NUMBER(38,0) (Java's `bitOp`).
pub(super) fn bit_op(d: &Snowflake, w: &mut SqlWriter, f: SqlFn, a: &[SqlExpr]) -> Result<(), Refusal> {
    let name = match f {
        SqlFn::BitAnd => "BITAND",
        SqlFn::BitOr => "BITOR",
        SqlFn::BitXor => "BITXOR",
        SqlFn::BitShiftLeft => "BITSHIFTLEFT",
        SqlFn::BitShiftRight => "BITSHIFTRIGHT",
        other => return Err(Refusal::illegal_state(format!("not a bit op: {other}"))),
    };
    w.function(d, name, a)?;
    Ok(())
}

/// `writer`, for an aggregate Snowflake cannot order: an ordered one is refused (Java's `unordered`).
fn unordered(r: &Reducer) -> Result<(), Refusal> {
    if !r.order_by.is_empty() {
        return Err(Refusal::capability(format!("an ordered {} reached Snowflake", r.r#fn)));
    }
    Ok(())
}

/// A window's partition, order and (normalized) frame (Java's `Window`).
#[derive(Clone)]
struct Window<'a> {
    partition_by: Vec<SqlExpr>,
    order_by: &'a [SortKey],
    frame: Option<Frame>,
}

impl<'a> Window<'a> {
    fn with_frame(&self, f: Option<Frame>) -> Window<'a> {
        Window { partition_by: self.partition_by.clone(), order_by: self.order_by, frame: f }
    }
}

/// Whether the frame runs from the partition's start to the current row: cumulative (Java's `cumulative`).
fn cumulative(win: &Window<'_>) -> bool {
    !win.order_by.is_empty()
        && match &win.frame {
            None => true,
            Some(f) => f.from == Bound::UnboundedPreceding && f.to == Bound::CurrentRow,
        }
}

fn rows_frame(win: &Window<'_>) -> bool {
    matches!(&win.frame, Some(f) if f.kind == FrameKind::Rows)
}

/// The whole partition, as ROWS (Java's `WHOLE`).
fn whole() -> Frame {
    Frame { kind: FrameKind::Rows, from: Bound::UnboundedPreceding, to: Bound::UnboundedFollowing }
}

/// A date or date-time key (Java's `temporal`).
fn temporal(e: &SqlExpr) -> bool {
    matches!(type_of(e), Some(SqlType::Scalar(Scalar::Date | Scalar::Timestamp | Scalar::Timestamptz)))
}

/// The aggregates no ordered or framed window takes (Java's `UNORDERED_ONLY`).
fn unordered_only(f: AggFn) -> bool {
    matches!(f, AggFn::Median | AggFn::Mode | AggFn::QuantileCont | AggFn::QuantileDisc)
}

impl Snowflake {
    /// Pure round is half-EVEN: ROUND(x, s, 'HALF_TO_EVEN') over a NUMBER; a FLOAT's scaled value to the even
    /// neighbour when exactly halfway, else to the nearest, in binary (Java's `roundHalfEven`).
    pub(super) fn round_half_even_arm(&self, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
        if !is_double(arg(a, 0)?) {
            w.append("ROUND(").expr(self, &a[0], 0)?.append(", ");
            if a.len() > 1 {
                w.expr(self, &a[1], 0)?;
            } else {
                w.append("0");
            }
            w.append(", 'HALF_TO_EVEN')");
            return Ok(());
        }
        let (v, scale) = self.scaled(a);
        w.append("((CASE WHEN ABS(").piece(&*v)?.append(" - TRUNC(").piece(&*v)?.append(")) = 0.5 THEN 2 * ROUND(");
        w.piece(&*v)?.append(" / 2) ELSE SIGN(").piece(&*v)?.append(") * FLOOR(ABS(").piece(&*v)?;
        w.append(") + 0.5) END)");
        if let Some(scale) = &scale {
            w.append(" / ").piece(&**scale)?;
        }
        w.append(")");
        Ok(())
    }

    /// A Float aggregate's arguments, each a DOUBLE (Java's `doubleArgs`).
    fn double_args(&self, r: &Reducer) -> Reducer {
        let args = r
            .args
            .iter()
            .map(|x| {
                if is_double(x) { x.clone() } else { crate::build::cast(x.clone(), SqlType::Scalar(Scalar::Double)) }
            })
            .collect();
        crate::build::reducer(r.r#fn, args, r.distinct, r.order_by.clone())
    }

    /// Every aggregate, as Snowflake names it (Java's `reducer`).
    pub(super) fn reducer_arm(&self, w: &mut SqlWriter, r: &Reducer) -> Result<(), Refusal> {
        let plain = |w: &mut SqlWriter, name: &str| -> Result<(), Refusal> {
            unordered(r)?;
            ansi::aggregate(self, w, name, r)
        };
        let moment = |w: &mut SqlWriter, name: &str| -> Result<(), Refusal> {
            unordered(r)?;
            ansi::aggregate(self, w, name, &self.double_args(r))
        };
        match r.r#fn {
            AggFn::Sum => plain(w, "SUM"),
            AggFn::Count => plain(w, "COUNT"),
            AggFn::Min => plain(w, "MIN"),
            AggFn::Max => plain(w, "MAX"),
            AggFn::AnyValue => plain(w, "ANY_VALUE"),
            AggFn::Avg => {
                unordered(r)?;
                self.avg(w, r)
            }
            AggFn::StddevSamp => moment(w, "STDDEV_SAMP"),
            AggFn::StddevPop => moment(w, "STDDEV_POP"),
            AggFn::VarSamp => moment(w, "VAR_SAMP"),
            AggFn::VarPop => moment(w, "VAR_POP"),
            AggFn::Corr => moment(w, "CORR"),
            AggFn::CovarSamp => moment(w, "COVAR_SAMP"),
            AggFn::CovarPop => moment(w, "COVAR_POP"),
            AggFn::Variance => moment(w, "VARIANCE"),
            AggFn::Stddev => moment(w, "STDDEV"),
            AggFn::BoolAnd | AggFn::BoolOr => {
                unordered(r)?;
                w.append(if r.r#fn == AggFn::BoolAnd { "BOOLAND_AGG(" } else { "BOOLOR_AGG(" });
                w.list(self, &r.args)?.append(")");
                Ok(())
            }
            // LISTAGG answers '' over no row; DuckDB's string_agg NULL
            AggFn::StringAgg => {
                w.append("(CASE WHEN COUNT(").expr(self, arg(&r.args, 0)?, 0)?.append(") = 0 THEN NULL ELSE ");
                w.piece(&*self.listagg(r))?.append(" END)");
                Ok(())
            }
            // DuckDB's median interpolates numbers in DOUBLE (an integer's median is a DOUBLE)
            AggFn::Median => {
                unordered(r)?;
                w.append("MEDIAN(").piece(&*self.numeric_arg(arg(&r.args, 0)?))?.append(")");
                Ok(())
            }
            AggFn::Mode => {
                unordered(r)?;
                w.append("MODE(").expr(self, arg(&r.args, 0)?, 0)?.append(")");
                Ok(())
            }
            AggFn::QuantileCont => self.percentile(w, "PERCENTILE_CONT", r, true),
            AggFn::QuantileDisc => self.percentile(w, "PERCENTILE_DISC", r, false),
            // a NULL value kept, in the keys' order
            AggFn::List => self.array_agg(w, r),
            // the value at the key's greatest (least) row, rows of a NULL key ignored
            AggFn::ArgMax | AggFn::ArgMin => {
                unordered(r)?;
                if r.args.len() != 2 || r.distinct {
                    return Err(Refusal::capability(format!("{} of this shape reached Snowflake", r.r#fn)));
                }
                w.append(if r.r#fn == AggFn::ArgMax { "MAX_BY(" } else { "MIN_BY(" }).list(self, &r.args)?.append(")");
                Ok(())
            }
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

    /// The `AVG` arm: a DOUBLE, a Decimal's average cast to one (Java's `avg`).
    fn avg(&self, w: &mut SqlWriter, r: &Reducer) -> Result<(), Refusal> {
        if r.args.len() == 1 && matches!(type_of(&r.args[0]), Some(SqlType::Decimal { .. })) {
            w.append("CAST(");
            ansi::aggregate(self, w, "AVG", r)?;
            w.append(" AS DOUBLE)");
            return Ok(());
        }
        ansi::aggregate(self, w, "AVG", &self.double_args(r))
    }

    /// `name(level) WITHIN GROUP (ORDER BY value[ DESC])`, the value a number when `numeric` (Java's `percentile`).
    fn percentile(&self, w: &mut SqlWriter, name: &str, r: &Reducer, numeric: bool) -> Result<(), Refusal> {
        if r.args.len() != 2 || r.distinct || r.order_by.len() > 1 {
            return Err(Refusal::capability(format!("{} of this shape reached Snowflake", r.r#fn)));
        }
        let desc = r.order_by.first().is_some_and(|k| !k.ascending);
        w.append(name).append("(").expr(self, &r.args[1], 0)?.append(") WITHIN GROUP (ORDER BY ");
        if numeric {
            w.piece(&*self.numeric_arg(&r.args[0]))?;
        } else {
            w.expr(self, &r.args[0], 0)?;
        }
        w.append(if desc { " DESC" } else { "" }).append(")");
        Ok(())
    }

    /// The `LIST` arm: ARRAY_AGG keeping a NULL, in the keys' order (Java's `arrayAgg`).
    fn array_agg(&self, w: &mut SqlWriter, r: &Reducer) -> Result<(), Refusal> {
        if r.args.len() != 1 {
            return Err(Refusal::capability(format!(
                "a LIST aggregate of {} arguments reached Snowflake",
                r.args.len()
            )));
        }
        w.append("ARRAY_AGG(").append(if r.distinct { "DISTINCT " } else { "" }).piece(&*keep_null(self, &r.args[0]))?;
        w.append(")");
        if !r.order_by.is_empty() {
            w.append(" WITHIN GROUP (ORDER BY ").join(&r.order_by, ", ", |w, k| ansi::aggregate_order_key(self, w, k))?;
            w.append(")");
        }
        Ok(())
    }

    /// An integer read as a DOUBLE (Java's `numericArg`).
    fn numeric_arg<'a>(&'a self, x: &'a SqlExpr) -> P<'a> {
        if is_integer(type_of(x)) {
            return pc(move |w| {
                w.append("CAST(").expr(self, x, 0)?.append(" AS DOUBLE)");
                Ok(())
            });
        }
        crate::pieces::Ex::ex(self, x, 0)
    }

    /// `LISTAGG([DISTINCT ]args)[ WITHIN GROUP (ORDER BY keys)]` (Java's `listagg`).
    fn listagg<'a>(&'a self, r: &'a Reducer) -> P<'a> {
        pc(move |w| {
            w.append("LISTAGG(").append(if r.distinct { "DISTINCT " } else { "" }).list(self, &r.args)?.append(")");
            if !r.order_by.is_empty() {
                w.append(" WITHIN GROUP (ORDER BY ");
                w.join(&r.order_by, ", ", |w, k| ansi::aggregate_order_key(self, w, k))?.append(")");
            }
            Ok(())
        })
    }

    /// Windowed, as DuckDB windows (Java's `windowCall`): a frame needs an ORDER BY (a whole-partition frame without
    /// one is no frame); a ranking window with no order is ordered by NULL.
    pub(super) fn window_call_arm(
        &self,
        w: &mut SqlWriter,
        f: &SqlAgg,
        partition_by: &[SqlExpr],
        order_by: &[SortKey],
        frame: Option<&Frame>,
    ) -> Result<(), Refusal> {
        let mut frame = frame.cloned();
        if let Some(fr) = &frame
            && order_by.is_empty()
        {
            if !(fr.from == Bound::UnboundedPreceding && fr.to == Bound::UnboundedFollowing) {
                return Err(Refusal::capability(
                    "a window frame without an ORDER BY reached Snowflake (a frame needs one, measured)",
                ));
            }
            frame = None;
        }
        let win = Window { partition_by: partition_by.to_vec(), order_by, frame };
        match f {
            SqlAgg::Reducer(r) => self.windowed_reducer(w, r, &win),
            // a ranking function takes no frame (ANSI ignores one; Snowflake refuses it)
            SqlAgg::RankingFn { r#fn, args } => {
                let name = self.aggregate_name(*r#fn);
                w.append(&name).append("(").list(self, args)?.append(")");
                if win.order_by.is_empty() {
                    // Snowflake refuses a ranking window with no order: a constant order numbers the rows as they come
                    w.append(" OVER (");
                    if !win.partition_by.is_empty() {
                        w.append("PARTITION BY ").list(self, &win.partition_by)?.append(" ");
                    }
                    w.append("ORDER BY NULL)");
                    return Ok(());
                }
                self.over(w, &win.with_frame(None))
            }
            SqlAgg::ValueFn { r#fn, args } => self.value_fn(w, *r#fn, args, &win),
        }
    }

    /// FIRST/LAST/NTH_VALUE under a cumulative frame (refused there) unrolled over the whole partition (Java's
    /// `valueFn`).
    fn value_fn(&self, w: &mut SqlWriter, f: AggFn, args: &[SqlExpr], win: &Window<'_>) -> Result<(), Refusal> {
        let value = matches!(f, AggFn::FirstValue | AggFn::LastValue | AggFn::NthValue);
        if !value || !cumulative(win) {
            let name = self.aggregate_name(f);
            w.append(&name).append("(").list(self, args)?.append(")");
            return self.over(w, win);
        }
        let whole_win = win.with_frame(Some(whole()));
        if f == AggFn::FirstValue {
            w.append("FIRST_VALUE(").list(self, args)?.append(")");
            return self.over(w, &whole_win);
        }
        if f == AggFn::LastValue {
            if rows_frame(win) {
                w.append("(").expr(self, arg(args, 0)?, 0)?.append(")");
                return Ok(());
            }
            let mut peers = win.partition_by.clone();
            peers.extend(win.order_by.iter().map(|k| k.expr.clone()));
            w.append("LAST_VALUE(").list(self, args)?.append(")");
            return self.over(w, &Window { partition_by: peers, order_by: win.order_by, frame: Some(whole()) });
        }
        if args.len() != 2 {
            return Err(Refusal::capability(format!("NTH_VALUE of {} arguments reached Snowflake", args.len())));
        }
        w.append("(CASE WHEN COUNT(*)");
        let counted = if win.frame.is_some() {
            win.clone()
        } else {
            let cumulative = Frame { kind: FrameKind::Range, from: Bound::UnboundedPreceding, to: Bound::CurrentRow };
            win.with_frame(Some(cumulative))
        };
        self.over(w, &counted)?;
        w.append(" >= ").expr(self, &args[1], 0)?.append(" THEN NTH_VALUE(").list(self, args)?.append(")");
        self.over(w, &whole_win)?;
        w.append(" END)");
        Ok(())
    }

    /// A reducer over a window (Java's `windowedReducer`).
    fn windowed_reducer(&self, w: &mut SqlWriter, r: &Reducer, win: &Window<'_>) -> Result<(), Refusal> {
        let ordered = !win.order_by.is_empty();
        let string_or_list = matches!(r.r#fn, AggFn::StringAgg | AggFn::List);
        // over a whole-partition frame, the aggregate in the window's order is the partition's ordered aggregate
        if string_or_list
            && ordered
            && let Some(fr) = &win.frame
            && fr.from == Bound::UnboundedPreceding
            && fr.to == Bound::UnboundedFollowing
        {
            let within = if r.order_by.is_empty() {
                Reducer {
                    r#fn: r.r#fn,
                    args: r.args.clone(),
                    distinct: r.distinct,
                    order_by: win.order_by.to_vec(),
                    r#type: r.r#type.clone(),
                }
            } else {
                r.clone()
            };
            return self.windowed_reducer(
                w,
                &within,
                &Window { partition_by: win.partition_by.clone(), order_by: &[], frame: None },
            );
        }
        if unordered_only(r.r#fn) && (ordered || win.frame.is_some()) {
            return Err(Refusal::capability(format!(
                "{} over an ordered or framed window reached Snowflake",
                r.r#fn
            )));
        }
        if r.r#fn == AggFn::QuantileCont {
            let p = self.window_quantile(r, win)?;
            w.piece(&*p)?;
            return Ok(());
        }
        if string_or_list && ordered {
            if !r.order_by.is_empty() && r.order_by.as_slice() != win.order_by {
                return Err(Refusal::capability(format!(
                    "an ordered {} over a window of another order reached Snowflake (WITHIN GROUP is refused beside \
                     an OVER's ORDER BY, measured)",
                    r.r#fn
                )));
            }
            if r.distinct {
                return Err(Refusal::capability(format!(
                    "a DISTINCT {} over an ordered window reached Snowflake",
                    r.r#fn
                )));
            }
        }
        if r.r#fn == AggFn::StringAgg {
            w.append("(CASE WHEN COUNT(").expr(self, arg(&r.args, 0)?, 0)?.append(")");
            self.over(w, win)?;
            w.append(" = 0 THEN NULL ELSE ");
            if ordered {
                // the frame's values in the window's order (ARRAY_AGG drops a NULL, as string_agg)
                w.append("ARRAY_TO_STRING(ARRAY_AGG(").expr(self, &r.args[0], 0)?.append(")");
                self.over(w, win)?;
                w.append(", ").expr(self, arg(&r.args, 1)?, 0)?.append(")");
            } else {
                w.piece(&*self.listagg(r))?;
                self.over(w, win)?;
            }
            w.append(" END)");
            return Ok(());
        }
        if r.r#fn == AggFn::List && ordered {
            if r.args.len() != 1 {
                return Err(Refusal::capability(format!(
                    "a LIST aggregate of {} arguments reached Snowflake",
                    r.args.len()
                )));
            }
            w.append("ARRAY_AGG(").piece(&*keep_null(self, &r.args[0]))?.append(")");
            return self.over(w, win);
        }
        if r.r#fn == AggFn::Avg && r.args.len() == 1 && matches!(type_of(&r.args[0]), Some(SqlType::Decimal { .. })) {
            w.append("CAST(");
            ansi::aggregate(self, w, "AVG", r)?;
            self.over(w, win)?;
            w.append(" AS DOUBLE)");
            return Ok(());
        }
        self.reducer(w, r)?;
        self.over(w, win)
    }

    /// A continuous quantile over a window's partition, as DuckDB and the engine interpolate it (Java's
    /// `windowQuantile`): the values and q bound once, in a one-element TRANSFORM.
    fn window_quantile<'a>(&'a self, r: &'a Reducer, win: &Window<'a>) -> Result<P<'a>, Refusal> {
        if r.args.len() != 2 || r.distinct || r.order_by.len() > 1 {
            return Err(Refusal::capability(format!("{} of this shape reached Snowflake", r.r#fn)));
        }
        let ascending = r.order_by.first().is_none_or(|k| k.ascending);
        let o = self.delimited(&self.fresh_name("q"));
        let sorted = format!("CAST(GET({o}, 's') AS ARRAY)");
        let n = format!("ARRAY_SIZE({sorted})");
        let rn = format!("(CAST({n} - 1 AS DOUBLE) * CAST(GET({o}, 'q') AS DOUBLE))");
        let lo = format!("CAST(GET({sorted}, CAST(FLOOR({rn}) AS INTEGER)) AS DOUBLE)");
        let hi = format!("CAST(GET({sorted}, CAST(CEIL({rn}) AS INTEGER)) AS DOUBLE)");
        let win = win.clone();
        Ok(pc(move |w| {
            w.append("CAST(GET(TRANSFORM(ARRAY_CONSTRUCT(OBJECT_CONSTRUCT_KEEP_NULL('s', ARRAY_SORT(ARRAY_COMPACT(");
            w.append("ARRAY_AGG(").expr(self, &r.args[0], 0)?.append(")");
            self.over(w, &win)?;
            w.append(&format!("), {}), 'q', CAST(", if ascending { "TRUE" } else { "FALSE" }));
            w.expr(self, &r.args[1], 0)?;
            w.append(&format!(
                " AS DOUBLE))), {o} OBJECT -> CASE WHEN {n} = 0 THEN NULL ELSE {lo} + ({hi} - {lo}) * ({rn} - \
                 FLOOR({rn})) END), 0) AS DOUBLE)"
            ));
            Ok(())
        }))
    }

    /// A window's `OVER (...)` (Java's `over`): RANGE CURRENT ROW to CURRENT ROW over one date or timestamp key is its
    /// peers, written as the zero-interval frame.
    fn over(&self, w: &mut SqlWriter, win: &Window<'_>) -> Result<(), Refusal> {
        w.append(" OVER (");
        if !win.partition_by.is_empty() {
            w.append("PARTITION BY ").list(self, &win.partition_by)?;
        }
        if !win.order_by.is_empty() {
            w.append(if win.partition_by.is_empty() { "" } else { " " }).append("ORDER BY ");
            w.join(win.order_by, ", ", |w, k| self.sort_key(w, k))?;
        }
        if let Some(f) = &win.frame {
            let peers = f.kind == FrameKind::Range
                && f.from == Bound::CurrentRow
                && f.to == Bound::CurrentRow
                && win.order_by.len() == 1
                && temporal(&win.order_by[0].expr);
            w.append(&format!(" {} BETWEEN ", f.kind));
            if peers {
                let unit = if matches!(type_of(&win.order_by[0].expr), Some(SqlType::Scalar(Scalar::Date))) {
                    "DAYS"
                } else {
                    "SECONDS"
                };
                w.append(&format!("INTERVAL '0 {unit}' PRECEDING AND INTERVAL '0 {unit}' FOLLOWING"));
            } else {
                let text = format!("{} AND {}", self.bound(&f.from)?, self.bound(&f.to)?);
                w.append(&text);
            }
        }
        w.append(")");
        Ok(())
    }
}
