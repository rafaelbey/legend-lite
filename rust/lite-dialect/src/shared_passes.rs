// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! The MIR passes the dialects past DuckDB and the H2 family share, each its Java class ported
//! (`core/src/main/java/com/legend/sql/dialect/<Name>.java`): Postgres installs them first (`Postgres.passes()`,
//! Postgres.java:253; [`crate::postgres`]), and SQLite, MariaDB, MySQL, Trino, Snowflake, Databricks, ClickHouse and
//! BigQuery install them too --
//!
//! - [`StarExceptToColumns`]: a star exclusion spelled as the columns it stands for (Postgres, MariaDB, SQLite, Trino);
//! - [`WholePartitionOrderedSets`]: a median, mode or quantile over a whole partition as a correlated aggregate
//!   (Postgres, SQLite);
//! - [`StringCountsAsInt`]: a string function's count cast to a 32-bit integer (Postgres);
//! - [`AggregatesDeliverDouble`]: named grouped aggregates cast to DOUBLE (H2's AVG, Postgres's moments, five more's);
//! - [`QualifyToSubselect`]: QUALIFY as a wrapping select -- the plain form, and the by-outputs form Postgres, MariaDB,
//!   SQLite and Trino install;
//! - [`RootNumericTypes`]: a root column typed DECIMAL(p,s) or HUGEINT cast to its type (Postgres, Databricks,
//!   ClickHouse, Snowflake, Trino);
//! - [`PivotAsGroupBy`]: a static pivot as a grouped CASE aggregation, for a dialect with lists and no PIVOT (Trino,
//!   Snowflake, ClickHouse);
//! - [`AsOfAsPick`]: an as-of join as a LEFT JOIN whose rows a QUALIFY picks (Trino).
//!
//! `ConstantKeysAsExpressions` (both forms) and `SubstringClamp` are [`crate::passes`]', `CarrierStrategies`
//! [`crate::carrier`]'s. Each pass rebuilds through [`crate::build`] (Java's constructors) and answers [`Rw::Same`]
//! exactly where Java hands back its input.

use crate::build::{self, call, derived, query};
use crate::carrier::{as_of_key, copy_with_alias, generated_slot, remap_alias, with_children};
use crate::passes::Stars;
use crate::refusal::Refusal;
use crate::rewriter::{Rw, SqlRewriter, rewrite_expr};
use crate::typing::{children, source_alias, source_outputs};
use lite_sql::{
    AggFn, JoinKind, Origin, Projection, Reducer, Scalar, SortKey, SqlAgg, SqlExpr, SqlFn, SqlQuery, SqlSelect,
    SqlSource, SqlType,
};
use std::collections::HashMap;

fn same(s: SqlSelect) -> Result<Rw<SqlQuery>, Refusal> {
    Ok(Rw::Same(query(s)))
}

/// Java's `SqlSelect.Projection.outputName()`: its alias, else a column's (or a stored read's) name.
fn output_name(p: &Projection) -> Option<String> {
    if let Some(alias) = &p.alias {
        return Some(alias.clone());
    }
    match &p.expr {
        SqlExpr::Column(c) => Some(c.name.clone()),
        SqlExpr::StoredRead { column, .. } => Some(column.name.clone()),
        _ => None,
    }
}

/// An expression walk with the rewriter's own traversal and one post-children hook (Java's anonymous `SqlRewriter`
/// subclasses that override `expr` alone).
struct ExprHook<'a>(&'a mut dyn FnMut(SqlExpr) -> Result<Rw<SqlExpr>, Refusal>);

impl SqlRewriter for ExprHook<'_> {
    fn name(&self) -> &'static str {
        "SqlRewriter"
    }

    fn expr(&mut self, e: SqlExpr) -> Result<Rw<SqlExpr>, Refusal> {
        (self.0)(e)
    }
}

/// `e` walked with `hook`: Java's `new SqlRewriter() { expr(x) ... }.rewriteExpr(e)`.
fn walk(e: SqlExpr, hook: &mut dyn FnMut(SqlExpr) -> Result<Rw<SqlExpr>, Refusal>) -> Result<SqlExpr, Refusal> {
    Ok(rewrite_expr(&mut ExprHook(hook), e)?.into_inner())
}

// ==================================================================
// StarExceptToColumns
// ==================================================================

/// A star exclusion (`t.* EXCLUDE (a, b)`, DuckDB's) spelled as the columns it stands for, for a database that has none
/// (Java's `StarExceptToColumns`): the source's declared outputs, less the excluded names, in order. The lowering
/// builds one where a pivot over several columns folds them into one key.
#[derive(Debug, Default)]
pub struct StarExceptToColumns;

impl SqlRewriter for StarExceptToColumns {
    fn name(&self) -> &'static str {
        "StarExceptToColumns"
    }

    fn select(&mut self, s: SqlSelect) -> Result<Rw<SqlQuery>, Refusal> {
        if !s.projections.iter().any(|p| matches!(p.expr, SqlExpr::StarExcept { .. })) {
            return same(s);
        }
        let leaves = Stars::sources(&s.from);
        let mut ps = Vec::new();
        for p in &s.projections {
            match &p.expr {
                SqlExpr::StarExcept { table, except, .. } => {
                    ps.extend(Stars::columns(&leaves, table.as_deref(), except, &mut |c| c)?);
                }
                _ => ps.push(p.clone()),
            }
        }
        Ok(Rw::New(query(build::with_projections(s, ps)?)))
    }
}

// ==================================================================
// WholePartitionOrderedSets
// ==================================================================

/// A median, mode or quantile over a WHOLE partition, for a database whose ordered-set aggregates are not window
/// functions (Java's `WholePartitionOrderedSets`; Postgres: percentile_cont and mode take no OVER clause). Over a
/// window with no order and no frame the value is the aggregate of the partition's rows: the same aggregate over a
/// fresh copy of the select's source, under the select's own filter, of the rows whose partition keys are not distinct
/// from the current row's -- a correlated subquery. Any other window, or a select over a join, is left as it is: the
/// dialect refuses it by name.
#[derive(Debug, Default)]
pub struct WholePartitionOrderedSets;

/// Java's `ORDERED_SETS`.
const ORDERED_SETS: [AggFn; 4] = [AggFn::Median, AggFn::Mode, AggFn::QuantileCont, AggFn::QuantileDisc];

/// The reducer and the partition of an ordered set over a whole partition (no order, no frame), when `e` is one.
fn whole_partition(e: &SqlExpr) -> Option<(&Reducer, &[SqlExpr])> {
    match e {
        SqlExpr::WindowCall { r#fn, partition_by, order_by, frame: None, .. } if order_by.is_empty() => {
            match r#fn.as_ref() {
                SqlAgg::Reducer(r) if ORDERED_SETS.contains(&r.r#fn) => Some((r, partition_by)),
                _ => None,
            }
        }
        _ => None,
    }
}

fn rewritable(e: &SqlExpr) -> bool {
    whole_partition(e).is_some() || children(e).iter().any(|k| rewritable(k))
}

/// `e` with each whole-partition ordered set replaced (`None`: `e` itself); `alias` names the copy, unique in the
/// statement by the projection's position (a nested ordered set is a second copy one level down: `alias_<i>`).
fn replace(e: &SqlExpr, s: &SqlSelect, alias: &str) -> Result<Option<SqlExpr>, Refusal> {
    if let Some((r, partition)) = whole_partition(e) {
        return partition_aggregate(r, partition, s, alias);
    }
    let kids = children(e);
    if kids.is_empty() {
        return Ok(None);
    }
    let mut mapped = Vec::with_capacity(kids.len());
    let mut changed = false;
    for (i, k) in kids.into_iter().enumerate() {
        match replace(&k, s, &format!("{alias}_{i}"))? {
            Some(m) => {
                changed = true;
                mapped.push(m);
            }
            None => mapped.push(k.into_owned()),
        }
    }
    if changed { Ok(Some(with_children(e, mapped)?)) } else { Ok(None) }
}

/// `(SELECT r(args') FROM copy WHERE where' AND NOT (p1' IS DISTINCT FROM p1) ...)`, or none when the select's source
/// cannot be copied.
fn partition_aggregate(
    r: &Reducer,
    partition: &[SqlExpr],
    s: &SqlSelect,
    alias: &str,
) -> Result<Option<SqlExpr>, Refusal> {
    let from = source_alias(&s.from)?;
    let Some(copy) = copy_with_alias(&s.from, alias) else {
        return Ok(None);
    };
    let args = r.args.iter().map(|a| remap_alias(a, from, alias)).collect::<Result<Vec<_>, _>>()?;
    let mut conds = Vec::new();
    if let Some(w) = &s.r#where {
        conds.push(remap_alias(w, from, alias)?);
    }
    for p in partition {
        conds.push(call(SqlFn::Not, vec![call(SqlFn::IsDistinctFrom, vec![remap_alias(p, from, alias)?, p.clone()])]));
    }
    let mut r#where = None;
    for c in conds {
        r#where = Some(match r#where {
            None => c,
            Some(w) => call(SqlFn::And, vec![w, c]),
        });
    }
    let agg = build::reducer(r.r#fn, args, r.distinct, r.order_by.clone());
    let select = build::select(SqlSelect {
        projections: vec![build::projection(SqlExpr::Reducer(agg), None, None)?],
        distinct: false,
        from: copy,
        r#where,
        group_by: Vec::new(),
        having: None,
        qualify: None,
        order_by: Vec::new(),
        limit: None,
        offset: None,
        outputs: Vec::new(),
    })?;
    Ok(Some(build::scalar_subquery(query(select))))
}

impl SqlRewriter for WholePartitionOrderedSets {
    fn name(&self) -> &'static str {
        "WholePartitionOrderedSets"
    }

    fn select(&mut self, s: SqlSelect) -> Result<Rw<SqlQuery>, Refusal> {
        if !s.group_by.is_empty()
            || matches!(s.from, SqlSource::Join { .. })
            || !s.projections.iter().any(|p| rewritable(&p.expr))
        {
            return same(s);
        }
        let mut ps = Vec::with_capacity(s.projections.len());
        let mut changed = false;
        for (i, p) in s.projections.iter().enumerate() {
            let alias = format!("{}_ops{i}", source_alias(&s.from)?);
            match replace(&p.expr, &s, &alias)? {
                Some(e) => {
                    changed = true;
                    ps.push(build::projection(e, p.alias.clone(), p.out.clone())?);
                }
                None => ps.push(p.clone()),
            }
        }
        if !changed {
            return same(s);
        }
        Ok(Rw::New(query(build::with_projections(s, ps)?)))
    }
}

// ==================================================================
// StringCountsAsInt
// ==================================================================

/// Postgres's string functions take their counts and positions as a 32-bit `integer` (Java's `StringCountsAsInt`):
/// `substr`, `left`, `right`, `repeat`, `lpad`, `rpad` and `chr` have no `bigint` overload, so each such argument that
/// is no literal (and not already cast to INTEGER) is cast to one. It runs after `SubstringClamp`, whose start it casts
/// too.
#[derive(Debug, Default)]
pub struct StringCountsAsInt;

/// Java's `COUNTS`: the functions, by the positions of their 32-bit arguments.
fn counts(f: SqlFn) -> Option<&'static [usize]> {
    Some(match f {
        SqlFn::Substring => &[1, 2],
        SqlFn::Left | SqlFn::Right | SqlFn::RepeatStr | SqlFn::Lpad | SqlFn::Rpad => &[1],
        SqlFn::Chr => &[0],
        _ => return None,
    })
}

impl SqlRewriter for StringCountsAsInt {
    fn name(&self) -> &'static str {
        "StringCountsAsInt"
    }

    fn expr(&mut self, e: SqlExpr) -> Result<Rw<SqlExpr>, Refusal> {
        let SqlExpr::Call { r#fn, mut args, r#type } = e else {
            return Ok(Rw::Same(e));
        };
        let Some(positions) = counts(r#fn) else {
            return Ok(Rw::Same(SqlExpr::Call { r#fn, args, r#type }));
        };
        let mut changed = false;
        for &i in positions {
            if i < args.len()
                && !matches!(args[i], SqlExpr::IntLit { .. })
                && !matches!(args[i], SqlExpr::Cast { target: SqlType::Scalar(Scalar::Integer), .. })
            {
                let x = std::mem::replace(&mut args[i], build::null_lit());
                args[i] = build::cast(x, SqlType::Scalar(Scalar::Integer));
                changed = true;
            }
        }
        if changed {
            return Ok(Rw::New(call(r#fn, args)));
        }
        Ok(Rw::Same(SqlExpr::Call { r#fn, args, r#type }))
    }
}

// ==================================================================
// AggregatesDeliverDouble
// ==================================================================

/// The dialect delivers the platform's type facts on its wire (Java's `AggregatesDeliverDouble`): each named grouped
/// aggregate cast to DOUBLE over its exact result (H2 computes every average as DECFLOAT; Postgres answers avg and the
/// moments over an integer or a numeric as numeric). A windowed one is not seen: the walk hands a window's reducer to
/// no hook.
#[derive(Debug)]
pub struct AggregatesDeliverDouble {
    fns: Vec<AggFn>,
}

impl AggregatesDeliverDouble {
    /// Java's `new AggregatesDeliverDouble(Set.of(fns...))`.
    pub fn new(fns: &[AggFn]) -> AggregatesDeliverDouble {
        AggregatesDeliverDouble { fns: fns.to_vec() }
    }
}

impl SqlRewriter for AggregatesDeliverDouble {
    fn name(&self) -> &'static str {
        "AggregatesDeliverDouble"
    }

    fn expr(&mut self, e: SqlExpr) -> Result<Rw<SqlExpr>, Refusal> {
        if let SqlExpr::Reducer(r) = &e
            && self.fns.contains(&r.r#fn)
        {
            return Ok(Rw::New(build::cast(e, SqlType::Scalar(Scalar::Double))));
        }
        Ok(Rw::Same(e))
    }
}

// ==================================================================
// QualifyToSubselect
// ==================================================================

/// A backend without a QUALIFY clause filters window results by wrapping the select (Java's `QualifyToSubselect`).
///
/// The plain form (`new QualifyToSubselect()`, [`QualifyToSubselect::default`]) moves the predicate verbatim: `SELECT *
/// FROM (inner) AS qualify_src WHERE <qualify>`. The by-outputs form (`new QualifyToSubselect(true)`,
/// [`QualifyToSubselect::by_outputs`]; Postgres, MariaDB, SQLite, Trino) addresses everything through `qualify_src`: a
/// predicate or sort operand that IS a projection reads that output, any other window call or column becomes a hidden
/// inner projection (`__qualify<n>`; the outer select lists the declared outputs, so it never leaks), and ORDER BY,
/// LIMIT and OFFSET move outside, after the filter.
#[derive(Debug, Default)]
pub struct QualifyToSubselect {
    /// Java's `byOutputs`.
    by_outputs: bool,
}

/// The wrapping select's alias.
const QUALIFY_ALIAS: &str = "qualify_src";

impl QualifyToSubselect {
    /// Java's `new QualifyToSubselect(true)`.
    pub fn by_outputs() -> QualifyToSubselect {
        QualifyToSubselect { by_outputs: true }
    }
}

/// `new SqlSelect(projections, false, from, where, List.of(), null, null, order, limit, offset, outputs)`.
fn wrapping(
    projections: Vec<Projection>,
    from: SqlSource,
    r#where: SqlExpr,
    order_by: Vec<SortKey>,
    limit: Option<i64>,
    offset: Option<i64>,
    outputs: Vec<lite_sql::OutputCol>,
) -> Result<SqlSelect, Refusal> {
    build::select(SqlSelect {
        projections,
        distinct: false,
        from,
        r#where: Some(r#where),
        group_by: Vec::new(),
        having: None,
        qualify: None,
        order_by,
        limit,
        offset,
        outputs,
    })
}

fn star_projection() -> Result<Projection, Refusal> {
    build::projection(build::star(None), None, None)
}

/// The by-outputs form.
fn by_outputs(s: SqlSelect) -> Result<SqlQuery, Refusal> {
    let Some(qualify) = s.qualify.clone() else {
        return Err(Refusal::null_pointer("the select's qualify"));
    };
    let mut subquery = has_subquery(&qualify)?;
    for k in &s.order_by {
        if subquery {
            break;
        }
        subquery = has_subquery(&k.expr)?;
    }
    if subquery {
        return Err(Refusal::capability(
            "a QUALIFY or sort holding a subquery cannot be re-addressed through its wrapping select",
        ));
    }
    let projections = if s.projections.is_empty() { vec![star_projection()?] } else { s.projections.clone() };
    let mut hidden: Vec<Projection> = Vec::new();
    let r#where = read_through_outputs(qualify, &projections, &mut hidden)?;
    let mut order = Vec::with_capacity(s.order_by.len());
    for k in &s.order_by {
        let e = read_through_outputs(k.expr.clone(), &projections, &mut hidden)?;
        order.push(SortKey {
            expr: e,
            ascending: k.ascending,
            null_order: k.null_order,
            output_name: k.output_name.clone(),
        });
    }
    if !hidden.is_empty() && s.distinct {
        return Err(Refusal::capability(
            "a DISTINCT select's QUALIFY or sort reads a value it does not project; a hidden column would change what \
             is distinct",
        ));
    }
    let no_hidden = hidden.is_empty();
    let mut all = projections;
    all.extend(hidden);
    let (limit, offset, outputs) = (s.limit, s.offset, s.outputs.clone());
    let inner = build::select(SqlSelect {
        projections: all,
        distinct: s.distinct,
        from: s.from,
        r#where: s.r#where,
        group_by: s.group_by,
        having: s.having,
        qualify: None,
        order_by: Vec::new(),
        limit: None,
        offset: None,
        outputs: s.outputs,
    })?;
    // the declared outputs, by name: a hidden column never leaves (a star when nothing is hidden -- the outputs are
    // the inner select's own)
    let outer = if no_hidden {
        vec![star_projection()?]
    } else {
        outputs
            .iter()
            .map(|o| build::projection(derived(Some(QUALIFY_ALIAS), &o.name), None, Some(o.clone())))
            .collect::<Result<Vec<_>, _>>()?
    };
    let from = build::subselect(query(inner), QUALIFY_ALIAS, None);
    Ok(query(wrapping(outer, from, r#where, order, limit, offset, outputs)?))
}

/// `e` re-addressed through `qualify_src`: whole window calls first (so their own column operands stay inner), then
/// the columns left over.
fn read_through_outputs(
    e: SqlExpr,
    projections: &[Projection],
    hidden: &mut Vec<Projection>,
) -> Result<SqlExpr, Refusal> {
    let windows_read = walk(e, &mut |x| match x {
        SqlExpr::WindowCall { .. } => Ok(Rw::New(output(x, projections, hidden)?)),
        other => Ok(Rw::Same(other)),
    })?;
    walk(windows_read, &mut |x| match &x {
        SqlExpr::Column(c) if c.table.as_deref() != Some(QUALIFY_ALIAS) => Ok(Rw::New(output(x, projections, hidden)?)),
        _ => Ok(Rw::Same(x)),
    })
}

/// The output that projects `x`, else a new hidden projection of it.
fn output(x: SqlExpr, projections: &[Projection], hidden: &mut Vec<Projection>) -> Result<SqlExpr, Refusal> {
    for p in projections {
        let name = match (&p.alias, &p.out) {
            (Some(alias), _) => Some(alias.clone()),
            (None, Some(out)) => Some(out.name.clone()),
            (None, None) => output_name(p),
        };
        if let Some(name) = name
            && structurally_same(&p.expr, &x)
        {
            return Ok(derived(Some(QUALIFY_ALIAS), &name));
        }
    }
    for p in hidden.iter() {
        if structurally_same(&p.expr, &x) {
            let Some(alias) = &p.alias else {
                return Err(Refusal::null_pointer("a hidden projection's alias"));
            };
            return Ok(derived(Some(QUALIFY_ALIAS), alias));
        }
    }
    let name = format!("__qualify{}", hidden.len());
    hidden.push(build::projection(x, Some(name.clone()), None)?);
    Ok(derived(Some(QUALIFY_ALIAS), &name))
}

/// Java's `same`: structural equality, a column the same column whatever type fact it carries.
fn structurally_same(a: &SqlExpr, b: &SqlExpr) -> bool {
    match (a, b) {
        (SqlExpr::Column(ca), SqlExpr::Column(cb)) => ca.table == cb.table && ca.name == cb.name,
        _ => a == b,
    }
}

/// Whether `e` holds a subquery (EXISTS, a scalar subquery, IN a subquery, a quantified comparison), found by the
/// rewriter's own walk.
fn has_subquery(e: &SqlExpr) -> Result<bool, Refusal> {
    let mut found = false;
    walk(e.clone(), &mut |x| {
        if matches!(
            x,
            SqlExpr::Exists { .. }
                | SqlExpr::ScalarSubquery { .. }
                | SqlExpr::InSubquery { .. }
                | SqlExpr::Quantified { .. }
        ) {
            found = true;
        }
        Ok(Rw::Same(x))
    })?;
    Ok(found)
}

impl SqlRewriter for QualifyToSubselect {
    fn name(&self) -> &'static str {
        "QualifyToSubselect"
    }

    fn select(&mut self, s: SqlSelect) -> Result<Rw<SqlQuery>, Refusal> {
        let Some(qualify) = s.qualify.clone() else {
            return same(s);
        };
        if self.by_outputs {
            return Ok(Rw::New(by_outputs(s)?));
        }
        let outputs = s.outputs.clone();
        let inner = build::select(SqlSelect { qualify: None, ..s })?;
        let from = build::subselect(query(inner), QUALIFY_ALIAS, None);
        Ok(Rw::New(query(wrapping(vec![star_projection()?], from, qualify, Vec::new(), None, None, outputs)?)))
    }
}

// ==================================================================
// RootNumericTypes
// ==================================================================

/// The dialect delivers the platform's type facts on its wire, here at the statement's boundary (Java's
/// `RootNumericTypes`): a root column the platform types DECIMAL(p,s) or HUGEINT, which Postgres would deliver as an
/// unsized numeric (a computed numeric keeps no precision, and there is no 128-bit integer), is cast to its type --
/// NUMERIC(p,s), HUGEINT's NUMERIC(38). Root only: a union's branches and a WITH's body are roots too; nothing inside
/// the statement is read. A DISTINCT select's sort key that was a cast column's expression is the cast one.
#[derive(Debug, Default)]
pub struct RootNumericTypes;

/// The SQL type of a Decimal no precision is declared for (the lowering's `PureSql.type(DECIMAL)`).
const UNBOUNDED_DECIMAL: SqlType = SqlType::Decimal { precision: 38, scale: 18 };

/// The type a root column is cast to: its label's DECIMAL(p,s) or HUGEINT, unless the label is the unbounded Decimal
/// (which states no scale of its own) or the column is already that (a cast to it, or a table's own column).
fn numeric(e: &SqlExpr, label: Option<&SqlType>) -> Option<SqlType> {
    let label = label?;
    if !(matches!(label, SqlType::Decimal { .. }) || *label == SqlType::Scalar(Scalar::Hugeint)) {
        return None;
    }
    if *label == UNBOUNDED_DECIMAL {
        return None;
    }
    let as_declared = matches!(e, SqlExpr::Cast { target, .. } if target == label)
        || matches!(e, SqlExpr::Column(c) if c.origin != Some(Origin::Derived));
    if as_declared { None } else { Some(label.clone()) }
}

fn typed(s: SqlSelect) -> Result<SqlSelect, Refusal> {
    // Java's HashMap<String, SqlType>: a repeated name, the last output's type
    let mut labels: HashMap<&str, &SqlType> = HashMap::new();
    for o in &s.outputs {
        labels.insert(&o.name, &o.r#type);
    }
    let mut ps = Vec::with_capacity(s.projections.len());
    // Java's HashMap<SqlExpr, SqlExpr> (by equality, a repeated key's last value): only looked up
    let mut cast: Vec<(SqlExpr, SqlExpr)> = Vec::new();
    for p in &s.projections {
        let name = output_name(p);
        let label = name.as_deref().and_then(|n| labels.get(n).copied());
        let fallback = match p.expr.r#type() {
            lite_sql::TypeFact::Typed { r#type, .. } => Some(r#type),
            _ => None,
        };
        match numeric(&p.expr, label.or(fallback)) {
            None => ps.push(p.clone()),
            Some(t) => {
                let c = build::cast(p.expr.clone(), t);
                ps.push(build::projection(c.clone(), name, p.out.clone())?);
                cast.push((p.expr.clone(), c));
            }
        }
    }
    if cast.is_empty() {
        return Ok(s);
    }
    // a DISTINCT select orders by its select list's expressions only: a key that was a cast column's expression is the
    // cast one now (the cast changes no value, so no order either)
    let order = if s.distinct {
        s.order_by
            .iter()
            .map(|k| match cast.iter().rev().find(|(from, _)| *from == k.expr) {
                None => k.clone(),
                Some((_, c)) => SortKey {
                    expr: c.clone(),
                    ascending: k.ascending,
                    null_order: k.null_order,
                    output_name: k.output_name.clone(),
                },
            })
            .collect()
    } else {
        s.order_by.clone()
    };
    build::select(SqlSelect { projections: ps, order_by: order, ..s })
}

impl SqlRewriter for RootNumericTypes {
    fn name(&self) -> &'static str {
        "RootNumericTypes"
    }

    fn rewrite_root(&mut self, q: SqlQuery) -> Result<SqlQuery, Refusal> {
        match q {
            SqlQuery::Select(s) => Ok(query(typed(*s)?)),
            // a new union always: its labels re-reconcile
            SqlQuery::Union(u) => {
                let branches = u.branches.into_iter().map(|b| self.rewrite_root(b)).collect::<Result<Vec<_>, _>>()?;
                Ok(SqlQuery::Union(build::union(branches, u.all, u.outputs)?))
            }
            SqlQuery::With(w) => {
                let body = self.rewrite_root(*w.body)?;
                build::prepend(w.ctes, body, w.recursive)
            }
        }
    }
}

// ==================================================================
// PivotAsGroupBy
// ==================================================================

/// A static pivot as a grouped CASE aggregation (Java's `PivotAsGroupBy`): every column of the pivot's output that is
/// not a pivoted one is a group key, and each value and aggregate `AGG(CASE WHEN on = v THEN arg END) AS
/// "v__|__alias"` -- the portable form `CarrierStrategies` writes for a database without lists, here for one with
/// lists and no PIVOT, where that pass stands aside. A dynamic pivot (no values), or one of several ON keys, is left
/// as it came.
#[derive(Debug, Default)]
pub struct PivotAsGroupBy;

/// A pivot value as the platform names its column (Java's `pivotValueText`): a string bare, an integer and a boolean
/// as their text; any other value refused, naming its class.
fn pivot_value_text(v: &SqlExpr) -> Result<String, Refusal> {
    match v {
        SqlExpr::StringLit { value, .. } => Ok(value.clone()),
        SqlExpr::IntLit { value, .. } => Ok(value.to_string()),
        SqlExpr::BoolLit { value, .. } => Ok(value.to_string()),
        other => Err(Refusal::capability(format!(
            "a pivot value {} reached a dialect without a native pivot, with no column name",
            crate::java::simple_name(other)
        ))),
    }
}

impl SqlRewriter for PivotAsGroupBy {
    fn name(&self) -> &'static str {
        "PivotAsGroupBy"
    }

    fn source(&mut self, s: SqlSource) -> Result<Rw<SqlSource>, Refusal> {
        // a dynamic pivot a cast names pivots on the named values (`Pivot.values()`: `in`'s, else the named ones)
        let SqlSource::Pivot { source, on, r#in, named, usings, alias, outputs } = &s else {
            return Ok(Rw::Same(s));
        };
        let values = if r#in.is_empty() { named } else { r#in };
        if values.is_empty() || on.len() != 1 {
            return Ok(Rw::Same(s));
        }
        let mut ps = Vec::new();
        let mut group = Vec::new();
        for oc in outputs {
            if !oc.name.contains("__|__") {
                let g = SqlExpr::Column(build::column_of(Some(source_alias(source)?), oc));
                group.push(g.clone());
                ps.push(build::projection(g, Some(oc.name.clone()), Some(oc.clone()))?);
            }
        }
        for v in values {
            for u in usings {
                let agg = &u.agg;
                let mut args = agg.args.clone();
                if args.is_empty() {
                    return Err(Refusal::capability(
                        "a pivot whose aggregate takes no argument reached a dialect without a native pivot",
                    ));
                }
                let first = std::mem::replace(&mut args[0], build::null_lit());
                args[0] = build::case(vec![build::when(call(SqlFn::Equal, vec![on[0].clone(), v.clone()]), first)], None);
                let name = format!("{}__|__{}", pivot_value_text(v)?, u.alias);
                let slot = generated_slot(outputs, u, &name)?;
                ps.push(build::projection(
                    SqlExpr::Reducer(build::reducer(agg.r#fn, args, agg.distinct, agg.order_by.clone())),
                    Some(name),
                    Some(slot),
                )?);
            }
        }
        let sel = build::with_group_by(build::with_projections(build::star_of(source.as_ref().clone())?, ps)?, group)?;
        Ok(Rw::New(build::subselect(query(sel), alias, None)))
    }
}

// ==================================================================
// AsOfAsPick
// ==================================================================

/// An as-of join for a dialect with none of its own (Java's `AsOfAsPick`): a select whose FROM is an as-of join, as a
/// LEFT JOIN on the same condition, each left row keeping its matches whose right key is the extreme one -- `QUALIFY
/// k = MAX(k) OVER (PARTITION BY every left column) OR MAX(k) OVER (...) IS NULL`, the second arm keeping a left row
/// with no match -- a QUALIFY, which `QualifyToSubselect` writes as a filter over a wrapping select. Only under a
/// select that filters, groups and windows nothing of its own.
#[derive(Debug, Default)]
pub struct AsOfAsPick;

/// Whether `e` holds a window call (Java's `windowed`).
fn windowed(e: &SqlExpr) -> bool {
    matches!(e, SqlExpr::WindowCall { .. }) || children(e).iter().any(|k| windowed(k))
}

impl SqlRewriter for AsOfAsPick {
    fn name(&self) -> &'static str {
        "AsOfAsPick"
    }

    fn select(&mut self, s: SqlSelect) -> Result<Rw<SqlQuery>, Refusal> {
        let SqlSource::Join { left, right, kind: JoinKind::AsofLeft, on, .. } = &s.from else {
            return same(s);
        };
        let Some((right_key, pick_max)) = as_of_key(&s.from)? else {
            return Err(Refusal::capability(
                "an as-of join with no inequality between its sides reached a dialect without an as-of join",
            ));
        };
        if s.r#where.is_some()
            || !s.group_by.is_empty()
            || s.having.is_some()
            || s.qualify.is_some()
            || s.projections.iter().any(|p| windowed(&p.expr))
        {
            return Err(Refusal::capability(
                "an as-of join under a select that filters, groups or windows reached a dialect without an as-of \
                 join: its pick is that select's filter",
            ));
        }
        let left_alias = source_alias(left)?.to_owned();
        let left_outputs = source_outputs(left);
        if left_outputs.is_empty() {
            return Err(Refusal::capability(
                "an as-of join whose left side has no alias and columns reached a dialect without an as-of join",
            ));
        }
        let partition: Vec<SqlExpr> =
            left_outputs.iter().map(|oc| SqlExpr::Column(build::column_of(Some(&left_alias), oc))).collect();
        let extreme = build::window_call(
            SqlAgg::Reducer(build::reducer(
                if pick_max { AggFn::Max } else { AggFn::Min },
                vec![right_key.clone()],
                false,
                Vec::new(),
            )),
            partition,
            Vec::new(),
            None,
        );
        let pick = call(
            SqlFn::Or,
            vec![call(SqlFn::Equal, vec![right_key, extreme.clone()]), call(SqlFn::IsNull, vec![extreme])],
        );
        let join = build::join(
            left.as_ref().clone(),
            right.as_ref().clone(),
            JoinKind::Left,
            on.as_deref().cloned(),
        )?;
        Ok(Rw::New(query(build::with_qualify(build::with_from(s, join)?, Some(pick))?)))
    }
}

#[cfg(test)]
mod tests;
