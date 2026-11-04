// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! The two passes only ClickHouse installs: `ClickHouse.ArrayJoinColumnsNamed` (its nested class) and
//! `OuterJoinResidualsAsMatches` (`core/src/main/java/com/legend/sql/dialect/OuterJoinResidualsAsMatches.java`). Each
//! rebuilds through [`crate::build`] (Java's constructors) and answers [`Rw::Same`] exactly where Java hands back its
//! input.

use crate::build;
use crate::explosion_markers::{is_marker, marker_column};
use crate::refusal::Refusal;
use crate::rewriter::{Rw, SqlRewriter, rewrite};
use crate::typing::{children, source_alias};
use lite_sql::{Column, JoinKind, Projection, SqlExpr, SqlFn, SqlQuery, SqlSelect, SqlSource, TypeFact};

// ==================================================================
// ArrayJoinColumnsNamed
// ==================================================================

/// ClickHouse's `*` does not include an ARRAY JOIN's columns, so a select over a lateral explosion names them after
/// its star, each labeled with its own name (Java's `ClickHouse.ArrayJoinColumnsNamed`).
#[derive(Debug, Default)]
pub struct ArrayJoinColumnsNamed;

/// Each lateral marker's alias in `src`'s joins and its projections, in the order a `LinkedHashMap` keeps them (a
/// repeated alias keeps its first place, its last projections) (Java's `markerAliases`).
fn marker_aliases<'a>(src: &'a SqlSource, into: &mut Vec<(String, &'a [Projection])>) {
    if let SqlSource::Join { left, right, kind, .. } = src {
        marker_aliases(left, into);
        match right.as_ref() {
            SqlSource::Subselect { inner, alias, .. }
                if is_marker(inner) && matches!(kind, JoinKind::CrossLateral | JoinKind::LeftLateral) =>
            {
                let SqlQuery::Select(marker) = inner.as_ref() else { unreachable!("a marker is a select") };
                match into.iter_mut().find(|(a, _)| a == alias) {
                    Some(slot) => slot.1 = &marker.projections,
                    None => into.push((alias.clone(), &marker.projections)),
                }
            }
            other => marker_aliases(other, into),
        }
    }
}

/// `StarExcept`'s record `toString`, which the refusal embeds.
fn star_except_text(table: Option<&str>, except: &[String], t: &TypeFact) -> String {
    format!(
        "StarExcept[table={}, except=[{}], type={}]",
        table.unwrap_or("null"),
        except.join(", "),
        crate::java::type_fact_text(t)
    )
}

impl SqlRewriter for ArrayJoinColumnsNamed {
    fn name(&self) -> &'static str {
        "ClickHouse.ArrayJoinColumnsNamed"
    }

    fn select(&mut self, s: SqlSelect) -> Result<Rw<SqlQuery>, Refusal> {
        let mut found = Vec::new();
        marker_aliases(&s.from, &mut found);
        if found.is_empty() {
            return Ok(Rw::Same(build::query(s)));
        }
        let mut named: Vec<(String, Vec<Projection>)> = Vec::with_capacity(found.len());
        for (alias, ps) in found {
            let mut cols = Vec::with_capacity(ps.len());
            for p in ps {
                let name = marker_column(p)?;
                cols.push(build::projection(build::derived(Some(&alias), &name), Some(name), p.out.clone())?);
            }
            named.push((alias, cols));
        }
        let all: Vec<Projection> = named.iter().flat_map(|(_, ps)| ps.iter().cloned()).collect();
        let mut ps = Vec::new();
        if s.projections.is_empty() {
            ps.push(build::projection(build::star(None), None, None)?);
            ps.extend(all.iter().cloned());
        }
        for p in &s.projections {
            match &p.expr {
                SqlExpr::Star { table: Some(t), .. } if named.iter().any(|(a, _)| a == t) => {
                    let cols = &named.iter().find(|(a, _)| a == t).expect("a marker alias").1;
                    ps.extend(cols.iter().cloned());
                }
                SqlExpr::Star { table: None, .. } => {
                    ps.push(p.clone());
                    ps.extend(all.iter().cloned());
                }
                SqlExpr::StarExcept { table, except, r#type } => {
                    return Err(Refusal::capability(format!(
                        "a star exclusion over a lateral explosion reached ClickHouse: {}",
                        star_except_text(table.as_deref(), except, r#type)
                    )));
                }
                _ => ps.push(p.clone()),
            }
        }
        Ok(Rw::New(build::query(build::select(SqlSelect { projections: ps, ..s })?)))
    }
}

// ==================================================================
// OuterJoinResidualsAsMatches
// ==================================================================

/// A LEFT join whose condition holds a conjunct that reads both sides and is no equality, as its MATCHES (Java's
/// `OuterJoinResidualsAsMatches`): each distinct tuple of the left columns the condition reads INNER-joined to the
/// right side on it, then the left side LEFT-joined to those matches on that tuple, null-safe. A select that projects
/// a star keeps its join.
#[derive(Debug, Default)]
pub struct OuterJoinResidualsAsMatches {
    /// Numbers each rewritten join's helper names across the statement (Java's `next`, reset at the root).
    next: usize,
}

/// `e`'s conjuncts: an AND's operands and a group's inner, each flattened (the pass's own `conjuncts`).
fn conjuncts(e: &SqlExpr) -> Vec<&SqlExpr> {
    match e {
        SqlExpr::Call { r#fn: SqlFn::And, args, .. } => args.iter().flat_map(conjuncts).collect(),
        SqlExpr::Group { inner, .. } => conjuncts(inner),
        other => vec![other],
    }
}

/// The aliases a source defines (Java's `aliases`; a Dual's `alias()` refuses).
fn aliases(s: &SqlSource, out: &mut Vec<String>) -> Result<(), Refusal> {
    if let SqlSource::Join { left, right, .. } = s {
        aliases(left, out)?;
        return aliases(right, out);
    }
    let a = source_alias(s)?.to_owned();
    if !out.contains(&a) {
        out.push(a);
    }
    Ok(())
}

/// The aliases `e`'s columns read, through its children (Java's `tablesRead`).
fn tables_read(e: &SqlExpr, out: &mut Vec<String>) {
    if let SqlExpr::Column(c) = e
        && let Some(t) = &c.table
        && !out.contains(t)
    {
        out.push(t.clone());
    }
    for child in children(e) {
        tables_read(&child, out);
    }
}

fn has_subquery(e: &SqlExpr) -> bool {
    matches!(
        e,
        SqlExpr::ScalarSubquery { .. } | SqlExpr::Exists { .. } | SqlExpr::InSubquery { .. } | SqlExpr::Quantified { .. }
    ) || children(e).iter().any(|c| has_subquery(c))
}

/// Each column of a left alias `e` reads, into `out` (in order, once each); false when one is of no typed slot.
fn left_columns(e: &SqlExpr, lefts: &[String], out: &mut Vec<Column>) -> bool {
    if let SqlExpr::Column(c) = e
        && let Some(t) = &c.table
        && lefts.contains(t)
    {
        if !matches!(c.r#type, TypeFact::Typed { .. }) {
            return false;
        }
        if !out.contains(c) {
            out.push(c.clone());
        }
        return true;
    }
    for child in children(e) {
        if !left_columns(&child, lefts, out) {
            return false;
        }
    }
    true
}

/// `e` with each node a key of `to` replaced by its value, a node rebuilt through `withChildren` only where a child
/// changed (Java's `substitute`): `None` where nothing did.
fn substitute(e: &SqlExpr, to: &[(SqlExpr, SqlExpr)]) -> Result<Option<SqlExpr>, Refusal> {
    if let Some((_, hit)) = to.iter().find(|(k, _)| k == e) {
        return Ok(Some(hit.clone()));
    }
    crate::carrier::map_children(e, &mut |c| substitute(c, to))
}

impl OuterJoinResidualsAsMatches {
    fn joins(&mut self, src: SqlSource) -> Result<Rw<SqlSource>, Refusal> {
        let SqlSource::Join { left, right, kind, on, outputs } = src else {
            return Ok(Rw::Same(src));
        };
        let left_rw = self.joins(*left)?;
        let changed = left_rw.is_new();
        let left = left_rw.into_inner();
        // a right side that is a join stays as it is; any other source is its own `joins`
        let join = if changed {
            build::join(left, *right, kind, on.map(|b| *b))?
        } else {
            SqlSource::Join { left: Box::new(left), right, kind, on, outputs }
        };
        Ok(match self.matched(&join)? {
            Some(m) => Rw::New(m),
            None if changed => Rw::New(join),
            None => Rw::Same(join),
        })
    }

    /// `j` as its matches, or none when it is no LEFT join with a residual both sides read (Java's `matched`).
    fn matched(&mut self, j: &SqlSource) -> Result<Option<SqlSource>, Refusal> {
        let SqlSource::Join { left, right, kind, on, .. } = j else { unreachable!("a join") };
        let right_alias = source_alias(right)?.to_owned();
        let Some(on) = on.as_deref() else {
            return Ok(None);
        };
        if *kind != JoinKind::Left || matches!(right.as_ref(), SqlSource::Join { .. }) || has_subquery(on) {
            return Ok(None);
        }
        let mut lefts = Vec::new();
        aliases(left, &mut lefts)?;
        let mut residual = false;
        for c in conjuncts(on) {
            let mut read = Vec::new();
            tables_read(c, &mut read);
            let both = read.contains(&right_alias) && read.iter().any(|t| lefts.contains(t));
            if both && !matches!(c, SqlExpr::Call { r#fn: SqlFn::Equal, .. }) {
                residual = true;
            }
        }
        if !residual {
            return Ok(None);
        }
        // the left columns the condition reads, each typed
        let mut refs = Vec::new();
        if !left_columns(on, &lefts, &mut refs) {
            return Ok(None);
        }
        let stem = format!("__oj{}", self.next);
        self.next += 1;
        let keys_alias = format!("{stem}l");
        let mut key_cols = Vec::new();
        let mut match_cols = Vec::new();
        let mut to_keys: Vec<(SqlExpr, SqlExpr)> = Vec::new();
        let mut rejoin = Vec::new();
        for (i, c) in refs.iter().enumerate() {
            let TypeFact::Typed { r#type, .. } = &c.r#type else { unreachable!("a typed column") };
            let key = build::output_col(&format!("{stem}c{i}"), r#type.clone(), true);
            let matching = build::output_col(&format!("{stem}k{i}"), r#type.clone(), true);
            key_cols.push(build::projection(SqlExpr::Column(c.clone()), Some(key.name.clone()), Some(key.clone()))?);
            let key_read = SqlExpr::Column(build::column_of(Some(&keys_alias), &key));
            to_keys.push((SqlExpr::Column(c.clone()), key_read.clone()));
            match_cols.push(build::projection(key_read, Some(matching.name.clone()), Some(matching.clone()))?);
            rejoin.push(build::call(
                SqlFn::NullSafeEqual,
                vec![SqlExpr::Column(c.clone()), SqlExpr::Column(build::column_of(Some(&right_alias), &matching))],
            ));
        }
        match_cols.push(build::projection(build::star(Some(&right_alias)), None, None)?);
        let keys = build::with_distinct(build::with_projections(build::star_of(left.as_ref().clone())?, key_cols)?)?;
        let condition = substitute(on, &to_keys)?.unwrap_or_else(|| on.clone());
        let inner = build::join(
            build::subselect(build::query(keys), &keys_alias, None),
            right.as_ref().clone(),
            JoinKind::Inner,
            Some(condition),
        )?;
        let matches = build::with_projections(build::star_of(inner)?, match_cols)?;
        let mut rejoin = rejoin.into_iter();
        let mut key_on = rejoin.next().expect("a left column");
        for k in rejoin {
            key_on = build::call(SqlFn::And, vec![key_on, k]);
        }
        Ok(Some(build::join(
            left.as_ref().clone(),
            build::subselect(build::query(matches), &right_alias, None),
            JoinKind::Left,
            Some(key_on),
        )?))
    }
}

impl SqlRewriter for OuterJoinResidualsAsMatches {
    fn name(&self) -> &'static str {
        "OuterJoinResidualsAsMatches"
    }

    fn rewrite_root(&mut self, q: SqlQuery) -> Result<SqlQuery, Refusal> {
        self.next = 0;
        Ok(rewrite(self, q)?.into_inner())
    }

    fn select(&mut self, s: SqlSelect) -> Result<Rw<SqlQuery>, Refusal> {
        if !matches!(s.from, SqlSource::Join { .. })
            || s.projections.iter().any(|p| matches!(p.expr, SqlExpr::Star { .. } | SqlExpr::StarExcept { .. }))
        {
            return Ok(Rw::Same(build::query(s)));
        }
        let SqlSelect { projections, distinct, from, r#where, group_by, having, qualify, order_by, limit, offset, outputs } =
            s;
        match self.joins(from)? {
            Rw::Same(from) => Ok(Rw::Same(build::query(SqlSelect {
                projections,
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
            }))),
            Rw::New(from) => {
                let s = SqlSelect {
                    projections,
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
                };
                Ok(Rw::New(build::query(build::select(s)?)))
            }
        }
    }
}
