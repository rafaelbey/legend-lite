// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! `CarrierStrategies` pinned from the Java's own logic: DuckDB's capabilities leave every tree as it came, and over
//! a dialect without native lists (H2's capabilities, Postgres's) each rule rewrites a hand-built tree as the Java
//! builds it. A rewritten tree is shown as DuckDB writes it (no passes), so an assertion reads as SQL; the type facts
//! and outputs the constructors compute are asserted where a rule depends on them.

use super::*;
use crate::build::{array_lit, bool_lit, derived, null_lit, scalar_subquery, typed_column};
use crate::rewriter::{rewrite, rewrite_expr, rewrite_source};
use crate::{AnsiSqlRenderer, DuckDb, SqlWriter};
use lite_sql::{ColumnType, NameQuoting, PivotUsing};

const H2: Caps = Caps { native_lists: false, correlated_explode: false, json_carrier: false, native_full_outer: false };
const POSTGRES: Caps =
    Caps { native_lists: false, correlated_explode: true, json_carrier: true, native_full_outer: true };

fn ty(s: Scalar) -> SqlType {
    SqlType::Scalar(s)
}

fn fact(s: Scalar, nullable: bool) -> TypeFact {
    TypeFact::Typed { r#type: ty(s), nullable }
}

fn out(name: &str, s: Scalar, nullable: bool, origin: Origin) -> OutputCol {
    OutputCol { name: name.to_owned(), r#type: ty(s), nullable, origin }
}

fn table(name: &str, alias: &str, cols: &[(&str, Scalar)]) -> SqlSource {
    SqlSource::Table {
        name: name.to_owned(),
        alias: alias.to_owned(),
        outputs: cols.iter().map(|(n, s)| out(n, *s, false, Origin::Physical)).collect(),
        call: false,
        stored_types: Vec::<(String, ColumnType)>::new(),
        quoting: NameQuoting::default(),
    }
}

fn c(alias: &str, name: &str, s: Scalar) -> SqlExpr {
    typed_column(Some(alias), name, ty(s), false, Origin::Physical)
}

fn s_lit(v: &str) -> SqlExpr {
    string_lit(v)
}

fn sel(projections: Vec<Projection>, from: SqlSource) -> SqlSelect {
    build::select_of(projections, from, Vec::new()).unwrap()
}

fn p(e: SqlExpr, alias: &str, o: OutputCol) -> Projection {
    build::projection(e, Some(alias.to_owned()), Some(o)).unwrap()
}

/// `(SELECT LIST(t.v) FROM T t)`: a collect of `t.v`, its order keys `keys`.
fn collect(keys: Vec<SortKey>) -> SqlExpr {
    let list = SqlExpr::Reducer(build::reducer(AggFn::List, vec![c("t", "v", Scalar::Integer)], false, keys));
    let o = build::output_col("l", SqlType::Array { element: Box::new(ty(Scalar::Integer)) }, true);
    scalar_subquery(build::query(sel(vec![p(list, "l", o)], table("T", "t", &[("v", Scalar::Integer)]))))
}

fn sql_e(e: &SqlExpr) -> String {
    let d = DuckDb::new();
    d.ansi_state().inline.set(true);
    let mut w = SqlWriter::new();
    d.expr(&mut w, e, 0).unwrap();
    w.into_text().unwrap()
}

fn sql_q(q: &SqlQuery) -> String {
    let d = DuckDb::new();
    d.ansi_state().inline.set(true);
    let mut w = SqlWriter::new();
    d.query(&mut w, q, 0).unwrap();
    w.into_text().unwrap()
}

fn on(caps: Caps, e: SqlExpr) -> Rw<SqlExpr> {
    rewrite_expr(&mut CarrierStrategies::new(caps), e).unwrap()
}

fn h2(e: SqlExpr) -> String {
    let r = on(H2, e);
    assert!(r.is_new(), "the rule fired");
    sql_e(&r.into_inner())
}

fn h2_q(s: SqlSelect) -> SqlQuery {
    rewrite(&mut CarrierStrategies::new(H2), build::query(s)).unwrap().into_inner()
}

// ---- DuckDB: every hook the identity

#[test]
fn duckdb_caps_leave_every_tree_as_it_came() {
    let t = table("T", "t", &[("v", Scalar::Integer), ("k", Scalar::Bigint)]);
    let u = table("U", "u", &[("v", Scalar::Integer)]);
    let full = build::join(
        t,
        u,
        JoinKind::Full,
        Some(call(SqlFn::Equal, vec![c("t", "v", Scalar::Integer), c("u", "v", Scalar::Integer)])),
    )
    .unwrap();
    let member = build::membership(c("t", "k", Scalar::Bigint), array_lit(vec![int_lit(1), int_lit(2)]));
    let concat = call(SqlFn::ListConcat, vec![array_lit(vec![int_lit(1)]), array_lit(vec![int_lit(2)])]);
    let unnest = call(SqlFn::Unnest, vec![collect(Vec::new())]);
    let s = build::select(SqlSelect {
        r#where: Some(member),
        ..sel(
            vec![
                p(concat, "c", build::output_col("c", SqlType::Array { element: Box::new(ty(Scalar::Bigint)) }, false)),
                p(unnest, "u", out("u", Scalar::Integer, true, Origin::Derived)),
            ],
            full,
        )
    })
    .unwrap();
    let q = build::query(s);
    let r = rewrite(&mut CarrierStrategies::new(crate::duckdb::CAPS), q.clone()).unwrap();
    assert_eq!(r, Rw::Same(q));
}

// ---- the expression rules

#[test]
fn a_concatenation_of_literal_lists_folds_to_one_literal() {
    let e = call(
        SqlFn::ListConcat,
        vec![array_lit(vec![int_lit(1), int_lit(2)]), array_lit(vec![int_lit(3)]), array_lit(Vec::new())],
    );
    let r = on(H2, e).into_inner();
    assert_eq!(sql_e(&r), "[1, 2, 3]");
    assert_eq!(
        *r.r#type(),
        TypeFact::Typed { r#type: SqlType::Array { element: Box::new(ty(Scalar::Bigint)) }, nullable: false }
    );
    // one non-literal arm: no rule
    let e = call(SqlFn::ListConcat, vec![array_lit(vec![int_lit(1)]), c("t", "l", Scalar::Integer)]);
    assert!(!on(H2, e).is_new());
}

#[test]
fn a_list_length_is_compile_time_a_count_or_a_separator_count() {
    assert_eq!(h2(call(SqlFn::ListLength, vec![array_lit(vec![s_lit("a"), null_lit(), s_lit("b")])])), "3");
    let split = call(SqlFn::Split, vec![c("t", "s", Scalar::Varchar), s_lit(",")]);
    assert_eq!(
        h2(call(SqlFn::ListLength, vec![split])),
        "length(CAST(t.s AS VARCHAR)) - length(CAST(replace(t.s, ',', '') AS VARCHAR)) + 1"
    );
    // a two-character separator has no rule
    let split2 = call(SqlFn::Split, vec![c("t", "s", Scalar::Varchar), s_lit(", ")]);
    assert!(!on(H2, call(SqlFn::ListLength, vec![split2])).is_new());
    let counted = on(H2, call(SqlFn::ListLength, vec![collect(Vec::new())])).into_inner();
    assert_eq!(sql_e(&counted), "(SELECT COUNT(*) AS l FROM T AS t)");
    // the COUNT never nulls and the select is one row: the subquery's slot keeps the declared label
    assert_eq!(*counted.r#type(), TypeFact::Typed { r#type: ty(Scalar::Bigint), nullable: false });
}

#[test]
fn a_filtered_literal_length_sums_the_element_tests() {
    let x = |n: &str| derived(None, n);
    let filtered = call(
        SqlFn::ListFilter,
        vec![
            array_lit(vec![int_lit(1), int_lit(5)]),
            build::lambda(&["x"], call(SqlFn::Greater, vec![x("x"), int_lit(2)])),
        ],
    );
    assert_eq!(
        h2(call(SqlFn::ListLength, vec![filtered])),
        "CASE WHEN 1 > 2 THEN 1 ELSE 0 END + CASE WHEN 5 > 2 THEN 1 ELSE 0 END"
    );
    // compacted: an element must be non-null too, and a struct literal's null test is its answer
    let point = build::struct_lit(vec![StructLitField { name: "a".to_owned(), value: int_lit(1), declared: None }]);
    let compact = build::compact_list(call(
        SqlFn::ListFilter,
        vec![
            array_lit(vec![point]),
            build::lambda(&["p"], call(SqlFn::Greater, vec![derived(Some("p"), "a"), int_lit(0)])),
        ],
    ));
    assert_eq!(h2(call(SqlFn::ListLength, vec![compact])), "CASE WHEN TRUE AND 1 > 0 THEN 1 ELSE 0 END");
}

#[test]
fn literal_variants_fold_in_java_s_number_model() {
    let json = |t: &str| cast(s_lit(t), ty(Scalar::Json));
    // an array element is a Long
    assert_eq!(h2(call(SqlFn::VariantGet, vec![json("[10, 20]"), int_lit(1)])), "20");
    // a composite re-emits as a JSON cast of its text; a decimal keeps BigDecimal's text
    assert_eq!(
        h2(call(SqlFn::VariantGet, vec![json(r#"{"a": [1, 2.50, "x\"y"], "b": null}"#), s_lit("a")])),
        r#"CAST('[1,2.50,"x\"y"]' AS JSON)"#
    );
    assert_eq!(h2(call(SqlFn::VariantGet, vec![json(r#"{"n": 1e5}"#), s_lit("n")])), "CAST('1E+5' AS JSON)");
    assert_eq!(h2(call(SqlFn::VariantGet, vec![json(r#"{"n": null}"#), s_lit("n")])), "NULL");
    // a missing key or an index past the end: no fold
    assert!(!on(H2, call(SqlFn::VariantGet, vec![json("[1]"), int_lit(1)])).is_new());
    assert!(!on(H2, call(SqlFn::VariantGet, vec![json(r#"{"n": 1}"#), s_lit("m")])).is_new());
    assert_eq!(
        h2(call(SqlFn::VariantElements, vec![json(r#"[1, "a", {"k": true}]"#)])),
        r#"[CAST('1' AS JSON), CAST('"a"' AS JSON), CAST('{"k":true}' AS JSON)]"#
    );
    // the elements of a literal collection are the collection
    assert_eq!(h2(call(SqlFn::VariantElements, vec![array_lit(vec![int_lit(7)])])), "[7]");
    // malformed literal JSON is Java's refusal
    let r = rewrite_expr(&mut CarrierStrategies::new(H2), call(SqlFn::VariantElements, vec![json("[1] 2")]));
    assert_eq!(r.unwrap_err(), Refusal::illegal_state("trailing JSON at 4"));
}

#[test]
fn a_typeof_date_test_is_a_length_test() {
    let e = call(
        SqlFn::Equal,
        vec![
            call(SqlFn::Typeof, vec![c("t", "d", Scalar::Timestamp)]),
            SqlExpr::TypeNameLit { scalar: Scalar::Date, r#type: fact(Scalar::Varchar, false) },
        ],
    );
    assert_eq!(h2(e), "length(CAST(CAST(t.d AS VARCHAR) AS VARCHAR)) = 10");
}

#[test]
fn a_sorted_or_filtered_collect_is_the_collect_reordered_or_filtered() {
    let key = build::sort_key(
        SqlExpr::RowOrder { table: Some("t".to_owned()), r#type: fact(Scalar::Bigint, false) },
        true,
        None,
        None,
    );
    assert_eq!(
        h2(call(SqlFn::ListSort, vec![collect(vec![key])])),
        "(SELECT LIST(t.v ORDER BY t.v ASC NULLS LAST, t.rowid ASC) AS l FROM T AS t)"
    );
    let pred = build::lambda(&["x"], call(SqlFn::Greater, vec![derived(None, "x"), int_lit(1)]));
    assert_eq!(
        h2(call(SqlFn::ListFilter, vec![collect(Vec::new()), pred])),
        "(SELECT LIST(t.v) AS l FROM T AS t WHERE t.v > 1)"
    );
}

#[test]
fn literal_reductions_fold_null_ignoring_in_the_reduction_s_type() {
    // SUM over BIGINT elements is HUGEINT; the additions are BIGINT, so the fold casts
    let folded = on(H2, call(SqlFn::ListSum, vec![array_lit(vec![int_lit(1), null_lit()])])).into_inner();
    assert_eq!(
        sql_e(&folded),
        "CAST(CASE WHEN 1 IS NULL AND NULL IS NULL THEN NULL ELSE coalesce(1, 0) + coalesce(NULL, 0) END AS HUGEINT)"
    );
    assert_eq!(*folded.r#type(), TypeFact::Typed { r#type: ty(Scalar::Hugeint), nullable: true });
    assert_eq!(
        h2(call(SqlFn::ListProduct, vec![array_lit(vec![int_lit(2), int_lit(3)])])),
        // the 1.0 factor is a DECIMAL(2,1), so the product's chain is DECIMAL(38,1): cast to the call's DOUBLE
        "CAST(CASE WHEN 2 IS NULL AND 3 IS NULL THEN NULL ELSE 1.0 * coalesce(2, 1) * coalesce(3, 1) END AS DOUBLE)"
    );
    assert_eq!(
        h2(call(SqlFn::ListBoolOr, vec![array_lit(vec![bool_lit(true)])])),
        "CASE WHEN TRUE IS NULL THEN NULL ELSE coalesce(TRUE, FALSE) END"
    );
    // a reduction with no literal fold keeps the node
    assert!(!on(H2, call(SqlFn::ListDistinct, vec![array_lit(vec![int_lit(1)])])).is_new());
}

#[test]
fn a_list_read_is_a_compile_time_pick_a_coalesce_a_split_part_or_a_limited_collect() {
    let lit = || array_lit(vec![s_lit("a"), s_lit("b"), s_lit("c")]);
    assert_eq!(h2(call(SqlFn::ListGet, vec![lit(), int_lit(2)])), "'b'");
    assert_eq!(h2(call(SqlFn::ListGet, vec![lit(), int_lit(-1)])), "'c'");
    assert_eq!(h2(call(SqlFn::ListGet, vec![lit(), int_lit(0)])), "NULL");
    assert_eq!(h2(call(SqlFn::ListGet, vec![lit(), int_lit(4)])), "NULL");
    let not_null = build::lambda(&["x"], call(SqlFn::IsNotNull, vec![derived(None, "x")]));
    let first = call(
        SqlFn::ListFilter,
        vec![array_lit(vec![c("t", "a", Scalar::Varchar), c("t", "b", Scalar::Varchar)]), not_null],
    );
    assert_eq!(h2(call(SqlFn::ListGet, vec![first, int_lit(1)])), "coalesce(t.a, t.b)");
    let split = || call(SqlFn::Split, vec![c("t", "s", Scalar::Varchar), s_lit("-")]);
    assert_eq!(h2(call(SqlFn::ListGet, vec![split(), int_lit(1)])), "split_part(t.s, '-', 1)");
    assert_eq!(
        h2(call(SqlFn::ListGet, vec![split(), int_lit(3)])),
        "CASE WHEN length(CAST(t.s AS VARCHAR)) - length(CAST(replace(t.s, '-', '') AS VARCHAR)) < 2 THEN NULL ELSE \
         split_part(t.s, '-', 3) END"
    );
    let key = build::sort_key(c("t", "v", Scalar::Integer), false, Some(NullOrder::NullsFirst), None);
    assert_eq!(
        h2(call(SqlFn::ListGet, vec![collect(vec![key.clone()]), int_lit(2)])),
        "(SELECT t.v AS l FROM T AS t ORDER BY t.v DESC NULLS FIRST LIMIT 1 OFFSET 1)"
    );
    assert_eq!(
        h2(call(SqlFn::ListGet, vec![collect(vec![key]), int_lit(-1)])),
        "(SELECT t.v AS l FROM T AS t ORDER BY t.v NULLS LAST LIMIT 1)"
    );
    // a keyless last has no defined order: no rule
    assert!(!on(H2, call(SqlFn::ListGet, vec![collect(Vec::new()), int_lit(-1)])).is_new());
}

#[test]
fn a_literal_read_at_a_computed_position_is_a_case_typed_by_the_channel() {
    let channel =
        cast(array_lit(vec![s_lit("x"), s_lit("y")]), SqlType::Array { element: Box::new(ty(Scalar::Varchar)) });
    let r = on(H2, call(SqlFn::ListGet, vec![channel, c("t", "i", Scalar::Bigint)])).into_inner();
    assert_eq!(sql_e(&r), "CAST(CASE WHEN t.i = 1 THEN 'x' WHEN t.i = 2 THEN 'y' END AS VARCHAR)");
    assert_eq!(*r.r#type(), fact(Scalar::Varchar, true));
}

#[test]
fn membership_is_an_or_chain_pushed_through_concat_and_case_or_an_exists() {
    let k = || c("t", "k", Scalar::Bigint);
    assert_eq!(h2(build::membership(k(), array_lit(vec![int_lit(1), null_lit(), int_lit(2)]))), "t.k = 1 OR t.k = 2");
    assert_eq!(h2(build::membership(k(), array_lit(vec![null_lit()]))), "FALSE");
    let concat = call(SqlFn::ListConcat, vec![array_lit(vec![int_lit(1)]), array_lit(vec![int_lit(2)])]);
    // a concatenation of literals folds first (bottom-up), so push the arms through a CASE
    let branched = build::case(
        vec![when(c("t", "b", Scalar::Boolean), array_lit(vec![int_lit(1)]))],
        Some(array_lit(vec![int_lit(2), int_lit(3)])),
    );
    assert_eq!(h2(build::membership(k(), branched)), "CASE WHEN t.b THEN t.k = 1 ELSE t.k = 2 OR t.k = 3 END");
    assert_eq!(membership_rule(&k(), &concat).unwrap().map(|e| sql_e(&e)).as_deref(), Some("t.k = 1 OR t.k = 2"));
    let not_null = build::lambda(&["x"], call(SqlFn::IsNotNull, vec![derived(None, "x")]));
    let filtered = call(SqlFn::ListFilter, vec![collect(Vec::new()), not_null]);
    // the collect's filter folds into it first; membership then reads the collect
    let r = on(H2, build::membership(k(), filtered)).into_inner();
    assert_eq!(sql_e(&r), "EXISTS (SELECT 1 FROM T AS t WHERE t.v IS NOT NULL AND t.v = t.k)");
    assert_eq!(
        h2(build::membership(k(), build::compact_list(collect(Vec::new())))),
        "EXISTS (SELECT 1 FROM T AS t WHERE t.v = t.k)"
    );
    // any arm without a rule: the node survives whole
    let half = build::case(
        vec![when(c("t", "b", Scalar::Boolean), array_lit(vec![int_lit(1)]))],
        Some(c("t", "l", Scalar::Integer)),
    );
    assert!(!on(H2, build::membership(k(), half)).is_new());
}

#[test]
fn a_reduction_fuses_into_its_collect() {
    let sep = s_lit(",");
    let key = build::sort_key(
        SqlExpr::RowOrder { table: Some("t".to_owned()), r#type: fact(Scalar::Bigint, false) },
        true,
        None,
        None,
    );
    let agg = |coll: SqlExpr| build::reduce_collection(AggFn::StringAgg, coll, vec![s_lit(",")]);
    assert_eq!(
        h2(agg(collect(vec![key.clone()]))),
        "(SELECT STRING_AGG(t.v, ',' ORDER BY t.rowid ASC) AS l FROM T AS t)"
    );
    // the element transform substitutes into the collected value
    let upper = build::lambda(&["x"], call(SqlFn::Upper, vec![derived(None, "x")]));
    let transformed = call(SqlFn::ListTransform, vec![collect(vec![key.clone()]), upper.clone()]);
    assert_eq!(h2(agg(transformed)), "(SELECT STRING_AGG(upper(t.v), ',' ORDER BY t.rowid ASC) AS l FROM T AS t)");
    // a sort between: the walk is bottom-up, so the sort of the collect is already the collect reordered (by its
    // value, its own keys the tiebreak) when the reduction fuses
    let sorted = call(SqlFn::ListTransform, vec![call(SqlFn::ListSort, vec![collect(vec![key])]), upper]);
    assert_eq!(
        h2(agg(sorted)),
        "(SELECT STRING_AGG(upper(t.v), ',' ORDER BY t.v ASC NULLS LAST, t.rowid ASC) AS l FROM T AS t)"
    );
    // a sort the walk has not reordered (no collect beneath it) is the fuse's own: ORDER BY the raw value
    assert_eq!(
        fuse(AggFn::StringAgg, &call(SqlFn::ListSort, vec![collect(Vec::new())]), &[s_lit(",")], true)
            .unwrap()
            .map(|e| sql_e(&e))
            .as_deref(),
        Some("(SELECT STRING_AGG(t.v, ',' ORDER BY t.v ASC) AS l FROM T AS t)")
    );
    // a LIST_* reducer is the reduction
    assert_eq!(h2(call(SqlFn::ListMax, vec![collect(Vec::new())])), "(SELECT MAX(t.v) AS l FROM T AS t)");
    // a literal collection joins as a CONCAT chain, unless literal folds are off (the engine text's)
    let lit = array_lit(vec![s_lit("a"), s_lit("b")]);
    assert_eq!(h2(agg(lit.clone())), "concat('a', ',', 'b')");
    let off = rewrite_expr(&mut CarrierStrategies::with_literal_folds(H2, false), agg(lit)).unwrap();
    assert!(!off.is_new());
    let _ = sep;
}

#[test]
fn a_row_major_cell_collect_joins_per_row_or_sorts_globally() {
    let cells = array_lit(vec![c("t", "a", Scalar::Varchar), c("t", "b", Scalar::Varchar)]);
    let list = SqlExpr::Reducer(build::reducer(AggFn::List, vec![cells], false, Vec::new()));
    let o = build::output_col("l", SqlType::Array { element: Box::new(ty(Scalar::Varchar)) }, true);
    let coll = scalar_subquery(build::query(sel(
        vec![p(list, "l", o)],
        table("T", "t", &[("a", Scalar::Varchar), ("b", Scalar::Varchar)]),
    )));
    let flat = || call(SqlFn::ListFlatten, vec![coll.clone()]);
    let agg = |x: SqlExpr| build::reduce_collection(AggFn::StringAgg, x, vec![s_lit("|")]);
    assert_eq!(h2(agg(flat())), "(SELECT STRING_AGG(concat(t.a, '|', t.b), '|') AS l FROM T AS t)");
    assert_eq!(
        h2(agg(call(SqlFn::ListSort, vec![flat()]))),
        "(SELECT STRING_AGG(_cells.v, '|' ORDER BY _cells.v ASC) FROM ( SELECT t.a AS v FROM T AS t UNION ALL SELECT t.b AS v FROM T AS t ) AS _cells)"
    );
}

#[test]
fn a_through_subselect_cell_collect_joins_in_the_inner_scope() {
    let inner = sel(
        vec![p(
            array_lit(vec![c("t", "a", Scalar::Varchar), c("t", "b", Scalar::Varchar)]),
            "cs",
            build::output_col("cs", SqlType::Array { element: Box::new(ty(Scalar::Varchar)) }, false),
        )],
        table("T", "t", &[("a", Scalar::Varchar), ("b", Scalar::Varchar)]),
    );
    let read = typed_column(
        Some("i"),
        "cs",
        SqlType::Array { element: Box::new(ty(Scalar::Varchar)) },
        false,
        Origin::Derived,
    );
    let list = SqlExpr::Reducer(build::reducer(AggFn::List, vec![read], false, Vec::new()));
    let o = build::output_col(
        "l",
        SqlType::Array { element: Box::new(SqlType::Array { element: Box::new(ty(Scalar::Varchar)) }) },
        true,
    );
    let coll =
        scalar_subquery(build::query(sel(vec![p(list, "l", o)], build::subselect(build::query(inner), "i", None))));
    let agg = |x: SqlExpr| build::reduce_collection(AggFn::StringAgg, x, vec![s_lit("|")]);
    assert_eq!(
        h2(agg(call(SqlFn::ListFlatten, vec![coll.clone()]))),
        "(SELECT STRING_AGG(i.cs, '|') AS l FROM ( SELECT concat(t.a, '|', t.b) AS cs FROM T AS t ) AS i)"
    );
    assert_eq!(
        h2(agg(call(SqlFn::ListSort, vec![call(SqlFn::ListFlatten, vec![coll])]))),
        "(SELECT STRING_AGG(_cells.v, '|' ORDER BY _cells.v ASC) FROM ( SELECT t.a AS v FROM T AS t UNION ALL SELECT t.b AS v FROM T AS t ) AS _cells)"
    );
}

#[test]
fn a_singleton_flatten_unwraps_to_a_collect_of_the_elements() {
    let inner = sel(
        vec![p(
            array_lit(vec![c("t", "d", Scalar::Date)]),
            "ds",
            build::output_col("ds", SqlType::Array { element: Box::new(ty(Scalar::Date)) }, false),
        )],
        table("T", "t", &[("d", Scalar::Date)]),
    );
    let read =
        typed_column(Some("i"), "ds", SqlType::Array { element: Box::new(ty(Scalar::Date)) }, false, Origin::Derived);
    let list = SqlExpr::Reducer(build::reducer(AggFn::List, vec![read], false, Vec::new()));
    let o = build::output_col("l", SqlType::Array { element: Box::new(ty(Scalar::Date)) }, true);
    let coll =
        scalar_subquery(build::query(sel(vec![p(list, "l", o)], build::subselect(build::query(inner), "i", None))));
    assert_eq!(
        h2(build::reduce_collection(AggFn::Min, call(SqlFn::ListFlatten, vec![coll]), Vec::new())),
        "(SELECT MIN(i.ds) AS l FROM ( SELECT t.d AS ds FROM T AS t ) AS i)"
    );
}

// ---- the select rules: explode placements, FULL OUTER emulation

fn unnest_select(arg: SqlExpr) -> SqlSelect {
    sel(vec![p(call(SqlFn::Unnest, vec![arg]), "x", out("x", Scalar::Bigint, true, Origin::Derived))], SqlSource::Dual)
}

#[test]
fn an_explode_of_a_literal_is_a_union_of_rows() {
    let q = h2_q(unnest_select(array_lit(vec![int_lit(1), int_lit(2)])));
    assert_eq!(sql_q(&q), "SELECT 1 AS x UNION ALL SELECT 2 AS x");
    // one element is its one row, a union needs two
    let q = h2_q(unnest_select(array_lit(vec![int_lit(1)])));
    assert_eq!(sql_q(&q), "SELECT 1 AS x");
    // NULL explodes to no rows
    let q = h2_q(unnest_select(null_lit()));
    assert_eq!(sql_q(&q), "SELECT CAST(NULL AS BIGINT) AS x WHERE FALSE");
    // a JSON array cast folds to its literal first
    let json = cast(cast(s_lit("[3, 4]"), ty(Scalar::Json)), SqlType::Array { element: Box::new(ty(Scalar::Bigint)) });
    assert_eq!(sql_q(&h2_q(unnest_select(json))), "SELECT 3 AS x UNION ALL SELECT 4 AS x");
    // concatenated arms flatten into one union; a compaction is a no-op over rows
    let concat = build::compact_list(call(
        SqlFn::ListConcat,
        vec![array_lit(vec![int_lit(1), int_lit(2)]), collect(Vec::new())],
    ));
    assert_eq!(
        sql_q(&h2_q(unnest_select(concat))),
        "SELECT 1 AS x UNION ALL SELECT 2 AS x UNION ALL SELECT t.v AS x FROM T AS t"
    );
}

#[test]
fn an_explode_of_a_collect_is_its_rows() {
    let key = build::sort_key(c("t", "v", Scalar::Integer), false, None, None);
    assert_eq!(
        sql_q(&h2_q(unnest_select(collect(vec![key.clone()])))),
        "SELECT t.v AS x FROM T AS t ORDER BY t.v DESC NULLS FIRST"
    );
    assert_eq!(
        sql_q(&h2_q(unnest_select(call(SqlFn::ListSort, vec![collect(vec![key])])))),
        "SELECT t.v AS x FROM T AS t ORDER BY t.v NULLS LAST, t.v DESC NULLS FIRST"
    );
    let pred = build::lambda(&["e"], call(SqlFn::IsNotNull, vec![derived(None, "e")]));
    let filtered = call(SqlFn::ListFilter, vec![array_lit(vec![int_lit(1), null_lit()]), pred]);
    assert_eq!(
        sql_q(&h2_q(unnest_select(filtered))),
        "SELECT 1 AS x WHERE 1 IS NOT NULL UNION ALL SELECT CAST(NULL AS BIGINT) AS x WHERE NULL IS NOT \
         NULL"
    );
}

#[test]
fn an_explode_through_a_subselect_reads_its_cells() {
    let inner = sel(
        vec![p(
            array_lit(vec![c("t", "a", Scalar::Bigint), c("t", "b", Scalar::Bigint)]),
            "cs",
            build::output_col("cs", SqlType::Array { element: Box::new(ty(Scalar::Bigint)) }, false),
        )],
        table("T", "t", &[("a", Scalar::Bigint), ("b", Scalar::Bigint)]),
    );
    let read =
        typed_column(Some("i"), "cs", SqlType::Array { element: Box::new(ty(Scalar::Bigint)) }, false, Origin::Derived);
    let s = sel(
        vec![p(call(SqlFn::Unnest, vec![read]), "x", out("x", Scalar::Bigint, true, Origin::Derived))],
        build::subselect(build::query(inner), "i", None),
    );
    assert_eq!(sql_q(&h2_q(s)), "SELECT t.a AS x FROM T AS t UNION ALL SELECT t.b AS x FROM T AS t");
}

/// `LIST_FILTER(list, (x, i) -> LIST_POSITION(list's keys, key(x)) = i)`: the ordered dedup.
fn dedup(list: SqlExpr, key: SqlExpr) -> SqlExpr {
    let keep = build::lambda(
        &["x", "i"],
        call(SqlFn::Equal, vec![call(SqlFn::ListPosition, vec![derived(None, "keys"), key]), derived(None, "i")]),
    );
    call(SqlFn::ListFilter, vec![list, keep])
}

#[test]
fn an_ordered_dedup_explodes_to_the_first_row_of_each_key() {
    let q = h2_q(unnest_select(dedup(array_lit(vec![int_lit(1), int_lit(2), int_lit(1)]), derived(None, "x"))));
    let text = sql_q(&q);
    assert_eq!(
        text,
        "SELECT dedup_first.x FROM ( SELECT dedup_src.x AS x, dedup_src.__dd_index AS __dd_index, ROW_NUMBER() OVER \
         (PARTITION BY dedup_src.__dd_key ORDER BY dedup_src.__dd_index NULLS LAST) AS __dd_first FROM ( SELECT 1 AS x, \
         1 AS __dd_key, 0 AS __dd_index UNION ALL SELECT 2 AS x, 2 AS __dd_key, 1 AS __dd_index UNION ALL SELECT 1 AS x, \
         1 AS __dd_key, 2 AS __dd_index ) AS dedup_src ) AS dedup_first WHERE dedup_first.__dd_first = 1 ORDER BY \
         dedup_first.__dd_index NULLS LAST"
    );
    // a collect by the value itself is DISTINCT, its order keys dropped
    let key = build::sort_key(c("t", "v", Scalar::Integer), true, None, None);
    let q = h2_q(unnest_select(dedup(collect(vec![key]), derived(None, "x"))));
    assert_eq!(sql_q(&q), "SELECT DISTINCT t.v AS x FROM T AS t");
    // by a key: one row per key
    let q = h2_q(unnest_select(dedup(collect(Vec::new()), call(SqlFn::Lower, vec![derived(None, "x")]))));
    assert_eq!(
        sql_q(&q),
        "SELECT dedup_first.x FROM ( SELECT dedup_src.x AS x, ROW_NUMBER() OVER (PARTITION BY dedup_src.__dd_key) AS \
         __dd_first FROM ( SELECT t.v AS x, lower(t.v) AS __dd_key FROM T AS t ) AS dedup_src ) AS dedup_first WHERE \
         dedup_first.__dd_first = 1"
    );
}

#[test]
fn a_full_outer_join_is_a_left_branch_and_an_anti_joined_right_branch() {
    let t = table("T", "t", &[("id", Scalar::Bigint)]);
    let u = table("U", "u", &[("id", Scalar::Bigint)]);
    let cond = call(SqlFn::Equal, vec![c("t", "id", Scalar::Bigint), c("u", "id", Scalar::Bigint)]);
    let full = build::join(t, u, JoinKind::Full, Some(cond)).unwrap();
    let s = sel(
        vec![
            p(c("t", "id", Scalar::Bigint), "a", out("a", Scalar::Bigint, true, Origin::Derived)),
            p(c("u", "id", Scalar::Bigint), "b", out("b", Scalar::Bigint, true, Origin::Derived)),
        ],
        full,
    );
    let plain = h2_q(s.clone());
    assert_eq!(
        sql_q(&plain),
        "SELECT t.id AS a, u.id AS b FROM T AS t LEFT OUTER JOIN U AS u ON t.id = u.id UNION ALL SELECT t.id AS \
         a, u.id AS b FROM T AS t RIGHT OUTER JOIN U AS u ON t.id = u.id WHERE NOT EXISTS (SELECT 1 FROM T AS _full \
         WHERE _full.id = u.id)"
    );
    // a native FULL OUTER (Postgres's caps) keeps the join
    let pg = rewrite(&mut CarrierStrategies::new(POSTGRES), build::query(s.clone())).unwrap();
    assert!(!pg.is_new());
    // the order and the limit belong to the union: keyed by output name
    let ordered = build::with_limit(
        build::with_order_by(s.clone(), vec![build::sort_key(c("u", "id", Scalar::Bigint), false, None, None)])
            .unwrap(),
        Some(5),
    )
    .unwrap();
    let q = h2_q(ordered);
    let text = sql_q(&q);
    assert!(text.starts_with("SELECT * FROM ( SELECT t.id AS a, u.id AS b FROM T AS t LEFT OUTER JOIN"), "{text}");
    assert!(text.ends_with(") AS _fullu ORDER BY _fullu.b DESC NULLS FIRST LIMIT 5"), "{text}");
    // a key naming no output is refused
    let bad = build::with_order_by(s, vec![build::sort_key(c("t", "zz", Scalar::Bigint), true, None, None)]).unwrap();
    let err = rewrite(&mut CarrierStrategies::new(H2), build::query(bad)).unwrap_err();
    assert_eq!(err.class, Refusal::DIALECT_CAPABILITY);
    assert!(
        err.message
            .starts_with("FULL OUTER JOIN emulation: sort key Column[...] is not one of the select's outputs [a, b]")
    );
}

// ---- the source rules: PIVOT as GROUP BY, ASOF emulation

fn pivot(usings: Vec<PivotUsing>, values: Vec<SqlExpr>) -> SqlSource {
    SqlSource::Pivot {
        source: Box::new(table(
            "S",
            "s",
            &[("city", Scalar::Varchar), ("yr", Scalar::Bigint), ("qty", Scalar::Bigint)],
        )),
        on: vec![c("s", "yr", Scalar::Bigint)],
        r#in: values,
        named: Vec::new(),
        usings,
        alias: "pv".to_owned(),
        outputs: vec![
            out("city", Scalar::Varchar, false, Origin::Physical),
            out("2011__|__total", Scalar::Hugeint, true, Origin::Derived),
        ],
    }
}

fn sum_qty(r#type: Option<SqlType>) -> PivotUsing {
    PivotUsing {
        agg: build::reducer(AggFn::Sum, vec![c("s", "qty", Scalar::Bigint)], false, Vec::new()),
        alias: "total".to_owned(),
        r#type,
    }
}

#[test]
fn a_static_pivot_is_a_group_by_with_a_filtered_aggregate_per_value() {
    let pv = pivot(vec![sum_qty(Some(ty(Scalar::Hugeint)))], vec![int_lit(2011), int_lit(2012)]);
    let r = rewrite_source(&mut CarrierStrategies::new(H2), pv).unwrap();
    assert!(r.is_new());
    let SqlSource::Subselect { inner, alias, .. } = r.into_inner() else { panic!("a subselect") };
    assert_eq!(alias, "pv");
    assert_eq!(
        sql_q(&inner),
        "SELECT s.city AS city, SUM(CASE WHEN s.yr = 2011 THEN s.qty END) AS \"2011__|__total\", SUM(CASE WHEN s.yr = \
         2012 THEN s.qty END) AS \"2012__|__total\" FROM S AS s GROUP BY s.city"
    );
    // the generated slots: the pivot's own where it names one, else the USING's typed result, nullable
    let outs = crate::typing::query_outputs(&inner).to_vec();
    assert_eq!(outs[1], out("2011__|__total", Scalar::Hugeint, true, Origin::Derived));
    assert_eq!(outs[2], out("2012__|__total", Scalar::Hugeint, true, Origin::Derived));
    // a USING with no typed slot, at a value the pivot does not name, is Java's refusal
    let untyped = pivot(vec![sum_qty(None)], vec![int_lit(2013)]);
    let err = rewrite_source(&mut CarrierStrategies::new(H2), untyped).unwrap_err();
    assert_eq!(
        err,
        Refusal::illegal_state(
            "the pivot's USING 'total' carries no typed slot, so its generated column '2013__|__total' has none"
        )
    );
    // a dynamic pivot (no values) stays for the renderer's wall
    let dynamic = pivot(vec![sum_qty(None)], Vec::new());
    assert!(!rewrite_source(&mut CarrierStrategies::new(H2), dynamic).unwrap().is_new());
}

#[test]
fn a_pivot_value_s_column_name_is_its_literal_text() {
    assert_eq!(lit_text(&s_lit("NY")), "NY");
    assert_eq!(lit_text(&bool_lit(true)), "true");
    let date = SqlExpr::DateLit { iso: "2020-01-01".to_owned(), r#type: fact(Scalar::Date, false) };
    assert_eq!(lit_text(&date), "DateLit[iso=2020-01-01, type=Typed[type=DATE, nullable=false]]");
    assert_eq!(
        lit_text(&build::float_lit(1.5)),
        "FloatLit[value=1.5, type=Typed[type=Decimal[precision=2, scale=1], nullable=false]]"
    );
}

#[test]
fn an_asof_join_is_a_left_join_pinned_to_the_extreme_key() {
    let t = table("T", "t", &[("k", Scalar::Bigint), ("ts", Scalar::Timestamp)]);
    let q = table("Q", "q", &[("k", Scalar::Bigint), ("ts", Scalar::Timestamp)]);
    let cond = call(
        SqlFn::And,
        vec![
            call(SqlFn::Equal, vec![c("t", "k", Scalar::Bigint), c("q", "k", Scalar::Bigint)]),
            call(SqlFn::GreaterEqual, vec![c("t", "ts", Scalar::Timestamp), c("q", "ts", Scalar::Timestamp)]),
        ],
    );
    let asof = build::join(t, q, JoinKind::AsofLeft, Some(cond)).unwrap();
    assert_eq!(as_of_key(&asof).unwrap(), Some((c("q", "ts", Scalar::Timestamp), true)));
    let r = rewrite_source(&mut CarrierStrategies::new(H2), asof).unwrap().into_inner();
    let s = sel(vec![build::projection(build::star(None), None, None).unwrap()], r);
    assert_eq!(
        sql_q(&build::query(s)),
        "SELECT * FROM T AS t LEFT OUTER JOIN Q AS q ON t.k = q.k AND t.ts >= q.ts AND q.ts = (SELECT MAX(_asof.ts) FROM Q \
         AS _asof WHERE t.k = _asof.k AND t.ts >= _asof.ts)"
    );
    // bounded below: the least key
    let t = table("T", "t", &[("ts", Scalar::Timestamp)]);
    let q = table("Q", "q", &[("ts", Scalar::Timestamp)]);
    let below = build::join(
        t,
        q,
        JoinKind::AsofLeft,
        Some(call(SqlFn::Less, vec![c("t", "ts", Scalar::Timestamp), c("q", "ts", Scalar::Timestamp)])),
    )
    .unwrap();
    assert_eq!(as_of_key(&below).unwrap(), Some((c("q", "ts", Scalar::Timestamp), false)));
}

// ---- the helpers' identity

#[test]
fn a_helper_that_changes_nothing_hands_the_node_back_and_a_rebuild_keeps_withchildren_s_facts() {
    let e = call(SqlFn::Plus, vec![c("t", "a", Scalar::Bigint), int_lit(1)]);
    assert_eq!(remapped(&e, "u", "_x").unwrap(), None);
    assert_eq!(sql_e(&remap_alias(&e, "t", "_x").unwrap()), "_x.a + 1");
    // withChildren keeps an array literal's given fact and a lambda's own (SqlRewriter's constructors do not)
    let given = TypeFact::Typed { r#type: SqlType::Array { element: Box::new(ty(Scalar::Varchar)) }, nullable: false };
    let arr = build::array_lit_given(vec![derived(None, "p")], given.clone());
    let lam = SqlExpr::Lambda(Lambda {
        params: vec!["y".to_owned()],
        body: Box::new(arr),
        r#type: fact(Scalar::Json, false),
    });
    let substituted = subst(&lam, "p", &null_lit()).unwrap();
    let SqlExpr::Lambda(l) = &substituted else { panic!("a lambda") };
    assert_eq!(l.r#type, fact(Scalar::Json, false));
    assert_eq!(*l.body.r#type(), given);
    // a stored read's column replaced by a non-column is Java's refusal
    let read =
        build::stored_read(build::plain_column(None, "p"), ColumnType::Plain { kind: lite_sql::ColumnKind::Other });
    let err = subst(&read, "p", &int_lit(1)).unwrap_err();
    assert_eq!(err.class, Refusal::ILLEGAL_STATE);
    assert!(err.message.starts_with("a stored read reads a table column, not IntLit"));
    // a qualified parameter read is the field of the value, folded over a literal struct
    let point = build::struct_lit(vec![StructLitField { name: "a".to_owned(), value: int_lit(9), declared: None }]);
    assert_eq!(subst(&derived(Some("p"), "a"), "p", &point).unwrap(), int_lit(9));
}

#[test]
fn copies_re_alias_the_simple_sources_alone() {
    let t = table("T", "t", &[("a", Scalar::Bigint)]);
    let SqlSource::Table { alias, outputs, .. } = copy_with_alias(&t, "_c").unwrap() else { panic!("a table") };
    assert_eq!((alias.as_str(), outputs.len()), ("_c", 1));
    let j = build::join(t.clone(), t, JoinKind::Cross, None).unwrap();
    assert_eq!(copy_with_alias(&j, "_c"), None);
}

#[test]
fn a_generated_slot_is_the_pivot_s_own_or_the_using_s_typed_result() {
    let outs = vec![out("2011__|__total", Scalar::Integer, false, Origin::Derived)];
    assert_eq!(generated_slot(&outs, &sum_qty(None), "2011__|__total").unwrap(), outs[0]);
    assert_eq!(
        generated_slot(&outs, &sum_qty(Some(ty(Scalar::Hugeint))), "2012__|__total").unwrap(),
        build::output_col("2012__|__total", ty(Scalar::Hugeint), true)
    );
}

#[test]
fn a_dedup_over_a_union_with_no_single_slot_is_a_distinct_star() {
    // an output-less projection: the indexed form needs the one slot, so the union is wrapped DISTINCT
    let s = sel(
        vec![
            build::projection(
                call(SqlFn::Unnest, vec![dedup(array_lit(vec![int_lit(1), int_lit(1)]), derived(None, "x"))]),
                None,
                None,
            )
            .unwrap(),
        ],
        SqlSource::Dual,
    );
    assert_eq!(sql_q(&h2_q(s)), "SELECT DISTINCT * FROM ( SELECT 1 UNION ALL SELECT 1 ) AS dedup_src");
}

#[test]
fn a_pivot_whose_using_takes_no_argument_stays() {
    let count = PivotUsing {
        agg: build::reducer(AggFn::Count, Vec::new(), false, Vec::new()),
        alias: "n".to_owned(),
        r#type: Some(ty(Scalar::Bigint)),
    };
    let pv = pivot(vec![count], vec![int_lit(2011)]);
    assert_eq!(rewrite_source(&mut CarrierStrategies::new(H2), pv.clone()).unwrap(), Rw::Same(pv));
}
