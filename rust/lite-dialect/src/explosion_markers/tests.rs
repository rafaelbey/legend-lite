// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! The explosion markers held to their Java (`ExplosionMarkers.java`): the predicates, the exploded read's type, and
//! each pass's rewrite with the identity cases the oracles cannot see -- a pass answering [`Rw::Same`] exactly where Java
//! hands back its input, and `SelectListUnnestAsLateral`'s names counted by the pass object.

use super::*;
use crate::rewriter::rewrite;
use lite_sql::{NameQuoting, OutputCol, SortKey};

fn scalar(s: Scalar) -> SqlType {
    SqlType::Scalar(s)
}

fn list_type(s: Scalar) -> SqlType {
    SqlType::Array { element: Box::new(scalar(s)) }
}

fn col(table: &str, name: &str, t: SqlType) -> SqlExpr {
    build::typed_column(Some(table), name, t, true, Origin::Physical)
}

fn unnest(list: SqlExpr) -> SqlExpr {
    build::call(SqlFn::Unnest, vec![list])
}

fn proj(e: SqlExpr, alias: &str) -> Projection {
    Projection { expr: e, alias: Some(alias.to_owned()), out: None }
}

fn select(projections: Vec<Projection>, from: SqlSource) -> SqlSelect {
    build::select_of(projections, from, Vec::new()).unwrap()
}

fn table() -> SqlSource {
    SqlSource::Table {
        name: "t".into(),
        alias: "t0".into(),
        outputs: vec![build::output_col("xs", list_type(Scalar::Bigint), true)],
        call: false,
        stored_types: Vec::new(),
        quoting: NameQuoting::default(),
    }
}

fn marker() -> SqlSelect {
    select(vec![proj(unnest(col("t0", "xs", list_type(Scalar::Bigint))), "e")], SqlSource::Dual)
}

#[test]
fn the_predicates() {
    assert!(is_marker_select(&marker()));
    let mut ordered = marker();
    ordered.order_by.push(SortKey { expr: build::int_lit(1), ascending: true, null_order: None, output_name: None });
    assert!(!is_marker_select(&ordered));
    let mut limited = marker();
    limited.limit = Some(1);
    assert!(!is_marker_select(&limited));
    assert!(!is_marker_select(&select(vec![proj(build::int_lit(1), "x")], SqlSource::Dual)));
    assert!(!is_marker_select(&select(vec![proj(unnest(build::null_lit()), "e")], table())));
    assert!(!is_unnest(&build::call(SqlFn::Unnest, vec![build::null_lit(), build::null_lit()])));
    assert!(null_list(&build::null_lit()));
    assert!(null_list(&build::compact_list(build::null_lit())));
    assert!(!null_list(&build::array_lit(Vec::new())));
    let min = build::call(SqlFn::ListMin, vec![build::null_lit()]);
    assert!(null_over_null_list(&min));
    assert!(null_over_null_list(&build::cast(min, scalar(Scalar::Bigint))));
    assert!(null_over_null_list(&build::reduce_collection(AggFn::Max, build::null_lit(), Vec::new())));
    assert!(!null_over_null_list(&build::call(SqlFn::ListMin, vec![col("t0", "xs", list_type(Scalar::Bigint))])));
    // a cast of NULL itself is no read of a list
    assert!(!null_over_null_list(&build::cast(build::null_lit(), scalar(Scalar::Bigint))));
    // the column a marker's projection names: its alias, else its output's, else refused
    let out = build::output_col("o", scalar(Scalar::Bigint), true);
    let named = Projection { expr: unnest(build::null_lit()), alias: None, out: Some(out) };
    assert_eq!(marker_column(&named).unwrap(), "o");
    let nameless = Projection { expr: unnest(build::null_lit()), alias: None, out: None };
    assert_eq!(
        marker_column(&nameless).unwrap_err(),
        Refusal::capability("an UNNEST without a column name reached the renderer")
    );
}

#[test]
fn an_exploded_read_is_typed_as_its_element_or_its_slot() {
    let xs = col("t0", "xs", list_type(Scalar::Bigint));
    // the element's type when known
    let read = exploded_read("u", &proj(unnest(xs.clone()), "e"), None).unwrap();
    assert_eq!(read, build::typed_column(Some("u"), "e", scalar(Scalar::Bigint), true, Origin::Derived));
    // an untyped list: a scalar slot's type; a composite slot over a real list none; over a NULL list the slot's
    let untyped = build::derived(Some("t0"), "ys");
    let p = proj(unnest(untyped), "e");
    let varchar = scalar(Scalar::Varchar);
    assert_eq!(
        exploded_read("u", &p, Some(&varchar)).unwrap(),
        build::typed_column(Some("u"), "e", varchar.clone(), true, Origin::Derived)
    );
    let composite = list_type(Scalar::Bigint);
    assert_eq!(exploded_read("u", &p, Some(&composite)).unwrap(), build::derived(Some("u"), "e"));
    let nulls = proj(unnest(build::null_lit()), "e");
    assert_eq!(
        exploded_read("u", &nulls, Some(&composite)).unwrap(),
        build::typed_column(Some("u"), "e", composite.clone(), true, Origin::Derived)
    );
    // a JSON element under a VARCHAR slot is its JSON text
    let js = col("t0", "js", list_type(Scalar::Json));
    assert_eq!(
        exploded_read("u", &proj(unnest(js), "e"), Some(&varchar)).unwrap(),
        build::cast(build::typed_column(Some("u"), "e", scalar(Scalar::Json), true, Origin::Derived), varchar)
    );
}

#[test]
fn a_star_over_a_marker_is_the_marker() {
    let star = build::star_of(build::subselect(build::query(marker()), "i0", None)).unwrap();
    let q = build::query(select(vec![], build::subselect(build::query(star.clone()), "u", None)));
    let out = rewrite(&mut StarOverExplosionUnwrapped, q).unwrap();
    assert!(out.is_new());
    let SqlQuery::Select(s) = out.into_inner() else { panic!("a select") };
    assert_eq!(s.from, build::subselect(build::query(marker()), "u", None));
    // a star with a clause, or over no marker, is left as it came
    let mut filtered = star;
    filtered.r#where = Some(build::bool_lit(true));
    let q = build::query(select(vec![], build::subselect(build::query(filtered), "u", None)));
    assert!(!rewrite(&mut StarOverExplosionUnwrapped, q).unwrap().is_new());
    let over_table = build::star_of(table()).unwrap();
    let q = build::query(select(vec![], build::subselect(build::query(over_table), "u", None)));
    assert!(!rewrite(&mut StarOverExplosionUnwrapped, q).unwrap().is_new());
}

/// `t CROSS JOIN LATERAL (SELECT UNNEST(c.lst) AS elem FROM (SELECT t0.xs AS lst) AS c) AS u`.
fn carried(kind: JoinKind, carry_filter: Option<SqlExpr>) -> SqlQuery {
    let mut carry = select(vec![proj(col("t0", "xs", list_type(Scalar::Bigint)), "lst")], SqlSource::Dual);
    carry.r#where = carry_filter;
    let explode = select(
        vec![proj(unnest(build::typed_column(Some("c"), "lst", list_type(Scalar::Bigint), true, Origin::Derived)), "elem")],
        build::subselect(build::query(carry), "c", None),
    );
    let on = (kind == JoinKind::LeftLateral).then(|| build::bool_lit(true));
    let from = build::join(table(), build::subselect(build::query(explode), "u", None), kind, on).unwrap();
    build::query(select(vec![proj(build::derived(Some("u"), "elem"), "elem")], from))
}

#[test]
fn a_lateral_explosion_of_a_carried_list_is_a_marker() {
    for kind in [JoinKind::CrossLateral, JoinKind::LeftLateral] {
        let out = rewrite(&mut LateralUnnestAsMarker, carried(kind, None)).unwrap();
        assert!(out.is_new());
        let SqlQuery::Select(s) = out.into_inner() else { panic!("a select") };
        let SqlSource::Join { right, kind: k, .. } = &s.from else { panic!("a join") };
        assert_eq!(*k, kind);
        let SqlSource::Subselect { inner, alias, .. } = right.as_ref() else { panic!("a subselect") };
        assert_eq!(alias, "u");
        assert!(is_marker(inner));
        let SqlQuery::Select(m) = inner.as_ref() else { panic!("a select") };
        // the carried list inlined, the projection's name kept
        assert_eq!(m.projections[0].expr, unnest(col("t0", "xs", list_type(Scalar::Bigint))));
        assert_eq!(m.projections[0].alias.as_deref(), Some("elem"));
    }
    // a carry with a filter, and a join that is no lateral, are left as they came
    let filtered = carried(JoinKind::CrossLateral, Some(build::bool_lit(true)));
    assert!(!rewrite(&mut LateralUnnestAsMarker, filtered).unwrap().is_new());
    let plain = build::query(select(
        vec![],
        build::join(table(), build::subselect(build::query(marker()), "u", None), JoinKind::Cross, None).unwrap(),
    ));
    assert!(!rewrite(&mut LateralUnnestAsMarker, plain).unwrap().is_new());
}

#[test]
fn a_select_list_unnest_is_a_lateral_marker_named_by_the_pass() {
    let xs = col("t0", "xs", list_type(Scalar::Bigint));
    let out = build::output_col("e", scalar(Scalar::Bigint), true);
    let exploding = || {
        let p = Projection { expr: unnest(xs.clone()), alias: Some("e".into()), out: Some(out.clone()) };
        build::query(select(vec![p, proj(build::int_lit(1), "k")], table()))
    };
    let mut pass = SelectListUnnestAsLateral::default();
    for n in 0..2 {
        let rewritten = rewrite(&mut pass, exploding()).unwrap();
        assert!(rewritten.is_new());
        let SqlQuery::Select(s) = rewritten.into_inner() else { panic!("a select") };
        let SqlSource::Join { right, kind, on, .. } = &s.from else { panic!("a join") };
        assert_eq!((*kind, on.is_none()), (JoinKind::CrossLateral, true));
        let SqlSource::Subselect { alias, .. } = right.as_ref() else { panic!("a subselect") };
        // the pass object counts its markers
        assert_eq!(alias, &format!("__unnest{n}"));
        assert_eq!(
            s.projections[0].expr,
            build::typed_column(Some(alias), "e", scalar(Scalar::Bigint), true, Origin::Derived)
        );
        assert_eq!(s.projections[1].expr, build::int_lit(1));
        assert_eq!(s.outputs[0], OutputCol { origin: Origin::Derived, ..out.clone() });
    }
    // a fresh pass starts over
    let mut fresh = SelectListUnnestAsLateral::default();
    let SqlQuery::Select(s) = rewrite(&mut fresh, exploding()).unwrap().into_inner() else { panic!("a select") };
    assert!(matches!(&s.from, SqlSource::Join { right, .. }
        if matches!(right.as_ref(), SqlSource::Subselect { alias, .. } if alias == "__unnest0")));
    // over no source: the marker is the FROM
    let dual = select(vec![proj(unnest(xs.clone()), "e"), proj(build::int_lit(1), "k")], SqlSource::Dual);
    let SqlQuery::Select(s) = rewrite(&mut fresh, build::query(dual)).unwrap().into_inner() else {
        panic!("a select")
    };
    assert!(matches!(&s.from, SqlSource::Subselect { alias, inner, .. } if alias == "__unnest1" && is_marker(inner)));
    // a select that groups, an UNNEST over an aggregate, and a marker are left as they came
    let mut grouped = select(vec![proj(unnest(xs.clone()), "e")], table());
    grouped.group_by.push(build::int_lit(1));
    assert!(!rewrite(&mut fresh, build::query(grouped)).unwrap().is_new());
    let aggregate = build::reducer(AggFn::List, vec![build::int_lit(1)], false, Vec::new());
    let over_aggregate = select(vec![proj(unnest(SqlExpr::Reducer(aggregate)), "e")], table());
    assert!(!rewrite(&mut fresh, build::query(over_aggregate)).unwrap().is_new());
    assert!(!rewrite(&mut fresh, build::query(marker())).unwrap().is_new());
}
