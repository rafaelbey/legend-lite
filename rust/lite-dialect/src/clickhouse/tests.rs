// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! ClickHouse's hooks held to their Java (`ClickHouse.java`, `OuterJoinResidualsAsMatches.java`) where the golden
//! oracles cannot reach them: the arms a helper's one caller never passes (`listCall`'s LIST_REVERSE, RANGE_FN,
//! REPEAT_VALUE and default, `bitOp`'s and `temporal`'s defaults, `outsideDates`' units refused before it, `jodaText`'s
//! millisecond field), the answer the arms oracle could not record (a list lambda that is no lambda: Java's
//! `ClassCastException`, its message less the JVM's class loader), the date range, the literals and identifiers, the
//! per-render SETTINGS state, the wire names and the pass list. The oracles hold the rest
//! (`tests/dialect_differential.rs`, `mir/dialect-arms.jsonl`).

use super::aggregates::{descending, joda_text, wire_type_name};
use super::*;
use crate::SqlDialect;
use crate::build::{call, float_lit, int_lit, null_lit, string_lit};
use lite_sql::{BigDecimal, DateFmtPart, DatePart};

fn typed(s: Scalar) -> TypeFact {
    TypeFact::Typed { r#type: SqlType::Scalar(s), nullable: true }
}

fn col(name: &str, t: TypeFact) -> SqlExpr {
    SqlExpr::Column(lite_sql::Column {
        table: Some("t0".into()),
        name: name.into(),
        r#type: t,
        origin: Some(Origin::Physical),
    })
}

fn list(name: &str, s: Scalar) -> SqlExpr {
    col(name, TypeFact::Typed { r#type: SqlType::Array { element: Box::new(SqlType::Scalar(s)) }, nullable: true })
}

fn render_result(e: &SqlExpr) -> Result<String, Refusal> {
    let mut w = SqlWriter::new();
    ClickHouse::new().expr(&mut w, e, 0)?;
    w.into_text()
}

fn render(e: &SqlExpr) -> String {
    render_result(e).unwrap_or_else(|r| panic!("{r}"))
}

fn list_call(f: SqlFn, args: &[SqlExpr]) -> Result<String, Refusal> {
    let mut w = SqlWriter::new();
    ClickHouse::new().list_call(&mut w, f, args)?;
    w.into_text()
}

/// `SELECT <e> AS "v"`, rendered as a root statement.
fn select_of(e: SqlExpr) -> SqlQuery {
    build::query(
        build::select_of(vec![Projection { expr: e, alias: Some("v".into()), out: None }], SqlSource::Dual, vec![])
            .unwrap(),
    )
}

#[test]
fn the_list_calls_no_call_arm_reaches_answer_as_java() {
    let xs = list("xs", Scalar::Bigint);
    // LIST_REVERSE: `call` spells it `arrayReverse(...)` itself; listCall's own arm reads the empty collection too
    assert_eq!(list_call(SqlFn::ListReverse, std::slice::from_ref(&xs)).unwrap(), r#"arrayReverse("t0"."xs")"#);
    assert_eq!(
        list_call(SqlFn::ListReverse, &[null_lit()]).unwrap(),
        "arrayReverse(CAST([] AS Array(Nullable(Nothing))))"
    );
    for f in [SqlFn::RangeFn, SqlFn::RepeatValue] {
        let r = list_call(f, std::slice::from_ref(&xs)).unwrap_err();
        assert_eq!((r.class, r.message), (Refusal::ILLEGAL_STATE, format!("{f} is rendered by call")));
    }
    let r = list_call(SqlFn::Abs, std::slice::from_ref(&xs)).unwrap_err();
    assert_eq!((r.class, r.message.as_str()), (Refusal::ILLEGAL_STATE, "not a list call: ABS"));
    let r = list_call(SqlFn::ListSort, &[]).unwrap_err();
    assert_eq!((r.class, r.message.as_str()), (Refusal::INDEX_OUT_OF_BOUNDS, "Index 0 out of bounds for length 0"));
    // bitOp's default: its one caller passes the five bit operators only
    let mut w = SqlWriter::new();
    let r = ClickHouse::new().bit_op(&mut w, SqlFn::Plus, &[int_lit(1), int_lit(2)]).unwrap_err();
    assert_eq!((r.class, r.message.as_str()), (Refusal::ILLEGAL_STATE, "not a bit op: PLUS"));
    // temporal's default: its one caller passes the temporal functions only
    let mut w = SqlWriter::new();
    let r = ClickHouse::new().temporal(&mut w, SqlFn::Abs, &[], &TypeFact::Unknown).unwrap_err();
    assert_eq!((r.class, r.message.as_str()), (Refusal::ILLEGAL_STATE, "not a temporal call: ABS"));
}

#[test]
fn a_list_lambda_that_is_no_lambda_is_javas_class_cast() {
    // Java's `(SqlExpr.Lambda) args.get(1)` after the argument is written: its ClassCastException, the message less
    // the JVM's class-loader clause (the arms oracle dropped the answer for that clause)
    let xs = list("xs", Scalar::Bigint);
    let not_a_lambda = col("f", typed(Scalar::Bigint));
    for f in [SqlFn::ListTransform, SqlFn::ListFilter, SqlFn::ListExists, SqlFn::ListForAll] {
        let r = render_result(&call(f, vec![xs.clone(), not_a_lambda.clone()])).unwrap_err();
        assert_eq!(
            (r.class, r.message.as_str()),
            (
                Refusal::CLASS_CAST,
                "class com.legend.sql.SqlExpr$Column cannot be cast to class com.legend.sql.SqlExpr$Lambda"
            ),
            "{f}"
        );
    }
    let lambda = SqlExpr::Lambda(build::lambda_of(vec!["x".into(), "i".into()], build::derived(None, "x")));
    assert_eq!(
        render(&call(SqlFn::ListTransform, vec![xs, lambda])),
        r#"arrayMap(("x", "i") -> "x", "t0"."xs", arrayEnumerate("t0"."xs"))"#
    );
}

#[test]
fn the_helpers_walls_no_caller_reaches_answer_as_java() {
    let d = ClickHouse::new();
    let date = col("d", typed(Scalar::Date));
    for unit in [DatePart::Quarter, DatePart::Nanosecond, DatePart::DayOfYear, DatePart::IsoDayOfWeek] {
        let r = d.outside_dates(unit, &date, &date).err().expect("refused");
        assert_eq!(
            (r.class, r.message),
            (Refusal::ILLEGAL_STATE, format!("the unit {unit} reached the date range check: refused before it"))
        );
    }
    for part in [DateFmtPart::SubsecMin, DateFmtPart::SubsecNano] {
        let r = joda_text(&[DateFmt::Part(part)]).unwrap_err();
        assert_eq!(
            (r.class, r.message),
            (Refusal::ILLEGAL_STATE, format!("a {part} field reached a Joda format: formatted() writes it"))
        );
    }
    assert_eq!(
        joda_text(&[DateFmt::Text { s: "o'clock ".into() }, DateFmt::Part(DateFmtPart::Hour12Nopad)]).unwrap(),
        "'o''clock 'h"
    );
    // a descending key of a type arraySort cannot negate
    let r = descending("k", Some(&SqlType::Scalar(Scalar::Json))).unwrap_err();
    assert_eq!(r.message, "a descending sort key of JSON reached ClickHouse");
    let r = descending("k", None).unwrap_err();
    assert_eq!(r.message, "a descending sort key of no type reached ClickHouse");
}

#[test]
fn literals_answer_as_java() {
    let d = ClickHouse::new();
    // a backslash, a quote and a NUL escaped by a backslash
    assert_eq!(d.string_lit("a\\b'c\0d").unwrap(), r"'a\\b\'c\0d'");
    // a date in ClickHouse's years; the year read by Integer.parseInt
    assert_eq!(d.date_lit("2024-02-29").unwrap(), "toDate32('2024-02-29')");
    assert_eq!(d.date_lit("1900-01-01").unwrap(), "toDate32('1900-01-01')");
    let r = d.date_lit("2300-01-01").unwrap_err();
    assert_eq!(
        (r.class, r.message.as_str()),
        (
            Refusal::DIALECT_CAPABILITY,
            "the date 2300-01-01 reached ClickHouse, whose dates are 1900 to 2299 (it clamps a date past either end)"
        )
    );
    let r = d.date_lit("-0005-01-01").unwrap_err();
    assert!(r.message.starts_with("the date -0005-01-01 reached ClickHouse"), "{r}");
    let r = d.date_lit("12345678901-01-01").unwrap_err();
    assert_eq!((r.class, r.message.as_str()), (Refusal::NUMBER_FORMAT, "For input string: \"12345678901\""));
    let r = d.date_lit("x2024").unwrap_err();
    assert_eq!(
        (r.class, r.message.as_str()),
        (Refusal::ILLEGAL_STATE, "a date literal with no year reached ClickHouse: x2024")
    );
    // a date-time's T a space, its fraction cut to six digits
    assert_eq!(
        d.timestamp_lit("2024-01-02T03:04:05.123456789").unwrap(),
        "toDateTime64('2024-01-02 03:04:05.123456', 6, 'UTC')"
    );
    assert_eq!(d.timestamp_lit("2024-01-02T03:04:05").unwrap(), "toDateTime64('2024-01-02 03:04:05', 6, 'UTC')");
    let r = d.timestamp_lit("9999-12-31T23:59:59.9999999").unwrap_err();
    assert!(r.message.starts_with("the date 9999-12-31 23:59:59.999999 reached"), "{r}");
    // a Float literal an exact Decimal of its digits; NaN and the infinities Float64; an extreme one its text
    assert_eq!(d.float_literal(1.5).unwrap(), "CAST('1.5' AS Decimal(2, 1))");
    assert_eq!(d.float_literal(0.001).unwrap(), "CAST('0.001' AS Decimal(4, 3))");
    assert_eq!(d.float_literal(1.0e7).unwrap(), "CAST('10000000.0' AS Decimal(9, 1))");
    assert_eq!(d.float_literal(f64::NAN).unwrap(), "nan");
    assert_eq!(d.float_literal(f64::INFINITY).unwrap(), "inf");
    assert_eq!(d.float_literal(f64::NEG_INFINITY).unwrap(), "-inf");
    assert_eq!(d.float_literal(1.0e20).unwrap(), "toFloat64('1.0E20')");
    assert_eq!(render(&float_lit(-2.5)), "CAST('-2.5' AS Decimal(2, 1))");
    // a DECIMAL literal past 76 digits is refused; a HUGEINT-typed one is an Int128
    let big = format!("1.{}", "0".repeat(76));
    let r = render_result(&build::decimal_lit(BigDecimal::parse(&big).unwrap())).unwrap_err();
    assert_eq!(r.message, "a decimal literal of 77 digits reached ClickHouse, whose Decimal holds 76");
    let huge = SqlExpr::DecimalLit { value: BigDecimal::parse("1E+20").unwrap(), r#type: typed(Scalar::Hugeint) };
    assert_eq!(render(&huge), "toInt128('100000000000000000000')");
    assert_eq!(render(&int_lit(7)), "toInt64(7)");
    let r = d.time_lit("10:00:00").unwrap_err();
    assert_eq!(r.message, "a time of day: ClickHouse's Time type is experimental");
}

#[test]
fn identifiers_and_types_answer_as_java() {
    let d = ClickHouse::new();
    // every identifier quoted; a double-quoted declaration is its own spelling
    assert_eq!(d.ident("plain"), r#""plain""#);
    assert_eq!(d.ident(r#""Mixed""#), r#""Mixed""#);
    assert_eq!(d.ident(r#""a""b""#), r#""a""b""#);
    assert_eq!(d.ident(r#"a"b"#), r#""a""b""#);
    assert_eq!(d.ident(r#"""#), r#""""""#);
    assert_eq!(d.map_type_name("Nullable(String)", "Nullable(Int64)"), "Map(String, Nullable(Int64))");
    let st = vec![
        lite_sql::StructField { name: "a".into(), r#type: SqlType::Scalar(Scalar::Bigint) },
        lite_sql::StructField {
            name: "b".into(),
            r#type: SqlType::Struct {
                fields: vec![lite_sql::StructField { name: "c".into(), r#type: SqlType::Scalar(Scalar::Varchar) }],
            },
        },
    ];
    assert_eq!(
        d.struct_type_name(&st).unwrap(),
        r#"Tuple("a" Nullable(Int64), "b" Variant(Tuple("c" Nullable(String))))"#
    );
    assert_eq!(d.array_type_name("Nullable(Int64)").unwrap(), "Array(Nullable(Int64))");
}

#[test]
fn the_root_carries_its_settings_and_the_correlated_flag_is_the_roots_own() {
    let d = ClickHouse::new();
    let q = select_of(int_lit(1));
    let plain = format!("SELECT toInt64(1) AS \"v\"\n{SETTINGS}{UNCORRELATED}");
    assert_eq!(AnsiSqlRenderer::render(&d, q.clone()).unwrap(), plain);
    // a subquery written inline carries none; a second render on the same object starts its flag over
    assert_eq!(AnsiSqlRenderer::render(&d, q.clone()).unwrap(), plain);
    assert_eq!(ansi::render_inline(&d, q).unwrap(), format!("SELECT toInt64(1) AS \"v\" {SETTINGS}{UNCORRELATED}"));
    assert_eq!(d.nesting.get(), 0);
}

#[test]
fn the_wire_names_answer_as_java() {
    for (driver, wire) in [
        ("Nullable(Int64)", "BIGINT"),
        (" Nullable( Nullable(UInt32) ) ", "BIGINT"),
        ("Int8", "INTEGER"),
        ("UInt64", "HUGEINT"),
        ("Int128", "HUGEINT"),
        ("Float64", "DOUBLE"),
        ("Float32", "FLOAT"),
        ("Date32", "DATE"),
        ("Nullable(DateTime64(6, 'UTC'))", "TIMESTAMP"),
        ("Decimal(10, 2)", "DECIMAL"),
        ("Tuple(a Int64)", "STRUCT"),
        ("Nothing", "NULL"),
        ("String", "VARCHAR"),
    ] {
        assert_eq!(wire_type_name(Some(driver)), crate::wire_names::fold(Some(wire)), "{driver}");
    }
    assert_eq!(wire_type_name(None), crate::wire_names::fold(None));
    let d = ClickHouse::new();
    assert!(d.delivers_own(&SqlType::Scalar(Scalar::Json), "VARCHAR"));
    assert!(!d.delivers_own(&SqlType::Scalar(Scalar::Json), "JSON"));
    assert!(!d.delivers_own(&SqlType::Scalar(Scalar::Varchar), "VARCHAR"));
    assert_eq!(d.typing_wire_name(Some("Nullable(Int32)")), Some(crate::wire_names::fold(Some("INTEGER"))));
}

#[test]
fn the_passes_are_javas_in_javas_order() {
    let names: Vec<&str> = ClickHouse::new().passes().iter().map(|p| p.name()).collect();
    assert_eq!(
        names,
        [
            "CarrierStrategies",
            "PivotAsGroupBy",
            "OuterJoinResidualsAsMatches",
            "ExplosionMarkers.LateralUnnestAsMarker",
            "ExplosionMarkers.StarOverExplosionUnwrapped",
            "ClickHouse.ArrayJoinColumnsNamed",
            "CheckedDefectsToLists",
            "SubstringClamp",
            "AggregatesDeliverDouble",
            "ConstantKeysAsExpressions",
            "RootNumericTypes",
        ]
    );
}

#[test]
fn a_residual_outer_join_is_its_matches_numbered_per_render() {
    // t0 LEFT JOIN t1 ON t0.a < t1.b: the matches of each distinct t0.a, joined null-safe
    let out = |name: &str| OutputCol {
        name: name.into(),
        r#type: SqlType::Scalar(Scalar::Bigint),
        nullable: true,
        origin: Origin::Physical,
    };
    let table = |alias: &str, c: &str| SqlSource::Table {
        name: format!("T_{alias}"),
        alias: alias.into(),
        outputs: vec![out(c)],
        call: false,
        stored_types: Vec::new(),
        quoting: NameQuoting::default(),
    };
    let a = SqlExpr::Column(build::column_of(Some("t0"), &out("a")));
    let b = SqlExpr::Column(build::column_of(Some("t1"), &out("b")));
    let on = call(SqlFn::Less, vec![a.clone(), b]);
    let join = build::join(table("t0", "a"), table("t1", "b"), JoinKind::Left, Some(on)).unwrap();
    let q = build::query(
        build::select_of(vec![build::projection(a, Some("a".into()), None).unwrap()], join, vec![]).unwrap(),
    );
    let d = ClickHouse::new();
    let sql = AnsiSqlRenderer::render(&d, q.clone()).unwrap();
    assert!(sql.contains(r#"SELECT DISTINCT "t0"."a" AS "__oj0c0""#), "{sql}");
    assert!(sql.contains(r#"isNotDistinctFrom("t0"."a", "t1"."__oj0k0")"#), "{sql}");
    // a second render numbers from 0 again
    assert_eq!(AnsiSqlRenderer::render(&d, q).unwrap(), sql);
    // a right side that is a join has no single alias: Java's `j.right().alias()` refuses
    let nested = build::join(table("t1", "b"), table("t2", "c"), JoinKind::Inner, Some(build::bool_lit(true))).unwrap();
    let outer = build::join(table("t0", "a"), nested, JoinKind::Inner, Some(build::bool_lit(true))).unwrap();
    let q = build::query(
        build::select_of(vec![build::projection(string_lit("x"), Some("x".into()), None).unwrap()], outer, vec![])
            .unwrap(),
    );
    let r = AnsiSqlRenderer::render(&d, q).unwrap_err();
    assert_eq!(
        (r.class, r.message.as_str()),
        (Refusal::ILLEGAL_STATE, "a nested join has no single alias — resolve per side")
    );
}

#[test]
fn a_from_less_subquery_has_no_alias_to_check_its_correlation() {
    // Java's `aliasesOf` asks the subquery's FROM for its alias: a FROM-less one is Dual's IllegalStateException
    let one = build::query(
        build::select_of(vec![build::projection(int_lit(1), Some("one".into()), None).unwrap()], SqlSource::Dual, vec![])
            .unwrap(),
    );
    let r = render_result(&build::exists(one)).unwrap_err();
    assert_eq!(
        (r.class, r.message.as_str()),
        (Refusal::ILLEGAL_STATE, "a Dual (FROM-less) source has no alias — caller bug")
    );
}
