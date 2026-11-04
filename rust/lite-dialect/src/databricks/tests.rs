// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! Databricks' hooks held to their Java (`Databricks.java`) where the golden oracles cannot reach them: the arms a
//! helper's one caller never passes (`listCall`'s LIST_REVERSE, RANGE_FN, REPEAT_VALUE and default, `bitOp`'s
//! default), the two answers the arms oracle could not record (a list lambda that is no lambda, a pivot of AVG: Java's
//! `ClassCastException`s, their messages less the JVM's class loader), the session's temporary view (no oracle line
//! asks `sessionTable`), the frame intervals and offsets, the 1-based index read from Spark's 0-based one, the quoted
//! split delimiter, the literals and identifiers, and the pass list. The oracles hold the rest
//! (`tests/dialect_differential.rs`, `mir/dialect-arms.jsonl`).

use super::functions::java_replacement;
use super::*;
use crate::SqlDialect;
use crate::build::{call, float_lit, int_lit, null_lit, string_lit};
use lite_sql::{DdlColumn, JavaNumber, PivotUsing};

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
    Databricks::new().expr(&mut w, e, 0)?;
    w.into_text()
}

fn render(e: &SqlExpr) -> String {
    render_result(e).unwrap_or_else(|r| panic!("{r}"))
}

fn list_call(f: SqlFn, args: &[SqlExpr]) -> Result<String, Refusal> {
    let mut w = SqlWriter::new();
    Databricks::new().list_call(&mut w, f, args)?;
    w.into_text()
}

fn out(name: &str, t: SqlType) -> OutputCol {
    OutputCol { name: name.into(), r#type: t, nullable: true, origin: Origin::Physical }
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

#[test]
fn the_list_calls_no_call_arm_reaches_answer_as_java() {
    let xs = list("xs", Scalar::Bigint);
    // LIST_REVERSE: `call` spells it `reverse(xs)` itself; listCall's own arm reads the empty collection too
    assert_eq!(list_call(SqlFn::ListReverse, std::slice::from_ref(&xs)).unwrap(), "reverse(`t0`.`xs`)");
    assert_eq!(list_call(SqlFn::ListReverse, &[null_lit()]).unwrap(), "reverse(CAST(NULL AS ARRAY<VOID>))");
    for f in [SqlFn::RangeFn, SqlFn::RepeatValue] {
        let r = list_call(f, std::slice::from_ref(&xs)).unwrap_err();
        assert_eq!((r.class, r.message), (Refusal::ILLEGAL_STATE, format!("{f} is rendered by call")));
    }
    let r = list_call(SqlFn::Abs, std::slice::from_ref(&xs)).unwrap_err();
    assert_eq!((r.class, r.message.as_str()), (Refusal::ILLEGAL_STATE, "not a list call: ABS"));
    let r = list_call(SqlFn::ListSort, &[]).unwrap_err();
    assert_eq!((r.class, r.message.as_str()), (Refusal::INDEX_OUT_OF_BOUNDS, "Index 0 out of bounds for length 0"));
    let mut w = SqlWriter::new();
    let r = Databricks::new().bit_op(&mut w, SqlFn::Abs, &[int_lit(1), int_lit(2)]).unwrap_err();
    assert_eq!((r.class, r.message.as_str()), (Refusal::ILLEGAL_STATE, "not a bit op: ABS"));
}

#[test]
fn a_list_lambda_that_is_no_lambda_is_javas_class_cast() {
    // Java's `(SqlExpr.Lambda) args.get(1)`: a ClassCastException, its message less the JVM's class loader
    let xs = list("xs", Scalar::Bigint);
    for f in [SqlFn::ListTransform, SqlFn::ListFilter] {
        let r = render_result(&call(f, vec![xs.clone(), col("n", typed(Scalar::Bigint))])).unwrap_err();
        assert_eq!(
            (r.class, r.message.as_str(), r.kind()),
            (
                Refusal::CLASS_CAST,
                "class com.legend.sql.SqlExpr$Column cannot be cast to class com.legend.sql.SqlExpr$Lambda",
                "CLASS_CAST"
            ),
            "{f}"
        );
    }
}

#[test]
fn a_pivot_of_avg_is_javas_class_cast() {
    // AggregatesDeliverDouble casts the pivot's AVG to DOUBLE, which the walk then reads back as a reducer: Java's
    // `(SqlAgg.Reducer) rewriteExpr(u.agg())` fails
    let px = SqlType::Decimal { precision: 18, scale: 6 };
    let pivot = |f: AggFn| SqlSource::Pivot {
        source: Box::new(table("t0", vec![out("id", SqlType::Scalar(Scalar::Bigint)), out("px", px.clone())])),
        on: vec![col("sym", typed(Scalar::Varchar))],
        r#in: vec![string_lit("A")],
        named: Vec::new(),
        usings: vec![PivotUsing {
            agg: build::reducer(f, vec![build::derived(None, "px")], false, Vec::new()),
            alias: "s".into(),
            r#type: Some(SqlType::Scalar(Scalar::Double)),
        }],
        alias: "p".into(),
        outputs: vec![out("id", SqlType::Scalar(Scalar::Bigint)), out("A__|__s", SqlType::Scalar(Scalar::Double))],
    };
    let query = |f| build::query(build::star_of(pivot(f)).unwrap());
    let r = SqlDialect::render(&Databricks::new(), query(AggFn::Avg)).unwrap_err();
    assert_eq!(
        (r.class, r.message.as_str()),
        (
            Refusal::CLASS_CAST,
            "class com.legend.sql.SqlExpr$Cast cannot be cast to class com.legend.sql.SqlAgg$Reducer"
        )
    );
    // any other aggregate pivots natively, the value's column named by its value and the aggregate's alias
    assert_eq!(
        SqlDialect::render(&Databricks::new(), query(AggFn::Sum)).unwrap(),
        "SELECT *\nFROM (SELECT * FROM `trades` AS `t0` PIVOT (SUM(`px`) AS `s` FOR `sym` IN ('A' AS `A__|__s`))) AS \
         `p`"
    );
}

fn create_table(temporary: bool) -> SqlDdl {
    SqlDdl::CreateTable {
        schema: None,
        table: "legend_rows".into(),
        quoting: NameQuoting::default(),
        columns: vec![
            DdlColumn {
                name: "ID".into(),
                declared_quoted: false,
                r#type: ColumnType::Plain { kind: ColumnKind::Integer },
                not_null: true,
                primary_key: true,
            },
            DdlColumn {
                name: "Name".into(),
                declared_quoted: true,
                r#type: ColumnType::Sized { kind: "VARCHAR".into(), size: 20 },
                not_null: false,
                primary_key: false,
            },
        ],
        temporary,
    }
}

fn rows(cells: Vec<Vec<SqlExpr>>) -> SqlDml {
    SqlDml::InsertValues {
        schema: None,
        table: "legend_rows".into(),
        quoting: NameQuoting::default(),
        columns: vec!["ID".into(), "Name".into()],
        quoted_columns: vec!["Name".into()],
        rows: cells,
    }
}

#[test]
fn a_session_table_is_a_temporary_view_over_its_rows() {
    let d = Databricks::new();
    let held = rows(vec![vec![int_lit(1), string_lit("it's")], vec![int_lit(2), null_lit()]]);
    assert_eq!(
        d.session_table(&create_table(true), Some(&held)).unwrap(),
        ["CREATE TEMPORARY VIEW `legend_rows` AS SELECT CAST(`c0` AS INTEGER) AS `ID`, CAST(`c1` AS VARCHAR(20)) AS \
          `Name` FROM VALUES (1L, 'it\\'s'), (2L, NULL) AS `t`(`c0`, `c1`)"]
    );
    // no rows: each column a typed NULL, no row
    assert_eq!(
        d.session_table(&create_table(true), None).unwrap(),
        ["CREATE TEMPORARY VIEW `legend_rows` AS SELECT CAST(NULL AS INTEGER) AS `ID`, CAST(NULL AS VARCHAR(20)) AS \
          `Name` WHERE FALSE"]
    );
    let r = d.session_table(&create_table(false), None).unwrap_err();
    assert_eq!((r.class, r.message.as_str()), (Refusal::ILLEGAL_ARGUMENT, "a session table is temporary: legend_rows"));
    // a row holds values: a plan parameter is refused, naming the bind
    let p = SqlExpr::PlanParam {
        name: "p".into(),
        kind: lite_sql::ParamKind::Other,
        optional: false,
        enum_map_fn: None,
        r#type: typed(Scalar::Bigint),
        by_value: None,
    };
    let r = d.session_table(&create_table(true), Some(&rows(vec![vec![p, null_lit()]]))).unwrap_err();
    assert_eq!(r.class, Refusal::DIALECT_CAPABILITY);
    assert!(r.message.starts_with("plan parameters [Bind[parameter=p, "), "{}", r.message);
    assert!(r.message.ends_with(" in a session table's rows: a row holds values, and a DDL statement is text"));
}

#[test]
fn a_frame_offset_is_in_its_keys_type_and_an_interval_one_type() {
    use super::aggregates::interval;
    assert_eq!(interval(3, "DAYS").unwrap(), "INTERVAL '3 00:00:00' DAY TO SECOND");
    assert_eq!(interval(2, "weeks").unwrap(), "INTERVAL '14 00:00:00' DAY TO SECOND");
    assert_eq!(interval(90061, "SECONDS").unwrap(), "INTERVAL '1 01:01:01' DAY TO SECOND");
    assert_eq!(interval(14, "MONTHS").unwrap(), "INTERVAL '1-2' YEAR TO MONTH");
    assert_eq!(interval(2, "YEAR").unwrap(), "INTERVAL '2-0' YEAR TO MONTH");
    // a negative count: each field signed, as Java's String.format("%02d") writes a negative number
    assert_eq!(interval(-1, "HOURS").unwrap(), "INTERVAL '0 -1:00:00' DAY TO SECOND");
    let r = interval(i64::MAX, "HOURS").unwrap_err();
    assert_eq!((r.class, r.message.as_str()), ("java.lang.ArithmeticException", "long overflow"));
    let r = interval(1, "QUARTERS").unwrap_err();
    assert_eq!(r, Refusal::capability("a QUARTERS window frame offset has no Databricks interval"));
    // a RANGE frame's offset has its ORDER BY key's type; a ranking function's argument is an INT
    let key = |s: Scalar| lite_sql::SortKey {
        expr: col("k", typed(s)),
        ascending: true,
        null_order: None,
        output_name: None,
    };
    let frame = |n: JavaNumber| lite_sql::Frame {
        kind: lite_sql::FrameKind::Range,
        from: lite_sql::Bound::Preceding { n },
        to: lite_sql::Bound::CurrentRow,
    };
    let sum = || SqlAgg::Reducer(build::reducer(AggFn::Sum, vec![col("x", typed(Scalar::Bigint))], false, Vec::new()));
    let windowed =
        |k: Scalar, n: JavaNumber| render(&build::window_call(sum(), Vec::new(), vec![key(k)], Some(frame(n))));
    assert!(windowed(Scalar::Integer, JavaNumber::Long(2)).ends_with("RANGE BETWEEN 2 PRECEDING AND CURRENT ROW)"));
    assert!(windowed(Scalar::Bigint, JavaNumber::Integer(2)).ends_with("RANGE BETWEEN 2L PRECEDING AND CURRENT ROW)"));
    let double = windowed(Scalar::Double, JavaNumber::Long(2));
    assert!(double.ends_with("RANGE BETWEEN CAST(2 AS DOUBLE) PRECEDING AND CURRENT ROW)"), "{double}");
    let ntile = SqlAgg::RankingFn { r#fn: AggFn::Ntile, args: vec![int_lit(4)] };
    assert_eq!(render(&build::window_call(ntile, Vec::new(), Vec::new(), None)), "NTILE(4) OVER (ORDER BY 1)");
}

#[test]
fn a_lambdas_index_is_read_one_based_from_sparks_zero_based() {
    let xs = list("xs", Scalar::Bigint);
    // the index parameter renamed, each unqualified reference to it read as the renamed one plus one
    let i = build::derived(None, "i");
    let qualified = SqlExpr::Column(lite_sql::Column {
        table: Some("t0".into()),
        name: "i".into(),
        r#type: typed(Scalar::Bigint),
        origin: Some(Origin::Physical),
    });
    let body = call(SqlFn::Greater, vec![call(SqlFn::Times, vec![i.clone(), int_lit(2)]), qualified]);
    let l = build::lambda(&["x", "i"], body);
    assert_eq!(
        render(&call(SqlFn::ListFilter, vec![xs.clone(), l])),
        "filter(`t0`.`xs`, (`x`, `i__0`) -> (`i__0` + 1L) * 2L > `t0`.`i`)"
    );
    // a lambda of the element alone is itself
    let one = build::lambda(&["x"], call(SqlFn::IsNotNull, vec![build::derived(None, "x")]));
    assert_eq!(render(&call(SqlFn::ListTransform, vec![xs, one])), "transform(`t0`.`xs`, `x` -> `x` IS NOT NULL)");
}

#[test]
fn literals_identifiers_and_the_quoted_split_are_databricks() {
    let d = Databricks::new();
    // a backslash and a quote escaped by a backslash; a NUL spliced
    assert_eq!(d.string_lit("it's a\\b").unwrap(), "'it\\'s a\\\\b'");
    assert_eq!(d.string_lit("a\0b\0").unwrap(), "('a' || chr(0) || 'b' || chr(0) || '')");
    assert_eq!(render(&int_lit(-7)), "-7L");
    assert_eq!(render(&float_lit(1.5)), "1.5");
    // every identifier in backticks; a double-quoted declaration is its inner name
    assert_eq!(d.ident("x"), "`x`");
    assert_eq!(d.ident("\"date\""), "`date`");
    assert_eq!(d.ident("\"a\"\"b\""), "`a\"b`");
    assert_eq!(d.ident("a`b"), "`a``b`");
    // Spark's split takes a regex: the literal delimiter quoted as Pattern.quote does, \E closed and reopened
    let s = col("s", typed(Scalar::Varchar));
    assert_eq!(render(&call(SqlFn::Split, vec![s.clone(), string_lit("|")])), "split(`t0`.`s`, '\\\\Q|\\\\E', -1)");
    assert_eq!(super::pattern_quote("a\\Eb"), "\\Qa\\E\\\\E\\Qb\\E");
    assert_eq!(
        render(&call(SqlFn::Split, vec![s.clone(), s])),
        "split(`t0`.`s`, concat('\\\\Q', replace(`t0`.`s`, '\\\\E', '\\\\E\\\\\\\\E\\\\Q'), '\\\\E'), -1)"
    );
    // Character.isDigit and Character.digit: an Arabic-Indic three is group 3
    assert_eq!(java_replacement("\\\u{0663}$", 1), "$4\\$");
    let r = d.scalar_type_name(Scalar::Time).unwrap_err();
    assert_eq!(r, Refusal::capability("a time of day: Databricks has no TIME type"));
    let r = d.scalar_type_name(Scalar::DecimalText).unwrap_err();
    assert_eq!(r.class, Refusal::ILLEGAL_STATE);
    assert_eq!(
        d.typeof_name(Scalar::Bigint).unwrap_err(),
        Refusal::capability("a TYPEOF compared with BIGINT's name reached Databricks")
    );
}

#[test]
fn the_passes_are_javas_in_order() {
    let names: Vec<&str> = Databricks::new().passes().iter().map(|p| p.name()).collect();
    assert_eq!(
        names,
        [
            "CarrierStrategies",
            "UnqualifyPivotArgs",
            "CheckedDefectsToLists",
            "SubstringClamp",
            "AggregatesDeliverDouble",
            "ConstantKeysAsExpressions",
            "RootNumericTypes",
        ]
    );
}
