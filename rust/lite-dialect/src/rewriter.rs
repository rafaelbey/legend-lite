// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! Java's `SqlRewriter` (`core/src/main/java/com/legend/sql/SqlRewriter.java`): the one structural MIR-to-MIR walk the
//! dialect passes share -- a bottom-up deep map whose hooks fire after a node's children were rewritten, each hook
//! the identity by default. A pass overrides the hooks it cares about, or `rewrite_root` to see the statement's root
//! first.
//!
//! The walk takes the tree by value and hands back the rewritten one. Two of Java's walk's effects are not here yet,
//! for the passes' port to settle:
//!
//! - Java rebuilds a node whose child changed through its constructor, and the constructors of `Call`, `Case`,
//!   `Cast`, `FoldCall`, `Membership`, `ReduceCollection`, `OrderedListAgg`, `Reducer` and `Join` recompute the node's
//!   type fact or outputs (`SqlTyping`); this walk keeps the node's fact as it was.
//! - Java keeps a node's identity when nothing below it changed (`==`), which some passes test; this walk has no
//!   identity to keep.

use crate::refusal::Refusal;
use lite_sql::{
    Cte, Hoist, JsonArrayAggKey, PivotUsing, Projection, Reducer, SortKey, SqlAgg, SqlExpr, SqlQuery, SqlSelect,
    SqlSource, SqlUnion, SqlWith, StructLitField, TypeFact, When,
};

/// A MIR pass (Java's `SqlRewriter` subclass).
pub trait SqlRewriter {
    /// The Java class the pass is.
    fn name(&self) -> &'static str;

    /// The renderer's entry: sees the statement root before the deep walk. Default: the plain deep rewrite.
    fn rewrite_root(&mut self, q: SqlQuery) -> Result<SqlQuery, Refusal> {
        rewrite(self, q)
    }

    /// A select, after its clauses were rewritten.
    fn select(&mut self, s: SqlSelect) -> Result<SqlQuery, Refusal> {
        Ok(SqlQuery::Select(Box::new(s)))
    }

    /// A union, after its branches were rewritten.
    fn union(&mut self, u: SqlUnion) -> Result<SqlQuery, Refusal> {
        Ok(SqlQuery::Union(u))
    }

    /// A source, after its children were rewritten.
    fn source(&mut self, s: SqlSource) -> Result<SqlSource, Refusal> {
        Ok(s)
    }

    /// An expression, after its children were rewritten.
    fn expr(&mut self, e: SqlExpr) -> Result<SqlExpr, Refusal> {
        Ok(e)
    }
}

/// Java's `rewrite(SqlQuery)`.
pub fn rewrite<R: SqlRewriter + ?Sized>(r: &mut R, q: SqlQuery) -> Result<SqlQuery, Refusal> {
    match q {
        SqlQuery::Select(s) => {
            let s = *s;
            let projections = s
                .projections
                .into_iter()
                .map(|p| Ok(Projection { expr: rewrite_expr(r, p.expr)?, alias: p.alias, out: p.out }))
                .collect::<Result<_, Refusal>>()?;
            let from = rewrite_source(r, s.from)?;
            let r#where = s.r#where.map(|e| rewrite_expr(r, e)).transpose()?;
            let group_by = rewrite_exprs(r, s.group_by)?;
            let having = s.having.map(|e| rewrite_expr(r, e)).transpose()?;
            let qualify = s.qualify.map(|e| rewrite_expr(r, e)).transpose()?;
            let order_by = rewrite_keys(r, s.order_by)?;
            r.select(SqlSelect { projections, from, r#where, group_by, having, qualify, order_by, ..s })
        }
        SqlQuery::Union(u) => {
            let branches = u.branches.into_iter().map(|b| rewrite(r, b)).collect::<Result<_, _>>()?;
            r.union(SqlUnion { branches, ..u })
        }
        SqlQuery::With(w) => {
            let ctes = w
                .ctes
                .into_iter()
                .map(|c| Ok(Cte { query: rewrite(r, c.query)?, ..c }))
                .collect::<Result<_, Refusal>>()?;
            let body = Box::new(rewrite(r, *w.body)?);
            Ok(SqlQuery::With(SqlWith { ctes, body, recursive: w.recursive }))
        }
    }
}

fn rewrite_exprs<R: SqlRewriter + ?Sized>(r: &mut R, es: Vec<SqlExpr>) -> Result<Vec<SqlExpr>, Refusal> {
    es.into_iter().map(|e| rewrite_expr(r, e)).collect()
}

fn rewrite_keys<R: SqlRewriter + ?Sized>(r: &mut R, ks: Vec<SortKey>) -> Result<Vec<SortKey>, Refusal> {
    ks.into_iter().map(|k| Ok(SortKey { expr: rewrite_expr(r, k.expr)?, ..k })).collect()
}

/// A boxed child rewritten in its box.
fn boxed<R: SqlRewriter + ?Sized>(r: &mut R, mut e: Box<SqlExpr>) -> Result<Box<SqlExpr>, Refusal> {
    let child = std::mem::replace(&mut *e, SqlExpr::NullLit { r#type: TypeFact::Bottom });
    *e = rewrite_expr(r, child)?;
    Ok(e)
}

/// A boxed subquery rewritten in its box.
fn subquery<R: SqlRewriter + ?Sized>(r: &mut R, mut q: Box<SqlQuery>) -> Result<Box<SqlQuery>, Refusal> {
    let child =
        std::mem::replace(&mut *q, SqlQuery::Union(SqlUnion { branches: Vec::new(), all: false, outputs: Vec::new() }));
    *q = rewrite(r, child)?;
    Ok(q)
}

/// Java's `rewriteSource`.
pub fn rewrite_source<R: SqlRewriter + ?Sized>(r: &mut R, s: SqlSource) -> Result<SqlSource, Refusal> {
    let out = match s {
        SqlSource::Subselect { inner, alias, frame_name } => {
            SqlSource::Subselect { inner: subquery(r, inner)?, alias, frame_name }
        }
        SqlSource::Values { rows, columns, alias, outputs } => SqlSource::Values {
            rows: rows.into_iter().map(|row| rewrite_exprs(r, row)).collect::<Result<_, _>>()?,
            columns,
            alias,
            outputs,
        },
        SqlSource::Join { left, right, kind, on, outputs } => SqlSource::Join {
            left: Box::new(rewrite_source(r, *left)?),
            right: Box::new(rewrite_source(r, *right)?),
            kind,
            on: on.map(|e| boxed(r, e)).transpose()?,
            outputs,
        },
        SqlSource::Pivot { source, on, r#in, named, usings, alias, outputs } => SqlSource::Pivot {
            source: Box::new(rewrite_source(r, *source)?),
            on: rewrite_exprs(r, on)?,
            r#in: rewrite_exprs(r, r#in)?,
            named: rewrite_exprs(r, named)?,
            usings: usings
                .into_iter()
                .map(|u| match rewrite_expr(r, SqlExpr::Reducer(u.agg))? {
                    SqlExpr::Reducer(agg) => Ok(PivotUsing { agg, ..u }),
                    other => Err(Refusal::class_cast(crate::java::simple_name(&other), "SqlAgg$Reducer")),
                })
                .collect::<Result<_, Refusal>>()?,
            alias,
            outputs,
        },
        // the leaves: a table, a CTE reference, a URL, a plan variable, carried text, a catalog grid, Dual
        leaf => leaf,
    };
    r.source(out)
}

/// Java's `rewriteExpr`.
pub fn rewrite_expr<R: SqlRewriter + ?Sized>(r: &mut R, e: SqlExpr) -> Result<SqlExpr, Refusal> {
    let out = match e {
        SqlExpr::StoredRead { column, stored, r#type } => match rewrite_expr(r, SqlExpr::Column(column))? {
            SqlExpr::Column(column) => SqlExpr::StoredRead { column, stored, r#type },
            other => {
                return Err(Refusal::illegal_state(format!(
                    "a pass rewrote a stored read's column into {}: a stored read reads a table column, nothing else",
                    crate::java::expr_text(&other)
                )));
            }
        },
        SqlExpr::Membership { needle, collection, r#type } => {
            SqlExpr::Membership { needle: boxed(r, needle)?, collection: boxed(r, collection)?, r#type }
        }
        SqlExpr::ReduceCollection { reducer, collection, extras, r#type } => SqlExpr::ReduceCollection {
            reducer,
            collection: boxed(r, collection)?,
            extras: rewrite_exprs(r, extras)?,
            r#type,
        },
        SqlExpr::Group { inner, r#type } => SqlExpr::Group { inner: boxed(r, inner)?, r#type },
        SqlExpr::OrderedListAgg { value, order_by, r#type } => {
            SqlExpr::OrderedListAgg { value: boxed(r, value)?, order_by: boxed(r, order_by)?, r#type }
        }
        SqlExpr::ArrayLit { elements, r#type } => SqlExpr::ArrayLit { elements: rewrite_exprs(r, elements)?, r#type },
        SqlExpr::StructLit { fields, r#type } => SqlExpr::StructLit {
            fields: fields
                .into_iter()
                .map(|f| Ok(StructLitField { value: rewrite_expr(r, f.value)?, ..f }))
                .collect::<Result<_, Refusal>>()?,
            r#type,
        },
        SqlExpr::StructGet { source, field, r#type } => SqlExpr::StructGet { source: boxed(r, source)?, field, r#type },
        SqlExpr::Call { r#fn, args, r#type } => SqlExpr::Call { r#fn, args: rewrite_exprs(r, args)?, r#type },
        SqlExpr::Case { whens, otherwise, r#type } => SqlExpr::Case {
            whens: whens
                .into_iter()
                .map(|w| Ok(When { condition: rewrite_expr(r, w.condition)?, then: rewrite_expr(r, w.then)? }))
                .collect::<Result<_, Refusal>>()?,
            otherwise: otherwise.map(|o| boxed(r, o)).transpose()?,
            r#type,
        },
        SqlExpr::Exists { subquery: q, r#type } => SqlExpr::Exists { subquery: subquery(r, q)?, r#type },
        SqlExpr::InSubquery { value, subquery: q, r#type } => {
            SqlExpr::InSubquery { value: boxed(r, value)?, subquery: subquery(r, q)?, r#type }
        }
        SqlExpr::Quantified { value, comparison, quantifier, subquery: q, r#type } => {
            SqlExpr::Quantified { value: boxed(r, value)?, comparison, quantifier, subquery: subquery(r, q)?, r#type }
        }
        SqlExpr::CheckedDefects { own, hoists, r#type } => SqlExpr::CheckedDefects {
            own: boxed(r, own)?,
            hoists: hoists
                .into_iter()
                .map(|h| Ok(Hoist { envelope: rewrite_expr(r, h.envelope)?, ..h }))
                .collect::<Result<_, Refusal>>()?,
            r#type,
        },
        SqlExpr::CheckedChildValue { envelope, to_many, r#type } => {
            SqlExpr::CheckedChildValue { envelope: boxed(r, envelope)?, to_many, r#type }
        }
        // rewriteExpr, not the hook alone: a pass reaches the calls nested under the wrapper
        SqlExpr::CheckedOne { list, scalar_carrier, at_least_only, r#type } => {
            SqlExpr::CheckedOne { list: boxed(r, list)?, scalar_carrier, at_least_only, r#type }
        }
        SqlExpr::CompactList { list, r#type } => SqlExpr::CompactList { list: boxed(r, list)?, r#type },
        SqlExpr::ScalarSubquery { subquery: q, r#type } => {
            SqlExpr::ScalarSubquery { subquery: subquery(r, q)?, r#type }
        }
        SqlExpr::DeferredTdsString { inner, alias, id, form, render_tds_null, r#type } => {
            match rewrite(r, SqlQuery::Select(inner))? {
                SqlQuery::Select(inner) => {
                    SqlExpr::DeferredTdsString { inner, alias, id, form, render_tds_null, r#type }
                }
                _ => return Err(Refusal::class_cast("SqlUnion", "SqlSelect")),
            }
        }
        SqlExpr::JsonObject { kv, r#type } => SqlExpr::JsonObject { kv: rewrite_exprs(r, kv)?, r#type },
        SqlExpr::JsonArray { elements, r#type } => SqlExpr::JsonArray { elements: rewrite_exprs(r, elements)?, r#type },
        SqlExpr::JsonArrayAgg { value, order_keys, r#type } => SqlExpr::JsonArrayAgg {
            value: boxed(r, value)?,
            order_keys: order_keys
                .into_iter()
                .map(|k| Ok(JsonArrayAggKey { expr: rewrite_expr(r, k.expr)?, desc: k.desc }))
                .collect::<Result<_, Refusal>>()?,
            r#type,
        },
        SqlExpr::WindowCall { r#fn, partition_by, order_by, frame, r#type } => SqlExpr::WindowCall {
            r#fn: Box::new(rewrite_agg(r, *r#fn)?),
            partition_by: rewrite_exprs(r, partition_by)?,
            order_by: rewrite_keys(r, order_by)?,
            frame,
            r#type,
        },
        SqlExpr::Lambda(mut l) => {
            l.body = boxed(r, l.body)?;
            SqlExpr::Lambda(l)
        }
        SqlExpr::Cast { value, target, conform, r#type } => {
            SqlExpr::Cast { value: boxed(r, value)?, target, conform, r#type }
        }
        SqlExpr::FoldCall { source, lambda, init, acc_is_list, homogeneous, r#type } => {
            let source = boxed(r, source)?;
            let lambda = match rewrite_expr(r, SqlExpr::Lambda(lambda))? {
                SqlExpr::Lambda(l) => l,
                other => return Err(Refusal::class_cast(crate::java::simple_name(&other), "Lambda")),
            };
            SqlExpr::FoldCall { source, lambda, init: boxed(r, init)?, acc_is_list, homogeneous, r#type }
        }
        SqlExpr::Reducer(red) => SqlExpr::Reducer(rewrite_reducer(r, red)?),
        // the leaves
        leaf => leaf,
    };
    r.expr(out)
}

fn rewrite_reducer<R: SqlRewriter + ?Sized>(r: &mut R, red: Reducer) -> Result<Reducer, Refusal> {
    Ok(Reducer { args: rewrite_exprs(r, red.args)?, order_by: rewrite_keys(r, red.order_by)?, ..red })
}

/// Java's `rewriteAgg`: a reducer, a ranking or a value function, its arguments rewritten.
pub fn rewrite_agg<R: SqlRewriter + ?Sized>(r: &mut R, a: SqlAgg) -> Result<SqlAgg, Refusal> {
    Ok(match a {
        SqlAgg::Reducer(red) => SqlAgg::Reducer(rewrite_reducer(r, red)?),
        SqlAgg::RankingFn { r#fn, args } => SqlAgg::RankingFn { r#fn, args: rewrite_exprs(r, args)? },
        SqlAgg::ValueFn { r#fn, args } => SqlAgg::ValueFn { r#fn, args: rewrite_exprs(r, args)? },
    })
}
