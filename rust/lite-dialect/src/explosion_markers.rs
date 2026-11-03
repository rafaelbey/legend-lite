// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! Java's `ExplosionMarkers` (`core/src/main/java/com/legend/sql/dialect/ExplosionMarkers.java`): for a database with
//! no set-returning projection, every list explosion becomes a MARKER -- a FROM-less select whose projections are each
//! one list's `UNNEST` -- placed in a FROM, where the dialect writes it over the positions of its longest list (SQLite a
//! `json_each`, Snowflake a `FLATTEN`, BigQuery an `UNNEST(GENERATE_ARRAY(..))`) and reads each exploded column as its
//! list's element at the position. The three passes place the markers; the predicates name them. SQLite installs them
//! first ([`crate::sqlite`]); ClickHouse, MariaDB, Snowflake, Trino and BigQuery install them too.
//!
//! Each pass rebuilds through [`crate::build`] (Java's constructors: a join's delivered columns, a select's outputs, a
//! projection's attached output normalized) and answers [`Rw::Same`] exactly where Java hands back its input.

use crate::build;
use crate::refusal::{Refusal, arg};
use crate::rewriter::{Rw, SqlRewriter};
use crate::typing::children;
use lite_sql::{
    AggFn, JoinKind, Origin, Projection, Scalar, SqlExpr, SqlFn, SqlQuery, SqlSelect, SqlSource, SqlType, TypeFact,
};

// ==================================================================
// The predicates
// ==================================================================

/// The read of an exploded column, as its slot wants it (Java's `explodedRead`): its element's type when known; else
/// the slot's when the slot is scalar or the list is NULL (an exploded NULL list has no element type), never a
/// composite slot over a real list. A JSON element under a VARCHAR slot is its JSON text, cast.
pub fn exploded_read(alias: &str, p: &Projection, slot: Option<&SqlType>) -> Result<SqlExpr, Refusal> {
    let name = marker_column(p)?;
    let element = match p.expr.r#type() {
        TypeFact::Typed { r#type, .. } => Some(r#type),
        _ => None,
    };
    let list = unnest_list(&p.expr)?;
    let t = match element {
        Some(t) => Some(t),
        None if matches!(slot, Some(SqlType::Scalar(_))) || null_list(list) => slot,
        None => None,
    };
    let read = match t {
        None => build::derived(Some(alias), &name),
        Some(t) => build::typed_column(Some(alias), &name, t.clone(), true, Origin::Derived),
    };
    Ok(if t == Some(&SqlType::Scalar(Scalar::Json)) && slot == Some(&SqlType::Scalar(Scalar::Varchar)) {
        build::cast(read, SqlType::Scalar(Scalar::Varchar))
    } else {
        read
    })
}

/// `((SqlExpr.Call) e).args().get(0)`: the list a marker's projection explodes.
pub fn unnest_list(e: &SqlExpr) -> Result<&SqlExpr, Refusal> {
    match e {
        SqlExpr::Call { args, .. } => arg(args, 0),
        other => Err(Refusal::class_cast(crate::java::simple_name(other), "Call")),
    }
}

/// A list's element, min or max over the NULL list: NULL (Java's `nullOverNullList`).
pub fn null_over_null_list(e: &SqlExpr) -> bool {
    if let SqlExpr::Cast { value, .. } = e
        && !matches!(value.as_ref(), SqlExpr::NullLit { .. })
    {
        return null_over_null_list(value);
    }
    match e {
        SqlExpr::Call { r#fn: SqlFn::ListMin | SqlFn::ListMax | SqlFn::ListGet, args, .. } => {
            args.first().is_some_and(null_list)
        }
        SqlExpr::ReduceCollection { reducer: AggFn::Min | AggFn::Max, collection, .. } => null_list(collection),
        _ => false,
    }
}

/// The NULL list, compacted or not (Java's `nullList`).
pub fn null_list(e: &SqlExpr) -> bool {
    match e {
        SqlExpr::NullLit { .. } => true,
        SqlExpr::CompactList { list, .. } => null_list(list),
        _ => false,
    }
}

/// A select that only explodes lists: every projection a one-list UNNEST, FROM nothing, no clause (Java's
/// `isMarker`).
pub fn is_marker(q: &SqlQuery) -> bool {
    matches!(q, SqlQuery::Select(s) if is_marker_select(s))
}

/// [`is_marker`] of a select.
pub fn is_marker_select(s: &SqlSelect) -> bool {
    matches!(s.from, SqlSource::Dual)
        && !s.projections.is_empty()
        && s.projections.iter().all(|p| is_unnest(&p.expr))
        && s.r#where.is_none()
        && s.group_by.is_empty()
        && s.having.is_none()
        && s.qualify.is_none()
        && s.order_by.is_empty()
        && s.limit.is_none()
        && s.offset.is_none()
        && !s.distinct
}

/// A one-list UNNEST call (Java's `isUnnest`).
pub fn is_unnest(e: &SqlExpr) -> bool {
    matches!(e, SqlExpr::Call { r#fn: SqlFn::Unnest, args, .. } if args.len() == 1)
}

/// The column a marker's projection names: its alias, else its output's name (Java's `markerColumn`).
pub fn marker_column(p: &Projection) -> Result<String, Refusal> {
    match (&p.alias, &p.out) {
        (Some(alias), _) => Ok(alias.clone()),
        (None, Some(out)) => Ok(out.name.clone()),
        (None, None) => Err(Refusal::capability("an UNNEST without a column name reached the renderer")),
    }
}

/// A select over no source with every other clause empty (Java's eleven-argument `new SqlSelect` so).
fn dual_select(projections: Vec<Projection>) -> Result<SqlSelect, Refusal> {
    build::select_of(projections, SqlSource::Dual, Vec::new())
}

// ==================================================================
// StarOverExplosionUnwrapped
// ==================================================================

/// `(SELECT * FROM (marker) AS t0) AS t1` is the marker itself, as `t1` (Java's `StarOverExplosionUnwrapped`):
/// Snowflake refuses a correlated lateral subquery, and a FLATTEN joins laterally only bare.
#[derive(Debug, Default)]
pub struct StarOverExplosionUnwrapped;

fn plain_star(s: &SqlSelect) -> bool {
    s.projections.is_empty()
        && !s.distinct
        && s.r#where.is_none()
        && s.group_by.is_empty()
        && s.having.is_none()
        && s.qualify.is_none()
        && s.order_by.is_empty()
        && s.limit.is_none()
        && s.offset.is_none()
}

impl SqlRewriter for StarOverExplosionUnwrapped {
    fn name(&self) -> &'static str {
        "ExplosionMarkers.StarOverExplosionUnwrapped"
    }

    fn source(&mut self, s: SqlSource) -> Result<Rw<SqlSource>, Refusal> {
        let SqlSource::Subselect { inner: outer_inner, alias, frame_name } = s else {
            return Ok(Rw::Same(s));
        };
        let unwrap = match outer_inner.as_ref() {
            SqlQuery::Select(star) if plain_star(star) => {
                matches!(&star.from, SqlSource::Subselect { inner, .. } if is_marker(inner))
            }
            _ => false,
        };
        if !unwrap {
            return Ok(Rw::Same(SqlSource::Subselect { inner: outer_inner, alias, frame_name }));
        }
        let SqlQuery::Select(star) = *outer_inner else { unreachable!("matched a select above") };
        let SqlSource::Subselect { inner, .. } = star.from else { unreachable!("matched a subselect above") };
        Ok(Rw::New(build::subselect(*inner, &alias, frame_name)))
    }
}

// ==================================================================
// LateralUnnestAsMarker
// ==================================================================

/// A LATERAL explosion of a carried list (`CROSS/LEFT JOIN LATERAL (SELECT UNNEST(c.lst) AS elem FROM (SELECT <list>
/// AS lst) AS c)`) as a MARKER -- `(SELECT UNNEST(<list>) AS elem)` -- which the writer spells as a lateral explosion
/// of its own (Java's `LateralUnnestAsMarker`): the carry is inlined and nothing is nested.
#[derive(Debug, Default)]
pub struct LateralUnnestAsMarker;

/// Java's `SqlSelect.Projection.outputName()`: its alias, else a column's (or a stored read's) name.
fn output_name(p: &Projection) -> Option<&str> {
    if let Some(alias) = &p.alias {
        return Some(alias);
    }
    match &p.expr {
        SqlExpr::Column(c) => Some(&c.name),
        SqlExpr::StoredRead { column, .. } => Some(&column.name),
        _ => None,
    }
}

/// `inner` with its carry's lists inlined, or none when it is not an explosion of a carry (Java's `inlined`).
fn inlined(inner: &SqlSelect) -> Result<Option<SqlSelect>, Refusal> {
    let SqlSource::Subselect { inner: carry_q, alias: carry_alias, .. } = &inner.from else {
        return Ok(None);
    };
    let SqlQuery::Select(c) = carry_q.as_ref() else {
        return Ok(None);
    };
    if !matches!(c.from, SqlSource::Dual)
        || c.r#where.is_some()
        || !c.group_by.is_empty()
        || inner.r#where.is_some()
        || !inner.group_by.is_empty()
        || inner.having.is_some()
        || inner.qualify.is_some()
        || !inner.order_by.is_empty()
        || inner.limit.is_some()
        || inner.offset.is_some()
        || inner.distinct
        || inner.projections.is_empty()
    {
        return Ok(None);
    }
    // Java's LinkedHashMap: a repeated name's later value replaces the earlier in place
    let mut carried: Vec<(&str, &SqlExpr)> = Vec::new();
    for p in &c.projections {
        let Some(name) = output_name(p) else {
            return Ok(None);
        };
        match carried.iter_mut().find(|(n, _)| *n == name) {
            Some(slot) => slot.1 = &p.expr,
            None => carried.push((name, &p.expr)),
        }
    }
    let mut ps = Vec::with_capacity(inner.projections.len());
    for p in &inner.projections {
        if !is_unnest(&p.expr) {
            return Ok(None);
        }
        let SqlExpr::Column(col) = unnest_list(&p.expr)? else {
            return Ok(None);
        };
        if col.table.as_deref() != Some(carry_alias.as_str()) {
            return Ok(None);
        }
        let Some((_, list)) = carried.iter().find(|(n, _)| *n == col.name) else {
            return Ok(None);
        };
        ps.push(build::projection(build::call(SqlFn::Unnest, vec![(*list).clone()]), p.alias.clone(), p.out.clone())?);
    }
    Ok(Some(dual_select(ps)?))
}

impl SqlRewriter for LateralUnnestAsMarker {
    fn name(&self) -> &'static str {
        "ExplosionMarkers.LateralUnnestAsMarker"
    }

    fn source(&mut self, s: SqlSource) -> Result<Rw<SqlSource>, Refusal> {
        let marker = match &s {
            SqlSource::Join { right, kind: JoinKind::CrossLateral | JoinKind::LeftLateral, .. } => match right.as_ref() {
                SqlSource::Subselect { inner, .. } => match inner.as_ref() {
                    SqlQuery::Select(inner) if !is_marker_select(inner) => inlined(inner)?,
                    _ => None,
                },
                _ => None,
            },
            _ => None,
        };
        let Some(marker) = marker else {
            return Ok(Rw::Same(s));
        };
        let SqlSource::Join { left, right, kind, on, .. } = s else { unreachable!("matched a join above") };
        let SqlSource::Subselect { alias, frame_name, .. } = *right else { unreachable!("matched a subselect above") };
        let right = build::subselect(build::query(marker), &alias, frame_name);
        Ok(Rw::New(build::join(*left, right, kind, on.map(|o| *o))?))
    }
}

// ==================================================================
// SelectListUnnestAsLateral
// ==================================================================

/// A select-list UNNEST over a FROM (DuckDB explodes it in place: `SELECT k, UNNEST(xs) AS x FROM t`) as a lateral
/// MARKER joined to the FROM, the projection reading its element (Java's `SelectListUnnestAsLateral`). Several
/// UNNESTs explode in parallel (one marker). A select that groups, or whose list holds an aggregate, is left as it is:
/// the writer refuses it by name. A bare marker is the writer's. Its markers are named `__unnest<n>`, `n` counted by
/// the pass object from 0 (a fresh pass for each render).
#[derive(Debug, Default)]
pub struct SelectListUnnestAsLateral {
    next: usize,
}

/// An aggregate or a window anywhere in `e` (Java's `aggregates`).
fn aggregates(e: &SqlExpr) -> bool {
    if matches!(e, SqlExpr::Reducer(_) | SqlExpr::WindowCall { .. }) {
        return true;
    }
    children(e).iter().any(|k| aggregates(k))
}

impl SqlRewriter for SelectListUnnestAsLateral {
    fn name(&self) -> &'static str {
        "ExplosionMarkers.SelectListUnnestAsLateral"
    }

    fn select(&mut self, s: SqlSelect) -> Result<Rw<SqlQuery>, Refusal> {
        if is_marker_select(&s)
            || !s.projections.iter().any(|p| is_unnest(&p.expr))
            || !s.group_by.is_empty()
            || s.having.is_some()
            || s.qualify.is_some()
        {
            return Ok(Rw::Same(build::query(s)));
        }
        for p in &s.projections {
            if is_unnest(&p.expr) && aggregates(unnest_list(&p.expr)?) {
                return Ok(Rw::Same(build::query(s)));
            }
        }
        let alias = format!("__unnest{}", self.next);
        self.next += 1;
        let mut unnests = Vec::new();
        let mut ps = Vec::with_capacity(s.projections.len());
        for p in &s.projections {
            if !is_unnest(&p.expr) {
                ps.push(p.clone());
                continue;
            }
            let name = marker_column(p)?;
            unnests.push(build::projection(p.expr.clone(), Some(name), p.out.clone())?);
            let slot = p.out.as_ref().map(|o| &o.r#type);
            ps.push(build::projection(exploded_read(&alias, p, slot)?, p.alias.clone(), p.out.clone())?);
        }
        let marker = build::subselect(build::query(dual_select(unnests)?), &alias, None);
        let SqlSelect { distinct, from, r#where, group_by, having, qualify, order_by, limit, offset, outputs, .. } = s;
        let from = match from {
            SqlSource::Dual => marker,
            other => build::join(other, marker, JoinKind::CrossLateral, None)?,
        };
        let out = build::select(SqlSelect {
            projections: ps,
            distinct,
            from,
            r#where,
            group_by,
            having,
            qualify,
            order_by,
            limit,
            offset,
            outputs,
        })?;
        Ok(Rw::New(build::query(out)))
    }
}

#[cfg(test)]
mod tests;
