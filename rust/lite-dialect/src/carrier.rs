// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! Java's `CarrierStrategies` (`core/src/main/java/com/legend/sql/dialect/CarrierStrategies.java`): the strategy pass
//! that rewrites the semantic collection nodes into a dialect's emission, over the dialect's collection capabilities
//! ([`Caps`]). It runs first of every dialect's passes. On a dialect with native lists (DuckDB's [`Caps`], all four
//! capabilities) every hook is the identity: the renderer spells the semantic nodes itself. Without them:
//!
//! - a select: the FULL OUTER emulation (no native FULL OUTER JOIN: a LEFT branch UNION ALL a RIGHT branch
//!   anti-joined on a copy of the left source) and the explode placements (`SELECT unnest(arg)` as rows, by the
//!   argument's witnessed shape);
//! - a source: the ASOF emulation (a LEFT JOIN pinned to the correlated MIN/MAX of the right key) and a static PIVOT
//!   as GROUP BY with one filtered aggregate per value and USING;
//! - an expression: the compile-time folds over literal collections, the fusion of a reduction into its collecting
//!   subselect, the list reads and membership in the portable forms.
//!
//! Every node is built through [`crate::build`], so it carries Java's type fact; a hook answers [`Rw::Same`] exactly
//! where Java's returns its input. Java's helpers that rebuild through `children()`/`withChildren` (`remapAlias`,
//! `substParam`, `literalNullTests`) keep its identity rule: a node none of whose children changed is the node itself
//! (here `None`), else it is rebuilt through `withChildren`'s constructors ([`with_children`]).

use crate::build::{self, call, cast, int_lit, string_lit, when};
use crate::json::{self, Json};
use crate::refusal::{Refusal, arg};
use crate::rewriter::{Rw, SqlRewriter};
use crate::typing::{children, source_alias};
use lite_sql::{
    AggFn, Column, JoinKind, JsonArrayAggKey, Lambda, NullOrder, Origin, OutputCol, PivotUsing, Projection, Reducer,
    Scalar, SortKey, SqlAgg, SqlExpr, SqlFn, SqlQuery, SqlSelect, SqlSource, SqlType, StructLitField, TypeFact, When,
};

/// Java's `CarrierStrategies.Caps`: a dialect's collection capabilities.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Caps {
    pub native_lists: bool,
    pub correlated_explode: bool,
    pub json_carrier: bool,
    pub native_full_outer: bool,
}

/// The carrier strategy pass (Java's `CarrierStrategies`).
#[derive(Debug)]
pub struct CarrierStrategies {
    caps: Caps,
    /// Java's `foldLiteralReductions`: a reduction over a compile-time literal collection folds to its scalar chain.
    fold_literal_reductions: bool,
}

impl CarrierStrategies {
    /// Java's `new CarrierStrategies(caps)`.
    pub fn new(caps: Caps) -> CarrierStrategies {
        CarrierStrategies::with_literal_folds(caps, true)
    }

    /// Java's `new CarrierStrategies(caps, foldLiteralReductions)`.
    pub fn with_literal_folds(caps: Caps, fold_literal_reductions: bool) -> CarrierStrategies {
        CarrierStrategies { caps, fold_literal_reductions }
    }
}

fn index_error(i: usize, len: usize) -> Refusal {
    Refusal { class: Refusal::INDEX_OUT_OF_BOUNDS, message: format!("Index {i} out of bounds for length {len}") }
}

fn name_at(names: &[String], i: usize) -> Result<&str, Refusal> {
    names.get(i).map(String::as_str).ok_or_else(|| index_error(i, names.len()))
}

// ==================================================================
// slots
// ==================================================================

/// `CarrierStrategies.generatedSlot`: a pivot's generated column's slot -- the pivot's own output of that name, else
/// the USING's typed result, nullable.
pub fn generated_slot(pivot_outputs: &[OutputCol], u: &PivotUsing, name: &str) -> Result<OutputCol, Refusal> {
    if let Some(own) = slot(pivot_outputs, name) {
        return Ok(own.clone());
    }
    match &u.r#type {
        None => Err(Refusal::illegal_state(format!(
            "the pivot's USING '{}' carries no typed slot, so its generated column '{name}' has none",
            u.alias
        ))),
        Some(t) => Ok(build::output_col(name, t.clone(), true)),
    }
}

fn slot<'a>(outs: &'a [OutputCol], name: &str) -> Option<&'a OutputCol> {
    outs.iter().find(|c| c.name == name)
}

/// The single slot of a one-column frame, or none.
fn slot0(outs: &[OutputCol]) -> Option<OutputCol> {
    outs.first().cloned()
}

// ==================================================================
// Java's `children()` / `withChildren` identity rule
// ==================================================================

/// `f` over each direct child, the node rebuilt through `withChildren` when one changed (`None`: the node itself).
pub(crate) fn map_children(
    e: &SqlExpr,
    f: &mut dyn FnMut(&SqlExpr) -> Result<Option<SqlExpr>, Refusal>,
) -> Result<Option<SqlExpr>, Refusal> {
    let kids = children(e);
    if kids.is_empty() {
        return Ok(None);
    }
    let mut mapped = Vec::with_capacity(kids.len());
    let mut changed = false;
    for k in kids {
        match f(&k)? {
            Some(m) => {
                changed = true;
                mapped.push(m);
            }
            None => mapped.push(k.into_owned()),
        }
    }
    if changed { Ok(Some(with_children(e, mapped)?)) } else { Ok(None) }
}

/// Java's `SqlExpr.withChildren(cs)`: the node with its direct children replaced, through the constructors that
/// method calls (an array literal and a struct read keep their given fact, a lambda its own).
pub fn with_children(e: &SqlExpr, cs: Vec<SqlExpr>) -> Result<SqlExpr, Refusal> {
    let mut it = cs.into_iter();
    let mut next = || it.next().expect("as many children as children() names");
    Ok(match e {
        SqlExpr::StoredRead { stored, .. } => match next() {
            SqlExpr::Column(c) => build::stored_read(c, stored.clone()),
            other => {
                return Err(Refusal::illegal_state(format!(
                    "a stored read reads a table column, not {}",
                    crate::java::expr_text(&other)
                )));
            }
        },
        SqlExpr::ReduceCollection { reducer, extras, .. } => {
            let collection = next();
            let rest = (0..extras.len()).map(|_| next()).collect();
            build::reduce_collection(*reducer, collection, rest)
        }
        SqlExpr::Membership { .. } => {
            let needle = next();
            build::membership(needle, next())
        }
        SqlExpr::InSubquery { subquery, .. } => build::in_subquery(next(), subquery.as_ref().clone()),
        SqlExpr::CheckedDefects { hoists, .. } => {
            let own = next();
            let hs = hoists.iter().map(|h| lite_sql::Hoist { envelope: next(), ..h.clone() }).collect();
            build::checked_defects(own, hs)
        }
        SqlExpr::CheckedChildValue { to_many, .. } => build::checked_child_value(next(), *to_many),
        SqlExpr::Quantified { comparison, quantifier, subquery, .. } => {
            build::quantified(next(), *comparison, *quantifier, subquery.as_ref().clone())
        }
        SqlExpr::CheckedOne { scalar_carrier, at_least_only, .. } => {
            build::checked_one(next(), *scalar_carrier, *at_least_only)
        }
        SqlExpr::CompactList { .. } => build::compact_list(next()),
        SqlExpr::Group { .. } => build::group(next()),
        SqlExpr::ArrayLit { elements, r#type } => {
            build::array_lit_given((0..elements.len()).map(|_| next()).collect(), r#type.clone())
        }
        SqlExpr::OrderedListAgg { .. } => {
            let value = next();
            build::ordered_list_agg(value, next())
        }
        SqlExpr::StructLit { fields, .. } => {
            build::struct_lit(fields.iter().map(|f| StructLitField { value: next(), ..f.clone() }).collect())
        }
        SqlExpr::StructGet { field, r#type, .. } => build::struct_get_given(next(), field.clone(), r#type.clone()),
        SqlExpr::Call { r#fn, args, .. } => build::call(*r#fn, (0..args.len()).map(|_| next()).collect()),
        SqlExpr::Case { whens, otherwise, .. } => {
            let ws = whens
                .iter()
                .map(|_| {
                    let condition = next();
                    When { condition, then: next() }
                })
                .collect();
            let ot = otherwise.as_ref().map(|_| next());
            build::case(ws, ot)
        }
        SqlExpr::WindowCall { r#fn, partition_by, order_by, frame, .. } => {
            let f = match r#fn.as_ref() {
                SqlAgg::Reducer(_) => match next() {
                    SqlExpr::Reducer(r) => SqlAgg::Reducer(r),
                    other => {
                        return Err(Refusal {
                            class: Refusal::CLASS_CAST,
                            message: format!(
                                "class com.legend.sql.SqlExpr${} cannot be cast to class com.legend.sql.SqlAgg",
                                crate::java::simple_name(&other)
                            ),
                        });
                    }
                },
                other => other.clone(),
            };
            let pb = (0..partition_by.len()).map(|_| next()).collect();
            let ks = order_by.iter().map(|k| SortKey { expr: next(), ..k.clone() }).collect();
            build::window_call(f, pb, ks, frame.clone())
        }
        SqlExpr::Lambda(l) => {
            SqlExpr::Lambda(Lambda { params: l.params.clone(), body: Box::new(next()), r#type: l.r#type.clone() })
        }
        SqlExpr::Cast { target, conform, .. } => build::cast_conform(next(), target.clone(), *conform),
        SqlExpr::FoldCall { acc_is_list, homogeneous, .. } => {
            let source = next();
            let lambda = match next() {
                SqlExpr::Lambda(l) => l,
                other => return Err(Refusal::class_cast(crate::java::simple_name(&other), "Lambda")),
            };
            build::fold_call(source, lambda, next(), *acc_is_list, *homogeneous)
        }
        SqlExpr::JsonObject { kv, .. } => build::json_object((0..kv.len()).map(|_| next()).collect()),
        SqlExpr::JsonArray { elements, .. } => build::json_array((0..elements.len()).map(|_| next()).collect()),
        SqlExpr::JsonArrayAgg { order_keys, .. } => {
            let value = next();
            let ks = order_keys.iter().map(|k| JsonArrayAggKey { expr: next(), desc: k.desc }).collect();
            build::json_array_agg(value, ks)
        }
        SqlExpr::Reducer(r) => {
            let args = (0..r.args.len()).map(|_| next()).collect();
            let ks = r.order_by.iter().map(|k| SortKey { expr: next(), ..k.clone() }).collect();
            SqlExpr::Reducer(build::reducer(r.r#fn, args, r.distinct, ks))
        }
        // the leaves, and the nodes whose only children are queries: the node itself
        other => other.clone(),
    })
}

/// `SqlExpr.StructGet.of(source, field)`: a literal struct's field is that field's value.
fn struct_get_of(source: SqlExpr, field: &str) -> SqlExpr {
    if let SqlExpr::StructLit { fields, .. } = &source
        && let Some(f) = fields.iter().find(|f| f.name == field)
    {
        return f.value.clone();
    }
    build::struct_get(source, field)
}

/// `CarrierStrategies.substParam`: the lambda parameter's reads replaced with `value` -- a bare read is the value, a
/// read qualified by the parameter that field of it. `None`: the body itself.
fn subst_param(body: &SqlExpr, param: &str, value: &SqlExpr) -> Result<Option<SqlExpr>, Refusal> {
    if let SqlExpr::Column(c) = body {
        if c.table.is_none() && c.name == param {
            return Ok(Some(value.clone()));
        }
        if c.table.as_deref() == Some(param) {
            return Ok(Some(struct_get_of(value.clone(), &c.name)));
        }
    }
    map_children(body, &mut |k| subst_param(k, param, value))
}

fn subst(body: &SqlExpr, param: &str, value: &SqlExpr) -> Result<SqlExpr, Refusal> {
    Ok(subst_param(body, param, value)?.unwrap_or_else(|| body.clone()))
}

/// `CarrierStrategies.remapAlias`: every column qualified by `from` qualified by `to`, its stamped fact kept.
pub fn remap_alias(e: &SqlExpr, from: &str, to: &str) -> Result<SqlExpr, Refusal> {
    Ok(remapped(e, from, to)?.unwrap_or_else(|| e.clone()))
}

fn remapped(e: &SqlExpr, from: &str, to: &str) -> Result<Option<SqlExpr>, Refusal> {
    if let SqlExpr::Column(c) = e
        && c.table.as_deref() == Some(from)
    {
        return Ok(Some(SqlExpr::Column(Column { table: Some(to.to_owned()), ..c.clone() })));
    }
    map_children(e, &mut |k| remapped(k, from, to))
}

/// `CarrierStrategies.literalNullTests`: a struct literal's null tests as their answers. `None`: the node itself.
fn literal_null_tests(e: &SqlExpr) -> Result<Option<SqlExpr>, Refusal> {
    if let SqlExpr::Call { r#fn: f @ (SqlFn::IsNull | SqlFn::IsNotNull), args, .. } = e
        && let [SqlExpr::StructLit { .. }] = args.as_slice()
    {
        return Ok(Some(build::bool_lit(*f == SqlFn::IsNotNull)));
    }
    map_children(e, &mut literal_null_tests)
}

fn null_tests_answered(e: SqlExpr) -> Result<SqlExpr, Refusal> {
    Ok(literal_null_tests(&e)?.unwrap_or(e))
}

// ==================================================================
// the select hook: FULL OUTER emulation, explode placements
// ==================================================================

fn full_outer(
    s: &SqlSelect,
    left: &SqlSource,
    right: &SqlSource,
    on: &SqlExpr,
    left_copy: SqlSource,
) -> Result<SqlQuery, Refusal> {
    // the select's ORDER BY / LIMIT / OFFSET belong to the union, not to its branches
    let bare = build::with_offset(build::with_limit(build::with_order_by(s.clone(), Vec::new())?, None)?, None)?;
    let left_branch =
        build::with_from(bare.clone(), build::join(left.clone(), right.clone(), JoinKind::Left, Some(on.clone()))?)?;
    let probe = build::with_projections(build::star_of(left_copy)?, vec![build::projection(int_lit(1), None, None)?])?;
    let probe = build::with_where(probe, Some(remap_alias(on, source_alias(left)?, "_full")?))?;
    let anti = call(SqlFn::Not, vec![build::exists(build::query(probe))]);
    let right_branch =
        build::with_from(bare, build::join(left.clone(), right.clone(), JoinKind::Right, Some(on.clone()))?)?;
    let w = match &s.r#where {
        None => anti,
        Some(w) => call(SqlFn::And, vec![w.clone(), anti]),
    };
    let right_branch = build::with_where(right_branch, Some(w))?;
    let union = build::union(vec![build::query(left_branch), build::query(right_branch)], true, s.outputs.clone())?;
    if s.order_by.is_empty() && s.limit.is_none() && s.offset.is_none() {
        return Ok(SqlQuery::Union(union));
    }
    let outer = build::star_of(build::subselect(SqlQuery::Union(union), "_fullu", None))?;
    let keys = s.order_by.iter().map(|k| output_keyed_sort(k, s, "_fullu")).collect::<Result<Vec<_>, _>>()?;
    let outer = build::with_offset(build::with_limit(build::with_order_by(outer, keys)?, s.limit)?, s.offset)?;
    Ok(build::query(outer))
}

/// `CarrierStrategies.outputKeyedSort`: a sort key over the emulated union's outer select, addressing the union's
/// output (by the key's output name, else the projection whose expression the key is).
fn output_keyed_sort(k: &SortKey, s: &SqlSelect, outer_alias: &str) -> Result<SortKey, Refusal> {
    let mut out: Option<&OutputCol> = None;
    for i in 0..s.projections.len() {
        let o = s.outputs.get(i).ok_or_else(|| index_error(i, s.outputs.len()))?;
        if k.output_name.as_ref() == Some(&o.name) || s.projections[i].expr == k.expr {
            out = Some(o);
            break;
        }
    }
    if out.is_none()
        && s.projections.is_empty()
        && let SqlExpr::Column(kc) = &k.expr
    {
        // a star select: the key names one of its expanded columns, uniquely
        let named: Vec<&OutputCol> = s.outputs.iter().filter(|o| o.name == kc.name).collect();
        if let [one] = named.as_slice() {
            out = Some(one);
        }
    }
    let Some(out) = out else {
        let names: Vec<&str> = s.outputs.iter().map(|o| o.name.as_str()).collect();
        return Err(Refusal::capability(format!(
            "FULL OUTER JOIN emulation: sort key {} is not one of the select's outputs [{}]",
            record_text(&k.expr),
            names.join(", ")
        )));
    };
    // the union's output is a derived frame column of the outer select
    Ok(build::sort_key(
        build::typed_column(Some(outer_alias), &out.name, out.r#type.clone(), out.nullable, Origin::Derived),
        k.ascending,
        k.null_order,
        Some(out.name.clone()),
    ))
}

/// The ordered-dedup idiom: its list, and the filter that keeps x at index i iff x's key first occurs there.
struct Dedup {
    list: SqlExpr,
    keep: Lambda,
}

/// `CarrierStrategies.dedupList`: the bare `LIST_FILTER(list, (x, i) -> ...)` or its subquery-carrying form.
fn dedup_list(e: &SqlExpr) -> Option<Dedup> {
    if let SqlExpr::Call { r#fn: SqlFn::ListFilter, args, .. } = e
        && let [list, SqlExpr::Lambda(l)] = args.as_slice()
        && l.params.len() == 2
    {
        return Some(Dedup { list: list.clone(), keep: l.clone() });
    }
    if let SqlExpr::ScalarSubquery { subquery, .. } = e
        && let SqlQuery::Select(sel) = subquery.as_ref()
        && sel.projections.len() == 1
        && let SqlSource::Subselect { inner, alias: carry_alias, .. } = &sel.from
        && let SqlQuery::Select(cs) = inner.as_ref()
        && cs.projections.len() == 1
        && matches!(cs.from, SqlSource::Dual)
        && let SqlExpr::Call { r#fn: SqlFn::ListFilter, args, .. } = &sel.projections[0].expr
        && let [SqlExpr::Column(lc), SqlExpr::Lambda(l2)] = args.as_slice()
        && lc.table.as_deref() == Some(carry_alias.as_str())
        && l2.params.len() == 2
    {
        return Some(Dedup { list: cs.projections[0].expr.clone(), keep: l2.clone() });
    }
    None
}

/// `CarrierStrategies.dedupKey`: the key the idiom compares `value` by, or none when the filter is not
/// `LIST_POSITION(keys, key(x)) = i`.
fn dedup_key(keep: &Lambda, value: &SqlExpr) -> Result<Option<SqlExpr>, Refusal> {
    if let SqlExpr::Call { r#fn: SqlFn::Equal, args, .. } = keep.body.as_ref()
        && let [SqlExpr::Call { r#fn: SqlFn::ListPosition, args: pos, .. }, SqlExpr::Column(i)] = args.as_slice()
        && pos.len() == 2
        && i.table.is_none()
        && i.name == name_at(&keep.params, 1)?
    {
        return Ok(Some(subst(&pos[1], name_at(&keep.params, 0)?, value)?));
    }
    Ok(None)
}

fn select_cast_error(q: &SqlQuery) -> Refusal {
    let got = match q {
        SqlQuery::Select(_) => "SqlSelect",
        SqlQuery::Union(_) => "SqlUnion",
        SqlQuery::With(_) => "SqlWith",
    };
    Refusal {
        class: Refusal::CLASS_CAST,
        message: format!("class com.legend.sql.{got} cannot be cast to class com.legend.sql.SqlSelect"),
    }
}

/// `CarrierStrategies.filterBranches`: each branch of an exploded query gains the predicate as a WHERE over its own
/// projected value.
fn filter_branches(q: SqlQuery, pred: &Lambda) -> Result<SqlQuery, Refusal> {
    match q {
        SqlQuery::Union(u) => {
            let bs = u.branches.into_iter().map(|b| filter_branches(b, pred)).collect::<Result<Vec<_>, _>>()?;
            Ok(SqlQuery::Union(build::union(bs, u.all, u.outputs)?))
        }
        SqlQuery::Select(sel) => {
            let value = sel.projections.first().ok_or_else(|| index_error(0, 0))?.expr.clone();
            let cond = subst(&pred.body, name_at(&pred.params, 0)?, &value)?;
            let w = match &sel.r#where {
                None => cond,
                Some(w) => call(SqlFn::And, vec![w.clone(), cond]),
            };
            Ok(build::query(build::with_where(*sel, Some(w))?))
        }
        other => Err(select_cast_error(&other)),
    }
}

fn bigint() -> SqlType {
    SqlType::Scalar(Scalar::Bigint)
}

fn col(table: &str, oc: &OutputCol) -> SqlExpr {
    SqlExpr::Column(build::column_of(Some(table), oc))
}

fn key_slot(key: &SqlExpr, value: &OutputCol) -> OutputCol {
    let t = match key.r#type() {
        TypeFact::Typed { r#type, .. } => r#type.clone(),
        _ => value.r#type.clone(),
    };
    build::output_col("__dd_key", t, true)
}

fn row_number(partition: Vec<SqlExpr>, order: Vec<SortKey>) -> SqlExpr {
    build::window_call(SqlAgg::RankingFn { r#fn: AggFn::RowNumber, args: Vec::new() }, partition, order, None)
}

fn plain_select(
    projections: Vec<Projection>,
    from: SqlSource,
    r#where: Option<SqlExpr>,
    order_by: Vec<SortKey>,
    outputs: Vec<OutputCol>,
) -> Result<SqlSelect, Refusal> {
    build::select(SqlSelect {
        projections,
        distinct: false,
        from,
        r#where,
        group_by: Vec::new(),
        having: None,
        qualify: None,
        order_by,
        limit: None,
        offset: None,
        outputs,
    })
}

fn named(e: SqlExpr, oc: &OutputCol) -> Result<Projection, Refusal> {
    build::projection(e, Some(oc.name.clone()), Some(oc.clone()))
}

/// `CarrierStrategies.distinctOf`: the ordered dedup over an exploded query, or none for an unwitnessed shape.
fn distinct_of(q: SqlQuery, outer: &SqlSelect, keep: &Lambda) -> Result<Option<SqlQuery>, Refusal> {
    if let SqlQuery::Union(u) = &q
        && u.all
        && outer.outputs.len() == 1
        && u.branches.iter().all(|b| matches!(b, SqlQuery::Select(bs) if bs.projections.len() == 1))
    {
        let value = &outer.outputs[0];
        let index = build::output_col("__dd_index", bigint(), false);
        let first = build::output_col("__dd_first", bigint(), false);
        let mut key_col: Option<OutputCol> = None;
        let mut indexed = Vec::with_capacity(u.branches.len());
        for (i, b) in u.branches.iter().enumerate() {
            let SqlQuery::Select(b) = b else { unreachable!("every branch is a select") };
            let v = &b.projections[0].expr;
            let Some(key) = dedup_key(keep, v)? else {
                return Ok(None);
            };
            let kc = key_col.get_or_insert_with(|| key_slot(&key, value)).clone();
            indexed.push(build::query(build::with_projections(
                b.as_ref().clone(),
                vec![named(v.clone(), value)?, named(key, &kc)?, named(int_lit(i as i64), &index)?],
            )?));
        }
        let Some(key_col) = key_col else {
            return Ok(None);
        };
        let src = build::subselect(
            SqlQuery::Union(build::union(indexed, true, vec![value.clone(), key_col.clone(), index.clone()])?),
            "dedup_src",
            None,
        );
        let rn = row_number(
            vec![col("dedup_src", &key_col)],
            vec![build::sort_key(col("dedup_src", &index), true, None, None)],
        );
        let ranked = plain_select(
            vec![named(col("dedup_src", value), value)?, named(col("dedup_src", &index), &index)?, named(rn, &first)?],
            src,
            None,
            Vec::new(),
            vec![value.clone(), index.clone(), first.clone()],
        )?;
        let firsts = build::subselect(build::query(ranked), "dedup_first", None);
        return Ok(Some(build::query(plain_select(
            vec![build::projection(col("dedup_first", value), None, Some(value.clone()))?],
            firsts,
            Some(call(SqlFn::Equal, vec![col("dedup_first", &first), int_lit(1)])),
            vec![build::sort_key(col("dedup_first", &index), true, None, None)],
            outer.outputs.clone(),
        )?)));
    }
    // a single select's rows have no order of their own: by the value itself it is DISTINCT; by a key, one row per key
    let me = build::derived(None, name_at(&keep.params, 0)?);
    if dedup_key(keep, &me)?.as_ref() != Some(&me) {
        return keyed_distinct(q, outer, keep);
    }
    if let SqlQuery::Select(sel) = q {
        return Ok(Some(build::query(build::select(SqlSelect { distinct: true, order_by: Vec::new(), ..*sel })?)));
    }
    let star = build::projection(build::star(None), None, None)?;
    let mut s =
        plain_select(vec![star], build::subselect(q, "dedup_src", None), None, Vec::new(), outer.outputs.clone())?;
    s.distinct = true;
    Ok(Some(build::query(build::select(s)?)))
}

/// `CarrierStrategies.keyedDistinct`: one row per key of a single select's value, or none for an unwitnessed shape.
fn keyed_distinct(q: SqlQuery, outer: &SqlSelect, keep: &Lambda) -> Result<Option<SqlQuery>, Refusal> {
    let SqlQuery::Select(sel) = q else {
        return Ok(None);
    };
    if sel.projections.len() != 1 || outer.outputs.len() != 1 {
        return Ok(None);
    }
    let value = &outer.outputs[0];
    let v = sel.projections[0].expr.clone();
    let Some(key) = dedup_key(keep, &v)? else {
        return Ok(None);
    };
    let key_col = key_slot(&key, value);
    let first = build::output_col("__dd_first", bigint(), false);
    let keyed = build::with_projections(*sel, vec![named(v, value)?, named(key, &key_col)?])?;
    let src = build::subselect(build::query(keyed), "dedup_src", None);
    let rn = row_number(vec![col("dedup_src", &key_col)], Vec::new());
    let ranked = plain_select(
        vec![named(col("dedup_src", value), value)?, named(rn, &first)?],
        src,
        None,
        Vec::new(),
        vec![value.clone(), first.clone()],
    )?;
    Ok(Some(build::query(plain_select(
        vec![build::projection(col("dedup_first", value), None, Some(value.clone()))?],
        build::subselect(build::query(ranked), "dedup_first", None),
        Some(call(SqlFn::Equal, vec![col("dedup_first", &first), int_lit(1)])),
        Vec::new(),
        outer.outputs.clone(),
    )?)))
}

/// `CarrierStrategies.collectSelect`: the collect select beneath a scalar subquery -- one projection, a bare
/// non-distinct one-argument LIST reducer, no other clauses.
fn collect_select(e: &SqlExpr) -> Option<&SqlSelect> {
    if let SqlExpr::ScalarSubquery { subquery, .. } = e
        && let SqlQuery::Select(sel) = subquery.as_ref()
        && sel.projections.len() == 1
        && let SqlExpr::Reducer(r) = &sel.projections[0].expr
        && r.r#fn == AggFn::List
        && !r.distinct
        && r.args.len() == 1
        && sel.group_by.is_empty()
        && sel.having.is_none()
        && sel.qualify.is_none()
        && sel.order_by.is_empty()
        && sel.limit.is_none()
        && sel.offset.is_none()
        && !sel.distinct
    {
        return Some(sel);
    }
    None
}

/// The collect select's LIST reducer.
fn collect_of(sel: &SqlSelect) -> &Reducer {
    match &sel.projections[0].expr {
        SqlExpr::Reducer(r) => r,
        _ => unreachable!("a collect select projects its LIST reducer"),
    }
}

/// `sel` projecting `e` in its one projection's place (its alias and slot kept).
fn reprojected(sel: &SqlSelect, e: SqlExpr) -> Result<SqlSelect, Refusal> {
    let p0 = &sel.projections[0];
    build::with_projections(sel.clone(), vec![build::projection(e, p0.alias.clone(), p0.out.clone())?])
}

/// `CarrierStrategies.explode`: the portable form of `SELECT unnest(arg) AS alias` for the witnessed argument
/// shapes, or none (the renderer's wall stays).
fn explode(arg: &SqlExpr, s: &SqlSelect, alias: Option<&str>) -> Result<Option<SqlQuery>, Refusal> {
    let dual = matches!(s.from, SqlSource::Dual);
    let mut arg = arg.clone();
    // carrier compaction is a no-op over rows
    while let SqlExpr::CompactList { list, .. } = arg {
        arg = *list;
    }
    // an array-cast wrapper over a folded literal unwraps
    if let SqlExpr::Cast { value, target: SqlType::Array { .. }, .. } = &arg
        && let Some(jl) = json_literal(value)
        && let Json::List(ll) = json::parse(jl)?
    {
        arg = build::array_lit(ll.iter().map(json_lit_expr).collect());
    }
    let alias_s = alias.map(str::to_owned);
    let one = |e: SqlExpr| -> Result<Vec<Projection>, Refusal> {
        Ok(vec![build::projection(e, alias_s.clone(), slot0(&s.outputs))?])
    };
    // unnest(NULL) yields zero rows: the select's shape kept, killed with WHERE FALSE
    if matches!(arg, SqlExpr::NullLit { .. }) {
        let p = build::with_projections(s.clone(), one(build::null_lit())?)?;
        return Ok(Some(build::query(build::with_where(p, Some(build::bool_lit(false)))?)));
    }
    // a literal collection: a UNION ALL of one-row selects
    if dual
        && let SqlExpr::ArrayLit { elements, .. } = &arg
        && !elements.is_empty()
    {
        let mut branches = Vec::with_capacity(elements.len());
        for el in elements {
            branches.push(build::query(build::with_projections(s.clone(), one(el.clone())?)?));
        }
        return Ok(Some(union_or_one(branches, &s.outputs)?));
    }
    // explode of a collect: the collecting row set, the collect's order keys carried over
    if dual && let Some(coll) = collect_select(&arg) {
        let collect = collect_of(coll);
        let p = build::with_projections(coll.clone(), one(collect.args[0].clone())?)?;
        return Ok(Some(build::query(build::with_order_by(p, collect.order_by.clone())?)));
    }
    // a sorted explode: ASC NULLS LAST, the collect keys secondary
    if dual
        && let SqlExpr::Call { r#fn: SqlFn::ListSort, args, .. } = &arg
        && args.len() == 1
        && let Some(coll) = collect_select(&args[0])
    {
        let collect = collect_of(coll);
        let raw = collect.args[0].clone();
        let mut keys = vec![build::sort_key(raw.clone(), true, Some(NullOrder::NullsLast), None)];
        keys.extend(collect.order_by.iter().cloned());
        let p = build::with_projections(coll.clone(), one(raw)?)?;
        return Ok(Some(build::query(build::with_order_by(p, keys)?)));
    }
    // a filtered explode: the inner list exploded, the predicate each branch's WHERE
    if dual
        && let SqlExpr::Call { r#fn: SqlFn::ListFilter, args, .. } = &arg
        && let [list, SqlExpr::Lambda(flam)] = args.as_slice()
        && flam.params.len() == 1
        && let Some(inner) = explode(list, s, alias)?
    {
        return Ok(Some(filter_branches(inner, flam)?));
    }
    // the ordered-dedup idiom over rows
    if dual
        && let Some(d) = dedup_list(&arg)
        && let Some(inner) = explode(&d.list, s, alias)?
    {
        return distinct_of(inner, s, &d.keep);
    }
    // a concatenation: the branches of each arm in turn
    if dual
        && let SqlExpr::Call { r#fn: SqlFn::ListConcat, args, .. } = &arg
        && args.len() >= 2
    {
        let mut branches = Vec::new();
        for arm in args {
            match explode(arm, s, alias)? {
                None => return Ok(None),
                Some(SqlQuery::Union(u)) => branches.extend(u.branches),
                Some(b) => branches.push(b),
            }
        }
        return Ok(Some(SqlQuery::Union(build::union(branches, true, s.outputs.clone())?)));
    }
    // through-subselect cells: one branch of the inner select per cell
    if let SqlExpr::Column(c) = &arg
        && let SqlSource::Subselect { inner, .. } = &s.from
        && let SqlQuery::Select(inner) = inner.as_ref()
        && !inner.distinct
        && inner.group_by.is_empty()
        && inner.having.is_none()
        && inner.qualify.is_none()
        && inner.order_by.is_empty()
        && inner.limit.is_none()
        && inner.offset.is_none()
    {
        let src = inner.projections.iter().rfind(|ip| ip.alias.as_deref() == Some(c.name.as_str()));
        if let Some(Projection { expr: SqlExpr::ArrayLit { elements: cells, .. }, .. }) = src
            && !cells.is_empty()
        {
            let mut branches = Vec::with_capacity(cells.len());
            for cell in cells {
                branches.push(build::query(build::with_projections(inner.as_ref().clone(), one(cell.clone())?)?));
            }
            return Ok(Some(union_or_one(branches, &s.outputs)?));
        }
    }
    Ok(None)
}

/// One branch as itself (a union needs two), else their UNION ALL.
fn union_or_one(mut branches: Vec<SqlQuery>, outputs: &[OutputCol]) -> Result<SqlQuery, Refusal> {
    if branches.len() == 1 {
        return Ok(branches.pop().expect("one branch"));
    }
    Ok(SqlQuery::Union(build::union(branches, true, outputs.to_vec())?))
}

impl SqlRewriter for CarrierStrategies {
    fn name(&self) -> &'static str {
        "CarrierStrategies"
    }

    fn select(&mut self, s: SqlSelect) -> Result<Rw<SqlQuery>, Refusal> {
        if self.caps.native_lists {
            return Ok(Rw::Same(SqlQuery::Select(Box::new(s))));
        }
        // FULL OUTER emulation: LEFT branch UNION ALL RIGHT branch anti-joined on a copy of the LEFT source
        if !self.caps.native_full_outer
            && let SqlSource::Join { left, right, kind: JoinKind::Full, on: Some(on), .. } = &s.from
            && let Some(left_copy) = copy_with_alias(left, "_full")
        {
            return Ok(Rw::New(full_outer(&s, left, right, on, left_copy)?));
        }
        // explode placements: a single-projection SELECT unnest(arg) with no other clauses
        if let [p] = s.projections.as_slice()
            && s.r#where.is_none()
            && s.group_by.is_empty()
            && s.having.is_none()
            && s.qualify.is_none()
            && s.order_by.is_empty()
            && s.limit.is_none()
            && s.offset.is_none()
            && !s.distinct
            && let SqlExpr::Call { r#fn: SqlFn::Unnest, args, .. } = &p.expr
            && let [a] = args.as_slice()
            && let Some(ex) = explode(a, &s, p.alias.as_deref())?
        {
            return Ok(Rw::New(ex));
        }
        Ok(Rw::Same(SqlQuery::Select(Box::new(s))))
    }

    fn source(&mut self, s: SqlSource) -> Result<Rw<SqlSource>, Refusal> {
        if self.caps.native_lists {
            return Ok(Rw::Same(s));
        }
        if let SqlSource::Join { kind: JoinKind::AsofLeft, on: Some(_), .. } = &s
            && let Some(em) = as_of_emulation(&s)?
        {
            return Ok(Rw::New(em));
        }
        match pivot_as_group_by(&s)? {
            Some(sub) => Ok(Rw::New(sub)),
            None => Ok(Rw::Same(s)),
        }
    }

    fn expr(&mut self, e: SqlExpr) -> Result<Rw<SqlExpr>, Refusal> {
        if self.caps.native_lists {
            return Ok(Rw::Same(e));
        }
        match self.rewritten(&e)? {
            Some(n) => Ok(Rw::New(n)),
            None => Ok(Rw::Same(e)),
        }
    }
}

// ==================================================================
// the source hook: static PIVOT emulation, ASOF emulation
// ==================================================================

/// A static pivot as GROUP BY over the non-pivoted columns with one filtered aggregate per value and USING; none
/// for a dynamic pivot, more than one ON key, or a COUNT(*)-style USING.
fn pivot_as_group_by(s: &SqlSource) -> Result<Option<SqlSource>, Refusal> {
    let SqlSource::Pivot { source, on, r#in, named, usings, alias, outputs } = s else {
        return Ok(None);
    };
    let values = if r#in.is_empty() { named } else { r#in };
    if values.is_empty() || on.len() != 1 {
        return Ok(None);
    }
    let mut ps = Vec::new();
    let mut group = Vec::new();
    for oc in outputs {
        if !oc.name.contains("__|__") {
            let g = SqlExpr::Column(build::column_of(Some(source_alias(source)?), oc));
            group.push(g.clone());
            ps.push(named_projection(g, oc)?);
        }
    }
    for v in values {
        for u in usings {
            let agg = &u.agg;
            let mut args = agg.args.clone();
            if args.is_empty() {
                // a COUNT(*)-style USING: unwitnessed
                return Ok(None);
            }
            let first = std::mem::replace(&mut args[0], build::null_lit());
            args[0] = build::case(vec![when(call(SqlFn::Equal, vec![on[0].clone(), v.clone()]), first)], None);
            let pname = format!("{}__|__{}", lit_text(v), u.alias);
            let slot = generated_slot(outputs, u, &pname)?;
            ps.push(build::projection(
                SqlExpr::Reducer(build::reducer(agg.r#fn, args, agg.distinct, agg.order_by.clone())),
                Some(pname),
                Some(slot),
            )?);
        }
    }
    let sel = build::with_group_by(build::with_projections(build::star_of(source.as_ref().clone())?, ps)?, group)?;
    Ok(Some(build::subselect(build::query(sel), alias, None)))
}

fn named_projection(e: SqlExpr, oc: &OutputCol) -> Result<Projection, Refusal> {
    build::projection(e, Some(oc.name.clone()), Some(oc.clone()))
}

/// `CarrierStrategies.asOfEmulation`: a plain LEFT JOIN whose ON pins the right key to the correlated MAX (bounded
/// above) or MIN (bounded below) over a copy of the right source; none for an unrecognized inequality or right side.
fn as_of_emulation(j: &SqlSource) -> Result<Option<SqlSource>, Refusal> {
    let SqlSource::Join { left, right, on: Some(on), .. } = j else {
        return Ok(None);
    };
    let right_alias = source_alias(right)?;
    let mut conjuncts = Vec::new();
    flatten_and(on, &mut conjuncts);
    let Some((right_key, pick_max)) = as_of_key(j)? else {
        return Ok(None);
    };
    let Some(copy) = copy_with_alias(right, "_asof") else {
        return Ok(None);
    };
    let mut r#where: Option<SqlExpr> = None;
    for c in conjuncts {
        let c = remap_alias(c, right_alias, "_asof")?;
        r#where = Some(match r#where {
            None => c,
            Some(w) => call(SqlFn::And, vec![w, c]),
        });
    }
    let pick_fn = if pick_max { AggFn::Max } else { AggFn::Min };
    let picked = SqlExpr::Reducer(build::reducer(
        pick_fn,
        vec![remap_alias(&right_key, right_alias, "_asof")?],
        false,
        Vec::new(),
    ));
    let pick = build::with_where(
        build::with_projections(build::star_of(copy)?, vec![build::projection(picked, None, None)?])?,
        r#where,
    )?;
    let on2 = call(
        SqlFn::And,
        vec![on.as_ref().clone(), call(SqlFn::Equal, vec![right_key, build::scalar_subquery(build::query(pick))])],
    );
    Ok(Some(build::join(left.as_ref().clone(), right.as_ref().clone(), JoinKind::Left, Some(on2))?))
}

/// `CarrierStrategies.copyWithAlias`: a fresh re-aliased copy of a simple source (a table, a CTE reference, a
/// subselect, VALUES), or none.
pub fn copy_with_alias(src: &SqlSource, alias: &str) -> Option<SqlSource> {
    let alias = alias.to_owned();
    Some(match src {
        SqlSource::Table { name, outputs, call, stored_types, quoting, .. } => SqlSource::Table {
            name: name.clone(),
            alias,
            outputs: outputs.clone(),
            call: *call,
            stored_types: stored_types.clone(),
            quoting: *quoting,
        },
        SqlSource::Cte { name, outputs, .. } => SqlSource::Cte { name: name.clone(), alias, outputs: outputs.clone() },
        SqlSource::Subselect { inner, .. } => SqlSource::Subselect { inner: inner.clone(), alias, frame_name: None },
        SqlSource::Values { rows, columns, outputs, .. } => {
            SqlSource::Values { rows: rows.clone(), columns: columns.clone(), alias, outputs: outputs.clone() }
        }
        _ => return None,
    })
}

/// `CarrierStrategies.asOfKey`: an as-of join's right key and whether the join takes its greatest value (`l >= r`)
/// -- its ON's (last) inequality between the two sides; none when it has none.
pub fn as_of_key(j: &SqlSource) -> Result<Option<(SqlExpr, bool)>, Refusal> {
    let SqlSource::Join { right, on, .. } = j else {
        return Ok(None);
    };
    let right_alias = source_alias(right)?;
    let on = on.as_ref().ok_or_else(|| Refusal::null_pointer("the as-of join's ON"))?;
    let mut conjuncts = Vec::new();
    flatten_and(on, &mut conjuncts);
    let mut key = None;
    for c in conjuncts {
        if let SqlExpr::Call { r#fn, args, .. } = c
            && let [a0, a1] = args.as_slice()
        {
            let r0 = mentions_alias(a0, right_alias);
            let r1 = mentions_alias(a1, right_alias);
            if r0 == r1 {
                continue;
            }
            match r#fn {
                SqlFn::GreaterEqual | SqlFn::Greater => key = Some((if r1 { a1 } else { a0 }.clone(), r1)),
                SqlFn::LessEqual | SqlFn::Less => key = Some((if r0 { a0 } else { a1 }.clone(), r0)),
                _ => {}
            }
        }
    }
    Ok(key)
}

fn flatten_and<'a>(e: &'a SqlExpr, out: &mut Vec<&'a SqlExpr>) {
    if let SqlExpr::Call { r#fn: SqlFn::And, args, .. } = e {
        args.iter().for_each(|a| flatten_and(a, out));
        return;
    }
    out.push(e);
}

fn mentions_alias(e: &SqlExpr, alias: &str) -> bool {
    if let SqlExpr::Column(c) = e
        && c.table.as_deref() == Some(alias)
    {
        return true;
    }
    children(e).iter().any(|k| mentions_alias(k, alias))
}

/// `CarrierStrategies.litText`: a pivot value's column-name text -- a string bare, a number plain, any other
/// literal its record text.
fn lit_text(v: &SqlExpr) -> String {
    match v {
        SqlExpr::StringLit { value, .. } => value.clone(),
        SqlExpr::IntLit { value, .. } => value.to_string(),
        SqlExpr::BoolLit { value, .. } => value.to_string(),
        other => record_text(other),
    }
}

/// A literal's record `toString` (`DateLit[iso=2020-01-01, type=Typed[type=DATE, nullable=false]]`); any other node
/// as [`crate::java::expr_text`] names it.
fn record_text(e: &SqlExpr) -> String {
    let fact = |t: &TypeFact| match t {
        TypeFact::Typed { r#type, nullable } => {
            format!("Typed[type={}, nullable={nullable}]", crate::java::sql_type_text(r#type))
        }
        TypeFact::Bottom => "Bottom[]".to_owned(),
        TypeFact::Raises => "Raises[]".to_owned(),
        TypeFact::Unknown => "Unknown[]".to_owned(),
    };
    match e {
        SqlExpr::StringLit { value, r#type } => format!("StringLit[value={value}, type={}]", fact(r#type)),
        SqlExpr::IntLit { value, r#type } => format!("IntLit[value={value}, type={}]", fact(r#type)),
        SqlExpr::BoolLit { value, r#type } => format!("BoolLit[value={value}, type={}]", fact(r#type)),
        SqlExpr::FloatLit { value, r#type } => format!("FloatLit[value={}, type={}]", value.text(), fact(r#type)),
        SqlExpr::DecimalLit { value, r#type } => {
            format!("DecimalLit[value={}, type={}]", json::big_decimal_text(value.as_str()), fact(r#type))
        }
        SqlExpr::DateLit { iso, r#type } => format!("DateLit[iso={iso}, type={}]", fact(r#type)),
        SqlExpr::TimestampLit { iso, r#type } => format!("TimestampLit[iso={iso}, type={}]", fact(r#type)),
        SqlExpr::TimeLit { iso, r#type } => format!("TimeLit[iso={iso}, type={}]", fact(r#type)),
        SqlExpr::NullLit { r#type } => format!("NullLit[type={}]", fact(r#type)),
        other => crate::java::expr_text(other),
    }
}

// ==================================================================
// the expression hook
// ==================================================================

/// `LIST_REDUCERS`: the LIST_* reducers over collection values, as the reductions the fuse rules apply to.
fn list_reducer(f: SqlFn) -> Option<AggFn> {
    Some(match f {
        SqlFn::ListMin => AggFn::Min,
        SqlFn::ListMax => AggFn::Max,
        SqlFn::ListSum => AggFn::Sum,
        SqlFn::ListAvg => AggFn::Avg,
        SqlFn::ListMedian => AggFn::Median,
        _ => return None,
    })
}

impl CarrierStrategies {
    /// The expression hook's rules, in Java's order; none when no rule applies.
    fn rewritten(&mut self, e: &SqlExpr) -> Result<Option<SqlExpr>, Refusal> {
        // LIST_CONCAT over compile-time collections folds (nested concats inside-out: the walk is bottom-up)
        if let SqlExpr::Call { r#fn: SqlFn::ListConcat, args, .. } = e
            && !args.is_empty()
            && args.iter().all(|x| matches!(x, SqlExpr::ArrayLit { .. }))
        {
            let mut els = Vec::new();
            for x in args {
                if let SqlExpr::ArrayLit { elements, .. } = x {
                    els.extend(elements.iter().cloned());
                }
            }
            return Ok(Some(build::array_lit(els)));
        }
        if let SqlExpr::Call { r#fn, args, .. } = e
            && let [coll] = args.as_slice()
            && let Some(red) = list_reducer(*r#fn)
            && let Some(fused) = fuse(red, coll, &[], self.fold_literal_reductions)?
        {
            return Ok(Some(fused));
        }
        // LIST_LENGTH: a literal's length is compile-time, a filtered literal's a sum of tests, a split's token count
        // its separators plus one, a collect's COUNT(*) over the same rows
        if let SqlExpr::Call { r#fn: SqlFn::ListLength, args, .. } = e
            && let [a0] = args.as_slice()
        {
            if let SqlExpr::ArrayLit { elements, .. } = a0 {
                return Ok(Some(int_lit(elements.len() as i64)));
            }
            if let Some(counted) = filtered_literal_length(a0)? {
                return Ok(Some(counted));
            }
            if let SqlExpr::Call { r#fn: SqlFn::Split, args: sp, .. } = a0
                && let [s0, sep @ SqlExpr::StringLit { value: sl, .. }] = sp.as_slice()
                && sl.encode_utf16().count() == 1
            {
                return Ok(Some(call(
                    SqlFn::Plus,
                    vec![
                        call(
                            SqlFn::Minus,
                            vec![
                                call(SqlFn::Length, vec![s0.clone()]),
                                call(
                                    SqlFn::Length,
                                    vec![call(SqlFn::Replace, vec![s0.clone(), sep.clone(), string_lit("")])],
                                ),
                            ],
                        ),
                        int_lit(1),
                    ],
                )));
            }
            if let Some(sel) = collect_select(a0) {
                let count = SqlExpr::Reducer(build::reducer(AggFn::Count, Vec::new(), false, Vec::new()));
                return Ok(Some(build::scalar_subquery(build::query(reprojected(sel, count)?))));
            }
        }
        // the elements of an already-literal collection are the collection
        if let SqlExpr::Call { r#fn: SqlFn::VariantElements, args, .. } = e
            && let [a0 @ SqlExpr::ArrayLit { .. }] = args.as_slice()
        {
            return Ok(Some(a0.clone()));
        }
        // literal-variant const-folds: the JSON text is compile-time
        if let SqlExpr::Call { r#fn: f @ (SqlFn::VariantGet | SqlFn::VariantElements), args, .. } = e
            && let Some(lit) = json_literal(arg(args, 0)?)
        {
            let node = json::parse(lit)?;
            if *f == SqlFn::VariantGet
                && let [_, key] = args.as_slice()
            {
                let picked = match (key, &node) {
                    (SqlExpr::IntLit { value: ix, .. }, Json::List(l)) if *ix >= 0 && (*ix as usize) < l.len() => {
                        Some(&l[*ix as usize])
                    }
                    (SqlExpr::StringLit { value: k, .. }, Json::Map(m)) => {
                        m.iter().find(|(name, _)| name == k).map(|(_, v)| v)
                    }
                    _ => None,
                };
                if let Some(picked) = picked {
                    return Ok(Some(json_lit_expr(picked)));
                }
            }
            if *f == SqlFn::VariantElements
                && let Json::List(l) = &node
            {
                // the elements of a Variant array are Variants: each a JSON cast literal, a scalar too
                let els = l.iter().map(|el| cast(string_lit(&json_text(el)), SqlType::Scalar(Scalar::Json))).collect();
                return Ok(Some(build::array_lit(els)));
            }
        }
        // typeof(e) = 'DATE' is a length test on the text: a DATE casts to 10 characters
        if let SqlExpr::Call { r#fn: SqlFn::Equal, args, .. } = e
            && let [
                SqlExpr::Call { r#fn: SqlFn::Typeof, args: tf, .. },
                SqlExpr::TypeNameLit { scalar: Scalar::Date, .. },
            ] = args.as_slice()
            && let [x] = tf.as_slice()
        {
            return Ok(Some(call(
                SqlFn::Equal,
                vec![call(SqlFn::Length, vec![cast(x.clone(), SqlType::Scalar(Scalar::Varchar))]), int_lit(10)],
            )));
        }
        // a sorted collect is the collect ordered by its value, ASC NULLS LAST, its own keys the tiebreak
        if let SqlExpr::Call { r#fn: SqlFn::ListSort, args, .. } = e
            && let [a0] = args.as_slice()
            && let Some(sel) = collect_select(a0)
        {
            let collect = collect_of(sel);
            let raw = collect.args[0].clone();
            let mut keys = vec![build::sort_key(raw, true, Some(NullOrder::NullsLast), None)];
            keys.extend(collect.order_by.iter().cloned());
            let list = SqlExpr::Reducer(build::reducer(AggFn::List, collect.args.clone(), false, keys));
            return Ok(Some(build::scalar_subquery(build::query(reprojected(sel, list)?))));
        }
        // a filtered collect pushes the element predicate into the collect's WHERE
        if let SqlExpr::Call { r#fn: SqlFn::ListFilter, args, .. } = e
            && let [a0, SqlExpr::Lambda(flam)] = args.as_slice()
            && flam.params.len() == 1
            && let Some(sel) = collect_select(a0)
        {
            let collect = collect_of(sel);
            let pred = subst(&flam.body, &flam.params[0], &collect.args[0])?;
            let w = match &sel.r#where {
                None => pred,
                Some(w) => call(SqlFn::And, vec![w.clone(), pred]),
            };
            return Ok(Some(build::scalar_subquery(build::query(build::with_where(sel.clone(), Some(w))?))));
        }
        if let SqlExpr::Call { r#fn, args, r#type } = e
            && let [SqlExpr::ArrayLit { elements, .. }] = args.as_slice()
            && !elements.is_empty()
            && let Some(folded) = literal_fold(*r#fn, r#type, elements)
        {
            return Ok(Some(folded));
        }
        if let SqlExpr::Call { r#fn: SqlFn::ListGet, args, .. } = e
            && let [coll, idx] = args.as_slice()
        {
            if let Some(got) = list_get_rule(coll, idx)? {
                return Ok(Some(got));
            }
            if let Some(picked) = self.literal_pick(coll, idx)? {
                return Ok(Some(picked));
            }
        }
        if let SqlExpr::Membership { needle, collection, .. } = e
            && let Some(rewritten) = membership_rule(needle, collection)?
        {
            return Ok(Some(rewritten));
        }
        if let SqlExpr::ReduceCollection { reducer, collection, extras, .. } = e
            && let Some(fused) = fuse(*reducer, collection, extras, self.fold_literal_reductions)?
        {
            return Ok(Some(fused));
        }
        Ok(None)
    }

    /// `CarrierStrategies.literalPick`: a literal array read at a computed position, as a CASE over the position
    /// (each piece through this hook again, as Java's `expr(...)`); none where the array is not a literal or the
    /// position is.
    fn literal_pick(&mut self, array: &SqlExpr, position: &SqlExpr) -> Result<Option<SqlExpr>, Refusal> {
        let (element, lit) = match array {
            SqlExpr::Cast { value, target: SqlType::Array { element }, .. } => (Some(element.as_ref()), value.as_ref()),
            _ => (None, array),
        };
        let SqlExpr::ArrayLit { elements, .. } = lit else {
            return Ok(None);
        };
        if matches!(position, SqlExpr::IntLit { .. }) {
            return Ok(None);
        }
        let mut whens = Vec::with_capacity(elements.len());
        for (i, el) in elements.iter().enumerate() {
            let pos = self.expr(position.clone())?.into_inner();
            let then = self.expr(el.clone())?.into_inner();
            whens.push(when(call(SqlFn::Equal, vec![pos, int_lit(i as i64 + 1)]), then));
        }
        let pick = build::case(whens, None);
        // the channel's slot type rides on the pick
        Ok(Some(match element {
            Some(t) => cast(pick, t.clone()),
            None => pick,
        }))
    }
}

/// `CarrierStrategies.fuse`: the reduction pushed into its collecting subselect, or none when the collection is no
/// recognized collect shape. The collect's order keys carry over.
fn fuse(
    reducer: AggFn,
    collection: &SqlExpr,
    extras: &[SqlExpr],
    fold_literals: bool,
) -> Result<Option<SqlExpr>, Refusal> {
    let mut coll = collection.clone();
    let mut transform: Option<Lambda> = None;
    if let SqlExpr::Call { r#fn: SqlFn::ListTransform, args, .. } = &coll
        && let [inner, SqlExpr::Lambda(lam)] = args.as_slice()
        && lam.params.len() == 1
    {
        transform = Some(lam.clone());
        coll = inner.clone();
    }
    // a sorted join: the fused STRING_AGG orders by the raw collected value
    let mut sorted = false;
    if let SqlExpr::Call { r#fn: SqlFn::ListSort, args, .. } = &coll
        && let [inner] = args.as_slice()
    {
        sorted = true;
        coll = inner.clone();
    }
    // a literal collection: STRING_AGG is the CONCAT chain, no subquery at all
    if let SqlExpr::ArrayLit { elements, .. } = &coll
        && reducer == AggFn::StringAgg
        && extras.len() == 1
        && !elements.is_empty()
    {
        return Ok(if fold_literals { Some(concat_join(elements, transform.as_ref(), &extras[0])?) } else { None });
    }
    // the singleton-flatten unwrap: a collect of one-element literals is a collect of the elements
    if let SqlExpr::Call { r#fn: SqlFn::ListFlatten, args, .. } = &coll
        && let [SqlExpr::ScalarSubquery { subquery, .. }] = args.as_slice()
        && let SqlQuery::Select(usel) = subquery.as_ref()
        && usel.projections.len() == 1
        && let SqlExpr::Reducer(ucollect) = &usel.projections[0].expr
        && ucollect.r#fn == AggFn::List
        && !ucollect.distinct
        && let [SqlExpr::Column(ucol)] = ucollect.args.as_slice()
        && let SqlSource::Subselect { inner, alias: usub_alias, frame_name } = &usel.from
        && let SqlQuery::Select(uinner) = inner.as_ref()
        && uinner.projections.len() == 1
        && uinner.projections[0].alias.as_deref() == Some(ucol.name.as_str())
        && let SqlExpr::ArrayLit { elements: ual, .. } = &uinner.projections[0].expr
        && ual.len() == 1
    {
        let new_inner = reprojected(uinner, ual[0].clone())?;
        coll = build::scalar_subquery(build::query(build::with_from(
            usel.as_ref().clone(),
            build::subselect(build::query(new_inner), usub_alias, frame_name.clone()),
        )?));
    }
    // a row-major cell collect: STRING_AGG over the per-row CONCAT of the cells
    if let SqlExpr::Call { r#fn: SqlFn::ListFlatten, args, .. } = &coll
        && let [SqlExpr::ScalarSubquery { subquery, .. }] = args.as_slice()
        && let SqlQuery::Select(fsel) = subquery.as_ref()
        && fsel.projections.len() == 1
        && let SqlExpr::Reducer(fcollect) = &fsel.projections[0].expr
        && fcollect.r#fn == AggFn::List
        && !fcollect.distinct
        && let [SqlExpr::ArrayLit { elements: cells, .. }] = fcollect.args.as_slice()
        && reducer == AggFn::StringAgg
        && extras.len() == 1
        && !cells.is_empty()
    {
        let sep = &extras[0];
        if !sorted {
            let row_joined = concat_join(cells, transform.as_ref(), sep)?;
            let fused = SqlExpr::Reducer(build::reducer(
                AggFn::StringAgg,
                vec![row_joined, sep.clone()],
                false,
                fcollect.order_by.clone(),
            ));
            return Ok(Some(build::scalar_subquery(build::query(reprojected(fsel, fused)?))));
        }
        // a sorted row-major join: the cells sort globally, one branch per compile-time cell
        let mut branches = Vec::with_capacity(cells.len());
        for cell in cells {
            branches.push(build::query(build::with_projections(
                fsel.as_ref().clone(),
                vec![build::projection(cell.clone(), Some("v".to_owned()), None)?],
            )?));
        }
        let union = union_or_one(branches, &[])?;
        return Ok(Some(sorted_cells(union, transform.as_ref(), sep)?));
    }
    // a through-subselect row-major collect: the cells live in the inner select's scope
    if let SqlExpr::Call { r#fn: SqlFn::ListFlatten, args, .. } = &coll
        && let [SqlExpr::ScalarSubquery { subquery, .. }] = args.as_slice()
        && let SqlQuery::Select(fsel2) = subquery.as_ref()
        && fsel2.projections.len() == 1
        && let SqlExpr::Reducer(fc2) = &fsel2.projections[0].expr
        && fc2.r#fn == AggFn::List
        && !fc2.distinct
        && let [fcol_expr @ SqlExpr::Column(fcol)] = fc2.args.as_slice()
        && let SqlSource::Subselect { inner, alias: fsub_alias, frame_name } = &fsel2.from
        && let SqlQuery::Select(finner) = inner.as_ref()
        && !finner.distinct
        && finner.group_by.is_empty()
        && finner.having.is_none()
        && finner.qualify.is_none()
        && finner.limit.is_none()
        && finner.offset.is_none()
        && reducer == AggFn::StringAgg
        && extras.len() == 1
    {
        let mut src = None;
        for (i, p) in finner.projections.iter().enumerate() {
            if p.alias.as_deref() == Some(fcol.name.as_str()) {
                src = Some((i, &p.expr));
            }
        }
        if let Some((src_ix, SqlExpr::ArrayLit { elements: cells2, .. })) = src
            && !cells2.is_empty()
        {
            let sep = &extras[0];
            if !sorted {
                let row_joined = concat_join(cells2, transform.as_ref(), sep)?;
                let mut np = finner.projections.clone();
                np[src_ix] =
                    build::projection(row_joined, Some(fcol.name.clone()), finner.projections[src_ix].out.clone())?;
                let fused = SqlExpr::Reducer(build::reducer(
                    AggFn::StringAgg,
                    vec![fcol_expr.clone(), sep.clone()],
                    false,
                    fc2.order_by.clone(),
                ));
                let rebuilt = build::with_from(
                    fsel2.as_ref().clone(),
                    build::subselect(
                        build::query(build::with_projections(finner.as_ref().clone(), np)?),
                        fsub_alias,
                        frame_name.clone(),
                    ),
                )?;
                return Ok(Some(build::scalar_subquery(build::query(reprojected(&rebuilt, fused)?))));
            }
            let mut branches = Vec::with_capacity(cells2.len());
            for cell in cells2 {
                branches.push(build::query(build::with_projections(
                    finner.as_ref().clone(),
                    vec![build::projection(cell.clone(), Some("v".to_owned()), None)?],
                )?));
            }
            // no one-branch case here: a single cell is Java's union refusal
            let union = SqlQuery::Union(build::union(branches, true, Vec::new())?);
            return Ok(Some(sorted_cells(union, transform.as_ref(), sep)?));
        }
    }
    let SqlExpr::ScalarSubquery { subquery, .. } = &coll else {
        return Ok(None);
    };
    let SqlQuery::Select(sel) = subquery.as_ref() else {
        return Ok(None);
    };
    if sel.projections.len() != 1 {
        return Ok(None);
    }
    let SqlExpr::Reducer(collect) = &sel.projections[0].expr else {
        return Ok(None);
    };
    if collect.r#fn != AggFn::List || collect.distinct || collect.args.len() != 1 {
        return Ok(None);
    }
    let raw = &collect.args[0];
    let value = match &transform {
        Some(t) => subst(&t.body, name_at(&t.params, 0)?, raw)?,
        None => raw.clone(),
    };
    let mut args = vec![value];
    args.extend(extras.iter().cloned());
    let order = if sorted { vec![build::sort_key(raw.clone(), true, None, None)] } else { collect.order_by.clone() };
    let fused = SqlExpr::Reducer(build::reducer(reducer, args, false, order));
    Ok(Some(build::scalar_subquery(build::query(reprojected(sel, fused)?))))
}

/// `STRING_AGG(t(v), sep ORDER BY v)` over the cells' union, aliased `_cells`.
fn sorted_cells(union: SqlQuery, transform: Option<&Lambda>, sep: &SqlExpr) -> Result<SqlExpr, Refusal> {
    let v_read = build::derived(Some("_cells"), "v");
    let tv = match transform {
        None => v_read.clone(),
        Some(t) => subst(&t.body, name_at(&t.params, 0)?, &v_read)?,
    };
    let fused = SqlExpr::Reducer(build::reducer(
        AggFn::StringAgg,
        vec![tv, sep.clone()],
        false,
        vec![build::sort_key(v_read, true, None, None)],
    ));
    let outer = build::with_projections(
        build::star_of(build::subselect(union, "_cells", None))?,
        vec![build::projection(fused, None, None)?],
    )?;
    Ok(build::scalar_subquery(build::query(outer)))
}

/// `CarrierStrategies.concatJoin`: `t(e1) || sep || t(e2) || ...` over compile-time elements.
fn concat_join(elements: &[SqlExpr], transform: Option<&Lambda>, sep: &SqlExpr) -> Result<SqlExpr, Refusal> {
    let mut out: Option<SqlExpr> = None;
    for e in elements {
        let v = match transform {
            None => e.clone(),
            Some(t) => subst(&t.body, name_at(&t.params, 0)?, e)?,
        };
        out = Some(match out {
            None => v,
            Some(o) => call(SqlFn::Concat, vec![call(SqlFn::Concat, vec![o, sep.clone()]), v]),
        });
    }
    out.ok_or_else(|| Refusal::null_pointer("concatJoin over no elements"))
}

/// `CarrierStrategies.listGetRule`: the portable LIST_GET for the witnessed shapes (1-based, -1 the last, 0 and out
/// of range NULL); none when the shape is unwitnessed.
fn list_get_rule(coll: &SqlExpr, idx: &SqlExpr) -> Result<Option<SqlExpr>, Refusal> {
    let SqlExpr::IntLit { value: i, .. } = idx else {
        return Ok(None);
    };
    let i = *i;
    // a compile-time pick over a literal collection
    if let SqlExpr::ArrayLit { elements, .. } = coll {
        let n = elements.len() as i64;
        let pos = if i > 0 {
            i
        } else if i < 0 {
            n + i + 1
        } else {
            0
        };
        return Ok(Some(if pos >= 1 && pos <= n { elements[(pos - 1) as usize].clone() } else { build::null_lit() }));
    }
    // the first-non-null idiom: LIST_FILTER(lit, x | x IS NOT NULL)[1] is COALESCE over the elements
    if i == 1
        && let SqlExpr::Call { r#fn: SqlFn::ListFilter, args, .. } = coll
        && let [SqlExpr::ArrayLit { elements: fal, .. }, SqlExpr::Lambda(lam)] = args.as_slice()
        && !fal.is_empty()
        && is_not_null_of_param(lam)
    {
        return Ok(Some(if fal.len() == 1 { fal[0].clone() } else { call(SqlFn::Coalesce, fal.clone()) }));
    }
    // a token pick over a split: SPLIT_PART guarded to NULL when the token is missing
    if i >= 1
        && let SqlExpr::Call { r#fn: SqlFn::Split, args, .. } = coll
        && let [s0, sep @ SqlExpr::StringLit { value: sl, .. }] = args.as_slice()
        && sl.encode_utf16().count() == 1
    {
        let missing = call(
            SqlFn::Less,
            vec![
                call(
                    SqlFn::Minus,
                    vec![
                        call(SqlFn::Length, vec![s0.clone()]),
                        call(SqlFn::Length, vec![call(SqlFn::Replace, vec![s0.clone(), sep.clone(), string_lit("")])]),
                    ],
                ),
                int_lit(i - 1),
            ],
        );
        let part = call(SqlFn::SplitPart, vec![s0.clone(), sep.clone(), idx.clone()]);
        return Ok(Some(if i == 1 { part } else { build::case(vec![when(missing, build::null_lit())], Some(part)) }));
    }
    // an element pick over a collect: an order-carrying LIMIT/OFFSET; the last needs keys to flip
    if let Some(sel) = collect_select(coll) {
        let collect = collect_of(sel);
        let picked = reprojected(sel, collect.args[0].clone())?;
        if i >= 1 {
            let p = build::with_order_by(picked, collect.order_by.clone())?;
            let p = build::with_offset(build::with_limit(p, Some(1))?, if i > 1 { Some(i - 1) } else { None })?;
            return Ok(Some(build::scalar_subquery(build::query(p))));
        }
        if i == -1 && !collect.order_by.is_empty() {
            let flipped = collect
                .order_by
                .iter()
                .map(|k| build::sort_key(k.expr.clone(), !k.ascending, flip_nulls(k.null_order), k.output_name.clone()))
                .collect();
            let p = build::with_limit(build::with_order_by(picked, flipped)?, Some(1))?;
            return Ok(Some(build::scalar_subquery(build::query(p))));
        }
    }
    Ok(None)
}

/// A one-parameter lambda `x -> x IS NOT NULL`.
fn is_not_null_of_param(lam: &Lambda) -> bool {
    if let [p] = lam.params.as_slice()
        && let SqlExpr::Call { r#fn: SqlFn::IsNotNull, args, .. } = lam.body.as_ref()
        && let [SqlExpr::Column(pc)] = args.as_slice()
        && pc.table.is_none()
        && pc.name == *p
    {
        return true;
    }
    false
}

/// `CarrierStrategies.jsonLiteral`: the compile-time JSON text of `CAST('...' AS JSON)`.
fn json_literal(e: &SqlExpr) -> Option<&str> {
    if let SqlExpr::Cast { value, target: SqlType::Scalar(Scalar::Json), .. } = e
        && let SqlExpr::StringLit { value, .. } = value.as_ref()
    {
        return Some(value);
    }
    None
}

/// `CarrierStrategies.jsonLitExpr`: a parsed node as the literal it prints as -- a scalar a plain literal, anything
/// else (a composite, a decimal, a big integer) a JSON cast of its text.
fn json_lit_expr(node: &Json) -> SqlExpr {
    match node {
        Json::Null => build::null_lit(),
        Json::Str(s) => string_lit(s),
        Json::Long(n) => int_lit(*n),
        Json::Bool(b) => build::bool_lit(*b),
        other => cast(string_lit(&json_text(other)), SqlType::Scalar(Scalar::Json)),
    }
}

/// `CarrierStrategies.jsonText`: the minimal JSON writer for re-emitting folded nodes (a string's backslashes and
/// quotes escaped, nothing else; a number as `String.valueOf` writes it).
fn json_text(node: &Json) -> String {
    match node {
        Json::Null => "null".to_owned(),
        Json::Str(s) => format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\"")),
        Json::List(l) => format!("[{}]", l.iter().map(json_text).collect::<Vec<_>>().join(",")),
        Json::Map(m) => format!(
            "{{{}}}",
            m.iter()
                .map(|(k, v)| format!("{}:{}", json_text(&Json::Str(k.clone())), json_text(v)))
                .collect::<Vec<_>>()
                .join(",")
        ),
        Json::Long(n) => n.to_string(),
        Json::BigInteger(t) => t.clone(),
        Json::Decimal(t) => json::big_decimal_text(t),
        Json::Bool(b) => b.to_string(),
    }
}

/// `CarrierStrategies.literalFold`: BOOL_AND/BOOL_OR/SUM/PRODUCT over a compile-time collection as the exact
/// null-ignoring fold, delivering the reduction's own type; none for any other reduction.
fn literal_fold(f: SqlFn, lr_type: &TypeFact, elements: &[SqlExpr]) -> Option<SqlExpr> {
    let folded = match f {
        SqlFn::ListBoolAnd => lit_fold(elements, build::bool_lit(true), SqlFn::And, None),
        SqlFn::ListBoolOr => lit_fold(elements, build::bool_lit(false), SqlFn::Or, None),
        SqlFn::ListSum => lit_fold(elements, int_lit(0), SqlFn::Plus, None),
        // a list product is DOUBLE: the leading 1.0 factor pins the type
        SqlFn::ListProduct => lit_fold(elements, int_lit(1), SqlFn::Times, Some(build::float_lit(1.0))),
        _ => return None,
    };
    if let TypeFact::Typed { r#type: t, .. } = lr_type
        && !matches!(folded.r#type(), TypeFact::Typed { r#type: ft, .. } if ft == t)
    {
        return Some(build::cast_conform(folded, t.clone(), false));
    }
    Some(folded)
}

/// `CASE WHEN all null THEN NULL ELSE op-chain of COALESCE(e, neutral) END`, `lead` prepended to the chain.
fn lit_fold(elements: &[SqlExpr], neutral: SqlExpr, op: SqlFn, lead: Option<SqlExpr>) -> SqlExpr {
    let mut all_null: Option<SqlExpr> = None;
    let mut chain = lead;
    for el in elements {
        let is_null = call(SqlFn::IsNull, vec![el.clone()]);
        all_null = Some(match all_null {
            None => is_null,
            Some(a) => call(SqlFn::And, vec![a, is_null]),
        });
        let v = call(SqlFn::Coalesce, vec![el.clone(), neutral.clone()]);
        chain = Some(match chain {
            None => v,
            Some(c) => call(op, vec![c, v]),
        });
    }
    build::case(
        vec![when(all_null.expect("a non-empty literal"), build::null_lit())],
        Some(chain.expect("a non-empty literal")),
    )
}

fn flip_nulls(n: Option<NullOrder>) -> Option<NullOrder> {
    n.map(|n| if n == NullOrder::NullsFirst { NullOrder::NullsLast } else { NullOrder::NullsFirst })
}

/// `CarrierStrategies.membershipRule`: a literal collection's OR-chain of equalities (NULL elements dropped), pushed
/// through LIST_CONCAT and CASE, seen through a non-null filter or a compaction, a collect's EXISTS; none when any
/// arm has no rule.
fn membership_rule(needle: &SqlExpr, collection: &SqlExpr) -> Result<Option<SqlExpr>, Refusal> {
    if let SqlExpr::ArrayLit { elements, .. } = collection {
        let mut chain: Option<SqlExpr> = None;
        for el in elements {
            if matches!(el, SqlExpr::NullLit { .. }) {
                continue;
            }
            let eq = call(SqlFn::Equal, vec![needle.clone(), el.clone()]);
            chain = Some(match chain {
                None => eq,
                Some(c) => call(SqlFn::Or, vec![c, eq]),
            });
        }
        return Ok(Some(chain.unwrap_or_else(|| build::bool_lit(false))));
    }
    if let SqlExpr::Call { r#fn: SqlFn::ListConcat, args, .. } = collection
        && args.len() >= 2
    {
        let mut chain: Option<SqlExpr> = None;
        for a in args {
            let Some(member) = membership_rule(needle, a)? else {
                return Ok(None);
            };
            chain = Some(match chain {
                None => member,
                Some(c) => call(SqlFn::Or, vec![c, member]),
            });
        }
        return Ok(chain);
    }
    if let SqlExpr::Case { whens, otherwise, .. } = collection {
        let mut ws = Vec::with_capacity(whens.len());
        for w in whens {
            let Some(member) = membership_rule(needle, &w.then)? else {
                return Ok(None);
            };
            ws.push(when(w.condition.clone(), member));
        }
        let mut ot = None;
        if let Some(o) = otherwise {
            let Some(member) = membership_rule(needle, o)? else {
                return Ok(None);
            };
            ot = Some(member);
        }
        return Ok(Some(build::case(ws, ot)));
    }
    // a non-null filter is redundant under membership
    if let SqlExpr::Call { r#fn: SqlFn::ListFilter, args, .. } = collection
        && let [inner, SqlExpr::Lambda(nn)] = args.as_slice()
        && is_not_null_of_param(nn)
    {
        return membership_rule(needle, inner);
    }
    // the compact list (nulls dropped) is the same redundancy
    if let SqlExpr::CompactList { list, .. } = collection {
        return membership_rule(needle, list);
    }
    if let SqlExpr::ScalarSubquery { subquery, .. } = collection
        && let SqlQuery::Select(sel) = subquery.as_ref()
        && sel.projections.len() == 1
        && let SqlExpr::Reducer(collect) = &sel.projections[0].expr
        && collect.r#fn == AggFn::List
        && !collect.distinct
        && let [raw] = collect.args.as_slice()
    {
        let eq = call(SqlFn::Equal, vec![raw.clone(), needle.clone()]);
        let inner = build::with_projections(sel.as_ref().clone(), vec![build::projection(int_lit(1), None, None)?])?;
        let w = match &inner.r#where {
            None => eq,
            Some(w) => call(SqlFn::And, vec![w.clone(), eq]),
        };
        let with_eq = build::with_where(inner, Some(w))?;
        return Ok(Some(build::exists(build::query(with_eq))));
    }
    Ok(None)
}

/// `CarrierStrategies.filteredLiteralLength`: a literal list's filtered length as the sum over its elements of
/// `CASE WHEN p1(e) AND ... THEN 1 ELSE 0 END`; none when the argument is no such chain.
fn filtered_literal_length(a: &SqlExpr) -> Result<Option<SqlExpr>, Refusal> {
    let mut filters: Vec<&Lambda> = Vec::new();
    let mut x = a;
    // a compacted list drops its NULL elements: counted, an element must be non-null too
    let mut compacted = false;
    loop {
        match x {
            SqlExpr::CompactList { list, .. } => {
                compacted = true;
                x = list;
            }
            SqlExpr::Call { r#fn: SqlFn::ListFilter, args, .. } => match args.as_slice() {
                [inner, SqlExpr::Lambda(lam)] if lam.params.len() == 1 => {
                    filters.insert(0, lam);
                    x = inner;
                }
                _ => break,
            },
            _ => break,
        }
    }
    let SqlExpr::ArrayLit { elements, .. } = x else {
        return Ok(None);
    };
    if filters.is_empty() {
        return Ok(None);
    }
    let mut sum: Option<SqlExpr> = None;
    for el in elements {
        let mut cond =
            if compacted { Some(null_tests_answered(call(SqlFn::IsNotNull, vec![el.clone()]))?) } else { None };
        for lam in &filters {
            let p = null_tests_answered(subst(&lam.body, &lam.params[0], el)?)?;
            cond = Some(match cond {
                None => p,
                Some(c) => call(SqlFn::And, vec![c, p]),
            });
        }
        let one = build::case(vec![when(cond.expect("at least one filter"), int_lit(1))], Some(int_lit(0)));
        sum = Some(match sum {
            None => one,
            Some(s) => call(SqlFn::Plus, vec![s, one]),
        });
    }
    Ok(Some(sum.unwrap_or_else(|| int_lit(0))))
}

#[cfg(test)]
mod tests;
