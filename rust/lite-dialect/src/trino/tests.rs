// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! Trino's hooks held to their Java (`Trino.java`) where the golden oracles cannot reach them: the arms a helper's one
//! caller never passes (`listCall`'s LIST_REVERSE, RANGE_FN, REPEAT_VALUE and default, `bitOp`'s default), a list
//! lambda that is no lambda (Java's `ClassCastException`, an answer the arms oracle could not record), the frame
//! intervals' overflow, a replacement's non-ASCII group digit, the literals' extreme magnitudes, the identifier rule
//! and the pass list. The oracles hold the rest (`tests/dialect_differential.rs`, `mir/dialect-arms.jsonl`).

use super::functions::java_replacement;
use super::*;
use crate::build::{call, float_lit, int_lit, string_lit};

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
    Trino::new().expr(&mut w, e, 0)?;
    w.into_text()
}

fn render(e: &SqlExpr) -> String {
    render_result(e).unwrap_or_else(|r| panic!("{r}"))
}

fn list_call(f: SqlFn, args: &[SqlExpr]) -> Result<String, Refusal> {
    let mut w = SqlWriter::new();
    Trino::new().list_call(&mut w, f, args)?;
    w.into_text()
}

#[test]
fn the_list_calls_no_call_arm_reaches_answer_as_java() {
    let xs = list("xs", Scalar::Bigint);
    // LIST_REVERSE: `call` spells it `reverse(xs)` itself; listCall's own arm reads the empty collection too
    assert_eq!(list_call(SqlFn::ListReverse, std::slice::from_ref(&xs)).unwrap(), "reverse(\"t0\".\"xs\")");
    assert_eq!(
        list_call(SqlFn::ListReverse, &[crate::build::null_lit()]).unwrap(),
        "reverse(CAST(NULL AS ARRAY(UNKNOWN)))"
    );
    for f in [SqlFn::RangeFn, SqlFn::RepeatValue] {
        let r = list_call(f, std::slice::from_ref(&xs)).unwrap_err();
        assert_eq!((r.class, r.message), (Refusal::ILLEGAL_STATE, format!("{f} is rendered by call")));
    }
    let r = list_call(SqlFn::Abs, std::slice::from_ref(&xs)).unwrap_err();
    assert_eq!((r.class, r.message.as_str()), (Refusal::ILLEGAL_STATE, "not a list call: ABS"));
    // the first argument is read first, as Java's `args.get(0)`
    let r = list_call(SqlFn::ListSort, &[]).unwrap_err();
    assert_eq!((r.class, r.message.as_str()), (Refusal::INDEX_OUT_OF_BOUNDS, "Index 0 out of bounds for length 0"));
    // bitOp's default: its one caller passes only its five operators
    let mut w = SqlWriter::new();
    let r = Trino::new().bit_op(&mut w, SqlFn::Abs, &[int_lit(1), int_lit(2)]).unwrap_err();
    assert_eq!((r.class, r.message.as_str()), (Refusal::ILLEGAL_STATE, "not a bit op: ABS"));
}

#[test]
fn a_list_lambda_that_is_no_lambda_is_javas_class_cast() {
    // Java's `((SqlExpr.Lambda) args.get(1))`: a ClassCastException, its message less the JVM's class loader
    let xs = list("xs", Scalar::Bigint);
    for f in [SqlFn::ListTransform, SqlFn::ListFilter] {
        let r = render_result(&call(f, vec![xs.clone(), int_lit(1)])).unwrap_err();
        assert_eq!(
            (r.class, r.message.as_str(), r.kind()),
            (
                Refusal::CLASS_CAST,
                "class com.legend.sql.SqlExpr$IntLit cannot be cast to class com.legend.sql.SqlExpr$Lambda",
                "CLASS_CAST"
            ),
            "{f}"
        );
    }
    let r = render_result(&call(SqlFn::ListExists, vec![xs.clone(), col("c", typed(Scalar::Boolean))])).unwrap_err();
    assert_eq!(r.message, "class com.legend.sql.SqlExpr$Column cannot be cast to class com.legend.sql.SqlExpr$Lambda");
    // a lambda of the element and its index: DuckDB's form over zip_with; LIST_EXISTS takes the element alone
    let pair = crate::build::lambda(&["x", "i"], col("b", typed(Scalar::Boolean)));
    assert_eq!(
        render(&call(SqlFn::ListTransform, vec![xs.clone(), pair.clone()])),
        "zip_with(\"t0\".\"xs\", slice(sequence(1, cardinality(\"t0\".\"xs\") + 1), 1, cardinality(\"t0\".\"xs\")), \
         (\"x\", \"i\") -> \"t0\".\"b\")"
    );
    let r = render_result(&call(SqlFn::ListExists, vec![xs, pair])).unwrap_err();
    assert_eq!(r.message, "LIST_EXISTS with an element index reached Trino: its lambdas take the element alone");
}

#[test]
fn a_frame_offset_is_a_trino_interval() {
    use super::aggregates::interval;
    assert_eq!(interval(3, "DAYS").unwrap(), "INTERVAL '3' DAY");
    assert_eq!(interval(3, "day").unwrap(), "INTERVAL '3' DAY");
    assert_eq!(interval(2, "WEEKS").unwrap(), "INTERVAL '14' DAY");
    assert_eq!(interval(1, "months").unwrap(), "INTERVAL '1' MONTH");
    // Math.multiplyExact's overflow
    let r = interval(i64::MAX / 2, "WEEKS").unwrap_err();
    assert_eq!((r.class, r.message.as_str()), ("java.lang.ArithmeticException", "long overflow"));
    let r = interval(1, "QUARTERS").unwrap_err();
    assert_eq!(r, Refusal::capability("a QUARTERS window frame offset has no Trino interval"));
}

#[test]
fn a_replacement_reads_duckdbs_groups_as_javas() {
    assert_eq!(java_replacement(r"a\1b$c\.", 0), r"a$1b\$c\.");
    assert_eq!(java_replacement(r"\0\2", 1), "$1$3");
    // Character.isDigit and Character.digit: an Arabic-Indic three is group 3
    assert_eq!(java_replacement("\\\u{0663}x", 0), "$3x");
    assert_eq!(java_replacement("\\\u{FF19}", 1), "$10");
    // a trailing backslash is itself
    assert_eq!(java_replacement("x\\", 0), "x\\");
}

#[test]
fn literals_are_trinos() {
    // every integer literal a BIGINT; a Float its plain digits, an extreme one Double.toString's
    assert_eq!(render(&int_lit(-7)), "BIGINT '-7'");
    assert_eq!(render(&float_lit(1.5)), "1.5");
    assert_eq!(render(&float_lit(1e20)), "1.0E20");
    // past a DECIMAL's 38 digits a decimal literal is a DOUBLE; a big integer a DECIMAL
    let big = lite_sql::BigDecimal::parse("1234567890123456789012345678901234567890.5").unwrap();
    assert_eq!(render(&crate::build::decimal_lit(big)), "DOUBLE '1234567890123456789012345678901234567890.5'");
    let huge = lite_sql::BigDecimal::parse("99999999999999999999").unwrap();
    assert_eq!(render(&crate::build::decimal_lit(huge)), "DECIMAL '99999999999999999999'");
    // a timestamp's first T is its space
    assert_eq!(Trino::new().timestamp_lit("2024-01-02T03:04:05.123").unwrap(), "TIMESTAMP '2024-01-02 03:04:05.123'");
    assert_eq!(render(&string_lit("it's")), "'it''s'");
    // the signed hash of no argument hashes an empty concat_ws, as Java writes it
    assert_eq!(
        render(&call(SqlFn::Hash, Vec::new())),
        "from_big_endian_64(xxhash64(to_utf8(coalesce(concat_ws(chr(31), ), ''))))"
    );
}

#[test]
fn identifiers_types_and_names_are_trinos() {
    let d = Trino::new();
    assert_eq!(d.ident("x"), "\"x\"");
    assert_eq!(d.ident("\"date\""), "\"date\"");
    assert_eq!(d.ident("\"a\"\"b\""), "\"a\"\"b\"");
    assert_eq!(d.ident("\"a\"b\""), "\"\"\"a\"\"b\"\"\"");
    assert_eq!(d.ident("\""), "\"\"\"\"");
    let r = d.scalar_type_name(Scalar::DecimalText).unwrap_err();
    assert_eq!(
        (r.class, r.message.as_str()),
        (Refusal::ILLEGAL_STATE, "DECIMAL_TEXT cast reached a dialect without DECIMAL_TEXT support")
    );
    assert_eq!(d.typeof_name(Scalar::Date).unwrap(), "date");
    assert_eq!(
        d.typeof_name(Scalar::Bigint).unwrap_err(),
        Refusal::capability("a TYPEOF compared with BIGINT's name reached Trino")
    );
    assert_eq!(d.json_kind_name(JsonKind::UnsignedInteger).unwrap(), "UBIGINT");
    let row = SqlType::Struct {
        fields: vec![
            StructField { name: "a".into(), r#type: SqlType::Scalar(Scalar::Bigint) },
            StructField {
                name: "b".into(),
                r#type: SqlType::Array { element: Box::new(SqlType::Scalar(Scalar::Date)) },
            },
        ],
    };
    assert_eq!(cast_type_name(&d, &row).unwrap(), "ROW(\"a\" BIGINT, \"b\" ARRAY(DATE))");
    // the two writers no pass leaves a node for
    let r = d.as_of_join_clause().unwrap_err();
    assert_eq!(r.class, Refusal::ILLEGAL_STATE);
    assert_eq!(r.message, "an as-of join reached the join clause: Trino writes it as a pick (AsOfAsPick)");
}

#[test]
fn the_passes_are_javas_in_order() {
    let names: Vec<&str> = Trino::new().passes().iter().map(|p| p.name()).collect();
    assert_eq!(
        names,
        [
            "CarrierStrategies",
            "PivotAsGroupBy",
            "AsOfAsPick",
            "StarExceptToColumns",
            "ExplosionMarkers.LateralUnnestAsMarker",
            "ExplosionMarkers.SelectListUnnestAsLateral",
            "ExplosionMarkers.StarOverExplosionUnwrapped",
            "CheckedDefectsToLists",
            "SubstringClamp",
            "AggregatesDeliverDouble",
            "QualifyToSubselect",
            "ConstantKeysAsExpressions",
            "RootNumericTypes",
        ]
    );
}
