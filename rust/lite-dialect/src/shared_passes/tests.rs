// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! The shared passes held to their Java classes' own logic, one hand-built tree at a time: each rewrite, where each
//! hands back its input as it came (Java's `==`), and the refusals in Java's words. Each rewritten tree is written by
//! Postgres's renderer with no pass of its own. The golden oracles hold the passes over real statements
//! (`tests/dialect_differential.rs`).

use super::*;
use crate::build::{int_lit, string_lit};
use crate::postgres::Postgres;
use crate::renderer::AnsiSqlRenderer;
use crate::rewriter::{SqlRewriter, rewrite};
use crate::writer::SqlWriter;
use lite_sql::{Column, NameQuoting, OutputCol, TypeFact};

// ---- builders

fn t(s: Scalar) -> SqlType {
    SqlType::Scalar(s)
}

fn out(name: &str, ty: SqlType, origin: Origin) -> OutputCol {
    OutputCol { name: name.into(), r#type: ty, nullable: true, origin }
}

fn table(alias: &str, outputs: Vec<OutputCol>) -> SqlSource {
    SqlSource::Table {
        name: "trades".into(),
        alias: alias.into(),
        outputs,
        call: false,
        stored_types: Vec::new(),
        quoting: NameQuoting::default(),
    }
}

fn trades(alias: &str) -> SqlSource {
    table(
        alias,
        vec![
            out("id", t(Scalar::Bigint), Origin::Physical),
            out("sym", t(Scalar::Varchar), Origin::Physical),
            out("px", SqlType::Decimal { precision: 18, scale: 6 }, Origin::Physical),
        ],
    )
}

fn col(table: &str, name: &str, ty: SqlType) -> SqlExpr {
    SqlExpr::Column(Column {
        table: Some(table.into()),
        name: name.into(),
        r#type: TypeFact::Typed { r#type: ty, nullable: true },
        origin: Some(Origin::Physical),
    })
}

/// `e AS alias`, its output declared as the expression's type (VARCHAR when it has none).
fn proj(e: SqlExpr, alias: &str) -> Projection {
    let ty = match e.r#type() {
        TypeFact::Typed { r#type, .. } => r#type.clone(),
        _ => t(Scalar::Varchar),
    };
    build::projection(e, Some(alias.into()), Some(build::output_col(alias, ty, true))).unwrap()
}

fn select_of(projections: Vec<Projection>, from: SqlSource) -> SqlSelect {
    build::select(SqlSelect {
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
        outputs: Vec::new(),
    })
    .unwrap()
}

fn run(pass: &mut dyn SqlRewriter, q: SqlQuery) -> Rw<SqlQuery> {
    rewrite(pass, q).unwrap()
}

/// The query as Postgres writes it, no pass run.
fn written(q: &SqlQuery) -> String {
    let d = Postgres::new();
    let mut w = SqlWriter::new();
    d.query(&mut w, q, 0).unwrap();
    w.into_text().unwrap()
}

fn window(f: AggFn, args: Vec<SqlExpr>, partition: Vec<SqlExpr>, order: Vec<SortKey>) -> SqlExpr {
    build::window_call(SqlAgg::Reducer(build::reducer(f, args, false, Vec::new())), partition, order, None)
}

fn key(e: SqlExpr) -> SortKey {
    SortKey { expr: e, ascending: true, null_order: None, output_name: None }
}

// ---- StarExceptToColumns

#[test]
fn a_star_exclusion_is_spelled_as_its_columns() {
    let none = select_of(vec![proj(col("t0", "id", t(Scalar::Bigint)), "id")], trades("t0"));
    assert!(!run(&mut StarExceptToColumns, query(none)).is_new(), "no exclusion: as it came");
    let except =
        SqlExpr::StarExcept { table: Some("t0".into()), except: vec!["sym".into()], r#type: TypeFact::Unknown };
    let s = select_of(vec![Projection { expr: except, alias: None, out: None }], trades("t0"));
    let q = run(&mut StarExceptToColumns, query(s));
    assert!(q.is_new());
    assert_eq!(written(q.get()), "SELECT \"t0\".id, \"t0\".px\nFROM trades AS \"t0\"");
}

// ---- WholePartitionOrderedSets

#[test]
fn a_whole_partitions_ordered_set_is_a_correlated_aggregate() {
    let px = col("t0", "px", SqlType::Decimal { precision: 18, scale: 6 });
    let sym = col("t0", "sym", t(Scalar::Varchar));
    let median = window(AggFn::Median, vec![px.clone()], vec![sym.clone()], Vec::new());
    let mut s = select_of(vec![proj(sym.clone(), "sym"), proj(median, "m")], trades("t0"));
    s.r#where = Some(build::call(SqlFn::Greater, vec![col("t0", "id", t(Scalar::Bigint)), int_lit(1)]));
    let q = run(&mut WholePartitionOrderedSets, query(s));
    assert!(q.is_new());
    // the copy is named by the projection's place; the select's own filter remapped; the partition matched null-safely
    assert_eq!(
        written(q.get()),
        "SELECT \"t0\".sym AS \"sym\", (SELECT percentile_cont(0.5) WITHIN GROUP (ORDER BY \"t0_ops1\".px) FROM \
         trades AS \"t0_ops1\" WHERE \"t0_ops1\".id > 1 AND NOT (\"t0_ops1\".sym IS DISTINCT FROM \"t0\".sym)) AS \"m\"\n\
         FROM trades AS \"t0\"\nWHERE \"t0\".id > 1"
    );
    // an ordered window, a grouped select or a select over a join is left as it is
    let ordered = window(AggFn::Median, vec![px.clone()], vec![sym.clone()], vec![key(sym.clone())]);
    let s = select_of(vec![proj(ordered, "m")], trades("t0"));
    assert!(!run(&mut WholePartitionOrderedSets, query(s)).is_new());
    let median = window(AggFn::Mode, vec![px.clone()], Vec::new(), Vec::new());
    let mut grouped = select_of(vec![proj(median.clone(), "m")], trades("t0"));
    grouped.group_by = vec![sym.clone()];
    assert!(!run(&mut WholePartitionOrderedSets, query(grouped)).is_new());
    let join = build::join(trades("t0"), trades("t1"), lite_sql::JoinKind::Cross, None).unwrap();
    assert!(!run(&mut WholePartitionOrderedSets, query(select_of(vec![proj(median.clone(), "m")], join))).is_new());
    // a nested ordered set is a copy one level down, its alias the child's place
    let nested = build::call(SqlFn::Plus, vec![int_lit(1), median]);
    let q = run(&mut WholePartitionOrderedSets, query(select_of(vec![proj(nested, "m")], trades("t0"))));
    assert!(written(q.get()).contains("FROM trades AS \"t0_ops0_1\""), "{}", written(q.get()));
    // a source no copy can be made of: as it came
    let cte = SqlSource::Cte { name: "c".into(), alias: "t0".into(), outputs: Vec::new() };
    let mode = window(AggFn::Mode, vec![px], Vec::new(), Vec::new());
    assert!(run(&mut WholePartitionOrderedSets, query(select_of(vec![proj(mode, "m")], cte))).is_new());
}

// ---- StringCountsAsInt

#[test]
fn a_string_functions_computed_count_is_an_integer() {
    let s = col("t0", "s", t(Scalar::Varchar));
    let n = col("t0", "n", t(Scalar::Bigint));
    let mut pass = StringCountsAsInt;
    let sub = build::call(SqlFn::Substring, vec![s.clone(), n.clone(), build::call(SqlFn::Length, vec![s.clone()])]);
    let e = rewrite_expr(&mut pass, sub).unwrap();
    assert!(e.is_new());
    let mut w = SqlWriter::new();
    Postgres::new().expr(&mut w, e.get(), 0).unwrap();
    // (Postgres's LENGTH arm reads a non-literal as text)
    assert_eq!(
        w.sql(),
        "substr(\"t0\".s, CAST(\"t0\".n AS INTEGER), CAST(length(CAST(\"t0\".s AS VARCHAR)) AS INTEGER))"
    );
    // a literal, or a count already an INTEGER, is left as it is; a call it does not name too
    let left = build::call(SqlFn::Left, vec![s.clone(), int_lit(2)]);
    assert!(!rewrite_expr(&mut pass, left).unwrap().is_new());
    let cast = build::call(SqlFn::Right, vec![s.clone(), build::cast(n.clone(), t(Scalar::Integer))]);
    assert!(!rewrite_expr(&mut pass, cast).unwrap().is_new());
    assert!(!rewrite_expr(&mut pass, build::call(SqlFn::Upper, vec![s])).unwrap().is_new());
    // chr's code is its first argument
    let chr = rewrite_expr(&mut pass, build::call(SqlFn::Chr, vec![n])).unwrap();
    assert!(matches!(chr.get(), SqlExpr::Call { args, .. } if matches!(args[0], SqlExpr::Cast { .. })));
}

// ---- AggregatesDeliverDouble

#[test]
fn the_named_aggregates_deliver_double() {
    let mut pass = AggregatesDeliverDouble::new(&[AggFn::Avg, AggFn::Corr]);
    let n = col("t0", "n", t(Scalar::Bigint));
    let avg = SqlExpr::Reducer(build::reducer(AggFn::Avg, vec![n.clone()], false, Vec::new()));
    let e = rewrite_expr(&mut pass, avg).unwrap();
    assert!(matches!(e.get(), SqlExpr::Cast { target: SqlType::Scalar(Scalar::Double), .. }));
    let sum = SqlExpr::Reducer(build::reducer(AggFn::Sum, vec![n], false, Vec::new()));
    assert!(!rewrite_expr(&mut pass, sum).unwrap().is_new());
}

// ---- QualifyToSubselect

fn qualified(distinct: bool) -> SqlSelect {
    let id = col("t0", "id", t(Scalar::Bigint));
    let sym = col("t0", "sym", t(Scalar::Varchar));
    let rn = window(AggFn::RowNumber, Vec::new(), vec![sym.clone()], vec![key(id.clone())]);
    let rn = match rn {
        SqlExpr::WindowCall { partition_by, order_by, frame, .. } => build::window_call(
            SqlAgg::RankingFn { r#fn: AggFn::RowNumber, args: Vec::new() },
            partition_by,
            order_by,
            frame,
        ),
        other => other,
    };
    let mut s = select_of(vec![proj(sym.clone(), "sym"), proj(id.clone(), "id")], trades("t0"));
    s.distinct = distinct;
    s.qualify = Some(build::call(SqlFn::Equal, vec![rn, int_lit(1)]));
    s.order_by = vec![key(sym)];
    s.limit = Some(3);
    s
}

#[test]
fn qualify_wraps_and_reads_through_its_outputs() {
    let plain = select_of(vec![proj(col("t0", "id", t(Scalar::Bigint)), "id")], trades("t0"));
    assert!(!run(&mut QualifyToSubselect::by_outputs(), query(plain)).is_new(), "no QUALIFY: as it came");
    // the window the filter reads a hidden inner column; the outer lists the outputs; sort and limit after the filter
    let q = run(&mut QualifyToSubselect::by_outputs(), query(qualified(false)));
    assert_eq!(
        written(q.get()),
        "SELECT \"qualify_src\".\"sym\" AS \"sym\", \"qualify_src\".\"id\" AS \"id\"\nFROM (\n  SELECT \"t0\".sym AS \
         \"sym\", \"t0\".id AS \"id\", ROW_NUMBER() OVER (PARTITION BY \"t0\".sym ORDER BY \"t0\".id NULLS LAST) AS \
         \"__qualify0\"\n  FROM trades AS \"t0\"\n) AS \"qualify_src\"\nWHERE \"qualify_src\".\"__qualify0\" = 1\nORDER \
         BY \"qualify_src\".\"sym\" NULLS LAST\nLIMIT 3"
    );
    // a DISTINCT select whose filter reads what it does not project is refused
    assert_eq!(
        rewrite(&mut QualifyToSubselect::by_outputs(), query(qualified(true))).unwrap_err(),
        Refusal::capability(
            "a DISTINCT select's QUALIFY or sort reads a value it does not project; a hidden column would change \
             what is distinct"
        )
    );
    // a subquery in the filter cannot be re-addressed
    let mut s = qualified(false);
    s.qualify = Some(build::exists(query(select_of(vec![proj(int_lit(1), "one")], SqlSource::Dual))));
    assert_eq!(
        rewrite(&mut QualifyToSubselect::by_outputs(), query(s)).unwrap_err(),
        Refusal::capability("a QUALIFY or sort holding a subquery cannot be re-addressed through its wrapping select")
    );
    // the plain form moves the predicate verbatim
    let q = run(&mut QualifyToSubselect::default(), query(qualified(false)));
    assert!(written(q.get()).starts_with("SELECT *\nFROM (\n"), "{}", written(q.get()));
}

#[test]
fn qualify_over_a_projected_window_reads_its_output() {
    let id = col("t0", "id", t(Scalar::Bigint));
    let rn = build::window_call(
        SqlAgg::RankingFn { r#fn: AggFn::RowNumber, args: Vec::new() },
        Vec::new(),
        vec![key(id.clone())],
        None,
    );
    let mut s = select_of(vec![proj(id, "id"), proj(rn.clone(), "rn")], trades("t0"));
    s.qualify = Some(build::call(SqlFn::LessEqual, vec![rn, int_lit(2)]));
    let q = run(&mut QualifyToSubselect::by_outputs(), query(s));
    let text = written(q.get());
    // nothing hidden: the outer select is a star
    assert!(text.starts_with("SELECT *\nFROM (\n"), "{text}");
    assert!(text.ends_with("WHERE \"qualify_src\".\"rn\" <= 2"), "{text}");
}

// ---- RootNumericTypes

#[test]
fn a_root_columns_numeric_type_is_stated() {
    let px = col("t0", "px", SqlType::Decimal { precision: 18, scale: 6 });
    let times = build::call(SqlFn::Times, vec![px.clone(), int_lit(2)]);
    let mut s = select_of(vec![proj(times, "p2"), proj(px.clone(), "px")], trades("t0"));
    // a declared Decimal(10,2) output: the computed column cast to it; the table's own column read as declared
    s.outputs[0].r#type = SqlType::Decimal { precision: 10, scale: 2 };
    let q = RootNumericTypes.rewrite_root(query(s)).unwrap();
    assert_eq!(
        written(&q),
        "SELECT CAST(\"t0\".px * 2 AS DECIMAL(10, 2)) AS \"p2\", \"t0\".px AS \"px\"\nFROM trades AS \"t0\""
    );
    // the unbounded Decimal states no scale: left as it is
    let times = build::call(SqlFn::Times, vec![px.clone(), int_lit(2)]);
    let mut s = select_of(vec![proj(times, "p2")], trades("t0"));
    s.outputs[0].r#type = SqlType::Decimal { precision: 38, scale: 18 };
    assert!(!written(&RootNumericTypes.rewrite_root(query(s)).unwrap()).contains("CAST"));
    // a HUGEINT root column is a NUMERIC(38); a DISTINCT select's sort key that was the cast column is the cast one
    let n = col("t0", "id", t(Scalar::Bigint));
    let sum = build::call(SqlFn::Plus, vec![n.clone(), n.clone()]);
    let mut s = select_of(vec![proj(sum.clone(), "s")], trades("t0"));
    s.outputs[0].r#type = t(Scalar::Hugeint);
    s.distinct = true;
    s.order_by = vec![key(sum)];
    assert_eq!(
        written(&RootNumericTypes.rewrite_root(query(s)).unwrap()),
        "SELECT DISTINCT CAST(\"t0\".id + \"t0\".id AS NUMERIC(38)) AS \"s\"\nFROM trades AS \"t0\"\nORDER BY \
         CAST(\"t0\".id + \"t0\".id AS NUMERIC(38)) NULLS LAST"
    );
    // a union's branches are roots too, the union rebuilt
    let branch = || {
        let mut b = select_of(vec![proj(build::call(SqlFn::Times, vec![px.clone(), int_lit(2)]), "p")], trades("t0"));
        b.outputs[0].r#type = SqlType::Decimal { precision: 9, scale: 1 };
        query(b)
    };
    let union = SqlQuery::Union(build::union(vec![branch(), branch()], true, Vec::new()).unwrap());
    let text = written(&RootNumericTypes.rewrite_root(union).unwrap());
    assert_eq!(text.matches("AS DECIMAL(9, 1)").count(), 2, "{text}");
    let _ = string_lit("unused");
}

// ---- PivotAsGroupBy

/// `trades` pivoted on `sym` over `values`, one USING `agg(px) AS s`; its outputs the group key `id` and each value's
/// column.
fn pivot(values: Vec<SqlExpr>, agg: AggFn, args: Vec<SqlExpr>) -> SqlSource {
    let px = SqlType::Decimal { precision: 18, scale: 6 };
    let mut outputs = vec![out("id", t(Scalar::Bigint), Origin::Physical)];
    for v in &values {
        if let SqlExpr::StringLit { value, .. } = v {
            outputs.push(out(&format!("{value}__|__s"), px.clone(), Origin::Derived));
        }
    }
    SqlSource::Pivot {
        source: Box::new(table("t0", vec![out("id", t(Scalar::Bigint), Origin::Physical)])),
        on: vec![col("t0", "sym", t(Scalar::Varchar))],
        r#in: values,
        named: Vec::new(),
        usings: vec![lite_sql::PivotUsing {
            agg: build::reducer(agg, args, false, Vec::new()),
            alias: "s".into(),
            r#type: Some(px),
        }],
        alias: "p".into(),
        outputs,
    }
}

fn star_over(from: SqlSource) -> SqlQuery {
    query(build::star_of(from).unwrap())
}

#[test]
fn a_static_pivot_is_a_grouped_case_aggregation() {
    let px = col("t0", "px", SqlType::Decimal { precision: 18, scale: 6 });
    let q = star_over(pivot(vec![string_lit("A"), string_lit("B")], AggFn::Sum, vec![px.clone()]));
    let Rw::New(q) = run(&mut PivotAsGroupBy, q) else { panic!("a static pivot is rewritten") };
    assert_eq!(
        written(&q),
        "SELECT *\nFROM (\n  SELECT \"t0\".id AS \"id\", SUM(CASE WHEN \"t0\".sym = 'A' THEN \"t0\".px END) AS \
         \"A__|__s\", SUM(CASE WHEN \"t0\".sym = 'B' THEN \"t0\".px END) AS \"B__|__s\"\n  FROM trades AS \"t0\"\n  \
         GROUP BY \"t0\".id\n) AS \"p\""
    );
    // a dynamic pivot (no values) and one of two ON keys are left as they came
    let dynamic = star_over(pivot(Vec::new(), AggFn::Sum, vec![px.clone()]));
    assert!(matches!(run(&mut PivotAsGroupBy, dynamic), Rw::Same(_)));
    let mut two = pivot(vec![string_lit("A")], AggFn::Sum, vec![px.clone()]);
    if let SqlSource::Pivot { on, .. } = &mut two {
        on.push(col("t0", "id", t(Scalar::Bigint)));
    }
    assert!(matches!(run(&mut PivotAsGroupBy, star_over(two)), Rw::Same(_)));
    // an integer value names its column by its digits; a value of no literal text is refused, naming its class
    let q = star_over(pivot(vec![int_lit(7)], AggFn::Sum, vec![px.clone()]));
    let Rw::New(q) = run(&mut PivotAsGroupBy, q) else { panic!("rewritten") };
    assert!(written(&q).contains("AS \"7__|__s\""), "{}", written(&q));
    let date = SqlExpr::DateLit { iso: "2024-01-01".into(), r#type: TypeFact::Unknown };
    let r = rewrite(&mut PivotAsGroupBy, star_over(pivot(vec![date], AggFn::Sum, vec![px]))).unwrap_err();
    assert_eq!(
        r,
        Refusal::capability("a pivot value DateLit reached a dialect without a native pivot, with no column name")
    );
    // an aggregate of no argument (COUNT(*)) has nothing to filter
    let count = star_over(pivot(vec![string_lit("A")], AggFn::Count, Vec::new()));
    let r = rewrite(&mut PivotAsGroupBy, count).unwrap_err();
    assert_eq!(
        r,
        Refusal::capability("a pivot whose aggregate takes no argument reached a dialect without a native pivot")
    );
}

// ---- AsOfAsPick

fn as_of(on: SqlExpr) -> SqlSource {
    let right = table("r", vec![out("ts", t(Scalar::Bigint), Origin::Physical)]);
    build::join(trades("l"), right, JoinKind::AsofLeft, Some(on)).unwrap()
}

/// The query as DuckDB writes it (QUALIFY native), no pass run.
fn written_qualified(q: &SqlQuery) -> String {
    let d = crate::duckdb::DuckDb::new();
    let mut w = SqlWriter::new();
    d.query(&mut w, q, 0).unwrap();
    w.into_text().unwrap()
}

#[test]
fn an_as_of_join_is_a_left_join_picked_by_its_extreme_key() {
    let (id, ts) = (col("l", "id", t(Scalar::Bigint)), col("r", "ts", t(Scalar::Bigint)));
    let ge = build::call(SqlFn::GreaterEqual, vec![id, ts]);
    let s = select_of(vec![proj(col("l", "sym", t(Scalar::Varchar)), "sym")], as_of(ge.clone()));
    let Rw::New(q) = run(&mut AsOfAsPick, query(s)) else { panic!("an as-of join is rewritten") };
    assert_eq!(
        written_qualified(&q),
        "SELECT l.sym AS sym\nFROM trades AS l\nLEFT OUTER JOIN trades AS r ON l.id >= r.ts\nQUALIFY r.ts = \
         MAX(r.ts) OVER \
         (PARTITION BY l.id, l.sym, l.px) OR MAX(r.ts) OVER (PARTITION BY l.id, l.sym, l.px) IS NULL"
    );
    // bounded below: the least right key
    let le = build::call(SqlFn::LessEqual, vec![col("l", "id", t(Scalar::Bigint)), col("r", "ts", t(Scalar::Bigint))]);
    let s = select_of(vec![proj(col("l", "sym", t(Scalar::Varchar)), "sym")], as_of(le));
    let Rw::New(q) = run(&mut AsOfAsPick, query(s)) else { panic!("rewritten") };
    assert!(written_qualified(&q).contains("r.ts = MIN(r.ts) OVER"), "{}", written_qualified(&q));
    // any other join is left as it came
    let inner = build::join(trades("l"), trades("r"), JoinKind::Inner, Some(ge.clone())).unwrap();
    let s = select_of(vec![proj(col("l", "sym", t(Scalar::Varchar)), "sym")], inner);
    assert!(matches!(run(&mut AsOfAsPick, query(s)), Rw::Same(_)));
    // no inequality between the sides: refused
    let eq = build::call(SqlFn::Equal, vec![col("l", "id", t(Scalar::Bigint)), col("r", "ts", t(Scalar::Bigint))]);
    let s = select_of(vec![proj(col("l", "sym", t(Scalar::Varchar)), "sym")], as_of(eq));
    assert_eq!(
        rewrite(&mut AsOfAsPick, query(s)).unwrap_err(),
        Refusal::capability(
            "an as-of join with no inequality between its sides reached a dialect without an as-of join"
        )
    );
    // the pick is the select's own filter: a select that filters (or windows) is refused
    let mut s = select_of(vec![proj(col("l", "sym", t(Scalar::Varchar)), "sym")], as_of(ge.clone()));
    s.r#where = Some(build::bool_lit(true));
    let r = rewrite(&mut AsOfAsPick, query(s)).unwrap_err();
    assert_eq!(
        r.message,
        "an as-of join under a select that filters, groups or windows reached a dialect without an as-of join: its \
         pick is that select's filter"
    );
    let ranked = window(AggFn::Count, vec![col("l", "id", t(Scalar::Bigint))], Vec::new(), Vec::new());
    let s = select_of(vec![proj(ranked, "n")], as_of(ge));
    assert!(rewrite(&mut AsOfAsPick, query(s)).is_err());
}
