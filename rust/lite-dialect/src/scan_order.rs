// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! Java's `StableScanOrder` (`core/src/main/java/com/legend/sql/dialect/StableScanOrder.java`) and the scan-order key
//! it applies (`com.legend.sql.ScanOrder`, [`ScanOrder`]): the engine corpus's scan order made explicit, engine-corpus
//! compatibility only, run when the JVM's `legend.exec.engineScanOrder` property is set
//! ([`crate::duckdb::DuckDb::engine_scan_order`]). The engine's tests assert positionally while relying on H2's
//! implicit scan order; replaying them on DuckDB needs that order spelled.
//!
//! Every node is built through [`crate::build`] (Java's constructors: a rebuilt select's outputs recomputed, a join's
//! delivered columns); each step answers [`Rw::Same`] where Java hands back its input (`==`). Java's maps here
//! (`cteOrds`, `frames`, `framesNeedingOrder`'s set) are only ever looked up, so no iteration order reaches output.

use crate::build::{self, column_in, row_order, sort_key};
use crate::refusal::Refusal;
use crate::rewriter::{Rw, SqlRewriter, rewrite};
use crate::typing::children;
use lite_sql::{AggFn, Cte, Projection, Reducer, Scalar, SortKey, SqlExpr, SqlQuery, SqlSelect, SqlSource, SqlType};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};

fn bigint() -> SqlType {
    SqlType::Scalar(Scalar::Bigint)
}

/// `SqlSelect.Projection.outputName()`: the alias, else a bare column's own name (a stored read's column's).
fn output_name(p: &Projection) -> Option<String> {
    match (&p.alias, &p.expr) {
        (Some(a), _) => Some(a.clone()),
        (None, SqlExpr::Column(c)) => Some(c.name.clone()),
        (None, SqlExpr::StoredRead { column, .. }) => Some(column.name.clone()),
        _ => None,
    }
}

// ==================================================================
// ScanOrder
// ==================================================================

/// Java's `com.legend.sql.ScanOrder`: the deterministic base-scan order key for a query that asked for none -- join
/// trees rooted at a bare table scan, never over DISTINCT, GROUP BY, an aggregate or a user ORDER BY; a cap wrapper
/// over an orderable inner orders the inner.
pub struct ScanOrder;

impl ScanOrder {
    /// `ScanOrder.stabilize(q, frames)`: `q` with the key applied where the shape needs and permits it, `q` itself
    /// otherwise; `frames` are the statement's CTE names, which a table source naming one reads (no rowid).
    pub fn stabilize(q: SqlQuery, frames: &HashSet<String>) -> Result<Rw<SqlQuery>, Refusal> {
        let SqlQuery::Select(s) = q else {
            return Ok(Rw::Same(q));
        };
        Ok(match stabilize_select(&s, frames)? {
            Some(st) => Rw::New(build::query(st)),
            None => Rw::Same(SqlQuery::Select(s)),
        })
    }
}

/// `stabilize` over a select: the new select, or none where Java returns its input.
fn stabilize_select(s: &SqlSelect, frames: &HashSet<String>) -> Result<Option<SqlSelect>, Refusal> {
    if let Some(base) = orderable_base(s, frames) {
        return Ok(Some(ordered(s, base, frames)?));
    }
    // a rows->at(N)-style cap whose inner carries the relation
    if let SqlSource::Subselect { inner, alias, frame_name } = &s.from
        && let SqlQuery::Select(inner) = inner.as_ref()
        && (inner.limit.is_some() || inner.offset.is_some())
        && let Some(inner_base) = orderable_base(inner, frames)
    {
        let inner = ordered(inner, inner_base, frames)?;
        let from = build::subselect(build::query(inner), alias, frame_name.clone());
        return Ok(Some(build::with_from(s.clone(), from)?));
    }
    Ok(None)
}

/// `ScanOrder.ordered`: the key lexicographic over the join tree's base-table scans, the driving table first.
fn ordered(s: &SqlSelect, base_alias: &str, frames: &HashSet<String>) -> Result<SqlSelect, Refusal> {
    let mut tables = Vec::new();
    scan_tables(&s.from, frames, &mut tables);
    let mut keys: Vec<SortKey> =
        tables.into_iter().map(|alias| sort_key(row_order(Some(alias.to_owned())), true, None, None)).collect();
    if keys.is_empty() {
        keys.push(sort_key(row_order(Some(base_alias.to_owned())), true, None, None));
    }
    build::with_order_by(s.clone(), keys)
}

/// `ScanOrder.scanTables`: the aliases of a join tree's base-table scans in join order (leftmost first; a join's
/// right side counts only when it is a scan itself).
fn scan_tables<'a>(from: &'a SqlSource, frames: &HashSet<String>, out: &mut Vec<&'a str>) {
    match from {
        SqlSource::Join { left, right, .. } => {
            scan_tables(left, frames, out);
            if let SqlSource::Table { alias, .. } = right.as_ref()
                && is_scan(right, frames)
            {
                out.push(alias);
            }
        }
        SqlSource::Table { alias, .. } if is_scan(from, frames) => out.push(alias),
        _ => {}
    }
}

/// `ScanOrder.isScan`: a base-table scan -- not a tabular function's rows, not a frame of the statement.
fn is_scan(t: &SqlSource, frames: &HashSet<String>) -> bool {
    matches!(t, SqlSource::Table { name, call: false, .. } if !frames.contains(name))
}

/// `ScanOrder.orderableBase`: the driving base-table scan's alias when the select is order-stabilizable.
fn orderable_base<'a>(s: &'a SqlSelect, frames: &HashSet<String>) -> Option<&'a str> {
    if !s.order_by.is_empty()
        || s.distinct
        || !s.group_by.is_empty()
        || s.qualify.is_some()
        || !matches!(s.from, SqlSource::Join { .. })
        || s.projections.iter().any(|p| contains_reducer(&p.expr))
    {
        return None;
    }
    let mut leftmost = &s.from;
    while let SqlSource::Join { left, .. } = leftmost {
        leftmost = left;
    }
    match leftmost {
        SqlSource::Table { alias, .. } if is_scan(leftmost, frames) => Some(alias),
        _ => None,
    }
}

/// `ScanOrder.containsReducer`: a top-level aggregate anywhere in the expression (subqueries are their own scope).
fn contains_reducer(e: &SqlExpr) -> bool {
    match e {
        SqlExpr::Reducer(_) | SqlExpr::JsonArrayAgg { .. } | SqlExpr::OrderedListAgg { .. } => true,
        SqlExpr::ScalarSubquery { .. } | SqlExpr::Exists { .. } => false,
        _ => children(e).iter().any(|c| contains_reducer(c)),
    }
}

// ==================================================================
// StableScanOrder
// ==================================================================

/// Java's `FIRINGS`: the statements every [`StableScanOrder`] of this process changed (an order the statement lacked
/// was added). The corpus runner attributes them to the test that ran and pins that set per lane
/// (`rcorpus/<lane>-engine-order-register.txt`); the JVM reads it through the seam (`scanOrderFirings`).
static FIRINGS: AtomicU64 = AtomicU64::new(0);

/// The statements every [`StableScanOrder`] of this process has changed (Java's `StableScanOrder.firings()`).
pub fn firings() -> u64 {
    FIRINGS.load(Ordering::Relaxed)
}

/// The engine-corpus scan-order pass (Java's `StableScanOrder`).
#[derive(Debug, Default)]
pub struct StableScanOrder {
    /// The statements this pass object changed; [`firings`] counts them for the whole process, as Java's does.
    pub firings: u64,
    /// Java's `cteOrds`: the ordinals each frame CTE of the statement exports, per statement.
    cte_ords: HashMap<String, Vec<String>>,
    /// Java's `frames`: the statement's own CTE names, per statement.
    frames: HashSet<String>,
}

impl StableScanOrder {
    pub fn new() -> StableScanOrder {
        StableScanOrder::default()
    }

    /// `widened(c)`: a frame reference with its threaded ordinals in scope (its outputs widened); none when the
    /// frame threads none.
    fn widened(&self, c: &SqlSource) -> Option<SqlSource> {
        let SqlSource::Cte { name, alias, outputs } = c else {
            return None;
        };
        let ords = self.cte_ords.get(name)?;
        let mut outs = outputs.clone();
        for n in ords {
            if !outs.iter().any(|o| o.name == *n) {
                outs.push(build::output_col(n, bigint(), true));
            }
        }
        Some(SqlSource::Cte { name: name.clone(), alias: alias.clone(), outputs: outs })
    }

    /// `stabilizeOverFrame(s)`: a positional read (a LIMIT/OFFSET select with no sort of its own) whose leftmost
    /// scan is a threaded frame, ordered by the frame's ordinals; none where Java returns `s`.
    fn stabilize_over_frame(&self, s: &SqlSelect) -> Result<Option<SqlSelect>, Refusal> {
        if !s.order_by.is_empty()
            || s.distinct
            || !s.group_by.is_empty()
            || (s.limit.is_none() && s.offset.is_none())
            || s.projections.iter().any(|p| aggregates(&p.expr))
        {
            return Ok(None);
        }
        let mut leftmost = &s.from;
        while let SqlSource::Join { left, .. } = leftmost {
            leftmost = left;
        }
        let SqlSource::Cte { name, .. } = leftmost else {
            return Ok(None);
        };
        let Some(wide) = self.widened(leftmost) else {
            return Ok(None);
        };
        // scan-major: the ordinals are recorded probe-major, so the cap reads them reversed
        let mut ords = self.cte_ords.get(name).cloned().unwrap_or_default();
        ords.reverse();
        let SqlSource::Cte { alias: wide_alias, outputs: wide_outputs, .. } = &wide else {
            return Ok(None);
        };
        let keys: Vec<SortKey> = ords
            .iter()
            .map(|n| sort_key(SqlExpr::Column(column_in(Some(wide_alias), wide_outputs, n)), true, None, None))
            .collect();
        let from = replace_leftmost(s.from.clone(), &wide)?;
        Ok(Some(build::with_order_by(build::with_from(s.clone(), from)?, keys)?))
    }

    /// `threadScan(sel)`: `sel` with hidden `__agg_ordN` projections appended for every base-table rowid reachable
    /// probe-major through its from tree; none when the shape refuses (set semantics, star frames).
    fn thread_scan(&self, sel: &SqlSelect) -> Result<Option<(SqlSelect, Vec<String>)>, Refusal> {
        if sel.distinct
            || !sel.group_by.is_empty()
            || sel.limit.is_some()
            || sel.offset.is_some()
            || sel.projections.iter().any(|p| aggregates(&p.expr))
            || sel.projections.is_empty()
            || sel
                .projections
                .iter()
                .any(|p| matches!(p.expr, SqlExpr::Star { .. } | SqlExpr::StarExcept { .. }) || p.out.is_none())
        {
            return Ok(None);
        }
        let mut ord_exprs = Vec::new();
        let from2 = self.walk_from(&sel.from, &mut ord_exprs)?.unwrap_or_else(|| sel.from.clone());
        if ord_exprs.is_empty() {
            return Ok(None);
        }
        let mut ps = sel.projections.clone();
        let mut outs = sel.outputs.clone();
        let mut names = Vec::new();
        for e in ord_exprs {
            let name = format!("__agg_ord{}", names.len());
            let col = build::output_col(&name, bigint(), true);
            ps.push(build::projection(e, Some(name.clone()), Some(col.clone()))?);
            outs.push(col);
            names.push(name);
        }
        let threaded = build::select(SqlSelect { projections: ps, from: from2, outputs: outs, ..sel.clone() })?;
        Ok(Some((threaded, names)))
    }

    /// `walkFrom(src, ordExprs)`: the probe-major rowid walk (a join's right side before its left), the source
    /// rewritten where a frame threads ordinals; none where Java returns `src` itself.
    fn walk_from(&self, src: &SqlSource, ord_exprs: &mut Vec<SqlExpr>) -> Result<Option<SqlSource>, Refusal> {
        match src {
            SqlSource::Table { alias, .. } => {
                ord_exprs.push(row_order(Some(alias.clone())));
                Ok(None)
            }
            SqlSource::Cte { name, .. } => {
                // a frame reference re-exports the ordinals its definition threads
                let Some(wide) = self.widened(src) else {
                    return Ok(None);
                };
                if let SqlSource::Cte { alias, outputs, .. } = &wide {
                    for n in self.cte_ords.get(name).into_iter().flatten() {
                        ord_exprs.push(SqlExpr::Column(column_in(Some(alias), outputs, n)));
                    }
                }
                Ok(Some(wide))
            }
            SqlSource::Join { left, right, kind, on, .. } => {
                let mut right_ords = Vec::new();
                let r = self.walk_from(right, &mut right_ords)?;
                let mut left_ords = Vec::new();
                let l = self.walk_from(left, &mut left_ords)?;
                ord_exprs.extend(right_ords);
                ord_exprs.extend(left_ords);
                if r.is_none() && l.is_none() {
                    return Ok(None);
                }
                let l = l.unwrap_or_else(|| left.as_ref().clone());
                let r = r.unwrap_or_else(|| right.as_ref().clone());
                Ok(Some(build::join(l, r, *kind, on.as_deref().cloned())?))
            }
            SqlSource::Subselect { inner, alias, frame_name } => match inner.as_ref() {
                SqlQuery::Select(inner) => {
                    let Some((threaded, names)) = self.thread_scan(inner)? else {
                        return Ok(None);
                    };
                    for n in &names {
                        ord_exprs.push(SqlExpr::Column(column_in(Some(alias), &threaded.outputs, n)));
                    }
                    Ok(Some(build::subselect(build::query(threaded), alias, frame_name.clone())))
                }
                // a UNION ALL frame: H2 runs its legs in turn -- every leg appends its index and its own first scan
                // ordinal, and the frame re-exports both
                SqlQuery::Union(u) if u.all => {
                    let mut branches = Vec::with_capacity(u.branches.len());
                    for (i, b) in u.branches.iter().enumerate() {
                        let SqlQuery::Select(leg) = b else {
                            return Ok(None);
                        };
                        let Some(leg2) = self.union_leg_ordinals(leg, i as i64)? else {
                            return Ok(None);
                        };
                        branches.push(build::query(leg2));
                    }
                    let mut outs = u.outputs.clone();
                    outs.push(build::output_col("__agg_leg", bigint(), true));
                    outs.push(build::output_col("__agg_legord", bigint(), true));
                    let u2 = build::union(branches, true, outs.clone())?;
                    ord_exprs.push(SqlExpr::Column(column_in(Some(alias), &outs, "__agg_leg")));
                    ord_exprs.push(SqlExpr::Column(column_in(Some(alias), &outs, "__agg_legord")));
                    Ok(Some(build::subselect(SqlQuery::Union(u2), alias, frame_name.clone())))
                }
                _ => Ok(None),
            },
            _ => Ok(None),
        }
    }

    /// `unionLegOrdinals(leg, index)`: a union leg with `__agg_leg` (its index) and `__agg_legord` (its first
    /// probe-major scan ordinal, NULL when none) appended; none when the leg's shape refuses.
    fn union_leg_ordinals(&self, leg: &SqlSelect, index: i64) -> Result<Option<SqlSelect>, Refusal> {
        if leg.distinct
            || !leg.group_by.is_empty()
            || leg.limit.is_some()
            || leg.offset.is_some()
            || leg
                .projections
                .iter()
                .any(|p| matches!(p.expr, SqlExpr::Star { .. } | SqlExpr::StarExcept { .. }) || p.out.is_none())
        {
            return Ok(None);
        }
        let mut ords = Vec::new();
        // Java walks the leg's from tree for its ordinals alone (the rewritten source is not kept)
        self.walk_from(&leg.from, &mut ords)?;
        let ord = if ords.is_empty() { build::cast(build::null_lit(), bigint()) } else { ords.swap_remove(0) };
        let mut ps = leg.projections.clone();
        if ps.is_empty() {
            // an implicit star frame keeps its row explicitly (`SELECT *, <ordinals>`)
            ps.push(build::projection(build::star(None), None, None)?);
        }
        let mut outs = leg.outputs.clone();
        let leg_col = build::output_col("__agg_leg", bigint(), true);
        let ord_col = build::output_col("__agg_legord", bigint(), true);
        ps.push(build::projection(build::int_lit(index), Some("__agg_leg".to_owned()), Some(leg_col.clone()))?);
        ps.push(build::projection(ord, Some("__agg_legord".to_owned()), Some(ord_col.clone()))?);
        outs.push(leg_col);
        outs.push(ord_col);
        Ok(Some(build::select(SqlSelect { projections: ps, outputs: outs, ..leg.clone() })?))
    }

    /// The select hook's frame case: an order-sensitive reducer over a from-subselect orders by the input's user
    /// sort keys, then the frame's threaded scan ordinals; none where Java returns `s`.
    fn through_frame(&self, s: &SqlSelect) -> Result<Option<SqlSelect>, Refusal> {
        let SqlSource::Subselect { inner, alias: sub_alias, frame_name } = &s.from else {
            return Ok(None);
        };
        let SqlQuery::Select(inner) = inner.as_ref() else {
            return Ok(None);
        };
        // the reducer's value must read the from-subselect's own alias
        let wants = |r: &Reducer| {
            order_sensitive(r) && r.order_by.is_empty() && !r.args.is_empty() && reads_alias(&r.args[0], sub_alias)
        };
        if !s.projections.iter().any(|p| matches!(&p.expr, SqlExpr::Reducer(r) if wants(r))) {
            return Ok(None);
        }
        let Some((mut widened, ord_names)) = self.thread_scan(inner)? else {
            return Ok(None);
        };
        if ord_names.is_empty() {
            return Ok(None);
        }
        // user sort keys reference exported outputs by name; a key the inner does not export is exported hidden
        let mut keys: Vec<SortKey> = Vec::new();
        for k in &inner.order_by {
            let want = k.output_name.clone().or_else(|| match &k.expr {
                SqlExpr::Column(kc) => Some(kc.name.clone()),
                _ => None,
            });
            let name = match want {
                Some(want) if widened.outputs.iter().any(|c| c.name == want) => want,
                _ => {
                    let hidden = format!("__agg_key{}", keys.len());
                    let col = build::output_col(&hidden, SqlType::Scalar(Scalar::Varchar), true);
                    let mut wp = widened.projections.clone();
                    let mut wo = widened.outputs.clone();
                    wp.push(build::projection(k.expr.clone(), Some(hidden.clone()), Some(col.clone()))?);
                    wo.push(col);
                    widened = build::select(SqlSelect { projections: wp, outputs: wo, ..widened })?;
                    hidden
                }
            };
            let reference = SqlExpr::Column(column_in(Some(sub_alias), &widened.outputs, &name));
            keys.push(sort_key(reference, k.ascending, k.null_order, None));
        }
        for ord in &ord_names {
            keys.push(sort_key(SqlExpr::Column(column_in(Some(sub_alias), &widened.outputs, ord)), true, None, None));
        }
        let mut out = s.projections.clone();
        for p in out.iter_mut() {
            if let SqlExpr::Reducer(r) = &p.expr
                && wants(r)
            {
                let agg = build::reducer(r.r#fn, r.args.clone(), r.distinct, keys.clone());
                *p = build::projection(SqlExpr::Reducer(agg), output_name(p), p.out.clone())?;
            }
        }
        let from = build::subselect(build::query(widened), sub_alias, frame_name.clone());
        Ok(Some(build::select(SqlSelect { projections: out, from, ..s.clone() })?))
    }
}

/// `baseTableReducers(s)`: an order-sensitive reducer whose value is a column of a base table of this select's own
/// from tree orders by that table's row number; none where Java returns `s`.
fn base_table_reducers(s: &SqlSelect) -> Result<Option<SqlSelect>, Refusal> {
    let mut out: Option<Vec<Projection>> = None;
    for (i, p) in s.projections.iter().enumerate() {
        if let SqlExpr::Reducer(r) = &p.expr
            && order_sensitive(r)
            && r.order_by.is_empty()
            && let Some(SqlExpr::Column(vc)) = r.args.first()
            && alias_is_base_table(&s.from, vc.table.as_deref())
        {
            let key = sort_key(row_order(vc.table.clone()), true, None, None);
            let agg = build::reducer(r.r#fn, r.args.clone(), r.distinct, vec![key]);
            let ps = out.get_or_insert_with(|| s.projections.clone());
            ps[i] = build::projection(SqlExpr::Reducer(agg), output_name(p), p.out.clone())?;
        }
    }
    match out {
        None => Ok(None),
        Some(ps) => Ok(Some(build::select(SqlSelect { projections: ps, ..s.clone() })?)),
    }
}

/// `aliasIsBaseTable(src, alias)`: whether `alias` names a base-table scan of the from tree.
fn alias_is_base_table(src: &SqlSource, alias: Option<&str>) -> bool {
    match src {
        SqlSource::Table { alias: a, call, .. } => Some(a.as_str()) == alias && !call,
        SqlSource::Join { left, right, .. } => alias_is_base_table(left, alias) || alias_is_base_table(right, alias),
        _ => false,
    }
}

/// `StableScanOrder.aggregates(e)`: a reducer anywhere among the expression's children.
fn aggregates(e: &SqlExpr) -> bool {
    matches!(e, SqlExpr::Reducer(_)) || children(e).iter().any(|c| aggregates(c))
}

/// `readsAlias(e, alias)`: every column reference of the expression resolves in `alias` (or there is none).
fn reads_alias(e: &SqlExpr, alias: &str) -> bool {
    if let SqlExpr::Column(c) = e {
        return c.table.as_deref() == Some(alias);
    }
    children(e).iter().all(|c| reads_alias(c, alias))
}

/// `orderSensitive(r)`: a group concat or a LIST collect, not DISTINCT.
fn order_sensitive(r: &Reducer) -> bool {
    matches!(r.r#fn, AggFn::StringAgg | AggFn::List) && !r.distinct
}

/// `replaceLeftmost(src, wide)`: the join tree with its leftmost source the widened frame reference.
fn replace_leftmost(src: SqlSource, wide: &SqlSource) -> Result<SqlSource, Refusal> {
    match src {
        SqlSource::Join { left, right, kind, on, .. } => {
            build::join(replace_leftmost(*left, wide)?, *right, kind, on.map(|o| *o))
        }
        _ => Ok(wide.clone()),
    }
}

/// `frameNames(src, throughSubselects, out)`: the frame CTEs a from tree scans.
fn frame_names(src: &SqlSource, through_subselects: bool, out: &mut HashSet<String>) {
    match src {
        SqlSource::Cte { name, .. } => {
            out.insert(name.clone());
        }
        SqlSource::Join { left, right, .. } => {
            frame_names(left, through_subselects, out);
            frame_names(right, through_subselects, out);
        }
        SqlSource::Subselect { inner, .. } if through_subselects => {
            if let SqlQuery::Select(inner) = inner.as_ref() {
                frame_names(&inner.from, true, out);
            }
        }
        _ => {}
    }
}

/// The walk of `framesNeedingOrder`: the frame CTEs some select reads by position (a LIMIT/OFFSET select) or
/// through an order-sensitive aggregate.
#[derive(Default)]
struct FramesNeedingOrder(HashSet<String>);

impl SqlRewriter for FramesNeedingOrder {
    fn name(&self) -> &'static str {
        "StableScanOrder.framesNeedingOrder"
    }

    fn select(&mut self, s: SqlSelect) -> Result<Rw<SqlQuery>, Refusal> {
        if s.limit.is_some() || s.offset.is_some() {
            frame_names(&s.from, false, &mut self.0);
        }
        if s.projections
            .iter()
            .any(|p| matches!(&p.expr, SqlExpr::Reducer(r) if order_sensitive(r) && r.order_by.is_empty()))
        {
            frame_names(&s.from, true, &mut self.0);
        }
        Ok(Rw::Same(build::query(s)))
    }
}

impl SqlRewriter for StableScanOrder {
    fn name(&self) -> &'static str {
        "StableScanOrder"
    }

    fn rewrite_root(&mut self, q: SqlQuery) -> Result<SqlQuery, Refusal> {
        let original = q.clone();
        self.cte_ords.clear();
        self.frames.clear();
        let mut q = q;
        let threaded_ctes = if let SqlQuery::With(w) = &q {
            for c in &w.ctes {
                self.frames.insert(c.name.clone());
            }
            // only when a reader needs order: a frame CTE threads its scan ordinals only if some select reads it by
            // position or through an order-sensitive aggregate
            let mut needs = FramesNeedingOrder::default();
            rewrite(&mut needs, q.clone())?;
            let mut cs: Vec<Cte> = Vec::with_capacity(w.ctes.len());
            let mut changed = false;
            for c in &w.ctes {
                let threaded = match &c.query {
                    SqlQuery::Select(body) if needs.0.contains(&c.name) => self.thread_scan(body)?,
                    _ => None,
                };
                match threaded {
                    Some((select, ord_names)) if !ord_names.is_empty() => {
                        self.cte_ords.insert(c.name.clone(), ord_names);
                        cs.push(Cte {
                            name: c.name.clone(),
                            query: build::query(select),
                            materialized: c.materialized,
                        });
                        changed = true;
                    }
                    _ => cs.push(c.clone()),
                }
            }
            if changed { Some(SqlQuery::With(build::with(cs, (*w.body).clone(), w.recursive)?)) } else { None }
        } else {
            None
        };
        if let Some(threaded) = threaded_ctes {
            q = threaded;
        }
        // the deep walk first (the select hook on every nested select), then the root-shape special cases
        let walked = rewrite(self, q)?.into_inner();
        let out = ScanOrder::stabilize(walked, &self.frames)?.into_inner();
        if out != original {
            self.firings += 1;
            FIRINGS.fetch_add(1, Ordering::Relaxed);
        }
        Ok(out)
    }

    fn select(&mut self, s: SqlSelect) -> Result<Rw<SqlQuery>, Refusal> {
        let rewritten = match &s.from {
            SqlSource::Subselect { inner, .. } if matches!(inner.as_ref(), SqlQuery::Select(_)) => {
                self.through_frame(&s)?
            }
            _ => base_table_reducers(&s)?,
        };
        Ok(match rewritten {
            Some(n) => Rw::New(build::query(n)),
            None => Rw::Same(build::query(s)),
        })
    }

    /// Subselect inners stabilize too (a verdict statement compiles the asserted relation as a subquery), FROM
    /// subselects only (an ORDER BY is legal there).
    fn source(&mut self, s: SqlSource) -> Result<Rw<SqlSource>, Refusal> {
        let SqlSource::Subselect { inner, alias, frame_name } = &s else {
            return Ok(Rw::Same(s));
        };
        let SqlQuery::Select(sel) = inner.as_ref() else {
            return Ok(Rw::Same(s));
        };
        let st = match stabilize_select(sel, &self.frames)? {
            Some(st) => Some(st),
            None => self.stabilize_over_frame(sel)?,
        };
        Ok(match st {
            Some(st) => Rw::New(build::subselect(build::query(st), alias, frame_name.clone())),
            None => Rw::Same(s),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{ScanOrder, StableScanOrder};
    use crate::build;
    use crate::rewriter::{Rw, SqlRewriter};
    use crate::{DuckDb, SqlDialect};
    use lite_sql::{
        AggFn, Column, Cte, JoinKind, NameQuoting, Origin, OutputCol, Projection, Scalar, SortKey, SqlExpr, SqlFn,
        SqlQuery, SqlSelect, SqlSource, SqlType, TypeFact,
    };
    use std::collections::HashSet;

    fn varchar() -> SqlType {
        SqlType::Scalar(Scalar::Varchar)
    }

    fn physical(name: &str, t: SqlType) -> OutputCol {
        OutputCol { name: name.to_owned(), r#type: t, nullable: true, origin: Origin::Physical }
    }

    /// A base table with columns `id` (BIGINT) and `name` (VARCHAR).
    fn table(name: &str, alias: &str) -> SqlSource {
        SqlSource::Table {
            name: name.to_owned(),
            alias: alias.to_owned(),
            outputs: vec![physical("id", SqlType::Scalar(Scalar::Bigint)), physical("name", varchar())],
            call: false,
            stored_types: Vec::new(),
            quoting: NameQuoting::default(),
        }
    }

    fn col(table: &str, name: &str, t: SqlType) -> SqlExpr {
        SqlExpr::Column(Column {
            table: Some(table.to_owned()),
            name: name.to_owned(),
            r#type: TypeFact::Typed { r#type: t, nullable: true },
            origin: Some(Origin::Physical),
        })
    }

    fn name_of(alias: &str) -> SqlExpr {
        col(alias, "name", varchar())
    }

    fn id_of(alias: &str) -> SqlExpr {
        col(alias, "id", SqlType::Scalar(Scalar::Bigint))
    }

    /// `e AS alias`, its slot declared VARCHAR.
    fn named(e: SqlExpr, alias: &str) -> Projection {
        build::projection(e, Some(alias.to_owned()), Some(build::output_col(alias, varchar(), true))).unwrap()
    }

    fn select(projections: Vec<Projection>, from: SqlSource) -> SqlSelect {
        build::select_of(projections, from, Vec::new()).unwrap()
    }

    fn joined() -> SqlSource {
        build::join(
            table("T", "t0"),
            table("U", "t1"),
            JoinKind::Inner,
            Some(build::call(SqlFn::Equal, vec![id_of("t0"), id_of("t1")])),
        )
        .unwrap()
    }

    fn rowid_key(alias: &str) -> SortKey {
        build::sort_key(build::row_order(Some(alias.to_owned())), true, None, None)
    }

    fn string_agg(value: SqlExpr) -> SqlExpr {
        SqlExpr::Reducer(build::reducer(AggFn::StringAgg, vec![value, build::string_lit(",")], false, Vec::new()))
    }

    fn as_select(q: &SqlQuery) -> &SqlSelect {
        match q {
            SqlQuery::Select(s) => s,
            other => panic!("not a select: {other:?}"),
        }
    }

    fn inner_select(s: &SqlSource) -> &SqlSelect {
        match s {
            SqlSource::Subselect { inner, .. } => as_select(inner),
            other => panic!("not a subselect: {other:?}"),
        }
    }

    fn names(os: &[OutputCol]) -> Vec<&str> {
        os.iter().map(|o| o.name.as_str()).collect()
    }

    #[test]
    fn a_positional_read_over_a_join_of_scans_orders_by_each_scans_row_order() {
        // SELECT t0.name AS name FROM T t0 JOIN U t1 ON t0.id = t1.id LIMIT 2
        let s = SqlSelect { limit: Some(2), ..select(vec![named(name_of("t0"), "name")], joined()) };
        let mut pass = StableScanOrder::new();
        let out = pass.rewrite_root(build::query(s)).unwrap();
        // the driving table first, then the joined one (ScanOrder.ordered)
        assert_eq!(as_select(&out).order_by, vec![rowid_key("t0"), rowid_key("t1")]);
        assert_eq!(as_select(&out).limit, Some(2));
        assert_eq!(pass.firings, 1);
        // a joined subselect contributes no key: only the driving scan's
        let sub =
            build::subselect(build::query(select(vec![named(name_of("t1"), "name")], table("U", "t1"))), "s1", None);
        let on = build::call(SqlFn::Equal, vec![name_of("t0"), col("s1", "name", varchar())]);
        let over_frame = select(
            vec![named(name_of("t0"), "name")],
            build::join(table("T", "t0"), sub, JoinKind::Left, Some(on)).unwrap(),
        );
        let out = ScanOrder::stabilize(build::query(over_frame), &HashSet::new()).unwrap();
        assert!(out.is_new());
        assert_eq!(as_select(out.get()).order_by, vec![rowid_key("t0")]);
    }

    #[test]
    fn a_statement_the_key_does_not_fit_is_left_as_it_came_and_counts_no_firing() {
        let mut pass = StableScanOrder::new();
        // a user sort, a GROUP BY, a DISTINCT, an aggregate, a single table: each keeps what it says
        let sorted = SqlSelect {
            order_by: vec![build::sort_key(name_of("t0"), true, None, None)],
            ..select(vec![named(name_of("t0"), "name")], joined())
        };
        let grouped =
            SqlSelect { group_by: vec![name_of("t0")], ..select(vec![named(name_of("t0"), "name")], joined()) };
        let distinct = SqlSelect { distinct: true, ..select(vec![named(name_of("t0"), "name")], joined()) };
        let counted = select(
            vec![named(SqlExpr::Reducer(build::reducer(AggFn::Count, vec![id_of("t0")], false, Vec::new())), "n")],
            joined(),
        );
        let single = SqlSelect { limit: Some(1), ..select(vec![named(name_of("t0"), "name")], table("T", "t0")) };
        for s in [sorted, grouped, distinct, counted, single] {
            let q = build::query(s);
            assert_eq!(pass.rewrite_root(q.clone()).unwrap(), q);
        }
        assert_eq!(pass.firings, 0);
        // the hooks answer Java's `return s`: the very node
        let s = select(vec![named(name_of("t0"), "name")], table("T", "t0"));
        assert!(matches!(pass.select(s).unwrap(), Rw::Same(_)));
        // a frame of the statement named as a table is no scan
        let mut frames = HashSet::new();
        frames.insert("T".to_owned());
        let framed = select(vec![named(name_of("t0"), "name")], joined());
        assert!(matches!(ScanOrder::stabilize(build::query(framed), &frames).unwrap(), Rw::Same(_)));
    }

    #[test]
    fn a_cap_wrapper_orders_the_relation_its_inner_caps() {
        // SELECT * FROM (SELECT t0.name AS name FROM T t0 JOIN U t1 ON ... OFFSET 1) AS sub
        let inner = SqlSelect { offset: Some(1), ..select(vec![named(name_of("t0"), "name")], joined()) };
        let outer = build::star_of(build::subselect(build::query(inner), "sub", None)).unwrap();
        let out = ScanOrder::stabilize(build::query(outer), &HashSet::new()).unwrap();
        assert!(out.is_new());
        let s = as_select(out.get());
        assert!(s.order_by.is_empty());
        assert_eq!(inner_select(&s.from).order_by, vec![rowid_key("t0"), rowid_key("t1")]);
        assert_eq!(names(&s.outputs), ["name"]);
    }

    #[test]
    fn an_order_sensitive_aggregate_over_a_scan_orders_by_its_row_order() {
        // SELECT STRING_AGG(t0.name, ',') AS names FROM T t0
        let s = select(vec![named(string_agg(name_of("t0")), "names")], table("T", "t0"));
        let mut pass = StableScanOrder::new();
        let out = pass.rewrite_root(build::query(s)).unwrap();
        let SqlExpr::Reducer(r) = &as_select(&out).projections[0].expr else { panic!("a reducer") };
        assert_eq!(r.order_by, vec![rowid_key("t0")]);
        assert_eq!(r.r#type, TypeFact::Typed { r#type: varchar(), nullable: true });
        assert_eq!(as_select(&out).projections[0].alias.as_deref(), Some("names"));
        assert_eq!(pass.firings, 1);
        // a DISTINCT concat is set-shaped
        let distinct = SqlExpr::Reducer(build::reducer(
            AggFn::StringAgg,
            vec![name_of("t0"), build::string_lit(",")],
            true,
            Vec::new(),
        ));
        let s = select(vec![named(distinct, "names")], table("T", "t0"));
        assert!(matches!(pass.select(s).unwrap(), Rw::Same(_)));
    }

    #[test]
    fn an_order_sensitive_aggregate_over_a_frame_orders_by_its_sort_then_its_threaded_scan() {
        // SELECT STRING_AGG(sub.name, ',') AS names FROM (SELECT t0.name AS name FROM T t0 ORDER BY t0.id DESC) sub
        let inner = SqlSelect {
            order_by: vec![build::sort_key(id_of("t0"), false, None, None)],
            ..select(vec![named(name_of("t0"), "name")], table("T", "t0"))
        };
        let s = select(
            vec![named(string_agg(col("sub", "name", varchar())), "names")],
            build::subselect(build::query(inner), "sub", None),
        );
        let mut pass = StableScanOrder::new();
        let Rw::New(out) = pass.select(s).unwrap() else { panic!("rewritten") };
        let out = as_select(&out);
        let frame = inner_select(&out.from);
        // the scan ordinal threads as a hidden projection, then the unexported sort key is exported hidden; Java's
        // constructor reconciles each slot to its expression's fact (a row number is a non-null BIGINT)
        assert_eq!(names(&frame.outputs), ["name", "__agg_ord0", "__agg_key0"]);
        assert!(!frame.outputs[1].nullable);
        assert_eq!(frame.outputs[1].r#type, SqlType::Scalar(Scalar::Bigint));
        assert_eq!(frame.outputs[2].r#type, SqlType::Scalar(Scalar::Bigint));
        assert_eq!(frame.projections[1].expr, build::row_order(Some("t0".to_owned())));
        assert_eq!(frame.projections[2].expr, id_of("t0"));
        let SqlExpr::Reducer(r) = &out.projections[0].expr else { panic!("a reducer") };
        let key = |name: &str, asc: bool| {
            build::sort_key(SqlExpr::Column(build::column_in(Some("sub"), &frame.outputs, name)), asc, None, None)
        };
        assert_eq!(r.order_by, vec![key("__agg_key0", false), key("__agg_ord0", true)]);
        // the outer select's outputs are rebuilt from its projections
        assert_eq!(names(&out.outputs), ["names"]);
    }

    #[test]
    fn a_union_all_frame_threads_each_legs_index_and_first_scan() {
        let leg = |t: &str, a: &str| build::query(select(vec![named(name_of(a), "name")], table(t, a)));
        let union =
            build::union(vec![leg("T", "t0"), leg("U", "t1")], true, vec![build::output_col("name", varchar(), true)])
                .unwrap();
        let middle = select(
            vec![named(col("u", "name", varchar()), "name")],
            build::subselect(SqlQuery::Union(union), "u", None),
        );
        let s = select(
            vec![named(string_agg(col("m", "name", varchar())), "names")],
            build::subselect(build::query(middle), "m", None),
        );
        let mut pass = StableScanOrder::new();
        let Rw::New(out) = pass.select(s).unwrap() else { panic!("rewritten") };
        let frame = inner_select(&as_select(&out).from);
        assert_eq!(names(&frame.outputs), ["name", "__agg_ord0", "__agg_ord1"]);
        let SqlSource::Subselect { inner, .. } = &frame.from else { panic!("the union frame") };
        let SqlQuery::Union(u) = inner.as_ref() else { panic!("a union") };
        assert_eq!(names(&u.outputs), ["name", "__agg_leg", "__agg_legord"]);
        let second = as_select(&u.branches[1]);
        assert_eq!(second.projections[1].expr, build::int_lit(1));
        assert_eq!(second.projections[2].expr, build::row_order(Some("t1".to_owned())));
        // the frame reads the leg columns as the local list labels them (Java's `Column.of(alias, outs, name)`)
        let leg_ref =
            SqlExpr::Column(build::column_of(Some("u"), &build::output_col("__agg_leg", super::bigint(), true)));
        assert_eq!(frame.projections[1].expr, leg_ref);
    }

    fn with_frame(body: SqlQuery) -> SqlQuery {
        let frame = build::query(select(vec![named(name_of("t0"), "name")], table("T", "t0")));
        let ctes = vec![Cte { name: "f".to_owned(), query: frame, materialized: false }];
        SqlQuery::With(build::with(ctes, body, false).unwrap())
    }

    fn frame_ref() -> SqlSource {
        SqlSource::Cte {
            name: "f".to_owned(),
            alias: "f0".to_owned(),
            outputs: vec![build::output_col("name", varchar(), true)],
        }
    }

    #[test]
    fn a_frame_cte_threads_its_ordinals_when_a_positional_read_needs_them() {
        // WITH f AS (SELECT t0.name AS name FROM T t0) SELECT * FROM (SELECT f0.name AS name FROM f f0 LIMIT 1) s0
        let capped =
            SqlSelect { limit: Some(1), ..select(vec![named(col("f0", "name", varchar()), "name")], frame_ref()) };
        let body = build::star_of(build::subselect(build::query(capped), "s0", None)).unwrap();
        let mut pass = StableScanOrder::new();
        let out = pass.rewrite_root(with_frame(build::query(body))).unwrap();
        let SqlQuery::With(w) = &out else { panic!("a WITH") };
        let frame = as_select(&w.ctes[0].query);
        assert_eq!(names(&frame.outputs), ["name", "__agg_ord0"]);
        let read = inner_select(&as_select(&w.body).from);
        // the reference re-exports the frame's ordinals, and the cap orders by them
        let SqlSource::Cte { outputs, .. } = &read.from else { panic!("the frame reference") };
        assert_eq!(names(outputs), ["name", "__agg_ord0"]);
        let key = SqlExpr::Column(build::column_in(Some("f0"), outputs, "__agg_ord0"));
        assert_eq!(read.order_by, vec![build::sort_key(key, true, None, None)]);
        assert_eq!(pass.firings, 1);
    }

    #[test]
    fn a_frame_cte_stays_bare_when_no_reader_needs_its_order() {
        // WITH f AS (...) SELECT f0.name AS name FROM f f0: read as a multiset, the product plan as it is
        let body = build::query(select(vec![named(col("f0", "name", varchar()), "name")], frame_ref()));
        let q = with_frame(body);
        let mut pass = StableScanOrder::new();
        assert_eq!(pass.rewrite_root(q.clone()).unwrap(), q);
        assert_eq!(pass.firings, 0);
    }

    #[test]
    fn the_engine_scan_order_flag_runs_the_pass_in_duckdbs_pipeline() {
        let s = SqlSelect { limit: Some(2), ..select(vec![named(name_of("t0"), "name")], joined()) };
        let plain = DuckDb::new().render(build::query(s.clone())).unwrap();
        let mut d = DuckDb::new();
        d.engine_scan_order = true;
        let ordered = d.render(build::query(s)).unwrap();
        assert!(!plain.contains("rowid"), "{plain}");
        assert_eq!(
            ordered,
            "SELECT t0.name AS name\nFROM T AS t0\nJOIN U AS t1 ON t0.id = t1.id\nORDER BY t0.rowid NULLS LAST, t1.rowid \
             NULLS LAST\nLIMIT 2"
        );
    }
}
