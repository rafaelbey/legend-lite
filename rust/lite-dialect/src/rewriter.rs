// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! Java's `SqlRewriter` (`core/src/main/java/com/legend/sql/SqlRewriter.java`): the one structural MIR-to-MIR walk the
//! dialect passes share -- a bottom-up deep map whose hooks fire after a node's children were rewritten, each hook
//! the identity by default. A pass overrides the hooks it cares about, or `rewrite_root` to see the statement's root
//! first.
//!
//! Java's walk preserves identity: a node none of whose children changed (`==`) is handed on as it is, and one whose
//! child changed is rebuilt through its record constructor, which recomputes its type fact (`SqlTyping`), a select's
//! outputs, a union's labels, a join's delivered columns. Rust has no identity to compare, so every step says which
//! it is: [`Rw::Same`] is Java's `return e` (the very node it was handed), [`Rw::New`] any other node. A hook that
//! hands back what it was given answers `Same`; the walk rebuilds a parent through [`crate::build`] exactly where Java
//! would, so a rewritten tree carries Java's facts.

use crate::build;
use crate::refusal::Refusal;
use lite_sql::{
    Cte, Hoist, JsonArrayAggKey, PivotUsing, Projection, Reducer, SortKey, SqlAgg, SqlExpr, SqlQuery, SqlSelect,
    SqlSource, SqlUnion, SqlWith, StructLitField, TypeFact, When,
};

/// A step's result: the node it was handed (Java's `==`), or another.
#[derive(Debug, Clone, PartialEq)]
pub enum Rw<T> {
    /// The node as it was handed in: Java's `return e`.
    Same(T),
    /// A node Java's `==` tells apart from the one handed in (a rebuild, a replacement, even an equal copy).
    New(T),
}

impl<T> Rw<T> {
    /// The node.
    pub fn into_inner(self) -> T {
        match self {
            Rw::Same(t) | Rw::New(t) => t,
        }
    }

    /// The node, by reference.
    pub fn get(&self) -> &T {
        match self {
            Rw::Same(t) | Rw::New(t) => t,
        }
    }

    /// Whether it is another node than the one handed in.
    pub fn is_new(&self) -> bool {
        matches!(self, Rw::New(_))
    }

    /// The node, `changed` set when it is another.
    pub fn take(self, changed: &mut bool) -> T {
        *changed |= self.is_new();
        self.into_inner()
    }

    /// `New` whatever it was, when `rebuilt`.
    fn after(self, rebuilt: bool) -> Rw<T> {
        if rebuilt { Rw::New(self.into_inner()) } else { self }
    }
}

/// A MIR pass (Java's `SqlRewriter` subclass).
pub trait SqlRewriter {
    /// The Java class the pass is.
    fn name(&self) -> &'static str;

    /// The renderer's entry: sees the statement root before the deep walk. Default: the plain deep rewrite.
    fn rewrite_root(&mut self, q: SqlQuery) -> Result<SqlQuery, Refusal> {
        Ok(rewrite(self, q)?.into_inner())
    }

    /// A select, after its clauses were rewritten.
    fn select(&mut self, s: SqlSelect) -> Result<Rw<SqlQuery>, Refusal> {
        Ok(Rw::Same(SqlQuery::Select(Box::new(s))))
    }

    /// A union, after its branches were rewritten.
    fn union(&mut self, u: SqlUnion) -> Result<Rw<SqlQuery>, Refusal> {
        Ok(Rw::Same(SqlQuery::Union(u)))
    }

    /// A source, after its children were rewritten.
    fn source(&mut self, s: SqlSource) -> Result<Rw<SqlSource>, Refusal> {
        Ok(Rw::Same(s))
    }

    /// An expression, after its children were rewritten.
    fn expr(&mut self, e: SqlExpr) -> Result<Rw<SqlExpr>, Refusal> {
        Ok(Rw::Same(e))
    }
}

/// Java's `rewrite(SqlQuery)`.
pub fn rewrite<R: SqlRewriter + ?Sized>(r: &mut R, q: SqlQuery) -> Result<Rw<SqlQuery>, Refusal> {
    match q {
        SqlQuery::Select(s) => {
            let s = *s;
            let mut ch = false;
            let mut projections = Vec::with_capacity(s.projections.len());
            for p in s.projections {
                let e2 = rewrite_expr(r, p.expr)?;
                if e2.is_new() {
                    ch = true;
                    projections.push(build::projection(e2.into_inner(), p.alias, p.out)?);
                } else {
                    projections.push(Projection { expr: e2.into_inner(), alias: p.alias, out: p.out });
                }
            }
            let from = rewrite_source(r, s.from)?.take(&mut ch);
            let r#where = opt(r, s.r#where, &mut ch)?;
            let group_by = exprs(r, s.group_by, &mut ch)?;
            let having = opt(r, s.having, &mut ch)?;
            let qualify = opt(r, s.qualify, &mut ch)?;
            let order_by = keys(r, s.order_by, &mut ch)?;
            let out = SqlSelect { projections, from, r#where, group_by, having, qualify, order_by, ..s };
            let out = if ch { build::select(out)? } else { out };
            Ok(r.select(out)?.after(ch))
        }
        SqlQuery::Union(u) => {
            let mut ch = false;
            let mut branches = Vec::with_capacity(u.branches.len());
            for b in u.branches {
                branches.push(rewrite(r, b)?.take(&mut ch));
            }
            let out = if ch { build::union(branches, u.all, u.outputs)? } else { SqlUnion { branches, ..u } };
            Ok(r.union(out)?.after(ch))
        }
        SqlQuery::With(w) => {
            let mut ch = false;
            let mut ctes = Vec::with_capacity(w.ctes.len());
            for c in w.ctes {
                ctes.push(Cte { query: rewrite(r, c.query)?.take(&mut ch), ..c });
            }
            let body = Box::new(rewrite(r, *w.body)?.take(&mut ch));
            let out = SqlQuery::With(SqlWith { ctes, body, recursive: w.recursive });
            Ok(if ch { Rw::New(out) } else { Rw::Same(out) })
        }
    }
}

fn opt<R: SqlRewriter + ?Sized>(r: &mut R, e: Option<SqlExpr>, ch: &mut bool) -> Result<Option<SqlExpr>, Refusal> {
    e.map(|e| Ok(rewrite_expr(r, e)?.take(ch))).transpose()
}

fn exprs<R: SqlRewriter + ?Sized>(r: &mut R, es: Vec<SqlExpr>, ch: &mut bool) -> Result<Vec<SqlExpr>, Refusal> {
    es.into_iter().map(|e| Ok(rewrite_expr(r, e)?.take(ch))).collect()
}

fn keys<R: SqlRewriter + ?Sized>(r: &mut R, ks: Vec<SortKey>, ch: &mut bool) -> Result<Vec<SortKey>, Refusal> {
    ks.into_iter().map(|k| Ok(SortKey { expr: rewrite_expr(r, k.expr)?.take(ch), ..k })).collect()
}

/// A boxed child rewritten in its box.
fn boxed<R: SqlRewriter + ?Sized>(r: &mut R, mut e: Box<SqlExpr>, ch: &mut bool) -> Result<Box<SqlExpr>, Refusal> {
    let child = std::mem::replace(&mut *e, SqlExpr::NullLit { r#type: TypeFact::Bottom });
    *e = rewrite_expr(r, child)?.take(ch);
    Ok(e)
}

/// A boxed subquery rewritten in its box.
fn subquery<R: SqlRewriter + ?Sized>(r: &mut R, mut q: Box<SqlQuery>, ch: &mut bool) -> Result<Box<SqlQuery>, Refusal> {
    let child =
        std::mem::replace(&mut *q, SqlQuery::Union(SqlUnion { branches: Vec::new(), all: false, outputs: Vec::new() }));
    *q = rewrite(r, child)?.take(ch);
    Ok(q)
}

/// Java's `rewriteSource`.
pub fn rewrite_source<R: SqlRewriter + ?Sized>(r: &mut R, s: SqlSource) -> Result<Rw<SqlSource>, Refusal> {
    let mut ch = false;
    let out = match s {
        SqlSource::Subselect { inner, alias, frame_name } => {
            SqlSource::Subselect { inner: subquery(r, inner, &mut ch)?, alias, frame_name }
        }
        SqlSource::Values { rows, columns, alias, outputs } => SqlSource::Values {
            rows: rows.into_iter().map(|row| exprs(r, row, &mut ch)).collect::<Result<_, _>>()?,
            columns,
            alias,
            outputs,
        },
        SqlSource::Join { left, right, kind, on, outputs } => {
            let left = rewrite_source(r, *left)?.take(&mut ch);
            let right = rewrite_source(r, *right)?.take(&mut ch);
            let on = on.map(|e| Ok::<_, Refusal>(rewrite_expr(r, *e)?.take(&mut ch))).transpose()?;
            if ch {
                build::join(left, right, kind, on)?
            } else {
                SqlSource::Join { left: Box::new(left), right: Box::new(right), kind, on: on.map(Box::new), outputs }
            }
        }
        SqlSource::Pivot { source, on, r#in, named, usings, alias, outputs } => {
            let source = Box::new(rewrite_source(r, *source)?.take(&mut ch));
            let on = exprs(r, on, &mut ch)?;
            let r#in = exprs(r, r#in, &mut ch)?;
            let named = exprs(r, named, &mut ch)?;
            let mut us = Vec::with_capacity(usings.len());
            for u in usings {
                match rewrite_expr(r, SqlExpr::Reducer(u.agg))? {
                    Rw::Same(SqlExpr::Reducer(agg)) => us.push(PivotUsing { agg, ..u }),
                    Rw::New(SqlExpr::Reducer(agg)) => {
                        ch = true;
                        us.push(PivotUsing { agg, ..u });
                    }
                    // Java's `(SqlAgg.Reducer) rewriteExpr(u.agg())`: its ClassCastException's message, less the JVM's
                    // class-loader clause (`(... are in unnamed module of loader 'app')`)
                    other => {
                        return Err(Refusal {
                            class: Refusal::CLASS_CAST,
                            message: format!(
                                "class com.legend.sql.SqlExpr${} cannot be cast to class com.legend.sql.SqlAgg$Reducer",
                                crate::java::simple_name(other.get())
                            ),
                        });
                    }
                }
            }
            SqlSource::Pivot { source, on, r#in, named, usings: us, alias, outputs }
        }
        // the leaves: a table, a CTE reference, a URL, a plan variable, carried text, a catalog grid, Dual
        leaf => leaf,
    };
    Ok(r.source(out)?.after(ch))
}

/// Java's `rewriteExpr`.
pub fn rewrite_expr<R: SqlRewriter + ?Sized>(r: &mut R, e: SqlExpr) -> Result<Rw<SqlExpr>, Refusal> {
    let mut ch = false;
    let c = &mut ch;
    let out = match e {
        SqlExpr::StoredRead { column, stored, r#type } => match rewrite_expr(r, SqlExpr::Column(column))? {
            Rw::Same(SqlExpr::Column(column)) => SqlExpr::StoredRead { column, stored, r#type },
            Rw::New(SqlExpr::Column(column)) => {
                *c = true;
                build::stored_read(column, stored)
            }
            other => {
                return Err(Refusal::illegal_state(format!(
                    "a pass rewrote a stored read's column into {}: a stored read reads a table column, nothing else",
                    crate::java::expr_text(other.get())
                )));
            }
        },
        SqlExpr::Membership { needle, collection, r#type } => {
            let needle = boxed(r, needle, c)?;
            let collection = boxed(r, collection, c)?;
            if *c {
                build::membership(*needle, *collection)
            } else {
                SqlExpr::Membership { needle, collection, r#type }
            }
        }
        SqlExpr::ReduceCollection { reducer, collection, extras, r#type } => {
            let collection = boxed(r, collection, c)?;
            let extras = exprs(r, extras, c)?;
            if *c {
                build::reduce_collection(reducer, *collection, extras)
            } else {
                SqlExpr::ReduceCollection { reducer, collection, extras, r#type }
            }
        }
        SqlExpr::Group { inner, r#type } => {
            let inner = boxed(r, inner, c)?;
            if *c { build::group(*inner) } else { SqlExpr::Group { inner, r#type } }
        }
        SqlExpr::OrderedListAgg { value, order_by, r#type } => {
            let value = boxed(r, value, c)?;
            let order_by = boxed(r, order_by, c)?;
            if *c {
                build::ordered_list_agg(*value, *order_by)
            } else {
                SqlExpr::OrderedListAgg { value, order_by, r#type }
            }
        }
        SqlExpr::ArrayLit { elements, r#type } => {
            let elements = exprs(r, elements, c)?;
            // the one-argument constructor: the given type is gone
            if *c { build::array_lit(elements) } else { SqlExpr::ArrayLit { elements, r#type } }
        }
        SqlExpr::StructLit { fields, r#type } => {
            let fields = fields
                .into_iter()
                .map(|f| Ok(StructLitField { value: rewrite_expr(r, f.value)?.take(c), ..f }))
                .collect::<Result<Vec<_>, Refusal>>()?;
            if *c { build::struct_lit(fields) } else { SqlExpr::StructLit { fields, r#type } }
        }
        SqlExpr::StructGet { source, field, r#type } => {
            let source = boxed(r, source, c)?;
            // the two-argument constructor: the given type is gone
            if *c {
                build::struct_get_given(*source, field, TypeFact::Unknown)
            } else {
                SqlExpr::StructGet { source, field, r#type }
            }
        }
        SqlExpr::Call { r#fn, args, r#type } => {
            let args = exprs(r, args, c)?;
            if *c { build::call(r#fn, args) } else { SqlExpr::Call { r#fn, args, r#type } }
        }
        SqlExpr::Case { whens, otherwise, r#type } => {
            let whens = whens
                .into_iter()
                .map(|w| {
                    let condition = rewrite_expr(r, w.condition)?.take(c);
                    let then = rewrite_expr(r, w.then)?.take(c);
                    Ok(When { condition, then })
                })
                .collect::<Result<Vec<_>, Refusal>>()?;
            let otherwise = otherwise.map(|o| boxed(r, o, c)).transpose()?;
            if *c { build::case(whens, otherwise.map(|o| *o)) } else { SqlExpr::Case { whens, otherwise, r#type } }
        }
        SqlExpr::Exists { subquery: q, r#type } => {
            let q = subquery(r, q, c)?;
            if *c { build::exists(*q) } else { SqlExpr::Exists { subquery: q, r#type } }
        }
        SqlExpr::InSubquery { value, subquery: q, r#type } => {
            let value = boxed(r, value, c)?;
            let q = subquery(r, q, c)?;
            if *c { build::in_subquery(*value, *q) } else { SqlExpr::InSubquery { value, subquery: q, r#type } }
        }
        SqlExpr::Quantified { value, comparison, quantifier, subquery: q, r#type } => {
            let value = boxed(r, value, c)?;
            let q = subquery(r, q, c)?;
            if *c {
                build::quantified(*value, comparison, quantifier, *q)
            } else {
                SqlExpr::Quantified { value, comparison, quantifier, subquery: q, r#type }
            }
        }
        SqlExpr::CheckedDefects { own, hoists, r#type } => {
            let own = boxed(r, own, c)?;
            let hoists = hoists
                .into_iter()
                .map(|h| Ok(Hoist { envelope: rewrite_expr(r, h.envelope)?.take(c), ..h }))
                .collect::<Result<Vec<_>, Refusal>>()?;
            if *c { build::checked_defects(*own, hoists) } else { SqlExpr::CheckedDefects { own, hoists, r#type } }
        }
        SqlExpr::CheckedChildValue { envelope, to_many, r#type } => {
            let envelope = boxed(r, envelope, c)?;
            if *c {
                build::checked_child_value(*envelope, to_many)
            } else {
                SqlExpr::CheckedChildValue { envelope, to_many, r#type }
            }
        }
        // rewriteExpr, not the hook alone: a pass reaches the calls nested under the wrapper
        SqlExpr::CheckedOne { list, scalar_carrier, at_least_only, r#type } => {
            let list = boxed(r, list, c)?;
            if *c {
                build::checked_one(*list, scalar_carrier, at_least_only)
            } else {
                SqlExpr::CheckedOne { list, scalar_carrier, at_least_only, r#type }
            }
        }
        SqlExpr::CompactList { list, r#type } => {
            let list = boxed(r, list, c)?;
            if *c { build::compact_list(*list) } else { SqlExpr::CompactList { list, r#type } }
        }
        SqlExpr::ScalarSubquery { subquery: q, r#type } => {
            let q = subquery(r, q, c)?;
            if *c { build::scalar_subquery(*q) } else { SqlExpr::ScalarSubquery { subquery: q, r#type } }
        }
        SqlExpr::DeferredTdsString { inner, alias, id, form, render_tds_null, r#type } => {
            match rewrite(r, SqlQuery::Select(inner))?.take(c) {
                SqlQuery::Select(inner) => {
                    SqlExpr::DeferredTdsString { inner, alias, id, form, render_tds_null, r#type }
                }
                _ => return Err(Refusal::class_cast("SqlUnion", "SqlSelect")),
            }
        }
        SqlExpr::JsonObject { kv, r#type } => {
            let kv = exprs(r, kv, c)?;
            if *c { build::json_object(kv) } else { SqlExpr::JsonObject { kv, r#type } }
        }
        SqlExpr::JsonArray { elements, r#type } => {
            let elements = exprs(r, elements, c)?;
            if *c { build::json_array(elements) } else { SqlExpr::JsonArray { elements, r#type } }
        }
        SqlExpr::JsonArrayAgg { value, order_keys, r#type } => {
            let value = boxed(r, value, c)?;
            let order_keys = order_keys
                .into_iter()
                .map(|k| Ok(JsonArrayAggKey { expr: rewrite_expr(r, k.expr)?.take(c), desc: k.desc }))
                .collect::<Result<Vec<_>, Refusal>>()?;
            if *c {
                build::json_array_agg(*value, order_keys)
            } else {
                SqlExpr::JsonArrayAgg { value, order_keys, r#type }
            }
        }
        SqlExpr::WindowCall { r#fn, partition_by, order_by, frame, r#type } => {
            let f = rewrite_agg(r, *r#fn)?.take(c);
            let partition_by = exprs(r, partition_by, c)?;
            let order_by = keys(r, order_by, c)?;
            if *c {
                build::window_call(f, partition_by, order_by, frame)
            } else {
                SqlExpr::WindowCall { r#fn: Box::new(f), partition_by, order_by, frame, r#type }
            }
        }
        SqlExpr::Lambda(mut l) => {
            l.body = boxed(r, l.body, c)?;
            // the two-argument constructor: a rebuilt lambda's fact is UNKNOWN
            if *c { SqlExpr::Lambda(build::lambda_of(l.params, *l.body)) } else { SqlExpr::Lambda(l) }
        }
        SqlExpr::Cast { value, target, conform, r#type } => {
            let value = boxed(r, value, c)?;
            if *c {
                build::cast_conform(*value, target, conform)
            } else {
                SqlExpr::Cast { value, target, conform, r#type }
            }
        }
        SqlExpr::FoldCall { source, lambda, init, acc_is_list, homogeneous, r#type } => {
            let source = boxed(r, source, c)?;
            let lambda = match rewrite_expr(r, SqlExpr::Lambda(lambda))? {
                Rw::Same(SqlExpr::Lambda(l)) => l,
                Rw::New(SqlExpr::Lambda(l)) => {
                    *c = true;
                    l
                }
                other => return Err(Refusal::class_cast(crate::java::simple_name(other.get()), "Lambda")),
            };
            let init = boxed(r, init, c)?;
            if *c {
                build::fold_call(*source, lambda, *init, acc_is_list, homogeneous)
            } else {
                SqlExpr::FoldCall { source, lambda, init, acc_is_list, homogeneous, r#type }
            }
        }
        SqlExpr::Reducer(red) => SqlExpr::Reducer(rewrite_reducer(r, red, c)?),
        // the leaves
        leaf => leaf,
    };
    Ok(r.expr(out)?.after(ch))
}

fn rewrite_reducer<R: SqlRewriter + ?Sized>(r: &mut R, red: Reducer, c: &mut bool) -> Result<Reducer, Refusal> {
    let mut ch = false;
    let args = exprs(r, red.args, &mut ch)?;
    let order_by = keys(r, red.order_by, &mut ch)?;
    *c |= ch;
    Ok(if ch { build::reducer(red.r#fn, args, red.distinct, order_by) } else { Reducer { args, order_by, ..red } })
}

/// Java's `rewriteAgg`: a reducer, a ranking or a value function, its arguments rewritten (no hook sees it).
pub fn rewrite_agg<R: SqlRewriter + ?Sized>(r: &mut R, a: SqlAgg) -> Result<Rw<SqlAgg>, Refusal> {
    let mut ch = false;
    let out = match a {
        SqlAgg::Reducer(red) => SqlAgg::Reducer(rewrite_reducer(r, red, &mut ch)?),
        SqlAgg::RankingFn { r#fn, args } => SqlAgg::RankingFn { r#fn, args: exprs(r, args, &mut ch)? },
        SqlAgg::ValueFn { r#fn, args } => SqlAgg::ValueFn { r#fn, args: exprs(r, args, &mut ch)? },
    };
    Ok(if ch { Rw::New(out) } else { Rw::Same(out) })
}

/// An expression walked with no pass's hooks (every hook the identity): for a pass's own inner walks.
pub struct Identity;

impl SqlRewriter for Identity {
    fn name(&self) -> &'static str {
        "SqlRewriter"
    }
}
