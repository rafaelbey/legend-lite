// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! The two passes only the MariaDB family installs: Java's `WindowAggregatesAsCorrelated` (MariaDB's and MySQL's: a
//! window call of an aggregate the database has no window form of, GROUP_CONCAT's, as the same aggregate over the
//! window's rows, a correlated scalar subquery) and `WindowsComputedBelow` (MySQL's: each window call inside a larger
//! expression computed in a select below, the expression written over its column). No other dialect installs either,
//! so they live with their dialect (as ClickHouse's and BigQuery's own passes do).
//!
//! Each rebuilds through [`crate::build`] (Java's constructors) and answers [`Rw::Same`] exactly where Java hands back
//! its input; the helper columns are numbered per statement (`rewriteRoot` starts the count over).

use crate::build;
use crate::carrier;
use crate::refusal::Refusal;
use crate::rewriter::{Rw, SqlRewriter, rewrite};
use crate::typing::{self, source_alias, source_outputs};
use lite_sql::{
    AggFn, Bound, Frame, FrameKind, JavaNumber, OutputCol, Projection, Scalar, SortKey, SqlAgg, SqlExpr, SqlFn,
    SqlQuery, SqlSelect, SqlSource, SqlType, TypeFact,
};

/// A window call's parts, owned (Java's `SqlExpr.WindowCall` as a map key).
fn window_parts(w: &SqlExpr) -> (&SqlAgg, &[SqlExpr], &[SortKey], Option<&Frame>) {
    match w {
        SqlExpr::WindowCall { r#fn, partition_by, order_by, frame, .. } => {
            (r#fn, partition_by, order_by, frame.as_ref())
        }
        _ => unreachable!("a window call"),
    }
}

/// `e` with each expression `replaced` names replaced by its value (Java's `substitute`): `None` where nothing changed.
fn substitute(e: &SqlExpr, replaced: &[(SqlExpr, SqlExpr)]) -> Result<Option<SqlExpr>, Refusal> {
    if let Some((_, r)) = replaced.iter().find(|(k, _)| k == e) {
        return Ok(Some(r.clone()));
    }
    carrier::map_children(e, &mut |c| substitute(c, replaced))
}

/// The FROM's own columns read under its alias, each a projection naming it (a star's spelling over a source with
/// helper columns beside it).
fn own_columns(alias: &str, outputs: &[OutputCol]) -> Result<Vec<Projection>, Refusal> {
    outputs
        .iter()
        .map(|oc| {
            build::projection(
                SqlExpr::Column(build::column_of(Some(alias), oc)),
                Some(oc.name.clone()),
                Some(oc.clone()),
            )
        })
        .collect()
}

/// The projections of `s` over the FROM read under `alias`: a star over it the FROM's own columns, any other projection
/// with the window calls `replaced` substituted.
fn projections_over(
    s: &SqlSelect,
    alias: &str,
    outputs: &[OutputCol],
    replaced: &[(SqlExpr, SqlExpr)],
) -> Result<Vec<Projection>, Refusal> {
    let mut ps = Vec::with_capacity(s.projections.len());
    for p in &s.projections {
        if let SqlExpr::Star { table, .. } = &p.expr
            && table.as_deref().is_none_or(|t| t == alias)
        {
            ps.extend(own_columns(alias, outputs)?);
            continue;
        }
        match substitute(&p.expr, replaced)? {
            None => ps.push(p.clone()),
            Some(x) => ps.push(build::projection(x, p.alias.clone(), p.out.clone())?),
        }
    }
    Ok(ps)
}

/// Whether a select is left as it is by both passes: it groups, is distinct, filters on windows or reads a join, or
/// its FROM has no alias or declares no column.
fn left_alone(s: &SqlSelect) -> bool {
    !s.group_by.is_empty()
        || s.having.is_some()
        || s.distinct
        || s.qualify.is_some()
        || matches!(s.from, SqlSource::Join { .. } | SqlSource::Dual)
        || source_outputs(&s.from).is_empty()
}

/// `new SqlSelect(projections, false, new Subselect(rows, alias, null), null, [], null, null, order, limit, offset,
/// outputs)`.
fn over_rows(
    projections: Vec<Projection>,
    rows: SqlSelect,
    alias: &str,
    order_by: Vec<SortKey>,
    s: &SqlSelect,
) -> Result<SqlSelect, Refusal> {
    build::select(SqlSelect {
        projections,
        distinct: false,
        from: build::subselect(build::query(rows), alias, None),
        r#where: None,
        group_by: Vec::new(),
        having: None,
        qualify: None,
        order_by,
        limit: s.limit,
        offset: s.offset,
        outputs: s.outputs.clone(),
    })
}

// ==================================================================
// WindowAggregatesAsCorrelated
// ==================================================================

/// A window call of an aggregate the dialect writes only grouped as that aggregate over the window's rows (Java's
/// `WindowAggregatesAsCorrelated`): the FROM read numbered under its own alias -- each row's position in its partition
/// in the window's order (ties broken by every column of the FROM), its rank there (a RANGE frame's peers share it),
/// the window's partition values and arguments as columns -- and each call a correlated scalar subquery aggregating
/// the numbered rows of the row's partition inside its frame. A RANGE frame with offsets, an interval frame, an
/// aggregate with its own order, and a select that groups, is distinct or reads a join are left as they are.
#[derive(Debug)]
pub struct WindowAggregatesAsCorrelated {
    /// Java's `unwindowed`: the aggregates rewritten.
    unwindowed: Vec<AggFn>,
    /// Java's `next`: numbers each rewritten select's helper columns across the statement.
    next: usize,
}

impl WindowAggregatesAsCorrelated {
    /// Java's `new WindowAggregatesAsCorrelated(unwindowed)`.
    pub fn new(unwindowed: &[AggFn]) -> WindowAggregatesAsCorrelated {
        WindowAggregatesAsCorrelated { unwindowed: unwindowed.to_vec(), next: 0 }
    }

    /// Java's `collect`: each window call of an unwindowed aggregate under `e`.
    fn collect(&self, e: &SqlExpr, out: &mut Vec<SqlExpr>) {
        if let SqlExpr::WindowCall { r#fn, .. } = e
            && let SqlAgg::Reducer(r) = r#fn.as_ref()
            && self.unwindowed.contains(&r.r#fn)
        {
            out.push(e.clone());
            return;
        }
        for c in typing::children(e) {
            self.collect(&c, out);
        }
    }

    /// Java's `rewritable`: an unwindowed aggregate without its own order, over a frame of positions or of CURRENT ROW
    /// and UNBOUNDED ranks.
    fn rewritable(&self, w: &SqlExpr) -> bool {
        let (f, _, _, frame) = window_parts(w);
        let SqlAgg::Reducer(r) = f else {
            return false;
        };
        if !self.unwindowed.contains(&r.r#fn) || !r.order_by.is_empty() {
            return false;
        }
        let Some(frame) = frame else {
            return true;
        };
        if frame.kind == FrameKind::Rows {
            return !(interval(&frame.from) || interval(&frame.to));
        }
        rank_bound(&frame.from) && rank_bound(&frame.to)
    }
}

fn interval(b: &Bound) -> bool {
    matches!(b, Bound::IntervalPreceding { .. } | Bound::IntervalFollowing { .. })
}

/// Java's `rankBound`.
fn rank_bound(b: &Bound) -> bool {
    matches!(b, Bound::UnboundedPreceding | Bound::CurrentRow | Bound::UnboundedFollowing)
}

/// Java's `helper(value, name)`: a helper column, typed when its value is.
fn helper(value: &SqlExpr, name: String) -> Result<Projection, Refusal> {
    let out = match value.r#type() {
        TypeFact::Typed { r#type, .. } => Some(build::output_col(&name, r#type.clone(), true)),
        _ => None,
    };
    build::projection(value.clone(), Some(name), out)
}

/// Java's `frame(w)`: its own frame, an ordered window's default (RANGE UNBOUNDED PRECEDING to CURRENT ROW), or none
/// for an unordered window's whole partition.
fn frame_of(order_by: &[SortKey], frame: Option<&Frame>) -> Option<Frame> {
    if frame.is_some() || order_by.is_empty() {
        return frame.cloned();
    }
    Some(Frame { kind: FrameKind::Range, from: Bound::UnboundedPreceding, to: Bound::CurrentRow })
}

/// Java's `Number.longValue()`.
fn long_value(n: &JavaNumber) -> i64 {
    match n {
        JavaNumber::Long(v) => *v,
        JavaNumber::Integer(v) => i64::from(*v),
        JavaNumber::Double(v) => v.value() as i64,
        JavaNumber::BigDecimal(v) => {
            // toBigInteger().longValue(): the integer part, its low 64 bits
            let plain = crate::java::big_decimal_plain(v.as_str());
            let (negative, digits) = match plain.strip_prefix('-') {
                Some(rest) => (true, rest),
                None => (false, plain.as_str()),
            };
            let whole = digits.split('.').next().unwrap_or("");
            let mut low: u64 = 0;
            for d in whole.bytes() {
                low = low.wrapping_mul(10).wrapping_add(u64::from(d - b'0'));
            }
            (if negative { low.wrapping_neg() } else { low }) as i64
        }
    }
}

/// Java's `bound(b, row)`: the position (or rank) a frame bound names relative to the row's, none for an unbounded
/// one.
fn bound(b: &Bound, row: &SqlExpr) -> Result<Option<SqlExpr>, Refusal> {
    Ok(match b {
        Bound::UnboundedPreceding | Bound::UnboundedFollowing => None,
        Bound::CurrentRow => Some(row.clone()),
        Bound::Preceding { n } => Some(build::call(SqlFn::Minus, vec![row.clone(), build::int_lit(long_value(n))])),
        Bound::Following { n } => Some(build::call(SqlFn::Plus, vec![row.clone(), build::int_lit(long_value(n))])),
        Bound::IntervalPreceding { .. } | Bound::IntervalFollowing { .. } => {
            return Err(Refusal::illegal_state("an interval bound passed rewritable()"));
        }
    })
}

/// Java's `aggregateOfFrame`: `w` as the aggregate of the numbered rows in the outer row's frame.
fn aggregate_of_frame(w: &SqlExpr, c: &str, rows: &SqlSelect, outer: &str, inner: &str) -> Result<SqlExpr, Refusal> {
    let (f, partition_by, order_by, frame) = window_parts(w);
    let SqlAgg::Reducer(r) = f else { unreachable!("a rewritable window is a reducer's") };
    let args: Vec<SqlExpr> = r
        .args
        .iter()
        .enumerate()
        .map(|(i, a)| {
            if matches!(a, SqlExpr::StringLit { .. }) {
                a.clone()
            } else {
                build::derived(Some(inner), &format!("{c}a{i}"))
            }
        })
        .collect();
    // a joined or listed value keeps the window's order
    let ordered = matches!(r.r#fn, AggFn::StringAgg | AggFn::List);
    let order = if ordered {
        vec![build::sort_key(build::derived(Some(inner), &format!("{c}n")), true, None, None)]
    } else {
        Vec::new()
    };
    let agg = build::reducer(r.r#fn, args, r.distinct, order);
    let mut conds = Vec::new();
    for i in 0..partition_by.len() {
        conds.push(build::call(
            SqlFn::NullSafeEqual,
            vec![build::derived(Some(inner), &format!("{c}p{i}")), build::derived(Some(outer), &format!("{c}p{i}"))],
        ));
    }
    if let Some(f) = frame_of(order_by, frame) {
        let col = if f.kind == FrameKind::Rows { format!("{c}n") } else { format!("{c}k") };
        let mine = build::derived(Some(inner), &col);
        let row = build::derived(Some(outer), &col);
        let lo = bound(&f.from, &row)?;
        let hi = bound(&f.to, &row)?;
        if let Some(lo) = lo {
            conds.push(build::call(SqlFn::GreaterEqual, vec![mine.clone(), lo]));
        }
        if let Some(hi) = hi {
            conds.push(build::call(SqlFn::LessEqual, vec![mine, hi]));
        }
    }
    let mut r#where: Option<SqlExpr> = None;
    for cond in conds {
        r#where = Some(match r#where {
            None => cond,
            Some(w) => build::call(SqlFn::And, vec![w, cond]),
        });
    }
    let frame_select = build::select(SqlSelect {
        projections: vec![build::projection(SqlExpr::Reducer(agg), None, None)?],
        distinct: false,
        from: build::subselect(build::query(rows.clone()), inner, None),
        r#where,
        group_by: Vec::new(),
        having: None,
        qualify: None,
        order_by: Vec::new(),
        limit: None,
        offset: None,
        outputs: Vec::new(),
    })?;
    Ok(build::scalar_subquery(build::query(frame_select)))
}

impl SqlRewriter for WindowAggregatesAsCorrelated {
    fn name(&self) -> &'static str {
        "WindowAggregatesAsCorrelated"
    }

    fn rewrite_root(&mut self, q: SqlQuery) -> Result<SqlQuery, Refusal> {
        self.next = 0;
        Ok(rewrite(self, q)?.into_inner())
    }

    fn select(&mut self, s: SqlSelect) -> Result<Rw<SqlQuery>, Refusal> {
        let mut calls = Vec::new();
        for p in &s.projections {
            self.collect(&p.expr, &mut calls);
        }
        if calls.is_empty() || left_alone(&s) || calls.iter().any(|w| !self.rewritable(w)) {
            return Ok(Rw::Same(build::query(s)));
        }
        let alias = source_alias(&s.from)?.to_owned();
        let stem = format!("__wa{}", self.next);
        self.next += 1;
        let from_outputs = source_outputs(&s.from).to_vec();

        // the FROM numbered: its own columns, then each call's partition values, arguments, position and rank
        let mut numbered = own_columns(&alias, &from_outputs)?;
        let tiebreak: Vec<SortKey> = from_outputs
            .iter()
            .map(|oc| build::sort_key(SqlExpr::Column(build::column_of(Some(&alias), oc)), true, None, None))
            .collect();
        let mut names: Vec<(SqlExpr, String)> = Vec::new();
        for w in &calls {
            if names.iter().any(|(k, _)| k == w) {
                continue;
            }
            let c = format!("{stem}c{}", names.len());
            names.push((w.clone(), c.clone()));
            let (f, partition_by, order_by, frame) = window_parts(w);
            for (i, p) in partition_by.iter().enumerate() {
                numbered.push(helper(p, format!("{c}p{i}"))?);
            }
            let SqlAgg::Reducer(r) = f else { unreachable!("a rewritable window is a reducer's") };
            for (i, a) in r.args.iter().enumerate() {
                if !matches!(a, SqlExpr::StringLit { .. }) {
                    numbered.push(helper(a, format!("{c}a{i}"))?);
                }
            }
            let mut order = order_by.to_vec();
            order.extend(tiebreak.iter().cloned());
            let position = build::window_call(
                SqlAgg::RankingFn { r#fn: AggFn::RowNumber, args: Vec::new() },
                partition_by.to_vec(),
                order,
                None,
            );
            let n = format!("{c}n");
            numbered.push(build::projection(
                position,
                Some(n.clone()),
                Some(build::output_col(&n, SqlType::Scalar(Scalar::Bigint), false)),
            )?);
            if frame_of(order_by, frame).is_some_and(|f| f.kind == FrameKind::Range) {
                let rank = build::window_call(
                    SqlAgg::RankingFn { r#fn: AggFn::Rank, args: Vec::new() },
                    partition_by.to_vec(),
                    order_by.to_vec(),
                    None,
                );
                let k = format!("{c}k");
                numbered.push(build::projection(
                    rank,
                    Some(k.clone()),
                    Some(build::output_col(&k, SqlType::Scalar(Scalar::Bigint), false)),
                )?);
            }
        }
        let rows = build::select(SqlSelect {
            projections: numbered,
            distinct: false,
            from: s.from.clone(),
            r#where: s.r#where.clone(),
            group_by: Vec::new(),
            having: None,
            qualify: None,
            order_by: Vec::new(),
            limit: None,
            offset: None,
            outputs: Vec::new(),
        })?;

        // the select over the numbered rows, each call the aggregate of its frame's rows
        let inner = format!("{stem}b");
        let mut replaced = Vec::with_capacity(names.len());
        for (w, c) in &names {
            replaced.push((w.clone(), aggregate_of_frame(w, c, &rows, &alias, &inner)?));
        }
        let ps = projections_over(&s, &alias, &from_outputs, &replaced)?;
        let out = over_rows(ps, rows, &alias, s.order_by.clone(), &s)?;
        Ok(Rw::New(build::query(out)))
    }
}

// ==================================================================
// WindowsComputedBelow
// ==================================================================

/// Each window call inside a larger expression of a select's projection computed in a select below, the expression
/// written over its column (Java's `WindowsComputedBelow`, MySQL's: it evaluates a subquery of an expression over a
/// window call on every row, whatever branch of a CASE it is in). The select below reads the FROM under its own alias;
/// a select that groups, is distinct, filters on windows or reads a join is left as it is.
#[derive(Debug, Default)]
pub struct WindowsComputedBelow {
    /// Java's `next`: numbers each rewritten select's window columns across the statement.
    next: usize,
}

/// Java's `collect`: each window call under `e`.
fn windows(e: &SqlExpr, out: &mut Vec<SqlExpr>) {
    if matches!(e, SqlExpr::WindowCall { .. }) {
        out.push(e.clone());
        return;
    }
    for c in typing::children(e) {
        windows(&c, out);
    }
}

impl SqlRewriter for WindowsComputedBelow {
    fn name(&self) -> &'static str {
        "WindowsComputedBelow"
    }

    fn rewrite_root(&mut self, q: SqlQuery) -> Result<SqlQuery, Refusal> {
        self.next = 0;
        Ok(rewrite(self, q)?.into_inner())
    }

    fn select(&mut self, s: SqlSelect) -> Result<Rw<SqlQuery>, Refusal> {
        let mut calls = Vec::new();
        for p in &s.projections {
            if !matches!(p.expr, SqlExpr::WindowCall { .. }) {
                windows(&p.expr, &mut calls);
            }
        }
        if calls.is_empty() || left_alone(&s) {
            return Ok(Rw::Same(build::query(s)));
        }
        let alias = source_alias(&s.from)?.to_owned();
        let stem = format!("__wb{}", self.next);
        self.next += 1;
        let from_outputs = source_outputs(&s.from).to_vec();
        let mut below = own_columns(&alias, &from_outputs)?;
        let mut replaced: Vec<(SqlExpr, SqlExpr)> = Vec::new();
        for w in &calls {
            if replaced.iter().any(|(k, _)| k == w) {
                continue;
            }
            let name = format!("{stem}w{}", replaced.len());
            let out = match w.r#type() {
                TypeFact::Typed { r#type, .. } => Some(build::output_col(&name, r#type.clone(), true)),
                _ => None,
            };
            let read = match &out {
                Some(out) => SqlExpr::Column(build::column_of(Some(&alias), out)),
                None => build::derived(Some(&alias), &name),
            };
            below.push(build::projection(w.clone(), Some(name), out)?);
            replaced.push((w.clone(), read));
        }
        let rows = build::select(SqlSelect {
            projections: below,
            distinct: false,
            from: s.from.clone(),
            r#where: s.r#where.clone(),
            group_by: Vec::new(),
            having: None,
            qualify: None,
            order_by: Vec::new(),
            limit: None,
            offset: None,
            outputs: Vec::new(),
        })?;
        let ps = projections_over(&s, &alias, &from_outputs, &replaced)?;
        let mut order = Vec::with_capacity(s.order_by.len());
        for k in &s.order_by {
            match substitute(&k.expr, &replaced)? {
                None => order.push(k.clone()),
                Some(e) => order.push(build::sort_key(e, k.ascending, k.null_order, k.output_name.clone())),
            }
        }
        let out = over_rows(ps, rows, &alias, order, &s)?;
        Ok(Rw::New(build::query(out)))
    }
}
