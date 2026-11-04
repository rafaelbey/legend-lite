// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! BigQuery's hooks held to their Java (`BigQuery.java`, `FoldsAsRecursiveCtes.java`) where the golden oracles cannot
//! reach them: the NULL list of unknown element type (a `NullLit` is always bottom-typed, so no built statement holds
//! one), the helpers' defaults their one caller never passes (`maps`, `bitOp`), the closed fold hoisted to a top-level
//! recursive expression, the regular-expression rewrites (`onlyGroup`, `shiftGroups`, FORMAT's conversions), the
//! literals and identifiers, the frame offsets' overflow, the parameter types, the wire names and the pass list. The
//! oracles hold the rest (`tests/dialect_differential.rs`, `mir/dialect-arms.jsonl`).

use super::functions::{conversions, only_group, shift_groups};
use super::*;
use crate::SqlDialect;
use crate::build::{call, int_lit, null_lit, string_lit};
use lite_sql::{BigDecimal, DatePart, JavaNumber};

fn typed(t: SqlType) -> TypeFact {
    TypeFact::Typed { r#type: t, nullable: true }
}

fn col(name: &str, t: TypeFact) -> SqlExpr {
    SqlExpr::Column(lite_sql::Column {
        table: Some("t0".into()),
        name: name.into(),
        r#type: t,
        origin: Some(Origin::Physical),
    })
}

fn list_of(s: Scalar) -> TypeFact {
    typed(SqlType::Array { element: Box::new(SqlType::Scalar(s)) })
}

fn render_result(e: &SqlExpr) -> Result<String, Refusal> {
    let mut w = SqlWriter::new();
    BigQuery::new().expr(&mut w, e, 0)?;
    w.into_text()
}

fn render(e: &SqlExpr) -> String {
    render_result(e).unwrap_or_else(|r| panic!("{r}"))
}

#[test]
fn a_null_list_of_unknown_element_type_is_refused() {
    // a NullLit the builders never make: untyped, not bottom (Java's wall behind `nullList && known == null &&
    // !bottom`)
    let untyped_null = SqlExpr::NullLit { r#type: TypeFact::Unknown };
    let r = render_result(&call(SqlFn::ListPosition, vec![untyped_null.clone(), int_lit(1)])).unwrap_err();
    assert_eq!(
        (r.class, r.message.as_str()),
        (Refusal::DIALECT_CAPABILITY, "a NULL list of unknown element type reached BigQuery")
    );
    // the bottom-typed empty list [] reads as Nil's VARCHAR carrier
    assert_eq!(
        render(&call(SqlFn::ListPosition, vec![null_lit(), int_lit(1)])),
        "(SELECT MIN(`__o`) + 1 FROM UNNEST(CAST(NULL AS ARRAY<STRING>)) AS `__e` WITH OFFSET AS `__o` WHERE `__e` IS \
         NOT DISTINCT FROM 1)"
    );
}

#[test]
fn the_helpers_defaults_no_caller_reaches_answer_as_java() {
    let d = BigQuery::new();
    let mut w = SqlWriter::new();
    let r = d.maps(&mut w, SqlFn::Abs, &[], &TypeFact::Unknown).unwrap_err();
    assert_eq!((r.class, r.message.as_str()), (Refusal::ILLEGAL_STATE, "not a map call: ABS"));
    let mut w = SqlWriter::new();
    let r = d.bit_op(&mut w, SqlFn::Plus, &[int_lit(1), int_lit(2)]).unwrap_err();
    assert_eq!((r.class, r.message.as_str()), (Refusal::ILLEGAL_STATE, "not a bit op: PLUS"));
    // the right shift is arithmetic: a negative value shifts its complement
    let mut w = SqlWriter::new();
    d.bit_op(&mut w, SqlFn::BitShiftRight, &[int_lit(-16), int_lit(2)]).unwrap();
    assert_eq!(
        w.into_text().unwrap(),
        "(CASE WHEN CAST(-16 AS INT64) < 0 THEN ~((~CAST(-16 AS INT64)) >> CAST(2 AS INT64)) ELSE CAST(-16 AS INT64) >> \
         CAST(2 AS INT64) END)"
    );
}

#[test]
fn a_closed_fold_is_hoisted_to_a_top_level_recursive_expression() {
    let xs = col("xs", list_of(Scalar::Bigint));
    let ints = build::array_lit(vec![int_lit(1), int_lit(2)]);
    let body = call(SqlFn::Plus, vec![build::derived(None, "acc"), build::derived(None, "x")]);
    let closed = build::fold_call(
        ints.clone(),
        build::lambda_of(vec!["x".into(), "acc".into()], body.clone()),
        int_lit(0),
        false,
        true,
    );
    let q = build::query(
        build::select_of(vec![build::projection(closed, Some("v".into()), None).unwrap()], SqlSource::Dual, vec![])
            .unwrap(),
    );
    let mut pass = folds::FoldsAsRecursiveCtes::default();
    let SqlQuery::With(with) = pass.rewrite_root(q).unwrap() else { panic!("a WITH") };
    assert!(with.recursive);
    assert_eq!(with.ctes.len(), 1);
    assert_eq!(with.ctes[0].name, "__fold0");
    let SqlQuery::Select(s) = with.body.as_ref() else { panic!("a select") };
    assert!(matches!(s.projections[0].expr, SqlExpr::ScalarSubquery { .. }));
    // a fold that reads its row stays where it is
    let open =
        build::fold_call(xs, build::lambda_of(vec!["x".into(), "acc".into()], body.clone()), int_lit(0), false, true);
    let q = build::query(
        build::select_of(vec![build::projection(open, Some("v".into()), None).unwrap()], SqlSource::Dual, vec![])
            .unwrap(),
    );
    let mut pass = folds::FoldsAsRecursiveCtes::default();
    assert!(matches!(pass.rewrite_root(q).unwrap(), SqlQuery::Select(_)));
    // a fold over the NULL list is its init
    let over_null =
        build::fold_call(null_lit(), build::lambda_of(vec!["x".into(), "acc".into()], body), int_lit(7), false, true);
    let q = build::query(
        build::select_of(vec![build::projection(over_null, Some("v".into()), None).unwrap()], SqlSource::Dual, vec![])
            .unwrap(),
    );
    let mut pass = folds::FoldsAsRecursiveCtes::default();
    let SqlQuery::Select(s) = pass.rewrite_root(q).unwrap() else { panic!("a select") };
    assert_eq!(s.projections[0].expr, int_lit(7));
}

#[test]
fn the_regular_expression_rewrites_answer_as_java() {
    // only the kept group captures; a non-capturing, an escaped and a class's parenthesis copied
    assert_eq!(only_group("(a)(b)", 2), "(?:a)(b)");
    assert_eq!(only_group("(a)(b)", 0), "(?:a)(?:b)");
    assert_eq!(only_group(r"\((a)[(]\Q(x)\E(?i)", 1), r"\((a)[(]\Q(x)\E(?i)");
    assert_eq!(only_group("(?P<n>a)(b)", 2), "(?:a)(b)");
    assert_eq!(only_group("(?<n>a)", 1), "(?<n>a)");
    assert_eq!(only_group("[]a](b)", 1), "[]a](b)");
    assert_eq!(only_group(r"\Qab", 0), r"\Qab");
    // a replacement's groups moved past the prefix and the pattern's own
    assert_eq!(shift_groups(r"\0-\1x\\").unwrap(), r"\2-\3x\\");
    let r = shift_groups(r"\8").unwrap_err();
    assert_eq!(r.message, "a first-match regexp_replace referring to group 8 reached BigQuery (RE2 refers to nine)");
    // a non-ASCII digit's number is its code unit less '0' (Java's `r.charAt(i) - '0'`)
    let r = shift_groups("\\\u{0661}").unwrap_err();
    assert_eq!(r.message, "a first-match regexp_replace referring to group 1585 reached BigQuery (RE2 refers to nine)");
    // FORMAT's conversions: flags, a width, a precision; %% none; a directive with no conversion skipped
    assert_eq!(conversions("%-05.2f %s %% %d %.x %"), vec!['f', 's', '%', 'd']);
    assert_eq!(conversions("%.s"), Vec::<char>::new());
}

#[test]
fn literals_and_identifiers_answer_as_java() {
    let d = BigQuery::new();
    assert_eq!(d.string_lit("a'b\\c?\n\r\t\u{1}\u{7f}é").unwrap(), r"'a\'b\\c\x3f\n\r\t\x01\x7fé'");
    assert_eq!(d.ident("plain"), "`plain`");
    assert_eq!(d.ident(r#""da""te""#), r#"`da"te`"#);
    assert_eq!(d.delimited("a`b\\c"), r"`a\`b\\c`");
    assert_eq!(d.timestamp_lit("2024-01-02T03:04:05.123456789").unwrap(), "DATETIME '2024-01-02 03:04:05.123456'");
    assert_eq!(d.float_literal(1.5).unwrap(), "BIGNUMERIC '1.5'");
    assert_eq!(d.float_literal(f64::NAN).unwrap(), "CAST('nan' AS FLOAT64)");
    assert_eq!(d.float_literal(f64::INFINITY).unwrap(), "CAST('inf' AS FLOAT64)");
    assert_eq!(d.float_literal(f64::NEG_INFINITY).unwrap(), "CAST('-inf' AS FLOAT64)");
    assert_eq!(d.float_literal(1.0e20).unwrap(), "CAST('1.0E20' AS FLOAT64)");
    let big = format!("1{}", "0".repeat(39));
    let r = render_result(&build::decimal_lit(BigDecimal::parse(&big).unwrap())).unwrap_err();
    assert_eq!(
        r.message,
        format!("a decimal literal {big} reached BigQuery, whose BIGNUMERIC holds 38 digits each side of the point")
    );
    // trailing zeros do not count against BIGNUMERIC's digits
    assert_eq!(render(&build::decimal_lit(BigDecimal::parse("1.500").unwrap())), "BIGNUMERIC '1.500'");
    // a list of lists has no type here
    let r = d.array_type_name("ARRAY<INT64>").unwrap_err();
    assert!(r.message.starts_with("a list of lists reached BigQuery"), "{r}");
    assert_eq!(d.map_type_name("STRING", "INT64"), "ARRAY<STRUCT<`key` STRING, `value` INT64>>");
    assert_eq!(render(&string_lit("x")), "'x'");
}

#[test]
fn the_frame_offsets_and_parameter_types_answer_as_java() {
    let d = BigQuery::new();
    let k = col("d", typed(SqlType::Scalar(Scalar::Date)));
    let frame = |n: i64, unit: &str| Frame {
        kind: lite_sql::FrameKind::Range,
        from: lite_sql::Bound::IntervalPreceding { n, unit: unit.into() },
        to: lite_sql::Bound::CurrentRow,
    };
    let order = [build::sort_key(k, true, None, None)];
    let count = SqlAgg::Reducer(build::reducer(AggFn::Count, vec![int_lit(1)], false, vec![]));
    let mut w = SqlWriter::new();
    d.window_call(&mut w, &count, &[], &order, Some(&frame(2, "WEEKS"))).unwrap();
    assert_eq!(
        w.into_text().unwrap(),
        "COUNT(1) OVER (ORDER BY IFNULL(CAST(UNIX_DATE(`t0`.`d`) AS FLOAT64), CAST('inf' AS FLOAT64)) ASC RANGE BETWEEN 14 \
         PRECEDING AND CURRENT ROW)"
    );
    // a day key takes no hour offset
    let mut w = SqlWriter::new();
    let r = d.window_call(&mut w, &count, &[], &order, Some(&frame(2, "HOURS"))).unwrap_err();
    assert_eq!(r.message, "a HOURS window frame offset over this key reached BigQuery");
    // Math.multiplyExact's overflow
    let mut w = SqlWriter::new();
    let r = d.window_call(&mut w, &count, &[], &order, Some(&frame(i64::MAX, "WEEKS"))).unwrap_err();
    assert_eq!((r.class, r.message.as_str()), ("java.lang.ArithmeticException", "long overflow"));
    let rows = Frame {
        kind: lite_sql::FrameKind::Rows,
        from: lite_sql::Bound::Preceding { n: JavaNumber::Long(1) },
        to: lite_sql::Bound::UnboundedFollowing,
    };
    let mut w = SqlWriter::new();
    d.window_call(&mut w, &count, &[], &order, Some(&rows)).unwrap();
    assert_eq!(
        w.into_text().unwrap(),
        "COUNT(1) OVER (ORDER BY (`t0`.`d`) IS NULL ASC, `t0`.`d` ASC ROWS BETWEEN 1 PRECEDING AND UNBOUNDED FOLLOWING)"
    );
    assert_eq!(d.hole_type(ValueKind::DateTime).unwrap(), TypeSpelling::new("DATETIME", Digits::None));
    assert_eq!(
        d.hole_type(ValueKind::DateTimeNanos).unwrap(),
        TypeSpelling { name: "DATETIME".into(), digits: Digits::None, fraction: Some(6) }
    );
    assert!(d.hole_type(ValueKind::Integer).is_err());
    let _ = DatePart::Day;
}

#[test]
fn the_wire_and_the_facts_answer_as_java() {
    let d = BigQuery::new();
    assert_eq!(d.wire_type_name(Some("INT64")), crate::wire_names::fold(Some("BIGINT")));
    assert_eq!(d.wire_type_name(Some("FLOAT64")), crate::wire_names::fold(Some("DOUBLE")));
    assert_eq!(d.wire_type_name(Some("DATETIME")), crate::wire_names::fold(Some("TIMESTAMP")));
    assert_eq!(d.wire_type_name(Some("STRING")), "VARCHAR");
    assert_eq!(d.wire_type_name(None), "");
    let map = SqlType::Map {
        key: Box::new(SqlType::Scalar(Scalar::Varchar)),
        value: Box::new(SqlType::Scalar(Scalar::Bigint)),
    };
    assert!(d.delivers_own(&map, "ARRAY"));
    assert!(d.delivers_own(&SqlType::Scalar(Scalar::Hugeint), "BIGNUMERIC"));
    assert!(d.delivers_own(&SqlType::Scalar(Scalar::Double), "BIGNUMERIC"));
    assert!(d.delivers_own(&SqlType::Decimal { precision: 76, scale: 38 }, "BIGNUMERIC"));
    assert!(!d.delivers_own(&SqlType::Decimal { precision: 77, scale: 38 }, "BIGNUMERIC"));
    assert!(!d.delivers_own(&SqlType::Decimal { precision: 40, scale: 39 }, "BIGNUMERIC"));
    assert!(!d.delivers_own(&SqlType::Scalar(Scalar::Bigint), "BIGNUMERIC"));
    assert_eq!(d.result_column_limit().unwrap(), Some(10000));
    assert_eq!(
        d.typeof_name(Scalar::Date).unwrap_err().message,
        "typeof reached BigQuery: its type names are BigQuery's"
    );
}

#[test]
fn the_passes_are_javas_in_javas_order() {
    let names: Vec<&str> = BigQuery::new().passes().iter().map(|p| p.name()).collect();
    assert_eq!(
        names,
        [
            "CarrierStrategies",
            "FoldsAsRecursiveCtes",
            "SubstringClamp",
            "ExplosionMarkers.LateralUnnestAsMarker",
            "ExplosionMarkers.SelectListUnnestAsLateral",
            "ExplosionMarkers.StarOverExplosionUnwrapped",
            "AggregatesDeliverDouble",
            "ConstantKeysAsExpressions",
            "CommonSubexpressions",
        ]
    );
    let d = BigQuery::new();
    assert!(d.binds_float_text() && d.binds_float_print());
}

#[test]
fn a_mode_asks_its_from_for_an_alias_first_and_a_subquery_reads_the_roots_labels() {
    // `modesCounted` asks the FROM for its alias before its own check: a FROM-less select is Dual's refusal
    let x = col("x", typed(SqlType::Scalar(Scalar::Bigint)));
    let mode = SqlExpr::Reducer(build::reducer(AggFn::Mode, vec![x], false, vec![]));
    let q = build::query(
        build::select_of(vec![build::projection(mode, Some("m".into()), None).unwrap()], SqlSource::Dual, vec![])
            .unwrap(),
    );
    let r = AnsiSqlRenderer::render(&BigQuery::new(), q).unwrap_err();
    assert_eq!(
        (r.class, r.message.as_str()),
        (Refusal::ILLEGAL_STATE, "a Dual (FROM-less) source has no alias — caller bug")
    );
    // a subquery written inline is at depth 0 too: its NULL by shape is cast to the root's label of its position
    let nulls = call(SqlFn::Coalesce, vec![null_lit(), null_lit()]);
    let inner = build::query(
        build::select_of(vec![build::projection(nulls, Some("n".into()), None).unwrap()], SqlSource::Dual, vec![])
            .unwrap(),
    );
    let label =
        OutputCol { name: "v".into(), r#type: SqlType::Scalar(Scalar::Varchar), nullable: true, origin: Origin::Derived };
    let root = build::query(
        build::select_of(
            vec![build::projection(build::scalar_subquery(inner), Some("v".into()), Some(label)).unwrap()],
            SqlSource::Dual,
            vec![],
        )
        .unwrap(),
    );
    assert_eq!(
        AnsiSqlRenderer::render(&BigQuery::new(), root).unwrap(),
        "SELECT (SELECT CAST(NULL AS STRING) AS `n`) AS `v`"
    );
}
