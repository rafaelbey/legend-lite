// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! Java's `FoldsAsRecursiveCtes` (`core/src/main/java/com/legend/sql/dialect/FoldsAsRecursiveCtes.java`), the pass only
//! BigQuery installs: a fold that reads nothing of the row it is in becomes a recursive common table expression at the
//! top of the statement (BigQuery's `WITH RECURSIVE` is top-level only), read in its place as the last row's
//! accumulator. Pure's fold `(x, acc)` over `xs` from `init`:
//!
//! ```text
//!   __foldN AS (SELECT 0 AS __i, init AS __acc
//!               UNION ALL
//!               SELECT __foldN.__i + 1, body[x := xs[__i + 1], acc := __foldN.__acc]
//!               FROM __foldN WHERE __foldN.__i < length(xs))
//! ```
//!
//! read as `(SELECT __acc FROM __foldN ORDER BY __i DESC LIMIT 1)`. A fold over the NULL list is its init; DuckDB's
//! `list_reduce(xs, (acc, x))` likewise from 1. A fold inside another is hoisted first, and the outer one reads it.
//!
//! Java keeps the reads it made in an identity set, which the closedness test consults; Rust keeps them by value (a
//! read is `SELECT __acc FROM __foldN ...` over the pass's own CTE name, which no other node of a statement equals).

use crate::build;
use crate::explosion_markers::null_list;
use crate::refusal::Refusal;
use crate::rewriter::{Rw, SqlRewriter, rewrite};
use crate::typing::children;
use lite_sql::{Cte, OutputCol, Projection, SqlExpr, SqlFn, SqlQuery, SqlSelect, SqlSource};

const I: &str = "__i";
const ACC: &str = "__acc";

/// Java's `FoldsAsRecursiveCtes`: the hoisted folds, in the order they were met, and the reads that replace them.
#[derive(Debug, Default)]
pub struct FoldsAsRecursiveCtes {
    ctes: Vec<Cte>,
    reads: Vec<SqlExpr>,
}

/// A select of `ps` over `from`, every other clause but the filter, order and limit empty (Java's `select`).
fn select(
    ps: Vec<Projection>,
    from: SqlSource,
    r#where: Option<SqlExpr>,
    order_by: Vec<lite_sql::SortKey>,
    limit: Option<i64>,
) -> Result<SqlSelect, Refusal> {
    build::select(SqlSelect {
        projections: ps,
        distinct: false,
        from,
        r#where,
        group_by: Vec::new(),
        having: None,
        qualify: None,
        order_by,
        limit,
        offset: None,
        outputs: Vec::new(),
    })
}

fn named(e: SqlExpr, name: &str) -> Result<Projection, Refusal> {
    build::projection(e, Some(name.to_owned()), None)
}

/// `e` with the parameter `name` read as `with`: a bare read is `with`; a read qualified by it is that field of
/// `with`; a lambda rebinding the name shadows it (Java's `substitute`). `None` where nothing changed.
fn substitute(e: &SqlExpr, name: &str, with: &SqlExpr) -> Result<Option<SqlExpr>, Refusal> {
    if let SqlExpr::Column(c) = e {
        if c.table.is_none() && c.name == name {
            return Ok(Some(with.clone()));
        }
        if c.table.as_deref() == Some(name) {
            return Ok(Some(crate::carrier::struct_get_of(with.clone(), &c.name)));
        }
        return Ok(None);
    }
    if let SqlExpr::Lambda(l) = e
        && l.params.iter().any(|p| p == name)
    {
        return Ok(None);
    }
    crate::carrier::map_children(e, &mut |k| substitute(k, name, with))
}

fn substituted(e: SqlExpr, name: &str, with: &SqlExpr) -> Result<SqlExpr, Refusal> {
    Ok(substitute(&e, name, with)?.unwrap_or(e))
}

impl FoldsAsRecursiveCtes {
    /// Whether `e` names nothing but `bound` lambda parameters (and the reads this pass made) (Java's `closed`).
    fn closed(&self, e: &SqlExpr, bound: &[String]) -> bool {
        match e {
            SqlExpr::Column(c) => match &c.table {
                None => bound.contains(&c.name),
                Some(t) => bound.contains(t),
            },
            SqlExpr::Lambda(l) => {
                let mut inner = bound.to_vec();
                inner.extend(l.params.iter().cloned());
                self.closed(&l.body, &inner)
            }
            SqlExpr::ScalarSubquery { .. } => self.reads.contains(e),
            SqlExpr::Exists { .. }
            | SqlExpr::InSubquery { .. }
            | SqlExpr::Quantified { .. }
            | SqlExpr::DeferredTdsString { .. }
            | SqlExpr::TempTableInSplice { .. }
            | SqlExpr::RowOrder { .. }
            | SqlExpr::StoredRead { .. }
            | SqlExpr::Star { .. }
            | SqlExpr::StarExcept { .. }
            | SqlExpr::WindowCall { .. }
            | SqlExpr::Reducer(_) => false,
            SqlExpr::FoldCall { source, lambda, init, .. } => {
                self.closed(source, bound)
                    && self.closed(init, bound)
                    && self.closed(&SqlExpr::Lambda(lambda.clone()), bound)
            }
            other => children(other).iter().all(|k| self.closed(k, bound)),
        }
    }

    /// The recursive expression stepping `body` over `xs` from offset `from` with `init`, and the read of its last
    /// accumulator (Java's `hoist`).
    fn hoist(
        &mut self,
        xs: &SqlExpr,
        from: SqlExpr,
        init: SqlExpr,
        body: &SqlExpr,
        x: &str,
        acc: &str,
    ) -> Result<SqlExpr, Refusal> {
        let name = format!("__fold{}", self.ctes.len());
        let anchor = select(vec![named(from, I)?, named(init, ACC)?], SqlSource::Dual, None, Vec::new(), None)?;
        let outs: Vec<OutputCol> = anchor.outputs.clone();
        let own = SqlSource::Cte { name: name.clone(), alias: name.clone(), outputs: outs.clone() };
        let i = SqlExpr::Column(build::column_in(Some(&name), &outs, I));
        let element =
            build::call(SqlFn::ListGet, vec![xs.clone(), build::call(SqlFn::Plus, vec![i.clone(), build::int_lit(1)])]);
        let acc_read = SqlExpr::Column(build::column_in(Some(&name), &outs, ACC));
        let step = substituted(substituted(body.clone(), x, &element)?, acc, &acc_read)?;
        let next = select(
            vec![named(build::call(SqlFn::Plus, vec![i.clone(), build::int_lit(1)]), I)?, named(step, ACC)?],
            own.clone(),
            Some(build::call(SqlFn::Less, vec![i.clone(), build::call(SqlFn::ListLength, vec![xs.clone()])])),
            Vec::new(),
            None,
        )?;
        let union = build::union(vec![build::query(anchor), build::query(next)], true, outs.clone())?;
        self.ctes.push(Cte { name: name.clone(), query: SqlQuery::Union(union), materialized: false });
        let last = build::scalar_subquery(build::query(select(
            vec![named(acc_read, ACC)?],
            own,
            None,
            vec![build::sort_key(i, false, None, None)],
            Some(1),
        )?));
        self.reads.push(last.clone());
        Ok(last)
    }
}

impl SqlRewriter for FoldsAsRecursiveCtes {
    fn name(&self) -> &'static str {
        "FoldsAsRecursiveCtes"
    }

    fn rewrite_root(&mut self, q: SqlQuery) -> Result<SqlQuery, Refusal> {
        self.ctes.clear();
        self.reads.clear();
        let out = rewrite(self, q)?.into_inner();
        if self.ctes.is_empty() {
            return Ok(out);
        }
        build::prepend(self.ctes.clone(), out, true)
    }

    fn expr(&mut self, e: SqlExpr) -> Result<Rw<SqlExpr>, Refusal> {
        if let SqlExpr::FoldCall { source, lambda, init, .. } = &e
            && lambda.params.len() == 2
            && self.closed(&e, &[])
        {
            if null_list(source) {
                return Ok(Rw::New(init.as_ref().clone()));
            }
            let (x, acc) = (lambda.params[0].clone(), lambda.params[1].clone());
            let read = self.hoist(source, build::int_lit(0), init.as_ref().clone(), &lambda.body, &x, &acc)?;
            return Ok(Rw::New(read));
        }
        if let SqlExpr::Call { r#fn: SqlFn::ListReduce, args, .. } = &e
            && args.len() == 2
            && let SqlExpr::Lambda(l) = &args[1]
            && l.params.len() == 2
            && self.closed(&e, &[])
        {
            let xs = &args[0];
            let first = build::call(SqlFn::ListGet, vec![xs.clone(), build::int_lit(1)]);
            let (x, acc) = (l.params[1].clone(), l.params[0].clone());
            let read = self.hoist(xs, build::int_lit(1), first, &l.body, &x, &acc)?;
            return Ok(Rw::New(read));
        }
        Ok(Rw::Same(e))
    }
}
