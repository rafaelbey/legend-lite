// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! Java's `MongoDbPipeline` (`core/src/main/java/com/legend/sql/dialect/MongoDbPipeline.java`): a query as MongoDB's
//! aggregation pipeline (docs/MONGODB_PLAN_2026_10_16.md §5.1) -- the stages [`MongoDb`] writes for a select, a union
//! and their sources. A select's result rows are flat documents; inside a select the current row holds each source
//! under its alias; a join is a `$lookup` and an `$unwind`; a group is one `$group` whose accumulators are read back
//! through markers; a sort key is computed into a carried field with its null flag; a literal relation is
//! `$documents`. Each function is Java's method of the same name, its text and refusals Java's, in Java's order.

use super::aggregates::{self, GroupField};
use super::json;
use super::{AGGREGATE_MARKER, MongoDb, Pc, SUBQUERY_MARKER, WINDOW_MARKER, java_text, pc, text};
use crate::build;
use crate::carrier;
use crate::refusal::{Refusal, arg};
use crate::typing::{aggregates_select, children, query_outputs, source_alias, source_outputs};
use crate::writer::SqlWriter;
use lite_sql::{
    Bound, Frame, FrameKind, JoinKind, NullOrder, Origin, OutputCol, Projection, Quantifier, SortKey, SqlAgg, SqlExpr,
    SqlFn, SqlQuery, SqlSelect, SqlSource, SqlUnion,
};
use std::collections::BTreeMap;

/// The stages written so far at one level: a comma before each but the first.
#[derive(Default)]
struct Stages {
    written: usize,
}

impl Stages {
    fn begin<'w>(&mut self, w: &'w mut SqlWriter) -> &'w mut SqlWriter {
        if self.written > 0 {
            w.append(", ");
        }
        self.written += 1;
        w
    }
}

/// How a source's rows are shaped: under the source's alias, or flat (the right side of a lookup).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Shape {
    Wrapped,
    Flat,
}

// ==================================================================
// The command
// ==================================================================

/// The whole `aggregate` command for `q`: over its base collection, or the database when the query reads only
/// literal relations.
pub fn command(d: &MongoDb, w: &mut SqlWriter, q: &SqlQuery) -> Result<(), Refusal> {
    let base = base_collection(q)?;
    let head = match &base {
        None => "1".to_owned(),
        Some(b) => json::string(b),
    };
    w.append("{\"aggregate\": ").append(&head).append(", \"pipeline\": [");
    query(d, w, q, &mut Stages::default())?;
    w.append("], \"cursor\": {}}");
    Ok(())
}

fn no_with() -> Refusal {
    Refusal::capability("a WITH (common table expressions) reached MongoDb, which has no shape for one yet")
}

/// The collection a query's pipeline starts from: its FROM's base table, read through its subselects and down the left
/// side of its joins; none for a query over literal rows alone.
fn base_collection(q: &SqlQuery) -> Result<Option<String>, Refusal> {
    match q {
        SqlQuery::Select(s) => base_of(&s.from),
        SqlQuery::Union(u) => base_collection(first_branch(u)?),
        SqlQuery::With(_) => Err(no_with()),
    }
}

fn first_branch(u: &SqlUnion) -> Result<&SqlQuery, Refusal> {
    u.branches.first().ok_or_else(|| Refusal {
        class: Refusal::INDEX_OUT_OF_BOUNDS,
        message: format!("Index 0 out of bounds for length {}", u.branches.len()),
    })
}

fn base_of(src: &SqlSource) -> Result<Option<String>, Refusal> {
    match src {
        SqlSource::Table { name, call, .. } => collection_name(name, *call).map(Some),
        SqlSource::Subselect { inner, .. } => base_collection(inner),
        SqlSource::Join { left, .. } => base_of(left),
        SqlSource::Values { .. } | SqlSource::Dual => Ok(None),
        SqlSource::Cte { .. } => Err(Refusal::capability("a CTE reference reached MongoDb: no shape yet")),
        SqlSource::VarSetPlaceholder { var_name, .. } => Err(Refusal::illegal_state(format!(
            "a plan variable ({var_name}) reached an executable dialect: plan-text vocabulary only"
        ))),
        SqlSource::RawSql { .. } => Err(Refusal::capability(
            "a raw statement as a relation reached MongoDb: a user's own pipeline as a source is not run yet",
        )),
        SqlSource::Catalog { .. } => Err(Refusal::capability(
            "a catalog grid reached MongoDb: its catalog (listCollections) is not read as a grid yet",
        )),
        SqlSource::SourceUrl { .. } => {
            Err(Refusal::capability("sourceUrl reached MongoDb: no external source encoding"))
        }
        SqlSource::Pivot { source, .. } => base_of(source),
    }
}

/// A collection's name: a table's bare name; a schema-qualified or tabular-function table is refused.
fn collection_name(name: &str, call: bool) -> Result<String, Refusal> {
    if call {
        return Err(Refusal::capability(format!("a tabular function ({name}) reached MongoDb, which has none")));
    }
    if name.contains('.') {
        return Err(Refusal::capability(format!(
            "a schema-qualified table ({name}) reached MongoDb: a collection has no schema (its database is the \
             connection's)"
        )));
    }
    Ok(name.to_owned())
}

// ==================================================================
// Queries
// ==================================================================

/// The query's stages; the fields its last projection names, or none when a star carries more than it names.
fn query(d: &MongoDb, w: &mut SqlWriter, q: &SqlQuery, st: &mut Stages) -> Result<Option<Vec<String>>, Refusal> {
    match q {
        SqlQuery::Select(s) => select(d, w, s, st),
        SqlQuery::Union(u) => union(d, w, u, st),
        SqlQuery::With(_) => Err(no_with()),
    }
}

/// A union: the first branch's stages, then each other branch as a `$unionWith` over its own base; a deduplicating
/// union groups the rows afterwards.
fn union(d: &MongoDb, w: &mut SqlWriter, u: &SqlUnion, st: &mut Stages) -> Result<Option<Vec<String>>, Refusal> {
    let projected = query(d, w, first_branch(u)?, st)?;
    for branch in u.branches.iter().skip(1) {
        let base = base_collection(branch)?;
        st.begin(w).append("{\"$unionWith\": {");
        if let Some(b) = base {
            w.append("\"coll\": ").append(&json::string(&b)).append(", ");
        }
        w.append("\"pipeline\": [");
        query(d, w, branch, &mut Stages::default())?;
        w.append("]}}");
    }
    if !u.all {
        distinct(w, st);
    }
    Ok(projected)
}

/// The rows made distinct: grouped by the whole row, then each group's row.
fn distinct(w: &mut SqlWriter, st: &mut Stages) {
    st.begin(w).append("{\"$group\": {\"_id\": \"$$ROOT\"}}");
    st.begin(w).append("{\"$replaceWith\": \"$_id\"}");
}

/// One select, clause by clause, in SQL's order of evaluation.
fn select(d: &MongoDb, w: &mut SqlWriter, s: &SqlSelect, st: &mut Stages) -> Result<Option<Vec<String>>, Refusal> {
    source(d, w, &s.from, st, Shape::Wrapped)?;
    // the row wrapped under one alias: an unqualified column reads under it until the projection flattens it
    let aliases = aliases(&s.from)?;
    let sole =
        if aliases.len() == 1 && !matches!(s.from, SqlSource::Join { .. }) { Some(aliases[0].clone()) } else { None };
    d.sole_alias(sole)?;
    let clauses = subqueries(d, w, s, st)?;
    if let Some(wh) = &clauses.r#where {
        match_stage(d, w, st, wh)?;
    }
    let g = group(d, w, &s.group_by, clauses.projections, clauses.having, clauses.order_by, st)?;
    if let Some(h) = &g.having {
        match_stage(d, w, st, h)?;
    }
    let g = windows(d, w, st, g, s.qualify.as_ref())?;
    let sorts = sort_carries(d, w, st, &g)?;
    let projected = projection(d, w, s, st, &g, &sorts)?;
    d.sole_alias(None)?;
    if s.distinct {
        // the sort carries are functions of the row's columns, so equal rows carry equal keys
        distinct(w, st);
    }
    if !sorts.is_empty() {
        st.begin(w).append("{\"$sort\": {");
        for (i, c) in sorts.iter().enumerate() {
            if i > 0 {
                w.append(", ");
            }
            w.append(&json::string(&c.null_field)).append(": ").append(if c.nulls_first { "-1" } else { "1" });
            w.append(", ").append(&json::string(&c.field)).append(": ").append(if c.ascending { "1" } else { "-1" });
        }
        w.append("}}");
        st.begin(w).append("{\"$unset\": [");
        for (i, c) in sorts.iter().enumerate() {
            if i > 0 {
                w.append(", ");
            }
            w.append(&json::string(&c.field)).append(", ").append(&json::string(&c.null_field));
        }
        w.append("]}");
    }
    if let Some(offset) = s.offset {
        st.begin(w).append("{\"$skip\": ").append_long(offset).append("}");
    }
    if let Some(limit) = s.limit {
        st.begin(w).append("{\"$limit\": ").append_long(limit).append("}");
    }
    Ok(projected)
}

/// A projection's output name: its alias, else a bare column's name, else a stored read's column's (Java's
/// `Projection.outputName()`).
fn output_name(p: &Projection) -> Option<String> {
    match (&p.alias, &p.expr) {
        (Some(a), _) => Some(a.clone()),
        (None, SqlExpr::Column(c)) => Some(c.name.clone()),
        (None, SqlExpr::StoredRead { column, .. }) => Some(column.name.clone()),
        _ => None,
    }
}

/// `p` with its expression replaced: itself when the walk handed the expression back (Java's `e == p.expr()`), else
/// a new projection keeping its output's name.
fn reprojected(p: &Projection, e: Option<SqlExpr>) -> Result<Projection, Refusal> {
    match e {
        None => Ok(p.clone()),
        Some(e) => build::projection(e, p.alias.clone().or_else(|| output_name(p)), p.out.clone()),
    }
}

// ==================================================================
// Subqueries
// ==================================================================

/// A select's clauses with each subquery read through its lookup's marker.
struct Clauses {
    r#where: Option<SqlExpr>,
    projections: Vec<Projection>,
    having: Option<SqlExpr>,
    order_by: Vec<SortKey>,
}

/// The subqueries of a select's expressions, each replaced by its marker.
#[derive(Default)]
struct Subqueries {
    found: Vec<SqlExpr>,
}

impl Subqueries {
    /// The expression with its subqueries replaced; none when it is the expression handed in.
    fn apply(&mut self, e: &SqlExpr) -> Result<Option<SqlExpr>, Refusal> {
        Ok(match e {
            SqlExpr::ScalarSubquery { .. } | SqlExpr::Exists { .. } => Some(self.marker(e)),
            SqlExpr::InSubquery { value, .. } => {
                let v = self.applied(value)?;
                Some(build::membership(v, self.marker(e)))
            }
            SqlExpr::Quantified { value, comparison, quantifier, .. } => {
                let v = self.applied(value)?;
                let rows = self.marker(e);
                let row = format!("__q{}", self.found.len() - 1);
                let compared = build::call(*comparison, vec![v, build::derived(None, &row)]);
                let f = if *quantifier == Quantifier::Any { SqlFn::ListExists } else { SqlFn::ListForAll };
                Some(build::call(f, vec![rows, build::lambda(&[row.as_str()], compared)]))
            }
            _ => carrier::map_children(e, &mut |c| self.apply(c))?,
        })
    }

    fn applied(&mut self, e: &SqlExpr) -> Result<SqlExpr, Refusal> {
        Ok(self.apply(e)?.unwrap_or_else(|| e.clone()))
    }

    fn marker(&mut self, subquery: &SqlExpr) -> SqlExpr {
        self.found.push(subquery.clone());
        build::derived(None, &format!("{SUBQUERY_MARKER}{}", self.found.len() - 1))
    }
}

/// Each subquery of the select as a `$lookup` into a marker field, after the sources and before the clauses that read
/// it: the row's aliases are the lookup's `let` variables.
fn subqueries(d: &MongoDb, w: &mut SqlWriter, s: &SqlSelect, st: &mut Stages) -> Result<Clauses, Refusal> {
    let mut sub = Subqueries::default();
    let r#where = match &s.r#where {
        None => None,
        Some(e) => Some(sub.applied(e)?),
    };
    let mut ps = Vec::with_capacity(s.projections.len());
    for p in &s.projections {
        let e = sub.apply(&p.expr)?;
        ps.push(reprojected(p, e)?);
    }
    let having = match &s.having {
        None => None,
        Some(e) => Some(sub.applied(e)?),
    };
    let mut keys = Vec::with_capacity(s.order_by.len());
    for k in &s.order_by {
        keys.push(build::sort_key(sub.applied(&k.expr)?, k.ascending, k.null_order, k.output_name.clone()));
    }
    if !sub.found.is_empty() {
        let outer = aliases(&s.from)?;
        let mut reads: Vec<(String, Pc<'static>)> = Vec::new();
        for (i, found) in sub.found.iter().enumerate() {
            let field = format!("{SUBQUERY_MARKER}{i}");
            let inner: &SqlQuery = match found {
                SqlExpr::ScalarSubquery { subquery, .. }
                | SqlExpr::Exists { subquery, .. }
                | SqlExpr::InSubquery { subquery, .. }
                | SqlExpr::Quantified { subquery, .. } => subquery,
                other => {
                    return Err(Refusal::illegal_state(format!("not a subquery: {}", java_text::expr(other))));
                }
            };
            let base = base_collection(inner)?;
            st.begin(w).append("{\"$lookup\": {");
            if let Some(b) = base {
                w.append("\"from\": ").append(&json::string(&b)).append(", ");
            }
            w.append("\"let\": {");
            for (a, alias) in outer.iter().enumerate() {
                w.append(if a > 0 { ", " } else { "" }).append(&json::string(&json::variable(alias)));
                w.append(": ").append(&json::path(&[alias]));
            }
            w.append("}, \"pipeline\": [");
            d.push_lookup_scope(&outer, None);
            let projected = query(d, w, inner, &mut Stages::default());
            d.pop_scope();
            let projected = projected?;
            w.append("], \"as\": ").append(&json::key(&field)?).append("}}");
            let read = subquery_read(found, &field, inner, projected.as_deref())?;
            reads.push((field, read));
        }
        d.subquery_reads(reads)?;
    }
    Ok(Clauses { r#where, projections: ps, having, order_by: keys })
}

fn subquery_read(
    subquery: &SqlExpr,
    field: &str,
    inner: &SqlQuery,
    projected: Option<&[String]>,
) -> Result<Pc<'static>, Refusal> {
    let path = json::string(&format!("${field}"));
    Ok(match subquery {
        SqlExpr::ScalarSubquery { .. } => text(
            [
                "{\"$let\": {\"vars\": {\"r\": {\"$arrayElemAt\": [",
                &path,
                ", 0]}}, \"in\": ",
                &json::string(&format!("$$r.{}", read_field(inner, projected)?)),
                "}}",
            ]
            .concat(),
        ),
        SqlExpr::Exists { .. } => text(["{\"$gt\": [{\"$size\": ", &path, "}, 0]}"].concat()),
        SqlExpr::InSubquery { .. } | SqlExpr::Quantified { .. } => text(
            [
                "{\"$map\": {\"input\": ",
                &path,
                ", \"in\": ",
                &json::string(&format!("$$this.{}", read_field(inner, projected)?)),
                "}}",
            ]
            .concat(),
        ),
        other => return Err(Refusal::illegal_state(format!("not a subquery: {}", java_text::expr(other)))),
    })
}

/// The one field a subquery in expression position reads: the field its pipeline's projection stage named; else its
/// one output column.
fn read_field(inner: &SqlQuery, projected: Option<&[String]>) -> Result<String, Refusal> {
    if let Some(p) = projected
        && p.len() == 1
    {
        return Ok(p[0].clone());
    }
    let outs = query_outputs(inner);
    if outs.len() != 1 {
        return Err(Refusal::capability(format!(
            "a subquery of {} columns reached MongoDb in expression position: one column is read",
            outs.len()
        )));
    }
    Ok(outs[0].name.clone())
}

/// The field the `i`th projection becomes: its alias, else its output column's name, else positional.
fn field_name(p: &Projection, i: usize) -> String {
    output_name(p).unwrap_or_else(|| format!("_c{i}"))
}

fn match_stage(d: &MongoDb, w: &mut SqlWriter, st: &mut Stages, condition: &SqlExpr) -> Result<(), Refusal> {
    st.begin(w).append("{\"$match\": {\"$expr\": ").expr(d, condition, 0)?.append("}}");
    Ok(())
}

// ==================================================================
// Sources
// ==================================================================

fn source(d: &MongoDb, w: &mut SqlWriter, src: &SqlSource, st: &mut Stages, shape: Shape) -> Result<(), Refusal> {
    match src {
        SqlSource::Table { name, alias, outputs, call, .. } => scan(w, name, *call, alias, outputs, st, shape),
        SqlSource::Values { rows, columns, alias, .. } => values(d, w, rows, columns, alias, st, shape),
        SqlSource::Dual => {
            st.begin(w).append("{\"$documents\": [{}]}");
            Ok(())
        }
        SqlSource::Subselect { inner, alias, .. } => {
            query(d, w, inner, st)?;
            if shape == Shape::Wrapped {
                st.begin(w).append("{\"$replaceWith\": {").append(&json::key(alias)?).append(": \"$$ROOT\"}}");
            }
            Ok(())
        }
        SqlSource::Join { .. } => {
            if shape == Shape::Flat {
                return Err(Refusal::capability(
                    "a join on the right side of a join reached MongoDb: a lookup over a joined source has no shape \
                     yet",
                ));
            }
            join(d, w, src, st)
        }
        SqlSource::Pivot { .. } => pivot(d, w, src, st, shape),
        other => base_of(other).map(|_| ()),
    }
}

/// The separator of a pivot column's name (its value, then its USING alias).
const PIVOT_SEPARATOR: &str = "__|__";

/// A pivot value's text, as the pivot column's name spells it: a number's digits, a string itself.
fn pivot_value_text(v: &SqlExpr) -> Result<String, Refusal> {
    match v {
        SqlExpr::IntLit { value, .. } => Ok(value.to_string()),
        SqlExpr::StringLit { value, .. } => Ok(value.clone()),
        SqlExpr::FloatLit { value, .. } => Ok(lite_javacompat::double_to_string(value.value())),
        SqlExpr::BoolLit { value, .. } => Ok(value.to_string()),
        other => Err(Refusal::capability(format!(
            "a pivot value that is no literal ({}) reached MongoDb",
            java_text::expr(other)
        ))),
    }
}

/// Java's `String.substring(begin, end)` refused as it words a bad range.
fn substring_refused(begin: i64, end: i64, length: usize) -> Refusal {
    Refusal {
        class: "java.lang.StringIndexOutOfBoundsException",
        message: format!("begin {begin}, end {end}, length {length}"),
    }
}

/// A static pivot as two groups and a projection: the source's rows grouped by the pivot's keys and its ON column,
/// each USING aggregate accumulated; the groups regrouped by the keys alone, the ON value and the aggregates pushed;
/// each pivot column projected as the pushed aggregate of its value.
fn pivot(d: &MongoDb, w: &mut SqlWriter, src: &SqlSource, st: &mut Stages, shape: Shape) -> Result<(), Refusal> {
    let SqlSource::Pivot { source: inner, on, r#in, named, usings, alias, outputs } = src else {
        return Err(Refusal::illegal_state("pivot is handed a Pivot"));
    };
    let values = if r#in.is_empty() { named } else { r#in };
    if values.is_empty() {
        return Err(Refusal::capability(
            "a dynamic pivot reached MongoDb: its columns are known only once the database is asked (a static \
             pivot, its values given, has its grouped form)",
        ));
    }
    if on.len() != 1 {
        return Err(Refusal::capability(format!(
            "a static pivot over {} columns reached MongoDb: its values name one column's",
            on.len()
        )));
    }
    // the source's row wrapped under its alias
    source(d, w, inner, st, Shape::Wrapped)?;
    let keys: Vec<&OutputCol> = outputs.iter().filter(|c| !c.name.contains(PIVOT_SEPARATOR)).collect();
    let mut pivoted: Vec<OutputCol> = outputs.iter().filter(|c| c.name.contains(PIVOT_SEPARATOR)).cloned().collect();
    if pivoted.is_empty() && r#in.is_empty() {
        // a dynamic pivot a cast names: its columns are the named values' by each USING
        let mut generated = Vec::new();
        for v in named {
            for u in usings {
                let name = format!("{}{PIVOT_SEPARATOR}{}", pivot_value_text(v)?, u.alias);
                generated.push(carrier::generated_slot(outputs, u, &name)?);
            }
        }
        pivoted = generated;
    }
    // the first group: by the keys and the ON value, each USING aggregate accumulated
    st.begin(w).append("{\"$group\": {\"_id\": {");
    for (i, k) in keys.iter().enumerate() {
        let path = json::path(&[source_alias(inner)?, &k.name]);
        w.append("\"k").append_long(i as i64).append("\": ").append(&path).append(", ");
    }
    w.append("\"on\": ").expr(d, &on[0], 0)?.append("}");
    let mut reads: Vec<Pc<'static>> = Vec::new();
    let mut by_field: Vec<(String, Pc<'static>)> = Vec::new();
    for (i, u) in usings.iter().enumerate() {
        let field = format!("{AGGREGATE_MARKER}{i}");
        w.append(", ").append(&json::string(&field)).append(": ");
        let gf = GroupField::new(&field, false)?;
        let read = aggregates::accumulator(d, w, &SqlExpr::Reducer(u.agg.clone()), &gf)?;
        reads.push(read.clone());
        by_field.push((field, read));
    }
    w.append("}}");
    // the second group: by the keys alone, the ON value and the aggregates' reads pushed
    d.aggregate_reads(by_field)?;
    st.begin(w).append("{\"$group\": {\"_id\": {");
    for i in 0..keys.len() {
        w.append(if i > 0 { ", " } else { "" }).append("\"k").append_long(i as i64).append("\": \"$_id.k");
        w.append_long(i as i64).append("\"");
    }
    w.append("}, \"__p\": {\"$push\": {\"on\": \"$_id.on\"");
    for (i, r) in reads.iter().enumerate() {
        w.append(", \"a").append_long(i as i64).append("\": ");
        r(d, w)?;
    }
    w.append("}}}}");
    // the projection: the keys, and each pivot column as its value's pushed aggregate
    st.begin(w).append("{\"$project\": {\"_id\": 0, ");
    if shape == Shape::Wrapped {
        w.append(&json::key(alias)?).append(": {");
    }
    for (i, k) in keys.iter().enumerate() {
        w.append(if i > 0 { ", " } else { "" }).append(&json::key(&k.name)?).append(": \"$_id.k");
        w.append_long(i as i64).append("\"");
    }
    for c in &pivoted {
        let name = c.name.as_str();
        let bare = if name.starts_with('\'') && name.ends_with('\'') {
            if name.len() < 2 {
                return Err(substring_refused(1, name.encode_utf16().count() as i64 - 1, name.encode_utf16().count()));
            }
            &name[1..name.len() - 1]
        } else {
            name
        };
        let Some(sep) = bare.rfind(PIVOT_SEPARATOR) else {
            return Err(substring_refused(0, -1, bare.encode_utf16().count()));
        };
        let value_text = &bare[..sep];
        let using_alias = &bare[sep + PIVOT_SEPARATOR.len()..];
        let using = usings.iter().rposition(|u| u.alias == using_alias);
        let mut value = None;
        for v in values {
            if value_text == pivot_value_text(v)? {
                value = Some(v);
            }
        }
        let (Some(using), Some(value)) = (using, value) else {
            let aliases: Vec<String> = usings.iter().map(|u| u.alias.clone()).collect();
            return Err(Refusal::illegal_state(format!(
                "the pivot column {name} names no USING alias and value of the pivot ({}, {})",
                java_text::exprs(values),
                java_text::strings(&aliases)
            )));
        };
        let first = pivoted.iter().position(|x| x == c) == Some(0);
        w.append(if keys.is_empty() && first { "" } else { ", " }).append(&json::key(name)?);
        w.append(concat!(
            ": {\"$let\": {\"vars\": {\"m\": {\"$arrayElemAt\": [{\"$filter\": {\"input\": \"$__p\",",
            " \"cond\": {\"$eq\": [\"$$this.on\", "
        ));
        w.expr(d, value, 0)?.append("]}}}, 0]}}, \"in\": {\"$ifNull\": [\"$$m.a").append_long(using as i64);
        w.append("\", null]}}}");
    }
    if shape == Shape::Wrapped {
        w.append("}");
    }
    w.append("}}");
    Ok(())
}

/// A collection's scan: each declared column read with `$ifNull`, so an absent field is null (D7), under the alias or
/// flat; the document's own `_id` dropped unless the store declares it.
fn scan(
    w: &mut SqlWriter,
    name: &str,
    call: bool,
    alias: &str,
    outputs: &[OutputCol],
    st: &mut Stages,
    shape: Shape,
) -> Result<(), Refusal> {
    collection_name(name, call)?;
    let declares_id = outputs.iter().any(|c| c.name == "_id");
    st.begin(w).append("{\"$project\": {");
    if !declares_id {
        w.append("\"_id\": 0").append(if outputs.is_empty() { "" } else { ", " });
    }
    if shape == Shape::Wrapped {
        w.append(&json::key(alias)?).append(": {");
    }
    for (i, c) in outputs.iter().enumerate() {
        if i > 0 {
            w.append(", ");
        }
        w.append(&json::key(&c.name)?).append(": {\"$ifNull\": [").append(&json::path(&[&c.name])).append(", null]}");
    }
    if shape == Shape::Wrapped {
        w.append("}");
    }
    w.append("}}");
    Ok(())
}

/// Literal rows as `$documents`, each row's cells under the relation's column names.
fn values(
    d: &MongoDb,
    w: &mut SqlWriter,
    rows: &[Vec<SqlExpr>],
    columns: &[String],
    alias: &str,
    st: &mut Stages,
    shape: Shape,
) -> Result<(), Refusal> {
    st.begin(w).append("{\"$documents\": [");
    for (r, row) in rows.iter().enumerate() {
        if r > 0 {
            w.append(", ");
        }
        if shape == Shape::Wrapped {
            w.append("{").append(&json::key(alias)?).append(": ");
        }
        w.append("{");
        for (c, cell) in row.iter().enumerate() {
            if c > 0 {
                w.append(", ");
            }
            let column = columns.get(c).ok_or_else(|| Refusal {
                class: Refusal::INDEX_OUT_OF_BOUNDS,
                message: format!("Index {c} out of bounds for length {}", columns.len()),
            })?;
            w.append(&json::key(column)?).append(": ").expr(d, cell, 0)?;
        }
        w.append("}");
        if shape == Shape::Wrapped {
            w.append("}");
        }
    }
    w.append("]}");
    Ok(())
}

/// The aliases a source brings to the row, in order.
fn aliases(src: &SqlSource) -> Result<Vec<String>, Refusal> {
    Ok(match src {
        SqlSource::Join { left, right, .. } => {
            let mut out = aliases(left)?;
            out.extend(aliases(right)?);
            out
        }
        SqlSource::Dual => Vec::new(),
        other => vec![source_alias(other)?.to_owned()],
    })
}

/// A join as a `$lookup` and an `$unwind`; an as-of join keeps the nearest right row; a right join looks the left side
/// up from the right; a full join appends the right rows no left row matched.
fn join(d: &MongoDb, w: &mut SqlWriter, j: &SqlSource, st: &mut Stages) -> Result<(), Refusal> {
    let SqlSource::Join { left, right, kind, on, .. } = j else {
        return Err(Refusal::illegal_state("join is handed a Join"));
    };
    let on = on.as_deref();
    if matches!(right.as_ref(), SqlSource::Join { .. }) {
        return Err(Refusal::capability(
            "a join on the right side of a join reached MongoDb: a lookup over a joined source has no shape yet",
        ));
    }
    match kind {
        JoinKind::Inner | JoinKind::Cross | JoinKind::CrossLateral => {
            lookup_join(d, w, st, left, right, on, false, None)
        }
        JoinKind::Left | JoinKind::LeftLateral => lookup_join(d, w, st, left, right, on, true, None),
        JoinKind::AsofLeft => {
            let nearest = as_of_key(on, source_alias(right)?)?;
            lookup_join(d, w, st, left, right, on, true, Some(&nearest))
        }
        JoinKind::Right => {
            one_source(left, *kind)?;
            lookup_join(d, w, st, right, left, on, true, None)
        }
        JoinKind::Full => {
            one_source(left, *kind)?;
            lookup_join(d, w, st, left, right, on, true, None)?;
            st.begin(w).append("{\"$unionWith\": {");
            if let Some(b) = base_of(right)? {
                w.append("\"coll\": ").append(&json::string(&b)).append(", ");
            }
            w.append("\"pipeline\": [");
            let mut inner = Stages::default();
            source(d, w, right, &mut inner, Shape::Wrapped)?;
            lookup(d, w, &mut inner, &aliases(right)?, left, on, None)?;
            let left_alias = json::key(source_alias(left)?)?;
            inner.begin(w).append("{\"$match\": {").append(&left_alias).append(": {\"$size\": 0}}}");
            // the left side (an empty array here) replaced by its null columns -- under $literal
            let left_alias = json::key(source_alias(left)?)?;
            inner.begin(w).append("{\"$addFields\": {").append(&left_alias).append(": {\"$literal\": ");
            nulls(w, left)?;
            w.append("}}}");
            w.append("]}}");
            Ok(())
        }
    }
}

fn one_source(left: &SqlSource, kind: JoinKind) -> Result<(), Refusal> {
    if matches!(left, SqlSource::Join { .. }) {
        return Err(Refusal::capability(format!(
            "a {kind} join over a joined left side reached MongoDb: one alias looks up one source"
        )));
    }
    Ok(())
}

/// The outer source wrapped, the inner looked up and unwound; the outer rows kept (`preserve`) with the inner side
/// padded, or dropped where the inner is empty.
#[allow(clippy::too_many_arguments)]
fn lookup_join(
    d: &MongoDb,
    w: &mut SqlWriter,
    st: &mut Stages,
    outer: &SqlSource,
    inner: &SqlSource,
    on: Option<&SqlExpr>,
    preserve: bool,
    nearest: Option<&SortKey>,
) -> Result<(), Refusal> {
    source(d, w, outer, st, Shape::Wrapped)?;
    lookup(d, w, st, &aliases(outer)?, inner, on, nearest)?;
    let path = json::path(&[source_alias(inner)?]);
    st.begin(w).append("{\"$unwind\": {\"path\": ").append(&path).append(", \"preserveNullAndEmptyArrays\": ");
    w.append(if preserve { "true" } else { "false" }).append("}}");
    if preserve {
        pad(w, st, inner)?;
    }
    Ok(())
}

/// The `$lookup` of `inner` into the field its alias names, the outer aliases its variables.
fn lookup(
    d: &MongoDb,
    w: &mut SqlWriter,
    st: &mut Stages,
    outer: &[String],
    inner: &SqlSource,
    on: Option<&SqlExpr>,
    nearest: Option<&SortKey>,
) -> Result<(), Refusal> {
    let r#as = source_alias(inner)?.to_owned();
    let base = base_of(inner)?;
    st.begin(w).append("{\"$lookup\": {");
    if let Some(b) = base {
        w.append("\"from\": ").append(&json::string(&b)).append(", ");
    }
    w.append("\"let\": {");
    for (i, o) in outer.iter().enumerate() {
        if i > 0 {
            w.append(", ");
        }
        w.append(&json::string(&json::variable(o))).append(": ").append(&json::path(&[o]));
    }
    w.append("}, \"pipeline\": [");
    d.push_lookup_scope(outer, Some(&r#as));
    let written = (|| -> Result<(), Refusal> {
        let mut stages = Stages::default();
        source(d, w, inner, &mut stages, Shape::Flat)?;
        if let Some(on) = on {
            match_stage(d, w, &mut stages, on)?;
        }
        if let Some(k) = nearest {
            let SqlExpr::Column(c) = &k.expr else {
                return Err(Refusal::class_cast(crate::java::simple_name(&k.expr), "Column"));
            };
            let key = json::key(&c.name)?;
            stages.begin(w).append("{\"$sort\": {").append(&key).append(": ");
            w.append(if k.ascending { "1" } else { "-1" }).append("}}");
            stages.begin(w).append("{\"$limit\": 1}");
        }
        Ok(())
    })();
    d.pop_scope();
    written?;
    w.append("], \"as\": ").append(&json::key(&r#as)?).append("}}");
    Ok(())
}

/// A missed row: the side's columns, each null, so every later read finds the field.
fn pad(w: &mut SqlWriter, st: &mut Stages, side: &SqlSource) -> Result<(), Refusal> {
    let alias = source_alias(side)?;
    st.begin(w).append("{\"$addFields\": {").append(&json::key(alias)?).append(": {\"$ifNull\": [");
    w.append(&json::path(&[alias])).append(", ");
    nulls(w, side)?;
    w.append("]}}}");
    Ok(())
}

/// `{col: null, ...}`: the side's columns, each null.
fn nulls(w: &mut SqlWriter, side: &SqlSource) -> Result<(), Refusal> {
    w.append("{");
    for (i, c) in source_outputs(side).iter().enumerate() {
        if i > 0 {
            w.append(", ");
        }
        w.append(&json::key(&c.name)?).append(": null");
    }
    w.append("}");
    Ok(())
}

/// The right-side column an as-of join's match inequality compares, and the direction that puts the nearest right row
/// first.
fn as_of_key(on: Option<&SqlExpr>, right: &str) -> Result<SortKey, Refusal> {
    let mut m = on;
    while let Some(SqlExpr::Call { r#fn: SqlFn::And, args, .. }) = m {
        m = Some(args.last().ok_or_else(|| Refusal {
            class: Refusal::INDEX_OUT_OF_BOUNDS,
            message: "Index -1 out of bounds for length 0".to_owned(),
        })?);
    }
    let Some(cmp @ SqlExpr::Call { r#fn: cmp_fn, args, .. }) = m else {
        return Err(Refusal::illegal_state(format!(
            "an as-of join's match is not a comparison: {}",
            m.map_or_else(|| "null".to_owned(), java_text::expr)
        )));
    };
    if args.len() != 2 {
        return Err(Refusal::illegal_state(format!(
            "an as-of join's match is not a comparison: {}",
            java_text::expr(cmp)
        )));
    }
    let first = column_of(&args[0]);
    let second = column_of(&args[1]);
    let right_first = first.is_some_and(|c| c.table.as_deref() == Some(right));
    let key = if right_first { first } else { second };
    let Some(key) = key.filter(|k| k.table.as_deref() == Some(right)) else {
        return Err(Refusal::capability(format!(
            "an as-of join matching on {} reached MongoDb: its lookup sorts by the right side's column",
            java_text::expr(cmp)
        )));
    };
    let left_greater = match cmp_fn {
        SqlFn::Greater | SqlFn::GreaterEqual => !right_first,
        SqlFn::Less | SqlFn::LessEqual => right_first,
        other => {
            return Err(Refusal::illegal_state(format!("an as-of join's match is not an inequality: {other}")));
        }
    };
    Ok(build::sort_key(SqlExpr::Column(key.clone()), !left_greater, None, None))
}

/// The column under a comparison's operand -- itself, or wrapped in a checked narrowing or parentheses.
fn column_of(e: &SqlExpr) -> Option<&lite_sql::Column> {
    match e {
        SqlExpr::Column(c) => Some(c),
        SqlExpr::CheckedOne { list, .. } => column_of(list),
        SqlExpr::Group { inner, .. } => column_of(inner),
        _ => None,
    }
}

// ==================================================================
// Grouping
// ==================================================================

/// A grouped select's rewritten clauses: each aggregate node a marker the group stage computed, each group key a read
/// of the group's `_id`.
struct Grouping {
    projections: Vec<Projection>,
    having: Option<SqlExpr>,
    order_by: Vec<SortKey>,
}

/// The rewrite: aggregates to markers, keys to `_id` reads.
struct Rewrite<'k> {
    keys: &'k [SqlExpr],
    aggregates: Vec<SqlExpr>,
}

impl Rewrite<'_> {
    fn apply(&mut self, e: &SqlExpr) -> Result<Option<SqlExpr>, Refusal> {
        if matches!(e, SqlExpr::Reducer(_) | SqlExpr::OrderedListAgg { .. } | SqlExpr::JsonArrayAgg { .. }) {
            let at = match self.aggregates.iter().position(|a| a == e) {
                Some(at) => at,
                None => {
                    self.aggregates.push(e.clone());
                    self.aggregates.len() - 1
                }
            };
            return Ok(Some(build::derived(None, &format!("{AGGREGATE_MARKER}{at}"))));
        }
        if let Some(k) = self.keys.iter().position(|k| k == e) {
            return Ok(Some(build::derived(None, &format!("_id.k{k}"))));
        }
        carrier::map_children(e, &mut |c| self.apply(c))
    }

    fn applied(&mut self, e: &SqlExpr) -> Result<SqlExpr, Refusal> {
        Ok(self.apply(e)?.unwrap_or_else(|| e.clone()))
    }
}

/// Whether `e` reads a dynamic pivot's key (`KeyCell`): its value differs per copy of a per-key region.
fn reads_key(e: &SqlExpr) -> bool {
    matches!(e, SqlExpr::KeyCell { .. }) || children(e).iter().any(|c| reads_key(c))
}

/// The `$group` stage when the select groups or aggregates; the clauses rewritten over its outputs.
fn group(
    d: &MongoDb,
    w: &mut SqlWriter,
    group_by: &[SqlExpr],
    projections: Vec<Projection>,
    having0: Option<SqlExpr>,
    order_by: Vec<SortKey>,
    st: &mut Stages,
) -> Result<Grouping, Refusal> {
    let aggregates = projections.iter().any(|p| aggregates_select(&p.expr))
        || having0.as_ref().is_some_and(aggregates_select)
        || order_by.iter().any(|k| aggregates_select(&k.expr));
    if group_by.is_empty() && !aggregates {
        return Ok(Grouping { projections, having: having0, order_by });
    }
    let mut rewrite = Rewrite { keys: group_by, aggregates: Vec::new() };
    let mut ps = Vec::with_capacity(projections.len());
    for p in &projections {
        // the output keeps its name: a key projected bare reads the group's _id, under the column's own name
        let e = rewrite.applied(&p.expr)?;
        ps.push(build::projection(e, p.alias.clone().or_else(|| output_name(p)), p.out.clone())?);
    }
    let having = match &having0 {
        None => None,
        Some(h) => Some(rewrite.applied(h)?),
    };
    let mut keys = Vec::with_capacity(order_by.len());
    for k in &order_by {
        keys.push(build::sort_key(rewrite.applied(&k.expr)?, k.ascending, k.null_order, k.output_name.clone()));
    }
    st.begin(w).append("{\"$group\": {\"_id\": ");
    if group_by.is_empty() {
        w.append("null");
    } else {
        w.append("{");
        for (i, g) in group_by.iter().enumerate() {
            if i > 0 {
                w.append(", ");
            }
            w.append("\"k").append_long(i as i64).append("\": ").expr(d, g, 0)?;
        }
        w.append("}");
    }
    let mut reads: Vec<(String, Pc<'static>)> = Vec::new();
    // a dynamic pivot's per-key aggregates are written last, a per-key region the runner writes once per key
    let mut per_key: Vec<usize> = Vec::new();
    for (i, agg) in rewrite.aggregates.iter().enumerate() {
        if reads_key(agg) {
            per_key.push(i);
            continue;
        }
        let field = format!("{AGGREGATE_MARKER}{i}");
        w.append(", ").append(&json::string(&field)).append(": ");
        let gf = GroupField::new(&field, false)?;
        let read = aggregates::accumulator(d, w, agg, &gf)?;
        put(&mut reads, field, read);
    }
    if !per_key.is_empty() {
        let aggs = &rewrite.aggregates;
        let reads = &mut reads;
        w.region(", ", ", ", Some(""), |x| {
            for (n, i) in per_key.iter().enumerate() {
                let field = GroupField::new(&format!("{AGGREGATE_MARKER}{i}"), true)?;
                field.name(if n > 0 { x.append(", ") } else { x }, "")?.append(": ");
                let read = aggregates::accumulator(d, x, &aggs[*i], &field)?;
                put(reads, format!("{AGGREGATE_MARKER}{i}"), read);
            }
            Ok(())
        })?;
    }
    w.append("}}");
    if group_by.is_empty() {
        // SQL's one row over no input: the empty row is unioned in and the first of the two kept
        st.begin(w).append("{\"$unionWith\": {\"pipeline\": [{\"$documents\": [{\"_id\": null");
        for (i, agg) in rewrite.aggregates.iter().enumerate() {
            if !per_key.contains(&i) {
                w.append(", ").append(&json::string(&format!("{AGGREGATE_MARKER}{i}"))).append(": ");
                w.append(aggregates::empty_input(agg)?);
            }
        }
        if !per_key.is_empty() {
            let aggs = &rewrite.aggregates;
            w.region(", ", ", ", Some(""), |x| {
                for (n, i) in per_key.iter().enumerate() {
                    let field = GroupField::new(&format!("{AGGREGATE_MARKER}{i}"), true)?;
                    field.name(if n > 0 { x.append(", ") } else { x }, "")?.append(": ");
                    x.append(aggregates::empty_input(&aggs[*i])?);
                }
                Ok(())
            })?;
        }
        w.append(", \"__none\": 1}]}]}}");
        st.begin(w).append("{\"$sort\": {\"__none\": 1}}");
        st.begin(w).append("{\"$limit\": 1}");
        st.begin(w).append("{\"$unset\": \"__none\"}");
    }
    d.aggregate_reads(reads)?;
    Ok(Grouping { projections: ps, having, order_by: keys })
}

/// Java's `LinkedHashMap.put`: a new key last, a present one's value replaced in place.
fn put(reads: &mut Vec<(String, Pc<'static>)>, key: String, value: Pc<'static>) {
    match reads.iter_mut().find(|(k, _)| *k == key) {
        Some(slot) => slot.1 = value,
        None => reads.push((key, value)),
    }
}

// ==================================================================
// Windows
// ==================================================================

/// The window functions of a select's expressions, each replaced by its marker column.
#[derive(Default)]
struct Windows {
    found: Vec<SqlExpr>,
}

impl Windows {
    fn apply(&mut self, e: &SqlExpr) -> Result<Option<SqlExpr>, Refusal> {
        if matches!(e, SqlExpr::WindowCall { .. }) {
            let at = match self.found.iter().position(|f| f == e) {
                Some(at) => at,
                None => {
                    self.found.push(e.clone());
                    self.found.len() - 1
                }
            };
            return Ok(Some(build::derived(None, &format!("{WINDOW_MARKER}{at}"))));
        }
        carrier::map_children(e, &mut |c| self.apply(c))
    }

    fn applied(&mut self, e: &SqlExpr) -> Result<SqlExpr, Refusal> {
        Ok(self.apply(e)?.unwrap_or_else(|| e.clone()))
    }
}

/// What a window function asks of the stage that computes it (Java's `Need`, in its declaration order).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
enum Need {
    Any,
    Sorted,
    Single,
    ToCurrent,
    RangeKey,
}

/// A window call's parts.
struct Call<'a> {
    r#fn: &'a SqlAgg,
    partition_by: &'a [SqlExpr],
    order_by: &'a [SortKey],
    frame: Option<&'a Frame>,
}

fn parts(e: &SqlExpr) -> Call<'_> {
    match e {
        SqlExpr::WindowCall { r#fn, partition_by, order_by, frame, .. } => {
            Call { r#fn, partition_by, order_by, frame: frame.as_ref() }
        }
        _ => unreachable!("Windows finds window calls alone"),
    }
}

fn need(c: &Call<'_>) -> Need {
    match c.r#fn {
        SqlAgg::RankingFn { r#fn, .. } => {
            if *r#fn == lite_sql::AggFn::CumeDist {
                Need::ToCurrent
            } else {
                Need::Single
            }
        }
        SqlAgg::ValueFn { r#fn, .. } => {
            if matches!(r#fn, lite_sql::AggFn::Lag | lite_sql::AggFn::Lead) {
                Need::Sorted
            } else {
                framed(c)
            }
        }
        SqlAgg::Reducer(_) => framed(c),
    }
}

fn framed(c: &Call<'_>) -> Need {
    let Some(f) = c.frame else {
        return if c.order_by.is_empty() { Need::Any } else { Need::ToCurrent };
    };
    if f.kind == FrameKind::Rows {
        return Need::Sorted;
    }
    if offset(&f.from) || offset(&f.to) { Need::RangeKey } else { Need::ToCurrent }
}

fn offset(b: &Bound) -> bool {
    !matches!(b, Bound::UnboundedPreceding | Bound::UnboundedFollowing | Bound::CurrentRow)
}

fn nulls_first(k: &SortKey) -> bool {
    match k.null_order {
        None => !k.ascending,
        Some(o) => o == NullOrder::NullsFirst,
    }
}

/// The window functions of the select, after its group and before its sort and projection, each a field of a
/// `$setWindowFields` stage, grouped by their window specification; a QUALIFY is the match after the stages.
fn windows(
    d: &MongoDb,
    w: &mut SqlWriter,
    st: &mut Stages,
    g: Grouping,
    qualify: Option<&SqlExpr>,
) -> Result<Grouping, Refusal> {
    let mut rewrite = Windows::default();
    let mut ps = Vec::with_capacity(g.projections.len());
    for p in &g.projections {
        let e = rewrite.apply(&p.expr)?;
        ps.push(reprojected(p, e)?);
    }
    let qualified = match qualify {
        None => None,
        Some(q) => Some(rewrite.applied(q)?),
    };
    let mut keys = Vec::with_capacity(g.order_by.len());
    for k in &g.order_by {
        keys.push(build::sort_key(rewrite.applied(&k.expr)?, k.ascending, k.null_order, k.output_name.clone()));
    }
    if rewrite.found.is_empty() {
        return Ok(g);
    }
    let mut reads: Vec<(String, Pc<'static>)> = Vec::new();
    // the calls grouped by their window specification, in first-seen order
    let mut groups: Vec<Vec<usize>> = Vec::new();
    for i in 0..rewrite.found.len() {
        let c = parts(&rewrite.found[i]);
        let mut placed = false;
        for group in groups.iter_mut() {
            let first = parts(&rewrite.found[group[0]]);
            if first.partition_by == c.partition_by && first.order_by == c.order_by {
                group.push(i);
                placed = true;
                break;
            }
        }
        if !placed {
            groups.push(vec![i]);
        }
    }
    for (serial, group) in groups.iter().enumerate() {
        let spec = parts(&rewrite.found[group[0]]);
        let mut by_need: BTreeMap<Need, Vec<usize>> = BTreeMap::new();
        for &i in group {
            by_need.entry(need(&parts(&rewrite.found[i]))).or_default().push(i);
        }
        let ordered = !spec.order_by.is_empty();
        let one = by_need.contains_key(&Need::Single) || by_need.contains_key(&Need::ToCurrent);
        let sorted = ordered || by_need.len() > 1 || !by_need.contains_key(&Need::Any);
        let sort_field = format!("__ws{serial}");
        let rank_field = format!("__wd{serial}");
        let direction: i64 = if ordered && !spec.order_by[0].ascending { -1 } else { 1 };
        let mut key_fields: Vec<String> = Vec::new();
        let mut key_directions: Vec<i64> = Vec::new();
        if sorted {
            st.begin(w).append("{\"$addFields\": {");
            if !ordered {
                // a sort every row ties: the stage's operators ask for one
                w.append(&json::string(&sort_field)).append(": 0");
            } else if one {
                w.append(&json::string(&sort_field)).append(": {");
                for (i, k) in spec.order_by.iter().enumerate() {
                    if (if k.ascending { 1 } else { -1 }) != direction {
                        return Err(Refusal::capability(
                            "a window ranking or ranging over keys sorted both ways reached MongoDb, whose ranking \
                             operators sort by one field",
                        ));
                    }
                    // the flag that puts the nulls where the key asks, in the one field's direction
                    let null_flag: i64 = if nulls_first(k) == k.ascending { 0 } else { 1 };
                    w.append(if i > 0 { ", " } else { "" }).append("\"k").append_long(i as i64);
                    w.append("\": {\"n\": {\"$cond\": [{\"$eq\": [").expr(d, &k.expr, 0)?.append(", null]}, ");
                    w.append_long(null_flag).append(", ").append_long(1 - null_flag);
                    w.append("]}, \"v\": {\"$ifNull\": [").expr(d, &k.expr, 0)?.append(", null]}}");
                }
                w.append("}");
            } else {
                for (i, k) in spec.order_by.iter().enumerate() {
                    let field = format!("__wk{serial}_{i}");
                    w.append(if i > 0 { ", " } else { "" }).append(&json::string(&field)).append(": ");
                    w.expr(d, &k.expr, 0)?.append(", ").append(&json::string(&format!("{field}n")));
                    w.append(": {\"$eq\": [").expr(d, &k.expr, 0)?.append(", null]}");
                    key_fields.push(field);
                    key_directions.push(if k.ascending { 1 } else { -1 });
                }
            }
            w.append("}}");
        }
        // the sorted stage: the ranking operators, the lags and leads, the ROWS frames, the unordered aggregates, and
        // the dense rank the ranges to the current row sort by
        let mut first: Vec<usize> = Vec::new();
        for n in [Need::Any, Need::Sorted, Need::Single] {
            if let Some(is) = by_need.get(&n) {
                first.extend(is);
            }
        }
        let ranks_for_range = by_need.contains_key(&Need::ToCurrent);
        if !first.is_empty() || ranks_for_range {
            st.begin(w).append("{\"$setWindowFields\": {");
            partition_by(d, w, &spec)?;
            if sorted {
                w.append("\"sortBy\": {");
                if !ordered || one {
                    w.append(&json::string(&sort_field)).append(": ").append_long(direction);
                } else {
                    for (i, kf) in key_fields.iter().enumerate() {
                        let k = &spec.order_by[i];
                        w.append(if i > 0 { ", " } else { "" }).append(&json::string(&format!("{kf}n"))).append(": ");
                        w.append(if nulls_first(k) { "-1" } else { "1" }).append(", ").append(&json::string(kf));
                        w.append(": ").append_long(key_directions[i]);
                    }
                }
                w.append("}, ");
            }
            w.append("\"output\": {");
            let mut comma = false;
            if ranks_for_range {
                w.append(&json::string(&rank_field)).append(": {\"$denseRank\": {}}");
                comma = true;
            }
            for &i in &first {
                let c = parts(&rewrite.found[i]);
                w.append(if comma { ", " } else { "" });
                comma = true;
                let field = format!("{WINDOW_MARKER}{i}");
                let window = documents_window(&c);
                let read = aggregates::window_output(d, w, c.r#fn, &field, &window)?;
                put(&mut reads, field, read);
            }
            w.append("}}}");
        }
        // the stage sorted by the dense rank: RANGE windows between unbounded and the current row
        if ranks_for_range {
            st.begin(w).append("{\"$setWindowFields\": {");
            partition_by(d, w, &spec)?;
            w.append("\"sortBy\": {").append(&json::string(&rank_field)).append(": 1}, \"output\": {");
            let mut comma = false;
            for &i in by_need.get(&Need::ToCurrent).map_or(&[][..], Vec::as_slice) {
                let c = parts(&rewrite.found[i]);
                w.append(if comma { ", " } else { "" });
                comma = true;
                let field = format!("{WINDOW_MARKER}{i}");
                let window = to_current_window(&c);
                let read = aggregates::window_output(d, w, c.r#fn, &field, &window)?;
                put(&mut reads, field, read);
            }
            w.append("}}}");
        }
        // the stage sorted by the one key, ascending: RANGE windows by offsets
        if let Some(range_key) = by_need.get(&Need::RangeKey) {
            if spec.order_by.len() != 1 {
                return Err(Refusal::capability(format!(
                    "a range window by offsets over {} sort keys reached MongoDb, whose range windows order by one \
                     field",
                    spec.order_by.len()
                )));
            }
            let k = &spec.order_by[0];
            let key_field = format!("__wr{serial}");
            // a null key sorts to its end as the infinity of the key's kind
            let nulls_last = nulls_first(k) != k.ascending;
            let mut dates = false;
            for &i in range_key {
                let c = parts(&rewrite.found[i]);
                if let Some(f) = c.frame
                    && (unit_of(&f.from)?.is_some() || unit_of(&f.to)?.is_some())
                {
                    dates = true;
                    break;
                }
            }
            let infinity = if dates {
                format!("{{\"$date\": \"{}-01-01T00:00:00Z\"}}", if nulls_last { "9000" } else { "1000" })
            } else {
                format!("{{\"$numberDouble\": \"{}Infinity\"}}", if nulls_last { "" } else { "-" })
            };
            st.begin(w).append("{\"$addFields\": {").append(&json::string(&key_field)).append(": {\"$ifNull\": [");
            w.expr(d, &k.expr, 0)?.append(", ").append(&infinity).append("]}}}");
            st.begin(w).append("{\"$setWindowFields\": {");
            partition_by(d, w, &spec)?;
            w.append("\"sortBy\": {").append(&json::string(&key_field)).append(": 1}, \"output\": {");
            let mut comma = false;
            for &i in range_key {
                let c = parts(&rewrite.found[i]);
                let Some(f) = c.frame else {
                    return Err(Refusal::illegal_state(
                        "a range window by offsets without a frame (MongoDbPipeline.need)",
                    ));
                };
                w.append(if comma { ", " } else { "" });
                comma = true;
                let field = format!("{WINDOW_MARKER}{i}");
                let window = offset_window(f, k.ascending)?;
                let read = aggregates::window_output(d, w, c.r#fn, &field, &window)?;
                put(&mut reads, field, read);
            }
            w.append("}}}");
        }
    }
    d.window_reads(reads)?;
    if let Some(q) = &qualified {
        match_stage(d, w, st, q)?;
    }
    Ok(Grouping { projections: ps, having: g.having, order_by: keys })
}

/// `"partitionBy": ...,` -- the one key's expression or the keys' document -- or nothing.
fn partition_by(d: &MongoDb, w: &mut SqlWriter, spec: &Call<'_>) -> Result<(), Refusal> {
    if spec.partition_by.is_empty() {
        return Ok(());
    }
    w.append("\"partitionBy\": ");
    if spec.partition_by.len() == 1 {
        w.expr(d, &spec.partition_by[0], 0)?;
    } else {
        w.append("{");
        for (i, p) in spec.partition_by.iter().enumerate() {
            w.append(if i > 0 { ", " } else { "" }).append("\"p").append_long(i as i64).append("\": ").expr(d, p, 0)?;
        }
        w.append("}");
    }
    w.append(", ");
    Ok(())
}

/// A sorted-stage call's `, "window": {"documents": [...]}` for a ROWS frame; nothing otherwise.
fn documents_window(c: &Call<'_>) -> Pc<'static> {
    match c.frame {
        Some(f) if f.kind == FrameKind::Rows => {
            text([", \"window\": {\"documents\": [", &bound(&f.from, false), ", ", &bound(&f.to, false), "]}"].concat())
        }
        _ => text(""),
    }
}

/// A dense-rank-sorted call's `, "window": {"range": [...]}`: the frame's unbounded and current bounds, or SQL's
/// default over an order, unbounded preceding to the current row.
fn to_current_window(c: &Call<'_>) -> Pc<'static> {
    let (from, to) = match c.frame {
        None => ("\"unbounded\"".to_owned(), "0".to_owned()),
        Some(f) => (bound(&f.from, true), bound(&f.to, true)),
    };
    text([", \"window\": {\"range\": [", &from, ", ", &to, "]}"].concat())
}

/// A key-sorted call's `, "window": {"range": [...], "unit": ...}`: the frame's bounds as offsets of the key, which the
/// stage sorts ascending -- a descending order's bounds negated and swapped.
fn offset_window(f: &Frame, ascending: bool) -> Result<Pc<'static>, Refusal> {
    let mut from = bound(&f.from, true);
    let mut to = bound(&f.to, true);
    let from_unit = unit_of(&f.from)?;
    let to_unit = unit_of(&f.to)?;
    let mut unit = from_unit.or(to_unit);
    if let (Some(fu), Some(tu)) = (from_unit, to_unit)
        && fu != tu
    {
        let (Some(from_millis), Some(to_millis)) = (millis(fu), millis(tu)) else {
            return Err(Refusal::capability(format!(
                "a range window bounded in {fu}s and {tu}s reached MongoDb, whose range windows count in one unit"
            )));
        };
        unit = Some(if from_millis < to_millis { fu } else { tu });
        let finer = from_millis.min(to_millis);
        from = parse_long(&from)?.wrapping_mul(from_millis / finer).to_string();
        to = parse_long(&to)?.wrapping_mul(to_millis / finer).to_string();
    }
    if !ascending {
        let lo = negated(&to);
        to = negated(&from);
        from = lo;
    }
    let unit_text = match unit {
        None => String::new(),
        Some(u) => format!(", \"unit\": {}", json::string(u)),
    };
    Ok(text([", \"window\": {\"range\": [", &from, ", ", &to, "]", &unit_text, "}"].concat()))
}

/// Java's `Long.parseLong`, refused as it words a text that is no number.
fn parse_long(s: &str) -> Result<i64, Refusal> {
    s.parse::<i64>()
        .map_err(|_| Refusal { class: Refusal::NUMBER_FORMAT, message: format!("For input string: \"{s}\"") })
}

/// The milliseconds of each unit of fixed length.
fn millis(unit: &str) -> Option<i64> {
    Some(match unit {
        "week" => 604_800_000,
        "day" => 86_400_000,
        "hour" => 3_600_000,
        "minute" => 60_000,
        "second" => 1_000,
        "millisecond" => 1,
        _ => return None,
    })
}

fn negated(bound: &str) -> String {
    if bound.starts_with('"') || bound == "0" {
        return bound.to_owned();
    }
    match bound.strip_prefix('-') {
        Some(rest) => rest.to_owned(),
        None => format!("-{bound}"),
    }
}

/// A Java `Number`'s `toString`.
fn number_text(n: &lite_sql::JavaNumber) -> String {
    match n {
        lite_sql::JavaNumber::Long(v) => v.to_string(),
        lite_sql::JavaNumber::Integer(v) => v.to_string(),
        lite_sql::JavaNumber::Double(v) => v.text().to_owned(),
        lite_sql::JavaNumber::BigDecimal(v) => v.as_str().to_owned(),
    }
}

/// A frame bound as `$setWindowFields` spells it. The current row of a range window is the offset 0.
fn bound(b: &Bound, range: bool) -> String {
    match b {
        Bound::UnboundedPreceding | Bound::UnboundedFollowing => "\"unbounded\"".to_owned(),
        Bound::CurrentRow => (if range { "0" } else { "\"current\"" }).to_owned(),
        Bound::Preceding { n } => format!("-{}", number_text(n)),
        Bound::Following { n } => number_text(n),
        Bound::IntervalPreceding { n, .. } => format!("-{n}"),
        Bound::IntervalFollowing { n, .. } => n.to_string(),
    }
}

/// The unit of an interval bound, in `$setWindowFields`' names; none for a plain bound.
fn unit_of(b: &Bound) -> Result<Option<&'static str>, Refusal> {
    let unit = match b {
        Bound::IntervalPreceding { unit, .. } | Bound::IntervalFollowing { unit, .. } => unit,
        _ => return Ok(None),
    };
    Ok(Some(match unit.to_uppercase().as_str() {
        "YEARS" | "YEAR" => "year",
        "MONTHS" | "MONTH" => "month",
        "WEEKS" | "WEEK" => "week",
        "DAYS" | "DAY" => "day",
        "HOURS" | "HOUR" => "hour",
        "MINUTES" | "MINUTE" => "minute",
        "SECONDS" | "SECOND" => "second",
        "MILLISECONDS" | "MILLISECOND" => "millisecond",
        _ => {
            return Err(Refusal::capability(format!(
                "a range window by {unit} reached MongoDb, whose windows range by years to milliseconds"
            )));
        }
    }))
}

// ==================================================================
// Sorting and projecting
// ==================================================================

/// A sort key computed into a carried field, with its null flag beside it.
struct SortCarry {
    field: String,
    null_field: String,
    ascending: bool,
    nulls_first: bool,
}

/// Each sort key computed before the projection (`$addFields`), with its null flag.
fn sort_carries(d: &MongoDb, w: &mut SqlWriter, st: &mut Stages, g: &Grouping) -> Result<Vec<SortCarry>, Refusal> {
    if g.order_by.is_empty() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    st.begin(w).append("{\"$addFields\": {");
    for (i, k) in g.order_by.iter().enumerate() {
        let field = format!("__sort{i}");
        let null_field = format!("__null{i}");
        if i > 0 {
            w.append(", ");
        }
        w.append(&json::string(&field)).append(": ").expr(d, &k.expr, 0)?.append(", ");
        w.append(&json::string(&null_field)).append(": {\"$eq\": [").expr(d, &k.expr, 0)?.append(", null]}");
        out.push(SortCarry { field, null_field, ascending: k.ascending, nulls_first: nulls_first(k) });
    }
    w.append("}}");
    Ok(out)
}

/// The projection as one `$replaceWith`: the outputs by name, a star as the merged source documents, and the sort
/// carries beside them.
fn projection(
    d: &MongoDb,
    w: &mut SqlWriter,
    s: &SqlSelect,
    st: &mut Stages,
    g: &Grouping,
    sorts: &[SortCarry],
) -> Result<Option<Vec<String>>, Refusal> {
    st.begin(w).append("{\"$replaceWith\": ");
    let mut names: Vec<String> = Vec::new();
    let mut exprs: Vec<&SqlExpr> = Vec::new();
    let mut unnested: Vec<String> = Vec::new();
    let mut key_stems: Vec<String> = Vec::new();
    let mut key_exprs: Vec<&SqlExpr> = Vec::new();
    // declared last, so dropped first: the fields piece it ends with borrows the lists above
    let mut merged: Vec<Pc<'_>> = Vec::new();
    if g.projections.is_empty() {
        for alias in aliases(&s.from)? {
            merged.push(text(json::path(&[&alias])));
        }
    }
    for (i, p) in g.projections.iter().enumerate() {
        match &p.expr {
            SqlExpr::Star { table, .. } => match table {
                Some(t) => merged.push(text(json::path(&[t]))),
                None => {
                    for alias in aliases(&s.from)? {
                        merged.push(text(json::path(&[&alias])));
                    }
                }
            },
            SqlExpr::StarExcept { table, except, .. } => {
                // the starred row without the excluded fields
                let starred = match table {
                    Some(t) => vec![t.clone()],
                    None => aliases(&s.from)?,
                };
                for alias in starred {
                    merged.push(pc(move |_, x| {
                        let mut input = json::path(&[&alias]);
                        for name in except {
                            json::key(name)?;
                            input = [
                                "{\"$unsetField\": {\"field\": {\"$literal\": ",
                                &json::string(name),
                                "}, \"input\": ",
                                &input,
                                "}}",
                            ]
                            .concat();
                        }
                        x.append(&input);
                        Ok(())
                    }));
                }
            }
            _ if p.out.as_ref().is_some_and(|o| o.origin == Origin::PerKey) => {
                // a dynamic pivot's per-key column: written in the per-key run below
                let out = p.out.as_ref().expect("checked above");
                json::key(&out.name)?;
                key_stems.push(out.name.clone());
                key_exprs.push(&p.expr);
            }
            _ => {
                let name = field_name(p, i);
                names.push(name.clone());
                if let SqlExpr::Call { r#fn: SqlFn::Unnest, args, .. } = &p.expr {
                    // the list is projected whole and exploded after ($unwind): a collection result's rows
                    unnested.push(name);
                    exprs.push(arg(args, 0)?);
                } else {
                    exprs.push(&p.expr);
                }
            }
        }
    }
    let merged_before = !merged.is_empty();
    let fields: Pc<'_> = {
        let names = &names;
        let exprs = &exprs;
        let key_stems = &key_stems;
        let key_exprs = &key_exprs;
        pc(move |d, x| {
            x.append("{");
            for (i, name) in names.iter().enumerate() {
                if i > 0 {
                    x.append(", ");
                }
                x.append(&json::key(name)?).append(": ").expr(d, exprs[i], 0)?;
            }
            for (i, c) in sorts.iter().enumerate() {
                if !names.is_empty() || i > 0 {
                    x.append(", ");
                }
                x.append(&json::string(&c.field)).append(": ").append(&json::path(&[&c.field])).append(", ");
                x.append(&json::string(&c.null_field)).append(": ").append(&json::path(&[&c.null_field]));
            }
            if !key_stems.is_empty() {
                // the per-key run: written once per key by the runner, each field named by the key's position
                let before = !names.is_empty() || !sorts.is_empty();
                let empty = if before || merged_before { Some("") } else { None };
                x.region(if before { ", " } else { "" }, ", ", empty, |r| {
                    for (i, stem) in key_stems.iter().enumerate() {
                        r.append(if i > 0 { ", " } else { "" }).append("\"_pk").counter()?;
                        r.append(&format!("_{stem}\": ")).expr(d, key_exprs[i], 0)?;
                    }
                    Ok(())
                })?;
            }
            x.append("}");
            Ok(())
        })
    };
    if merged.is_empty() {
        fields(d, w)?;
    } else {
        merged.push(fields);
        json::op_of(d, w, "$mergeObjects", &merged)?;
    }
    w.append("}");
    unwind(w, st, &unnested)?;
    Ok(if merged.is_empty() && key_stems.is_empty() { Some(names.clone()) } else { None })
}

/// The exploded lists of a projection: one list unwound; several zipped by position to the longest, null-padded,
/// unwound, and read back into their columns.
fn unwind(w: &mut SqlWriter, st: &mut Stages, unnested: &[String]) -> Result<(), Refusal> {
    if unnested.is_empty() {
        return Ok(());
    }
    if unnested.len() == 1 {
        st.begin(w).append("{\"$unwind\": ").append(&json::path(&[&unnested[0]])).append("}");
        return Ok(());
    }
    st.begin(w).append("{\"$addFields\": {\"__zip\": {\"$zip\": {\"inputs\": [");
    for (i, u) in unnested.iter().enumerate() {
        w.append(if i > 0 { ", " } else { "" }).append("{\"$ifNull\": [").append(&json::path(&[u])).append(", []]}");
    }
    w.append("], \"useLongestLength\": true}}}}");
    st.begin(w).append("{\"$unwind\": \"$__zip\"}");
    st.begin(w).append("{\"$addFields\": {");
    for (i, u) in unnested.iter().enumerate() {
        w.append(if i > 0 { ", " } else { "" }).append(&json::key(u)?).append(": {\"$arrayElemAt\": [\"$__zip\", ");
        w.append_long(i as i64).append("]}");
    }
    w.append("}}");
    st.begin(w).append("{\"$unset\": \"__zip\"}");
    Ok(())
}

/// A source written flat (the right side of a lookup), as `lookup` writes one: a unit test's door to the flat-join wall
/// no query reaches (`join` refuses a joined right side first).
#[cfg(test)]
pub(super) fn flat_source(d: &MongoDb, w: &mut SqlWriter, src: &SqlSource) -> Result<(), Refusal> {
    source(d, w, src, &mut Stages::default(), Shape::Flat)
}
