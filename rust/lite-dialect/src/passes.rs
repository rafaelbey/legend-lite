// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! The MIR passes the DuckDB dialect runs, each its Java class ported (`core/src/main/java/com/legend/sql/dialect/
//! <Name>.java`), in the order [`crate::ansi::render_passes`] runs them (Java's `AnsiSqlRenderer.renderPasses`): the
//! Float expansions, the dialect's `passes()` ([`duckdb_passes`]: Java's `DuckDb.passes`, line 675), then
//! [`StoredReads`]. `CarrierStrategies` is [`crate::carrier`], `StableScanOrder` [`crate::scan_order`]; [`Stars`] is
//! the helper `VariantEgress` and `StoredReads` spell a star with.
//!
//! Each pass rebuilds through [`crate::build`], so the nodes it makes carry Java's type facts; where Java tests
//! identity (`==`) the pass answers [`Rw::Same`] exactly where Java's would hand back its input.
//!
//! [`rust/lite-dialect/PARITY.md`](../PARITY.md) measures the pipeline on the golden dump.

use crate::build::{self, call, cast, int_lit, string_lit, when};
use crate::float_text;
use crate::refusal::{Refusal, arg};
use crate::rewriter::{Rw, SqlRewriter, rewrite, rewrite_expr};
use crate::typing::{children, source_alias, source_outputs};
use lite_sql::{
    AggFn, ColumnType, Hoist, JoinKind, Origin, OutputCol, Projection, Reducer, Scalar, SortKey, SqlAgg, SqlExpr,
    SqlFn, SqlQuery, SqlSelect, SqlSource, SqlType, StructLitField, TypeFact,
};
use std::collections::{HashMap, HashSet};

pub use crate::carrier::{Caps, CarrierStrategies};
pub use crate::scan_order::StableScanOrder;

fn varchar() -> SqlType {
    SqlType::Scalar(Scalar::Varchar)
}

/// Java's `List.get(i)` of a list of names, refused past the end in `ArrayList`'s words.
fn name_at(names: &[String], i: usize) -> Result<&str, Refusal> {
    names.get(i).map(String::as_str).ok_or_else(|| Refusal {
        class: Refusal::INDEX_OUT_OF_BOUNDS,
        message: format!("Index {i} out of bounds for length {}", names.len()),
    })
}

// ==================================================================
// FloatPrintText.Expansion, FloatCanonText.Expansion
// ==================================================================

/// The first pass of a dialect that does not spell `FLOAT_PRINT` itself (Java's `FloatPrintText.Expansion`): each
/// call becomes [`float_text::print_of`], so every later pass sees the expanded tree.
#[derive(Debug)]
pub struct FloatPrintExpansion {
    /// Java's `bind`: the value and its text read from a one-row derived table, each written once.
    pub bind: bool,
    /// Java's `fresh`: the derived tables' alias counter (`__fpN`), per pass object.
    pub fresh: u32,
    /// Java's `spelledCalls`: an equal call is the same spelling (a `HashMap` keyed by record equality).
    spelled: Vec<(SqlExpr, SqlExpr)>,
}

impl FloatPrintExpansion {
    pub fn new(bind: bool) -> FloatPrintExpansion {
        FloatPrintExpansion { bind, fresh: 0, spelled: Vec::new() }
    }

    fn bound(&mut self, x: SqlExpr) -> Result<SqlExpr, Refusal> {
        let inner = format!("__fp{}", self.fresh);
        let outer = format!("__fp{}", self.fresh + 1);
        self.fresh += 2;
        let double = || SqlType::Scalar(Scalar::Double);
        let projection = |e: SqlExpr, name: &str, t: SqlType| {
            build::projection(e, Some(name.to_owned()), Some(build::output_col(name, t, true)))
        };
        let column =
            |alias: &str, name: &str, t: SqlType| build::typed_column(Some(alias), name, t, true, Origin::Derived);
        let value = build::select_of(vec![projection(x, "v", double())?], SqlSource::Dual, Vec::new())?;
        let v = column(&inner, "v", double());
        let texted = build::select_of(
            vec![projection(v.clone(), "v", double())?, projection(cast(v, varchar()), "t", varchar())?],
            build::subselect(build::query(value), &inner, None),
            Vec::new(),
        )?;
        let spelled = float_text::print(column(&outer, "v", double()), column(&outer, "t", varchar()));
        let used = build::select_of(
            vec![projection(spelled, "p", varchar())?],
            build::subselect(build::query(texted), &outer, None),
            Vec::new(),
        )?;
        Ok(build::scalar_subquery(build::query(used)))
    }
}

fn is_double(x: &SqlExpr) -> bool {
    matches!(x.r#type(), TypeFact::Typed { r#type: SqlType::Scalar(Scalar::Double), .. })
}

impl SqlRewriter for FloatPrintExpansion {
    fn name(&self) -> &'static str {
        "FloatPrintText.Expansion"
    }

    fn expr(&mut self, e: SqlExpr) -> Result<Rw<SqlExpr>, Refusal> {
        let SqlExpr::Call { r#fn: SqlFn::FloatPrint, args, .. } = &e else {
            return Ok(Rw::Same(e));
        };
        // Java's `expr(c.args().get(0))`: this hook over the (already walked) argument
        let x = self.expr(arg(args, 0)?.clone())?.into_inner();
        if !(self.bind && is_double(&x)) {
            return Ok(Rw::New(float_text::print_of(x)));
        }
        if let Some((_, known)) = self.spelled.iter().find(|(k, _)| *k == e) {
            return Ok(Rw::New(known.clone()));
        }
        let spelled = self.bound(x)?;
        self.spelled.push((e, spelled.clone()));
        Ok(Rw::New(spelled))
    }
}

/// The first pass of a dialect that does not spell `FLOAT_CANON` itself (Java's `FloatCanonText.Expansion`): each
/// call becomes [`float_text::canon_of`].
#[derive(Debug)]
pub struct FloatCanonExpansion {
    /// Java's `bindText`: the double's text read from a one-row derived table, written once.
    pub bind_text: bool,
    /// Java's `fresh`: the derived tables' alias counter (`__fcN`), per pass object.
    pub fresh: u32,
    /// Java's `spelledCalls`: an equal call is the same node, its alias too.
    spelled: Vec<(SqlExpr, SqlExpr)>,
}

impl FloatCanonExpansion {
    pub fn new(bind_text: bool) -> FloatCanonExpansion {
        FloatCanonExpansion { bind_text, fresh: 0, spelled: Vec::new() }
    }
}

impl SqlRewriter for FloatCanonExpansion {
    fn name(&self) -> &'static str {
        "FloatCanonText.Expansion"
    }

    fn expr(&mut self, e: SqlExpr) -> Result<Rw<SqlExpr>, Refusal> {
        let SqlExpr::Call { r#fn: SqlFn::FloatCanon, args, .. } = &e else {
            return Ok(Rw::Same(e));
        };
        if !self.bind_text {
            return Ok(Rw::New(float_text::canon_of(arg(args, 0)?.clone())));
        }
        if let Some((_, known)) = self.spelled.iter().find(|(k, _)| *k == e) {
            return Ok(Rw::New(known.clone()));
        }
        let alias = format!("__fc{}", self.fresh);
        self.fresh += 1;
        let text = cast(arg(args, 0)?.clone(), varchar());
        let t = build::output_col("t", varchar(), true);
        let reference = build::typed_column(Some(&alias), "t", varchar(), true, Origin::Derived);
        let bind = build::select_of(
            vec![build::projection(text, Some("t".to_owned()), Some(t))?],
            SqlSource::Dual,
            Vec::new(),
        )?;
        let spelled = float_text::canon_of(reference);
        let used = build::select_of(
            vec![build::projection(spelled, Some("v".to_owned()), Some(build::output_col("v", varchar(), true)))?],
            build::subselect(build::query(bind), &alias, None),
            Vec::new(),
        )?;
        let spelled_call = build::scalar_subquery(build::query(used));
        self.spelled.push((e, spelled_call.clone()));
        Ok(Rw::New(spelled_call))
    }
}

// ==================================================================
// QuantileOrder
// ==================================================================

/// DuckDB's quantile family takes no within-group order (Java's `QuantileOrder`): an ascending continuous
/// percentile interpolates over the sorted list, a descending one over the negated values, a discrete one picks the
/// ceil(p*N)-th element; a windowed percentile windows every reducer of its encoding.
#[derive(Debug, Default)]
pub struct QuantileOrder;

fn ordered(r: &Reducer) -> bool {
    matches!(r.r#fn, AggFn::QuantileCont | AggFn::QuantileDisc) && r.order_by.len() <= 1 && r.args.len() == 2
}

fn encode(r: &Reducer) -> SqlExpr {
    let value = r.args[0].clone();
    let p = r.args[1].clone();
    let ascending = r.order_by.first().is_none_or(|k| k.ascending);
    if r.r#fn == AggFn::QuantileDisc {
        return discrete(value, p, r.distinct, ascending);
    }
    if ascending {
        return interpolate(value, p, r.distinct);
    }
    call(SqlFn::Negate, vec![interpolate(call(SqlFn::Negate, vec![value]), p, r.distinct)])
}

fn reducer_expr(f: AggFn, value: SqlExpr, distinct: bool) -> SqlExpr {
    SqlExpr::Reducer(build::reducer(f, vec![value], distinct, Vec::new()))
}

fn discrete(value: SqlExpr, p: SqlExpr, distinct: bool, ascending: bool) -> SqlExpr {
    call(
        SqlFn::ListGet,
        vec![
            call(
                if ascending { SqlFn::ListSort } else { SqlFn::ListSortDesc },
                vec![reducer_expr(AggFn::List, value.clone(), distinct)],
            ),
            call(
                SqlFn::Greatest,
                vec![
                    cast(
                        call(
                            SqlFn::Ceiling,
                            vec![call(
                                SqlFn::Times,
                                vec![
                                    cast(p, SqlType::Scalar(Scalar::Double)),
                                    reducer_expr(AggFn::Count, value, distinct),
                                ],
                            )],
                        ),
                        SqlType::Scalar(Scalar::Bigint),
                    ),
                    int_lit(1),
                ],
            ),
        ],
    )
}

fn interpolate(value: SqlExpr, p: SqlExpr, distinct: bool) -> SqlExpr {
    let sorted = call(SqlFn::ListSort, vec![reducer_expr(AggFn::List, value.clone(), distinct)]);
    let n = reducer_expr(AggFn::Count, value, distinct);
    let idx = call(SqlFn::Times, vec![p, call(SqlFn::Minus, vec![n, int_lit(1)])]);
    let floor_idx = call(SqlFn::Floor, vec![idx.clone()]);
    let lo = at(sorted.clone(), floor_idx.clone());
    let hi = at(sorted, call(SqlFn::Ceiling, vec![idx.clone()]));
    call(
        SqlFn::Plus,
        vec![
            lo.clone(),
            call(SqlFn::Times, vec![call(SqlFn::Minus, vec![idx, floor_idx]), call(SqlFn::Minus, vec![hi, lo])]),
        ],
    )
}

fn at(sorted: SqlExpr, zero_based: SqlExpr) -> SqlExpr {
    call(
        SqlFn::ListGet,
        vec![sorted, call(SqlFn::Plus, vec![cast(zero_based, SqlType::Scalar(Scalar::Bigint)), int_lit(1)])],
    )
}

/// Every reducer of the encoding takes the window's spec.
fn windowize(e: SqlExpr, partition_by: &[SqlExpr], order_by: &[SortKey], frame: &Option<lite_sql::Frame>) -> SqlExpr {
    match e {
        SqlExpr::Reducer(r) => {
            build::window_call(SqlAgg::Reducer(r), partition_by.to_vec(), order_by.to_vec(), frame.clone())
        }
        SqlExpr::Call { r#fn, args, .. } => {
            call(r#fn, args.into_iter().map(|x| windowize(x, partition_by, order_by, frame)).collect())
        }
        SqlExpr::Cast { value, target, conform, .. } => {
            build::cast_conform(windowize(*value, partition_by, order_by, frame), target, conform)
        }
        other => other,
    }
}

impl SqlRewriter for QuantileOrder {
    fn name(&self) -> &'static str {
        "QuantileOrder"
    }

    fn expr(&mut self, e: SqlExpr) -> Result<Rw<SqlExpr>, Refusal> {
        if let SqlExpr::Reducer(r) = &e
            && ordered(r)
        {
            return Ok(Rw::New(encode(r)));
        }
        if let SqlExpr::WindowCall { r#fn, partition_by, order_by, frame, .. } = &e
            && let SqlAgg::Reducer(r) = r#fn.as_ref()
            && ordered(r)
        {
            return Ok(Rw::New(windowize(encode(r), partition_by, order_by, frame)));
        }
        Ok(Rw::Same(e))
    }
}

// ==================================================================
// UnqualifyPivotArgs
// ==================================================================

/// DuckDB's PIVOT forbids qualified column references inside ON and USING (Java's `UnqualifyPivotArgs`): every
/// pivot is rebuilt with its ON keys and USING aggregates unqualified -- a walk of its own that stops at subqueries
/// and lambdas.
#[derive(Debug, Default)]
pub struct UnqualifyPivotArgs;

fn unqualify(e: SqlExpr) -> SqlExpr {
    let all = |es: Vec<SqlExpr>| es.into_iter().map(unqualify).collect::<Vec<_>>();
    match e {
        SqlExpr::Column(c) => SqlExpr::Column(lite_sql::Column { table: None, ..c }),
        SqlExpr::StoredRead { column, stored, .. } => {
            build::stored_read(lite_sql::Column { table: None, ..column }, stored)
        }
        SqlExpr::RowOrder { .. } => build::row_order(None),
        SqlExpr::Call { r#fn, args, .. } => call(r#fn, all(args)),
        SqlExpr::Cast { value, target, conform, .. } => build::cast_conform(unqualify(*value), target, conform),
        SqlExpr::Case { whens, otherwise, .. } => build::case(
            whens.into_iter().map(|w| when(unqualify(w.condition), unqualify(w.then))).collect(),
            otherwise.map(|o| unqualify(*o)),
        ),
        SqlExpr::ArrayLit { elements, .. } => build::array_lit(all(elements)),
        SqlExpr::StructGet { source, field, .. } => build::struct_get(unqualify(*source), &field),
        SqlExpr::StructLit { fields, .. } => {
            build::struct_lit(fields.into_iter().map(|f| StructLitField { value: unqualify(f.value), ..f }).collect())
        }
        SqlExpr::OrderedListAgg { value, order_by, .. } => {
            build::ordered_list_agg(unqualify(*value), unqualify(*order_by))
        }
        SqlExpr::JsonObject { kv, .. } => build::json_object(all(kv)),
        SqlExpr::JsonArray { elements, .. } => build::json_array(all(elements)),
        SqlExpr::JsonArrayAgg { value, order_keys, .. } => build::json_array_agg(
            unqualify(*value),
            order_keys
                .into_iter()
                .map(|k| lite_sql::JsonArrayAggKey { expr: unqualify(k.expr), desc: k.desc })
                .collect(),
        ),
        SqlExpr::Reducer(r) => SqlExpr::Reducer(unqualify_reducer(r)),
        SqlExpr::FoldCall { source, lambda, init, acc_is_list, homogeneous, .. } => {
            build::fold_call(unqualify(*source), lambda, unqualify(*init), acc_is_list, homogeneous)
        }
        SqlExpr::WindowCall { r#fn, partition_by, order_by, frame, .. } => {
            build::window_call(*r#fn, all(partition_by), order_by, frame)
        }
        SqlExpr::Group { inner, .. } => build::group(unqualify(*inner)),
        // subqueries own their aliases; lambdas bind their own params; leaves carry no qualifier
        other => other,
    }
}

fn unqualify_reducer(r: Reducer) -> Reducer {
    build::reducer(
        r.r#fn,
        r.args.into_iter().map(unqualify).collect(),
        r.distinct,
        r.order_by.into_iter().map(|k| SortKey { expr: unqualify(k.expr), ..k }).collect(),
    )
}

impl SqlRewriter for UnqualifyPivotArgs {
    fn name(&self) -> &'static str {
        "UnqualifyPivotArgs"
    }

    fn source(&mut self, s: SqlSource) -> Result<Rw<SqlSource>, Refusal> {
        let SqlSource::Pivot { source, on, r#in, named, usings, alias, outputs } = s else {
            return Ok(Rw::Same(s));
        };
        Ok(Rw::New(SqlSource::Pivot {
            source,
            on: on.into_iter().map(unqualify).collect(),
            r#in,
            named,
            usings: usings.into_iter().map(|u| lite_sql::PivotUsing { agg: unqualify_reducer(u.agg), ..u }).collect(),
            alias,
            outputs,
        }))
    }
}

// ==================================================================
// FoldToListReduce
// ==================================================================

/// DuckDB's fold encoding (Java's `FoldToListReduce`): a `FoldCall` becomes a `LIST_REDUCE` call tree, Pure's
/// `(element, acc)` lambda swapped to DuckDB's `(acc, element)`; a list accumulator wraps each element as a
/// one-element list and unwraps its references.
#[derive(Debug, Default)]
pub struct FoldToListReduce;

impl SqlRewriter for FoldToListReduce {
    fn name(&self) -> &'static str {
        "FoldToListReduce"
    }

    fn expr(&mut self, e: SqlExpr) -> Result<Rw<SqlExpr>, Refusal> {
        let SqlExpr::FoldCall { source, lambda, init, acc_is_list, homogeneous, .. } = e else {
            return Ok(Rw::Same(e));
        };
        let elem = name_at(&lambda.params, 0)?.to_owned();
        let acc = name_at(&lambda.params, 1)?.to_owned();
        if !acc_is_list {
            if !homogeneous {
                return Err(Refusal::illegal_state(
                    "fold body is not decomposable and the accumulator is scalar — rewrite accumulator-first ({e, a | \
                     $a <op> ...}) so the reduction can decompose",
                ));
            }
            let swapped = SqlExpr::Lambda(build::lambda_of(vec![acc, elem], *lambda.body));
            // a fold over the empty (SQL NULL) collection is the init value
            return Ok(Rw::New(call(
                SqlFn::ListReduce,
                vec![call(SqlFn::Coalesce, vec![*source, build::array_lit(Vec::new())]), swapped, *init],
            )));
        }
        let wrapped = call(
            SqlFn::ListTransform,
            vec![
                *source,
                SqlExpr::Lambda(build::lambda_of(
                    vec![elem.clone()],
                    build::array_lit(vec![build::derived(None, &elem)]),
                )),
            ],
        );
        let body = unwrap_elem_refs(*lambda.body, &elem);
        let swapped = SqlExpr::Lambda(build::lambda_of(vec![acc, elem], body));
        Ok(Rw::New(call(SqlFn::ListReduce, vec![wrapped, swapped, *init])))
    }
}

/// Every bare reference to `elem` as `LIST_GET(elem, 1)`, through casts, cases, arrays and inner lambdas (one
/// re-binding the name shadows it).
fn unwrap_elem_refs(e: SqlExpr, elem: &str) -> SqlExpr {
    let all = |es: Vec<SqlExpr>| es.into_iter().map(|x| unwrap_elem_refs(x, elem)).collect::<Vec<_>>();
    match e {
        SqlExpr::Column(c) if c.table.is_none() && c.name == elem => {
            call(SqlFn::ListGet, vec![build::derived(None, elem), int_lit(1)])
        }
        SqlExpr::Call { r#fn, args, .. } => call(r#fn, all(args)),
        // the two-argument constructor: not a conformance cast
        SqlExpr::Cast { value, target, .. } => cast(unwrap_elem_refs(*value, elem), target),
        SqlExpr::ArrayLit { elements, .. } => build::array_lit(all(elements)),
        SqlExpr::Case { whens, otherwise, .. } => build::case(
            whens
                .into_iter()
                .map(|w| when(unwrap_elem_refs(w.condition, elem), unwrap_elem_refs(w.then, elem)))
                .collect(),
            otherwise.map(|o| unwrap_elem_refs(*o, elem)),
        ),
        SqlExpr::StructLit { fields, .. } => build::struct_lit(
            fields.into_iter().map(|f| StructLitField { value: unwrap_elem_refs(f.value, elem), ..f }).collect(),
        ),
        SqlExpr::StructGet { source, field, .. } => build::struct_get(unwrap_elem_refs(*source, elem), &field),
        SqlExpr::Lambda(l) => {
            if l.params.iter().any(|p| p == elem) {
                SqlExpr::Lambda(l)
            } else {
                SqlExpr::Lambda(build::lambda_of(l.params, unwrap_elem_refs(*l.body, elem)))
            }
        }
        SqlExpr::FoldCall { source, lambda, init, acc_is_list, homogeneous, .. } => {
            let lambda = if lambda.params.iter().any(|p| p == elem) {
                lambda
            } else {
                build::lambda_of(lambda.params, unwrap_elem_refs(*lambda.body, elem))
            };
            build::fold_call(
                unwrap_elem_refs(*source, elem),
                lambda,
                unwrap_elem_refs(*init, elem),
                acc_is_list,
                homogeneous,
            )
        }
        SqlExpr::Group { inner, .. } => build::group(unwrap_elem_refs(*inner, elem)),
        SqlExpr::CheckedOne { list, scalar_carrier, at_least_only, .. } => {
            build::checked_one(unwrap_elem_refs(*list, elem), scalar_carrier, at_least_only)
        }
        SqlExpr::CompactList { list, .. } => build::compact_list(unwrap_elem_refs(*list, elem)),
        // leaves and structures that cannot hold the element's reference (a subquery's is a known hole in Java)
        other => other,
    }
}

// ==================================================================
// CheckedDefectsToLists
// ==================================================================

/// DuckDB's spelling of the checked-envelope nodes as list lambdas (Java's `CheckedDefectsToLists`).
#[derive(Debug, Default)]
pub struct CheckedDefectsToLists;

fn variant_get(v: SqlExpr, key: &str) -> SqlExpr {
    call(SqlFn::VariantGet, vec![v, string_lit(key)])
}

fn elements(v: SqlExpr) -> SqlExpr {
    call(SqlFn::VariantElements, vec![v])
}

fn hoisted(h: Hoist) -> SqlExpr {
    if !h.to_many {
        return call(
            SqlFn::Coalesce,
            vec![
                call(
                    SqlFn::ListTransform,
                    vec![
                        elements(variant_get(h.envelope, "defects")),
                        build::lambda(&["d"], prefixed(build::derived(None, "d"), &h.path, build::null_lit())),
                    ],
                ),
                build::array_lit(Vec::new()),
            ],
        );
    }
    let index = call(SqlFn::Minus, vec![build::derived(None, "i"), int_lit(1)]);
    call(
        SqlFn::ListFlatten,
        vec![call(
            SqlFn::ListTransform,
            vec![
                elements(h.envelope),
                build::lambda(
                    &["e", "i"],
                    call(
                        SqlFn::ListTransform,
                        vec![
                            elements(variant_get(build::derived(None, "e"), "defects")),
                            build::lambda(&["d"], prefixed(build::derived(None, "d"), &h.path, index)),
                        ],
                    ),
                ),
            ],
        )],
    )
}

fn prefixed(d: SqlExpr, path: &[String], index: SqlExpr) -> SqlExpr {
    let mut nodes = Vec::with_capacity(path.len());
    for (i, p) in path.iter().enumerate() {
        nodes.push(build::json_object(vec![
            string_lit("propertyName"),
            string_lit(p),
            string_lit("index"),
            if i == path.len() - 1 { index.clone() } else { build::null_lit() },
        ]));
    }
    let full = call(
        SqlFn::ToVariant,
        vec![call(SqlFn::ListConcat, vec![build::array_lit(nodes), elements(variant_get(d.clone(), "path"))])],
    );
    call(SqlFn::JsonMergePatch, vec![d, build::json_object(vec![string_lit("path"), full])])
}

impl SqlRewriter for CheckedDefectsToLists {
    fn name(&self) -> &'static str {
        "CheckedDefectsToLists"
    }

    fn expr(&mut self, e: SqlExpr) -> Result<Rw<SqlExpr>, Refusal> {
        match e {
            SqlExpr::CheckedChildValue { envelope, to_many, .. } => {
                if !to_many {
                    return Ok(Rw::New(variant_get(*envelope, "value")));
                }
                Ok(Rw::New(call(
                    SqlFn::ToVariant,
                    vec![call(
                        SqlFn::ListTransform,
                        vec![
                            elements(*envelope),
                            build::lambda(&["e"], variant_get(build::derived(None, "e"), "value")),
                        ],
                    )],
                )))
            }
            SqlExpr::CheckedDefects { own, hoists, .. } => {
                let mut all = *own;
                for h in hoists {
                    all = call(SqlFn::ListConcat, vec![all, hoisted(h)]);
                }
                Ok(Rw::New(all))
            }
            other => Ok(Rw::Same(other)),
        }
    }
}

// ==================================================================
// SubstringClamp
// ==================================================================

/// DuckDB's SUBSTRING start clamp (Java's `SubstringClamp`): a literal start below 1 is 1, a computed one
/// `greatest(start, 1)`.
#[derive(Debug, Default)]
pub struct SubstringClamp;

impl SqlRewriter for SubstringClamp {
    fn name(&self) -> &'static str {
        "SubstringClamp"
    }

    fn expr(&mut self, e: SqlExpr) -> Result<Rw<SqlExpr>, Refusal> {
        let SqlExpr::Call { r#fn: SqlFn::Substring, args, r#type } = e else {
            return Ok(Rw::Same(e));
        };
        if args.len() < 2 || matches!(args[1], SqlExpr::IntLit { value, .. } if value >= 1) {
            return Ok(Rw::Same(SqlExpr::Call { r#fn: SqlFn::Substring, args, r#type }));
        }
        let mut a2 = args;
        let st = std::mem::replace(&mut a2[1], build::null_lit());
        a2[1] = match st {
            SqlExpr::IntLit { .. } => int_lit(1),
            st => call(SqlFn::Greatest, vec![st, int_lit(1)]),
        };
        Ok(Rw::New(call(SqlFn::Substring, a2)))
    }
}

// ==================================================================
// ConstantKeysAsExpressions
// ==================================================================

/// A constant GROUP BY (or ORDER BY) key rendered as a typed expression (Java's `ConstantKeysAsExpressions`);
/// DuckDB's runs over GROUP BY's integers alone ([`ConstantKeysAsExpressions::group_by_only`]).
#[derive(Debug)]
pub struct ConstantKeysAsExpressions {
    /// Java's `orderToo`: every constant key of GROUP BY and ORDER BY, not GROUP BY's integers alone.
    pub order_too: bool,
    /// Java's `database`: the database a refusal names.
    pub database: &'static str,
}

impl Default for ConstantKeysAsExpressions {
    /// Java's no-argument constructor: Postgres's.
    fn default() -> ConstantKeysAsExpressions {
        ConstantKeysAsExpressions { order_too: true, database: "Postgres" }
    }
}

impl ConstantKeysAsExpressions {
    /// Java's `groupByOnly()`: DuckDB's.
    pub fn group_by_only() -> ConstantKeysAsExpressions {
        ConstantKeysAsExpressions { order_too: false, database: "DuckDB" }
    }

    /// A literal key as `CAST(<lit> AS <its own type>)`; any other key as it is.
    fn typed_key(&self, e: SqlExpr, clause: &str) -> Result<Rw<SqlExpr>, Refusal> {
        if matches!(e, SqlExpr::NullLit { .. }) && clause == "GROUP BY" {
            return Ok(Rw::New(cast(e, SqlType::Scalar(Scalar::Integer))));
        }
        if matches!(e, SqlExpr::NullLit { .. }) {
            return Err(Refusal::capability(format!(
                "a NULL {clause} key reached {}, which refuses a constant key and has no type to cast it to",
                self.database
            )));
        }
        let constant = matches!(
            e,
            SqlExpr::StringLit { .. }
                | SqlExpr::IntLit { .. }
                | SqlExpr::FloatLit { .. }
                | SqlExpr::DecimalLit { .. }
                | SqlExpr::BoolLit { .. }
        );
        if !constant {
            return Ok(Rw::Same(e));
        }
        let TypeFact::Typed { r#type, .. } = e.r#type() else {
            return Err(Refusal::capability(format!(
                "an untyped constant {clause} key reached {}, which reads a constant key otherwise and has no type to \
                 cast it to",
                self.database
            )));
        };
        let t = r#type.clone();
        Ok(Rw::New(cast(e, t)))
    }
}

impl SqlRewriter for ConstantKeysAsExpressions {
    fn name(&self) -> &'static str {
        "ConstantKeysAsExpressions"
    }

    fn select(&mut self, s: SqlSelect) -> Result<Rw<SqlQuery>, Refusal> {
        let mut changed = false;
        let mut s = s;
        let group = std::mem::take(&mut s.group_by);
        let mut g = Vec::with_capacity(group.len());
        for e in group {
            let k = if self.order_too || matches!(e, SqlExpr::IntLit { .. }) {
                self.typed_key(e, "GROUP BY")?
            } else {
                Rw::Same(e)
            };
            g.push(k.take(&mut changed));
        }
        let order = std::mem::take(&mut s.order_by);
        let mut ob = Vec::with_capacity(order.len());
        for k in order {
            let e = if self.order_too { self.typed_key(k.expr, "ORDER BY")? } else { Rw::Same(k.expr) };
            ob.push(SortKey { expr: e.take(&mut changed), ..k });
        }
        let s = SqlSelect { group_by: g, order_by: ob, ..s };
        if !changed {
            return Ok(Rw::Same(build::query(s)));
        }
        Ok(Rw::New(build::query(build::select(s)?)))
    }
}

// ==================================================================
// OuterJoinSubqueryFilters
// ==================================================================

/// A LEFT JOIN condition's conjunct that holds a scalar subquery and reads no column of the left side filters the
/// right side before the join (Java's `OuterJoinSubqueryFilters`): DuckDB cannot plan a scalar subquery in an outer
/// join's condition.
#[derive(Debug, Default)]
pub struct OuterJoinSubqueryFilters;

fn moved(source: SqlSource) -> Result<Rw<SqlSource>, Refusal> {
    let SqlSource::Join { left, right, kind, on, outputs } = source else {
        return Ok(Rw::Same(source));
    };
    let left = moved(*left)?;
    let right = moved(*right)?;
    if kind == JoinKind::Left
        && let Some(on_expr) = &on
        && matches!(right.get(), SqlSource::Table { .. } | SqlSource::Subselect { .. })
    {
        let mut left_aliases = HashSet::new();
        aliases(left.get(), &mut left_aliases)?;
        let mut conjuncts = Vec::new();
        collect_conjuncts(on_expr, &mut conjuncts);
        let mut kept = Vec::new();
        let mut filters = Vec::new();
        for c in conjuncts {
            if holds_scalar_subquery(c)? && !reads_any(c, &left_aliases)? {
                filters.push(c.clone());
            } else {
                kept.push(c.clone());
            }
        }
        if !filters.is_empty() && !kept.is_empty() {
            let right = right.into_inner();
            let alias = source_alias(&right)?.to_owned();
            let outputs = source_outputs(&right).to_vec();
            let filtered = build::select(SqlSelect {
                projections: vec![build::projection(build::star(None), None, None)?],
                distinct: false,
                from: right,
                r#where: Some(and(filters)),
                group_by: Vec::new(),
                having: None,
                qualify: None,
                order_by: Vec::new(),
                limit: None,
                offset: None,
                outputs,
            })?;
            let sub = build::subselect(build::query(filtered), &alias, None);
            return Ok(Rw::New(build::join(left.into_inner(), sub, kind, Some(and(kept)))?));
        }
    }
    if !left.is_new() && !right.is_new() {
        return Ok(Rw::Same(SqlSource::Join {
            left: Box::new(left.into_inner()),
            right: Box::new(right.into_inner()),
            kind,
            on,
            outputs,
        }));
    }
    Ok(Rw::New(build::join(left.into_inner(), right.into_inner(), kind, on.map(|o| *o))?))
}

fn aliases(s: &SqlSource, out: &mut HashSet<String>) -> Result<(), Refusal> {
    if let SqlSource::Join { left, right, .. } = s {
        aliases(left, out)?;
        return aliases(right, out);
    }
    out.insert(source_alias(s)?.to_owned());
    Ok(())
}

fn collect_conjuncts<'a>(e: &'a SqlExpr, out: &mut Vec<&'a SqlExpr>) {
    match e {
        SqlExpr::Call { r#fn: SqlFn::And, args, .. } => args.iter().for_each(|a| collect_conjuncts(a, out)),
        SqlExpr::Group { inner, .. } => collect_conjuncts(inner, out),
        _ => out.push(e),
    }
}

fn and(parts: Vec<SqlExpr>) -> SqlExpr {
    let mut it = parts.into_iter();
    let mut out = it.next().expect("at least one conjunct");
    for p in it {
        out = call(SqlFn::And, vec![out, p]);
    }
    out
}

/// An expression walk with the rewriter's own traversal (post-children hooks, into subqueries), its hook reading.
struct Find<F: FnMut(&SqlExpr) -> bool>(F, bool);

impl<F: FnMut(&SqlExpr) -> bool> SqlRewriter for Find<F> {
    fn name(&self) -> &'static str {
        "OuterJoinSubqueryFilters.Walk"
    }

    fn expr(&mut self, x: SqlExpr) -> Result<Rw<SqlExpr>, Refusal> {
        if (self.0)(&x) {
            self.1 = true;
        }
        Ok(Rw::Same(x))
    }
}

fn holds_scalar_subquery(e: &SqlExpr) -> Result<bool, Refusal> {
    let mut f = Find(|x: &SqlExpr| matches!(x, SqlExpr::ScalarSubquery { .. }), false);
    rewrite_expr(&mut f, e.clone())?;
    Ok(f.1)
}

/// Whether `e` reads a column of one of `aliases`, or an unqualified one, anywhere the walk reaches.
fn reads_any(e: &SqlExpr, aliases: &HashSet<String>) -> Result<bool, Refusal> {
    let mut f = Find(
        |x: &SqlExpr| matches!(x, SqlExpr::Column(c) if c.table.as_ref().is_none_or(|t| aliases.contains(t))),
        false,
    );
    rewrite_expr(&mut f, e.clone())?;
    Ok(f.1)
}

impl SqlRewriter for OuterJoinSubqueryFilters {
    fn name(&self) -> &'static str {
        "OuterJoinSubqueryFilters"
    }

    fn select(&mut self, s: SqlSelect) -> Result<Rw<SqlQuery>, Refusal> {
        let mut s = s;
        let from = std::mem::replace(&mut s.from, SqlSource::Dual);
        match moved(from)? {
            Rw::Same(from) => Ok(Rw::Same(build::query(SqlSelect { from, ..s }))),
            Rw::New(from) => Ok(Rw::New(build::query(build::select(SqlSelect { from, ..s })?))),
        }
    }
}

// ==================================================================
// Stars, VariantEgress, StoredReads
// ==================================================================

/// A star spelled out (Java's `Stars`): the columns `*`, `t.*` or `t.* EXCLUDE (...)` stand for, from the sources a
/// select's FROM names directly, in FROM order.
pub struct Stars;

impl Stars {
    /// The sources a select's FROM names directly, in order: through joins, not into them.
    pub fn sources(from: &SqlSource) -> Vec<&SqlSource> {
        let mut out = Vec::new();
        collect_sources(from, &mut out);
        out
    }

    /// The columns `*` (`table` none) or `table.*` stands for, less `except`, each through `reference`.
    pub fn columns(
        leaves: &[&SqlSource],
        table: Option<&str>,
        except: &[String],
        reference: &mut dyn FnMut(SqlExpr) -> SqlExpr,
    ) -> Result<Vec<Projection>, Refusal> {
        let mut out = Vec::new();
        for l in leaves {
            if matches!(l, SqlSource::Dual) {
                continue;
            }
            let alias = source_alias(l)?;
            if table.is_some_and(|t| t != alias) {
                continue;
            }
            for oc in source_outputs(l) {
                if !except.contains(&oc.name) {
                    let c = SqlExpr::Column(build::column_of(Some(alias), oc));
                    out.push(Projection { expr: reference(c), alias: None, out: None });
                }
            }
        }
        Ok(out)
    }
}

fn collect_sources<'a>(s: &'a SqlSource, out: &mut Vec<&'a SqlSource>) {
    if let SqlSource::Join { left, right, .. } = s {
        collect_sources(left, out);
        collect_sources(right, out);
    } else {
        out.push(s);
    }
}

/// At the statement's root, a star over a source holding a stored Variant column is spelled into its columns
/// (Java's `VariantEgress`), so the dialect's column reference reads it as JSON; a union's branches and a WITH's
/// body are roots too. Root only: no deep walk.
#[derive(Debug, Default)]
pub struct VariantEgress;

fn holds_stored_variant(leaves: &[&SqlSource], table: Option<&str>) -> Result<bool, Refusal> {
    for l in leaves {
        if matches!(l, SqlSource::Dual) {
            continue;
        }
        if let Some(t) = table
            && t != source_alias(l)?
        {
            continue;
        }
        if source_outputs(l).iter().any(stored_variant) {
            return Ok(true);
        }
    }
    Ok(false)
}

/// An output column born at the store, typed JSON.
fn stored_variant(o: &OutputCol) -> bool {
    matches!(o.origin, Origin::Physical | Origin::PhysicalQuoted) && o.r#type == SqlType::Scalar(Scalar::Json)
}

fn variant_spelled(s: SqlSelect) -> Result<SqlSelect, Refusal> {
    let leaves = Stars::sources(&s.from);
    if s.projections.is_empty() {
        if !holds_stored_variant(&leaves, None)? {
            return Ok(s);
        }
        let ps = Stars::columns(&leaves, None, &[], &mut |e| e)?;
        return build::with_projections(s, ps);
    }
    let mut ps = Vec::with_capacity(s.projections.len());
    let mut any = false;
    for p in &s.projections {
        let (table, except) = match &p.expr {
            SqlExpr::Star { table, .. } => (table.as_deref(), &[][..]),
            SqlExpr::StarExcept { table, except, .. } => (table.as_deref(), &except[..]),
            _ => {
                ps.push(p.clone());
                continue;
            }
        };
        if holds_stored_variant(&leaves, table)? {
            ps.extend(Stars::columns(&leaves, table, except, &mut |e| e)?);
            any = true;
        } else {
            ps.push(p.clone());
        }
    }
    if any { build::with_projections(s, ps) } else { Ok(s) }
}

impl SqlRewriter for VariantEgress {
    fn name(&self) -> &'static str {
        "VariantEgress"
    }

    fn rewrite_root(&mut self, q: SqlQuery) -> Result<SqlQuery, Refusal> {
        match q {
            SqlQuery::Select(s) => Ok(build::query(variant_spelled(*s)?)),
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

/// The stored reads (Java's `StoredReads`), registered last on every dialect: a table column whose declared store
/// type the dialect reads other than as held (`reads`) is read so at every reference, and a star over its table is
/// spelled out so the read reaches the columns it carries.
#[derive(Debug)]
pub struct StoredReads {
    /// Java's `reads`: the dialect's `readsStored`.
    pub reads: fn(&ColumnType) -> bool,
    /// Java's `read`: a scanned table's alias -> its columns this dialect reads, in Java's `HashMap` order, each with
    /// its stored type.
    read: HashMap<String, Vec<(String, ColumnType)>>,
}

impl StoredReads {
    pub fn new(reads: fn(&ColumnType) -> bool) -> StoredReads {
        StoredReads { reads, read: HashMap::new() }
    }

    fn stored(&self, table: &str, name: &str) -> Option<&ColumnType> {
        self.read.get(table)?.iter().find(|(n, _)| n == name).map(|(_, t)| t)
    }

    /// Java's `this::expr` as a star's column reference.
    fn read_ref(&self, e: SqlExpr) -> SqlExpr {
        match e {
            SqlExpr::Column(c) => match c.table.as_deref().and_then(|t| self.stored(t, &c.name)).cloned() {
                Some(t) => build::stored_read(c, t),
                None => SqlExpr::Column(c),
            },
            other => other,
        }
    }

    fn star_over(
        &self,
        leaves: &[&SqlSource],
        table: Option<&str>,
        except: &[String],
    ) -> Result<Option<Vec<Projection>>, Refusal> {
        let reaches = match table {
            None => leaves.iter().any(|l| matches!(l, SqlSource::Table { alias, .. } if self.read.contains_key(alias))),
            Some(t) => self.read.contains_key(t),
        };
        if !reaches {
            return Ok(None);
        }
        Ok(Some(Stars::columns(leaves, table, except, &mut |e| self.read_ref(e))?))
    }
}

/// The collecting walk of `StoredReads.rewriteRoot`: every scanned table's read columns.
struct ReadTables<'a> {
    reads: fn(&ColumnType) -> bool,
    read: &'a mut HashMap<String, Vec<(String, ColumnType)>>,
}

impl SqlRewriter for ReadTables<'_> {
    fn name(&self) -> &'static str {
        "StoredReads.collect"
    }

    fn source(&mut self, s: SqlSource) -> Result<Rw<SqlSource>, Refusal> {
        if let SqlSource::Table { alias, stored_types, .. } = &s {
            let cols: Vec<(String, ColumnType)> =
                stored_types.iter().filter(|(_, t)| (self.reads)(t)).cloned().collect();
            if !cols.is_empty() {
                // Java's `new LinkedHashMap<>(cols)` of a HashMap: the HashMap's order
                let names: Vec<&str> = cols.iter().map(|(n, _)| n.as_str()).collect();
                let ordered = crate::java::hash_map_order(&names)
                    .into_iter()
                    .map(|n| cols.iter().find(|(c, _)| c == n).expect("a collected name").clone())
                    .collect();
                self.read.insert(alias.clone(), ordered);
            }
        }
        Ok(Rw::Same(s))
    }
}

/// Refuses, by name, an unqualified physical reference in a select's own clauses to a read column of a table in its
/// FROM.
fn refuse_unqualified_select(s: &SqlSelect, read_names: &HashMap<String, String>) -> Result<(), Refusal> {
    let mut own: Vec<&SqlExpr> = s.projections.iter().map(|p| &p.expr).collect();
    own.extend(s.r#where.iter());
    own.extend(s.group_by.iter());
    own.extend(s.having.iter());
    own.extend(s.qualify.iter());
    own.extend(s.order_by.iter().map(|k| &k.expr));
    join_conditions(&s.from, &mut own);
    own.into_iter().try_for_each(|e| refuse_unqualified(e, read_names))
}

fn refuse_unqualified(e: &SqlExpr, read_names: &HashMap<String, String>) -> Result<(), Refusal> {
    if let SqlExpr::Column(c) = e
        && c.table.is_none()
        && c.origin != Some(Origin::Derived)
        && let Some(table) = read_names.get(&c.name)
    {
        return Err(Refusal::capability(format!(
            "an unqualified reference to column '{}' of table '{table}' cannot be read by its declared type",
            c.name
        )));
    }
    children(e).iter().try_for_each(|c| refuse_unqualified(c, read_names))
}

fn join_conditions<'a>(s: &'a SqlSource, out: &mut Vec<&'a SqlExpr>) {
    if let SqlSource::Join { left, right, on, .. } = s {
        join_conditions(left, out);
        join_conditions(right, out);
        if let Some(on) = on {
            out.push(on);
        }
    }
}

impl SqlRewriter for StoredReads {
    fn name(&self) -> &'static str {
        "StoredReads"
    }

    fn rewrite_root(&mut self, q: SqlQuery) -> Result<SqlQuery, Refusal> {
        self.read.clear();
        let mut collect = ReadTables { reads: self.reads, read: &mut self.read };
        let q = rewrite(&mut collect, q)?.into_inner();
        if self.read.is_empty() {
            return Ok(q);
        }
        Ok(rewrite(self, q)?.into_inner())
    }

    fn expr(&mut self, e: SqlExpr) -> Result<Rw<SqlExpr>, Refusal> {
        if let SqlExpr::Column(c) = &e
            && let Some(table) = &c.table
            && self.stored(table, &c.name).is_some()
        {
            return Ok(Rw::New(self.read_ref(e)));
        }
        Ok(Rw::Same(e))
    }

    fn source(&mut self, s: SqlSource) -> Result<Rw<SqlSource>, Refusal> {
        if let SqlSource::Pivot { source, .. } = &s {
            for t in Stars::sources(source) {
                if let SqlSource::Table { name, alias, .. } = t
                    && let Some(cols) = self.read.get(alias)
                {
                    let names: Vec<&str> = cols.iter().map(|(n, _)| n.as_str()).collect();
                    return Err(Refusal::capability(format!(
                        "a PIVOT straight over table '{name}' cannot read its columns [{}] by their declared types",
                        names.join(", ")
                    )));
                }
            }
        }
        Ok(Rw::Same(s))
    }

    fn select(&mut self, s: SqlSelect) -> Result<Rw<SqlQuery>, Refusal> {
        let leaves = Stars::sources(&s.from);
        let mut unqualified: HashMap<String, String> = HashMap::new();
        let mut any = false;
        for l in &leaves {
            if let SqlSource::Table { name, alias, .. } = l
                && let Some(cols) = self.read.get(alias)
            {
                any = true;
                for (n, _) in cols {
                    unqualified.insert(n.clone(), name.clone());
                }
            }
        }
        if !any {
            return Ok(Rw::Same(build::query(s)));
        }
        refuse_unqualified_select(&s, &unqualified)?;
        if s.projections.is_empty() {
            let ps = Stars::columns(&leaves, None, &[], &mut |e| self.read_ref(e))?;
            return Ok(Rw::New(build::query(build::select(SqlSelect { projections: ps, ..s })?)));
        }
        let mut ps = Vec::new();
        let mut expanded = false;
        for p in &s.projections {
            let spelled = match &p.expr {
                SqlExpr::Star { table, .. } => self.star_over(&leaves, table.as_deref(), &[])?,
                SqlExpr::StarExcept { table, except, .. } => self.star_over(&leaves, table.as_deref(), except)?,
                _ => None,
            };
            match spelled {
                None => ps.push(p.clone()),
                Some(cols) => {
                    ps.extend(cols);
                    expanded = true;
                }
            }
        }
        if !expanded {
            return Ok(Rw::Same(build::query(s)));
        }
        Ok(Rw::New(build::query(build::select(SqlSelect { projections: ps, ..s })?)))
    }
}

/// DuckDB's `passes()` (Java's `DuckDb.passes`, line 675): carrier strategies first, then its structural rewrites,
/// `VariantEgress` last of them (`StoredReads` follows, in `render_passes`).
pub fn duckdb_passes(engine_scan_order: bool) -> Vec<Box<dyn SqlRewriter>> {
    let mut ps: Vec<Box<dyn SqlRewriter>> = vec![
        Box::new(CarrierStrategies::new(crate::duckdb::CAPS)),
        Box::new(QuantileOrder),
        Box::new(UnqualifyPivotArgs),
        Box::new(FoldToListReduce),
        Box::new(CheckedDefectsToLists),
        Box::new(SubstringClamp),
        Box::new(ConstantKeysAsExpressions::group_by_only()),
        // a LEFT JOIN condition's scalar subquery of the null-supplying side filters that side before the join
        Box::new(OuterJoinSubqueryFilters),
    ];
    if engine_scan_order {
        ps.push(Box::new(StableScanOrder::new()));
    }
    // last of this dialect's passes, on the statement root only: a stored Variant leaving through a star read as JSON
    ps.push(Box::new(VariantEgress));
    ps
}
