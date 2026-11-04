// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! CommonSubexpressions held to its Java (`CommonSubexpressions.java`): a repeated movable node of 40 or more written
//! once, recursively, a lambda's body within itself; what is never moved (a raise, a fresh value, an aggregate's
//! projection, a node naming a lambda's parameter); the walks' and the subqueries' weights; a bind of its own shape
//! movable; and the selects rewritten once per content. (Java-rendered: `mir/sqlite-arms.jsonl` holds it firing on
//! SQLite, the differential its text.)

use super::*;
use crate::rewriter::rewrite;
use lite_sql::AggFn;

fn n() -> SqlExpr {
    build::typed_column(Some("t0"), "n", SqlType::Scalar(Scalar::Bigint), true, Origin::Physical)
}

/// `((n + 1) + 2) ... + k`: 2k + 1 nodes.
fn chain(base: SqlExpr, k: i64) -> SqlExpr {
    (1..=k).fold(base, |x, i| build::call(SqlFn::Plus, vec![x, build::int_lit(i)]))
}

fn times(a: SqlExpr, b: SqlExpr) -> SqlExpr {
    build::call(SqlFn::Times, vec![a, b])
}

fn shared_of(e: &SqlExpr) -> Option<SqlExpr> {
    CommonSubexpressions::new().shared(e).unwrap()
}

/// The bind `e` is: its alias, the expression it writes once, and the expression over it.
fn bind_of(e: &SqlExpr) -> (String, SqlExpr, SqlExpr) {
    let Some((use_, bind)) = bind_shape(e) else { panic!("no bind: {e:?}") };
    let SqlSource::Subselect { alias, .. } = &use_.from else { panic!("a subselect") };
    (alias.clone(), bind.projections[0].expr.clone(), use_.projections[0].expr.clone())
}

fn reference(alias: &str) -> SqlExpr {
    build::typed_column(Some(alias), "v", SqlType::Scalar(Scalar::Bigint), true, Origin::Derived)
}

#[test]
fn a_repeated_large_node_is_written_once() {
    let x = chain(n(), 20);
    assert_eq!(size(&x), 41);
    let (alias, inner, outer) = bind_of(&shared_of(&times(x.clone(), x.clone())).expect("bound"));
    assert_eq!(alias, "__cse0");
    assert_eq!(inner, x);
    assert_eq!(outer, times(reference("__cse0"), reference("__cse0")));
    // under 40 nodes nothing is bound
    let small = chain(n(), 19);
    assert_eq!(size(&small), 39);
    assert_eq!(shared_of(&times(small.clone(), small)), None);
    // once is no repeat
    assert_eq!(shared_of(&times(x, n())), None);
}

#[test]
fn the_largest_is_bound_first_then_within_each_side() {
    let x = chain(n(), 20);
    let y = build::call(SqlFn::Minus, vec![x.clone(), x.clone()]);
    let (alias, inner, outer) = bind_of(&shared_of(&times(y.clone(), y)).expect("bound"));
    // the outer bind is taken first; the inner one, bound within the node, next
    assert_eq!(alias, "__cse0");
    assert_eq!(outer, times(reference("__cse0"), reference("__cse0")));
    let (inner_alias, x_bound, minus) = bind_of(&inner);
    assert_eq!(inner_alias, "__cse1");
    assert_eq!(x_bound, x);
    assert_eq!(minus, build::call(SqlFn::Minus, vec![reference("__cse1"), reference("__cse1")]));
}

#[test]
fn a_lambdas_body_is_bound_within_itself_and_never_above_it() {
    let e = build::derived(None, "e");
    let z = chain(e, 20);
    let xs = build::typed_column(
        Some("t0"),
        "xs",
        SqlType::Array { element: Box::new(SqlType::Scalar(Scalar::Bigint)) },
        true,
        Origin::Physical,
    );
    let transform = build::call(SqlFn::ListTransform, vec![xs.clone(), build::lambda(&["e"], times(z.clone(), z.clone()))]);
    let out = shared_of(&transform).expect("bound within the lambda");
    let SqlExpr::Call { args, .. } = &out else { panic!("a call") };
    let SqlExpr::Lambda(l) = &args[1] else { panic!("a lambda") };
    let (alias, inner, _) = bind_of(&l.body);
    assert_eq!((alias.as_str(), inner), ("__cse0", z.clone()));
    // a repeated node naming a lambda's parameter is never bound above the lambda
    let once = build::call(SqlFn::ListTransform, vec![xs, build::lambda(&["e"], z)]);
    assert_eq!(shared_of(&times(once.clone(), once)), None);
}

#[test]
fn what_raises_draws_afresh_or_aggregates_is_never_moved() {
    let x = chain(n(), 20);
    let parse = build::call(SqlFn::ParseInt, vec![build::call(SqlFn::Concat, vec![x.clone(), build::string_lit("")])]);
    assert_eq!(shared_of(&times(parse.clone(), parse)), None);
    let guid = build::call(SqlFn::Concat, vec![x.clone(), build::call(SqlFn::Guid, vec![])]);
    assert_eq!(shared_of(&times(guid.clone(), guid)), None);
    // a projection with an aggregate anywhere is left as it is
    let sum = SqlExpr::Reducer(build::reducer(AggFn::Sum, vec![x.clone()], false, Vec::new()));
    let s = build::select_of(
        vec![build::projection(times(times(x.clone(), x), sum), Some("v".into()), None).unwrap()],
        SqlSource::Dual,
        Vec::new(),
    )
    .unwrap();
    assert!(!rewrite(&mut CommonSubexpressions::new(), build::query(s)).unwrap().is_new());
}

#[test]
fn a_walk_and_a_subquery_weigh_more() {
    let s = build::typed_column(Some("t0"), "s", SqlType::Scalar(Scalar::Varchar), true, Origin::Physical);
    let walk = build::call(SqlFn::RegexpExtract, vec![s.clone(), build::string_lit("a")]);
    assert_eq!(size(&walk), 202);
    let (alias, inner, _) = bind_of(&shared_of(&build::call(SqlFn::Concat, vec![walk.clone(), walk.clone()])).unwrap());
    assert_eq!((alias.as_str(), inner), ("__cse0", walk));
    let split = build::call(SqlFn::Split, vec![s, build::string_lit(",")]);
    assert_eq!(size(&split), 22);
}

#[test]
fn a_bind_of_its_own_shape_is_movable() {
    let x = chain(n(), 20);
    let bound = shared_of(&times(x.clone(), x)).unwrap();
    assert!(is_bind(&bound) && movable(&bound));
    // a scalar subquery of any other shape is not
    let other = build::scalar_subquery(build::query(
        build::select_of(
            vec![build::projection(build::int_lit(1), Some("v".into()), None).unwrap()],
            build::subselect(build::query(build::star_of(SqlSource::Dual).unwrap()), "a", None),
            Vec::new(),
        )
        .map(|mut s| {
            s.limit = Some(1);
            s
        })
        .unwrap(),
    ));
    assert!(!movable(&other));
}

#[test]
fn equal_selects_share_one_rewrite() {
    let x = chain(n(), 20);
    let select = |e: SqlExpr| {
        build::query(
            build::select_of(vec![build::projection(e, Some("v".into()), None).unwrap()], SqlSource::Dual, Vec::new())
                .unwrap(),
        )
    };
    // the second equal select is handed the first's rewrite: its bind's name too
    let union = SqlQuery::Union(
        build::union(vec![select(times(x.clone(), x.clone())), select(times(x.clone(), x.clone()))], true, Vec::new())
            .unwrap(),
    );
    let SqlQuery::Union(u) = rewrite(&mut CommonSubexpressions::new(), union).unwrap().into_inner() else {
        panic!("a union")
    };
    assert_eq!(u.branches[0], u.branches[1]);
    let SqlQuery::Select(s) = &u.branches[0] else { panic!("a select") };
    assert_eq!(bind_of(&s.projections[0].expr).0, "__cse0");
    // an equal select with nothing bound is still another node than the one handed in: its parent is rebuilt
    let plain = SqlQuery::Union(build::union(vec![select(n()), select(n())], true, Vec::new()).unwrap());
    assert!(rewrite(&mut CommonSubexpressions::new(), plain).unwrap().is_new());
    let single = SqlQuery::Union(build::union(vec![select(n()), select(build::int_lit(1))], true, Vec::new()).unwrap());
    assert!(!rewrite(&mut CommonSubexpressions::new(), single).unwrap().is_new());
}

#[test]
fn a_marker_and_a_select_list_unnest_are_left_as_they_are() {
    let x = chain(n(), 20);
    let unnest = build::call(SqlFn::Unnest, vec![build::array_lit(vec![times(x.clone(), x.clone())])]);
    let marker = build::select_of(vec![build::projection(unnest, Some("e".into()), None).unwrap()], SqlSource::Dual, Vec::new())
        .unwrap();
    assert!(!rewrite(&mut CommonSubexpressions::new(), build::query(marker)).unwrap().is_new());
}

#[test]
fn of_two_repeated_nodes_of_one_size_the_first_met_is_bound() {
    // Java's choice is its IdentityHashMap's order (identity hashes): not reproducible; Rust's is the pre-order walk's
    let a = chain(n(), 20);
    let b = chain(build::typed_column(Some("t0"), "m", SqlType::Scalar(Scalar::Bigint), true, Origin::Physical), 20);
    let both = build::call(SqlFn::Plus, vec![times(a.clone(), a.clone()), times(b.clone(), b.clone())]);
    let (alias, inner, _) = bind_of(&shared_of(&both).unwrap());
    assert_eq!((alias.as_str(), inner), ("__cse0", a));
}
