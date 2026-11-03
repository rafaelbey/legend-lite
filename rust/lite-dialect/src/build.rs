// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! The MIR's constructors as Java writes them (`SqlExpr.Call.of(fn, args...)`, `new SqlExpr.Case(...)`, `new
//! SqlSelect(...)`, ...): a node built here carries the type fact Java's record constructor computes
//! ([`crate::typing`], Java's `SqlTyping`), a select its outputs rebuilt from its projections, a union its labels
//! reconciled, a join its delivered columns. The passes build and rebuild through these, so a rewritten tree is the
//! tree Java's would be; the renderers' own built trees go through them too.
//!
//! A leaf the builder supplies (a `Column`'s fact, a `Lambda`'s, a `PlanParam`'s) is the caller's; a literal's is the
//! constant Java stamps.

use crate::refusal::Refusal;
use crate::typing::{self, scalar};
use lite_sql::{
    AggFn, BigDecimal, Column, Cte, Double, Frame, Hoist, JoinKind, JsonArrayAggKey, Lambda, NullOrder, Origin,
    OutputCol, Projection, Quantifier, Reducer, Scalar, SortKey, SqlAgg, SqlExpr, SqlFn, SqlQuery, SqlSelect,
    SqlSource, SqlType, SqlUnion, SqlWith, StructLitField, TypeFact, When,
};

// ---- literals

/// `new SqlExpr.StringLit(value)`.
pub fn string_lit(value: &str) -> SqlExpr {
    SqlExpr::StringLit { value: value.to_owned(), r#type: scalar(Scalar::Varchar) }
}

/// `new SqlExpr.IntLit(value)`.
pub fn int_lit(value: i64) -> SqlExpr {
    SqlExpr::IntLit { value, r#type: scalar(Scalar::Bigint) }
}

/// `new SqlExpr.BoolLit(value)`.
pub fn bool_lit(value: bool) -> SqlExpr {
    SqlExpr::BoolLit { value, r#type: scalar(Scalar::Boolean) }
}

/// `new SqlExpr.FloatLit(value)`: the double as MirJson carries one, its `Double.toString` text, typed by the decimal
/// it is written as (`decimalLitType(BigDecimal.valueOf(value))`).
pub fn float_lit(value: f64) -> SqlExpr {
    let text = lite_javacompat::double_to_string(value);
    let r#type = typing::float_lit_type(&text);
    let value = Double::parse(&text).expect("Java's text of a double");
    SqlExpr::FloatLit { value, r#type }
}

/// `new SqlExpr.DecimalLit(value)`.
pub fn decimal_lit(value: BigDecimal) -> SqlExpr {
    let r#type = typing::decimal_lit_type(&value);
    SqlExpr::DecimalLit { value, r#type }
}

/// `new SqlExpr.NullLit()`: the NULL value, bottom.
pub fn null_lit() -> SqlExpr {
    SqlExpr::NullLit { r#type: TypeFact::Bottom }
}

/// `new SqlExpr.RowOrder(table)`.
pub fn row_order(table: Option<String>) -> SqlExpr {
    SqlExpr::RowOrder { table, r#type: scalar(Scalar::Bigint) }
}

/// `new SqlExpr.Star(table)`.
pub fn star(table: Option<&str>) -> SqlExpr {
    SqlExpr::Star { table: table.map(str::to_owned), r#type: TypeFact::Unknown }
}

// ---- references

/// `SqlExpr.Column.derived(table, name)`: an untyped reference to a name the query invented.
pub fn derived(table: Option<&str>, name: &str) -> SqlExpr {
    SqlExpr::Column(Column {
        table: table.map(str::to_owned),
        name: name.to_owned(),
        r#type: TypeFact::Unknown,
        origin: Some(Origin::Derived),
    })
}

/// `new SqlExpr.Column(table, name)`: an untyped reference, no origin.
pub fn plain_column(table: Option<&str>, name: &str) -> Column {
    Column { table: table.map(str::to_owned), name: name.to_owned(), r#type: TypeFact::Unknown, origin: None }
}

/// `SqlExpr.Column.of(table, OutputCol)`: the stamped reference to a source's declared output.
pub fn column_of(table: Option<&str>, oc: &OutputCol) -> Column {
    Column {
        table: table.map(str::to_owned),
        name: oc.name.clone(),
        r#type: TypeFact::Typed { r#type: oc.r#type.clone(), nullable: oc.nullable },
        origin: Some(oc.origin),
    }
}

/// `SqlExpr.Column.of(table, name, type, nullable, origin)`: the builder's own synthetic column.
pub fn typed_column(table: Option<&str>, name: &str, t: SqlType, nullable: bool, origin: Origin) -> SqlExpr {
    SqlExpr::Column(Column {
        table: table.map(str::to_owned),
        name: name.to_owned(),
        r#type: TypeFact::Typed { r#type: t, nullable },
        origin: Some(origin),
    })
}

/// `SqlExpr.Column.of(table, outs, name)`: stamped when `outs` claims the name, plain otherwise.
pub fn column_in(table: Option<&str>, outs: &[OutputCol], name: &str) -> Column {
    match outs.iter().find(|c| c.name == name) {
        Some(oc) => column_of(table, oc),
        None => plain_column(table, name),
    }
}

/// `new SqlExpr.StoredRead(column, stored)`: the column's own fact.
pub fn stored_read(column: Column, stored: lite_sql::ColumnType) -> SqlExpr {
    let r#type = column.r#type.clone();
    SqlExpr::StoredRead { column, stored, r#type }
}

// ---- compositions

/// `SqlExpr.Call.of(fn, args...)` / `new SqlExpr.Call(fn, args)`.
pub fn call(f: SqlFn, args: Vec<SqlExpr>) -> SqlExpr {
    let r#type = typing::call_type(f, &args);
    SqlExpr::Call { r#fn: f, args, r#type }
}

/// `new SqlExpr.Case(whens, otherwise)`.
pub fn case(whens: Vec<When>, otherwise: Option<SqlExpr>) -> SqlExpr {
    let r#type = typing::case_type(&whens, otherwise.as_ref());
    SqlExpr::Case { whens, otherwise: otherwise.map(Box::new), r#type }
}

/// `new SqlExpr.Case.When(condition, then)`.
pub fn when(condition: SqlExpr, then: SqlExpr) -> When {
    When { condition, then }
}

/// `new SqlExpr.Cast(value, target)`.
pub fn cast(value: SqlExpr, target: SqlType) -> SqlExpr {
    cast_conform(value, target, false)
}

/// `new SqlExpr.Cast(value, target, conform)`.
pub fn cast_conform(value: SqlExpr, target: SqlType, conform: bool) -> SqlExpr {
    let r#type = typing::cast_type(&value, &target);
    SqlExpr::Cast { value: Box::new(value), target, conform, r#type }
}

/// `new SqlExpr.Group(inner)`: parentheses transport the inner fact.
pub fn group(inner: SqlExpr) -> SqlExpr {
    let r#type = inner.r#type().clone();
    SqlExpr::Group { inner: Box::new(inner), r#type }
}

/// `new SqlExpr.ArrayLit(elements)`.
pub fn array_lit(elements: Vec<SqlExpr>) -> SqlExpr {
    array_lit_given(elements, TypeFact::Unknown)
}

/// `new SqlExpr.ArrayLit(elements, type)`: the type the elements prove, else the one given.
pub fn array_lit_given(elements: Vec<SqlExpr>, given: TypeFact) -> SqlExpr {
    let proven = typing::array_lit_type(&elements);
    let r#type = if matches!(proven, TypeFact::Unknown) { given } else { proven };
    SqlExpr::ArrayLit { elements, r#type }
}

/// `new SqlExpr.StructLit(fields)`.
pub fn struct_lit(fields: Vec<StructLitField>) -> SqlExpr {
    let r#type = typing::struct_lit_type(&fields);
    SqlExpr::StructLit { fields, r#type }
}

/// `new SqlExpr.StructGet(source, field)`.
pub fn struct_get(source: SqlExpr, field: &str) -> SqlExpr {
    struct_get_given(source, field.to_owned(), TypeFact::Unknown)
}

/// `new SqlExpr.StructGet(source, field, type)`: the type the source proves, else the one given.
pub fn struct_get_given(source: SqlExpr, field: String, given: TypeFact) -> SqlExpr {
    let proven = typing::struct_get_type(&source, &field);
    let r#type = if matches!(proven, TypeFact::Unknown) { given } else { proven };
    SqlExpr::StructGet { source: Box::new(source), field, r#type }
}

/// `new SqlExpr.Membership(needle, collection)`.
pub fn membership(needle: SqlExpr, collection: SqlExpr) -> SqlExpr {
    let r#type = typing::membership_type(&needle, &collection);
    SqlExpr::Membership { needle: Box::new(needle), collection: Box::new(collection), r#type }
}

/// `new SqlExpr.ReduceCollection(reducer, collection, extras)`.
pub fn reduce_collection(reducer: AggFn, collection: SqlExpr, extras: Vec<SqlExpr>) -> SqlExpr {
    let r#type = typing::reduce_collection_type(reducer, &collection);
    SqlExpr::ReduceCollection { reducer, collection: Box::new(collection), extras, r#type }
}

/// `new SqlExpr.OrderedListAgg(value, orderBy)`: the LIST reducer's fact.
pub fn ordered_list_agg(value: SqlExpr, order_by: SqlExpr) -> SqlExpr {
    let r#type = typing::reducer_type(AggFn::List, value.r#type());
    SqlExpr::OrderedListAgg { value: Box::new(value), order_by: Box::new(order_by), r#type }
}

/// `new SqlAgg.Reducer(fn, args, distinct, orderBy)`.
pub fn reducer(f: AggFn, args: Vec<SqlExpr>, distinct: bool, order_by: Vec<SortKey>) -> Reducer {
    let r#type = typing::reducer_type(f, args.first().map_or(&TypeFact::Unknown, SqlExpr::r#type));
    Reducer { r#fn: f, args, distinct, order_by, r#type }
}

/// `new SqlExpr.Exists(subquery)`.
pub fn exists(subquery: SqlQuery) -> SqlExpr {
    SqlExpr::Exists { subquery: Box::new(subquery), r#type: scalar(Scalar::Boolean) }
}

/// `new SqlExpr.InSubquery(value, subquery)`.
pub fn in_subquery(value: SqlExpr, subquery: SqlQuery) -> SqlExpr {
    SqlExpr::InSubquery { value: Box::new(value), subquery: Box::new(subquery), r#type: scalar(Scalar::Boolean) }
}

/// `new SqlExpr.Quantified(value, comparison, quantifier, subquery)`.
pub fn quantified(value: SqlExpr, comparison: SqlFn, quantifier: Quantifier, subquery: SqlQuery) -> SqlExpr {
    SqlExpr::Quantified {
        value: Box::new(value),
        comparison,
        quantifier,
        subquery: Box::new(subquery),
        r#type: scalar(Scalar::Boolean),
    }
}

/// `new SqlExpr.ScalarSubquery(subquery)`.
pub fn scalar_subquery(subquery: SqlQuery) -> SqlExpr {
    let r#type = typing::scalar_subquery_type(&subquery);
    SqlExpr::ScalarSubquery { subquery: Box::new(subquery), r#type }
}

/// `new SqlExpr.CheckedDefects(own, hoists)`: no rule types it.
pub fn checked_defects(own: SqlExpr, hoists: Vec<Hoist>) -> SqlExpr {
    SqlExpr::CheckedDefects { own: Box::new(own), hoists, r#type: TypeFact::Unknown }
}

/// `new SqlExpr.CheckedChildValue(envelope, toMany)`: no rule types it.
pub fn checked_child_value(envelope: SqlExpr, to_many: bool) -> SqlExpr {
    SqlExpr::CheckedChildValue { envelope: Box::new(envelope), to_many, r#type: TypeFact::Unknown }
}

/// `new SqlExpr.CheckedOne(list, scalarCarrier, atLeastOnly)`: an at-least-one guard keeps the list's fact, the
/// exactly-one narrowing yields the element's.
pub fn checked_one(list: SqlExpr, scalar_carrier: bool, at_least_only: bool) -> SqlExpr {
    let r#type = if at_least_only { list.r#type().clone() } else { typing::checked_one_type(&list) };
    SqlExpr::CheckedOne { list: Box::new(list), scalar_carrier, at_least_only, r#type }
}

/// `new SqlExpr.CompactList(list)`: compaction keeps the carrier's fact.
pub fn compact_list(list: SqlExpr) -> SqlExpr {
    let r#type = list.r#type().clone();
    SqlExpr::CompactList { list: Box::new(list), r#type }
}

/// `new SqlExpr.JsonObject(kv)`.
pub fn json_object(kv: Vec<SqlExpr>) -> SqlExpr {
    SqlExpr::JsonObject { kv, r#type: scalar(Scalar::Json) }
}

/// `new SqlExpr.JsonArray(elements)`.
pub fn json_array(elements: Vec<SqlExpr>) -> SqlExpr {
    SqlExpr::JsonArray { elements, r#type: scalar(Scalar::Json) }
}

/// `new SqlExpr.JsonArrayAgg(value, orderKeys)`.
pub fn json_array_agg(value: SqlExpr, order_keys: Vec<JsonArrayAggKey>) -> SqlExpr {
    SqlExpr::JsonArrayAgg { value: Box::new(value), order_keys, r#type: scalar(Scalar::Json) }
}

/// `new SqlExpr.WindowCall(fn, partitionBy, orderBy, frame)`.
pub fn window_call(f: SqlAgg, partition_by: Vec<SqlExpr>, order_by: Vec<SortKey>, frame: Option<Frame>) -> SqlExpr {
    let r#type = typing::window_type(&f);
    SqlExpr::WindowCall { r#fn: Box::new(f), partition_by, order_by, frame, r#type }
}

/// `new SqlExpr.Lambda(params, body)`: a lambda's fact is a leaf's, UNKNOWN by default.
pub fn lambda(params: &[&str], body: SqlExpr) -> SqlExpr {
    SqlExpr::Lambda(lambda_of(params.iter().map(|p| (*p).to_owned()).collect(), body))
}

/// `new SqlExpr.Lambda(params, body)`, as the record.
pub fn lambda_of(params: Vec<String>, body: SqlExpr) -> Lambda {
    Lambda { params, body: Box::new(body), r#type: TypeFact::Unknown }
}

/// `new SqlExpr.FoldCall(source, lambda, init, accIsList, homogeneous)`.
pub fn fold_call(source: SqlExpr, lambda: Lambda, init: SqlExpr, acc_is_list: bool, homogeneous: bool) -> SqlExpr {
    let r#type = typing::fold_type(&source, &lambda, &init, acc_is_list);
    SqlExpr::FoldCall { source: Box::new(source), lambda, init: Box::new(init), acc_is_list, homogeneous, r#type }
}

/// `new SqlSelect.SortKey(expr, ascending, nullOrder, outputName)`.
pub fn sort_key(expr: SqlExpr, ascending: bool, null_order: Option<NullOrder>, output_name: Option<String>) -> SortKey {
    SortKey { expr, ascending, null_order, output_name }
}

// ---- queries and sources

/// `new SqlSelect.Projection(expr, alias, out)`: an attached output is the query's own declaration, its origin
/// normalized to DERIVED (a per-key name keeps its mark); a star carries none.
pub fn projection(expr: SqlExpr, alias: Option<String>, out: Option<OutputCol>) -> Result<Projection, Refusal> {
    let star = matches!(expr, SqlExpr::Star { .. } | SqlExpr::StarExcept { .. });
    if star && out.is_some() {
        return Err(Refusal::illegal_argument(
            "a star projection carries the source's whole output list — attaching a single OutputCol is a caller bug",
        ));
    }
    let out = out.map(|o| match o.origin {
        Origin::Derived | Origin::PerKey => o,
        _ => OutputCol { origin: Origin::Derived, ..o },
    });
    Ok(Projection { expr, alias, out })
}

/// `new OutputCol(name, type, nullable)`: a derived column.
pub fn output_col(name: &str, t: SqlType, nullable: bool) -> OutputCol {
    OutputCol { name: name.to_owned(), r#type: t, nullable, origin: Origin::Derived }
}

/// `new SqlSelect(...)`'s canonical constructor over `s`: a projection frame's outputs rebuilt from its projections,
/// a star-framed join under a WHERE its pads neutralized; a star frame keeps the outputs it was given.
pub fn select(mut s: SqlSelect) -> Result<SqlSelect, Refusal> {
    if !s.projections.is_empty() {
        s.outputs = typing::outputs_from(&s)?;
    } else if let (SqlSource::Join { .. }, Some(w)) = (&s.from, &s.r#where)
        && !s.outputs.is_empty()
    {
        let outputs = std::mem::take(&mut s.outputs);
        s.outputs = typing::where_pad_neutralized(&s.from, w, outputs);
    }
    Ok(s)
}

/// A select of `projections` over `from` with every other clause empty (Java's eleven-argument constructor so).
pub fn select_of(
    projections: Vec<Projection>,
    from: SqlSource,
    outputs: Vec<lite_sql::OutputCol>,
) -> Result<SqlSelect, Refusal> {
    select(SqlSelect {
        projections,
        distinct: false,
        from,
        r#where: None,
        group_by: Vec::new(),
        having: None,
        qualify: None,
        order_by: Vec::new(),
        limit: None,
        offset: None,
        outputs,
    })
}

/// `SqlSelect.starOf(from)`: `SELECT * FROM from`, every other clause empty.
pub fn star_of(from: SqlSource) -> Result<SqlSelect, Refusal> {
    let outputs = typing::source_outputs(&from).to_vec();
    select_of(Vec::new(), from, outputs)
}

/// `s.withProjections(p)`: refused for an empty list (a star frame's outputs are the caller's).
pub fn with_projections(s: SqlSelect, p: Vec<Projection>) -> Result<SqlSelect, Refusal> {
    if p.is_empty() {
        return Err(Refusal::illegal_argument(
            "withProjections(empty): a star frame's outputs are caller knowledge — use withOutputs",
        ));
    }
    select(SqlSelect { projections: p, ..s })
}

/// `s.withFrom(f)`.
pub fn with_from(s: SqlSelect, from: SqlSource) -> Result<SqlSelect, Refusal> {
    select(SqlSelect { from, ..s })
}

/// `s.withWhere(w)`.
pub fn with_where(s: SqlSelect, r#where: Option<SqlExpr>) -> Result<SqlSelect, Refusal> {
    select(SqlSelect { r#where, ..s })
}

/// `s.withGroupBy(keys)`.
pub fn with_group_by(s: SqlSelect, group_by: Vec<SqlExpr>) -> Result<SqlSelect, Refusal> {
    select(SqlSelect { group_by, ..s })
}

/// `s.withOrderBy(keys)`.
pub fn with_order_by(s: SqlSelect, order_by: Vec<SortKey>) -> Result<SqlSelect, Refusal> {
    select(SqlSelect { order_by, ..s })
}

/// `s.withLimit(n)`.
pub fn with_limit(s: SqlSelect, limit: Option<i64>) -> Result<SqlSelect, Refusal> {
    select(SqlSelect { limit, ..s })
}

/// `s.withOffset(n)`.
pub fn with_offset(s: SqlSelect, offset: Option<i64>) -> Result<SqlSelect, Refusal> {
    select(SqlSelect { offset, ..s })
}

/// `s.withDistinct()`.
pub fn with_distinct(s: SqlSelect) -> Result<SqlSelect, Refusal> {
    select(SqlSelect { distinct: true, ..s })
}

/// `s.withOutputs(out)`: a star frame's outputs door, refused on a projection frame.
pub fn with_outputs(s: SqlSelect, outputs: Vec<OutputCol>) -> Result<SqlSelect, Refusal> {
    if !s.projections.is_empty() {
        return Err(Refusal::illegal_state(
            "withOutputs on a projection frame: its outputs derive from the projections",
        ));
    }
    select(SqlSelect { outputs, ..s })
}

/// `new SqlUnion(branches, all, outputs)`: at least two branches, the labels reconciled against them.
pub fn union(branches: Vec<SqlQuery>, all: bool, outputs: Vec<OutputCol>) -> Result<SqlUnion, Refusal> {
    if branches.len() < 2 {
        return Err(Refusal::illegal_argument("a union needs at least two branches"));
    }
    let outputs = typing::reconcile_union_labels(&branches, outputs);
    Ok(SqlUnion { branches, all, outputs })
}

/// `new SqlWith(ctes, body, recursive)`: refused without expressions.
pub fn with(ctes: Vec<Cte>, body: SqlQuery, recursive: bool) -> Result<SqlWith, Refusal> {
    if ctes.is_empty() {
        return Err(Refusal::illegal_argument("a WITH without expressions is its body"));
    }
    Ok(SqlWith { ctes, body: Box::new(body), recursive })
}

/// `SqlWith.prepend(ctes, q, recursive)`: `ctes` ahead of `q`'s own (a WITH merges; any other query is the body).
pub fn prepend(ctes: Vec<Cte>, q: SqlQuery, recursive: bool) -> Result<SqlQuery, Refusal> {
    if ctes.is_empty() {
        return Ok(q);
    }
    if let SqlQuery::With(w) = q {
        let mut all = ctes;
        all.extend(w.ctes);
        return Ok(SqlQuery::With(with(all, *w.body, recursive || w.recursive)?));
    }
    Ok(SqlQuery::With(with(ctes, q, recursive)?))
}

/// `new SqlSource.Join(left, right, kind, on)`: the ON clause kind-coupled (the CROSS family takes none, every other
/// kind requires one), the delivered columns computed.
pub fn join(left: SqlSource, right: SqlSource, kind: JoinKind, on: Option<SqlExpr>) -> Result<SqlSource, Refusal> {
    let onless = matches!(kind, JoinKind::Cross | JoinKind::CrossLateral);
    if onless && on.is_some() {
        return Err(Refusal::illegal_argument(format!("{kind} takes no ON clause")));
    }
    if !onless && on.is_none() {
        return Err(Refusal::illegal_argument(format!("{kind} requires an ON condition")));
    }
    let outputs = typing::join_delivered(&left, &right, kind);
    Ok(SqlSource::Join { left: Box::new(left), right: Box::new(right), kind, on: on.map(Box::new), outputs })
}

/// `new SqlSource.Subselect(inner, alias, frameName)`.
pub fn subselect(inner: SqlQuery, alias: &str, frame_name: Option<String>) -> SqlSource {
    SqlSource::Subselect { inner: Box::new(inner), alias: alias.to_owned(), frame_name }
}

/// A select as a query.
pub fn query(s: SqlSelect) -> SqlQuery {
    SqlQuery::Select(Box::new(s))
}
