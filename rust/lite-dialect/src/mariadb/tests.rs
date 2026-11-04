// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! MariaDB's and MySQL's hooks held to their Java (`MariaDb.java`, `MySql.java`) where the golden oracles cannot reach
//! them: the helpers' defaults their one caller never passes (`bitOp`'s, `lists`', `maps`'), the cast type names no
//! cast reaches (`scalarTypeName`'s JSON, TEMPORAL_TEXT, TIME and DECIMAL_TEXT, `structTypeName`), the non-finite Float
//! a literal cannot hold, MySQL's seven answers behind MariaDB's `!correlatesDerived()` walls, the family's hooks one
//! by one, the facts and the wire, a root select's delivered values (MySQL's JSON as text) beside a nested one's, a
//! MODE or a ranked quantile no count reached, Java's `ConstantKeysAsExpressions` naming Postgres, the pass lists, and
//! the two passes only this family installs, shape by shape. The oracles hold the rest
//! (`tests/dialect_differential.rs`, `mir/dialect-arms.jsonl`).

use super::passes_family::{WindowAggregatesAsCorrelated, WindowsComputedBelow};
use super::*;
use crate::SqlDialect;
use crate::build::{call, int_lit, string_lit};
use crate::rewriter::{SqlRewriter, rewrite};
use lite_sql::{Bound, Column, DatePart, FrameKind, JavaNumber};

// ---- builders

fn t(s: Scalar) -> SqlType {
    SqlType::Scalar(s)
}

fn out(name: &str, ty: SqlType) -> OutputCol {
    OutputCol { name: name.into(), r#type: ty, nullable: true, origin: Origin::Physical }
}

fn table(alias: &str, outputs: Vec<OutputCol>) -> SqlSource {
    SqlSource::Table {
        name: "t".into(),
        alias: alias.into(),
        outputs,
        call: false,
        stored_types: Vec::new(),
        quoting: NameQuoting::default(),
    }
}

/// `t AS t0 (k VARCHAR, n BIGINT, s VARCHAR, xs BIGINT[])`.
fn t0() -> SqlSource {
    table(
        "t0",
        vec![
            out("k", t(Scalar::Varchar)),
            out("n", t(Scalar::Bigint)),
            out("s", t(Scalar::Varchar)),
            out("xs", SqlType::Array { element: Box::new(t(Scalar::Bigint)) }),
        ],
    )
}

fn col(name: &str, ty: SqlType) -> SqlExpr {
    SqlExpr::Column(Column {
        table: Some("t0".into()),
        name: name.into(),
        r#type: TypeFact::Typed { r#type: ty, nullable: true },
        origin: Some(Origin::Physical),
    })
}

fn k() -> SqlExpr {
    col("k", t(Scalar::Varchar))
}

fn n() -> SqlExpr {
    col("n", t(Scalar::Bigint))
}

fn s() -> SqlExpr {
    col("s", t(Scalar::Varchar))
}

fn xs() -> SqlExpr {
    col("xs", SqlType::Array { element: Box::new(t(Scalar::Bigint)) })
}

fn proj(e: SqlExpr, alias: &str) -> Projection {
    let ty = match e.r#type() {
        TypeFact::Typed { r#type, .. } => r#type.clone(),
        _ => t(Scalar::Varchar),
    };
    build::projection(e, Some(alias.into()), Some(build::output_col(alias, ty, true))).unwrap()
}

fn select(projections: Vec<Projection>, from: SqlSource) -> SqlSelect {
    build::select_of(projections, from, Vec::new()).unwrap()
}

fn expr_result(d: &MariaDb, e: &SqlExpr) -> Result<String, Refusal> {
    let mut w = SqlWriter::new();
    d.expr(&mut w, e, 0)?;
    w.into_text()
}

fn expr_text(d: &MariaDb, e: &SqlExpr) -> String {
    expr_result(d, e).unwrap_or_else(|r| panic!("{r}"))
}

fn piece_text(p: P<'_>) -> String {
    let mut w = SqlWriter::new();
    w.piece(&*p).unwrap();
    w.into_text().unwrap()
}

fn refused(r: Result<impl std::fmt::Debug, Refusal>) -> (&'static str, String) {
    let r = r.expect_err("a refusal");
    (r.class, r.message)
}

fn window(f: SqlAgg, partition_by: Vec<SqlExpr>, order_by: Vec<SortKey>, frame: Option<Frame>) -> SqlExpr {
    build::window_call(f, partition_by, order_by, frame)
}

fn string_agg(x: SqlExpr) -> SqlAgg {
    SqlAgg::Reducer(build::reducer(AggFn::StringAgg, vec![x, string_lit(",")], false, Vec::new()))
}

// ---- the helpers' defaults and the type names no cast reaches

#[test]
fn the_helpers_defaults_no_caller_reaches_answer_as_java() {
    for d in [MariaDb::new(), MariaDb::mysql()] {
        let mut w = SqlWriter::new();
        assert_eq!(
            refused(d.bit_op(&mut w, SqlFn::Plus, &[int_lit(1), int_lit(2)])),
            (Refusal::ILLEGAL_STATE, "not a bit op: PLUS".to_owned())
        );
        let mut w = SqlWriter::new();
        assert_eq!(
            refused(d.lists(&mut w, SqlFn::Abs, &[xs()], &TypeFact::Unknown)),
            (Refusal::ILLEGAL_STATE, "not a list call: ABS".to_owned())
        );
        let mut w = SqlWriter::new();
        assert_eq!(
            refused(d.maps(&mut w, SqlFn::Abs, &[xs()], &TypeFact::Unknown)),
            (Refusal::ILLEGAL_STATE, "not a map call: ABS".to_owned())
        );
        // the bit operators, unsigned here, read back signed
        let mut w = SqlWriter::new();
        d.bit_op(&mut w, SqlFn::BitShiftLeft, &[int_lit(1), int_lit(63)]).unwrap();
        assert_eq!(w.into_text().unwrap(), "CAST(1 << 63 AS SIGNED)");
    }
}

#[test]
fn the_cast_type_names_no_cast_reaches_answer_as_java() {
    // no castTypeName entry left to MariaDB carries these: its own casts are spelled by castTo
    for d in [MariaDb::new(), MariaDb::mysql()] {
        assert_eq!(d.scalar_type_name(Scalar::Json).unwrap(), "CHAR");
        assert_eq!(d.scalar_type_name(Scalar::TemporalText).unwrap(), "CHAR");
        assert_eq!(d.scalar_type_name(Scalar::Time).unwrap(), "TIME(6)");
        assert_eq!(
            refused(d.scalar_type_name(Scalar::DecimalText)),
            (Refusal::ILLEGAL_STATE, "DECIMAL_TEXT cast reached a dialect without DECIMAL_TEXT support".to_owned())
        );
        assert_eq!(
            refused(d.struct_type_name(&[])),
            (Refusal::DIALECT_CAPABILITY, "a STRUCT type reached a dialect without struct support".to_owned())
        );
        assert_eq!(d.array_type_name("BIGINT").unwrap(), "LONGTEXT");
        assert_eq!(d.map_type_name("CHAR", "BIGINT"), "LONGTEXT");
    }
}

#[test]
fn a_float_is_a_double_by_its_exponent_and_never_non_finite() {
    let d = MariaDb::new();
    assert_eq!(d.float_literal(1.5).unwrap(), "1.5E0");
    assert_eq!(d.float_literal(1e20).unwrap(), "1.0E20");
    assert_eq!(d.float_literal(-0.001).unwrap(), "-0.001E0");
    // a FloatLit holds no non-finite value: the wall no oracle line reaches
    assert_eq!(
        refused(d.float_literal(f64::NAN)),
        (Refusal::DIALECT_CAPABILITY, "a NaN literal reached MariaDB, whose DOUBLE has no infinity or NaN".to_owned())
    );
    assert_eq!(
        refused(MariaDb::mysql().float_literal(f64::NEG_INFINITY)),
        (
            Refusal::DIALECT_CAPABILITY,
            "a -Infinity literal reached MySQL, whose DOUBLE has no infinity or NaN".to_owned()
        )
    );
    assert_eq!(
        refused(d.float_literal(f64::INFINITY)).1,
        "a Infinity literal reached MariaDB, whose DOUBLE has no \
                                                             infinity or NaN"
    );
}

// ---- MySQL's answers behind MariaDB's walls (`!correlatesDerived()`)

#[test]
fn mysql_reads_a_row_where_mariadb_refuses_its_recursive_ctes() {
    let (maria, my) = (MariaDb::new(), MariaDb::mysql());
    // a fold over a row's list
    let fold = build::fold_call(
        xs(),
        build::lambda_of(
            vec!["x".into(), "acc".into()],
            call(SqlFn::Plus, vec![build::derived(None, "acc"), build::derived(None, "x")]),
        ),
        int_lit(0),
        false,
        false,
    );
    assert_eq!(
        refused(expr_result(&maria, &fold)),
        (
            Refusal::DIALECT_CAPABILITY,
            "a fold that reads a row reached MariaDB, whose recursive CTE cannot read an outer column (measured: \
             Unknown column)"
                .to_owned()
        )
    );
    assert!(expr_text(&my, &fold).starts_with("(WITH RECURSIVE \"__f\"(\"__i\", \"acc\") AS (SELECT 0, \"__a\".\"a\""));
    // a list reduction over a row's list
    let reduce = call(
        SqlFn::ListReduce,
        vec![
            xs(),
            build::lambda(
                &["acc", "x"],
                call(SqlFn::Plus, vec![build::derived(None, "acc"), build::derived(None, "x")]),
            ),
        ],
    );
    assert_eq!(
        refused(expr_result(&maria, &reduce)).1,
        "LIST_REDUCE reached MariaDB: a reduction that reads a row needs a recursive CTE over an outer column \
         (measured: Unknown column)"
    );
    assert!(expr_text(&my, &reduce).starts_with("(WITH RECURSIVE \"__f\"(\"__i\", \"acc\") AS (SELECT 1,"));
    // an edit distance and a Jaro-Winkler similarity of a row's text
    let distance = call(SqlFn::Levenshtein, vec![s(), string_lit("x")]);
    assert_eq!(
        refused(expr_result(&maria, &distance)).1,
        "LEVENSHTEIN reached MariaDB: an edit distance of texts that read a row needs a recursive CTE over an outer \
         column (measured: Unknown column)"
    );
    assert!(expr_text(&my, &distance).starts_with("(WITH RECURSIVE __lv("));
    let similarity = call(SqlFn::JaroWinkler, vec![s(), string_lit("x")]);
    assert_eq!(
        refused(expr_result(&maria, &similarity)).1,
        "JARO_WINKLER reached MariaDB: a Jaro-Winkler similarity of texts that read a row needs a recursive CTE over \
         an outer column (measured: Unknown column)"
    );
    assert!(expr_text(&my, &similarity).starts_with("(WITH RECURSIVE __jw("));
}

#[test]
fn mysql_joins_laterally_where_mariadb_refuses() {
    let (maria, my) = (MariaDb::new(), MariaDb::mysql());
    let source = |w: &mut SqlWriter, d: &MariaDb, src: &SqlSource| d.source(w, src, 0);
    // a lateral join to no derived table
    let to_table =
        build::join(t0(), table("t1", vec![out("v", t(Scalar::Bigint))]), JoinKind::CrossLateral, None).unwrap();
    let mut w = SqlWriter::new();
    assert_eq!(
        refused(source(&mut w, &maria, &to_table)).1,
        "a lateral join to a Table reached MariaDB, which has no LATERAL"
    );
    let mut w = SqlWriter::new();
    source(&mut w, &my, &to_table).unwrap();
    assert_eq!(w.into_text().unwrap(), "\"t\" AS \"t0\"\nCROSS JOIN LATERAL \"t\" AS \"t1\"");
    // a lateral subquery of no declared column
    let undeclared =
        SqlQuery::Select(Box::new(SqlSelect { outputs: Vec::new(), ..build::star_of(table("t2", vec![])).unwrap() }));
    let lateral = build::join(t0(), build::subselect(undeclared, "l", None), JoinKind::CrossLateral, None).unwrap();
    let mut w = SqlWriter::new();
    assert_eq!(refused(source(&mut w, &maria, &lateral)).1, "a lateral subquery of undeclared columns reached MariaDB");
    let mut w = SqlWriter::new();
    source(&mut w, &my, &lateral).unwrap();
    assert!(w.into_text().unwrap().contains("CROSS JOIN LATERAL ("));
    // a lateral subquery whose projections do not match its outputs (two stars)
    let mut two = select(vec![], table("t2", vec![out("v", t(Scalar::Bigint))]));
    two.projections = vec![
        build::projection(build::star(Some("t2")), None, None).unwrap(),
        build::projection(build::star(Some("t2")), None, None).unwrap(),
    ];
    two.outputs = vec![out("v", t(Scalar::Bigint))];
    let lateral =
        build::join(t0(), build::subselect(build::query(two), "l", None), JoinKind::CrossLateral, None).unwrap();
    let mut w = SqlWriter::new();
    assert_eq!(
        refused(source(&mut w, &maria, &lateral)).1,
        "a lateral subquery whose projections do not match its outputs reached MariaDB"
    );
    let mut w = SqlWriter::new();
    source(&mut w, &my, &lateral).unwrap();
    assert!(w.into_text().unwrap().contains("CROSS JOIN LATERAL ("));
}

// ---- the family's hooks

#[test]
fn the_family_hooks_answer_each_its_own() {
    let (maria, my) = (MariaDb::new(), MariaDb::mysql());
    assert_eq!(piece_text(maria.compact(text("j"))), "JSON_COMPACT(j)");
    assert_eq!(piece_text(my.compact(text("j"))), "CAST(j AS JSON)");
    assert_eq!(
        piece_text(maria.array_agg(true, text("v"), Some(text("o")), true)),
        "JSON_ARRAYAGG(DISTINCT v ORDER BY o LIMIT 1)"
    );
    assert_eq!(piece_text(my.array_agg(false, text("v"), None, true)), "JSON_ARRAYAGG(v)");
    assert_eq!(
        piece_text(my.array_agg(true, text("v"), Some(text("o")), true)),
        "JSON_EXTRACT(CAST(CONCAT('[', GROUP_CONCAT(DISTINCT JSON_ARRAY(v) ORDER BY o SEPARATOR ','), ']') AS JSON), \
         '$[*][0]')"
    );
    assert_eq!(piece_text(maria.index_path(text("i"))), "CONCAT('$[', i, ']')");
    assert_eq!(
        piece_text(my.index_path(text("i"))),
        "(CASE WHEN (i) < 0 THEN CONCAT('$[last-', -(i) - 1, ']') ELSE CONCAT('$[', i, ']') END)"
    );
    assert_eq!((maria.group_ref(2), my.group_ref(2)), ("\\2".to_owned(), "$2".to_owned()));
    assert_eq!((maria.product(), my.product()), ("MariaDB", "MySQL"));
    assert_eq!(piece_text(maria.json_table(text("j"), "S".into(), "__t".into())), "JSON_TABLE(j, S) AS __t");
    assert_eq!(
        piece_text(my.json_table(text("j"), "S".into(), "\"__a\"".into())),
        "(LATERAL (SELECT j AS \"j\") AS \"__a__j\" CROSS JOIN JSON_TABLE(\"__a__j\".\"j\", S) AS \"__a\")"
    );
    assert_eq!(piece_text(maria.json_text(text("j"), text("p"))), "JSON_VALUE(j, p)");
    assert_eq!(
        piece_text(my.json_text(text("j"), text("p"))),
        "(CASE WHEN JSON_TYPE(JSON_EXTRACT(j, p)) = 'NULL' THEN NULL ELSE JSON_UNQUOTE(JSON_EXTRACT(j, p)) END)"
    );
    assert_eq!(maria.json_text_of("j", "p"), "JSON_VALUE(j, p)");
    assert_eq!(my.json_text_of("j", "p"), piece_text(my.json_text(text("j"), text("p"))));
    assert_eq!(piece_text(maria.json_member(text("j"), text("p"))), "NULLIF(JSON_EXTRACT(j, p), 'null')");
    assert_eq!(
        piece_text(my.json_member(text("j"), text("p"))),
        "(CASE WHEN JSON_TYPE(JSON_EXTRACT(j, p)) = 'NULL' THEN NULL ELSE JSON_EXTRACT(j, p) END)"
    );
    assert_eq!((maria.boolean_column(), my.boolean_column()), ("BOOLEAN", "INT"));
    assert_eq!((maria.json_is_binary(), my.json_is_binary()), (false, true));
    assert_eq!((maria.correlates_derived(), my.correlates_derived()), (false, true));
    assert_eq!((maria.window_quantiles_native(), my.window_quantiles_native()), (true, false));
    assert_eq!((maria.grouped_modes_counted(), my.grouped_modes_counted()), (false, true));
    assert_eq!(piece_text(maria.json_as_text(text("j"))), "CAST(j AS CHAR)");
    assert_eq!(
        piece_text(my.json_as_text(text("j"))),
        "REGEXP_REPLACE(CAST(j AS CHAR), CONCAT('', '(\"(?:[^\"', CHAR(92 USING utf8mb4), CHAR(92 USING utf8mb4), \
         ']|', CHAR(92 USING utf8mb4), CHAR(92 USING utf8mb4), '.)*\")|([,:]) '), '$1$2')"
    );
    assert_eq!(
        piece_text(maria.two_rows(text("m"))),
        "(SELECT 1 FROM JSON_TABLE(JSON_ARRAY(@legend_raise := m, 0), '$[*]' COLUMNS (\"x\" LONGTEXT PATH '$')) AS __r)"
    );
    assert_eq!(piece_text(my.two_rows(text("m"))), "(SELECT @legend_raise := m UNION ALL SELECT 0)");
    let date = col("d", t(Scalar::Date));
    assert_eq!(piece_text(maria.year_of(&date)), "YEAR(\"t0\".\"d\")");
    assert_eq!(piece_text(my.year_of(&date)), "CAST(YEAR(\"t0\".\"d\") AS SIGNED)");
}

#[test]
fn the_literals_are_each_databases_own() {
    let (maria, my) = (MariaDb::new(), MariaDb::mysql());
    assert_eq!(maria.string_literal("it's"), "'it''s'");
    assert_eq!(maria.string_literal("a\\b"), "'a\\b'");
    // an odd run of backslashes before a quote: each backslash its own character
    assert_eq!(maria.string_literal("a\\'"), "CONCAT('a', CHAR(92 USING utf8mb4), '''')");
    assert_eq!(maria.string_literal("\\"), "CONCAT('', CHAR(92 USING utf8mb4), '')");
    assert_eq!(my.string_literal("plain"), "'plain'");
    assert_eq!(my.string_literal("a\\'b"), "CONCAT('', 'a', CHAR(92 USING utf8mb4), CHAR(39 USING utf8mb4), 'b')");
    assert_eq!(
        maria.timestamp_lit("2024-01-02T03:04:05.123456").unwrap(),
        "CAST('2024-01-02 03:04:05.123456' AS DATETIME(6))"
    );
    assert_eq!(maria.date_lit("2024-01-02").unwrap(), "DATE '2024-01-02'");
    let big = BigDecimal::parse("9223372036854775898").unwrap();
    assert_eq!(maria.huge_int_literal(&big), "9223372036854775898");
    assert_eq!(my.huge_int_literal(&big), "CAST(9223372036854775898 AS DECIMAL(38,0))");
    // every identifier quoted; a quoted declaration its own spelling
    assert_eq!(maria.ident("name"), "\"name\"");
    assert_eq!(maria.ident("\"Na\"\"me\""), "\"Na\"\"me\"");
    assert_eq!(maria.ident("a\"b"), "\"a\"\"b\"");
}

// ---- the facts and the wire

#[test]
fn the_facts_and_the_wire_answer_as_java() {
    let (maria, my) = (MariaDb::new(), MariaDb::mysql());
    assert_eq!(maria.dialect_name(), "MariaDb");
    assert_eq!(my.dialect_name(), "MySql");
    assert_eq!(maria.session_setup().len(), 7);
    assert_eq!(maria.session_setup()[0], "SET SESSION optimizer_switch = 'condition_pushdown_for_derived=off'");
    assert_eq!(
        maria.session_setup()[1],
        "SET SESSION sql_mode = 'ANSI_QUOTES,PIPES_AS_CONCAT,NO_BACKSLASH_ESCAPES,STRICT_ALL_TABLES,\
         NO_ENGINE_SUBSTITUTION'"
    );
    assert_eq!(maria.session_setup()[5], "SET SESSION max_recursive_iterations = 100000000");
    assert_eq!(my.session_setup().len(), 6);
    assert_eq!(
        my.session_setup()[0],
        "SET SESSION sql_mode = 'ANSI_QUOTES,PIPES_AS_CONCAT,NO_BACKSLASH_ESCAPES,STRICT_ALL_TABLES,\
         NO_ENGINE_SUBSTITUTION,NO_UNSIGNED_SUBTRACTION'"
    );
    assert_eq!(my.session_setup()[4], "SET SESSION cte_max_recursion_depth = 100000000");
    for d in [&maria, &my] {
        assert_eq!(d.keys_snapshot().unwrap(), KeysSnapshot::RepeatableRead);
        assert_eq!(d.result_column_limit().unwrap(), Some(4096));
        assert!(d.needs_static_pivot());
        assert!(d.temporary_tables());
        assert!(d.relational() && !d.in_process() && !d.loads_rows_in_bulk());
        assert_eq!(d.server_versions().unwrap(), None);
        assert!(d.for_server("11.4.13").is_none());
        assert_eq!(d.wire_type_name(Some("LONGTEXT")), "VARCHAR");
        assert_eq!(d.wire_type_name(Some("UUID")), "VARCHAR");
        assert_eq!(d.wire_type_name(Some("DATETIME")), "TIMESTAMP");
        assert_eq!(d.wire_type_name(Some("BIGINT")), crate::wire_names::fold(Some("BIGINT")));
        assert_eq!(d.wire_type_name(None), crate::wire_names::fold(None));
        assert_eq!(d.typing_wire_name(Some("TINYTEXT")), Some("VARCHAR".to_owned()));
        assert!(d.delivers_own(&t(Scalar::Varchar), "NULL"));
        assert!(d.delivers_own(&t(Scalar::Boolean), "TINYINT"));
        assert!(d.delivers_own(&t(Scalar::Bigint), "DECIMAL(65,0)"));
        assert!(d.delivers_own(&t(Scalar::Integer), "BIGINT UNSIGNED"));
        assert!(!d.delivers_own(&t(Scalar::Bigint), "DECIMAL(65,2)"));
        assert!(d.delivers_own(&SqlType::Decimal { precision: 10, scale: 2 }, "DECIMAL(12,2)"));
        assert!(!d.delivers_own(&SqlType::Decimal { precision: 10, scale: 2 }, "DECIMAL(12,3)"));
        assert!(d.delivers_own(&SqlType::Array { element: Box::new(t(Scalar::Bigint)) }, "JSON"));
        assert!(d.delivers_own(&t(Scalar::Json), "VARCHAR"));
        assert!(!d.delivers_own(&t(Scalar::Double), "DECIMAL(10,0)"));
        assert_eq!(d.script(&["a".into(), "b".into()]), "a;\nb;");
        assert_eq!(d.failing_statement("boom", &["a".into()]), None);
        assert_eq!(d.script_abort(), None);
        assert_eq!(
            refused(SqlDialect::from_engine_text(d, "SELECT 1")),
            (
                Refusal::DIALECT_CAPABILITY,
                "the engine's H2 text reached MariaDB, which has no translation of it".to_owned()
            )
        );
        assert_eq!(refused(d.typeof_name(Scalar::Bigint)).1, "typeof reached MariaDB: its type names are DuckDB's");
        assert_eq!(d.hole_type(ValueKind::Date).unwrap(), TypeSpelling::new("DATE", crate::statement::Digits::None));
        assert_eq!(
            d.hole_type(ValueKind::DateTimeNanos).unwrap(),
            TypeSpelling::new("DATETIME(6)", crate::statement::Digits::None)
        );
        assert!(d.hole_type(ValueKind::Integer).is_err());
    }
}

#[test]
fn the_pass_lists_are_javas_in_their_order() {
    let names = |d: &MariaDb| d.passes().iter().map(|p| p.name()).collect::<Vec<_>>();
    let maria = vec![
        "CarrierStrategies",
        "StarExceptToColumns",
        "SubstringClamp",
        "ExplosionMarkers.LateralUnnestAsMarker",
        "ExplosionMarkers.SelectListUnnestAsLateral",
        "ExplosionMarkers.StarOverExplosionUnwrapped",
        "QualifyToSubselect",
        "WindowAggregatesAsCorrelated",
        "ConstantKeysAsExpressions",
    ];
    assert_eq!(names(&MariaDb::new()), maria);
    let mut my = maria.clone();
    my.push("WindowsComputedBelow");
    assert_eq!(names(&MariaDb::mysql()), my);
    // a Float's literal spelling is the dialect's own; its print forms expanded before every pass
    let d = MariaDb::new();
    assert!(d.spells_float_canon() && !d.spells_float_print());
}

// ---- the statement's root: its labels delivered, MySQL's JSON as text

#[test]
fn a_root_select_delivers_its_labels_and_a_nested_one_does_not() {
    // a BIGINT labelled a Float is a DOUBLE at the root, as it is below it
    let root = select(vec![proj(n(), "n")], t0());
    let labelled = SqlQuery::Select(Box::new(SqlSelect {
        outputs: vec![build::output_col("n", t(Scalar::Double), true)],
        ..root.clone()
    }));
    let text = AnsiSqlRenderer::render(&MariaDb::new(), labelled.clone()).unwrap();
    assert_eq!(text, "SELECT CAST(\"t0\".\"n\" AS DOUBLE) AS \"n\"\nFROM \"t\" AS \"t0\"");
    // nested, the same select is no root: its value as it is
    let outer = select(vec![proj(col("n", t(Scalar::Bigint)), "n")], build::subselect(labelled, "q", None));
    let outer = SqlQuery::Select(Box::new(SqlSelect {
        outputs: vec![build::output_col("n", t(Scalar::Bigint), true)],
        ..outer
    }));
    let text = AnsiSqlRenderer::render(&MariaDb::new(), outer).unwrap();
    assert!(text.contains("SELECT \"t0\".\"n\" AS \"n\"\n  FROM"), "{text}");
    // MySQL delivers a root list as its text (its JSON reaches the driver as bytes); MariaDB as it is
    let lists = build::query(select(vec![proj(xs(), "xs")], t0()));
    assert_eq!(
        AnsiSqlRenderer::render(&MariaDb::mysql(), lists.clone()).unwrap(),
        "SELECT CAST(\"t0\".\"xs\" AS CHAR) AS \"xs\"\nFROM \"t\" AS \"t0\""
    );
    assert_eq!(
        AnsiSqlRenderer::render(&MariaDb::new(), lists).unwrap(),
        "SELECT \"t0\".\"xs\" AS \"xs\"\nFROM \"t\" AS \"t0\""
    );
    // nothing outlives a render: the dialect's stacks are empty again
    let d = MariaDb::mysql();
    let _ = AnsiSqlRenderer::render(&d, build::query(select(vec![proj(xs(), "xs")], t0())));
    assert!(d.roots.borrow().is_empty() && d.root_selects.borrow().is_empty() && d.as_text.borrow().is_empty());
    assert!(d.laterals.borrow().is_empty() && d.mode_counts.borrow().is_empty());
}

#[test]
fn a_mode_or_a_quantile_no_count_reached_is_javas_illegal_state() {
    // a windowed MODE in ORDER BY: modesCounted reads only the projections, HAVING and QUALIFY
    let mode =
        window(SqlAgg::Reducer(build::reducer(AggFn::Mode, vec![n()], false, Vec::new())), vec![k()], Vec::new(), None);
    let mut s = select(vec![proj(k(), "k")], t0());
    s.order_by = vec![build::sort_key(mode, true, None, None)];
    assert_eq!(
        refused(AnsiSqlRenderer::render(&MariaDb::new(), build::query(s))),
        (Refusal::ILLEGAL_STATE, "a MODE reached MariaDB's writer uncounted: modesCounted did not run".to_owned())
    );
    // MySQL reads a whole partition's median from the ranks modesCounted adds
    let median = window(
        SqlAgg::Reducer(build::reducer(AggFn::Median, vec![n()], false, Vec::new())),
        vec![k()],
        Vec::new(),
        None,
    );
    let mut s = select(vec![proj(k(), "k")], t0());
    s.order_by = vec![build::sort_key(median, true, None, None)];
    assert_eq!(
        refused(AnsiSqlRenderer::render(&MariaDb::mysql(), build::query(s))),
        (
            Refusal::ILLEGAL_STATE,
            "a windowed MEDIAN reached MySQL's writer unranked: modesCounted did not run".to_owned()
        )
    );
}

#[test]
fn a_constant_key_is_refused_naming_postgres_as_javas_pass_does() {
    // Java's MariaDb installs `new ConstantKeysAsExpressions()`, whose refusals name Postgres
    let mut s = select(vec![proj(k(), "k")], t0());
    s.order_by = vec![build::sort_key(build::null_lit(), true, None, None)];
    assert_eq!(
        refused(AnsiSqlRenderer::render(&MariaDb::new(), build::query(s))),
        (
            Refusal::DIALECT_CAPABILITY,
            "a NULL ORDER BY key reached Postgres, which refuses a constant key and has no type to cast it to"
                .to_owned()
        )
    );
}

#[test]
fn the_day_offsets_of_a_range_frame_are_javas_double_text() {
    let d = MariaDb::new();
    assert_eq!(d.interval_days(2, "days", true), Ok("2.0".to_owned()));
    assert_eq!(d.interval_days(36, "HOURS", true), Ok("1.5".to_owned()));
    assert_eq!(d.interval_days(1, "MILLISECONDS", true), Ok("1.1574074074074074E-8".to_owned()));
    assert_eq!(
        d.interval_days(1, "MONTHS", true),
        Err(Refusal::capability("a MONTHS window frame offset over this key reached MariaDB"))
    );
    // an interval offset over a key that is no date (the frame has no ORDER BY): refused whatever its unit
    assert_eq!(
        d.interval_days(1, "DAYS", false),
        Err(Refusal::capability("a DAYS window frame offset over this key reached MariaDB"))
    );
}

// ---- WindowAggregatesAsCorrelated

/// The rewritten select's FROM (a subselect over the numbered rows) and its first projection's value.
fn numbered(q: &SqlQuery) -> (&SqlSelect, &SqlExpr) {
    let SqlQuery::Select(s) = q else { panic!("a select") };
    let SqlSource::Subselect { inner, alias, .. } = &s.from else { panic!("a subselect FROM: {:?}", s.from) };
    assert_eq!(alias, "t0");
    let SqlQuery::Select(rows) = inner.as_ref() else { panic!("a select of rows") };
    (rows, &s.projections[1].expr)
}

fn names(s: &SqlSelect) -> Vec<String> {
    s.projections.iter().map(|p| p.alias.clone().unwrap_or_default()).collect()
}

fn rewritten(pass: &mut dyn SqlRewriter, q: SqlQuery) -> SqlQuery {
    pass.rewrite_root(q).unwrap()
}

#[test]
fn a_windowed_string_agg_over_rows_is_the_aggregate_of_its_frame() {
    let frame =
        Frame { kind: FrameKind::Rows, from: Bound::Preceding { n: JavaNumber::Integer(1) }, to: Bound::CurrentRow };
    let w = window(string_agg(s()), vec![k()], vec![build::sort_key(n(), true, None, None)], Some(frame));
    let q = build::query(select(vec![proj(k(), "k"), proj(w, "joined")], t0()));
    let mut pass = WindowAggregatesAsCorrelated::new(&[AggFn::StringAgg]);
    let out = rewritten(&mut pass, q);
    let (rows, joined) = numbered(&out);
    assert_eq!(names(rows), ["k", "n", "s", "xs", "__wa0c0p0", "__wa0c0a0", "__wa0c0n"]);
    // the position: ROW_NUMBER over the partition in the window's order, ties broken by every column of the FROM
    let SqlExpr::WindowCall { order_by, .. } = &rows.projections[6].expr else { panic!("a window") };
    assert_eq!(order_by.len(), 5);
    let SqlExpr::ScalarSubquery { subquery, .. } = joined else { panic!("a scalar subquery: {joined:?}") };
    let d = MariaDb::new();
    let mut w = SqlWriter::new();
    d.query(&mut w, subquery, 1).unwrap();
    let sql = w.into_text().unwrap();
    assert!(sql.contains(
        "WHERE (\"__wa0b\".\"__wa0c0p0\" <=> \"t0\".\"__wa0c0p0\") AND \"__wa0b\".\"__wa0c0n\" >= \
         \"t0\".\"__wa0c0n\" - 1 AND \"__wa0b\".\"__wa0c0n\" <= \"t0\".\"__wa0c0n\""
    ), "{sql}");
    // the joined text keeps the window's order, its separator a literal
    assert!(sql.starts_with("SELECT GROUP_CONCAT(CAST(\"__wa0b\".\"__wa0c0a0\" AS CHAR) ORDER BY"), "{sql}");
    assert!(sql.contains(" SEPARATOR ',')"), "{sql}");
}

#[test]
fn an_ordered_windows_default_frame_reads_the_ranks() {
    // an ordered window without a frame: RANGE UNBOUNDED PRECEDING to CURRENT ROW, its peers by their rank
    let w = window(string_agg(s()), Vec::new(), vec![build::sort_key(n(), true, None, None)], None);
    let q = build::query(select(vec![proj(k(), "k"), proj(w, "joined")], t0()));
    let mut pass = WindowAggregatesAsCorrelated::new(&[AggFn::StringAgg]);
    let out = rewritten(&mut pass, q);
    let (rows, joined) = numbered(&out);
    assert_eq!(names(rows), ["k", "n", "s", "xs", "__wa0c0a0", "__wa0c0n", "__wa0c0k"]);
    let SqlExpr::ScalarSubquery { subquery, .. } = joined else { panic!("a scalar subquery") };
    let SqlQuery::Select(frame) = subquery.as_ref() else { panic!("a select") };
    assert_eq!(
        frame.r#where,
        Some(call(
            SqlFn::LessEqual,
            vec![build::derived(Some("__wa0b"), "__wa0c0k"), build::derived(Some("t0"), "__wa0c0k")]
        ))
    );
}

#[test]
fn a_window_aggregate_the_pass_cannot_write_is_left_as_it_is() {
    let mut pass = WindowAggregatesAsCorrelated::new(&[AggFn::StringAgg]);
    // a RANGE frame with offsets
    let offsets =
        Frame { kind: FrameKind::Range, from: Bound::Preceding { n: JavaNumber::Integer(1) }, to: Bound::CurrentRow };
    let ranged = window(string_agg(s()), Vec::new(), vec![build::sort_key(n(), true, None, None)], Some(offsets));
    let q = build::query(select(vec![proj(ranged, "joined")], t0()));
    assert_eq!(rewrite(&mut pass, q.clone()).unwrap(), crate::rewriter::Rw::Same(q));
    // an aggregate with its own order
    let ordered = SqlAgg::Reducer(build::reducer(
        AggFn::StringAgg,
        vec![s(), string_lit(",")],
        false,
        vec![build::sort_key(n(), true, None, None)],
    ));
    let q = build::query(select(vec![proj(window(ordered, Vec::new(), Vec::new(), None), "joined")], t0()));
    assert_eq!(rewrite(&mut pass, q.clone()).unwrap(), crate::rewriter::Rw::Same(q));
    // a select that groups
    let mut grouped =
        select(vec![proj(k(), "k"), proj(window(string_agg(s()), Vec::new(), Vec::new(), None), "j")], t0());
    grouped.group_by = vec![k()];
    let q = build::query(grouped);
    assert_eq!(rewrite(&mut pass, q.clone()).unwrap(), crate::rewriter::Rw::Same(q));
    // an aggregate the dialect has a window form of
    let summed = SqlAgg::Reducer(build::reducer(AggFn::Sum, vec![n()], false, Vec::new()));
    let q = build::query(select(vec![proj(window(summed, Vec::new(), Vec::new(), None), "total")], t0()));
    assert_eq!(rewrite(&mut pass, q.clone()).unwrap(), crate::rewriter::Rw::Same(q));
}

#[test]
fn the_helper_columns_are_numbered_per_statement() {
    let w = || window(string_agg(s()), Vec::new(), Vec::new(), None);
    // two equal calls in one select are one helper; a second select is the next stem
    let inner = SqlQuery::Select(Box::new(select(vec![proj(k(), "k"), proj(w(), "a"), proj(w(), "b")], t0())));
    let outer = select(
        vec![proj(col("k", t(Scalar::Varchar)), "k"), proj(window(string_agg(k()), Vec::new(), Vec::new(), None), "c")],
        build::subselect(inner, "t0", None),
    );
    let mut pass = WindowAggregatesAsCorrelated::new(&[AggFn::StringAgg]);
    let first = rewritten(&mut pass, build::query(outer.clone()));
    let text = format!("{first:?}");
    assert!(text.contains("__wa0c0a0") && text.contains("__wa1c0a0") && !text.contains("__wa0c1"), "{text}");
    // the next statement starts the count over
    let second = rewritten(&mut pass, build::query(outer));
    assert_eq!(first, second);
}

// ---- WindowsComputedBelow

#[test]
fn a_window_inside_an_expression_is_computed_below() {
    let lead = || {
        window(
            SqlAgg::ValueFn { r#fn: AggFn::Lead, args: vec![n()] },
            vec![k()],
            vec![build::sort_key(n(), true, None, None)],
            None,
        )
    };
    let guarded = build::case(
        vec![build::when(call(SqlFn::IsNull, vec![lead()]), build::null_lit())],
        Some(call(SqlFn::Plus, vec![lead(), int_lit(1)])),
    );
    let mut s = select(vec![proj(k(), "k"), proj(guarded, "next")], t0());
    s.order_by = vec![build::sort_key(lead(), true, None, None)];
    let mut pass = WindowsComputedBelow::default();
    let out = rewritten(&mut pass, build::query(s));
    let SqlQuery::Select(top) = &out else { panic!("a select") };
    let SqlSource::Subselect { inner, alias, .. } = &top.from else { panic!("a subselect") };
    assert_eq!(alias, "t0");
    let SqlQuery::Select(below) = inner.as_ref() else { panic!("a select below") };
    // the one window, once
    assert_eq!(names(below), ["k", "n", "s", "xs", "__wb0w0"]);
    let read = SqlExpr::Column(build::column_of(Some("t0"), &build::output_col("__wb0w0", t(Scalar::Bigint), true)));
    assert_eq!(top.order_by[0].expr, read);
    assert!(format!("{:?}", top.projections[1].expr).contains("__wb0w0"));
    // a bare window projection is left to the select (its own CASE guards nothing)
    let bare = build::query(select(
        vec![proj(window(SqlAgg::RankingFn { r#fn: AggFn::RowNumber, args: vec![] }, vec![], vec![], None), "rn")],
        t0(),
    ));
    assert_eq!(rewrite(&mut WindowsComputedBelow::default(), bare.clone()).unwrap(), crate::rewriter::Rw::Same(bare));
}

#[test]
fn an_extract_and_a_trunc_part_answer_as_java() {
    // the part DuckDB has no MariaDB spelling of, and the date-truncation parts it refuses
    let d = MariaDb::new();
    let part = |p: DatePart| SqlExpr::DatePartLit { part: p, r#type: TypeFact::Unknown };
    let date = col("d", t(Scalar::Date));
    assert_eq!(
        refused(expr_result(&d, &call(SqlFn::Extract, vec![part(DatePart::Nanosecond), date.clone()]))).1,
        "date part 'nanosecond' has no measured MariaDB spelling"
    );
    assert_eq!(
        refused(expr_result(&d, &call(SqlFn::DateTrunc, vec![part(DatePart::DayOfYear), date.clone()]))).1,
        "date_trunc part 'day_of_year' has no measured MariaDB spelling"
    );
    assert_eq!(expr_text(&d, &call(SqlFn::Extract, vec![part(DatePart::Year), date.clone()])), "YEAR(\"t0\".\"d\")");
    assert_eq!(
        expr_text(&MariaDb::mysql(), &call(SqlFn::Extract, vec![part(DatePart::Year), date])),
        "CAST(YEAR(\"t0\".\"d\") AS SIGNED)"
    );
}

#[test]
fn printf_is_sformat_on_mariadb_and_pieces_on_mysql() {
    let f = |fmt: &str, args: Vec<SqlExpr>| {
        let mut a = vec![string_lit(fmt)];
        a.extend(args);
        call(SqlFn::Format, a)
    };
    let maria = MariaDb::new();
    assert_eq!(
        expr_text(&maria, &f("{%-5d}%%%s", vec![n(), s()])),
        "SFORMAT('{{{:<5d}}}%{}', \"t0\".\"n\", CAST(\"t0\".\"s\" AS CHAR))"
    );
    assert_eq!(refused(expr_result(&maria, &f("%q", vec![n()]))).1, "printf conversion %q reached MariaDB");
    let my = MariaDb::mysql();
    assert_eq!(expr_text(&my, &f("a%xb", vec![n()])), "CONCAT('a', LOWER(CONV(\"t0\".\"n\", 10, 16)), 'b')");
    assert_eq!(
        refused(expr_result(&my, &f("%d%d", vec![n()]))).1,
        "a format with more conversions than arguments reached MySQL"
    );
    assert_eq!(refused(expr_result(&my, &f("%e", vec![n()]))).1, "printf conversion %e reached MySQL");
}

#[test]
fn the_regular_expressions_replace_as_each_database_names_a_group() {
    let replace = |r: &str, options: Option<&str>| {
        let mut a = vec![s(), string_lit("(a)"), string_lit(r)];
        a.extend(options.map(string_lit));
        call(SqlFn::RegexpReplace, a)
    };
    let maria = MariaDb::new();
    assert_eq!(
        expr_text(&maria, &replace("<\\1>", None)),
        "REGEXP_REPLACE(\"t0\".\"s\", '^([\\s\\S]*?)((a))', '\\1<\\3>')"
    );
    assert_eq!(expr_text(&maria, &replace("x", Some("gi"))), "REGEXP_REPLACE(\"t0\".\"s\", '(?i)(a)', 'x')");
    assert_eq!(
        refused(expr_result(&maria, &replace("\\8", None))).1,
        "regexp_replace of group 8 (first match only) reached MariaDB"
    );
    assert_eq!(refused(expr_result(&maria, &replace("x", Some("z")))).1, "regexp_replace option 'z' reached MariaDB");
    let my = MariaDb::mysql();
    assert_eq!(
        expr_text(&my, &replace("$\\1", Some("g"))),
        "REGEXP_REPLACE(\"t0\".\"s\", '(a)', CONCAT('', CHAR(92 USING utf8mb4), '$$1'), 1, 0)"
    );
    assert_eq!(super::functions::capture_groups("(a)(?:b)(?<n>c)[(]\\(d"), 2);
    assert_eq!(super::functions::icu_replacement("\\1\\x$\\"), "$1\\x\\$\\\\");
}
