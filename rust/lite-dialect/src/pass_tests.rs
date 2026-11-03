// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! The passes, the shared walk and the typing rules held to their Java classes' own logic, one hand-built tree at a
//! time: each pass's rewrite, the identity Java's `==` keeps (a parent rebuilt only when a child changed), and the
//! facts Java's constructors compute. The golden dump holds the passes over real statements
//! (`tests/dialect_differential.rs`); `CarrierStrategies` and `StableScanOrder` pin theirs in their own modules.

use crate::build::{self, call, cast, int_lit, string_lit};
use crate::passes::{
    CheckedDefectsToLists, ConstantKeysAsExpressions, FloatCanonExpansion, FloatPrintExpansion, FoldToListReduce,
    OuterJoinSubqueryFilters, QuantileOrder, StoredReads, SubstringClamp, UnqualifyPivotArgs, VariantEgress,
};
use crate::rewriter::{Identity, Rw, SqlRewriter, rewrite, rewrite_expr};
use crate::typing::{self, scalar};
use crate::{DuckDb, Refusal, SqlDialect};
use lite_sql::{
    AggFn, BigDecimal, Column, ColumnKind, ColumnType, Hoist, JoinKind, NameQuoting, Origin, OutputCol, PivotUsing,
    Projection, Scalar, SortKey, SqlAgg, SqlExpr, SqlFn, SqlQuery, SqlSelect, SqlSource, SqlType, TypeFact,
};

// ---- builders

fn t(s: Scalar) -> SqlType {
    SqlType::Scalar(s)
}

fn fact(s: Scalar, nullable: bool) -> TypeFact {
    TypeFact::Typed { r#type: t(s), nullable }
}

fn column(table: Option<&str>, name: &str, s: Scalar, nullable: bool) -> Column {
    Column {
        table: table.map(str::to_owned),
        name: name.to_owned(),
        r#type: fact(s, nullable),
        origin: Some(Origin::Physical),
    }
}

fn col(table: &str, name: &str, s: Scalar) -> SqlExpr {
    SqlExpr::Column(column(Some(table), name, s, true))
}

fn out(name: &str, s: Scalar, origin: Origin) -> OutputCol {
    OutputCol { name: name.to_owned(), r#type: t(s), nullable: true, origin }
}

fn table(name: &str, alias: &str, outputs: Vec<OutputCol>, stored: Vec<(&str, ColumnType)>) -> SqlSource {
    SqlSource::Table {
        name: name.to_owned(),
        alias: alias.to_owned(),
        outputs,
        call: false,
        stored_types: stored.into_iter().map(|(n, ct)| (n.to_owned(), ct)).collect(),
        quoting: NameQuoting::default(),
    }
}

fn proj(e: SqlExpr, alias: &str, s: Scalar) -> Projection {
    build::projection(e, Some(alias.to_owned()), Some(build::output_col(alias, t(s), true))).unwrap()
}

fn bare(e: SqlExpr) -> Projection {
    Projection { expr: e, alias: None, out: None }
}

fn sel(projections: Vec<Projection>, from: SqlSource) -> SqlSelect {
    build::select_of(projections, from, Vec::new()).unwrap()
}

fn q(s: SqlSelect) -> SqlQuery {
    build::query(s)
}

fn select_of(q: &SqlQuery) -> &SqlSelect {
    match q {
        SqlQuery::Select(s) => s,
        other => panic!("not a select: {other:?}"),
    }
}

fn fn_of(e: &SqlExpr) -> SqlFn {
    match e {
        SqlExpr::Call { r#fn, .. } => *r#fn,
        other => panic!("not a call: {other:?}"),
    }
}

fn args_of(e: &SqlExpr) -> &[SqlExpr] {
    match e {
        SqlExpr::Call { args, .. } => args,
        other => panic!("not a call: {other:?}"),
    }
}

/// `e` through `pass`'s expression walk.
fn walk<R: SqlRewriter>(pass: &mut R, e: SqlExpr) -> Rw<SqlExpr> {
    rewrite_expr(pass, e).unwrap()
}

fn other() -> ColumnType {
    ColumnType::Plain { kind: ColumnKind::Other }
}

// ==================================================================
// the walk: Java's identity and its constructors' facts
// ==================================================================

#[test]
fn an_untouched_tree_is_handed_back_as_it_came() {
    let s = sel(
        vec![proj(call(SqlFn::Upper, vec![col("t0", "NAME", Scalar::Varchar)]), "n", Scalar::Varchar)],
        table("T", "t0", vec![out("NAME", Scalar::Varchar, Origin::Physical)], Vec::new()),
    );
    let before = q(s);
    let after = rewrite(&mut Identity, before.clone()).unwrap();
    assert!(!after.is_new());
    assert_eq!(after.into_inner(), before);
}

#[test]
fn a_parent_is_rebuilt_only_where_a_child_changed_and_its_fact_recomputed() {
    // two stale facts Java's constructors would never store: the walk recomputes the changed branch's parent alone
    let stale = |f: SqlFn, args: Vec<SqlExpr>| SqlExpr::Call { r#fn: f, args, r#type: TypeFact::Unknown };
    let clamped = stale(SqlFn::Substring, vec![string_lit("abc"), int_lit(0), int_lit(2)]);
    let kept = stale(SqlFn::Substring, vec![string_lit("abc"), int_lit(1), int_lit(2)]);
    let e = stale(SqlFn::Concat, vec![clamped, stale(SqlFn::Upper, vec![kept])]);
    let r = walk(&mut SubstringClamp, e);
    assert!(r.is_new());
    let e = r.into_inner();
    // CONCAT rebuilt (VARCHAR, never NULL: concat skips NULL arguments)
    assert_eq!(e.r#type(), &fact(Scalar::Varchar, false));
    let a = args_of(&e);
    assert_eq!(args_of(&a[0])[1], int_lit(1));
    assert_eq!(a[0].r#type(), &fact(Scalar::Varchar, false));
    // the UPPER branch was not touched: its stale fact is as it came (Java hands the very node back)
    assert_eq!(a[1].r#type(), &TypeFact::Unknown);
}

#[test]
fn a_rebuilt_lambda_loses_its_fact_and_an_array_literal_its_given_one() {
    let lam = SqlExpr::Lambda(lite_sql::Lambda {
        params: vec!["x".to_owned()],
        body: Box::new(call(SqlFn::Substring, vec![build::derived(None, "x"), int_lit(-1)])),
        r#type: fact(Scalar::Varchar, true),
    });
    let given = TypeFact::Typed { r#type: SqlType::Array { element: Box::new(t(Scalar::Varchar)) }, nullable: false };
    let arr = SqlExpr::ArrayLit { elements: vec![lam], r#type: given };
    let e = walk(&mut SubstringClamp, arr).into_inner();
    let SqlExpr::ArrayLit { elements, r#type } = &e else { panic!("{e:?}") };
    // Java's `new ArrayLit(es)`: the one-argument constructor, the given type gone (a lambda proves none)
    assert_eq!(r#type, &TypeFact::Unknown);
    // Java's `new Lambda(params, body)`: the two-argument constructor, the fact UNKNOWN
    assert_eq!(elements[0].r#type(), &TypeFact::Unknown);
}

#[test]
fn a_rebuilt_select_reconciles_its_outputs_and_a_rebuilt_join_its_columns() {
    let left = table("A", "a", vec![out("X", Scalar::Bigint, Origin::Physical)], Vec::new());
    let right =
        table("B", "b", vec![OutputCol { nullable: false, ..out("Y", Scalar::Varchar, Origin::Physical) }], Vec::new());
    let on = call(
        SqlFn::Equal,
        vec![col("a", "X", Scalar::Bigint), call(SqlFn::Substring, vec![col("b", "Y", Scalar::Varchar), int_lit(0)])],
    );
    let from = build::join(left, right, JoinKind::Left, Some(on)).unwrap();
    // the LEFT join pads its right side: Y weakens to nullable
    assert!(typing::source_outputs(&from)[1].nullable);
    let mut s = sel(vec![proj(build::row_order(None), "r", Scalar::Varchar)], from);
    // the declared label lies (VARCHAR for a BIGINT row order): Java's constructor adopts the wire
    assert_eq!(s.outputs[0].r#type, t(Scalar::Bigint));
    assert!(!s.outputs[0].nullable);
    s.outputs.clear();
    let r = rewrite(&mut SubstringClamp, q(s)).unwrap();
    assert!(r.is_new());
    let s = select_of(r.get());
    // the select was rebuilt (its join's ON changed): its outputs recomputed from the projections
    assert_eq!(s.outputs.len(), 1);
    let SqlSource::Join { outputs, .. } = &s.from else { panic!() };
    assert_eq!(outputs.len(), 2);
    assert!(outputs[1].nullable);
}

// ==================================================================
// the typing rules, from SqlTyping's own receipts
// ==================================================================

fn dec(text: &str) -> TypeFact {
    typing::decimal_lit_type(&BigDecimal::parse(text).unwrap())
}

fn decimal(p: i32, s: i32) -> TypeFact {
    typing::typed(SqlType::Decimal { precision: p, scale: s })
}

#[test]
fn a_decimal_literal_is_typed_as_its_emission() {
    // "0.99 -> DECIMAL(3,2), 0.5 -> DECIMAL(2,1), 12.345 -> DECIMAL(5,3)"
    assert_eq!(dec("0.99"), decimal(3, 2));
    assert_eq!(dec("0.5"), decimal(2, 1));
    assert_eq!(dec("12.345"), decimal(5, 3));
    // "17774 -> Decimal(p,0)", "1E+3 renders 1000", a value past long HUGEINT, past 128 bits DOUBLE
    assert_eq!(dec("17774"), decimal(5, 0));
    assert_eq!(dec("1E+3"), decimal(4, 0));
    assert_eq!(dec("9999999999999999999"), scalar(Scalar::Hugeint));
    assert_eq!(dec("-9223372036854775808"), decimal(19, 0));
    assert_eq!(dec("9223372036854775808"), scalar(Scalar::Hugeint));
    assert_eq!(dec("1000000000000000000000000000000000000000"), scalar(Scalar::Double));
    // 39 rendered digits with a fraction read DOUBLE
    assert_eq!(dec("1.00000000000000000000000000000000000000"), scalar(Scalar::Double));
    // a Float literal is typed by BigDecimal.valueOf: Double.toString's text
    assert_eq!(typing::float_lit_type("19.75"), decimal(4, 2));
    assert_eq!(build::float_lit(1e-17).r#type(), &decimal(19, 18));
}

#[test]
fn arithmetic_promotes_as_probed() {
    let lit = |p: i32, s: i32| SqlExpr::DecimalLit { value: BigDecimal::parse("1").unwrap(), r#type: decimal(p, s) };
    // "(18,0)+(18,0) -> (18,0)": the carry never crosses the int64 class
    assert_eq!(call(SqlFn::Plus, vec![lit(18, 0), lit(18, 0)]).r#type(), &decimal(18, 0));
    assert_eq!(call(SqlFn::Plus, vec![lit(5, 2), lit(3, 1)]).r#type(), &decimal(6, 2));
    assert_eq!(call(SqlFn::Times, vec![lit(5, 2), lit(3, 1)]).r#type(), &decimal(8, 3));
    // "DEC(3,1)%INT -> DEC(11,1)"
    let int = SqlExpr::Column(column(None, "i", Scalar::Integer, false));
    assert_eq!(
        call(SqlFn::Rem, vec![lit(3, 1), int.clone()]).r#type(),
        &TypeFact::Typed { r#type: SqlType::Decimal { precision: 11, scale: 1 }, nullable: true }
    );
    // any DOUBLE wins; all-integer the widest; a DATE plus an integer is a DATE
    let dbl = SqlExpr::Column(column(None, "d", Scalar::Double, false));
    assert_eq!(call(SqlFn::Plus, vec![int.clone(), dbl]).r#type(), &fact(Scalar::Double, false));
    assert_eq!(call(SqlFn::Plus, vec![int.clone(), int_lit(1)]).r#type(), &fact(Scalar::Bigint, false));
    let date = SqlExpr::Column(column(None, "dt", Scalar::Date, false));
    assert_eq!(call(SqlFn::Plus, vec![date, int]).r#type(), &fact(Scalar::Date, false));
    // NULL propagates
    assert_eq!(call(SqlFn::Minus, vec![int_lit(1), build::null_lit()]).r#type(), &TypeFact::Bottom);
}

#[test]
fn a_case_a_reducer_and_a_scalar_subquery_type_as_java_does() {
    let w = build::when(build::bool_lit(true), int_lit(1));
    // a missing ELSE is NULL
    assert_eq!(build::case(vec![w.clone()], None).r#type(), &fact(Scalar::Bigint, true));
    assert_eq!(build::case(vec![w.clone()], Some(int_lit(2))).r#type(), &fact(Scalar::Bigint, false));
    // integer and decimal branches unite without a carry digit: [Dec(2,1), BIGINT] -> Dec(20,1)
    let d = SqlExpr::DecimalLit { value: BigDecimal::parse("1.5").unwrap(), r#type: decimal(2, 1) };
    assert_eq!(build::case(vec![w], Some(d)).r#type(), &decimal(20, 1));
    // SUM widens an integer to HUGEINT, nullable at the node; COUNT is never NULL
    let x = SqlExpr::Column(column(None, "x", Scalar::Integer, false));
    assert_eq!(build::reducer(AggFn::Sum, vec![x.clone()], false, Vec::new()).r#type, fact(Scalar::Hugeint, true));
    assert_eq!(build::reducer(AggFn::Count, vec![x.clone()], false, Vec::new()).r#type, fact(Scalar::Bigint, false));
    // an ungrouped aggregate select is one row: its subquery's slot is the reducer's own
    let agg = SqlExpr::Reducer(build::reducer(AggFn::Count, vec![x], false, Vec::new()));
    let inner = sel(vec![proj(agg, "c", Scalar::Bigint)], SqlSource::Dual);
    assert_eq!(build::scalar_subquery(q(inner.clone())).r#type(), &fact(Scalar::Bigint, false));
    // a LIMIT 0 can drop the row: nullable
    let capped = build::with_limit(inner, Some(0)).unwrap();
    assert_eq!(build::scalar_subquery(q(capped)).r#type(), &fact(Scalar::Bigint, true));
}

#[test]
fn a_star_framed_join_under_a_strict_where_restores_its_pads() {
    let left =
        table("A", "a", vec![OutputCol { nullable: false, ..out("X", Scalar::Bigint, Origin::Physical) }], Vec::new());
    let right =
        table("B", "b", vec![OutputCol { nullable: false, ..out("Y", Scalar::Bigint, Origin::Physical) }], Vec::new());
    let on = call(SqlFn::Equal, vec![col("a", "X", Scalar::Bigint), col("b", "Y", Scalar::Bigint)]);
    let from = build::join(left, right, JoinKind::Left, Some(on)).unwrap();
    let outputs = typing::source_outputs(&from).to_vec();
    assert!(outputs[1].nullable);
    let strict = call(SqlFn::Greater, vec![col("b", "Y", Scalar::Bigint), int_lit(0)]);
    let s = build::select(SqlSelect { r#where: Some(strict), outputs, ..build::star_of(from.clone()).unwrap() });
    assert!(!s.unwrap().outputs[1].nullable);
    // an IS NULL test tolerates the pad: no restoration
    let tolerant = call(SqlFn::IsNull, vec![col("b", "Y", Scalar::Bigint)]);
    let outputs = typing::source_outputs(&from).to_vec();
    let s = build::select(SqlSelect { r#where: Some(tolerant), outputs, ..build::star_of(from).unwrap() });
    assert!(s.unwrap().outputs[1].nullable);
}

#[test]
fn a_java_hash_map_iterates_by_bucket() {
    // "Aa" and "BB" share String.hashCode 2112 (bucket 0): their insertion order holds; "c" (99) is bucket 3
    assert_eq!(crate::java::string_hash("Aa"), 2112);
    assert_eq!(crate::java::hash_map_order(&["c", "BB", "Aa"]), ["BB", "Aa", "c"]);
    assert_eq!(crate::java::hash_map_order(&["b", "a"]), ["a", "b"]);
    // thirteen keys pass 16 * 0.75: the table doubles to 32 and "q" (113 & 31 = 17) follows "a" (97 & 31 = 1)
    let keys = ["q", "a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l"];
    let order = crate::java::hash_map_order(&keys);
    assert_eq!(order[0], "a");
    assert_eq!(order[12], "q");
}

// ==================================================================
// the Float expansions
// ==================================================================

#[test]
fn a_float_canon_is_expanded_in_place_or_bound_once_per_equal_call() {
    let v = SqlExpr::Column(column(Some("t0"), "F", Scalar::Double, true));
    let canon = || call(SqlFn::FloatCanon, vec![v.clone()]);
    // DuckDB's: the spelling over CAST(v AS VARCHAR), in place
    let e = walk(&mut FloatCanonExpansion::new(false), canon()).into_inner();
    assert_eq!(e, crate::float_text::canon_of(v.clone()));
    assert!(matches!(e, SqlExpr::Case { .. }));
    // bound: (SELECT <spelling over __fc0.t> AS v FROM (SELECT CAST(v AS VARCHAR) AS t) AS __fc0), an equal call
    // the same node, another call the next alias
    let mut bound = FloatCanonExpansion::new(true);
    let first = walk(&mut bound, canon()).into_inner();
    let again = walk(&mut bound, canon()).into_inner();
    assert_eq!(first, again);
    let SqlExpr::ScalarSubquery { subquery, r#type } = &first else { panic!("{first:?}") };
    assert_eq!(r#type, &fact(Scalar::Varchar, true));
    let SqlSource::Subselect { alias, .. } = &select_of(subquery).from else { panic!() };
    assert_eq!(alias, "__fc0");
    let next = walk(&mut bound, call(SqlFn::FloatCanon, vec![int_lit(1)])).into_inner();
    let SqlExpr::ScalarSubquery { subquery, .. } = &next else { panic!() };
    let SqlSource::Subselect { alias, .. } = &select_of(subquery).from else { panic!() };
    assert_eq!(alias, "__fc1");
}

#[test]
fn a_float_print_binds_a_double_and_reads_any_other_value_in_place() {
    let dbl = SqlExpr::Column(column(Some("t0"), "F", Scalar::Double, true));
    let e = walk(&mut FloatPrintExpansion::new(false), call(SqlFn::FloatPrint, vec![dbl.clone()])).into_inner();
    assert_eq!(e, crate::float_text::print_of(dbl.clone()));
    let mut bound = FloatPrintExpansion::new(true);
    let e = walk(&mut bound, call(SqlFn::FloatPrint, vec![dbl])).into_inner();
    let SqlExpr::ScalarSubquery { subquery, .. } = &e else { panic!("{e:?}") };
    let outer = select_of(subquery);
    assert_eq!(outer.projections[0].alias.as_deref(), Some("p"));
    let SqlSource::Subselect { alias, inner, .. } = &outer.from else { panic!() };
    assert_eq!(alias, "__fp1");
    let SqlSource::Subselect { alias, .. } = &select_of(inner).from else { panic!() };
    assert_eq!(alias, "__fp0");
    // a literal is the database's own cast: in place, even when binding
    let lit = int_lit(3);
    let e = walk(&mut bound, call(SqlFn::FloatPrint, vec![lit.clone()])).into_inner();
    assert_eq!(e, crate::float_text::print_of(lit));
}

// ==================================================================
// SubstringClamp
// ==================================================================

#[test]
fn a_substring_start_is_clamped_to_one() {
    let x = col("t0", "CODE", Scalar::Varchar);
    let sub = |start: SqlExpr| call(SqlFn::Substring, vec![x.clone(), start, int_lit(2)]);
    // a start of 1 or more is the node itself
    assert!(!walk(&mut SubstringClamp, sub(int_lit(1))).is_new());
    // a literal below 1 is 1
    let r = walk(&mut SubstringClamp, sub(int_lit(0)));
    assert!(r.is_new());
    assert_eq!(r.into_inner(), sub(int_lit(1)));
    // a computed start is greatest(start, 1)
    let computed = call(SqlFn::Plus, vec![call(SqlFn::Strpos, vec![x.clone(), string_lit("-")]), int_lit(1)]);
    let e = walk(&mut SubstringClamp, sub(computed.clone())).into_inner();
    assert_eq!(args_of(&e)[1], call(SqlFn::Greatest, vec![computed, int_lit(1)]));
    // a one-argument call has no start
    assert!(!walk(&mut SubstringClamp, call(SqlFn::Substring, vec![x.clone()])).is_new());
    // end to end: DuckDB's text
    let s = sel(
        vec![proj(sub(int_lit(0)), "c", Scalar::Varchar)],
        table("T", "t0", vec![out("CODE", Scalar::Varchar, Origin::Physical)], Vec::new()),
    );
    assert!(DuckDb::new().render(q(s)).unwrap().contains("substr(t0.CODE, 1, 2)"));
}

// ==================================================================
// QuantileOrder
// ==================================================================

fn quantile(f: AggFn, v: SqlExpr, p: f64, order: Vec<SortKey>) -> lite_sql::Reducer {
    build::reducer(f, vec![v, build::float_lit(p)], false, order)
}

#[test]
fn a_discrete_percentile_picks_the_ceil_p_n_th_sorted_value() {
    let v = col("t0", "V", Scalar::Bigint);
    let e = walk(&mut QuantileOrder, SqlExpr::Reducer(quantile(AggFn::QuantileDisc, v.clone(), 0.6, Vec::new())));
    let e = e.into_inner();
    // list_get(list_sort(LIST(v)), greatest(CAST(ceil(CAST(p AS DOUBLE) * COUNT(v)) AS BIGINT), 1))
    assert_eq!(fn_of(&e), SqlFn::ListGet);
    let [sorted, index] = args_of(&e) else { panic!() };
    assert_eq!(fn_of(sorted), SqlFn::ListSort);
    assert!(matches!(&args_of(sorted)[0], SqlExpr::Reducer(r) if r.r#fn == AggFn::List));
    assert_eq!(fn_of(index), SqlFn::Greatest);
    assert_eq!(args_of(index)[1], int_lit(1));
    // descending sorts descending
    let desc = SortKey { expr: v.clone(), ascending: false, null_order: None, output_name: None };
    let e = walk(&mut QuantileOrder, SqlExpr::Reducer(quantile(AggFn::QuantileDisc, v.clone(), 0.6, vec![desc])));
    assert_eq!(fn_of(&args_of(e.get())[0]), SqlFn::ListSortDesc);
    // two order keys are no percentile the pass knows: as it came
    let k = SortKey { expr: v.clone(), ascending: true, null_order: None, output_name: None };
    let two = SqlExpr::Reducer(quantile(AggFn::QuantileDisc, v, 0.6, vec![k.clone(), k]));
    assert!(!walk(&mut QuantileOrder, two).is_new());
}

#[test]
fn a_continuous_percentile_is_interpolated_and_a_descending_one_negated() {
    let v = col("t0", "V", Scalar::Double);
    let e = walk(&mut QuantileOrder, SqlExpr::Reducer(quantile(AggFn::QuantileCont, v.clone(), 0.9, Vec::new())));
    // lo + (idx - floor(idx)) * (hi - lo)
    let e = e.into_inner();
    assert_eq!(fn_of(&e), SqlFn::Plus);
    assert_eq!(fn_of(&args_of(&e)[0]), SqlFn::ListGet);
    assert_eq!(fn_of(&args_of(&e)[1]), SqlFn::Times);
    let desc = SortKey { expr: v.clone(), ascending: false, null_order: None, output_name: None };
    let e = walk(&mut QuantileOrder, SqlExpr::Reducer(quantile(AggFn::QuantileCont, v, 0.9, vec![desc])));
    let e = e.into_inner();
    assert_eq!(fn_of(&e), SqlFn::Negate);
    assert_eq!(fn_of(&args_of(&e)[0]), SqlFn::Plus);
}

#[test]
fn a_windowed_percentile_windows_every_reducer_of_its_encoding() {
    let v = col("t0", "V", Scalar::Bigint);
    let part = vec![col("t0", "G", Scalar::Varchar)];
    let w = build::window_call(
        SqlAgg::Reducer(quantile(AggFn::QuantileDisc, v, 0.5, Vec::new())),
        part.clone(),
        Vec::new(),
        None,
    );
    let e = walk(&mut QuantileOrder, w).into_inner();
    let mut windows = 0;
    let mut bare_reducers = 0;
    fn count(e: &SqlExpr, windows: &mut usize, bare: &mut usize, part: &[SqlExpr]) {
        match e {
            SqlExpr::WindowCall { partition_by, .. } => {
                assert_eq!(partition_by, part);
                *windows += 1;
            }
            SqlExpr::Reducer(_) => *bare += 1,
            _ => typing::children(e).iter().for_each(|c| count(c, windows, bare, part)),
        }
    }
    count(&e, &mut windows, &mut bare_reducers, &part);
    assert_eq!((windows, bare_reducers), (2, 0));
}

// ==================================================================
// UnqualifyPivotArgs
// ==================================================================

#[test]
fn a_pivots_on_and_using_are_unqualified() {
    let src = table("S", "t4", vec![out("k", Scalar::Varchar, Origin::Physical)], Vec::new());
    let sum = build::reducer(AggFn::Sum, vec![col("t4", "qty", Scalar::Bigint)], false, Vec::new());
    let sub = build::scalar_subquery(q(sel(
        vec![proj(col("t9", "z", Scalar::Bigint), "z", Scalar::Bigint)],
        table("Z", "t9", vec![out("z", Scalar::Bigint, Origin::Physical)], Vec::new()),
    )));
    let pivot = SqlSource::Pivot {
        source: Box::new(src),
        on: vec![col("t4", "k", Scalar::Varchar), call(SqlFn::Coalesce, vec![sub.clone(), int_lit(0)])],
        r#in: Vec::new(),
        named: Vec::new(),
        usings: vec![PivotUsing { agg: sum, alias: "s".to_owned(), r#type: Some(t(Scalar::Hugeint)) }],
        alias: "p".to_owned(),
        outputs: Vec::new(),
    };
    let r = crate::rewriter::rewrite_source(&mut UnqualifyPivotArgs, pivot).unwrap();
    assert!(r.is_new());
    let SqlSource::Pivot { on, usings, .. } = r.into_inner() else { panic!() };
    assert!(matches!(&on[0], SqlExpr::Column(c) if c.table.is_none() && c.name == "k"));
    // a subquery owns its aliases: as it came
    assert_eq!(args_of(&on[1])[0], sub);
    assert!(matches!(&usings[0].agg.args[0], SqlExpr::Column(c) if c.table.is_none()));
    // every pivot is rebuilt, a select over it too
    let unchanged = SqlSource::Pivot {
        source: Box::new(table("S", "t4", Vec::new(), Vec::new())),
        on: Vec::new(),
        r#in: Vec::new(),
        named: Vec::new(),
        usings: Vec::new(),
        alias: "p".to_owned(),
        outputs: Vec::new(),
    };
    assert!(crate::rewriter::rewrite_source(&mut UnqualifyPivotArgs, unchanged).unwrap().is_new());
}

// ==================================================================
// FoldToListReduce
// ==================================================================

fn fold(body: SqlExpr, acc_is_list: bool, homogeneous: bool) -> SqlExpr {
    let source = SqlExpr::Column(Column {
        table: Some("t0".to_owned()),
        name: "L".to_owned(),
        r#type: TypeFact::Typed { r#type: SqlType::Array { element: Box::new(t(Scalar::Bigint)) }, nullable: true },
        origin: Some(Origin::Physical),
    });
    let lambda = build::lambda_of(vec!["e".to_owned(), "a".to_owned()], body);
    build::fold_call(source, lambda, int_lit(0), acc_is_list, homogeneous)
}

#[test]
fn a_scalar_fold_is_a_list_reduce_over_the_coalesced_list_its_lambda_swapped() {
    let body = call(SqlFn::Plus, vec![build::derived(None, "a"), build::derived(None, "e")]);
    let e = walk(&mut FoldToListReduce, fold(body.clone(), false, true)).into_inner();
    assert_eq!(fn_of(&e), SqlFn::ListReduce);
    let [list, lam, init] = args_of(&e) else { panic!() };
    assert_eq!(fn_of(list), SqlFn::Coalesce);
    assert_eq!(args_of(list)[1], build::array_lit(Vec::new()));
    let SqlExpr::Lambda(l) = lam else { panic!() };
    assert_eq!(l.params, ["a", "e"]);
    assert_eq!(*l.body, body);
    assert_eq!(init, &int_lit(0));
    // three arguments: list_reduce's rule types two, so the call is UNKNOWN (as Java's)
    assert_eq!(e.r#type(), &TypeFact::Unknown);
}

#[test]
fn a_scalar_fold_whose_body_does_not_decompose_is_refused() {
    let body = call(SqlFn::Plus, vec![build::derived(None, "e"), build::derived(None, "a")]);
    let err = rewrite_expr(&mut FoldToListReduce, fold(body, false, false)).unwrap_err();
    assert_eq!(err.class, Refusal::ILLEGAL_STATE);
    assert_eq!(
        err.message,
        "fold body is not decomposable and the accumulator is scalar — rewrite accumulator-first ({e, a | $a <op> \
         ...}) so the reduction can decompose"
    );
}

#[test]
fn a_list_fold_wraps_each_element_and_unwraps_its_references() {
    let shadowing = build::lambda(&["e"], build::derived(None, "e"));
    let conformed = build::cast_conform(build::derived(None, "e"), t(Scalar::Varchar), true);
    let body = call(
        SqlFn::ListConcat,
        vec![build::derived(None, "a"), call(SqlFn::ListTransform, vec![conformed, shadowing.clone()])],
    );
    let e = walk(&mut FoldToListReduce, fold(body, true, false)).into_inner();
    let [wrapped, lam, _] = args_of(&e) else { panic!() };
    // list_transform(source, e -> [e])
    assert_eq!(fn_of(wrapped), SqlFn::ListTransform);
    let SqlExpr::Lambda(w) = &args_of(wrapped)[1] else { panic!() };
    assert_eq!(*w.body, build::array_lit(vec![build::derived(None, "e")]));
    let SqlExpr::Lambda(l) = lam else { panic!() };
    let inner = args_of(&args_of(&l.body)[1]);
    // a reference under a cast unwraps (the cast rebuilt by the two-argument constructor: not a conformance)
    let SqlExpr::Cast { value, conform, .. } = &inner[0] else { panic!() };
    assert!(!conform);
    assert_eq!(**value, call(SqlFn::ListGet, vec![build::derived(None, "e"), int_lit(1)]));
    // a lambda re-binding the name shadows it
    assert_eq!(inner[1], shadowing);
}

// ==================================================================
// CheckedDefectsToLists
// ==================================================================

#[test]
fn a_checked_childs_value_reads_the_envelope() {
    let env = col("t0", "ENV", Scalar::Json);
    let one = walk(&mut CheckedDefectsToLists, build::checked_child_value(env.clone(), false)).into_inner();
    assert_eq!(one, call(SqlFn::VariantGet, vec![env.clone(), string_lit("value")]));
    let many = walk(&mut CheckedDefectsToLists, build::checked_child_value(env.clone(), true)).into_inner();
    assert_eq!(fn_of(&many), SqlFn::ToVariant);
    let transform = &args_of(&many)[0];
    assert_eq!(args_of(transform)[0], call(SqlFn::VariantElements, vec![env]));
}

#[test]
fn checked_defects_concatenate_the_hoisted_paths() {
    let own = col("t0", "OWN", Scalar::Json);
    let hoist = |path: &[&str], to_many| Hoist {
        path: path.iter().map(|p| (*p).to_owned()).collect(),
        envelope: col("t0", "CHILD", Scalar::Json),
        to_many,
    };
    let e = build::checked_defects(own.clone(), vec![hoist(&["firm", "address"], false), hoist(&["employees"], true)]);
    let e = walk(&mut CheckedDefectsToLists, e).into_inner();
    // ((own ++ toOne) ++ toMany)
    assert_eq!(fn_of(&e), SqlFn::ListConcat);
    let [first, many] = args_of(&e) else { panic!() };
    assert_eq!(fn_of(many), SqlFn::ListFlatten);
    let [own2, one] = args_of(first) else { panic!() };
    assert_eq!(own2, &own);
    // coalesce(list_transform(elements(env->defects), d -> prefixed(d)), [])
    assert_eq!(fn_of(one), SqlFn::Coalesce);
    let SqlExpr::Lambda(d) = &args_of(&args_of(one)[0])[1] else { panic!() };
    // json_merge_patch(d, {path: to_json([{firm, null}, {address, null}] ++ elements(d->path))})
    let patch = &args_of(&d.body)[1];
    let SqlExpr::JsonObject { kv, .. } = patch else { panic!() };
    let nodes = &args_of(&args_of(&kv[1])[0])[0];
    let SqlExpr::ArrayLit { elements, .. } = nodes else { panic!() };
    assert_eq!(elements.len(), 2);
    let SqlExpr::JsonObject { kv, .. } = &elements[0] else { panic!() };
    assert_eq!(kv[1], string_lit("firm"));
    assert_eq!(kv[3], build::null_lit());
}

// ==================================================================
// ConstantKeysAsExpressions
// ==================================================================

fn grouped(keys: Vec<SqlExpr>, order: Vec<SqlExpr>) -> SqlQuery {
    let from = table("T", "t0", vec![out("A", Scalar::Bigint, Origin::Physical)], Vec::new());
    let s = sel(vec![proj(col("t0", "A", Scalar::Bigint), "a", Scalar::Bigint)], from);
    let order = order.into_iter().map(|e| SortKey { expr: e, ascending: true, null_order: None, output_name: None });
    q(SqlSelect { group_by: keys, order_by: order.collect(), ..s })
}

#[test]
fn duckdbs_constant_keys_cast_only_a_group_by_integer() {
    let mut pass = ConstantKeysAsExpressions::group_by_only();
    let r = rewrite(&mut pass, grouped(vec![int_lit(2), string_lit("[ROOT]")], vec![int_lit(1)])).unwrap();
    assert!(r.is_new());
    let s = select_of(r.get());
    assert_eq!(s.group_by[0], cast(int_lit(2), t(Scalar::Bigint)));
    assert_eq!(s.group_by[1], string_lit("[ROOT]"));
    assert_eq!(s.order_by[0].expr, int_lit(1));
    // nothing constant: the select as it came
    let none = grouped(vec![col("t0", "A", Scalar::Bigint)], Vec::new());
    assert!(!rewrite(&mut pass, none).unwrap().is_new());
}

#[test]
fn postgress_constant_keys_cast_every_literal_and_refuse_a_null_order_key() {
    let mut pass = ConstantKeysAsExpressions::default();
    let r = rewrite(&mut pass, grouped(vec![build::null_lit(), string_lit("x")], vec![string_lit("y")])).unwrap();
    let s = select_of(r.get());
    assert_eq!(s.group_by[0], cast(build::null_lit(), t(Scalar::Integer)));
    assert_eq!(s.group_by[1], cast(string_lit("x"), t(Scalar::Varchar)));
    assert_eq!(s.order_by[0].expr, cast(string_lit("y"), t(Scalar::Varchar)));
    let err = rewrite(&mut pass, grouped(Vec::new(), vec![build::null_lit()])).unwrap_err();
    assert_eq!(err.class, Refusal::DIALECT_CAPABILITY);
    assert_eq!(
        err.message,
        "a NULL ORDER BY key reached Postgres, which refuses a constant key and has no type to cast it to"
    );
    let untyped = SqlExpr::IntLit { value: 1, r#type: TypeFact::Unknown };
    let err = rewrite(&mut ConstantKeysAsExpressions::group_by_only(), grouped(vec![untyped], Vec::new())).unwrap_err();
    assert_eq!(
        err.message,
        "an untyped constant GROUP BY key reached DuckDB, which reads a constant key otherwise and has no type to cast \
         it to"
    );
}

// ==================================================================
// OuterJoinSubqueryFilters
// ==================================================================

fn max_id() -> SqlExpr {
    let agg = SqlExpr::Reducer(build::reducer(AggFn::Max, vec![col("u2", "id", Scalar::Bigint)], false, Vec::new()));
    build::scalar_subquery(q(sel(
        vec![proj(agg, "m", Scalar::Bigint)],
        table("U", "u2", vec![out("id", Scalar::Bigint, Origin::Physical)], Vec::new()),
    )))
}

fn outer_join(kind: JoinKind, on: SqlExpr) -> SqlQuery {
    let l = table("T", "t", vec![out("id", Scalar::Bigint, Origin::Physical)], Vec::new());
    let r = table(
        "U",
        "u",
        vec![out("id", Scalar::Bigint, Origin::Physical), out("t_id", Scalar::Bigint, Origin::Physical)],
        Vec::new(),
    );
    let from = build::join(l, r, kind, Some(on)).unwrap();
    q(sel(vec![proj(col("t", "id", Scalar::Bigint), "id", Scalar::Bigint)], from))
}

#[test]
fn a_left_joins_subquery_conjunct_of_the_right_side_filters_it_first() {
    let reads_left = call(SqlFn::Equal, vec![col("u", "t_id", Scalar::Bigint), col("t", "id", Scalar::Bigint)]);
    let right_only = call(SqlFn::Equal, vec![col("u", "id", Scalar::Bigint), max_id()]);
    let on = call(SqlFn::And, vec![reads_left.clone(), right_only.clone()]);
    let statement = outer_join(JoinKind::Left, on);
    let r = rewrite(&mut OuterJoinSubqueryFilters, statement.clone()).unwrap();
    assert!(r.is_new());
    let SqlSource::Join { right, on, .. } = &select_of(r.get()).from else { panic!() };
    assert_eq!(on.as_deref(), Some(&reads_left));
    // (SELECT * FROM u WHERE u.id = (SELECT MAX(id) ...)) AS u: the alias kept, its outputs the table's
    let SqlSource::Subselect { inner, alias, .. } = right.as_ref() else { panic!("{right:?}") };
    assert_eq!(alias, "u");
    let filtered = select_of(inner);
    assert_eq!(filtered.r#where.as_ref(), Some(&right_only));
    assert_eq!(filtered.outputs.len(), 2);
    // the renderer runs every pass over the statement as the lowering wrote it
    assert_eq!(
        DuckDb::new().render_inline(statement).unwrap(),
        "SELECT t.id AS id FROM T AS t LEFT OUTER JOIN ( SELECT * FROM U AS u WHERE u.id = (SELECT MAX(u2.id) AS m \
         FROM U AS u2) ) AS u ON u.t_id = t.id"
    );
}

#[test]
fn a_join_with_nothing_to_move_is_handed_back_as_it_came() {
    // the subquery conjunct alone: nothing would stay in the ON
    let alone = call(SqlFn::Equal, vec![col("u", "id", Scalar::Bigint), max_id()]);
    assert!(!rewrite(&mut OuterJoinSubqueryFilters, outer_join(JoinKind::Left, alone.clone())).unwrap().is_new());
    // an inner join plans it
    let both = call(SqlFn::And, vec![call(SqlFn::Greater, vec![col("u", "t_id", Scalar::Bigint), int_lit(0)]), alone]);
    assert!(!rewrite(&mut OuterJoinSubqueryFilters, outer_join(JoinKind::Inner, both.clone())).unwrap().is_new());
    // a conjunct reading the left side stays
    let left_read = call(SqlFn::Equal, vec![col("t", "id", Scalar::Bigint), max_id()]);
    let on =
        call(SqlFn::And, vec![call(SqlFn::Greater, vec![col("u", "t_id", Scalar::Bigint), int_lit(0)]), left_read]);
    assert!(!rewrite(&mut OuterJoinSubqueryFilters, outer_join(JoinKind::Left, on)).unwrap().is_new());
}

// ==================================================================
// VariantEgress
// ==================================================================

fn variant_table() -> SqlSource {
    table(
        "T",
        "t0",
        vec![out("ID", Scalar::Bigint, Origin::Physical), out("V", Scalar::Json, Origin::Physical)],
        Vec::new(),
    )
}

#[test]
fn a_root_star_over_a_stored_variant_is_spelled_into_its_columns() {
    let star_frame = q(build::star_of(variant_table()).unwrap());
    let r = VariantEgress.rewrite_root(star_frame.clone()).unwrap();
    let s = select_of(&r);
    assert_eq!(s.projections.len(), 2);
    assert!(matches!(&s.projections[1].expr, SqlExpr::Column(c) if c.name == "V" && c.table.as_deref() == Some("t0")));
    // Java's withProjections over out-less projections: no slot is declared
    assert!(s.outputs.is_empty());
    assert_eq!(DuckDb::new().render_inline(star_frame).unwrap(), "SELECT t0.ID, CAST(t0.V AS JSON) AS V FROM T AS t0");
    // a star over another source stays a star
    let plain = table("P", "t1", vec![out("ID", Scalar::Bigint, Origin::Physical)], Vec::new());
    let s = sel(vec![bare(build::star(Some("t1"))), proj(int_lit(1), "x", Scalar::Bigint)], plain);
    assert_eq!(VariantEgress.rewrite_root(q(s.clone())).unwrap(), q(s));
}

#[test]
fn a_star_below_the_root_is_left_to_the_column_reference() {
    let inner = build::star_of(variant_table()).unwrap();
    let outer =
        sel(vec![proj(col("s", "ID", Scalar::Bigint), "ID", Scalar::Bigint)], build::subselect(q(inner), "s", None));
    let before = q(outer);
    assert_eq!(VariantEgress.rewrite_root(before.clone()).unwrap(), before);
    // a union's branches are roots
    let b = || q(build::star_of(variant_table()).unwrap());
    let u = build::union(vec![b(), b()], true, Vec::new()).unwrap();
    let SqlQuery::Union(u) = VariantEgress.rewrite_root(SqlQuery::Union(u)).unwrap() else { panic!() };
    assert!(u.branches.iter().all(|b| select_of(b).projections.len() == 2));
}

// ==================================================================
// StoredReads
// ==================================================================

fn read_table() -> SqlSource {
    table(
        "T",
        "t0",
        vec![out("ID", Scalar::Bigint, Origin::Physical), out("ADDR", Scalar::Varchar, Origin::Physical)],
        vec![("ID", ColumnType::Plain { kind: ColumnKind::Bigint }), ("ADDR", other())],
    )
}

#[test]
fn a_read_column_is_read_by_its_stored_type_at_every_reference() {
    let mut pass = StoredReads::new(crate::ansi::reads_as_text);
    let s = sel(vec![proj(col("t0", "ADDR", Scalar::Varchar), "addr", Scalar::Varchar)], read_table());
    let s = SqlSelect { r#where: Some(call(SqlFn::IsNotNull, vec![col("t0", "ADDR", Scalar::Varchar)])), ..s };
    let r = pass.rewrite_root(q(s.clone())).unwrap();
    let read = select_of(&r);
    assert!(matches!(&read.projections[0].expr, SqlExpr::StoredRead { column, .. } if column.name == "ADDR"));
    assert!(matches!(&args_of(read.r#where.as_ref().unwrap())[0], SqlExpr::StoredRead { .. }));
    assert_eq!(
        DuckDb::new().render_inline(q(s)).unwrap(),
        "SELECT CAST(t0.ADDR AS VARCHAR) AS addr FROM T AS t0 WHERE CAST(t0.ADDR AS VARCHAR) IS NOT NULL"
    );
    // a second pass over the read tree reads the read's column again: refused, as Java's walk refuses it
    let err = StoredReads::new(crate::ansi::reads_as_text).rewrite_root(r).unwrap_err();
    assert_eq!(err.class, Refusal::ILLEGAL_STATE);
}

#[test]
fn a_star_over_a_read_table_is_spelled_and_a_statement_over_none_comes_back_as_it_came() {
    let mut pass = StoredReads::new(crate::ansi::reads_as_text);
    let r = pass.rewrite_root(q(build::star_of(read_table()).unwrap())).unwrap();
    let s = select_of(&r);
    assert_eq!(s.projections.len(), 2);
    assert!(matches!(&s.projections[0].expr, SqlExpr::Column(c) if c.name == "ID"));
    assert!(matches!(&s.projections[1].expr, SqlExpr::StoredRead { .. }));
    let plain = q(build::star_of(table("P", "t1", vec![out("ID", Scalar::Bigint, Origin::Physical)], vec![])).unwrap());
    assert_eq!(pass.rewrite_root(plain.clone()).unwrap(), plain);
}

#[test]
fn an_unqualified_read_and_a_pivot_over_a_read_table_are_refused_by_name() {
    let mut pass = StoredReads::new(crate::ansi::reads_as_text);
    let unqualified = SqlExpr::Column(column(None, "ADDR", Scalar::Varchar, true));
    let s = sel(vec![proj(unqualified, "a", Scalar::Varchar)], read_table());
    let err = pass.rewrite_root(q(s)).unwrap_err();
    assert_eq!(err.class, Refusal::DIALECT_CAPABILITY);
    assert_eq!(
        err.message,
        "an unqualified reference to column 'ADDR' of table 'T' cannot be read by its declared type"
    );
    // the key set in Java's HashMap order: "B" (66) before "C" (67) whatever the declaration order
    let stored = vec![("C", other()), ("B", other())];
    let src = table("T", "t0", vec![out("B", Scalar::Varchar, Origin::Physical)], stored);
    let pivot = SqlSource::Pivot {
        source: Box::new(src),
        on: Vec::new(),
        r#in: Vec::new(),
        named: Vec::new(),
        usings: Vec::new(),
        alias: "p".to_owned(),
        outputs: Vec::new(),
    };
    let err = pass.rewrite_root(q(build::star_of(pivot).unwrap())).unwrap_err();
    assert_eq!(err.message, "a PIVOT straight over table 'T' cannot read its columns [B, C] by their declared types");
}
