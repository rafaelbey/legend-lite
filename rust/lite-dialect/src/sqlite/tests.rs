// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! SQLite's hooks held to their Java (`SQLite.java`) where the oracles cannot reach them: a non-finite Float literal
//! (Java's FloatLit constructor cannot make one), the per-render state of one dialect object (the fresh names started
//! over at each outermost query render, a DML render counting on, the root's labels and selects popped after a
//! refusal), the pass list, the DDL's type names. The golden dumps and `mir/sqlite-arms.jsonl` (rendered by Java's own
//! SQLite) hold every spelling (`tests/dialect_differential.rs`).

use super::*;
use crate::SqlDialect;
use lite_sql::NameQuoting;

fn n() -> SqlExpr {
    build::typed_column(Some("t0"), "n", SqlType::Scalar(Scalar::Bigint), true, Origin::Physical)
}

fn s() -> SqlExpr {
    build::typed_column(Some("t0"), "s", SqlType::Scalar(Scalar::Varchar), true, Origin::Physical)
}

fn table() -> SqlSource {
    SqlSource::Table {
        name: "t".into(),
        alias: "t0".into(),
        outputs: vec![build::output_col("n", SqlType::Scalar(Scalar::Bigint), true)],
        call: false,
        stored_types: Vec::new(),
        quoting: NameQuoting::default(),
    }
}

/// A large text operand (a dozen nodes): one a repeating spelling binds once.
fn big() -> SqlExpr {
    let mut args = Vec::new();
    for k in 0..6 {
        args.push(s());
        args.push(build::string_lit(&k.to_string()));
    }
    build::call(SqlFn::Concat, args)
}

fn select_of(e: SqlExpr) -> SqlQuery {
    build::query(
        build::select_of(vec![build::projection(e, Some("v".into()), None).unwrap()], table(), Vec::new()).unwrap(),
    )
}

#[test]
fn a_float_literal_is_a_real_and_nan_has_none() {
    let d = Sqlite::new();
    assert_eq!(
        d.float_literal(f64::NAN).unwrap_err(),
        Refusal::capability("a NaN literal reached SQLite, which has no NaN (0.0 / 0 is NULL)")
    );
    assert_eq!(d.float_literal(f64::INFINITY).unwrap(), "9e999");
    assert_eq!(d.float_literal(f64::NEG_INFINITY).unwrap(), "-9e999");
    assert_eq!(d.float_literal(1.0).unwrap(), "1.0");
    assert_eq!(d.float_literal(2.5).unwrap(), "2.5");
    // Java's plain text of a double past its plain range keeps its exponent (mir/sqlite-arms.jsonl renders it so)
    assert_eq!(d.float_literal(1.0e20).unwrap(), "1.0E20");
}

#[test]
fn the_fresh_names_start_over_at_each_render_and_a_dml_render_counts_on() {
    let d = Sqlite::new();
    let lpad = build::call(SqlFn::Lpad, vec![big(), build::int_lit(10), build::string_lit("x")]);
    let first = SqlDialect::render(&d, select_of(lpad.clone())).unwrap();
    assert!(first.contains("AS __b0)"), "{first}");
    // the same dialect object: the next render's names are its own, from 0 (the user's ruling, 2026-11-03)
    assert_eq!(SqlDialect::render(&d, select_of(lpad.clone())).unwrap(), first);
    assert_eq!(d.binds.get(), 1);
    // Java's tempTableAs pushes the root but starts no names over: a DML render counts on
    let temp = SqlDml::CreateTempTableAs { table: "tmp".into(), query: select_of(lpad), columns: Vec::new(), staged: false };
    let made = SqlDialect::render_dml(&d, &temp).unwrap();
    assert!(made.starts_with("CREATE TEMPORARY TABLE tmp AS SELECT (SELECT"), "{made}");
    assert!(made.contains("AS __b1)"), "{made}");
    // and the statement's state is popped, a refusal's too
    assert!(d.roots.borrow().is_empty() && d.root_selects.borrow().is_empty());
    let refused = select_of(build::call(SqlFn::Translate, vec![s(), s(), s()]));
    assert!(SqlDialect::render(&d, refused).is_err());
    assert!(d.roots.borrow().is_empty() && d.root_selects.borrow().is_empty());
    assert!(d.laterals.borrow().is_empty() && d.row_laterals.borrow().is_empty());
}

#[test]
fn a_root_select_is_known_by_its_content() {
    let d = Sqlite::new();
    let root = build::select_of(vec![build::projection(n(), Some("v".into()), None).unwrap()], table(), Vec::new())
        .unwrap();
    let _scope = d.enter(Vec::new());
    // pending until the root is written
    assert!(!d.is_root_select(&root));
    let q = build::query(root.clone());
    let mut w = SqlWriter::new();
    d.query(&mut w, &q, 0).unwrap();
    // Java's identity after CommonSubexpressions (which makes equal selects one object) is equality
    assert!(d.is_root_select(&root.clone()));
    let other = build::select_of(vec![build::projection(s(), Some("v".into()), None).unwrap()], table(), Vec::new())
        .unwrap();
    assert!(!d.is_root_select(&other));
}

#[test]
fn the_passes_are_sqlites_in_javas_order() {
    let names: Vec<&str> = ansi::render_passes(&Sqlite::new()).iter().map(|p| p.name()).collect();
    assert_eq!(
        names,
        vec![
            "FloatPrintText.Expansion",
            "FloatCanonText.Expansion",
            "CarrierStrategies",
            "StarExceptToColumns",
            "SubstringClamp",
            "WholePartitionOrderedSets",
            "ExplosionMarkers.LateralUnnestAsMarker",
            "ExplosionMarkers.SelectListUnnestAsLateral",
            "ExplosionMarkers.StarOverExplosionUnwrapped",
            "QualifyToSubselect",
            "ConstantKeysAsExpressions",
            "CommonSubexpressions",
            "StoredReads",
        ]
    );
    assert!(Sqlite::new().binds_float_text());
}

#[test]
fn the_ddl_spells_sqlites_storage_classes() {
    let d = Sqlite::new();
    let ddl = |t: ColumnType| d.ddl_type(&t).unwrap();
    assert_eq!(ddl(ColumnType::Plain { kind: ColumnKind::Bit }), "INTEGER");
    assert_eq!(ddl(ColumnType::Plain { kind: ColumnKind::Real }), "REAL");
    assert_eq!(ddl(ColumnType::Plain { kind: ColumnKind::TimestampNs }), "TEXT");
    assert_eq!(ddl(ColumnType::Sized { kind: "varbinary".into(), size: 3 }), "BLOB");
    assert_eq!(ddl(ColumnType::Sized { kind: "VARCHAR".into(), size: 3 }), "TEXT");
    assert_eq!(ddl(ColumnType::Scaled { kind: "NUMERIC".into(), precision: 10, scale: 2 }), "REAL");
    assert_eq!(d.scalar_type_name(Scalar::Hugeint).unwrap(), "INTEGER");
    assert_eq!(
        d.scalar_type_name(Scalar::DecimalText).unwrap_err(),
        Refusal::illegal_state("DECIMAL_TEXT cast reached a dialect without DECIMAL_TEXT support")
    );
    assert_eq!(
        d.struct_type_name(&[]).unwrap_err(),
        Refusal::capability("a STRUCT type reached a dialect without struct support")
    );
    assert_eq!(d.array_type_name("INTEGER"), "TEXT");
    assert_eq!(d.map_type_name("TEXT", "INTEGER"), "TEXT");
    assert_eq!(d.lit("a\0b"), "('a' || char(0) || 'b')");
    assert_eq!(d.lit("it's"), "'it''s'");
    assert_eq!(
        d.typeof_name(Scalar::Bigint).unwrap_err(),
        Refusal::capability("typeof reached SQLite: its type names are SQLite's storage classes")
    );
}

#[test]
fn a_replacements_group_reference_is_a_backslash_and_a_java_digit_unit() {
    let replace = |replacement: &str| {
        let e = build::call(SqlFn::RegexpReplace, vec![s(), build::string_lit("a(b)"), build::string_lit(replacement)]);
        let mut w = SqlWriter::new();
        Sqlite::new().expr(&mut w, &e, 0).map(|_| w.into_text().unwrap())
    };
    // a digit is a group number; any script's, its unit less '0' (U+0661 is group 1585, past the pattern's: refused)
    let one = replace("x\\1").unwrap();
    assert!(one.contains("(__rp.f ->> 2)"), "{one}");
    assert!(replace("x\\\u{0661}").unwrap_err().message.contains("no group 1585"));
    // a superscript is no decimal digit: text
    let superscript = replace("x\\\u{00B9}").unwrap();
    assert!(superscript.contains("'x¹'") && !superscript.contains("->> 2"), "{superscript}");
    // a supplementary digit is two units: Java reads the high surrogate after the backslash, no digit, so text
    let bold = replace("\\\u{1D7CF}").unwrap();
    assert!(bold.contains("'\u{1D7CF}'") && !bold.contains("->> 2"), "{bold}");
    // a group past the pattern's is refused by name
    assert!(replace("\\\u{0663}").unwrap_err().message.contains("no group 1587"));
}

#[test]
fn a_lambdas_struct_parameter_is_read_as_json() {
    let st = SqlType::Struct {
        fields: vec![lite_sql::StructField { name: "a".into(), r#type: SqlType::Scalar(Scalar::Bigint) }],
    };
    let read = SqlExpr::Column(Column {
        table: Some("x".into()),
        name: "a".into(),
        r#type: TypeFact::Typed { r#type: SqlType::Scalar(Scalar::Bigint), nullable: true },
        origin: None,
    });
    let l = build::lambda_of(vec!["x".into()], build::call(SqlFn::Greater, vec![read, build::int_lit(1)]));
    let body = lambda_body(&l, Some(&st)).unwrap();
    let SqlExpr::Call { args, .. } = &body else { panic!("a call") };
    assert_eq!(
        args[0],
        build::struct_get_given(
            build::typed_column(None, "x", st, true, Origin::Derived),
            "a".into(),
            TypeFact::Typed { r#type: SqlType::Scalar(Scalar::Bigint), nullable: true }
        )
    );
    let mut w = SqlWriter::new();
    Sqlite::new().expr(&mut w, &body, 0).unwrap();
    assert_eq!(w.into_text().unwrap(), "(x ->> 'a') > 1");
}
