// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! The H2 family's passes held to their Java classes' own logic, one hand-built tree at a time: each rewrite, where
//! each hands back its input as it came (Java's `==`), and the refusals in Java's words. Each rewritten tree is written
//! by H2's renderer with no pass of its own. The golden oracles hold the passes over real statements
//! (`tests/dialect_differential.rs`).

use super::*;
use crate::build::{bool_lit, int_lit, string_lit};
use crate::h2::H2;
use crate::renderer::AnsiSqlRenderer;
use crate::typing;
use crate::writer::SqlWriter;
use lite_sql::{Column, NameQuoting, NullOrder, Origin, SortKey, SqlAgg, StructLitField};

// ---- builders

fn t(s: Scalar) -> SqlType {
    SqlType::Scalar(s)
}

fn col(table: &str, name: &str, origin: Origin) -> SqlExpr {
    SqlExpr::Column(Column {
        table: Some(table.into()),
        name: name.into(),
        r#type: TypeFact::Typed { r#type: t(Scalar::Bigint), nullable: true },
        origin: Some(origin),
    })
}

fn out(name: &str, ty: SqlType, origin: Origin) -> OutputCol {
    OutputCol { name: name.into(), r#type: ty, nullable: true, origin }
}

fn table(name: &str, alias: &str, outputs: Vec<OutputCol>) -> SqlSource {
    SqlSource::Table {
        name: name.into(),
        alias: alias.into(),
        outputs,
        call: false,
        stored_types: Vec::new(),
        quoting: NameQuoting::default(),
    }
}

fn proj(e: SqlExpr, alias: &str, ty: SqlType) -> Projection {
    build::projection(e, Some(alias.into()), Some(output_col(alias, ty, true))).unwrap()
}

fn sel(projections: Vec<Projection>, from: SqlSource) -> SqlSelect {
    plain_select(projections, from, None, Vec::new()).unwrap()
}

/// The one-row anchor a literal instance's lateral hangs off.
fn anchor() -> SqlSource {
    SqlSource::Values {
        rows: vec![vec![int_lit(1)]],
        columns: vec!["_anchor".into()],
        alias: "t2".into(),
        outputs: vec![out("_anchor", t(Scalar::Bigint), Origin::Derived)],
    }
}

/// `left <kind> JOIN LATERAL (inner) AS alias ON TRUE` (a CROSS lateral takes no condition).
fn lateral(left: SqlSource, inner: SqlSelect, alias: &str, kind: JoinKind) -> SqlSource {
    let on = (kind != JoinKind::CrossLateral).then(|| bool_lit(true));
    build::join(left, build::subselect(query(inner), alias, None), kind, on).unwrap()
}

/// `SELECT UNNEST(list) AS elem`, the element typed `element`.
fn unnest(list: SqlExpr, element: Option<SqlType>) -> SqlSelect {
    let p = match element {
        Some(ty) => proj(call(SqlFn::Unnest, vec![list]), "elem", ty),
        None => Projection { expr: call(SqlFn::Unnest, vec![list]), alias: Some("elem".into()), out: None },
    };
    sel(vec![p], SqlSource::Dual)
}

fn run(pass: &mut dyn SqlRewriter, q: SqlQuery) -> Rw<SqlQuery> {
    rewrite(pass, q).unwrap_or_else(|r| panic!("{r}"))
}

/// The query as H2 writes it, with no pass of its own.
fn written(q: &SqlQuery) -> String {
    let d = H2::new();
    let mut w = SqlWriter::new();
    d.query(&mut w, q, 0).unwrap_or_else(|r| panic!("{r}"));
    w.into_text().unwrap()
}

fn field(name: &str, value: SqlExpr) -> StructLitField {
    StructLitField { name: name.into(), value, declared: None }
}

// ---- QualifyToSubselect

#[test]
fn a_qualify_is_a_filter_over_the_wrapped_select() {
    let base = table("T", "t0", vec![out("A", t(Scalar::Bigint), Origin::Physical)]);
    let mut s = sel(vec![proj(col("t0", "A", Origin::Physical), "a", t(Scalar::Bigint))], base);
    assert!(!run(&mut QualifyToSubselect::default(), query(s.clone())).is_new(), "no QUALIFY: as it came");
    s.qualify = Some(call(SqlFn::Equal, vec![col("t0", "K", Origin::Physical), int_lit(1)]));
    let wrapped = run(&mut QualifyToSubselect::default(), query(s)).into_inner();
    assert_eq!(
        written(&wrapped),
        "SELECT *\nFROM (\n  SELECT \"t0\".A AS \"a\"\n  FROM T AS \"t0\"\n) AS \"qualify_src\"\nWHERE \"t0\".K = 1"
    );
    // the star's outputs are the wrapped select's
    assert_eq!(typing::query_outputs(&wrapped), [output_col("a", t(Scalar::Bigint), true)]);
}

// ---- ConstantLateralsAsRows

#[test]
fn a_lateral_over_a_constant_list_is_its_rows() {
    let list = build::array_lit(vec![int_lit(1), int_lit(2)]);
    let from = lateral(anchor(), unnest(list, Some(t(Scalar::Bigint))), "t1", JoinKind::LeftLateral);
    let s = sel(vec![proj(col("t1", "elem", Origin::Derived), "v", t(Scalar::Bigint))], from);
    let rows = run(&mut ConstantLateralsAsRows, query(s)).into_inner();
    assert_eq!(
        written(&rows),
        "SELECT \"t1\".\"elem\" AS \"v\"\nFROM (VALUES (1)) AS \"t2\"(\"_anchor\")\nLEFT OUTER JOIN (\n  SELECT 1 AS \
         \"elem\"\n  UNION ALL\n  SELECT 2 AS \"elem\"\n) AS \"t1\" ON TRUE"
    );
    // a CROSS lateral is an inner join; a NULL list has no rows, which no row passes
    let empty = lateral(anchor(), unnest(build::null_lit(), Some(t(Scalar::Bigint))), "t1", JoinKind::CrossLateral);
    let s = sel(vec![proj(col("t1", "elem", Origin::Derived), "v", t(Scalar::Bigint))], empty);
    assert_eq!(
        written(&run(&mut ConstantLateralsAsRows, query(s)).into_inner()),
        "SELECT \"t1\".\"elem\" AS \"v\"\nFROM (VALUES (1)) AS \"t2\"(\"_anchor\")\nJOIN (\n  SELECT CAST(NULL AS \
         BIGINT) AS \"elem\"\n  WHERE FALSE\n) AS \"t1\" ON TRUE"
    );
}

#[test]
fn a_struct_element_is_its_fields_and_each_field_read_their_column() {
    let person =
        |name: &str, age: i64| build::struct_lit(vec![field("name", string_lit(name)), field("age", int_lit(age))]);
    let list = build::array_lit(vec![person("Ann", 30), person("Bob", 40)]);
    let elem = || col("t1", "elem", Origin::Derived);
    let from = || lateral(anchor(), unnest(list.clone(), None), "t1", JoinKind::LeftLateral);
    let s = sel(
        vec![
            proj(build::struct_get(elem(), "name"), "name", t(Scalar::Varchar)),
            proj(build::struct_get(elem(), "age"), "age", t(Scalar::Bigint)),
        ],
        from(),
    );
    let rows = run(&mut ConstantLateralsAsRows, query(s)).into_inner();
    assert_eq!(
        written(&rows),
        "SELECT \"t1\".\"elem__name\" AS \"name\", \"t1\".\"elem__age\" AS \"age\"\nFROM (VALUES (1)) AS \
         \"t2\"(\"_anchor\")\nLEFT OUTER JOIN (\n  SELECT 'Ann' AS \"elem__name\", 30 AS \"elem__age\"\n  UNION ALL\n  \
         SELECT 'Bob' AS \"elem__name\", 40 AS \"elem__age\"\n) AS \"t1\" ON TRUE"
    );
    // a field read the elements do not have, or the element read whole: left as it is, refused where it is written
    for read in [build::struct_get(elem(), "nope"), elem()] {
        let s = sel(vec![proj(read, "x", t(Scalar::Varchar))], from());
        assert!(!run(&mut ConstantLateralsAsRows, query(s)).is_new());
    }
    // elements of differing fields: no rows
    let odd = build::array_lit(vec![person("Ann", 30), build::struct_lit(vec![field("name", string_lit("Bob"))])]);
    let s = sel(
        vec![proj(build::struct_get(elem(), "name"), "name", t(Scalar::Varchar))],
        lateral(anchor(), unnest(odd, None), "t1", JoinKind::LeftLateral),
    );
    assert!(!run(&mut ConstantLateralsAsRows, query(s)).is_new());
}

#[test]
fn a_list_that_reads_a_row_is_left_to_the_per_row_explosion() {
    let base = table("T", "t0", vec![out("A", t(Scalar::Bigint), Origin::Physical)]);
    let list = build::array_lit(vec![col("t0", "A", Origin::Physical)]);
    let from = lateral(base, unnest(list, Some(t(Scalar::Bigint))), "t1", JoinKind::LeftLateral);
    let s = sel(vec![proj(col("t1", "elem", Origin::Derived), "v", t(Scalar::Bigint))], from);
    assert!(!run(&mut ConstantLateralsAsRows, query(s.clone())).is_new());
    // no join at all: as it came
    let plain = sel(vec![proj(int_lit(1), "v", t(Scalar::Bigint))], SqlSource::Dual);
    assert!(!run(&mut ConstantLateralsAsRows, query(plain)).is_new());
    // a scalar element of no declared type has no column to carry it
    let untyped = lateral(anchor(), unnest(build::array_lit(vec![int_lit(1)]), None), "t1", JoinKind::LeftLateral);
    let s = sel(vec![proj(col("t1", "elem", Origin::Derived), "v", t(Scalar::Bigint))], untyped);
    assert!(!run(&mut ConstantLateralsAsRows, query(s)).is_new());
}

// ---- LateralExplodeToUnion

#[test]
fn a_per_row_literal_list_is_a_union_keyed_by_the_rows_identity() {
    let base = || table("T", "t0", vec![out("A", t(Scalar::Bigint), Origin::Physical)]);
    let a = || col("t0", "A", Origin::Physical);
    let b = || col("t0", "B", Origin::Physical);
    let list = call(SqlFn::ListConcat, vec![build::array_lit(vec![a()]), build::array_lit(vec![b()])]);
    let from = lateral(base(), unnest(list, Some(t(Scalar::Bigint))), "t1", JoinKind::LeftLateral);
    let s = sel(vec![proj(col("t1", "elem", Origin::Derived), "v", t(Scalar::Bigint))], from);
    let union = run(&mut LateralExplodeToUnion, query(s)).into_inner();
    assert_eq!(
        written(&union),
        "SELECT \"t1\".\"elem\" AS \"v\"\nFROM T AS \"t0\"\nLEFT OUTER JOIN (\n  SELECT \"t0\".A AS \"elem\", \
         \"t0\"._ROWID_ AS \"__rid\"\n  FROM T AS \"t0\"\n  UNION ALL\n  SELECT \"t0\".B AS \"elem\", \"t0\"._ROWID_ \
         AS \"__rid\"\n  FROM T AS \"t0\"\n) AS \"t1\" ON \"t1\".\"__rid\" = \"t0\"._ROWID_"
    );
    // the null-dropping singleton: its branch drops the NULL; a CROSS lateral joins inner
    let singleton = build::case(
        vec![build::when(call(SqlFn::IsNull, vec![a()]), build::array_lit(vec![]))],
        Some(build::array_lit(vec![a()])),
    );
    let from = lateral(base(), unnest(singleton, Some(t(Scalar::Bigint))), "t1", JoinKind::CrossLateral);
    let s = sel(vec![proj(col("t1", "elem", Origin::Derived), "v", t(Scalar::Bigint))], from);
    assert_eq!(
        written(&run(&mut LateralExplodeToUnion, query(s)).into_inner()),
        "SELECT \"t1\".\"elem\" AS \"v\"\nFROM T AS \"t0\"\nJOIN (\n  SELECT \"t0\".A AS \"elem\", \"t0\"._ROWID_ AS \
         \"__rid\"\n  FROM T AS \"t0\"\n  WHERE \"t0\".A IS NOT NULL\n) AS \"t1\" ON \"t1\".\"__rid\" = \"t0\"._ROWID_"
    );
}

#[test]
fn a_list_with_nothing_to_key_on_or_no_literal_elements_is_left_alone() {
    let base = || table("T", "t0", vec![out("L", t(Scalar::Bigint), Origin::Physical)]);
    let elem = || proj(col("t1", "elem", Origin::Derived), "v", t(Scalar::Bigint));
    // literal elements correlating to no row
    let constant = lateral(base(), unnest(build::array_lit(vec![int_lit(1)]), None), "t1", JoinKind::LeftLateral);
    assert!(!run(&mut LateralExplodeToUnion, query(sel(vec![elem()], constant))).is_new());
    // a stored list
    let stored = lateral(base(), unnest(col("t0", "L", Origin::Physical), None), "t1", JoinKind::LeftLateral);
    assert!(!run(&mut LateralExplodeToUnion, query(sel(vec![elem()], stored))).is_new());
}

#[test]
fn an_element_of_no_stated_type_is_refused() {
    let base = table("T", "t0", vec![out("A", t(Scalar::Bigint), Origin::Physical)]);
    let untyped = SqlExpr::Column(Column {
        table: Some("t0".into()),
        name: "A".into(),
        r#type: TypeFact::Unknown,
        origin: Some(Origin::Physical),
    });
    let list = build::array_lit_given(vec![untyped], TypeFact::Unknown);
    let from = lateral(base, unnest(list, None), "t1", JoinKind::LeftLateral);
    let s = sel(vec![proj(col("t1", "elem", Origin::Derived), "v", t(Scalar::Bigint))], from);
    assert_eq!(
        rewrite(&mut LateralExplodeToUnion, query(s)).unwrap_err(),
        Refusal::capability(
            "an exploded list states no element type (Unknown[]): this family's reader casts each element to the \
             list's type, and the list has none"
        )
    );
}

// ---- AggregatesDeliverDouble

#[test]
fn an_average_is_delivered_as_a_double() {
    let a = || col("t0", "A", Origin::Physical);
    let avg = SqlExpr::Reducer(build::reducer(AggFn::Avg, vec![a()], false, vec![]));
    let sum = SqlExpr::Reducer(build::reducer(AggFn::Sum, vec![a()], false, vec![]));
    let windowed = build::window_call(
        SqlAgg::Reducer(build::reducer(AggFn::Avg, vec![a()], false, vec![])),
        vec![],
        vec![SortKey { expr: a(), ascending: true, null_order: Some(NullOrder::NullsLast), output_name: None }],
        None,
    );
    let s = sel(
        vec![
            proj(avg, "avg", t(Scalar::Double)),
            proj(sum.clone(), "sum", t(Scalar::Bigint)),
            proj(windowed, "w", t(Scalar::Double)),
        ],
        table("T", "t0", vec![]),
    );
    let mut pass = AggregatesDeliverDouble::new(&[AggFn::Avg]);
    let cast = run(&mut pass, query(s)).into_inner();
    // a grouped average cast over its exact result; a windowed one is its dialect's to cast
    assert_eq!(
        written(&cast),
        "SELECT CAST(AVG(\"t0\".A) AS DOUBLE PRECISION) AS \"avg\", SUM(\"t0\".A) AS \"sum\", AVG(\"t0\".A) OVER \
         (ORDER BY \"t0\".A NULLS LAST) AS \"w\"\nFROM T AS \"t0\""
    );
    let SqlQuery::Select(s) = &cast else { panic!("a select") };
    assert_eq!(*s.projections[0].expr.r#type(), TypeFact::Typed { r#type: t(Scalar::Double), nullable: true });
    let only_sum = sel(vec![proj(sum, "sum", t(Scalar::Bigint))], table("T", "t0", vec![]));
    assert!(!run(&mut pass, query(only_sum)).is_new());
}

// ---- SourceSpelling

#[test]
fn a_reference_takes_its_sources_spelling() {
    let inner = sel(
        vec![proj(col("t0", "ID", Origin::Physical), "id", t(Scalar::Bigint))],
        table("T", "t0", vec![out("ID", t(Scalar::Bigint), Origin::Physical)]),
    );
    let outer = |reference: SqlExpr| {
        sel(vec![proj(reference, "x", t(Scalar::Bigint))], build::subselect(query(inner.clone()), "t3", None))
    };
    // stamped PHYSICAL against a subselect's DERIVED label: re-stamped from the source, its type the output's
    let stamped = SqlExpr::Column(Column {
        table: Some("t3".into()),
        name: "id".into(),
        r#type: TypeFact::Unknown,
        origin: Some(Origin::Physical),
    });
    let respelled = SourceSpelling::default().rewrite_root(query(outer(stamped))).unwrap();
    assert_eq!(
        written(&respelled),
        "SELECT \"t3\".\"id\" AS \"x\"\nFROM (\n  SELECT \"t0\".ID AS \"id\"\n  FROM T AS \"t0\"\n) AS \"t3\""
    );
    let SqlQuery::Select(s) = &respelled else { panic!("a select") };
    assert_eq!(*s.projections[0].expr.r#type(), TypeFact::Typed { r#type: t(Scalar::Bigint), nullable: true });
    // an origin-less reference is stamped too
    let bare = SqlExpr::Column(Column {
        table: Some("t3".into()),
        name: "id".into(),
        r#type: TypeFact::Unknown,
        origin: None,
    });
    assert!(written(&SourceSpelling::default().rewrite_root(query(outer(bare))).unwrap()).contains("\"t3\".\"id\""));
    // already the source's spelling, a name the source does not declare, an unknown alias: as they came
    for kept in
        [col("t3", "id", Origin::Derived), col("t3", "other", Origin::Physical), col("t9", "id", Origin::Physical)]
    {
        let mut pass = SourceSpelling::default();
        let q = query(outer(kept));
        assert!(!rewrite(&mut Scope(&mut pass.scope), q.clone()).unwrap().is_new());
        assert!(!rewrite(&mut pass, q).unwrap().is_new());
    }
}
